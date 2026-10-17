# Particle effects (ParticleEmitter)

Smoke, steam, fire, sparks, embers, dust and debris: textured, blended,
animated particles that render in Studio (Edit and Play) and in the Player,
from one simulation crate and one renderer.

This is the visual-effects path. Physical particles (fluids, electrons) are
`ParticleSimulation`, documented in `PARTICLE_SIMULATION.md`; the two share
nothing but the word "particle".

## Where it stands today (2026-10-16)

| Surface | State |
| --- | --- |
| Studio, Edit | `spawners/audio_vfx/particle_emitter.rs` is a stub: a 0.1 m white quad marks the emitter, no particles. |
| Studio Play and the Player | `play-runtime/src/particles.rs` simulates on the CPU (cap 6,000 for the whole session) and `particle_render.rs` draws each particle as an opaque, depth-writing sphere. `Texture` is ignored, `Transparency` shrinks a particle instead of fading it, nothing is additive. |
| Data | `ParticleEmitter` round-trips every property through TOML and the Fjall store. The Rust class (`common/src/classes.rs`) and the Play DataModel (Roblox property names) both carry it. |

So the class exists and saves, and nothing on screen looks like smoke or fire.

## Goals

1. **Looks.** Textured billboards with alpha, additive and premultiplied
   blending; flipbooks; rotation; colour, size, transparency and squash over
   life; velocity-stretched streaks for sparks; lit smoke; emissive fire that
   blooms in HDR.
2. **Scale.** Hundreds of thousands of particles on the CPU path now, millions
   on a GPU path later, with a session budget so no Space can stall a frame.
3. **One path.** The same simulation and renderer in Studio Edit (live
   preview, as authors expect), Studio Play and the Player, desktop and web
   (WebGPU and WebGL2).
4. **Compatible.** Imported Roblox emitters (`Lifetime`, `Size`, `Transparency`,
   `Color`, `LightEmission`, `LightInfluence`, `Brightness`, `Orientation`,
   `Squash`, `Flipbook*`, `Shape*`, `EmissionDirection`, `SpreadAngle`,
   `Rotation`, `RotSpeed`, `Acceleration`, `Drag`, `LockedToPart`,
   `VelocityInheritance`, `TimeScale`, `ZOffset`) look the way they did there.

## Architecture

```
ParticleEmitter (class or DataModel record)
        |  EffectDesc::from_class / from_record   (common glue, play-runtime glue)
        v
eustress-vfx (pure crate, opt-level 3 in dev)
  EffectDesc  ->  EmitterSim::step(dt, frame)  ->  write_instances(camera) -> [BillboardInstance]
        |
        v
common realism/vfx renderer (Bevy render world)
  one Transparent3d item per emitter, instanced quads, blend-mode pipelines,
  flipbook atlas, built-in procedural textures
```

### `eustress-vfx` (new crate, `crates/vfx`)

Pure Rust: `bevy_math`, `rayon`, `bytemuck`. No Bevy app, no render code, so
it compiles at opt-level 3 in dev builds (`.cargo/config.toml`, the same trap
`eustress-particle-sim` and `eustress-ocean` avoid) and its tests run in
seconds.

- **`EffectDesc`**: everything an emitter needs, a superset of the Rust class
  and the Roblox properties: rate, bursts, lifetime range, speed range, spread,
  shape (point, sphere, box, cylinder, cone, disc, ring; volume or surface;
  outward, inward or both), emission direction, acceleration, gravity scale,
  drag, wind influence, curl-noise turbulence, collision with a ground plane
  (bounce, friction, kill), rotation and spin, size, transparency, squash and
  colour sequences (Roblox `NumberSequence` envelopes included), flipbook
  layout, mode (loop, one-shot, ping-pong, random) and frame rate, orientation
  (camera-facing, camera-facing world-up, velocity-parallel,
  velocity-perpendicular), light emission, light influence, brightness,
  time scale, locked-to-emitter, Z offset.
- **`EmitterSim`**: structure-of-arrays state (positions, velocities, age,
  life, rotation, spin, seeds). Emission is frame-rate independent (fractional
  particles carried between steps). Steps run in parallel with rayon above a
  threshold. Deterministic for a given seed and step sequence.
- **`write_instances`**: fills `BillboardInstance` (64 bytes, `Pod`) per live
  particle, in world space, sorted back to front for alpha blending (radix
  sort on quantised depth, so 200,000 particles sort in about a millisecond)
  and unsorted for additive.
- **Budget**: a session-wide `VfxBudget` caps live particles (default 250,000)
  and scales each emitter's rate down by distance and screen size before it
  hits the cap, so the cap is a backstop, not the usual state.
- **Presets**: smoke, steam, fire, embers, sparks, debris, dust, magic,
  explosion. Each is an `EffectDesc` plus a built-in texture name. Studio's
  Insert menu and the MCP `create_instance` can start from one.

### Renderer (`common/src/realism/vfx/`)

A custom pipeline in the style of the existing particle-cloud and billboard
pipelines (shader baked with `include_str!`, pipeline keyed on each view's MSAA
and target format, `RenderLayers` so the Slint overlay camera draws nothing).

- **Per-instance vertex buffer, not a storage buffer**, so it runs on WebGL2
  (the existing sphere renderer needs vertex storage and skips WebGL2).
- **One draw per emitter** in `Transparent3d`, sorted by emitter centre; per
  particle order comes from the CPU sort.
- **Pipelines per blend mode**: alpha, additive, premultiplied, multiply.
  Depth test on, depth write off.
- **Vertex shader** expands each instance into a quad: camera-facing from the
  view's right and up vectors, or stretched along the velocity axis for
  streaks, rotated, offset toward the camera by `ZOffset`.
- **Fragment**: texture times colour, flipbook frame from the atlas grid,
  emissive brightness above 1.0 for HDR bloom, and a cheap lit term for smoke
  (half-Lambert on a normal bent outward from the quad centre, against the
  sun direction and ambient colour), scaled by `LightInfluence`.
- **Built-in textures**, generated procedurally at startup, so effects look
  right with no asset: `soft` (gaussian puff), `smoke` (noisy puff), `flame`
  (4x4 flipbook), `spark` (hot core streak), `glow`, `debris` (irregular
  chunk). A `Texture` set to an asset resolves through the existing asset
  path; while it loads, the built-in for the preset draws instead.
- **Light emission**: emitters with `light_emission` add one point light at
  the emitter centre (intensity from the live particle count), not one light
  per particle.

### Wiring

| Surface | How it reaches the renderer |
| --- | --- |
| Studio Edit | The spawner attaches the simulation to the emitter entity; the preview runs live, as authors expect, and stops when the entity is hidden or Play starts. |
| Studio Play and the Player | `play-runtime/src/particles.rs` maps each DataModel emitter to an `EffectDesc` and feeds the same renderer; the sphere path stays for `ParticleSimulation` clouds only. |
| Scripts | `Emit(n)` and `Clear()` reach `EmitterSim::burst` and `clear`. |

## Phases

1. **Textured and blended (this phase).** `eustress-vfx`, the renderer,
   built-in textures, presets, Studio Edit preview, Play and Player switch-over.
2. **Soft particles and lighting.** Depth-faded edges where particles meet
   geometry (needs the view's depth texture), shadow-receiving smoke, light
   probes for `LightInfluence`.
3. **GPU simulation.** A compute path for emitters above about 50,000
   particles on desktop and WebGPU (spawn, update, compaction and indirect
   draw on the GPU), the same `EffectDesc`, CPU path kept for WebGL2. This is
   the shared prerequisite with the ocean's GPU phase. `bevy_hanabi` 0.19.0 is
   compatible with Bevy 0.19 and was considered: it needs compute (no WebGL2),
   adds rand 0.10 to the graph, and its effect graph would be a second
   authoring model beside the class properties, so the GPU phase extends this
   renderer instead.
4. **Mesh particles and trails.** Debris as instanced meshes with tumbling,
   ribbon trails behind sparks and projectiles.

## Acceptance

- Unit tests in `eustress-vfx`: emission count over 1 s matches `Rate` within
  one particle at 30, 60 and 240 Hz; lifetimes stay inside the range; sequence
  sampling matches hand values including envelopes; flipbook frames for each
  mode; back-to-front order; the budget caps live particles; one seed gives one
  result.
- A benchmark binary (not a test) reports step and sort time for 10,000,
  100,000 and 250,000 particles at opt-level 3.
- In Studio: each preset inserted into an empty Space renders in Edit within
  one second, with the look its name promises, at 60 FPS or better on the dev
  machine with 250,000 live particles across emitters.
- The same Space in the Player renders the same effects.
- An imported Roblox place with fire and smoke emitters renders textured,
  faded and additive where the source was.

## Ownership

| Area | Owner |
| --- | --- |
| `crates/vfx/**`, `common/src/realism/vfx/**`, `engine/src/spawners/audio_vfx/particle_emitter.rs` | Particle Simulations |
| `play-runtime/src/particles.rs`, `particle_render.rs` | Client (the switch-over lands as Client's change, from a patch Particle Simulations supplies) |

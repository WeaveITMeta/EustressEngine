# Terrain Ocean

The Terrain's water is a live, physically based ocean, set entirely by the
Terrain's own properties. There is no separate water object: select the
Terrain, choose a sea state, and the whole map's sea moves, foams, shoals on
its beaches and floats what is dropped in it.

One object, several solvers, each where it holds:

| Scale | Solver | Where it applies |
|---|---|---|
| Open sea, kilometres | Spectral waves (FFT, Tessendorf) | Everywhere the Terrain's sea is deep enough |
| Coast, rivers, lakes, floods | Shallow-water equations on a 2D grid (phase 3) | A window around the camera and active areas |
| Splashes, spray, waterfalls | The particle solver, `eustress-particle-sim` (phase 4) | Only where the surface breaks or a Part hits it |

## Why the ocean is not one particle simulation

Terrain Benchmark is 10 km by 10 km. One particle per cubic metre of only the
top metre of sea is 100 million particles; splash-quality spacing (10 cm) is
100 billion. The particle solver runs thousands of particles a frame on the
CPU and GPU SPH reaches a few million. Particle cost grows with the volume of
water; a spectral ocean costs the same per frame whatever its size, and
particles are spent only where water actually breaks.

## Terrain properties

The Terrain class already carries `WaterColor`, `WaterTransparency` and
`WaterWaveSize`. They keep their meaning and are joined by the sea state:

| Property | Type | Default | Meaning |
|---|---|---|---|
| WaterWaves | Choice: Still, Calm, Breeze, Choppy, Rough, Storm | Breeze | The sea state. Sets WaterWindSpeed; Still is a flat sea |
| WaterWindSpeed | m/s | 7 (from WaterWaves) | Wind 10 m above the sea. Typing a value makes the sea state Custom |
| WaterWindDirection | degrees, compass, wind blows from | 225 | Where the wind comes from; waves run downwind |
| WaterSwell | 0 to 1 | 0.3 | Long, regular waves from distant storms, about 2 m at 1 |
| WaterChoppiness | 0 to 2 | 1 | Crest sharpness; 1 is physical |
| WaterFoam | 0 to 1 | 1 | Whitecap and shore foam strength |
| WaterDetail | Choice: Low, Medium, High | Medium | Wave resolution per tile: 64, 128 or 256 |
| WaterBuoyancy | bool | true | Unanchored Parts float, bob and right themselves |
| WaterDensity | kg/m³ | 1025 | Seawater; buoyancy uses it |
| WaterColor | Color | existing | Deep water tint |
| WaterTransparency | 0 to 1 | existing | Deep water clarity |
| WaterWaveSize | 0 to 1 | existing | Fine ripple size on still water (lakes, rivers, imported places) |

`_terrain.toml` `[water]` keeps `enabled` and `sea_level`. The ocean moves
when `enabled` is on and WaterWaves is above Still.

## What the Terrain works out for itself

- **Wave height follows the sea state.** A fully developed sea has
  significant height Hs = 0.21 U² / g (Pierson-Moskowitz): Calm about 0.2 m,
  Breeze 1.0 m, Choppy 2.6 m, Rough 5.5 m, Storm 12 m at Earth gravity.
- **Gravity comes from Workspace.gravity.** On a low-gravity world the same
  wind raises taller, slower waves, since wave speed and height both follow g.
- **Tile sizes come from the sea state.** Three wave tiles, the largest about
  eight peak wavelengths, the others smaller by a non-integer ratio so no
  repetition shows from a beach or from 2 km up.
- **Shoaling is automatic.** Waves shrink as the water shallows, using the
  terrain height under the sea; they never cut through land, and the shore
  foam band grows with the waves.
- **Whitecaps form where crests fold** (the surface Jacobian drops below 1),
  so they appear with the wind rather than being painted on.
- **The same Space gives the same sea.** Wave phases derive from the Space's
  `GlobalRngSeed`.
- **The world clock decides.** In Edit the surface moves on real time, as a
  view, the way ripples already drift. In Play it follows the physics clock,
  so buoyancy replays exactly; Pause freezes it and Stop restores the
  authored properties.

## Architecture

- **`eustress-ocean`** (`crates/ocean`): a pure crate, rayon only, compiled
  optimised in dev builds like `eustress-particle-sim`. JONSWAP and
  Pierson-Moskowitz spectra with directional spreading and depth-aware
  dispersion, a deterministic radix-2 FFT, three cascades with disjoint
  wavenumber bands, and CPU samplers: displacement, height (inverting the
  horizontal displacement) and vertical velocity.
- **Ocean runtime** (common, so Studio, the Player and a server share it):
  reads the Terrain's properties, steps the ocean once a frame on a worker
  task, uploads six textures (per cascade: displacement, and slopes with the
  Jacobian) and holds the CPU sampler for physics.
- **Ocean renderer** (common): the open sea on a camera-following clipmap,
  reusing the terrain far field's shared meshes and level transforms
  (`far_field_level_mesh`, `far_field_level_transform`). A material extension
  adds a vertex shader that displaces by the cascades and fades with depth; the
  fragment shader continues the water look (depth tint, shore foam, pixels
  over land dropped), with normals from the slopes, whitecaps from the
  Jacobian, and Bevy's PBR for sun glints and sky reflections. It replaces the
  flat ocean plane while the sea is moving.
- **Buoyancy** (Play only): for each unanchored Part touching the sea, the
  surface is sampled at points over its collider; each submerged point gets
  ρ g V of lift, which also rights the Part, and drag against the water's
  vertical velocity, through Avian forces.

## Budget (estimates until measured with the microprofiler)

- Medium: 3 tiles of 128², four complex transforms each, about 1 ms a frame
  on a worker thread; textures about 1.5 MB a frame.
- The mesh is a fixed clipmap, so cost does not grow with the map.
- Buoyancy: a handful of samples per floating Part.

## Phases

1. Moving ocean, Terrain properties and buoyancy.
2. Caustics, refraction and underwater fog.
3. Live shallow water for rivers, lakes and the coast: rain, river discharge
   from the existing flow accumulation, flooding, wakes.
4. Splash particles: the particle solver spawned where crests break or Parts
   hit the water, folded back into the surface as it settles.
5. The first wired GPU compute pipeline; the FFT, shallow water and SPH move
   onto it for higher resolution and millions of splash particles.

## Ownership (proposed)

- **Particle Simulations:** `crates/ocean/**`, the ocean runtime's stepping
  and sampler, buoyancy, and the splash coupling.
- **Terrain:** the properties on the Terrain class and their TOML, the ocean
  renderer and its shader, retiring the flat plane while the sea moves, and
  drawing in Studio and the Player.
- **Shared lines** (workspace member, `.cargo/config.toml` optimisation entry,
  common's dependency): landed on the orchestrator's go.

## Acceptance in Terrain Benchmark

- Select the Terrain and set WaterWaves to Choppy: waves of about 2.5 m
  significant height cross the 10 km sea, shrink to nothing on the beaches,
  and show no seams or tiling from 2 m to 2 km up.
- The ocean costs under 2 ms a frame at Medium on the dev machine, measured
  with the microprofiler.
- In Play, a 2 m wooden crate floats, bobs and rights itself; a steel block
  sinks.
- Stop restores everything, and the same Space and seed give the same sea.

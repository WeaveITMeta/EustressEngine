# Avatar & Parity: Audit Verdict and Revamp Plan

**Scope:** Studio Play Mode ↔ Client parity, the avatar system, and procedural animation quality.
**Status of claims:** the 48-finding parity audit is treated as ground truth. Everything I state as *verified* below I re-checked directly against the repo, the pinned crate sources, or the GLB binaries in this session. Where I could not verify, I say so.

---

## 0. Two verified facts that change the plan

Before section 1, because they invalidate the naive sequencing every design instinct suggests.

### 0.1 The animation clips do not bind to the Studio body. At all.

`AnimationTargetId::from_names` hashes the **full `Name` path** from the animation root. I parsed the GLB JSON chunks and reconstructed every node path, then intersected the clip's animated-node path set against each body's path set:

```
male_walking     vs x_bot: animated=65  exact-path matches= 7
male_walking     vs y_bot: animated=65  exact-path matches=61
female_walking   vs x_bot: animated=65  exact-path matches= 7
female_walking   vs y_bot: animated=65  exact-path matches=61
male_idle        vs x_bot: animated=65  exact-path matches= 7
male_idle        vs y_bot: animated=65  exact-path matches=61
```

x_bot's 7 matches are the spine chain only (`Hips_01/Spine_02/Spine1_03/Spine2_04/Neck_05/Head_06/HeadTop_End_07`). Every limb chain misses: the clip carries `mixamorig:LeftUpLeg_056`, x_bot carries `mixamorig:LeftUpLeg_061`. y_bot's 4 misses are exactly `RightLeg_062`, `RightFoot_063`, `RightToeBase_064`, `RightToe_End_065`.

- **Studio Play Mode defaults to `BiologicalSex::Female`** (`eustress/crates/engine/src/play_mode_runtime.rs:64`) → `CharacterModel::XBot` (`common/src/services/player.rs:985`). **Studio's play character can only ever animate its spine. Arms and legs are frozen in bind pose.**
- **The Client defaults to `BiologicalSex::Male`** (`client/src/plugins/player_plugin.rs:856`) → `YBot`. **The Client's character walks with a dead right leg.**

None of the 48 findings catches this, because every symptom reads as "the animation system is broken" — and it genuinely also is, for the separate reasons in §1.

I verified the fix works: canonicalising each path segment (strip `mixamorig:`, strip trailing `_NNN`, lowercase, alphanumerics only) recovers **65/65 on both bodies**. This means a clip *rekey* pass is mandatory, not an optimisation, and it must land before any animation-graph work.

### 0.2 The clips key `scale` channels

```
clip channel paths: {'rotation', 'scale', 'translation'}
```

Verified on `male_walking.glb`. Consequence: **any body-customisation scheme that writes bone `Transform.scale` will be overwritten by `animate_targets` every frame.** Height/build applied by bone scaling must *compose with* the animated value in `PostUpdate` after `AnimationSystems`, not be written once at bind time. Anyone who implements height as "scale the armature root at spawn" will ship an avatar that snaps back to default size the moment a clip plays on that bone.

Also verified while I was in there: both `x_bot.glb` and `y_bot.glb` carry **0 morph-target primitives** — there is no blendshape path available on shipped art. The `Armature` root has `scale = [0.01, 0.01, 0.01]` and **no rotation**, with `Hips_01` at local `+104.27` on Y (i.e. Y-up, cm units pre-scale). The `Quat::from_rotation_x(FRAC_PI_2)` "model is Z-up, we need Y-up" fix at `common/src/plugins/skinned_character.rs:239` is therefore applying a 90° tip to an already-upright model. **I have not run the engine to confirm what this looks like on screen** — it is possible something downstream compensates. Verify visually before deleting it.

---

## 1. Parity verdict

### The honest headline

**Press Play in Studio:** a mesh appears, frozen in T-pose from the shoulders down, sliding through the world with no collider and no gravity, driven by a camera that flies in from the world origin at 45° FOV, while the editor camera, editor keybindings, and editor click-to-select all continue to run underneath. The character cannot fall, stand, collide, or jump.

**Launch the Client:** nothing appears. The `bundled://` asset source is never registered (`client/src/main.rs:60`; `file_path` is the CWD-relative `"../common/assets"` at `:61`), so the body GLB and all 8 clips 404. Even if they loaded, the scene has no sun, no light-class sync, two hardcoded cuboids for a collision world, no script compiler, no pause menu, no published-content download path, and the same missing physics body.

These are not "the same code behaving slightly differently." They are two different applications sharing a type namespace.

### Root cause, stated plainly

**The play character has no `RigidBody` and no `Collider` on either side.** `spawn_skinned_character` computes a capsule and then leaves the entire Avian insert commented out (`common/src/plugins/skinned_character.rs:205-215`), and both hosts default to that path (`play_mode_runtime.rs:63`, `client/src/plugins/player_plugin.rs:89`). `SharedCharacterPlugin`'s `character_movement_physics` / `character_jump` / `ground_check` / `update_locomotion` are empty bodies commented out of registration (`common/src/plugins/character_plugin.rs:172-186`, `:418-443`).

Everything downstream is a consequence:

```
no collider/RigidBody ──► no LinearVelocity ──► nothing writes LocomotionController
                                            ──► get_animation_state() returns Idle forever
                                            ──► current_state == target_state forever
                                            ──► crossfade, blend tree, speed-scaling all unreachable
```

Findings 44 and 48 ("animation is dead") are **not animation bugs**. They are the physics blocker observed from downstream. Fixing the animation graph without fixing the body produces no visible change.

And note the trap in the commented-out code itself:

```rust
// Collider::capsule(capsule_radius, capsule_half_height),   // skinned_character.rs:208
```

Verified against `avian3d-0.7.0/src/collision/collider/parry/mod.rs:790`: `pub fn capsule(radius: Scalar, length: Scalar)` takes the **cylinder length**, not a half-height. The Client's live procedural capsule (`player_plugin.rs:307`) has the same bug. **Anyone who "just uncomments B1" ships a character roughly half the intended height with the mesh floating above it.**

### The 17 blockers

| # | Sev | Subsystem | file:line | Player-visible impact | Fix | Effort |
|---|---|---|---|---|---|---|
| 1 | blocker | boot | `client/src/main.rs:60` | Client player is invisible — every character GLB and clip fails to load | Move asset-source registration into `common`, call from both hosts before `AssetPlugin` | S |
| 21 | blocker | boot | `client/src/main.rs:60` | Client cannot load *any* `space://` or `bundled://` asset; shipped `.exe` cannot resolve its CWD-relative root | Same fix + `UnapprovedPathMode::Allow` + exe-adjacent-first resolution | S |
| 43 | blocker | anim | `client/src/main.rs:60` | `SharedAnimationPlugin` is registered but 100% inert client-side | Same fix (3 findings, one change) | — |
| 2 | blocker | boot | `skinned_character.rs:206` | Default spawn path attaches no physics on either side | Restore insert, derived from `BodyMetrics`, with `capsule(radius, cylinder_len)` | M |
| 7 | blocker | spawn | `skinned_character.rs:206` | Character cannot move, jump, or fall in either shell | Kinematic controller on Avian `MoveAndSlide` (verified present at `avian3d-0.7.0/src/character_controller/move_and_slide.rs`) | L |
| 44 | blocker | input | `character_plugin.rs:439` | Animation pinned to Idle forever; every blend path unreachable | One shared `LocomotionController` producer reading `LinearVelocity` + ground probe | L |
| **new** | **blocker** | **anim** | `skinned_character.rs:92-110` | **Studio T-poses below the shoulders; Client walks with a dead right leg** | **Canonical-key rig bind + clip curve rekey (§0.1)** | **M** |
| 13 | blocker | anim | `client/src/plugins/lighting_plugin.rs:37` | Client has no sun; scene is flat ambient | Move sun hydration out of engine `LightingPlugin` into shared core | M |
| 14 | blocker | anim | `engine/src/light_sync.rs:46` | Client cannot render any placed light; fallback 50× too dim | Move `LightClassPlugin` into shared core | S |
| 19 | blocker | physics | `client/Cargo.toml:20` | Client's `DefaultPlugins` silently builds a different engine (6 features dropped) | Realign features **and** add a compile-time import gate (§2, Gate 3) | S |
| 20 | blocker | physics | `client/src/soul/mod.rs:60` | Client has no Rune compiler; scripts never compile | Move compile/lifecycle out of `PlayModeCorePlugin` into shared core | XL |
| 25 | blocker | camera | `client/src/soul/mod.rs:60` | Client never spawns any script — `SoulScriptData` is defined in the engine *binary* crate | Move `SoulScriptData` to `common`; move the seeding pass | XL |
| 26 | blocker | camera | `client/src/soul/mod.rs:73` | 142 Rune ECS bindings in Studio vs 12 in Client → real scripts fail to **compile** | Register `rune_ecs_module` from shared core | L |
| 27 | blocker | camera | `engine/src/soul/rune_api.rs:355` | Luau VM runs client-side but the scene is invisible to it | Move instance seeding + entity↔VM registry + collision opt-in to shared | M |
| 31 | blocker | world | `engine/src/part_selection.rs:189` | Studio LMB runs editor select/drag during Play, raycasting from the **disabled** editor camera | Gate on `PlayModeState`; raycast from the active camera | M |
| 32 | blocker | world | `character_plugin.rs:274` | Escape double-bound on both sides, unordered → cursor state is schedule-order dependent | Single `HostSeams::escape_action`; move cursor toggle to its own binding | M |
| 37 | blocker | script | `engine/src/play_mode.rs:826` | Play Mode's camera-disable loop destroys the Slint overlay camera (order 300, premultiplied alpha) permanently | Exclude `SlintOverlayCamera`; set `is_active` instead of replacing the component | M |
| 38 | blocker | script | `character_plugin.rs:228` | Play Mode renders at Bevy's default 45° FOV / far 1000; Client at 70° | Spawn an explicit `Projection` + `DepthPrepass` from shared tuning | S |

Note that findings 1, 21, and 43 are **one bug counted three times**, and 2 and 7 are one bug counted twice. The 17 blockers are really **13 distinct defects**, and three of them (asset sources, physics body, script hosting) account for nine.

### The 27 majors, by group

| Group | Findings | Shape of the problem | Fix shape | Effort |
|---|---|---|---|---|
| **Dimension / speed chaos** | 3, 11 | Four hardcoded body-dimension sets (1.83/0.33/0.585 vs 1.75/0.24/0.635 vs 0.3/1.2), three spawn heights (+1.015/+1.375/+1.0), and two speed universes (`Character` 1.8 m/s vs `Humanoid` 16 studs/s = 0.798 m/s under `units.rs:127`) | One derived `BodyMetrics`; `Humanoid` becomes the studs-facing authored view converted at one boundary | M |
| **Player identity** | 4, 8 | Studio never spawns a `Player` or sets `PlayerService.local_player`; neither side sets `Player.character`; the two `use_skinned_characters=false` branches spawn structurally different entities | One sealed spawn path; delete the fallback branch entirely | M |
| **Camera** | 5, 17, 33, 39, 40, 41, 42, 6 | Studio's camera spawns at world origin; fog filter `order==0` includes the Client camera and excludes Studio's (order 10); editor fly-camera runs during Play and its drift is never restored; no first-person branch in shared `camera_follow` (hardcoded `height_offset 1.5`); body-hiding keys on `CharacterBody`, which no live path inserts; no UI-focus gate | Shared camera driven by `BodyMetrics.eye_height` + `HostSeams`; gate editor camera on `PlayModeState`; fog by marker not order | L |
| **Client world content** | 9, 16, 18, 10 | Client's collision world is 2 hardcoded cuboids; no terrain streaming/LOD/culling; no PBR-param lookup, no UV tiling, no `MaterialSyncPlugin`; `MoversPlugin`/`JointResolverPlugin`/`ColliderStreamingPlugin` registered inside `SlintUiPlugin` | Move all of these out of `SlintUiPlugin::build` into a shared core plugin | L |
| **Lighting** | 23 | `LightClassPlugin` + engine `LightingPlugin` hydration + `SunDiscPlugin` all Studio-only | Same relocation | M |
| **Physics/render contract** | 22, 24 | Client at Bevy's default 64 Hz / 250 ms catch-up, no `DeterminismPlugin`, no `RenderPlugin` override (default GPU pref, async pipeline compilation, Fifo vsync) | One shared `AvatarTuning`/`HostTuning` resource folded into the parity hash | S |
| **Scripting lifecycle** | 28, 29, 30 | Client registers 2 of 5 Rune hooks, drains no errors; mutates GUI data nothing renders; physics bridge is a stub whose blocking comment is stale (the migration already landed in `common/src/gui/physics_commands.rs`) | Shared script lifecycle; add `SlintGuiPlugin`/`BillboardGuiPlugin` equivalents to the Client | L |
| **Input / meta-controls** | 15, 34, 35 | Escape triple-bound; editor shortcuts live during Play (`Ctrl+Shift+S` double-fires, Delete/F/1/2/3 mutate the scene mid-gameplay); Client pause menu is a no-op shell | `HostSeams` + a play-mode gate on `dispatch_keyboard_shortcuts` | L |
| **Avatar identity & animation** | 45, 46 | `BiologicalSex::character_model` (Female→XBot, `player.rs:985`) and `SkinnedCharacter::new` (XBot→Male, `skinned_character.rs:143`) disagree; authored `Animator`/`KeyframeSequence` have no playback system in either app and no type at all in the Client | One `BaseBody` type returning mesh **and** clip prefix from the same `match`; compile `KeyframeSequence` against the bound rig's `AnimationTargetId`s | M–XL |

Minors (12, 36, 47) and the cleanup item (48) fall out of the above work; #47 (double-applied facing at 8.0 and 10.0 rad/s across `skinned_character.rs:306` and `character_plugin.rs:446`) is deleted by having one facing integrator.

---

## 2. The one structural change that ends parity drift permanently

### Why `SharedCharacterPlugin` failed

Its module doc says it "ensures identical gameplay behavior in both contexts." It produced 48 divergences with 17 blockers, and four of its own systems are commented out of registration. The failure is specific and diagnosable — it shared **systems**, and left four things free:

1. **Entity construction.** `spawn_skinned_character(commands, server, pos, model, gender)` is `pub` with free parameters. That signature *is* the bug that let Studio pass Female and the Client pass Male. Each host also appended its own `.insert()` tail.
2. **Plugin composition.** `SkinnedCharacterPlugin`, `SharedAnimationPlugin`, `SharedCharacterPlugin` are all `pub`; both hosts add different subsets, and the Client adds a second, competing character stack in `PlayerServicePlugin`.
3. **Feature selection.** `bevy = { default-features = false }` at the workspace root means each crate's feature list *is* its whole Bevy. Feature lists are prose; prose drifts.
4. **Environment.** Asset sources, physics tick, gravity, camera FOV, present mode — never shared, never asserted.

Every one of the 17 blockers lives in one of those four gaps. Convention cannot fail loudly, so it failed silently for 48 findings' worth of commits.

### The move: **one sealed avatar runtime, six enforcement gates**

Not "share it harder." **Make divergence unrepresentable, then assert what's left.**

**Gate 1 — the sealed spawn token (compile-time).**

```rust
// common/src/avatar/mod.rs
#[derive(Component, Debug)]
pub struct SpawnedByAvatarRuntime(());   // private tuple field
```

No crate outside `common` can construct it. Every avatar system filters `With<SpawnedByAvatarRuntime>`. The only public API is a message:

```rust
#[derive(Message, Debug, Clone)]
pub struct SpawnAvatar { descriptor: AvatarDescriptor, at: Vec3, yaw: f32, control: AvatarControl }
// fields private; constructors take a descriptor, never a model or a sex
```

This is strictly stronger than a `pub(crate)` spawn function: it also makes it impossible for a host to spawn a character the runtime does *not* drive — which is exactly the Studio-ghost-vs-Client-capsule divergence (finding 8). After this, `grep -rn 'fn spawn.*-> Entity' | grep -i char` returns nothing.

**Gate 2 — sub-plugins are private.** `SkinnedCharacterPlugin`, `SharedCharacterPlugin`, `SharedAnimationPlugin` become `pub(crate)`. The only export is `AvatarRuntimePlugin`. A host that tries to add a sub-plugin gets `error[E0603]`. `client/src/plugins/animation_plugin.rs` and `character_controller.rs` are **deleted, not deprecated** — a `#[deprecated]` alias that still compiles is another drift vector.

**Gate 3 — the Bevy feature list is enforced by the type system.**

```rust
// common/src/avatar/required_bevy.rs
#![allow(unused_imports)]
use bevy::animation::AnimationPlayer;
use bevy::pbr::StandardMaterial;
use bevy::gltf::GltfNode;
use bevy::scene::SceneInstanceReady;
use bevy::state::state::States;

#[cfg(all(feature = "avatar", not(feature = "physics")))]
compile_error!("an avatar without a collider is blocker #7 and is not a supported build");
```

Ten lines turn finding 19 from a silent behaviour change into a compile error **inside `eustress-common`** when building `-p eustress-client`. Caveat: Cargo unions features across a whole-workspace build, so CI must run the per-binary builds explicitly, not only `cargo build --workspace`.

**Gate 4 — boot panics with the remedy in the message.** `AvatarRuntimePlugin::build` asserts the asset sources are registered, Avian is added, and `AnimationPlugin` is added. Findings 1/21/43 become an unmissable startup failure in *shipped builds*, not only in tests. This matters because `app_core.rs:70` sets the `bundled://` root to a compile-time `CARGO_MANIFEST_DIR` path with no exe-adjacent fallback (unlike `default://` at `main.rs:175-184`) — **packaged Studio builds are already broken for avatars, not only the Client.**

**Gate 5 — host differences are an exhaustive `const fn` match, not composition.**

```rust
pub const fn seams(self) -> HostSeams {
    match self {
        AvatarHost::Studio => HostSeams {
            gate_input_on_viewport_focus: true,   // closes findings 42, 33
            suppress_editor_camera: true,         // closes findings 41, 33
            camera_order: 10,
            escape_action: EscapeAction::StopPlay, // closes findings 15, 32
            gate_editor_shortcuts: true,           // closes finding 34
        },
        AvatarHost::Client => HostSeams { /* ... */ },
    }
}
```

Adding a seam field is a compile error in **both** arms. Studio genuinely needs viewport-focus gating and editor-camera suppression; a design that only deletes host-specific code has nowhere to put them, and they come back as ungated host code. This is the piece the "share a plugin" approach never had.

**Gate 6 — the golden parity test, two tiers.** Both binaries expose `build_app() -> App` (a prerequisite refactor, not a convenience — it is what makes the two hosts diffable as values).

*Tier A, every commit, sub-second, headless:*
1. `bundled://characters/y_bot.glb` resolves in both. (Kills 1/21/43 permanently.)
2. The `TypeId` set registered in every `AvatarSystems::*` set is byte-identical.
3. The sorted component set on the entity produced by `SpawnAvatar(default_descriptor)` is identical. **This is the assertion that would have caught the commented-out physics insert the day it was written.**
4. `Time<Fixed>` timestep, `Gravity`, `Time<Virtual>::max_delta`, `Projection` FOV, `Camera.order` match (via `HostSeams`). (Kills 22, 24, 38, 17.)
5. `BodyMetrics` resolved from the same descriptor is bit-identical.
6. `RetargetedClip.bound_bones >= 60` for every (clip, body) pair. **This is the only assertion in the whole plan that goes red on a currently-shipping defect: x_bot scores 7 today.**

*Tier B, nightly and pre-release:* a scripted 600-tick input tape (walk, sprint, jump, land, ramp, ledge, turn) replayed through both profiles. Exact equality on discrete channels (`grounded`, `AnimationState`, active graph nodes, equipped slot set, LOD tier); `1e-4` on continuous ones; **first-divergent-tick reported on failure.**

**Determinism is engineered, not assumed.** The character is `RigidBody::Kinematic` with `CustomPositionIntegration`, driven by Avian 0.7's `MoveAndSlide` — verified present at `avian3d-0.7.0/src/character_controller/move_and_slide.rs`, and a *pure function* of (shape, position, velocity, dt, config, filter). A dynamic body's result depends on solver iterations, substeps, contact ordering and sleep thresholds — a large surface that can differ between hosts without anyone noticing. Kinematic reduces the parity surface to *inputs*, which Tier A already asserts equal. Cost, stated plainly: **the character no longer pushes dynamic props for free** — that needs an explicit impulse from the `on_hit` callback, and it belongs in the commit message.

**P0's gate is that the test COMPILES AND FAILS on today's code**, printing a named diff. A green test at P0 means the test is wrong.

### The one relocation that stops the *next* feature from being Studio-only

`InteractionPlugin`, the spawner-group plugins (`ui/slint_ui.rs:1313-1338`), and `LightClassPlugin` (`:1355`) live inside `SlintUiPlugin::build`. That makes them editor-window-only **by construction** — the Client has no class registry, no appearance runtime, and no lights, and it is structurally incapable of getting them. Findings 10, 14, 16, 18, 23 and 29 all reduce to this. Move them into the shared core. Plugin *placement*, not feature code, is the parity blocker.

---

## 3. Progressing Client + Play Mode

Ordered. Each item is independently landable.

| # | Work | Why it is first / what it unblocks | Effort |
|---|---|---|---|
| 1 | **Shared asset-source registration** (`common/src/avatar/boot.rs`), exe-adjacent-first for `bundled://`, `UnapprovedPathMode::Allow`, called by both hosts before `AssetPlugin` | Closes 1/21/43 in one change. **Also fixes packaged Studio builds**, which cannot load a character GLB today. Nothing else can be tested until this lands | S |
| 2 | **`build_app() -> App` in both binaries** | Prerequisite for every parity gate. Without it Gate 6 cannot exist | S |
| 3 | **Client Bevy feature realignment + `required_bevy.rs`** | Six missing features gate plugins inside `DefaultPlugins`. Do it with the compile gate, not by hand, or it drifts again | S |
| 4 | **Move `InteractionPlugin` + spawner groups + `LightClassPlugin` + `MoversPlugin` + `JointResolverPlugin` + `ColliderStreamingPlugin` out of `SlintUiPlugin`** into a shared core plugin | Closes 10, 14, 23 and makes 16/18/29 tractable. Without this, moving `appearance.rs` into `common` is a silent no-op client-side — the systems compile but no components exist to drive them | M |
| 5 | **Sun + lighting hydration into shared core** | Closes 13. The Client currently applies `Atmosphere` to a sunless camera | M |
| 6 | **`AvatarRuntimePlugin` + sealed spawn + `BodyMetrics` + kinematic controller** | The whole of §2 Gates 1–5 plus blockers 2/7/44. This is where the character starts standing | L |
| 7 | **Rig bind + clip rekey** (§0.1) | Without it the character stands and slides in a T-pose. Must precede any graph work | M |
| 8 | **`HostSeams` + input/camera untangling** — gate editor camera, editor shortcuts, and `part_selection` on `PlayModeState`; single Escape owner; explicit `Projection`; camera at spawn position; fog by marker | Closes 5, 15, 17, 31, 32, 33, 34, 37, 38, 41, 42 as a block | L |
| 9 | **Client scene ingestion from R2** — see below | Nothing published can be played. This is the product blocker | L |
| 10 | **Client collider generation + terrain streaming/LOD/culling + `MaterialSyncPlugin`** | Closes 9, 16, 18. Depends on #4 | L |
| 11 | **Script hosting into shared core** — `SoulScriptData` moved to `common`, `compile_scripts_on_play` + the 10-system lifecycle + `rune_ecs_module` + Luau scene seeding | Closes 20, 25, 26, 27, 28, 30. Largest remaining item; independent of the avatar work | XL |
| 12 | **Client pause menu + meta-controls** | Closes 12, 35, 36 | M |

### The R2 gap — nothing can play a published simulation

This is the single largest product hole and it is not in the 48 findings.

**Studio → R2 is real and complete.** `do_publish` (`engine/src/ui/file_event_handler.rs:514`) resolves the Universe root, requires an `AuthState` token, writes `publish.toml`/`publish-journal.toml`/`sync.toml`, captures a thumbnail, and hands off to a background thread. `execute_publish_upload` (`:701`) tars+zstds the whole Universe into a `.pak` (`:707`), hash-skips unchanged content, `POST`s `/api/simulations/publish` (`:742`), then `PUT`s the `.pak` — single PUT under 100 MB (`:764`), multipart create/part/complete above (`:775`, `:795`, `:818`) — and `PUT`s the thumbnail (`:839`). `PUBLISH_API = "https://api.eustress.dev"` (`:688`). The module doc at `:24` calling this a stub is stale.

**Client → R2 does not exist.** `client/src/systems/scene_loader.rs` opens with a deprecation notice that *describes the intended architecture* ("Client downloads the binary blob and deserializes it using the shared binary format parser") and then does not implement it. All four `#[allow(deprecated)]` / `#[deprecated]` markers are on the only loaders present: RON and JSON readers off a local CLI argument. There is no HTTP fetch, no `.pak` extraction, no zstd decompression, no `.eustress` binary deserialiser, no local cache, no manifest.

**Consequence:** the publish pipeline writes to a bucket nothing reads. A user can publish a simulation and no one can play it.

**Fix shape (L):**
1. Extract the `.eustress` binary parser out of `engine/src/serialization/binary.rs` into `common` — the deprecation notice already names this as the plan.
2. `GET /api/simulations/{id}/space` → `.pak` → zstd decode → untar into a content-addressed local cache keyed on the manifest hash the publish side already computes for hash-skip.
3. Reuse `instance_loader`'s spawn path so Client and Studio materialise identical entities — do **not** write a second spawner.
4. Extend `prepare_publish_manifests` (`:966`) with a `[required_assets]` block so a published Universe declares the bodies, clip sets and worn items it needs. **Character assets are `bundled://`, never `space://`, so no publish has ever carried them** — this is the slot that fixes it.
5. Fold the resulting entity set into Tier B of the parity test: same `.pak` loaded in both shells must produce the same component sets.

---

## 4. Avatar system: honest current state

**The entire avatar asset library is 2 Mixamo bodies and 8 clips.** `x_bot.glb` (1.8 MB), `y_bot.glb` (2.2 MB), and `{male,female}_{idle,walking,running,jump}.glb` (~1.5 MB total). There is no hair, no garment, no hat, no glasses, no face asset. A repo-wide grep for "hair" across `common/src`, `engine/src` and `client/src` returns **one** hit, and it is the English word in a comment about rounding at `common/src/terrain/worldgen/export.rs:121`.

### REAL (runs, does something observable)

- Clip loading → flat 5-node `AnimationGraph` → `idle` on repeat. `mark_new_characters_for_animation` (`skinned_character.rs:276`) → `load_character_animation_clips` (`animation_plugin.rs:269`) → `create_animation_graphs` (`:300`) → `start_idle_animation` (`:403`). **On y_bot this animates 61/65 bones. On x_bot it animates 7.** This is the entirety of observable animation.
- `HumanoidSnapshot` play/stop round-trip (`play_mode.rs:728-735`, `:1012-1017`). Real, but 4 of 13 fields, and both queries carry `Without<PlayModeCharacter>` — it never touches the avatar itself.
- `.eustress` binary `Humanoid` round-trip (`binary.rs:1399`, `:2249`). Lossy: `hip_height` is stuffed into the `jump_height` slot; `rig_type` hardcoded 0.
- `BiologicalSex` → body GLB. **The only avatar axis that reaches a rendered character**, and it is hardcoded to opposite values on the two sides.
- `TimelineAnimationPlugin` (`engine/src/timeline_animation.rs:273`) genuinely ticks and drives `Transform`/`BasePart` tracks — but touches no skeleton and no `AnimationPlayer`. It is an object-track editor feature, not character animation.

### STUB (code exists, cannot execute)

- **Foot IK.** `update_foot_ik` (`animation_plugin.rs:661`) is written and **never registered**. Its ray helper `raycast_ground` (`:751-759`) has an unconditional `None` body under a `#[cfg(feature = "physics")]` that is *enabled in both binaries*. It approximates feet at ±0.15 m from the character centre (`:696`) rather than reading foot bones. `apply_foot_ik_to_bones` (`:762`) *is* registered but requires `&HumanoidRig`, which is never inserted.
- **Layered/additive animation.** `update_animation_layers` (`:851`) lerps a float and ends at a TODO (`:870`) blaming Bevy for missing bone masks. **Verified false**: `bevy_animation-0.19.0/src/graph.rs` ships `AnimationMask = u64` (`:426`), `add_clip_with_mask` (`:492`), `add_blend` (`:537`), `add_additive_blend` (`:577`), `add_target_to_mask_group` (`:672`). The blocker is self-inflicted.
- **Root motion.** `extract_root_motion` (`:807`) registered but gated on `HumanoidRig`. `apply_root_motion` (`:840`) is a **zero-parameter empty function**.
- **Website customizer.** `web/src/pages/profile.rs:357-518` renders 11 controls (2 sliders, 6 dropdowns, 21 hex swatches, 31 discrete options). Scanning lines 356-520 for `on:click`/`on:change`/`on:input`/`prop:value`/`RwSignal` returns **zero matches**. "Save Avatar" (`:513`) has no handler. The 3D preview (`:364`) is a `<div>` containing the literal text `"3D Preview"`.
- **Marketplace.** `infrastructure/cloudflare/api/src/index.js:560-562` — `// Marketplace (stub — not yet implemented)`, returns `{items:[],total:0,page:1}` for every request. The avatar tab renders "No items found" forever.
- **Accessory attach.** `engine/src/interaction/equip.rs:238-241` documents the missing Attachment-by-name resolver. Hat and Glasses cannot be worn.
- **`CharacterMesh` swap.** `appearance.rs:352-359` — an explicit TODO that only logs.

### DEAD (registered/declared, provably unreachable, or zero references)

- `HumanoidRig` (`services/animation.rs:166`): **3 query sites, 0 insert sites.** This one absence blocks foot IK, root motion, look-at, masks, secondary motion and retargeting.
- `LocomotionController`: inserted, never written in either live schedule. The only `update_from_velocity` call sites are inside `#[allow(dead_code)]` (`client/character_controller.rs:169`, `client/player_plugin.rs:1227`), in a module that declares no `Plugin`.
- `BlendTree1D`, `BlendTree2D`, `locomotion_8dir`, `IKTarget`, `IKLimb`, `AnimationLayer`, `LayerBlendMode`, `LayerMask`, `CharacterAnimationBundle`, `AnimationService` (5 fields, 0 readers), `PlayAnimationEvent`, `AnimationFinishedEvent`, `AnimationEventTriggered` (0 writers, 0 readers) — **pure unreferenced type surface.**
- `update_directional_blend` / `apply_directional_blend`: registered, write into `nodes.walk_forward/backward/left/right`, which `create_animation_graphs` never populates (`..default()` at `:360`). Always `None`. No directional clips exist anyway.
- `apply_procedural_limb_animation` (`humanoid.rs:198-202`): an **empty no-arg function**, imported into `SharedCharacterPlugin` at `character_plugin.rs:148` and never registered — making `animate_arm` (`:204`) and `animate_leg` (`:271`) dead private fns.
- **The best animation code in the repo is dead.** `client/src/plugins/player_plugin.rs:1501+` has counter-phase shoulder swing, elbow flex coupled to shoulder phase, distinct airborne pump/brace poses and idle micro-sway. `#[allow(dead_code)]`, unregistered, and keyed on `CharacterBody` which the live path never inserts (~380 lines).
- `PlayerProfile.avatar_data: Option<String>` (`player.rs:1024`): **3 repo-wide references, all inside its own struct definition and `Default` impl.** Never written, never read, no schema.
- `HumanoidDescription` (`classes.rs:10292`): complete spawner with import/export/tests, **zero consumer systems**. `height_scale`, `width_scale`, `body_type_scale` change nothing.
- All of `engine/src/interaction/appearance.rs` (~500 lines, correct, tested): keys on `CharacterLimb`, inserted only at `humanoid.rs:179` and `:188` in the legacy procedural path neither binary takes. Registered but unreachable. It also speaks Roblox BrickColor integers (`:65-102`) while the website ships hex.
- Authored `Animator` / `KeyframeSequence` / `Pose` / `CurveAnimation` / `IKControl` classes: full spawners, **no playback system in either binary**. `animator.rs:14-15` admits "the playback systems (NOT owned by this task)".
- Space TOML round-trip for `Humanoid`: `engine/src/space/instance_loader.rs` contains **zero occurrences of "humanoid"**. Authored `WalkSpeed` does not survive a reload.

### The startup banner lies

`animation_plugin.rs:1008-1013` prints five checkmarked features (crossfade, 1D/2D blend trees, foot IK with ground adaptation, layered animation, root motion) on every boot of both binaries. **Zero of the five function.** Delete it now; each line comes back only when a test asserts a measured quantity.

### Verdict

Of 18 AAA motion features audited, **zero are real at runtime**. What a player observes is one looping idle clip on a mannequin that slides — and in Studio, only the mannequin's spine moves. Architecture-that-could-work: ~1.5/10. What a player sees: ~0.5/10.

---

## 5. The revamp

**Winning architecture: KINESIS** (sealed spawn token, `HostSeams`, canonical-key rig bind + clip rekey, kinematic `MoveAndSlide`, `BodyMetrics`), with three mandatory grafts:

- **From MORPHIC:** a separate no-Bevy `eustress-avatar-schema` crate. This is not a preference. I verified `eustress/crates/web` is pure CSR wasm (`leptos 0.7` with `csr`, `wasm-bindgen`, `web-sys`) with **no `eustress-common` dependency**. A descriptor that derives `bevy::Component` cannot compile there, so KINESIS's "the same serde shape serves both" is a hand-mirror — the exact class of failure this design exists to eliminate. Also graft `required_bevy.rs` (Gate 3), the boot-panic gates (Gate 4), and the `Norm01`/`Srgb8` newtypes.
- **From ASSEMBLY:** the `VariableCurve`-opacity risk with its half-day spike scheduled *before* the phase commits; topological bone inference as the fallback when the alias table misses; `ItemValidation` with bind-pose deviation rejected at ingest; the `AvatarLod` tiers; and the `SlintUiPlugin` relocation.
- **From my own verification:** the clip `scale`-channel finding (§0.2) forces the body-scale applicator to compose in `PostUpdate` rather than write once.

**Cut from KINESIS:** the ragdoll/get-up phase. Scope the user did not ask for, and the only weak item in the strongest design. Explicit stop line: **P0–P6 is the product.**

### 5.1 The descriptor — `eustress/crates/avatar-schema/src/descriptor.rs`

No Bevy. No glam. `serde` only. Bevy derives behind a `bevy` feature the web crate does not enable. This crate compiles to wasm for Leptos **and** links natively into `eustress-common`. Shared compilation is a linker fact, not a convention.

```rust
pub const AVATAR_SCHEMA_VERSION: u16 = 1;

#[cfg(feature = "bevy")] use bevy_ecs::prelude::Component;
#[cfg(feature = "bevy")] use bevy_reflect::Reflect;

// ─── scalars ─────────────────────────────────────────────────────────────
/// 0.0..=1.0 authoring scalar. The website ships 0..100 integer sliders;
/// `from_percent`/`percent` is the ONLY conversion, so the slider value and
/// the runtime value are structurally incapable of disagreeing.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(transparent)]
pub struct Norm01(f32);

impl Norm01 {
    pub fn new(v: f32) -> Self { Self(if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.5 }) }
    pub fn from_percent(p: i32) -> Self { Self::new(p as f32 / 100.0) }
    pub fn percent(self) -> i32 { (self.0 * 100.0).round() as i32 }
    pub fn get(self) -> f32 { self.0 }
    pub fn remap(self, lo: f32, hi: f32) -> f32 { lo + (hi - lo) * self.0 }
}
impl Default for Norm01 { fn default() -> Self { Norm01(0.5) } }

/// sRGB bytes. Round-trips byte-exact with the website's "#rrggbb" swatches.
/// This is the NATIVE colour model; the 31-entry BrickColor table at
/// engine/src/interaction/appearance.rs:65-102 is demoted to an import adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
pub struct Srgb8(pub u8, pub u8, pub u8);

impl Srgb8 {
    pub fn from_hex(s: &str) -> Result<Self, AvatarError> { /* "#rrggbb" | "rrggbb" */ }
    pub fn to_hex(self) -> String { format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2) }
    /// Linear-space floats for StandardMaterial::base_color.
    pub fn to_linear(self) -> [f32; 3] { /* sRGB EOTF */ }
}
// serde as the literal "#rrggbb" string the website already emits.

// ─── base body: mesh and clip prefix from ONE match ──────────────────────
/// Replaces the crossed pair `BiologicalSex::character_model`
/// (common/src/services/player.rs:985, Female -> XBot) and
/// `SkinnedCharacter::new` (skinned_character.rs:143, XBot -> Male).
/// Because both come out of the same type, the pairing is uncrossable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(rename_all = "snake_case")]
pub enum BaseBody { #[default] Feminine, Masculine }

impl BaseBody {
    pub const fn body_asset(self) -> &'static str {
        match self {
            BaseBody::Feminine  => "bundled://characters/y_bot.glb",
            BaseBody::Masculine => "bundled://characters/x_bot.glb",
        }
    }
    pub const fn clip_prefix(self) -> &'static str {
        match self { BaseBody::Feminine => "female", BaseBody::Masculine => "male" }
    }
}

// ─── continuous body axes ────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(default)]
pub struct BodyMorphs {
    pub height: Norm01,        // website "Height" slider, profile.rs:379
    pub build:  Norm01,        // website "Build"  slider, profile.rs:383
    pub leg_ratio: Norm01,     // reserved; remaps 0.46..=0.56
    /// Face Shape stored as four exclusive weights so today's 4-option
    /// dropdown and a future continuous face slider are the SAME data with
    /// no migration. `FaceShape::apply` / `FaceShape::dominant` convert.
    pub face_round: Norm01, pub face_square: Norm01,
    pub face_oval:  Norm01, pub face_diamond: Norm01,
}

// ─── the ONE dimension source ────────────────────────────────────────────
pub const MIN_HEIGHT_M: f32 = 1.45;
pub const MAX_HEIGHT_M: f32 = 2.05;
pub const GRAVITY_MPS2: f32 = 9.80665;      // matches client/src/main.rs:68
pub const REFERENCE_LEG_LENGTH_M: f32 = 0.93;

/// Retires: 1.83/0.33/0.585 (skinned_character.rs:193-195),
/// 1.75/0.24/0.635 (client player_plugin.rs:265 region), 0.3/1.2
/// (humanoid.rs:135), and the spawn heights +1.015 / +1.375 / +1.0.
/// Every metric dimension in the runtime comes out of this ONE function.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
pub struct BodyMetrics {
    pub height_m: f32,
    pub capsule_radius: f32,
    /// Avian's `Collider::capsule(radius, length)` takes the CYLINDER LENGTH.
    /// Verified: avian3d-0.7.0/src/collision/collider/parry/mod.rs:790.
    /// The commented-out insert at skinned_character.rs:208 and the Client's
    /// live capsule at player_plugin.rs:307 both pass a half-height.
    /// This field name exists so that bug is unrepresentable.
    pub capsule_cylinder_len: f32,
    pub spawn_center_offset: f32,     // root Y above the ground contact point
    pub mesh_offset: f32,             // local Y of the mesh child
    pub eye_height: f32,
    pub hip_height: f32,
    pub leg_length: f32,
    pub shoulder_half_width: f32,
    /// Lateral distance from the root axis to each foot, MEASURED from the
    /// bind pose. Replaces the `±0.15` guess at animation_plugin.rs:696.
    pub foot_half_separation: f32,
    pub mass_kg: f32,
    /// leg_length / REFERENCE_LEG_LENGTH_M. Scales blend-space knots,
    /// playback rate, and the Hips translation curves during retarget.
    pub stride_scale: f32,
    /// Rig uniform scale = height_m / MEASURED bind height.
    pub rig_scale: f32,
}

impl BodyMorphs {
    /// `bind_height_m` is what `AvatarRig` MEASURED on the loaded skeleton,
    /// falling back to a nominal before the rig binds. Passing it in keeps
    /// this pure, so wasm and the engine agree bit-for-bit.
    pub fn metrics(&self, bind_height_m: f32) -> BodyMetrics {
        let h     = self.height.remap(MIN_HEIGHT_M, MAX_HEIGHT_M);
        let build = self.build.get() * 2.0 - 1.0;                 // -1..=1
        let r     = 0.155 * h * (1.0 + 0.22 * build);
        let leg   = h * self.leg_ratio.remap(0.46, 0.56);
        BodyMetrics {
            height_m: h,
            capsule_radius: r,
            capsule_cylinder_len: (h - 2.0 * r).max(0.05),
            spawn_center_offset: h * 0.5 + 0.02,
            mesh_offset: -(h * 0.5),
            eye_height: h * 0.935,
            hip_height: leg,
            leg_length: leg,
            shoulder_half_width: 0.115 * h * (1.0 + 0.25 * build),
            foot_half_separation: 0.055 * h,
            mass_kg: 22.0 * h * h * (1.0 + 0.30 * build),
            stride_scale: leg / REFERENCE_LEG_LENGTH_M,
            rig_scale: h / bind_height_m.max(0.5),
        }
    }
}

// ─── motion: authored overrides on top of derived defaults ───────────────
/// `None` = derive from the body. `Some` = an explicit authored override
/// (Humanoid property edit, `humanoid.WalkSpeed = 24`, MCP). Keeping the
/// override separate means resizing the body never discards authored intent,
/// and authoring never freezes the body-derived value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(default)]
pub struct MotionOverrides {
    pub walk_speed_mps: Option<f32>,
    pub run_speed_mps:  Option<f32>,
    pub jump_apex_m:    Option<f32>,
    pub max_slope_deg:  Option<f32>,
    pub step_height_m:  Option<f32>,
    pub turn_rate_rad_s: Option<f32>,
    pub clip_set: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
pub struct ResolvedMotion {
    pub walk_speed_mps: f32,
    pub run_speed_mps:  f32,
    pub sprint_speed_mps: f32,
    pub jump_apex_m: f32,
    pub jump_speed_mps: f32,      // sqrt(2*g*apex) — the impulse actually applied
    pub max_slope_rad: f32,
    pub step_height_m: f32,
    pub turn_rate_rad_s: f32,
}

impl MotionOverrides {
    pub fn resolve(&self, m: &BodyMetrics) -> ResolvedMotion {
        // Froude-scaled: v = Fr.sqrt(g*L), Fr ~ 0.25 walk / 1.0 run. A 1.45 m
        // and a 2.05 m avatar look like the same gait, not a fast dwarf.
        let g = (GRAVITY_MPS2 * m.leg_length).sqrt();
        let walk = self.walk_speed_mps.unwrap_or(0.25f32.sqrt() * g);
        let run  = self.run_speed_mps.unwrap_or(g);
        let apex = self.jump_apex_m.unwrap_or(0.95 * m.stride_scale);
        ResolvedMotion {
            walk_speed_mps: walk, run_speed_mps: run,
            sprint_speed_mps: run * 1.45,
            jump_apex_m: apex,
            jump_speed_mps: (2.0 * GRAVITY_MPS2 * apex.max(0.0)).sqrt(),
            max_slope_rad: self.max_slope_deg.unwrap_or(48.0).to_radians(),
            step_height_m: self.step_height_m.unwrap_or(0.30 * m.stride_scale),
            turn_rate_rad_s: self.turn_rate_rad_s.unwrap_or(9.0),
        }
    }
}

// ─── slots + catalog ─────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(rename_all = "snake_case")]
pub enum WearSlot { Hair, Face, Top, Bottom, Shoes, Hat, Glasses, Back, Neck }

/// A concrete attach point created by the rig binder as a child of a bone.
/// This IS the "Attachment-by-name resolver" that engine/src/interaction/
/// equip.rs:238-241 documents as the blocker for the whole Accessory path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(rename_all = "snake_case")]
pub enum AvatarSocket {
    HeadTop, EyeLine, FaceFront, ChestFront, Back, Hips,
    LeftFootSole, RightFootSole, LeftHandGrip, RightHandGrip,
}

/// How a slot item binds. This field is what makes a marketplace payload
/// EXECUTABLE rather than decorative.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotFit {
    /// Skinned garment/hair: its SkinnedMesh.joints are remapped onto the
    /// host rig by canonical bone name, so it deforms with height/build.
    Skinned,
    /// Rigid mesh parented to a socket.
    Socket { socket: AvatarSocket },
    /// Roblox-style 2D template texture onto existing body materials. This is
    /// the ONE path engine/src/interaction/appearance.rs already implements.
    Decal { target: DecalTarget },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(tag = "src", rename_all = "snake_case")]
pub enum AssetRef {
    Builtin { path: String },        // -> bundled://
    Space   { path: String },        // -> space://
    Market  { item_id: u64, rev: u32 }, // -> avatar://item/<id>.<rev>.glb
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
pub struct SlotItem {
    pub asset: AssetRef,
    pub fit: SlotFit,
    pub tint: Option<PaletteSlot>,
    pub adjust: SlotAdjust,
    /// Body regions this item hides. Kills garment/skin z-fighting without
    /// per-item shader work — the most common UGC artifact.
    pub hides: BodyRegionMask,
    /// Spring-chain roots inside the mesh (hair, coat tails, straps).
    pub spring_roots: Vec<String>,
}

// ─── the descriptor ──────────────────────────────────────────────────────
/// THE avatar. One type, one serde impl, four consumers: the website PUT
/// body, the KV blob, `class_schema/Avatar/_instance.toml`, and the runtime
/// spawn input. Inserted on the character ROOT; `Changed<AvatarDescriptor>`
/// is the ONLY re-apply trigger — there is no "apply once at spawn" path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Component, Reflect), reflect(Component))]
#[serde(default)]
pub struct AvatarDescriptor {
    pub version: u16,
    pub base_body: BaseBody,
    pub morphs: BodyMorphs,
    pub palette: AvatarPalette,          // skin / hair / eyes / top / bottom
    pub slots: BTreeMap<WearSlot, SlotItem>,   // BTreeMap => stable hash
    pub motion: MotionOverrides,
}

impl AvatarDescriptor {
    pub fn migrate(self) -> Result<Self, AvatarError>;
    /// Field-order-independent canonical hash. Stable CDN key for the
    /// server-rendered preview, stable dedup key for the publish manifest,
    /// stable merge key for the LOD atlas bake.
    pub fn content_hash(&self) -> u64;
    pub fn to_json(&self) -> String;
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error>;
    pub fn to_toml(&self) -> String;     // same serde impl => disk == wire
    /// A purchased marketplace item overlays fields. ONE operation serves
    /// equip, try-on, inventory, gifting and bundles.
    pub fn apply(&mut self, frag: &AvatarFragment);
}
```

`PlayerProfile.avatar_data: Option<String>` (`player.rs:1024`) becomes `pub avatar: AvatarDescriptor`. There is no user data to migrate — take the free break now.

### 5.2 The runtime rig — `common/src/avatar/rig.rs`

```rust
/// What `HumanoidRig` (services/animation.rs:166) was supposed to be.
/// That type has 3 query sites and 0 insert sites, which is the single
/// reason foot IK, root motion, look-at, masks and secondary motion are
/// all unreachable.
#[derive(Component, Debug, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct AvatarRig {
    pub bones: HashMap<HumanoidBone, Entity>,
    pub bind_local: HashMap<HumanoidBone, Transform>,
    pub bind_model: HashMap<HumanoidBone, Transform>,
    /// The full `Name` path from the animation root. Input to
    /// `AnimationTargetId::from_names` — and therefore the ONLY way to
    /// author or rekey a curve this skeleton will accept.
    pub bone_paths: HashMap<HumanoidBone, Vec<Name>>,
    pub target_ids: HashMap<HumanoidBone, AnimationTargetId>,
    pub sockets: HashMap<AvatarSocket, Entity>,
    pub skin_materials: Vec<Handle<StandardMaterial>>,
    pub player: Entity,          // the entity carrying AnimationPlayer
    pub animation_root: Entity,
    pub measured: RigMeasurements,
    /// hash(sorted canonical bone set, measurements quantised to 1 mm).
    /// Keys the retarget cache so a 1.45 m and a 2.05 m avatar NEVER share
    /// a retargeted clip.
    pub signature: u64,
    /// Bones the alias table + topological fallback could not resolve.
    /// Non-empty => error! with every raw name seen vs every token expected,
    /// and panic under debug_assertions.
    pub unresolved: Vec<HumanoidBone>,
}

/// `"mixamorig:LeftUpLeg_061"` -> `"leftupleg"`.
/// `"mixamorig:LeftUpLeg_056"` -> `"leftupleg"`.
/// VERIFIED: this recovers 65/65 animated nodes on BOTH shipped bodies,
/// where exact path matching scores 7/65 on x_bot and 61/65 on y_bot.
pub fn canonical_bone_key(raw: &str) -> String {
    let s = raw.rsplit([':', '|']).next().unwrap_or(raw);
    let s = s.trim_end_matches(|c: char| c.is_ascii_digit());
    let s = s.trim_end_matches('_');
    s.chars().filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase).collect()
}
```

`BONE_ALIASES` covers Mixamo, UE5 (`thigh_l`), Unity (`LeftUpperLeg`) and VRM (`J_Bip_L_UpperLeg`). When alias coverage misses a *required* bone, fall back to topological inference (graft from ASSEMBLY): the deepest node with two symmetric descendant chains of length ≥ 3 whose leaves have the lowest world-Y is Hips-with-legs; the longest ancestor chain upward is the spine, terminating at Head; chains branching from the highest spine node with opposite-sign leaf world-X are arms; order within a chain is Upper/Lower/Extremity by depth; symmetry resolved by bind-pose world X sign, never by name. That is what makes "any humanoid GLB binds" true rather than aspirational.

### 5.3 Website axis → engine representation

| Website control (`web/src/pages/profile.rs`) | Descriptor field | Runtime mechanism | Needs new art? |
|---|---|---|---|
| Height slider `:379` (0..100) | `morphs.height` → `Norm01` | `BodyMetrics.rig_scale` composed onto the armature root **in `PostUpdate` after `AnimationSystems`** (clips key scale — §0.2); drives capsule, spawn offset, eye height, stride, gait speeds, jump | **No** |
| Build slider `:383` | `morphs.build` | capsule radius, shoulder/hip bone scale, `mass_kg` | **No** |
| Face Shape `:394-399` (Round/Square/Oval/Diamond) | `morphs.face_*` one-hot | `slots[Face]` → `SlotFit::Decal{Face}` v1; morph weights when a body ships targets (**verified 0 today**) | Yes (4 textures) |
| Skin Tone `:404-409` (6 hex) | `palette.skin` | `Srgb8::to_linear()` → `base_color` on rig-bound `CharacterLimb` entities | **No** |
| Hair Style `:421-429` (7) | `slots[Hair]` → `SlotFit::Skinned` + `spring_roots` on long styles | joint remap by canonical bone; `SpringChain` | Yes (7 meshes) |
| Hair Colour `:434-442` (9 hex) | `palette.hair` | material tint | **No** |
| Top `:454-460` (5) | `slots[Top]` → `Skinned` (Hoodie/Jacket/Suit) or `Decal{TorsoAndArms}` (T-Shirt/Tank) | joint remap or template texture | Partial |
| Bottom `:463-470` (5) | `slots[Bottom]` → `Decal{Legs}` (Jeans/Shorts/Pants) or `Skinned` (Skirt) | same | Partial |
| Shirt Colour `:475-480` (6 hex) | `palette.top` | material tint | **No** |
| Hat `:492-498` (5) | `slots[Hat]` → `Socket{HeadTop}` | rides the bound bone | Yes (4 meshes) |
| Glasses `:502-508` (5) | `slots[Glasses]` → `Socket{EyeLine}` | rides the bound bone | Yes (4 meshes) |
| *(not on the website)* Base body | `base_body` | `BaseBody::body_asset()` + `clip_prefix()` from one match — closes finding 45 and the `// TODO: Get biological sex from player profile` at `player_plugin.rs:855` | **No** |

**Ship the "No" column first.** Skin, hair colour, shirt colour, height and build are material writes and bone scaling against the two GLBs already in the repo. They make the website's promise partially true in one pass, before a single garment mesh exists. Hair/hat/glasses slots resolve to `None` and render nothing until art lands — which is honest, and the `SlotFit` machinery is testable with a placeholder cube on `HeadTop`.

**3D preview:** server-rendered from `eustress-headless --render gpu`, cached in R2 keyed on `content_hash()`, 24 pre-rendered yaws for a turntable. Not a wasm glTF viewer — a second renderer is a second parity liability, which is the exact failure this design exists to prevent. Until `--render gpu` lands (known P6 of the headless plan) the placeholder stays a placeholder. Delete the vestigial `WebGl2RenderingContext` feature from `web/Cargo.toml`.

**Persistence:** users already live in Cloudflare KV, not `backend/src/db.rs` (which creates 6 tables, none for users). Add an `avatar` JSON field beside `avatar_url: null` (`infrastructure/cloudflare/api/src/index.js:672`) plus `GET`/`PUT /api/community/users/:name/avatar` with server-side `AvatarCatalog::validate()`. **No SQL migration.** Today the worker has *no* PUT/PATCH profile route at all — "Save Avatar" has literally nowhere to send data.

### 5.4 File-by-file change map

**New: `eustress/crates/avatar-schema/`** (workspace member; `serde` + `serde_json` + `toml`; optional `bevy_ecs`/`bevy_reflect` behind a `bevy` feature; **no glam**)
- `descriptor.rs` — everything in §5.1
- `metrics.rs` — `BodyMetrics`, `metrics()`, `ResolvedMotion`, `resolve()`
- `catalog.rs` — `AvatarCatalog`, `CatalogEntry`, `validate()`, the 21 verbatim swatch constants
- `hash.rs` — canonical content hash (declaration-order byte encoding, never `serde_json` of anything map-shaped)

**New: `eustress/crates/common/src/avatar/`**
- `mod.rs` — `AvatarRuntimePlugin`, `AvatarHost`, `HostSeams`, `AvatarSystems`, `SpawnedByAvatarRuntime(())`, Gate-4 boot assertions
- `required_bevy.rs` — Gate 3
- `boot.rs` — `register_avatar_asset_sources()`: moves `bundled://` out of the engine crate, adds exe-adjacent-first resolution, adds `avatar://` over the existing `AssetResolver` (`common/src/assets/resolver.rs`, 496 lines, multi-source with LRU, currently registered by nobody)
- `spawn.rs` — the ONLY code in the repo that creates a character entity
- `rig.rs` + `bind.rs` — `canonical_bone_key`, `BONE_ALIASES`, topological fallback, `SceneInstanceReady` binder, bind-pose measurement, socket creation, `CharacterLimb` insertion, signature
- `retarget.rs` — source-map reconstruction from the clip `Gltf`, curve rekey, Hips translation rescale, `jump.glb` slicing, foot-plant + authored-speed analysis, `RetargetCache` keyed on `(clip AssetId, rig.signature)`
- `graph.rs` — `add_blend` / `add_additive_blend_with_mask` / mask groups from `rig.target_ids`
- `router.rs` — `route(&MotionParams, &BodyMetrics, dt, &prev) -> MotionWeights`, pure and total
- `locomotion.rs` — kinematic controller on `MoveAndSlide`, ground probe, coyote/buffer, step-up, and the single `LocomotionController` producer
- `ik.rs` — two-bone analytic solver, foot lock, pelvis levelling, look-at cone
- `spring.rs` — Verlet chain solver
- `procedural.rs` — breathing/exertion, idle break, turn-in-place, lean, landing flex
- `wearables.rs` — catalog/marketplace resolution, socket attach/detach on `Changed<AvatarDescriptor>`, palette tinting, hide masks
- `appearance.rs` — the ~500 lines moved from `engine/src/interaction/appearance.rs`, re-keyed onto rig-bound `CharacterLimb`, taking `Srgb8` not BrickColor ints
- `lod.rs` — 4 tiers, per-`content_hash` merged mesh + 2048 atlas, impostor tier over `engine/src/billboard_pipeline.rs`
- `parity.rs` — `assert_avatar_contract()`, the `AvatarContract` fold

**Modified — common**
- `plugins/skinned_character.rs` — delete `spawn_skinned_character`; delete `apply_skinned_character_facing` (`:306`, half of finding 47); delete `MESH_VERTICAL_OFFSET` and the 1.83/0.33 constants; delete the `Quat::from_rotation_x(FRAC_PI_2)` at `:239` **after visual confirmation**; fix or delete the crossed `SkinnedCharacter::new` gender map (`:143`)
- `plugins/character_plugin.rs` — delete the four empty stubs (`:418-443`) and the three dead imports (`:148-150`); delete `update_character_facing` (`:446`, other half of 47); `spawn_play_mode_camera` gains `Projection` (FOV from tuning) + `DepthPrepass` + spawn-relative placement + `BodyMetrics.eye_height` instead of the hardcoded 1.5 (`:517`); Escape moves to `HostSeams::escape_action`
- `plugins/animation_plugin.rs` — delete the five-checkmark banner (`:1008-1013`), the flat clip graph (`:343-351`), `update_locomotion_blend` (`:563-599`, the fade-to-nothing bug and the framerate-dependent `0.016` at `:584`), the global `playing_animations_mut()` retime (`:621`), `raycast_ground`'s unconditional `None` (`:751-759`), `apply_root_motion`'s empty body (`:840`), and both `DirectionalBlend` systems
- `services/animation.rs` — delete `default_transitions`, `BlendTree1D/2D`, `IKTarget`, `AnimationLayer/LayerBlendMode/LayerMask`, `CharacterAnimationBundle`, `AnimationService`; `LocomotionController` gains `yaw_rate`/`lateral_accel`; `HumanoidRig` superseded by `AvatarRig`
- `services/player.rs` — `avatar_data` retyped; delete `BiologicalSex::character_model`/`character_gender` (`:983-996`); `Character`'s speed fields become a read-through view of `ResolvedMotion`; insert `PlayerProfileCache` as a real resource
- `plugins/humanoid.rs` — delete `spawn_humanoid_character` (a `Capsule3d` + `Sphere` behind a doc comment claiming "full skeletal hierarchy"), `animate_arm`, `animate_leg`, `apply_procedural_limb_animation`, `update_head_look_system`; keep `CharacterLimb` solely as the appearance key
- `properties.rs` — `Humanoid::list_properties` returns all six names (`:302-309` silently drops `HipHeight` and `AutoRotate`); add `PropertyAccess` for `AvatarDescriptor`
- `Cargo.toml` — `physics` into default features (the feature gate is what let `raycast_ground` ship as an unconditional `None`)

**Modified — engine**
- `app_core.rs` — `register_asset_sources` delegates to `common::avatar::boot`; extract `build_app()`
- `play_mode.rs` — the three `add_plugins` at `:1547-1553` become one `AvatarRuntimePlugin::new(AvatarHost::Studio)`; exclude `SlintOverlayCamera` from the camera-disable loop (`:826`)
- `play_mode_runtime.rs` — delete `PlayModeCharacterConfig` (`:53-67`, the hardcoded Female); spawn via `SpawnAvatar`
- `ui/slint_ui.rs` — move `InteractionPlugin` (`:1355`), the spawner groups (`:1313-1338`) and `LightClassPlugin` into the shared core
- `interaction/equip.rs` — the accessory TODO (`:238-241`) resolved via `AvatarRig.sockets`
- `interaction/appearance.rs` — deleted; BrickColor table demoted to an import adapter
- `part_selection.rs`, `keybindings.rs`, `camera_controller.rs` — play-mode gates
- `space/instance_loader.rs` — handle the `[avatar]` block (the file has **zero** occurrences of "humanoid" today)
- `spawners/character7/avatar.rs` (new) + `spawners/character/humanoid.rs` (new) — `ClassSpawner`s so both round-trip losslessly through `fold_spawner_properties` (`world_db_binary.rs:275`), the only live class-registry consumer
- `ui/file_event_handler.rs` — `prepare_publish_manifests` (`:966`) gains `[required_assets]`
- `bin/avatar_pack.rs` (new) — creator CLI: GLB → bind check → `ItemValidation` → auto-LOD via `mesh_optimizer.rs` → fragment → content hash → upload

**Modified — client**
- `main.rs` — call `register_avatar_asset_sources` before `DefaultPlugins`; `UnapprovedPathMode::Allow`; exe-adjacent-first `file_path` (currently `"../common/assets"` at `:61`); `build_app()`; `AvatarRuntimePlugin::new(AvatarHost::Client)`; 60 Hz / 33 ms; `RenderPlugin` override
- `plugins/player_plugin.rs` — delete the procedural branch (`:303+`) **after** porting its working `RigidBody`/`Collider`/`SweptCcd`/`LockedAxes` insert into `avatar/spawn.rs`; delete the dead block `:1370-1750`; delete the hardcoded `BiologicalSex::Male` (`:856`)
- `systems/scene_loader.rs` — replace the deprecated RON/JSON readers with the R2 `.pak` path (§3)
- `plugins/{character_controller,animation_plugin}.rs` — **deleted**
- `Cargo.toml` — six Bevy features restored, now enforced by `required_bevy.rs`

**Modified — web / worker**
- `web/Cargo.toml` — depend on `eustress-avatar-schema` with `default-features = false`; drop `WebGl2RenderingContext`
- `web/src/pages/profile.rs` — one `RwSignal<AvatarDescriptor>`, 11 bindings, dropdowns and swatch rows **generated** from the catalog and swatch constants, `Save Avatar` (`:513`) PUTs, header image (`:187`) reads the `avatar_url` the component already fetched at `:123`
- `web/src/api/avatar.rs` (new), `web/src/api/marketplace.rs` — `MarketplaceItem` gains `payload: Option<AvatarFragment + bundle_hash + ItemValidation>`
- `infrastructure/cloudflare/api/src/index.js` — `avatar` KV field; GET/PUT avatar route; replace the marketplace stub (`:560-562`); `PUT/GET /api/avatar-assets/{hash}`

---

## 6. Procedural animation & realism plan

Implementation order. Each item names its Bevy 0.19 / Avian 0.7 mechanism. Every bone write runs in `AvatarSystems::PostAnim`, ordered **`.after(bevy::animation::AnimationSystems).before(TransformSystems::Propagate)`** — today `apply_foot_ik_to_bones` and `extract_root_motion` are in bare `PostUpdate` (`animation_plugin.rs:987-994`) with no constraint, so even once `AvatarRig` exists their writes would be ambiguously ordered against `animate_targets` and silently stomped. That failure looks like "IK does nothing" with no error anywhere.

**1. Rig bind + clip rekey** *(prerequisite for all of the below)*
`SceneInstanceReady` (not a `Children` poll — that races the loader) → canonical-key match → insert `AvatarRig` with `bone_paths` and memoised `AnimationTargetId::from_names`. Then rekey: load each clip file as a `Gltf` asset (not just `#Animation0`), reconstruct its node paths, canonicalise, build `source_map: HumanoidBone -> AnimationTargetId`, and rebuild a fresh `AnimationClip` — `AnimationCurves = HashMap<AnimationTargetId, Vec<VariableCurve>, NoOpHash>` at `bevy_animation-0.19.0/src/lib.rs:161`, `curves()` at `:220`, `curves_mut()` at `:226`, `add_variable_curve_to_target` at `:305`, `AnimationClip: Clone`. Cost: 8 clips × 2 rigs = 16 rebuilds at load, amortised to zero after the first spawn per signature.

> **Flagged uncertainty (graft from ASSEMBLY).** `VariableCurve` is `Box<dyn AnimationCurve>`; `AnimationCurve` exposes `sample_clamped` and is not downcastable. The **rekey** is a pure map rebuild and is safe. The **Hips translation rescale** and the **120 Hz foot-plant sampling** both need to *read* curve values, which may not be possible through this API. **Spike this on day one of the phase, not day four.** Fallback: move both to GLTF load time using the `gltf` crate (already an optional dep in `common` behind the model-import feature) and read the accessors directly. Cost of the fallback is ~2 days.

**2. Physics body + kinematic controller**
`RigidBody::Kinematic` + `Collider::capsule(m.capsule_radius, m.capsule_cylinder_len)` + `CustomPositionIntegration` + `CollisionMargin(0.02)` + `Mass(m.mass_kg)`. **`Friction::new(0.0).with_combine_rule(CoefficientCombine::Min)`** — the commented-out `Friction::new(1.0)` would glue the player to every wall. Ground detection is a `ShapeCaster` **sphere** cast of radius `0.9 × capsule_radius` down `0.35 m`, not a ray: a ray off a ledge edge reports airborne and stutters. Movement via `MoveAndSlide::move_and_slide(...) -> MoveAndSlideOutput { position, projected_velocity }`. Step-up is a forward `cast_move` at `step_height` plus a downward confirm applying a **position correction**, never an impulse. Coyote 0.12 s, jump buffer 0.10 s. Jump sets `v = sqrt(2·g·apex)`, so "jump 0.95 m" is a descriptor number rather than a magic 50.0 or 5.5.

**3. The router — a total function replacing the state machine**
`route(&MotionParams, &BodyMetrics, dt, &prev) -> MotionWeights`. No `Query`, unit-testable. Delete `AnimationStateMachine::default_transitions()` and `request_transition`'s `return false`. The existing 15-edge table has no `Idle→Run`, no `JumpAir→Idle/Walk/Run` and no `Falling→*`, so the first landing after physics is restored would strand the avatar permanently. A total function makes stranding **unrepresentable**, not patched. All smoothing uses `alpha = 1 - (-rate·dt).exp()`, never a hardcoded `0.016`.

**4. Graph rebuild on Bevy's own nodes**
Every node is `play()`ed exactly **once** at graph build; steady state only moves weights. That single decision erases the entire "source node was never playing" crossfade bug class.

```
root
├── ground   add_blend(1.0, root)          // children NORMALISED by Bevy
│   ├── idle / walk / run
│   └── [strafe_l/r, turn_l/r reserved: Option<AnimationNodeIndex> = None]
├── air      add_blend(0.0, root)
│   ├── jump_up (Once) / fall_loop (Forever) / land (Once)
└── additive add_additive_blend_with_mask(0.0, M_LOWER, root)
    └── breathe / lean / look_offset
```

Because `ground` is a `Blend` node, Bevy normalises `idle/walk/run`. The "walk at `1.0 - blend` with an unplayed run node fades the character to nothing as it speeds up" bug (`animation_plugin.rs:588-598`) becomes **impossible**. Mask groups via `add_target_to_mask_group(target_id, group)` (`graph.rs:672`) over `rig.target_ids`: `G_LOWER`, `G_UPPER`, `G_HEAD`, `G_ARM_L`, `G_ARM_R` — 5 of 64 bits used. Masks address bone **groups**, never individual bones, so the u64 budget never becomes a problem. Clip completion from `AnimationPlayer::is_finished` (`lib.rs:553`) / `just_completed` (`:682`) — the first real writer for `AnimationFinishedEvent`, and what stops the jump clip playing `RepeatAnimation::Forever` (`animation_plugin.rs:546`).

`jump.glb` is sliced into three sub-clips at retarget time by cropping curve domains, apex located at the max of the Hips translation-Y curve. That is what makes `JumpStart`/`JumpLand`/`FallStart`/`FallLand` — currently unreachable in `get_animation_state()` — reachable at all, on an 8-clip budget.

**5. Anti-slide, layer 1: measured stride matching**
`WALK_ANIM_SPEED = 1.6` / `RUN_ANIM_SPEED = 4.0` (`animation_plugin.rs:41-50`) are guesses. Replace with measurement: sample `LeftFoot`/`RightFoot` in model space at 120 Hz across the clip; a plant is an interval where foot planar speed < 15% of its cycle peak; `authored_speed = 2·mean_stride / duration`. Store `PhaseWindow`s per foot on `RetargetedClip`. Then `playback_rate = planar_speed / (authored_speed · stride_scale)`, clamped `[0.60, 1.60]`, applied **per node** to `walk`/`run` only — never via `player.playing_animations_mut()` (`:621`), which today retimes a jump clip mid-crossfade to walk speed. **This is the mechanism by which the website's Height slider is visible in MOTION, not only in silhouette.**

**6. Anti-slide, layer 2: foot lock — the one that actually works**
Playback-rate clamping can never eliminate sliding; a foot lock can. Per foot per frame:
1. Read the animated foot `GlobalTransform` from `AvatarRig` — not `char_pos ± 0.15`.
2. `spatial_query.cast_ray(foot + Y·0.5, Dir3::NEG_Y, 0.8, true, filter_excluding_self)` — a real Avian call replacing the unconditional `None`.
3. `locked = grounded && (clip phase inside a PhaseWindow || foot planar world speed < 0.15·stride_scale)`. On the locking frame, latch world pos/normal/yaw.
4. While locked the IK target is the **latched world position**, so the foot does not slide however wrong the rate is. Unlock on phase-window exit, `reach_ratio > 0.92`, or ground loss; ramp weight to 0 over 0.12 s.
5. **Graft from ASSEMBLY:** also warp the IK target's *horizontal offset* so `stride_length · cadence == |ground velocity|` exactly, rather than letting the clamp leak.

**7. Two-bone analytic IK**
Law of cosines with a pole vector, ~60 lines, used for **both** feet and hands. Segment lengths come from `RigMeasurements`, measured, not assumed. Pole = hip position + character forward × `0.35 · leg_length`, so knees never invert or wander. `IKTarget`/`IKLimb` (`animation.rs:711-732`, currently zero references) finally get a solver.

**8. Ground adaptation**
Pelvis drop = `min(0, left_delta, right_delta)` through a critically damped spring (ω = 12, ζ = 1); **feet re-solved after the hip moves** so targets stay on the ground. Foot orientation `Quat::from_rotation_arc(Vec3::Y, normal)` with pitch clamped to 35°. IK weight faded to 0 above `1.4 × run_speed` and while airborne — IK on a fast run reads worse than the clip.

**9. Body-scale applicator (composes, does not overwrite)**
**Verified constraint (§0.2): the clips key `scale` channels.** So the height/build applicator runs in `PostAnim` and multiplies the animated local scale rather than assigning it. Height is uniform on the armature root (exact). Build is a bounded non-uniform XZ scale on Hips/Spine/UpperArms/UpperLegs with a compensating inverse XZ on each immediate child so girth does not cascade into limb length — a plausible silhouette, **not real anatomy**. Say so in the UI copy or the Build slider will feel broken at the extremes. Real anatomy needs morph targets, and both shipped bodies have zero.

**10. Look-at with a cone**
Yaw **and** pitch distributed Spine2 20% / Neck 30% / Head 50%, clamped 20°/35°/45°, smoothed at rate 9. Beyond a 100° half-angle from body forward, weight decays to 0 over 0.3 s so the head never snaps behind the shoulder. `update_head_look_system` (`humanoid.rs:378-398`) already has the 40/60 split idea; it is yaw-only, queries the dead `CharacterBody`, and is imported at `character_plugin.rs:150` and never registered.

**11. Additive procedural layer — ported from the dead client block**
`client/src/plugins/player_plugin.rs:1371-1750` is genuinely good work (counter-phase shoulder swing, elbow flex coupled to shoulder phase, distinct airborne pump/brace, idle micro-sway) and it is `#[allow(dead_code)]`. Retarget it from primitive `CharacterLimb` entities onto `AvatarRig` bones and feed it through the masked `additive` node so it layers **on top of** the Mixamo clips. ~380 lines of dead code become the secondary-motion layer.

**12. Breathing + exertion**
`ProceduralAnimation::get_breathing_offset`/`get_sway_offset` (`animation.rs:812-842`) is correct math whose only caller is dead code. Wire as additive: Chest scale-Z `1 + 0.012·sin(φ)`, Spine1 pitch `0.006·sin(φ)`, Neck counter-pitch. Rate `12 + 22·exertion` breaths/min, amplitude `×(1 + 0.9·exertion)`, where `exertion` is a low-passed `speed_norm` with a 6 s decay. **A player who just sprinted keeps breathing hard for several seconds** — cheap, and it is most of what "alive" means.

**13. Landing impact**
`land_impact = clamp(-touchdown_vy / 8.0, 0, 1)`, latched on the grounding edge. Drives the one-shot `land` node, a knee flex of `-0.20 · leg_length · impact` released by a critically damped spring (ω = 14), and a camera dip of `0.12·impact` over 0.18 s. `AnimationEventType::Footstep` emitted at each plant onset — the first hook footstep audio has ever had.

**14. Turn-in-place (procedural, labelled)**
No clip exists. When `planar_speed < 0.15·stride_scale` and `|yaw_rate| > 0.6 rad/s`: hips counter-rotated by `-0.45 · turn_blend · 0.35 rad` against the root, pivot foot held by foot lock, free foot swung through a sine arc with a plant at the end. `LocomotionController.turning` — declared, `Reflect`-registered, never written — is the producer's output. `n_turn_left`/`n_turn_right` are reserved so a purchased clip displaces the stand-in with no code change.

**15. Idle break**
`IdleBreak { timer, next_at: rand(9.0..19.0) }` → hip lateral sway ±0.03 m over 1.4 s, a head glance to a random point in the look cone, one shoulder roll. Reserved node `n_idle_break`.

**16. Spring chains**
Semi-implicit Verlet in world space, 2 substeps at 60 Hz, per node: velocity damping, gravity, stiffness pull-back toward the animated pose, `enforce_length` against the parent, `push_out_of` self-colliders (head/chest capsules), `clamp_cone`. Then convert back to local rotations by aiming each bone at its simulated child. An `inertia` term blends in root frame velocity so hair lags on a sprint start. **Length constraint + cone limit + self-collision is what stops chains inverting.** One solver serves hair, coat hems, ponytails, backpack straps and every marketplace accessory that declares `spring_roots`. Highest visual-fidelity-per-line item available.

**17. Root motion policy**
**Off** for locomotion — velocity comes from input, and stride matching + foot lock carry the look. **On** for emotes and any future get-up. `extract_root_motion` finally matches its query once `AvatarRig` exists; `apply_root_motion` (a zero-parameter empty fn today) becomes a real system feeding `MoveAndSlide`.

**Not in scope, stated plainly:** ragdoll, facial FACS, cloth simulation. Eight clips is the authored ceiling; turn-in-place, idle-break, landing flex, lean, breathing and all secondary motion here are **procedural**, and the reserved graph slots exist so purchased clips displace them without a code change. Do not call this AAA animation.

---

## 7. Phased roadmap

A phase is done when its **gate** passes, not when it compiles.

---

### P0 — Harness, asset sources, and truth *(S, ~3 days)*

Extract `build_app() -> App` from both binaries. Land `common/tests/avatar_parity.rs` Tier A. Move asset-source registration into `common/src/avatar/boot.rs` with exe-adjacent-first `bundled://` resolution and `UnapprovedPathMode::Allow`. Realign the Client's Bevy features and add `required_bevy.rs`. Delete the five-checkmark banner (`animation_plugin.rs:1008-1013`).

**Gate:** the parity test **compiles and FAILS** on today's code, printing a named diff: `bundled://` unresolved in the Client, `RigidBody`/`Collider` absent from the character component set on **both** sides, 64 vs 60 Hz, 45° vs 70° FOV, six missing features. Deleting `bevy_animation` from `client/Cargo.toml` produces a compile error in `eustress-common`. Re-adding `SkinnedCharacterPlugin` in either host produces `E0603`. Skipping `register_avatar_asset_sources` panics at startup with the remedy in the message. **A green test here means the test is wrong.**

---

### P1 — The character can stand, walk and jump identically in both shells *(L, ~7 days)*

`eustress-avatar-schema` crate with the full type set. `AvatarRuntimePlugin`, `SpawnedByAvatarRuntime(())`, `SpawnAvatar`, `HostSeams`. `BodyMetrics` with `capsule_cylinder_len`. Kinematic controller on `MoveAndSlide` with the sphere ground probe, coyote, buffer, slope projection and three-cast step-up. The single `LocomotionController` producer. Delete the four stubs, `PlayModeCharacterConfig`, the Client's hardcoded `BiologicalSex::Male`, `character_controller.rs`, `client/animation_plugin.rs`, and both facing integrators (keep one). Port the Client's working physics insert into `avatar/spawn.rs` **before** deleting its procedural branch.

**Gate:**
- Tier A green. Tier B trajectory identical within `1e-4` over 600 ticks, first-divergent-tick report empty.
- Manually, **identically in both binaries**: the character falls under gravity, lands, stands on the baseplate, walks at `ResolvedMotion.walk_speed` within 2%, sprints, jumps to a measured apex within 5 cm of `jump_apex_m`, ascends a 30° ramp, is stopped by a 55° ramp, steps a 0.30 m ledge without jumping, and returns to a ground state after landing from 10 m.
- `cargo test -p eustress-avatar-schema`: JSON and TOML round-trip a fully-populated descriptor byte-identically; a property test over height × build asserts capsule bottom == spawn surface to `1e-5` and no metric is NaN or negative.
- `cargo build -p eustress-web --target wasm32-unknown-unknown` compiles the schema crate.
- `grep -rn 'fn spawn.*-> Entity' eustress/crates | grep -i char` returns zero rows.
- `grep -n '1\.83\|1\.375\|1\.015\|0\.585\|0\.635\|0\.24' ` over the character paths returns nothing.

*No cosmetic or animation work starts until this passes.*

---

### P2 — Rig bind + clip rekey: the limbs actually move *(M, ~5 days)*

**Day one: the `VariableCurve` readability spike.** Then `bind.rs` (canonicaliser, alias table, topological fallback, socket creation, bind-pose measurement, `CharacterLimb` insertion, signature) and `retarget.rs` (source map from the clip `Gltf`, curve rekey, Hips rescale, `jump.glb` slicing, plant + authored-speed analysis, `RetargetCache` keyed on `(clip, signature)`).

**Gate:**
- `RetargetedClip.bound_bones >= 60` for **all 8 clips against both bodies**, asserted in Tier B. *(Baseline today: x_bot 7, y_bot 61.)*
- `AvatarRig.unresolved` is empty on both bodies; 22/22 `HumanoidBone` entries bind.
- Visually: **x_bot's arms AND legs animate; y_bot's right leg animates.**
- Synthetic 1.45 m and 2.05 m descriptors both walk with feet on the ground and no root float (proves the Hips rescale, or its `gltf`-crate fallback).
- A hat parented at `HeadTop` tracks the head bone through a full walk cycle.
- Assert the applicator composes: with a clip playing, the descriptor's height still holds — i.e. the scale channels do not overwrite it.

---

### P3 — Motion graph and router *(M, ~4 days)*

`graph.rs` on `add_blend`/`add_additive_blend_with_mask`, mask groups from `target_ids`, `router.rs` as a pure total function, measured stride-matched playback, clip-completion detection. Delete the transition table, `BlendState`, both `DirectionalBlend` systems, `update_locomotion_blend`.

**Gate:** a 10-minute randomised input-tape fuzz never drives total clip weight below 0.98 across a 0 → 6 m/s sweep *(today it approaches 0 as you speed up)*; 200 consecutive jumps all return to idle/walk/run *(today the first landing strands permanently)*; the jump clip plays **once**, not forever; Tier B weight-vector equality green.

---

### P4 — Foot IK, foot lock, ground adaptation *(L, ~6 days)*

Two-bone solver, real Avian raycasts, world-space foot locking driven by the P2 phase windows, IK-target stride warping, critically damped pelvis levelling, clamped foot roll, weight fade above run speed. Ordering asserted `.after(AnimationSystems).before(TransformSystems::Propagate)`.

**Gate:** instrumented planted-foot world drift **< 2 cm per step** at playback rates 0.6×–1.6×, on flat ground and on a 25° slope, ascending and descending, and at both height extremes. No knee inversion across 10 000 randomised target positions in a unit test of `solve_two_bone`. Feet visibly conform to a stair edge; pelvis drops correctly on a 30° slope.

---

### P5 — Procedural life layer + secondary motion *(L, ~7 days)*

Exertion-scaled breathing on the masked additive node, idle break, lean, procedural turn-in-place, landing impact with knee flex and camera dip, look-at cone, footstep events, spring chains, the ported additive arm swing.

**Gate:** standing still for 60 s produces ≥ 3 distinct idle breaks and never a T-pose frame; sprint-then-stop shows breathing amplitude decaying over ~6 s; a 4 m drop produces a visible knee flex that recovers without overshoot; head tracks a moving target and releases smoothly past 100°; a 6-node hair chain settles within 1.2 s, never inverts across a 10-minute fuzz, never penetrates the head capsule, and is stable at both 30 Hz and 240 Hz fixed steps. **The deleted banner returns one line at a time, each backed by a test asserting a measured quantity.**

---

### P6 — Descriptor persistence, website, slots, marketplace *(L, ~8 days)*

KV `avatar` field + GET/PUT routes with catalog validation. 11 Leptos signals; dropdowns and swatch rows generated from the catalog. `SlotFit::Skinned` joint remapping, `SlotFit::Socket` attach, `SlotFit::Decal` wired to the relocated `appearance.rs`. `avatar://` asset source. `MarketplaceItem.payload`. `avatar_pack` CLI with `ItemValidation` (`MAX_BIND_DEV_M = 0.02`) and auto-LOD. `[required_assets]` in the publish manifest. Register the `Avatar` and `Humanoid` `ClassSpawner`s.

**Gate:**
- Change hair colour, skin tone, height and build on eustress.dev, press Save; `wrangler kv key get` shows the descriptor verbatim; a fresh launch of **both** binaries for that account spawns exactly that descriptor, verified via MCP `avatar.get` matching the JSON byte-for-byte — **never by driving the engine UI** (computer-use is unavailable on this machine).
- An invalid descriptor (hat id in the glasses slot, unknown catalog id) is rejected at the API boundary with a 400.
- A placeholder hat stays attached through a jump and a 2.05 → 1.45 m height change.
- A deliberately over-budget item fails `avatar_pack` with a readable reason.
- Tier A/B still green with descriptor-driven spawns.

---

### P7 — Crowds *(M, ~5 days)*

`AvatarLod` tiers, per-`content_hash` merged mesh + 2048 atlas bake, impostor tier over the existing billboard pipeline, `live_l0_budget`, thresholds folded into the contract hash.

**Gate:** 500 avatars across ≥ 20 distinct descriptors hold ≥ 60 FPS **in the microprofiler** on the reference machine, with draw calls flat in slot count; no visible pop at the configured LOD distances.

---

### Explicit stop line

**P0–P2 fixes every blocker that makes the avatar unplayable and is shippable on its own.** P3–P5 make it good. P6 makes it match the website. P7 makes it survive a crowd.

If time compresses, **cut P7 first, then P6's marketplace half (keep persistence + the 11 signals), then P5's spring chains.** Do not cut a gate to make a phase pass — a half-wired IK system is exactly the state the repo is in today, and it is the state this plan exists to end.

---

## Residual risks and open uncertainties

- **`VariableCurve` opacity** — the single unguarded load-bearing assumption. `AnimationCurve` exposes only `sample_clamped` and is not downcastable. The rekey is safe; the Hips rescale and the 120 Hz foot sampling may not be. **Spike day one of P2.** Fallback via the `gltf` crate costs ~2 days.
- **The `FRAC_PI_2` rotation fix** — the asset data says it is wrong (Armature has scale 0.01 and no rotation; joints are Y-up). I have **not** run the engine. If something downstream compensates, removing it will visibly break the pose. Verify visually before the commit lands.
- **Cargo feature unification** — Gate 3 holds for `cargo build -p eustress-client` but a whole-workspace build unions features across members and would mask a client-side omission. CI must run per-binary builds explicitly.
- **Kinematic controller** — loses free dynamic-prop pushing. Needs an explicit impulse from the `on_hit` callback. Anyone expecting the old behaviour will report it as a regression; put it in the commit message.
- **Unit reconciliation touches authored content** — `Humanoid` at 16 studs/s = 0.798 m/s under `units.rs:127` vs `Character` at 1.8 m/s is a 2.25× gap. Any existing scene with an authored `WalkSpeed` changes behaviour on load. Needs a `[metadata] unit` check and a one-time migration note.
- **Scope of the parity guarantee** — this covers the **avatar surface only**. Findings 13/14/16/18/20/23/25/26/27/29 (Client sun, light-class sync, terrain streaming, material sync, script hosting, GUI rendering) stay open after P7. **A green avatar parity test must not be read as "the Client matches Play Mode."** What this establishes is a reusable pattern — sealed token, host enum, `build_app()`, contract fold, replay test — that each remaining subsystem can adopt. Design the `AvatarContract` fold to be extensible so `LightingContract` and `ScriptContract` are additive rows, not rewrites.
- **UGC policy** — a creator marketplace needs a review queue and a licensing answer relative to PolyForm Shield **before** public UGC opens. That is a policy blocker on P6 shipping publicly, independent of the code.
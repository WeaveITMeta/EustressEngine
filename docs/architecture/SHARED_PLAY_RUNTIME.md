# Shared Play runtime

Studio's Play session is the one authority for a running Space. Its DataModel tree
replicates to every joined Player, and the Player draws that tree with the same code
Studio's Play uses. The code lives once, in a crate both apps link, so what a player sees
matches what the host sees by construction.

McKale, 2026-09-24: "replicate to the client from the runtime of Studio and just do it
right", and "Just make it out to be correct for production."

Status: approved 2026-09-24, with the HUD and mesh decisions in sections 6 and 7.

## 1. Where things stand

**Studio.** `engine/src/play_datamodel/` (5,647 lines) runs a Play frame in four sets:

```text
PlayScriptSet::Pull     remote players, poses, input, mouse ray, collisions,
                        GUI clicks, the local character          (ECS -> tree)
PlayScriptSet::Scripts  drive_luau                               (Luau on the tree)
PlayScriptSet::Apply    apply_frame, npc, camera, audio, particles,
                        commerce, drain_output                   (tree -> ECS)
PlayScriptSet::End      trim events to the VM's cursor, end_frame
```

At Play start, `seed.rs` builds the tree FROM the ECS: every entity the Space loader made
is bound into the tree with `create_bound`. So in Studio, `apply.rs` spawns only what
scripts create, and otherwise writes properties onto entities that already exist.

**The Player.** It builds its tree from the downloaded Space (`common/src/tree_read.rs`),
replication writes into it (`client/src/systems/net_replica.rs`), LocalScripts run on it
(`common/src/tree_scripts.rs`), and `common/src/tree_apply.rs` (636 lines) draws it. Every
part arrives as a spawn, so on the Player the spawn path is the main path. `tree_apply`
draws parts and the Space's scripted camera only.

What the Player lacks today, found while mapping both sides:

| Missing on the Player | Effect |
|---|---|
| ScreenGui and BillboardGui drawing | No HUD, no name plates or price tags. HUD LocalScripts build GUI in the tree that nothing draws |
| Sounds, particles | `Replica::apply` queues `sound_commands` and `particle_emits`; nothing drains them |
| Lights from the tree | Lights come from the Space's files only, so a host's light changes never arrive |
| Lighting writes | `Lighting.ClockTime` and friends reach the tree but never `LightingService` |
| The frame clock and input in the tree | Nothing writes `frame.dt`, `frame.time`, `input` or `mouse`: LocalScripts run with `dt = 0` and no input |
| GUI clicks | No hit test, so no `Activated` on the Player |
| A camera without an avatar | The only `Camera3d` is the avatar's (`avatar/control.rs:175`); a Space with `CharacterAutoLoads` off (Pong) draws nothing |
| Queue draining | `output`, `humanoid_commands`, `physics_commands`, `terrain_commands`, `touch_watch` grow every frame; `end_frame` never clears them |
| Materials | Parts get `space_read::part_material`, not Studio's material library (`MaterialRegistry`) |
| Custom meshes | A `MeshId` draws as a block |
| Event trimming | `tree_apply` trims to `event_cursor()`, not to the VM's cursor, which forces a strict ordering on everything that writes the tree |

## 2. The split

| Code | Goes to | Why |
|---|---|---|
| `apply.rs` (tree to ECS: spawns, reparents, property writes, destroys, lights, GUI, billboards, lighting) | shared crate | The draw step. Both apps run it |
| `camera.rs` + `tree_apply`'s camera step | shared crate, merged | One scripted-camera system; the camera it follows is per machine (section 4) |
| `audio.rs` | shared crate | Sounds the host's scripts play reach the Player through `sound_commands` already |
| `particles.rs` + `particles/render.rs` (449 lines + `particle_cloud.wgsl`) | shared crate | Same, through `particle_emits`. The Player has no particle renderer |
| `material_sync.rs` (788) + the `MaterialRegistry` half of `space/material_loader.rs` (1,133) | shared crate | Studio's part look comes from `sync_basepart_to_material`, which reacts to `Changed<BasePart>` after `apply` spawns. Without it the Player cannot match |
| `rebuild_collider_on_size_change` (`scale_tool.rs:183`) | shared crate | A script's `Size` write in Play relies on it, and it lives in an editor tool plugin today |
| `billboard_gui.rs` draw section (about 2,600 of 3,576 lines) + `billboard_pipeline.rs` (1,170) | shared crate | Billboards. The TOML save-back, label editing and class-to-display authoring syncs stay in the engine |
| GUI layout and hit test (`resolve_gui_rect`, the hidden-ancestor walk, the visibility filter, the click hit test from `update_slint_ui_focus`) | shared crate | The HUD (section 6) |
| `gui_loader::gui_display_from_props` (pure TOML to `GuiElementDisplay`) | eustress-common | The Player's reader needs it; the rest of `gui_loader.rs` is loading and authoring |
| `pull_frame_state`'s clock and input half, `pull_mouse_hit`, `pull_gui_clicks` | shared crate | Per machine: each machine feeds its own input to its own LocalScripts |
| `drain_output` | shared crate, with a sink hook | Studio forwards to its Output panel; the Player logs |
| `end_frame` | shared crate | Trims to the VM's cursor on both |
| `seed.rs`, `pull_poses`, `pull_collisions`, `pull_character`, `remote_players.rs`, the host side of `commerce.rs`, `net_replicate.rs` | stay in the engine | Host only: they create the tree and read the simulation |
| `npc.rs` | stays in the engine | It simulates (writes `LinearVelocity`, answers `MoveTo`). On the Player, NPC motion arrives through the motion lane and NPC rigs animate through the Animator |
| `apply_script_terrain_edits` | stays in the engine, as a hook | Uses `terrain_commands` with the editor's undo stack; terrain edits are not replicated yet |
| The Studio overlay, `gui_containers/*` spawners, `gui_loader.rs` IO | stay in the engine | Edit-mode authoring and file IO |

Per machine, never replicated: each player's camera, input, LocalScripts, audio listener,
Output sink, and viewport.

## 3. The crate

`eustress/crates/play-runtime`, package `eustress-play-runtime`, library
`eustress_play_runtime`. It depends on `eustress-common` (features `physics`, `gui`,
`luau`), `eustress-networking` (for `ReplicationTap`; both apps link it already),
`avian3d`, `bevy` (adds `bevy_audio`, which common does not enable), `tiny-skia` 0.11,
`cosmic-text` 0.12 and `texture-gen` (the material registry's mips). The engine and
the client both depend on it. It also takes the engine's rebuild load down: the engine
rlib is near the 4 GB archive limit.

The existing `crates/runtime` (`eustress-runtime`, 803 lines, only `crates/server` links
it) predates Bevy 0.19 and has its physics disabled. It is left alone; retiring it is a
separate task.

**What lives in eustress-common instead.** `common/src/tree_scripts.rs` orders itself
against the draw step and reads the tree resource, and common cannot depend on the new
crate. So the tree resource and the frame's system sets live in common (a few dozen
lines), and the systems live in the crate:

- `PlayDataModel { dm }` moves from `engine/src/play_datamodel/mod.rs` to common and
  replaces `PlayerTree`.
- `begin_session(dm, local_player, &SessionStart)` in `play_session` is the one session
  setup both apps run: `Players.LocalPlayer`, `Workspace.CurrentCamera`,
  `workspace.Gravity`, `Players.RespawnTime` and `Players.CharacterAutoLoads`. Each app
  makes its local `Player` its own way first (Studio's joins after the server scripts
  start; the Player's sits under Players before replication binds). The camera rule is
  the one difference: Studio takes a fresh camera each session (`SessionCamera::Fresh`),
  since the Space's saved camera is the editor's; the Player shows the Space's own
  camera (`SessionCamera::SpaceOrFresh`).
- `PlayRole` resource: `Authority` (Studio's Play, hosting or alone) or `Replica` (a
  joined Player). `FromTree(InstanceId)`, `PlayAssetRoot` and `PlayEventCursor` live
  beside it in `play_session`.
- `PlayScriptSet { Pull, Scripts, Apply, End }` moves to common. `TreeApply` and
  `RunTreeScripts` become aliases for `Apply` and `Scripts` during the switch, then go.
- `DataModelSpawned`, `MeshSource`, `PartEntity`, `PlayModePhysicsActivated`,
  `ViewportBounds`, `PropertyCommand` and `part_type_to_glb_path` move to common. None
  of them depends on anything engine-only. The engine re-exports each from its old path,
  so no engine call site changes.

## 4. Roles

Where the two roles differ, `PlayRole` decides, inside the one implementation:

| Rule | Authority (Studio) | Replica (Player) |
|---|---|---|
| Body of an unanchored part | `Dynamic` + `PlayModePhysicsActivated` (unchanged) | `Kinematic`: the host simulates, only its writes and the motion lane move it |
| Anchored part | `Static` | `Static` |
| A part's ECS parent | the tree's parent entity, pose through `local_transform` (unchanged) | none: a part's `CFrame` is its world pose, and the motion lane writes world poses into `Transform`. GUI objects and billboards keep `ChildOf` on both, since layout and anchoring read it |
| `CanCollide` off | `Sensor` when `CanTouch` or `CanQuery`, else no collider (unchanged) | Same as Studio, so LocalScript raycasts hit `CanQuery` parts. (`tree_apply` gave no collider at all) |
| Impulses (`physics_commands`), `touch_watch` | applied | dropped each frame |
| Terrain edits | the engine's hook | dropped each frame (not replicated) |
| The scripted camera | follows `Workspace.CurrentCamera` (unchanged) | follows `Workspace.CurrentCamera`, which only this machine's scripts set: replication never sends it. With no avatar, the play camera shows that camera's pose whatever its `CameraType`, as the Space's own view |
| A play camera with no avatar | `ScriptedPlayCamera` from `handle_start_play` (unchanged) | the crate spawns the same camera when no avatar camera exists |
| Replication tap | feeds `ReplicationTap` when hosting (unchanged) | absent |
| `end_frame` trim | to the Luau VM's cursor (unchanged) | the crate's `end_frame`, to `PlayEventCursor`, which the VM's owner (`TreeScripts`) sets each frame; to `event_cursor()` only when no VM exists |

Rules both roles follow (these change nothing in Studio except where marked):

1. **Only Workspace draws parts.** Parts elsewhere spawn hidden with `ColliderDisabled`,
   as Studio does today.
2. **Parts under a Character belong to the avatar runtime.** Studio never spawns them
   (`build_character` makes the `HumanoidRootPart` bound to the avatar body and the
   `Head` virtual). On the Player every replicated instance arrives as a spawn, so the
   draw step skips the Models some `Player.Character` points at, and undraws a Model's
   parts when a `Character` write names it (from `tree_apply`).
3. **The draw step despawns only what it spawned**, recognised by `FromTree` on the
   Player and `DataModelSpawned` in Studio. An entity bound to an avatar is never
   despawned or unbound (Studio already skips `AvatarBody`). Its property writes
   still apply: a Humanoid write (`WalkSpeed`, `JumpHeight`, `AutoRotate`, the
   ability switches) reaches the `AvatarBody` bound to the character's
   `HumanoidRootPart`, and a `CFrame` write to that root teleports and turns the
   avatar. On the Player that is the only way a host script's speed pad or
   teleport reaches the player's own avatar, since the owner simulates it.
4. **Replication writes through ordinary DataModel writes** (`Replica::apply`), so the
   draw step sees them as spawns and dirty properties like any script write.
5. **ScreenGuis draw only under `Players.LocalPlayer.PlayerGui`.** On the host, joined
   players' PlayerGuis exist, but their HUD belongs to their machines. This replaces
   Multiplayer's host guard `hide_joined_player_guis` (it goes once the rule lands).
   *Changes Studio:* today's overlay also draws a ScreenGui a script creates under
   StarterGui during Play, which Roblox does not.
6. **StarterGui originals never draw during Play.** Studio hides them at seed; on the
   Player they arrive with the downloaded world. `spawn_instance` counts StarterGui as a
   visible service (`apply.rs`, `"Workspace" | "Players" | "Lighting" | "StarterGui"`);
   the rule moves into the visibility filter so it holds on both.

## 5. Engine ties, and what replaces each

| Tie | Replacement |
|---|---|
| `crate::play_mode::PlayModeState` gating the sets | The sets run while `PlayDataModel` exists (`resource_exists`). Studio also keeps its `in_state(Playing)` condition on the sets |
| `crate::spawn::{MeshSource, part_type_to_glb_path}`, `crate::rendering::PartEntity`, `crate::play_mode::PlayModePhysicsActivated`, `crate::ui::ViewportBounds`, `crate::commands::PropertyCommand` | Moved to common (section 3) |
| `crate::space::space_asset_source::space_asset_root` | `PlayAssetRoot(PathBuf)` resource. Studio fills it from its asset source; the Player from `LiveWorld.space_root` or `OpenSpace.root` |
| `crate::terrain_commands::apply_terrain_commands` (with undo) | Not called by the crate. The engine keeps `apply_script_terrain_edits` as its own system in `PlayScriptSet::Apply`, ordered where it runs today |
| `crate::ui::slint_ui::OutputConsole` | The crate drains `output` into `PlayOutput` messages. The engine forwards them to the Output panel; the Player logs them |
| `crate::profiler::{phase_armed, phase_window}` | Environment-variable readers; copied into the crate |
| `crate::space::material_loader::MaterialRegistry`, `crate::space::file_loader::LoadInProgress` | The registry moves; `LoadInProgress` becomes an optional resource the engine inserts |
| `crate::particles::render` | Moves |
| `crate::billboard_pipeline::*` | Moves |
| `crate::space::file_loader::{bulk_load_active, ui_sync_tick}` in the billboard systems | An optional `DrawThrottle` resource; absent means every frame |
| `crate::ui::SlintUIFocus`, the scene tab, Slint popups, the purchase prompt, in the click hit test | An optional `GuiInputBlocked(bool)` the engine sets each frame; the Player sets it while its pause menu or a purchase prompt is open |
| `eustress_networking::repl::ReplicationTap` | Stays; the crate depends on eustress-networking |

## 6. The HUD: one GUI renderer

**Decided: Option 1.** One renderer draws the HUD and billboards in both apps, under
these conditions:

- **No full-screen re-raster.** Each GUI element rasterizes into its own atlas slot, as
  billboards already slot; only dirty elements redraw, and the GPU composites the slots
  as screen-space quads in z order, each clipped to its ancestors' clip rect.
- **Buttons and TextBox input** (clicks, focus, typing) go through the shared hit test,
  so they behave the same in both apps.
- **Measured on Box Head's HUD** (ammo, cash and timers changing), from the microprofiler,
  against this budget for the HUD's CPU work per frame: at most 0.1 ms when nothing
  changed, at most 0.5 ms in a frame where up to 5 elements changed, and at most 5 ms
  for a full rebuild, which happens only when a HUD opens or the window resizes.
- **Studio's Edit-mode StarterGui preview** stays on the overlay for now (section 12).

The options as weighed:

Studio draws a ScreenGui through its editor window: `sync_gui_elements_to_slint`
(`ui/slint_ui.rs:3136`) writes rows into the Studio window's `gui-elements` model
(`main.slint`), and `render_slint_to_texture` software-renders the whole window onto a
quad. The Player has no Studio window, so this code cannot run there. The overlay also
has limits: `clip` does not clip descendants, and font family, `TextScaled` and text
stroke are ignored. Billboards use a different renderer: the engine's CPU rasterizer
(tiny-skia and cosmic-text) into a texture atlas. Its `ImageLabel` draws a placeholder
rectangle.

- **Option 1, one GUI renderer for everything (recommended).** The billboard
  rasterizer also draws ScreenGuis, as a screen-space layer, in both apps. Layout,
  clipping, fonts, `TextScaled`, stroke and images come from one implementation, so the
  HUD matches by construction, and billboards and the HUD render text the same way.
  Work: a screen-space target for the rasterizer, real image drawing (for `ImageLabel`
  and `ImageButton`, in billboards too), and descendant clipping. Cost: Studio's Play HUD
  changes renderer, so its text looks slightly different from today. Studio's Edit-mode
  StarterGui preview stays on the overlay until a later switch.
- **Option 2, two painters sharing the layout.** Studio keeps its overlay; the Player
  gets its own painter. Both share `resolve_gui_rect`, the filters and the hit test, so
  element positions match, but text, clipping and images are painted twice and can
  drift. Studio's Play is untouched.
- **Option 3, bevy_ui on both.** Both apps draw the HUD with Bevy's UI nodes. The
  Player gains `bevy_ui` and `bevy_text`, Studio's Play HUD changes renderer as in
  Option 1, and billboards still use the rasterizer, so the HUD and billboards render
  text differently.

### How the one renderer is built

- **One rasterizer.** `billboard_gui.rs`'s drawing half moves into the crate as
  `gui/raster.rs`: `render_element`, `render_text`, the clip mask, rounded rects and
  the font state (`BillboardTextState`, cosmic-text plus a swash cache). It gains
  image drawing: an `ImageLabel` or `ImageButton` loads its file once (from the
  Space's folder), decodes it and draws it scaled into its rect, for billboards too.
  Nested clipping intersects every clipping ancestor's rect; today the nearest one
  replaces the rest.
- **One layout.** The UDim2 walk (`collect_subtree`: scale times the parent's extent
  plus offset, then `AnchorPoint`) lays out billboards against their pixel canvas
  and ScreenGuis against the 3D viewport's logical size. The hit test reads the same
  laid-out rects, so what is clicked is what is drawn.
- **Billboards** keep their atlas and pipeline (`billboard_pipeline.rs` and
  `billboard.wgsl` move into the crate unchanged). The editor's parts of
  `billboard_gui.rs` stay in the engine: the TOML save-back, the double-click label
  editing, and the class-to-display syncs that mirror Properties-panel edits.
- **The HUD** is drawn in panels. Each element directly under a drawn ScreenGui is
  one panel: its subtree rasterizes into the panel's own texture at the viewport's
  physical pixel density. A panel re-rasterizes only when its subtree's content hash
  changes, so an ammo counter repaints its own panel and nothing else.
- **Compositing after post-processing.** Panels are unlit textured quads on a
  dedicated overlay camera: orthographic in viewport pixels, its own render layer,
  no tonemapping, no bloom, and alpha-blended over the play camera's output. It
  sits just above the play camera's order and below Studio's Slint overlay (order
  300). Drawn inside the 3D pass, the HUD would be tonemapped, so white text would
  come out grey. Z order is the ScreenGui's `DisplayOrder`, then each element's
  `ZIndex`.
- **Which ScreenGuis draw** (rules 5 and 6): those under
  `Players.LocalPlayer.PlayerGui`, that are `Enabled`, with no hidden ancestor.
  StarterGui never draws in Play, and neither does any other player's PlayerGui.
- **Input.** The mouse position becomes viewport-local logical pixels. The hit test
  walks the laid-out rects from the top of the z order, skipping invisible elements,
  `mouse_filter = "ignore"`, and points outside a clipping ancestor. A press on a
  `TextButton` or `ImageButton` writes the same `GuiActivated` event
  `pull_gui_clicks` writes today. A press on a `TextBox` focuses it: typed
  characters, Backspace and the arrow keys edit its `Text` through the tree, and
  Enter or a click elsewhere fires `FocusLost`. A hit marks the input as
  game-processed. Studio sets `GuiInputBlocked` while its own UI has focus, a popup
  is open or a purchase prompt shows; the Player sets it for its pause menu.
- **Studio's switch.** In Play, `sync_gui_elements_to_slint` stops sending ScreenGui
  rows to the overlay, and the Play hit test in `update_slint_ui_focus` gives way to
  the shared one. In Edit, both stay as they are (section 12).
- **Measurement.** Microprofiler spans around the HUD's layout, raster and upload,
  read on Box Head's HUD against the budget above. The dev profile builds
  dependencies at `opt-level` 3 and workspace crates at 2, so tiny-skia and
  cosmic-text run optimized in the build that is measured.

## 7. Primitive meshes: one copy in common

**Decided: the recommended option below.** Common is the source of truth for assets.

Studio's script-made parts load `parts/<shape>.glb` from `engine/assets/parts` (six files,
80 KB). The Player ships `common/assets` only, which both apps already register as
`bundled://`. Saved Spaces also spell meshes as `parts/<shape>.glb`, so that spelling must
keep working.

- **Recommended:** the six files move to `common/assets/parts` and load as
  `bundled://parts/<shape>.glb` in both apps. One helper in common names the path, and
  one normaliser maps the saved `parts/<shape>.glb` spelling to it. The engine's 78
  references (20 files) move to the helper in one mechanical pass.
- **Alternative:** the Player keeps building primitive meshes in code
  (`space_read::primitive_mesh`). Nothing moves, but wedges and UVs differ from Studio's.

## 8. Frame order

**Studio** (unchanged; the crate's systems take the places of the moved ones):

```text
Pull     track/sync remote players, pull_frame_state*, pull_poses, pull_collisions,
         pull_character, pull_remote_characters, pull_gui_clicks*, pull_mouse_hit*
Scripts  drive_luau
Apply    apply_frame*, npc, camera*, net_replicate (after apply, before audio),
         audio*, particles*, commerce, output*, terrain edits (engine hook)
End      end_frame*, then Multiplayer's hide_joined_player_guis until rule 5 lands
         (* = runs from the shared crate)
```

**The Player** (Multiplayer's constraint: replication first, LocalScripts, the draw step
last; the Animation System's order inside):

```text
net_replica      bind_tree, apply_world_frames, apply_motion, receive_replies,
                 follow_characters (after AvatarSystems::Locomotion, as Studio's Pull is)
Pull             TreeClock, the frame clock and input, pull_mouse_hit, pull_gui_clicks
                 Animator tracks step
Scripts          run_tree_scripts
                 rigs driven, send_remote_calls
Apply            the shared draw step (apply, camera, audio, particles, output)
End              end_frame (trim to the VM's cursor)
```

Three edges the Player lacks today are added: `follow_characters` after
`AvatarSystems::Locomotion`, `open_requested_space` before the draw step, and
`player_pump` before `apply_world_frames`.

## 9. What `tree_apply` brings

Everything `tree_apply.rs` does that `apply.rs` lacks is kept (the full list, with the
reasons, went to the orchestrator as `TREE_APPLY_HANDOFF.md`):

1. The session setup (`begin_session`): `Players.LocalPlayer` created before replication
   binds, and the Space's own camera as `Workspace.CurrentCamera`.
2. Character Models skipped and undrawn on a `Character` write (rule 2).
3. `Static` and `Kinematic` bodies on a replica, never `Dynamic`.
4. Flat parts with world poses on a replica (section 4's table), and
   `space_read::clean_pose`'s non-finite guard on both roles (Avian panics on a
   non-finite pose or a zero extent).
5. Only Workspace draws (rule 1).
6. The climb rules' `Climbable` mark from the tree's attributes and rigs
   (`climb_mark`, `rigged_models`), because `spawn_instance` inserts `Attributes::new()`
   empty and gives a Humanoid no entity. Without it, anchored NPC parts would become
   climbable.
7. `FromTree(InstanceId)` on every drawn entity; `space_world` despawns them on a
   re-open, and the motion lane moves only them.
8. The scripted camera's screen-relative movement (`AvatarCamera.yaw` from the view),
   orthographic `FixedVertical` sizing and the perspective restore.
9. Its four tests, re-homed with the code.

## 10. Order of work

Every step leaves both apps building and behaving as before, except where the step says.

1. **Common first.** Move the small types and the sets (section 3) with re-exports.
   No behaviour changes.
2. **The crate, Studio only.** Create `crates/play-runtime`; move `apply.rs`,
   `camera.rs`, `audio.rs`, `particles.rs` + the particle renderer, `material_sync.rs` +
   `MaterialRegistry`, the resize collider system, `end_frame` and `drain_output`. The
   engine's `PlayDataModelPlugin` registers them in the same sets, in the same order.
   Studio's Play behaves exactly as before; `seed.rs`'s parity test stays green.
3. **GUI.** Move the billboard draw section and pipeline, the layout, filters and hit
   test; build the HUD per the decision in section 6; rules 5 and 6 land and Multiplayer
   deletes its guard.
4. **The Player switches.** The Player adds the crate with `PlayRole::Replica` in place
   of `TreeApplyPlugin`; `PlayerTree` becomes `PlayDataModel` in `net_replica.rs`,
   `tree_scripts.rs`, `space_world.rs` and the Animation System's `follow_player_tree`;
   `tree_apply.rs` is deleted; `space_world` stops spawning lights from files on the tree
   path (the tree has them); the Player's local open reads its Space through
   `tree_read` as a joined world does, so `space_read`'s part spawning retires; the
   frame clock, input and GUI clicks feed its LocalScripts. Also fixed here:
   `TreeScripts` is created only once, so a second world opening keeps the old VM on the
   old tree.
   Step 4 lands in parts, each type-checked in a scratch mirror before it touches the
   tree, and each leaving both apps building:
   - **4a, primitive meshes in common.** The six `parts/*.glb` move to
     `common/assets/parts`, the one folder both apps ship. Studio's default asset source
     gets a reader that looks in `engine/assets` and then in `common/assets`, so
     `parts/block.glb` resolves in both apps with no change to the 78 references or to
     saved Spaces, and the files exist once.
   - **4b, the replica role in the shared step.** `PlayRole` (in `play_session`) and
     everything section 4's table lists for a replica: `Kinematic` bodies, flat parts,
     `FromTree`, character Models skipped, the climb marks, the pinned camera, the play
     camera with no avatar, and dropping the queues a replica does not consume. Studio
     stays `Authority`, so nothing in Studio changes.
   - **4c, what a Space brings on the Player.** The material library loading
     (`file_loader.rs`'s `.mat.toml` walk) moves into the crate beside the registry, so
     the Player fills the registry when it opens a Space. The per-machine half of
     `pull_frame_state` (the clock, keys, mouse, `game_processed` from `HudPointer`)
     moves too, with `MouseRayState` and `InjectedInput`; exactly one of it and
     `TreeClock` advances the clock.
   - **4d, the switch.** The Player depends on the crate and adds its plugins with
     `PlayRole::Replica`; `PlayerTree` becomes `PlayDataModel`, set up by
     `begin_session`; `tree_apply.rs` is deleted; `space_world` stops spawning lights
     from files on the tree path; the local open reads through `tree_read`. mlua ports
     `tree_scripts.rs` against the new names, Multiplayer `net_replica.rs`, and the
     Animation System `follow_player_tree`.

5. **Proof** on a build the orchestrator schedules: Studio hosts Box Head, the Player
   joins, and both show the same HUD, billboards, effects, zombies and top-down camera,
   with `EUSTRESS_CLOUDS=off` and `EUSTRESS_SKY=gradient` on both.

Owners to coordinate with: mlua (`tree_scripts`, the Luau VM, `TreeClock`), Multiplayer
(`net_replica`, PlayerGui, the host guard), Eustress WASM (`tree_read`, `record`, the
parity test), Eustress Vehicle Simulator (its second patch lands in the shared `apply`
after step 2), the Animation System (its drawing hooks go in the crate), and Lights
(`light_classes.rs`, the lighting plugin).

## 11. Verification

- Unit tests move with the code (`apply.rs`'s, `tree_apply`'s four), plus new tests for
  each row of section 4's role table and for GUI rules 5 and 6.
- `seed.rs`'s parity test (the Player's reader against Studio's seed) stays green.
- The access-conflict scan reports 0 conflicts over `engine/src`, `common/src`,
  `client/src` and `play-runtime/src`.
- Lock-free type checks, common from source first, then networking, the crate, the
  engine and the client, with each dependent's own feature set (networking builds common
  with `physics` only).
- The Box Head proof in step 5.

## 12. Outside this move

- **The Space's Sun, Moon and Sky.** Only Studio hydrates them from files
  (`hydrate_lighting_entities`), so the Player draws the default sky. This belongs to
  Lights, and it matters for matching Box Head's look.
- **Terrain edits over the network.** There is no `ReplOp` for them (Multiplayer).
- **Parts scripts parent into a character** (an equipped tool's handle, an accessory).
  Studio draws them; rule 2 skips them on the Player. This resolves with Multiplayer's
  server-authority phase 2, when character roots bind to avatar entities and the rule
  can narrow to the body parts `build_character` makes.
- **Custom meshes** (`MeshId`): the moved `apply` loads them through `space://`, which
  the Player registers already, so milestone M6's gap closes with step 4.
- **Studio's Edit-mode StarterGui preview** still draws through the Slint overlay. It
  switches to the shared GUI renderer after the Play HUD has, which retires the overlay's
  `gui-elements` model in `main.slint` and `sync_gui_elements_to_slint`.

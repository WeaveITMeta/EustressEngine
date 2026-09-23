# Box Head Zombie Defense Tycoon: what building it found in Eustress

A top-down orthographic zombie shooter with per-player tycoon bases, built entirely as Space content
(Luau and Rune scripts, Blender meshes, synthesized sounds) in Universe **Games**, Space **Box Head Zombie
Defense Tycoon**. Building it drove the engine work below. Every item carries its status:

- **LIVE**: fixed or built, and seen working in a running Studio session.
- **COMPILED**: fixed or built, compiles, not yet seen running.
- **FIXED**: changed in source, waiting for the next build.
- **OPEN**: found, not changed.

## Hard stops

| Stop | What the game needed | Status |
|---|---|---|
| HS1 | A live DataModel that Luau and Rune read and write during Play: properties both ways, `Instance.new`, `Clone`, `Destroy`, parenting, signals, input, `Mouse.Hit`, a scriptable orthographic camera, ScreenGui, sounds, `Touched`, raycasts, NPC humanoids | LIVE |
| HS2 | Smooth frame rate with 90+ zombies alive (microprofiler) | Much improved, not yet met: 4 to 8 fps became about 24 to 34 with the full horde on the dev build (57 to 69 with a few zombies); physics (#100) and the Slint UI (#90) are what remain |
| HS3 | What the full game exposed: particles, GUI transparency, input for AI play-testing, lighting from scripts, Rune input, Play-time script editing | GUI transparency, input injection, Rune input and script editing during a session LIVE; particles and scripted lighting COMPILED |
| HS4 | Networked multiplayer | Declared: Eustress has no network transport, so the game runs one local player. Its server and client scripts already talk through RemoteEvents, so it is ready for a transport. |

## Seen working live

In Play, from the HS1 probe scripts and the game itself:

- `game`, `workspace`, `script`, `GetService` for RunService, TweenService, Debris, CollectionService,
  SoundService, Players, UserInputService.
- `Instance.new`, `Clone`, `Destroy`, attributes, value objects, `task.wait` (0.990 s for 1.0), `TweenService`,
  `Debris:AddItem`.
- Physics both ways: an unanchored script-made part falls and lands; a Heartbeat script moves an anchored part;
  `Touched` fires; `workspace:Raycast` hits the ground.
- `PlayerAdded`, `LocalPlayer`, the character spawning at the SpawnLocation, `CharacterAdded`.
- NPC `Humanoid:MoveTo` and `MoveToFinished`.
- Orthographic scripted camera following the character.
- ScreenGui built from code in PlayerGui; `print` reaching the Output panel.
- The game: 4 bases and 4 zombie gates set up, waves spawn Blender-meshed zombies that path through the city on a
  flow field and damage the player, and the HUD updates.
- **Rune and Luau on one tree**: the Rune `BountyDirector` (`use eustress::dm`) finds the zombies, marks one with an
  attribute, turns it gold Neon, and fires a BindableEvent; the Luau `GameDirector` handles it and sends a
  RemoteEvent to the client, whose HUD shows the bounty.
- Driven through `input.inject` (build 7): the player walks the road route from the plaza to a corner base at a
  steady pace, steps on the claim pad and owns the base; the free Cash Register pad buys and builds its
  Blender-meshed machine, and the next tier's pads (walls, turret) appear. Aiming and firing follow the cursor, R
  reloads, keys switch weapons only once they are owned, a death costs 25% of the cash, and a respawn teleports the
  player back to the spawn.
- Play started, stopped and started again in one Studio process: one avatar, one camera, one character binding.
- **Rune gameplay input** (build 8): Q, read by the Rune `AbilityDirector` through `dm::key_down`, fires a
  BindableEvent; the Luau `GameDirector` answers with the shockwave blast and the HUD shows it.
- A script saved while Studio runs hot-reloads on the spot (build 8).

## Engine bugs

### Scripting (Luau)

| # | Bug | Status |
|---|---|---|
| 1 | Property writes skipped `__newindex`: no dirty tracking, `Changed` never fired | LIVE (new Play VM: instances are handles into the DataModel) |
| 3 | Play had no live ECS bridge: names only were seeded, `Instance.new` never reached the scene | LIVE (`play_datamodel`: seed, pull, apply) |
| 6 | `Camera` and `Mouse` were constant stubs | LIVE (camera; `Mouse.UnitRay` from an injected cursor aims and fires the game's gun); a real-cursor check is still to do |
| 7 | `Players.LocalPlayer` static, `Character` always nil, `PlayerAdded` never fired | LIVE |
| 23 | `workspace:Raycast` always nil | LIVE |
| 107 | `UDim2` has no `==`: two equal values compare unequal, so a script that writes a GUI property only when it changes rewrites every UDim2 every frame, and each write repaints the overlay | OPEN (add `__eq`, and `UDim`'s; the game compares them by `tostring`) |
| 108 | `CanQuery` set after a part spawns does nothing: the engine decides a part's collider once, from its flags at spawn, so rays keep hitting it. Corpses fading out after a kill kept stopping bullets | OPEN (the game skips hits on dead zombies instead) |
| 29 | `Instance.new` returned a plain table: `:Destroy`, `:Clone`, `.Touched` errored | LIVE |
| 30 | `GetService` errored for RunService, UserInputService, TweenService, Debris, SoundService; no `script` | LIVE |
| 32 | Scheduler clock reset to 0, first `task.wait(n)` ended at once | LIVE |
| 33 | `while true do end` froze Studio | LIVE (VM interrupt) |
| 34 | `print`, `warn` and errors never reached the Output panel | LIVE |
| 37 | `TweenService:Create` and `Debris:AddItem` errored | LIVE |
| 40 | A folder-form SoulScript with `.luau` source loaded as Rune | COMPILED |
| 41 | CFrame math mixed conventions (Angles gave the inverse rotation) | LIVE |
| 57 | `number * Vector3` errored | COMPILED |
| 70 | **`==` was false for the same instance or enum item** (`input.KeyCode == Enum.KeyCode.W`, `hit.Parent == character`): Luau calls `__eq` even when both sides are one object, and mlua's owned borrow (exclusive under the `send` feature) failed on the second operand | LIVE (identity-first `__eq`, scoped borrows; the game's key and mouse-button checks now fire) |
| 71 | Same root: `v + v`, `v:Dot(v)`, `v * v` errored | COMPILED |
| 72 | `CollectionService:AddTag(part, tag)` errored (bound to `Instance:AddTag(tag)`) | COMPILED (both forms) |

### Rune

| # | Bug | Status |
|---|---|---|
| 9 | No handle to existing parts; a fresh VM per callback | LIVE for the new `eustress::dm` module: Rune reads and writes the same tree as Luau, state lives in attributes |
| 15 | No RNG or clock | LIVE (`dm::random`, `dm::random_index`, `dm::now`, `dm::delta`) |
| 17 | Rune and Luau shared nothing | LIVE |
| 85 | **The physics functions crossed threads through thread-locals shared by separate systems**: `part_get_mass` and `part_get_velocity` read an empty map, `part_apply_impulse` and `part_set_velocity` landed late or never, and `workspace_get_gravity` always returned 9.80665 | COMPILED (one `RunePhysicsBridge` resource, installed and drained on the VM thread; the snapshot is built only when a script names a getter) |
| 10 / 86 | The older Rune API's input, mouse, camera, TweenService, task scheduler, DataStore and Marketplace bridges are never installed (their setters have no callers), so those functions return defaults; its `is_key_down(i32)` has no key-code table at all | OPEN for the older API. LIVE for `eustress::dm`, which now reads the same input Luau sees (the game's Rune `AbilityDirector` reads Q and fires the shockwave): `dm::key_down("W")`, `dm::mouse_down("MouseButton1")`, `dm::mouse_position()`, `dm::mouse_hit()`, `dm::mouse_target()` |
| 12 | `part_set_*` rewrites the part's file on disk during Play | OPEN |
| 13 | `instance_delete(name)` is `remove_dir_all` on authored files, no trash | OPEN (data loss) |
| 16 | The script editor analyzer executes `on_init` and `on_update` on every edit | OPEN |
| 18b | Any spawn or despawn rebuilds a full-scene Rune snapshot (two Strings per entity) | OPEN (HS2 candidate) |
| 104 | Every Play start logs one WARN line per Luau script ("skipped: run_context is Luau, not Rune"), six for this game, so real warnings drown in expected ones | OPEN (log the normal skip at debug level) |

### Play session, camera, input

| # | Bug | Status |
|---|---|---|
| 4 / 59 | The Fjall mirrors wrote every moving part to the database each frame in Play | COMPILED |
| 19 | No scriptable or orthographic Play camera | LIVE |
| 21 | SpawnLocation never attached: Play fell back to (0, 7, 0) | LIVE |
| 22 / 54 | Editor input stayed live in Play (selection, Delete, camera keys) | COMPILED |
| 69 | Studio's viewport overlays (view dropdown, FPS badge, Playing banner) covered the game's HUD | LIVE (hidden in Play; a thin green border marks Play) |
| 73 | `pull_character` kept the previous session's character binding: the next session reused a stale id, never fired `CharacterAdded`, and `root.Position` read nil | LIVE (per-session key; `CharacterAdded` fires every session) |
| 74 | The sound player map outlived the session, keyed by ids that restart | COMPILED |
| 79 | **The avatar and its camera outlived every Stop** through the Stop action: `handle_stop_play` clears the play snapshot and declared an avatar-despawn writer it never used, and the enter-edit safety net that does despawn it returns early once the snapshot is gone | LIVE (Stop despawns the avatar: no avatar entity remains) |
| 96 | With two avatars alive (the one #79 left behind plus the new one), the player's `Character` was rebound to whichever came first each frame, so it was destroyed and rebuilt every few seconds: the player crept a few centimetres per frame and the camera jumped between bodies | LIVE (a second Play in one process has one avatar, bound once) |
| 78 | No way to drive game input from outside (AI play-testing) | LIVE: bridge method `input.inject` (virtual cursor, held and tapped keys and buttons, wheel) aims, fires, reloads and walks the avatar; the MCP tool `play_input` that wraps it is FIXED (needs an MCP server rebuild and a reconnect) |
| 60 | `--play` CLI flag is parsed and never read | LIVE (Play with the character starts once the Space and its scripts have loaded: about 5 seconds after launch, GameDirector and ClientController report ready and both Rune scripts compile) |
| 24 / 47 | ClickDetector and ProximityPrompt never attached | OPEN |
| 25 | F6 pause pauses physics only | OPEN |
| 105 | **The player's body was drawn 3.5 times too big and off its own position.** The rig measured the Mixamo body's height through the skinned mesh's node, which applies the Armature's 0.01 scale a second time (the vertices are already metres): 0.018 m. `rig_scale` floors that at 0.5 m, so 1.75 / 0.5 = 3.5. The camera, the held gun and every shot follow the true 1.75 m body, so the drawn one sat up and ahead of them | FIXED (skinned meshes are measured as they are drawn, through their first joint and its inverse bind) |
| 109 | **The screen flashed at every wave start.** The game moves `Lighting.ClockTime` toward dusk for about 6 seconds per wave, and during Play `advance_sim_clock` (studio_plugins) rewrote the time of day from its own clock on every frame the two disagreed. The sun flipped between the scripted evening (0 lux, 16 degrees below the horizon) and the saved afternoon (78,000 lux) several times a second, then the afternoon won, so no scripted time of day ever stuck | FIXED (the sim clock adopts a time another writer set, then keeps ticking from it) |
| 106 | The hips were pinned to whatever the clips showed 8 frames after the animations started. The avatar spawns in the air, so that frame is the jump clip: the pelvis stayed 0.66 m forward and 0.43 m up for the whole session | FIXED (pinned to the body's own bind pose, mapped through the skeleton root's current transform) |

### Scene, rendering, GUI, audio

| # | Bug | Status |
|---|---|---|
| 76 | **Every translucent GUI element rendered opaque**: `main.slint` passed alpha as `a * 255` but Slint's `rgba()` alpha is 0 to 1 (a hurt flash covered the whole screen) | LIVE |
| 45 | ParticleEmitter was a stub | COMPILED: `Emit(n)` bursts and `Rate` streams with Color, Size and Transparency sequences, SpreadAngle, Acceleration, Drag, EmissionDirection, drawn by the instanced particle renderer |
| 55 | SoundService audio files were never loaded | LIVE (sounds load; `Sound:Play()` runs) |
| 58 | A collider was rebuilt on every BasePart write (a colour change) | COMPILED |
| 53 | GUI click hit test ignored UDim2 scale | COMPILED |
| 75 | A panic (`attempt to add with overflow` in euclid) inside the Slint software renderer during a GUI-heavy session | OPEN: GUI rects need clamping before they reach Slint |
| 77 | TextLabel text elides with "..." (no `TextWrapped` / `TextScaled`) | COMPILED (text wraps when its box is two or more lines tall) |
| 81 / 93 | **Every street light also loaded as a collidable box**: the root-level `Light` rows in the Explorer were 41 stale binary cores (`__bin_PointLight_*`), which the binary path spawned as unanchored, collidable parts that fell onto the roads. Lights were allowed into binary storage, the binary path has no light spawner, and the folder-form light dropped its uuid, so a baked twin could not be recognised | LIVE (lights stay folder-form in the engine and the Roblox importer, the binary loader skips light cores, the light keeps its uuid) |
| 43 | BillboardGui costs too much for per-zombie health bars | OPEN (game avoids them) |
| 51 / 61 | Workspace Gravity (TOML and Properties panel, 196.2 studs/s²) never reaches physics; Play always uses 9.80665 m/s² | OPEN |
| 62 | A Play-spawned Part parented directly under another Part inherits the parent's size as scale (a part's scale is its size), and a child spawned in the same frame as its parent part was placed against the parent's not-yet-propagated identity transform | COMPILED (child scales divide the parent's out; parent poses come from the `Transform` chain; a reparent keeps the world scale) |
| new | `game.Lighting.ClockTime`, `Brightness`, fog and ambient written by a script never reached the renderer | COMPILED (applied to `LightingService`; Stop restores the editor's lighting) |

### Frame rate (HS2)

Measured with the per-system profiler in Play at wave 12 and up. The build was the dev profile (engine at
opt-level 2, dependencies at 3) and a compile ran beside it, so the figures rank the costs; they are not the
final frame rate.

Before the fixes below, a wave 12 horde ran at about 4 to 8 frames per second in that setting: rendering took
about 139 ms of a 231 ms frame (170 shadow views per frame), the Slint UI about 20 ms, the mesh optimizer 14 ms,
physics about 20 ms, the two per-frame Rune snapshots 5 ms, and the Luau scripts 4 ms.

After them (build 7, same dev profile, no compile running, Studio in the foreground), wave 12 with the 90-zombie
cap alive runs at about 24 frames per second sampled over 20 seconds (15 to 33), and the phase profile's last
60-frame window averaged 29 ms (34 frames per second). Rendering fell from about 139 ms to 2 ms per frame.
What is left: fixed-step physics about 11 ms (#100), Update about 11 ms (Luau 3 ms, the tree-to-ECS apply 3 ms
with 55 ms spikes when a burst of zombies spawns, Slint about 3.5 ms on average), PostUpdate 4 ms. With a few
zombies alive the same build holds 57 to 69 frames per second. A release build is faster still; the target for
HS2 (a steady 60 with the horde) needs #100 and the Slint cost below.

| # | Bug | Status |
|---|---|---|
| 84 | **Lamps ignored `shadows = false`**: the light loader read only PascalCase keys, and point lights default to casting shadows, so the lit city rendered about 170 shadow views per frame | LIVE (keys read in any case; integer values and 0 to 255 colours accepted; rendering 139 ms to 2 ms per frame) |
| 87 | **The mesh optimizer re-optimized every mesh over 500 triangles every frame** and re-uploaded each one to the GPU (10 to 14 ms per frame): its own write came back as a `Modified` event and queued the mesh again | COMPILED (its own writes are recognised and skipped; running in build 7, not measured on its own) |
| 88 | Each Play frame copied every named entity into a String-keyed map (2 to 4 ms) that nothing reads | COMPILED (only entities with simulation state) |
| 89 | The Universe list rescanned `Documents/Eustress` on the frame every 5 seconds (about 30 ms) | COMPILED (scans on the IO task pool) |
| 90 | The Slint UI repainted on every frame a script touched the HUD, because the overlay model was replaced wholesale, and a repaint costs 13 to 20 ms of software rendering plus a full 5.8 MB texture upload | IMPROVED, still OPEN: the overlay updates row by row, so about a quarter of frames repaint in Play; each still costs 13 to 17 ms for about a third of the window (the Slint software renderer). A 120-frame paint summary is now logged under `eustress_engine::slint_paint` |
| 91 | The sky's generated environment map rebuilds its bind groups and re-filters every frame (3 to 4 ms of CPU, plus GPU work) although the sky changes slowly | OPEN (regenerate only when the sky changes) |
| 100 | Physics substeps are pinned at 6 for determinism, which costs about 20 ms per frame with a wave of 118 dynamic zombies; a top-down game needs far fewer | OPEN (recommend a per-Space substep setting) |
| note | Below 30 frames per second the whole simulation runs in slow motion: virtual time advances at most 33 ms per frame (the guard against a fixed-timestep death spiral). Any frame-rate shortfall therefore also slows gameplay | By design; the fixes above raise the frame rate |

### Loading and files

| # | Bug | Status |
|---|---|---|
| 64 | One malformed property (`color = [0.0]`) dropped the whole part at load | COMPILED (warns, uses the default) |
| 65 | A part file that failed to load stayed missing after it was fixed on disk | COMPILED |
| 66 | A `.luau` script created while Studio runs, or handed over by the startup reconcile after an offline edit, never loaded (Play started with zero scripts) | COMPILED |
| 67 | A script deleted from disk comes back on the next open (the database keeps a tree-only record; reconcile reports `removed=0`) | OPEN |
| 68 / 97 | **Every file saved while Studio ran dropped out of the database tree.** An atomic save arrives as a same-path Remove and Create; the watcher handled the Remove as a modify (the file still exists) but broadcast it as a Remove, and because Creates sort first, the disk-to-database sync stored the new bytes and then deleted the key. On the next open the file looked new. This is why script edits made during Play never reached the next Play | LIVE (a Remove whose file still exists is broadcast as a modify; scripts edited during a session stay in the tree) |
| 98 | A script created while Studio runs was parented only through its service's `_service.toml`, which a database-loaded Space does not register, so it landed at the root and Play never ran it (after #97 this hit the game's own client script: no HUD, camera or aiming) | LIVE (parented to the folder it sits in, then its service; Play queues it) |
| 83 | A template created while Studio runs, in a folder that is also new, appears at the root until the next open: a new plain folder has no marker, so nothing spawned it, and the fallback looked for `<service>/_service.toml`, which nothing registers | COMPILED (a new folder becomes a Folder instance under its owner, as at load; parts, scripts and GUI files share the lookup) |
| 99 | A file-form part (`Crate.part.toml`) created while Studio runs was parented one folder too high (the folder-form rule applied to it) | COMPILED |
| 110 | **A configuration `.toml` added to a Space while Studio runs is rewritten.** The loader skips plain `.toml` files on purpose, but the file watcher fell back to the bare extension and handled one as an instance file: the class-schema self-heal rewrote it on disk ("self-healed ... (<unknown class>)"), dropping its comments and reordering its keys, and the load then failed on the missing `[metadata]`. Seen with `StarterPlayer/Characters/*.rig.toml` | FIXED (the watcher skips plain configuration `.toml` files the way the loader does; `_service.toml` is unchanged). The self-heal should also refuse to touch a file whose class it cannot name: OPEN |
| 102 | A script saved by an editor that writes a temp file and renames it over the original arrives as a lone Create; the create path returns early for a loaded file, so the edit was never hot-reloaded (it only applied on the next launch) | LIVE (a Create for a loaded script is handled as an edit) |

### Shipping

| # | Bug | Status |
|---|---|---|
| 28 | The Player app loads only Part classes and runs no Space scripts: this game cannot run outside Studio yet | OPEN |
| 63 | `eustress-engine --help` calls itself a "Game Development Studio", and the About dialog a "game engine" | COMPILED (both describe the AI-native orchestrator) |
| 94 | The bridge self-test logged an ERROR ("timed out after 10 s") on every launch that took longer than that to reach its first frame, while the bridge worked | LIVE (a note at 10 s, a 180 s budget) |
| 95 | Under Bevy 0.19 a bare `assets.get_mut(handle);` no longer marks the asset changed (only a mutable dereference does), so the "required" material touch after each Slint repaint does nothing | OPEN (harmless today: the image write alone re-uploads) |
| 103 | A Studio started from a copy of its executable still runs its language server from the source tree. `lsp_launcher.rs` looks for `eustress-lsp` in `EUSTRESS_LSP_BIN`, then beside the running executable, then under `target/release` and `target/debug` (paths fixed at compile time). While that child runs, Windows refuses to replace `target/debug/eustress-lsp.exe`, so another session's engine build fails at its last step ("failed to remove file", os error 5) | OPEN (workaround, verified: put `eustress-lsp.exe` beside the copy and set `EUSTRESS_LSP_BIN`; fix: start a source-tree binary from a copy in the temp folder) |

## Engine work added for the game

- `crates/engine/src/play_datamodel/`: seed (ECS to tree at Play start), pull (poses, input, mouse ray,
  collisions, the character), apply (tree to ECS: spawns, reparents, properties, lighting), audio, camera, NPC
  humanoids, particles, and a stage clock that writes `eustress_profile_play.txt` when `EUSTRESS_PROFILE` is
  set. Its pull, scripts and apply figures are wall spans between markers and include any system the
  executor ran in between; the per-system profiler measures what each system costs.
- `crates/common/src/datamodel/`: the live tree both languages share.
- `crates/common/src/luau/play/`: the Play VM.
- `crates/engine/src/soul/rune_datamodel.rs`: the `eustress::dm` Rune module, including input
  (`key_down`, `mouse_down`, `mouse_position`, `mouse_hit`, `mouse_target`).
- `crates/engine/src/soul/physics_bridge.rs`: `RunePhysicsBridge`, the one place Rune physics reads and commands
  cross threads.
- Bridge method `input.inject` and MCP tool `play_input` for AI play-testing.
- `eustress-engine --play` starts Play with the character once the Space has loaded.
- A 120-frame Slint paint summary in the log (`eustress_engine::slint_paint`): frames repainted, dirty share, render
  and copy time.

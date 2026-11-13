# Player parity: why a published Space does not play like Studio's Play

Status: research report, 2026-10-21, with the progress of 2026-11-12 below.
Sections 1 to 11 are the original report and describe the Player as it was
then; the progress section says what has changed since.

## Progress, 2026-11-12

McKale chose D1 option A: a published player world carries the server and
its code. A layer that obfuscates downloaded code comes later.

The desktop Player now plays as `PlayRole::Authority` for every launch but a
join (`client/src/main.rs`, `play_role`); `Launch::Join` stays `Replica`.
Steps 2 to 7 of plan (a) are in the tree:

| Step | What is in the tree | Proof |
|---|---|---|
| 2 | `play_session::give_player_containers` makes Backpack, PlayerScripts and PlayerGui (with the StarterGui copy) for a solo Player; Studio's remote players and the Player share `fill_player_gui` | common `a_solo_player_gets_its_containers_and_the_starter_gui` |
| 3 | `TreeScriptsPlugin` under Authority runs server scripts first in Studio's order (Workspace, ServerScriptService, SoulService), then `PlayerAdded`, then client scripts, with `is_server = true` | common `the_authority_runs_server_scripts_in_studios_order_and_a_replica_does_not`; fails when the role check is removed |
| 4 | `pull_poses`, `pull_drawn_poses`, `pull_collisions`, NPC humanoids, Seats, VehicleSeats and `ScriptedJointsPlugin` live in play-runtime and run only under Authority; the engine re-exports them | play-runtime `authority_drives_npc_a_replica_never_does` and 14 more; fails without `run_if(simulates)` |
| 5 | The solo and local launches are Authority; a local Space opens through the same tree as a published one | client tests |
| 6 | `echk_export` gives the Players audience every record (D1 option A); the Server audience is the source world | engine `a_player_world_carries_the_server_and_its_code`; fails when server-only paths are withheld again |
| 7 | Rune runs in the Player: `common/src/soul/rune_play.rs` (`TreeRunePlugin`), with `eustress::dm` in `common/src/soul/rune_datamodel.rs`. Solo runs every Rune script; a joined Player runs only its own | 13 common tests, including `the_super_station_answers_a_question` on the real Space |

GUI data is one parser for both shells (`common/src/gui/gui_record.rs`):
Studio's seed and the Player's reader map the same TOML to the same tree
properties (TextWrapped, TextStroke, CornerRadius, ClipsDescendants, Active,
BillboardGui and SurfaceGui fields). The Player draws SurfaceGui. Proof: the
engine's `gui_parity_a_players_gui_starts_equal_to_studios`.

The pause menu leaves Escape to a focused TextBox, restores the cursor it
found, and blocks HUD input while it is open.

Still open: terrain for scripts in the Player (step 8), "cannot run here"
notices (step 9), the browser VM (step 10, D2), commerce in solo play (D3),
realism and `DeterminismPlugin` (D4), hover events and 3D GUI clicks in both
shells, the UI layout classes, BillboardGui `Adornee`, TextBox
`PlaceholderText`, and the `studs_offset` and `border_size_pixel` spellings.
Listings published before option A carry no server code until they are
published again.

Branch note. The staging Player McKale downloaded was built from
`origin/ci/player-staging` (head `acb7bbe4`, 2026-10-20). That commit is an
ancestor of `staging/integration`. Between the two, `eustress/crates/client`
and every Play path in `play-runtime` are unchanged (only
`play-runtime/src/billboards.rs` differs), so the desktop findings below hold
for the build he ran. The browser Player is different: solo play
(`player-web/src/solo.rs`, `fetch.rs`) landed after that commit (`54a4f057`);
on `ci/player-staging`, `player-web/js/host.js:13-14` still says a published
world played alone "is not in this build yet". Which browser build the staging
site serves was not verified.

## 1. The answer in one paragraph

The Player was built as a viewer of someone else's simulation. Both Players
run the shared Play runtime only as `PlayRole::Replica`
(`client/src/main.rs:423-425`, `player-web/src/app.rs:128-130`), and no code
path in either Player ever runs as `PlayRole::Authority`. A Replica runs no
`Script` (`common/src/tree_scripts.rs:350`), no Rune, no NPC humanoid driver,
no seats, no joints, and turns every unanchored part kinematic
(`play-runtime/src/apply.rs:141-147`): it waits for a host to do all of that.
The Gallery's Play button gives a solo link whenever nobody is hosting
(`infrastructure/cloudflare/api/src/index.js:5955-5978`), so a solo Player
opens the world, draws it, and then nothing drives it. On top of that, the
published player world does not contain the server code at all
(`engine/src/space/echk_export.rs:163-178`, `:195-265`), and the Box Head
listing on staging has Share Source off, so even a Player with an Authority
path would have no `GameDirector` to run. The architecture (Replica only) is
the main cause. There are also real bugs (no `PlayerGui` in a solo session,
no terrain for scripts, no welds), listed in section 6.

## 2. How each shell runs a Space today

### Studio Play (the reference)

`PlayModeCorePlugin` (`engine/src/play_mode.rs`, plugin build near line 1740)
enters `PlayModeState::Playing`, then:

- `seed::seed_session` (`engine/src/play_datamodel/seed.rs:21`) builds the
  session tree from the ECS, makes the local `Player` with `PlayerGui`,
  `Backpack` and `PlayerScripts` (`seed.rs:73-81`), copies StarterGui into
  `PlayerGui` (`seed.rs:99-110`), hides storage services, and queues both the
  server `Script`s and the `LocalScript`s in one Luau VM (`seed.rs:131-146`;
  `collect_launches` at `seed.rs:262`).
- `drive_mlua` (`engine/src/play_datamodel/mod.rs:267`) starts server
  scripts, fires `PlayerAdded`, then starts client scripts; every frame it
  gives the VM live raycasts and live terrain reads.
- Pull (`mod.rs:197-210`): frame state and input, physics poses, Touched
  collisions, the local character, remote characters, the mouse hit.
- Apply (`mod.rs:229-244`): the shared `apply_frame`, script terrain edits,
  NPC humanoids (`npc.rs:54`), the scripted camera, particles, commerce,
  output.
- Seats and VehicleSeats (`mod.rs:224`, `mod.rs:251-254`), Rune sessions
  (`compile_scripts_on_play` and `rune_play::start_rune_session`, registered
  in `PlayModeCorePlugin`), joints and welds (`engine/src/app_core.rs:219`,
  `ScriptedJointsPlugin`), physics activation for unanchored parts
  (`play_mode.rs:2488`).
- The role is never inserted, so it is the default `Authority`
  (`common/src/play_session.rs:167-171`).

### Desktop Player, solo (Gallery Play with nobody hosting)

1. `eustress-player://play/<id>` becomes `Launch::Published`
   (`client/src/main.rs:140-143`), which inserts `PublishedTarget` and
   `request_published_world` (`main.rs:493-496`, `:647`).
2. `space_fetch` downloads the player world's `.echk` chunks
   (`client/src/systems/space_fetch.rs:132-188`).
3. `net_play::open_arrived_world` writes it to disk and opens it
   (`client/src/systems/net_play.rs:123-151`). Because this is a network
   launch, `space_world` opens it as a tree (`space_world.rs:311`, `:396`),
   inserts `PlayDataModel` (`:413`), and the shared runtime draws it as a
   Replica.
4. The avatar spawns at the Space's SpawnLocation (`net_play.rs:153-201`).
5. `TreeScriptsPlugin` (`main.rs:431`) starts only `LocalScript`s
   (`tree_scripts.rs:332-400`, the class filter at `:350`) with
   `is_server = false` (`:258`) and no terrain (`:274`, `:328`).
6. `ClientSoulPlugin` builds an empty Rune runtime; nothing in the client
   ever compiles a Rune script (`client/src/soul/mod.rs:49-68`; no caller of
   `compile_scripts`, `common/src/soul/rune_runtime.rs:283`). Its physics
   bridge is a stub (`soul/mod.rs:187-199`).

Nothing simulates: no server script, no NPC, no seat, no joint, and every
unanchored part is kinematic with no host to move it.

### Desktop Player, joined to a host

The same open path, then `NetReplicaPlugin` applies the host's world frames
into the tree before `Pull`, interpolates the host's moving bodies, sends
this Player's `FireServer` and `InvokeServer` calls, and exchanges animation
tracks (`common/eustress-networking/src/player_replica.rs:1-30`, `:357-392`).
The host runs everything a Studio Play runs. This path is architecturally
sound; section 4 lists where it diverges.

### Browser Player

The same Replica runtime (`player-web/src/app.rs:128-130`) with no script VM
at all: Luau (mlua) is C++ and does not build for `wasm32-unknown-unknown`
(`player-web/Cargo.toml:50-53`, `app.rs:141-143`). Rune is not wired either.
So in the browser not even LocalScripts run, solo or joined.

## 3. Parity matrix

RUNS: works as in Studio. PARTIAL: some of it. MISSING: does not happen.
HOST: happens on the host and reaches this Player through replication.
UNVERIFIED: not traced to the end in this review.

| System | Studio Play | Player solo | Player joined | Browser (solo / joined) | Evidence and notes |
|---|---|---|---|---|---|
| Open the Space's records as a tree | RUNS (from the ECS, `seed.rs:166`) | RUNS (`tree_read::open_player_world_dir`, `space_world.rs:396`) | RUNS | RUNS (`player-web/src/world.rs`) | A parity test already compares the two trees at start: `seed.rs:1052` |
| Server `Script` (Luau) | RUNS (`seed.rs:262-317`, `mod.rs:297-299`) | MISSING (`tree_scripts.rs:350`), and the code is not in the download (`echk_export.rs:195-265`) | HOST | MISSING / HOST | |
| `LocalScript` (Luau) | RUNS (same VM) | RUNS (`tree_scripts.rs`), but blocks on anything a server script makes | RUNS | MISSING / MISSING (no VM on wasm32) | |
| `ModuleScript` via `require` | RUNS | RUNS (client side only) | RUNS | MISSING | |
| Rune scripts (`eustress::dm`) | RUNS (`engine/src/soul/rune_play.rs`) | MISSING (empty runtime, `client/src/soul/mod.rs:49-68`) | HOST (server Rune) | MISSING / HOST | `engine/src/soul/rune_datamodel.rs` has no `crate::` imports, so it can move |
| `is_server` / `RunService:IsServer()` | true | false (`tree_scripts.rs:258`) | false | n/a | |
| `PlayerAdded`, `CharacterAdded` order | RUNS (`mod.rs:297-313`) | PARTIAL: `CharacterAdded` from `sync_local_character` (`common/src/animation/character.rs:59-109`); no server listeners exist | HOST | PARTIAL | Two character-sync implementations: engine `pull.rs:389` and common `animation/character.rs` |
| `PlayerGui` and the StarterGui copy | RUNS (`seed.rs:73-110`) | MISSING: the solo player has no `PlayerGui` (`tree_read.rs:226-231` makes a bare `Player`), so the HUD draws nothing (`play-runtime/src/hud.rs:213-216`) | HOST fills it (`engine/src/play_datamodel/remote_players.rs:346`) | MISSING / HOST | A bug, independent of the role |
| `Backpack`, StarterPack | RUNS (`seed.rs:80`, `play_session.rs:289`) | MISSING (no `Backpack` child, so `fill_backpack` returns early) | HOST | MISSING / HOST | |
| Physics of unanchored parts | RUNS (Dynamic) | MISSING: Kinematic with nobody to move them (`apply.rs:141-147`) | HOST (motion lane) | MISSING / HOST | |
| Physics poses into the tree (`Position`, `CFrame` that scripts read) | RUNS (`pull.rs:244`, `:296`) | MISSING | HOST | MISSING / HOST | |
| `Touched`, `TouchEnded` | RUNS (`pull.rs:350`) | MISSING | HOST | MISSING / HOST | |
| `workspace:Raycast` | RUNS (`mod.rs:284`) | RUNS (`tree_scripts.rs:288-327`) | RUNS (client) | MISSING (no VM) | |
| Terrain reads by scripts (`ReadVoxels`, hit material) | RUNS (`mod.rs:286-296`) | MISSING (`no_terrain`, `tree_scripts.rs:328`) | client: MISSING; server: HOST | MISSING | |
| Terrain edits by scripts | RUNS (`terrain_edits.rs:22`) | MISSING | HOST (UNVERIFIED that edits replicate) | MISSING | |
| Local avatar, controller, climb, animation | RUNS (`AvatarRuntimePlugin`) | RUNS (same plugin, `main.rs:395`) | RUNS | RUNS | Avatar contract test: `common/src/avatar/parity.rs` |
| NPC humanoids (`Humanoid:Move`, `MoveTo`, `WalkSpeed`) | RUNS (`npc.rs:54`) | MISSING | HOST | MISSING / HOST | `npc.rs` has no `crate::` imports |
| Seats, VehicleSeats | RUNS (`seats.rs`, `vehicle_seat_input.rs`) | MISSING (`Sit` queues only on a server, `common/src/mlua/play/instance.rs:693`) | HOST (`docs/networking/SEATS.md`) | MISSING / HOST | Neither file has `crate::` imports |
| Welds, joints, constraints, assemblies | RUNS (`app_core.rs:219`) | MISSING (`ScriptedJointsPlugin` is added only by the engine) | PARTIAL: the host simulates, each part is drawn flat (`apply.rs:152-160`) | MISSING / PARTIAL | |
| `ApplyImpulse`, velocities | RUNS (shared `apply_frame`) | MISSING (kinematic bodies) | HOST | MISSING / HOST | |
| RemoteEvent, RemoteFunction | RUNS (one VM, both sides) | MISSING: no server to receive | RUNS (`player_replica.rs:357-392`) | MISSING / MISSING (no VM fires them) | |
| BindableEvent | RUNS | RUNS (client only) | RUNS | MISSING | |
| TweenService, Debris, task scheduler | RUNS (prelude) | RUNS (client VM) | RUNS | MISSING | |
| Session clock (`time()`, Heartbeat) | RUNS (pull) | RUNS (`play-runtime/src/lib.rs:96-107`) | RUNS | RUNS | |
| DataStoreService (Luau) | MISSING in the Play VM too (not in `common/src/mlua/play`; only the older `mlua/runtime.rs:2956`, and Rune) | MISSING | MISSING | MISSING | The same gap on both sides: not a Player bug |
| MarketplaceService, commerce | RUNS (`commerce.rs:243`, test mode for the creator) | MISSING (no session commerce driver) | PARTIAL (`client/src/systems/commerce.rs`; the host decides) | MISSING | |
| Sounds | RUNS (shared `sound_bridge`) | RUNS for sounds in the tree; nothing creates server sounds | RUNS (the host's sounds replicate) | RUNS (browser audio UNVERIFIED) | |
| Particles | RUNS (shared `drive_particles`) | RUNS for authored emitters | RUNS (`Emit` ops) | PARTIAL (no draw on WebGL2, `particle_render.rs` `finish`) | |
| ScreenGui HUD, its input and clicks | RUNS (`play-runtime/src/hud.rs`, `hud_input.rs`) | RUNS, but has no `PlayerGui` to draw (row above) | RUNS | RUNS draw, no scripts | |
| BillboardGui, SurfaceGui | RUNS | RUNS (shared draw) | RUNS | RUNS (per class UNVERIFIED) | |
| Sky, lighting, atmosphere | RUNS | RUNS (`LightingServicePlugin`, `main.rs:387`) | RUNS | RUNS (`SharedLightingPlugin`) | Two lighting plugins: a parity risk |
| Terrain draw and collision | RUNS | RUNS (`space_world.rs` terrain section) | RUNS | UNVERIFIED | |
| Ocean, water bodies | RUNS in Studio | UNVERIFIED | UNVERIFIED | UNVERIFIED | Not traced |
| ClickDetector, ProximityPrompt | PARTIAL: ECS side only (`engine/src/interaction/click.rs`, `proximity.rs`); no Play tree events found | MISSING | MISSING | MISSING | Not wired into the Play tree on either side |
| Decals, Beams, Trails | RUNS (engine spawners) | UNVERIFIED (not found in `play-runtime/src/apply.rs`) | UNVERIFIED | UNVERIFIED | |
| Realism, sim values, `DeterminismPlugin` | RUNS (`app_core.rs:282-300`) | MISSING (the client does not add them) | HOST | MISSING | |

## 4. Joined Player: what replicates and where it diverges

Replicated services: Workspace, ReplicatedStorage, ReplicatedFirst, Lighting,
Players, Teams, StarterGui, StarterPack, StarterPlayer, SoundService, Chat
(`common/eustress-networking/src/repl/ops.rs:77-90`). Never replicated:
ServerScriptService, ServerStorage, any `Script`'s `Source` (`ops.rs:159`),
the host's Camera (`ops.rs:131-136`). `PlayerGui`, `PlayerScripts` and
`Backpack` go only to their owner (`ops.rs:92-93`, `:138-145`). The lanes:
world ops (spawns, property writes, attributes, tags, reparents, destroys,
remote events, particle emits), the motion lane (bodies the host's physics
moved, about 30 samples a second), the input lane, remote calls, and
animation tracks (`repl/mod.rs:1-20`).

Where a joined Player diverges from what the host runs:

1. Parts are drawn flat and kinematic (`apply.rs:152-160`), and motion is
   drawn "a little in the past" between samples (`player_replica.rs:6-7`).
   Expected for a replica: fast physics looks smoothed, not wrong.
2. The client VM gets no terrain (`tree_scripts.rs:328`): a LocalScript that
   reads terrain sees none, while the same script in Studio sees it.
3. A Space whose server code reads "the local player" or this machine's keys
   acts for the host only. Box Head's `AbilityDirector.rune` reads
   `dm::key_down("Q")` from the tree's input
   (`engine/src/soul/rune_datamodel.rs:363-365`), and `GameDirector` resolves
   the blast to `Players.LocalPlayer or Players:GetPlayers()[1]`
   (GameDirector line 1393). A joined player's Q does nothing. This is Space
   content written for one player, not a transport bug.
4. A dedicated host (`eustress-engine --server`) runs server Luau only from
   the listing's source world (`docs/networking/DEDICATED_SERVER.md:30-35`).
   Box Head's staging listing has no source world (section 5), so a
   dedicated host of Box Head runs no server scripts either. Only the
   author's own Studio, hosting Play, runs Box Head fully.

## 5. Conformance case 1: Box Head Zombie Defense Tycoon (staging)

Listing `07d084d8-9098-4192-bb48-84d2c428db36`, read on 2026-10-21 with
read-only GETs: `is_open_source: false`, version 2, updated 2026-10-03;
`/world/manifest` answers 200 (one Space, four chunks, `engine_version`
0.3.7); `/world/source` answers 404. The 2.5 MB chunk `0_0.echk` contains
`StarterPlayer/StarterPlayerScripts/ClientController`,
`ReplicatedStorage/Shared/Config` and `NavGrid`, and no `ServerScriptService`
record: the strings `GameDirector`, `BountyDirector` and
`ServerScriptService` do not occur in it, and `AbilityDirector` occurs only
in comments inside `Config` and `ClientController`.

What the Space relies on (local copy,
`Documents/Eustress/Games/Spaces/Box Head Zombie Defense Tycoon`):

| Piece | Kind | What it does |
|---|---|---|
| `ServerScriptService/GameDirector` (1,914 lines) | server Luau | Makes `ReplicatedStorage.Remotes` (Fire, Reload, Equip, Build, Notify, Fx) and `ReplicatedStorage.GameState` at run time (lines 46-81); bases and tycoon pads; waves; every zombie's brain (`Humanoid:Move`, lines 790-830) and spawning; weapons through `workspace:Raycast`; explosions with `ApplyImpulse`; turrets; the economy as Player attributes; every sound (`playSound` copies from SoundService); particles; BillboardGui tags; the `RunService.Heartbeat` loop (line 1874) |
| `ServerScriptService/BountyDirector.rune` | server Rune | Every 25 s marks a zombie as a gold bounty |
| `ServerScriptService/AbilityDirector.rune` | server Rune | The Q shockwave, through a BindableEvent |
| `StarterPlayerScripts/ClientController` (844 lines) | LocalScript | Waits for `Remotes` and `GameState` (lines 18-28), then builds the HUD ScreenGui, the orthographic top-down camera, aim, the laser sight, input, and the `FireServer` calls |
| `ReplicatedStorage/Shared/Config`, `NavGrid` | ModuleScripts | Shared tables and navigation |
| `StarterPlayer/Characters/*.rig.toml` | content | The box hero bodies |
| DataStore, commerce | none | Not used |

What each shell shows:

| System | Studio Play | Player solo (what McKale ran) | Player joined to McKale's Studio | Browser |
|---|---|---|---|---|
| Map, bases, lights | yes | yes | yes | yes |
| Hero avatar walks | yes | yes | yes | yes (local spawn landed in `8250a1bc`; UNVERIFIED on the deployed build) |
| Top-down camera, aim, laser | yes | NO: `ClientController` waits forever at `ReplicatedStorage:WaitForChild("Remotes")` (line 20), so the default follow camera stays | yes | NO (no Luau) |
| HUD (cash, HP, ammo, wave) | yes | NO (the same wait; also no `PlayerGui`) | yes | NO |
| Zombies, waves, bounty, Q | yes | NO (no server code downloaded, no Authority) | yes (Q for the host only) | NO |
| Weapons, turrets, tycoon pads | yes | NO | yes | NO |
| Sounds and particles | yes | NO (the server makes all of them) | yes | NO |

A falsifiable check of McKale's solo run: the Player log
(`~/.eustress_engine/logs/player-<pid>.log`) should hold the Play VM's
stuck-thread warning, `Infinite yield possible on
'ReplicatedStorage:WaitForChild("Remotes")'`
(`common/src/mlua/play/prelude.luau:1069`, written to the log by
`play-runtime/src/lib.rs:111-122`), and no error. If the log shows something
else, this diagnosis of his run is wrong and needs another look.

## 6. Root causes, ranked

1. **Architecture: no Authority path in either Player.** Both Players
   hard-code `PlayRole::Replica`. Solo is the Gallery's default whenever
   nobody hosts, so most Gallery plays reach a viewer with no simulation.
   This alone explains "launches but does not play".
2. **Content delivery: the player world withholds server code by design.**
   `echk_export.rs:163-178` drops ServerScriptService and ServerStorage, and
   `withhold_server_code` (`:195-265`) strips every server `Script`'s code
   anywhere else. The source world exists only with Share Source on, and the
   Worker serves it to everyone only then (`world.mjs:570-575`). An Authority
   Player still needs a decision on how server code reaches it (D1).
3. **Bug: a solo session has no `PlayerGui`, `Backpack` or `PlayerScripts`.**
   `tree_read.rs:226-231` creates a bare `Player`; the HUD draws only under
   `LocalPlayer.PlayerGui` (`hud.rs:213-216`); StarterGui is never copied.
   Every static ScreenGui of every Space is invisible in solo play. A small
   fix, and it belongs in the shared session start (step 2 below).
4. **Shared systems that are not shared yet.** NPC humanoids, seats, vehicle
   seats, joints, pull's physics half and Rune live only in the engine
   crate, so a Player cannot run them even as Authority until they move.
5. **The browser has no script VM.** mlua cannot build for wasm32; until a
   Luau VM exists there, the browser runs no LocalScript, even when joined.
6. **Smaller divergences:** the client VM gets no terrain
   (`tree_scripts.rs:328`); two character-sync implementations (engine
   `pull.rs:389` and common `animation/character.rs`); two lighting plugins;
   the client never adds `DeterminismPlugin`, realism or sim values
   (`app_core.rs:282-300`).

## 7. Plan (a): one shared Play runtime, Authority in the Player

### Target

`PlayRuntimePlugin { role }` becomes the whole Play session, not only its
draw side. Studio's Play, the desktop Player playing solo, a dedicated
server, and the browser Player playing solo all run `role: Authority` with
the same plugins and the same frame chain (`Pull`, `Scripts`, `Apply`,
`End`). `Replica` is used only when joining someone else's host. Studio keeps
only what is editor-specific: building the tree from its ECS (the seed's
scene half), the Output panel, the Slint viewport input gate, undo
snapshots, and Stop's restore.

### What moves, and where

| Module today | Moves to | Engine-only dependencies to cut | Size |
|---|---|---|---|
| Session start: local Player with PlayerGui, Backpack, PlayerScripts; the StarterGui copy; `collect_launches` (`seed.rs:21-160`, `:262-341`) | `common/src/play_session.rs` (`begin_authority_session`), beside `begin_session` | `AuthState` (the display name: pass it in); `CharacterAutoLoads` (already read by `play_session::character_auto_loads_in`); hiding `GuiElementDisplay` (stays in Studio) | S |
| `PlayMluaHost` and `drive_mlua` (`mod.rs:63-73`, `:267-371`) | `common/src/tree_scripts.rs` gains an Authority mode (server and client launches in one VM, `is_server = true`), or a new `common/src/session_scripts.rs` | None found; the terrain reader needs the terrain query, whose types are in common | M |
| The simulation half of `pull.rs`: `pull_poses`, `pull_drawn_poses`, `pull_collisions`, `pull_character`, `cast_ray` | `play-runtime/src/pull.rs` | `pull_frame_state` uses `ViewportBounds`, `SlintUIFocus`, `EustressCamera`: that per-machine part stays a Studio adapter; the Player already has `tree_scripts::pull_tree_input` over `machine_input` | M |
| `npc.rs` | `play-runtime/src/npc.rs` | None found | S |
| `seats.rs`, `vehicle_seat_input.rs` | `play-runtime/src/seats.rs` | None found | S |
| Registering `ScriptedJointsPlugin` | `PlayRuntimePlugin` (the plugin is already in play-runtime; only `app_core.rs:219` registers it) | None | S |
| `particles.rs`, `camera.rs`, `apply.rs` | Already shared (the engine files are 4-line re-exports) | None | done |
| `terrain_edits.rs` | `play-runtime/src/terrain_edits.rs` | `crate::terrain_commands` and the undo-tied Play snapshot: the edit applier moves, the snapshot and undo stay in Studio | M |
| Rune: `rune_datamodel.rs` (`eustress::dm`) | `common/src/soul/rune_datamodel.rs` | None found (no `crate::` imports) | S |
| Rune: the `rune_play.rs` frame driver and `compile_scripts_on_play` | `common/src/soul/rune_play.rs` | `SpaceFileRegistry`, `SpaceRoot`, `SimValuesResource`, `ScriptSpatialQuery`, `trash` (`rune_play.rs:57-68`): split into a core driver over the tree plus Studio-only bridges | L |
| `commerce.rs` (session commerce) | Stays in the engine for the host; the solo Player gets a driver per D3 | `AuthState`, `SpaceRoot` | M |
| `remote_players.rs`, `net_replicate.rs` | Stay with the host. A Player that hosts (Authority plus host) would need them later | n/a | later |
| Realism, sim values, `DeterminismPlugin` | `DeterminismPlugin` into `PlayRuntimePlugin` under Authority; realism and sim values per D4 | | S / M |

### Steps, in dependency order

| Step | Owner area | Size | Depends on | What the orchestrator's build verifies |
|---|---|---|---|---|
| 1. The support table and its test (plan c) land first, every current system listed with its present status | play-runtime, docs | S | none | `cargo test -p eustress-play-runtime --lib play_support` passes; deleting one row fails it (mutation) |
| 2. `begin_authority_session` in common: Player children, StarterGui copy, StarterPack, launches. Studio's seed calls it, and so does the Player's solo path. Fixes the missing `PlayerGui` (root cause 3) | common, engine seed, client | S | 1 | `cargo test -p eustress-common --lib play_session`; the engine's `a_players_tree_starts_equal_to_studios` (`seed.rs:1052`) extended to compare the Player's children and the copied GUIs |
| 3. An Authority script driver in common: server plus client launches in one VM, `PlayerAdded` order, character launches, the terrain reader | common | M | 2 | `cargo test -p eustress-common --lib tree_scripts`; a test that a `Script` runs under Authority and not under Replica, shown to fail when the role check is removed |
| 4. Move NPC, seats, vehicle seats and pull's physics half into play-runtime; register `ScriptedJointsPlugin` in `PlayRuntimePlugin`; Studio switches to the moved systems with no change in behaviour | play-runtime, engine | M | 3 | `cargo check -p eustress-engine`, `cargo test -p eustress-play-runtime --lib`, then a Studio Play of Pong and Box Head (McKale, or the playtest skill) |
| 5. The desktop Player's solo and local launches become Authority: `Launch::Published` and `Launch::Local` insert `PlayRole::Authority`; `Launch::Join` keeps `Replica` | client | S | 4, D1 | `cargo test -p eustress-client --bin eustress-client`; the harness (plan b) for Pong and the physics fixture |
| 6. Server code reaches the solo Player per D1 | engine publish, Worker, client fetch | M | D1 | a Worker test for the access rule; a Player test that a world without its server code shows the notice from step 9 |
| 7. The Rune core driver moves into common; both shells run Rune from it | common, engine | L | 3 | Box Head's BountyDirector and AbilityDirector in the harness |
| 8. Terrain for scripts in the Player (reads, then edits) | play-runtime, common | M | 4 | a harness case on a terrain fixture |
| 9. "Cannot run here" notices: a Space that needs something this shell lacks says so on screen through `ScreenNotice` (`play-runtime/src/screen_notice.rs`), never silently | play-runtime | S | 1 | a test: a Space with a server `Script` and no code shows the notice |
| 10. Browser: Authority for script-free and Rune-only Spaces first; Luau per D2 | player-web, common | L | 5, 7, D2 | `cargo check -p eustress-player-web --target wasm32-unknown-unknown`; the harness's wasm leg (plan b) |

Steps 2 and 9 are worth landing on their own even before the Authority
work: they fix every static HUD in solo play and turn every silent miss into
a message.

### The browser build

What can run on wasm32, today or after the moves above: the tree reader, the
shared draw, physics (avian builds there, `player-web/src/app.rs:106`), the
avatar, NPC, seats and joints (pure Rust), and Rune (pure Rust; common
already has a `realism-wasm` feature naming `rune-wasm`,
`common/Cargo.toml:175`).

What cannot, today: Luau. mlua wraps the C++ Luau VM, and
`wasm32-unknown-unknown` has no C++ standard library
(`player-web/Cargo.toml:50-53`). A draft exists for one script interface
with two backends (`common/src/scripting_vm/DRAFT_INTERFACE.md`, "pure" for
wasm32); it is not wired. Until a browser Luau exists, a Space with Luau
scripts opened in the browser must show a notice ("This Space's scripts need
the desktop Player", with the download link), not a silent world. That is
step 9.

Other browser limits: no particle draw on WebGL2 (the sim runs;
`particle_render.rs` `finish`), no blocking HTTP (HttpService calls are
refused), no local disk (records only, `PlaySpaceSource`).

### Risks

- Physics changes for every Player Space the moment unanchored parts turn
  Dynamic. A Space that looked fine frozen may now fall apart. The harness
  and the support table are how this gets caught.
- Running server code on the player's machine means the player can read and
  change it. Solo play has nobody to cheat but the player, but the code is
  no longer private (D1).
- One VM running server and client scripts together is what Studio does, so
  scripts that only work because of that (Box Head's `Players.LocalPlayer`
  inside a server script) keep working solo and stay broken for joined
  players. That is content, not runtime.
- The Rune move is the largest and most entangled (`rune_play.rs:57-68`).
- The two character-sync paths must become one, or the harness will show
  different `Character` trees.
- Avian is built with `parallel` and without `enhanced-determinism`
  (`eustress/Cargo.toml:87`), so physics can differ run to run and between
  native and wasm. The harness tolerances (plan b) are designed for that.

## 8. Plan (b): a parity conformance harness for any Space

### Where it lives

A new dev-only crate, `eustress/crates/play-conformance` (`publish = false`):

- `src/canonical.rs`: one canonical dump of a `DataModel`, promoted from the
  replication test's `dump`
  (`common/eustress-networking/src/repl/mod.rs:115`). Keyed by full name
  (with a stable index for duplicate names), each entry holds the class and
  sorted properties, attributes and tags. The hash is BLAKE3 over the
  serialized dump.
- `src/drive.rs`: builds a headless Play app for a shell (`MinimalPlugins`,
  `AssetPlugin`, `PhysicsPlugins`, the shell's Play plugins, no window or
  renderer), opens a Space folder, runs N fixed ticks at 60 Hz with the
  clock stepped by hand, and replays an input script through
  `common::machine_input` (the per-machine pull both shells already share).
- `spaces/<name>/`: the fixture Space folders, checked in.
- `spaces/<name>/inputs.toml`: input per tick, for example
  `[[tick]] at = 30, keys_down = ["W"], mouse = [640, 360], buttons = ["MouseButton1"]`.
- `goldens/<name>.<shell>.toml`: the canonical dump at chosen ticks, with its
  hash.

### What it compares

1. Studio Authority against Player Authority: the same Space, inputs and
   ticks. Before step 4, the Studio side is the engine's
   `PlayDataModelPlugin` in-process (as `seed.rs:1052` already does); after
   it, both sides are `PlayRuntimePlugin { role: Authority }` and the test
   guards against Studio-only additions.
2. Authority against a joined Replica, in-process: host and replica trees
   linked through the IO-free replication types (`HostReplicator`,
   `Replica`), as the `repl/mod.rs` tests already do, compared over what a
   player may see (`audience`).
3. Native against wasm32: the same driver built for wasm32 and run under
   `wasm-bindgen-test` in Node. Until a browser Luau exists, Luau Spaces
   assert the step 9 notice instead of a tree.
4. Each shell against its golden, so a change that moves both sides the same
   wrong way still fails.

### Tolerance rules

- Exact: class, name, parent, tags, booleans, strings, enums, integers,
  attribute names and their string or boolean values, instance counts per
  class.
- Numbers: rounded to 1e-4 before hashing.
- Anchored parts: exact after rounding.
- Bodies physics moves: compared after settling, in metres: within 0.05 m
  and 2 degrees between shells on the same machine, within 0.25 m and 10
  degrees between native and wasm. Velocities are not compared. A Space can
  tag instances `ConformanceLoose` to compare only that they exist and their
  class (debris, ragdolls).
- Randomness: a Space must seed its `Random`. Before any cross-shell
  compare, the harness runs each shell twice and fails if the two dumps
  differ (a determinism self-check).
- Time: the harness steps the session clock, never the wall clock.

### How it runs in the orchestrator's build

- `cargo test -p eustress-play-conformance --test conformance` runs every
  Space. Filters: `conformance::pong`, `conformance::physics_fixture`,
  `conformance::boxhead`, and by comparison `::studio_vs_player`,
  `::authority_vs_replica`, `::golden`.
- Regenerating goldens: `EUSTRESS_BLESS=1` with the same command. A bless is
  a reviewed diff in git, never automatic.
- The wasm leg: `wasm-pack test --node eustress/crates/play-conformance
  --features wasm-leg`, once per release build, not on every iteration.
- Each new comparison is shown to fail once with a planted change (for
  example, skipping `pull_collisions`) before it is trusted.

### The first three Spaces

| Space | Why | Ticks and checks |
|---|---|---|
| Physics fixture (new, small) | Six unanchored parts falling onto a ramp, one welded pair, one hinge, one seat, one NPC humanoid walking to a point, one `Touched` counter in a server script. Covers physics, joints, NPC, seats, Touched | Ticks 0, 60, 300; settled positions within tolerance; the Touched count exact |
| Pong (`Documents/Eustress/Games/Spaces/Pong`) | Parts only, all logic in one server script, input through a Player attribute, `CharacterAutoLoads` off | Ticks 0, 120, 600 with scripted paddle input; score digits exact; ball position within tolerance |
| Box Head | The whole stack: server Luau, Rune, a LocalScript HUD, remotes, NPC zombies, raycast weapons, impulses, sounds, particles | Ticks 0, 300 (first wave spawned), 1,200 with input (walk, fire, Q). `Remotes` and `GameState` exist; the HUD ScreenGui exists under `PlayerGui`; zombie count and the `Cash` attribute exact; zombie positions loose |

Box Head is McKale's content in his Documents folder; checking a copy into
the repo is D5.

## 9. Plan (c): a per-system support table, checked by a test

The table lives in the repo as data and as a document:

- `eustress/crates/play-runtime/src/play_support.rs`: a
  `const PLAY_SYSTEMS: &[PlaySystem]`, one row per system that runs in a Play
  frame: an id, the Bevy system and plugin type names it covers, and a
  status for each of `studio`, `player_solo`, `player_joined`,
  `browser_solo`, `browser_joined`: `Runs`, `Host` (arrives through
  replication), `Notice` (does not run here, and the screen says so), or
  `Missing { tracked_in }`.
- `docs/architecture/PLAY_SUPPORT.md`: generated from that table by the test
  (`EUSTRESS_BLESS=1` rewrites it; otherwise the test fails when the file
  and the table differ).

The test (`cargo test -p eustress-play-runtime --lib play_support`):

1. Builds Studio's Play plugin set headless. After step 4 that is
   `PlayRuntimePlugin { role: Authority }`; before it, the engine side runs
   the same check as `cargo test -p eustress-engine --lib play_support`.
2. Reads the `Update` schedule and collects every system in
   `PlayScriptSet::{Pull, Scripts, Apply, End}` and every session-start
   `OnEnter` system (Bevy 0.19 schedule-graph introspection; the exact API
   needs checking against 0.19 before it is written).
3. Fails when a system is in the schedule and in no row, when a row names a
   system that no longer exists, or when a row lacks a decision for any
   shell. So a system added to Studio Play without a Player and browser
   decision fails the build.
4. For every row marked `Notice`, checks that its notice exists in
   `screen_notice.rs`.

The first rows, from section 3, Box Head's needs first:

| id | studio | player_solo (today) | player_joined | browser_solo | browser_joined |
|---|---|---|---|---|---|
| `scripts.server_luau` | Runs | Missing (step 3, D1) | Host | Notice (D2) | Host |
| `scripts.local_luau` | Runs | Runs | Runs | Notice (D2) | Notice (D2) |
| `scripts.rune` | Runs | Missing (step 7) | Host | Missing (step 10) | Host |
| `session.player_gui` | Runs | Missing (step 2) | Host | Missing (step 2) | Host |
| `physics.dynamic_parts` | Runs | Missing (step 5) | Host | Missing (step 10) | Host |
| `pull.poses`, `pull.collisions` | Runs | Missing (step 4) | Host | Missing | Host |
| `npc.humanoid_move` | Runs | Missing (step 4) | Host | Missing | Host |
| `seats`, `vehicle_seats` | Runs | Missing (step 4) | Host | Missing | Host |
| `joints.scripted` | Runs | Missing (step 4) | Host | Missing | Host |
| `remotes` | Runs | Missing (no server) | Runs | Missing | Missing (no VM) |
| `terrain.script_reads`, `terrain.script_edits` | Runs | Missing (step 8) | Missing (client reads) | Missing | Missing |
| `commerce.session` | Runs | Missing (D3) | Host | Missing | Missing |
| `particles.draw` | Runs | Runs | Runs | Runs on WebGPU, Notice on WebGL2 | the same |
| `datastore.luau` | Missing | Missing | Missing | Missing | Missing |

## 10. Decisions for McKale

**D1. How a solo Player gets a Space's server code.** Today the player world
never carries it, on purpose, so players cannot read an author's server code.

- Option A, ship it with the world: publish a third manifest, a "solo
  world" with the server code, served to anyone who may play the listing.
  Solo play then works with no host. Cost: anyone can read and copy every
  server script of every listing.
- Option B, only for listings that opt in: the author ticks "Allow solo
  play" at publish (separate from Share Source). Listings without it play
  solo only while a host is live, and otherwise show "This Space needs a
  host". Each author chooses between privacy and hostless play.
- Option C, never on the player's machine: a solo Play starts a private
  hosted session (a dedicated server on Forge, or the author's machine), and
  the Player joins it as a Replica. Server code stays private, as in Roblox.
  Cost: hosting for every solo play, network latency, and nothing plays when
  no host capacity is up.

**D2. Luau in the browser.**

- Option A, a pure-Rust, Luau-compatible interpreter behind the draft
  `scripting_vm` interface: one script API with two backends. The longest
  path, but no C++ toolchain in the browser build.
- Option B, build the C++ Luau VM to WebAssembly separately (Emscripten) and
  call it from the Rust app through JavaScript. Sooner to a first working
  script; two toolchains, and a JavaScript bridge on every call whose cost is
  not measured.
- Option C, the browser stays a join-only viewer and shows "needs the
  desktop Player" for any Space with Luau. Smallest; the browser is then not
  a real Player for scripted Spaces.

**D3. Purchases in solo play.** Option A: refused, with a clear message.
Option B: test-mode purchases, as Studio does, for the listing's creator
only. Option C: live purchases through the Commerce API from a solo Player
(needs Security's review; live money).

**D4. Realism and sim values in the Player.** Option A: add them to the
shared runtime, so simulation Spaces run the same in the Player. Option B:
keep them Studio-only and mark them `Notice` in the support table.

**D5. Box Head in the repo.** Option A: check a copy into
`play-conformance/spaces/boxhead` (it becomes source-available with the
repo). Option B: the harness reads it from a folder named by
`EUSTRESS_CONFORMANCE_SPACES` on machines that have it, and says on screen
that it skipped elsewhere.

## 11. What was not verified

- Nothing was built or run. No Player, browser build or Studio Play was
  launched for this report. The "what McKale ran" column is derived from the
  code; the log line in section 5 is the check that confirms or refutes it.
- Which browser build the staging site serves: its `/player/host.js` was not
  reachable from this machine under the names tried.
- Whether the `ClientController` record in the staging chunk carries its
  full source: the chunk was searched for strings, not decoded.
- Ocean, Decals, Beams, Trails, SurfaceGui per class, ClickDetector and
  ProximityPrompt events, terrain in the browser, browser audio: marked
  UNVERIFIED or as found, not traced end to end.
- Whether a dedicated server's credential may read a listing's source world
  with Share Source off (`world.mjs:570-575` allows the author or an admin;
  the server credential's identity was not traced).
- The Bevy 0.19 schedule-introspection call the support-table test needs.
- Whether a host's script terrain edits replicate to a joined Player.

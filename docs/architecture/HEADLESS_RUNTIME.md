# Eustress Headless Runtime — Design Doc

> Canonical design for running `.eustress` spaces **apart from visualization** — physics, realism, scripts, and the agent bridge with no window. Composed from a verified ground-truth sweep of `eustress/crates` (2026-06-29, branch `main`). Status tags are honest; nothing here overclaims what exists.

---

## 1. Thesis

**Eustress is the Simulator** (per [WORLD_MODEL_SIMULATOR_ROADMAP.md](WORLD_MODEL_SIMULATOR_ROADMAP.md)): it outputs *true computable state* — geometry + physics + dynamics — that serves humans **and** programs/agents. A simulator whose only entry point is a 1600×900 window is half a simulator. The state loop (agent → action → state → observation) must run in CI, in containers, on a cloud box with no display, and inside an agent's tool-call without a desktop session.

This doc specifies how to get there **without forking the engine** — by factoring the headless-safe core out of the windowed editor and reusing it, then exposing it over the existing TCP bridge and the existing CLI.

### Reading conventions
- **Status tag** on work items: `[status: exists | extend | new | research]` and `[effort: S/M/L/XL]`.
- **Crates root is `eustress/crates`** (not repo-root `crates`).
- "Luau implies Rune" — the shared `common/scripting` host runs both; "scripts" means both runtimes.
- File refs are clickable: [`main.rs`](../../eustress/crates/engine/src/main.rs).

---

## 2. The three headless tiers

"Headless" is not one capability. It is three, and Eustress has very different coverage of each:

| Tier | Meaning | Today | Binary |
|---|---|---|---|
| **Inspect** | Read a space's state off disk; no sim | ✅ **Works** | [`eustress-space`](../../eustress/crates/eustress-space/src/main.rs) |
| **Simulate** | Tick physics + realism + scripts, no window | ❌ **Missing** | — |
| **Drive** | Query/command a running sim over a socket | ⚠️ **Partial** | [`eustress` CLI](../../eustress/crates/cli/src/main.rs) (stubbed) + bridge |

### 2.1 Inspect — done
[`eustress-space`](../../eustress/crates/eustress-space/src/main.rs) opens `world.fjalldb/` through the `worlddb` crate with **the engine never linked**: `open` (entity count, class histogram, bounds), `verify` (rkyv `CheckBytes` every core; non-zero exit on failure), `export` (binary → readable TOML tree). This is the portability + serialization-correctness escape hatch. It is the template for "small, engine-free, does one job." Nothing in this doc changes it.

### 2.2 Simulate — the gap
There is **no** binary that loads a space and runs the *real* simulation headlessly. The simulation stack —
- [`SimulationPlugin`](../../eustress/crates/engine/src/simulation/plugin.rs) (tick clock, watchpoints, telemetry, MCP sim-command drain),
- `RealismPlugin` + `ElectrochemistryPlugin` (materials/thermo/fluids closed-form),
- Avian 0.7 physics + the determinism pins,
- `EngineSoulPlugin` + Rune/Luau script execution,
- `WorldDbPlugin` (authoritative Fjall ECS store),
- `EngineBridgePlugin` (the agent drive surface)

— is **all added inline** onto the single windowed `App::new()` in [`main.rs:250`](../../eustress/crates/engine/src/main.rs) alongside ~100 render/UI plugins (Slint, `PartRenderingPlugin`, gizmos, tools, billboard pipeline). There is no way to instantiate the first list without the second.

The `--server` / `server_mode` flag exists ([`startup.rs:44`](../../eustress/crates/engine/src/startup.rs)) but is **parsed and never consumed** — a dead flag. The separate [`eustress-server`](../../eustress/crates/server/src/main.rs) binary *is* headless (`MinimalPlugins`), but its `server_tick` is **empty** ([`server/main.rs:351`](../../eustress/crates/server/src/main.rs)): no Avian, no realism, no `SimulationPlugin`, no entity load. It is a multiplayer-host skeleton, not a space simulator.

### 2.3 Drive — partial
[`engine_bridge`](../../eustress/crates/engine/src/engine_bridge/mod.rs) is a rich TCP JSON-RPC 2.0 surface — `ecs.query`, `ecs.inspect`, `entity.create/read/update/delete`, `sim.read`, `sim.step`, `raycast`, `tools.call`, `oplog_tail`, `ai_camera.*`, `viewport.capture` — discovered via `<universe>/.eustress/engine.port`. The MCP server already drives it through [`bridge_client.rs`](../../eustress/crates/mcp-server/src/bridge_client.rs) (clean, synchronous, std-only). **But the bridge only exists when the windowed engine runs.** And the [`eustress` CLI](../../eustress/crates/cli/src/main.rs)'s `agent` / `scene` / `stream` / `stats` subcommands are **stubs** that print "requires in-process access."

**Net:** you can inspect a dead space and drive a live *windowed* one. You cannot run a space's physics on a headless box, and the CLI can't drive anything.

---

## 3. What is — and isn't — render-coupled

The headline good news: the simulation core is **not inherently render-coupled**. The audit:

| Subsystem | Headless-safe? | Notes |
|---|---|---|
| [`SimulationPlugin`](../../eustress/crates/engine/src/simulation/plugin.rs) | ✅ Yes | Only UI touch is `Option<ResMut<OutputConsole>>` ([`plugin.rs:105`](../../eustress/crates/engine/src/simulation/plugin.rs)) — already optional, degrades to `None`. |
| `RealismPlugin`, `ElectrochemistryPlugin` | ✅ Yes | Closed-form math in `common`; no GPU. |
| Avian 0.7 + determinism pins | ✅ Yes | `Time::<Fixed>::from_hz(60.0)`, `SubstepCount(6)`, `GlobalRngSeed` ([`main.rs:616`](../../eustress/crates/engine/src/main.rs)). Tick-exact replay viable. |
| `EngineSoulPlugin` + Rune/Luau | ✅ Yes | Script VMs are pure logic. |
| `WorldDbPlugin`, `SpaceFileLoaderPlugin` | ✅ Yes | Disk/Fjall I/O. |
| `EngineBridgePlugin` | ✅ Yes | TCP + per-frame drain; no render deps. |
| Streaming / op-log | ✅ Yes | TCP + Fjall. |
| **`PlayModePlugin`** | ⚠️ **Split needed** | Its `PlayModeSystems` set is ordered `.after(SlintSystems::Drain)` and reads Slint button flags ([`play_mode.rs:1502`](../../eustress/crates/engine/src/play_mode.rs)). The **state** (`PlayModeState`, [`play_mode.rs:57`](../../eustress/crates/engine/src/play_mode.rs)) and the `OnEnter(Playing)` transitions (compile scripts, activate physics) are headless-safe; the **UI button handlers** are not. |
| `SharedCharacterPlugin` / `SkinnedCharacterPlugin` / `SharedAnimationPlugin` | ❓ Audit | Logic likely safe; skinned-mesh/animation may want `Assets<Mesh>`/`Assets<AnimationClip>` present (an `AssetPlugin` covers this — see §6.2). |
| Slint UI, tools, gizmos, `PartRenderingPlugin`, billboard, `ai_camera` | ❌ Render | Editor-only. `ai_camera` needs a render device for capture (see §7.3). |

**The one real refactor is `PlayModePlugin`.** Everything else is already either headless-safe or cleanly editor-only.

---

## 4. The TypeId constraint — resolved

The engine crate used to compile its modules **twice**: once into the library (`eustress_engine::simulation`, `::space`, `::soul`, `::play_mode`, …) and once more **bin-local** via ~104 `mod X;` declarations in [`main.rs`](../../eustress/crates/engine/src/main.rs). Two distinct compilations meant two distinct `TypeId`s per type — a lib-added system could never see a resource the bin inserted, and vice versa. That is why [`engine_bridge`](../../eustress/crates/engine/src/engine_bridge/mod.rs) was originally kept bin-local: its handlers read live resources (`StudioState`, `SpaceRoot`) and had to match whichever copy the editor actually populated.

**Resolved in commit `f10298b3` (2026-07-02): "untangle lib/bin dual-compilation — thin bin, one TypeId universe."** `main.rs`'s 108-line `mod` block collapsed to a single `use eustress_engine::{...}` import; every plugin the bin adds now comes from the lib's one compilation. `engine_bridge` — along with `history_stream`, `light_sync`, `photoreal`, `soul_script_migration` — was promoted to `pub mod` in [`lib.rs`](../../eustress/crates/engine/src/lib.rs) in the same change. `lib.rs` calls out the reason directly, right above `pub mod engine_bridge;`: promoting it is this plan's keystone, because a future `eustress-headless` bin needs lib-side bridge `TypeId`s.

**What this means for headless:** the hard constraint is gone. A future `eustress-headless` bin can `use eustress_engine::engine_bridge::EngineBridgePlugin;` — the same lib copy the editor uses, the same `TypeId`s, zero risk of the silent `None`-resource failure mode. The remaining factoring in §5 is now purely organizational: grouping `main.rs`'s ~100 `add_plugins` calls into a reusable `add_core_sim_plugins` / `add_editor_plugins` split, not a TypeId fix.

> One stale artifact survives the refactor: the comment above `.add_plugins(engine_bridge::EngineBridgePlugin)` in `main.rs` (~line 303) still reads "Bin-local... so its handlers share the bin's TypeIds — see the `mod engine_bridge` note above," describing the pre-refactor world (there is no such note above it anymore). Harmless — the code is correct — but worth a follow-up comment fix.

---

## 5. Target architecture

```
                    eustress_engine  (lib)
        ┌───────────────────────────────────────────────┐
        │  app_core::add_core_sim_plugins(app)           │  ← headless-safe
        │    space loader · WorldDb · Avian + determinism │
        │    realism · simulation · soul (Rune+Luau)      │
        │    services · streaming · op-log · ENGINE BRIDGE │
        │                                                 │
        │  app_editor::add_editor_plugins(app)            │  ← render/UI
        │    DefaultPlugins · Slint · tools · gizmos ·    │
        │    PartRendering · billboard · ai_camera         │
        └───────────────────────────────────────────────┘
            │                                   │
   ┌────────▼─────────┐               ┌─────────▼──────────┐
   │  eustress-engine │               │ eustress-headless  │  ★ new
   │  core + editor   │               │ core + ScheduleRun │
   │  (windowed)      │               │ (no window)        │
   └────────┬─────────┘               └─────────┬──────────┘
            │      both advertise <universe>/.eustress/engine.port
            └───────────────────┬───────────────┘
                       ┌─────────▼─────────┐
                       │  engine.port      │  TCP JSON-RPC
                       └─────────┬─────────┘
              ┌──────────────────┼──────────────────┐
        ┌─────▼─────┐      ┌─────▼─────┐      ┌──────▼─────┐
        │ eustress  │      │   MCP     │      │  AI agents │
        │   CLI     │      │  server   │      │  (POMDP)   │
        └───────────┘      └───────────┘      └────────────┘
```

### 5.1 `add_core_sim_plugins(app)` — the headless-safe set
Everything from §3 marked ✅, in dependency order. Verbatim list to lift from [`main.rs`](../../eustress/crates/engine/src/main.rs):

- Asset source registration (`space://`, `bundled://`) and `SpaceRoot` / `space_asset_source` sync.
- `avian3d::PhysicsPlugins` + `Gravity` + `Time::<Fixed>::from_hz(60.0)` + `SubstepCount(6)` + `SolverConfig` + virtual-time max-delta clamp + `DeterminismPlugin`.
- `RealismPlugin`, `SimulationPlugin`, `ElectrochemistryPlugin`.
- `SpaceFileLoaderPlugin`, `UniverseRegistryPlugin`, common `StreamingPlugin` (instance streaming), `WorldDbPlugin` (`#[cfg(world-db)]`).
- `EngineSoulPlugin`, `RunePhysicsBridgePlugin`, `RuneECSBindingsPlugin` (the GUI bridge is editor-only — audit), services (`PlayerService`, `DataStorePlugin`, `TeleportPlugin`, `MarketplacePlugin`, `TeamServicePlugin`, `GamepadServicePlugin` — gamepad is a no-op headless).
- `HistoryStreamPlugin`, op-log producer, `soul_script_migration`, `attribute_tag_migration`.
- **`PlayModeCorePlugin`** (new — see §5.3).
- `EngineBridgePlugin` — already lib-resident (§4); just needs including in the composition function.
- `#[cfg(streaming)]` `StreamNodePlugin` + `SimWriterResource` setup.

### 5.2 `add_editor_plugins(app)` — render/UI only
`DefaultPlugins` (window + render + winit), Slint (`SlintUiPlugin`, floating windows, service properties), `PartRenderingPlugin`, `MaterialSyncPlugin`, `LightClassPlugin`, `PhotorealPlugin`, all Smart Build Tools + gizmos + handles + modal tools, `AdornmentPlugin`, billboard pipeline + GUI, `ai_camera`, `CameraControllerPlugin`, diagnostics overlays, `cursor_badge`, timeline panels, `part_selection`, `UpdaterPlugin`, `WindowFocusPlugin`, `mesh_import`, `TxtToTomlWatcherPlugin`, `WorkshopPlugin`, `viga`, `generative_pipeline`, `StartupPlugin`.

Editor `main()` becomes: `let mut app = App::new(); app_core::add_core_sim_plugins(&mut app); app_editor::add_editor_plugins(&mut app); app.run();` — plus the existing panic-catch wrapper.

### 5.3 `PlayModeCorePlugin` — the one refactor
Split [`PlayModePlugin`](../../eustress/crates/engine/src/play_mode.rs) into:
- **`PlayModeCorePlugin`** (lib, headless-safe): `init_state::<PlayModeState>()`, the play/stop messages, `pause_physics_on_startup`, the Rune/Luau runtime resources, and **all `OnEnter`/`OnExit` transition systems** — `activate_physics_for_unanchored_parts`, `compile_scripts_on_play`, `start_luau_scripts_on_play`, `restore_scene_on_enter_edit`, etc. These are the actual simulate-start/stop logic and have no Slint dependency.
- **`PlayModeUiPlugin`** (editor only): `handle_start_play` / `handle_stop_play` / `handle_pause_toggle` / `play_mode_shortcuts` and the `PlayModeSystems.after(SlintSystems::Drain)` ordering. These read Slint button flags and stay in the editor tier.

The headless runner gets transitions without the UI. It enters `Playing` by issuing `run_simulation` through the existing sim-command drain ([`plugin.rs:518`](../../eustress/crates/engine/src/simulation/plugin.rs)) or by a direct `NextState<PlayModeState>` set at startup (see §6.2).

---

## 6. The `eustress-headless` binary

### 6.1 Placement
A new `[[bin]]` **in the engine crate** (`src/bin/headless.rs`), not a new crate — it must share the engine's lib compilation so bridge `TypeId`s line up (§4). Add to [`engine/Cargo.toml`](../../eustress/crates/engine/Cargo.toml):

```toml
[[bin]]
name = "eustress-headless"
path = "src/bin/headless.rs"
required-features = ["world-db"]   # headless authority = Fjall
```

### 6.2 Boot sequence
```rust
// src/bin/headless.rs  (sketch)
fn main() {
    let args = HeadlessArgs::parse();               // --space, --tick-rate, --ticks,
                                                    // --render(minimal|gpu), --watch, --out
    init_tracing(args.verbose);

    let mut app = App::new();
    // 1. Headless schedule driver instead of winit.
    match args.render {
        Render::Minimal => { app.add_plugins(MinimalPlugins); }          // no GPU
        Render::Gpu     => { app.add_plugins(headless_gpu_plugins()); }  // §7.3
    }
    app.add_plugins(AssetPlugin { file_path: "assets".into(), ..default() });
    app.add_plugins(StatesPlugin);                  // PlayModeState needs it

    // 2. Point SpaceRoot at the requested space BEFORE the loader runs
    //    (mirrors startup.rs --space handling).
    app.insert_resource(space::SpaceRoot(args.space.clone()));
    space::space_asset_source::set_space_asset_root(args.space.clone());

    // 3. The shared core (physics, realism, sim, scripts, WorldDb, bridge).
    eustress_engine::app_core::add_core_sim_plugins(&mut app);

    // 4. Auto-enter Playing (or wait for a bridge/CLI run_simulation command).
    if args.autoplay {
        app.add_systems(Startup, |mut s: ResMut<NextState<PlayModeState>>| {
            s.set(PlayModeState::Playing);
        });
    }
    // 5. Headless exit: stop after --ticks, or run until signal.
    if let Some(n) = args.ticks { app.add_plugins(TickLimitPlugin(n)); }

    app.run();
}
```

`MinimalPlugins` already bundles `ScheduleRunnerPlugin`, which is exactly how [`eustress-server`](../../eustress/crates/server/src/main.rs) drives its loop today — proven in-repo. The fixed 60 Hz timestep means `--ticks N` is deterministic wall-clock-independent.

### 6.3 Flags (v1)
| Flag | Default | Meaning |
|---|---|---|
| `--space <path>` | required | `.eustress` space root (or `--universe` → first space) |
| `--tick-rate <hz>` | `60` | Render/update loop rate (sim fixed-step stays 60 Hz) |
| `--ticks <n>` | ∞ | Stop after N sim ticks, exit 0 |
| `--autoplay` | `true` | Enter `Playing` at startup |
| `--render <minimal\|gpu>` | `minimal` | GPU tier enables `ai_camera`/`viewport.capture` (§7.3) |
| `--watch <key>` (repeat) | — | Pre-register watchpoints; dump to `--out` |
| `--out <file>` | — | Write the `SimulationRecording` JSON on exit |
| `--port <n>` | OS-assigned | Force bridge port (else `engine.port` auto) |
| `--no-bridge` | off | Disable the TCP bridge (pure batch mode) |

---

## 7. Drive surface — CLI over the bridge

### 7.1 Share the bridge client
Lift [`bridge_client.rs`](../../eustress/crates/mcp-server/src/bridge_client.rs) out of `mcp-server` into a small shared crate (`eustress-bridge-client`) or `eustress-tools`. It is already engine-free, synchronous, std-only, and has the per-universe + global `engine.port` discovery the CLI needs. The MCP server keeps using it; the CLI gains it.

### 7.2 Un-stub and extend the CLI
Replace the `agent` / `scene` / `stream` stubs ([`cli/src/main.rs:318`](../../eustress/crates/cli/src/main.rs)) with real bridge calls, and add a sim-control surface that works against **either** shell (headless runner or running editor):

| New / fixed verb | Bridge method |
|---|---|
| `eustress run <space> [--ticks N] [--out f]` | launches `eustress-headless` (or attaches if `engine.port` live) |
| `eustress sim step --ticks N` | `sim.step` |
| `eustress sim run [--scale x] [--for s]` | writes `run_simulation` sim-command |
| `eustress sim stop` | `stop_simulation` |
| `eustress ecs query <filter>` | `ecs.query` |
| `eustress entity create\|read\|update\|delete` | `entity.*` |
| `eustress raycast --from .. --dir ..` | `raycast` |
| `eustress oplog tail [-n N]` | `oplog_tail` |
| `eustress inspect` | `ecs.inspect` |

The existing `sim replay/best/convergence` history commands stay (they read the stream ring buffer).

### 7.3 Render tiers for observation
- **`--render minimal`** — `MinimalPlugins`, no GPU. Sim, scripts, physics, bridge, op-log all work. `ai_camera.capture` / `viewport.capture` return a clear "no render device in minimal mode" error. Best for CI, containers, headless cloud.
- **`--render gpu`** — `DefaultPlugins` with `WindowPlugin { primary_window: None, .. }` + `ScheduleRunnerPlugin` (no winit window), keeping `RenderPlugin`. The off-screen [`ai_camera`](../../eustress/crates/engine/src/ai_camera.rs) already renders to an `Image`, never the window — so the **AI keeps its eyes** with no desktop. Requires a usable GPU/adapter (real or software, e.g. `llvmpipe`/WARP) on the box.

---

## 8. One-shot batch runner — "spaces as a function"

The agent-eval and CI primitive the generative loop (§2 of the roadmap) wants:

```bash
eustress run ./spaces/V-Cell \
    --ticks 600 \
    --watch battery.voltage --watch battery.temperature_c \
    --out runs/vcell_run.json
echo $?        # non-zero if a breakpoint tripped
```

Boots `eustress-headless`, enters `Playing`, runs 600 deterministic ticks, and on exit writes the `SimulationRecording` — the export path **already exists** ([`plugin.rs:116`](../../eustress/crates/engine/src/simulation/plugin.rs)) and currently fires on `OnEnter(Editing)`. The runner just needs a `TickLimitPlugin` that flips to `Editing` (triggering the export) then `AppExit`. Breakpoints (`BreakPointRegistry`) already set exit conditions; map a tripped breakpoint to a non-zero process exit.

This makes a space a pure function: `(space, inputs, ticks) → recording.json + exit code`. That is the row-factory for the synthetic-data flywheel.

---

## 9. Sequencing

| Phase | Work | Effort | Status |
|---|---|---|---|
| **P0** | Untangle lib/bin dual-compilation, one `TypeId` universe, promote `engine_bridge`/`history_stream`/`light_sync`/`photoreal`/`soul_script_migration` to the lib | L | **`done`** — commit `f10298b3`, 2026-07-02 |
| **P1** | Split `PlayModePlugin` → `PlayModeCorePlugin` + `PlayModeUiPlugin` | M | `new` |
| **P2** | `app_core::add_core_sim_plugins` / `app_editor::add_editor_plugins` — group `main.rs`'s ~100 `add_plugins` calls into a composition function the headless bin can share | S | `new` |
| **P3** | `eustress-headless` bin (`MinimalPlugins` + core + autoplay + `TickLimitPlugin`) | S | `new` |
| **P4** | Lift `bridge_client` to shared crate; un-stub CLI; add `sim`/`ecs`/`raycast`/`run` verbs | M | `extend` |
| **P5** | `eustress run` batch runner + recording dump + breakpoint exit code | S | `extend` |
| **P6** | `--render gpu` tier (windowless `DefaultPlugins`) for `ai_camera` capture | M | `new` |
| **P7** | Retire dead `--server` flag; fold `eustress-server` onto `add_core_sim_plugins` (one sim path) or document it as multiplayer-only | S | `extend` |

**P1 → P2 → P3** is the spine that delivers "run spaces apart from visualization." With P0 already done, P1 and P2 are both small — P1 is a mechanical plugin split with no TypeId risk, and P2 is pure reorganization (grouping existing `add_plugins` calls, not fixing a bug). P4/P5 make the spine *usable*; P6 is the observation upgrade.

---

## 10. Risks & honest gaps

- **Hidden render coupling.** §3 is a static audit. Some "logic" plugin may `Query<&Camera>` or expect `Assets<StandardMaterial>` to exist. `MinimalPlugins` lacks render asset types; `AssetPlugin` covers `Assets<T>` registration for most, but a missing-type panic is the likely first failure. Mitigation: bring up P3 with the **minimum** core set, add plugins one at a time, fix or gate each panic. The `Option<Res<...>>` pattern (as `SimulationPlugin` already uses for `OutputConsole`) is the escape valve for soft deps.
- **`PlayModeState` snapshot/restore.** `restore_scene_on_enter_edit` deserializes a world snapshot; verify it round-trips with no editor camera/selection present.
- **Scripts that call editor-only bindings.** A Luau/Rune script reaching a GUI binding must degrade gracefully headless (the GUI bridge is editor-tier). Audit `rune_ecs_module` for editor-only calls.
- **GPU presence for `--render gpu`.** Cloud boxes may have no adapter; document the software-rasterizer fallback and keep `minimal` the default.
- **Two sim paths during migration.** Until P7, `eustress-server` and `eustress-headless` both exist. Keep them distinct in docs (server = multiplayer host; headless = single-process simulator) until folded.
- **Not in scope:** multiplayer replication headless (that's the server's job), and the FEA/topology-opt gaps already tracked in the roadmap.

---

## 11. Acceptance gates

A phase is "done" only when the gate passes — not when it compiles.

- **P3 gate:** `eustress-headless --space <V-Cell> --ticks 300 --watch battery.voltage` runs to completion, prints a non-empty watchpoint series, exits 0, **and writes no window**. Verified by running it and reading the log + recording.
- **P4 gate:** with a headless runner live, `eustress ecs query Part` and `eustress sim step --ticks 10` return real JSON from the bridge (round-trip proven, the contention the roadmap flags as "blocks the whole agent loop").
- **P5 gate:** `eustress run` produces a recording JSON whose tick count matches `--ticks`, and a space with a tripped breakpoint exits non-zero.
- **Determinism gate:** the same space + same `--ticks` produces byte-identical recordings across two runs (the determinism pins make this a real, testable claim).

---

## 12. Appendix — file change map

| File | Change |
|---|---|
| [`engine/src/play_mode.rs`](../../eustress/crates/engine/src/play_mode.rs) | Split plugin into `PlayModeCorePlugin` (lib) + `PlayModeUiPlugin` (editor) |
| `engine/src/app_core.rs` *(new)* | `add_core_sim_plugins(&mut App)` |
| `engine/src/app_editor.rs` *(new)* | `add_editor_plugins(&mut App)` |
| [`engine/src/lib.rs`](../../eustress/crates/engine/src/lib.rs) | `pub mod app_core; pub mod app_editor;` (`engine_bridge` already promoted — P0, done) |
| [`engine/src/main.rs`](../../eustress/crates/engine/src/main.rs) | Reduce the inline `add_plugins` chain to `add_core_sim_plugins(&mut app)` + `add_editor_plugins(&mut app)` + run |
| `engine/src/bin/headless.rs` *(new)* | The `eustress-headless` binary (§6) |
| `engine/src/tick_limit.rs` *(new)* | `TickLimitPlugin(n)` → stop + export + `AppExit` |
| [`engine/Cargo.toml`](../../eustress/crates/engine/Cargo.toml) | `[[bin]] eustress-headless`, `required-features = ["world-db"]` |
| `crates/bridge-client/` *(new)* or `eustress-tools` | Home for the lifted `bridge_client` |
| [`mcp-server/src/bridge_client.rs`](../../eustress/crates/mcp-server/src/bridge_client.rs) | Re-export from the shared crate |
| [`cli/src/main.rs`](../../eustress/crates/cli/src/main.rs) | Un-stub `agent`/`scene`; add `sim`/`ecs`/`entity`/`raycast`/`oplog`/`run` verbs over the bridge |
| [`engine/src/startup.rs`](../../eustress/crates/engine/src/startup.rs) | Remove dead `server_mode`, or repurpose `--headless` to exec the new bin |

---

*Companion docs: [WORLD_MODEL_SIMULATOR_ROADMAP.md](WORLD_MODEL_SIMULATOR_ROADMAP.md) (why the simulator + agent loop is the moat), [SCALING_ARCHITECTURE.md](SCALING_ARCHITECTURE.md) (10M-entity persistence vs live ECS). The headless runtime is the substrate both assume.*

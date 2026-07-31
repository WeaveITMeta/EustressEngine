# Rune Scripting

Rune is the primary scripting runtime for Eustress. This document describes how
a Rune script reaches the engine — the API surface it can call, the three places
it runs, and the rules each of those places follows.

Luau is the sibling runtime (Roblox-compatible, `.luau` / `.lua`); it uses a
different host and is documented separately.

## The API surface

Every Eustress function lives under the `eustress` crate namespace, so scripts
either import or fully qualify:

```rune
use eustress::{log_info, set_sim_value, Vector3, Instance};

pub fn on_init() {
    log_info("hello");
    set_sim_value("demo.value", 1.0);
}
```

The vocabulary is defined in exactly one place —
`engine::soul::rune_api::engine_rune_modules()` — and installed identically into
the play-mode compiler, the command-bar one-shot, and the script-editor
analyzer. What the editor autocompletes is what compiles and what runs.

It contains three module groups:

| Namespace | Source | Contents |
|---|---|---|
| `eustress::*` | `soul::rune_ecs_module::create_ecs_module` | sim values, Instance API, parts, camera, input, GUI, raycasting, tags, HTTP, DataStore, TweenService, `task`, Marketplace, RunService, plugin API, `Vector3` / `Color3` / `CFrame` / `UDim` / `UDim2` |
| `eustress::realism::<domain>::*` | `common::realism::scripting::laws` | STEM kernel laws — electrical, mechanics, thermodynamics, optics, propulsion, chemistry, … |
| `event_bus::*` | `soul::rune_ecs_module::create_event_bus_module` | `fire`, `fire_number`, `fire_bool`, `event_names`, `connection_count` |

The Player (`eustress-client`) installs a smaller set: the shared GUI + logging
module and `event_bus`. It has no ECS module.

### Adding a type to the API

An `#[derive(rune::Any)]` type must carry `#[rune(item = ::eustress)]`:

```rust
#[derive(Debug, Clone, Copy, rune::Any)]
#[rune(item = ::eustress)]
pub struct Vector3 { /* … */ }
```

`Module::with_crate("eustress")` places *functions*; it does not place types. A
type is placed by its own `item` attribute and defaults to the crate root.
Without the attribute the type still installs cleanly and its instance methods
still work, but every **associated** function becomes unreachable — a script
calling `Vector3::new(…)` fails with `Missing item ::eustress::Vector3::new`,
because the compiler resolves associated items under the type's own path.

Rune's own modules follow the same rule; `HashMap` carries
`#[rune(item = ::std::collections::hash_map)]`.

## Lifecycle callbacks

A `.rune` script in a Space defines any of:

```rune
pub fn on_init()        // once, when Play/Run starts
pub fn on_ready()       // one frame after on_init — subtree is complete
pub fn on_update(dt)    // every frame
pub fn on_exit()        // once, when Play/Run stops
pub fn on_button_click(name)   // a ScreenGui TextButton was clicked
```

None are required. A script with none of them compiles and simply does nothing.

Each callback runs in a fresh `rune::Vm` built from the compiled unit, so
**module-level state does not persist between callbacks or frames**. Carry state
across frames through `set_sim_value` / `get_sim_value`, an attribute, or the
scene itself.

## Where scripts run

### Play mode and Run mode

`Run` (free camera, no character) and `Play` (spawns your avatar) are both
`PlayModeState::Playing`, along with Server and Client modes. Scripts behave
identically in all four.

`soul::rune_play::drive_rune_frame` runs one frame of script execution as a
single Bevy system: install bridges → `on_init` / `on_ready` / `on_update` →
drain effects → tear down.

It is one system on purpose. `#[rune::function]` fns take no Bevy
`SystemParam`, so engine state reaches them through `thread_local!` bridges.
Bevy's `.after()` orders systems but does not pin them to a thread — split
across systems, the install and the read land on different workers and scripts
see empty bridges. Install and use must be the same system.

Scripts are compiled on entering Play (`compile_scripts_on_play`) and
hot-recompiled mid-session when the file watcher marks a source dirty, so
saving a `.rune` file in an external editor takes effect on the next frame
without leaving Play.

### The command bar

Defaults to Rune (toggle to Luau in the bar). It takes **expressions**, not
programs:

```rune
log_info("hi")
```

Bare input is wrapped in `pub fn main() { … }` before compiling, with leading
`use` items hoisted to item level, and gets `use eustress::*;` prepended so
functions and types resolve without an import line. Both apply only to bare
input: a snippet that declares any item (`fn`, `struct`, `impl`, `const`, `mod`)
is treated as a program and compiled exactly as written, imports included —
same contract as a `.rune` file on disk.

The prelude glob goes first, so an explicit `use eustress::log_info;` in the
same snippet shadows it rather than colliding with it.

The command bar runs in Edit mode as an authoring action, so `Instance::new()`
results are **persisted to disk** as `_instance.toml` and `Instance:Destroy()`
moves the backing file to the Space's trash.

### MCP / CLI

The `execute_rune` MCP tool writes `SoulService/<name>/<name>.rune` and the
engine picks it up through the normal file-watcher path — it becomes an ordinary
play-mode script. Compile diagnostics are published to the
`rune.compile.error` stream topic as `{ script, line }`, one event per
diagnostic, so a caller can iterate until the script compiles clean.

## Effects: queued, then drained

Native Rune functions have no `&mut World`, so side effects are queued during
the VM call and applied by the host afterwards, in the same frame:

| Script call | Queue | Applied as |
|---|---|---|
| `Instance::new(...)` | `InstanceRegistry` | spawned entity |
| `Instance:Destroy()` | `PENDING_DESTROY` | despawn |
| `Instance:Set(...)` | `PENDING_PROPERTY_WRITES` | `PropertyCommand::execute` |
| `set_sim_value(...)` | `SIM_VALUE_WRITES` | merged into `SimValuesResource` |
| `gui_set_*` | `GuiCommand` queue | GUI bridge |

### Play-mode spawns are transient

Instances created during Play are spawned ECS-only, marked `RuneSpawned`, and
despawned on Stop. Nothing is written to disk — Play-mode state is reverted on
Stop, so persisting it would leave debris after every session. Only Part-family
classes (`Part`, `MeshPart`, `SpherePart`, `CylinderPart`, `WedgePart`,
`CornerWedgePart`) materialize during Play; other classes log once and are
skipped. Author them from the command bar in Edit mode instead.

## Sim values

`set_sim_value(key, value)` is the general channel from a script to the rest of
the engine. Writes are merged into `SimValuesResource` each frame, which is what
watchpoints, simulation recordings, `runtime-snapshot.json`, and the MCP
`get_sim_value` / `list_sim_values` tools read.

Keys written **this frame** are additionally published as `ScriptSimWrites`, so
consumers can distinguish an explicit script assertion from an engine-published
value. `electrochemistry::apply_sim_values_to_ecs` uses this: a direct
`set_sim_value("battery.current", …)` overrides the default discharge mode for
as long as the script keeps asserting it.

Ordering within a frame is `drive_rune_frame` → `apply_sim_values_to_ecs` →
`electrochemical_tick` → `publish_echem_to_sim_values`, so a script write
reaches the physics in the same frame and the script reads the previous frame's
published state.

## Raycasting

```rune
if let Some(hit) = eustress::workspace_raycast(origin, direction, None) {
    log_info(`hit ${hit.instance} at ${hit.distance} m`);
}
```

Avian's `SpatialQuery` is a `SystemParam` over ECS queries; it cannot be handed
to a native Rune function, and the VM never holds `&World`. A raycast is
therefore a request: queued during the call, executed in `PostUpdate`, and read
back on the script's next call.

Requests and answers are keyed by **call slot** — the Nth raycast a script
performs in a frame, with the counter reset each frame. The Nth call reads the
answer to the Nth call of the previous frame. For the common shape (a fixed
number of casts per `on_update`) that is exact, one frame stale. A script that
branches into a different number of casts is protected by an origin-drift guard:
an answer whose recorded origin has moved more than
`ScriptSpatialQuery::staleness_guard_m` is discarded and the call returns
nothing rather than a wrong hit. The first frame always returns nothing.

## The Kernel gate (L12)

Every program passes `soul::kernel::validate_rune_rewrite` before compiling. It
is pure, deterministic, and offline — no API call, no learned threshold.

- **Syntax** — a Rune parse failure is fatal.
- **Withheld capability** — a catalogued call whose class the active universe
  does not grant is fatal. This is the gate that enforces universe policy.
- **Unknown capability** — advisory. The Rune linker is the authority on whether
  a symbol resolves; a call that exists in neither the catalog nor an installed
  module fails to link anyway.
- **Scope rules** — path traversal in `read_space_file` / `write_space_file` is
  fatal; immutable-physics universes reject `WorldLaw` mutations.
- **Entrypoint contract** — the one-shot context accepts `main` / `on_init`; the
  play context accepts the lifecycle hooks.

`UniverseLaws::eustress_core_default()` grants the full vocabulary, so ordinary
scripts pass unchanged. Named universes opt into restriction by withholding
capability classes.

The command-bar path hard-rejects on a fatal verdict (no side-effect window).
The play-mode path validates and logs; see the `kernel-play-gate-policy` TODO in
`rune_api.rs` for the open question of whether it should hard-reject too.

## Errors

Compile diagnostics and runtime errors reach the **Output panel** tagged `rune`,
one entry per diagnostic, formatted `script:line:col: severity: message`. The
same diagnostics drive the Problems panel and the editor's squiggle overlay via
`script_editor::analyzer`, and compile errors are teed to the
`rune.compile.error` stream topic.

A missing callback is not an error — `Missing entry \`on_update\`` is filtered.
Everything else surfaces.

## Feature flag

All Rune integration is gated behind `realism-scripting`, which is part of the
engine's default `core` tier. With the feature off, every entry point degrades
to a stub that reports Rune is unavailable.

## Related files

- `crates/engine/src/soul/rune_play.rs` — per-frame play/run driver
- `crates/engine/src/soul/rune_api.rs` — module set, compile-on-play, error drain
- `crates/engine/src/soul/rune_ecs_module.rs` — the `eustress::*` API
- `crates/engine/src/soul/kernel/` — the L12 validator, catalog, and laws
- `crates/engine/src/spatial_query_bridge.rs` — raycast request/answer bridge
- `crates/common/src/soul/rune_runtime.rs` — compile, callbacks, one-shot
- `crates/common/src/soul/rune_gui_module.rs` — the client's GUI + logging module

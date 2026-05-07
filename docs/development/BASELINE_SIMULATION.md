# BASELINE_SIMULATION.md

> The reference recipe for adding a new simulation domain to Eustress.
> Use this as the starting checklist for any new kernel laws, ECS components,
> TOML schemas, watchpoints, scripts, and demo scenes.

This document codifies the patterns proven by the **electrochemistry / V-Cell**
implementation and generalizes them so a new domain (life sciences, biotech,
fluids, robotics, structural, …) can be added in a predictable way.

---

## Table of Contents

1. [Philosophy](#1-philosophy)
2. [Domain Anatomy](#2-domain-anatomy)
3. [Step 1 — Kernel Laws](#3-step-1--kernel-laws)
4. [Step 2 — ECS Components](#4-step-2--ecs-components)
5. [Step 3 — TOML Schema](#5-step-3--toml-schema)
6. [Step 4 — Tick System](#6-step-4--tick-system)
7. [Step 5 — Sim-Value Bridge](#7-step-5--sim-value-bridge)
8. [Step 6 — Watchpoints](#8-step-6--watchpoints)
9. [Step 7 — Rune Scripting Surface](#9-step-7--rune-scripting-surface)
10. [Step 8 — Demo Scene](#10-step-8--demo-scene)
11. [Step 9 — Workshop Mode](#11-step-9--workshop-mode)
12. [Step 10 — Documentation & Tests](#12-step-10--documentation--tests)
13. [Reference Implementation Pointers](#13-reference-implementation-pointers)
14. [Per-Domain Quick Specs](#14-per-domain-quick-specs)
15. [Integration Checklist](#15-integration-checklist)

---

## 1. Philosophy

Every domain must satisfy four invariants:

1. **Laws are pure functions in Rust** — no globals, no Bevy types, no I/O. They
   live in `eustress/crates/common/src/realism/laws/<domain>.rs` and read like a
   physics textbook with paper references in the doc-comments.
2. **State is an ECS component** — physical state is a `Component` on entities,
   not a bag in a `Resource`. This is what lets one space contain many cells,
   bioreactors, robots, columns, organisms in parallel without aliasing.
3. **Authoring is plain TOML** — every component round-trips to a section in
   `_instance.toml`. No proprietary blobs, git-diffable, AI-readable.
4. **Observation is a sim-value key** — every interesting field must be
   publishable as `<domain>.<field>` so watchpoints, breakpoints, recordings,
   Rune scripts, MCP tools, and the timeline panel can all read it through one
   surface.

If a feature breaks any of those, redesign before merging.

---

## 2. Domain Anatomy

A complete domain ships **eight** artifacts. None are optional for an alpha
release; all are required for the domain to be discoverable, scriptable,
auditable, and AI-readable.

| # | Artifact | Path |
|---|---|---|
| 1 | Kernel-law module | `eustress/crates/common/src/realism/laws/<domain>.rs` |
| 2 | ECS component(s) | `eustress/crates/common/src/realism/particles/components.rs` |
| 3 | TOML schema | `eustress/crates/engine/src/space/instance_loader.rs` (`Toml<Domain>State`) |
| 4 | Tick system | `eustress/crates/engine/src/simulation/<domain>.rs` |
| 5 | Sim-value publisher | inside the tick system file |
| 6 | Watchpoint registration | `simulation/plugin.rs` (`register_<domain>_watchpoints`) |
| 7 | Rune module bindings | `eustress/crates/engine/src/soul/rune_ecs_module.rs` |
| 8 | Demo scene + script | `assets/scene_templates/<Domain>Demo/` + `*.rune` |

---

## 3. Step 1 — Kernel Laws

Create `eustress/crates/common/src/realism/laws/<domain>.rs` with **pure
functions only**. Each function is one equation, named after the equation, with
arguments in canonical units (SI), doc-commented with a paper reference.

### 3.1 File template

```rust
//! # <Domain> Laws
//!
//! Fundamental <domain> equations for general-purpose simulation.
//! Domain-agnostic implementations of core principles.
//!
//! ## Table of Contents
//!
//! 1. **<Law>** — short description
//! 2. **<Law>** — short description

use crate::realism::constants;

// ============================================================================
// 1. <Law Group>
// ============================================================================

/// One-line description with the equation in inline LaTeX-like form:
/// `y = f(x)`
///
/// # Arguments
/// * `arg1` — what it is (unit)
/// * `arg2` — what it is (unit)
///
/// # References
/// * Author, Year. Title. doi:10.xxxx
#[inline]
pub fn law_name(arg1: f32, arg2: f32) -> f32 {
    // Guard against degenerate inputs first.
    if arg2 <= 0.0 { return 0.0; }
    // Body.
    arg1 / arg2
}
```

### 3.2 Rules

- **No `Component`, no `Resource`, no `Query`** — laws are reusable from CLI,
  tests, MCP tools, the LSP, and Rune scripts.
- **Guard divide-by-zero / log(0) / sqrt(neg)** at the top of every function
  and return a sensible identity (the existing `electrochemistry::nernst_potential`
  returning `e_standard` when `activity_ratio <= 0.0` is the canonical pattern).
- **`#[inline]` on hot paths** — small algebraic functions get inlined; complex
  multi-step laws (Butler-Volmer, Gillespie step) do not need the hint.
- **f32 by default** — match the existing realism crate. Promote to f64 only
  when you can demonstrate a measurable error budget violation in a test.

### 3.3 Register the module

In `eustress/crates/common/src/realism/laws/mod.rs`:

```rust
pub mod thermodynamics;
pub mod mechanics;
pub mod conservation;
pub mod electrochemistry;
pub mod <domain>;            // ← add this

pub mod prelude {
    pub use super::thermodynamics::*;
    pub use super::mechanics::*;
    pub use super::conservation::*;
    pub use super::electrochemistry::*;
    pub use super::<domain>::*;   // ← and this
}
```

---

## 4. Step 2 — ECS Components

State that mutates during a tick lives in a Bevy `Component`. Static parameters
(rate constants, geometry) can live in the same component or split off into a
`<Domain>Params` component if they're set once and never changed.

### 4.1 Component template

In `eustress/crates/common/src/realism/particles/components.rs`:

```rust
/// Runtime state for the <domain> system.
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct <Domain>State {
    /// Field with unit in the doc-comment. Always SI.
    pub field_a: f32,
    /// Vector field — use `Vec<f32>` for time-varying length, `[f32; N]` for fixed.
    pub field_b: Vec<f32>,
    /// Counter — `u32` is fine for cycle counts, generations, tick numbers.
    pub counter: u32,
}

impl Default for <Domain>State {
    fn default() -> Self {
        Self {
            field_a: 0.0,
            field_b: Vec::new(),
            counter: 0,
        }
    }
}

impl <Domain>State {
    /// Standard initial state — call from tests and demo scenes for parity.
    pub fn standard() -> Self {
        Self::default()
    }
}
```

### 4.2 Rules

- **Derive `Component, Reflect, Clone, Debug, Serialize, Deserialize`** — all
  five are required: `Component` to attach, `Reflect` for Bevy-side scripting,
  `Clone+Debug` for snapshots, `Serialize+Deserialize` for TOML round-tripping
  and the binary save format.
- **`#[reflect(Component)]`** — without it the Bevy reflection registry won't
  see the component and play-mode snapshot/restore breaks.
- **Pair with `ThermodynamicState` whenever the domain emits/absorbs heat.**
  See `electrochemistry::publish_echem_to_sim_values` for the pattern of
  reading `Option<&ThermodynamicState>` alongside the domain component.

---

## 5. Step 3 — TOML Schema

Authors should be able to drop a `[<domain>]` section into any
`_instance.toml` and have the engine load it. The instance-loader holds a typed
mirror struct that converts to the runtime component.

### 5.1 Toml mirror struct

In `eustress/crates/engine/src/space/instance_loader.rs`, alongside
`TomlElectrochemicalState`:

```rust
/// <Domain> state as it appears in `_instance.toml [<domain>]`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Toml<Domain>State {
    #[serde(default)] pub field_a: f32,
    #[serde(default)] pub field_b: Vec<f32>,
    #[serde(default)] pub counter: u32,
}

impl Toml<Domain>State {
    pub fn to_component(&self)
        -> eustress_common::realism::particles::prelude::<Domain>State
    {
        eustress_common::realism::particles::prelude::<Domain>State {
            field_a: self.field_a,
            field_b: self.field_b.clone(),
            counter: self.counter,
        }
    }
}
```

Add the field to `InstanceDefinition`:

```rust
/// Optional <domain> state (dynamic on any class)
#[serde(default)]
pub <domain>: Option<Toml<Domain>State>,
```

### 5.2 Author-facing TOML

```toml
[<domain>]
field_a = 1.0
field_b = [0.0, 0.0, 0.0]
counter = 0
```

### 5.3 Rules

- **Every field gets `#[serde(default)]`** — partial TOMLs must load. The
  author should be able to write only `field_a = 1.0` and have the rest fall
  back to component defaults. This is what kept the V-Cell battery loadable
  through several iterations of schema evolution.
- **Never strip unknown sections in the auto-save path.** `instance_loader.rs`
  uses a raw `toml::Value` patch when writing back; preserve that pattern when
  adding the new section so domain-specific data isn't clobbered by transform
  saves.
- **Snake-case keys on disk** — the loader normalizes incoming keys but write
  out snake-case for diff cleanliness.

---

## 6. Step 4 — Tick System

The tick system is the bridge from kernel-law functions to ECS state. It runs
in `Update` only when `PlayModeState::Playing`, in a fixed three-stage chain:

```
apply_sim_values_to_ecs  →  <domain>_tick  →  publish_<domain>_to_sim_values
```

### 6.1 Tick template

Create `eustress/crates/engine/src/simulation/<domain>.rs`:

```rust
//! # <Domain> Tick System
//!
//! Advances <Domain>State each simulation tick using kernel-law functions
//! from `eustress_common::realism::laws::<domain>`.

use bevy::prelude::*;
use eustress_common::realism::laws::<domain> as laws;
use eustress_common::realism::particles::components::{<Domain>State, ThermodynamicState};
use eustress_common::simulation::SimulationClock;

use crate::play_mode::PlayModeState;

pub struct <Domain>Plugin;

impl Plugin for <Domain>Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                apply_sim_values_to_ecs,
                <domain>_tick.after(apply_sim_values_to_ecs),
                publish_<domain>_to_sim_values.after(<domain>_tick),
            ).run_if(in_state(PlayModeState::Playing)),
        );
    }
}

fn <domain>_tick(
    clock: Res<SimulationClock>,
    mut query: Query<(&mut <Domain>State, Option<&mut ThermodynamicState>)>,
) {
    let dt = clock.dt() as f32;
    if dt <= 0.0 { return; }

    for (mut state, _thermo) in &mut query {
        // 1. Read inputs (other components, sim-values applied above).
        // 2. Call kernel-law functions.
        // 3. Update state in place.
        state.field_a = laws::law_name(state.field_a, dt);
    }
}
```

### 6.2 Rules

- **`SimulationClock::dt()` is your only time source.** Never use `Time::delta_seconds()`
  — that's wall-clock; SimulationClock honors the time-compression slider so a
  1-year-per-second study still computes correctly.
- **`PlayModeState::Playing` gate is mandatory.** Editor mode must not tick
  physics, otherwise the entity state diverges from disk and Reset is broken.
- **One entity may carry multiple domain components.** A bioreactor entity
  could carry `MetabolicState` + `FlowField` + `ThermodynamicState` and three
  tick systems run on it in deterministic order via Bevy's `.after()`.
- **Skip degenerate entities early** — if `state.field_a == 0.0` and that
  means "passive component", `continue` rather than burn cycles. The
  electrochemistry tick filters `capacity_ah <= 0.0` for exactly this reason.

---

## 7. Step 5 — Sim-Value Bridge

`SIM_VALUES` is the thread-local hashmap that connects the simulation to the
rest of the engine: Rune scripts, watchpoints, the recorder, breakpoints, and
the EustressStream pipeline all read from it. There's also a `SimValuesResource`
(Bevy `Resource`) used by the recorder which runs on a non-main thread.

### 7.1 Publisher template

```rust
fn publish_<domain>_to_sim_values(
    query: Query<(&<Domain>State, Option<&ThermodynamicState>)>,
    mut sim_res: ResMut<crate::simulation::plugin::SimValuesResource>,
) {
    // For battery-style "one cell stack" domains, pick the entity with the
    // largest scalar param (capacity, mass, volume) — that's the primary.
    // For multi-instance domains (one curve per organism), publish keyed
    // names: "organism.<id>.<field>".
    let Some((state, thermo)) = query.iter()
        .max_by(|(a, _), (b, _)| a.field_a.partial_cmp(&b.field_a).unwrap_or(std::cmp::Ordering::Equal))
    else { return };

    let temp_c = thermo.map(|t| t.temperature - 273.15).unwrap_or(25.0);

    let values = [
        ("<domain>.field_a",      state.field_a as f64),
        ("<domain>.counter",      state.counter as f64),
        ("<domain>.temperature_c", temp_c as f64),
    ];

    crate::soul::rune_ecs_module::SIM_VALUES.with(|sv| {
        let mut sv = sv.borrow_mut();
        for (k, v) in &values { sv.insert(k.to_string(), *v); }
    });
    for (k, v) in &values { sim_res.0.insert(k.to_string(), *v); }
}
```

### 7.2 Reader template (script → ECS)

```rust
fn apply_sim_values_to_ecs(mut query: Query<&mut <Domain>State>) {
    let target = crate::soul::rune_ecs_module::SIM_VALUES.with(|sv| {
        sv.borrow().get("<domain>.target_field_a").copied()
    });
    let Some(target) = target else { return };
    let Some(mut state) = query.iter_mut().next() else { return };
    state.field_a = target as f32;
}
```

### 7.3 Naming rules

| Pattern | Meaning |
|---|---|
| `<domain>.<field>` | Scalar reading from the primary entity in this domain. |
| `<domain>.target_<field>` | Setpoint a script can write to drive the domain. |
| `<domain>.mode` | Discrete state (`0=idle, 1=charging, 2=discharging`). |
| `<domain>.<entity_name>.<field>` | Per-entity reading when there are many. |

Always **publish in SI** and let the UI/script handle display conversion. The
electrochemistry publisher's `temp_c` derivation is the only place °C is
allowed; everywhere else it's K.

---

## 8. Step 6 — Watchpoints

Watchpoints are how the timeline panel, recorder, and breakpoint system
discover what the domain produces. Register them on `OnEnter(PlayModeState::Playing)`.

### 8.1 Registration template

In `eustress/crates/engine/src/simulation/plugin.rs` (or extend it from your
domain plugin):

```rust
fn register_<domain>_watchpoints(
    mut watchpoints: ResMut<WatchPointRegistry>,
) {
    let entries = [
        ("<domain>.field_a",       "Field A",     "unit"),
        ("<domain>.counter",       "Tick count",  ""),
        ("<domain>.temperature_c", "Temperature", "°C"),
    ];

    for (name, label, unit) in &entries {
        if watchpoints.get(name).is_none() {
            watchpoints.register(WatchPoint::new(name, label, unit));
        }
    }
    info!("📊 Registered {} <domain> watchpoints", entries.len());
}
```

Wire it in the plugin:

```rust
.add_systems(OnEnter(PlayModeState::Playing), register_<domain>_watchpoints)
```

### 8.2 Rules

- **Idempotent registration** — the `if watchpoints.get(name).is_none()` guard
  matters. Play/Stop/Play must not duplicate the entries.
- **Human-readable labels and units** — the timeline panel renders them as-is.
  `"State of Charge"`, `"%"` reads better than `"battery.soc"`, `""`.
- **Match the publisher exactly** — a typo silently produces a watchpoint that
  never records. Cross-check the constants live in one place if you can.
- **Breakpoints are watchpoints with predicates** — once registered, scripts
  and the simulation-settings dialog can attach `value > 0.8` style
  breakpoints. No extra wiring.

---

## 9. Step 7 — Rune Scripting Surface

Authors drive the simulation from `.rune` scripts. Their entry point is
`get_sim_value` / `set_sim_value` plus optional domain-specific helpers
exposed through the `eustress::` module.

### 9.1 Minimum surface

`get_sim_value` and `set_sim_value` already work for any new domain — no code
changes needed. A domain-aware author can write:

```rune
use eustress::{get_sim_value, set_sim_value, log_info};

pub fn on_button_click(button_name) {
    match button_name {
        "Start" => {
            set_sim_value("<domain>.mode", 1.0);
            set_sim_value("<domain>.target_field_a", 100.0);
            log_info("<domain>: started");
        }
        "Stop"  => set_sim_value("<domain>.mode", 0.0),
        _ => {}
    }
}

pub fn on_update(_dt) {
    let v = get_sim_value("<domain>.field_a");
    // …drive HUD, decisions, etc.
}
```

### 9.2 When to add domain-specific functions

Add a `#[rune::function]` to `rune_ecs_module.rs` only when:

- The operation is **structural** (spawn an organism, add a feed pulse) and
  can't be expressed as a sim-value setpoint, **OR**
- The operation is **hot enough** that the f64-keyed hashmap lookup overhead
  is measurable in profiling (rare; usually only inside per-tick loops with
  thousands of entities).

For everything else, a sim-value key is the right interface.

---

## 10. Step 8 — Demo Scene

Every domain needs one runnable scene that proves the full stack: ECS load
from TOML, tick advances state, sim-values publish, watchpoints record, a
HUD reads them, buttons drive setpoints. This is also what the marketing PDF
points at.

### 10.1 Scene layout

```
assets/scene_templates/<Domain>Demo/
├── _service.toml          # Workspace folder
├── <domain>_subject/
│   ├── _instance.toml     # Part with [<domain>] section
│   └── meshes/...
├── HUD/
│   ├── _instance.toml     # ScreenGui
│   ├── ValueLabel.frame.toml
│   ├── StartButton.text_button.toml
│   └── StopButton.text_button.toml
└── scripts/
    └── <domain>_hud.rune  # on_update + on_button_click
```

### 10.2 Acceptance criteria

The scene **must** demonstrate, without manual intervention:

1. Press Play → scene loads, watchpoints register, tick runs.
2. HUD shows live values within one frame.
3. Start button changes the value visibly within a few seconds at default
   compression.
4. Stop button freezes it.
5. Press Stop (engine) → recording exports to
   `Universe/.eustress/knowledge/recordings/<Space>/sim_<timestamp>.json`
   with the new watchpoints in the report.

If any of those don't work, the domain isn't shipped.

---

## 11. Step 9 — Workshop Mode

A new domain deserves a Workshop mode so the AI co-engineer surfaces the
right tools.

In `eustress/crates/tools/src/modes.rs`:

```rust
pub enum WorkshopMode {
    General,
    // …existing variants…
    <Domain>,
}

impl WorkshopMode {
    pub const ALL: &'static [WorkshopMode] = &[
        // …existing…
        WorkshopMode::<Domain>,
    ];
    // … display_name, icon, system_prompt, tools …
}
```

The mode dropdown in `workshop_panel.slint` and the system-prompt fragment in
`modes/` complete the wiring. Adding a mode is mostly metadata; the main work
is writing a good system prompt that tells Claude what the domain's invariants
are.

---

## 12. Step 10 — Documentation & Tests

Every law module gets:

- **Unit tests** in the same file under `#[cfg(test)] mod tests`. Validate
  against textbook numbers, not just self-consistency. The
  `electrochemistry::nernst_potential` test against `E° + 25.7 mV × ln(Q)` at
  298 K is the model.
- **A short doc page** in `docs/development/<DOMAIN>_SYSTEM.md` linking to:
  the law module, the component, the TOML schema, the demo scene path, and a
  "first 5 minutes" tutorial.
- **An entry in [`FEATURE_PARITY.md`](../FEATURE_PARITY.md)** under the
  appropriate domain row.

---

## 13. Reference Implementation Pointers

When in doubt, copy these files and adapt:

| Layer | Reference file |
|---|---|
| Kernel laws | [`eustress/crates/common/src/realism/laws/electrochemistry.rs`](../../eustress/crates/common/src/realism/laws/electrochemistry.rs) |
| ECS component | [`eustress/crates/common/src/realism/particles/components.rs`](../../eustress/crates/common/src/realism/particles/components.rs) → `ElectrochemicalState` |
| TOML mirror | [`eustress/crates/engine/src/space/instance_loader.rs`](../../eustress/crates/engine/src/space/instance_loader.rs) → `TomlElectrochemicalState` |
| Tick + sim-value bridge | [`eustress/crates/engine/src/simulation/electrochemistry.rs`](../../eustress/crates/engine/src/simulation/electrochemistry.rs) |
| Watchpoint registration | [`eustress/crates/engine/src/simulation/plugin.rs`](../../eustress/crates/engine/src/simulation/plugin.rs) → `register_battery_watchpoints` |
| Rune sim-value bindings | [`eustress/crates/engine/src/soul/rune_ecs_module.rs`](../../eustress/crates/engine/src/soul/rune_ecs_module.rs) → `set_sim_value` / `get_sim_value` |
| Demo scene | `Universe1/Spaces/Space1/Workspace/V-Cell/V1/` |
| Demo script | `Universe1/Spaces/Space1/StarterGui/BatteryHUD/scripts/battery_hud.rune` |

---

## 14. Per-Domain Quick Specs

Concrete starting points for the alpha-pilot domains. Each gives the law
module name, the first 3–5 laws to ship, the canonical ECS state name, and one
scene idea.

### 14.1 Diffusion (life sciences)
- **Module:** `laws/diffusion.rs`
- **Laws:** Fick's first law, Fick's second law (1-D), Stokes-Einstein,
  permeability across a membrane.
- **State:** `ConcentrationField { species: HashMap<String, Vec<f32>>, dx: f32 }`
- **Scene:** Two-chamber diffusion cell with a permeable membrane.

### 14.2 Reaction Kinetics (life sciences)
- **Module:** `laws/reaction_kinetics.rs`
- **Laws:** Michaelis-Menten, Hill equation, Arrhenius rate, mass-action.
- **State:** `ReactionState { rates: Vec<f32>, concentrations: Vec<f32> }`
- **Scene:** Single-substrate enzyme assay reading absorbance over time.

### 14.3 Population Dynamics (biotech)
- **Module:** `laws/population_dynamics.rs`
- **Laws:** Logistic growth, Monod growth, Lotka-Volterra, Hill kill curve.
- **State:** `PopulationState { density: f32, viability: f32, generation: u32 }`
- **Scene:** Stirred-tank fed-batch fermentation with glucose feed and lactate
  buildup. Workshop mode: `Biotech`.

### 14.4 Fluid Dynamics (microfluidics, perfusion)
- **Module:** `laws/fluid_dynamics.rs`
- **Laws:** Hagen-Poiseuille, Reynolds, Womersley (pulsatile), capillary number.
- **State:** `FlowField { velocity: Vec3, pressure: f32, shear_stress: f32 }`
- **Scene:** Microfluidic Y-channel with two inlets and a single outlet.

### 14.5 Pharmacokinetics (med-device, drug delivery)
- **Module:** `laws/pharmacokinetics.rs`
- **Laws:** One-compartment model, two-compartment model, half-life, AUC,
  bioavailability.
- **State:** `PKState { compartments: Vec<f32>, ke: f32, ka: f32 }`
- **Scene:** Oral dose curve with absorption-phase ramp and elimination tail.

### 14.6 Gene Regulation (synthetic biology)
- **Module:** `laws/gene_regulation.rs`
- **Laws:** Hill repression/activation, mass-action transcription, Gillespie SSA.
- **State:** `GeneNetwork { species: Vec<f32>, reactions: Vec<Reaction> }`
- **Scene:** Toggle switch (mutual repressor pair) showing bistability under
  noise.

---

## 15. Integration Checklist

Use this as the PR description for any new domain:

- [ ] Law module under `realism/laws/<domain>.rs`, registered in `mod.rs` and `prelude`.
- [ ] All public law functions have doc-comments with paper references.
- [ ] Unit tests covering each law against textbook values.
- [ ] ECS component(s) under `realism/particles/components.rs` with `Component, Reflect, Clone, Debug, Serialize, Deserialize` and `#[reflect(Component)]`.
- [ ] `Toml<Domain>State` mirror struct in `instance_loader.rs` with `#[serde(default)]` on every field.
- [ ] `InstanceDefinition` carries `pub <domain>: Option<Toml<Domain>State>`.
- [ ] Auto-save raw-TOML patch path preserves the new section.
- [ ] Tick system in `simulation/<domain>.rs` plugin gated on `PlayModeState::Playing`.
- [ ] `apply_sim_values_to_ecs → <domain>_tick → publish_<domain>_to_sim_values` chain wired with `.after()`.
- [ ] `register_<domain>_watchpoints` runs on `OnEnter(PlayModeState::Playing)` and is idempotent.
- [ ] Sim-value keys follow the `<domain>.<field>` convention.
- [ ] Rune `on_button_click` / `on_update` example is included with the demo scene.
- [ ] Demo scene template in `assets/scene_templates/<Domain>Demo/`.
- [ ] Workshop mode added in `tools/src/modes.rs` with `display_name`, `icon`, and `system_prompt`.
- [ ] Mode appears in the dropdown defined in `main.slint` (`workshop-available-modes`).
- [ ] Domain doc page `docs/development/<DOMAIN>_SYSTEM.md` written.
- [ ] Row added to `docs/FEATURE_PARITY.md`.
- [ ] Acceptance criteria from §10.2 pass on a clean checkout.

When every box is checked, the domain is alpha-ready and the marketing claim
"real physics, not effects" continues to hold for it.

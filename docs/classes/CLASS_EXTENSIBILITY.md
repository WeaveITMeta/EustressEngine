# Class Extensibility — the canonical guide to adding classes

> **This is the single authoritative doc for "how do I add a class to
> Eustress."** The pre-2026 hardcoded-registry guides (six edits per
> class to `src/classes.rs` + `src/properties.rs` + `src/serialization/scene.rs`
> + three UI files) have been deleted — see the migration-history
> note at the bottom of this doc.
>
> **Related:** [`CLASS_CONVERSION.md`](../development/CLASS_CONVERSION.md)
> covers a separate concern — the *conversion tool* that lets users
> change an existing instance from one class to another in Studio
> (e.g. `Part` → `Seat`). Adding a new class is this doc; defining
> how that class converts to/from others is that one.

## What changed (one-paragraph migration summary)

The class registry is now driven by **`.defaults.toml` template files**
in `eustress/crates/common/assets/class_schema/`. `common/build.rs` globs
that directory at compile time and generates `BUILTIN_TEMPLATES`
automatically — adding a class no longer requires editing six source
files. For classes with their own TOML sections that need to become ECS
components, plugins register an [`ExtraSectionClaim`](../../eustress/crates/common/src/class_schema/mod.rs)
that the engine dispatches when a matching section is loaded.
Spawn-pipeline-only changes (Tier 2) still need a `ClassName` enum
variant; Tier 1 (defaults-only subclass) needs only the template file.

## Picking a tier

How to register new classes and class-owned TOML sections from a plugin
crate without editing `eustress-engine` or `eustress-common`. The
example throughout is a **Hydrodynamics** plugin that adds a
`FluidBody` class plus a `[hydrodynamics]` section that parts can opt
into.

## Three tiers of "add a class"

Pick the tier that matches your class's needs — most additions stop at
Tier 1 or Tier 2.

| Tier | What you touch | When to pick it |
|-----:|---------------|-----------------|
| **1** | ONE file — drop a `.defaults.toml` | Class is a Part subclass with no unique behaviour, just different defaults (e.g. `Trampoline` = Seat with different bounce). The base `Part` spawn pipeline handles everything. |
| **2** | Template + `ClassName` enum variant | Class needs a distinct enum variant so `file_loader` dispatches it correctly or other engine code can pattern-match on it. |
| **3** | Template + enum + `ExtraSectionClaim` plugin | Class has unique TOML sections that need to become ECS components, OR needs a custom spawn pipeline beyond Part/GUI/Script. |

## Walkthrough: Hydrodynamics plugin

Goal: a `FluidBody` class that carries a `[hydrodynamics]` section
declaring `density`, `viscosity`, and `surface_tension`. When a
`FluidBody` spawns, attach a `HydrodynamicsState` component the
simulation system queries each tick.

### 1. Drop the template

Create `eustress/crates/common/assets/class_schema/FluidBody.defaults.toml`:

```toml
# FluidBody — Volumetric fluid region with SPH-ready defaults.
# Hydrodynamics plugin: consumes [hydrodynamics] to spawn
# HydrodynamicsState components.

[metadata]
class_name = "FluidBody"
archivable = true

[transform]
position = [0.0, 0.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [4.0, 4.0, 4.0]

[properties]
color = [36, 120, 200]
transparency = 0.4
anchored = true
can_collide = false
cast_shadow = false
reflectance = 0.2
material = "Plastic"
locked = false

[hydrodynamics]
density = 1000.0          # kg/m³ (freshwater)
viscosity = 0.001002      # Pa·s at 20°C
surface_tension = 0.0728  # N/m at 20°C
```

**That's the only disk file you touch.** `common/build.rs` globs the
directory on the next `cargo check` and generates an updated
`BUILTIN_TEMPLATES` slice with `FluidBody` included automatically.
No edits to `templates.rs`.

### 2. Add the `ClassName` enum variant

Edit `eustress/crates/common/src/classes.rs`:

```rust
pub enum ClassName {
    // ... existing variants ...
    FluidBody,         // NEW — hydrodynamics plugin
}

impl ClassName {
    pub fn as_str(&self) -> &'static str {
        match self {
            // ... existing arms ...
            ClassName::FluidBody => "FluidBody",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            // ... existing arms ...
            "FluidBody" => Ok(ClassName::FluidBody),
            _ => Err(format!("Unknown class: {}", s)),
        }
    }
}
```

**Startup drift check** (`log_schema_validation` at
`common/src/class_schema/mod.rs`) will emit a warn-level log line if
you add a template without an enum variant — catch drift at boot, not
mid-play.

### 3. Register the `ExtraSectionClaim`

The `[hydrodynamics]` section isn't a field on `InstanceDefinition`
(it's plugin-specific), so it lands in `InstanceDefinition.extra`.
`spawn_instance` attaches a `PendingExtraSections` component carrying
that extras map. A system the common crate already registers —
`dispatch_pending_extras` — walks the map and calls every registered
claimant.

Your plugin registers a claimant in its `Plugin::build`:

```rust
// your-plugin/src/hydrodynamics.rs
use bevy::prelude::*;
use eustress_common::class_schema::{
    ClaimResult, ExtraSectionClaim, ExtraSectionRegistry,
};

#[derive(Component, Debug, Clone)]
pub struct HydrodynamicsState {
    pub density: f32,
    pub viscosity: f32,
    pub surface_tension: f32,
}

pub struct HydrodynamicsClaim;

impl ExtraSectionClaim for HydrodynamicsClaim {
    fn section_names(&self) -> &'static [&'static str] {
        &["hydrodynamics"]
    }

    fn claim(
        &self,
        _name: &str,
        value: &toml::Value,
        entity: Entity,
        commands: &mut Commands<'_, '_>,
    ) -> ClaimResult {
        let Some(table) = value.as_table() else {
            return ClaimResult::Invalid(
                "[hydrodynamics] is not a table".to_string(),
            );
        };
        let density = table
            .get("density")
            .and_then(|v| v.as_float())
            .unwrap_or(1000.0) as f32;
        let viscosity = table
            .get("viscosity")
            .and_then(|v| v.as_float())
            .unwrap_or(0.001) as f32;
        let surface_tension = table
            .get("surface_tension")
            .and_then(|v| v.as_float())
            .unwrap_or(0.0728) as f32;

        commands.entity(entity).insert(HydrodynamicsState {
            density,
            viscosity,
            surface_tension,
        });
        ClaimResult::Claimed
    }
}

pub struct HydrodynamicsPlugin;

impl Plugin for HydrodynamicsPlugin {
    fn build(&self, app: &mut App) {
        // Ensure the registry exists (engine already inserts it, this
        // is a safety net for headless builds or test harnesses).
        app.init_resource::<ExtraSectionRegistry>();
        app.world_mut()
            .resource_mut::<ExtraSectionRegistry>()
            .register(HydrodynamicsClaim);

        // … your simulation systems that query `HydrodynamicsState` …
    }
}
```

### 4. Mount the plugin

In the engine or client's `main.rs`:

```rust
app.add_plugins(your_plugin::HydrodynamicsPlugin);
```

That's it. Any `_instance.toml` with `[hydrodynamics]` now:

1. Parses through `load_and_heal_instance` (self-heals missing fields
   from `FluidBody.defaults.toml` if class_name matches).
2. The `[hydrodynamics]` section lands in `InstanceDefinition.extra`
   (since `InstanceDefinition` has no typed `hydrodynamics` field).
3. `spawn_instance` attaches `PendingExtraSections { sections: … }`.
4. `dispatch_pending_extras` calls `HydrodynamicsClaim::claim`, which
   inserts `HydrodynamicsState` onto the entity.
5. Your simulation systems query `With<HydrodynamicsState>` and step
   the physics as usual.

## Why this design

- **No engine edits for the plugin.** A third-party crate can ship a
  fully working class without ever touching `eustress-engine` or
  `eustress-common` source (except — optionally — adding its template
  to common's assets directory via a workspace patch or a contribution
  PR).
- **Unclaimed sections survive round-trips.** A TOML with a
  `[mycompany_proprietary]` section loads + saves unchanged even if
  no plugin claims it — `InstanceDefinition.extra` is a flatten
  HashMap, so writes preserve it on `toml::to_string_pretty`.
- **Multiple claimants can compose.** If a `[hydrodynamics]` section
  should spawn both a `HydrodynamicsState` AND a `FluidRenderer`
  component, either one claimant attaches both, or two claimants
  register for the same section name (first-match-wins under current
  `dispatch`, so order claims to produce a single source of truth).
- **Deterministic load:** extras dispatch runs on
  `Added<PendingExtraSections>` which fires exactly once per entity.

## Adding an `[attributes]`-style generic catch

A plugin can claim a section ONCE and use it for many classes. Example:
a `[telemetry]` section that any class can opt into to get metric
collection attached:

```rust
impl ExtraSectionClaim for TelemetryClaim {
    fn section_names(&self) -> &'static [&'static str] { &["telemetry"] }
    fn claim(&self, _, value, entity, commands) -> ClaimResult {
        // parse + insert `TelemetryTracker { … }`
        ClaimResult::Claimed
    }
}
```

Works for `Part`, `FluidBody`, any future class. No changes to those
classes' templates are needed beyond opting in via the section being
present.

## Testing checklist for a new class

1. `cargo check -p eustress-common` — verifies the template parses
   cleanly as TOML.
2. Boot the engine; check logs for `class_schema drift: template
   FluidBody.defaults.toml has no matching ClassName enum variant` —
   absence of that line means the enum+template are aligned.
3. Create a `Workspace/TestFluid/_instance.toml` with minimal content:
   ```toml
   [metadata]
   class_name = "FluidBody"
   ```
   Reload. Expect the engine to self-heal the file to the full
   template (every missing section filled in).
4. Query for your component: `Query<(Entity, &HydrodynamicsState)>`
   should return the freshly-spawned entity.
5. Edit the value in the Properties panel / TOML, save, reload —
   verify the new value round-trips through `HydrodynamicsClaim::claim`
   into the ECS component.

## Reference

- [`eustress_common::class_schema`](../../eustress/crates/common/src/class_schema/mod.rs) —
  registry, trait, dispatcher.
- [`eustress_common::classes::ClassName`](../../eustress/crates/common/src/classes.rs) —
  enum with `from_str`.
- [`eustress/crates/common/build.rs`](../../eustress/crates/common/build.rs) —
  filesystem scanner for `BUILTIN_TEMPLATES`.
- [`instance_loader::spawn_instance`](../../eustress/crates/engine/src/space/instance_loader.rs) —
  where `PendingExtraSections` gets attached.
- [`CLASS_CONVERSION.md`](../development/CLASS_CONVERSION.md) — class
  conversion tool semantics (orthogonal concern).
- [`eustress/crates/common/assets/class_schema/`](../../eustress/crates/common/assets/class_schema/) —
  every shipped `.defaults.toml` template lives here; this directory
  IS the class registry.

## Migration history

The 2026 `class_schema` migration replaced hardcoded class registration
(edit `src/classes.rs` + `src/properties.rs` + `src/serialization/scene.rs`
+ three UI files, six edits per class) with the template-driven system
documented above. The companion docs from that era — guides for the
old six-edit process, "core 10" / "extended 15" class lists, F9
legacy/new-system toggle, the `PartData` ↔ class compatibility layer —
were all deleted during the cleanup that produced the current
[`docs/classes/README.md`](README.md). The class registry itself is now
the source of truth: count
[`eustress/crates/common/assets/class_schema/*.defaults.toml`](../../eustress/crates/common/assets/class_schema/)
to see what ships today.

When in doubt, this doc + the `class_schema/` directory are the source
of truth. If you find guidance elsewhere that contradicts what's
written here, treat it as stale and prefer this doc.

//! # ClassSpawner trait registry
//!
//! Per-`ClassName` spawner contract, defined in
//! `docs/architecture/CLASS_REGISTRY.md` §2.
//!
//! ## Wave 2.2 scope (this crate)
//!
//! This crate ships the **scaffold only**: the trait, the registry
//! resource, the `RobloxInstance` stub, the [`PropertyBag`] container,
//! the [`SpawnCtx`] system-param bundle, and the LOD types.
//!
//! - **No spawners are registered.** The registry boots empty.
//! - **No engine systems use it yet.** Wave 2.3 ships the Bevy plugin
//!   (`engine/src/class_registry/plugin.rs`) and the LOOP-5 drain
//!   assertion that prevents resource-leak regressions.
//! - **No legacy path is touched.** `instance_loader`, `file_loader`,
//!   `gui_loader`, and `spawn.rs` keep their hardcoded match arms
//!   exactly as today; Wave 3 starts swapping classes over one PR at
//!   a time (per spec §7.3).
//!
//! When this scaffold compiles, the rest of the project compiles
//! unchanged, and Wave 2.3 has a stable trait shape to build against.
//!
//! ## Object safety — the load-bearing invariant
//!
//! `ClassSpawner` is **object-safe**: it can be stored as
//! `Box<dyn ClassSpawner>`. Object safety forbids:
//! - methods returning `Self`,
//! - methods with generic type parameters (`fn foo<T>(...)`),
//! - methods taking `Self` by value.
//!
//! Every trait method below respects those constraints — that's why
//! `serialize` returns `Vec<u8>` (not `<W: Write>`) and `deserialize`
//! takes `&[u8]` (not `<R: Read>`). The cost is one virtual call per
//! spawn (negligible — see spec §13 R10) in exchange for a registry
//! that's both a plain `HashMap` and friendly to third-party plugin
//! crates that ship their own classes. Sealing the trait via an enum
//! would close that door permanently.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::classes::ClassName;

pub mod lod;
pub mod property_bag;
pub mod spawn_ctx;

pub use lod::{ComponentBundle, DynamicComponent, LodTier};
pub use property_bag::PropertyBag;
pub use spawn_ctx::SpawnCtx;

// ============================================================================
// Roblox-instance adapter — stub trait alias
// ============================================================================

/// Property value yielded by [`RobloxInstance::property`].
///
/// **Wave 2 scope: stub.** Just enough variants for the trait method
/// signatures to compile. The real adapter (Wave 4 importer) replaces
/// this with rich `rbx_dom_weak::Variant` round-tripping. Spawners
/// written against the Wave 2 shape will need extension at that point —
/// that's fine, no Wave 3 spawner calls these accessors until the
/// importer ships.
#[derive(Debug, Clone)]
pub enum RobloxPropertyValue {
    Bool(bool),
    Float(f32),
    Int(i32),
    String(String),
    /// Type-erased catch-all so Wave 4 can extend without breaking the
    /// Wave 2 trait surface. Spawners that don't recognise the inner
    /// type return an empty `PropertyBag`.
    Other,
}

impl RobloxPropertyValue {
    /// Shortcut readers that mirror the spec's worked example
    /// (`p.as_f32()`, `p.as_bool()`). `None` on type mismatch.
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            RobloxPropertyValue::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            RobloxPropertyValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            RobloxPropertyValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            RobloxPropertyValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

/// Roblox-instance adapter trait.
///
/// **Wave 2 scope: stub.** Five methods, just enough for trait method
/// signatures to compile. The real importer (Wave 4) defines a richer
/// adapter that wraps `rbx_dom_weak::Instance` directly — see
/// `docs/architecture/ROBLOX_IMPORT_SPEC.md` for the planned shape.
///
/// Keeping this trait in `eustress-common` (rather than the engine or
/// the future importer crate) means the `ClassSpawner::import_from_roblox`
/// method can compile without pulling `rbx_dom_weak` into `eustress-common`'s
/// dependency tree — a Wave 1 design constraint.
pub trait RobloxInstance: Send + Sync {
    /// Roblox class name (`"Part"`, `"MeshPart"`, `"PointLight"`, …).
    /// The importer pipeline maps this to a `ClassName` before
    /// dispatching to a spawner.
    fn class_name(&self) -> &str;

    /// Roblox `Instance.Name`.
    fn name(&self) -> &str;

    /// Read a property by Roblox PascalCase key (`"Color"`,
    /// `"Brightness"`, …). Returns `None` when the property is absent
    /// from the source instance.
    fn property(&self, key: &str) -> Option<RobloxPropertyValue>;

    /// Children of this instance — for spawners (e.g. `Model`) that
    /// recursively spawn their subtree.
    fn children(&self) -> Vec<&dyn RobloxInstance>;

    /// Roblox referent (the 64-bit id used to resolve cross-references
    /// like `BillboardGui.Adornee`). Wave 4 importer feeds these into
    /// a referent → `Entity` map after the full tree is spawned.
    fn referent(&self) -> u64;
}

// ============================================================================
// ClassSpawner — the trait this whole module exists for
// ============================================================================

/// One spawner per [`ClassName`] variant. Owns the entire lifecycle of
/// one class: spawn (cold load, hot create), edit (in-place property
/// apply, possibly with respawn), serialize (Fjall rkyv archive),
/// deserialize (Fjall byte buffer → `PropertyBag`), import
/// (Roblox/TOML → `PropertyBag`), and export
/// (entity → `toml::Value`).
///
/// ## Object safety
///
/// Every method takes `&self` (no `Self` by value, no generic methods).
/// The registry stores `Box<dyn ClassSpawner>` and dispatches via vtable.
///
/// ## Send + Sync
///
/// Spawners live in a Bevy [`Resource`] ([`ClassRegistry`]) and must be
/// `Send + Sync + 'static`. State held by a spawner is therefore
/// immutable after registration; per-spawn mutability lives in
/// [`SpawnCtx`].
///
/// ## Determinism
///
/// `serialize` must be deterministic given the same world state — the
/// Fjall write path expects byte equality for change-detection. See
/// [`PropertyBag`]'s module docs for the iteration-order contract that
/// `serialize` and `export_to_toml` must honor.
pub trait ClassSpawner: Send + Sync + 'static {
    // ── Identity ───────────────────────────────────────────────────────

    /// Which class this spawner handles. The registry indexes by this
    /// value; one variant per spawner. Returning a value that doesn't
    /// match the registration key panics at registration time (see
    /// [`ClassRegistry::register`]).
    fn class_name(&self) -> ClassName;

    // ── Spawn (cold load / hot create) ────────────────────────────────

    /// Spawn an entity for this class. Called from `file_loader` cold
    /// load, from the Insert menu hot path, and from the Roblox
    /// importer after `import_from_roblox` has populated `props`.
    ///
    /// The spawner MUST attach `Instance` (with the correct
    /// `class_name`), `Name`, and an `InstanceFile` component when
    /// `props` carries a `source_path` (cold load) — these are the
    /// cross-cutting requirements every existing match arm enforces
    /// today.
    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity;

    // ── Persistence (Fjall rkyv) ──────────────────────────────────────

    /// Serialize this entity's class-relevant state to a tagged rkyv
    /// archive. Bytes are layout-stable for storage; the first byte is
    /// the schema tag (see spec Appendix A). Must include EVERY field
    /// the class round-trips through `_instance.toml` so a
    /// Fjall-authoritative world reload produces a byte-identical
    /// entity.
    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8>;

    /// Inverse of [`serialize`](Self::serialize): turn raw Fjall bytes
    /// into a `PropertyBag` the spawner can hand back to
    /// [`spawn`](Self::spawn). The first byte is the schema tag;
    /// reject mismatched tags with an empty bag so a migration-time
    /// mismatch becomes a logged warning rather than a corrupted
    /// entity.
    fn deserialize(&self, bytes: &[u8]) -> PropertyBag;

    // ── Live edits (Properties panel, scripts, MCP) ───────────────────

    /// Apply a property delta to an already-spawned entity in place.
    /// Returns `true` when the change requires a full respawn — e.g.
    /// `Part.shape` toggle from `Block` to `Sphere` needs a new
    /// mesh+collider; `Lighting.Technology` change from `Voxel` to
    /// `Future` needs the render graph rebuilt. Returns `false` for
    /// cheap mutations (color, transparency, brightness) that can be
    /// reflected by writing to existing components.
    ///
    /// The caller (Properties panel / script runtime) handles the
    /// respawn dance: it captures the entity's children + parent,
    /// despawns recursively, then re-invokes `spawn` and re-attaches
    /// the captured children. Keeping the respawn logic out of the
    /// spawner means a spawner is *only* the "build this entity"
    /// recipe.
    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool;

    // ── LOD ───────────────────────────────────────────────────────────

    /// The component bundle this class should carry at the given LOD
    /// tier. See [`LodTier`] for the four-tier model and
    /// `RENDER_CASCADE.md` for tier selection rules.
    ///
    /// Returning [`ComponentBundle::empty()`] is valid — it signals
    /// "no LOD-tier-specific work for this class" (Folder, Sound,
    /// scripts).
    fn lod_components(&self, tier: LodTier) -> ComponentBundle;

    // ── Roblox & TOML import ──────────────────────────────────────────

    /// Convert a Roblox instance (via the [`RobloxInstance`] adapter)
    /// to the `PropertyBag` the Eustress spawner consumes.
    ///
    /// Spawners that don't have a Roblox cognate (`SoulScript`,
    /// `ChunkedWorld`, asset classes) return an empty bag.
    fn import_from_roblox(&self, rbx_instance: &dyn RobloxInstance) -> PropertyBag;

    /// Convert a raw `toml::Value` (typically a `[section]` body for
    /// folder-form `_instance.toml`, or the whole table for flat
    /// `*.<class>.toml` files) to a `PropertyBag`.
    ///
    /// This replaces the dozens of bespoke `serde::Deserialize` impls
    /// scattered across `instance_loader::InstanceDefinition`,
    /// `gui_loader::GuiTomlFile`, `service_loader::ServiceDefinition`,
    /// etc. Each spawner owns its own schema, mirroring the
    /// `ClassSchemaRegistry` template structure.
    ///
    /// Key normalisation (snake_case canonicalization) happens BEFORE
    /// this is called via `class_schema::normalise_keys`, so spawners
    /// always read snake_case keys.
    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag;

    // ── TOML export (for save round-trip & class conversion) ──────────

    /// Inverse of [`import_from_toml`](Self::import_from_toml): read
    /// the entity's components and emit a `toml::Value::Table` matching
    /// the on-disk schema for this class. Used by
    /// `write_instance_changes_system` to persist live edits and by
    /// `class_conversion::ConversionOperation` to preserve sections
    /// during class swaps.
    ///
    /// Determinism: keys MUST be emitted in canonical (template) order
    /// so two equivalent entities produce byte-identical TOML.
    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value;
}

// ============================================================================
// ClassRegistry — Bevy Resource holding one spawner per ClassName
// ============================================================================

/// Bevy [`Resource`] holding one [`ClassSpawner`] per [`ClassName`]
/// variant.
///
/// Inserted at App build time by `ClassRegistryPlugin` (Wave 2.3) and
/// queried by `file_loader`, `instance_loader`, `gui_loader`, the
/// Roblox importer, the worlddb read/write path, and the Properties
/// panel.
///
/// **Wave 2.2 scope:** the resource compiles and a Wave 2.3 plugin will
/// initialise it. No spawners are registered at this point in the
/// project — `get(...)` returns `None` for every class, and callers
/// fall back to the legacy hardcoded path (per spec §7.3).
#[derive(Resource)]
pub struct ClassRegistry {
    spawners: HashMap<ClassName, Box<dyn ClassSpawner>>,
}

impl Default for ClassRegistry {
    fn default() -> Self {
        // 80 is the variant count in `ClassName` today (see spec §8.17);
        // pre-sizing the map avoids the early growth allocations the
        // Wave 2.3 plugin would otherwise trigger when it registers
        // every built-in spawner.
        Self {
            spawners: HashMap::with_capacity(80),
        }
    }
}

impl ClassRegistry {
    /// Register a spawner.
    ///
    /// Panics if a spawner for the same class is already registered
    /// (drift-bug guard — silent overwrite has been the
    /// `class_name() mismatched its registration key` failure mode in
    /// every other plugin system the project has tried).
    pub fn register<S: ClassSpawner>(&mut self, spawner: S) {
        let class = spawner.class_name();
        if self.spawners.contains_key(&class) {
            panic!(
                "ClassRegistry: spawner for {} already registered \
                 (likely double-plugin-add or class_name() returning the wrong variant)",
                class.as_str()
            );
        }
        self.spawners.insert(class, Box::new(spawner));
    }

    /// Returns the spawner for this class, or `None` if no spawner has
    /// been registered.
    ///
    /// `None` triggers the hardcoded fallback during the migration
    /// window (spec §7.3 — Wave 5 removes the fallback once every
    /// `ClassName` has a spawner).
    pub fn get(&self, class: ClassName) -> Option<&dyn ClassSpawner> {
        self.spawners.get(&class).map(|b| &**b)
    }

    /// True when a spawner exists for `class`. Cheaper than `get` when
    /// the caller only needs the existence check (e.g. the LOOP-5
    /// drain-validator in Wave 2.3 walks every variant).
    pub fn contains(&self, class: ClassName) -> bool {
        self.spawners.contains_key(&class)
    }

    /// Iterate every registered class for diagnostics + validation.
    /// Used by Wave 2.3's `log_registry_validation` startup system to
    /// warn about `ClassName` variants that have no spawner.
    pub fn registered_classes(&self) -> impl Iterator<Item = ClassName> + '_ {
        self.spawners.keys().copied()
    }

    /// Number of registered spawners.
    pub fn len(&self) -> usize {
        self.spawners.len()
    }

    /// True when no spawners are registered. The state of the resource
    /// immediately after `Default::default()` and before any Wave 2.3
    /// plugin has run.
    pub fn is_empty(&self) -> bool {
        self.spawners.is_empty()
    }
}

// ============================================================================
// RegisterClassExt — Bevy-idiomatic plugin registration
// ============================================================================

/// Extension trait so plugins use Bevy-idiomatic registration:
///
/// ```ignore
/// app.register_class::<PointLightSpawner>();
/// ```
///
/// rather than reaching into the resource directly. Mirrors the
/// pattern `ClassSchemaRegistry` does NOT have today (one of the small
/// wins of doing this trait redesign).
pub trait RegisterClassExt {
    /// Register a spawner that implements `Default`. The default
    /// instance is what the registry stores.
    fn register_class<S: ClassSpawner + Default>(&mut self) -> &mut Self;

    /// Register a spawner instance whose construction needs explicit
    /// configuration (e.g. a spawner that wraps a handle to a runtime
    /// asset table — rare, but the pattern keeps the door open).
    fn register_class_with<S: ClassSpawner>(&mut self, spawner: S) -> &mut Self;
}

impl RegisterClassExt for App {
    fn register_class<S: ClassSpawner + Default>(&mut self) -> &mut Self {
        self.world_mut()
            .resource_mut::<ClassRegistry>()
            .register(S::default());
        self
    }

    fn register_class_with<S: ClassSpawner>(&mut self, spawner: S) -> &mut Self {
        self.world_mut()
            .resource_mut::<ClassRegistry>()
            .register(spawner);
        self
    }
}

// ============================================================================
// Tests — proof of object safety + empty-default invariants
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// **Deliverable criterion #3 from the task spec.**
    ///
    /// If this compiles, `ClassSpawner` is object-safe and the registry
    /// can hold trait objects. The body never executes; the *type
    /// check* is the test.
    #[allow(dead_code)]
    fn assert_object_safe() {
        fn takes_dyn(_: Box<dyn ClassSpawner>) {}
        fn takes_dyn_ref(_: &dyn ClassSpawner) {}
        let _ = takes_dyn;
        let _ = takes_dyn_ref;
    }

    /// **Deliverable criterion #4 from the task spec.**
    ///
    /// The registry type compiles and stores boxed trait objects.
    #[allow(dead_code)]
    fn assert_registry_holds_boxed() {
        let _registry: HashMap<ClassName, Box<dyn ClassSpawner>> = HashMap::new();
    }

    /// **Deliverable criterion #5 from the task spec.**
    ///
    /// The default registry is empty — Wave 2.3's plugin is the thing
    /// that registers spawners; this crate never does.
    #[test]
    fn default_registry_is_empty() {
        let registry = ClassRegistry::default();
        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
        assert!(registry.get(ClassName::Part).is_none());
        assert!(!registry.contains(ClassName::Part));
        assert_eq!(registry.registered_classes().count(), 0);
    }
}

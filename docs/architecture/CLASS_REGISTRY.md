# ClassSpawner Trait Registry — Architectural Specification

**Status:** Wave 1 SPEC (no code) — 2026-05-26
**Owner:** Engine Core
**Cross-refs:** `CLASS_LIGHTING_AUDIT.md` · `ROBLOX_IMPORT_SPEC.md` · `RENDER_CASCADE.md` · `IDENTITY.md` · `FEATURE_PARITY.md` · `CLASS_CONVERSION.md`

> **Executive Summary** — Today, spawning a Eustress class entity routes through three monolithic `if/else if matches!(class_name, …)` ladders (`instance_loader::spawn_instance`, `file_loader::spawn_directory_entry`, `gui_loader::spawn_gui_element`) plus ~40 hand-rolled `spawn_<class>` helpers in `spawn.rs`. Adding a class costs 4–6 simultaneous edits in three crates and silently fails if any are missed (the file_loader dispatches unknown classes as `Folder`). This spec defines a single `ClassSpawner` trait with one `Box<dyn ClassSpawner>` registered per `ClassName` variant. Each spawner owns spawn, edit-apply, rkyv serialize/deserialize, LOD bundle selection, Roblox/TOML import, and TOML export for its class. The hardcoded match arms become the fallback path during incremental migration, gated by a `class-registry` cargo feature so we can flip individual classes over class-by-class with zero ABI risk. The trait is intentionally object-safe (no generic methods) so the registry can be a plain `HashMap<ClassName, Box<dyn ClassSpawner>>`, addressing the only architectural choice we don't get to take back later. Wave 3 implements all 80+ spawner modules listed in §7 against this trait.

---

## Table of Contents

1. [Problem Statement — What Wave 1 Is Replacing](#1-problem-statement)
2. [Trait Definition](#2-trait-definition)
3. [SpawnCtx — System-Param Bundle](#3-spawnctx)
4. [PropertyBag — Roundtrip Container](#4-propertybag)
5. [Registry Data Structure & Bevy Plug-in Point](#5-registry)
6. [Plugin Registration Pattern](#6-plugin-pattern)
7. [Migration Strategy — Incremental Cutover Behind a Feature Flag](#7-migration)
8. [Spawner Module Checklist — Every ClassName Variant](#8-spawner-checklist)
9. [LOD Bundles — Hero / Active / Streamed / Horizon](#9-lod-bundles)
10. [Roblox Import / TOML Import / TOML Export](#10-import-export)
11. [Cross-Document References](#11-cross-refs)
12. [Open Questions for Human Decision](#12-open-questions)
13. [Risks & Mitigations](#13-risks)
14. [Appendix A — Wire-Format Tag Bytes](#appendix-a)
15. [Appendix B — Worked Example: PointLight Migration](#appendix-b)

---

## 1. Problem Statement

### 1.1 Current State (audited against `2026-05-26 HEAD`)

Class spawning today lives in **four** uncoordinated places:

| File | Lines | Pattern |
|------|------:|---------|
| `eustress/crates/engine/src/space/instance_loader.rs` | ~2,664 | `spawn_instance` mega-function (lines 1413–1909): branches on `(asset_present?, primitive_mesh?, custom_mesh?)`, hardcoded for **Part / SoulScript / GUI containers / non-visual classes**. UI component dispatch via `attach_ui_component` (~line 1913) `match class_name { TextLabel => …, TextButton => …, … }`. |
| `eustress/crates/engine/src/space/file_loader.rs` | ~2,563 | `spawn_directory_entry` (line 934+): `if is_screen_gui … else if is_gui_container … else if BillboardGui … else if Image | Video … else if SoulScript … else if Part … else if (text/image leaves) … else { Folder fallback }`. ~12 hardcoded branches spanning lines 1066–1695. Unknown `ClassName` silently degrades to a `Folder`. |
| `eustress/crates/engine/src/space/gui_loader.rs` | ~1,103 | `spawn_gui_element` (line 574): `match gui_type { "ScreenGui" => spawn_screen_gui_element, "TextLabel" => spawn_text_label_element, … }`. Nine hardcoded delegations. |
| `eustress/crates/engine/src/spawn.rs` | ~1,580 | **40 free functions** named `spawn_part_glb`, `spawn_part`, `spawn_model`, `spawn_folder`, `spawn_humanoid`, `spawn_camera`, `spawn_point_light`, `spawn_spot_light`, `spawn_surface_light`, `spawn_directional_light`, `spawn_sound`, `spawn_attachment`, `spawn_weld_constraint`, `spawn_motor6d`, `spawn_particle_emitter`, `spawn_beam`, `spawn_sky`, `spawn_atmosphere`, `spawn_terrain`, `spawn_screen_gui`, `spawn_billboard_gui`, `spawn_surface_gui`, `spawn_frame`, `spawn_scrolling_frame`, `spawn_text_label`, `spawn_text_label_ui`, `spawn_image_label`, `spawn_text_button`, `spawn_image_button`, `spawn_text_box`, `spawn_viewport_frame`, `spawn_video_frame`, `spawn_document_frame`, `spawn_web_frame`, `spawn_special_mesh`, `spawn_decal`, `spawn_decal_at`, `spawn_union`, `spawn_animator`, `spawn_keyframe_sequence`. Each takes a class-specific signature; **callers must already know which to dispatch**, which is precisely the lookup the trait registry is built to centralize. |

The conversion layer (`eustress/crates/engine/src/class_conversion.rs`) further partitions every `ClassName` into a `ConversionCategory` (`Geometry / Container / GuiContainer / GuiLeaf / Light / Constraint / NonConvertible`) — the same set of buckets a per-class trait would express directly via its `class_name()` + `lod_components()` shape.

### 1.2 Symptoms of the Status Quo

1. **Adding a class = 4–7 simultaneous edits in unrelated files.** A new `BindableEvent` class needs: variant in `ClassName` enum, arm in `ClassName::from_str`, arm in `ClassName::as_str`, branch in `file_loader::spawn_directory_entry`, branch in `instance_loader::spawn_instance` (or `attach_ui_component`), free function in `spawn.rs`, extension in `class_conversion::get_extension`. Forget one and the class spawns as a `Folder` with no error.
2. **No serialization symmetry.** Every loader writes back via its own bespoke path (`write_instance_changes_system` does it raw for parts; gui_loader has its own; lighting refuses to write at all per the comment at `instance_loader.rs:2180`). Fjall persistence (`worlddb::rkyv_values`) is wired separately via `ArchTransform` + `EusValue` and is not class-aware at all.
3. **LOD has no per-class hook.** Every part gets the same `VisibilityRange` from `part_visibility_range()` (`instance_loader.rs:1372`) regardless of whether it's a hero character, a streamed prop, or a horizon billboard. Lights, GUIs, particles get no LOD treatment at all.
4. **Roblox importer has no entry point.** `FEATURE_PARITY.md §20` lists RBXL reader, RBXM reader, RBXLX reader, etc. as unimplemented. There is no contract for "convert one Roblox instance into Eustress components" — every class would need its own bespoke importer.
5. **Edit application is implicit.** Live property edits today flow through `material_sync` for visual properties or the Properties panel's per-class writers; there is no contract that says "given this PropertyBag delta, mutate this entity (possibly respawning if the change requires a new mesh/collider)."

### 1.3 What the Trait Replaces

```
BEFORE (today):                       AFTER (Wave 3 target):
─────────────────                     ──────────────────────
file_loader::spawn_directory_entry    file_loader::spawn_directory_entry
  ├── 12 hardcoded branches    ──►      └── registry.get(class_name).unwrap().spawn(ctx, props)
  └── Folder fallback                   └── (Folder path stays for unregistered classes)

instance_loader::spawn_instance       instance_loader::spawn_instance
  ├── Part-mega-spawn          ──►      └── registry.get(ClassName::Part).unwrap().spawn(ctx, props)
  └── attach_ui_component (8 arms)

gui_loader::spawn_gui_element         (deleted; rolled into spawner trait)
  └── 9 hardcoded delegations  ──►      registry.get(class_name).spawn(ctx, props)

spawn.rs (40 free fns)                spawn.rs (deleted or thin re-exports)
  └── Caller must know which   ──►      registry.get(class_name).spawn(ctx, props)
```

---

## 2. Trait Definition

### 2.1 Exact Rust Signature

```rust
// File (Wave 2): eustress/crates/common/src/class_registry/mod.rs

use bevy::ecs::{entity::Entity, system::Commands, world::World};
use bevy::asset::AssetServer;
use crate::classes::ClassName;
use crate::class_registry::property_bag::PropertyBag;
use crate::class_registry::spawn_ctx::SpawnCtx;
use crate::class_registry::lod::{LodTier, ComponentBundle};

/// One spawner per `ClassName` variant. Owns the entire lifecycle of one
/// class: spawn (cold load, hot create), edit (in-place property apply,
/// possibly with respawn), serialize (Fjall rkyv archive), deserialize
/// (Fjall byte buffer → PropertyBag), import (Roblox/TOML → PropertyBag),
/// export (entity → toml::Value), and LOD bundle selection per tier.
///
/// ## Object safety
///
/// Every method takes `&self` (no `Self` by value, no generic methods).
/// The registry stores `Box<dyn ClassSpawner>` and dispatches via vtable.
/// This is non-negotiable: a generic spawner would force the registry to
/// be parameterized by class, defeating the runtime dispatch the
/// `from_str` → enum path delivers.
///
/// ## Send + Sync
///
/// Spawners live in a Bevy `Resource` (`ClassRegistry`) and must be
/// `Send + Sync + 'static`. State held by a spawner is therefore
/// immutable after registration; per-spawn mutability lives in `SpawnCtx`.
///
/// ## Determinism
///
/// `serialize` must be deterministic given the same world state — the
/// Fjall write path expects byte equality for change-detection. See
/// `PropertyBag::canonical_order` for the deterministic-iteration
/// contract that `serialize` and `export_to_toml` must honor.
pub trait ClassSpawner: Send + Sync + 'static {
    // ── Identity ───────────────────────────────────────────────────────

    /// Which class this spawner handles. The registry indexes by this
    /// value; one variant per spawner. Returning the wrong value will
    /// panic at registration time (see `ClassRegistry::register`).
    fn class_name(&self) -> ClassName;

    // ── Spawn (cold load / hot create) ────────────────────────────────

    /// Spawn an entity for this class. Called from `file_loader` cold
    /// load, from the Insert menu hot path, and from the Roblox importer
    /// after `import_from_roblox` has populated `props`.
    ///
    /// `props` is the deserialized PropertyBag — for cold load this came
    /// from TOML or Fjall; for hot create it's the class template's
    /// defaults (`ClassSchemaRegistry::template`); for import it's the
    /// output of `import_from_roblox` / `import_from_toml`.
    ///
    /// The spawner attaches whatever components the class needs and
    /// returns the spawned entity. It MUST attach `Instance` (with the
    /// correct `class_name`), `Name`, and an `InstanceFile` if `props`
    /// carries a `toml_path` (cold load) — these are the cross-cutting
    /// requirements every existing match arm enforces today.
    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity;

    // ── Persistence (Fjall rkyv) ──────────────────────────────────────

    /// Serialize this entity's class-relevant state to a tagged rkyv
    /// archive. Bytes are layout-stable for storage; the first byte is
    /// the schema tag (see Appendix A). Must include EVERY field the
    /// class round-trips through `_instance.toml` so a Fjall-authoritative
    /// world reload produces a byte-identical entity.
    ///
    /// Implementations build a class-specific rkyv mirror struct (see
    /// `eustress-worlddb::rkyv_values::ArchTransform` for the pattern)
    /// and call `rkyv::to_bytes` after prepending the tag byte.
    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8>;

    /// Inverse of `serialize`: turn raw Fjall bytes into a PropertyBag
    /// the spawner can hand back to `spawn`. The first byte is the
    /// schema tag; reject mismatched tags with an empty bag so a
    /// migration-time mismatch becomes a logged warning rather than
    /// a corrupted entity. (Tag bumps move in lockstep with
    /// `worlddb::header::WorldSchemaVersion` per Appendix A.)
    fn deserialize(&self, bytes: &[u8]) -> PropertyBag;

    // ── Live edits (Properties panel, scripts, MCP) ───────────────────

    /// Apply a property delta to an already-spawned entity in place.
    /// Returns `true` when the change requires a full respawn — e.g.
    /// `Part.shape` toggle from Block to Sphere needs a new mesh+collider,
    /// `Lighting.Technology` change from `Voxel` to `Future` needs the
    /// render graph rebuilt. Returns `false` for cheap mutations (color,
    /// transparency, brightness) that can be reflected by writing to
    /// existing components.
    ///
    /// The caller (Properties panel / script runtime) handles the
    /// respawn dance: it captures the entity's children + parent, calls
    /// `world.entity_mut(entity).despawn_recursive()`, then re-invokes
    /// `spawn(ctx, &updated_props)` and re-attaches the captured
    /// children. Keeping the respawn logic out of the spawner means a
    /// spawner is *only* the "build this entity" recipe — the orchestration
    /// belongs in one place.
    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool;

    // ── LOD ───────────────────────────────────────────────────────────

    /// The component bundle this class should carry at the given LOD
    /// tier. See `LodTier` for the four-tier model and `RENDER_CASCADE.md`
    /// for tier selection rules.
    ///
    /// `Hero` parts get full PBR + colliders + shadow casting; `Active`
    /// drops shadow casting and shrinks visibility range; `Streamed`
    /// adds `VisibilityRange` distance cull only; `Horizon` collapses
    /// to a single billboarded impostor (or nothing for entities that
    /// have no horizon representation — Sound, Script, Folder).
    ///
    /// Implementations return a `ComponentBundle` (see §9) describing
    /// which components to add and which to remove relative to the
    /// previous tier.
    fn lod_components(&self, tier: LodTier) -> ComponentBundle;

    // ── Roblox & TOML import ──────────────────────────────────────────

    /// Convert an `rbx_dom_weak::types::Instance` (or any structurally
    /// equivalent intermediate; see `ROBLOX_IMPORT_SPEC.md`) to the
    /// PropertyBag the Eustress spawner consumes.
    ///
    /// The importer pipeline is `RBXL bytes → rbx_dom_weak tree → walk
    /// each instance → look up registered spawner by mapped ClassName
    /// → call import_from_roblox → call spawn`. Spawners that don't
    /// have a Roblox cognate (SoulScript, ChunkedWorld, ImageAsset,
    /// Material asset class) return an empty bag and the importer
    /// emits a warn-level log line.
    ///
    /// IMPORTANT: this method takes `&dyn RobloxInstance` (a trait
    /// alias defined in `ROBLOX_IMPORT_SPEC.md`) rather than the
    /// concrete `rbx_dom_weak::Instance` to avoid pulling the rbx_dom
    /// crate into `eustress-common` — Wave 1 keeps the importer in
    /// `eustress-engine`. The trait alias re-exports the property
    /// accessor methods needed.
    fn import_from_roblox(&self, rbx_instance: &dyn RobloxInstance) -> PropertyBag;

    /// Convert a raw `toml::Value` (typically a `[section]` body for
    /// folder-form `_instance.toml`, or the whole table for flat
    /// `*.<class>.toml` files) to a PropertyBag.
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

    /// Inverse of `import_from_toml`: read the entity's components and
    /// emit a `toml::Value::Table` matching the on-disk schema for this
    /// class. Used by `write_instance_changes_system` to persist live
    /// edits and by `class_conversion::ConversionOperation` to preserve
    /// sections during class swaps.
    ///
    /// Determinism: keys MUST be emitted in `PropertyBag::canonical_order`
    /// so two equivalent entities produce byte-identical TOML. See §4.3.
    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value;
}
```

### 2.2 Method Cardinality Summary

| Method | Mutability | Returns | Called from |
|---|---|---|---|
| `class_name` | `&self` | `ClassName` | Registry registration, validation |
| `spawn` | `&self` | `Entity` | Cold load, hot create, import-then-spawn |
| `serialize` | `&self` | `Vec<u8>` | `worlddb` write path |
| `deserialize` | `&self` | `PropertyBag` | `worlddb` read path |
| `apply_edit` | `&self` (on world: `&mut World`) | `bool` (true ⇒ respawn) | Properties panel, scripts, MCP |
| `lod_components` | `&self` | `ComponentBundle` | Per-frame LOD transition system |
| `import_from_roblox` | `&self` | `PropertyBag` | `eustress-importer-roblox` crate (Wave 4) |
| `import_from_toml` | `&self` | `PropertyBag` | `file_loader`, `gui_loader`, `service_loader` |
| `export_to_toml` | `&self` | `toml::Value` | `write_instance_changes_system`, `ConversionOperation::execute` |

### 2.3 Why No Generic Methods

The trait is **object-safe**: it can be stored as `Box<dyn ClassSpawner>`. Object safety forbids:
- Methods that return `Self`
- Methods with generic type parameters (`fn foo<T>(…)`)
- Methods that take `Self` by value

This rules out e.g. `fn deserialize<R: Read>(reader: R) -> PropertyBag`. We pass `&[u8]` instead. It rules out `fn spawn<B: Bundle>(…)` — we use the runtime `ComponentBundle` from §9 instead. The cost is a virtual call per spawn (negligible) and a small heap allocation for `Box<dyn>` per registered spawner (~80 spawners total — also negligible).

The alternative (a sealed enum dispatch) was rejected because it would force every spawner into the `eustress-common` crate, preventing third-party plugin spawners from existing at all. The trait + plugin pattern (§6) keeps the engine open to extension.

---

## 3. SpawnCtx

`SpawnCtx` is the system-param bundle a spawner needs to actually build an entity. It mirrors what today's `spawn_instance` collects via its 6-parameter signature (`&mut Commands, &AssetServer, &mut Assets<StandardMaterial>, &mut MaterialRegistry, &mut PrimitiveMeshCache, PathBuf, InstanceDefinition`) plus what the GUI spawners need (`&mut Assets<Mesh>`, `&mut Assets<Image>`, `ForwardDecalMaterial` assets).

### 3.1 Struct Definition

```rust
// File (Wave 2): eustress/crates/common/src/class_registry/spawn_ctx.rs
// Engine-side wrapper at: eustress/crates/engine/src/class_registry/spawn_ctx_engine.rs
// (the engine wrapper exposes Bevy-asset-server-bound state; the common
// trait sees a struct with only `Send + Sync` references.)

pub struct SpawnCtx<'w, 's> {
    // ── Always-required ──
    pub commands: &'w mut Commands<'w, 's>,
    pub asset_server: &'w AssetServer,
    pub class_schema: &'w ClassSchemaResource,  // for default backfill
    pub source_path: Option<PathBuf>,           // _instance.toml path, if cold-loading from disk

    // ── Asset stores (mutable, common across many spawners) ──
    pub meshes: &'w mut Assets<Mesh>,
    pub standard_materials: &'w mut Assets<StandardMaterial>,
    pub images: &'w mut Assets<Image>,
    pub decal_materials: &'w mut Assets<ForwardDecalMaterial<StandardMaterial>>,

    // ── Eustress-specific registries ──
    pub material_registry: &'w mut super::material_loader::MaterialRegistry,
    pub mesh_cache: &'w mut super::instance_loader::PrimitiveMeshCache,

    // ── Hierarchy hints (set by file_loader) ──
    pub parent_entity: Option<Entity>,          // parent to attach to
    pub measure_unit: eustress_common::units::MeasureUnit,
    pub load_in_progress: bool,                 // suppresses write-back during cold load
}
```

### 3.2 Construction

`SpawnCtx` is built fresh per spawn call. It is **not** a Bevy `Resource` — passing it as a resource would force all asset mutations through a single mutable borrow per frame. Instead, the `file_loader` already holds the right `ResMut<…>` borrows; it constructs a `SpawnCtx` from them per spawn site.

Construction pattern (engine-side helper, Wave 3):

```rust
fn build_spawn_ctx<'w, 's>(
    commands: &'w mut Commands<'w, 's>,
    asset_server: &'w Res<AssetServer>,
    class_schema: &'w Res<ClassSchemaResource>,
    meshes: &'w mut ResMut<Assets<Mesh>>,
    materials: &'w mut ResMut<Assets<StandardMaterial>>,
    images: &'w mut ResMut<Assets<Image>>,
    decal_materials: &'w mut ResMut<Assets<ForwardDecalMaterial<StandardMaterial>>>,
    material_registry: &'w mut ResMut<MaterialRegistry>,
    mesh_cache: &'w mut ResMut<PrimitiveMeshCache>,
    source_path: Option<PathBuf>,
    parent: Option<Entity>,
    unit: MeasureUnit,
    load_in_progress: bool,
) -> SpawnCtx<'w, 's> { … }
```

### 3.3 Why Not a System Param Trait

Bevy's `SystemParam` derive would let us write `fn spawn(ctx: SpawnCtx, props: &PropertyBag)` and have Bevy build the param automatically. That path is rejected for two reasons:

1. **Object safety**: trait methods that take `impl SystemParam` aren't object-safe — same reason §2.3 rules out generics.
2. **The asset borrows are heterogeneous per spawner**: a `Sound` spawner needs `Assets<AudioSource>`; a `Decal` spawner needs `Assets<ForwardDecalMaterial>`; a `Folder` spawner needs nothing. A union-of-everything `SpawnCtx` overlocks the borrow checker. The compromise here — one big struct with all of them — is acceptable because spawn calls are coarse (per-entity), not hot-loop per-frame.

---

## 4. PropertyBag

`PropertyBag` is the typed container that crosses the trait API boundary. It wraps `Vec<(String, PropertyValue)>` (not `HashMap<String, PropertyValue>`) because **insertion order is the canonical roundtrip order** — see §4.3.

### 4.1 PropertyValue (Already Exists)

The canonical `PropertyValue` enum is in `eustress/crates/common/src/classes.rs:7315`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyValue {
    String(String),
    Float(f32),
    Int(i32),
    Bool(bool),
    Vector2([f32; 2]),
    Vector3(Vec3),
    UDim2(crate::ui_types::UDim2),
    Color(Color),
    Color3([f32; 3]),
    Transform(Transform),
    Material(Material),
    Enum(String),
}
```

This is sufficient for every existing class's properties — verified by scanning the field types of every struct in §1's enumeration. A handful of additions are expected during Wave 3:

| Add | Reason | Where used |
|---|---|---|
| `EntityRef(Option<u32>)` | Roblox-style instance reference (e.g. `BillboardGui.adornee` is a name today; should become a typed handle) | BillboardGui, SurfaceGui, WeldConstraint, Motor6D |
| `NumberSequence(Vec<(f32, f32)>)` | ParticleEmitter color/size/alpha sequences | ParticleEmitter, Beam, Trail |
| `ColorSequence(Vec<(f32, [f32; 3])>)` | Same shape for colors | ParticleEmitter, Beam |
| `AssetPath(String)` | Strongly-typed asset reference (today these masquerade as `String`) | Sound, Decal, Image, Video, all texture-bearing classes |
| `Quaternion([f32; 4])` | Roblox `CFrame.rotation` round-trip without going through Transform | Bone, Attachment |

These extensions are additive (new enum variants) and gated by the rkyv tag in Appendix A.

### 4.2 PropertyBag Wrapper

```rust
// File (Wave 2): eustress/crates/common/src/class_registry/property_bag.rs

use crate::classes::PropertyValue;

/// Typed property container with deterministic iteration.
///
/// Backed by `Vec<(String, PropertyValue)>` (NOT HashMap) because:
///
/// 1. Iteration order must be deterministic for Fjall byte-equality
///    change detection (`rkyv::to_bytes` outputs differ if input field
///    order differs) and for TOML round-trip stability (Roblox importers
///    expect the same key order as Studio writes today).
/// 2. Insertion order is the canonical "schema order" — when a spawner
///    inserts (anchored, then can_collide, then color, then material),
///    that's the order export_to_toml emits and the order
///    `class_schema::merge_template_into` produces. Keeping the same
///    order across import → spawn → serialize → export round-trips
///    eliminates a class of "the TOML reshuffled overnight" diffs in
///    git-controlled spaces.
/// 3. `Vec<(K, V)>` is rkyv-archivable directly (already in use as
///    `worlddb::rkyv_values::EusValue::Table`); HashMap is not without
///    a custom sort step.
///
/// `O(n)` get is fine: spawners read each property once at spawn time
/// (~50 props per class * 1 spawn = trivial); the lookup is amortized
/// against the hashmap-vs-vec constant-factor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PropertyBag {
    entries: Vec<(String, PropertyValue)>,
}

impl PropertyBag {
    pub fn new() -> Self { Self::default() }

    pub fn with_capacity(n: usize) -> Self {
        Self { entries: Vec::with_capacity(n) }
    }

    /// Insert a property. If the key already exists, the value is
    /// REPLACED in place (order preserved). If the key is new, it's
    /// APPENDED (preserving insertion order — the canonical order).
    pub fn set(&mut self, key: impl Into<String>, value: PropertyValue) {
        let key = key.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| k == &key) {
            slot.1 = value;
        } else {
            self.entries.push((key, value));
        }
    }

    pub fn get(&self, key: &str) -> Option<&PropertyValue> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, PropertyValue)> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    /// True when this bag has the canonical key ordering. A spawner
    /// uses this in debug builds to assert it built the bag in the
    /// same order export_to_toml will emit — a regression test
    /// against TOML diff churn.
    #[cfg(debug_assertions)]
    pub fn canonical_order(&self, schema: &ClassTemplate) -> bool { … }

    /// Typed accessor helpers — returns None on type mismatch.
    pub fn get_f32(&self, key: &str) -> Option<f32> { … }
    pub fn get_vec3(&self, key: &str) -> Option<Vec3> { … }
    pub fn get_color(&self, key: &str) -> Option<Color> { … }
    pub fn get_string(&self, key: &str) -> Option<&str> { … }
    pub fn get_transform(&self, key: &str) -> Option<&Transform> { … }
    pub fn get_bool(&self, key: &str) -> Option<bool> { … }
    pub fn get_enum(&self, key: &str) -> Option<&str> { … }
}

/// Conversion to/from EusValue for Fjall persistence — these are the
/// inverses spawners use inside their `serialize` / `deserialize` impls.
impl From<&PropertyBag> for eustress_worlddb::rkyv_values::EusValue { … }
impl From<eustress_worlddb::rkyv_values::EusValue> for PropertyBag { … }

/// TOML round-trip helpers — these wrap the existing
/// `class_schema::normalise_keys` snake_case path.
impl PropertyBag {
    pub fn from_toml_table(table: &toml::value::Table) -> Self { … }
    pub fn to_toml_table(&self) -> toml::value::Table { … }
}
```

### 4.3 Canonical Key Order

The order rule is: **a spawner's `import_from_toml` and `import_from_roblox` MUST insert keys in the same order as the class template** (`assets/class_schema/<Class>/_instance.toml`). The template is the authoritative source.

For a Part this means:
```
metadata.class_name, metadata.archivable, metadata.name,
transform.position, transform.rotation, transform.scale,
asset.mesh, asset.scene,
properties.material, properties.color, properties.transparency,
properties.reflectance, properties.anchored, properties.can_collide,
properties.locked, properties.cast_shadow,
attributes.*, tags.*, extra.*
```

`PropertyBag::canonical_order(&template)` returns `true` when the bag's iteration order matches the template's flattened key order. A spawner unit test asserts this in debug builds.

When the trait registry rolls out, `ClassSchemaRegistry` is extended with a `canonical_keys(class_name: ClassName) -> &'static [&'static str]` method that returns the flat ordered keys — derivable from the build-time template parse (`common/build.rs`).

---

## 5. Registry

### 5.1 Data Structure

```rust
// File (Wave 2): eustress/crates/common/src/class_registry/mod.rs

use bevy::prelude::Resource;
use std::collections::HashMap;
use crate::classes::ClassName;

/// Bevy Resource holding one `ClassSpawner` per `ClassName` variant.
/// Inserted at App build time; queried by file_loader, instance_loader,
/// gui_loader, the Roblox importer, the world DB read/write path, and
/// the Properties panel.
#[derive(Resource)]
pub struct ClassRegistry {
    spawners: HashMap<ClassName, Box<dyn ClassSpawner>>,
}

impl Default for ClassRegistry {
    fn default() -> Self {
        Self { spawners: HashMap::with_capacity(80) }
    }
}

impl ClassRegistry {
    /// Register a spawner. Panics if a spawner for the same class is
    /// already registered (drift-bug guard — silent overwrite was the
    /// `class_name() mismatched its registration key` failure mode in
    /// every other plugin system the project's tried).
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

    /// Returns the spawner for this class, or None if no spawner has
    /// been registered. `None` triggers the hardcoded fallback during
    /// the migration window (see §7).
    pub fn get(&self, class: ClassName) -> Option<&dyn ClassSpawner> {
        self.spawners.get(&class).map(|b| &**b)
    }

    /// Iterate every registered class for diagnostics + validation.
    pub fn registered_classes(&self) -> impl Iterator<Item = ClassName> + '_ {
        self.spawners.keys().copied()
    }

    pub fn len(&self) -> usize { self.spawners.len() }
}
```

### 5.2 Bevy Plug-in Wiring

```rust
// File (Wave 2): eustress/crates/engine/src/class_registry/plugin.rs

use bevy::prelude::*;
use eustress_common::class_registry::ClassRegistry;

/// Plugin that initializes the registry and registers every built-in
/// spawner. Third-party plugins add their own spawners via
/// `app.register_class::<MySpawner>()` after this plugin runs.
pub struct ClassRegistryPlugin;

impl Plugin for ClassRegistryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ClassRegistry>()
            .add_systems(Startup, log_registry_validation);

        // Built-in spawners — one per ClassName variant, registered in
        // alphabetic order to match the table in §8.
        #[cfg(feature = "class-registry")]
        {
            app.register_class::<spawners::adornments::ArcHandlesSpawner>()
               .register_class::<spawners::geometry::AttachmentSpawner>()
               // … 80+ more, see §8 …
               .register_class::<spawners::ui::ViewportFrameSpawner>();
        }
    }
}

/// Startup-time consistency check. Warns when a `ClassName` variant
/// has no registered spawner — mirrors `class_schema::log_schema_validation`
/// exactly. Catches the "added a variant, forgot to register a spawner"
/// regression at boot rather than at spawn time.
fn log_registry_validation(registry: Res<ClassRegistry>) {
    let registered: std::collections::HashSet<_> =
        registry.registered_classes().collect();
    let all = [/* every ClassName variant — see §8 */];
    for class in all {
        if !registered.contains(&class) {
            warn!(
                "class_registry: ClassName::{:?} has no spawner — file_loader will fall back to hardcoded match arm or Folder",
                class
            );
        }
    }
    info!("class_registry: {} spawners registered", registry.len());
}
```

---

## 6. Plugin Pattern

### 6.1 Extension Trait

```rust
// File (Wave 2): eustress/crates/common/src/class_registry/mod.rs

/// Extension trait so plugins use Bevy-idiomatic registration:
///
///   app.register_class::<PointLightSpawner>();
///
/// rather than reaching into the resource directly. Mirrors the
/// pattern `ClassSchemaRegistry` does NOT have (one of the small wins
/// of doing this trait redesign).
pub trait RegisterClassExt {
    fn register_class<S: ClassSpawner + Default>(&mut self) -> &mut Self;
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
```

### 6.2 Third-Party Plugin Example

A simulation plugin (e.g. `eustress-plasma`) registers its own classes the same way the engine does its built-ins:

```rust
// File: eustress/plugins/plasma/src/lib.rs

use bevy::prelude::*;
use eustress_common::class_registry::RegisterClassExt;

pub struct PlasmaPlugin;

impl Plugin for PlasmaPlugin {
    fn build(&self, app: &mut App) {
        // The plasma plugin adds a `PlasmaField` ClassName variant via
        // the same path third-party crates use — adding a variant to
        // `ClassName` is still a common-crate edit (see §12 Q3 for the
        // open question on dynamic class names), but the spawner itself
        // is plugin-owned.
        app.register_class::<plasma_spawners::PlasmaFieldSpawner>();
    }
}
```

### 6.3 Ordering Guarantees

Spawner registration order is irrelevant — registration is keyed by `ClassName`, not insertion order, and the registry rejects double-registration. The `ClassRegistryPlugin` MUST run before any plugin that depends on a registered spawner; plugin authors enforce this via the standard Bevy `plugin.before(ClassRegistryPlugin)` API or by adding `ClassRegistryPlugin` to their own plugin group.

---

## 7. Migration Strategy

### 7.1 Goals

1. **Zero ABI breakage during the cutover.** A user with an existing space must be able to upgrade engine versions across the migration window without their world breaking — `_instance.toml` and Fjall persistence stay identical bytes.
2. **One class at a time.** A bug in (say) the new `PointLightSpawner` should not affect `Part` spawning.
3. **Reversible.** Every step is behind a feature flag — `cargo build --no-default-features --features core` reverts to the old monolithic path.
4. **No silent fallback to `Folder`.** The migration window is the time to surface every class the existing match arms silently dropped.

### 7.2 Cargo Feature

Add to `eustress/crates/engine/Cargo.toml`:

```toml
[features]
# When ON: route spawn calls through ClassRegistry. When a class has no
# registered spawner, fall back to the legacy hardcoded match arm + emit a
# warn-level log line. When OFF: skip the registry lookup entirely and use
# the existing match arms. Enabled in `core` once Wave 3 lands.
class-registry = []

# Add to the `core` feature list AFTER Wave 3 ships ALL spawners and the
# legacy match arms are deleted (Wave 5).
core = [..., "class-registry"]
```

### 7.3 Phased Rollout

```
Wave 1 (NOW): SPEC — this doc + cross-refs. No code changes.

Wave 2: TRAIT — Implement the trait, registry, plugin pattern, PropertyBag,
        SpawnCtx. NO spawner impls; legacy paths untouched. `class-registry`
        feature exists but does nothing yet. Lands as a single PR.

Wave 3: SPAWNER IMPLS — One PR per logical group (lights, GUI containers,
        GUI leaves, constraints, etc.). Each PR:
          1. Implements the spawner struct under `eustress/crates/engine/
             src/spawners/<group>/<class>.rs`.
          2. Wires it into `ClassRegistryPlugin` behind `cfg(feature = "class-registry")`.
          3. Adds a roundtrip test: cold-load a fixture, serialize, deserialize,
             assert byte equality. Tests live in `eustress/crates/engine/tests/
             class_registry/<class>.rs`.
          4. Inserts a SHIM in the legacy match arm:
                if let Some(spawner) = registry.and_then(|r| r.get(class_name)) {
                    return spawner.spawn(&mut ctx, &props);
                }
                /* fall through to legacy match arm */
        After each PR, both paths produce identical entities.

Wave 4: ROBLOX IMPORTER — Build the `eustress-importer-roblox` crate using
        the `import_from_roblox` trait method. Each spawner ships an impl
        of this method during Wave 3; Wave 4 is just the walk + dispatch.
        See ROBLOX_IMPORT_SPEC.md for the full pipeline.

Wave 5: DELETE LEGACY — Once ALL classes have spawners and Wave 3 tests
        prove byte-equivalence, delete the match arms. `class-registry`
        moves from opt-in to default. Files dropped:
          - eustress/crates/engine/src/spawn.rs (40 functions)
          - eustress/crates/engine/src/space/gui_loader.rs::spawn_gui_element
          - `attach_ui_component` and its match arms in instance_loader.rs
          - `spawn_directory_entry`'s 12 hardcoded branches → single lookup
        Wave 5 is one mechanical PR per file deleted.
```

### 7.4 Cutover Order

Classes migrate in dependency order — leaf classes first, container/composite classes last:

```
Order 1 (pure leaf, no parent dependencies, smallest blast radius):
  - Sound, Decal, Attachment, KeyframeSequence, Animator
  - PointLight, SpotLight, SurfaceLight, DirectionalLight
  - Atmosphere, Sky, Clouds, Star, Moon
  - All 18 adornment classes (BoxHandleAdornment, etc.)

Order 2 (constraints, depend on Attachment):
  - WeldConstraint, Motor6D
  - HingeConstraint, BallSocketConstraint, SpringConstraint,
    RopeConstraint, PrismaticConstraint, DistanceConstraint

Order 3 (parts and seats — touch BasePart + mesh assets):
  - Part, SpecialMesh, UnionOperation
  - Seat, VehicleSeat, SpawnLocation

Order 4 (GUI leaves):
  - TextLabel, TextButton, TextBox, ImageLabel, ImageButton

Order 5 (GUI containers):
  - Frame, ScrollingFrame, ScreenGui, BillboardGui, SurfaceGui
  - VideoFrame, DocumentFrame, WebFrame, ViewportFrame

Order 6 (containers):
  - Folder, Model

Order 7 (services + environment):
  - Workspace, Lighting, Terrain, ChunkedWorld
  - Team, SolarSystem, CelestialBody, RegionChunk

Order 8 (special / non-Roblox):
  - SoulScript, LuauScript, LuauLocalScript, LuauModuleScript
  - RemoteEvent, RemoteFunction, BindableEvent, BindableFunction
  - WorkshopConversation
  - Document, ImageAsset, VideoAsset, Material (asset class), Image, Video
```

### 7.5 Roundtrip Test Template

Every spawner PR ships with at least one test:

```rust
// eustress/crates/engine/tests/class_registry/point_light.rs

#[test]
fn point_light_serialize_deserialize_roundtrip() {
    let mut app = test_app_with_registry();
    let world = app.world_mut();

    // Build expected props
    let mut props = PropertyBag::new();
    props.set("color", PropertyValue::Color3([1.0, 0.8, 0.6]));
    props.set("brightness", PropertyValue::Float(1500.0));
    props.set("range", PropertyValue::Float(20.0));
    props.set("shadows", PropertyValue::Bool(true));

    // Spawn via registry
    let mut ctx = build_spawn_ctx(...);
    let registry = world.resource::<ClassRegistry>();
    let spawner = registry.get(ClassName::PointLight).unwrap();
    let entity = spawner.spawn(&mut ctx, &props);

    // Serialize → deserialize → must match
    let bytes = spawner.serialize(world, entity);
    let restored = spawner.deserialize(&bytes);
    for (k, v) in props.iter() {
        assert_eq!(restored.get(k), Some(v),
            "round-trip mismatch for key {}", k);
    }
}

#[test]
fn point_light_legacy_parity() {
    // Spawn the same PropertyBag via BOTH the legacy spawn_point_light
    // and the new spawner; assert resulting entities are component-equal.
    // See `test_helpers::assert_entities_equal` for the comparison logic
    // (compares Instance, Name, Transform, PointLight, and every Eustress
    // component, ignoring Bevy-internal generation counters).
    ...
}
```

### 7.6 Rollback Plan

If Wave 3 surfaces a class whose spawner can't be made byte-equivalent in time:

1. The legacy match arm is still present in the source (Wave 5 hasn't run yet).
2. Remove that class from the `register_class::<…>()` chain in `ClassRegistryPlugin`.
3. The runtime falls back to the match arm via the §7.3 SHIM.
4. File a follow-up issue; ship the rest of Wave 3 without that class.

This is the same rollback story as `units_v1` and `world-db` (both already shipping under the same feature-flag-with-fallback pattern, per the Cargo.toml comments at lines 253–325).

---

## 8. Spawner Module Checklist

This is the Wave 3 implementation checklist. Every `ClassName` variant currently in `eustress_common::classes::ClassName` (verified against `classes.rs:207–317`) gets exactly one spawner.

Naming convention: `spawners::<group>::<ClassName>Spawner` at path `eustress/crates/engine/src/spawners/<group>/<snake_case_class>.rs`.

### 8.1 Geometry & Parts

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `Part` | `spawners::geometry::PartSpawner` | `instance_loader.rs:1413` mega-spawn |
| `BasePart` | (abstract — no spawner; Part covers it) | — |
| `PVInstance` | (abstract — no spawner) | — |
| `Instance` | (abstract — no spawner) | — |
| `Model` | `spawners::geometry::ModelSpawner` | `spawn.rs:295` |
| `Folder` | `spawners::container::FolderSpawner` | `spawn.rs:313` |
| `SpecialMesh` | `spawners::geometry::SpecialMeshSpawner` | `spawn.rs:1406` |
| `UnionOperation` | `spawners::geometry::UnionOperationSpawner` | `spawn.rs:1512` |
| `Decal` | `spawners::geometry::DecalSpawner` | `spawn.rs:1430` |
| `Humanoid` | `spawners::geometry::HumanoidSpawner` | `spawn.rs:352` |
| `Seat` | `spawners::geometry::SeatSpawner` | — (currently a Part shim) |
| `VehicleSeat` | `spawners::geometry::VehicleSeatSpawner` | — |
| `SpawnLocation` | `spawners::geometry::SpawnLocationSpawner` | — |

### 8.2 Lights

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `PointLight` | `spawners::lights::PointLightSpawner` | `spawn.rs:409` |
| `SpotLight` | `spawners::lights::SpotLightSpawner` | `spawn.rs:437` |
| `SurfaceLight` | `spawners::lights::SurfaceLightSpawner` | `spawn.rs:463` |
| `DirectionalLight` | `spawners::lights::DirectionalLightSpawner` | `spawn.rs:489` |

### 8.3 Constraints & Attachments

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `Attachment` | `spawners::constraints::AttachmentSpawner` | `spawn.rs:540` |
| `WeldConstraint` | `spawners::constraints::WeldConstraintSpawner` | `spawn.rs:557` |
| `Motor6D` | `spawners::constraints::Motor6DSpawner` | `spawn.rs:573` |
| `HingeConstraint` | `spawners::constraints::HingeConstraintSpawner` | (stub) |
| `DistanceConstraint` | `spawners::constraints::DistanceConstraintSpawner` | (stub) |
| `PrismaticConstraint` | `spawners::constraints::PrismaticConstraintSpawner` | (stub) |
| `BallSocketConstraint` | `spawners::constraints::BallSocketConstraintSpawner` | (stub) |
| `SpringConstraint` | `spawners::constraints::SpringConstraintSpawner` | (stub) |
| `RopeConstraint` | `spawners::constraints::RopeConstraintSpawner` | (stub) |

### 8.4 VFX

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `ParticleEmitter` | `spawners::vfx::ParticleEmitterSpawner` | `spawn.rs:593` |
| `Beam` | `spawners::vfx::BeamSpawner` | `spawn.rs:610` |

### 8.5 GUI Containers

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `ScreenGui` | `spawners::gui::ScreenGuiSpawner` | `spawn.rs:684` + `gui_loader.rs:722` |
| `BillboardGui` | `spawners::gui::BillboardGuiSpawner` | `spawn.rs:708` + `file_loader.rs:1160` |
| `SurfaceGui` | `spawners::gui::SurfaceGuiSpawner` | `spawn.rs:756` |
| `Frame` | `spawners::gui::FrameSpawner` | `spawn.rs:779` + `gui_loader.rs:862` |
| `ScrollingFrame` | `spawners::gui::ScrollingFrameSpawner` | `spawn.rs:827` + `gui_loader.rs:906` |

### 8.6 GUI Leaves

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `TextLabel` | `spawners::gui::TextLabelSpawner` | `spawn.rs:870` + `gui_loader.rs:941` |
| `TextButton` | `spawners::gui::TextButtonSpawner` | `spawn.rs:1072` |
| `TextBox` | `spawners::gui::TextBoxSpawner` | `spawn.rs:1183` |
| `ImageLabel` | `spawners::gui::ImageLabelSpawner` | `spawn.rs:1029` |
| `ImageButton` | `spawners::gui::ImageButtonSpawner` | `spawn.rs:1130` |
| `ViewportFrame` | `spawners::gui::ViewportFrameSpawner` | `spawn.rs:1244` |
| `VideoFrame` | `spawners::gui::VideoFrameSpawner` | `spawn.rs:1279` |
| `DocumentFrame` | `spawners::gui::DocumentFrameSpawner` | `spawn.rs:1319` |
| `WebFrame` | `spawners::gui::WebFrameSpawner` | `spawn.rs:1359` |

### 8.7 Camera & Audio

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `Camera` | `spawners::camera::CameraSpawner` | `spawn.rs:373` |
| `Sound` | `spawners::audio::SoundSpawner` | `spawn.rs:518` |

### 8.8 Animation

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `Animator` | `spawners::animation::AnimatorSpawner` | `spawn.rs:1550` |
| `KeyframeSequence` | `spawners::animation::KeyframeSequenceSpawner` | `spawn.rs:1567` |

### 8.9 Environment & Lighting Service

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `Sky` | `spawners::environment::SkySpawner` | `spawn.rs:630` |
| `Atmosphere` | `spawners::environment::AtmosphereSpawner` | `spawn.rs:646` |
| `Lighting` | `spawners::environment::LightingServiceSpawner` | `service_loader.rs` |
| `Workspace` | `spawners::environment::WorkspaceSpawner` | `service_loader.rs` |
| `Clouds` | `spawners::environment::CloudsSpawner` | (stub) |
| `Star` | `spawners::environment::StarSpawner` | (stub, currently spawned as non-visual instance) |
| `Moon` | `spawners::environment::MoonSpawner` | (stub) |

### 8.10 Terrain & Large-Scale World

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `Terrain` | `spawners::world::TerrainSpawner` | `spawn.rs:663` |
| `ChunkedWorld` | `spawners::world::ChunkedWorldSpawner` | (stub) |

### 8.11 Networking

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `RemoteEvent` | `spawners::networking::RemoteEventSpawner` | (stub) |
| `RemoteFunction` | `spawners::networking::RemoteFunctionSpawner` | (stub) |
| `BindableEvent` | `spawners::networking::BindableEventSpawner` | (stub) |
| `BindableFunction` | `spawners::networking::BindableFunctionSpawner` | (stub) |

### 8.12 Scripting

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `SoulScript` | `spawners::scripting::SoulScriptSpawner` | `file_loader.rs:1471` + `file_loader.rs:713` |
| `LuauScript` | `spawners::scripting::LuauScriptSpawner` | `file_loader.rs:753` |
| `LuauLocalScript` | `spawners::scripting::LuauLocalScriptSpawner` | (uses LuauScript path) |
| `LuauModuleScript` | `spawners::scripting::LuauModuleScriptSpawner` | (uses LuauScript path) |
| `WorkshopConversation` | `spawners::scripting::WorkshopConversationSpawner` | (stub) |

### 8.13 Teams & Players

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `Team` | `spawners::players::TeamSpawner` | (stub) |

### 8.14 Adornments (Editor Meta-Entities)

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `BoxHandleAdornment` | `spawners::adornments::BoxHandleAdornmentSpawner` | (gizmo code) |
| `SphereHandleAdornment` | `spawners::adornments::SphereHandleAdornmentSpawner` | (gizmo code) |
| `ConeHandleAdornment` | `spawners::adornments::ConeHandleAdornmentSpawner` | (gizmo code) |
| `CylinderHandleAdornment` | `spawners::adornments::CylinderHandleAdornmentSpawner` | (gizmo code) |
| `LineHandleAdornment` | `spawners::adornments::LineHandleAdornmentSpawner` | (gizmo code) |
| `PyramidHandleAdornment` | `spawners::adornments::PyramidHandleAdornmentSpawner` | (gizmo code) |
| `WireframeHandleAdornment` | `spawners::adornments::WireframeHandleAdornmentSpawner` | (gizmo code) |
| `ImageHandleAdornment` | `spawners::adornments::ImageHandleAdornmentSpawner` | (gizmo code) |
| `SelectionBox` | `spawners::adornments::SelectionBoxSpawner` | (selection sync) |
| `SelectionSphere` | `spawners::adornments::SelectionSphereSpawner` | (stub) |
| `SurfaceSelection` | `spawners::adornments::SurfaceSelectionSpawner` | (stub) |
| `ArcHandles` | `spawners::adornments::ArcHandlesSpawner` | (gizmo code) |
| `Handles` | `spawners::adornments::HandlesSpawner` | (gizmo code) |
| `PathfindingLink` | `spawners::adornments::PathfindingLinkSpawner` | (stub) |
| `PathfindingModifier` | `spawners::adornments::PathfindingModifierSpawner` | (stub) |
| `GridSensor` | `spawners::adornments::GridSensorSpawner` | (smart-grid code) |
| `AlignmentGuide` | `spawners::adornments::AlignmentGuideSpawner` | (smart-grid code) |
| `SnapIndicator` | `spawners::adornments::SnapIndicatorSpawner` | (smart-grid code) |

### 8.15 Orbital / Geospatial

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `SolarSystem` | `spawners::orbital::SolarSystemSpawner` | (stub) |
| `CelestialBody` | `spawners::orbital::CelestialBodySpawner` | (stub) |
| `RegionChunk` | `spawners::orbital::RegionChunkSpawner` | (stub) |

### 8.16 Asset Classes

| `ClassName` | Spawner Module | Source today |
|---|---|---|
| `Material` (asset) | `spawners::assets::MaterialAssetSpawner` | `material_loader.rs::spawn_material_entity` |
| `Image` (asset) | `spawners::assets::ImageSpawner` | `file_loader.rs:1302` |
| `Video` (asset) | `spawners::assets::VideoSpawner` | `file_loader.rs:1302` |
| `Document` | `spawners::assets::DocumentSpawner` | (stub) |
| `ImageAsset` | `spawners::assets::ImageAssetSpawner` | (stub) |
| `VideoAsset` | `spawners::assets::VideoAssetSpawner` | (stub) |

### 8.17 Total

**80 spawners across 16 groups.** That matches the 80 `ClassName` variants in `classes.rs:207–317`, minus the four abstract base classes (`Instance`, `PVInstance`, `BasePart`, `Folder` — wait, `Folder` is concrete; corrected count: 80 concrete spawners). Abstract classes are intentionally **not registered**; any TOML that names them as `class_name` resolves to the concrete fallback (Part for BasePart, Folder for Instance/PVInstance) via the existing `ClassName::from_str` aliases.

---

## 9. LOD Bundles

### 9.1 Tier Definitions

```rust
// File (Wave 2): eustress/crates/common/src/class_registry/lod.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LodTier {
    /// In-frustum, focal point of attention. Full PBR, full shadows,
    /// full collider, full UI rendering. Currently the only tier the
    /// engine has — every part gets this.
    Hero,

    /// Visible at mid-range. Shadow casting off (NotShadowCaster), full
    /// material, full collider. Visibility range matches Workspace
    /// render_distance.
    Active,

    /// Streamed but not actively rendered every frame. VisibilityRange
    /// distance cull active; collider may be downgraded to AABB only;
    /// material may swap to an unlit variant.
    Streamed,

    /// Beyond active range; represented by a single billboarded impostor
    /// or omitted entirely. Sound is muted; particles paused; scripts
    /// suspended via RunService.PreRender check.
    Horizon,
}

/// What a spawner's `lod_components` returns. Empty bundles are valid —
/// they signal "no LOD-tier-specific components for this class" (e.g.
/// Folder has no LOD model at all).
pub struct ComponentBundle {
    /// Components to insert when this tier is entered.
    pub insert: Vec<DynamicComponent>,
    /// Component type IDs to remove when this tier is entered.
    pub remove: Vec<bevy::reflect::TypeId>,
}

/// Type-erased "spawn this component". Boxed because the bundle must
/// be returned through the object-safe trait API. The transition
/// system unwraps with `commands.entity(e).insert_reflect(boxed_component)`
/// at apply time.
pub struct DynamicComponent(pub Box<dyn bevy::reflect::Reflect>);
```

### 9.2 Example: PointLight LOD

```rust
impl ClassSpawner for PointLightSpawner {
    fn lod_components(&self, tier: LodTier) -> ComponentBundle {
        match tier {
            LodTier::Hero | LodTier::Active => ComponentBundle {
                insert: vec![],            // PointLight stays as spawned
                remove: vec![TypeId::of::<NoShadow>()],
            },
            LodTier::Streamed => ComponentBundle {
                insert: vec![DynamicComponent(Box::new(NoShadow))],
                remove: vec![],
            },
            LodTier::Horizon => ComponentBundle {
                insert: vec![DynamicComponent(Box::new(Visibility::Hidden))],
                remove: vec![TypeId::of::<PointLight>()],
            },
        }
    }
}
```

### 9.3 Tier Transitions

The actual tier-selection system (which entity is at which tier) is out of scope for this spec — see `RENDER_CASCADE.md` for the rules. This spec only defines the **trait method that exposes per-class behavior** to the cascade.

A skeleton system shape (lives in `eustress-engine`, Wave 3):

```rust
fn apply_lod_transitions(
    mut commands: Commands,
    registry: Res<ClassRegistry>,
    changed: Query<(Entity, &Instance, &CurrentLodTier), Changed<CurrentLodTier>>,
) {
    for (entity, instance, tier) in &changed {
        let Some(spawner) = registry.get(instance.class_name) else { continue };
        let bundle = spawner.lod_components(tier.0);
        let mut ec = commands.entity(entity);
        for component in bundle.insert {
            ec.insert_reflect(component.0);
        }
        for type_id in bundle.remove {
            ec.remove_by_id(type_id);
        }
    }
}
```

---

## 10. Roblox Import / TOML Import / TOML Export

### 10.1 Roblox Import Path

```
*.rbxl/*.rbxm/*.rbxlx file
  │
  ▼  (eustress-importer-roblox, Wave 4)
rbx_dom_weak::WeakDom (parsed instance tree)
  │
  ▼  (walk + map ClassName)
for each rbx::Instance:
  ├── Eustress ClassName from rbx_class_to_eustress_class(rbx.class.as_str())
  │     └── e.g. "Part" → Part, "MeshPart" → Part, "Folder" → Folder,
  │         "Script" → LuauScript, "ModuleScript" → LuauModuleScript,
  │         …see ROBLOX_IMPORT_SPEC.md §3 for the full mapping
  ├── spawner = registry.get(class_name)
  ├── props = spawner.import_from_roblox(&RobloxInstanceAdapter(rbx))
  └── entity = spawner.spawn(&mut ctx, &props)
```

The `RobloxInstance` trait alias (defined in `ROBLOX_IMPORT_SPEC.md`):

```rust
pub trait RobloxInstance: Send + Sync {
    fn class_name(&self) -> &str;
    fn name(&self) -> &str;
    fn property(&self, key: &str) -> Option<RobloxPropertyValue>;
    fn children(&self) -> Vec<&dyn RobloxInstance>;
    fn referent(&self) -> u64;       // for resolving cross-references
}
```

This abstraction keeps `rbx_dom_weak` out of `eustress-common`; spawners only see the trait. A test mock (`MockRobloxInstance`) lives in `eustress-common::class_registry::test_helpers` so spawner unit tests don't need the real Roblox parser.

### 10.2 TOML Import Path

Replaces the existing `serde::Deserialize` blobs (`InstanceDefinition`, `GuiTomlFile`, `ServiceDefinition`). Each spawner owns its own schema.

```rust
// e.g. PartSpawner
fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
    let mut bag = PropertyBag::with_capacity(20);

    // Pull from the [transform] section
    if let Some(tx) = toml_value.get("transform") {
        if let Some(pos) = tx.get("position").and_then(|v| v.as_array()) {
            bag.set("transform.position", PropertyValue::Vector3(
                Vec3::new(
                    pos.get(0).and_then(|v| v.as_float()).unwrap_or(0.0) as f32,
                    pos.get(1).and_then(|v| v.as_float()).unwrap_or(0.0) as f32,
                    pos.get(2).and_then(|v| v.as_float()).unwrap_or(0.0) as f32,
                )
            ));
        }
        // … rotation, scale …
    }

    // [properties] section
    if let Some(props) = toml_value.get("properties") {
        if let Some(mat) = props.get("material").and_then(|v| v.as_str()) {
            bag.set("properties.material", PropertyValue::String(mat.to_string()));
        }
        // … color, transparency, reflectance, anchored, can_collide, locked, cast_shadow
    }

    // [asset] section
    if let Some(asset) = toml_value.get("asset") {
        if let Some(mesh) = asset.get("mesh").and_then(|v| v.as_str()) {
            bag.set("asset.mesh", PropertyValue::String(mesh.to_string()));
        }
    }

    bag
}
```

### 10.3 TOML Export Path

The inverse — emits keys in canonical order (§4.3) so the on-disk TOML stays diff-stable.

```rust
fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
    let mut root = toml::value::Table::new();
    let mut transform = toml::value::Table::new();
    let mut props = toml::value::Table::new();
    let mut asset = toml::value::Table::new();

    let Some(t) = world.entity(entity).get::<Transform>() else {
        return toml::Value::Table(root);
    };
    transform.insert("position".into(), vec3_to_toml(t.translation));
    transform.insert("rotation".into(), quat_to_toml(t.rotation));
    transform.insert("scale".into(), vec3_to_toml(t.scale));

    if let Some(bp) = world.entity(entity).get::<BasePart>() {
        props.insert("material".into(), toml::Value::String(bp.material.as_str().into()));
        props.insert("color".into(), color_to_toml(bp.color));
        props.insert("transparency".into(), toml::Value::Float(bp.transparency as f64));
        props.insert("reflectance".into(), toml::Value::Float(bp.reflectance as f64));
        props.insert("anchored".into(), toml::Value::Boolean(bp.anchored));
        props.insert("can_collide".into(), toml::Value::Boolean(bp.can_collide));
        props.insert("locked".into(), toml::Value::Boolean(bp.locked));
        props.insert("cast_shadow".into(), toml::Value::Boolean(bp.cast_shadow));
    }

    if let Some(mesh_src) = world.entity(entity).get::<MeshSource>() {
        asset.insert("mesh".into(), toml::Value::String(mesh_src.path.clone()));
    }

    root.insert("transform".into(), toml::Value::Table(transform));
    root.insert("properties".into(), toml::Value::Table(props));
    root.insert("asset".into(), toml::Value::Table(asset));
    toml::Value::Table(root)
}
```

This replaces the bespoke `write_instance_changes_system` per-class field-by-field building in `instance_loader.rs:2144–2300`.

---

## 11. Cross-Document References

This spec is **referenced from** the following Wave 1 sibling specs (which will be produced separately):

| Doc | How it references this one |
|---|---|
| `CLASS_LIGHTING_AUDIT.md` | "Per-class light spawner contract is defined in `CLASS_REGISTRY.md §2.1` — the `spawn` method receives a `PropertyBag` with `color/brightness/range/shadows` keys, returns a Bevy `PointLight`/`SpotLight`/`DirectionalLight` entity." |
| `ROBLOX_IMPORT_SPEC.md` | "Per-class import logic lives in each spawner's `import_from_roblox` method (see `CLASS_REGISTRY.md §2.1` + §10.1). The importer pipeline walks the `rbx_dom_weak::WeakDom`, dispatches to the registry, and the spawner builds a `PropertyBag` from the Roblox properties." |
| `RENDER_CASCADE.md` | "Per-class LOD bundle selection is defined in `CLASS_REGISTRY.md §9` via `ClassSpawner::lod_components(tier)`. This document defines the tier-selection RULES; the trait method defines the per-class COMPONENT BUNDLES." |
| `IDENTITY.md` | "Per-class persistence (rkyv tag bytes, serialize/deserialize symmetry) is defined in `CLASS_REGISTRY.md §2.1` (`serialize`/`deserialize` methods) and Appendix A (tag byte layout). Every persisted class has a registered spawner; classes without spawners are skipped in the Fjall write path." |

This spec **references**:

| Doc | Used for |
|---|---|
| `FEATURE_PARITY.md` | Source of truth for "every ClassName needs a spawner eventually" (§1, §8) |
| `CLASS_CONVERSION.md` (existing) | Origin of `ConversionCategory` buckets that match the spawner groupings (§8) |
| `SPACE_ARCHITECTURE.md` (existing) | Source-of-truth for `_instance.toml` schema this trait round-trips |
| `SERIALIZATION_AUDIT.md` (existing) | Source-of-truth for the rkyv schema this trait persists |

---

## 12. Open Questions for Human Decision

These are explicitly **not resolved** by this spec — each represents a design choice the human owner should make before Wave 2 lands.

### Q1. Does `PropertyBag` preserve field order, sorted alphabetic order, or class-template order?

The spec proposes **template order** (§4.3). Alternatives:

- **Insertion order**: simplest, but two spawners building the same bag in different orders produce different bytes — breaks Fjall change detection.
- **Sorted alphabetic**: deterministic, easy to compute, no template dependency. Costs: TOML diffs reshuffle ("color" before "material" instead of grouped by category).
- **Template order**: matches existing on-disk authoring (§4.3). Costs: tightly couples PropertyBag to ClassSchemaRegistry; new classes must add their template before the first PropertyBag round-trip works.

**Recommendation**: template order, fall back to alphabetic when no template is registered. Asks for: confirmation from someone who's looked at how Roblox Studio actually orders fields in their RBXL writer.

### Q2. Are spawners object-safe (`Box<dyn ClassSpawner>`) or generic (`Vec<Box<dyn AnySpawner>>` with downcast)?

The spec mandates **object-safe** (§2.3). Alternatives considered:

- **Static dispatch with enum**: Wraps every spawner in a sealed `enum AnySpawner { PointLight(PointLightSpawner), … }`. Forces every spawner into `eustress-common`, kills third-party plugins.
- **Object-safe trait + downcast**: What this spec proposes. `Box<dyn ClassSpawner>` per class. Virtual dispatch (one indirect call per spawn). Plugins ship spawners freely.
- **`Box<dyn Any + Send + Sync>` registry + per-class trait**: Two-level lookup. Strictly more complex with no win.

**Recommendation**: object-safe trait. Asks for: confirmation that we don't anticipate per-spawn perf measurement showing virtual dispatch as a meaningful cost (a 1µs spawner has plenty of headroom).

### Q3. How does the registry handle classes whose component bundles touch `!Send` Slint state?

Some spawners (notably any UI class that touches `slint::Window`) need access to non-`Send` state. `ClassSpawner: Send + Sync + 'static` rules this out as a direct dependency. Options:

- **Defer Slint mutation**: spawner emits a Bevy `Event<SlintSpawnRequest>` that a non-Send local system on the main thread consumes. Adds one frame of latency.
- **Main-thread-only spawner subset**: register some spawners in a sibling `MainThreadClassRegistry` resource that only systems on the main thread can access. Bevy supports this via `NonSend`.
- **Avoid Slint in spawners**: keep Slint state pure-UI; spawners only build the data side (GuiElementDisplay, etc.) and a separate Slint sync system reads it.

**Recommendation**: the third option (matches the existing `GuiElementDisplay` ↔ Slint sync system). Spawners stay `Send + Sync`. Asks for: confirmation that no class genuinely needs to push state directly into a Slint property at spawn time (vs. on the next sync system tick).

### Q4. Should `apply_edit` return `bool` or an `EditResult` enum?

The spec uses `bool` (true = respawn needed). A richer enum could distinguish:
- `Applied` (mutation in place, no further work)
- `RespawnRequired` (caller must despawn + re-spawn)
- `PartialRespawnRequired { children_to_keep: Vec<Entity> }` (e.g. a Frame container whose layout changes but whose TextLabel children should be re-parented intact)
- `Rejected { reason: String }` (the edit is illegal — e.g. trying to change `Sound.PlaybackSpeed` to negative)

**Recommendation**: ship `bool` in Wave 2; upgrade to an enum in Wave 3 only if a real class hits the `PartialRespawnRequired` case. Asks for: confirmation that no current class needs the partial respawn path *before* Wave 2 freezes the API.

### Q5. Where does the `SpawnCtx`'s `parent_entity` come from for hot-creates (Insert menu)?

For cold load, `file_loader::spawn_directory_entry` knows the parent. For Roblox import, the importer walks the tree and knows it. For Insert menu hot create, the user has a selection; the Insert menu currently does its own parenting after spawn.

Options:
- **`SpawnCtx.parent_entity` is mandatory; Insert menu passes the selection**: spawner attaches the parent.
- **`SpawnCtx.parent_entity` is optional; caller does post-spawn parenting** (current behavior).

**Recommendation**: optional; keep the post-spawn pattern. Asks for: confirmation that Wave 3 doesn't need any class to know its parent at spawn time (e.g. for a `BillboardGui` to read the adornee's transform — even today this happens *after* spawn via `BillboardAdornee`).

### Q6. Do we register the abstract classes (`Instance`, `BasePart`, `PVInstance`)?

The spec says **no** (§8.17). Alternative: register them with a stub spawner that errors loudly. Rationale for stubs: catch buggy TOMLs that try to spawn an abstract class instead of silently falling back. Rationale for no-registration: today's `from_str` already aliases these to concrete classes, so this question is moot in practice.

**Recommendation**: no registration. Asks for: confirmation that no user/MCP/script path writes `class_name = "BasePart"` to disk (a one-off `find` would settle it).

### Q7. Does the migration window need a TOML-level `class_registry_version` marker?

When a spawner ships a breaking change (rare but possible), how do old TOMLs know which deserializer to use? Options:
- **Embed a version in the rkyv tag byte** (current spec — see Appendix A). Catches Fjall reads; doesn't catch TOML reads.
- **Add `class_registry_version = N` to `_instance.toml`'s `[metadata]`**: explicit, but pollutes every file with a usually-redundant field.
- **Rely on `ClassSchemaRegistry` template versioning**: templates already self-heal; a spawner upgrade ships a template upgrade.

**Recommendation**: rely on schema templates. Asks for: confirmation that all anticipated breaking changes are additive (new fields with defaults) rather than reshape (rename / move sections).

---

## 13. Risks & Mitigations

| # | Risk | Probability | Impact | Mitigation |
|---|---|---|---|---|
| R1 | Spawner trait churn during Wave 3 forces every implementer to re-PR | Medium | High | Freeze the trait at the end of Wave 2; treat any change as a major version bump. Add a `#[non_exhaustive]` default impl path so additive method additions don't break out-of-tree spawners. |
| R2 | A class's existing behavior can't be expressed in the trait (e.g. needs an unusual system param) | Medium | Medium | The SpawnCtx struct (§3) is a union of all needs. When a new spawner needs a borrow not in SpawnCtx, add it — the cost is one field per asset type. Identified candidates: `Assets<AudioSource>`, `Assets<KeyframeSequence>`, `Slint` `NonSend` state (see Q3). |
| R3 | Byte-equivalence test in Wave 3 reveals that legacy spawn ordering subtly changes when re-implemented (e.g. component insertion order matters for some Bevy lifecycle) | High | Medium | Wave 3's parity test (§7.5 `point_light_legacy_parity`) catches this per class. Mitigation when caught: order the new spawner's `commands.spawn((…))` tuple to match the legacy `spawn_point_light` exactly. |
| R4 | Third-party plugins ship spawners targeting Wave 2 trait shape; we break them in Wave 3 | Low | High | Public commitment: trait is frozen at Wave 2 end. Any change post-Wave-2 requires a major version bump and migration doc. |
| R5 | LOD `ComponentBundle` mechanism leaks `Box<dyn Reflect>` allocations per transition; per-frame transitions for thousands of entities pin allocator | Medium | Medium | Bench in Wave 3 with a 10k-entity scene cycling through tiers. If it hurts, replace `DynamicComponent` with a pre-built `Bundle` per tier — at the cost of mode-statically-typed bundles per spawner per tier (still expressible behind the object-safe trait by erasing through `Box<dyn FnOnce(EntityCommands)>`). |
| R6 | Roblox importer's `RobloxInstance` trait shape diverges from `rbx_dom_weak`'s actual API | High | Medium | Build the trait against rbx_dom_weak's current API (re-verify in Wave 4); accept that we re-export, not pass-through. If rbx_dom_weak changes, an adapter layer absorbs it. |
| R7 | `PropertyBag` key strings get duplicated across thousands of bags; memory waste | Low | Low | Wave 3 measures memory footprint at 50k entities; if it matters, intern keys via `Arc<str>` or use the `string_cache` crate. The 80-spawner-x-20-keys ceiling is ~1600 unique key strings — fits in a small intern table. |
| R8 | Spawner registration order panics surface late (only at first spawn of the affected class) | Low | Medium | `log_registry_validation` runs at Startup (§5.2) and warns when a `ClassName` variant has no spawner. Promote to `panic!` in debug builds; keep `warn` in release so a forgotten plugin doesn't crash production. |
| R9 | Fjall rkyv schema breakage during migration causes data loss | Medium | Critical | Tag byte (Appendix A) rejects mismatched archives; reject = "skip this entity, log warning, fall back to TOML if available." Never silently overwrite. Tag bump is gated on the same `WorldSchemaVersion` discipline `worlddb` already uses (`header.rs`). |
| R10 | Cold-load performance regresses because the registry adds a HashMap lookup per spawn that the match arms didn't have | Low | Low | HashMap lookup is ~50ns; spawn itself is microseconds. The benchmark in `eustress/crates/engine/src/bin/generate_benchmark_map.rs` is the existing perf gate. Wave 3 ships a comparison run. |

---

## Appendix A: Wire-Format Tag Bytes

Every rkyv archive emitted by `ClassSpawner::serialize` carries a leading tag byte. The byte encodes (schema_version << 4) | class_group_tag, allowing both global schema bumps and per-class group upgrades.

```
Byte 0 layout:
+----+----+----+----+----+----+----+----+
| schema_version (4 bits) | group (4 bits) |
+----+----+----+----+----+----+----+----+

schema_version: 1..=15 — matches WorldSchemaVersion in worlddb::header.
                Bump in lockstep with the rkyv struct shape changing.

group:          0 = generic (most spawners)
                1 = geometry (Part, Model, SpecialMesh — extra mesh path)
                2 = gui (extra UI props)
                3 = constraint (extra Part0/Part1 referent fields)
                4 = light (extra texture handle field)
                5 = adornment (transient — may not need persistence at all)
                6 = asset (Material/Image/Video — extra asset-path field)
                7..=15 reserved
```

Current `ArchTransform` and `EusValue` tag is `1` (per `worlddb::rkyv_values::RKYV_VALUE_TAG`). Class spawners inherit the same tag space initially (`schema_version=1, group=0..=6`).

**Decode contract**: a deserialize call that receives a mismatched tag returns an empty PropertyBag and logs a `warn!` line with the entity key and tag bytes. The engine never panics on a tag mismatch.

---

## Appendix B: Worked Example — PointLight Migration

End-to-end walkthrough of migrating `PointLight` from the current `spawn.rs::spawn_point_light` to a registered spawner. This is the canonical Wave 3 PR template.

### B.1 Current State (Wave 0)

`eustress/crates/engine/src/spawn.rs:409`:

```rust
pub fn spawn_point_light(
    commands: &mut Commands,
    instance: Instance,
    light: EustressPointLight,
    transform: Transform,
) -> Entity {
    let name = instance.name.clone();
    commands.spawn((
        PointLight {
            color: light.color,
            intensity: light.brightness,
            range: light.range,
            radius: light.radius,
            shadows_enabled: light.shadows,
            ..default()
        },
        transform, instance, light, Name::new(name),
    )).id()
}
```

Today: not called from `file_loader` at all (PointLights spawn as non-visual instances and then a Lighting service sync system attaches the actual `PointLight` component later). The function exists in `spawn.rs` but its actual production path is split.

### B.2 Wave 3 Spawner

`eustress/crates/engine/src/spawners/lights/point_light.rs`:

```rust
use bevy::prelude::*;
use eustress_common::class_registry::*;
use eustress_common::classes::*;

#[derive(Default)]
pub struct PointLightSpawner;

impl ClassSpawner for PointLightSpawner {
    fn class_name(&self) -> ClassName { ClassName::PointLight }

    fn spawn(&self, ctx: &mut SpawnCtx, props: &PropertyBag) -> Entity {
        let name = props.get_string("metadata.name").unwrap_or("PointLight").to_string();
        let color = props.get_color("light.color").unwrap_or(Color::WHITE);
        let brightness = props.get_f32("light.brightness").unwrap_or(800.0);
        let range = props.get_f32("light.range").unwrap_or(16.0);
        let radius = props.get_f32("light.radius").unwrap_or(0.0);
        let shadows = props.get_bool("light.shadows").unwrap_or(true);
        let transform = props.get_transform("transform").cloned().unwrap_or_default();

        ctx.commands.spawn((
            PointLight {
                color, intensity: brightness, range, radius,
                shadows_enabled: shadows,
                ..default()
            },
            transform,
            Instance {
                name: name.clone(),
                class_name: ClassName::PointLight,
                archivable: true, id: 0, ai: false, uuid: String::new(),
            },
            EustressPointLight { color, brightness, range, radius, shadows, texture: None },
            Name::new(name),
        )).id()
    }

    fn serialize(&self, world: &World, entity: Entity) -> Vec<u8> {
        let mut out = vec![RKYV_VALUE_TAG_POINT_LIGHT];
        // … rkyv archive of PointLight + EustressPointLight + Transform mirror …
        out
    }

    fn deserialize(&self, bytes: &[u8]) -> PropertyBag {
        if bytes.first() != Some(&RKYV_VALUE_TAG_POINT_LIGHT) {
            warn!("PointLightSpawner: tag mismatch, returning empty bag");
            return PropertyBag::new();
        }
        // … rkyv decode + populate PropertyBag …
    }

    fn apply_edit(&self, world: &mut World, entity: Entity, props: &PropertyBag) -> bool {
        // Color / brightness / range / shadows changes are cheap mutations.
        // No respawn needed for any PointLight property.
        if let Some(mut pl) = world.entity_mut(entity).get_mut::<PointLight>() {
            if let Some(c) = props.get_color("light.color") { pl.color = c; }
            if let Some(b) = props.get_f32("light.brightness") { pl.intensity = b; }
            if let Some(r) = props.get_f32("light.range") { pl.range = r; }
            if let Some(s) = props.get_bool("light.shadows") { pl.shadows_enabled = s; }
        }
        false  // never needs respawn
    }

    fn lod_components(&self, tier: LodTier) -> ComponentBundle {
        match tier {
            LodTier::Hero | LodTier::Active => ComponentBundle {
                insert: vec![],
                remove: vec![TypeId::of::<NotShadowCaster>()],
            },
            LodTier::Streamed => ComponentBundle {
                insert: vec![DynamicComponent(Box::new(NotShadowCaster))],
                remove: vec![],
            },
            LodTier::Horizon => ComponentBundle {
                insert: vec![DynamicComponent(Box::new(Visibility::Hidden))],
                remove: vec![],
            },
        }
    }

    fn import_from_roblox(&self, rbx: &dyn RobloxInstance) -> PropertyBag {
        let mut bag = PropertyBag::new();
        bag.set("metadata.name", PropertyValue::String(rbx.name().into()));
        if let Some(color) = rbx.property("Color") {
            bag.set("light.color", color.into_eustress_color());
        }
        if let Some(b) = rbx.property("Brightness").and_then(|p| p.as_f32()) {
            bag.set("light.brightness", PropertyValue::Float(b * 800.0));  // Roblox units → lumens
        }
        if let Some(r) = rbx.property("Range").and_then(|p| p.as_f32()) {
            bag.set("light.range", PropertyValue::Float(r));
        }
        if let Some(s) = rbx.property("Shadows").and_then(|p| p.as_bool()) {
            bag.set("light.shadows", PropertyValue::Bool(s));
        }
        bag
    }

    fn import_from_toml(&self, toml_value: &toml::Value) -> PropertyBag {
        let mut bag = PropertyBag::new();
        if let Some(meta) = toml_value.get("metadata") {
            if let Some(n) = meta.get("name").and_then(|v| v.as_str()) {
                bag.set("metadata.name", PropertyValue::String(n.into()));
            }
        }
        if let Some(light) = toml_value.get("light") {
            // color / brightness / range / radius / shadows extraction
            // (~30 lines of straightforward toml::Value get + type check)
        }
        if let Some(tx) = toml_value.get("transform") {
            // position / rotation / scale → PropertyValue::Transform(…)
        }
        bag
    }

    fn export_to_toml(&self, world: &World, entity: Entity) -> toml::Value {
        let mut root = toml::value::Table::new();
        // [metadata]
        if let Some(inst) = world.entity(entity).get::<Instance>() {
            let mut meta = toml::value::Table::new();
            meta.insert("class_name".into(), "PointLight".into());
            meta.insert("name".into(), inst.name.clone().into());
            meta.insert("archivable".into(), inst.archivable.into());
            root.insert("metadata".into(), toml::Value::Table(meta));
        }
        // [transform], [light] — same pattern as PartSpawner::export_to_toml
        toml::Value::Table(root)
    }
}
```

### B.3 Registration

In `eustress/crates/engine/src/class_registry/plugin.rs`:

```rust
#[cfg(feature = "class-registry")]
{
    app.register_class::<spawners::lights::PointLightSpawner>();
    // … other lights …
}
```

### B.4 Shim

In `eustress/crates/engine/src/spawn.rs` (kept until Wave 5):

```rust
pub fn spawn_point_light(
    commands: &mut Commands,
    instance: Instance,
    light: EustressPointLight,
    transform: Transform,
    // NEW (Wave 3): optional registry for the shim path
    registry: Option<&ClassRegistry>,
    asset_server: Option<&AssetServer>,
    // … other ctx params, only used when registry is Some
) -> Entity {
    #[cfg(feature = "class-registry")]
    if let (Some(registry), Some(asset_server)) = (registry, asset_server) {
        if let Some(spawner) = registry.get(ClassName::PointLight) {
            // Build PropertyBag from the legacy struct args
            let mut props = PropertyBag::new();
            props.set("metadata.name", PropertyValue::String(instance.name.clone()));
            props.set("light.color", PropertyValue::Color(light.color));
            props.set("light.brightness", PropertyValue::Float(light.brightness));
            props.set("light.range", PropertyValue::Float(light.range));
            props.set("light.radius", PropertyValue::Float(light.radius));
            props.set("light.shadows", PropertyValue::Bool(light.shadows));
            props.set("transform", PropertyValue::Transform(transform));

            let mut ctx = build_spawn_ctx_minimal(commands, asset_server);
            return spawner.spawn(&mut ctx, &props);
        }
    }

    // Legacy path (unchanged from B.1)
    let name = instance.name.clone();
    commands.spawn((
        PointLight {
            color: light.color,
            intensity: light.brightness,
            range: light.range,
            radius: light.radius,
            shadows_enabled: light.shadows,
            ..default()
        },
        transform, instance, light, Name::new(name),
    )).id()
}
```

### B.5 Test

`eustress/crates/engine/tests/class_registry/point_light.rs`:

```rust
mod roundtrip { … as in §7.5 … }
mod legacy_parity { … as in §7.5 … }

#[test]
fn point_light_import_from_roblox_default_brightness() {
    let mock = MockRobloxInstance::new("PointLight")
        .with_property("Brightness", 1.0);  // Roblox default
    let spawner = PointLightSpawner;
    let props = spawner.import_from_roblox(&mock);
    assert_eq!(props.get_f32("light.brightness"), Some(800.0));  // → lumens
}

#[test]
fn point_light_lod_streamed_drops_shadow() {
    let spawner = PointLightSpawner;
    let bundle = spawner.lod_components(LodTier::Streamed);
    assert!(bundle.insert.iter().any(|c| c.0.is::<NotShadowCaster>()));
}
```

### B.6 PR Size

One spawner = ~400 lines of new code + ~50 lines of test + ~20 lines of shim. Reviewable in one sitting. Total ~80 spawners → 80 PRs in Wave 3, each independently bisectable.

---

### Critical Files for Implementation

These are the 5 files most critical to building this trait registry (Wave 2):

- E:/Workspace/EustressEngine/eustress/crates/common/src/classes.rs
- E:/Workspace/EustressEngine/eustress/crates/common/src/class_schema/mod.rs
- E:/Workspace/EustressEngine/eustress/crates/engine/src/space/instance_loader.rs
- E:/Workspace/EustressEngine/eustress/crates/engine/src/spawn.rs
- E:/Workspace/EustressEngine/eustress/crates/worlddb/src/rkyv_values.rs

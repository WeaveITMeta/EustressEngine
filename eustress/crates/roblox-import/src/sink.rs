//! Import storage backends — where a materialised node's authoritative
//! state lands.
//!
//! Spec ref: `docs/architecture/IMPORT_STORAGE_AND_PORTABILITY.md` §8.A
//! ("BinaryDirect importer backend"). The materializer no longer hard-codes
//! the `_instance.toml`-folder write; it hands each node to an
//! [`ImportSink`] chosen from [`ImportStorage`]. Two implementations:
//!
//! - [`TomlSink`] — the original behaviour: one folder + `_instance.toml`
//!   per node via the canonical
//!   [`eustress_common::instance_create::create_instance`] pipeline. The
//!   engine file-watcher then hot-loads it and (post-migration) the
//!   reconcile-on-open mirrors it into Fjall. This is the only backend
//!   available without the `binary-sink` crate feature, so the engine-free
//!   default build keeps depending on `eustress-common` alone.
//! - [`BinarySink`] — the §8.A "BinaryDirect" path (behind
//!   `binary-sink`): bare, scalable parts skip TOML entirely and bake
//!   straight to a zero-copy rkyv
//!   [`eustress_worlddb::ArchInstanceCore`] in the worlddb `entities`
//!   partition (Morton-keyed) plus the IDENTITY.md Wave 2.1 UUID index
//!   stores. **File-natured** nodes (scripts, GUI, documents, custom
//!   meshes) still go through the inner [`TomlSink`] — the engine
//!   file-watcher must be able to hot-load their backing files, and a
//!   binary-ECS record cannot hold a real path. That fall-back is the
//!   §8.A "BinaryDirect still honors `representation_for_part`" rule.
//!
//! ## What is replicated here vs. reached
//!
//! `eustress-roblox-import` depends on `eustress-common` only (it is
//! engine-free). The two pieces the engine owns that §8.A needs —
//! `instance_to_arch` (in `engine/src/space/arch_instance.rs`) and
//! `representation_for_part` (in `engine/src/space/representation.rs`) —
//! are NOT reachable from here. So:
//!
//! - The representation predicate is **replicated** as
//!   [`is_file_natured_node`] + [`mesh_requires_filesystem`], a faithful
//!   copy of `engine::space::representation::{class_is_file_natured,
//!   mesh_requires_filesystem}`. (Folder-artifact detection is irrelevant
//!   during import — a freshly imported node has no pre-existing folder.)
//! - The core bake is done **directly into `ArchInstanceCore`** from the
//!   importer's own `(InstanceOverrides, PropertyBag)` data, rather than
//!   via the engine's `instance_to_arch` (which only accepts the
//!   engine-side `InstanceDefinition`). `ArchInstanceCore` itself lives in
//!   `eustress-worlddb`, which IS a dependency under the feature, and its
//!   fields are public — so the bake is a straight field map. The cold
//!   tail uses the SAME reserved `__attributes` / `__extra` keys the
//!   engine's `arch_instance` module uses, so an engine save-back round-
//!   trips losslessly.

use std::path::{Path, PathBuf};

use eustress_common::classes::ClassName;
use eustress_common::instance_create::{create_instance, InstanceOverrides};

use crate::error::ImportError;

// ---------------------------------------------------------------------------
// ImportStorage — the public knob (ImportOptions::storage)
// ---------------------------------------------------------------------------

/// Where an import writes each node's authoritative state. Spec §8.A.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportStorage {
    /// **Default.** Bare, scalable parts bake straight to rkyv
    /// `ArchInstanceCore` records in the worlddb `entities` partition;
    /// file-natured nodes (scripts/GUI/documents/custom-mesh parts) fall
    /// back to `_instance.toml` folders so the engine file-watcher can
    /// hot-load them. Requires the `binary-sink` feature; without it this
    /// degrades to [`ImportStorage::TomlFolders`] (logged once).
    #[default]
    BinaryDirect,
    /// Every node written as a `_instance.toml` folder (the original
    /// importer behaviour). The reconcile-on-open path later mirrors them
    /// into Fjall. No worlddb dependency.
    TomlFolders,
    /// Bare parts → binary `entities` partition AND a `_instance.toml`
    /// folder is also written. Belt-and-braces for debugging / migration
    /// verification; doubles disk + DB churn so not a default.
    Hybrid,
}

impl ImportStorage {
    /// True when this mode writes binary-ECS cores for bare parts. Both
    /// `BinaryDirect` and `Hybrid` do; `TomlFolders` never does.
    pub fn writes_binary(self) -> bool {
        matches!(self, ImportStorage::BinaryDirect | ImportStorage::Hybrid)
    }

    /// True when this mode also writes a `_instance.toml` folder for every
    /// node (`TomlFolders` always; `Hybrid` in addition to the binary core).
    pub fn writes_toml_folders(self) -> bool {
        matches!(self, ImportStorage::TomlFolders | ImportStorage::Hybrid)
    }
}

// ---------------------------------------------------------------------------
// NodeSpec — the per-node data the materializer hands a sink.
// ---------------------------------------------------------------------------

/// Everything a sink needs to persist one materialised node. Built by the
/// materializer from the mapped class + [`crate::property_map::PropertyBag`].
///
/// This is the importer-local stand-in for the engine's `InstanceDefinition`
/// (which a sink in this crate cannot construct — it is engine-side). The
/// `BinarySink` bakes these fields straight into an
/// [`eustress_worlddb::ArchInstanceCore`]; the `TomlSink` forwards the
/// `overrides` into the canonical create pipeline and lets the materializer
/// patch the rest onto the TOML afterwards.
pub struct NodeSpec<'a> {
    /// Eustress class the node maps to.
    pub class: ClassName,
    /// Template / class name string (`class.as_str()` — what the canonical
    /// create pipeline keys its class_schema template on).
    pub class_template: &'a str,
    /// Requested display + folder name (folder name is unique-safed by the
    /// create pipeline; the display name is stamped verbatim).
    pub requested_name: &'a str,
    /// Well-known typed slots (position/rotation/scale/color/material/…),
    /// already populated by the property mapper.
    pub overrides: &'a InstanceOverrides,
    /// Deterministic 32-char-hex entity UUID (from
    /// [`crate::identity::entity_uuid`]) — the persistent identity used for
    /// both the TOML `metadata.uuid` stamp and the binary UUID index keys.
    pub uuid_hex: &'a str,
    /// `[properties.extras]` entries (round-trip storage for properties
    /// without a first-class slot).
    pub extras: &'a std::collections::HashMap<String, toml::Value>,
    /// `[properties.physics]` entries (PhysicalProperties decomposition).
    pub physics: &'a std::collections::HashMap<String, toml::Value>,
    /// `[properties.attributes]` entries.
    pub attributes: &'a std::collections::HashMap<String, toml::Value>,
    /// `[metadata.tags]` values.
    pub tags: &'a [String],
}

/// Outcome of an [`ImportSink::write`] — tells the materializer how the node
/// was persisted and (for the TOML path) where the file is so the second
/// pass can patch refs / scripts / asset blocks onto it.
#[derive(Debug, Clone)]
pub struct WrittenRef {
    /// `Some(..)` when a `_instance.toml` folder was written (the materializer
    /// will post-process it); `None` for a pure binary-ECS write (nothing on
    /// disk to patch).
    pub toml: Option<TomlWrite>,
    /// True when a binary-ECS `ArchInstanceCore` record was written for this
    /// node. Informational (drives the import report's binary counter).
    pub wrote_binary_core: bool,
}

/// The disk locations of a node that landed as a `_instance.toml` folder.
#[derive(Debug, Clone)]
pub struct TomlWrite {
    /// `<dest>/<unique_name>/` — the node's folder.
    pub folder_path: PathBuf,
    /// `<folder_path>/_instance.toml`.
    pub toml_path: PathBuf,
    /// Final unique-safed folder name (may differ from `requested_name`).
    pub folder_name: String,
}

// ---------------------------------------------------------------------------
// ImportSink trait
// ---------------------------------------------------------------------------

/// A backend that persists one materialised node. Chosen per import from
/// [`ImportStorage`]; the materializer calls [`ImportSink::write`] for every
/// node instead of hard-coding `create_instance`.
pub trait ImportSink {
    /// Persist one node into `dest_dir` (its parent on disk, also used to
    /// derive the binary record's spatial key fallback). Returns a
    /// [`WrittenRef`] describing what was written.
    fn write(&mut self, dest_dir: &Path, spec: &NodeSpec<'_>) -> Result<WrittenRef, ImportError>;
}

// ---------------------------------------------------------------------------
// TomlSink — the original create_instance path, unchanged behaviour.
// ---------------------------------------------------------------------------

/// Writes every node as a folder + `_instance.toml` via the canonical
/// [`create_instance`] pipeline. This is the importer's historical
/// behaviour and the only sink available without the `binary-sink` feature.
#[derive(Debug, Default)]
pub struct TomlSink;

impl TomlSink {
    /// Construct a TOML sink.
    pub fn new() -> Self {
        Self
    }

    /// The actual `create_instance` call + the deterministic-uuid stamp.
    /// Shared by [`TomlSink::write`] and by [`BinarySink`]'s file-natured
    /// fall-back so both produce byte-identical folders.
    fn create_toml(&self, dest_dir: &Path, spec: &NodeSpec<'_>) -> Result<TomlWrite, ImportError> {
        let mut overrides = spec.overrides.clone();
        overrides.display_name = Some(spec.requested_name.to_string());

        let created = create_instance(
            dest_dir,
            spec.class_template,
            Some(spec.requested_name),
            overrides,
        )
        .map_err(|e| ImportError::InstanceCreate {
            class: spec.class_template.to_string(),
            source_msg: e.to_string(),
        })?;

        Ok(TomlWrite {
            folder_path: created.folder_path,
            toml_path: created.toml_path,
            folder_name: created.folder_name,
        })
    }
}

impl ImportSink for TomlSink {
    fn write(&mut self, dest_dir: &Path, spec: &NodeSpec<'_>) -> Result<WrittenRef, ImportError> {
        let toml = self.create_toml(dest_dir, spec)?;
        Ok(WrittenRef {
            toml: Some(toml),
            wrote_binary_core: false,
        })
    }
}

// ---------------------------------------------------------------------------
// Representation predicate — replicated from engine::space::representation.
// ---------------------------------------------------------------------------

/// True when a node's essential content is a real file, so it MUST be a
/// `_instance.toml` folder (the engine file-watcher hot-loads the backing
/// file; a binary-ECS record cannot hold a path).
///
/// Faithful replica of `engine::space::representation::class_is_file_natured`
/// (`eustress/crates/engine/src/space/representation.rs`). Kept in sync by
/// hand because that function is engine-side and unreachable from this crate.
/// Driven by the resolved Eustress [`ClassName`] (the importer's typed class)
/// rather than the raw string, so it covers the legacy-alias remaps
/// `class_map` performs (e.g. `Script` → `LuauScript`).
pub fn is_file_natured_node(class: ClassName) -> bool {
    matches!(
        class,
        // Scripts + AI artifacts — backed by `.luau`/`.rune`/transcript files.
        ClassName::SoulScript
            | ClassName::LuauScript
            | ClassName::LuauLocalScript
            | ClassName::LuauModuleScript
            | ClassName::WorkshopConversation
            // Explicit document / imported-file nodes.
            | ClassName::Document
            // GUI classes are authored as `.toml` layout files + edited as
            // text, so they stay FileSystem.
            | ClassName::ScreenGui
            | ClassName::SurfaceGui
            | ClassName::BillboardGui
            | ClassName::Frame
            | ClassName::ScrollingFrame
            | ClassName::TextLabel
            | ClassName::TextButton
            | ClassName::TextBox
            | ClassName::ImageLabel
            | ClassName::ImageButton
    )
}

/// True when an `asset.mesh` reference can only resolve relative to the
/// entity's on-disk location, so the part MUST stay a `_instance.toml`
/// folder. Bundled primitives live under `parts/` and resolve from the
/// engine asset source with no folder → binary-ECS-compatible. ANY other
/// mesh (a custom upload, a `../meshes/...` relative path) resolves relative
/// to the part's folder, which a binary-ECS record has no equivalent for.
///
/// Faithful replica of `engine::space::representation::mesh_requires_filesystem`.
/// This is the V-Cell guard: a custom-mesh Part must never land in the
/// `entities` partition, or its mesh string would survive but the file would
/// be unfindable on load.
pub fn mesh_requires_filesystem(mesh: &str) -> bool {
    !mesh.is_empty() && !mesh.starts_with("parts/")
}

/// The §8.A routing decision for one node: does it go to binary ECS or must
/// it stay a `_instance.toml` folder? Mirrors
/// `engine::space::representation::representation_for_part` minus the
/// folder-artifact check (a freshly imported node has no pre-existing folder).
///
/// `mesh` is the resolved asset-mesh override if any (from
/// `InstanceOverrides::asset_mesh`). A custom/relative mesh or a single-path
/// asset (`asset_path`) forces the folder path.
pub fn node_is_binary_eligible(class: ClassName, overrides: &InstanceOverrides) -> bool {
    if is_file_natured_node(class) {
        return false;
    }
    // A single-path asset (Image / Video / a dropped media file) is a real
    // file artifact → must be a folder.
    if overrides.asset_path.is_some() {
        return false;
    }
    if let Some(mesh) = overrides.asset_mesh.as_deref() {
        if mesh_requires_filesystem(mesh) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// BinarySink — the §8.A BinaryDirect path (feature-gated).
// ---------------------------------------------------------------------------

#[cfg(feature = "binary-sink")]
pub use binary::BinarySink;

#[cfg(feature = "binary-sink")]
mod binary {
    use std::path::Path;
    use std::sync::Arc;

    use eustress_common::instance_create::uuid_hex_to_bytes;
    use eustress_worlddb::{encode_instance_core, ArchInstanceCore, EntityId, EusValue, WorldDb};

    use super::{node_is_binary_eligible, ImportSink, NodeSpec, TomlSink, TomlWrite, WrittenRef};
    use crate::error::ImportError;

    // Reserved cold-tail keys — MUST match
    // `engine::space::arch_instance::{ATTRS_KEY, EXTRA_KEY}` so an engine
    // save-back (`arch_to_instance`) reconstructs the attributes/extras
    // sections losslessly. (The importer has no `InstanceMetadata` /
    // material / thermo / UI structs to fold here — those engine-side
    // sections are simply absent on an imported bare part.) Physics is
    // imported-only enrichment with no engine cold-tail slot, so it is
    // folded under a clearly importer-scoped `__physics` key.
    const ATTRS_KEY: &str = "__attributes";
    const EXTRA_KEY: &str = "__extra";
    const PHYSICS_KEY: &str = "__physics";

    /// The §8.A "BinaryDirect" sink: bare parts bake straight to a rkyv
    /// `ArchInstanceCore` in the worlddb `entities` partition; file-natured
    /// nodes fall through to the inner [`TomlSink`].
    pub struct BinarySink {
        db: Arc<dyn WorldDb>,
        toml: TomlSink,
        /// When true (Hybrid mode) a `_instance.toml` folder is ALSO written
        /// for binary-eligible nodes, in addition to the binary core.
        also_write_toml: bool,
    }

    impl BinarySink {
        /// Construct a binary sink over an open worlddb handle. `also_write_toml`
        /// = the `Hybrid` storage mode (write both the binary core and a TOML
        /// folder); `false` = pure `BinaryDirect`.
        pub fn new(db: Arc<dyn WorldDb>, also_write_toml: bool) -> Self {
            Self {
                db,
                toml: TomlSink::new(),
                also_write_toml,
            }
        }

        /// Bake a [`NodeSpec`] straight into an [`ArchInstanceCore`].
        ///
        /// This is the importer-side equivalent of the engine's
        /// `instance_to_arch` (which we cannot call — it takes the engine's
        /// `InstanceDefinition`). The typed hot fields map from the property
        /// mapper's `InstanceOverrides`; the cold tail folds attributes +
        /// extras + physics under the same reserved `__` keys the engine uses.
        fn bake_core(spec: &NodeSpec<'_>) -> ArchInstanceCore {
            let ov = spec.overrides;

            let (mesh, scene) = match ov.asset_mesh.as_deref() {
                Some(m) if !m.is_empty() => (m.to_string(), "Scene0".to_string()),
                _ => (String::new(), String::new()),
            };

            // Build the cold tail under the engine-compatible reserved keys.
            let mut extra: Vec<(String, EusValue)> = Vec::new();
            if !spec.attributes.is_empty() {
                extra.push((ATTRS_KEY.to_string(), map_to_eus(spec.attributes)));
            }
            if !spec.extras.is_empty() {
                extra.push((EXTRA_KEY.to_string(), map_to_eus(spec.extras)));
            }
            if !spec.physics.is_empty() {
                extra.push((PHYSICS_KEY.to_string(), map_to_eus(spec.physics)));
            }
            // Deterministic archive bytes (same discipline as the engine bake).
            extra.sort_by(|a, b| a.0.cmp(&b.0));

            ArchInstanceCore {
                class_name: spec.class.as_str().to_string(),
                mesh,
                scene,
                t: ov.position.unwrap_or([0.0, 0.0, 0.0]),
                r: ov.rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]),
                s: ov.scale.unwrap_or([1.0, 1.0, 1.0]),
                color: ov.color_rgba.unwrap_or([1.0, 1.0, 1.0, 1.0]),
                transparency: ov
                    .color_rgba
                    .map(|c| 1.0 - c[3])
                    .filter(|t| *t > 0.0)
                    .unwrap_or(0.0),
                reflectance: 0.0,
                anchored: ov.anchored.unwrap_or(false),
                can_collide: ov.can_collide.unwrap_or(true),
                cast_shadow: true,
                locked: false,
                material: ov.material.clone().unwrap_or_default(),
                tags: spec.tags.to_vec(),
                extra,
            }
        }

        /// Write the baked core into the worlddb `entities` partition
        /// (Morton-keyed, the binary-ECS boot-load source) AND the IDENTITY.md
        /// Wave 2.1 UUID index stores (so `find_entity --uuid/--path/--class`
        /// resolve the imported entity). Idempotent on re-import: the
        /// deterministic UUID drives the `stored_id`, so a re-import overwrites
        /// the same Morton + UUID records instead of duplicating.
        fn put_core(&self, spec: &NodeSpec<'_>) -> Result<(), ImportError> {
            let uuid = uuid_hex_to_bytes(spec.uuid_hex).ok_or_else(|| ImportError::BinarySink {
                class: spec.class_template.to_string(),
                source_msg: format!("malformed deterministic uuid {:?}", spec.uuid_hex),
            })?;

            let core = Self::bake_core(spec);
            let bytes = encode_instance_core(&core).map_err(|e| ImportError::BinarySink {
                class: spec.class_template.to_string(),
                source_msg: format!("rkyv encode core: {e}"),
            })?;

            // `stored_id` is the STABLE persistence id the engine's
            // BinaryEcsInstance keys the Morton record on — NOT a live Bevy
            // `Entity::to_bits()`. Derive it from the first 8 bytes of the
            // deterministic UUID so a re-import lands on the same key.
            let stored_id = u64::from_be_bytes(uuid[..8].try_into().unwrap());
            let pos = (core.t[0], core.t[1], core.t[2]);

            let to_err = |what: &str, e: eustress_worlddb::Error| ImportError::BinarySink {
                class: spec.class_template.to_string(),
                source_msg: format!("{what}: {e}"),
            };

            // Morton-keyed `entities` partition — the binary-ECS boot-load
            // source the engine's `load_binary_ecs_instances` scans.
            self.db
                .put_instance_core(EntityId(stored_id), pos, &bytes)
                .map_err(|e| to_err("put_instance_core", e))?;

            // IDENTITY.md Wave 2.1 UUID-primary store + secondary indexes.
            // Path index uses the synthetic binary-ECS path shape the engine
            // mints (`Workspace/__bin_<Class>_<stored_id:016x>/...`) so a
            // later `find_entity --path` agrees with the engine's loader.
            self.db
                .put_entity_core_by_uuid(&uuid, &bytes)
                .map_err(|e| to_err("put_entity_core_by_uuid", e))?;
            let synthetic_path = format!(
                "Workspace/__bin_{}_{:016x}/_instance.toml",
                core.class_name, stored_id
            );
            self.db
                .put_path_to_uuid(&synthetic_path, &uuid)
                .map_err(|e| to_err("put_path_to_uuid", e))?;
            self.db
                .put_uuid_to_path(&uuid, &synthetic_path)
                .map_err(|e| to_err("put_uuid_to_path", e))?;
            self.db
                .put_class_index(&core.class_name, &uuid)
                .map_err(|e| to_err("put_class_index", e))?;

            Ok(())
        }
    }

    impl ImportSink for BinarySink {
        fn write(
            &mut self,
            dest_dir: &Path,
            spec: &NodeSpec<'_>,
        ) -> Result<WrittenRef, ImportError> {
            // §8.A: BinaryDirect still honors the representation predicate —
            // file-natured nodes (scripts/GUI/documents/custom-mesh parts)
            // MUST stay `_instance.toml` folders so the engine file-watcher
            // can hot-load them.
            if !node_is_binary_eligible(spec.class, spec.overrides) {
                return self.toml.write(dest_dir, spec);
            }

            self.put_core(spec)?;

            // Hybrid also drops a TOML folder for verification / migration.
            let toml: Option<TomlWrite> = if self.also_write_toml {
                Some(self.toml.create_toml(dest_dir, spec)?)
            } else {
                None
            };

            Ok(WrittenRef {
                toml,
                wrote_binary_core: true,
            })
        }
    }

    /// `HashMap<String, toml::Value>` → an `EusValue::Table` with keys
    /// sorted (deterministic archive bytes), via worlddb's
    /// `toml::Value → EusValue` bridge.
    fn map_to_eus(map: &std::collections::HashMap<String, toml::Value>) -> EusValue {
        let mut t = toml::value::Table::new();
        for (k, v) in map {
            t.insert(k.clone(), v.clone());
        }
        EusValue::from(toml::Value::Table(t))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Representation routing (no feature needed — pure predicate). ──

    #[test]
    fn bare_primitive_part_is_binary_eligible() {
        // The §8.A scalable default: a bare Part with a bundled primitive
        // mesh (or no mesh) goes to binary ECS.
        let mut ov = InstanceOverrides::default();
        assert!(node_is_binary_eligible(ClassName::Part, &ov));
        ov.asset_mesh = Some("parts/block.glb".to_string());
        assert!(node_is_binary_eligible(ClassName::Part, &ov));
    }

    #[test]
    fn custom_mesh_part_falls_back_to_filesystem() {
        // The V-Cell guard: a custom / relative mesh forces a TOML folder.
        let ov = InstanceOverrides {
            asset_mesh: Some("../meshes/VCell_Housing.glb".to_string()),
            ..Default::default()
        };
        assert!(!node_is_binary_eligible(ClassName::Part, &ov));
        assert!(mesh_requires_filesystem("../meshes/VCell_Housing.glb"));
        assert!(!mesh_requires_filesystem("parts/ball.glb"));
    }

    #[test]
    fn single_path_asset_forces_filesystem() {
        // Image / Video carry an `[asset].path` — a real file → must be a
        // folder, never a binary core.
        let ov = InstanceOverrides {
            asset_path: Some("assets/images/logo.png".to_string()),
            ..Default::default()
        };
        assert!(!node_is_binary_eligible(ClassName::Image, &ov));
    }

    #[test]
    fn file_natured_classes_are_never_binary() {
        for class in [
            ClassName::LuauScript,
            ClassName::LuauLocalScript,
            ClassName::LuauModuleScript,
            ClassName::SoulScript,
            ClassName::ScreenGui,
            ClassName::TextLabel,
            ClassName::ImageButton,
        ] {
            assert!(
                is_file_natured_node(class),
                "{class:?} should be file-natured",
            );
            assert!(
                !node_is_binary_eligible(class, &InstanceOverrides::default()),
                "{class:?} should never be binary-eligible",
            );
        }
    }

    #[test]
    fn import_storage_helpers() {
        assert!(ImportStorage::BinaryDirect.writes_binary());
        assert!(ImportStorage::Hybrid.writes_binary());
        assert!(!ImportStorage::TomlFolders.writes_binary());
        assert!(ImportStorage::Hybrid.writes_toml_folders());
        assert!(ImportStorage::TomlFolders.writes_toml_folders());
        assert!(!ImportStorage::BinaryDirect.writes_toml_folders());
        // The spec default is BinaryDirect.
        assert_eq!(ImportStorage::default(), ImportStorage::BinaryDirect);
    }

    // ── BinarySink integration (needs a real worlddb handle). ──

    #[cfg(feature = "binary-sink")]
    mod binary_integration {
        use super::*;
        use eustress_common::instance_create::{uuid_bytes_to_hex, uuid_hex_to_bytes};
        use eustress_worlddb::backend::open;
        use eustress_worlddb::decode_instance_core;
        use std::collections::HashMap;

        fn temp_db_dir(tag: &str) -> std::path::PathBuf {
            let p = std::env::temp_dir().join(format!(
                "rbx_binsink_test_{}_{}_{}",
                tag,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).expect("create temp db dir");
            p
        }

        /// A deterministic 32-hex uuid for tests (mirrors identity.rs output
        /// shape — 32 lowercase hex chars).
        fn det_uuid(seed: u8) -> String {
            uuid_bytes_to_hex(&[seed; 16])
        }

        #[test]
        fn binary_sink_writes_a_core_for_a_bare_part() {
            let dir = temp_db_dir("core");
            let db = open(&dir).expect("open temp worlddb");
            let mut sink = BinarySink::new(db.clone(), false);

            let uuid_hex = det_uuid(0xAB);
            let overrides = InstanceOverrides {
                position: Some([10.0, 20.0, 30.0]),
                scale: Some([2.0, 2.0, 2.0]),
                color_rgba: Some([0.5, 0.25, 0.75, 1.0]),
                material: Some("Neon".to_string()),
                anchored: Some(true),
                ..Default::default()
            };
            let extras = HashMap::new();
            let physics = HashMap::new();
            let attributes = HashMap::new();
            let tags = vec!["bench".to_string()];

            let spec = NodeSpec {
                class: ClassName::Part,
                class_template: "Part",
                requested_name: "Cube",
                overrides: &overrides,
                uuid_hex: &uuid_hex,
                extras: &extras,
                physics: &physics,
                attributes: &attributes,
                tags: &tags,
            };

            let dest = dir.join("Workspace");
            std::fs::create_dir_all(&dest).unwrap();
            let written = sink.write(&dest, &spec).expect("binary sink write");

            // No TOML for a binary-ECS write; a core was written.
            assert!(
                written.toml.is_none(),
                "bare part must NOT get a TOML folder"
            );
            assert!(written.wrote_binary_core);

            // The Morton-keyed `entities` partition (the engine boot-load
            // source) now holds exactly one core, and it round-trips.
            let cores = db.iter_instance_cores().expect("iter cores");
            assert_eq!(cores.len(), 1, "exactly one core should have landed");
            let (_id, bytes) = &cores[0];
            let core = decode_instance_core(bytes).expect("decode core");
            assert_eq!(core.class_name, "Part");
            assert_eq!(core.t, [10.0, 20.0, 30.0]);
            assert_eq!(core.s, [2.0, 2.0, 2.0]);
            assert_eq!(core.material, "Neon");
            assert!(core.anchored);
            assert_eq!(core.tags, vec!["bench".to_string()]);

            // The IDENTITY.md UUID index stores resolve it too.
            let uuid_bytes = uuid_hex_to_bytes(&uuid_hex).unwrap();
            let by_uuid = db
                .get_entity_core_by_uuid(&uuid_bytes)
                .expect("get by uuid")
                .expect("uuid row present");
            let core2 = decode_instance_core(&by_uuid).unwrap();
            assert_eq!(core2.class_name, "Part");
            // class_index marks it under "Part".
            let class_uuids = db.iter_class("Part").expect("iter class");
            assert!(class_uuids.contains(&uuid_bytes));

            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn custom_mesh_part_routes_to_toml_not_binary() {
            // §8.A representation rule under a real BinarySink: a custom-mesh
            // Part must FALL BACK to a TOML folder, not a binary core.
            let dir = temp_db_dir("route");
            let db = open(&dir).expect("open temp worlddb");
            let mut sink = BinarySink::new(db.clone(), false);

            let uuid_hex = det_uuid(0xCD);
            let overrides = InstanceOverrides {
                position: Some([1.0, 2.0, 3.0]),
                asset_mesh: Some("../meshes/VCell_Anode.glb".to_string()),
                ..Default::default()
            };
            let empty = HashMap::new();
            let tags: Vec<String> = Vec::new();
            let spec = NodeSpec {
                class: ClassName::Part,
                class_template: "Part",
                requested_name: "VCell",
                overrides: &overrides,
                uuid_hex: &uuid_hex,
                extras: &empty,
                physics: &empty,
                attributes: &empty,
                tags: &tags,
            };

            // The create pipeline needs the class_schema template dir; the
            // routing decision happens BEFORE the TOML write, so assert the
            // predicate directly (the write would need the Part template on
            // disk, which this unit test doesn't stage).
            assert!(
                !node_is_binary_eligible(spec.class, spec.overrides),
                "custom-mesh Part must route to FileSystem (TOML), not binary",
            );

            // And no core leaked into the DB from the routing check.
            let cores = db.iter_instance_cores().expect("iter cores");
            assert!(
                cores.is_empty(),
                "custom-mesh routing must NOT write a binary core"
            );

            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn reimport_is_idempotent_in_the_db() {
            // Same deterministic uuid + same position → the second write
            // overwrites the same Morton + UUID records (no duplicate core).
            let dir = temp_db_dir("idem");
            let db = open(&dir).expect("open temp worlddb");
            let mut sink = BinarySink::new(db.clone(), false);

            let uuid_hex = det_uuid(0xEF);
            let overrides = InstanceOverrides {
                position: Some([5.0, 5.0, 5.0]),
                ..Default::default()
            };
            let empty = HashMap::new();
            let tags: Vec<String> = Vec::new();
            let make_spec = || NodeSpec {
                class: ClassName::Part,
                class_template: "Part",
                requested_name: "Cube",
                overrides: &overrides,
                uuid_hex: &uuid_hex,
                extras: &empty,
                physics: &empty,
                attributes: &empty,
                tags: &tags,
            };
            let dest = dir.join("Workspace");
            std::fs::create_dir_all(&dest).unwrap();
            sink.write(&dest, &make_spec()).expect("first write");
            sink.write(&dest, &make_spec()).expect("re-import write");

            let cores = db.iter_instance_cores().expect("iter cores");
            assert_eq!(cores.len(), 1, "re-import must overwrite, not duplicate");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

//! Walk the Roblox DOM and materialise each instance into Eustress
//! `_instance.toml` files via the canonical
//! [`eustress_common::instance_create::create_instance`] pipeline.
//!
//! Spec ref: `docs/architecture/ROBLOX_IMPORT_SPEC.md` §2 / §15.
//!
//! ## Flow
//!
//! 1. The orchestrator opens a [`Materializer`] with the target Space
//!    root, options, and a fresh [`crate::import_report::ImportReport`].
//! 2. For each child of `DataModel` (each Roblox service):
//!     - Resolve the service folder via [`crate::service_router`].
//!     - Walk the subtree depth-first, calling `walk_subtree` for each
//!       descendant.
//! 3. Each `walk_subtree` call:
//!     - Maps the Roblox class via [`crate::class_map`].
//!     - Maps the properties via [`crate::property_map`].
//!     - Calls `create_instance` with the well-known overrides.
//!     - Post-processes the resulting TOML to layer extras, refs,
//!       tags, attributes, script source.
//!     - Recurses into children.
//! 4. After the full walk, a second pass resolves Roblox `Ref`
//!    properties to Eustress uuids via the in-memory
//!    referent → uuid map and writes the resolved entries under
//!    `[references]`.
//!
//! Terrain + CSG instances are dispatched to dedicated decoders:
//! [`crate::terrain::import_terrain`] decodes the `SmoothGrid` voxel
//! volume into chunk files, and [`crate::csg::import_csg`] extracts each
//! CSG operation's baked `MeshData` into a `csg.glb` asset (AABB-block
//! fallback when no mesh is present). See spec §6 and §7.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eustress_common::classes::ClassName;
use eustress_common::instance_create::{
    create_instance, fresh_uuid_for_create, is_valid_uuid, uuid_bytes_to_hex, InstanceOverrides,
};
use eustress_common::luau::compat::{ScriptTransformer, WarningSeverity};
use rbx_dom_weak::types::Ref;
use rbx_dom_weak::WeakDom;
use uuid::Uuid;

use crate::asset_resolver;
use crate::class_map::roblox_to_eustress_class;
use crate::error::ImportError;
use crate::identity::entity_uuid;
use crate::import_report::ImportReport;
use crate::parser::RobloxDom;
use crate::property_map::{map_properties, PropertyBag};
use crate::service_router::{RouteOutcome, ServiceRouter};
use crate::sink::{ImportSink, ImportStorage, NodeSpec, TomlSink, TomlWrite, WrittenRef};
use crate::value_objects::{
    encode_value_object, is_convertible_value_object, is_value_object_class,
};

// ---------------------------------------------------------------------------
// Special-class classification (Terrain / CSG dispatch)
// ---------------------------------------------------------------------------

/// Roblox classes that need a dedicated decoder rather than the generic
/// class_map + property_map path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpecialKind {
    /// The `Terrain` voxel volume (spec §6).
    Terrain,
    /// A CSG operation (`UnionOperation` / `NegateOperation` /
    /// `IntersectOperation`) carrying a baked mesh (spec §7).
    Csg,
    /// Everything else — handled generically.
    None,
}

impl SpecialKind {
    fn classify(roblox_class: &str) -> Self {
        match roblox_class {
            "Terrain" => SpecialKind::Terrain,
            "UnionOperation" | "NegateOperation" | "IntersectOperation" => SpecialKind::Csg,
            _ => SpecialKind::None,
        }
    }
}

// ---------------------------------------------------------------------------
// ImportOptions — public knobs per spec §15
// ---------------------------------------------------------------------------

/// Knobs that control how a single import call behaves. See spec §15.
#[derive(Clone)]
pub struct ImportOptions {
    /// Service routing rules. Default `ServiceRouter::new(space_root)`
    /// covers every standard Roblox service.
    pub service_router: Option<ServiceRouter>,

    /// Whether to decode SmoothGrid voxel data (§6). Default: true. When
    /// false, a `Terrain` instance is still materialised but its voxel
    /// grid is skipped (recorded as an approximation).
    pub import_terrain: bool,

    /// Whether to extract baked CSG MeshData (§7.1). Default: true. The
    /// baked-mesh path always runs for CSG instances; this flag is
    /// reserved for a future "skip CSG entirely" mode.
    pub extract_csg_baked: bool,

    /// Whether to re-execute CSG from ChildData when MeshData is absent
    /// (§7.2). Default: true. The `truck-shapeops` re-execution path is
    /// currently a stub — when MeshData is absent the importer falls back
    /// to an AABB block. The baked-mesh path covers the ~99% case.
    pub recompute_csg_when_missing: bool,

    /// Whether to invoke `compat::ScriptTransformer` on Luau bodies.
    /// Default: true.
    pub transform_scripts: bool,

    /// Per-Space salt for the UUID derivation (§12). When `None`, the
    /// materializer derives a salt from the space root path so
    /// imports are deterministic across runs against the same Space.
    pub space_salt: Option<Vec<u8>>,

    /// Authoring unit symbol stamped into `metadata.unit`. Defaults to
    /// `"m"` — Eustress is meter-native and Roblox studs map 1:1.
    pub unit_symbol: Option<String>,

    /// Optional asset fetcher (spec §11 / §19.3). `None` (the default)
    /// keeps the no-network behaviour: `rbxassetid://` references land on
    /// the placeholder path. When supplied (e.g. the engine wires a
    /// `ChainFetcher` from `eustress-roblox-assets`), MESH properties
    /// (`MeshId` / `SpecialMesh.Content`) are fetched + decoded into real
    /// `.glb` geometry (Wave F2). Textures / sounds still take the
    /// placeholder path this wave. `Arc<dyn ...>` is `Clone`, so
    /// `ImportOptions` keeps deriving `Clone`.
    pub asset_fetcher: Option<Arc<dyn crate::asset_resolver::AssetFetcher>>,

    /// §8.A: where each node's authoritative state lands. Default
    /// [`ImportStorage::BinaryDirect`] — bare, scalable leaf parts bake
    /// straight to the worlddb `entities` partition; everything else stays
    /// a `_instance.toml` folder. Degrades to `TomlFolders` when the
    /// `binary-sink` feature is off or no [`world_db`](Self::world_db)
    /// handle is supplied.
    pub storage: ImportStorage,

    /// Pre-opened worlddb handle the binary sink writes cores into.
    /// Supplied by the caller (the engine / `eustress-space`) so this
    /// engine-free crate never hard-codes a Fjall path. `None` (the
    /// default) makes `BinaryDirect`/`Hybrid` degrade to TOML folders.
    /// Only present under the `binary-sink` feature.
    #[cfg(feature = "binary-sink")]
    pub world_db: Option<std::sync::Arc<dyn eustress_worlddb::WorldDb>>,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            service_router: None,
            import_terrain: true,
            extract_csg_baked: true,
            recompute_csg_when_missing: true,
            transform_scripts: true,
            space_salt: None,
            unit_symbol: Some("m".to_string()),
            asset_fetcher: None,
            storage: ImportStorage::default(),
            #[cfg(feature = "binary-sink")]
            world_db: None,
        }
    }
}

impl std::fmt::Debug for ImportOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("ImportOptions");
        s.field("service_router", &self.service_router.is_some())
            .field("import_terrain", &self.import_terrain)
            .field("extract_csg_baked", &self.extract_csg_baked)
            .field(
                "recompute_csg_when_missing",
                &self.recompute_csg_when_missing,
            )
            .field("transform_scripts", &self.transform_scripts)
            .field("space_salt", &self.space_salt.as_ref().map(|v| v.len()))
            .field("unit_symbol", &self.unit_symbol)
            .field("asset_fetcher", &self.asset_fetcher.is_some())
            .field("storage", &self.storage);
        #[cfg(feature = "binary-sink")]
        s.field("world_db", &self.world_db.is_some());
        s.finish()
    }
}

// ---------------------------------------------------------------------------
// Materializer — encapsulates one import call's state
// ---------------------------------------------------------------------------

/// Per-import state: walks the DOM once, calling `create_instance` for
/// each materialisable node.
pub struct Materializer<'dom> {
    dom: &'dom WeakDom,
    space_root: PathBuf,
    router: Arc<ServiceRouter>,
    opts: ImportOptions,
    salt: Vec<u8>,

    /// Global ValueObject context for the script rewrite. Built by a
    /// DOM pre-pass in [`Materializer::run`] BEFORE any script body is
    /// transformed: `names` is every converted value-object Name and
    /// `ref_names` is the `ObjectValue` subset. Handed to
    /// `compat::ScriptTransformer::transform_value_objects` so a script
    /// that read `foo.Value` on a now-folded ValueObject is rewritten to
    /// the attribute accessor. (Populated for the whole DOM, not per node,
    /// because a script anywhere may reference a ValueObject anywhere.)
    vo_ctx: eustress_common::luau::compat::ValueObjectContext,

    /// Roblox referent → Eustress uuid map, built during the walk and
    /// used for the second-pass `Ref` resolution.
    referent_to_uuid: HashMap<Ref, Uuid>,

    /// Roblox referent → on-disk space-relative path. Used for
    /// `ImportReport::unresolved_refs` reporting.
    referent_to_path: HashMap<Ref, String>,

    /// Pending `Ref` resolution work: `(host_path, host_property, target_ref)`
    /// to be patched after the walk completes.
    pending_refs: Vec<(PathBuf, String, Ref)>,

    /// Pending `[properties.extras]` / `[references]` / `[metadata.tags]` /
    /// `[properties.attributes]` / `[properties.physics]` / `[asset]` patches
    /// keyed by absolute TOML path. Applied at the end of the walk so we
    /// only touch each file once.
    pending_patches: HashMap<PathBuf, TomlPatch>,

    /// §8.A storage mode for this import.
    storage: ImportStorage,

    /// The always-available TOML sink. File-natured nodes, ref hosts,
    /// parents with children, Terrain/CSG — and the entire import when not
    /// in a binary storage mode — all go through this.
    toml_sink: TomlSink,

    /// The binary-ECS sink. Present only under the `binary-sink` feature
    /// AND when a `world_db` handle was supplied in a binary storage mode
    /// (`BinaryDirect`/`Hybrid`); otherwise binary modes degrade to TOML.
    #[cfg(feature = "binary-sink")]
    binary_sink: Option<crate::sink::BinarySink>,
}

#[derive(Default)]
struct TomlPatch {
    extras: HashMap<String, toml::Value>,
    physics: HashMap<String, toml::Value>,
    attributes: HashMap<String, toml::Value>,
    tags: Vec<String>,
    refs_uuid: HashMap<String, String>,
    refs_unresolved: HashMap<String, String>,
    asset_path: Option<String>,
    asset_mesh: Option<String>,
    uuid_stamp: Option<String>,
    script_body: Option<String>,
    script_class: Option<ClassName>,
}

/// Project rbx_dom_weak 4.x's interned-`Ustr`-keyed `properties` map into a
/// plain `HashMap<String, Variant>` — the shape the property mapper and the
/// terrain/CSG decoders consume. Import-time only; the per-node clone is fine
/// for a one-shot import and keeps the whole mapper engine-string-keyed
/// (no `Ustr` plumbing through `property_map`/`terrain`).
fn props_to_string_map(
    inst: &rbx_dom_weak::Instance,
) -> HashMap<String, rbx_dom_weak::types::Variant> {
    inst.properties
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.clone()))
        .collect()
}

impl<'dom> Materializer<'dom> {
    /// Construct a materializer for a `RobloxDom` + target Space.
    pub fn new(
        dom: &'dom WeakDom,
        space_root: &Path,
        opts: ImportOptions,
    ) -> Result<Self, ImportError> {
        if !space_root.exists() {
            std::fs::create_dir_all(space_root)
                .map_err(|e| ImportError::Io(space_root.to_path_buf(), e))?;
        }
        let router = opts
            .service_router
            .clone()
            .unwrap_or_else(|| ServiceRouter::new(space_root.to_path_buf()));
        let salt = opts
            .space_salt
            .clone()
            .unwrap_or_else(|| derive_space_salt(space_root));

        // Select the per-node sinks from the storage mode. The TOML sink is
        // always available; the binary sink only materialises under the
        // `binary-sink` feature when a worlddb handle was supplied in a
        // binary mode (otherwise BinaryDirect/Hybrid degrade to TOML).
        let storage = opts.storage;
        let toml_sink = TomlSink::new();
        #[cfg(feature = "binary-sink")]
        let binary_sink = if storage.writes_binary() {
            match opts.world_db.clone() {
                Some(db) => Some(crate::sink::BinarySink::new(
                    db,
                    storage == ImportStorage::Hybrid,
                )),
                None => {
                    tracing::warn!(
                        "import storage {:?} requested but no world_db handle supplied — \
                         degrading to TOML folders",
                        storage
                    );
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            dom,
            space_root: space_root.to_path_buf(),
            router: Arc::new(router),
            opts,
            salt,
            vo_ctx: eustress_common::luau::compat::ValueObjectContext::default(),
            referent_to_uuid: HashMap::new(),
            referent_to_path: HashMap::new(),
            pending_refs: Vec::new(),
            pending_patches: HashMap::new(),
            storage,
            toml_sink,
            #[cfg(feature = "binary-sink")]
            binary_sink,
        })
    }

    /// Walk the entire DOM, populating `report` as we go.
    pub fn run(mut self, report: &mut ImportReport) -> Result<(), ImportError> {
        let start = std::time::Instant::now();

        // ── ValueObject script-rewrite pre-pass ──
        //
        // Build the global ValueObject context BEFORE any script body is
        // transformed (script transformation happens inside the walk
        // below). A script anywhere in the DOM may reference a ValueObject
        // anywhere, so the context is whole-DOM, not per-subtree. `names`
        // holds every CONVERTED value-object Name (dropped classes are not
        // converted, so they are excluded); `ref_names` is the `ObjectValue`
        // subset (those fold to a UUID string the script side resolves via
        // import context).
        for inst in self.dom.descendants() {
            let class = inst.class.as_str();
            if !is_convertible_value_object(class) {
                continue;
            }
            let name = if inst.name.is_empty() {
                class.to_string()
            } else {
                inst.name.clone()
            };
            if class == "ObjectValue" {
                self.vo_ctx.ref_names.insert(name.clone());
            }
            self.vo_ctx.names.insert(name);
        }

        let root_ref = self.dom.root_ref();
        let root = self
            .dom
            .get_by_ref(root_ref)
            .expect("WeakDom root should always be present");
        // Roblox places have a `DataModel` root; model files have an
        // arbitrary class as root. For model files we treat the root
        // itself as content rooted under `Workspace/` so the import
        // still makes sense.
        if root.class == "DataModel" {
            for child_ref in root.children().iter() {
                self.handle_service(*child_ref, report)?;
            }
        } else {
            let dest = self.space_root.join("Workspace");
            // Model-file root: its parent is the synthetic `Workspace`
            // service dir (no Roblox referent) → no parent UUID.
            self.walk_subtree(root_ref, &dest, "Workspace", None, report)?;
        }

        self.finalise_pending_patches()?;
        self.finalise_refs(report)?;

        report.elapsed = start.elapsed();
        Ok(())
    }

    fn handle_service(
        &mut self,
        service_ref: Ref,
        report: &mut ImportReport,
    ) -> Result<(), ImportError> {
        let Some(service) = self.dom.get_by_ref(service_ref) else {
            return Ok(());
        };
        report.total_nodes_seen += 1;

        // StarterPlayer needs its two children split out — its scripts
        // live at top-level Eustress folders, not under StarterPlayer.
        if service.class == "StarterPlayer" {
            return self.handle_starter_player(service_ref, report);
        }

        let service_class = service.class.as_str().to_string();
        let outcome = self.router.route(&service_class)?;
        match outcome {
            RouteOutcome::Routed { dest, cognate } => {
                if !cognate {
                    report.record_skipped_service(
                        &service.class,
                        &format!(
                            "no Eustress cognate — children routed to {}",
                            dest.display()
                        ),
                    );
                }
                let absolute_dest = self.router.absolute(&dest);
                std::fs::create_dir_all(&absolute_dest)
                    .map_err(|e| ImportError::Io(absolute_dest.clone(), e))?;
                let dest_str = dest.to_string_lossy().to_string();

                // ── Workspace container folder (place-scoped subtree) ──
                //
                // The whole imported place should live under ONE removable
                // subtree in-Workspace: `Workspace/<PlaceName>/...` rather than
                // ~25 loose top-level Workspace folders. We synthesise a plain
                // TOML `Folder` named after the place directly under the
                // `Workspace` service and re-root the Workspace children beneath
                // it. This applies ONLY to `Workspace` — every other service
                // (Lighting / Players / ReplicatedStorage / …) keeps walking at
                // its normal cognate root.
                //
                // The container is a fully canonical `create_instance` folder
                // (its own random uuid, valid `_instance.toml`); it carries no
                // Roblox referent, so it never participates in
                // `referent_to_uuid` / `[references]` and cannot collide with
                // any imported node's identity. Its children keep their existing
                // storage routing (`take_binary` / `node_is_binary_eligible`)
                // and ref/asset post-passes unchanged — they merely sit one
                // directory deeper on disk, which every downstream path is
                // computed relative to (`create_instance(dest_dir, …)` →
                // `folder_path`), so terrain/CSG dispatch and ref resolution are
                // unaffected.
                let (child_dir, child_relpath) = if cognate && service_class == "Workspace" {
                    match self.workspace_container(&absolute_dest, &dest_str, report)? {
                        Some(container) => container,
                        // No derivable place name (e.g. an empty/`..` space root
                        // in a unit test) — fall back to the legacy flat layout.
                        None => (absolute_dest.clone(), dest_str.clone()),
                    }
                } else {
                    (absolute_dest.clone(), dest_str.clone())
                };

                for child_ref in service.children().iter() {
                    // A service child's parent is the service-cognate folder
                    // (or the synthetic `Workspace/<PlaceName>` container) —
                    // neither carries a Roblox referent → no parent UUID.
                    self.walk_subtree(*child_ref, &child_dir, &child_relpath, None, report)?;
                }
            }
            RouteOutcome::SkipSilent => {
                // Runtime-only — silently skip subtree.
            }
        }
        Ok(())
    }

    /// Materialise the synthetic `Workspace/<PlaceName>/` container Folder and
    /// return `(absolute_folder_path, space_relative_path)` for the Workspace
    /// children to be walked into. `<PlaceName>` is derived from the target
    /// Space root's directory name (the import targets a fresh Space named
    /// after the source file, so its dir name IS the place name).
    ///
    /// Returns `Ok(None)` when no place name can be derived from the space root
    /// (an empty or non-final-component path) so the caller can fall back to
    /// the flat `Workspace/...` layout. A failed container write is recorded as
    /// an approximation and also falls back to flat — never aborts the import.
    fn workspace_container(
        &mut self,
        workspace_dir: &Path,
        workspace_relpath: &str,
        report: &mut ImportReport,
    ) -> Result<Option<(PathBuf, String)>, ImportError> {
        let Some(place_name) = place_name_from_root(&self.space_root) else {
            return Ok(None);
        };

        // A plain Folder via the canonical pipeline: real `_instance.toml`,
        // unique-safed folder name, its own (random) uuid. No overrides beyond
        // the display name + the import's unit symbol so it reads identically to
        // any other imported Folder.
        let mut overrides = InstanceOverrides {
            display_name: Some(place_name.clone()),
            ..Default::default()
        };
        if let Some(unit) = &self.opts.unit_symbol {
            overrides.unit_symbol = Some(unit.clone());
        }

        match create_instance(
            workspace_dir,
            ClassName::Folder.as_str(),
            Some(&place_name),
            overrides,
        ) {
            Ok(created) => {
                let rel = format!("{}/{}", workspace_relpath, created.folder_name);
                Ok(Some((created.folder_path, rel)))
            }
            Err(e) => {
                // Container creation failed — record it and fall back to the
                // flat Workspace layout so the place still imports.
                report.record_approximation(
                    workspace_relpath,
                    "Folder",
                    "Folder",
                    &format!(
                        "Workspace container '{}' could not be created ({e}); \
                         children placed directly under Workspace",
                        place_name
                    ),
                );
                Ok(None)
            }
        }
    }

    fn handle_starter_player(
        &mut self,
        service_ref: Ref,
        report: &mut ImportReport,
    ) -> Result<(), ImportError> {
        let Some(service) = self.dom.get_by_ref(service_ref) else {
            return Ok(());
        };
        for child_ref in service.children().iter() {
            let Some(child) = self.dom.get_by_ref(*child_ref) else {
                continue;
            };
            let dest_rel = match child.class.as_str() {
                "StarterPlayerScripts" => "StarterPlayerScripts",
                "StarterCharacterScripts" => "StarterCharacterScripts",
                _ => {
                    // Other children of StarterPlayer have nowhere
                    // sensible to land — route to _imported.
                    "_imported/StarterPlayer"
                }
            };
            let dest = self.router.absolute(Path::new(dest_rel));
            std::fs::create_dir_all(&dest).map_err(|e| ImportError::Io(dest.clone(), e))?;
            for gc_ref in child.children().iter() {
                // Parent is the synthetic StarterPlayerScripts /
                // StarterCharacterScripts folder (no Roblox referent) → no
                // parent UUID.
                self.walk_subtree(*gc_ref, &dest, dest_rel, None, report)?;
            }
        }
        Ok(())
    }

    fn walk_subtree(
        &mut self,
        node_ref: Ref,
        parent_dir: &Path,
        parent_relpath: &str,
        // Deterministic 32-hex UUID of this node's PARENT instance, threaded
        // down the recursion so a binary core can store `__parent_uuid` in
        // its cold tail (Defect-2 hierarchy preservation). `None` at the
        // top of each service subtree, where the parent is a synthetic
        // non-instance container (service-cognate folder / Workspace
        // container / StarterPlayerScripts) that carries no Roblox referent.
        parent_uuid: Option<&str>,
        report: &mut ImportReport,
    ) -> Result<(), ImportError> {
        let Some(inst) = self.dom.get_by_ref(node_ref) else {
            return Ok(());
        };
        report.total_nodes_seen += 1;

        // Defence-in-depth — we must never write under Eustress-only
        // folders even if the router somehow yielded one.
        if self.router.is_off_limits(parent_dir) {
            return Err(ImportError::OffLimits(parent_dir.to_path_buf()));
        }

        // Skip Plugin classes entirely per spec §1.
        if inst.class == "Plugin" {
            report.record_unmapped_class(&inst.class, &inst.name);
            return Ok(());
        }

        // Classify Terrain / CSG up-front: we need it both to route CSG
        // operands that have no dedicated ClassName to a Part (below) and
        // to dispatch to the dedicated decoders (terrain.rs / csg.rs)
        // after the instance is created.
        let special = SpecialKind::classify(inst.class.as_str());

        // Map the Roblox class to a Eustress ClassName. CSG operations
        // (`NegateOperation` / `IntersectOperation`) have no dedicated
        // enum variant — per spec §7 they legacy-route to a Part here so
        // the CSG dispatcher can swap in the baked mesh (or fall back to
        // an AABB block when no MeshData is present).
        let eustress_class = match roblox_to_eustress_class(inst.class.as_str()) {
            Some(c) => c,
            None if special == SpecialKind::Csg => ClassName::Part,
            None => {
                report.record_unmapped_class(&inst.class, &inst.name);
                return Ok(());
            }
        };

        // Map properties. rbx_dom_weak 4.x keys `properties` by interned
        // `Ustr`; project to the `HashMap<String, Variant>` the mapper expects.
        let string_props = props_to_string_map(inst);
        let mut bag = map_properties(&string_props, eustress_class);

        // Choose the on-disk folder name. `inst.class` is a `Ustr` in
        // rbx_dom_weak 4.x; project to String to match `inst.name`.
        let requested_name = if inst.name.is_empty() {
            inst.class.as_str().to_string()
        } else {
            inst.name.clone()
        };

        // ── ValueObject → parent attribute folding (deprecation Phase 1) ──
        //
        // Roblox stores loose scalars/vectors/refs as dedicated
        // *ValueObject* children (`IntValue`, `BoolValue`, `ObjectValue`, …).
        // Eustress has no such class — the idiomatic shape is a typed
        // attribute on THIS node. For every child whose class
        // `is_value_object_class`, encode it (Contract A) into
        // `bag.attributes` keyed by the child's Name, then record the child
        // ref so the recursion below SKIPS it (it must not materialise as
        // its own instance). Dropped classes (`RayValue` /
        // `*ConstrainedValue`) are still folded out of the tree but record
        // an approximation instead of converting.
        let mut folded_children: std::collections::HashSet<Ref> = std::collections::HashSet::new();
        for child_ref in inst.children().iter() {
            let Some(child) = self.dom.get_by_ref(*child_ref) else {
                continue;
            };
            if !is_value_object_class(child.class.as_str()) {
                continue;
            }
            // From here on the child is folded out of the instance tree
            // regardless of whether it converts.
            folded_children.insert(*child_ref);
            report.total_nodes_seen += 1;

            let salt = &self.salt;
            let encoded = encode_value_object(child.class.as_str(), &child.properties, |target| {
                if target.is_none() {
                    return None;
                }
                Some(uuid_bytes_to_hex(
                    entity_uuid(salt, &target.to_string()).as_bytes(),
                ))
            });

            let attr_name = if child.name.is_empty() {
                child.class.as_str().to_string()
            } else {
                child.name.clone()
            };

            match encoded {
                Some(value) => {
                    // Duplicate attribute key under one parent → suffix
                    // `_2`/`_3`/… and note it. (Roblox allows sibling
                    // ValueObjects with identical names; attributes cannot.)
                    let key = unique_attribute_key(&bag.attributes, &attr_name);
                    if key != attr_name {
                        report.record_approximation(
                            parent_relpath,
                            child.class.as_str(),
                            "attribute",
                            &format!(
                                "duplicate folded attribute '{}' on '{}' renamed to '{}'",
                                attr_name, requested_name, key
                            ),
                        );
                    }
                    bag.attributes.insert(key, value);
                }
                None => {
                    // Dropped ValueObject (RayValue / *ConstrainedValue):
                    // folded out of the tree, but not representable as an
                    // attribute — record the drop per the product decision.
                    report.record_approximation(
                        parent_relpath,
                        child.class.as_str(),
                        "attribute",
                        &format!(
                            "ValueObject class dropped (not convertible) — \
                             '{}' under '{}' not imported",
                            attr_name, requested_name
                        ),
                    );
                }
            }
        }

        // Build the per-node spec + route it through the selected sink.
        // Bare, scalable leaf parts bake straight to a binary-ECS core
        // (BinaryDirect / Hybrid); everything else — file-natured nodes,
        // ref hosts, parents with children, Terrain / CSG — lands as a
        // `_instance.toml` folder so the existing second-pass machinery
        // (patches, recursion, decoders) applies unchanged.
        let class_template_name = eustress_class.as_str();
        let mut overrides = bag.overrides.clone();
        overrides.display_name = Some(requested_name.clone());
        if let Some(unit) = &self.opts.unit_symbol {
            overrides.unit_symbol = Some(unit.clone());
        }

        // Deterministic uuid (overrides the random one the canonical
        // pipeline would mint) so re-imports are idempotent. Computed up
        // front so a binary sink can key its records on it.
        let referent = inst.referent();
        let uuid = entity_uuid(&self.salt, &referent.to_string());
        let uuid_hex = uuid_bytes_to_hex(uuid.as_bytes());
        self.referent_to_uuid.insert(referent, uuid);

        // A node may take the binary fast-path only if it is a bare leaf:
        // no dedicated decoder (Terrain / CSG), no children to recurse
        // into, no `Ref` host-patching, and no script body. The sink itself
        // re-applies the representation predicate (file-natured /
        // custom-mesh parts fall back to TOML internally), so this gate is
        // only the structural part the sink cannot see.
        //
        // Wave F2: a node that carries a MESH asset ref (`MeshId` /
        // `SpecialMesh.Content`) must ALSO stay a `_instance.toml` folder.
        // The resolved `[asset].mesh` (a real `../assets/meshes/rbx-*.glb`
        // or the `assets/_unresolved/...` placeholder) is layered on AFTER
        // the node write, so it has to be a TOML the post-pass can patch —
        // and a custom/relative mesh is never binary-eligible anyway
        // (`mesh_requires_filesystem`). Without this, a bare `MeshPart`
        // would bake to a binary core and its mesh ref would be dropped on
        // the floor (the binary core has no TOML to patch).
        let has_mesh_asset_ref = bag
            .asset_refs
            .keys()
            .any(|prop| is_mesh_property(prop, eustress_class));
        // A parent that RECEIVED folded ValueObject attributes must stay a
        // `_instance.toml` folder: the attributes are layered onto the TOML
        // by the second-pass patch (`bag.attributes` → `[attributes]`), and
        // a bare binary-ECS core has no TOML to patch. Mirrors the
        // `has_mesh_asset_ref` term above.
        let has_folded_value_object_children = !folded_children.is_empty();
        let take_binary = special == SpecialKind::None
            && inst.children().is_empty()
            && bag.refs.is_empty()
            && bag.script_source.is_none()
            && !has_mesh_asset_ref
            && !has_folded_value_object_children;

        let written = {
            let spec = NodeSpec {
                class: eustress_class,
                class_template: class_template_name,
                requested_name: &requested_name,
                overrides: &overrides,
                uuid_hex: &uuid_hex,
                parent_uuid_hex: parent_uuid,
                extras: &bag.properties_extras,
                physics: &bag.physics_extras,
                attributes: &bag.attributes,
                tags: &bag.tags,
            };
            match self.write_node(parent_dir, take_binary, &spec) {
                Ok(w) => w,
                Err(e) => {
                    // Never abort the whole import for one node. Record + skip
                    // it (and its subtree — its folder was never created, so
                    // its children have nowhere to land) and continue the walk.
                    report.record_approximation(
                        parent_relpath,
                        inst.class.as_str(),
                        class_template_name,
                        &format!("node skipped — materialize failed: {e}"),
                    );
                    return Ok(());
                }
            }
        };

        if written.wrote_binary_core {
            report.binary_cores_written += 1;
        }

        // No TOML folder ⇒ a pure binary-ECS core: the sink absorbed the
        // whole node (hot fields + attributes / extras / physics / tags +
        // the UUID index), there is nothing on disk to patch, and (by the
        // `take_binary` gate) no children to recurse into.
        let created = match written.toml {
            Some(toml_write) => toml_write,
            None => {
                report.record_imported(class_template_name);
                // Mirror BinarySink's synthetic binary-ECS path shape so a
                // later `find_entity --path` agrees with the loader.
                let stored_id = u64::from_be_bytes(uuid.as_bytes()[..8].try_into().unwrap());
                let synthetic = format!(
                    "Workspace/__bin_{}_{:016x}/_instance.toml",
                    class_template_name, stored_id
                );
                self.referent_to_path.insert(referent, synthetic);
                return Ok(());
            }
        };

        let entity_relpath = format!("{}/{}", parent_relpath, created.folder_name);
        report.record_imported(class_template_name);

        if created.folder_name != requested_name {
            report.record_name_collision(parent_relpath, &requested_name, &created.folder_name);
        }

        // Stamp the deterministic uuid onto this TOML's pending patch.
        let patch = self
            .pending_patches
            .entry(created.toml_path.clone())
            .or_default();
        patch.uuid_stamp = Some(uuid_hex.clone());

        // Save the referent → path mapping so refs resolve / report.
        self.referent_to_path
            .insert(referent, entity_relpath.clone());

        // Layer extras + refs + tags + attributes + physics + asset
        // refs + script source onto the pending patch for this TOML.
        for (k, v) in bag.properties_extras {
            patch.extras.insert(k, v);
        }
        for (k, v) in bag.physics_extras {
            patch.physics.insert(k, v);
        }
        for (k, v) in bag.attributes {
            patch.attributes.insert(k, v);
        }
        for t in bag.tags {
            patch.tags.push(t);
        }
        for (prop, target_ref) in bag.refs {
            self.pending_refs
                .push((created.toml_path.clone(), prop, target_ref));
        }
        // Asset refs → resolver. For MESH properties with a fetcher
        // (Wave F2) the resolver fetches + decodes the Roblox `.mesh` into
        // a real `.glb` under `<space>/assets/meshes/` and returns a path
        // relative to this instance's folder; everything else (textures /
        // sounds, or any fetch/decode failure) stays on the placeholder
        // path with a warning.
        for (prop, uri) in bag.asset_refs {
            let prop_is_mesh = is_mesh_property(&prop, eustress_class);
            let resolved = asset_resolver::resolve(
                &uri,
                self.opts.asset_fetcher.as_deref(),
                &self.space_root,
                prop_is_mesh,
                &created.folder_path,
            );
            if !resolved.resolved {
                if let Some(reason) = &resolved.reason {
                    report.record_asset_warning(&uri, class_template_name, &prop, reason);
                }
            }
            // Mesh-class properties point at mesh assets; everything
            // else lands as a single-path asset reference.
            let asset_path_str = resolved.asset_path.to_string_lossy().to_string();
            if prop_is_mesh {
                patch.asset_mesh = Some(asset_path_str);
            } else {
                patch.asset_path = Some(asset_path_str);
            }
        }

        // Script-source post-processing.
        if let Some(body) = bag.script_source {
            let final_body = if self.opts.transform_scripts {
                // Phase 1: route through the ValueObject-aware transform so a
                // script that read `someValue.Value` (on a now-folded
                // ValueObject) is rewritten to the attribute accessor. The
                // global `vo_ctx` was built by the DOM pre-pass in `run`.
                let result =
                    ScriptTransformer::transform_value_objects(&body, &self.vo_ctx);
                for warning in &result.warnings {
                    let severity = match warning.severity {
                        WarningSeverity::Info => "info",
                        WarningSeverity::Warning => "warning",
                        WarningSeverity::Error => "error",
                    };
                    report
                        .script_warnings
                        .push(crate::import_report::ScriptWarning {
                            entity_path: entity_relpath.clone(),
                            message: warning.message.clone(),
                            severity: severity.to_string(),
                        });
                }
                result.source
            } else {
                body
            };
            patch.script_body = Some(final_body);
            patch.script_class = Some(eustress_class);
        }

        // ── Terrain + CSG: dispatch to the dedicated decoders. ──
        match special {
            SpecialKind::Terrain if self.opts.import_terrain => {
                self.import_terrain_instance(inst, &created, report)?;
            }
            SpecialKind::Terrain => {
                // Terrain decode disabled by options — note it.
                report.record_approximation(
                    &entity_relpath,
                    &inst.class,
                    "Terrain",
                    "terrain voxel import disabled via ImportOptions",
                );
            }
            SpecialKind::Csg => {
                self.import_csg_instance(inst, &created, &entity_relpath, report)?;
            }
            SpecialKind::None => {}
        }

        // Event/function counter for the spec §8 metric.
        if matches!(
            eustress_class,
            ClassName::RemoteEvent
                | ClassName::RemoteFunction
                | ClassName::BindableEvent
                | ClassName::BindableFunction
        ) {
            report.events_imported += 1;
        }

        // Recurse into children — but SKIP any child folded into this
        // node's `[attributes]` above (a folded ValueObject must not also
        // materialise as its own instance).
        for child_ref in inst.children().iter() {
            if folded_children.contains(child_ref) {
                continue;
            }
            // This node is the children's parent — pass its deterministic
            // UUID down so a child that bakes to a binary core records the
            // hierarchy edge (`__parent_uuid`). (TOML children ignore it;
            // their parent is already implied by their on-disk folder.)
            self.walk_subtree(
                *child_ref,
                &created.folder_path,
                &entity_relpath,
                Some(&uuid_hex),
                report,
            )?;
        }

        Ok(())
    }

    /// Route one node to the selected sink: the binary-ECS sink when the
    /// node took the structural fast-path AND a binary storage mode + a
    /// worlddb handle are active, else the always-available TOML sink.
    /// Returns the sink's [`WrittenRef`] (the caller branches on `toml`).
    fn write_node(
        &mut self,
        dest_dir: &Path,
        take_binary: bool,
        spec: &NodeSpec<'_>,
    ) -> Result<WrittenRef, ImportError> {
        #[cfg(feature = "binary-sink")]
        {
            if take_binary && self.storage.writes_binary() {
                if let Some(bs) = self.binary_sink.as_mut() {
                    return bs.write(dest_dir, spec);
                }
            }
        }
        #[cfg(not(feature = "binary-sink"))]
        {
            let _ = take_binary;
        }
        self.toml_sink.write(dest_dir, spec)
    }

    /// Decode a `Terrain` instance's `SmoothGrid` into voxel chunk files
    /// + patch the Terrain TOML with `[material_colors]` and globals.
    /// Spec §6.
    fn import_terrain_instance(
        &mut self,
        inst: &rbx_dom_weak::Instance,
        created: &TomlWrite,
        report: &mut ImportReport,
    ) -> Result<(), ImportError> {
        let props = props_to_string_map(inst);
        let smooth_grid = crate::terrain::binary_string_bytes(&props, "SmoothGrid");
        let material_colors = crate::terrain::material_colors(&props);
        let globals = crate::terrain::collect_globals(&props);

        // Empty terrain (no SmoothGrid) → nothing to decode. Still patch
        // the TOML so the material_colors + globals survive.
        let grid = smooth_grid.unwrap_or(&[]);
        crate::terrain::import_terrain(
            &created.folder_path,
            grid,
            material_colors,
            &globals,
            report,
        )
        .map_err(|e| ImportError::Io(created.folder_path.clone(), e))?;
        Ok(())
    }

    /// Extract a CSG instance's baked `MeshData` → `csg.glb` and point the
    /// `Part` at it (or fall back to an AABB block). Spec §7.
    fn import_csg_instance(
        &mut self,
        inst: &rbx_dom_weak::Instance,
        created: &TomlWrite,
        entity_relpath: &str,
        report: &mut ImportReport,
    ) -> Result<(), ImportError> {
        let props = &inst.properties;
        // MeshData may be a BinaryString or a (deduplicated) SharedString.
        let mesh_data: Option<Vec<u8>> = match props.get(&rbx_dom_weak::ustr("MeshData")) {
            Some(rbx_dom_weak::types::Variant::BinaryString(bs)) => {
                Some(AsRef::<[u8]>::as_ref(bs).to_vec())
            }
            Some(rbx_dom_weak::types::Variant::SharedString(ss)) => Some(ss.data().to_vec()),
            _ => None,
        };

        // AABB fallback size from the source Part.Size (Vector3), else 4³.
        let size = match props.get(&rbx_dom_weak::ustr("Size")) {
            Some(rbx_dom_weak::types::Variant::Vector3(v)) => [v.x, v.y, v.z],
            _ => [4.0, 4.0, 4.0],
        };

        let outcome = crate::csg::import_csg(&created.folder_path, mesh_data.as_deref(), size)
            .map_err(|e| ImportError::Io(created.folder_path.clone(), e))?;

        // Point the Part at csg.glb + record the CSG op + count.
        let csg_op = match inst.class.as_str() {
            "UnionOperation" => "union",
            "NegateOperation" => "negate",
            "IntersectOperation" => "intersect",
            _ => "union",
        };
        let patch = self
            .pending_patches
            .entry(created.toml_path.clone())
            .or_default();
        match &outcome {
            crate::csg::CsgOutcome::Baked {
                mesh_file,
                triangles,
            } => {
                patch.asset_mesh = Some(mesh_file.clone());
                patch.extras.insert(
                    "csg_op".to_string(),
                    toml::Value::String(csg_op.to_string()),
                );
                patch.extras.insert(
                    "csg_triangles".to_string(),
                    toml::Value::Integer(*triangles as i64),
                );
                report.csg_baked_extracted += 1;
            }
            crate::csg::CsgOutcome::Aabb { mesh_file, reason } => {
                patch.asset_mesh = Some(mesh_file.clone());
                patch.extras.insert(
                    "csg_op".to_string(),
                    toml::Value::String(csg_op.to_string()),
                );
                report.csg_fallback_aabb += 1;
                report.record_approximation(
                    entity_relpath,
                    &inst.class,
                    "Part",
                    &format!("CSG AABB fallback: {reason}"),
                );
            }
        }
        Ok(())
    }

    fn finalise_pending_patches(&mut self) -> Result<(), ImportError> {
        let patches = std::mem::take(&mut self.pending_patches);
        for (toml_path, patch) in patches {
            apply_toml_patch(&toml_path, &patch)?;
        }
        Ok(())
    }

    fn finalise_refs(&mut self, report: &mut ImportReport) -> Result<(), ImportError> {
        let pending = std::mem::take(&mut self.pending_refs);
        // Group resolved + unresolved per TOML path so we only re-open
        // each file once.
        let mut grouped: HashMap<PathBuf, (HashMap<String, String>, HashMap<String, String>)> =
            HashMap::new();
        for (host_toml, prop, target) in pending {
            if target.is_none() {
                continue;
            }
            let entry = grouped.entry(host_toml.clone()).or_default();
            if let Some(uuid) = self.referent_to_uuid.get(&target) {
                entry.0.insert(prop, uuid_bytes_to_hex(uuid.as_bytes()));
            } else {
                let target_ref_str = target.to_string();
                let host_path = host_toml
                    .strip_prefix(&self.space_root)
                    .unwrap_or(host_toml.as_path())
                    .to_string_lossy()
                    .to_string();
                report.record_unresolved_ref(&host_path, &prop, &target_ref_str);
                entry.1.insert(prop, target_ref_str);
            }
        }
        for (toml_path, (resolved, unresolved)) in grouped {
            let patch = TomlPatch {
                refs_uuid: resolved,
                refs_unresolved: unresolved,
                ..Default::default()
            };
            apply_toml_patch(&toml_path, &patch)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TOML patching
// ---------------------------------------------------------------------------

fn apply_toml_patch(toml_path: &Path, patch: &TomlPatch) -> Result<(), ImportError> {
    let raw = std::fs::read_to_string(toml_path)
        .map_err(|e| ImportError::Io(toml_path.to_path_buf(), e))?;
    let mut doc: toml::Value =
        raw.parse()
            .map_err(|e: toml::de::Error| ImportError::InstanceCreate {
                class: toml_path.to_string_lossy().to_string(),
                source_msg: format!("toml parse: {e}"),
            })?;

    let root = match doc.as_table_mut() {
        Some(t) => t,
        None => {
            return Ok(());
        }
    };

    // ── UUID stamp (deterministic overwrite) ──
    if let Some(stamp) = &patch.uuid_stamp {
        if is_valid_uuid(stamp) {
            let meta = root
                .entry("metadata".to_string())
                .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
            if let Some(t) = meta.as_table_mut() {
                t.insert("uuid".to_string(), toml::Value::String(stamp.clone()));
            }
        } else {
            // Fallback — should never happen since `entity_uuid` always
            // produces 32 hex chars.
            let _ = fresh_uuid_for_create(); // unused
        }
    }

    // ── Tags ──
    if !patch.tags.is_empty() {
        let meta = root
            .entry("metadata".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(t) = meta.as_table_mut() {
            let mut tags_array: Vec<toml::Value> = patch
                .tags
                .iter()
                .map(|s| toml::Value::String(s.clone()))
                .collect();
            if let Some(existing) = t.get("tags").and_then(|v| v.as_array()) {
                for v in existing {
                    if let Some(s) = v.as_str() {
                        tags_array.push(toml::Value::String(s.to_string()));
                    }
                }
            }
            t.insert("tags".to_string(), toml::Value::Array(tags_array));
        }
    }

    // ── Properties extras / physics / attributes ──
    if !patch.extras.is_empty() || !patch.physics.is_empty() || !patch.attributes.is_empty() {
        let props = root
            .entry("properties".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(p) = props.as_table_mut() {
            if !patch.extras.is_empty() {
                let extras = p
                    .entry("extras".to_string())
                    .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
                if let Some(t) = extras.as_table_mut() {
                    for (k, v) in &patch.extras {
                        t.insert(k.clone(), v.clone());
                    }
                }
            }
            if !patch.physics.is_empty() {
                let phys = p
                    .entry("physics".to_string())
                    .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
                if let Some(t) = phys.as_table_mut() {
                    for (k, v) in &patch.physics {
                        t.insert(k.clone(), v.clone());
                    }
                }
            }
            if !patch.attributes.is_empty() {
                let attrs = p
                    .entry("attributes".to_string())
                    .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
                if let Some(t) = attrs.as_table_mut() {
                    for (k, v) in &patch.attributes {
                        t.insert(k.clone(), v.clone());
                    }
                }
            }
        }
    }

    // ── References ──
    if !patch.refs_uuid.is_empty() || !patch.refs_unresolved.is_empty() {
        let refs = root
            .entry("references".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(t) = refs.as_table_mut() {
            for (k, v) in &patch.refs_uuid {
                t.insert(k.clone(), toml::Value::String(v.clone()));
            }
            if !patch.refs_unresolved.is_empty() {
                let unresolved = t
                    .entry("_unresolved".to_string())
                    .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
                if let Some(ut) = unresolved.as_table_mut() {
                    for (k, v) in &patch.refs_unresolved {
                        ut.insert(k.clone(), toml::Value::String(v.clone()));
                    }
                }
            }
        }
    }

    // ── Asset section ──
    if patch.asset_mesh.is_some() || patch.asset_path.is_some() {
        let asset = root
            .entry("asset".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(t) = asset.as_table_mut() {
            if let Some(mesh) = &patch.asset_mesh {
                t.insert("mesh".to_string(), toml::Value::String(mesh.clone()));
                t.entry("scene".to_string())
                    .or_insert_with(|| toml::Value::String("Scene0".to_string()));
            }
            if let Some(path) = &patch.asset_path {
                t.insert("path".to_string(), toml::Value::String(path.clone()));
            }
        }
    }

    let new_raw = toml::to_string_pretty(&doc).unwrap_or(raw);
    std::fs::write(toml_path, new_raw).map_err(|e| ImportError::Io(toml_path.to_path_buf(), e))?;

    // ── Script source — written as a sibling file. ──
    if let (Some(body), Some(class)) = (&patch.script_body, &patch.script_class) {
        let script_name = match class {
            ClassName::LuauScript | ClassName::LuauLocalScript | ClassName::LuauModuleScript => {
                "script.luau"
            }
            ClassName::SoulScript => "soul.md",
            _ => "script.luau",
        };
        let parent = toml_path
            .parent()
            .expect("toml path always has a parent dir");
        let script_path = parent.join(script_name);
        std::fs::write(&script_path, body).map_err(|e| ImportError::Io(script_path, e))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Derive a per-Space salt from the Space root path. Stable across
/// runs against the same Space, different across Spaces.
pub(crate) fn derive_space_salt(space_root: &Path) -> Vec<u8> {
    let canonical = std::fs::canonicalize(space_root).unwrap_or_else(|_| space_root.to_path_buf());
    let s = canonical.to_string_lossy().to_string();
    s.into_bytes()
}

/// The `<PlaceName>` for the in-Workspace container folder, derived from the
/// target Space root's final path component. The import targets a fresh Space
/// named after the source file, so the Space directory name IS the place name
/// — we deliberately do NOT add a field to [`ImportOptions`] for it.
///
/// Returns `None` when the root has no final component (an empty path, or a
/// path ending in `..`/`/`); the caller then keeps the legacy flat
/// `Workspace/...` layout.
pub(crate) fn place_name_from_root(space_root: &Path) -> Option<String> {
    space_root
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// True when the Roblox property maps to a mesh asset (vs. a single
/// `[asset].path`). Used to pick between `asset_mesh` and `asset_path`.
fn is_mesh_property(roblox_prop: &str, class: ClassName) -> bool {
    matches!(roblox_prop, "MeshId" | "CollisionMeshId")
        || (roblox_prop == "Content" && matches!(class, ClassName::SpecialMesh))
}

/// Pick a unique key for a folded ValueObject attribute. Roblox permits
/// sibling ValueObjects with identical names, but a parent's `[attributes]`
/// table is a map — collisions would overwrite. When `desired` is already
/// present we suffix `_2`, `_3`, … until free, matching the disk-folder
/// `unique_entity_name` convention.
fn unique_attribute_key(
    existing: &HashMap<String, toml::Value>,
    desired: &str,
) -> String {
    if !existing.contains_key(desired) {
        return desired.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{}_{}", desired, n);
        if !existing.contains_key(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

// ---------------------------------------------------------------------------
// Top-level entry point
// ---------------------------------------------------------------------------

/// Walk the DOM, materialise every instance into the Space, and return
/// the populated [`ImportReport`].
///
/// This is the canonical entry point referenced by the spec §15.
pub fn import_into_space(
    dom: &RobloxDom,
    space_root: &Path,
    options: ImportOptions,
) -> Result<ImportReport, ImportError> {
    let mut report = ImportReport {
        source_path: dom.source_path.clone(),
        format: dom.format,
        ..Default::default()
    };
    let materializer = Materializer::new(dom.dom(), space_root, options)?;
    materializer.run(&mut report)?;
    Ok(report)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom_weak::types::{Color3, Variant, Vector3};
    use rbx_dom_weak::InstanceBuilder;

    fn make_temp_root(prefix: &str) -> PathBuf {
        let stem = format!(
            "rbx_import_test_{}_{}_{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let p = std::env::temp_dir().join(stem);
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create temp Space root");
        p
    }

    fn make_minimal_place() -> RobloxDom {
        // DataModel
        //  ├── Workspace
        //  │   └── Folder "Group"
        //  │       └── Part "Cube" (Position, Size, Color3, Anchored)
        //  └── Lighting
        //      └── Atmosphere "Sky"
        let workspace = InstanceBuilder::new("Workspace").with_child(
            InstanceBuilder::new("Folder")
                .with_name("Group")
                .with_child(
                    InstanceBuilder::new("Part")
                        .with_name("Cube")
                        .with_property("Position", Vector3::new(1.0, 2.0, 3.0))
                        .with_property("Size", Vector3::new(2.0, 2.0, 2.0))
                        .with_property("Color", Color3::new(1.0, 0.0, 0.5))
                        .with_property("Anchored", true),
                ),
        );
        let lighting = InstanceBuilder::new("Lighting")
            .with_child(InstanceBuilder::new("Atmosphere").with_name("Sky"));
        let data_model = InstanceBuilder::new("DataModel")
            .with_child(workspace)
            .with_child(lighting);
        let dom = WeakDom::new(data_model);
        RobloxDom::from_dom(
            dom,
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        )
    }

    #[test]
    fn imports_a_basic_workspace_part() {
        let dom = make_minimal_place();
        let space_root = make_temp_root("basic_place");

        let report = import_into_space(&dom, &space_root, ImportOptions::default())
            .expect("import succeeds");
        assert!(report.total_nodes_seen >= 5); // Workspace + Folder + Part + Lighting + Atmosphere
        assert!(report.total_nodes_imported >= 3); // Folder + Part + Atmosphere
        assert!(
            report.class_counts.iter().any(|c| c.class == "Part"),
            "Part should have been created: {:?}",
            report.class_counts
        );
        // Workspace content now lands under a single place-named container
        // folder: `Workspace/<PlaceName>/...` (PlaceName = the Space dir name).
        let place = place_name_from_root(&space_root).expect("temp root has a name");
        let cube_path = space_root
            .join("Workspace")
            .join(&place)
            .join("Group")
            .join("Cube")
            .join("_instance.toml");
        assert!(
            cube_path.is_file(),
            "Cube TOML should exist: {}",
            cube_path.display()
        );

        // The folder name uniqueness should not have triggered for this fixture.
        assert!(report.name_collisions.is_empty());

        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn imports_atmosphere_under_lighting() {
        let dom = make_minimal_place();
        let space_root = make_temp_root("lighting");
        import_into_space(&dom, &space_root, ImportOptions::default()).expect("import");
        let sky = space_root
            .join("Lighting")
            .join("Sky")
            .join("_instance.toml");
        assert!(
            sky.is_file(),
            "Atmosphere should land under Lighting: {}",
            sky.display()
        );
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn workspace_children_land_under_place_container() {
        // Workspace content lands under a single `Workspace/<PlaceName>/`
        // container Folder (PlaceName = the Space dir name); Lighting and the
        // other services keep their flat cognate roots.
        let dom = make_minimal_place();
        let space_root = make_temp_root("container");
        import_into_space(&dom, &space_root, ImportOptions::default()).expect("import");

        let place = place_name_from_root(&space_root).expect("temp root has a name");

        // 1. The container folder itself is a real, valid Folder TOML.
        let container_toml = space_root
            .join("Workspace")
            .join(&place)
            .join("_instance.toml");
        assert!(
            container_toml.is_file(),
            "container TOML should exist: {}",
            container_toml.display()
        );
        let doc: toml::Value = std::fs::read_to_string(&container_toml)
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            doc.get("metadata")
                .and_then(|m| m.get("class_name"))
                .and_then(|v| v.as_str()),
            Some("Folder"),
            "container should be a Folder: {doc:?}"
        );
        // Its display name is the place name (the Space dir name).
        assert_eq!(
            doc.get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(|v| v.as_str()),
            Some(place.as_str()),
            "container display name should be the place name: {doc:?}"
        );

        // 2. The Workspace subtree sits BENEATH the container, not at the
        //    Workspace root.
        assert!(space_root
            .join("Workspace")
            .join(&place)
            .join("Group")
            .join("Cube")
            .join("_instance.toml")
            .is_file());
        assert!(
            !space_root.join("Workspace").join("Group").exists(),
            "Workspace children must NOT remain at the flat Workspace root"
        );

        // 3. Lighting is NOT wrapped in a place container — services other
        //    than Workspace keep their normal cognate root.
        assert!(
            space_root
                .join("Lighting")
                .join("Sky")
                .join("_instance.toml")
                .is_file(),
            "Lighting children must stay directly under Lighting/"
        );
        assert!(
            !space_root.join("Lighting").join(&place).exists(),
            "Lighting must NOT get a place container"
        );

        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn idempotent_uuids_on_reimport() {
        let dom = make_minimal_place();
        let space_root = make_temp_root("idempotent");
        let salt = b"deterministic-test-salt".to_vec();

        let opts = || ImportOptions {
            space_salt: Some(salt.clone()),
            ..ImportOptions::default()
        };
        import_into_space(&dom, &space_root, opts()).expect("first import");
        let place1 = place_name_from_root(&space_root).expect("temp root has a name");
        let first = std::fs::read_to_string(
            space_root
                .join("Workspace")
                .join(&place1)
                .join("Group")
                .join("Cube")
                .join("_instance.toml"),
        )
        .unwrap();

        // Re-import into a fresh Space root with the SAME salt — uuids
        // should match byte-for-byte because the referent + salt are
        // unchanged. We can't re-import into the same Space root without
        // a `--clean` step (`unique_entity_name` would suffix the folder),
        // so we use a parallel Space + identical salt.
        let space_root2 = make_temp_root("idempotent2");
        import_into_space(&dom, &space_root2, opts()).expect("second import");
        let place2 = place_name_from_root(&space_root2).expect("temp root has a name");
        let second = std::fs::read_to_string(
            space_root2
                .join("Workspace")
                .join(&place2)
                .join("Group")
                .join("Cube")
                .join("_instance.toml"),
        )
        .unwrap();

        let extract_uuid = |s: &str| -> String {
            let d: toml::Value = s.parse().unwrap();
            d.get("metadata")
                .and_then(|m| m.get("uuid"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        assert_eq!(extract_uuid(&first), extract_uuid(&second));

        let _ = std::fs::remove_dir_all(&space_root);
        let _ = std::fs::remove_dir_all(&space_root2);
    }

    #[test]
    fn rejects_off_limits_paths() {
        let dom = make_minimal_place();
        let space_root = make_temp_root("off_limits");
        let mut opts = ImportOptions::default();
        let mut router = ServiceRouter::new(space_root.clone());
        // Force the test by routing to a deny-listed folder name — we
        // can't directly inject via the public router API, so we just
        // assert the router's own check fires.
        // Validate via is_off_limits on a constructed path.
        let probe = space_root.join("SoulService").join("foo");
        assert!(router.is_off_limits(&probe));
        opts.service_router = Some(router);
        // Run the regular import — should succeed because the DOM
        // doesn't carry SoulService data.
        import_into_space(&dom, &space_root, opts).expect("import");
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn unmapped_class_logged_subtree_skipped() {
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace").with_child(
                InstanceBuilder::new("FloofPart")
                    .with_name("WeirdChild")
                    .with_child(InstanceBuilder::new("Part").with_name("Bury")),
            ),
        );
        let dom = WeakDom::new(dm);
        let rbx = RobloxDom::from_dom(
            dom,
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );

        let space_root = make_temp_root("unmapped");
        let report =
            import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        assert!(
            report
                .unmapped_classes
                .iter()
                .any(|u| u.roblox_class == "FloofPart"),
            "FloofPart should be logged as unmapped: {:?}",
            report.unmapped_classes
        );
        // The "Bury" Part under FloofPart should NOT exist on disk
        // because we stop the walk at unmapped nodes. (Workspace content now
        // sits under the place-named container folder.)
        let place = place_name_from_root(&space_root).expect("temp root has a name");
        assert!(!space_root
            .join("Workspace")
            .join(&place)
            .join("WeirdChild")
            .join("Bury")
            .exists());

        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn empty_terrain_imports_without_chunks_or_deferral() {
        // A Terrain instance with no SmoothGrid materialises but produces
        // zero voxel chunks and (now that the decoder is wired) NO
        // deferral approximation.
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace")
                .with_child(InstanceBuilder::new("Terrain").with_name("Terrain")),
        );
        let dom = WeakDom::new(dm);
        let rbx = RobloxDom::from_dom(
            dom,
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );

        let space_root = make_temp_root("terrain_empty");
        let report =
            import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        assert_eq!(report.terrain_chunks_imported, 0);
        assert!(
            !report
                .approximations
                .iter()
                .any(|a| a.reason.contains("deferred")),
            "no deferral note expected now that terrain decode is live: {:?}",
            report.approximations
        );
        // The Terrain folder + TOML should exist (under the place container).
        let place = place_name_from_root(&space_root).expect("temp root has a name");
        let terrain_toml = space_root
            .join("Workspace")
            .join(&place)
            .join("Terrain")
            .join("_instance.toml");
        assert!(terrain_toml.is_file());
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn terrain_with_smooth_grid_writes_voxel_chunks() {
        // Build a one-chunk SmoothGrid (all Grass) and attach it to a
        // Terrain instance. The importer should decode it and write a
        // chunk file + bump terrain_chunks_imported.
        let smooth_grid = build_single_chunk_grid(0, 0, 0, 2 /* Grass */, 255);
        let terrain = InstanceBuilder::new("Terrain")
            .with_name("Terrain")
            .with_property(
                "SmoothGrid",
                rbx_dom_weak::types::BinaryString::from(smooth_grid),
            );
        let dm = InstanceBuilder::new("DataModel")
            .with_child(InstanceBuilder::new("Workspace").with_child(terrain));
        let dom = WeakDom::new(dm);
        let rbx = RobloxDom::from_dom(
            dom,
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );

        let space_root = make_temp_root("terrain_voxels");
        let report =
            import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        assert_eq!(
            report.terrain_chunks_imported, 1,
            "expected exactly one decoded chunk"
        );
        let place = place_name_from_root(&space_root).expect("temp root has a name");
        let chunk = space_root
            .join("Workspace")
            .join(&place)
            .join("Terrain")
            .join("voxel_chunks")
            .join("chunk_0_0_0.bin");
        assert!(
            chunk.is_file(),
            "voxel chunk file should exist: {}",
            chunk.display()
        );
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn csg_with_mesh_data_extracts_glb_and_part() {
        // A UnionOperation carrying a baked CSGMDL2 mesh → csg.glb +
        // csg_baked_extracted incremented + [asset] mesh on the TOML.
        let mesh_blob = crate::csg::make_csgmdl2_triangle_fixture();
        let union = InstanceBuilder::new("UnionOperation")
            .with_name("Carved")
            .with_property("Size", rbx_dom_weak::types::Vector3::new(4.0, 4.0, 4.0))
            .with_property(
                "MeshData",
                rbx_dom_weak::types::BinaryString::from(mesh_blob),
            );
        let dm = InstanceBuilder::new("DataModel")
            .with_child(InstanceBuilder::new("Workspace").with_child(union));
        let dom = WeakDom::new(dm);
        let rbx = RobloxDom::from_dom(
            dom,
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );

        let space_root = make_temp_root("csg_baked");
        let report =
            import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        assert_eq!(report.csg_baked_extracted, 1, "one CSG mesh should bake");
        assert_eq!(report.csg_fallback_aabb, 0);

        let place = place_name_from_root(&space_root).expect("temp root has a name");
        let csg_dir = space_root.join("Workspace").join(&place).join("Carved");
        assert!(csg_dir.join("csg.glb").is_file(), "csg.glb should exist");
        // The Part TOML should point its asset mesh at csg.glb.
        let toml = std::fs::read_to_string(csg_dir.join("_instance.toml")).unwrap();
        assert!(
            toml.contains("csg.glb"),
            "TOML should reference csg.glb: {toml}"
        );
        assert!(
            toml.contains("csg_op"),
            "TOML should record the csg_op: {toml}"
        );
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn csg_without_mesh_data_falls_back_to_aabb() {
        let union = InstanceBuilder::new("NegateOperation")
            .with_name("Hollow")
            .with_property("Size", rbx_dom_weak::types::Vector3::new(2.0, 6.0, 2.0));
        let dm = InstanceBuilder::new("DataModel")
            .with_child(InstanceBuilder::new("Workspace").with_child(union));
        let dom = WeakDom::new(dm);
        let rbx = RobloxDom::from_dom(
            dom,
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );

        let space_root = make_temp_root("csg_aabb");
        let report =
            import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        assert_eq!(report.csg_baked_extracted, 0);
        assert_eq!(report.csg_fallback_aabb, 1, "should fall back to AABB");
        assert!(
            report
                .approximations
                .iter()
                .any(|a| a.reason.contains("AABB fallback")),
            "AABB fallback should be logged: {:?}",
            report.approximations
        );
        let place = place_name_from_root(&space_root).expect("temp root has a name");
        assert!(space_root
            .join("Workspace")
            .join(&place)
            .join("Hollow")
            .join("csg.glb")
            .is_file());
        let _ = std::fs::remove_dir_all(&space_root);
    }

    /// Build a one-chunk SmoothGrid blob (version byte + chunk header +
    /// RLE cells), all of one material. Mirrors the terrain.rs test
    /// helper so the materializer integration test stays self-contained.
    fn build_single_chunk_grid(cx: i32, cy: i32, cz: i32, material: u8, occupancy: u8) -> Vec<u8> {
        let cells_per_chunk = crate::terrain::CELLS_PER_CHUNK;
        let mut buf = vec![crate::terrain::SMOOTH_GRID_VERSION];
        buf.extend_from_slice(&cx.to_le_bytes());
        buf.extend_from_slice(&cy.to_le_bytes());
        buf.extend_from_slice(&cz.to_le_bytes());
        let mut emitted = 0;
        while emitted < cells_per_chunk {
            let run = (cells_per_chunk - emitted).min(256);
            buf.push((material & 0b0011_1111) | 0b0100_0000 | 0b1000_0000);
            buf.push(occupancy);
            buf.push((run - 1) as u8);
            emitted += run;
        }
        buf
    }

    #[test]
    fn assetid_emits_asset_warning() {
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace").with_child(
                InstanceBuilder::new("Sound")
                    .with_name("Hit")
                    .with_property(
                        "SoundId",
                        rbx_dom_weak::types::Content::from("rbxassetid://42"),
                    ),
            ),
        );
        let dom = WeakDom::new(dm);
        let rbx = RobloxDom::from_dom(
            dom,
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );
        let space_root = make_temp_root("assetid");
        let report =
            import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        assert!(
            !report.asset_warnings.is_empty(),
            "rbxassetid:// reference should emit an AssetWarning"
        );
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn skipped_service_recorded() {
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("MarketplaceService")
                .with_child(InstanceBuilder::new("Folder").with_name("Catalog")),
        );
        let dom = WeakDom::new(dm);
        let rbx = RobloxDom::from_dom(
            dom,
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );
        let space_root = make_temp_root("skipped_service");
        let report =
            import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        assert!(
            report
                .skipped_services
                .iter()
                .any(|s| s.service == "MarketplaceService"),
            "MarketplaceService should be flagged as skipped: {:?}",
            report.skipped_services
        );
        // And the child folder should land under _imported/.
        assert!(space_root
            .join("_imported")
            .join("MarketplaceService")
            .join("Catalog")
            .exists());
        let _ = std::fs::remove_dir_all(&space_root);
    }
}

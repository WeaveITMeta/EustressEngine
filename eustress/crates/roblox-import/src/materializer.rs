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
    constrained_range, encode_value_object, is_convertible_value_object, is_value_object_class,
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

    /// Authoring unit symbol stamped into `metadata.unit`. Defaults to the
    /// Eustress stud (`Unit::Stud`, 0.28 m), which a Roblox stud is; the
    /// engine's unit system converts to metres at the load boundary. Verbatim
    /// Size / Position / CFrame stud numbers are unchanged; the tag does the
    /// conversion. Angular props (Orientation, CFrame rotation) are never
    /// unit-converted.
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
            // A Roblox stud is the Eustress stud: files keep Roblox's numbers.
            unit_symbol: Some(eustress_common::units::Unit::Stud.symbol().to_string()),
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

    /// Every written node's world pose, as Roblox means it and as a loader
    /// composes it from the files (`crate::pose`), so each node's
    /// `[transform]` is written relative to its parent's pose.
    poses: HashMap<Ref, (crate::pose::Pose, crate::pose::Pose)>,

    /// Roblox animation ids the place uses (an `Animation`'s `AnimationId`,
    /// a literal in a script that works with animations), each with where it
    /// was first found. After the walk each becomes a clip file
    /// (`crate::animation`) or a report line saying why not.
    animation_ids: std::collections::BTreeMap<u64, &'static str>,

    /// The Lighting Sky's `SunAngularSize` and `MoonAngularSize`, in Roblox's
    /// degrees. After the walk they size the Space's Sun and Moon
    /// (`write_celestial_sizes`).
    celestial_sizes: (Option<f64>, Option<f64>),

    /// Roblox referent → on-disk space-relative path. Used for
    /// `ImportReport::unresolved_refs` reporting.
    referent_to_path: HashMap<Ref, String>,

    /// Pending `Ref` resolution work: `(host_path, host_property, target_ref)`
    /// to be patched after the walk completes.
    pending_refs: Vec<(PathBuf, String, Ref)>,

    /// Pending `[properties.extras]` / `[references]` / root `tags` /
    /// root `[attributes]` / `[properties.physics]` / `[asset]` patches
    /// keyed by absolute TOML path. Applied at the end of the walk so we
    /// only touch each file once.
    pending_patches: HashMap<PathBuf, TomlPatch>,

    /// §8.A storage mode for this import.
    storage: ImportStorage,

    /// The always-available TOML sink. File-natured nodes, ref hosts,
    /// parents with children, Terrain/CSG — and the entire import when not
    /// in a binary storage mode — all go through this.
    toml_sink: TomlSink,

    /// Per-place color manifest accumulated during the walk and flushed to
    /// `<space_root>/.eustress/color_manifest.ndjson` in [`Materializer::run`].
    /// One row per colored `BasePart` — the extract stage of the color study.
    color_manifest: crate::color_manifest::ColorManifestWriter,

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
    /// Additive `[metadata]` scalar keys (e.g. `roblox_brick_color`,
    /// `roblox_color_srgb`) lifted from `PropertyBag::metadata_extras`.
    metadata: HashMap<String, toml::Value>,
    /// Overrides for whole top-level sections other than `[properties]`
    /// (`section -> key -> value`) from `PropertyBag::section_props` — GUI
    /// `[text]` / `[gui]` keys merged onto the class template.
    section_props: HashMap<String, HashMap<String, toml::Value>>,
    tags: Vec<String>,
    refs_uuid: HashMap<String, String>,
    refs_unresolved: HashMap<String, String>,
    asset_mesh: Option<String>,
    /// Folded DataMesh (`SpecialMesh`/`BlockMesh`/`CylinderMesh`) visual
    /// scale — written as a top-level `mesh_scale = [x, y, z]` key (see
    /// `apply_toml_patch` for why it is NOT under `[asset]`).
    mesh_scale: Option<[f32; 3]>,
    /// Folded DataMesh visual offset — top-level `mesh_offset = [x, y, z]`.
    mesh_offset: Option<[f32; 3]>,
    uuid_stamp: Option<String>,
    script_body: Option<String>,
    script_class: Option<ClassName>,
    /// A `KeyframeSequence`'s `keyframes` array (`crate::animation`), written
    /// at the document root, where the clip reader reads it.
    keyframes: Option<toml::Value>,
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
            poses: HashMap::new(),
            animation_ids: Default::default(),
            celestial_sizes: (None, None),
            referent_to_path: HashMap::new(),
            pending_refs: Vec::new(),
            pending_patches: HashMap::new(),
            storage,
            toml_sink,
            color_manifest: crate::color_manifest::ColorManifestWriter::default(),
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
            if constrained_range(class, &inst.properties).is_some() {
                self.vo_ctx.range_names.insert(name.clone());
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

        // ── A clip for each Roblox animation id the place uses ──
        self.write_animation_clips(report);

        // ── The Sky's sun and moon sizes, on the Sun and the Moon ──
        self.write_celestial_sizes(report);

        // ── Flush the per-place color manifest ──
        if !self.color_manifest.is_empty() {
            let dir = self.space_root.join(".eustress");
            if std::fs::create_dir_all(&dir).is_ok() {
                let path = dir.join("color_manifest.ndjson");
                if self.color_manifest.flush_ndjson(&path).is_ok() {
                    report.color_manifest_path = Some(path);
                }
            }
        }

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
                // The service's own properties: Lighting's time of day and fog,
                // Workspace's gravity, and so on.
                if cognate {
                    self.write_service_toml(service_ref, &absolute_dest, report)?;
                }

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
                // Mirror the Roblox place EXACTLY: a service's children land
                // directly under its cognate folder — Workspace children go
                // straight into `Workspace/`, NOT wrapped in a synthetic
                // `Workspace/<PlaceName>/` container. The Space directory is
                // already named after the place, so the extra nesting was a
                // redundant second layer the user doesn't want.
                let _ = (cognate, service_class);
                let (child_dir, child_relpath) = (absolute_dest.clone(), dest_str.clone());

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
    #[allow(dead_code)]
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

    /// Write `<dir>/_service.toml` for a Roblox service: the engine's template
    /// for it with the place's own values laid over `[properties]`
    /// (`service_props`). A service without a template is left alone, and an
    /// existing file is never overwritten.
    fn write_service_toml(
        &self,
        service_ref: Ref,
        dir: &Path,
        report: &mut ImportReport,
    ) -> Result<(), ImportError> {
        let Some(service) = self.dom.get_by_ref(service_ref) else {
            return Ok(());
        };
        let path = dir.join("_service.toml");
        if path.exists() {
            return Ok(());
        }
        let props = props_to_string_map(service);
        let Some(mapped) = crate::service_props::map_service_properties(service.class.as_str(), &props) else {
            return Ok(());
        };
        for (property, ty) in &mapped.unmapped {
            report.record_unmapped_property(service.class.as_str(), property, ty);
        }
        if let Some(body) = crate::service_props::service_toml(service.class.as_str(), &mapped) {
            std::fs::write(&path, body).map_err(|e| ImportError::Io(path.clone(), e))?;
        }
        Ok(())
    }

    fn handle_starter_player(
        &mut self,
        service_ref: Ref,
        report: &mut ImportReport,
    ) -> Result<(), ImportError> {
        let Some(service) = self.dom.get_by_ref(service_ref) else {
            return Ok(());
        };
        // StarterPlayer's own properties (character and camera defaults) go to
        // its service folder even though its children land elsewhere.
        let starter_dir = self.router.absolute(Path::new("StarterPlayer"));
        std::fs::create_dir_all(&starter_dir).map_err(|e| ImportError::Io(starter_dir.clone(), e))?;
        self.write_service_toml(service_ref, &starter_dir, report)?;
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

        // Skip Camera — Roblox's `Workspace.Camera` is a runtime-transient
        // instance (the per-client `CurrentCamera`), not authored scene
        // content. Materialising it duplicates the engine's own viewport
        // camera (the user sees two "Camera" entities). A real place has no
        // standalone Camera to mirror, so we never import one.
        if inst.class == "Camera" {
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
            // A value object that holds children: a Folder for them (its
            // value was folded into its parent's attributes).
            _ if is_value_object_class(inst.class.as_str()) && !inst.children().is_empty() => ClassName::Folder,
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
        for miss in bag.unmapped.drain(..) {
            report.record_unmapped_property(inst.class.as_str(), &miss.property, &miss.variant_type);
        }

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
            // A value object with children of its own (a car's
            // `Handling.Torque` holding `Location`, `Suspension`, ...) stays
            // in the tree as a Folder (see `walk_subtree`'s class mapping),
            // so its children fold into ITS attributes. Its value still folds
            // here: `Handling.Torque.Value` reads `Handling`'s attribute.
            // Any other value object is folded out of the tree, whether or
            // not it converts.
            let keeps_children = !child.children().is_empty();
            if keeps_children {
                report.record_approximation(
                    parent_relpath,
                    child.class.as_str(),
                    "attribute",
                    &format!(
                        "value object '{}' holds children: kept as a Folder for them; \
                         its value is an attribute of '{}'",
                        child.name, requested_name
                    ),
                );
            } else {
                folded_children.insert(*child_ref);
                report.total_nodes_seen += 1;
            }

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
                    // A constrained value's range folds beside it, keyed off
                    // the value's own (possibly de-duplicated) key.
                    if let Some((min, max)) = constrained_range(child.class.as_str(), &child.properties) {
                        for (suffix, bound) in [("MinValue", min), ("MaxValue", max)] {
                            let bound_key = unique_attribute_key(&bag.attributes, &format!("{key}_{suffix}"));
                            bag.attributes.insert(bound_key, bound);
                        }
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

        // ── DataMesh children: SpecialMesh / BlockMesh / CylinderMesh ──
        //
        // A Roblox DataMesh changes what its part DRAWS, never what it
        // collides as: a ball wheel with a SpecialMesh Cylinder rolls on a
        // sphere. So a primitive DataMesh (BlockMesh, CylinderMesh, a
        // SpecialMesh of any MeshType but FileMesh) stays a child instance
        // with its `[mesh]` section, both apps draw it
        // (`eustress_common::data_mesh`), and this part's `[asset]` mesh
        // stays its own shape. Only a SpecialMesh FileMesh with a mesh
        // folds here: its `MeshId` joins the PARENT's `bag.asset_refs` and
        // rides the resolver/fetch path below into `[asset].mesh`, with
        // `Scale` / `Offset` as the top-level visual `mesh_scale` /
        // `mesh_offset` keys (render transform only, never the collider).
        // MeshType numbers (rbx reflection database): Head=0 Torso=1
        // Wedge=2 Sphere=3 Cylinder=4 FileMesh=5 Brick=6 Prism=7 Pyramid=8
        // ParallelRamp=9 RightAngleRamp=10 CornerWedge=11; an unknown
        // number with a mesh is treated as FileMesh.
        let mut folded_mesh_scale: Option<[f32; 3]> = None;
        let mut folded_mesh_offset: Option<[f32; 3]> = None;
        // True when a SpecialMesh FileMesh supplied this part's mesh (its
        // `MeshId` won the slot). Roblox renders such a mesh at its NATIVE
        // size times `Scale`, independent of the part's `Size`, unlike a
        // MeshPart whose mesh stretches to fill `Size`.
        let mut folded_file_mesh = false;
        let mut folded_texture_uri: Option<String> = None;
        let mut folded_vertex_color: Option<[f32; 3]> = None;
        let mut saw_mesh_child = false;
        for child_ref in inst.children().iter() {
            let Some(child) = self.dom.get_by_ref(*child_ref) else {
                continue;
            };
            if child.class.as_str() != "SpecialMesh" {
                continue;
            }
            // Route the child's properties through the same mapper the
            // parent used so every URI spelling (`Content` / legacy
            // `ContentId` / plain `String`) of `MeshId` / `MeshContent`
            // lands in `asset_refs` uniformly.
            let child_props = props_to_string_map(child);
            let child_bag = map_properties(&child_props, ClassName::SpecialMesh);
            let mesh_uri: Option<String> = ["MeshId", "MeshContent", "Content"]
                .iter()
                .find_map(|k| child_bag.asset_refs.get(*k))
                .filter(|u| !u.trim().is_empty())
                .cloned();
            let mesh_type: Option<u32> = match child_props.get("MeshType") {
                Some(rbx_dom_weak::types::Variant::Enum(e)) => Some(e.to_u32()),
                _ => None,
            };
            // A primitive SpecialMesh stays a child (above).
            let file_mesh = mesh_uri.is_some() && !matches!(mesh_type, Some(t) if t != 5 && t <= 11);
            if !file_mesh {
                continue;
            }
            // A folded file mesh never spawns as its own instance.
            folded_children.insert(*child_ref);
            report.total_nodes_seen += 1;

            if saw_mesh_child {
                // Roblox honours a single DataMesh per part; extras are
                // dead data. Fold them out + note the drop.
                report.record_approximation(
                    parent_relpath,
                    "SpecialMesh",
                    "asset",
                    &format!(
                        "duplicate mesh child '{}' under '{}' dropped — one DataMesh per part",
                        child.name, requested_name
                    ),
                );
                continue;
            }
            saw_mesh_child = true;
            if let Some(unknown) = mesh_type.filter(|t| *t > 11) {
                report.record_approximation(
                    parent_relpath,
                    "SpecialMesh",
                    "asset",
                    &format!("unknown SpecialMesh MeshType {unknown} — treated as FileMesh"),
                );
            }

            // The texture is baked into the part's mesh after the mesh
            // resolves (see `texture_bake`); an empty TextureId is no texture.
            if let Some(uri) = ["TextureContent", "TextureId"]
                .iter()
                .find_map(|k| child_bag.asset_refs.get(*k))
                .filter(|u| !u.trim().is_empty())
            {
                folded_texture_uri = Some(uri.clone());
            }
            if let Some(rbx_dom_weak::types::Variant::Vector3(v)) = child_props.get("VertexColor") {
                folded_vertex_color = Some([v.x, v.y, v.z]);
            }

            // DataMesh base properties: `Scale` (default 1,1,1) and
            // `Offset` (default 0,0,0). Defaults are omitted so the
            // emitted TOML stays additive-only.
            if let Some(rbx_dom_weak::types::Variant::Vector3(v)) = child_props.get("Scale") {
                if (v.x, v.y, v.z) != (1.0, 1.0, 1.0) {
                    folded_mesh_scale = Some([v.x, v.y, v.z]);
                }
            }
            if let Some(rbx_dom_weak::types::Variant::Vector3(v)) = child_props.get("Offset") {
                if (v.x, v.y, v.z) != (0.0, 0.0, 0.0) {
                    folded_mesh_offset = Some([v.x, v.y, v.z]);
                }
            }

            // The ref joins the PARENT's asset_refs and rides the resolve →
            // fetch → `.glb` pipeline below. `entry().or_insert` so a
            // MeshPart's own MeshId/MeshContent always wins over the child's.
            if let Some(uri) = mesh_uri {
                if !bag.asset_refs.contains_key("MeshId") {
                    folded_file_mesh = true;
                }
                bag.asset_refs.entry("MeshId".to_string()).or_insert(uri);
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

        // ── A Roblox cylinder or ball part's size ──
        // A SpecialMesh FileMesh folded onto a Shape=Cylinder part draws it
        // instead of the engine's cylinder, so the part keeps Roblox's pose
        // and size rather than the cylinder's turn. A part that draws as its
        // own cylinder or ball gets Roblox's round sides; one with a DataMesh
        // child keeps its whole box, which that child's look is scaled by.
        let has_data_mesh = inst.children().iter().any(|r| {
            self.dom
                .get_by_ref(*r)
                .is_some_and(|c| matches!(c.class.as_str(), "SpecialMesh" | "BlockMesh" | "CylinderMesh" | "FileMesh"))
        });
        if folded_file_mesh && overrides.asset_mesh.as_deref() == Some("parts/cylinder.glb") {
            crate::property_map::unturn_cylinder(&mut overrides);
        } else if !has_data_mesh {
            if let Some(roblox) = crate::property_map::round_sides(&mut overrides) {
                let across = overrides.scale.map_or(0.0, |s| s[0]);
                let note = if overrides.asset_mesh.as_deref() == Some("parts/ball.glb") {
                    format!("the ball is {across} studs across, as Roblox draws it: the smallest side of Size {roblox:?}")
                } else {
                    format!(
                        "the cylinder is {across} studs across, as Roblox draws it: the smaller of Size.Y {} and Size.Z {}",
                        roblox[0], roblox[2]
                    )
                };
                report.record_approximation(
                    &format!("{parent_relpath}/{requested_name}"),
                    inst.class.as_str(),
                    class_template_name,
                    &note,
                );
            }
        }

        // ── Pose relative to the parent's pose (the `ParentPose` rule) ──
        // Roblox gives a part's CFrame in world space and an Attachment's
        // relative to its part. Each node's `[transform]` is its world pose
        // relative to its PARENT's pose as a loader composes it, so a reader
        // that composes parent by parent lands every node where Roblox had
        // it, whatever sits in between. A node whose parent chain holds no
        // pose is written exactly as before.
        {
            use crate::pose::Pose;
            let (parent_roblox, parent_written) = self
                .poses
                .get(&inst.parent())
                .copied()
                .unwrap_or((Pose::IDENTITY, Pose::IDENTITY));
            let posed = overrides.position.is_some() || overrides.rotation.is_some();
            let own = Pose {
                t: overrides.position.unwrap_or([0.0; 3]),
                r: overrides.rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]),
            };
            let is_part = eustress_common::datamodel::record::loads_as_part(eustress_class);
            let is_attached = matches!(eustress_class, ClassName::Attachment | ClassName::Bone);
            let (roblox, written) = if posed && is_part {
                // `own` is the part's world pose as written (a cylinder's axis
                // correction included); Roblox's is its CFrame.
                let roblox = match string_props.get("CFrame") {
                    Some(rbx_dom_weak::types::Variant::CFrame(cf)) => {
                        let (t, r) = crate::property_map::cframe_to_translation_quat(cf);
                        Pose { t, r }
                    }
                    _ => own,
                };
                (roblox, own)
            } else if posed && is_attached {
                // `own` is relative to the part's CFrame.
                let world = parent_roblox.compose(&own);
                (world, world)
            } else {
                (parent_roblox, parent_written)
            };
            if posed && (is_part || is_attached) && parent_written != Pose::IDENTITY {
                let local = parent_written.relative(&written);
                overrides.position = Some(local.t);
                overrides.rotation = Some(local.r);
            }
            self.poses.insert(inst.referent(), (roblox, written));
        }
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

        // ── Color manifest row (one per colored part) ──
        // Emitted here so BOTH the TOML and binary-ECS write paths
        // contribute uniformly. Gated on a resolved color (default-grey
        // parts carry no Color/BrickColor variant and are skipped).
        if let Some(rgba) = overrides.color_rgba {
            let srgb = [
                (rgba[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                (rgba[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                (rgba[2] * 255.0).round().clamp(0.0, 255.0) as u8,
            ];
            let pos = overrides.position.unwrap_or([0.0, 0.0, 0.0]);
            let part_id = u64::from_be_bytes(uuid.as_bytes()[..8].try_into().unwrap());
            self.color_manifest.push(crate::color_manifest::ColorRow {
                world_id: self
                    .space_root
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string(),
                part_id,
                srgb,
                oklch: crate::color_manifest::srgb_to_oklch(rgba[0], rgba[1], rgba[2]),
                roblox_brick: overrides.brick_number,
                class: eustress_class.as_str().to_string(),
                morton: crate::color_manifest::morton_for(pos),
            });
        }

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
        // A parent that RECEIVED folded children must stay a
        // `_instance.toml` folder: folded ValueObject attributes are
        // layered onto the TOML by the second-pass patch
        // (`bag.attributes` → `[attributes]`), and a folded DataMesh
        // child's primitive routing + `mesh_scale`/`mesh_offset` keys are
        // patched the same way — a bare binary-ECS core has no TOML to
        // patch. Mirrors the `has_mesh_asset_ref` term above.
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
        for (k, v) in bag.metadata_extras {
            patch.metadata.insert(k, v);
        }
        // A Sky's SunAngularSize and MoonAngularSize size the Space's Sun and
        // Moon, which own their discs (`write_celestial_sizes`); the Sky
        // itself holds neither. Only the Sky under Lighting is the one Roblox
        // draws.
        if eustress_class == ClassName::Sky {
            if let Some(sky) = bag.section_props.get_mut("sky") {
                let number = |v: toml::Value| v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
                let sun = sky.remove("sun_angular_size").and_then(number);
                let moon = sky.remove("moon_angular_size").and_then(number);
                let under_lighting = self.dom.get_by_ref(inst.parent()).is_some_and(|p| p.class.as_str() == "Lighting");
                if under_lighting {
                    self.celestial_sizes = (sun.or(self.celestial_sizes.0), moon.or(self.celestial_sizes.1));
                }
            }
        }
        for (section, kvs) in bag.section_props {
            patch.section_props.entry(section).or_default().extend(kvs);
        }
        for (prop, target_ref) in bag.refs {
            self.pending_refs
                .push((created.toml_path.clone(), prop, target_ref));
        }
        // Asset refs → resolver. With a fetcher present, MESH properties
        // (Wave F2) fetch + decode the Roblox `.mesh` into a real `.glb`
        // under `<space>/assets/meshes/`, and every other id-bearing ref
        // (Wave F3) is content-sniffed into `assets/textures/` or
        // `assets/sounds/`. Fetch/decode/sniff failures stay on the
        // placeholder path with a warning.
        let mut file_mesh_native_extent: Option<[f32; 3]> = None;
        // The mesh the part draws, once it is a real fetched `.mesh`, and a
        // MeshPart's own texture: together they make a textured variant below.
        let mut resolved_mesh_id: Option<u64> = None;
        let meshpart_texture_uri: Option<String> = if eustress_class == ClassName::Part {
            ["TextureContent", "TextureID"]
                .iter()
                .find_map(|k| bag.asset_refs.get(*k))
                .filter(|u| !u.trim().is_empty())
                .cloned()
        } else {
            None
        };
        if special == SpecialKind::Csg {
            // A union's `AssetId` names the `PartOperationAsset` model that
            // holds its geometry; `import_csg_instance` fetches and decodes
            // it. Sent through the media path it was fetched, sniffed as
            // "not an image or sound", and reported as a failure.
            bag.asset_refs.remove("AssetId");
        }
        for (prop, uri) in bag.asset_refs {
            // An Animation's id is kept as written: a Roblox id finds its
            // clip through the Space's id map, which the clip fetch after the
            // walk fills (`crate::animation`).
            if eustress_class == ClassName::Animation && prop == "AnimationId" {
                if !uri.trim().is_empty() {
                    patch
                        .section_props
                        .entry("properties".to_string())
                        .or_default()
                        .insert("animation_id".to_string(), toml::Value::String(uri.clone()));
                    if let Some(id) = eustress_common::animation::content::roblox_asset_id(&uri) {
                        self.animation_ids.entry(id).or_insert("AnimationId");
                    }
                }
                continue;
            }
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
            if prop_is_mesh {
                // Mesh-class properties point at mesh assets.
                if folded_file_mesh && prop == "MeshId" {
                    file_mesh_native_extent = resolved.native_extent;
                }
                if resolved.resolved {
                    resolved_mesh_id = asset_resolver::AssetReference::parse(&uri).asset_id();
                }
                // Forward slashes, so the path reads the same on every OS.
                patch.asset_mesh =
                    Some(resolved.asset_path.to_string_lossy().replace('\\', "/"));
                continue;
            }
            // Media goes where the class's engine loader reads it. It must
            // never become `[asset].path`: the engine's `[asset]` table
            // requires `mesh`, so an `[asset]` holding only `path` fails to
            // deserialize and the whole instance is skipped at load.
            let space_url = if resolved.resolved {
                space_url(&self.space_root, &created.folder_path, &resolved.asset_path)
            } else {
                None
            };
            match (media_section_key(eustress_class, &prop), space_url) {
                (Some((section, key)), Some(url)) => {
                    patch
                        .section_props
                        .entry(section.to_string())
                        .or_default()
                        .insert(key.to_string(), toml::Value::String(url));
                }
                // Resolved but no engine slot for it (a MeshPart texture, a
                // Trail texture): keep where the file landed.
                (None, Some(url)) => {
                    patch.extras.insert(prop.clone(), toml::Value::String(url));
                }
                // Not fetched: keep the Roblox URI so a later run with a
                // credential, or a tool, can still resolve it. The class key
                // stays empty, which the engine draws as nothing, like Roblox
                // does for an image that fails to load.
                (_, None) => {
                    if !uri.trim().is_empty() {
                        patch.extras.insert(prop.clone(), toml::Value::String(uri.clone()));
                    }
                }
            }
        }

        // A folded SpecialMesh FileMesh's visual scale and offset.
        if folded_mesh_scale.is_some() {
            patch.mesh_scale = folded_mesh_scale;
        }
        // A resolved FileMesh is written unit-sized, and the engine multiplies
        // it by the part's `Size`. Roblox instead draws it at native size times
        // `Scale`, so the visual scale must cancel `Size` and restore the native
        // extent: native * Scale / Size, per axis. The collider stays the
        // part's `Size` box, which is also what Roblox collides against.
        if let (Some(native), Some(size)) = (file_mesh_native_extent, overrides.scale) {
            let scale = folded_mesh_scale.unwrap_or([1.0, 1.0, 1.0]);
            patch.mesh_scale = Some([0usize, 1, 2].map(|a| {
                let s = size[a].abs();
                native[a] * scale[a] / if s > 1e-6 { s } else { 1.0 }
            }));
        }
        if folded_mesh_offset.is_some() {
            patch.mesh_offset = folded_mesh_offset;
        }

        // Textured mesh: bake the texture into the part's glb and let the
        // engine draw the glb's own material (`respect_gltf_materials`).
        let texture = meshpart_texture_uri
            .map(|uri| (uri, true))
            .or_else(|| folded_texture_uri.map(|uri| (uri, false)));
        if let (Some((tex_uri, is_meshpart)), Some(mesh_id), Some(fetcher)) =
            (texture, resolved_mesh_id, self.opts.asset_fetcher.as_deref())
        {
            match asset_resolver::AssetReference::parse(&tex_uri).asset_id() {
                Some(tex_id) => {
                    let colour = overrides.color_rgba.unwrap_or([1.0, 1.0, 1.0, 1.0]);
                    let (roughness, metallic, _) = eustress_common::classes::Material::from_string(
                        overrides.material.as_deref().unwrap_or("Plastic"),
                    )
                    .pbr_params();
                    let look = crate::texture_bake::TextureLook {
                        under: is_meshpart.then(|| {
                            [0, 1, 2].map(|c| (colour[c].clamp(0.0, 1.0) * 255.0).round() as u8)
                        }),
                        tint: if is_meshpart {
                            [1.0, 1.0, 1.0]
                        } else {
                            folded_vertex_color.unwrap_or([1.0, 1.0, 1.0])
                        },
                        opacity: colour[3],
                        roughness,
                        metallic,
                    };
                    match asset_resolver::bake_textured_mesh(
                        fetcher,
                        mesh_id,
                        tex_id,
                        &look,
                        &self.space_root,
                        &created.folder_path,
                    ) {
                        Ok(rel) => {
                            patch.asset_mesh = Some(rel.to_string_lossy().replace('\\', "/"));
                            patch
                                .section_props
                                .entry("properties".to_string())
                                .or_default()
                                .insert("respect_gltf_materials".to_string(), toml::Value::Boolean(true));
                            report.textured_meshes += 1;
                        }
                        Err(e) => report.record_asset_warning(
                            &tex_uri,
                            class_template_name,
                            if is_meshpart { "TextureID" } else { "TextureId" },
                            &format!("texture not applied: {e}"),
                        ),
                    }
                }
                None => report.record_asset_warning(
                    &tex_uri,
                    class_template_name,
                    if is_meshpart { "TextureID" } else { "TextureId" },
                    "texture not applied: not an asset id",
                ),
            }
        }

        // Script-source post-processing.
        if let Some(body) = bag.script_source {
            // Roblox animation ids the script names in literals join the
            // clip fetch (`crate::animation::script_animation_ids`).
            for id in crate::animation::script_animation_ids(&body) {
                self.animation_ids.entry(id).or_insert("script");
            }
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

        // A KeyframeSequence is one record: its Keyframes, with their Poses,
        // NumberPoses and markers, are written inline (`crate::animation`)
        // rather than as folders of their own.
        if eustress_class == ClassName::KeyframeSequence {
            let record = crate::animation::sequence_record(self.dom, inst);
            for note in &record.notes {
                report.record_approximation(&entity_relpath, inst.class.as_str(), "KeyframeSequence", note);
            }
            patch
                .section_props
                .entry("keyframe_sequence".to_string())
                .or_default()
                .extend(record.sequence);
            patch.keyframes = Some(toml::Value::Array(record.keyframes));
            report.animation_sequences += 1;
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
            // A KeyframeSequence's Keyframes are in its record.
            if eustress_class == ClassName::KeyframeSequence
                && self.dom.get_by_ref(*child_ref).is_some_and(|c| c.class.as_str() == "Keyframe")
            {
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
        // Roblox has stored a union's baked mesh in two places over time.
        // Modern files carry it in `MeshData2` as a deduplicated SharedString
        // and leave the legacy `MeshData` present but EMPTY; older files have
        // only `MeshData`. Reading the legacy field alone turned every union
        // in a current place into a grey AABB block (Vehicle Simulator:
        // 12,121 of them), so prefer the modern field and treat an empty blob
        // as absent so it can never shadow a populated one.
        let blob_of = |name: &str| -> Option<Vec<u8>> {
            let bytes: &[u8] = match props.get(&rbx_dom_weak::ustr(name))? {
                rbx_dom_weak::types::Variant::BinaryString(bs) => bs.as_ref(),
                rbx_dom_weak::types::Variant::SharedString(ss) => ss.data(),
                _ => return None,
            };
            (!bytes.is_empty()).then(|| bytes.to_vec())
        };
        let mut mesh_data: Option<Vec<u8>> = blob_of("MeshData2").or_else(|| blob_of("MeshData"));

        // No inline geometry: Roblox may keep it in the cloud, as a
        // `PartOperationAsset` model named by `AssetId`. Fetch that when a
        // fetcher is configured; otherwise say plainly why the union is a
        // stand-in block.
        let mut missing_reason: Option<String> = None;
        if mesh_data.is_none() {
            let asset_uri = match props.get(&rbx_dom_weak::ustr("AssetId")) {
                Some(rbx_dom_weak::types::Variant::ContentId(c)) => c.as_str().to_string(),
                Some(rbx_dom_weak::types::Variant::Content(c)) => {
                    c.as_uri().unwrap_or_default().to_string()
                }
                Some(rbx_dom_weak::types::Variant::String(s)) => s.clone(),
                _ => String::new(),
            };
            let asset_id = asset_resolver::AssetReference::parse(&asset_uri).asset_id();
            missing_reason = Some(match (asset_id, self.opts.asset_fetcher.as_deref()) {
                (None, _) => "no inline MeshData and no cloud AssetId".to_string(),
                (Some(id), None) => format!(
                    "geometry is in the cloud (rbxassetid://{id}) and no asset fetcher is configured"
                ),
                (Some(id), Some(fetcher)) => match fetcher
                    .fetch(id)
                    .and_then(|bytes| crate::csg::part_operation_asset_blob(&bytes))
                {
                    Ok(blob) => {
                        mesh_data = Some(blob);
                        report.csg_cloud_fetched += 1;
                        String::new()
                    }
                    Err(e) => format!("cloud geometry rbxassetid://{id} unavailable: {e}"),
                },
            })
            .filter(|r| !r.is_empty());
        }

        let use_part_color = matches!(
            props.get(&rbx_dom_weak::ustr("UsePartColor")),
            Some(rbx_dom_weak::types::Variant::Bool(true))
        );
        let outcome =
            crate::csg::import_csg(&created.folder_path, mesh_data.as_deref(), use_part_color)
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
                vertex_coloured,
            } => {
                if *vertex_coloured {
                    // Roblox draws this union in its original parts' colours,
                    // which the mesh carries per vertex. The engine multiplies
                    // the part colour into them, so the part colour is white;
                    // the union's own Color stays in `roblox_color_srgb`.
                    let alpha = match props.get(&rbx_dom_weak::ustr("Transparency")) {
                        Some(rbx_dom_weak::types::Variant::Float32(t)) => (1.0 - *t as f64).clamp(0.0, 1.0),
                        _ => 1.0,
                    };
                    patch.section_props.entry("properties".to_string()).or_default().insert(
                        "color".to_string(),
                        toml::Value::Array(
                            [1.0, 1.0, 1.0, alpha].into_iter().map(toml::Value::Float).collect(),
                        ),
                    );
                }
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
                let reason = missing_reason.as_deref().unwrap_or(reason.as_str());
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

    /// Fetch each Roblox animation id the place uses and write the clip it
    /// holds to `assets/animations/rbx-<id>.anim.toml`, naming the file in
    /// the Space's id map (`assets/roblox_ids.toml`), where an `AnimationId`
    /// finds its clip at run time. An id with no clip is reported with the
    /// reason; a script's literal that turns out to be an image or a sound
    /// is left alone.
    /// The Lighting Sky's `SunAngularSize` and `MoonAngularSize` as the
    /// Space's Sun and Moon disc sizes: `Lighting/Sun.instance.toml`
    /// `[star] angular_size` and `Lighting/Moon.instance.toml`
    /// `[moon] angular_size`, where the engine reads them. A file the Space
    /// lacks is made from the engine's own template first. Without the Roblox
    /// values the Sun and Moon keep their defaults.
    fn write_celestial_sizes(&mut self, report: &mut ImportReport) {
        let (sun, moon) = std::mem::take(&mut self.celestial_sizes);
        let targets = [
            (sun, "Sun.instance.toml", ClassName::Star, SUN_TEMPLATE, ROBLOX_SUN_ANGULAR_SIZE, SUN_ANGULAR_SIZE),
            (moon, "Moon.instance.toml", ClassName::Moon, MOON_TEMPLATE, ROBLOX_MOON_ANGULAR_SIZE, MOON_ANGULAR_SIZE),
        ];
        for (roblox, file, class, template, roblox_default, default) in targets {
            let Some(roblox) = roblox.filter(|v| v.is_finite()) else { continue };
            let size = celestial_size(roblox, roblox_default, default);
            let rel = format!("Lighting/{file}");
            let path = self.space_root.join("Lighting").join(file);
            let text = std::fs::read_to_string(&path).unwrap_or_else(|_| template.to_string());
            let written = text
                .parse::<toml::Value>()
                .map_err(|e| e.to_string())
                .and_then(|mut doc| {
                    let mut section = toml::value::Table::new();
                    section.insert("angular_size".to_string(), toml::Value::Float(size));
                    eustress_common::plugins::celestial_sections::store_section(&mut doc, class, section);
                    toml::to_string_pretty(&doc).map_err(|e| e.to_string())
                })
                .and_then(|out| {
                    std::fs::create_dir_all(self.space_root.join("Lighting")).map_err(|e| e.to_string())?;
                    std::fs::write(&path, out).map_err(|e| e.to_string())
                });
            let note = match written {
                Ok(()) => format!("the Sky's {roblox} degrees (Roblox's size includes its glow) is a disc of {size} degrees"),
                Err(e) => format!("the Sky's {roblox} degrees were not written: {e}"),
            };
            report.record_approximation(&rel, "Sky", class.as_str(), &note);
        }
    }

    fn write_animation_clips(&mut self, report: &mut ImportReport) {
        let ids = std::mem::take(&mut self.animation_ids);
        let mut clips = std::collections::BTreeMap::new();
        for (id, found_in) in ids {
            let missing = |reason: String| crate::import_report::MissingAnimation {
                id,
                found_in: found_in.to_string(),
                reason,
            };
            let fetched = match self.opts.asset_fetcher.as_deref() {
                Some(fetcher) => fetcher.fetch(id).map_err(|e| format!("not fetched: {e}")),
                None => Err("not fetched: this import has no asset fetcher".to_string()),
            };
            match fetched.and_then(|bytes| crate::animation::clip_from_model(&bytes, id)) {
                Ok(Some(clip)) => {
                    let rel = crate::animation::clip_path(id);
                    let path = self.space_root.join(&rel);
                    let written = path
                        .parent()
                        .map_or(Ok(()), std::fs::create_dir_all)
                        .and_then(|_| std::fs::write(&path, &clip.text));
                    match written {
                        Ok(()) => {
                            for note in &clip.notes {
                                report.record_approximation(&rel, "KeyframeSequence", "KeyframeSequence", note);
                            }
                            clips.insert(id, rel);
                            report.animation_clips += 1;
                        }
                        Err(e) => report.animation_ids_missing.push(missing(format!("{}: {e}", path.display()))),
                    }
                }
                Ok(None) if found_in == "script" => {}
                Ok(None) => report
                    .animation_ids_missing
                    .push(missing("the asset holds no KeyframeSequence".to_string())),
                Err(reason) => report.animation_ids_missing.push(missing(reason)),
            }
        }
        if !clips.is_empty() {
            if let Err(e) = crate::animation::merge_id_map(&self.space_root, &clips) {
                for id in clips.keys() {
                    report.animation_ids_missing.push(crate::import_report::MissingAnimation {
                        id: *id,
                        found_in: "AnimationId".to_string(),
                        reason: format!("the clip was written but the id map was not: {e}"),
                    });
                }
            }
        }
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
    // At the document root: `tags = [...]` is what the engine's
    // `InstanceDefinition` and `datamodel::record::record_props` read.
    // Written under `[metadata]`, every imported CollectionService tag was
    // invisible to Studio, to Play and to a Player.
    if !patch.tags.is_empty() {
        let mut tags_array: Vec<toml::Value> = Vec::new();
        let existing = root.get("tags").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        for tag in existing
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .chain(patch.tags.iter().cloned())
        {
            if !tags_array.iter().any(|t| t.as_str() == Some(tag.as_str())) {
                tags_array.push(toml::Value::String(tag));
            }
        }
        root.insert("tags".to_string(), toml::Value::Array(tags_array));
    }

    // ── Metadata extras (roblox_brick_color, roblox_color_srgb) ──
    if !patch.metadata.is_empty() {
        let meta = root
            .entry("metadata".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(t) = meta.as_table_mut() {
            for (k, v) in &patch.metadata {
                t.insert(k.clone(), v.clone());
            }
        }
    }

    // ── Section overrides (GUI [text] / [gui] keys) ──
    // Merge onto the class template's existing sections so unset keys keep
    // their defaults (a TextLabel keeps its template [text] fields and only
    // `text` / `z_index` are overridden from the source place).
    for (section, kvs) in &patch.section_props {
        if kvs.is_empty() {
            continue;
        }
        let sect = root
            .entry(section.clone())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(t) = sect.as_table_mut() {
            for (k, v) in kvs {
                t.insert(k.clone(), v.clone());
            }
        }
    }

    // ── A KeyframeSequence's keyframes ──
    if let Some(keyframes) = &patch.keyframes {
        root.insert("keyframes".to_string(), keyframes.clone());
    }

    // ── Properties extras / physics ──
    if !patch.extras.is_empty() || !patch.physics.is_empty() {
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
        }
    }

    // ── Attributes ──
    // At the document root: `[attributes]` is the table the engine reads
    // (`InstanceDefinition::attributes`, `datamodel::record::record_props`,
    // the Properties panel). Written under `[properties]`, every Roblox
    // attribute and every folded value object was invisible to Studio, to
    // Play and to a Player, and `GetAttribute` returned nil.
    if !patch.attributes.is_empty() {
        let attrs = root
            .entry("attributes".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(t) = attrs.as_table_mut() {
            for (k, v) in &patch.attributes {
                t.insert(k.clone(), v.clone());
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

    // ── Folded DataMesh visual adjustment ──
    //
    // Written as TOP-LEVEL `mesh_scale` / `mesh_offset` keys, NOT under
    // `[asset]`, deliberately: the engine deserialises `[asset]` into
    // `AssetReference`, whose field set is frozen (it is struct-literal
    // constructed across the engine), while `InstanceDefinition`'s
    // `#[serde(flatten)] extra` map already captures unknown top-level
    // keys on every load path — the loader extracts these two from there
    // and applies them to the RENDER transform only.
    if let Some(s) = &patch.mesh_scale {
        root.insert(
            "mesh_scale".to_string(),
            toml::Value::Array(s.iter().map(|f| toml::Value::Float(*f as f64)).collect()),
        );
    }
    if let Some(o) = &patch.mesh_offset {
        root.insert(
            "mesh_offset".to_string(),
            toml::Value::Array(o.iter().map(|f| toml::Value::Float(*f as f64)).collect()),
        );
    }

    // ── Asset section ── (mesh only: the engine's `[asset]` requires `mesh`;
    // media lands in its class section, see `media_section_key`.)
    if let Some(mesh) = &patch.asset_mesh {
        let asset = root
            .entry("asset".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(t) = asset.as_table_mut() {
            t.insert("mesh".to_string(), toml::Value::String(mesh.clone()));
            t.entry("scene".to_string())
                .or_insert_with(|| toml::Value::String("Scene0".to_string()));
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
/// Roblox's default `SunAngularSize` and `MoonAngularSize`, in degrees.
const ROBLOX_SUN_ANGULAR_SIZE: f64 = 21.0;
const ROBLOX_MOON_ANGULAR_SIZE: f64 = 11.0;
/// The engine's default drawn Sun and Moon sizes those defaults stand for, in
/// degrees across.
const SUN_ANGULAR_SIZE: f64 = 8.0;
const MOON_ANGULAR_SIZE: f64 = 2.0;
/// The engine's own Sun and Moon files, which a Space's open-time repair
/// writes when they are missing.
const SUN_TEMPLATE: &str = include_str!("../../engine/assets/lighting_templates/Sun.instance.toml");
const MOON_TEMPLATE: &str = include_str!("../../engine/assets/lighting_templates/Moon.instance.toml");

/// A Roblox Sky size as the engine's disc. Roblox's sizes a billboard that
/// includes a wide glow and the engine's is the physical disc, so the size is
/// scaled from Roblox's default to the engine's: a place at Roblox's default
/// lands exactly on the engine's, and one that doubled its sun gets twice the
/// engine's. Kept within the readers' 0.05 to 20 degrees.
fn celestial_size(roblox: f64, roblox_default: f64, default: f64) -> f64 {
    (default * (roblox / roblox_default)).clamp(0.05, 20.0)
}

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

/// The `[section] key` an engine loader reads a media path from, for a Roblox
/// asset property on a given class. `None` when the engine has no slot for it.
///
/// Every one of these loaders hands the string straight to Bevy's asset
/// server, so the value is written as a `space://` URL (see [`space_url`]).
fn media_section_key(class: ClassName, roblox_prop: &str) -> Option<(&'static str, &'static str)> {
    Some(match (class, roblox_prop) {
        (ClassName::Decal, "Texture" | "TextureContent") => ("decal", "texture"),
        (ClassName::Texture, "Texture" | "TextureContent") => ("texture", "texture"),
        (ClassName::ImageLabel | ClassName::ImageButton, "Image" | "ImageContent") => {
            ("image", "image")
        }
        (ClassName::Sound, "SoundId" | "AudioContent") => ("sound", "sound_id"),
        (ClassName::ParticleEmitter, "Texture") => ("particle", "texture"),
        (ClassName::Beam, "Texture") => ("beam", "texture"),
        (ClassName::VideoFrame, "Video" | "VideoContent") => ("video", "source"),
        (ClassName::Sky, "SkyboxFt") => ("sky", "skybox_front"),
        (ClassName::Sky, "SkyboxBk") => ("sky", "skybox_back"),
        (ClassName::Sky, "SkyboxLf") => ("sky", "skybox_left"),
        (ClassName::Sky, "SkyboxRt") => ("sky", "skybox_right"),
        (ClassName::Sky, "SkyboxUp") => ("sky", "skybox_top"),
        (ClassName::Sky, "SkyboxDn") => ("sky", "skybox_bottom"),
        _ => return None,
    })
}

/// A fetched file as the `space://` URL the engine's asset server resolves
/// against the Space root. `rel_to_instance` is the path the resolver
/// returned, relative to the instance folder. `None` when the file is not
/// inside the Space.
fn space_url(space_root: &Path, instance_dir: &Path, rel_to_instance: &Path) -> Option<String> {
    use std::path::Component;
    let mut abs = PathBuf::new();
    for comp in instance_dir.join(rel_to_instance).components() {
        match comp {
            Component::ParentDir => {
                abs.pop();
            }
            Component::CurDir => {}
            other => abs.push(other.as_os_str()),
        }
    }
    let rel = abs.strip_prefix(space_root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| format!("space://{}", parts.join("/")))
}

/// True when the Roblox property maps to a mesh asset (vs. media that lands in
/// a class section). Used to pick the `[asset].mesh` path.
fn is_mesh_property(roblox_prop: &str, class: ClassName) -> bool {
    // `MeshContent` is the modern `Content`-typed spelling of
    // `MeshPart.MeshId` — without it here a MeshPart whose only mesh ref
    // is `MeshContent` would (a) fail the `has_mesh_asset_ref` TOML gate
    // and bake to a binary core that drops the ref, and (b) resolve as a
    // single-path `[asset].path` instead of `[asset].mesh`.
    matches!(roblox_prop, "MeshId" | "CollisionMeshId" | "MeshContent")
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
    // Before the first instance file: a Space whose import stops partway
    // never holds parent-relative files without the key that says so.
    mark_parent_pose_rule(space_root);
    let materializer = Materializer::new(dom.dom(), space_root, options)?;
    materializer.run(&mut report)?;
    write_import_readme(space_root);
    Ok(report)
}

/// The note an imported Space carries at its root, `README.md`: its lengths
/// are Roblox studs, its scripts work in studs through the script boundary,
/// and its gravity is Roblox's. Written only when the Space has no README,
/// so a user's own is never replaced.
fn write_import_readme(space_root: &Path) {
    // Roblox's standard gravity, studs/s².
    const ROBLOX_GRAVITY_STUDS: f64 = 196.2;
    let path = space_root.join("README.md");
    if path.exists() {
        return;
    }
    let stud = eustress_common::units::Unit::Stud.to_meters();
    let text = format!(
        "# Imported from Roblox\n\n\
         This Space was imported from a Roblox place. Its lengths are in Roblox studs \
         (1 stud = {stud} m); Studio shows metres by default and studs when the unit menu \
         is set to Studs.\n\n\
         Its scripts are marked `origin = \"roblox\"` and work in studs: lengths they read or \
         write on parts, models, attachments, the camera, the mouse and in spatial calls \
         convert at the script boundary. Values a script hands another script directly \
         (attributes, RemoteEvents, BindableEvents, ModuleScripts, value objects) keep the \
         units they were written in, so a native Eustress script reading them gets studs.\n\n\
         Its gravity is Roblox's, {g} studs/s² ({gm:.1} m/s²), so imported scripts that \
         assume it match the physics.\n",
        g = ROBLOX_GRAVITY_STUDS,
        gm = ROBLOX_GRAVITY_STUDS * stud,
    );
    let _ = std::fs::write(&path, text);
}

/// Set `[space] transform_rule = "parent_pose"` in the Space's `space.toml`,
/// creating the file when there is none and keeping everything else in it.
/// An imported instance's `[transform]` is relative to its parent's pose.
fn mark_parent_pose_rule(space_root: &Path) {
    let path = space_root.join("space.toml");
    let mut doc: toml::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or_else(|| toml::Value::Table(toml::value::Table::new()));
    if let Some(root) = doc.as_table_mut() {
        let space = root
            .entry("space".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(space) = space.as_table_mut() {
            space.insert("transform_rule".to_string(), toml::Value::String("parent_pose".to_string()));
        }
    }
    let _ = std::fs::create_dir_all(space_root);
    if let Ok(text) = toml::to_string_pretty(&doc) {
        let _ = std::fs::write(&path, text);
    }
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
        // Workspace content mirrors the Roblox place EXACTLY — children land
        // directly under `Workspace/`, not a `Workspace/<PlaceName>/` container.
        let cube_path = space_root
            .join("Workspace")
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
    fn workspace_children_mirror_directly_under_workspace() {
        // The Roblox place is mirrored EXACTLY: a Workspace child (the "Group"
        // folder) lands directly under `Workspace/`, NOT wrapped in a synthetic
        // `Workspace/<PlaceName>/` container. Other services stay flat too.
        let dom = make_minimal_place();
        let space_root = make_temp_root("mirror");
        import_into_space(&dom, &space_root, ImportOptions::default()).expect("import");

        let place = place_name_from_root(&space_root).expect("temp root has a name");

        // 1. The Workspace child sits directly under `Workspace/`.
        assert!(
            space_root
                .join("Workspace")
                .join("Group")
                .join("Cube")
                .join("_instance.toml")
                .is_file(),
            "Cube must mirror to Workspace/Group/Cube"
        );

        // 2. There is NO synthetic place-named container folder.
        assert!(
            !space_root.join("Workspace").join(&place).exists(),
            "Workspace must NOT wrap children in a Workspace/<PlaceName>/ container"
        );

        // 3. Other services keep their flat cognate root (Lighting unchanged).
        assert!(
            space_root
                .join("Lighting")
                .join("Sky")
                .join("_instance.toml")
                .is_file(),
            "Lighting children must stay directly under Lighting/"
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
        let first = std::fs::read_to_string(
            space_root
                .join("Workspace")
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
        let second = std::fs::read_to_string(
            space_root2
                .join("Workspace")
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
        // mirrors the Roblox place directly under `Workspace/`.)
        assert!(!space_root
            .join("Workspace")
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
        // The Terrain folder + TOML should exist (directly under Workspace).
        let terrain_toml = space_root
            .join("Workspace")
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
        let chunk = space_root
            .join("Workspace")
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

        let csg_dir = space_root.join("Workspace").join("Carved");
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
        assert!(space_root
            .join("Workspace")
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
        // File header `[version, log2(chunk_edge)]`, then one chunk whose
        // coordinate is its delta from the origin, stored as three
        // byte-plane-interleaved big-endian i32s (see terrain::read_chunk_delta).
        let mut buf = vec![crate::terrain::SMOOTH_GRID_VERSION, 0x05];
        let (bx, by, bz) = (cx.to_be_bytes(), cy.to_be_bytes(), cz.to_be_bytes());
        for plane in 0..4 {
            buf.push(bx[plane]);
            buf.push(by[plane]);
            buf.push(bz[plane]);
        }
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

    /// Serves the same `.mesh` bytes for every id.
    struct FixedMesh(Vec<u8>);
    impl crate::asset_resolver::AssetFetcher for FixedMesh {
        fn fetch(&self, _id: u64) -> Result<Vec<u8>, String> {
            Ok(self.0.clone())
        }
    }

    fn read_top_level_vec3(toml_path: &std::path::Path, key: &str) -> Option<[f64; 3]> {
        let doc: toml::Value = std::fs::read_to_string(toml_path).ok()?.parse().ok()?;
        let a = doc.get(key)?.as_array()?;
        let f = |v: &toml::Value| v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
        Some([f(&a[0])?, f(&a[1])?, f(&a[2])?])
    }

    /// A legacy SpecialMesh FileMesh draws at its NATIVE size times `Scale`,
    /// ignoring the part's `Size`; a MeshPart stretches its mesh to fill `Size`.
    /// Meshes are now written unit-sized and the engine multiplies them by
    /// `Size`, so the FileMesh's visual scale must be native * Scale / Size per
    /// axis, and a MeshPart must get none. Extents, Scale and Size all differ
    /// per axis so a transposed or unit-less result cannot pass.
    #[test]
    fn file_mesh_draws_at_native_size_times_scale() {
        use rbx_dom_weak::types::{Content, Enum, Vector3};
        // native extent 4 x 2 x 6
        let mesh = crate::roblox_mesh::make_v2_mesh_fixture(&[
            [0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 2.0, 6.0],
        ]);
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace")
                .with_child(
                    InstanceBuilder::new("Part")
                        .with_name("Lamp")
                        .with_property("Size", Vector3::new(2.0, 1.0, 4.0))
                        .with_child(
                            InstanceBuilder::new("SpecialMesh")
                                .with_name("Mesh")
                                .with_property("MeshType", Enum::from_u32(5))
                                .with_property("MeshId", Content::from("rbxassetid://42"))
                                .with_property("Scale", Vector3::new(2.0, 3.0, 0.5)),
                        ),
                )
                .with_child(
                    InstanceBuilder::new("MeshPart")
                        .with_name("Wheel")
                        .with_property("Size", Vector3::new(3.0, 3.0, 3.0))
                        .with_property("MeshId", Content::from("rbxassetid://42")),
                ),
        );
        let rbx = RobloxDom::from_dom(
            WeakDom::new(dm),
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );
        let space_root = make_temp_root("file_mesh_scale");
        let opts = ImportOptions {
            asset_fetcher: Some(std::sync::Arc::new(FixedMesh(mesh))),
            ..Default::default()
        };
        import_into_space(&rbx, &space_root, opts).expect("import");

        let lamp = space_root.join("Workspace").join("Lamp").join("_instance.toml");
        let got = read_top_level_vec3(&lamp, "mesh_scale").expect("FileMesh part needs mesh_scale");
        // native * Scale / Size = [4*2/2, 2*3/1, 6*0.5/4]
        let want = [4.0, 6.0, 0.75];
        for a in 0..3 {
            assert!((got[a] - want[a]).abs() < 1e-4, "axis {a}: got {:?}, want {want:?}", got);
        }

        let wheel = space_root.join("Workspace").join("Wheel").join("_instance.toml");
        assert!(
            read_top_level_vec3(&wheel, "mesh_scale").is_none(),
            "a MeshPart stretches to its Size and must not get a compensating mesh_scale"
        );
        let _ = std::fs::remove_dir_all(&space_root);
    }

    /// Every property no handler maps is reported, aggregated per Roblox
    /// (class, property, type) with a count. This was dead plumbing, so the
    /// report could not say which data an import carried but never used.
    #[test]
    fn unmapped_properties_are_reported_with_counts() {
        let part = |name: &str| {
            InstanceBuilder::new("Part")
                .with_name(name)
                .with_property("TotallyUnmappedProp", 7i32)
        };
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace").with_child(part("A")).with_child(part("B")),
        );
        let rbx = RobloxDom::from_dom(
            WeakDom::new(dm),
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );
        let space_root = make_temp_root("unmapped_props");
        let report = import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        let hit: Vec<_> = report
            .unmapped_properties
            .iter()
            .filter(|u| u.property == "TotallyUnmappedProp")
            .collect();
        assert_eq!(hit.len(), 1, "one aggregated row, got {:?}", report.unmapped_properties);
        assert_eq!(hit[0].class, "Part");
        assert_eq!(hit[0].count, 2);
        let _ = std::fs::remove_dir_all(&space_root);
    }

    /// CollectionService tags land in the document's root `tags` array, the
    /// key the engine's loader and the shared record conversion read.
    #[test]
    fn tags_are_written_where_the_engine_reads_them() {
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace").with_child(
                InstanceBuilder::new("Part").with_name("Tagged").with_property(
                    "Tags",
                    rbx_dom_weak::types::Tags::from(vec!["car".to_string(), "sfx".to_string()]),
                ),
            ),
        );
        let rbx = RobloxDom::from_dom(
            WeakDom::new(dm),
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );
        let space_root = make_temp_root("root_tags");
        import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        let raw = std::fs::read_to_string(space_root.join("Workspace/Tagged/_instance.toml"))
            .expect("the part's toml");
        let doc: toml::Value = raw.parse().expect("parse");
        let tags: Vec<&str> = doc
            .get("tags")
            .and_then(|t| t.as_array())
            .expect("root tags array")
            .iter()
            .filter_map(|t| t.as_str())
            .collect();
        assert_eq!(tags, vec!["car", "sfx"]);
        assert!(
            doc.get("metadata").and_then(|m| m.get("tags")).is_none(),
            "no second copy under [metadata]"
        );
        let _ = std::fs::remove_dir_all(&space_root);
    }

    /// A value object with children stays as a Folder for them; its own
    /// value is still its parent's attribute.
    #[test]
    fn a_value_object_with_children_keeps_them() {
        use rbx_dom_weak::types::{Variant, Vector3};
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace").with_child(
                InstanceBuilder::new("Folder").with_name("Handling").with_child(
                    InstanceBuilder::new("NumberValue")
                        .with_name("Torque")
                        .with_property("Value", Variant::Float64(500.0))
                        .with_child(
                            InstanceBuilder::new("Vector3Value")
                                .with_name("Location")
                                .with_property("Value", Variant::Vector3(Vector3::new(1.0, 2.0, 3.0))),
                        ),
                ),
            ),
        );
        let rbx = RobloxDom::from_dom(
            WeakDom::new(dm),
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );
        let space_root = make_temp_root("vo_children");
        import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        let read = |rel: &str| -> toml::Value {
            std::fs::read_to_string(space_root.join(rel)).expect(rel).parse().expect("parse")
        };
        let handling = read("Workspace/Handling/_instance.toml");
        assert_eq!(handling["attributes"]["Torque"].as_float(), Some(500.0), "the value folds into the parent");
        let torque = read("Workspace/Handling/Torque/_instance.toml");
        assert_eq!(torque["metadata"]["class_name"].as_str(), Some("Folder"));
        assert_eq!(
            torque["attributes"]["Location"].as_array().map(|a| a.len()),
            Some(3),
            "its child folds into it"
        );
        let _ = std::fs::remove_dir_all(&space_root);
    }

    /// Under the `ParentPose` rule a part nested in a part is written
    /// relative to its parent's pose, and the Space says so in space.toml.
    #[test]
    fn a_nested_part_is_written_relative_to_its_parent() {
        use rbx_dom_weak::types::{CFrame, Matrix3, Variant, Vector3};
        // Parent at (10, 0, 0), turned +90 degrees about Y (columns: right,
        // up, back; rows are what rbx_types stores).
        let turned = Matrix3::new(
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(-1.0, 0.0, 0.0),
        );
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace").with_child(
                InstanceBuilder::new("Part")
                    .with_name("Pad")
                    .with_property("CFrame", Variant::CFrame(CFrame::new(Vector3::new(10.0, 0.0, 0.0), turned)))
                    .with_child(
                        InstanceBuilder::new("Part").with_name("Ignore").with_property(
                            "CFrame",
                            Variant::CFrame(CFrame::new(Vector3::new(10.0, 0.0, 5.0), Matrix3::identity())),
                        ),
                    ),
            ),
        );
        let rbx = RobloxDom::from_dom(
            WeakDom::new(dm),
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );
        let space_root = make_temp_root("parent_pose");
        import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        let read = |rel: &str| -> toml::Value {
            std::fs::read_to_string(space_root.join(rel)).expect(rel).parse().expect("parse")
        };
        let pos = |doc: &toml::Value| -> Vec<f64> {
            doc["transform"]["position"].as_array().unwrap().iter().map(|v| v.as_float().unwrap()).collect()
        };
        let pad = read("Workspace/Pad/_instance.toml");
        assert!((pos(&pad)[0] - 10.0).abs() < 1e-4, "a top-level part keeps its world pose");
        let ignore = read("Workspace/Pad/Ignore/_instance.toml");
        let local = pos(&ignore);
        // 5 studs along world +Z from the pad is 5 along the pad's local -X.
        assert!((local[0] + 5.0).abs() < 1e-3 && local[1].abs() < 1e-3 && local[2].abs() < 1e-3, "{local:?}");
        let space = read("space.toml");
        assert_eq!(space["space"]["transform_rule"].as_str(), Some("parent_pose"));
        let _ = std::fs::remove_dir_all(&space_root);
    }

    /// An Animation keeps its id where the engine reads it, a
    /// KeyframeSequence is one record with its keyframes inline, and a
    /// fetched clip is named in the id map the runtime reads.
    #[test]
    fn animations_import_as_clips_the_runtime_finds() {
        use rbx_dom_weak::types::{ContentId, Enum};
        let clip = WeakDom::new(
            InstanceBuilder::new("KeyframeSequence").with_name("Wave").with_child(
                InstanceBuilder::new("Keyframe")
                    .with_property("Time", Variant::Float32(0.5))
                    .with_child(InstanceBuilder::new("Pose").with_name("HumanoidRootPart")),
            ),
        );
        let mut clip_bytes = Vec::new();
        rbx_binary::to_writer(&mut clip_bytes, &clip, &[clip.root_ref()]).expect("clip model");
        struct Clips(Vec<u8>);
        impl crate::asset_resolver::AssetFetcher for Clips {
            fn fetch(&self, id: u64) -> Result<Vec<u8>, String> {
                if id == 507770239 {
                    Ok(self.0.clone())
                } else {
                    Err("HTTP 401".to_string())
                }
            }
        }
        let dm = InstanceBuilder::new("DataModel")
            .with_child(
                InstanceBuilder::new("Workspace").with_child(
                    InstanceBuilder::new("Animation")
                        .with_name("Wave")
                        .with_property("AnimationId", Variant::ContentId(ContentId::from("rbxassetid://507770239"))),
                ),
            )
            .with_child(
                InstanceBuilder::new("ReplicatedStorage").with_child(
                    InstanceBuilder::new("KeyframeSequence")
                        .with_name("Idle")
                        .with_property("Priority", Variant::Enum(Enum::from_u32(0)))
                        .with_child(
                            InstanceBuilder::new("Keyframe").with_property("Time", Variant::Float32(1.0)).with_child(
                                InstanceBuilder::new("Pose")
                                    .with_name("HumanoidRootPart")
                                    .with_child(InstanceBuilder::new("Pose").with_name("LowerTorso")),
                            ),
                        ),
                ),
            );
        let rbx = RobloxDom::from_dom(
            WeakDom::new(dm),
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );
        let space_root = make_temp_root("animations");
        let options = ImportOptions {
            asset_fetcher: Some(std::sync::Arc::new(Clips(clip_bytes))),
            ..ImportOptions::default()
        };
        let report = import_into_space(&rbx, &space_root, options).expect("import");
        let text = |rel: &str| std::fs::read_to_string(space_root.join(rel)).expect(rel);

        let animation: toml::Value = text("Workspace/Wave/_instance.toml").parse().expect("parse");
        assert_eq!(animation["properties"]["animation_id"].as_str(), Some("rbxassetid://507770239"));
        assert_eq!(eustress_common::datamodel::record::record_animation_id(&animation).as_deref(), Some("rbxassetid://507770239"));

        let idle_text = text("ReplicatedStorage/Idle/_instance.toml");
        let idle: toml::Value = idle_text.parse().expect("parse");
        assert_eq!(idle["keyframe_sequence"]["priority"].as_str(), Some("Idle"));
        assert_eq!(idle["keyframes"][0]["poses"]["LowerTorso"]["parent"].as_str(), Some("HumanoidRootPart"));
        let (sequence, problems) = eustress_common::animation::clip::parse_sequence(&idle_text, "Idle").expect("clip");
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(sequence.keyframes.len(), 1);
        assert!(!space_root.join("ReplicatedStorage/Idle/Keyframe").exists(), "keyframes are inline");
        assert!(report.unmapped_classes.iter().all(|u| u.roblox_class != "Keyframe"));
        assert_eq!(report.animation_sequences, 1);

        assert_eq!(report.animation_clips, 1, "{:?}", report.animation_ids_missing);
        let file = eustress_common::animation::content::roblox_id_file(&space_root, 507770239).expect("mapped");
        assert_eq!(file, "assets/animations/rbx-507770239.anim.toml");
        let (fetched, problems) = eustress_common::animation::clip::parse_sequence(&text(&file), "x").expect("clip");
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(fetched.keyframes[0].time, 0.5);
        let _ = std::fs::remove_dir_all(&space_root);
    }

    /// A Roblox cylinder part fills Roblox's box on the engine's turned axes,
    /// and a cylinder or ball with no DataMesh child draws at its smallest
    /// round side, as Roblox draws it. One with a DataMesh child keeps its
    /// whole box, and one a FileMesh draws keeps Roblox's pose and size.
    #[test]
    fn cylinder_and_ball_parts_keep_their_roblox_shape() {
        let part = |name: &str, shape: u32, size: Vector3| {
            InstanceBuilder::new("Part")
                .with_name(name)
                .with_property("Shape", rbx_dom_weak::types::Enum::from_u32(shape))
                .with_property("Size", size)
        };
        let file_mesh = InstanceBuilder::new("SpecialMesh")
            .with_name("Mesh")
            .with_property("MeshType", rbx_dom_weak::types::Enum::from_u32(5))
            .with_property(
                "MeshId",
                rbx_dom_weak::types::Variant::ContentId(rbx_dom_weak::types::ContentId::from("rbxassetid://123456")),
            );
        let lopsided = Vector3::new(60.0, 40.0, 30.0);
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace")
                .with_child(part("Helipad", 2, Vector3::new(1.4, 30.0, 30.0)))
                .with_child(part("Lights", 2, lopsided))
                .with_child(part("Lamp", 0, Vector3::new(6.5, 1.25, 7.75)))
                .with_child(part("Hub", 2, lopsided).with_child(InstanceBuilder::new("BlockMesh").with_name("Mesh")))
                .with_child(part("Knob", 2, lopsided).with_child(file_mesh)),
        );
        let rbx = RobloxDom::from_dom(WeakDom::new(dm), crate::parser::RobloxFormat::BinaryPlace, PathBuf::new());
        let space_root = make_temp_root("cylinder_ball_sizes");
        let report = import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        let transform = |name: &str, key: &str| -> Vec<f64> {
            let path = space_root.join("Workspace").join(name).join("_instance.toml");
            let doc: toml::Value = std::fs::read_to_string(&path).expect("part file").parse().expect("parses");
            doc.get("transform")
                .and_then(|t| t.get(key))
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_float()).collect())
                .unwrap_or_else(|| if key == "rotation" { vec![0.0, 0.0, 0.0, 1.0] } else { panic!("{name} has no {key}") })
        };
        let close = |got: Vec<f64>, want: &[f64]| {
            assert!(
                got.len() == want.len() && got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-4),
                "{got:?} vs {want:?}"
            );
        };
        // Roblox's length (Size.X) is the mesh's Y.
        close(transform("Helipad", "scale"), &[30.0, 1.4, 30.0]);
        // The smaller of Size.Y and Size.Z; a ball's smallest side.
        close(transform("Lights", "scale"), &[30.0, 60.0, 30.0]);
        close(transform("Lamp", "scale"), &[1.25, 1.25, 1.25]);
        // A DataMesh child's look is scaled by the whole turned box.
        close(transform("Hub", "scale"), &[40.0, 60.0, 30.0]);
        // A FileMesh draws the Knob: Roblox's pose and size.
        close(transform("Knob", "scale"), &[60.0, 40.0, 30.0]);
        close(transform("Knob", "rotation"), &[0.0, 0.0, 0.0, 1.0]);
        let notes = report.approximations.iter().filter(|a| a.reason.contains("as Roblox draws it")).count();
        assert_eq!(notes, 2, "the Lights and the Lamp are noted");
        let _ = std::fs::remove_dir_all(&space_root);
    }

    /// An imported Space explains itself in a README.md, and a README the
    /// Space already has is kept.
    #[test]
    fn an_imported_space_gets_a_readme_and_keeps_its_own() {
        let place = || {
            let dm = InstanceBuilder::new("DataModel").with_child(InstanceBuilder::new("Workspace"));
            RobloxDom::from_dom(WeakDom::new(dm), crate::parser::RobloxFormat::BinaryPlace, PathBuf::new())
        };
        let space_root = make_temp_root("readme");
        import_into_space(&place(), &space_root, ImportOptions::default()).expect("import");
        let text = std::fs::read_to_string(space_root.join("README.md")).expect("a README");
        assert!(text.starts_with("# Imported from Roblox"), "{text}");
        assert!(text.contains("origin = \"roblox\"") && text.contains("studs/s"), "{text}");
        let _ = std::fs::remove_dir_all(&space_root);

        let space_root = make_temp_root("readme_kept");
        std::fs::create_dir_all(&space_root).unwrap();
        std::fs::write(space_root.join("README.md"), "mine").unwrap();
        import_into_space(&place(), &space_root, ImportOptions::default()).expect("import");
        assert_eq!(std::fs::read_to_string(space_root.join("README.md")).unwrap(), "mine");
        let _ = std::fs::remove_dir_all(&space_root);
    }

    /// A Roblox Sky's sun and moon sizes land on the Space's Sun and Moon,
    /// scaled from Roblox's defaults, and the Sky keeps neither.
    #[test]
    fn the_sky_sizes_the_sun_and_moon() {
        assert_eq!(celestial_size(21.0, ROBLOX_SUN_ANGULAR_SIZE, SUN_ANGULAR_SIZE), 8.0, "Roblox's default is ours");
        assert_eq!(celestial_size(11.0, ROBLOX_MOON_ANGULAR_SIZE, MOON_ANGULAR_SIZE), 2.0, "the Moon's too");
        assert!((celestial_size(10.5, 21.0, 8.0) - 4.0).abs() < 1e-12, "half Roblox's is half ours");
        assert_eq!(celestial_size(1e6, 21.0, 8.0), 20.0);
        assert_eq!(celestial_size(0.0, 11.0, 2.0), 0.05);

        let dm = InstanceBuilder::new("DataModel")
            .with_child(InstanceBuilder::new("Workspace"))
            .with_child(
                InstanceBuilder::new("Lighting").with_child(
                    InstanceBuilder::new("Sky")
                        .with_property("SunAngularSize", Variant::Float32(42.0))
                        .with_property("MoonAngularSize", Variant::Float32(11.0)),
                ),
            );
        let rbx = RobloxDom::from_dom(WeakDom::new(dm), crate::parser::RobloxFormat::BinaryPlace, PathBuf::new());
        let space_root = make_temp_root("sky_sizes");
        import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        let read = |rel: &str| -> toml::Value {
            std::fs::read_to_string(space_root.join(rel)).expect(rel).parse().expect("parse")
        };
        let sun = read("Lighting/Sun.instance.toml");
        assert_eq!(sun["metadata"]["class_name"].as_str(), Some("Star"));
        let size = sun["star"]["angular_size"].as_float().unwrap();
        assert!((size - 16.0).abs() < 1e-9, "twice Roblox's default is twice ours: {size}");
        assert!(sun["star"].get("intensity").is_some(), "the template's other keys are kept");
        let moon = read("Lighting/Moon.instance.toml");
        assert!((moon["moon"]["angular_size"].as_float().unwrap() - 2.0).abs() < 1e-9);
        // The engine's reader sees the same sizes.
        let read_sun = eustress_common::plugins::celestial_sections::sun_from_section(
            sun.get("star"),
            eustress_common::plugins::celestial_sections::default_sun(),
        );
        assert!((read_sun.angular_size as f64 - 16.0).abs() < 1e-5, "{}", read_sun.angular_size);
        let sky_text = std::fs::read_dir(space_root.join("Lighting"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("Sky"))
            .map(|e| {
                let p = e.path();
                let file = if p.is_dir() { p.join("_instance.toml") } else { p };
                std::fs::read_to_string(file).unwrap_or_default()
            })
            .collect::<String>();
        assert!(!sky_text.contains("angular_size"), "the Sky holds neither size");
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn attributes_are_written_where_the_engine_reads_them() {
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace").with_child(
                InstanceBuilder::new("Part")
                    .with_name("Car")
                    .with_property(
                        "Attributes",
                        rbx_dom_weak::types::Attributes::new()
                            .with("Speed", rbx_dom_weak::types::Variant::Float64(12.5)),
                    )
                    .with_child(
                        InstanceBuilder::new("NumberValue")
                            .with_name("Fuel")
                            .with_property("Value", rbx_dom_weak::types::Variant::Float64(40.0)),
                    ),
            ),
        );
        let rbx = RobloxDom::from_dom(
            WeakDom::new(dm),
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );
        let space_root = make_temp_root("root_attributes");
        import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        let raw = std::fs::read_to_string(space_root.join("Workspace/Car/_instance.toml"))
            .expect("the part's toml");
        let doc: toml::Value = raw.parse().expect("parse");
        let attrs = doc.get("attributes").and_then(|a| a.as_table()).expect("root [attributes]");
        assert_eq!(attrs.get("Speed").and_then(|v| v.as_float()), Some(12.5), "a Roblox attribute");
        assert_eq!(attrs.get("Fuel").and_then(|v| v.as_float()), Some(40.0), "a folded value object");
        assert!(
            doc.get("properties").and_then(|p| p.get("attributes")).is_none(),
            "no copy under [properties], which nothing reads"
        );
        let _ = std::fs::remove_dir_all(&space_root);
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

    /// Import a single Workspace Part carrying one DataMesh child and
    /// return `(space_root, parsed parent TOML, report)`. The parent
    /// lands at `Workspace/MeshHost/_instance.toml`.
    fn import_part_with_mesh_child(
        prefix: &str,
        child: InstanceBuilder,
    ) -> (PathBuf, toml::Value, ImportReport) {
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace").with_child(
                InstanceBuilder::new("Part")
                    .with_name("MeshHost")
                    .with_property("Size", Vector3::new(4.0, 1.0, 2.0))
                    .with_child(child),
            ),
        );
        let dom = WeakDom::new(dm);
        let rbx = RobloxDom::from_dom(
            dom,
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );
        let space_root = make_temp_root(prefix);
        let report =
            import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        let host_dir = space_root.join("Workspace").join("MeshHost");
        let raw = std::fs::read_to_string(host_dir.join("_instance.toml"))
            .expect("parent TOML exists");
        let doc: toml::Value = raw.parse().expect("parent TOML parses");
        // The folded child must never materialise as its own instance.
        assert!(
            !host_dir.join("Mesh").exists(),
            "DataMesh child must not spawn standalone"
        );
        (space_root, doc, report)
    }

    fn asset_mesh_of(doc: &toml::Value) -> String {
        doc.get("asset")
            .and_then(|a| a.get("mesh"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    /// Import a Workspace Part (Shape `shape`, when given) carrying one
    /// primitive DataMesh child, which stays a child. Returns
    /// `(space_root, the part's TOML, the child's TOML)`.
    fn import_part_keeping_mesh_child(
        prefix: &str,
        shape: Option<u32>,
        child: InstanceBuilder,
    ) -> (PathBuf, toml::Value, toml::Value) {
        let mut part = InstanceBuilder::new("Part")
            .with_name("MeshHost")
            .with_property("Size", Vector3::new(4.0, 1.0, 2.0));
        if let Some(s) = shape {
            part = part.with_property("Shape", rbx_dom_weak::types::Enum::from_u32(s));
        }
        let dm = InstanceBuilder::new("DataModel").with_child(InstanceBuilder::new("Workspace").with_child(part.with_child(child)));
        let rbx = RobloxDom::from_dom(WeakDom::new(dm), crate::parser::RobloxFormat::BinaryPlace, PathBuf::new());
        let space_root = make_temp_root(prefix);
        import_into_space(&rbx, &space_root, ImportOptions::default()).expect("import");
        let host_dir = space_root.join("Workspace").join("MeshHost");
        let read = |p: PathBuf| -> toml::Value {
            std::fs::read_to_string(&p).unwrap_or_else(|_| panic!("{} exists", p.display())).parse().expect("TOML parses")
        };
        let part_doc = read(host_dir.join("_instance.toml"));
        let child_doc = read(host_dir.join("Mesh").join("_instance.toml"));
        (space_root, part_doc, child_doc)
    }

    fn mesh_section<'a>(doc: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
        doc.get("mesh").and_then(|m| m.get(key))
    }

    fn floats(v: Option<&toml::Value>) -> Vec<f64> {
        v.and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_float()).collect()).unwrap_or_default()
    }

    #[test]
    fn a_special_mesh_brick_stays_a_child_and_the_part_keeps_its_shape() {
        let child = InstanceBuilder::new("SpecialMesh")
            .with_name("Mesh")
            .with_property("MeshType", rbx_dom_weak::types::Enum::from_u32(6)) // Brick
            .with_property("Scale", Vector3::new(2.0, 3.0, 4.0));
        let (space_root, part, mesh) = import_part_keeping_mesh_child("specialmesh_brick", None, child);
        // A block part carries no [asset]: both apps load it as the block.
        assert_eq!(asset_mesh_of(&part), "", "the part keeps its own block shape");
        assert!(part.get("mesh_scale").is_none(), "the look is the child's, not the part's");
        assert_eq!(mesh.get("metadata").and_then(|m| m.get("class_name")).and_then(|v| v.as_str()), Some("SpecialMesh"));
        assert_eq!(mesh_section(&mesh, "mesh_type").and_then(|v| v.as_str()), Some("Brick"));
        assert_eq!(floats(mesh_section(&mesh, "scale")), vec![2.0, 3.0, 4.0]);
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn a_ball_wheel_keeps_its_ball_under_a_cylinder_special_mesh() {
        // Vehicle Simulator's wheels: a Ball part drawn as a disc.
        let child = InstanceBuilder::new("SpecialMesh")
            .with_name("Mesh")
            .with_property("MeshType", rbx_dom_weak::types::Enum::from_u32(4)) // Cylinder
            .with_property("Scale", Vector3::new(0.33, 1.0, 1.0));
        let (space_root, part, mesh) = import_part_keeping_mesh_child("ball_wheel", Some(0), child);
        assert_eq!(asset_mesh_of(&part), "parts/ball.glb", "the wheel collides as its ball");
        assert_eq!(mesh_section(&mesh, "mesh_type").and_then(|v| v.as_str()), Some("Cylinder"));
        assert!((floats(mesh_section(&mesh, "scale"))[0] - 0.33).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn every_mesh_type_keeps_its_name_and_offset_is_metres() {
        let child = InstanceBuilder::new("SpecialMesh")
            .with_name("Mesh")
            .with_property("MeshType", rbx_dom_weak::types::Enum::from_u32(2)) // Wedge
            .with_property("Offset", Vector3::new(0.0, 10.0, 0.0));
        let (space_root, _part, mesh) = import_part_keeping_mesh_child("specialmesh_wedge", None, child);
        assert_eq!(mesh_section(&mesh, "mesh_type").and_then(|v| v.as_str()), Some("Wedge"));
        let stud = eustress_common::units::Unit::Stud.to_meters();
        assert!((floats(mesh_section(&mesh, "offset"))[1] - 10.0 * stud).abs() < 1e-5, "offsets are metres");
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn a_data_mesh_on_a_cylinder_part_keeps_roblox_values() {
        let child = InstanceBuilder::new("SpecialMesh")
            .with_name("Mesh")
            .with_property("MeshType", rbx_dom_weak::types::Enum::from_u32(6))
            .with_property("Scale", Vector3::new(1.0, 2.0, 3.0));
        let (space_root, _part, mesh) = import_part_keeping_mesh_child("cylinder_part_mesh", Some(2), child);
        assert_eq!(floats(mesh_section(&mesh, "scale")), vec![1.0, 2.0, 3.0]);
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn special_mesh_filemesh_routes_mesh_ref_to_parent() {
        let child = InstanceBuilder::new("SpecialMesh")
            .with_name("Mesh")
            .with_property("MeshType", rbx_dom_weak::types::Enum::from_u32(5)) // FileMesh
            .with_property(
                "MeshId",
                rbx_dom_weak::types::Variant::ContentId(rbx_dom_weak::types::ContentId::from(
                    "rbxassetid://123456",
                )),
            )
            .with_property("Offset", Vector3::new(0.0, 1.5, 0.0));
        let (space_root, doc, _report) =
            import_part_with_mesh_child("specialmesh_filemesh", child);
        // No fetcher configured → the parent's [asset].mesh carries the
        // unresolved placeholder for the CHILD's MeshId.
        let mesh = asset_mesh_of(&doc).replace('\\', "/");
        assert!(
            mesh.contains("_unresolved") && mesh.contains("123456"),
            "parent [asset].mesh should carry the folded mesh ref placeholder, got {mesh:?}"
        );
        let offset: Vec<f64> = doc
            .get("mesh_offset")
            .and_then(|v| v.as_array())
            .expect("top-level mesh_offset written")
            .iter()
            .filter_map(|v| v.as_float())
            .collect();
        assert_eq!(offset, vec![0.0, 1.5, 0.0]);
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn block_and_cylinder_meshes_stay_children() {
        let block_child = InstanceBuilder::new("BlockMesh")
            .with_name("Mesh")
            .with_property("Scale", Vector3::new(0.5, 0.5, 0.5));
        let (root_a, part_a, mesh_a) = import_part_keeping_mesh_child("blockmesh", None, block_child);
        assert_eq!(asset_mesh_of(&part_a), "", "a block part keeps its block (no [asset])");
        assert_eq!(mesh_a.get("metadata").and_then(|m| m.get("class_name")).and_then(|v| v.as_str()), Some("BlockMesh"));
        assert_eq!(floats(mesh_section(&mesh_a, "scale")), vec![0.5, 0.5, 0.5]);
        let _ = std::fs::remove_dir_all(&root_a);

        let cyl_child = InstanceBuilder::new("CylinderMesh").with_name("Mesh");
        let (root_b, part_b, mesh_b) = import_part_keeping_mesh_child("cylindermesh", None, cyl_child);
        assert_eq!(asset_mesh_of(&part_b), "", "a checkpoint collides as its block (no [asset])");
        assert_eq!(mesh_b.get("metadata").and_then(|m| m.get("class_name")).and_then(|v| v.as_str()), Some("CylinderMesh"));
        let _ = std::fs::remove_dir_all(&root_b);
    }
}

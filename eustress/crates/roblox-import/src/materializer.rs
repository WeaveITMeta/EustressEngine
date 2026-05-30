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
//! Terrain + CSG instances are emitted as plain placeholders with an
//! `ImportReport::approximations` entry — the full decoders land in
//! Wave 4.A.2 (terrain) and Wave 4.A.3 (CSG mesh extraction). See spec
//! sections §6 and §7 for the deferred work.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eustress_common::classes::ClassName;
use eustress_common::instance_create::{
    create_instance, fresh_uuid_for_create, is_valid_uuid, uuid_bytes_to_hex,
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

// ---------------------------------------------------------------------------
// ImportOptions — public knobs per spec §15
// ---------------------------------------------------------------------------

/// Knobs that control how a single import call behaves. See spec §15.
#[derive(Clone)]
pub struct ImportOptions {
    /// Service routing rules. Default `ServiceRouter::new(space_root)`
    /// covers every standard Roblox service.
    pub service_router: Option<ServiceRouter>,

    /// Whether to decode SmoothGrid voxel data (§6). Default: true. The
    /// actual decoder is deferred to Wave 4.A.2 — Wave 4.A.1 just emits
    /// an `Approximation` entry no matter what this is set to.
    pub import_terrain: bool,

    /// Whether to extract baked CSG MeshData (§7.1). Default: true.
    /// Deferred to Wave 4.A.2 — Wave 4.A.1 emits an `Approximation`.
    pub extract_csg_baked: bool,

    /// Whether to re-execute CSG from ChildData when MeshData is absent
    /// (§7.2). Default: true. Deferred to Wave 4.A.2.
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
        }
    }
}

impl std::fmt::Debug for ImportOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportOptions")
            .field("service_router", &self.service_router.is_some())
            .field("import_terrain", &self.import_terrain)
            .field("extract_csg_baked", &self.extract_csg_baked)
            .field("recompute_csg_when_missing", &self.recompute_csg_when_missing)
            .field("transform_scripts", &self.transform_scripts)
            .field("space_salt", &self.space_salt.as_ref().map(|v| v.len()))
            .field("unit_symbol", &self.unit_symbol)
            .finish()
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

impl<'dom> Materializer<'dom> {
    /// Construct a materializer for a `RobloxDom` + target Space.
    pub fn new(
        dom: &'dom WeakDom,
        space_root: &Path,
        opts: ImportOptions,
    ) -> Result<Self, ImportError> {
        if !space_root.exists() {
            std::fs::create_dir_all(space_root).map_err(|e| {
                ImportError::Io(space_root.to_path_buf(), e)
            })?;
        }
        let router = opts
            .service_router
            .clone()
            .unwrap_or_else(|| ServiceRouter::new(space_root.to_path_buf()));
        let salt = opts
            .space_salt
            .clone()
            .unwrap_or_else(|| derive_space_salt(space_root));
        Ok(Self {
            dom,
            space_root: space_root.to_path_buf(),
            router: Arc::new(router),
            opts,
            salt,
            referent_to_uuid: HashMap::new(),
            referent_to_path: HashMap::new(),
            pending_refs: Vec::new(),
            pending_patches: HashMap::new(),
        })
    }

    /// Walk the entire DOM, populating `report` as we go.
    pub fn run(mut self, report: &mut ImportReport) -> Result<(), ImportError> {
        let start = std::time::Instant::now();

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
            self.walk_subtree(root_ref, &dest, "Workspace", report)?;
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

        let outcome = self.router.route(service.class.as_str())?;
        match outcome {
            RouteOutcome::Routed { dest, cognate } => {
                if !cognate {
                    report.record_skipped_service(
                        &service.class,
                        &format!("no Eustress cognate — children routed to {}", dest.display()),
                    );
                }
                let absolute_dest = self.router.absolute(&dest);
                std::fs::create_dir_all(&absolute_dest).map_err(|e| {
                    ImportError::Io(absolute_dest.clone(), e)
                })?;
                let dest_str = dest.to_string_lossy().to_string();
                for child_ref in service.children().iter() {
                    self.walk_subtree(*child_ref, &absolute_dest, &dest_str, report)?;
                }
            }
            RouteOutcome::SkipSilent => {
                // Runtime-only — silently skip subtree.
            }
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
            std::fs::create_dir_all(&dest).map_err(|e| {
                ImportError::Io(dest.clone(), e)
            })?;
            for gc_ref in child.children().iter() {
                self.walk_subtree(*gc_ref, &dest, dest_rel, report)?;
            }
        }
        Ok(())
    }

    fn walk_subtree(
        &mut self,
        node_ref: Ref,
        parent_dir: &Path,
        parent_relpath: &str,
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

        // Map the Roblox class to a Eustress ClassName.
        let Some(eustress_class) = roblox_to_eustress_class(inst.class.as_str()) else {
            report.record_unmapped_class(&inst.class, &inst.name);
            return Ok(());
        };

        // Map properties.
        let bag = map_properties(&inst.properties, eustress_class);

        // Choose the on-disk folder name.
        let requested_name = if inst.name.is_empty() {
            inst.class.clone()
        } else {
            inst.name.clone()
        };

        // For Terrain / CSG, emit an Approximation entry + plain
        // placeholder. The real decode lands in Wave 4.A.2.
        let approx_note = if inst.class == "Terrain" {
            Some(("Terrain — voxel data deferred to Wave 4.A.2".to_string(), "Terrain"))
        } else if matches!(
            inst.class.as_str(),
            "UnionOperation" | "NegateOperation" | "IntersectOperation"
        ) {
            Some((
                format!("CSG ({}) — baked mesh extraction deferred to Wave 4.A.2", inst.class),
                "Part",
            ))
        } else {
            None
        };

        // Create the instance via the canonical pipeline.
        let class_template_name = eustress_class.as_str();
        let mut overrides = bag.overrides.clone();
        overrides.display_name = Some(requested_name.clone());
        if let Some(unit) = &self.opts.unit_symbol {
            overrides.unit_symbol = Some(unit.clone());
        }

        let created = create_instance(
            parent_dir,
            class_template_name,
            Some(&requested_name),
            overrides,
        )
        .map_err(|e| ImportError::InstanceCreate {
            class: class_template_name.to_string(),
            source_msg: e.to_string(),
        })?;

        let entity_relpath = format!("{}/{}", parent_relpath, created.folder_name);
        report.record_imported(class_template_name);

        if created.folder_name != requested_name {
            report.record_name_collision(parent_relpath, &requested_name, &created.folder_name);
        }

        // Stamp a deterministic uuid (overrides the random one the
        // canonical pipeline minted) so re-imports are idempotent.
        let referent = inst.referent();
        let uuid = entity_uuid(&self.salt, &referent.to_string());
        let uuid_hex = uuid_bytes_to_hex(uuid.as_bytes());
        let patch = self.pending_patches.entry(created.toml_path.clone()).or_default();
        patch.uuid_stamp = Some(uuid_hex.clone());

        // Save the referent ↔ uuid + path mapping so refs resolve.
        self.referent_to_uuid.insert(referent, uuid);
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
        // Asset refs → resolver → placeholder path + warning.
        for (prop, uri) in bag.asset_refs {
            let resolved = asset_resolver::resolve(&uri);
            if !resolved.resolved {
                if let Some(reason) = &resolved.reason {
                    report.record_asset_warning(
                        &uri,
                        class_template_name,
                        &prop,
                        reason,
                    );
                }
            }
            // Mesh-class properties point at mesh assets; everything
            // else lands as a single-path asset reference.
            let asset_path_str = resolved.asset_path.to_string_lossy().to_string();
            if is_mesh_property(&prop, eustress_class) {
                patch.asset_mesh = Some(asset_path_str);
            } else {
                patch.asset_path = Some(asset_path_str);
            }
        }

        // Script-source post-processing.
        if let Some(body) = bag.script_source {
            let final_body = if self.opts.transform_scripts {
                let result = ScriptTransformer::transform(&body);
                for warning in &result.warnings {
                    let severity = match warning.severity {
                        WarningSeverity::Info => "info",
                        WarningSeverity::Warning => "warning",
                        WarningSeverity::Error => "error",
                    };
                    report.script_warnings.push(
                        crate::import_report::ScriptWarning {
                            entity_path: entity_relpath.clone(),
                            message: warning.message.clone(),
                            severity: severity.to_string(),
                        },
                    );
                }
                result.source
            } else {
                body
            };
            patch.script_body = Some(final_body);
            patch.script_class = Some(eustress_class);
        }

        // Approximation note (Terrain, CSG).
        if let Some((reason, target)) = approx_note {
            report.record_approximation(&entity_relpath, &inst.class, target, &reason);
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

        // Recurse into children.
        for child_ref in inst.children().iter() {
            self.walk_subtree(*child_ref, &created.folder_path, &entity_relpath, report)?;
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
    let mut doc: toml::Value = raw.parse().map_err(|e: toml::de::Error| {
        ImportError::InstanceCreate {
            class: toml_path.to_string_lossy().to_string(),
            source_msg: format!("toml parse: {e}"),
        }
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
    if !patch.extras.is_empty()
        || !patch.physics.is_empty()
        || !patch.attributes.is_empty()
    {
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
    std::fs::write(toml_path, new_raw)
        .map_err(|e| ImportError::Io(toml_path.to_path_buf(), e))?;

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
        std::fs::write(&script_path, body)
            .map_err(|e| ImportError::Io(script_path, e))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Derive a per-Space salt from the Space root path. Stable across
/// runs against the same Space, different across Spaces.
pub(crate) fn derive_space_salt(space_root: &Path) -> Vec<u8> {
    let canonical = std::fs::canonicalize(space_root)
        .unwrap_or_else(|_| space_root.to_path_buf());
    let s = canonical.to_string_lossy().to_string();
    s.into_bytes()
}

/// True when the Roblox property maps to a mesh asset (vs. a single
/// `[asset].path`). Used to pick between `asset_mesh` and `asset_path`.
fn is_mesh_property(roblox_prop: &str, class: ClassName) -> bool {
    matches!(roblox_prop, "MeshId" | "CollisionMeshId")
        || (roblox_prop == "Content" && matches!(class, ClassName::SpecialMesh))
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
        let lighting = InstanceBuilder::new("Lighting").with_child(
            InstanceBuilder::new("Atmosphere").with_name("Sky"),
        );
        let data_model = InstanceBuilder::new("DataModel")
            .with_child(workspace)
            .with_child(lighting);
        let dom = WeakDom::new(data_model);
        RobloxDom::from_dom(dom, crate::parser::RobloxFormat::BinaryPlace, PathBuf::new())
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
        let cube_path = space_root
            .join("Workspace")
            .join("Group")
            .join("Cube")
            .join("_instance.toml");
        assert!(cube_path.is_file(), "Cube TOML should exist: {}", cube_path.display());

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
        assert!(sky.is_file(), "Atmosphere should land under Lighting: {}", sky.display());
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
        let report = import_into_space(&rbx, &space_root, ImportOptions::default())
            .expect("import");
        assert!(
            report
                .unmapped_classes
                .iter()
                .any(|u| u.roblox_class == "FloofPart"),
            "FloofPart should be logged as unmapped: {:?}",
            report.unmapped_classes
        );
        // The "Bury" Part under FloofPart should NOT exist on disk
        // because we stop the walk at unmapped nodes.
        assert!(!space_root
            .join("Workspace")
            .join("WeirdChild")
            .join("Bury")
            .exists());

        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn terrain_emits_deferral_approximation() {
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

        let space_root = make_temp_root("terrain");
        let report = import_into_space(&rbx, &space_root, ImportOptions::default())
            .expect("import");
        assert!(
            report
                .approximations
                .iter()
                .any(|a| a.reason.contains("voxel data deferred")),
            "Terrain should produce a Wave 4.A.2 approximation note: {:?}",
            report.approximations
        );
        let _ = std::fs::remove_dir_all(&space_root);
    }

    #[test]
    fn csg_emits_deferral_approximation() {
        let dm = InstanceBuilder::new("DataModel").with_child(
            InstanceBuilder::new("Workspace").with_child(
                InstanceBuilder::new("UnionOperation").with_name("Carved"),
            ),
        );
        let dom = WeakDom::new(dm);
        let rbx = RobloxDom::from_dom(
            dom,
            crate::parser::RobloxFormat::BinaryPlace,
            PathBuf::new(),
        );

        let space_root = make_temp_root("csg");
        let report = import_into_space(&rbx, &space_root, ImportOptions::default())
            .expect("import");
        assert!(
            report
                .approximations
                .iter()
                .any(|a| a.reason.contains("CSG") && a.reason.contains("deferred")),
            "CSG should produce a Wave 4.A.2 approximation note: {:?}",
            report.approximations
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
        let report = import_into_space(&rbx, &space_root, ImportOptions::default())
            .expect("import");
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
        let report = import_into_space(&rbx, &space_root, ImportOptions::default())
            .expect("import");
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

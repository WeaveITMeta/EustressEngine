//! Per-import diagnostics surfaced to the user (modal dialog + on-disk
//! JSON archive at `<space_root>/.eustress/import_reports/<ts>.json`).
//!
//! All types here are **real** (not stubbed); they have no rbx_* dep so
//! the crate's report surface can be linked against from UI code without
//! waiting on the parser implementation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::parser::RobloxFormat;

/// Top-level outcome of one import call.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ImportReport {
    /// The source `.rbxl` / `.rbxlx` / etc. file we read.
    pub source_path: PathBuf,
    /// Which format the parser detected.
    pub format: RobloxFormat,

    /// Every Roblox instance we visited, mapped or not.
    pub total_nodes_seen: usize,
    /// Subset of `total_nodes_seen` that materialised into Eustress.
    pub total_nodes_imported: usize,

    /// One entry per Eustress `ClassName`, with how many we created.
    pub class_counts: Vec<ClassCount>,

    /// Roblox classes with no Eustress analogue; subtree skipped.
    pub unmapped_classes: Vec<UnmappedClass>,

    /// Properties on mapped classes we recognised but didn't translate.
    pub unmapped_properties: Vec<UnmappedProperty>,

    /// `rbxassetid://` references we couldn't resolve.
    pub asset_warnings: Vec<AssetWarning>,

    /// Source-level warnings from `compat::ScriptTransformer`.
    pub script_warnings: Vec<ScriptWarning>,

    /// Approximations: e.g. `UnionOperation → Block AABB`.
    pub approximations: Vec<Approximation>,

    /// Roblox services that had no Eustress cognate — routed to
    /// `_imported/<ServiceName>/` rather than skipped.
    pub skipped_services: Vec<SkippedService>,

    /// Roblox `Ref` properties that pointed to an instance we never saw.
    pub unresolved_refs: Vec<UnresolvedRef>,

    /// Cases where `unique_entity_name` had to disambiguate a sibling
    /// collision.
    pub name_collisions: Vec<NameCollision>,

    // ── Terrain (Wave 4.A.2 deferred — these stay 0 / empty here). ──
    /// Wave 4.A.2 will fill: count of decoded SmoothGrid chunks.
    #[serde(default)]
    pub terrain_chunks_imported: usize,
    /// Wave 4.A.2 will fill: terrain material approximations.
    #[serde(default)]
    pub terrain_material_approximations: Vec<TerrainMaterialApproximation>,
    /// Wave 4.A.2 will fill: per-chunk decode errors.
    #[serde(default)]
    pub terrain_decode_errors: Vec<TerrainDecodeError>,

    // ── CSG (Wave 4.A.2 deferred — these stay 0 here). ──
    /// Wave 4.A.2 will fill: CSG instances whose baked mesh was
    /// extracted.
    #[serde(default)]
    pub csg_baked_extracted: usize,
    /// Wave 4.A.2 will fill: CSG instances re-executed via the
    /// `truck-shapeops` fallback.
    #[serde(default)]
    pub csg_recomputed: usize,
    /// Wave 4.A.2 will fill: CSG instances that landed on a plain AABB
    /// because no usable mesh was present.
    #[serde(default)]
    pub csg_fallback_aabb: usize,

    /// Events / functions (`RemoteEvent`, `RemoteFunction`,
    /// `BindableEvent`, `BindableFunction`) imported.
    #[serde(default)]
    pub events_imported: usize,

    /// Wave 8.A: instance cores written directly to the worlddb binary
    /// store via `BinarySink` (bypassing the TOML intermediary). 0 in
    /// TomlFolders mode.
    #[serde(default)]
    pub binary_cores_written: usize,

    /// Wave 9.B: terrain voxel chunks written to the Fjall voxel store
    /// (vs. loose `voxel_chunks/*.bin` files). 0 in TomlFolders mode.
    #[serde(default)]
    pub terrain_chunks_to_db: usize,

    /// Wall-clock duration of the import call.
    pub elapsed: Duration,
}

impl ImportReport {
    /// Record one materialised instance, bumping the count + class count.
    pub fn record_imported(&mut self, class: &str) {
        self.total_nodes_imported += 1;
        match self.class_counts.iter_mut().find(|c| c.class == class) {
            Some(c) => c.count += 1,
            None => self.class_counts.push(ClassCount {
                class: class.to_string(),
                count: 1,
            }),
        }
    }

    /// Record a Roblox class we didn't know how to import — increments
    /// the matching entry if it exists, otherwise creates one.
    pub fn record_unmapped_class(&mut self, roblox_class: &str, sample_name: &str) {
        match self
            .unmapped_classes
            .iter_mut()
            .find(|u| u.roblox_class == roblox_class)
        {
            Some(u) => u.count += 1,
            None => self.unmapped_classes.push(UnmappedClass {
                roblox_class: roblox_class.to_string(),
                count: 1,
                sample_name: sample_name.to_string(),
            }),
        }
    }

    /// Record a per-property mapping miss.
    pub fn record_unmapped_property(&mut self, class: &str, property: &str, variant_type: &str) {
        self.unmapped_properties.push(UnmappedProperty {
            class: class.to_string(),
            property: property.to_string(),
            variant_type: variant_type.to_string(),
        });
    }

    /// Record an unresolved asset reference.
    pub fn record_asset_warning(
        &mut self,
        url: &str,
        host_class: &str,
        host_property: &str,
        reason: &str,
    ) {
        self.asset_warnings.push(AssetWarning {
            url: url.to_string(),
            host_class: host_class.to_string(),
            host_property: host_property.to_string(),
            reason: reason.to_string(),
        });
    }

    /// Record an approximation (UnionOperation → AABB Part, etc.).
    pub fn record_approximation(
        &mut self,
        entity_path: &str,
        original_class: &str,
        eustress_class: &str,
        reason: &str,
    ) {
        self.approximations.push(Approximation {
            entity_path: entity_path.to_string(),
            original_class: original_class.to_string(),
            eustress_class: eustress_class.to_string(),
            reason: reason.to_string(),
        });
    }

    /// Record a Roblox `Ref` property whose target never materialised.
    pub fn record_unresolved_ref(
        &mut self,
        host_path: &str,
        host_property: &str,
        target_referent: &str,
    ) {
        self.unresolved_refs.push(UnresolvedRef {
            host_path: host_path.to_string(),
            host_property: host_property.to_string(),
            target_referent: target_referent.to_string(),
        });
    }

    /// Record a sibling-name disambiguation outcome.
    pub fn record_name_collision(
        &mut self,
        parent_path: &str,
        original_name: &str,
        final_name: &str,
    ) {
        self.name_collisions.push(NameCollision {
            parent_path: parent_path.to_string(),
            original_name: original_name.to_string(),
            final_name: final_name.to_string(),
        });
    }

    /// Record a Roblox service that didn't have a Eustress cognate (its
    /// children were routed to `_imported/<ServiceName>/`).
    pub fn record_skipped_service(&mut self, service: &str, reason: &str) {
        self.skipped_services.push(SkippedService {
            service: service.to_string(),
            reason: reason.to_string(),
        });
    }

    /// Aggregate per-class counts into a stable map view.
    pub fn class_counts_as_map(&self) -> HashMap<String, usize> {
        self.class_counts
            .iter()
            .map(|c| (c.class.clone(), c.count))
            .collect()
    }
}

/// `ClassName` → count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassCount {
    /// Eustress class name (`ClassName::as_str()` form).
    pub class: String,
    /// How many instances of this class were materialised.
    pub count: usize,
}

/// A Roblox class with no Eustress mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmappedClass {
    /// Roblox class name (e.g. `"PluginGuiService"`).
    pub roblox_class: String,
    /// How many instances of this class appeared.
    pub count: usize,
    /// A sample `Name` so the user can find one quickly.
    pub sample_name: String,
}

/// A property the importer saw but didn't know how to translate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmappedProperty {
    /// Eustress class on which the unmapped property appeared.
    pub class: String,
    /// Roblox property name (e.g. `"CollisionGroupId"`).
    pub property: String,
    /// `rbx_dom_weak` variant type tag (e.g. `"Int32"`, `"Color3"`).
    pub variant_type: String,
}

/// An asset reference we couldn't (or didn't try to) fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetWarning {
    /// Original URL (e.g. `"rbxassetid://1234567890"`).
    pub url: String,
    /// Eustress class hosting the reference (e.g. `"Decal"`).
    pub host_class: String,
    /// Eustress property name (e.g. `"asset_path"`).
    pub host_property: String,
    /// Human-readable reason.
    pub reason: String,
}

/// A `compat::TransformWarning` annotated with the entity that hosted
/// the script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptWarning {
    /// Path to the host entity (e.g. `"Workspace/MyModel/Script"`).
    pub entity_path: String,
    /// The warning message (verbatim from `compat::TransformWarning`).
    pub message: String,
    /// Severity (`"info"` | `"warning"` | `"error"`).
    pub severity: String,
}

/// A geometric / semantic approximation made during import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approximation {
    /// Path to the affected entity.
    pub entity_path: String,
    /// Roblox class that triggered the approximation (e.g. `"UnionOperation"`).
    pub original_class: String,
    /// Eustress class we degraded to (e.g. `"Part"`).
    pub eustress_class: String,
    /// Why (e.g. `"CSG baked-mesh extraction deferred to Wave 4.A.2"`).
    pub reason: String,
}

/// A Roblox service routed to `_imported/<ServiceName>/` rather than to
/// a Eustress cognate (or silently skipped).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedService {
    /// Roblox service class (e.g. `"MarketplaceService"`).
    pub service: String,
    /// Why it ended up under `_imported/`.
    pub reason: String,
}

/// A Roblox terrain material that collapsed to a different Eustress
/// material than the user intended.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrainMaterialApproximation {
    /// Roblox source material (e.g. `"Marble"`).
    pub roblox_material: String,
    /// Eustress destination (e.g. `"Rock"`).
    pub eustress_material: String,
    /// How many voxels affected.
    pub voxel_count: usize,
}

/// A per-chunk terrain decode failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrainDecodeError {
    /// Chunk grid coordinates.
    pub cx: i32,
    /// Chunk grid coordinates.
    pub cy: i32,
    /// Chunk grid coordinates.
    pub cz: i32,
    /// Error message.
    pub reason: String,
}

/// A Roblox `Ref` property whose target wasn't in the source DOM (or
/// was filtered out earlier in the walk).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedRef {
    /// Eustress space-relative path of the host entity.
    pub host_path: String,
    /// Roblox property name on the host (e.g. `"Part0"`).
    pub host_property: String,
    /// The referent string the property pointed to.
    pub target_referent: String,
}

/// A sibling-name disambiguation outcome — `unique_entity_name` had to
/// suffix the requested name to avoid a folder collision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameCollision {
    /// Space-relative path of the parent.
    pub parent_path: String,
    /// What the Roblox source called the entity.
    pub original_name: String,
    /// What the materializer named it on disk.
    pub final_name: String,
}

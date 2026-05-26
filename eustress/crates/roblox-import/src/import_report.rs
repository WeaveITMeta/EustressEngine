//! Per-import diagnostics surfaced to the user (modal dialog + on-disk
//! JSON archive at `<space_root>/.eustress/import_reports/<ts>.json`).
//!
//! All types here are **real** (not stubbed); they have no rbx_*
//! dep so the crate's report surface can be linked against from Wave 2
//! UI code without waiting on the parser implementation.

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

    /// `rbxassetid://` references we couldn't resolve (Wave 1 = all of them).
    pub asset_warnings: Vec<AssetWarning>,

    /// Source-level warnings from `compat::ScriptTransformer`.
    pub script_warnings: Vec<ScriptWarning>,

    /// Approximations: e.g. `UnionOperation → Block AABB`.
    pub approximations: Vec<Approximation>,

    /// Wall-clock duration of the import call.
    pub elapsed: Duration,
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
    /// Human-readable reason (Wave 1: always "rbxassetid not yet resolved").
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
    /// Why (e.g. `"CSG resolution not yet implemented; using AABB bounds"`).
    pub reason: String,
}

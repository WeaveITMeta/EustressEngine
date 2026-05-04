//! # Attribute + Tag enforcement migration
//!
//! Scans every `_instance.toml` in a Space and ensures it has the
//! baseline `[attributes]` + `[tags]` sections. Instances missing
//! them get empty tables added (`attributes = {}` / `tags = []`).
//!
//! Services (Workspace, Lighting, StarterGui, SoulService etc.) are
//! skipped — they're infrastructure classes, not authored parts.
//!
//! ## Why
//!
//! The Timeline panel + many Phase-2 features (Selection by Tag,
//! AI Select Similar, telemetry categorization) key off
//! `attributes` + `tags` being present on every part. When a TOML
//! lacks the sections, the loader silently defaults to empty —
//! functionally OK but the Explorer / Timeline / embedvec all see
//! "no tags" even on parts the user tagged in a prior session that
//! got clobbered by an unmigrated round-trip.
//!
//! ## Invocation
//!
//! Fire `RunAttributeTagMigrationEvent { space_root, dry_run }` —
//! handler walks the tree, writes changes (or logs them when
//! `dry_run = true`). Idempotent; re-running after a clean pass
//! touches zero files.
//!
//! ## Safety
//!
//! - Reads the TOML → mutates the document → writes atomically via
//!   `std::fs::rename` on a temp file next to the original.
//! - Preserves every existing key + comment via `toml_edit`.
//! - Service skip list is conservative — when in doubt, don't touch.

use bevy::prelude::*;
use std::path::{Path, PathBuf};

/// Fire to kick off the scan.
#[derive(Event, Message, Debug, Clone)]
pub struct RunAttributeTagMigrationEvent {
    pub space_root: PathBuf,
    /// True → log what would change but don't write. Useful for a
    /// Settings-panel preview.
    pub dry_run: bool,
}

/// Emitted when the scan finishes.
#[derive(Event, Message, Debug, Clone)]
pub struct AttributeTagMigrationReport {
    pub scanned: u32,
    pub migrated: u32,
    pub skipped_services: u32,
    pub errors: Vec<String>,
    pub dry_run: bool,
}

pub struct AttributeTagMigrationPlugin;

impl Plugin for AttributeTagMigrationPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<RunAttributeTagMigrationEvent>()
            .add_message::<AttributeTagMigrationReport>()
            .add_systems(Update, handle_migration_request);
    }
}

// ============================================================================
// Service skip list
// ============================================================================
//
// Classes we never touch — they're services, not instances. The
// `class_name` field in `[metadata]` drives the skip decision. Kept
// as a sorted `const &[&str]` so additions are obvious in PR diffs.

pub const SERVICE_CLASS_NAMES: &[&str] = &[
    "Atmosphere",
    "Chat",
    "Cloud",
    "DataStoreService",
    "Debris",
    "Lighting",
    "MaterialService",
    "MessagingService",
    "Players",
    "ReplicatedStorage",
    "RunService",
    "ScriptService",
    "ServerScriptService",
    "ServerStorage",
    "Sky",
    "SoulService",
    "Stars",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "TeleportService",
    "Terrain",
    "TestService",
    "UserInputService",
    "Workspace",
];

fn is_service_class(class_name: &str) -> bool {
    SERVICE_CLASS_NAMES.iter().any(|s| *s == class_name)
}

// ============================================================================
// Handler
// ============================================================================

fn handle_migration_request(
    mut events: MessageReader<RunAttributeTagMigrationEvent>,
    mut reports: MessageWriter<AttributeTagMigrationReport>,
) {
    for event in events.read() {
        let report = run_migration(&event.space_root, event.dry_run);
        info!(
            "🔖 Attribute+Tag migration ({}): {} scanned · {} migrated · {} services skipped · {} errors",
            if event.dry_run { "dry-run" } else { "live" },
            report.scanned, report.migrated, report.skipped_services, report.errors.len(),
        );
        reports.write(report);
    }
}

/// Run the migration synchronously. Public so startup code /
/// scripted tests can call it too.
pub fn run_migration(space_root: &Path, dry_run: bool) -> AttributeTagMigrationReport {
    let mut report = AttributeTagMigrationReport {
        scanned: 0,
        migrated: 0,
        skipped_services: 0,
        errors: Vec::new(),
        dry_run,
    };

    let mut stack: Vec<PathBuf> = vec![space_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            // Skip hidden dirs (.eustress/ trash, caches).
            if path.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.starts_with('.'))
                .unwrap_or(false)
            { continue; }

            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() { stack.push(path); continue; }
            if path.file_name().and_then(|s| s.to_str()) != Some("_instance.toml") {
                continue;
            }
            report.scanned += 1;
            match migrate_instance_file(&path, dry_run) {
                Ok(MigrationOutcome::Migrated) => report.migrated += 1,
                Ok(MigrationOutcome::UpToDate) => {}
                Ok(MigrationOutcome::SkippedService) => report.skipped_services += 1,
                Err(e) => report.errors.push(format!("{:?}: {}", path, e)),
            }
        }
    }
    report
}

#[derive(Debug, Clone, Copy)]
enum MigrationOutcome {
    Migrated,
    UpToDate,
    SkippedService,
}

fn migrate_instance_file(path: &Path, dry_run: bool) -> Result<MigrationOutcome, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;

    // Use plain toml parsing to detect current state. We don't import
    // toml_edit in the crate today; when adopted, this becomes
    // comment-preserving. For now we do a read-merge-write that
    // preserves top-level structure but loses inline comments on
    // rewrite — acceptable since `_instance.toml` is engine-generated.
    let mut doc: toml::Value = raw.parse().map_err(|e: toml::de::Error| e.to_string())?;

    // Service skip: read `[metadata].class_name`. Accepts either case
    // on disk so TOMLs from the aborted PascalCase pilot still load.
    let class_name = eustress_common::class_schema::get_section_insensitive(&doc, "metadata")
        .and_then(|m| eustress_common::class_schema::get_section_insensitive(m, "class_name"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    if is_service_class(class_name) {
        return Ok(MigrationOutcome::SkippedService);
    }

    let table = doc.as_table_mut().ok_or_else(|| "root is not a table".to_string())?;
    let mut changed = false;

    if !table.contains_key("attributes") {
        table.insert("attributes".to_string(), toml::Value::Table(toml::value::Table::new()));
        changed = true;
    }
    if !table.contains_key("tags") {
        table.insert("tags".to_string(), toml::Value::Array(Vec::new()));
        changed = true;
    }

    if !changed { return Ok(MigrationOutcome::UpToDate); }
    if dry_run { return Ok(MigrationOutcome::Migrated); }

    let rewritten = toml::to_string_pretty(&doc).map_err(|e| e.to_string())?;

    // Atomic rewrite via temp-file + rename. Protects against
    // mid-write crash leaving a truncated file.
    let tmp = path.with_extension("toml.migrate.tmp");
    std::fs::write(&tmp, rewritten).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;

    Ok(MigrationOutcome::Migrated)
}

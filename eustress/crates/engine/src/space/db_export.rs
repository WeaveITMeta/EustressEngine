//! Export the world DB's binary instance cores to human-readable TOML.
//!
//! Binary-ECS entities live in Fjall as rkyv `ArchInstanceCore` blobs — fast to
//! stream, impossible to read, grep, or diff. This module dumps them back out
//! as the same `_instance.toml` documents the disk representation uses, so a
//! world that exists only in the database can be inspected, version-controlled,
//! or handed to a tool that speaks TOML.
//!
//! Distinct from [`super::promote`] in intent, and that difference matters:
//! *promote* CHANGES an entity's persistence backing (binary → disk, in place,
//! one entity, must be resident). *Export* changes nothing — it reads the DB
//! and writes a copy elsewhere, works on entities that were never streamed in,
//! and covers the whole world in one pass. Exporting does not make the DB stop
//! owning those entities.
//!
//! Everything here is pure CPU + filesystem: no `World`, no Bevy resources. The
//! caller snapshots cores out of the DB on the main thread and runs the decode
//! and write off it, so a 50 000-instance export doesn't stall the frame loop.

#![cfg(feature = "world-db")]

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use eustress_worlddb::decode_instance_core;
use serde::Serialize;

use super::arch_instance::arch_to_instance;
use super::instance_loader::InstanceDefinition;

/// Where the export lands when the caller doesn't say. Under `.eustress/`
/// deliberately: both the Space loader and the file watcher skip that folder,
/// so an export can never be re-ingested as a second copy of the scene.
pub const DEFAULT_EXPORT_SUBDIR: &str = ".eustress/exports/instances";

/// Filename used by [`ExportLayout::SingleFile`].
pub const SINGLE_FILE_NAME: &str = "instances.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportLayout {
    /// One `<Name>_<id>/_instance.toml` folder per instance — the canonical
    /// on-disk shape, so the output can be copied into a Space's `Workspace/`
    /// and loaded as-is.
    Folders,
    /// One document holding every instance as an `[[instance]]` entry. Better
    /// for reading, diffing, and committing; not directly loadable.
    SingleFile,
}

impl ExportLayout {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "folders" => Ok(Self::Folders),
            "single_file" => Ok(Self::SingleFile),
            other => Err(format!(
                "unknown layout '{other}' (expected \"folders\" or \"single_file\")"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Folders => "folders",
            Self::SingleFile => "single_file",
        }
    }
}

/// A validated export destination + options.
pub struct ExportPlan {
    pub output_dir: PathBuf,
    pub layout: ExportLayout,
    /// Exact class name to keep, or `None` for every class.
    pub class_filter: Option<String>,
}

/// What an export actually did. Every core is accounted for in exactly one
/// bucket, so `written + filtered_out + undecodable` equals the input count —
/// a silently-dropped instance would otherwise look like a successful export.
#[derive(Debug, Default, Clone)]
pub struct ExportReport {
    pub written: usize,
    pub filtered_out: usize,
    pub undecodable: usize,
    pub output_dir: PathBuf,
}

/// Wrapper that gives [`ExportLayout::SingleFile`] its `[[instance]]` array.
#[derive(Serialize)]
struct ExportDocument {
    instance: Vec<InstanceDefinition>,
}

/// Resolve and validate the output directory.
///
/// A relative path is taken against the Space root; an absolute one must still
/// land inside it. Confining the destination matters because the caller is
/// usually an AI acting on a natural-language instruction — an unconstrained
/// `output_dir` is a "write thousands of files anywhere on disk" primitive.
pub fn resolve_output_dir(space_root: &Path, requested: Option<&str>) -> Result<PathBuf, String> {
    let raw = requested
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_EXPORT_SUBDIR);

    let joined = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        space_root.join(raw)
    };

    // Normalise `..` textually — the path usually doesn't exist yet, so
    // `canonicalize` isn't available to do it for us.
    let normalised = normalise(&joined);
    let root = normalise(&space_root.canonicalize().unwrap_or_else(|_| space_root.to_path_buf()));
    if !normalised.starts_with(&root) {
        return Err(format!(
            "output_dir '{raw}' resolves outside the Space root ({}) — exports must stay inside the Space",
            root.display()
        ));
    }
    Ok(normalised)
}

fn normalise(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// True when `dir` already holds something an export would sit on top of.
/// Callers surface this as a refusal unless the caller passed `overwrite`.
pub fn dir_has_content(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut it| it.next().is_some())
        .unwrap_or(false)
}

/// Folder-safe rendering of an instance name. Keeps the name recognisable
/// while guaranteeing a legal, collision-free directory component.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "Instance".to_string()
    } else {
        trimmed.chars().take(64).collect()
    }
}

/// Decode `cores` and write them as TOML under `plan.output_dir`.
///
/// Pure CPU + filesystem — safe to call from a worker thread. An individual
/// core that fails to decode is counted and skipped rather than aborting the
/// run: one corrupt row shouldn't cost you the other 49 999.
pub fn write_export(cores: &[(u64, Vec<u8>)], plan: &ExportPlan) -> Result<ExportReport, String> {
    std::fs::create_dir_all(&plan.output_dir)
        .map_err(|e| format!("create {}: {e}", plan.output_dir.display()))?;

    let mut report = ExportReport {
        output_dir: plan.output_dir.clone(),
        ..Default::default()
    };
    let mut docs: Vec<InstanceDefinition> = Vec::new();
    // Folder names are name-derived, and thousands of parts share a name, so
    // the stored id disambiguates. This guards the residual case of two ids
    // rendering the same component.
    let mut used: HashSet<String> = HashSet::new();

    for (stored_id, bytes) in cores {
        let core = match decode_instance_core(bytes) {
            Ok(c) => c,
            Err(_) => {
                report.undecodable += 1;
                continue;
            }
        };
        if let Some(want) = &plan.class_filter {
            if &core.class_name != want {
                report.filtered_out += 1;
                continue;
            }
        }

        let def = arch_to_instance(&core);
        match plan.layout {
            ExportLayout::SingleFile => docs.push(def),
            ExportLayout::Folders => {
                let name = def
                    .metadata
                    .name
                    .clone()
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| core.class_name.clone());
                let mut folder_name = format!("{}_{stored_id:016x}", sanitize(&name));
                while !used.insert(folder_name.clone()) {
                    folder_name.push('_');
                }
                let folder = plan.output_dir.join(&folder_name);
                std::fs::create_dir_all(&folder)
                    .map_err(|e| format!("create {}: {e}", folder.display()))?;
                let text = toml::to_string_pretty(&def)
                    .map_err(|e| format!("serialize {folder_name}: {e}"))?;
                std::fs::write(folder.join("_instance.toml"), text)
                    .map_err(|e| format!("write {}: {e}", folder.display()))?;
                report.written += 1;
            }
        }
    }

    if plan.layout == ExportLayout::SingleFile {
        report.written = docs.len();
        let text = toml::to_string_pretty(&ExportDocument { instance: docs })
            .map_err(|e| format!("serialize export document: {e}"))?;
        let path = plan.output_dir.join(SINGLE_FILE_NAME);
        std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_space(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress-export-test-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn default_dir_lands_under_the_space() {
        let space = temp_space("default");
        let out = resolve_output_dir(&space, None).unwrap();
        assert!(out.ends_with("instances"));
        assert!(out.starts_with(normalise(&space.canonicalize().unwrap())));
    }

    #[test]
    fn escaping_paths_are_rejected() {
        let space = temp_space("escape");
        assert!(resolve_output_dir(&space, Some("../../elsewhere")).is_err());
        assert!(resolve_output_dir(&space, Some("sub/../../../etc")).is_err());
        // An absolute path outside the Space is rejected too.
        let outside = std::env::temp_dir().join("definitely-not-the-space");
        assert!(resolve_output_dir(&space, outside.to_str()).is_err());
    }

    #[test]
    fn nested_relative_paths_are_allowed() {
        let space = temp_space("nested");
        let out = resolve_output_dir(&space, Some("Exports/run1")).unwrap();
        assert!(out.ends_with(Path::new("Exports").join("run1")));
    }

    #[test]
    fn sanitize_produces_usable_components() {
        assert_eq!(sanitize("House Roof"), "House_Roof");
        assert_eq!(sanitize("a/b\\c"), "a_b_c");
        assert_eq!(sanitize("___"), "Instance");
        assert_eq!(sanitize(""), "Instance");
        assert_eq!(sanitize("Keep-This_1"), "Keep-This_1");
        assert!(sanitize(&"x".repeat(500)).len() <= 64);
    }

    #[test]
    fn undecodable_cores_are_counted_not_fatal() {
        let space = temp_space("garbage");
        let plan = ExportPlan {
            output_dir: space.join("out"),
            layout: ExportLayout::Folders,
            class_filter: None,
        };
        let cores = vec![(1u64, vec![0xFF, 0x00]), (2u64, Vec::new())];
        let report = write_export(&cores, &plan).unwrap();
        assert_eq!(report.written, 0);
        assert_eq!(report.undecodable, 2);
    }

    #[test]
    fn layout_parsing_rejects_unknown() {
        assert_eq!(ExportLayout::parse("folders").unwrap(), ExportLayout::Folders);
        assert_eq!(
            ExportLayout::parse("single_file").unwrap(),
            ExportLayout::SingleFile
        );
        assert!(ExportLayout::parse("csv").is_err());
    }
}

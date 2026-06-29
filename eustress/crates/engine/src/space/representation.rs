//! Representation router — decides whether an entity is stored as a
//! **FileSystem** entity (folder + `_instance.toml` + sibling files; the
//! `tree` partition / disk) or a **BinaryEcs** entity (zero-copy rkyv
//! [`eustress_worlddb::ArchInstanceCore`] in the `entities` partition,
//! scalable to millions).
//!
//! ## The rule — the "utility + scalability factor"
//!
//! - **FileSystem** when the entity needs a real filesystem path:
//!   - it carries an attached artifact — e.g. a `.pptx` dropped inside a
//!     Part, an imported image/document, a `.rune` script source; OR
//!   - it is a *file-natured class* whose essential content IS a file (a
//!     SoulScript's source, a Workshop conversation's transcript, a GUI
//!     `.toml` layout, a Document node).
//!   Binary ECS cannot hold a real path, so anything file-bearing MUST
//!   live here.
//! - **BinaryEcs** otherwise — a bare Part / primitive that is pure
//!   component data (transform, render + physics flags, tags, attributes).
//!   This is the scalable set, and the Insert-menu default.
//!
//! ## Dynamic, event-driven conversion (wired by the Studio listeners)
//!
//! - **Promote BinaryEcs → FileSystem** the instant the entity gains its
//!   first real-path artifact (paste / drop a file into it): materialize
//!   the folder, write `_instance.toml`, drop the file in. Safe + additive.
//! - **Demote FileSystem → BinaryEcs** automatically when the *last* file
//!   artifact is removed and the entity is a bare scalable type: fold the
//!   core back into a rkyv record and drop the now-empty folder.
//!
//! The TOML ↔ rkyv bridge those conversions use is
//! [`super::arch_instance`] (`instance_to_arch` / `arch_to_instance`).
//!
//! NOTE: this module is the *decision* layer. It is pure (no Bevy, no DB
//! handle) so it can be unit-tested and called from any site. The
//! entities-partition load + save path (`world_db_binary`) and the Morton
//! ("K2") `INSTANCE_CORE` codec (`keys::MortonKeyEncoder`,
//! `fjall_backend::put_instance_core`) are wired and live; `spawn_binary_instance`
//! already honors this router at create. The remaining work is flipping the
//! *create default* at every Insert/paste/MCP site to route bare Parts here
//! (roadmap Phase 1, the "create-flip") and pointing the Properties inspector at
//! the live core instead of re-parsing disk TOML — not the storage path itself.

use std::path::Path;

/// Where an entity's authoritative state lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Representation {
    /// Folder + `_instance.toml` + sibling files (`tree` partition / disk).
    /// The only form that can hold a real-path file artifact.
    FileSystem,
    /// Zero-copy rkyv `ArchInstanceCore` in the `entities` partition.
    /// Pure component data; scales to millions. Cannot hold a file path.
    BinaryEcs,
}

/// Classify the representation an entity SHOULD have right now.
///
/// `class_name` is the instance's class. `folder` is the entity's
/// folder-form directory if it has one (`None` for a flat or
/// not-yet-materialized entity — a bare Insert-menu part).
pub fn representation_for(class_name: &str, folder: Option<&Path>) -> Representation {
    // 1. File-natured classes are always FileSystem: their essential
    //    content is itself a real file (script source, transcript, GUI
    //    layout, document).
    if class_is_file_natured(class_name) {
        return Representation::FileSystem;
    }
    // 2. An attached real-path artifact forces FileSystem — binary ECS
    //    cannot reference a path.
    if folder.map(folder_has_attached_artifacts).unwrap_or(false) {
        return Representation::FileSystem;
    }
    // 3. Pure component data → scalable binary ECS (the bare-Part default).
    Representation::BinaryEcs
}

/// Classes whose essential content is a real file, so they are always
/// FileSystem regardless of folder contents. (Unknown names simply fall
/// through to the artifact check — harmless if a name here doesn't exist.)
pub fn class_is_file_natured(class_name: &str) -> bool {
    matches!(
        class_name,
        // Scripts + AI artifacts — backed by `.rune`/`.lua`/transcript files.
        "SoulScript" | "WorkshopConversation"
        // Explicit document / imported-file nodes.
        | "Document" | "File"
        // GUI classes are authored as `.toml` layout files and edited as
        // text, so they stay FileSystem.
        | "ScreenGui" | "SurfaceGui" | "BillboardGui"
        | "Frame" | "ScrollingFrame"
        | "TextLabel" | "TextButton" | "TextBox"
        | "ImageLabel" | "ImageButton"
        // Environment / lighting nodes: small TOML config read by the
        // directory loader's env arm; keep FileSystem so they are not
        // skipped on streaming-primary imports.
        | "Atmosphere" | "Sky" | "Clouds" | "DirectionalLight"
    )
}

/// True when the entity's folder holds a real artifact — a file that is
/// NOT one of the entity's own marker files and not a nested child-entity
/// folder. A `.pptx`, a `.png`, a `.rune` sibling, etc. all count.
///
/// This is what makes "drop a PowerPoint into a Part" classify the Part
/// as FileSystem.
pub fn folder_has_attached_artifacts(folder: &Path) -> bool {
    let Ok(read_dir) = std::fs::read_dir(folder) else {
        return false;
    };
    read_dir.flatten().any(|entry| {
        let path = entry.path();
        path.is_file() && !is_marker_file(&path)
    })
}

/// The entity's own definition/marker files — these are NOT "attached
/// artifacts" (every folder-form entity has an `_instance.toml`).
fn is_marker_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("_instance.toml") | Some("_service.toml")
    )
}

/// True when an `asset.mesh` reference can only be resolved relative to the
/// entity's on-disk location, so the part MUST stay FileSystem.
///
/// The engine's bundled primitives live under `parts/` (`parts/block.glb`,
/// `parts/ball.glb`, …) and resolve from the engine asset source with no
/// folder, so they are BinaryEcs-compatible. ANYTHING else — a relative
/// `../meshes/VCell_Housing.glb`, a custom upload — resolves relative to the
/// part's folder, which a BinaryEcs entity does not have (it carries only a
/// synthetic path). Letting such a part fall into binary ECS is exactly how
/// V-Cell would lose its mesh: the core stores the string but load can't
/// find the file. This is the "TOML meshes must not silently end up in
/// binary ECS Fjall" guard.
pub fn mesh_requires_filesystem(mesh: &str) -> bool {
    !mesh.is_empty() && !mesh.starts_with("parts/")
}

/// Mesh-aware variant of [`representation_for`]: a custom / relative mesh
/// forces FileSystem regardless of class or folder contents. Creation and
/// promote/demote sites that know the instance's mesh should call THIS so a
/// custom-mesh part is never routed into the `entities` partition.
pub fn representation_for_part(
    class_name: &str,
    mesh: Option<&str>,
    folder: Option<&Path>,
) -> Representation {
    if mesh.map(mesh_requires_filesystem).unwrap_or(false) {
        return Representation::FileSystem;
    }
    representation_for(class_name, folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_part_defaults_to_binary_ecs() {
        // The Insert-menu default: a primitive with no files → scalable.
        assert_eq!(representation_for("Part", None), Representation::BinaryEcs);
        assert_eq!(representation_for("WedgePart", None), Representation::BinaryEcs);
        assert_eq!(representation_for("Model", None), Representation::BinaryEcs);
    }

    #[test]
    fn file_natured_classes_are_filesystem() {
        for class in [
            "SoulScript",
            "WorkshopConversation",
            "Document",
            "File",
            "ScreenGui",
            "TextLabel",
            "ImageButton",
        ] {
            assert_eq!(
                representation_for(class, None),
                Representation::FileSystem,
                "{class} should be FileSystem",
            );
        }
    }

    #[test]
    fn marker_files_are_not_artifacts() {
        assert!(is_marker_file(Path::new("Foo/_instance.toml")));
        assert!(is_marker_file(Path::new("Foo/_service.toml")));
        assert!(!is_marker_file(Path::new("Foo/presentation.pptx")));
        assert!(!is_marker_file(Path::new("Foo/diagram.png")));
    }

    #[test]
    fn primitive_meshes_are_binary_ecs_custom_meshes_are_filesystem() {
        // Engine primitives resolve with no folder → BinaryEcs-compatible.
        assert!(!mesh_requires_filesystem("parts/block.glb"));
        assert!(!mesh_requires_filesystem("parts/ball.glb"));
        assert!(!mesh_requires_filesystem(""));
        // Custom / relative meshes need the on-disk folder → FileSystem.
        assert!(mesh_requires_filesystem("../meshes/VCell_Housing.glb"));
        assert!(mesh_requires_filesystem("meshes/custom.glb"));

        // A bare Part with a primitive mesh stays scalable…
        assert_eq!(
            representation_for_part("Part", Some("parts/block.glb"), None),
            Representation::BinaryEcs,
        );
        // …but the same Part with a custom mesh (V-Cell) is FileSystem.
        assert_eq!(
            representation_for_part("Part", Some("../meshes/VCell_Anode.glb"), None),
            Representation::FileSystem,
        );
    }
}

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
        // Local lights are read by the directory loader's light arm. The
        // binary path has no light spawner: a light core came back as an
        // unanchored, collidable block and never lit anything.
        | "PointLight" | "SpotLight" | "SurfaceLight"
        // Gaussian-splat clouds: essential content is a real `.ply`
        // radiance-field file (referenced by a `[gaussian_splats].path`
        // section), exactly like a custom-mesh part owns its `.glb`. This
        // keeps the instance folder-form (FileSystem) so it persists +
        // reloads on DB-primary Spaces via the same load-merge path as
        // custom meshes — instead of the DB layer trying to `bincode`-
        // collapse it into a flat binary core, which FAILS (the
        // `#[serde(flatten)]` `extra` table carrying the splat path is not
        // bincode-serialisable) and left the folder unbacked, so the
        // reconcile sweep trashed it on the next open ("imported splat
        // vanishes on restart").
        | "GaussianSplats"
        // Particle simulations: their properties are TOML field tables
        // (`[particle_simulation]` / `[particle_species]`) that a binary core
        // cannot carry, and a simulation is a container of species folders.
        | "ParticleSimulation" | "ParticleSpecies"
        // Terrain layers, for the same reasons: `[terrain_spline]` and its
        // siblings are field tables, and a spline is a container of point
        // folders under `Workspace/Terrain/Layers`.
        | "TerrainSpline" | "TerrainSplinePoint" | "TerrainStamp"
        | "TerrainFlattenPad" | "TerrainNoise" | "TerrainMaterialFill" | "TerrainScatter"
        | "TerrainWaterBody"
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

/// Whether an instance's raw TOML text references a custom mesh.
///
/// Deliberately a TEXT scan rather than a parsed-field check: a mesh path can
/// appear in `[asset].mesh`, in `[properties.extras]`, or in a section the
/// `InstanceDefinition` parser does not model, and a binary core cannot carry
/// any of them. Over-matching is the safe direction: a false positive keeps an
/// entity in the tree, a false negative converts one the loader still spawns
/// and creates it twice.
///
/// Shared so every caller of [`streams_from_db`] answers this the same way.
pub fn toml_mentions_custom_mesh(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    l.contains("mesh") || l.contains(".glb") || l.contains(".obj")
}

/// True for the file kinds whose `tree` copy is brought back in step with
/// disk every time the Space opens: `.toml` definitions and the `.rune`,
/// `.luau`, `.lua`, `.soul` and `.md` script sources the loader reads out of
/// the tree.
///
/// Every other kind (GLB meshes, images, audio, JSON) was copied into the
/// tree once, by the first import, and the open never compares it with disk
/// again. An edit made while the engine was closed leaves the tree copy
/// stale, so a reader that needs current bytes for such a file (the `.echk`
/// export) reads the disk file instead.
///
/// The disk-to-tree reconcile on open (`world_db_plugin`) enumerates files
/// through this, so the two cannot disagree. `rel` is a tree key or any path;
/// only its final component is read.
pub fn tree_tracks(rel: &str) -> bool {
    let name = rel.rsplit(['/', '\\']).next().unwrap_or(rel);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    // `.toml` alone is a hidden file with no extension, as `Path` reads it.
    !stem.is_empty()
        && matches!(
            ext.to_ascii_lowercase().as_str(),
            "toml" | "rune" | "luau" | "lua" | "soul" | "md"
        )
}

#[cfg(test)]
mod tree_tracks_tests {
    use super::tree_tracks;

    #[test]
    fn definitions_and_script_sources_are_tracked() {
        for rel in [
            "Workspace/Brick/_instance.toml",
            "Workspace/_service.toml",
            "ServerScriptService/Main/Main.rune",
            "ServerScriptService/Loop/Loop.luau",
            "ServerScriptService/Old/Old.lua",
            "ServerScriptService/Soul/Soul.soul",
            "ServerScriptService/Soul/Soul.md",
            "Workspace/Brick.part.toml",
            "Workspace/Brick/_INSTANCE.TOML",
            "Workspace\\Brick\\_instance.toml",
        ] {
            assert!(tree_tracks(rel), "{rel} must be tracked");
        }
    }

    #[test]
    fn assets_and_other_files_are_not_tracked() {
        for rel in [
            "Workspace/Car/Body.glb",
            "Workspace/Sign/face.png",
            "SoundService/Horn/horn.ogg",
            "Workspace/Data/table.json",
            "Workspace/Scene/scene.ron",
            "Workspace/Readme",
            "Workspace/.toml",
            "Workspace/Brick.toml/mesh.glb",
            "",
        ] {
            assert!(!tree_tracks(rel), "{rel} must not be tracked");
        }
    }
}

/// The class a raw TOML `class_name` resolves to, exactly as the loader
/// resolves it: the legacy `"Script"` is the Rune script class, and a name
/// that matches no class loads as a Folder.
///
/// [`streams_from_db`] classifies through this, so a caller holding the raw
/// string (the bake, the publish export) and one holding the loader's parsed
/// class reach the same answer. Comparing raw strings instead is how a raw
/// `"Script"` came to be baked by one caller and spawned from the tree by the
/// other.
pub fn class_from_toml(raw: &str) -> eustress_common::classes::ClassName {
    // One rule, shared with a Player's reader of the same records.
    eustress_common::datamodel::record::class_from_toml(raw)
}

/// THE single test for "this entity is owned by the binary-ECS / streaming
/// tier, not by the filesystem tree loader".
///
/// Three callers depend on this being one function rather than agreeing
/// predicates:
///
/// * `file_loader`'s streaming-primary gate SKIPS spawning a tree entity when
///   this is true and a core exists for it, because residency streams it
///   from the `entities` partition.
/// * `bake_cores` CONVERTS a tree entity into a core when this is true.
/// * `echk_export` publishes the core in place of the tree row when this is
///   true.
///
/// Written separately, they drift, and the drift is silent in both
/// directions: an entity converted but still spawned exists TWICE, one
/// skipped but never converted vanishes.
///
/// It is an ALLOWLIST: a childless, mesh-free `Part` under the Workspace, and
/// nothing else. A core is flat (`spawn_binary_core` parents every core to
/// Workspace) and carries no hierarchy, so only an object that means the same
/// thing flat under Workspace may become one. An exclude-list admitted
/// Textures, Decals, Welds, Sounds, Luau scripts and Folders from every
/// service, templates in ReplicatedStorage and ServerStorage included, and
/// spawned them all under Workspace: a Texture needs a BasePart parent to
/// render, a script needs its source child to run in Play, and a template does
/// not belong in the world.
///
/// `SpawnLocation`, `Seat` and `VehicleSeat` are classes of their own, so
/// they fall outside the list: they attach a subclass component the streaming
/// path does not carry.
///
/// `rel_path` is the entity's tree key, e.g.
/// `Workspace/Map/Brick/_instance.toml`.
pub fn streams_from_db(
    class_name: &str,
    has_children: bool,
    has_custom_mesh: bool,
    rel_path: &str,
) -> bool {
    if has_children || has_custom_mesh {
        return false;
    }
    if !rel_path.starts_with("Workspace/") {
        return false;
    }
    matches!(
        class_from_toml(class_name),
        eustress_common::classes::ClassName::Part
    )
}

#[cfg(test)]
mod streams_from_db_tests {
    use super::*;

    const IN_WORKSPACE: &str = "Workspace/Map/Brick/_instance.toml";

    #[test]
    fn a_bare_part_in_the_workspace_streams() {
        assert!(streams_from_db("Part", false, false, IN_WORKSPACE));
    }

    #[test]
    fn a_parent_never_streams() {
        assert!(!streams_from_db("Part", true, false, IN_WORKSPACE));
    }

    #[test]
    fn a_custom_mesh_never_streams() {
        assert!(!streams_from_db("Part", false, true, IN_WORKSPACE));
    }

    /// Templates and materials live outside the Workspace; a core would spawn
    /// them into the world. `Workspace_backup` checks the prefix boundary.
    #[test]
    fn a_part_outside_the_workspace_never_streams() {
        for rel in [
            "ReplicatedStorage/Car/Body/_instance.toml",
            "ServerStorage/Template/_instance.toml",
            "MaterialService/Plate/_instance.toml",
            "Workspace_backup/Part/_instance.toml",
        ] {
            assert!(!streams_from_db("Part", false, false, rel), "{rel} must not stream");
        }
    }

    /// Everything the old exclude-list admitted, plus the Part subclasses and
    /// the file-natured classes it already refused.
    #[test]
    fn only_parts_stream() {
        for c in [
            "Texture", "Decal", "Weld", "Sound", "Folder", "Beam", "ParticleEmitter",
            "LuauScript", "LuauModuleScript", "LuauLocalScript", "Model",
            "SpawnLocation", "Seat", "VehicleSeat",
            "SoulScript", "ScreenGui", "TextLabel", "Atmosphere", "Sky",
            "WedgePart", "TrussPart",
        ] {
            assert!(!streams_from_db(c, false, false, IN_WORKSPACE), "{c} must not stream");
        }
    }

    /// `file_loader` passes the Debug name of the class it parsed; `bake_cores`
    /// and `echk_export` pass the raw TOML string. Both must decide alike.
    #[test]
    fn raw_and_parsed_class_names_decide_alike() {
        for raw in ["Part", "Script", "SoulScript", "NoSuchClass", "part", "MeshPart"] {
            let parsed = format!("{:?}", class_from_toml(raw));
            assert_eq!(
                streams_from_db(raw, false, false, IN_WORKSPACE),
                streams_from_db(&parsed, false, false, IN_WORKSPACE),
                "{raw} vs {parsed}"
            );
        }
    }
}

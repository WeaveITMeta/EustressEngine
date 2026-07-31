//! # Asset-source registration, shared by both shells
//!
//! ## The bug this replaces
//!
//! `bundled://` was registered only by the engine crate
//! (`engine/src/app_core.rs:70`). The Client never registered it, so
//! `spawn_skinned_character` — which loads its body mesh and all eight
//! animation clips through `bundled://` — resolved nothing, and the Client
//! player was invisible. Three separate audit findings (1, 21, 43) were this
//! one omission observed from three directions.
//!
//! ## The second bug, which nobody had noticed
//!
//! The engine's registration used a **compile-time** `CARGO_MANIFEST_DIR`
//! path with no exe-adjacent fallback, unlike the `default://` source which
//! does fall back (`engine/src/main.rs:175-184`). That path does not exist on
//! any machine except the one that compiled the binary, so **packaged Studio
//! builds cannot load a character either** — the avatar only ever worked when
//! run from a dev checkout.
//!
//! Resolution order here is exe-adjacent first, then the compile-time source
//! path, so a shipped build works and a dev build still picks up live asset
//! edits.

use bevy::prelude::*;
// Logging macros come in explicitly, not through `bevy::prelude::*`.
// The prelude only re-exports them when Bevy's `bevy_log` feature is on, and
// feature unification across the test target can turn it off — which made
// `cargo test -p eustress-common --lib` fail to compile this module while the
// ordinary lib build succeeded. Importing from `tracing` (what `bevy_log`
// re-exports anyway) makes the module build under every feature combination.
// An explicit import also shadows the glob, so there is no ambiguity.
use tracing::{debug, error, info, warn};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by [`register_avatar_asset_sources`] so [`super::AvatarRuntimePlugin`]
/// can turn "you forgot to call this" into a startup panic carrying the fix,
/// rather than a 404 that reads as an animation bug.
static REGISTERED: AtomicBool = AtomicBool::new(false);

pub(crate) fn avatar_asset_sources_registered() -> bool {
    REGISTERED.load(Ordering::Relaxed)
}

/// Candidate roots for the bundled common assets, best first.
fn bundled_asset_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    // 1. Next to the executable — how a shipped build finds its assets.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("assets"));
            out.push(dir.join("common/assets"));
            // cargo puts binaries in target/<profile>/, so a dev run from a
            // checkout reaches the crate tree in three hops.
            out.push(dir.join("../../../crates/common/assets"));
        }
    }

    // 2. The compile-time source tree — correct for `cargo run`, absent in a
    //    shipped build. Kept as a fallback rather than the primary.
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"));

    // 3. Relative to the working directory, for `cargo run -p` from the
    //    workspace root.
    out.push(PathBuf::from("eustress/crates/common/assets"));
    out.push(PathBuf::from("crates/common/assets"));

    out
}

/// Resolve the bundled asset root, preferring a directory that actually
/// contains the character assets over one that merely exists.
fn resolve_bundled_root() -> PathBuf {
    let candidates = bundled_asset_candidates();

    // A directory is only the right one if the avatar assets are in it.
    // Without this check an empty `assets/` next to the exe would win and the
    // failure would resurface as a 404 with a plausible-looking root in the
    // log.
    for c in &candidates {
        if c.join("characters/y_bot.glb").is_file() {
            return c.clone();
        }
    }
    for c in &candidates {
        if c.is_dir() {
            warn!(
                "avatar: bundled asset root {:?} exists but has no characters/y_bot.glb — \
                 character assets will fail to load",
                c
            );
            return c.clone();
        }
    }

    error!(
        "avatar: no bundled asset root found. Tried: {:#?}\n\
         Character meshes and animation clips will not load.",
        candidates
    );
    candidates.into_iter().next().unwrap_or_else(|| PathBuf::from("assets"))
}

/// Register every asset source the avatar runtime needs.
///
/// MUST be called before the shell adds `AssetPlugin` (directly or via
/// `DefaultPlugins`). Bevy freezes the asset-source table when `AssetPlugin`
/// builds, so calling this afterwards silently does nothing.
///
/// Idempotent: registering the same source twice would panic inside Bevy, and
/// the engine shell also registers `space://` for its own reasons, so this
/// guards itself.
pub fn register_avatar_asset_sources(app: &mut App) {
    if REGISTERED.swap(true, Ordering::Relaxed) {
        debug!("avatar: asset sources already registered, skipping");
        return;
    }

    let root = resolve_bundled_root();
    info!("avatar: bundled asset source -> {:?}", root);

    app.register_asset_source(
        "bundled",
        bevy::asset::io::AssetSourceBuilder::platform_default(&root.to_string_lossy(), None),
    );
}

/// The resolved bundled asset root, for code that must read a file directly
/// rather than through `AssetServer` (clip retargeting parses the source glTF
/// to recover its node hierarchy, which the loaded `AnimationClip` no longer
/// carries).
pub fn bundled_root() -> PathBuf {
    resolve_bundled_root()
}

/// Register every component type a glTF scene graph carries, so the
/// `WorldAsset` spawner never panics with "unregistered type" mid-load.
///
/// This list previously lived only in `engine/src/app_core.rs`, whose own doc
/// noted it is "needed by any shell that spawns glb-backed instances (both
/// do)" — while the Client registered exactly three types. The bug was latent
/// because the Client never registered `bundled://` either, so no character
/// glTF ever loaded and the spawner never ran. Fixing the asset source
/// un-masked this immediately.
///
/// Shared here so a shell cannot get one without the other.
pub fn register_gltf_scene_types(app: &mut App) {
    use bevy::gltf::{
        GltfExtras, GltfMaterialExtras, GltfMaterialName, GltfMeshExtras, GltfMeshName,
        GltfSceneExtras, GltfSceneName,
    };

    app.register_type::<GltfExtras>()
        .register_type::<GltfSceneExtras>()
        .register_type::<GltfMeshExtras>()
        .register_type::<GltfMaterialExtras>()
        .register_type::<GltfSceneName>()
        .register_type::<GltfMeshName>()
        .register_type::<GltfMaterialName>()
        // Hierarchy / transform / visibility / name.
        .register_type::<bevy::transform::components::TransformTreeChanged>()
        .register_type::<Children>()
        .register_type::<ChildOf>()
        .register_type::<Transform>()
        .register_type::<GlobalTransform>()
        .register_type::<Visibility>()
        .register_type::<InheritedVisibility>()
        .register_type::<ViewVisibility>()
        .register_type::<Name>()
        // Render components on glTF mesh entities.
        .register_type::<Mesh3d>()
        .register_type::<MeshMaterial3d<StandardMaterial>>()
        .register_type::<bevy::camera::primitives::Aabb>()
        // Skinning + animation — carried by every rigged character glTF.
        //
        // Absent from the engine's original list because that list was written
        // for a "static glb scene graph" (its own words) and never covered a
        // rigged body. `DynamicSkinnedMeshBounds` in particular is inserted by
        // Bevy on skinned mesh entities for dynamic culling bounds, and is the
        // type that was actually panicking the spawner — found by building with
        // `bevy/debug` rather than by guessing.
        .register_type::<bevy::camera::visibility::DynamicSkinnedMeshBounds>()
        .register_type::<bevy::mesh::skinning::SkinnedMesh>()
        .register_type::<bevy::animation::AnimationPlayer>()
        // Bevy 0.19 names the entity→player link `AnimatedBy`; there is no
        // `AnimationTarget` type.
        .register_type::<bevy::animation::AnimatedBy>()
        .register_type::<bevy::animation::AnimationTargetId>();
}

/// Mark the sources as externally registered.
///
/// The engine shell registers `bundled://` inside its own
/// `app_core::register_asset_sources` for non-avatar reasons (material
/// textures, fonts). Calling this from there keeps a single registration
/// while still satisfying the runtime's boot assertion.
pub fn note_asset_sources_registered_externally() {
    REGISTERED.store(true, Ordering::Relaxed);
}

/// Bevy 0.19 refuses to read assets from absolute paths outside the asset
/// root unless this is relaxed. Studio already hit this with Gaussian-splat
/// and image imports; the Client never set it at all.
pub fn permissive_asset_plugin(file_path: impl Into<String>) -> bevy::asset::AssetPlugin {
    bevy::asset::AssetPlugin {
        file_path: file_path.into(),
        unapproved_path_mode: bevy::asset::UnapprovedPathMode::Allow,
        ..default()
    }
}

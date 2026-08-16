//! Opens a Eustress Space in the Player instead of the hardcoded demo scene.
//!
//! Before this, the Client's world was `spawn_baseplate` + `spawn_welcome_cube`
//! — two hardcoded primitives — and `space_fetch` unpacked archives that
//! nothing consumed. There was no concept of a *current Space* anywhere in the
//! shell, which is why published content could never be played.
//!
//! Which Space, in order:
//! 1. `EUSTRESS_SPACE` — an absolute path to a Space root.
//! 2. `<workspace>/Movement/Spaces/Climbing` — the movement test course.
//! 3. Nothing; the shell keeps its demo scene.
//!
//! The workspace root resolution mirrors the engine's: `EUSTRESS_WORKSPACE`,
//! else `~/Documents/Eustress`. It deliberately does **not** use
//! `dirs::document_dir()`, because on Windows that follows OneDrive's Known
//! Folder Move and lands somewhere the engine is not looking.

use bevy::prelude::*;
use eustress_common::space_read::{read_space_parts, spawn_space_parts};
use std::path::PathBuf;

/// Where the Space came from, so the rest of the shell can tell whether it is
/// showing authored content or the fallback demo.
#[derive(Resource, Debug, Clone, Default)]
pub struct OpenSpace {
    pub root: Option<PathBuf>,
    pub parts: usize,
    pub skipped: usize,
}

pub struct SpaceWorldPlugin;

impl Plugin for SpaceWorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenSpace>()
            .add_systems(Startup, open_space);
    }
}

fn workspace_root() -> Option<PathBuf> {
    if let Some(w) = std::env::var_os("EUSTRESS_WORKSPACE") {
        return Some(PathBuf::from(w));
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join("Documents").join("Eustress"))
}

/// True when a real Space will be opened, so the shell can skip its built-in
/// demo scene. Without this the hardcoded 512 m baseplate and welcome cube
/// spawn UNDERNEATH the authored course — overlapping ground planes and a
/// cube sitting at the origin of the level.
pub fn space_is_available() -> bool {
    resolve_space().is_some()
}

fn resolve_space() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("EUSTRESS_SPACE") {
        let p = PathBuf::from(p);
        return p.join("Workspace").is_dir().then_some(p);
    }
    let candidate = workspace_root()?
        .join("Movement")
        .join("Spaces")
        .join("Climbing");
    candidate.join("Workspace").is_dir().then_some(candidate)
}

fn open_space(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut open: ResMut<OpenSpace>,
) {
    let Some(root) = resolve_space() else {
        info!("space: none found — keeping the built-in demo scene");
        return;
    };

    let geo = match read_space_parts(&root) {
        Ok(g) => g,
        Err(e) => {
            // Loud, not silent: a Space that fails to open is the difference
            // between testing the course and testing an empty plane.
            error!("space: {} failed to open — {e}", root.display());
            return;
        }
    };

    for err in &geo.errors {
        warn!("space: {err}");
    }
    if !geo.skipped.is_empty() {
        // Reported by class so "the course looks wrong" has an answer that is
        // not "read the loader".
        warn!("space: skipped non-Part content: {:?}", geo.skipped);
    }

    let spawned = spawn_space_parts(&mut commands, &mut meshes, &mut materials, &geo);
    open.root = Some(root.clone());
    open.parts = spawned;
    open.skipped = geo.skipped_total();

    info!(
        "space: opened {} — {spawned} parts ({} skipped, {} errors)",
        root.display(),
        open.skipped,
        geo.errors.len()
    );
}

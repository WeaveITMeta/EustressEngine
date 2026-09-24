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
use eustress_common::terrain::layer_instances::{read_layer_instances, spawn_layer_instances};
use std::path::PathBuf;

/// Where the Space came from, so the rest of the shell can tell whether it is
/// showing authored content or the fallback demo.
#[derive(Resource, Debug, Clone, Default)]
pub struct OpenSpace {
    pub root: Option<PathBuf>,
    pub parts: usize,
    pub skipped: usize,
}

/// Open a Space folder now. How a world that arrived at runtime (from a host,
/// or a published simulation) gets opened.
#[derive(Message, Debug, Clone)]
pub struct OpenSpaceRequest {
    pub root: PathBuf,
}

/// A Space finished opening: its parts and terrain layers are spawned.
#[derive(Message, Debug, Clone)]
pub struct SpaceOpened {
    pub root: PathBuf,
    pub parts: usize,
}

pub struct SpaceWorldPlugin {
    /// Open the local Space (`EUSTRESS_SPACE`, else the movement course) at
    /// startup. Off when the Player was launched to join a host or play a
    /// published simulation: that world arrives later, through
    /// [`OpenSpaceRequest`].
    pub open_local: bool,
}

impl Plugin for SpaceWorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpenSpace>()
            .add_message::<OpenSpaceRequest>()
            .add_message::<SpaceOpened>()
            .add_systems(Update, open_requested_space);
        if self.open_local {
            app.add_systems(Startup, open_local_space);
        }
    }
}

pub(crate) fn workspace_root() -> Option<PathBuf> {
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

pub(crate) fn resolve_space() -> Option<PathBuf> {
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

fn open_local_space(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut open: ResMut<OpenSpace>,
    mut opened: MessageWriter<SpaceOpened>,
) {
    let Some(root) = resolve_space() else {
        info!("space: none found — keeping the built-in demo scene");
        return;
    };
    if let Some(parts) = open_space_at(&root, &mut commands, &mut meshes, &mut materials, &mut open) {
        opened.write(SpaceOpened { root, parts });
    }
}

/// Open each requested Space, replacing whatever Space parts are already in
/// the world.
fn open_requested_space(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut open: ResMut<OpenSpace>,
    mut requests: MessageReader<OpenSpaceRequest>,
    mut opened: MessageWriter<SpaceOpened>,
    existing: Query<Entity, With<eustress_common::space_read::SpawnedFromSpace>>,
) {
    let Some(request) = requests.read().last().cloned() else { return };
    for e in &existing {
        commands.entity(e).try_despawn();
    }
    if let Some(parts) = open_space_at(&request.root, &mut commands, &mut meshes, &mut materials, &mut open) {
        opened.write(SpaceOpened { root: request.root, parts });
    }
}

/// Read a Space folder and spawn its parts and terrain layers. Returns the
/// number of parts, or `None` when the Space could not be read.
fn open_space_at(
    root: &std::path::Path,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    open: &mut OpenSpace,
) -> Option<usize> {
    let root = root.to_path_buf();
    let geo = match read_space_parts(&root) {
        Ok(g) => g,
        Err(e) => {
            // Loud, not silent: a Space that fails to open is the difference
            // between testing the course and testing an empty plane.
            error!("space: {} failed to open — {e}", root.display());
            return None;
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

    let spawned = spawn_space_parts(commands, meshes, materials, &geo);

    // Terrain layers (roads, stamps, pads, noise, material fills), scatter
    // layers and water bodies are instances too; the shared terrain plugin
    // bakes the layers over whatever terrain the Space spawns, and places
    // the scatter and floods the water bodies on the result.
    let (layers, layer_problems) = read_layer_instances(&root);
    for problem in &layer_problems {
        warn!("space: terrain layer {problem}");
    }
    let layer_entities = spawn_layer_instances(commands, &layers);
    if layer_entities > 0 {
        info!("space: {layer_entities} terrain layer instances");
    }

    open.root = Some(root.clone());
    open.parts = spawned;
    open.skipped = geo.skipped_total();

    info!(
        "space: opened {} — {spawned} parts ({} skipped, {} errors)",
        root.display(),
        open.skipped,
        geo.errors.len()
    );
    Some(spawned)
}

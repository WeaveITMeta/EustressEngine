//! The engine bridge's `space.open`, `assets.list_meshes` and
//! `assets.reload_mesh`. The MCP `open_space`, `list_meshes` and
//! `reload_mesh` tools call them.
//!
//! `space.open` is Studio's own File > Open (`space_ops::open_space`): it
//! saves pending edits, despawns the open Space and loads the new one over
//! many frames, so it is answered once the load settles. It also clears the
//! undo history, whose steps name the Space being closed; that, and changing
//! the Space the person works in, cannot be undone, so the MCP tool is
//! classed Destructive and asks first.
//!
//! `assets.reload_mesh` asks the asset server to load a mesh file again, so a
//! mesh replaced on disk (or one whose earlier load failed and stayed cached
//! by its URL) shows without a restart: every part drawing it holds the same
//! handle, which the reload fills.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::server::Pending;
use super::{BridgeError, BridgeRequest, BridgeResponse};
use crate::space::InstanceFile;

/// How long a `space.open` waits for the load to settle before answering
/// that it is still loading.
const OPEN_TIMEOUT: Duration = Duration::from_secs(300);

fn refuse(req: &BridgeRequest, why: impl Into<String>) -> BridgeResponse {
    BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(why.into()))
}

// ---------------------------------------------------------------------------
// space.open
// ---------------------------------------------------------------------------

/// The Space `params` names: `name`, a Space folder beside the open one in
/// this Universe's Spaces folder. `Err` says why not.
pub(super) fn space_to_open(world: &World, params: &Value) -> Result<PathBuf, String> {
    super::properties::editing_only(world)?;
    let current = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone()).ok_or("no Space is open")?;
    let spaces = current.parent().ok_or("the open Space has no Spaces folder")?;
    let raw = params.get("name").and_then(Value::as_str).ok_or("`name` is the Space's folder name in this Universe")?;
    let name = eustress_tools::sandbox::plain_name(raw).map_err(|e| e.to_string())?;
    let path = spaces.join(&name);
    let meta = std::fs::symlink_metadata(&path).map_err(|_| format!("there is no Space named '{name}' in this Universe"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!("'{name}' is not a Space folder"));
    }
    if path == current {
        return Err(format!("'{name}' is already open"));
    }
    Ok(path)
}

struct Inflight {
    responder: oneshot::Sender<BridgeResponse>,
    id: Value,
    path: PathBuf,
    started: Instant,
}

/// The `space.open` waiting for its load to settle.
#[derive(Resource, Default)]
pub(super) struct SpaceOpenJob(Option<Inflight>);

/// Start a `space.open`; it is answered by [`pump_space_open`].
///
/// The MCP tool's "ask first" (`requires_approval`, and its Destructive
/// class) is kept by Workshop and by the MCP client's own permission prompt,
/// not here: a bridge client holding this launch's token can call
/// `space.open` directly. That is the residual accepted for the bridge (the
/// token holder is this user), not a check this method makes.
pub fn begin_space_open(world: &mut World, pending: Pending) {
    let Pending { request, responder } = pending;
    if world.get_resource::<SpaceOpenJob>().is_some_and(|j| j.0.is_some()) {
        let _ = responder.send(refuse(&request, "a Space is already opening; wait for it"));
        return;
    }
    let path = match space_to_open(world, &request.params) {
        Ok(path) => path,
        Err(e) => {
            let _ = responder.send(refuse(&request, e));
            return;
        }
    };
    crate::space::space_ops::open_space(world, &path);
    let opened = world.get_resource::<crate::space::SpaceRoot>().is_some_and(|r| r.0 == path);
    if !opened {
        let _ = responder.send(refuse(&request, "the Space could not be opened; the Output panel says why"));
        return;
    }
    // Every step in the history names the Space just closed.
    if let Some(mut stack) = world.get_resource_mut::<crate::undo::UndoStack>() {
        stack.clear();
    }
    info!("bridge space.open {}", path.display());
    world.get_resource_or_insert_with(SpaceOpenJob::default).0 =
        Some(Inflight { responder, id: request.id.clone(), path, started: Instant::now() });
}

/// Answer the waiting `space.open` once the load settles (or after
/// [`OPEN_TIMEOUT`], saying it is still loading).
pub(super) fn pump_space_open(world: &mut World) {
    let Some(job) = world.get_resource_mut::<SpaceOpenJob>().and_then(|mut j| j.0.take()) else { return };
    let loading = world.get_resource::<crate::space::file_loader::LoadInProgress>().is_some_and(|l| l.active);
    let timed_out = job.started.elapsed() >= OPEN_TIMEOUT;
    if loading && !timed_out {
        world.resource_mut::<SpaceOpenJob>().0 = Some(job);
        return;
    }
    let name = job.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let _ = job.responder.send(BridgeResponse::ok(
        job.id,
        json!({
            "opened": name,
            "loaded": !loading,
            "seconds": job.started.elapsed().as_secs_f32(),
            "undo_history": "cleared",
        }),
    ));
}

// ---------------------------------------------------------------------------
// assets.list_meshes / assets.reload_mesh
// ---------------------------------------------------------------------------

/// Most distinct meshes a listing returns.
const MAX_LISTED: usize = 5_000;

/// The meshes the open Space's parts draw, Space-relative, with how many
/// parts draw each, most used first. Meshes outside the Space are counted,
/// not named.
pub(super) fn mesh_counts(world: &mut World) -> Result<(Vec<(String, usize)>, usize), String> {
    let root = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone()).ok_or("no Space is open")?;
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut elsewhere = 0usize;
    let mut q = world.query::<&InstanceFile>();
    for file in q.iter(world) {
        if file.mesh_path.as_os_str().is_empty() {
            continue;
        }
        match file.mesh_path.strip_prefix(&root) {
            Ok(rel) => *counts.entry(rel.to_string_lossy().replace('\\', "/")).or_default() += 1,
            Err(_) => elsewhere += 1,
        }
    }
    let mut list: Vec<(String, usize)> = counts.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    list.truncate(MAX_LISTED);
    Ok((list, elsewhere))
}

/// `assets.list_meshes`: see [`mesh_counts`].
pub fn list_meshes(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    match mesh_counts(world) {
        Ok((list, elsewhere)) => BridgeResponse::ok(
            req.id.clone(),
            json!({
                "meshes": list.iter().map(|(path, parts)| json!({ "path": path, "parts": parts })).collect::<Vec<_>>(),
                "count": list.len(),
                "parts_drawing_meshes_outside_the_space": elsewhere,
            }),
        ),
        Err(e) => refuse(req, e),
    }
}

/// The Space-relative mesh file `raw` names, checked: inside the Space, a
/// file, never through `..`, a drive or a link.
pub(super) fn mesh_in_space(root: &Path, raw: &str) -> Result<String, String> {
    let rel = eustress_tools::sandbox::relative(raw).map_err(|e| e.to_string())?;
    if rel.as_os_str().is_empty() {
        return Err("`path` names the mesh file, relative to the Space".to_string());
    }
    let full = root.join(&rel);
    let meta = std::fs::symlink_metadata(&full).map_err(|_| format!("{raw} is not a file in this Space"))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(format!("{raw} is not a file in this Space"));
    }
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

/// `assets.reload_mesh`: `path`, a mesh file relative to the Space. Loads it
/// again; the parts drawing it update when the load finishes.
pub fn reload_mesh(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let result = (|| -> Result<Value, String> {
        super::properties::editing_only(world)?;
        let root = world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone()).ok_or("no Space is open")?;
        let raw = req.params.get("path").and_then(Value::as_str).ok_or("`path` names the mesh file, relative to the Space")?;
        let rel = mesh_in_space(&root, raw)?;
        // The URL the loader drew it from (`space://<relative>`, under the
        // asset source's own root).
        let asset_root = crate::space::space_asset_source::space_asset_root();
        let url_rel = root
            .join(&rel)
            .strip_prefix(&asset_root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| rel.clone());
        let parts = mesh_counts(world)?.0.into_iter().find(|(p, _)| *p == rel).map_or(0, |(_, n)| n);
        let assets = world.get_resource::<AssetServer>().ok_or("the asset server is not running")?;
        assets.reload(format!("space://{url_rel}"));
        info!("bridge assets.reload_mesh {rel} ({parts} parts)");
        Ok(json!({
            "reloaded": rel,
            "parts": parts,
            "note": "The parts drawing it update when the load finishes, usually within a few frames.",
        }))
    })();
    match result {
        Ok(v) => BridgeResponse::ok(req.id.clone(), v),
        Err(e) => refuse(req, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eus_assets_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_mesh_path_must_be_a_file_inside_the_space() {
        let root = scratch("mesh");
        std::fs::create_dir_all(root.join("Meshes")).unwrap();
        std::fs::write(root.join("Meshes").join("car.glb"), [0u8; 4]).unwrap();
        assert_eq!(mesh_in_space(&root, "Meshes/car.glb").as_deref(), Ok("Meshes/car.glb"));
        assert_eq!(mesh_in_space(&root, "Meshes\\car.glb").as_deref(), Ok("Meshes/car.glb"));
        for bad in ["", "../car.glb", "C:/Windows/notepad.exe", "Meshes/missing.glb", "Meshes", "/etc/passwd"] {
            assert!(mesh_in_space(&root, bad).is_err(), "{bad:?}");
        }
        let _ = std::fs::remove_dir_all(root);
    }
}

/// The bridge path on a bare World with the resources it reads.
#[cfg(test)]
mod engine_tests {
    use super::*;
    use crate::play_mode::PlayModeState;
    use crate::space::SpaceRoot;

    fn universe(tag: &str) -> (World, PathBuf) {
        let spaces = std::env::temp_dir().join(format!("eus_open_{tag}_{}", std::process::id())).join("Spaces");
        let _ = std::fs::remove_dir_all(spaces.parent().unwrap());
        std::fs::create_dir_all(spaces.join("Open")).unwrap();
        std::fs::create_dir_all(spaces.join("Other")).unwrap();
        let mut world = World::new();
        world.insert_resource(SpaceRoot(spaces.join("Open")));
        world.insert_resource(State::new(PlayModeState::Editing));
        (world, spaces)
    }

    #[test]
    fn only_another_space_in_this_universe_is_opened() {
        let (mut world, spaces) = universe("plan");
        assert_eq!(space_to_open(&world, &json!({ "name": "Other" })).unwrap(), spaces.join("Other"));
        assert!(space_to_open(&world, &json!({ "name": "Open" })).unwrap_err().contains("already open"));
        assert!(space_to_open(&world, &json!({ "name": "Missing" })).is_err());
        assert!(space_to_open(&world, &json!({ "name": "../Other" })).is_err());
        assert!(space_to_open(&world, &json!({ "name": "C:\\\\Windows" })).is_err());
        world.insert_resource(State::new(PlayModeState::Playing));
        assert!(space_to_open(&world, &json!({ "name": "Other" })).unwrap_err().contains("Play"));
        let _ = std::fs::remove_dir_all(spaces.parent().unwrap());
    }

    #[test]
    fn meshes_are_listed_by_use_and_outside_ones_only_counted() {
        let (mut world, spaces) = universe("list");
        let root = spaces.join("Open");
        let mesh = |rel: &str| InstanceFile {
            toml_path: root.join("Workspace/x/_instance.toml"),
            mesh_path: root.join(rel),
            name: "x".into(),
        };
        world.spawn(mesh("Meshes/wheel.glb"));
        world.spawn(mesh("Meshes/wheel.glb"));
        world.spawn(mesh("Meshes/body.glb"));
        world.spawn(InstanceFile { toml_path: PathBuf::new(), mesh_path: PathBuf::from("C:/elsewhere/x.glb"), name: "y".into() });
        world.spawn(InstanceFile { toml_path: PathBuf::new(), mesh_path: PathBuf::new(), name: "z".into() });
        let (list, elsewhere) = mesh_counts(&mut world).unwrap();
        assert_eq!(list, vec![("Meshes/wheel.glb".to_string(), 2), ("Meshes/body.glb".to_string(), 1)]);
        assert_eq!(elsewhere, 1);
        let _ = std::fs::remove_dir_all(spaces.parent().unwrap());
    }
}

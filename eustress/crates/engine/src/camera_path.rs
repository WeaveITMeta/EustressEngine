//! # Camera path player (G1.04)
//!
//! Drives the AI camera ([`crate::ai_camera::AiCamera`]) through a declared
//! list of exact poses, one pose per tick, so a capture harness can say "frame
//! N of path CP-B" and mean the same shot on every run and on every machine
//! (`docs/PROMPTS/02_CAPTURE_HARNESS.md` invariant I2).
//!
//! ## Contract
//!
//! * Armed by the environment, with no other trigger: `EUSTRESS_CAMERA_PATH`
//!   names the path (`CP-A`, `CP-B`, `CP-S`). Without it this plugin does
//!   nothing and costs nothing.
//! * `EUSTRESS_CAMERA_PATH_FILE` is the absolute location of the path file. The
//!   engine may be running on a copy of the Space, so the name alone does not
//!   locate it. Without it the player looks for
//!   `spaces/harness/paths/<name>.json` under the working directory and under
//!   each ancestor of the executable's folder.
//! * `EUSTRESS_CAMERA_PATH_LOG` (optional) is where the pose log is written
//!   when the path finishes; `EUSTRESS_CAMERA_PATH_EXIT=1` (optional) then
//!   sends `AppExit::Success`. `harness-scenes paths --verify` sets both.
//!
//! ## What a tick is
//!
//! One run of the `FixedUpdate` schedule (`Time<Fixed>`, 60 Hz in this
//! engine). It is counted by the player itself, from the tick the path
//! starts. It is neither wall-clock time nor a frame count: a slow machine
//! runs `FixedUpdate` more than once in a frame, a fast one skips frames, and
//! the pose for path tick N is the same in both.
//!
//! The index is not `SimulationClock::tick_count` and not
//! `PhysicsStepCounter`: the clock advances only while a Play session is
//! running and the physics counter only while Avian's `Time<Physics>` is not
//! paused, and a measurement run opens the Space in the editor with neither
//! running. `FixedUpdate` runs in every mode, which is also the index the
//! frame-time recorder falls back on (`profiler.rs`, `count_fixed_step`).
//!
//! ## No interpolation
//!
//! A path file enumerates one pose per tick. The player sets the pose of tick
//! N and does nothing else: no easing, no smoothing, no snapping.
//!
//! ## The pose, and its readback one tick later
//!
//! The pose is written the way the bridge's `ai_camera.set_pose` writes the
//! `rotation` form: `Transform.translation = position`, `Transform.rotation =
//! Quat::from_xyzw(..)`, no normalisation. (That handler is private to
//! `engine_bridge`, so the player repeats its two assignments.) On the next
//! tick, before it sets the next pose, the player reads the camera's
//! `Transform` back and logs the raw `f32` bit patterns. A readback that
//! differs from the pose that was set is recorded as a mismatch and is never
//! corrected. The last pose is read back one tick after it is set, so a path
//! of N poses takes N + 1 ticks and logs N rows.
//!
//! The camera keeps its `NoAtmosphere` marker; nothing here touches it.

use bevy::app::AppExit;
use bevy::prelude::*;
use std::path::{Path, PathBuf};

use crate::ai_camera::AiCamera;

/// The environment variable that arms the player, and names the path.
pub const ENV_PATH: &str = "EUSTRESS_CAMERA_PATH";
/// Absolute location of the path file.
pub const ENV_FILE: &str = "EUSTRESS_CAMERA_PATH_FILE";
/// Where the pose log is written when the path finishes.
pub const ENV_LOG: &str = "EUSTRESS_CAMERA_PATH_LOG";
/// `1` exits the engine once the log is written.
pub const ENV_EXIT: &str = "EUSTRESS_CAMERA_PATH_EXIT";

/// The path file schema this player reads.
pub const PATH_SCHEMA: &str = "eustress.camera_path/1";
/// The log schema this player writes.
pub const LOG_SCHEMA: &str = "eustress.camera_path_log/1";

/// CONFIG DEFAULT: ticks to wait, after the Space's deferred services have
/// loaded and the AI camera exists, before tick 0 of the path (2 s at 60 Hz).
pub const SETTLE_TICKS: u32 = 120;

/// One enumerated pose: position in metres, rotation as `[x, y, z, w]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathPose {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

impl PathPose {
    /// The seven raw `f32` bit patterns: position xyz, then rotation xyzw.
    pub fn bits(&self) -> [u32; 7] {
        [
            self.position[0].to_bits(),
            self.position[1].to_bits(),
            self.position[2].to_bits(),
            self.rotation[0].to_bits(),
            self.rotation[1].to_bits(),
            self.rotation[2].to_bits(),
            self.rotation[3].to_bits(),
        ]
    }
}

/// The seven raw `f32` bit patterns of a `Transform`'s translation and rotation.
pub fn transform_bits(tf: &Transform) -> [u32; 7] {
    [
        tf.translation.x.to_bits(),
        tf.translation.y.to_bits(),
        tf.translation.z.to_bits(),
        tf.rotation.x.to_bits(),
        tf.rotation.y.to_bits(),
        tf.rotation.z.to_bits(),
        tf.rotation.w.to_bits(),
    ]
}

/// A loaded pose path.
#[derive(Clone, Debug)]
pub struct LoadedPath {
    pub id: String,
    pub ticks_declared: usize,
    pub poses: Vec<PathPose>,
}

/// Parse a path file (`eustress.camera_path/1`, `kind = "poses"`). The pose at
/// array index `i` must declare `tick = i`: a path is an enumeration, and a
/// gap or a reorder is an error, not something to fill in.
pub fn parse_path(text: &str) -> Result<LoadedPath, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    if v.get("schema").and_then(|s| s.as_str()) != Some(PATH_SCHEMA) {
        return Err(format!("schema is not {PATH_SCHEMA:?}"));
    }
    let kind = v.get("kind").and_then(|s| s.as_str()).unwrap_or("");
    if kind != "poses" {
        return Err(format!("kind is {kind:?}, not \"poses\": this path carries no camera poses"));
    }
    let id = v.get("id").and_then(|s| s.as_str()).ok_or("no id")?.to_string();
    let ticks_declared = v.get("ticks").and_then(|n| n.as_u64()).ok_or("no ticks")? as usize;
    let list = v.get("poses").and_then(|p| p.as_array()).ok_or("no poses array")?;
    if list.len() != ticks_declared {
        return Err(format!("declares {ticks_declared} ticks but lists {} poses", list.len()));
    }
    let mut poses = Vec::with_capacity(list.len());
    for (i, p) in list.iter().enumerate() {
        if p.get("tick").and_then(|t| t.as_u64()) != Some(i as u64) {
            return Err(format!("pose {i} does not declare tick {i}"));
        }
        let mut pose = PathPose { position: [0.0; 3], rotation: [0.0; 4] };
        for (key, out) in [("position", &mut pose.position[..]), ("rotation", &mut pose.rotation[..])] {
            let a = p.get(key).and_then(|a| a.as_array()).ok_or_else(|| format!("pose {i}: no {key}"))?;
            if a.len() != out.len() {
                return Err(format!("pose {i}: {key} has {} numbers, wants {}", a.len(), out.len()));
            }
            for (slot, n) in out.iter_mut().zip(a) {
                let x = n.as_f64().ok_or_else(|| format!("pose {i}: {key} holds a non-number"))?;
                if !x.is_finite() {
                    return Err(format!("pose {i}: {key} holds a non-finite number"));
                }
                *slot = x as f32;
            }
        }
        poses.push(pose);
    }
    Ok(LoadedPath { id, ticks_declared, poses })
}

/// Where the player is in its run.
#[derive(Clone, Debug, PartialEq)]
enum Phase {
    /// Waiting for the Space to load and the camera to exist, then the settle.
    Waiting { settle_left: u32 },
    /// `next` is the index of the pose to set on the coming tick.
    Playing { next: usize },
    Done,
}

/// What one call to [`step_path`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// Not started: the Space is still loading, or the settle has not run out.
    Waiting,
    /// The pose for this path tick was set.
    Set(usize),
    /// The last pose was read back; the path is finished.
    Finished,
    /// The path was already finished.
    Idle,
}

/// The player's state: the path, the phase, and the log it builds.
#[derive(Resource)]
pub struct CameraPathPlayer {
    path: LoadedPath,
    phase: Phase,
    /// One row of seven bit patterns per path tick, read back from the camera.
    log: Vec<[u32; 7]>,
    /// Path ticks whose readback differed from the pose that was set.
    readback_mismatches: Vec<usize>,
    /// Set when the run could not be carried out; the log carries it.
    error: Option<String>,
    log_path: Option<PathBuf>,
    exit_when_done: bool,
    written: bool,
}

impl CameraPathPlayer {
    /// A player for `path`, waiting for the Space to load.
    pub fn new(path: LoadedPath) -> Self {
        Self {
            path,
            phase: Phase::Waiting { settle_left: SETTLE_TICKS },
            log: Vec::new(),
            readback_mismatches: Vec::new(),
            error: None,
            log_path: None,
            exit_when_done: false,
            written: false,
        }
    }

    fn failed(id: String, message: String) -> Self {
        let mut player = Self::new(LoadedPath { id, ticks_declared: 0, poses: Vec::new() });
        player.phase = Phase::Done;
        player.error = Some(message);
        player
    }

    /// Armed from the environment, or `None` when `EUSTRESS_CAMERA_PATH` is not set.
    pub fn from_env() -> Option<Self> {
        let name = std::env::var(ENV_PATH).ok().filter(|s| !s.trim().is_empty())?;
        let name = name.trim().to_string();
        let mut player = match locate_path_file(&name).and_then(|file| {
            std::fs::read_to_string(&file)
                .map_err(|e| format!("cannot read {}: {e}", file.display()))
                .and_then(|text| parse_path(&text).map_err(|e| format!("{}: {e}", file.display())))
        }) {
            Ok(path) if path.id == name => Self::new(path),
            Ok(path) => Self::failed(name.clone(), format!("the file declares id {:?}, not {name:?}", path.id)),
            Err(e) => Self::failed(name.clone(), e),
        };
        player.log_path = std::env::var(ENV_LOG).ok().filter(|s| !s.is_empty()).map(PathBuf::from);
        player.exit_when_done = std::env::var(ENV_EXIT).map(|v| !v.is_empty() && v != "0").unwrap_or(false);
        Some(player)
    }

    /// Path ticks logged so far.
    pub fn ticks_logged(&self) -> usize {
        self.log.len()
    }

    /// The finished log, one row of seven bit patterns per path tick.
    pub fn log(&self) -> &[[u32; 7]] {
        &self.log
    }

    /// Path ticks whose readback differed from the pose that was set.
    pub fn readback_mismatches(&self) -> &[usize] {
        &self.readback_mismatches
    }

    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// The log as JSON: the declared and replayed tick counts, the mismatches,
    /// and every row. Rows are raw `f32` bit patterns, so a comparison of two
    /// logs is a comparison of integers.
    pub fn log_json(&self) -> serde_json::Value {
        serde_json::json!({
            "schema": LOG_SCHEMA,
            "path_id": self.path.id,
            "ticks_declared": self.path.ticks_declared,
            "ticks_replayed": self.log.len(),
            "readback_mismatches": self.readback_mismatches,
            "tick_source": "FixedUpdate runs since the path started (Time<Fixed>)",
            "row_layout": ["pos_x", "pos_y", "pos_z", "rot_x", "rot_y", "rot_z", "rot_w"],
            "error": self.error,
            "rows": self.log,
        })
    }
}

/// One tick of the path player. The per-tick step: everything the player does
/// to the camera is in here, and a caller that owns the camera's `Transform`
/// (the system below, or a capture trigger that must act on the same tick)
/// calls it once per `FixedUpdate` run.
///
/// `ready` is whether the Space is loaded and the camera exists; it only
/// matters before tick 0.
pub fn step_path(player: &mut CameraPathPlayer, camera: &mut Transform, ready: bool) -> StepOutcome {
    match player.phase.clone() {
        Phase::Done => StepOutcome::Idle,
        Phase::Waiting { settle_left } => {
            if !ready {
                player.phase = Phase::Waiting { settle_left: SETTLE_TICKS };
                return StepOutcome::Waiting;
            }
            if settle_left > 0 {
                player.phase = Phase::Waiting { settle_left: settle_left - 1 };
                return StepOutcome::Waiting;
            }
            player.phase = Phase::Playing { next: 0 };
            step_path(player, camera, ready)
        }
        Phase::Playing { next } => {
            // The readback of the pose set on the previous tick.
            if next > 0 {
                let want = player.path.poses[next - 1].bits();
                let got = transform_bits(camera);
                if got != want {
                    player.readback_mismatches.push(next - 1);
                }
                player.log.push(got);
            }
            if next >= player.path.poses.len() {
                player.phase = Phase::Done;
                return StepOutcome::Finished;
            }
            let pose = player.path.poses[next];
            camera.translation = Vec3::new(pose.position[0], pose.position[1], pose.position[2]);
            camera.rotation = Quat::from_xyzw(pose.rotation[0], pose.rotation[1], pose.rotation[2], pose.rotation[3]);
            player.phase = Phase::Playing { next: next + 1 };
            StepOutcome::Set(next)
        }
    }
}

/// The path file for `name`: `EUSTRESS_CAMERA_PATH_FILE`, else
/// `spaces/harness/paths/<name>.json` under the working directory or under an
/// ancestor of the executable's folder.
fn locate_path_file(name: &str) -> Result<PathBuf, String> {
    if let Some(file) = std::env::var(ENV_FILE).ok().filter(|s| !s.is_empty()) {
        let file = PathBuf::from(file);
        return if file.is_file() { Ok(file) } else { Err(format!("{ENV_FILE} names {}, which is not a file", file.display())) };
    }
    let rel = |base: &Path| base.join("spaces").join("harness").join("paths").join(format!("{name}.json"));
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        bases.push(cwd.clone());
        bases.push(cwd.join("eustress"));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        for ancestor in dir.ancestors().take(6) {
            bases.push(ancestor.to_path_buf());
            bases.push(ancestor.join("eustress"));
        }
    }
    bases
        .iter()
        .map(|b| rel(b))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("no {name}.json found; set {ENV_FILE}"))
}

/// Write the log next to its final name through a temporary file, so a reader
/// never sees half of it.
fn write_log(player: &CameraPathPlayer, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = serde_json::to_string(&player.log_json()).map_err(|e| e.to_string())?;
    let tmp = dest.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, dest).map_err(|e| format!("rename to {}: {e}", dest.display()))
}

/// The `FixedUpdate` system: one [`step_path`] per tick, then the log and the
/// exit once the path is done.
fn camera_path_tick(
    mut player: ResMut<CameraPathPlayer>,
    loader: Option<Res<crate::space::file_loader::DeferredServiceLoader>>,
    mut cameras: Query<&mut Transform, With<AiCamera>>,
    mut exit: MessageWriter<AppExit>,
) {
    if !player.written {
        let loaded = loader.map(|l| l.priority_done && l.pending.is_empty()).unwrap_or(true);
        match cameras.single_mut() {
            Ok(mut camera) => {
                step_path(&mut player, &mut camera, loaded);
            }
            Err(_) => {
                // No camera. Before tick 0 that is a wait (the camera is put
                // back within a frame); mid-path it is a failed run, because a
                // despawned camera comes back at its starting pose.
                if matches!(player.phase, Phase::Playing { .. }) {
                    player.error = Some("the AI camera was lost mid-path".to_string());
                    player.phase = Phase::Done;
                } else if matches!(player.phase, Phase::Waiting { .. }) {
                    player.phase = Phase::Waiting { settle_left: SETTLE_TICKS };
                }
            }
        }
    }
    if player.is_done() && !player.written {
        player.written = true;
        if let Some(dest) = player.log_path.clone() {
            match write_log(&player, &dest) {
                Ok(()) => info!(
                    "camera path {}: {} ticks logged, {} readback mismatches -> {}",
                    player.path.id,
                    player.log.len(),
                    player.readback_mismatches.len(),
                    dest.display()
                ),
                Err(e) => error!("camera path {}: log not written: {e}", player.path.id),
            }
        } else if let Some(e) = &player.error {
            error!("camera path {}: {e}", player.path.id);
        }
        if player.exit_when_done {
            exit.write(AppExit::Success);
        }
    }
}

/// Registers the path player. Dormant unless `EUSTRESS_CAMERA_PATH` is set.
pub struct CameraPathPlugin;

impl Plugin for CameraPathPlugin {
    fn build(&self, app: &mut App) {
        let Some(player) = CameraPathPlayer::from_env() else {
            return;
        };
        app.insert_resource(player).add_systems(FixedUpdate, camera_path_tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LoadedPath {
        let poses = (0..5)
            .map(|i| PathPose {
                position: [i as f32 * 0.5, 2.0, 10.0 - i as f32],
                rotation: [0.0, (i as f32) * 0.1, 0.0, 0.995],
            })
            .collect::<Vec<_>>();
        LoadedPath { id: "CP-T".to_string(), ticks_declared: poses.len(), poses }
    }

    /// Run a whole path against a plain `Transform`, as the system would.
    fn replay(path: &LoadedPath, perturb_at: Option<usize>) -> CameraPathPlayer {
        let mut player = CameraPathPlayer::new(path.clone());
        let mut camera = Transform::default();
        let mut guard = 0;
        // Waiting for the Space, then the settle.
        assert_eq!(step_path(&mut player, &mut camera, false), StepOutcome::Waiting);
        while !player.is_done() {
            guard += 1;
            assert!(guard < 10_000, "the player never finished");
            let outcome = step_path(&mut player, &mut camera, true);
            if let (StepOutcome::Set(i), Some(at)) = (outcome, perturb_at) {
                if i == at {
                    camera.translation.x += f32::EPSILON * 8.0;
                }
            }
        }
        player
    }

    #[test]
    fn a_path_replays_bit_for_bit_and_logs_one_row_per_tick() {
        let path = sample();
        let a = replay(&path, None);
        let b = replay(&path, None);
        assert_eq!(a.log().len(), path.ticks_declared, "one row per declared tick");
        assert_eq!(a.log(), b.log(), "two replays are bit-identical");
        assert!(a.readback_mismatches().is_empty());
        for (row, pose) in a.log().iter().zip(&path.poses) {
            assert_eq!(*row, pose.bits(), "the readback is the pose that was set, bit for bit");
        }
    }

    /// The mismatch check can fail: a camera moved by one part in 10^6 between
    /// the set and the readback is recorded, and the log is not corrected.
    #[test]
    fn a_readback_that_differs_is_recorded_not_hidden() {
        let path = sample();
        let player = replay(&path, Some(2));
        assert_eq!(player.readback_mismatches(), &[2]);
        assert_ne!(player.log()[2], path.poses[2].bits());
        assert_eq!(player.log().len(), path.ticks_declared);
    }

    /// Nothing happens until the Space is ready, and a Space that stops being
    /// ready restarts the settle.
    #[test]
    fn the_path_waits_for_the_space_and_restarts_the_settle() {
        let mut player = CameraPathPlayer::new(sample());
        let mut camera = Transform::default();
        for _ in 0..(SETTLE_TICKS / 2) {
            assert_eq!(step_path(&mut player, &mut camera, true), StepOutcome::Waiting);
        }
        assert_eq!(step_path(&mut player, &mut camera, false), StepOutcome::Waiting);
        for _ in 0..SETTLE_TICKS {
            assert_eq!(step_path(&mut player, &mut camera, true), StepOutcome::Waiting);
        }
        assert_eq!(step_path(&mut player, &mut camera, true), StepOutcome::Set(0));
    }

    #[test]
    fn a_path_file_parses_and_a_gap_is_refused() {
        let good = r#"{"schema":"eustress.camera_path/1","kind":"poses","id":"CP-T","ticks":2,
            "poses":[{"tick":0,"position":[1,2,3],"rotation":[0,0,0,1]},
                     {"tick":1,"position":[1.5,2,3],"rotation":[0,0,0,1]}]}"#;
        let path = parse_path(good).expect("parses");
        assert_eq!(path.ticks_declared, 2);
        assert_eq!(path.poses[1].position, [1.5, 2.0, 3.0]);
        let gap = good.replace("\"tick\":1", "\"tick\":2");
        assert!(parse_path(&gap).is_err(), "a pose that skips a tick is an error");
        let short = good.replace("\"ticks\":2", "\"ticks\":3");
        assert!(parse_path(&short).is_err(), "a declared count that is not the pose count is an error");
        let ui = good.replace("\"kind\":\"poses\"", "\"kind\":\"ui_sequences\"");
        assert!(parse_path(&ui).is_err(), "a UI sequence file carries no camera poses");
    }
}

//! # Agent control surface
//!
//! A loopback JSON-lines socket that lets an agent drive the running Client and
//! observe what happened — press keys, advance frames, read structured state,
//! capture a picture.
//!
//! ## Why this exists
//!
//! Four bugs in one session were invisible to log-based instrumentation: a
//! missing `gltf_animation` Cargo feature, a missing `AnimatedBy` component, a
//! ground probe that started flush with the capsule bottom, and a ground
//! collider built at half size. Every one lived in the seam between this code
//! and Bevy/Avian, and every one was found by a human describing the screen.
//! The logs were green throughout, because they measured only the code that
//! wrote them.
//!
//! So the observation payload here is deliberately built from *failure modes
//! already observed*, not from what is convenient to expose:
//!
//! | field | the bug it would have caught |
//! |---|---|
//! | `grounded`, `planar_speed` | ground probe never reporting contact |
//! | `clip_weights`, `clip_elapsed` | player advancing while nothing moves |
//! | `peak_bone_delta_deg` | clip playing but the pose frozen |
//! | `rig_bones_bound`, `rig_unresolved` | skeleton bound to the wrong names |
//! | `asset_errors` | `bundled://` unregistered, clips 404 |
//! | `frame` | everything the numbers cannot say |
//!
//! ## Protocol
//!
//! One JSON object per line, request and response, over TCP on
//! `EUSTRESS_AGENT_PORT` (loopback only; absent = disabled). Every request may
//! carry an `id` which is echoed back.
//!
//! ```json
//! {"cmd":"hold","keys":["W","ShiftLeft"]}
//! {"cmd":"release","keys":["W"]}
//! {"cmd":"step","frames":60,"keys":["W"],"capture":true}
//! {"cmd":"observe","capture":false}
//! {"cmd":"reset"}
//! ```
//!
//! `step` is the workhorse: hold `keys`, advance `frames`, then reply with the
//! observation. The reply is deferred until the frame budget is spent, so the
//! agent gets act→observe in one round trip.
//!
//! ## Safety
//!
//! Off unless `EUSTRESS_AGENT_PORT` is set, and bound to `127.0.0.1` only. It
//! injects into `ButtonInput<KeyCode>` *before* the avatar reads input, and
//! only for keys it is currently holding — a human at the keyboard is
//! unaffected for every other key.

use bevy::prelude::*;
use eustress_common::avatar::rig::{AvatarRig, HumanoidBone};
use eustress_common::avatar::spawn::{AvatarBody, AvatarLocomotion};
use eustress_common::avatar::SpawnedByAvatarRuntime;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

// ─────────────────────────────────────────────────────────────────────────────
// Wire types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentRequest {
    pub cmd: String,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub frames: Option<u32>,
    #[serde(default)]
    pub capture: bool,
    /// Mouse buttons to hold: "Left" | "Right" | "Middle".
    #[serde(default)]
    pub mouse: Vec<String>,
    /// Per-frame mouse delta to synthesise, for drag gestures like orbit.
    #[serde(default)]
    pub mouse_delta: Option<[f32; 2]>,
}

#[derive(Debug, Serialize, Default)]
pub struct Observation {
    pub id: Option<u64>,
    pub ok: bool,
    pub error: Option<String>,

    // ── Body ──
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw_deg: f32,
    pub grounded: bool,
    pub planar_speed: f32,
    pub speed_norm: f32,
    pub air_time: f32,

    // ── Animation ──
    /// idle / walk / run / jump, in that order.
    pub clip_weights: [f32; 4],
    pub clip_elapsed: f32,
    /// Walk clip's own elapsed time — proves the walk node is advancing, not
    /// merely weighted.
    pub walk_elapsed: f32,
    pub walk_speed_mult: f32,
    /// Blend-node weights, read from the GRAPH ASSET. `AnimationPlayer` cannot
    /// see these: a Blend node is not a playing animation, so querying it
    /// through the player silently returns nothing.
    pub ground_weight: f32,
    pub air_weight: f32,
    /// Largest bone rotation change over the step window, degrees. Zero here
    /// with non-zero weights means the pose is frozen.
    pub peak_bone_delta_deg: f32,
    pub peak_bone: String,

    // ── Rig ──
    pub rig_bones_bound: usize,
    pub rig_unresolved: Vec<String>,

    // ── Environment ──
    pub held_keys: Vec<String>,
    pub frames_advanced: u32,
    pub frame: Option<String>,
    /// Horizontal offset of the HIPS from the capsule axis, in the avatar's
    /// own frame: [right, forward] metres. Non-zero here means the body pivots
    /// around a point that is not its centre of mass.
    pub camera_yaw_deg: f32,
    pub camera_pitch_deg: f32,
    pub hips_offset_local: [f32; 2],
    /// Same for the mesh child's origin.
    pub mesh_offset_local: [f32; 2],
    /// True when a human is driving and the agent's holds are being ignored.
    /// Surfaced so an agent sees it has lost control instead of silently
    /// issuing inputs that do nothing.
    pub human_override: bool,

    // ── Climb ──
    //
    // The point of these is falsifiability. `climb_phase` alone would say the
    // mechanic ran; it would not say the hands went anywhere. `hand_error_m`
    // is the distance from the SOLVED hand to the ledge point it was told to
    // reach, so a broken solver reports a large number instead of a
    // reassuring state name.
    /// `none` / `hanging` / `mantling`.
    pub climb_phase: String,
    /// World point on the ledge the hands are gripping.
    pub grab_point: [f32; 3],
    /// Arm-chain IK blend weight, 0..1.
    pub hand_ik_weight: f32,
    /// Worst hand's distance from its ledge target, metres.
    pub hand_error_m: f32,
    /// Worst sole's distance from the wall face, metres.
    pub climb_foot_error_m: f32,
    /// Straight-line distance from each hand bone to `grab_point`, metres —
    /// measured off the bone transforms, independent of the solver's own
    /// bookkeeping.
    pub hand_to_ledge_m: [f32; 2],
}

// ─────────────────────────────────────────────────────────────────────────────
// Plugin
// ─────────────────────────────────────────────────────────────────────────────

struct Pending {
    req: AgentRequest,
    reply: Sender<Observation>,
    frames_left: u32,
    /// Bone rotations at the start of the step, for the pose-change signal.
    snapshot: Vec<(HumanoidBone, Quat)>,
    peak: f32,
    peak_bone: Option<HumanoidBone>,
}

/// Channel endpoints are `Send` but not `Sync`, and a Bevy `Resource` must be
/// both. They only ever get touched from Bevy systems (single-threaded with
/// respect to this resource), so one mutex to satisfy the bound is sufficient
/// and uncontended.
struct AgentComms {
    inbox: Receiver<(AgentRequest, Sender<Observation>)>,
    pending: Option<Pending>,
}

#[derive(Resource)]
pub struct AgentControl {
    comms: Mutex<AgentComms>,
    held: HashSet<KeyCode>,
    held_mouse: HashSet<MouseButton>,
    /// What was injected last frame, so it can be RELEASED when no longer
    /// held. `ButtonInput::press` persists until an explicit `release` — an
    /// injector that only ever presses leaves keys and buttons stuck down
    /// forever, which made the camera keep orbiting with no button held.
    injected_keys: HashSet<KeyCode>,
    injected_mouse: HashSet<MouseButton>,
    mouse_delta: Vec2,
    /// Wall-clock seconds until which a real human is considered in control.
    ///
    /// Refreshed by every genuine OS input event. While this is in the future,
    /// the agent stops injecting — a human at the keyboard should never have to
    /// fight a held key they did not press.
    human_until: f64,
    /// Set when a capture is requested; the capture system clears it.
    pub capture_request: Option<std::path::PathBuf>,
    capture_seq: u32,
}

pub struct AgentControlPlugin;

impl Plugin for AgentControlPlugin {
    fn build(&self, app: &mut App) {
        let Ok(port) = std::env::var("EUSTRESS_AGENT_PORT") else { return };
        let Ok(port) = port.parse::<u16>() else {
            error!("agent: EUSTRESS_AGENT_PORT is not a port number");
            return;
        };

        let (tx, rx) = channel::<(AgentRequest, Sender<Observation>)>();
        let tx = Arc::new(Mutex::new(tx));

        std::thread::Builder::new()
            .name("eustress-agent".into())
            .spawn(move || serve(port, tx))
            .ok();

        app.insert_resource(AgentControl {
            comms: Mutex::new(AgentComms { inbox: rx, pending: None }),
            held: HashSet::new(),
            held_mouse: HashSet::new(),
            injected_keys: HashSet::new(),
            injected_mouse: HashSet::new(),
            mouse_delta: Vec2::ZERO,
            human_until: 0.0,
            capture_request: None,
            capture_seq: 0,
        })
        // Injection must land before the avatar samples input.
        .add_systems(
            Update,
            (detect_human_input, drain_requests, inject_input)
                .chain()
                .before(eustress_common::avatar::AvatarSystems::Input),
        )
        // Observation is read after the pose is final for this frame.
        .add_systems(PostUpdate, complete_steps.after(TransformSystems::Propagate))
        .add_systems(PostUpdate, service_capture_requests);

        info!("🤖 agent control listening on 127.0.0.1:{port}");
    }
}

/// Turn a pending capture request into an actual screenshot.
///
/// Without this the agent's `capture: true` set a path that nothing ever wrote,
/// and the observation reported a filename for a file that did not exist — a
/// tool reporting success for work it never did.
fn service_capture_requests(mut commands: Commands, mut agent: ResMut<AgentControl>) {
    let Some(path) = agent.capture_request.take() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    commands
        .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
        .observe(bevy::render::view::screenshot::save_to_disk(path));
}

/// One connection at a time, sequential request/response. An agent loop is
/// inherently serial, and a single connection keeps ordering unambiguous.
fn serve(port: u16, tx: Arc<Mutex<Sender<(AgentRequest, Sender<Observation>)>>>) {
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            error!("agent: cannot bind 127.0.0.1:{port}: {e}");
            return;
        }
    };

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        handle_conn(stream, &tx);
    }
}

fn handle_conn(stream: TcpStream, tx: &Arc<Mutex<Sender<(AgentRequest, Sender<Observation>)>>>) {
    let mut out = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }

        let req: AgentRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let obs = Observation {
                    ok: false,
                    error: Some(format!("bad request: {e}")),
                    ..Default::default()
                };
                let _ = writeln!(out, "{}", serde_json::to_string(&obs).unwrap_or_default());
                continue;
            }
        };

        let (rtx, rrx) = channel::<Observation>();
        if tx.lock().map(|t| t.send((req, rtx)).is_err()).unwrap_or(true) {
            break;
        }

        // The app replies when the step's frame budget is spent.
        match rrx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(obs) => {
                let _ = writeln!(out, "{}", serde_json::to_string(&obs).unwrap_or_default());
            }
            Err(_) => {
                let obs = Observation {
                    ok: false,
                    error: Some("timed out waiting for the app".into()),
                    ..Default::default()
                };
                let _ = writeln!(out, "{}", serde_json::to_string(&obs).unwrap_or_default());
            }
        }
    }
}

fn key_from_str(s: &str) -> Option<KeyCode> {
    Some(match s {
        "W" => KeyCode::KeyW,
        "A" => KeyCode::KeyA,
        "S" => KeyCode::KeyS,
        "D" => KeyCode::KeyD,
        "Space" => KeyCode::Space,
        "ShiftLeft" | "Shift" => KeyCode::ShiftLeft,
        "ControlLeft" | "Ctrl" => KeyCode::ControlLeft,
        "Escape" => KeyCode::Escape,
        "F9" => KeyCode::F9,
        _ => return None,
    })
}

fn mouse_from_str(s: &str) -> Option<MouseButton> {
    Some(match s {
        "Left" => MouseButton::Left,
        "Right" => MouseButton::Right,
        "Middle" => MouseButton::Middle,
        _ => return None,
    })
}

fn key_to_str(k: KeyCode) -> String {
    match k {
        KeyCode::KeyW => "W",
        KeyCode::KeyA => "A",
        KeyCode::KeyS => "S",
        KeyCode::KeyD => "D",
        KeyCode::Space => "Space",
        KeyCode::ShiftLeft => "ShiftLeft",
        KeyCode::ControlLeft => "ControlLeft",
        KeyCode::Escape => "Escape",
        _ => "?",
    }
    .to_string()
}

fn drain_requests(
    mut agent: ResMut<AgentControl>,
    mut spawn: MessageWriter<eustress_common::avatar::SpawnAvatar>,
    mut despawn: MessageWriter<eustress_common::avatar::DespawnAllAvatars>,
    rigs: Query<&AvatarRig, With<SpawnedByAvatarRuntime>>,
    bones: Query<&Transform, Without<SpawnedByAvatarRuntime>>,
) {
    // A step in flight owns the socket until it completes.
    let (req, reply) = {
        let Ok(mut comms) = agent.comms.lock() else { return };
        if comms.pending.is_some() {
            return;
        }
        let Ok(pair) = comms.inbox.try_recv() else { return };
        pair
    };

    match req.cmd.as_str() {
        "hold" => {
            for k in req.keys.iter().filter_map(|s| key_from_str(s)) {
                agent.held.insert(k);
            }
            for b in req.mouse.iter().filter_map(|s| mouse_from_str(s)) {
                agent.held_mouse.insert(b);
            }
            let _ = reply.send(Observation { id: req.id, ok: true, ..Default::default() });
        }
        "release" => {
            agent.held_mouse.clear();
            agent.mouse_delta = Vec2::ZERO;
            if req.keys.is_empty() {
                agent.held.clear();
            } else {
                for k in req.keys.iter().filter_map(|s| key_from_str(s)) {
                    agent.held.remove(&k);
                }
            }
            let _ = reply.send(Observation { id: req.id, ok: true, ..Default::default() });
        }
        "reset" => {
            despawn.write(eustress_common::avatar::DespawnAllAvatars);
            spawn.write(eustress_common::avatar::SpawnAvatar::new(
                eustress_common::avatar::AvatarDescriptor::default(),
                Vec3::new(0.0, 2.0, 8.0),
            ));
            agent.held.clear();
            let _ = reply.send(Observation { id: req.id, ok: true, ..Default::default() });
        }
        "step" | "observe" => {
            for k in req.keys.iter().filter_map(|s| key_from_str(s)) {
                agent.held.insert(k);
            }
            for b in req.mouse.iter().filter_map(|s| mouse_from_str(s)) {
                agent.held_mouse.insert(b);
            }
            agent.mouse_delta = req.mouse_delta.map(Vec2::from).unwrap_or(Vec2::ZERO);
            // Snapshot the pose so the reply can report how much it changed.
            let snapshot = rigs
                .iter()
                .next()
                .map(|rig| {
                    rig.bones
                        .iter()
                        .filter_map(|(b, e)| bones.get(*e).ok().map(|t| (*b, t.rotation)))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let frames = if req.cmd == "observe" { 1 } else { req.frames.unwrap_or(30).min(600) };

            if req.capture {
                let dir = std::env::var("EUSTRESS_CAPTURE_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| std::path::PathBuf::from("capture"));
                let seq = agent.capture_seq;
                agent.capture_seq += 1;
                agent.capture_request = Some(dir.join(format!("agent_{seq:04}.png")));
            }

            if let Ok(mut comms) = agent.comms.lock() {
                comms.pending = Some(Pending {
                    req,
                    reply,
                    frames_left: frames,
                    snapshot,
                    peak: 0.0,
                    peak_bone: None,
                });
            }
        }
        other => {
            let _ = reply.send(Observation {
                id: req.id,
                ok: false,
                error: Some(format!("unknown cmd: {other}")),
                ..Default::default()
            });
        }
    }
}

/// How long a human keeps control after their last input.
const HUMAN_TAKEOVER_SECS: f64 = 2.0;

/// Hand control back to the human the moment they touch anything.
///
/// Detection relies on a property of the injection path: `ButtonInput::press`
/// mutates state directly and emits NO `KeyboardInput` message. Those messages
/// only ever originate from winit — so any message here is unambiguously a real
/// person, with no need to tag or filter synthetic input.
///
/// Agent holds are dropped immediately on takeover rather than left set, so a
/// key the agent was holding cannot stick down under the human's hands.
fn detect_human_input(
    time: Res<Time>,
    mut agent: ResMut<AgentControl>,
    mut keyboard: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut mouse_btn: MessageReader<bevy::input::mouse::MouseButtonInput>,
    mut mouse_move: MessageReader<bevy::input::mouse::MouseMotion>,
) {
    // Keyboard and mouse BUTTONS are unambiguous intent. Raw mouse motion is
    // not: the cursor drifting across the window, or OS-level jitter, would
    // otherwise steal control every couple of seconds. Camera look already
    // requires holding right-mouse, so buttons cover the real look gesture —
    // motion only counts once it is decisively a deliberate movement.
    const DELIBERATE_MOUSE_PX: f32 = 8.0;
    let moved: f32 = mouse_move.read().map(|m| m.delta.length()).sum();

    let human_acted = keyboard.read().next().is_some()
        || mouse_btn.read().next().is_some()
        || moved > DELIBERATE_MOUSE_PX;

    if !human_acted {
        return;
    }

    let now = time.elapsed_secs_f64();
    let was_agent_driving = !agent.held.is_empty() && agent.human_until <= now;

    agent.human_until = now + HUMAN_TAKEOVER_SECS;

    if was_agent_driving {
        agent.held.clear();
        info!("🤖→🧑 human input detected — agent holds released");
    }
}

/// Write the held keys into Bevy's input state, before the avatar reads it.
///
/// Yields to the human unless a *sequenced* step is mid-flight: an agent that
/// asked for 90 frames of W is running a deliberate scripted sequence and
/// should be allowed to finish it. Ad-hoc holds lose immediately.
fn inject_input(
    time: Res<Time>,
    mut agent: ResMut<AgentControl>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    mut motion: MessageWriter<bevy::input::mouse::MouseMotion>,
) {
    let human_in_control = time.elapsed_secs_f64() < agent.human_until;
    let sequenced = agent
        .comms
        .lock()
        .map(|c| c.pending.is_some())
        .unwrap_or(false);

    // Always release what we injected but no longer hold — including when
    // handing control back to a human, so nothing stays stuck down.
    let want_keys: HashSet<KeyCode> =
        if human_in_control && !sequenced { HashSet::new() } else { agent.held.clone() };
    let want_mouse: HashSet<MouseButton> =
        if human_in_control && !sequenced { HashSet::new() } else { agent.held_mouse.clone() };

    for k in agent.injected_keys.clone() {
        if !want_keys.contains(&k) {
            keys.release(k);
        }
    }
    for b in agent.injected_mouse.clone() {
        if !want_mouse.contains(&b) {
            buttons.release(b);
        }
    }

    for k in want_keys.iter() {
        if !keys.pressed(*k) {
            keys.press(*k);
        }
    }
    for b in want_mouse.iter() {
        if !buttons.pressed(*b) {
            buttons.press(*b);
        }
    }
    agent.injected_keys = want_keys;
    agent.injected_mouse = want_mouse;

    if human_in_control && !sequenced {
        return;
    }
    // Synthesised drag. Emitted as a real `MouseMotion` message so the camera
    // consumes it through exactly the same path a physical mouse would.
    if agent.mouse_delta != Vec2::ZERO {
        motion.write(bevy::input::mouse::MouseMotion { delta: agent.mouse_delta });
    }
}

#[allow(clippy::too_many_arguments)]
fn complete_steps(
    time: Res<Time>,
    mut agent: ResMut<AgentControl>,
    bodies: Query<
        (&GlobalTransform, &AvatarLocomotion, &AvatarBody, &AvatarRig),
        With<SpawnedByAvatarRuntime>,
    >,
    bones: Query<&Transform, Without<SpawnedByAvatarRuntime>>,
    graphs: Query<&eustress_common::avatar::anim::AvatarMotionGraph>,
    players: Query<&bevy::animation::AnimationPlayer>,
    velocities: Query<&avian3d::prelude::LinearVelocity, With<SpawnedByAvatarRuntime>>,
    graph_assets: Res<Assets<bevy::animation::graph::AnimationGraph>>,
    globals: Query<&GlobalTransform>,
    mesh_children: Query<&eustress_common::avatar::spawn::AvatarMeshChild>,
    cameras: Query<&eustress_common::avatar::control::AvatarCamera>,
    climbs: Query<
        (
            &eustress_common::avatar::climb::AvatarClimb,
            &eustress_common::avatar::ik::AvatarFootIk,
        ),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    let finished = {
        let Ok(mut comms) = agent.comms.lock() else { return };
        let Some(pending) = comms.pending.as_mut() else { return };

        // Track the largest pose change over the window, not just the
        // endpoints — endpoint sampling is phase-locked against a looping clip
        // and understates the amplitude.
        if let Some((_, _, _, rig)) = bodies.iter().next() {
            let mut frame_peak = (0.0_f32, None);
            for (b, start) in pending.snapshot.iter() {
                let Some(e) = rig.bone(*b) else { continue };
                let Ok(now) = bones.get(e) else { continue };
                let d = start.angle_between(now.rotation).to_degrees();
                if d > frame_peak.0 {
                    frame_peak = (d, Some(*b));
                }
            }
            if frame_peak.0 > pending.peak {
                pending.peak = frame_peak.0;
                pending.peak_bone = frame_peak.1;
            }
        }

        pending.frames_left = pending.frames_left.saturating_sub(1);
        if pending.frames_left > 0 {
            return;
        }
        comms.pending.take()
    };

    let Some(Pending { req, reply, peak, peak_bone, .. }) = finished else { return };

    let mut obs = Observation {
        id: req.id,
        ok: true,
        peak_bone_delta_deg: peak,
        peak_bone: peak_bone.map(|b| format!("{b:?}")).unwrap_or_default(),
        held_keys: agent.held.iter().map(|k| key_to_str(*k)).collect(),
        frames_advanced: req.frames.unwrap_or(30),
        frame: agent.capture_request.as_ref().map(|p| p.to_string_lossy().to_string()),
        human_override: time.elapsed_secs_f64() < agent.human_until,
        ..Default::default()
    };

    if let Some((gt, loco, body, rig)) = bodies.iter().next() {
        let p = gt.translation();
        obs.position = [p.x, p.y, p.z];
        obs.yaw_deg = gt.rotation().to_euler(EulerRot::YXZ).0.to_degrees();
        obs.grounded = loco.grounded;
        obs.planar_speed = loco.planar_speed;
        obs.speed_norm = loco.speed_norm;
        obs.air_time = loco.air_time;
        obs.rig_bones_bound = rig.bones.len();
        obs.rig_unresolved = rig.unresolved.iter().map(|b| format!("{b:?}")).collect();
        let _ = body;
    } else {
        obs.ok = false;
        obs.error = Some("no avatar spawned".into());
    }

    // Where is the body actually centred relative to the axis it spins about?
    if let Some((gt, _, _, rig)) = bodies.iter().next() {
        let root_pos = gt.translation();
        let inv = gt.rotation().inverse();
        if let Some(h) = rig.bone(HumanoidBone::Hips) {
            if let Ok(hg) = globals.get(h) {
                let d = inv * (hg.translation() - root_pos);
                obs.hips_offset_local = [d.x, -d.z];
            }
        }
        if let Some(mc) = mesh_children.iter().next() {
            if let Ok(mg) = globals.get(mc.0) {
                let d = inv * (mg.translation() - root_pos);
                obs.mesh_offset_local = [d.x, -d.z];
            }
        }
    }

    if let Some(v) = velocities.iter().next() {
        obs.velocity = [v.0.x, v.0.y, v.0.z];
    }

    if let Some(cam) = cameras.iter().next() {
        obs.camera_yaw_deg = cam.yaw.to_degrees();
        obs.camera_pitch_deg = cam.pitch.to_degrees();
    }

    if let Some((climb, ik)) = climbs.iter().next() {
        use eustress_common::avatar::climb::ClimbPhase;
        obs.climb_phase = match climb.phase {
            ClimbPhase::None => "none",
            ClimbPhase::Hanging => "hanging",
            ClimbPhase::Shimmy => "shimmy",
            ClimbPhase::Transfer => "transfer",
            ClimbPhase::Mantling => "mantling",
            ClimbPhase::Vaulting => "vaulting",
            ClimbPhase::Lowering => "lowering",
        }
        .into();
        obs.grab_point = climb.grab_point().into();
        obs.hand_ik_weight = ik.hand_weight;
        obs.hand_error_m = ik.hand_error_m;
        obs.climb_foot_error_m = ik.climb_foot_error_m;

        // Measured independently of the solver: read the hand bones' own
        // world transforms. If the solver reports success while these stay
        // metres away, the solver is lying and this catches it.
        if let Some((_, _, _, rig)) = bodies.iter().next() {
            for (slot, bone) in
                [(0usize, HumanoidBone::LeftHand), (1, HumanoidBone::RightHand)]
            {
                if let Some(e) = rig.bone(bone) {
                    if let Ok(g) = globals.get(e) {
                        obs.hand_to_ledge_m[slot] =
                            (g.translation() - climb.grab_point()).length();
                    }
                }
            }
        }
    }

    if let Some(g) = graphs.iter().next() {
        if let Ok(p) = players.get(g.player) {
            // Player weights are pinned to 1.0 and act as an on/off gate; the
            // real blend lives in the graph asset, so report that.
            let w = |n| p.animation(n).map(|a| a.weight()).unwrap_or(0.0);
            let _ = w(g.idle);
            obs.clip_elapsed = p.animation(g.idle).map(|a| a.elapsed()).unwrap_or(0.0);
            obs.walk_elapsed = p.animation(g.walk).map(|a| a.elapsed()).unwrap_or(-1.0);
            obs.walk_speed_mult = p.animation(g.walk).map(|a| a.speed()).unwrap_or(-1.0);
        }
        if let Some(graph) = graph_assets.get(&g.handle) {
            let gw = |n| graph.get(n).map(|x| x.weight).unwrap_or(-1.0);
            obs.clip_weights = [gw(g.idle), gw(g.walk), gw(g.run), gw(g.jump)];
            obs.ground_weight = gw(g.ground);
            obs.air_weight = gw(g.air);
        }
    }

    let _ = reply.send(obs);
}

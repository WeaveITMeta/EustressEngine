//! # Input and camera — one implementation, both shells
//!
//! ## What was wrong before
//!
//! Input was genuinely shared (`SharedCharacterPlugin::character_movement_input`),
//! but the **camera was not**. Play Mode called the shared
//! `spawn_play_mode_camera` (`TonyMcMapface`, distance 8.0, pitch 0.3 rad,
//! order 10, and — critically — *no `Projection`*, so it rendered at Bevy's
//! default 45° FOV). The Client hand-rolled its own inline
//! (`client/plugins/player_plugin.rs:893-912`): `Reinhard`, explicit 70° FOV,
//! distance 5.0, pitch -15°, no order. Same follow logic afterwards, visibly
//! different framing and colour grading.
//!
//! `camera_follow` also hardcoded a 1.5 m height offset for every body, so the
//! Height slider would have moved the avatar's eyes without moving the camera.
//! Here the offset is [`BodyMetrics::eye_height`].
//!
//! ## Escape
//!
//! Escape was bound in three places with opposite effects and no ordering
//! constraint: the shared `toggle_cursor_lock` flipped cursor grab,
//! `play_mode_shortcuts` requested Stop, and the Client's pause menu wanted to
//! open. Whichever ran last won, and the schedule made no promise. Here
//! Escape has exactly one owner and the host decides its meaning through
//! [`HostSeams::escape_action`].

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use super::spawn::{AvatarBody, AvatarIntent};
use super::{
    AvatarControl, AvatarHostConfig, AvatarSystems, EscapeAction, LocalAvatar,
    SpawnedByAvatarRuntime,
};

/// The avatar's third-person/first-person camera. Spawned by the runtime, one
/// definition for both shells.
#[derive(Component, Debug, Clone)]
pub struct AvatarCamera {
    pub target: Entity,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub sensitivity: f32,
    pub zoom_speed: f32,
    pub pitch_min: f32,
    pub pitch_max: f32,
    pub first_person: bool,
}

impl AvatarCamera {
    pub fn new(target: Entity) -> Self {
        Self {
            target,
            yaw: 0.0,
            pitch: 0.30,
            distance: 5.0,
            min_distance: 0.0,
            max_distance: 12.0,
            sensitivity: 0.003,
            zoom_speed: 0.6,
            pitch_min: -1.35,
            pitch_max: 1.35,
            first_person: false,
        }
    }
}

/// Field of view, shared so the two shells cannot render at different
/// framings. 70° matches what the Client already used; Play Mode was silently
/// on Bevy's 45° default because it never inserted a `Projection`.
pub const AVATAR_FOV_DEG: f32 = 70.0;

/// Whether gameplay input is currently allowed. Studio sets this false when
/// the 3D viewport does not have focus, so typing in a panel does not walk the
/// character. The Client leaves it true.
#[derive(Resource, Debug)]
pub struct AvatarInputEnabled(pub bool);

impl Default for AvatarInputEnabled {
    fn default() -> Self {
        // Default true: the Client never gates, and Studio explicitly sets
        // false when its viewport loses focus. Defaulting false would make a
        // host that forgets to set it silently unable to move.
        Self(true)
    }
}

/// Raised when the sole Escape owner fires. The host decides what it means.
#[derive(Message, Debug, Clone, Copy)]
pub struct AvatarEscapePressed(pub EscapeAction);

pub(crate) struct AvatarControlPlugin;

impl Plugin for AvatarControlPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AvatarInputEnabled>()
            .add_message::<AvatarEscapePressed>()
            .add_systems(Update, spawn_camera_for_new_avatars.in_set(AvatarSystems::Lifecycle))
            .add_systems(
                Update,
                (
                    sample_movement_input,
                    camera_look,
                    camera_orbit_keys,
                    camera_zoom,
                    escape_owner,
                )
                    .in_set(AvatarSystems::Input),
            )
            // Follow runs after locomotion has moved the body, so the camera
            // never trails by a frame.
            .add_systems(PostUpdate, camera_follow.before(TransformSystems::Propagate))
            // Head hiding must land AFTER the clips write bone scale, or
            // `animate_targets` overwrites it the same frame.
            .add_systems(PostUpdate, hide_head_in_first_person.in_set(AvatarSystems::PostAnim));
    }
}

/// Every avatar the local player controls gets exactly one camera.
fn spawn_camera_for_new_avatars(
    mut commands: Commands,
    cfg: Res<AvatarHostConfig>,
    new_avatars: Query<(Entity, &Transform), (Added<LocalAvatar>, With<SpawnedByAvatarRuntime>)>,
    // Second line of defence against the leak fixed in `handle_despawn_all`:
    // anything still pointing at a body that no longer exists is an orphan and
    // is cleared before a replacement is made, so a camera can never
    // accumulate even if some other path despawns an avatar directly.
    existing: Query<(Entity, &AvatarCamera)>,
    bodies: Query<(), With<SpawnedByAvatarRuntime>>,
) {
    if !new_avatars.is_empty() {
        for (e, cam) in existing.iter() {
            if bodies.get(cam.target).is_err() {
                // `try_despawn`: `handle_despawn_all` may have queued the same
                // entity this frame, and a second application of a real
                // `despawn` panics the schedule.
                commands.entity(e).try_despawn();
            }
        }
    }

    for (target, tf) in new_avatars.iter() {
        let mut cam = AvatarCamera::new(target);
        cam.yaw = tf.rotation.to_euler(EulerRot::YXZ).0;

        commands.spawn((
            Camera3d::default(),
            Camera { order: cfg.seams.camera_order, ..default() },
            // Explicit projection: without this Play Mode silently used
            // Bevy's 45° default while the Client used 70°.
            Projection::Perspective(PerspectiveProjection {
                fov: AVATAR_FOV_DEG.to_radians(),
                ..default()
            }),
            // Reinhard needs no LUT textures. TonyMcMapface requires the
            // `tonemapping_luts` feature, which the Client does not enable —
            // picking it here would render magenta in one shell only.
            bevy::core_pipeline::tonemapping::Tonemapping::Reinhard,
            // Spawn at the avatar, not the world origin. Play Mode's camera
            // used to fly in from (0,0,0) on every Play press.
            Transform::from_translation(tf.translation + Vec3::new(0.0, 2.0, 5.0))
                .looking_at(tf.translation, Vec3::Y),
            cam,
            Name::new("AvatarCamera"),
        ));
    }
}

fn sample_movement_input(
    keys: Res<ButtonInput<KeyCode>>,
    enabled: Res<AvatarInputEnabled>,
    cameras: Query<&AvatarCamera>,
    mut q: Query<(&mut AvatarIntent, &AvatarBody), (With<LocalAvatar>, With<SpawnedByAvatarRuntime>)>,
) {
    for (mut intent, body) in q.iter_mut() {
        if body.control != AvatarControl::LocalPlayer || !enabled.0 {
            intent.direction = Vec3::ZERO;
            intent.sprint = false;
            intent.crouch = false;
            continue;
        }

        let yaw = cameras.iter().next().map(|c| c.yaw).unwrap_or(0.0);

        let mut local = Vec3::ZERO;
        if keys.pressed(KeyCode::KeyW) {
            local.z -= 1.0;
        }
        if keys.pressed(KeyCode::KeyS) {
            local.z += 1.0;
        }
        if keys.pressed(KeyCode::KeyA) {
            local.x -= 1.0;
        }
        if keys.pressed(KeyCode::KeyD) {
            local.x += 1.0;
        }

        if local.length_squared() > 0.0 {
            local = local.normalize();
        }

        // Camera-relative on the ground plane.
        let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
        intent.direction = forward * -local.z + right * local.x;

        intent.sprint = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        intent.crouch = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);

        // Latched, not level-triggered: the controller clears it on consume so
        // one press cannot produce two jumps.
        if keys.just_pressed(KeyCode::Space) {
            intent.jump_pressed = true;
        }
    }
}

fn camera_look(
    mut motion: MessageReader<MouseMotion>,
    mouse: Res<ButtonInput<MouseButton>>,
    enabled: Res<AvatarInputEnabled>,
    mut cameras: Query<&mut AvatarCamera>,
    mut warned: Local<bool>,
) {
    if !enabled.0 {
        motion.clear();
        return;
    }

    // Deliberately NOT `single_mut()`. That returns `Err` on two-or-more and
    // the old early return then dropped every mouse delta on the floor, so a
    // single leaked camera disabled looking entirely with nothing in the log
    // to say why. Driving all of them keeps the control alive while the count
    // is reported once.
    let n = cameras.iter().count();
    if n != 1 && !*warned {
        *warned = true;
        warn!("avatar: expected 1 AvatarCamera, found {n} — orbit will drive all of them");
    }

    let deltas: Vec<Vec2> = motion.read().map(|e| e.delta).collect();
    if deltas.is_empty() {
        return;
    }

    for mut cam in cameras.iter_mut() {
        // First person always looks; third person orbits on right-drag.
        if !(cam.first_person || mouse.pressed(MouseButton::Right)) {
            continue;
        }
        for d in &deltas {
            cam.yaw -= d.x * cam.sensitivity;
            cam.pitch =
                (cam.pitch + d.y * cam.sensitivity).clamp(cam.pitch_min, cam.pitch_max);
        }
    }
}

/// Radians per second the arrow keys orbit at. ~110°/s — a full turn in a
/// little over three seconds, fast enough to be useful and slow enough to aim.
const ARROW_ORBIT_RATE: f32 = 1.9;

/// Arrow-key camera orbit.
///
/// Drives the same `yaw`/`pitch` the mouse does, so the two can never disagree
/// about where the camera is — but on a *rate* rather than a delta, since a
/// held key has no magnitude.
///
/// Signs follow the mouse rather than the maths: the mouse uses `yaw -= dx`,
/// so Right decrements yaw and the view swings right. Pitch raises the camera
/// (`camera_follow` puts the orbit offset at `pitch.sin() * distance`), so Up
/// lifts the camera above the character.
fn camera_orbit_keys(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    enabled: Res<AvatarInputEnabled>,
    mut cameras: Query<&mut AvatarCamera>,
) {
    if !enabled.0 {
        return;
    }

    let mut yaw = 0.0_f32;
    let mut pitch = 0.0_f32;
    if keys.pressed(KeyCode::ArrowRight) {
        yaw -= 1.0;
    }
    if keys.pressed(KeyCode::ArrowLeft) {
        yaw += 1.0;
    }
    if keys.pressed(KeyCode::ArrowUp) {
        pitch += 1.0;
    }
    if keys.pressed(KeyCode::ArrowDown) {
        pitch -= 1.0;
    }
    if yaw == 0.0 && pitch == 0.0 {
        return;
    }

    let step = ARROW_ORBIT_RATE * time.delta_secs();
    for mut cam in cameras.iter_mut() {
        cam.yaw += yaw * step;
        cam.pitch = (cam.pitch + pitch * step).clamp(cam.pitch_min, cam.pitch_max);
    }
}

fn camera_zoom(
    mut wheel: MessageReader<MouseWheel>,
    enabled: Res<AvatarInputEnabled>,
    mut cameras: Query<&mut AvatarCamera>,
    mut cursor: Query<&mut CursorOptions, With<Window>>,
) {
    let Ok(mut cam) = cameras.single_mut() else {
        wheel.clear();
        return;
    };
    if !enabled.0 {
        wheel.clear();
        return;
    }

    let mut delta = 0.0;
    for e in wheel.read() {
        delta += e.y;
    }
    if delta == 0.0 {
        return;
    }

    cam.distance = (cam.distance - delta * cam.zoom_speed).clamp(cam.min_distance, cam.max_distance);

    let was_first = cam.first_person;
    cam.first_person = cam.distance <= cam.min_distance + 1e-3;

    if cam.first_person != was_first {
        if let Ok(mut c) = cursor.single_mut() {
            if cam.first_person {
                c.grab_mode = CursorGrabMode::Locked;
                c.visible = false;
            } else {
                c.grab_mode = CursorGrabMode::None;
                c.visible = true;
            }
        }
    }
}

/// The ONE Escape handler. Emits an intent; the host acts on it.
fn escape_owner(
    keys: Res<ButtonInput<KeyCode>>,
    cfg: Res<AvatarHostConfig>,
    mut out: MessageWriter<AvatarEscapePressed>,
    mut cameras: Query<&mut AvatarCamera>,
    mut cursor: Query<&mut CursorOptions, With<Window>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }

    // In first person, Escape releases the cursor first — it does not also
    // stop play or open a menu in the same press.
    if let Ok(mut cam) = cameras.single_mut() {
        if cam.first_person {
            cam.first_person = false;
            cam.distance = cam.min_distance + 1.5;
            if let Ok(mut c) = cursor.single_mut() {
                c.grab_mode = CursorGrabMode::None;
                c.visible = true;
            }
            return;
        }
    }

    out.write(AvatarEscapePressed(cfg.seams.escape_action));
}

/// How far ahead of the eyes the first-person camera sits, in metres.
pub(crate) const FIRST_PERSON_FORWARD_M: f32 = 0.12;

/// The bones collapsed while in first person.
///
/// Named rather than written inline so the parity contract can compare it
/// between shells. Hiding the head but not the neck is the exact drift this
/// list exists to make visible.
pub(crate) const FIRST_PERSON_HIDDEN_BONES: [super::rig::HumanoidBone; 3] = [
    super::rig::HumanoidBone::Neck,
    super::rig::HumanoidBone::Head,
    super::rig::HumanoidBone::HeadTop,
];

fn camera_follow(
    time: Res<Time>,
    bodies: Query<(&Transform, &AvatarBody), (With<SpawnedByAvatarRuntime>, Without<AvatarCamera>)>,
    mut cameras: Query<(&mut Transform, &AvatarCamera)>,
) {
    let dt = time.delta_secs();
    for (mut cam_tf, cam) in cameras.iter_mut() {
        let Ok((body_tf, body)) = bodies.get(cam.target) else { continue };

        // Eye height comes from the descriptor-derived metrics, so the Height
        // slider moves the camera with the body. The old shared follow
        // hardcoded 1.5 for every avatar.
        let eye = body_tf.translation - Vec3::Y * body.metrics.capsule_half_extent()
            + Vec3::Y * body.metrics.eye_height;

        if cam.first_person {
            // Slightly AHEAD of the eyes, not exactly on them.
            //
            // Sitting on the eye point leaves the throat and upper chest in
            // front of the near plane the moment the view pitches down. Real
            // first-person cameras are pushed forward for this reason. Kept
            // small, and horizontal only, so looking down does not slide the
            // viewpoint out of the head.
            let facing = Quat::from_rotation_y(cam.yaw);
            cam_tf.translation = eye + facing * (Vec3::NEG_Z * FIRST_PERSON_FORWARD_M);
            cam_tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, -cam.pitch, 0.0);
            continue;
        }

        let offset = Vec3::new(
            cam.yaw.sin() * cam.pitch.cos(),
            cam.pitch.sin(),
            cam.yaw.cos() * cam.pitch.cos(),
        ) * cam.distance;

        let desired = eye + offset;
        // Framerate-independent smoothing.
        let alpha = 1.0 - (-12.0 * dt).exp();
        cam_tf.translation = cam_tf.translation.lerp(desired, alpha);
        cam_tf.look_at(eye, Vec3::Y);
    }
}

/// Collapse the head in first person.
///
/// The body is ONE skinned mesh, so `Visibility` cannot hide a part of it —
/// hiding the head entity hides nothing, because the head's vertices are
/// driven by joint matrices on the shared mesh. Scaling the head joint to
/// nearly zero collapses those vertices to a point instead, which is the
/// standard trick and costs nothing.
///
/// Runs in `PostAnim`: the shipped clips animate `scale` channels, so writing
/// this any earlier would be overwritten by `animate_targets` on the same
/// frame.
fn hide_head_in_first_person(
    cameras: Query<&AvatarCamera>,
    rigs: Query<&super::rig::AvatarRig, With<SpawnedByAvatarRuntime>>,
    mut bones: Query<&mut Transform, Without<SpawnedByAvatarRuntime>>,
    mut was_first_person: Local<bool>,
) {
    use super::rig::HumanoidBone;

    let first_person = cameras.iter().next().map(|c| c.first_person).unwrap_or(false);

    // Restoring on the FALLING EDGE rather than every third-person frame: the
    // shrink had no `else` at all, so zooming back out left the head at 1e-4
    // forever and the character was permanently decapitated. Writing
    // `Vec3::ONE` unconditionally would instead stomp whatever scale the clips
    // author on these bones every frame, so the write happens once, on the
    // transition, and the animation owns the value from the next frame on.
    let leaving_first_person = *was_first_person && !first_person;
    *was_first_person = first_person;

    if !first_person && !leaving_first_person {
        return;
    }

    for rig in rigs.iter() {
        // The NECK collapses too.
        //
        // Hiding only the head left the neck standing in open view directly
        // under the camera — visible on every downward glance, and the reason
        // first person looked like it was filmed from inside someone's throat.
        // The neck's children are the head bones, which are collapsed anyway,
        // so nothing else is affected.
        for bone in FIRST_PERSON_HIDDEN_BONES {
            let Some(e) = rig.bone(bone) else { continue };
            let Ok(mut t) = bones.get_mut(e) else { continue };
            t.scale = if first_person {
                // Not exactly zero: a zero-scale joint produces a degenerate
                // matrix, which some skinning paths turn into NaN vertices.
                Vec3::splat(1.0e-4)
            } else {
                Vec3::ONE
            };
        }
    }
}

/// Face the movement direction. One integrator — the old code had two running
/// simultaneously at different rates (8.0 rad/s in `skinned_character.rs:306`
/// and 10.0 rad/s in `character_plugin.rs:446`) fighting over the same
/// transform.
pub(crate) fn face_movement_direction(
    time: Res<Time>,
    mut q: Query<
        (&mut Transform, &AvatarIntent, Option<&super::climb::AvatarClimb>),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    let dt = time.delta_secs();
    for (mut tf, intent, climb) in q.iter_mut() {
        // A climbing avatar's facing is owned by the ledge, not by input.
        // This runs in PostAnim, strictly AFTER `drive_climb`'s face-the-wall
        // slerp in Update, so without this guard any held direction during a
        // hang rotated the body off the wall while the IK kept the hands
        // welded to the ledge frame.
        if climb.is_some_and(|c| c.is_climbing()) {
            continue;
        }
        let mut d = intent.direction;
        d.y = 0.0;
        if d.length_squared() < 1e-4 {
            continue;
        }
        let target = (-d.x).atan2(-d.z);
        let current = tf.rotation.to_euler(EulerRot::YXZ).0;
        let diff = (target - current + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let alpha = 1.0 - (-10.0 * dt).exp();
        tf.rotation = Quat::from_rotation_y(current + diff * alpha);
    }
}

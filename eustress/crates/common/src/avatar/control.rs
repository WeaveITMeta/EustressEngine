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

/// A script owns this avatar camera (`Camera.CameraType = Scriptable`): the
/// orbit, zoom and follow systems leave it alone and the host writes its
/// transform and projection. `yaw` on its [`AvatarCamera`] is still kept in
/// step, so WASD stays relative to what the player sees.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct AvatarCameraScripted;

/// Script control over an avatar body (`Humanoid.AutoRotate`, and the facing
/// a script gives `HumanoidRootPart`). With `auto_rotate` off the body keeps
/// `facing_yaw` instead of turning toward its movement, which is what a
/// top-down shooter needs to strafe while aiming.
#[derive(Component, Debug, Clone, Copy)]
pub struct AvatarScriptControl {
    pub auto_rotate: bool,
    /// Radians about +Y, Bevy convention (0 faces -Z).
    pub facing_yaw: Option<f32>,
}

impl Default for AvatarScriptControl {
    fn default() -> Self {
        Self { auto_rotate: true, facing_yaw: None }
    }
}

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
        if !app.is_plugin_added::<crate::input_actions::InputActionsPlugin>() {
            app.add_plugins(crate::input_actions::InputActionsPlugin);
        }
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
            // After a seat has carried its rider, so the camera never trails a
            // moving vehicle by a frame.
            .add_systems(
                PostUpdate,
                camera_follow.after(AvatarSystems::Ride).before(TransformSystems::Propagate),
            )
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
            // A view camera: the shared look (TonyMcMapface, SMAA, bloom and
            // the Space's colour grade) from `camera_look`, the same as
            // Studio's editor camera. Builds without its lookup tables give
            // it Reinhard there instead.
            crate::plugins::camera_look::ViewCamera,
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
    actions: Res<crate::input_actions::Actions>,
    enabled: Res<AvatarInputEnabled>,
    // A GUI TextBox holding the keyboard takes the keys, WASD and Space too.
    // The action layer also sinks those keys while this is set.
    typing: Option<Res<crate::play_session::GuiKeyboardFocus>>,
    cameras: Query<&AvatarCamera>,
    mut q: Query<(&mut AvatarIntent, &AvatarBody), (With<LocalAvatar>, With<SpawnedByAvatarRuntime>)>,
) {
    for (mut intent, body) in q.iter_mut() {
        if body.control != AvatarControl::LocalPlayer || !enabled.0 || typing.as_deref().is_some_and(|t| t.0) {
            intent.direction = Vec3::ZERO;
            intent.sprint = false;
            intent.crouch = false;
            continue;
        }

        let yaw = cameras.iter().next().map(|c| c.yaw).unwrap_or(0.0);
        let move_v = actions.d2("Move");
        // Move.y is forward (W). The ground vector's z is -forward, which is
        // what reading KeyW used to write.
        intent.direction = crate::input_actions::camera_relative_direction(move_v.x, -move_v.y, yaw);
        intent.sprint = actions.pressed("Sprint");
        intent.crouch = actions.pressed("Crouch");

        // Latched, not level-triggered: the controller clears it on consume so
        // one press cannot produce two jumps.
        if actions.pressed_this_frame("Jump") {
            intent.jump_pressed = true;
        }
    }
}

/// Mouse pixels keep [`AvatarCamera::sensitivity`]. Touch radians are already
/// scaled (0.01 rad per logical pixel) and are not multiplied again.
pub fn apply_look(
    mut yaw: f32,
    mut pitch: f32,
    pitch_min: f32,
    pitch_max: f32,
    sensitivity: f32,
    first_person: bool,
    right_held: bool,
    mouse_px: Vec2,
    touch_rad: Vec2,
) -> (f32, f32) {
    if first_person || right_held {
        yaw -= mouse_px.x * sensitivity;
        pitch = (pitch + mouse_px.y * sensitivity).clamp(pitch_min, pitch_max);
    }
    yaw -= touch_rad.x;
    // Screen Y grows downward, so a finger moving up is a negative delta.
    // Positive pitch lifts the camera (`offset.y = sin(pitch) * distance`),
    // which is the direction Arrow Up already uses. Adding the raw delta
    // pitched the orbit down through the floor.
    pitch = (pitch - touch_rad.y).clamp(pitch_min, pitch_max);
    (yaw, pitch)
}

fn camera_look(
    actions: Res<crate::input_actions::Actions>,
    mouse: Res<ButtonInput<MouseButton>>,
    enabled: Res<AvatarInputEnabled>,
    mut cameras: Query<&mut AvatarCamera, Without<AvatarCameraScripted>>,
    mut warned: Local<bool>,
) {
    if !enabled.0 {
        return;
    }

    // Deliberately NOT `single_mut()`. That returns `Err` on two-or-more and
    // the old early return then dropped every mouse delta on the floor, so a
    // single leaked camera disabled looking entirely with nothing in the log
    // to say why. Driving all of them keeps the control alive while the count
    // is reported once.
    // None is normal: a Space that loads no character, or the frames before
    // the avatar arrives. Two or more is a leak.
    let n = cameras.iter().count();
    if n > 1 && !*warned {
        *warned = true;
        warn!("avatar: expected 1 AvatarCamera, found {n} — orbit will drive all of them");
    }

    let right = mouse.pressed(MouseButton::Right);
    if actions.mouse_look_px == Vec2::ZERO && actions.touch_look_rad == Vec2::ZERO {
        return;
    }

    for mut cam in cameras.iter_mut() {
        // First person always looks; third person orbits on right-drag.
        // Touch look is not gated on the right button: the right half of the
        // screen is the camera zone.
        let (yaw, pitch) = apply_look(
            cam.yaw,
            cam.pitch,
            cam.pitch_min,
            cam.pitch_max,
            cam.sensitivity,
            cam.first_person,
            right,
            actions.mouse_look_px,
            actions.touch_look_rad,
        );
        cam.yaw = yaw;
        cam.pitch = pitch;
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
    mut cameras: Query<&mut AvatarCamera, Without<AvatarCameraScripted>>,
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
    actions: Res<crate::input_actions::Actions>,
    enabled: Res<AvatarInputEnabled>,
    mut cameras: Query<&mut AvatarCamera, Without<AvatarCameraScripted>>,
    mut cursor: Query<&mut CursorOptions, With<Window>>,
) {
    let Ok(mut cam) = cameras.single_mut() else {
        return;
    };
    if !enabled.0 {
        return;
    }

    // Wheel notches, plus pinch already converted to that unit.
    let delta = actions.zoom_notches;
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

/// How far above the feet the third-person camera is allowed to sit.
/// The pitch clamp is an angle. `sin(pitch) * distance` is the Y that
/// goes through the ground, and ±1.35 rad still does.
pub const CAMERA_FLOOR_CLEARANCE_M: f32 = 0.35;

/// Gap left between an occluder and the pulled camera, in metres.
pub const CAMERA_OCCLUSION_SKIN_M: f32 = 0.25;

/// Third-person orbit translation and the pitch that produced it.
///
/// `pitch` is clamped to the angle limits, then raised until the camera's Y
/// is at least `floor_y`. The raised angle is the one returned: a floor
/// correction that only moves the translation leaves `AvatarCamera.pitch`
/// at the stop (−1.35), and every later upward drag is spent climbing out
/// of that dead zone before the camera leaves the ground.
pub fn follow_camera_orbit(
    eye: Vec3,
    yaw: f32,
    mut pitch: f32,
    distance: f32,
    pitch_min: f32,
    pitch_max: f32,
    floor_y: f32,
) -> (Vec3, f32) {
    pitch = pitch.clamp(pitch_min, pitch_max);
    let offset_at = |pitch: f32| {
        Vec3::new(yaw.sin() * pitch.cos(), pitch.sin(), yaw.cos() * pitch.cos()) * distance
    };
    let mut desired = eye + offset_at(pitch);
    if desired.y < floor_y && distance > 1e-4 {
        let min_sin = ((floor_y - eye.y) / distance).clamp(-1.0, 1.0);
        pitch = pitch.max(min_sin.asin()).clamp(pitch_min, pitch_max);
        desired = eye + offset_at(pitch);
        desired.y = desired.y.max(floor_y);
    }
    (desired, pitch)
}

/// Third-person orbit translation. See [`follow_camera_orbit`].
pub fn follow_camera_translation(
    eye: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    pitch_min: f32,
    pitch_max: f32,
    floor_y: f32,
) -> Vec3 {
    follow_camera_orbit(eye, yaw, pitch, distance, pitch_min, pitch_max, floor_y).0
}

/// Where the camera sits when a ray from `eye` toward `camera` hits at
/// `hit_distance` metres. `None` when that hit does not block the orbit.
/// The returned point is the pulled position, on the near side of the hit.
pub fn pull_follow_camera(eye: Vec3, camera: Vec3, hit_distance: f32) -> Option<Vec3> {
    let to_cam = camera - eye;
    let dist = to_cam.length();
    if dist < 1e-4 || !(hit_distance > 0.05 && hit_distance < dist - CAMERA_OCCLUSION_SKIN_M) {
        return None;
    }
    let along = (hit_distance - CAMERA_OCCLUSION_SKIN_M).max(0.4);
    Some(eye + (to_cam / dist) * along.min(dist))
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
    spatial: avian3d::prelude::SpatialQuery,
    bodies: Query<(&Transform, &AvatarBody), (With<SpawnedByAvatarRuntime>, Without<AvatarCamera>)>,
    mut cameras: Query<(&mut Transform, &mut AvatarCamera), Without<AvatarCameraScripted>>,
    mut since_log: Local<f32>,
) {
    let dt = time.delta_secs();
    for (mut cam_tf, mut cam) in cameras.iter_mut() {
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

        // The angle clamp alone does not keep the camera above the ground:
        // pitch -1.35 puts `sin(pitch) * distance` several metres under the
        // feet. `follow_camera_translation` clamps that Y. A hit on the way
        // there is the position the camera keeps, so the next frame does
        // not lerp back through the wall.
        let feet_y = body_tf.translation.y - body.metrics.capsule_half_extent();
        let floor_y = feet_y + CAMERA_FLOOR_CLEARANCE_M;
        let stored_pitch = cam.pitch;
        let (open, used_pitch) = follow_camera_orbit(
            eye,
            cam.yaw,
            stored_pitch,
            cam.distance,
            cam.pitch_min,
            cam.pitch_max,
            floor_y,
        );
        // The floor raised the angle it used for this frame's position.
        // Store that angle. Leaving −1.35 in `cam.pitch` makes the next
        // upward drag invisible until the finger climbs back to the angle
        // the camera is already using.
        if used_pitch > stored_pitch + 1.0e-3 {
            if used_pitch - stored_pitch > 0.05 {
                eprintln!(
                    "phase0: floor wrote pitch {stored_pitch:.3} -> {used_pitch:.3} (camera y was already on the floor)"
                );
            }
            cam.pitch = used_pitch;
        }
        let mut desired = open;
        let mut pulled = false;
        let to_cam = open - eye;
        let dist = to_cam.length();
        if dist > 0.05 {
            if let Ok(dir) = Dir3::new(to_cam / dist) {
                let filter = avian3d::prelude::SpatialQueryFilter::default().with_excluded_entities([cam.target]);
                if let Some(hit) = spatial.cast_ray(eye, dir, dist, true, &filter) {
                    if let Some(place) = pull_follow_camera(eye, open, hit.distance) {
                        desired = place;
                        desired.y = desired.y.max(floor_y);
                        pulled = true;
                    }
                }
            }
        }
        *since_log += dt;
        if *since_log > 0.4 && (pulled || (open.y - (eye.y + cam.pitch.sin() * cam.distance)).abs() > 0.2) {
            *since_log = 0.0;
            eprintln!(
                "phase0: follow camera at ({:.2}, {:.2}, {:.2}) pitch={:.2} floor={floor_y:.2} pulled={pulled}",
                desired.x, desired.y, desired.z, cam.pitch
            );
        }
        // Framerate-independent smoothing toward the safe position.
        let alpha = 1.0 - (-12.0 * dt).exp();
        cam_tf.translation = cam_tf.translation.lerp(desired, alpha);
        if cam_tf.translation.y < floor_y {
            cam_tf.translation.y = floor_y;
        }
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
        (
            &mut Transform,
            &AvatarIntent,
            Option<&super::climb::AvatarClimb>,
            Option<&AvatarScriptControl>,
            Has<super::seat::AvatarSeated>,
        ),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    let dt = time.delta_secs();
    for (mut tf, intent, climb, script, seated) in q.iter_mut() {
        // A rider faces where its seat faces.
        if seated {
            continue;
        }
        // A climbing avatar's facing is owned by the ledge, not by input.
        // This runs in PostAnim, strictly AFTER `drive_climb`'s face-the-wall
        // slerp in Update, so without this guard any held direction during a
        // hang rotated the body off the wall while the IK kept the hands
        // welded to the ledge frame.
        if climb.is_some_and(|c| c.is_climbing()) {
            continue;
        }
        // A script owns the facing (`Humanoid.AutoRotate = false`).
        if let Some(control) = script {
            if !control.auto_rotate {
                if let Some(yaw) = control.facing_yaw {
                    tf.rotation = Quat::from_rotation_y(yaw);
                }
                continue;
            }
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

#[cfg(test)]
mod look_tests {
    use super::apply_look;

    #[test]
    fn mouse_pixels_use_the_camera_sensitivity_and_the_right_button() {
        let (yaw, pitch) = apply_look(0.0, 0.3, -1.35, 1.35, 0.003, false, true, bevy::prelude::Vec2::new(10.0, -4.0), bevy::prelude::Vec2::ZERO);
        assert!((yaw - -0.03).abs() < 1e-6, "{yaw}");
        assert!((pitch - (0.3 - 0.012)).abs() < 1e-6, "{pitch}");
        let (yaw, pitch) = apply_look(0.0, 0.3, -1.35, 1.35, 0.003, false, false, bevy::prelude::Vec2::new(10.0, -4.0), bevy::prelude::Vec2::ZERO);
        assert_eq!((yaw, pitch), (0.0, 0.3), "third person ignores the mouse until the right button is held");
    }

    #[test]
    fn touch_radians_apply_without_the_mouse_button() {
        // Positive touch Y is a downward drag and lowers the camera.
        let (yaw, pitch) = apply_look(0.0, 0.0, -1.35, 1.35, 0.003, false, false, bevy::prelude::Vec2::ZERO, bevy::prelude::Vec2::new(0.01, 0.02));
        assert!((yaw - -0.01).abs() < 1e-6 && (pitch - -0.02).abs() < 1e-6, "{yaw} {pitch}");
    }

    #[test]
    fn touch_look_cannot_pitch_past_the_desktop_limit() {
        // A finger moving up is a negative screen delta and lifts the camera,
        // the same direction as Arrow Up.
        let (_, pitch) = apply_look(0.0, 1.2, -1.35, 1.35, 0.003, false, false, bevy::prelude::Vec2::ZERO, bevy::prelude::Vec2::new(0.0, -4.0));
        assert!((pitch - 1.35).abs() < 1e-5, "an upward drag stops at pitch_max, got {pitch}");
        let (_, pitch) = apply_look(0.0, -1.2, -1.35, 1.35, 0.003, false, false, bevy::prelude::Vec2::ZERO, bevy::prelude::Vec2::new(0.0, 4.0));
        assert!((pitch - -1.35).abs() < 1e-5, "a downward drag stops at pitch_min, got {pitch}");
    }

    #[test]
    fn a_pitch_under_the_ground_stays_above_the_floor_and_a_hit_pulls_the_camera() {
        use bevy::prelude::Vec3;
        let eye = Vec3::new(11.0, 1.66, 10.29);
        // The parity11 orbit: pitch -1.35 at ~4.82 m lands at y=-3.04.
        let buried = super::follow_camera_translation(eye, 0.4, -1.35, 4.82, -1.35, 1.35, 0.35);
        assert!(buried.y >= 0.35 - 1e-3, "camera went under the floor at y={}", buried.y);
        let high = super::follow_camera_translation(eye, 0.4, 1.35, 4.82, -1.35, 1.35, 0.35);
        assert!(high.y > eye.y, "positive pitch (drag up) should lift the camera, y={}", high.y);

        let underground = Vec3::new(10.19, -3.04, 9.62);
        let pulled = super::pull_follow_camera(eye, underground, 1.49).expect("the hit blocks the orbit");
        let dir = (underground - eye).normalize();
        let expected = eye + dir * (1.49 - super::CAMERA_OCCLUSION_SKIN_M);
        assert!(
            pulled.distance(expected) < 1e-3,
            "pull left the camera at {pulled:?}, wanted {expected:?}"
        );
        assert!(pulled.distance(eye) + 0.2 < underground.distance(eye));
        assert!(super::pull_follow_camera(eye, underground, 50.0).is_none());
    }

    #[test]
    fn the_floor_writes_its_pitch_back_so_an_upward_drag_leaves_the_ground() {
        use bevy::prelude::Vec3;
        let eye = Vec3::new(11.0, 1.66, 10.29);
        let (buried, pitch) = super::follow_camera_orbit(eye, 0.4, -1.35, 4.82, -1.35, 1.35, 0.35);
        assert!(buried.y >= 0.35 - 1e-3, "camera went under the floor at y={}", buried.y);
        assert!(
            pitch > -0.5,
            "the floor left pitch at the angle stop {pitch}; an upward drag would not move the camera"
        );
        let (_, stayed) = super::follow_camera_orbit(eye, 0.4, 0.4, 4.82, -1.35, 1.35, 0.35);
        assert!((stayed - 0.4).abs() < 1e-5, "a pitch already above the floor was rewritten to {stayed}");

        // One short upward drag from the angle the camera is actually using.
        let (_, raised) = apply_look(0.4, pitch, -1.35, 1.35, 0.003, false, false, bevy::prelude::Vec2::ZERO, bevy::prelude::Vec2::new(0.0, -0.05));
        assert!(raised > pitch + 0.04, "upward drag did not raise pitch ({pitch} -> {raised})");
        let (up, _) = super::follow_camera_orbit(eye, 0.4, raised, 4.82, -1.35, 1.35, 0.35);
        assert!(
            up.y > buried.y + 0.05,
            "upward drag from the floor pitch left the camera at y={} (was {})",
            up.y,
            buried.y
        );
    }
}

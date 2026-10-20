//! Look and move while this page has no avatar.
//!
//! A Space with `Players.CharacterAutoLoads` off (a paddle game) has no body,
//! and the Play runtime shows the Space's camera. This drives that camera
//! from the keyboard and the mouse, and writes the pose back onto the tree
//! camera so the Play runtime's camera step (which copies the tree onto the
//! view each frame) keeps it. Cameras are not replicated, so a joined session
//! is not moved on the host by looking around here.
//!
//! While an avatar camera exists, the character controller and its follow
//! camera own the keys and the mouse, and this system does nothing.
//!
//! Click locks the mouse. Esc releases it. WASD moves on the look direction,
//! Space up, Ctrl down, Shift faster.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use eustress_common::avatar::control::AvatarCamera;
use eustress_common::datamodel::DmValue;
use eustress_common::play_session::{PlayDataModel, ScriptedPlayCamera};
use eustress_common::CFrame;

/// Present on every browser session. The system does nothing until the
/// play camera exists.
#[derive(Resource, Default)]
pub(crate) struct Explore;

const LOOK: f32 = 0.0025;
const WALK: f32 = 12.0;
const FAST: f32 = 28.0;
const PITCH_LIMIT: f32 = 1.45;

pub(crate) fn drive(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut cams: Query<&mut Transform, With<ScriptedPlayCamera>>,
    avatar: Query<(), With<AvatarCamera>>,
    dm: Option<Res<PlayDataModel>>,
    _explore: Option<Res<Explore>>,
) {
    if _explore.is_none() || !avatar.is_empty() {
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        if let Ok(mut cursor) = cursor.single_mut() {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        if let Ok(mut cursor) = cursor.single_mut() {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }

    let Ok(mut tf) = cams.single_mut() else {
        return;
    };
    let locked = cursor
        .single()
        .is_ok_and(|c| c.grab_mode == CursorGrabMode::Locked);
    let mut changed = false;

    if locked {
        let delta = motion.as_deref().map(|m| m.delta).unwrap_or_default();
        if delta.length_squared() > 0.0 {
            let (mut yaw, mut pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
            yaw -= delta.x * LOOK;
            pitch = (pitch - delta.y * LOOK).clamp(-PITCH_LIMIT, PITCH_LIMIT);
            tf.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
            changed = true;
        }
    }

    let mut wish = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        wish.z -= 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        wish.z += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        wish.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        wish.x += 1.0;
    }
    if keys.pressed(KeyCode::Space) {
        wish.y += 1.0;
    }
    if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) {
        wish.y -= 1.0;
    }
    if wish.length_squared() > 0.0 {
        let fast = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        let speed = if fast { FAST } else { WALK };
        let step = tf.rotation * wish.normalize() * speed * time.delta_secs();
        tf.translation += step;
        changed = true;
    }

    if !changed {
        return;
    }
    let Some(dm) = dm else { return };
    let mut tree = dm.dm.lock();
    let Some(camera) = tree.current_camera() else {
        return;
    };
    tree.set_prop_from_engine(
        camera,
        "CFrame",
        DmValue::CFrame(CFrame::from_transform(&tf)),
    );
}

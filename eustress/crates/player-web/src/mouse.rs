//! The browser Player's mouse.
//!
//! - It follows the browser's pointer lock (`mouse_lock.rs`). Esc in the
//!   browser releases the mouse without the page seeing the key, so the
//!   Player watches the document instead: once the browser lets go, the
//!   cursor is freed and shown, and a first-person view steps back out as
//!   Esc does on the desktop Player (`common` `avatar::control`
//!   `escape_owner`). A lock the browser refused is given up after a second.
//! - In first person with no lock, the mouse does not turn the view, and a
//!   click on the world takes the mouse again.
//! - In third person, the right button turns the camera. The mouse is held
//!   for the drag, so the turn does not stop at the screen's edge or wander
//!   off the canvas, and it is let go when the button comes up.
//!
//! With no avatar camera (a Space with no character) the Player never takes
//! the mouse, as on the desktop Player: the Space's scripts move its camera.

use bevy::ecs::message::Messages;
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use eustress_common::avatar::control::AvatarCamera;

use crate::mouse_lock::{LockStep, LockWatch};

/// How far out a first-person view steps on Esc, past the closest zoom, as
/// the desktop Player's Escape does.
const ESC_STEP_OUT_M: f32 = 1.5;

pub(crate) fn add(app: &mut App) {
    app.add_systems(
        Update,
        follow_browser_lock.before(eustress_common::avatar::AvatarSystems::Input),
    );
}

#[allow(clippy::too_many_arguments)]
fn follow_browser_lock(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut watch: Local<LockWatch>,
    mut dragging: Local<bool>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut cameras: Query<&mut AvatarCamera>,
    mut motion: ResMut<Messages<MouseMotion>>,
) {
    let Ok(mut c) = cursor.single_mut() else {
        return;
    };
    let holds = browser_holds_lock();
    let asked = c.grab_mode == CursorGrabMode::Locked;
    match watch.step(asked, holds, time.delta_secs()) {
        LockStep::Keep => {}
        LockStep::LeftByPerson => {
            free(&mut c);
            *dragging = false;
            for mut cam in cameras.iter_mut() {
                if cam.first_person {
                    cam.first_person = false;
                    cam.distance = cam.min_distance + ESC_STEP_OUT_M;
                }
            }
            return;
        }
        LockStep::Refused => {
            free(&mut c);
            *dragging = false;
            return;
        }
    }

    if cameras.is_empty() {
        return;
    }
    if cameras.iter().any(|cam| cam.first_person) {
        if !holds {
            // A free mouse does not turn the first-person view.
            motion.clear();
            if mouse.just_pressed(MouseButton::Left) && c.grab_mode != CursorGrabMode::Locked {
                lock(&mut c);
            }
        }
        return;
    }
    if mouse.just_pressed(MouseButton::Right) && c.grab_mode != CursorGrabMode::Locked {
        lock(&mut c);
        *dragging = true;
    }
    if *dragging && !mouse.pressed(MouseButton::Right) {
        free(&mut c);
        *dragging = false;
    }
}

fn lock(c: &mut CursorOptions) {
    c.grab_mode = CursorGrabMode::Locked;
    c.visible = false;
}

fn free(c: &mut CursorOptions) {
    c.grab_mode = CursorGrabMode::None;
    c.visible = true;
}

/// The document has a pointer-lock element.
fn browser_holds_lock() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.pointer_lock_element())
        .is_some()
}

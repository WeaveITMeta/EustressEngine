//! Lock / Unlock paint-mode tools.
//!
//! The ribbon's Lock and Unlock buttons each switch the active tool to
//! [`crate::ui::Tool::Lock`] or [`crate::ui::Tool::Unlock`]. While
//! either is active, clicking a part in the viewport flips its
//! `BasePart.locked` state — Lock paints `true`, Unlock paints `false`.
//!
//! Why a tool mode rather than "lock the current selection":
//! - The Select tool's hit-test deliberately *excludes* locked parts
//!   (so locked parts don't intercept clicks meant for the box-select
//!   that drags around them). That makes it impossible to recover an
//!   accidentally-locked part through normal selection — there's no
//!   way to click on it. The Unlock tool's hit-test ignores the locked
//!   filter, giving the only path back to "this part is now editable
//!   again" without round-tripping through the Properties panel.
//! - Painting locks across many parts in succession (e.g. lock every
//!   prop in a finished area before moving on) is faster than
//!   select-N-things → press shortcut → repeat.
//!
//! Press Escape to return to the Select tool. Both tools are
//! mutually exclusive with the standard transform tools (Move/Rotate/
//! Scale) — switching to Lock/Unlock cancels any in-progress drag.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use eustress_common::classes::BasePart;
use crate::ui::{StudioState, Tool, SlintUIFocus};
use crate::math_utils::ray_intersects_part;

pub struct LockToolPlugin;

impl Plugin for LockToolPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            handle_lock_unlock_clicks,
            cancel_on_escape,
        ));
    }
}

/// Click handler for both [`Tool::Lock`] and [`Tool::Unlock`]. Reads the
/// current tool, raycasts the part under the cursor (locked parts ARE
/// included so Unlock can target them), and sets the part's `locked`
/// flag to the corresponding value.
fn handle_lock_unlock_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    studio_state: Option<Res<StudioState>>,
    ui_focus: Option<Res<SlintUIFocus>>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut parts: Query<(Entity, &GlobalTransform, &mut BasePart, &Name)>,
) {
    let Some(studio_state) = studio_state else { return };
    let target_locked = match studio_state.current_tool {
        Tool::Lock => true,
        Tool::Unlock => false,
        _ => return,
    };

    if !mouse.just_pressed(MouseButton::Left) { return; }

    // Block when Slint UI has focus or cursor is over UI panels — same
    // gating as the standard select tool.
    if ui_focus.as_ref().map(|f| f.has_focus || f.text_input_focused).unwrap_or(false) {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };

    if let Some(vb) = viewport_bounds.as_deref() {
        let scale = window.scale_factor() as f32;
        if !vb.contains_logical(cursor_pos, scale) { return; }
    }

    let Some((camera, camera_transform)) = cameras.iter().find(|(c, _)| c.order == 0) else { return };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return };

    // Find the closest part under the cursor — INCLUDING locked parts.
    // This is the deliberate divergence from the Select tool: Unlock
    // mode must be able to target locked parts, and Lock mode shouldn't
    // care either way.
    let mut best: Option<(Entity, f32)> = None;
    for (entity, gt, bp, _name) in parts.iter() {
        let t = gt.compute_transform();
        if let Some(distance) = ray_intersects_part(ray.origin, *ray.direction, &t, bp.size) {
            match best {
                Some((_, d)) if d <= distance => {}
                _ => best = Some((entity, distance)),
            }
        }
    }

    let Some((hit_entity, _)) = best else { return };
    let Ok((_, _, mut basepart, name)) = parts.get_mut(hit_entity) else { return };

    if basepart.locked == target_locked {
        // Already in the desired state — log only, so the user gets
        // visual feedback that the click was received.
        info!(
            "{} '{}' is already {}",
            if target_locked { "🔒" } else { "🔓" },
            name.as_str(),
            if target_locked { "locked" } else { "unlocked" },
        );
        return;
    }

    basepart.locked = target_locked;
    info!(
        "{} '{}' is now {}",
        if target_locked { "🔒" } else { "🔓" },
        name.as_str(),
        if target_locked { "locked" } else { "unlocked" },
    );
}

/// Escape exits Lock/Unlock back to Select. Mirrors how Move/Scale/
/// Rotate handle Escape during a drag, except here it just switches
/// tools without any in-progress state to cancel.
fn cancel_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    studio_state: Option<ResMut<StudioState>>,
    ui_focus: Option<Res<SlintUIFocus>>,
) {
    if ui_focus.as_ref().map(|f| f.text_input_focused).unwrap_or(false) { return; }
    if !keys.just_pressed(KeyCode::Escape) { return; }
    let Some(mut studio_state) = studio_state else { return };
    if matches!(studio_state.current_tool, Tool::Lock | Tool::Unlock) {
        studio_state.current_tool = Tool::Select;
    }
}

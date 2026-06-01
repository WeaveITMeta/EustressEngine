//! ClickDetector runtime — camera-raycast hit-testing + event firing.
//!
//! Roblox `ClickDetector` fires `MouseClick` when the player clicks a part
//! (or any descendant of the part) the detector is parented to, from within
//! `max_activation_distance`. It also fires `MouseHoverEnter` /
//! `MouseHoverLeave` as the cursor moves on/off the part.
//!
//! ## How it works
//!
//! Both systems cast a ray from the active editor/play camera through the
//! cursor (the canonical engine idiom — see `part_selection.rs`). The hit
//! entity, or one of its ancestors (walking `ChildOf`), may carry a
//! [`ClickDetector`]. If the hit distance is within the detector's
//! `max_activation_distance` we treat it as a valid target.
//!
//! - [`click_detector_system`] handles the click → `MouseClick`.
//! - [`click_hover_system`] tracks the hovered detector → `MouseHoverEnter` /
//!   `MouseHoverLeave`, storing the current hover in [`HoverState`].
//!
//! ## Events
//!
//! Events fire on the [`EventBus`] under per-entity topic names so a Luau /
//! Rune script can `EventBus:Connect("ClickDetector.<entity>.MouseClick", …)`.
//! The payload is `[EntityId(detector), EntityId(player_character)]` — the
//! detector that fired and the character that clicked it (Roblox passes the
//! clicking `Player`; we pass the character entity, the closest analogue).
//!
//! Topic shape (stable):
//! - `ClickDetector.<entity_bits>.MouseClick`
//! - `ClickDetector.<entity_bits>.MouseHoverEnter`
//! - `ClickDetector.<entity_bits>.MouseHoverLeave`
//!
//! plus a class-wide aggregate topic (`ClickDetector.MouseClick`, …) for
//! listeners that don't want to bind per-entity.
//!
//! ## No-op safety
//!
//! Resolves the local character via [`LocalCharacter`]; if there's no
//! character, no window, or no order-0 camera, the systems return early.
//!
//! [`EventBus`]: eustress_common::events::EventBus

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};

use eustress_common::classes::ClickDetector;
use eustress_common::events::EventBusResource;
use eustress_common::scripting::events::SignalArg;

use super::LocalCharacter;

/// Tracks which [`ClickDetector`] (if any) the cursor currently hovers, so
/// hover-enter / hover-leave fire exactly once on transitions.
#[derive(Resource, Default)]
pub struct HoverState {
    /// The detector entity currently hovered, or `None`.
    pub hovered: Option<Entity>,
}

/// Walk up the `ChildOf` chain from `start` (inclusive) and return the first
/// ancestor that carries a [`ClickDetector`], plus the detector itself.
/// Capped at a sane depth to avoid pathological hierarchies.
fn find_click_detector(
    start: Entity,
    detectors: &Query<&ClickDetector>,
    parents: &Query<&ChildOf>,
) -> Option<(Entity, f32)> {
    let mut current = start;
    for _ in 0..32 {
        if let Ok(det) = detectors.get(current) {
            return Some((current, det.max_activation_distance));
        }
        match parents.get(current) {
            Ok(child_of) => current = child_of.parent(),
            Err(_) => break,
        }
    }
    None
}

/// Cast a ray from the order-0 camera through the cursor and return the
/// closest physics hit `(entity, distance)`, or `None`.
fn cursor_ray_hit(
    windows: &Query<&Window, With<PrimaryWindow>>,
    cameras: &Query<(&Camera, &GlobalTransform)>,
    spatial: &SpatialQuery,
) -> Option<(Entity, f32)> {
    let window = windows.iter().next()?;
    let cursor_pos = window.cursor_position()?;
    let (camera, cam_transform) = cameras.iter().find(|(c, _)| c.order == 0)?;
    let ray = camera.viewport_to_world(cam_transform, cursor_pos).ok()?;
    let dir = Dir3::new(*ray.direction).ok()?;

    let hits = spatial.ray_hits(ray.origin, dir, 10_000.0, 20, true, &SpatialQueryFilter::default());
    // ray_hits is not guaranteed sorted; pick the nearest.
    hits.iter()
        .map(|h| (h.entity, h.distance))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

/// On left-click, raycast from the camera; if the hit entity (or an
/// ancestor) has a [`ClickDetector`] within `max_activation_distance`, fire
/// its `MouseClick` event on the [`EventBus`].
pub fn click_detector_system(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    spatial: SpatialQuery,
    detectors: Query<&ClickDetector>,
    parents: Query<&ChildOf>,
    local: LocalCharacter,
    bus: Res<EventBusResource>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // No local character ⇒ no-op (editor, pre-spawn).
    let Some(character) = local.entity() else { return };

    let Some((hit_entity, distance)) = cursor_ray_hit(&windows, &cameras, &spatial) else {
        return;
    };
    let Some((detector, max_dist)) = find_click_detector(hit_entity, &detectors, &parents) else {
        return;
    };
    if distance > max_dist {
        return; // hit a detector, but too far to activate
    }

    let payload = vec![
        SignalArg::EntityId(detector.to_bits()),
        SignalArg::EntityId(character.to_bits()),
    ];
    bus.0.fire(
        &format!("ClickDetector.{}.MouseClick", detector.to_bits()),
        payload.clone(),
    );
    bus.0.fire("ClickDetector.MouseClick", payload);

    tracing::debug!(
        "[ClickDetector] MouseClick on {:?} (dist {:.2} ≤ {:.2})",
        detector,
        distance,
        max_dist
    );
}

/// Track the hovered [`ClickDetector`] and fire `MouseHoverEnter` /
/// `MouseHoverLeave` on transitions.
pub fn click_hover_system(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    spatial: SpatialQuery,
    detectors: Query<&ClickDetector>,
    parents: Query<&ChildOf>,
    local: LocalCharacter,
    bus: Res<EventBusResource>,
    mut hover: ResMut<HoverState>,
) {
    let Some(character) = local.entity() else {
        // Clear any stale hover when the character vanishes.
        hover.hovered = None;
        return;
    };

    // Determine the detector currently under the cursor (if any, in range).
    let current = cursor_ray_hit(&windows, &cameras, &spatial).and_then(|(hit, distance)| {
        find_click_detector(hit, &detectors, &parents).and_then(|(det, max_dist)| {
            (distance <= max_dist).then_some(det)
        })
    });

    if current == hover.hovered {
        return; // no transition
    }

    // Leave the previous detector.
    if let Some(prev) = hover.hovered {
        let payload = vec![
            SignalArg::EntityId(prev.to_bits()),
            SignalArg::EntityId(character.to_bits()),
        ];
        bus.0.fire(
            &format!("ClickDetector.{}.MouseHoverLeave", prev.to_bits()),
            payload.clone(),
        );
        bus.0.fire("ClickDetector.MouseHoverLeave", payload);
    }
    // Enter the new detector.
    if let Some(now) = current {
        let payload = vec![
            SignalArg::EntityId(now.to_bits()),
            SignalArg::EntityId(character.to_bits()),
        ];
        bus.0.fire(
            &format!("ClickDetector.{}.MouseHoverEnter", now.to_bits()),
            payload.clone(),
        );
        bus.0.fire("ClickDetector.MouseHoverEnter", payload);
    }

    hover.hovered = current;
}

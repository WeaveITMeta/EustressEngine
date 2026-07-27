//! # Viewport (3D) right-click context menu
//!
//! Detects a right-click on the 3D viewport that wasn't a camera-orbit
//! drag, resolves what the user actually pointed at, and asks the Slint
//! side to show the build-oriented viewport menu at the release position.
//!
//! A right-click in the viewport is ambiguous — it can start a camera
//! orbit (hold + drag) or request a context menu (press + release in
//! place). We distinguish by tracking the press position and comparing
//! it against the release position; if the cursor moved less than
//! [`DRAG_THRESHOLD_PX`], we treat it as a click, otherwise a drag.
//!
//! A click that survives that test raycasts through the cursor with the
//! same Avian [`SpatialQuery`](avian3d::prelude::SpatialQuery) pick every
//! modal tool uses, then applies left-click selection semantics to the
//! frontmost hit: select the parent `Model` unless Alt is held (then the
//! part itself), never select a locked part, and leave the selection
//! completely alone when the target is already part of it — right-
//! clicking one of five selected parts must not collapse the selection to
//! one. Empty space selects nothing; it just opens the menu.
//!
//! The result is published two ways. [`ViewportContextTarget`] carries the
//! hit entity and world-space hit point for the Bevy-side action handlers
//! (`insert-here` / `paste-here` place at the cursor rather than in front
//! of the camera), and [`VIEWPORT_CONTEXT_NODE_ID`] carries the Explorer
//! row id across to the Slint overlay thread so the menu can name its
//! target. `context-menu-target-type` is `"viewport"`, which is what the
//! Slint `ContextMenu` component switches on to render the viewport item
//! set instead of the Explorer entity item set.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use std::sync::atomic::{AtomicI32, Ordering};

use super::slint_bridge::SlintBridge;
use super::ViewportBounds;
use super::SlintUIFocus;
use crate::classes::{BasePart, ClassName, Instance};
use crate::entity_utils::entity_to_id_string;
use crate::rendering::PartEntity;
use crate::selection_sync::SelectionSyncManager;
use eustress_common::default_scene::PartEntityMarker;

/// If the cursor moves farther than this (in pixels) between right-mouse
/// down and up, treat the gesture as a camera orbit rather than a click.
const DRAG_THRESHOLD_PX: f32 = 4.0;

/// How far the pick ray travels before giving up. Matches
/// `modal_tool::run_active_modal_tool` so right-click reach and tool
/// reach are the same distance.
const PICK_RAY_LENGTH: f32 = 10_000.0;

/// Per-click state for the viewport right-click detector.
#[derive(Resource, Default)]
pub struct ViewportRightClickState {
    /// Cursor position at press time (window-local). `None` when not
    /// currently pressed.
    press_pos: Option<Vec2>,
}

/// What the most recent viewport right-click pointed at.
///
/// Overwritten on every viewport right-click that opens the menu, so the
/// action handlers that run later (when the user picks an item) see the
/// state from the click that opened the menu they're acting on.
#[derive(Resource, Default, Clone)]
pub struct ViewportContextTarget {
    /// Entity under the cursor at right-click time, None for empty space.
    ///
    /// This is the frontmost *selectable* entity — a hit on a locked part
    /// or a non-Part class (Folder, ScreenGui, …) reports `None` here
    /// while still reporting a `hit_point`, so the menu can't offer to act
    /// on something the click deliberately refused to select.
    pub entity: Option<Entity>,
    /// World-space point the ray struck — the surface point for a hit, or
    /// the ray/ground-plane intersection for empty space. None if neither
    /// resolved.
    pub hit_point: Option<Vec3>,
    /// Explorer node id for [`Self::entity`] after the model-vs-part rule
    /// has been applied, or `-1` when nothing resolved. Same value as
    /// [`VIEWPORT_CONTEXT_NODE_ID`]; kept here so Bevy-side consumers
    /// don't have to reach for the atomic.
    pub node_id: i32,
    /// Set to the click position (window-local pixels) when this target was
    /// resolved by a right-click that should OPEN the menu.
    ///
    /// `slint_ui::sync_bevy_to_slint` takes it and pushes
    /// `show-context-menu = true` onto the live `StudioWindow`. The
    /// [`SlintBridge`] request below carries the same signal to the separate
    /// overlay-thread shell; that shell is not part of the in-process build,
    /// so this field — not the bridge — is what actually opens the menu.
    pub open_request: Option<(f32, f32)>,
}

/// Explorer node id for the entity the last viewport right-click resolved,
/// or `-1` for empty space / an entity the Explorer has not assigned a row
/// id to.
///
/// This is a cross-thread hand-off, not a convenience, and it exists only
/// for the separate-overlay-thread path: `slint_main`'s `apply_bridge_state`
/// has no World access, so it cannot read [`ViewportContextTarget`] directly
/// — the same reason `slint_ui::OVERLAY_INPUT_FOCUSED` exists. The producer
/// stores here BEFORE stamping `request_viewport_context_menu` onto the
/// bridge, and the bridge mutex supplies the happens-before edge, so a tick
/// that sees the request always sees the matching id.
///
/// Consumers that ARE Bevy systems (the in-process `sync_bevy_to_slint` path
/// in `slint_ui`, and every `ContextAction` handler) should read
/// [`ViewportContextTarget::node_id`] instead. It carries the same value plus
/// the entity and hit point, and needs no ordering reasoning.
pub static VIEWPORT_CONTEXT_NODE_ID: AtomicI32 = AtomicI32::new(-1);

/// Component lookup for the entity the pick ray struck. All-`Option` so a
/// single `get()` answers every question the selection rules ask —
/// identity, class, lock state, and parent — without four separate
/// queries eating four of Bevy's 16 system-param slots.
type PickLookup<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static PartEntity>,
        Option<&'static PartEntityMarker>,
        Option<&'static Instance>,
        Option<&'static BasePart>,
        Option<&'static ChildOf>,
    ),
>;

/// Bevy system. Runs every frame; cheap when idle (no mouse events) —
/// the raycast only runs on the release frame of a click that passed the
/// drag threshold.
pub fn detect_viewport_right_click(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    spatial_query: avian3d::prelude::SpatialQuery,
    viewport: Option<Res<ViewportBounds>>,
    ui_focus: Option<Res<SlintUIFocus>>,
    bridge: Option<Res<SlintBridge>>,
    selection_manager: Option<Res<SelectionSyncManager>>,
    mut explorer_state: Option<ResMut<crate::ui::slint_ui::UnifiedExplorerState>>,
    mut state: ResMut<ViewportRightClickState>,
    mut commands: Commands,
    pick_q: PickLookup,
) {
    // NOTE: `bridge` is deliberately NOT a hard requirement. `SlintBridge` is
    // only inserted by the separate-overlay-thread shell, which the in-process
    // build does not run — gating on it here made every right-click return
    // before the raycast, so the menu could never open at all.
    let Some(viewport) = viewport else { return };
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        // Cursor left the window — discard any pending press so a release
        // outside doesn't fire the menu.
        state.press_pos = None;
        return;
    };

    // Skip if Slint owns pointer focus (user is inside a panel/UI).
    let ui_has_focus = ui_focus.map(|f| f.has_focus).unwrap_or(false);
    if ui_has_focus {
        state.press_pos = None;
        return;
    }

    // Only consider clicks inside the viewport rectangle. `ViewportBounds`
    // is stored in PHYSICAL pixels but `cursor_position()` is LOGICAL, so
    // the comparison goes through `contains_logical` — the same DPI trap
    // that once made every 3D click silently miss on a scaled display.
    let in_viewport = viewport.contains_logical(cursor, window.scale_factor() as f32);

    if mouse.just_pressed(MouseButton::Right) && in_viewport {
        state.press_pos = Some(cursor);
        return;
    }

    if mouse.just_released(MouseButton::Right) {
        let Some(start) = state.press_pos.take() else { return };
        let delta = cursor - start;
        // Square-norm comparison avoids a sqrt in the hot path.
        if delta.length_squared() > DRAG_THRESHOLD_PX * DRAG_THRESHOLD_PX {
            return; // It was a drag (camera orbit) — no menu.
        }

        // Resolve what's under the cursor BEFORE the menu opens, so the
        // menu operates on what the user pointed at rather than on
        // whatever happened to be selected beforehand.
        let mut target = resolve_viewport_target(
            cursor,
            &keys,
            &cameras,
            &spatial_query,
            &pick_q,
            selection_manager.as_deref(),
            explorer_state.as_deref_mut(),
        );
        // PHYSICAL pixels: the consumer divides by Slint's own scale factor to
        // reach Slint logical coordinates, which is the only conversion that
        // lands the menu under the cursor on a scaled display.
        let scale = window.scale_factor() as f32;
        target.open_request = Some((cursor.x * scale, cursor.y * scale));

        // Publish the node id first: the bridge mutex below is what the
        // overlay thread synchronizes on, so storing ahead of it means an
        // overlay tick can never read a stale id for a fresh request.
        VIEWPORT_CONTEXT_NODE_ID.store(target.node_id, Ordering::Release);
        commands.insert_resource(target);

        // Same request for the overlay-thread shell, when one is running.
        if let Some(bridge) = bridge {
            bridge.lock().request_viewport_context_menu = Some((cursor.x, cursor.y));
        }
    }
}

/// Raycast through `cursor`, apply left-click selection semantics to the
/// frontmost hit, and describe the result.
///
/// Split out of the detector so the gesture bookkeeping above stays
/// readable; it borrows rather than owns every param so the caller keeps
/// its `SystemParam`s.
fn resolve_viewport_target(
    cursor: Vec2,
    keys: &ButtonInput<KeyCode>,
    cameras: &Query<(&Camera, &GlobalTransform)>,
    spatial_query: &avian3d::prelude::SpatialQuery,
    pick_q: &PickLookup,
    selection_manager: Option<&SelectionSyncManager>,
    mut explorer_state: Option<&mut crate::ui::slint_ui::UnifiedExplorerState>,
) -> ViewportContextTarget {
    let mut target = ViewportContextTarget {
        entity: None,
        hit_point: None,
        node_id: -1,
        // The caller stamps the click position once this returns — resolving
        // the target says nothing about whether the gesture was a click or an
        // orbit drag, and only a click should open the menu.
        open_request: None,
    };

    // Pick the main 3D camera (order=0) — same convention as every other
    // interaction system. Without it there's no ray and no target, but the
    // menu still opens (as an empty-space menu).
    let Some((camera, cam_transform)) = cameras.iter().find(|(c, _)| c.order == 0) else {
        return target;
    };
    let Ok(ray) = camera.viewport_to_world(cam_transform, cursor) else {
        return target;
    };

    // Physics raycast, verbatim from `modal_tool::run_active_modal_tool`
    // so right-click picks exactly what a modal tool would pick under the
    // same cursor. Avian 0.6's `prelude::Dir` alias is `pub(crate)` only,
    // hence reaching through `bevy::math::Dir3` (the same underlying type
    // `ray_hits` takes in 3d mode).
    let raw_hit = {
        use avian3d::prelude::SpatialQueryFilter;
        use bevy::math::Dir3;
        if let Ok(dir) = Dir3::new(*ray.direction) {
            let hits = spatial_query.ray_hits(
                ray.origin,
                dir,
                PICK_RAY_LENGTH,
                1,
                true,
                &SpatialQueryFilter::default(),
            );
            hits.first().map(|h| (h.entity, ray.origin + *ray.direction * h.distance))
        } else {
            None
        }
    };

    let Some((hit_entity, hit_point)) = raw_hit else {
        // Empty space. Fall back to where the ray crosses the ground
        // plane (y = 0) so `insert-here` still has somewhere to put
        // things; `None` when the ray is parallel to it or aimed away.
        if ray.direction.y.abs() > 1e-6 {
            let t = -ray.origin.y / ray.direction.y;
            if t > 0.0 {
                target.hit_point = Some(ray.origin + *ray.direction * t);
            }
        }
        return target;
    };
    target.hit_point = Some(hit_point);

    // The collider we struck may not be something the editor lets you
    // select. Mirror `part_selection.rs` exactly: abstract/container
    // classes are never click-selectable, locked parts are never
    // selectable, and an entity with no part identity at all is skipped.
    let Ok((part_entity, part_marker, instance, basepart, child_of)) = pick_q.get(hit_entity) else {
        return target;
    };

    if let Some(inst) = instance {
        match inst.class_name {
            ClassName::Folder
            | ClassName::Model
            | ClassName::ScreenGui
            | ClassName::Frame
            | ClassName::SoulScript
            | ClassName::Workspace
            | ClassName::Lighting
            | ClassName::Camera => return target,
            _ => {}
        }
    }

    if basepart.map(|bp| bp.locked).unwrap_or(false) {
        return target;
    }

    let entity_id = entity_to_id_string(hit_entity);
    let part_id = part_entity
        .map(|pe| pe.part_id.clone())
        .filter(|id| !id.is_empty())
        .or_else(|| part_marker.map(|pm| pm.part_id.clone()).filter(|id| !id.is_empty()))
        .or_else(|| instance.map(|_| entity_id));
    let Some(part_id) = part_id else { return target };

    target.entity = Some(hit_entity);

    // Promote to the parent Model unless Alt is held — the same rule
    // left-click uses, so right-click never selects at a different
    // granularity than a plain click on the same pixel would.
    let alt_pressed = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let parent_model = child_of.and_then(|c| {
        let parent = c.parent();
        match pick_q.get(parent) {
            Ok((_, _, Some(inst), _, _)) if inst.class_name == ClassName::Model => Some(parent),
            _ => None,
        }
    });

    let (selection_entity, selection_id) = if alt_pressed {
        (hit_entity, part_id)
    } else if let Some(model_entity) = parent_model {
        (model_entity, entity_to_id_string(model_entity))
    } else {
        (hit_entity, part_id)
    };

    if let Some(mgr) = selection_manager {
        let sel = mgr.0.write();
        // Standard file-manager behaviour: right-clicking something that
        // is already selected acts on the WHOLE selection, so five
        // selected parts stay five. Only an unselected target replaces
        // the selection.
        if !sel.is_selected(&selection_id) {
            sel.select(selection_id.clone());
            info!("[viewport-menu] selected '{}' under right-click", selection_id);

            // Mirror left-click's Explorer follow-through so the tree row
            // for the just-selected entity scrolls into view instead of
            // the menu acting on a row the user can't see.
            if let Some(es) = explorer_state.as_deref_mut() {
                es.pending_scroll_target_entity = Some(selection_entity);
                es.needs_immediate_sync = true;
            }
        }
    }

    // Reverse the Explorer's id → Entity map. It's a linear scan, but it
    // runs once per right-click (not per frame) and the map only holds
    // rows the Explorer has actually materialized.
    if let Some(es) = explorer_state.as_deref() {
        if let Some((id, _)) = es.entity_id_cache.iter().find(|(_, e)| **e == selection_entity) {
            target.node_id = *id;
        }
    }

    target
}

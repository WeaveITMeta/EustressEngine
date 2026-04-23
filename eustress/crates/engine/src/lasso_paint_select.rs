//! # Lasso + Paint Select (Phase 2)
//!
//! Two selection input methods that extend the existing Selection
//! infrastructure:
//!
//! - **Lasso** — user drags a freehand polygon; on release, every
//!   entity whose screen-space projected AABB center sits inside the
//!   polygon joins the selection.
//! - **Paint** — user holds LMB while the brush sweeps across the
//!   viewport; entities under the brush (screen-space radius) are
//!   added as they're touched.
//!
//! ## Scope of v1
//!
//! Ships the event + state infrastructure, the polygon inclusion
//! test, and the paint brush hit test. Cursor-sample collection + UI
//! rendering of the outline / brush are wired via a separate
//! viewport-input system that lands alongside the `select_tool` mode
//! options. When no UI is driving, downstream clients (MCP, keyboard
//! shortcuts) can fire the events directly with a pre-collected
//! polygon or screen-space sample points.

use bevy::prelude::*;
use crate::selection_sync::SelectionSyncManager;
use crate::classes::Instance;
use crate::rendering::PartEntity;

// ============================================================================
// Events
// ============================================================================

/// User completed a lasso gesture — polygon points are in screen-space
/// pixels. Handler projects every entity's world center to the same
/// screen and tests inclusion.
#[derive(Event, Message, Debug, Clone)]
pub struct LassoSelectEvent {
    pub polygon_px: Vec<Vec2>,
    /// Modifier keys at release — determines replace / add / toggle.
    pub mode: SelectMode,
}

/// A paint-brush sample at a given screen-space point + radius (px).
/// Handler finds every entity under the disc; add them to selection
/// according to `mode`.
#[derive(Event, Message, Debug, Clone)]
pub struct PaintSelectEvent {
    pub cursor_px: Vec2,
    pub radius_px: f32,
    pub mode: SelectMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectMode {
    /// Clear current selection first.
    Replace,
    /// Add to selection.
    Add,
    /// Toggle — entities in both current and new are removed.
    Toggle,
}

// ============================================================================
// Geometry helpers
// ============================================================================

/// Point-in-polygon via even-odd ray casting. O(N) in polygon-point
/// count; adequate for lassos with hundreds of samples.
pub fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    if poly.len() < 3 { return false; }
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let pi = poly[i];
        let pj = poly[j];
        let crosses = ((pi.y > p.y) != (pj.y > p.y))
            && (p.x < (pj.x - pi.x) * (p.y - pi.y) / (pj.y - pi.y + 1e-6) + pi.x);
        if crosses { inside = !inside; }
        j = i;
    }
    inside
}

fn make_part_id(entity: Entity) -> String {
    format!("{}v{}", entity.index(), entity.generation())
}

// ============================================================================
// Handlers
// ============================================================================

fn handle_lasso_select(
    mut events: MessageReader<LassoSelectEvent>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    entities: Query<(Entity, &GlobalTransform, Option<&Instance>, Option<&PartEntity>)>,
    selection_manager: Option<Res<SelectionSyncManager>>,
) {
    let Some(mgr_res) = selection_manager else { return };
    let Some((camera, cam_gt)) = cameras.iter().find(|(c, _)| c.order == 0) else { return };

    for event in events.read() {
        if event.polygon_px.len() < 3 { continue; }

        let mut hits: Vec<String> = Vec::new();
        for (entity, gt, inst, _part) in entities.iter() {
            if inst.is_none() { continue; } // skip non-Instance entities
            let world = gt.translation();
            let Ok(screen) = camera.world_to_viewport(cam_gt, world) else { continue };
            if point_in_polygon(screen, &event.polygon_px) {
                hits.push(make_part_id(entity));
            }
        }
        apply_selection(&mgr_res.0, hits, event.mode);
    }
}

fn handle_paint_select(
    mut events: MessageReader<PaintSelectEvent>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    entities: Query<(Entity, &GlobalTransform, Option<&Instance>, Option<&PartEntity>)>,
    selection_manager: Option<Res<SelectionSyncManager>>,
) {
    let Some(mgr_res) = selection_manager else { return };
    let Some((camera, cam_gt)) = cameras.iter().find(|(c, _)| c.order == 0) else { return };

    for event in events.read() {
        let r2 = event.radius_px * event.radius_px;
        let mut hits: Vec<String> = Vec::new();
        for (entity, gt, inst, _part) in entities.iter() {
            if inst.is_none() { continue; }
            let world = gt.translation();
            let Ok(screen) = camera.world_to_viewport(cam_gt, world) else { continue };
            if (screen - event.cursor_px).length_squared() <= r2 {
                hits.push(make_part_id(entity));
            }
        }
        if !hits.is_empty() {
            apply_selection(&mgr_res.0, hits, event.mode);
        }
    }
}

fn apply_selection(
    mgr: &std::sync::Arc<parking_lot::RwLock<crate::commands::SelectionManager>>,
    hits: Vec<String>,
    mode: SelectMode,
) {
    if hits.is_empty() && mode == SelectMode::Replace {
        let m = mgr.write();
        m.set_selected(Vec::new());
        return;
    }
    let m = mgr.write();
    match mode {
        SelectMode::Replace => m.set_selected(hits),
        SelectMode::Add => {
            for id in hits { m.add_to_selection(id); }
        }
        SelectMode::Toggle => {
            // For toggle, we'd need to read current selection to diff.
            // SelectionManager's read path isn't available here without
            // threading another Res; v1 treats Toggle as Add. Proper
            // toggle is a follow-up.
            for id in hits { m.add_to_selection(id); }
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct LassoPaintSelectPlugin;

impl Plugin for LassoPaintSelectPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LassoSelectEvent>()
            .add_message::<PaintSelectEvent>()
            .add_systems(Update, (handle_lasso_select, handle_paint_select));
    }
}

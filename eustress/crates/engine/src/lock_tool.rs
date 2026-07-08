//! Lock / Unlock tools.
//!
//! - **Lock** is a POINTER tool: while it is active, the part under the cursor
//!   is highlighted (a wireframe box, colored by its current lock state) and a
//!   left-click TOGGLES that part's `BasePart.locked` flag. The tool stays
//!   active so you can keep pointing at parts and flipping them.
//! - **Unlock** is a one-shot "Unlock All": pressing it unlocks EVERY instance
//!   in the Space at once, then returns to the Select tool. The ribbon button
//!   is labelled "Unlock All".
//!
//! Why Lock is a pointer tool rather than "lock the current selection":
//! the Select tool's hit-test deliberately EXCLUDES locked parts (so they
//! don't intercept clicks / box-selects that drag around them). That makes an
//! accidentally-locked part impossible to click through normal selection. The
//! Lock tool's hover + toggle (and, as the sledgehammer, Unlock All) are the
//! way back to "this part is editable again" without round-tripping through
//! the Properties panel.
//!
//! Press Escape to return to the Select tool. Both are mutually exclusive with
//! the standard transform tools (Move/Rotate/Scale).

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use eustress_common::classes::BasePart;
use crate::ui::{StudioState, Tool, SlintUIFocus};

pub struct LockToolPlugin;

impl Plugin for LockToolPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            unlock_all_on_entry,
            lock_tool_toggle_click,
            lock_tool_hover_highlight,
            cancel_on_escape,
        ).chain());
    }
}

/// Wireframe-box hover color for an UNLOCKED part (cyan accent — "click to
/// lock"). Matches the editor's cyan selection accent.
const HOVER_UNLOCKED: Color = Color::srgb(0.0, 0.737, 0.831); // ~#00bcd4
/// Wireframe-box hover color for a LOCKED part (amber — "click to unlock").
const HOVER_LOCKED: Color = Color::srgb(1.0, 0.76, 0.03); // ~#ffc107

/// Cast a ray from the cursor into the scene and return the closest part hit
/// (LOCKED parts INCLUDED — the whole point of this tool is to target them).
/// Returns `None` when the cursor is over a UI panel, a Slint input has focus,
/// no camera exists, or nothing is under the cursor. Uses the Avian spatial
/// query (collider BVH) so it stays O(log n) even on a 100K+-part Space — the
/// hover path runs every frame, so a linear scan of all parts would be far too
/// slow here.
fn cursor_part(
    ui_focus: &Option<Res<SlintUIFocus>>,
    viewport_bounds: &Option<Res<crate::ui::ViewportBounds>>,
    windows: &Query<&Window, With<PrimaryWindow>>,
    cameras: &Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    spatial_query: &SpatialQuery,
) -> Option<Entity> {
    if ui_focus.as_ref().map(|f| f.has_focus || f.text_input_focused).unwrap_or(false) {
        return None;
    }
    let window = windows.single().ok()?;
    let cursor_pos = window.cursor_position()?;
    if let Some(vb) = viewport_bounds.as_deref() {
        let scale = window.scale_factor() as f32;
        if !vb.contains_logical(cursor_pos, scale) { return None; }
    }
    let (camera, camera_transform) = cameras.iter().find(|(c, _)| c.order == 0)?;
    let ray = camera.viewport_to_world(camera_transform, cursor_pos).ok()?;
    let dir = Dir3::new(*ray.direction).ok()?;
    // `ray_hits` is NOT guaranteed sorted; gather a handful and pick the
    // nearest (same pattern as `interaction::click::cursor_ray_hit`).
    let hits = spatial_query.ray_hits(
        ray.origin, dir, 10_000.0, 20, true, &SpatialQueryFilter::default(),
    );
    hits.iter()
        .min_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(std::cmp::Ordering::Equal))
        .map(|h| h.entity)
}

/// While the Lock tool is active, draw a wireframe box around the part under
/// the cursor so the user can see exactly what a click will flip — cyan for an
/// unlocked part (click → lock), amber for a locked one (click → unlock).
fn lock_tool_hover_highlight(
    studio_state: Option<Res<StudioState>>,
    ui_focus: Option<Res<SlintUIFocus>>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    parts: Query<(&GlobalTransform, &BasePart)>,
    spatial_query: SpatialQuery,
    mut gizmos: Gizmos,
) {
    let Some(state) = studio_state else { return };
    if state.current_tool != Tool::Lock { return; }

    let Some(entity) = cursor_part(&ui_focus, &viewport_bounds, &windows, &cameras, &spatial_query)
    else { return };
    let Ok((gt, bp)) = parts.get(entity) else { return };

    let t = gt.compute_transform();
    let color = if bp.locked { HOVER_LOCKED } else { HOVER_UNLOCKED };
    draw_wire_box(&mut gizmos, t.translation, t.rotation, bp.size, color);
}

/// Draw a 12-edge wireframe box of `size` (full extents) centered at `center`
/// with orientation `rotation`. Uses `gizmos.line` — the immediate-mode API
/// already used across the editor (grid, move-tool axes) — rather than the
/// 0.19 `primitive_3d(&Cuboid, ..)` path, to keep the trait/`Isometry3d`
/// surface out of it.
fn draw_wire_box(gizmos: &mut Gizmos, center: Vec3, rotation: Quat, size: Vec3, color: Color) {
    let h = (size * 0.5).max(Vec3::splat(0.01));
    let local = [
        Vec3::new( h.x,  h.y,  h.z),
        Vec3::new(-h.x,  h.y,  h.z),
        Vec3::new(-h.x, -h.y,  h.z),
        Vec3::new( h.x, -h.y,  h.z),
        Vec3::new( h.x,  h.y, -h.z),
        Vec3::new(-h.x,  h.y, -h.z),
        Vec3::new(-h.x, -h.y, -h.z),
        Vec3::new( h.x, -h.y, -h.z),
    ];
    let c: [Vec3; 8] = core::array::from_fn(|i| center + rotation * local[i]);
    // Top face, bottom face, then the four vertical edges.
    const EDGES: [(usize, usize); 12] = [
        (0, 1), (1, 2), (2, 3), (3, 0),
        (4, 5), (5, 6), (6, 7), (7, 4),
        (0, 4), (1, 5), (2, 6), (3, 7),
    ];
    for (a, b) in EDGES {
        gizmos.line(c[a], c[b], color);
    }
}

/// Left-click while the Lock tool is active TOGGLES the locked flag of the
/// part under the cursor (locked ⇄ unlocked). The tool stays active so the
/// user can keep flipping parts.
fn lock_tool_toggle_click(
    mouse: Res<ButtonInput<MouseButton>>,
    studio_state: Option<Res<StudioState>>,
    ui_focus: Option<Res<SlintUIFocus>>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut parts: Query<(&mut BasePart, &Name)>,
    instance_files: Query<&crate::space::instance_loader::InstanceFile>,
    spatial_query: SpatialQuery,
    auth: Option<Res<crate::auth::AuthState>>,
) {
    let Some(state) = studio_state else { return };
    if state.current_tool != Tool::Lock { return; }
    if !mouse.just_pressed(MouseButton::Left) { return; }

    let Some(entity) = cursor_part(&ui_focus, &viewport_bounds, &windows, &cameras, &spatial_query)
    else { return };
    let Ok((mut bp, name)) = parts.get_mut(entity) else { return };

    let now_locked = !bp.locked;
    bp.locked = now_locked;
    info!(
        "{} '{}' is now {}",
        if now_locked { "🔒" } else { "🔓" },
        name.as_str(),
        if now_locked { "locked" } else { "unlocked" },
    );
    persist_locked(entity, now_locked, &instance_files, &auth);
}

/// Pressing the Unlock tool is a one-shot "Unlock All": every part in the
/// Space is unlocked in a single action, then we return to Select. This is the
/// escape hatch when many parts (or parts with no collider to click) are
/// locked. Fires only on the frame the tool is entered.
fn unlock_all_on_entry(
    mut studio_state: Option<ResMut<StudioState>>,
    mut all_parts: Query<(Entity, &mut BasePart, &Name)>,
    instance_files: Query<&crate::space::instance_loader::InstanceFile>,
    auth: Option<Res<crate::auth::AuthState>>,
    mut prev_tool: Local<Tool>,
) {
    let Some(ref mut state) = studio_state else { return };
    let current = state.current_tool;
    let previous = *prev_tool;
    *prev_tool = current;

    // Only fire on the frame we ENTER Unlock mode.
    if current != Tool::Unlock || previous == Tool::Unlock { return; }

    let mut count = 0u32;
    for (entity, mut bp, name) in all_parts.iter_mut() {
        if !bp.locked { continue; }
        bp.locked = false;
        count += 1;
        // Keep the per-part log at debug — on a huge Space an INFO per part is
        // a log-I/O stall; the aggregate below is the signal.
        debug!("🔓 '{}' unlocked (Unlock All)", name.as_str());
        persist_locked(entity, false, &instance_files, &auth);
    }
    info!("🔓 Unlock All: unlocked {} part(s) in the Space", count);

    // One-shot action — return to Select.
    state.current_tool = Tool::Select;
}

/// Persist a part's new `locked` state to its `_instance.toml` (signed) when it
/// has a disk file. Binary-ECS parts (no `InstanceFile`) keep the change in the
/// live ECS for the session; their persisted store is written through the
/// normal save path, not here.
fn persist_locked(
    entity: Entity,
    locked: bool,
    instance_files: &Query<&crate::space::instance_loader::InstanceFile>,
    auth: &Option<Res<crate::auth::AuthState>>,
) {
    let Ok(inst_file) = instance_files.get(entity) else { return };
    let stamp = auth.as_deref().and_then(crate::space::instance_loader::current_stamp);
    if let Ok(mut def) = crate::space::instance_loader::load_instance_definition(&inst_file.toml_path) {
        def.properties.locked = locked;
        let _ = crate::space::instance_loader::write_instance_definition_signed(
            &inst_file.toml_path, &mut def, stamp.as_ref(),
        );
    }
}

/// Escape exits Lock/Unlock back to Select. (Unlock returns to Select on its
/// own after firing, but a user who opened it and changed their mind before it
/// ran still gets out cleanly.)
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

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use crate::rendering::PartEntity;
use eustress_common::default_scene::PartEntityMarker;
use crate::classes::{Instance, BasePart};
use crate::selection_box::{Hovered, Selected};
use crate::math_utils::ray_obb_intersection;
use crate::entity_utils::entity_to_id_string;

/// Bevy message emitted when the user double-clicks a part in the
/// 3D viewport. Consumers — currently only the billboard text-edit
/// mode in `billboard_gui.rs` — read this to react to the
/// just-double-clicked entity. Detection lives inside
/// [`part_selection_system`] (same raycast result powers both single
/// and double click) so we don't waste a second raycast per frame.
#[derive(bevy::ecs::message::Message, Debug, Clone, Copy)]
pub struct DoubleClickedPart {
    pub entity: Entity,
}

/// Per-system click history used to detect double-clicks. Stored as
/// a `Local<>` on `part_selection_system` so it doesn't pollute the
/// world's resource set. 400 ms is the standard double-click window.
#[derive(Default)]
pub struct DoubleClickTracker {
    pub last_at: Option<std::time::Instant>,
    pub last_entity: Option<Entity>,
}

#[cfg(not(target_arch = "wasm32"))]
use crate::rendering::BevySelectionManager;

/// How a viewport click combines with the existing selection.
///
/// Mirrors the Explorer's modifier model (`slint_ui.rs`) so the tree and
/// the viewport never disagree about what a modifier means. Toggle alone
/// is not enough for a refine pass: the user has to already know an
/// item's membership before the click lands, which is exactly what they
/// can't see mid-sweep across a marquee result. Add and Subtract are
/// stateless, so a whole sweep of clicks is predictable from the modifier
/// alone.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SelectionCombine {
    /// No modifier — replace the selection with just this entity.
    Replace,
    /// Ctrl — flip this entity's membership.
    Toggle,
    /// Shift — add, never remove.
    Add,
    /// Ctrl+Shift — remove, never add.
    Subtract,
}

/// System for left-click part selection with raycasting (Modern ECS)
/// Tool-state bundle for `part_selection_system` — groups the three
/// active-tool resources so the outer system stays under Bevy's
/// 16-parameter soft limit. Bevy unwraps this into the individual
/// resource reads via the `SystemParam` derive.
#[cfg(not(target_arch = "wasm32"))]
#[derive(bevy::ecs::system::SystemParam)]
pub struct PartSelectionToolStates<'w> {
    pub move_state:   Option<Res<'w, crate::move_tool::MoveToolState>>,
    pub scale_state:  Option<Res<'w, crate::scale_tool::ScaleToolState>>,
    pub rotate_state: Option<Res<'w, crate::rotate_tool::RotateToolState>>,
    pub studio_state: Option<Res<'w, crate::ui::StudioState>>,
}

/// Click-extras bundle: double-click tracker + message writer +
/// attributes lookup. Kept as a single `SystemParam` so the parent
/// [`part_selection_system`] stays under Bevy's 16-tuple soft limit.
#[cfg(not(target_arch = "wasm32"))]
#[derive(bevy::ecs::system::SystemParam)]
pub struct PartClickExtras<'w, 's> {
    pub dbl_tracker: Local<'s, DoubleClickTracker>,
    pub dbl_writer:  MessageWriter<'w, DoubleClickedPart>,
    /// Queried so the Ctrl+Alt+Click handler can look up a `Link`
    /// attribute on the hit entity and open it in the OS default
    /// browser without going through the selection path.
    pub attributes_q: Query<'w, 's, &'static eustress_common::attributes::Attributes>,
    /// Render-Aabb lookup for the OBB hit-test fallback. Gaussian Splat
    /// clouds have no BasePart / Mesh3d / Collider — their only bounds
    /// are the `Aabb` bevy_gaussian_splatting computes and inserts.
    /// Lives in this bundle so the parent system stays under Bevy's
    /// 16-param ceiling.
    pub aabb_q: Query<'w, 's, &'static bevy::camera::primitives::Aabb>,
    /// Which entities have an Avian collider. The OBB hit-test pass runs for
    /// EVERY selectable part now (not just when the physics raycast missed
    /// entirely), so parts with NO collider — e.g. an imported part whose
    /// mirrored/degenerate mesh scale made a valid collider impossible — are
    /// still clickable even when a collider'd part sits behind them. Collider'd
    /// parts are skipped in that pass because the physics raycast already hit
    /// them precisely; re-testing their (looser) OBB could wrongly grab one in
    /// front of the part the ray actually strikes.
    pub collider_q: Query<'w, 's, (), With<avian3d::prelude::Collider>>,
}

/// The query every viewport pick walks: anything that can be clicked in
/// edit mode. Shared by the click (`part_selection_system`) and the hover
/// preview (`hover_highlight_system`) so both resolve the same entity.
pub type PickQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        Option<&'static PartEntity>,
        Option<&'static PartEntityMarker>,
        Option<&'static Instance>,
        &'static GlobalTransform,
        Option<&'static Mesh3d>,
        Option<&'static BasePart>,
        Option<&'static ChildOf>,
    ),
    Or<(
        With<PartEntityMarker>,
        With<PartEntity>,
        (With<BasePart>, With<Instance>),
        (With<Instance>, With<bevy::camera::primitives::Aabb>, Without<Mesh3d>),
    )>,
>;

/// What a viewport pick resolved to.
pub struct PickHit {
    /// The part (or splat cloud) under the ray.
    pub entity: Entity,
    /// The Model a plain click selects instead of the part, when the part
    /// sits directly inside one.
    pub parent_model: Option<Entity>,
    pub distance: f32,
}

/// When to run the oriented-bounding-box test on top of the collider raycast.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ObbFallback {
    /// Test every candidate: catches parts whose collider is stale or absent.
    /// The click uses this; it runs once per click.
    Always,
    /// Only when the collider raycast found nothing. The hover preview uses
    /// this because it runs on every cursor move.
    OnColliderMiss,
}

/// Find the selectable part under `ray`.
///
/// Containers (Model, Folder, services), adornments and locked parts are
/// never hit. The collider raycast answers first; the OBB test covers parts
/// with no collider (splat clouds, mesh parts still loading) and, for the
/// click, parts whose collider does not match their mesh.
pub fn pick_under_cursor(
    ray: &Ray3d,
    parts: &PickQuery,
    parent_query: &Query<&Instance>,
    spatial_query: &avian3d::prelude::SpatialQuery,
    aabb_q: &Query<&bevy::camera::primitives::Aabb>,
    fallback: ObbFallback,
) -> Option<PickHit> {
    use avian3d::prelude::SpatialQueryFilter;

    // entity -> the Model a plain click resolves it to
    let mut candidates: std::collections::HashMap<Entity, Option<Entity>> =
        std::collections::HashMap::new();
    for (entity, part_entity, part_entity_marker, instance, _transform, _mesh, basepart, child_of) in parts.iter() {
        if let Some(inst) = instance {
            match inst.class_name {
                crate::classes::ClassName::Folder
                | crate::classes::ClassName::Model
                | crate::classes::ClassName::ScreenGui
                | crate::classes::ClassName::Frame
                | crate::classes::ClassName::SoulScript
                | crate::classes::ClassName::Workspace
                | crate::classes::ClassName::Lighting
                | crate::classes::ClassName::Camera => continue,
                _ => {}
            }
        }
        if part_entity.is_none() && part_entity_marker.is_none() && instance.is_none() {
            continue;
        }
        if let Some(bp) = basepart {
            if bp.locked {
                continue;
            }
        }
        let parent_model = child_of.and_then(|c| {
            let parent = c.parent();
            parent_query
                .get(parent)
                .ok()
                .filter(|pi| pi.class_name == crate::classes::ClassName::Model)
                .map(|_| parent)
        });
        candidates.insert(entity, parent_model);
    }

    let mut closest: Option<(Entity, f32)> = None;
    if let Ok(dir) = Dir3::new(*ray.direction) {
        spatial_query.ray_hits_callback(
            ray.origin,
            dir,
            10000.0,
            true,
            &SpatialQueryFilter::default(),
            |hit| {
                if candidates.contains_key(&hit.entity)
                    && closest.map_or(true, |(_, d)| hit.distance < d)
                {
                    closest = Some((hit.entity, hit.distance));
                }
                true
            },
        );
    }

    if fallback == ObbFallback::Always || closest.is_none() {
        for (entity, _pe, _pem, _inst, transform, _mesh, basepart, _child_of) in parts.iter() {
            if !candidates.contains_key(&entity) {
                continue;
            }
            let t = transform.compute_transform();
            let (obb_center, size) = if let Some(bp) = basepart {
                (t.translation, bp.size)
            } else if let Ok(aabb) = aabb_q.get(entity) {
                let world_center = t.translation + t.rotation * (Vec3::from(aabb.center) * t.scale);
                (world_center, Vec3::from(aabb.half_extents) * 2.0 * t.scale)
            } else {
                (t.translation, t.scale)
            };
            // ray_obb_intersection takes HALF-extents, not full size
            if let Some(distance) =
                ray_obb_intersection(ray.origin, *ray.direction, obb_center, size * 0.5, t.rotation)
            {
                if closest.map_or(true, |(_, d)| distance < d) {
                    closest = Some((entity, distance));
                }
            }
        }
    }

    closest.map(|(entity, distance)| PickHit {
        entity,
        parent_model: candidates.get(&entity).copied().flatten(),
        distance,
    })
}

/// Where the hover preview last looked, so a still cursor costs nothing.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct HoverProbe {
    cursor: Option<Vec2>,
    target: Option<Entity>,
}

/// Outline whatever a click would select, before the click.
///
/// Runs the same pick as the click on every cursor move while the Select
/// tool is active and nothing is being dragged, and marks the result
/// `Hovered`; `selection_box` draws the outline and removes it when the
/// marker goes. Holding Alt previews the part instead of its Model, exactly
/// as Alt+click selects it. An already selected target gets no hover mark,
/// so a selected part never wears two outlines.
#[cfg(not(target_arch = "wasm32"))]
pub fn hover_highlight_system(
    mouse_button: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform, &Projection)>,
    part_entities_query: PickQuery,
    parent_query: Query<&Instance>,
    aabb_q: Query<&bevy::camera::primitives::Aabb>,
    hovered: Query<Entity, With<Hovered>>,
    selected: Query<(), With<Selected>>,
    tool_states: PartSelectionToolStates,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    (ui_focus, play_state): (
        Option<Res<crate::ui::SlintUIFocus>>,
        Option<Res<State<crate::play_mode::PlayModeState>>>,
    ),
    spatial_query: avian3d::prelude::SpatialQuery,
    mut commands: Commands,
    mut probe: Local<HoverProbe>,
) {
    let PartSelectionToolStates { move_state, scale_state, rotate_state, studio_state } = tool_states;
    // No hover outline during a Play session; the path below clears one left
    // over from editing.
    let select_tool = crate::play_mode::editor_input_enabled(play_state)
        && studio_state
            .as_ref()
            .map(|s| s.current_tool == crate::ui::Tool::Select)
            .unwrap_or(true);
    let dragging = mouse_button.pressed(MouseButton::Left)
        || move_state.as_ref().and_then(|s| s.dragged_axis).is_some()
        || scale_state.as_ref().and_then(|s| s.dragged_axis).is_some()
        || rotate_state.as_ref().and_then(|s| s.dragged_axis).is_some();
    let over_ui = ui_focus.as_ref().map(|f| f.has_focus).unwrap_or(false);

    let cursor = windows.single().ok().and_then(|w| {
        let pos = w.cursor_position()?;
        let inside = viewport_bounds
            .as_ref()
            .map(|vb| vb.contains_logical(pos, w.scale_factor() as f32))
            .unwrap_or(true);
        inside.then_some(pos)
    });

    let alt_edge = keys.just_pressed(KeyCode::AltLeft)
        || keys.just_pressed(KeyCode::AltRight)
        || keys.just_released(KeyCode::AltLeft)
        || keys.just_released(KeyCode::AltRight);

    let target: Option<Entity> = match cursor {
        Some(pos) if select_tool && !dragging && !over_ui => {
            if probe.cursor == Some(pos) && !alt_edge {
                // Nothing moved: keep the current target unless it vanished.
                match probe.target {
                    Some(t) if hovered.contains(t) => return,
                    Some(_) => None,
                    None => return,
                }
            } else {
                probe.cursor = Some(pos);
                let camera = camera_query.iter().find(|(c, _, _)| c.order == 0);
                let ray = camera.and_then(|(c, tf, _)| c.viewport_to_world(tf, pos).ok());
                let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
                ray.and_then(|ray| {
                    pick_under_cursor(
                        &ray,
                        &part_entities_query,
                        &parent_query,
                        &spatial_query,
                        &aabb_q,
                        ObbFallback::OnColliderMiss,
                    )
                })
                .map(|hit| if alt { hit.entity } else { hit.parent_model.unwrap_or(hit.entity) })
                .filter(|e| !selected.contains(*e))
            }
        }
        _ => {
            probe.cursor = None;
            None
        }
    };

    if target == probe.target && target.map_or(true, |t| hovered.contains(t)) {
        return;
    }
    for e in hovered.iter() {
        commands.entity(e).remove::<Hovered>();
    }
    if let Some(t) = target {
        commands.entity(t).insert(Hovered);
    }
    probe.target = target;
}

/// Supports both PartEntity (legacy) and Instance (modern) components
#[cfg(not(target_arch = "wasm32"))]
pub fn part_selection_system(
    mouse_button: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform, &Projection)>,
    // Query selectable parts: must have PartEntityMarker OR (Instance + BasePart) so Folders,
    // Services, Scripts, and UI entities are excluded from raycasting entirely.
    // The same `PickQuery` the hover system uses: `pick_under_cursor` takes
    // `&PickQuery`, and `Query` is invariant in its data tuple, so an
    // expanded copy with elided lifetimes here does not unify with it.
    part_entities_query: PickQuery,
    // Query for children to calculate accurate group bounds (matching move_tool.rs)
    children_query: Query<&Children>,
    // Query for child transforms/baseparts
    child_transform_query: Query<(&GlobalTransform, Option<&BasePart>)>,
    // Query for SELECTED entities (for tool handle checks) - Matches tool rendering logic
    selected_query: Query<(Entity, &GlobalTransform, Option<&BasePart>), With<Selected>>,
    // Query to check if a parent entity is a Model
    parent_query: Query<&Instance>,
    selection_manager: Option<Res<BevySelectionManager>>,
    tool_states: PartSelectionToolStates,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    // `_ui_focus` is retained so the gate can be reinstated per-widget later;
    // selection itself is deliberately UI-blind (see the note above the
    // viewport-bounds check). Paired with the Play state in one tuple: this
    // system is at Bevy's 16-parameter ceiling.
    (_ui_focus, play_state): (
        Option<Res<crate::ui::SlintUIFocus>>,
        Option<Res<State<crate::play_mode::PlayModeState>>>,
    ),
    spatial_query: avian3d::prelude::SpatialQuery,
    // Used to signal "scroll the Explorer to the just-selected entity"
    // when a single-click happens in the 3D viewport. Multi-select
    // (Ctrl/Shift) deliberately doesn't scroll — it'd be jarring as
    // each accumulating click whips the tree around.
    mut explorer_state: Option<ResMut<crate::ui::slint_ui::UnifiedExplorerState>>,
    // Click extras: double-click tracker + writer + attributes query
    // (Ctrl+Alt+Click follows an entity's `Link` attribute).
    mut click_extras: PartClickExtras,
) {
    // During a Play session the mouse belongs to the game: a click that
    // shoots must not select (or double-click-edit) the scene.
    if !crate::play_mode::editor_input_enabled(play_state) {
        return;
    }
    // Re-expose the bundle fields under their pre-bundle names so the
    // body below needs no further edits. Each field is already the
    // exact `Option<Res<_>>` the body expects.
    let PartSelectionToolStates { move_state, scale_state, rotate_state, studio_state } = tool_states;
    // Transform mode governs whether the Move gizmo is axis-aligned to
    // world (IDENTITY) or rotated to match the active entity. Hit test
    // must use the same rotation or clicking the rotated handle fails.
    let transform_mode = studio_state
        .as_ref()
        .map(|s| s.transform_mode)
        .unwrap_or(crate::ui::TransformMode::World);
    let Some(selection_manager) = selection_manager else { return };
    let move_active = move_state.as_ref().map(|s| s.active).unwrap_or(false);
    let scale_active = scale_state.as_ref().map(|s| s.active).unwrap_or(false);
    let rotate_active = rotate_state.as_ref().map(|s| s.active).unwrap_or(false);
    let move_dragging = move_state.as_ref().and_then(|s| s.dragged_axis).is_some();
    let scale_dragging = scale_state.as_ref().and_then(|s| s.dragged_axis).is_some();
    let rotate_dragging = rotate_state.as_ref().and_then(|s| s.dragged_axis).is_some();
    // Only trigger on left click press
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }

    debug!("[select] LEFT CLICK detected — processing selection");

    // Selection deliberately ignores UI hit-state. The ONLY thing that gates a
    // viewport click is geometry: is the cursor inside the 3D viewport rect
    // (checked below).
    //
    // Two gates used to live here and both cost more than they bought:
    //
    // * `SlintUIFocus.has_focus` — set as `!in_viewport`, so it duplicated the
    //   `ViewportBounds` test below while being computed in a DIFFERENT system
    //   on a DIFFERENT frame. Any lag between the two produced a click that the
    //   geometry accepted and the flag rejected: selection that failed at
    //   random with nothing on screen to explain it.
    //
    // * `gui_element_hit` — a screen-space rect test over every entity carrying
    //   `GuiElementDisplay`. That component is shared by ScreenGui AND by
    //   BillboardGui/SurfaceGui, whose rects resolve against their own canvas,
    //   not the viewport — so every billboard label in the Space projected a
    //   phantom blocking rect near the viewport origin.
    //
    // Double-clicking a billboard to edit its text still works: that path is
    // driven by `DoubleClickedPart`, which this very system emits further down,
    // so it rides on selection rather than competing with it.
    //
    // Consequence to keep in mind: a click on a ScreenGui button now ALSO
    // reaches 3D selection. That is the intended trade for now — a part
    // selected behind a HUD is a visible, recoverable annoyance, whereas a
    // click silently vanishing is not.

    // Check if click is within the viewport bounds (not on UI panels)
    let window = match windows.single() {
        Ok(w) => w,
        Err(_) => return,
    };

    let cursor_position = match window.cursor_position() {
        Some(pos) => pos,
        None => return,
    };

    // Block selection if click is outside the 3D viewport area.
    // `ViewportBounds` is stored in PHYSICAL pixels but `cursor_position`
    // is LOGICAL, so we go through the contains_logical helper to avoid
    // the DPI-scale bug that silently rejected every click on any display
    // with scale_factor ≠ 1.0.
    if let Some(vb) = viewport_bounds.as_ref() {
        let scale = window.scale_factor() as f32;
        if !vb.contains_logical(cursor_position, scale) {
            trace!("[select] blocked — cursor outside viewport");
            return;
        }
    } else {
        trace!("[select] no ViewportBounds");
    }
    
    // Selection modifiers. Ctrl toggles, Shift only ever grows the
    // selection, Ctrl+Shift only ever shrinks it — see [`SelectionCombine`]
    // for why plain toggle isn't sufficient on its own.
    //
    // Alt is ORTHOGONAL to all three: it chooses WHAT the click resolves to
    // (the individual part rather than its parent Model), not how that
    // result merges with the selection, so it composes with every branch
    // below.
    let shift_pressed = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let ctrl_pressed = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let alt_pressed = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let combine = match (ctrl_pressed, shift_pressed) {
        (false, false) => SelectionCombine::Replace,
        (true, false) => SelectionCombine::Toggle,
        (false, true) => SelectionCombine::Add,
        (true, true) => SelectionCombine::Subtract,
    };

    // Find the main 3D camera (order=0) — there may be multiple cameras (e.g. Slint overlay at order=100)
    let (camera, camera_transform, projection) = match camera_query.iter().find(|(c, _, _)| c.order == 0) {
        Some(ct) => ct,
        None => return,
    };
    
    // Convert screen space to ray
    let ray = match camera.viewport_to_world(camera_transform, cursor_position) {
        Ok(r) => r,
        Err(_) => return,
    };
    
    // Raycast against all part entities
    let mut closest_hit: Option<(String, f32, Entity, Option<Entity>)> = None;
    
    // PRIORITY CHECK: Check if we are clicking on a tool handle BEFORE checking for part hits
    // This ensures handles are always clickable even if a part is behind them
    
    // Check Move Tool handles
    if move_active {
        if !selected_query.is_empty() {
            let mut bounds_min = Vec3::splat(f32::MAX);
            let mut bounds_max = Vec3::splat(f32::MIN);
            let mut count = 0;
            
            for (entity, global_transform, basepart) in selected_query.iter() {
                let t = global_transform.compute_transform();
                
                // Use calculated AABB for accurate center (matches MoveTool logic)
                let size = basepart.map(|bp| bp.size).unwrap_or(t.scale);
                let half_size = size * 0.5;
                let (part_min, part_max) = crate::math_utils::calculate_rotated_aabb(t.translation, half_size, t.rotation);
                
                bounds_min = bounds_min.min(part_min);
                bounds_max = bounds_max.max(part_max);
                count += 1;
                
                // Include children in bounds calculation (CRITICAL for Models)
                if let Ok(children) = children_query.get(entity) {
                    for child in children.iter() {
                        if let Ok((child_global, child_bp)) = child_transform_query.get(child) {
                            let child_t = child_global.compute_transform();
                            let child_size = child_bp.map(|bp| bp.size).unwrap_or(child_t.scale);
                            let child_half = child_size * 0.5;
                            let (c_min, c_max) = crate::math_utils::calculate_rotated_aabb(child_t.translation, child_half, child_t.rotation);
                            
                            bounds_min = bounds_min.min(c_min);
                            bounds_max = bounds_max.max(c_max);
                            count += 1;
                        }
                    }
                }
            }
            
            if count > 0 {
                let center = (bounds_min + bounds_max) * 0.5;

                // Sized by `move_tool::camera_scale_factor` — the same function
                // `move_handles` uses to lay out the meshes the user actually
                // sees and clicks. This used to restate the formula inline under
                // a "MUST match ... exactly!" comment; a comment cannot enforce
                // that, and the sibling guard in the deselect branch below had
                // already drifted to an entirely different one.
                let fov = crate::camera_controller::gizmo_fov(
                    projection,
                    camera_transform.translation(),
                    center,
                );
                let handle_length = crate::move_tool::camera_scale_factor(
                    camera_transform.translation(),
                    center,
                    fov,
                );

                let gizmo_rotation = crate::move_tool::gizmo_rotation_for(
                    transform_mode,
                    selected_query.iter().map(|(_, gt, _)| gt.compute_transform().rotation),
                );
                if crate::move_tool::is_clicking_move_handle(
                    &ray, center, Vec3::ONE, handle_length, &camera_transform, gizmo_rotation,
                ) {
                    return; // Clicking move handle, abort selection
                }
            }
        }
    }
    
    // Check Rotate Tool handles
    if rotate_active {
        // Compute combined bounding box of all selected entities (matching rotate_tool.rs)
        let mut rot_bmin = Vec3::splat(f32::MAX);
        let mut rot_bmax = Vec3::splat(f32::MIN);
        let mut rot_count = 0;
        for (entity, global_transform, basepart) in selected_query.iter() {
            let t = global_transform.compute_transform();
            let size = basepart.map(|bp| bp.size).unwrap_or(t.scale);
            let (mn, mx) = crate::math_utils::calculate_rotated_aabb(t.translation, size * 0.5, t.rotation);
            rot_bmin = rot_bmin.min(mn);
            rot_bmax = rot_bmax.max(mx);
            rot_count += 1;
            
            // Include children in bounds (matching rotate_tool.rs)
            if let Ok(children) = children_query.get(entity) {
                for child in children.iter() {
                    if let Ok((child_global, child_bp)) = child_transform_query.get(child) {
                        let child_t = child_global.compute_transform();
                        let child_size = child_bp.map(|bp| bp.size).unwrap_or(child_t.scale);
                        let (c_min, c_max) = crate::math_utils::calculate_rotated_aabb(child_t.translation, child_size * 0.5, child_t.rotation);
                        rot_bmin = rot_bmin.min(c_min);
                        rot_bmax = rot_bmax.max(c_max);
                        rot_count += 1;
                    }
                }
            }
        }
        if rot_count > 0 {
            let rot_center = (rot_bmin + rot_bmax) * 0.5;
            let rot_extent = rot_bmax - rot_bmin;
            // Use same radius calculation as rotate_tool.rs
            let radius = crate::rotate_tool::compute_ring_radius(rot_center, rot_extent, &camera_transform, projection);
            let rotate_rotation = crate::move_tool::gizmo_rotation_for(
                transform_mode,
                selected_query.iter().map(|(_, gt, _)| gt.compute_transform().rotation),
            );
            if crate::rotate_tool::is_clicking_rotate_handle(&ray, rot_center, radius, &camera_transform, rotate_rotation) {
                return; // Clicking rotate handle, abort selection
            }
        }
    }
    
    // Check Scale Tool handles
    if scale_active {
        // Group-level scale-handle check — matches the group-aware
        // `scale_handles` layout. One test replaces the N per-part tests
        // that used to run.
        let mut scale_bmin = Vec3::splat(f32::MAX);
        let mut scale_bmax = Vec3::splat(f32::MIN);
        let mut scale_count = 0;
        for (_e, gt, bp) in selected_query.iter() {
            let t = gt.compute_transform();
            let sz = bp.map(|b| b.size).unwrap_or(t.scale);
            let (mn, mx) = crate::math_utils::calculate_rotated_aabb(t.translation, sz * 0.5, t.rotation);
            scale_bmin = scale_bmin.min(mn);
            scale_bmax = scale_bmax.max(mx);
            scale_count += 1;
        }
        if scale_count > 0 {
            let group_center = (scale_bmin + scale_bmax) * 0.5;
            let group_extent = (scale_bmax - scale_bmin) * 0.5;
            let fov_s = crate::camera_controller::gizmo_fov(
                projection,
                camera_transform.translation(),
                group_center,
            );
            let screen_scale = crate::scale_tool::compute_scale_screen_scale(
                group_center, camera_transform.translation(), fov_s,
            );
            let scale_rotation = crate::move_tool::gizmo_rotation_for(
                transform_mode,
                selected_query.iter().map(|(_, gt, _)| gt.compute_transform().rotation),
            );
            if crate::scale_tool::is_clicking_scale_handle_group(&ray, group_center, group_extent, screen_scale, scale_rotation) {
                return; // Clicking scale handle, abort selection
            }
        }
    }
    
    // One pick for click and hover alike (see `pick_under_cursor`), so the
    // outline shown under the cursor is exactly what the click will select.
    if let Some(hit) = pick_under_cursor(
        &ray,
        &part_entities_query,
        &parent_query,
        &spatial_query,
        &click_extras.aabb_q,
        ObbFallback::Always,
    ) {
        closest_hit = Some((entity_to_id_string(hit.entity), hit.distance, hit.entity, hit.parent_model));
    }

    // Update selection
    if let Some((part_id, distance, hit_entity, parent_model)) = closest_hit {
        info!("[select] hit part_id='{}' dist={:.2}", part_id, distance);

        // Ctrl+Shift+Alt+Click on a part with a `Link` attribute: open the
        // URL in the OS default browser and skip the selection update
        // entirely. The lookup walks up the ChildOf chain so clicking
        // a part inside a Model with the Link attribute on the Model
        // also resolves (and matches Roblox-style "tap part to follow
        // link" intent). Case-insensitive key match — "Link", "link",
        // "LINK" all work; the first hit on the chain wins. The three-key
        // chord (Ctrl+Shift+Alt) keeps link-follow clear of the plain and
        // single-modifier selection clicks, and intentionally mirrors the
        // Ctrl+Shift+Alt+wheel hover-resize gesture (see
        // `hover_resize_system`). Adding Alt to Ctrl+Shift is what
        // distinguishes it from the Subtract modifier; a link-less part
        // clicked with this chord simply falls through and is subtracted.
        if ctrl_pressed && shift_pressed && alt_pressed {
            let candidates = std::iter::once(hit_entity).chain(parent_model.into_iter());
            let mut opened = false;
            for candidate in candidates {
                if let Ok(attrs) = click_extras.attributes_q.get(candidate) {
                    let link = attrs.values.iter().find_map(|(k, v)| {
                        if k.eq_ignore_ascii_case("Link") {
                            if let eustress_common::AttributeValue::String(s) = v {
                                return Some(s.clone());
                            }
                        }
                        None
                    });
                    if let Some(url) = link {
                        info!("[select] Ctrl+Shift+Alt+Click → opening Link '{}'", url);
                        open_url_in_default_browser(&url);
                        opened = true;
                        break;
                    }
                }
            }
            if opened {
                // Skip selection so Ctrl+Alt+Click feels like a hyperlink
                // tap rather than a click that ALSO mutates selection.
                return;
            }
        }

        // Double-click detection — emit a message when this is the
        // second click on the same entity within 400 ms. We run this
        // BEFORE the tool-dragging short-circuit so a tool-active state
        // doesn't swallow the double-click intent. The tracker is
        // always updated so a single click followed by a click on
        // empty space (closest_hit == None) doesn't carry stale state.
        {
            let now = std::time::Instant::now();
            let is_double = click_extras
                .dbl_tracker
                .last_at
                .map(|t| {
                    now.duration_since(t).as_millis() < 400
                        && click_extras.dbl_tracker.last_entity == Some(hit_entity)
                })
                .unwrap_or(false);
            click_extras.dbl_tracker.last_at = Some(now);
            click_extras.dbl_tracker.last_entity = Some(hit_entity);
            if is_double {
                info!("[select] DOUBLE-CLICK on entity {:?}", hit_entity);
                click_extras.dbl_writer.write(DoubleClickedPart { entity: hit_entity });
            }
        }

        // Hit a part - check if we should allow selection changes
        // Only block if a tool is ACTIVELY DRAGGING (not just active/visible)
        let tool_is_dragging =
            (move_active && move_dragging) ||
            (scale_active && scale_dragging) ||
            (rotate_active && rotate_dragging);

        if tool_is_dragging {
            return; // Tool is being used right now, don't change selection
        }
        
        // Determine what to select: the part itself, or its parent Model
        // Alt+Click = select the individual part (bypass parent selection)
        // Normal Click = select the parent Model if the part is a child of one
        let selection_id = if alt_pressed {
            part_id.clone()
        } else if let Some(model_entity) = parent_model {
            let model_id = entity_to_id_string(model_entity);
            info!("[select] selecting parent Model id='{}'", model_id);
            model_id
        } else {
            part_id.clone()
        };
        
        let sel = selection_manager.0.write();

        match combine {
            SelectionCombine::Toggle => {
                if sel.is_selected(&selection_id) {
                    sel.remove_from_selection(&selection_id);
                    info!("[select] Ctrl+click removed '{}' from selection", selection_id);
                } else {
                    sel.add_to_selection(selection_id.clone());
                    info!("[select] Ctrl+click added '{}' to selection", selection_id);
                }
            }
            SelectionCombine::Add => {
                // `add_to_selection` is idempotent, so re-clicking something
                // already in the set is a no-op rather than a silent removal.
                // That's the whole point: a Shift sweep across a marquee
                // result can't accidentally punch holes in it.
                sel.add_to_selection(selection_id.clone());
                info!("[select] Shift+click added '{}' to selection", selection_id);
            }
            SelectionCombine::Subtract => {
                // Ctrl+Shift never adds. Note that Ctrl+Shift+Alt is claimed
                // FIRST by the Link-follow chord above, which returns early
                // whenever the hit part (or its Model) actually carries a
                // `Link` attribute — only a link-less part reaches here, and
                // subtracting it is the right fallback for that chord.
                sel.remove_from_selection(&selection_id);
                info!("[select] Ctrl+Shift+click removed '{}' from selection", selection_id);
            }
            SelectionCombine::Replace => {
                sel.select(selection_id.clone());
                info!("[select] selected '{}'", selection_id);

                // Single-click selection — request the Explorer to scroll
                // this entity's tree row to the top. Skipped for every
                // modifier branch above so accumulating clicks don't whip
                // the tree around with each add/remove.
                if let Some(ref mut es) = explorer_state {
                    let target = if alt_pressed {
                        hit_entity
                    } else {
                        parent_model.unwrap_or(hit_entity)
                    };
                    es.pending_scroll_target_entity = Some(target);
                    es.needs_immediate_sync = true;
                }
            }
        }
    } else {
        // Clicked on empty space - check if we should deselect
        // Block deselection if:
        // 1. Actively dragging a tool handle
        // 2. About to click a tool handle (prevent clearing before tool processes click)
        let tool_is_dragging = 
            (move_active && move_dragging) ||
            (scale_active && scale_dragging) ||
            (rotate_active && rotate_dragging);
        
        if tool_is_dragging {
            return; // Tool is being used, don't deselect
        }
        
        // Check if we're about to click a tool handle
        // For move tool: check group center
        if move_active {
            let sel = selection_manager.0.read();
            let selected = sel.get_selected();
            
            if !selected.is_empty() {
                let mut center = Vec3::ZERO;
                let mut total_scale = 0.0;
                let mut count = 0;
                
                for (entity, part_entity, part_entity_marker, instance, transform, _mesh, _basepart, _child_of) in part_entities_query.iter() {
                    // Same canonical id as everywhere else. The comment here used
                    // to claim `"indexVgeneration"` while the code preferred the
                    // components' stored `part_id`, which for disk-loaded parts is
                    // the folder NAME — so this lookup missed every such part,
                    // `count` stayed 0, the gizmo-centre guard below never ran, and
                    // a click near the gizmo fell through to "empty space" and
                    // cleared the selection.
                    let part_id = (part_entity.is_some()
                        || part_entity_marker.is_some()
                        || instance.is_some())
                        .then(|| entity_to_id_string(entity));

                    if let Some(id) = part_id {
                        if selected.contains(&id) {
                            let t = transform.compute_transform();
                            center += t.translation;
                            total_scale += t.scale.max_element();
                            count += 1;
                        }
                    }
                }
                
                if count > 0 {
                    center /= count as f32;
                    let avg_scale = total_scale / count as f32;
                    // Camera-distance sizing, identical to the pre-raycast guard
                    // above and to the handles `move_handles` draws.
                    //
                    // This read `(avg_scale * 0.5) + 1.5` — a world-space length
                    // with no camera term at all. `detect_axis_hit` derives its
                    // hit radius as `handle_len * 0.18`, so a selection with
                    // avg_scale ~100 produced a 51 m handle and a 9 m hit
                    // cylinder along each axis. Clicking empty space anywhere in
                    // that volume matched "about to click a handle" and returned
                    // instead of clearing, so the selection never dropped. A
                    // click on a LOCKED part reaches this same branch (locked
                    // parts are excluded from the hit candidates, so they yield
                    // no hit), which is why both symptoms appeared together and
                    // only while the Move tool was active.
                    let fov_m = crate::camera_controller::gizmo_fov(
                        projection,
                        camera_transform.translation(),
                        center,
                    );
                    let handle_length = crate::move_tool::camera_scale_factor(
                        camera_transform.translation(),
                        center,
                        fov_m,
                    );

                    let gizmo_rotation = crate::move_tool::gizmo_rotation_for(
                        transform_mode,
                        selected_query.iter().map(|(_, gt, _)| gt.compute_transform().rotation),
                    );
                    if crate::move_tool::is_clicking_move_handle(
                        &ray, center, Vec3::splat(avg_scale), handle_length, &camera_transform, gizmo_rotation,
                    ) {
                        return; // About to click move handle, don't clear selection
                    }
                }
            }
        }
        
        // For scale tool: check each selected part
        if scale_active {
            // Same group-level hit check as the scale_active path above —
            // "don't clear selection on empty-space click if it's actually
            // a scale-handle click on the existing group".
            let mut s_bmin = Vec3::splat(f32::MAX);
            let mut s_bmax = Vec3::splat(f32::MIN);
            let mut s_count = 0;
            for (_e, gt, bp) in selected_query.iter() {
                let t = gt.compute_transform();
                let sz = bp.map(|b| b.size).unwrap_or(t.scale);
                let (mn, mx) = crate::math_utils::calculate_rotated_aabb(t.translation, sz * 0.5, t.rotation);
                s_bmin = s_bmin.min(mn);
                s_bmax = s_bmax.max(mx);
                s_count += 1;
            }
            if s_count > 0 {
                let group_center = (s_bmin + s_bmax) * 0.5;
                let group_extent = (s_bmax - s_bmin) * 0.5;
                let fov_s = crate::camera_controller::gizmo_fov(
                    projection,
                    camera_transform.translation(),
                    group_center,
                );
                let screen_scale = crate::scale_tool::compute_scale_screen_scale(
                    group_center, camera_transform.translation(), fov_s,
                );
                let scale_rotation = crate::move_tool::gizmo_rotation_for(
                    transform_mode,
                    selected_query.iter().map(|(_, gt, _)| gt.compute_transform().rotation),
                );
                if crate::scale_tool::is_clicking_scale_handle_group(&ray, group_center, group_extent, screen_scale, scale_rotation) {
                    return; // About to click scale handle, don't clear selection
                }
            }
        }
        
        // Clear selection when clicking on empty space (no part hit)
        // This works for ALL tools - clicking on nothing should deselect
        let sel = selection_manager.0.write();
        sel.clear();
        info!("Deselected - clicked on empty space");
    }
}

/// Open `url` in the OS default browser via the platform's "open" shim.
/// Best-effort: failure logs a warning but doesn't surface to the user
/// (the worst case is a no-op click, which matches the failure mode of
/// any other unhandled hyperlink).
///
/// We deliberately avoid pulling in the `webbrowser` / `opener` crates
/// — the three native one-liners below are stable, sandbox-friendly
/// (no NPM-style supply chain), and identical to what those crates do
/// internally.
#[cfg(not(target_arch = "wasm32"))]
fn open_url_in_default_browser(url: &str) {
    // Reject anything that doesn't look like a URL. A `Link` attribute
    // carrying a relative path or random string shouldn't trigger a
    // shell invocation — that's how command-injection bugs are born.
    let trimmed = url.trim();
    let is_http = trimmed.starts_with("http://") || trimmed.starts_with("https://");
    let is_mailto = trimmed.starts_with("mailto:");
    if !(is_http || is_mailto) {
        warn!("[select] Link attribute '{}' isn't an http(s)/mailto URL — ignoring", trimmed);
        return;
    }

    #[cfg(target_os = "windows")]
    let spawn = std::process::Command::new("cmd")
        .args(["/C", "start", "", trimmed])
        .spawn();
    #[cfg(target_os = "macos")]
    let spawn = std::process::Command::new("open").arg(trimmed).spawn();
    #[cfg(target_os = "linux")]
    let spawn = std::process::Command::new("xdg-open").arg(trimmed).spawn();

    match spawn {
        Ok(_) => info!("[select] launched browser for {}", trimmed),
        Err(e) => warn!("[select] failed to launch browser for '{}': {}", trimmed, e),
    }
}

/// **Ctrl+Shift+Alt + mouse-wheel resizes the part DIRECTLY UNDER THE
/// CURSOR** — no click, no selection required. The cursor ray-casts against
/// every part's visible OBB (`BasePart.size`, so it's collider-independent
/// and works on anchored `can_collide = false` parts too); the closest hit
/// is resized multiplicatively (~10% per notch, wheel-up grows / wheel-down
/// shrinks) by firing the shared `ResizePartEvent`, so primitive-mesh regen,
/// custom-GLB `Transform.scale` mode, `BasePart.cframe`, and the Avian
/// collider rebuild all stay consistent. The same modifier chord zeroes the
/// camera-zoom contribution in `eustress_camera_controls`, so the gesture
/// resizes the part WITHOUT also dollying the camera.
#[cfg(not(target_arch = "wasm32"))]
pub fn hover_resize_system(
    mut ev_wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
    parts_query: Query<(Entity, &GlobalTransform, &BasePart)>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    // Used to keep a part's child BillboardGui label proportional to the
    // part when it's resized (scale its studs size + z-index by the same
    // factor).
    children_query: Query<&Children>,
    mut billboard_query: Query<&mut eustress_common::classes::BillboardGui>,
    mut resize_events: MessageWriter<crate::scale_tool::ResizePartEvent>,
    // A wheel resize is an edit like any other: one undo step per gesture.
    // The local translation is what the undo restores alongside the size;
    // CAD parts resize through their feature tree and are left to it.
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    local_transforms: Query<&Transform>,
    cad_parts: Query<(), With<crate::cad_plugin::CadPart>>,
    play_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
) {
    // The wheel is the game's during a Play session.
    if !crate::play_mode::editor_input_enabled(play_state) {
        ev_wheel.clear();
        return;
    }
    use bevy::input::mouse::MouseScrollUnit;

    // Always drain wheel events (avoid buildup) and accumulate this frame's
    // notches. Line unit = one notch per `ev.y`; Pixel unit (trackpads) is
    // scaled down to a comparable magnitude.
    let mut scroll = 0.0_f32;
    for ev in ev_wheel.read() {
        scroll += if ev.unit == MouseScrollUnit::Line { ev.y } else { ev.y * 0.1 };
    }
    if scroll == 0.0 {
        return;
    }

    // Strict Ctrl+Shift+Alt gate — anything less is a normal wheel (camera
    // zoom / panel scroll), which this must not hijack.
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if !(ctrl && shift && alt) {
        return;
    }

    // Cursor must be over the 3D viewport, not a Slint panel.
    if ui_focus.as_ref().map(|f| f.has_focus).unwrap_or(false) {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    if let Some(vb) = viewport_bounds.as_ref() {
        if !vb.contains_logical(cursor, window.scale_factor() as f32) {
            return;
        }
    }

    // Main 3D camera (order 0 — the Slint overlay camera is order 100).
    let Some((camera, cam_tf)) = camera_query.iter().find(|(c, _)| c.order == 0) else { return };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else { return };

    // Closest part whose visible OBB the cursor ray pierces.
    let mut best: Option<(Entity, f32, Vec3)> = None;
    for (entity, gt, bp) in parts_query.iter() {
        if bp.locked {
            continue;
        }
        let t = gt.compute_transform();
        if let Some(dist) =
            ray_obb_intersection(ray.origin, *ray.direction, t.translation, bp.size * 0.5, t.rotation)
        {
            if best.map_or(true, |(_, d, _)| dist < d) {
                best = Some((entity, dist, bp.size));
            }
        }
    }
    let Some((entity, _, size)) = best else { return };

    // Multiplicative resize: ~10% per notch, symmetric (up grows, down
    // shrinks). Clamp to a small floor so a part can't be scrolled to zero
    // (which would make it un-pickable and degenerate the collider).
    let factor = 1.1_f32.powf(scroll);
    let new_size = (size * factor).max(Vec3::splat(0.05));
    resize_events.write(crate::scale_tool::ResizePartEvent { entity, new_size });
    // Notches rolled within the coalescing window fold into one step, so a
    // whole roll of the wheel is one Ctrl+Z.
    if !cad_parts.contains(entity) {
        if let Ok(t) = local_transforms.get(entity) {
            undo_stack.push_coalesced(
                "wheel-resize",
                "Resize with wheel",
                crate::undo::Action::ScaleEntities {
                    old_states: vec![(entity.to_bits(), t.translation.to_array(), size.to_array())],
                    new_states: vec![(entity.to_bits(), t.translation.to_array(), new_size.to_array())],
                },
            );
        }
    }

    // Keep the part's child BillboardGui label PROPORTIONAL to the part:
    // scale its STUDS size (UDim2 Scale component) by the SAME factor and
    // re-derive ZIndex from the new studs size, so a 2×-the-part label stays
    // 2× and keeps clearing the bigger part from the camera. e.g. part 2→4
    // ⇒ billboard 4→8, ZIndex 4→8. Pure-pixel labels (Scale == 0) are left
    // alone, per "scale it up if it's not in pixels". Mutating BillboardGui
    // fires the marker→quad sync + the disk save-back automatically.
    if let Ok(children) = children_query.get(entity) {
        for child in children.iter() {
            if let Ok(mut bb) = billboard_query.get_mut(child) {
                if bb.size.x.scale > 0.0 {
                    bb.size.x.scale *= factor;
                }
                if bb.size.y.scale > 0.0 {
                    bb.size.y.scale *= factor;
                }
                // ZIndex tracks the (studs) billboard size — biggest axis —
                // so the label always rides far enough toward the camera to
                // clear the now-larger part. Avoids cumulative integer-round
                // drift by deriving from the size rather than ×factor-ing the
                // old z each notch.
                let z_studs = bb.size.x.scale.max(bb.size.y.scale);
                if z_studs > 0.0 {
                    bb.z_index = z_studs.round() as i32;
                }
            }
        }
    }
}


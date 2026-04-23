//! # Align & Distribute
//!
//! Last Phase-0 tool feature per TOOLSET.md §4.4 + §4.13.8. Operates
//! on the current selection (`With<Selected>`); single-click operations
//! that produce one undo entry per event.
//!
//! ## Align
//!
//! For a given world axis + mode (Min/Center/Max), compute the target
//! coordinate:
//!   - **Min**    → `min(entity.aabb.min_axis for entity in selection)`
//!   - **Center** → `(group_aabb.min_axis + group_aabb.max_axis) * 0.5`
//!   - **Max**    → `max(entity.aabb.max_axis for entity in selection)`
//!
//! Each entity translates so that its own `aabb.min_axis` / `center` /
//! `max_axis` matches the target. Other two axes stay.
//!
//! Align-to-Active variant uses the active (last-selected) entity's
//! coordinate as the target instead of the group extrema.
//!
//! ## Distribute
//!
//! For a given world axis, sort selection by center-axis coordinate,
//! keep the two end entities fixed, space the middle entities evenly
//! by center-to-center distance. Requires ≥ 3 selected.
//!
//! ## Undo + TOML persistence
//!
//! Every handler builds a `TransformEntities` undo entry (position-only
//! delta) and writes the new `Transform.translation` back through
//! `write_instance_definition_signed` so the change survives reload.
//! Matches the Move-tool persistence path.

use bevy::prelude::*;
use crate::selection_box::Selected;
use crate::move_tool::Axis3d;
use crate::math_utils::calculate_rotated_aabb;

// ============================================================================
// Events
// ============================================================================

/// Align selection along `axis` to `mode` (Min/Center/Max).
/// Target is computed from the group's combined AABB.
#[derive(Event, Message, Debug, Clone, Copy)]
pub struct AlignEntitiesEvent {
    pub axis: Axis3d,
    pub mode: AlignMode,
}

/// Distribute selection evenly along `axis`. Keeps the two end
/// entities fixed, spaces the middle entities by equal center-to-
/// center distance. Requires >= 3 selected; otherwise no-op.
#[derive(Event, Message, Debug, Clone, Copy)]
pub struct DistributeEntitiesEvent {
    pub axis: Axis3d,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignMode {
    Min,
    Center,
    Max,
}

// ============================================================================
// Plugin
// ============================================================================

pub struct AlignDistributePlugin;

impl Plugin for AlignDistributePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AlignEntitiesEvent>()
            .add_message::<DistributeEntitiesEvent>()
            .add_systems(Update, (
                handle_align_events,
                handle_distribute_events,
            ));
    }
}

// ============================================================================
// AABB helper
// ============================================================================

/// Compute an entity's world-space AABB using BasePart.size when
/// available (primitive parts) or Transform.scale as a fallback.
/// Returns `(min, max)` along each axis.
fn entity_aabb(
    transform: &Transform,
    base_part: Option<&crate::classes::BasePart>,
) -> (Vec3, Vec3) {
    let size = base_part.map(|bp| bp.size).unwrap_or(transform.scale);
    calculate_rotated_aabb(transform.translation, size * 0.5, transform.rotation)
}

/// Pick an axis component from a Vec3.
#[inline]
fn axis_component(v: Vec3, axis: Axis3d) -> f32 {
    match axis {
        Axis3d::X => v.x,
        Axis3d::Y => v.y,
        Axis3d::Z => v.z,
    }
}

// ============================================================================
// Align handler
// ============================================================================

fn handle_align_events(
    mut events: MessageReader<AlignEntitiesEvent>,
    mut query: Query<(
        Entity,
        &mut Transform,
        Option<&mut crate::classes::BasePart>,
    ), With<Selected>>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    instance_files: Query<&crate::space::instance_loader::InstanceFile>,
    auth: Option<Res<crate::auth::AuthState>>,
    mut notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    for event in events.read() {
        let axis = event.axis;
        let mode = event.mode;

        // First pass: compute each entity's AABB + the group target.
        let aabbs: Vec<(Entity, Vec3, Vec3, Vec3)> = query.iter()
            .map(|(entity, transform, bp)| {
                let (mn, mx) = entity_aabb(transform, bp.as_deref());
                (entity, transform.translation, mn, mx)
            })
            .collect();

        if aabbs.len() < 2 {
            // Nothing silently fails — tell the user why. The old
            // behavior was a silent `continue` which left users
            // wondering if the ribbon button even fired (the
            // "buttons don't work" complaint 2026-04-23).
            if let Some(ref mut n) = notifications {
                n.warning(format!(
                    "Align {}: needs ≥2 parts selected (have {})",
                    axis_label(axis), aabbs.len(),
                ));
            }
            continue;
        }

        let target = match mode {
            AlignMode::Min => aabbs.iter().map(|(_, _, mn, _)| axis_component(*mn, axis)).fold(f32::INFINITY, f32::min),
            AlignMode::Max => aabbs.iter().map(|(_, _, _, mx)| axis_component(*mx, axis)).fold(f32::NEG_INFINITY, f32::max),
            AlignMode::Center => {
                let group_min = aabbs.iter().map(|(_, _, mn, _)| axis_component(*mn, axis)).fold(f32::INFINITY, f32::min);
                let group_max = aabbs.iter().map(|(_, _, _, mx)| axis_component(*mx, axis)).fold(f32::NEG_INFINITY, f32::max);
                (group_min + group_max) * 0.5
            }
        };

        // Second pass: compute each entity's required translation and
        // apply. For Min/Max we align the AABB edge; for Center we
        // align the AABB center.
        let mut old_transforms = Vec::new();
        let mut new_transforms = Vec::new();

        for (entity, mut transform, bp_opt) in query.iter_mut() {
            let Some((_, _initial_pos, mn, mx)) = aabbs.iter().find(|(e, ..)| *e == entity).copied() else { continue };

            let current = match mode {
                AlignMode::Min    => axis_component(mn, axis),
                AlignMode::Max    => axis_component(mx, axis),
                AlignMode::Center => (axis_component(mn, axis) + axis_component(mx, axis)) * 0.5,
            };
            let delta = target - current;

            if delta.abs() < 0.0001 { continue; }

            let offset = match axis {
                Axis3d::X => Vec3::new(delta, 0.0, 0.0),
                Axis3d::Y => Vec3::new(0.0, delta, 0.0),
                Axis3d::Z => Vec3::new(0.0, 0.0, delta),
            };

            let initial_pos = transform.translation;
            let new_pos = initial_pos + offset;

            old_transforms.push((entity.to_bits(), initial_pos.to_array(), transform.rotation.to_array()));
            new_transforms.push((entity.to_bits(), new_pos.to_array(),     transform.rotation.to_array()));

            transform.translation = new_pos;
            if let Some(mut bp) = bp_opt {
                bp.cframe.translation = new_pos;
            }
        }

        if !old_transforms.is_empty() {
            let n = old_transforms.len();
            undo_stack.push_labeled(
                format!("Align {:?} {:?} ({} parts)", mode, axis, n),
                crate::undo::Action::TransformEntities {
                    old_transforms,
                    new_transforms,
                },
            );
        }

        // TOML persistence — same signed-write path Move uses.
        let stamp = auth.as_deref().and_then(crate::space::instance_loader::current_stamp);
        for (entity, transform, _) in query.iter().map(|(e, t, _)| (e, t, ())) {
            if let Ok(inst_file) = instance_files.get(entity) {
                if let Ok(mut def) = crate::space::instance_loader::load_instance_definition(&inst_file.toml_path) {
                    def.transform.position = transform.translation.to_array();
                    def.transform.rotation = [
                        transform.rotation.x, transform.rotation.y,
                        transform.rotation.z, transform.rotation.w,
                    ];
                    let _ = crate::space::instance_loader::write_instance_definition_signed(
                        &inst_file.toml_path, &mut def, stamp.as_ref(),
                    );
                }
            }
        }

        info!("↔ Align {:?} {:?} applied to {} entities", axis, mode, aabbs.len());
        if let Some(ref mut n) = notifications {
            n.success(format!(
                "Align {} {} → {} parts",
                axis_label(axis), mode_label(mode), aabbs.len(),
            ));
        }
    }
}

/// Short axis string used in user-visible toasts + log output.
fn axis_label(axis: crate::move_tool::Axis3d) -> &'static str {
    match axis {
        crate::move_tool::Axis3d::X => "X",
        crate::move_tool::Axis3d::Y => "Y",
        crate::move_tool::Axis3d::Z => "Z",
    }
}

/// Short mode string ("Min" / "Max" / "Center") paired with axis_label
/// for the Align success toast.
fn mode_label(mode: AlignMode) -> &'static str {
    match mode {
        AlignMode::Min    => "Min",
        AlignMode::Max    => "Max",
        AlignMode::Center => "Center",
    }
}

// ============================================================================
// Distribute handler
// ============================================================================

fn handle_distribute_events(
    mut events: MessageReader<DistributeEntitiesEvent>,
    mut query: Query<(
        Entity,
        &mut Transform,
        Option<&mut crate::classes::BasePart>,
    ), With<Selected>>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    instance_files: Query<&crate::space::instance_loader::InstanceFile>,
    auth: Option<Res<crate::auth::AuthState>>,
    mut notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    for event in events.read() {
        let axis = event.axis;

        // Snapshot selection sorted by center coordinate along axis.
        let mut items: Vec<(Entity, Vec3, Vec3)> = query.iter()
            .map(|(entity, transform, bp)| {
                let (mn, mx) = entity_aabb(transform, bp.as_deref());
                let center = (mn + mx) * 0.5;
                (entity, transform.translation, center)
            })
            .collect();

        if items.len() < 3 {
            info!("↔ Distribute skipped — needs ≥ 3 selected (got {})", items.len());
            // User-visible version of the same warning — distribute
            // on <3 parts would silently no-op otherwise (the
            // "buttons don't work" feeling).
            if let Some(ref mut n) = notifications {
                n.warning(format!(
                    "Distribute {}: needs ≥3 parts selected (have {})",
                    axis_label(axis), items.len(),
                ));
            }
            continue;
        }

        items.sort_by(|a, b| {
            axis_component(a.2, axis).partial_cmp(&axis_component(b.2, axis)).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Keep ends fixed; space middle entities by equal center-to-
        // center distance.
        let first_center = axis_component(items.first().unwrap().2, axis);
        let last_center  = axis_component(items.last().unwrap().2, axis);
        let n = items.len();
        let step = (last_center - first_center) / (n as f32 - 1.0);

        if step.abs() < 0.0001 { continue; }

        let mut old_transforms = Vec::new();
        let mut new_transforms = Vec::new();

        for (i, (entity, initial_pos, center)) in items.iter().enumerate() {
            if i == 0 || i == n - 1 { continue; } // ends fixed

            let target_center = first_center + step * i as f32;
            let delta = target_center - axis_component(*center, axis);
            if delta.abs() < 0.0001 { continue; }

            let offset = match axis {
                Axis3d::X => Vec3::new(delta, 0.0, 0.0),
                Axis3d::Y => Vec3::new(0.0, delta, 0.0),
                Axis3d::Z => Vec3::new(0.0, 0.0, delta),
            };
            let new_pos = *initial_pos + offset;

            if let Ok((_, mut transform, bp_opt)) = query.get_mut(*entity) {
                old_transforms.push((entity.to_bits(), initial_pos.to_array(), transform.rotation.to_array()));
                new_transforms.push((entity.to_bits(), new_pos.to_array(),     transform.rotation.to_array()));

                transform.translation = new_pos;
                if let Some(mut bp) = bp_opt {
                    bp.cframe.translation = new_pos;
                }
            }
        }

        if !old_transforms.is_empty() {
            let moved = old_transforms.len();
            undo_stack.push_labeled(
                format!("Distribute {:?} ({} / {} parts moved)", axis, moved, n),
                crate::undo::Action::TransformEntities {
                    old_transforms,
                    new_transforms,
                },
            );
        }

        // TOML persistence.
        let stamp = auth.as_deref().and_then(crate::space::instance_loader::current_stamp);
        for (entity, transform, _) in query.iter().map(|(e, t, _)| (e, t, ())) {
            if let Ok(inst_file) = instance_files.get(entity) {
                if let Ok(mut def) = crate::space::instance_loader::load_instance_definition(&inst_file.toml_path) {
                    def.transform.position = transform.translation.to_array();
                    def.transform.rotation = [
                        transform.rotation.x, transform.rotation.y,
                        transform.rotation.z, transform.rotation.w,
                    ];
                    let _ = crate::space::instance_loader::write_instance_definition_signed(
                        &inst_file.toml_path, &mut def, stamp.as_ref(),
                    );
                }
            }
        }

        info!("↔ Distribute {:?} applied to {} entities (step {:.3})", axis, n, step);
        if let Some(ref mut n_notif) = notifications {
            n_notif.success(format!(
                "Distribute {} → {} parts spaced {:.2}",
                axis_label(axis), n, step.abs(),
            ));
        }
    }
}

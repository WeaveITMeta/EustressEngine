//! Buoyancy for Avian's dynamic rigid bodies, from the forces of
//! [`eustress_ocean::buoyancy`].
//!
//! Every FixedUpdate tick of a Physics session, before Avian steps, each
//! dynamic body is visited with every collider Avian has attached to it
//! (`RigidBodyColliders`: its own, a child's, or one welded on by hand),
//! sensors and disabled colliders left out. A collider is sampled over its
//! box in its own frame: a cuboid's half extents, or the local bounding box
//! of any other shape, cut into 3 x 3 x 3 cells whose volumes add up to the
//! shape's true volume (its mass at density 1). The box is placed from the
//! body's current `Position` and `Rotation` and the collider's offset on the
//! body (`ColliderTransform`), not from the collider's own `Position`, which
//! Avian brings up to date only at the start of each step. A box whose
//! lowest point is more than three significant wave heights above sea level
//! is skipped.
//!
//! Each cell in the water takes lift and drag at its centre against the
//! water column of [`OceanQueries::water_column`], shore fade included,
//! with the body's velocity there, v + w x (p - c) about the centre of mass
//! c that Avian computes (`ComputedCenterOfMass`). The forces go to Avian
//! through [`Forces`], whose `apply_force_at_point` adds the torque about
//! that centre of mass; Avian clears them after the step, so they are
//! applied afresh every tick, and waking the body each time keeps anything
//! in moving water awake. A calm, flat sea floats bodies at its rest level
//! too.
//!
//! Lift is the water's density times the downward component of Avian's
//! [`Gravity`] (its magnitude in Eustress, where gravity points down -Y; a
//! gravity that does not point down lifts nothing) times the volume under
//! water. The drag rate is [`DEFAULT_DRAG`], lowered by [`stable_drag`] for
//! a body so light that one step would reverse its motion through the
//! water. Nothing is applied while Avian's clock is paused or stopped:
//! forces would pile up until it runs again.

use avian3d::prelude::{
    Collider, ColliderDisabled, ColliderTransform, ComputeMassProperties3d, ComputedMass, Forces, Gravity, Physics,
    PhysicsTime, ReadRigidBodyForces, RigidBody, RigidBodyColliders, RigidBodyDisabled, Rotation, Sensor, SimpleCollider,
    WriteRigidBodyForces,
};
use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;
use eustress_ocean::{
    box_samples, sample_force, scale_volumes, stable_drag, submerged_fraction, BuoyancySample, WaterColumn, DEFAULT_DRAG,
};

use super::{OceanClock, OceanControl, OceanQueries};

/// Cells along each edge of a collider's box.
pub const CELLS_PER_EDGE: usize = 3;
/// Boxes whose lowest point is more than this many significant wave heights
/// above sea level are skipped.
pub const SKIP_ABOVE_WAVE_HEIGHTS: f32 = 3.0;

/// The box a collider is sampled over, in the collider's own frame, and
/// the volume of its shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColliderBox {
    /// Centre of the box in the collider's frame (m).
    pub center: Vec3,
    /// Half extents of the box along the collider's axes (m).
    pub half_extents: Vec3,
    /// Volume of the shape (m^3): its mass at density 1, or the box's
    /// volume where that is not a positive number (an open mesh).
    pub volume: f32,
}

impl ColliderBox {
    /// A cuboid's own half extents, or the local bounding box of any other
    /// shape, with the shape's volume. Both come from the scaled shape.
    pub fn of(collider: &Collider) -> Self {
        let (center, half_extents) = match collider.shape_scaled().as_cuboid() {
            Some(cuboid) => {
                let half = cuboid.half_extents;
                (Vec3::ZERO, Vec3::new(half.x, half.y, half.z))
            }
            None => {
                let aabb = collider.aabb(Vec3::ZERO, Rotation::IDENTITY);
                ((aabb.min + aabb.max) * 0.5, (aabb.max - aabb.min) * 0.5)
            }
        };
        let shape_volume = ComputeMassProperties3d::mass(collider, 1.0);
        let volume = if shape_volume.is_finite() && shape_volume > 0.0 {
            shape_volume
        } else {
            8.0 * half_extents.x * half_extents.y * half_extents.z
        };
        Self { center, half_extents, volume }
    }

    /// The lowest world height the box reaches with its centre at world
    /// `center` and its axes turned by `rotation`.
    pub fn lowest_point(&self, center: Vec3, rotation: Quat) -> f32 {
        let reach = (rotation * Vec3::X).y.abs() * self.half_extents.x
            + (rotation * Vec3::Y).y.abs() * self.half_extents.y
            + (rotation * Vec3::Z).y.abs() * self.half_extents.z;
        center.y - reach
    }
}

/// A collider's box and the pass that last used it.
#[derive(Clone, Copy, Debug)]
struct CachedBox {
    shape: ColliderBox,
    pass: u32,
}

/// What the system keeps between ticks: the colliders' boxes, worked out
/// again whenever a collider changes and trusted only from one pass to the
/// next, and buffers it reuses.
#[derive(Default)]
pub(super) struct BuoyancyScratch {
    boxes: EntityHashMap<CachedBox>,
    pass: u32,
    cells: Vec<BuoyancySample>,
    columns: Vec<WaterColumn>,
}

/// Pushes every dynamic body in the water with lift and drag for the coming
/// physics step (see the module documentation).
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_buoyancy(
    control: Res<OceanControl>,
    clock: Res<OceanClock>,
    queries: Res<OceanQueries>,
    gravity: Option<Res<Gravity>>,
    physics_time: Option<Res<Time<Physics>>>,
    fixed_time: Option<Res<Time<Fixed>>>,
    mut bodies: Query<(&RigidBody, &ComputedMass, &RigidBodyColliders, Forces), Without<RigidBodyDisabled>>,
    colliders: Query<(Ref<Collider>, &ColliderTransform), (Without<Sensor>, Without<ColliderDisabled>)>,
    mut scratch: Local<BuoyancyScratch>,
) {
    let scratch = &mut *scratch;
    // The length of the coming physics step: Avian advances its clock by the
    // fixed step times its relative speed, and not at all while paused.
    let step = match (&physics_time, &fixed_time) {
        (Some(physics), Some(fixed)) if !physics.is_paused() => fixed.timestep().as_secs_f32() * physics.relative_speed(),
        _ => 0.0,
    };
    let density = control.water_density;
    let running = *clock == OceanClock::Physics
        && control.buoyancy
        && queries.is_enabled()
        && step > 0.0
        && density > 0.0
        && density.is_finite();
    if !running {
        // A collider changed while nothing ran would go unseen next time.
        if !scratch.boxes.is_empty() {
            scratch.boxes.clear();
        }
        return;
    }
    let gravity = gravity.map_or(0.0, |g| (-g.0.y).max(0.0));
    let ceiling = queries.sea_level() + SKIP_ABOVE_WAVE_HEIGHTS * queries.significant_wave_height();
    scratch.pass = scratch.pass.wrapping_add(1);
    let pass = scratch.pass;
    let BuoyancyScratch { boxes, cells, columns, .. } = scratch;

    for (body, mass, body_colliders, mut forces) in &mut bodies {
        let inverse_mass = mass.inverse();
        if !body.is_dynamic() || !(inverse_mass > 0.0) {
            // Only dynamic bodies of finite mass move under a force.
            continue;
        }
        // The cells of each of the body's colliders that may reach the water.
        let body_position = forces.position().0;
        let body_rotation = forces.rotation().0;
        cells.clear();
        for entity in body_colliders {
            let Ok((collider, offset)) = colliders.get(entity) else {
                continue;
            };
            let shape = match boxes.get_mut(&entity) {
                Some(cached) if !collider.is_changed() => {
                    cached.pass = pass;
                    cached.shape
                }
                _ => {
                    let shape = ColliderBox::of(&collider);
                    boxes.insert(entity, CachedBox { shape, pass });
                    shape
                }
            };
            let rotation = (body_rotation * offset.rotation.0).normalize();
            let center = body_position + body_rotation * offset.translation + rotation * shape.center;
            if shape.lowest_point(center, rotation) > ceiling {
                continue;
            }
            let first = cells.len();
            cells.extend(box_samples(center.to_array(), rotation.to_array(), shape.half_extents.to_array(), CELLS_PER_EDGE));
            scale_volumes(&mut cells[first..], shape.volume);
        }
        if cells.is_empty() {
            continue;
        }
        columns.clear();
        columns.extend(cells.iter().map(|cell| queries.water_column(cell.point[0], cell.point[2])));
        let submerged: f32 = cells.iter().zip(columns.iter()).map(|(cell, column)| cell.volume * submerged_fraction(cell, *column)).sum();
        if !(submerged > 0.0) {
            continue;
        }
        let drag = stable_drag(DEFAULT_DRAG, inverse_mass.recip(), density, submerged, step);
        for (cell, column) in cells.iter().zip(columns.iter()) {
            let velocity = forces.velocity_at_point(Vec3::from_array(cell.point));
            if let Some(push) = sample_force(cell, *column, velocity.to_array(), gravity, density, drag) {
                forces.apply_force_at_point(Vec3::from_array(push.force), Vec3::from_array(push.point));
            }
        }
    }
    boxes.retain(|_, cached| cached.pass == pass);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collider_boxes_hold_the_shape_and_its_volume() {
        let cuboid = ColliderBox::of(&Collider::cuboid(2.0, 1.0, 3.0));
        assert_eq!(cuboid.center, Vec3::ZERO);
        assert!((cuboid.half_extents - Vec3::new(1.0, 0.5, 1.5)).abs().max_element() < 1e-6, "{cuboid:?}");
        assert!((cuboid.volume - 6.0).abs() < 1e-5, "{cuboid:?}");

        // A ball fills about half its box: the cells carry its volume.
        let ball = ColliderBox::of(&Collider::sphere(0.5));
        let ball_volume = 4.0 / 3.0 * std::f32::consts::PI * 0.125;
        assert!((ball.half_extents - Vec3::splat(0.5)).abs().max_element() < 1e-5, "{ball:?}");
        assert!((ball.volume - ball_volume).abs() < 1e-4, "{ball:?}");

        // A shape away from the collider's origin keeps its box there.
        let offset = Collider::compound(vec![(Vec3::new(2.0, 0.0, -1.0), Quat::IDENTITY, Collider::sphere(0.5))]);
        let shifted = ColliderBox::of(&offset);
        assert!((shifted.center - Vec3::new(2.0, 0.0, -1.0)).abs().max_element() < 1e-5, "{shifted:?}");
        assert!((shifted.half_extents - Vec3::splat(0.5)).abs().max_element() < 1e-5, "{shifted:?}");
        assert!((shifted.volume - ball_volume).abs() < 1e-4, "{shifted:?}");
    }

    #[test]
    fn the_lowest_point_follows_the_turn() {
        let shape = ColliderBox { center: Vec3::ZERO, half_extents: Vec3::new(1.0, 0.5, 1.5), volume: 6.0 };
        let at = Vec3::new(4.0, 3.0, -2.0);
        assert!((shape.lowest_point(at, Quat::IDENTITY) - 2.5).abs() < 1e-6);
        // A quarter turn about X stands the 3 m side upright.
        let on_end = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        assert!((shape.lowest_point(at, on_end) - 1.5).abs() < 1e-5);
        // Turned about the vertical, nothing drops.
        assert!((shape.lowest_point(at, Quat::from_rotation_y(1.1)) - 2.5).abs() < 1e-6);
    }
}

//! # A non-colliding part's mass in Play
//!
//! Avian weighs a body by its colliders, and a sensor collider (a part with
//! `CanCollide` off) weighs nothing: a dynamic body that weighs nothing is an
//! immovable one to Avian. Roblox weighs such a part like any other, so a
//! dynamic part with no collider, or only a sensor, gets the mass, inertia and
//! centre its shape, size and density (`BasePart::effective_density`) give it, marked
//! [`ShapeMass`], and loses them again once it collides (a colliding part
//! weighs through its collider's density, which physics_plugin keeps equal to
//! `BasePart.density`). A body whose `Mass` something else set (an avatar, a
//! joint's carrier) is left alone.

use avian3d::prelude::{AngularInertia, CenterOfMass, Collider, Mass, RigidBody, Sensor};
use bevy::prelude::*;

use eustress_common::classes::{BasePart, Part, PartType};

/// Weighs the Play session's non-colliding dynamic parts ([`weigh_parts`]),
/// in the fixed step before physics, which runs in the fixed post-update.
pub struct PartMassPlugin;

impl Plugin for PartMassPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(FixedUpdate, weigh_parts);
    }
}

/// The mass and principal inertia this module gave a part that cannot weigh
/// through a collider.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ShapeMass {
    pub mass: f32,
    pub inertia: Vec3,
}

/// Mass (kg) and principal inertia (kg m^2, about the part's own axes) of a
/// part of this shape and size (m) at this density (kg/m^3), with its centre
/// at the part's origin. Shapes are the colliders the engine gives parts: a
/// ball across the smallest side, a cylinder (and cone) along Y across X and
/// Z, wedges as the share of the box they fill, and anything else (a mesh) as
/// its size's box.
pub fn shape_mass(shape: PartType, size: Vec3, density: f32) -> (f32, Vec3) {
    let size = size.abs();
    let block = |share: f32| {
        let m = density * size.x * size.y * size.z * share;
        let (x2, y2, z2) = (size.x * size.x, size.y * size.y, size.z * size.z);
        (m, Vec3::new(y2 + z2, x2 + z2, x2 + y2) * (m / 12.0))
    };
    match shape {
        PartType::Ball => {
            let r = 0.5 * size.min_element();
            let m = density * 4.0 / 3.0 * std::f32::consts::PI * r * r * r;
            (m, Vec3::splat(0.4 * m * r * r))
        }
        PartType::Cylinder | PartType::Cone => {
            let r = 0.5 * size.x.min(size.z);
            let h = size.y;
            let m = density * std::f32::consts::PI * r * r * h;
            let across = m * (3.0 * r * r + h * h) / 12.0;
            (m, Vec3::new(across, 0.5 * m * r * r, across))
        }
        PartType::Wedge => block(0.5),
        PartType::CornerWedge => block(1.0 / 3.0),
        PartType::Block => block(1.0),
    }
}

/// Gives every dynamic part that has no colliding collider the mass its shape
/// gives it, and takes it off a part that collides again (see the module
/// docs). Runs when a part's body type, properties (its size, `CanCollide`,
/// material, density) or collider change.
pub fn weigh_parts(
    mut commands: Commands,
    parts: Query<
        (Entity, &BasePart, Option<&Part>, &RigidBody, Has<Collider>, Has<Sensor>, Option<&ShapeMass>, Has<Mass>),
        Or<(Changed<RigidBody>, Changed<BasePart>, Added<Collider>, Added<Sensor>)>,
    >,
) {
    for (e, base, part, body, has_collider, sensor, given, has_mass) in parts.iter() {
        if has_mass && given.is_none() {
            continue;
        }
        if !body.is_dynamic() || (has_collider && !sensor) {
            if given.is_some() {
                commands.entity(e).remove::<(ShapeMass, Mass, AngularInertia, CenterOfMass)>();
            }
            continue;
        }
        let (mass, inertia) = shape_mass(part.map_or(PartType::Block, |p| p.shape), base.size, base.effective_density());
        if !(mass.is_finite() && mass > 0.0 && inertia.is_finite()) {
            continue;
        }
        let weighed = ShapeMass { mass, inertia: inertia.max(Vec3::splat(1.0e-6)) };
        if given != Some(&weighed) {
            commands.entity(e).insert((weighed, Mass(weighed.mass), AngularInertia::new(weighed.inertia), CenterOfMass(Vec3::ZERO)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use avian3d::prelude::*;
    use eustress_common::classes::Material;

    fn physics_world() -> App {
        let mut app = App::new();
        bevy::tasks::IoTaskPool::get_or_init(Default::default);
        bevy::tasks::AsyncComputeTaskPool::get_or_init(Default::default);
        bevy::tasks::ComputeTaskPool::get_or_init(Default::default);
        app.add_plugins((
            bevy::time::TimePlugin,
            bevy::transform::TransformPlugin,
            bevy::asset::AssetPlugin::default(),
            bevy::diagnostic::DiagnosticsPlugin,
        ));
        app.init_resource::<avian3d::spatial_query::SpatialQueryDiagnostics>();
        app.init_resource::<avian3d::collider_tree::ColliderTreeDiagnostics>();
        app.init_resource::<avian3d::collision::CollisionDiagnostics>();
        app.init_resource::<avian3d::dynamics::solver::SolverDiagnostics>();
        app.init_asset::<Mesh>();
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)));
        app.add_plugins(PhysicsPlugins::default());
        app.add_plugins(PartMassPlugin);
        app
    }

    fn part(size: Vec3, material: Material) -> BasePart {
        BasePart { size, density: BasePart::material_default_density(&material), material, ..Default::default() }
    }

    fn computed_mass(app: &App, e: Entity) -> f32 {
        app.world().get::<ComputedMass>(e).map_or(f32::NAN, |m| m.value())
    }

    #[test]
    fn shapes_weigh_what_their_volume_does() {
        let (m, i) = shape_mass(PartType::Block, Vec3::new(1.0, 2.0, 3.0), 1000.0);
        assert!((m - 6000.0).abs() < 1e-2 && (i.x - 6500.0).abs() < 1e-1, "{m} {i}");
        let (m, i) = shape_mass(PartType::Ball, Vec3::splat(0.66), 232.5);
        assert!((m - 35.0).abs() < 0.1 && (i.x - 0.4 * m * 0.33 * 0.33).abs() < 1e-3, "{m} {i}");
        let (m, _) = shape_mass(PartType::Wedge, Vec3::ONE, 900.0);
        assert!((m - 450.0).abs() < 1e-3, "{m}");
    }

    /// A 0.3 m Plastic cube that does not collide (a script's strut) weighs
    /// 24.3 kg and turns like one, not an immovable nothing; a bare body with
    /// no collider at all weighs its own material.
    #[test]
    fn a_part_that_does_not_collide_still_weighs() {
        let mut app = physics_world();
        let size = Vec3::splat(0.3);
        let sensor = app
            .world_mut()
            .spawn((part(size, Material::Plastic), Part { shape: PartType::Block }, RigidBody::Dynamic, Collider::cuboid(1.0, 1.0, 1.0), Sensor, Transform::from_scale(size)))
            .id();
        let bare = app
            .world_mut()
            .spawn((part(size, Material::Metal), Part { shape: PartType::Block }, RigidBody::Dynamic, Transform::from_xyz(-3.0, 0.0, 0.0)))
            .id();
        for _ in 0..3 {
            app.update();
        }
        let want = 900.0 * 0.027;
        assert!((computed_mass(&app, sensor) - want).abs() < 0.05, "the sensor weighs {}", computed_mass(&app, sensor));
        assert!((computed_mass(&app, bare) - 7850.0 * 0.027).abs() < 0.2, "bare steel weighs {}", computed_mass(&app, bare));
        let inertia = app.world().get::<ComputedAngularInertia>(sensor).unwrap().principal_angular_inertia_with_local_frame().0;
        assert!((inertia.x - want * 0.09 / 6.0).abs() < 1e-3, "and turns like a cube, {inertia}");

        // It collides from now on: the shape's mass comes off, and its collider weighs it.
        app.world_mut().entity_mut(sensor).remove::<Sensor>();
        app.world_mut().get_mut::<BasePart>(sensor).unwrap().can_collide = true;
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().get::<ShapeMass>(sensor).is_none(), "the shape's mass is taken off");
        assert!(app.world().get::<Mass>(sensor).is_none(), "and its Mass with it");
    }

    /// A resized part is weighed again at its new size.
    #[test]
    fn a_resized_part_is_weighed_again() {
        let mut app = physics_world();
        let e = app
            .world_mut()
            .spawn((part(Vec3::ONE, Material::Plastic), Part { shape: PartType::Block }, RigidBody::Dynamic, Collider::cuboid(1.0, 1.0, 1.0), Sensor))
            .id();
        app.update();
        app.world_mut().get_mut::<BasePart>(e).unwrap().size = Vec3::new(2.0, 1.0, 1.0);
        for _ in 0..3 {
            app.update();
        }
        assert!((computed_mass(&app, e) - 1800.0).abs() < 0.5, "weighs {}", computed_mass(&app, e));
    }

    /// A body whose mass something else set keeps it.
    #[test]
    fn an_explicit_mass_is_left_alone() {
        let mut app = physics_world();
        let e = app
            .world_mut()
            .spawn((part(Vec3::ONE, Material::Plastic), Part { shape: PartType::Block }, RigidBody::Dynamic, Collider::cuboid(1.0, 1.0, 1.0), Sensor, Mass(70.0)))
            .id();
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().get::<ShapeMass>(e).is_none());
        assert!((computed_mass(&app, e) - 70.0).abs() < 0.05, "weighs {}", computed_mass(&app, e));
    }
}

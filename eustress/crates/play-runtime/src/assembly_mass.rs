//! `AssemblyMass` in a session's tree: every part of a rigid body reads the
//! body's mass, kg, from Avian's `ComputedMass`; an anchored assembly reads
//! `math.huge`. A part merged into its weld group's body (`assemblies`) is one
//! of that body's colliders and reads the group's mass; its own disabled body
//! writes nothing. Written when a body's mass or kind changes, never every frame,
//! and for every body once when a session's tree is new (a fresh tree holds
//! no masses, though the bodies may not have changed since the last session).

use avian3d::prelude::{ComputedMass, RigidBody, RigidBodyColliders};
use bevy::prelude::*;
use eustress_common::datamodel::{DataModel, DmValue};
use eustress_common::play_session::PlayDataModel;

type Body = (Entity, &'static ComputedMass, &'static RigidBody, Option<&'static RigidBodyColliders>);

/// Pull `AssemblyMass` into the tree (see the module docs). Runs in
/// `PlayScriptSet::Pull`, so scripts read this frame's masses.
#[allow(clippy::type_complexity)]
pub fn pull_assembly_mass(
    dm: Option<Res<PlayDataModel>>,
    all: Query<Body, Without<crate::assemblies::AssemblyMember>>,
    changed: Query<Body, (Or<(Changed<ComputedMass>, Changed<RigidBody>)>, Without<crate::assemblies::AssemblyMember>)>,
    mut session: Local<u64>,
) {
    let Some(dm) = dm else {
        *session = 0;
        return;
    };
    let tree = eustress_common::datamodel::session_of(&dm.dm);
    let fresh = *session != tree;
    *session = tree;
    if !fresh && changed.is_empty() {
        return;
    }
    let mut g = dm.dm.lock();
    let bodies: Vec<_> = if fresh { all.iter().collect() } else { changed.iter().collect() };
    for (body, mass, kind, colliders) in bodies {
        let value = DmValue::Number(assembly_mass(kind, mass.inverse()));
        write(&mut g, body, &value);
        for part in colliders.into_iter().flat_map(|c| c.iter()) {
            write(&mut g, part, &value);
        }
    }
}

fn write(g: &mut DataModel, part: Entity, value: &DmValue) {
    if let Some(id) = g.by_entity(part.to_bits()) {
        g.set_prop_from_engine(id, "AssemblyMass", value.clone());
    }
}

/// A body's mass, kg, from its inverse mass: infinite for an anchored
/// (static) assembly and for one Avian treats as immovable (zero inverse).
pub fn assembly_mass(kind: &RigidBody, inverse: f32) -> f64 {
    if kind.is_static() || !inverse.is_finite() || inverse <= 0.0 {
        f64::INFINITY
    } else {
        1.0 / inverse as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new Play session's tree gets every body's mass, though nothing about
    /// the bodies changed: it is a new session, whatever memory its tree got.
    #[test]
    fn a_new_session_never_keeps_the_old_sessions_state() {
        let mut world = World::new();
        let body = world.spawn((RigidBody::Dynamic, ComputedMass::new(2.0))).id();
        let mut schedule = Schedule::default();
        schedule.add_systems(pull_assembly_mass);
        let session = |world: &mut World| {
            let dm = eustress_common::datamodel::new_shared();
            let part = {
                let mut g = dm.lock();
                let part = g.create("Part");
                g.bind_entity(part, body.to_bits());
                part
            };
            world.insert_resource(PlayDataModel { dm: dm.clone() });
            (dm, part)
        };
        let mass = |dm: &eustress_common::datamodel::SharedDataModel, part| {
            dm.lock().get_prop(part, "AssemblyMass").and_then(|v| v.as_number())
        };

        let (first, part) = session(&mut world);
        schedule.run(&mut world);
        assert_eq!(mass(&first, part), Some(2.0));
        world.remove_resource::<PlayDataModel>();
        drop(first);
        schedule.run(&mut world);

        // The body is untouched; only the session is new.
        let (second, part) = session(&mut world);
        schedule.run(&mut world);
        assert_eq!(mass(&second, part), Some(2.0), "the new session's tree got the mass");
    }

    #[test]
    fn an_anchored_assembly_is_infinitely_heavy() {
        assert_eq!(assembly_mass(&RigidBody::Static, 0.5), f64::INFINITY);
        assert_eq!(assembly_mass(&RigidBody::Dynamic, 0.0), f64::INFINITY, "Avian's immovable body");
        assert!((assembly_mass(&RigidBody::Dynamic, 0.25) - 4.0).abs() < 1e-9);
        assert!((assembly_mass(&RigidBody::Kinematic, 0.5) - 2.0).abs() < 1e-9);
    }
}

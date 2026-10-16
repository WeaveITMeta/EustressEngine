//! # Play's simulated parts, drawn between physics steps
//!
//! Physics steps at a fixed 60 Hz while the display draws at its own rate, so a
//! body drawn at its last physics pose stands still on some frames and jumps on
//! others: on a 44 Hz display a car at 30 m/s moves 0.5 m one frame and 1 m the
//! next, and a camera following it judders. Avian's interpolation, in its
//! default plugins, draws a body part of the way from its previous physics pose
//! to its latest, by how far the clock has run into the next step, but only for
//! the bodies that ask: here every dynamic part asks while it simulates on this
//! machine, and stops asking once it is anchored.
//!
//! Interpolation moves `Transform`, which is what is drawn and what a seated
//! avatar and the camera follow. What needs the physics pose itself reads
//! `Position` and `Rotation`: a joint's frames, and the poses scripts read. A
//! script that moves a part (a teleport) moves its `Position` with it
//! ([`crate::apply`]), and the part is not eased for the physics step that
//! follows, or the easing would carry it back toward where it was.

use avian3d::prelude::{
    NoRotationEasing, NoTranslationEasing, Position, RigidBody, Rotation, RotationInterpolation,
    TransformEasingSystems, TranslationInterpolation,
};
use bevy::prelude::*;
use eustress_common::classes::BasePart;
use eustress_common::play_session::{PlayDataModel, PlayRole};

/// Eases the session's simulated parts; added with the scripted joints.
pub struct BodyEasingPlugin;

impl Plugin for BodyEasingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, ease_simulated_parts.before(TransformSystems::Propagate))
            .add_systems(FixedLast, end_script_moves.before(TransformEasingSystems::UpdateEnd));
    }
}

/// A part a script moved: it is not eased until a physics step has run from
/// where it was put.
#[derive(Component, Debug, Clone, Copy)]
pub struct MovedByScript;

/// A dynamic part simulated here is eased; any other part is not. A part
/// anchored during the session is put back at its physics pose, which an eased
/// `Transform` may trail by up to one step. When the session ends the easing
/// just comes off: Stop puts every part back where it was before Play.
///
/// Taking the easing off also marks the part not to be eased: the easing's
/// state stays on the entity, and the easing would otherwise go on blending
/// from it until the next physics step clears it, over the pose just set.
fn ease_simulated_parts(
    mut commands: Commands,
    session: Option<Res<PlayDataModel>>,
    role: Option<Res<PlayRole>>,
    // A part merged into its weld group is drawn with its group's body (`assemblies`).
    changed: Query<(Entity, &RigidBody, Has<TranslationInterpolation>), (Changed<RigidBody>, With<BasePart>, Without<crate::assemblies::AssemblyMember>)>,
    eased: Query<Entity, (With<TranslationInterpolation>, With<BasePart>)>,
    poses: Query<(&Position, &Rotation)>,
    parents: Query<&ChildOf>,
    mut transforms: Query<&mut Transform>,
) {
    let simulating = session.is_some() && role.map_or(true, |r| *r == PlayRole::Authority);
    let mut stop = Vec::new();
    if simulating {
        for (entity, body, is_eased) in &changed {
            match (*body == RigidBody::Dynamic, is_eased) {
                (true, false) => {
                    commands
                        .entity(entity)
                        .insert((TranslationInterpolation, RotationInterpolation))
                        .try_remove::<(NoTranslationEasing, NoRotationEasing)>();
                }
                (false, true) => stop.push(entity),
                _ => {}
            }
        }
    } else {
        // The session is over (or this machine only draws): nothing is eased.
        for entity in &eased {
            commands
                .entity(entity)
                .try_remove::<(TranslationInterpolation, RotationInterpolation, MovedByScript)>()
                .try_insert((NoTranslationEasing, NoRotationEasing));
        }
    }
    for entity in stop {
        commands
            .entity(entity)
            .try_remove::<(TranslationInterpolation, RotationInterpolation, MovedByScript)>()
            .try_insert((NoTranslationEasing, NoRotationEasing));
        let Ok((position, rotation)) = poses.get(entity) else { continue };
        let world = Transform::from_translation(position.0).with_rotation(rotation.0);
        // The parent's world transform, composed up its chain.
        let mut parent_affine = bevy::math::Affine3A::IDENTITY;
        let mut node = parents.get(entity).ok().map(|c| c.parent());
        while let Some(parent) = node {
            if let Ok(t) = transforms.get(parent) {
                parent_affine = t.compute_affine() * parent_affine;
            }
            node = parents.get(parent).ok().map(|c| c.parent());
        }
        let local = parent_affine.inverse() * world.compute_affine();
        let (_, rotation, translation) = local.to_scale_rotation_translation();
        if let Ok(mut t) = transforms.get_mut(entity) {
            t.translation = translation;
            t.rotation = rotation;
        }
    }
}

/// After the physics step that follows a script's move, the part eases again
/// from where that step left it.
fn end_script_moves(mut commands: Commands, moved: Query<Entity, With<MovedByScript>>) {
    for entity in &moved {
        commands.entity(entity).try_remove::<(MovedByScript, NoTranslationEasing, NoRotationEasing)>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use avian3d::prelude::PhysicsPlugins;
    use eustress_common::datamodel::{DmValue, InstanceId, SharedDataModel};
    use eustress_common::play_session::PlayScriptSet;
    use eustress_common::scripting::{CFrame, Vector3};

    /// A Play session drawn by the apply step, physics at 60 Hz, a display at 144 Hz.
    fn session() -> (App, SharedDataModel) {
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
        app.init_asset::<Mesh>().init_asset::<StandardMaterial>();
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 144.0)));
        app.add_plugins((PhysicsPlugins::default(), BodyEasingPlugin));
        let tree = eustress_common::datamodel::new_shared();
        app.insert_resource(PlayDataModel { dm: tree.clone() }).insert_resource(PlayRole::Authority);
        app.add_systems(Update, crate::apply::apply_frame.in_set(PlayScriptSet::Apply));
        // `App::run` finishes the plugins, and the transform easing adds its
        // systems there; a test that only calls `update` must finish them too.
        app.finish();
        app.cleanup();
        (app, tree)
    }

    /// A 1 m unanchored part 50 m up, drawn.
    fn falling_part(app: &mut App, tree: &SharedDataModel) -> (InstanceId, Entity) {
        let id = {
            let mut g = tree.lock();
            let ws = g.get_service("Workspace").unwrap();
            let p = g.create("Part");
            g.set_prop(p, "Size", DmValue::Vector3(Vector3::new(1.0, 1.0, 1.0))).unwrap();
            g.set_prop(p, "CFrame", DmValue::CFrame(CFrame::new(0.0, 50.0, 0.0))).unwrap();
            g.set_prop(p, "Anchored", DmValue::Bool(false)).unwrap();
            g.set_parent(p, Some(ws)).unwrap();
            p
        };
        app.update();
        app.update();
        let e = Entity::from_bits(tree.lock().entity_of(id).expect("drawn"));
        (id, e)
    }

    fn drawn_and_physics_y(app: &App, e: Entity) -> (f32, f32) {
        let w = app.world();
        (w.get::<Transform>(e).unwrap().translation.y, w.get::<Position>(e).unwrap().0.y)
    }

    /// Between physics steps a falling part is still drawn moving: on every
    /// frame no step ran, its Transform moved anyway.
    #[test]
    fn a_falling_part_is_drawn_between_physics_steps() {
        let (mut app, tree) = session();
        let (_, e) = falling_part(&mut app, &tree);
        assert!(app.world().get::<TranslationInterpolation>(e).is_some(), "a dynamic part is eased");
        // Easing has a start and an end once a physics step has run.
        for _ in 0..6 {
            app.update();
        }
        let (mut without_step, mut moved) = (0, 0);
        let mut last = drawn_and_physics_y(&app, e);
        for _ in 0..144 {
            app.update();
            let now = drawn_and_physics_y(&app, e);
            if now.1 == last.1 {
                without_step += 1;
                if now.0 != last.0 {
                    moved += 1;
                }
            }
            last = now;
        }
        assert!(without_step > 40, "a 144 Hz display has frames with no physics step, here {without_step}");
        assert_eq!(moved, without_step, "the part was drawn standing still on {} of {without_step} frames with no step", without_step - moved);
    }

    /// A script's move of a falling part sticks: the easing does not carry it
    /// back toward where it was, drawn or simulated.
    #[test]
    fn a_scripts_move_of_an_eased_part_sticks() {
        let (mut app, tree) = session();
        let (id, e) = falling_part(&mut app, &tree);
        for _ in 0..30 {
            app.update();
        }
        tree.lock().set_prop(id, "CFrame", DmValue::CFrame(CFrame::new(20.0, 80.0, 0.0))).unwrap();
        for _ in 0..20 {
            app.update();
            let w = app.world();
            let drawn = w.get::<Transform>(e).unwrap().translation.x;
            let simulated = w.get::<Position>(e).unwrap().0.x;
            assert!((drawn - 20.0).abs() < 1e-3 && (simulated - 20.0).abs() < 1e-3, "moved to x 20, now drawn at {drawn}, simulated at {simulated}");
        }
    }

    /// Anchored, a part stops easing and stands at its physics pose, not up to
    /// a step behind it.
    #[test]
    fn an_anchored_part_stops_easing_at_its_physics_pose() {
        let (mut app, tree) = session();
        let (id, e) = falling_part(&mut app, &tree);
        for _ in 0..37 {
            app.update();
        }
        tree.lock().set_prop(id, "Anchored", DmValue::Bool(true)).unwrap();
        app.update();
        app.update();
        let (drawn, simulated) = drawn_and_physics_y(&app, e);
        assert!(app.world().get::<TranslationInterpolation>(e).is_none(), "an anchored part is not eased");
        assert!((drawn - simulated).abs() < 1e-4, "drawn at y {drawn}, its physics pose at {simulated}");
    }
}

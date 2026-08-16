//! # Behavioural tests — the ones that actually move the character
//!
//! Every other test in this module tree is a pure-function check: easing
//! curves, quaternion algebra, threshold constants. Not one of them executes
//! `drive_locomotion` or `drive_climb`, which means the entire controller could
//! be deleted and the suite would stay green. That gap is why bug after bug
//! reached the player and had to be diagnosed from screenshots.
//!
//! These tests build a real `App` with Avian physics, put real geometry in it,
//! drive the avatar through `AvatarIntent`, and assert on where the body
//! actually ends up.
//!
//! ## Driving input without a keyboard
//!
//! `control::sample_movement_input` rewrites `AvatarIntent` from `ButtonInput`
//! every frame, so writing the component directly from a test is overwritten
//! before the controller reads it. [`TestIntent`] is injected by a system
//! scheduled *between* the Input and Locomotion sets — the same seam the real
//! input occupies — so the controller cannot tell the difference.

use bevy::prelude::*;

/// Intent for the harness to inject, standing in for a keyboard.
#[derive(Resource, Debug, Clone, Default)]
pub struct TestIntent {
    pub direction: Vec3,
    pub sprint: bool,
    pub crouch: bool,
    pub jump: bool,
}

/// Copies [`TestIntent`] onto every avatar, after input sampling and before the
/// controller runs.
pub fn inject_test_intent(
    mut wanted: ResMut<TestIntent>,
    mut q: Query<&mut super::spawn::AvatarIntent, With<super::SpawnedByAvatarRuntime>>,
) {
    for mut intent in q.iter_mut() {
        intent.direction = wanted.direction;
        intent.sprint = wanted.sprint;
        intent.crouch = wanted.crouch;
        if wanted.jump {
            intent.jump_pressed = true;
        }
    }
    // Jump is edge-triggered: a held flag would re-fire every frame and the
    // buffer logic would never see a release.
    wanted.jump = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::avatar::climb::{AvatarClimb, ClimbPhase};
    use crate::avatar::grip::Grip;
    use eustress_avatar_schema::{resolve, NOMINAL_BIND_HEIGHT_M};
    use crate::avatar::spawn::{AvatarBody, AvatarLocomotion};
    use crate::avatar::{
        AvatarDescriptor, AvatarHost, AvatarRuntimePlugin, AvatarSystems, SpawnAvatar,
        SpawnedByAvatarRuntime,
    };
    use avian3d::prelude::*;

    /// A world with physics, the avatar runtime, and the test intent seam.
    fn world(host: AvatarHost) -> App {
        let mut app = App::new();
        // All three pools. `TaskPoolPlugin` (part of DefaultPlugins) normally
        // does this; adding Bevy's plugins individually leaves them
        // uninitialised, and the first system that dispatches work panics.
        bevy::tasks::IoTaskPool::get_or_init(Default::default);
        bevy::tasks::AsyncComputeTaskPool::get_or_init(Default::default);
        bevy::tasks::ComputeTaskPool::get_or_init(Default::default);
        crate::avatar::boot::register_avatar_asset_sources(&mut app);
        app.add_plugins((
            bevy::app::ScheduleRunnerPlugin::default(),
            bevy::time::TimePlugin,
            bevy::transform::TransformPlugin,
            bevy::asset::AssetPlugin::default(),
            bevy::input::InputPlugin,
            bevy::animation::AnimationPlugin,
            // Avian's spatial-query and collider-tree systems take
            // `ResMut<SpatialQueryDiagnostics>` / `ResMut<ColliderTreeDiagnostics>`
            // unconditionally. `DefaultPlugins` supplies those in both shells,
            // so the omission is invisible outside tests — and it is why no
            // test has ever raycast or moved a collider: the first frame that
            // tried panicked on param validation.
            bevy::diagnostic::DiagnosticsPlugin,
        ));
        // Avian's spatial-query and collider-tree systems take
        // `ResMut<SpatialQueryDiagnostics>` / `ResMut<ColliderTreeDiagnostics>`
        // unconditionally, but `register_physics_diagnostics` guards its
        // `init_resource` behind `is_resource_added`, which does not hold at
        // plugin-build time in a minimal app. With `DefaultPlugins` the
        // resources end up present anyway, so the gap is invisible in both
        // shells — and it is why no test has ever raycast or moved a collider:
        // the first frame that tried panicked on parameter validation.
        //
        // All seven, not just the two that happened to panic first — this is a
        // whole class, and fixing it one resource per test run is how a
        // harness ends up half-built.
        app.init_resource::<avian3d::spatial_query::SpatialQueryDiagnostics>();
        app.init_resource::<avian3d::collider_tree::ColliderTreeDiagnostics>();
        app.init_resource::<avian3d::collision::CollisionDiagnostics>();
        app.init_resource::<avian3d::dynamics::solver::SolverDiagnostics>();
        app.init_asset::<Mesh>();
        app.init_asset::<WorldAsset>();
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        // Drive time deterministically.
        //
        // `TimePlugin` reads the wall clock, so a tight `for _ in 0..120 {
        // app.update() }` loop advances a couple of MILLISECONDS, not two
        // seconds — the character barely falls and barely moves, and every
        // assertion measures noise. `ManualDuration` makes one update mean
        // exactly one frame, which also makes the whole suite reproducible
        // rather than dependent on how fast the machine happens to be.
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1.0 / 60.0),
        ));
        app.add_plugins(PhysicsPlugins::default());
        app.insert_resource(Gravity(Vec3::NEG_Y * eustress_avatar_schema::GRAVITY_MPS2));
        app.add_plugins(AvatarRuntimePlugin::new(host));
        app.init_resource::<TestIntent>();
        app.add_systems(
            Update,
            inject_test_intent
                .after(AvatarSystems::Input)
                .before(AvatarSystems::Locomotion),
        );
        app
    }

    /// A static box. `Collider::cuboid` takes FULL lengths.
    fn box_at(app: &mut App, centre: Vec3, size: Vec3, rot: Quat) {
        app.world_mut().spawn((
            Transform::from_translation(centre).with_rotation(rot),
            Collider::cuboid(size.x, size.y, size.z),
            RigidBody::Static,
        ));
    }

    fn ground(app: &mut App) {
        // Top face at y = 0.
        box_at(app, Vec3::new(0.0, -0.5, 0.0), Vec3::new(80.0, 1.0, 80.0), Quat::IDENTITY);
    }

    fn spawn_at(app: &mut App, at: Vec3) {
        app.world_mut()
            .write_message(SpawnAvatar::new(AvatarDescriptor::default(), at));
    }

    fn step(app: &mut App, frames: usize) {
        for _ in 0..frames {
            app.update();
        }
    }

    fn body_pos(app: &mut App) -> Vec3 {
        let w = app.world_mut();
        let mut q = w.query_filtered::<&Transform, With<SpawnedByAvatarRuntime>>();
        q.iter(w).next().map(|t| t.translation).expect("no avatar")
    }

    fn loco(app: &mut App) -> AvatarLocomotion {
        let w = app.world_mut();
        let mut q = w.query_filtered::<&AvatarLocomotion, With<SpawnedByAvatarRuntime>>();
        q.iter(w).next().cloned().expect("no avatar")
    }

    fn climb_phase(app: &mut App) -> ClimbPhase {
        let w = app.world_mut();
        let mut q = w.query_filtered::<&AvatarClimb, With<SpawnedByAvatarRuntime>>();
        q.iter(w).next().map(|c| c.phase).unwrap_or(ClimbPhase::None)
    }

    fn half_extent(app: &mut App) -> f32 {
        let w = app.world_mut();
        let mut q = w.query_filtered::<&AvatarBody, With<SpawnedByAvatarRuntime>>();
        q.iter(w).next().map(|b| b.metrics.capsule_half_extent()).expect("no avatar")
    }

    fn set_intent(app: &mut App, dir: Vec3, sprint: bool) {
        let mut t = app.world_mut().resource_mut::<TestIntent>();
        t.direction = dir;
        t.sprint = sprint;
    }

    /// Settle onto the ground and return the resting position.
    fn settle(app: &mut App) -> Vec3 {
        step(app, 120);
        body_pos(app)
    }

    // ── Standing still ─────────────────────────────────────────────────────

    #[test]
    fn standing_on_flat_ground_does_not_drift() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        spawn_at(&mut app, Vec3::new(0.0, 2.0, 0.0));
        let start = settle(&mut app);
        step(&mut app, 180);
        let end = body_pos(&mut app);

        let drift = (end - start).with_y(0.0).length();
        assert!(
            drift < 0.02,
            "drifted {drift:.3} m in 3 s with no input (from {start:?} to {end:?})"
        );
        assert!(loco(&mut app).grounded, "lost contact with flat ground");
    }

    /// The one the player reported twice: a downhill slide, then an uphill
    /// shove after the first fix.
    #[test]
    fn standing_on_a_slope_does_not_slide_or_climb() {
        for deg in [15.0_f32, 30.0, 40.0] {
            let mut app = world(AvatarHost::Client);
            ground(&mut app);
            // A wide ramp centred at the origin.
            box_at(
                &mut app,
                Vec3::new(0.0, 2.0, 0.0),
                Vec3::new(20.0, 0.5, 20.0),
                Quat::from_rotation_x(deg.to_radians()),
            );
            spawn_at(&mut app, Vec3::new(0.0, 6.0, 0.0));
            let start = settle(&mut app);
            step(&mut app, 180);
            let end = body_pos(&mut app);

            let drift = (end - start).with_y(0.0).length();
            assert!(
                drift < 0.10,
                "{deg}°: drifted {drift:.3} m in 3 s with no input — \
                 downhill is a slide, uphill is a shove, both are bugs"
            );
        }
    }

    // ── Moving ─────────────────────────────────────────────────────────────

    #[test]
    fn holding_a_direction_actually_moves_the_body() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        spawn_at(&mut app, Vec3::new(0.0, 2.0, 0.0));
        let start = settle(&mut app);

        set_intent(&mut app, Vec3::NEG_Z, false);
        step(&mut app, 120);
        let end = body_pos(&mut app);

        let moved = (end - start).with_y(0.0).length();
        assert!(moved > 1.0, "walked only {moved:.2} m in 2 s");
        assert!(loco(&mut app).grounded, "walking on flat ground went airborne");
    }

    /// Walking down a small riser must not launch the body: going airborne on
    /// every tread is what drives the jump clip on stairs.
    #[test]
    fn walking_off_a_small_step_stays_grounded() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        // A 0.25 m platform to walk off (below the 0.30 step height).
        box_at(&mut app, Vec3::new(0.0, 0.125, -3.0), Vec3::new(8.0, 0.25, 8.0), Quat::IDENTITY);
        spawn_at(&mut app, Vec3::new(0.0, 2.0, -3.0));
        settle(&mut app);

        set_intent(&mut app, Vec3::Z, false);
        let mut airborne_frames = 0;
        for _ in 0..150 {
            app.update();
            if !loco(&mut app).grounded {
                airborne_frames += 1;
            }
        }
        assert!(
            airborne_frames < 12,
            "spent {airborne_frames} frames airborne walking off a 0.25 m step"
        );
    }

    #[test]
    fn a_step_below_step_height_is_climbed() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        box_at(&mut app, Vec3::new(0.0, 0.11, -4.0), Vec3::new(8.0, 0.22, 4.0), Quat::IDENTITY);
        spawn_at(&mut app, Vec3::new(0.0, 2.0, -1.0));
        let start = settle(&mut app);

        set_intent(&mut app, Vec3::NEG_Z, false);
        step(&mut app, 200);
        let end = body_pos(&mut app);

        assert!(
            end.y > start.y + 0.12,
            "did not get up a 0.22 m step: y {:.2} -> {:.2}",
            start.y,
            end.y
        );
    }

    /// "Land on the ledge and keep walking."
    ///
    /// A vault used to zero the velocity, so clearing a knee-high block brought
    /// the character to a dead stop on top of it and the player had to release
    /// and re-press to carry on.
    #[test]
    fn vaulting_a_low_block_lands_on_top_still_moving() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        // 0.60 m block — above STEP_HEIGHT so it cannot be walked up, well
        // below the vault ceiling so it must not be hung from.
        box_at(&mut app, Vec3::new(0.0, 0.3, -5.0), Vec3::new(10.0, 0.6, 4.0), Quat::IDENTITY);
        spawn_at(&mut app, Vec3::new(0.0, 1.5, 0.0));
        settle(&mut app);

        set_intent(&mut app, Vec3::NEG_Z, false);
        let mut saw_vault = false;
        for _ in 0..600 {
            app.update();
            if climb_phase(&mut app) == ClimbPhase::Vaulting {
                saw_vault = true;
            }
            // Stop once it is up and past the phase.
            if saw_vault && climb_phase(&mut app) == ClimbPhase::None && body_pos(&mut app).y > 1.2
            {
                break;
            }
        }
        assert!(saw_vault, "a 0.60 m block was never vaulted (phase {:?})", climb_phase(&mut app));

        let p = body_pos(&mut app);
        let half = half_extent(&mut app);
        assert!(
            (p.y - (0.6 + half)).abs() < 0.30,
            "did not end up standing on the 0.60 m block: y {:.2}, expected about {:.2}",
            p.y,
            0.6 + half
        );
        // Still moving. This is the whole point of vaulting over climbing.
        for _ in 0..10 {
            app.update();
        }
        assert!(
            loco(&mut app).planar_speed > 0.5,
            "landed and stopped dead: {:.2} m/s",
            loco(&mut app).planar_speed
        );
    }


    // ── Climbing ───────────────────────────────────────────────────────────

    /// A ramp is not a ledge. Grabbing one produced the seated pose.
    #[test]
    fn a_ramp_is_never_grabbed() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        box_at(
            &mut app,
            Vec3::new(0.0, 1.2, -4.0),
            Vec3::new(10.0, 0.5, 8.0),
            Quat::from_rotation_x(45.0_f32.to_radians()),
        );
        spawn_at(&mut app, Vec3::new(0.0, 2.0, 0.0));
        settle(&mut app);

        set_intent(&mut app, Vec3::NEG_Z, true);
        for _ in 0..240 {
            app.update();
            assert_eq!(
                climb_phase(&mut app),
                ClimbPhase::None,
                "grabbed a 45° ramp — that is a slope, not a ledge"
            );
        }
    }

    /// "Walking against walls slides me all the way to the edge."
    ///
    /// Collide-and-slide projects the whole movement onto the wall plane and
    /// keeps its magnitude, so a run that is 99% into the face converts into a
    /// fast glide ALONG it. Damping that has to be tested both ways: stopping a
    /// head-on run is only correct if running deliberately along a wall still
    /// works, and a naive fix kills both.
    #[test]
    fn a_head_on_wall_stops_you_but_running_along_one_does_not() {
        // Drift along the wall after driving into it for two seconds.
        fn lateral_travel(dir: Vec3) -> f32 {
            let mut app = world(AvatarHost::Client);
            ground(&mut app);
            // Wall face at z = -3, running a long way in x.
            box_at(&mut app, Vec3::new(0.0, 3.0, -5.0), Vec3::new(60.0, 6.0, 4.0), Quat::IDENTITY);
            spawn_at(&mut app, Vec3::new(0.0, 1.5, 0.0));
            let start = settle(&mut app);

            set_intent(&mut app, dir, true);
            step(&mut app, 120);
            (body_pos(&mut app).x - start.x).abs()
        }

        // Nearly square into the face, with just enough angle to pick a side.
        let head_on = lateral_travel(Vec3::new(0.12, 0.0, -1.0).normalize());
        // Deliberately along the wall, barely leaning into it.
        let along = lateral_travel(Vec3::new(1.0, 0.0, -0.12).normalize());

        assert!(
            head_on < 1.5,
            "ran square into a wall and glided {head_on:.2} m along it"
        );
        assert!(
            along > 3.0,
            "running ALONG a wall was damped too: only {along:.2} m of travel"
        );
        assert!(
            along > head_on * 3.0,
            "no meaningful difference between running into a wall ({head_on:.2} m)              and running along one ({along:.2} m) — the damping is not angle-aware"
        );
    }

    /// "I still glide up this very unrealistically."
    ///
    /// Damping only the sideways glide fixed walls and did nothing for ramps:
    /// the push INTO a steep face survived, and collide-and-slide projects a
    /// horizontal push onto a 60° slope as motion UP it. Tested against a
    /// walkable slope too, because refusing to climb anything at all would pass
    /// the first assertion and be just as wrong.
    #[test]
    fn a_slope_too_steep_to_stand_on_cannot_be_walked_up() {
        fn climbed(angle_deg: f32) -> f32 {
            let mut app = world(AvatarHost::Client);
            ground(&mut app);
            // A long ramp tilted about X, so its face runs across -Z.
            let tilt = Quat::from_rotation_x(angle_deg.to_radians());
            box_at(&mut app, Vec3::new(0.0, 0.0, -6.0), Vec3::new(20.0, 1.0, 12.0), tilt);
            spawn_at(&mut app, Vec3::new(0.0, 1.5, 1.5));
            let start = settle(&mut app);

            set_intent(&mut app, Vec3::NEG_Z, true);
            step(&mut app, 180);
            body_pos(&mut app).y - start.y
        }

        // MAX_SLOPE_DEG is 50, so 65° is unwalkable and 25° is a stroll.
        let steep = climbed(65.0);
        let walkable = climbed(25.0);

        assert!(
            steep < 0.6,
            "glided {steep:.2} m up a 65° face that cannot be stood on"
        );
        assert!(
            walkable > 0.5,
            "could not walk up a 25° slope either: only {walkable:.2} m —              the damping is rejecting ground it should accept"
        );
    }



    #[test]
    fn a_tall_wall_produces_a_hang_that_persists() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        // Top at 1.9 m — inside the reach band, above the vault threshold.
        box_at(&mut app, Vec3::new(0.0, 0.95, -4.0), Vec3::new(10.0, 1.9, 4.0), Quat::IDENTITY);
        spawn_at(&mut app, Vec3::new(0.0, 2.0, 0.0));
        settle(&mut app);

        set_intent(&mut app, Vec3::NEG_Z, false);
        let mut grabbed = false;
        for _ in 0..300 {
            app.update();
            if climb_phase(&mut app) != ClimbPhase::None {
                grabbed = true;
                break;
            }
        }
        assert!(grabbed, "never grabbed a 1.9 m wall while walking into it");

        // Neutral input must hold the hang — the property the whole traversal
        // set depends on.
        set_intent(&mut app, Vec3::ZERO, false);
        step(&mut app, 180);
        assert_ne!(
            climb_phase(&mut app),
            ClimbPhase::None,
            "the hang expired on its own with no input"
        );
    }

    /// "It clips me to the nearest midpoint instead of going to where I am."
    ///
    /// The grab must happen where the player approached the wall, not at some
    /// canonical point on it.
    #[test]
    fn a_grab_happens_where_the_player_is_not_at_the_wall_centre() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        // A wide wall centred on x = 0.
        box_at(&mut app, Vec3::new(0.0, 0.95, -4.0), Vec3::new(24.0, 1.9, 4.0), Quat::IDENTITY);
        // Approach well off-centre.
        spawn_at(&mut app, Vec3::new(7.0, 2.0, 0.0));
        settle(&mut app);

        set_intent(&mut app, Vec3::NEG_Z, false);
        for _ in 0..300 {
            app.update();
            if climb_phase(&mut app) != ClimbPhase::None {
                break;
            }
        }
        assert_ne!(climb_phase(&mut app), ClimbPhase::None, "never grabbed the wall");

        step(&mut app, 60);
        let x = body_pos(&mut app).x;
        assert!(
            (x - 7.0).abs() < 0.6,
            "grabbed at x={x:.2} after approaching at x=7.0 — the body was              pulled along the wall instead of taking hold where it stood"
        );
    }

    /// "Pressing W or Space doesn't climb up."
    #[test]
    fn jumping_from_a_hang_on_a_standable_ledge_completes_a_mantle() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        // Standable top at 1.9 m, deep enough to stand on.
        box_at(&mut app, Vec3::new(0.0, 0.95, -5.0), Vec3::new(10.0, 1.9, 6.0), Quat::IDENTITY);
        spawn_at(&mut app, Vec3::new(0.0, 2.0, 0.0));
        let start = settle(&mut app);

        set_intent(&mut app, Vec3::NEG_Z, false);
        for _ in 0..300 {
            app.update();
            if climb_phase(&mut app) != ClimbPhase::None {
                break;
            }
        }
        assert_ne!(climb_phase(&mut app), ClimbPhase::None, "never grabbed");

        // Ask to go up.
        set_intent(&mut app, Vec3::ZERO, false);
        app.world_mut().resource_mut::<TestIntent>().jump = true;
        step(&mut app, 240);

        let end = body_pos(&mut app);
        assert!(
            end.y > start.y + 1.5,
            "jump from a hang did not get onto a 1.9 m ledge: y {:.2} -> {:.2}              (phase {:?})",
            start.y,
            end.y,
            climb_phase(&mut app)
        );
    }

    /// "I still can't go around corners."
    #[test]
    fn shimmying_to_the_end_of_a_lip_transfers_around_the_corner() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        // One wall faces +Z along x = -6..0. Its lip ENDS at x = 0, and the
        // outer corner is the wall's own end face (normal +X) — no second wall
        // needed, and the previous two-wall setup left a 2 m gap that was not
        // a corner at all.
        box_at(&mut app, Vec3::new(-3.0, 0.95, -4.0), Vec3::new(6.0, 1.9, 4.0), Quat::IDENTITY);
        spawn_at(&mut app, Vec3::new(-3.0, 2.0, 0.0));
        settle(&mut app);

        set_intent(&mut app, Vec3::NEG_Z, false);
        for _ in 0..300 {
            app.update();
            if climb_phase(&mut app) != ClimbPhase::None {
                break;
            }
        }
        assert_ne!(climb_phase(&mut app), ClimbPhase::None, "never grabbed wall A");

        // Shimmy toward the corner (+X) and keep going.
        set_intent(&mut app, Vec3::X, false);
        let mut saw_transfer = false;
        for _ in 0..600 {
            app.update();
            if climb_phase(&mut app) == ClimbPhase::Transfer {
                saw_transfer = true;
                break;
            }
        }
        assert!(
            saw_transfer,
            "reached the end of the lip and never transferred around the corner              (phase {:?}, pos {:?})",
            climb_phase(&mut app),
            body_pos(&mut app)
        );
    }

    /// The lip running out is a cue to jump, not a hard stop.
    ///
    /// Tested on the PROBE rather than end-to-end. Action selection tries the
    /// corner and the continuing wall before the leap, and any box wide enough
    /// to hang from also has a grabbable end face — so an end-to-end setup
    /// turns the corner instead, which is correct behaviour and tells us
    /// nothing about whether the leap works.
    #[test]
    fn the_leap_probe_crosses_a_gap_and_stops_at_open_air() {
        #[derive(Resource, Default, Debug)]
        struct Found(Option<Vec3>, Option<Vec3>);

        fn probe_system(spatial: SpatialQuery, mut out: ResMut<Found>) {
            let filter = SpatialQueryFilter::default();
            let (metrics, motion) = resolve(&AvatarDescriptor::default(), NOMINAL_BIND_HEIGHT_M);
            let body = AvatarBody {
                metrics,
                motion,
                control: crate::avatar::AvatarControl::LocalPlayer,
                metrics_finalised: false,
            };
            // Hanging on wall A, whose lip ends at x = 0, y = 1.9, z = -2.
            let g = Grip {
                point: Vec3::new(-0.2, 1.9, -2.0),
                normal: Vec3::Z,
                tangent: Vec3::X,
                top: Vec3::new(-0.2, 1.9, -2.2),
                standable: true,
                kind: crate::avatar::grip::GripKind::Ledge,
            };
            out.0 = crate::avatar::climb::probe_leap(&spatial, &filter, &g, &body, Vec3::X)
                .map(|f| f.point);
            // From the FAR end of wall A, heading away from it, there is
            // nothing but open air and flat ground.
            let edge = Grip {
                point: Vec3::new(-5.9, 1.9, -2.0),
                ..g
            };
            out.1 = crate::avatar::climb::probe_leap(&spatial, &filter, &edge, &body, Vec3::NEG_X)
                .map(|f| f.point);
        }

        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        // Wall A ends at x = 0.
        box_at(&mut app, Vec3::new(-3.0, 0.95, -4.0), Vec3::new(6.0, 1.9, 4.0), Quat::IDENTITY);
        // Wall B starts at x = 1.1 — a 1.1 m gap, past a static reach.
        box_at(&mut app, Vec3::new(4.1, 0.95, -4.0), Vec3::new(6.0, 1.9, 4.0), Quat::IDENTITY);
        app.init_resource::<Found>();
        app.add_systems(Update, probe_system);
        step(&mut app, 6);

        let found = app.world().resource::<Found>();
        let across = found.0.expect("no hold found across a 1.1 m gap");
        assert!(
            across.x > 0.9,
            "found something, but not on the far wall: {across:?}"
        );
        assert!(
            found.1.is_none(),
            "invented a hold in open air off the end of the wall: {:?}",
            found.1
        );
    }

    /// "D has some motion in the left leg, but not A."
    ///
    /// Travelling one way but not the other means the two directions are not
    /// being measured the same. The lip scan is what gates a shimmy — no room
    /// reported that way, no traverse — so on a symmetric wall its two halves
    /// must agree.
    #[test]
    fn the_lip_scan_measures_both_directions_the_same() {
        #[derive(Resource, Default, Debug)]
        struct Found((f32, f32));

        fn probe_system(spatial: SpatialQuery, mut out: ResMut<Found>) {
            let filter = SpatialQueryFilter::default();
            let (metrics, motion) = resolve(&AvatarDescriptor::default(), NOMINAL_BIND_HEIGHT_M);
            let body = AvatarBody {
                metrics,
                motion,
                control: crate::avatar::AvatarControl::LocalPlayer,
                metrics_finalised: false,
            };
            // Dead centre of a wall running x = -10..10, lip at 1.9, face z = -2.
            let g = Grip {
                point: Vec3::new(0.0, 1.9, -2.0),
                normal: Vec3::Z,
                tangent: Vec3::X,
                top: Vec3::new(0.0, 1.9, -2.2),
                standable: true,
                kind: crate::avatar::grip::GripKind::Ledge,
            };
            let cfg = crate::avatar::climb::probe_config_for(&body, 0.0);
            out.0 = crate::avatar::grip::edge_extent(&spatial, &filter, &g, &cfg, 6.0, 0.25);
        }

        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        box_at(&mut app, Vec3::new(0.0, 0.95, -4.0), Vec3::new(20.0, 1.9, 4.0), Quat::IDENTITY);
        app.init_resource::<Found>();
        app.add_systems(Update, probe_system);
        step(&mut app, 6);

        let (left, right) = app.world().resource::<Found>().0;
        assert!(
            left > 1.0 && right > 1.0,
            "no room reported either way on a 20 m wall: left {left:.2}, right {right:.2}"
        );
        assert!(
            (left - right).abs() < 0.5,
            "the lip scan disagrees by direction on a symmetric wall:              left {left:.2} vs right {right:.2} — a shimmy will work one way and not the other"
        );
    }


    /// "Jump down with a hand lowering me down." Releasing a ledge that has
    /// another ledge under it should climb down onto it, not fall past it.
    ///
    /// Also probe-level: reaching the upper lip of a two-tier wall end-to-end
    /// depends on the whole ascent working, which other tests already cover.
    #[test]
    fn the_downward_probe_finds_a_shelf_below_and_ignores_a_sheer_face() {
        #[derive(Resource, Default, Debug)]
        struct Found(Option<Vec3>, Option<Vec3>);

        fn probe_system(spatial: SpatialQuery, mut out: ResMut<Found>) {
            let filter = SpatialQueryFilter::default();
            let (metrics, motion) = resolve(&AvatarDescriptor::default(), NOMINAL_BIND_HEIGHT_M);
            let body = AvatarBody {
                metrics,
                motion,
                control: crate::avatar::AvatarControl::LocalPlayer,
                metrics_finalised: false,
            };
            let at = |x: f32| Grip {
                point: Vec3::new(x, 4.2, -3.0),
                normal: Vec3::Z,
                tangent: Vec3::X,
                top: Vec3::new(x, 4.2, -3.2),
                standable: true,
                kind: crate::avatar::grip::GripKind::Ledge,
            };
            // Above the shelf.
            out.0 = crate::avatar::climb::probe_below(&spatial, &filter, &at(0.0), &body)
                .map(|f| f.point);
            // Far along the same wall the face is sheer.
            out.1 = crate::avatar::climb::probe_below(&spatial, &filter, &at(30.0), &body)
                .map(|f| f.point);
        }

        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        // A tall wall, lip at 4.2 m, running a long way in x.
        box_at(&mut app, Vec3::new(0.0, 2.1, -5.0), Vec3::new(80.0, 4.2, 4.0), Quat::IDENTITY);
        // A shelf protruding from it near x = 0, lip at 2.6 m.
        box_at(&mut app, Vec3::new(0.0, 1.3, -2.7), Vec3::new(8.0, 2.6, 0.8), Quat::IDENTITY);
        app.init_resource::<Found>();
        app.add_systems(Update, probe_system);
        step(&mut app, 6);

        let found = app.world().resource::<Found>();
        let shelf = found.0.expect("no lower hold found above an obvious shelf");
        assert!(
            (shelf.y - 2.6).abs() < 0.35,
            "found a lower hold, but not the shelf at 2.60 m: {shelf:?}"
        );
        assert!(
            found.1.is_none(),
            "found something to climb down onto on a sheer face: {:?}",
            found.1
        );
    }

    /// A body that ends up inside geometry must be able to get OUT.
    ///
    /// The blocked-move bisection measures back toward where the body already
    /// is, so when that start point is itself solid every candidate fails and
    /// the only answer available is "stay put" — welding the character inside
    /// the wall permanently. Anything that clipped them once, at a corner or
    /// through a splayed limb, stranded them there for good.
    #[test]
    fn a_body_inside_geometry_escapes_instead_of_being_welded_there() {
        #[derive(Resource, Default, Debug)]
        struct Out(Vec3, bool, Vec3, bool);

        fn probe_system(spatial: SpatialQuery, mut out: ResMut<Out>) {
            let filter = SpatialQueryFilter::default();
            let (metrics, motion) = resolve(&AvatarDescriptor::default(), NOMINAL_BIND_HEIGHT_M);
            let body = AvatarBody {
                metrics,
                motion,
                control: crate::avatar::AvatarControl::LocalPlayer,
                metrics_finalised: false,
            };
            let shape = crate::avatar::climb::clearance_shape(&body);

            // Sunk a few centimetres into a wall face — what clipping actually
            // looks like — and asked to slide along it. The wall's face is at
            // z = 1.0, so the body clears only past z = 1.0 + its own radius.
            let mut tf = Transform::from_translation(Vec3::new(0.0, 2.0, 1.15));
            let moved = crate::avatar::climb::place_body(
                &mut tf,
                Vec3::new(0.2, 2.0, 1.15),
                &spatial,
                &shape,
                &filter,
                metrics.capsule_radius,
            );
            out.0 = tf.translation;
            out.1 = moved;
            out.3 = crate::avatar::locomotion::capsule_fits(
                &spatial,
                &shape,
                tf.translation,
                tf.rotation,
                &filter,
            );
            // Free space stays free: a clear move must still be reported clear.
            let mut clear_tf = Transform::from_translation(Vec3::new(0.0, 30.0, 0.0));
            let _ = crate::avatar::climb::place_body(
                &mut clear_tf,
                Vec3::new(0.0, 31.0, 0.0),
                &spatial,
                &shape,
                &filter,
                metrics.capsule_radius,
            );
            out.2 = clear_tf.translation;
        }

        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        // A wall whose front face is at z = 1.0.
        box_at(&mut app, Vec3::new(0.0, 2.0, 0.0), Vec3::new(10.0, 4.0, 2.0), Quat::IDENTITY);
        app.init_resource::<Out>();
        app.add_systems(Update, probe_system);
        step(&mut app, 6);

        let out = app.world().resource::<Out>();
        assert!(!out.1, "reported a clear move while inside a wall");
        // It moved at all — the bisection alone can only ever say "stay put"
        // from an invalid start, which is what welded the body in place.
        assert!(
            out.0.distance(Vec3::new(0.0, 2.0, 1.15)) > 1e-3,
            "did not move at all from inside the wall: {:?}",
            out.0
        );
        // And it ended up somewhere legal.
        assert!(
            out.3,
            "moved, but is still intersecting the wall: {:?}",
            out.0
        );
        // And an unobstructed move is untouched.
        assert!(
            out.2.distance(Vec3::new(0.0, 31.0, 0.0)) < 1e-3,
            "a move through open air was interfered with: {:?}",
            out.2
        );
    }



    /// Naughty Dog on Uncharted 4: "the feet actually look for an edge instead
    /// of having them just dangling and swinging."
    ///
    /// Tested on the SEARCH itself rather than end-to-end, because
    /// `AvatarFootIk` only attaches once the rig binds and the rig needs the
    /// glTF loader, which this harness does not have. Mechanics are testable
    /// headlessly; limb pose is not, and asserting on it here would be
    /// asserting on a component that never exists.
    #[test]
    fn the_foot_search_finds_a_ledge_and_ignores_a_blank_wall() {
        #[derive(Resource, Default, Debug)]
        struct Found(Option<Vec3>, bool);

        fn probe_system(spatial: SpatialQuery, mut out: ResMut<Found>) {
            let filter = SpatialQueryFilter::default();
            // Face at z = -2, ledge top at y = 0.55 near x = 0.
            out.0 = crate::avatar::ik::find_foothold(
                &spatial,
                &filter,
                Vec3::new(0.0, 0.0, -2.0),
                Vec3::Z,
                1.2,
                0.2,
                0.14,
            );
            // Far along the wall there is no ledge, only blank face.
            out.1 = crate::avatar::ik::find_foothold(
                &spatial,
                &filter,
                Vec3::new(20.0, 0.0, -2.0),
                Vec3::Z,
                1.2,
                0.2,
                0.14,
            )
            .is_some();
        }

        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        box_at(&mut app, Vec3::new(0.0, 0.95, -5.0), Vec3::new(60.0, 1.9, 6.0), Quat::IDENTITY);
        // A foothold protruding from the face, only near x = 0.
        box_at(&mut app, Vec3::new(0.0, 0.5, -1.85), Vec3::new(4.0, 0.1, 0.5), Quat::IDENTITY);
        app.init_resource::<Found>();
        app.add_systems(Update, probe_system);
        step(&mut app, 6);

        let found = app.world().resource::<Found>();
        let p = found.0.expect("no foothold found where a ledge exists");
        assert!(
            (p.y - 0.55).abs() < 0.06,
            "foothold at y={:.2}, expected the ledge top at 0.55",
            p.y
        );
        assert!(
            !found.1,
            "reported a foothold on blank wall — the feet would plant on nothing"
        );
    }

    /// Wedged between two colliders, the controller used to hang in the air
    /// playing a fall that never landed, with no input that helped. It must
    /// free itself.
    #[test]
    fn a_body_wedged_between_two_parts_frees_itself() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        // A wall with a pillar just in front of it, pinched to 0.20 m — far
        // narrower than the capsule's 0.54 m width. This is the shape the
        // player actually got caught in: a slot with a way out sideways, not
        // two infinite planes (which would bury the body rather than pinch it,
        // and is not something a recovery can or should rescue).
        box_at(&mut app, Vec3::new(0.0, 3.0, -1.0), Vec3::new(8.0, 6.0, 0.5), Quat::IDENTITY);
        box_at(&mut app, Vec3::new(0.0, 1.5, -0.35), Vec3::new(0.4, 3.0, 0.4), Quat::IDENTITY);
        // Drop the avatar straight into the slot.
        spawn_at(&mut app, Vec3::new(0.0, 2.0, -0.65));

        // Long enough for gravity to settle it and the recovery to fire.
        step(&mut app, 300);

        let p = body_pos(&mut app);
        let l = loco(&mut app);
        assert!(
            p.is_finite(),
            "wedge recovery produced a non-finite position: {p:?}"
        );
        assert!(
            l.grounded,
            "still pinned in the gap at {p:?} — the fall never resolves"
        );
    }

    /// A hang must never put the body below the surface it grabbed from.
    #[test]
    fn a_hang_never_ends_up_below_the_ground() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        box_at(&mut app, Vec3::new(0.0, 0.95, -4.0), Vec3::new(10.0, 1.9, 4.0), Quat::IDENTITY);
        spawn_at(&mut app, Vec3::new(0.0, 2.0, 0.0));
        settle(&mut app);
        let half = half_extent(&mut app);

        set_intent(&mut app, Vec3::NEG_Z, false);
        for _ in 0..400 {
            app.update();
            let feet = body_pos(&mut app).y - half;
            assert!(
                feet > -0.25,
                "feet reached {feet:.2} m — below the ground plane at 0"
            );
        }
    }

    /// "I can also climb into objects, so collisions still need to work."
    ///
    /// Every climb phase drives the body by hand rather than through the
    /// character controller, so nothing stopped a pull-up from passing straight
    /// through whatever sat above the ledge.
    #[test]
    fn a_mantle_never_pulls_the_body_through_a_ceiling() {
        let mut app = world(AvatarHost::Client);
        ground(&mut app);
        // A standable ledge at 1.9 m.
        box_at(&mut app, Vec3::new(0.0, 0.95, -5.0), Vec3::new(10.0, 1.9, 6.0), Quat::IDENTITY);
        spawn_at(&mut app, Vec3::new(0.0, 2.0, 0.0));
        settle(&mut app);

        set_intent(&mut app, Vec3::NEG_Z, false);
        for _ in 0..300 {
            app.update();
            if climb_phase(&mut app) != ClimbPhase::None {
                break;
            }
        }
        assert_ne!(climb_phase(&mut app), ClimbPhase::None, "never grabbed the ledge");

        // NOW put a slab overhead, underside at 2.40 m. Added after the grab
        // rather than before it so the ledge probe sees the same clear approach
        // as every other test — the thing under test is the pull-up, not the
        // search.
        box_at(&mut app, Vec3::new(0.0, 2.9, -5.0), Vec3::new(10.0, 1.0, 6.0), Quat::IDENTITY);
        step(&mut app, 2);

        // The invariant under test: the body is never inside geometry, and
        // never reaches standing height on a ledge it cannot stand on.
        set_intent(&mut app, Vec3::NEG_Z, false);
        let mut worst: f32 = 0.0;
        for _ in 0..600 {
            app.world_mut().resource_mut::<TestIntent>().jump = true;
            app.update();
            worst = worst.max(body_pos(&mut app).y);
        }

        // The ledge top is 1.9 m and the body is ~0.88 m of half extent, so
        // standing on it means a centre near 2.78 m — which the 2.40 m slab
        // makes impossible. Anything close to that means it topped out anyway.
        assert!(
            worst < 2.5,
            "topped out under a 2.40 m ceiling: highest centre {worst:.2} m"
        );
        // And it is still holding on. A blocked top-out is not a dead end —
        // the character should hang there, not bounce off the wall forever.
        assert!(
            climb_phase(&mut app).is_attached(),
            "let go of a perfectly good ledge because the top was obstructed: {:?} at {:.2} m",
            climb_phase(&mut app),
            body_pos(&mut app).y
        );

    }

}

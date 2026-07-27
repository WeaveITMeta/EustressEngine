//! # The avatar contract — the golden parity fold
//!
//! Both shells build an `App`. This module reduces one to an
//! [`AvatarContract`]: a small, comparable value covering everything that must
//! be identical for "it plays the same in the Client as in Play Mode" to be
//! true.
//!
//! A `SharedCharacterPlugin`-style promise cannot be tested. A fold can:
//! `assert_eq!(studio_contract, client_contract)` either passes or names the
//! field that drifted.
//!
//! ## The assertion that matters most
//!
//! [`AvatarContract::spawned_components`] is the sorted component set of the
//! entity `SpawnAvatar` actually produces. **This is the assertion that would
//! have caught the commented-out physics insert the day it was written** —
//! `RigidBody` and `Collider` simply would not have been in the set, in either
//! shell, and the test would have said so by name.

use bevy::prelude::*;

use super::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
use super::{AvatarHostConfig, SpawnedByAvatarRuntime};
use eustress_avatar_schema::{BodyMetrics, NOMINAL_BIND_HEIGHT_M};

/// Everything that must match between the two shells.
#[derive(Debug, Clone, PartialEq)]
pub struct AvatarContract {
    /// Fixed-timestep rate. The Client ran at Bevy's 64 Hz default while
    /// Studio pinned 60 Hz, so identical inputs integrated differently.
    pub fixed_hz: f64,
    pub gravity_y: f32,
    pub camera_order: isize,
    pub camera_fov_deg: f32,
    /// Sorted short type names on a freshly spawned avatar.
    pub spawned_components: Vec<String>,
    /// Metrics derived from the default descriptor at the nominal bind height.
    pub default_metrics: BodyMetricsKey,
}

/// `BodyMetrics` reduced to exactly-comparable bits. Floats are compared as
/// bit patterns deliberately: the derivation is pure, so "close enough" would
/// hide a real divergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyMetricsKey {
    pub height: u32,
    pub radius: u32,
    pub cylinder_len: u32,
    pub spawn_offset: u32,
    pub eye_height: u32,
}

impl From<&BodyMetrics> for BodyMetricsKey {
    fn from(m: &BodyMetrics) -> Self {
        Self {
            height: m.height_m.to_bits(),
            radius: m.capsule_radius.to_bits(),
            cylinder_len: m.capsule_cylinder_len.to_bits(),
            spawn_offset: m.spawn_center_offset.to_bits(),
            eye_height: m.eye_height.to_bits(),
        }
    }
}

/// Components an avatar MUST carry to be playable.
///
/// Encoded as a required list rather than an exact-equality set so the test
/// stays useful while the runtime grows — but `RigidBody` and `Collider` are
/// in here permanently, because their absence is the defect that made the
/// character unable to stand, walk, fall or jump in either shell.
pub const REQUIRED_AVATAR_COMPONENTS: &[&str] = &[
    "SpawnedByAvatarRuntime",
    "AvatarBody",
    "AvatarIntent",
    "AvatarLocomotion",
    "AvatarDescriptor",
    "Transform",
    "RigidBody",
    "Collider",
    "LinearVelocity",
    "LockedAxes",
];

/// Read the contract off a built `App`.
///
/// Call after `App::update()` has run at least once so spawn messages have
/// been drained.
pub fn avatar_contract(app: &mut App) -> AvatarContract {
    let world = app.world_mut();

    let fixed_hz = world
        .get_resource::<Time<Fixed>>()
        .map(|t| 1.0 / t.timestep().as_secs_f64())
        .unwrap_or(0.0);

    let gravity_y = world
        .get_resource::<avian3d::prelude::Gravity>()
        .map(|g| g.0.y)
        .unwrap_or(f32::NAN);

    let (camera_order, _seams) = world
        .get_resource::<AvatarHostConfig>()
        .map(|c| (c.seams.camera_order, c.seams))
        .unwrap_or((isize::MIN, super::AvatarHost::Client.seams()));

    let spawned_components = spawned_avatar_components(world);

    let default_metrics = BodyMetricsKey::from(
        &super::AvatarDescriptor::default().morphs.metrics(NOMINAL_BIND_HEIGHT_M),
    );

    AvatarContract {
        fixed_hz,
        gravity_y,
        camera_order,
        camera_fov_deg: super::control::AVATAR_FOV_DEG,
        spawned_components,
        default_metrics,
    }
}

/// Which required components are present on the spawned avatar.
///
/// Deliberately **typed queries, not type-name strings**: Bevy 0.19 compiles
/// out component names unless the `debug` feature is on, so a name-based check
/// silently degrades to comparing `"<Enable the debug feature…>"` against
/// itself — which passes while asserting nothing. A missing `RigidBody` must
/// fail loudly in a release-feature build, since that is precisely the defect
/// that shipped.
pub fn spawned_avatar_components(world: &mut World) -> Vec<String> {
    use avian3d::prelude::{Collider, LinearVelocity, LockedAxes, RigidBody};

    let mut root = world.query_filtered::<Entity, With<SpawnedByAvatarRuntime>>();
    if root.iter(world).next().is_none() {
        return Vec::new();
    }

    macro_rules! present {
        ($name:literal, $t:ty) => {{
            let mut q = world.query_filtered::<(), (With<$t>, With<SpawnedByAvatarRuntime>)>();
            if q.iter(world).next().is_some() {
                Some($name.to_string())
            } else {
                None
            }
        }};
    }

    let mut out: Vec<String> = [
        Some("SpawnedByAvatarRuntime".to_string()),
        present!("AvatarBody", AvatarBody),
        present!("AvatarIntent", AvatarIntent),
        present!("AvatarLocomotion", AvatarLocomotion),
        present!("AvatarDescriptor", super::AvatarDescriptor),
        present!("Transform", Transform),
        present!("RigidBody", RigidBody),
        present!("Collider", Collider),
        present!("LinearVelocity", LinearVelocity),
        present!("LockedAxes", LockedAxes),
    ]
    .into_iter()
    .flatten()
    .collect();

    out.sort();
    out
}

/// Assert an avatar is playable. Panics naming the missing components.
pub fn assert_avatar_playable(world: &mut World) {
    let have = spawned_avatar_components(world);
    assert!(
        !have.is_empty(),
        "no entity carrying SpawnedByAvatarRuntime exists — SpawnAvatar never spawned"
    );

    let missing: Vec<&str> = REQUIRED_AVATAR_COMPONENTS
        .iter()
        .copied()
        .filter(|req| !have.iter().any(|h| h == req))
        .collect();

    assert!(
        missing.is_empty(),
        "avatar is missing required components {missing:?}\n\
         present: {have:?}\n\
         (RigidBody/Collider absent means the character cannot stand, walk, fall or jump — \
          this is the defect that shipped as a commented-out insert in skinned_character.rs)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::avatar::{AvatarDescriptor, AvatarHost, SpawnAvatar};

    /// Build a headless app with the avatar runtime.
    ///
    /// No window and no render device, so this runs in CI. The body GLB will
    /// not resolve here — which is exactly right: the physics body must exist
    /// independently of whether the mesh loaded. Coupling them is how "the
    /// player is invisible" and "the player cannot move" became the same bug.
    fn headless(host: AvatarHost) -> App {
        let mut app = App::new();
        // `AssetServer::load` dispatches onto the IO pool. `TaskPoolPlugin`
        // is part of DefaultPlugins/MinimalPlugins; adding the plugins
        // individually leaves the pool uninitialized.
        bevy::tasks::IoTaskPool::get_or_init(Default::default);
        crate::avatar::boot::register_avatar_asset_sources(&mut app);
        app.add_plugins((
            bevy::app::ScheduleRunnerPlugin::default(),
            bevy::time::TimePlugin,
            bevy::transform::TransformPlugin,
            bevy::asset::AssetPlugin::default(),
            // Required: the camera systems read MouseMotion/MouseWheel and
            // ButtonInput. Without InputPlugin those messages are never
            // initialized, every reader fails param validation, and the whole
            // system panics — the same silent-drain failure class that has
            // bitten this repo before.
            bevy::input::InputPlugin,
            // The motion graph reads Assets<AnimationClip>/<AnimationGraph>
            // and drives AnimationPlayer. Without this the anim systems fail
            // param validation ("Resource does not exist") and panic the
            // whole schedule.
            bevy::animation::AnimationPlugin,
        ));
        // Avian is built with `collider-from-mesh`, so its collider backend
        // reads `AssetEvent<Mesh>`. `AssetPlugin` alone does not register
        // `Assets<Mesh>`, which leaves that message uninitialized and panics
        // the whole schedule at param validation.
        app.init_asset::<Mesh>();
        // The spawner loads the body glTF as a `WorldAsset` (Bevy 0.19's
        // replacement for `Scene`). Registering the asset type lets the handle
        // allocate; the file itself need not resolve for the physics body to
        // exist, which is the separation this test is asserting.
        app.init_asset::<WorldAsset>();
        app.insert_resource(Time::<Fixed>::from_hz(60.0));
        app.add_plugins(avian3d::prelude::PhysicsPlugins::default());
        app.insert_resource(avian3d::prelude::Gravity(
            Vec3::NEG_Y * eustress_avatar_schema::GRAVITY_MPS2,
        ));
        app.add_plugins(crate::avatar::AvatarRuntimePlugin::new(host));
        app
    }

    #[test]
    fn spawned_avatar_has_a_physics_body() {
        // The regression guard for the blocker. If someone comments the Avian
        // insert out again, this names RigidBody and Collider and fails.
        let mut app = headless(AvatarHost::Client);
        app.world_mut()
            .write_message(SpawnAvatar::new(AvatarDescriptor::default(), Vec3::ZERO));
        app.update();

        assert_avatar_playable(app.world_mut());
    }

    #[test]
    fn both_hosts_produce_an_identical_avatar_contract() {
        let mut studio = headless(AvatarHost::Studio);
        let mut client = headless(AvatarHost::Client);

        for app in [&mut studio, &mut client] {
            app.world_mut()
                .write_message(SpawnAvatar::new(AvatarDescriptor::default(), Vec3::ZERO));
            app.update();
        }

        let s = avatar_contract(&mut studio);
        let c = avatar_contract(&mut client);

        // Camera order is a legitimate host difference declared in HostSeams —
        // Studio must out-rank the editor camera. Everything else must match.
        assert_eq!(s.fixed_hz, c.fixed_hz, "fixed timestep drifted");
        assert_eq!(s.gravity_y, c.gravity_y, "gravity drifted");
        assert_eq!(s.camera_fov_deg, c.camera_fov_deg, "camera FOV drifted");
        assert_eq!(s.default_metrics, c.default_metrics, "derived body metrics drifted");
        assert_eq!(
            s.spawned_components, c.spawned_components,
            "the two shells spawn structurally different avatars"
        );
    }

    #[test]
    fn host_seams_differ_only_where_declared() {
        let s = AvatarHost::Studio.seams();
        let c = AvatarHost::Client.seams();

        // Studio gates on viewport focus and suppresses its editor camera;
        // the Client does neither. These are the ONLY sanctioned differences.
        assert!(s.gate_input_on_viewport_focus && !c.gate_input_on_viewport_focus);
        assert!(s.suppress_editor_camera && !c.suppress_editor_camera);
        assert!(s.gate_editor_shortcuts && !c.gate_editor_shortcuts);
        assert!(s.camera_order > c.camera_order);
        assert_ne!(s.escape_action, c.escape_action);
    }

    #[test]
    fn despawn_removes_every_avatar() {
        let mut app = headless(AvatarHost::Studio);
        app.world_mut()
            .write_message(SpawnAvatar::new(AvatarDescriptor::default(), Vec3::ZERO));
        app.update();
        assert!(!spawned_avatar_components(app.world_mut()).is_empty());

        app.world_mut().write_message(crate::avatar::DespawnAllAvatars);
        app.update();
        assert!(
            spawned_avatar_components(app.world_mut()).is_empty(),
            "Stop left an avatar behind"
        );
    }

    /// A leaked camera is not cosmetic: `camera_look` used `single_mut()`, so
    /// the second one silently disabled right-drag orbit and first-person look
    /// with nothing in the log. Every respawn leaked one, which meant Play
    /// Mode stop/start broke the camera permanently.
    #[test]
    fn respawning_does_not_accumulate_cameras() {
        use crate::avatar::control::AvatarCamera;

        let mut app = headless(AvatarHost::Client);
        let count = |app: &mut bevy::prelude::App| {
            let world = app.world_mut();
            let mut q = world.query::<&AvatarCamera>();
            q.iter(world).count()
        };

        for cycle in 0..4 {
            app.world_mut()
                .write_message(SpawnAvatar::new(AvatarDescriptor::default(), Vec3::ZERO));
            app.update();
            app.update();
            assert_eq!(
                count(&mut app),
                1,
                "cycle {cycle}: expected exactly one camera after spawn"
            );

            app.world_mut().write_message(crate::avatar::DespawnAllAvatars);
            app.update();
            assert_eq!(count(&mut app), 0, "cycle {cycle}: despawn left a camera behind");
        }
    }

    #[test]
    fn avatar_metrics_are_sane_at_spawn() {
        let mut app = headless(AvatarHost::Client);
        app.world_mut()
            .write_message(SpawnAvatar::new(AvatarDescriptor::default(), Vec3::ZERO));
        app.update();

        let world = app.world_mut();
        let mut q = world.query_filtered::<&AvatarBody, With<SpawnedByAvatarRuntime>>();
        let body = q.iter(world).next().expect("no avatar");
        assert!(body.metrics.is_sane(), "{:?}", body.metrics);
        assert!(body.motion.walk_speed > 0.0);
        assert!(body.motion.jump_velocity() > 0.0);
    }

    #[test]
    fn locomotion_and_intent_components_exist_for_the_animation_layer() {
        // AvatarLocomotion is what the animation graph reads. Nothing wrote
        // its predecessor (`LocomotionController`), which pinned the state
        // machine to Idle and made every blend path unreachable.
        let mut app = headless(AvatarHost::Client);
        app.world_mut()
            .write_message(SpawnAvatar::new(AvatarDescriptor::default(), Vec3::ZERO));
        app.update();

        let world = app.world_mut();
        let mut q =
            world.query_filtered::<(&AvatarIntent, &AvatarLocomotion), With<SpawnedByAvatarRuntime>>();
        assert!(q.iter(world).next().is_some(), "avatar has no intent/locomotion pair");
    }
}

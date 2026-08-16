//! # The one avatar spawner
//!
//! This is the only code in the repo that creates a character entity. Both
//! shells reach it through the [`SpawnAvatar`] message, so the entity they get
//! is the same entity by construction rather than by review.
//!
//! ## What was wrong before
//!
//! `spawn_skinned_character` computed a capsule and then left the entire Avian
//! insert commented out (`plugins/skinned_character.rs:205-215`), under a
//! comment that still read "Spawn the character root with physics". Both hosts
//! defaulted to that path, so the play character had **no `RigidBody` and no
//! `Collider` on either side** — it could not fall, stand, collide, or jump.
//! The one live physics insert in the repo sat in the Client's unreachable
//! procedural branch.
//!
//! ## The trap in the code that was commented out
//!
//! ```ignore
//! Collider::capsule(capsule_radius, capsule_half_height)   // WRONG
//! ```
//!
//! `Collider::capsule(radius, length)` takes the **cylinder length**
//! (`avian3d-0.7.0/src/collision/collider/parry/mod.rs:790`), not a
//! half-height. Uncommenting that line as written would ship a character
//! roughly half the intended height with its mesh floating above it. The
//! dimension field is named `capsule_cylinder_len` so the mistake cannot be
//! made silently at this call site.

use bevy::prelude::*;
use avian3d::prelude::*;

use super::rig::AwaitingRigBind;
use super::{AvatarControl, AvatarDescriptor, AvatarSystems, LocalAvatar, SpawnedByAvatarRuntime};
use super::{DespawnAllAvatars, SpawnAvatar};
use eustress_avatar_schema::{resolve, BodyMetrics, ResolvedMotion, NOMINAL_BIND_HEIGHT_M};

/// Runtime state carried by every avatar.
#[derive(Component, Debug, Clone)]
pub struct AvatarBody {
    pub metrics: BodyMetrics,
    pub motion: ResolvedMotion,
    pub control: AvatarControl,
    /// True once `AvatarRig` has been bound and metrics recomputed against the
    /// measured bind height rather than the nominal.
    pub metrics_finalised: bool,
}

/// Movement request produced by input, consumed by the controller.
///
/// Distinct from the old `MovementIntent` in that `direction` is always
/// already camera-relative and normalised, and nothing outside
/// [`AvatarSystems::Input`] writes it.
#[derive(Component, Debug, Clone, Default)]
pub struct AvatarIntent {
    pub direction: Vec3,
    pub sprint: bool,
    pub crouch: bool,
    /// Edge-triggered; cleared by the controller once consumed so a jump
    /// cannot be double-consumed by two systems in one frame.
    pub jump_pressed: bool,
}

/// Ground/air state and the gait signal that drives animation.
///
/// This is the component nothing wrote in the old code — which pinned the
/// animation state machine to `Idle` forever and made the crossfade, the
/// walk/run blend and the speed-scaling unreachable.
#[derive(Component, Debug, Clone, Default)]
pub struct AvatarLocomotion {
    pub grounded: bool,
    pub ground_normal: Vec3,
    /// Horizontal speed in m/s.
    pub planar_speed: f32,
    /// `planar_speed` normalised against run speed: 0 idle, ~0.5 walk, 1 run.
    pub speed_norm: f32,
    pub vertical_velocity: f32,
    pub air_time: f32,
    pub yaw_rate: f32,
    /// Latched on the grounding edge, 0..1, from touchdown vertical speed.
    ///
    /// Normalised over [`LAND_IMPACT_FULL_MPS`]. The old divisor of 8 m/s
    /// saturated after a 3.3 m fall, so a 4 m drop and a 200 m drop produced
    /// an identical value and nothing downstream could tell them apart.
    pub land_impact: f32,
    /// Raw downward speed at touchdown, m/s. Kept unnormalised because
    /// thresholds (roll, stumble, damage) are naturally expressed in real
    /// units, and a normalised signal cannot express one above its ceiling.
    pub land_speed_mps: f32,
    /// True for exactly the frame the avatar touches down. The edge itself —
    /// `land_impact` decays, so it cannot be used to detect the transition.
    pub just_landed: bool,
}

/// How far below the capsule bottom the sole is planted.
///
/// Slightly into the surface rather than exactly on it: contact reads as
/// planted, whereas the collision skin's worth of clearance reads as hovering.
const SOLE_SINK: f32 = 0.012;

/// The spawned body mesh, so foot calibration can adjust its offset.
#[derive(Component, Debug)]
pub struct AvatarMeshChild(pub Entity);

/// Set until the feet have been grounded against the capsule.
///
/// Waits a few frames so the clip-frame correction and the first animated pose
/// have settled — calibrating against the bind pose would ground the wrong
/// posture.
#[derive(Component, Debug)]
pub struct PendingFootCalibration {
    pub frames_waited: u8,
}

pub(crate) struct AvatarSpawnPlugin;

impl Plugin for AvatarSpawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (handle_spawn_avatar, handle_despawn_all, finalise_metrics_on_bind)
                .in_set(AvatarSystems::Lifecycle),
        )
        // After transform propagation: this is the one place `GlobalTransform`
        // is authoritative for the current frame's animated pose.
        .add_systems(PostUpdate, calibrate_feet_to_ground.after(TransformSystems::Propagate));
    }
}

/// Ground the model by MEASURING where its feet actually are.
///
/// The alternative — deriving the mesh offset from capsule geometry — assumes
/// the model's origin sits exactly at its feet. That held for the raw Mixamo
/// body, but stops holding once the skeleton root is rotated into the clip's
/// authoring frame, and it would break again for any body whose origin is at
/// the hips. Measuring the lowest foot bone and shifting by the difference is
/// correct for all of those without a per-asset special case.
fn calibrate_feet_to_ground(
    mut commands: Commands,
    globals: Query<&GlobalTransform>,
    // Only calibrate once the motion graph is live, so the measured pose is
    // the animated one.
    animated: Query<(), With<super::anim::AvatarMotionGraph>>,
    mut transforms: Query<&mut Transform>,
    mut q: Query<
        (
            Entity,
            &GlobalTransform,
            &AvatarBody,
            &super::rig::AvatarRig,
            &AvatarMeshChild,
            &mut PendingFootCalibration,
        ),
        With<SpawnedByAvatarRuntime>,
    >,
) {
    use super::rig::HumanoidBone;

    for (root, root_global, body, rig, mesh_child, mut pending) in q.iter_mut() {
        // Wait for the ANIMATED pose, not the bind pose.
        //
        // Calibrating 4 frames after spawn measured the un-animated skeleton —
        // before the clips loaded, before the skeleton root was rotated into
        // the clip's frame, and before root motion was pinned. It produced a
        // +1.03 m "correction" that happened to look right while compensating
        // for a pose that no longer existed a second later. Grounding has to be
        // measured against the pose the player will actually see.
        if !animated.contains(root) {
            continue;
        }
        if pending.frames_waited < 20 {
            pending.frames_waited += 1;
            continue;
        }

        // Align the SOLE, not the ankle.
        //
        // Foot and toe BONES sit above the sole — the visible foot geometry
        // hangs below them. Grounding the lowest bone therefore buries the
        // foot mesh by roughly the ankle height. A Mixamo body's model origin
        // IS the sole, and the mesh child carries that origin, so aligning the
        // mesh child is both simpler and correct.
        let Ok(mesh_global) = globals.get(mesh_child.0) else {
            commands.entity(root).remove::<PendingFootCalibration>();
            continue;
        };
        let lowest = mesh_global.translation().y;
        let _ = HumanoidBone::LeftFoot;

        if !lowest.is_finite() {
            commands.entity(root).remove::<PendingFootCalibration>();
            continue;
        }

        // Where the capsule actually meets the ground.
        //
        // Target a hair BELOW the capsule bottom. The capsule rests on a small
        // collision skin, so aligning the sole exactly to it leaves the
        // character hovering by that skin — about a centimetre, which reads as
        // floating. A millimetre or two of sink reads as contact instead, and
        // is invisible.
        let capsule_bottom =
            root_global.translation().y - body.metrics.capsule_half_extent() - SOLE_SINK;
        let delta = capsule_bottom - lowest;

        // Always report. A calibration that stays silent when it finds nothing
        // is indistinguishable from one that never ran — and that ambiguity
        // cost a debugging round when the feet looked wrong but the avatar was
        // in fact correct (the ground collider was the one at fault).
        if delta.abs() > 0.002 {
            if let Ok(mut t) = transforms.get_mut(mesh_child.0) {
                t.translation.y += delta;
                info!(
                    "avatar: feet grounded — mesh offset corrected by {:+.3} m \
                     (lowest foot {:.3}, capsule bottom {:.3})",
                    delta, lowest, capsule_bottom
                );
            }
        } else {
            info!(
                "avatar: feet already grounded within {:.4} m \
                 (lowest foot {:.3}, capsule bottom {:.3}) — no correction",
                delta.abs(),
                lowest,
                capsule_bottom
            );
        }

        commands.entity(root).remove::<PendingFootCalibration>();
    }
}

fn handle_spawn_avatar(
    mut commands: Commands,
    mut events: MessageReader<SpawnAvatar>,
    asset_server: Res<AssetServer>,
) {
    for req in events.read() {
        let desc = req.descriptor().clone();

        // Before the rig binds, metrics use the nominal bind height. They are
        // recomputed in `finalise_metrics_on_bind` from the MEASURED skeleton,
        // so a body whose export scale differs does not silently produce a
        // mis-sized capsule.
        let (metrics, motion) = resolve(&desc, NOMINAL_BIND_HEIGHT_M);
        debug_assert!(metrics.is_sane(), "avatar spawn produced insane metrics: {metrics:?}");

        let root = commands
            .spawn((
                Transform::from_translation(req.at() + Vec3::Y * metrics.spawn_center_offset)
                    .with_rotation(Quat::from_rotation_y(req.yaw())),
                Visibility::default(),
                Name::new("Avatar"),
                // ── The sealed token. Cannot be constructed outside this crate.
                SpawnedByAvatarRuntime(()),
                AwaitingRigBind,
                AvatarBody {
                    metrics,
                    motion,
                    control: req.control(),
                    metrics_finalised: false,
                },
                AvatarIntent::default(),
                AvatarLocomotion::default(),
                desc.clone(),
            ))
            .id();

        if req.control() == AvatarControl::LocalPlayer {
            commands.entity(root).insert(LocalAvatar);
        }

        insert_physics(&mut commands, root, &metrics);

        // Body mesh as a child, dropped so the feet land on the capsule bottom.
        // Bevy 0.19: glTF scenes load as `WorldAsset`, spawned via
        // `WorldAssetRoot` (the old `Scene`/`SceneRoot` pair is gone).
        let scene: Handle<WorldAsset> =
            asset_server.load(format!("{}#Scene0", desc.base_body.body_asset()));
        // Mixamo bodies face +Z in their own space; Bevy's forward is -Z. The
        // facing integrator computes yaw for a -Z-forward convention, so
        // without this the character walks backwards relative to where it
        // looks (user-observed: "facing the wrong direction from movement").
        //
        // Corrected here on the mesh child rather than in the yaw maths, so
        // the physics capsule, the camera, and the facing integrator all keep
        // one consistent convention.
        let mesh_child = commands
            .spawn((
                WorldAssetRoot(scene),
                Transform::from_xyz(0.0, metrics.mesh_offset, 0.0)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
                Visibility::default(),
                Name::new("AvatarMesh"),
                ChildOf(root),
            ))
            .id();

        commands
            .entity(root)
            .insert((AvatarMeshChild(mesh_child), PendingFootCalibration { frames_waited: 0 }));

        info!(
            "avatar: spawned {:?} at {:?} — height {:.2} m, capsule r={:.3} len={:.3}",
            desc.base_body,
            req.at(),
            metrics.height_m,
            metrics.capsule_radius,
            metrics.capsule_cylinder_len
        );
    }
}

/// The physics body. One definition, both shells.
///
/// `RigidBody::Kinematic` + `CustomPositionIntegration` means position is
/// updated only by the move-and-slide pass, never also by the integrator.
/// A dynamic body's result depends on solver iterations, substeps, contact
/// ordering and sleep thresholds — a large surface that can differ between
/// hosts without anyone noticing. Kinematic reduces the parity surface to
/// *inputs*, which the parity test already asserts equal.
///
/// Trade-off, stated because it is a real regression from the dead procedural
/// branch: the character no longer pushes dynamic props for free. That needs
/// an explicit impulse from the move-and-slide `on_hit` callback.
fn insert_physics(commands: &mut Commands, root: Entity, m: &BodyMetrics) {
    commands.entity(root).insert((
        RigidBody::Kinematic,
        // radius, CYLINDER LENGTH — not half-height. See the module doc.
        Collider::capsule(m.capsule_radius, m.capsule_cylinder_len),
        CustomPositionIntegration,
        CollisionMargin(0.02),
        LockedAxes::ROTATION_LOCKED,
        // Zero friction with Min combine: move-and-slide does the sliding.
        // The commented-out `Friction::new(1.0)` would glue the player to
        // every wall it brushed.
        Friction::new(0.0).with_combine_rule(CoefficientCombine::Min),
        Restitution::new(0.0).with_combine_rule(CoefficientCombine::Min),
        Mass(m.mass_kg),
        LinearVelocity::default(),
    ));
}

/// Recompute metrics against the measured bind height once the rig binds, and
/// resize the collider to match.
fn finalise_metrics_on_bind(
    mut commands: Commands,
    mut q: Query<
        (Entity, &mut AvatarBody, &AvatarDescriptor, &super::rig::AvatarRig),
        (With<SpawnedByAvatarRuntime>, Added<super::rig::AvatarRig>),
    >,
    mut transforms: Query<&mut Transform>,
) {
    for (e, mut body, desc, rig) in q.iter_mut() {
        if body.metrics_finalised {
            continue;
        }
        let (metrics, motion) = resolve(desc, rig.bind_height_m);
        if !metrics.is_sane() {
            warn!("avatar: measured bind height {:.3} produced insane metrics, keeping nominal", rig.bind_height_m);
            body.metrics_finalised = true;
            continue;
        }

        let old = body.metrics;
        body.metrics = metrics;
        body.motion = motion;
        body.metrics_finalised = true;

        // Resize the capsule and keep the feet on the ground: the root moves
        // by the change in half-extent, otherwise a re-measured body either
        // sinks or pops.
        commands
            .entity(e)
            .insert(Collider::capsule(metrics.capsule_radius, metrics.capsule_cylinder_len));

        let delta = metrics.spawn_center_offset - old.spawn_center_offset;
        if delta.abs() > 1e-4 {
            if let Ok(mut t) = transforms.get_mut(e) {
                t.translation.y += delta;
            }
        }

        debug!(
            "avatar: metrics finalised from measured bind height {:.3} m (was nominal {:.3})",
            rig.bind_height_m, NOMINAL_BIND_HEIGHT_M
        );
    }
}

fn handle_despawn_all(
    mut commands: Commands,
    mut events: MessageReader<DespawnAllAvatars>,
    q: Query<Entity, With<SpawnedByAvatarRuntime>>,
    // The camera is a SEPARATE entity and deliberately does not carry
    // `SpawnedByAvatarRuntime` — that marker means "is an avatar body" and
    // half the runtime's queries rely on it meaning exactly that. So it has to
    // be despawned explicitly here. It was not, and every respawn leaked one:
    // `camera_look` then failed `single_mut()` and silently returned, which
    // killed right-drag orbit outright. Play Mode stop/start hit the same path.
    cams: Query<Entity, With<super::control::AvatarCamera>>,
) {
    if events.read().next().is_none() {
        return;
    }
    events.clear();
    // `try_despawn`, not `despawn`: the camera-orphan sweep in
    // `spawn_camera_for_new_avatars` can target the same entity in the same
    // frame, and a `get_entity` check cannot see a despawn another system has
    // already QUEUED. The second application then panics the schedule.
    let mut n = 0;
    for e in q.iter() {
        commands.entity(e).try_despawn();
        n += 1;
    }
    let mut c = 0;
    for e in cams.iter() {
        commands.entity(e).try_despawn();
        c += 1;
    }
    if n > 0 || c > 0 {
        info!("avatar: despawned {n} avatar(s) and {c} camera(s)");
    }
}

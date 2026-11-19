//! Simulation LOD applied to Avian, on the machine that simulates the
//! session.
//!
//! The policy (`eustress_common::world_sim::sim_tier`) says which tier each
//! dynamic body earns; this module makes it so, using only Avian's own
//! sleeping, which keeps everything a body is:
//!
//! - **Full:** whatever LOD changed is undone: the body's own
//!   `SleepThreshold` comes back, and a sleep LOD forced is lifted. A sleep
//!   Avian chose itself is left alone.
//! - **Reduced:** the body sleeps at `reduced_sleep_scale` times its own
//!   threshold, so a far body that is nearly still rests sooner. Avian
//!   still simulates every step it is awake.
//! - **Dormant:** its island is put to sleep (`Sleeping`). Avian keeps the
//!   pose, the velocities and every joint; a contact or a script's push
//!   wakes it as it would any sleeping body, and the next pass counts that
//!   in `woken_by_physics` and leaves it awake.
//!
//! What LOD never does: it never adds `RigidBodyDisabled` (weld groups use
//! it for merged parts, `crate::assemblies`, and it switches joints off),
//! never turns a body kinematic (that changes mass and joint behaviour),
//! never touches a merged part (its hub carries it), an avatar, or a
//! focus, and never runs on a replica, whose bodies the host moves. When
//! the session ends, every body is given back as it was.
//!
//! The tier also goes into the tree, quietly, as `__SimTier` on the body's
//! instance, for the script VMs to read; see the design doc for the
//! follow-up that sleeps a Dormant model's scripts.

use std::collections::HashMap;

use avian3d::dynamics::solver::islands::{BodyIslandNode, IslandId};
use avian3d::prelude::{ColliderAabb, LinearVelocity, RigidBody, SleepThreshold, Sleeping, SleepingDisabled};
use bevy::platform::time::Instant;
use bevy::prelude::*;

use eustress_common::avatar::spawn::AvatarBody;
use eustress_common::datamodel::DmValue;
use eustress_common::play_session::{PlayDataModel, PlayRole};
use eustress_common::world_sim::sim_tier::{plan_tiers, BodySample, SimTier, SimTierConfig, SimTierReport, TierSample, SIM_TIER_PROP};
use eustress_common::world_sim::{ensure_core, StreamFocus, WorldSimSet};

use crate::assemblies::AssemblyMember;

/// What LOD changed on a body, so it can be undone exactly.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct SimLodApplied {
    /// The body's own sleep threshold before LOD scaled it.
    pub prior_threshold: SleepThreshold,
    /// LOD inserted `Sleeping`; only then does LOD remove it.
    pub added_sleeping: bool,
}

/// Simulation LOD for Studio's Play and the Player. Both add it; it acts
/// only while a session runs, and only on the Authority.
pub struct SimLodPlugin;

impl Plugin for SimLodPlugin {
    fn build(&self, app: &mut App) {
        ensure_core(app);
        app.insert_resource(SimTierConfig::from_env())
            .add_systems(
                Update,
                (tag_avatar_foci, evaluate_sim_tiers, apply_sim_tiers)
                    .chain()
                    .in_set(WorldSimSet::Tier)
                    .run_if(sim_lod_active),
            )
            .add_systems(Update, release_sim_lod.run_if(not(sim_lod_active)));
    }
}

/// A session runs here, LOD is on, and this machine simulates it.
pub fn sim_lod_active(cfg: Res<SimTierConfig>, role: Option<Res<PlayRole>>, dm: Option<Res<PlayDataModel>>) -> bool {
    cfg.enabled && dm.is_some() && role.map_or(true, |r| *r == PlayRole::Authority)
}

/// Every avatar is a focus: the world is simulated in full around each
/// player's character, the host's and every joined player's.
pub fn tag_avatar_foci(mut commands: Commands, avatars: Query<Entity, (With<AvatarBody>, Without<StreamFocus>)>) {
    for e in avatars.iter() {
        commands.entity(e).insert(StreamFocus);
    }
}

type BodyItem<'a> = (
    Entity,
    &'a GlobalTransform,
    &'a RigidBody,
    Option<&'a LinearVelocity>,
    Option<&'a SimTier>,
    Option<&'a BodyIslandNode>,
    Option<&'a ColliderAabb>,
    Option<&'a SimLodApplied>,
    Has<SleepingDisabled>,
    Has<Sleeping>,
);

/// Decide every dynamic body's tier on the cadence frame. Inserts
/// `SimTier` only where it changed, so the reactor runs for transitions
/// alone. With no focus at all (no camera, no player), nothing changes.
#[allow(clippy::type_complexity)]
pub fn evaluate_sim_tiers(
    mut commands: Commands,
    cfg: Res<SimTierConfig>,
    mut frame: Local<u64>,
    cameras: Query<(&GlobalTransform, &Camera, Option<&Projection>), With<Camera3d>>,
    foci: Query<&GlobalTransform, With<StreamFocus>>,
    bodies: Query<BodyItem<'static>, (Without<AssemblyMember>, Without<AvatarBody>, Without<StreamFocus>)>,
    dm: Option<Res<PlayDataModel>>,
    mut report: ResMut<SimTierReport>,
) {
    *frame = frame.wrapping_add(1);
    if *frame % cfg.cadence_frames.max(1) as u64 != 0 {
        return;
    }
    let t0 = Instant::now();
    let views: Vec<(Vec3, f32)> = cameras
        .iter()
        .filter(|(_, c, _)| c.is_active)
        .map(|(t, _, p)| {
            let fov = match p {
                Some(Projection::Perspective(pp)) => pp.fov,
                _ => std::f32::consts::FRAC_PI_4,
            };
            (t.translation(), fov)
        })
        .collect();
    let points: Vec<Vec3> = views.iter().map(|(p, _)| *p).chain(foci.iter().map(|t| t.translation())).collect();
    report.enabled = true;
    report.role = "authority".into();
    report.foci = points.len();
    if points.is_empty() {
        return;
    }

    let mut entities: Vec<(Entity, Option<SimTier>)> = Vec::new();
    let mut samples: Vec<BodySample> = Vec::new();
    for (e, t, body, vel, tier, island, aabb, applied, never_sleeps, sleeping) in bodies.iter() {
        if *body != RigidBody::Dynamic {
            continue;
        }
        // Physics woke a body LOD put to sleep: leave it awake, say so.
        if let Some(a) = applied {
            if a.added_sleeping && !sleeping {
                report.woken_by_physics += 1;
                commands.entity(e).insert(SimLodApplied { added_sleeping: false, ..*a });
            }
        }
        let p = t.translation();
        let distance = points.iter().map(|f| f.distance(p)).fold(f32::INFINITY, f32::min);
        let radius = aabb.map_or(0.5, |b| (Vec3::from(b.max) - Vec3::from(b.min)).length() * 0.5);
        let screen = views
            .iter()
            .map(|(c, fov)| eustress_common::world_sim::sim_tier::screen_fraction(radius, c.distance(p), *fov))
            .fold(None, |best: Option<f32>, s| Some(best.map_or(s, |b| b.max(s))));
        let island = island.map(|n| n.island_id()).filter(|id| *id != IslandId::PLACEHOLDER).map(|id| id.0 as u64);
        entities.push((e, tier.copied()));
        samples.push(BodySample {
            key: e.to_bits(),
            island,
            distance,
            screen,
            speed: vel.map_or(0.0, |v| v.0.length()),
            current: tier.copied(),
            never_sleeps,
        });
    }
    let (tiers, stats) = plan_tiers(&samples, &cfg);

    let mut counts: HashMap<SimTier, usize> = HashMap::new();
    let mut transitions: Vec<(Entity, SimTier, f32)> = Vec::new();
    for (i, (e, current)) in entities.iter().enumerate() {
        *counts.entry(tiers[i]).or_default() += 1;
        if *current != Some(tiers[i]) {
            commands.entity(*e).insert(tiers[i]);
            transitions.push((*e, tiers[i], samples[i].distance));
        }
    }
    if let (Some(dm), false) = (dm, transitions.is_empty()) {
        let mut g = dm.dm.lock();
        for (e, tier, _) in &transitions {
            if let Some(id) = g.by_entity(e.to_bits()) {
                g.set_prop_quietly(id, SIM_TIER_PROP, DmValue::String(tier.as_str().to_string()));
            }
        }
    }
    report.bodies = samples.len();
    report.full = counts.get(&SimTier::Full).copied().unwrap_or(0);
    report.reduced = counts.get(&SimTier::Reduced).copied().unwrap_or(0);
    report.dormant = counts.get(&SimTier::Dormant).copied().unwrap_or(0);
    report.transitions_last_pass = transitions.len();
    report.transitions_total += transitions.len() as u64;
    report.stats_last_pass = stats;
    for (e, tier, distance) in transitions {
        report.push_recent(TierSample { entity: e.to_bits(), tier, distance });
    }
    report.last_pass_ms = t0.elapsed().as_secs_f32() * 1000.0;
}

/// Apply each tier change to Avian (see the module docs).
#[allow(clippy::type_complexity)]
pub fn apply_sim_tiers(
    mut commands: Commands,
    cfg: Res<SimTierConfig>,
    changed: Query<
        (Entity, &SimTier, Option<&SimLodApplied>, Option<&SleepThreshold>, Has<Sleeping>, Has<SleepingDisabled>),
        Changed<SimTier>,
    >,
    mut report: ResMut<SimTierReport>,
) {
    for (e, tier, applied, threshold, sleeping, never_sleeps) in changed.iter() {
        // Full on a body LOD never touched: nothing to undo.
        if *tier == SimTier::Full && applied.is_none() {
            continue;
        }
        let prior = applied.map(|a| a.prior_threshold).or(threshold.copied()).unwrap_or_default();
        let mut added_sleeping = applied.is_some_and(|a| a.added_sleeping);
        let mut ec = commands.entity(e);
        match tier {
            SimTier::Full => {
                ec.insert(prior);
                if added_sleeping && sleeping {
                    ec.remove::<Sleeping>();
                }
                ec.remove::<SimLodApplied>();
                continue;
            }
            SimTier::Reduced => {
                ec.insert(scaled(prior, cfg.reduced_sleep_scale));
                if added_sleeping && sleeping {
                    ec.remove::<Sleeping>();
                }
                added_sleeping = false;
            }
            SimTier::Dormant => {
                ec.insert(scaled(prior, cfg.reduced_sleep_scale));
                if !sleeping && !never_sleeps {
                    ec.insert(Sleeping);
                    added_sleeping = true;
                    report.forced_sleeps += 1;
                }
            }
        }
        ec.insert(SimLodApplied { prior_threshold: prior, added_sleeping });
    }
}

/// The body's own threshold times `scale` (at least 1x).
pub fn scaled(t: SleepThreshold, scale: f32) -> SleepThreshold {
    let k = if scale.is_finite() { scale.max(1.0) } else { 1.0 };
    SleepThreshold { linear: t.linear * k, angular: t.angular * k }
}

/// Give every body back as it was: on Stop, on a switch to replica, or when
/// LOD is turned off. Nothing to do (an empty query) on every other frame.
pub fn release_sim_lod(
    mut commands: Commands,
    applied: Query<(Entity, &SimLodApplied, Has<Sleeping>)>,
    tiered: Query<Entity, (With<SimTier>, Without<SimLodApplied>)>,
    mut report: ResMut<SimTierReport>,
) {
    for (e, a, sleeping) in applied.iter() {
        let mut ec = commands.entity(e);
        ec.insert(a.prior_threshold);
        if a.added_sleeping && sleeping {
            ec.remove::<Sleeping>();
        }
        ec.remove::<(SimLodApplied, SimTier)>();
    }
    for e in tiered.iter() {
        commands.entity(e).remove::<SimTier>();
    }
    if report.enabled {
        report.enabled = false;
        report.role = "idle".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session with one focus at the origin, LOD every frame.
    fn session(role: PlayRole) -> App {
        let mut app = App::new();
        app.insert_resource(role)
            .insert_resource(PlayDataModel { dm: eustress_common::datamodel::new_shared() })
            .add_plugins(SimLodPlugin);
        app.insert_resource(SimTierConfig { cadence_frames: 1, enabled: true, ..SimTierConfig::default() });
        app.world_mut().spawn((StreamFocus, GlobalTransform::default()));
        app
    }

    fn body(app: &mut App, at: Vec3, vel: Vec3) -> Entity {
        app.world_mut()
            .spawn((RigidBody::Dynamic, GlobalTransform::from_translation(at), LinearVelocity(vel), SleepThreshold::default()))
            .id()
    }

    #[test]
    fn rage_far_still_body_goes_dormant_and_sleeps() {
        let mut app = session(PlayRole::Authority);
        let far = body(&mut app, Vec3::new(2_000.0, 0.0, 0.0), Vec3::ZERO);
        app.update();
        let w = app.world();
        assert_eq!(w.get::<SimTier>(far), Some(&SimTier::Dormant));
        assert!(w.get::<Sleeping>(far).is_some(), "LOD put its island to sleep");
        let a = w.get::<SimLodApplied>(far).expect("recorded");
        assert!(a.added_sleeping);
        assert_eq!(w.get::<LinearVelocity>(far).unwrap().0, Vec3::ZERO, "state kept");
        assert!(w.get::<SleepThreshold>(far).unwrap().linear > SleepThreshold::default().linear);
    }

    #[test]
    fn rage_near_body_stays_full_and_untouched() {
        let mut app = session(PlayRole::Authority);
        let near = body(&mut app, Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO);
        app.update();
        let w = app.world();
        assert_eq!(w.get::<SimTier>(near), Some(&SimTier::Full));
        assert!(w.get::<Sleeping>(near).is_none());
        assert!(w.get::<SimLodApplied>(near).is_none(), "nothing changed, nothing recorded");
        assert_eq!(w.get::<SleepThreshold>(near), Some(&SleepThreshold::default()));
    }

    #[test]
    fn rage_far_moving_body_keeps_simulating() {
        let mut app = session(PlayRole::Authority);
        let mover = body(&mut app, Vec3::new(2_000.0, 0.0, 0.0), Vec3::new(15.0, 0.0, 0.0));
        app.update();
        let w = app.world();
        assert_eq!(w.get::<SimTier>(mover), Some(&SimTier::Reduced));
        assert!(w.get::<Sleeping>(mover).is_none(), "never frozen while moving");
        assert_eq!(w.get::<LinearVelocity>(mover).unwrap().0, Vec3::new(15.0, 0.0, 0.0));
    }

    #[test]
    fn rage_returning_focus_wakes_only_what_lod_put_to_sleep() {
        let mut app = session(PlayRole::Authority);
        let lod_slept = body(&mut app, Vec3::new(2_000.0, 0.0, 0.0), Vec3::ZERO);
        // Avian had already put this one to sleep on its own.
        let self_slept = body(&mut app, Vec3::new(2_000.0, 0.0, 10.0), Vec3::ZERO);
        app.world_mut().entity_mut(self_slept).insert(Sleeping);
        app.update();
        assert!(app.world().get::<SimLodApplied>(self_slept).is_some_and(|a| !a.added_sleeping));
        // The player walks over.
        let mut q = app.world_mut().query_filtered::<&mut GlobalTransform, With<StreamFocus>>();
        for mut t in q.iter_mut(app.world_mut()) {
            *t = GlobalTransform::from_xyz(2_000.0, 0.0, 5.0);
        }
        app.update();
        let w = app.world();
        assert_eq!(w.get::<SimTier>(lod_slept), Some(&SimTier::Full));
        assert!(w.get::<Sleeping>(lod_slept).is_none(), "LOD's sleep lifted");
        assert!(w.get::<Sleeping>(self_slept).is_some(), "Avian's own sleep left alone");
        assert_eq!(w.get::<SleepThreshold>(lod_slept), Some(&SleepThreshold::default()), "threshold restored");
        assert!(w.get::<SimLodApplied>(lod_slept).is_none());
    }

    #[test]
    fn rage_a_replica_never_tiers() {
        let mut app = session(PlayRole::Replica);
        let far = body(&mut app, Vec3::new(2_000.0, 0.0, 0.0), Vec3::ZERO);
        app.update();
        assert!(app.world().get::<SimTier>(far).is_none());
        assert!(app.world().get::<Sleeping>(far).is_none(), "the host simulates it");
    }

    #[test]
    fn rage_physics_wake_is_counted_and_respected() {
        let mut app = session(PlayRole::Authority);
        let far = body(&mut app, Vec3::new(2_000.0, 0.0, 0.0), Vec3::ZERO);
        app.update();
        // A contact wakes it (Avian removes `Sleeping`).
        app.world_mut().entity_mut(far).remove::<Sleeping>();
        app.update();
        let w = app.world();
        assert_eq!(w.resource::<SimTierReport>().woken_by_physics, 1);
        assert!(w.get::<Sleeping>(far).is_none(), "LOD does not fight physics");
        assert!(w.get::<SimLodApplied>(far).is_some_and(|a| !a.added_sleeping));
    }

    #[test]
    fn rage_session_end_gives_every_body_back() {
        let mut app = session(PlayRole::Authority);
        let far = body(&mut app, Vec3::new(2_000.0, 0.0, 0.0), Vec3::ZERO);
        let near = body(&mut app, Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO);
        app.update();
        app.world_mut().remove_resource::<PlayDataModel>();
        app.update();
        let w = app.world();
        for e in [far, near] {
            assert!(w.get::<SimTier>(e).is_none());
            assert!(w.get::<SimLodApplied>(e).is_none());
            assert!(w.get::<Sleeping>(e).is_none());
            assert_eq!(w.get::<SleepThreshold>(e), Some(&SleepThreshold::default()));
        }
    }

    #[test]
    fn rage_tier_reaches_the_tree_quietly() {
        let mut app = session(PlayRole::Authority);
        let far = body(&mut app, Vec3::new(2_000.0, 0.0, 0.0), Vec3::ZERO);
        let dm = app.world().resource::<PlayDataModel>().dm.clone();
        let id = {
            let mut g = dm.lock();
            let ws = g.get_service("Workspace").unwrap();
            g.create_bound("Part", "Crate", far.to_bits(), Some(ws), Vec::new())
        };
        app.update();
        let g = dm.lock();
        assert_eq!(g.get_prop(id, SIM_TIER_PROP), Some(DmValue::String("Dormant".into())));
    }
}

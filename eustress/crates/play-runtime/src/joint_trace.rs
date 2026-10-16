//! # A trace of Play's joints and weld groups
//!
//! For a body that flies off in Studio and nowhere else: a log of what the weld
//! groups and the scripted joints did, and of the bodies they moved, close
//! enough to see which joint or contact threw what.
//!
//! Off unless, when a Play session starts, the environment has
//! `EUSTRESS_JOINT_TRACE=1` or the file `~/.eustress_engine/joint_trace.on`
//! exists. On, it logs (every line starts `[JT]`):
//!
//! - every weld-group plan that changes something: each group's body, its parts
//!   and where they sit on it, and every joint on those bodies;
//! - every scripted constraint built, or waiting for a body: class, bodies, frames;
//! - for the 3 s after either, every physics step: the three fastest bodies, and
//!   every body whose speed changed by more than 15 m/s in that step, with what
//!   it is and the joints on it;
//! - any body faster than 40 m/s, or with a pose that is not finite, at any
//!   time, once per body per second.

use std::collections::HashMap;

use avian3d::prelude::*;
use bevy::math::Isometry3d;
use bevy::prelude::*;

use eustress_common::classes::Instance;

use crate::assemblies::AssemblyMember;
use crate::joints::SpringJoint;

/// How long after a plan or a build every step is traced, s.
const WATCH_SECS: f64 = 3.0;
/// A change of speed within one step that is traced, m/s.
const JUMP: f32 = 15.0;
/// A speed that is traced at any time, m/s.
const FAST: f32 = 40.0;

/// Traces the session's bodies after the joints change (see the module docs).
pub struct JointTracePlugin;

impl Plugin for JointTracePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<JointTrace>()
            .add_systems(FixedPostUpdate, watch_bodies.after(PhysicsSystems::Writeback).run_if(|t: Res<JointTrace>| t.on));
    }
}

/// Whether the trace is on this session, and what it is watching.
#[derive(Resource, Default)]
pub struct JointTrace {
    pub on: bool,
    /// Every step is traced until this time (the fixed clock), s.
    watch_until: f64,
    /// Each body's speed at the last step.
    last_speed: HashMap<Entity, f32>,
    /// When each body was last reported for being fast, s.
    reported: HashMap<Entity, f64>,
}

/// Whether the trace was asked for: the environment variable, or the file.
pub fn asked_for() -> bool {
    if std::env::var("EUSTRESS_JOINT_TRACE").is_ok_and(|v| !v.is_empty() && v != "0") {
        return true;
    }
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .is_some_and(|home| std::path::Path::new(&home).join(".eustress_engine").join("joint_trace.on").exists())
}

/// A new Play session: the trace is on for it when asked for.
pub(crate) fn start_session(world: &mut World) {
    let on = asked_for();
    let mut trace = world.get_resource_or_insert_with(JointTrace::default);
    *trace = JointTrace { on, ..default() };
    if on {
        info!("[JT] joint trace on for this Play session (EUSTRESS_JOINT_TRACE or ~/.eustress_engine/joint_trace.on)");
    }
}

fn on(world: &World) -> bool {
    world.get_resource::<JointTrace>().is_some_and(|t| t.on)
}

/// Every step is traced for the next few seconds.
fn watch(world: &mut World) {
    let now = world.resource::<Time<Fixed>>().elapsed_secs_f64();
    if let Some(mut trace) = world.get_resource_mut::<JointTrace>() {
        trace.watch_until = now + WATCH_SECS;
    }
}

fn v3(v: Vec3) -> String {
    format!("({:.3}, {:.3}, {:.3})", v.x, v.y, v.z)
}

fn pose(p: Isometry3d) -> String {
    let (axis, angle) = p.rotation.to_axis_angle();
    format!("at {} turned {:.1} deg about {}", v3(Vec3::from(p.translation)), angle.to_degrees(), v3(axis))
}

/// What a body is: its name, kind, mass, and whether it is merged into another.
pub fn describe(world: &World, e: Entity) -> String {
    let Ok(r) = world.get_entity(e) else { return format!("{e} (gone)") };
    let name = r.get::<Name>().map_or_else(|| "?".to_string(), |n| n.as_str().to_string());
    let kind = r.get::<RigidBody>().map_or("no body".to_string(), |b| format!("{b:?}"));
    let mut s = format!("{name} {e} {kind}");
    if r.contains::<RigidBodyDisabled>() {
        s += " body off";
    }
    if let Some(m) = r.get::<AssemblyMember>() {
        s += &format!(" merged into {} {}", describe_name(world, m.hub), pose(m.in_hub));
    }
    if let Some(c) = r.get::<ColliderOf>().filter(|c| c.body != e) {
        s += &format!(" collider on {}", c.body);
    }
    if r.contains::<Sensor>() {
        s += " sensor";
    }
    if r.contains::<ColliderDisabled>() {
        s += " collider off";
    }
    if let Some(m) = r.get::<ComputedMass>() {
        s += &format!(" {:.1} kg", m.value());
    }
    if let (Some(p), Some(rot)) = (r.get::<Position>(), r.get::<Rotation>()) {
        s += &format!(" {}", pose(Isometry3d::new(p.0, rot.0)));
    }
    if let (Some(v), Some(w)) = (r.get::<LinearVelocity>(), r.get::<AngularVelocity>()) {
        s += &format!(" v {} ({:.2} m/s) w {:.2} rad/s", v3(v.0), v.0.length(), w.0.length());
    }
    s
}

fn describe_name(world: &World, e: Entity) -> String {
    world.get::<Name>(e).map_or_else(|| format!("{e}"), |n| format!("{} {e}", n.as_str()))
}

/// Every joint on any of `bodies`: its type, bodies, and who made it.
pub fn joints_on(world: &mut World, bodies: &[Entity]) -> Vec<String> {
    let mut lines = Vec::new();
    macro_rules! scan {
        ($ty:ty, $label:expr) => {{
            let mut q = world.query::<(Entity, &$ty, Has<JointDisabled>, Option<&Instance>, Option<&Name>)>();
            for (e, j, off, inst, name) in q.iter(world) {
                if bodies.contains(&j.body1) || bodies.contains(&j.body2) {
                    let who = inst.map_or_else(|| name.map_or("scripted".to_string(), |n| n.as_str().to_string()), |i| format!("loaded {}", i.name));
                    lines.push(format!("{} {e} {} {} -> {}{}", $label, who, j.body1, j.body2, if off { " (off)" } else { "" }));
                }
            }
        }};
    }
    scan!(FixedJoint, "Fixed");
    scan!(RevoluteJoint, "Revolute");
    scan!(PrismaticJoint, "Prismatic");
    scan!(SphericalJoint, "Spherical");
    scan!(DistanceJoint, "Distance");
    scan!(SpringJoint, "Spring");
    lines
}

/// The weld groups were planned again and changed something.
pub(crate) fn plan(world: &mut World, members: &HashMap<Entity, (Entity, Isometry3d)>, bodies: &[Entity], leaving: &[Entity], joining: usize) {
    if !on(world) {
        return;
    }
    let mut hubs: Vec<Entity> = members.values().map(|m| m.0).collect();
    hubs.sort();
    hubs.dedup();
    info!("[JT] weld groups planned: {} merged groups, {} parts merged ({joining} joining, {} leaving), {} bodies welded", hubs.len(), members.len(), leaving.len(), bodies.len());
    for &hub in &hubs {
        info!("[JT] group body: {}", describe(world, hub));
        let mut parts: Vec<(&Entity, &(Entity, Isometry3d))> = members.iter().filter(|(_, m)| m.0 == hub).collect();
        parts.sort_by_key(|(e, _)| **e);
        for (part, (_, in_hub)) in parts {
            info!("[JT]   part {} {}", describe_name(world, *part), pose(*in_hub));
        }
    }
    for e in leaving {
        info!("[JT] left its group: {}", describe(world, *e));
    }
    for line in joints_on(world, bodies) {
        info!("[JT]   joint {line}");
    }
    watch(world);
}

/// A scripted constraint was built, or waits for a body to simulate.
pub(crate) fn built(world: &mut World, what: &str, class: &str, ends: Option<(Entity, Entity, Isometry3d, Isometry3d)>, waiting: Option<[Entity; 2]>) {
    if !on(world) {
        return;
    }
    match (ends, waiting) {
        (Some((b0, b1, f0, f1)), _) => info!(
            "[JT] built {class} {what}: {} frame {} | {} frame {}",
            describe_name(world, b0),
            pose(f0),
            describe_name(world, b1),
            pose(f1)
        ),
        (None, Some([b0, b1])) => info!("[JT] {class} {what} waits: neither {} nor {} simulates", describe_name(world, b0), describe_name(world, b1)),
        (None, None) => info!("[JT] {class} {what}: no joint"),
    }
    watch(world);
}

/// Each step while watching: the fastest bodies and the ones whose speed
/// jumped; at any time, a body that is too fast or not finite.
fn watch_bodies(world: &mut World) {
    let now = world.resource::<Time<Fixed>>().elapsed_secs_f64();
    let watching = world.resource::<JointTrace>().watch_until > now;
    let mut q = world.query_filtered::<(Entity, &LinearVelocity, &Position), (With<RigidBody>, Without<RigidBodyDisabled>)>();
    let mut speeds: Vec<(Entity, f32, bool)> = q.iter(world).map(|(e, v, p)| (e, v.0.length(), p.0.is_finite() && v.0.is_finite())).collect();
    let (jumps, alarms) = {
        let mut trace = world.resource_mut::<JointTrace>();
        let mut jumps = Vec::new();
        let mut alarms = Vec::new();
        for &(e, speed, finite) in &speeds {
            let last = trace.last_speed.insert(e, speed).unwrap_or(speed);
            if watching && (speed - last).abs() > JUMP {
                jumps.push((e, last, speed));
            }
            if (!finite || speed > FAST) && trace.reported.get(&e).is_none_or(|t| now - t >= 1.0) {
                trace.reported.insert(e, now);
                alarms.push(e);
            }
        }
        (jumps, alarms)
    };
    if watching {
        speeds.sort_by(|a, b| b.1.total_cmp(&a.1));
        let top: Vec<String> = speeds.iter().take(3).map(|(e, s, _)| format!("{} {s:.2} m/s", describe_name(world, *e))).collect();
        info!("[JT] t {now:.3} fastest: {}", top.join(", "));
    }
    for (e, last, speed) in jumps {
        info!("[JT] t {now:.3} speed jumped {last:.2} -> {speed:.2} m/s: {}", describe(world, e));
        for line in joints_on(world, &[e]) {
            info!("[JT]     joint {line}");
        }
    }
    for e in alarms {
        warn!("[JT] t {now:.3} too fast or not finite: {}", describe(world, e));
        for line in joints_on(world, &[e]) {
            warn!("[JT]     joint {line}");
        }
    }
}

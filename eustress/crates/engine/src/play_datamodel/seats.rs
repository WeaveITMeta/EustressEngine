//! # Seats: who sits where
//!
//! The host decides (`docs/networking/SEATS.md`). A `Seat` or `VehicleSeat`
//! takes a character whose root touches it, or one a script asked for with
//! `Seat:Sit`; its occupant leaves on a jump the Humanoid may make, or when a
//! script, the seat or the character's death ends it. Seating is ordinary
//! writes, so every Player sees the same `Occupant`, `Sit`, `SeatPart` and
//! `SeatWeld`, and `Humanoid.Seated` fires with Roblox's arguments.
//!
//! The avatar standing for the occupant rides the seat
//! (`avatar::seat::AvatarSeated`): here, the host's own avatar or a joined
//! player's replica; on a Player, the same writes reach `net_replica`, which
//! seats the avatars it draws.
//!
//! The record of who sits where is compared with the tree every frame, so a
//! script's write ends a seating however it was made.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use avian3d::prelude::CollisionEventsEnabled;
use bevy::prelude::*;

use eustress_common::avatar::seat::{seat_offset, AvatarSeated};
use eustress_common::classes::{ClassName, Instance};
use eustress_common::avatar::spawn::AvatarBody;
use eustress_common::datamodel::{DataModel, DmEvent, DmValue, InstanceId, SitRequest};
use eustress_common::scripting::{CFrame, Vector3};

use super::{PlayDataModel, PlayScriptSet};

/// Seconds a Humanoid that left a seat waits before any seat takes it again.
const SIT_COOLDOWN: f64 = 1.0;

pub struct SeatPlugin;

impl Plugin for SeatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Seating>()
            .add_systems(
                Update,
                seat_characters
                    .in_set(PlayScriptSet::Pull)
                    .after(super::pull::pull_collisions)
                    .after(super::remote_players::pull_remote_characters),
            )
            .add_systems(Update, report_seat_touches.in_set(PlayScriptSet::Pull));
    }
}

/// A seat takes a character whose root touches it, and physics reports a
/// collider's contacts only when it asks (`CollisionEventsEnabled`), which
/// otherwise happens only once a script listens to its Touched. So in Play
/// every Seat and VehicleSeat asks: all of them when the session starts, and
/// each one that arrives later (streamed in). What Play turned on carries
/// `TouchReportingForPlay`, and Stop turns it off again; a seat that asked
/// already is left as it was. Seats scripts make ask from the start.
fn report_seat_touches(
    mut commands: Commands,
    play: Option<Res<PlayDataModel>>,
    mut session: Local<Option<Weak<parking_lot::Mutex<DataModel>>>>,
    all: Query<(Entity, &Instance), Without<CollisionEventsEnabled>>,
    arrived: Query<(Entity, &Instance), (Added<Instance>, Without<CollisionEventsEnabled>)>,
) {
    let Some(play) = play else {
        *session = None;
        return;
    };
    let current = session.as_ref().is_some_and(|w| w.upgrade().is_some_and(|s| Arc::ptr_eq(&s, &play.dm)));
    let is_seat = |i: &Instance| matches!(i.class_name, ClassName::Seat | ClassName::VehicleSeat);
    let seats: Vec<Entity> = if current {
        arrived.iter().filter(|(_, i)| is_seat(i)).map(|(e, _)| e).collect()
    } else {
        *session = Some(Arc::downgrade(&play.dm));
        all.iter().filter(|(_, i)| is_seat(i)).map(|(e, _)| e).collect()
    };
    for e in seats {
        commands.entity(e).insert((CollisionEventsEnabled, super::apply::TouchReportingForPlay));
    }
}

struct Occupancy {
    seat: InstanceId,
    root: InstanceId,
    weld: InstanceId,
    /// The avatar riding, when this machine has one for the character.
    avatar: Option<Entity>,
    /// The jump key was down last frame: only a new press leaves.
    jump_held: bool,
}

#[derive(Resource, Default)]
struct Seating {
    by_humanoid: HashMap<InstanceId, Occupancy>,
    /// When each Humanoid last left a seat, on the tree's clock.
    left_at: HashMap<InstanceId, f64>,
    cursor: u64,
    session: usize,
}

/// A character's Humanoid and root, from its root part: a
/// `HumanoidRootPart` whose Model holds a Humanoid.
fn rider(g: &DataModel, part: InstanceId) -> Option<(InstanceId, InstanceId)> {
    if g.name_of(part) != Some("HumanoidRootPart") {
        return None;
    }
    let model = g.parent(part).filter(|m| g.class_of(*m) == Some("Model"))?;
    let humanoid = g.find_first_child_of_class(model, "Humanoid", false)?;
    Some((humanoid, part))
}

fn is_seat(g: &DataModel, id: InstanceId) -> bool {
    matches!(g.class_of(id), Some("Seat" | "VehicleSeat"))
}

fn bool_prop(g: &DataModel, id: InstanceId, name: &str, default: bool) -> bool {
    g.get_prop(id, name).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn number_prop(g: &DataModel, id: InstanceId, name: &str, default: f64) -> f64 {
    g.get_prop(id, name).and_then(|v| v.as_number()).unwrap_or(default)
}

/// The Humanoid may jump: `JumpEnabled`, and a jump above zero.
fn may_jump(g: &DataModel, humanoid: InstanceId) -> bool {
    let amount = if bool_prop(g, humanoid, "UseJumpPower", false) {
        number_prop(g, humanoid, "JumpPower", 0.0)
    } else {
        number_prop(g, humanoid, "JumpHeight", 0.0)
    };
    bool_prop(g, humanoid, "JumpEnabled", true) && amount > 0.0
}

/// The jump key for the player whose character holds `humanoid`: the
/// host's own keyboard for its player, the input lane for a joined one.
fn jump_key(g: &DataModel, humanoid: InstanceId) -> bool {
    let Some(model) = g.parent(humanoid) else { return false };
    let Some(players) = g.find_service("Players") else { return false };
    let player = g
        .children(players)
        .iter()
        .copied()
        .find(|p| g.get_prop(*p, "Character").and_then(|v| v.as_instance()) == Some(model));
    match player {
        Some(p) if Some(p) == g.local_player => g.input.keys.contains("Space"),
        Some(p) => g.player_input.get(&p).is_some_and(|i| i.keys.contains("Space")),
        None => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn seat_characters(
    mut commands: Commands,
    dm: Option<Res<PlayDataModel>>,
    mut seating: ResMut<Seating>,
    bodies: Query<&AvatarBody>,
) {
    let Some(dm) = dm else {
        seating.by_humanoid.clear();
        seating.left_at.clear();
        return;
    };
    let seating = &mut *seating;
    let mut g = dm.dm.lock();
    let tree = Arc::as_ptr(&dm.dm) as usize;
    if seating.session != tree {
        seating.session = tree;
        seating.by_humanoid.clear();
        seating.left_at.clear();
        seating.cursor = g.event_cursor();
    }
    let now = g.frame.time;

    // Leaving, from the record against the tree.
    let seated: Vec<InstanceId> = seating.by_humanoid.keys().copied().collect();
    for humanoid in seated {
        let o = &seating.by_humanoid[&humanoid];
        let (seat, root, weld) = (o.seat, o.root, o.weld);
        let key = jump_key(&g, humanoid);
        let pressed = key && !o.jump_held;
        let script_jump = bool_prop(&g, humanoid, "Jump", false);
        let intact = g.exists(humanoid)
            && g.exists(root)
            && g.exists(seat)
            && g.exists(weld)
            && number_prop(&g, humanoid, "Health", 100.0) > 0.0
            && bool_prop(&g, humanoid, "Sit", false)
            && !bool_prop(&g, seat, "Disabled", false)
            && g.get_prop(seat, "Occupant").and_then(|v| v.as_instance()) == Some(humanoid)
            && g.get_prop(humanoid, "SeatPart").and_then(|v| v.as_instance()) == Some(seat);
        let jumps = (pressed || script_jump) && may_jump(&g, humanoid);
        if script_jump {
            let _ = g.set_prop(humanoid, "Jump", DmValue::Bool(false));
        }
        if intact && !jumps {
            if let Some(o) = seating.by_humanoid.get_mut(&humanoid) {
                o.jump_held = key;
            }
            continue;
        }
        let o = seating.by_humanoid.remove(&humanoid).expect("listed above");
        release(&mut commands, &mut g, humanoid, &o);
        seating.left_at.insert(humanoid, now);
    }

    // Sitting: this frame's touches, then the scripts' requests.
    let (events, next) = g.events_since(seating.cursor);
    seating.cursor = next;
    let mut asks: Vec<SitRequest> = Vec::new();
    for e in events {
        if let DmEvent::Touched { part, other } = e {
            let (seat, rider_part) = if is_seat(&g, part) { (part, other) } else if is_seat(&g, other) { (other, part) } else { continue };
            if let Some((humanoid, _)) = rider(&g, rider_part) {
                asks.push(SitRequest { seat, humanoid });
            }
        }
    }
    asks.extend(std::mem::take(&mut g.sit_requests));
    for SitRequest { seat, humanoid } in asks {
        let free = g.exists(seat)
            && is_seat(&g, seat)
            && !bool_prop(&g, seat, "Disabled", false)
            && g.get_prop(seat, "Occupant").and_then(|v| v.as_instance()).is_none();
        let ready = g.exists(humanoid)
            && g.class_of(humanoid) == Some("Humanoid")
            && number_prop(&g, humanoid, "Health", 100.0) > 0.0
            && !bool_prop(&g, humanoid, "Sit", false)
            && !seating.by_humanoid.contains_key(&humanoid)
            && seating.left_at.get(&humanoid).is_none_or(|t| now - t >= SIT_COOLDOWN);
        if !(free && ready) {
            continue;
        }
        let Some(root) = g.parent(humanoid).and_then(|m| g.find_first_child(m, "HumanoidRootPart", false)) else { continue };
        // The avatar standing for the character, and the seat's body.
        let avatar = g.entity_of(root).map(Entity::from_bits).filter(|e| bodies.contains(*e));
        let seat_entity = g.entity_of(seat).map(Entity::from_bits);
        let size_y = g.get_prop(seat, "Size").and_then(|v| v.as_vector3()).map_or(1.0, |s| s.y as f32);
        let offset = avatar.and_then(|a| bodies.get(a).ok()).map(|b| seat_offset(size_y, &b.metrics));
        let weld = sit(&mut g, seat, humanoid, root, offset.map_or(size_y / 2.0, |o| o.translation.y));
        if let (Some(avatar), Some(seat_entity), Some(offset)) = (avatar, seat_entity, offset) {
            commands.entity(avatar).insert(AvatarSeated { seat: seat_entity, offset });
        }
        seating.by_humanoid.insert(humanoid, Occupancy { seat, root, weld, avatar, jump_held: jump_key(&g, humanoid) });
    }
}

/// The writes that seat `humanoid` in `seat`, in one frame: `Occupant`,
/// `Sit`, `SeatPart`, a `SeatWeld` record, and `Seated`. Returns the weld.
fn sit(g: &mut DataModel, seat: InstanceId, humanoid: InstanceId, root: InstanceId, height: f32) -> InstanceId {
    let _ = g.set_prop(seat, "Occupant", DmValue::Instance(humanoid));
    let _ = g.set_prop(humanoid, "Sit", DmValue::Bool(true));
    let _ = g.set_prop(humanoid, "SeatPart", DmValue::Instance(seat));
    let weld = g.create("Weld");
    let _ = g.rename(weld, "SeatWeld");
    let mut c0 = CFrame::from_quaternion([0.0, 0.0, 0.0, 1.0]);
    c0.position = Vector3::new(0.0, height as f64, 0.0);
    let _ = g.set_prop(weld, "Part0", DmValue::Instance(seat));
    let _ = g.set_prop(weld, "Part1", DmValue::Instance(root));
    let _ = g.set_prop(weld, "C0", DmValue::CFrame(c0));
    let _ = g.set_parent(weld, Some(seat));
    g.push_event(DmEvent::Signal { id: humanoid, name: "Seated".into(), args: vec![DmValue::Bool(true), DmValue::Instance(seat)] });
    weld
}

/// Undo a seating: whatever of it the tree still has.
fn release(commands: &mut Commands, g: &mut DataModel, humanoid: InstanceId, o: &Occupancy) {
    if g.exists(o.seat) && g.get_prop(o.seat, "Occupant").and_then(|v| v.as_instance()) == Some(humanoid) {
        let _ = g.set_prop(o.seat, "Occupant", DmValue::Nil);
    }
    if g.exists(humanoid) {
        if bool_prop(g, humanoid, "Sit", false) {
            let _ = g.set_prop(humanoid, "Sit", DmValue::Bool(false));
        }
        if g.get_prop(humanoid, "SeatPart").and_then(|v| v.as_instance()).is_some() {
            let _ = g.set_prop(humanoid, "SeatPart", DmValue::Nil);
        }
        g.push_event(DmEvent::Signal { id: humanoid, name: "Seated".into(), args: vec![DmValue::Bool(false), DmValue::Nil] });
    }
    if g.exists(o.weld) {
        g.destroy(o.weld);
    }
    if let Some(avatar) = o.avatar {
        if let Ok(mut e) = commands.get_entity(avatar) {
            e.remove::<AvatarSeated>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(g: &mut DataModel, name: &str) -> (InstanceId, InstanceId) {
        let ws = g.get_service("Workspace").unwrap();
        let model = g.create_virtual("Model", name, Some(ws));
        let root = g.create_virtual("Part", "HumanoidRootPart", Some(model));
        let humanoid = g.create_virtual("Humanoid", "Humanoid", Some(model));
        (humanoid, root)
    }

    #[test]
    fn only_a_characters_root_rides() {
        let mut g = DataModel::new();
        let (humanoid, root) = character(&mut g, "Alice");
        assert_eq!(rider(&g, root), Some((humanoid, root)));
        let hat = g.create_virtual("Part", "Hat", g.parent(root));
        assert_eq!(rider(&g, hat), None, "another part of the character");
        let ws = g.get_service("Workspace").unwrap();
        let loose = g.create_virtual("Part", "HumanoidRootPart", Some(ws));
        assert_eq!(rider(&g, loose), None, "a root with no character around it");
    }

    #[test]
    fn a_jump_leaves_only_when_the_humanoid_may_jump() {
        let mut g = DataModel::new();
        let (humanoid, _) = character(&mut g, "Alice");
        assert!(may_jump(&g, humanoid), "the defaults jump");
        g.set_prop(humanoid, "JumpEnabled", DmValue::Bool(false)).unwrap();
        assert!(!may_jump(&g, humanoid), "a game that keeps its driver in");
        g.set_prop(humanoid, "JumpEnabled", DmValue::Bool(true)).unwrap();
        g.set_prop(humanoid, "JumpHeight", DmValue::Number(0.0)).unwrap();
        assert!(!may_jump(&g, humanoid));
        g.set_prop(humanoid, "UseJumpPower", DmValue::Bool(true)).unwrap();
        assert!(may_jump(&g, humanoid), "JumpPower counts under UseJumpPower");
    }

    #[test]
    fn sitting_is_four_writes_and_a_seated_signal() {
        let mut g = DataModel::new();
        let (humanoid, root) = character(&mut g, "Alice");
        let ws = g.get_service("Workspace").unwrap();
        let seat = g.create("VehicleSeat");
        g.set_parent(seat, Some(ws)).unwrap();
        let cursor = g.event_cursor();
        let weld = sit(&mut g, seat, humanoid, root, 1.25);
        assert_eq!(g.get_prop(seat, "Occupant"), Some(DmValue::Instance(humanoid)));
        assert_eq!(g.get_prop(humanoid, "Sit"), Some(DmValue::Bool(true)));
        assert_eq!(g.get_prop(humanoid, "SeatPart"), Some(DmValue::Instance(seat)));
        assert_eq!((g.name_of(weld), g.parent(weld)), (Some("SeatWeld"), Some(seat)));
        assert_eq!(g.get_prop(weld, "Part1"), Some(DmValue::Instance(root)));
        let c0 = g.get_prop(weld, "C0").and_then(|v| v.as_cframe()).unwrap();
        assert!((c0.position.y - 1.25).abs() < 1e-6);
        let (events, _) = g.events_since(cursor);
        let seated = events.iter().any(|e| {
            matches!(e, DmEvent::Signal { id, name, args } if *id == humanoid && name == "Seated" && args.first() == Some(&DmValue::Bool(true)))
        });
        assert!(seated, "Humanoid.Seated(true, seat)");
    }

    /// A seat loaded from the Space, with no script listening to its
    /// Touched, reports touches in Play, so it seats whoever touches it; a
    /// plain part does not, and a seat that already asked is left as it was
    /// (Stop turns off only what Play turned on).
    #[test]
    fn authored_seats_report_touches_in_play() {
        let instance = |name: &str, class_name| Instance {
            name: name.into(),
            class_name,
            archivable: true,
            id: 0,
            uuid: String::new(),
            ai: false,
        };
        let mut app = App::new();
        app.add_systems(Update, report_seat_touches);
        let seat = app.world_mut().spawn(instance("Seat", ClassName::Seat)).id();
        let driver = app.world_mut().spawn(instance("DriveSeat", ClassName::VehicleSeat)).id();
        let part = app.world_mut().spawn(instance("Floor", ClassName::Part)).id();
        let asked = app.world_mut().spawn((instance("Listened", ClassName::Seat), CollisionEventsEnabled)).id();

        app.update();
        let w = app.world();
        assert!(!w.entity(seat).contains::<CollisionEventsEnabled>(), "only in Play");

        app.world_mut().insert_resource(PlayDataModel { dm: eustress_common::datamodel::new_shared() });
        app.update();
        let w = app.world();
        for e in [seat, driver] {
            assert!(w.entity(e).contains::<CollisionEventsEnabled>(), "a seat reports touches in Play");
            assert!(w.entity(e).contains::<super::super::apply::TouchReportingForPlay>(), "and Stop turns it off");
        }
        assert!(!w.entity(part).contains::<CollisionEventsEnabled>(), "a plain part does not");
        assert!(!w.entity(asked).contains::<super::super::apply::TouchReportingForPlay>(), "an asking seat is left as it was");

        // A seat streamed in during the session asks too.
        let late = app.world_mut().spawn(instance("Late", ClassName::VehicleSeat)).id();
        app.update();
        assert!(app.world().entity(late).contains::<CollisionEventsEnabled>());
    }
}

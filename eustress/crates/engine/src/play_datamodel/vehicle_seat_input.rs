//! # A VehicleSeat's input
//!
//! A `VehicleSeat` reads the keys of whoever sits in it, as Roblox's does:
//! `W` or `Up` sets `Throttle` to 1, `S` or `Down` to -1, `D` or `Right` sets
//! `Steer` to 1, `A` or `Left` to -1, and a held pair cancels. `ThrottleFloat`
//! and `SteerFloat` carry the same values. There is no smoothing: the scripts
//! that drive the vehicle shape the response.
//!
//! The occupant's keys are the host's own input for the host's character, and
//! for a joined player that player's input frames (`DataModel::player_input`).
//! The seat is found from the player's side: each `Player`'s `Character`, its
//! `Humanoid`, the Humanoid's `SeatPart`, checked against the seat's
//! `Occupant` (the seat system writes both, docs/networking/SEATS.md).
//!
//! Written only when the keys change the value, and once more as zero when
//! the occupant leaves, so a script's own write to `Throttle` or `Steer` (an
//! AI driver, a cutscene) holds until the driver's keys next change it. Each
//! write is an ordinary one, so it replicates, and a driver's HUD on their own
//! machine reads the same `Throttle` the host's scripts do.
//!
//! Host only: the host seats characters and runs the scripts that read these.

use std::collections::HashMap;

use bevy::prelude::*;

use eustress_common::datamodel::{DataModel, DmValue, InputState, InstanceId};

use super::PlayDataModel;

/// The value this system last wrote to each seat it drives.
#[derive(Default)]
pub struct DrivenSeats(HashMap<InstanceId, (i8, i8)>);

/// Throttle and steer from the occupants' keys, before the frame's scripts.
pub fn drive_vehicle_seats(dm: Option<Res<PlayDataModel>>, mut driven: Local<DrivenSeats>) {
    let Some(dm) = dm else {
        driven.0.clear();
        return;
    };
    let mut g = dm.dm.lock();
    if !g.is_server {
        return;
    }
    step(&mut g, &mut driven.0);
}

/// One frame: write each occupied seat's value where it changed, and zero
/// the seats whose driver left.
fn step(g: &mut DataModel, driven: &mut HashMap<InstanceId, (i8, i8)>) {
    let occupied = occupied_seats(g);
    let mut writes: Vec<(InstanceId, (i8, i8))> = Vec::new();
    for &(seat, value) in &occupied {
        if driven.get(&seat) != Some(&value) {
            writes.push((seat, value));
        }
    }
    driven.retain(|seat, last| {
        let still = occupied.iter().any(|(s, _)| s == seat);
        if !still && *last != (0, 0) {
            writes.push((*seat, (0, 0)));
        }
        still
    });
    for (seat, value) in writes {
        write_seat(g, seat, value);
        if occupied.iter().any(|(s, _)| *s == seat) {
            driven.insert(seat, value);
        }
    }
}

/// Every `VehicleSeat` a player sits in, with the value its keys make.
fn occupied_seats(g: &DataModel) -> Vec<(InstanceId, (i8, i8))> {
    let mut out = Vec::new();
    let Some(players) = g.find_service("Players") else { return out };
    for &player in g.children(players) {
        if g.class_of(player) != Some("Player") {
            continue;
        }
        let Some(DmValue::Instance(character)) = g.get_prop(player, "Character") else { continue };
        let Some(humanoid) = g.children(character).iter().copied().find(|c| g.class_of(*c) == Some("Humanoid")) else {
            continue;
        };
        let Some(DmValue::Instance(seat)) = g.get_prop(humanoid, "SeatPart") else { continue };
        if g.class_of(seat) != Some("VehicleSeat") {
            continue;
        }
        if g.get_prop(seat, "Occupant").and_then(|v| v.as_instance()) != Some(humanoid) {
            continue;
        }
        let input = if g.local_player == Some(player) {
            Some(&g.input)
        } else {
            g.player_input.get(&player)
        };
        out.push((seat, input.map_or((0, 0), axes)));
    }
    out
}

/// Throttle and steer from held keys.
fn axes(input: &InputState) -> (i8, i8) {
    let held = |a: &str, b: &str| input.keys.contains(a) || input.keys.contains(b);
    let axis = |plus: bool, minus: bool| plus as i8 - minus as i8;
    (axis(held("W", "Up"), held("S", "Down")), axis(held("D", "Right"), held("A", "Left")))
}

fn write_seat(g: &mut DataModel, seat: InstanceId, (throttle, steer): (i8, i8)) {
    for (name, value) in [
        ("Throttle", throttle),
        ("ThrottleFloat", throttle),
        ("Steer", steer),
        ("SteerFloat", steer),
    ] {
        let _ = g.set_prop(seat, name, DmValue::Number(value as f64));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree with the host's player sitting in a VehicleSeat and a joined
    /// player sitting in another.
    fn seated() -> (DataModel, InstanceId, InstanceId, InstanceId, InstanceId) {
        let mut g = DataModel::new();
        let players = g.get_service("Players").expect("Players");
        let workspace = g.get_service("Workspace").expect("Workspace");
        let sit = |g: &mut DataModel, name: &str| {
            let player = g.create("Player");
            g.rename(player, name).unwrap();
            g.set_parent(player, Some(players)).unwrap();
            let character = g.create("Model");
            g.set_parent(character, Some(workspace)).unwrap();
            let humanoid = g.create("Humanoid");
            g.set_parent(humanoid, Some(character)).unwrap();
            let seat = g.create("VehicleSeat");
            g.set_parent(seat, Some(workspace)).unwrap();
            g.set_prop(player, "Character", DmValue::Instance(character)).unwrap();
            g.set_prop(humanoid, "SeatPart", DmValue::Instance(seat)).unwrap();
            g.set_prop(seat, "Occupant", DmValue::Instance(humanoid)).unwrap();
            (player, humanoid, seat)
        };
        let (host, _, host_seat) = sit(&mut g, "Host");
        let (guest, guest_humanoid, guest_seat) = sit(&mut g, "Guest");
        g.local_player = Some(host);
        g.player_input.insert(guest, InputState::default());
        let _ = g.take_dirty();
        (g, host_seat, guest, guest_seat, guest_humanoid)
    }

    fn value(g: &DataModel, seat: InstanceId, prop: &str) -> f64 {
        g.get_prop(seat, prop).and_then(|v| v.as_number()).unwrap_or(f64::NAN)
    }

    #[test]
    fn each_seat_reads_its_own_occupant() {
        let (mut g, host_seat, guest, guest_seat, _) = seated();
        let mut driven = HashMap::new();
        g.input.keys.insert("W".into());
        g.input.keys.insert("A".into());
        g.player_input.get_mut(&guest).unwrap().keys.insert("Down".into());
        step(&mut g, &mut driven);
        assert_eq!(value(&g, host_seat, "Throttle"), 1.0);
        assert_eq!(value(&g, host_seat, "Steer"), -1.0);
        assert_eq!(value(&g, host_seat, "SteerFloat"), -1.0);
        assert_eq!(value(&g, guest_seat, "Throttle"), -1.0);
        assert_eq!(value(&g, guest_seat, "Steer"), 0.0);
    }

    #[test]
    fn writes_only_on_change_so_a_script_write_holds() {
        let (mut g, host_seat, _, _, _) = seated();
        let mut driven = HashMap::new();
        g.input.keys.insert("W".into());
        step(&mut g, &mut driven);
        assert!(g.take_dirty().iter().any(|(id, _)| *id == host_seat));
        // Held keys, no change: nothing written.
        step(&mut g, &mut driven);
        assert!(!g.take_dirty().iter().any(|(id, _)| *id == host_seat));
        // A script's write stands until the keys change.
        g.set_prop(host_seat, "Throttle", DmValue::Number(0.5)).unwrap();
        step(&mut g, &mut driven);
        assert_eq!(value(&g, host_seat, "Throttle"), 0.5);
        g.input.keys.insert("S".into());
        step(&mut g, &mut driven);
        assert_eq!(value(&g, host_seat, "Throttle"), 0.0, "W and S cancel");
    }

    #[test]
    fn leaving_zeroes_the_seat_once() {
        let (mut g, _, guest, guest_seat, guest_humanoid) = seated();
        let mut driven = HashMap::new();
        g.player_input.get_mut(&guest).unwrap().keys.insert("D".into());
        step(&mut g, &mut driven);
        assert_eq!(value(&g, guest_seat, "Steer"), 1.0);
        g.set_prop(guest_seat, "Occupant", DmValue::Nil).unwrap();
        g.set_prop(guest_humanoid, "SeatPart", DmValue::Nil).unwrap();
        step(&mut g, &mut driven);
        assert_eq!(value(&g, guest_seat, "Steer"), 0.0);
        let _ = g.take_dirty();
        // An AI script now drives the empty seat; nothing overrides it.
        g.set_prop(guest_seat, "Throttle", DmValue::Number(1.0)).unwrap();
        step(&mut g, &mut driven);
        assert_eq!(value(&g, guest_seat, "Throttle"), 1.0);
    }

    #[test]
    fn a_plain_seat_carries_no_input() {
        let (mut g, host_seat, _, _, _) = seated();
        let workspace = g.get_service("Workspace").unwrap();
        let seat = g.create("Seat");
        g.set_parent(seat, Some(workspace)).unwrap();
        let host = g.local_player.unwrap();
        let Some(DmValue::Instance(character)) = g.get_prop(host, "Character") else { panic!() };
        let humanoid = g.children(character)[0];
        g.set_prop(humanoid, "SeatPart", DmValue::Instance(seat)).unwrap();
        g.set_prop(seat, "Occupant", DmValue::Instance(humanoid)).unwrap();
        g.set_prop(host_seat, "Occupant", DmValue::Nil).unwrap();
        g.input.keys.insert("W".into());
        let mut driven = HashMap::new();
        step(&mut g, &mut driven);
        assert!(g.get_prop(seat, "Throttle").is_none());
    }
}

//! The input lane: what each player is doing with its devices, for any
//! simulation.
//!
//! A player sends raw device state, never meaning: the keys held, the mouse
//! and gamepad buttons, the sticks and triggers, where its camera is and
//! where it looks and aims. Whatever reads the host's own input (the
//! character controller, a `VehicleSeat`, a server script asking a `Player`)
//! reads each player's the same way, so a simulation needs no input code to
//! become multiplayer.
//!
//! Each datagram carries the newest [`INPUT_REDUNDANCY`] samples, so a lost
//! one costs nothing: the next carries it again.

use std::collections::{BTreeMap, HashMap};

use bevy::input::keyboard::KeyCode;
use bevy::math::Vec3;
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::wire::PeerId;

/// Samples per datagram.
pub const INPUT_REDUNDANCY: usize = 4;
/// Samples the host keeps per player.
const KEPT: usize = 128;
/// Farthest from the origin a camera may be, as for avatars.
const WORLD_LIMIT: f32 = 1.0e5;

/// Keys the lane carries, by bit, with their Roblox `KeyCode` names.
pub const KEYS: &[(KeyCode, &str)] = &[
    (KeyCode::KeyA, "A"), (KeyCode::KeyB, "B"), (KeyCode::KeyC, "C"), (KeyCode::KeyD, "D"),
    (KeyCode::KeyE, "E"), (KeyCode::KeyF, "F"), (KeyCode::KeyG, "G"), (KeyCode::KeyH, "H"),
    (KeyCode::KeyI, "I"), (KeyCode::KeyJ, "J"), (KeyCode::KeyK, "K"), (KeyCode::KeyL, "L"),
    (KeyCode::KeyM, "M"), (KeyCode::KeyN, "N"), (KeyCode::KeyO, "O"), (KeyCode::KeyP, "P"),
    (KeyCode::KeyQ, "Q"), (KeyCode::KeyR, "R"), (KeyCode::KeyS, "S"), (KeyCode::KeyT, "T"),
    (KeyCode::KeyU, "U"), (KeyCode::KeyV, "V"), (KeyCode::KeyW, "W"), (KeyCode::KeyX, "X"),
    (KeyCode::KeyY, "Y"), (KeyCode::KeyZ, "Z"),
    (KeyCode::Digit0, "Zero"), (KeyCode::Digit1, "One"), (KeyCode::Digit2, "Two"), (KeyCode::Digit3, "Three"),
    (KeyCode::Digit4, "Four"), (KeyCode::Digit5, "Five"), (KeyCode::Digit6, "Six"), (KeyCode::Digit7, "Seven"),
    (KeyCode::Digit8, "Eight"), (KeyCode::Digit9, "Nine"),
    (KeyCode::Space, "Space"), (KeyCode::Enter, "Return"), (KeyCode::Escape, "Escape"), (KeyCode::Tab, "Tab"),
    (KeyCode::Backspace, "Backspace"), (KeyCode::Delete, "Delete"), (KeyCode::Insert, "Insert"),
    (KeyCode::Home, "Home"), (KeyCode::End, "End"), (KeyCode::PageUp, "PageUp"), (KeyCode::PageDown, "PageDown"),
    (KeyCode::ArrowUp, "Up"), (KeyCode::ArrowDown, "Down"), (KeyCode::ArrowLeft, "Left"), (KeyCode::ArrowRight, "Right"),
    (KeyCode::ShiftLeft, "LeftShift"), (KeyCode::ShiftRight, "RightShift"),
    (KeyCode::ControlLeft, "LeftControl"), (KeyCode::ControlRight, "RightControl"),
    (KeyCode::AltLeft, "LeftAlt"), (KeyCode::AltRight, "RightAlt"),
    (KeyCode::SuperLeft, "LeftSuper"), (KeyCode::SuperRight, "RightSuper"), (KeyCode::CapsLock, "CapsLock"),
    (KeyCode::F1, "F1"), (KeyCode::F2, "F2"), (KeyCode::F3, "F3"), (KeyCode::F4, "F4"),
    (KeyCode::F5, "F5"), (KeyCode::F6, "F6"), (KeyCode::F7, "F7"), (KeyCode::F8, "F8"),
    (KeyCode::F9, "F9"), (KeyCode::F10, "F10"), (KeyCode::F11, "F11"), (KeyCode::F12, "F12"),
    (KeyCode::Minus, "Minus"), (KeyCode::Equal, "Equals"), (KeyCode::BracketLeft, "LeftBracket"),
    (KeyCode::BracketRight, "RightBracket"), (KeyCode::Backslash, "BackSlash"), (KeyCode::Semicolon, "Semicolon"),
    (KeyCode::Quote, "Quote"), (KeyCode::Backquote, "Backquote"), (KeyCode::Comma, "Comma"),
    (KeyCode::Period, "Period"), (KeyCode::Slash, "Slash"),
    (KeyCode::Numpad0, "KeypadZero"), (KeyCode::Numpad1, "KeypadOne"), (KeyCode::Numpad2, "KeypadTwo"),
    (KeyCode::Numpad3, "KeypadThree"), (KeyCode::Numpad4, "KeypadFour"), (KeyCode::Numpad5, "KeypadFive"),
    (KeyCode::Numpad6, "KeypadSix"), (KeyCode::Numpad7, "KeypadSeven"), (KeyCode::Numpad8, "KeypadEight"),
    (KeyCode::Numpad9, "KeypadNine"), (KeyCode::NumpadEnter, "KeypadEnter"), (KeyCode::NumpadAdd, "KeypadPlus"),
    (KeyCode::NumpadSubtract, "KeypadMinus"), (KeyCode::NumpadMultiply, "KeypadMultiply"),
    (KeyCode::NumpadDivide, "KeypadDivide"), (KeyCode::NumpadDecimal, "KeypadPeriod"),
];

/// The lane's bit for `key`, if it carries it.
pub fn key_bit(key: KeyCode) -> Option<usize> {
    KEYS.iter().position(|(k, _)| *k == key)
}

/// The bit for a Roblox `KeyCode` name (`"W"`, `"LeftShift"`).
pub fn key_bit_by_name(name: &str) -> Option<usize> {
    let name = name.trim_start_matches("Enum.KeyCode.");
    KEYS.iter().position(|(_, n)| *n == name)
}

/// Mouse buttons, as bits of [`InputSample::mouse`].
pub const MOUSE_LEFT: u8 = 1;
pub const MOUSE_RIGHT: u8 = 2;
pub const MOUSE_MIDDLE: u8 = 4;

/// Gamepad axes, as indices of [`InputSample::axes`].
pub const AXIS_LEFT_X: usize = 0;
pub const AXIS_LEFT_Y: usize = 1;
pub const AXIS_RIGHT_X: usize = 2;
pub const AXIS_RIGHT_Y: usize = 3;
pub const AXIS_LEFT_TRIGGER: usize = 4;
pub const AXIS_RIGHT_TRIGGER: usize = 5;

/// A player's devices at one tick.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct InputSample {
    /// The host tick the player meant this for (its estimate).
    pub tick: u64,
    /// Held keys, one bit each, by [`KEYS`] position.
    pub keys: [u64; 2],
    pub mouse: u8,
    /// Gamepad buttons, one bit each, in Bevy's `GamepadButton` order.
    pub pad: u32,
    /// Sticks and triggers, ±1 as ±32767.
    pub axes: [i16; 6],
    pub camera: [f32; 3],
    /// Where the camera looks, octahedral (see [`encode_dir`]).
    pub look: [i16; 2],
    /// Where the pointer aims into the world, octahedral.
    pub aim: [i16; 2],
}

impl InputSample {
    pub fn key(&self, bit: usize) -> bool {
        bit < 128 && self.keys[bit / 64] & (1 << (bit % 64)) != 0
    }

    pub fn set_key(&mut self, bit: usize) {
        if bit < 128 {
            self.keys[bit / 64] |= 1 << (bit % 64);
        }
    }

    /// A Roblox `KeyCode` name is held.
    pub fn key_named(&self, name: &str) -> bool {
        key_bit_by_name(name).is_some_and(|b| self.key(b))
    }

    pub fn axis(&self, i: usize) -> f32 {
        self.axes.get(i).map_or(0.0, |v| *v as f32 / 32767.0)
    }

    pub fn camera(&self) -> Vec3 {
        Vec3::from_array(self.camera)
    }

    pub fn look(&self) -> Vec3 {
        decode_dir(self.look)
    }

    pub fn aim(&self) -> Vec3 {
        decode_dir(self.aim)
    }

    /// Fit to use: a finite camera inside the world.
    pub fn is_valid(&self) -> bool {
        self.camera.iter().all(|v| v.is_finite() && v.abs() < WORLD_LIMIT)
    }
}

/// The newest samples, newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct InputFrame {
    pub samples: Vec<InputSample>,
}

/// A unit direction as two 16-bit numbers (octahedral mapping): about
/// 0.01° everywhere on the sphere.
pub fn encode_dir(d: Vec3) -> [i16; 2] {
    let l1 = d.x.abs() + d.y.abs() + d.z.abs();
    if !(l1 > 0.0) || !l1.is_finite() {
        return [0, 0];
    }
    let (mut x, mut y) = (d.x / l1, d.y / l1);
    if d.z < 0.0 {
        let (ox, oy) = (x, y);
        x = (1.0 - oy.abs()) * ox.signum();
        y = (1.0 - ox.abs()) * oy.signum();
    }
    [(x * 32767.0).round() as i16, (y * 32767.0).round() as i16]
}

pub fn decode_dir(e: [i16; 2]) -> Vec3 {
    let x = e[0] as f32 / 32767.0;
    let y = e[1] as f32 / 32767.0;
    let z = 1.0 - x.abs() - y.abs();
    let (x, y) = if z < 0.0 { ((1.0 - y.abs()) * x.signum(), (1.0 - x.abs()) * y.signum()) } else { (x, y) };
    let v = Vec3::new(x, y, z);
    if v.length_squared() > 0.0 {
        v.normalize()
    } else {
        Vec3::NEG_Z
    }
}

/// One player's recent input, by tick.
#[derive(Debug, Default)]
pub struct PeerInput {
    samples: BTreeMap<u64, InputSample>,
}

impl PeerInput {
    /// Keep a frame's samples that are new and valid. Returns how many.
    /// Ticks only move forward past the newest kept, except to fill a gap
    /// a lost datagram left.
    pub fn accept(&mut self, frame: &InputFrame) -> usize {
        let newest = self.samples.keys().next_back().copied();
        let mut kept = 0;
        for s in frame.samples.iter().take(INPUT_REDUNDANCY) {
            if !s.is_valid() || self.samples.contains_key(&s.tick) {
                continue;
            }
            if let Some(n) = newest {
                if s.tick + KEPT as u64 <= n {
                    continue; // too old to matter
                }
            }
            self.samples.insert(s.tick, *s);
            kept += 1;
        }
        while self.samples.len() > KEPT {
            self.samples.pop_first();
        }
        kept
    }

    /// The newest sample.
    pub fn latest(&self) -> Option<&InputSample> {
        self.samples.values().next_back()
    }

    /// The newest sample at or before `tick`.
    pub fn at(&self, tick: u64) -> Option<&InputSample> {
        self.samples.range(..=tick).next_back().map(|(_, s)| s)
    }
}

/// Every joined player's input, on the host.
#[derive(Resource, Debug, Default)]
pub struct PeerInputs {
    pub by_peer: HashMap<PeerId, PeerInput>,
}

impl PeerInputs {
    pub fn latest(&self, peer: PeerId) -> Option<&InputSample> {
        self.by_peer.get(&peer).and_then(PeerInput::latest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_table_fits_and_names_are_unique() {
        assert!(KEYS.len() <= 128);
        for (i, (k, n)) in KEYS.iter().enumerate() {
            assert_eq!(key_bit(*k), Some(i));
            assert_eq!(key_bit_by_name(n), Some(i), "{n} appears twice");
        }
        let mut s = InputSample::default();
        s.set_key(key_bit(KeyCode::KeyW).unwrap());
        s.set_key(key_bit(KeyCode::NumpadDecimal).unwrap());
        assert!(s.key_named("W") && s.key_named("Enum.KeyCode.KeypadPeriod"));
        assert!(!s.key_named("S"));
    }

    #[test]
    fn directions_survive_the_trip() {
        for d in [Vec3::X, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z, Vec3::new(0.3, -0.8, -0.52), Vec3::new(-1.0, 1.0, -1.0)] {
            let d = d.normalize();
            let back = decode_dir(encode_dir(d));
            assert!(back.angle_between(d) < 0.0005, "{d} came back as {back}");
        }
        assert_eq!(decode_dir(encode_dir(Vec3::ZERO)).length(), 1.0);
    }

    #[test]
    fn redundant_samples_fill_gaps_once() {
        let mut p = PeerInput::default();
        let s = |tick| InputSample { tick, ..Default::default() };
        assert_eq!(p.accept(&InputFrame { samples: vec![s(10), s(9), s(8), s(7)] }), 4);
        // The next datagram repeats three of them.
        assert_eq!(p.accept(&InputFrame { samples: vec![s(11), s(10), s(9), s(8)] }), 1);
        assert_eq!(p.latest().unwrap().tick, 11);
        assert_eq!(p.at(8).unwrap().tick, 8);
        let mut bad = s(12);
        bad.camera[0] = f32::NAN;
        assert_eq!(p.accept(&InputFrame { samples: vec![bad] }), 0);
        // Far too old to matter.
        assert_eq!(p.accept(&InputFrame { samples: vec![s(500)] }), 1);
        assert_eq!(p.accept(&InputFrame { samples: vec![s(12)] }), 0);
    }
}

//! [`WireValue`]: a property, attribute or remote argument on the wire.
//!
//! Every [`DmValue`] has a form here, plus arrays and string-keyed maps for
//! remote arguments (Luau tables). Instance references travel as [`NetId`]s;
//! a reference the receiver cannot see arrives as nil, as it does in Roblox.
//!
//! Numbers keep full `f64` precision and a `CFrame` keeps its exact rotation
//! matrix: property writes are rare next to motion, and a value read back on a
//! player equals the one the host's script wrote.

use eustress_common::datamodel::{DmValue, EnumItem, InstanceId};
use eustress_common::scripting::{CFrame, Color3, NumberRange, UDim, UDim2, Vector2, Vector3};
use serde::{Deserialize, Serialize};

use super::id::NetId;

/// A value as it crosses the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WireValue {
    Nil,
    Bool(bool),
    Number(f64),
    String(String),
    Vector2([f64; 2]),
    Vector3([f64; 3]),
    /// Position, then the row-major rotation matrix.
    CFrame([f64; 3], [f64; 9]),
    Color3([f64; 3]),
    /// Scale, offset.
    UDim([f64; 2]),
    /// X scale, X offset, Y scale, Y offset.
    UDim2([f64; 4]),
    NumberRange([f64; 2]),
    /// Enum type, item name.
    Enum(String, String),
    /// [`NetId::NONE`] for a reference the receiver cannot resolve.
    Instance(NetId),
    /// Keypoints as (time, value).
    NumberSequence(Vec<[f64; 2]>),
    /// Keypoints as (time, colour).
    ColorSequence(Vec<(f64, [f64; 3])>),
    /// A Luau array. Remote arguments only.
    Array(Vec<WireValue>),
    /// A Luau table with string keys. Remote arguments only.
    Map(Vec<(String, WireValue)>),
}

/// Bounds a value from a player must fit before a host script sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueLimits {
    /// Nesting of arrays and maps.
    pub max_depth: usize,
    /// Values in total, counting every nested one.
    pub max_values: usize,
    /// Bytes of one string (keys included).
    pub max_string_bytes: usize,
    /// Keypoints in one sequence.
    pub max_keypoints: usize,
}

impl Default for ValueLimits {
    fn default() -> Self {
        Self { max_depth: 16, max_values: 4096, max_string_bytes: 64 * 1024, max_keypoints: 20 }
    }
}

impl WireValue {
    /// The wire form of `v`. `net_of` names an instance for this receiver, or
    /// `None` when the receiver cannot see it.
    pub fn from_dm(v: &DmValue, net_of: &dyn Fn(InstanceId) -> Option<NetId>) -> WireValue {
        match v {
            DmValue::Nil => WireValue::Nil,
            DmValue::Bool(b) => WireValue::Bool(*b),
            DmValue::Number(n) => WireValue::Number(*n),
            DmValue::String(s) => WireValue::String(s.clone()),
            DmValue::Vector2(v) => WireValue::Vector2([v.x, v.y]),
            DmValue::Vector3(v) => WireValue::Vector3([v.x, v.y, v.z]),
            DmValue::CFrame(cf) => {
                let r = cf.rotation_matrix();
                WireValue::CFrame(
                    [cf.position.x, cf.position.y, cf.position.z],
                    [r[0][0], r[0][1], r[0][2], r[1][0], r[1][1], r[1][2], r[2][0], r[2][1], r[2][2]],
                )
            }
            DmValue::Color3(c) => WireValue::Color3([c.r, c.g, c.b]),
            DmValue::UDim(u) => WireValue::UDim([u.scale, u.offset]),
            DmValue::UDim2(u) => WireValue::UDim2([u.x.scale, u.x.offset, u.y.scale, u.y.offset]),
            DmValue::NumberRange(r) => WireValue::NumberRange([r.min, r.max]),
            DmValue::Enum(e) => WireValue::Enum(e.enum_type.clone(), e.name.clone()),
            DmValue::Instance(id) => WireValue::Instance(net_of(*id).unwrap_or(NetId::NONE)),
            DmValue::NumberSequence(k) => WireValue::NumberSequence(k.iter().map(|(t, v)| [*t, *v]).collect()),
            DmValue::ColorSequence(k) => {
                WireValue::ColorSequence(k.iter().map(|(t, c)| (*t, [c.r, c.g, c.b])).collect())
            }
        }
    }

    /// The DataModel form, when there is one. Arrays and maps have none.
    /// `local_of` resolves an instance reference; an unresolved one is
    /// reported through `Err` so the caller can retry it once the frame's
    /// spawns are in (a model can name a part spawned after it).
    pub fn to_dm(&self, local_of: &dyn Fn(NetId) -> Option<InstanceId>) -> Result<DmValue, Unresolved> {
        Ok(match self {
            WireValue::Nil => DmValue::Nil,
            WireValue::Bool(b) => DmValue::Bool(*b),
            WireValue::Number(n) => DmValue::Number(*n),
            WireValue::String(s) => DmValue::String(s.clone()),
            WireValue::Vector2([x, y]) => DmValue::Vector2(Vector2 { x: *x, y: *y }),
            WireValue::Vector3([x, y, z]) => DmValue::Vector3(Vector3 { x: *x, y: *y, z: *z }),
            WireValue::CFrame(p, r) => DmValue::CFrame(CFrame::from_rotation_matrix(
                Vector3 { x: p[0], y: p[1], z: p[2] },
                [[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]],
            )),
            WireValue::Color3([r, g, b]) => DmValue::Color3(Color3 { r: *r, g: *g, b: *b }),
            WireValue::UDim([s, o]) => DmValue::UDim(UDim { scale: *s, offset: *o }),
            WireValue::UDim2([xs, xo, ys, yo]) => DmValue::UDim2(UDim2 {
                x: UDim { scale: *xs, offset: *xo },
                y: UDim { scale: *ys, offset: *yo },
            }),
            WireValue::NumberRange([a, b]) => DmValue::NumberRange(NumberRange { min: *a, max: *b }),
            WireValue::Enum(ty, name) => DmValue::Enum(EnumItem::new(ty.clone(), name.clone())),
            WireValue::Instance(net) if net.is_none() => DmValue::Nil,
            WireValue::Instance(net) => match local_of(*net) {
                Some(id) => DmValue::Instance(id),
                None => return Err(Unresolved::Instance(*net)),
            },
            WireValue::NumberSequence(k) => DmValue::NumberSequence(k.iter().map(|[t, v]| (*t, *v)).collect()),
            WireValue::ColorSequence(k) => DmValue::ColorSequence(
                k.iter().map(|(t, [r, g, b])| (*t, Color3 { r: *r, g: *g, b: *b })).collect(),
            ),
            WireValue::Array(_) | WireValue::Map(_) => return Err(Unresolved::Table),
        })
    }

    /// Roblox's `typeof` name for the value.
    pub fn type_name(&self) -> &'static str {
        match self {
            WireValue::Nil => "nil",
            WireValue::Bool(_) => "boolean",
            WireValue::Number(_) => "number",
            WireValue::String(_) => "string",
            WireValue::Vector2(_) => "Vector2",
            WireValue::Vector3(_) => "Vector3",
            WireValue::CFrame(..) => "CFrame",
            WireValue::Color3(_) => "Color3",
            WireValue::UDim(_) => "UDim",
            WireValue::UDim2(_) => "UDim2",
            WireValue::NumberRange(_) => "NumberRange",
            WireValue::Enum(..) => "EnumItem",
            WireValue::Instance(_) => "Instance",
            WireValue::NumberSequence(_) => "NumberSequence",
            WireValue::ColorSequence(_) => "ColorSequence",
            WireValue::Array(_) | WireValue::Map(_) => "table",
        }
    }

    /// Every instance this value names, nested ones included.
    pub fn instances(&self, out: &mut Vec<NetId>) {
        match self {
            WireValue::Instance(n) if !n.is_none() => out.push(*n),
            WireValue::Array(items) => items.iter().for_each(|v| v.instances(out)),
            WireValue::Map(entries) => entries.iter().for_each(|(_, v)| v.instances(out)),
            _ => {}
        }
    }

    /// Check a value that came from a player: bounded in size and nesting,
    /// and every vector, colour and frame finite with a rotation that is a
    /// rotation. A number may be anything Luau allows (`math.huge`, NaN): a
    /// typed remote can refuse those (see [`super::remote::check_signature`]).
    pub fn check(&self, limits: &ValueLimits) -> Result<(), String> {
        let mut count = 0usize;
        self.check_at(limits, 0, &mut count)
    }

    fn check_at(&self, limits: &ValueLimits, depth: usize, count: &mut usize) -> Result<(), String> {
        *count += 1;
        if *count > limits.max_values {
            return Err(format!("more than {} values", limits.max_values));
        }
        let finite = |xs: &[f64]| xs.iter().all(|x| x.is_finite());
        let string_ok = |s: &str| s.len() <= limits.max_string_bytes;
        match self {
            WireValue::String(s) if !string_ok(s) => Err(format!("a string longer than {} bytes", limits.max_string_bytes)),
            WireValue::Enum(t, n) if !string_ok(t) || !string_ok(n) => Err("an enum name too long".into()),
            WireValue::Vector2(v) if !finite(v) => Err("a Vector2 that is not finite".into()),
            WireValue::Vector3(v) if !finite(v) => Err("a Vector3 that is not finite".into()),
            WireValue::Color3(v) if !finite(v) => Err("a Color3 that is not finite".into()),
            WireValue::UDim(v) if !finite(v) => Err("a UDim that is not finite".into()),
            WireValue::UDim2(v) if !finite(v) => Err("a UDim2 that is not finite".into()),
            WireValue::NumberRange(v) if !finite(v) => Err("a NumberRange that is not finite".into()),
            WireValue::CFrame(p, r) => {
                if !finite(p) || !finite(r) {
                    return Err("a CFrame that is not finite".into());
                }
                if !is_rotation(r) {
                    return Err("a CFrame whose rotation is not a rotation".into());
                }
                Ok(())
            }
            WireValue::NumberSequence(k) if k.len() > limits.max_keypoints => Err("too many keypoints".into()),
            WireValue::ColorSequence(k) if k.len() > limits.max_keypoints => Err("too many keypoints".into()),
            WireValue::Array(items) => {
                if depth >= limits.max_depth {
                    return Err(format!("tables nested deeper than {}", limits.max_depth));
                }
                items.iter().try_for_each(|v| v.check_at(limits, depth + 1, count))
            }
            WireValue::Map(entries) => {
                if depth >= limits.max_depth {
                    return Err(format!("tables nested deeper than {}", limits.max_depth));
                }
                for (k, v) in entries {
                    if !string_ok(k) {
                        return Err("a table key too long".into());
                    }
                    v.check_at(limits, depth + 1, count)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Orthonormal with determinant +1, to a tolerance a quantized or
/// re-normalized matrix still meets.
fn is_rotation(r: &[f64; 9]) -> bool {
    let row = |i: usize| [r[i * 3], r[i * 3 + 1], r[i * 3 + 2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (x, y, z) = (row(0), row(1), row(2));
    let unit = |a: [f64; 3]| (dot(a, a) - 1.0).abs() < 1e-3;
    let det = x[0] * (y[1] * z[2] - y[2] * z[1]) - x[1] * (y[0] * z[2] - y[2] * z[0]) + x[2] * (y[0] * z[1] - y[1] * z[0]);
    unit(x) && unit(y) && unit(z) && dot(x, y).abs() < 1e-3 && dot(y, z).abs() < 1e-3 && dot(x, z).abs() < 1e-3
        && (det - 1.0).abs() < 1e-3
}

/// Why a wire value has no DataModel form yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unresolved {
    /// It names an instance this side does not have (yet).
    Instance(NetId),
    /// It is a table; tables are remote arguments, not property values.
    Table,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_kind() -> Vec<DmValue> {
        vec![
            DmValue::Nil,
            DmValue::Bool(true),
            DmValue::Number(0.1),
            DmValue::Number(f64::MAX),
            DmValue::String("héllo".into()),
            DmValue::Vector2(Vector2 { x: 1.5, y: -2.0 }),
            DmValue::Vector3(Vector3 { x: 1.0e7, y: 0.001, z: -3.25 }),
            DmValue::CFrame(CFrame::from_axis_angle(Vector3 { x: 0.0, y: 1.0, z: 0.0 }, 0.7)),
            DmValue::Color3(Color3 { r: 0.1, g: 0.2, b: 0.3 }),
            DmValue::UDim(UDim { scale: 0.5, offset: 12.0 }),
            DmValue::UDim2(UDim2 { x: UDim { scale: 0.25, offset: 4.0 }, y: UDim { scale: 1.0, offset: -8.0 } }),
            DmValue::NumberRange(NumberRange { min: 1.0, max: 3.0 }),
            DmValue::Enum(EnumItem::new("Material", "Neon")),
            DmValue::NumberSequence(vec![(0.0, 1.0), (1.0, 0.0)]),
            DmValue::ColorSequence(vec![(0.0, Color3 { r: 1.0, g: 0.0, b: 0.0 }), (1.0, Color3 { r: 0.0, g: 0.0, b: 1.0 })]),
        ]
    }

    #[test]
    fn every_property_type_round_trips_exactly() {
        let none = |_: InstanceId| None;
        let unresolvable = |_: NetId| None;
        for v in every_kind() {
            let wire = WireValue::from_dm(&v, &none);
            let back = wire.to_dm(&unresolvable).expect("no references in these");
            assert_eq!(back, v, "{} changed on the way through", v.type_name());
            assert_eq!(wire.type_name(), v.type_name());
        }
    }

    #[test]
    fn instance_references_map_through_net_ids() {
        let host_part = InstanceId(42);
        let player_part = InstanceId(7);
        let wire = WireValue::from_dm(&DmValue::Instance(host_part), &|id| (id == host_part).then_some(NetId(900)));
        assert_eq!(wire, WireValue::Instance(NetId(900)));
        assert_eq!(wire.to_dm(&|n| (n == NetId(900)).then_some(player_part)), Ok(DmValue::Instance(player_part)));
        assert_eq!(wire.to_dm(&|_| None), Err(Unresolved::Instance(NetId(900))));
        // A reference the receiver may not see goes as nil.
        let hidden = WireValue::from_dm(&DmValue::Instance(host_part), &|_| None);
        assert_eq!(hidden.to_dm(&|_| None), Ok(DmValue::Nil));
    }

    #[test]
    fn player_values_are_bounded() {
        let limits = ValueLimits::default();
        let mut deep = WireValue::Number(1.0);
        for _ in 0..limits.max_depth {
            deep = WireValue::Array(vec![deep]);
        }
        assert!(deep.check(&limits).is_ok());
        assert!(WireValue::Array(vec![deep]).check(&limits).is_err());

        let wide = WireValue::Array(vec![WireValue::Bool(true); limits.max_values]);
        assert!(wide.check(&limits).is_err(), "the array itself counts");
        assert!(WireValue::String("x".repeat(limits.max_string_bytes + 1)).check(&limits).is_err());
        assert!(WireValue::Vector3([0.0, f64::NAN, 0.0]).check(&limits).is_err());
        assert!(WireValue::Number(f64::NAN).check(&limits).is_ok(), "Luau numbers may be NaN");
        let squash = WireValue::CFrame([0.0; 3], [2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        assert!(squash.check(&limits).is_err());
        let mirror = WireValue::CFrame([0.0; 3], [-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
        assert!(mirror.check(&limits).is_err(), "a reflection is not a rotation");
        let turned = WireValue::from_dm(&DmValue::CFrame(CFrame::from_axis_angle(Vector3 { x: 1.0, y: 1.0, z: 0.0 }, 2.0)), &|_| None);
        assert!(turned.check(&limits).is_ok());
    }
}

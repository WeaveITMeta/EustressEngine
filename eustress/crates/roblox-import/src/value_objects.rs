//! Roblox `ValueObject` → parent attribute folding (deprecation Phase 1).
//!
//! Roblox stores loose scalars/vectors/refs as dedicated *ValueObject*
//! instances (`IntValue`, `BoolValue`, `ObjectValue`, …) parented under
//! the object they describe. Eustress has no such instance class; the
//! idiomatic representation is a typed **attribute** on the parent
//! (`parent:GetAttribute("Name")`).
//!
//! This module owns the two importer-side primitives that fold a
//! ValueObject child into its parent:
//!
//! - [`is_value_object_class`] — classifies a raw Roblox class string as a
//!   ValueObject (the 11 convertible classes + the 2 *Constrained* classes
//!   that are dropped per the product decision).
//! - [`encode_value_object`] — reads a ValueObject's `Value` property and
//!   produces the `toml::Value` that lands in the parent's
//!   `[attributes]` table, following **Contract A** (see
//!   `materializer.rs` and the loader's `rich_toml_value_to_attribute`,
//!   which decode the same shapes back into
//!   `eustress_common::AttributeValue`).
//!
//! ## Contract A — `[attributes]` encoding
//!
//! | Roblox class                     | `toml::Value` shape                                  |
//! |----------------------------------|------------------------------------------------------|
//! | `BoolValue`                      | bare bool                                            |
//! | `IntValue`                       | bare integer                                         |
//! | `NumberValue`                    | bare float                                           |
//! | `StringValue`, `BinaryStringValue` | bare string                                        |
//! | `Vector3Value`                   | `[x, y, z]` (3 floats)                               |
//! | `Color3Value`                    | `{ Color3 = [r, g, b] }` (0..1 floats)              |
//! | `CFrameValue`                    | `{ CFrame = [px, py, pz, qx, qy, qz, qw] }`         |
//! | `BrickColorValue`                | `{ BrickColor = N }` (palette index integer)        |
//! | `ObjectValue`                    | bare string holding the resolved Eustress UUID hex  |
//! | `RayValue`, `IntConstrainedValue`, `DoubleConstrainedValue` | dropped (`None`) |

use rbx_dom_weak::types::{Ref, Variant};
use rbx_dom_weak::UstrMap;

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// True when `roblox_class` is a Roblox *ValueObject* — either one of the
/// 11 convertible classes or one of the 2 *Constrained* classes that the
/// importer drops (records an approximation rather than converting).
///
/// The materializer uses this to decide, for each child of a node, whether
/// to fold it into the parent's `[attributes]` (convertible) or skip it
/// with an approximation note (Constrained) — in BOTH cases the child is
/// NOT materialised as its own instance.
pub fn is_value_object_class(roblox_class: &str) -> bool {
    is_convertible_value_object(roblox_class) || is_dropped_value_object(roblox_class)
}

/// The 10 ValueObject classes the importer folds into a typed attribute.
/// (`RayValue` is a recognised ValueObject but is dropped, not converted —
/// see [`is_dropped_value_object`].)
pub fn is_convertible_value_object(roblox_class: &str) -> bool {
    matches!(
        roblox_class,
        "NumberValue"
            | "IntValue"
            | "BoolValue"
            | "StringValue"
            | "ObjectValue"
            | "Color3Value"
            | "Vector3Value"
            | "CFrameValue"
            | "BrickColorValue"
            | "BinaryStringValue"
    )
}

/// The 2 *Constrained* ValueObject classes plus `RayValue` — recognised as
/// ValueObjects (so they are folded out of the instance tree) but NOT
/// converted: `encode_value_object` returns `None` for them and the
/// materializer records an approximation noting the class was dropped.
pub fn is_dropped_value_object(roblox_class: &str) -> bool {
    matches!(
        roblox_class,
        "RayValue" | "IntConstrainedValue" | "DoubleConstrainedValue"
    )
}

// ---------------------------------------------------------------------------
// Encoding (Contract A)
// ---------------------------------------------------------------------------

/// Encode a single ValueObject child into the `toml::Value` that lands in
/// its parent's `[attributes]` table, per **Contract A**.
///
/// `props` is the child's raw `rbx_dom_weak` property map
/// (`inst.properties`, keyed by interned `Ustr`); the ValueObject's payload
/// lives under its `Value` property.
///
/// `resolve_ref` maps a Roblox [`Ref`] (an `ObjectValue`'s target) to a
/// resolved Eustress UUID hex string. Because the importer derives entity
/// UUIDs deterministically from `(space_salt, referent)`, this can resolve
/// *any* referent up-front — there is no walk-order dependency. When the
/// ref is null / unresolvable the encoder still emits an `ObjectValue`
/// attribute holding the empty string `""` (the caller records an
/// approximation); a missing attribute would silently drop the link.
///
/// Returns `None` for the dropped classes (`RayValue`,
/// `IntConstrainedValue`, `DoubleConstrainedValue`) — the caller then skips
/// the child and records an approximation.
pub fn encode_value_object(
    roblox_class: &str,
    props: &UstrMap<Variant>,
    resolve_ref: impl Fn(Ref) -> Option<String>,
) -> Option<toml::Value> {
    // Dropped classes: recognised but never converted.
    if is_dropped_value_object(roblox_class) {
        return None;
    }

    let value = props.get(&rbx_dom_weak::ustr("Value"));

    match roblox_class {
        "BoolValue" => match value {
            Some(Variant::Bool(b)) => Some(toml::Value::Boolean(*b)),
            // A BoolValue with no stored Value defaults to false in Roblox.
            _ => Some(toml::Value::Boolean(false)),
        },
        "IntValue" => match value {
            Some(Variant::Int64(i)) => Some(toml::Value::Integer(*i)),
            Some(Variant::Int32(i)) => Some(toml::Value::Integer(*i as i64)),
            // Some serialisers store the IntValue payload as a float.
            Some(Variant::Float64(f)) => Some(toml::Value::Integer(*f as i64)),
            Some(Variant::Float32(f)) => Some(toml::Value::Integer(*f as i64)),
            _ => Some(toml::Value::Integer(0)),
        },
        "NumberValue" => match value {
            Some(Variant::Float64(f)) => Some(toml::Value::Float(*f)),
            Some(Variant::Float32(f)) => Some(toml::Value::Float(*f as f64)),
            Some(Variant::Int64(i)) => Some(toml::Value::Float(*i as f64)),
            Some(Variant::Int32(i)) => Some(toml::Value::Float(*i as f64)),
            _ => Some(toml::Value::Float(0.0)),
        },
        "StringValue" => match value {
            Some(Variant::String(s)) => Some(toml::Value::String(s.clone())),
            _ => Some(toml::Value::String(String::new())),
        },
        "BinaryStringValue" => match value {
            // Folded as a bare string per Contract A. BinaryString may hold
            // non-UTF-8 bytes; lossily render so the attribute always lands.
            Some(Variant::BinaryString(bs)) => {
                let bytes: &[u8] = bs.as_ref();
                Some(toml::Value::String(
                    String::from_utf8_lossy(bytes).to_string(),
                ))
            }
            Some(Variant::String(s)) => Some(toml::Value::String(s.clone())),
            _ => Some(toml::Value::String(String::new())),
        },
        "Vector3Value" => match value {
            // Contract A: bare `[x, y, z]` — the loader's existing 3-float
            // array arm decodes it straight to `AttributeValue::Vector3`.
            Some(Variant::Vector3(v)) => Some(toml::Value::Array(vec![
                toml::Value::Float(v.x as f64),
                toml::Value::Float(v.y as f64),
                toml::Value::Float(v.z as f64),
            ])),
            _ => Some(toml::Value::Array(vec![
                toml::Value::Float(0.0),
                toml::Value::Float(0.0),
                toml::Value::Float(0.0),
            ])),
        },
        "Color3Value" => match value {
            // Contract A: `{ Color3 = [r, g, b] }` (0..1 floats).
            Some(Variant::Color3(c)) => Some(tagged_array(
                "Color3",
                vec![c.r as f64, c.g as f64, c.b as f64],
            )),
            Some(Variant::Color3uint8(c)) => Some(tagged_array(
                "Color3",
                vec![
                    c.r as f64 / 255.0,
                    c.g as f64 / 255.0,
                    c.b as f64 / 255.0,
                ],
            )),
            _ => Some(tagged_array("Color3", vec![0.0, 0.0, 0.0])),
        },
        "CFrameValue" => match value {
            // Contract A: `{ CFrame = [px, py, pz, qx, qy, qz, qw] }`.
            Some(Variant::CFrame(cf)) => {
                let (t, q) = cframe_to_translation_quat(cf);
                Some(tagged_array(
                    "CFrame",
                    vec![
                        t[0] as f64,
                        t[1] as f64,
                        t[2] as f64,
                        q[0] as f64,
                        q[1] as f64,
                        q[2] as f64,
                        q[3] as f64,
                    ],
                ))
            }
            _ => Some(tagged_array(
                "CFrame",
                // Identity: zero translation + identity quaternion.
                vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            )),
        },
        "BrickColorValue" => match value {
            // Contract A: `{ BrickColor = N }`. `BrickColor` is `#[repr(u16)]`
            // — the discriminant IS the palette/value the loader stores and
            // `BrickColor::from_number` round-trips.
            Some(Variant::BrickColor(bc)) => {
                Some(tagged_int("BrickColor", *bc as u16 as i64))
            }
            Some(Variant::Int64(i)) => Some(tagged_int("BrickColor", *i)),
            Some(Variant::Int32(i)) => Some(tagged_int("BrickColor", *i as i64)),
            // Default BrickColor 194 = "Medium stone grey".
            _ => Some(tagged_int("BrickColor", 194)),
        },
        "ObjectValue" => {
            // Contract A: a bare string holding the resolved Eustress UUID
            // hex. NOT a tagged table — the script side keys off import
            // context, not disk shape. `GetAttribute` returns this string
            // for the resolver to map back to an entity.
            let resolved = match value {
                Some(Variant::Ref(r)) if r.is_some() => resolve_ref(*r).unwrap_or_default(),
                _ => String::new(),
            };
            Some(toml::Value::String(resolved))
        }
        // `RayValue` and the *Constrained* classes are handled by the
        // `is_dropped_value_object` early-return above; any other class is
        // not a ValueObject and should not reach here.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a single-key inline table `{ tag = [floats...] }`.
fn tagged_array(tag: &str, floats: Vec<f64>) -> toml::Value {
    let mut t = toml::value::Table::new();
    t.insert(
        tag.to_string(),
        toml::Value::Array(floats.into_iter().map(toml::Value::Float).collect()),
    );
    toml::Value::Table(t)
}

/// Build a single-key inline table `{ tag = N }`.
fn tagged_int(tag: &str, n: i64) -> toml::Value {
    let mut t = toml::value::Table::new();
    t.insert(tag.to_string(), toml::Value::Integer(n));
    toml::Value::Table(t)
}

/// Roblox CFrame → `(translation, quaternion[x,y,z,w])`.
///
/// Mirrors `property_map::cframe_to_translation_quat` (kept local so this
/// module has no cross-module private dependency). Roblox stores rotation
/// as a row-major basis (right / up / back); we build the column-basis
/// matrix and convert to a quaternion via Shepperd's method.
fn cframe_to_translation_quat(cf: &rbx_dom_weak::types::CFrame) -> ([f32; 3], [f32; 4]) {
    let translation = [cf.position.x, cf.position.y, cf.position.z];
    let r = cf.orientation.x;
    let u = cf.orientation.y;
    let b = cf.orientation.z;
    let m = [[r.x, r.y, r.z], [u.x, u.y, u.z], [b.x, b.y, b.z]];
    (translation, mat3_to_quat(m))
}

/// Column-major 3×3 → quaternion `[x, y, z, w]` (Shepperd's method).
fn mat3_to_quat(m: [[f32; 3]; 3]) -> [f32; 4] {
    let m00 = m[0][0];
    let m11 = m[1][1];
    let m22 = m[2][2];
    let trace = m00 + m11 + m22;

    if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (m[1][2] - m[2][1]) / s,
            (m[2][0] - m[0][2]) / s,
            (m[0][1] - m[1][0]) / s,
            0.25 * s,
        ]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [
            0.25 * s,
            (m[1][0] + m[0][1]) / s,
            (m[2][0] + m[0][2]) / s,
            (m[1][2] - m[2][1]) / s,
        ]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [
            (m[1][0] + m[0][1]) / s,
            0.25 * s,
            (m[2][1] + m[1][2]) / s,
            (m[2][0] - m[0][2]) / s,
        ]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [
            (m[2][0] + m[0][2]) / s,
            (m[2][1] + m[1][2]) / s,
            0.25 * s,
            (m[0][1] - m[1][0]) / s,
        ]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom_weak::types::{BrickColor, Color3, Vector3};
    use rbx_dom_weak::HashMapExt;

    fn props_with(pairs: Vec<(&str, Variant)>) -> UstrMap<Variant> {
        let mut m = UstrMap::new();
        for (k, v) in pairs {
            m.insert(rbx_dom_weak::ustr(k), v);
        }
        m
    }

    fn no_ref(_: Ref) -> Option<String> {
        None
    }

    #[test]
    fn classifies_convertible_and_dropped() {
        for c in [
            "NumberValue",
            "IntValue",
            "BoolValue",
            "StringValue",
            "ObjectValue",
            "Color3Value",
            "Vector3Value",
            "CFrameValue",
            "BrickColorValue",
            "BinaryStringValue",
        ] {
            assert!(is_value_object_class(c), "{c} should be a ValueObject");
            assert!(is_convertible_value_object(c), "{c} should be convertible");
            assert!(!is_dropped_value_object(c), "{c} should not be dropped");
        }
        for c in ["RayValue", "IntConstrainedValue", "DoubleConstrainedValue"] {
            assert!(is_value_object_class(c), "{c} should be a ValueObject");
            assert!(is_dropped_value_object(c), "{c} should be dropped");
            assert!(
                !is_convertible_value_object(c),
                "{c} should not be convertible"
            );
        }
        assert!(!is_value_object_class("Part"));
        assert!(!is_value_object_class("Folder"));
    }

    #[test]
    fn bool_int_number_string_encode_bare() {
        assert_eq!(
            encode_value_object("BoolValue", &props_with(vec![("Value", Variant::Bool(true))]), no_ref),
            Some(toml::Value::Boolean(true))
        );
        assert_eq!(
            encode_value_object("IntValue", &props_with(vec![("Value", Variant::Int64(42))]), no_ref),
            Some(toml::Value::Integer(42))
        );
        assert_eq!(
            encode_value_object("NumberValue", &props_with(vec![("Value", Variant::Float64(4.5))]), no_ref),
            Some(toml::Value::Float(4.5))
        );
        assert_eq!(
            encode_value_object(
                "StringValue",
                &props_with(vec![("Value", Variant::String("hi".to_string()))]),
                no_ref
            ),
            Some(toml::Value::String("hi".to_string()))
        );
    }

    #[test]
    fn vector3_encodes_bare_three_float_array() {
        let got = encode_value_object(
            "Vector3Value",
            &props_with(vec![("Value", Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)))]),
            no_ref,
        )
        .unwrap();
        match got {
            toml::Value::Array(a) => {
                assert_eq!(a.len(), 3);
                assert_eq!(a[0], toml::Value::Float(1.0));
                assert_eq!(a[2], toml::Value::Float(3.0));
            }
            other => panic!("expected bare array, got {other:?}"),
        }
    }

    #[test]
    fn color3_encodes_tagged_table() {
        let got = encode_value_object(
            "Color3Value",
            &props_with(vec![("Value", Variant::Color3(Color3::new(1.0, 0.0, 0.5)))]),
            no_ref,
        )
        .unwrap();
        let tbl = got.as_table().expect("Color3 → table");
        let arr = tbl.get("Color3").and_then(|v| v.as_array()).expect("Color3 key");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], toml::Value::Float(1.0));
    }

    #[test]
    fn cframe_encodes_seven_float_tagged_table() {
        let cf = rbx_dom_weak::types::CFrame::new(
            Vector3::new(5.0, 6.0, 7.0),
            rbx_dom_weak::types::Matrix3::identity(),
        );
        let got = encode_value_object(
            "CFrameValue",
            &props_with(vec![("Value", Variant::CFrame(cf))]),
            no_ref,
        )
        .unwrap();
        let arr = got
            .as_table()
            .and_then(|t| t.get("CFrame"))
            .and_then(|v| v.as_array())
            .expect("CFrame key holds an array");
        assert_eq!(arr.len(), 7, "position(3) + quaternion(4)");
        assert_eq!(arr[0], toml::Value::Float(5.0));
        // Identity rotation → quaternion [0,0,0,1].
        assert_eq!(arr[6], toml::Value::Float(1.0));
    }

    #[test]
    fn brickcolor_encodes_tagged_int() {
        let got = encode_value_object(
            "BrickColorValue",
            &props_with(vec![("Value", Variant::BrickColor(BrickColor::ReallyRed))]),
            no_ref,
        )
        .unwrap();
        let tbl = got.as_table().expect("BrickColor → table");
        let n = tbl.get("BrickColor").and_then(|v| v.as_integer());
        assert_eq!(n, Some(BrickColor::ReallyRed as u16 as i64));
    }

    #[test]
    fn object_value_resolves_ref_to_uuid_string() {
        let target = Ref::new();
        let got = encode_value_object(
            "ObjectValue",
            &props_with(vec![("Value", Variant::Ref(target))]),
            |r| {
                if r == target {
                    Some("deadbeefdeadbeefdeadbeefdeadbeef".to_string())
                } else {
                    None
                }
            },
        )
        .unwrap();
        assert_eq!(
            got,
            toml::Value::String("deadbeefdeadbeefdeadbeefdeadbeef".to_string())
        );
    }

    #[test]
    fn object_value_null_ref_yields_empty_string() {
        let got = encode_value_object(
            "ObjectValue",
            &props_with(vec![("Value", Variant::Ref(Ref::none()))]),
            no_ref,
        )
        .unwrap();
        assert_eq!(got, toml::Value::String(String::new()));
    }

    #[test]
    fn dropped_classes_return_none() {
        for c in ["RayValue", "IntConstrainedValue", "DoubleConstrainedValue"] {
            assert_eq!(
                encode_value_object(c, &props_with(vec![]), no_ref),
                None,
                "{c} should drop"
            );
        }
    }
}

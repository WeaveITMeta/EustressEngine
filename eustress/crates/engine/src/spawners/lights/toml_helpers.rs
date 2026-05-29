//! Shared TOML descriptor readers + emitters used by every light
//! spawner.
//!
//! The lighting templates ship descriptors of the form
//! `Brightness = { type = "float", value = 100000.0, min = 0.0, ... }`
//! rather than raw scalars (`Brightness = 100000.0`). Wave 3 spawners
//! tolerate both — the template form for files synthesized by
//! `space_ops.rs::create_default_space`, the bare-scalar form for
//! hand-authored TOMLs (the most common shape after a user
//! find-and-replaces their lights).
//!
//! Per `LIGHTING_AUDIT.md` §4.X "TOML schema example" the on-disk form
//! is descriptor-wrapped; this module is the single place those
//! per-class schemas land + read so future spawners can adopt without
//! re-implementing the descriptor parsing.

use bevy::prelude::*;

/// Read `Key = { type = "float", value = X, ... }` or `Key = X`.
pub(super) fn read_descriptor_f32(section: &toml::Value, key: &str) -> Option<f32> {
    section.get(key).and_then(|v| {
        if let Some(table) = v.as_table() {
            if let Some(value) = table.get("value") {
                return value.as_float().map(|f| f as f32);
            }
        }
        v.as_float().map(|f| f as f32)
    })
}

/// Read `Key = { type = "bool", value = X, ... }` or `Key = X`.
pub(super) fn read_descriptor_bool(section: &toml::Value, key: &str) -> Option<bool> {
    section.get(key).and_then(|v| {
        if let Some(table) = v.as_table() {
            if let Some(value) = table.get("value") {
                return value.as_bool();
            }
        }
        v.as_bool()
    })
}

/// Read `Key = { type = "Color3", value = [r, g, b], ... }` or
/// `Key = [r, g, b]`.
pub(super) fn read_descriptor_color3(section: &toml::Value, key: &str) -> Option<[f32; 3]> {
    section.get(key).and_then(|v| {
        let array = if let Some(table) = v.as_table() {
            table.get("value")?.as_array()?
        } else {
            v.as_array()?
        };
        if array.len() < 3 {
            return None;
        }
        Some([
            array[0].as_float().unwrap_or(0.0) as f32,
            array[1].as_float().unwrap_or(0.0) as f32,
            array[2].as_float().unwrap_or(0.0) as f32,
        ])
    })
}

/// Read `Key = { type = "...", value = "string", ... }` or `Key = "..."`.
pub(super) fn read_descriptor_string(section: &toml::Value, key: &str) -> Option<String> {
    section.get(key).and_then(|v| {
        if let Some(table) = v.as_table() {
            if let Some(value) = table.get("value") {
                return value.as_str().map(str::to_string);
            }
        }
        v.as_str().map(str::to_string)
    })
}

/// Read `Key = { type = "enum", value = "Foo", options = [...] }` or
/// `Key = "Foo"`.
pub(super) fn read_descriptor_enum(section: &toml::Value, key: &str) -> Option<String> {
    // Same parse path as `read_descriptor_string` — kept as a separate
    // function so callers express intent ("this is an enum
    // discriminant, not free text").
    read_descriptor_string(section, key)
}

/// Read the `[transform]` section into a Bevy `Transform`. Returns
/// `None` only when the section itself is missing — partial sections
/// fall back to the per-field default (zero translation, identity
/// rotation, unit scale).
pub(super) fn read_transform_section(root: &toml::Value) -> Option<Transform> {
    let section = root.get("transform")?;
    let pos = section
        .get("position")
        .and_then(|v| v.as_array())
        .map(|a| {
            Vec3::new(
                a.first().and_then(|v| v.as_float()).unwrap_or(0.0) as f32,
                a.get(1).and_then(|v| v.as_float()).unwrap_or(0.0) as f32,
                a.get(2).and_then(|v| v.as_float()).unwrap_or(0.0) as f32,
            )
        });
    let rot = section
        .get("rotation")
        .and_then(|v| v.as_array())
        .map(|a| {
            Quat::from_xyzw(
                a.first().and_then(|v| v.as_float()).unwrap_or(0.0) as f32,
                a.get(1).and_then(|v| v.as_float()).unwrap_or(0.0) as f32,
                a.get(2).and_then(|v| v.as_float()).unwrap_or(0.0) as f32,
                a.get(3).and_then(|v| v.as_float()).unwrap_or(1.0) as f32,
            )
        });
    let scale = section.get("scale").and_then(|v| v.as_array()).map(|a| {
        Vec3::new(
            a.first().and_then(|v| v.as_float()).unwrap_or(1.0) as f32,
            a.get(1).and_then(|v| v.as_float()).unwrap_or(1.0) as f32,
            a.get(2).and_then(|v| v.as_float()).unwrap_or(1.0) as f32,
        )
    });
    Some(Transform {
        translation: pos.unwrap_or(Vec3::ZERO),
        rotation: rot.unwrap_or(Quat::IDENTITY),
        scale: scale.unwrap_or(Vec3::ONE),
    })
}

pub(super) fn transform_to_toml(t: Transform) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert(
        "position".into(),
        toml::Value::Array(vec![
            (t.translation.x as f64).into(),
            (t.translation.y as f64).into(),
            (t.translation.z as f64).into(),
        ]),
    );
    table.insert(
        "rotation".into(),
        toml::Value::Array(vec![
            (t.rotation.x as f64).into(),
            (t.rotation.y as f64).into(),
            (t.rotation.z as f64).into(),
            (t.rotation.w as f64).into(),
        ]),
    );
    table.insert(
        "scale".into(),
        toml::Value::Array(vec![
            (t.scale.x as f64).into(),
            (t.scale.y as f64).into(),
            (t.scale.z as f64).into(),
        ]),
    );
    toml::Value::Table(table)
}

// ── Descriptor emitters used by every `export_to_toml` ───────────────

pub(super) fn descriptor_f32(value: f32) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert("type".into(), toml::Value::String("float".into()));
    table.insert("value".into(), toml::Value::Float(value as f64));
    toml::Value::Table(table)
}

pub(super) fn descriptor_bool(value: bool) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert("type".into(), toml::Value::String("bool".into()));
    table.insert("value".into(), toml::Value::Boolean(value));
    toml::Value::Table(table)
}

pub(super) fn descriptor_color3(value: [f32; 3]) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert("type".into(), toml::Value::String("Color3".into()));
    table.insert(
        "value".into(),
        toml::Value::Array(vec![
            (value[0] as f64).into(),
            (value[1] as f64).into(),
            (value[2] as f64).into(),
        ]),
    );
    toml::Value::Table(table)
}

pub(super) fn descriptor_string(value: &str) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert("type".into(), toml::Value::String("string".into()));
    table.insert("value".into(), toml::Value::String(value.into()));
    toml::Value::Table(table)
}

pub(super) fn descriptor_enum(value: &str, options: &[&str]) -> toml::Value {
    let mut table = toml::value::Table::new();
    table.insert("type".into(), toml::Value::String("enum".into()));
    table.insert("value".into(), toml::Value::String(value.into()));
    table.insert(
        "options".into(),
        toml::Value::Array(
            options
                .iter()
                .map(|o| toml::Value::String((*o).into()))
                .collect(),
        ),
    );
    toml::Value::Table(table)
}

/// Convert a Bevy `Color` to the linear-sRGB triple our descriptors
/// store. Alpha is dropped (light components don't carry one).
pub(super) fn color_to_color3(color: Color) -> [f32; 3] {
    let s = color.to_srgba();
    [s.red, s.green, s.blue]
}

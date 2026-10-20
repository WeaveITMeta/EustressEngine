//! # One conversion from a world's records to the tree's properties
//!
//! Studio's Play session and a Player each build a DataModel tree of the same
//! world, and replication assumes the two start equal: only later writes
//! travel, never the loaded state (SA-1 in `docs/networking/SERVER_AUTHORITY.md`).
//! Studio seeds its tree from the ECS its loader filled
//! (`engine/src/play_datamodel/seed.rs`); a Player builds its tree straight
//! from the world's records ([`crate::tree_read`]). Everything that decides a
//! property's name, type or value lives here, and both call it:
//!
//! * [`part_props`]: a BasePart's properties, from a [`BasePart`] and its
//!   world pose. Studio passes the part its loader built; a Player passes
//!   [`base_part_from_record`].
//! * [`property_to_dm`] and [`attribute_to_dm`]: the Properties panel's values
//!   and attribute values as script values; [`component_props`] turns a
//!   component's listed properties into them, and [`record_attachment`] builds
//!   an Attachment's component from its file.
//! * [`toml_to_attribute`], [`attribute_to_toml`] and
//!   [`merge_attribute_table`]: an `[attributes]` entry, as every reader reads
//!   it and Studio writes it back.
//! * [`class_from_toml`]: a file's `class_name`, as Studio's loader resolves it.
//! * [`tree_class`], [`luau_script_class`], [`strip_script_suffix`] and
//!   [`folder_display_name`]: the class and name an instance has in the tree.
//! * [`record_props`]: one instance file, as Studio's loader builds it.
//! * [`record_name`], [`record_attribute_values`], [`record_tags`],
//!   [`record_model_pivot`], [`record_script_disabled`] and
//!   [`record_animation_id`]: one part of a file, for the loader branches
//!   that build the rest themselves.
//!   [`loads_as_part`] says which folders the loader builds as parts;
//!   [`general_branch_pose`], [`has_model_pivot`] and
//!   [`script_starts_disabled`] are the general branch's rules by class, and
//!   [`model_pivot_prop`] is a Model's pivot in the tree.
//!
//! ## Where a pose comes from
//!
//! A file's `[transform]` is relative to the pose of the instance it nests
//! under, never its size, and its `scale` is a part's own size
//! ([`compose_pose`]). Studio's loader converts position and scale from the
//! file's `[metadata] unit` to metres and parents the entity; a sized part
//! hangs its children from an unscaled anchor, so Bevy's propagation composes
//! poses only. The tree's `CFrame` is the world pose with the scale stripped
//! ([`world_cframe`]), and `Size` is the part's own size. [`record_props`]
//! composes onto the basis [`TransformRule::basis`] gives it. A Space whose
//! `space.toml` does not name the rule predates it: there a child composes
//! onto its parent's whole transform, size included, as it was written.

use bevy::prelude::*;
use serde::Deserialize;

use super::{is_base_part, DmValue, EnumItem};
use crate::attributes::{AttributeValue, ColorSequenceKeypoint, NumberSequenceKeypoint};
use crate::classes::{Attachment, BasePart, ClassName, Material, PartType, PropertyAccess, PropertyValue};
use crate::scripting::{CFrame, Color3, NumberRange, UDim, UDim2, Vector2, Vector3};
use crate::space_read::{authored_pose, decode_color, primitive_shape, shape_name};

// ============================================================================
// Values
// ============================================================================

/// A colour as the tree holds it: the sRGB channels.
pub fn color3_of(c: Color) -> Color3 {
    let s = c.to_srgba();
    Color3::new(s.red as f64, s.green as f64, s.blue as f64)
}

/// A world pose as the tree's `CFrame`: rotation and position.
pub fn cframe_of(translation: Vec3, rotation: Quat) -> CFrame {
    let mut cf = CFrame::from_quaternion([rotation.x as f64, rotation.y as f64, rotation.z as f64, rotation.w as f64]);
    cf.position = Vector3::from_vec3(translation);
    cf
}

/// World pose with the size-as-scale stripped.
pub fn world_cframe(gt: &GlobalTransform) -> CFrame {
    let (_, rot, pos) = gt.to_scale_rotation_translation();
    cframe_of(pos, rot)
}

/// `PropertyValue` (Properties panel) as a script value.
pub fn property_to_dm(name: &str, v: &PropertyValue) -> Option<DmValue> {
    Some(match v {
        PropertyValue::String(s) => DmValue::String(s.clone()),
        PropertyValue::Float(f) => DmValue::Number(*f as f64),
        PropertyValue::Int(i) => DmValue::Number(*i as f64),
        PropertyValue::Bool(b) => DmValue::Bool(*b),
        PropertyValue::Vector2(a) => DmValue::Vector2(Vector2::new(a[0] as f64, a[1] as f64)),
        PropertyValue::Vector3(v) => DmValue::Vector3(Vector3::from_vec3(*v)),
        PropertyValue::UDim2(u) => DmValue::UDim2(UDim2::new(
            u.x.scale as f64,
            u.x.offset as f64,
            u.y.scale as f64,
            u.y.offset as f64,
        )),
        PropertyValue::Color(c) => DmValue::Color3(color3_of(*c)),
        PropertyValue::Color3(c) => DmValue::Color3(Color3::new(c[0] as f64, c[1] as f64, c[2] as f64)),
        PropertyValue::Transform(t) => DmValue::CFrame(cframe_of(t.translation, t.rotation)),
        PropertyValue::Material(m) => DmValue::Enum(EnumItem::new("Material", m.as_str())),
        PropertyValue::Enum(s) => DmValue::Enum(EnumItem::parse(s, name)),
    })
}

/// A component's properties in the tree: each property it lists, through
/// [`property_to_dm`]. Studio's seed turns its components into properties with
/// it, and a Player's reader turns the component it builds from a file
/// ([`record_attachment`]) into the same properties.
pub fn component_props<T: PropertyAccess>(c: &T) -> Vec<(String, DmValue)> {
    let mut out = Vec::new();
    for d in c.list_properties() {
        if let Some(v) = c.get_property(&d.name) {
            if let Some(dv) = property_to_dm(&d.name, &v) {
                out.push((d.name.clone(), dv));
            }
        }
    }
    out
}

/// An Attachment's component, from its file. Its `[transform]` is its
/// `CFrame`, local to the part it hangs under, in metres ([`record_pose`]);
/// `Position` and `Orientation` (degrees, XYZ) follow from it. `[attachment]`
/// holds `visible`, `axis` and `secondary_axis`. `name` is the instance's.
/// Studio's loader attaches it and a Player's reader turns it into properties
/// ([`component_props`]), so both trees start equal.
pub fn record_attachment(doc: &toml::Value, name: &str) -> Attachment {
    let pose = record_pose(doc);
    let (x, y, z) = pose.rotation.to_euler(EulerRot::XYZ);
    let section = get_ci(doc, "attachment");
    let visible = section.and_then(|s| get_ci(s, "visible")).and_then(|v| v.as_bool()).unwrap_or(false);
    let axis = |key: &str, default: Vec3| {
        section.map_or(default, |s| Vec3::from_array(vec_of(get_ci(s, key), default.to_array())))
    };
    Attachment {
        position: pose.translation,
        orientation: Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees()),
        cframe: Transform { translation: pose.translation, rotation: pose.rotation, scale: Vec3::ONE },
        name: name.to_string(),
        visible,
        axis: axis("axis", Vec3::X),
        secondary_axis: axis("secondary_axis", Vec3::Y),
    }
}

/// An attribute value as a script value. `None` for the kinds the tree does
/// not model: object references, BrickColors, fonts and rects.
pub fn attribute_to_dm(v: &AttributeValue) -> Option<DmValue> {
    use AttributeValue as A;
    Some(match v {
        A::String(s) => DmValue::String(s.clone()),
        A::Number(n) => DmValue::Number(*n),
        A::Int(i) => DmValue::Number(*i as f64),
        A::Bool(b) => DmValue::Bool(*b),
        A::Vector2(v) => DmValue::Vector2(Vector2::new(v.x as f64, v.y as f64)),
        A::Vector3(v) => DmValue::Vector3(Vector3::from_vec3(*v)),
        A::Color(c) | A::Color3(c) => DmValue::Color3(color3_of(*c)),
        A::CFrame(t) => DmValue::CFrame(cframe_of(t.translation, t.rotation)),
        A::UDim2 { x_scale, x_offset, y_scale, y_offset } => {
            DmValue::UDim2(UDim2::new(*x_scale as f64, *x_offset as f64, *y_scale as f64, *y_offset as f64))
        }
        A::NumberRange { min, max } => DmValue::NumberRange(NumberRange::new(*min, *max)),
        A::UDim { scale, offset } => DmValue::UDim(UDim::new(*scale as f64, *offset as f64)),
        A::NumberSequence(kps) => {
            DmValue::NumberSequence(kps.iter().map(|k| (k.time as f64, k.value as f64)).collect())
        }
        A::ColorSequence(kps) => {
            DmValue::ColorSequence(kps.iter().map(|k| (k.time as f64, color3_of(k.color))).collect())
        }
        A::EnumItem { enum_type, name, .. } => DmValue::Enum(EnumItem::new(enum_type.as_str(), name.as_str())),
        _ => return None,
    })
}

/// An `[attributes]` entry as every reader reads it, and as
/// [`attribute_to_toml`] writes it. Studio's loader also reads the entries of
/// a meshless instance's other sections through it.
///
/// Scalars keep their kind (an integer stays an `Int`). A numeric array of 2,
/// 3 or 4 is a `Vector2`, a `Vector3` or an RGBA `Color`. Every other kind is
/// a one-key inline table named for its type, so no two kinds share a form:
///
/// | Kind | Form |
/// |---|---|
/// | `Color3` | `{ Color3 = [r, g, b] }`, channels 0 to 1 |
/// | `CFrame` | `{ CFrame = [px, py, pz, qx, qy, qz, qw] }` |
/// | `BrickColor` | `{ BrickColor = 21 }`, the palette number |
/// | `UDim` | `{ UDim = [scale, offset] }` |
/// | `UDim2` | `{ UDim2 = [x_scale, x_offset, y_scale, y_offset] }` |
/// | `Rect` | `{ Rect = [x0, y0, x1, y1] }` |
/// | `NumberRange` | `{ NumberRange = [min, max] }` |
/// | `NumberSequence` | `{ NumberSequence = [[time, value, envelope], ...] }` |
/// | `ColorSequence` | `{ ColorSequence = [[time, r, g, b], ...] }` |
/// | `Font` | `{ Font = { family = "...", weight = 400, style = "Normal" } }` |
/// | `EnumItem` | `{ EnumItem = { type = "Material", name = "Plastic", value = 256 } }` |
///
/// The Roblox importer writes Roblox attributes and folded value objects in
/// these forms. Anything else, such as its `{ Bytes = "<hex>" }`, is not an
/// attribute a script can use: it is not loaded, and [`merge_attribute_table`]
/// leaves it on disk as written.
pub fn toml_to_attribute(v: &toml::Value) -> Option<AttributeValue> {
    match v {
        toml::Value::Boolean(b) => Some(AttributeValue::Bool(*b)),
        toml::Value::Integer(i) => Some(AttributeValue::Int(*i)),
        toml::Value::Float(f) => Some(AttributeValue::Number(*f)),
        toml::Value::String(s) => Some(AttributeValue::String(s.clone())),
        toml::Value::Array(_) => {
            let f = numbers(v)?;
            match f.len() {
                2 => Some(AttributeValue::Vector2(Vec2::new(f[0] as f32, f[1] as f32))),
                3 => Some(AttributeValue::Vector3(Vec3::new(f[0] as f32, f[1] as f32, f[2] as f32))),
                4 => Some(AttributeValue::Color(Color::srgba(f[0] as f32, f[1] as f32, f[2] as f32, f[3] as f32))),
                _ => None,
            }
        }
        toml::Value::Table(tbl) if tbl.len() == 1 => {
            let (kind, body) = tbl.iter().next()?;
            tagged_attribute(kind, body)
        }
        _ => None,
    }
}

/// The value of a one-key table, by the kind its key names. The name matches
/// in any case, with or without underscores (`Color3`, `color3`, `c_frame`),
/// so a file whose keys were snake-cased still reads.
fn tagged_attribute(kind: &str, body: &toml::Value) -> Option<AttributeValue> {
    let f32s = |n: usize| numbers_n(body, n).map(|f| f.iter().map(|x| *x as f32).collect::<Vec<f32>>());
    let int = |v: &toml::Value| u32::try_from(v.as_integer()?).ok();
    let kind = kind.replace('_', "").to_ascii_lowercase();
    Some(match kind.as_str() {
        "color3" => {
            let c = f32s(3)?;
            AttributeValue::Color3(Color::srgb(c[0], c[1], c[2]))
        }
        "cframe" => {
            let c = f32s(7)?;
            AttributeValue::CFrame(Transform {
                translation: Vec3::new(c[0], c[1], c[2]),
                rotation: Quat::from_xyzw(c[3], c[4], c[5], c[6]),
                ..Default::default()
            })
        }
        "brickcolor" => AttributeValue::BrickColor(int(body)?),
        "udim" => {
            let u = f32s(2)?;
            AttributeValue::UDim { scale: u[0], offset: u[1] }
        }
        "udim2" => {
            let u = f32s(4)?;
            AttributeValue::UDim2 { x_scale: u[0], x_offset: u[1], y_scale: u[2], y_offset: u[3] }
        }
        "rect" => {
            let r = f32s(4)?;
            AttributeValue::Rect { min: Vec2::new(r[0], r[1]), max: Vec2::new(r[2], r[3]) }
        }
        "numberrange" => {
            let r = numbers_n(body, 2)?;
            AttributeValue::NumberRange { min: r[0], max: r[1] }
        }
        "numbersequence" => AttributeValue::NumberSequence(
            keypoints(body, 3)?
                .iter()
                .map(|k| NumberSequenceKeypoint { time: k[0] as f32, value: k[1] as f32, envelope: k[2] as f32 })
                .collect(),
        ),
        "colorsequence" => AttributeValue::ColorSequence(
            keypoints(body, 4)?
                .iter()
                .map(|k| ColorSequenceKeypoint {
                    time: k[0] as f32,
                    color: Color::srgb(k[1] as f32, k[2] as f32, k[3] as f32),
                })
                .collect(),
        ),
        "font" => {
            let t = body.as_table()?;
            AttributeValue::Font {
                family: t.get("family")?.as_str()?.to_string(),
                weight: match t.get("weight") {
                    Some(w) => int(w)?,
                    None => 400,
                },
                style: match t.get("style") {
                    Some(s) => s.as_str()?.to_string(),
                    None => "Normal".to_string(),
                },
            }
        }
        "enumitem" => {
            let t = body.as_table()?;
            AttributeValue::EnumItem {
                enum_type: t.get("type")?.as_str()?.to_string(),
                name: t.get("name")?.as_str()?.to_string(),
                value: match t.get("value") {
                    Some(n) => Some(int(n)?),
                    None => None,
                },
            }
        }
        _ => return None,
    })
}

/// Every entry of a numeric array, or `None` when an entry is not a number.
fn numbers(v: &toml::Value) -> Option<Vec<f64>> {
    v.as_array()?
        .iter()
        .map(|item| match item {
            toml::Value::Float(f) => Some(*f),
            toml::Value::Integer(i) => Some(*i as f64),
            _ => None,
        })
        .collect()
}

/// A numeric array of exactly `n` entries.
fn numbers_n(v: &toml::Value, n: usize) -> Option<Vec<f64>> {
    numbers(v).filter(|f| f.len() == n)
}

/// A sequence's keypoints: an array of numeric arrays of `width` entries.
fn keypoints(v: &toml::Value, width: usize) -> Option<Vec<Vec<f64>>> {
    v.as_array()?.iter().map(|k| numbers_n(k, width)).collect()
}

/// An attribute value as an `[attributes]` entry, in the form
/// [`toml_to_attribute`] reads back as the same value. `None` for `Object`
/// and `EntityRef`, which name entities of one session.
///
/// Single-precision fields are written as their shortest decimal, so a
/// channel of 0.1 stays `0.1` in the file.
pub fn attribute_to_toml(v: &AttributeValue) -> Option<toml::Value> {
    use toml::Value as T;
    fn tagged(kind: &str, body: T) -> T {
        let mut t = toml::Table::new();
        t.insert(kind.to_string(), body);
        T::Table(t)
    }
    fn f64s(values: &[f64]) -> T {
        T::Array(values.iter().map(|x| T::Float(*x)).collect())
    }
    fn f32s(values: &[f32]) -> T {
        T::Array(values.iter().map(|x| T::Float(shortest(*x))).collect())
    }
    fn rgb(c: &Color) -> [f32; 3] {
        let s = c.to_srgba();
        [s.red, s.green, s.blue]
    }
    Some(match v {
        AttributeValue::String(s) => T::String(s.clone()),
        AttributeValue::Number(n) => T::Float(*n),
        AttributeValue::Int(i) => T::Integer(*i),
        AttributeValue::Bool(b) => T::Boolean(*b),
        AttributeValue::Vector2(p) => f32s(&[p.x, p.y]),
        AttributeValue::Vector3(p) => f32s(&[p.x, p.y, p.z]),
        AttributeValue::Color(c) => {
            let s = c.to_srgba();
            f32s(&[s.red, s.green, s.blue, s.alpha])
        }
        AttributeValue::Color3(c) => tagged("Color3", f32s(&rgb(c))),
        AttributeValue::BrickColor(n) => tagged("BrickColor", T::Integer(i64::from(*n))),
        AttributeValue::CFrame(t) => {
            let (p, q) = (t.translation, t.rotation);
            tagged("CFrame", f32s(&[p.x, p.y, p.z, q.x, q.y, q.z, q.w]))
        }
        AttributeValue::Object(_) | AttributeValue::EntityRef(_) => return None,
        AttributeValue::UDim { scale, offset } => tagged("UDim", f32s(&[*scale, *offset])),
        AttributeValue::UDim2 { x_scale, x_offset, y_scale, y_offset } => {
            tagged("UDim2", f32s(&[*x_scale, *x_offset, *y_scale, *y_offset]))
        }
        AttributeValue::Rect { min, max } => tagged("Rect", f32s(&[min.x, min.y, max.x, max.y])),
        AttributeValue::NumberRange { min, max } => tagged("NumberRange", f64s(&[*min, *max])),
        AttributeValue::NumberSequence(kps) => tagged(
            "NumberSequence",
            T::Array(kps.iter().map(|k| f32s(&[k.time, k.value, k.envelope])).collect()),
        ),
        AttributeValue::ColorSequence(kps) => tagged(
            "ColorSequence",
            T::Array(
                kps.iter()
                    .map(|k| {
                        let [r, g, b] = rgb(&k.color);
                        f32s(&[k.time, r, g, b])
                    })
                    .collect(),
            ),
        ),
        AttributeValue::Font { family, weight, style } => {
            let mut t = toml::Table::new();
            t.insert("family".into(), T::String(family.clone()));
            t.insert("weight".into(), T::Integer(i64::from(*weight)));
            t.insert("style".into(), T::String(style.clone()));
            tagged("Font", T::Table(t))
        }
        AttributeValue::EnumItem { enum_type, name, value } => {
            let mut t = toml::Table::new();
            t.insert("type".into(), T::String(enum_type.clone()));
            t.insert("name".into(), T::String(name.clone()));
            if let Some(n) = value {
                t.insert("value".into(), T::Integer(i64::from(*n)));
            }
            tagged("EnumItem", T::Table(t))
        }
    })
}

/// A single-precision value as the double its shortest decimal names:
/// `0.1_f32` becomes `0.1`, not `0.10000000149011612`, and still reads back as
/// the same `f32`.
fn shortest(x: f32) -> f64 {
    match x.to_string().parse::<f64>() {
        Ok(y) if y as f32 == x => y,
        _ => f64::from(x),
    }
}

/// The `[attributes]` table to write back: `fresh` (an entity's attributes,
/// through [`attribute_to_toml`]) over the table on disk. An entry
/// [`toml_to_attribute`] cannot read stays as written unless `fresh` has its
/// name, since no session loaded it, so none could have edited or removed it.
/// A readable entry that `fresh` lacks was removed. `None` when nothing is
/// left, and the table goes.
///
/// This holds only for an entity whose attributes were loaded from this same
/// table: one that started empty would read as every entry removed.
pub fn merge_attribute_table(
    existing: Option<&toml::Table>,
    fresh: impl IntoIterator<Item = (String, toml::Value)>,
) -> Option<toml::Table> {
    let mut merged: toml::Table = fresh.into_iter().collect();
    for (name, value) in existing.into_iter().flatten() {
        if !merged.contains_key(name) && toml_to_attribute(value).is_none() {
            merged.insert(name.clone(), value.clone());
        }
    }
    (!merged.is_empty()).then_some(merged)
}

// ============================================================================
// Classes and names
// ============================================================================

/// A file's `class_name` as Studio's loader resolves it: the legacy `Script`
/// is a `SoulScript` folder, and a name the engine does not know is a
/// `Folder`. Mirrors `representation::class_from_toml`, which the loader,
/// the bake and the export share.
pub fn class_from_toml(raw: &str) -> ClassName {
    let resolved = if raw == "Script" { "SoulScript" } else { raw };
    ClassName::from_str(resolved).unwrap_or(ClassName::Folder)
}

/// A folder's name as Studio names the entity it makes for most folder
/// classes: the first letter capitalised.
pub fn folder_display_name(folder: &str) -> String {
    let mut chars = folder.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// The class a loaded instance has in the tree, as Studio's seed names it:
/// the Luau script classes by their Roblox names, and a `Part` whose mesh is
/// not one of the engine's primitives as a `MeshPart`. A `SoulScript` holding
/// Luau takes its class from [`luau_script_class`] instead.
pub fn tree_class(class: ClassName, custom_mesh: bool) -> String {
    match class {
        ClassName::LuauScript => "Script".to_string(),
        ClassName::LuauLocalScript => "LocalScript".to_string(),
        ClassName::LuauModuleScript => "ModuleScript".to_string(),
        ClassName::Part if custom_mesh => "MeshPart".to_string(),
        other => other.as_str().to_string(),
    }
}

/// The class of a Luau source file, by its Rojo-style name: `x.server.luau` is
/// a Script, `x.client.luau` a LocalScript, `x.module.luau` a ModuleScript. A
/// plain name is a ModuleScript where Roblox never runs code (the storage
/// services) and a Script everywhere else, so existing Spaces keep running.
pub fn luau_script_class(file_name: &str, service: &str) -> &'static str {
    let file = file_name.to_lowercase();
    if file.contains(".server.") {
        "Script"
    } else if file.contains(".client.") {
        "LocalScript"
    } else if file.contains(".module.") {
        "ModuleScript"
    } else if matches!(service, "ReplicatedStorage" | "ServerStorage" | "ReplicatedFirst") {
        "ModuleScript"
    } else {
        "Script"
    }
}

/// A Luau script's name without its Rojo suffix (`Door.server` is `Door`).
pub fn strip_script_suffix(name: &str) -> String {
    let mut name = name.to_string();
    for suffix in [".server", ".client", ".module"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.to_string();
        }
    }
    name
}

// ============================================================================
// Parts
// ============================================================================

/// The colour a part has when its file gives none, or gives fewer than three
/// channels: medium grey, `[163, 162, 165]`.
pub const DEFAULT_PART_COLOR: [f32; 4] = [163.0 / 255.0, 162.0 / 255.0, 165.0 / 255.0, 1.0];

/// A part file's `[properties]`, with the defaults of Studio's loader
/// (`InstanceProperties`): unanchored, colliding, casting shadows, Plastic,
/// medium grey.
#[derive(Debug, Clone, Deserialize)]
pub struct PartProperties {
    #[serde(default)]
    pub color: Option<Vec<toml::Value>>,
    #[serde(default)]
    pub transparency: f32,
    #[serde(default)]
    pub anchored: bool,
    #[serde(default = "yes")]
    pub can_collide: bool,
    #[serde(default = "yes")]
    pub cast_shadow: bool,
    #[serde(default)]
    pub reflectance: f32,
    #[serde(default = "plastic")]
    pub material: String,
    #[serde(default)]
    pub locked: bool,
}

impl Default for PartProperties {
    fn default() -> Self {
        Self {
            color: None,
            transparency: 0.0,
            anchored: false,
            can_collide: true,
            cast_shadow: true,
            reflectance: 0.0,
            material: plastic(),
            locked: false,
        }
    }
}

fn yes() -> bool {
    true
}

fn plastic() -> String {
    "Plastic".to_string()
}

impl PartProperties {
    /// The colour as RGBA, by the engine's rule: an all-integer array is
    /// 0-255, anything holding a float is 0-1, and fewer than three channels
    /// is [`DEFAULT_PART_COLOR`].
    pub fn rgba(&self) -> [f32; 4] {
        match self.color.as_deref() {
            Some(raw) if raw.len() >= 3 => {
                let c = decode_color(raw).to_srgba();
                [c.red, c.green, c.blue, c.alpha]
            }
            _ => DEFAULT_PART_COLOR,
        }
    }
}

/// The `BasePart` Studio's loader builds for a part file: its size, colour,
/// transparency, reflectance, anchoring, collision, lock, shadow and material
/// from `[properties]`, and every other field at its default.
pub fn base_part_from_record(props: &PartProperties, size: Vec3, pose: Transform) -> BasePart {
    let [r, g, b, a] = props.rgba();
    BasePart {
        size,
        color: Color::srgba(r, g, b, a),
        transparency: props.transparency,
        reflectance: props.reflectance,
        anchored: props.anchored,
        can_collide: props.can_collide,
        locked: props.locked,
        cast_shadow: props.cast_shadow,
        material: Material::from_string(&props.material),
        material_name: props.material.clone(),
        cframe: pose,
        ..Default::default()
    }
}

/// A BasePart's properties in the tree: its world pose, its own size, its
/// look and its physics flags, then `Shape` when the part has one and
/// `MeshId` for a `MeshPart`. The material is the file's own name when it
/// has one (a `.mat.toml` library entry keeps its name) and the preset's
/// otherwise.
pub fn part_props(bp: &BasePart, cframe: CFrame, shape: Option<PartType>, mesh_id: Option<String>) -> Vec<(String, DmValue)> {
    let mut props: Vec<(String, DmValue)> = Vec::with_capacity(15);
    props.push(("CFrame".into(), DmValue::CFrame(cframe)));
    props.push(("Size".into(), DmValue::Vector3(Vector3::from_vec3(bp.size))));
    props.push(("Color".into(), DmValue::Color3(color3_of(bp.color))));
    let mat = if !bp.material_name.is_empty() { bp.material_name.clone() } else { bp.material.as_str().to_string() };
    props.push(("Material".into(), DmValue::Enum(EnumItem::new("Material", mat))));
    props.push(("Transparency".into(), DmValue::Number(bp.transparency as f64)));
    props.push(("Reflectance".into(), DmValue::Number(bp.reflectance as f64)));
    props.push(("Anchored".into(), DmValue::Bool(bp.anchored)));
    props.push(("CanCollide".into(), DmValue::Bool(bp.can_collide)));
    props.push(("CanTouch".into(), DmValue::Bool(bp.can_touch)));
    props.push(("CastShadow".into(), DmValue::Bool(bp.cast_shadow)));
    props.push(("Locked".into(), DmValue::Bool(bp.locked)));
    props.push(("Mass".into(), DmValue::Number(bp.mass as f64)));
    props.push(("CollisionGroup".into(), DmValue::String(bp.collision_group.clone())));
    if let Some(shape) = shape {
        props.push(("Shape".into(), DmValue::Enum(EnumItem::new("PartType", shape_name(shape)))));
    }
    if let Some(mesh) = mesh_id {
        props.push(("MeshId".into(), DmValue::String(mesh)));
    }
    props
}

// ============================================================================
// One instance file
// ============================================================================

/// Top-level keys Studio's loader reads into typed fields. Every other key is
/// an extra section, and on an instance with no mesh its entries become
/// attributes.
const TYPED_KEYS: &[&str] = &[
    "asset",
    "transform",
    "properties",
    "metadata",
    "material",
    "thermodynamic",
    "electrochemical",
    "nuclear",
    "plasma",
    "ui",
    "attributes",
    "tags",
    "parameters",
    // The Roblox importer's folded DataMesh: removed before attributes are
    // read, since it only moves what is drawn.
    "mesh_scale",
    "mesh_offset",
];

/// Whether an extra section of an instance file is its class's own field
/// table, never attributes. Studio's `spawn_instance` and [`record_props`]
/// both skip these when they turn a meshless instance's other sections into
/// attributes: `[gaussian_splats]`, the particle and terrain-layer sections, a
/// light's `[light]`, a KeyframeSequence's `[keyframe_sequence]`, and a Star's,
/// Moon's, Sky's or Atmosphere's own section with the old Lighting templates'
/// descriptor sections.
pub fn class_owned_section(class: ClassName, section: &str) -> bool {
    section == "gaussian_splats"
        || section == crate::realism::particle_sim::class::SIMULATION_SECTION
        || section == crate::realism::particle_sim::class::SPECIES_SECTION
        || crate::terrain::layer_instances::LAYER_SECTIONS.contains(&section)
        || (crate::plugins::light_classes::is_light_class(class) && section.eq_ignore_ascii_case("light"))
        || (class == ClassName::KeyframeSequence && section == "keyframe_sequence")
        // A Sun's `[star]`, a Moon's `[moon]`, a Sky's `[sky]`, an
        // Atmosphere's `[atmosphere]`, and the old Lighting templates'
        // descriptor sections, which were never attributes.
        || crate::plugins::celestial_sections::owns_section(class, section)
}

/// Which of Studio's two loading paths an instance file takes. Both end in
/// `spawn_instance` after the file is healed with its class template; they
/// differ in how the class name resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordForm {
    /// A flat `<Name>.instance.toml`: the class resolves with
    /// `ClassName::from_str`, where `Script` is a `LuauScript` and a name the
    /// engine does not know is a `Part`.
    Flat,
    /// A folder's `_instance.toml` of a class Studio spawns from its file (a
    /// `Part`, a particle simulation or a terrain layer): the class resolves
    /// with [`class_from_toml`].
    Folder,
}

/// One instance file, handed over by a world's reader.
pub struct InstanceRecord<'a> {
    /// The file's document. It is healed with its class template here, as
    /// Studio's loader heals it.
    pub doc: toml::Value,
    /// The file's Space-relative path.
    pub key: &'a str,
    /// The name when the file gives none: the folder's name, or a flat file's
    /// name up to its first dot.
    pub fallback_name: &'a str,
    pub form: RecordForm,
    /// The world pose of the instance this one nests under.
    pub parent_world: GlobalTransform,
}

/// An instance file read into the tree's terms.
#[derive(Debug, Clone)]
pub struct RecordInstance {
    /// The engine class the file resolves to.
    pub engine_class: ClassName,
    /// The class in the tree ([`tree_class`]).
    pub class: String,
    pub name: String,
    pub props: Vec<(String, DmValue)>,
    pub attributes: Vec<(String, DmValue)>,
    pub tags: Vec<String>,
    /// This instance's world pose, for the instances nested under it.
    pub world: GlobalTransform,
    /// Whether Studio draws a mesh for it: the file has an `[asset]`, or it is
    /// a `Part`, which the loader gives a block when it names none. Only such
    /// an instance is a BasePart in Studio, whatever its class.
    pub has_mesh: bool,
    /// The `[script]` table, for the reader to find a script's source.
    pub script: Option<toml::Table>,
    /// What could not be read, for the reader's report.
    pub problems: Vec<String>,
}

/// A table key in either spelling (`class_name` / `ClassName`).
fn get_ci<'v>(v: &'v toml::Value, key: &str) -> Option<&'v toml::Value> {
    crate::class_schema::get_section_insensitive(v, key)
}

fn vec_of<const N: usize>(v: Option<&toml::Value>, default: [f32; N]) -> [f32; N] {
    let Some(arr) = v.and_then(|v| v.as_array()) else { return default };
    if arr.len() != N {
        return default;
    }
    let mut out = default;
    for (slot, item) in out.iter_mut().zip(arr) {
        match item.as_float().or_else(|| item.as_integer().map(|n| n as f64)) {
            Some(x) => *slot = x as f32,
            None => return default,
        }
    }
    out
}

/// The mesh Studio's loader gives a part with no `[asset]`, of any
/// [`loads_as_part`] class (a Seat is a part). A wedge is a `Part` whose
/// `[asset]` names the wedge: the engine has no wedge class.
const DEFAULT_PART_MESH: &str = "parts/block.glb";

/// A file's own pose: its `[transform]` in the unit its `[metadata] unit`
/// names, converted to metres and cleaned as the loader cleans it. `scale`
/// is a part's size.
pub fn record_pose(doc: &toml::Value) -> Transform {
    let unit = get_ci(doc, "metadata").and_then(|m| get_ci(m, "unit")).and_then(|u| u.as_str());
    let t = get_ci(doc, "transform");
    authored_pose(
        vec_of(t.and_then(|t| get_ci(t, "position")), [0.0; 3]),
        vec_of(t.and_then(|t| get_ci(t, "rotation")), [0.0, 0.0, 0.0, 1.0]),
        vec_of(t.and_then(|t| get_ci(t, "scale")), [1.0; 3]),
        unit,
    )
}

/// A child's world pose: its `[transform]` carried by the parent's rotation
/// and position, never the parent's size. The result's scale is the child's
/// own (a part's size).
pub fn compose_pose(parent: Isometry3d, local: &Transform) -> Transform {
    Transform {
        translation: parent.transform_point(local.translation).into(),
        rotation: parent.rotation * local.rotation,
        scale: local.scale,
    }
}

/// The inverse of [`compose_pose`]: a world pose as the `[transform]` that
/// places it under a parent's pose.
pub fn pose_relative_to(parent: Isometry3d, world: &Transform) -> Transform {
    Transform {
        translation: parent.inverse_transform_point(world.translation).into(),
        rotation: parent.rotation.inverse() * world.rotation,
        scale: world.scale,
    }
}

/// How a Space composes a child's `[transform]` onto its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformRule {
    /// Onto the parent's pose, never its size ([`compose_pose`]): the Space's
    /// `space.toml` says `[space] transform_rule = "parent_pose"`.
    ParentPose,
    /// Onto the parent's whole transform, size included: a Space written
    /// before the rule, whose `space.toml` does not name it.
    #[default]
    Legacy,
}

impl TransformRule {
    /// The rule a Space's `space.toml` names.
    pub fn of_space(space_toml: Option<&toml::Value>) -> Self {
        let named = space_toml
            .and_then(|doc| get_ci(doc, "space"))
            .and_then(|s| get_ci(s, "transform_rule"))
            .and_then(|r| r.as_str());
        if named == Some("parent_pose") { Self::ParentPose } else { Self::Legacy }
    }

    /// What a child composes onto, from its parent's world transform: the
    /// parent's pose alone, or all of it under the legacy rule.
    pub fn basis(self, parent_world: GlobalTransform) -> GlobalTransform {
        match self {
            Self::ParentPose => GlobalTransform::from_isometry(parent_world.to_isometry()),
            Self::Legacy => parent_world,
        }
    }

    /// The pose the general branch gives a folder of `class`: its file's
    /// pose without the scale, or under the legacy rule
    /// [`general_branch_pose`], whose exceptions keep such a Space as it was
    /// written.
    pub fn folder_pose(self, class: ClassName, doc: &toml::Value) -> Transform {
        match self {
            Self::ParentPose => Transform { scale: Vec3::ONE, ..record_pose(doc) },
            Self::Legacy => general_branch_pose(class, doc),
        }
    }

    /// Whether the general branch applies a folder's own `[transform]` under
    /// this rule. Under the legacy rule an Attachment, a Bone and a part class
    /// keep the identity.
    pub fn folder_is_posed(self, class: ClassName) -> bool {
        match self {
            Self::ParentPose => true,
            Self::Legacy => {
                !(matches!(class, ClassName::Attachment | ClassName::Bone) || is_base_part(class.as_str()))
            }
        }
    }
}

/// A file's own name: its `[metadata] name` exactly as written. `None` when
/// it has none or an empty one; the caller then names the instance after its
/// file or folder.
pub fn record_name(doc: &toml::Value) -> Option<&str> {
    get_ci(doc, "metadata")
        .and_then(|m| get_ci(m, "name"))
        .and_then(|n| n.as_str())
        .filter(|n| !n.is_empty())
}

/// A file's typed `[attributes]` table, as the loader fills an entity's
/// `Attributes` from it. An entry no reader can use is left out; it stays on
/// disk ([`merge_attribute_table`]).
pub fn record_attribute_values(doc: &toml::Value) -> Vec<(String, AttributeValue)> {
    get_ci(doc, "attributes")
        .and_then(|a| a.as_table())
        .map(|t| t.iter().filter_map(|(k, v)| Some((k.clone(), toml_to_attribute(v)?))).collect())
        .unwrap_or_default()
}

/// A file's tags: its root `tags`, or the `[metadata] tags` an older file
/// carries when the root has none. A root `tags` that is present but empty
/// means no tags.
pub fn record_tags(doc: &toml::Value) -> Vec<String> {
    let strings = |v: &toml::Value| -> Vec<String> {
        v.as_array()
            .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    };
    match get_ci(doc, "tags") {
        Some(root) => strings(root),
        None => get_ci(doc, "metadata").and_then(|m| get_ci(m, "tags")).map(strings).unwrap_or_default(),
    }
}

/// A Model's stored pivot: the `world_pivot` of its `[model]` section (a
/// WorldModel's `[world_model]`), in world space and metres as written; the
/// file's unit applies to `[transform]` only. A pivot has no scale. `None`
/// when the file stores none.
pub fn record_model_pivot(doc: &toml::Value) -> Option<Transform> {
    let pivot = ["model", "world_model"]
        .iter()
        .find_map(|section| get_ci(doc, section).and_then(|s| get_ci(s, "world_pivot")))?;
    let rotation = Quat::from_array(vec_of(get_ci(pivot, "rotation"), [0.0, 0.0, 0.0, 1.0]));
    Some(Transform {
        translation: Vec3::from_array(vec_of(get_ci(pivot, "position"), [0.0; 3])),
        rotation: if rotation.length_squared() > 0.0 { rotation.normalize() } else { Quat::IDENTITY },
        scale: Vec3::ONE,
    })
}

/// A Model's `WorldPivot` in the tree. `None` for the identity, which
/// `GetPivot` reads as no stored pivot, so the property is there exactly when
/// the Model has a pivot of its own.
pub fn model_pivot_prop(pivot: Transform) -> Option<(String, DmValue)> {
    (pivot.translation != Vec3::ZERO || pivot.rotation != Quat::IDENTITY)
        .then(|| ("WorldPivot".to_string(), DmValue::CFrame(cframe_of(pivot.translation, pivot.rotation))))
}

/// An Animation's clip: its `[properties] animation_id`, as Studio's
/// `Animation` component holds it. `None` when the file names none.
pub fn record_animation_id(doc: &toml::Value) -> Option<String> {
    get_ci(doc, "properties")
        .and_then(|p| get_ci(p, "animation_id"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// Whether a file switches its script off: `[script] enabled = false`.
pub fn record_script_disabled(doc: &toml::Value) -> bool {
    get_ci(doc, "script").and_then(|s| get_ci(s, "enabled")).and_then(|e| e.as_bool()) == Some(false)
}

/// Whether a script starts as a disabled template, never run where it sits:
/// a Script or LocalScript whose file switches it off. A ModuleScript has no
/// `Disabled`.
pub fn script_starts_disabled(class: ClassName, doc: &toml::Value) -> bool {
    matches!(class, ClassName::LuauScript | ClassName::LuauLocalScript) && record_script_disabled(doc)
}

/// The classes whose folders Studio builds as parts, through `spawn_instance`:
/// every part class the engine has. The importer writes MeshPart, WedgePart,
/// TrussPart and the rest as `Part`. A part folder's files are its own assets;
/// only its subfolders hold instances.
pub fn loads_as_part(class: ClassName) -> bool {
    matches!(
        class,
        ClassName::Part | ClassName::UnionOperation | ClassName::Seat | ClassName::VehicleSeat | ClassName::SpawnLocation
    )
}

/// The classes that carry a stored pivot: a Model and its subclasses.
pub fn has_model_pivot(class: ClassName) -> bool {
    matches!(class, ClassName::Model | ClassName::WorldModel | ClassName::Actor)
}

/// The pose Studio's general branch gives a folder of `class`: its file's
/// [`record_pose`] without the scale, since a folder has no size and its
/// `[transform]` never scales what it holds. Two kinds keep the identity,
/// because composing their `[transform]` onto a parent's whole transform
/// would misplace them: an Attachment or a Bone, whose `[transform]` is
/// local to the part it hangs under, and a part class that reaches this
/// branch (a Seat, a SpawnLocation), whose `[transform]` is a world pose that
/// its imported children repeat.
pub fn general_branch_pose(class: ClassName, doc: &toml::Value) -> Transform {
    if TransformRule::Legacy.folder_is_posed(class) {
        Transform { scale: Vec3::ONE, ..record_pose(doc) }
    } else {
        Transform::IDENTITY
    }
}

/// A file's raw `class_name`, looked up as Studio's loader looks it up.
pub fn raw_class_name(doc: &toml::Value) -> Option<&str> {
    let meta = doc.get("metadata").or_else(|| doc.get("Metadata"))?;
    meta.get("class_name").or_else(|| meta.get("ClassName"))?.as_str()
}

/// Read one instance file as Studio's `spawn_instance` builds it: healed with
/// its class template, its class resolved by its [`RecordForm`], its pose
/// composed onto its parent's in metres, BasePart properties from
/// [`part_props`] when it draws a mesh, its attributes from `[attributes]`
/// (and, when it draws no mesh, from its other sections), and its tags.
///
/// Studio's seed adds a few properties from components this function does not
/// model: a `Humanoid`'s, the GUI element classes' and the light, sound,
/// emitter, beam, decal, attachment and special-mesh classes'. Those
/// instances get their class defaults here; [`reads_own_properties`] says
/// which classes are fully read.
pub fn record_props(rec: InstanceRecord<'_>) -> RecordInstance {
    let mut problems = Vec::new();
    let engine_class = match rec.form {
        RecordForm::Flat => raw_class_name(&rec.doc)
            .and_then(|raw| ClassName::from_str(raw).ok())
            .unwrap_or(ClassName::Part),
        // A folder whose file names no class is a Folder.
        RecordForm::Folder => raw_class_name(&rec.doc).map(class_from_toml).unwrap_or(ClassName::Folder),
    };

    // Healed with the class template, as the loader heals before parsing:
    // key spellings normalised, missing keys filled from the template.
    let registry = crate::class_schema::ClassSchemaRegistry::builtin();
    let doc = match crate::class_schema::heal_instance_value(rec.doc, registry) {
        Ok(h) => h.value,
        Err(e) => {
            problems.push(format!("{}: {e}", rec.key));
            toml::Value::Table(toml::Table::new())
        }
    };

    let metadata = get_ci(&doc, "metadata");
    let name = metadata
        .and_then(|m| get_ci(m, "name"))
        .and_then(|n| n.as_str())
        .filter(|n| !n.is_empty())
        .unwrap_or(rec.fallback_name)
        .to_string();
    let unit = metadata.and_then(|m| get_ci(m, "unit")).and_then(|u| u.as_str());
    if let Some(symbol) = unit.filter(|s| crate::units::Unit::from_symbol(s).is_none()) {
        problems.push(format!("{}: unknown unit {symbol:?}, read as metres", rec.key));
    }

    // The mesh: a primitive names the part's shape, anything else is custom.
    let mesh = get_ci(&doc, "asset")
        .and_then(|a| get_ci(a, "mesh"))
        .and_then(|m| m.as_str())
        .filter(|m| !m.trim().is_empty())
        .map(str::to_owned);
    let mesh = match mesh {
        None if loads_as_part(engine_class) => Some(DEFAULT_PART_MESH.to_string()),
        other => other,
    };
    let has_mesh = mesh.is_some();
    let custom_mesh = mesh.as_deref().is_some_and(|m| primitive_shape(m).is_none());
    let shape = mesh.as_deref().map(|m| primitive_shape(m).unwrap_or(PartType::Block));

    // The pose: the file's numbers in metres, cleaned, on the parent's.
    let local = record_pose(&doc);
    let world = rec.parent_world.mul_transform(local);

    let class = tree_class(engine_class, custom_mesh);
    let mut props: Vec<(String, DmValue)> = Vec::new();
    if has_mesh {
        let part = match get_ci(&doc, "properties").cloned().map(PartProperties::deserialize) {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                problems.push(format!("{}: [properties]: {e}", rec.key));
                PartProperties::default()
            }
            None => PartProperties::default(),
        };
        let bp = base_part_from_record(&part, local.scale, local);
        let mesh_id = (class == "MeshPart").then(|| mesh_id_of(rec.key, mesh.as_deref().unwrap_or_default()));
        props = part_props(&bp, world_cframe(&world), shape, mesh_id);
    }
    if class == "Camera" {
        props.extend(camera_props(&doc, &world));
    }
    props.extend(record_class_props(&class, &doc));

    let mut attributes: Vec<(String, DmValue)> = Vec::new();
    let set_attr = |k: &str, v: &toml::Value, out: &mut Vec<(String, DmValue)>| {
        if let Some(dv) = toml_to_attribute(v).as_ref().and_then(attribute_to_dm) {
            out.retain(|(n, _)| n != k);
            out.push((k.to_string(), dv));
        }
    };
    for (k, v) in record_attribute_values(&doc) {
        if let Some(dv) = attribute_to_dm(&v) {
            attributes.retain(|(n, _)| *n != k);
            attributes.push((k, dv));
        }
    }
    if !has_mesh {
        if let Some(table) = doc.as_table() {
            for (section, value) in table {
                if TYPED_KEYS.contains(&section.as_str()) || class_owned_section(engine_class, section) {
                    continue;
                }
                match value {
                    toml::Value::Table(entries) => {
                        for (k, v) in entries {
                            // Rich schema: `{ type = "...", value = ..., description = "..." }`.
                            let raw = match v {
                                toml::Value::Table(inline) => inline.get("value").unwrap_or(v),
                                _ => v,
                            };
                            set_attr(k, raw, &mut attributes);
                        }
                    }
                    other => set_attr(section, other, &mut attributes),
                }
            }
        }
    }

    let tags = record_tags(&doc);

    let script = get_ci(&doc, "script").and_then(|s| s.as_table()).cloned();

    RecordInstance {
        engine_class,
        class,
        name,
        props,
        attributes,
        tags,
        world,
        has_mesh,
        script,
        problems,
    }
}

/// A class's own properties in the tree, read from its file's class section,
/// for the classes whose settings live there: `Seat`, `VehicleSeat` and
/// `KeyframeSequence`.
/// Studio's loader and a Player's reader both call it, so the two trees start
/// equal; a class added here needs nothing else for parity.
pub fn record_class_props(class: &str, doc: &toml::Value) -> Vec<(String, DmValue)> {
    match class {
        "Seat" | "VehicleSeat" => seat_props(class, doc),
        "KeyframeSequence" => keyframe_sequence_props(doc),
        _ => Vec::new(),
    }
}

/// A KeyframeSequence's own settings, from its `[keyframe_sequence]` section:
/// `Loop`, `Priority` (an `AnimationPriority` item) and `AuthoredHipHeight`, a
/// length in the file's `[metadata] unit`, read in metres. A key the file
/// lacks keeps the class default.
fn keyframe_sequence_props(doc: &toml::Value) -> Vec<(String, DmValue)> {
    let mut props = Vec::new();
    let Some(sec) = get_ci(doc, "keyframe_sequence") else { return props };
    if let Some(v) = get_ci(sec, "loop").and_then(|v| v.as_bool()) {
        props.push(("Loop".to_string(), DmValue::Bool(v)));
    }
    if let Some(name) = get_ci(sec, "priority").and_then(|v| v.as_str()) {
        props.push(("Priority".to_string(), DmValue::Enum(EnumItem::new("AnimationPriority", name))));
    }
    let height = get_ci(sec, "authored_hip_height").and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)));
    if let Some(h) = height {
        props.push(("AuthoredHipHeight".to_string(), DmValue::Number(file_length_to_metres(doc, h))));
    }
    props
}

/// A `Seat`'s or `VehicleSeat`'s own settings in the tree, from its file's
/// `[seat]` or `[vehicle]` section: `Disabled`, and a VehicleSeat's `MaxSpeed`,
/// `Torque`, `TurnSpeed` and `HeadsUpDisplay`. `max_speed` is in the file's
/// `[metadata] unit` per second, as its positions are in that unit, and reads
/// as metres per second. `Occupant`, `Throttle` and `Steer` are Play state:
/// they start at their class defaults whatever the file holds.
fn seat_props(class: &str, doc: &toml::Value) -> Vec<(String, DmValue)> {
    let mut props = Vec::new();
    let section = match class {
        "Seat" => "seat",
        "VehicleSeat" => "vehicle",
        _ => return props,
    };
    let Some(sec) = get_ci(doc, section) else { return props };
    let number = |key: &str| get_ci(sec, key).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)));
    if let Some(v) = get_ci(sec, "disabled").and_then(|v| v.as_bool()) {
        props.push(("Disabled".to_string(), DmValue::Bool(v)));
    }
    if class == "VehicleSeat" {
        if let Some(v) = number("max_speed") {
            props.push(("MaxSpeed".to_string(), DmValue::Number(file_length_to_metres(doc, v))));
        }
        for (key, name) in [("torque", "Torque"), ("turn_speed", "TurnSpeed")] {
            if let Some(v) = number(key) {
                props.push((name.to_string(), DmValue::Number(v)));
            }
        }
        if let Some(v) = get_ci(sec, "heads_up_display").and_then(|v| v.as_bool()) {
            props.push(("HeadsUpDisplay".to_string(), DmValue::Bool(v)));
        }
    }
    props
}

/// A length (or a length a second) in the file's `[metadata] unit`, in
/// metres, by the rule [`record_pose`] reads positions with.
fn file_length_to_metres(doc: &toml::Value, v: f64) -> f64 {
    #[cfg(feature = "units_v1")]
    {
        let unit = get_ci(doc, "metadata").and_then(|m| get_ci(m, "unit")).and_then(|u| u.as_str());
        if let Some(u) = unit.and_then(crate::units::Unit::from_symbol) {
            return crate::units::authored_to_engine_f32(v as f32, u) as f64;
        }
    }
    #[cfg(not(feature = "units_v1"))]
    let _ = doc;
    v
}

/// What [`record_class_props`] read for a loaded instance, kept on its entity
/// for Studio's seed, so Studio's tree and a Player's hold the same values.
#[derive(Component, Debug, Clone, Default)]
pub struct RecordClassProps(pub Vec<(String, DmValue)>);

/// Whether [`record_props`] reads every property Studio's seed gives a class.
/// The rest get their class defaults and their attributes.
pub fn reads_own_properties(class: &str) -> bool {
    !matches!(
        class,
        "Humanoid"
            | "ScreenGui"
            | "Frame"
            | "ScrollingFrame"
            | "BillboardGui"
            | "SurfaceGui"
            | "TextLabel"
            | "TextButton"
            | "TextBox"
            | "ImageLabel"
            | "ImageButton"
            | "ViewportFrame"
            | "PointLight"
            | "SpotLight"
            | "SurfaceLight"
            | "Sound"
            | "ParticleEmitter"
            | "Beam"
            | "Decal"
            | "Attachment"
            | "SpecialMesh"
    )
}

/// A custom mesh's `MeshId`: its path relative to the Space, as Studio's
/// seed writes it. The file names the mesh relative to its own folder.
pub fn mesh_id_of(record_key: &str, mesh: &str) -> String {
    let mesh = mesh.replace('\\', "/");
    let base = match record_key.rfind('/') {
        Some(i) => &record_key[..i],
        None => "",
    };
    let mut parts: Vec<&str> = if mesh.starts_with('/') { Vec::new() } else { base.split('/').filter(|s| !s.is_empty()).collect() };
    for seg in mesh.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// A `Camera` file's view: `[camera] camera_type`, `projection`,
/// `orthographic_size` and `field_of_view`, and its pose.
pub fn camera_props(doc: &toml::Value, world: &GlobalTransform) -> Vec<(String, DmValue)> {
    let mut props = vec![("CFrame".to_string(), DmValue::CFrame(world_cframe(world)))];
    let Some(cam) = get_ci(doc, "camera") else { return props };
    if let Some(t) = get_ci(cam, "camera_type").and_then(|v| v.as_str()) {
        props.push(("CameraType".into(), DmValue::Enum(EnumItem::parse(t, "CameraType"))));
    }
    if let Some(p) = get_ci(cam, "projection").and_then(|v| v.as_str()) {
        props.push(("Projection".into(), DmValue::Enum(EnumItem::parse(p, "CameraProjection"))));
    }
    let number = |k: &str| get_ci(cam, k).and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)));
    if let Some(s) = number("orthographic_size") {
        props.push(("OrthographicSize".into(), DmValue::Number(s)));
    }
    if let Some(f) = number("field_of_view") {
        props.push(("FieldOfView".into(), DmValue::Number(f)));
    }
    props
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str, key: &str) -> RecordInstance {
        record_props(InstanceRecord {
            doc: text.parse::<toml::Value>().expect("test file parses"),
            key,
            fallback_name: "Fallback",
            form: if key.ends_with("/_instance.toml") { RecordForm::Folder } else { RecordForm::Flat },
            parent_world: GlobalTransform::IDENTITY,
        })
    }

    fn prop<'a>(r: &'a RecordInstance, name: &str) -> &'a DmValue {
        &r.props.iter().find(|(k, _)| k == name).unwrap_or_else(|| panic!("no {name} in {:?}", r.props)).1
    }

    #[test]
    fn a_seat_reads_its_own_settings() {
        let r = read(
            "[metadata]\nclass_name = \"VehicleSeat\"\nname = \"DriveSeat\"\n\n\
             [vehicle]\ndisabled = true\nmax_speed = 30.0\ntorque = 12\nthrottle = 1\n",
            "Workspace/Car/DriveSeat/_instance.toml",
        );
        assert_eq!(r.class, "VehicleSeat");
        assert_eq!(prop(&r, "Disabled"), &DmValue::Bool(true));
        assert_eq!(prop(&r, "MaxSpeed"), &DmValue::Number(30.0));
        assert_eq!(prop(&r, "Torque"), &DmValue::Number(12.0));
        assert!(r.props.iter().all(|(k, _)| k != "Throttle"), "Throttle is Play state");
        // A file with no max_speed of its own gives no MaxSpeed (the class
        // template carries none, since a healed number would be read in each
        // file's unit), and the tree keeps the class default.
        let bare = read(
            "[metadata]\nclass_name = \"VehicleSeat\"\nname = \"Seat2\"\nunit = \"ft\"\n\n[vehicle]\ndisabled = false\n",
            "Workspace/Car/Seat2/_instance.toml",
        );
        assert!(bare.props.iter().all(|(k, _)| k != "MaxSpeed"), "{:?}", bare.props);
        // An imported file is in feet: its MaxSpeed is feet a second.
        let feet = record_class_props(
            "VehicleSeat",
            &"[metadata]\nunit = \"ft\"\n\n[vehicle]\nmax_speed = 25.0\n".parse::<toml::Value>().unwrap(),
        );
        let DmValue::Number(v) = feet[0].1 else { panic!("{feet:?}") };
        #[cfg(feature = "units_v1")]
        assert!((v - 7.62).abs() < 1e-4, "{v}");
        #[cfg(not(feature = "units_v1"))]
        assert_eq!(v, 25.0);
        let plain = record_class_props("Seat", &"[seat]\ndisabled = true\n".parse::<toml::Value>().unwrap());
        assert_eq!(plain, vec![("Disabled".to_string(), DmValue::Bool(true))]);
        assert!(record_class_props("Part", &"[seat]\ndisabled = true\n".parse::<toml::Value>().unwrap()).is_empty());
    }

    #[test]
    fn a_part_reads_as_studio_seeds_it() {
        let r = read(
            "[metadata]\nclass_name = \"Part\"\nname = \"Paddle\"\n\n\
             [transform]\nposition = [1.0, 2.0, 3.0]\nscale = [0.5, 2.0, 6.0]\n\n\
             [properties]\ncolor = [255, 0, 0]\nanchored = true\nmaterial = \"Neon\"\n",
            "Workspace/Paddle/_instance.toml",
        );
        assert_eq!((r.class.as_str(), r.name.as_str()), ("Part", "Paddle"));
        let order: Vec<&str> = r.props.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            order,
            [
                "CFrame", "Size", "Color", "Material", "Transparency", "Reflectance", "Anchored", "CanCollide",
                "CanTouch", "CastShadow", "Locked", "Mass", "CollisionGroup", "Shape"
            ],
            "the seed's order, with Shape for a primitive"
        );
        match prop(&r, "CFrame") {
            DmValue::CFrame(cf) => assert_eq!(cf.position.to_vec3(), Vec3::new(1.0, 2.0, 3.0)),
            other => panic!("{other:?}"),
        }
        assert_eq!(prop(&r, "Size"), &DmValue::Vector3(Vector3::new(0.5, 2.0, 6.0)));
        assert_eq!(prop(&r, "Color"), &DmValue::Color3(Color3::new(1.0, 0.0, 0.0)));
        assert_eq!(prop(&r, "Material"), &DmValue::Enum(EnumItem::new("Material", "Neon")));
        assert_eq!(prop(&r, "Anchored"), &DmValue::Bool(true));
        assert_eq!(prop(&r, "Shape"), &DmValue::Enum(EnumItem::new("PartType", "Block")), "no [asset] is a block");
        assert_eq!(prop(&r, "Mass"), &DmValue::Number(BasePart::default().mass as f64));
    }

    #[test]
    fn defaults_are_the_loaders_not_the_movement_readers() {
        let r = read("[metadata]\nclass_name = \"Part\"\n", "Workspace/P.instance.toml");
        assert_eq!(prop(&r, "Anchored"), &DmValue::Bool(false), "the loader's default is unanchored");
        assert_eq!(prop(&r, "CanCollide"), &DmValue::Bool(true));
        let grey = Color3::new(163.0 / 255.0, 162.0 / 255.0, 165.0 / 255.0);
        match prop(&r, "Color") {
            DmValue::Color3(c) => assert!((c.r - grey.r).abs() < 1e-6 && (c.b - grey.b).abs() < 1e-6),
            other => panic!("{other:?}"),
        }
        assert_eq!(r.name, "Fallback", "no name in the file takes the folder's");
    }

    #[test]
    fn a_custom_mesh_part_is_a_mesh_part_with_its_space_relative_mesh() {
        let r = read(
            "[metadata]\nclass_name = \"Part\"\n\n[asset]\nmesh = \"../meshes/Car.glb\"\n",
            "Workspace/Garage/Car/_instance.toml",
        );
        assert_eq!(r.class, "MeshPart");
        assert_eq!(prop(&r, "MeshId"), &DmValue::String("Workspace/Garage/meshes/Car.glb".into()));
        assert_eq!(prop(&r, "Shape"), &DmValue::Enum(EnumItem::new("PartType", "Block")));
        let ball = read("[metadata]\nclass_name = \"Part\"\n\n[asset]\nmesh = \"parts/ball.glb\"\n", "Workspace/B/_instance.toml");
        assert_eq!(ball.class, "Part");
        assert_eq!(prop(&ball, "Shape"), &DmValue::Enum(EnumItem::new("PartType", "Ball")));
        assert!(ball.props.iter().all(|(k, _)| k != "MeshId"), "only a MeshPart has a MeshId");
    }

    #[test]
    fn children_compose_onto_the_parent_scale_included() {
        let parent = GlobalTransform::from(Transform {
            translation: Vec3::new(10.0, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(2.0),
        });
        let r = record_props(InstanceRecord {
            doc: "[metadata]\nclass_name = \"Part\"\n\n[transform]\nposition = [1.0, 0.0, 0.0]\nscale = [4.0, 1.0, 1.0]\n"
                .parse()
                .unwrap(),
            key: "Workspace/M/P/_instance.toml",
            fallback_name: "P",
            form: RecordForm::Folder,
            parent_world: parent,
        });
        match prop(&r, "CFrame") {
            DmValue::CFrame(cf) => assert!((cf.position.to_vec3() - Vec3::new(12.0, 0.0, 0.0)).length() < 1e-5, "as Bevy propagates"),
            other => panic!("{other:?}"),
        }
        assert_eq!(prop(&r, "Size"), &DmValue::Vector3(Vector3::new(4.0, 1.0, 1.0)), "Size is the part's own");
    }

    #[cfg(feature = "units_v1")]
    #[test]
    fn a_files_unit_converts_to_metres() {
        let r = read(
            "[metadata]\nclass_name = \"Part\"\nunit = \"ft\"\n\n[transform]\nposition = [10.0, 0.0, 0.0]\nscale = [1.0, 1.0, 1.0]\n",
            "Workspace/P/_instance.toml",
        );
        match prop(&r, "CFrame") {
            DmValue::CFrame(cf) => assert!((cf.position.x - 3.048).abs() < 1e-5),
            other => panic!("{other:?}"),
        }
        match prop(&r, "Size") {
            DmValue::Vector3(v) => assert!((v.x - 0.3048).abs() < 1e-5),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn classes_resolve_as_the_loader_resolves_them() {
        assert_eq!(class_from_toml("Script"), ClassName::SoulScript, "the legacy Script is a SoulScript folder");
        assert_eq!(class_from_toml("NoSuchClass"), ClassName::Folder);
        assert_eq!(class_from_toml("MeshPart"), ClassName::Part);
        assert_eq!(tree_class(ClassName::LuauLocalScript, false), "LocalScript");
        assert_eq!(luau_script_class("Door.server.luau", "Workspace"), "Script");
        assert_eq!(luau_script_class("hud.client.luau", "StarterGui"), "LocalScript");
        assert_eq!(luau_script_class("util.luau", "ReplicatedStorage"), "ModuleScript");
        assert_eq!(luau_script_class("script.luau", "Workspace"), "Script");
        assert_eq!(strip_script_suffix("Door.server"), "Door");
    }

    #[test]
    fn attributes_and_extra_sections_read_as_the_loader_reads_them() {
        let r = read(
            "[metadata]\nclass_name = \"Folder\"\n\n\
             [attributes]\nscore = 3\nlabel = \"hi\"\ntint = { Color3 = [1.0, 0.5, 0.0] }\n\n\
             [gameplay]\nspeed = { type = \"number\", value = 12.5 }\nteam = \"red\"\n\n\
             [gaussian_splats]\npath = \"x.ply\"\n",
            "Workspace/Config.instance.toml",
        );
        let attr = |k: &str| r.attributes.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(attr("score"), Some(DmValue::Number(3.0)));
        assert_eq!(attr("label"), Some(DmValue::String("hi".into())));
        assert_eq!(attr("tint"), Some(DmValue::Color3(Color3::new(1.0, 0.5, 0.0))));
        assert_eq!(attr("speed"), Some(DmValue::Number(12.5)), "a rich-schema entry reads its value");
        assert_eq!(attr("team"), Some(DmValue::String("red".into())));
        assert_eq!(attr("path"), None, "a class's own section is not attributes");
    }

    #[test]
    fn an_attachment_is_built_from_its_file_and_read_as_its_part_local_cframe() {
        let doc: toml::Value = "[metadata]\nclass_name = \"Attachment\"\n\n\
                                [transform]\nposition = [0.5, 1.0, 0.0]\nrotation = [0.0, 0.7071068, 0.0, 0.7071068]\n\n\
                                [attachment]\nvisible = true\naxis = [0.0, 0.0, 1.0]\n"
            .parse()
            .unwrap();
        let a = record_attachment(&doc, "Grip");
        assert_eq!(a.name, "Grip");
        assert_eq!(a.cframe.translation, Vec3::new(0.5, 1.0, 0.0));
        assert_eq!(a.position, a.cframe.translation);
        assert!((a.orientation.y - 90.0).abs() < 1e-3, "{:?}", a.orientation);
        assert!(a.visible);
        assert_eq!(a.axis, Vec3::Z);
        assert_eq!(a.secondary_axis, Vec3::Y, "a missing axis keeps the component default");
        let props = component_props(&a);
        let get = |k: &str| props.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert!(matches!(get("CFrame"), Some(DmValue::CFrame(cf)) if (cf.position.to_vec3() - a.cframe.translation).length() < 1e-5));
        assert!(matches!(get("Position"), Some(DmValue::Vector3(_))));
    }

    #[test]
    fn a_keyframe_sequence_reads_its_own_settings() {
        let doc: toml::Value = "[metadata]\nclass_name = \"KeyframeSequence\"\nunit = \"ft\"\n\n\
                                [keyframe_sequence]\nloop = false\npriority = \"Movement\"\nauthored_hip_height = 2.0\n"
            .parse()
            .unwrap();
        let props = record_class_props("KeyframeSequence", &doc);
        let get = |k: &str| props.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(get("Loop"), Some(DmValue::Bool(false)));
        assert_eq!(get("Priority"), Some(DmValue::Enum(EnumItem::new("AnimationPriority", "Movement"))));
        let Some(DmValue::Number(h)) = get("AuthoredHipHeight") else { panic!("{props:?}") };
        #[cfg(feature = "units_v1")]
        assert!((h - 0.6096).abs() < 1e-4, "{h}");
        #[cfg(not(feature = "units_v1"))]
        assert_eq!(h, 2.0);
        let bare: toml::Value = "[keyframe_sequence]\n".parse().unwrap();
        assert!(record_class_props("KeyframeSequence", &bare).is_empty(), "a missing key keeps the class default");
    }

    #[test]
    fn a_class_section_never_becomes_attributes() {
        assert!(class_owned_section(ClassName::PointLight, "light"));
        assert!(!class_owned_section(ClassName::Part, "light"), "only a light's [light] is its own");
        assert!(class_owned_section(ClassName::KeyframeSequence, "keyframe_sequence"));
        assert!(class_owned_section(ClassName::Part, "gaussian_splats"));
        let r = read(
            "[metadata]\nclass_name = \"PointLight\"\nname = \"Lamp\"\n\n[light]\nbrightness = 2.0\n\n[gameplay]\nlit = true\n",
            "Workspace/Lamp.instance.toml",
        );
        assert!(r.attributes.iter().all(|(k, _)| k != "brightness"), "{:?}", r.attributes);
        assert!(r.attributes.iter().any(|(k, _)| k == "lit"), "{:?}", r.attributes);
    }

    #[test]
    fn a_part_takes_attributes_only_from_its_attributes_table() {
        let r = read(
            "[metadata]\nclass_name = \"Part\"\n\n[attributes]\nhp = 5\n\n[gameplay]\nspeed = 3\n",
            "Workspace/P/_instance.toml",
        );
        assert_eq!(r.attributes, vec![("hp".to_string(), DmValue::Number(5.0))]);
    }

    #[test]
    fn a_camera_reads_its_view() {
        let r = read(
            "[metadata]\nclass_name = \"Camera\"\n\n[transform]\nposition = [0.0, 30.0, 0.0]\n\n\
             [camera]\ncamera_type = \"Scriptable\"\nprojection = \"Orthographic\"\northographic_size = 25.0\nfield_of_view = 60\n",
            "Workspace/Camera/_instance.toml",
        );
        assert_eq!(r.class, "Camera");
        assert_eq!(prop(&r, "CameraType").as_enum_name(), Some("Scriptable"));
        assert_eq!(prop(&r, "Projection").as_enum_name(), Some("Orthographic"));
        assert_eq!(prop(&r, "OrthographicSize"), &DmValue::Number(25.0));
        assert_eq!(prop(&r, "FieldOfView"), &DmValue::Number(60.0));
    }

    #[test]
    fn toml_attributes_cover_the_importers_tagged_values() {
        let cf: toml::Value = "v = { CFrame = [1, 2, 3, 0, 0, 0, 1] }".parse::<toml::Value>().unwrap()["v"].clone();
        assert!(matches!(toml_to_attribute(&cf), Some(AttributeValue::CFrame(t)) if t.translation == Vec3::new(1.0, 2.0, 3.0)));
        let bc: toml::Value = "v = { BrickColor = 21 }".parse::<toml::Value>().unwrap()["v"].clone();
        assert!(matches!(toml_to_attribute(&bc), Some(AttributeValue::BrickColor(21))));
        assert!(toml_to_attribute(&toml::Value::Array(vec![1.into()])).is_none());
    }

    fn every_kind() -> Vec<AttributeValue> {
        vec![
            AttributeValue::String("a \"quoted\" line".into()),
            AttributeValue::Number(5.0),
            AttributeValue::Number(-0.1),
            AttributeValue::Int(7),
            AttributeValue::Bool(true),
            AttributeValue::Vector2(Vec2::new(1.5, -2.0)),
            AttributeValue::Vector3(Vec3::new(0.1, 2.0, 3.0)),
            AttributeValue::Color(Color::srgba(0.1, 0.2, 0.3, 0.4)),
            AttributeValue::Color3(Color::srgb(0.5, 0.25, 1.0)),
            AttributeValue::BrickColor(21),
            AttributeValue::CFrame(Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_y(0.5))),
            AttributeValue::UDim { scale: 0.5, offset: 12.0 },
            AttributeValue::UDim2 { x_scale: 0.5, x_offset: 10.0, y_scale: 1.0, y_offset: -4.0 },
            AttributeValue::Rect { min: Vec2::new(0.0, 1.0), max: Vec2::new(2.0, 3.0) },
            AttributeValue::NumberRange { min: 1.0, max: 2.5 },
            AttributeValue::NumberSequence(vec![
                NumberSequenceKeypoint { time: 0.0, value: 1.0, envelope: 0.0 },
                NumberSequenceKeypoint { time: 1.0, value: 0.3, envelope: 0.1 },
            ]),
            AttributeValue::ColorSequence(vec![
                ColorSequenceKeypoint { time: 0.0, color: Color::srgb(1.0, 0.0, 0.0) },
                ColorSequenceKeypoint { time: 1.0, color: Color::srgb(0.1, 0.2, 0.9) },
            ]),
            AttributeValue::Font {
                family: "rbxasset://fonts/families/SourceSansPro.json".into(),
                weight: 700,
                style: "Italic".into(),
            },
            AttributeValue::EnumItem { enum_type: "Material".into(), name: "Plastic".into(), value: Some(256) },
            AttributeValue::EnumItem { enum_type: "Material".into(), name: "Neon".into(), value: None },
        ]
    }

    #[test]
    fn every_attribute_kind_reads_back_from_the_text_it_writes() {
        let kinds = every_kind();
        let table: toml::Table = kinds
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let written = attribute_to_toml(v).unwrap_or_else(|| panic!("{} wrote nothing", v.type_name()));
                (format!("k{i}"), written)
            })
            .collect();
        let text = toml::to_string(&table).unwrap();
        let back: toml::Table = toml::from_str(&text).unwrap();
        for (i, v) in kinds.iter().enumerate() {
            let read = toml_to_attribute(&back[&format!("k{i}")]);
            assert_eq!(read.as_ref(), Some(v), "{} read back as {read:?} from:\n{text}", v.type_name());
        }
        assert!(attribute_to_toml(&AttributeValue::Object(None)).is_none());
        assert!(attribute_to_toml(&AttributeValue::EntityRef(None)).is_none());
    }

    #[test]
    fn tagged_kinds_never_read_as_bare_arrays() {
        let v = |s: &str| s.parse::<toml::Value>().unwrap()["v"].clone();
        assert!(matches!(toml_to_attribute(&v("v = { UDim2 = [0.5, 10, 1, -4] }")), Some(AttributeValue::UDim2 { .. })));
        assert!(matches!(toml_to_attribute(&v("v = { NumberRange = [1, 2] }")), Some(AttributeValue::NumberRange { .. })));
        assert!(matches!(toml_to_attribute(&v("v = [0.5, 10, 1, -4]")), Some(AttributeValue::Color(_))));
        assert!(toml_to_attribute(&v("v = { Bytes = \"00ff\" }")).is_none());
        assert!(toml_to_attribute(&v("v = { UDim2 = [1, 2, 3] }")).is_none());
        assert!(toml_to_attribute(&v("v = [1, \"x\"]")).is_none());
        assert!(toml_to_attribute(&v("v = { EnumItem = { type = \"Material\", value = 256 } }")).is_none());
    }

    #[test]
    fn a_write_back_keeps_what_it_cannot_read_and_owns_what_it_can() {
        let disk: toml::Table = toml::from_str(
            "blob = { Bytes = \"00ff\" }\ngone = 5\nrenamed = \"old\"\nspot = { CFrame = [1, 2, 3, 0, 0, 0, 1] }\n",
        )
        .unwrap();
        let fresh = vec![
            ("renamed".to_string(), attribute_to_toml(&AttributeValue::String("new".into())).unwrap()),
            ("spot".to_string(), attribute_to_toml(&AttributeValue::CFrame(Transform::from_xyz(4.0, 5.0, 6.0))).unwrap()),
        ];
        let merged = merge_attribute_table(Some(&disk), fresh).unwrap();
        assert_eq!(merged.get("blob"), disk.get("blob"), "an entry no reader loads stays as written");
        assert!(merged.get("gone").is_none(), "a readable entry the entity no longer has was removed");
        assert_eq!(merged["renamed"].as_str(), Some("new"));
        assert!(matches!(
            toml_to_attribute(&merged["spot"]),
            Some(AttributeValue::CFrame(t)) if t.translation == Vec3::new(4.0, 5.0, 6.0)
        ));
        let readable_only: toml::Table = toml::from_str("n = 1").unwrap();
        assert!(merge_attribute_table(Some(&readable_only), Vec::<(String, toml::Value)>::new()).is_none());
    }

    #[test]
    fn single_precision_fields_write_their_shortest_decimal() {
        let written = attribute_to_toml(&AttributeValue::Color3(Color::srgb(0.1, 0.2, 0.3))).unwrap();
        let channels: Vec<f64> = written["Color3"].as_array().unwrap().iter().map(|c| c.as_float().unwrap()).collect();
        assert_eq!(channels, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn sequences_udims_and_enum_items_reach_scripts() {
        let seq = AttributeValue::NumberSequence(vec![NumberSequenceKeypoint { time: 0.0, value: 2.0, envelope: 0.5 }]);
        assert_eq!(attribute_to_dm(&seq), Some(DmValue::NumberSequence(vec![(0.0, 2.0)])));
        let item = AttributeValue::EnumItem { enum_type: "Material".into(), name: "Neon".into(), value: Some(288) };
        assert_eq!(attribute_to_dm(&item), Some(DmValue::Enum(EnumItem::new("Material", "Neon"))));
        let udim = AttributeValue::UDim { scale: 0.5, offset: 4.0 };
        assert_eq!(attribute_to_dm(&udim), Some(DmValue::UDim(UDim::new(0.5, 4.0))));
    }

    #[test]
    fn folder_names_capitalise_their_first_letter() {
        assert_eq!(folder_display_name("court"), "Court");
        assert_eq!(folder_display_name("Court"), "Court");
        assert_eq!(folder_display_name("1st"), "1st");
        assert_eq!(folder_display_name(""), "");
    }

    #[test]
    fn a_files_name_attributes_and_tags_read_through_the_shared_helpers() {
        let doc: toml::Value = toml::from_str(
            "tags = []\n\n[metadata]\nclass_name = \"Model\"\nname = \"Court\"\ntags = [\"stale\"]\n\n\
             [attributes]\nscore = 3\nblob = { Bytes = \"00ff\" }\ntint = { Color3 = [1.0, 0.5, 0.0] }\n",
        )
        .unwrap();
        assert_eq!(record_name(&doc), Some("Court"));
        assert!(record_tags(&doc).is_empty(), "a present but empty root tags means none");
        let names: Vec<String> = record_attribute_values(&doc).into_iter().map(|(k, _)| k).collect();
        assert_eq!(names, vec!["score".to_string(), "tint".to_string()], "an entry no reader uses is left out");

        let older: toml::Value = toml::from_str("[Metadata]\nname = \"\"\ntags = [\"a\", \"b\"]\n").unwrap();
        assert_eq!(record_name(&older), None, "an empty name is no name");
        assert_eq!(record_tags(&older), vec!["a".to_string(), "b".to_string()], "no root tags: the metadata copy");
    }

    #[test]
    fn a_models_pivot_reads_as_written_and_is_carried_only_when_set() {
        let doc: toml::Value = toml::from_str(
            "[metadata]\nclass_name = \"Model\"\nunit = \"ft\"\n\n[model]\n\
             world_pivot = { position = [3.048, 0.0, -1.0], rotation = [0.0, 0.0, 0.0, 1.0], scale = [1.0, 1.0, 1.0] }\n",
        )
        .unwrap();
        let pivot = record_model_pivot(&doc).expect("a stored pivot");
        assert_eq!(pivot.translation, Vec3::new(3.048, 0.0, -1.0), "metres as written: the unit is for [transform]");
        assert_eq!(model_pivot_prop(pivot).map(|(k, _)| k), Some("WorldPivot".to_string()));
        assert!(model_pivot_prop(Transform::IDENTITY).is_none(), "identity is no pivot of its own");
        let none: toml::Value = toml::from_str("[model]\nscale = 1.0\n").unwrap();
        assert!(record_model_pivot(&none).is_none());
    }

    #[test]
    fn the_general_branch_poses_and_disables_by_class() {
        let doc: toml::Value = toml::from_str(
            "[metadata]\nclass_name = \"LuauScript\"\n\n[transform]\nposition = [1.0, 2.0, 3.0]\nscale = [2.0, 2.0, 2.0]\n\n\
             [Script]\nEnabled = false\n",
        )
        .unwrap();
        let pose = general_branch_pose(ClassName::Folder, &doc);
        assert_eq!(pose.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(pose.scale, Vec3::ONE, "a folder's scale never scales what it holds");
        assert_eq!(general_branch_pose(ClassName::Attachment, &doc), Transform::IDENTITY, "part-local, not yet composed");
        assert_eq!(general_branch_pose(ClassName::Seat, &doc), Transform::IDENTITY, "a part class keeps its children's world poses");
        assert!(record_script_disabled(&doc));
        assert!(script_starts_disabled(ClassName::LuauScript, &doc));
        assert!(!script_starts_disabled(ClassName::LuauModuleScript, &doc), "a ModuleScript has no Disabled");
        assert!(has_model_pivot(ClassName::Actor) && !has_model_pivot(ClassName::Folder));
        assert!(loads_as_part(ClassName::VehicleSeat) && loads_as_part(ClassName::UnionOperation));
        assert!(!loads_as_part(ClassName::Model) && !loads_as_part(ClassName::Attachment));
    }

    #[test]
    fn a_child_rides_its_parents_pose_never_its_size() {
        let parent = Isometry3d::new(Vec3::new(0.0, 0.0, 10.0), Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let local = Transform::from_xyz(1.0, 1.0, 0.0).with_scale(Vec3::new(0.2, 2.0, 0.2));
        let world = compose_pose(parent, &local);
        assert!((world.translation - Vec3::new(0.0, 1.0, 9.0)).length() < 1e-5, "{}", world.translation);
        assert_eq!(world.scale, local.scale, "the child keeps its own size");
        let back = pose_relative_to(parent, &world);
        assert!((back.translation - local.translation).length() < 1e-5);
        assert!(crate::animation::clip::rotation_gap(back.rotation, local.rotation) < 1e-5, "{}", back.rotation);
    }

    #[test]
    fn a_space_names_its_transform_rule() {
        let named: toml::Value = toml::from_str("[space]\ntransform_rule = \"parent_pose\"\n").unwrap();
        assert_eq!(TransformRule::of_space(Some(&named)), TransformRule::ParentPose);
        assert_eq!(TransformRule::of_space(None), TransformRule::Legacy, "a Space from before the rule");
        let sized = GlobalTransform::from(Transform::from_xyz(1.0, 2.0, 3.0).with_scale(Vec3::splat(4.0)));
        let (scale, _, at) = TransformRule::ParentPose.basis(sized).to_scale_rotation_translation();
        assert!((scale - Vec3::ONE).length() < 1e-6 && at == Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(TransformRule::Legacy.basis(sized), sized);
        let seat: toml::Value = toml::from_str("[transform]\nposition = [1.0, 2.0, 3.0]\n").unwrap();
        assert_eq!(TransformRule::ParentPose.folder_pose(ClassName::Seat, &seat).translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(TransformRule::Legacy.folder_pose(ClassName::Seat, &seat), Transform::IDENTITY);
    }

    #[test]
    fn mesh_ids_resolve_against_the_files_folder() {
        assert_eq!(mesh_id_of("Workspace/Car/_instance.toml", "meshes/Car.glb"), "Workspace/Car/meshes/Car.glb");
        assert_eq!(mesh_id_of("Workspace/Car/_instance.toml", "../../assets/x.glb"), "assets/x.glb");
        assert_eq!(mesh_id_of("Workspace/Car.instance.toml", "Car.glb"), "Workspace/Car.glb");
    }

    #[test]
    fn tagged_kinds_read_in_any_case() {
        let read = |text: &str| toml_to_attribute(&format!("v = {text}").parse::<toml::Value>().unwrap()["v"]);
        let tint = Some(AttributeValue::Color3(Color::srgb(1.0, 0.5, 0.0)));
        assert_eq!(read("{ Color3 = [1.0, 0.5, 0.0] }"), tint);
        assert_eq!(read("{ color3 = [1.0, 0.5, 0.0] }"), tint);
        assert_eq!(read("{ number_range = [0.0, 2.0] }"), Some(AttributeValue::NumberRange { min: 0.0, max: 2.0 }));
        assert!(matches!(read("{ c_frame = [1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0] }"), Some(AttributeValue::CFrame(_))));
        assert_eq!(read("{ Colour3 = [1.0, 0.5, 0.0] }"), None, "an unknown kind reads as nothing");
    }
}

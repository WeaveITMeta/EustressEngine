//! Roblox `rbx_types::Variant` properties → Eustress
//! `InstanceOverrides` + TOML extras.
//!
//! Spec ref: `docs/architecture/ROBLOX_IMPORT_SPEC.md` §10.
//!
//! The output is a [`PropertyBag`]:
//! - **`overrides`** — the well-known slots that flow into
//!   [`eustress_common::instance_create::InstanceOverrides`] (position,
//!   rotation, scale, color, material, anchored, can_collide, asset
//!   refs). These are written directly to the root `_instance.toml` by
//!   the canonical pipeline.
//! - **`properties_extras`** — `toml::Value` entries that get written
//!   under `[properties.extras]` in the TOML, opaque round-trip storage
//!   for properties without a first-class slot.
//! - **`physics_extras`** — `toml::Value` entries that get written under
//!   `[properties.physics]` (PhysicalProperties decomposition).
//! - **`tags`** — values for `[metadata.tags]`.
//! - **`attributes`** — values for `[properties.attributes]`.
//! - **`refs`** — Roblox `Ref` properties keyed by Roblox property name;
//!   resolved by the materializer's second pass via the
//!   referent → uuid map.
//! - **`asset_refs`** — `rbxassetid://` / `rbxasset://` / http
//!   references keyed by Roblox property name; routed through the asset
//!   resolver.
//! - **`script_source`** — for Script/LocalScript/ModuleScript only:
//!   the source body lifted out of Roblox's `Source` property.
//!
//! ## Variant coverage (41 variants per `rbx_types::Variant`)
//!
//! Every `Variant` arm reachable by `Variant::ty()` has a branch in
//! [`apply_variant`]. The full enumeration:
//!
//! - `Bool`, `String`, `Int32`, `Int64`, `Float32`, `Float64`
//! - `Vector2`, `Vector2int16`, `Vector3`, `Vector3int16`
//! - `CFrame`, `OptionalCFrame`
//! - `Color3`, `Color3uint8`, `BrickColor`
//! - `UDim`, `UDim2`, `Rect`
//! - `Enum`
//! - `Content`
//! - `BinaryString`, `SharedString`
//! - `NumberSequence`, `ColorSequence`, `NumberRange`
//! - `PhysicalProperties`
//! - `Ray`, `Region3`, `Region3int16`
//! - `Faces`, `Axes`
//! - `MaterialColors`
//! - `Font`
//! - `Tags`, `Attributes`
//! - `Ref`
//! - `UniqueId`
//! - `SecurityCapabilities`
//!
//! The `Variant` enum is `#[non_exhaustive]`. We add a catch-all that
//! falls through to the opaque-string-encoding path so a future Roblox
//! type lands in extras rather than crashing the import.

use std::collections::HashMap;

use eustress_common::classes::ClassName;
use eustress_common::instance_create::InstanceOverrides;
use rbx_dom_weak::types::{
    Axes, BinaryString, BrickColor, CFrame, Color3, Color3uint8, ColorSequence, Content, Enum,
    Faces, Font, MaterialColors, NumberRange, NumberSequence, PhysicalProperties, Ray, Rect, Ref,
    Region3, Region3int16, SharedString, Tags, UDim, UDim2, UniqueId, Variant, Vector2,
    Vector2int16, Vector3, Vector3int16,
};

// ---------------------------------------------------------------------------
// PropertyBag — output of the per-instance property mapping pass.
// ---------------------------------------------------------------------------

/// A mapped property set, ready to feed `create_instance` (well-known
/// slots) and to emit into the `_instance.toml`'s
/// `[properties.extras]` / `[properties.physics]` /
/// `[metadata.tags]` / `[properties.attributes]` /
/// `[references]` blocks.
///
/// Build with [`map_properties`]; consume via [`PropertyBag::into_overrides`]
/// for the canonical-create call and the various accessor methods for the
/// TOML post-write step.
#[derive(Debug, Default, Clone)]
pub struct PropertyBag {
    /// Well-known slots — position, rotation, scale, color, material,
    /// anchored, can_collide, asset refs. These flow through the
    /// canonical [`eustress_common::instance_create::create_instance`]
    /// pipeline.
    pub overrides: InstanceOverrides,

    /// Properties without a first-class slot. Written under
    /// `[properties.extras]` in the TOML.
    pub properties_extras: HashMap<String, toml::Value>,

    /// `PhysicalProperties` decomposition. Written under
    /// `[properties.physics]`.
    pub physics_extras: HashMap<String, toml::Value>,

    /// CollectionService tag values. Written under
    /// `[metadata.tags]` as an array of strings.
    pub tags: Vec<String>,

    /// Roblox `Attributes` payload. Written under
    /// `[properties.attributes]`.
    pub attributes: HashMap<String, toml::Value>,

    /// Roblox `Ref` properties, keyed by Roblox property name. Resolved
    /// to Eustress uuids by the materializer's second pass and written
    /// under `[references]`.
    pub refs: HashMap<String, Ref>,

    /// Asset references (`rbxassetid://`, `rbxasset://`, `http(s)://`)
    /// keyed by Roblox property name. The materializer routes these
    /// through the asset resolver; resolved paths land in
    /// `[asset]` (mesh / path) or `[properties.extras]` as fallback.
    pub asset_refs: HashMap<String, String>,

    /// Source for Script / LocalScript / ModuleScript. Held verbatim;
    /// the materializer optionally pipes it through
    /// `compat::ScriptTransformer` and writes to the canonical
    /// `script.luau` / `script.lua` next to the instance TOML.
    pub script_source: Option<String>,

    /// Unmapped properties (key + Variant type tag string) — surfaced
    /// to `ImportReport::unmapped_properties`.
    pub unmapped: Vec<UnmappedRecord>,

    /// Approximation log entries — surfaced to
    /// `ImportReport::approximations` (Int64 truncation, BrickColor →
    /// Color3, etc.).
    pub approximation_notes: Vec<String>,
}

/// A property that surfaced during mapping but the importer didn't
/// translate into either a first-class slot or a typed extra.
#[derive(Debug, Clone)]
pub struct UnmappedRecord {
    /// Roblox property name (e.g. `"CollisionGroupId"`).
    pub property: String,
    /// `rbx_types::Variant` type tag (e.g. `"Int32"`, `"Color3"`).
    pub variant_type: String,
}

impl PropertyBag {
    /// Consume the bag, returning just the `InstanceOverrides`. The
    /// rest of the bag (extras, refs, asset refs, tags, attributes,
    /// script source) is what the materializer post-processes after
    /// the canonical `create_instance` call lands.
    pub fn into_overrides(self) -> InstanceOverrides {
        self.overrides
    }
}

// ---------------------------------------------------------------------------
// Entry point — map a Roblox property map to a PropertyBag.
// ---------------------------------------------------------------------------

/// Transform a Roblox property map (as decoded by `rbx_dom_weak`) into a
/// [`PropertyBag`] shaped for the target Eustress class.
///
/// The `target_class` is informational only at this layer — every
/// per-Variant translation is value-driven, not class-driven. Class
/// influences which properties make sense (e.g. `Sound.SoundId` is an
/// asset ref; `Decal.Texture` is too), but the dispatch happens via the
/// Roblox property name, which is what the spec table is keyed on.
pub fn map_properties(
    rbx_props: &HashMap<String, Variant>,
    target_class: ClassName,
) -> PropertyBag {
    let mut bag = PropertyBag::default();
    for (key, variant) in rbx_props {
        apply_variant(&mut bag, target_class, key.as_str(), variant);
    }
    bag
}

/// Per-property dispatch. Splits a Roblox `(name, Variant)` pair into
/// either a well-known slot on [`InstanceOverrides`] (when the name +
/// variant kind match a known Eustress slot) or onto one of the
/// PropertyBag's extras buckets.
fn apply_variant(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) {
    // The Roblox `Name` and `Parent` properties never land in extras —
    // they're handled by the materializer (folder name + tree shape).
    if key == "Name" || key == "Parent" {
        return;
    }

    // ── Asset-reference shortcut ────────────────────────────────────
    //
    // The spec's §11 routes Content/ContentId/SoundId/MeshId/Image/etc.
    // through the asset resolver. We recognise the well-known asset
    // property names + every `Content` variant.
    if is_asset_property_name(key) {
        if let Variant::Content(c) = variant {
            bag.asset_refs
                .insert(key.to_string(), AsRef::<str>::as_ref(c).to_string());
            return;
        }
        if let Variant::String(s) = variant {
            // Some older `.rbxl`s store asset URIs as plain strings on
            // `SoundId`, `Image`, `MeshId`, etc. instead of the typed
            // `Content` variant. Treat them the same.
            bag.asset_refs.insert(key.to_string(), s.clone());
            return;
        }
    }

    // ── Script source ──────────────────────────────────────────────
    //
    // Scripts hold their body in the `Source` property (a Roblox
    // ProtectedString → exposed here as `Variant::String`). We hoist
    // it out so the materializer can write it as a sibling `script.luau`.
    if key == "Source"
        && matches!(
            target_class,
            ClassName::LuauScript
                | ClassName::LuauLocalScript
                | ClassName::LuauModuleScript
                | ClassName::SoulScript
        )
    {
        if let Variant::String(s) = variant {
            bag.script_source = Some(s.clone());
            return;
        }
    }

    // ── Well-known overrides (Position, Size, Color, Material, …) ──
    if try_well_known(bag, target_class, key, variant) {
        return;
    }

    // ── Tags + Attributes top-level handling ───────────────────────
    if let Variant::Tags(tags) = variant {
        for t in tags.iter() {
            bag.tags.push(t.to_string());
        }
        return;
    }
    if let Variant::Attributes(attrs) = variant {
        // `Attributes` is opaque in rbx_types; we lift the serialised
        // form into `properties.attributes`. The wire form is a sequence
        // of (name, value) pairs; we approximate by serialising the
        // whole blob as a JSON-via-debug-string so the field survives
        // the round-trip without losing data. Wave-3 first-class
        // promotion will deserialise these into per-key TOML values.
        let dbg = format!("{:?}", attrs);
        bag.attributes
            .insert("_raw_debug".to_string(), toml::Value::String(dbg));
        return;
    }

    // ── PhysicalProperties decomposition ───────────────────────────
    if let Variant::PhysicalProperties(pp) = variant {
        match pp {
            PhysicalProperties::Default => {
                bag.physics_extras
                    .insert("preset".to_string(), toml::Value::String("Default".into()));
            }
            PhysicalProperties::Custom(c) => {
                bag.physics_extras
                    .insert("density".to_string(), toml::Value::Float(c.density as f64));
                bag.physics_extras.insert(
                    "friction".to_string(),
                    toml::Value::Float(c.friction as f64),
                );
                bag.physics_extras.insert(
                    "elasticity".to_string(),
                    toml::Value::Float(c.elasticity as f64),
                );
                bag.physics_extras.insert(
                    "friction_weight".to_string(),
                    toml::Value::Float(c.friction_weight as f64),
                );
                bag.physics_extras.insert(
                    "elasticity_weight".to_string(),
                    toml::Value::Float(c.elasticity_weight as f64),
                );
            }
        }
        return;
    }

    // ── Ref handling — collected for the second-pass resolution. ───
    if let Variant::Ref(r) = variant {
        bag.refs.insert(key.to_string(), *r);
        return;
    }

    // ── Variant → opaque TOML representation for everything else. ──
    let toml_val = variant_to_toml(variant, bag);
    bag.properties_extras.insert(key.to_string(), toml_val);
}

// ---------------------------------------------------------------------------
// Well-known overrides (Position/Size/Color/etc. → InstanceOverrides slots)
// ---------------------------------------------------------------------------

fn try_well_known(
    bag: &mut PropertyBag,
    _target_class: ClassName,
    key: &str,
    variant: &Variant,
) -> bool {
    match key {
        // ─── Position / CFrame / Size ──────────────────────────────
        "CFrame" => {
            if let Variant::CFrame(cf) = variant {
                let (translation, rotation) = cframe_to_translation_quat(cf);
                bag.overrides.position = Some(translation);
                bag.overrides.rotation = Some(rotation);
                return true;
            }
        }
        "Position" => {
            if let Variant::Vector3(v) = variant {
                bag.overrides.position = Some([v.x, v.y, v.z]);
                return true;
            }
        }
        "Size" => {
            if let Variant::Vector3(v) = variant {
                // Eustress `scale` is `Vec3` of meters — Roblox `Size`
                // is studs and per the spec we treat 1 stud = 1 m.
                bag.overrides.scale = Some([v.x, v.y, v.z]);
                return true;
            }
        }
        "Orientation" => {
            // Roblox `Orientation` is an Euler-angle Vec3 in degrees
            // (Y / X / Z order). Only land it if a CFrame hasn't
            // already covered the rotation — CFrame is the canonical
            // source.
            if let (Variant::Vector3(v), None) = (variant, &bag.overrides.rotation) {
                let rot = euler_yxz_degrees_to_quat(v.x, v.y, v.z);
                bag.overrides.rotation = Some(rot);
                return true;
            }
        }

        // ─── Color / Material ──────────────────────────────────────
        "Color" => match variant {
            Variant::Color3(c) => {
                bag.overrides.color_rgba = Some([c.r, c.g, c.b, 1.0]);
                return true;
            }
            Variant::Color3uint8(c) => {
                bag.overrides.color_rgba = Some([
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    1.0,
                ]);
                return true;
            }
            _ => {}
        },
        "BrickColor" => {
            if let Variant::BrickColor(bc) = variant {
                let c = bc.to_color3uint8();
                bag.overrides.color_rgba = Some([
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    1.0,
                ]);
                bag.approximation_notes
                    .push(format!("BrickColor '{}' → Color3", bc));
                return true;
            }
        }
        "Transparency" => {
            // Roblox uses 0 = opaque, 1 = invisible. We modify alpha on
            // the color override if a color is already present;
            // otherwise stash as an extra.
            if let Variant::Float32(t) = variant {
                let alpha = (1.0 - t).clamp(0.0, 1.0);
                match bag.overrides.color_rgba.as_mut() {
                    Some(rgba) => rgba[3] = alpha,
                    None => bag.overrides.color_rgba = Some([1.0, 1.0, 1.0, alpha]),
                }
                return true;
            }
        }
        "Material" => {
            if let Variant::Enum(e) = variant {
                bag.overrides.material = Some(roblox_material_enum_to_name(e.to_u32()));
                return true;
            }
            if let Variant::String(s) = variant {
                bag.overrides.material = Some(s.clone());
                return true;
            }
        }

        // ─── Anchored / CanCollide ─────────────────────────────────
        "Anchored" => {
            if let Variant::Bool(b) = variant {
                bag.overrides.anchored = Some(*b);
                return true;
            }
        }
        "CanCollide" => {
            if let Variant::Bool(b) = variant {
                bag.overrides.can_collide = Some(*b);
                return true;
            }
        }

        _ => {}
    }
    false
}

// ---------------------------------------------------------------------------
// CFrame → translation + quaternion rotation
// ---------------------------------------------------------------------------

/// Spec §10.1 — Roblox CFrame stores rotation as a row-major 3×3
/// matrix; `glam` (and our `[f32; 4]` quaternion slot) want a quaternion
/// derived from a column-basis matrix. Function inlined here so the
/// crate doesn't need `glam` directly.
fn cframe_to_translation_quat(cf: &CFrame) -> ([f32; 3], [f32; 4]) {
    let translation = [cf.position.x, cf.position.y, cf.position.z];
    // Roblox: rows are basis vectors (right, up, back).
    let r = cf.orientation.x;
    let u = cf.orientation.y;
    let b = cf.orientation.z;
    // Build the rotation matrix in column-basis form. The "back" basis
    // vector points into +Z in Roblox; glam's convention follows the
    // right-up-back convention as columns, so this is a transpose of
    // the row-stored layout.
    let m = [[r.x, r.y, r.z], [u.x, u.y, u.z], [b.x, b.y, b.z]];
    let q = mat3_to_quat(m);
    (translation, q)
}

/// `glam::Quat::from_mat3` re-implemented to avoid a `glam` dep. Returns
/// quaternion as `[x, y, z, w]`. The matrix is in column-major
/// `[col0, col1, col2]` form per `cframe_to_translation_quat`.
fn mat3_to_quat(m: [[f32; 3]; 3]) -> [f32; 4] {
    // Shepherd's algorithm — picks the largest diagonal element for
    // numerical stability.
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

/// Roblox Euler-angles → quaternion. Roblox `Orientation` is stored as
/// `(x, y, z)` rotations in degrees applied in YXZ order.
fn euler_yxz_degrees_to_quat(x_deg: f32, y_deg: f32, z_deg: f32) -> [f32; 4] {
    let (x, y, z) = (x_deg.to_radians(), y_deg.to_radians(), z_deg.to_radians());
    // Half-angles
    let (sx, cx) = (x * 0.5).sin_cos();
    let (sy, cy) = (y * 0.5).sin_cos();
    let (sz, cz) = (z * 0.5).sin_cos();
    // YXZ Tait-Bryan composition: Q = Qy * Qx * Qz.
    // Pre-multiplying out gives:
    [
        cy * sx * cz + sy * cx * sz,
        sy * cx * cz - cy * sx * sz,
        cy * cx * sz - sy * sx * cz,
        cy * cx * cz + sy * sx * sz,
    ]
}

// ---------------------------------------------------------------------------
// Generic Variant → toml::Value (for the extras path)
// ---------------------------------------------------------------------------

/// Convert a Roblox `Variant` to a `toml::Value` for the extras path.
/// The function logs any approximations (Int64 truncation, etc.) into
/// `bag.approximation_notes`. Returns `toml::Value::String("<unsupported>")`
/// for the few catch-all branches.
fn variant_to_toml(variant: &Variant, bag: &mut PropertyBag) -> toml::Value {
    match variant {
        Variant::Bool(b) => toml::Value::Boolean(*b),
        Variant::String(s) => toml::Value::String(s.clone()),
        Variant::Int32(i) => toml::Value::Integer(*i as i64),
        Variant::Int64(i) => {
            // Roblox Int64 may exceed i32 range; toml::Value::Integer
            // is i64 so the value lands fine, but flag the truncation
            // risk for any downstream consumer that casts back to i32.
            if *i > i32::MAX as i64 || *i < i32::MIN as i64 {
                bag.approximation_notes
                    .push(format!("Int64 value {} exceeds i32 range", i));
            }
            toml::Value::Integer(*i)
        }
        Variant::Float32(f) => toml::Value::Float(*f as f64),
        Variant::Float64(f) => {
            bag.approximation_notes
                .push("Float64 downcast to f32 round-trip".to_string());
            toml::Value::Float(*f)
        }
        Variant::Vector2(v) => vec2_to_toml(v),
        Variant::Vector2int16(v) => vec2i16_to_toml(v),
        Variant::Vector3(v) => vec3_to_toml(v),
        Variant::Vector3int16(v) => vec3i16_to_toml(v),
        Variant::CFrame(cf) => cframe_to_toml(cf),
        Variant::OptionalCFrame(opt) => match opt {
            Some(cf) => cframe_to_toml(cf),
            None => toml::Value::String("None".to_string()),
        },
        Variant::Color3(c) => color3_to_toml(c),
        Variant::Color3uint8(c) => color3uint8_to_toml(c),
        Variant::BrickColor(bc) => brick_color_to_toml(bc, bag),
        Variant::UDim(u) => udim_to_toml(u),
        Variant::UDim2(u) => udim2_to_toml(u),
        Variant::Rect(r) => rect_to_toml(r),
        Variant::Enum(e) => toml::Value::Integer(e.to_u32() as i64),
        Variant::Content(c) => toml::Value::String(AsRef::<str>::as_ref(c).to_string()),
        Variant::BinaryString(bs) => binary_string_to_toml(bs),
        Variant::SharedString(ss) => shared_string_to_toml(ss),
        Variant::NumberSequence(ns) => number_sequence_to_toml(ns),
        Variant::ColorSequence(cs) => color_sequence_to_toml(cs),
        Variant::NumberRange(nr) => number_range_to_toml(nr),
        Variant::Ray(ray) => ray_to_toml(ray),
        Variant::Region3(r) => region3_to_toml(r),
        Variant::Region3int16(r) => region3int16_to_toml(r),
        Variant::Faces(f) => faces_to_toml(f),
        Variant::Axes(a) => axes_to_toml(a),
        Variant::MaterialColors(mc) => material_colors_to_toml(mc),
        Variant::Font(f) => font_to_toml(f),
        Variant::UniqueId(uid) => unique_id_to_toml(uid),
        // Tags / Attributes / Ref / PhysicalProperties handled at
        // `apply_variant`'s top — those paths never reach here. Keep
        // catch-all arms for the `#[non_exhaustive]` future-proofing
        // (rbx_types may add variants in a minor release).
        Variant::Tags(t) => {
            let arr: Vec<toml::Value> = t
                .iter()
                .map(|s| toml::Value::String(s.to_string()))
                .collect();
            toml::Value::Array(arr)
        }
        Variant::Attributes(_) => toml::Value::String("<attributes>".to_string()),
        Variant::Ref(r) => toml::Value::String(format!("{}", r)),
        Variant::PhysicalProperties(_) => toml::Value::String("<physical_properties>".to_string()),
        Variant::SecurityCapabilities(_) => {
            bag.approximation_notes
                .push("SecurityCapabilities discarded — no Eustress cognate".to_string());
            toml::Value::String("<security_capabilities>".to_string())
        }
        // Future-proof catch-all (Variant is `#[non_exhaustive]`).
        _ => toml::Value::String(format!("{:?}", variant.ty())),
    }
}

// ---------------------------------------------------------------------------
// Per-variant TOML encoders
// ---------------------------------------------------------------------------

fn vec2_to_toml(v: &Vector2) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(v.x as f64),
        toml::Value::Float(v.y as f64),
    ])
}

fn vec2i16_to_toml(v: &Vector2int16) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Integer(v.x as i64),
        toml::Value::Integer(v.y as i64),
    ])
}

fn vec3_to_toml(v: &Vector3) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(v.x as f64),
        toml::Value::Float(v.y as f64),
        toml::Value::Float(v.z as f64),
    ])
}

fn vec3i16_to_toml(v: &Vector3int16) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Integer(v.x as i64),
        toml::Value::Integer(v.y as i64),
        toml::Value::Integer(v.z as i64),
    ])
}

fn cframe_to_toml(cf: &CFrame) -> toml::Value {
    // 12-element row form: position (3) + orientation rows (3 × 3).
    let p = &cf.position;
    let r = &cf.orientation.x;
    let u = &cf.orientation.y;
    let b = &cf.orientation.z;
    toml::Value::Array(vec![
        toml::Value::Float(p.x as f64),
        toml::Value::Float(p.y as f64),
        toml::Value::Float(p.z as f64),
        toml::Value::Float(r.x as f64),
        toml::Value::Float(r.y as f64),
        toml::Value::Float(r.z as f64),
        toml::Value::Float(u.x as f64),
        toml::Value::Float(u.y as f64),
        toml::Value::Float(u.z as f64),
        toml::Value::Float(b.x as f64),
        toml::Value::Float(b.y as f64),
        toml::Value::Float(b.z as f64),
    ])
}

fn color3_to_toml(c: &Color3) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(c.r as f64),
        toml::Value::Float(c.g as f64),
        toml::Value::Float(c.b as f64),
    ])
}

fn color3uint8_to_toml(c: &Color3uint8) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(c.r as f64 / 255.0),
        toml::Value::Float(c.g as f64 / 255.0),
        toml::Value::Float(c.b as f64 / 255.0),
    ])
}

fn brick_color_to_toml(bc: &BrickColor, bag: &mut PropertyBag) -> toml::Value {
    let c = bc.to_color3uint8();
    bag.approximation_notes
        .push(format!("BrickColor '{}' → Color3 RGB", bc));
    toml::Value::Array(vec![
        toml::Value::Float(c.r as f64 / 255.0),
        toml::Value::Float(c.g as f64 / 255.0),
        toml::Value::Float(c.b as f64 / 255.0),
    ])
}

fn udim_to_toml(u: &UDim) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(u.scale as f64),
        toml::Value::Integer(u.offset as i64),
    ])
}

fn udim2_to_toml(u: &UDim2) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(u.x.scale as f64),
        toml::Value::Integer(u.x.offset as i64),
        toml::Value::Float(u.y.scale as f64),
        toml::Value::Integer(u.y.offset as i64),
    ])
}

fn rect_to_toml(r: &Rect) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(r.min.x as f64),
        toml::Value::Float(r.min.y as f64),
        toml::Value::Float(r.max.x as f64),
        toml::Value::Float(r.max.y as f64),
    ])
}

fn binary_string_to_toml(bs: &BinaryString) -> toml::Value {
    // hex-encode so the value round-trips through TOML safely.
    let bytes: &[u8] = bs.as_ref();
    toml::Value::String(hex::encode(bytes))
}

fn shared_string_to_toml(_ss: &SharedString) -> toml::Value {
    // SharedString is a hash + buffer in the rbx_types crate. Public
    // API doesn't expose the raw bytes directly — we record a marker
    // that the property was present so a future Wave 4.A.2/3 pass can
    // pull the data from the file's shared-string table. For now, a
    // placeholder is enough to round-trip "presence".
    toml::Value::String("<shared_string>".to_string())
}

fn number_sequence_to_toml(ns: &NumberSequence) -> toml::Value {
    let kp: Vec<toml::Value> = ns
        .keypoints
        .iter()
        .map(|k| {
            toml::Value::Array(vec![
                toml::Value::Float(k.time as f64),
                toml::Value::Float(k.value as f64),
                toml::Value::Float(k.envelope as f64),
            ])
        })
        .collect();
    toml::Value::Array(kp)
}

fn color_sequence_to_toml(cs: &ColorSequence) -> toml::Value {
    let kp: Vec<toml::Value> = cs
        .keypoints
        .iter()
        .map(|k| {
            toml::Value::Array(vec![
                toml::Value::Float(k.time as f64),
                toml::Value::Float(k.color.r as f64),
                toml::Value::Float(k.color.g as f64),
                toml::Value::Float(k.color.b as f64),
            ])
        })
        .collect();
    toml::Value::Array(kp)
}

fn number_range_to_toml(nr: &NumberRange) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(nr.min as f64),
        toml::Value::Float(nr.max as f64),
    ])
}

fn ray_to_toml(r: &Ray) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(r.origin.x as f64),
        toml::Value::Float(r.origin.y as f64),
        toml::Value::Float(r.origin.z as f64),
        toml::Value::Float(r.direction.x as f64),
        toml::Value::Float(r.direction.y as f64),
        toml::Value::Float(r.direction.z as f64),
    ])
}

fn region3_to_toml(r: &Region3) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(r.min.x as f64),
        toml::Value::Float(r.min.y as f64),
        toml::Value::Float(r.min.z as f64),
        toml::Value::Float(r.max.x as f64),
        toml::Value::Float(r.max.y as f64),
        toml::Value::Float(r.max.z as f64),
    ])
}

fn region3int16_to_toml(r: &Region3int16) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Integer(r.min.x as i64),
        toml::Value::Integer(r.min.y as i64),
        toml::Value::Integer(r.min.z as i64),
        toml::Value::Integer(r.max.x as i64),
        toml::Value::Integer(r.max.y as i64),
        toml::Value::Integer(r.max.z as i64),
    ])
}

fn faces_to_toml(f: &Faces) -> toml::Value {
    toml::Value::Integer(f.bits() as i64)
}

fn axes_to_toml(a: &Axes) -> toml::Value {
    toml::Value::Integer(a.bits() as i64)
}

fn material_colors_to_toml(mc: &MaterialColors) -> toml::Value {
    // `MaterialColors::encode` produces a binary blob (see rbx_types
    // source). For first-pass round-trip we hex-encode and let the
    // Wave 4.A.2 terrain dispatcher decode it properly via the typed
    // accessors.
    let encoded = mc.encode();
    toml::Value::String(hex::encode(&encoded))
}

fn font_to_toml(f: &Font) -> toml::Value {
    let mut t = toml::value::Table::new();
    t.insert("family".to_string(), toml::Value::String(f.family.clone()));
    t.insert(
        "weight".to_string(),
        toml::Value::String(format!("{:?}", f.weight)),
    );
    t.insert(
        "style".to_string(),
        toml::Value::String(format!("{:?}", f.style)),
    );
    if let Some(cf) = &f.cached_face_id {
        t.insert(
            "cached_face_id".to_string(),
            toml::Value::String(cf.clone()),
        );
    }
    toml::Value::Table(t)
}

fn unique_id_to_toml(uid: &UniqueId) -> toml::Value {
    toml::Value::String(format!("{}", uid))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Names of properties that point to a Roblox asset (via either
/// `Content` variant or a raw string URI).
fn is_asset_property_name(name: &str) -> bool {
    matches!(
        name,
        "SoundId"
            | "MeshId"
            | "Image"
            | "Texture"
            | "TextureId"
            | "DecalTexture"
            | "CollisionMeshId"
            | "Skin"
            | "AnimationId"
            | "VideoId"
            | "SkyboxBk"
            | "SkyboxDn"
            | "SkyboxFt"
            | "SkyboxLf"
            | "SkyboxRt"
            | "SkyboxUp"
            | "AssetId"
            | "FaceTextureId"
            | "TextureID"
            | "FrontMaterial"
            | "Content"
    )
}

/// Roblox `Material` enum value → Eustress material preset name.
/// Source numbers come from Roblox's Material enum; we collapse
/// near-equivalents per spec §6.3.
fn roblox_material_enum_to_name(value: u32) -> String {
    let name = match value {
        256 => "Plastic",
        272 => "SmoothPlastic",
        288 => "Neon",
        544 => "Wood",
        545 => "WoodPlanks",
        816 => "Marble",
        784 => "Granite",
        832 => "Slate",
        800 => "Concrete",
        880 => "CrackedLava",
        864 => "Brick",
        848 => "Pebble",
        1280 => "Sand",
        1296 => "Fabric",
        1536 => "Snow",
        1552 => "Glacier",
        1568 => "Ice",
        1280..=1300 => "Sand",
        1792 => "Cobblestone",
        1808 => "Rock",
        1824 => "Sandstone",
        1840 => "CorrodedMetal",
        1856 => "DiamondPlate",
        1872 => "Foil",
        1888 => "Metal",
        1904 => "Grass",
        1920 => "LeafyGrass",
        1936 => "Mud",
        1952 => "Ground",
        1968 => "Asphalt",
        1984 => "Salt",
        2000 => "Limestone",
        2016 => "Basalt",
        2032 => "Pavement",
        2048 => "Glass",
        2064 => "ForceField",
        _ => return format!("RobloxMaterial_{}", value),
    };
    name.to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom_weak::types::{Color3, Color3uint8, Matrix3, Variant, Vector3};

    fn props_with(pairs: Vec<(&str, Variant)>) -> HashMap<String, Variant> {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    #[test]
    fn cframe_identity_to_quat_is_identity() {
        let id = CFrame::new(Vector3::new(0.0, 0.0, 0.0), Matrix3::identity());
        let (t, q) = cframe_to_translation_quat(&id);
        assert_eq!(t, [0.0, 0.0, 0.0]);
        // Quat::identity = [0, 0, 0, 1]
        assert!((q[0]).abs() < 1e-6);
        assert!((q[1]).abs() < 1e-6);
        assert!((q[2]).abs() < 1e-6);
        assert!((q[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cframe_position_carries_through() {
        let cf = CFrame::new(Vector3::new(1.5, -2.0, 7.25), Matrix3::identity());
        let (t, _) = cframe_to_translation_quat(&cf);
        assert_eq!(t, [1.5, -2.0, 7.25]);
    }

    #[test]
    fn vector3_position_lands_on_overrides() {
        let bag = map_properties(
            &props_with(vec![(
                "Position",
                Variant::Vector3(Vector3::new(1.0, 2.0, 3.0)),
            )]),
            ClassName::Part,
        );
        assert_eq!(bag.overrides.position, Some([1.0, 2.0, 3.0]));
    }

    #[test]
    fn size_lands_on_scale() {
        let bag = map_properties(
            &props_with(vec![(
                "Size",
                Variant::Vector3(Vector3::new(4.0, 1.0, 4.0)),
            )]),
            ClassName::Part,
        );
        assert_eq!(bag.overrides.scale, Some([4.0, 1.0, 4.0]));
    }

    #[test]
    fn color3_lands_on_color_rgba() {
        let bag = map_properties(
            &props_with(vec![(
                "Color",
                Variant::Color3(Color3::new(0.5, 0.25, 0.75)),
            )]),
            ClassName::Part,
        );
        let rgba = bag.overrides.color_rgba.unwrap();
        assert!((rgba[0] - 0.5).abs() < 1e-6);
        assert!((rgba[1] - 0.25).abs() < 1e-6);
        assert!((rgba[2] - 0.75).abs() < 1e-6);
        assert!((rgba[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn color3uint8_normalises_to_floats() {
        let bag = map_properties(
            &props_with(vec![(
                "Color",
                Variant::Color3uint8(Color3uint8::new(255, 0, 128)),
            )]),
            ClassName::Part,
        );
        let rgba = bag.overrides.color_rgba.unwrap();
        assert!((rgba[0] - 1.0).abs() < 1e-6);
        assert!((rgba[1] - 0.0).abs() < 1e-6);
        assert!((rgba[2] - 128.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn anchored_lands_directly() {
        let bag = map_properties(
            &props_with(vec![("Anchored", Variant::Bool(true))]),
            ClassName::Part,
        );
        assert_eq!(bag.overrides.anchored, Some(true));
    }

    #[test]
    fn can_collide_lands_directly() {
        let bag = map_properties(
            &props_with(vec![("CanCollide", Variant::Bool(false))]),
            ClassName::Part,
        );
        assert_eq!(bag.overrides.can_collide, Some(false));
    }

    #[test]
    fn transparency_modulates_color_alpha() {
        let bag = map_properties(
            &props_with(vec![("Transparency", Variant::Float32(0.5))]),
            ClassName::Part,
        );
        let rgba = bag.overrides.color_rgba.unwrap();
        assert!((rgba[3] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn sound_id_routes_to_asset_refs() {
        let bag = map_properties(
            &props_with(vec![(
                "SoundId",
                Variant::Content(Content::from("rbxassetid://12345")),
            )]),
            ClassName::Sound,
        );
        assert_eq!(
            bag.asset_refs.get("SoundId"),
            Some(&"rbxassetid://12345".to_string())
        );
    }

    #[test]
    fn texture_string_uri_routes_to_asset_refs() {
        let bag = map_properties(
            &props_with(vec![(
                "Texture",
                Variant::String("rbxassetid://9876".to_string()),
            )]),
            ClassName::Decal,
        );
        assert_eq!(
            bag.asset_refs.get("Texture"),
            Some(&"rbxassetid://9876".to_string())
        );
    }

    #[test]
    fn ref_lands_in_refs_bucket() {
        let r = Ref::new();
        let bag = map_properties(
            &props_with(vec![("Part0", Variant::Ref(r))]),
            ClassName::WeldConstraint,
        );
        assert_eq!(bag.refs.get("Part0"), Some(&r));
    }

    #[test]
    fn physical_properties_decomposes() {
        use rbx_dom_weak::types::CustomPhysicalProperties;
        let pp = PhysicalProperties::Custom(CustomPhysicalProperties {
            density: 1.5,
            friction: 0.3,
            elasticity: 0.0,
            friction_weight: 1.0,
            elasticity_weight: 1.0,
        });
        let bag = map_properties(
            &props_with(vec![(
                "CustomPhysicalProperties",
                Variant::PhysicalProperties(pp),
            )]),
            ClassName::Part,
        );
        assert!(bag.physics_extras.contains_key("density"));
        assert!(bag.physics_extras.contains_key("friction"));
    }

    #[test]
    fn tags_land_in_tags_bucket() {
        let mut t = Tags::new();
        t.push("Enemy");
        t.push("Boss");
        let bag = map_properties(
            &props_with(vec![("Tags", Variant::Tags(t))]),
            ClassName::Part,
        );
        assert_eq!(bag.tags, vec!["Enemy".to_string(), "Boss".to_string()]);
    }

    #[test]
    fn script_source_lifts_to_script_source_field() {
        let bag = map_properties(
            &props_with(vec![(
                "Source",
                Variant::String("print('hello')".to_string()),
            )]),
            ClassName::LuauScript,
        );
        assert_eq!(bag.script_source.as_deref(), Some("print('hello')"));
    }

    #[test]
    fn extras_path_covers_int32() {
        let bag = map_properties(
            &props_with(vec![("CollisionGroupId", Variant::Int32(7))]),
            ClassName::Part,
        );
        assert_eq!(
            bag.properties_extras.get("CollisionGroupId"),
            Some(&toml::Value::Integer(7))
        );
    }

    #[test]
    fn brick_color_to_color3_logs_approximation() {
        let bag = map_properties(
            &props_with(vec![(
                "BrickColor",
                Variant::BrickColor(BrickColor::ReallyRed),
            )]),
            ClassName::Part,
        );
        assert!(bag.overrides.color_rgba.is_some());
        assert!(
            !bag.approximation_notes.is_empty(),
            "BrickColor conversion should log an approximation note"
        );
    }

    #[test]
    fn udim2_round_trips_through_extras() {
        let u = UDim2::new(UDim::new(0.5, 100), UDim::new(0.0, 50));
        let bag = map_properties(
            &props_with(vec![("Size", Variant::UDim2(u))]),
            ClassName::Frame,
        );
        // For a GUI Size we don't have a well-known slot — it lands in
        // extras (the size override slot is for 3D scale, not UDim2).
        assert!(bag.properties_extras.contains_key("Size"));
    }

    #[test]
    fn enum_extras_carry_integer_label() {
        let bag = map_properties(
            &props_with(vec![("Shape", Variant::Enum(Enum::from_u32(2)))]),
            ClassName::Part,
        );
        assert_eq!(
            bag.properties_extras.get("Shape"),
            Some(&toml::Value::Integer(2))
        );
    }

    #[test]
    fn material_enum_resolves_to_string() {
        let bag = map_properties(
            &props_with(vec![("Material", Variant::Enum(Enum::from_u32(288)))]),
            ClassName::Part,
        );
        assert_eq!(bag.overrides.material.as_deref(), Some("Neon"));
    }

    #[test]
    fn vector2_lands_in_extras() {
        let bag = map_properties(
            &props_with(vec![(
                "WindForce",
                Variant::Vector2(Vector2::new(2.0, -1.0)),
            )]),
            ClassName::Part,
        );
        assert!(bag.properties_extras.contains_key("WindForce"));
    }

    #[test]
    fn binary_string_hex_encoded() {
        let bs = BinaryString::from(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let bag = map_properties(
            &props_with(vec![("MeshData", Variant::BinaryString(bs))]),
            ClassName::Part,
        );
        assert_eq!(
            bag.properties_extras.get("MeshData"),
            Some(&toml::Value::String("deadbeef".to_string()))
        );
    }

    #[test]
    fn parent_and_name_are_dropped() {
        let bag = map_properties(
            &props_with(vec![
                ("Name", Variant::String("Foo".to_string())),
                ("Parent", Variant::Ref(Ref::none())),
            ]),
            ClassName::Folder,
        );
        // Neither field should land anywhere — names come from the
        // walker, parents come from the tree shape.
        assert!(bag.properties_extras.is_empty());
        assert!(bag.refs.is_empty());
    }
}

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
    Axes, BinaryString, BrickColor, CFrame, Color3, Color3uint8, ColorSequence, Content,
    ContentId, Enum, Faces, Font, MaterialColors, NumberRange, NumberSequence,
    PhysicalProperties, Ray, Rect, Ref, Region3, Region3int16, SharedString, Tags, UDim, UDim2,
    UniqueId, Variant, Vector2, Vector2int16, Vector3, Vector3int16,
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

    /// Additive `[metadata]` scalar keys (e.g. `roblox_brick_color`,
    /// `roblox_color_srgb`). Written verbatim under `[metadata]` by the
    /// materializer; never read by the part-color render path (which uses
    /// `overrides.color_rgba`). Lets an imported part keep its original
    /// Roblox BrickColor token + full sRGB-0-255 alongside the rendered
    /// 0..1 color.
    pub metadata_extras: HashMap<String, toml::Value>,

    /// Overrides for whole top-level TOML sections other than `[properties]`,
    /// keyed `section -> (key -> value)`. Used for GUI elements whose engine
    /// schema lives in dedicated sections (`[gui]`, `[text]`) rather than
    /// `[properties]`: a TextLabel's `Text` -> `[text].text`, any GUI's
    /// `ZIndex` -> `[gui].z_index`. The materializer merges these onto the
    /// class template's existing sections, so unset keys keep their defaults.
    pub section_props: HashMap<String, HashMap<String, toml::Value>>,
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
                .insert(key.to_string(), c.as_uri().unwrap_or_default().to_string());
            return;
        }
        if let Variant::ContentId(c) = variant {
            // rbx_types 3.x split the old string `Content` into the legacy
            // `ContentId` (a bare URI string — what `SpecialMesh.MeshId` /
            // `FileMesh.TextureId` and other non-migrated properties decode
            // to) and the modern object `Content`. Without this arm a
            // `ContentId` mesh ref fell through to the opaque-extras path
            // and never reached the asset resolver — imported SpecialMesh
            // geometry was the visible symptom.
            bag.asset_refs.insert(key.to_string(), c.as_str().to_string());
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

    // ── Light properties (PointLight / SpotLight / SurfaceLight) ───
    //
    // Routed BEFORE `try_well_known` so a light's `Color`/`Brightness`/…
    // land in the light bucket instead of being captured as a part color
    // (`try_well_known` would otherwise grab `Color`). These keys flow
    // into `[properties.extras]` (the only patchable bucket the
    // materializer exposes); the engine's light-load arm reads them back
    // over the class template's `[Light]` defaults. See
    // `engine::space::file_loader::spawn_directory_entry`.
    if try_light_property(bag, target_class, key, variant) {
        return;
    }

    // ── Environment / Sound / Particle / Beam / Decal+Mesh / Character /
    //    Constraint / Spawn-Seat-Vehicle-Team / GUI-leaf families ──────
    // Routed BEFORE `try_well_known` so class-owned `Color`/`Transparency`/etc.
    // land in the class section instead of being captured as a BasePart color.
    // The GUI-leaf fn runs before `try_gui_property` so the corrected
    // Position/Size single-4-tuple + border_size win for those leaf classes;
    // unmatched shared GuiObject keys fall through to `try_gui_property`.
    if try_env_property(bag, target_class, key, variant) { return; }
    if try_sound_property(bag, target_class, key, variant) { return; }
    if try_particle_property(bag, target_class, key, variant) { return; }
    if try_beam_property(bag, target_class, key, variant) { return; }
    if try_decal_mesh_property(bag, target_class, key, variant) { return; }
    if try_character_property(bag, target_class, key, variant) { return; }
    if try_constraint_property(bag, target_class, key, variant) { return; }
    if try_spawn_seat_vehicle_team_property(bag, target_class, key, variant) { return; }
    if try_gui_leaf_property(bag, target_class, key, variant) { return; }

    // ── GUI section properties (Text → [text], ZIndex → [gui]) ────
    // GUI elements store these in dedicated top-level sections the engine's
    // `gui_loader` reads, NOT in `[properties]`. Routed before `try_well_known`
    // so they reach those sections instead of the inert `[properties.extras]`.
    if try_gui_property(bag, target_class, key, variant) {
        return;
    }

    // ── Well-known overrides (Position, Size, Color, Material, …) ──
    if try_well_known(bag, target_class, key, variant) {
        return;
    }

    // ── Residual class-specific props (run AFTER try_well_known so
    //    Color/Material/Anchored/CFrame keep flowing to overrides) ──
    if try_part_property(bag, target_class, key, variant) { return; }
    if try_camera_model_worldmodel_property(bag, target_class, key, variant) { return; }

    // ── Tags + Attributes top-level handling ───────────────────────
    if let Variant::Tags(tags) = variant {
        for t in tags.iter() {
            bag.tags.push(t.to_string());
        }
        return;
    }
    if let Variant::Attributes(attrs) = variant {
        // First-class attribute promotion: every (name, value) pair
        // becomes a typed `[attributes]` TOML key (bool / number / string /
        // Vector3 / Color3 / …) via the same `variant_to_toml` encoder the
        // extras path uses — so `GetAttribute`/`SetAttribute` bindings
        // survive the import with real values instead of an opaque
        // debug-string blob.
        for (name, value) in attrs.iter() {
            let v = variant_to_toml(value, bag);
            bag.attributes.insert(name.to_string(), v);
        }
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
                // Emit keys matching the engine loader's physics vocabulary
                // (`engine::space::instance_loader` reads these at the Avian
                // collider-insert sites). Roblox has a single scalar
                // `friction()`; Avian distinguishes static vs kinetic, so we
                // seed both from the one Roblox value. `elasticity()` maps to
                // Avian's `restitution`.
                bag.physics_extras
                    .insert("density".to_string(), toml::Value::Float(c.density() as f64));
                bag.physics_extras.insert(
                    "friction_static".to_string(),
                    toml::Value::Float(c.friction() as f64),
                );
                bag.physics_extras.insert(
                    "friction_kinetic".to_string(),
                    toml::Value::Float(c.friction() as f64),
                );
                bag.physics_extras.insert(
                    "restitution".to_string(),
                    toml::Value::Float(c.elasticity() as f64),
                );
                // Round-trip Roblox's weight knobs too — no Avian cognate
                // today, but preserved under `[properties.physics]` so a
                // re-export keeps them.
                bag.physics_extras.insert(
                    "friction_weight".to_string(),
                    toml::Value::Float(c.friction_weight() as f64),
                );
                bag.physics_extras.insert(
                    "elasticity_weight".to_string(),
                    toml::Value::Float(c.elasticity_weight() as f64),
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
                bag.metadata_extras.insert(
                    "roblox_color_srgb".to_string(),
                    toml::Value::Array(vec![
                        toml::Value::Integer((c.r * 255.0).round() as i64),
                        toml::Value::Integer((c.g * 255.0).round() as i64),
                        toml::Value::Integer((c.b * 255.0).round() as i64),
                    ]),
                );
                return true;
            }
            Variant::Color3uint8(c) => {
                bag.overrides.color_rgba = Some([
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    1.0,
                ]);
                bag.metadata_extras.insert(
                    "roblox_color_srgb".to_string(),
                    toml::Value::Array(vec![
                        toml::Value::Integer(c.r as i64),
                        toml::Value::Integer(c.g as i64),
                        toml::Value::Integer(c.b as i64),
                    ]),
                );
                return true;
            }
            _ => {}
        },
        "BrickColor" => {
            if let Variant::BrickColor(bc) = variant {
                let c = bc.to_color3uint8();
                // Render color stays 0..1 — unchanged behavior.
                bag.overrides.color_rgba = Some([
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    1.0,
                ]);
                // Preserve the categorical token + full sRGB-0-255. BrickColor
                // is `#[repr(u16)]` so the discriminant IS the palette number;
                // `*bc as u16` is the established repo cast (value_objects.rs).
                // `brick_number` feeds the per-place color manifest; the
                // `[metadata]` keys round-trip on disk and are never read by
                // the color render path.
                let token = *bc as u16;
                bag.overrides.brick_number = Some(token);
                bag.metadata_extras.insert(
                    "roblox_brick_color".to_string(),
                    toml::Value::Integer(token as i64),
                );
                bag.metadata_extras.insert(
                    "roblox_color_srgb".to_string(),
                    toml::Value::Array(vec![
                        toml::Value::Integer(c.r as i64),
                        toml::Value::Integer(c.g as i64),
                        toml::Value::Integer(c.b as i64),
                    ]),
                );
                bag.approximation_notes.push(format!(
                    "BrickColor '{}' → Color3 (token + sRGB preserved in metadata)",
                    bc
                ));
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

        // ─── Reflectance / CastShadow ──────────────────────────────
        // The runtime loader builds `BasePart.reflectance` / `cast_shadow`
        // from `[properties].reflectance` / `cast_shadow` and
        // `material_sync` applies them — so route these to first-class
        // override slots instead of letting them fall to inert extras.
        "Reflectance" => {
            if let Variant::Float32(r) = variant {
                bag.overrides.reflectance = Some(*r);
                return true;
            }
        }
        "CastShadow" => {
            if let Variant::Bool(b) = variant {
                bag.overrides.cast_shadow = Some(*b);
                return true;
            }
        }

        _ => {}
    }
    false
}

// ---------------------------------------------------------------------------
// GUI section properties (Text → [text], ZIndex → [gui])
// ---------------------------------------------------------------------------

/// True for any Roblox 2D-GUI class — the gate for routing into the engine's
/// dedicated `[gui]` / `[text]` sections rather than the inert extras bucket.
fn is_gui_class(c: ClassName) -> bool {
    matches!(
        c,
        ClassName::ScreenGui
            | ClassName::BillboardGui
            | ClassName::SurfaceGui
            | ClassName::Frame
            | ClassName::ScrollingFrame
            | ClassName::ViewportFrame
            | ClassName::TextLabel
            | ClassName::TextButton
            | ClassName::TextBox
            | ClassName::ImageLabel
            | ClassName::ImageButton
    )
}

/// Roblox `Color3` / `Color3uint8` → a `[r, g, b]` 0-255 integer array — the
/// form the engine's `[gui]` / `[text]` sections store colors in (the class
/// templates use `[255, 255, 255]` / `[0, 0, 0]`, not 0..1 floats).
fn color_u8(variant: &Variant) -> Option<toml::Value> {
    let (r, g, b) = match variant {
        Variant::Color3(c) => (
            (c.r * 255.0).round() as i64,
            (c.g * 255.0).round() as i64,
            (c.b * 255.0).round() as i64,
        ),
        Variant::Color3uint8(c) => (c.r as i64, c.g as i64, c.b as i64),
        _ => return None,
    };
    Some(toml::Value::Array(vec![
        toml::Value::Integer(r.clamp(0, 255)),
        toml::Value::Integer(g.clamp(0, 255)),
        toml::Value::Integer(b.clamp(0, 255)),
    ]))
}

/// A GUI enum property's ordinal (they arrive as `Variant::Enum`, sometimes
/// `Int32` from older files).
fn enum_u32(variant: &Variant) -> Option<u32> {
    match variant {
        Variant::Enum(e) => Some(e.to_u32()),
        Variant::Int32(i) => u32::try_from(*i).ok(),
        _ => None,
    }
}

fn val_bool(v: &Variant) -> Option<toml::Value> {
    if let Variant::Bool(b) = v {
        Some(toml::Value::Boolean(*b))
    } else {
        None
    }
}

fn val_float(v: &Variant) -> Option<toml::Value> {
    match v {
        Variant::Float32(f) => Some(toml::Value::Float(*f as f64)),
        Variant::Float64(f) => Some(toml::Value::Float(*f)),
        _ => None,
    }
}

fn val_int(v: &Variant) -> Option<toml::Value> {
    match v {
        Variant::Int32(i) => Some(toml::Value::Integer(*i as i64)),
        Variant::Int64(i) => Some(toml::Value::Integer(*i)),
        _ => None,
    }
}

fn val_string(v: &Variant) -> Option<toml::Value> {
    if let Variant::String(s) = v {
        Some(toml::Value::String(s.clone()))
    } else {
        None
    }
}

fn f32_pair(a: f32, b: f32) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(a as f64),
        toml::Value::Float(b as f64),
    ])
}

fn f32_triple(a: f32, b: f32, c: f32) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(a as f64),
        toml::Value::Float(b as f64),
        toml::Value::Float(c as f64),
    ])
}

/// Roblox `FontFace` (a `Variant::Font`) → the engine's `[text].font` string,
/// best-effort: the family file basename (`rbxasset://fonts/families/Arial.json`
/// → `Arial`). Unknown names fall back to the template default at load.
fn gui_font_name(v: &Variant) -> Option<toml::Value> {
    if let Variant::Font(f) = v {
        let fam = f
            .family
            .rsplit('/')
            .next()
            .unwrap_or(f.family.as_str())
            .trim_end_matches(".json");
        if fam.is_empty() {
            None
        } else {
            Some(toml::Value::String(fam.to_string()))
        }
    } else {
        None
    }
}

/// Route a Roblox GUI property into the PropertyBag's `section_props` bucket so
/// it lands in the dedicated top-level TOML section the engine's `gui_loader`
/// reads (`[gui]` layout/appearance, `[text]` for the TextLabel family), not
/// the ignored `[properties.extras]`. Returns `true` when the property was
/// recognised and consumed.
///
/// Roblox keeps every GuiObject property on the instance; before this they ALL
/// fell to `[properties.extras]` and were dropped, so imported GUIs rendered
/// with template defaults ("Label", black text, opaque box). Conversions:
/// `Color3` 0..1 → `[r,g,b]` 0-255; `UDim2` → `*_scale` + `*_offset` pairs;
/// enum ordinal → the engine's string label.
fn try_gui_property(
    bag: &mut PropertyBag,
    target_class: ClassName,
    key: &str,
    variant: &Variant,
) -> bool {
    if !is_gui_class(target_class) {
        return false;
    }
    let is_text = matches!(
        target_class,
        ClassName::TextLabel | ClassName::TextButton | ClassName::TextBox
    );
    let is_billboard = matches!(target_class, ClassName::BillboardGui);
    let is_container = matches!(
        target_class,
        ClassName::BillboardGui | ClassName::SurfaceGui | ClassName::ScreenGui
    );

    // `Size` / `Position` are UDim2 → two keys (scale + offset) under [gui].
    if key == "Position" || key == "Size" {
        if let Variant::UDim2(u) = variant {
            let (scale_key, offset_key) = if key == "Position" {
                ("position_scale", "position_offset")
            } else {
                ("size_scale", "size_offset")
            };
            let g = bag.section_props.entry("gui".to_string()).or_default();
            g.insert(scale_key.to_string(), f32_pair(u.x.scale, u.y.scale));
            g.insert(
                offset_key.to_string(),
                f32_pair(u.x.offset as f32, u.y.offset as f32),
            );
            return true;
        }
    }

    // Single-key props → (section, key, value).
    let mapped: Option<(&str, &str, toml::Value)> = match key {
        // ── common GuiObject → [gui] ──
        "Visible" => val_bool(variant).map(|v| ("gui", "visible", v)),
        "Active" => val_bool(variant).map(|v| ("gui", "active", v)),
        "ClipsDescendants" => val_bool(variant).map(|v| ("gui", "clips_descendants", v)),
        // GUI colors are 0..1 floats — the billboard/Slint renderer multiplies
        // them by 255. (Unlike a BasePart's BrickColor/Color, which the color
        // wheel keeps as the 0..255 sRGB spectrum.) Using `color_u8` here would
        // hand the rasterizer 255.0 and saturate every channel to white.
        "BackgroundColor3" => color_f32(variant).map(|v| ("gui", "background_color", v)),
        "BackgroundTransparency" => {
            val_float(variant).map(|v| ("gui", "background_transparency", v))
        }
        "BorderColor3" => color_f32(variant).map(|v| ("gui", "border_color", v)),
        "BorderSizePixel" => val_int(variant).map(|v| ("gui", "border_size_pixel", v)),
        "BorderMode" => enum_u32(variant).map(|e| {
            let s = if e == 1 { "Middle" } else { "Outline" };
            ("gui", "border_mode", toml::Value::String(s.to_string()))
        }),
        "LayoutOrder" => val_int(variant).map(|v| ("gui", "layout_order", v)),
        "Rotation" => val_float(variant).map(|v| ("gui", "rotation", v)),
        "AnchorPoint" => match variant {
            Variant::Vector2(p) => Some(("gui", "anchor_point", f32_pair(p.x, p.y))),
            _ => None,
        },
        "AutomaticSize" => enum_u32(variant).map(|e| {
            let s = match e {
                1 => "X",
                2 => "Y",
                3 => "XY",
                _ => "None",
            };
            ("gui", "automatic_size", toml::Value::String(s.to_string()))
        }),
        "ZIndex" => val_int(variant).map(|v| ("gui", "z_index", v)),

        // ── BillboardGui / container → [gui] ──
        "AlwaysOnTop" if is_billboard => val_bool(variant).map(|v| ("gui", "always_on_top", v)),
        "StudsOffset" if is_billboard => match variant {
            Variant::Vector3(p) => Some(("gui", "studs_offset", f32_triple(p.x, p.y, p.z))),
            _ => None,
        },
        "MaxDistance" if is_billboard => val_float(variant).map(|v| ("gui", "max_distance", v)),
        "LightInfluence" if is_billboard => {
            val_float(variant).map(|v| ("gui", "light_influence", v))
        }
        "Enabled" if is_container => val_bool(variant).map(|v| ("gui", "enabled", v)),
        "ZIndexBehavior" if is_container => enum_u32(variant).map(|e| {
            let s = if e == 0 { "Global" } else { "Sibling" };
            ("gui", "z_index_behavior", toml::Value::String(s.to_string()))
        }),

        // ── text family → [text] ──
        "Text" if is_text => val_string(variant).map(|v| ("text", "text", v)),
        "TextColor3" if is_text => color_f32(variant).map(|v| ("text", "text_color", v)),
        "TextTransparency" if is_text => {
            val_float(variant).map(|v| ("text", "text_transparency", v))
        }
        "TextStrokeColor3" if is_text => {
            color_f32(variant).map(|v| ("text", "text_stroke_color", v))
        }
        "TextStrokeTransparency" if is_text => {
            val_float(variant).map(|v| ("text", "text_stroke_transparency", v))
        }
        "TextSize" if is_text => val_float(variant).map(|v| ("text", "font_size", v)),
        "TextScaled" if is_text => val_bool(variant).map(|v| ("text", "text_scaled", v)),
        "TextWrapped" if is_text => val_bool(variant).map(|v| ("text", "text_wrapped", v)),
        "RichText" if is_text => val_bool(variant).map(|v| ("text", "rich_text", v)),
        "LineHeight" if is_text => val_float(variant).map(|v| ("text", "line_height", v)),
        "TextXAlignment" if is_text => enum_u32(variant).map(|e| {
            let s = match e {
                0 => "Left",
                1 => "Right",
                _ => "Center",
            };
            ("text", "text_x_alignment", toml::Value::String(s.to_string()))
        }),
        "TextYAlignment" if is_text => enum_u32(variant).map(|e| {
            let s = match e {
                0 => "Top",
                2 => "Bottom",
                _ => "Center",
            };
            ("text", "text_y_alignment", toml::Value::String(s.to_string()))
        }),
        "FontFace" if is_text => gui_font_name(variant).map(|v| ("text", "font", v)),

        _ => None,
    };

    if let Some((section, k, v)) = mapped {
        bag.section_props
            .entry(section.to_string())
            .or_default()
            .insert(k.to_string(), v);
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Shared section helpers (used by the per-family try_* fns below)
// ---------------------------------------------------------------------------

/// Roblox `Color3` / `Color3uint8` -> a `[f32; 3]` 0..1 FLOAT toml array — the
/// form `[color_grading_effect].tint_color` uses (template is `[1.0,1.0,1.0]`,
/// not u8). NEW helper (no prior cognate).
fn color_f32(variant: &Variant) -> Option<toml::Value> {
    let (r, g, b) = match variant {
        Variant::Color3(c) => (c.r as f64, c.g as f64, c.b as f64),
        Variant::Color3uint8(c) => (c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0),
        _ => return None,
    };
    Some(toml::Value::Array(vec![
        toml::Value::Float(r),
        toml::Value::Float(g),
        toml::Value::Float(b),
    ]))
}

/// Insert one `key = value` into the named top-level TOML `[section]` of the
/// bag's `section_props`. The materializer merges `section_props[section]` onto
/// the class template's existing `[section]` table, so unset template keys keep
/// their defaults. This is the same merge the GUI fix relies on.
fn put_section(bag: &mut PropertyBag, section: &str, key: &str, value: toml::Value) {
    bag.section_props
        .entry(section.to_string())
        .or_default()
        .insert(key.to_string(), value);
}

/// UDim2 -> the single 4-float `[scale_x, offset_x, scale_y, offset_y]` array
/// the loader's UDim2 `position`/`size` field deserializes. NEW helper used by
/// the GUI-leaf Position/Size fix.
fn udim2_to_section_value(u: &UDim2) -> toml::Value {
    toml::Value::Array(vec![
        toml::Value::Float(u.x.scale as f64),
        toml::Value::Float(u.x.offset as f64),
        toml::Value::Float(u.y.scale as f64),
        toml::Value::Float(u.y.offset as f64),
    ])
}

// ---------------------------------------------------------------------------
// Per-family property routers (Wave-3 section mappings)
// ---------------------------------------------------------------------------

/// Atmosphere / Sky / Clouds / ColorGradingEffect / DirectionalLight ->
/// `[atmosphere]`/`[sky]`/`[clouds]`/`[color_grading_effect]`/`[light]`.
/// MUST run before try_well_known so `Color` lands in the class color slot,
/// not as a BasePart color_rgba. Asset refs (Skybox*, Sun/MoonTextureId) are
/// routed upstream by is_asset_property_name.
fn try_env_property(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) -> bool {
    let mapped: Option<(&str, &str, toml::Value)> = match target_class {
        ClassName::Atmosphere => match key {
            "Density" => val_float(variant).map(|v| ("atmosphere", "density", v)),
            "Offset" => val_float(variant).map(|v| ("atmosphere", "offset", v)),
            "Color" => color_u8(variant).map(|v| ("atmosphere", "color", v)),
            "Decay" => color_u8(variant).map(|v| ("atmosphere", "decay_color", v)),
            "Glare" => val_float(variant).map(|v| ("atmosphere", "glare", v)),
            "Haze" => val_float(variant).map(|v| ("atmosphere", "haze", v)),
            _ => None,
        },
        ClassName::Sky => match key {
            "CelestialBodiesShown" => val_bool(variant).map(|v| ("sky", "celestial_bodies_shown", v)),
            "StarCount" => val_int(variant).map(|v| ("sky", "star_count", v)),
            "SunAngularSize" => val_float(variant).map(|v| ("sky", "sun_angular_size", v)),
            "MoonAngularSize" => val_float(variant).map(|v| ("sky", "moon_angular_size", v)),
            _ => None,
        },
        ClassName::Clouds => match key {
            "Enabled" => val_bool(variant).map(|v| ("clouds", "enabled", v)),
            "Cover" => val_float(variant).map(|v| ("clouds", "cover", v)),
            "Density" => val_float(variant).map(|v| ("clouds", "density", v)),
            "Color" => color_u8(variant).map(|v| ("clouds", "color", v)),
            _ => None,
        },
        ClassName::ColorGradingEffect => match key {
            "Enabled" => val_bool(variant).map(|v| ("color_grading_effect", "enabled", v)),
            "Brightness" => val_float(variant).map(|v| ("color_grading_effect", "brightness", v)),
            "Contrast" => val_float(variant).map(|v| ("color_grading_effect", "contrast", v)),
            "Saturation" => val_float(variant).map(|v| ("color_grading_effect", "saturation", v)),
            "TintColor" => color_f32(variant).map(|v| ("color_grading_effect", "tint_color", v)),
            "Tonemapper" => val_string(variant).map(|v| ("color_grading_effect", "tonemapper", v)),
            _ => None,
        },
        ClassName::DirectionalLight => match key {
            // Do NOT apply the x800 lumens scale used for Point/Spot/Surface;
            // directional brightness is consumed raw (illuminance = b x 10000).
            "Brightness" => val_float(variant).map(|v| ("light", "brightness", v)),
            "Color" => color_u8(variant).map(|v| ("light", "color", v)),
            "Shadows" => val_bool(variant).map(|v| ("light", "shadows", v)),
            "Enabled" => val_bool(variant).map(|v| ("light", "enabled", v)),
            _ => None,
        },
        _ => return false,
    };
    if let Some((section, k, v)) = mapped {
        put_section(bag, section, k, v);
        return true;
    }
    false
}

/// Sound -> `[sound]`. Double-writes the spawner-read keys (pitch /
/// roll_off_* / sound_group) AND the template keys (playback_speed /
/// rolloff_* / time_position) so the on-disk file is template-shaped AND the
/// only live reader (SoundSpawner::import_from_toml, binary-ECS path) works.
fn try_sound_property(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) -> bool {
    if !matches!(target_class, ClassName::Sound) {
        return false;
    }
    match key {
        "Volume" => { if let Some(v) = val_float(variant) { put_section(bag, "sound", "volume", v); return true; } }
        "Looped" => { if let Some(v) = val_bool(variant) { put_section(bag, "sound", "looped", v); return true; } }
        "Playing" => { if let Some(v) = val_bool(variant) { put_section(bag, "sound", "playing", v); return true; } }
        "PlaybackSpeed" => {
            if let Some(v) = val_float(variant) {
                put_section(bag, "sound", "pitch", v.clone());
                put_section(bag, "sound", "playback_speed", v);
                return true;
            }
        }
        "Pitch" => {
            // Deprecated alias; only fill if PlaybackSpeed hasn't already.
            let already = bag.section_props.get("sound").map(|s| s.contains_key("pitch")).unwrap_or(false);
            if !already {
                if let Some(v) = val_float(variant) {
                    put_section(bag, "sound", "pitch", v.clone());
                    put_section(bag, "sound", "playback_speed", v);
                    return true;
                }
            } else {
                return true; // PlaybackSpeed already won
            }
        }
        "TimePosition" => { if let Some(v) = val_float(variant) { put_section(bag, "sound", "time_position", v); return true; } }
        "RollOffMinDistance" => {
            if let Some(v) = val_float(variant) {
                put_section(bag, "sound", "roll_off_min_distance", v.clone());
                put_section(bag, "sound", "rolloff_min_distance", v);
                return true;
            }
        }
        "RollOffMaxDistance" => {
            if let Some(v) = val_float(variant) {
                put_section(bag, "sound", "roll_off_max_distance", v.clone());
                put_section(bag, "sound", "rolloff_max_distance", v);
                return true;
            }
        }
        "RollOffMode" => {
            let label: Option<&'static str> = match variant {
                Variant::Enum(_) | Variant::Int32(_) => enum_u32(variant).map(|o| rolloff_mode_ordinal_to_label(o, &mut bag.approximation_notes)),
                Variant::String(s) => Some(normalise_rolloff_label(s.as_str())),
                _ => None,
            };
            if let Some(l) = label {
                let val = toml::Value::String(l.to_string());
                put_section(bag, "sound", "roll_off_mode", val.clone());
                put_section(bag, "sound", "rolloff_mode", val);
                return true;
            }
        }
        _ => {}
    }
    false
}

/// Roblox RollOffMode ordinal -> engine SoundRolloffMode DebugName label.
/// Roblox: 0=Inverse,1=Linear,2=InverseTapered,3=LinearSquare. The latter two
/// have no engine cognate -> approximated to Inverse/Linear.
fn rolloff_mode_ordinal_to_label(ordinal: u32, notes: &mut Vec<String>) -> &'static str {
    match ordinal {
        0 => "Inverse",
        1 => "Linear",
        2 => { notes.push("Sound.RollOffMode InverseTapered -> Inverse (no engine cognate)".into()); "Inverse" }
        3 => { notes.push("Sound.RollOffMode LinearSquare -> Linear (no engine cognate)".into()); "Linear" }
        other => { notes.push(format!("Sound.RollOffMode ordinal {other} unknown -> Inverse")); "Inverse" }
    }
}

/// String-valued RollOffMode normalised to an engine SoundRolloffMode label.
fn normalise_rolloff_label(s: &str) -> &'static str {
    match s {
        "Linear" => "Linear",
        "InverseSquared" => "InverseSquared",
        "Logarithmic" => "Logarithmic",
        "None" => "None",
        "Custom" => "Custom",
        _ => "Inverse", // Inverse / InverseTapered / LinearSquare / unknown
    }
}

/// ParticleEmitter -> `[particle]` (template section). Collapses Roblox
/// sequences/ranges to the scalar template slots (first keypoint for *Sequence,
/// min/max split for NumberRange) and logs the approximation.
fn try_particle_property(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) -> bool {
    if !matches!(target_class, ClassName::ParticleEmitter) {
        return false;
    }
    let mapped: Option<(&str, toml::Value)> = match key {
        "Enabled" => val_bool(variant).map(|v| ("enabled", v)),
        "Rate" => val_float(variant).map(|v| ("rate", v)),
        "Drag" => val_float(variant).map(|v| ("drag", v)),
        "LightEmission" => val_float(variant).map(|v| ("light_emission", v)),
        "LightInfluence" => val_float(variant).map(|v| ("light_influence", v)),
        "ZOffset" => val_float(variant).map(|v| ("z_offset", v)),
        "SpreadAngle" => match variant {
            Variant::Vector2(p) => {
                if (p.y - p.x).abs() > f32::EPSILON {
                    bag.approximation_notes.push(format!("ParticleEmitter.SpreadAngle Vector2({},{}) collapsed to scalar {} (template spread_angle is scalar)", p.x, p.y, p.x));
                }
                Some(("spread_angle", toml::Value::Float(p.x as f64)))
            }
            Variant::Float32(f) => Some(("spread_angle", toml::Value::Float(*f as f64))),
            _ => None,
        },
        "EmissionDirection" => enum_u32(variant).map(|e| {
            let s = match e { 0 => "Right", 1 => "Top", 2 => "Back", 3 => "Left", 4 => "Bottom", 5 => "Front", _ => "Top" };
            ("emission_direction", toml::Value::String(s.to_string()))
        }),
        "Color" => match variant {
            Variant::ColorSequence(cs) => cs.keypoints.first().map(|k| {
                bag.approximation_notes.push("ParticleEmitter.Color ColorSequence collapsed to start-keypoint color".to_string());
                ("color", toml::Value::Array(vec![
                    toml::Value::Integer((k.color.r * 255.0).round().clamp(0.0, 255.0) as i64),
                    toml::Value::Integer((k.color.g * 255.0).round().clamp(0.0, 255.0) as i64),
                    toml::Value::Integer((k.color.b * 255.0).round().clamp(0.0, 255.0) as i64),
                ]))
            }),
            Variant::Color3(_) | Variant::Color3uint8(_) => color_u8(variant).map(|v| ("color", v)),
            _ => None,
        },
        "Size" => match variant {
            Variant::NumberSequence(ns) => ns.keypoints.first().map(|k| {
                bag.approximation_notes.push("ParticleEmitter.Size NumberSequence collapsed to start-keypoint value".to_string());
                ("size", toml::Value::Float(k.value as f64))
            }),
            Variant::Float32(f) => Some(("size", toml::Value::Float(*f as f64))),
            _ => None,
        },
        "Transparency" => match variant {
            Variant::NumberSequence(ns) => ns.keypoints.first().map(|k| {
                bag.approximation_notes.push("ParticleEmitter.Transparency NumberSequence collapsed to start-keypoint value".to_string());
                ("transparency", toml::Value::Float(k.value as f64))
            }),
            Variant::Float32(f) => Some(("transparency", toml::Value::Float(*f as f64))),
            _ => None,
        },
        _ => None,
    };
    if let Some((k, v)) = mapped {
        put_section(bag, "particle", k, v);
        return true;
    }
    // Range-valued props -> two scalar template keys.
    let range_split: Option<(&str, &str)> = match key {
        "Lifetime" => Some(("lifetime_min", "lifetime_max")),
        "Speed" => Some(("speed_min", "speed_max")),
        "RotSpeed" => Some(("rotation_speed_min", "rotation_speed_max")),
        _ => None,
    };
    if let Some((min_key, max_key)) = range_split {
        if let Variant::NumberRange(nr) = variant {
            put_section(bag, "particle", min_key, toml::Value::Float(nr.min as f64));
            put_section(bag, "particle", max_key, toml::Value::Float(nr.max as f64));
            return true;
        }
        if let Variant::Float32(f) = variant {
            put_section(bag, "particle", min_key, toml::Value::Float(*f as f64));
            put_section(bag, "particle", max_key, toml::Value::Float(*f as f64));
            return true;
        }
    }
    false
}

/// Beam -> `[beam]` (template read contract). Color (ColorSequence) and
/// Transparency (NumberSequence) collapse to the scalar template slots (first
/// keypoint) and the full gradient is preserved in [properties.extras] for
/// round-trip. Attachment0/1 are Refs (bag.refs); asset Texture is upstream.
fn try_beam_property(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) -> bool {
    if !matches!(target_class, ClassName::Beam) {
        return false;
    }
    if key == "Color" {
        if let Some(v) = beam_color_first_u8(variant) {
            put_section(bag, "beam", "color", v);
            if matches!(variant, Variant::ColorSequence(_)) {
                let raw = variant_to_toml(variant, bag);
                bag.properties_extras.insert("Color".to_string(), raw);
            }
            return true;
        }
    }
    if key == "Transparency" {
        if let Some(v) = beam_number_first_f32(variant) {
            put_section(bag, "beam", "transparency", v);
            if matches!(variant, Variant::NumberSequence(_)) {
                let raw = variant_to_toml(variant, bag);
                bag.properties_extras.insert("Transparency".to_string(), raw);
            }
            return true;
        }
    }
    let mapped: Option<(&str, toml::Value)> = match key {
        "Width0" => val_float(variant).map(|v| ("width0", v)),
        "Width1" => val_float(variant).map(|v| ("width1", v)),
        "CurveSize0" => val_float(variant).map(|v| ("curve_size0", v)),
        "CurveSize1" => val_float(variant).map(|v| ("curve_size1", v)),
        "Segments" => val_int(variant).map(|v| ("segments", v)),
        "ZOffset" => val_float(variant).map(|v| ("z_offset", v)),
        "FaceCamera" => val_bool(variant).map(|v| ("face_camera", v)),
        "Enabled" => val_bool(variant).map(|v| ("enabled", v)),
        "LightEmission" => val_float(variant).map(|v| ("light_emission", v)),
        "LightInfluence" => val_float(variant).map(|v| ("light_influence", v)),
        "TextureLength" => val_float(variant).map(|v| ("texture_length", v)),
        "TextureSpeed" => val_float(variant).map(|v| ("texture_speed", v)),
        "Texture" => val_string(variant).map(|v| ("texture", v)), // only plain (non-URI) strings reach here
        "Brightness" => val_float(variant).map(|v| ("brightness", v)),
        "TextureMode" => enum_u32(variant).map(|e| {
            let s = match e { 1 => "Stretch", 2 => "Static", _ => "Wrap" };
            ("texture_mode", toml::Value::String(s.to_string()))
        }),
        _ => None,
    };
    if let Some((k, v)) = mapped {
        put_section(bag, "beam", k, v);
        return true;
    }
    false
}

/// First ColorSequence keypoint (or bare Color3/Color3uint8) -> [beam].color
/// `[r,g,b]` 0-255. ColorSequence keypoint colors are Color3 floats 0..1.
fn beam_color_first_u8(variant: &Variant) -> Option<toml::Value> {
    match variant {
        Variant::ColorSequence(cs) => {
            let c = &cs.keypoints.first()?.color;
            Some(toml::Value::Array(vec![
                toml::Value::Integer(((c.r * 255.0).round() as i64).clamp(0, 255)),
                toml::Value::Integer(((c.g * 255.0).round() as i64).clamp(0, 255)),
                toml::Value::Integer(((c.b * 255.0).round() as i64).clamp(0, 255)),
            ]))
        }
        Variant::Color3(_) | Variant::Color3uint8(_) => color_u8(variant),
        _ => None,
    }
}

/// First NumberSequence keypoint value (or bare Float32) -> [beam].transparency
/// scalar (0=opaque). Roblox transparency sense matches the template.
fn beam_number_first_f32(variant: &Variant) -> Option<toml::Value> {
    match variant {
        Variant::NumberSequence(ns) => {
            let v = ns.keypoints.first()?.value;
            Some(toml::Value::Float((v as f64).clamp(0.0, 1.0)))
        }
        Variant::Float32(f) => Some(toml::Value::Float((*f as f64).clamp(0.0, 1.0))),
        _ => None,
    }
}

/// Decal -> `[decal]`, SpecialMesh -> `[mesh]` (template read contracts).
/// MUST run before try_well_known so Decal.Transparency/Color land in [decal],
/// not as a phantom part color override. Asset refs handled upstream.
fn try_decal_mesh_property(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) -> bool {
    match target_class {
        ClassName::Decal => {
            let mapped: Option<(&str, toml::Value)> = match key {
                "Color3" | "Color" => color_u8(variant).map(|v| ("color", v)),
                "Transparency" => val_float(variant).map(|v| ("transparency", v)),
                "Face" => match variant {
                    Variant::Enum(_) | Variant::Int32(_) => enum_u32(variant).map(|e| {
                        // Roblox NormalId: 0=Right,1=Top,2=Back,3=Left,4=Bottom,5=Front.
                        let s = match e { 0 => "Right", 1 => "Top", 2 => "Back", 3 => "Left", 4 => "Bottom", _ => "Front" };
                        ("face", toml::Value::String(s.to_string()))
                    }),
                    Variant::String(s) => Some(("face", toml::Value::String(s.clone()))),
                    _ => None,
                },
                "ZIndex" => val_int(variant).map(|v| ("z_index", v)),
                _ => None,
            };
            if let Some((k, v)) = mapped {
                put_section(bag, "decal", k, v);
                return true;
            }
            false
        }
        ClassName::SpecialMesh => {
            let mapped: Option<(&str, toml::Value)> = match key {
                "MeshType" => enum_u32(variant).map(|e| {
                    // rbx-700 MeshType: 0=Head 1=Torso 2=Wedge 3=Sphere 4=Cylinder 5=FileMesh 6=Brick.
                    let s = match e { 0 => "Head", 1 => "Torso", 3 => "Sphere", 4 => "Cylinder", 6 => "Brick", _ => "FileMesh" };
                    ("mesh_type", toml::Value::String(s.to_string()))
                }),
                "Scale" => match variant { Variant::Vector3(v) => Some(("scale", f32_triple(v.x, v.y, v.z))), _ => None },
                "Offset" => match variant { Variant::Vector3(v) => Some(("offset", f32_triple(v.x, v.y, v.z))), _ => None },
                "VertexColor" => match variant {
                    Variant::Vector3(v) => Some(("vertex_color", toml::Value::Array(vec![
                        toml::Value::Integer((v.x * 255.0).round().clamp(0.0, 255.0) as i64),
                        toml::Value::Integer((v.y * 255.0).round().clamp(0.0, 255.0) as i64),
                        toml::Value::Integer((v.z * 255.0).round().clamp(0.0, 255.0) as i64),
                    ]))),
                    _ => None,
                },
                _ => None,
            };
            if let Some((k, v)) = mapped {
                put_section(bag, "mesh", k, v);
                return true;
            }
            false
        }
        _ => false,
    }
}

/// Humanoid -> `[humanoid]` (snake_case template keys), Animator ->
/// `[properties]` (what AnimatorSpawner::import_from_toml reads).
fn try_character_property(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) -> bool {
    match target_class {
        ClassName::Humanoid => {
            let mapped: Option<(&str, toml::Value)> = match key {
                "Health" => val_float(variant).map(|v| ("health", v)),
                "MaxHealth" => val_float(variant).map(|v| ("max_health", v)),
                "WalkSpeed" => val_float(variant).map(|v| ("walk_speed", v)),
                "JumpPower" => val_float(variant).map(|v| ("jump_power", v)),
                "HipHeight" => val_float(variant).map(|v| ("hip_height", v)),
                "AutoRotate" => val_bool(variant).map(|v| ("auto_rotate", v)),
                "JumpHeight" => val_float(variant).map(|v| ("jump_height", v)),
                "MaxSlopeAngle" => val_float(variant).map(|v| ("max_slope_angle", v)),
                "AutoJumpEnabled" => val_bool(variant).map(|v| ("auto_jump_enabled", v)),
                "UseJumpPower" => val_bool(variant).map(|v| ("use_jump_power", v)),
                "DisplayName" => val_string(variant).map(|v| ("display_name", v)),
                "NameDisplayDistance" => val_float(variant).map(|v| ("name_display_distance", v)),
                "HealthDisplayDistance" => val_float(variant).map(|v| ("health_display_distance", v)),
                "DisplayDistanceType" => enum_u32(variant).map(|e| {
                    let s = match e { 0 => "Viewer", 1 => "Subject", _ => "None" };
                    ("display_distance_type", toml::Value::String(s.to_string()))
                }),
                "HealthDisplayType" => enum_u32(variant).map(|e| {
                    let s = match e { 1 => "AlwaysOn", 2 => "AlwaysOff", _ => "DisplayWhenDamaged" };
                    ("health_display_type", toml::Value::String(s.to_string()))
                }),
                "RigType" => match variant {
                    Variant::String(s) => Some(("rig_type", toml::Value::String(s.clone()))),
                    Variant::Enum(e) => Some(("rig_type", toml::Value::String(format!("RigType_{}", e.to_u32())))),
                    _ => None,
                },
                "HumanoidStateMachine" => val_bool(variant).map(|v| ("humanoid_state_machine", v)),
                "RequiresNeck" => val_bool(variant).map(|v| ("requires_neck", v)),
                "BreakJointsOnDeath" => val_bool(variant).map(|v| ("break_joints_on_death", v)),
                _ => None,
            };
            if let Some((k, v)) = mapped {
                put_section(bag, "humanoid", k, v);
                return true;
            }
            false
        }
        ClassName::Animator => {
            let mapped: Option<(&str, toml::Value)> = match key {
                "PreferredAnimationSpeed" => val_float(variant).map(|v| ("preferred_animation_speed", v)),
                "RigType" => match variant {
                    Variant::String(s) => Some(("rig_type", toml::Value::String(s.clone()))),
                    Variant::Enum(e) => Some(("rig_type", toml::Value::String(format!("RigType_{}", e.to_u32())))),
                    _ => None,
                },
                "EvaluationThrottled" => val_bool(variant).map(|v| ("evaluation_throttled", v)),
                "PreferLodEnabled" => val_bool(variant).map(|v| ("prefer_lod_enabled", v)),
                "RootMotionWeight" => val_float(variant).map(|v| ("root_motion_weight", v)),
                _ => None,
            };
            if let Some((k, v)) = mapped {
                put_section(bag, "properties", k, v);
                return true;
            }
            false
        }
        _ => false,
    }
}

/// Constraints + Attachment -> `[constraint]` / `[motor]` / `[attachment]`,
/// the sections the LIVE file-load path reads (physics/joint_resolver.rs reads
/// [constraint], NOT [properties]). Part0/Part1/Attachment0/Attachment1 are
/// Refs -> early-return false so apply_variant's Ref arm collects them.
fn try_constraint_property(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) -> bool {
    use ClassName::*;
    let is_joint = matches!(target_class, WeldConstraint | Motor6D | HingeConstraint | DistanceConstraint | SpringConstraint | RopeConstraint | PrismaticConstraint | BallSocketConstraint);
    let is_attachment = matches!(target_class, Attachment);
    if !is_joint && !is_attachment {
        return false;
    }
    // Refs (Part0/Part1/Attachment0/Attachment1) stay in bag.refs.
    if matches!(variant, Variant::Ref(_)) {
        return false;
    }

    // Attachment -> [attachment] (round-trip; no live reader). CFrame /
    // Position / Color are well-known overrides — leave to try_well_known.
    if is_attachment {
        match key {
            "Visible" => { if let Some(v) = val_bool(variant) { put_section(bag, "attachment", "visible", v); return true; } }
            "Axis" => { if let Variant::Vector3(a) = variant { put_section(bag, "attachment", "axis", f32_triple(a.x, a.y, a.z)); return true; } }
            "SecondaryAxis" => { if let Variant::Vector3(a) = variant { put_section(bag, "attachment", "secondary_axis", f32_triple(a.x, a.y, a.z)); return true; } }
            _ => {}
        }
        return false;
    }

    // Shared across all joint classes.
    match key {
        "Enabled" => { if let Some(v) = val_bool(variant) { put_section(bag, "constraint", "enabled", v); return true; } }
        "Visible" => { if let Some(v) = val_bool(variant) { put_section(bag, "constraint", "visible", v); return true; } }
        "Thickness" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "thickness", v); return true; } }
        "Color" => { if let Some(v) = color_u8(variant) { put_section(bag, "constraint", "color", v); return true; } }
        _ => {}
    }

    // Hinge / Motor6D angle limits (raw degrees; joint_resolver passthrough).
    if matches!(target_class, HingeConstraint | Motor6D) {
        match key {
            "LowerAngle" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "lower_angle", v); return true; } }
            "UpperAngle" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "upper_angle", v); return true; } }
            _ => {}
        }
    }
    // Hinge round-trip-only knobs.
    if matches!(target_class, HingeConstraint) {
        match key {
            "LimitsEnabled" => { if let Some(v) = val_bool(variant) { put_section(bag, "constraint", "limits_enabled", v); return true; } }
            "AngularVelocity" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "angular_velocity", v); return true; } }
            "ActuatorType" => { if let Some(e) = enum_u32(variant) { let s = match e { 1 => "Motor", 2 => "Servo", _ => "None" }; put_section(bag, "constraint", "actuator_type", toml::Value::String(s.to_string())); return true; } }
            _ => {}
        }
    }
    // DistanceConstraint -> [constraint].max_distance (the key the loader reads).
    if matches!(target_class, DistanceConstraint) {
        match key {
            "Length" | "MaxLength" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "max_distance", v); return true; } }
            "MinLength" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "min_length", v); return true; } }
            "LimitsEnabled" => { if let Some(v) = val_bool(variant) { put_section(bag, "constraint", "limits_enabled", v); return true; } }
            _ => {}
        }
    }
    // Motor6D round-trip-only knobs + C0/C1.
    if matches!(target_class, Motor6D) {
        match key {
            "MaxVelocity" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "max_velocity", v); return true; } }
            "DesiredAngle" => { if let Some(v) = val_float(variant) { put_section(bag, "motor", "target_velocity", v); return true; } }
            "MaxTorque" => { if let Some(v) = val_float(variant) { put_section(bag, "motor", "max_torque", v); return true; } }
            _ => {}
        }
    }
    if matches!(target_class, Motor6D | WeldConstraint) {
        match key {
            "C0" => { if let Variant::CFrame(cf) = variant { put_section(bag, "constraint", "c0", f32_triple(cf.position.x, cf.position.y, cf.position.z)); return true; } }
            "C1" => { if let Variant::CFrame(cf) = variant { put_section(bag, "constraint", "c1", f32_triple(cf.position.x, cf.position.y, cf.position.z)); return true; } }
            _ => {}
        }
    }
    // SpringConstraint.
    if matches!(target_class, SpringConstraint) {
        match key {
            "Stiffness" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "stiffness", v); return true; } }
            "Damping" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "damping", v); return true; } }
            "FreeLength" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "free_length", v.clone()); put_section(bag, "constraint", "rest_length", v); return true; } }
            "MaxLength" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "max_length", v); put_section(bag, "constraint", "limits_enabled", toml::Value::Boolean(true)); return true; } }
            "MinLength" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "min_length", v); put_section(bag, "constraint", "limits_enabled", toml::Value::Boolean(true)); return true; } }
            "LimitsEnabled" => { if let Some(v) = val_bool(variant) { put_section(bag, "constraint", "limits_enabled", v); return true; } }
            "Coils" => { if let Some(v) = val_int(variant) { put_section(bag, "constraint", "coils", v); return true; } }
            _ => {}
        }
    }
    // RopeConstraint.
    if matches!(target_class, RopeConstraint) {
        match key {
            "Length" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "length", v); return true; } }
            "Restitution" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "restitution", v); return true; } }
            _ => {}
        }
    }
    // PrismaticConstraint (+ [motor]).
    if matches!(target_class, PrismaticConstraint) {
        match key {
            "LowerLimit" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "lower_limit", v); return true; } }
            "UpperLimit" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "upper_limit", v); return true; } }
            "LimitsEnabled" => { if let Some(v) = val_bool(variant) { put_section(bag, "constraint", "limits_enabled", v); return true; } }
            "Velocity" => { if let Some(v) = val_float(variant) { put_section(bag, "motor", "target_velocity", v); return true; } }
            "MotorMaxForce" => { if let Some(v) = val_float(variant) { put_section(bag, "motor", "max_force", v); return true; } }
            "ActuatorType" => { if let Some(e) = enum_u32(variant) { let model = match e { 2 => "ForceBased", _ => "AccelerationBased" }; put_section(bag, "motor", "model", toml::Value::String(model.to_string())); return true; } }
            _ => {}
        }
    }
    // BallSocketConstraint.
    if matches!(target_class, BallSocketConstraint) {
        match key {
            "UpperAngle" => { if let Some(v) = val_float(variant) { put_section(bag, "constraint", "upper_angle", v); bag.approximation_notes.push("BallSocketConstraint.UpperAngle written as degrees to [constraint].upper_angle; engine cone_angle is radians (deg->rad unresolved, loader does not read it yet)".to_string()); return true; } }
            "LimitsEnabled" => { if let Some(v) = val_bool(variant) { put_section(bag, "constraint", "limits_enabled", v); return true; } }
            _ => {}
        }
    }
    false
}

/// SpawnLocation / Seat / VehicleSeat / Team behavioral props ->
/// `[spawn]`/`[seat]`/`[vehicle]`/`[team]` (template sections). Matches ONLY
/// the behavioral keys; returns false for BasePart visual props so they flow
/// to try_well_known.
fn try_spawn_seat_vehicle_team_property(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) -> bool {
    let mapped: Option<(&str, &str, toml::Value)> = match target_class {
        ClassName::SpawnLocation => match key {
            "Enabled" => val_bool(variant).map(|v| ("spawn", "enabled", v)),
            "Neutral" => val_bool(variant).map(|v| ("spawn", "neutral", v)),
            "AllowTeamChangeOnTouch" => val_bool(variant).map(|v| ("spawn", "allow_team_change_on_touch", v)),
            "Duration" => val_int(variant).map(|v| ("spawn", "duration", v)),
            "TeamColor" => match variant {
                Variant::BrickColor(bc) => Some(("spawn", "team_color", toml::Value::String(bc.to_string()))),
                Variant::String(s) => Some(("spawn", "team_color", toml::Value::String(s.clone()))),
                _ => None,
            },
            _ => None,
        },
        ClassName::Seat => match key {
            "Disabled" => val_bool(variant).map(|v| ("seat", "disabled", v)),
            _ => None,
        },
        ClassName::VehicleSeat => match key {
            "Disabled" => val_bool(variant).map(|v| ("vehicle", "disabled", v)),
            "MaxSpeed" => val_float(variant).map(|v| ("vehicle", "max_speed", v)),
            "Torque" => val_float(variant).map(|v| ("vehicle", "torque", v)),
            "TurnSpeed" => val_float(variant).map(|v| ("vehicle", "turn_speed", v)),
            "Steer" => val_int(variant).map(|v| ("vehicle", "steer", v)),
            "Throttle" => val_int(variant).map(|v| ("vehicle", "throttle", v)),
            _ => None,
        },
        ClassName::Team => match key {
            "AutoAssignable" => val_bool(variant).map(|v| ("team", "auto_assignable", v)),
            "AutoColorCharacters" => val_bool(variant).map(|v| ("team", "auto_color_characters", v)),
            "TeamColor" => match variant {
                Variant::BrickColor(bc) => {
                    let c = bc.to_color3uint8();
                    Some(("team", "team_color", toml::Value::Array(vec![
                        toml::Value::Integer(c.r as i64),
                        toml::Value::Integer(c.g as i64),
                        toml::Value::Integer(c.b as i64),
                    ])))
                }
                Variant::Color3(_) | Variant::Color3uint8(_) => color_u8(variant).map(|v| ("team", "team_color", v)),
                _ => None,
            },
            _ => None,
        },
        _ => return false,
    };
    if let Some((section, k, v)) = mapped {
        put_section(bag, section, k, v);
        return true;
    }
    false
}

/// Camera -> `[camera]`, Model -> `[model]`, WorldModel -> `[world_model]`.
/// Refs (PrimaryPart/CameraSubject/Focus) early-return false -> bag.refs.
/// CFrame is left to try_well_known (pose).
fn try_camera_model_worldmodel_property(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) -> bool {
    let is_camera = matches!(target_class, ClassName::Camera);
    let is_model = matches!(target_class, ClassName::Model);
    let is_world_model = matches!(target_class, ClassName::WorldModel);
    if !(is_camera || is_model || is_world_model) {
        return false;
    }
    // Archivable -> [metadata].archivable (all three).
    if key == "Archivable" {
        if let Variant::Bool(b) = variant {
            bag.metadata_extras.insert("archivable".to_string(), toml::Value::Boolean(*b));
            return true;
        }
    }
    // Refs handled upstream; never claim PrimaryPart/CameraSubject/Focus here.
    if matches!(variant, Variant::Ref(_)) {
        return false;
    }
    let mapped: Option<(&str, &str, toml::Value)> = match (target_class, key) {
        (ClassName::Camera, "FieldOfView") => val_float(variant).map(|v| ("camera", "field_of_view", v)),
        (ClassName::Camera, "DiagonalFieldOfView") => val_float(variant).map(|v| ("camera", "diagonal_field_of_view", v)),
        (ClassName::Camera, "FieldOfViewMode") => enum_u32(variant).map(|e| {
            let s = match e { 1 => "Diagonal", 2 => "MaxAxis", _ => "Vertical" };
            ("camera", "field_of_view_mode", toml::Value::String(s.to_string()))
        }),
        (ClassName::Camera, "CameraType") => match variant {
            Variant::Enum(e) => {
                let s = match e.to_u32() { 0 => "Fixed", 1 => "Attach", 2 => "Watch", 3 => "Track", 4 => "Follow", 6 => "Scriptable", 7 => "Orbital", _ => "Custom" };
                Some(("camera", "camera_type", toml::Value::String(s.to_string())))
            }
            Variant::String(s) => Some(("camera", "camera_type", toml::Value::String(s.clone()))),
            _ => None,
        },
        (ClassName::Camera, "HeadLocked") => val_bool(variant).map(|v| ("camera", "head_locked", v)),
        (ClassName::Camera, "HeadScale") => val_float(variant).map(|v| ("camera", "head_scale", v)),
        (ClassName::Camera, "NearPlaneZ") | (ClassName::Camera, "NearClip") => val_float(variant).map(|v| ("camera", "near_plane_z", v)),
        (ClassName::Camera, "FarPlaneZ") | (ClassName::Camera, "FarClip") => val_float(variant).map(|v| ("camera", "far_plane_z", v)),
        (ClassName::Model, "ModelStreamingMode") => enum_u32(variant).map(|e| {
            let s = match e { 1 => "Persistent", 2 => "PersistentPerPlayer", 3 => "Streamed", _ => "Default" };
            ("model", "model_streaming_mode", toml::Value::String(s.to_string()))
        }),
        (ClassName::Model, "LevelOfDetail") => enum_u32(variant).map(|e| {
            let s = match e { 1 => "StreamingMesh", 2 => "Disabled", _ => "Automatic" };
            ("model", "level_of_detail", toml::Value::String(s.to_string()))
        }),
        (ClassName::Model, "Scale") => val_float(variant).map(|v| ("model", "scale", v)),
        (ClassName::WorldModel, "IsolatedPhysics") => val_bool(variant).map(|v| ("world_model", "isolated_physics", v)),
        _ => None,
    };
    if let Some((section, k, v)) = mapped {
        put_section(bag, section, k, v);
        return true;
    }
    // Model.WorldPivot -> [model].world_pivot table {position,rotation,scale}.
    if is_model && key == "WorldPivot" {
        if let Variant::CFrame(cf) = variant {
            let (t, q) = cframe_to_translation_quat(cf);
            let mut pivot = toml::value::Table::new();
            pivot.insert("position".to_string(), f32_triple(t[0], t[1], t[2]));
            pivot.insert("rotation".to_string(), toml::Value::Array(vec![
                toml::Value::Float(q[0] as f64), toml::Value::Float(q[1] as f64),
                toml::Value::Float(q[2] as f64), toml::Value::Float(q[3] as f64),
            ]));
            pivot.insert("scale".to_string(), f32_triple(1.0, 1.0, 1.0));
            put_section(bag, "model", "world_pivot", toml::Value::Table(pivot));
            return true;
        }
    }
    false
}

/// ImageLabel / ImageButton / ScrollingFrame / ViewportFrame. Runs BEFORE
/// try_gui_property so leaf-specific keys + the corrected Position/Size single
/// 4-tuple + border_size are handled; shared GuiObject keys it doesn't match
/// fall through to try_gui_property.
fn try_gui_leaf_property(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) -> bool {
    let is_image = matches!(target_class, ClassName::ImageLabel | ClassName::ImageButton);
    let is_image_button = matches!(target_class, ClassName::ImageButton);
    let is_scroll = matches!(target_class, ClassName::ScrollingFrame);
    let is_viewport = matches!(target_class, ClassName::ViewportFrame);
    if !(is_image || is_scroll || is_viewport) {
        return false;
    }
    // Position / Size: canonical single 4-tuple (loader reads) + legacy split.
    if key == "Position" || key == "Size" {
        if let Variant::UDim2(u) = variant {
            let (single, scale_key, offset_key) = if key == "Position" {
                ("position", "position_scale", "position_offset")
            } else {
                ("size", "size_scale", "size_offset")
            };
            put_section(bag, "gui", single, udim2_to_section_value(u));
            put_section(bag, "gui", scale_key, f32_pair(u.x.scale, u.y.scale));
            put_section(bag, "gui", offset_key, f32_pair(u.x.offset as f32, u.y.offset as f32));
            return true;
        }
    }
    // Shared [gui] keys (corrected: border_size as f32).
    match key {
        "Visible" => { if let Some(v) = val_bool(variant) { put_section(bag, "gui", "visible", v); return true; } }
        "BackgroundColor3" => { if let Some(v) = color_u8(variant) { put_section(bag, "gui", "background_color", v); return true; } }
        "BorderColor3" => { if let Some(v) = color_u8(variant) { put_section(bag, "gui", "border_color", v); return true; } }
        "BorderSizePixel" => { if let Some(toml::Value::Integer(i)) = val_int(variant) { put_section(bag, "gui", "border_size", toml::Value::Float(i as f64)); return true; } }
        "AnchorPoint" => { if let Variant::Vector2(p) = variant { put_section(bag, "gui", "anchor_point", f32_pair(p.x, p.y)); return true; } }
        "ZIndex" => { if let Some(v) = val_int(variant) { put_section(bag, "gui", "z_index", v); return true; } }
        "BackgroundTransparency" => { if let Some(v) = val_float(variant) { put_section(bag, "gui", "background_transparency", v); return true; } }
        "Active" => { if let Some(v) = val_bool(variant) { put_section(bag, "gui", "active", v); return true; } }
        "ClipsDescendants" => { if let Some(v) = val_bool(variant) { put_section(bag, "gui", "clips_descendants", v); return true; } }
        "LayoutOrder" => { if let Some(v) = val_int(variant) { put_section(bag, "gui", "layout_order", v); return true; } }
        "Rotation" => { if let Some(v) = val_float(variant) { put_section(bag, "gui", "rotation", v); return true; } }
        "AutomaticSize" => { if let Some(e) = enum_u32(variant) { let s = match e { 1 => "X", 2 => "Y", 3 => "XY", _ => "None" }; put_section(bag, "gui", "automatic_size", toml::Value::String(s.to_string())); return true; } }
        _ => {}
    }
    // ImageLabel / ImageButton -> [image] (dropped by loader today).
    if is_image {
        match key {
            "ImageColor3" => { if let Some(v) = color_u8(variant) { put_section(bag, "image", "image_color", v); return true; } }
            "ImageTransparency" => { if let Some(v) = val_float(variant) { put_section(bag, "image", "image_transparency", v); return true; } }
            "ScaleType" => { if let Some(e) = enum_u32(variant) { let s = match e { 1 => "Slice", 2 => "Tile", 3 => "Fit", 4 => "Crop", _ => "Stretch" }; put_section(bag, "image", "scale_type", toml::Value::String(s.to_string())); return true; } }
            "SliceCenter" => { if let Variant::Rect(r) = variant { put_section(bag, "image", "slice_center", toml::Value::Array(vec![toml::Value::Float(r.min.x as f64), toml::Value::Float(r.min.y as f64), toml::Value::Float(r.max.x as f64), toml::Value::Float(r.max.y as f64)])); return true; } }
            "SliceScale" => { if let Some(v) = val_float(variant) { put_section(bag, "image", "slice_scale", v); return true; } }
            "TileSize" => {
                if let Variant::UDim2(u) = variant { put_section(bag, "image", "tile_size", f32_pair(u.x.offset as f32, u.y.offset as f32)); return true; }
                if let Variant::Vector2(p) = variant { put_section(bag, "image", "tile_size", f32_pair(p.x, p.y)); return true; }
            }
            _ => {}
        }
        if is_image_button {
            match key {
                "AutoButtonColor" => { if let Some(v) = val_bool(variant) { put_section(bag, "gui", "auto_button_color", v); return true; } }
                "HoverImage" => { if let Some(v) = val_string(variant) { put_section(bag, "image", "hover_image", v); return true; } }
                "PressedImage" => { if let Some(v) = val_string(variant) { put_section(bag, "image", "pressed_image", v); return true; } }
                _ => {}
            }
        }
    }
    // ScrollingFrame -> [scrolling] (dropped today).
    if is_scroll {
        match key {
            "ScrollingEnabled" => { if let Some(v) = val_bool(variant) { put_section(bag, "scrolling", "scrolling_enabled", v); return true; } }
            "ScrollBarThickness" => { if let Some(v) = val_int(variant) { put_section(bag, "scrolling", "scroll_bar_thickness", v); return true; } }
            "CanvasSize" => { if let Variant::UDim2(u) = variant { put_section(bag, "scrolling", "canvas_size", f32_pair(u.x.offset as f32, u.y.offset as f32)); return true; } }
            "CanvasPosition" => { if let Variant::Vector2(p) = variant { put_section(bag, "scrolling", "canvas_position", f32_pair(p.x, p.y)); return true; } }
            "ScrollingDirection" => { if let Some(e) = enum_u32(variant) { let s = match e { 1 => "X", 2 => "Y", _ => "XY" }; put_section(bag, "scrolling", "scrolling_direction", toml::Value::String(s.to_string())); return true; } }
            "ScrollBarImageColor3" => { if let Some(v) = color_u8(variant) { put_section(bag, "scrolling", "scroll_bar_image_color", v); return true; } }
            "ScrollBarImageTransparency" => { if let Some(v) = val_float(variant) { put_section(bag, "scrolling", "scroll_bar_image_transparency", v); return true; } }
            "ElasticBehavior" => { if let Some(e) = enum_u32(variant) { let s = match e { 1 => "Always", 2 => "WhenScrollable", _ => "Never" }; put_section(bag, "scrolling", "elastic_behavior", toml::Value::String(s.to_string())); return true; } }
            "TopImage" => { if let Some(v) = val_string(variant) { put_section(bag, "scrolling", "top_image", v); return true; } }
            "MidImage" => { if let Some(v) = val_string(variant) { put_section(bag, "scrolling", "mid_image", v); return true; } }
            "BottomImage" => { if let Some(v) = val_string(variant) { put_section(bag, "scrolling", "bottom_image", v); return true; } }
            _ => {}
        }
    }
    // ViewportFrame -> [viewport] (dropped today). CurrentCamera is a Ref.
    if is_viewport {
        match key {
            "Ambient" => { if let Some(v) = color_u8(variant) { put_section(bag, "viewport", "ambient", v); return true; } }
            "LightColor" => { if let Some(v) = color_u8(variant) { put_section(bag, "viewport", "light_color", v); return true; } }
            "LightDirection" => { if let Variant::Vector3(v) = variant { put_section(bag, "viewport", "light_direction", f32_triple(v.x, v.y, v.z)); return true; } }
            "ImageColor3" => { if let Some(v) = color_u8(variant) { put_section(bag, "viewport", "image_color", v); return true; } }
            "ImageTransparency" => { if let Some(v) = val_float(variant) { put_section(bag, "viewport", "image_transparency", v); return true; } }
            _ => {}
        }
    }
    false
}

/// Part / SpawnLocation residual props with no InstanceOverrides slot.
/// Runs AFTER try_well_known so Color/Material/Anchored/etc. keep flowing to
/// overrides. Scope: ONLY `Locked` -> [properties].locked. SpawnLocation's
/// [spawn] keys are handled by try_spawn_seat_vehicle_team_property (earlier).
fn try_part_property(bag: &mut PropertyBag, target_class: ClassName, key: &str, variant: &Variant) -> bool {
    if !matches!(target_class, ClassName::Part | ClassName::SpawnLocation) {
        return false;
    }
    if key == "Locked" {
        if let Some(v) = val_bool(variant) {
            put_section(bag, "properties", "locked", v);
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Light properties (PointLight / SpotLight / SurfaceLight → extras)
// ---------------------------------------------------------------------------

/// Route a Roblox light property into the PropertyBag's `properties_extras`
/// bucket under a stable `light_*` key. Returns `true` when the
/// `(class, key)` pair is a recognised light property (and was consumed).
///
/// The engine's light-load arm (`file_loader::spawn_directory_entry`)
/// reads these `light_*` extras back over the class template's `[Light]`
/// section defaults, building `EustressPointLight` / `EustressSpotLight` /
/// `SurfaceLight` and calling `spawn::spawn_*`.
///
/// `Brightness` is scaled ×800 to convert Roblox's unitless multiplier to
/// physically-based lumens — matching the convention in
/// `engine::spawners::lights::point_light::import_from_roblox`.
fn try_light_property(
    bag: &mut PropertyBag,
    target_class: ClassName,
    key: &str,
    variant: &Variant,
) -> bool {
    if !matches!(
        target_class,
        ClassName::PointLight | ClassName::SpotLight | ClassName::SurfaceLight
    ) {
        return false;
    }
    match key {
        "Brightness" => {
            if let Variant::Float32(b) = variant {
                // Roblox unitless multiplier → lumens (×800).
                bag.properties_extras.insert(
                    "light_brightness".to_string(),
                    toml::Value::Float((*b * 800.0) as f64),
                );
                return true;
            }
        }
        "Range" => {
            if let Variant::Float32(r) = variant {
                bag.properties_extras
                    .insert("light_range".to_string(), toml::Value::Float(*r as f64));
                return true;
            }
        }
        "Color" => match variant {
            Variant::Color3(c) => {
                bag.properties_extras.insert(
                    "light_color".to_string(),
                    toml::Value::Array(vec![
                        toml::Value::Float(c.r as f64),
                        toml::Value::Float(c.g as f64),
                        toml::Value::Float(c.b as f64),
                    ]),
                );
                return true;
            }
            Variant::Color3uint8(c) => {
                bag.properties_extras.insert(
                    "light_color".to_string(),
                    toml::Value::Array(vec![
                        toml::Value::Float(c.r as f64 / 255.0),
                        toml::Value::Float(c.g as f64 / 255.0),
                        toml::Value::Float(c.b as f64 / 255.0),
                    ]),
                );
                return true;
            }
            _ => {}
        },
        // Spotlight cone half-angle (degrees).
        "Angle" => {
            if let Variant::Float32(a) = variant {
                bag.properties_extras
                    .insert("light_angle".to_string(), toml::Value::Float(*a as f64));
                return true;
            }
        }
        "Shadows" => {
            if let Variant::Bool(s) = variant {
                bag.properties_extras
                    .insert("light_shadows".to_string(), toml::Value::Boolean(*s));
                return true;
            }
        }
        "Enabled" => {
            if let Variant::Bool(e) = variant {
                bag.properties_extras
                    .insert("light_enabled".to_string(), toml::Value::Boolean(*e));
                return true;
            }
        }
        // SurfaceLight face — string or enum; store the raw label.
        "Face" => match variant {
            Variant::String(s) => {
                bag.properties_extras
                    .insert("light_face".to_string(), toml::Value::String(s.clone()));
                return true;
            }
            Variant::Enum(e) => {
                bag.properties_extras.insert(
                    "light_face".to_string(),
                    toml::Value::Integer(e.to_u32() as i64),
                );
                return true;
            }
            _ => {}
        },
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
        Variant::Content(c) => toml::Value::String(c.as_uri().unwrap_or_default().to_string()),
        Variant::ContentId(c) => toml::Value::String(c.as_str().to_string()),
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
            // Modern MeshPart mesh/texture refs. The current reflection
            // database (roblox-700) declares `MeshPart.MeshId` with
            // `Serialization::Migrate(MeshId → MeshContent,
            // ContentIdToContent)`, and BOTH rbx_binary 2.x and rbx_xml 2.x
            // apply that migration on deserialize — so a MeshPart's mesh
            // reference arrives here as `MeshContent` (a `Variant::Content`)
            // for legacy AND modern files alike. Without these names the
            // ref landed in `[properties.extras] MeshContent = "rbxassetid
            // ://…"` and the asset resolver never fetched it — every
            // imported MeshPart rendered as a colored block.
            | "MeshContent"
            | "TextureContent"
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
            | "SunTextureId"
            | "MoonTextureId"
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
    fn mesh_content_routes_to_asset_refs() {
        // Modern MeshPart mesh ref (and the migrated legacy `MeshId` —
        // rbx_binary/rbx_xml 2.x rewrite it to `MeshContent` on load).
        let bag = map_properties(
            &props_with(vec![(
                "MeshContent",
                Variant::Content(Content::from("rbxassetid://555")),
            )]),
            ClassName::Part,
        );
        assert_eq!(
            bag.asset_refs.get("MeshContent"),
            Some(&"rbxassetid://555".to_string())
        );
        assert!(
            !bag.properties_extras.contains_key("MeshContent"),
            "mesh ref must not leak into extras"
        );
    }

    #[test]
    fn content_id_mesh_id_routes_to_asset_refs() {
        // SpecialMesh.MeshId decodes as the legacy `ContentId` variant
        // (no migration entry in the reflection database).
        let bag = map_properties(
            &props_with(vec![(
                "MeshId",
                Variant::ContentId(ContentId::from("rbxassetid://777")),
            )]),
            ClassName::SpecialMesh,
        );
        assert_eq!(
            bag.asset_refs.get("MeshId"),
            Some(&"rbxassetid://777".to_string())
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
        // rbx_types 3.x: CustomPhysicalProperties is non-exhaustive — build it
        // via the constructor (density, friction, elasticity, friction_weight,
        // elasticity_weight, acoustic_absorption).
        let pp = PhysicalProperties::Custom(CustomPhysicalProperties::new(
            1.5, 0.3, 0.0, 1.0, 1.0, 0.0,
        ));
        let bag = map_properties(
            &props_with(vec![(
                "CustomPhysicalProperties",
                Variant::PhysicalProperties(pp),
            )]),
            ClassName::Part,
        );
        assert!(bag.physics_extras.contains_key("density"));
        assert!(bag.physics_extras.contains_key("friction_static"));
        assert!(bag.physics_extras.contains_key("friction_kinetic"));
        assert!(bag.physics_extras.contains_key("restitution"));
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
    fn udim2_size_maps_to_gui_scale_and_offset() {
        let u = UDim2::new(UDim::new(0.5, 100), UDim::new(0.0, 50));
        let bag = map_properties(
            &props_with(vec![("Size", Variant::UDim2(u))]),
            ClassName::Frame,
        );
        // A GUI Size now maps to the engine's [gui] section, not inert extras.
        let gui = bag.section_props.get("gui").expect("gui section present");
        assert_eq!(
            gui.get("size_scale"),
            Some(&toml::Value::Array(vec![
                toml::Value::Float(0.5),
                toml::Value::Float(0.0),
            ]))
        );
        assert_eq!(
            gui.get("size_offset"),
            Some(&toml::Value::Array(vec![
                toml::Value::Float(100.0),
                toml::Value::Float(50.0),
            ]))
        );
        assert!(!bag.properties_extras.contains_key("Size"));
    }

    #[test]
    fn background_transparency_routes_to_gui_section() {
        let bag = map_properties(
            &props_with(vec![("BackgroundTransparency", Variant::Float32(1.0))]),
            ClassName::TextLabel,
        );
        assert_eq!(
            bag.section_props
                .get("gui")
                .and_then(|m| m.get("background_transparency")),
            Some(&toml::Value::Float(1.0))
        );
    }

    #[test]
    fn text_color3_maps_to_0_255_text_color() {
        let bag = map_properties(
            &props_with(vec![(
                "TextColor3",
                Variant::Color3(Color3::new(1.0, 1.0, 1.0)),
            )]),
            ClassName::TextLabel,
        );
        assert_eq!(
            bag.section_props
                .get("text")
                .and_then(|m| m.get("text_color")),
            Some(&toml::Value::Array(vec![
                toml::Value::Integer(255),
                toml::Value::Integer(255),
                toml::Value::Integer(255),
            ]))
        );
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

    #[test]
    fn text_label_text_routes_to_text_section() {
        let bag = map_properties(
            &props_with(vec![("Text", Variant::String("Hello".to_string()))]),
            ClassName::TextLabel,
        );
        assert_eq!(
            bag.section_props.get("text").and_then(|m| m.get("text")),
            Some(&toml::Value::String("Hello".to_string())),
            "TextLabel.Text must land in [text].text, not extras"
        );
        assert!(
            !bag.properties_extras.contains_key("Text"),
            "Text must not leak into the ignored [properties.extras] bucket"
        );
    }

    #[test]
    fn gui_zindex_routes_to_gui_section() {
        let bag = map_properties(
            &props_with(vec![("ZIndex", Variant::Int32(5))]),
            ClassName::TextLabel,
        );
        assert_eq!(
            bag.section_props.get("gui").and_then(|m| m.get("z_index")),
            Some(&toml::Value::Integer(5)),
            "ZIndex must land in [gui].z_index"
        );
    }

    #[test]
    fn text_on_non_text_class_does_not_make_text_section() {
        let bag = map_properties(
            &props_with(vec![("Text", Variant::String("x".to_string()))]),
            ClassName::Part,
        );
        assert!(
            bag.section_props.get("text").is_none(),
            "a non-text class must not synthesise a [text] section"
        );
    }

    // ── Wave-3 family routers ──────────────────────────────────────────

    #[test]
    fn sound_volume_routes_to_sound_section() {
        let bag = map_properties(
            &props_with(vec![("Volume", Variant::Float32(0.7))]),
            ClassName::Sound,
        );
        match bag.section_props.get("sound").and_then(|m| m.get("volume")) {
            Some(toml::Value::Float(v)) => assert!((*v - 0.7).abs() < 1e-6),
            other => panic!("expected [sound].volume float, got {other:?}"),
        }
    }

    #[test]
    fn sound_rolloff_mode_enum_2_approximates_to_inverse() {
        let bag = map_properties(
            &props_with(vec![("RollOffMode", Variant::Enum(Enum::from_u32(2)))]),
            ClassName::Sound,
        );
        let sound = bag.section_props.get("sound").expect("sound section");
        assert_eq!(
            sound.get("roll_off_mode"),
            Some(&toml::Value::String("Inverse".to_string()))
        );
        assert_eq!(
            sound.get("rolloff_mode"),
            Some(&toml::Value::String("Inverse".to_string()))
        );
        assert!(
            bag.approximation_notes
                .iter()
                .any(|n| n.contains("InverseTapered")),
            "RollOffMode enum 2 should log an approximation note"
        );
    }

    #[test]
    fn particle_lifetime_range_splits_to_min_max() {
        let bag = map_properties(
            &props_with(vec![(
                "Lifetime",
                Variant::NumberRange(NumberRange::new(0.5, 2.0)),
            )]),
            ClassName::ParticleEmitter,
        );
        let p = bag.section_props.get("particle").expect("particle section");
        assert_eq!(p.get("lifetime_min"), Some(&toml::Value::Float(0.5)));
        assert_eq!(p.get("lifetime_max"), Some(&toml::Value::Float(2.0)));
    }

    #[test]
    fn beam_color_sequence_collapses_and_preserves_gradient() {
        use rbx_dom_weak::types::{ColorSequence, ColorSequenceKeypoint};
        let cs = ColorSequence {
            keypoints: vec![
                ColorSequenceKeypoint::new(0.0, Color3::new(1.0, 0.0, 0.0)),
                ColorSequenceKeypoint::new(1.0, Color3::new(0.0, 0.0, 1.0)),
            ],
        };
        let bag = map_properties(
            &props_with(vec![("Color", Variant::ColorSequence(cs))]),
            ClassName::Beam,
        );
        assert_eq!(
            bag.section_props.get("beam").and_then(|m| m.get("color")),
            Some(&toml::Value::Array(vec![
                toml::Value::Integer(255),
                toml::Value::Integer(0),
                toml::Value::Integer(0),
            ]))
        );
        assert!(
            bag.properties_extras.contains_key("Color"),
            "full beam gradient must round-trip into extras"
        );
    }

    #[test]
    fn decal_transparency_routes_to_decal_not_overrides() {
        let bag = map_properties(
            &props_with(vec![("Transparency", Variant::Float32(0.25))]),
            ClassName::Decal,
        );
        assert_eq!(
            bag.section_props
                .get("decal")
                .and_then(|m| m.get("transparency")),
            Some(&toml::Value::Float(0.25))
        );
        assert!(
            bag.overrides.color_rgba.is_none(),
            "Decal.Transparency must not become a phantom part color override"
        );
    }

    #[test]
    fn atmosphere_color_is_u8_triple() {
        let bag = map_properties(
            &props_with(vec![(
                "Color",
                Variant::Color3(Color3::new(1.0, 0.5, 0.0)),
            )]),
            ClassName::Atmosphere,
        );
        assert_eq!(
            bag.section_props
                .get("atmosphere")
                .and_then(|m| m.get("color")),
            Some(&toml::Value::Array(vec![
                toml::Value::Integer(255),
                toml::Value::Integer(128),
                toml::Value::Integer(0),
            ]))
        );
        assert!(bag.overrides.color_rgba.is_none());
    }

    #[test]
    fn color_grading_tint_color_is_f32_triple() {
        let bag = map_properties(
            &props_with(vec![(
                "TintColor",
                Variant::Color3(Color3::new(1.0, 1.0, 1.0)),
            )]),
            ClassName::ColorGradingEffect,
        );
        assert_eq!(
            bag.section_props
                .get("color_grading_effect")
                .and_then(|m| m.get("tint_color")),
            Some(&toml::Value::Array(vec![
                toml::Value::Float(1.0),
                toml::Value::Float(1.0),
                toml::Value::Float(1.0),
            ]))
        );
    }

    #[test]
    fn weld_constraint_part0_ref_still_lands_in_refs() {
        let r = Ref::new();
        let bag = map_properties(
            &props_with(vec![("Part0", Variant::Ref(r))]),
            ClassName::WeldConstraint,
        );
        assert_eq!(
            bag.refs.get("Part0"),
            Some(&r),
            "constraint Refs must fall through to bag.refs, not be claimed"
        );
        assert!(bag.section_props.get("constraint").is_none());
    }

    #[test]
    fn spawn_location_enabled_routes_to_spawn_but_color_stays_override() {
        let bag = map_properties(
            &props_with(vec![
                ("Enabled", Variant::Bool(true)),
                ("Color", Variant::Color3(Color3::new(0.1, 0.2, 0.3))),
            ]),
            ClassName::SpawnLocation,
        );
        assert_eq!(
            bag.section_props
                .get("spawn")
                .and_then(|m| m.get("enabled")),
            Some(&toml::Value::Boolean(true))
        );
        let rgba = bag
            .overrides
            .color_rgba
            .expect("SpawnLocation Color must still flow to overrides");
        assert!((rgba[0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn image_button_size_maps_to_single_4_tuple() {
        let u = UDim2::new(UDim::new(0.5, 100), UDim::new(0.25, 50));
        let bag = map_properties(
            &props_with(vec![("Size", Variant::UDim2(u))]),
            ClassName::ImageButton,
        );
        let gui = bag.section_props.get("gui").expect("gui section present");
        assert_eq!(
            gui.get("size"),
            Some(&toml::Value::Array(vec![
                toml::Value::Float(0.5),
                toml::Value::Float(100.0),
                toml::Value::Float(0.25),
                toml::Value::Float(50.0),
            ])),
            "ImageButton Size must use the single 4-tuple loader shape"
        );
    }
}

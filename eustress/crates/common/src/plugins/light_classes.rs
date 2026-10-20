//! # Light classes: the authoring components drive real Bevy lights
//!
//! The one owner of the mapping from Eustress's light classes to Bevy's
//! lights, shared by Studio and the Player through `SharedLightingPlugin`.
//! Every spawn path (Space load, hot reload, Insert, paste, scripts, the
//! Play snapshot) attaches only the authoring component; the systems here
//! build the Bevy light and keep it in step.
//!
//! | Class | Authoring component | Rendered as |
//! |---|---|---|
//! | PointLight | [`EustressPointLight`] | `PointLight` on the entity |
//! | SpotLight | [`EustressSpotLight`] | `SpotLight` on an emitter child |
//! | SurfaceLight | [`SurfaceLight`] | `SpotLight` on an emitter child at the face |
//! | DirectionalLight | [`EustressDirectionalLight`] | `DirectionalLight` on the entity, no sun disc |
//!
//! ## Brightness is display-referred
//!
//! `brightness` is a Roblox-style dial. Bevy's lights are physical (lumens,
//! lux) and the camera's exposure adapts to the sky by up to nine stops
//! between noon and a moonlit night, so fixed lumens per brightness step
//! cannot work: calibrated at Bevy's default exposure (EV100 9.7), a
//! brightness-1 lamp came out about ten times too dim at daylight exposure
//! (EV100 13) and about fifty times too hot at night (EV100 4).
//!
//! The physical intensity therefore follows the ADAPTED exposure
//! ([`SkyExposure::adapted_ev100`], which excludes the author's exposure
//! compensation, so brightening the exposure still brightens the lights):
//! a lamp lights a surface the same on screen at any time of day, as in
//! Roblox, while the sun and sky stay physical. The adaptation is a function
//! of the sun, moon and sky only, never of frame luminance, so the two cannot
//! feed back into each other.
//!
//! ## Range is reach
//!
//! Roblox's Range is how far a light reaches. Here the luminous power grows
//! with the square of `range`, calibrated so a white wall facing the light
//! at half its range reads [`LightUnits::half_range_luminance`] (0.25)
//! exposed per unit of brightness; it is about 1.1 at a quarter of the
//! range, about 0.06 at three quarters and nothing at the range itself.
//! The range used for power is capped at [`MAX_REACH_M`], and the exposed
//! power at [`MAX_EXPOSED_LUMENS`], which keeps a surface 5 cm from the
//! brightest allowed light below the Rgba16Float limit (65,504): past it the
//! value becomes infinite and bloom and tonemapping turn it into NaN.
//!
//! `EUSTRESS_LIGHT_MODEL=physical` restores fixed physical units (50,000 lm
//! and 10,000 lux per unit of brightness) for measurement work, and
//! [`physical_lumens`] gives the lumens a light is actually emitting.
//!
//! ## Faces
//!
//! A SpotLight or SurfaceLight shines out of its `face`. Its Bevy light
//! lives on an emitter child ([`LightEmitter`]) so the face can turn and
//! place it without touching the light's own authored `Transform`: the
//! emitter is rotated from -Z (Bevy's spot axis, and Roblox's Front face)
//! onto the face normal, and when the light's parent is a part it sits at
//! the centre of that face. Parts are unit meshes scaled to their size, so
//! the offset is the part's half size divided by its scale.
//!
//! ## Budget
//!
//! The engine's light culler writes a [`LightBudget`] instead of touching the
//! Bevy light, so a light it turns off is rebuilt correctly when it comes
//! back, whatever was edited meanwhile.

use std::f32::consts::PI;

use bevy::light::SunDisk;
use bevy::prelude::*;

use crate::classes::{
    BasePart, ClassName, EustressDirectionalLight, EustressPointLight, EustressSpotLight,
    SurfaceLight,
};
use crate::plugins::sky_atmosphere::{SkyExposure, SkyLightSet};

// ============================================================================
// The brightness model
// ============================================================================

/// Exposed luminance of a white wall at half a light's range, per unit of
/// brightness (see the module docs). `EUSTRESS_LIGHT_HALF_RANGE_LUMINANCE`
/// overrides it for tuning.
pub const HALF_RANGE_LUMINANCE: f32 = 0.25;

/// The longest range, in metres, that still adds power. Roblox caps Range at
/// 60 studs (18.3 m), so an import never reaches it.
pub const MAX_REACH_M: f32 = 40.0;

/// Ceiling on a light's exposed luminous power (lumens times exposure): a
/// white surface 5 cm from it stays below the f16 limit.
pub const MAX_EXPOSED_LUMENS: f32 = 6_400.0;

/// Exposure the directional light calibration refers to: daylight.
pub const REFERENCE_EV100: f32 = 13.0;

/// Illuminance of a brightness-1 DirectionalLight at [`REFERENCE_EV100`].
pub const DIRECTIONAL_LUX_AT_REFERENCE: f32 = 10_000.0;

/// Lumens per unit of brightness under [`LightModel::Physical`].
pub const PHYSICAL_LUMENS_PER_BRIGHTNESS: f32 = 50_000.0;

/// Lux per unit of brightness under [`LightModel::Physical`].
pub const PHYSICAL_LUX_PER_BRIGHTNESS: f32 = 10_000.0;

/// Bevy's range window `(1 - (d/r)^4)^2` at half the range.
const WINDOW_AT_HALF_RANGE: f32 = 0.878_906_25;

/// Inner cone as a fraction of the outer one: the soft edge of a spot.
const INNER_CONE_FRACTION: f32 = 0.8;

/// How `brightness` becomes Bevy's physical units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LightModel {
    /// Display-referred, following the adapted exposure (the default).
    #[default]
    Display,
    /// Fixed physical units per brightness step, whatever the exposure.
    Physical,
}

/// The exposure the lights are compensated for, and the model in force.
#[derive(Resource, Clone, Copy, Debug)]
pub struct LightUnits {
    pub model: LightModel,
    /// Adapted EV100 the lights currently follow.
    pub ev100: f32,
    /// See [`HALF_RANGE_LUMINANCE`].
    pub half_range_luminance: f32,
}

impl Default for LightUnits {
    fn default() -> Self {
        let model = match std::env::var("EUSTRESS_LIGHT_MODEL") {
            Ok(v) if v.trim().eq_ignore_ascii_case("physical") => LightModel::Physical,
            _ => LightModel::Display,
        };
        let half_range_luminance = std::env::var("EUSTRESS_LIGHT_HALF_RANGE_LUMINANCE")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(HALF_RANGE_LUMINANCE);
        Self { model, ev100: REFERENCE_EV100, half_range_luminance }
    }
}

/// Bevy's exposure factor for an EV100 (`Exposure::exposure`).
pub fn exposure_factor(ev100: f32) -> f32 {
    (-ev100).exp2() / 1.2
}

/// Luminous power times exposure: what a light of `brightness` and `range`
/// puts on screen, independent of the exposure.
pub fn exposed_lumens(brightness: f32, range: f32, half_range_luminance: f32) -> f32 {
    let reach = if range.is_finite() { range.clamp(0.0, MAX_REACH_M) } else { MAX_REACH_M };
    let b = if brightness.is_finite() { brightness.max(0.0) } else { 0.0 };
    (b * half_range_luminance * PI * PI * reach * reach / WINDOW_AT_HALF_RANGE)
        .min(MAX_EXPOSED_LUMENS)
}

/// Lumens for a point, spot or surface light.
pub fn light_lumens(brightness: f32, range: f32, units: &LightUnits) -> f32 {
    match units.model {
        LightModel::Display => {
            exposed_lumens(brightness, range, units.half_range_luminance) / exposure_factor(units.ev100)
        }
        LightModel::Physical => brightness.max(0.0) * PHYSICAL_LUMENS_PER_BRIGHTNESS,
    }
}

/// Lux for a directional light.
pub fn directional_lux(brightness: f32, units: &LightUnits) -> f32 {
    let b = if brightness.is_finite() { brightness.max(0.0) } else { 0.0 };
    match units.model {
        LightModel::Display => b * DIRECTIONAL_LUX_AT_REFERENCE * (units.ev100 - REFERENCE_EV100).exp2(),
        LightModel::Physical => b * PHYSICAL_LUX_PER_BRIGHTNESS,
    }
}

/// The lumens a light emits at an EV100, for readouts and analysis tools
/// that need the physical figure behind a display-referred brightness.
pub fn physical_lumens(brightness: f32, range: f32, ev100: f32) -> f32 {
    light_lumens(
        brightness,
        range,
        &LightUnits { model: LightModel::Display, ev100, half_range_luminance: HALF_RANGE_LUMINANCE },
    )
}

/// Bevy's `(inner, outer)` half-angles in radians for a Roblox cone angle
/// (the full apex angle in degrees). Bevy needs the outer angle below 90
/// degrees; at 180 a Roblox SurfaceLight lights the whole half-space, which
/// 89 degrees approximates.
pub fn spot_cone(angle_deg: f32) -> (f32, f32) {
    let a = if angle_deg.is_finite() { angle_deg } else { crate::classes::DEFAULT_LIGHT_ANGLE };
    let outer = (a * 0.5).clamp(1.0, 89.0).to_radians();
    (outer * INNER_CONE_FRACTION, outer)
}

// ============================================================================
// Faces
// ============================================================================

/// The six face labels, in the order the Properties panel lists them.
pub const FACES: [&str; 6] = ["Top", "Bottom", "Front", "Back", "Left", "Right"];

/// A face label in canonical form. Accepts any case, the `Enum.NormalId.`
/// and `NormalId.` prefixes, and Roblox's `NormalId` numbers (0 Right,
/// 1 Top, 2 Back, 3 Left, 4 Bottom, 5 Front). Anything else is Front, the
/// default face.
pub fn normalize_face(raw: &str) -> &'static str {
    let s = raw.trim();
    let s = s.rsplit('.').next().unwrap_or(s);
    match s.to_ascii_lowercase().as_str() {
        "top" | "1" => "Top",
        "bottom" | "4" => "Bottom",
        "back" | "2" => "Back",
        "left" | "3" => "Left",
        "right" | "0" => "Right",
        _ => "Front",
    }
}

/// A face's outward normal in the part's frame: Front is -Z, Back +Z, Top
/// +Y, Bottom -Y, Right +X, Left -X.
pub fn face_normal(face: &str) -> Vec3 {
    match normalize_face(face) {
        "Top" => Vec3::Y,
        "Bottom" => Vec3::NEG_Y,
        "Back" => Vec3::Z,
        "Right" => Vec3::X,
        "Left" => Vec3::NEG_X,
        _ => Vec3::NEG_Z,
    }
}

/// The rotation that turns a spot light's axis (-Z) onto a face normal.
pub fn face_rotation(face: &str) -> Quat {
    Quat::from_rotation_arc(Vec3::NEG_Z, face_normal(face))
}

/// Where a light's emitter sits, relative to the light, for a face of the
/// part it is in. `host` is that part's `(size, scale)`: parts are unit
/// meshes scaled to their size, so a face centre is half the size divided
/// by the scale out from the centre. With no part the emitter stays on the
/// light and only turns.
pub fn emitter_transform(face: &str, host: Option<(Vec3, Vec3)>) -> Transform {
    let normal = face_normal(face);
    let translation = match host {
        Some((size, scale)) => {
            let safe = Vec3::new(
                if scale.x.abs() > 1e-6 { scale.x } else { 1.0 },
                if scale.y.abs() > 1e-6 { scale.y } else { 1.0 },
                if scale.z.abs() > 1e-6 { scale.z } else { 1.0 },
            );
            normal * size * 0.5 / safe
        }
        None => Vec3::ZERO,
    };
    Transform { translation, rotation: face_rotation(face), scale: Vec3::ONE }
}

/// The two sides of a face light's rectangle, used to widen its highlight
/// the way an area light's is wide.
fn face_extent(face: &str, size: Vec3) -> (f32, f32) {
    match normalize_face(face) {
        "Top" | "Bottom" => (size.x, size.z),
        "Left" | "Right" => (size.y, size.z),
        _ => (size.x, size.y),
    }
}

// ============================================================================
// Components
// ============================================================================

/// What the engine's light culler allows a light this frame. Absent means
/// both. The sync reads it; nothing else writes the Bevy light for a class.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightBudget {
    /// Off: the light is dimmed to nothing (too far, or past the active cap).
    pub active: bool,
    /// Off: the light may not cast shadows (past the shadow cap).
    pub shadows: bool,
}

impl Default for LightBudget {
    fn default() -> Self {
        Self { active: true, shadows: true }
    }
}

/// The child entity that carries a SpotLight's or SurfaceLight's Bevy
/// `SpotLight`, turned and placed by its face.
#[derive(Component, Clone, Copy, Debug)]
pub struct LightEmitter {
    /// The light-class entity the emitter belongs to.
    pub owner: Entity,
}

/// On a SpotLight or SurfaceLight: its emitter child.
#[derive(Component, Clone, Copy, Debug)]
pub struct LightEmitterLink(pub Entity);

/// The part a face light is attached to, when it is not its hierarchy
/// parent: the Player places a Space's lights in world space, so it records
/// the part's size here. A light in a part's hierarchy reads the part itself.
#[derive(Component, Clone, Copy, Debug)]
pub struct LightFaceHost {
    /// Size of the part, in metres.
    pub size: Vec3,
}

/// The entity that holds a light's Bevy component: its emitter for a
/// SpotLight or SurfaceLight, the light itself otherwise.
pub fn render_entity(entity: Entity, link: Option<&LightEmitterLink>) -> Entity {
    link.map_or(entity, |l| l.0)
}

/// True for the four light classes.
pub fn is_light_class(class: ClassName) -> bool {
    matches!(
        class,
        ClassName::PointLight | ClassName::SpotLight | ClassName::SurfaceLight | ClassName::DirectionalLight
    )
}

// ============================================================================
// TOML
// ============================================================================

/// A light's authored values as read from its instance file. Every field is
/// optional: what a file leaves out keeps the component's default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LightSection {
    pub brightness: Option<f32>,
    pub color: Option<Color>,
    pub range: Option<f32>,
    pub radius: Option<f32>,
    pub angle: Option<f32>,
    pub face: Option<String>,
    pub shadows: Option<bool>,
    pub enabled: Option<bool>,
    pub shadow_depth_bias: Option<f32>,
    pub shadow_normal_bias: Option<f32>,
}

/// The `light_*` keys the Roblox importer wrote into `[properties.extras]`
/// before lights carried a `[light]` section, and the `[light]` key each
/// became. Their brightness was multiplied by [`LEGACY_EXTRAS_BRIGHTNESS`],
/// and their lengths are Roblox studs (see [`legacy_studs_to_m`]).
pub const LEGACY_LIGHT_EXTRAS: [(&str, &str); 8] = [
    ("light_brightness", "brightness"),
    ("light_color", "color"),
    ("light_range", "range"),
    ("light_radius", "radius"),
    ("light_angle", "angle"),
    ("light_face", "face"),
    ("light_shadows", "shadows"),
    ("light_enabled", "enabled"),
];

/// The factor the importer applied to Roblox's Brightness in
/// `light_brightness` (a lumens convention the engine no longer uses).
pub const LEGACY_EXTRAS_BRIGHTNESS: f32 = 800.0;

/// Metres per stud in the legacy `light_range` and `light_radius` keys: the
/// importer copied Roblox's Range as authored, and its stud is a foot (the
/// same stud its files' `unit = "ft"` declares).
pub const LEGACY_EXTRAS_STUD_M: f64 = 0.3048;

/// A legacy stud length in metres, rounded to the micrometre so a migrated
/// file reads `4.8768` rather than float noise.
pub fn legacy_studs_to_m(studs: f64) -> f64 {
    (studs * LEGACY_EXTRAS_STUD_M * 1.0e6).round() / 1.0e6
}

/// A value, or the `value` of an inline `{ type, value }` descriptor (the
/// rich-schema form some templates use).
fn unwrap_descriptor(v: &toml::Value) -> &toml::Value {
    match v {
        toml::Value::Table(t) => t.get("value").unwrap_or(v),
        _ => v,
    }
}

fn section_get<'a>(section: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let table = section.as_table()?;
    let v = table.get(key).or_else(|| {
        table
            .iter()
            .find(|(k, _)| k.replace('_', "").eq_ignore_ascii_case(&key.replace('_', "")))
            .map(|(_, v)| v)
    })?;
    Some(unwrap_descriptor(v))
}

fn as_f32(v: &toml::Value) -> Option<f32> {
    v.as_float()
        .or_else(|| v.as_integer().map(|i| i as f64))
        .map(|f| f as f32)
        .filter(|f| f.is_finite())
}

/// A colour array: all integers are 0-255 channels, anything with a float
/// is 0-1 (the rule the part loader uses).
pub fn color_from_toml(v: &toml::Value) -> Option<Color> {
    let arr = v.as_array()?;
    if arr.len() < 3 {
        return None;
    }
    let all_int = arr.iter().take(3).all(|c| c.is_integer());
    let ch = |i: usize| -> Option<f32> {
        let c = as_f32(&arr[i])?;
        Some(if all_int { c / 255.0 } else { c }.clamp(0.0, 1.0))
    };
    Some(Color::srgb(ch(0)?, ch(1)?, ch(2)?))
}

fn face_from_toml(v: &toml::Value) -> Option<String> {
    match v {
        toml::Value::String(s) => Some(normalize_face(s).to_string()),
        toml::Value::Integer(i) => Some(normalize_face(&i.to_string()).to_string()),
        _ => None,
    }
}

impl LightSection {
    /// Read the `[light]` section (any key case, plain values or inline
    /// descriptors) and the legacy `[properties.extras]` `light_*` keys of an
    /// instance document. A legacy key wins over the section: a file that
    /// still carries them is an import never migrated, whose `[light]` is the
    /// class template's defaults (see
    /// `class_schema::migrate_legacy_light_extras`).
    pub fn from_document(doc: &toml::Value) -> Self {
        let section = doc.get("light").or_else(|| doc.get("Light"));
        let extras = doc
            .get("properties")
            .or_else(|| doc.get("Properties"))
            .and_then(|p| p.get("extras"));
        Self::from_parts(section, extras)
    }

    /// Like [`Self::from_document`], from the section and the extras table.
    pub fn from_parts(section: Option<&toml::Value>, extras: Option<&toml::Value>) -> Self {
        let mut out = Self::default();
        if let Some(s) = section {
            out.brightness = section_get(s, "brightness").and_then(as_f32);
            out.color = section_get(s, "color").and_then(color_from_toml);
            out.range = section_get(s, "range").and_then(as_f32);
            out.radius = section_get(s, "radius").and_then(as_f32);
            out.angle = section_get(s, "angle").and_then(as_f32);
            out.face = section_get(s, "face").and_then(face_from_toml);
            out.shadows = section_get(s, "shadows").and_then(|v| v.as_bool());
            out.enabled = section_get(s, "enabled").and_then(|v| v.as_bool());
            out.shadow_depth_bias = section_get(s, "shadow_depth_bias").and_then(as_f32);
            out.shadow_normal_bias = section_get(s, "shadow_normal_bias").and_then(as_f32);
        }
        if let Some(e) = extras {
            if let Some(b) = e.get("light_brightness").and_then(as_f32) {
                out.brightness = Some(b / LEGACY_EXTRAS_BRIGHTNESS);
            }
            if let Some(c) = e.get("light_color").and_then(color_from_toml) {
                out.color = Some(c);
            }
            if let Some(r) = e.get("light_range").and_then(as_f32) {
                out.range = Some(legacy_studs_to_m(r as f64) as f32);
            }
            if let Some(r) = e.get("light_radius").and_then(as_f32) {
                out.radius = Some(legacy_studs_to_m(r as f64) as f32);
            }
            if let Some(a) = e.get("light_angle").and_then(as_f32) {
                out.angle = Some(a);
            }
            if let Some(f) = e.get("light_face").and_then(face_from_toml) {
                out.face = Some(f);
            }
            if let Some(s) = e.get("light_shadows").and_then(|v| v.as_bool()) {
                out.shadows = Some(s);
            }
            if let Some(en) = e.get("light_enabled").and_then(|v| v.as_bool()) {
                out.enabled = Some(en);
            }
        }
        out
    }

    pub fn point(&self) -> EustressPointLight {
        let d = EustressPointLight::default();
        EustressPointLight {
            brightness: self.brightness.unwrap_or(d.brightness).max(0.0),
            color: self.color.unwrap_or(d.color),
            range: self.range.unwrap_or(d.range).max(0.0),
            radius: self.radius.unwrap_or(d.radius).max(0.0),
            shadows: self.shadows.unwrap_or(d.shadows),
            enabled: self.enabled.unwrap_or(d.enabled),
            texture: d.texture,
        }
    }

    pub fn spot(&self) -> EustressSpotLight {
        let d = EustressSpotLight::default();
        EustressSpotLight {
            brightness: self.brightness.unwrap_or(d.brightness).max(0.0),
            color: self.color.unwrap_or(d.color),
            range: self.range.unwrap_or(d.range).max(0.0),
            angle: self.angle.unwrap_or(d.angle).clamp(0.0, 180.0),
            shadows: self.shadows.unwrap_or(d.shadows),
            enabled: self.enabled.unwrap_or(d.enabled),
            face: self.face.clone().unwrap_or(d.face),
            texture: d.texture,
        }
    }

    pub fn surface(&self) -> SurfaceLight {
        let d = SurfaceLight::default();
        SurfaceLight {
            brightness: self.brightness.unwrap_or(d.brightness).max(0.0),
            color: self.color.unwrap_or(d.color),
            range: self.range.unwrap_or(d.range).max(0.0),
            face: self.face.clone().unwrap_or(d.face),
            // The SurfaceLight template once wrote `angle = 0.0` (the angle
            // was not read then), so every SurfaceLight inserted or healed
            // since carries it. Read as a cone that is a pencil beam; it
            // means "unset", the Roblox default.
            angle: self.angle.filter(|a| *a > 0.0).unwrap_or(d.angle).clamp(0.0, 180.0),
            shadows: self.shadows.unwrap_or(d.shadows),
            enabled: self.enabled.unwrap_or(d.enabled),
            texture: d.texture,
        }
    }

    pub fn directional(&self) -> EustressDirectionalLight {
        let d = EustressDirectionalLight::default();
        EustressDirectionalLight {
            brightness: self.brightness.unwrap_or(d.brightness).max(0.0),
            color: self.color.unwrap_or(d.color),
            shadows: self.shadows.unwrap_or(d.shadows),
            enabled: self.enabled.unwrap_or(d.enabled),
            shadow_depth_bias: self.shadow_depth_bias.unwrap_or(d.shadow_depth_bias),
            shadow_normal_bias: self.shadow_normal_bias.unwrap_or(d.shadow_normal_bias),
            texture: d.texture,
        }
    }
}

/// Read a light-class instance document straight into the class's authoring
/// component, as a boxed bundle. `None` for any other class.
pub fn light_component_from_document(class: ClassName, doc: &toml::Value) -> Option<LightComponent> {
    LightComponent::from_section(class, &LightSection::from_document(doc))
}

/// One light class's authoring component.
#[derive(Debug, Clone)]
pub enum LightComponent {
    Point(EustressPointLight),
    Spot(EustressSpotLight),
    Surface(SurfaceLight),
    Directional(EustressDirectionalLight),
}

impl LightComponent {
    pub fn from_section(class: ClassName, s: &LightSection) -> Option<Self> {
        Some(match class {
            ClassName::PointLight => Self::Point(s.point()),
            ClassName::SpotLight => Self::Spot(s.spot()),
            ClassName::SurfaceLight => Self::Surface(s.surface()),
            ClassName::DirectionalLight => Self::Directional(s.directional()),
            _ => return None,
        })
    }

    /// Put the component on an entity (replacing one already there).
    pub fn insert(self, entity: &mut EntityCommands) {
        match self {
            Self::Point(c) => entity.insert(c),
            Self::Spot(c) => entity.insert(c),
            Self::Surface(c) => entity.insert(c),
            Self::Directional(c) => entity.insert(c),
        };
    }

    /// Put the component on an entity unless it already carries an equal
    /// one, so a reload that changes nothing marks nothing changed.
    pub fn replace_in(self, entity: &mut EntityWorldMut) {
        macro_rules! put {
            ($c:expr, $ty:ty) => {{
                let same = entity.get::<$ty>().is_some_and(|old| same_light_value(old, &$c));
                if !same {
                    entity.insert($c);
                }
            }};
        }
        match self {
            Self::Point(c) => put!(c, EustressPointLight),
            Self::Spot(c) => put!(c, EustressSpotLight),
            Self::Surface(c) => put!(c, SurfaceLight),
            Self::Directional(c) => put!(c, EustressDirectionalLight),
        }
    }

    /// The `[light]` section that stores this component.
    pub fn to_section(&self) -> toml::value::Table {
        match self {
            Self::Point(c) => light_section_table(&LightSectionRef::Point(c)),
            Self::Spot(c) => light_section_table(&LightSectionRef::Spot(c)),
            Self::Surface(c) => light_section_table(&LightSectionRef::Surface(c)),
            Self::Directional(c) => light_section_table(&LightSectionRef::Directional(c)),
        }
    }
}

/// Equality through the serialized form: the components hold `Color`, which
/// compares exactly, so this is the value a file round-trip would keep.
fn same_light_value<T: serde::Serialize>(a: &T, b: &T) -> bool {
    match (toml::Value::try_from(a), toml::Value::try_from(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// A borrowed light component, for serializing without a clone.
pub enum LightSectionRef<'a> {
    Point(&'a EustressPointLight),
    Spot(&'a EustressSpotLight),
    Surface(&'a SurfaceLight),
    Directional(&'a EustressDirectionalLight),
}

/// A colour as the 0-255 integer triple the class templates use.
pub fn color_to_toml(c: Color) -> toml::Value {
    let s = c.to_srgba();
    let ch = |v: f32| toml::Value::Integer((v.clamp(0.0, 1.0) * 255.0).round() as i64);
    toml::Value::Array(vec![ch(s.red), ch(s.green), ch(s.blue)])
}

fn float(v: f32) -> toml::Value {
    // Round away float noise (0.30000001) so saved files stay readable.
    toml::Value::Float(((v as f64) * 1.0e6).round() / 1.0e6)
}

/// The `[light]` section for a light component: the keys the class template
/// declares, in its spelling. Range and radius are metres.
pub fn light_section_table(light: &LightSectionRef) -> toml::value::Table {
    let mut t = toml::value::Table::new();
    match light {
        LightSectionRef::Point(c) => {
            t.insert("brightness".into(), float(c.brightness));
            t.insert("color".into(), color_to_toml(c.color));
            t.insert("range".into(), float(c.range));
            t.insert("radius".into(), float(c.radius));
            t.insert("shadows".into(), toml::Value::Boolean(c.shadows));
            t.insert("enabled".into(), toml::Value::Boolean(c.enabled));
        }
        LightSectionRef::Spot(c) => {
            t.insert("brightness".into(), float(c.brightness));
            t.insert("color".into(), color_to_toml(c.color));
            t.insert("range".into(), float(c.range));
            t.insert("angle".into(), float(c.angle));
            t.insert("face".into(), toml::Value::String(normalize_face(&c.face).to_string()));
            t.insert("shadows".into(), toml::Value::Boolean(c.shadows));
            t.insert("enabled".into(), toml::Value::Boolean(c.enabled));
        }
        LightSectionRef::Surface(c) => {
            t.insert("brightness".into(), float(c.brightness));
            t.insert("color".into(), color_to_toml(c.color));
            t.insert("range".into(), float(c.range));
            t.insert("angle".into(), float(c.angle));
            t.insert("face".into(), toml::Value::String(normalize_face(&c.face).to_string()));
            t.insert("shadows".into(), toml::Value::Boolean(c.shadows));
            t.insert("enabled".into(), toml::Value::Boolean(c.enabled));
        }
        LightSectionRef::Directional(c) => {
            t.insert("brightness".into(), float(c.brightness));
            t.insert("color".into(), color_to_toml(c.color));
            t.insert("shadows".into(), toml::Value::Boolean(c.shadows));
            t.insert("enabled".into(), toml::Value::Boolean(c.enabled));
            t.insert("shadow_depth_bias".into(), float(c.shadow_depth_bias));
            t.insert("shadow_normal_bias".into(), float(c.shadow_normal_bias));
        }
    }
    t
}

/// Write `section` as an instance document's `[light]`: keys it does not
/// name (a texture, say) stay, a PascalCase `[Light]` duplicate is folded
/// in and removed, and the legacy `light_*` extras are dropped so the
/// section is the only place the values live.
pub fn store_light_section(doc: &mut toml::Value, section: toml::value::Table) {
    let Some(root) = doc.as_table_mut() else { return };
    let mut merged = match root.remove("light") {
        Some(toml::Value::Table(t)) => t,
        _ => toml::value::Table::new(),
    };
    if let Some(toml::Value::Table(old)) = root.remove("Light") {
        for (k, v) in old {
            let key = crate::class_schema::pascal_to_snake(&k);
            merged.entry(key).or_insert(v);
        }
    }
    for (k, v) in section {
        merged.insert(k, v);
    }
    root.insert("light".into(), toml::Value::Table(merged));
    remove_legacy_light_extras(doc);
}

/// Drop the importer's legacy `light_*` keys from `[properties.extras]`,
/// and the extras table itself when that empties it. True when any went.
pub fn remove_legacy_light_extras(doc: &mut toml::Value) -> bool {
    let Some(props) = doc.get_mut("properties").and_then(|p| p.as_table_mut()) else {
        return false;
    };
    let Some(extras) = props.get_mut("extras").and_then(|e| e.as_table_mut()) else {
        return false;
    };
    let mut removed = false;
    for (key, _) in LEGACY_LIGHT_EXTRAS {
        removed |= extras.remove(key).is_some();
    }
    if extras.is_empty() {
        props.remove("extras");
    }
    removed
}

/// A light's pose from its instance document: `[transform]` position and
/// rotation (integers or floats), the position converted from the file's
/// `[metadata] unit` to metres as the part loader converts it (with
/// `units_v1`, the default). A light is never scaled.
pub fn light_transform_from_document(doc: &toml::Value) -> Transform {
    let t = doc.get("transform").or_else(|| doc.get("Transform"));
    let arr = |key: &str, n: usize| -> Option<Vec<f32>> {
        let a = t?.get(key)?.as_array()?;
        let v: Vec<f32> = a.iter().take(n).filter_map(as_f32).collect();
        (v.len() == n).then_some(v)
    };
    #[allow(unused_mut)]
    let mut translation = arr("position", 3).map(|p| Vec3::new(p[0], p[1], p[2])).unwrap_or(Vec3::ZERO);
    #[cfg(feature = "units_v1")]
    {
        let unit = doc
            .get("metadata")
            .and_then(|m| m.get("unit"))
            .and_then(|u| u.as_str())
            .and_then(crate::units::Unit::from_symbol);
        if let Some(unit) = unit {
            translation =
                Vec3::from_array(crate::units::authored_to_engine_vec3_f32(translation.to_array(), unit));
        }
    }
    let rotation = arr("rotation", 4)
        .map(|r| Quat::from_xyzw(r[0], r[1], r[2], r[3]))
        .filter(|q| q.is_finite() && q.length_squared() > 1e-6)
        .map(|q| q.normalize())
        .unwrap_or(Quat::IDENTITY);
    Transform { translation, rotation, scale: Vec3::ONE }
}

/// Re-read a light's `[light]` section when its file changes on disk (MCP
/// edits, git checkouts, a text editor, the Properties panel's own save).
/// Queued as an entity command so the file watcher needs no new system
/// parameters; a no-op for any other class and when nothing changed.
pub fn queue_light_reload(commands: &mut Commands, entity: Entity, class: ClassName, toml_text: &str) {
    if !is_light_class(class) {
        return;
    }
    let Ok(doc) = toml_text.parse::<toml::Value>() else { return };
    let Some(fresh) = light_component_from_document(class, &doc) else { return };
    commands.entity(entity).queue(move |mut e: EntityWorldMut| fresh.replace_in(&mut e));
}

// ============================================================================
// A Space's lights, for a shell that reads Spaces from disk (the Player)
// ============================================================================

/// One light read out of a Space, placed in world space.
#[derive(Debug, Clone)]
pub struct SpaceLight {
    pub name: String,
    pub component: LightComponent,
    /// World pose.
    pub transform: Transform,
    /// Size of the part the light is in, when it is in one: the face a
    /// SpotLight or SurfaceLight shines out of.
    pub host_size: Option<Vec3>,
}

/// The frame an instance's children nest in: its world position and
/// rotation, and its size when it is a part.
#[derive(Clone, Copy, Default)]
struct NestFrame {
    translation: Vec3,
    rotation: Quat,
    part_size: Option<Vec3>,
}

/// Every light instance under a Space's `Workspace/` and `Lighting/`, placed
/// as Studio places it: at its parent part's centre, turned with the part,
/// its own offset in the part's size units (Studio parents it to the part,
/// whose scale is its size). Instances nest as in `space_read`: a directory
/// holding `_instance.toml` is that instance, and the folder form wins over
/// a flat `<Name>.instance.toml`. Files that cannot be read are reported,
/// never dropped silently.
pub fn read_space_lights(space_root: &std::path::Path) -> (Vec<SpaceLight>, Vec<String>) {
    let mut lights = Vec::new();
    let mut problems = Vec::new();
    for service in ["Workspace", "Lighting"] {
        let dir = space_root.join(service);
        if dir.is_dir() {
            walk_space_lights(&dir, NestFrame { rotation: Quat::IDENTITY, ..Default::default() }, &mut lights, &mut problems);
        }
    }
    (lights, problems)
}

fn walk_space_lights(
    dir: &std::path::Path,
    parent: NestFrame,
    lights: &mut Vec<SpaceLight>,
    problems: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        problems.push(format!("unreadable directory: {}", dir.display()));
        return;
    };
    let mut dirs = Vec::new();
    let mut flat = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|s| s.to_str()) else { continue };
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            dirs.push(p);
        } else if name.ends_with(".instance.toml") {
            flat.push(p);
        }
    }
    let dir_names: Vec<String> = dirs
        .iter()
        .filter_map(|d| d.file_name()?.to_str().map(str::to_owned))
        .collect();
    for f in flat {
        let stem = f
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.trim_end_matches(".instance.toml").to_owned())
            .unwrap_or_default();
        if dir_names.contains(&stem) {
            continue;
        }
        ingest_space_instance(&f, &stem, &parent, lights, problems);
    }
    for d in dirs {
        let name = d.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_owned();
        let inst = d.join("_instance.toml");
        let here = if inst.is_file() {
            ingest_space_instance(&inst, &name, &parent, lights, problems).unwrap_or(parent)
        } else {
            parent
        };
        walk_space_lights(&d, here, lights, problems);
    }
}

/// Read one instance file: a light is collected; anything else returns the
/// frame its children nest in.
fn ingest_space_instance(
    path: &std::path::Path,
    fallback_name: &str,
    parent: &NestFrame,
    lights: &mut Vec<SpaceLight>,
    problems: &mut Vec<String>,
) -> Option<NestFrame> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            problems.push(format!("{}: {e}", path.display()));
            return None;
        }
    };
    let doc: toml::Value = match text.parse() {
        Ok(d) => d,
        Err(e) => {
            problems.push(format!("{}: {e}", path.display()));
            return None;
        }
    };
    let meta = doc.get("metadata").or_else(|| doc.get("Metadata"));
    let class_str = meta
        .and_then(|m| m.get("class_name").or_else(|| m.get("ClassName")))
        .and_then(|c| c.as_str())
        .unwrap_or("Part");
    let name = meta
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or(fallback_name)
        .to_string();

    if let Some(class) = ClassName::from_str(class_str).ok().filter(|c| is_light_class(*c)) {
        let local = light_transform_from_document(&doc);
        let offset = parent.part_size.map_or(local.translation, |s| local.translation * s);
        let transform = Transform {
            translation: parent.translation + parent.rotation * offset,
            rotation: parent.rotation * local.rotation,
            scale: Vec3::ONE,
        };
        if let Some(component) = light_component_from_document(class, &doc) {
            lights.push(SpaceLight { name, component, transform, host_size: parent.part_size });
        }
        return None;
    }

    // Any other instance: the frame of what nests in it.
    let t = doc.get("transform").or_else(|| doc.get("Transform"));
    let arr = |key: &str, n: usize, default: &[f32]| -> Vec<f32> {
        t.and_then(|t| t.get(key))
            .and_then(|a| a.as_array())
            .map(|a| a.iter().take(n).filter_map(as_f32).collect::<Vec<f32>>())
            .filter(|v| v.len() == n)
            .unwrap_or_else(|| default.to_vec())
    };
    let p = arr("position", 3, &[0.0, 0.0, 0.0]);
    let r = arr("rotation", 4, &[0.0, 0.0, 0.0, 1.0]);
    let s = arr("scale", 3, &[1.0, 1.0, 1.0]);
    let unit = meta.and_then(|m| m.get("unit")).and_then(|u| u.as_str());
    let pose = crate::space_read::authored_pose([p[0], p[1], p[2]], [r[0], r[1], r[2], r[3]], [s[0], s[1], s[2]], unit);
    Some(NestFrame {
        translation: parent.translation + parent.rotation * pose.translation,
        rotation: parent.rotation * pose.rotation,
        part_size: crate::datamodel::is_base_part(class_str).then_some(pose.scale),
    })
}

/// Spawn the lights [`read_space_lights`] found. Each gets its class
/// component (the systems below build its Bevy light) and, when it is in a
/// part, a [`LightFaceHost`] with the part's size. With the `physics`
/// feature they carry `space_read::SpawnedFromSpace`, so a shell clearing a
/// Space's parts clears its lights too.
pub fn spawn_space_lights(commands: &mut Commands, lights: &[SpaceLight]) -> usize {
    for light in lights {
        let mut e = commands.spawn((light.transform, Visibility::default(), Name::new(light.name.clone())));
        light.component.clone().insert(&mut e);
        if let Some(size) = light.host_size {
            e.insert(LightFaceHost { size });
        }
        #[cfg(feature = "physics")]
        e.insert(crate::space_read::SpawnedFromSpace);
    }
    lights.len()
}

// ============================================================================
// Plugin and systems
// ============================================================================

/// The systems that build and update Bevy lights from the light classes.
/// The engine's culler runs before this set, so a budget change lands in the
/// same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LightSyncSet;

/// Builds every light class's Bevy light and keeps it in step.
pub struct LightClassSyncPlugin;

impl Plugin for LightClassSyncPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LightUnits>()
            .register_type::<EustressPointLight>()
            .register_type::<EustressSpotLight>()
            .register_type::<SurfaceLight>()
            .register_type::<EustressDirectionalLight>()
            .configure_sets(Update, LightSyncSet.after(SkyLightSet))
            .add_systems(
                Update,
                (
                    update_light_units,
                    (
                        sync_point_lights,
                        sync_face_lights,
                        sync_directional_lights,
                        despawn_orphan_emitters,
                    ),
                )
                    .chain()
                    .in_set(LightSyncSet),
            );
    }
}

/// Follow the adapted exposure. Written only when it moves, so the lights
/// are rewritten only then.
fn update_light_units(exposure: Option<Res<SkyExposure>>, mut units: ResMut<LightUnits>) {
    let ev = exposure.map_or(REFERENCE_EV100, |e| e.adapted_ev100);
    if ev.is_finite() && (ev - units.ev100).abs() > 1.0e-3 {
        units.ev100 = ev;
    }
}

/// Whether a light is on and allowed shadows, from its own switches and the
/// culler's budget.
fn gates(enabled: bool, shadows: bool, budget: Option<&LightBudget>) -> (bool, bool) {
    let b = budget.copied().unwrap_or_default();
    let on = enabled && b.active;
    (on, on && shadows && b.shadows)
}

/// `EustressPointLight` → `PointLight` on the same entity.
fn sync_point_lights(
    mut commands: Commands,
    units: Res<LightUnits>,
    mut lights: Query<(
        Entity,
        Ref<EustressPointLight>,
        Option<Ref<LightBudget>>,
        Option<&mut PointLight>,
    )>,
) {
    let all = units.is_changed();
    for (entity, light, budget, existing) in &mut lights {
        let budget_changed = budget.as_ref().is_some_and(|b| b.is_changed());
        if !(all || light.is_changed() || budget_changed || existing.is_none()) {
            continue;
        }
        let (on, shadows) = gates(light.enabled, light.shadows, budget.as_deref());
        let intensity = if on { light_lumens(light.brightness, light.range, &units) } else { 0.0 };
        match existing {
            Some(mut pl) => {
                // Write only what differs: an exposure step that leaves a
                // dimmed light at zero must not mark it changed.
                if pl.intensity != intensity {
                    pl.intensity = intensity;
                }
                if pl.color != light.color {
                    pl.color = light.color;
                }
                if pl.range != light.range {
                    pl.range = light.range;
                }
                if pl.radius != light.radius {
                    pl.radius = light.radius;
                }
                if pl.shadow_maps_enabled != shadows {
                    pl.shadow_maps_enabled = shadows;
                }
            }
            None => {
                commands.entity(entity).try_insert(PointLight {
                    color: light.color,
                    intensity,
                    range: light.range,
                    radius: light.radius,
                    shadow_maps_enabled: shadows,
                    ..default()
                });
            }
        }
    }
}

/// What a SpotLight or SurfaceLight asks of its emitter.
struct FaceLight {
    brightness: f32,
    color: Color,
    range: f32,
    angle: f32,
    shadows: bool,
    enabled: bool,
    /// True for a SurfaceLight, whose highlight widens with its face.
    area: bool,
}

/// The emitter's `SpotLight` and local transform for a face light.
fn face_emitter(
    light: &FaceLight,
    face: &str,
    host: Option<(Vec3, Vec3)>,
    budget: Option<&LightBudget>,
    units: &LightUnits,
) -> (SpotLight, Transform) {
    let (on, shadows) = gates(light.enabled, light.shadows, budget);
    let (inner_angle, outer_angle) = spot_cone(light.angle);
    // A surface light's highlight is as wide as a quarter of its face's
    // shorter side, capped: specular only, the diffuse light is unchanged.
    let radius = match (light.area, host) {
        (true, Some((size, _))) => {
            let (w, h) = face_extent(face, size);
            (w.min(h) * 0.25).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    (
        SpotLight {
            color: light.color,
            intensity: if on { light_lumens(light.brightness, light.range, units) } else { 0.0 },
            range: light.range,
            radius,
            shadow_maps_enabled: shadows,
            inner_angle,
            outer_angle,
            ..default()
        },
        emitter_transform(face, host),
    )
}

/// SpotLight and SurfaceLight → a `SpotLight` on an emitter child, turned
/// and placed by the face.
#[allow(clippy::type_complexity)]
fn sync_face_lights(
    mut commands: Commands,
    units: Res<LightUnits>,
    lights: Query<(
        Entity,
        Option<Ref<EustressSpotLight>>,
        Option<Ref<SurfaceLight>>,
        Option<Ref<LightBudget>>,
        Option<&LightEmitterLink>,
        Option<&ChildOf>,
        Option<Ref<LightFaceHost>>,
        Has<Visibility>,
    ), Or<(With<EustressSpotLight>, With<SurfaceLight>)>>,
    hosts: Query<(Ref<BasePart>, Ref<Transform>), Without<LightEmitter>>,
    mut emitters: Query<(&mut SpotLight, &mut Transform), With<LightEmitter>>,
) {
    let all = units.is_changed();
    for (entity, spot, surface, budget, link, child_of, face_host, has_visibility) in &lights {
        let (light, face, class_changed) = match (&spot, &surface) {
            (Some(s), _) => (
                FaceLight {
                    brightness: s.brightness,
                    color: s.color,
                    range: s.range,
                    angle: s.angle,
                    shadows: s.shadows,
                    enabled: s.enabled,
                    area: false,
                },
                s.face.as_str(),
                s.is_changed(),
            ),
            (None, Some(s)) => (
                FaceLight {
                    brightness: s.brightness,
                    color: s.color,
                    range: s.range,
                    angle: s.angle,
                    shadows: s.shadows,
                    enabled: s.enabled,
                    area: true,
                },
                s.face.as_str(),
                s.is_changed(),
            ),
            (None, None) => continue,
        };
        // The part the light is in: its hierarchy parent, or the host the
        // Player recorded. Its size and scale place the emitter on the face.
        let parent_host = child_of.and_then(|c| hosts.get(c.parent()).ok());
        let host_changed = parent_host
            .as_ref()
            .is_some_and(|(bp, t)| bp.is_changed() || t.is_changed())
            || face_host.as_ref().is_some_and(|h| h.is_changed());
        let host = parent_host
            .as_ref()
            .map(|(bp, t)| (bp.size, t.scale))
            .or_else(|| face_host.as_ref().map(|h| (h.size, Vec3::ONE)));
        let budget_changed = budget.as_ref().is_some_and(|b| b.is_changed());
        let emitter = link.and_then(|l| emitters.get_mut(l.0).ok());
        let dirty = all || class_changed || budget_changed || host_changed || emitter.is_none();
        if !dirty {
            continue;
        }
        let (want, local) = face_emitter(&light, face, host, budget.as_deref(), &units);
        match emitter {
            Some((mut sl, mut t)) => {
                if sl.intensity != want.intensity {
                    sl.intensity = want.intensity;
                }
                if sl.color != want.color {
                    sl.color = want.color;
                }
                if sl.range != want.range {
                    sl.range = want.range;
                }
                if sl.radius != want.radius {
                    sl.radius = want.radius;
                }
                if sl.inner_angle != want.inner_angle || sl.outer_angle != want.outer_angle {
                    sl.inner_angle = want.inner_angle;
                    sl.outer_angle = want.outer_angle;
                }
                if sl.shadow_maps_enabled != want.shadow_maps_enabled {
                    sl.shadow_maps_enabled = want.shadow_maps_enabled;
                }
                if *t != local {
                    *t = local;
                }
            }
            None => {
                // The emitter inherits visibility, so the light needs its own.
                if !has_visibility {
                    commands.entity(entity).try_insert(Visibility::Inherited);
                }
                let child = commands
                    .spawn((
                        LightEmitter { owner: entity },
                        want,
                        local,
                        Visibility::Inherited,
                        Name::new("LightEmitter"),
                        ChildOf(entity),
                    ))
                    .id();
                commands.entity(entity).try_insert(LightEmitterLink(child));
            }
        }
    }
}

/// `EustressDirectionalLight` → `DirectionalLight` on the same entity. It
/// is not the sun, so Bevy's atmosphere must not draw a sun disc for it
/// (every directional light without a `SunDisk` gets `SunDisk::EARTH`).
fn sync_directional_lights(
    mut commands: Commands,
    units: Res<LightUnits>,
    mut lights: Query<(
        Entity,
        Ref<EustressDirectionalLight>,
        Option<&mut DirectionalLight>,
        Has<SunDisk>,
    )>,
) {
    let all = units.is_changed();
    for (entity, light, existing, has_disk) in &mut lights {
        if !(all || light.is_changed() || existing.is_none() || !has_disk) {
            continue;
        }
        let on = light.enabled;
        let illuminance = if on { directional_lux(light.brightness, &units) } else { 0.0 };
        let shadows = on && light.shadows;
        match existing {
            Some(mut dl) => {
                if dl.illuminance != illuminance {
                    dl.illuminance = illuminance;
                }
                if dl.color != light.color {
                    dl.color = light.color;
                }
                if dl.shadow_maps_enabled != shadows {
                    dl.shadow_maps_enabled = shadows;
                }
                if dl.shadow_depth_bias != light.shadow_depth_bias {
                    dl.shadow_depth_bias = light.shadow_depth_bias;
                }
                if dl.shadow_normal_bias != light.shadow_normal_bias {
                    dl.shadow_normal_bias = light.shadow_normal_bias;
                }
                if !has_disk {
                    commands.entity(entity).try_insert(SunDisk::OFF);
                }
            }
            None => {
                commands.entity(entity).try_insert((
                    DirectionalLight {
                        color: light.color,
                        illuminance,
                        shadow_maps_enabled: shadows,
                        shadow_depth_bias: light.shadow_depth_bias,
                        shadow_normal_bias: light.shadow_normal_bias,
                        ..default()
                    },
                    SunDisk::OFF,
                ));
            }
        }
    }
}

/// An emitter whose light is gone, or no longer a face light, goes too.
fn despawn_orphan_emitters(
    mut commands: Commands,
    emitters: Query<(Entity, &LightEmitter)>,
    owners: Query<(), Or<(With<EustressSpotLight>, With<SurfaceLight>)>>,
) {
    for (entity, emitter) in &emitters {
        if owners.get(emitter.owner).is_err() {
            if let Ok(mut e) = commands.get_entity(entity) {
                e.despawn();
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn units(ev100: f32) -> LightUnits {
        LightUnits { model: LightModel::Display, ev100, half_range_luminance: HALF_RANGE_LUMINANCE }
    }

    /// Exposed luminance of a white wall facing a light, the way Bevy's
    /// point light shader computes it (Lambert, inverse square, range window).
    fn wall(lumens: f32, range: f32, d: f32, ev100: f32) -> f32 {
        let window = (1.0 - (d / range).powi(4)).clamp(0.0, 1.0).powi(2);
        lumens / (4.0 * PI) * window / (d * d) / PI * exposure_factor(ev100)
    }

    #[test]
    fn a_lamp_looks_the_same_at_noon_and_at_midnight() {
        let noon = light_lumens(1.0, 16.0, &units(13.0));
        let night = light_lumens(1.0, 16.0, &units(4.0));
        // Physical power follows the exposure: ~512x apart...
        assert!((noon / night - 2f32.powi(9)).abs() < 1.0);
        // ...so the wall reads the same on screen.
        let a = wall(noon, 16.0, 8.0, 13.0);
        let b = wall(night, 16.0, 8.0, 4.0);
        assert!((a - b).abs() < 1e-4, "{a} vs {b}");
    }

    #[test]
    fn range_is_reach() {
        // Half range reads the calibration, whatever the range.
        for range in [2.4, 8.0, 16.0, 30.0] {
            let l = light_lumens(1.0, range, &units(13.0));
            let v = wall(l, range, range * 0.5, 13.0);
            assert!((v - HALF_RANGE_LUMINANCE).abs() < 1e-3, "range {range}: {v}");
        }
        // Brighter toward the light, dark at the range.
        let l = light_lumens(1.0, 16.0, &units(13.0));
        assert!(wall(l, 16.0, 4.0, 13.0) > 1.0);
        assert!(wall(l, 16.0, 12.0, 13.0) < 0.08);
        assert_eq!(wall(l, 16.0, 16.0, 13.0), 0.0);
    }

    #[test]
    fn nothing_near_a_light_overflows_half_float() {
        // The brightest light the caps allow, 5 cm from a white wall.
        let l = light_lumens(100.0, 1_000.0, &units(4.0));
        let v = wall(l, 1_000.0, 0.05, 4.0);
        assert!(v < 65_504.0, "{v}");
    }

    #[test]
    fn directional_brightness_one_is_ten_thousand_lux_by_day() {
        assert!((directional_lux(1.0, &units(13.0)) - 10_000.0).abs() < 1e-2);
        // Display-referred: at night exposure it is dimmed by the same stops.
        assert!((directional_lux(1.0, &units(4.0)) - 10_000.0 / 512.0).abs() < 1e-2);
    }

    #[test]
    fn physical_model_ignores_exposure() {
        let p = LightUnits { model: LightModel::Physical, ..units(4.0) };
        assert_eq!(light_lumens(2.0, 16.0, &p), 100_000.0);
        assert_eq!(directional_lux(2.0, &p), 20_000.0);
    }

    #[test]
    fn roblox_angle_is_the_full_cone() {
        let (inner, outer) = spot_cone(90.0);
        assert!((outer - 45f32.to_radians()).abs() < 1e-6);
        assert!(inner < outer);
        // 180 (a SurfaceLight's half-space) stays inside Bevy's limit.
        assert!(spot_cone(180.0).1 < PI / 2.0);
        assert!(spot_cone(0.0).1 > 0.0);
    }

    #[test]
    fn faces_parse_every_spelling() {
        assert_eq!(normalize_face("top"), "Top");
        assert_eq!(normalize_face("Enum.NormalId.Bottom"), "Bottom");
        assert_eq!(normalize_face("NormalId.Left"), "Left");
        assert_eq!(normalize_face("0"), "Right");
        assert_eq!(normalize_face("5"), "Front");
        assert_eq!(normalize_face("diagonal"), "Front");
    }

    #[test]
    fn the_emitter_sits_on_the_face_and_points_out() {
        // A 4 x 2 x 6 part, unit mesh scaled to its size.
        let size = Vec3::new(4.0, 2.0, 6.0);
        let t = emitter_transform("Bottom", Some((size, size)));
        assert!((t.translation - Vec3::new(0.0, -0.5, 0.0)).length() < 1e-5);
        assert!((t.rotation * Vec3::NEG_Z - Vec3::NEG_Y).length() < 1e-5);
        // A part whose mesh is baked at size (scale 1): half the size out.
        let t = emitter_transform("Right", Some((size, Vec3::ONE)));
        assert!((t.translation - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-5);
        // Front is the spot's own axis: no turn, no part, no offset.
        let t = emitter_transform("Front", None);
        assert_eq!(t.translation, Vec3::ZERO);
        assert!((t.rotation * Vec3::NEG_Z - Vec3::NEG_Z).length() < 1e-5);
    }

    #[test]
    fn legacy_import_extras_win_and_lose_their_lumens() {
        // A light imported before the `[light]` section, then healed: the
        // section holds template defaults, the extras the real values in
        // Roblox's units (brightness x800, a 16-stud range).
        let doc: toml::Value = toml::from_str(
            r#"
            [metadata]
            class_name = "SpotLight"
            [light]
            brightness = 1.0
            range = 16.0
            angle = 90.0
            face = "Front"
            [properties.extras]
            light_brightness = 1600.0
            light_range = 16.0
            light_angle = 60.0
            light_face = 4
            light_enabled = false
            "#,
        )
        .unwrap();
        let spot = LightSection::from_document(&doc).spot();
        assert!((spot.brightness - 2.0).abs() < 1e-6);
        assert!((spot.range - 4.8768).abs() < 1e-6);
        assert_eq!(spot.angle, 60.0);
        assert_eq!(spot.face, "Bottom");
        assert!(!spot.enabled);
    }

    #[test]
    fn the_section_reads_every_spelling() {
        let doc: toml::Value = toml::from_str(
            r#"
            [Light]
            Brightness = { type = "float", value = 3.0 }
            Color = [255, 128, 0]
            Range = 12
            Shadows = true
            "#,
        )
        .unwrap();
        let p = LightSection::from_document(&doc).point();
        assert_eq!(p.brightness, 3.0);
        assert_eq!(p.range, 12.0);
        assert!(p.shadows);
        let c = p.color.to_srgba();
        assert!((c.green - 128.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn storing_a_section_folds_the_legacy_keys_away() {
        let mut doc: toml::Value = toml::from_str(
            r#"
            [light]
            texture = "cookie.ktx2"
            [properties.extras]
            light_brightness = 800.0
            imported_tag = "keep"
            "#,
        )
        .unwrap();
        let light = EustressPointLight { brightness: 2.0, ..Default::default() };
        store_light_section(&mut doc, light_section_table(&LightSectionRef::Point(&light)));
        let section = doc.get("light").unwrap();
        assert_eq!(section.get("brightness").and_then(|v| v.as_float()), Some(2.0));
        assert_eq!(section.get("texture").and_then(|v| v.as_str()), Some("cookie.ktx2"));
        let extras = doc.get("properties").and_then(|p| p.get("extras")).unwrap();
        assert!(extras.get("light_brightness").is_none());
        assert!(extras.get("imported_tag").is_some());
        // And it reads back as it was stored.
        assert_eq!(LightSection::from_document(&doc).point().brightness, 2.0);
    }

    #[test]
    fn transforms_keep_rotation_and_integer_positions() {
        let doc: toml::Value = toml::from_str(
            r#"
            [metadata]
            unit = "ft"
            [transform]
            position = [10, 0, 0]
            rotation = [-0.7071068, 0.0, 0.0, 0.7071068]
            "#,
        )
        .unwrap();
        let t = light_transform_from_document(&doc);
        // Feet to metres, as parts convert (with `units_v1`, the default).
        let expected_x = if cfg!(feature = "units_v1") { 3.048 } else { 10.0 };
        assert!((t.translation.x - expected_x).abs() < 1e-4);
        assert!((t.rotation * Vec3::NEG_Z - Vec3::NEG_Y).length() < 1e-4);
        assert_eq!(t.scale, Vec3::ONE);
    }
}

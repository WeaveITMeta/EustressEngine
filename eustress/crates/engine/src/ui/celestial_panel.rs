//! Properties panel for the Lighting children with their own sections: the
//! Sun (class Star), the Moon, the Sky, the Atmosphere and the Clouds.
//!
//! Rows come from the live class component, so the panel shows what the
//! renderer is using, not a Part's Appearance, Physics and Transform (a Sun
//! has no Material, and its place is set by Lighting's clock), and not the
//! old Lighting templates' descriptor tables folded into Attributes, which is
//! what it showed before.
//!
//! An edit is parsed, written to the component (the renderer follows it the
//! same frame), saved into the file's own section (`[star]`, `[moon]`,
//! `[sky]`, `[atmosphere]`, `[clouds]`, see `celestial_sections`) and pushed on the undo
//! stack as a `ChangeClassField`, whose replay comes back through
//! [`set_field_text`]. The pattern is `light_panel`'s.

use std::path::Path;

use bevy::ecs::component::Mutable;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use eustress_common::classes::{Atmosphere, ClassName, Clouds, Moon as MoonClass, Sky, Sun as SunClass};
use eustress_common::plugins::celestial_sections::{
    artistic, atmosphere_section, cloud_coverage_name, cloud_coverage_named, cloud_layer_type_name,
    cloud_layer_type_named, clouds_section, is_celestial_class, moon_section, sky_section, star_section,
    store_section, CLOUD_COVERAGE_MODES, CLOUD_LAYER_TYPES, MAX_STAR_COUNT,
};
use eustress_common::services::lighting::{AtmosphereRenderingMode, EustressAtmosphere};

use super::light_panel::EditOutcome;

/// The choices the Atmosphere's RenderingMode row offers.
pub fn rendering_mode_options() -> Vec<slint::SharedString> {
    vec!["LookupTexture".into(), "Raymarched".into()]
}

/// The choices a Clouds object's LayerType row offers.
pub fn cloud_layer_type_options() -> Vec<slint::SharedString> {
    CLOUD_LAYER_TYPES.iter().map(|(name, _)| (*name).into()).collect()
}

/// The choices a Clouds object's CoverageMode row offers.
pub fn cloud_coverage_options() -> Vec<slint::SharedString> {
    CLOUD_COVERAGE_MODES.iter().map(|(name, _)| (*name).into()).collect()
}

/// The choices the Moon's Phase row offers.
pub fn moon_phase_options() -> Vec<slint::SharedString> {
    MOON_PHASES.iter().map(|name| (*name).into()).collect()
}

/// The Moon's eight named phases in order through the synodic month, each
/// centred on its eighth of the month (New Moon on day 0, Full Moon on day
/// 14.77).
const MOON_PHASES: [&str; 8] = [
    "New Moon",
    "Waxing Crescent",
    "First Quarter",
    "Waxing Gibbous",
    "Full Moon",
    "Waning Gibbous",
    "Last Quarter",
    "Waning Crescent",
];

/// The named phase nearest a lunar day.
fn phase_name(lunar_day: f32) -> &'static str {
    let month = MoonClass::SYNODIC_MONTH;
    let eighth = (lunar_day.rem_euclid(month) / month * 8.0).round() as usize;
    MOON_PHASES[eighth % 8]
}

/// The lunar day in the middle of a named phase, the name in any case and
/// with or without spaces.
fn phase_day(name: &str) -> Result<f32, String> {
    let flat = |s: &str| s.replace([' ', '_'], "").to_ascii_lowercase();
    MOON_PHASES
        .iter()
        .position(|phase| flat(phase) == flat(name))
        .map(|i| i as f32 * MoonClass::SYNODIC_MONTH / 8.0)
        .ok_or_else(|| format!("expected a phase such as Full Moon, got {:?}", name.trim()))
}

// ============================================================================
// Text forms
// ============================================================================

fn fmt_f32(v: f32) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".to_string() } else { s.to_string() }
}

/// `r, g, b` in 0-255.
fn color_text(c: [f32; 4]) -> String {
    let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("{}, {}, {}", ch(c[0]), ch(c[1]), ch(c[2]))
}

fn parse_bool(text: &str) -> Result<bool, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        other => Err(format!("expected true or false, got {other:?}")),
    }
}

fn parse_number(text: &str) -> Result<f32, String> {
    let v: f32 = text.trim().parse().map_err(|_| format!("expected a number, got {:?}", text.trim()))?;
    if v.is_finite() { Ok(v) } else { Err("expected a finite number".into()) }
}

fn parse_non_negative(text: &str) -> Result<f32, String> {
    Ok(parse_number(text)?.max(0.0))
}

/// `#RRGGBB`, `RRGGBB`, or `r, g, b` in 0-255 (or 0-1 when every channel is
/// a fraction written with a decimal point). Alpha is kept from `old`.
fn parse_color(text: &str, old: [f32; 4]) -> Result<[f32; 4], String> {
    let t = text.trim();
    let hex = t.trim_start_matches('#');
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let b = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
        return Ok([b(0), b(2), b(4), old[3]]);
    }
    let parts: Vec<&str> = t.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.len() < 3 {
        return Err(format!("expected r, g, b, got {t:?}"));
    }
    let mut ch = [0f32; 3];
    for (i, p) in parts.iter().take(3).enumerate() {
        ch[i] = parse_number(p).map_err(|_| format!("bad colour channel {p:?}"))?;
    }
    let fractions = parts.iter().take(3).all(|p| p.contains('.')) && ch.iter().all(|c| *c <= 1.0);
    let scale = if fractions { 1.0 } else { 1.0 / 255.0 };
    Ok([
        (ch[0] * scale).clamp(0.0, 1.0),
        (ch[1] * scale).clamp(0.0, 1.0),
        (ch[2] * scale).clamp(0.0, 1.0),
        old[3],
    ])
}

/// Three numbers, `x, y, z`.
fn parse_triple(text: &str) -> Result<[f32; 3], String> {
    let parts: Vec<&str> = text.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.len() != 3 {
        return Err(format!("expected three numbers, got {:?}", text.trim()));
    }
    Ok([parse_number(parts[0])?, parse_number(parts[1])?, parse_number(parts[2])?])
}

// ============================================================================
// Field access
// ============================================================================

/// Text in, text out: every field in the form the undo stack stores it.
trait CelestialFields {
    const CLASS: ClassName;
    /// The panel's rows, in order: `(category, name, kind)`.
    const ROWS: &'static [(&'static str, &'static str, &'static str)];
    fn get_text(&self, key: &str) -> Option<String>;
    fn set_text(&mut self, key: &str, text: &str) -> Result<(), String>;
    fn section(&self) -> toml::value::Table;
    /// The field an edit of `key` is undone through. A row that shows a
    /// rounded view of another field (the Moon's Phase of its LunarDay) is
    /// undone through that field, so undo restores the exact value.
    fn undo_key(key: &str) -> &str {
        key
    }
}

impl CelestialFields for SunClass {
    const CLASS: ClassName = ClassName::Star;
    const ROWS: &'static [(&'static str, &'static str, &'static str)] = &[
        ("Appearance", "Enabled", "bool"),
        ("Appearance", "Color", "color"),
        ("Appearance", "AngularSize", "float"),
        ("Light", "Intensity", "float"),
        ("Light", "CastShadows", "bool"),
        ("Light", "GodRays", "float"),
        ("Orbit", "DayOfYear", "float"),
    ];
    fn get_text(&self, key: &str) -> Option<String> {
        Some(match key {
            "Enabled" => self.enabled.to_string(),
            "Color" => color_text(self.noon_color),
            "AngularSize" => fmt_f32(self.angular_size),
            "Intensity" => fmt_f32(self.noon_intensity),
            "CastShadows" => self.cast_shadows.to_string(),
            "GodRays" => fmt_f32(self.god_rays_intensity),
            "DayOfYear" => self.day_of_year.to_string(),
            _ => return None,
        })
    }
    fn set_text(&mut self, key: &str, text: &str) -> Result<(), String> {
        match key {
            "Enabled" => self.enabled = parse_bool(text)?,
            "Color" => self.noon_color = parse_color(text, self.noon_color)?,
            "AngularSize" => self.angular_size = parse_number(text)?.clamp(0.05, 20.0),
            "Intensity" => self.noon_intensity = parse_non_negative(text)?,
            "CastShadows" => self.cast_shadows = parse_bool(text)?,
            "GodRays" => self.god_rays_intensity = parse_non_negative(text)?,
            "DayOfYear" => self.day_of_year = parse_number(text)?.round().clamp(1.0, 365.0) as u16,
            _ => return Err(format!("the Sun has no {key}")),
        }
        Ok(())
    }
    fn section(&self) -> toml::value::Table {
        star_section(self)
    }
}

impl CelestialFields for MoonClass {
    const CLASS: ClassName = ClassName::Moon;
    const ROWS: &'static [(&'static str, &'static str, &'static str)] = &[
        ("Appearance", "Enabled", "bool"),
        ("Appearance", "Color", "color"),
        ("Appearance", "AngularSize", "float"),
        ("Appearance", "Glow", "float"),
        ("Appearance", "Earthshine", "float"),
        ("Light", "Intensity", "float"),
        ("Light", "CastShadows", "bool"),
        ("Orbit", "Phase", "choice"),
        ("Orbit", "LunarDay", "float"),
        ("Orbit", "FollowsCalendar", "bool"),
        ("Orbit", "OrbitalInclination", "float"),
        ("Orbit", "AscendingNode", "float"),
    ];
    fn get_text(&self, key: &str) -> Option<String> {
        Some(match key {
            "Enabled" => self.enabled.to_string(),
            "Color" => color_text(self.color),
            "AngularSize" => fmt_f32(self.angular_size),
            "Glow" => fmt_f32(self.glow_intensity),
            "Earthshine" => fmt_f32(self.earthshine_intensity),
            "Intensity" => fmt_f32(self.full_intensity),
            "CastShadows" => self.cast_shadows.to_string(),
            "Phase" => phase_name(self.lunar_day).to_string(),
            "LunarDay" => fmt_f32(self.lunar_day),
            "FollowsCalendar" => self.sync_with_sun.to_string(),
            "OrbitalInclination" => fmt_f32(self.orbital_inclination),
            "AscendingNode" => fmt_f32(self.ascending_node),
            _ => return None,
        })
    }
    fn set_text(&mut self, key: &str, text: &str) -> Result<(), String> {
        match key {
            "Enabled" => self.enabled = parse_bool(text)?,
            "Color" => self.color = parse_color(text, self.color)?,
            "AngularSize" => self.angular_size = parse_number(text)?.clamp(0.05, 20.0),
            "Glow" => self.glow_intensity = parse_non_negative(text)?,
            "Earthshine" => self.earthshine_intensity = parse_non_negative(text)?,
            "Intensity" => self.full_intensity = parse_non_negative(text)?,
            "CastShadows" => self.cast_shadows = parse_bool(text)?,
            "Phase" => self.lunar_day = phase_day(text)?,
            "LunarDay" => self.lunar_day = parse_number(text)?.rem_euclid(MoonClass::SYNODIC_MONTH),
            "FollowsCalendar" => self.sync_with_sun = parse_bool(text)?,
            "OrbitalInclination" => self.orbital_inclination = parse_number(text)?.clamp(-90.0, 90.0),
            "AscendingNode" => self.ascending_node = parse_number(text)?.rem_euclid(360.0),
            _ => return Err(format!("the Moon has no {key}")),
        }
        Ok(())
    }
    fn section(&self) -> toml::value::Table {
        moon_section(self)
    }
    fn undo_key(key: &str) -> &str {
        if key == "Phase" { "LunarDay" } else { key }
    }
}

impl CelestialFields for Sky {
    const CLASS: ClassName = ClassName::Sky;
    // Roblox's names for the six faces.
    const ROWS: &'static [(&'static str, &'static str, &'static str)] = &[
        ("Appearance", "CelestialBodiesShown", "bool"),
        ("Appearance", "StarCount", "float"),
        ("Skybox", "SkyboxBk", "string"),
        ("Skybox", "SkyboxDn", "string"),
        ("Skybox", "SkyboxFt", "string"),
        ("Skybox", "SkyboxLf", "string"),
        ("Skybox", "SkyboxRt", "string"),
        ("Skybox", "SkyboxUp", "string"),
    ];
    fn get_text(&self, key: &str) -> Option<String> {
        let t = &self.skybox_textures;
        Some(match key {
            "CelestialBodiesShown" => self.celestial_bodies_shown.to_string(),
            "StarCount" => self.star_count.to_string(),
            "SkyboxBk" => t.back.clone(),
            "SkyboxDn" => t.down.clone(),
            "SkyboxFt" => t.front.clone(),
            "SkyboxLf" => t.left.clone(),
            "SkyboxRt" => t.right.clone(),
            "SkyboxUp" => t.up.clone(),
            _ => return None,
        })
    }
    fn set_text(&mut self, key: &str, text: &str) -> Result<(), String> {
        let face = text.trim().to_string();
        let t = &mut self.skybox_textures;
        match key {
            "CelestialBodiesShown" => self.celestial_bodies_shown = parse_bool(text)?,
            "StarCount" => {
                self.star_count = parse_number(text)?.round().clamp(0.0, MAX_STAR_COUNT as f32) as u32
            }
            "SkyboxBk" => t.back = face,
            "SkyboxDn" => t.down = face,
            "SkyboxFt" => t.front = face,
            "SkyboxLf" => t.left = face,
            "SkyboxRt" => t.right = face,
            "SkyboxUp" => t.up = face,
            _ => return Err(format!("the Sky has no {key}")),
        }
        Ok(())
    }
    fn section(&self) -> toml::value::Table {
        sky_section(self)
    }
}

impl CelestialFields for EustressAtmosphere {
    const CLASS: ClassName = ClassName::Atmosphere;
    const ROWS: &'static [(&'static str, &'static str, &'static str)] = &[
        ("Appearance", "Density", "float"),
        ("Appearance", "Offset", "float"),
        ("Appearance", "Color", "color"),
        ("Appearance", "Decay", "color"),
        ("Appearance", "Glare", "float"),
        ("Appearance", "Haze", "float"),
        ("Scattering", "RayleighCoefficient", "string"),
        ("Scattering", "MieCoefficient", "float"),
        ("Scattering", "MieDirection", "float"),
        // Metres whatever the display unit: the planet is never measured
        // against the Space's parts.
        ("Planet", "PlanetRadius", "float"),
        ("Planet", "AtmosphereHeight", "float"),
        ("Rendering", "RenderingMode", "choice"),
        ("Rendering", "EnvironmentIntensity", "float"),
    ];
    fn get_text(&self, key: &str) -> Option<String> {
        Some(match key {
            "Density" => fmt_f32(self.density),
            "Offset" => fmt_f32(self.offset),
            "Color" => color_text(self.color),
            "Decay" => color_text(self.decay),
            "Glare" => fmt_f32(self.glare),
            "Haze" => fmt_f32(self.haze),
            "RayleighCoefficient" => {
                let r = self.rayleigh_coefficient;
                format!("{}, {}, {}", fmt_f32(r[0]), fmt_f32(r[1]), fmt_f32(r[2]))
            }
            "MieCoefficient" => fmt_f32(self.mie_coefficient),
            "MieDirection" => fmt_f32(self.mie_direction),
            "PlanetRadius" => fmt_f32(self.planet_radius),
            "AtmosphereHeight" => fmt_f32(self.atmosphere_height),
            "RenderingMode" => match self.rendering_mode {
                AtmosphereRenderingMode::LookupTexture => "LookupTexture".to_string(),
                AtmosphereRenderingMode::Raymarched => "Raymarched".to_string(),
            },
            "EnvironmentIntensity" => fmt_f32(self.environment_intensity),
            _ => return None,
        })
    }
    fn set_text(&mut self, key: &str, text: &str) -> Result<(), String> {
        match key {
            "Density" => self.density = parse_non_negative(text)?,
            "Offset" => self.offset = parse_number(text)?,
            "Color" => self.color = parse_color(text, self.color)?,
            "Decay" => self.decay = parse_color(text, self.decay)?,
            "Glare" => self.glare = parse_non_negative(text)?,
            "Haze" => self.haze = parse_non_negative(text)?,
            "RayleighCoefficient" => self.rayleigh_coefficient = parse_triple(text)?.map(|c| c.max(0.0)),
            "MieCoefficient" => self.mie_coefficient = parse_non_negative(text)?,
            "MieDirection" => self.mie_direction = parse_number(text)?.clamp(-0.99, 0.99),
            "PlanetRadius" => self.planet_radius = parse_number(text)?.max(1_000.0),
            "AtmosphereHeight" => self.atmosphere_height = parse_number(text)?.max(1_000.0),
            "RenderingMode" => {
                self.rendering_mode = match text.trim().to_ascii_lowercase().as_str() {
                    "raymarched" | "raymarch" => AtmosphereRenderingMode::Raymarched,
                    "lookuptexture" | "lookup" => AtmosphereRenderingMode::LookupTexture,
                    other => return Err(format!("expected LookupTexture or Raymarched, got {other:?}")),
                }
            }
            "EnvironmentIntensity" => self.environment_intensity = parse_non_negative(text)?,
            _ => return Err(format!("the Atmosphere has no {key}")),
        }
        Ok(())
    }
    fn section(&self) -> toml::value::Table {
        atmosphere_section(self)
    }
}

impl CelestialFields for Clouds {
    const CLASS: ClassName = ClassName::Clouds;
    const ROWS: &'static [(&'static str, &'static str, &'static str)] = &[
        ("Appearance", "Enabled", "bool"),
        ("Appearance", "Cover", "float"),
        ("Appearance", "Density", "float"),
        ("Appearance", "Color", "color"),
        ("Appearance", "ShadowColor", "color"),
        ("Appearance", "LayerType", "choice"),
        ("Appearance", "Softness", "float"),
        ("Appearance", "Spread", "float"),
        ("Appearance", "TimeOfDayTint", "bool"),
        // Metres whatever the display unit: the layer is kilometres up and
        // never measured against the Space's parts.
        ("Layer", "Altitude", "float"),
        ("Layer", "Thickness", "float"),
        ("Layer", "CoverageMode", "choice"),
        ("Layer", "CoverageBias", "float"),
        ("Wind", "WindSpeed", "float"),
        ("Wind", "WindDirection", "float"),
        ("Wind", "AnimationSpeed", "float"),
    ];
    fn get_text(&self, key: &str) -> Option<String> {
        Some(match key {
            "Enabled" => self.enabled.to_string(),
            "Cover" => fmt_f32(self.coverage),
            "Density" => fmt_f32(self.density),
            "Color" => color_text(self.color),
            "ShadowColor" => color_text(self.shadow_color),
            "LayerType" => cloud_layer_type_name(self.layer_type).to_string(),
            "Softness" => fmt_f32(self.softness),
            "Spread" => fmt_f32(self.spread),
            "TimeOfDayTint" => self.time_of_day_tinting.to_string(),
            "Altitude" => fmt_f32(self.altitude),
            "Thickness" => fmt_f32(self.thickness),
            "CoverageMode" => cloud_coverage_name(self.coverage_mode).to_string(),
            "CoverageBias" => fmt_f32(self.coverage_bias),
            "WindSpeed" => fmt_f32(self.wind_speed),
            "WindDirection" => fmt_f32(self.wind_direction),
            "AnimationSpeed" => fmt_f32(self.animation_speed),
            _ => return None,
        })
    }
    fn set_text(&mut self, key: &str, text: &str) -> Result<(), String> {
        let unit = |text: &str| parse_number(text).map(|v| v.clamp(0.0, 1.0));
        match key {
            "Enabled" => self.enabled = parse_bool(text)?,
            "Cover" => self.coverage = unit(text)?,
            "Density" => self.density = unit(text)?,
            "Color" => self.color = parse_color(text, self.color)?,
            "ShadowColor" => self.shadow_color = parse_color(text, self.shadow_color)?,
            "LayerType" => {
                self.layer_type = cloud_layer_type_named(text.trim()).ok_or_else(|| {
                    format!("expected Cumulus, Cirrus, Stratus, Cumulonimbus or Altocumulus, got {:?}", text.trim())
                })?
            }
            "Softness" => self.softness = unit(text)?,
            "Spread" => self.spread = parse_number(text)?.max(0.05),
            "TimeOfDayTint" => self.time_of_day_tinting = parse_bool(text)?,
            "Altitude" => self.altitude = parse_number(text)?.max(50.0),
            "Thickness" => self.thickness = parse_number(text)?.max(50.0),
            "CoverageMode" => {
                self.coverage_mode = cloud_coverage_named(text.trim()).ok_or_else(|| {
                    format!("expected Full, Scattered, Horizon, Zenith or a compass side, got {:?}", text.trim())
                })?
            }
            "CoverageBias" => self.coverage_bias = unit(text)?,
            "WindSpeed" => self.wind_speed = parse_non_negative(text)?,
            "WindDirection" => self.wind_direction = parse_number(text)?.rem_euclid(360.0),
            "AnimationSpeed" => self.animation_speed = parse_non_negative(text)?,
            _ => return Err(format!("the Clouds have no {key}")),
        }
        Ok(())
    }
    fn section(&self) -> toml::value::Table {
        clouds_section(self)
    }
}

// ============================================================================
// Rows
// ============================================================================

/// Read access for building rows.
#[derive(SystemParam)]
pub struct PanelQueries<'w, 's> {
    bodies: Query<
        'w,
        's,
        (
            Option<&'static SunClass>,
            Option<&'static MoonClass>,
            Option<&'static Sky>,
            Option<&'static EustressAtmosphere>,
            Option<&'static Clouds>,
        ),
    >,
}

fn rows_of<T: CelestialFields>(body: &T) -> Vec<(&'static str, &'static str, String, &'static str)> {
    T::ROWS
        .iter()
        .filter_map(|(category, name, kind)| Some((*category, *name, body.get_text(name)?, *kind)))
        .collect()
}

/// Property rows `(category, name, value, kind)` for a Sun, Moon, Sky,
/// Atmosphere or Clouds, from its live component. `None` for any other class, or one
/// whose component has not been attached yet.
pub fn celestial_rows(
    class: ClassName,
    entity: Entity,
    queries: &PanelQueries,
) -> Option<Vec<(&'static str, &'static str, String, &'static str)>> {
    if !is_celestial_class(class) {
        return None;
    }
    let (sun, moon, sky, atmosphere, clouds) = queries.bodies.get(entity).ok()?;
    match class {
        ClassName::Star => sun.map(rows_of),
        ClassName::Moon => moon.map(rows_of),
        ClassName::Sky => sky.map(rows_of),
        ClassName::Atmosphere => atmosphere.map(rows_of),
        ClassName::Clouds => clouds.map(rows_of),
        _ => None,
    }
}

// ============================================================================
// Edits
// ============================================================================

/// Write access for panel edits. It holds no resources, and no `Instance`
/// (the drain system's bundle holds that mutably; a second access is a
/// startup panic), so the class is told by the component it carries: each
/// of the five has exactly one of them.
#[derive(SystemParam)]
pub struct EditQueries<'w, 's> {
    bodies: Query<
        'w,
        's,
        (
            Option<&'static mut SunClass>,
            Option<&'static mut MoonClass>,
            Option<&'static mut Sky>,
            Option<&'static mut EustressAtmosphere>,
            Option<&'static mut Atmosphere>,
            Option<&'static mut Clouds>,
        ),
    >,
}

/// Apply a Properties edit if `key` is a field of the selected Sun, Moon,
/// Sky, Atmosphere or Clouds. `None` means "not ours" (Name, a key the class does
/// not have): the generic handler takes it.
pub fn handle_edit(
    entity: Entity,
    key: &str,
    raw: &str,
    toml_path: Option<&Path>,
    queries: &mut EditQueries,
) -> Option<Result<EditOutcome, String>> {
    let (sun, moon, sky, atmosphere, explorer_atmosphere, clouds) = queries.bodies.get_mut(entity).ok()?;
    if let Some(mut sun) = sun {
        return apply(&mut *sun, key, raw, toml_path);
    }
    if let Some(mut moon) = moon {
        return apply(&mut *moon, key, raw, toml_path);
    }
    if let Some(mut sky) = sky {
        return apply(&mut *sky, key, raw, toml_path);
    }
    if let Some(mut clouds) = clouds {
        return apply(&mut *clouds, key, raw, toml_path);
    }
    let mut model = atmosphere?;
    let outcome = apply(&mut *model, key, raw, toml_path);
    // The Explorer class mirrors the model's six artistic fields.
    if matches!(outcome, Some(Ok(_))) {
        if let Some(mut mirror) = explorer_atmosphere {
            *mirror = artistic(&model);
        }
    }
    outcome
}

fn apply<T: CelestialFields>(
    body: &mut T,
    key: &str,
    raw: &str,
    toml_path: Option<&Path>,
) -> Option<Result<EditOutcome, String>> {
    if !T::ROWS.iter().any(|(_, name, _)| *name == key) {
        return None;
    }
    let stored = T::undo_key(key);
    let old_text = body.get_text(stored).unwrap_or_default();
    if let Err(e) = body.set_text(key, raw) {
        return Some(Err(format!("{key}: {e}")));
    }
    let new_text = body.get_text(stored).unwrap_or_default();
    if new_text == old_text {
        return Some(Ok(EditOutcome { message: String::new(), undo: None }));
    }
    let mut undo = None;
    if let Some(path) = toml_path {
        if let Err(e) = save_section(path, T::CLASS, body.section()) {
            return Some(Err(format!("{key} changed but was not saved: {e}")));
        }
        undo = Some(crate::undo::Action::ChangeClassField {
            toml_path: path.to_path_buf(),
            property: stored.to_string(),
            old_text,
            new_text,
        });
    }
    let shown = body.get_text(key).unwrap_or_default();
    Some(Ok(EditOutcome { message: format!("{key} = {shown}"), undo }))
}

/// Write a class's section into its instance file: the WorldDb copy when a
/// DB is active (authoritative on migrated Spaces) and the disk mirror.
/// Every other section is preserved, and the old Lighting templates'
/// descriptor sections are dropped, so the class's own section is the only
/// source.
pub fn save_section(toml_path: &Path, class: ClassName, section: toml::value::Table) -> Result<(), String> {
    let text = match crate::space::active_db::get_instance_text(toml_path) {
        Some(t) => t,
        None => std::fs::read_to_string(toml_path).map_err(|e| format!("read {}: {e}", toml_path.display()))?,
    };
    let mut doc: toml::Value = text.parse().map_err(|e| format!("parse {}: {e}", toml_path.display()))?;
    if !store_section(&mut doc, class, section) {
        return Err(format!("{} is not a TOML table", toml_path.display()));
    }
    let out = toml::to_string_pretty(&doc).map_err(|e| format!("serialize {}: {e}", toml_path.display()))?;
    let db_ok = crate::space::active_db::put_instance_text(toml_path, &out);
    if let Err(e) = crate::space::gui_loader::write_atomic(toml_path, out.as_bytes()) {
        if !db_ok {
            return Err(format!("write {}: {e}", toml_path.display()));
        }
    }
    Ok(())
}

/// Set one field from its stored text form on the Sun, Moon, Sky, Atmosphere
/// or Clouds whose file is `toml_path`, and save the section: how the undo
/// stack's `ChangeClassField` replays an edit. `None` when the entity is none
/// of them; otherwise the parse result, and inside it the save result.
pub fn set_field_text(
    world: &mut World,
    entity: Entity,
    toml_path: &Path,
    property: &str,
    text: &str,
) -> Option<Result<Option<Result<(), String>>, String>> {
    fn set<T: CelestialFields + Component<Mutability = Mutable>>(
        world: &mut World,
        entity: Entity,
        toml_path: &Path,
        property: &str,
        text: &str,
    ) -> Option<Result<Option<Result<(), String>>, String>> {
        let mut component = world.get_mut::<T>(entity)?;
        Some(
            component
                .set_text(property, text)
                .map(|_| Some(save_section(toml_path, T::CLASS, component.section()))),
        )
    }
    let class = world.get::<eustress_common::classes::Instance>(entity)?.class_name;
    let result = match class {
        ClassName::Star => set::<SunClass>(world, entity, toml_path, property, text),
        ClassName::Moon => set::<MoonClass>(world, entity, toml_path, property, text),
        ClassName::Sky => set::<Sky>(world, entity, toml_path, property, text),
        ClassName::Atmosphere => set::<EustressAtmosphere>(world, entity, toml_path, property, text),
        ClassName::Clouds => set::<Clouds>(world, entity, toml_path, property, text),
        _ => None,
    };
    if class == ClassName::Atmosphere {
        if let Some(mirror) = world.get::<EustressAtmosphere>(entity).map(artistic) {
            if let Some(mut explorer) = world.get_mut::<Atmosphere>(entity) {
                *explorer = mirror;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_reads_and_writes_back_its_own_text() {
        fn round_trip<T: CelestialFields>(mut body: T) {
            for (_, name, _) in T::ROWS {
                let text = body.get_text(name).unwrap_or_else(|| panic!("{name} has no text"));
                body.set_text(name, &text).unwrap_or_else(|e| panic!("{name} = {text:?}: {e}"));
                assert_eq!(body.get_text(name).as_deref(), Some(text.as_str()), "{name} drifts");
            }
        }
        round_trip(eustress_common::plugins::celestial_sections::default_sun());
        round_trip(MoonClass::default());
        round_trip(Sky::default());
        round_trip(EustressAtmosphere::default());
        round_trip(Clouds::default());
    }

    #[test]
    fn the_phase_row_names_the_nearest_phase_and_sets_its_middle() {
        assert_eq!(phase_name(0.3), "New Moon");
        assert_eq!(phase_name(29.4), "New Moon", "the month wraps");
        assert_eq!(phase_name(14.0), "Full Moon");
        assert_eq!(phase_name(8.0), "First Quarter");
        let mut moon = MoonClass::default();
        moon.set_text("Phase", "last quarter").unwrap();
        assert!((moon.lunar_day - 22.1475).abs() < 1e-3, "{}", moon.lunar_day);
        assert_eq!(moon.get_text("Phase").as_deref(), Some("Last Quarter"));
        assert!(moon.set_text("Phase", "blue moon").is_err());
    }

    #[test]
    fn a_phase_edit_is_undone_through_the_exact_lunar_day() {
        let mut moon = MoonClass { lunar_day: 12.3, ..MoonClass::default() };
        let done = apply(&mut moon, "Phase", "Full Moon", None).unwrap().unwrap();
        assert_eq!(done.message, "Phase = Full Moon");
        assert_eq!(MoonClass::undo_key("Phase"), "LunarDay");
        assert_eq!(Clouds::undo_key("Cover"), "Cover");
    }

    #[test]
    fn cloud_choices_are_read_by_name_and_clamped() {
        let mut clouds = Clouds::default();
        clouds.set_text("LayerType", "cirrus").unwrap();
        assert_eq!(clouds.get_text("LayerType").as_deref(), Some("Cirrus"));
        clouds.set_text("CoverageMode", "Horizon").unwrap();
        assert_eq!(clouds.get_text("CoverageMode").as_deref(), Some("Horizon"));
        assert!(clouds.set_text("LayerType", "fog").is_err());
        clouds.set_text("Cover", "3").unwrap();
        assert_eq!(clouds.coverage, 1.0);
        clouds.set_text("WindDirection", "-90").unwrap();
        assert_eq!(clouds.wind_direction, 270.0);
        assert_eq!(cloud_layer_type_options().len(), 5);
        assert_eq!(cloud_coverage_options().len(), 8);
        assert_eq!(moon_phase_options().len(), 8);
    }

    #[test]
    fn edits_are_clamped_to_what_the_renderer_takes() {
        let mut sun = SunClass::default();
        sun.set_text("DayOfYear", "400").unwrap();
        assert_eq!(sun.day_of_year, 365);
        sun.set_text("Intensity", "-5").unwrap();
        assert_eq!(sun.noon_intensity, 0.0);
        let mut moon = MoonClass::default();
        moon.set_text("LunarDay", "31").unwrap();
        assert!(moon.lunar_day < MoonClass::SYNODIC_MONTH);
        let mut atmosphere = EustressAtmosphere::default();
        atmosphere.set_text("RenderingMode", "raymarched").unwrap();
        assert_eq!(atmosphere.rendering_mode, AtmosphereRenderingMode::Raymarched);
        assert!(atmosphere.set_text("RenderingMode", "fast").is_err());
        assert!(atmosphere.set_text("RayleighCoefficient", "1, 2").is_err());
    }

    #[test]
    fn a_colour_keeps_its_alpha_and_takes_every_form() {
        let old = [0.0, 0.0, 0.0, 0.3];
        assert_eq!(parse_color("255, 0, 0", old).unwrap(), [1.0, 0.0, 0.0, 0.3]);
        assert_eq!(parse_color("#00FF00", old).unwrap()[1], 1.0);
        assert_eq!(parse_color("0.5, 0.25, 1.0", old).unwrap()[0], 0.5);
        assert!(parse_color("blue", old).is_err());
    }

    #[test]
    fn a_class_without_a_row_is_left_to_the_generic_handler() {
        let mut sky = Sky::default();
        assert!(apply(&mut sky, "Name", "Sky2", None).is_none());
        assert!(apply(&mut sky, "StarCount", "100", None).is_some());
        assert_eq!(sky.star_count, 100);
    }
}

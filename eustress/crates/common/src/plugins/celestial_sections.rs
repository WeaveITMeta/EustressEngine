//! The Lighting children's own TOML sections: a Star's `[star]`, a Moon's
//! `[moon]`, a Sky's `[sky]` and an Atmosphere's `[atmosphere]`.
//!
//! Each class keeps its authored values in one section named for it. The
//! loader reads the section into the class component when the instance
//! spawns, and the Properties panel writes the component back into it, the
//! way the light classes use `[light]` (see `light_classes`).
//!
//! ## What each object owns
//!
//! The Lighting service owns the clock, the latitude, the day cycle, the
//! overall Brightness and the global shadow switch. The Sun owns what the
//! sun itself is: its light above the atmosphere, its colour, its disc, its
//! shadows, its shafts and the date its path follows. The Moon owns its
//! light, disc, phase and orbit; the Sky its stars and cubemap; the
//! Atmosphere its scattering. Nothing else writes these values.
//!
//! ## The old template sections
//!
//! Spaces created before these sections carry the old Lighting templates'
//! `[Appearance]`, `[Position]`, `[Lighting]` and similar tables of
//! `{ type, value, description }` descriptors. Nothing ever read them into a
//! component: the loader folded them into Attributes, which is how a Sun came
//! to show a column of attributes that did nothing. Their values are not
//! carried over, because carrying them over would change how every such Space
//! looks today (the Atmosphere template's Mie coefficient of 21, for one, is
//! a thick haze). The class owns them, so they never become attributes, and
//! they are dropped when the file is healed or its section is next written.

use bevy::ecs::system::EntityCommands;
use bevy::light::light_consts::lux::RAW_SUNLIGHT;

use crate::classes::{Atmosphere, ClassName, Moon as MoonClass, Sky, SkyboxTextures, Sun as SunClass};
use crate::services::lighting::{AtmosphereRenderingMode, EustressAtmosphere};

/// A Star's section.
pub const STAR_SECTION: &str = "star";
/// A Moon's section.
pub const MOON_SECTION: &str = "moon";
/// A Sky's section.
pub const SKY_SECTION: &str = "sky";
/// An Atmosphere's section.
pub const ATMOSPHERE_SECTION: &str = "atmosphere";

/// The old Lighting templates' descriptor sections, compared with case and
/// underscores ignored (`TimeOfDay` and `time_of_day` alike).
pub const LEGACY_TEMPLATE_SECTIONS: [&str; 11] = [
    "appearance",
    "position",
    "lighting",
    "timeofday",
    "advanced",
    "phase",
    "proceduralsky",
    "clouds",
    "scattering",
    "rendering",
    "reflections",
];

/// The four Lighting children whose values live in their own section here.
pub fn is_celestial_class(class: ClassName) -> bool {
    matches!(class, ClassName::Star | ClassName::Moon | ClassName::Sky | ClassName::Atmosphere)
}

/// The section a class keeps its values in.
pub fn section_name(class: ClassName) -> Option<&'static str> {
    match class {
        ClassName::Star => Some(STAR_SECTION),
        ClassName::Moon => Some(MOON_SECTION),
        ClassName::Sky => Some(SKY_SECTION),
        ClassName::Atmosphere => Some(ATMOSPHERE_SECTION),
        _ => None,
    }
}

fn flat(name: &str) -> String {
    name.replace('_', "").to_ascii_lowercase()
}

fn is_legacy_section(name: &str) -> bool {
    LEGACY_TEMPLATE_SECTIONS.contains(&flat(name).as_str())
}

/// True when `section` belongs to `class` and so is read into its component
/// rather than folded into Attributes: its own section, in any key case, and
/// the old template sections.
pub fn owns_section(class: ClassName, section: &str) -> bool {
    let Some(own) = section_name(class) else { return false };
    flat(section) == own || is_legacy_section(section)
}

/// The class's section of an instance document, in any key case.
pub fn section_of<'a>(doc: &'a toml::Value, class: ClassName) -> Option<&'a toml::Value> {
    let own = section_name(class)?;
    let table = doc.as_table()?;
    table
        .get(own)
        .or_else(|| table.iter().find(|(k, _)| flat(k) == own).map(|(_, v)| v))
}

// ============================================================================
// Reading
// ============================================================================

/// A field of a section, in any key case, unwrapping an inline
/// `{ type, value }` descriptor.
fn field<'a>(section: Option<&'a toml::Value>, key: &str) -> Option<&'a toml::Value> {
    let table = section?.as_table()?;
    let want = flat(key);
    let v = table
        .get(key)
        .or_else(|| table.iter().find(|(k, _)| flat(k) == want).map(|(_, v)| v))?;
    Some(match v {
        toml::Value::Table(t) => t.get("value").unwrap_or(v),
        _ => v,
    })
}

fn num(section: Option<&toml::Value>, key: &str) -> Option<f32> {
    let v = field(section, key)?;
    v.as_float()
        .or_else(|| v.as_integer().map(|i| i as f64))
        .map(|f| f as f32)
        .filter(|f| f.is_finite())
}

fn flag(section: Option<&toml::Value>, key: &str) -> Option<bool> {
    field(section, key)?.as_bool()
}

fn text(section: Option<&toml::Value>, key: &str) -> Option<String> {
    field(section, key)?.as_str().map(str::to_string)
}

/// A colour: all-integer channels are 0-255, anything with a float is 0-1
/// (the rule the part and light loaders use). Alpha is kept from `fallback`.
fn rgba(section: Option<&toml::Value>, key: &str, fallback: [f32; 4]) -> [f32; 4] {
    let Some(arr) = field(section, key).and_then(|v| v.as_array()) else { return fallback };
    if arr.len() < 3 {
        return fallback;
    }
    let all_int = arr.iter().take(3).all(|c| c.is_integer());
    let ch = |i: usize| -> Option<f32> {
        let c = arr[i].as_float().or_else(|| arr[i].as_integer().map(|n| n as f64))? as f32;
        let c = if all_int { c / 255.0 } else { c };
        c.is_finite().then(|| c.clamp(0.0, 1.0))
    };
    match (ch(0), ch(1), ch(2)) {
        (Some(r), Some(g), Some(b)) => [r, g, b, fallback[3]],
        _ => fallback,
    }
}

fn vec3(section: Option<&toml::Value>, key: &str) -> Option<[f32; 3]> {
    let arr = field(section, key)?.as_array()?;
    let ch = |i: usize| -> Option<f32> {
        let v = arr.get(i)?;
        let f = v.as_float().or_else(|| v.as_integer().map(|n| n as f64))? as f32;
        f.is_finite().then_some(f)
    };
    Some([ch(0)?, ch(1)?, ch(2)?])
}

/// The Sun from its `[star]` section. Fields the section does not set keep
/// `base`'s value, and so do the ones the Lighting service owns (the clock,
/// the latitude and the cycle).
pub fn sun_from_section(section: Option<&toml::Value>, base: SunClass) -> SunClass {
    SunClass {
        enabled: flag(section, "enabled").unwrap_or(base.enabled),
        noon_intensity: num(section, "intensity").map(|v| v.max(0.0)).unwrap_or(base.noon_intensity),
        noon_color: rgba(section, "color", base.noon_color),
        angular_size: num(section, "angular_size")
            .map(|v| v.clamp(0.05, 20.0))
            .unwrap_or(base.angular_size),
        cast_shadows: flag(section, "cast_shadows").unwrap_or(base.cast_shadows),
        god_rays_intensity: num(section, "god_rays").map(|v| v.max(0.0)).unwrap_or(base.god_rays_intensity),
        day_of_year: num(section, "day_of_year")
            .map(|v| v.round().clamp(1.0, 365.0) as u16)
            .unwrap_or(base.day_of_year),
        ..base
    }
}

/// The Moon from its `[moon]` section, over `base`.
pub fn moon_from_section(section: Option<&toml::Value>, base: MoonClass) -> MoonClass {
    MoonClass {
        enabled: flag(section, "enabled").unwrap_or(base.enabled),
        full_intensity: num(section, "intensity").map(|v| v.max(0.0)).unwrap_or(base.full_intensity),
        color: rgba(section, "color", base.color),
        angular_size: num(section, "angular_size")
            .map(|v| v.clamp(0.05, 20.0))
            .unwrap_or(base.angular_size),
        lunar_day: num(section, "lunar_day")
            .map(|v| v.rem_euclid(MoonClass::SYNODIC_MONTH))
            .unwrap_or(base.lunar_day),
        cast_shadows: flag(section, "cast_shadows").unwrap_or(base.cast_shadows),
        glow_intensity: num(section, "glow").map(|v| v.max(0.0)).unwrap_or(base.glow_intensity),
        earthshine_intensity: num(section, "earthshine")
            .map(|v| v.max(0.0))
            .unwrap_or(base.earthshine_intensity),
        orbital_inclination: num(section, "orbital_inclination")
            .map(|v| v.clamp(-90.0, 90.0))
            .unwrap_or(base.orbital_inclination),
        ascending_node: num(section, "ascending_node")
            .map(|v| v.rem_euclid(360.0))
            .unwrap_or(base.ascending_node),
        ..base
    }
}

/// The most stars a Sky may ask for; the field is built on a worker thread,
/// and past this it only costs time.
pub const MAX_STAR_COUNT: u32 = 100_000;

/// The Sky from its `[sky]` section, over `base`. The six faces use the
/// importer's key names.
pub fn sky_from_section(section: Option<&toml::Value>, base: Sky) -> Sky {
    let t = base.skybox_textures;
    Sky {
        skybox_textures: SkyboxTextures {
            back: text(section, "skybox_back").unwrap_or(t.back),
            front: text(section, "skybox_front").unwrap_or(t.front),
            left: text(section, "skybox_left").unwrap_or(t.left),
            right: text(section, "skybox_right").unwrap_or(t.right),
            up: text(section, "skybox_top").unwrap_or(t.up),
            down: text(section, "skybox_bottom").unwrap_or(t.down),
        },
        star_count: num(section, "star_count")
            .map(|v| v.round().clamp(0.0, MAX_STAR_COUNT as f32) as u32)
            .unwrap_or(base.star_count),
        celestial_bodies_shown: flag(section, "celestial_bodies_shown")
            .unwrap_or(base.celestial_bodies_shown),
    }
}

/// The atmosphere from its `[atmosphere]` section, over `base`: the full
/// model, and the Explorer class's six artistic fields mirrored from it so
/// either component reads the same values. The importer writes the horizon
/// colour as `decay_color`; older files use `decay`.
pub fn atmosphere_from_section(
    section: Option<&toml::Value>,
    base: &EustressAtmosphere,
) -> (Atmosphere, EustressAtmosphere) {
    let decay = if field(section, "decay_color").is_some() {
        rgba(section, "decay_color", base.decay)
    } else {
        rgba(section, "decay", base.decay)
    };
    let model = EustressAtmosphere {
        density: num(section, "density").map(|v| v.max(0.0)).unwrap_or(base.density),
        offset: num(section, "offset").unwrap_or(base.offset),
        color: rgba(section, "color", base.color),
        decay,
        glare: num(section, "glare").map(|v| v.max(0.0)).unwrap_or(base.glare),
        haze: num(section, "haze").map(|v| v.max(0.0)).unwrap_or(base.haze),
        rendering_mode: text(section, "rendering_mode")
            .map(|s| match flat(&s).as_str() {
                "raymarched" | "raymarch" => AtmosphereRenderingMode::Raymarched,
                _ => AtmosphereRenderingMode::LookupTexture,
            })
            .unwrap_or(base.rendering_mode),
        sky_max_samples: num(section, "sky_max_samples")
            .map(|v| v.round().clamp(8.0, 128.0) as u32)
            .unwrap_or(base.sky_max_samples),
        planet_radius: num(section, "planet_radius")
            .map(|v| v.max(1_000.0))
            .unwrap_or(base.planet_radius),
        atmosphere_height: num(section, "atmosphere_height")
            .map(|v| v.max(1_000.0))
            .unwrap_or(base.atmosphere_height),
        rayleigh_coefficient: vec3(section, "rayleigh_coefficient")
            .map(|v| v.map(|c| c.max(0.0)))
            .unwrap_or(base.rayleigh_coefficient),
        mie_coefficient: num(section, "mie_coefficient")
            .map(|v| v.max(0.0))
            .unwrap_or(base.mie_coefficient),
        mie_direction: num(section, "mie_direction")
            .or_else(|| num(section, "mie_directional_factor"))
            .map(|v| v.clamp(-0.99, 0.99))
            .unwrap_or(base.mie_direction),
        environment_map_enabled: flag(section, "environment_map_enabled")
            .unwrap_or(base.environment_map_enabled),
        environment_intensity: num(section, "environment_intensity")
            .map(|v| v.max(0.0))
            .unwrap_or(base.environment_intensity),
        atmosphere_environment_light: flag(section, "atmosphere_environment_light")
            .unwrap_or(base.atmosphere_environment_light),
    };
    (artistic(&model), model)
}

/// The Explorer class's six fields, from the model.
pub fn artistic(model: &EustressAtmosphere) -> Atmosphere {
    Atmosphere {
        density: model.density,
        offset: model.offset,
        color: model.color,
        decay: model.decay,
        glare: model.glare,
        haze: model.haze,
    }
}

/// The Sun a Space gets when its file sets nothing: [`SunClass::default`]
/// carrying the raw sunlight bevy's atmosphere is calibrated for.
pub fn default_sun() -> SunClass {
    SunClass { noon_intensity: RAW_SUNLIGHT, ..SunClass::default() }
}

/// Insert the class component for a Star, Moon, Sky or Atmosphere built from
/// its section: the load path's single entry. A Star and a Moon get their
/// light and marker from the engine's hydration, which keeps this component.
pub fn insert_class_component(entity: &mut EntityCommands, class: ClassName, section: Option<&toml::Value>) {
    match class {
        ClassName::Star => {
            entity.insert(sun_from_section(section, default_sun()));
        }
        ClassName::Moon => {
            entity.insert(moon_from_section(section, MoonClass::default()));
        }
        ClassName::Sky => {
            entity.insert(sky_from_section(section, Sky::default()));
        }
        ClassName::Atmosphere => {
            entity.insert(atmosphere_from_section(section, &EustressAtmosphere::default()));
        }
        _ => {}
    }
}

// ============================================================================
// Writing
// ============================================================================

/// A float as TOML, at the precision it was typed: `0.53`, not the
/// `0.5299999713897705` a plain widening writes.
fn float(v: f32) -> toml::Value {
    toml::Value::Float(format!("{v}").parse::<f64>().unwrap_or(v as f64))
}

fn color(c: [f32; 4]) -> toml::Value {
    toml::Value::Array(
        c[..3]
            .iter()
            .map(|v| toml::Value::Integer((v.clamp(0.0, 1.0) * 255.0).round() as i64))
            .collect(),
    )
}

fn table(entries: Vec<(&str, toml::Value)>) -> toml::value::Table {
    entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// A Sun's `[star]` section.
pub fn star_section(sun: &SunClass) -> toml::value::Table {
    table(vec![
        ("enabled", toml::Value::Boolean(sun.enabled)),
        ("intensity", float(sun.noon_intensity)),
        ("color", color(sun.noon_color)),
        ("angular_size", float(sun.angular_size)),
        ("cast_shadows", toml::Value::Boolean(sun.cast_shadows)),
        ("god_rays", float(sun.god_rays_intensity)),
        ("day_of_year", toml::Value::Integer(sun.day_of_year as i64)),
    ])
}

/// A Moon's `[moon]` section.
pub fn moon_section(moon: &MoonClass) -> toml::value::Table {
    table(vec![
        ("enabled", toml::Value::Boolean(moon.enabled)),
        ("intensity", float(moon.full_intensity)),
        ("color", color(moon.color)),
        ("angular_size", float(moon.angular_size)),
        ("lunar_day", float(moon.lunar_day)),
        ("cast_shadows", toml::Value::Boolean(moon.cast_shadows)),
        ("glow", float(moon.glow_intensity)),
        ("earthshine", float(moon.earthshine_intensity)),
        ("orbital_inclination", float(moon.orbital_inclination)),
        ("ascending_node", float(moon.ascending_node)),
    ])
}

/// A Sky's `[sky]` section.
pub fn sky_section(sky: &Sky) -> toml::value::Table {
    let t = &sky.skybox_textures;
    let s = |v: &str| toml::Value::String(v.to_string());
    table(vec![
        ("celestial_bodies_shown", toml::Value::Boolean(sky.celestial_bodies_shown)),
        ("star_count", toml::Value::Integer(sky.star_count as i64)),
        ("skybox_back", s(&t.back)),
        ("skybox_front", s(&t.front)),
        ("skybox_left", s(&t.left)),
        ("skybox_right", s(&t.right)),
        ("skybox_top", s(&t.up)),
        ("skybox_bottom", s(&t.down)),
    ])
}

/// An Atmosphere's `[atmosphere]` section.
pub fn atmosphere_section(a: &EustressAtmosphere) -> toml::value::Table {
    table(vec![
        ("density", float(a.density)),
        ("offset", float(a.offset)),
        ("color", color(a.color)),
        ("decay_color", color(a.decay)),
        ("glare", float(a.glare)),
        ("haze", float(a.haze)),
        (
            "rayleigh_coefficient",
            toml::Value::Array(a.rayleigh_coefficient.iter().map(|c| float(*c)).collect()),
        ),
        ("mie_coefficient", float(a.mie_coefficient)),
        ("mie_direction", float(a.mie_direction)),
        ("planet_radius", float(a.planet_radius)),
        ("atmosphere_height", float(a.atmosphere_height)),
        (
            "rendering_mode",
            toml::Value::String(
                match a.rendering_mode {
                    AtmosphereRenderingMode::Raymarched => "Raymarched",
                    AtmosphereRenderingMode::LookupTexture => "LookupTexture",
                }
                .to_string(),
            ),
        ),
        ("sky_max_samples", toml::Value::Integer(a.sky_max_samples as i64)),
        ("environment_map_enabled", toml::Value::Boolean(a.environment_map_enabled)),
        ("environment_intensity", float(a.environment_intensity)),
        ("atmosphere_environment_light", toml::Value::Boolean(a.atmosphere_environment_light)),
    ])
}

/// Put `section` in `doc` as `class`'s section, merged over what is there so
/// keys this module does not write (an import's `sun_angular_size`) survive,
/// and drop the old template sections. False when `doc` is not a table or
/// `class` has no section.
pub fn store_section(doc: &mut toml::Value, class: ClassName, section: toml::value::Table) -> bool {
    let Some(own) = section_name(class) else { return false };
    let Some(root) = doc.as_table_mut() else { return false };
    // One spelling of the section: fold a differently cased copy into it.
    let stray: Vec<String> = root.keys().filter(|k| k.as_str() != own && flat(k) == own).cloned().collect();
    let mut merged = root
        .remove(own)
        .and_then(|v| match v {
            toml::Value::Table(t) => Some(t),
            _ => None,
        })
        .unwrap_or_default();
    for key in stray {
        if let Some(toml::Value::Table(t)) = root.remove(&key) {
            for (k, v) in t {
                merged.entry(k).or_insert(v);
            }
        }
    }
    for (k, v) in section {
        merged.insert(k, v);
    }
    root.insert(own.to_string(), toml::Value::Table(merged));
    root.retain(|k, _| !is_legacy_section(k));
    true
}

/// Drop the old template sections from a Star, Moon, Sky or Atmosphere
/// document. The heal runs this, so a loaded file never carries them into
/// Attributes. True when anything was removed.
pub fn drop_legacy_template_sections(doc: &mut toml::Value) -> bool {
    let class = doc
        .get("metadata")
        .or_else(|| doc.get("Metadata"))
        .and_then(|m| m.get("class_name").or_else(|| m.get("ClassName")))
        .and_then(|c| c.as_str());
    if !matches!(class, Some("Star" | "Moon" | "Sky" | "Atmosphere")) {
        return false;
    }
    let Some(root) = doc.as_table_mut() else { return false };
    let before = root.len();
    root.retain(|k, v| !(v.is_table() && is_legacy_section(k)));
    root.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> toml::Value {
        text.parse::<toml::Value>().expect("test TOML parses")
    }

    fn schema(class: ClassName, text: &str) -> toml::Value {
        section_of(&doc(text), class).cloned().expect("the class schema has its section")
    }

    const STAR_SCHEMA: &str = include_str!("../../assets/class_schema/Star/_instance.toml");
    const MOON_SCHEMA: &str = include_str!("../../assets/class_schema/Moon/_instance.toml");
    const SKY_SCHEMA: &str = include_str!("../../assets/class_schema/Sky/_instance.toml");
    const ATMOSPHERE_SCHEMA: &str = include_str!("../../assets/class_schema/Atmosphere/_instance.toml");

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1.0 / 255.0 + 1e-4 * b.abs()
    }

    fn close4(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| close(*x, *y))
    }

    #[test]
    fn the_star_schema_is_the_default_sun() {
        // The heal merges the schema into every file, so a template that
        // disagreed with the Rust default would relight every Space.
        let d = default_sun();
        let s = sun_from_section(Some(&schema(ClassName::Star, STAR_SCHEMA)), SunClass { noon_intensity: 1.0, ..SunClass::default() });
        assert_eq!(s.enabled, d.enabled);
        assert!(close(s.noon_intensity, d.noon_intensity), "{} lux", s.noon_intensity);
        assert!(close4(s.noon_color, d.noon_color));
        assert!(close(s.angular_size, d.angular_size));
        assert_eq!(s.cast_shadows, d.cast_shadows);
        assert!(close(s.god_rays_intensity, d.god_rays_intensity));
        assert_eq!(s.day_of_year, d.day_of_year);
    }

    #[test]
    fn the_moon_schema_is_the_default_moon() {
        let d = MoonClass::default();
        let m = moon_from_section(Some(&schema(ClassName::Moon, MOON_SCHEMA)), MoonClass { angular_size: 5.0, ..d.clone() });
        assert!(close(m.angular_size, d.angular_size));
        assert!(close(m.full_intensity, d.full_intensity));
        assert!(close4(m.color, d.color));
        assert!(close(m.lunar_day, d.lunar_day));
        assert!(close(m.glow_intensity, d.glow_intensity));
        assert!(close(m.earthshine_intensity, d.earthshine_intensity));
        assert!(close(m.orbital_inclination, d.orbital_inclination));
    }

    #[test]
    fn the_sky_schema_is_the_default_sky() {
        let d = Sky::default();
        let s = sky_from_section(Some(&schema(ClassName::Sky, SKY_SCHEMA)), Sky { star_count: 1, ..d.clone() });
        assert_eq!(s.star_count, d.star_count);
        assert_eq!(s.celestial_bodies_shown, d.celestial_bodies_shown);
        assert!(s.skybox_textures.back.is_empty());
    }

    #[test]
    fn the_atmosphere_schema_is_the_default_atmosphere() {
        let d = EustressAtmosphere::default();
        let base = EustressAtmosphere { density: 9.0, mie_coefficient: 99.0, ..d.clone() };
        let (_, a) = atmosphere_from_section(Some(&schema(ClassName::Atmosphere, ATMOSPHERE_SCHEMA)), &base);
        assert!(close(a.density, d.density), "density {}", a.density);
        assert!(close4(a.color, d.color));
        assert!(close4(a.decay, d.decay));
        assert!(close(a.mie_coefficient, d.mie_coefficient));
        assert!(close(a.mie_direction, d.mie_direction));
        assert!(close(a.planet_radius, d.planet_radius));
        assert_eq!(a.rendering_mode, d.rendering_mode);
    }

    #[test]
    fn a_section_round_trips_through_its_writer() {
        let sun = SunClass { noon_intensity: 90_000.0, angular_size: 1.5, day_of_year: 30, cast_shadows: false, ..default_sun() };
        let back = sun_from_section(Some(&toml::Value::Table(star_section(&sun))), default_sun());
        assert_eq!((back.noon_intensity, back.angular_size, back.day_of_year, back.cast_shadows), (90_000.0, 1.5, 30, false));

        let moon = MoonClass { lunar_day: 3.25, glow_intensity: 0.7, ..MoonClass::default() };
        let back = moon_from_section(Some(&toml::Value::Table(moon_section(&moon))), MoonClass::default());
        assert_eq!((back.lunar_day, back.glow_intensity), (3.25, 0.7));

        let mut sky = Sky::default();
        sky.star_count = 1200;
        sky.skybox_textures.up = "sky/up.png".into();
        let back = sky_from_section(Some(&toml::Value::Table(sky_section(&sky))), Sky::default());
        assert_eq!((back.star_count, back.skybox_textures.up.as_str()), (1200, "sky/up.png"));

        let atmosphere = EustressAtmosphere {
            haze: 1.5,
            rendering_mode: AtmosphereRenderingMode::Raymarched,
            ..EustressAtmosphere::default()
        };
        let (class, back) =
            atmosphere_from_section(Some(&toml::Value::Table(atmosphere_section(&atmosphere))), &EustressAtmosphere::default());
        assert_eq!(back.haze, 1.5);
        assert_eq!(class.haze, 1.5, "the Explorer class mirrors the model");
        assert_eq!(back.rendering_mode, AtmosphereRenderingMode::Raymarched);
    }

    #[test]
    fn floats_are_written_as_typed() {
        let t = star_section(&SunClass { angular_size: 0.53, ..default_sun() });
        assert_eq!(t["angular_size"].as_float(), Some(0.53));
    }

    #[test]
    fn the_old_template_sections_are_owned_and_never_read() {
        // A Sun as the old Lighting template wrote it.
        let d = doc(
            "[metadata]\nclass_name = \"Star\"\n\n\
             [Advanced]\nGodRaysIntensity = { type = \"float\", value = 0.3 }\n\n\
             [TimeOfDay]\nSunriseTime = { type = \"float\", value = 6.0 }\n",
        );
        assert!(owns_section(ClassName::Star, "Advanced"));
        assert!(owns_section(ClassName::Star, "time_of_day"));
        assert!(owns_section(ClassName::Star, "Star"));
        assert!(!owns_section(ClassName::Part, "appearance"), "a Part's sections are its own business");
        let sun = sun_from_section(section_of(&d, ClassName::Star), default_sun());
        assert_eq!(sun.god_rays_intensity, default_sun().god_rays_intensity, "legacy values are not carried over");
    }

    #[test]
    fn storing_a_section_drops_the_old_templates_and_keeps_foreign_keys() {
        let mut d = doc(
            "[metadata]\nclass_name = \"Sky\"\n\n\
             [sky]\nsun_angular_size = 11.0\nstar_count = 3000\n\n\
             [Appearance]\nStarCount = { type = \"int\", value = 3000 }\n\n\
             [attributes]\nMood = \"calm\"\n",
        );
        let sky = Sky { star_count: 500, ..Sky::default() };
        assert!(store_section(&mut d, ClassName::Sky, sky_section(&sky)));
        assert_eq!(d["sky"]["star_count"].as_integer(), Some(500));
        assert_eq!(d["sky"]["sun_angular_size"].as_float(), Some(11.0), "an import's key survives");
        assert!(d.get("Appearance").is_none());
        assert!(d.get("attributes").is_some(), "real attributes are untouched");
    }

    #[test]
    fn the_heal_drops_old_template_sections_only_from_lighting_children() {
        let mut sun = doc("[metadata]\nclass_name = \"Star\"\n\n[Position]\nAzimuth = { type = \"float\", value = 135.0 }\n");
        assert!(drop_legacy_template_sections(&mut sun));
        assert!(sun.get("Position").is_none());
        let mut part = doc("[metadata]\nclass_name = \"Part\"\n\n[appearance]\ncolor = [1, 2, 3]\n");
        assert!(!drop_legacy_template_sections(&mut part));
        assert!(part.get("appearance").is_some());
    }
}

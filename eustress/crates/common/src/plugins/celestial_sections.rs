//! The Lighting children's own TOML sections: a Star's `[star]`, a Moon's
//! `[moon]`, a Sky's `[sky]`, an Atmosphere's `[atmosphere]` and a Clouds
//! object's `[clouds]`.
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
//! Atmosphere its scattering; the Clouds object its cloud layer. Nothing
//! else writes these values.
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
use bevy::prelude::{Commands, Name};
use bevy::light::light_consts::lux::RAW_SUNLIGHT;

use crate::classes::{
    Atmosphere, ClassName, CloudCoverage, CloudLayerType, Clouds, Moon as MoonClass, Sky, SkyboxTextures,
    Sun as SunClass,
};
use crate::services::lighting::{AtmosphereRenderingMode, EustressAtmosphere, LightingService};

/// A Star's section.
pub const STAR_SECTION: &str = "star";
/// A Moon's section.
pub const MOON_SECTION: &str = "moon";
/// A Sky's section.
pub const SKY_SECTION: &str = "sky";
/// An Atmosphere's section.
pub const ATMOSPHERE_SECTION: &str = "atmosphere";
/// A Clouds object's section.
pub const CLOUDS_SECTION: &str = "clouds";

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

/// The Lighting children whose values live in their own section here.
pub fn is_celestial_class(class: ClassName) -> bool {
    matches!(
        class,
        ClassName::Star | ClassName::Moon | ClassName::Sky | ClassName::Atmosphere | ClassName::Clouds
    )
}

/// The section a class keeps its values in.
pub fn section_name(class: ClassName) -> Option<&'static str> {
    match class {
        ClassName::Star => Some(STAR_SECTION),
        ClassName::Moon => Some(MOON_SECTION),
        ClassName::Sky => Some(SKY_SECTION),
        ClassName::Atmosphere => Some(ATMOSPHERE_SECTION),
        ClassName::Clouds => Some(CLOUDS_SECTION),
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
        sync_with_sun: flag(section, "follows_calendar").unwrap_or(base.sync_with_sun),
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

/// Cloud layer types by name, in the order the Properties panel offers them.
pub const CLOUD_LAYER_TYPES: [(&str, CloudLayerType); 5] = [
    ("Cumulus", CloudLayerType::Cumulus),
    ("Cirrus", CloudLayerType::Cirrus),
    ("Stratus", CloudLayerType::Stratus),
    ("Cumulonimbus", CloudLayerType::Cumulonimbus),
    ("Altocumulus", CloudLayerType::Altocumulus),
];

/// Where a cloud layer gathers, by name, in the order the panel offers them.
pub const CLOUD_COVERAGE_MODES: [(&str, CloudCoverage); 8] = [
    ("Full", CloudCoverage::Full),
    ("Scattered", CloudCoverage::Scattered),
    ("Horizon", CloudCoverage::Horizon),
    ("Zenith", CloudCoverage::Zenith),
    ("Northern", CloudCoverage::Northern),
    ("Southern", CloudCoverage::Southern),
    ("Eastern", CloudCoverage::Eastern),
    ("Western", CloudCoverage::Western),
];

/// A cloud layer type from its name, in any case.
pub fn cloud_layer_type_named(name: &str) -> Option<CloudLayerType> {
    CLOUD_LAYER_TYPES.iter().find(|(n, _)| flat(n) == flat(name)).map(|(_, t)| *t)
}

/// A cloud layer type's name.
pub fn cloud_layer_type_name(kind: CloudLayerType) -> &'static str {
    CLOUD_LAYER_TYPES.iter().find(|(_, t)| *t == kind).map_or("Cumulus", |(n, _)| n)
}

/// A coverage mode from its name, in any case.
pub fn cloud_coverage_named(name: &str) -> Option<CloudCoverage> {
    CLOUD_COVERAGE_MODES.iter().find(|(n, _)| flat(n) == flat(name)).map(|(_, m)| *m)
}

/// A coverage mode's name.
pub fn cloud_coverage_name(mode: CloudCoverage) -> &'static str {
    CLOUD_COVERAGE_MODES.iter().find(|(_, m)| *m == mode).map_or("Full", |(n, _)| n)
}

/// A cloud layer from its `[clouds]` section, over `base`. The Roblox importer
/// writes `enabled`, `cover` (Roblox's name for the coverage), `density` and
/// `color` in 0-255 channels; the rest is this renderer's own. An unknown
/// layer type or coverage mode keeps `base`'s.
pub fn clouds_from_section(section: Option<&toml::Value>, base: Clouds) -> Clouds {
    Clouds {
        enabled: flag(section, "enabled").unwrap_or(base.enabled),
        coverage: num(section, "cover")
            .or_else(|| num(section, "coverage"))
            .map(|v| v.clamp(0.0, 1.0))
            .unwrap_or(base.coverage),
        density: num(section, "density").map(|v| v.clamp(0.0, 1.0)).unwrap_or(base.density),
        color: rgba(section, "color", base.color),
        shadow_color: rgba(section, "shadow_color", base.shadow_color),
        layer_type: text(section, "layer_type")
            .and_then(|s| cloud_layer_type_named(&s))
            .unwrap_or(base.layer_type),
        softness: num(section, "softness").map(|v| v.clamp(0.0, 1.0)).unwrap_or(base.softness),
        spread: num(section, "spread").map(|v| v.max(0.05)).unwrap_or(base.spread),
        noise_scale: num(section, "noise_scale").map(|v| v.max(0.05)).unwrap_or(base.noise_scale),
        altitude: num(section, "altitude").map(|v| v.max(50.0)).unwrap_or(base.altitude),
        thickness: num(section, "thickness").map(|v| v.max(50.0)).unwrap_or(base.thickness),
        coverage_mode: text(section, "coverage_mode")
            .and_then(|s| cloud_coverage_named(&s))
            .unwrap_or(base.coverage_mode),
        coverage_bias: num(section, "coverage_bias")
            .map(|v| v.clamp(0.0, 1.0))
            .unwrap_or(base.coverage_bias),
        wind_speed: num(section, "wind_speed").map(|v| v.max(0.0)).unwrap_or(base.wind_speed),
        wind_direction: num(section, "wind_direction")
            .map(|v| v.rem_euclid(360.0))
            .unwrap_or(base.wind_direction),
        animation_speed: num(section, "animation_speed")
            .map(|v| v.max(0.0))
            .unwrap_or(base.animation_speed),
        time_of_day_tinting: flag(section, "time_of_day_tinting").unwrap_or(base.time_of_day_tinting),
        ..base
    }
}

/// The Sun a Space gets when its file sets nothing: [`SunClass::default`]
/// carrying the raw sunlight bevy's atmosphere is calibrated for.
pub fn default_sun() -> SunClass {
    SunClass { noon_intensity: RAW_SUNLIGHT, ..SunClass::default() }
}

/// A Lighting child's class component(s), built from its section: what every
/// loader inserts (Studio's file and instance loaders through
/// [`insert_class_component`], the Player through [`spawn_space_celestials`]).
#[derive(Debug, Clone)]
pub enum CelestialBody {
    Sun(SunClass),
    Moon(MoonClass),
    Sky(Sky),
    /// The Explorer class and the scattering model it mirrors.
    Atmosphere(Atmosphere, EustressAtmosphere),
    Clouds(Clouds),
}

impl CelestialBody {
    /// The component for a Star (the Sun), Moon, Sky, Atmosphere or Clouds
    /// object from its section; `None` for any other class.
    pub fn from_section(class: ClassName, section: Option<&toml::Value>) -> Option<Self> {
        Some(match class {
            ClassName::Star => Self::Sun(sun_from_section(section, default_sun())),
            ClassName::Moon => Self::Moon(moon_from_section(section, MoonClass::default())),
            ClassName::Sky => Self::Sky(sky_from_section(section, Sky::default())),
            ClassName::Atmosphere => {
                let (atmosphere, model) = atmosphere_from_section(section, &EustressAtmosphere::default());
                Self::Atmosphere(atmosphere, model)
            }
            ClassName::Clouds => Self::Clouds(clouds_from_section(section, Clouds::default())),
            _ => return None,
        })
    }

    /// Insert the component(s) onto `entity`.
    pub fn insert(self, entity: &mut EntityCommands) {
        match self {
            Self::Sun(sun) => entity.insert(sun),
            Self::Moon(moon) => entity.insert(moon),
            Self::Sky(sky) => entity.insert(sky),
            Self::Atmosphere(atmosphere, model) => entity.insert((atmosphere, model)),
            Self::Clouds(clouds) => entity.insert(clouds),
        };
    }
}

/// Insert the class component for a Star, Moon, Sky, Atmosphere or Clouds
/// object built from its section: the load path's single entry.
pub fn insert_class_component(entity: &mut EntityCommands, class: ClassName, section: Option<&toml::Value>) {
    if let Some(body) = CelestialBody::from_section(class, section) {
        body.insert(entity);
    }
}

/// A sky object found in a Space's files.
#[derive(Debug, Clone)]
pub struct SpaceCelestial {
    pub name: String,
    pub body: CelestialBody,
}

/// Where a Space keeps its sky objects, and whether each place holds every
/// class or Clouds only. Lighting holds them all (Roblox renders the Sky, the
/// Atmosphere, the sun and the moon only as Lighting children, and a new
/// Space's Clouds sits there too); Workspace/Terrain holds Clouds, where
/// Roblox parents them and the importer writes them. Both are read at any
/// depth.
pub const CELESTIAL_FOLDERS: [(&str, bool); 2] = [("Lighting", true), ("Workspace/Terrain", false)];

/// A Space's Sun, Moon, Sky, Atmosphere and Clouds objects, read from
/// [`CELESTIAL_FOLDERS`] the way Studio loads them: a flat `Name.instance.toml`
/// or a `Name/_instance.toml` (the folder wins when both exist), the class
/// from `[metadata] class_name` (the legacy `Sun` is a Star), the old
/// template sections dropped as the heal drops them, and the values from the
/// class's own section over the class defaults, which equal its schema.
/// This is the Player's reader; unreadable files come back as problems.
pub fn read_space_celestials(space_root: &std::path::Path) -> (Vec<SpaceCelestial>, Vec<String>) {
    let mut found = Vec::new();
    let mut problems = Vec::new();
    for (folder, every_class) in CELESTIAL_FOLDERS {
        walk_celestials(&space_root.join(folder), every_class, &mut found, &mut problems);
    }
    (found, problems)
}

fn walk_celestials(
    dir: &std::path::Path,
    every_class: bool,
    found: &mut Vec<SpaceCelestial>,
    problems: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut dirs = Vec::new();
    let mut flats = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()).map(str::to_owned) else { continue };
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            dirs.push((name, path));
        } else if let Some(stem) = name.strip_suffix(".instance.toml") {
            flats.push((stem.to_owned(), path));
        }
    }
    flats.retain(|(stem, _)| !dirs.iter().any(|(d, _)| d == stem));
    let files = dirs
        .iter()
        .map(|(name, path)| (name.clone(), path.join("_instance.toml")))
        .filter(|(_, file)| file.is_file())
        .chain(flats);
    for (fallback_name, path) in files {
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                problems.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        // Terrain holds many layer objects and at most a Clouds; skip the
        // parse for the rest.
        if !every_class && !text.contains("Clouds") {
            continue;
        }
        let mut doc = match text.parse::<toml::Value>() {
            Ok(doc) => doc,
            Err(e) => {
                problems.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        drop_legacy_template_sections(&mut doc);
        let meta = doc.get("metadata").or_else(|| doc.get("Metadata"));
        let Some(class) = meta
            .and_then(|m| m.get("class_name").or_else(|| m.get("ClassName")))
            .and_then(|c| c.as_str())
            .and_then(|c| ClassName::from_str(c).ok())
        else {
            continue;
        };
        if !every_class && class != ClassName::Clouds {
            continue;
        }
        let name = meta
            .and_then(|m| m.get("name").or_else(|| m.get("Name")))
            .and_then(|n| n.as_str())
            .map_or(fallback_name, str::to_owned);
        if let Some(body) = CelestialBody::from_section(class, section_of(&doc, class)) {
            found.push(SpaceCelestial { name, body });
        }
    }
    for (_, sub) in dirs {
        walk_celestials(&sub, every_class, found, problems);
    }
}

/// Spawn what [`read_space_celestials`] found, each lit as Studio lights it:
/// a Sun through `lighting_plugin::sun_light` placed by `lighting` (the
/// Space's Lighting service), a Moon through `moon_light`, the Sky, the
/// Atmosphere and the Clouds as their class components. They clear with the
/// Space. Returns how many were spawned.
pub fn spawn_space_celestials(commands: &mut Commands, found: &[SpaceCelestial], lighting: &LightingService) -> usize {
    use crate::plugins::lighting_plugin::{moon_light, sun_light};
    for object in found {
        #[cfg(feature = "physics")]
        let marker = crate::space_read::SpawnedFromSpace;
        #[cfg(not(feature = "physics"))]
        let marker = ();
        let name = Name::new(object.name.clone());
        match &object.body {
            // The Player's cameras carry no `ContactShadows`.
            CelestialBody::Sun(sun) => commands.spawn((name, marker, sun_light(sun, lighting, false))),
            CelestialBody::Moon(moon) => commands.spawn((name, marker, moon_light(moon))),
            CelestialBody::Sky(sky) => commands.spawn((name, marker, sky.clone())),
            CelestialBody::Atmosphere(atmosphere, model) => {
                commands.spawn((name, marker, atmosphere.clone(), model.clone()))
            }
            CelestialBody::Clouds(clouds) => commands.spawn((name, marker, clouds.clone())),
        };
    }
    found.len()
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
        ("follows_calendar", toml::Value::Boolean(moon.sync_with_sun)),
    ])
}

/// A Clouds object's `[clouds]` section. The coverage is written as `cover`,
/// the importer's and Roblox's name, which the reader prefers.
pub fn clouds_section(c: &Clouds) -> toml::value::Table {
    let s = |v: &str| toml::Value::String(v.to_string());
    table(vec![
        ("enabled", toml::Value::Boolean(c.enabled)),
        ("cover", float(c.coverage)),
        ("density", float(c.density)),
        ("color", color(c.color)),
        ("shadow_color", color(c.shadow_color)),
        ("layer_type", s(cloud_layer_type_name(c.layer_type))),
        ("softness", float(c.softness)),
        ("spread", float(c.spread)),
        ("altitude", float(c.altitude)),
        ("thickness", float(c.thickness)),
        ("coverage_mode", s(cloud_coverage_name(c.coverage_mode))),
        ("coverage_bias", float(c.coverage_bias)),
        ("wind_speed", float(c.wind_speed)),
        ("wind_direction", float(c.wind_direction)),
        ("animation_speed", float(c.animation_speed)),
        ("time_of_day_tinting", toml::Value::Boolean(c.time_of_day_tinting)),
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
    // A Clouds object's own section shares its name with an old Sky template
    // section, so the class's own is always kept.
    root.retain(|k, _| k == own || !is_legacy_section(k));
    true
}

/// The star count the old Sky templates wrote into `[sky]`, which a heal
/// baked into many files that no one ever set.
pub const OLD_DEFAULT_STAR_COUNT: i64 = 3000;

/// Drop the old template sections from a Star, Moon, Sky or Atmosphere
/// document. The heal runs this, so a loaded file never carries them into
/// Attributes. True when anything was removed.
///
/// A Sky that still carries them was never edited under the current
/// sections, so its `[sky] star_count` of 3000 is the old template's, not
/// an author's: it becomes the current default. Once the Sky is edited the
/// old sections are gone and whatever count it holds is kept.
pub fn drop_legacy_template_sections(doc: &mut toml::Value) -> bool {
    let class = doc
        .get("metadata")
        .or_else(|| doc.get("Metadata"))
        .and_then(|m| m.get("class_name").or_else(|| m.get("ClassName")))
        .and_then(|c| c.as_str());
    if !matches!(class, Some("Star" | "Moon" | "Sky" | "Atmosphere")) {
        return false;
    }
    let is_sky = class == Some("Sky");
    let Some(root) = doc.as_table_mut() else { return false };
    let before = root.len();
    root.retain(|k, v| !(v.is_table() && is_legacy_section(k)));
    let dropped = root.len() != before;
    if dropped && is_sky {
        let own = root.iter_mut().find(|(k, _)| flat(k) == SKY_SECTION).map(|(_, v)| v);
        if let Some(count) = own.and_then(|s| s.as_table_mut()).and_then(|s| s.get_mut("star_count")) {
            if count.as_integer() == Some(OLD_DEFAULT_STAR_COUNT) {
                *count = toml::Value::Integer(Sky::default().star_count as i64);
            }
        }
    }
    dropped
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
        assert_eq!(m.sync_with_sun, d.sync_with_sun);
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

    const CLOUDS_SCHEMA: &str = include_str!("../../assets/class_schema/Clouds/_instance.toml");

    #[test]
    fn the_clouds_schema_is_the_default_fair_weather_layer() {
        let d = Clouds::default();
        let base = Clouds { coverage: 0.9, density: 0.9, altitude: 9000.0, softness: 0.0, ..d.clone() };
        let c = clouds_from_section(Some(&schema(ClassName::Clouds, CLOUDS_SCHEMA)), base);
        assert!(close(c.coverage, d.coverage) && close(c.density, d.density), "{c:?}");
        assert!(close(c.altitude, d.altitude) && close(c.thickness, d.thickness));
        assert!(close(c.softness, d.softness) && close(c.wind_speed, d.wind_speed));
        assert!(close4(c.color, d.color) && close4(c.shadow_color, d.shadow_color));
        assert_eq!((c.layer_type, c.coverage_mode), (d.layer_type, d.coverage_mode));
        assert_eq!(c.enabled, d.enabled);
    }

    #[test]
    fn an_imported_cloud_layer_keeps_its_cover_colour_and_switch() {
        // The shape the Roblox importer writes.
        let d = doc("[clouds]\ncolor = [231, 231, 231]\ncover = 0.5\ndensity = 0.25\nenabled = false\n");
        let c = clouds_from_section(section_of(&d, ClassName::Clouds), Clouds::default());
        assert!(!c.enabled);
        assert_eq!((c.coverage, c.density), (0.5, 0.25));
        assert!((c.color[0] - 231.0 / 255.0).abs() < 1e-6, "0-255 channels");
        assert_eq!(c.altitude, Clouds::default().altitude, "unwritten fields keep the default");
    }

    #[test]
    fn cloud_names_are_read_in_any_case_and_unknown_ones_keep_the_base() {
        let d = doc("[clouds]\nlayer_type = \"cirrus\"\ncoverage_mode = \"SCATTERED\"\n");
        let c = clouds_from_section(section_of(&d, ClassName::Clouds), Clouds::default());
        assert_eq!((c.layer_type, c.coverage_mode), (CloudLayerType::Cirrus, CloudCoverage::Scattered));
        let d = doc("[clouds]\nlayer_type = \"nonsense\"\n");
        let c = clouds_from_section(section_of(&d, ClassName::Clouds), Clouds::default());
        assert_eq!(c.layer_type, CloudLayerType::Cumulus);
    }

    #[test]
    fn a_cloud_layer_round_trips_and_keeps_its_own_section_when_stored() {
        let c = Clouds {
            coverage: 0.62,
            layer_type: CloudLayerType::Altocumulus,
            coverage_mode: CloudCoverage::Horizon,
            time_of_day_tinting: false,
            ..Clouds::default()
        };
        let mut d = doc("[metadata]\nclass_name = \"Clouds\"\n");
        assert!(store_section(&mut d, ClassName::Clouds, clouds_section(&c)));
        assert!(d.get("clouds").is_some(), "the class's own [clouds] survives the old-section sweep");
        let back = clouds_from_section(section_of(&d, ClassName::Clouds), Clouds::default());
        assert_eq!(back.coverage, 0.62);
        assert_eq!((back.layer_type, back.coverage_mode), (CloudLayerType::Altocumulus, CloudCoverage::Horizon));
        assert!(!back.time_of_day_tinting);
    }

    #[test]
    fn the_player_reads_the_sky_objects_studio_reads() {
        let root = std::env::temp_dir().join(format!("eustress-celestials-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let lighting = root.join("Lighting");
        let terrain = root.join("Workspace").join("Terrain");
        std::fs::create_dir_all(lighting.join("Clouds")).unwrap();
        std::fs::create_dir_all(terrain.join("Clouds")).unwrap();
        // A folder wins over a flat file of the same name.
        std::fs::write(lighting.join("Clouds").join("_instance.toml"), CLOUDS_SCHEMA).unwrap();
        std::fs::write(lighting.join("Clouds.instance.toml"), "[metadata]\nclass_name = \"Clouds\"\n[clouds]\ncover = 0.9\n").unwrap();
        // The legacy class name, as MCP scaffolds wrote it.
        std::fs::write(lighting.join("Sun.instance.toml"), "[metadata]\nclass_name = \"Sun\"\n[star]\nangular_size = 0.7\n").unwrap();
        // An untouched old-template Sky: its 3000 stars were never an author's.
        std::fs::write(
            lighting.join("Sky.instance.toml"),
            "[metadata]\nclass_name = \"Sky\"\n[sky]\nstar_count = 3000\n[Appearance]\nx = 1\n",
        )
        .unwrap();
        // The importer's folder-form Clouds under Terrain, and a Sky there,
        // which Roblox would not draw.
        std::fs::write(
            terrain.join("Clouds").join("_instance.toml"),
            "[metadata]\nclass_name = \"Clouds\"\nunit = \"stud\"\n[clouds]\ncover = 0.8\nenabled = false\n",
        )
        .unwrap();
        std::fs::write(terrain.join("Sky.instance.toml"), "[metadata]\nclass_name = \"Sky\"\n").unwrap();
        let (found, problems) = read_space_celestials(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(found.len(), 4, "{found:?}");
        let clouds: Vec<&Clouds> = found
            .iter()
            .filter_map(|c| match &c.body {
                CelestialBody::Clouds(clouds) => Some(clouds),
                _ => None,
            })
            .collect();
        assert_eq!(clouds.len(), 2);
        assert!(clouds.iter().any(|c| close(c.coverage, Clouds::default().coverage)), "the folder's file");
        assert!(clouds.iter().any(|c| !c.enabled && close(c.coverage, 0.8)), "the imported layer");
        assert!(found.iter().any(|c| matches!(&c.body, CelestialBody::Sun(s) if close(s.angular_size, 0.7))));
        assert!(found.iter().any(|c| matches!(&c.body, CelestialBody::Sky(s) if s.star_count == Sky::default().star_count)));
    }

    #[test]
    fn a_moon_that_follows_the_calendar_round_trips() {
        let moon = MoonClass { sync_with_sun: false, ..MoonClass::default() };
        let back = moon_from_section(Some(&toml::Value::Table(moon_section(&moon))), MoonClass::default());
        assert!(!back.sync_with_sun);
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
    fn an_untouched_old_sky_gets_the_new_star_count_and_an_edited_one_keeps_its_own() {
        let mut old = doc(
            "[metadata]\nclass_name = \"Sky\"\n\n[sky]\nstar_count = 3000\n\n[appearance]\nstar_count = 3000\n",
        );
        assert!(drop_legacy_template_sections(&mut old));
        assert_eq!(old["sky"]["star_count"].as_integer(), Some(Sky::default().star_count as i64));
        // Edited under the current sections (no old ones left): kept.
        let mut edited = doc("[metadata]\nclass_name = \"Sky\"\n\n[sky]\nstar_count = 3000\n");
        drop_legacy_template_sections(&mut edited);
        assert_eq!(edited["sky"]["star_count"].as_integer(), Some(3000));
        // Any other count on an old file: the author's.
        let mut chosen = doc("[metadata]\nclass_name = \"Sky\"\n\n[sky]\nstar_count = 1200\n\n[Appearance]\nx = 1\n");
        drop_legacy_template_sections(&mut chosen);
        assert_eq!(chosen["sky"]["star_count"].as_integer(), Some(1200));
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

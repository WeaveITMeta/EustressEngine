//! The Lighting service's own properties (ClockTime, GeographicLatitude,
//! Brightness, the ambient terms, fog, exposure, the day cycle) carried into
//! the live [`LightingService`]: the one parser both apps use.
//!
//! Studio's Lighting service loads its `_service.toml` into a
//! `ServiceComponent` and hands its properties to
//! [`apply_lighting_properties`] through its own [`LightingProperties`]
//! adapter; its Properties panel edits land the same way. The Player has no
//! service entity, so it reads the file with [`read_space_lighting`], which
//! gathers the properties exactly as Studio's service loader does
//! ([`service_document_properties`]) and applies them with the same function.
//! A key, a spelling or a unit accepted by one app is accepted by the other.

use std::path::Path;

use super::lighting::LightingService;

/// One Lighting property value, as either store holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LightingValue<'a> {
    Number(f32),
    Bool(bool),
    Text(&'a str),
    /// 0-1 channels; a three-number colour has alpha 1.
    Color([f32; 4]),
}

/// A Lighting service's properties by key (snake_case, as the Properties
/// panel writes them).
pub trait LightingProperties {
    fn value(&self, key: &str) -> Option<LightingValue<'_>>;
}

/// A service file's properties, as [`service_document_properties`] gathers
/// them. An array of three or four numbers is a colour, the rule Studio's
/// service loader applies (`toml_to_property_value`).
impl LightingProperties for toml::value::Table {
    fn value(&self, key: &str) -> Option<LightingValue<'_>> {
        Some(match self.get(key)? {
            toml::Value::Float(v) => LightingValue::Number(*v as f32),
            toml::Value::Integer(v) => LightingValue::Number(*v as f32),
            toml::Value::Boolean(v) => LightingValue::Bool(*v),
            toml::Value::String(s) => LightingValue::Text(s),
            toml::Value::Array(items) => {
                let n: Vec<f32> = items
                    .iter()
                    .filter_map(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                    .map(|v| v as f32)
                    .collect();
                match n.len() {
                    3 => LightingValue::Color([n[0], n[1], n[2], 1.0]),
                    4 => LightingValue::Color([n[0], n[1], n[2], n[3]]),
                    _ => return None,
                }
            }
            _ => return None,
        })
    }
}

fn number(props: &impl LightingProperties, key: &str) -> Option<f32> {
    match props.value(key)? {
        LightingValue::Number(v) => Some(v),
        _ => None,
    }
}

fn flag(props: &impl LightingProperties, key: &str) -> Option<bool> {
    match props.value(key)? {
        LightingValue::Bool(v) => Some(v),
        _ => None,
    }
}

fn color(props: &impl LightingProperties, key: &str) -> Option<[f32; 4]> {
    match props.value(key)? {
        LightingValue::Color(v) => Some(v),
        _ => None,
    }
}

/// Parse `"HH:MM:SS"`, `"HH:MM"`, or a bare decimal hour into hours.
///
/// Wraps into `[0, 24)` rather than clamping, so 25:00 reads as 01:00 instead of
/// pinning to midnight.
pub fn parse_hours(text: &str) -> Option<f32> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if !text.contains(':') {
        return text.parse::<f32>().ok().map(|h| h.rem_euclid(24.0));
    }
    let mut parts = text.split(':');
    let h: f32 = parts.next()?.trim().parse().ok()?;
    let m: f32 = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
    let s: f32 = parts.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);
    (h.is_finite() && m.is_finite() && s.is_finite())
        .then(|| (h + m / 60.0 + s / 3600.0).rem_euclid(24.0))
}

/// The Lighting service's properties applied to `lighting`: one arm per key
/// the panel writes (its row name in snake_case), plus the older spellings
/// a Space may still carry. Studio's `lighting_property_defaults` lists the
/// panel's keys, and its test walks the panel's schema and fails on any row
/// whose key reaches nothing here. `GlobalShadows` was saved as
/// `global_shadows` and read as `shadows_enabled`, so it reset on every reload.
pub fn apply_lighting_properties(props: &impl LightingProperties, lighting: &mut LightingService) {
    // `clock_time` is hours (0-24). Accept a string too: the panel used to
    // write it as text, and authored Spaces may still carry either shape.
    let clock_hours = match props.value("clock_time") {
        Some(LightingValue::Number(v)) => Some(v),
        Some(LightingValue::Text(s)) => parse_hours(s),
        _ => None,
    };
    if let Some(hours) = clock_hours {
        let new_tod = (hours / 24.0).rem_euclid(1.0);
        if (lighting.time_of_day - new_tod).abs() > 0.001 {
            lighting.time_of_day = new_tod;
            let total = (new_tod * 24.0 * 3600.0).round() as u32 % 86_400;
            lighting.clock_time = format!("{:02}:{:02}:{:02}", total / 3600, (total / 60) % 60, total % 60);
        }
    }

    // Appearance.
    if let Some(v) = color(props, "ambient") {
        lighting.ambient = v;
    }
    if let Some(v) = number(props, "brightness") {
        lighting.brightness = v;
    }
    if let Some(v) = color(props, "color_shift_bottom") {
        lighting.color_shift_bottom = v;
    }
    if let Some(v) = color(props, "color_shift_top") {
        lighting.color_shift_top = v;
    }
    if let Some(v) = number(props, "environment_diffuse_scale") {
        lighting.environment_diffuse_scale = v;
    }
    if let Some(v) = number(props, "environment_specular_scale") {
        lighting.environment_specular_scale = v;
    }
    if let Some(v) = flag(props, "global_shadows").or_else(|| flag(props, "shadows_enabled")) {
        lighting.shadows_enabled = v;
    }
    if let Some(v) = color(props, "outdoor_ambient") {
        lighting.outdoor_ambient = v;
    }

    // Exposure.
    if let Some(v) = number(props, "exposure_compensation") {
        lighting.exposure_compensation = v;
    }

    // Time.
    if let Some(v) = number(props, "geographic_latitude") {
        lighting.geographic_latitude = v;
    }
    if let Some(v) = flag(props, "day_cycle").or_else(|| flag(props, "cycle_enabled")) {
        lighting.cycle_enabled = v;
    }
    if let Some(v) = number(props, "day_length_minutes") {
        lighting.day_length_minutes = v.max(0.01);
    }

    // Fog.
    if let Some(v) = color(props, "fog_color") {
        lighting.fog_color = v;
    }
    if let Some(v) = number(props, "fog_start") {
        lighting.fog_start = v;
    }
    if let Some(v) = number(props, "fog_end") {
        lighting.fog_end = v;
    }
    if let Some(v) = flag(props, "fog_enabled") {
        lighting.fog_enabled = v;
    }

    // The sun, for a Space with no Sun object (its Properties own these when
    // it has one).
    if let Some(v) = number(props, "sun_intensity") {
        lighting.sun_intensity = v;
    }
    if let Some(v) = color(props, "sun_color") {
        lighting.sun_color = v;
    }
}

/// The header keys of a service file's `[service]` table, which are the
/// service's identity, not its properties.
const SERVICE_HEADER_KEYS: [&str; 4] = ["class_name", "icon", "description", "can_have_children"];

/// Every property a service file defines, from the places Studio's service
/// loader takes them: flattened under `[service]` (where a save writes them),
/// top-level keys outside any section, and the `[properties]` section (where
/// the shipped templates put them), which wins a collision. Keys in any case
/// are normalised to snake_case first, as the loader does.
pub fn service_document_properties(doc: &toml::Value) -> toml::value::Table {
    let mut doc = doc.clone();
    crate::class_schema::normalise_keys(&mut doc);
    let mut out = toml::value::Table::new();
    let Some(root) = doc.as_table() else { return out };
    if let Some(service) = root.get("service").and_then(|s| s.as_table()) {
        for (key, value) in service {
            if !SERVICE_HEADER_KEYS.contains(&key.as_str()) {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    for (key, value) in root {
        if !value.is_table() && !matches!(key.as_str(), "service" | "metadata") {
            out.insert(key.clone(), value.clone());
        }
    }
    if let Some(section) = root.get("properties").and_then(|s| s.as_table()) {
        for (key, value) in section {
            out.insert(key.clone(), value.clone());
        }
    }
    out
}

/// The Lighting service a Space's `Lighting/_service.toml` describes: the
/// defaults with the file's properties applied, which is what Studio's
/// service loader gives `LightingService` on open. `Ok(None)` when the Space
/// has no Lighting service file.
pub fn read_space_lighting(space_root: &Path) -> Result<Option<LightingService>, String> {
    let path = space_root.join("Lighting").join("_service.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: toml::Value = text.parse().map_err(|e| format!("{}: {e}", path.display()))?;
    let mut lighting = LightingService::default();
    apply_lighting_properties(&service_document_properties(&doc), &mut lighting);
    Ok(Some(lighting))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = include_str!("../../assets/service_templates/Lighting/_service.toml");

    fn props(text: &str) -> toml::value::Table {
        service_document_properties(&text.parse::<toml::Value>().expect("test TOML parses"))
    }

    #[test]
    fn a_new_space_opens_at_the_templates_values() {
        let mut lighting = LightingService::default();
        apply_lighting_properties(&props(TEMPLATE), &mut lighting);
        assert_eq!(lighting.clock_time, "14:00:00");
        assert!((lighting.geographic_latitude - 41.73).abs() < 1e-4);
        assert_eq!(lighting.brightness, 2.0);
        assert_eq!(lighting.outdoor_ambient, [0.5, 0.5, 0.5, 1.0]);
        assert!(lighting.shadows_enabled);
        assert!(!lighting.cycle_enabled);
    }

    #[test]
    fn properties_come_from_every_place_the_loader_takes_them() {
        let p = props(
            "[service]\nclass_name = \"Lighting\"\nBrightness = 3.0\nFogEnd = 500\n\n[properties]\nbrightness = 1.5\nClockTime = \"06:30:00\"\n",
        );
        assert!(!p.contains_key("class_name"), "the header is not a property");
        let mut lighting = LightingService::default();
        apply_lighting_properties(&p, &mut lighting);
        assert_eq!(lighting.brightness, 1.5, "[properties] wins a collision");
        assert_eq!(lighting.fog_end, 500.0, "a [service] property and an integer both count");
        assert_eq!(lighting.clock_time, "06:30:00", "a clock string, in PascalCase");
    }

    #[test]
    fn old_spellings_and_three_number_colours_still_load() {
        let p = props("[properties]\nshadows_enabled = false\ncycle_enabled = true\nambient = [0.1, 0.2, 0.3]\n");
        let mut lighting = LightingService::default();
        apply_lighting_properties(&p, &mut lighting);
        assert!(!lighting.shadows_enabled);
        assert!(lighting.cycle_enabled);
        assert_eq!(lighting.ambient, [0.1, 0.2, 0.3, 1.0]);
    }

    #[test]
    fn clock_text_wraps_rather_than_clamps() {
        assert_eq!(parse_hours("25:00"), Some(1.0));
        assert_eq!(parse_hours("6.5"), Some(6.5));
        assert_eq!(parse_hours(""), None);
    }

    #[test]
    fn a_space_without_a_lighting_file_reads_none() {
        let root = std::env::temp_dir().join(format!("eustress-no-lighting-{}", std::process::id()));
        assert!(matches!(read_space_lighting(&root), Ok(None)));
    }
}

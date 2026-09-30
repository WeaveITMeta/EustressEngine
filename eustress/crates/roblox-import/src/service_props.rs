//! A Roblox service's own properties, written to the engine's
//! `<Service>/_service.toml`.
//!
//! The importer used to route a service's children and drop the service
//! itself, so Lighting's time of day, ambient light and fog, Workspace's
//! gravity, StarterPlayer's character and camera defaults, Players' respawn
//! time and MaterialService's material overrides were all lost. Each is now
//! written over the engine's own template for that service
//! (`common/assets/service_templates/<Service>/_service.toml`), so every key
//! the source place does not set keeps the engine default.
//!
//! Property names, enum values and which properties serialise come from the
//! reflection database (`rbx_reflection_database 2.0.2+roblox-700`).
//!
//! Units follow the template: Workspace, Players and StarterPlayer keep
//! Roblox's own values (gravity 196.2, walk speed 16), as the templates do.
//! Lighting's fog distances are the exception: the engine hands them straight
//! to Bevy's distance fog, in meters, so they convert from studs.

use std::collections::HashMap;

use rbx_dom_weak::types::Variant;

/// Meters per Roblox stud: the Eustress stud (`units::Unit::Stud`, 0.28 m).
const STUD_TO_M: f64 = eustress_common::units::Unit::Stud.to_meters();

/// Services whose imported values are Roblox's raw lengths and speeds, in the
/// stud: `StarterPlayer`'s character movement, display and camera distances.
const IN_STUDS: &[&str] = &["StarterPlayer"];

/// Roblox's default `FogEnd`: at this distance fog is effectively off.
const ROBLOX_FOG_OFF: f32 = 100_000.0;

/// `[properties]` keys for one service, and the Roblox properties that had
/// no key (reported, not lost: they are also written under
/// `[properties.extras]`).
#[derive(Debug, Default)]
pub struct ServiceProps {
    /// Keys to set in the template's `[properties]`.
    pub properties: toml::value::Table,
    /// Roblox properties with no key, and their variant type.
    pub unmapped: Vec<(String, String)>,
    /// The unmapped properties' values, kept under `[properties.extras]`.
    pub extras: toml::value::Table,
}

/// Map a service's properties. `None` for a service this module does not
/// know, whose template (if any) is used as is.
pub fn map_service_properties(class: &str, props: &HashMap<String, Variant>) -> Option<ServiceProps> {
    let mut out = ServiceProps::default();
    let mapper: fn(&str, &Variant, &HashMap<String, Variant>, &mut toml::value::Table) -> bool = match class {
        "Lighting" => lighting,
        "Workspace" => workspace,
        "Players" => players,
        "StarterPlayer" => starter_player,
        "SoundService" => sound_service,
        "MaterialService" => material_service,
        _ => return None,
    };
    for (key, value) in props {
        if matches!(key.as_str(), "Name" | "Parent") {
            continue;
        }
        if !mapper(key, value, props, &mut out.properties) && is_authored(key, value) {
            out.unmapped.push((key.clone(), format!("{:?}", value.ty())));
            if let Some(v) = plain_value(value) {
                out.extras.insert(key.clone(), v);
            }
        }
    }
    Some(out)
}

/// Roblox-internal bookkeeping that is not part of a place's design.
fn is_authored(key: &str, _value: &Variant) -> bool {
    !matches!(
        key,
        "Capabilities" | "Sandboxed" | "SourceAssetId" | "UniqueId" | "HistoryId" | "Tags"
            | "Attributes" | "DefinesCapabilities" | "Archivable"
    )
}

fn plain_value(v: &Variant) -> Option<toml::Value> {
    Some(match v {
        Variant::Bool(b) => toml::Value::Boolean(*b),
        Variant::Int32(i) => toml::Value::Integer(*i as i64),
        Variant::Int64(i) => toml::Value::Integer(*i),
        Variant::Float32(f) => toml::Value::Float(*f as f64),
        Variant::Float64(f) => toml::Value::Float(*f),
        Variant::String(s) => toml::Value::String(s.clone()),
        Variant::Enum(e) => toml::Value::Integer(e.to_u32() as i64),
        Variant::Color3(c) => colour(c.r, c.g, c.b),
        Variant::Vector3(v) => floats(&[v.x as f64, v.y as f64, v.z as f64]),
        _ => return None,
    })
}

fn floats(v: &[f64]) -> toml::Value {
    toml::Value::Array(v.iter().map(|f| toml::Value::Float(*f)).collect())
}

/// A colour as the templates store it: `[r, g, b, 1.0]`, 0..1.
fn colour(r: f32, g: f32, b: f32) -> toml::Value {
    floats(&[r as f64, g as f64, b as f64, 1.0])
}

fn colour_of(v: &Variant) -> Option<toml::Value> {
    match v {
        Variant::Color3(c) => Some(colour(c.r, c.g, c.b)),
        Variant::Color3uint8(c) => Some(colour(c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0)),
        _ => None,
    }
}

fn float_of(v: &Variant) -> Option<f64> {
    match v {
        Variant::Float32(f) => Some(*f as f64),
        Variant::Float64(f) => Some(*f),
        Variant::Int32(i) => Some(*i as f64),
        Variant::Int64(i) => Some(*i as f64),
        _ => None,
    }
}

fn enum_of(v: &Variant) -> Option<u32> {
    match v {
        Variant::Enum(e) => Some(e.to_u32()),
        Variant::Int32(i) => Some(*i as u32),
        _ => None,
    }
}

fn set(t: &mut toml::value::Table, key: &str, v: toml::Value) -> bool {
    t.insert(key.to_string(), v);
    true
}

fn set_float(t: &mut toml::value::Table, key: &str, v: &Variant, scale: f64) -> bool {
    match float_of(v) {
        Some(f) => set(t, key, toml::Value::Float(f * scale)),
        None => false,
    }
}

fn set_bool(t: &mut toml::value::Table, key: &str, v: &Variant) -> bool {
    match v {
        Variant::Bool(b) => set(t, key, toml::Value::Boolean(*b)),
        _ => false,
    }
}

fn set_int(t: &mut toml::value::Table, key: &str, v: &Variant) -> bool {
    match v {
        Variant::Int32(i) => set(t, key, toml::Value::Integer(*i as i64)),
        Variant::Int64(i) => set(t, key, toml::Value::Integer(*i)),
        _ => false,
    }
}

fn set_enum(t: &mut toml::value::Table, key: &str, v: &Variant, names: &[&str]) -> bool {
    match enum_of(v).and_then(|i| names.get(i as usize)) {
        Some(name) => set(t, key, toml::Value::String(name.to_string())),
        None => false,
    }
}

/// `"HH:MM:SS"` -> hours.
fn hours_of(s: &str) -> Option<f64> {
    let mut parts = s.split(':').map(|p| p.trim().parse::<f64>());
    let h = parts.next()?.ok()?;
    let m = parts.next().and_then(|p| p.ok()).unwrap_or(0.0);
    let sec = parts.next().and_then(|p| p.ok()).unwrap_or(0.0);
    Some(h + m / 60.0 + sec / 3600.0)
}

fn lighting(key: &str, v: &Variant, all: &HashMap<String, Variant>, t: &mut toml::value::Table) -> bool {
    match key {
        "Ambient" => colour_of(v).is_some_and(|c| set(t, "ambient", c)),
        "OutdoorAmbient" => colour_of(v).is_some_and(|c| set(t, "outdoor_ambient", c)),
        "ColorShift_Top" => colour_of(v).is_some_and(|c| set(t, "color_shift_top", c)),
        "ColorShift_Bottom" => colour_of(v).is_some_and(|c| set(t, "color_shift_bottom", c)),
        "FogColor" => colour_of(v).is_some_and(|c| set(t, "fog_color", c)),
        "Brightness" => set_float(t, "brightness", v, 1.0),
        "ShadowSoftness" => set_float(t, "shadow_softness", v, 1.0),
        "EnvironmentDiffuseScale" => set_float(t, "environment_diffuse_scale", v, 1.0),
        "EnvironmentSpecularScale" => set_float(t, "environment_specular_scale", v, 1.0),
        "ExposureCompensation" => set_float(t, "exposure_compensation", v, 1.0),
        "GeographicLatitude" => set_float(t, "geographic_latitude", v, 1.0),
        "ClockTime" => set_float(t, "clock_time", v, 1.0),
        // The same time as a string; used only when ClockTime is absent.
        "TimeOfDay" => match v {
            Variant::String(s) if !all.contains_key("ClockTime") => {
                hours_of(s).is_some_and(|h| set(t, "clock_time", toml::Value::Float(h)))
            }
            Variant::String(_) => true,
            _ => false,
        },
        // World distances for Bevy's fog, in meters. Fog shows only when the
        // place pulled FogEnd in from Roblox's "off" distance.
        "FogStart" => set_float(t, "fog_start", v, STUD_TO_M),
        "FogEnd" => match float_of(v) {
            Some(f) => {
                set(t, "fog_enabled", toml::Value::Boolean((f as f32) < ROBLOX_FOG_OFF));
                set(t, "fog_end", toml::Value::Float(f * STUD_TO_M))
            }
            None => false,
        },
        // The template names it `global_shadows`; the renderer reads
        // `shadows_enabled`. Both carry it.
        "GlobalShadows" => match v {
            Variant::Bool(b) => {
                set(t, "shadows_enabled", toml::Value::Boolean(*b));
                set(t, "global_shadows", toml::Value::Boolean(*b))
            }
            _ => false,
        },
        "Technology" => set_enum(t, "technology", v, &["Legacy", "Voxel", "Compatibility", "ShadowMap", "Future", "Unified"]),
        _ => false,
    }
}

fn workspace(key: &str, v: &Variant, _all: &HashMap<String, Variant>, t: &mut toml::value::Table) -> bool {
    match key {
        // Roblox's gravity in its own stud as a vector in m/s² (196.2 studs/s²
        // down is [0, -54.9, 0]), so an imported world falls as it did in
        // Roblox. A vector, not a scalar, marks the value as metres: an older
        // file's scalar is a legacy number the loader reads another way.
        "Gravity" => match float_of(v) {
            Some(g) => set(t, "gravity", floats(&[0.0, -g * STUD_TO_M, 0.0])),
            None => false,
        },
        "FallenPartsDestroyHeight" => set_float(t, "fallen_parts_destroy_height", v, STUD_TO_M),
        "GlobalWind" => match v {
            Variant::Vector3(w) => {
                let m = STUD_TO_M;
                set(t, "global_wind", floats(&[w.x as f64 * m, w.y as f64 * m, w.z as f64 * m]))
            }
            _ => false,
        },
        "StreamingEnabled" => set_bool(t, "streaming_enabled", v),
        "StreamingMinRadius" => set_int(t, "streaming_min_radius", v),
        "StreamingTargetRadius" => set_int(t, "streaming_target_radius", v),
        "TouchesUseCollisionGroups" => set_bool(t, "touches_use_collision_groups", v),
        "AllowThirdPartySales" => set_bool(t, "allow_third_party_sales", v),
        "SignalBehavior" => set_enum(t, "signal_behavior", v, &["Default", "Immediate", "Deferred", "AncestryDeferred"]),
        _ => false,
    }
}

fn players(key: &str, v: &Variant, _all: &HashMap<String, Variant>, t: &mut toml::value::Table) -> bool {
    match key {
        "CharacterAutoLoads" => set_bool(t, "character_auto_loads", v),
        "RespawnTime" => set_float(t, "respawn_time", v, 1.0),
        "MaxPlayers" => set_int(t, "max_players", v),
        "PreferredPlayers" => set_int(t, "preferred_players", v),
        _ => false,
    }
}

fn starter_player(key: &str, v: &Variant, _all: &HashMap<String, Variant>, t: &mut toml::value::Table) -> bool {
    match key {
        "AutoJumpEnabled" => set_bool(t, "auto_jump_enabled", v),
        "CharacterJumpHeight" => set_float(t, "character_jump_height", v, 1.0),
        "CharacterJumpPower" => set_float(t, "character_jump_power", v, 1.0),
        "CharacterMaxSlopeAngle" => set_float(t, "character_max_slope_angle", v, 1.0),
        "CharacterUseJumpPower" => set_bool(t, "character_use_jump_power", v),
        "CharacterWalkSpeed" => set_float(t, "character_walk_speed", v, 1.0),
        "HealthDisplayDistance" => set_float(t, "health_display_distance", v, 1.0),
        "NameDisplayDistance" => set_float(t, "name_display_distance", v, 1.0),
        "CameraMaxZoomDistance" => set_float(t, "camera_max_zoom_distance", v, 1.0),
        "CameraMinZoomDistance" => set_float(t, "camera_min_zoom_distance", v, 1.0),
        "CameraMode" => set_enum(t, "camera_mode", v, &["Classic", "LockFirstPerson"]),
        "DevCameraOcclusionMode" => set_enum(t, "dev_camera_occlusion_mode", v, &["Zoom", "Invisicam"]),
        "DevComputerCameraMovementMode" => set_enum(
            t,
            "dev_computer_camera_movement_mode",
            v,
            &["UserChoice", "Classic", "Follow", "Orbital", "CameraToggle"],
        ),
        "DevTouchCameraMovementMode" => set_enum(
            t,
            "dev_touch_camera_movement_mode",
            v,
            &["UserChoice", "Classic", "Follow", "Orbital"],
        ),
        "EnableMouseLockOption" => set_bool(t, "enable_mouse_lock_option", v),
        "LoadCharacterAppearance" => set_bool(t, "load_character_appearance", v),
        "UserEmotesEnabled" => set_bool(t, "user_emotes_enabled", v),
        _ => false,
    }
}

fn sound_service(key: &str, v: &Variant, _all: &HashMap<String, Variant>, t: &mut toml::value::Table) -> bool {
    match key {
        "AmbientReverb" => set_enum(
            t,
            "ambient_reverb",
            v,
            &[
                "NoReverb", "GenericReverb", "PaddedCell", "Room", "Bathroom", "LivingRoom", "StoneRoom",
                "Auditorium", "ConcertHall", "Cave", "Arena", "Hangar", "CarpettedHallway", "Hallway",
                "StoneCorridor", "Alley", "Forest", "City", "Mountains", "Quarry", "Plain", "ParkingLot",
                "SewerPipe", "UnderWater",
            ],
        ),
        "DistanceFactor" => set_float(t, "distance_factor", v, 1.0),
        "DopplerScale" => set_float(t, "doppler_scale", v, 1.0),
        "RolloffScale" => set_float(t, "rolloff_scale", v, 1.0),
        "RespectFilteringEnabled" => set_bool(t, "respect_filtering_enabled", v),
        _ => false,
    }
}

/// `Use2022Materials`, and `<Material>Name`: the MaterialVariant every part of
/// that material uses by default. Kept as `material_overrides`, a table from
/// material name to variant name (only the ones set).
fn material_service(key: &str, v: &Variant, _all: &HashMap<String, Variant>, t: &mut toml::value::Table) -> bool {
    match key {
        "Use2022Materials" => set_bool(t, "use_2022_materials", v),
        "Use2022MaterialsXml" => true,
        _ => match (key.strip_suffix("Name"), v) {
            (Some(material), Variant::String(variant)) => {
                if !variant.is_empty() {
                    let entry = t
                        .entry("material_overrides".to_string())
                        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
                    if let Some(table) = entry.as_table_mut() {
                        table.insert(material.to_string(), toml::Value::String(variant.clone()));
                    }
                }
                true
            }
            _ => false,
        },
    }
}

/// The template body for `class`, from the engine's service templates, with
/// `props` laid over its `[properties]`. `None` when there is no template.
pub fn service_toml(class: &str, mapped: &ServiceProps) -> Option<String> {
    let path = eustress_common::service_templates_dir().join(class).join("_service.toml");
    let template = std::fs::read_to_string(path).ok()?;
    let mut doc: toml::Value = template.parse().ok()?;
    let root = doc.as_table_mut()?;
    let props = root
        .entry("properties".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    if let Some(p) = props.as_table_mut() {
        for (k, v) in &mapped.properties {
            p.insert(k.clone(), v.clone());
        }
        if !mapped.extras.is_empty() {
            p.insert("extras".to_string(), toml::Value::Table(mapped.extras.clone()));
        }
    }
    // A service whose values are Roblox's raw lengths and speeds says so:
    // readers convert them from the file's unit, as they do for parts.
    if IN_STUDS.contains(&class) {
        let metadata = root
            .entry("metadata".to_string())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(m) = metadata.as_table_mut() {
            m.insert(
                "unit".to_string(),
                toml::Value::String(eustress_common::units::Unit::Stud.symbol().to_string()),
            );
        }
    }
    toml::to_string_pretty(&doc).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom_weak::types::{Color3, Enum};

    fn props(pairs: Vec<(&str, Variant)>) -> HashMap<String, Variant> {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    fn f(t: &toml::value::Table, k: &str) -> f64 {
        t.get(k).and_then(|v| v.as_float()).unwrap_or_else(|| panic!("no float {k}"))
    }

    #[test]
    fn lighting_time_fog_and_shadows_map() {
        let m = map_service_properties(
            "Lighting",
            &props(vec![
                ("ClockTime", Variant::Float32(18.5)),
                ("FogEnd", Variant::Float32(1000.0)),
                ("FogStart", Variant::Float32(100.0)),
                ("GlobalShadows", Variant::Bool(false)),
                ("Ambient", Variant::Color3(Color3::new(0.25, 0.5, 1.0))),
                ("Technology", Variant::Enum(Enum::from_u32(4))),
            ]),
        )
        .unwrap();
        let t = &m.properties;
        assert_eq!(f(t, "clock_time"), 18.5);
        assert!((f(t, "fog_end") - 1000.0 * STUD_TO_M).abs() < 1e-6, "fog is in meters");
        assert!((f(t, "fog_start") - 100.0 * STUD_TO_M).abs() < 1e-6);
        assert_eq!(t.get("fog_enabled").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(t.get("shadows_enabled").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(t.get("technology").and_then(|v| v.as_str()), Some("Future"));
        assert_eq!(t.get("ambient").and_then(|v| v.as_array()).map(|a| a.len()), Some(4));
        assert!(m.unmapped.is_empty(), "{:?}", m.unmapped);
    }

    #[test]
    fn default_fog_stays_off_and_time_of_day_is_a_fallback() {
        let m = map_service_properties(
            "Lighting",
            &props(vec![
                ("FogEnd", Variant::Float32(100_000.0)),
                ("TimeOfDay", Variant::String("06:30:00".into())),
            ]),
        )
        .unwrap();
        assert_eq!(m.properties.get("fog_enabled").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(f(&m.properties, "clock_time"), 6.5);
    }

    #[test]
    fn workspace_gravity_is_metres_and_starter_player_keeps_roblox_units() {
        let w = map_service_properties("Workspace", &props(vec![("Gravity", Variant::Float32(196.2))])).unwrap();
        let g: Vec<f64> = w.properties["gravity"].as_array().unwrap().iter().map(|x| x.as_float().unwrap()).collect();
        assert_eq!(g.len(), 3, "a vector marks m/s²");
        assert!(g[0] == 0.0 && g[2] == 0.0 && (g[1] + 196.2 * STUD_TO_M).abs() < 1e-4, "{g:?}");
        let s = map_service_properties(
            "StarterPlayer",
            &props(vec![
                ("CharacterWalkSpeed", Variant::Float32(24.0)),
                ("CameraMode", Variant::Enum(Enum::from_u32(1))),
            ]),
        )
        .unwrap();
        assert_eq!(f(&s.properties, "character_walk_speed"), 24.0);
        assert_eq!(s.properties.get("camera_mode").and_then(|v| v.as_str()), Some("LockFirstPerson"));
    }

    /// StarterPlayer keeps Roblox's raw movement values, and its file says
    /// they are in the stud, so a reader converts them as it does a part's.
    #[test]
    fn starter_player_values_are_raw_and_tagged_in_studs() {
        let s = map_service_properties(
            "StarterPlayer",
            &props(vec![
                ("CharacterWalkSpeed", Variant::Float32(24.0)),
                ("CharacterJumpPower", Variant::Float32(50.0)),
            ]),
        )
        .unwrap();
        assert_eq!(f(&s.properties, "character_walk_speed"), 24.0);
        assert_eq!(f(&s.properties, "character_jump_power"), 50.0);
        let text = service_toml("StarterPlayer", &s).expect("the StarterPlayer template");
        let doc: toml::Value = text.parse().unwrap();
        assert_eq!(doc["metadata"]["unit"].as_str(), Some(eustress_common::units::Unit::Stud.symbol()));
        assert_eq!(doc["properties"]["character_walk_speed"].as_float(), Some(24.0));
    }

    #[test]
    fn material_service_overrides_become_a_table() {
        let m = map_service_properties(
            "MaterialService",
            &props(vec![
                ("AsphaltName", Variant::String("Racing_Asphalt_A".into())),
                ("SlateName", Variant::String(String::new())),
            ]),
        )
        .unwrap();
        let o = m.properties.get("material_overrides").and_then(|v| v.as_table()).unwrap();
        assert_eq!(o.get("Asphalt").and_then(|v| v.as_str()), Some("Racing_Asphalt_A"));
        assert!(!o.contains_key("Slate"), "an empty override is no override");
    }

    #[test]
    fn unknown_properties_are_reported_and_kept() {
        let m = map_service_properties("Lighting", &props(vec![("Outlines", Variant::Bool(true))])).unwrap();
        assert_eq!(m.unmapped.len(), 1);
        assert_eq!(m.extras.get("Outlines").and_then(|v| v.as_bool()), Some(true));
        assert!(map_service_properties("ReplicatedStorage", &HashMap::new()).is_none());
    }
}

//! The choices an enum-valued Properties row offers, keyed by class and row.
//!
//! A row whose value is one of a fixed set is a dropdown ("choice"), never a
//! text field. Two sources feed it:
//!
//! - the class table below, for class components (a TextLabel's
//!   TextXAlignment, a SpotLight's Face, a Sound's RollOffMode, ...), each
//!   list taken from the code that reads the value back;
//! - a service's schema in `common/assets/service_properties/<Service>.toml`,
//!   whose `type = "enum"` entries carry their own `options` (Workspace's
//!   SignalBehavior, StarterPlayer's CameraMode, ...).
//!
//! [`apply`] runs over a panel's rows before they are pushed: a row with
//! choices becomes a dropdown holding them. An enum row with none stays a
//! text field and is reported once, so a missing list is seen, not guessed.
//! Rows and classes are matched without case or underscores, so
//! `RollOffMode` finds `rolloff_mode`.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use super::slint_ui::PropertyData;

const TEXT_X: &[&str] = &["Left", "Center", "Right"];
const TEXT_Y: &[&str] = &["Top", "Center", "Bottom"];
/// `classes::Font`, in declaration order; the Font row shows the live value.
const FONTS: &[&str] =
    &["SourceSans", "RobotoMono", "GothamBold", "GothamLight", "Fantasy", "Bangers", "Merriweather", "Nunito", "Ubuntu"];

/// A class's own enum rows: (class, row, choices). Only rows whose edits
/// apply belong here; a dropdown that changes nothing is worse than a text
/// field.
fn class_table() -> Vec<(&'static str, &'static str, Vec<String>)> {
    let own = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let shared = |list: Vec<slint::SharedString>| list.into_iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let mut t = vec![
        // The editor camera's Perspective rows (`camera_property_rows`).
        ("Camera", "ViewMode", own(&["3D", "2D"])),
        ("Camera", "Projection", own(&["Perspective", "Orthographic"])),
        // Lights (`light_panel`).
        ("SpotLight", "Face", shared(super::light_panel::face_options())),
        ("SurfaceLight", "Face", shared(super::light_panel::face_options())),
        // Sun, Moon, Sky, Atmosphere and Clouds (`celestial_panel`).
        ("Atmosphere", "RenderingMode", shared(super::celestial_panel::rendering_mode_options())),
        ("Clouds", "LayerType", shared(super::celestial_panel::cloud_layer_type_options())),
        ("Clouds", "CoverageMode", shared(super::celestial_panel::cloud_coverage_options())),
        ("Moon", "Phase", shared(super::celestial_panel::moon_phase_options())),
        // Sounds (`sound_panel`).
        ("Sound", "RollOffMode", shared(super::sound_panel::rolloff_mode_options())),
        ("Sound", "SoundGroup", shared(super::sound_panel::sound_group_options())),
        // GUI text and layering (their drain arms in `slint_ui`).
        ("TextLabel", "Font", own(FONTS)),
        ("BillboardGui", "ZIndexBehavior", own(&["Sibling", "Global"])),
    ];
    for class in ["TextLabel", "TextButton", "TextBox"] {
        t.push((class, "TextXAlignment", own(TEXT_X)));
        t.push((class, "TextYAlignment", own(TEXT_Y)));
    }
    t
}

/// A name compared without case or underscores.
fn key_form(name: &str) -> String {
    name.chars().filter(|c| *c != '_').flat_map(char::to_lowercase).collect()
}

/// A service's schema, read once per service (`None` when it has none).
fn service_schema(service: &str) -> Option<toml::Table> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<toml::Table>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entry(service.to_string())
        .or_insert_with(|| {
            let path = eustress_common::service_properties_dir().join(format!("{service}.toml"));
            std::fs::read_to_string(path).ok()?.parse::<toml::Table>().ok()
        })
        .clone()
}

/// The `options` of a schema's `type = "enum"` entry for `row`, in any of
/// its sections.
fn schema_options(schema: &toml::Table, row: &str) -> Option<Vec<String>> {
    let want = key_form(row);
    schema.values().filter_map(|section| section.as_table()).find_map(|section| {
        let (_, entry) = section.iter().find(|(k, _)| key_form(k) == want)?;
        let entry = entry.as_table()?;
        if entry.get("type").and_then(|v| v.as_str()) != Some("enum") {
            return None;
        }
        let options: Vec<String> = entry
            .get("options")?
            .as_array()?
            .iter()
            .filter_map(|o| o.as_str().map(str::to_string))
            .collect();
        Some(options)
    })
}

/// The choices row `row` of class `class` offers, or `None` when it is not
/// an enum row this registry knows.
pub fn options_for(class: &str, row: &str) -> Option<Vec<String>> {
    let (c, r) = (key_form(class), key_form(row));
    if let Some((_, _, list)) = class_table().into_iter().find(|(tc, tr, _)| key_form(tc) == c && key_form(tr) == r) {
        return Some(list);
    }
    service_schema(class).and_then(|schema| schema_options(&schema, row))
}

/// Report an enum row with no choices, once per class and row.
fn warn_once(class: &str, row: &str) {
    static SEEN: OnceLock<Mutex<HashSet<(String, String)>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let mut seen = seen.lock().unwrap_or_else(|e| e.into_inner());
    if seen.insert((class.to_string(), row.to_string())) {
        bevy::log::warn!("Properties: {class}.{row} is an enum row with no choices; it stays a text field");
    }
}

/// Give every enum row of `class` its choices: a row with choices becomes a
/// dropdown ("choice") holding them; an enum row without stays a text field
/// and is reported once. Headers and attributes are left alone.
pub fn apply(class: &str, rows: &mut [PropertyData]) {
    use slint::Model;
    for row in rows.iter_mut() {
        if row.is_header || row.is_attribute {
            continue;
        }
        let kind = row.property_type.to_string();
        if !matches!(kind.as_str(), "choice" | "enum" | "string") {
            continue;
        }
        if let Some(list) = options_for(class, row.name.as_str()).filter(|l| !l.is_empty()) {
            let shared: Vec<slint::SharedString> = list.iter().map(|s| s.as_str().into()).collect();
            row.options = slint::ModelRc::new(slint::VecModel::from(shared));
            row.property_type = "choice".into();
        } else if kind != "string" && row.options.row_count() == 0 {
            warn_once(class, row.name.as_str());
            // A dropdown with nothing in it cannot be edited.
            row.property_type = "string".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class_schema_default(class: &str, row: &str) -> Option<String> {
        let path = eustress_common::assets_dir().join("class_schema").join(class).join("_instance.toml");
        let doc: toml::Table = std::fs::read_to_string(path).ok()?.parse().ok()?;
        let want = key_form(row);
        doc.values()
            .filter_map(|s| s.as_table())
            .find_map(|s| s.iter().find(|(k, _)| key_form(k) == want).and_then(|(_, v)| v.as_str().map(str::to_string)))
    }

    #[test]
    fn every_class_list_is_whole_and_holds_its_schema_default() {
        for (class, row, list) in class_table() {
            assert!(!list.is_empty(), "{class}.{row} has no choices");
            let unique: HashSet<&String> = list.iter().collect();
            assert_eq!(unique.len(), list.len(), "{class}.{row} repeats a choice");
            if let Some(default) = class_schema_default(class, row) {
                assert!(list.contains(&default), "{class}.{row}: the template default {default:?} is not a choice");
            }
            assert_eq!(options_for(class, row).as_ref(), Some(&list));
        }
    }

    #[test]
    fn every_service_enum_has_choices_holding_its_default() {
        let dir = eustress_common::service_properties_dir();
        let mut enums = 0;
        for entry in std::fs::read_dir(&dir).expect("service_properties dir") {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let service = path.file_stem().unwrap().to_str().unwrap().to_string();
            let schema: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
            for section in schema.values().filter_map(|s| s.as_table()) {
                for (name, entry) in section {
                    let Some(entry) = entry.as_table() else { continue };
                    if entry.get("type").and_then(|v| v.as_str()) != Some("enum") {
                        continue;
                    }
                    enums += 1;
                    let list = options_for(&service, name).unwrap_or_default();
                    assert!(!list.is_empty(), "{service}.{name} is an enum with no options");
                    if let Some(default) = entry.get("value").and_then(|v| v.as_str()) {
                        assert!(list.iter().any(|o| o == default), "{service}.{name}: default {default:?} is not an option");
                    }
                }
            }
        }
        assert!(enums >= 6, "found {enums} service enums; Workspace and StarterPlayer have at least 6");
        assert_eq!(
            options_for("Workspace", "SignalBehavior"),
            Some(vec!["Default".to_string(), "Immediate".to_string(), "Deferred".to_string()])
        );
    }

    #[test]
    fn rows_match_without_case_or_underscores() {
        assert!(options_for("sound", "roll_off_mode").is_some());
        assert!(options_for("Workspace", "signal_behavior").is_some());
        assert!(options_for("Part", "Material").is_none(), "materials are dynamic and listed elsewhere");
        assert!(options_for("NoSuchService", "Anything").is_none());
    }
}

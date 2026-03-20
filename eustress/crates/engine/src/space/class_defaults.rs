//! # Class Defaults Registry
//!
//! ## Table of Contents
//!
//! 1. ClassDefaultsRegistry — Bevy Resource holding per-class default TOML tables
//! 2. load_class_defaults — Scans class_defaults/ directory and parses all *.defaults.toml files
//! 3. merge_defaults — Deep-merges missing keys from class defaults into a parsed TOML value
//! 4. color_u8_to_f32 — Converts [u8; 3] (0-255) color arrays to [f32; 3] (0.0-1.0) for Bevy
//!
//! ## Architecture
//!
//! Class default TOML files live in `crates/engine/assets/class_defaults/ClassName.defaults.toml`.
//! They follow the same file-system-first philosophy as service_templates — easy to edit
//! without recompilation. The registry loads them at engine startup, then the instance loader
//! and GUI loader merge missing fields from the class defaults before deserialization.
//!
//! ## Color Convention
//!
//! All color properties in TOML use 0-255 integer RGB arrays: `color = [163, 162, 165]`.
//! The loader converts to Bevy's 0.0-1.0 float range at deserialization time.

use bevy::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ============================================================================
// 1. ClassDefaultsRegistry — Bevy Resource
// ============================================================================

/// Bevy Resource holding per-class default TOML tables.
/// Key: class name string (e.g., "Part", "TextLabel", "ScreenGui").
/// Value: parsed TOML table containing all default properties for that class.
#[derive(Resource, Debug, Clone)]
pub struct ClassDefaultsRegistry {
    /// Map of class_name → parsed TOML Value (always a Table at the top level)
    pub defaults: HashMap<String, toml::Value>,
    /// Path to the class_defaults directory (for hot-reload support)
    pub source_dir: PathBuf,
}

impl Default for ClassDefaultsRegistry {
    fn default() -> Self {
        Self {
            defaults: HashMap::new(),
            source_dir: PathBuf::new(),
        }
    }
}

impl ClassDefaultsRegistry {
    /// Look up the default TOML table for a given class name.
    /// Returns None if no defaults file exists for this class.
    pub fn get(&self, class_name: &str) -> Option<&toml::Value> {
        self.defaults.get(class_name)
    }

    /// Get a specific section from the class defaults (e.g., "properties", "gui", "light").
    /// Returns None if the class or section doesn't exist.
    pub fn get_section(&self, class_name: &str, section: &str) -> Option<&toml::Value> {
        self.defaults.get(class_name)
            .and_then(|v| v.get(section))
    }

    /// Get the number of loaded class defaults
    pub fn len(&self) -> usize {
        self.defaults.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.defaults.is_empty()
    }
}

// ============================================================================
// 2. load_class_defaults — Scan and parse all *.defaults.toml files
// ============================================================================

/// Load all class default TOML files from the given directory.
/// Returns a populated ClassDefaultsRegistry.
///
/// Each file is named `ClassName.defaults.toml` and the class name is extracted
/// from the filename (everything before `.defaults.toml`).
pub fn load_class_defaults(class_defaults_dir: &Path) -> ClassDefaultsRegistry {
    let mut registry = ClassDefaultsRegistry {
        defaults: HashMap::new(),
        source_dir: class_defaults_dir.to_path_buf(),
    };

    if !class_defaults_dir.is_dir() {
        warn!(
            "Class defaults directory not found: {:?}. Using empty defaults.",
            class_defaults_dir
        );
        return registry;
    }

    let entries = match std::fs::read_dir(class_defaults_dir) {
        Ok(entries) => entries,
        Err(err) => {
            error!(
                "Failed to read class defaults directory {:?}: {}",
                class_defaults_dir, err
            );
            return registry;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        // Only process *.defaults.toml files
        if !file_name.ends_with(".defaults.toml") {
            continue;
        }

        // Extract class name: "Part.defaults.toml" → "Part"
        let class_name = file_name.trim_end_matches(".defaults.toml").to_string();
        if class_name.is_empty() {
            continue;
        }

        // Read and parse the TOML file
        match std::fs::read_to_string(&path) {
            Ok(content) => match content.parse::<toml::Value>() {
                Ok(value) => {
                    registry.defaults.insert(class_name.clone(), value);
                }
                Err(err) => {
                    error!(
                        "Failed to parse class defaults {:?}: {}",
                        path, err
                    );
                }
            },
            Err(err) => {
                error!(
                    "Failed to read class defaults file {:?}: {}",
                    path, err
                );
            }
        }
    }

    info!(
        "Loaded {} class defaults from {:?}",
        registry.defaults.len(),
        class_defaults_dir
    );

    registry
}

// ============================================================================
// 3. merge_defaults — Deep-merge missing keys from class defaults
// ============================================================================

/// Deep-merge missing keys from `defaults` into `target`.
///
/// For each key in `defaults`:
/// - If `target` doesn't have it, insert the default value.
/// - If both are tables, recurse into them.
/// - If `target` already has a non-table value, keep the existing value.
///
/// This ensures that user-specified values are never overwritten, but any
/// missing properties get filled in from the class defaults.
pub fn merge_defaults(target: &mut toml::Value, defaults: &toml::Value) {
    let (Some(target_table), Some(defaults_table)) = (target.as_table_mut(), defaults.as_table()) else {
        return;
    };

    for (key, default_value) in defaults_table {
        match target_table.get_mut(key) {
            Some(existing) => {
                // Both exist — if both are tables, recurse. Otherwise keep existing.
                if existing.is_table() && default_value.is_table() {
                    merge_defaults(existing, default_value);
                }
            }
            None => {
                // Missing in target — insert the default
                target_table.insert(key.clone(), default_value.clone());
            }
        }
    }
}

// ============================================================================
// 4. Color conversion helpers — 0-255 integers ↔ 0.0-1.0 floats
// ============================================================================

/// Convert a 0-255 RGB integer array from TOML to 0.0-1.0 float array for Bevy.
///
/// Usage: `let [r, g, b] = color_u8_to_f32(&toml_color_value);`
pub fn color_u8_to_f32(value: &toml::Value) -> Option<[f32; 3]> {
    let arr = value.as_array()?;
    if arr.len() < 3 {
        return None;
    }
    let r = arr[0].as_integer()? as f32 / 255.0;
    let g = arr[1].as_integer()? as f32 / 255.0;
    let b = arr[2].as_integer()? as f32 / 255.0;
    Some([r, g, b])
}

/// Convert a 0-255 RGBA integer array from TOML to 0.0-1.0 float array for Bevy.
pub fn color_u8_to_f32_rgba(value: &toml::Value) -> Option<[f32; 4]> {
    let arr = value.as_array()?;
    if arr.len() < 3 {
        return None;
    }
    let r = arr[0].as_integer()? as f32 / 255.0;
    let g = arr[1].as_integer()? as f32 / 255.0;
    let b = arr[2].as_integer()? as f32 / 255.0;
    let a = if arr.len() >= 4 {
        arr[3].as_integer().unwrap_or(255) as f32 / 255.0
    } else {
        1.0
    };
    Some([r, g, b, a])
}

/// Convert a 0.0-1.0 float RGB array to 0-255 integer array for TOML serialization.
pub fn color_f32_to_u8(rgb: &[f32; 3]) -> [u8; 3] {
    [
        (rgb[0] * 255.0).round() as u8,
        (rgb[1] * 255.0).round() as u8,
        (rgb[2] * 255.0).round() as u8,
    ]
}

/// Convert a 0.0-1.0 float RGBA array to 0-255 integer array for TOML serialization.
pub fn color_f32_to_u8_rgba(rgba: &[f32; 4]) -> [u8; 4] {
    [
        (rgba[0] * 255.0).round() as u8,
        (rgba[1] * 255.0).round() as u8,
        (rgba[2] * 255.0).round() as u8,
        (rgba[3] * 255.0).round() as u8,
    ]
}

// ============================================================================
// 5. Startup system — load class defaults at engine init
// ============================================================================

/// Bevy startup system that loads class defaults from the engine assets directory.
/// The class_defaults directory is resolved relative to the engine's compiled-in asset path.
pub fn startup_load_class_defaults(mut commands: Commands) {
    // Resolve the class_defaults directory relative to the engine binary/assets
    let class_defaults_dir = resolve_class_defaults_dir();
    let registry = load_class_defaults(&class_defaults_dir);
    info!(
        "ClassDefaultsRegistry initialized with {} class defaults",
        registry.len()
    );
    commands.insert_resource(registry);
}

/// Resolve the path to the class_defaults directory.
/// Checks multiple locations in priority order:
/// 1. Relative to the executable (for deployed builds)
/// 2. Cargo manifest dir (for development builds)
/// 3. Hardcoded fallback
fn resolve_class_defaults_dir() -> PathBuf {
    // Development: relative to cargo manifest dir (set by build.rs or env)
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let dev_path = PathBuf::from(&manifest_dir)
            .join("assets")
            .join("class_defaults");
        if dev_path.is_dir() {
            return dev_path;
        }
    }

    // Development fallback: OUT_DIR from build.rs
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        // OUT_DIR is typically target/debug/build/crate-hash/out
        // Walk up to find the workspace root
        let mut path = PathBuf::from(&out_dir);
        for _ in 0..8 {
            let candidate = path.join("crates").join("engine").join("assets").join("class_defaults");
            if candidate.is_dir() {
                return candidate;
            }
            if !path.pop() {
                break;
            }
        }
    }

    // Relative to executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let exe_relative = exe_dir.join("assets").join("class_defaults");
            if exe_relative.is_dir() {
                return exe_relative;
            }
        }
    }

    // Relative to current directory
    let cwd_relative = PathBuf::from("crates")
        .join("engine")
        .join("assets")
        .join("class_defaults");
    if cwd_relative.is_dir() {
        return cwd_relative;
    }

    // Absolute fallback for common development layout
    let fallback = PathBuf::from("assets").join("class_defaults");
    if fallback.is_dir() {
        return fallback;
    }

    warn!("Could not locate class_defaults directory, using empty defaults");
    PathBuf::from("class_defaults")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_defaults_missing_key() {
        let mut target: toml::Value = toml::from_str(r#"
            [metadata]
            class_name = "Part"
        "#).unwrap();

        let defaults: toml::Value = toml::from_str(r#"
            [metadata]
            class_name = "Part"
            archivable = true

            [properties]
            color = [163, 162, 165]
            transparency = 0.0
        "#).unwrap();

        merge_defaults(&mut target, &defaults);

        // metadata.archivable should be added
        assert_eq!(
            target.get("metadata").unwrap().get("archivable").unwrap().as_bool(),
            Some(true)
        );
        // properties section should be added entirely
        assert!(target.get("properties").is_some());
        assert_eq!(
            target.get("properties").unwrap().get("transparency").unwrap().as_float(),
            Some(0.0)
        );
    }

    #[test]
    fn test_merge_defaults_existing_key_not_overwritten() {
        let mut target: toml::Value = toml::from_str(r#"
            [properties]
            color = [255, 0, 0]
        "#).unwrap();

        let defaults: toml::Value = toml::from_str(r#"
            [properties]
            color = [163, 162, 165]
            transparency = 0.0
        "#).unwrap();

        merge_defaults(&mut target, &defaults);

        // color should NOT be overwritten
        let color = target.get("properties").unwrap().get("color").unwrap().as_array().unwrap();
        assert_eq!(color[0].as_integer(), Some(255));
        assert_eq!(color[1].as_integer(), Some(0));
        assert_eq!(color[2].as_integer(), Some(0));

        // transparency should be added
        assert_eq!(
            target.get("properties").unwrap().get("transparency").unwrap().as_float(),
            Some(0.0)
        );
    }

    #[test]
    fn test_color_u8_to_f32() {
        let val: toml::Value = toml::from_str("color = [255, 128, 0]").unwrap();
        let color = color_u8_to_f32(val.get("color").unwrap()).unwrap();
        assert!((color[0] - 1.0).abs() < 0.01);
        assert!((color[1] - 0.502).abs() < 0.01);
        assert!((color[2] - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_color_f32_to_u8_roundtrip() {
        let original = [0.639_f32, 0.635, 0.647];
        let u8_color = color_f32_to_u8(&original);
        assert_eq!(u8_color, [163, 162, 165]);
    }
}

//! TOML-driven theme engine.
//!
//! A theme is a flat map of design-token → color/length, authored as a
//! `*.toml` file. Two built-ins (`classic`, `modern`) ship compiled into the
//! binary via `include_str!` (they double as copy-me templates); any number of
//! user themes are scanned from `%LOCALAPPDATA%/Eustress/Themes/` — that folder
//! IS the v1 "marketplace": drop a `.toml` in, rescan, it appears.
//!
//! This module is deliberately UI-framework-agnostic — it only parses and
//! validates palettes into [`ThemePalette`]. The conversion of a palette into
//! the Slint-generated `ThemeData` struct (and the actual live swap) lives in
//! `ui/slint_ui.rs`, where the Slint window and its generated types are in
//! scope. Keeping the parser here means it has no dependency on the generated
//! bindings and can be unit-tested standalone.

use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Every themeable token name, kebab-case, matching the `ThemeData` struct
/// fields in `ui/slint/theme.slint` 1:1. Used to validate authored TOMLs:
/// unknown keys are warned about and dropped (never crash on a typo).
///
/// The first 67 are colors (`#rrggbb` / `#rrggbbaa`); the last 6 are lengths
/// (`"4px"`). Keep in sync with the struct — the Rust-side builder in
/// `slint_ui.rs` is exhaustive, so a token added there without a matching
/// field is a compile error, which keeps this list honest in practice.
pub const KNOWN_TOKENS: &[&str] = &[
    // Colors
    "background-primary",
    "background-secondary",
    "background-tertiary",
    "panel-background",
    "header-background",
    "viewport-background",
    "tab-active",
    "tab-inactive",
    "tab-hover",
    "text-primary",
    "text-secondary",
    "text-disabled",
    "text-accent",
    "text-error",
    "text-warning",
    "text-success",
    "border-color",
    "border-focus",
    "border-error",
    "success-surface",
    "success-surface-hover",
    "success-surface-border",
    "success-surface-text-hover",
    "error-surface",
    "error-surface-hover",
    "error-surface-border",
    "button-background",
    "button-hover",
    "button-pressed",
    "button-primary",
    "button-primary-hover",
    "input-background",
    "input-border",
    "input-focus",
    "selection-background",
    "selection-border",
    "overlay-background",
    "dialog-background",
    "modal-backdrop",
    "dialog-titlebar-background",
    "accent-blue",
    "accent-cyan",
    "accent-eustress",
    "accent-green",
    "accent-green-bright",
    "accent-orange",
    "accent-purple",
    "accent-red",
    "accent-yellow",
    "cat-parts",
    "cat-structure",
    "cat-constraint",
    "cat-modify",
    "cat-data",
    "panel-glass",
    "border-highlight",
    "shadow-float",
    "border-glow-color",
    "ribbon-background",
    "ribbon-tab-active",
    "ribbon-tab-inactive",
    "ribbon-separator",
    "explorer-item-hover",
    "explorer-item-selected",
    "explorer-folder",
    "explorer-file",
    "output-info",
    "output-warning",
    "output-error",
    "output-debug",
    "gizmo-x",
    "gizmo-y",
    "gizmo-z",
    "shadow-color",
    // Lengths
    "accent-glow-blur",
    "backdrop-blur-lg",
    "radius-sm",
    "radius-md",
    "radius-lg",
    "radius-xl",
];

/// A parsed, validated theme palette. `tokens` holds only *known* token names
/// (unknown ones stripped during validation); missing tokens are filled at
/// apply-time from the Modern base palette, so a theme author only specifies
/// what they want to change.
#[derive(Debug, Clone)]
pub struct ThemePalette {
    pub id: String,
    pub name: String,
    pub description: String,
    /// True for the two compiled-in themes; false for user-folder themes.
    /// (Purely informational — used to label the source in the UI.)
    pub builtin: bool,
    pub tokens: HashMap<String, String>,
}

// ── TOML deserialization shapes ─────────────────────────────────────────────

#[derive(Deserialize)]
struct ThemeFile {
    meta: MetaSection,
    #[serde(default)]
    tokens: HashMap<String, String>,
}

#[derive(Deserialize)]
struct MetaSection {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
}

/// Parse a theme TOML string into a [`ThemePalette`], stripping unknown tokens
/// (with a `warn!` per drop). Returns `Err` only on structurally-invalid TOML
/// or a missing `[meta] id`/`name` — never on a bad individual token.
pub fn parse_theme_toml(text: &str, builtin: bool) -> Result<ThemePalette, String> {
    let file: ThemeFile = toml::from_str(text).map_err(|e| e.to_string())?;
    if file.meta.id.trim().is_empty() {
        return Err("theme [meta].id is empty".to_string());
    }
    if file.meta.name.trim().is_empty() {
        return Err("theme [meta].name is empty".to_string());
    }

    let mut kept: HashMap<String, String> = HashMap::new();
    for (k, v) in file.tokens {
        if !KNOWN_TOKENS.contains(&k.as_str()) {
            warn!(
                "theme '{}': unknown token '{}' ignored (not in KNOWN_TOKENS)",
                file.meta.id, k
            );
            continue;
        }
        // Sanity-check the value now so a malformed hex/length is dropped at
        // parse time (falls back to Modern base) rather than silently applied.
        let is_len = k.starts_with("radius-") || k.ends_with("-blur") || k == "backdrop-blur-lg";
        let ok = if is_len {
            parse_len(&v).is_some()
        } else {
            parse_hex(&v).is_some()
        };
        if !ok {
            warn!(
                "theme '{}': token '{}' has malformed value '{}' — ignored",
                file.meta.id, k, v
            );
            continue;
        }
        kept.insert(k, v);
    }

    Ok(ThemePalette {
        id: file.meta.id,
        name: file.meta.name,
        description: file.meta.description,
        builtin,
        tokens: kept,
    })
}

/// Parse `#rgb`, `#rrggbb`, `#rrggbbaa` (and 4-digit `#rgba`) into `[r,g,b,a]`.
pub fn parse_hex(s: &str) -> Option<[u8; 4]> {
    let h = s.trim().strip_prefix('#')?;
    let expand = |c: char| -> Option<u8> {
        let d = c.to_digit(16)? as u8;
        Some(d * 16 + d)
    };
    match h.len() {
        3 => {
            let mut it = h.chars();
            let r = expand(it.next()?)?;
            let g = expand(it.next()?)?;
            let b = expand(it.next()?)?;
            Some([r, g, b, 255])
        }
        4 => {
            let mut it = h.chars();
            let r = expand(it.next()?)?;
            let g = expand(it.next()?)?;
            let b = expand(it.next()?)?;
            let a = expand(it.next()?)?;
            Some([r, g, b, a])
        }
        6 => {
            let r = u8::from_str_radix(&h[0..2], 16).ok()?;
            let g = u8::from_str_radix(&h[2..4], 16).ok()?;
            let b = u8::from_str_radix(&h[4..6], 16).ok()?;
            Some([r, g, b, 255])
        }
        8 => {
            let r = u8::from_str_radix(&h[0..2], 16).ok()?;
            let g = u8::from_str_radix(&h[2..4], 16).ok()?;
            let b = u8::from_str_radix(&h[4..6], 16).ok()?;
            let a = u8::from_str_radix(&h[6..8], 16).ok()?;
            Some([r, g, b, a])
        }
        _ => None,
    }
}

/// Parse a Slint length literal (`"4px"` or bare `"4"`) into logical pixels.
pub fn parse_len(s: &str) -> Option<f32> {
    let t = s.trim().strip_suffix("px").unwrap_or(s.trim());
    t.trim().parse::<f32>().ok()
}

/// The two compiled-in palettes. These are authoritative for Classic/Modern
/// and double as the copy-me templates users clone into the Themes folder.
pub fn load_builtin_themes() -> Vec<ThemePalette> {
    let mut out = Vec::new();
    for (text, id) in [
        (include_str!("../themes/classic.toml"), "classic"),
        (include_str!("../themes/modern.toml"), "modern"),
    ] {
        match parse_theme_toml(text, true) {
            Ok(p) => out.push(p),
            // A malformed built-in is a DEV bug (we ship them), so make it loud.
            Err(e) => error!("built-in theme '{}' failed to parse: {}", id, e),
        }
    }
    out
}

/// `%LOCALAPPDATA%/Eustress/Themes/` (created if absent so it's droppable-into).
pub fn user_themes_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Eustress").join("Themes"))
}

/// Scan the user Themes folder for `*.toml`. Parse errors are logged and the
/// file skipped — a malformed drop-in never crashes the app.
pub fn scan_user_themes() -> Vec<ThemePalette> {
    let Some(dir) = user_themes_dir() else {
        return Vec::new();
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("could not create themes dir {}: {}", dir.display(), e);
        return Vec::new();
    }
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_theme_toml(&text, false) {
                Ok(p) => out.push(p),
                Err(e) => warn!("skipping theme {}: {}", path.display(), e),
            },
            Err(e) => warn!("could not read theme {}: {}", path.display(), e),
        }
    }
    out
}

/// The live set of loaded themes + which one is active. Built-ins first, then
/// user themes (sorted by name). `signature` bumps whenever the set changes so
/// the Slint-push system knows to re-send the list.
#[derive(Resource, Default)]
pub struct ThemeRegistry {
    pub themes: Vec<ThemePalette>,
    pub active_id: String,
    /// Monotonic version — bumped on (re)load so the sync system re-pushes.
    pub signature: u64,
}

impl ThemeRegistry {
    pub fn find(&self, id: &str) -> Option<&ThemePalette> {
        self.themes.iter().find(|t| t.id == id)
    }

    /// Reload built-ins + user folder, preserving `active_id` if it still
    /// resolves (else falling back to the first theme, typically "classic").
    pub fn reload(&mut self) {
        let mut themes = load_builtin_themes();
        let mut user = scan_user_themes();
        user.sort_by(|a, b| a.name.cmp(&b.name));
        themes.extend(user);
        // De-dupe by id (a user theme with a built-in id shadows nothing —
        // keep the first, which is the built-in).
        let mut seen = std::collections::HashSet::new();
        themes.retain(|t| seen.insert(t.id.clone()));
        if self.active_id.is_empty() || !themes.iter().any(|t| t.id == self.active_id) {
            if !themes.iter().any(|t| t.id == self.active_id) && !self.active_id.is_empty() {
                warn!(
                    "active theme '{}' no longer found; falling back",
                    self.active_id
                );
            }
            self.active_id = themes
                .first()
                .map(|t| t.id.clone())
                .unwrap_or_else(|| "classic".to_string());
        }
        self.themes = themes;
        self.signature = self.signature.wrapping_add(1);
    }
}

/// Loads the theme registry at startup and resolves the initial active theme
/// from `EditorSettings` (with a `theme_modern` back-compat migration).
pub struct StudioThemePlugin;

impl Plugin for StudioThemePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ThemeRegistry>()
            .add_systems(Startup, init_theme_registry);
    }
}

fn init_theme_registry(
    mut registry: ResMut<ThemeRegistry>,
    settings: Option<Res<crate::editor_settings::EditorSettings>>,
) {
    // Resolve initial active id: explicit active_theme_id wins; else migrate
    // the legacy theme_modern bool (true→modern, false→classic).
    if let Some(settings) = settings {
        registry.active_id = match settings.active_theme_id.as_deref() {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => {
                if settings.theme_modern {
                    "modern".to_string()
                } else {
                    "classic".to_string()
                }
            }
        };
    }
    registry.reload();
    info!(
        "themes: {} loaded, active = '{}'",
        registry.themes.len(),
        registry.active_id
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_forms() {
        assert_eq!(parse_hex("#000000"), Some([0, 0, 0, 255]));
        assert_eq!(parse_hex("#ffffff"), Some([255, 255, 255, 255]));
        assert_eq!(parse_hex("#00e5a066"), Some([0, 229, 160, 102]));
        assert_eq!(parse_hex("#abc"), Some([170, 187, 204, 255]));
        assert_eq!(parse_hex("nothex"), None);
        assert_eq!(parse_hex("#12345"), None);
    }

    #[test]
    fn parse_len_forms() {
        assert_eq!(parse_len("4px"), Some(4.0));
        assert_eq!(parse_len("20"), Some(20.0));
        assert_eq!(parse_len("bad"), None);
    }

    #[test]
    fn builtins_parse_and_are_complete() {
        let themes = load_builtin_themes();
        assert_eq!(themes.len(), 2, "classic + modern must both parse");
        for t in &themes {
            // A built-in must specify EVERY known token (it's the base others
            // fall back to; incompleteness would leak the compiled-in default).
            for tok in KNOWN_TOKENS {
                assert!(
                    t.tokens.contains_key(*tok),
                    "built-in theme '{}' is missing token '{}'",
                    t.id,
                    tok
                );
            }
        }
    }

    #[test]
    fn unknown_tokens_stripped() {
        // Double-hash raw-string delimiter: the TOML content itself contains
        // `"#` (inside hex values like "#101010"), which would prematurely
        // close a single-hash r#"..."# raw string.
        let text = r##"
[meta]
id = "t"
name = "T"
[tokens]
background-primary = "#101010"
bogus-token = "#ffffff"
"##;
        let p = parse_theme_toml(text, false).unwrap();
        assert!(p.tokens.contains_key("background-primary"));
        assert!(!p.tokens.contains_key("bogus-token"));
    }
}

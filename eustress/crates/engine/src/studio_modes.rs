//! Eustress Modes — task-oriented Studio layouts.
//!
//! A "Mode" reconfigures the workspace for a kind of work: it selects a subset
//! of ribbon tabs, a default panel Layout preset, and (optionally) a single
//! accent-color override. A mode may also declare **custom tabs** (sections of
//! tools beyond the 9 built-ins — e.g. Business's "Product"/"Manufacturing")
//! and **submodes** (a secondary menu — e.g. Military's six service
//! branches). Modes are pure-data TOML manifests — no code (that's what the
//! Luau/Rune plugin system is for). Nine built-ins ship compiled in; users
//! drop their own into `%LOCALAPPDATA%/Eustress/Modes/` — that folder IS the
//! v1 "marketplace".
//!
//! Security posture for v1: the icon is a *named id* resolved against a tiny
//! allowlist ([`KNOWN_ICON_IDS`]), never an arbitrary file path — no image
//! decode of untrusted paths. Ribbon tabs are validated against
//! [`KNOWN_TAB_IDS`]; unknown ids are dropped with a warning. A malformed
//! user manifest is skipped, never fatal. Custom-tab tool ids are opaque
//! strings routed through the same `on-menu-action` dispatch every built-in
//! ribbon button uses — an unrecognized one is inert (button does nothing),
//! not a security concern, so they aren't allowlist-validated.

use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;

/// The nine real ribbon tab ids (see `ribbon.slint`'s tab array). A mode may
/// show any subset; unknown ids in a manifest are dropped.
pub const KNOWN_TAB_IDS: &[&str] = &[
    "home", "model", "cad", "data", "ui", "terrain", "test", "mindspace", "plugins",
];

/// Named icons a mode may use. Unknown → warn + fall back to "gear".
/// Must stay in sync with `mode-icon()` in `ribbon.slint`.
pub const KNOWN_ICON_IDS: &[&str] = &[
    "gear", "gavel", "gamepad", "bridge", "scales", "factory", "mortarboard", "briefcase",
    "health", "military",
];

/// The five panel-Layout preset names (see `dock_layout.slint`). A mode names
/// its default; resolved at apply-time.
pub const KNOWN_LAYOUT_PRESETS: &[&str] =
    &["Default", "Scripting", "Building", "Minimal", "Wide Panels"];

/// One entry in a mode's secondary "submodes" menu (e.g. Military's six
/// service branches, Justice's Civil/Criminal/Judge). v1 semantics:
/// selecting a submode activates the PARENT mode and records which submode
/// is active; submodes do not (yet) carry their own tab/layout overrides —
/// that's the natural next step once a concrete need for it shows up.
#[derive(Debug, Clone)]
pub struct SubmodeMeta {
    pub id: String,
    pub name: String,
    /// Gate: this submode is hidden from the parent mode's submenu unless the
    /// user holds this role (see [`UserRoles`]). `None` = visible to everyone.
    /// The parent MODE stays visible regardless — this gates only the one
    /// submode (e.g. Justice is public; its "Judge" submode is judges-only).
    pub required_role: Option<String>,
}

impl SubmodeMeta {
    /// Whether this submode should appear in the parent mode's submenu for the
    /// given roles. Ungated submodes are always visible.
    pub fn visible(&self, roles: &UserRoles) -> bool {
        self.required_role.as_deref().map_or(true, |r| roles.has(r))
    }
}

/// One tool button inside a custom-tab section — an opaque action id routed
/// through the same `on-menu-action` dispatch as every built-in ribbon
/// button. Empty `tools` on a section is valid and renders a "Coming soon"
/// placeholder (a mode may be declared before its tooling exists).
#[derive(Debug, Clone)]
pub struct CustomTabSection {
    pub name: String,
    pub tools: Vec<String>,
}

/// A mode-defined tab beyond the 9 built-ins (e.g. Business's "Product").
#[derive(Debug, Clone)]
pub struct CustomTab {
    pub id: String,
    pub name: String,
    /// Hex color for the tab pill's active-state fill; falls back to the
    /// theme's `accent-eustress` when absent/malformed.
    pub color: Option<String>,
    pub sections: Vec<CustomTabSection>,
}

#[derive(Debug, Clone)]
pub struct ModeManifest {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub version: String,
    pub author: String,
    /// Validated, non-empty subset of KNOWN_TAB_IDS (in manifest order).
    pub tabs: Vec<String>,
    pub layout_preset: String,
    /// Optional accent (hex) — overlays `accent-eustress` only, never the base.
    pub accent: Option<String>,
    /// Gate: mode is hidden from the dropdown unless the active user holds
    /// this role (see `UserRoles`). `None` = visible to everyone. v1 is a
    /// client-side check (env-var-seeded `UserRoles`); the documented future
    /// path is role assignment from KYC-verified identity via Cloudflare KV,
    /// imported through Parameters — this field is the stable integration
    /// point for that, unchanged when the source of roles changes.
    pub required_role: Option<String>,
    pub submodes: Vec<SubmodeMeta>,
    pub custom_tabs: Vec<CustomTab>,
    pub builtin: bool,
}

// ── TOML shapes ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ModeFile {
    mode: ModeMetaSection,
    #[serde(default)]
    ribbon: RibbonSection,
    #[serde(default)]
    layout: LayoutSection,
    #[serde(default)]
    theme: ThemeSection,
    #[serde(default)]
    submodes: Vec<SubmodeSection>,
    #[serde(default)]
    tabs: Vec<TabSection>,
}

#[derive(Deserialize)]
struct ModeMetaSection {
    id: String,
    name: String,
    #[serde(default = "default_icon")]
    icon: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    author: String,
    #[serde(default, rename = "required-role")]
    required_role: Option<String>,
}

#[derive(Deserialize, Default)]
struct RibbonSection {
    #[serde(default)]
    tabs: Vec<String>,
}

#[derive(Deserialize, Default)]
struct LayoutSection {
    #[serde(default)]
    preset: Option<String>,
}

#[derive(Deserialize, Default)]
struct ThemeSection {
    #[serde(default)]
    accent: Option<String>,
}

#[derive(Deserialize)]
struct SubmodeSection {
    id: String,
    name: String,
    #[serde(default, rename = "required-role")]
    required_role: Option<String>,
}

#[derive(Deserialize)]
struct TabSection {
    id: String,
    name: String,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    sections: Vec<TabSectionSection>,
}

#[derive(Deserialize)]
struct TabSectionSection {
    name: String,
    #[serde(default)]
    tools: Vec<String>,
}

fn default_icon() -> String {
    "gear".to_string()
}

/// Parse + validate a mode manifest. Returns `Err` only on structurally-broken
/// TOML or an empty id/name/tab-set; individual bad fields degrade gracefully.
pub fn parse_mode_toml(text: &str, builtin: bool) -> Result<ModeManifest, String> {
    let f: ModeFile = toml::from_str(text).map_err(|e| e.to_string())?;
    if f.mode.id.trim().is_empty() {
        return Err("mode [mode].id is empty".into());
    }
    if f.mode.name.trim().is_empty() {
        return Err("mode [mode].name is empty".into());
    }

    // Icon allowlist.
    let icon = if KNOWN_ICON_IDS.contains(&f.mode.icon.as_str()) {
        f.mode.icon
    } else {
        warn!(
            "mode '{}': unknown icon '{}' — using 'gear'",
            f.mode.id, f.mode.icon
        );
        "gear".to_string()
    };

    // Built-in ribbon tabs: keep only known ids, in manifest order.
    let mut tabs: Vec<String> = Vec::new();
    for t in f.ribbon.tabs {
        if KNOWN_TAB_IDS.contains(&t.as_str()) {
            if !tabs.contains(&t) {
                tabs.push(t);
            }
        } else {
            warn!("mode '{}': unknown ribbon tab '{}' ignored", f.mode.id, t);
        }
    }

    // Custom tabs: id/name required non-empty; a tab with zero sections still
    // renders (empty content), matching the "invent tabs ahead of tools"
    // requirement — the ribbon shows a "Coming soon" placeholder for it.
    let mut custom_tabs: Vec<CustomTab> = Vec::new();
    for t in f.tabs {
        if t.id.trim().is_empty() || t.name.trim().is_empty() {
            warn!("mode '{}': custom tab with empty id/name skipped", f.mode.id);
            continue;
        }
        if KNOWN_TAB_IDS.contains(&t.id.as_str()) || custom_tabs.iter().any(|c| c.id == t.id) || tabs.contains(&t.id) {
            warn!(
                "mode '{}': custom tab id '{}' collides with a built-in or duplicate — skipped",
                f.mode.id, t.id
            );
            continue;
        }
        let color = match t.color {
            Some(c) if crate::studio_theme::parse_hex(&c).is_some() => Some(c),
            Some(c) => {
                warn!(
                    "mode '{}': custom tab '{}' malformed color '{}' — ignored",
                    f.mode.id, t.id, c
                );
                None
            }
            None => None,
        };
        custom_tabs.push(CustomTab {
            id: t.id,
            name: t.name,
            color,
            sections: t
                .sections
                .into_iter()
                .map(|s| CustomTabSection { name: s.name, tools: s.tools })
                .collect(),
        });
    }
    // Custom tabs count toward the ribbon tab strip alongside built-ins —
    // a mode with zero built-in tabs but real custom tabs is still valid.
    if tabs.is_empty() && custom_tabs.is_empty() {
        return Err(format!(
            "mode '{}' has no valid ribbon tabs (of {:?}) and no custom tabs",
            f.mode.id, KNOWN_TAB_IDS
        ));
    }

    // Layout preset: validate against known names, else fall back to Default.
    let layout_preset = match f.layout.preset {
        Some(p) if KNOWN_LAYOUT_PRESETS.contains(&p.as_str()) => p,
        Some(p) => {
            warn!(
                "mode '{}': unknown layout preset '{}' — using 'Default'",
                f.mode.id, p
            );
            "Default".to_string()
        }
        None => "Default".to_string(),
    };

    // Accent: validate hex, else drop.
    let accent = match f.theme.accent {
        Some(a) if crate::studio_theme::parse_hex(&a).is_some() => Some(a),
        Some(a) => {
            warn!("mode '{}': malformed accent '{}' — ignored", f.mode.id, a);
            None
        }
        None => None,
    };

    // Submodes: id/name required non-empty, deduped.
    let mut submodes: Vec<SubmodeMeta> = Vec::new();
    for s in f.submodes {
        if s.id.trim().is_empty() || s.name.trim().is_empty() {
            warn!("mode '{}': submode with empty id/name skipped", f.mode.id);
            continue;
        }
        if submodes.iter().any(|sm| sm.id == s.id) {
            warn!("mode '{}': duplicate submode id '{}' skipped", f.mode.id, s.id);
            continue;
        }
        let required_role = s.required_role.filter(|r| !r.trim().is_empty());
        submodes.push(SubmodeMeta { id: s.id, name: s.name, required_role });
    }

    let required_role = f.mode.required_role.filter(|r| !r.trim().is_empty());

    Ok(ModeManifest {
        id: f.mode.id,
        name: f.mode.name,
        icon,
        version: f.mode.version,
        author: f.mode.author,
        tabs,
        layout_preset,
        accent,
        required_role,
        submodes,
        custom_tabs,
        builtin,
    })
}

/// The compiled-in modes, in display order (Rust source order = dropdown
/// order for built-ins — `ModeRegistry::reload` does not re-sort them, only
/// user-folder modes get alphabetized). A malformed built-in is a dev bug →
/// loud error, since we ship it ourselves.
pub fn load_builtin_modes() -> Vec<ModeManifest> {
    let mut out = Vec::new();
    for (text, id) in [
        (include_str!("../modes/gaming.toml"), "gaming"),
        (include_str!("../modes/student.toml"), "student"),
        (include_str!("../modes/business.toml"), "business"),
        (include_str!("../modes/legal.toml"), "legal"),
        (include_str!("../modes/civil.toml"), "civil"),
        (include_str!("../modes/engineering.toml"), "engineering"),
        (include_str!("../modes/justice.toml"), "justice"),
        (include_str!("../modes/health.toml"), "health"),
        (include_str!("../modes/military.toml"), "military"),
    ] {
        match parse_mode_toml(text, true) {
            Ok(m) => out.push(m),
            Err(e) => error!("built-in mode '{}' failed to parse: {}", id, e),
        }
    }
    out
}

/// `%LOCALAPPDATA%/Eustress/Modes/` (created if absent).
pub fn user_modes_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("Eustress").join("Modes"))
}

/// Scan the user Modes folder; malformed files are logged + skipped.
pub fn scan_user_modes() -> Vec<ModeManifest> {
    let Some(dir) = user_modes_dir() else {
        return Vec::new();
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("could not create modes dir {}: {}", dir.display(), e);
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
            Ok(text) => match parse_mode_toml(&text, false) {
                Ok(m) => out.push(m),
                Err(e) => warn!("skipping mode {}: {}", path.display(), e),
            },
            Err(e) => warn!("could not read mode {}: {}", path.display(), e),
        }
    }
    out
}

/// Roles held by the current user, gating `ModeManifest.required_role`.
///
/// v1: client-side only, seeded once at startup from the `EUSTRESS_ROLES`
/// env var (comma-separated role names) — enough to develop/test role-gated
/// modes locally. The documented production path (not yet built): KYC
/// document verification → role assignment in a real database → synced into
/// Cloudflare KV under the user's identity → this resource populated from
/// that KV lookup instead of an env var. `ModeManifest.required_role` is the
/// stable integration point; nothing else needs to change when that lands.
#[derive(Resource, Default)]
pub struct UserRoles(pub HashSet<String>);

impl UserRoles {
    pub fn has(&self, role: &str) -> bool {
        self.0.contains(role)
    }
}

fn init_user_roles(mut roles: ResMut<UserRoles>) {
    if let Ok(raw) = std::env::var("EUSTRESS_ROLES") {
        roles.0 = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !roles.0.is_empty() {
            info!("user roles (from EUSTRESS_ROLES): {:?}", roles.0);
        }
    }
}

/// The live set of loaded modes + the active mode/submode id.
#[derive(Resource, Default)]
pub struct ModeRegistry {
    pub modes: Vec<ModeManifest>,
    pub active_id: String,
    /// Active submode within `active_id`'s mode, if any (empty = none).
    pub active_submode_id: String,
    /// Monotonic version — bumped on (re)load so the sync system re-pushes.
    pub signature: u64,
}

impl ModeRegistry {
    pub fn find(&self, id: &str) -> Option<&ModeManifest> {
        self.modes.iter().find(|m| m.id == id)
    }

    pub fn active(&self) -> Option<&ModeManifest> {
        self.find(&self.active_id)
    }

    /// Modes visible in the dropdown for the given roles — `required_role`
    /// modes are hidden unless held. The FULL list (`self.modes`) is kept
    /// internally regardless, so `find`/`active` still resolve a
    /// currently-active-but-now-hidden mode (e.g. a role was revoked)
    /// instead of silently breaking it.
    pub fn visible_modes<'a>(&'a self, roles: &'a UserRoles) -> impl Iterator<Item = &'a ModeManifest> {
        self.modes
            .iter()
            .filter(move |m| m.required_role.as_deref().map_or(true, |r| roles.has(r)))
    }

    pub fn reload(&mut self) {
        let mut modes = load_builtin_modes();
        let mut user = scan_user_modes();
        user.sort_by(|a, b| a.name.cmp(&b.name));
        modes.extend(user);
        let mut seen = std::collections::HashSet::new();
        modes.retain(|m| seen.insert(m.id.clone()));
        if self.active_id.is_empty() || !modes.iter().any(|m| m.id == self.active_id) {
            self.active_id = modes
                .iter()
                .find(|m| m.id == "engineering")
                .or_else(|| modes.first())
                .map(|m| m.id.clone())
                .unwrap_or_else(|| "engineering".to_string());
        }
        self.modes = modes;
        self.signature = self.signature.wrapping_add(1);
    }
}

pub struct StudioModesPlugin;

impl Plugin for StudioModesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ModeRegistry>()
            .init_resource::<UserRoles>()
            .add_systems(Startup, (init_user_roles, init_mode_registry).chain());
    }
}

fn init_mode_registry(
    mut registry: ResMut<ModeRegistry>,
    settings: Option<Res<crate::editor_settings::EditorSettings>>,
) {
    if let Some(settings) = settings {
        registry.active_id = settings.active_mode_id.clone();
        registry.active_submode_id = settings.active_submode_id.clone();
    }
    registry.reload();
    info!(
        "modes: {} loaded, active = '{}'{}",
        registry.modes.len(),
        registry.active_id,
        if registry.active_submode_id.is_empty() {
            String::new()
        } else {
            format!(" / '{}'", registry.active_submode_id)
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse() {
        let modes = load_builtin_modes();
        assert_eq!(modes.len(), 9, "all nine built-in modes must parse");
        let eng = modes.iter().find(|m| m.id == "engineering").unwrap();
        assert_eq!(eng.tabs.len(), KNOWN_TAB_IDS.len(), "engineering shows all tabs");
        let jus = modes.iter().find(|m| m.id == "justice").unwrap();
        assert!(jus.tabs.len() < KNOWN_TAB_IDS.len(), "justice is a subset");
        assert!(jus.accent.is_some(), "justice overrides accent");
        assert_eq!(jus.required_role, None, "justice mode is public — open to all");
        assert_eq!(jus.submodes.len(), 3, "justice has Civil/Criminal/Judge");
        // Only the Judge submode is gated; Civil + Criminal are public.
        let judge = jus.submodes.iter().find(|s| s.id == "judge").unwrap();
        assert_eq!(judge.required_role.as_deref(), Some("judge"), "Judge submode gated to judges");
        for open in ["civil", "criminal"] {
            let sm = jus.submodes.iter().find(|s| s.id == open).unwrap();
            assert_eq!(sm.required_role, None, "{open} submode is public");
        }
        let mil = modes.iter().find(|m| m.id == "military").unwrap();
        assert_eq!(mil.submodes.len(), 6, "military has six branches");
        let biz = modes.iter().find(|m| m.id == "business").unwrap();
        assert!(
            biz.custom_tabs.iter().any(|t| t.id == "product") && biz.custom_tabs.iter().any(|t| t.id == "manufacturing"),
            "business consolidates Product + Manufacturing as tabs"
        );
        assert!(!modes.iter().any(|m| m.id == "manufacturing"), "standalone Manufacturing mode removed");
        for id in ["gaming", "student", "legal", "justice", "civil"] {
            let m = modes.iter().find(|m| m.id == id).unwrap();
            assert!(m.tabs.contains(&"mindspace".to_string()), "{id} should have MindSpace");
        }
    }

    #[test]
    fn justice_public_but_judge_submode_gated() {
        // Justice mode is visible to everyone (no mode-level gate)...
        let modes = load_builtin_modes();
        let mut registry = ModeRegistry::default();
        registry.modes = modes;
        registry.active_id = "engineering".to_string();
        let no_roles = UserRoles::default();
        let visible: Vec<&str> = registry.visible_modes(&no_roles).map(|m| m.id.as_str()).collect();
        assert!(visible.contains(&"justice"), "justice is public — visible with no roles");

        // ...but its Judge submode is hidden without the "judge" role, while
        // Civil + Criminal always show.
        let jus = registry.find("justice").unwrap();
        let judge = jus.submodes.iter().find(|s| s.id == "judge").unwrap();
        assert!(!judge.visible(&no_roles), "Judge submode hidden without the role");
        for open in ["civil", "criminal"] {
            let sm = jus.submodes.iter().find(|s| s.id == open).unwrap();
            assert!(sm.visible(&no_roles), "{open} submode always visible");
        }
        let mut judges = UserRoles::default();
        judges.0.insert("judge".to_string());
        assert!(judge.visible(&judges), "Judge submode visible with the role");
    }

    #[test]
    fn mode_level_role_gating_still_works() {
        // The mode-level gate capability is retained (no built-in uses it now,
        // but a user manifest can). Verify with a synthetic gated mode.
        let text = r#"
[mode]
id = "secret"
name = "Secret"
required-role = "insider"
[ribbon]
tabs = ["home"]
"#;
        let m = parse_mode_toml(text, false).unwrap();
        assert_eq!(m.required_role.as_deref(), Some("insider"));
        let mut registry = ModeRegistry::default();
        registry.modes = vec![m];
        let none = UserRoles::default();
        assert_eq!(registry.visible_modes(&none).count(), 0, "gated mode hidden without role");
        let mut with = UserRoles::default();
        with.0.insert("insider".to_string());
        assert_eq!(registry.visible_modes(&with).count(), 1, "gated mode visible with role");
    }

    #[test]
    fn unknown_tabs_dropped() {
        let text = r#"
[mode]
id = "t"
name = "T"
icon = "bogus"
[ribbon]
tabs = ["home", "not-a-tab", "test"]
"#;
        let m = parse_mode_toml(text, false).unwrap();
        assert_eq!(m.tabs, vec!["home", "test"]);
        assert_eq!(m.icon, "gear", "unknown icon falls back");
    }

    #[test]
    fn custom_tab_with_empty_tools_is_valid() {
        let text = r##"
[mode]
id = "t2"
name = "T2"
[ribbon]
tabs = ["home"]
[[tabs]]
id = "notebook"
name = "Notebook"
  [[tabs.sections]]
  name = "Notes"
  tools = []
"##;
        let m = parse_mode_toml(text, false).unwrap();
        assert_eq!(m.custom_tabs.len(), 1);
        assert_eq!(m.custom_tabs[0].sections[0].tools.len(), 0);
    }
}

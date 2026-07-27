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
    "health", "military", "capitol",
];

/// The five panel-Layout preset names (see `dock_layout.slint`). A mode names
/// its default; resolved at apply-time.
pub const KNOWN_LAYOUT_PRESETS: &[&str] =
    &["Default", "Scripting", "Building", "Minimal", "Wide Panels"];

/// One entry in a mode's secondary "submodes" menu (e.g. Military's six
/// service branches, Justice's Civil/Criminal/Judge). Selecting a submode
/// activates the PARENT mode and records which submode is active. A submode
/// may optionally carry its own custom-tab set (`ModeManifest.submode_tabs`,
/// declared via `[[submode_tabs]]` in the manifest) that replaces the parent
/// mode's `custom_tabs` while it's active — e.g. Justice's Civil vs Criminal.
/// Layout/accent overrides per-submode are not built — no concrete need yet.
#[derive(Debug, Clone)]
pub struct SubmodeMeta {
    pub id: String,
    pub name: String,
    /// Gate: this submode is hidden from the parent mode's submenu unless the
    /// user holds this role (see [`UserRoles`]). `None` = visible to everyone.
    /// The parent MODE stays visible regardless — this gates only the one
    /// submode (e.g. Justice is public; its "Judge" submode is judges-only).
    pub required_role: Option<String>,
    /// Per-submode identity color (hex) — e.g. Justice's Civil/Criminal or
    /// Military's six branches each read in their own color in the dropdown,
    /// independent of the parent mode's `menu_color`. `None` falls back to
    /// the active-state highlight (theme accent when selected, else
    /// secondary text) — same "identity color vs. live-state highlight"
    /// split as `ModeManifest.menu_color` vs. `accent`.
    pub color: Option<String>,
    /// Discipline glyph — an allowlisted archetype icon id (validated
    /// against `tool_metadata::TOOL_ICON_IDS`, same never-an-arbitrary-path
    /// posture as the mode `icon`). Shown in the Modes dropdown submenu.
    pub icon: Option<String>,
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
    /// Explicit mode-selection-menu color (hex). When set, the mode's Modes-
    /// dropdown entry is drawn in this color; when absent it falls back to the
    /// mode's `accent`, then to the theme's base accent. Lets a mode pick a
    /// distinct menu identity color (e.g. a Roblox BrickColor) without changing
    /// `accent` (which tints the active UI and must read as a selection ring).
    pub menu_color: Option<String>,
    /// Gate: mode is hidden from the dropdown unless the active user holds
    /// this role (see `UserRoles`). `None` = visible to everyone. v1 is a
    /// client-side check (env-var-seeded `UserRoles`); the documented future
    /// path is role assignment from KYC-verified identity via Cloudflare KV,
    /// imported through Parameters — this field is the stable integration
    /// point for that, unchanged when the source of roles changes.
    pub required_role: Option<String>,
    pub submodes: Vec<SubmodeMeta>,
    pub custom_tabs: Vec<CustomTab>,
    /// Per-submode custom-tab OVERRIDES, keyed by submode id (e.g. Justice's
    /// "civil"/"criminal" each get their own Case/Docket/Evidence content).
    /// A submode absent from this map falls back to `custom_tabs` — see
    /// [`ModeManifest::effective_custom_tabs`].
    pub submode_tabs: std::collections::HashMap<String, Vec<CustomTab>>,
    pub builtin: bool,
}

impl ModeManifest {
    /// The custom-tab set to actually render given the currently active
    /// submode: that submode's own override when one exists, else the
    /// parent mode's `custom_tabs`. `submode_id` empty (no submode active,
    /// or this mode has none) always uses `custom_tabs`.
    pub fn effective_custom_tabs(&self, submode_id: &str) -> &[CustomTab] {
        if !submode_id.is_empty() {
            if let Some(tabs) = self.submode_tabs.get(submode_id) {
                return tabs;
            }
        }
        &self.custom_tabs
    }
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
    #[serde(default)]
    submode_tabs: Vec<SubmodeTabSection>,
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
    /// Explicit mode-selection-menu color (hex). See `ModeManifest.menu_color`.
    #[serde(default)]
    color: Option<String>,
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
    /// Per-submode identity color (hex). See `SubmodeMeta.color`.
    #[serde(default)]
    color: Option<String>,
    /// Discipline glyph (allowlisted archetype id). See `SubmodeMeta.icon`.
    #[serde(default)]
    icon: Option<String>,
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

/// One `[[submode_tabs]]` entry — same shape as `TabSection` plus which
/// submode it belongs to. Several entries may share a `submode` (each
/// becomes one tab in that submode's set), and different submodes may reuse
/// the same `id` (e.g. both "civil" and "criminal" declaring a "case" tab) —
/// they're never rendered at the same time, so no collision.
#[derive(Deserialize)]
struct SubmodeTabSection {
    submode: String,
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

    // Submode-scoped custom tabs: same validation as mode-level custom tabs,
    // grouped by `submode`. Collision checks are scoped to their own
    // submode's bucket — different submodes may reuse the same tab id since
    // they never render at the same time.
    let mut submode_tabs: std::collections::HashMap<String, Vec<CustomTab>> =
        std::collections::HashMap::new();
    for t in f.submode_tabs {
        if t.submode.trim().is_empty() {
            warn!("mode '{}': submode_tabs entry missing 'submode' — skipped", f.mode.id);
            continue;
        }
        if t.id.trim().is_empty() || t.name.trim().is_empty() {
            warn!(
                "mode '{}': submode '{}' tab with empty id/name skipped",
                f.mode.id, t.submode
            );
            continue;
        }
        let bucket = submode_tabs.entry(t.submode.clone()).or_default();
        if KNOWN_TAB_IDS.contains(&t.id.as_str()) || bucket.iter().any(|c| c.id == t.id) {
            warn!(
                "mode '{}': submode '{}' tab id '{}' collides with a built-in or duplicate — skipped",
                f.mode.id, t.submode, t.id
            );
            continue;
        }
        let color = match t.color {
            Some(c) if crate::studio_theme::parse_hex(&c).is_some() => Some(c),
            Some(c) => {
                warn!(
                    "mode '{}': submode '{}' tab '{}' malformed color '{}' — ignored",
                    f.mode.id, t.submode, t.id, c
                );
                None
            }
            None => None,
        };
        bucket.push(CustomTab {
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

    // Menu color: validate hex, else drop (falls back to accent at render).
    let menu_color = match f.mode.color {
        Some(c) if crate::studio_theme::parse_hex(&c).is_some() => Some(c),
        Some(c) => {
            warn!("mode '{}': malformed menu color '{}' — ignored", f.mode.id, c);
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
        let color = match s.color {
            Some(c) if crate::studio_theme::parse_hex(&c).is_some() => Some(c),
            Some(c) => {
                warn!("mode '{}': submode '{}' malformed color '{}' — ignored", f.mode.id, s.id, c);
                None
            }
            None => None,
        };
        // Icon allowlist — mirrors `mode.icon`'s posture: a named archetype
        // id, never a file path. Unknown ids warn and drop (the dropdown
        // just renders no glyph for that row).
        let icon = match s.icon {
            Some(i) if crate::tool_metadata::TOOL_ICON_IDS.contains(&i.as_str()) => Some(i),
            Some(i) => {
                warn!("mode '{}': submode '{}' unknown icon '{}' — ignored", f.mode.id, s.id, i);
                None
            }
            None => None,
        };
        submodes.push(SubmodeMeta { id: s.id, name: s.name, required_role, color, icon });
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
        menu_color,
        required_role,
        submodes,
        custom_tabs,
        submode_tabs,
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
        (include_str!("../modes/government.toml"), "government"),
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

    /// Government is the twelve-discipline appropriation/administration mode.
    /// It is deliberately UNGATED: every discipline models an ANALYSIS seat,
    /// never an authority seat, so no discipline may quietly acquire a
    /// `required_role` without that being an explicit decision.
    #[test]
    fn government_mode_shape() {
        let modes = load_builtin_modes();
        let gov = modes.iter().find(|m| m.id == "government").expect("government mode parses");

        assert_eq!(gov.icon, "capitol");
        assert!(
            KNOWN_ICON_IDS.contains(&gov.icon.as_str()),
            "the mode icon must be allowlisted or it silently falls back to a gear"
        );
        assert!(gov.tabs.len() < KNOWN_TAB_IDS.len(), "government is a tab subset");
        assert!(gov.tabs.contains(&"mindspace".to_string()), "policy graphs need MindSpace");
        assert!(gov.tabs.contains(&"data".to_string()), "government is record work");

        // Analysis seats, not authority seats — mode and every discipline open.
        assert_eq!(gov.required_role, None, "government mode is public");
        assert_eq!(gov.submodes.len(), 12, "twelve disciplines");
        for sm in &gov.submodes {
            assert_eq!(
                sm.required_role, None,
                "discipline '{}' must stay ungated until real identity-backed roles exist",
                sm.id
            );
            assert_eq!(sm.color, None, "discipline '{}' renders default grey", sm.id);
        }

        // Every discipline carries its own tab set, and each leads with the
        // shared Jurisdiction intake tab. `effective_custom_tabs` OVERRIDES
        // rather than merges, so a discipline that omitted Jurisdiction would
        // silently lose data intake entirely — the exact regression this guards.
        for sm in &gov.submodes {
            let tabs = gov.effective_custom_tabs(&sm.id);
            assert!(tabs.len() >= 4, "discipline '{}' should have real tabs", sm.id);
            assert_eq!(
                tabs[0].id, "jurisdiction",
                "discipline '{}' must lead with the shared Jurisdiction tab",
                sm.id
            );
            let mut ids: Vec<&str> = tabs.iter().map(|t| t.id.as_str()).collect();
            let before = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(before, ids.len(), "discipline '{}' has a duplicate tab id", sm.id);
        }

        // The disciplines genuinely differ; they are not one tab set relabelled.
        let exec: Vec<&str> =
            gov.effective_custom_tabs("executive").iter().map(|t| t.id.as_str()).collect();
        let audit: Vec<&str> =
            gov.effective_custom_tabs("audit").iter().map(|t| t.id.as_str()).collect();
        assert_ne!(exec, audit, "Executive and Audit must not share a tab set");

        // Audit is the adversarial seat — the mode is only credible if the
        // falsification surface actually ships.
        assert!(
            audit.contains(&"audit-falsification") && audit.contains(&"audit-provenance"),
            "Audit must carry Falsification and Provenance: {audit:?}"
        );
    }

    #[test]
    fn builtins_parse() {
        let modes = load_builtin_modes();
        assert_eq!(modes.len(), 10, "all ten built-in modes must parse");
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

        // Submodes render in the default grey, like every other mode's
        // submenu (e.g. Business's) — no per-submode color override for
        // Justice or Military (reverted 2026-07-21 per explicit direction).
        for id in ["civil", "criminal", "judge"] {
            let sm = jus.submodes.iter().find(|s| s.id == id).unwrap();
            assert_eq!(sm.color, None, "justice submode '{id}' should NOT have a color override");
        }
        for branch in ["marines", "army", "navy", "coast-guard", "air-force", "space-force"] {
            let sm = mil.submodes.iter().find(|s| s.id == branch).unwrap();
            assert_eq!(sm.color, None, "military branch '{branch}' should NOT have a color override");
        }

        // Mode-level accent matches menu_color for every mode that sets an
        // explicit menu_color — one "core mode color" drives both the
        // dropdown/header identity AND the live selection-ring/active-tool
        // tint, instead of two values that can visually diverge.
        for id in ["business", "student", "justice", "military", "health", "government"] {
            let m = modes.iter().find(|m| m.id == id).unwrap();
            assert_eq!(
                m.accent, m.menu_color,
                "{id}: accent must match menu_color so the selection highlight reads as the mode's own color"
            );
        }

        // Justice's Civil and Criminal submodes each carry their OWN Case
        // tab (civil-procedure vs criminal-process shaped) — genuinely
        // different tabs/sections, not just a cosmetic label swap. Judge
        // (and no-submode-active) falls back to the mode-level tabs, which
        // deliberately have NO Case tab (judges preside, they don't build
        // cases) — only Docket + Evidence.
        assert!(!jus.custom_tabs.iter().any(|t| t.id == "case"), "fallback has no Case tab");
        assert!(jus.custom_tabs.iter().any(|t| t.id == "docket"));
        assert!(jus.custom_tabs.iter().any(|t| t.id == "evidence"));
        let civil_tabs = jus.effective_custom_tabs("civil");
        let criminal_tabs = jus.effective_custom_tabs("criminal");
        let civil_case = civil_tabs.iter().find(|t| t.id == "case").expect("civil has a Case tab");
        let criminal_case = criminal_tabs.iter().find(|t| t.id == "case").expect("criminal has a Case tab");
        assert_ne!(
            civil_case.sections.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
            criminal_case.sections.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
            "civil and criminal Case tabs must have genuinely different sections"
        );
        assert!(!civil_tabs.iter().any(|t| t.id == "evidence"), "civil uses Discovery, not an Evidence tab");
        assert!(criminal_tabs.iter().any(|t| t.id == "evidence"), "criminal keeps its own Evidence tab");
        // Judge now carries its own set: the Bench tab (chambers/courtroom/
        // sentencing) FIRST, then the same shared tabs as the fallback. The
        // no-submode state still falls back to the mode-level custom_tabs.
        let judge_tabs = jus.effective_custom_tabs("judge");
        assert_eq!(judge_tabs.len(), jus.custom_tabs.len() + 1, "judge = Bench + shared set");
        assert_eq!(judge_tabs[0].id, "bench", "Bench leads the judge tab strip");
        assert!(!judge_tabs.iter().any(|t| t.id == "case"), "judges preside, they don't build cases");
        assert_eq!(jus.effective_custom_tabs("").len(), jus.custom_tabs.len());

        // Metrics/Reform/Reference (added 2026-07-21) apply regardless of
        // which submode is active — unlike Case, which genuinely differs
        // per-submode — so they must be present in the Judge/no-submode
        // fallback AND duplicated into both Criminal's and Civil's
        // submode_tabs (submode_tabs REPLACES custom_tabs, it doesn't merge).
        for tabs in [jus.effective_custom_tabs(""), civil_tabs, criminal_tabs] {
            for id in ["metrics", "reform", "reference"] {
                assert!(tabs.iter().any(|t| t.id == id), "'{id}' tab missing from one of Judge/Civil/Criminal");
            }
        }

        // Legal's new tabs (added 2026-07-21) sit alongside the pre-existing
        // Case/Discovery without disturbing them — the pitch's stated "zero
        // breaking changes" principle.
        let legal = modes.iter().find(|m| m.id == "legal").unwrap();
        let legal_case = legal.custom_tabs.iter().find(|t| t.id == "case").expect("legal keeps its Case tab");
        for section in ["Files", "Parties", "Chronology", "Strategy"] {
            assert!(
                legal_case.sections.iter().any(|s| s.name == section),
                "legal Case must keep its original '{section}' section"
            );
        }
        for id in ["research", "draft", "clients", "metrics"] {
            assert!(legal.custom_tabs.iter().any(|t| t.id == id), "legal '{id}' tab missing");
        }

        // Legal's practice-area rename: "mitigation" -> "mediation" (the
        // standard Litigation/Mediation/Arbitration ADR trio, confirmed with
        // the user — not the narrower sentencing-mitigation concept).
        assert!(legal.submodes.iter().any(|s| s.id == "mediation"), "legal should have a 'mediation' submode");
        assert!(!legal.submodes.iter().any(|s| s.id == "mitigation"), "legal's 'mitigation' id should be renamed away");

        // ── Per-discipline submode_tabs (2026-07-21): each of Student/
        // Engineering/Business/Health/Military/Legal's submodes gets its own
        // distinct tab beyond the shared/universal ones, which must be
        // carried forward into EVERY submode's block (never dropped by
        // omission — the exact regression this test guards against).

        let student = modes.iter().find(|m| m.id == "student").unwrap();
        for sub in &student.submodes {
            let tabs = student.effective_custom_tabs(&sub.id);
            for id in ["notebook", "assignments"] {
                assert!(tabs.iter().any(|t| t.id == id), "student '{}' missing universal tab '{id}'", sub.id);
            }
            assert!(
                tabs.len() >= 3,
                "student '{}' should have its own subject tab beyond Notebook+Assignments",
                sub.id
            );
        }

        let engineering = modes.iter().find(|m| m.id == "engineering").unwrap();
        assert_eq!(engineering.submodes.len(), 7, "engineering has seven disciplines (civil lives at its own mode)");
        assert!(!engineering.submodes.iter().any(|s| s.id == "civil"), "engineering should not duplicate the standalone Civil mode");
        for sub in &engineering.submodes {
            let tabs = engineering.effective_custom_tabs(&sub.id);
            // Each discipline gets its own 3-tab "dream scenario" set
            // (2026-07-21), e.g. "mechanical-fundamentals" / "-design-
            // assembly" / "-analysis" — no single tab literally named after
            // the discipline id anymore, so check the count instead.
            assert_eq!(tabs.len(), 3, "engineering '{}' should have exactly 3 of its own discipline tabs", sub.id);
        }

        let business = modes.iter().find(|m| m.id == "business").unwrap();
        for sub in &business.submodes {
            let tabs = business.effective_custom_tabs(&sub.id);
            if sub.id == "operations" {
                // Deliberately no override — falls back to the plain
                // Product+Manufacturing+Operations set, which already IS
                // this function's content.
                assert_eq!(tabs.len(), business.custom_tabs.len(), "business 'operations' submode should fall back, not override");
                continue;
            }
            for id in ["product", "operations"] {
                assert!(tabs.iter().any(|t| t.id == id), "business '{}' missing universal tab '{id}'", sub.id);
            }
            if sub.id == "supply-chain" {
                assert!(tabs.iter().any(|t| t.id == "manufacturing"), "business 'supply-chain' should carry Manufacturing forward");
            } else {
                assert!(!tabs.iter().any(|t| t.id == "manufacturing"), "business '{}' should NOT show fabrication tools", sub.id);
            }
        }

        let health = modes.iter().find(|m| m.id == "health").unwrap();
        for sub in &health.submodes {
            let tabs = health.effective_custom_tabs(&sub.id);
            for id in ["patients", "vitals"] {
                assert!(tabs.iter().any(|t| t.id == id), "health '{}' missing universal tab '{id}'", sub.id);
            }
        }

        // military's Operations->Map dead `data:overlay` button, fixed
        // 2026-07-21 (matches civil.toml's identical earlier fix).
        let military_map = mil.custom_tabs.iter().find(|t| t.id == "operations").unwrap()
            .sections.iter().find(|s| s.name == "Map").unwrap();
        assert_eq!(military_map.tools, vec!["data:grid".to_string()], "military Map section should not reference the dead 'data:overlay' id");
        for sub in &mil.submodes {
            let tabs = mil.effective_custom_tabs(&sub.id);
            for id in ["operations", "logistics"] {
                assert!(tabs.iter().any(|t| t.id == id), "military '{}' missing universal tab '{id}'", sub.id);
            }
        }

        for sub in &legal.submodes {
            let tabs = legal.effective_custom_tabs(&sub.id);
            for id in ["discovery", "research", "draft", "clients", "metrics"] {
                assert!(tabs.iter().any(|t| t.id == id), "legal '{}' missing universal tab '{id}'", sub.id);
            }
            // Each practice area gets its OWN 3-tab "dream scenario" set
            // (2026-07-21) beyond the 5 universal tabs above — no longer a
            // single generic "case" id, so check for genuine specialization
            // (extra tabs whose ids aren't among the 5 universal ones) instead.
            let own_tabs = tabs.iter().filter(|t| !["discovery", "research", "draft", "clients", "metrics"].contains(&t.id.as_str())).count();
            assert_eq!(own_tabs, 3, "legal '{}' should have exactly 3 of its own specialized tabs", sub.id);
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

    #[test]
    fn builtins_fully_populated() {
        // The "complete for every single option" guarantee (2026-07-22):
        // every section of every built-in mode/submode has at least one
        // button, every button has generated display metadata (label /
        // tooltip / archetype icon — regenerate with
        // `python scripts/gen_tool_metadata.py` after editing a manifest),
        // and every submode carries an allowlisted discipline glyph. A
        // failure here means a manifest edit shipped a bare button, an
        // empty "Coming soon" section, or a glyph the Slint resolver can't
        // draw. (User manifests may still do all of these — the synthetic
        // tests below keep those degradation paths covered.)
        let modes = load_builtin_modes();
        for m in &modes {
            let mut tab_sets: Vec<&[CustomTab]> = vec![m.custom_tabs.as_slice()];
            for sm in &m.submodes {
                assert!(
                    sm.icon.is_some(),
                    "{}/{}: submode has no icon (allowlist: tool_metadata::TOOL_ICON_IDS)",
                    m.id, sm.id
                );
                tab_sets.push(m.effective_custom_tabs(&sm.id));
            }
            for tabs in tab_sets {
                for tab in tabs {
                    for sec in &tab.sections {
                        assert!(
                            !sec.tools.is_empty(),
                            "{}: tab '{}' section '{}' is empty — every built-in section must have buttons",
                            m.id, tab.id, sec.name
                        );
                        for tool in &sec.tools {
                            let meta = crate::tool_metadata::tool_meta(tool);
                            let meta = meta.unwrap_or_else(|| panic!(
                                "{}: tool '{}' has no generated metadata — rerun scripts/gen_tool_metadata.py",
                                m.id, tool
                            ));
                            assert!(
                                crate::tool_metadata::TOOL_ICON_IDS.contains(&meta.icon),
                                "{}: tool '{}' icon '{}' missing from TOOL_ICON_IDS",
                                m.id, tool, meta.icon
                            );
                            assert!(!meta.label.is_empty() && !meta.tooltip.is_empty());
                            // Composed per-tool icon (wave 3): every id gets
                            // its own base+badge SVG — rerun
                            // scripts/gen_tool_icons.py after manifest edits.
                            let composed = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                                .join("assets/icons/tools")
                                .join(format!("{}.svg", tool.replace(':', "__").replace('-', "_")));
                            assert!(
                                composed.exists(),
                                "{}: tool '{}' missing composed icon {:?} — rerun scripts/gen_tool_icons.py",
                                m.id, tool, composed
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn submode_tabs_group_by_submode_and_allow_id_reuse() {
        let text = r##"
[mode]
id = "t3"
name = "T3"
[ribbon]
tabs = ["home"]
[[submodes]]
id = "a"
name = "A"
color = "#112233"
[[submodes]]
id = "b"
name = "B"

[[submode_tabs]]
submode = "a"
id = "shared"
name = "A's tab"
  [[submode_tabs.sections]]
  name = "Only in A"
  tools = []

[[submode_tabs]]
submode = "b"
id = "shared"
name = "B's tab"
  [[submode_tabs.sections]]
  name = "Only in B"
  tools = []
"##;
        let m = parse_mode_toml(text, false).unwrap();
        assert_eq!(m.submodes.iter().find(|s| s.id == "a").unwrap().color.as_deref(), Some("#112233"));
        assert_eq!(m.submodes.iter().find(|s| s.id == "b").unwrap().color, None);

        // Same tab id ("shared") reused across two different submodes is
        // NOT a collision — they never render at the same time.
        let a_tabs = m.effective_custom_tabs("a");
        let b_tabs = m.effective_custom_tabs("b");
        assert_eq!(a_tabs.len(), 1);
        assert_eq!(a_tabs[0].name, "A's tab");
        assert_eq!(b_tabs.len(), 1);
        assert_eq!(b_tabs[0].name, "B's tab");
        // A submode with no override, or no submode at all, falls back to
        // the (here empty) mode-level custom_tabs.
        assert!(m.effective_custom_tabs("c").is_empty());
        assert!(m.effective_custom_tabs("").is_empty());
    }

    #[test]
    fn submode_tabs_duplicate_id_within_same_submode_skipped() {
        let text = r##"
[mode]
id = "t4"
name = "T4"
[ribbon]
tabs = ["home"]
[[submodes]]
id = "a"
name = "A"

[[submode_tabs]]
submode = "a"
id = "dup"
name = "First"
[[submode_tabs]]
submode = "a"
id = "dup"
name = "Second"
[[submode_tabs]]
submode = "a"
id = "data"
name = "Collides with built-in"
"##;
        let m = parse_mode_toml(text, false).unwrap();
        let a_tabs = m.effective_custom_tabs("a");
        assert_eq!(a_tabs.len(), 1, "duplicate id within the same submode dropped, built-in-colliding id dropped");
        assert_eq!(a_tabs[0].name, "First");
    }
}

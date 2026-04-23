//! # Accessibility scaffold (Phase 2)
//!
//! Slint's accessibility surface (`accessible-role`, `accessible-label`,
//! AT-SPI / UIA bindings) is under development upstream. Until the
//! full API ships, Eustress collects a **design-time accessibility
//! manifest** — every UI element that *should* carry a role + label
//! registers one here. When Slint's API arrives, the bridge lifts
//! these into the live accessibility tree in one pass without
//! touching every component.
//!
//! ## Why bother now
//!
//! - Keeps the labeling work distributed — each feature PR adds its
//!   own accessibility metadata alongside the code it owns.
//! - Gives screen readers a Rust-side introspection surface
//!   immediately via the [`AccessibilityManifest::describe`] API
//!   callable from MCP / Rune / RPA scripts.
//! - Documents the wiring point for Slint's binding when it lands —
//!   the `apply_to_slint_window` helper below is the single
//!   integration site.
//!
//! ## Usage
//!
//! ```ignore
//! fn register_labels(mut manifest: ResMut<AccessibilityManifest>) {
//!     manifest.declare(
//!         "tool_options_bar.cancel_button",
//!         AccessibleRole::Button,
//!         "Cancel active tool",
//!     );
//! }
//! ```
//!
//! The string key is the component's DOM-ish path. Downstream Slint
//! wiring will match keys to the corresponding Slint element ids
//! via the `accessible-name` property Slint ships when the binding
//! lands.

use bevy::prelude::*;
use std::collections::HashMap;

/// ARIA-style semantic roles. Subset of what WAI-ARIA defines,
/// restricted to what the Eustress UI needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessibleRole {
    Button,
    Checkbox,
    Toggle,
    Slider,
    TextInput,
    List,
    ListItem,
    Dialog,
    Tab,
    TabPanel,
    Menu,
    MenuItem,
    ProgressBar,
    Status,
    Heading,
    Group,
    Tree,
    TreeItem,
    /// Catch-all for elements that don't map cleanly; still gets a
    /// label so screen readers have text.
    Generic,
}

impl AccessibleRole {
    pub fn as_str(self) -> &'static str {
        match self {
            AccessibleRole::Button       => "button",
            AccessibleRole::Checkbox     => "checkbox",
            AccessibleRole::Toggle       => "toggle",
            AccessibleRole::Slider       => "slider",
            AccessibleRole::TextInput    => "textbox",
            AccessibleRole::List         => "list",
            AccessibleRole::ListItem     => "listitem",
            AccessibleRole::Dialog       => "dialog",
            AccessibleRole::Tab          => "tab",
            AccessibleRole::TabPanel     => "tabpanel",
            AccessibleRole::Menu         => "menu",
            AccessibleRole::MenuItem     => "menuitem",
            AccessibleRole::ProgressBar  => "progressbar",
            AccessibleRole::Status       => "status",
            AccessibleRole::Heading      => "heading",
            AccessibleRole::Group        => "group",
            AccessibleRole::Tree         => "tree",
            AccessibleRole::TreeItem     => "treeitem",
            AccessibleRole::Generic      => "generic",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AccessibleEntry {
    pub role: AccessibleRole,
    pub label: String,
    /// Optional extended description for screen readers.
    pub description: Option<String>,
    /// Optional keyboard shortcut hint (e.g. "Ctrl+Z").
    pub shortcut: Option<String>,
}

/// Central ARIA-style manifest. Each feature plugin populates this
/// at startup or lazily. Keys are DOM-ish path strings — e.g.
/// `"ribbon.cad.pattern.linear_array"`.
#[derive(Resource, Debug, Default)]
pub struct AccessibilityManifest {
    entries: HashMap<String, AccessibleEntry>,
}

impl AccessibilityManifest {
    pub fn declare(&mut self, key: impl Into<String>, role: AccessibleRole, label: impl Into<String>) {
        self.entries.insert(key.into(), AccessibleEntry {
            role,
            label: label.into(),
            description: None,
            shortcut: None,
        });
    }

    pub fn declare_full(
        &mut self,
        key: impl Into<String>,
        role: AccessibleRole,
        label: impl Into<String>,
        description: Option<String>,
        shortcut: Option<String>,
    ) {
        self.entries.insert(key.into(), AccessibleEntry {
            role, label: label.into(), description, shortcut,
        });
    }

    pub fn get(&self, key: &str) -> Option<&AccessibleEntry> {
        self.entries.get(key)
    }

    /// Flatten into (key, role, label) triples for external
    /// consumers (MCP `describe_ui` tool, RPA tests, reviewer
    /// audits).
    pub fn describe(&self) -> Vec<(String, &'static str, String)> {
        self.entries.iter().map(|(k, v)| {
            (k.clone(), v.role.as_str(), v.label.clone())
        }).collect()
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct AccessibilityPlugin;

impl Plugin for AccessibilityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AccessibilityManifest>()
            .add_systems(Startup, seed_core_labels);
    }
}

/// Populate the manifest with the most important UI elements. Other
/// feature plugins can declare their own entries independently.
fn seed_core_labels(mut manifest: ResMut<AccessibilityManifest>) {
    use AccessibleRole::*;
    // Viewport controls.
    manifest.declare("viewport.move_tool",   Button, "Move tool");
    manifest.declare("viewport.scale_tool",  Button, "Scale tool");
    manifest.declare("viewport.rotate_tool", Button, "Rotate tool");
    manifest.declare("viewport.select_tool", Button, "Select tool");

    // Ribbon CAD tab.
    manifest.declare("ribbon.cad.gap_fill",         Button, "Gap Fill");
    manifest.declare("ribbon.cad.resize_align",     Button, "Resize Align");
    manifest.declare("ribbon.cad.edge_align",       Button, "Edge Align");
    manifest.declare("ribbon.cad.part_swap",        Button, "Part Swap");
    manifest.declare("ribbon.cad.mirror",           Button, "Mirror / Model Reflect");
    manifest.declare("ribbon.cad.linear_array",     Button, "Linear Array");
    manifest.declare("ribbon.cad.radial_array",     Button, "Radial Array");
    manifest.declare("ribbon.cad.grid_array",       Button, "Grid Array");
    manifest.declare("ribbon.cad.align_x_center",   Button, "Align selection X center");
    manifest.declare("ribbon.cad.align_y_center",   Button, "Align selection Y center");
    manifest.declare("ribbon.cad.align_z_center",   Button, "Align selection Z center");
    manifest.declare("ribbon.cad.distribute_x",     Button, "Distribute evenly along X");
    manifest.declare("ribbon.cad.distribute_y",     Button, "Distribute evenly along Y");
    manifest.declare("ribbon.cad.distribute_z",     Button, "Distribute evenly along Z");

    // Bottom-panel tabs.
    manifest.declare("bottom_panel.output_tab",   Tab, "Output console");
    manifest.declare("bottom_panel.timeline_tab", Tab, "Timeline");

    // Floating Numeric Input.
    manifest.declare_full(
        "floating_numeric_input",
        TextInput,
        "Numeric input for gizmo drag",
        Some("Type a number to commit an exact value. Press Enter to confirm, Escape to cancel.".into()),
        None,
    );

    // Tool Options Bar.
    manifest.declare("tool_options_bar",                 Group,  "Active tool options");
    manifest.declare("tool_options_bar.advanced_toggle", Toggle, "Show advanced options");
    manifest.declare("tool_options_bar.cancel_button",   Button, "Cancel active tool");

    // Toast Undo.
    manifest.declare_full(
        "toast_undo.undo_button",
        Button,
        "Undo last action",
        Some("Reverts the action described in the toast.".into()),
        Some("Ctrl+Z".into()),
    );
    manifest.declare("toast_undo.dismiss", Button, "Dismiss toast");

    // Timeline.
    manifest.declare("timeline.filter_button",    Button,   "Open timeline filter");
    manifest.declare("timeline.legend.keyframe",  Toggle,   "Toggle keyframe visibility");
    manifest.declare("timeline.legend.watchpoint",Toggle,   "Toggle watchpoint visibility");
    manifest.declare("timeline.legend.breakpoint",Toggle,   "Toggle breakpoint visibility");
    manifest.declare("timeline.search",           TextInput,"Search timeline events");
    manifest.declare("timeline.filter_modal",     Dialog,   "Timeline tag filter");

    info!("♿ Accessibility manifest seeded with {} entries", manifest.len());
}

// ============================================================================
// Slint integration hook — lands when Slint exposes the API
// ============================================================================

/// Single integration point for when Slint ships `accessible-role` +
/// `accessible-label` + AT-SPI/UIA bindings. At that point this
/// function walks the manifest and calls the eventual Slint API.
///
/// Today: logs the first 10 entries for diagnostic parity and
/// returns. Harmless; no-op in production.
pub fn apply_to_slint_window(
    manifest: &AccessibilityManifest,
) {
    let total = manifest.len();
    if total == 0 { return; }
    let sample: Vec<_> = manifest.describe().into_iter().take(10).collect();
    debug!(
        "♿ Accessibility manifest has {} entries (first {}: {:?}) — apply_to_slint_window \
         is a no-op until Slint exposes the API",
        total, sample.len(), sample
    );
}

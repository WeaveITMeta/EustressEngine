//! # GUI Element Loader — Parse .textlabel.toml, .frame.toml, etc. into Bevy UI entities
//!
//! ## Table of Contents
//!
//! 1. GuiTomlDefinition — deserialization structs for GUI TOML files
//! 2. load_gui_definition — parse a GUI TOML file from disk
//! 3. spawn_gui_element — spawn a Bevy UI entity with proper rendering components
//!
//! ## Architecture
//!
//! GUI TOML files in StarterGui use this format:
//!   [instance]     — name
//!   [metadata]     — class_name, archivable
//!   [gui]          — position, size, background_color, border_size, z_index, etc.
//!   [text]         — text, text_color, font_size, alignment (TextLabel/TextButton only)
//!
//! Each element is spawned with Bevy UI components (Node, BackgroundColor, Text, etc.)
//! so they render visually in the viewport overlay. ScreenGui is a fullscreen root container;
//! child elements (Frame, TextLabel, TextButton) are positioned absolutely within it.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::spawn::{TextLabelMarker, TextBoxMarker};

pub use eustress_common::gui::billboard_renderer::{BillboardGuiMarker, SurfaceGuiMarker};

// Re-export from common crate so engine code can use it
pub use eustress_common::gui::billboard_renderer::GuiElementDisplay;

// ============================================================================
// 1. TOML deserialization structs
// ============================================================================

/// Top-level GUI TOML file structure
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlFile {
    #[serde(default)]
    pub instance: GuiTomlInstance,
    #[serde(default)]
    pub metadata: GuiTomlMetadata,
    #[serde(default)]
    pub gui: GuiTomlProperties,
    #[serde(default)]
    pub text: Option<GuiTomlText>,
    // ScreenGui files may use instance_loader format with [asset], [transform], [properties]
    #[serde(default)]
    pub asset: Option<toml::Value>,
    #[serde(default)]
    pub transform: Option<toml::Value>,
    #[serde(default)]
    pub properties: Option<toml::Value>,
    /// `CollectionService` tags for this GUI instance — written from
    /// `LuauCreatedInstance.tags` and hydrated into the ECS
    /// [`Tags`](eustress_common::attributes::Tags) component on space load.
    /// Same shape and semantics as `instance_loader::InstanceDefinition.tags`,
    /// so MCP `add_tag` / `get_tagged_entities` see GUI tags identically to
    /// Part tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// [instance] section
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlInstance {
    #[serde(default)]
    pub name: String,
}

/// [metadata] section
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlMetadata {
    #[serde(default)]
    pub class_name: String,
    #[serde(default)]
    pub archivable: bool,
    /// Authored unit symbol (`"m"`, `"cm"`, `"mm"`, `"ft"`, `"in"`,
    /// `"studs"`). `None` means the file was authored without a unit
    /// declaration → treat as engine-native meters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// [gui] section — shared visual properties
///
/// **Roblox parity, no legacy paths.** `position` and `size` are
/// strictly `UDim2` (4-float `[scale_x, offset_x, scale_y, offset_y]`).
/// The previous `Vec<f32>` accept-anything form has been removed —
/// older TOML files using `[x, y]` / `[w, h]` will fail to load and
/// must be migrated to the 4-tuple form.
#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct GuiTomlProperties {
    #[serde(default)]
    pub position: eustress_common::ui_types::UDim2,
    #[serde(default = "default_size_udim2")]
    pub size: eustress_common::ui_types::UDim2,
    #[serde(default)]
    pub anchor_point: [f32; 2],
    #[serde(default = "default_bg_color")]
    pub background_color: [f32; 4],
    #[serde(default)]
    pub border_size: f32,
    #[serde(default = "default_border_color")]
    pub border_color: [f32; 4],
    #[serde(default)]
    pub corner_radius: f32,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub z_index: i32,

    // ── BillboardGui-specific properties ──────────────────────────────────
    // Behaviour flags
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always_on_top: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clips_descendants: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_on_spawn: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stiffness_by_distance: Option<bool>,

    // Distance
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_distance: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance_lower_limit: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance_upper_limit: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance_step: Option<f32>,

    // Appearance
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_influence: Option<f32>,

    // Offsets — skip-if-none so files don't bloat with zeroes.
    /// Roblox `SizeOffset` — `Vector2` `[offset_x, offset_y]` in pixels.
    /// Lenient deserialize accepts BOTH the new 2-tuple and the older
    /// 4-tuple UDim2 shape `[scale_x, offset_x, scale_y, offset_y]` —
    /// when migrating from a previous version the existing offset
    /// components survive even though Scale is discarded (Roblox parity
    /// makes Scale meaningless for `SizeOffset`).
    #[serde(default, deserialize_with = "deserialize_size_offset_lenient", skip_serializing_if = "Option::is_none")]
    pub size_offset: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extents_offset: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extents_offset_world_space: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units_offset: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units_offset_world_space: Option<[f32; 3]>,

    // Sorting
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_index_behavior: Option<String>,  // "Sibling" | "Global"

    // Adornee — instance name reference; resolved to entity at load time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adornee: Option<String>,
}

/// [text] section — text-specific properties for TextLabel, TextButton, TextBox
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GuiTomlText {
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_text_color")]
    pub text_color: [f32; 4],
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub font_family: String,
    /// Font family variant — e.g. "GothamBold", "SourceSans", "RobotoMono".
    /// Feeds font_weight derivation: names containing "Bold" → 700, else 400.
    #[serde(default)]
    pub font: String,
    #[serde(default = "default_left")]
    pub text_x_alignment: String,
    #[serde(default = "default_center")]
    pub text_y_alignment: String,
    /// Roblox `TextScaled`. When `true` the renderer ignores `font_size`
    /// and auto-fits the text to the element's rect (binary search).
    /// `skip_serializing_if = false` keeps TOMLs that haven't opted in
    /// from carrying a redundant `text_scaled = false` line.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub text_scaled: bool,
}

/// Map a font name to a CSS-style weight. Conservative: anything with "Bold"
/// becomes 700, "Light" becomes 300, otherwise 400. Slint's software renderer
/// uses the weight to pick the right variant when multiple are registered.
pub fn font_weight_from_name(name: &str) -> i32 {
    let lower = name.to_lowercase();
    if lower.contains("bold") { 700 }
    else if lower.contains("light") || lower.contains("thin") { 300 }
    else { 400 }
}

fn default_size_udim2() -> eustress_common::ui_types::UDim2 {
    eustress_common::ui_types::UDim2::from_pixels(100.0, 30.0)
}
fn default_bg_color() -> [f32; 4] { [0.2, 0.2, 0.2, 0.8] }
fn default_border_color() -> [f32; 4] { [0.5, 0.5, 0.5, 1.0] }
fn default_text_color() -> [f32; 4] { [1.0, 1.0, 1.0, 1.0] }
fn default_font_size() -> f32 { 14.0 }
fn default_left() -> String { "Left".to_string() }
fn default_center() -> String { "Center".to_string() }
fn default_true() -> bool { true }

/// Accept the new 2-element `[offset_x, offset_y]` form for `size_offset`.
/// Legacy 4-element UDim2 shapes are treated as missing (default `[0, 0]`):
/// an earlier code path wrote `size_offset` as a UDim2 carrying the same
/// values as `size`, so naively extracting `[arr[1], arr[3]]` would
/// migrate stale duplicate data into the Vector2 field. Roblox's
/// `SizeOffset` default is `(0, 0)`; resetting on migration matches that
/// and the save-on-change pass writes the cleaned value back to disk on
/// the first edit. Users with legitimately-set non-zero values will need
/// to re-enter them once after upgrade — acceptable since the field is
/// rarely used and the cleanup eliminates a confusing redundancy.
fn deserialize_size_offset_lenient<'de, D>(de: D) -> Result<Option<[f32; 2]>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let v: Option<Vec<f32>> = Option::deserialize(de)?;
    Ok(v.and_then(|arr| match arr.len() {
        2 => Some([arr[0], arr[1]]),
        // Legacy 4-tuple → reset to default. See doc above.
        _ => None,
    }))
}

// ============================================================================
// 2. Create / Write GUI TOML files
// ============================================================================

/// Create a default GUI TOML definition for a given class name.
///
/// Returns a `GuiTomlFile` with sensible defaults that can be serialized to disk.
pub fn create_default_gui_toml(class_name: &str, display_name: &str) -> GuiTomlFile {
    let has_text = matches!(
        class_name,
        "TextLabel" | "TextButton" | "TextBox"
    );

    let text = if has_text {
        Some(GuiTomlText {
            text: display_name.to_string(),
            text_color: default_text_color(),
            font_size: default_font_size(),
            font_family: String::new(),
            // `font` is a newer field alongside `font_family`; drives
            // the billboard card's `font-weight` binding via the
            // `font_weight_from_name` helper. Default is empty — tools
            // read either field.
            font: String::new(),
            text_x_alignment: "Center".to_string(),
            text_y_alignment: "Center".to_string(),
            text_scaled: false,
        })
    } else {
        None
    };

    // Strict UDim2 sizes per class. Pure-pixel offsets (Scale=0) for
    // most; ScreenGui inherits its viewport so size is ignored.
    let size = match class_name {
        "ScreenGui"                => eustress_common::ui_types::UDim2::default(),
        "Frame" | "ScrollingFrame" => eustress_common::ui_types::UDim2::from_pixels(200.0, 150.0),
        _                          => default_size_udim2(),
    };

    let bg = match class_name {
        "ScreenGui" => [0.0, 0.0, 0.0, 0.0], // Transparent
        "TextButton" => [0.25, 0.25, 0.3, 0.9],
        _ => default_bg_color(),
    };

    GuiTomlFile {
        instance: GuiTomlInstance {
            name: display_name.to_string(),
        },
        metadata: GuiTomlMetadata {
            class_name: class_name.to_string(),
            archivable: true,
            unit: None,
        },
        gui: GuiTomlProperties {
            position: eustress_common::ui_types::UDim2::default(),
            size,
            anchor_point: [0.0, 0.0],
            background_color: bg,
            border_size: if class_name == "TextBox" { 1.0 } else { 0.0 },
            border_color: default_border_color(),
            corner_radius: if class_name == "TextButton" { 4.0 } else { 0.0 },
            visible: true,
            z_index: 0,
            // BillboardGui-specific fields default to None — the loader
            // applies class defaults when these are absent. Listing them
            // explicitly is wasteful; `..Default::default()` covers the
            // full optional set in one line.
            ..Default::default()
        },
        text,
        asset: None,
        transform: None,
        properties: None,
        tags: Vec::new(),
    }
}

/// Persist a GUI definition.
///
/// DB-first (full conversion): a converted Space writes the binary GUI
/// record into Fjall — no disk, no TOML serialise. Disk-TOML write
/// happens ONLY for a legacy un-converted world (no active Fjall DB).
pub fn write_gui_toml(path: &Path, gui_def: &GuiTomlFile) -> Result<(), String> {
    if crate::space::active_db::put_gui(path, gui_def) {
        return Ok(());
    }
    let content = toml::to_string_pretty(gui_def)
        .map_err(|e| format!("Failed to serialize GUI TOML: {}", e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
    }
    // Atomic-write + retry. Windows reports `os error 32` ("file in
    // use by another process") whenever something has the file open
    // without the FILE_SHARE_WRITE flag — antivirus, text editors,
    // and the engine's own reload-after-write pass all trip this
    // race transiently. A naive `std::fs::write` would lose the edit
    // silently, which is exactly the bug users reported as
    // "copy-paste keeps the defaults" — the source save never landed
    // on disk, so the copy read the pre-edit content. (Historically
    // this was made worse by a redundant `notify::Watcher` in the
    // streaming module; that was consolidated away on 2026-05-12.)
    //
    // Strategy:
    //   1. Write to `<path>.tmp` (a separate file that no reader is
    //      currently holding).
    //   2. Rename `<path>.tmp` → `<path>`. On Windows, `rename` over an
    //      existing file is implemented as `MoveFileEx` with the
    //      REPLACE_EXISTING flag; it's atomic from the filesystem's
    //      perspective, so any reader sees either the OLD bytes or the
    //      NEW bytes — never a half-written mix.
    //   3. Retry the rename a few times on transient `os error 32`,
    //      since AV/file watchers can still hold a brief lock on the
    //      destination at the moment of replacement.
    write_atomic(path, content.as_bytes())
        .map_err(|e| format!("Failed to write GUI TOML {:?}: {}", path, e))
}

/// Atomic-write helper used by every TOML save path in the workspace.
/// Writes to a sibling `.tmp` file, then renames it onto `path`. The
/// rename is the atomic step from the filesystem's perspective; any
/// concurrent reader sees either the OLD complete bytes or the NEW
/// complete bytes, never a mid-write torn read. Retries the rename a
/// few times when Windows reports the destination is briefly locked
/// (antivirus mid-scan, file watcher mid-reload, text-editor reads)
/// so the transient share-mode conflict doesn't drop a user edit.
///
/// Since the workspace now has exactly ONE notify watcher
/// (`engine::space::file_watcher`, which marks paths it just wrote as
/// `recently_written` so it skips its own reload pass), the retry
/// count of 8 is generous — under normal operation the first rename
/// succeeds. The retries exist as a guard against external readers
/// (AV, text editor, OS indexer) that we don't control.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let tmp_path = parent.join(format!(".{}.tmp", stem));
    std::fs::write(&tmp_path, bytes)?;

    // Three attempts is the sweet spot now that the workspace runs
    // exactly one notify watcher (the engine's own, which
    // self-suppresses via `recently_written`). The first try
    // succeeds in the steady state; the second/third absorb the
    // rare external-reader collision (AV scan, indexer, text-editor
    // reload). A longer ladder is just sleep time after a real
    // permission error.
    const ATTEMPTS: u32 = 3;
    let mut last_err: Option<std::io::Error> = None;
    for i in 0..ATTEMPTS {
        match std::fs::rename(&tmp_path, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                let kind = e.kind();
                let transient = matches!(
                    kind,
                    std::io::ErrorKind::PermissionDenied
                        | std::io::ErrorKind::Other,
                );
                if !transient {
                    let _ = std::fs::remove_file(&tmp_path);
                    return Err(e);
                }
                last_err = Some(e);
                if i + 1 < ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        }
    }
    let _ = std::fs::remove_file(&tmp_path);
    Err(last_err.unwrap_or_else(|| std::io::Error::new(
        std::io::ErrorKind::Other,
        "write_atomic: exhausted retries without a recorded error",
    )))
}

// ============================================================================
// 3. load_gui_definition — parse from disk
// ============================================================================

/// In-memory twin of [`load_gui_definition`] — parses GUI TOML from
/// content the caller already sourced (Fjall tree or disk), identical
/// key-normalise + strict deserialize, no `std::fs`. The
/// SpaceSource-threaded loader uses this so GUI entities load from a
/// Fjall-authoritative world with zero disk reads.
pub fn load_gui_definition_from_str(content: &str) -> Result<GuiTomlFile, String> {
    let mut value: toml::Value = content
        .parse()
        .map_err(|e: toml::de::Error| format!("Failed to parse GUI TOML: {}", e))?;
    eustress_common::class_schema::normalise_keys(&mut value);
    let parsed: GuiTomlFile = value
        .try_into()
        .map_err(|e: toml::de::Error| format!("Failed to deserialize GUI TOML: {}", e))?;
    Ok(parsed)
}

/// Load and parse a GUI TOML file from disk
pub fn load_gui_definition(path: &Path) -> Result<GuiTomlFile, String> {
    // DB-first (full conversion): a converted Space serves the binary
    // GUI record from Fjall — zero disk. Every billboard/slint/tool
    // call site funnels through here. Disk read only for a legacy
    // un-converted world (no active Fjall DB).
    if let Some(def) = crate::space::active_db::get_gui(path) {
        return Ok(def);
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read GUI file {:?}: {}", path, e))?;
    // Parse to a generic `toml::Value` first and run the shared key
    // normalisation pass. Any TOML left over from the aborted PascalCase
    // migration (`[Gui]`, `[Metadata]`, `[Text]`) gets flipped back to
    // snake_case before the strict `GuiTomlFile` deserialize, so ScreenGui /
    // Frame / TextLabel / etc. all keep loading regardless of which
    // direction the file on disk currently uses.
    let mut value: toml::Value = content
        .parse()
        .map_err(|e: toml::de::Error| format!("Failed to parse GUI TOML {:?}: {}", path, e))?;
    eustress_common::class_schema::normalise_keys(&mut value);
    let parsed: GuiTomlFile = value
        .try_into()
        .map_err(|e: toml::de::Error| format!("Failed to deserialize GUI TOML {:?}: {}", path, e))?;
    Ok(parsed)
}

/// Determine the GUI element type from a path. Prefers the legacy
/// compound-extension form (`Name.textlabel.toml`) when present so
/// existing flat files keep working, and falls back to reading
/// `[metadata] class_name` from `_instance.toml` for the
/// folder-per-element layout the Insert menu now emits.
///
/// Returns `"Frame"` when neither the extension nor the TOML
/// metadata yields a recognized class — the old default.
pub fn gui_class_from_extension(path: &Path) -> &'static str {
    let path_str = path.to_string_lossy();
    if path_str.ends_with(".screengui.toml")      { return "ScreenGui"; }
    if path_str.ends_with(".textlabel.toml")      { return "TextLabel"; }
    if path_str.ends_with(".textbutton.toml")     { return "TextButton"; }
    if path_str.ends_with(".frame.toml")          { return "Frame"; }
    if path_str.ends_with(".imagelabel.toml")     { return "ImageLabel"; }
    if path_str.ends_with(".imagebutton.toml")    { return "ImageButton"; }
    if path_str.ends_with(".scrollingframe.toml") { return "ScrollingFrame"; }
    if path_str.ends_with(".textbox.toml")        { return "TextBox"; }
    if path_str.ends_with(".viewportframe.toml")  { return "ViewportFrame"; }
    if path_str.ends_with(".videoframe.toml")     { return "VideoFrame"; }
    if path_str.ends_with(".documentframe.toml")  { return "DocumentFrame"; }
    if path_str.ends_with(".webframe.toml")       { return "WebFrame"; }
    if path_str.ends_with(".surfacegui.toml")     { return "SurfaceGui"; }
    if path_str.ends_with(".billboardgui.toml")   { return "BillboardGui"; }

    // Folder convention: `Name/_instance.toml` — peek the metadata
    // class_name and map it back to our `&'static str` universe.
    // Returning a `&'static str` forces an exhaustive match instead
    // of borrowing from the parsed doc; keeps the caller (which
    // currently threads this into other `&'static str` lookups) free
    // of lifetime plumbing.
    if path_str.ends_with("_instance.toml") {
        // DB-first: peek `[metadata] class_name` from the binary/tree
        // record (no disk). Fall back to a raw disk read only for a
        // legacy un-converted world (no active Fjall DB).
        let class_name: Option<String> = crate::space::active_db::peek_class_name(path).or_else(|| {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
                .and_then(|doc| {
                    eustress_common::class_schema::get_section_insensitive(&doc, "metadata")
                        .and_then(|m| {
                            eustress_common::class_schema::get_section_insensitive(m, "class_name")
                        })
                        .and_then(|c| c.as_str())
                        .map(|s| s.to_string())
                })
        });
        if let Some(cn) = class_name {
            {
                return match cn.as_str() {
                    "ScreenGui"      => "ScreenGui",
                    "TextLabel"      => "TextLabel",
                    "TextButton"     => "TextButton",
                    "Frame"          => "Frame",
                    "ImageLabel"     => "ImageLabel",
                    "ImageButton"    => "ImageButton",
                    "ScrollingFrame" => "ScrollingFrame",
                    "TextBox"        => "TextBox",
                    "ViewportFrame"  => "ViewportFrame",
                    "VideoFrame"     => "VideoFrame",
                    "DocumentFrame"  => "DocumentFrame",
                    "WebFrame"       => "WebFrame",
                    "SurfaceGui"     => "SurfaceGui",
                    "BillboardGui"   => "BillboardGui",
                    _                => "Frame",
                };
            }
        }
    }
    "Frame" // default
}

/// Map GUI type string to ClassName enum
pub fn gui_class_name_from_type(gui_type: &str) -> eustress_common::classes::ClassName {
    use eustress_common::classes::ClassName;
    match gui_type {
        "ScreenGui" => ClassName::ScreenGui,
        "Frame" => ClassName::Frame,
        "TextLabel" => ClassName::TextLabel,
        "TextButton" => ClassName::TextButton,
        "ImageLabel" => ClassName::ImageLabel,
        "ScrollingFrame" => ClassName::ScrollingFrame,
        "TextBox" => ClassName::TextBox,
        _ => ClassName::Frame,
    }
}

/// Extract display name from filename (everything before first dot)
pub fn gui_display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.splitn(2, '.').next())
        .unwrap_or("Unknown")
        .to_string()
}

// ============================================================================
// 3. spawn_gui_element — create Bevy UI entity with rendering components
// ============================================================================

/// Spawn a GUI element entity with proper Bevy UI rendering components.
///
/// Reads the TOML file, determines the element type from the file extension,
/// and spawns with Node, BackgroundColor, Text, etc. so it renders visually.
pub fn spawn_gui_element(
    commands: &mut Commands,
    path: &Path,
    gui_def: &GuiTomlFile,
) -> Entity {
    let gui_type = gui_class_from_extension(path);
    let display_name = if !gui_def.instance.name.is_empty() {
        gui_def.instance.name.clone()
    } else {
        gui_display_name(path)
    };

    // Parse the authored unit now so it can drive both the
    // Stage 3 dimensional conversion below (when `units_v1` is on)
    // AND the MeasureUnit component attached after spawn.
    let parsed_unit = gui_def.metadata.unit.as_deref()
        .and_then(eustress_common::units::Unit::from_symbol)
        .unwrap_or(eustress_common::units::ENGINE_NATIVE_UNIT);

    // Stage 3: convert BillboardGui's 3D dimensional fields from the
    // authored unit to meters. Pixel-space UDim2 fields (`position`,
    // `size`, `anchor_point`, `size_offset`) are 2D canvas coordinates
    // and are intentionally NOT converted. `extents_offset` /
    // `extents_offset_world_space` are in part-size multipliers (ratio,
    // not length) so they're also skipped.
    #[cfg(feature = "units_v1")]
    let gui_owned: GuiTomlProperties = {
        let mut g = gui_def.gui.clone();
        let to_unit = eustress_common::units::ENGINE_NATIVE_UNIT;
        if parsed_unit != to_unit {
            if let Some(v) = g.units_offset {
                g.units_offset = Some(eustress_common::units::convert_vec3_f32(v, parsed_unit, to_unit));
            }
            if let Some(v) = g.units_offset_world_space {
                g.units_offset_world_space = Some(eustress_common::units::convert_vec3_f32(v, parsed_unit, to_unit));
            }
            for f in [&mut g.max_distance, &mut g.distance_lower_limit,
                      &mut g.distance_upper_limit, &mut g.distance_step] {
                if let Some(v) = *f {
                    *f = Some(eustress_common::units::convert_f32(v, parsed_unit, to_unit));
                }
            }
        }
        g
    };
    #[cfg(feature = "units_v1")]
    let gui = &gui_owned;
    #[cfg(not(feature = "units_v1"))]
    let gui = &gui_def.gui;
    let class_name = match gui_type {
        "ScreenGui" => eustress_common::classes::ClassName::ScreenGui,
        "TextLabel" => eustress_common::classes::ClassName::TextLabel,
        "TextButton" => eustress_common::classes::ClassName::TextButton,
        "Frame" => eustress_common::classes::ClassName::Frame,
        "ImageLabel" => eustress_common::classes::ClassName::ImageLabel,
        "ImageButton" => eustress_common::classes::ClassName::ImageButton,
        "ScrollingFrame" => eustress_common::classes::ClassName::ScrollingFrame,
        "TextBox" => eustress_common::classes::ClassName::TextBox,
        "ViewportFrame" => eustress_common::classes::ClassName::ViewportFrame,
        _ => eustress_common::classes::ClassName::Frame,
    };

    let instance = eustress_common::classes::Instance {
        name: display_name.clone(),
        class_name,
        archivable: gui_def.metadata.archivable,
        id: 0,
        ai: false,
                uuid: String::new(),
    };

    let loaded_from = super::file_loader::LoadedFromFile {
        path: path.to_path_buf(),
        file_type: super::file_loader::FileType::GuiElement,
        service: "StarterGui".to_string(),
    };

    let entity = match gui_type {
        "ScreenGui" => spawn_screen_gui_element(commands, instance, loaded_from, &display_name),
        "TextLabel" => spawn_text_label_element(commands, instance, loaded_from, &display_name, gui, gui_def.text.as_ref()),
        "TextButton" => spawn_text_button_element(commands, instance, loaded_from, &display_name, gui, gui_def.text.as_ref()),
        "Frame" => spawn_frame_element(commands, instance, loaded_from, &display_name, gui),
        "ScrollingFrame" => spawn_scrolling_frame_element(commands, instance, loaded_from, &display_name, gui),
        "TextBox" => spawn_text_box_element(commands, instance, loaded_from, &display_name, gui, gui_def.text.as_ref()),
        // Media classes — render as placeholder frames with class label until
        // full media rendering is implemented (PDF, video, web, viewport)
        "ImageLabel" | "ImageButton" | "DocumentFrame" | "VideoFrame" | "WebFrame" | "ViewportFrame" => {
            // Use placeholder text showing the class type
            let placeholder_text = Some(GuiTomlText {
                text: format!("[{}]", gui_type),
                text_color: [0.5, 0.5, 0.5, 0.8],
                font_size: 12.0,
                text_x_alignment: "Center".to_string(),
                text_y_alignment: "Center".to_string(),
                ..Default::default()
            });
            spawn_frame_element_with_text(commands, instance, loaded_from, &display_name, gui, placeholder_text.as_ref())
        }
        _ => spawn_frame_element(commands, instance, loaded_from, &display_name, gui),
    };

    // CRITICAL: attach `InstanceFile` so the save_*_changes systems
    // (Changed<TextLabel>, Changed<BillboardGui>, Changed<Frame>, …)
    // can find the entity. Their queries require BOTH the class
    // component AND InstanceFile to write back to disk; without it,
    // every Properties-panel edit silently no-ops at save time —
    // ECS mutates, watcher never sees a TOML modification, the
    // edit dies on the next session reload. This bit users
    // 2026-05-12: "copy-paste loses all properties" was the visible
    // symptom but the root cause was the source TOML never being
    // updated in the first place.
    let display_name_for_inst = if !gui_def.instance.name.is_empty() {
        gui_def.instance.name.clone()
    } else {
        gui_display_name(path).to_string()
    };
    commands.entity(entity).insert(super::instance_loader::InstanceFile {
        toml_path: path.to_path_buf(),
        mesh_path: std::path::PathBuf::new(),
        name: display_name_for_inst,
    });

    // Attach MeasureUnit from the unit parsed above. Unknown symbols on
    // disk warn-once and fall back to engine-native meters rather than
    // failing the whole GUI load — symbol typos shouldn't black-hole a
    // ScreenGui tree.
    if let Some(sym) = gui_def.metadata.unit.as_deref() {
        if eustress_common::units::Unit::from_symbol(sym).is_none() {
            warn!("Unknown unit symbol {:?} in {} — using engine-native meters", sym, path.display());
        }
    }
    commands.entity(entity).insert(eustress_common::units::MeasureUnit(parsed_unit));

    info!("🪧 spawn_gui_element: {} attached InstanceFile → {}", gui_type, path.display());

    // Attach the ECS Tags component so MCP `get_tagged_entities` and other
    // engine systems can see GUI tags. Empty-tags case is a no-op — the
    // Tags component would just hold an empty Vec.
    if !gui_def.tags.is_empty() {
        commands.entity(entity).insert(
            eustress_common::attributes::Tags(gui_def.tags.clone()),
        );
    }

    entity
}

/// ScreenGui — fullscreen absolute overlay root container
fn spawn_screen_gui_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
) -> Entity {
    commands.spawn((
        instance,
        loaded_from,
        Name::new(display_name.to_string()),
        // Fullscreen Bevy UI root
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            ..default()
        },
        GlobalZIndex(100), // Above 3D scene, below Slint overlay
        // Transparent so it doesn't block the 3D view
        BackgroundColor(Color::NONE),
    )).id()
}

/// Frame — container with background color and optional border
/// Build a GuiElementDisplay from TOML properties + optional text
pub fn gui_display_from_props(gui: &GuiTomlProperties, text_props: Option<&GuiTomlText>, class_type: &str) -> GuiElementDisplay {
    let (text, text_color, font_size, text_align, text_y_align, font_weight, text_scaled) = if let Some(tp) = text_props {
        let weight_source = if !tp.font.is_empty() { &tp.font } else { &tp.font_family };
        (
            tp.text.clone(),
            tp.text_color,
            tp.font_size,
            tp.text_x_alignment.clone(),
            tp.text_y_alignment.clone(),
            font_weight_from_name(weight_source),
            tp.text_scaled,
        )
    } else {
        (String::new(), [1.0, 1.0, 1.0, 1.0], 14.0, "Center".to_string(), "Center".to_string(), 400, false)
    };

    // `gui.position` and `gui.size` are `UDim2`. We store the source
    // 4-tuple AND a best-effort resolved-pixel rect (Offset-only here,
    // since the parent's pixel extent isn't known at this site).
    // `collect_subtree` resolves Scale at render time using the parent
    // billboard's canvas dimensions.
    GuiElementDisplay {
        x: gui.position.x.offset,
        y: gui.position.y.offset,
        width: gui.size.x.offset.max(1.0),
        height: gui.size.y.offset.max(1.0),
        position_udim2: [
            gui.position.x.scale, gui.position.x.offset,
            gui.position.y.scale, gui.position.y.offset,
        ],
        size_udim2: [
            gui.size.x.scale, gui.size.x.offset,
            gui.size.y.scale, gui.size.y.offset,
        ],
        anchor_point: gui.anchor_point,
        z_order: gui.z_index,
        visible: gui.visible,
        clip_children: class_type.eq_ignore_ascii_case("scrollingframe"),
        scroll_x: 0.0,
        scroll_y: 0.0,
        bg_color: gui.background_color,
        border_size: gui.border_size,
        border_color: gui.border_color,
        corner_radius: gui.corner_radius,
        text,
        text_color,
        font_size,
        font_weight,
        text_align,
        text_y_align,
        text_stroke_color: [0.0, 0.0, 0.0, 0.0],
        text_scaled,
        image_path: String::new(),
        class_type: class_type.to_string(),
        mouse_filter: "stop".to_string(),
    }
}

/// Build a `BillboardGui` class component from parsed `[gui]` TOML
/// properties. This is the single mapping from the on-disk schema
/// (`GuiTomlProperties`) to the typed class — used by the cold-load path
/// (`file_loader.rs`), the hot-*create* path (`file_watcher.rs`), and the
/// hot-*modify* path. Centralising it here fixes the bug where an in-place
/// `_instance.toml` edit (e.g. changing `[gui] size`) updated the file but
/// not the live quad: the modify handler only re-inserted `Transform` +
/// material and never refreshed the `BillboardGui` class, so the quad kept
/// its spawn-time size. Re-inserting the class component this builds fires
/// `Changed<BillboardGui>` → `sync_billboard_class_to_marker` →
/// `sync_billboard_properties`, which rebuilds the quad scale, canvas, and
/// z-bias from the new values. Optional schema fields that are absent leave
/// the class default in place (older TOMLs that predate a property still
/// load cleanly).
pub fn billboard_class_from_props(
    g: &GuiTomlProperties,
) -> eustress_common::classes::BillboardGui {
    let mut bb = eustress_common::classes::BillboardGui::default();

    // Geometry
    bb.size = g.size;
    if let Some(v) = g.size_offset { bb.size_offset = v; }
    if let Some(v) = g.extents_offset { bb.extents_offset = v; }
    if let Some(v) = g.extents_offset_world_space { bb.extents_offset_world_space = v; }
    if let Some(v) = g.units_offset { bb.units_offset = v; }
    if let Some(v) = g.units_offset_world_space { bb.units_offset_world_space = v; }

    // Distance
    if let Some(v) = g.max_distance { bb.max_distance = v; }
    if let Some(v) = g.distance_lower_limit { bb.distance_lower_limit = v; }
    if let Some(v) = g.distance_upper_limit { bb.distance_upper_limit = v; }
    if let Some(v) = g.distance_step { bb.distance_step = v; }

    // Behaviour flags
    if let Some(v) = g.active { bb.active = v; }
    if let Some(v) = g.enabled { bb.enabled = v; }
    if let Some(v) = g.always_on_top { bb.always_on_top = v; }
    if let Some(v) = g.clips_descendants { bb.clips_descendants = v; }
    if let Some(v) = g.reset_on_spawn { bb.reset_on_spawn = v; }
    if let Some(v) = g.stiffness_by_distance { bb.stiffness_by_distance = v; }

    // Appearance
    if let Some(v) = g.brightness { bb.brightness = v; }
    if let Some(v) = g.light_influence { bb.light_influence = v; }

    // Sorting
    if let Some(ref s) = g.z_index_behavior {
        bb.z_index_behavior = match s.as_str() {
            "Global" => eustress_common::classes::ZIndexBehavior::Global,
            _ => eustress_common::classes::ZIndexBehavior::Sibling,
        };
    }
    bb.z_index = g.z_index;

    bb
}

fn spawn_frame_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
) -> Entity {
    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        // Minimal Node for Bevy hierarchy — actual rendering is done by Slint overlay
        Node { display: Display::None, ..default() },
        gui_display_from_props(gui, None, "Frame"),
    )).id();
    commands.entity(entity).insert(loaded_from);
    entity
}

/// Frame with optional text overlay (used for media class placeholders)
fn spawn_frame_element_with_text(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
    text_props: Option<&GuiTomlText>,
) -> Entity {
    let class = instance.class_name;
    // Roblox-parity: class_type stores the PascalCase class identifier
    // (TextLabel, TextButton, ImageLabel, …). Debug-format already gives
    // PascalCase since `ClassName` derives `Debug` on its variants, so no
    // post-processing is needed.
    let class_str = format!("{:?}", class);
    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        Node { display: Display::None, ..default() },
        gui_display_from_props(gui, text_props, &class_str),
    )).id();
    commands.entity(entity).insert(loaded_from);
    entity
}

/// ScrollingFrame — container with clip and scroll
fn spawn_scrolling_frame_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
) -> Entity {
    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        Node { display: Display::None, ..default() },
        gui_display_from_props(gui, None, "ScrollingFrame"),
    )).id();
    commands.entity(entity).insert(loaded_from);
    entity
}

/// TextLabel — non-interactive text display.
///
/// Mirrors the component set produced by `spawn::spawn_text_label`
/// (the Insert-menu path) so reloaded TextLabels behave identically
/// to freshly-inserted ones. The earlier version was missing the
/// `TextLabel` ECS class component entirely AND inserted a stray
/// `Node{display:None}` that confused Bevy's UI layout when the
/// parent was a 3D BillboardGui (which is the common case for
/// MindSpace-attached labels). Both fixes here:
///
///   1. Insert the typed `TextLabel` component, populated from the
///      `[text]` and `[gui]` TOML sections so Properties panel and
///      anything that queries by class component see real data.
///   2. Drop the `Node` entirely — the billboard renderer reads
///      `GuiElementDisplay` directly. ScreenGui-parented TextLabels
///      get their Bevy UI Node added later by the runtime UI system
///      (`runtime_ui::sync_screen_gui_layout`), so a hardcoded Node
///      here just stomps on that.
fn spawn_text_label_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
    text_props: Option<&GuiTomlText>,
) -> Entity {
    use eustress_common::classes::{TextLabel, Font, TextXAlignment, TextYAlignment};

    let (text_value, text_color3, font_size, x_align, y_align, font) = match text_props {
        Some(t) => {
            let weight_source = if !t.font.is_empty() { &t.font } else { &t.font_family };
            let font_variant = match weight_source.as_str() {
                "GothamBold" | "Bold" => Font::GothamBold,
                "GothamLight" | "Light" => Font::GothamLight,
                _ => Font::default(),
            };
            let xa = match t.text_x_alignment.to_ascii_lowercase().as_str() {
                "left"   => TextXAlignment::Left,
                "right"  => TextXAlignment::Right,
                _        => TextXAlignment::Center,
            };
            let ya = match t.text_y_alignment.to_ascii_lowercase().as_str() {
                "top"    => TextYAlignment::Top,
                "bottom" => TextYAlignment::Bottom,
                _        => TextYAlignment::Center,
            };
            (
                t.text.clone(),
                [t.text_color[0], t.text_color[1], t.text_color[2]],
                t.font_size,
                xa, ya,
                font_variant,
            )
        }
        None => (
            String::new(),
            [1.0, 1.0, 1.0],
            14.0,
            TextXAlignment::Center,
            TextYAlignment::Center,
            Font::default(),
        ),
    };

    let mut label = TextLabel::default();
    label.text = text_value;
    label.text_color3 = text_color3;
    label.text_transparency = text_props.map(|t| 1.0 - t.text_color[3]).unwrap_or(0.0);
    label.font = font;
    label.font_size = font_size;
    label.text_x_alignment = x_align;
    label.text_y_alignment = y_align;
    label.background_color3 = [gui.background_color[0], gui.background_color[1], gui.background_color[2]];
    label.background_transparency = 1.0 - gui.background_color[3];
    label.border_color3 = [gui.border_color[0], gui.border_color[1], gui.border_color[2]];
    label.border_size_pixel = gui.border_size as i32;
    // Roblox-parity Position/Size as UDim2 — single source of truth on disk.
    label.size = gui.size;
    label.position = gui.position;
    label.visible = gui.visible;
    label.z_index = gui.z_index;

    let entity = commands.spawn((
        instance,
        label,
        Name::new(display_name.to_string()),
        TextLabelMarker,
        gui_display_from_props(gui, text_props, "TextLabel"),
    )).id();
    commands.entity(entity).insert(loaded_from);
    entity
}

/// TextButton — clickable text with background
fn spawn_text_button_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
    text_props: Option<&GuiTomlText>,
) -> Entity {
    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        Node { display: Display::None, ..default() },
        gui_display_from_props(gui, text_props, "TextButton"),
    )).id();
    commands.entity(entity).insert(loaded_from);
    entity
}

/// TextBox — text input field
fn spawn_text_box_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
    text_props: Option<&GuiTomlText>,
) -> Entity {
    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        TextBoxMarker,
        Node { display: Display::None, ..default() },
        gui_display_from_props(gui, text_props, "TextBox"),
    )).id();
    commands.entity(entity).insert(loaded_from);
    entity
}

// ============================================================================
// Helpers
// ============================================================================

/// Convert a [f32; 4] RGBA array to a Bevy Color
fn to_color(rgba: &[f32; 4]) -> Color {
    Color::srgba(rgba[0], rgba[1], rgba[2], rgba[3])
}

/// Extract text properties from optional [text] section, with defaults
fn resolve_text_props(text_props: Option<&GuiTomlText>) -> (String, Color, f32, JustifyContent, AlignItems) {
    match text_props {
        Some(t) => {
            let text_color = to_color(&t.text_color);
            let justify = match t.text_x_alignment.to_ascii_lowercase().as_str() {
                "left" => JustifyContent::FlexStart,
                "center" => JustifyContent::Center,
                "right" => JustifyContent::FlexEnd,
                _ => JustifyContent::FlexStart,
            };
            let align = match t.text_y_alignment.to_ascii_lowercase().as_str() {
                "top" => AlignItems::FlexStart,
                "center" => AlignItems::Center,
                "bottom" => AlignItems::FlexEnd,
                _ => AlignItems::Center,
            };
            (t.text.clone(), text_color, t.font_size, justify, align)
        }
        None => (
            String::new(),
            Color::WHITE,
            14.0,
            JustifyContent::FlexStart,
            AlignItems::Center,
        ),
    }
}

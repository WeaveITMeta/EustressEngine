//! # GUI Element Loader — Parse .textlabel.toml, .frame.toml, etc. into Bevy UI entities
//!
//! ## Table of Contents
//!
//! 1. The GUI TOML schema and its builders, re-exported from
//!    `eustress_common::gui::gui_record`, which a Player's reader shares
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
use std::path::Path;

use crate::spawn::{TextLabelMarker, TextBoxMarker};

pub use eustress_common::gui::billboard_renderer::{BillboardGuiMarker, SurfaceGuiMarker};

// Re-export from common crate so engine code can use it
pub use eustress_common::gui::billboard_renderer::GuiElementDisplay;

// ============================================================================
// 1. The file's structs and builders, shared with a Player's reader
// ============================================================================

// The GUI file's schema and every builder from it (what a GUI object draws
// as, a BillboardGui's and a SurfaceGui's class) live in
// `eustress_common::gui::gui_record`, which a Player's reader uses too, so
// Studio and a Player never read one file two ways.
pub use eustress_common::gui::gui_record::{
    billboard_class_from_def, billboard_class_from_props, billboard_lengths_to_native, font_weight_from_name,
    gui_display_for_class, gui_display_from_props, gui_image_source, screen_gui_display, surface_class_from_props,
    GuiTomlFile, GuiTomlImage, GuiTomlInstance, GuiTomlMetadata, GuiTomlProperties, GuiTomlScrolling, GuiTomlText,
    GuiTomlViewport,
};
use eustress_common::gui::gui_record::{
    default_bg_color, default_border_color, default_font_size, default_size_udim2, default_text_color,
};

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
            // Compile scaffold: struct serde-defaults (opaque text, no stroke)
            // so this load path builds. Co-agent's text-stroke feature wires
            // the real values from the loaded instance.
            text_transparency: 0.0,
            text_stroke_color: [0.0, 0.0, 0.0, 1.0],
            text_stroke_transparency: 1.0,
            text_wrapped: true,
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
        image: None,
        scrolling: None,
        viewport: None,
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
    eustress_common::gui::gui_record::gui_definition_from_str(content)
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
    if let Some(class) = compound_extension_class(path) {
        return class;
    }
    let path_str = path.to_string_lossy();

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
            return known_gui_class(&cn).unwrap_or("Frame");
        }
    }
    "Frame" // default
}

/// The class a legacy flat GUI file's compound extension names
/// (`Name.textlabel.toml`), if it has one.
fn compound_extension_class(path: &Path) -> Option<&'static str> {
    let path_str = path.to_string_lossy();
    const EXTENSIONS: [(&str, &str); 14] = [
        (".screengui.toml", "ScreenGui"),
        (".textlabel.toml", "TextLabel"),
        (".textbutton.toml", "TextButton"),
        (".frame.toml", "Frame"),
        (".imagelabel.toml", "ImageLabel"),
        (".imagebutton.toml", "ImageButton"),
        (".scrollingframe.toml", "ScrollingFrame"),
        (".textbox.toml", "TextBox"),
        (".viewportframe.toml", "ViewportFrame"),
        (".videoframe.toml", "VideoFrame"),
        (".documentframe.toml", "DocumentFrame"),
        (".webframe.toml", "WebFrame"),
        (".surfacegui.toml", "SurfaceGui"),
        (".billboardgui.toml", "BillboardGui"),
    ];
    EXTENSIONS.iter().find(|(ext, _)| path_str.ends_with(ext)).map(|(_, class)| *class)
}

/// A GUI class name as the loader's `&'static str`, or `None` for a name
/// that is no GUI class.
fn known_gui_class(name: &str) -> Option<&'static str> {
    Some(match name {
        "ScreenGui" => "ScreenGui",
        "TextLabel" => "TextLabel",
        "TextButton" => "TextButton",
        "Frame" => "Frame",
        "ImageLabel" => "ImageLabel",
        "ImageButton" => "ImageButton",
        "ScrollingFrame" => "ScrollingFrame",
        "TextBox" => "TextBox",
        "ViewportFrame" => "ViewportFrame",
        "VideoFrame" => "VideoFrame",
        "DocumentFrame" => "DocumentFrame",
        "WebFrame" => "WebFrame",
        "SurfaceGui" => "SurfaceGui",
        "BillboardGui" => "BillboardGui",
        _ => return None,
    })
}

/// The GUI class a parsed file loads as: a flat file's compound extension,
/// else the class its `[metadata]` names, else what
/// [`gui_class_from_extension`] finds. Reading the parsed metadata first
/// spares a folder element a second read of its own file.
pub fn gui_class_of(path: &Path, def: &GuiTomlFile) -> &'static str {
    compound_extension_class(path)
        .or_else(|| known_gui_class(&def.metadata.class_name))
        .unwrap_or_else(|| gui_class_from_extension(path))
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

/// Extract display name from filename (everything before first dot). A
/// folder-form element's `_instance.toml` takes its folder's name instead.
pub fn gui_display_name(path: &Path) -> String {
    if path.file_name().is_some_and(|n| n == "_instance.toml") {
        if let Some(folder) = path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) {
            return folder.to_string();
        }
    }
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
    let gui_type = gui_class_of(path, gui_def);
    let display_name = if !gui_def.instance.name.is_empty() {
        gui_def.instance.name.clone()
    } else {
        gui_display_name(path)
    };

    // Parse the authored unit now so it can drive the MeasureUnit
    // component attached after spawn.
    let parsed_unit = gui_def.metadata.unit.as_deref()
        .and_then(eustress_common::units::Unit::from_symbol)
        .unwrap_or(eustress_common::units::ENGINE_NATIVE_UNIT);

    let gui_owned: GuiTomlProperties = {
        let mut g = gui_def.gui.clone();
        g.size = g.resolved_size();
        billboard_lengths_to_native(&mut g, gui_def.metadata.unit.as_deref());
        g
    };
    let gui = &gui_owned;
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

    // What it draws as: the one builder a Player's reader uses too
    // (`gui_display_for_class`). The remaining media classes (PDF, video,
    // web, viewport) draw their class name until they render.
    let display = gui_display_for_class(gui_type, gui_def);
    let entity = match gui_type {
        "ScreenGui" => spawn_screen_gui_element(commands, instance, loaded_from, &display_name, display),
        "TextLabel" => spawn_text_label_element(commands, instance, loaded_from, &display_name, gui, gui_def.text.as_ref(), display),
        "TextButton" => spawn_text_button_element(commands, instance, loaded_from, &display_name, display),
        "ScrollingFrame" => spawn_scrolling_frame_element(commands, instance, loaded_from, &display_name, gui, gui_def, display),
        "TextBox" => spawn_text_box_element(commands, instance, loaded_from, &display_name, display),
        // ImageLabel / ImageButton: render the actual image (source in
        // [asset].path, resolved against the folder) via GuiElementDisplay.image_path.
        "ImageLabel" | "ImageButton" => {
            spawn_image_element(commands, instance, loaded_from, &display_name, gui, gui_def, path, display)
        }
        _ => spawn_frame_element(commands, instance, loaded_from, &display_name, display),
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

    // debug!, not info!: this fires once PER GUI ELEMENT during load — a
    // 57K-TextLabel world emitted 57K console lines, and Windows console IO
    // alone stretched load by minutes.
    debug!("🪧 spawn_gui_element: {} attached InstanceFile → {}", gui_type, path.display());

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

/// ScreenGui — fullscreen absolute overlay root container.
///
/// The fullscreen Bevy UI root is attached ONLY when the ScreenGui is a
/// genuine screen overlay, i.e. it lives under the `StarterGui` service.
/// Roblox semantics: a ScreenGui renders only from `PlayerGui` (which
/// `StarterGui` is cloned into at runtime). One parented anywhere else — a
/// vehicle's `DriveSeat`, a Tool, `ReplicatedStorage` — is an INERT
/// template that draws nothing until a script clones it into PlayerGui.
///
/// Imported places are full of those: Mountain Ascension carries 11
/// ScreenGuis, every one of them a car-dashboard UI under
/// `Workspace/<vehicle>/DriveSeat/`. Attaching a fullscreen absolute `Node`
/// to each meant parenting a UI node to a 3D part entity (which has a
/// `Transform` but no `Node`), leaving origin-anchored overlay roots stacked
/// on the editor's UI layer — the "frame outlines / UI boxes at the top-left
/// origin" import artifact.
///
/// This is the same defect (and the same fix) already applied to
/// `spawn_text_label_element` — see its doc comment: a stray `Node` "confused
/// Bevy's UI layout when the parent was a 3D BillboardGui".
///
/// Non-overlay ScreenGuis still spawn as real entities (Explorer, Properties,
/// scripts and `Clone()` all see them) — they just don't get a UI root.
fn spawn_screen_gui_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    display: GuiElementDisplay,
) -> Entity {
    // Read the service before `loaded_from` moves into the spawn bundle.
    let is_screen_overlay = loaded_from.service == "StarterGui";

    let mut ec = commands.spawn((
        instance,
        loaded_from,
        Name::new(display_name.to_string()),
    ));
    if is_screen_overlay {
        ec.insert((
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
        ));
    }
    // The overlay draws a GUI element only under an entity carrying a
    // "screengui" GuiElementDisplay, and hides the subtree when it is not
    // visible (Enabled = false). The cold-load path attaches this in
    // file_loader.rs; without it here, a ScreenGui made by Insert, the file
    // watcher or the UI Builder painted nothing until the Space reopened.
    ec.insert(display);
    ec.id()
}

fn spawn_frame_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    display: GuiElementDisplay,
) -> Entity {
    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        // NO bevy_ui Node here — rendering is the Slint overlay via
        // GuiElementDisplay. PERF: every entity carrying `Node` is walked by
        // ui_layout_system / update_clipping / ui_stack EVERY frame even at
        // Display::None (bevy_ui 0.18 only skips taffy compute, not the
        // per-entity sync + recursion) — ~8K imported GUI elements cost
        // ~9 ms/frame in the editor.
        display,
    )).id();
    commands.entity(entity).insert(loaded_from);
    entity
}

/// ScrollingFrame — container with clip and scroll. Attaches the typed
/// `ScrollingFrame` component from the importer-written `[scrolling]`
/// section and seeds the display's initial scroll offset from
/// `canvas_position` so clip + starting scroll are honored on load.
/// (Live scroll input + scrollbar visuals remain a deferred FEATURE.)
fn spawn_scrolling_frame_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
    gui_def: &GuiTomlFile,
    mut display: GuiElementDisplay,
) -> Entity {

    // Build the typed component from [scrolling]; default-fill the rest.
    let mut sf = eustress_common::classes::ScrollingFrame::default();
    if let Some(s) = gui_def.scrolling.as_ref() {
        sf.scrolling_enabled = s.scrolling_enabled;
        if s.scroll_bar_thickness != 0 {
            sf.scroll_bar_thickness = s.scroll_bar_thickness;
        }
        if s.canvas_size.len() == 2 {
            sf.canvas_size = [s.canvas_size[0], s.canvas_size[1]];
        }
        if s.canvas_position.len() == 2 {
            sf.canvas_position = [s.canvas_position[0], s.canvas_position[1]];
        }
        sf.scroll_bar_image_color3 = [
            s.scroll_bar_image_color[0],
            s.scroll_bar_image_color[1],
            s.scroll_bar_image_color[2],
        ];
        sf.scroll_bar_image_transparency = s.scroll_bar_image_transparency;
        if !s.top_image.is_empty() { sf.top_image = s.top_image.clone(); }
        if !s.mid_image.is_empty() { sf.mid_image = s.mid_image.clone(); }
        if !s.bottom_image.is_empty() { sf.bottom_image = s.bottom_image.clone(); }
    }
    sf.visible = gui.visible;
    sf.z_index = gui.z_index;

    // Seed the renderer's initial scroll offset from canvas_position.
    display.scroll_x = sf.canvas_position[0];
    display.scroll_y = sf.canvas_position[1];

    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        // No bevy_ui Node — see spawn_frame_element PERF note.
        display,
        sf,
    )).id();
    commands.entity(entity).insert(loaded_from);
    entity
}

/// ImageLabel / ImageButton — resolves the image SOURCE from
/// `[asset].path` (importer-written) with a `[image].image` fallback,
/// joins it against the GUI folder, sets `GuiElementDisplay.image_path`
/// (the Slint overlay renderer reads this), and attaches the typed
/// ImageLabel / ImageButton component so the Properties panel sees real
/// data. (ImageButton hover/pressed state-swap is a deferred FEATURE.)
fn spawn_image_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
    gui_def: &GuiTomlFile,
    path: &Path,
    mut display: GuiElementDisplay,
) -> Entity {
    let is_button = matches!(instance.class_name, eustress_common::classes::ClassName::ImageButton);

    // Image SOURCE: [asset].path first (importer), then [image].image.
    let rel_image: Option<String> = gui_image_source(gui_def);

    // Resolve the relative path against the GUI folder for slint load.
    if let Some(rel) = rel_image.as_ref() {
        if !rel.is_empty() {
            let base = path.parent().unwrap_or(Path::new("."));
            let abs = base.join(rel);
            display.image_path = abs.to_string_lossy().to_string();
        }
    }

    let img = gui_def.image.as_ref();
    let image_color3 = img
        .map(|i| [i.image_color[0], i.image_color[1], i.image_color[2]])
        .unwrap_or([1.0, 1.0, 1.0]);
    let image_transparency = img.map(|i| i.image_transparency).unwrap_or(0.0);
    let resolved_image = rel_image.unwrap_or_default();

    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        // No bevy_ui Node — see spawn_frame_element PERF note.
        display,
    )).id();

    if is_button {
        let hover = img.map(|i| i.hover_image.clone()).unwrap_or_default();
        let pressed = img.map(|i| i.pressed_image.clone()).unwrap_or_default();
        commands.entity(entity).insert(eustress_common::classes::ImageButton {
            image: resolved_image,
            hover_image: hover,
            pressed_image: pressed,
            image_color3,
            image_transparency,
            visible: gui.visible,
            z_index: gui.z_index,
            ..Default::default()
        });
    } else {
        commands.entity(entity).insert(eustress_common::classes::ImageLabel {
            image: resolved_image,
            image_color3,
            image_transparency,
            visible: gui.visible,
            z_index: gui.z_index,
            ..Default::default()
        });
    }
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
///      `GuiElementDisplay` directly, and ScreenGui-parented TextLabels
///      render through the Slint overlay from the same component, so
///      neither needs a Bevy UI Node.
fn spawn_text_label_element(
    commands: &mut Commands,
    instance: eustress_common::classes::Instance,
    loaded_from: super::file_loader::LoadedFromFile,
    display_name: &str,
    gui: &GuiTomlProperties,
    text_props: Option<&GuiTomlText>,
    display: GuiElementDisplay,
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
    label.text_transparency = text_props.map(|t| t.text_transparency).unwrap_or(0.0);
    label.font = font;
    label.font_size = font_size;
    label.text_x_alignment = x_align;
    label.text_y_alignment = y_align;
    label.background_color3 = [gui.background_color[0], gui.background_color[1], gui.background_color[2]];
    // `resolved_background_transparency()` — see its doc comment. The
    // billboard sync systems propagate this class value straight into
    // `GuiElementDisplay.bg_color`, so getting it right here is what
    // actually reaches the renderer.
    label.background_transparency = gui.resolved_background_transparency();
    label.border_color3 = [gui.border_color[0], gui.border_color[1], gui.border_color[2]];
    label.border_size_pixel = gui.border_size as i32;
    // Roblox-parity Position/Size as UDim2 — single source of truth on disk.
    label.size = gui.size;
    label.position = gui.position;
    // AnchorPoint + TextScaled round-trip through `apply_text_label_to_toml`
    // (save-back writes `class.anchor_point` / `class.text_scaled`). The
    // loader previously left BOTH at `TextLabel::default()`, so cold-load
    // showed AnchorPoint (0,0) in the Properties panel even when the TOML
    // said (0, 0.5) — and the very next save-back then CLOBBERED the disk
    // back to the defaults (anchor (0,0), text_scaled=false), un-centering
    // every banner. Load them here so the class mirrors disk and the
    // round-trip is lossless. (The billboard renderer reads these from
    // `GuiElementDisplay` on a separate path, which is why the 3D banner
    // looked centered while the class component / panel did not.)
    label.anchor_point = gui.anchor_point;
    label.text_scaled = text_props.map(|t| t.text_scaled).unwrap_or(false);
    // Wrapping and the stroke, which `sync_textlabel_to_display` copies from
    // the class onto the display: the class must hold the file's values, or
    // Studio draws what a Player reading the same file does not.
    label.text_wrapped = text_props.map(|t| t.text_wrapped).unwrap_or(true);
    if let Some(t) = text_props {
        label.text_stroke_color3 = [t.text_stroke_color[0], t.text_stroke_color[1], t.text_stroke_color[2]];
        label.text_stroke_transparency = t.text_stroke_transparency;
    }
    label.active = gui.active.unwrap_or(false);
    label.clips_descendants = gui.clips_descendants.unwrap_or(false);
    label.visible = gui.visible;
    label.z_index = gui.z_index;

    let entity = commands.spawn((
        instance,
        label,
        Name::new(display_name.to_string()),
        TextLabelMarker,
        display,
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
    display: GuiElementDisplay,
) -> Entity {
    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        // No bevy_ui Node — see spawn_frame_element PERF note.
        display,
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
    display: GuiElementDisplay,
) -> Entity {
    let entity = commands.spawn((
        instance,
        Name::new(display_name.to_string()),
        TextBoxMarker,
        // No bevy_ui Node — see spawn_frame_element PERF note.
        display,
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

// ============================================================================
// Tests — GUI color length-leniency (Roblox Color3 RGB → Eustress RGBA)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact regression: an imported GUI leaf whose `[gui]` /
    /// `[text]` colors are 3-element 0-255 integer arrays (Roblox
    /// `Color3`, the form the class-schema templates author). Before the
    /// lenient deserialize this failed with *"invalid length 3, expected
    /// an array of length 4"* — the 8038 Vehicle Simulator warnings.
    #[test]
    fn loads_three_element_integer_colors() {
        let toml = r#"
            [metadata]
            class_name = "TextLabel"

            [gui]
            background_color = [255, 255, 255]
            border_color = [27, 27, 27]

            [text]
            text = "Label"
            text_color = [0, 0, 0]
        "#;
        let def = load_gui_definition_from_str(toml).expect("3-element RGB must load");
        // 0-255 integers normalise to 0.0-1.0; alpha padded opaque.
        assert_eq!(def.gui.background_color, [1.0, 1.0, 1.0, 1.0]);
        let bc = def.gui.border_color;
        assert!((bc[0] - 27.0_f32 / 255.0).abs() < 1e-6 && bc[3] == 1.0);
        assert_eq!(def.text.unwrap().text_color, [0.0, 0.0, 0.0, 1.0]);
    }

    /// 3-element 0.0-1.0 float colors also pad alpha and stay verbatim.
    #[test]
    fn loads_three_element_float_colors() {
        let toml = r#"
            [gui]
            background_color = [0.5, 0.25, 0.75]
        "#;
        let def = load_gui_definition_from_str(toml).unwrap();
        assert_eq!(def.gui.background_color, [0.5, 0.25, 0.75, 1.0]);
    }

    /// Existing 4-element 0.0-1.0 float RGBA data deserialises
    /// byte-for-byte identically to the old strict `[f32; 4]` path —
    /// no normalisation, alpha preserved.
    #[test]
    fn four_element_float_rgba_is_identical() {
        let toml = r#"
            [gui]
            background_color = [0.2, 0.2, 0.2, 0.8]
            border_color = [0.5, 0.5, 0.5, 1.0]

            [text]
            text_color = [1.0, 1.0, 1.0, 0.5]
        "#;
        let def = load_gui_definition_from_str(toml).unwrap();
        assert_eq!(def.gui.background_color, [0.2, 0.2, 0.2, 0.8]);
        assert_eq!(def.gui.border_color, [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(def.text.unwrap().text_color, [1.0, 1.0, 1.0, 0.5]);
    }

    /// Absent color keys fall back to the typed defaults (the `default
    /// = …` arm runs, NOT the lenient deserialiser).
    #[test]
    fn missing_colors_use_defaults() {
        let def = load_gui_definition_from_str("[gui]\nvisible = true\n").unwrap();
        assert_eq!(def.gui.background_color, default_bg_color());
        assert_eq!(def.gui.border_color, default_border_color());
    }

    /// A genuinely malformed color (length 2) still errors clearly.
    #[test]
    fn rejects_two_element_color() {
        let toml = "[gui]\nbackground_color = [1.0, 0.0]\n";
        assert!(load_gui_definition_from_str(toml).is_err());
    }

    /// Whole-number normalised floats (e.g. pure red `[1.0, 0.0, 0.0,
    /// 1.0]`) are serialised by the `f32` field as TOML floats, never
    /// bare integers, so a reload reads them verbatim and never
    /// re-divides by 255. This guards the integer-vs-float heuristic
    /// against a save→load round-trip after a template import.
    #[test]
    fn whole_number_float_color_not_treated_as_255() {
        // Simulate the on-disk shape after a [255,0,0] import is saved
        // back: the `f32` Serialize writes `1.0`, not `1`.
        let toml = "[gui]\nbackground_color = [1.0, 0.0, 0.0, 1.0]\n";
        let def = load_gui_definition_from_str(toml).unwrap();
        assert_eq!(def.gui.background_color, [1.0, 0.0, 0.0, 1.0]);
    }
}

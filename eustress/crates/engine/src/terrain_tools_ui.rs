//! The terrain tools' Slint surfaces (see `docs/design/TERRAIN_TOOLS_UX.md`,
//! section 7): the `TerrainToolsUi` global that the tool bar, the cursor
//! readout and the ribbon's Terrain tab read, kept in step with
//! [`TerrainBrush`], and its callbacks answered.
//!
//! Also here: the brush settings and presets, saved between sessions in the
//! per-user config folder next to the keybindings; the material thumbnails
//! the picker shows, decoded from each slot's albedo texture on a background
//! thread; and [`tool_bar_contains`], which the Studio's focus test uses so
//! a click on the bar never reaches the ground behind it.
//!
//! Lengths cross into Slint in the status-bar display unit and come back in
//! it; the brush keeps metres.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};

use eustress_common::terrain::{
    BrushHardness, BrushPivot, BrushShape, MirrorAxes, PaintMode, RegionMode, SeaLevelMode, TerrainBrush, TerrainBrushHover,
    TerrainMaterialSlots, TerrainMode, TerrainTool, BRUSH_SIZE_MAX, BRUSH_SIZE_MIN, FIRST_CUSTOM_MATERIAL_SLOT,
    SNAP_STEPS,
};
use eustress_common::units::{convert_f32, DisplayUnit, Unit};

use crate::keybindings::{Action, KeyBindings};
use crate::terrain_cursor::{family_rgb, TerrainCursorReadout};
use crate::terrain_region::{RegionOp, RegionTool, RegionTransform, REGION_SCALE_RANGE};
use crate::terrain_sea_level::SeaLevelTool;

/// Sea Level's buttons in the bar: the labels its `action` callback sends
/// back, and the mode each applies.
const SEA_LEVEL_ACTIONS: [(&str, SeaLevelMode); 2] = [("Fill", SeaLevelMode::Fill), ("Evaporate", SeaLevelMode::Evaporate)];
/// Region's buttons, per mode.
const REGION_SELECT_ACTIONS: [(&str, RegionOp); 5] = [
    ("Copy", RegionOp::Copy),
    ("Cut", RegionOp::Cut),
    ("Paste", RegionOp::Paste),
    ("Duplicate", RegionOp::Duplicate),
    ("Delete", RegionOp::Delete),
];
const REGION_TRANSFORM_ACTIONS: [(&str, RegionOp); 3] =
    [("Rotate", RegionOp::Rotate), ("Apply", RegionOp::ApplyTransform), ("Cancel", RegionOp::Cancel)];
const REGION_FILL_ACTIONS: [(&str, RegionOp); 2] = [("Fill", RegionOp::Fill), ("Replace", RegionOp::Replace)];

/// The Sea Level mode a bar button names.
fn sea_level_action(name: &str) -> Option<SeaLevelMode> {
    SEA_LEVEL_ACTIONS.iter().find(|(label, _)| *label == name).map(|(_, mode)| *mode)
}

/// The Region edit a bar button names, in any mode.
fn region_action(name: &str) -> Option<RegionOp> {
    REGION_SELECT_ACTIONS
        .iter()
        .chain(&REGION_TRANSFORM_ACTIONS)
        .chain(&REGION_FILL_ACTIONS)
        .find(|(label, _)| *label == name)
        .map(|(_, op)| *op)
}
use crate::ui::slint_ui::{StudioWindow, TerrainMaterialTile, TerrainToolsUi};
use crate::ui::SetTerrainBrushEvent;

/// Connects the terrain tools to their Slint surfaces.
pub struct TerrainToolsUiPlugin;

impl Plugin for TerrainToolsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerrainToolsInbox>()
            .init_resource::<TerrainBrushPresets>()
            .init_resource::<TerrainThumbnails>()
            .init_resource::<BrushSettingsSave>()
            .add_systems(Startup, load_brush_settings)
            .add_systems(
                Update,
                (
                    register_callbacks,
                    apply_tool_bar_messages,
                    request_thumbnails,
                    sync_terrain_tools_to_slint.after(crate::terrain_cursor::update_terrain_cursor),
                    save_brush_settings,
                )
                    .chain(),
            );
    }
}

// ============================================================================
// Settings and presets on disk
// ============================================================================

/// A named brush: every setting but the active tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrushPreset {
    pub name: String,
    pub brush: TerrainBrush,
}

/// The saved brush presets, in the order they were first saved.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TerrainBrushPresets {
    pub presets: Vec<BrushPreset>,
    /// The preset last applied or saved, shown as current in the bar.
    pub active: Option<String>,
}

/// What `terrain_brush.ron` holds.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct BrushSettingsFile {
    brush: TerrainBrush,
    presets: TerrainBrushPresets,
}

/// `~/.eustress_engine/terrain_brush.ron`, beside the keybindings.
fn settings_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".eustress_engine").join("terrain_brush.ron"))
}

/// Seconds a change waits before it is written, so a scrub or a drag of
/// the size gesture writes once at the end.
const SAVE_DELAY_SECS: f64 = 1.0;

/// When the brush or the presets last changed and were not yet written.
#[derive(Resource, Default)]
struct BrushSettingsSave {
    pending_since: Option<f64>,
}

/// Read the saved brush and presets at startup, when there are any. The
/// Studio always starts with the tools off; the active tool and plane lock
/// are kept, the stroke state is not.
fn load_brush_settings(mut brush: ResMut<TerrainBrush>, mut presets: ResMut<TerrainBrushPresets>) {
    let Some(path) = settings_path() else { return };
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    match ron::from_str::<BrushSettingsFile>(&text) {
        Ok(file) => {
            *brush = file.brush;
            *presets = file.presets;
        }
        Err(error) => warn!("terrain brush settings at {} unreadable, using defaults: {error}", path.display()),
    }
}

/// Write the brush and presets a moment after they stop changing.
fn save_brush_settings(
    time: Res<Time<Real>>,
    brush: Res<TerrainBrush>,
    presets: Res<TerrainBrushPresets>,
    mut save: ResMut<BrushSettingsSave>,
) {
    let now = time.elapsed_secs_f64();
    if (brush.is_changed() && !brush.is_added()) || (presets.is_changed() && !presets.is_added()) {
        save.pending_since = Some(now);
    }
    let Some(since) = save.pending_since else { return };
    if now - since < SAVE_DELAY_SECS {
        return;
    }
    save.pending_since = None;
    let Some(path) = settings_path() else { return };
    let file = BrushSettingsFile { brush: brush.clone(), presets: presets.clone() };
    let written = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
        .map_err(|error| error.to_string())
        .and_then(|text| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
            }
            std::fs::write(&path, text).map_err(|error| error.to_string())
        });
    if let Err(error) = written {
        warn!("could not save terrain brush settings to {}: {error}", path.display());
    }
}

// ============================================================================
// Callbacks
// ============================================================================

/// What the bar's callbacks queue for [`apply_tool_bar_messages`].
enum ToolBarMsg {
    Tool(String),
    Mode(i32),
    Setting(String, String),
    Material(String, i32),
    Action(String),
    Preset(String, String),
    Close,
}

#[derive(Resource, Default)]
struct TerrainToolsInbox(Arc<Mutex<Vec<ToolBarMsg>>>);

/// Hook the global's callbacks once the Slint window exists.
fn register_callbacks(
    slint: Option<NonSend<crate::ui::SlintUiState>>,
    inbox: Res<TerrainToolsInbox>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(slint) = slint else { return };
    let g = slint.window.global::<TerrainToolsUi>();
    let push = |queue: &Arc<Mutex<Vec<ToolBarMsg>>>, msg: ToolBarMsg| {
        if let Ok(mut queue) = queue.lock() {
            queue.push(msg);
        }
    };
    let q = inbox.0.clone();
    g.on_tool_selected(move |id: SharedString| push(&q, ToolBarMsg::Tool(id.to_string())));
    let q = inbox.0.clone();
    g.on_mode_selected(move |index: i32| push(&q, ToolBarMsg::Mode(index)));
    let q = inbox.0.clone();
    g.on_setting_changed(move |key: SharedString, value: SharedString| {
        push(&q, ToolBarMsg::Setting(key.to_string(), value.to_string()))
    });
    let q = inbox.0.clone();
    g.on_material_picked(move |which: SharedString, slot: i32| push(&q, ToolBarMsg::Material(which.to_string(), slot)));
    let q = inbox.0.clone();
    g.on_action(move |name: SharedString| push(&q, ToolBarMsg::Action(name.to_string())));
    let q = inbox.0.clone();
    g.on_preset(move |op: SharedString, name: SharedString| {
        push(&q, ToolBarMsg::Preset(op.to_string(), name.to_string()))
    });
    let q = inbox.0.clone();
    g.on_close(move || push(&q, ToolBarMsg::Close));
    *done = true;
}

/// A length typed or scrubbed in the display unit, in metres.
fn to_metres(value: &str, unit: Unit) -> Option<f32> {
    let value: f32 = value.trim().parse().ok()?;
    value.is_finite().then(|| convert_f32(value, unit, Unit::Meter))
}

fn parse_bool(value: &str) -> bool {
    matches!(value.trim(), "true" | "1" | "on")
}

/// Apply what the bar sent since last frame.
#[allow(clippy::too_many_arguments)]
fn apply_tool_bar_messages(
    inbox: Res<TerrainToolsInbox>,
    mut brush: ResMut<TerrainBrush>,
    mut presets: ResMut<TerrainBrushPresets>,
    hover: Res<TerrainBrushHover>,
    display_unit: Option<Res<DisplayUnit>>,
    mut tool_events: MessageWriter<SetTerrainBrushEvent>,
    studio_state: Option<ResMut<crate::ui::StudioState>>,
    (mut sea_level, mut region): (Option<ResMut<SeaLevelTool>>, Option<ResMut<RegionTool>>),
) {
    let messages: Vec<ToolBarMsg> = match inbox.0.lock() {
        Ok(mut queue) if !queue.is_empty() => queue.drain(..).collect(),
        _ => return,
    };
    let unit = display_unit.as_deref().map_or(Unit::Meter, DisplayUnit::get);
    let mut studio_state = studio_state;
    for msg in messages {
        match msg {
            ToolBarMsg::Tool(id) => {
                if let Some(tool) = TerrainTool::from_id(&id) {
                    tool_events.write(SetTerrainBrushEvent { tool, mode: None });
                }
            }
            ToolBarMsg::Mode(index) => {
                if let Ok(index) = usize::try_from(index) {
                    let tool = brush.tool;
                    brush.set_mode_index(tool, index);
                }
            }
            ToolBarMsg::Setting(key, value) if key == "sea-level" => {
                if let (Some(sea_level), Some(metres)) = (sea_level.as_deref_mut(), to_metres(&value, unit)) {
                    sea_level.level = Some(metres);
                }
            }
            ToolBarMsg::Setting(key, value) if key == "region-angle" || key == "region-scale" => {
                if let Some(region) = region.as_deref_mut() {
                    apply_transform_setting(region, &key, &value);
                }
            }
            ToolBarMsg::Setting(key, value) => apply_setting(&mut brush, &hover, &key, &value, unit),
            ToolBarMsg::Material(which, slot) => {
                let Ok(slot) = u8::try_from(slot) else { continue };
                if which == "source" {
                    brush.source_material = slot;
                } else {
                    brush.paint_material = slot;
                }
            }
            ToolBarMsg::Action(name) => {
                let done = match brush.tool {
                    TerrainTool::SeaLevel => sea_level_action(&name).zip(sea_level.as_deref_mut()).map(|(mode, tool)| {
                        tool.pending = Some(mode);
                    }),
                    TerrainTool::Region => region_action(&name).zip(region.as_deref_mut()).map(|(op, tool)| {
                        tool.pending = Some(op);
                    }),
                    _ => None,
                };
                if done.is_none() {
                    warn!("terrain tool bar sent an action {name:?} the {} tool does not have", brush.tool.label());
                }
            }
            ToolBarMsg::Preset(op, name) => apply_preset(&mut brush, &mut presets, &op, name.trim()),
            ToolBarMsg::Close => {
                if let Some(state) = studio_state.as_deref_mut() {
                    if state.current_tool == crate::ui::Tool::Terrain {
                        state.current_tool = crate::ui::Tool::Select;
                    }
                }
            }
        }
    }
}

/// Region's Transform fields: `region-angle` in degrees (kept within a
/// turn) and `region-scale` in percent (kept within
/// [`REGION_SCALE_RANGE`]). A box is needed; the Transform opens on it.
fn apply_transform_setting(region: &mut RegionTool, key: &str, value: &str) {
    if region.region.is_none() {
        return;
    }
    let Ok(number) = value.trim().parse::<f32>() else { return };
    if !number.is_finite() {
        return;
    }
    let transform = region.transform.get_or_insert_with(RegionTransform::default);
    match key {
        "region-angle" => transform.angle = number.rem_euclid(360.0),
        "region-scale" => transform.scale = (number / 100.0).clamp(REGION_SCALE_RANGE.0, REGION_SCALE_RANGE.1),
        _ => {}
    }
}

/// One `setting-changed(key, value)` from the bar.
fn apply_setting(brush: &mut TerrainBrush, hover: &TerrainBrushHover, key: &str, value: &str, unit: Unit) {
    match key {
        "size" => {
            if let Some(metres) = to_metres(value, unit) {
                brush.set_size(metres);
            }
        }
        "strength" => {
            if let Ok(strength) = value.trim().parse::<f32>() {
                brush.set_strength(strength);
            }
        }
        "height" => {
            if let Some(metres) = to_metres(value, unit) {
                brush.set_draw_height(metres);
            }
        }
        "shape" => {
            if let Some(shape) = BrushShape::from_id(value) {
                brush.shape = shape;
            }
        }
        "pivot" => {
            if let Some(pivot) = BrushPivot::from_id(value) {
                brush.pivot = pivot;
            }
        }
        "plane-lock" => {
            brush.plane_lock = if parse_bool(value) {
                brush.plane_lock.or(hover.surface.map(|p| p.y)).or(hover.target.map(|p| p.y)).or(Some(0.0))
            } else {
                None
            };
        }
        "plane-height" => {
            if let Some(metres) = to_metres(value, unit) {
                brush.plane_lock = Some(metres);
            }
        }
        "snap" => brush.snap = parse_bool(value),
        "snap-step" => {
            if let Ok(step) = value.trim().parse::<f32>() {
                if step.is_finite() && step > 0.0 {
                    brush.snap_step = step;
                }
            }
        }
        "contours" => brush.contours = parse_bool(value),
        "mirror" => {
            if let Some(axes) = MirrorAxes::from_id(value) {
                let was_off = brush.mirror == MirrorAxes::Off;
                brush.set_mirror(axes);
                if was_off && axes != MirrorAxes::Off {
                    if let Some(at) = hover.surface.or(hover.target) {
                        brush.mirror_origin = at.xz();
                    }
                }
            }
        }
        "auto-material" => brush.auto_material = parse_bool(value),
        "hardness" => {
            if let Some(hardness) = BrushHardness::from_id(value) {
                brush.hardness = hardness;
            }
        }
        "smoothing" => {
            if let Ok(smoothing) = value.trim().parse::<f32>() {
                if smoothing.is_finite() {
                    brush.smoothing = smoothing.clamp(0.0, 1.0);
                }
            }
        }
        _ => warn!("terrain tool bar sent an unknown setting {key:?}"),
    }
}

/// `preset(op, name)`: apply, save or delete a named brush. Applying keeps
/// the active tool; a preset holds everything else.
fn apply_preset(brush: &mut TerrainBrush, presets: &mut TerrainBrushPresets, op: &str, name: &str) {
    if name.is_empty() {
        return;
    }
    match op {
        "apply" => {
            if let Some(preset) = presets.presets.iter().find(|p| p.name == name) {
                let tool = brush.tool;
                *brush = TerrainBrush { tool, ..preset.brush.clone() };
                presets.active = Some(name.to_string());
            }
        }
        "save" => {
            let saved = BrushPreset { name: name.to_string(), brush: brush.clone() };
            match presets.presets.iter_mut().find(|p| p.name == name) {
                Some(existing) => *existing = saved,
                None => presets.presets.push(saved),
            }
            presets.active = Some(name.to_string());
        }
        "delete" => {
            presets.presets.retain(|p| p.name != name);
            if presets.active.as_deref() == Some(name) {
                presets.active = None;
            }
        }
        _ => {}
    }
}

// ============================================================================
// Thumbnails
// ============================================================================

/// Side of a material thumbnail, in pixels.
const THUMBNAIL_SIDE: u32 = 64;

/// Decoded RGBA pixels of one thumbnail.
#[derive(Clone)]
struct ThumbnailPixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// The material thumbnails the picker shows, decoded off the main thread
/// from each slot's albedo texture.
#[derive(Resource, Default)]
struct TerrainThumbnails {
    /// Decoded so far, by the albedo path they came from.
    done: Arc<Mutex<HashMap<PathBuf, ThumbnailPixels>>>,
    /// Paths already handed to a decoder.
    requested: std::collections::HashSet<PathBuf>,
    /// Bumped by the decoder each time a thumbnail lands.
    revision: Arc<std::sync::atomic::AtomicU64>,
}

/// The albedo texture of a slot, for its thumbnail.
fn slot_albedo(slot: &eustress_common::terrain::MaterialSlot, bundled_root: &std::path::Path) -> Option<PathBuf> {
    slot.texture_set.as_ref().map(|set| set.files(bundled_root).albedo)
}

/// Hand every slot albedo not yet asked for to a background decoder.
fn request_thumbnails(slots: Option<Res<TerrainMaterialSlots>>, mut thumbnails: ResMut<TerrainThumbnails>) {
    let Some(slots) = slots else { return };
    if !slots.is_changed() && !thumbnails.requested.is_empty() {
        return;
    }
    let root = eustress_common::avatar::boot::bundled_root();
    let wanted: Vec<PathBuf> = slots
        .iter()
        .filter_map(|(_, slot)| slot_albedo(slot, &root))
        .filter(|path| !thumbnails.requested.contains(path))
        .collect();
    if wanted.is_empty() {
        return;
    }
    thumbnails.requested.extend(wanted.iter().cloned());
    let done = thumbnails.done.clone();
    let revision = thumbnails.revision.clone();
    std::thread::Builder::new()
        .name("terrain-thumbnails".into())
        .spawn(move || {
            for path in wanted {
                let Ok(image) = image::open(&path) else { continue };
                let small = image.thumbnail(THUMBNAIL_SIDE, THUMBNAIL_SIDE).to_rgba8();
                let pixels = ThumbnailPixels { width: small.width(), height: small.height(), rgba: small.into_raw() };
                if let Ok(mut done) = done.lock() {
                    done.insert(path, pixels);
                }
                revision.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .ok();
}

/// The Slint image of a decoded thumbnail.
fn thumbnail_image(pixels: &ThumbnailPixels) -> slint::Image {
    slint::Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&pixels.rgba, pixels.width, pixels.height))
}

// ============================================================================
// Sync
// ============================================================================

/// What the last sync pushed, so an unchanged frame pushes nothing.
#[derive(Default)]
struct SyncCache {
    tool: Option<TerrainTool>,
    keys: Vec<String>,
    materials_revision: Option<(u64, u64)>,
    presets: Vec<String>,
    readout: Option<TerrainCursorReadout>,
    tool_actions: Option<ToolBarActions>,
}

/// What the bar shows for Sea Level: its level field, whether its buttons
/// can apply, and the hint.
#[derive(Clone, Debug, Default, PartialEq)]
struct ToolBarActions {
    /// The buttons, and which of them can act now.
    actions: Vec<String>,
    enabled: Vec<bool>,
    hint: String,
    /// Sea Level's level in the display unit, and whether one is set.
    level: f32,
    level_set: bool,
    /// Region's Transform fields: shown, its angle in degrees and scale in
    /// percent.
    show_transform: bool,
    transform_angle: f32,
    transform_scale: f32,
}

impl ToolBarActions {
    /// The buttons and hint for the brush's tool and mode, from the Sea
    /// Level and Region tools' state; levels in the display unit through
    /// `display`. The brushes have none.
    fn of(
        brush: &TerrainBrush,
        sea_level: Option<&SeaLevelTool>,
        region: Option<&RegionTool>,
        display: impl Fn(f32) -> f32,
    ) -> Self {
        let buttons = |labels: &[&str], enabled: Vec<bool>, hint: &str| ToolBarActions {
            actions: labels.iter().map(|label| label.to_string()).collect(),
            enabled,
            hint: hint.to_string(),
            ..ToolBarActions::default()
        };
        match brush.tool {
            TerrainTool::SeaLevel => {
                let rect = sea_level.and_then(|sea_level| sea_level.rect);
                let level = sea_level.and_then(|sea_level| sea_level.level);
                let can_apply = rect.is_some() && level.is_some();
                let hint = if rect.is_some() { "Drag inside it to move the level" } else { "Drag a rectangle on the ground" };
                let labels: Vec<&str> = SEA_LEVEL_ACTIONS.iter().map(|(label, _)| *label).collect();
                ToolBarActions {
                    level: display(level.unwrap_or(0.0)),
                    level_set: level.is_some(),
                    ..buttons(labels.as_slice(), vec![can_apply; labels.len()], hint)
                }
            }
            TerrainTool::Region => {
                let has_box = region.is_some_and(|region| region.region.is_some());
                let has_copy = region.is_some_and(|region| region.clipboard.is_some());
                let pasting = region.is_some_and(|region| region.paste_at.is_some());
                let moved = region
                    .and_then(|region| region.transform)
                    .is_some_and(|transform| transform != RegionTransform::default());
                let labels = |table: &[(&'static str, RegionOp)]| -> Vec<&'static str> {
                    table.iter().map(|(label, _)| *label).collect()
                };
                match brush.region_mode {
                    RegionMode::Select => {
                        let hint = if pasting {
                            "Click to place the copy; Esc cancels"
                        } else if has_box {
                            "Drag a face handle to resize the box"
                        } else {
                            "Drag a box on the ground"
                        };
                        buttons(
                            labels(&REGION_SELECT_ACTIONS[..]).as_slice(),
                            vec![has_box, has_box, has_copy, has_box, has_box],
                            hint,
                        )
                    }
                    RegionMode::Transform => {
                        let hint = if has_box {
                            "Drag the box to move it; PageUp and PageDown raise it"
                        } else {
                            "Draw a box in Select first"
                        };
                        let transform =
                            region.and_then(|region| region.transform).unwrap_or_default();
                        ToolBarActions {
                            show_transform: true,
                            transform_angle: transform.angle,
                            transform_scale: transform.scale * 100.0,
                            ..buttons(labels(&REGION_TRANSFORM_ACTIONS[..]).as_slice(), vec![has_box, moved, moved], hint)
                        }
                    }
                    RegionMode::Fill => {
                        let hint = if has_box { "" } else { "Drag a box on the ground" };
                        buttons(labels(&REGION_FILL_ACTIONS[..]).as_slice(), vec![has_box, has_box], hint)
                    }
                }
            }
            _ => ToolBarActions::default(),
        }
    }
}

fn string_model(items: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(items.iter().map(SharedString::from).collect::<Vec<_>>()))
}

fn slint_rgb(rgb: [f32; 3]) -> slint::Color {
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    slint::Color::from_rgb_u8(byte(rgb[0]), byte(rgb[1]), byte(rgb[2]))
}

/// Push the terrain tools' state to the `TerrainToolsUi` global: every
/// property the bar and ribbon read, compared first so an idle frame costs a
/// handful of reads, and the readout every frame the cursor moves.
#[allow(clippy::too_many_arguments)]
fn sync_terrain_tools_to_slint(
    slint: Option<NonSend<crate::ui::SlintUiState>>,
    mode: Res<TerrainMode>,
    brush: Res<TerrainBrush>,
    presets: Res<TerrainBrushPresets>,
    readout: Res<TerrainCursorReadout>,
    (slots, thumbnails, display_unit, bindings, keys): (
        Option<Res<TerrainMaterialSlots>>,
        Res<TerrainThumbnails>,
        Option<Res<DisplayUnit>>,
        Option<Res<KeyBindings>>,
        Res<ButtonInput<KeyCode>>,
    ),
    (sea_level, region): (Option<Res<SeaLevelTool>>, Option<Res<RegionTool>>),
    mut cache: Local<SyncCache>,
) {
    let Some(slint) = slint else { return };
    let g = slint.window.global::<TerrainToolsUi>();
    let unit = display_unit.as_deref().map_or(Unit::Meter, DisplayUnit::get);
    let display = |metres: f32| convert_f32(metres, Unit::Meter, unit);

    // The bar shows while the tools are on, unless the UI Builder panel is
    // open over the same corner or a modal tool's options bar holds it.
    let ui_builder_open = slint.window.global::<crate::ui::slint_ui::UiBuilderUi>().get_open();
    let active = *mode == TerrainMode::Editor && !ui_builder_open && !slint.window.get_tool_options_visible();
    if g.get_active() != active {
        g.set_active(active);
    }
    let readout_visible = readout.visible && *mode == TerrainMode::Editor;
    if g.get_readout_visible() != readout_visible {
        g.set_readout_visible(readout_visible);
    }

    // The tools' keys, from the binding table.
    if let Some(bindings) = bindings.as_deref() {
        let keys: Vec<String> = [
            Action::TerrainDraw,
            Action::TerrainSculpt,
            Action::TerrainSmooth,
            Action::TerrainFlatten,
            Action::TerrainPaint,
            Action::TerrainSeaLevel,
            Action::TerrainRegion,
        ]
        .into_iter()
        .map(|action| bindings.get(action).map(|binding| binding.display()).unwrap_or_default())
        .collect();
        if keys != cache.keys {
            g.set_tool_keys(string_model(&keys));
            cache.keys = keys;
        }
        let tools_key: SharedString =
            bindings.get(Action::TerrainTools).map(|binding| binding.display()).unwrap_or_default().into();
        if g.get_tools_key() != tools_key {
            g.set_tools_key(tools_key);
        }
    }

    // The readout, every frame it moves.
    if cache.readout.as_ref() != Some(&*readout) {
        g.set_readout_x(readout.x);
        g.set_readout_y(readout.y);
        g.set_readout_text(readout.text.as_str().into());
        if let Some(family) = readout.family {
            g.set_readout_accent(slint_rgb(family_rgb(family)));
        }
        g.set_readout_plane(readout.plane);
        g.set_readout_snap(readout.snap);
        g.set_readout_contours(readout.contours);
        g.set_readout_mirror(readout.mirror);
        cache.readout = Some(readout.clone());
    }

    // The modes, and which one Ctrl swaps to while it is held.
    let tool = brush.tool;
    if cache.tool != Some(tool) {
        let labels: Vec<String> = tool.mode_labels().iter().map(|label| label.to_string()).collect();
        g.set_modes(string_model(&labels));
        g.set_active_tool(tool.id().into());
        // Sea Level and Region show their own settings and buttons in place
        // of the brush's.
        g.set_show_brush(tool.is_brush());
        g.set_show_mirror(tool.is_brush());
        g.set_show_level(tool == TerrainTool::SeaLevel);
        cache.tool = Some(tool);
    }
    let tool_actions = ToolBarActions::of(&brush, sea_level.as_deref(), region.as_deref(), display);
    if cache.tool_actions.as_ref() != Some(&tool_actions) {
        g.set_actions(string_model(&tool_actions.actions));
        g.set_action_enabled(ModelRc::new(VecModel::from(tool_actions.enabled.clone())));
        g.set_actions_enabled(true);
        g.set_hint(tool_actions.hint.as_str().into());
        g.set_level(tool_actions.level);
        g.set_level_set(tool_actions.level_set);
        g.set_show_region_transform(tool_actions.show_transform);
        g.set_region_angle(tool_actions.transform_angle);
        g.set_region_scale(tool_actions.transform_scale);
        cache.tool_actions = Some(tool_actions);
    }
    let mode_index = brush.mode_index(tool) as i32;
    if g.get_active_mode() != mode_index {
        g.set_active_mode(mode_index);
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let swapped = match (tool, ctrl) {
        (TerrainTool::Draw | TerrainTool::Sculpt | TerrainTool::SeaLevel, true) => 1 - mode_index,
        (TerrainTool::Flatten, true) if mode_index < 2 => 1 - mode_index,
        _ => -1,
    };
    if g.get_swapped_mode() != swapped {
        g.set_swapped_mode(swapped);
    }

    // The materials, when the table or the thumbnails change.
    let revision = thumbnails.revision.load(std::sync::atomic::Ordering::Relaxed);
    let slots_tick = slots.as_ref().map_or(0, |slots| slots.last_changed().get() as u64);
    let root = eustress_common::avatar::boot::bundled_root();
    let pixels_for = |slot: u8| -> Option<ThumbnailPixels> {
        let slots = slots.as_deref()?;
        let path = slot_albedo(slots.get(slot)?, &root)?;
        thumbnails.done.lock().ok()?.get(&path).cloned()
    };
    if cache.materials_revision != Some((slots_tick, revision)) {
        if let Some(slots) = slots.as_deref() {
            let tile = |slot: u8, def: &eustress_common::terrain::MaterialSlot| {
                let pixels = pixels_for(slot);
                TerrainMaterialTile {
                    slot_id: i32::from(slot),
                    name: def.name.as_str().into(),
                    swatch: slint_rgb(def.swatch_srgb()),
                    thumbnail: pixels.as_ref().map(thumbnail_image).unwrap_or_default(),
                    has_thumbnail: pixels.is_some(),
                    custom: slot >= FIRST_CUSTOM_MATERIAL_SLOT,
                }
            };
            // Water is real water, never a ground colour: its own tile under
            // the ground materials rather than one of them.
            let water_slot = eustress_common::terrain::TerrainMaterial::Water.to_u8();
            let tiles: Vec<TerrainMaterialTile> =
                slots.iter().filter(|(slot, _)| *slot != water_slot).map(|(slot, def)| tile(slot, def)).collect();
            let water_tile = slots.get(water_slot).map(|def| tile(water_slot, def));
            g.set_has_water_tile(water_tile.is_some());
            g.set_water_tile(water_tile.unwrap_or_default());
            g.set_materials(ModelRc::new(VecModel::from(tiles)));
            cache.materials_revision = Some((slots_tick, revision));
            // The swatches below re-read their thumbnails on this pass.
            g.set_material(-1);
            g.set_source_material(-1);
        }
    }

    // Everything else comes from the brush, pushed when it changed.
    let material_changed = g.get_material() != i32::from(brush.paint_material)
        || g.get_source_material() != i32::from(brush.source_material);
    if !brush.is_changed() && !display_unit.as_ref().is_some_and(|u| u.is_changed()) && !material_changed {
        sync_presets(&g, &presets, &mut cache);
        return;
    }
    g.set_unit(unit.symbol().into());
    g.set_size(display(brush.size()));
    g.set_size_min(display(BRUSH_SIZE_MIN));
    g.set_size_max(display(BRUSH_SIZE_MAX));
    let strength_tools = matches!(tool, TerrainTool::Sculpt | TerrainTool::Smooth | TerrainTool::Flatten | TerrainTool::Paint);
    g.set_show_strength(strength_tools);
    g.set_strength(brush.strength());
    g.set_show_height(tool == TerrainTool::Draw && brush.shape.has_height());
    g.set_brush_height(display(brush.draw_height()));
    g.set_shape(brush.shape.id().into());
    g.set_show_pivot(tool == TerrainTool::Draw);
    g.set_pivot(brush.pivot.id().into());
    g.set_show_plane(tool.is_brush());
    g.set_plane_lock(brush.plane_lock.is_some());
    g.set_plane_height(display(brush.plane_lock.unwrap_or(0.0)));
    g.set_snap(brush.snap);
    g.set_snap_step(brush.snap_step);
    let steps: Vec<String> = SNAP_STEPS.iter().map(|step| format!("{step}")).collect();
    g.set_snap_steps(string_model(&steps));
    g.set_contours(brush.contours);
    g.set_mirror(brush.mirror.id().into());
    let replace = tool == TerrainTool::Paint && brush.paint_mode == PaintMode::Replace;
    // Region's Fill fills with the material and Replace swaps the source
    // for it, so both swatches show there.
    let region_fill = tool == TerrainTool::Region && brush.region_mode == RegionMode::Fill;
    g.set_show_material(matches!(tool, TerrainTool::Draw | TerrainTool::Paint) || region_fill);
    g.set_material_label(if replace { "Target" } else { "Material" }.into());
    let slot_name = |slot: u8| {
        slots
            .as_deref()
            .and_then(|slots| slots.get(slot))
            .map_or_else(|| format!("Material {slot}"), |def| def.name.clone())
    };
    let slot_swatch = |slot: u8| {
        slots.as_deref().map_or(slint::Color::from_rgb_u8(128, 128, 128), |slots| slint_rgb(slots.swatch_srgb(slot)))
    };
    let material = brush.paint_material;
    let material_pixels = pixels_for(material);
    g.set_material(i32::from(material));
    g.set_material_name(slot_name(material).into());
    g.set_material_swatch(slot_swatch(material));
    g.set_material_has_thumbnail(material_pixels.is_some());
    g.set_material_thumbnail(material_pixels.as_ref().map(thumbnail_image).unwrap_or_default());
    g.set_show_source(replace || region_fill);
    let source = brush.source_material;
    let source_pixels = pixels_for(source);
    g.set_source_material(i32::from(source));
    g.set_source_name(slot_name(source).into());
    g.set_source_swatch(slot_swatch(source));
    g.set_source_has_thumbnail(source_pixels.is_some());
    g.set_source_thumbnail(source_pixels.as_ref().map(thumbnail_image).unwrap_or_default());
    g.set_show_auto_material(tool == TerrainTool::Draw);
    g.set_auto_material(brush.auto_material);
    g.set_show_hardness(strength_tools);
    g.set_hardness(brush.hardness.id().into());
    g.set_smoothing(brush.smoothing);
    sync_presets(&g, &presets, &mut cache);
}

/// Push the preset names and the current one when they changed.
fn sync_presets(g: &TerrainToolsUi<'_>, presets: &TerrainBrushPresets, cache: &mut SyncCache) {
    let names: Vec<String> = presets.presets.iter().map(|p| p.name.clone()).collect();
    if names != cache.presets {
        g.set_presets(string_model(&names));
        cache.presets = names;
    }
    let active: SharedString = presets.active.clone().unwrap_or_default().into();
    if g.get_active_preset() != active {
        g.set_active_preset(active);
    }
}

/// Whether logical cursor `pos` is over the terrain tool bar, which the
/// overlay draws inside the viewport at `viewport` `(x, y, width, height)`
/// (logical pixels). The Studio's focus test calls this so a click on the
/// bar stays on the UI.
pub fn tool_bar_contains(window: &StudioWindow, pos: (f32, f32), viewport: (f32, f32, f32, f32)) -> bool {
    let g = window.global::<TerrainToolsUi>();
    if !g.get_active() {
        return false;
    }
    let (vx, vy, _, _) = viewport;
    let (x, y) = (vx + g.get_bar_x(), vy + g.get_bar_y());
    let (w, h) = (g.get_bar_width(), g.get_bar_height());
    w > 0.0 && h > 0.0 && pos.0 >= x && pos.0 <= x + w && pos.1 >= y && pos.1 <= y + h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_save_apply_and_delete_by_name() {
        let mut brush = TerrainBrush::default();
        let mut presets = TerrainBrushPresets::default();
        brush.set_size(40.0);
        apply_preset(&mut brush, &mut presets, "save", "Wide");
        assert_eq!(presets.presets.len(), 1);
        assert_eq!(presets.active.as_deref(), Some("Wide"));
        brush.set_size(4.0);
        brush.tool = TerrainTool::Paint;
        apply_preset(&mut brush, &mut presets, "apply", "Wide");
        assert_eq!(brush.tool, TerrainTool::Paint, "applying keeps the active tool");
        assert_eq!(brush.surface_size, 40.0);
        apply_preset(&mut brush, &mut presets, "delete", "Wide");
        assert!(presets.presets.is_empty() && presets.active.is_none());
    }

    #[test]
    fn settings_arrive_in_the_display_unit() {
        let mut brush = TerrainBrush::default();
        let hover = TerrainBrushHover::default();
        apply_setting(&mut brush, &hover, "size", "10", Unit::Meter);
        assert_eq!(brush.size(), 10.0);
        apply_setting(&mut brush, &hover, "shape", "box", Unit::Meter);
        assert_eq!(brush.shape, BrushShape::Box);
        apply_setting(&mut brush, &hover, "plane-lock", "true", Unit::Meter);
        assert_eq!(brush.plane_lock, Some(0.0), "off the terrain the plane starts at 0");
        apply_setting(&mut brush, &hover, "plane-height", "12.5", Unit::Meter);
        assert_eq!(brush.plane_lock, Some(12.5));
        apply_setting(&mut brush, &hover, "mirror", "xz", Unit::Meter);
        assert_eq!(brush.mirror, MirrorAxes::XZ);
        apply_setting(&mut brush, &hover, "strength", "nan", Unit::Meter);
        assert!(brush.strength().is_finite());
    }

    #[test]
    fn each_tool_shows_its_own_buttons_and_enables_what_can_act() {
        let mut brush = TerrainBrush::default();
        assert_eq!(ToolBarActions::of(&brush, None, None, |m| m), ToolBarActions::default(), "brushes have none");

        brush.tool = TerrainTool::SeaLevel;
        let mut sea = SeaLevelTool::default();
        let bar = ToolBarActions::of(&brush, Some(&sea), None, |m| m);
        assert_eq!(bar.actions, ["Fill", "Evaporate"]);
        assert_eq!(bar.enabled, [false, false], "nothing to apply before a rectangle");
        sea.rect = Some((Vec2::ZERO, Vec2::ONE));
        sea.level = Some(3.0);
        let bar = ToolBarActions::of(&brush, Some(&sea), None, |m| m * 2.0);
        assert_eq!((bar.enabled.clone(), bar.level, bar.level_set), (vec![true, true], 6.0, true));
        assert_eq!(sea_level_action("Evaporate"), Some(SeaLevelMode::Evaporate));

        brush.tool = TerrainTool::Region;
        let mut region = RegionTool::default();
        let bar = ToolBarActions::of(&brush, None, Some(&region), |m| m);
        assert_eq!(bar.actions, ["Copy", "Cut", "Paste", "Duplicate", "Delete"]);
        assert!(bar.enabled.iter().all(|enabled| !enabled), "no box and no copy");
        region.clipboard = Some(crate::terrain_region::RegionClip {
            dims: UVec3::ONE,
            resolution: 1.0,
            materials: vec![eustress_common::terrain::api::TerrainFill::Air],
            occupancies: vec![0.0],
            ground_above_bottom: 0.0,
        });
        assert_eq!(ToolBarActions::of(&brush, None, Some(&region), |m| m).enabled, [false, false, true, false, false]);
        brush.region_mode = RegionMode::Fill;
        assert_eq!(ToolBarActions::of(&brush, None, Some(&region), |m| m).actions, ["Fill", "Replace"]);
        assert_eq!(region_action("Apply"), Some(RegionOp::ApplyTransform));
        assert_eq!(region_action("Nope"), None);
    }
}

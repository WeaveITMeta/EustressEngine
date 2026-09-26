//! Terrain brush editing: the settings the terrain tools read, and the
//! stroke that applies them (see `docs/design/TERRAIN_TOOLS_UX.md`).
//!
//! [`TerrainBrush`] holds what the user sets: the active [`TerrainTool`] and
//! each tool's mode, the brush shape, size and strength, plane lock, snap,
//! mirror and the materials. At a stroke's press the modifiers held turn the
//! tool into a [`BrushAction`] (`Ctrl` swaps the tool's mode, `Shift` smooths),
//! fixed for the whole stroke, and [`TerrainBrush::dab`] turns that into a
//! [`BrushDab`], the parameters every dab of the stroke applies.
//!
//! The surface actions (Grow, Erode, Smooth, Flatten, Paint, Replace) write
//! the height raster and the material map cell by cell
//! ([`apply_surface_dab`]), each cell at most once a dab. Draw's Add and
//! Subtract write the sparse [`TerrainVolume`] one CSG shape a dab
//! ([`apply_draw_dab`]), so they build overhangs and dig caves; Smooth also
//! rounds 3D edits inside its ball. Dabs are spaced along the path by
//! distance and repeat at a fixed rate while the cursor rests
//! ([`terrain_paint_system`]), so a stroke does the same at any frame rate.
//!
//! [`update_brush_hover`] works out, every frame, where the brush is and what
//! a press would do ([`TerrainBrushHover`]); the stroke and the host's cursor
//! both read it, so what the cursor shows is where the dab lands.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::brush_cursor::Footprint;
use super::height_query::{ensure_material_cache, height_at_world, material_at_world, raycast_terrain_surface};
use super::material::{material_cell_weights, paint_material_cell, MaterialCell, TerrainMaterial, MATERIAL_SLOT_NONE};
use super::volume::{apply_shape_clipped, apply_smooth, ClipPlane, CsgOp, CsgShape, VolumeEdit};
use super::water_bodies::CellGrid;
use super::{
    surface_data, terrain_stroke_label, TerrainBaked, TerrainConfig, TerrainData, TerrainDirtyChunks,
    TerrainEditRecorder, TerrainRoot, TerrainVolume,
};

// ============================================================================
// Tools and modes
// ============================================================================

/// The terrain tools (design section 4.4), in key order: `1` is Draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TerrainTool {
    /// Adds or carves a 3D brush volume.
    Draw,
    /// Grows or erodes the surface.
    #[default]
    Sculpt,
    /// Relaxes the surface and rounds 3D edits.
    Smooth,
    /// Levels toward a target height.
    Flatten,
    /// Paints or replaces material.
    Paint,
    /// Fills or evaporates water below a level over a rectangle.
    SeaLevel,
    /// Selects, transforms and fills a box of terrain.
    Region,
}

impl TerrainTool {
    /// How many tools there are.
    pub const COUNT: usize = 7;
    /// Every tool, in key order.
    pub const ALL: [Self; Self::COUNT] =
        [Self::Draw, Self::Sculpt, Self::Smooth, Self::Flatten, Self::Paint, Self::SeaLevel, Self::Region];

    /// Position in [`Self::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Stable id the UI and the keybindings use.
    pub fn id(self) -> &'static str {
        match self {
            Self::Draw => "draw",
            Self::Sculpt => "sculpt",
            Self::Smooth => "smooth",
            Self::Flatten => "flatten",
            Self::Paint => "paint",
            Self::SeaLevel => "sealevel",
            Self::Region => "region",
        }
    }

    /// The tool with id `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.id() == id)
    }

    /// The name the ribbon, the tool bar and the readout show.
    pub fn label(self) -> &'static str {
        match self {
            Self::Draw => "Draw",
            Self::Sculpt => "Sculpt",
            Self::Smooth => "Smooth",
            Self::Flatten => "Flatten",
            Self::Paint => "Paint",
            Self::SeaLevel => "Sea Level",
            Self::Region => "Region",
        }
    }

    /// Whether the tool strokes with a brush. Sea Level and Region drag
    /// boxes instead.
    pub fn is_brush(self) -> bool {
        !matches!(self, Self::SeaLevel | Self::Region)
    }

    /// Whether the tool's size is the volume brushes' (Draw) rather than
    /// the surface brushes'.
    pub fn uses_volume_size(self) -> bool {
        self == Self::Draw
    }

    /// The names of the tool's modes in order, empty for a tool without.
    pub fn mode_labels(self) -> &'static [&'static str] {
        match self {
            Self::Draw => &["Add", "Subtract"],
            Self::Sculpt => &["Grow", "Erode"],
            Self::Smooth => &[],
            Self::Flatten => &["Erode to Flat", "Grow to Flat", "Flatten All"],
            Self::Paint => &["Paint", "Replace"],
            Self::SeaLevel => &["Fill", "Evaporate"],
            Self::Region => &["Select", "Transform", "Fill"],
        }
    }
}

/// Draw's modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DrawMode {
    #[default]
    Add,
    Subtract,
}

/// Sculpt's modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SculptMode {
    #[default]
    Grow,
    Erode,
}

/// Flatten's modes: which way it may move the ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FlattenMode {
    /// Cuts ground above the target, never raises.
    ErodeToFlat,
    /// Fills ground below the target, never lowers.
    GrowToFlat,
    /// Both.
    #[default]
    FlattenAll,
}

impl FlattenMode {
    /// The mode `Ctrl` swaps to: Erode and Grow trade places; Flatten All
    /// stays.
    pub fn swapped(self) -> Self {
        match self {
            Self::ErodeToFlat => Self::GrowToFlat,
            Self::GrowToFlat => Self::ErodeToFlat,
            Self::FlattenAll => Self::FlattenAll,
        }
    }
}

/// Paint's modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PaintMode {
    #[default]
    Paint,
    /// Paints the target material only over the source material.
    Replace,
}

/// Sea Level's modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SeaLevelMode {
    #[default]
    Fill,
    Evaporate,
}

/// Region's modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RegionMode {
    #[default]
    Select,
    Transform,
    Fill,
}

/// The brush's shape. Its footprint on the ground is the shape's
/// cross-section: a disc for the sphere and the cylinder, a square for the
/// box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BrushShape {
    #[default]
    Sphere,
    Box,
    Cylinder,
}

impl BrushShape {
    /// Every shape, in tool bar order.
    pub const ALL: [Self; 3] = [Self::Sphere, Self::Box, Self::Cylinder];

    /// Stable id the UI uses.
    pub fn id(self) -> &'static str {
        match self {
            Self::Sphere => "sphere",
            Self::Box => "box",
            Self::Cylinder => "cylinder",
        }
    }

    /// The shape with id `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|shape| shape.id() == id)
    }

    /// Whether the footprint is a square (else a disc).
    pub fn footprint_square(self) -> bool {
        self == Self::Box
    }

    /// Whether the shape has a height of its own (else it is as tall as it
    /// is wide).
    pub fn has_height(self) -> bool {
        self != Self::Sphere
    }
}

/// How the push fades from the brush centre to its rim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BrushHardness {
    /// A straight ramp from the centre to the rim.
    Soft,
    /// A dome: full near the centre, fading faster toward the rim.
    #[default]
    Smooth,
    /// Nearly full to the rim, then a quick drop.
    Hard,
}

impl BrushHardness {
    /// Every hardness, in tool bar order.
    pub const ALL: [Self; 3] = [Self::Soft, Self::Smooth, Self::Hard];

    /// Stable id the UI uses.
    pub fn id(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Smooth => "smooth",
            Self::Hard => "hard",
        }
    }

    /// The hardness with id `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|hardness| hardness.id() == id)
    }

    /// The falloff exponent [`falloff_weight`] takes.
    pub fn falloff(self) -> f32 {
        match self {
            Self::Soft => 1.0,
            Self::Smooth => 0.5,
            Self::Hard => 0.15,
        }
    }
}

/// Where a Draw volume sits on the point the cursor hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BrushPivot {
    /// The volume's bottom on the hit: it builds up from the ground.
    Bottom,
    /// Centred on the hit.
    #[default]
    Center,
    /// The volume's top on the hit: it digs down into the ground.
    Top,
}

impl BrushPivot {
    /// Every pivot, in tool bar order.
    pub const ALL: [Self; 3] = [Self::Bottom, Self::Center, Self::Top];

    /// Stable id the UI uses.
    pub fn id(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Center => "center",
            Self::Top => "top",
        }
    }

    /// The pivot with id `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|pivot| pivot.id() == id)
    }

    /// The next pivot, wrapping (`.`).
    pub fn next(self) -> Self {
        Self::ALL[(self as usize + 1) % Self::ALL.len()]
    }

    /// The previous pivot, wrapping (`,`).
    pub fn prev(self) -> Self {
        Self::ALL[(self as usize + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Which mirror planes a stroke is repeated across.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MirrorAxes {
    #[default]
    Off,
    /// Across the plane `x = origin.x`.
    X,
    /// Across the plane `z = origin.y`.
    Z,
    /// Across both.
    XZ,
}

impl MirrorAxes {
    /// Every setting, in tool bar order.
    pub const ALL: [Self; 4] = [Self::Off, Self::X, Self::Z, Self::XZ];

    /// Stable id the UI uses.
    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::X => "x",
            Self::Z => "z",
            Self::XZ => "xz",
        }
    }

    /// The setting with id `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|axes| axes.id() == id)
    }

    /// Mirrors across `x = origin.x`.
    pub fn mirror_x(self) -> bool {
        matches!(self, Self::X | Self::XZ)
    }

    /// Mirrors across `z = origin.y`.
    pub fn mirror_z(self) -> bool {
        matches!(self, Self::Z | Self::XZ)
    }
}

// ============================================================================
// Settings
// ============================================================================

/// Smallest brush diameter, in metres.
pub const BRUSH_SIZE_MIN: f32 = 0.5;
/// Largest brush diameter, in metres.
pub const BRUSH_SIZE_MAX: f32 = 256.0;
/// Grid steps snap cycles through, in metres.
pub const SNAP_STEPS: [f32; 6] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0];
/// Factor `[` and `]` step the size by.
pub const SIZE_STEP_FACTOR: f32 = 1.25;
/// Points `Shift+[` and `Shift+]` step the strength by.
pub const STRENGTH_STEP: f32 = 0.1;
/// Weakest strength a brush keeps: a zero strength would stroke for nothing.
pub const STRENGTH_MIN: f32 = 0.01;
/// Seconds between the dabs of a resting surface stroke: 20 a second.
pub const SURFACE_DAB_INTERVAL_SECS: f64 = 0.05;
/// A full-strength Grow or Erode dab moves the ground at the brush centre by
/// this many brush radii.
pub const SCULPT_RATE: f32 = 0.04;
/// A full-strength Smooth or Flatten dab moves each cell this fraction of
/// the way toward its target. Kept below 1 so a stroke never shaves the
/// ground flat in one pass.
pub const BLEND_RATE: f32 = 0.5;
/// Dabs are laid this many brush radii apart along a stroke.
pub const DAB_SPACING: f32 = 0.25;
/// The most dabs laid along a stroke in one frame. A longer gap is a jump,
/// and the stroke starts afresh at the new point instead of stringing dabs
/// across the map.
pub const MAX_DABS_PER_FRAME: usize = 16;

/// Everything the user sets for the terrain tools (design section 4.5). The
/// Studio persists it between sessions.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerrainBrush {
    /// The active tool.
    pub tool: TerrainTool,
    pub draw_mode: DrawMode,
    pub sculpt_mode: SculptMode,
    pub flatten_mode: FlattenMode,
    pub paint_mode: PaintMode,
    pub sea_level_mode: SeaLevelMode,
    pub region_mode: RegionMode,
    /// Draw's diameter, in metres.
    pub volume_size: f32,
    /// The diameter of Sculpt, Smooth, Flatten and Paint, in metres.
    pub surface_size: f32,
    /// Draw's height with the box or the cylinder, in metres; `None` follows
    /// the size.
    pub height: Option<f32>,
    /// Strength (0 to 1) of each tool, indexed by [`TerrainTool::index`].
    pub strengths: [f32; TerrainTool::COUNT],
    pub shape: BrushShape,
    pub hardness: BrushHardness,
    pub pivot: BrushPivot,
    /// Height of the locked plane, `None` when plane lock is off.
    pub plane_lock: Option<f32>,
    /// Snap the brush centre to the grid.
    pub snap: bool,
    /// The grid step, in metres.
    pub snap_step: f32,
    /// Draw height contours around the cursor.
    pub contours: bool,
    /// Mirror planes strokes repeat across.
    pub mirror: MirrorAxes,
    /// The axes `M` turns back on.
    pub last_mirror: MirrorAxes,
    /// World XZ the mirror planes pass through.
    pub mirror_origin: Vec2,
    /// Draw Add fills with the material under the cursor at the stroke's
    /// start instead of [`Self::paint_material`].
    pub auto_material: bool,
    /// The active material slot: what Draw fills with, Paint lays down and
    /// Replace paints in.
    pub paint_material: u8,
    /// The material Replace paints over.
    pub source_material: u8,
    /// Stroke smoothing, 0 to 1: the brush centre trails the cursor on a
    /// leash of this fraction of its radius.
    pub smoothing: f32,
}

impl Default for TerrainBrush {
    fn default() -> Self {
        Self {
            tool: TerrainTool::default(),
            draw_mode: DrawMode::default(),
            sculpt_mode: SculptMode::default(),
            flatten_mode: FlattenMode::default(),
            paint_mode: PaintMode::default(),
            sea_level_mode: SeaLevelMode::default(),
            region_mode: RegionMode::default(),
            volume_size: 8.0,
            surface_size: 16.0,
            height: None,
            // Draw, Sculpt, Smooth, Flatten, Paint, Sea Level, Region.
            strengths: [0.5, 0.4, 0.5, 0.6, 0.8, 1.0, 1.0],
            shape: BrushShape::default(),
            hardness: BrushHardness::default(),
            pivot: BrushPivot::default(),
            plane_lock: None,
            snap: false,
            snap_step: 1.0,
            contours: false,
            mirror: MirrorAxes::Off,
            last_mirror: MirrorAxes::X,
            mirror_origin: Vec2::ZERO,
            auto_material: false,
            paint_material: TerrainMaterial::Grass.to_u8(),
            source_material: TerrainMaterial::Grass.to_u8(),
            smoothing: 0.0,
        }
    }
}

/// `size` held to the brush size range; a non-finite size is the smallest.
fn clamp_size(size: f32) -> f32 {
    if size.is_finite() {
        size.clamp(BRUSH_SIZE_MIN, BRUSH_SIZE_MAX)
    } else {
        BRUSH_SIZE_MIN
    }
}

impl TerrainBrush {
    /// The active tool's diameter, in metres.
    pub fn size(&self) -> f32 {
        if self.tool.uses_volume_size() {
            self.volume_size
        } else {
            self.surface_size
        }
    }

    /// Set the active tool's diameter, held to the size range.
    pub fn set_size(&mut self, size: f32) {
        let size = clamp_size(size);
        if self.tool.uses_volume_size() {
            self.volume_size = size;
        } else {
            self.surface_size = size;
        }
    }

    /// Step the size up or down by [`SIZE_STEP_FACTOR`].
    pub fn step_size(&mut self, up: bool) {
        let size = self.size();
        self.set_size(if up { size * SIZE_STEP_FACTOR } else { size / SIZE_STEP_FACTOR });
    }

    /// The active tool's radius, in metres.
    pub fn radius(&self) -> f32 {
        self.size() * 0.5
    }

    /// The active tool's strength, 0 to 1.
    pub fn strength(&self) -> f32 {
        self.strengths[self.tool.index()]
    }

    /// Set the active tool's strength, held to `STRENGTH_MIN..=1`.
    pub fn set_strength(&mut self, strength: f32) {
        if strength.is_finite() {
            self.strengths[self.tool.index()] = strength.clamp(STRENGTH_MIN, 1.0);
        }
    }

    /// Step the strength by [`STRENGTH_STEP`], landing on whole percent.
    pub fn step_strength(&mut self, up: bool) {
        let step = if up { STRENGTH_STEP } else { -STRENGTH_STEP };
        self.set_strength(((self.strength() + step) * 100.0).round() / 100.0);
    }

    /// Draw's height with the box or the cylinder, in metres.
    pub fn draw_height(&self) -> f32 {
        self.height.map_or(self.volume_size, clamp_size)
    }

    /// Set Draw's height, held to the size range.
    pub fn set_draw_height(&mut self, height: f32) {
        self.height = Some(clamp_size(height));
    }

    /// The falloff exponent of the brush's hardness.
    pub fn falloff(&self) -> f32 {
        self.hardness.falloff()
    }

    /// Index of `tool`'s current mode in [`TerrainTool::mode_labels`] (0 for
    /// a tool without modes).
    pub fn mode_index(&self, tool: TerrainTool) -> usize {
        match tool {
            TerrainTool::Draw => self.draw_mode as usize,
            TerrainTool::Sculpt => self.sculpt_mode as usize,
            TerrainTool::Smooth => 0,
            TerrainTool::Flatten => self.flatten_mode as usize,
            TerrainTool::Paint => self.paint_mode as usize,
            TerrainTool::SeaLevel => self.sea_level_mode as usize,
            TerrainTool::Region => self.region_mode as usize,
        }
    }

    /// Set `tool`'s mode by its index in [`TerrainTool::mode_labels`]; an
    /// index past the list changes nothing.
    pub fn set_mode_index(&mut self, tool: TerrainTool, index: usize) {
        match (tool, index) {
            (TerrainTool::Draw, 0) => self.draw_mode = DrawMode::Add,
            (TerrainTool::Draw, 1) => self.draw_mode = DrawMode::Subtract,
            (TerrainTool::Sculpt, 0) => self.sculpt_mode = SculptMode::Grow,
            (TerrainTool::Sculpt, 1) => self.sculpt_mode = SculptMode::Erode,
            (TerrainTool::Flatten, 0) => self.flatten_mode = FlattenMode::ErodeToFlat,
            (TerrainTool::Flatten, 1) => self.flatten_mode = FlattenMode::GrowToFlat,
            (TerrainTool::Flatten, 2) => self.flatten_mode = FlattenMode::FlattenAll,
            (TerrainTool::Paint, 0) => self.paint_mode = PaintMode::Paint,
            (TerrainTool::Paint, 1) => self.paint_mode = PaintMode::Replace,
            (TerrainTool::SeaLevel, 0) => self.sea_level_mode = SeaLevelMode::Fill,
            (TerrainTool::SeaLevel, 1) => self.sea_level_mode = SeaLevelMode::Evaporate,
            (TerrainTool::Region, 0) => self.region_mode = RegionMode::Select,
            (TerrainTool::Region, 1) => self.region_mode = RegionMode::Transform,
            (TerrainTool::Region, 2) => self.region_mode = RegionMode::Fill,
            _ => {}
        }
    }

    /// Set the mirror axes, remembering the last ones that were on.
    pub fn set_mirror(&mut self, axes: MirrorAxes) {
        self.mirror = axes;
        if axes != MirrorAxes::Off {
            self.last_mirror = axes;
        }
    }

    /// `M`: mirror off, or back on with the last axes that were on.
    pub fn toggle_mirror(&mut self) {
        if self.mirror == MirrorAxes::Off {
            let axes = if self.last_mirror == MirrorAxes::Off { MirrorAxes::X } else { self.last_mirror };
            self.set_mirror(axes);
        } else {
            self.set_mirror(MirrorAxes::Off);
        }
    }

    /// `Shift+M`: the next axes among X, Z, X and Z, turning mirror on.
    pub fn next_mirror_axes(&mut self) {
        self.set_mirror(match self.mirror {
            MirrorAxes::Off | MirrorAxes::XZ => MirrorAxes::X,
            MirrorAxes::X => MirrorAxes::Z,
            MirrorAxes::Z => MirrorAxes::XZ,
        });
    }

    /// `Shift+G`: the next grid step in [`SNAP_STEPS`], wrapping.
    pub fn next_snap_step(&mut self) {
        let next = SNAP_STEPS
            .iter()
            .position(|step| (*step - self.snap_step).abs() < 1e-4)
            .map_or(SNAP_STEPS[2], |i| SNAP_STEPS[(i + 1) % SNAP_STEPS.len()]);
        self.snap_step = next;
    }

    /// The grid step while snap is on, else `None`.
    pub fn active_snap(&self) -> Option<f32> {
        (self.snap && self.snap_step > 0.0).then_some(self.snap_step)
    }

    /// What a stroke does with the modifiers held (design section 4.3):
    /// `Shift` smooths from any brush; `Ctrl` swaps the tool's mode.
    pub fn action(&self, ctrl: bool, shift: bool) -> BrushAction {
        if shift && self.tool.is_brush() {
            return BrushAction::Smooth;
        }
        match self.tool {
            TerrainTool::Draw => match (self.draw_mode, ctrl) {
                (DrawMode::Add, false) | (DrawMode::Subtract, true) => BrushAction::Add,
                _ => BrushAction::Subtract,
            },
            TerrainTool::Sculpt => match (self.sculpt_mode, ctrl) {
                (SculptMode::Grow, false) | (SculptMode::Erode, true) => BrushAction::Grow,
                _ => BrushAction::Erode,
            },
            TerrainTool::Smooth => BrushAction::Smooth,
            TerrainTool::Flatten => {
                BrushAction::Flatten(if ctrl { self.flatten_mode.swapped() } else { self.flatten_mode })
            }
            TerrainTool::Paint => match self.paint_mode {
                PaintMode::Paint => BrushAction::Paint,
                PaintMode::Replace => BrushAction::Replace,
            },
            TerrainTool::SeaLevel => BrushAction::SeaLevel(match (self.sea_level_mode, ctrl) {
                (SeaLevelMode::Fill, false) | (SeaLevelMode::Evaporate, true) => SeaLevelMode::Fill,
                _ => SeaLevelMode::Evaporate,
            }),
            TerrainTool::Region => BrushAction::Region(self.region_mode),
        }
    }

    /// The dab parameters `action` strokes with, `None` for the actions that
    /// do not stroke (Sea Level, Region). Smooth reached with `Shift` keeps
    /// the active tool's size and strength.
    pub fn dab(&self, action: BrushAction) -> Option<BrushDab> {
        let mode = action.brush_mode()?;
        let radius = self.radius();
        Some(BrushDab {
            mode,
            radius,
            height: if self.shape.has_height() { self.draw_height() } else { self.size() },
            strength: self.strength(),
            falloff: self.falloff(),
            shape: self.shape,
            material: self.paint_material,
            source_material: self.source_material,
            flatten_mode: match action {
                BrushAction::Flatten(mode) => mode,
                _ => self.flatten_mode,
            },
            flatten_target: None,
            plane: self.plane_lock,
            pivot: self.pivot,
            rate: SCULPT_RATE * radius,
            blend: BLEND_RATE,
        })
    }
}

/// What a stroke does once the modifiers have had their say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushAction {
    /// Draw Add.
    Add,
    /// Draw Subtract.
    Subtract,
    Grow,
    Erode,
    Smooth,
    Flatten(FlattenMode),
    Paint,
    Replace,
    SeaLevel(SeaLevelMode),
    Region(RegionMode),
}

/// The colour groups of the terrain tools (design section 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushFamily {
    /// Draw Add, Sculpt Grow.
    Build,
    /// Draw Subtract, Sculpt Erode.
    Cut,
    /// Smooth, Flatten.
    Shape,
    /// Paint, Replace.
    Surface,
    /// Sea Level.
    Water,
    /// Region.
    Region,
}

impl BrushAction {
    /// The name the readout shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Add => "Draw",
            Self::Subtract => "Subtract",
            Self::Grow => "Grow",
            Self::Erode => "Erode",
            Self::Smooth => "Smooth",
            Self::Flatten(_) => "Flatten",
            Self::Paint => "Paint",
            Self::Replace => "Replace",
            Self::SeaLevel(SeaLevelMode::Fill) => "Sea Level",
            Self::SeaLevel(SeaLevelMode::Evaporate) => "Evaporate",
            Self::Region(_) => "Region",
        }
    }

    /// The action's colour group.
    pub fn family(self) -> BrushFamily {
        match self {
            Self::Add | Self::Grow => BrushFamily::Build,
            Self::Subtract | Self::Erode => BrushFamily::Cut,
            Self::Smooth | Self::Flatten(_) => BrushFamily::Shape,
            Self::Paint | Self::Replace => BrushFamily::Surface,
            Self::SeaLevel(_) => BrushFamily::Water,
            Self::Region(_) => BrushFamily::Region,
        }
    }

    /// The dab mode the action strokes with, `None` when it does not stroke.
    pub fn brush_mode(self) -> Option<BrushMode> {
        Some(match self {
            Self::Add => BrushMode::VoxelAdd,
            Self::Subtract => BrushMode::VoxelRemove,
            Self::Grow => BrushMode::Raise,
            Self::Erode => BrushMode::Lower,
            Self::Smooth => BrushMode::Smooth,
            Self::Flatten(_) => BrushMode::Flatten,
            Self::Paint => BrushMode::PaintTexture,
            Self::Replace => BrushMode::Replace,
            Self::SeaLevel(_) | Self::Region(_) => return None,
        })
    }

    /// Whether the action adds or carves a 3D volume.
    pub fn is_volume(self) -> bool {
        matches!(self, Self::Add | Self::Subtract)
    }

    /// Whether the action strokes with a brush.
    pub fn is_brush(self) -> bool {
        self.brush_mode().is_some()
    }
}

/// What one dab does to the terrain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BrushMode {
    /// Raise the surface (Grow).
    #[default]
    Raise,
    /// Lower the surface (Erode).
    Lower,
    /// Relax the surface toward its neighbours.
    Smooth,
    /// Level the surface toward the dab's target.
    Flatten,
    /// Paint the dab's material into the material map.
    PaintTexture,
    /// Paint the dab's material only over its source material.
    Replace,
    /// Union the brush volume into the terrain.
    VoxelAdd,
    /// Carve the brush volume out, heightfield included.
    VoxelRemove,
    /// Round the edges of earlier 3D edits.
    VoxelSmooth,
}

impl BrushMode {
    /// Whether this mode edits the 3D terrain volume instead of the
    /// heightfield raster.
    pub fn is_volumetric(self) -> bool {
        matches!(self, Self::VoxelAdd | Self::VoxelRemove | Self::VoxelSmooth)
    }

    /// Whether this mode writes the material map rather than heights.
    pub fn is_paint(self) -> bool {
        matches!(self, Self::PaintTexture | Self::Replace)
    }
}

/// The weight (0 to 1) a dab of radius `radius` pushes with at `distance`
/// from its centre: 1 at the centre, fading along `1 - (d / r)^(1 /
/// falloff)` to 0 at the rim, and 0 past it. A hard edge (`falloff` 0)
/// pushes fully to the rim.
pub fn falloff_weight(distance: f32, radius: f32, falloff: f32) -> f32 {
    if !(radius > 0.0) {
        return 0.0;
    }
    let t = (distance / radius).max(0.0);
    if !(t <= 1.0) {
        return 0.0;
    }
    if falloff > 0.0 && t > 0.0 {
        (1.0 - t.powf(1.0 / falloff)).max(0.0)
    } else {
        1.0
    }
}

/// The parameters every dab of one stroke applies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushDab {
    pub mode: BrushMode,
    /// Radius, in metres (a box's half side).
    pub radius: f32,
    /// Full height of a Draw box or cylinder, in metres.
    pub height: f32,
    /// 0 to 1.
    pub strength: f32,
    /// Falloff exponent (see [`falloff_weight`]).
    pub falloff: f32,
    pub shape: BrushShape,
    /// The material Draw fills with and Paint and Replace lay down.
    pub material: u8,
    /// The material Replace paints over.
    pub source_material: u8,
    pub flatten_mode: FlattenMode,
    /// The visible height Flatten levels to, set at the stroke's press.
    pub flatten_target: Option<f32>,
    /// The locked plane: Draw keeps its volume on one side of it, Grow and
    /// Erode stop at it.
    pub plane: Option<f32>,
    pub pivot: BrushPivot,
    /// Metres a full-strength Grow or Erode dab moves the ground at the
    /// centre.
    pub rate: f32,
    /// Fraction of the way a full-strength Smooth or Flatten dab moves each
    /// cell.
    pub blend: f32,
}

impl BrushDab {
    /// A dab of `mode` with the Studio's default hardness on a sphere:
    /// Grow and Erode at [`SCULPT_RATE`], Smooth and Flatten at
    /// [`BLEND_RATE`].
    pub fn new(mode: BrushMode, radius: f32, strength: f32) -> Self {
        Self {
            mode,
            radius,
            height: radius * 2.0,
            strength,
            falloff: BrushHardness::default().falloff(),
            shape: BrushShape::Sphere,
            material: TerrainMaterial::Grass.to_u8(),
            source_material: TerrainMaterial::Grass.to_u8(),
            flatten_mode: FlattenMode::FlattenAll,
            flatten_target: None,
            plane: None,
            pivot: BrushPivot::Center,
            rate: SCULPT_RATE * radius,
            blend: BLEND_RATE,
        }
    }

    /// The dab's footprint around `center`.
    pub fn footprint(&self, center: Vec2) -> Footprint {
        Footprint { center, radius: self.radius, square: self.shape.footprint_square() }
    }

    /// Distance of `offset` from the centre in the footprint's own metric:
    /// Euclidean for a disc, Chebyshev for a square.
    pub fn footprint_distance(&self, offset: Vec2) -> f32 {
        if self.shape.footprint_square() {
            offset.x.abs().max(offset.y.abs())
        } else {
            offset.length()
        }
    }

    /// The weight this dab pushes with at `distance` (footprint metric).
    pub fn weight_at(&self, distance: f32) -> f32 {
        falloff_weight(distance, self.radius, self.falloff)
    }

    /// Half the height of the dab's volume: the radius for a sphere, half
    /// the height for a box or a cylinder.
    pub fn volume_half_height(&self) -> f32 {
        if self.shape.has_height() {
            self.height * 0.5
        } else {
            self.radius
        }
    }

    /// Where the volume's centre sits for a hit at `hit`, by the pivot.
    pub fn volume_center(&self, hit: Vec3) -> Vec3 {
        match self.pivot {
            BrushPivot::Bottom => hit + Vec3::Y * self.volume_half_height(),
            BrushPivot::Center => hit,
            BrushPivot::Top => hit - Vec3::Y * self.volume_half_height(),
        }
    }

    /// The CSG shape a Draw dab at `hit` adds or carves (the ball Smooth
    /// rounds, for [`BrushMode::VoxelSmooth`]).
    pub fn volume_shape(&self, hit: Vec3) -> CsgShape {
        if self.mode == BrushMode::VoxelSmooth {
            return CsgShape::Sphere { center: hit, radius: self.radius };
        }
        let center = self.volume_center(hit);
        let half_height = self.volume_half_height();
        match self.shape {
            BrushShape::Sphere => CsgShape::Sphere { center, radius: self.radius },
            BrushShape::Box => {
                CsgShape::AxisBox { center, half_extents: Vec3::new(self.radius, half_height, self.radius) }
            }
            BrushShape::Cylinder => CsgShape::Cylinder { center, radius: self.radius, half_height },
        }
    }

    /// The side of the locked plane a Draw dab keeps its volume on: Add
    /// fills up to the plane and never above it; Subtract carves from the
    /// plane up.
    pub fn clip(&self) -> Option<ClipPlane> {
        let y = self.plane?;
        match self.mode {
            BrushMode::VoxelAdd => Some(ClipPlane { y, keep_below: true }),
            BrushMode::VoxelRemove => Some(ClipPlane { y, keep_below: false }),
            _ => None,
        }
    }
}

// ============================================================================
// Where the brush is
// ============================================================================

/// Farthest a cursor ray reaches the terrain or the locked plane, in metres.
pub const BRUSH_RAY_REACH: f32 = 2000.0;

/// Where `ray` crosses the horizontal plane at height `y`, `None` when it
/// runs along it, points away from it or crosses it past
/// [`BRUSH_RAY_REACH`].
pub fn ray_plane_hit(ray: Ray3d, y: f32) -> Option<Vec3> {
    let direction = *ray.direction;
    if direction.y.abs() < 1e-6 {
        return None;
    }
    let t = (y - ray.origin.y) / direction.y;
    (t > 0.0 && t <= BRUSH_RAY_REACH).then(|| ray.origin + direction * t)
}

/// `value` snapped to the nearest multiple of `step`.
fn snap_to(value: f32, step: f32) -> f32 {
    (value / step).round() * step
}

/// The brush centre for a cursor `ray` (design sections 4.2, 4.6, 4.7): its
/// crossing of the locked `plane`, else `surface_hit`, then snapped to
/// `snap` in X and Z, and in Y too when `snap_y` (a volume brush off the
/// plane). `None` when the cursor is off both.
pub fn brush_target(
    ray: Ray3d,
    surface_hit: Option<Vec3>,
    plane: Option<f32>,
    snap: Option<f32>,
    snap_y: bool,
) -> Option<Vec3> {
    let point = match plane {
        Some(y) => ray_plane_hit(ray, y)?,
        None => surface_hit?,
    };
    Some(match snap {
        Some(step) if step > 0.0 => Vec3::new(
            snap_to(point.x, step),
            if snap_y && plane.is_none() { snap_to(point.y, step) } else { point.y },
            snap_to(point.z, step),
        ),
        _ => point,
    })
}

/// The XZ centres a dab at `center` lands on with mirror `axes` through
/// `origin`: `center` first, then its reflections, without repeats.
pub fn mirror_centres(center: Vec2, origin: Vec2, axes: MirrorAxes) -> Vec<Vec2> {
    let mut centres = vec![center];
    let across_x = |p: Vec2| Vec2::new(2.0 * origin.x - p.x, p.y);
    let across_z = |p: Vec2| Vec2::new(p.x, 2.0 * origin.y - p.y);
    if axes.mirror_x() {
        centres.push(across_x(center));
    }
    if axes.mirror_z() {
        centres.push(across_z(center));
    }
    if axes.mirror_x() && axes.mirror_z() {
        centres.push(across_z(across_x(center)));
    }
    let mut unique: Vec<Vec2> = Vec::with_capacity(centres.len());
    for c in centres {
        if !unique.iter().any(|u| u.distance_squared(c) < 1e-8) {
            unique.push(c);
        }
    }
    unique
}

/// The dabs a moving stroke lays this frame, from the last dab toward
/// `target`, `spacing` apart along the path (design section 4.2). The first
/// dab of a stroke lands on `target`; a gap shorter than `spacing` lays
/// none; a gap longer than [`MAX_DABS_PER_FRAME`] spacings is a jump, and
/// the stroke starts afresh at `target`.
pub fn dab_path(last: Option<Vec3>, target: Vec3, spacing: f32) -> Vec<Vec3> {
    let Some(last) = last else {
        return vec![target];
    };
    let spacing = spacing.max(1e-3);
    let gap = last.distance(target);
    if !gap.is_finite() || gap > spacing * MAX_DABS_PER_FRAME as f32 {
        return vec![target];
    }
    if gap < spacing {
        return Vec::new();
    }
    let steps = (gap / spacing).floor() as usize;
    (1..=steps).map(|i| last.lerp(target, i as f32 * spacing / gap)).collect()
}

/// Where the brush centre sits when stroke smoothing trails `target` on a
/// leash of `leash` metres from `previous`: it stays put until the cursor
/// pulls the leash taut, then is dragged along behind it.
pub fn trail(previous: Option<Vec3>, target: Vec3, leash: f32) -> Vec3 {
    match previous {
        Some(previous) if leash > 0.0 => {
            let pull = target - previous;
            let length = pull.length();
            if length > leash {
                target - pull / length * leash
            } else {
                previous
            }
        }
        _ => target,
    }
}

/// Seconds between the dabs of a resting Draw stroke at full strength...
const VOXEL_DAB_INTERVAL_MIN_SECS: f64 = 0.03;
/// ...and at zero strength. A dab lands at the ray's hit on the surface the
/// previous dab made, so a held Add grows toward the camera and a held
/// Subtract digs along the view ray; strength sets how fast. On a locked
/// plane the hit stays put and repeated dabs change nothing.
const VOXEL_DAB_INTERVAL_MAX_SECS: f64 = 0.25;

/// Seconds between the dabs of a resting Draw stroke at `strength` (0 to 1).
pub fn voxel_dab_interval(strength: f32) -> f64 {
    let t = if strength.is_finite() { strength.clamp(0.0, 1.0) as f64 } else { 0.0 };
    VOXEL_DAB_INTERVAL_MAX_SECS + (VOXEL_DAB_INTERVAL_MIN_SECS - VOXEL_DAB_INTERVAL_MAX_SECS) * t
}

// ============================================================================
// Dabs
// ============================================================================

/// The strongest slot of a material cell, `None` for a cell with none.
fn primary_slot(cell: MaterialCell) -> Option<u8> {
    material_cell_weights(cell)
        .into_iter()
        .filter(|(slot, weight)| *slot != MATERIAL_SLOT_NONE && *weight > 0.0)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(slot, _)| slot)
}

/// Apply one surface dab (Grow, Erode, Smooth, Flatten, Paint, Replace)
/// centred on each of `centres` (world XZ) to the base raster `data`.
/// Returns the world XZ box the footprints cover, which the caller marks in
/// [`TerrainDirtyChunks`], or `None` when it misses the raster.
///
/// Every raster cell under the footprints takes the dab once, weighted by
/// the strongest footprint over it, so mirrored footprints that overlap push
/// once there. Holes of a sparse surface are left alone: a height or a
/// material would give them ground.
///
/// `baked` is the root's layer bake, when it has one. Flatten's target and
/// the locked plane are heights of the ground the user sees; each cell
/// measures them against its own visible height (base plus the layers'
/// offset there), so over a Noise layer or a stamp the visible ground levels,
/// not the base. `recorder` is told about every tile under the footprints
/// before anything is written.
pub fn apply_surface_dab(
    dab: &BrushDab,
    centres: &[Vec2],
    config: &TerrainConfig,
    data: &mut TerrainData,
    baked: Option<&TerrainBaked>,
    mut recorder: Option<&mut TerrainEditRecorder>,
) -> Option<(Vec2, Vec2)> {
    if dab.mode.is_volumetric() || !(dab.radius > 0.0) || !(dab.strength > 0.0) || centres.is_empty() {
        return None;
    }
    if dab.mode.is_paint() && dab.material == MATERIAL_SLOT_NONE {
        return None;
    }
    let grid = CellGrid::of(config, data)?;
    let reach = Vec2::splat(dab.radius);
    let lo = centres.iter().fold(Vec2::splat(f32::INFINITY), |acc, c| acc.min(*c - reach));
    let hi = centres.iter().fold(Vec2::splat(f32::NEG_INFINITY), |acc, c| acc.max(*c + reach));
    let (x0, z0, x1, z1) = grid.cells_in(lo, hi)?;

    if let Some(recorder) = recorder.as_deref_mut() {
        recorder.record_world_rect(config, data, grid.world(x0, z0), grid.world(x1, z1));
    }
    if dab.mode.is_paint() {
        ensure_material_cache(data);
    }

    let width = grid.width;
    // The visible ground's height over the base, per cell of the box: the
    // layer bake minus the base, when there is a bake laid out like the base.
    let span = x1 - x0 + 1;
    let offsets: Option<Vec<f32>> = baked
        .map(|baked| surface_data(data, Some(baked)))
        .filter(|surface| !std::ptr::eq(*surface, &*data))
        .map(|surface| {
            let mut offsets = Vec::with_capacity(span * (z1 - z0 + 1));
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let i = z * width + x;
                    offsets.push(config.world_height(surface.height_cache[i]) - config.world_height(data.height_cache[i]));
                }
            }
            offsets
        });
    let offset_at = |x: usize, z: usize| offsets.as_ref().map_or(0.0, |o| o[(z - z0) * span + (x - x0)]);

    // Smooth reads a copy of the footprint and a one-cell margin, so the
    // order the cells are written in does not matter.
    let (sx0, sz0) = (x0.saturating_sub(1), z0.saturating_sub(1));
    let (sx1, sz1) = ((x1 + 1).min(grid.width - 1), (z1 + 1).min(grid.height - 1));
    let snap_width = sx1 - sx0 + 1;
    let snapshot: Vec<f32> = if dab.mode == BrushMode::Smooth {
        let mut copy = Vec::with_capacity(snap_width * (sz1 - sz0 + 1));
        for z in sz0..=sz1 {
            copy.extend_from_slice(&data.height_cache[z * width + sx0..=z * width + sx1]);
        }
        copy
    } else {
        Vec::new()
    };
    let snapshot_at = |x: usize, z: usize| snapshot[(z - sz0) * snap_width + (x - sx0)];

    for z in z0..=z1 {
        for x in x0..=x1 {
            let p = grid.world(x, z);
            let weight = centres
                .iter()
                .map(|c| dab.weight_at(dab.footprint_distance(p - *c)))
                .fold(0.0_f32, f32::max);
            if !(weight > 0.0) {
                continue;
            }
            let index = z * width + x;
            if data.cell_is_hole(index) {
                continue;
            }
            let push = (dab.strength * weight).min(1.0);
            if dab.mode.is_paint() {
                let Some(current) = data.material_cache.get(index).copied() else { continue };
                if dab.mode == BrushMode::Replace && primary_slot(current) != Some(dab.source_material) {
                    continue;
                }
                let painted = paint_material_cell(current, dab.material, push);
                if painted != current {
                    data.material_cache[index] = painted;
                    data.material_dirty = true;
                }
                continue;
            }

            let base = config.world_height(data.height_cache[index]);
            let offset = offset_at(x, z);
            let visible = base + offset;
            let new_visible = match dab.mode {
                BrushMode::Raise => {
                    let raised = visible + dab.rate * push;
                    match dab.plane {
                        Some(y) if visible >= y => continue,
                        Some(y) => raised.min(y),
                        None => raised,
                    }
                }
                BrushMode::Lower => {
                    let lowered = visible - dab.rate * push;
                    match dab.plane {
                        Some(y) if visible <= y => continue,
                        Some(y) => lowered.max(y),
                        None => lowered,
                    }
                }
                BrushMode::Smooth => {
                    let mut sum = 0.0;
                    let mut count = 0.0;
                    for nz in z.saturating_sub(1)..=(z + 1).min(grid.height - 1) {
                        for nx in x.saturating_sub(1)..=(x + 1).min(grid.width - 1) {
                            sum += config.world_height(snapshot_at(nx, nz));
                            count += 1.0;
                        }
                    }
                    let own = config.world_height(snapshot_at(x, z));
                    own + (sum / count - own) * (dab.blend * push).min(1.0) + offset
                }
                BrushMode::Flatten => {
                    let Some(target) = dab.flatten_target else { continue };
                    let diff = target - visible;
                    let allowed = match dab.flatten_mode {
                        FlattenMode::ErodeToFlat => diff < 0.0,
                        FlattenMode::GrowToFlat => diff > 0.0,
                        FlattenMode::FlattenAll => true,
                    };
                    if !allowed {
                        continue;
                    }
                    visible + diff * (dab.blend * push).min(1.0)
                }
                _ => continue,
            };
            let new_base = config.clamp_to_saved_band(new_visible - offset);
            data.height_cache[index] = config.normalized_height(new_base);
        }
    }
    Some((lo, hi))
}

/// Apply one Draw dab (Add or Subtract), or a 3D Smooth, at each of `hits`
/// to `volume`: the dab's shape (see [`BrushDab::volume_shape`]) placed on
/// each hit by the pivot, clipped to the locked plane. Add fills with the
/// dab's material; Subtract leaves Rock on the walls it exposes.
///
/// `recorder` is told about every brick a shape can write before the write,
/// over the same box the CSG op visits, so an undo puts back exactly the
/// bricks from before the stroke. The returned edit goes to
/// `TerrainDirtyChunks::mark_volume_edit`.
pub fn apply_draw_dab(
    dab: &BrushDab,
    hits: &[Vec3],
    config: &TerrainConfig,
    volume: &mut TerrainVolume,
    mut recorder: Option<&mut TerrainEditRecorder>,
) -> VolumeEdit {
    let mut changed = VolumeEdit::default();
    for hit in hits {
        let shape = dab.volume_shape(*hit);
        match dab.mode {
            BrushMode::VoxelAdd | BrushMode::VoxelRemove => {
                let (lo, hi) = shape.edit_bounds(config);
                if let Some(recorder) = recorder.as_deref_mut() {
                    recorder.record_volume_aabb(config, volume, lo, hi);
                }
                let (op, material) = if dab.mode == BrushMode::VoxelAdd {
                    (CsgOp::Add, dab.material)
                } else {
                    (CsgOp::Carve, TerrainMaterial::Rock.to_u8())
                };
                changed.merge(&apply_shape_clipped(config, volume, shape, op, material, dab.clip()));
            }
            BrushMode::VoxelSmooth => {
                if volume.is_empty() {
                    continue;
                }
                // `apply_smooth` visits the lattice points of the ball's box.
                let (lo, hi) = shape.bounds();
                if let Some(recorder) = recorder.as_deref_mut() {
                    recorder.record_volume_aabb(config, volume, lo, hi);
                }
                changed.merge(&apply_smooth(config, volume, *hit, dab.radius, dab.strength));
            }
            _ => {}
        }
    }
    changed
}

/// One heightfield dab of `dab` at `hit_point`, for callers that dab one
/// point (the terrain API): [`apply_surface_dab`] over the one centre.
/// Returns the world XZ box the footprint covers.
pub fn apply_heightfield_brush(
    dab: &BrushDab,
    hit_point: Vec3,
    config: &TerrainConfig,
    data: &mut TerrainData,
    baked: Option<&TerrainBaked>,
    recorder: Option<&mut TerrainEditRecorder>,
) -> (Vec2, Vec2) {
    let center = hit_point.xz();
    let reach = Vec2::splat(dab.radius.max(0.0));
    apply_surface_dab(dab, &[center], config, data, baked, recorder);
    (center - reach, center + reach)
}

// ============================================================================
// Systems
// ============================================================================

/// Host-app veto on terrain painting, checked by [`update_brush_hover`] and
/// [`terrain_paint_system`].
///
/// The brush reads the raw cursor, so on its own it happily sculpts while
/// the pointer is over ribbon buttons or a docked panel. Hosts with editor
/// chrome (the Studio engine) insert this resource and set `allowed` each
/// frame from their viewport bounds and UI focus. Hosts without chrome (the
/// Client) never insert it: an absent resource means "no veto".
#[derive(Resource, Debug, Clone, Copy)]
pub struct TerrainPaintGate {
    /// `false` = the pointer is over UI chrome this frame, or the host holds
    /// the mouse for something else (a brush-size gesture): no stroke starts
    /// and an open one lays no dabs.
    pub allowed: bool,
}

impl Default for TerrainPaintGate {
    fn default() -> Self {
        Self { allowed: true }
    }
}

/// Where the brush is and what a press would do, worked out each frame by
/// [`update_brush_hover`] for the stroke and the host's cursor.
#[derive(Resource, Debug, Default, Clone)]
pub struct TerrainBrushHover {
    /// The cursor ray, `None` off the window or without a scene camera.
    pub ray: Option<Ray3d>,
    /// The cursor's hit on the terrain surface, `None` off the terrain.
    pub surface: Option<Vec3>,
    /// The brush centre: on the locked plane when there is one, snapped
    /// when snap is on. `None` when the cursor is off both, or over chrome.
    pub target: Option<Vec3>,
    /// What a press would do with the modifiers held now, or what the open
    /// stroke does.
    pub action: Option<BrushAction>,
}

impl TerrainBrushHover {
    /// Same hit, target and action as `other`, the ray aside. The ray moves
    /// with the camera even when nothing a reader draws does.
    fn same_as(&self, other: &Self) -> bool {
        self.surface == other.surface && self.target == other.target && self.action == other.action
    }
}

/// The open stroke, between its press and its release.
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveStroke {
    /// What the stroke does, fixed at its press.
    pub action: BrushAction,
    /// The dab every step of the stroke applies, fixed at its press.
    pub dab: BrushDab,
    /// The brush centre after stroke smoothing.
    pub centre: Option<Vec3>,
    /// Where the latest dab landed.
    pub last_dab: Option<Vec3>,
    /// When the latest dab landed (real seconds).
    pub last_dab_secs: f64,
}

/// The terrain stroke in progress, if any.
#[derive(Resource, Debug, Default, Clone)]
pub struct TerrainStroke {
    pub active: Option<ActiveStroke>,
}

/// Whether either key of a modifier pair is held.
fn held(keys: &ButtonInput<KeyCode>, left: KeyCode, right: KeyCode) -> bool {
    keys.pressed(left) || keys.pressed(right)
}

/// Work out [`TerrainBrushHover`]: the cursor ray from the scene camera,
/// its hit on the terrain, the brush centre and the action a press would
/// take (the open stroke's, while one is open).
#[allow(clippy::too_many_arguments)]
pub fn update_brush_hover(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    terrain_query: Query<
        (&TerrainConfig, &TerrainData, Option<&TerrainVolume>, Option<&TerrainBaked>),
        With<TerrainRoot>,
    >,
    brush: Res<TerrainBrush>,
    stroke: Res<TerrainStroke>,
    gate: Option<Res<TerrainPaintGate>>,
    mut hover: ResMut<TerrainBrushHover>,
) {
    let ctrl = held(&keys, KeyCode::ControlLeft, KeyCode::ControlRight);
    let shift = held(&keys, KeyCode::ShiftLeft, KeyCode::ShiftRight);
    let action = stroke.active.as_ref().map_or_else(|| brush.action(ctrl, shift), |open| open.action);
    let mut next = TerrainBrushHover { action: Some(action), ..TerrainBrushHover::default() };

    // The scene camera, NOT `single()`: the Studio runs several Camera3d
    // entities (scene camera at order 0, the Slint chrome overlay, the AI
    // camera). `order == 0` is the engine-wide "camera the user looks
    // through"; a single-camera host matches it too.
    next.ray = windows.single().ok().and_then(|window| {
        let cursor = window.cursor_position()?;
        let (camera, transform) = camera_query.iter().find(|(camera, _)| camera.order == 0)?;
        camera.viewport_to_world(transform, cursor).ok()
    });
    let over_chrome = !gate.map_or(true, |gate| gate.allowed);
    if let (Some(ray), Ok((config, data, volume, baked))) = (next.ray, terrain_query.single()) {
        next.surface =
            raycast_terrain_surface(config, surface_data(data, baked), volume, ray, BRUSH_RAY_REACH, 2.0);
        if action.is_brush() && !over_chrome {
            next.target = brush_target(ray, next.surface, brush.plane_lock, brush.active_snap(), action.is_volume());
        }
    }
    // Only a change in what readers draw marks the resource changed.
    if !hover.same_as(&next) {
        *hover = next;
    } else {
        hover.bypass_change_detection().ray = next.ray;
    }
}

/// Apply the terrain stroke under the cursor (design section 4.2).
///
/// A stroke opens on a left press with the pointer over the viewport
/// ([`TerrainPaintGate`]) and `Alt` up (`Alt` with the left button is the
/// camera's orbit), at a point [`TerrainBrushHover`] found. It fixes its
/// action and dab there: Flatten's target is the locked plane or the height
/// under the press; Auto material samples the ground there. It stays open
/// until the button comes up, laying no dabs while the pointer is over
/// chrome.
///
/// While open, the brush centre trails the hover target by the stroke
/// smoothing leash, and dabs land every [`DAB_SPACING`] radii along its
/// path ([`dab_path`]); resting, surface actions dab every
/// [`SURFACE_DAB_INTERVAL_SECS`] and Draw every [`voxel_dab_interval`]. Each
/// dab also lands at its mirrored centres. Surface actions write the height
/// raster or material map ([`apply_surface_dab`]); Draw writes the volume
/// ([`apply_draw_dab`]); Smooth also rounds 3D edits inside its ball.
///
/// When the host inserts a [`TerrainEditRecorder`] (the Studio engine does,
/// the Client does not), the stroke opens an edit on its first write and
/// records every tile and brick before writing into it; the host closes the
/// edit when the button comes up and pushes it onto its undo stack.
#[allow(clippy::too_many_arguments)]
pub fn terrain_paint_system(
    mut commands: Commands,
    real_time: Res<Time<Real>>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut terrain_query: Query<
        (Entity, &TerrainConfig, &mut TerrainData, Option<&mut TerrainVolume>, Option<&TerrainBaked>),
        With<TerrainRoot>,
    >,
    brush: Res<TerrainBrush>,
    hover: Res<TerrainBrushHover>,
    gate: Option<Res<TerrainPaintGate>>,
    mut stroke: ResMut<TerrainStroke>,
    mut dirty: ResMut<TerrainDirtyChunks>,
    mut recorder: Option<ResMut<TerrainEditRecorder>>,
) {
    if !buttons.pressed(MouseButton::Left) {
        if stroke.active.is_some() {
            stroke.active = None;
        }
        return;
    }
    let allowed = gate.map_or(true, |gate| gate.allowed);
    let now = real_time.elapsed_secs_f64();

    if buttons.just_pressed(MouseButton::Left) {
        stroke.active = None;
        let alt = held(&keys, KeyCode::AltLeft, KeyCode::AltRight);
        let (Some(action), Some(target)) = (hover.action, hover.target) else { return };
        if alt || !allowed {
            return;
        }
        let Some(mut dab) = brush.dab(action) else { return };
        let Ok((_, config, data, _, baked)) = terrain_query.single() else { return };
        if let BrushAction::Flatten(_) = action {
            dab.flatten_target = Some(brush.plane_lock.unwrap_or(target.y));
        }
        if action == BrushAction::Add && brush.auto_material {
            if let Some(hit) = hover.surface {
                if let Some(sample) = material_at_world(config, surface_data(data, baked), hit.x, hit.z) {
                    dab.material = sample.primary;
                }
            }
        }
        stroke.active =
            Some(ActiveStroke { action, dab, centre: None, last_dab: None, last_dab_secs: f64::NEG_INFINITY });
    }
    let Some(active) = stroke.active.as_mut() else { return };
    let (action, dab) = (active.action, active.dab);

    // Every held frame of a 3D stroke counts as the stroke being live, the
    // ones between throttled dabs and the ones whose ray misses included:
    // the trimesh colliders stay deferred until the button is released.
    if action.is_volume() {
        dirty.hold_colliders();
    }
    if !allowed {
        return;
    }
    let Some(target) = hover.target else { return };

    let centre = trail(active.centre, target, brush.smoothing.clamp(0.0, 1.0) * dab.radius);
    active.centre = Some(centre);
    let mut centres = dab_path(active.last_dab, centre, dab.radius * DAB_SPACING);
    let interval = if action.is_volume() { voxel_dab_interval(dab.strength) } else { SURFACE_DAB_INTERVAL_SECS };
    if centres.is_empty() && now - active.last_dab_secs >= interval {
        centres.push(centre);
    }
    let Some(&last) = centres.last() else { return };
    active.last_dab = Some(last);
    active.last_dab_secs = now;

    let Ok((root, config, mut data, volume, baked)) = terrain_query.single_mut() else { return };
    // Procedural terrain has no raster to write into, and the marching-cubes
    // mesher refuses a terrain without one, so a volume edit there would
    // never be drawn either.
    if data.height_cache.is_empty() {
        return;
    }
    let mut volume = match volume {
        Some(volume) => Some(volume),
        None => {
            // Every spawn path gives the root a volume; one that arrived
            // without it gets an empty one now and takes Draw dabs from the
            // next frame, once the insert has landed.
            commands.entity(root).try_insert(TerrainVolume::default());
            if action.is_volume() {
                return;
            }
            None
        }
    };

    if let Some(recorder) = recorder.as_deref_mut() {
        if !recorder.is_recording() {
            recorder.begin(terrain_stroke_label(dab.mode), Some(root), config, &data);
        }
    }

    // The ground height at a mirrored centre: the locked plane, else the
    // visible surface there.
    let ground_at = |data: &TerrainData, p: Vec2| {
        dab.plane.unwrap_or_else(|| height_at_world(config, surface_data(data, baked), p.x, p.y))
    };
    for c in centres {
        let points = mirror_centres(c.xz(), brush.mirror_origin, brush.mirror);
        if action.is_volume() {
            let Some(volume) = volume.as_deref_mut() else { return };
            let hits: Vec<Vec3> = points
                .iter()
                .enumerate()
                .map(|(i, p)| if i == 0 { c } else { Vec3::new(p.x, ground_at(&data, *p), p.y) })
                .collect();
            let edit = apply_draw_dab(&dab, &hits, config, volume, recorder.as_deref_mut());
            dirty.mark_volume_edit(config, &edit);
            continue;
        }

        let Some((min_xz, max_xz)) =
            apply_surface_dab(&dab, &points, config, &mut data, baked, recorder.as_deref_mut())
        else {
            continue;
        };
        // A paint stroke moves no height, so it leaves the height counter the
        // water's height texture re-uploads on.
        if dab.mode.is_paint() {
            dirty.mark_world_rect_materials(config, min_xz, max_xz);
        } else {
            dirty.mark_world_rect(config, min_xz, max_xz);
        }
        // Smooth also rounds the 3D edits inside its ball.
        if dab.mode == BrushMode::Smooth {
            if let Some(volume) = volume.as_deref_mut().filter(|volume| !volume.is_empty()) {
                let ball = BrushDab { mode: BrushMode::VoxelSmooth, ..dab };
                let hits: Vec<Vec3> = points.iter().map(|p| Vec3::new(p.x, ground_at(&data, *p), p.y)).collect();
                let edit = apply_draw_dab(&ball, &hits, config, volume, recorder.as_deref_mut());
                dirty.mark_volume_edit(config, &edit);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{apply_terrain_bricks, apply_terrain_tiles, sample_field_parts, TerrainTileSide};

    fn test_config() -> TerrainConfig {
        TerrainConfig { chunk_size: 16.0, chunk_resolution: 8, chunks_x: 1, chunks_z: 1, ..TerrainConfig::default() }
    }

    fn flat_data(config: &TerrainConfig, world_y: f32) -> TerrainData {
        let mut data = TerrainData::procedural();
        data.resize_cache(config);
        let h = config.normalized_height(world_y);
        data.height_cache.iter_mut().for_each(|v| *v = h);
        data
    }

    #[test]
    fn modifiers_swap_modes_and_shift_smooths() {
        let mut brush = TerrainBrush { tool: TerrainTool::Draw, ..TerrainBrush::default() };
        assert_eq!(brush.action(false, false), BrushAction::Add);
        assert_eq!(brush.action(true, false), BrushAction::Subtract);
        assert_eq!(brush.action(false, true), BrushAction::Smooth);
        brush.draw_mode = DrawMode::Subtract;
        assert_eq!(brush.action(false, false), BrushAction::Subtract);
        assert_eq!(brush.action(true, false), BrushAction::Add);

        brush.tool = TerrainTool::Sculpt;
        assert_eq!(brush.action(false, false), BrushAction::Grow);
        assert_eq!(brush.action(true, false), BrushAction::Erode);

        brush.tool = TerrainTool::Flatten;
        brush.flatten_mode = FlattenMode::ErodeToFlat;
        assert_eq!(brush.action(true, false), BrushAction::Flatten(FlattenMode::GrowToFlat));
        brush.flatten_mode = FlattenMode::FlattenAll;
        assert_eq!(brush.action(true, false), BrushAction::Flatten(FlattenMode::FlattenAll));

        brush.tool = TerrainTool::Paint;
        assert_eq!(brush.action(true, false), BrushAction::Paint, "Ctrl leaves Paint alone");
        // Shift never smooths from a tool that does not stroke.
        brush.tool = TerrainTool::Region;
        assert_eq!(brush.action(false, true), BrushAction::Region(RegionMode::Select));
        assert!(brush.dab(BrushAction::Region(RegionMode::Select)).is_none());
        assert_eq!(BrushAction::Grow.family(), BrushFamily::Build);
        assert_eq!(BrushAction::Subtract.family(), BrushFamily::Cut);
    }

    #[test]
    fn size_and_strength_are_kept_per_family_and_per_tool() {
        let mut brush = TerrainBrush::default();
        brush.tool = TerrainTool::Sculpt;
        brush.set_size(30.0);
        brush.set_strength(0.9);
        brush.tool = TerrainTool::Paint;
        assert_eq!(brush.size(), 30.0, "Paint shares the surface size");
        assert_ne!(brush.strength(), 0.9, "but keeps its own strength");
        brush.tool = TerrainTool::Draw;
        assert_eq!(brush.size(), 8.0, "Draw keeps the volume size");
        brush.set_size(1000.0);
        assert_eq!(brush.size(), BRUSH_SIZE_MAX);
        brush.set_size(f32::NAN);
        assert_eq!(brush.size(), BRUSH_SIZE_MIN);
        brush.set_strength(0.95);
        brush.step_strength(true);
        assert_eq!(brush.strength(), 1.0);
        brush.step_strength(false);
        assert!((brush.strength() - 0.9).abs() < 1e-6);
        brush.set_size(8.0);
        brush.step_size(true);
        assert!((brush.size() - 10.0).abs() < 1e-5);
    }

    #[test]
    fn mirror_and_snap_steps_cycle() {
        let mut brush = TerrainBrush::default();
        brush.toggle_mirror();
        assert_eq!(brush.mirror, MirrorAxes::X);
        brush.next_mirror_axes();
        assert_eq!(brush.mirror, MirrorAxes::Z);
        brush.toggle_mirror();
        assert_eq!(brush.mirror, MirrorAxes::Off);
        brush.toggle_mirror();
        assert_eq!(brush.mirror, MirrorAxes::Z, "M brings back the last axes");
        brush.snap_step = 8.0;
        brush.next_snap_step();
        assert_eq!(brush.snap_step, 0.25);
        assert_eq!(brush.active_snap(), None);
        brush.snap = true;
        assert_eq!(brush.active_snap(), Some(0.25));
    }

    #[test]
    fn the_target_follows_the_plane_and_snaps() {
        let ray = Ray3d::new(Vec3::new(0.3, 50.0, 0.3), Dir3::new(Vec3::new(0.2, -1.0, 0.1)).unwrap());
        let surface = Some(Vec3::new(2.3, 7.4, 1.4));
        assert_eq!(brush_target(ray, surface, None, None, false), surface);
        let on_plane = brush_target(ray, surface, Some(20.0), None, false).unwrap();
        assert!((on_plane.y - 20.0).abs() < 1e-4, "the locked plane wins over the ground");
        let snapped = brush_target(ray, surface, None, Some(1.0), true).unwrap();
        assert_eq!(snapped, Vec3::new(2.0, 7.0, 1.0));
        let snapped_xz = brush_target(ray, surface, None, Some(1.0), false).unwrap();
        assert_eq!(snapped_xz.y, 7.4, "surface brushes snap only X and Z");
        assert_eq!(brush_target(ray, None, None, None, false), None);
        // A ray running along the plane never crosses it.
        let level = Ray3d::new(Vec3::new(0.0, 5.0, 0.0), Dir3::X);
        assert_eq!(ray_plane_hit(level, 5.0), None);
    }

    #[test]
    fn mirrored_centres_reflect_without_repeats() {
        let origin = Vec2::new(10.0, 0.0);
        assert_eq!(mirror_centres(Vec2::new(4.0, 3.0), origin, MirrorAxes::Off), vec![Vec2::new(4.0, 3.0)]);
        assert_eq!(
            mirror_centres(Vec2::new(4.0, 3.0), origin, MirrorAxes::X),
            vec![Vec2::new(4.0, 3.0), Vec2::new(16.0, 3.0)]
        );
        assert_eq!(mirror_centres(Vec2::new(4.0, 3.0), origin, MirrorAxes::XZ).len(), 4);
        // On the mirror plane the reflection is the point itself.
        assert_eq!(mirror_centres(Vec2::new(10.0, 3.0), origin, MirrorAxes::X).len(), 1);
    }

    #[test]
    fn dabs_are_spaced_along_the_path_and_a_jump_restarts() {
        let start = Vec3::ZERO;
        assert_eq!(dab_path(None, start, 1.0), vec![start]);
        assert!(dab_path(Some(start), Vec3::X * 0.5, 1.0).is_empty(), "under one spacing lays nothing");
        let dabs = dab_path(Some(start), Vec3::X * 3.5, 1.0);
        assert_eq!(dabs.len(), 3, "evenly spaced, the rest carried over");
        for (dab, expected) in dabs.iter().zip([1.0, 2.0, 3.0]) {
            assert!((*dab - Vec3::X * expected).length() < 1e-5, "{dab} vs {expected}");
        }
        let far = Vec3::X * 100.0;
        assert_eq!(dab_path(Some(start), far, 1.0), vec![far], "a jump starts afresh");
    }

    #[test]
    fn smoothing_trails_the_cursor_on_a_leash() {
        assert_eq!(trail(None, Vec3::X, 2.0), Vec3::X);
        assert_eq!(trail(Some(Vec3::ZERO), Vec3::X, 2.0), Vec3::ZERO, "inside the leash the centre stays");
        let dragged = trail(Some(Vec3::ZERO), Vec3::X * 5.0, 2.0);
        assert!((dragged - Vec3::X * 3.0).length() < 1e-5, "a taut leash drags it to within 2 m");
        assert_eq!(trail(Some(Vec3::ZERO), Vec3::X, 0.0), Vec3::X, "no smoothing, no trail");
    }

    #[test]
    fn a_grow_dab_raises_metres_not_the_band_and_takes_each_cell_once() {
        let config = TerrainConfig { height_scale: 128.0, ..test_config() };
        let mut data = flat_data(&config, 10.0);
        let grid = CellGrid::of(&config, &data).unwrap();
        let (x, z) = (grid.width / 2, grid.height / 2);
        let centre = grid.world(x, z);
        let dab = BrushDab::new(BrushMode::Raise, 4.0, 1.0);
        apply_surface_dab(&dab, &[centre], &config, &mut data, None, None).expect("on the raster");
        let raised = config.world_height(data.height_cache[z * grid.width + x]);
        let expected = 10.0 + SCULPT_RATE * 4.0;
        assert!((raised - expected).abs() < 1e-3, "one dab moves the centre {expected} m, got {raised}");
        let far = config.world_height(data.height_cache[(z + 4) * grid.width + x + 4]);
        assert!((far - 10.0).abs() < 1e-4, "ground outside the disc stays");
        // Overlapping mirrored footprints push once where they overlap.
        let mut mirrored = flat_data(&config, 10.0);
        apply_surface_dab(&dab, &[centre, centre], &config, &mut mirrored, None, None);
        assert_eq!(mirrored.height_cache, data.height_cache);
    }

    #[test]
    fn grow_and_erode_stop_at_the_locked_plane() {
        let config = test_config();
        let mut data = flat_data(&config, 10.0);
        let dab = BrushDab { plane: Some(10.1), rate: 5.0, ..BrushDab::new(BrushMode::Raise, 4.0, 1.0) };
        apply_surface_dab(&dab, &[Vec2::ZERO], &config, &mut data, None, None);
        assert!((height_at_world(&config, &data, 0.0, 0.0) - 10.1).abs() < 1e-3, "Grow stops at the plane");
        let erode = BrushDab { mode: BrushMode::Lower, plane: Some(9.5), ..dab };
        apply_surface_dab(&erode, &[Vec2::ZERO], &config, &mut data, None, None);
        assert!((height_at_world(&config, &data, 0.0, 0.0) - 9.5).abs() < 1e-3, "Erode stops at the plane");
    }

    #[test]
    fn flatten_levels_toward_its_target_within_its_mode() {
        let config = test_config();
        let mut data = flat_data(&config, 10.0);
        // A hard edge, so every cell near the centre takes the full blend.
        let dab = BrushDab {
            flatten_target: Some(12.0),
            blend: 1.0,
            falloff: 0.0,
            ..BrushDab::new(BrushMode::Flatten, 4.0, 1.0)
        };
        let erode_only = BrushDab { flatten_mode: FlattenMode::ErodeToFlat, ..dab };
        apply_surface_dab(&erode_only, &[Vec2::ZERO], &config, &mut data, None, None);
        assert!((height_at_world(&config, &data, 0.0, 0.0) - 10.0).abs() < 1e-4, "Erode to Flat never raises");
        apply_surface_dab(&dab, &[Vec2::ZERO], &config, &mut data, None, None);
        assert!((height_at_world(&config, &data, 0.0, 0.0) - 12.0).abs() < 1e-3, "Flatten All levels to the target");
    }

    #[test]
    fn smooth_relaxes_a_spike_and_leaves_flat_ground() {
        let config = test_config();
        let mut data = flat_data(&config, 10.0);
        let grid = CellGrid::of(&config, &data).unwrap();
        let (x, z) = (grid.width / 2, grid.height / 2);
        let centre = grid.world(x, z);
        data.height_cache[z * grid.width + x] = config.normalized_height(20.0);
        let dab = BrushDab::new(BrushMode::Smooth, 3.0, 1.0);
        apply_surface_dab(&dab, &[centre], &config, &mut data, None, None);
        let spike = config.world_height(data.height_cache[z * grid.width + x]);
        assert!(spike < 20.0 && spike > 10.0, "the spike sinks toward its neighbours: {spike}");
        let beside = config.world_height(data.height_cache[z * grid.width + x + 1]);
        assert!(beside > 10.0, "and its neighbours rise toward it, from the same copy: {beside}");
    }

    #[test]
    fn paint_and_replace_lay_material_and_undo_exactly() {
        let config = test_config();
        let mut data = flat_data(&config, 0.0);
        let original = data.clone();
        let basalt = TerrainMaterial::Basalt.to_u8();
        let sand = TerrainMaterial::Sand.to_u8();
        let paint = BrushDab { material: basalt, falloff: 0.0, ..BrushDab::new(BrushMode::PaintTexture, 3.0, 1.0) };

        let mut recorder = TerrainEditRecorder::default();
        assert!(recorder.begin(terrain_stroke_label(paint.mode), None, &config, &data));
        apply_surface_dab(&paint, &[Vec2::new(1.0, 1.0)], &config, &mut data, None, Some(&mut recorder));
        assert!(data.material_dirty);
        assert_eq!(material_at_world(&config, &data, 1.0, 1.0).map(|s| s.primary), Some(basalt));
        let far = material_at_world(&config, &data, 12.0, -12.0).expect("the stroke allocated the layer");
        assert_eq!(far.primary, TerrainMaterial::Grass.to_u8(), "ground outside the brush stays Grass");
        assert_eq!(data.height_cache, original.height_cache, "painting leaves heights alone");
        let edit = recorder.finish(None, &data).expect("the stroke painted");
        assert_eq!(edit.label, "Paint Terrain");
        let mut undone = data.clone();
        apply_terrain_tiles(&config, &mut undone, &edit.tiles, TerrainTileSide::Before).unwrap();
        assert!(undone.material_cache.is_empty(), "the stroke allocated the layer, so undo drops it");

        // Replace paints sand only over the basalt.
        let replace = BrushDab {
            material: sand,
            source_material: basalt,
            falloff: 0.0,
            ..BrushDab::new(BrushMode::Replace, 8.0, 1.0)
        };
        apply_surface_dab(&replace, &[Vec2::new(1.0, 1.0)], &config, &mut data, None, None);
        assert_eq!(material_at_world(&config, &data, 1.0, 1.0).map(|s| s.primary), Some(sand));
        assert_eq!(
            material_at_world(&config, &data, 5.0, 5.0).map(|s| s.primary),
            Some(TerrainMaterial::Grass.to_u8()),
            "grass under the brush (outside the basalt) is not the source, so it stays"
        );
    }

    #[test]
    fn a_grow_stroke_across_a_tile_border_records_both_tiles_before_writing() {
        let config = test_config();
        let mut data = TerrainData::procedural();
        data.resize_cache(&config);
        for (i, h) in data.height_cache.iter_mut().enumerate() {
            *h = (i % 7) as f32 * 0.01;
        }
        let original = data.clone();
        let dab = BrushDab { falloff: 0.0, ..BrushDab::new(BrushMode::Raise, 3.0, 1.0) };
        let mut recorder = TerrainEditRecorder::default();
        assert!(recorder.begin(terrain_stroke_label(dab.mode), None, &config, &data));
        apply_surface_dab(&dab, &[Vec2::new(0.7, 8.0)], &config, &mut data, None, Some(&mut recorder));
        let edit = recorder.finish(None, &data).expect("the stroke raised the ground");
        assert_eq!(edit.label, "Grow Terrain");
        assert!(edit.tiles.len() >= 2, "the footprint straddles a tile border");
        let mut undone = data.clone();
        apply_terrain_tiles(&config, &mut undone, &edit.tiles, TerrainTileSide::Before).unwrap();
        assert_eq!(undone.height_cache, original.height_cache);
        apply_terrain_tiles(&config, &mut undone, &edit.tiles, TerrainTileSide::After).unwrap();
        assert_eq!(undone.height_cache, data.height_cache);
    }

    #[test]
    fn volume_shapes_follow_the_brush_shape_and_pivot() {
        let hit = Vec3::new(1.0, 2.0, 3.0);
        let dab = |shape: BrushShape, pivot: BrushPivot| BrushDab {
            shape,
            pivot,
            height: 6.0,
            ..BrushDab::new(BrushMode::VoxelAdd, 4.0, 1.0)
        };
        assert_eq!(
            dab(BrushShape::Sphere, BrushPivot::Center).volume_shape(hit),
            CsgShape::Sphere { center: hit, radius: 4.0 }
        );
        assert_eq!(
            dab(BrushShape::Sphere, BrushPivot::Bottom).volume_shape(hit),
            CsgShape::Sphere { center: hit + Vec3::Y * 4.0, radius: 4.0 },
            "pivot Bottom sits the ball on the hit"
        );
        assert_eq!(
            dab(BrushShape::Box, BrushPivot::Top).volume_shape(hit),
            CsgShape::AxisBox { center: hit - Vec3::Y * 3.0, half_extents: Vec3::new(4.0, 3.0, 4.0) }
        );
        assert_eq!(
            dab(BrushShape::Cylinder, BrushPivot::Center).volume_shape(hit),
            CsgShape::Cylinder { center: hit, radius: 4.0, half_height: 3.0 }
        );
        let smooth = BrushDab { mode: BrushMode::VoxelSmooth, ..dab(BrushShape::Box, BrushPivot::Bottom) };
        assert_eq!(smooth.volume_shape(hit), CsgShape::Sphere { center: hit, radius: 4.0 }, "Smooth rounds a ball");
        assert_eq!(dab(BrushShape::Box, BrushPivot::Center).clip(), None);
        let planed = BrushDab { plane: Some(5.0), ..dab(BrushShape::Box, BrushPivot::Center) };
        assert_eq!(planed.clip(), Some(ClipPlane { y: 5.0, keep_below: true }));
        assert_eq!(
            BrushDab { mode: BrushMode::VoxelRemove, ..planed }.clip(),
            Some(ClipPlane { y: 5.0, keep_below: false })
        );
    }

    #[test]
    fn a_subtract_dab_digs_under_the_ground_and_undoes_exactly() {
        let config = test_config();
        let data = flat_data(&config, 10.0);
        let dab = BrushDab { shape: BrushShape::Box, height: 6.0, ..BrushDab::new(BrushMode::VoxelRemove, 3.0, 1.0) };
        let mut volume = TerrainVolume::new();
        let below = Vec3::new(1.0, 9.0, 1.0);
        assert!(sample_field_parts(&config, &data, &volume, below).is_solid(), "ground before the dab");

        let mut recorder = TerrainEditRecorder::default();
        assert!(recorder.begin(terrain_stroke_label(dab.mode), None, &config, &data));
        let edit = apply_draw_dab(&dab, &[Vec3::new(1.0, 10.0, 1.0)], &config, &mut volume, Some(&mut recorder));
        assert!(!edit.is_empty());
        assert!(!sample_field_parts(&config, &data, &volume, below).is_solid(), "the box is dug out");
        assert!(
            sample_field_parts(&config, &data, &volume, Vec3::new(12.0, 9.0, 12.0)).is_solid(),
            "ground well away from the box stays"
        );
        let recorded = recorder.finish_with_volume(None, &data, &volume).expect("the dab changed bricks");
        assert_eq!(recorded.label, "Subtract Terrain");
        let mut undone = volume.clone();
        apply_terrain_bricks(&config, &mut undone, &recorded.bricks, TerrainTileSide::Before).unwrap();
        assert!(undone.is_empty());
    }

    #[test]
    fn a_draw_add_on_a_locked_plane_never_builds_above_it() {
        // The test lattice is 2 m, so the slab kept below the plane is three
        // cells thick and each check sits a cell or more from its faces: a
        // one-cell slab puts both faces on lattice points, where the stored
        // distance is zero, and nothing between them can read solid.
        let config = test_config();
        let data = flat_data(&config, 0.0);
        let dab = BrushDab {
            shape: BrushShape::Box,
            height: 8.0,
            pivot: BrushPivot::Bottom,
            plane: Some(6.0),
            ..BrushDab::new(BrushMode::VoxelAdd, 2.0, 1.0)
        };
        let mut volume = TerrainVolume::new();
        apply_draw_dab(&dab, &[Vec3::ZERO], &config, &mut volume, None);
        assert!(sample_field_parts(&config, &data, &volume, Vec3::new(0.0, 3.0, 0.0)).is_solid(), "filled below the plane");
        assert!(!sample_field_parts(&config, &data, &volume, Vec3::new(0.0, 7.0, 0.0)).is_solid(), "nothing above it");
    }

    #[test]
    fn surface_modes_do_not_touch_the_volume() {
        let config = test_config();
        let dab = BrushDab::new(BrushMode::Raise, 3.0, 1.0);
        let mut volume = TerrainVolume::new();
        assert!(apply_draw_dab(&dab, &[Vec3::ZERO], &config, &mut volume, None).is_empty());
        assert!(volume.is_empty());
    }

    #[test]
    fn stronger_draw_strokes_dab_more_often() {
        assert!(voxel_dab_interval(1.0) < voxel_dab_interval(0.2));
        assert!(voxel_dab_interval(0.2) < voxel_dab_interval(0.0));
        assert_eq!(voxel_dab_interval(5.0), voxel_dab_interval(1.0));
        assert_eq!(voxel_dab_interval(f32::NAN), voxel_dab_interval(0.0));
    }

    #[test]
    fn falloff_weights_fade_to_the_rim() {
        assert_eq!(falloff_weight(0.0, 4.0, 0.5), 1.0);
        assert_eq!(falloff_weight(4.0, 4.0, 0.5), 0.0);
        assert_eq!(falloff_weight(5.0, 4.0, 0.5), 0.0, "past the rim");
        assert_eq!(falloff_weight(3.9, 4.0, 0.0), 1.0, "a hard edge pushes fully to the rim");
        assert!((falloff_weight(2.0, 4.0, 1.0) - 0.5).abs() < 1e-6, "Soft is a straight ramp");
        assert_eq!(falloff_weight(1.0, 0.0, 0.5), 0.0, "a zero radius pushes nowhere");
    }

    #[test]
    fn tool_ids_and_modes_round_trip() {
        for tool in TerrainTool::ALL {
            assert_eq!(TerrainTool::from_id(tool.id()), Some(tool));
        }
        let mut brush = TerrainBrush::default();
        for tool in TerrainTool::ALL {
            for (i, _) in tool.mode_labels().iter().enumerate() {
                brush.set_mode_index(tool, i);
                assert_eq!(brush.mode_index(tool), i, "{tool:?} mode {i}");
            }
        }
        for shape in BrushShape::ALL {
            assert_eq!(BrushShape::from_id(shape.id()), Some(shape));
        }
        assert_eq!(BrushPivot::Top.next(), BrushPivot::Bottom);
        assert_eq!(BrushPivot::Bottom.prev(), BrushPivot::Top);
    }
}

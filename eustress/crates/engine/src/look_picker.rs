//! The universal look picker and the paint bucket.
//!
//! A "look" is what you would copy off something to match it: a colour, a
//! part material, a terrain material. Press `I` over a part or the terrain,
//! in any tool, and the look under the cursor becomes the sampled look
//! ([`SampledLook`]). Every tool that applies a look reads it from there:
//!
//! - **Terrain brush.** The terrain slot the sample resolved to becomes the
//!   brush's paint material (or, in Paint with Replace, the material to
//!   replace). A part's material converts when the terrain has a material of
//!   the same name (Brick, Concrete, Grass, Ice, Sand, Slate, WoodPlanks, or
//!   a custom slot named like the part material), and the other way round.
//! - **Paint bucket** (Ctrl+Alt+B). Click a part to give it the sampled
//!   colour and part material. Scope picks how much a click paints: the part,
//!   its whole Model, or every part in the Space that looks like the one
//!   clicked. One click is one undo step.
//!
//! Locked parts can be sampled but never painted.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use eustress_common::classes::ClassName;
use eustress_common::terrain::{
    material_at_world, raycast_terrain_surface, surface_data, PaintMode, TerrainBaked, TerrainBrush, TerrainConfig,
    TerrainData, TerrainMaterial, TerrainMaterialSlots, TerrainRoot, TerrainTool, TerrainVolume,
};

use crate::classes::{BasePart, Instance};
use crate::keybindings::Action;
use crate::modal_tool::{
    ModalTool, ModalToolRegistry, ToolContext, ToolOptionControl, ToolOptionKind, ToolStepResult, ViewportHit,
};
use crate::notifications::NotificationManager;
use crate::part_selection::{pick_under_cursor_filtered, ObbFallback, PickQuery};
use crate::undo::{Action as UndoAction, PropertyValueSnapshot, UndoStack};

/// How far the picker's ray looks for terrain, in metres.
const LOOK_RAY_REACH: f32 = 4000.0;

/// Part materials by their canonical names, the set `Material::from_string`
/// reads. A test pins this list to the enum.
const PART_MATERIALS: &[&str] = &[
    "Plastic",
    "SmoothPlastic",
    "Wood",
    "WoodPlanks",
    "Metal",
    "CorrodedMetal",
    "DiamondPlate",
    "Foil",
    "Grass",
    "Concrete",
    "Brick",
    "Granite",
    "Marble",
    "Slate",
    "Sand",
    "Fabric",
    "Glass",
    "Neon",
    "Ice",
    "Gold",
    "Silver",
    "Bronze",
];

// ============================================================================
// The sampled look
// ============================================================================

/// What a sample was taken from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LookSource {
    /// Nothing sampled yet.
    #[default]
    Nothing,
    /// A part, by name.
    Part(String),
    /// The terrain.
    Terrain,
}

/// The look last sampled with `I`: what every applying tool matches.
///
/// Each field is filled when the source has it or the other kind has an
/// equivalent: a Brick part also carries terrain slot 9, a Grass slot also
/// carries the part material Grass. A field with no equivalent stays `None`.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct SampledLook {
    pub source: LookSource,
    /// sRGB colour, as a part stores it (alpha included).
    pub color: Option<[f32; 4]>,
    /// Part material name: a built-in material or a MaterialService name.
    pub part_material: Option<String>,
    /// Terrain material slot.
    pub terrain_slot: Option<u8>,
    /// The terrain slot's display name.
    pub terrain_slot_name: Option<String>,
    /// Moves on every sample, so a reader can tell a new sample from the same one.
    pub revision: u32,
}

impl SampledLook {
    pub fn is_empty(&self) -> bool {
        self.source == LookSource::Nothing
    }

    /// One line naming the look, for toasts and the bucket's hint:
    /// "Brick, #A04030 (from 'Wall')".
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(name) = self.part_material.as_deref().or(self.terrain_slot_name.as_deref()) {
            parts.push(name.to_string());
        }
        if let Some(color) = self.color {
            parts.push(hex([color[0], color[1], color[2]]));
        }
        let look = if parts.is_empty() { "a look".to_string() } else { parts.join(", ") };
        match &self.source {
            LookSource::Nothing => look,
            LookSource::Part(name) => format!("{look} (from '{name}')"),
            LookSource::Terrain => format!("{look} (from the terrain)"),
        }
    }
}

/// The sampled look's one-line summary, readable from code with no World
/// (a modal tool's step label has none). Written whenever a sample lands.
static LOOK_SUMMARY: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

fn set_summary(text: &str) {
    if let Ok(mut summary) = LOOK_SUMMARY.write() {
        summary.clear();
        summary.push_str(text);
    }
}

fn look_summary() -> String {
    LOOK_SUMMARY.read().map(|s| s.clone()).unwrap_or_default()
}

/// `#RRGGBB` for an sRGB colour.
fn hex(rgb: [f32; 3]) -> String {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", byte(rgb[0]), byte(rgb[1]), byte(rgb[2]))
}

/// A name with case, spaces, underscores and hyphens removed, so "Wood
/// Planks", "wood_planks" and "WoodPlanks" are one name.
fn squash(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The terrain Water slot. Water is its own tile with its own tool (Sea
/// Level), so a sample never puts it on the brush and no part material maps
/// to it: painting with it would lay real water.
fn water_slot() -> u8 {
    TerrainMaterial::Water.to_u8()
}

/// The terrain slot named like part material `name`, the lowest slot first.
/// Never the Water slot.
pub fn terrain_slot_for_part_material(name: &str, slots: &TerrainMaterialSlots) -> Option<u8> {
    let wanted = squash(name);
    slots
        .iter()
        .find(|(id, slot)| *id != water_slot() && squash(&slot.name) == wanted)
        .map(|(id, _)| id)
}

/// The part material named like terrain slot `slot_name`: only a built-in
/// part material, by its canonical name.
pub fn part_material_for_terrain_slot(slot_name: &str) -> Option<String> {
    let wanted = squash(slot_name);
    PART_MATERIALS.iter().find(|m| squash(m) == wanted).map(|m| m.to_string())
}

/// The part's material name: its MaterialService name when it has one, else
/// the built-in material.
fn part_material_name(part: &BasePart) -> String {
    if part.material_name.is_empty() {
        part.material.as_str().to_string()
    } else {
        part.material_name.clone()
    }
}

/// The look of a part.
pub fn look_from_part(name: &str, part: &BasePart, slots: &TerrainMaterialSlots) -> SampledLook {
    let material = part_material_name(part);
    let c = part.color.to_srgba();
    let terrain_slot = terrain_slot_for_part_material(&material, slots);
    SampledLook {
        source: LookSource::Part(name.to_string()),
        color: Some([c.red, c.green, c.blue, c.alpha]),
        part_material: Some(material),
        terrain_slot,
        terrain_slot_name: terrain_slot.and_then(|slot| slots.get(slot).map(|s| s.name.clone())),
        revision: 0,
    }
}

/// The look of terrain slot `slot`: its swatch colour, its name, and the part
/// material of the same name if there is one.
pub fn look_from_terrain(slot: u8, slots: &TerrainMaterialSlots) -> SampledLook {
    let (name, color) = match slots.get(slot) {
        Some(def) => {
            let [r, g, b] = def.swatch_srgb();
            (def.name.clone(), [r, g, b, 1.0])
        }
        None => match TerrainMaterial::from_u8(slot) {
            Some(builtin) => {
                let c = builtin.base_color().to_srgba();
                (builtin.name().to_string(), [c.red, c.green, c.blue, 1.0])
            }
            None => (format!("Material {slot}"), [0.5, 0.5, 0.5, 1.0]),
        },
    };
    SampledLook {
        source: LookSource::Terrain,
        color: Some(color),
        part_material: part_material_for_terrain_slot(&name),
        // Ground that older Spaces painted Water keeps its colour and name,
        // but the brush does not follow: it would lay real water.
        terrain_slot: (slot != water_slot()).then_some(slot),
        terrain_slot_name: Some(name),
        revision: 0,
    }
}

// ============================================================================
// Terrain brush
// ============================================================================

/// Which brush material a sample goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushField {
    /// What the brush lays down.
    Paint,
    /// What Paint with Replace paints over.
    Source,
}

/// The brush change a sample asks for, `None` when the look has no terrain
/// slot or the brush already has it. In Paint with Replace the sample is the
/// material to replace; everywhere else it is the material to paint.
pub fn brush_change(look: &SampledLook, brush: &TerrainBrush) -> Option<(BrushField, u8)> {
    let slot = look.terrain_slot?;
    let field = if brush.tool == TerrainTool::Paint && brush.paint_mode == PaintMode::Replace {
        BrushField::Source
    } else {
        BrushField::Paint
    };
    let current = match field {
        BrushField::Source => brush.source_material,
        BrushField::Paint => brush.paint_material,
    };
    (current != slot).then_some((field, slot))
}

// ============================================================================
// The picker: I
// ============================================================================

/// What is nearest along the pick ray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nearest {
    Part,
    Terrain,
    Nothing,
}

/// The nearer of a part hit and a terrain hit, by distance along the ray.
fn nearest(part: Option<f32>, terrain: Option<f32>) -> Nearest {
    match (part, terrain) {
        (Some(p), Some(t)) => {
            if p <= t {
                Nearest::Part
            } else {
                Nearest::Terrain
            }
        }
        (Some(_), None) => Nearest::Part,
        (None, Some(_)) => Nearest::Terrain,
        (None, None) => Nearest::Nothing,
    }
}

/// The cursor's ray, or `None` when the cursor is off the viewport, over a
/// panel, or no scene camera exists.
fn cursor_ray(
    windows: &Query<&Window, With<PrimaryWindow>>,
    cameras: &Query<(&Camera, &GlobalTransform)>,
    bounds: Option<&crate::ui::ViewportBounds>,
    focus: Option<&crate::ui::SlintUIFocus>,
) -> Option<Ray3d> {
    if focus.is_some_and(|f| f.has_focus) {
        return None;
    }
    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    if let Some(vb) = bounds {
        if !vb.contains_logical(cursor, window.scale_factor() as f32) {
            return None;
        }
    }
    let (camera, transform) = cameras.iter().find(|(c, _)| c.order == 0)?;
    camera.viewport_to_world(transform, cursor).ok()
}

pub struct LookPickerPlugin;

impl Plugin for LookPickerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SampledLook>()
            .add_systems(Startup, register_paint_bucket)
            .add_systems(Update, sample_look_on_action.run_if(crate::play_mode::editor_input_enabled));
    }
}

fn register_paint_bucket(mut registry: ResMut<ModalToolRegistry>) {
    registry.register("paint_bucket", || Box::new(PaintBucket::default()));
}

/// `I` (and the terrain tools' own `I`): sample the look under the cursor.
///
/// A part and the terrain compete by distance, so the sample is whichever the
/// cursor actually sees. Locked parts are sampled too. The sample goes to
/// [`SampledLook`] and, when it has a terrain slot, to the terrain brush.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sample_look_on_action(
    mut menu: MessageReader<crate::ui::MenuActionEvent>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    pick_parts: PickQuery,
    instances: Query<&Instance>,
    aabbs: Query<&bevy::camera::primitives::Aabb>,
    spatial_query: avian3d::prelude::SpatialQuery,
    base_parts: Query<&BasePart>,
    terrain: Query<(&TerrainConfig, &TerrainData, Option<&TerrainVolume>, Option<&TerrainBaked>), With<TerrainRoot>>,
    slots: Option<Res<TerrainMaterialSlots>>,
    (ui_focus, viewport_bounds): (Option<Res<crate::ui::SlintUIFocus>>, Option<Res<crate::ui::ViewportBounds>>),
    (mut look, brush, mut notifications): (
        ResMut<SampledLook>,
        Option<ResMut<TerrainBrush>>,
        Option<ResMut<NotificationManager>>,
    ),
) {
    let mut wanted = false;
    for event in menu.read() {
        if matches!(event.action, Action::SampleLook | Action::TerrainSampleMaterial) {
            wanted = true;
        }
    }
    if !wanted {
        return;
    }
    let Some(ray) = cursor_ray(&windows, &cameras, viewport_bounds.as_deref(), ui_focus.as_deref()) else {
        return;
    };

    let builtin_slots;
    let slots: &TerrainMaterialSlots = match slots.as_deref() {
        Some(slots) => slots,
        None => {
            builtin_slots = TerrainMaterialSlots::builtins();
            &builtin_slots
        }
    };

    let part_hit = pick_under_cursor_filtered(&ray, &pick_parts, &instances, &spatial_query, &aabbs, ObbFallback::Always, true);
    let terrain_hit = terrain.single().ok().and_then(|(config, data, volume, baked)| {
        let point = raycast_terrain_surface(config, surface_data(data, baked), volume, ray, LOOK_RAY_REACH, 2.0)?;
        Some((point, config, data, baked))
    });

    let sampled = match nearest(
        part_hit.as_ref().map(|h| h.distance),
        terrain_hit.as_ref().map(|(point, ..)| point.distance(ray.origin)),
    ) {
        Nearest::Part => part_hit.as_ref().and_then(|hit| {
            let part = base_parts.get(hit.entity).ok()?;
            let name = instances.get(hit.entity).map(|i| i.name.clone()).unwrap_or_default();
            Some(look_from_part(&name, part, slots))
        }),
        Nearest::Terrain => terrain_hit.as_ref().and_then(|(point, config, data, baked)| {
            // `terrain_hit` holds `Option<&TerrainBaked>` and `as_ref` lends the tuple, so
            // `baked` is `&Option<&_>` here: copy the option out.
            let sample = material_at_world(config, surface_data(data, *baked), point.x, point.z)?;
            Some(look_from_terrain(sample.primary, slots))
        }),
        Nearest::Nothing => None,
    };
    let Some(mut sampled) = sampled else {
        if let Some(n) = notifications.as_deref_mut() {
            n.info("Nothing to sample under the cursor: aim at a part or at terrain that has a material layer.");
        }
        return;
    };
    sampled.revision = look.revision.wrapping_add(1);
    set_summary(&sampled.summary());

    // The brush follows the sample.
    let mut brush_took: Option<String> = None;
    if let (Some(mut brush), Some(slot)) = (brush, sampled.terrain_slot) {
        if let Some((field, slot)) = brush_change(&sampled, &brush) {
            match field {
                BrushField::Paint => brush.paint_material = slot,
                BrushField::Source => brush.source_material = slot,
            }
        }
        brush_took = Some(match sampled.terrain_slot_name.as_deref() {
            Some(name) => name.to_string(),
            None => format!("material {slot}"),
        });
    }

    if let Some(n) = notifications.as_deref_mut() {
        let mut message = format!("Sampled {}.", sampled.summary());
        if let Some(name) = brush_took {
            message.push_str(&format!(" Terrain brush: {name}."));
        }
        if sampled.terrain_slot.is_none() && sampled.terrain_slot_name.as_deref() == Some("Water") {
            message.push_str(" Water is laid with the Sea Level tool, so the brush is unchanged.");
        }
        if sampled.color.is_some() || sampled.part_material.is_some() {
            message.push_str(" Ctrl+Alt+B paints parts with it.");
        }
        n.info(message);
    }
    *look = sampled;
}

// ============================================================================
// The paint bucket
// ============================================================================

/// What a click applies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ApplyWhat {
    #[default]
    Both,
    Color,
    Material,
}

impl ApplyWhat {
    const LABELS: [&'static str; 3] = ["Color and material", "Color", "Material"];

    fn label(self) -> &'static str {
        match self {
            ApplyWhat::Both => Self::LABELS[0],
            ApplyWhat::Color => Self::LABELS[1],
            ApplyWhat::Material => Self::LABELS[2],
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        [ApplyWhat::Both, ApplyWhat::Color, ApplyWhat::Material].into_iter().find(|w| w.label() == label)
    }
}

/// How much one click paints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaintScope {
    /// The part clicked.
    #[default]
    Part,
    /// Every part inside the clicked part's Model.
    Model,
    /// Every part in the Space with the clicked part's colour and material:
    /// "replace this look with that one".
    Matching,
}

impl PaintScope {
    const LABELS: [&'static str; 3] = ["Part", "Model", "Matching parts"];

    fn label(self) -> &'static str {
        match self {
            PaintScope::Part => Self::LABELS[0],
            PaintScope::Model => Self::LABELS[1],
            PaintScope::Matching => Self::LABELS[2],
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        [PaintScope::Part, PaintScope::Model, PaintScope::Matching].into_iter().find(|s| s.label() == label)
    }
}

/// Click a part to paint it with the sampled look.
#[derive(Default)]
pub struct PaintBucket {
    apply: ApplyWhat,
    scope: PaintScope,
}

impl ModalTool for PaintBucket {
    fn id(&self) -> &'static str {
        "paint_bucket"
    }

    fn name(&self) -> &'static str {
        "Paint Bucket"
    }

    fn step_label(&self) -> String {
        let look = look_summary();
        if look.is_empty() {
            "press I over a part or the terrain to sample a look".to_string()
        } else {
            format!("click a part to paint it with {look}")
        }
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        let choice = |id: &str, label: &str, options: &[&str], selected: &str| ToolOptionControl {
            id: id.to_string(),
            label: label.to_string(),
            kind: ToolOptionKind::Choice {
                options: options.iter().map(|o| o.to_string()).collect(),
                selected: selected.to_string(),
            },
            advanced: false,
        };
        vec![
            choice("apply", "Apply", &ApplyWhat::LABELS, self.apply.label()),
            choice("scope", "Paint", &PaintScope::LABELS, self.scope.label()),
            ToolOptionControl {
                id: "hint".to_string(),
                label: String::new(),
                kind: ToolOptionKind::Label {
                    text: "I samples a look. Locked parts are never painted. Esc leaves the tool.".to_string(),
                },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, hit: &ViewportHit, ctx: &mut ToolContext) -> ToolStepResult {
        let (clicked, apply, scope) = (hit.hit_entity, self.apply, self.scope);
        ctx.commands.queue(move |world: &mut World| {
            paint_with_sampled_look(world, clicked, apply, scope);
        });
        ToolStepResult::Continue
    }

    fn on_option_changed(&mut self, control_id: &str, value: &str, _ctx: &mut ToolContext) -> ToolStepResult {
        match control_id {
            "apply" => {
                if let Some(apply) = ApplyWhat::from_label(value) {
                    self.apply = apply;
                }
            }
            "scope" => {
                if let Some(scope) = PaintScope::from_label(value) {
                    self.scope = scope;
                }
            }
            _ => {}
        }
        ToolStepResult::Continue
    }

    fn commit(&mut self, _world: &mut World) {}

    fn cancel(&mut self, _commands: &mut Commands) {}

    /// Stays armed so many parts can be painted in a row.
    fn auto_exit_on_commit(&self) -> bool {
        false
    }
}

/// What one bucket click did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PaintReport {
    pub painted: usize,
    pub locked_skipped: usize,
}

/// The part an entity belongs to: itself, or its nearest ancestor with a
/// `BasePart` (a click can land on a part's mesh child).
fn owning_part(world: &World, entity: Entity) -> Option<Entity> {
    let mut current = entity;
    for _ in 0..16 {
        if world.get::<BasePart>(current).is_some() {
            return Some(current);
        }
        current = world.get::<ChildOf>(current)?.parent();
    }
    None
}

/// Every part at or under `root`.
fn parts_under(world: &World, root: Entity) -> Vec<Entity> {
    let mut parts = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if world.get::<BasePart>(entity).is_some() {
            parts.push(entity);
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    parts
}

/// The outermost Model above `start`, reaching through Folders; `None` when
/// the part sits in no Model.
fn topmost_model(world: &World, start: Entity) -> Option<Entity> {
    let mut found = None;
    let mut current = start;
    while let Some(parent) = world.get::<ChildOf>(current).map(|c| c.parent()) {
        match world.get::<Instance>(parent).map(|i| i.class_name) {
            Some(ClassName::Model) => found = Some(parent),
            Some(ClassName::Folder) => {}
            _ => break,
        }
        current = parent;
    }
    found
}

/// Whether two colours are the same colour to the eye: within half a step of
/// an 8-bit channel, the precision a part file keeps.
fn same_color(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= 0.5 / 255.0)
}

/// The parts one click paints.
fn collect_targets(world: &mut World, start: Entity, scope: PaintScope) -> Vec<Entity> {
    match scope {
        PaintScope::Part => vec![start],
        PaintScope::Model => match topmost_model(world, start) {
            Some(model) => parts_under(world, model),
            None => vec![start],
        },
        PaintScope::Matching => {
            let Some(clicked) = world.get::<BasePart>(start) else {
                return vec![start];
            };
            let c = clicked.color.to_srgba();
            let color = [c.red, c.green, c.blue, c.alpha];
            let material = part_material_name(clicked);
            let mut query = world.query::<(Entity, &BasePart)>();
            query
                .iter(world)
                .filter(|(_, part)| {
                    let c = part.color.to_srgba();
                    same_color(color, [c.red, c.green, c.blue, c.alpha])
                        && part_material_name(part).eq_ignore_ascii_case(&material)
                })
                .map(|(entity, _)| entity)
                .collect()
        }
    }
}

/// Paint the clicked part (and what `scope` adds) with the sampled look, as
/// one undo step. Says why on screen when nothing was painted.
pub fn paint_with_sampled_look(world: &mut World, clicked: Option<Entity>, apply: ApplyWhat, scope: PaintScope) -> PaintReport {
    fn tell(world: &mut World, warn: bool, message: String) {
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            if warn {
                n.warning(message);
            } else {
                n.success(message);
            }
        }
    }

    let look = world.get_resource::<SampledLook>().cloned().unwrap_or_default();
    if look.is_empty() {
        tell(world, true, "Nothing sampled yet: press I over a part or the terrain first.".to_string());
        return PaintReport::default();
    }
    let new_color = if apply == ApplyWhat::Material { None } else { look.color };
    let new_material = if apply == ApplyWhat::Color { None } else { look.part_material.clone() };
    if new_color.is_none() && new_material.is_none() {
        let what = if apply == ApplyWhat::Material {
            format!("{} has no part material to paint with", look.summary())
        } else {
            format!("{} has nothing to paint with", look.summary())
        };
        tell(world, true, what);
        return PaintReport::default();
    }
    let Some(start) = clicked.and_then(|e| owning_part(world, e)) else {
        tell(world, true, "Click a part to paint it. Terrain is painted with the terrain Paint tool.".to_string());
        return PaintReport::default();
    };

    let mut report = PaintReport::default();
    let mut color_before: Vec<(u32, PropertyValueSnapshot)> = Vec::new();
    let mut material_before: Vec<(u32, PropertyValueSnapshot)> = Vec::new();
    for entity in collect_targets(world, start, scope) {
        let id = world.get::<Instance>(entity).map(|i| i.id).unwrap_or(0);
        let Some(mut part) = world.get_mut::<BasePart>(entity) else { continue };
        if part.locked {
            report.locked_skipped += 1;
            continue;
        }
        let mut changed = false;
        if let Some(color) = new_color {
            let c = part.color.to_srgba();
            let before = [c.red, c.green, c.blue, c.alpha];
            if !same_color(before, color) {
                part.color = Color::srgba(color[0], color[1], color[2], color[3]);
                if id != 0 {
                    color_before.push((id, PropertyValueSnapshot::Color(before)));
                }
                changed = true;
            }
        }
        if let Some(material) = new_material.as_deref() {
            let before = part_material_name(&part);
            if !before.eq_ignore_ascii_case(material) {
                part.material = crate::classes::Material::from_string(material);
                part.material_name = material.to_string();
                if id != 0 {
                    material_before.push((id, PropertyValueSnapshot::Material(before)));
                }
                changed = true;
            }
        }
        if changed {
            report.painted += 1;
        }
    }

    let mut steps: Vec<UndoAction> = Vec::new();
    if let (Some(color), false) = (new_color, color_before.is_empty()) {
        steps.push(UndoAction::ChangePropertyMulti {
            entities: color_before,
            property: "Color".to_string(),
            new_value: PropertyValueSnapshot::Color(color),
        });
    }
    if let (Some(material), false) = (new_material.clone(), material_before.is_empty()) {
        steps.push(UndoAction::ChangePropertyMulti {
            entities: material_before,
            property: "Material".to_string(),
            new_value: PropertyValueSnapshot::Material(material),
        });
    }
    if !steps.is_empty() {
        let action = if steps.len() == 1 { steps.remove(0) } else { UndoAction::Batch { actions: steps } };
        if let Some(mut undo) = world.get_resource_mut::<UndoStack>() {
            let n = report.painted;
            undo.push_labeled(format!("Paint {} part{}", n, if n == 1 { "" } else { "s" }), action);
        }
    }

    if report.painted == 0 && report.locked_skipped == 0 {
        tell(world, false, format!("Already {}.", look.summary()));
    } else if report.painted == 0 {
        tell(world, true, format!("{} locked part(s) skipped; nothing painted.", report.locked_skipped));
    } else {
        let mut message = format!(
            "Painted {} part{} with {}.",
            report.painted,
            if report.painted == 1 { "" } else { "s" },
            look.summary()
        );
        if report.locked_skipped > 0 {
            message.push_str(&format!(" {} locked skipped.", report.locked_skipped));
        }
        tell(world, false, message);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classes::Material;

    fn part(color: Color, material: Material, name: &str) -> BasePart {
        BasePart { color, material, material_name: name.to_string(), ..Default::default() }
    }

    #[test]
    fn the_part_material_list_matches_the_enum() {
        for name in PART_MATERIALS {
            assert_eq!(Material::from_string(name).as_str(), *name, "{name}");
        }
    }

    #[test]
    fn names_match_ignoring_case_spaces_and_underscores() {
        let slots = TerrainMaterialSlots::builtins();
        assert_eq!(terrain_slot_for_part_material("brick", &slots), Some(TerrainMaterial::Brick.to_u8()));
        assert_eq!(terrain_slot_for_part_material("Wood Planks", &slots), Some(TerrainMaterial::WoodPlanks.to_u8()));
        assert_eq!(terrain_slot_for_part_material("Plastic", &slots), None);
        assert_eq!(part_material_for_terrain_slot("wood_planks"), Some("WoodPlanks".to_string()));
        assert_eq!(part_material_for_terrain_slot("Rock"), None);
    }

    #[test]
    fn a_part_look_carries_its_terrain_equivalent() {
        let slots = TerrainMaterialSlots::builtins();
        let brick = look_from_part("Wall", &part(Color::srgb(0.6, 0.25, 0.2), Material::Brick, ""), &slots);
        assert_eq!(brick.part_material.as_deref(), Some("Brick"));
        assert_eq!(brick.terrain_slot, Some(TerrainMaterial::Brick.to_u8()));
        assert_eq!(brick.terrain_slot_name.as_deref(), Some("Brick"));
        let plastic = look_from_part("Block", &part(Color::WHITE, Material::Plastic, ""), &slots);
        assert_eq!(plastic.terrain_slot, None, "terrain has no Plastic");
        // A MaterialService name takes the place of the enum.
        let custom = look_from_part("Pipe", &part(Color::WHITE, Material::Metal, "Concrete"), &slots);
        assert_eq!(custom.part_material.as_deref(), Some("Concrete"));
        assert_eq!(custom.terrain_slot, Some(TerrainMaterial::Concrete.to_u8()));
    }

    #[test]
    fn a_terrain_look_carries_its_part_equivalent_and_colour() {
        let slots = TerrainMaterialSlots::builtins();
        let grass = look_from_terrain(TerrainMaterial::Grass.to_u8(), &slots);
        assert_eq!(grass.part_material.as_deref(), Some("Grass"));
        assert_eq!(grass.terrain_slot_name.as_deref(), Some("Grass"));
        assert!(grass.color.is_some());
        let rock = look_from_terrain(TerrainMaterial::Rock.to_u8(), &slots);
        assert_eq!(rock.part_material, None, "parts have no Rock");
        assert!(rock.color.is_some(), "the colour still matches");
    }

    #[test]
    fn water_is_never_put_on_the_brush() {
        let slots = TerrainMaterialSlots::builtins();
        let water = look_from_terrain(TerrainMaterial::Water.to_u8(), &slots);
        assert_eq!(water.terrain_slot, None, "water has its own tool");
        assert_eq!(water.terrain_slot_name.as_deref(), Some("Water"));
        assert!(water.color.is_some(), "the colour still matches");
        assert_eq!(terrain_slot_for_part_material("Water", &slots), None);
        assert_eq!(brush_change(&water, &TerrainBrush::default()), None);
    }

    #[test]
    fn the_summary_reads_as_one_line() {
        let look = SampledLook {
            source: LookSource::Part("Wall".to_string()),
            color: Some([160.0 / 255.0, 64.0 / 255.0, 48.0 / 255.0, 1.0]),
            part_material: Some("Brick".to_string()),
            ..Default::default()
        };
        assert_eq!(look.summary(), "Brick, #A04030 (from 'Wall')");
        assert!(SampledLook::default().is_empty());
    }

    #[test]
    fn the_nearer_hit_wins() {
        assert_eq!(nearest(Some(3.0), Some(8.0)), Nearest::Part);
        assert_eq!(nearest(Some(9.0), Some(8.0)), Nearest::Terrain);
        assert_eq!(nearest(None, Some(8.0)), Nearest::Terrain);
        assert_eq!(nearest(Some(1.0), None), Nearest::Part);
        assert_eq!(nearest(None, None), Nearest::Nothing);
    }

    #[test]
    fn the_brush_follows_a_sample_by_mode() {
        let slots = TerrainMaterialSlots::builtins();
        let brick = look_from_part("Wall", &part(Color::WHITE, Material::Brick, ""), &slots);
        let mut brush = TerrainBrush::default();
        brush.paint_material = TerrainMaterial::Grass.to_u8();
        brush.source_material = TerrainMaterial::Grass.to_u8();
        brush.tool = TerrainTool::Sculpt;
        assert_eq!(brush_change(&brick, &brush), Some((BrushField::Paint, TerrainMaterial::Brick.to_u8())));
        brush.tool = TerrainTool::Paint;
        brush.paint_mode = PaintMode::Replace;
        assert_eq!(brush_change(&brick, &brush), Some((BrushField::Source, TerrainMaterial::Brick.to_u8())));
        brush.source_material = TerrainMaterial::Brick.to_u8();
        assert_eq!(brush_change(&brick, &brush), None, "already there");
        let plastic = look_from_part("Block", &part(Color::WHITE, Material::Plastic, ""), &slots);
        assert_eq!(brush_change(&plastic, &brush), None, "no terrain equivalent");
    }

    // ── The bucket, against a World ──────────────────────────────────────

    fn spawn_part(world: &mut World, id: u32, name: &str, color: Color, material: &str, locked: bool) -> Entity {
        world
            .spawn((
                Instance { name: name.to_string(), class_name: ClassName::Part, id, ..Default::default() },
                BasePart {
                    color,
                    material: Material::from_string(material),
                    material_name: material.to_string(),
                    locked,
                    ..Default::default()
                },
            ))
            .id()
    }

    fn bucket_world() -> World {
        let mut world = World::new();
        world.init_resource::<UndoStack>();
        world.insert_resource(SampledLook {
            source: LookSource::Part("Wall".to_string()),
            color: Some([0.6, 0.25, 0.2, 1.0]),
            part_material: Some("Brick".to_string()),
            terrain_slot: Some(TerrainMaterial::Brick.to_u8()),
            terrain_slot_name: Some("Brick".to_string()),
            revision: 1,
        });
        world
    }

    #[test]
    fn a_click_paints_the_part_and_undo_restores_colour_and_material() {
        let mut world = bucket_world();
        let blue = Color::srgb(0.1, 0.2, 0.8);
        let part_entity = spawn_part(&mut world, 11, "Door", blue, "BrushedMetal", false);
        let report = paint_with_sampled_look(&mut world, Some(part_entity), ApplyWhat::Both, PaintScope::Part);
        assert_eq!(report, PaintReport { painted: 1, locked_skipped: 0 });
        let painted = world.get::<BasePart>(part_entity).unwrap();
        assert!(same_color(
            [0.6, 0.25, 0.2, 1.0],
            { let c = painted.color.to_srgba(); [c.red, c.green, c.blue, c.alpha] }
        ));
        assert_eq!(painted.material_name, "Brick");
        assert_eq!(world.resource::<UndoStack>().current_index(), 1, "one click is one step");
        assert_eq!(world.resource::<UndoStack>().last_label(), Some("Paint 1 part"));

        let step = world.resource::<UndoStack>().history().front().cloned().unwrap();
        crate::undo::apply_undo_action(&step, &mut world);
        let restored = world.get::<BasePart>(part_entity).unwrap();
        let c = restored.color.to_srgba();
        assert!(same_color([0.1, 0.2, 0.8, 1.0], [c.red, c.green, c.blue, c.alpha]));
        assert_eq!(restored.material_name, "BrushedMetal", "the custom name comes back, not just the enum");
    }

    #[test]
    fn colour_only_leaves_the_material_alone() {
        let mut world = bucket_world();
        let e = spawn_part(&mut world, 12, "Slab", Color::WHITE, "Concrete", false);
        paint_with_sampled_look(&mut world, Some(e), ApplyWhat::Color, PaintScope::Part);
        assert_eq!(world.get::<BasePart>(e).unwrap().material_name, "Concrete");
    }

    #[test]
    fn locked_parts_are_never_painted() {
        let mut world = bucket_world();
        let locked = spawn_part(&mut world, 13, "Scenery", Color::WHITE, "Plastic", true);
        let report = paint_with_sampled_look(&mut world, Some(locked), ApplyWhat::Both, PaintScope::Part);
        assert_eq!(report, PaintReport { painted: 0, locked_skipped: 1 });
        assert_eq!(world.get::<BasePart>(locked).unwrap().material_name, "Plastic");
        assert_eq!(world.resource::<UndoStack>().current_index(), 0, "nothing to undo");
    }

    #[test]
    fn model_scope_paints_every_part_in_the_model_and_no_others() {
        let mut world = bucket_world();
        let model = world
            .spawn(Instance { name: "House".to_string(), class_name: ClassName::Model, id: 20, ..Default::default() })
            .id();
        let a = spawn_part(&mut world, 21, "A", Color::WHITE, "Plastic", false);
        let b = spawn_part(&mut world, 22, "B", Color::WHITE, "Plastic", false);
        let outside = spawn_part(&mut world, 23, "Outside", Color::WHITE, "Plastic", false);
        world.entity_mut(a).insert(ChildOf(model));
        world.entity_mut(b).insert(ChildOf(model));
        let report = paint_with_sampled_look(&mut world, Some(a), ApplyWhat::Both, PaintScope::Model);
        assert_eq!(report.painted, 2);
        assert_eq!(world.get::<BasePart>(b).unwrap().material_name, "Brick");
        assert_eq!(world.get::<BasePart>(outside).unwrap().material_name, "Plastic");
        assert_eq!(world.resource::<UndoStack>().last_label(), Some("Paint 2 parts"));
    }

    #[test]
    fn matching_scope_replaces_one_look_everywhere() {
        let mut world = bucket_world();
        let red = Color::srgb(0.9, 0.1, 0.1);
        let a = spawn_part(&mut world, 31, "A", red, "Plastic", false);
        let b = spawn_part(&mut world, 32, "B", red, "Plastic", false);
        let other = spawn_part(&mut world, 33, "Other", Color::srgb(0.1, 0.9, 0.1), "Plastic", false);
        let report = paint_with_sampled_look(&mut world, Some(a), ApplyWhat::Both, PaintScope::Matching);
        assert_eq!(report.painted, 2);
        assert_eq!(world.get::<BasePart>(b).unwrap().material_name, "Brick");
        assert_eq!(world.get::<BasePart>(other).unwrap().material_name, "Plastic");
    }

    #[test]
    fn a_click_that_hits_no_part_paints_nothing() {
        let mut world = bucket_world();
        let report = paint_with_sampled_look(&mut world, None, ApplyWhat::Both, PaintScope::Part);
        assert_eq!(report, PaintReport::default());
        assert_eq!(world.resource::<UndoStack>().current_index(), 0);
    }

    #[test]
    fn nothing_sampled_paints_nothing() {
        let mut world = World::new();
        world.init_resource::<UndoStack>();
        let e = spawn_part(&mut world, 41, "A", Color::WHITE, "Plastic", false);
        let report = paint_with_sampled_look(&mut world, Some(e), ApplyWhat::Both, PaintScope::Part);
        assert_eq!(report, PaintReport::default());
    }

    #[test]
    fn a_click_on_a_mesh_child_paints_the_part_that_owns_it() {
        let mut world = bucket_world();
        let owner = spawn_part(&mut world, 51, "Owner", Color::WHITE, "Plastic", false);
        let mesh_child = world.spawn_empty().insert(ChildOf(owner)).id();
        let report = paint_with_sampled_look(&mut world, Some(mesh_child), ApplyWhat::Both, PaintScope::Part);
        assert_eq!(report.painted, 1);
        assert_eq!(world.get::<BasePart>(owner).unwrap().material_name, "Brick");
    }
}

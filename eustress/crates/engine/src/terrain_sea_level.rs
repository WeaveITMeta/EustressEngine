//! The Sea Level tool (`docs/design/TERRAIN_TOOLS_UX.md`, sections 4.4 and
//! 10.1): a left drag on the ground draws a rectangle, snapped when snap is
//! on; the water plane then stands over it at the level, which a left drag
//! inside the rectangle moves up or down (on a vertical plane facing the
//! camera), `PageUp`/`PageDown` nudge by the snap step, and the tool bar
//! types. `Enter` or the bar's buttons apply it through the terrain API as
//! one undo entry: Fill (`TerrainCommand::FillWater`) puts water in the air
//! under the level, Evaporate (`TerrainCommand::DrainWater`) takes away the
//! water standing at or below it. Neither touches the ground. `Ctrl` swaps
//! the two for `Enter`, as it swaps every tool's mode. A click without a drag
//! clears the rectangle, and so does `Esc` (a second `Esc` leaves the tools).
//! The rectangle stays after an apply, so the level can be moved and applied
//! again.
//!
//! `terrain_cursor` draws the rectangle, the plane, its corner posts and the
//! shoreline the level would make, and the readout.

use bevy::ecs::schedule::common_conditions::resource_equals;
use bevy::prelude::*;
use eustress_common::terrain::api::TerrainCommand;
use eustress_common::terrain::{
    ray_plane_hit, SeaLevelMode, TerrainBrush, TerrainBrushHover, TerrainMode, TerrainPaintGate, TerrainTool,
};
use eustress_common::units::Unit;

use crate::terrain_commands::{apply_terrain_commands, TerrainCommandOrigin};
use crate::terrain_cursor::compact_length;

/// The Sea Level tool's input and apply.
pub struct TerrainSeaLevelPlugin;

impl Plugin for TerrainSeaLevelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SeaLevelTool>()
            .add_systems(
                Update,
                sea_level_input
                    .after(eustress_common::terrain::update_brush_hover)
                    .before(crate::terrain_cursor::update_terrain_cursor)
                    .run_if(resource_equals(TerrainMode::Editor)),
            )
            .add_systems(Update, apply_pending_sea_level.after(sea_level_input));
    }
}

/// A rectangle side shorter than this is a click, which clears the rectangle.
pub const MIN_RECT_SIDE: f32 = 0.5;
/// How far below the level the box an apply sends reaches: past any ground,
/// so Evaporate dries every column whose water stands at or below the level.
const APPLY_DEPTH: f32 = 1.0e5;

/// The Sea Level tool's rectangle, level and drag.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct SeaLevelTool {
    /// The rectangle's world XZ corners (min, max), `None` before one is drawn.
    pub rect: Option<(Vec2, Vec2)>,
    /// The water level, world Y. Kept for the session, so each new
    /// rectangle starts at the last level; the first starts at the ground
    /// where its drag began.
    pub level: Option<f32>,
    /// What to apply at the next update (`Enter` or a bar button).
    pub pending: Option<SeaLevelMode>,
    drag: Option<SeaLevelDrag>,
}

impl SeaLevelTool {
    /// A drag is moving the rectangle's corner or the level.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The level is being dragged.
    pub fn dragging_level(&self) -> bool {
        matches!(self.drag, Some(SeaLevelDrag::Level { .. }))
    }

    /// Forget the rectangle and any drag; the level stays.
    pub fn clear(&mut self) {
        self.rect = None;
        self.drag = None;
        self.pending = None;
    }

    /// Nudge the level by `delta` metres, when there is one.
    pub fn nudge_level(&mut self, delta: f32) {
        if let Some(level) = self.level.as_mut() {
            *level += delta;
        }
    }

    /// The box an apply of `mode` sends, `None` without a rectangle and a
    /// level.
    pub fn command(&self, mode: SeaLevelMode) -> Option<TerrainCommand> {
        let ((lo, hi), level) = (self.rect?, self.level?);
        let min = Vec3::new(lo.x, level - APPLY_DEPTH, lo.y);
        let max = Vec3::new(hi.x, level, hi.y);
        Some(match mode {
            SeaLevelMode::Fill => TerrainCommand::FillWater { min, max },
            SeaLevelMode::Evaporate => TerrainCommand::DrainWater { min, max },
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SeaLevelDrag {
    /// Drawing the rectangle from world XZ `from`, whose ground stands at
    /// `height` (the plane the corner follows off the terrain).
    Rect { from: Vec2, height: f32 },
    /// Moving the level on the vertical plane through `point` facing the
    /// camera along `normal`: the level was `start_level` when the pointer
    /// crossed that plane at height `start_y`.
    Level { start_level: f32, start_y: f32, point: Vec3, normal: Vec3 },
}

/// `value` snapped to `step`, or as it is without one.
fn snapped(value: f32, step: Option<f32>) -> f32 {
    match step {
        Some(step) => (value / step).round() * step,
        None => value,
    }
}

/// Where `ray` crosses the plane through `point` with `normal`, `None` when
/// it runs along it or the plane is behind it.
fn ray_plane(ray: Ray3d, point: Vec3, normal: Vec3) -> Option<Vec3> {
    let direction = *ray.direction;
    let denominator = direction.dot(normal);
    if denominator.abs() < 1e-4 {
        return None;
    }
    let t = (point - ray.origin).dot(normal) / denominator;
    (t > 0.0).then(|| ray.origin + direction * t)
}

/// World XZ `p` lies in `rect`, grown by `margin`.
fn inside(rect: (Vec2, Vec2), p: Vec2, margin: f32) -> bool {
    p.x >= rect.0.x - margin && p.y >= rect.0.y - margin && p.x <= rect.1.x + margin && p.y <= rect.1.y + margin
}

fn held(keys: &ButtonInput<KeyCode>, left: KeyCode, right: KeyCode) -> bool {
    keys.pressed(left) || keys.pressed(right)
}

/// Draw the rectangle and drag the level with the left button, and queue
/// an apply on `Enter` (see the module docs). Runs only while the terrain
/// tools are on; a drag ends when Sea Level stops being the tool.
#[allow(clippy::too_many_arguments)]
fn sea_level_input(
    brush: Res<TerrainBrush>,
    hover: Res<TerrainBrushHover>,
    gate: Option<Res<TerrainPaintGate>>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    mut tool: ResMut<SeaLevelTool>,
) {
    if brush.tool != TerrainTool::SeaLevel {
        if tool.drag.is_some() {
            tool.drag = None;
        }
        return;
    }
    let step = brush.active_snap();
    let allowed = gate.map_or(true, |gate| gate.allowed);
    let alt = held(&keys, KeyCode::AltLeft, KeyCode::AltRight);

    if buttons.just_pressed(MouseButton::Left) && allowed && !alt {
        let on_box = match (tool.rect, tool.level, hover.ray) {
            (Some(rect), Some(level), Some(ray)) => {
                let margin = step.unwrap_or(0.0).max(0.25);
                ray_plane_hit(ray, level).is_some_and(|p| inside(rect, p.xz(), margin))
                    || hover.surface.is_some_and(|p| inside(rect, p.xz(), margin))
            }
            _ => false,
        };
        if on_box {
            let (rect, level, ray) = (tool.rect.unwrap_or_default(), tool.level.unwrap_or_default(), hover.ray);
            let point = Vec3::new((rect.0.x + rect.1.x) * 0.5, level, (rect.0.y + rect.1.y) * 0.5);
            // A vertical plane facing the camera; straight down it has no
            // horizontal facing, and the Z axis stands in.
            let facing = ray.map(|ray| Vec3::new(ray.direction.x, 0.0, ray.direction.z)).unwrap_or(Vec3::Z);
            let normal = facing.try_normalize().unwrap_or(Vec3::Z);
            if let Some(start) = ray.and_then(|ray| ray_plane(ray, point, normal)) {
                tool.drag = Some(SeaLevelDrag::Level { start_level: level, start_y: start.y, point, normal });
            }
        } else if let Some(hit) = hover.surface {
            let from = Vec2::new(snapped(hit.x, step), snapped(hit.z, step));
            tool.drag = Some(SeaLevelDrag::Rect { from, height: hit.y });
            tool.rect = Some((from, from));
            if tool.level.is_none() {
                tool.level = Some(snapped(hit.y, step));
            }
        }
    }

    if buttons.pressed(MouseButton::Left) {
        match tool.drag {
            Some(SeaLevelDrag::Rect { from, height }) => {
                let at = hover.surface.or_else(|| hover.ray.and_then(|ray| ray_plane_hit(ray, height)));
                if let Some(at) = at {
                    let to = Vec2::new(snapped(at.x, step), snapped(at.z, step));
                    let rect = (from.min(to), from.max(to));
                    if tool.rect != Some(rect) {
                        tool.rect = Some(rect);
                    }
                }
            }
            Some(SeaLevelDrag::Level { start_level, start_y, point, normal }) => {
                if let Some(at) = hover.ray.and_then(|ray| ray_plane(ray, point, normal)) {
                    let level = snapped(start_level + (at.y - start_y), step);
                    if level.is_finite() && tool.level != Some(level) {
                        tool.level = Some(level);
                    }
                }
            }
            None => {}
        }
    } else if let Some(drag) = tool.drag {
        // Read first and written only here, so an idle frame leaves the
        // resource unchanged. A click without a drag clears the rectangle.
        tool.drag = None;
        if let (SeaLevelDrag::Rect { .. }, Some((lo, hi))) = (drag, tool.rect) {
            let side = hi - lo;
            if side.x < MIN_RECT_SIDE || side.y < MIN_RECT_SIDE {
                tool.rect = None;
            }
        }
    }

    let enter = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter);
    let typing = ui_focus.as_deref().is_some_and(|focus| focus.text_input_focused)
        || crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed);
    if enter && !typing && tool.drag.is_none() && tool.rect.is_some() {
        let ctrl = held(&keys, KeyCode::ControlLeft, KeyCode::ControlRight);
        let mode = match (brush.sea_level_mode, ctrl) {
            (SeaLevelMode::Fill, false) | (SeaLevelMode::Evaporate, true) => SeaLevelMode::Fill,
            _ => SeaLevelMode::Evaporate,
        };
        tool.pending = Some(mode);
    }
}

/// Apply the queued Fill or Evaporate over the rectangle as one undo entry,
/// and say so when it changed nothing.
fn apply_pending_sea_level(world: &mut World) {
    let queued = world.get_resource::<SeaLevelTool>().and_then(|tool| tool.pending);
    let Some(mode) = queued else { return };
    let command = {
        let mut tool = world.resource_mut::<SeaLevelTool>();
        tool.pending = None;
        tool.command(mode)
    };
    let Some(command) = command else {
        if let Some(mut n) = world.get_resource_mut::<crate::notifications::NotificationManager>() {
            n.info("Sea Level: drag a rectangle on the ground first.");
        }
        return;
    };
    let label = match mode {
        SeaLevelMode::Fill => "Sea Level",
        SeaLevelMode::Evaporate => "Evaporate",
    };
    let result = apply_terrain_commands(world, vec![command], TerrainCommandOrigin::Tool { label: label.to_string() })
        .pop()
        .unwrap_or_else(|| Err("no terrain command ran".to_string()));
    let message = match (&result, mode) {
        (Ok(effect), _) if effect.water_changed => None,
        (Ok(_), SeaLevelMode::Fill) => Some("Sea Level: the ground stands at or above the level everywhere in the rectangle, so there was no air to fill.".to_string()),
        (Ok(_), SeaLevelMode::Evaporate) => Some("Evaporate: no water stands at or below the level in the rectangle.".to_string()),
        (Err(e), _) => Some(format!("{label}: {e}")),
    };
    if let Some(message) = message {
        info!("🌊 {message}");
        if let Some(mut n) = world.get_resource_mut::<crate::notifications::NotificationManager>() {
            if result.is_err() {
                n.warning(message);
            } else {
                n.info(message);
            }
        }
    }
}

/// The readout beside the cursor while Sea Level is the tool: the mode, the
/// rectangle's size and the level, in the display unit.
pub fn sea_level_readout(tool: &SeaLevelTool, mode: SeaLevelMode, unit: Unit) -> String {
    let name = match mode {
        SeaLevelMode::Fill => "Sea Level",
        SeaLevelMode::Evaporate => "Evaporate",
    };
    let mut parts = vec![name.to_string()];
    match tool.rect {
        Some((lo, hi)) => {
            let side = hi - lo;
            parts.push(format!("{} × {}", compact_length(side.x, unit), compact_length(side.y, unit)));
        }
        None => parts.push("drag a rectangle".to_string()),
    }
    if let Some(level) = tool.level {
        parts.push(format!("level {}", compact_length(level, unit)));
    }
    if tool.rect.is_some() && !tool.dragging() {
        parts.push("Enter to apply".to_string());
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_apply_sends_the_rectangle_down_from_the_level() {
        let mut tool = SeaLevelTool::default();
        assert!(tool.command(SeaLevelMode::Fill).is_none(), "nothing to apply before a rectangle");
        tool.rect = Some((Vec2::new(-4.0, -2.0), Vec2::new(6.0, 3.0)));
        assert!(tool.command(SeaLevelMode::Fill).is_none(), "nor without a level");
        tool.level = Some(12.5);
        let Some(TerrainCommand::FillWater { min, max }) = tool.command(SeaLevelMode::Fill) else {
            panic!("Fill sends FillWater");
        };
        assert_eq!(max, Vec3::new(6.0, 12.5, 3.0));
        assert_eq!((min.x, min.z), (-4.0, -2.0));
        assert!(min.y < -1000.0, "the box reaches past any ground");
        assert!(matches!(tool.command(SeaLevelMode::Evaporate), Some(TerrainCommand::DrainWater { .. })));

        tool.nudge_level(-0.5);
        assert_eq!(tool.level, Some(12.0));
        tool.clear();
        assert_eq!((tool.rect, tool.level), (None, Some(12.0)), "a clear keeps the level");
    }

    #[test]
    fn snapping_and_the_drag_plane() {
        assert_eq!(snapped(3.3, Some(0.5)), 3.5);
        assert_eq!(snapped(3.3, None), 3.3);
        let ray = Ray3d::new(Vec3::new(0.0, 10.0, -10.0), Dir3::new(Vec3::new(0.0, -1.0, 1.0)).unwrap());
        let hit = ray_plane(ray, Vec3::ZERO, Vec3::Z).expect("the ray crosses the plane");
        assert!(hit.distance(Vec3::ZERO) < 1e-4);
        assert!(ray_plane(ray, Vec3::ZERO, Vec3::X).is_none(), "a ray along the plane misses it");
        assert!(inside((Vec2::ZERO, Vec2::ONE), Vec2::new(1.2, 0.5), 0.25));
        assert!(!inside((Vec2::ZERO, Vec2::ONE), Vec2::new(1.3, 0.5), 0.25));
    }

    #[test]
    fn the_readout_names_the_mode_size_and_level() {
        let mut tool = SeaLevelTool::default();
        assert_eq!(sea_level_readout(&tool, SeaLevelMode::Fill, Unit::Meter), "Sea Level · drag a rectangle");
        tool.rect = Some((Vec2::ZERO, Vec2::new(24.0, 16.0)));
        tool.level = Some(12.5);
        assert_eq!(
            sea_level_readout(&tool, SeaLevelMode::Evaporate, Unit::Meter),
            "Evaporate · 24 m × 16 m · level 12.5 m · Enter to apply"
        );
    }
}

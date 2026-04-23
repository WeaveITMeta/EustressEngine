//! # Measure Tool (Phase 1 + Phase 2)
//!
//! Multi-mode measurement ModalTool. Modes:
//! - **Distance** (Phase 1): click 2 points → world-space length + Δx/y/z.
//! - **Angle** (Phase 2): click 3 points → angle at the middle point.
//! - **Area** (Phase 2): reports surface area of the selected entity's
//!   AABB faces (approx; exact surface-area of arbitrary meshes lands
//!   when mesh-edit mode does).
//! - **Volume** (Phase 2): reports the selected entity's AABB volume.
//!
//! Distance + Angle use viewport clicks. Area + Volume read the
//! current selection without any click — the Tool Options Bar displays
//! the readout immediately and updates on selection change.
//!
//! ## Why a modal tool (not a viewport overlay)
//!
//! Consistency — every other aim-then-click tool (Gap Fill, Resize
//! Align, Edge Align, Part Swap) is a ModalTool, so users already know
//! the interaction shape. Esc cancels, the Options Bar shows the step
//! label, Ctrl+Alt+<letter> activates.

use bevy::prelude::*;
use crate::modal_tool::{
    ModalTool, ToolContext, ToolOptionControl, ToolOptionKind,
    ToolStepResult, ViewportHit, ModalToolRegistry,
};
use crate::selection_box::Selected;

// ============================================================================
// Tool state
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasureMode {
    Distance,
    Angle,
    Area,
    Volume,
    Mass, // mass + center of mass, weighted by MaterialProperties.density
}

impl MeasureMode {
    pub fn as_str(self) -> &'static str {
        match self {
            MeasureMode::Distance => "distance",
            MeasureMode::Angle    => "angle",
            MeasureMode::Area     => "area",
            MeasureMode::Volume   => "volume",
            MeasureMode::Mass     => "mass",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "angle"  => MeasureMode::Angle,
            "area"   => MeasureMode::Area,
            "volume" => MeasureMode::Volume,
            "mass"   => MeasureMode::Mass,
            _        => MeasureMode::Distance,
        }
    }
}

pub struct MeasureDistanceTool {
    mode: MeasureMode,
    point_a: Option<Vec3>,
    point_b: Option<Vec3>,
    point_c: Option<Vec3>,
}

impl Default for MeasureDistanceTool {
    fn default() -> Self {
        Self {
            mode: MeasureMode::Distance,
            point_a: None,
            point_b: None,
            point_c: None,
        }
    }
}

impl ModalTool for MeasureDistanceTool {
    fn id(&self) -> &'static str { "measure_distance" }
    fn name(&self) -> &'static str { "Measure" }

    fn step_label(&self) -> String {
        match self.mode {
            MeasureMode::Distance => match (self.point_a, self.point_b) {
                (None, _)           => "click first point".to_string(),
                (Some(_), None)     => "click second point".to_string(),
                (Some(a), Some(b))  => format!("{:.3} studs", (b - a).length()),
            },
            MeasureMode::Angle => match (self.point_a, self.point_b, self.point_c) {
                (None, _, _)                => "click first leg endpoint".to_string(),
                (Some(_), None, _)          => "click vertex".to_string(),
                (Some(_), Some(_), None)    => "click second leg endpoint".to_string(),
                (Some(a), Some(b), Some(c)) => {
                    let ba = (a - b).normalize_or_zero();
                    let bc = (c - b).normalize_or_zero();
                    format!("{:.2}°", ba.dot(bc).clamp(-1.0, 1.0).acos().to_degrees())
                }
            },
            MeasureMode::Area   => "area — read selection".to_string(),
            MeasureMode::Volume => "volume — read selection".to_string(),
            MeasureMode::Mass   => "mass — read selection (uses material density)".to_string(),
        }
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        let mut controls = Vec::new();

        controls.push(ToolOptionControl {
            id: "mode".into(),
            label: "Mode".into(),
            kind: ToolOptionKind::Choice {
                options: vec![
                    "distance".into(), "angle".into(),
                    "area".into(), "volume".into(), "mass".into(),
                ],
                selected: self.mode.as_str().into(),
            },
            advanced: false,
        });

        let readout_text = match self.mode {
            MeasureMode::Distance => match (self.point_a, self.point_b) {
                (Some(a), Some(b)) => {
                    let d = (b - a).length();
                    let delta = b - a;
                    format!("{:.3} studs  (Δ {:.3}, {:.3}, {:.3})", d, delta.x.abs(), delta.y.abs(), delta.z.abs())
                }
                _ => "— click two points —".into(),
            },
            MeasureMode::Angle => match (self.point_a, self.point_b, self.point_c) {
                (Some(a), Some(b), Some(c)) => {
                    let ba = (a - b).normalize_or_zero();
                    let bc = (c - b).normalize_or_zero();
                    let rad = ba.dot(bc).clamp(-1.0, 1.0).acos();
                    format!("{:.2}°  ({:.4} rad)", rad.to_degrees(), rad)
                }
                _ => "— click leg, vertex, leg —".into(),
            },
            // Area / Volume / Mass: the real computation needs
            // `&mut World`, available only in `commit`. Show a hint;
            // user hits "Compute" to log the reading.
            MeasureMode::Area   => "select parts, click Compute to read area".into(),
            MeasureMode::Volume => "select parts, click Compute to read volume".into(),
            MeasureMode::Mass   => "select parts, click Compute to read mass + center-of-mass".into(),
        };

        controls.push(ToolOptionControl {
            id: "readout".into(),
            label: "Value".into(),
            kind: ToolOptionKind::Label { text: readout_text },
            advanced: false,
        });

        // Area/Volume/Mass modes get a Compute button — triggers a
        // commit that reads the selection inside `&mut World`, logs +
        // toasts the result. Distance/Angle don't need it (click-driven).
        if matches!(self.mode, MeasureMode::Area | MeasureMode::Volume | MeasureMode::Mass) {
            controls.push(ToolOptionControl {
                id: "compute".into(),
                label: "Compute".into(),
                kind: ToolOptionKind::Bool { value: false },
                advanced: false,
            });
        }

        controls.push(ToolOptionControl {
            id: "reset".into(),
            label: "Reset".into(),
            kind: ToolOptionKind::Bool { value: false },
            advanced: false,
        });

        controls
    }

    fn on_click(&mut self, hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        match self.mode {
            MeasureMode::Distance => {
                if self.point_a.is_none() {
                    self.point_a = Some(hit.hit_point);
                } else if self.point_b.is_none() {
                    self.point_b = Some(hit.hit_point);
                } else {
                    // Third click restarts the pair.
                    self.point_a = Some(hit.hit_point);
                    self.point_b = None;
                }
            }
            MeasureMode::Angle => {
                if self.point_a.is_none() {
                    self.point_a = Some(hit.hit_point);
                } else if self.point_b.is_none() {
                    self.point_b = Some(hit.hit_point);
                } else if self.point_c.is_none() {
                    self.point_c = Some(hit.hit_point);
                } else {
                    self.point_a = Some(hit.hit_point);
                    self.point_b = None;
                    self.point_c = None;
                }
            }
            // Area / Volume / Mass: no click-based interaction —
            // Compute button drives readout.
            MeasureMode::Area | MeasureMode::Volume | MeasureMode::Mass => {}
        }
        ToolStepResult::Continue
    }

    fn on_option_changed(&mut self, id: &str, value: &str, _ctx: &mut ToolContext) -> ToolStepResult {
        match id {
            "mode" => {
                self.mode = MeasureMode::from_str(value);
                // Reset points when switching modes so stale clicks don't
                // bleed across.
                self.point_a = None;
                self.point_b = None;
                self.point_c = None;
                ToolStepResult::Continue
            }
            "reset" if value == "true" => {
                self.point_a = None;
                self.point_b = None;
                self.point_c = None;
                ToolStepResult::Continue
            }
            "compute" if value == "true" => {
                // Route through `commit` which has `&mut World` — we
                // return Commit but override auto_exit so the tool
                // stays active for repeated reads.
                ToolStepResult::Commit
            }
            _ => ToolStepResult::Continue,
        }
    }

    fn auto_exit_on_commit(&self) -> bool { false }

    fn commit(&mut self, world: &mut World) {
        // For Distance / Angle modes, `commit` is unused — the click
        // flow suffices.
        if !matches!(
            self.mode,
            MeasureMode::Area | MeasureMode::Volume | MeasureMode::Mass,
        ) {
            return;
        }

        let mut count = 0usize;
        let mut surface_total = 0.0_f32;
        let mut volume_total  = 0.0_f32;
        let mut mass_total    = 0.0_f32;
        let mut com_weighted  = Vec3::ZERO;

        let mut q = world.query_filtered::<
            (
                &GlobalTransform,
                Option<&crate::classes::BasePart>,
                Option<&eustress_common::realism::materials::properties::MaterialProperties>,
            ),
            With<Selected>,
        >();
        for (gt, bp, mat) in q.iter(world) {
            let t = gt.compute_transform();
            let size = bp.map(|b| b.size).unwrap_or(t.scale);
            let vol = size.x * size.y * size.z;
            let sa  = 2.0 * (size.x * size.y + size.y * size.z + size.z * size.x);
            // Density fallback: 1000 kg/m³ (water-equivalent plastic)
            // for parts without `MaterialProperties`. Matches the
            // default physics behavior for Part.Plastic today.
            let density = mat.map(|m| m.density).unwrap_or(1000.0);
            let mass = density * vol;
            surface_total += sa;
            volume_total  += vol;
            mass_total    += mass;
            com_weighted  += t.translation * mass;
            count += 1;
        }

        if count == 0 {
            info!("📏 Measure {}: no selection", self.mode.as_str());
            return;
        }
        let com = if mass_total > 0.0 { com_weighted / mass_total } else { Vec3::ZERO };

        let msg = match self.mode {
            MeasureMode::Area => format!(
                "Area: {:.3} studs² across {} parts (AABB surface total)",
                surface_total, count
            ),
            MeasureMode::Volume => format!(
                "Volume: {:.3} studs³ across {} parts",
                volume_total, count
            ),
            MeasureMode::Mass => format!(
                "Mass {:.2} kg · CoM ({:.2}, {:.2}, {:.2}) across {} parts",
                mass_total, com.x, com.y, com.z, count
            ),
            _ => String::new(),
        };

        info!("📏 Measure {}: {}", self.mode.as_str(), msg);
        if let Some(mut notif) = world.get_resource_mut::<crate::notifications::NotificationManager>() {
            notif.info(msg);
        }
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.point_a = None;
        self.point_b = None;
        self.point_c = None;
    }
}


// ============================================================================
// Plugin
// ============================================================================

pub struct MeasureToolPlugin;

impl Plugin for MeasureToolPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_measure_tool);
    }
}

fn register_measure_tool(mut registry: ResMut<ModalToolRegistry>) {
    registry.register("measure_distance", || Box::new(MeasureDistanceTool::default()));
}

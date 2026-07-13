//! Modal tool: pick two parts + optional hit anchors for assembly mates.
//!
//! Flow:
//! 1. Ribbon starts tool with a mate kind (`mate_hinge`, …).
//! 2. Click part A (stores entity + local hit point as anchor).
//! 3. Click part B.
//! 4. Commits → `CadCreateMateEvent` with anchors in each part's local space.

use bevy::prelude::*;

use crate::cad_assembly::{CadCreateMateEvent, MateKind};
use crate::modal_tool::{
    ModalTool, ModalToolRegistry, ToolContext, ToolOptionControl, ToolOptionKind,
    ToolStepResult, ViewportHit,
};

pub struct MatePickTool {
    kind: MateKind,
    part_a: Option<Entity>,
    anchor_a_world: Option<Vec3>,
    part_b: Option<Entity>,
    anchor_b_world: Option<Vec3>,
}

impl Default for MatePickTool {
    fn default() -> Self {
        Self {
            kind: MateKind::Revolute,
            part_a: None,
            anchor_a_world: None,
            part_b: None,
            anchor_b_world: None,
        }
    }
}

impl MatePickTool {
    pub fn with_kind(kind: MateKind) -> Self {
        Self {
            kind,
            ..Default::default()
        }
    }
}

impl ModalTool for MatePickTool {
    fn id(&self) -> &'static str {
        match self.kind {
            MateKind::Coincident => "mate_coincident",
            MateKind::Revolute => "mate_revolute",
            MateKind::Prismatic => "mate_prismatic",
            MateKind::Ball => "mate_ball",
            MateKind::Distance => "mate_distance",
        }
    }

    fn name(&self) -> &'static str {
        match self.kind {
            MateKind::Coincident => "Mate: Weld",
            MateKind::Revolute => "Mate: Hinge",
            MateKind::Prismatic => "Mate: Slide",
            MateKind::Ball => "Mate: Ball",
            MateKind::Distance => "Mate: Distance",
        }
    }

    fn icon_path(&self) -> &'static str {
        "assets/icons/ui/link.svg"
    }

    fn step_label(&self) -> String {
        match (self.part_a, self.part_b) {
            (None, _) => "click first part (anchor = hit point)".into(),
            (Some(_), None) => "click second part — mate commits on pick".into(),
            (Some(_), Some(_)) => "committing…".into(),
        }
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "kind".into(),
                label: "Mate".into(),
                kind: ToolOptionKind::Choice {
                    options: vec![
                        "Weld".into(),
                        "Hinge".into(),
                        "Slide".into(),
                        "Ball".into(),
                        "Distance".into(),
                    ],
                    selected: match self.kind {
                        MateKind::Coincident => "Weld".into(),
                        MateKind::Revolute => "Hinge".into(),
                        MateKind::Prismatic => "Slide".into(),
                        MateKind::Ball => "Ball".into(),
                        MateKind::Distance => "Distance".into(),
                    },
                },
                advanced: false,
            },
            ToolOptionControl {
                id: "hint".into(),
                label: "".into(),
                kind: ToolOptionKind::Label {
                    text: "Anchors use the surface point you click on each part".into(),
                },
                advanced: false,
            },
        ]
    }

    fn on_option_changed(
        &mut self,
        id: &str,
        value: &str,
        _ctx: &mut ToolContext,
    ) -> ToolStepResult {
        if id == "kind" {
            self.kind = match value {
                "Weld" => MateKind::Coincident,
                "Slide" => MateKind::Prismatic,
                "Ball" => MateKind::Ball,
                "Distance" => MateKind::Distance,
                _ => MateKind::Revolute,
            };
        }
        ToolStepResult::Continue
    }

    fn on_click(&mut self, hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        let Some(entity) = hit.hit_entity else {
            return ToolStepResult::Continue;
        };
        match (self.part_a, self.part_b) {
            (None, _) => {
                self.part_a = Some(entity);
                self.anchor_a_world = Some(hit.hit_point);
                ToolStepResult::Continue
            }
            (Some(a), None) if a == entity => ToolStepResult::Continue,
            (Some(_), None) => {
                self.part_b = Some(entity);
                self.anchor_b_world = Some(hit.hit_point);
                // Commit on second pick — anchors already captured.
                ToolStepResult::Commit
            }
            (Some(_), Some(_)) => ToolStepResult::Commit,
        }
    }

    fn commit(&mut self, world: &mut World) {
        let (Some(a), Some(b)) = (self.part_a, self.part_b) else {
            return;
        };
        // Convert world hit points → local anchors using GlobalTransform.
        let ga = world.get::<GlobalTransform>(a).cloned();
        let gb = world.get::<GlobalTransform>(b).cloned();
        let anchor_a = match (ga, self.anchor_a_world) {
            (Some(gt), Some(world_pt)) => gt.affine().inverse().transform_point3(world_pt),
            _ => Vec3::ZERO,
        };
        let anchor_b = match (gb, self.anchor_b_world) {
            (Some(gt), Some(world_pt)) => gt.affine().inverse().transform_point3(world_pt),
            _ => Vec3::ZERO,
        };

        world.insert_resource(PendingMateAnchors {
            part_a: a,
            part_b: b,
            anchor_a,
            anchor_b,
        });
        // Bevy 0.19 Messages — same path as menu/engine-bridge commits.
        world.write_message(CadCreateMateEvent {
            kind: self.kind,
            part_a: Some(a),
            part_b: Some(b),
            anchor_a: Some(anchor_a),
            anchor_b: Some(anchor_b),
            record_undo: true,
        });
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        // No preview entities to despawn — picking state only.
        self.part_a = None;
        self.anchor_a_world = None;
        self.part_b = None;
        self.anchor_b_world = None;
    }
}

/// One-frame resource: anchors for the next mate create.
#[derive(Resource, Debug, Clone)]
pub struct PendingMateAnchors {
    pub part_a: Entity,
    pub part_b: Entity,
    pub anchor_a: Vec3,
    pub anchor_b: Vec3,
}

pub struct MateToolPlugin;

impl Plugin for MateToolPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_mate_tools);
    }
}

fn register_mate_tools(mut registry: ResMut<ModalToolRegistry>) {
    registry.register("mate_coincident", || {
        Box::new(MatePickTool::with_kind(MateKind::Coincident))
    });
    registry.register("mate_revolute", || {
        Box::new(MatePickTool::with_kind(MateKind::Revolute))
    });
    registry.register("mate_prismatic", || {
        Box::new(MatePickTool::with_kind(MateKind::Prismatic))
    });
    registry.register("mate_ball", || {
        Box::new(MatePickTool::with_kind(MateKind::Ball))
    });
    registry.register("mate_distance", || {
        Box::new(MatePickTool::with_kind(MateKind::Distance))
    });
    info!("🔗 Registered mate pick tools");
}

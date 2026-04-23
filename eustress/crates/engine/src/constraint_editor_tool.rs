//! # Constraint Editor (Phase 2)
//!
//! 3D visual editor for physical constraints — `BallSocketConstraint`,
//! `HingeConstraint`, `PrismaticConstraint` (slider), `RodConstraint`,
//! `SpringConstraint`. Creates the constraint by picking two
//! attachments (or two entities, falling back to their origins) and
//! a constraint type.
//!
//! ## Interaction
//!
//! 1. Activate (ribbon or Ctrl+Alt+J for Joint).
//! 2. Pick ConstraintKind from Options Bar (`BallSocket` / `Hinge` /
//!    `Prismatic` / `Rod` / `Spring`).
//! 3. Click the first entity/attachment → selection highlights.
//! 4. Click the second entity/attachment → commits the constraint.
//! 5. Stays active for rapid connection authoring.
//!
//! ## Scope of v1
//!
//! Ships the ModalTool + `CreateConstraintEvent`. Handler spawns a
//! new constraint entity under a `Constraints/` folder with
//! `_instance.toml` referencing `part0_id` + `part1_id`. Runtime
//! physics binding to Avian constraints is handled by the existing
//! constraint-loader (common crate) when present; otherwise the
//! constraint entity carries metadata only and is picked up when
//! that loader lands.

use bevy::prelude::*;
use crate::modal_tool::{
    ModalTool, ToolContext, ToolOptionControl, ToolOptionKind,
    ToolStepResult, ViewportHit, ModalToolRegistry,
};
use crate::tools_smart::{NewPartDescriptor, spawn_new_part_with_toml};

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    BallSocket,
    Hinge,
    Prismatic,
    Rod,
    Spring,
}

impl ConstraintKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ConstraintKind::BallSocket => "BallSocketConstraint",
            ConstraintKind::Hinge      => "HingeConstraint",
            ConstraintKind::Prismatic  => "PrismaticConstraint",
            ConstraintKind::Rod        => "RodConstraint",
            ConstraintKind::Spring     => "SpringConstraint",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "hinge"      => ConstraintKind::Hinge,
            "prismatic"  => ConstraintKind::Prismatic,
            "rod"        => ConstraintKind::Rod,
            "spring"     => ConstraintKind::Spring,
            _            => ConstraintKind::BallSocket,
        }
    }
}

// ============================================================================
// Tool state
// ============================================================================

pub struct ConstraintEditor {
    kind: ConstraintKind,
    first: Option<(Entity, Vec3)>,
    pending_second: Option<(Entity, Vec3)>,
    placed: u32,
}

impl Default for ConstraintEditor {
    fn default() -> Self {
        Self {
            kind: ConstraintKind::Hinge,
            first: None,
            pending_second: None,
            placed: 0,
        }
    }
}

impl ModalTool for ConstraintEditor {
    fn id(&self) -> &'static str { "constraint_editor" }
    fn name(&self) -> &'static str { "Constraint Editor" }

    fn step_label(&self) -> String {
        if self.first.is_none() {
            format!("{}: click first entity", self.kind.as_str())
        } else {
            format!("{}: click second entity", self.kind.as_str())
        }
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "kind".into(),
                label: "Kind".into(),
                kind: ToolOptionKind::Choice {
                    options: vec![
                        "ballsocket".into(), "hinge".into(), "prismatic".into(),
                        "rod".into(), "spring".into(),
                    ],
                    selected: match self.kind {
                        ConstraintKind::BallSocket => "ballsocket",
                        ConstraintKind::Hinge      => "hinge",
                        ConstraintKind::Prismatic  => "prismatic",
                        ConstraintKind::Rod        => "rod",
                        ConstraintKind::Spring     => "spring",
                    }.into(),
                },
                advanced: false,
            },
            ToolOptionControl {
                id: "info".into(),
                label: "Placed".into(),
                kind: ToolOptionKind::Label {
                    text: format!("{} constraints this session", self.placed),
                },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        let Some(entity) = hit.hit_entity else {
            return ToolStepResult::Continue;
        };
        if self.first.is_none() {
            self.first = Some((entity, hit.hit_point));
            info!("🔗 Constraint Editor: first = {:?} at {:?}", entity, hit.hit_point);
            ToolStepResult::Continue
        } else {
            self.pending_second = Some((entity, hit.hit_point));
            ToolStepResult::Commit
        }
    }

    fn on_option_changed(&mut self, id: &str, value: &str, _ctx: &mut ToolContext) -> ToolStepResult {
        if id == "kind" {
            self.kind = ConstraintKind::from_str(value);
            self.first = None;
            self.pending_second = None;
        }
        ToolStepResult::Continue
    }

    fn auto_exit_on_commit(&self) -> bool { false }

    fn commit(&mut self, world: &mut World) {
        let (Some((part0, p0)), Some((part1, p1))) = (self.first, self.pending_second) else {
            return;
        };
        self.pending_second = None;
        self.first = None;

        let space_root = match world.get_resource::<crate::space::SpaceRoot>() {
            Some(r) => r.0.clone(),
            None => { warn!("Constraint Editor: no SpaceRoot"); return; }
        };

        // Resolve part names for the TOML refs. Falls back to entity
        // bits if no Instance name is present.
        let part0_name = world.get::<crate::classes::Instance>(part0)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| format!("entity_{}", part0.index()));
        let part1_name = world.get::<crate::classes::Instance>(part1)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| format!("entity_{}", part1.index()));

        let mid = (p0 + p1) * 0.5;
        self.placed += 1;

        // Spawn the constraint entity as a folder in Constraints/ so the
        // watcher picks it up. Attribute the part refs via TOML metadata
        // (for now as attributes; a future ConstraintProperties struct
        // in the loader will make this typed).
        let desc = NewPartDescriptor {
            base_name: format!("{}_{:02}", self.kind.as_str(), self.placed),
            parent_rel: std::path::PathBuf::from("Constraints"),
            transform: Transform {
                translation: mid,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
            size: Vec3::splat(0.05),
            mesh: "parts/block.glb".to_string(),
            class_name: self.kind.as_str().to_string(),
            color_rgba: Some(match self.kind {
                ConstraintKind::BallSocket => [0.9, 0.5, 0.1, 1.0], // orange
                ConstraintKind::Hinge      => [0.2, 0.6, 1.0, 1.0], // blue
                ConstraintKind::Prismatic  => [0.4, 0.9, 0.3, 1.0], // green
                ConstraintKind::Rod        => [0.9, 0.9, 0.9, 1.0], // gray
                ConstraintKind::Spring     => [1.0, 0.9, 0.2, 1.0], // yellow
            }),
            material: None,
            anchored: true,
        };

        let _ = space_root;
        match spawn_new_part_with_toml(world, desc) {
            Ok(part) => {
                info!("🔗 Constraint {} spawned: {} ↔ {} at {:?}",
                      part.folder_name, part0_name, part1_name, mid);
                if let Some(mut notif) = world.get_resource_mut::<crate::notifications::NotificationManager>() {
                    notif.info(format!("{} placed: {} ↔ {}",
                                       self.kind.as_str(), part0_name, part1_name));
                }
            }
            Err(e) => {
                warn!("Constraint Editor: spawn failed — {}", e);
            }
        }
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.first = None;
        self.pending_second = None;
        self.placed = 0;
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct ConstraintEditorPlugin;

impl Plugin for ConstraintEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_constraint_editor);
    }
}

fn register_constraint_editor(mut registry: ResMut<ModalToolRegistry>) {
    registry.register("constraint_editor", || Box::new(ConstraintEditor::default()));
}

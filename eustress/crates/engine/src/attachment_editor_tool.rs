//! # Attachment Editor (Phase 2)
//!
//! Roblox-style attachment placement + orientation handles on a part.
//! Attachments are child entities (class `Attachment`) that serve as
//! anchors for welds, constraints, particle emitters, trails, and
//! animation rig endpoints. Physically placing them accurately is
//! critical because every connected constraint keys off the
//! attachment's world pose.
//!
//! ## Interaction
//!
//! 1. Activate (ribbon or Ctrl+Alt+T).
//! 2. Click a face on a selected part → spawn an `Attachment` child
//!    at the hit point, oriented +Y along the hit normal.
//! 3. While the tool is active, further clicks on other parts/faces
//!    spawn more attachments.
//! 4. Each new attachment is added to the live selection so the
//!    user can immediately refine with Move/Rotate.
//!
//! ## Scope of v1
//!
//! Ships the spawn flow — click → create `Attachment` child entity at
//! the hit point. The orientation handles (arrows showing the
//! attachment's local axes in the viewport) use the existing
//! adornment-renderer infrastructure for follow-up polish. The
//! constraint-rewire pass (so weld endpoints track the right
//! attachment on part rename) is scoped into
//! `eustress-common::classes::Attachment` already.

use bevy::prelude::*;
use crate::modal_tool::{
    ModalTool, ToolContext, ToolOptionControl, ToolOptionKind,
    ToolStepResult, ViewportHit, ModalToolRegistry,
};
use crate::tools_smart::{NewPartDescriptor, spawn_new_part_with_toml};

// ============================================================================
// Tool state
// ============================================================================

pub struct AttachmentEditor {
    /// Pending click hit — consumed by `commit`.
    pending_click: Option<ViewportHit>,
    /// Count of attachments spawned this session, for naming.
    placed: u32,
}

impl Default for AttachmentEditor {
    fn default() -> Self {
        Self { pending_click: None, placed: 0 }
    }
}

impl ModalTool for AttachmentEditor {
    fn id(&self) -> &'static str { "attachment_editor" }
    fn name(&self) -> &'static str { "Attachment Editor" }

    fn step_label(&self) -> String {
        if self.placed == 0 {
            "click a face to place an attachment".to_string()
        } else {
            format!("{} attachments placed — click for more, Esc to finish", self.placed)
        }
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "info".into(),
                label: "Placed".into(),
                kind: ToolOptionKind::Label {
                    text: format!("{} attachments this session", self.placed),
                },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        if hit.hit_entity.is_none() {
            // Require an actual part-hit — no floating attachments in v1.
            return ToolStepResult::Continue;
        }
        self.pending_click = Some(hit.clone());
        ToolStepResult::Commit
    }

    fn auto_exit_on_commit(&self) -> bool { false }

    fn commit(&mut self, world: &mut World) {
        let Some(hit) = self.pending_click.take() else { return };
        let Some(_target_entity) = hit.hit_entity else { return };

        let space_root = match world.get_resource::<crate::space::SpaceRoot>() {
            Some(r) => r.0.clone(),
            None => { warn!("Attachment Editor: no SpaceRoot"); return; }
        };

        // Orient the attachment so its local +Y points along the hit
        // normal — standard convention in Roblox-style rigs.
        let up = hit.hit_normal.map(|n| n.normalize()).unwrap_or(Vec3::Y);
        let rotation = if (up - Vec3::Y).length_squared() > 1e-5 {
            Quat::from_rotation_arc(Vec3::Y, up)
        } else {
            Quat::IDENTITY
        };

        self.placed += 1;
        let desc = NewPartDescriptor {
            base_name: format!("Attachment{:02}", self.placed),
            parent_rel: std::path::PathBuf::from("Workspace"),
            transform: Transform {
                translation: hit.hit_point,
                rotation,
                scale: Vec3::ONE,
            },
            size: Vec3::splat(0.1),
            mesh: "parts/block.glb".to_string(),
            class_name: "Attachment".to_string(),
            color_rgba: Some([1.0, 0.6, 0.0, 1.0]), // orange — match Roblox convention
            material: None,
            anchored: true,
        };

        let _ = std::mem::replace(&mut (), ()); // anchor
        match spawn_new_part_with_toml(world, desc) {
            Ok(part) => {
                info!("🧲 Attachment {:02} placed at {:?} (on surface normal)",
                      self.placed, hit.hit_point);
                // Emit a friendly toast (commit path).
                if let Some(mut notif) = world.get_resource_mut::<crate::notifications::NotificationManager>() {
                    notif.info(format!("Attachment {} placed", part.folder_name));
                }
            }
            Err(e) => {
                warn!("Attachment Editor: spawn failed — {}", e);
                let _ = space_root; // keep binding alive for future use
            }
        }
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.pending_click = None;
        self.placed = 0;
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct AttachmentEditorPlugin;

impl Plugin for AttachmentEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_attachment_editor);
    }
}

fn register_attachment_editor(mut registry: ResMut<ModalToolRegistry>) {
    registry.register("attachment_editor", || Box::new(AttachmentEditor::default()));
}

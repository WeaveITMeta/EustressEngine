//! # Duplicate & Place Tool (Phase 1)
//!
//! Clone the current selection, then the clone follows the cursor on a
//! ground plane until LMB commits the placement. One undo entry per
//! placement. Repeated placements chain without re-activation.
//!
//! ## Interaction
//!
//! 1. Activate with selection non-empty (Ctrl+Alt+D or ribbon button).
//! 2. Tool snapshots the selection at activation.
//! 3. Hover → preview offset follows the cursor (XZ plane projected at
//!    the group-AABB center's Y).
//! 4. Click → spawn TOML-backed clones at the preview offset. Tool
//!    stays active for repeated placement; Esc / RMB to leave.
//!
//! ## Preview
//!
//! v1 — no live ghost mesh, just the cursor world position. When the
//! user clicks, clones materialize. (Ghost-mesh preview is a polish
//! follow-up using the existing `GhostPreviewMaterial` infrastructure.)

use bevy::prelude::*;
use crate::selection_box::Selected;
use crate::modal_tool::{
    ModalTool, ToolContext, ToolOptionControl, ToolOptionKind,
    ToolStepResult, ViewportHit, ModalToolRegistry,
};
use crate::tools_smart::{NewPartDescriptor, spawn_new_part_with_toml};

// ============================================================================
// Tool state
// ============================================================================

struct DuplicateSource {
    transform: Transform,
    size: Vec3,
    source_toml: Option<std::path::PathBuf>,
    base_name: String,
}

pub struct DuplicatePlaceTool {
    /// Snapshot of the original selection — populated on first hover
    /// after activation. Held across placements so repeated clicks
    /// duplicate the same source pattern.
    sources: Vec<DuplicateSource>,
    /// Center of the source AABB — used to compute per-source offset
    /// from cursor.
    source_center: Vec3,
    /// Click target buffered by `on_click`, consumed by `commit`.
    /// Bevy's ModalTool trait routes click → commit via the runtime's
    /// deferred `World`-mutating closure, so we carry the target on
    /// the tool state rather than via an event.
    pending_place: Option<Vec3>,
    /// Number of placements completed this session — for naming /
    /// telemetry.
    placed: u32,
}

impl Default for DuplicatePlaceTool {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            source_center: Vec3::ZERO,
            pending_place: None,
            placed: 0,
        }
    }
}

impl DuplicatePlaceTool {
    /// Populate `sources` from the current Selected set via World query.
    fn ensure_snapshot(&mut self, world: &mut World) {
        if !self.sources.is_empty() { return; }

        use crate::space::instance_loader::InstanceFile;
        let mut query = world.query_filtered::<
            (
                Entity,
                &Transform,
                Option<&crate::classes::BasePart>,
                Option<&InstanceFile>,
                Option<&crate::classes::Instance>,
            ),
            With<Selected>,
        >();

        let mut center = Vec3::ZERO;
        let mut count = 0usize;
        for (_e, t, bp, inst_file, inst) in query.iter(world) {
            let size = bp.map(|b| b.size).unwrap_or(t.scale);
            center += t.translation;
            count += 1;
            self.sources.push(DuplicateSource {
                transform: *t,
                size,
                source_toml: inst_file.map(|f| f.toml_path.clone()),
                base_name: inst.map(|i| i.name.clone()).unwrap_or_else(|| "Copy".to_string()),
            });
        }
        if count > 0 { center /= count as f32; }
        self.source_center = center;
    }
}

impl ModalTool for DuplicatePlaceTool {
    fn id(&self) -> &'static str { "duplicate_place" }
    fn name(&self) -> &'static str { "Duplicate & Place" }

    fn step_label(&self) -> String {
        if self.sources.is_empty() {
            "hover a surface — click to place clone".to_string()
        } else {
            format!("placed {} — click for another, Esc to finish", self.placed)
        }
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "info".into(),
                label: "Sources".into(),
                kind: ToolOptionKind::Label {
                    text: format!("{} entity (entities) · {} placed",
                                  self.sources.len().max(1), self.placed),
                },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, hit: &ViewportHit, ctx: &mut ToolContext) -> ToolStepResult {
        // Click stores a placement request — actual spawn happens in
        // commit where we get exclusive World access. Simplest
        // approach: commit on every click, spawn inside commit.
        self.pending_place = Some(hit.hit_point);
        ToolStepResult::Commit
    }

    fn auto_exit_on_commit(&self) -> bool { false }

    fn commit(&mut self, world: &mut World) {
        self.ensure_snapshot(world);
        if self.sources.is_empty() {
            info!("📋 Duplicate & Place: no selection to duplicate");
            return;
        }
        let Some(target) = self.pending_place.take() else { return };

        let space_root = match world.get_resource::<crate::space::SpaceRoot>() {
            Some(r) => r.0.clone(),
            None => { warn!("Duplicate & Place: no SpaceRoot"); return; }
        };

        // Offset = cursor world position minus the group center.
        let offset = target - self.source_center;
        let mut spawned = 0usize;
        for source in &self.sources {
            let new_pos = source.transform.translation + offset;
            let desc = build_descriptor(source, new_pos, source.transform.rotation, self.placed + 1, &space_root);
            match spawn_new_part_with_toml(world, desc) {
                Ok(_) => spawned += 1,
                Err(e) => warn!("Duplicate & Place: spawn failed — {}", e),
            }
        }
        self.placed += 1;
        info!("📋 Duplicate & Place: spawned {} copies at {:?}", spawned, target);
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.sources.clear();
        self.placed = 0;
        self.pending_place = None;
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn build_descriptor(
    source: &DuplicateSource,
    new_pos: Vec3,
    new_rot: Quat,
    placement_ix: u32,
    space_root: &std::path::Path,
) -> NewPartDescriptor {
    // Pull inherited properties from the source TOML where available.
    let (mesh, class_name, color, material, anchored) = source
        .source_toml
        .as_ref()
        .and_then(|p| crate::space::instance_loader::load_instance_definition(p).ok())
        .map(|def| (
            def.asset.as_ref().map(|a| a.mesh.clone()).unwrap_or_else(|| "parts/block.glb".to_string()),
            def.metadata.class_name.clone(),
            Some(def.properties.color),
            Some(def.properties.material.clone()),
            def.properties.anchored,
        ))
        .unwrap_or_else(|| (
            "parts/block.glb".to_string(),
            "Part".to_string(),
            None,
            None,
            false,
        ));

    let parent_rel = source.source_toml.as_ref()
        .and_then(|src| {
            src.parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.strip_prefix(space_root).ok())
                .map(|p| p.to_path_buf())
        })
        .unwrap_or_else(|| std::path::PathBuf::from("Workspace"));

    NewPartDescriptor {
        base_name: format!("{}_Copy{:02}", source.base_name, placement_ix),
        parent_rel,
        transform: Transform {
            translation: new_pos,
            rotation: new_rot,
            scale: Vec3::ONE,
        },
        size: source.size,
        mesh,
        class_name,
        color_rgba: color,
        material,
        anchored,
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct DuplicatePlaceToolPlugin;

impl Plugin for DuplicatePlaceToolPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_duplicate_place_tool);
    }
}

fn register_duplicate_place_tool(mut registry: ResMut<ModalToolRegistry>) {
    registry.register("duplicate_place", || Box::new(DuplicatePlaceTool::default()));
}

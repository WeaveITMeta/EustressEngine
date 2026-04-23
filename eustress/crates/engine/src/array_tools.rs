//! # Array Tools (Phase 1)
//!
//! Linear / Radial / Grid array — clone the current selection into a
//! pattern. Ships as three sibling `ModalTool` implementations on the
//! CAD-tab Pattern ribbon group.
//!
//! ## Shared approach
//!
//! Every array tool:
//! 1. Snapshots the selection at activation (via `world.query_filtered`).
//! 2. Clones each selected entity's `InstanceDefinition` from disk so
//!    the copies inherit class, mesh, material, color, and anchored
//!    state faithfully. Falls back to a bare Part when the source is
//!    not TOML-backed.
//! 3. Applies a per-copy transform (translation/rotation) computed from
//!    the tool's parameters.
//! 4. Writes each copy via `spawn_new_part_with_toml` which handles
//!    folder creation, TOML write, ECS spawn, SpaceFileRegistry insert,
//!    and a `SpawnFolders` undo entry.
//!
//! ## Tool dispatch
//!
//! - `linear_array`: N copies along a step vector.
//! - `radial_array`: N copies around a pivot axis + angle.
//! - `grid_array`: Nx × Ny × Nz copies on three orthogonal step vectors.
//!
//! All three commit on an explicit "Apply" toggle in the Options Bar,
//! matching the Model Reflect pattern — no viewport click is needed;
//! the tool sits active while the user dials in parameters.

use bevy::prelude::*;
use crate::selection_box::Selected;
use crate::modal_tool::{
    ModalTool, ToolContext, ToolOptionControl, ToolOptionKind,
    ToolStepResult, ViewportHit, ModalToolRegistry,
};
use crate::tools_smart::{NewPartDescriptor, spawn_new_part_with_toml};

// ============================================================================
// Shared snapshot type
// ============================================================================

/// One entry per selected entity at tool activation — captures enough
/// to clone the entity via `spawn_new_part_with_toml` without hitting
/// the ECS again during commit.
struct ArraySource {
    transform: Transform,
    size: Vec3,
    /// Source TOML for faithful clone. None falls back to a bare Part.
    source_toml: Option<std::path::PathBuf>,
    base_name: String,
}

/// Snapshot the current selection. Called at the top of `commit`.
fn snapshot_selection(world: &mut World) -> Vec<ArraySource> {
    use crate::space::instance_loader::InstanceFile;
    let mut out = Vec::new();
    let mut q = world.query_filtered::<
        (
            Entity,
            &Transform,
            Option<&crate::classes::BasePart>,
            Option<&InstanceFile>,
            Option<&crate::classes::Instance>,
        ),
        With<Selected>,
    >();
    for (_e, t, bp, inst_file, inst) in q.iter(world) {
        let size = bp.map(|b| b.size).unwrap_or(t.scale);
        out.push(ArraySource {
            transform: *t,
            size,
            source_toml: inst_file.map(|f| f.toml_path.clone()),
            base_name: inst.map(|i| i.name.clone()).unwrap_or_else(|| "Array".to_string()),
        });
    }
    out
}

/// Build a `NewPartDescriptor` for one copy given its computed world
/// transform. Loads the source TOML when available so color / material /
/// class / anchored propagate.
fn descriptor_for_copy(
    source: &ArraySource,
    new_pos: Vec3,
    new_rot: Quat,
    suffix: &str,
    space_root: &std::path::Path,
) -> NewPartDescriptor {
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

    // Parent folder: same service as the source if we can resolve it,
    // else Workspace. Matches Model Reflect's behaviour.
    let parent_rel = source.source_toml.as_ref()
        .and_then(|src| {
            src.parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.strip_prefix(space_root).ok())
                .map(|p| p.to_path_buf())
        })
        .unwrap_or_else(|| std::path::PathBuf::from("Workspace"));

    NewPartDescriptor {
        base_name: format!("{}_{}", source.base_name, suffix),
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
// Linear Array
// ============================================================================

/// Clone the selection N times along a step vector. Step is expressed
/// in world units on each axis; `count` is the total number of parts
/// (including the original — so count=5 adds 4 copies).
pub struct LinearArray {
    pub count: u32,
    pub step_x: f32,
    pub step_y: f32,
    pub step_z: f32,
    ready_to_commit: bool,
}

impl Default for LinearArray {
    fn default() -> Self {
        Self {
            count: 5,
            step_x: 2.0,
            step_y: 0.0,
            step_z: 0.0,
            ready_to_commit: false,
        }
    }
}

impl ModalTool for LinearArray {
    fn id(&self) -> &'static str { "linear_array" }
    fn name(&self) -> &'static str { "Linear Array" }

    fn step_label(&self) -> String {
        format!("count {} · step ({:.2}, {:.2}, {:.2})", self.count, self.step_x, self.step_y, self.step_z)
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "count".into(), label: "Count".into(),
                kind: ToolOptionKind::Number { value: self.count as f32, min: 2.0, max: 100.0, step: 1.0, unit: "".into() },
                advanced: false,
            },
            ToolOptionControl {
                id: "step_x".into(), label: "Step X".into(),
                kind: ToolOptionKind::Number { value: self.step_x, min: -100.0, max: 100.0, step: 0.1, unit: "studs".into() },
                advanced: false,
            },
            ToolOptionControl {
                id: "step_y".into(), label: "Step Y".into(),
                kind: ToolOptionKind::Number { value: self.step_y, min: -100.0, max: 100.0, step: 0.1, unit: "studs".into() },
                advanced: false,
            },
            ToolOptionControl {
                id: "step_z".into(), label: "Step Z".into(),
                kind: ToolOptionKind::Number { value: self.step_z, min: -100.0, max: 100.0, step: 0.1, unit: "studs".into() },
                advanced: false,
            },
            ToolOptionControl {
                id: "commit".into(), label: "Apply".into(),
                kind: ToolOptionKind::Bool { value: self.ready_to_commit },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, _hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        ToolStepResult::Continue
    }

    fn on_option_changed(&mut self, id: &str, value: &str, _ctx: &mut ToolContext) -> ToolStepResult {
        match id {
            "count"  => { if let Ok(v) = value.parse::<f32>() { self.count = (v as u32).clamp(2, 100); } }
            "step_x" => { if let Ok(v) = value.parse::<f32>() { self.step_x = v; } }
            "step_y" => { if let Ok(v) = value.parse::<f32>() { self.step_y = v; } }
            "step_z" => { if let Ok(v) = value.parse::<f32>() { self.step_z = v; } }
            "commit" => {
                if value == "true" {
                    self.ready_to_commit = true;
                    return ToolStepResult::Commit;
                }
            }
            _ => {}
        }
        ToolStepResult::Continue
    }

    fn commit(&mut self, world: &mut World) {
        let sources = snapshot_selection(world);
        if sources.is_empty() {
            info!("↺ Linear Array: no selection");
            return;
        }
        let space_root = match world.get_resource::<crate::space::SpaceRoot>() {
            Some(r) => r.0.clone(),
            None => { warn!("Linear Array: no SpaceRoot"); return; }
        };

        let step = Vec3::new(self.step_x, self.step_y, self.step_z);
        let mut spawned = 0usize;

        for source in &sources {
            for i in 1..self.count {
                let new_pos = source.transform.translation + step * i as f32;
                let desc = descriptor_for_copy(
                    source, new_pos, source.transform.rotation,
                    &format!("L{:02}", i), &space_root,
                );
                match spawn_new_part_with_toml(world, desc) {
                    Ok(_) => spawned += 1,
                    Err(e) => warn!("Linear Array: spawn failed — {}", e),
                }
            }
        }
        info!("↺ Linear Array: spawned {} copies across {} sources (count={}, step={:?})",
              spawned, sources.len(), self.count, step);
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.ready_to_commit = false;
    }
}

// ============================================================================
// Radial Array
// ============================================================================

/// Clone the selection N times around a pivot axis. Pivot point defaults
/// to the world origin; axis to Y (vertical). Each copy rotates by
/// `angle_deg / (count - 1)` from the previous (or `/ count` for a full
/// 360° sweep, which is the common case).
pub struct RadialArray {
    pub count: u32,
    pub angle_deg: f32,
    /// Axis choice: "x" | "y" | "z". Y is default — horizontal rotation
    /// around vertical axis, the most common floor-plan use.
    pub axis: String,
    /// Pivot point in world coords. Defaults to origin; could be
    /// enhanced to pick from the selection's AABB center in v2.
    pub pivot_x: f32,
    pub pivot_y: f32,
    pub pivot_z: f32,
    ready_to_commit: bool,
}

impl Default for RadialArray {
    fn default() -> Self {
        Self {
            count: 8,
            angle_deg: 360.0,
            axis: "y".to_string(),
            pivot_x: 0.0, pivot_y: 0.0, pivot_z: 0.0,
            ready_to_commit: false,
        }
    }
}

impl ModalTool for RadialArray {
    fn id(&self) -> &'static str { "radial_array" }
    fn name(&self) -> &'static str { "Radial Array" }

    fn step_label(&self) -> String {
        format!("count {} · {}° around {}-axis", self.count, self.angle_deg, self.axis.to_uppercase())
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "count".into(), label: "Count".into(),
                kind: ToolOptionKind::Number { value: self.count as f32, min: 2.0, max: 360.0, step: 1.0, unit: "".into() },
                advanced: false,
            },
            ToolOptionControl {
                id: "angle".into(), label: "Sweep".into(),
                kind: ToolOptionKind::Number { value: self.angle_deg, min: -360.0, max: 360.0, step: 1.0, unit: "°".into() },
                advanced: false,
            },
            ToolOptionControl {
                id: "axis".into(), label: "Axis".into(),
                kind: ToolOptionKind::Choice { options: vec!["x".into(), "y".into(), "z".into()], selected: self.axis.clone() },
                advanced: false,
            },
            ToolOptionControl {
                id: "pivot_x".into(), label: "Pivot X".into(),
                kind: ToolOptionKind::Number { value: self.pivot_x, min: -1000.0, max: 1000.0, step: 0.1, unit: "studs".into() },
                advanced: true,
            },
            ToolOptionControl {
                id: "pivot_y".into(), label: "Pivot Y".into(),
                kind: ToolOptionKind::Number { value: self.pivot_y, min: -1000.0, max: 1000.0, step: 0.1, unit: "studs".into() },
                advanced: true,
            },
            ToolOptionControl {
                id: "pivot_z".into(), label: "Pivot Z".into(),
                kind: ToolOptionKind::Number { value: self.pivot_z, min: -1000.0, max: 1000.0, step: 0.1, unit: "studs".into() },
                advanced: true,
            },
            ToolOptionControl {
                id: "commit".into(), label: "Apply".into(),
                kind: ToolOptionKind::Bool { value: self.ready_to_commit },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, _hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        ToolStepResult::Continue
    }

    fn on_option_changed(&mut self, id: &str, value: &str, _ctx: &mut ToolContext) -> ToolStepResult {
        match id {
            "count"   => { if let Ok(v) = value.parse::<f32>() { self.count = (v as u32).clamp(2, 360); } }
            "angle"   => { if let Ok(v) = value.parse::<f32>() { self.angle_deg = v.clamp(-360.0, 360.0); } }
            "axis"    => { self.axis = value.to_string(); }
            "pivot_x" => { if let Ok(v) = value.parse::<f32>() { self.pivot_x = v; } }
            "pivot_y" => { if let Ok(v) = value.parse::<f32>() { self.pivot_y = v; } }
            "pivot_z" => { if let Ok(v) = value.parse::<f32>() { self.pivot_z = v; } }
            "commit" => {
                if value == "true" {
                    self.ready_to_commit = true;
                    return ToolStepResult::Commit;
                }
            }
            _ => {}
        }
        ToolStepResult::Continue
    }

    fn commit(&mut self, world: &mut World) {
        let sources = snapshot_selection(world);
        if sources.is_empty() {
            info!("↺ Radial Array: no selection");
            return;
        }
        let space_root = match world.get_resource::<crate::space::SpaceRoot>() {
            Some(r) => r.0.clone(),
            None => { warn!("Radial Array: no SpaceRoot"); return; }
        };

        let axis_vec = match self.axis.as_str() {
            "x" => Vec3::X,
            "z" => Vec3::Z,
            _   => Vec3::Y,
        };
        // Pivot resolution — if the user hasn't overridden the pivot
        // (still at the default `[0, 0, 0]`) AND the selection isn't
        // near the world origin, snap the pivot to the selection
        // center so the radial copies orbit the selection instead of
        // world-origin. The old behavior (orbit world-origin by
        // default) was the "wrong UX" the user called out 2026-04-23
        // — nobody radially-arrays around (0, 0, 0) without knowing
        // they need to enter a pivot manually first.
        let user_pivot_set = self.pivot_x.abs() > 1e-4
            || self.pivot_y.abs() > 1e-4
            || self.pivot_z.abs() > 1e-4;
        let pivot = if user_pivot_set {
            Vec3::new(self.pivot_x, self.pivot_y, self.pivot_z)
        } else {
            let mut center = Vec3::ZERO;
            for src in &sources { center += src.transform.translation; }
            if sources.is_empty() { Vec3::ZERO } else { center / sources.len() as f32 }
        };

        // For a full 360° sweep, divide by count so the last copy doesn't
        // land on top of the first. For a partial sweep, divide by
        // (count-1) so the endpoints land exactly at angle 0 and angle.
        let step_rad = if (self.angle_deg.abs() - 360.0).abs() < 0.01 {
            self.angle_deg.to_radians() / self.count as f32
        } else {
            self.angle_deg.to_radians() / (self.count - 1) as f32
        };

        let mut spawned = 0usize;
        for source in &sources {
            for i in 1..self.count {
                let rot = Quat::from_axis_angle(axis_vec, step_rad * i as f32);
                let offset = source.transform.translation - pivot;
                let new_pos = pivot + rot * offset;
                let new_rot = rot * source.transform.rotation;

                let desc = descriptor_for_copy(
                    source, new_pos, new_rot,
                    &format!("R{:02}", i), &space_root,
                );
                match spawn_new_part_with_toml(world, desc) {
                    Ok(_) => spawned += 1,
                    Err(e) => warn!("Radial Array: spawn failed — {}", e),
                }
            }
        }
        info!("↺ Radial Array: spawned {} copies ({} sources, {}° / {} axis)",
              spawned, sources.len(), self.angle_deg, self.axis);
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.ready_to_commit = false;
    }
}

// ============================================================================
// Grid Array
// ============================================================================

/// Clone the selection into a 3D grid. Total copies = `count_x * count_y
/// * count_z - 1` (subtracting the original). Each copy offsets by
/// `step_x * i + step_y * j + step_z * k` along world axes.
pub struct GridArray {
    pub count_x: u32,
    pub count_y: u32,
    pub count_z: u32,
    pub step_x: f32,
    pub step_y: f32,
    pub step_z: f32,
    ready_to_commit: bool,
}

impl Default for GridArray {
    fn default() -> Self {
        Self {
            count_x: 3, count_y: 1, count_z: 3,
            step_x: 2.0, step_y: 2.0, step_z: 2.0,
            ready_to_commit: false,
        }
    }
}

impl ModalTool for GridArray {
    fn id(&self) -> &'static str { "grid_array" }
    fn name(&self) -> &'static str { "Grid Array" }

    fn step_label(&self) -> String {
        format!("{}×{}×{} @ step ({:.1}, {:.1}, {:.1})",
                self.count_x, self.count_y, self.count_z,
                self.step_x, self.step_y, self.step_z)
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "count_x".into(), label: "Count X".into(),
                kind: ToolOptionKind::Number { value: self.count_x as f32, min: 1.0, max: 50.0, step: 1.0, unit: "".into() },
                advanced: false,
            },
            ToolOptionControl {
                id: "count_y".into(), label: "Count Y".into(),
                kind: ToolOptionKind::Number { value: self.count_y as f32, min: 1.0, max: 50.0, step: 1.0, unit: "".into() },
                advanced: false,
            },
            ToolOptionControl {
                id: "count_z".into(), label: "Count Z".into(),
                kind: ToolOptionKind::Number { value: self.count_z as f32, min: 1.0, max: 50.0, step: 1.0, unit: "".into() },
                advanced: false,
            },
            ToolOptionControl {
                id: "step_x".into(), label: "Step X".into(),
                kind: ToolOptionKind::Number { value: self.step_x, min: 0.1, max: 100.0, step: 0.1, unit: "studs".into() },
                advanced: true,
            },
            ToolOptionControl {
                id: "step_y".into(), label: "Step Y".into(),
                kind: ToolOptionKind::Number { value: self.step_y, min: 0.1, max: 100.0, step: 0.1, unit: "studs".into() },
                advanced: true,
            },
            ToolOptionControl {
                id: "step_z".into(), label: "Step Z".into(),
                kind: ToolOptionKind::Number { value: self.step_z, min: 0.1, max: 100.0, step: 0.1, unit: "studs".into() },
                advanced: true,
            },
            ToolOptionControl {
                id: "commit".into(), label: "Apply".into(),
                kind: ToolOptionKind::Bool { value: self.ready_to_commit },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, _hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        ToolStepResult::Continue
    }

    fn on_option_changed(&mut self, id: &str, value: &str, _ctx: &mut ToolContext) -> ToolStepResult {
        match id {
            "count_x" => { if let Ok(v) = value.parse::<f32>() { self.count_x = (v as u32).clamp(1, 50); } }
            "count_y" => { if let Ok(v) = value.parse::<f32>() { self.count_y = (v as u32).clamp(1, 50); } }
            "count_z" => { if let Ok(v) = value.parse::<f32>() { self.count_z = (v as u32).clamp(1, 50); } }
            "step_x"  => { if let Ok(v) = value.parse::<f32>() { self.step_x = v.max(0.1); } }
            "step_y"  => { if let Ok(v) = value.parse::<f32>() { self.step_y = v.max(0.1); } }
            "step_z"  => { if let Ok(v) = value.parse::<f32>() { self.step_z = v.max(0.1); } }
            "commit" => {
                if value == "true" {
                    self.ready_to_commit = true;
                    return ToolStepResult::Commit;
                }
            }
            _ => {}
        }
        ToolStepResult::Continue
    }

    fn commit(&mut self, world: &mut World) {
        let sources = snapshot_selection(world);
        if sources.is_empty() {
            info!("↺ Grid Array: no selection");
            return;
        }
        let total = self.count_x as u64 * self.count_y as u64 * self.count_z as u64;
        if total > 1000 {
            warn!("↺ Grid Array: refusing to spawn {} copies — cap at 1000 for safety", total);
            return;
        }
        let space_root = match world.get_resource::<crate::space::SpaceRoot>() {
            Some(r) => r.0.clone(),
            None => { warn!("Grid Array: no SpaceRoot"); return; }
        };

        let mut spawned = 0usize;
        for source in &sources {
            for i in 0..self.count_x {
                for j in 0..self.count_y {
                    for k in 0..self.count_z {
                        if i == 0 && j == 0 && k == 0 { continue; } // skip original slot
                        let offset = Vec3::new(
                            self.step_x * i as f32,
                            self.step_y * j as f32,
                            self.step_z * k as f32,
                        );
                        let new_pos = source.transform.translation + offset;
                        let desc = descriptor_for_copy(
                            source, new_pos, source.transform.rotation,
                            &format!("G{:02}x{:02}x{:02}", i, j, k), &space_root,
                        );
                        match spawn_new_part_with_toml(world, desc) {
                            Ok(_) => spawned += 1,
                            Err(e) => warn!("Grid Array: spawn failed — {}", e),
                        }
                    }
                }
            }
        }
        info!("↺ Grid Array: spawned {} copies ({}×{}×{}, {} sources)",
              spawned, self.count_x, self.count_y, self.count_z, sources.len());
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.ready_to_commit = false;
    }
}

// ============================================================================
// Path Array (Phase 2)
// ============================================================================
//
// Clone the selection along a polyline built from viewport clicks.
// User clicks N points (≥ 2) to define the path, chooses the number
// of copies, then Apply spawns evenly-spaced clones along the path
// length (arc-length parameterized). Tangent orientation optional.

pub struct PathArray {
    /// Picked control points in world space.
    points: Vec<Vec3>,
    /// Total copies spawned along the path, including the original.
    pub count: u32,
    /// When true, each clone's rotation aligns with the path tangent
    /// at that parameter — otherwise inherits source rotation.
    pub align_to_tangent: bool,
    ready_to_commit: bool,
}

impl Default for PathArray {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            count: 10,
            align_to_tangent: false,
            ready_to_commit: false,
        }
    }
}

impl ModalTool for PathArray {
    fn id(&self) -> &'static str { "path_array" }
    fn name(&self) -> &'static str { "Path Array" }

    fn step_label(&self) -> String {
        if self.points.len() < 2 {
            format!("click path points ({} so far · need ≥ 2)", self.points.len())
        } else {
            format!("{} points · {} copies — click more or Apply", self.points.len(), self.count)
        }
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "count".into(), label: "Count".into(),
                kind: ToolOptionKind::Number { value: self.count as f32, min: 2.0, max: 200.0, step: 1.0, unit: "".into() },
                advanced: false,
            },
            ToolOptionControl {
                id: "align_tangent".into(), label: "Align to Tangent".into(),
                kind: ToolOptionKind::Bool { value: self.align_to_tangent },
                advanced: false,
            },
            ToolOptionControl {
                id: "commit".into(), label: "Apply".into(),
                kind: ToolOptionKind::Bool { value: self.ready_to_commit },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        self.points.push(hit.hit_point);
        ToolStepResult::Continue
    }

    fn on_option_changed(&mut self, id: &str, value: &str, _ctx: &mut ToolContext) -> ToolStepResult {
        match id {
            "count" => { if let Ok(v) = value.parse::<f32>() { self.count = (v as u32).clamp(2, 200); } }
            "align_tangent" => { self.align_to_tangent = value == "true"; }
            "commit" => {
                if value == "true" && self.points.len() >= 2 {
                    self.ready_to_commit = true;
                    return ToolStepResult::Commit;
                }
            }
            _ => {}
        }
        ToolStepResult::Continue
    }

    fn commit(&mut self, world: &mut World) {
        let sources = snapshot_selection(world);
        if sources.is_empty() {
            info!("↺ Path Array: no selection");
            return;
        }
        if self.points.len() < 2 {
            info!("↺ Path Array: need ≥ 2 path points");
            return;
        }
        let space_root = match world.get_resource::<crate::space::SpaceRoot>() {
            Some(r) => r.0.clone(),
            None => { warn!("Path Array: no SpaceRoot"); return; }
        };

        // Arc-length parameterize the polyline so spacing is uniform.
        let mut cumulative = vec![0.0_f32];
        for i in 1..self.points.len() {
            let seg = (self.points[i] - self.points[i-1]).length();
            cumulative.push(cumulative[i-1] + seg);
        }
        let total_len = *cumulative.last().unwrap();
        if total_len <= 1e-4 { info!("↺ Path Array: zero-length path"); return; }

        let mut spawned = 0usize;
        let align_tangent = self.align_to_tangent;
        for source in &sources {
            for i in 1..self.count {
                // t in [0, 1] evenly — includes endpoints.
                let target_s = total_len * (i as f32 / (self.count - 1) as f32);
                // Find segment containing target_s.
                let mut seg_ix = 0;
                for j in 1..cumulative.len() {
                    if cumulative[j] >= target_s { seg_ix = j - 1; break; }
                }
                let seg_start = cumulative[seg_ix];
                let seg_end   = cumulative[seg_ix + 1];
                let local_t = ((target_s - seg_start) / (seg_end - seg_start).max(1e-6)).clamp(0.0, 1.0);
                let p0 = self.points[seg_ix];
                let p1 = self.points[seg_ix + 1];
                let sample_pos = p0.lerp(p1, local_t);
                // Offset = sample_pos - source.transform.translation so the clone sits on the path.
                let new_pos = sample_pos;
                let new_rot = if align_tangent {
                    let tangent = (p1 - p0).normalize_or_zero();
                    if tangent.length_squared() > 1e-6 {
                        Quat::from_rotation_arc(Vec3::X, tangent) * source.transform.rotation
                    } else {
                        source.transform.rotation
                    }
                } else {
                    source.transform.rotation
                };

                let desc = descriptor_for_copy(
                    source, new_pos, new_rot,
                    &format!("P{:03}", i), &space_root,
                );
                match spawn_new_part_with_toml(world, desc) {
                    Ok(_) => spawned += 1,
                    Err(e) => warn!("Path Array: spawn failed — {}", e),
                }
            }
        }
        info!("↺ Path Array: spawned {} copies along {:.2}-unit polyline ({} points)",
              spawned, total_len, self.points.len());
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.points.clear();
        self.ready_to_commit = false;
    }
}

// ============================================================================
// Plugin + registration
// ============================================================================

pub struct ArrayToolsPlugin;

impl Plugin for ArrayToolsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_array_tools);
    }
}

fn register_array_tools(mut registry: ResMut<ModalToolRegistry>) {
    registry.register("linear_array", || Box::new(LinearArray::default()));
    registry.register("radial_array", || Box::new(RadialArray::default()));
    registry.register("grid_array",   || Box::new(GridArray::default()));
    registry.register("path_array",   || Box::new(PathArray::default()));
}

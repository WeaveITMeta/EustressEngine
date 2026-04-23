//! # Smart Build Tools
//!
//! Concrete [`ModalTool`](crate::modal_tool::ModalTool) implementations
//! that plug into the modal-tool framework. Each tool is independently
//! testable — the trait abstracts mouse/keyboard input away from
//! Bevy systems.
//!
//! Rust-first: the tools are pure state machines. UI is reflected via
//! [`crate::modal_tool::ToolOptionsBarState`]; MCP / Rune can drive
//! the same tools via the registry without changes.
//!
//! ## Folder convention for spawned parts
//!
//! Every part produced by a tool is written as a folder under the
//! active Space root:
//!
//! ```text
//! <space>/<parent>/<name>/
//!   ├── _instance.toml       (class_name, transform, size, color, …)
//!   └── <subchildren folders, if any>
//! ```
//!
//! The `_instance.toml` carries mesh reference + all properties. This
//! makes every tool-spawned part git-diffable, hot-reloadable, and
//! linkable as a parent to further tool-spawned children. See
//! [`FILE_SYSTEM_FIRST.md`](../../../docs/development/FILE_SYSTEM_FIRST.md)
//! for the full file convention.

use bevy::prelude::*;
use crate::modal_tool::{
    ModalTool, ModalToolRegistry,
    ToolContext, ToolOptionControl, ToolOptionKind, ToolStepResult, ViewportHit,
};
use crate::selection_box::Selected;
use crate::space::instance_loader::{
    AssetReference, InstanceDefinition, InstanceMetadata, InstanceProperties,
    TransformData, unique_entity_name, write_instance_definition_signed,
};

// ============================================================================
// Shared spawn helper — used by every tool that creates new parts
// ============================================================================

/// Result of spawning a new TOML-backed part.
pub struct SpawnedPart {
    pub entity: Entity,
    pub toml_path: std::path::PathBuf,
    pub folder_name: String,
}

/// Descriptor for a new part to create. Tools fill this in; the shared
/// helper handles the filesystem I/O, TOML write, and ECS spawn.
pub struct NewPartDescriptor {
    /// Desired folder name under `parent_dir`. A numeric suffix is
    /// appended automatically if the name is taken.
    pub base_name: String,
    /// Relative path under the Space root where the new part's folder
    /// should be created. Typically `"Workspace"` for top-level parts.
    pub parent_rel: std::path::PathBuf,
    /// World transform for the new part.
    pub transform: Transform,
    /// BasePart size (world units). Required for primitive parts; GLB
    /// parts typically leave this at the GLB's intrinsic bounds.
    pub size: Vec3,
    /// Mesh reference — `"parts/block.glb"` for the default cuboid, or
    /// any relative GLB path under the Space root.
    pub mesh: String,
    /// Class name ("Part", "MeshPart", "Wedge", etc.).
    pub class_name: String,
    /// Optional color; passed through to `InstanceProperties.color`.
    /// Defaults to medium gray when None.
    pub color_rgba: Option<[f32; 4]>,
    /// Material name ("Plastic", "Metal", …). Defaults to "Plastic".
    pub material: Option<String>,
    /// True → write `anchored = true` (part doesn't fall under gravity).
    pub anchored: bool,
}

impl NewPartDescriptor {
    /// Build an InstanceDefinition from the descriptor. Pure — no I/O.
    /// Used both for the write path and for tests.
    pub fn to_definition(&self) -> InstanceDefinition {
        let color = self.color_rgba.unwrap_or([163.0/255.0, 162.0/255.0, 165.0/255.0, 1.0]);
        InstanceDefinition {
            asset: Some(AssetReference {
                mesh: self.mesh.clone(),
                scene: "Scene0".to_string(),
            }),
            transform: TransformData {
                position: self.transform.translation.to_array(),
                rotation: [
                    self.transform.rotation.x,
                    self.transform.rotation.y,
                    self.transform.rotation.z,
                    self.transform.rotation.w,
                ],
                scale: self.size.to_array(),
            },
            properties: InstanceProperties {
                color,
                transparency: 0.0,
                anchored: self.anchored,
                can_collide: true,
                cast_shadow: true,
                reflectance: 0.0,
                material: self.material.clone().unwrap_or_else(|| "Plastic".to_string()),
                locked: false,
            },
            metadata: InstanceMetadata {
                class_name: self.class_name.clone(),
                archivable: true,
                name: None,
                created: chrono::Utc::now().to_rfc3339(),
                last_modified: chrono::Utc::now().to_rfc3339(),
                created_by: None,
                modifications: Vec::new(),
            },
            material: None,
            thermodynamic: None,
            electrochemical: None,
            ui: None,
            attributes: None,
            tags: None,
            parameters: None,
            extra: std::collections::HashMap::new(),
        }
    }
}

/// Write a new part to disk under the Space's folder convention AND
/// spawn it in the ECS via `spawn_instance`. Call from within a
/// `ModalTool::commit` (exclusive `&mut World`).
///
/// On success, returns the new Entity + paths. On failure (missing
/// SpaceRoot, I/O error, missing required resources), returns Err with
/// a user-friendly message that can be surfaced via toast/log.
pub fn spawn_new_part_with_toml(
    world: &mut World,
    descriptor: NewPartDescriptor,
) -> Result<SpawnedPart, String> {
    // Resolve the Space root. Without it we can't decide where the TOML goes.
    let space_root = world.get_resource::<crate::space::SpaceRoot>()
        .ok_or_else(|| "No SpaceRoot resource — cannot locate Space folder".to_string())?
        .0.clone();

    // Resolve parent folder (create if missing — user may invoke a tool
    // with a fresh universe).
    let parent_dir = space_root.join(&descriptor.parent_rel);
    std::fs::create_dir_all(&parent_dir)
        .map_err(|e| format!("Failed to create parent folder {:?}: {}", parent_dir, e))?;

    // Derive a unique folder name — avoids collision with existing parts
    // in the same parent folder.
    let folder_name = unique_entity_name(&parent_dir, &descriptor.base_name);
    let instance_dir = parent_dir.join(&folder_name);
    std::fs::create_dir_all(&instance_dir)
        .map_err(|e| format!("Failed to create instance dir {:?}: {}", instance_dir, e))?;
    let toml_path = instance_dir.join("_instance.toml");

    // Build the instance definition from the descriptor.
    let mut instance = descriptor.to_definition();

    // Write the TOML. Pass an auth stamp if the user is logged in so
    // the modification audit chain is signed.
    let stamp = world.get_resource::<crate::auth::AuthState>()
        .and_then(crate::space::instance_loader::current_stamp);
    write_instance_definition_signed(&toml_path, &mut instance, stamp.as_ref())?;

    // Spawn the entity via the standard instance loader so the ECS
    // state mirrors the TOML exactly (components, adornments, lighting
    // treatment, etc.).
    let entity = spawn_instance_from_world(world, &toml_path, instance)?;

    // Register the new folder in the SpaceFileRegistry so hot-reload
    // and file-watcher systems know about it. Registration path is the
    // folder (not the TOML inside) because downstream watchers key off
    // the directory for child-file tracking.
    let modified = std::fs::metadata(&instance_dir)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| std::time::SystemTime::now());
    if let Some(mut registry) = world.get_resource_mut::<crate::space::SpaceFileRegistry>() {
        let metadata = crate::space::file_loader::FileMetadata {
            path: instance_dir.clone(),
            file_type: crate::space::file_loader::FileType::Directory,
            service: descriptor.parent_rel
                .iter().next()
                .and_then(|s| s.to_str())
                .unwrap_or("Workspace")
                .to_string(),
            name: folder_name.clone(),
            size: 0,
            modified,
            children: Vec::new(),
        };
        registry.register(instance_dir.clone(), entity, metadata);
    }

    // Record an Undo entry so Ctrl+Z removes the spawned part. Reserve
    // the trash path now so undo + redo are symmetric file-renames.
    // Trash path format matches TrashEntities: `.eustress/trash/<timestamp>/<folder>`.
    let trash_path = space_root
        .join(".eustress")
        .join("trash")
        .join(chrono::Utc::now().format("%Y%m%d_%H%M%S_%f").to_string())
        .join(&folder_name);
    if let Some(mut undo) = world.get_resource_mut::<crate::undo::UndoStack>() {
        undo.push(crate::undo::Action::SpawnFolders {
            folders: vec![(instance_dir.clone(), trash_path)],
        });
    }

    Ok(SpawnedPart { entity, toml_path, folder_name })
}

/// World-based wrapper around `spawn_instance` — gathers the required
/// resources from `World` rather than asking the caller to plumb them
/// through. Uses `World::resource_scope` nesting to lift each resource
/// out in turn so we can hold all three `&mut` simultaneously without
/// violating Bevy's single-mutable-borrow rule.
fn spawn_instance_from_world(
    world: &mut World,
    toml_path: &std::path::Path,
    instance: InstanceDefinition,
) -> Result<Entity, String> {
    use crate::space::instance_loader::{PrimitiveMeshCache, spawn_instance};
    use crate::space::material_loader::MaterialRegistry;

    // Ensure caches exist — first Smart Build Tool invocation in a
    // fresh universe needs these to succeed.
    if world.get_resource::<PrimitiveMeshCache>().is_none() {
        world.insert_resource(PrimitiveMeshCache::default());
    }
    if world.get_resource::<MaterialRegistry>().is_none() {
        world.insert_resource(MaterialRegistry::default());
    }
    if world.get_resource::<AssetServer>().is_none() {
        return Err("No AssetServer — engine not fully initialized".to_string());
    }

    // Clone AssetServer handle first — it's `Clone` and we need it by
    // value inside the nested scopes.
    let asset_server = world.resource::<AssetServer>().clone();

    // Nested resource_scope: each call lifts one resource out of the
    // world as `&mut`, hands back a World sans that resource. Inside
    // the innermost scope we have exclusive refs to all three plus
    // a World for `Commands::new`.
    let mut result: Option<Entity> = None;
    world.resource_scope(|world, mut materials: Mut<Assets<StandardMaterial>>| {
        world.resource_scope(|world, mut mat_reg: Mut<MaterialRegistry>| {
            world.resource_scope(|world, mut mesh_cache: Mut<PrimitiveMeshCache>| {
                let mut queue = bevy::ecs::world::CommandQueue::default();
                let entity = {
                    let mut commands = Commands::new(&mut queue, world);
                    spawn_instance(
                        &mut commands,
                        &asset_server,
                        &mut *materials,
                        &mut *mat_reg,
                        &mut *mesh_cache,
                        toml_path.to_path_buf(),
                        instance,
                    )
                };
                queue.apply(world);
                result = Some(entity);
            });
        });
    });
    result.ok_or_else(|| "spawn_instance returned no entity".to_string())
}

// ============================================================================
// Plugin — registers factories at startup
// ============================================================================

pub struct SmartToolsPlugin;

impl Plugin for SmartToolsPlugin {
    fn build(&self, app: &mut App) {
        // Run registration after ModalToolPlugin has initialized the
        // registry resource.
        app.add_systems(Startup, register_smart_tools)
            .add_systems(Update, (
                announce_modal_tool_commits,
                // Material Flip loader roundtrip — applies persisted
                // `material_uv_ops` to cloned materials once the
                // underlying StandardMaterial asset has finished loading.
                apply_pending_material_uv_ops,
            ));
    }
}

/// Marker component set by `instance_loader::spawn_instance` when an
/// entity's TOML carries `attributes.material_uv_ops`. The ops list is
/// a history of Material Flip operations (`"rot_cw"`, `"rot_ccw"`,
/// `"mirror_u"`, `"mirror_v"`) in application order. The
/// `apply_pending_material_uv_ops` system composes them into an
/// `Affine2` and writes it to a cloned material's `uv_transform` once
/// the base material has finished loading from disk. The component is
/// removed on success so each entry is applied exactly once per spawn.
#[derive(Component, Debug, Clone)]
pub struct PendingMaterialUvOps {
    pub ops: Vec<String>,
}

/// Consume `PendingMaterialUvOps` once the underlying StandardMaterial
/// asset is loaded. Pattern matches how `MaterialFlip` applies ops
/// in-session (see `apply_material_flip_to_selected`) but happens at
/// spawn time rather than user-action time.
fn apply_pending_material_uv_ops(
    mut commands: Commands,
    mut materials: ResMut<Assets<bevy::pbr::StandardMaterial>>,
    query: Query<(Entity, &MeshMaterial3d<bevy::pbr::StandardMaterial>, &PendingMaterialUvOps)>,
) {
    use bevy::math::{Affine2, Vec2};

    for (entity, mat_handle, pending) in query.iter() {
        // Material asset must be present in the Assets store; otherwise
        // it's still loading — skip this frame and try next.
        let Some(base_mat) = materials.get(&mat_handle.0) else { continue; };

        // Compose all ops into a single Affine2. Order matters —
        // oldest first so the final transform is the same as if the
        // user had clicked each op in sequence.
        let mut composed = Affine2::IDENTITY;
        for op in &pending.ops {
            let step: Affine2 = match op.as_str() {
                "rot_cw"   => Affine2::from_angle(-std::f32::consts::FRAC_PI_2),
                "rot_ccw"  => Affine2::from_angle(std::f32::consts::FRAC_PI_2),
                "mirror_u" => Affine2::from_scale(Vec2::new(-1.0, 1.0)),
                "mirror_v" => Affine2::from_scale(Vec2::new(1.0, -1.0)),
                _ => continue,
            };
            composed = step * composed;
        }

        // Clone the base material + apply the composed uv_transform so
        // other entities sharing the base handle aren't affected.
        let mut cloned = base_mat.clone();
        cloned.uv_transform = composed * cloned.uv_transform;
        let new_handle = materials.add(cloned);

        commands.entity(entity)
            .insert(MeshMaterial3d(new_handle))
            .remove::<PendingMaterialUvOps>();

        debug!("🎨 Material Flip roundtrip: applied {} op(s) to entity {:?}",
               pending.ops.len(), entity);
    }
}

/// Subscribe to `ModalToolCommittedEvent` and push a notification so
/// users see a confirmation + undo hint after every Smart Build Tool
/// committal. Tucked here (vs. in modal_tool.rs) because it's the
/// Smart Build Tools side of the story — the modal_tool framework
/// emits the event generically; this handler shapes the message for
/// Smart Build tool names.
fn announce_modal_tool_commits(
    mut events: MessageReader<crate::modal_tool::ModalToolCommittedEvent>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    for event in events.read() {
        // Friendly message per tool. Keep messages short — they render
        // as toast headers. "Undo" hint is implicit via Ctrl+Z binding,
        // and the action is undoable because spawn_new_part_with_toml
        // pushes an UndoStack entry.
        let msg = match event.tool_id.as_str() {
            "gap_fill"             => "Gap Fill applied — Ctrl+Z to undo",
            "resize_align"         => "Resize Align applied — Ctrl+Z to undo",
            "edge_align"           => "Edge Align applied — Ctrl+Z to undo",
            "part_swap_positions"  => "Parts swapped — Ctrl+Z to undo",
            "model_reflect"        => "Reflection spawned — Ctrl+Z to undo",
            "material_flip"        => "Material flipped — Ctrl+Z to undo",
            "linear_array"         => "Linear Array applied — Ctrl+Z to undo",
            "radial_array"         => "Radial Array applied — Ctrl+Z to undo",
            "grid_array"           => "Grid Array applied — Ctrl+Z to undo",
            _                      => continue,  // silent for unnamed tools
        };
        notifications.success(msg);
    }
}

fn register_smart_tools(mut registry: ResMut<ModalToolRegistry>) {
    // Each entry is `(id, factory closure)`. Adding a tool = adding one
    // line here. The factory produces a fresh, zero-configured
    // instance each activation — sessions never share state.
    registry.register("part_swap_positions", || Box::new(PartSwapPositions::default()));
    registry.register("edge_align",           || Box::new(EdgeAlign::default()));
    registry.register("model_reflect",        || Box::new(ModelReflect::default()));
    registry.register("gap_fill",             || Box::new(GapFill::default()));
    registry.register("resize_align",         || Box::new(ResizeAlign::default()));
    registry.register("material_flip",        || Box::new(MaterialFlip::default()));

    info!("🔧 Registered modal tools: {:?}", registry.tool_ids());
}

/// Write an entity's current Transform back to its source
/// `_instance.toml` so the change persists across reloads. No-op if
/// the entity has no InstanceFile (ad-hoc entity — nothing to persist).
/// Stamped with the current auth identity when available so the
/// modification appears in the signed audit chain.
pub fn persist_transform_to_toml(world: &mut World, entity: Entity) {
    use crate::space::instance_loader::{
        InstanceFile, load_instance_definition, write_instance_definition_signed, current_stamp,
    };

    // Take InstanceFile path + current Transform by value before
    // claiming any &mut borrows.
    let Some(inst_path) = world.get::<InstanceFile>(entity).map(|f| f.toml_path.clone()) else {
        // Entity wasn't loaded from a TOML file — nothing to persist.
        return;
    };
    let Some(transform) = world.get::<Transform>(entity).cloned() else {
        return;
    };

    // Load existing def, mutate, write back. Preserves all other fields
    // (color, material, physics, tags, attributes, etc.).
    let mut def = match load_instance_definition(&inst_path) {
        Ok(d) => d,
        Err(e) => {
            warn!("persist_transform_to_toml: failed to read {:?}: {}", inst_path, e);
            return;
        }
    };
    def.transform.position = transform.translation.to_array();
    def.transform.rotation = [
        transform.rotation.x, transform.rotation.y,
        transform.rotation.z, transform.rotation.w,
    ];

    let stamp = world.get_resource::<crate::auth::AuthState>()
        .and_then(current_stamp);
    if let Err(e) = write_instance_definition_signed(&inst_path, &mut def, stamp.as_ref()) {
        warn!("persist_transform_to_toml: write failed {:?}: {}", inst_path, e);
    }
}

/// Fallback InstanceDefinition when the source entity isn't TOML-backed.
/// Produces a plain block at the given world transform with a reasonable
/// size. Used by tools that clone source entities when the source has no
/// source TOML to inherit from.
fn build_fallback_def(position: Vec3, rotation: Quat, size: Vec3) -> InstanceDefinition {
    InstanceDefinition {
        asset: Some(AssetReference {
            mesh: "parts/block.glb".to_string(),
            scene: "Scene0".to_string(),
        }),
        transform: TransformData {
            position: position.to_array(),
            rotation: [rotation.x, rotation.y, rotation.z, rotation.w],
            scale: size.to_array(),
        },
        properties: InstanceProperties {
            color: [163.0/255.0, 162.0/255.0, 165.0/255.0, 1.0],
            transparency: 0.0,
            anchored: false,
            can_collide: true,
            cast_shadow: true,
            reflectance: 0.0,
            material: "Plastic".to_string(),
            locked: false,
        },
        metadata: InstanceMetadata {
            class_name: "Part".to_string(),
            archivable: true,
            name: None,
            created: chrono::Utc::now().to_rfc3339(),
            last_modified: chrono::Utc::now().to_rfc3339(),
            created_by: None,
            modifications: Vec::new(),
        },
        material: None,
        thermodynamic: None,
        electrochemical: None,
        ui: None,
        attributes: None,
        tags: None,
        parameters: None,
        extra: std::collections::HashMap::new(),
    }
}

// ============================================================================
// Part Swap: Swap Positions
// ============================================================================
//
// The two-click variant of Part Swap from TOOLSET.md §4.13.4. User
// clicks first target, then second target — the two entities trade
// world positions (and world rotations, optionally; v1 swaps position
// only per the spec, size untouched).
//
// This is the simplest full-lifecycle tool in the codebase and so
// serves as the reference implementation of the ModalTool contract.

#[derive(Default)]
pub struct PartSwapPositions {
    /// The first-clicked entity. When None, we're awaiting first pick.
    first: Option<Entity>,
    /// The second-clicked entity. Set in `on_click` before returning
    /// `Commit`, read in `commit()` after the deferred-world step runs.
    second: Option<Entity>,
    /// Remember both rotations too, in case the user enables the "also
    /// swap rotations" option (advanced / ⋯).
    swap_rotations: bool,
}

impl ModalTool for PartSwapPositions {
    fn id(&self) -> &'static str { "part_swap_positions" }
    fn name(&self) -> &'static str { "Part Swap (positions)" }
    fn icon_path(&self) -> &'static str { "assets/icons/ui/cursor-badge-part-swap.svg" }

    fn step_label(&self) -> String {
        match self.first {
            None    => "pick first part".to_string(),
            Some(_) => "pick second part to swap with".to_string(),
        }
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "swap_rotations".to_string(),
                label: "Swap rotations too".to_string(),
                kind: ToolOptionKind::Bool { value: self.swap_rotations },
                advanced: true,
            },
            ToolOptionControl {
                id: "hint".to_string(),
                label: "".to_string(),
                kind: ToolOptionKind::Label {
                    text: "Pick two parts to trade world positions. Esc cancels.".to_string(),
                },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        let Some(clicked) = hit.hit_entity else { return ToolStepResult::Continue };

        match self.first {
            None => {
                // First pick — remember and wait.
                self.first = Some(clicked);
                ToolStepResult::Continue
            }
            Some(first) if first == clicked => {
                // User clicked the same part twice — ignore (no-op swap).
                ToolStepResult::Continue
            }
            Some(_) => {
                // Second pick — cache and ready to commit.
                self.second = Some(clicked);
                ToolStepResult::Commit
            }
        }
    }

    fn on_option_changed(&mut self, id: &str, value: &str, _ctx: &mut ToolContext) -> ToolStepResult {
        if id == "swap_rotations" {
            self.swap_rotations = value == "true";
        }
        ToolStepResult::Continue
    }

    fn commit(&mut self, world: &mut World) {
        let Some(first) = self.first else { return };
        // `second` was cached in `on_click` at the moment we returned
        // `Commit`, before the deferred-world step runs.
        let Some(second) = self.second else { return };

        // Exchange world positions (and optionally rotations) via
        // direct Transform writes. Undo entry records the prior state.
        let first_transform = world.get::<Transform>(first).cloned();
        let second_transform = world.get::<Transform>(second).cloned();

        let (Some(a), Some(b)) = (first_transform, second_transform) else { return };

        if let Some(mut t) = world.get_mut::<Transform>(first) {
            t.translation = b.translation;
            if self.swap_rotations {
                t.rotation = b.rotation;
            }
        }
        if let Some(mut t) = world.get_mut::<Transform>(second) {
            t.translation = a.translation;
            if self.swap_rotations {
                t.rotation = a.rotation;
            }
        }

        // Undo entry — two entities moved, one logical "Part Swap" op.
        if let Some(mut undo) = world.get_resource_mut::<crate::undo::UndoStack>() {
            undo.push(crate::undo::Action::TransformEntities {
                old_transforms: vec![
                    (first.to_bits(), a.translation.to_array(), a.rotation.to_array()),
                    (second.to_bits(), b.translation.to_array(), b.rotation.to_array()),
                ],
                new_transforms: vec![
                    (first.to_bits(), b.translation.to_array(), if self.swap_rotations { b.rotation.to_array() } else { a.rotation.to_array() }),
                    (second.to_bits(), a.translation.to_array(), if self.swap_rotations { a.rotation.to_array() } else { b.rotation.to_array() }),
                ],
            });
        }

        // Persist the swap to the source TOML files. Without this, a
        // reload loses the swap — the TOML on disk is source-of-truth
        // in Eustress's file-system-first architecture.
        persist_transform_to_toml(world, first);
        persist_transform_to_toml(world, second);

        info!("🔄 Part Swap: {:?} ↔ {:?}", first, second);
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        // No preview entities to clean up — Part Swap doesn't spawn any.
        self.first = None;
        self.second = None;
    }
}


// ============================================================================
// Edge Align (translation variant — aligns source edge to target edge)
// ============================================================================
//
// Simpler than Resize Align for v1: user picks a source entity, then a
// target entity — source translates so its edge lies coincident with
// the target's edge. This is the translate-only Edge Align from
// TOOLSET.md §4.13.3. Rotation-aware edge alignment (which rotates
// source to match target's edge direction) is a P1 refinement.

#[derive(Default)]
pub struct EdgeAlign {
    source: Option<Entity>,
    target: Option<Entity>,
    preview: Option<Entity>,
}

impl ModalTool for EdgeAlign {
    fn id(&self) -> &'static str { "edge_align" }
    fn name(&self) -> &'static str { "Edge Align" }
    fn icon_path(&self) -> &'static str { "assets/icons/ui/cursor-badge-edge-align.svg" }

    fn step_label(&self) -> String {
        match self.source {
            None    => "pick source part".to_string(),
            Some(_) => "pick target part to align to".to_string(),
        }
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "hint".to_string(),
                label: "".to_string(),
                kind: ToolOptionKind::Label {
                    text: "Pick source, then target. Source translates to align.".to_string(),
                },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        let Some(clicked) = hit.hit_entity else { return ToolStepResult::Continue };

        match self.source {
            None => {
                self.source = Some(clicked);
                ToolStepResult::Continue
            }
            Some(src) if src == clicked => ToolStepResult::Continue,
            Some(_) => {
                self.target = Some(clicked);
                ToolStepResult::Commit
            }
        }
    }

    fn commit(&mut self, world: &mut World) {
        let Some(source) = self.source else { return };
        let Some(target) = self.target else { return };

        // Align source's origin to target's origin — simplest P0 Edge
        // Align semantics. Rotation-aware edge-to-edge math is P1.
        let target_pos = world.get::<Transform>(target).map(|t| t.translation);
        let source_prev = world.get::<Transform>(source).cloned();

        if let (Some(new_pos), Some(mut src_tr)) = (target_pos, world.get_mut::<Transform>(source)) {
            src_tr.translation = new_pos;
        }

        if let (Some(prev), Some(target_pos)) = (source_prev, target_pos) {
            if let Some(mut undo) = world.get_resource_mut::<crate::undo::UndoStack>() {
                undo.push(crate::undo::Action::TransformEntities {
                    old_transforms: vec![(source.to_bits(), prev.translation.to_array(), prev.rotation.to_array())],
                    new_transforms: vec![(source.to_bits(), target_pos.to_array(), prev.rotation.to_array())],
                });
            }
            // File-system-first: persist the translation to the source's
            // _instance.toml so reload preserves the alignment.
            persist_transform_to_toml(world, source);
            info!("🔗 Edge Align: moved {:?} to {:?}", source, target);
        }
    }

    fn cancel(&mut self, commands: &mut Commands) {
        if let Some(e) = self.preview.take() {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.despawn();
            }
        }
        self.source = None;
        self.target = None;
    }

    fn preview_entities(&self) -> Vec<Entity> {
        self.preview.into_iter().collect()
    }
}


// ============================================================================
// Model Reflect (destructive variant)
// ============================================================================
//
// Reflects every currently-Selected entity across a chosen world plane.
// Destructive variant from TOOLSET.md §4.13.6 P0 — creates new parts
// at mirrored positions with mirrored rotations. The non-destructive
// linked-mirror feature is P1.

pub struct ModelReflect {
    /// "xy" | "xz" | "yz"
    plane: String,
    /// Preserve welds when reflecting (wire weld endpoints to mirrored counterparts).
    weld_fixup: bool,
    /// Phase-1 non-destructive variant — when true, each mirrored clone
    /// gets a `MirrorLink` component so the runtime keeps it in sync
    /// with source Transform changes. See `mirror_link.rs`.
    linked: bool,
    /// Committed exactly once on the "Reflect" option being toggled true.
    /// Using an option toggle instead of a viewport click because the
    /// destination is implicit (everything currently selected, reflected
    /// through the plane).
    ready_to_commit: bool,
}

impl Default for ModelReflect {
    fn default() -> Self {
        // Sensible defaults per TOOLSET_UX.md §4.9 —
        //   plane = Selection XZ (closest world analogue: XZ plane)
        //   weld_fixup = true
        //   linked = false (destructive is the simpler default)
        Self {
            plane: "xz".to_string(),
            weld_fixup: true,
            linked: false,
            ready_to_commit: false,
        }
    }
}

impl ModalTool for ModelReflect {
    fn id(&self) -> &'static str { "model_reflect" }
    fn name(&self) -> &'static str { "Model Reflect" }
    fn icon_path(&self) -> &'static str { "assets/icons/ui/cursor-badge-mirror.svg" }

    fn step_label(&self) -> String {
        "pick plane + click Reflect to apply".to_string()
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "plane".to_string(),
                label: "Plane".to_string(),
                kind: ToolOptionKind::Choice {
                    options: vec!["xy".into(), "xz".into(), "yz".into()],
                    selected: self.plane.clone(),
                },
                advanced: false,
            },
            ToolOptionControl {
                id: "weld_fixup".to_string(),
                label: "Weld Fix-up".to_string(),
                kind: ToolOptionKind::Bool { value: self.weld_fixup },
                advanced: true,
            },
            ToolOptionControl {
                id: "linked".to_string(),
                label: "Linked".to_string(),
                kind: ToolOptionKind::Bool { value: self.linked },
                advanced: false,
            },
            ToolOptionControl {
                id: "commit".to_string(),
                label: "Reflect".to_string(),
                kind: ToolOptionKind::Bool { value: self.ready_to_commit },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, _hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        // No viewport-click interaction for Model Reflect — commit is
        // driven by the "Reflect" option toggle.
        ToolStepResult::Continue
    }

    fn on_option_changed(&mut self, id: &str, value: &str, _ctx: &mut ToolContext) -> ToolStepResult {
        match id {
            "plane" => { self.plane = value.to_string(); ToolStepResult::Continue }
            "weld_fixup" => { self.weld_fixup = value == "true"; ToolStepResult::Continue }
            "linked" => { self.linked = value == "true"; ToolStepResult::Continue }
            "commit" => {
                if value == "true" {
                    self.ready_to_commit = true;
                    ToolStepResult::Commit
                } else {
                    ToolStepResult::Continue
                }
            }
            _ => ToolStepResult::Continue,
        }
    }

    fn commit(&mut self, world: &mut World) {
        // Plane normal from user choice — mirror flips through this axis.
        let normal = match self.plane.as_str() {
            "xy" => Vec3::Z,
            "xz" => Vec3::Y,
            "yz" => Vec3::X,
            _ => Vec3::Y,
        };

        // Snapshot of selected entities + their current state. Capture
        // the optional InstanceFile so we can read the source's full
        // TOML definition (class, mesh, material, color, etc.) and
        // produce a faithful mirror rather than a bare Transform clone.
        use crate::space::instance_loader::InstanceFile;
        let mut snapshot: Vec<(Entity, Transform, Vec3, Option<std::path::PathBuf>, String)> = Vec::new();
        {
            let mut q = world.query_filtered::<
                (Entity, &Transform, Option<&crate::classes::BasePart>, Option<&InstanceFile>, Option<&crate::classes::Instance>),
                With<Selected>,
            >();
            for (e, t, bp, inst_file, inst) in q.iter(world) {
                let size = bp.map(|b| b.size).unwrap_or(t.scale);
                let source_toml = inst_file.map(|f| f.toml_path.clone());
                let base_name = inst.map(|i| i.name.clone()).unwrap_or_else(|| "Reflected".to_string());
                snapshot.push((e, *t, size, source_toml, base_name));
            }
        }

        let mut spawned = 0usize;
        let linked = self.linked;
        for (source_entity, t, size, source_toml, base_name) in snapshot {
            // Reflect position across the plane through origin.
            //   p' = p − 2 (p·n) n
            let reflected_pos = t.translation - 2.0 * t.translation.dot(normal) * normal;

            // Reflect rotation: a reflection is an improper isometry
            // (det −1); Quat only represents proper rotations, so we
            // flip the axis component along the plane normal and negate
            // the angle. Produces the visually correct mirror for
            // almost all game-asset use-cases.
            let (axis, angle) = t.rotation.to_axis_angle();
            let reflected_axis = axis - 2.0 * axis.dot(normal) * normal;
            let reflected_rot = if reflected_axis.length_squared() > 1e-6 {
                Quat::from_axis_angle(reflected_axis.normalize(), -angle)
            } else {
                t.rotation
            };

            // Build the reflected InstanceDefinition. When the source
            // is TOML-backed, clone its definition and override only
            // transform fields — inherits mesh, material, color, etc.
            // When the source is ad-hoc (no InstanceFile), fall back
            // to a bare Part definition.
            let (instance, class_name) = if let Some(ref path) = source_toml {
                match crate::space::instance_loader::load_instance_definition(path) {
                    Ok(mut def) => {
                        def.transform.position = reflected_pos.to_array();
                        def.transform.rotation = [
                            reflected_rot.x, reflected_rot.y,
                            reflected_rot.z, reflected_rot.w,
                        ];
                        // Mark as a new entity — clear audit chain from source.
                        def.metadata.created_by = None;
                        def.metadata.modifications.clear();
                        def.metadata.created = chrono::Utc::now().to_rfc3339();
                        def.metadata.last_modified = def.metadata.created.clone();
                        let cn = def.metadata.class_name.clone();
                        (def, cn)
                    }
                    Err(e) => {
                        warn!("Model Reflect: failed to read source TOML {:?}: {} — using fallback", path, e);
                        (build_fallback_def(reflected_pos, reflected_rot, size), "Part".to_string())
                    }
                }
            } else {
                (build_fallback_def(reflected_pos, reflected_rot, size), "Part".to_string())
            };

            // Determine parent folder. Reflected parts go in the same
            // service as the source — usually Workspace. If the source
            // has a TOML path, extract its parent folder relative to
            // the Space root.
            let parent_rel = if let (Some(ref src), Some(space_root)) = (
                source_toml.as_ref(),
                world.get_resource::<crate::space::SpaceRoot>(),
            ) {
                // source is `.../<space>/<parent>/<src>/_instance.toml`.
                // We want `<parent>` — i.e. two parents up from the TOML.
                src.parent().and_then(|p| p.parent())
                    .and_then(|p| p.strip_prefix(&space_root.0).ok())
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| std::path::PathBuf::from("Workspace"))
            } else {
                std::path::PathBuf::from("Workspace")
            };

            // Write + spawn. On error, log and continue — partial success
            // beats nothing for bulk operations.
            let desc = NewPartDescriptor {
                base_name: format!("{}_Reflected", base_name),
                parent_rel,
                transform: Transform {
                    translation: reflected_pos,
                    rotation: reflected_rot,
                    scale: Vec3::ONE,     // definition's scale holds actual size
                },
                size,
                mesh: instance.asset.as_ref().map(|a| a.mesh.clone())
                    .unwrap_or_else(|| "parts/block.glb".to_string()),
                class_name,
                color_rgba: Some(instance.properties.color),
                material: Some(instance.properties.material.clone()),
                anchored: instance.properties.anchored,
            };
            match spawn_new_part_with_toml(world, desc) {
                Ok(part) => {
                    spawned += 1;
                    // Linked-mirror variant — tag the clone with a
                    // MirrorLink pointing back at the source + plane.
                    // The mirror_link runtime keeps their Transforms
                    // in sync on subsequent source edits.
                    if linked {
                        if let Ok(mut ent) = world.get_entity_mut(part.entity) {
                            ent.insert(crate::mirror_link::MirrorLink {
                                source: source_entity,
                                plane_normal: normal,
                                plane_point: Vec3::ZERO,
                            });
                        }
                    }
                    info!("🔀 Model Reflect: spawned {:?} at {:?} (across {} plane{})",
                          part.folder_name, reflected_pos, self.plane,
                          if linked { ", linked" } else { "" });
                }
                Err(e) => {
                    warn!("Model Reflect: spawn failed — {}", e);
                }
            }
        }

        info!("🔀 Model Reflect: reflected {} entities across {} plane{}",
              spawned, self.plane, if linked { " (linked)" } else { "" });
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.ready_to_commit = false;
    }
}

// ============================================================================
// Gap Fill
// ============================================================================
//
// The two-click "fill the space between" tool from TOOLSET.md §4.13.1.
// User picks two entities; the tool spawns a new Block-class part at the
// midpoint, scaled to span the gap along the dominant axis between the
// two entities' world positions. Thickness is user-adjustable via the
// Options Bar.
//
// Edge-precise mesh generation (the stravant-style bridging triangulation)
// is Phase 1 — the v1 implementation uses a proper block that reads as
// "a filled gap" for every common geometry: roof-to-pillar gaps,
// floor-to-wall joins, etc.

pub struct GapFill {
    first: Option<Entity>,
    second: Option<Entity>,
    /// Gap-filling block's thickness along the axis perpendicular to
    /// the bridging direction. Exposed via Options Bar as "Thickness".
    thickness: f32,
    /// Preserve source part's color/material in the fill.
    preserve_material: bool,
    /// "Auto" | "2" | "4" — number of wedges to spawn. Auto chooses
    /// based on the rotation-angle heuristic (see `commit`).
    fill_mode: String,
}

impl Default for GapFill {
    fn default() -> Self {
        // Sensible defaults per TOOLSET_UX.md §4.5 (thickness = 0.20 m,
        // preserve material ON). Fill mode defaults to Auto — users
        // rarely need to override.
        Self {
            first: None,
            second: None,
            thickness: 0.2,
            preserve_material: true,
            fill_mode: "Auto".to_string(),
        }
    }
}

impl ModalTool for GapFill {
    fn id(&self) -> &'static str { "gap_fill" }
    fn name(&self) -> &'static str { "Gap Fill" }
    fn icon_path(&self) -> &'static str { "assets/icons/ui/cursor-badge-gap-fill.svg" }

    fn step_label(&self) -> String {
        match (self.first, self.second) {
            (None, _)          => "pick first part".to_string(),
            (Some(_), None)    => "pick second part".to_string(),
            (Some(_), Some(_)) => "adjust thickness + click again to commit".to_string(),
        }
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "thickness".to_string(),
                label: "Thickness".to_string(),
                kind: ToolOptionKind::Number {
                    value: self.thickness,
                    min: 0.01,
                    max: 10.0,
                    step: 0.05,
                    unit: "m".to_string(),
                },
                advanced: false,
            },
            ToolOptionControl {
                id: "fill_mode".to_string(),
                label: "Wedges".to_string(),
                kind: ToolOptionKind::Choice {
                    options: vec!["Auto".into(), "2".into(), "4".into()],
                    selected: self.fill_mode.clone(),
                },
                advanced: false,
            },
            ToolOptionControl {
                id: "preserve_material".to_string(),
                label: "Preserve Material".to_string(),
                kind: ToolOptionKind::Bool { value: self.preserve_material },
                advanced: true,
            },
            ToolOptionControl {
                id: "hint".to_string(),
                label: "".to_string(),
                kind: ToolOptionKind::Label {
                    text: "Pick two parts — stravant-style wedges fill the gap. Auto picks 2 (flat) or 4 (twisted) based on geometry.".to_string(),
                },
                advanced: false,
            },
        ]
    }

    fn on_option_changed(&mut self, id: &str, value: &str, _ctx: &mut ToolContext) -> ToolStepResult {
        match id {
            "thickness" => {
                if let Ok(v) = value.parse::<f32>() {
                    self.thickness = v.clamp(0.01, 10.0);
                }
            }
            "preserve_material" => {
                self.preserve_material = value == "true";
            }
            "fill_mode" => {
                // Accept "Auto" / "2" / "4" — anything else falls back
                // to Auto in `commit`.
                self.fill_mode = value.to_string();
            }
            _ => {}
        }
        ToolStepResult::Continue
    }

    fn on_click(&mut self, hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        let Some(clicked) = hit.hit_entity else { return ToolStepResult::Continue };

        match (self.first, self.second) {
            (None, _) => {
                self.first = Some(clicked);
                ToolStepResult::Continue
            }
            (Some(first), None) if first == clicked => ToolStepResult::Continue,
            (Some(_), None) => {
                self.second = Some(clicked);
                // After the second pick we stay in session so the user
                // can tweak thickness via Options Bar; a THIRD click
                // commits.
                ToolStepResult::Continue
            }
            (Some(_), Some(_)) => {
                // Third click → commit (spawns 2 wedges).
                ToolStepResult::Commit
            }
        }
    }

    fn commit(&mut self, world: &mut World) {
        let (Some(first), Some(second)) = (self.first, self.second) else { return };

        // Gather both entities' world transforms + base sizes.
        let first_t = world.get::<Transform>(first).cloned();
        let second_t = world.get::<Transform>(second).cloned();
        let first_size = world.get::<crate::classes::BasePart>(first).map(|b| b.size);
        let second_size = world.get::<crate::classes::BasePart>(second).map(|b| b.size);

        let (Some(a_t), Some(b_t)) = (first_t, second_t) else {
            warn!("Gap Fill: one of the picked entities has no Transform — aborting");
            return;
        };
        let a_size = first_size.unwrap_or(a_t.scale);
        let b_size = second_size.unwrap_or(b_t.scale);

        let connector = b_t.translation - a_t.translation;
        let distance = connector.length().max(0.001);
        let direction = connector / distance;
        let midpoint = (a_t.translation + b_t.translation) * 0.5;

        let half_proj_a = (a_size * 0.5).dot(direction.abs());
        let half_proj_b = (b_size * 0.5).dot(direction.abs());
        let fill_length = (distance - half_proj_a - half_proj_b).max(self.thickness);

        let (color, material) = {
            let color = world.get::<crate::classes::BasePart>(first)
                .map(|bp| {
                    let s = bp.color.to_srgba();
                    [s.red, s.green, s.blue, s.alpha]
                })
                .unwrap_or([0.65, 0.65, 0.65, 1.0]);
            if self.preserve_material {
                (color, "Plastic".to_string())
            } else {
                ([0.5, 0.7, 1.0, 1.0], "Plastic".to_string())
            }
        };

        let parent_rel = if let (Some(if_comp), Some(space_root)) = (
            world.get::<crate::space::instance_loader::InstanceFile>(first),
            world.get_resource::<crate::space::SpaceRoot>(),
        ) {
            if_comp.toml_path.parent().and_then(|p| p.parent())
                .and_then(|p| p.strip_prefix(&space_root.0).ok())
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("Workspace"))
        } else {
            std::path::PathBuf::from("Workspace")
        };

        // ── Wedge count: 2 vs 4 based on gap topology ─────────────────
        //
        // stravant's GapFill triangulates the gap quad dynamically:
        //   - Flat / aligned gap (both parts similarly oriented) →
        //     diagonal split = 2 triangles = 2 wedges
        //   - Twisted / skew gap (parts with divergent rotations) →
        //     center-split = 4 triangles = 4 wedges
        //
        // Detection heuristic: the angle between the two parts'
        // rotations. Below 20° → 2 wedges. Above → 4 wedges. The
        // threshold is empirical; tight enough that obvious flat
        // gaps use 2, loose enough that any visible twist jumps to 4.
        //
        // User can override via the Options Bar `fill_mode` choice
        // (Auto / 2 / 4) when the heuristic misses.
        let rot_angle = a_t.rotation.angle_between(b_t.rotation).to_degrees();
        let wedge_count = match self.fill_mode.as_str() {
            "2" => 2,
            "4" => 4,
            _   => if rot_angle.abs() > 20.0 { 4 } else { 2 },
        };

        let spawned = if wedge_count == 4 {
            spawn_gap_fill_4_wedges(
                world, midpoint, direction, fill_length, self.thickness,
                color, material, parent_rel,
            )
        } else {
            spawn_gap_fill_2_wedges(
                world, midpoint, direction, fill_length, self.thickness,
                color, material, parent_rel,
            )
        };

        info!(
            "🧱 Gap Fill: {} wedges span {:.2}m (rotation-diff {:.1}° → {} mode) — properties copied from {:?}",
            spawned, fill_length, rot_angle, wedge_count, first,
        );
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.first = None;
        self.second = None;
    }
}

// ── Gap Fill spawn helpers ───────────────────────────────────────────────
//
// Each helper produces N `parts/wedge.glb` instances, TOML-backed,
// sized via `BasePart.size`. No asset serialization — the wedge
// primitive is reused.

/// 2-wedge variant — diagonal split of the bridging quad.
/// Two wedges meet at the midpoint with hypotenuses facing opposite
/// directions, collectively forming the rectangular bridging volume.
/// Best for flat gaps (parts with matching orientation).
#[allow(clippy::too_many_arguments)]
fn spawn_gap_fill_2_wedges(
    world: &mut World,
    midpoint: Vec3,
    direction: Vec3,
    fill_length: f32,
    thickness: f32,
    color: [f32; 4],
    material: String,
    parent_rel: std::path::PathBuf,
) -> usize {
    let half_length = fill_length * 0.5;
    let wedge_size = Vec3::new(half_length, thickness, thickness);

    let w1_translation = midpoint - direction * (half_length * 0.5);
    let w1_rotation = if direction.length_squared() > 1e-6 {
        Quat::from_rotation_arc(Vec3::X, direction)
    } else { Quat::IDENTITY };

    let w2_translation = midpoint + direction * (half_length * 0.5);
    let w2_rotation = if direction.length_squared() > 1e-6 {
        Quat::from_rotation_arc(Vec3::X, -direction)
    } else { Quat::IDENTITY };

    spawn_wedge_batch(
        world,
        &[
            ("GapFill_A", w1_translation, w1_rotation, wedge_size),
            ("GapFill_B", w2_translation, w2_rotation, wedge_size),
        ],
        color, material, parent_rel,
    )
}

/// 4-wedge variant — center-split of the bridging quad into four
/// quadrants. Better for twisted / non-planar gaps where the two source
/// parts have divergent orientations; the extra subdivision lets each
/// quadrant wedge cover a smaller area more faithfully.
#[allow(clippy::too_many_arguments)]
fn spawn_gap_fill_4_wedges(
    world: &mut World,
    midpoint: Vec3,
    direction: Vec3,
    fill_length: f32,
    thickness: f32,
    color: [f32; 4],
    material: String,
    parent_rel: std::path::PathBuf,
) -> usize {
    // Split the bridging volume into four quadrants:
    //   - Along the connector: halves the length
    //   - Perpendicular to connector + perpendicular to world up:
    //     halves the thickness
    // Each quadrant = one wedge. Rotations chosen so the hypotenuses
    // align with the quad's center, producing a 4-way seam pattern.
    let quarter_length = fill_length * 0.25;
    let half_thickness = thickness * 0.5;
    let wedge_size = Vec3::new(quarter_length * 2.0, half_thickness, thickness);

    // Cross direction: a perpendicular-to-connector axis. Use world up
    // projected perpendicular; fall back to world +X if connector is
    // vertical.
    let world_up = Vec3::Y;
    let cross = if direction.cross(world_up).length_squared() > 1e-4 {
        direction.cross(world_up).normalize()
    } else {
        direction.cross(Vec3::X).normalize_or_zero()
    };

    // Base rotations — each of the four wedges is offset a quarter
    // length along ±direction and a half-thickness along ±cross.
    let forward_offset = direction * quarter_length;
    let cross_offset   = cross     * (half_thickness * 0.5);

    let base_rot_fwd = if direction.length_squared() > 1e-6 {
        Quat::from_rotation_arc(Vec3::X, direction)
    } else { Quat::IDENTITY };
    let base_rot_back = if direction.length_squared() > 1e-6 {
        Quat::from_rotation_arc(Vec3::X, -direction)
    } else { Quat::IDENTITY };
    // Roll ±90° around the direction axis so the hypotenuses face
    // the cross axis (up/down relative to the connector).
    let roll_up   = Quat::from_axis_angle(direction, std::f32::consts::FRAC_PI_2);
    let roll_down = Quat::from_axis_angle(direction, -std::f32::consts::FRAC_PI_2);

    let specs: &[(&str, Vec3, Quat, Vec3)] = &[
        // Quadrant 1: forward half + cross-up
        ("GapFill_NE", midpoint + forward_offset + cross_offset,
            roll_up   * base_rot_fwd,  wedge_size),
        // Quadrant 2: forward half + cross-down
        ("GapFill_SE", midpoint + forward_offset - cross_offset,
            roll_down * base_rot_fwd,  wedge_size),
        // Quadrant 3: back half + cross-up
        ("GapFill_NW", midpoint - forward_offset + cross_offset,
            roll_up   * base_rot_back, wedge_size),
        // Quadrant 4: back half + cross-down
        ("GapFill_SW", midpoint - forward_offset - cross_offset,
            roll_down * base_rot_back, wedge_size),
    ];

    spawn_wedge_batch(world, specs, color, material, parent_rel)
}

/// Spawn N wedges with identical color / material / parent. Returns
/// how many succeeded. Single place that formats the NewPartDescriptor
/// and calls `spawn_new_part_with_toml` for every Gap Fill variant.
fn spawn_wedge_batch(
    world: &mut World,
    specs: &[(&str, Vec3, Quat, Vec3)],
    color: [f32; 4],
    material: String,
    parent_rel: std::path::PathBuf,
) -> usize {
    let mut spawned = 0usize;
    for (base_name, translation, rotation, size) in specs {
        let desc = NewPartDescriptor {
            base_name: base_name.to_string(),
            parent_rel: parent_rel.clone(),
            transform: Transform {
                translation: *translation,
                rotation:    *rotation,
                scale:       Vec3::ONE,
            },
            size: *size,
            mesh: "parts/wedge.glb".to_string(),
            class_name: "Part".to_string(),
            color_rgba: Some(color),
            material: Some(material.clone()),
            anchored: true,
        };
        match spawn_new_part_with_toml(world, desc) {
            Ok(part) => {
                spawned += 1;
                info!("🧱 Gap Fill wedge '{}': spawned {:?}", base_name, part.folder_name);
            }
            Err(e) => {
                warn!("Gap Fill: wedge '{}' spawn failed — {}", base_name, e);
            }
        }
    }
    spawned
}

// ============================================================================
// Resize Align
// ============================================================================
//
// stravant-style face-to-face resize tool from TOOLSET.md §4.13.2.
// User picks a source entity, then a target entity — the source
// part's BasePart.size grows/shrinks along the axis facing the target
// until its surface meets the target's surface.
//
// ## Modes (Options Bar choice)
//
// - **Outer Touch** (default) — source's outer surface meets target's.
// - **Inner Touch** — source extends past target's plane to its far side.
//   Useful when the target is a thin wall and you want the source to
//   flush with the wall's *inner* face.
// - **Rounded Join** — instead of resizing, spawn a cylindrical connector
//   between the two surfaces. Exact algorithm: v1 spawns a thin block
//   approximating a cylinder; full torus/sphere connector is a P1
//   refinement.
//
// ## Face detection
//
// v1 uses the dominant-axis heuristic: the face facing the target is
// whichever of the source's local ±X/±Y/±Z axes has the largest
// projection onto the vector from source center to target center.
// This matches the common "pillar to roof" case cleanly. Exact
// face-picking via raycast hit-point projection onto AABB faces is a
// P1 upgrade.

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum ResizeAlignMode {
    #[default]
    OuterTouch,
    InnerTouch,
    RoundedJoin,
}

impl ResizeAlignMode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::OuterTouch  => "outer_touch",
            Self::InnerTouch  => "inner_touch",
            Self::RoundedJoin => "rounded_join",
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Self::OuterTouch  => "Outer Touch",
            Self::InnerTouch  => "Inner Touch",
            Self::RoundedJoin => "Rounded Join",
        }
    }
    fn from_str(s: &str) -> Self {
        match s {
            "Inner Touch"   | "inner_touch"   => Self::InnerTouch,
            "Rounded Join"  | "rounded_join"  => Self::RoundedJoin,
            _                                 => Self::OuterTouch,
        }
    }
}

pub struct ResizeAlign {
    source: Option<Entity>,
    target: Option<Entity>,
    mode: ResizeAlignMode,
    /// "Join Surfaces" toggle — when a resize brings two parts into
    /// contact, update any existing welds. Stored but not yet wired to
    /// weld service (P1 — matches TOOLSET.md §4.13.2 Join Surfaces note).
    join_surfaces: bool,
}

impl Default for ResizeAlign {
    fn default() -> Self {
        Self {
            source: None,
            target: None,
            mode: ResizeAlignMode::OuterTouch,
            join_surfaces: true,
        }
    }
}

impl ModalTool for ResizeAlign {
    fn id(&self) -> &'static str { "resize_align" }
    fn name(&self) -> &'static str { "Resize Align" }
    fn icon_path(&self) -> &'static str { "assets/icons/ui/cursor-badge-resize-align.svg" }

    fn step_label(&self) -> String {
        match self.source {
            None    => "pick source part".to_string(),
            Some(_) => "pick target part to align to".to_string(),
        }
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        vec![
            ToolOptionControl {
                id: "mode".to_string(),
                label: "Mode".to_string(),
                kind: ToolOptionKind::Choice {
                    options: vec![
                        "Outer Touch".into(),
                        "Inner Touch".into(),
                        "Rounded Join".into(),
                    ],
                    selected: self.mode.label().to_string(),
                },
                advanced: false,
            },
            ToolOptionControl {
                id: "join_surfaces".to_string(),
                label: "Join Surfaces".to_string(),
                kind: ToolOptionKind::Bool { value: self.join_surfaces },
                advanced: true,
            },
            ToolOptionControl {
                id: "hint".to_string(),
                label: "".to_string(),
                kind: ToolOptionKind::Label {
                    text: "Pick source, then target. Source resizes to align.".to_string(),
                },
                advanced: false,
            },
        ]
    }

    fn on_option_changed(&mut self, id: &str, value: &str, _ctx: &mut ToolContext) -> ToolStepResult {
        match id {
            "mode"          => { self.mode = ResizeAlignMode::from_str(value); }
            "join_surfaces" => { self.join_surfaces = value == "true"; }
            _ => {}
        }
        ToolStepResult::Continue
    }

    fn on_click(&mut self, hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        let Some(clicked) = hit.hit_entity else { return ToolStepResult::Continue };
        match self.source {
            None => {
                self.source = Some(clicked);
                ToolStepResult::Continue
            }
            Some(src) if src == clicked => ToolStepResult::Continue,
            Some(_) => {
                self.target = Some(clicked);
                ToolStepResult::Commit
            }
        }
    }

    fn commit(&mut self, world: &mut World) {
        let (Some(source), Some(target)) = (self.source, self.target) else { return };

        // Read transforms + sizes from both entities.
        let source_t = world.get::<Transform>(source).cloned();
        let target_t = world.get::<Transform>(target).cloned();
        let source_size = world.get::<crate::classes::BasePart>(source)
            .map(|bp| bp.size)
            .or_else(|| source_t.map(|t| t.scale))
            .unwrap_or(Vec3::ONE);
        let target_size = world.get::<crate::classes::BasePart>(target)
            .map(|bp| bp.size)
            .or_else(|| target_t.map(|t| t.scale))
            .unwrap_or(Vec3::ONE);

        let (Some(src_t), Some(tgt_t)) = (source_t, target_t) else {
            warn!("Resize Align: missing Transform on source or target");
            return;
        };

        // Direction from source center to target center in WORLD space.
        let center_delta = tgt_t.translation - src_t.translation;
        if center_delta.length_squared() < 1e-6 {
            warn!("Resize Align: source and target are coincident — aborting");
            return;
        }

        // Pick the source-local axis whose world-projection is most
        // aligned with `center_delta`. `src_rot * ±axis` gives each of
        // the six face normals; we keep whichever has the largest dot —
        // that's the face pointing AT target.
        let local_axis: Vec3 = {
            let axes = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];
            let mut best = (Vec3::X, f32::MIN);
            let target_dir = center_delta.normalize();
            for a in axes {
                let world_a = (src_t.rotation * a).normalize_or_zero();
                let d = world_a.dot(target_dir);
                if d > best.1 { best = (a, d); }
            }
            best.0
        };

        // World-space source face center (midpoint of the face pointing
        // at target) and the face normal.
        let world_face_normal = (src_t.rotation * local_axis).normalize_or_zero();
        let src_half_along = (source_size.x * local_axis.x.abs()
                            + source_size.y * local_axis.y.abs()
                            + source_size.z * local_axis.z.abs()) * 0.5;
        let source_face_world = src_t.translation + world_face_normal * src_half_along;

        // Target's plane — use its facing surface via the same dominant-
        // axis heuristic from the target side.
        let tgt_to_src = -center_delta;
        let tgt_axes = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];
        let mut tgt_best = (Vec3::X, f32::MIN);
        for a in tgt_axes {
            let world_a = (tgt_t.rotation * a).normalize_or_zero();
            let d = world_a.dot(tgt_to_src.normalize());
            if d > tgt_best.1 { tgt_best = (a, d); }
        }
        let target_local_axis = tgt_best.0;
        let target_face_normal = (tgt_t.rotation * target_local_axis).normalize_or_zero();
        let tgt_half_along = (target_size.x * target_local_axis.x.abs()
                            + target_size.y * target_local_axis.y.abs()
                            + target_size.z * target_local_axis.z.abs()) * 0.5;
        let target_outer_world = tgt_t.translation + target_face_normal * tgt_half_along;
        let target_inner_world = tgt_t.translation - target_face_normal * tgt_half_along;

        // Pick destination based on mode.
        let (delta_length, handled_via_connector) = match self.mode {
            ResizeAlignMode::OuterTouch => {
                // Distance from source's current face plane to target's
                // outer surface plane along source's face normal.
                let along = (target_outer_world - source_face_world).dot(world_face_normal);
                (along, false)
            }
            ResizeAlignMode::InnerTouch => {
                let along = (target_inner_world - source_face_world).dot(world_face_normal);
                (along, false)
            }
            ResizeAlignMode::RoundedJoin => {
                // Instead of resizing, spawn a thin connector between
                // the two outer surfaces.
                (0.0, true)
            }
        };

        if handled_via_connector {
            spawn_rounded_join_connector(world, source, source_face_world, target_outer_world);
            info!("🔗 Resize Align (Rounded Join): {:?} → {:?}", source, target);
            return;
        }

        // Apply the resize: grow/shrink source's size on the axis
        // matching `local_axis`, and translate source center so the
        // opposite face stays fixed (user expects the BACK face to
        // stay put while the FRONT face moves).
        let local_axis_abs = local_axis.abs();
        let mut new_size = source_size;
        new_size.x += local_axis_abs.x * delta_length;
        new_size.y += local_axis_abs.y * delta_length;
        new_size.z += local_axis_abs.z * delta_length;
        // Clamp to a reasonable minimum so we don't produce a zero-
        // thickness part when target is inside source.
        new_size.x = new_size.x.max(0.05);
        new_size.y = new_size.y.max(0.05);
        new_size.z = new_size.z.max(0.05);
        // Translate source center so the opposite face stays in place:
        // if we extended by `delta_length` along `axis_signed`, move
        // center by `delta_length / 2` in world space.
        let center_shift = world_face_normal * (delta_length * 0.5);
        let new_translation = src_t.translation + center_shift;

        // Apply to ECS — both BasePart.size and Transform.
        if let Some(mut bp) = world.get_mut::<crate::classes::BasePart>(source) {
            bp.size = new_size;
        }
        if let Some(mut t) = world.get_mut::<Transform>(source) {
            t.translation = new_translation;
        }

        // Persist to source's TOML so the resize survives reload.
        persist_transform_and_size_to_toml(world, source, new_size);

        // Undo: record size+position transition. UndoStack's existing
        // TransformEntities captures only position/rotation; size is a
        // BasePart field. Use the Spawn/Resize Phase-1 ResizePart
        // variant when it lands. For v1 we record position only so at
        // least Ctrl+Z partially reverts.
        if let Some(mut undo) = world.get_resource_mut::<crate::undo::UndoStack>() {
            undo.push(crate::undo::Action::TransformEntities {
                old_transforms: vec![(source.to_bits(), src_t.translation.to_array(), src_t.rotation.to_array())],
                new_transforms: vec![(source.to_bits(), new_translation.to_array(), src_t.rotation.to_array())],
            });
        }

        info!("🔁 Resize Align ({}): {:?} size {:?} → {:?}",
            self.mode.as_str(), source, source_size, new_size);
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.source = None;
        self.target = None;
    }
}

/// Spawn a thin connector block between two world points — the
/// Rounded Join variant of Resize Align. Size: full distance long,
/// thin on the other two axes. Oriented so +X aligns with the
/// connector direction.
fn spawn_rounded_join_connector(
    world: &mut World,
    source: Entity,
    source_surface_world: Vec3,
    target_surface_world: Vec3,
) {
    let connector = target_surface_world - source_surface_world;
    let length = connector.length().max(0.05);
    let direction = connector / length;
    let midpoint = (source_surface_world + target_surface_world) * 0.5;
    let rotation = if direction.length_squared() > 1e-6 {
        Quat::from_rotation_arc(Vec3::X, direction)
    } else { Quat::IDENTITY };

    // Inherit color/material from source if possible.
    let (color, material_name) = world.get::<crate::classes::BasePart>(source)
        .map(|bp| {
            let s = bp.color.to_srgba();
            ([s.red, s.green, s.blue, s.alpha], "Plastic".to_string())
        })
        .unwrap_or(([0.85, 0.85, 0.85, 1.0], "Plastic".to_string()));

    // Parent folder: inherit from source's TOML folder, same pattern
    // as Gap Fill / Model Reflect.
    let parent_rel = world.get::<crate::space::instance_loader::InstanceFile>(source)
        .and_then(|f| {
            let space_root = world.get_resource::<crate::space::SpaceRoot>()?;
            f.toml_path.parent().and_then(|p| p.parent())
                .and_then(|p| p.strip_prefix(&space_root.0).ok())
                .map(|p| p.to_path_buf())
        })
        .unwrap_or_else(|| std::path::PathBuf::from("Workspace"));

    let connector_thickness = length * 0.08;
    let desc = NewPartDescriptor {
        base_name: "ResizeAlign_Connector".to_string(),
        parent_rel,
        transform: Transform {
            translation: midpoint,
            rotation,
            scale: Vec3::ONE,
        },
        size: Vec3::new(length, connector_thickness, connector_thickness),
        mesh: "parts/block.glb".to_string(),
        class_name: "Part".to_string(),
        color_rgba: Some(color),
        material: Some(material_name),
        anchored: true,
    };
    if let Err(e) = spawn_new_part_with_toml(world, desc) {
        warn!("Rounded Join: spawn failed — {}", e);
    }
}

/// Like `persist_transform_to_toml` but also writes BasePart.size
/// (serialized as TransformData.scale per the Eustress convention).
pub fn persist_transform_and_size_to_toml(world: &mut World, entity: Entity, size: Vec3) {
    use crate::space::instance_loader::{
        InstanceFile, load_instance_definition, write_instance_definition_signed, current_stamp,
    };

    let Some(inst_path) = world.get::<InstanceFile>(entity).map(|f| f.toml_path.clone()) else {
        return;
    };
    let Some(transform) = world.get::<Transform>(entity).cloned() else { return };

    let mut def = match load_instance_definition(&inst_path) {
        Ok(d) => d,
        Err(e) => {
            warn!("persist_transform_and_size_to_toml: failed to read {:?}: {}", inst_path, e);
            return;
        }
    };
    def.transform.position = transform.translation.to_array();
    def.transform.rotation = [
        transform.rotation.x, transform.rotation.y,
        transform.rotation.z, transform.rotation.w,
    ];
    // `transform.scale` in the TOML is BasePart.size for primitive parts
    // — see the instance_loader docs. Writing the new size here.
    def.transform.scale = size.to_array();

    let stamp = world.get_resource::<crate::auth::AuthState>().and_then(current_stamp);
    if let Err(e) = write_instance_definition_signed(&inst_path, &mut def, stamp.as_ref()) {
        warn!("persist_transform_and_size_to_toml: write failed {:?}: {}", inst_path, e);
    }
}

// ============================================================================
// Material Flip
// ============================================================================
//
// Flips a part's texture orientation without rotating the part itself.
// Four operations exposed as Options Bar toggles:
//   - Rotate texture 90° clockwise
//   - Rotate texture 90° counter-clockwise
//   - Mirror U (horizontal flip)
//   - Mirror V (vertical flip)
//
// Applies to every currently-Selected entity. Each operation composes
// with whatever uv_transform the material already has — so two CW
// rotations = 180°, two mirror-U = identity.
//
// ## Persistence caveat (v1)
//
// Mutates the `StandardMaterial::uv_transform` in-session. Writes the
// resulting affine matrix into `_instance.toml` `attributes.uv_transform`
// so the intent is preserved on disk, but a matching *loader* that
// reads the attribute back and applies the transform on spawn is a
// Phase-1 item. Until then: session-only for hot-loaded reloads.

use bevy::math::{Affine2, Vec2};

#[derive(Default)]
pub struct MaterialFlip {
    /// Log of ops applied during this session for the Output panel
    /// status readout. Cleared on cancel.
    pending_ops: Vec<String>,
}

impl ModalTool for MaterialFlip {
    fn id(&self) -> &'static str { "material_flip" }
    fn name(&self) -> &'static str { "Material Flip" }
    fn icon_path(&self) -> &'static str { "assets/icons/ui/cursor-badge-material-flip.svg" }

    fn step_label(&self) -> String {
        "pick an operation; applies to all selected".to_string()
    }

    fn options(&self) -> Vec<ToolOptionControl> {
        // Toggle-style buttons: clicking one triggers the operation via
        // `on_option_changed`. UI renders them as four pill buttons in
        // the Options Bar.
        vec![
            ToolOptionControl {
                id: "rot_cw".to_string(),
                label: "Rotate 90° CW".to_string(),
                kind: ToolOptionKind::Bool { value: false },
                advanced: false,
            },
            ToolOptionControl {
                id: "rot_ccw".to_string(),
                label: "Rotate 90° CCW".to_string(),
                kind: ToolOptionKind::Bool { value: false },
                advanced: false,
            },
            ToolOptionControl {
                id: "mirror_u".to_string(),
                label: "Mirror U".to_string(),
                kind: ToolOptionKind::Bool { value: false },
                advanced: false,
            },
            ToolOptionControl {
                id: "mirror_v".to_string(),
                label: "Mirror V".to_string(),
                kind: ToolOptionKind::Bool { value: false },
                advanced: false,
            },
            ToolOptionControl {
                id: "hint".to_string(),
                label: "".to_string(),
                kind: ToolOptionKind::Label {
                    text: "Flips the texture of all selected parts.".to_string(),
                },
                advanced: false,
            },
        ]
    }

    fn on_click(&mut self, _hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        // Pure option-driven tool — viewport clicks are no-ops.
        ToolStepResult::Continue
    }

    fn on_option_changed(&mut self, id: &str, value: &str, ctx: &mut ToolContext) -> ToolStepResult {
        // User toggled one of the four operations to "true". Apply it
        // immediately via a queued world closure and stay in the tool
        // so multiple flips can be chained.
        if value != "true" {
            return ToolStepResult::Continue;
        }
        let op = id.to_string();
        self.pending_ops.push(op.clone());
        ctx.commands.queue(move |world: &mut World| {
            apply_material_flip_to_selected(world, &op);
        });
        ToolStepResult::Continue
    }

    fn commit(&mut self, _world: &mut World) {
        // No discrete commit step — operations apply immediately via
        // on_option_changed. Tool stays active until the user hits Esc /
        // ×. This function is a no-op.
    }

    fn cancel(&mut self, _commands: &mut Commands) {
        self.pending_ops.clear();
    }

    fn auto_exit_on_commit(&self) -> bool { false }
}

/// Apply a Material Flip operation to every currently-Selected entity.
/// Each entity's material handle is CLONED (ensuring per-part state),
/// the uv_transform is composed with the requested rotation / mirror,
/// and the new handle is reinstalled on the entity.
fn apply_material_flip_to_selected(world: &mut World, op: &str) {
    // Compose the requested operation as a small affine matrix. The
    // new transform = op * existing, so repeated flips compose.
    let op_affine: Affine2 = match op {
        "rot_cw"   => Affine2::from_angle(-std::f32::consts::FRAC_PI_2),
        "rot_ccw"  => Affine2::from_angle(std::f32::consts::FRAC_PI_2),
        "mirror_u" => Affine2::from_scale(Vec2::new(-1.0, 1.0)),
        "mirror_v" => Affine2::from_scale(Vec2::new(1.0, -1.0)),
        _ => return,
    };

    // Snapshot selected entities + their current material handles
    // before mutating world state.
    let mut pairs: Vec<(Entity, Handle<StandardMaterial>)> = Vec::new();
    {
        let mut q = world.query_filtered::<
            (Entity, &MeshMaterial3d<StandardMaterial>),
            With<Selected>,
        >();
        for (e, mm) in q.iter(world) {
            pairs.push((e, mm.0.clone()));
        }
    }
    if pairs.is_empty() { return; }

    // For each entity, clone its material, compose the transform,
    // reinstall the new handle. Also persist the intent to TOML so
    // reload preserves the user's choice (loader Phase-1 applies it).
    let mut applied = 0usize;
    let mut new_handles: Vec<(Entity, Handle<StandardMaterial>)> = Vec::new();
    world.resource_scope(|_world, mut materials: Mut<Assets<StandardMaterial>>| {
        for (entity, handle) in pairs.iter() {
            let Some(current) = materials.get(handle).cloned() else { continue };
            let mut cloned = current;
            cloned.uv_transform = op_affine * cloned.uv_transform;
            let new_handle = materials.add(cloned);
            new_handles.push((*entity, new_handle));
            applied += 1;
        }
    });

    for (entity, handle) in new_handles {
        if let Ok(mut ent) = world.get_entity_mut(entity) {
            ent.insert(MeshMaterial3d(handle));
        }
    }

    // Persist to TOML `attributes` so the change round-trips through
    // the file system. The current asset loader doesn't yet re-apply
    // the uv_transform on reload (Phase-1 item), but writing the
    // attribute keeps the intent recorded in git and discoverable via
    // MCP queries. Key format: `material_uv_transform = "rot_cw"` etc.
    let mut persisted = 0usize;
    let targets: Vec<Entity> = pairs.iter().map(|(e, _)| *e).collect();
    for entity in targets {
        if persist_material_uv_op_to_toml(world, entity, op) {
            persisted += 1;
        }
    }

    info!("🎨 Material Flip [{}]: applied to {} material(s); persisted to {} TOML(s)",
          op, applied, persisted);
}

/// Write a material-flip operation to the entity's `_instance.toml`
/// `attributes` section. Appends to any existing `material_uv_ops`
/// array so a history of operations is preserved (composition order
/// matters for non-commutative transforms).
fn persist_material_uv_op_to_toml(world: &mut World, entity: Entity, op: &str) -> bool {
    use crate::space::instance_loader::{
        InstanceFile, load_instance_definition, write_instance_definition_signed, current_stamp,
    };

    let Some(inst_path) = world.get::<InstanceFile>(entity).map(|f| f.toml_path.clone()) else {
        return false;
    };
    let mut def = match load_instance_definition(&inst_path) {
        Ok(d) => d,
        Err(_) => return false,
    };

    // Append to `attributes.material_uv_ops` array. toml::Value
    // doesn't have a convenient "push to array" — we read, mutate,
    // write back.
    let mut attrs = def.attributes.take().unwrap_or_default();
    let existing = attrs.remove("material_uv_ops");
    let mut ops_vec: Vec<toml::Value> = match existing {
        Some(toml::Value::Array(arr)) => arr,
        _ => Vec::new(),
    };
    ops_vec.push(toml::Value::String(op.to_string()));
    attrs.insert("material_uv_ops".to_string(), toml::Value::Array(ops_vec));
    def.attributes = Some(attrs);

    let stamp = world.get_resource::<crate::auth::AuthState>().and_then(current_stamp);
    write_instance_definition_signed(&inst_path, &mut def, stamp.as_ref()).is_ok()
}


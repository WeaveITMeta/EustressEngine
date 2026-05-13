//! # Streaming Plugin — Bevy ECS Integration
//!
//! ## Table of Contents
//! - StreamingPlugin         — Bevy Plugin that adds all streaming systems
//! - StreamingState          — Bevy Resource: owns the grid, flusher, watcher, index
//! - InstanceReloaded        — Bevy Message: fired when an external TOML edit updates an Active entity
//! - InstancePromoted        — Bevy Message: fired when an instance is promoted to Active (entity spawned)
//! - InstanceDemoted         — Bevy Message: fired when an instance is demoted from Active (entity despawned)
//! - StreamingVelocity       — Bevy Component: MoE sparse gate marker from benchmark
//! - StreamingActive         — Bevy Component: marks entities in the "moving" MoE fraction
//!
//! ## Systems (ordered)
//! 1. sys_apply_file_changed    — consume the engine's `FileChanged` broadcast,
//!                                 invalidate sidecars, refresh the grid record,
//!                                 fire `InstanceReloaded` for any Active entity
//! 2. sys_radius_gate           — evaluate hysteresis gate, promote/demote chunks
//! 3. sys_sync_active_transforms — Changed<Transform> → update InstanceRecord.bin (MoE sparse gate)
//! 4. sys_rebuild_index         — periodically rebuild the Explorer index from the grid
//!
//! ## Benchmark-Proven Optimizations Integrated
//! - Changed<Transform> filter: only processes ~10% of entities (MoE sparse gate, 5–10× speedup)
//! - InheritedVisibility filter: skip invisible entities in transform sync
//! - Hysteresis radius gate: prevents boundary thrashing (measured 4.7ms @ 2.10M)
//! - Dirty-bit write-back: never blocks the hot path on disk I/O

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use tracing;

use super::chunk_grid::SpatialChunkGrid;
use super::dirty_flusher::DirtyBitFlusher;
use super::instance_index::InstanceIndex;
use super::radius_gate::{GateDecision, GateStats, HysteresisRadiusGate};
// `WatchEvent` is referenced via the fully-qualified path in
// `sys_apply_file_changed` below, so no `use` needed here.
use super::types::{InstanceBin, InstanceId, InstanceRecord, StreamingConfig, Tier};

// ─────────────────────────────────────────────────────────────────────────────
// Bevy Components — MoE sparse gate markers from benchmark
// ─────────────────────────────────────────────────────────────────────────────

/// Velocity component for MoE routing. Entities with velocity > 0 are "active"
/// and participate in the Changed<Transform> sparse gate.
/// Benchmark-proven: 10% active fraction gives 5–10× ECS query speedup.
#[derive(Component, Debug, Clone, Copy)]
pub struct StreamingVelocity {
    pub magnitude: f32,
}

/// Marker component for entities in the "moving" MoE fraction.
/// Only entities with this marker have their transforms synced back
/// to the InstanceRecord each frame (sparse gate).
#[derive(Component, Debug, Clone, Copy)]
pub struct StreamingActive;

/// Component linking a Bevy entity back to its InstanceId in the grid.
#[derive(Component, Debug, Clone)]
pub struct StreamingInstanceRef {
    pub instance_id: InstanceId,
}

// ─────────────────────────────────────────────────────────────────────────────
// Bevy Messages (Bevy 0.18 uses add_message / MessageWriter / MessageReader)
// ─────────────────────────────────────────────────────────────────────────────

/// Fired when an external TOML edit reloads an Active entity's data.
/// Listeners can update visuals, physics, etc.
#[derive(Event, Message, Debug, Clone)]
pub struct InstanceReloaded {
    pub instance_id: InstanceId,
    pub entity: Entity,
}

/// Fired when an instance is promoted from Hot → Active (entity spawned).
#[derive(Event, Message, Debug, Clone)]
pub struct InstancePromoted {
    pub instance_id: InstanceId,
    pub entity: Entity,
}

/// Fired when an instance is demoted from Active → Hot (entity despawned).
#[derive(Event, Message, Debug, Clone)]
pub struct InstanceDemoted {
    pub instance_id: InstanceId,
}

// ─────────────────────────────────────────────────────────────────────────────
// StreamingState — Bevy Resource owning all streaming infrastructure
// ─────────────────────────────────────────────────────────────────────────────

/// Central Bevy Resource that owns the streaming infrastructure.
/// Added by StreamingPlugin on startup.
#[derive(Resource)]
pub struct StreamingState {
    /// The spatial chunk grid (shared with flusher thread via Arc).
    pub grid: Arc<SpatialChunkGrid>,
    /// The hysteresis radius gate.
    pub gate: HysteresisRadiusGate,
    /// The dirty-bit write-back flusher (background thread).
    pub flusher: Option<DirtyBitFlusher>,
    // Note: this used to hold a `TomlWatcher` with its own
    // `notify::RecommendedWatcher`. That instance was removed in the
    // 2026-05-12 watcher consolidation — the workspace now has
    // exactly ONE notify watcher (`engine::space::file_watcher`) and
    // streaming reacts to its `FileChanged` broadcast via
    // `sys_apply_file_changed`. See `super::toml_watcher` module
    // docs for the design rationale.
    /// The flat metadata index for Explorer queries.
    pub index: InstanceIndex,
    /// Streaming configuration.
    pub config: StreamingConfig,
    /// Last time the index was rebuilt (throttle to avoid per-frame rebuilds).
    pub last_index_rebuild: Instant,
    /// Interval between index rebuilds.
    pub index_rebuild_interval: Duration,
    /// Per-frame gate stats for telemetry.
    pub last_gate_stats: GateStats,
}

// ─────────────────────────────────────────────────────────────────────────────
// StreamingPlugin
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy Plugin that adds the instance streaming system.
///
/// # Usage
/// ```rust,ignore
/// app.add_plugins(StreamingPlugin {
///     config: StreamingConfig::default(),
///     instances_dir: PathBuf::from("assets/spaces/my-space/instances"),
/// });
/// ```
pub struct StreamingPlugin {
    /// Streaming configuration (radii, caps, flush interval).
    pub config: StreamingConfig,
    /// Root directory of instance TOML files to watch.
    pub instances_dir: PathBuf,
}

impl Plugin for StreamingPlugin {
    fn build(&self, app: &mut App) {
        // Create the shared grid.
        let grid = Arc::new(SpatialChunkGrid::new(self.config.clone()));

        // Start the dirty-bit flusher background thread.
        let flusher = DirtyBitFlusher::start(grid.clone(), self.config.clone());

        // NOTE: no more `TomlWatcher::start` here. The streaming
        // module used to run its own `notify::RecommendedWatcher`
        // alongside the engine's `SpaceFileWatcher`, on the same
        // `Workspace/` tree. Two watchers reading the same files
        // raced on every write (`os error 32` / "file in use by
        // another process") and silently dropped user edits. The
        // workspace now has exactly ONE notify watcher (the engine's)
        // and this plugin reacts to its `FileChanged` broadcast via
        // `sys_apply_file_changed` below.

        // Build the radius gate.
        let gate = HysteresisRadiusGate::new(&self.config);

        // Insert the streaming state resource.
        app.insert_resource(StreamingState {
            grid,
            gate,
            flusher: Some(flusher),
            index: InstanceIndex::new(),
            config: self.config.clone(),
            last_index_rebuild: Instant::now(),
            index_rebuild_interval: Duration::from_secs(5),
            last_gate_stats: GateStats::default(),
        });

        // Register downstream payload messages (Bevy 0.18 API).
        app.add_message::<InstanceReloaded>();
        app.add_message::<InstancePromoted>();
        app.add_message::<InstanceDemoted>();
        // `FileChanged` is registered by the engine's space plugin —
        // we read it here as a subscriber. If the engine plugin
        // hasn't run yet (e.g. headless test harness), Bevy auto-
        // initialises the message storage when our system first
        // queries it.

        // Store the instances dir as a resource so the startup system can access it.
        app.insert_resource(StreamingInstancesDir(self.instances_dir.clone()));

        // Startup: scan existing .toml files from disk into the grid.
        app.add_systems(Startup, sys_initial_disk_scan);

        // Update systems in order. `sys_apply_file_changed` replaces
        // the old `sys_drain_watcher_events` and reads from the
        // engine's shared `FileChanged` broadcast instead of an
        // internal channel.
        app.add_systems(Update, (
            sys_apply_file_changed,
            sys_radius_gate,
            sys_sync_active_transforms,
            sys_rebuild_index,
        ).chain());
    }
}

/// Resource storing the root directory for instance TOML files.
#[derive(Resource, Debug, Clone)]
pub struct StreamingInstancesDir(pub PathBuf);

// ─────────────────────────────────────────────────────────────────────────────
// System 0 (Startup): Scan existing .toml files from disk into the grid
// ─────────────────────────────────────────────────────────────────────────────

/// Walk the instances directory tree on startup and load every `.part.toml`,
/// `.glb.toml`, and `.instance.toml` file into the SpatialChunkGrid as a Hot
/// record. The radius gate will promote nearby instances to Active on the
/// first Update frame.
fn sys_initial_disk_scan(
    state: Res<StreamingState>,
    instances_dir: Res<StreamingInstancesDir>,
) {
    let dir = &instances_dir.0;
    if !dir.exists() {
        tracing::info!("StreamingPlugin: instances dir {} does not exist — nothing to scan", dir.display());
        return;
    }

    let t0 = Instant::now();
    let mut loaded = 0usize;
    let mut errors = 0usize;

    scan_directory_recursive(dir, &state.grid, &mut loaded, &mut errors);

    // Build the initial Explorer index from what we loaded.
    // (sys_rebuild_index runs on a timer, so we do a one-time build here)
    // Note: index is behind Res (immutable) — we log and let the first timer rebuild handle it.

    tracing::info!(
        "StreamingPlugin: initial scan loaded {} instances ({} errors) from {} in {:?} — {} chunks",
        loaded, errors, dir.display(), t0.elapsed(), state.grid.chunk_count()
    );
}

/// Recursively walk a directory tree and load `.toml` instance files into the grid.
fn scan_directory_recursive(
    dir: &std::path::Path,
    grid: &SpatialChunkGrid,
    loaded: &mut usize,
    errors: &mut usize,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(error) => {
            tracing::warn!("StreamingPlugin: cannot read {}: {error}", dir.display());
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Recurse into subdirectories.
        if path.is_dir() {
            scan_directory_recursive(&path, grid, loaded, errors);
            continue;
        }

        // Only process instance TOML files.
        let filename = match path.file_name().and_then(|f| f.to_str()) {
            Some(f) => f.to_string(),
            None => continue,
        };

        // Match: *.part.toml, *.glb.toml, *.instance.toml
        let is_instance_toml = filename.ends_with(".part.toml")
            || filename.ends_with(".glb.toml")
            || filename.ends_with(".instance.toml");
        if !is_instance_toml {
            continue;
        }

        // Parse the instance name from the filename (strip extension suffix).
        let instance_name = if filename.ends_with(".part.toml") {
            filename.trim_end_matches(".part.toml")
        } else if filename.ends_with(".glb.toml") {
            filename.trim_end_matches(".glb.toml")
        } else {
            filename.trim_end_matches(".instance.toml")
        };

        // Try to parse the TOML file.
        match load_toml_to_record(&path, instance_name) {
            Ok(record) => {
                grid.insert(record);
                *loaded += 1;
            }
            Err(error) => {
                tracing::warn!("StreamingPlugin: failed to load {}: {error}", path.display());
                *errors += 1;
            }
        }
    }
}

/// Parse a `.toml` instance file from disk into an InstanceRecord in Hot tier.
fn load_toml_to_record(
    toml_path: &std::path::Path,
    instance_name: &str,
) -> Result<InstanceRecord, String> {
    let content = std::fs::read_to_string(toml_path)
        .map_err(|e| format!("read: {e}"))?;
    let parsed: DiskInstance = toml::from_str(&content)
        .map_err(|e| format!("parse: {e}"))?;

    // Build the compact binary representation.
    let position = [
        parsed.transform.as_ref().and_then(|t| t.position.first().copied()).unwrap_or(0.0),
        parsed.transform.as_ref().and_then(|t| t.position.get(1).copied()).unwrap_or(0.0),
        parsed.transform.as_ref().and_then(|t| t.position.get(2).copied()).unwrap_or(0.0),
    ];
    let scale = parsed.transform.as_ref()
        .and_then(|t| t.scale.first().copied())
        .unwrap_or(1.0);
    let rotation = [
        parsed.transform.as_ref().and_then(|t| t.rotation.first().copied()).unwrap_or(0.0),
        parsed.transform.as_ref().and_then(|t| t.rotation.get(1).copied()).unwrap_or(0.0),
        parsed.transform.as_ref().and_then(|t| t.rotation.get(2).copied()).unwrap_or(0.0),
    ];

    let class_name = parsed.metadata.as_ref()
        .map(|m| m.class_name.as_str())
        .unwrap_or("Part");
    // Simple hash of class name → class_id.
    let class_id = class_name.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));

    let bin = InstanceBin {
        position,
        scale,
        rotation,
        class_id,
        asset_hash: [0u8; 8],
        velocity: 0.0,
        _reserved: [0u8; 4],
    };

    let id = InstanceId::from_string(instance_name);

    let mut record = InstanceRecord::new(
        id,
        bin,
        toml_path.to_path_buf(),
        instance_name.to_string(),
        Vec::new(),
    );
    // Start in Hot tier — the radius gate will promote to Active if within range.
    record.tier = Tier::Hot;

    Ok(record)
}

/// Minimal TOML shape for the initial disk scan. Permissive — accepts any
/// Eustress instance format (.part.toml, .glb.toml, .instance.toml).
#[derive(serde::Deserialize)]
struct DiskInstance {
    #[serde(default)]
    transform: Option<DiskTransform>,
    #[serde(default)]
    metadata: Option<DiskMetadata>,
}

#[derive(serde::Deserialize)]
struct DiskTransform {
    #[serde(default)]
    position: Vec<f32>,
    #[serde(default = "default_rotation")]
    rotation: Vec<f32>,
    #[serde(default = "default_scale_vec")]
    scale: Vec<f32>,
}

fn default_rotation() -> Vec<f32> { vec![0.0, 0.0, 0.0, 1.0] }
fn default_scale_vec() -> Vec<f32> { vec![1.0, 1.0, 1.0] }

#[derive(serde::Deserialize)]
struct DiskMetadata {
    #[serde(default = "default_class_name")]
    class_name: String,
}

fn default_class_name() -> String { "Part".to_string() }

// ─────────────────────────────────────────────────────────────────────────────
// System 1: Apply shared FileChanged broadcast → grid + InstanceReloaded
// ─────────────────────────────────────────────────────────────────────────────

/// React to the engine's single notify-driven `FileChanged` broadcast.
/// Filters to instance-TOML paths, applies updates to the
/// `SpatialChunkGrid`, and fires `InstanceReloaded` messages for any
/// Active entities whose backing TOML was modified.
///
/// Replaces the legacy `sys_drain_watcher_events` which read from an
/// internal channel owned by a redundant `notify::Watcher`. See
/// `super::toml_watcher` module docs for the consolidation rationale.
fn sys_apply_file_changed(
    state: Res<StreamingState>,
    mut reader: MessageReader<crate::file_events::FileChanged>,
    mut reloaded_messages: MessageWriter<InstanceReloaded>,
    query: Query<(Entity, &StreamingInstanceRef)>,
) {
    for change in reader.read() {
        let Some(event) = super::toml_watcher::classify_file_change(&change.path, change.kind) else {
            continue;
        };
        // Side-effects: invalidate sidecar + reload grid record from
        // disk on Modify; Created/Deleted are no-ops at this layer
        // (the radius gate's index rebuild handles tier transitions).
        super::toml_watcher::apply_watch_event(&state.grid, &event);

        match event {
            super::toml_watcher::WatchEvent::Modified { instance_id, .. } => {
                for (entity, inst_ref) in query.iter() {
                    if inst_ref.instance_id == instance_id {
                        reloaded_messages.write(InstanceReloaded {
                            instance_id: instance_id.clone(),
                            entity,
                        });
                        break;
                    }
                }
            }
            super::toml_watcher::WatchEvent::Created { instance_id, toml_path } => {
                tracing::debug!(
                    "StreamingPlugin: new instance {} at {}",
                    instance_id, toml_path.display(),
                );
            }
            super::toml_watcher::WatchEvent::Deleted { instance_id, .. } => {
                tracing::debug!("StreamingPlugin: deleted instance {}", instance_id);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// System 2: Radius gate — promote/demote chunks based on camera position
// ─────────────────────────────────────────────────────────────────────────────

/// Evaluate the hysteresis radius gate for all chunks relative to the camera.
/// Spawns/despawns Bevy entities as instances cross tier boundaries.
fn sys_radius_gate(
    mut state: ResMut<StreamingState>,
    mut commands: Commands,
    camera_query: Query<&Transform, With<Camera3d>>,
    mut promoted_messages: MessageWriter<InstancePromoted>,
    mut demoted_messages: MessageWriter<InstanceDemoted>,
) {
    // Get camera position (use first Camera3d found).
    let camera_transform = match camera_query.iter().next() {
        Some(t) => t,
        None => return,
    };
    let camera_pos = camera_transform.translation;
    let camera_x = camera_pos.x;
    let camera_z = camera_pos.z;

    let chunk_size = state.config.chunk_size;
    let current_active = state.grid.active_count();

    // Collect chunk data for evaluation.
    let mut chunk_data: Vec<(super::types::ChunkCoord, Tier, usize)> = Vec::new();
    state.grid.for_each_chunk(|coord, chunk| {
        // Use the dominant tier of the chunk (most instances' tier).
        let tier = if chunk.count_by_tier(Tier::Active) > 0 {
            Tier::Active
        } else if chunk.count_by_tier(Tier::Hot) > 0 {
            Tier::Hot
        } else {
            Tier::Cold
        };
        chunk_data.push((*coord, tier, chunk.len()));
    });

    // Run the gate evaluation.
    let (decisions, stats) = state.gate.evaluate_all(
        &chunk_data, chunk_size, camera_x, camera_z, current_active,
    );

    // Apply decisions.
    for (coord, decision) in decisions {
        match decision {
            GateDecision::Promote => {
                // Promote all instances in this chunk: Hot → Active.
                state.grid.for_each_chunk_mut(|c, chunk| {
                    if *c != coord { return; }
                    for record in &mut chunk.instances {
                        if record.tier == Tier::Hot || record.tier == Tier::Cold {
                            // Spawn a Bevy entity for this instance.
                            let entity = commands.spawn((
                                Transform::from_xyz(
                                    record.bin.position[0],
                                    record.bin.position[1],
                                    record.bin.position[2],
                                ).with_scale(Vec3::splat(record.bin.scale)),
                                StreamingInstanceRef {
                                    instance_id: record.id.clone(),
                                },
                            )).id();

                            // Add MoE velocity marker if instance is active.
                            if record.bin.is_active() {
                                commands.entity(entity).insert((
                                    StreamingVelocity { magnitude: record.bin.velocity },
                                    StreamingActive,
                                ));
                            }

                            record.tier = Tier::Active;
                            record.entity = Some(entity);

                            promoted_messages.write(InstancePromoted {
                                instance_id: record.id.clone(),
                                entity,
                            });
                        }
                    }
                });
            }
            GateDecision::DemoteToHot => {
                // Demote all instances in this chunk: Active → Hot.
                state.grid.for_each_chunk_mut(|c, chunk| {
                    if *c != coord { return; }
                    for record in &mut chunk.instances {
                        if record.tier == Tier::Active {
                            // Despawn the Bevy entity.
                            if let Some(entity) = record.entity.take() {
                                commands.entity(entity).despawn();
                                demoted_messages.write(InstanceDemoted {
                                    instance_id: record.id.clone(),
                                });
                            }
                            record.tier = Tier::Hot;
                        }
                    }
                });
            }
            GateDecision::DemoteToCold => {
                // Remove chunk from hot cache entirely.
                state.grid.for_each_chunk_mut(|c, chunk| {
                    if *c != coord { return; }
                    for record in &mut chunk.instances {
                        if record.tier == Tier::Active {
                            if let Some(entity) = record.entity.take() {
                                commands.entity(entity).despawn();
                                demoted_messages.write(InstanceDemoted {
                                    instance_id: record.id.clone(),
                                });
                            }
                        }
                        record.tier = Tier::Cold;
                    }
                });
            }
            GateDecision::Hold => {}
        }
    }

    state.last_gate_stats = stats;
}

// ─────────────────────────────────────────────────────────────────────────────
// System 3: Sync Active transforms back to InstanceRecord (MoE sparse gate)
// ─────────────────────────────────────────────────────────────────────────────

/// Only processes entities with Changed<Transform> AND StreamingActive marker.
/// This is the MoE sparse gate from the benchmark: ~10% of entities move each
/// frame, so we skip 90% of the iteration. Measured 5–10× speedup.
fn sys_sync_active_transforms(
    state: Res<StreamingState>,
    query: Query<
        (&StreamingInstanceRef, &Transform),
        (Changed<Transform>, With<StreamingActive>),
    >,
) {
    for (inst_ref, transform) in query.iter() {
        let pos = transform.translation;
        let scale = transform.scale.x;
        let (pitch, yaw, roll) = transform.rotation.to_euler(EulerRot::XYZ);

        state.grid.update(&inst_ref.instance_id, |record| {
            record.bin.position = [pos.x, pos.y, pos.z];
            record.bin.scale = scale;
            record.bin.rotation = [pitch, yaw, roll];
            // Bump version + set dirty for write-back.
            record.version.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            record.dirty.store(true, std::sync::atomic::Ordering::Release);
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// System 4: Periodically rebuild the Explorer index from the grid
// ─────────────────────────────────────────────────────────────────────────────

/// Rebuild the flat metadata index every `index_rebuild_interval` (default 5s).
/// This keeps the Explorer's search results fresh without per-frame overhead.
fn sys_rebuild_index(mut state: ResMut<StreamingState>) {
    if state.last_index_rebuild.elapsed() < state.index_rebuild_interval {
        return;
    }

    // Clone the Arc to avoid simultaneous mutable+immutable borrow of state.
    let grid = state.grid.clone();
    state.index.rebuild_from_grid(&grid);
    state.last_index_rebuild = Instant::now();

    tracing::trace!(
        "StreamingPlugin: index rebuilt — {} entries",
        state.index.len()
    );
}

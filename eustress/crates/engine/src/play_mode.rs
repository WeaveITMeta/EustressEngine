//! # Play Mode
//!
//! Enables testing games directly in the Studio editor.
//! Spawns a controllable character and enables physics/networking.
//!
//! ## Table of Contents
//!
//! 1. **PlayModeState** - Editor/Playing/Paused states
//! 2. **WorldSnapshot** - Full state capture for reset
//! 3. **SnapshotStack** - Multiple save points with branching
//! 4. **EntitySnapshot** - Per-entity transform + component data
//! 5. **Character Controller** - WASD movement for test player
//!
//! ## Modes
//!
//! - **Play Solo**: Local testing without networking
//! - **Play with Character**: Spawns player character with controls
//! - **Play Server**: Starts local server + connects as client
//!
//! ## Controls
//!
//! - F5: Play (with character)
//! - F6: Pause
//! - F7: Play Solo (no character)
//! - F8: Stop
//! - Ctrl+Shift+S: Create save point
//! - Ctrl+Shift+R: Restore to last save point

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
#[allow(unused_imports)]
use avian3d::prelude::*;
use eustress_common::classes::{Humanoid, Instance, BasePart, Part, Model, SpawnLocation, ClassName};
use eustress_common::services::{PlayerService, Workspace, get_spawn_position_or_default};
use eustress_common::services::player::{Player, Character, CharacterRoot, CharacterHead};
use eustress_common::services::animation::{AnimationStateMachine, LocomotionController, ProceduralAnimation};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::Instant;

// Import runtime module
use crate::play_mode_runtime::{
    PlayModeRuntime, PlayModePhysicsBody, PlayModeCharacterConfig,
    spawn_play_mode_character, spawn_play_mode_camera, cleanup_play_mode_entities,
};

// Import shared character types from common
pub use eustress_common::plugins::character_plugin::{
    CharacterFacing, MovementIntent, PlayModeCharacter, PlayModeCamera,
};

// ============================================================================
// Play Mode State
// ============================================================================

/// Current play mode state
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PlayModeState {
    /// Editor mode - normal editing
    #[default]
    Editing,
    /// Play mode - game is running
    Playing,
    /// Paused - game paused
    Paused,
}

/// Play mode type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayModeType {
    /// Solo play without networking
    #[default]
    Solo,
    /// Play with spawned character
    WithCharacter,
    /// Play as server (host) - starts embedded server + connects as client
    Server,
    /// Play as client (connect to external server)
    Client,
}

/// Play mode resource
#[derive(Resource, Debug)]
pub struct PlayMode {
    /// Current play type
    pub play_type: PlayModeType,
    /// Player character entity (if spawned)
    pub player_character: Option<Entity>,
    /// Camera entity before play mode (to restore)
    pub editor_camera: Option<Entity>,
    /// Snapshot of world state before play (for reset)
    pub world_snapshot: Option<WorldSnapshot>,
    /// Time when play started
    pub started_at: Option<std::time::Instant>,
    /// Full serialized scene BINARY for complete restoration (including deleted entities)
    pub serialized_scene: Option<Vec<u8>>,
}

impl Default for PlayMode {
    fn default() -> Self {
        Self {
            play_type: PlayModeType::Solo,
            player_character: None,
            editor_camera: None,
            world_snapshot: None,
            started_at: None,
            serialized_scene: None,
        }
    }
}

// ============================================================================
// Embedded Server (Play Server Mode)
// ============================================================================

/// Resource to track embedded server process for Play Server mode
#[derive(Resource)]
pub struct EmbeddedServer {
    /// Child process handle (None if not running)
    pub process: Option<std::process::Child>,
    /// Port the server is running on
    pub port: u16,
    /// Scene file being served (temp file)
    pub scene_path: Option<std::path::PathBuf>,
    /// Server state
    pub state: EmbeddedServerState,
    /// Time server was started
    pub started_at: Option<std::time::Instant>,
}

/// State of the embedded server
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmbeddedServerState {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

impl Default for EmbeddedServer {
    fn default() -> Self {
        Self {
            process: None,
            port: 7778, // Use different port than production (7777)
            scene_path: None,
            state: EmbeddedServerState::Stopped,
            started_at: None,
        }
    }
}

impl EmbeddedServer {
    /// Check if server is running
    pub fn is_running(&self) -> bool {
        self.state == EmbeddedServerState::Running && self.process.is_some()
    }
    
    /// Get uptime in seconds
    pub fn uptime_secs(&self) -> Option<u64> {
        self.started_at.map(|t| t.elapsed().as_secs())
    }
}

/// Message to request starting the embedded server
#[derive(Message, Debug, Clone)]
pub struct StartEmbeddedServerEvent {
    /// Port to use (default: 7778)
    pub port: Option<u16>,
}

/// Message to request stopping the embedded server
#[derive(Message, Debug, Clone)]
pub struct StopEmbeddedServerEvent;

/// Message fired when embedded server state changes
#[derive(Message, Debug, Clone)]
pub struct EmbeddedServerStateChanged {
    pub old_state: EmbeddedServerState,
    pub new_state: EmbeddedServerState,
    pub error: Option<String>,
}

// ============================================================================
// World Snapshot System
// ============================================================================

/// Configuration for what to capture in snapshots
#[derive(Resource, Debug, Clone)]
pub struct SnapshotConfig {
    /// Capture transforms (position, rotation, scale)
    pub capture_transforms: bool,
    /// Capture Instance properties (name, parent, archivable)
    pub capture_instance: bool,
    /// Capture BasePart properties (color, material, anchored)
    pub capture_basepart: bool,
    /// Capture Humanoid properties (health, walkspeed)
    pub capture_humanoid: bool,
    /// Track spawned/deleted entities during play
    pub track_entity_deltas: bool,
    /// Maximum snapshot size before spilling to disk (bytes)
    pub max_memory_bytes: usize,
    /// Maximum number of save points in stack
    pub max_save_points: usize,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            capture_transforms: true,
            capture_instance: true,
            capture_basepart: true,
            capture_humanoid: true,
            track_entity_deltas: true,
            max_memory_bytes: 10 * 1024 * 1024, // 10MB
            max_save_points: 10,
        }
    }
}

/// Complete snapshot of world state for reset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldSnapshot {
    /// Unique snapshot ID
    pub id: u64,
    /// Human-readable name
    pub name: String,
    /// When snapshot was created
    #[serde(skip)]
    pub created_at: Option<Instant>,
    /// Per-entity snapshots
    pub entities: HashMap<u64, EntitySnapshot>,
    /// Entities that existed at snapshot time (for tracking deletes)
    pub original_entities: Vec<u64>,
    /// Entities spawned after snapshot (to delete on restore)
    #[serde(default)]
    pub spawned_during_play: Vec<u64>,
    /// Entities deleted after snapshot (to respawn on restore)
    #[serde(default)]
    pub deleted_during_play: Vec<u64>,
    /// Approximate memory usage in bytes
    #[serde(skip)]
    pub memory_bytes: usize,
    /// Whether this snapshot is stored on disk
    #[serde(skip)]
    pub on_disk: bool,
    /// Disk path if stored externally
    #[serde(skip)]
    pub disk_path: Option<std::path::PathBuf>,
}

impl Default for WorldSnapshot {
    fn default() -> Self {
        Self {
            id: 0,
            name: "Snapshot".to_string(),
            created_at: Some(Instant::now()),
            entities: HashMap::new(),
            original_entities: Vec::new(),
            spawned_during_play: Vec::new(),
            deleted_during_play: Vec::new(),
            memory_bytes: 0,
            on_disk: false,
            disk_path: None,
        }
    }
}

impl WorldSnapshot {
    /// Create a new snapshot with given ID
    pub fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            created_at: Some(Instant::now()),
            ..Default::default()
        }
    }
    
    /// Estimate memory usage
    pub fn estimate_memory(&mut self) {
        // Rough estimate: 200 bytes per entity snapshot
        self.memory_bytes = self.entities.len() * 200 
            + self.original_entities.len() * 8
            + self.spawned_during_play.len() * 8
            + self.deleted_during_play.len() * 8;
    }
    
    /// Check if snapshot exceeds memory threshold
    pub fn exceeds_memory(&self, threshold: usize) -> bool {
        self.memory_bytes > threshold
    }
    
    /// Save snapshot to disk
    pub fn save_to_disk(&mut self, path: &std::path::Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Serialize error: {}", e))?;
        
        // Compress with snap if available
        #[cfg(feature = "compression")]
        let data = snap::raw::Encoder::new().compress_vec(json.as_bytes())
            .map_err(|e| format!("Compress error: {}", e))?;
        
        #[cfg(not(feature = "compression"))]
        let data = json.as_bytes().to_vec();
        
        std::fs::write(path, &data)
            .map_err(|e| format!("Write error: {}", e))?;
        
        self.on_disk = true;
        self.disk_path = Some(path.to_path_buf());
        
        // Clear in-memory data to save RAM
        self.entities.clear();
        
        Ok(())
    }
    
    /// Load snapshot from disk
    pub fn load_from_disk(path: &std::path::Path) -> Result<Self, String> {
        let data = std::fs::read(path)
            .map_err(|e| format!("Read error: {}", e))?;
        
        #[cfg(feature = "compression")]
        let json = snap::raw::Decoder::new().decompress_vec(&data)
            .map_err(|e| format!("Decompress error: {}", e))?;
        
        #[cfg(not(feature = "compression"))]
        let json = data;
        
        let snapshot: WorldSnapshot = serde_json::from_slice(&json)
            .map_err(|e| format!("Deserialize error: {}", e))?;
        
        Ok(snapshot)
    }
}

/// Per-entity snapshot data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySnapshot {
    /// Bevy Entity index (for mapping)
    pub entity_index: u64,
    /// Transform data
    pub transform: Option<TransformSnapshot>,
    /// Instance component data
    pub instance: Option<InstanceSnapshot>,
    /// BasePart component data
    pub basepart: Option<BasePartSnapshot>,
    /// Humanoid component data
    pub humanoid: Option<HumanoidSnapshot>,
    /// Part-specific data
    pub part: Option<PartSnapshot>,
    /// Model-specific data
    pub model: Option<ModelSnapshot>,
}

impl EntitySnapshot {
    pub fn new(entity_index: u64) -> Self {
        Self {
            entity_index,
            transform: None,
            instance: None,
            basepart: None,
            humanoid: None,
            part: None,
            model: None,
        }
    }
}

/// Transform snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformSnapshot {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl From<&Transform> for TransformSnapshot {
    fn from(t: &Transform) -> Self {
        Self {
            translation: [t.translation.x, t.translation.y, t.translation.z],
            rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
            scale: [t.scale.x, t.scale.y, t.scale.z],
        }
    }
}

impl TransformSnapshot {
    pub fn apply_to(&self, transform: &mut Transform) {
        transform.translation = Vec3::new(
            self.translation[0],
            self.translation[1],
            self.translation[2],
        );
        transform.rotation = Quat::from_xyzw(
            self.rotation[0],
            self.rotation[1],
            self.rotation[2],
            self.rotation[3],
        );
        transform.scale = Vec3::new(
            self.scale[0],
            self.scale[1],
            self.scale[2],
        );
    }
}

/// Instance component snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceSnapshot {
    pub name: String,
    pub archivable: bool,
}

/// BasePart component snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasePartSnapshot {
    pub anchored: bool,
    pub can_collide: bool,
    pub transparency: f32,
    pub color: [f32; 4],
}

/// Humanoid component snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanoidSnapshot {
    pub health: f32,
    pub max_health: f32,
    pub walk_speed: f32,
    pub jump_power: f32,
}

/// Part-specific snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartSnapshot {
    pub shape: String,
}

/// Model-specific snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSnapshot {
    pub primary_part_index: Option<u64>,
}

// ============================================================================
// Snapshot Stack (Multiple Save Points)
// ============================================================================

/// Stack of snapshots for multiple save points
#[derive(Resource, Debug, Default)]
pub struct SnapshotStack {
    /// Stack of snapshots (index 0 = oldest/initial)
    pub snapshots: Vec<WorldSnapshot>,
    /// Current position in stack (for undo/redo)
    pub current_index: usize,
    /// Next snapshot ID
    next_id: u64,
    /// Total memory used by all snapshots
    pub total_memory_bytes: usize,
}

impl SnapshotStack {
    /// Push a new snapshot onto the stack
    pub fn push(&mut self, mut snapshot: WorldSnapshot, config: &SnapshotConfig) {
        // Assign ID
        snapshot.id = self.next_id;
        self.next_id += 1;
        
        // Estimate memory
        snapshot.estimate_memory();
        
        // Check if we need to spill to disk
        if self.total_memory_bytes + snapshot.memory_bytes > config.max_memory_bytes {
            // Spill oldest snapshots to disk
            for old_snapshot in self.snapshots.iter_mut() {
                if !old_snapshot.on_disk {
                    let path = std::env::temp_dir()
                        .join(format!("eustress_snapshot_{}.json", old_snapshot.id));
                    if let Err(e) = old_snapshot.save_to_disk(&path) {
                        warn!("Failed to spill snapshot to disk: {}", e);
                    } else {
                        self.total_memory_bytes -= old_snapshot.memory_bytes;
                        old_snapshot.memory_bytes = 0;
                    }
                    break; // Spill one at a time
                }
            }
        }
        
        // Trim stack if exceeds max
        while self.snapshots.len() >= config.max_save_points {
            let removed = self.snapshots.remove(0);
            self.total_memory_bytes -= removed.memory_bytes;
            // Clean up disk file if exists
            if let Some(path) = removed.disk_path {
                let _ = std::fs::remove_file(path);
            }
            if self.current_index > 0 {
                self.current_index -= 1;
            }
        }
        
        // Truncate any "future" snapshots if we're not at the end
        if self.current_index < self.snapshots.len() {
            for removed in self.snapshots.drain(self.current_index..) {
                self.total_memory_bytes -= removed.memory_bytes;
                if let Some(path) = removed.disk_path {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        
        self.total_memory_bytes += snapshot.memory_bytes;
        self.snapshots.push(snapshot);
        self.current_index = self.snapshots.len();
    }
    
    /// Get the current snapshot (for restore)
    pub fn current(&self) -> Option<&WorldSnapshot> {
        if self.current_index > 0 && self.current_index <= self.snapshots.len() {
            self.snapshots.get(self.current_index - 1)
        } else {
            self.snapshots.last()
        }
    }
    
    /// Get the initial snapshot (play start)
    pub fn initial(&self) -> Option<&WorldSnapshot> {
        self.snapshots.first()
    }
    
    /// Pop and return the most recent snapshot
    pub fn pop(&mut self) -> Option<WorldSnapshot> {
        if self.snapshots.len() > 1 {
            let snapshot = self.snapshots.pop();
            if let Some(ref s) = snapshot {
                self.total_memory_bytes -= s.memory_bytes;
            }
            self.current_index = self.snapshots.len();
            snapshot
        } else {
            None // Keep at least the initial snapshot
        }
    }
    
    /// Clear all snapshots
    pub fn clear(&mut self) {
        for snapshot in self.snapshots.drain(..) {
            if let Some(path) = snapshot.disk_path {
                let _ = std::fs::remove_file(path);
            }
        }
        self.current_index = 0;
        self.total_memory_bytes = 0;
    }
    
    /// Get number of snapshots
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }
}

// ============================================================================
// Events
// ============================================================================

/// Event to start play mode
#[derive(Event, Message, Debug, Clone)]
pub struct StartPlayEvent {
    pub play_type: PlayModeType,
}

/// Event to stop play mode
#[derive(Event, Message, Debug, Clone)]
pub struct StopPlayEvent;

/// Event to pause/resume play mode
#[derive(Event, Message, Debug, Clone)]
pub struct TogglePauseEvent;

// ============================================================================
// Character Controller - Uses SHARED types from eustress_common::plugins::character_plugin
// PlayModeCharacter, PlayModeCamera, CharacterFacing, MovementIntent are imported above
// ============================================================================

/// Simple character input state (local to engine for legacy compatibility)
#[derive(Component, Debug, Default)]
pub struct CharacterInput {
    pub movement: Vec3,
    pub sprinting: bool,
    pub jump: bool,
    pub look_delta: Vec2,
}

// ============================================================================
// Systems
// ============================================================================

/// Handle start play event - captures full world snapshot and spawns client-like character
fn handle_start_play(
    mut commands: Commands,
    mut events: MessageReader<StartPlayEvent>,
    mut play_mode: ResMut<PlayMode>,
    mut runtime: ResMut<PlayModeRuntime>,
    mut snapshot_stack: ResMut<SnapshotStack>,
    snapshot_config: Res<SnapshotConfig>,
    char_config: Res<PlayModeCharacterConfig>,
    mut next_state: ResMut<NextState<PlayModeState>>,
    player_service: Res<PlayerService>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cursor_options: Query<&mut CursorOptions, With<Window>>,
    cameras: Query<(Entity, &Transform), With<Camera3d>>,
    spawn_locations: Query<(&Transform, &SpawnLocation)>,
    snapshot_query: Query<(
        Entity,
        Option<&Transform>,
        Option<&Instance>,
        Option<&BasePart>,
        Option<&Humanoid>,
        Option<&Part>,
        Option<&Model>,
    ), Without<PlayModeCharacter>>,
) {
    for event in events.read() {
        info!("🎮 Starting play mode: {:?}", event.play_type);
        
        // Store editor camera transform for restoration
        if let Some((cam_entity, cam_transform)) = cameras.iter().next() {
            play_mode.editor_camera = Some(cam_entity);
            runtime.editor_camera_transform = Some(*cam_transform);
        }
        
        // Create comprehensive world snapshot
        let mut snapshot = WorldSnapshot::new(0, "Play Start");
        
        for (entity, transform, instance, basepart, humanoid, part, model) in snapshot_query.iter() {
            let entity_index = entity.index() as u64;
            let mut entity_snapshot = EntitySnapshot::new(entity_index);
            
            // Capture transform
            if snapshot_config.capture_transforms {
                if let Some(t) = transform {
                    entity_snapshot.transform = Some(TransformSnapshot::from(t));
                }
            }
            
            // Capture Instance
            if snapshot_config.capture_instance {
                if let Some(inst) = instance {
                    entity_snapshot.instance = Some(InstanceSnapshot {
                        name: inst.name.clone(),
                        archivable: inst.archivable,
                    });
                }
            }
            
            // Capture BasePart
            if snapshot_config.capture_basepart {
                if let Some(bp) = basepart {
                    let rgba = bp.color.to_linear().to_f32_array();
                    entity_snapshot.basepart = Some(BasePartSnapshot {
                        anchored: bp.anchored,
                        can_collide: bp.can_collide,
                        transparency: bp.transparency,
                        color: rgba,
                    });
                }
            }
            
            // Capture Humanoid
            if snapshot_config.capture_humanoid {
                if let Some(h) = humanoid {
                    entity_snapshot.humanoid = Some(HumanoidSnapshot {
                        health: h.health,
                        max_health: h.max_health,
                        walk_speed: h.walk_speed,
                        jump_power: h.jump_power,
                    });
                }
            }
            
            // Capture Part
            if let Some(p) = part {
                entity_snapshot.part = Some(PartSnapshot {
                    shape: format!("{:?}", p.shape),
                });
            }
            
            // Capture Model
            if let Some(m) = model {
                entity_snapshot.model = Some(ModelSnapshot {
                    primary_part_index: m.primary_part.map(|e| e as u64),
                });
            }
            
            snapshot.original_entities.push(entity_index);
            snapshot.entities.insert(entity_index, entity_snapshot);
        }
        
        snapshot.estimate_memory();
        info!("📸 Captured {} entities ({} bytes)", snapshot.entities.len(), snapshot.memory_bytes);
        
        // Clear any existing snapshots and push initial
        snapshot_stack.clear();
        snapshot_stack.push(snapshot.clone(), &snapshot_config);
        
        play_mode.world_snapshot = Some(snapshot);
        play_mode.play_type = event.play_type;
        play_mode.started_at = Some(Instant::now());
        
        // Serialize scene to BINARY for complete restoration (handles deleted entities)
        // We'll do this via a deferred command since we need mutable World access
        commands.queue(|world: &mut World| {
            // Serialize to temp binary file
            let temp_path = std::env::temp_dir().join("eustress_play_mode_snapshot.eustressengine");
            match crate::serialization::save_binary_scene(world, &temp_path) {
                Ok(()) => {
                    // Read the binary back into memory
                    if let Ok(binary_data) = std::fs::read(&temp_path) {
                        if let Some(mut play_mode) = world.get_resource_mut::<PlayMode>() {
                            play_mode.serialized_scene = Some(binary_data);
                            info!("📦 Serialized scene to binary for play mode restoration ({} bytes)", play_mode.serialized_scene.as_ref().map(|d| d.len()).unwrap_or(0));
                        }
                    }
                    // Clean up temp file
                    let _ = std::fs::remove_file(&temp_path);
                }
                Err(e) => {
                    warn!("⚠️ Failed to serialize scene for play mode: {}", e);
                }
            }
        });
        
        // Handle different play modes
        match event.play_type {
            PlayModeType::Solo => {
                // RUN MODE: Free camera (keep editor camera), enable physics/scripts, NO character
                // Editor camera stays active - user can fly around and observe
                runtime.physics_enabled = true;
                info!("🏃 Run mode started - free camera, physics enabled, no character");
            }
            PlayModeType::WithCharacter => {
                // PLAY MODE: Spawn character, camera follows character
                // In networked mode, server spawns character, client spawns camera after receiving it
                // For local testing, we do both here
                
                // Find spawn position from SpawnLocation entities, or use default
                let (spawn_pos, spawn_protection) = get_spawn_position_or_default(
                    spawn_locations.iter(),
                    None, // TODO: Get player team when team system is implemented
                    player_service.spawn_position,
                );
                
                if spawn_protection > 0.0 {
                    info!("🛡️ Spawn protection: {:.1}s", spawn_protection);
                }
                
                // Spawn full character with physics (like server would)
                let character = spawn_play_mode_character(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &asset_server,
                    spawn_pos,
                    &mut runtime,
                    &char_config,
                );
                
                play_mode.player_character = Some(character);
                
                // Spawn play mode camera (like client would after receiving character)
                spawn_play_mode_camera(&mut commands, character, &mut runtime);
                
                // Disable ALL existing cameras during play
                for (cam_entity, _) in cameras.iter() {
                    commands.entity(cam_entity).insert(Camera {
                        is_active: false,
                        ..default()
                    });
                }
                
                // DON'T lock cursor - Play mode uses third-person camera like client
                // Cursor is free, right-click to orbit camera, scroll to zoom
                // Only first-person mode (zoomed all the way in) locks cursor
                if let Ok(mut cursor) = cursor_options.single_mut() {
                    runtime.editor_cursor_mode = Some(cursor.grab_mode);
                    // Keep cursor visible and free for third-person camera
                    cursor.grab_mode = CursorGrabMode::None;
                    cursor.visible = true;
                    runtime.cursor_locked = false;
                }
                
                runtime.physics_enabled = true;
                
                info!("👤 Play mode started - character at {:?} (third-person, right-click to orbit)", spawn_pos);
            }
            PlayModeType::Server => {
                // SERVER MODE: Start in-process server + connect as client
                // Character/camera spawning handled by networking layer
                runtime.physics_enabled = true;
                info!("🖥️ Server mode started - networking will handle character/camera");
            }
            PlayModeType::Client => {
                // CLIENT MODE: Connect to external server
                // Server spawns character, client spawns camera after receiving it
                runtime.physics_enabled = true;
                info!("🔌 Client mode started - waiting for server to spawn character");
            }
        }
        
        next_state.set(PlayModeState::Playing);
    }
}

/// Handle stop play event - restores world from snapshot and cleans up play mode entities
fn handle_stop_play(
    mut commands: Commands,
    mut events: MessageReader<StopPlayEvent>,
    mut play_mode: ResMut<PlayMode>,
    mut runtime: ResMut<PlayModeRuntime>,
    mut snapshot_stack: ResMut<SnapshotStack>,
    mut next_state: ResMut<NextState<PlayModeState>>,
    mut cursor_options: Query<&mut CursorOptions, With<Window>>,
    mut physics_time: ResMut<Time<Physics>>,
    // Query ALL entities with Instance (not just Parts) for proper restoration
    mut restore_query: Query<(
        Entity,
        Option<&mut Transform>,
        Option<&mut Instance>,
        Option<&mut BasePart>,
        Option<&mut Humanoid>,
    ), (With<Instance>, Without<PlayModeCharacter>, Without<SpawnedDuringPlayMode>)>,
    spawned_during_play: Query<Entity, With<SpawnedDuringPlayMode>>,
    // Track all existing entities to find deleted ones
    all_entities: Query<Entity, With<Instance>>,
    // Query for play mode entities to ensure they're despawned
    play_mode_entities: Query<Entity, Or<(With<PlayModeCharacter>, With<PlayModeCamera>)>>,
) {
    for _ in events.read() {
        info!("🛑 Stopping play mode");
        
        // Pause physics during editor mode
        physics_time.pause();
        
        // Despawn all entities that were spawned during play mode
        let mut despawned_count = 0;
        for entity in spawned_during_play.iter() {
            commands.entity(entity).despawn();
            despawned_count += 1;
        }
        if despawned_count > 0 {
            info!("🗑️ Despawned {} entities created during play", despawned_count);
        }
        
        // Explicitly despawn ALL play mode entities (character, camera, etc.)
        // This catches any that weren't tracked in runtime.spawned_entities
        let mut play_mode_despawned = 0;
        for entity in play_mode_entities.iter() {
            commands.entity(entity).despawn();
            play_mode_despawned += 1;
        }
        if play_mode_despawned > 0 {
            info!("🎮 Despawned {} play mode entities (character/camera)", play_mode_despawned);
        }
        
        // Clean up runtime tracking
        cleanup_play_mode_entities(&mut commands, &mut runtime);
        play_mode.player_character = None;
        
        // First, despawn ALL play mode cameras to ensure they don't interfere
        // This is done via the runtime cleanup above, but we also need to ensure
        // the editor camera is properly restored
        
        // Restore editor camera - just set is_active to true without resetting other properties
        if let Some(editor_cam) = play_mode.editor_camera {
            // Use a command to modify the existing Camera component instead of replacing it
            commands.queue(move |world: &mut World| {
                if let Some(mut camera) = world.get_mut::<Camera>(editor_cam) {
                    camera.is_active = true;
                    info!("📷 Editor camera re-enabled");
                } else {
                    // Camera component was removed, re-add it
                    world.entity_mut(editor_cam).insert(Camera {
                        is_active: true,
                        ..default()
                    });
                    info!("📷 Editor camera component restored");
                }
            });
            
            // Restore editor camera transform if saved
            if let Some(saved_transform) = runtime.editor_camera_transform.take() {
                commands.entity(editor_cam).insert(saved_transform);
            }
        }
        
        // Restore cursor state
        if let Ok(mut cursor) = cursor_options.single_mut() {
            cursor.grab_mode = runtime.editor_cursor_mode.unwrap_or(CursorGrabMode::None);
            cursor.visible = true;
        }
        
        // Restore world from initial snapshot
        if let Some(snapshot) = snapshot_stack.initial() {
            info!("🔄 Restoring {} entities from snapshot", snapshot.entities.len());
            
            // Despawn entities that were spawned during play
            for &entity_index in &snapshot.spawned_during_play {
                if let Some(entity) = Entity::try_from_bits(entity_index as u64) {
                    if commands.get_entity(entity).is_ok() {
                        commands.entity(entity).despawn();
                    }
                }
            }
            
            // Restore entity states
            let mut restored_count = 0;
            let mut transform_restored = 0;
            for (entity, transform, instance, basepart, humanoid) in restore_query.iter_mut() {
                let entity_index = entity.index() as u64;
                
                if let Some(entity_snapshot) = snapshot.entities.get(&entity_index) {
                    restored_count += 1;
                    
                    // Restore transform
                    if let (Some(mut t), Some(ts)) = (transform, &entity_snapshot.transform) {
                        ts.apply_to(&mut t);
                        transform_restored += 1;
                    }
                    
                    // Restore Instance
                    if let (Some(mut inst), Some(is)) = (instance, &entity_snapshot.instance) {
                        inst.name = is.name.clone();
                        inst.archivable = is.archivable;
                    }
                    
                    // Restore BasePart
                    if let (Some(mut bp), Some(bps)) = (basepart, &entity_snapshot.basepart) {
                        bp.anchored = bps.anchored;
                        bp.can_collide = bps.can_collide;
                        bp.transparency = bps.transparency;
                        bp.color = Color::linear_rgba(bps.color[0], bps.color[1], bps.color[2], bps.color[3]);
                    }
                    
                    // Restore Humanoid
                    if let (Some(mut h), Some(hs)) = (humanoid, &entity_snapshot.humanoid) {
                        h.health = hs.health;
                        h.max_health = hs.max_health;
                        h.walk_speed = hs.walk_speed;
                        h.jump_power = hs.jump_power;
                    }
                }
            }
            
            // Check for entities that were deleted during play and need to be respawned
            // Build set of current entity indices
            let current_entity_indices: std::collections::HashSet<u64> = all_entities
                .iter()
                .map(|e| e.index() as u64)
                .collect();
            
            // Find entities in snapshot that no longer exist
            let mut deleted_count = 0;
            for &original_index in &snapshot.original_entities {
                if !current_entity_indices.contains(&original_index) {
                    deleted_count += 1;
                }
            }
            
            info!("✅ World restored: {} entities matched, {} transforms restored", restored_count, transform_restored);
            
            // If entities were deleted, restore from full serialized binary scene
            if deleted_count > 0 {
                info!("🔄 {} entities were deleted during play, restoring from serialized binary...", deleted_count);
                
                // Take the serialized scene to use in the deferred command
                let serialized_scene = play_mode.serialized_scene.take();
                
                // Queue a deferred command to reload the scene
                commands.queue(move |world: &mut World| {
                    if let Some(binary_data) = serialized_scene {
                        // Instead of despawning ALL Instance entities (which includes UI),
                        // only despawn entities that were originally in the scene
                        
                        // Get the list of entities that should exist after restoration
                        let temp_path = std::env::temp_dir().join("eustress_play_mode_restore.eustressengine");
                        if std::fs::write(&temp_path, &binary_data).is_ok() {
                            use std::collections::HashSet;
                            
                            // Build set of entity IDs that should exist after restoration
                            let mut should_exist: HashSet<u64> = HashSet::new();
                            
                            // Load scene to collect entity IDs via callback
                            let collect_result = crate::serialization::load_binary_scene(&temp_path, |entity_data| {
                                should_exist.insert(entity_data.id as u64);
                                Ok(())
                            });
                            
                            if collect_result.is_ok() {
                                
                                // Find entities that exist now but shouldn't after restoration
                                let mut to_despawn: Vec<Entity> = Vec::new();
                                {
                                    let mut query = world.query_filtered::<(Entity, &Instance), 
                                        (Without<PlayModeCharacter>, Without<SpawnedDuringPlayMode>)>();
                                    for (entity, _) in query.iter(world) {
                                        let entity_id = entity.index() as u64;
                                        if !should_exist.contains(&entity_id) {
                                            to_despawn.push(entity);
                                        }
                                    }
                                }
                                
                                // Only despawn entities that shouldn't exist after restoration
                                for entity in to_despawn {
                                    world.despawn(entity);
                                }
                                
                                // Now load the binary scene (this will spawn the missing entities)
                                match crate::serialization::load_binary_scene_to_world(world, &temp_path) {
                                    Ok(count) => {
                                        info!("✅ Restored {} entities from binary scene", count);
                                    }
                                    Err(e) => {
                                        warn!("⚠️ Failed to restore scene: {}", e);
                                    }
                                }
                                
                                let _ = std::fs::remove_file(&temp_path);
                            } else {
                                warn!("⚠️ Failed to load scene for entity comparison");
                            }
                        }
                    }
                });
            }
        } else {
            warn!("⚠️ No snapshot found to restore from!");
        }
        
        // Clear snapshot stack and runtime state
        snapshot_stack.clear();
        play_mode.world_snapshot = None;
        play_mode.started_at = None;
        play_mode.serialized_scene = None;
        runtime.clear();
        
        next_state.set(PlayModeState::Editing);
    }
}

/// Event to create a save point during play
#[derive(Event, Message, Debug, Clone)]
pub struct CreateSavePointEvent {
    pub name: String,
}

/// Event to restore to a save point
#[derive(Event, Message, Debug, Clone)]
pub struct RestoreToSavePointEvent {
    /// Index in stack (None = most recent)
    pub index: Option<usize>,
}

/// Handle save point creation
fn handle_create_save_point(
    mut events: MessageReader<CreateSavePointEvent>,
    mut snapshot_stack: ResMut<SnapshotStack>,
    snapshot_config: Res<SnapshotConfig>,
    current_state: Res<State<PlayModeState>>,
    snapshot_query: Query<(
        Entity,
        Option<&Transform>,
        Option<&Instance>,
        Option<&BasePart>,
        Option<&Humanoid>,
    ), Without<PlayModeCharacter>>,
) {
    // Only allow during play
    if *current_state.get() == PlayModeState::Editing {
        return;
    }
    
    for event in events.read() {
        let mut snapshot = WorldSnapshot::new(0, &event.name);
        
        for (entity, transform, instance, basepart, humanoid) in snapshot_query.iter() {
            let entity_index = entity.index() as u64;
            let mut entity_snapshot = EntitySnapshot::new(entity_index);
            
            if let Some(t) = transform {
                entity_snapshot.transform = Some(TransformSnapshot::from(t));
            }
            
            if let Some(inst) = instance {
                entity_snapshot.instance = Some(InstanceSnapshot {
                    name: inst.name.clone(),
                    archivable: inst.archivable,
                });
            }
            
            if let Some(bp) = basepart {
                let rgba = bp.color.to_linear().to_f32_array();
                entity_snapshot.basepart = Some(BasePartSnapshot {
                    anchored: bp.anchored,
                    can_collide: bp.can_collide,
                    transparency: bp.transparency,
                    color: rgba,
                });
            }
            
            if let Some(h) = humanoid {
                entity_snapshot.humanoid = Some(HumanoidSnapshot {
                    health: h.health,
                    max_health: h.max_health,
                    walk_speed: h.walk_speed,
                    jump_power: h.jump_power,
                });
            }
            
            snapshot.entities.insert(entity_index, entity_snapshot);
        }
        
        snapshot_stack.push(snapshot, &snapshot_config);
        info!("💾 Created save point '{}' ({} total)", event.name, snapshot_stack.len());
    }
}

/// Handle restore to save point
fn handle_restore_save_point(
    mut events: MessageReader<RestoreToSavePointEvent>,
    snapshot_stack: Res<SnapshotStack>,
    current_state: Res<State<PlayModeState>>,
    mut restore_query: Query<(
        Entity,
        Option<&mut Transform>,
        Option<&mut Instance>,
        Option<&mut BasePart>,
        Option<&mut Humanoid>,
    ), Without<PlayModeCharacter>>,
) {
    if *current_state.get() == PlayModeState::Editing {
        return;
    }
    
    for event in events.read() {
        let snapshot = if let Some(idx) = event.index {
            snapshot_stack.snapshots.get(idx)
        } else {
            snapshot_stack.current()
        };
        
        let Some(snapshot) = snapshot else {
            warn!("No snapshot found to restore");
            continue;
        };
        
        info!("🔄 Restoring to save point '{}'", snapshot.name);
        
        for (entity, transform, instance, basepart, humanoid) in restore_query.iter_mut() {
            let entity_index = entity.index() as u64;
            
            if let Some(entity_snapshot) = snapshot.entities.get(&entity_index) {
                if let (Some(mut t), Some(ts)) = (transform, &entity_snapshot.transform) {
                    ts.apply_to(&mut t);
                }
                
                if let (Some(mut inst), Some(is)) = (instance, &entity_snapshot.instance) {
                    inst.name = is.name.clone();
                    inst.archivable = is.archivable;
                }
                
                if let (Some(mut bp), Some(bps)) = (basepart, &entity_snapshot.basepart) {
                    bp.anchored = bps.anchored;
                    bp.can_collide = bps.can_collide;
                    bp.transparency = bps.transparency;
                    bp.color = Color::linear_rgba(bps.color[0], bps.color[1], bps.color[2], bps.color[3]);
                }
                
                if let (Some(mut h), Some(hs)) = (humanoid, &entity_snapshot.humanoid) {
                    h.health = hs.health;
                    h.max_health = hs.max_health;
                    h.walk_speed = hs.walk_speed;
                    h.jump_power = hs.jump_power;
                }
            }
        }
        
        info!("✅ Restored to save point");
    }
}

/// Handle pause toggle
fn handle_pause_toggle(
    mut events: MessageReader<TogglePauseEvent>,
    current_state: Res<State<PlayModeState>>,
    mut next_state: ResMut<NextState<PlayModeState>>,
    mut physics_time: ResMut<Time<Physics>>,
) {
    for _ in events.read() {
        match current_state.get() {
            PlayModeState::Playing => {
                info!("⏸️ Pausing play mode - physics frozen");
                physics_time.pause();
                next_state.set(PlayModeState::Paused);
            }
            PlayModeState::Paused => {
                info!("▶️ Resuming play mode - physics resumed");
                physics_time.unpause();
                next_state.set(PlayModeState::Playing);
            }
            _ => {}
        }
    }
}

/// Character movement system (runs in Playing state)
fn character_movement(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut Transform, &mut CharacterInput, &Humanoid), With<PlayModeCharacter>>,
    time: Res<Time>,
) {
    for (mut transform, mut input, humanoid) in query.iter_mut() {
        // Gather input
        let mut movement = Vec3::ZERO;
        
        if keyboard.pressed(KeyCode::KeyW) {
            movement.z -= 1.0;
        }
        if keyboard.pressed(KeyCode::KeyS) {
            movement.z += 1.0;
        }
        if keyboard.pressed(KeyCode::KeyA) {
            movement.x -= 1.0;
        }
        if keyboard.pressed(KeyCode::KeyD) {
            movement.x += 1.0;
        }
        
        input.movement = movement.normalize_or_zero();
        input.sprinting = keyboard.pressed(KeyCode::ShiftLeft);
        input.jump = keyboard.just_pressed(KeyCode::Space);
        
        // Apply movement
        let speed = humanoid.effective_speed(input.sprinting);
        let velocity = input.movement * speed * time.delta_secs();
        transform.translation += velocity;
        
        // Simple jump (no physics yet)
        if input.jump {
            // TODO: Proper physics jump
        }
    }
}

/// Camera follow system (runs in Playing state)
fn camera_follow(
    play_mode: Res<PlayMode>,
    character_query: Query<&Transform, (With<PlayModeCharacter>, Without<Camera3d>)>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
) {
    if play_mode.player_character.is_none() {
        return;
    }
    
    let Ok(char_transform) = character_query.single() else { return };
    let Ok(mut cam_transform) = camera_query.single_mut() else { return };
    
    // Third-person camera offset
    let offset = Vec3::new(0.0, 3.0, 8.0);
    let target_pos = char_transform.translation + offset;
    
    // Smooth follow
    cam_transform.translation = cam_transform.translation.lerp(target_pos, 0.1);
    cam_transform.look_at(char_transform.translation + Vec3::Y * 1.0, Vec3::Y);
}

/// Handle UI button clicks for play mode (from ribbon)
fn handle_play_mode_ui_buttons(
    mut studio_state: ResMut<crate::ui::StudioState>,
    current_state: Res<State<PlayModeState>>,
    mut start_events: MessageWriter<StartPlayEvent>,
    mut stop_events: MessageWriter<StopPlayEvent>,
    mut pause_events: MessageWriter<TogglePauseEvent>,
) {
    // Play with character button
    if studio_state.play_with_character_requested {
        studio_state.play_with_character_requested = false;
        if *current_state.get() == PlayModeState::Editing {
            info!("▶️ Play with Character requested from UI");
            start_events.write(StartPlayEvent {
                play_type: PlayModeType::WithCharacter,
            });
        }
    }
    
    // Play solo button
    if studio_state.play_solo_requested {
        studio_state.play_solo_requested = false;
        if *current_state.get() == PlayModeState::Editing {
            info!("▶️ Play Solo requested from UI");
            start_events.write(StartPlayEvent {
                play_type: PlayModeType::Solo,
            });
        }
    }
    
    // Pause button
    if studio_state.pause_requested {
        studio_state.pause_requested = false;
        match current_state.get() {
            PlayModeState::Playing | PlayModeState::Paused => {
                info!("⏸️ Pause/Resume requested from UI");
                pause_events.write(TogglePauseEvent);
            }
            _ => {}
        }
    }
    
    // Stop button
    if studio_state.stop_requested {
        studio_state.stop_requested = false;
        match current_state.get() {
            PlayModeState::Playing | PlayModeState::Paused => {
                info!("⏹️ Stop requested from UI");
                stop_events.write(StopPlayEvent);
            }
            _ => {}
        }
    }
}

/// Keyboard shortcuts for play mode
fn play_mode_shortcuts(
    keyboard: Res<ButtonInput<KeyCode>>,
    current_state: Res<State<PlayModeState>>,
    mut start_events: MessageWriter<StartPlayEvent>,
    mut stop_events: MessageWriter<StopPlayEvent>,
    mut pause_events: MessageWriter<TogglePauseEvent>,
    mut save_point_events: MessageWriter<CreateSavePointEvent>,
    mut restore_events: MessageWriter<RestoreToSavePointEvent>,
    snapshot_stack: Res<SnapshotStack>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    
    // F5: Play with character
    if keyboard.just_pressed(KeyCode::F5) {
        match current_state.get() {
            PlayModeState::Editing => {
                info!("▶️ F5: Play with Character");
                start_events.write(StartPlayEvent {
                    play_type: PlayModeType::WithCharacter,
                });
            }
            _ => {}
        }
    }
    
    // F6: Pause/Resume
    if keyboard.just_pressed(KeyCode::F6) {
        match current_state.get() {
            PlayModeState::Playing | PlayModeState::Paused => {
                pause_events.write(TogglePauseEvent);
            }
            _ => {}
        }
    }
    
    // F7: Play solo (no character)
    if keyboard.just_pressed(KeyCode::F7) {
        match current_state.get() {
            PlayModeState::Editing => {
                info!("▶️ F7: Play Solo");
                start_events.write(StartPlayEvent {
                    play_type: PlayModeType::Solo,
                });
            }
            _ => {}
        }
    }
    
    // F8: Stop
    if keyboard.just_pressed(KeyCode::F8) {
        match current_state.get() {
            PlayModeState::Playing | PlayModeState::Paused => {
                info!("⏹️ F8: Stop");
                stop_events.write(StopPlayEvent);
            }
            _ => {}
        }
    }
    
    // Escape: Stop (alternative)
    if keyboard.just_pressed(KeyCode::Escape) {
        match current_state.get() {
            PlayModeState::Playing | PlayModeState::Paused => {
                stop_events.write(StopPlayEvent);
            }
            _ => {}
        }
    }
    
    // Ctrl+Shift+S: Create save point
    if ctrl && shift && keyboard.just_pressed(KeyCode::KeyS) {
        match current_state.get() {
            PlayModeState::Playing | PlayModeState::Paused => {
                let name = format!("Save Point {}", snapshot_stack.len());
                save_point_events.write(CreateSavePointEvent { name });
            }
            _ => {}
        }
    }
    
    // Ctrl+Shift+R: Restore to last save point
    if ctrl && shift && keyboard.just_pressed(KeyCode::KeyR) {
        match current_state.get() {
            PlayModeState::Playing | PlayModeState::Paused => {
                restore_events.write(RestoreToSavePointEvent { index: None });
            }
            _ => {}
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Play mode plugin for Studio
pub struct PlayModePlugin;

impl Plugin for PlayModePlugin {
    fn build(&self, app: &mut App) {
        // Add the SHARED character plugin - same code as client!
        // This ensures identical gameplay behavior in Studio play mode
        app.add_plugins(eustress_common::plugins::SharedCharacterPlugin);
        
        // Add skinned character support (GLB models)
        app.add_plugins(eustress_common::plugins::SkinnedCharacterPlugin);
        
        // Add AAA animation system (crossfade blending, locomotion blend tree)
        app.add_plugins(eustress_common::plugins::SharedAnimationPlugin);
        
        // Add the runtime plugin for play mode specific handling
        app.add_plugins(crate::play_mode_runtime::PlayModeRuntimePlugin);
        
        app
            // State
            .init_state::<PlayModeState>()
            
            // Resources
            .init_resource::<PlayMode>()
            .init_resource::<SnapshotStack>()
            .init_resource::<SnapshotConfig>()
            .init_resource::<EmbeddedServer>()
            
            // Messages
            .add_message::<StartPlayEvent>()
            .add_message::<StopPlayEvent>()
            .add_message::<TogglePauseEvent>()
            .add_message::<CreateSavePointEvent>()
            .add_message::<RestoreToSavePointEvent>()
            .add_message::<StartEmbeddedServerEvent>()
            .add_message::<StopEmbeddedServerEvent>()
            .add_message::<EmbeddedServerStateChanged>()
            
            // Systems (always run) - split into groups to avoid tuple size limits
            .add_systems(Update, (
                handle_play_mode_ui_buttons,  // Handle ribbon button clicks
                play_mode_shortcuts,
                handle_start_play,
                handle_stop_play,
                handle_pause_toggle,
            ))
            .add_systems(Update, (
                handle_create_save_point,
                handle_restore_save_point,
                handle_embedded_server_start,
                handle_embedded_server_stop,
                monitor_embedded_server,
            ))
            
            // Systems that run when entering/exiting play mode
            .add_systems(OnEnter(PlayModeState::Playing), activate_physics_for_unanchored_parts)
            .add_systems(OnEnter(PlayModeState::Playing), start_play_server_if_server_mode)
            .add_systems(OnExit(PlayModeState::Playing), deactivate_physics_for_parts)
            .add_systems(OnExit(PlayModeState::Playing), stop_play_server_if_server_mode)
            
            // Real-time anchored state sync during play mode
            .add_systems(Update, sync_anchored_to_rigidbody.run_if(in_state(PlayModeState::Playing)));
        
        info!("🎮 PlayModePlugin initialized with SHARED character plugin (same as client)");
    }
}

// ============================================================================
// Embedded Server Systems
// ============================================================================

/// System to handle starting the embedded server
fn handle_embedded_server_start(
    mut events: MessageReader<StartEmbeddedServerEvent>,
    mut server: ResMut<EmbeddedServer>,
    mut state_events: MessageWriter<EmbeddedServerStateChanged>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
    parts_query: Query<(Entity, &Instance, Option<&BasePart>, Option<&Part>)>,
) {
    for event in events.read() {
        if server.is_running() {
            notifications.warning("Server is already running");
            continue;
        }
        
        let old_state = server.state;
        server.state = EmbeddedServerState::Starting;
        server.port = event.port.unwrap_or(7778);
        
        info!("🚀 Starting embedded server on port {}...", server.port);
        
        // Save current scene to temp file
        let temp_dir = std::env::temp_dir().join("eustress_studio");
        if let Err(e) = std::fs::create_dir_all(&temp_dir) {
            error!("Failed to create temp directory: {}", e);
            server.state = EmbeddedServerState::Error;
            state_events.write(EmbeddedServerStateChanged {
                old_state,
                new_state: server.state,
                error: Some(format!("Failed to create temp directory: {}", e)),
            });
            continue;
        }
        
        let scene_path = temp_dir.join("play_session.json");
        
        // Build scene data from current entities
        let scene = build_scene_for_server(&parts_query);
        
        // Save scene to file
        match serde_json::to_string_pretty(&scene) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&scene_path, json) {
                    error!("Failed to write scene file: {}", e);
                    server.state = EmbeddedServerState::Error;
                    state_events.write(EmbeddedServerStateChanged {
                        old_state,
                        new_state: server.state,
                        error: Some(format!("Failed to write scene: {}", e)),
                    });
                    continue;
                }
            }
            Err(e) => {
                error!("Failed to serialize scene: {}", e);
                server.state = EmbeddedServerState::Error;
                continue;
            }
        }
        
        server.scene_path = Some(scene_path.clone());
        
        // Try to spawn server process
        let server_exe = find_server_executable();
        
        match std::process::Command::new(&server_exe)
            .arg("--port").arg(server.port.to_string())
            .arg("--scene").arg(&scene_path)
            .arg("--max-players").arg("10")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => {
                server.process = Some(child);
                server.state = EmbeddedServerState::Running;
                server.started_at = Some(std::time::Instant::now());
                
                notifications.success(format!("Server started on port {}", server.port));
                info!("✅ Embedded server started successfully");
                
                state_events.write(EmbeddedServerStateChanged {
                    old_state,
                    new_state: server.state,
                    error: None,
                });
            }
            Err(e) => {
                error!("Failed to start server: {}", e);
                server.state = EmbeddedServerState::Error;
                notifications.error(format!("Failed to start server: {}", e));
                
                state_events.write(EmbeddedServerStateChanged {
                    old_state,
                    new_state: server.state,
                    error: Some(format!("Failed to spawn process: {}", e)),
                });
            }
        }
    }
}

/// System to handle stopping the embedded server
fn handle_embedded_server_stop(
    mut events: MessageReader<StopEmbeddedServerEvent>,
    mut server: ResMut<EmbeddedServer>,
    mut state_events: MessageWriter<EmbeddedServerStateChanged>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    for _event in events.read() {
        if !server.is_running() {
            continue;
        }
        
        let old_state = server.state;
        server.state = EmbeddedServerState::Stopping;
        
        info!("🛑 Stopping embedded server...");
        
        // Kill the server process
        if let Some(ref mut process) = server.process {
            match process.kill() {
                Ok(_) => {
                    info!("Server process killed");
                }
                Err(e) => {
                    warn!("Failed to kill server process: {}", e);
                }
            }
            
            // Wait for process to exit
            let _ = process.wait();
        }
        
        // Clean up temp scene file
        if let Some(ref path) = server.scene_path {
            let _ = std::fs::remove_file(path);
        }
        
        // Reset state
        server.process = None;
        server.scene_path = None;
        server.state = EmbeddedServerState::Stopped;
        server.started_at = None;
        
        notifications.info("Server stopped");
        
        state_events.write(EmbeddedServerStateChanged {
            old_state,
            new_state: server.state,
            error: None,
        });
    }
}

/// System to monitor embedded server health
fn monitor_embedded_server(
    mut server: ResMut<EmbeddedServer>,
    mut state_events: MessageWriter<EmbeddedServerStateChanged>,
) {
    if server.state != EmbeddedServerState::Running {
        return;
    }
    
    // Check if process is still running
    if let Some(ref mut process) = server.process {
        match process.try_wait() {
            Ok(Some(status)) => {
                // Process has exited
                let old_state = server.state;
                server.state = if status.success() {
                    EmbeddedServerState::Stopped
                } else {
                    EmbeddedServerState::Error
                };
                server.process = None;
                
                warn!("Embedded server exited with status: {:?}", status);
                
                state_events.write(EmbeddedServerStateChanged {
                    old_state,
                    new_state: server.state,
                    error: if status.success() { None } else { Some("Server crashed".to_string()) },
                });
            }
            Ok(None) => {
                // Process still running
            }
            Err(e) => {
                warn!("Failed to check server status: {}", e);
            }
        }
    }
}

/// Find the server executable path
fn find_server_executable() -> std::path::PathBuf {
    // Try to find in same directory as current executable
    if let Ok(current_exe) = std::env::current_exe() {
        let exe_dir = current_exe.parent().unwrap_or(std::path::Path::new("."));
        
        #[cfg(windows)]
        let server_name = "eustress-server.exe";
        #[cfg(not(windows))]
        let server_name = "eustress-server";
        
        let server_path = exe_dir.join(server_name);
        if server_path.exists() {
            return server_path;
        }
        
        // Try target/debug or target/release
        let debug_path = exe_dir.join("../eustress-server").join(server_name);
        if debug_path.exists() {
            return debug_path;
        }
    }
    
    // Fallback to just the name (rely on PATH)
    #[cfg(windows)]
    return std::path::PathBuf::from("eustress-server.exe");
    #[cfg(not(windows))]
    return std::path::PathBuf::from("eustress-server");
}

/// Build scene data from current entities for the server
fn build_scene_for_server(
    parts_query: &Query<(Entity, &Instance, Option<&BasePart>, Option<&Part>)>,
) -> crate::serialization::Scene {
    use crate::serialization::{Scene, SceneMetadata, EntityData};
    use std::collections::HashMap;
    
    let mut entities = Vec::new();
    
    for (entity, instance, basepart, part) in parts_query.iter() {
        let mut properties: HashMap<String, serde_json::Value> = HashMap::new();
        
        // Add Instance properties
        properties.insert("Name".to_string(), serde_json::json!(instance.name));
        properties.insert("Archivable".to_string(), serde_json::json!(instance.archivable));
        
        // Add BasePart properties if present
        if let Some(bp) = basepart {
            properties.insert("Position".to_string(), serde_json::json!([
                bp.cframe.translation.x,
                bp.cframe.translation.y,
                bp.cframe.translation.z
            ]));
            properties.insert("Size".to_string(), serde_json::json!([
                bp.size.x,
                bp.size.y,
                bp.size.z
            ]));
            let srgba = bp.color.to_srgba();
            properties.insert("Color".to_string(), serde_json::json!([
                srgba.red,
                srgba.green,
                srgba.blue
            ]));
            properties.insert("Transparency".to_string(), serde_json::json!(bp.transparency));
            properties.insert("Anchored".to_string(), serde_json::json!(bp.anchored));
            properties.insert("CanCollide".to_string(), serde_json::json!(bp.can_collide));
        }
        
        // Add Part properties if present
        if let Some(p) = part {
            properties.insert("Shape".to_string(), serde_json::json!(format!("{:?}", p.shape)));
        }
        
        entities.push(EntityData {
            id: entity.to_bits() as u32,
            class: format!("{:?}", instance.class_name),
            parent: None,
            properties,
            children: vec![],
            attributes: std::collections::HashMap::new(),
            tags: vec![],
            parameters: None,
        });
    }
    
    Scene {
        format: "eustress_propertyaccess".to_string(),
        metadata: SceneMetadata {
            name: "Play Session".to_string(),
            description: "Scene for Play Server mode".to_string(),
            author: "Eustress Engine".to_string(),
            created: chrono::Local::now().to_rfc3339(),
            modified: chrono::Local::now().to_rfc3339(),
            engine_version: "0.1.0".to_string(),
        },
        entities,
        global_sources: None,
        domain_configs: None,
        global_variables: None,
    }
}

// ============================================================================
// Physics Activation Systems
// ============================================================================

/// Marker component to track parts that had their physics activated during play mode
#[derive(Component)]
pub struct PlayModePhysicsActivated;

/// Marker component for entities spawned during play mode (should be despawned on stop)
#[derive(Component)]
pub struct SpawnedDuringPlayMode;

/// Activate physics for unanchored parts when entering play mode
fn activate_physics_for_unanchored_parts(
    mut commands: Commands,
    mut physics_time: ResMut<Time<Physics>>,
    parts_query: Query<(Entity, &BasePart), (With<Part>, Without<PlayModeCharacter>)>,
) {
    // Ensure physics simulation is running
    physics_time.unpause();
    info!("⚡ Physics simulation started");
    
    let mut activated_count = 0;
    
    for (entity, basepart) in parts_query.iter() {
        // Only activate physics for unanchored parts
        if !basepart.anchored {
            commands.entity(entity)
                .insert(RigidBody::Dynamic)
                .insert(PlayModePhysicsActivated);
            activated_count += 1;
        }
    }
    
    if activated_count > 0 {
        info!("⚡ Activated physics for {} unanchored parts", activated_count);
    }
}

/// Deactivate physics for parts when exiting play mode (restore to Static)
fn deactivate_physics_for_parts(
    mut commands: Commands,
    parts_query: Query<Entity, With<PlayModePhysicsActivated>>,
) {
    let mut deactivated_count = 0;
    
    for entity in parts_query.iter() {
        commands.entity(entity)
            .insert(RigidBody::Static)
            .remove::<PlayModePhysicsActivated>();
        deactivated_count += 1;
    }
    
    if deactivated_count > 0 {
        info!("🛑 Deactivated physics for {} parts", deactivated_count);
    }
}

/// Sync BasePart.anchored changes to RigidBody in real-time during play mode
/// This allows scripts to anchor/unanchor parts and have physics respond immediately
fn sync_anchored_to_rigidbody(
    mut commands: Commands,
    changed_parts: Query<(Entity, &BasePart, Option<&RigidBody>), (Changed<BasePart>, With<Part>, Without<PlayModeCharacter>)>,
) {
    for (entity, basepart, current_body) in changed_parts.iter() {
        let current_is_dynamic = matches!(current_body, Some(RigidBody::Dynamic));
        let should_be_dynamic = !basepart.anchored;
        
        // Only update if state changed
        if current_is_dynamic != should_be_dynamic {
            if should_be_dynamic {
                // Unanchored - make dynamic
                commands.entity(entity)
                    .insert(RigidBody::Dynamic)
                    .insert(PlayModePhysicsActivated);
                debug!("🔓 Part {:?} unanchored - now Dynamic", entity);
            } else {
                // Anchored - make static and zero velocity
                commands.entity(entity)
                    .insert(RigidBody::Static)
                    .insert(LinearVelocity::ZERO)
                    .insert(AngularVelocity::ZERO)
                    .remove::<PlayModePhysicsActivated>();
                debug!("🔒 Part {:?} anchored - now Static", entity);
            }
        }
    }
}

// ============================================================================
// Play Server Integration
// ============================================================================

/// Start the in-process play server when entering play mode with Server type
fn start_play_server_if_server_mode(
    play_mode: Res<PlayMode>,
    mut start_server: MessageWriter<crate::play_server::StartPlayServerMessage>,
) {
    if play_mode.play_type == PlayModeType::Server {
        info!("🖥️ Starting in-process play server...");
        start_server.write(crate::play_server::StartPlayServerMessage {
            server_addr: None, // Use default 127.0.0.1:7778
        });
    }
}

/// Stop the in-process play server when exiting play mode
fn stop_play_server_if_server_mode(
    play_mode: Res<PlayMode>,
    mut stop_server: MessageWriter<crate::play_server::StopPlayServerMessage>,
) {
    if play_mode.play_type == PlayModeType::Server {
        info!("🛑 Stopping in-process play server...");
        stop_server.write(crate::play_server::StopPlayServerMessage);
    }
}

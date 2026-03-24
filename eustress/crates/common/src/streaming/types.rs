//! # Streaming Types
//!
//! ## Table of Contents
//! - InstanceId       — unique identifier per instance (UUID v4)
//! - InstanceBin      — compact binary representation for cache + GPU upload
//! - InstanceRecord   — in-memory wrapper: InstanceBin + dirty bit + version + tier
//! - ChunkCoord       — 2D spatial grid coordinate for chunk-keyed storage
//! - Tier             — which storage tier an instance currently lives in
//! - StreamingConfig  — tuneable parameters for the streaming system

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// InstanceId — UUID v4 string, matches the TOML filename stem
// ─────────────────────────────────────────────────────────────────────────────

/// Unique identifier for a single instance within a Space.
/// Format: UUID v4 string (e.g. "550e8400-e29b-41d4-a716-446655440000").
/// The TOML file on disk is `{instance_id}.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstanceId(pub String);

impl InstanceId {
    /// Generate a new random instance identifier.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Wrap an existing string as an InstanceId.
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The raw string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for InstanceId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// InstanceBin — compact binary layout for cache, GPU upload, and network sync
// ─────────────────────────────────────────────────────────────────────────────

/// Compact binary representation of one instance.
/// Layout is fixed-size for zero-copy GPU upload via bytemuck.
///
/// This matches the benchmark's `InstanceBin` — proven at 2M decode/sec
/// via bincode + zstd.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Copy)]
#[repr(C)]
pub struct InstanceBin {
    /// World-space position [x, y, z].
    pub position: [f32; 3],
    /// Uniform scale factor.
    pub scale: f32,
    /// Euler rotation [pitch, yaw, roll] in radians.
    pub rotation: [f32; 3],
    /// Instance class/type identifier (index into class table).
    pub class_id: u32,
    /// Asset reference hash (first 8 bytes of SHA-256).
    pub asset_hash: [u8; 8],
    /// Velocity magnitude (0.0 = static, >0 = active for MoE gate).
    pub velocity: f32,
    /// Reserved padding for future fields (alignment to 48 bytes).
    pub _reserved: [u8; 4],
}

impl InstanceBin {
    /// Returns true if this instance has non-zero velocity (active for MoE gate).
    pub fn is_active(&self) -> bool {
        self.velocity.abs() > f32::EPSILON
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tier — which storage layer an instance currently occupies
// ─────────────────────────────────────────────────────────────────────────────

/// Storage tier in the three-tier streaming hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tier {
    /// On disk only — TOML + .bin sidecar. Zero frame cost.
    Cold,
    /// In RAM (DashMap hot cache). Zero ECS/GPU cost per frame.
    Hot,
    /// Spawned as a Bevy ECS entity with full Transform + rendering.
    Active,
}

// ─────────────────────────────────────────────────────────────────────────────
// InstanceRecord — in-memory wrapper around InstanceBin
// ─────────────────────────────────────────────────────────────────────────────

/// In-memory record wrapping an InstanceBin with streaming metadata.
/// Stored in the SpatialChunkGrid (DashMap).
pub struct InstanceRecord {
    /// Unique identifier (matches TOML filename stem).
    pub id: InstanceId,
    /// The compact binary data.
    pub bin: InstanceBin,
    /// Current storage tier.
    pub tier: Tier,
    /// Monotonically increasing version — incremented on every mutation.
    /// Used for conflict detection between in-memory and disk states.
    pub version: AtomicU64,
    /// True if in-memory state is ahead of disk (needs write-back).
    pub dirty: AtomicBool,
    /// Disk path to the canonical TOML file.
    pub toml_path: PathBuf,
    /// Disk path to the binary sidecar cache file.
    pub sidecar_path: PathBuf,
    /// Optional Bevy entity handle when tier == Active.
    pub entity: Option<bevy::ecs::entity::Entity>,
    /// Human-readable name for Explorer index.
    pub name: String,
    /// Tags for Explorer search/filter.
    pub tags: Vec<String>,
}

impl InstanceRecord {
    /// Create a new record from parsed TOML data.
    pub fn new(
        id: InstanceId,
        bin: InstanceBin,
        toml_path: PathBuf,
        name: String,
        tags: Vec<String>,
    ) -> Self {
        let sidecar_path = toml_path.with_extension("toml.bin");
        Self {
            id,
            bin,
            tier: Tier::Cold,
            version: AtomicU64::new(1),
            dirty: AtomicBool::new(false),
            toml_path,
            sidecar_path,
            entity: None,
            name,
            tags,
        }
    }

    /// Mutate the binary data, bump version, and set dirty flag.
    /// This is the primary write path for CRUD updates.
    pub fn update(&mut self, new_bin: InstanceBin) {
        self.bin = new_bin;
        self.version.fetch_add(1, Ordering::Relaxed);
        self.dirty.store(true, Ordering::Release);
    }

    /// Mark this record as flushed to disk (dirty = false).
    pub fn mark_flushed(&self) {
        self.dirty.store(false, Ordering::Release);
    }

    /// Check if this record needs write-back.
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    /// Current version number.
    pub fn current_version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ChunkCoord — 2D spatial grid coordinate
// ─────────────────────────────────────────────────────────────────────────────

/// 2D grid coordinate identifying a spatial chunk.
/// Chunks are square cells of side `StreamingConfig::chunk_size` world units.
/// The chunk containing world position (x, z) is:
///   ChunkCoord { x: floor(x / chunk_size), z: floor(z / chunk_size) }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkCoord {
    pub x: i32,
    pub z: i32,
}

impl ChunkCoord {
    /// Compute the chunk coordinate for a world-space position.
    pub fn from_world_position(world_x: f32, world_z: f32, chunk_size: f32) -> Self {
        Self {
            x: (world_x / chunk_size).floor() as i32,
            z: (world_z / chunk_size).floor() as i32,
        }
    }

    /// Center of this chunk in world space.
    pub fn center(&self, chunk_size: f32) -> [f32; 2] {
        [
            (self.x as f32 + 0.5) * chunk_size,
            (self.z as f32 + 0.5) * chunk_size,
        ]
    }

    /// Squared distance from this chunk's center to a world-space point.
    pub fn distance_squared_to(&self, world_x: f32, world_z: f32, chunk_size: f32) -> f32 {
        let [cx, cz] = self.center(chunk_size);
        let dx = world_x - cx;
        let dz = world_z - cz;
        dx * dx + dz * dz
    }

    /// Filesystem directory name for this chunk.
    pub fn directory_name(&self) -> String {
        format!("chunk_{}_{}", self.x, self.z)
    }
}

impl std::fmt::Display for ChunkCoord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.x, self.z)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// StreamingConfig — tuneable parameters
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the streaming system.
/// All distances are in world units. Loaded from `space.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingConfig {
    /// Side length of one spatial chunk in world units.
    /// Smaller = more granular streaming, larger = fewer I/O ops.
    /// Default: 256.0 (256m × 256m chunks).
    pub chunk_size: f32,

    /// Radius within which instances are promoted from Hot → Active (ECS entities).
    /// Should be <= the camera far plane.
    /// Default: 500.0
    pub active_radius: f32,

    /// Radius beyond which Active instances are demoted back to Hot cache.
    /// Must be > active_radius to prevent thrashing (hysteresis band).
    /// Default: 600.0
    pub evict_radius: f32,

    /// Radius beyond which Hot cache entries are demoted to Cold (freed from RAM).
    /// Default: 2000.0
    pub cold_radius: f32,

    /// Maximum number of ECS entities in the active zone.
    /// Benchmark-proven ceiling: ~2.10M @ 24 FPS.
    /// Default: 2_000_000
    pub active_cap: usize,

    /// Interval between background dirty-bit flush passes.
    /// Default: 100ms
    pub flush_interval: Duration,

    /// Maximum instances to flush per batch (prevents I/O stalls).
    /// Default: 1000
    pub flush_batch_size: usize,

    /// MoE active fraction — percentage of entities expected to move each frame.
    /// Used for pre-allocating Changed<Transform> buffers.
    /// Benchmark-proven: 10% active gives 5–10× sparse gate speedup.
    /// Default: 0.10
    pub moe_active_fraction: f32,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            chunk_size:           256.0,
            active_radius:        500.0,
            evict_radius:         600.0,
            cold_radius:          2000.0,
            active_cap:           2_000_000,
            flush_interval:       Duration::from_millis(100),
            flush_batch_size:     1_000,
            moe_active_fraction:  0.10,
        }
    }
}

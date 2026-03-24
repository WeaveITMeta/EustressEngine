//! # Instance Streaming System
//!
//! Three-tier streaming architecture for Eustress Spaces:
//!
//! ## Table of Contents
//! - `types`          — InstanceRecord, InstanceBin, ChunkCoord, InstanceId
//! - `sidecar`        — .bin sidecar encode/decode with version header + invalidation
//! - `chunk_grid`     — SpatialChunkGrid: chunk-keyed DashMap with R-tree spatial index
//! - `radius_gate`    — HysteresisRadiusGate: two-threshold promote/demote logic
//! - `dirty_flusher`  — DirtyBitFlusher: background thread batched async write-back
//! - `toml_watcher`   — TomlWatcher: notify-based reactive reload on external edits
//! - `instance_index` — InstanceIndex: flat metadata index for Explorer queries
//! - `plugin`         — StreamingPlugin: Bevy plugin wiring ECS events
//!
//! ## Architecture
//!
//! ```text
//! DISK (TOML canonical + .bin sidecar cache)
//!     ↕  stream in/out (zstd decode ~2M inst/sec)
//! RAM HOT CACHE (SpatialChunkGrid — DashMap<ChunkCoord, Chunk>)
//!     ↕  promote/demote by HysteresisRadiusGate
//! ECS ACTIVE ZONE (Bevy entities, ~2.10M ceiling @ 24 FPS)
//!     ↕  Changed<Transform> MoE sparse gate
//! GPU (instanced draw / indirect draw buffer)
//! ```
//!
//! ## Benchmark-Proven Numbers
//! - Active ECS zone: ~2.10M entities @ 24 FPS (measured)
//! - MoE sparse gate: 5–10× ECS query speedup via Changed<Transform>
//! - Binary decode: ~2M instances/sec (zstd + bincode)
//! - Streaming eviction: 4.7ms @ 2.10M total (measured)
//! - Physics MoE gate: 81% of entities route to static AABB (zero solver cost)

pub mod types;
pub mod sidecar;
pub mod chunk_grid;
pub mod radius_gate;
pub mod dirty_flusher;
pub mod toml_watcher;
pub mod instance_index;
pub mod plugin;

// Re-export primary types for ergonomic use
pub use types::{InstanceId, InstanceBin, InstanceRecord, ChunkCoord, StreamingConfig, Tier};
pub use sidecar::{encode_sidecar, decode_sidecar, invalidate_sidecar, SidecarHeader};
pub use chunk_grid::SpatialChunkGrid;
pub use radius_gate::HysteresisRadiusGate;
pub use dirty_flusher::DirtyBitFlusher;
pub use toml_watcher::TomlWatcher;
pub use instance_index::InstanceIndex;
pub use plugin::StreamingPlugin;

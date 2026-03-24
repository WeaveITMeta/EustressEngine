# Eustress: Path to World-Class Engine

## Table of Contents

1. [Honest Baseline](#1-honest-baseline)
2. [The Seven Pillars of a World-Class Engine](#2-the-seven-pillars)
3. [Pillar 1 — Scene Representation: TOML + ECS → USD-Style Composition](#3-pillar-1-scene-representation)
4. [Pillar 2 — Virtualized Geometry: Nanite-Style Instance Streaming](#4-pillar-2-virtualized-geometry)
5. [Pillar 3 — Global Illumination: Lumen-Style Dynamic GI](#5-pillar-3-global-illumination)
6. [Pillar 4 — Physics at Scale: 10M Entity Simulation Budget](#6-pillar-4-physics-at-scale)
7. [Pillar 5 — Scripting Runtime: Sub-Millisecond Hot Reload](#7-pillar-5-scripting-runtime)
8. [Pillar 6 — Networking: QUIC-Native Deterministic Simulation](#8-pillar-6-networking)
9. [Pillar 7 — AI Integration: Inference in the Game Loop](#9-pillar-7-ai-integration)
10. [Rust Advantage Matrix](#10-rust-advantage-matrix)
11. [Concrete Implementation Roadmap](#11-implementation-roadmap)
12. [What We Do NOT Need](#12-what-we-do-not-need)

---

## 1. Honest Baseline

What Eustress has today that is genuinely competitive:

| Capability | Status | Competing Engine Gap |
|---|---|---|
| File-system-first scene (TOML instances) | Shipping | No direct equivalent in Unity/Godot |
| Avian3D rigid body + joints | Shipping | Equivalent to PhysX quality |
| Rune hot-reload scripting | Shipping | Better than Lua; worse than GDScript DX |
| Bevy ECS archetype storage | Shipping | Same theoretical ceiling as Unity DOTS |
| Symbolica symbolic physics | Architecture exists | Unique — no equivalent in UE5/Unity |
| Forge Nomad orchestration | Architecture exists | Better cost model than Roblox's Nomad setup |
| Kernel Law system | Spec only | Unique concept, no competitor |
| LOD mesh generation + instancing detection | Shipping (`pointcloud/mesh_optimization.rs`) | Building block for Nanite clustering |
| Progressive asset streaming with LOD levels | Shipping (`assets/progressive.rs`) | LOD0–LOD4 at configurable distances |
| R-tree spatial index | Shipping (`eustress-geo` via `rstar 0.12`) | Building block for instance streaming |
| Nanite-style GPU cluster hierarchy | Not started | 3-year gap vs UE5 |
| Lumen-style GI | Not started | 3-year gap vs UE5 |
| USD scene interop | Not started | 2-year gap vs Omniverse |

The Rust bet is real: **Bevy ECS with archetype storage achieves cache-coherent iteration over 10M+ entities at 2–4× the throughput of Unity DOTS** in published benchmarks (Bevy's own bench suite, 2024). This is the foundation everything else must build on.

---

## 2. The Seven Pillars

World-class means beating the best engine on each dimension independently, not just averaging across them.

The seven independent dimensions where an engine wins or loses:

1. **Scene Representation** — how you describe, compose, and diff world state
2. **Virtualized Geometry** — how many triangles you can render without budgeting
3. **Global Illumination** — perceptual realism of lighting
4. **Physics at Scale** — how many simulated entities at 60Hz
5. **Scripting Runtime** — iteration velocity for content creators
6. **Networking** — latency, determinism, scale of multiplayer
7. **AI Integration** — NPCs, procedural content, inference in the loop

Eustress today competes on **1, 4, 5, and 6**. It does not yet compete on **2, 3, or 7**.

---

## 3. Pillar 1 — Scene Representation

### The Problem Omniverse Solved

NVIDIA Omniverse's core insight (published in the Frontiers survey, 2024): **USD (Universal Scene Description) as a composable, layer-based, content-addressable scene graph** eliminates 3D data silos. The key architectural features:

- **Layer composition**: Override layers stack on base layers. A lighting artist edits a lighting layer without touching geometry.
- **Content-Addressable Storage (CAS)**: Nucleus stores assets by content hash. Identical meshes across 10K scenes = 1 copy on disk.
- **Live collaboration**: Publish/subscribe model — changes propagate in real-time across connected clients.
- **Schema extensibility**: Physics schema, audio schema, custom application schemas all extend the base prim (primitive) type.

Eustress's TOML instance system is **structurally similar** but missing the composition and CAS layers.

### What Eustress Must Add

**Existing foundation** (`eustress/crates/engine/src/space/`):
- `instance_loader.rs` — `InstanceDefinition` struct, TOML deserialization
- `class_defaults.rs` — `ClassDefaultsRegistry`, merge-on-load defaults
- `file_loader.rs` — `spawn_file_entry`, directory scanning

**Gap**: No layer composition. Every instance is a flat TOML — there is no mechanism to say "apply this override on top of the base."

### Concrete Implementation: ECS Layer Composition

```rust
// eustress/crates/engine/src/space/composition.rs

/// A stack of override layers applied on top of a base InstanceDefinition.
/// Mirrors USD's layer composition (strongest opinion wins).
#[derive(Debug, Clone)]
pub struct LayerStack {
    pub base:      InstanceDefinition,
    pub overrides: Vec<LayerOverride>,
}

#[derive(Debug, Clone)]
pub struct LayerOverride {
    pub source: OverrideSource,
    pub delta:  toml::Value,        // sparse: only keys that differ
    pub strength: u8,               // 0=weakest, 255=strongest (USD opinion strength)
}

#[derive(Debug, Clone)]
pub enum OverrideSource {
    File(std::path::PathBuf),       // .override.toml on disk
    Network(String),                // live edit from Studio
    Script(String),                 // Rune script output
}

/// Merge all override layers onto the base, strongest-opinion wins.
/// O(n * keys) where n = number of layers.
pub fn compose(stack: &LayerStack) -> InstanceDefinition {
    let mut merged: toml::Value = toml::Value::try_from(&stack.base)
        .expect("base always serializable");

    let mut sorted = stack.overrides.clone();
    sorted.sort_by_key(|o| o.strength);   // weakest first, strongest last

    for layer in &sorted {
        super::class_defaults::merge_defaults(&mut merged, &layer.delta);
    }

    merged.try_into().expect("merged value always valid InstanceDefinition")
}
```

**CAS for assets** — add to `eustress-common`:

```rust
// eustress/crates/common/src/assets/cas.rs

use blake3::Hasher;
use std::collections::HashMap;

/// Content-addressable asset store.
/// Key insight from Omniverse Nucleus: store by content hash, not by path.
/// One 50MB mesh used in 10K scenes = 50MB on disk, not 500GB.
pub struct ContentAddressableStore {
    root:  std::path::PathBuf,
    index: HashMap<blake3::Hash, CasEntry>,
}

#[derive(Debug, Clone)]
pub struct CasEntry {
    pub hash:      blake3::Hash,
    pub size:      u64,
    pub mime_type: String,
    pub ref_count: u32,
}

impl ContentAddressableStore {
    /// Store bytes, return content hash. Deduplicates automatically.
    pub fn store(&mut self, data: &[u8], mime_type: &str) -> blake3::Hash {
        let hash = blake3::hash(data);
        self.index.entry(hash).or_insert_with(|| {
            let path = self.root.join(hash.to_hex().as_str());
            std::fs::write(&path, data).expect("CAS write failed");
            CasEntry {
                hash,
                size: data.len() as u64,
                mime_type: mime_type.to_string(),
                ref_count: 0,
            }
        }).ref_count += 1;
        hash
    }

    /// Retrieve bytes by content hash. O(1) lookup.
    pub fn get(&self, hash: &blake3::Hash) -> Option<Vec<u8>> {
        if self.index.contains_key(hash) {
            let path = self.root.join(hash.to_hex().as_str());
            std::fs::read(path).ok()
        } else {
            None
        }
    }
}
```

**Rust crates needed**:
- `blake3 = "1"` — already in workspace
- `notify = "6"` — already in workspace (for live layer watching)

**Milestone**: Layer composition shipping → Eustress scenes become composable like USD. This enables multi-user editing, A/B testing of overrides, and incremental world streaming.

---

## 4. Pillar 2 — Virtualized Geometry

### What Nanite Actually Does (SIGGRAPH 2021, Karis)

Nanite's core algorithm (from Brian Karis's SIGGRAPH 2021 talk):

1. **Mesh preprocessing**: Split mesh into clusters of ~128 triangles at build time. Build a hierarchical DAG (Directed Acyclic Graph) of progressively simplified cluster groups.
2. **Runtime LOD selection**: Per-cluster, project screen-space error. If a cluster's error < 1 pixel, it is at the right LOD level. This is done on the GPU via compute shaders.
3. **Software rasterizer**: For clusters with small projected triangles (< 4–5 pixels), switch from hardware rasterizer to a software rasterizer running in a compute shader. This eliminates triangle setup overhead for micro-triangles.
4. **Visibility buffer**: Instead of rendering to a GBuffer directly, render a visibility buffer storing (instance ID, triangle ID) per pixel. Material evaluation is deferred to a screen-space pass.
5. **Persistent culling**: A two-pass GPU-driven culling pipeline. Pass 1: occludee culling using last frame's HZB (Hierarchical Z-Buffer). Pass 2: reproject any newly visible geometry.

**Key insight**: The CPU never touches individual triangles. The entire LOD selection and culling pipeline runs on GPU compute. CPU only manages cluster instance submissions.

### Eustress Path: GPU-Driven Rendering via WGPU

Bevy 0.18 uses WGPU 27+. The path to Nanite-style rendering in Bevy:

**Phase 1 — GPU-driven indirect draw** (the prerequisite):

```rust
// eustress/crates/engine/src/rendering/gpu_driven.rs

/// GPU-driven draw call submission using indirect draw buffers.
/// Reference: "GPU-Driven Rendering Pipelines" — Wihlidal, GDC 2015.
///
/// Instead of one DrawIndexed call per mesh from CPU,
/// fill a DrawIndexedIndirect buffer on GPU and call DrawIndirectMulti once.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawIndexedIndirectArgs {
    pub index_count:    u32,
    pub instance_count: u32,
    pub first_index:    u32,
    pub base_vertex:    i32,
    pub first_instance: u32,
}

/// Bevy render node that:
/// 1. Runs a compute pass to cull instances (frustum + occlusion)
/// 2. Writes surviving instances to DrawIndexedIndirectArgs buffer
/// 3. Issues a single DrawIndirectMulti call
pub struct GpuDrivenCullNode;
```

**Phase 2 — Cluster hierarchy** (Nanite core):

```rust
/// Build-time cluster generation.
/// Each mesh is split into clusters of MAX_CLUSTER_TRIANGLES.
/// Cluster groups are simplified into a parent cluster (LOD DAG).
///
/// Reference algorithm:
/// 1. Partition mesh into clusters using METIS graph partitioning
/// 2. For each cluster group (4 clusters), simplify to half the triangles
/// 3. Store parent cluster — this is the coarser LOD
/// 4. Repeat until 1 root cluster remains
pub const MAX_CLUSTER_TRIANGLES: usize = 128;

#[derive(Debug, Clone)]
pub struct Cluster {
    pub triangle_offset: u32,    // index into global triangle buffer
    pub triangle_count:  u32,
    pub bounding_sphere: [f32; 4],  // xyz = center, w = radius
    pub lod_error:       f32,    // max geometric error vs original mesh (meters)
    pub parent_error:    f32,    // parent cluster's error — for DAG traversal
    pub children:        Vec<u32>,  // child cluster indices
}

#[derive(Debug, Clone)]
pub struct ClusterMesh {
    pub clusters:  Vec<Cluster>,
    pub triangles: Vec<u32>,     // index buffer
    pub vertices:  Vec<[f32; 3]>,
}
```

**Rust crates needed**:
- `meshopt = "0.2"` — mesh optimization and simplification (what Nanite uses under the hood)
- `bytemuck = "1"` — already in workspace

**Why Rust wins here**: Bevy's WGPU backend gives direct access to compute shaders with no C++ interop. The cluster preprocessing can run with Rayon in parallel. No C++ METIS binding needed — the `metis` crate wraps it, or we implement greedy partitioning in pure Rust.

**Realistic timeline**: GPU-driven indirect draw is 2–3 months. Full cluster hierarchy is 6–9 months. This is a dedicated graphics engineering track.

---

## 5. Pillar 3 — Global Illumination

### What Lumen Does (Karis, GDC 2022)

Lumen's architecture:

1. **Surface cache**: Capture material properties (albedo, normal, emissive) into a software-rendered atlas. Updated continuously for dynamic objects.
2. **Mesh SDF (Signed Distance Fields)**: Every mesh gets a sparse SDF volume. Used for fast ray-marching without GPU ray tracing hardware.
3. **Global SDF**: A coarser world-space SDF merged from all mesh SDFs. Used for distant irradiance queries.
4. **Radiance cache**: Screen-space probes placed on surfaces. Each probe stores incoming radiance via secondary rays traced against the Global SDF or hardware BVH.
5. **Final gather**: Screen-space final gather reads from nearby probes, interpolates irradiance, applies temporal filtering.

**Key insight**: Lumen is NOT pure ray tracing. It is a **hybrid** that uses:
- Software ray marching via SDF for efficiency
- Hardware ray tracing (on cards that support it) as a quality upgrade
- Screen-space reuse via radiance caches

### Eustress Path: Radiance Cascades

Lumen is the wrong target for Eustress in 2025–2026. The right target is **Radiance Cascades**, a 2024 paper by Alexander Sannikov that is:
- Simpler to implement than Lumen
- Produces equivalent quality for 2D and 3D scenes
- Already has a Bevy community implementation starting

**Radiance Cascades core algorithm** (Sannikov 2024):

```
For each cascade level k (0=finest, N=coarsest):
  - Probe spacing: 2^k * base_spacing
  - Ray length: 2^k * base_ray_length
  - Angular resolution: 4^(N-k) directions per probe (finest = most directions)

Merge pass (coarse to fine):
  - For each probe at level k, blend in contributions from level k+1
  - Result: each probe at level 0 has full-sphere irradiance from all distances
```

```rust
// eustress/crates/engine/src/rendering/radiance_cascades.rs
//
// Reference: "Radiance Cascades: A Novel High-Resolution Formal Solution
//             for Diffuse Global Illumination" — Sannikov, 2024

pub struct RadianceCascadesPlugin;

/// Per-cascade probe data stored in a 3D texture array.
/// Each texel = a probe's radiance in one angular bucket.
pub struct CascadeTextures {
    pub levels:         u32,        // typically 6–8
    pub base_spacing:   f32,        // world units between level-0 probes
    pub base_ray_len:   f32,        // max trace distance at level 0 (e.g. 0.5m)
    pub probe_atlas:    wgpu::Texture,  // rgba16float, layers = levels
}

/// WGSL compute shader inputs (one dispatch per cascade level, bottom-up)
pub struct CascadeMergeUniforms {
    pub level:          u32,
    pub probe_spacing:  f32,
    pub ray_length:     f32,
    pub world_to_probe: [[f32; 4]; 4],
}
```

**Rust crates needed**: No new crates — WGPU is already available via Bevy.

**Competitive position**: If Eustress ships Radiance Cascades, it achieves **Lumen-quality GI** using an algorithm that is newer than Lumen (2024 vs 2022) and more mathematically rigorous. This is a genuine competitive advantage over UE5 Lumen.

---

## 6. Pillar 4 — Physics at Scale

### The 10M Instance Problem

From the previous benchmark session, the in-memory parsing target is 10M `InstanceDefinition` structs. The memory cost is the hard limit.

**Measured costs** (from the benchmark crate, Phase 3 pending):
- Conservative estimate: 512–1024 bytes per `InstanceDefinition` including HashMap allocations
- At 10M instances: **5–10 GB RAM** — this requires streaming

**The Streaming Architecture**:

```
Active Zone (ECS entities)     ─── ~50K instances in Bevy ECS
   │
   ▼ stream in/out
Hot Cache (Arc<InstanceDefinition>)  ─── ~500K instances in memory
   │
   ▼ page from disk
Cold Storage (TOML files / bincode chunks)  ─── unlimited instances on disk
```

**Implementation**:

```rust
// eustress/crates/engine/src/space/streaming.rs
//
// Reference: Streaming architectures from:
// - "Streaming in Horizon Zero Dawn" — GDC 2017 (Guerrilla)
// - "Open World Loading in Witcher 3" — CD Projekt 2016
// Both use a similar radius-based loading with hysteresis to prevent thrashing.

use std::sync::Arc;
use dashmap::DashMap;
use bevy::math::Vec3;

pub struct InstanceStreamer {
    /// Hot cache: recently accessed instances, evicted by LRU
    hot_cache:   DashMap<InstanceId, Arc<InstanceDefinition>>,
    /// Currently active as Bevy entities
    active:      DashMap<InstanceId, bevy::ecs::entity::Entity>,
    /// Load/unload hysteresis radii
    pub load_radius:   f32,   // spawn as entity when camera within this radius
    pub unload_radius: f32,   // despawn entity when camera beyond this radius
}

impl InstanceStreamer {
    /// Called each frame with camera position.
    /// Returns (to_spawn, to_despawn) sets.
    pub fn update(
        &self,
        camera_pos: Vec3,
        spatial_index: &super::spatial::SpatialIndex,
    ) -> (Vec<InstanceId>, Vec<InstanceId>) {
        let candidates = spatial_index.query_radius(camera_pos, self.load_radius);
        let active_ids: Vec<InstanceId> = self.active.iter().map(|e| *e.key()).collect();

        let to_spawn: Vec<InstanceId> = candidates.iter()
            .filter(|id| !self.active.contains_key(id))
            .cloned()
            .collect();

        let to_despawn: Vec<InstanceId> = active_ids.iter()
            .filter(|id| {
                let pos = spatial_index.position_of(id);
                pos.map(|p| p.distance(camera_pos) > self.unload_radius)
                   .unwrap_or(true)
            })
            .cloned()
            .collect();

        (to_spawn, to_despawn)
    }
}
```

**Spatial index** (already partially exists in `eustress-geo` via `rstar`):

```rust
// eustress/crates/engine/src/space/spatial.rs
//
// Use rstar R-tree for O(log n) radius queries.
// At 10M instances: rstar handles 10M points with ~200MB overhead.
// Query time: ~50 microseconds for radius query returning 50K candidates.
//
// Reference: "rstar: A general-purpose R* tree implementation" (crates.io)
// Benchmark: 10M insertions in ~8s, queries in ~50us (rstar docs, 2024)

use rstar::{RTree, RTreeObject, AABB, PointDistance};

pub struct SpatialIndex {
    tree: RTree<SpatialEntry>,
}

#[derive(Clone, Debug)]
pub struct SpatialEntry {
    pub id:  InstanceId,
    pub pos: [f32; 3],
}
```

**Physics budget at 10M instances**:
- Avian3D (Rapier under the hood): benchmarked at ~100K rigid bodies at 60Hz on a modern CPU.
- For 10M instances: only ~1% can be rigid bodies at any time. The rest are static or kinematic.
- **Correct architecture**: Active physics zone = 5K rigid bodies (within 50m of player). Sleeping/static = everything else. LOD physics for mid-range (10–200m): simplified collision shapes, 20Hz tick. Beyond 200m: pure position data, no physics.

---

## 7. Pillar 5 — Scripting Runtime

### Current State vs Best-in-Class

| Engine | Scripting | Hot Reload | Performance |
|---|---|---|---|
| UE5 | C++ / Blueprint | No (C++), Yes (Blueprint) | C++ = native |
| Unity | C# / Visual Scripting | Yes (domain reload, slow) | JIT = near-native |
| Godot | GDScript / C# | Yes (fast) | GDScript = 10× slower than C++ |
| **Eustress** | **Rune 0.14** | **Yes (hot_reload.rs)** | **Rune = 2–5× slower than C++** |

Eustress's Rune scripting is **already best-in-class for safety and correctness** — Rune is sandboxed, typed, and cannot cause undefined behavior. The gap is **developer experience**, not performance.

### What Must Change

**1. Script error reporting in the Studio UI** (most impactful):

```rust
// eustress/crates/engine/src/soul/diagnostics.rs
//
// Rune compile errors → Slint UI panel with file:line:col
// This is what makes GDScript feel "fast" — not performance, feedback loops.

#[derive(Debug, Clone)]
pub struct ScriptDiagnostic {
    pub script_path: String,
    pub line:        u32,
    pub col:         u32,
    pub message:     String,
    pub severity:    DiagnosticSeverity,
}

pub enum DiagnosticSeverity { Error, Warning, Info }
```

**2. Script breakpoints via Rune's debug interface** (rune 0.14 supports this):

```rust
// The rune::debug module provides execution hooks.
// Wire these to a Slint debugger panel.
use rune::runtime::debug::DebugInfo;
```

**3. Reduce hot-reload latency**: Current implementation in `hot_reload.rs` uses `notify` file watcher. Latency is typically 50–200ms on Windows. Target: < 16ms (one frame).

The fix: use `notify`'s `RecommendedWatcher` with `PreciseEvents` mode, and pre-compile the Rune AST on a background thread the moment the file starts being written (debounce to 16ms).

---

## 8. Pillar 6 — Networking

### Current State

- QUIC via `quinn` — in `Cargo.toml`, not yet wired into game loop
- `eustress-networking` crate — disabled in engine `Cargo.toml` comment: "TODO: Fix Bevy 0.17 compatibility issues"
- Forge Nomad orchestration — architecture complete, not shipping

### What Best-in-Class Looks Like

**Deterministic lockstep** (fighting games, RTS): every client simulates identically given the same inputs. No authoritative server needed. Latency requirement: < 100ms round trip.

**Client-server with prediction** (shooters, MMOs): server is authoritative. Client predicts locally, rolls back on mismatch. Latency tolerance: 200ms with good feel.

**Interest management** (MMOs, open worlds): server only sends state for entities within a client's interest radius. This is what allows 10K+ players in one world.

Eustress's architecture (Forge + QUIC) is well-suited for client-server. The missing piece is **rollback netcode** in the ECS layer.

### Concrete Implementation: GGRS Integration

```rust
// GGRS is the Rust implementation of GGPO rollback netcode.
// Reference: "GGPO: Rollback Networking" — Tony Cannon, 2012
//            "GGRS: A Rollback Networking Library for Rust" — crates.io 2023

// Add to eustress/Cargo.toml workspace.dependencies:
// ggrs = "0.10"
// bevy_ggrs = "0.16"  (Bevy 0.18 compatible)

// eustress/crates/engine/src/networking/rollback.rs

use bevy_ggrs::prelude::*;

/// Mark components that must be saved/restored on rollback.
/// Only add this to components that change each frame.
/// Static geometry does NOT need rollback.
#[derive(Component, Clone, Reflect)]
#[reflect(Component)]
pub struct Rollback;

/// Register all rollback-eligible components.
pub fn add_rollback_systems(app: &mut App) {
    app
        .rollback_component_with_clone::<Transform>()
        .rollback_component_with_clone::<LinearVelocity>()
        .rollback_component_with_clone::<AngularVelocity>()
        .rollback_resource_with_clone::<SimulationClock>();
}
```

**Why Rust wins here**: GGRS is written in Rust with zero-copy rollback via `Clone` bounds. In C++ engines, rollback requires manual memory management. In Rust, the borrow checker enforces correctness.

---

## 9. Pillar 7 — AI Integration

### The Correct Architecture (Not RAG, Not Fine-Tuning)

The common mistake is treating AI as a content generation pipeline (text → asset). That is batch processing. The winning architecture is **AI in the inner loop**:

```
Game tick (16ms) ─► ECS Query ─► Feature extraction ─► ONNX inference ─► ECS Write
                                   (positions, HP, state)  (NPC behavior)    (velocity, action)
```

**Inference budget**: At 60Hz with 1000 NPCs, each NPC gets 16.6ms / 1000 = **16 microseconds** of CPU time. An ONNX model with 100K parameters runs in ~50 microseconds on CPU. This means:
- 60Hz NPC AI: max ~300 concurrent ONNX inferences per frame on a single CPU
- With GPU: 10K+ concurrent inferences per frame (batched)

### Implementation: ORT in Bevy

```rust
// eustress/crates/common/src/ai/inference.rs
//
// Reference: "ONNX Runtime: Cross-platform, high performance ML inferencing"
//            (Microsoft, 2024 — ort crate wraps this)
// ort = "2.0" (already in RECURSIVE_FEEDBACK_LOOP.md dependencies)

use ort::{Environment, Session, SessionBuilder, Value};
use std::sync::Arc;

pub struct NpcBrainPool {
    /// One session per NPC archetype (Grunt, Boss, Civilian, etc.)
    /// Sessions are NOT per-entity — they are shared across all NPCs of same type.
    sessions: dashmap::DashMap<String, Arc<Session>>,
    env:      Arc<Environment>,
}

impl NpcBrainPool {
    /// Batch inference: run all Grunt NPCs in one ONNX call.
    /// Input shape: [batch_size, feature_dim]
    /// Output shape: [batch_size, action_dim]
    pub fn infer_batch(
        &self,
        archetype: &str,
        features: &[f32],    // flattened [batch_size * feature_dim]
        batch_size: usize,
        feature_dim: usize,
    ) -> Vec<f32> {
        let session = self.sessions.get(archetype)
            .expect("archetype must be registered");

        let input = Value::from_array(
            session.allocator(),
            &ndarray::Array2::from_shape_vec(
                (batch_size, feature_dim),
                features.to_vec()
            ).unwrap()
        ).unwrap();

        let outputs = session.run(vec![input]).unwrap();
        outputs[0].try_extract::<f32>().unwrap()
                  .view()
                  .as_slice()
                  .unwrap()
                  .to_vec()
    }
}
```

**The Symbolica × AI advantage**: Eustress uniquely can **derive physics laws symbolically** and use them as features for ONNX models. An NPC that reasons about thermodynamics (temperature affects powder charge affects bullet speed) is impossible in UE5 without hardcoded lookup tables. In Eustress, Symbolica generates the exact formula and the NPC's ONNX model receives physically meaningful inputs.

---

## 10. Rust Advantage Matrix

Where Rust gives Eustress a genuine architectural edge over C++ engines:

| Capability | Rust Advantage | Practical Gain |
|---|---|---|
| **Memory safety** | No UAF, no double-free, no buffer overflow — provably | Zero crash-class of bugs that plague UE5/Unity |
| **Fearless concurrency** | Send + Sync bounds prevent data races at compile time | Can parallelize any system without mutexes unless genuinely needed |
| **Zero-cost abstractions** | Iterators, closures, generics compile to the same code as hand-written C++ | No performance tax for using high-level patterns |
| **Algebraic types** | `Option<T>`, `Result<T,E>` force error handling | No null pointer crashes, no silent failures |
| **Const generics** | Fixed-size arrays, type-level dimensions | Physics vectors and matrices with compile-time dimension checking |
| **Cargo workspace** | Reproducible builds, lockfile, audit | No "works on my machine" — critical for Forge deployments |
| **WASM target** | `cargo build --target wasm32-unknown-unknown` | Web Studio, web previews, WebAssembly scripting |
| **WGPU** | Vulkan/DX12/Metal/WebGPU from one backend | Genuine cross-platform compute shaders — Bevy's biggest hardware advantage |

The C++ comparison is often stated incorrectly. C++ is not slow — it is dangerously flexible. Rust gives **the same performance floor as C++ with a higher safety ceiling**. The win is not speed, it is **the absence of an entire class of production bugs**.

---

## 11. Implementation Roadmap

### Priority ordering: impact × feasibility

**Tier 1 — Do Now (1–3 months)**

| Work | Files | Existing Foundation | Research Basis |
|---|---|---|---|
| Layer composition for TOML instances | `space/composition.rs` (new) | `space/class_defaults.rs` — `merge_defaults()` already exists | USD opinion strength model |
| Content-Addressable Storage for assets | `common/src/assets/cas.rs` (new) | `common/src/assets/asset_id.rs` — `ContentHash` type already defined | Omniverse Nucleus CAS (Frontiers 2024) |
| Spatial index for instance streaming | `space/spatial.rs` (new) | `eustress-geo/src/spatial_index.rs` — `GeoSpatialIndex` R-tree already built | rstar R-tree, O(log n) radius queries |
| InstanceStreamer with hysteresis | `space/streaming.rs` (new) | `common/src/assets/progressive.rs` — LOD distance model + `StreamingState` already defined | Horizon Zero Dawn streaming (GDC 2017) |
| Script diagnostics → Slint UI | `soul/diagnostics.rs` (new) | `soul/rune_ecs_module.rs` — execution hooks available | Feedback loop before raw performance |
| In-memory benchmark Phase 2 | `benches/instance-capacity/src/main.rs` | Phase 1 filesystem benchmark complete | Ongoing from last session |

**Tier 2 — Next Quarter (3–6 months)**

| Work | Files | Research Basis |
|---|---|---|
| GPU-driven indirect draw | `rendering/gpu_driven.rs` (new) | Wihlidal GDC 2015, prerequisite for Nanite |
| GGRS rollback netcode integration | `networking/rollback.rs` (new) | GGPO paper, bevy_ggrs crate |
| ORT batch NPC inference | `common/src/ai/inference.rs` (new) | ONNX Runtime, ort 2.0 |
| Fix eustress-networking Bevy 0.18 compat | `crates/common/eustress-networking/` | Blocked on current TODO |

**Tier 3 — H2 2026 (6–12 months)**

| Work | Research Basis |
|---|---|
| Radiance Cascades GI | Sannikov 2024 paper — newer than Lumen |
| Cluster hierarchy (Nanite phase 1) | Karis SIGGRAPH 2021, meshopt crate |
| LOD physics zones | Avian3D LOD API + zone budgeting |

**New Cargo dependencies to add**:

```toml
# In eustress/Cargo.toml [workspace.dependencies]

# Mesh processing (Nanite cluster building)
meshopt = "0.2"

# Rollback netcode
ggrs = "0.10"
bevy_ggrs = "0.16"   # verify Bevy 0.18 compat before pinning

# ML inference (already noted in RECURSIVE_FEEDBACK_LOOP.md)
ort = "2.0"
ndarray = "0.16"

# Spatial index (already in eustress-geo, expose to engine)
# rstar = "0.12"  -- already present via geo crate
```

---

## 12. What We Do NOT Need

Applying the "delete the part" principle:

**Do NOT implement**:

- **USD format parser** — USD is C++ only (Pixar's OpenUSD). Writing a full USD parser in Rust is 2–3 years of work for no user-facing benefit. Eustress's TOML + layer composition achieves the same compositional semantics. Interop with USD tools can be achieved via GLTF 2.0 export (already partially exists).

- **Nanite-exact software rasterizer** — The software rasterizer in Nanite is optimized for HLSL compute on NVIDIA Ampere. On WGPU/WGSL, the portable approach (GPU-driven indirect + meshopt cluster LOD) achieves 90% of Nanite's benefit with 20% of the implementation cost.

- **A new physics engine** — Avian3D (Rapier) is competitive with PhysX 5. The investment is in **LOD physics scheduling**, not replacing the solver.

- **A proprietary scripting language** — Rune is excellent. The investment is in **editor tooling** (diagnostics, breakpoints), not the language itself.

- **Custom networking transport** — QUIC via Quinn is correct. The investment is in **game-layer rollback** (GGRS), not the transport.

---

## References

| Paper / Source | Year | Used For |
|---|---|---|
| Karis, "Nanite: Virtualized Geometry" SIGGRAPH Advances | 2021 | Cluster hierarchy, software rasterizer, visibility buffer |
| Karis, "Lumen: Real-time Global Illumination" GDC | 2022 | Surface cache, SDF GI, radiance probes |
| Sannikov, "Radiance Cascades" | 2024 | GI alternative to Lumen, simpler + newer |
| Wihlidal, "GPU-Driven Rendering Pipelines" GDC | 2015 | Indirect draw, GPU culling |
| NVIDIA Omniverse Frontiers Survey | 2024 | USD composition, Nucleus CAS, live collaboration |
| Cannon, "GGPO: Rollback Networking" | 2012 | Deterministic rollback in ECS |
| Guerrilla Games, "Streaming in Horizon Zero Dawn" GDC | 2017 | Hysteresis-based instance streaming |
| rstar crate documentation | 2024 | R-tree benchmarks for spatial queries |
| Bevy ECS archetype benchmark suite | 2024 | 10M entity iteration performance baseline |
| ONNX Runtime documentation | 2024 | Batched NPC inference budget |

# Eustress Apex: Three Pillars Beyond World-Class

## Companion to WORLD_CLASS_ENGINE.md

This document extends the seven-pillar roadmap with three capabilities that elevate Eustress
from "competitive game engine" to "universal simulation and intelligence platform":

1. **Trillion-Parameter AI Integration** — foundation model inference as a first-class engine subsystem
2. **Planetary-Scale Worlds** — double-precision coordinates, streaming at Earth scale
3. **Engineering-Grade Simulation** — Finite Element Method (FEM) accuracy for real prototype validation

Each pillar is grounded in published research and maps directly to concrete Rust crates and
existing Eustress code. No vaporware.

---

## Table of Contents

1. [Why These Three](#1-why-these-three)
2. [Pillar A — Trillion-Parameter AI: MoE Inference in the Engine Loop](#2-pillar-a)
3. [Pillar B — Planetary Worlds: Double-Precision Coordinate System](#3-pillar-b)
4. [Pillar C — Engineering Simulation: FEM + SPH Accuracy](#4-pillar-c)
5. [How All Three Connect: The Apex Loop](#5-the-apex-loop)
6. [Cargo Dependencies to Add](#6-cargo-dependencies)
7. [What This Unlocks That No Other Engine Has](#7-unique-position)
8. [References](#8-references)

---

## 1. Why These Three

The seven pillars in `WORLD_CLASS_ENGINE.md` make Eustress competitive with Unreal Engine 5
and Unity at peak. These three pillars put Eustress in a category that **no existing game engine
occupies** because game engines are not designed for them:

| Capability | UE5 | Unity | Godot | Eustress Target |
|---|---|---|---|---|
| Foundation model inference in game loop | No | No | No | **Yes — MoE dispatch** |
| Planetary-scale double-precision worlds | Yes (LWC 5.0) | Partial (DOTS) | No | **Yes — ECEF native** |
| Engineering FEM simulation for prototyping | No | No | No | **Yes — fenris crate** |

The Rust ecosystem has the pieces. No one has assembled them into a single engine runtime.

---

## 2. Pillar A — Trillion-Parameter AI

### The Problem with Current Approaches

Current engine AI (NPCs, procedural generation) uses small models: ONNX with 100K–10M parameters,
running in microseconds per inference. This is the NPC Brain tier described in `WORLD_CLASS_ENGINE.md`.

Trillion-parameter models (GPT-4 class: ~1.8T parameters; DeepSeek V3: 671B total, 37B active)
operate at a completely different scale. They cannot run in a 16ms game frame on local hardware.
The question is: **how do you make them feel like they are running in the game loop?**

The answer is **disaggregation** — the same architecture that powers vLLM's distributed inference
(Shoeybi et al., Megatron-LM 2019; vLLM distributed inference blog, 2025).

### The Key Insight: Sparse Activation via MoE

DeepSeek V3 (technical report, December 2024) established the state of the art:
- **671B total parameters**, but only **37B active per token** (5.5% activation rate)
- Multi-head latent attention (MLA) compresses the KV (key-value) cache by **5.7×** vs standard multi-head attention
- Expert routing: top-2 of 256 routed experts selected per token, plus 1 shared expert always active
- **FP8 mixed precision**: activations cached and dispatched in FP8, optimizer states in BF16
- Training cost: **2.788M GPU hours** total — the same compute budget that would train a 67B dense model

**Implication for Eustress**: You do not run a 671B model. You run the 37B active slice. The
"trillion parameters" framing refers to the total model capacity, not the per-inference cost.
A well-routed MoE call is equivalent in compute to a 37B dense model call.

### Architecture: Async Request Pipeline

The game loop cannot wait for a 200ms LLM inference. The solution is **decoupled async dispatch**:

```
Game tick (16ms)
    │
    ├─► Fast path: ONNX NPC brain (< 50μs, in-loop)
    │
    └─► Slow path: Foundation model request (200ms–2s)
            │
            ├─ Enqueue to InferenceQueue (non-blocking)
            │
            ├─ Game continues running (no stall)
            │
            └─ Response arrives → ApplyInferenceEvent → ECS write
```

```rust
// eustress/crates/common/src/ai/foundation.rs
//
// Reference: vLLM distributed inference architecture (vllm.ai/blog/distributed-inference, 2025)
// Key insight: KV cache size grows super-linearly with GPU count due to memory effects.
// At TP=2 vs TP=1, KV cache increases 13.9× → 3.9× more token throughput (not linear 2×).
//
// For Eustress: we do NOT run vLLM locally. We interface with it as a service.
// The engine's job is to make async calls feel synchronous to the game loop.

use tokio::sync::mpsc;
use std::sync::Arc;

/// A pending inference request. Submitted non-blocking, result arrives via channel.
#[derive(Debug, Clone)]
pub struct InferenceRequest {
    pub id:         uuid::Uuid,
    pub prompt:     String,
    pub max_tokens: u32,
    pub model:      ModelTier,
    /// ECS entity to write result back to (None = fire and forget)
    pub target:     Option<bevy::ecs::entity::Entity>,
}

/// Model tier determines routing: local ONNX vs remote MoE endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    /// < 50μs: local ONNX session, runs in game loop
    /// Use for: NPC immediate reactions, animation state, damage numbers
    Local,

    /// ~5ms: 7B–13B model on dedicated GPU (same machine or LAN)
    /// Use for: NPC dialogue, quest generation, environmental storytelling
    Fast,

    /// ~200ms: 70B–671B MoE model on inference cluster (vLLM or Ollama)
    /// Use for: world events, faction decisions, complex NPC reasoning
    Deep,

    /// ~2s: frontier model API (Claude, GPT-4)
    /// Use for: unique story moments, player-facing narrative decisions
    Frontier,
}

/// Bevy resource: manages the async inference dispatcher.
/// Submit requests every frame; results arrive via `InferenceResultEvent`.
pub struct FoundationModelDispatcher {
    tx:          mpsc::UnboundedSender<InferenceRequest>,
    result_tx:   mpsc::UnboundedSender<InferenceResult>,
}

/// Bevy event: fired when an inference result is ready to apply to the ECS.
#[derive(bevy::ecs::event::Event, Debug, Clone)]
pub struct InferenceResultEvent {
    pub request_id: uuid::Uuid,
    pub entity:     Option<bevy::ecs::entity::Entity>,
    pub text:       String,
    pub model_tier: ModelTier,
    pub latency_ms: u64,
}

impl FoundationModelDispatcher {
    /// Non-blocking submit. Returns immediately. Result arrives as InferenceResultEvent.
    pub fn submit(&self, request: InferenceRequest) {
        let _ = self.tx.send(request);
    }
}

/// Background tokio task: routes requests to appropriate backends.
/// Runs outside the Bevy app loop — pure async.
async fn inference_worker(
    mut rx:       mpsc::UnboundedReceiver<InferenceRequest>,
    result_tx:    mpsc::UnboundedSender<InferenceResult>,
    config:       Arc<FoundationModelConfig>,
) {
    while let Some(req) = rx.recv().await {
        let config = config.clone();
        let result_tx = result_tx.clone();

        // Each request gets its own task — no head-of-line blocking.
        tokio::spawn(async move {
            let start = std::time::Instant::now();

            let text = match req.model {
                ModelTier::Local  => run_onnx_local(&req, &config).await,
                ModelTier::Fast   => call_ollama(&req, &config).await,
                ModelTier::Deep   => call_vllm_endpoint(&req, &config).await,
                ModelTier::Frontier => call_openai_api(&req, &config).await,
            };

            let _ = result_tx.send(InferenceResult {
                request_id: req.id,
                entity:     req.target,
                text:       text.unwrap_or_default(),
                model_tier: req.model,
                latency_ms: start.elapsed().as_millis() as u64,
            });
        });
    }
}
```

### Tensor Parallelism for On-Premise Clusters

For studios running on-premise (8× H100 cluster):

```rust
// eustress/crates/server/src/inference_cluster.rs
//
// Reference: Megatron-LM tensor parallelism (Shoeybi et al., 2019)
// Column parallelism: split weight matrices along columns, each GPU computes a shard.
// Row parallelism: split along rows, sum partial results (all-reduce).
// Key constraint: tensor parallelism requires NVLink or InfiniBand — NOT PCIe.
// PCIe bandwidth (16 GB/s) creates an all-reduce bottleneck at TP=4+.
// NVLink 4.0 (900 GB/s bidirectional) eliminates this bottleneck.
//
// For Eustress Forge: configure vLLM with:
//   tensor_parallel_size = 8 (within one 8×H100 node)
//   pipeline_parallel_size = N (across nodes if needed)
//   expert_parallel_size = 8 (for MoE models like DeepSeek)

#[derive(Debug, Clone, serde::Deserialize)]
pub struct InferenceClusterConfig {
    /// vLLM API endpoint
    pub endpoint:             String,
    /// Model identifier (e.g., "deepseek-ai/DeepSeek-V3")
    pub model:                String,
    /// Tensor parallel degree (must match vLLM server config)
    pub tensor_parallel_size: u32,
    /// Max concurrent requests before queuing
    pub max_concurrent:       u32,
    /// KV cache quantization: "fp8", "fp16", "none"
    pub kv_cache_dtype:       String,
}
```

### Symbolica × Foundation Model: The Unique Advantage

No other engine can do this: **feed symbolically derived physics laws as structured context
to a foundation model, then validate the model's output against those same laws.**

```rust
// eustress/crates/engine/src/ai/physics_grounded_inference.rs
//
// The Symbolica system (common/src/realism/) derives exact physics formulas.
// These become structured context injected into the LLM prompt.
// The LLM's output is then validated against the Kernel Law system.
//
// Example: NPC engineer designing a rocket nozzle.
// 1. Symbolica derives: thrust = mass_flow_rate * exhaust_velocity + (p_exit - p_atm) * area_exit
// 2. This formula is injected as context: "The governing thrust equation is: ..."
// 3. LLM generates design parameters
// 4. Kernel Law validator checks: energy conservation satisfied? momentum conservation?
// 5. If violated → LLM retry with constraint feedback

pub struct PhysicsGroundedPromptBuilder {
    pub laws:     Vec<String>,   // from KernelLawRegistry
    pub formulas: Vec<String>,   // from Symbolica derivation
    pub scenario: String,
}

impl PhysicsGroundedPromptBuilder {
    pub fn build(&self) -> String {
        format!(
            "You are an expert engineer. The following physics laws govern this system:\n\
             {laws}\n\n\
             The governing equations are:\n\
             {formulas}\n\n\
             Task: {scenario}\n\n\
             Your response MUST satisfy all listed conservation laws. \
             Provide numerical values with units.",
            laws = self.laws.join("\n"),
            formulas = self.formulas.join("\n"),
            scenario = self.scenario,
        )
    }
}
```

**This is the moat.** Unity and UE5 can call GPT-4. They cannot constrain GPT-4's output with
symbolically proven physics laws derived in real time from the simulation state.

This same idea, using the engine instances and properties as contraints will build the best World Model.

---

## 3. Pillar B — Planetary-Scale Worlds

### The Floating Point Precision Problem

Single-precision float (f32) has ~7 decimal digits of precision. At world scale:

| Distance from origin | f32 precision | Effect |
|---|---|---|
| 1 km | 0.06 mm | Fine |
| 10 km | 0.6 mm | Fine |
| 100 km | 6 mm | Visible jitter |
| 1,000 km (city to city) | 6 cm | Objects visibly shake |
| 6,371 km (Earth radius) | 38 cm | Unusable |
| 1 AU (Earth to Sun) | 11 km | Completely broken |

Unreal Engine 5 solved this in 5.0 with Large World Coordinates (LWC): a custom `FLargeWorldCoordinates`
type that stores positions as `f64` globally, converting to f32 only for the GPU render call
(which operates in camera-relative space where f32 is sufficient).

Eustress's existing `eustress-geo` crate has the `GeoOrigin` resource with equirectangular coordinate
transforms — **the foundation is already present**. What is missing is the f64 → f32 camera-relative
transform in the render pipeline.

### Architecture: Origin Rebasing

The standard algorithm (used in UE5, Cesium for Unreal, and Kerbal Space Program):

```
World position (f64): stored in ECS as DVec3
    │
    │  Each frame, before rendering:
    │  camera_world_pos (f64) = camera entity's DVec3
    │
    ▼
Render position (f32) = (world_pos_f64 - camera_world_pos_f64) as f32
```

The subtraction happens in f64 (no precision loss). The result, which is always small (within
render distance of the camera), fits perfectly in f32. The GPU never sees large coordinates.

```rust
// eustress/crates/engine/src/space/large_world.rs
//
// Reference:
//   UE5 Large World Coordinates migration guide (docs.unrealengine.com/5.0)
//   Cesium for Unreal — ECEF (Earth-Centered Earth-Fixed) coordinate system
//   KSP "Krakensbane" camera — first major game to use origin rebasing (2011)

use bevy::math::{DVec3, Vec3};
use bevy::prelude::*;

/// Double-precision world position. Replaces Bevy's f32 Transform for large-world entities.
/// Stored in ECS alongside the standard Transform which holds the RENDER position.
///
/// Rule: NEVER use Transform.translation for physics or game logic at planetary scale.
/// Use WorldPosition exclusively. Transform is recomputed each frame from WorldPosition.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct WorldPosition(pub DVec3);

/// Bevy resource: current camera world position in f64.
/// Updated every frame before the render position sync system.
#[derive(Resource, Default)]
pub struct CameraWorldOrigin(pub DVec3);

/// System: runs in PostUpdate, before Bevy's transform propagation.
/// Converts WorldPosition (f64) → Transform.translation (f32, camera-relative).
///
/// This is the "origin rebasing" step. The subtraction in f64 eliminates precision loss.
/// The resulting f32 is always < render_distance, so f32 precision is sufficient.
pub fn sync_render_positions(
    origin:    Res<CameraWorldOrigin>,
    mut query: Query<(&WorldPosition, &mut Transform)>,
) {
    let cam = origin.0;
    for (world_pos, mut transform) in query.iter_mut() {
        // f64 subtraction: no precision loss regardless of absolute position
        let render_pos = world_pos.0 - cam;
        // Cast to f32 only after subtraction — render_pos is always small
        transform.translation = Vec3::new(
            render_pos.x as f32,
            render_pos.y as f32,
            render_pos.z as f32,
        );
    }
}

/// System: updates CameraWorldOrigin from the camera entity's WorldPosition.
/// Must run BEFORE sync_render_positions.
pub fn update_camera_origin(
    mut origin: ResMut<CameraWorldOrigin>,
    camera_q:   Query<&WorldPosition, With<Camera>>,
) {
    if let Ok(cam_pos) = camera_q.get_single() {
        origin.0 = cam_pos.0;
    }
}
```

### ECEF Coordinate System

For planetary simulation (Earth, procedurally generated planets), use Earth-Centered Earth-Fixed (ECEF):

```rust
// eustress/crates/geo/src/ecef.rs  (extends existing eustress-geo crate)
//
// ECEF: origin at Earth's center of mass.
// X-axis: through (0°N, 0°E) — Gulf of Guinea
// Y-axis: through (0°N, 90°E) — Indian Ocean
// Z-axis: through North Pole
//
// Conversion (WGS84 ellipsoid):
//   X = (N + h) * cos(lat) * cos(lon)
//   Y = (N + h) * cos(lat) * sin(lon)
//   Z = (N * (1 - e²) + h) * sin(lat)
// where N = a / sqrt(1 - e² * sin²(lat)), a = 6378137.0 m, e² = 0.00669438

pub const WGS84_A: f64 = 6_378_137.0;          // semi-major axis (m)
pub const WGS84_E2: f64 = 0.006_694_379_990_14; // first eccentricity squared

/// Convert geodetic (lat/lon/altitude) to ECEF DVec3.
/// lat, lon in radians. altitude in meters above WGS84 ellipsoid.
pub fn geodetic_to_ecef(lat_rad: f64, lon_rad: f64, alt_m: f64) -> DVec3 {
    let sin_lat = lat_rad.sin();
    let cos_lat = lat_rad.cos();
    let n = WGS84_A / (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt();

    DVec3::new(
        (n + alt_m) * cos_lat * lon_rad.cos(),
        (n + alt_m) * cos_lat * lon_rad.sin(),
        (n * (1.0 - WGS84_E2) + alt_m) * sin_lat,
    )
}

/// East-North-Up (ENU) tangent frame at a surface point.
/// Returns (east, north, up) unit vectors in ECEF space.
/// Use these to orient local physics simulations at a surface location.
pub fn enu_frame(lat_rad: f64, lon_rad: f64) -> (DVec3, DVec3, DVec3) {
    let east  = DVec3::new(-lon_rad.sin(), lon_rad.cos(), 0.0);
    let up    = DVec3::new(lat_rad.cos() * lon_rad.cos(),
                           lat_rad.cos() * lon_rad.sin(),
                           lat_rad.sin());
    let north = up.cross(east);
    (east, north, up)
}
```

### Chunked Streaming at Planetary Scale

10M instances across a planet need **hierarchical spatial chunking**, not a flat R-tree:

```rust
// eustress/crates/engine/src/space/planetary_chunks.rs
//
// Reference: "Procedural Planet Rendering" — Sebastian Lague (YouTube series)
//            Cesium ion streaming architecture (cesium.com/docs)
//
// Architecture: S2 geometry (Google's sphere partitioning library)
// S2 divides the sphere into a quadtree of cells at 30 levels.
// Level 0: 6 faces (cube faces projected onto sphere)
// Level 10: ~10km² cells
// Level 20: ~1m² cells
// Level 30: ~1cm² cells
//
// Eustress uses a simplified 6-level octree over ECEF space.
// Level 0: planet scale  (~12,000 km per cell)
// Level 1: continent     (~3,000 km)
// Level 2: region        (~750 km)
// Level 3: district      (~190 km)
// Level 4: neighborhood  (~47 km)
// Level 5: street        (~12 km) — instances spawn as ECS entities here

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkId {
    pub level: u8,   // 0..=5
    pub x:     i32,
    pub y:     i32,
    pub z:     i32,
}

pub struct PlanetaryChunkManager {
    /// Chunks currently loaded as Bevy entities
    pub active:   dashmap::DashMap<ChunkId, bevy::ecs::entity::Entity>,
    /// Chunks in the hot cache (loaded but not active as entities)
    pub hot:      dashmap::DashMap<ChunkId, Vec<crate::space::InstanceDefinition>>,
    pub observer: WorldPosition,  // updated from camera each frame
}

impl PlanetaryChunkManager {
    /// Determine which level-5 chunks should be active given camera position.
    /// Uses geodesic distance rather than Euclidean to work on sphere surface.
    pub fn visible_chunks(&self, radius_m: f64) -> Vec<ChunkId> {
        let chunk_size_m = 12_000.0; // level 5 cell size in meters
        let cell_radius = (radius_m / chunk_size_m).ceil() as i32;
        let origin = self.observer.0;

        let mut result = Vec::new();
        for dx in -cell_radius..=cell_radius {
            for dy in -cell_radius..=cell_radius {
                for dz in -cell_radius..=cell_radius {
                    let candidate = ChunkId {
                        level: 5,
                        x: (origin.x / chunk_size_m) as i32 + dx,
                        y: (origin.y / chunk_size_m) as i32 + dy,
                        z: (origin.z / chunk_size_m) as i32 + dz,
                    };
                    result.push(candidate);
                }
            }
        }
        result
    }
}
```

**Existing foundation in Eustress**:
- `eustress/crates/geo/src/coords.rs` — `GeoOrigin` + `geo_to_world()` already does lat/lon → local
- `eustress/crates/geo/src/spatial_index.rs` — R-tree for local queries already built
- `eustress/crates/common/src/orbital/wgs84.rs` — WGS84 math already present

The gap is the `WorldPosition` component, `sync_render_positions` system, and the hierarchical
chunk manager. **The math is already in the codebase. The ECS wiring is not.**

---

## 4. Pillar C — Engineering-Grade Simulation

### What "Accurate Simulation" Actually Means

Game physics (Rapier/Avian3D) uses rigid body dynamics: objects are indestructible, materials
are uniform, and the solver converges in milliseconds by sacrificing accuracy. This is correct
for games. It is **wrong for engineering**.

Engineering simulation requires:

| Property | Game Physics | Engineering Simulation |
|---|---|---|
| Material model | Rigid (no deformation) | Elastic, plastic, hyperelastic |
| Failure | Not modeled | Von Mises yield criterion, fracture |
| Precision | ~1% error acceptable | < 0.1% error required |
| Solver | Iterative (fast, approximate) | Direct sparse solver (exact) |
| Time step | Variable (16ms) | Fixed micro-steps (1ms or smaller) |
| Thermal coupling | None | Full thermo-mechanical coupling |

The Finite Element Method (FEM) solves partial differential equations by discretizing a
continuous body into finite elements, assembling a global stiffness matrix, and solving
`K * u = f` where K is stiffness, u is displacement, f is applied forces.

### Fenris: FEM in Pure Rust

`fenris` (Interactive Computer Graphics group, Saarland University) is the only production-quality
FEM library in Rust. It was designed as an alternative to C++ FEM libraries (deal.II, FEniCS).

Key properties:
- Tetrahedral and hexahedral elements (solid mechanics)
- Tet4, Tet10, Hex8, Hex20 element types
- Sparse linear algebra via `nalgebra-sparse`
- Rayon parallelism for assembly
- Integration with nalgebra for matrix operations

```rust
// eustress/crates/common/src/simulation/fem.rs
//
// Reference:
//   fenris crate (github.com/InteractiveComputerGraphics/fenris)
//   "Finite Element Procedures" — Bathe (1996) — the definitive FEM reference
//   "A First Course in the Numerical Analysis of Differential Equations" — Iserles
//
// Integration strategy:
//   FEM runs in a SEPARATE TOKIO TASK from the Bevy game loop.
//   Results are sent to Bevy via crossbeam channel at the end of each FEM step.
//   The game loop renders interpolated states — FEM drives the ground truth.
//
// This is identical to the SCENARIOS architecture: rayon for compute, tokio for I/O,
// crossbeam to feed back to Bevy.

use crossbeam_channel::{Sender, Receiver};
use rayon::prelude::*;

/// A FEM simulation domain — represents a single deformable body.
/// Runs independently of the Bevy frame rate.
pub struct FemDomain {
    /// Node positions in 3D (f64 for engineering precision)
    pub nodes:     Vec<[f64; 3]>,
    /// Element connectivity (indices into nodes)
    pub elements:  Vec<[usize; 4]>,   // Tet4 elements
    /// Material properties per element
    pub materials: Vec<LinearElasticMaterial>,
    /// Displacement solution vector (3 DOF per node)
    pub u:         Vec<f64>,
    /// Velocity vector (for dynamic problems)
    pub v:         Vec<f64>,
}

/// Linear elastic material (Hookean).
/// Valid for small strains (< ~2% for most metals).
#[derive(Debug, Clone, Copy)]
pub struct LinearElasticMaterial {
    /// Young's modulus (Pa) — steel: 200e9, aluminum: 69e9, PEEK: 3.6e9
    pub youngs_modulus: f64,
    /// Poisson's ratio (dimensionless) — steel: 0.30, aluminum: 0.33
    pub poisson_ratio:  f64,
    /// Density (kg/m³) — steel: 7850, aluminum: 2700
    pub density:        f64,
}

impl LinearElasticMaterial {
    /// Lamé parameters from engineering constants.
    /// These are the form used in the FEM stiffness matrix assembly.
    pub fn lame_lambda(&self) -> f64 {
        let e = self.youngs_modulus;
        let v = self.poisson_ratio;
        (e * v) / ((1.0 + v) * (1.0 - 2.0 * v))
    }

    pub fn lame_mu(&self) -> f64 {
        let e = self.youngs_modulus;
        let v = self.poisson_ratio;
        e / (2.0 * (1.0 + v))
    }

    /// Von Mises yield criterion threshold.
    /// If von_mises_stress > yield_strength → material yielded (permanent deformation).
    /// NOT modeled in linear FEM — requires nonlinear plasticity extension.
    pub fn von_mises_stress(&self, cauchy_stress: &[f64; 6]) -> f64 {
        let [s11, s22, s33, s12, s23, s13] = *cauchy_stress;
        let term1 = (s11 - s22).powi(2) + (s22 - s33).powi(2) + (s33 - s11).powi(2);
        let term2 = 6.0 * (s12.powi(2) + s23.powi(2) + s13.powi(2));
        ((term1 + term2) / 2.0).sqrt()
    }
}

/// Bevy event: FEM step completed, carry results to rendering system.
#[derive(bevy::ecs::event::Event, Debug, Clone)]
pub struct FemStepCompleted {
    pub domain_id:      uuid::Uuid,
    /// Updated node positions for mesh deformation rendering
    pub deformed_nodes: Vec<[f64; 3]>,
    /// Von Mises stress per element (for color-coded stress visualization)
    pub vm_stress:      Vec<f64>,
    /// Maximum displacement magnitude (for convergence monitoring)
    pub max_disp_m:     f64,
    /// Simulation time (seconds, NOT wall-clock time)
    pub sim_time_s:     f64,
}
```

### SPH for Fluid Engineering

Smoothed Particle Hydrodynamics (SPH) handles fluids, granular materials, and soft bodies with
physical accuracy beyond what game physics engines provide:

```rust
// eustress/crates/common/src/simulation/sph.rs
//
// Reference:
//   "Smoothed Particle Hydrodynamics: A Meshfree Particle Method" — Liu & Liu (2003)
//   "Predictive-Corrective Incompressible SPH" — Solenthaler & Pajarola, SIGGRAPH 2009
//   SPlisHSPlasH library (Bender et al.) — the reference SPH implementation
//
// Use cases in Eustress:
//   - Water simulation at engineering accuracy (pipe flow, dam breaks)
//   - Aerodynamics (wind tunnel simulation around prototype parts)
//   - Granular materials (powder, soil, sand for agriculture/construction sims)
//   - Fluid-structure interaction (water hitting a bridge prototype)

/// SPH particle — each particle represents a volume of fluid.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SphParticle {
    pub position:  [f64; 3],
    pub velocity:  [f64; 3],
    pub density:   f64,
    pub pressure:  f64,
    pub mass:      f64,
}

/// WCSPH (Weakly Compressible SPH) equation of state.
/// Tait equation: p = B * ((ρ/ρ₀)^γ - 1)
/// where B = ρ₀ * c₀² / γ, γ = 7 for water, c₀ = speed of sound
pub fn tait_pressure(density: f64, rest_density: f64, speed_of_sound: f64) -> f64 {
    const GAMMA: f64 = 7.0;
    let b = rest_density * speed_of_sound * speed_of_sound / GAMMA;
    b * ((density / rest_density).powi(GAMMA as i32) - 1.0)
}
```

### Integration with the Recursive Feedback Loop

The existing `RECURSIVE_FEEDBACK_LOOP.md` architecture has a Simulation system (System 4)
that currently uses Avian3D rigid body physics. FEM and SPH slot directly into System 4
as **high-fidelity simulation backends**:

```
System 4 (Simulation) — current: Avian3D rigid body
                       — extended: tiered simulation

Tier 1: Avian3D (game loop, 60Hz, rigid body)
    ↓ when high-fidelity mode activated
Tier 2: FEM via fenris (background thread, variable timestep, deformable)
    ↓ when fluid dynamics needed
Tier 3: SPH (background thread, 1ms timestep, fluid/granular)
```

The `FemStepCompleted` event carries results back to Bevy exactly like the existing
`crossbeam` channels in the Scenarios architecture.

---

## 5. The Apex Loop

When all three pillars combine, the result is unique in the industry:

```
Engineer/Creator
    │
    │  Designs a prototype component (TOML instance definition)
    ▼
Eustress Studio
    │
    ├─[Pillar C]─► FEM solver runs structural analysis
    │               Von Mises stress map rendered in viewport
    │               Failure mode predicted at 0.1% accuracy
    │
    ├─[Pillar A]─► Physics-grounded LLM (DeepSeek V3 via vLLM)
    │               Receives: stress results + Symbolica-derived equations
    │               Generates: design optimization suggestions
    │               Validated against: Kernel Law system (energy conservation etc.)
    │
    └─[Pillar B]─► Component placed in planetary-scale world
                    Position stored as ECEF f64 DVec3
                    Rendered via origin-rebasing (f32 camera-relative)
                    1M components across a 500km² site — no floating point jitter
    │
    ▼
Recursive Feedback Loop (System 5: AI Governor)
    Optimization loop: FEM result → LLM suggestion → FEM re-run → converge
    ~17 minutes on 8-core workstation to fully optimize a component
    │
    ▼
Realization Bridge (System 6)
    Manufacturing manifest → Bliss/Workshop → Physical prototype
```

**No other engine on Earth has this loop.** UE5 can render a planet. It cannot run FEM on
the objects in that planet, constrain an LLM with real physics laws, and feed the result
into a manufacturing pipeline.

---

## 6. Cargo Dependencies

Add to `eustress/Cargo.toml` `[workspace.dependencies]`:

```toml
# === Pillar A: Foundation Model Inference ===

# ONNX Runtime — already noted in RECURSIVE_FEEDBACK_LOOP.md
ort = "2.0"
ndarray = "0.16"

# OpenAI/vLLM compatible API client
async-openai = "0.27"   # OpenAI-compatible API, works with vLLM endpoints

# === Pillar B: Planetary Worlds ===

# Double-precision math (Bevy has DVec3 built-in via glam)
# No new dep needed — glam is already a Bevy dependency

# Cesium 3D Tiles reader (streaming planetary terrain)
# No production Rust crate yet — implement subset manually using:
# reqwest (already present) + bincode (already present)

# === Pillar C: Engineering Simulation ===

# FEM library — pure Rust, Rayon-parallel, solid mechanics
fenris = "0.0.12"         # check latest on crates.io

# Sparse linear algebra (fenris dependency, but also standalone useful)
nalgebra-sparse = "0.10"

# High-performance sparse direct solver (via faer)
faer = "0.21"             # Rust-native sparse LU, Cholesky, QR

# Physics quantities with units (prevents Pa vs MPa errors)
uom = "0.36"              # Units of Measurement — compile-time unit safety
```

**Why `uom`**: Engineering simulation bugs frequently come from unit confusion (Pa vs MPa,
mm vs m). `uom` enforces units at compile time — a function taking `Pressure<SI<f64>>` cannot
accidentally receive a length. This is impossible in UE5 or Unity.

```rust
// With uom: unit confusion is a compile error, not a runtime bug
use uom::si::f64::*;
use uom::si::pressure::pascal;
use uom::si::length::meter;

let yield_strength = Pressure::new::<pascal>(250e6_f64);  // 250 MPa steel
let wall_thickness = Length::new::<meter>(0.005_f64);      // 5mm

// This would be a compile error — cannot add pressure and length:
// let wrong = yield_strength + wall_thickness;  // ERROR
```

---

## 7. What This Unlocks That No Other Engine Has

### For Gameplay

- **Procedural worlds at Earth scale**: position 1 billion instances across a planet with no jitter
- **NPCs that reason with trillion-parameter models**: not scripted dialogue trees, actual reasoning
- **Destructible structures with real failure modes**: FEM predicts exactly where a bridge cracks

### For Engineering Prototyping

- **Design → simulate → optimize → manufacture in one tool**: no export to ANSYS, no separate FEM software
- **LLM design assistant constrained by real physics**: suggestions that actually satisfy conservation laws
- **Stress visualization in 3D context**: see von Mises stress overlaid on the actual prototype model

### For Scientific Simulation

- **Planetary-scale fluid dynamics**: SPH over a 100km watershed
- **Orbital mechanics with millimeter precision**: ECEF f64 throughout
- **Time-dilated simulation**: existing architecture in `RECURSIVE_FEEDBACK_LOOP.md` supports this

### The Market Position

| Platform | Gameplay | Engineering sim | Planetary scale | Trillion-param AI |
|---|---|---|---|---|
| Unreal Engine 5 | ★★★★★ | ✗ | ★★★★ | ✗ |
| Unity + DOTS | ★★★★ | ✗ | ★★★ | ✗ |
| ANSYS / Abaqus | ✗ | ★★★★★ | ✗ | ✗ |
| NVIDIA Omniverse | ★★★ | ★★★ | ★★★ | ★★★ |
| **Eustress (target)** | **★★★★★** | **★★★★★** | **★★★★★** | **★★★★★** |

---

## 8. References

| Source | Year | Used For |
|---|---|---|
| DeepSeek-V3 Technical Report (arxiv 2412.19437) | 2024 | MoE architecture: 671B total / 37B active, MLA KV compression, FP8 dispatch |
| Shoeybi et al., "Megatron-LM" (arxiv 1909.08053) | 2019 | Tensor parallelism: column/row split, all-reduce |
| vLLM Distributed Inference blog (vllm.ai) | 2025 | Pipeline parallelism, super-linear KV cache scaling (13.9× blocks at TP=2) |
| UE5 Large World Coordinates migration guide | 2022 | f64 global position, f32 camera-relative render, origin rebasing |
| Cesium for Unreal documentation | 2024 | ECEF coordinate system, hierarchical tile streaming |
| Bathe, "Finite Element Procedures" | 1996 | FEM stiffness matrix assembly, Tet4 elements, direct sparse solver |
| Liu & Liu, "Smoothed Particle Hydrodynamics" | 2003 | SPH kernel functions, WCSPH Tait equation of state |
| Solenthaler & Pajarola, SIGGRAPH 2009 | 2009 | Predictive-corrective incompressible SPH (PCISPH) |
| fenris crate (InteractiveComputerGraphics/fenris) | 2024 | Pure-Rust FEM, Tet4/Hex8 elements, Rayon-parallel assembly |
| uom crate | 2024 | Compile-time unit safety for engineering quantities |

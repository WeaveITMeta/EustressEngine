//! Merged-cell HLOD (hierarchical level-of-detail) — the whole-map render.
//!
//! ## The problem this solves
//!
//! [`super::residency`] keeps the live ECS set bounded by spawning every
//! visible binary part (`BinaryEcsInstance`) as a full Bevy entity inside
//! `load_radius` (~350 m) and despawning everything beyond it. That makes
//! per-frame cost O(live count) — Bevy `check_visibility`, `extract_meshes`,
//! the change-queue / lighting mirrors all iterate the whole resident set —
//! AND it hard-caps render distance at `load_radius`, so the rest of a huge
//! map (Vehicle Simulator ≈ 161 K parts) simply never draws.
//!
//! HLOD is what Unreal HLOD / Roblox streaming-proxies do: a FAR Morton cell
//! renders as ONE merged mesh (a "proxy") instead of its thousands of
//! individual entities. The far district becomes a single draw, the live
//! entity count collapses to just the NEAR ring's parts, and the whole map
//! is visible out to `hlod_radius`.
//!
//! ## Division of labour (with residency)
//!
//! | Ring | Distance | Who renders it | Editable? |
//! |------|----------|----------------|-----------|
//! | NEAR | ≤ `load_radius` | residency spawns individual parts | yes |
//! | FAR  | `load_radius` … `hlod_radius` | THIS module: 1 proxy / cell | no (pure render) |
//! | beyond | > `hlod_radius` | nothing | — |
//!
//! The boundary is shared cell math (the SAME `chunk_size = 256` Morton grid
//! residency loads in), so a cell's geometry is EITHER drawn as individuals OR
//! as one proxy, never both. When a cell crosses far→near its proxy is
//! **hidden** (residency then spawns its individuals); near→far **re-shows**
//! it. We **suppress (hide) HLOD on the cells residency's keep-box owns** so
//! the two never overlap even through the hysteresis band.
//!
//! ## Lifecycle: BUILD-ONCE / PERSIST / VISIBILITY-TOGGLE
//!
//! The proxy set is derived from the DB's ACTUAL non-empty cells, not from a
//! camera-following box swept over the (mostly empty) `hlod_radius` volume:
//!
//! 1. **Enumerate once.** On Space load, one background task reads every
//!    instance core ([`active_db::iter_instance_cores`] — the same eager
//!    snapshot the boot-load uses), derives each core's Morton cell from its
//!    stored translation, and returns the DISTINCT set of non-empty cells
//!    (a few hundred for a real map, vs ~290 K cells in the empty box). The
//!    main thread only polls + drains that `Vec<Cell>` into the build queue.
//! 2. **Build each cell once, then persist forever.** Each enumerated cell
//!    gets ONE merge task (the SAME single-cell region scan +
//!    [`build_merged_mesh`] on [`AsyncComputeTaskPool`], capped); when it
//!    finishes the proxy is spawned (capped). A proxy is **never despawned
//!    for camera position** — once built it lives for the Space session. A
//!    few hundred frustum-culled proxies are cheap; Bevy's auto-`Aabb`
//!    `check_visibility` hides off-screen ones for free. The ONLY despawn
//!    path is the Space-change teardown.
//! 3. **Near-overlap is handled by VISIBILITY, not despawn.** A cheap
//!    cadence-gated system over the proxy set (a few hundred entities) toggles
//!    each proxy's [`Visibility`]: `Hidden` when its cell is inside
//!    residency's keep-box (`evict_radius` — residency owns it, drawing
//!    individuals), `Inherited` otherwise. Toggling in place means a cell
//!    going near hides INSTANTLY and going far shows INSTANTLY — zero rebuild,
//!    zero gap, no z-fight.
//!
//! So the whole map stays rendered as the camera moves anywhere (proxies
//! persist; only the frustum hides off-screen ones), steady-state Update is
//! back to near-ring cost (no per-frame O(map) walk — the plan/build systems
//! idle once every enumerated cell is built), and the no-double-render
//! invariant holds (hidden proxy ⇔ residency owns the cell).
//!
//! ## v1 merge scope (conservative — ship the whole-map render, defer polish)
//!
//! - Merge ONLY primitive shapes (`parts/*.glb`: block / ball / cylinder /
//!   wedge / corner_wedge / cone). A part with a custom/glb mesh
//!   ([`super::representation::mesh_requires_filesystem`]) or transparency > 0
//!   is SKIPPED from the merge (omitted from the proxy, never crashes).
//! - One opaque [`StandardMaterial`] with per-vertex colour
//!   (`Mesh::ATTRIBUTE_COLOR`) — no textures / no transparency at distance.
//! - The merge (decode + geometry append for ~hundreds–thousands of parts per
//!   cell) runs on [`AsyncComputeTaskPool`] so it never stalls the main
//!   thread; a poll system spawns the proxy once the mesh is finished.
//!
//! A proxy carries NO `BinaryEcsInstance` / `ColdStreamed` / `Collider` /
//! `Instance` / `Name` — it is render-only, not an editable part, and emits
//! no scene deltas (the change-queue keys on `Name`/`Instance`, which a proxy
//! lacks). Bevy's `calculate_bounds` auto-inserts an `Aabb` from the mesh, so
//! `check_visibility` frustum-culls off-screen cells for free.

#![cfg(feature = "world-db")]

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use eustress_worlddb::{decode_instance_core, keys::world_to_cell, MortonKeyEncoder};
use meshopt::{generate_vertex_remap, remap_index_buffer, remap_vertex_buffer, VertexDataAdapter};

use super::active_db;
use super::file_loader::LoadInProgress;
use super::residency::ResidencyConfig;
use super::service_loader::ServiceComponent;
use super::SpaceRoot;

/// Unsigned 21-bit Morton cell coordinate triple at `chunk_size` (256) —
/// the SAME unit residency loads/evicts in (mirrors `residency::Cell`,
/// re-declared here to avoid widening that module's visibility).
type Cell = (u32, u32, u32);

// ── tunables ─────────────────────────────────────────────────────────────

// ── proxy decimation (LOD simplification) ──────────────────────────────────
//
// A proxy is a FAR level-of-detail: it is only ever seen from `load_radius`
// (~350 m) or further, so it needs a coarse silhouette, NOT every triangle of
// every part. Before a proxy mesh leaves the worker thread we run it through
// `meshopt::simplify_sloppy` (sloppy is right for a far LOD — it ignores exact
// topology to always hit the target and is the cheapest reduction) down to an
// aggressive triangle budget, then COMPACT the vertex buffer so the position /
// normal / colour arrays shrink in step with the index list (not just fewer
// indices over the same fat vertex array). Net: each proxy becomes ~10 % the
// vertices and triangles, so `meshes.add`, the GPU upload, and the per-frame
// render extract/visibility cost all collapse proportionally.

/// Decimate a proxy down to ≈ this fraction of its original triangle count
/// (subject to the absolute cap below). 0.10 = keep ~10 %.
const DECIMATE_FRACTION: f32 = 0.10;
/// Hard upper bound on a proxy's *kept* triangle count, regardless of the
/// fraction — a distant cell never needs more silhouette than this. The budget
/// is `min(orig/10, this)`, so a very dense cell is capped here and a sparse
/// one keeps its 10 %.
const DECIMATE_MAX_TRIS: usize = 2_000;
/// Below this triangle count a proxy is already cheap; skip simplification and
/// the remap (the meshopt round-trip would cost more than it saves, and a tiny
/// cell — a handful of parts — reads fine un-decimated at distance).
const DECIMATE_MIN_TRIS: usize = 256;
/// Relative error tolerance handed to the simplifier (fraction of mesh extent).
/// Generous because this is a far LOD where deformation is invisible — a larger
/// tolerance lets `simplify_sloppy` reach the aggressive target reliably.
const DECIMATE_TARGET_ERROR: f32 = 0.1;

/// HLOD configuration. Radii are read from env once at startup so the
/// optimization↔distance balance can be dialed WITHOUT a 16-min rebuild.
#[derive(Resource)]
pub struct HlodConfig {
    /// Retained env knob (`EUSTRESS_HLOD_RADIUS`, default 8000 m). In the
    /// build-once/persist design the proxy set is the DB's ACTUAL non-empty
    /// cells (enumerated once), so planning no longer sweeps a camera box and
    /// this radius no longer gates which cells build — the WHOLE map renders
    /// unconditionally (frustum culling, not a radius, hides off-screen
    /// proxies). Kept so the env var stays valid and as the optional outer cap
    /// the one-time enumeration applies when a user deliberately shrinks it
    /// below the map extent to trim very distant draw load.
    pub hlod_radius: f32,
    /// Max merge tasks in flight at once (bounds worker-pool + memory
    /// pressure from many simultaneous region scans). Env `EUSTRESS_HLOD_TASKS`.
    pub max_tasks_in_flight: usize,
    /// Max proxies spawned per frame from finished merges (bounds the
    /// main-thread asset-insert + entity-spawn spike). Env `EUSTRESS_HLOD_SPAWN`.
    pub max_proxies_per_frame: usize,
    /// Re-evaluate the proxy VISIBILITY set every N frames (the only periodic
    /// HLOD work once every cell is built), matching residency's cadence so
    /// the keep-box hide/show stays in lockstep with residency's spawn/evict
    /// and neither thrashes a moving camera. Env `EUSTRESS_HLOD_CADENCE`.
    pub cadence_frames: u32,
}

impl Default for HlodConfig {
    fn default() -> Self {
        // hlod_radius defaults LARGE so the headline "the whole map renders"
        // holds out of the box. 8000 m comfortably covers Vehicle Simulator's
        // extent; beyond the authored content there are simply no cells with
        // cores, so an over-large radius costs only empty region scans (which
        // return nothing and create no proxy).
        let radius = std::env::var("EUSTRESS_HLOD_RADIUS")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| *v > 0.0)
            .unwrap_or(8000.0);
        let tasks = std::env::var("EUSTRESS_HLOD_TASKS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|v| *v >= 1)
            .unwrap_or(4);
        let spawn = std::env::var("EUSTRESS_HLOD_SPAWN")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|v| *v >= 1)
            .unwrap_or(8);
        let cadence = std::env::var("EUSTRESS_HLOD_CADENCE")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|v| *v >= 1)
            .unwrap_or(12);
        Self {
            hlod_radius: radius,
            max_tasks_in_flight: tasks,
            max_proxies_per_frame: spawn,
            cadence_frames: cadence,
        }
    }
}

// ── components / resources ─────────────────────────────────────────────────

/// Marks a merged-cell proxy entity: ONE render-only mesh standing in for
/// every primitive part in a Morton cell. Carries the cell coord (so the
/// visibility system can hide it when residency owns the cell) and the merged
/// part count (diagnostics). Deliberately NOT a `BinaryEcsInstance` /
/// `Instance` — residency never touches it and it is never editable/selectable.
///
/// A proxy is built ONCE per non-empty cell and then PERSISTS for the whole
/// Space session — it is never despawned for camera position (the frustum
/// hides it off-screen; the keep-box hides it via [`Visibility`] when near).
/// The only despawn path is [`sys_hlod_reset_on_space_change`].
#[derive(Component, Debug, Clone, Copy)]
pub struct MergedCellProxy {
    pub cell: Cell,
    pub part_count: u32,
}

/// Live HLOD state: the one-time cell-enumeration latch, the build queue of
/// not-yet-merged non-empty cells, and the in-flight worker tasks. `enabled`
/// mirrors `ResidencyState.enabled` — HLOD only runs for a large (streaming)
/// Space.
///
/// The build-once/persist design drops all camera-follow planning state:
/// there is no `last_camera_cell`, no shell cursor, and no `empty_cells` set
/// (we never scan the empty volume — we enumerate the DB's real non-empty
/// cells directly, once). Steady state (every enumerated cell built) is fully
/// idle: `pending_cells` empty, `tasks` empty, `enumerate_task` consumed.
#[derive(Resource, Default)]
pub struct HlodState {
    /// Set by the residency boot-load decision (true only for large Spaces).
    pub enabled: bool,
    frame: u32,
    /// One-time non-empty-cell enumeration latch. `false` until the
    /// enumeration has been kicked off for the current Space; the
    /// Space-change reset re-arms it to `false`. Guards so the (one) DB
    /// snapshot + cell-distinct pass runs exactly ONCE per Space load.
    enumerated: bool,
    /// The in-flight background enumeration task: reads every instance core
    /// (`active_db::iter_instance_cores`), derives each core's Morton cell
    /// from its stored translation, and returns the DISTINCT non-empty cell
    /// set. `Some` only between kicking it off and draining its result on the
    /// main thread; `None` before arming and after the result is consumed.
    enumerate_task: Option<Task<Vec<Cell>>>,
    /// Cells that currently HAVE a live proxy entity (spawned, finished).
    proxy_cells: HashSet<Cell>,
    /// Cells with a build queued or in flight (dedup so a cell is merged once).
    building_cells: HashSet<Cell>,
    /// FIFO of enumerated non-empty cells awaiting a merge task (drained under
    /// the task cap). Filled ONCE when the enumeration result arrives; empty
    /// thereafter (steady state).
    pending_cells: VecDeque<Cell>,
    /// In-flight merge tasks, keyed by cell. Polled each frame; a finished
    /// task yields the built `MergedMesh` (or `None` if the cell had no
    /// mergeable primitive — then we just clear it from `building_cells`).
    tasks: HashMap<Cell, Task<Option<MergedMesh>>>,
    /// Shared white vertex-colour material handle (built once, reused by
    /// every proxy). `None` until the first proxy is spawned.
    material: Option<Handle<StandardMaterial>>,
}

/// CPU-side merged geometry produced on a worker thread. Held until the poll
/// system can upload it via `Assets<Mesh>::add` on the main thread.
pub struct MergedMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    /// Cell origin (min corner, world space) the vertices are relative to —
    /// the proxy `Transform.translation`. Keeps merged coords small.
    origin: Vec3,
    /// Number of parts actually merged (for the `MergedCellProxy` marker).
    part_count: u32,
}

// ── pure cell math (mirrors residency, re-derived to avoid pub churn) ────────

fn chunk_size() -> f32 {
    MortonKeyEncoder::default().chunk_size
}

/// Camera world position + radius → inclusive cell box (same encoder, 256).
fn camera_cell_box(cam: Vec3, radius: f32) -> (Cell, Cell) {
    let cs = chunk_size();
    let lo = |c: f32| world_to_cell(c - radius, cs);
    let hi = |c: f32| world_to_cell(c + radius, cs);
    (
        (lo(cam.x), lo(cam.y), lo(cam.z)),
        (hi(cam.x), hi(cam.y), hi(cam.z)),
    )
}

/// The cell a world position falls in.
fn cell_of(pos: Vec3) -> Cell {
    let cs = chunk_size();
    (
        world_to_cell(pos.x, cs),
        world_to_cell(pos.y, cs),
        world_to_cell(pos.z, cs),
    )
}

fn in_box(c: Cell, b: (Cell, Cell)) -> bool {
    let (lo, hi) = b;
    c.0 >= lo.0 && c.0 <= hi.0 && c.1 >= lo.1 && c.1 <= hi.1 && c.2 >= lo.2 && c.2 <= hi.2
}

// Used only by the box-containment unit test. The runtime no longer
// materializes any cell box (planning enumerates the DB's real non-empty
// cells; `sys_hlod_visibility` only point-tests `in_box`), so this helper is
// test-only in the non-test build.
#[allow(dead_code)]
fn cells_in_box(b: (Cell, Cell)) -> Vec<Cell> {
    let (lo, hi) = b;
    let mut out = Vec::new();
    for x in lo.0..=hi.0 {
        for y in lo.1..=hi.1 {
            for z in lo.2..=hi.2 {
                out.push((x, y, z));
            }
        }
    }
    out
}

/// World-space min corner (origin) of a cell — inverse of [`world_to_cell`]:
/// `world_to_cell(c) = floor(c / cs) + 2^20`, so the cell's low edge is
/// `(cell - 2^20) * cs`. Used as the proxy `Transform.translation` so the
/// merged vertices stay near the origin (small float coords).
fn cell_origin(cell: Cell) -> Vec3 {
    const BIAS: i64 = 1 << 20;
    let cs = chunk_size();
    Vec3::new(
        (cell.0 as i64 - BIAS) as f32 * cs,
        (cell.1 as i64 - BIAS) as f32 * cs,
        (cell.2 as i64 - BIAS) as f32 * cs,
    )
}

// ── primitive geometry (local space, unit shapes matching parts/*.glb) ───────
//
// Binary parts render a UNIT glb mesh (`parts/block.glb`, span [-0.5, 0.5])
// scaled by `Transform.scale = size`. So a distant proxy matches the visible
// silhouette by generating the SAME unit primitive procedurally, then
// transforming its vertices by the part's full world `Transform`. Bevy's own
// primitive meshers (`Cuboid`/`Sphere`/`Cylinder`) produce the identical unit
// shapes the glbs were authored from; wedge / corner-wedge / cone get a
// purpose-built unit mesh (the engine's primitive glbs for those, see
// `instance_loader::PRIMITIVE_MESHES`). All meshes are emitted at LOW
// resolution — these are seen only from far away, so a sphere is an icosphere
// at subdivision 1 and a cylinder is an 8-gon, keeping the merged vertex count
// modest even for a dense cell.

/// Which primitive a `parts/<x>.glb` mesh path denotes, by the same filename
/// substring match `instance_loader` uses. `None` ⇒ not a known primitive
/// (caller skips it from the merge). A bare/empty mesh defaults to a block —
/// matching the loader's Part fallback that injects `parts/block.glb`.
fn primitive_kind(mesh: &str) -> Option<Prim> {
    let lower = mesh.to_lowercase();
    let fname = lower.rsplit('/').next().unwrap_or(&lower);
    // Order matters: "corner_wedge" must be tested before "wedge".
    if mesh.is_empty() || fname.contains("block") {
        Some(Prim::Block)
    } else if fname.contains("ball") {
        Some(Prim::Ball)
    } else if fname.contains("corner_wedge") || fname.contains("cornerwedge") {
        Some(Prim::CornerWedge)
    } else if fname.contains("wedge") {
        Some(Prim::Wedge)
    } else if fname.contains("cylinder") {
        Some(Prim::Cylinder)
    } else if fname.contains("cone") {
        Some(Prim::Cone)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
enum Prim {
    Block,
    Ball,
    Cylinder,
    Wedge,
    CornerWedge,
    Cone,
}

/// Unit-shape geometry (positions + normals + triangle indices) for a
/// primitive, in LOCAL space (centred on origin, span ≈ [-0.5, 0.5]). Built
/// once per merge via the cache below so N identical shapes in a cell reuse
/// one CPU buffer.
struct UnitGeom {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

impl Prim {
    /// Build the unit geometry by extracting it from a Bevy primitive mesher
    /// (block/ball/cylinder) or a hand-built triangle set (wedge/corner/cone).
    /// LOW resolution — proxies are distant.
    fn unit_geom(self) -> UnitGeom {
        match self {
            Prim::Block => mesh_to_geom(Mesh::from(Cuboid::new(1.0, 1.0, 1.0))),
            // Icosphere subdiv 1 is a cheap far-LOD sphere (matches ball.glb's
            // silhouette); fall back to a coarse uv-sphere if ico fails.
            Prim::Ball => mesh_to_geom(
                Sphere::new(0.5)
                    .mesh()
                    .ico(1)
                    .unwrap_or_else(|_| Sphere::new(0.5).mesh().uv(8, 6)),
            ),
            // Y-axis cylinder, radius 0.5, full height 1.0 (half_height 0.5),
            // matching `spawn.rs` `Cylinder::new(size.x/2, size.y)` semantics.
            // 10 radial segments is plenty at distance.
            Prim::Cylinder => mesh_to_geom(
                Cylinder::new(0.5, 1.0)
                    .mesh()
                    .resolution(10)
                    .build(),
            ),
            // Cone, radius 0.5, height 1.0, apex +Y — matches cone.glb / the
            // `Cone` adornment primitive orientation.
            Prim::Cone => mesh_to_geom(
                Cone {
                    radius: 0.5,
                    height: 1.0,
                }
                .mesh()
                .resolution(10)
                .build(),
            ),
            Prim::Wedge => unit_wedge(),
            Prim::CornerWedge => unit_corner_wedge(),
        }
    }
}

/// Pull POSITION + NORMAL + indices out of a Bevy-meshed primitive into the
/// flat `UnitGeom` the merger appends. A primitive mesh always has both
/// attributes as `Float32x3` and `U32`/`U16` indices; anything unexpected
/// yields an empty geom (that primitive simply contributes nothing — safe).
fn mesh_to_geom(mesh: Mesh) -> UnitGeom {
    use bevy::mesh::VertexAttributeValues;
    let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
        _ => Vec::new(),
    };
    let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
        // Defensive: if a mesher omitted normals, fill with +Y so lighting is
        // at least defined (won't happen for the primitives used here).
        _ => vec![[0.0, 1.0, 0.0]; positions.len()],
    };
    let indices = match mesh.indices() {
        Some(Indices::U32(v)) => v.clone(),
        Some(Indices::U16(v)) => v.iter().map(|&i| i as u32).collect(),
        None => (0..positions.len() as u32).collect(),
    };
    UnitGeom {
        positions,
        normals,
        indices,
    }
}

/// Unit wedge (triangular prism): a 1×1×1 box with the top sloped from the
/// +Z back-top edge down to the -Z front-bottom edge. Approximates wedge.glb
/// well enough for a distant proxy. Span [-0.5, 0.5] on every axis.
fn unit_wedge() -> UnitGeom {
    // 6 corners: bottom rectangle (y=-0.5) + top ridge edge (y=+0.5 at z=+0.5).
    // Layout (x: -0.5/+0.5, z: -0.5/+0.5):
    //   0: (-.5,-.5,-.5)  1: (+.5,-.5,-.5)  2: (+.5,-.5,+.5)  3: (-.5,-.5,+.5)
    //   4: (-.5,+.5,+.5)  5: (+.5,+.5,+.5)   ← top ridge over the back (+z) edge
    let p = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, -0.5, 0.5],
        [-0.5, -0.5, 0.5],
        [-0.5, 0.5, 0.5],
        [0.5, 0.5, 0.5],
    ];
    // Faces (CCW, outward): bottom, back(+z), slope, left, right.
    let tris: &[[usize; 3]] = &[
        [0, 2, 1],
        [0, 3, 2], // bottom (y=-0.5), normal -Y
        [3, 4, 5],
        [3, 5, 2], // back vertical face (z=+0.5), normal +Z
        [0, 1, 5],
        [0, 5, 4], // sloped top, normal up-ish/forward
        [0, 4, 3], // left triangle (x=-0.5), normal -X
        [1, 2, 5], // right triangle (x=+0.5), normal +X
    ];
    flat_geom(&p, tris)
}

/// Unit corner wedge: a tetra-like wedge sloping toward one corner. A 1×1×1
/// box footprint whose top collapses to the back-right top corner — a coarse
/// match for corner_wedge.glb at distance. Span [-0.5, 0.5].
fn unit_corner_wedge() -> UnitGeom {
    // 5 corners: bottom rectangle + one apex at the back-right top.
    //   0: (-.5,-.5,-.5)  1: (+.5,-.5,-.5)  2: (+.5,-.5,+.5)  3: (-.5,-.5,+.5)
    //   4: (+.5,+.5,+.5)  ← single top apex
    let p = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, -0.5, 0.5],
        [-0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
    ];
    let tris: &[[usize; 3]] = &[
        [0, 2, 1],
        [0, 3, 2], // bottom (y=-0.5)
        [1, 2, 4], // right vertical (x=+0.5)
        [2, 3, 4], // back vertical (z=+0.5)
        [0, 1, 4], // front slope
        [0, 4, 3], // left slope
    ];
    flat_geom(&p, tris)
}

/// Build flat-shaded `UnitGeom` from a corner list + triangle index list:
/// every triangle gets its own 3 vertices with a single face normal (so the
/// low-poly wedge/corner read crisply). Cheap; runs once per merge per shape.
fn flat_geom(corners: &[[f32; 3]], tris: &[[usize; 3]]) -> UnitGeom {
    let mut positions = Vec::with_capacity(tris.len() * 3);
    let mut normals = Vec::with_capacity(tris.len() * 3);
    let mut indices = Vec::with_capacity(tris.len() * 3);
    for t in tris {
        let a = Vec3::from_array(corners[t[0]]);
        let b = Vec3::from_array(corners[t[1]]);
        let c = Vec3::from_array(corners[t[2]]);
        let n = (b - a).cross(c - a).normalize_or_zero().to_array();
        let base = positions.len() as u32;
        positions.push(a.to_array());
        positions.push(b.to_array());
        positions.push(c.to_array());
        normals.push(n);
        normals.push(n);
        normals.push(n);
        indices.push(base);
        indices.push(base + 1);
        indices.push(base + 2);
    }
    UnitGeom {
        positions,
        normals,
        indices,
    }
}

// ── proxy decimation (runs on a worker thread, inside build_merged_mesh) ─────

/// One fully-interleaved proxy vertex: position + normal + colour, contiguous
/// and tightly packed (10 × `f32` = 40 bytes, no padding). Interleaving lets a
/// SINGLE `meshopt::generate_vertex_remap` pass compact ALL THREE attributes
/// consistently: binary-equivalence over the whole struct means two vertices
/// merge only when their position AND normal AND colour are identical (a true
/// duplicate), so the remap can never cross-merge verts that share a position
/// but differ in shading. `Clone + Copy + Default` satisfy the meshopt remap
/// generics; `#[repr(C)]` keeps the field order stable for the byte view.
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct MergeVertex {
    pos: [f32; 3],
    normal: [f32; 3],
    color: [f32; 4],
}

/// Aggressively simplify a freshly-merged proxy in place (positions/normals/
/// colours/indices) to a far-LOD triangle budget, then COMPACT the vertex
/// buffer so all three attribute arrays shrink with the index list. CPU-only —
/// called from inside the worker-thread merge, never on the main thread.
///
/// Strategy (all via `meshopt` 0.4):
/// 1. **Skip-guard** — a proxy already under `DECIMATE_MIN_TRIS` is left as-is
///    (the round-trip would cost more than it saves; tiny cells read fine).
/// 2. **`simplify_sloppy`** reduces the INDEX buffer to `target_index_count`
///    (= budget × 3) against the position stream (stride 12). Sloppy is the
///    right call for a far LOD: it discards exact topology to always reach the
///    target and is the cheapest variant. The result still references the
///    *original* (fat) vertex arrays — only the index list is shorter.
/// 3. **Compact** — `generate_vertex_remap` over the interleaved `MergeVertex`
///    stream WITH the simplified indices builds a full-length (`vertex_count`)
///    remap in which only vertices the new indices still reference get a slot
///    (unreferenced ones keep the `~0` sentinel and are dropped). `remap_index_
///    buffer` rewrites the indices into the compact space and `remap_vertex_
///    buffer` produces the small interleaved array, which we de-interleave back
///    into the three attribute `Vec`s. Net: small vertex buffer AND small index
///    buffer.
///
/// On any degenerate/empty intermediate result it LEAVES the buffers untouched
/// (the caller still ships the un-simplified proxy) rather than dropping the
/// cell — a recognisable-but-heavy proxy beats a hole in the map.
fn decimate_merged(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
) {
    let orig_index_count = indices.len();
    let orig_tris = orig_index_count / 3;
    let vertex_count = positions.len();

    // (1) Already cheap — nothing to gain. Also bail on anything malformed so
    // the meshopt FFI only ever sees a well-formed triangle list.
    if orig_tris <= DECIMATE_MIN_TRIS
        || orig_index_count < 3
        || orig_index_count % 3 != 0
        || vertex_count == 0
        || normals.len() != vertex_count
        || colors.len() != vertex_count
    {
        return;
    }

    // Target triangle budget: ~10 % of the original, capped, and at least one
    // triangle so `target_index_count` is always a valid (≥3) request.
    let target_tris = ((orig_tris as f32 * DECIMATE_FRACTION) as usize)
        .min(DECIMATE_MAX_TRIS)
        .max(1);
    let target_index_count = target_tris * 3;
    // If the budget isn't actually smaller than what we have, skip.
    if target_index_count >= orig_index_count {
        return;
    }

    // (2) Simplify the index buffer against the position stream. The adapter is
    // a (stride-12) byte view over the `[f32;3]` positions — the SAME pattern
    // `mesh_optimizer::optimize_mesh_in_place` uses.
    let pos_bytes: Vec<u8> =
        bytemuck::cast_slice::<[f32; 3], u8>(positions.as_slice()).to_vec();
    let pos_stride = std::mem::size_of::<[f32; 3]>(); // 12
    let Ok(adapter) = VertexDataAdapter::new(&pos_bytes, pos_stride, 0) else {
        return; // malformed view — keep the un-simplified proxy
    };
    let simplified = meshopt::simplify_sloppy(
        indices.as_slice(),
        &adapter,
        target_index_count,
        DECIMATE_TARGET_ERROR,
        None,
    );
    // Degenerate reduction (collapsed to nothing, or — defensively — somehow
    // grew): don't touch the buffers, ship the original.
    if simplified.len() < 3 || simplified.len() >= orig_index_count {
        return;
    }

    // (3) Compact the vertex buffer. Interleave once, build the remap from the
    // simplified indices (drops now-unreferenced verts + dedups exact dupes),
    // then rewrite indices + each attribute through it.
    let mut interleaved: Vec<MergeVertex> = Vec::with_capacity(vertex_count);
    for i in 0..vertex_count {
        interleaved.push(MergeVertex {
            pos: positions[i],
            normal: normals[i],
            color: colors[i],
        });
    }
    let (unique_count, remap) = generate_vertex_remap(&interleaved, Some(&simplified));
    // A sane remap yields at least one vertex and no more than we started with.
    if unique_count == 0 || unique_count > vertex_count {
        return;
    }
    let new_indices = remap_index_buffer(Some(&simplified), vertex_count, &remap);
    let compact: Vec<MergeVertex> = remap_vertex_buffer(&interleaved, unique_count, &remap);
    if compact.len() != unique_count || new_indices.len() != simplified.len() {
        return; // unexpected meshopt output — keep the original, never corrupt
    }

    // De-interleave back into the three attribute arrays the proxy mesh needs.
    let mut new_pos: Vec<[f32; 3]> = Vec::with_capacity(unique_count);
    let mut new_norm: Vec<[f32; 3]> = Vec::with_capacity(unique_count);
    let mut new_col: Vec<[f32; 4]> = Vec::with_capacity(unique_count);
    for v in &compact {
        new_pos.push(v.pos);
        new_norm.push(v.normal);
        new_col.push(v.color);
    }

    *positions = new_pos;
    *normals = new_norm;
    *colors = new_col;
    *indices = new_indices;
}

// ── the merge (runs on a worker thread) ──────────────────────────────────────

/// Decode + merge every mergeable primitive in a cell into one CPU mesh,
/// relative to `origin` (the cell's world-space min corner). Pure / no Bevy
/// world access, so it runs on `AsyncComputeTaskPool`. Returns `None` when the
/// cell contributed zero mergeable geometry (all custom-mesh / transparent /
/// undecodable) — the caller then spawns no proxy for it.
///
/// `cores` is the raw `(stored_id, rkyv bytes)` set the SAME DB region scan
/// residency's load uses returns, so the proxy covers exactly the parts
/// residency would otherwise spawn individually.
fn build_merged_mesh(cell: Cell, origin: Vec3, cores: Vec<(u64, Vec<u8>)>) -> Option<MergedMesh> {
    // Cache unit geometry per shape so N identical primitives in this cell
    // reuse one buffer (a cell is overwhelmingly one or two shapes).
    let mut geom_cache: HashMap<u8, UnitGeom> = HashMap::new();

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut part_count: u32 = 0;

    for (_stored_id, bytes) in &cores {
        let Ok(core) = decode_instance_core(bytes) else {
            continue; // undecodable record — skip, never crash the merge
        };

        // v1 skip rules: non-primitive (custom/glb) mesh OR any transparency.
        // `mesh_requires_filesystem` is exactly the router's "custom mesh"
        // predicate (non-empty AND not `parts/`), so this is the same guard
        // that keeps custom meshes out of binary-ECS.
        if super::representation::mesh_requires_filesystem(&core.mesh) {
            continue;
        }
        if core.transparency > 0.0 {
            continue;
        }
        let Some(prim) = primitive_kind(&core.mesh) else {
            continue;
        };
        let key = prim as u8;
        let geom = geom_cache.entry(key).or_insert_with(|| prim.unit_geom());
        if geom.positions.is_empty() {
            continue; // mesher produced nothing — shouldn't happen, but safe
        }

        // Part world transform, relative to the cell origin (small coords).
        let translation = Vec3::new(core.t[0], core.t[1], core.t[2]) - origin;
        let rotation = Quat::from_xyzw(core.r[0], core.r[1], core.r[2], core.r[3]);
        // A zero/degenerate quaternion would NaN the whole proxy; fall back to
        // identity (same defensiveness as `instance_loader::sanitize_rot`).
        let rotation = if rotation.length_squared().is_finite()
            && rotation.length_squared() > 1e-8
        {
            rotation.normalize()
        } else {
            Quat::IDENTITY
        };
        let scale = Vec3::new(core.s[0], core.s[1], core.s[2]);
        // Skip a non-finite transform outright rather than poison the buffer.
        if !translation.is_finite() || !scale.is_finite() {
            continue;
        }
        let xf = Transform {
            translation,
            rotation,
            scale,
        };
        // Normals transform by the inverse-transpose; for our axis-aligned,
        // non-negative scales the rotation alone (re-normalised) is a good
        // far-LOD approximation and avoids a per-vertex matrix inverse.
        let normal_rot = rotation;

        let [r, g, b, _a] = core.color;
        let vcolor = [r, g, b, 1.0]; // opaque at distance (alpha forced to 1)

        let base = positions.len() as u32;
        for (i, p) in geom.positions.iter().enumerate() {
            let world = xf.transform_point(Vec3::from_array(*p));
            positions.push(world.to_array());
            let n = normal_rot * Vec3::from_array(geom.normals[i]);
            normals.push(n.normalize_or_zero().to_array());
            colors.push(vcolor);
        }
        for idx in &geom.indices {
            indices.push(base + idx);
        }
        part_count += 1;
    }

    if part_count == 0 || positions.is_empty() {
        return None;
    }

    // Decimate to a far-LOD silhouette BEFORE returning — this is the headline
    // fix: the proxy keeps every part's full geometry until here, so without
    // this a dense cell ships thousands of triangles to be uploaded + extracted
    // + rendered every frame even though it's only ever seen from ≥350 m. All
    // CPU work, still on this worker thread (build_merged_mesh runs off the main
    // thread), so it never stalls the frame. Mutates the buffers in place; on a
    // tiny cell or any degenerate meshopt result it leaves them untouched (we
    // ship the un-simplified proxy rather than drop the cell).
    decimate_merged(&mut positions, &mut normals, &mut colors, &mut indices);

    Some(MergedMesh {
        positions,
        normals,
        colors,
        indices,
        origin,
        part_count,
    })
}

// ── systems ──────────────────────────────────────────────────────────────────

/// One-time plan: (1) ARM a single background task that enumerates the DB's
/// DISTINCT non-empty cells, then (2) drain those cells into capped merge
/// tasks. NO camera box, NO cadence gate, NO shell walk, NO per-frame O(map)
/// scan — once every enumerated cell is built this system is fully idle
/// (early-returns at the `pending_cells`/`enumerate_task` checks).
///
/// ### Why enumerate instead of sweep
///
/// A real map's parts occupy only a few hundred distinct Morton cells, but the
/// old `hlod_radius` box is ~290 K cells, almost all EMPTY. Walking that box
/// every frame (against a 290 K `empty_cells` set) was the Update spike. Here
/// we ask the DB for the cells that ACTUALLY hold cores — one eager snapshot
/// (`active_db::iter_instance_cores`, the same call the boot-load uses),
/// decode each core's translation to its cell, collect the distinct set — and
/// build a proxy for each, ONCE. The enumeration runs on
/// [`AsyncComputeTaskPool`] so the main thread never pays the DB read.
///
/// ### No keep-box exclusion at PLAN time
///
/// Unlike the old planner we do NOT subtract residency's keep-box here: every
/// non-empty cell gets a persistent proxy (so the whole map is always
/// covered). The no-double-render invariant is enforced LATER and cheaply by
/// [`sys_hlod_visibility`], which HIDES (never despawns) the proxy of any cell
/// residency currently owns. Hiding/showing in place means a near↔far
/// transition is instant with zero rebuild.
#[allow(clippy::too_many_arguments)]
pub fn sys_hlod_plan(
    load_in_progress: Res<LoadInProgress>,
    mut state: ResMut<HlodState>,
    cfg: Res<HlodConfig>,
) {
    if !state.enabled || load_in_progress.active || !active_db::is_active() {
        return;
    }
    state.frame = state.frame.wrapping_add(1);

    // ── (1) one-time enumeration ────────────────────────────────────────
    // Arm the background enumeration exactly once per Space (the
    // Space-change reset re-arms `enumerated`). The task reads the whole
    // entities partition ONCE off-thread and returns the distinct non-empty
    // cell set; the main thread only drains its Vec result.
    if !state.enumerated {
        state.enumerated = true;
        // Optional outer cap (default 8000 m, far beyond any real extent):
        // a cell whose origin is past this from the world origin is dropped
        // so a deliberately-shrunk EUSTRESS_HLOD_RADIUS still trims very
        // distant draw load. With the default it excludes nothing.
        let max_dist = cfg.hlod_radius;
        let pool = AsyncComputeTaskPool::get();
        let task = pool.spawn(async move {
            // SAME eager snapshot the boot-load consumes. We only need each
            // core's CELL: decode the rkyv core, read its translation `t`,
            // map to a Morton cell. (Decode is one-time + off-thread, so the
            // simplicity is free — no key-byte plumbing needed.)
            let cores = active_db::iter_instance_cores();
            let mut set: HashSet<Cell> = HashSet::new();
            for (_id, bytes) in &cores {
                let Ok(core) = decode_instance_core(bytes) else {
                    continue; // undecodable — skip (never panics the task)
                };
                let pos = Vec3::new(core.t[0], core.t[1], core.t[2]);
                if !pos.is_finite() {
                    continue;
                }
                // Optional far-trim against the (origin-relative) cap.
                if max_dist.is_finite() && pos.length() > max_dist {
                    continue;
                }
                set.insert(cell_of(pos));
            }
            set.into_iter().collect::<Vec<Cell>>()
        });
        state.enumerate_task = Some(task);
    }

    // Drain the enumeration result when it lands: push every distinct
    // non-empty cell into the build queue (deduped against cells already
    // proxied / building, so a re-arm after a partial build is harmless).
    // Take the task OUT to poll it (avoids holding a `&mut` on `state` while
    // we mutate sibling fields); put it back if it isn't finished yet.
    if let Some(mut task) = state.enumerate_task.take() {
        match block_on(future::poll_once(&mut task)) {
            Some(cells) => {
                // Finished — task dropped (not put back), latch stays armed.
                let mut queued = 0usize;
                for c in cells {
                    if state.proxy_cells.contains(&c) || state.building_cells.contains(&c) {
                        continue;
                    }
                    state.building_cells.insert(c);
                    state.pending_cells.push_back(c);
                    queued += 1;
                }
                info!(
                    target: "eustress_engine::world_db",
                    non_empty_cells = queued,
                    "HLOD: enumerated the Space's non-empty cells — building one persistent proxy each"
                );
                // Fall through: the queue is now full, so the merge-task loop
                // below kicks off the first builds on this SAME frame.
            }
            None => {
                // Still enumerating — put the task back and wait. Nothing else
                // to do until it lands (the build queue is still empty).
                state.enumerate_task = Some(task);
                return;
            }
        }
    }

    // ── (2) drain the build queue into capped merge tasks ───────────────
    // Idle once `pending_cells` is empty (every enumerated cell built). The
    // region scan happens on the worker, so the main thread pays only the
    // enqueue + spawn-future cost, bounded by `max_tasks_in_flight`.
    if state.pending_cells.is_empty() {
        return;
    }
    let pool = AsyncComputeTaskPool::get();
    while state.tasks.len() < cfg.max_tasks_in_flight {
        let Some(cell) = state.pending_cells.pop_front() else {
            break;
        };
        let origin = cell_origin(cell);
        let task = pool.spawn(async move {
            // SAME single-cell DB region scan residency's load uses — so the
            // proxy and the individuals cover the identical core set.
            let cores = active_db::iter_instance_cores_in_region(
                (cell.0, cell.0),
                (cell.1, cell.1),
                (cell.2, cell.2),
            );
            build_merged_mesh(cell, origin, cores)
        });
        state.tasks.insert(cell, task);
    }
}

/// Poll in-flight merge tasks; for each finished one, upload its mesh and
/// spawn the proxy (bounded by `max_proxies_per_frame` so a burst of
/// completions doesn't spike the frame). A task that returned `None` (no
/// mergeable primitive in the cell) just clears its `building_cells` slot.
#[allow(clippy::too_many_arguments)]
pub fn sys_hlod_collect(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    services: Query<(Entity, &ServiceComponent)>,
    mut state: ResMut<HlodState>,
    cfg: Res<HlodConfig>,
) {
    if !state.enabled || state.tasks.is_empty() {
        return;
    }
    // Need Workspace to parent the proxy under (matches binary parts' parent).
    let Some(workspace) = services
        .iter()
        .find(|(_, s)| s.class_name == "Workspace")
        .map(|(e, _)| e)
    else {
        return; // services not ready — leave tasks pending, retry next frame
    };

    // Collect finished cells first (can't mutate `state.tasks` while polling
    // through a borrow of it).
    let mut finished: Vec<(Cell, Option<MergedMesh>)> = Vec::new();
    for (cell, task) in state.tasks.iter_mut() {
        if finished.len() >= cfg.max_proxies_per_frame {
            break;
        }
        if let Some(result) = block_on(future::poll_once(task)) {
            finished.push((*cell, result));
        }
    }

    if finished.is_empty() {
        return;
    }

    // Ensure the one shared proxy material exists (white base, vertex colours
    // enabled, lit, fairly rough — a matte stand-in for the real materials).
    let material = state
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.9,
                ..default()
            })
        })
        .clone();

    for (cell, result) in finished {
        // Drop the task + clear the building latch regardless of outcome.
        state.tasks.remove(&cell);
        state.building_cells.remove(&cell);

        let Some(merged) = result else {
            // Cell held no mergeable primitive (all custom-mesh / transparent —
            // uncommon for an enumerated non-empty cell, but possible). The
            // building latch is already cleared above; we simply never spawn a
            // proxy for it. There is no re-scan to guard against: enumeration
            // is one-time, so this cell is never revisited.
            continue;
        };

        // Upload the CPU mesh. Vertex colours go on `ATTRIBUTE_COLOR`; Bevy's
        // `calculate_bounds` will auto-insert an `Aabb` from the positions so
        // `check_visibility` frustum-culls this proxy when off-screen.
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, merged.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, merged.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, merged.colors);
        mesh.insert_indices(Indices::U32(merged.indices));
        let mesh_handle = meshes.add(mesh);

        // Spawn the proxy: render-only. NO BinaryEcsInstance / ColdStreamed /
        // Instance / Name / Collider — residency never touches it, it is not
        // selectable/editable, and it emits no scene deltas. Born
        // `Visibility::Inherited` (drawn); `sys_hlod_visibility` flips it to
        // `Hidden` on the next cadence tick if this cell is inside residency's
        // keep-box (residency draws its individuals instead). The proxy now
        // PERSISTS for the Space session — nothing despawns it for camera
        // position.
        commands.spawn((
            Mesh3d(mesh_handle),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(merged.origin),
            Visibility::Inherited,
            MergedCellProxy {
                cell,
                part_count: merged.part_count,
            },
            ChildOf(workspace),
        ));
        state.proxy_cells.insert(cell);
    }
}

/// TOGGLE each proxy's [`Visibility`] so a cell residency currently owns
/// (inside its keep-box) is HIDDEN — residency draws that cell's individuals
/// instead — and every other proxy is shown. This is the no-double-render
/// handoff, done WITHOUT despawn: a cell going near hides instantly and going
/// far shows instantly, with zero rebuild and zero gap (the merged mesh stays
/// resident the whole time; only its `Visibility` flag flips).
///
/// Cheap: iterates ONLY the proxy set (one entity per non-empty cell — a few
/// hundred), cadence-gated to match residency's keep-box re-evaluation so the
/// hide/show stays in lockstep with residency's spawn/evict. Writes a
/// `Visibility` only when it actually changes (no needless change-detection
/// churn). Keeps its own `Local` frame counter so it never contends on
/// `HlodState` (it reads nothing mutable).
///
/// Keep-box radius = `evict_radius` (the band residency's individuals actually
/// occupy through the whole hysteresis), NOT `load_radius` — so the proxy
/// stays hidden until residency has TRULY released the cell, then re-shows.
/// Off-screen proxies (near or far) are additionally frustum-culled by Bevy's
/// auto-`Aabb` for free; this system only governs the near double-render.
pub fn sys_hlod_visibility(
    mut frame: Local<u32>,
    cameras: Query<(&GlobalTransform, &Camera), With<Camera3d>>,
    mut proxies: Query<(&MergedCellProxy, &mut Visibility)>,
    res_cfg: Res<ResidencyConfig>,
    cfg: Res<HlodConfig>,
    state: Res<HlodState>,
) {
    if !state.enabled {
        return;
    }
    // Cadence gate (its own counter — no shared-state mutation). Matches
    // residency's keep-box cadence so the two never thrash a moving camera.
    *frame = frame.wrapping_add(1);
    if *frame % cfg.cadence_frames.max(1) != 0 {
        return;
    }
    let Some((cam_tf, _)) = cameras.iter().find(|(_, c)| c.order == 0) else {
        return; // main viewport camera not up yet — leave visibilities as-is
    };
    let campos = cam_tf.translation();

    // Residency's keep-box (the SAME camera + `evict_radius` it evicts at). A
    // proxy whose cell is inside it must be HIDDEN (residency owns the cell);
    // outside it, SHOWN.
    let near_keep = camera_cell_box(campos, res_cfg.evict_radius);

    for (proxy, mut vis) in proxies.iter_mut() {
        // Hide a proxy whose cell residency owns (keep-box); show the rest.
        let want_hidden = in_box(proxy.cell, near_keep);
        let is_hidden = matches!(*vis, Visibility::Hidden);
        // Only write on a real change so we don't dirty unchanged proxies
        // (Bevy change-detection fires on any `DerefMut`, even a same-value
        // write — gating keeps the steady-state visibility pass allocation-
        // and churn-free).
        if want_hidden != is_hidden {
            *vis = if want_hidden {
                Visibility::Hidden // residency draws this cell's individuals
            } else {
                Visibility::Inherited // proxy draws (frustum may still cull it)
            };
        }
    }
}

/// Tear down ALL proxies + reset HLOD state on a Space switch, AND re-arm the
/// one-time non-empty-cell enumeration for the new Space. Mirrors the
/// residency reset path: a genuine Space change must not leave a previous
/// Space's proxies floating in the new one, and the new Space's distinct cells
/// must be re-enumerated (different content). The robust switch trigger is a
/// `SpaceRoot` path change — the same latch shape the binary boot-load uses.
///
/// This is the ONLY despawn path for a proxy now (camera movement never
/// despawns one — see `sys_hlod_visibility`, which hides/shows in place).
pub fn sys_hlod_reset_on_space_change(
    space_root: Res<SpaceRoot>,
    mut last_root: Local<Option<std::path::PathBuf>>,
    mut commands: Commands,
    proxies: Query<Entity, With<MergedCellProxy>>,
    mut state: ResMut<HlodState>,
) {
    if last_root.as_deref() == Some(space_root.0.as_path()) {
        return;
    }
    // First run latches without tearing down (nothing to tear down yet); a
    // later genuine path change clears the prior Space's proxies + state.
    let had_prior = last_root.is_some();
    *last_root = Some(space_root.0.clone());
    if !had_prior {
        return;
    }
    let mut n = 0usize;
    for e in proxies.iter() {
        commands.entity(e).despawn();
        n += 1;
    }
    // Reset live state but PRESERVE `enabled` — the residency boot-load
    // decision owns that flag and sets it for the new Space; clearing it here
    // would race that decision. RE-ARM the enumeration so the new Space's
    // distinct non-empty cells are discovered again (the next `sys_hlod_plan`
    // tick kicks off a fresh background enumeration). Drop the shared material
    // handle so the new Space rebuilds one (cheap; avoids a stale asset).
    state.frame = 0;
    state.enumerated = false; // re-arm one-time enumeration for the new Space
    state.enumerate_task = None; // drop any in-flight enumeration of the old Space
    state.proxy_cells.clear();
    state.building_cells.clear();
    state.pending_cells.clear();
    state.tasks.clear();
    state.material = None;
    if n > 0 {
        info!(
            target: "eustress_engine::world_db",
            despawned = n,
            space = %space_root.0.display(),
            "HLOD: cleared previous Space's merged-cell proxies on Space switch"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // world_to_cell(c, 256) = floor(c/256) + 2^20.
    const BIAS: u32 = 1 << 20;

    #[test]
    fn cell_origin_is_inverse_of_world_to_cell() {
        // A position at +600 on x → cell floor(600/256)=2 → BIAS+2; the cell's
        // world-space min corner is 2*256 = 512.
        let cell = cell_of(Vec3::new(600.0, 0.0, 0.0));
        assert_eq!(cell.0, BIAS + 2);
        let origin = cell_origin(cell);
        assert_eq!(origin.x, 512.0);
        // And a part at x=600 sits 88 m into its cell (600 - 512), so its
        // origin-relative x stays small — the whole point of cell-local coords.
        assert!((600.0 - origin.x) > 0.0 && (600.0 - origin.x) < 256.0);
    }

    #[test]
    fn cell_origin_handles_negative_coords() {
        // floor(-100/256) = -1 → BIAS-1; min corner = -1*256 = -256.
        let cell = cell_of(Vec3::new(-100.0, 0.0, 0.0));
        assert_eq!(cell.0, BIAS - 1);
        assert_eq!(cell_origin(cell).x, -256.0);
    }

    #[test]
    fn visibility_hides_keep_box_and_shows_far() {
        // The no-double-render invariant `sys_hlod_visibility` enforces:
        // a proxy whose cell is INSIDE residency's keep-box must be HIDDEN
        // (residency draws that cell's individuals), and a cell OUTSIDE it
        // must be SHOWN. The predicate is exactly `in_box(cell, keep_box)`.
        let cam = Vec3::ZERO;
        let keep = camera_cell_box(cam, 500.0); // residency's evict_radius box
        // Every cell residency owns (its whole keep-box) → hidden.
        for c in cells_in_box(keep) {
            assert!(
                in_box(c, keep),
                "keep-box cell {c:?} must read inside the keep-box → proxy Hidden"
            );
        }
        // A far cell (well outside the keep-box) → shown by the proxy.
        let far_cell = cell_of(Vec3::new(4000.0, 0.0, 0.0));
        assert!(
            !in_box(far_cell, keep),
            "far cell must read outside the keep-box → proxy Inherited/Visible"
        );
    }

    #[test]
    fn primitive_kind_matches_loader_filenames() {
        assert!(matches!(primitive_kind("parts/block.glb"), Some(Prim::Block)));
        assert!(matches!(primitive_kind("parts/ball.glb"), Some(Prim::Ball)));
        assert!(matches!(
            primitive_kind("parts/cylinder.glb"),
            Some(Prim::Cylinder)
        ));
        // corner_wedge must NOT be mis-classified as wedge.
        assert!(matches!(
            primitive_kind("parts/corner_wedge.glb"),
            Some(Prim::CornerWedge)
        ));
        assert!(matches!(primitive_kind("parts/wedge.glb"), Some(Prim::Wedge)));
        assert!(matches!(primitive_kind("parts/cone.glb"), Some(Prim::Cone)));
        // Empty mesh = the loader's Part fallback (block).
        assert!(matches!(primitive_kind(""), Some(Prim::Block)));
        // A custom mesh is not a primitive — merge skips it.
        assert!(primitive_kind("meshes/VCell_Housing.glb").is_none());
    }

    #[test]
    fn unit_block_geom_is_centered_unit_cube() {
        let g = Prim::Block.unit_geom();
        assert!(!g.positions.is_empty());
        // Every vertex within the unit box [-0.5, 0.5].
        for p in &g.positions {
            for axis in p {
                assert!(axis.abs() <= 0.5 + 1e-5, "block vertex {axis} outside unit cube");
            }
        }
        // Indices are in range.
        let n = g.positions.len() as u32;
        for i in &g.indices {
            assert!(*i < n);
        }
    }

    #[test]
    fn wedge_and_corner_geoms_are_unit_and_indexed() {
        for g in [unit_wedge(), unit_corner_wedge()] {
            assert!(!g.positions.is_empty());
            assert_eq!(g.positions.len(), g.normals.len());
            for p in &g.positions {
                for axis in p {
                    assert!(axis.abs() <= 0.5 + 1e-5);
                }
            }
            let n = g.positions.len() as u32;
            for i in &g.indices {
                assert!(*i < n);
            }
            // Flat-shaded: one triangle per 3 verts, indices count == verts.
            assert_eq!(g.indices.len(), g.positions.len());
        }
    }

    /// Build a dense `n × n`-quad indexed grid with a gentle bumpy height field
    /// (real shared-vertex topology AND curvature, so `simplify_sloppy` has
    /// something to collapse toward the target rather than flattening a planar
    /// sheet to near-nothing). Returns (positions, normals, colors, indices) the
    /// way `build_merged_mesh` assembles them.
    fn dense_grid(n: usize) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
        let w = n + 1; // verts per row
        let mut positions = Vec::with_capacity(w * w);
        let mut normals = Vec::with_capacity(w * w);
        let mut colors = Vec::with_capacity(w * w);
        for y in 0..w {
            for x in 0..w {
                // Bumpy surface: non-planar so the simplifier keeps real shape
                // and produces a stable, non-degenerate reduction.
                let h = (x as f32 * 0.7).sin() + (y as f32 * 0.5).cos();
                positions.push([x as f32, h, y as f32]);
                normals.push([0.0, 1.0, 0.0]);
                // Vary colour a little so de-interleave correctness is visible.
                colors.push([x as f32 / w as f32, y as f32 / w as f32, 0.5, 1.0]);
            }
        }
        let mut indices = Vec::with_capacity(n * n * 6);
        for y in 0..n {
            for x in 0..n {
                let i0 = (y * w + x) as u32;
                let i1 = i0 + 1;
                let i2 = i0 + w as u32;
                let i3 = i2 + 1;
                indices.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
            }
        }
        (positions, normals, colors, indices)
    }

    #[test]
    fn decimate_reduces_tris_and_compacts_vertices() {
        // 100×100 quads = 20 000 tris, 10 201 verts — well above the budget, so
        // it must actually simplify AND compact.
        let (mut p, mut nrm, mut c, mut idx) = dense_grid(100);
        let orig_tris = idx.len() / 3;
        let orig_verts = p.len();
        assert!(orig_tris > DECIMATE_MIN_TRIS);

        decimate_merged(&mut p, &mut nrm, &mut c, &mut idx);

        // Index buffer is a valid triangle list and strictly smaller.
        assert_eq!(idx.len() % 3, 0, "result must stay a triangle list");
        let new_tris = idx.len() / 3;
        assert!(new_tris >= 1, "must keep at least one triangle");
        assert!(
            new_tris < orig_tris,
            "expected fewer tris: {new_tris} !< {orig_tris}"
        );
        // Budget is min(orig/10, MAX) — for 20k tris that's MAX=2000; sloppy can
        // overshoot a little, so allow generous slack but require real reduction.
        assert!(
            new_tris <= DECIMATE_MAX_TRIS * 2,
            "tris {new_tris} should be near the budget"
        );

        // Vertex buffer shrank (unreferenced verts dropped) and all three
        // attribute arrays stay length-consistent.
        assert!(
            p.len() < orig_verts,
            "expected fewer verts: {} !< {orig_verts}",
            p.len()
        );
        assert_eq!(p.len(), nrm.len(), "positions/normals must stay aligned");
        assert_eq!(p.len(), c.len(), "positions/colors must stay aligned");

        // Every index references a real (compact) vertex — no dangling refs.
        let vc = p.len() as u32;
        for &i in &idx {
            assert!(i < vc, "index {i} out of range for {vc} verts");
        }
    }

    #[test]
    fn decimate_skips_tiny_mesh_unchanged() {
        // A handful of tris (below DECIMATE_MIN_TRIS) must pass through verbatim
        // — no simplification, no remap, byte-identical buffers.
        let (mut p, mut nrm, mut c, mut idx) = dense_grid(4); // 32 tris, 25 verts
        assert!(idx.len() / 3 <= DECIMATE_MIN_TRIS);
        let (p0, n0, c0, i0) = (p.clone(), nrm.clone(), c.clone(), idx.clone());

        decimate_merged(&mut p, &mut nrm, &mut c, &mut idx);

        assert_eq!(p, p0, "tiny-mesh positions must be untouched");
        assert_eq!(nrm, n0, "tiny-mesh normals must be untouched");
        assert_eq!(c, c0, "tiny-mesh colors must be untouched");
        assert_eq!(idx, i0, "tiny-mesh indices must be untouched");
    }

    #[test]
    fn decimate_is_robust_to_malformed_input() {
        // These inputs are ABOVE the tiny-mesh floor (so they pass the first
        // skip-guard) but malformed — they must hit the well-formedness guards
        // and be left untouched, never fed to the meshopt FFI.

        // (a) Index count not a multiple of 3. Start from a real grid (well over
        // the floor) then truncate one index so it's no longer a triangle list.
        let (mut p, mut nrm, mut c, mut idx) = dense_grid(40); // 3200 tris
        idx.pop(); // now len % 3 == 2
        assert!(idx.len() / 3 > DECIMATE_MIN_TRIS);
        assert_ne!(idx.len() % 3, 0);
        let before = idx.clone();
        decimate_merged(&mut p, &mut nrm, &mut c, &mut idx);
        assert_eq!(idx, before, "non-triangle-list index count must be left as-is");

        // (b) Mismatched attribute lengths (normals shorter than positions).
        let (mut p, mut nrm, mut c, mut idx) = dense_grid(40);
        nrm.pop(); // normals.len() != positions.len()
        let before = idx.clone();
        decimate_merged(&mut p, &mut nrm, &mut c, &mut idx);
        assert_eq!(idx, before, "mismatched attribute length must be left as-is");
    }
}

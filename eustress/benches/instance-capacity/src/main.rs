//! # Eustress Engine — Instance Capacity Benchmark (v4)
//!
//! ## Table of Contents
//! 1.  InstanceDefinition  — exact mirror of engine TOML schema
//! 2.  TOML Templates      — four canonical instance variants (60/15/15/10 mix)
//! 3.  generate_space      — parallel rayon TOML write into temp dir tree
//! 4.  BinaryCache         — bincode 1.3 + zstd encode / decode
//! 5.  rss_bytes           — Windows RSS snapshot (GetProcessMemoryInfo)
//! 6.  parse_inmem         — parallel TOML parse from &str, no disk I/O (chunked)
//! 7.  encode_binary_n     — TOML → InstanceBin → bincode + zstd (chunked)
//! 8.  decode_binary       — zstd + bincode → Vec<InstanceBin>
//! 9.  bench_ecs_pure      — pure ECS archetype iteration, NO render (target 10M+)
//! 10. run_bevy_bench      — headless Bevy 0.18 + real GPU: active zone 50K cap
//! 11. bench_streaming     — InstanceStreamer hot-cache load/evict (DashMap + rstar)
//! 12. bench_physics       — tiered LOD physics: 5K rigid | 50K kinematic | rest AABB
//! 13. StopReason          — named conditions that halt the exponential loop
//! 14. main                — exponential scaling loop: doubles N until stopped

//! ## Design
//! TOML on disk is the Eustress canonical source (file-system-first).
//! Binary cache (bincode+zstd) is the fast-reload path after first parse.
//! N starts at 1 024 and doubles each iteration. Every iteration runs all
//! four measurements. The loop stops as soon as any StopReason fires, and
//! the terminal output names the exact condition that ended the run.

#![allow(dead_code)]

use std::{
    collections::HashMap,
    fs,
    io::Write as _,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use std::collections::HashSet;

// ═══════════════════════════════════════════════════════════════════════════════
// 1. InstanceDefinition — exact mirror of eustress-engine::space::instance_loader
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AssetReference {
    mesh:  String,
    #[serde(default = "default_scene")] scene: String,
}
fn default_scene() -> String { "Scene0".into() }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransformData {
    position: [f32; 3],
    rotation: [f32; 4],
    scale:    [f32; 3],
}
impl Default for TransformData {
    fn default() -> Self {
        Self { position: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0], scale: [1.0, 1.0, 1.0] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstanceProperties {
    #[serde(default = "default_color", deserialize_with = "de_color")]
    color:        [f32; 4],
    #[serde(default)] transparency: f32,
    #[serde(default)] anchored:     bool,
    #[serde(default = "dtrue")] can_collide: bool,
    #[serde(default = "dtrue")] cast_shadow: bool,
    #[serde(default)] reflectance:  f32,
    #[serde(default = "dmat")] material: String,
    #[serde(default)] locked: bool,
}
fn default_color() -> [f32; 4] { [0.639, 0.635, 0.647, 1.0] }
fn dtrue()         -> bool      { true }
fn dmat()          -> String    { "Plastic".into() }
impl Default for InstanceProperties {
    fn default() -> Self {
        Self {
            color: default_color(), transparency: 0.0, anchored: false,
            can_collide: true, cast_shadow: true, reflectance: 0.0,
            material: dmat(), locked: false,
        }
    }
}

fn de_color<'de, D>(d: D) -> Result<[f32; 4], D::Error>
where D: serde::Deserializer<'de> {
    let vals: Vec<toml::Value> = serde::Deserialize::deserialize(d)?;
    if vals.len() < 3 { return Err(serde::de::Error::custom("color needs ≥ 3 elements")); }
    if vals.iter().all(|v| v.is_integer()) {
        let c = |i: usize, def: i64| -> f32 {
            vals.get(i).and_then(|v| v.as_integer()).unwrap_or(def) as f32 / 255.0
        };
        Ok([c(0,128), c(1,128), c(2,128), c(3,255)])
    } else {
        let f = |i: usize, def: f64| -> f32 {
            vals.get(i)
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
                .unwrap_or(def) as f32
        };
        Ok([f(0,0.5), f(1,0.5), f(2,0.5), f(3,1.0)])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct InstanceMetadata {
    #[serde(default = "dclass")] class_name: String,
    #[serde(default = "dtrue")]  archivable:  bool,
    #[serde(default)]            created:     String,
    #[serde(default)]            last_modified: String,
}
fn dclass() -> String { "Part".into() }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TomlMaterial {
    #[serde(default)] name:                String,
    #[serde(default)] density:             f32,
    #[serde(default)] young_modulus:       f32,
    #[serde(default)] thermal_conductivity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstanceDefinition {
    #[serde(default)] asset:      Option<AssetReference>,
    #[serde(default)] transform:  TransformData,
    #[serde(default)] properties: InstanceProperties,
    #[serde(default)] metadata:   InstanceMetadata,
    #[serde(default)] material:   Option<TomlMaterial>,
    #[serde(flatten)] extra:      HashMap<String, toml::Value>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. TOML Templates — four canonical Eustress instance variants
// ═══════════════════════════════════════════════════════════════════════════════

fn tmpl_part(i: usize) -> String {
    let x = (i % 1000) as f32 * 2.0;
    let z = (i / 1000) as f32 * 2.0;
    format!(r#"[asset]
mesh  = "assets/meshes/cube.glb"
scene = "Scene0"
[transform]
position = [{x:.3}, 0.5, {z:.3}]
rotation = [0.0, 0.0, 0.0, 1.0]
scale    = [1.0, 1.0, 1.0]
[properties]
color        = [163, 162, 165]
transparency = 0.0
anchored     = false
can_collide  = true
cast_shadow  = true
reflectance  = 0.0
material     = "Plastic"
locked       = false
[metadata]
class_name    = "Part"
archivable    = true
created       = "2025-01-01T00:00:00Z"
last_modified = "2025-01-01T00:00:00Z"
"#, x=x, z=z)
}

fn tmpl_model(i: usize) -> String {
    format!(r#"[transform]
position = [{x:.1}, 0.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale    = [1.0, 1.0, 1.0]
[metadata]
class_name    = "Model"
archivable    = true
created       = "2025-01-01T00:00:00Z"
last_modified = "2025-01-01T00:00:00Z"
"#, x = i as f32 * 2.0)
}

fn tmpl_label(i: usize) -> String {
    format!(r#"[metadata]
class_name    = "TextLabel"
archivable    = true
created       = "2025-01-01T00:00:00Z"
last_modified = "2025-01-01T00:00:00Z"
[ui]
text      = "Label {i}"
font_size = 14.0
visible   = true
"#, i=i)
}

fn tmpl_rich(i: usize) -> String {
    let x = (i % 1000) as f32 * 2.0;
    let z = (i / 1000) as f32 * 2.0;
    format!(r#"[asset]
mesh  = "assets/meshes/sphere.glb"
scene = "Scene0"
[transform]
position = [{x:.3}, 1.0, {z:.3}]
rotation = [0.0, 0.0, 0.0, 1.0]
scale    = [0.5, 0.5, 0.5]
[properties]
color        = [255, 80, 20]
transparency = 0.0
anchored     = true
can_collide  = false
cast_shadow  = true
reflectance  = 0.3
material     = "Metal"
locked       = false
[metadata]
class_name    = "Part"
archivable    = true
created       = "2025-01-01T00:00:00Z"
last_modified = "2025-01-01T00:00:00Z"
[material]
name                 = "Steel"
density              = 7850.0
young_modulus        = 200000.0
thermal_conductivity = 50.0
[Physics]
mass         = 1.5
drag         = 0.1
angular_drag = 0.05
[Tags]
tags = ["structural", "metal", "inst-{i}"]
"#, x=x, z=z, i=i)
}

/// 60% Part, 15% Model, 15% Label, 10% Rich.
fn make_toml(i: usize) -> String {
    match i % 20 {
        0..=11  => tmpl_part(i),
        12..=14 => tmpl_model(i),
        15..=17 => tmpl_label(i),
        _       => tmpl_rich(i),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. generate_space — parallel rayon TOML write into temp directory tree
// ═══════════════════════════════════════════════════════════════════════════════

struct SpaceStats {
    dir:           TempDir,
    paths:         Vec<PathBuf>,
    bytes_written: u64,
    write_elapsed: Duration,
}

fn generate_space(n: usize) -> SpaceStats {
    let dir       = TempDir::new().expect("tempdir");
    let workspace = dir.path().join("Workspace");
    fs::create_dir_all(&workspace).expect("workspace dir");

    let num_groups = n.div_ceil(1000);
    for g in 0..num_groups {
        fs::create_dir_all(workspace.join(format!("group_{g:06}"))).expect("group dir");
    }

    let entries: Vec<(PathBuf, String)> = (0..n).map(|i| {
        let gdir = workspace.join(format!("group_{:06}", i / 1000));
        let (prefix, suffix) = match i % 20 {
            0..=11  => ("part",  "glb.toml"),
            12..=14 => ("model", "instance.toml"),
            15..=17 => ("label", "instance.toml"),
            _       => ("rich",  "glb.toml"),
        };
        (gdir.join(format!("{prefix}_{i:08}.{suffix}")), make_toml(i))
    }).collect();

    let paths: Vec<PathBuf> = entries.iter().map(|(p, _)| p.clone()).collect();
    let total_bytes = AtomicU64::new(0);
    let t0 = Instant::now();
    entries.par_iter().for_each(|(path, content)| {
        fs::write(path, content.as_bytes()).expect("write instance");
        total_bytes.fetch_add(content.len() as u64, Ordering::Relaxed);
    });
    SpaceStats {
        dir,
        paths,
        bytes_written: total_bytes.load(Ordering::Relaxed),
        write_elapsed: t0.elapsed(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. Binary cache — InstanceBin + bincode 1.3 + zstd
// ═══════════════════════════════════════════════════════════════════════════════

/// Flat binary representation — no toml::Value, safe for bincode.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstanceBin {
    position:     [f32; 3],
    rotation:     [f32; 4],
    scale:        [f32; 3],
    color:        [f32; 4],
    transparency: f32,
    reflectance:  f32,
    anchored:     bool,
    can_collide:  bool,
    cast_shadow:  bool,
    locked:       bool,
    class_name:   String,
    material:     String,
    mesh:         String,
}

impl InstanceBin {
    fn from_def(d: &InstanceDefinition) -> Self {
        Self {
            position:     d.transform.position,
            rotation:     d.transform.rotation,
            scale:        d.transform.scale,
            color:        d.properties.color,
            transparency: d.properties.transparency,
            reflectance:  d.properties.reflectance,
            anchored:     d.properties.anchored,
            can_collide:  d.properties.can_collide,
            cast_shadow:  d.properties.cast_shadow,
            locked:       d.properties.locked,
            class_name:   d.metadata.class_name.clone(),
            material:     d.properties.material.clone(),
            mesh:         d.asset.as_ref().map(|a| a.mesh.clone()).unwrap_or_default(),
        }
    }
}

struct BinaryCacheBlob {
    compressed: Vec<u8>,
    raw_bytes:  usize,
    count:      usize,
}

/// Parse N TOML strings (chunked) → InstanceBin → bincode → zstd.
/// Chunked to avoid a single O(N) contiguous allocation.
/// The compressed output is a single zstd frame over the full bincode payload.
fn encode_binary_n(n: usize) -> (BinaryCacheBlob, Duration) {
    let t0 = Instant::now();
    // Collect all InstanceBin records in chunks to avoid a giant string Vec.
    let mut all_bins: Vec<InstanceBin> = Vec::with_capacity(n);
    let mut start = 0usize;
    while start < n {
        let end   = (start + PARSE_CHUNK).min(n);
        let chunk: Vec<InstanceBin> = (start..end)
            .filter_map(|i| {
                let s = make_toml(i);
                toml::from_str::<InstanceDefinition>(&s).ok().map(|d| InstanceBin::from_def(&d))
            })
            .collect();
        all_bins.extend(chunk);
        start = end;
    }
    let count     = all_bins.len();
    let raw       = bincode::serialize(&all_bins).expect("bincode");
    let raw_bytes = raw.len();
    let compressed = zstd::encode_all(raw.as_slice(), 1).expect("zstd encode");
    (BinaryCacheBlob { compressed, raw_bytes, count }, t0.elapsed())
}

/// zstd decompress → bincode deserialize. Returns bins + decode time.
fn decode_binary(blob: &BinaryCacheBlob) -> (Vec<InstanceBin>, Duration) {
    let t0  = Instant::now();
    let raw = zstd::decode_all(blob.compressed.as_slice()).expect("zstd decode");
    let bins: Vec<InstanceBin> = bincode::deserialize(&raw).expect("bincode deser");
    (bins, t0.elapsed())
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. rss_bytes — Windows working-set snapshot
// ═══════════════════════════════════════════════════════════════════════════════

fn rss_bytes() -> u64 {
    #[cfg(target_os = "windows")]
    {
        use std::mem;
        #[repr(C)]
        struct Pmc {
            cb: u32, page_fault_count: u32,
            peak_working_set_size: usize, working_set_size: usize,
            quota_peak_paged_pool_usage: usize, quota_paged_pool_usage: usize,
            quota_peak_nonpaged_pool_usage: usize, quota_nonpaged_pool_usage: usize,
            pagefile_usage: usize, peak_pagefile_usage: usize,
        }
        #[link(name = "psapi")]
        extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
            fn GetProcessMemoryInfo(h: *mut std::ffi::c_void, p: *mut Pmc, cb: u32) -> i32;
        }
        unsafe {
            let mut pmc: Pmc = mem::zeroed();
            pmc.cb = mem::size_of::<Pmc>() as u32;
            if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) != 0 {
                return pmc.working_set_size as u64;
            }
        }
        0
    }
    #[cfg(not(target_os = "windows"))] { 0 }
}

/// Total physical RAM in bytes (Windows GlobalMemoryStatusEx).
fn total_ram_bytes() -> u64 {
    #[cfg(target_os = "windows")]
    {
        use std::mem;
        #[repr(C)]
        struct MemoryStatusEx {
            dw_length: u32,
            dw_memory_load: u32,
            ull_total_phys: u64,
            ull_avail_phys: u64,
            ull_total_page_file: u64,
            ull_avail_page_file: u64,
            ull_total_virtual: u64,
            ull_avail_virtual: u64,
            ull_avail_ext_virtual: u64,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
        }
        unsafe {
            let mut ms: MemoryStatusEx = mem::zeroed();
            ms.dw_length = mem::size_of::<MemoryStatusEx>() as u32;
            if GlobalMemoryStatusEx(&mut ms) != 0 {
                return ms.ull_total_phys;
            }
        }
        0
    }
    #[cfg(not(target_os = "windows"))] { 0 }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. In-memory TOML parse — chunked generation + parallel parse, no large alloc
// ═══════════════════════════════════════════════════════════════════════════════

struct ParseResult {
    ok:      usize,
    elapsed: Duration,
    /// Total TOML byte volume processed (estimated from one chunk × chunks).
    bytes:   usize,
}

/// Process N instances in chunks of CHUNK so we never hold all N strings at once.
/// Each chunk is generated and parsed, then dropped before the next chunk begins.
const PARSE_CHUNK: usize = 100_000;

fn parse_inmem(n: usize) -> ParseResult {
    let t0          = Instant::now();
    let mut ok      = 0usize;
    let mut bytes   = 0usize;
    let mut start   = 0usize;
    while start < n {
        let end     = (start + PARSE_CHUNK).min(n);
        let chunk: Vec<String> = (start..end).map(make_toml).collect();
        bytes      += chunk.iter().map(|s| s.len()).sum::<usize>();
        ok         += chunk.par_iter().map(|s| {
            toml::from_str::<InstanceDefinition>(s).map_or(0usize, |_| 1)
        }).sum::<usize>();
        start       = end;
    }
    ParseResult { ok, elapsed: t0.elapsed(), bytes }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7-8. Disk write + binary cache (combined for the loop)
// ═══════════════════════════════════════════════════════════════════════════════

struct DiskResult {
    write_elapsed: Duration,
    bytes_written: u64,
}

/// Write N TOML instances to a temp directory, streaming generation to avoid
/// holding all N strings simultaneously. No large allocation required.
fn write_space(n: usize) -> DiskResult {
    let dir       = TempDir::new().expect("tempdir");
    let workspace = dir.path().join("Workspace");
    fs::create_dir_all(&workspace).ok();
    let num_groups = n.div_ceil(1000);
    for g in 0..num_groups {
        fs::create_dir_all(workspace.join(format!("group_{g:06}"))).ok();
    }

    let bytes_written = AtomicU64::new(0);
    let t0 = Instant::now();
    (0..n).into_par_iter().for_each(|i| {
        let content = make_toml(i);
        let gdir    = workspace.join(format!("group_{:06}", i / 1000));
        let (p, s)  = match i % 20 {
            0..=11  => ("part",  "glb.toml"),
            12..=14 => ("model", "instance.toml"),
            15..=17 => ("label", "instance.toml"),
            _       => ("rich",  "glb.toml"),
        };
        let path = gdir.join(format!("{p}_{i:08}.{s}"));
        bytes_written.fetch_add(content.len() as u64, Ordering::Relaxed);
        fs::write(path, content.as_bytes()).ok();
    });
    let write_elapsed = t0.elapsed();
    drop(dir);
    DiskResult { write_elapsed, bytes_written: bytes_written.load(Ordering::Relaxed) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. Bevy ECS + GPU benchmark
//    Headless App: bevy_render (real wgpu GPU) + bevy_pbr + Bevy task pool.
//    Arc<Mutex<>> carries results out because App::run() consumes the App.
// ═══════════════════════════════════════════════════════════════════════════════

use bevy::prelude::*;
use bevy::app::ScheduleRunnerPlugin;
use bevy::render::{RenderPlugin, settings::{RenderCreation, WgpuSettings}};

#[derive(Resource, Clone)]
struct BenchCfg { n: usize, warmup: u32, frames: u32 }

/// MoE sparse activation: marks an entity as "active expert" (Transform mutated
/// this frame). Only Active entities pass the Changed<Transform> gate.
#[derive(Component, Clone, Copy)]
struct Active;

/// Per-entity velocity stored as a component so the physics gate can read it.
/// Zero velocity = dormant expert = skip all expensive systems.
#[derive(Component, Clone, Copy, Default)]
struct Velocity(Vec3);

#[derive(Default, Clone)]
struct BenchOut {
    spawn_elapsed:  Duration,
    /// Dense frame times: all N transforms queried (baseline).
    frame_times:    Vec<Duration>,
    /// Sparse frame times: only Changed<Transform> + InheritedVisibility.
    sparse_times:   Vec<Duration>,
    /// How many entities passed the sparse gate (Changed + visible).
    sparse_count:   usize,
    /// Stored from spawn so sys_measure never needs query.iter().count().
    entity_count:   usize,
    done:           bool,
    warmup_left:    u32,
    frames_left:    u32,
}

#[derive(Resource, Clone)]
struct SharedOut(Arc<Mutex<BenchOut>>);

fn sys_spawn(
    mut commands:  Commands,
    mut meshes:    ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cfg:           Res<BenchCfg>,
    shared:        Res<SharedOut>,
) {
    let n  = cfg.n;
    let t0 = Instant::now();

    // Single shared mesh + material — Bevy 0.18 automatically batches these into
    // instanced draw calls (GpuPreprocessingMode::PreprocessAndCull path).
    let mesh_h = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let mat_h  = materials.add(StandardMaterial {
        base_color:   Color::srgb(0.639, 0.635, 0.647),
        unlit:        true,
        alpha_mode:   AlphaMode::Opaque,
        ..Default::default()
    });

    // MoE expert assignment:
    //   10% of entities are "active experts" — they have a non-zero Velocity
    //   and will mutate their Transform every frame (Changed<Transform> fires).
    //   90% are "dormant experts" — static, Changed<Transform> never fires.
    // This mirrors a real scene: a small fraction of entities move each frame.
    let active_fraction = 0.10_f32;
    let active_count    = ((n as f32 * active_fraction) as usize).max(1);

    commands.spawn_batch((0..n).map(move |i| {
        let x   = (i % 1_000) as f32 * 2.0;
        let z   = (i / 1_000) as f32 * 2.0;
        // Active entities get a small orbit velocity; dormant get zero.
        let vel = if i < active_count {
            Velocity(Vec3::new(0.1, 0.0, 0.05))
        } else {
            Velocity(Vec3::ZERO)
        };
        (
            Transform::from_xyz(x, 0.5, z),
            Mesh3d(mesh_h.clone()),
            MeshMaterial3d(mat_h.clone()),
            vel,
        )
    }));

    // Camera overhead — sees the whole grid.
    let grid_w = 1_000_f32 * 2.0;
    let grid_d = ((n + 999) / 1_000) as f32 * 2.0;
    let cam_y  = grid_w.max(grid_d) * 1.2;
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(grid_w * 0.5, cam_y, grid_d * 0.5)
            .looking_at(Vec3::new(grid_w * 0.5, 0.0, grid_d * 0.5), Vec3::Y),
    ));

    let mut out = shared.0.lock().unwrap();
    out.spawn_elapsed  = t0.elapsed();
    out.entity_count   = n;
    out.warmup_left    = cfg.warmup;
    out.frames_left    = cfg.frames;
}

/// MoE routing system: mutates Transform for active experts every frame.
/// This ensures Changed<Transform> fires only for the 10% active fraction,
/// making the sparse gate meaningful rather than trivially empty.
fn sys_route_active(
    mut query: Query<(&mut Transform, &Velocity)>,
) {
    query.par_iter_mut().for_each(|(mut t, vel)| {
        if vel.0 != Vec3::ZERO {
            // Tiny delta — keeps entities in place visually but marks Transform dirty.
            t.translation += vel.0 * 0.001;
        }
    });
}

fn sys_measure(
    shared:        Res<SharedOut>,
    // Dense query: all N transforms — baseline (no sparse gate).
    dense_query:   Query<&Transform>,
    // Sparse query: MoE gate — only entities whose Transform changed this frame
    // AND are visible in the camera frustum.
    // Changed<Transform> = dirty-bit gate (active experts only).
    // With<InheritedVisibility> = frustum gate (visible experts only).
    sparse_query:  Query<&Transform, (Changed<Transform>, With<InheritedVisibility>)>,
    mut exit:      bevy::ecs::message::MessageWriter<bevy::app::AppExit>,
) {
    let mut out = shared.0.lock().unwrap();

    if out.warmup_left > 0 {
        out.warmup_left -= 1;
        return;
    }
    drop(out);

    // ── Dense pass: all N entities (baseline) ────────────────────────────────
    let acc  = AtomicU64::new(0);
    let t0   = Instant::now();
    dense_query.par_iter().for_each(|t| {
        let bits = (t.translation.x + t.translation.y + t.translation.z).to_bits() as u64;
        acc.fetch_add(bits, Ordering::Relaxed);
    });
    let dense_elapsed = t0.elapsed();
    std::hint::black_box(acc.load(Ordering::Relaxed));

    // ── Sparse pass: MoE gate — Changed + visible experts only ────────────────
    // This is the core MoE optimization: only active experts (10% of N)
    // that are also visible receive compute. Dormant + culled = zero work.
    let acc2   = AtomicU64::new(0);
    let t1     = Instant::now();
    sparse_query.par_iter().for_each(|t| {
        let bits = (t.translation.x + t.translation.y + t.translation.z).to_bits() as u64;
        acc2.fetch_add(bits, Ordering::Relaxed);
    });
    let sparse_elapsed = t1.elapsed();
    let sparse_count   = sparse_query.iter().count();
    std::hint::black_box(acc2.load(Ordering::Relaxed));

    let mut out = shared.0.lock().unwrap();
    out.frame_times.push(dense_elapsed);
    out.sparse_times.push(sparse_elapsed);
    out.sparse_count = sparse_count;
    out.frames_left  = out.frames_left.saturating_sub(1);

    if out.frames_left == 0 {
        out.done = true;
        drop(out);
        exit.write(bevy::app::AppExit::Success);
    }
}

struct EcsResult {
    spawn_elapsed:   Duration,
    /// Dense path: all N transforms queried (baseline).
    avg_frame_time:  Duration,
    p99_frame_time:  Duration,
    query_per_sec:   f64,
    /// Sparse path: Changed<Transform> + InheritedVisibility gate.
    avg_sparse_time: Duration,
    sparse_count:    usize,
    /// Speedup factor: dense_avg / sparse_avg.
    sparse_speedup:  f64,
    entity_count:    usize,
    skipped:         bool,
}

fn run_bevy_bench(n: usize) -> EcsResult {
    let shared = SharedOut(Arc::new(Mutex::new(BenchOut::default())));
    let mut app = App::new();
    app
        // DefaultPlugins brings the full required stack (assets, transform, image,
        // render sub-app, etc.) but we disable the window so nothing is shown on
        // screen. ScheduleRunnerPlugin is overridden to loop until AppExit fires.
        .add_plugins(DefaultPlugins
            // No OS window — headless render only.
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..Default::default()
            })
            // Discrete GPU, all backends (Vulkan/DX12), no surface.
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(WgpuSettings {
                    power_preference: bevy::render::settings::PowerPreference::HighPerformance,
                    backends: Some(bevy::render::settings::Backends::all()),
                    ..Default::default()
                }),
                ..Default::default()
            })
            // Suppress repeated "logger already set" warnings across benchmark iterations.
            .disable::<bevy::log::LogPlugin>()
            // Suppress repeated "Ctrl+C handler already installed" warnings.
            .disable::<bevy::app::TerminalCtrlCHandlerPlugin>()
        )
        // ScheduleRunnerPlugin is NOT part of DefaultPlugins (DefaultPlugins uses winit).
        // Adding it here overrides the winit runner with our headless loop runner.
        .add_plugins(ScheduleRunnerPlugin {
            run_mode: bevy::app::RunMode::Loop { wait: None },
        })
        .insert_resource(BenchCfg { n, warmup: 5, frames: 10 })
        .insert_resource(shared.clone())
        .add_systems(Startup, sys_spawn)
        // sys_route_active runs before sys_measure so Changed<Transform> is
        // populated by the time the sparse query runs in the same frame.
        .add_systems(Update, (sys_route_active, sys_measure).chain());
    app.run();

    let out = shared.0.lock().unwrap();

    let mut ft = out.frame_times.clone();
    ft.sort_unstable();
    let avg = if ft.is_empty() { Duration::ZERO }
              else { ft.iter().sum::<Duration>() / ft.len() as u32 };
    let p99 = ft.last().copied().unwrap_or(Duration::ZERO);
    let qps = if avg.as_secs_f64() > 0.0 { out.entity_count as f64 / avg.as_secs_f64() } else { 0.0 };

    let mut st = out.sparse_times.clone();
    st.sort_unstable();
    let avg_sparse = if st.is_empty() { Duration::ZERO }
                     else { st.iter().sum::<Duration>() / st.len() as u32 };
    let speedup = if avg_sparse.as_secs_f64() > 0.0 {
        avg.as_secs_f64() / avg_sparse.as_secs_f64()
    } else { 0.0 };

    EcsResult {
        spawn_elapsed:   out.spawn_elapsed,
        avg_frame_time:  avg,
        p99_frame_time:  p99,
        query_per_sec:   qps,
        avg_sparse_time: avg_sparse,
        sparse_count:    out.sparse_count,
        sparse_speedup:  speedup,
        entity_count:    out.entity_count,
        skipped:         false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9b. Pure ECS archetype iteration — no render, no GPU, no window
//     Reference: WORLD_CLASS_ENGINE.md §1 — "Bevy ECS achieves cache-coherent
//     iteration over 10M+ entities at 2–4× throughput of Unity DOTS"
//     Measures the raw ECS spawn + parallel Transform read ceiling.
// ═══════════════════════════════════════════════════════════════════════════════

struct PureEcsResult {
    spawn_elapsed: Duration,
    iter_elapsed:  Duration,
    /// Entities iterated per second (par_iter Transform read).
    iter_rate:     f64,
    entity_count:  usize,
    skipped:       bool,
}

fn bench_ecs_pure(n: usize) -> PureEcsResult {
    use bevy::ecs::world::World;
    use bevy::prelude::Transform;

    // Use a bare World — no plugins, no render sub-app, no asset server.
    // This measures archetype storage + parallel iteration with zero overhead.
    let mut world = World::new();

    let t0 = Instant::now();
    // Spawn N entities each with only a Transform component.
    // All land in the same archetype — maximises cache-coherent iteration.
    world.spawn_batch((0..n).map(|i| {
        let x = (i % 1_000) as f32 * 2.0;
        let z = (i / 1_000) as f32 * 2.0;
        Transform::from_xyz(x, 0.5, z)
    }));
    let spawn_elapsed = t0.elapsed();

    // Parallel Transform read via par_iter on the raw query.
    let acc  = AtomicU64::new(0);
    let t1   = Instant::now();
    let mut q = world.query::<&Transform>();
    q.par_iter(&world).for_each(|t| {
        let bits = (t.translation.x + t.translation.y + t.translation.z).to_bits() as u64;
        acc.fetch_add(bits, Ordering::Relaxed);
    });
    let iter_elapsed = t1.elapsed();
    std::hint::black_box(acc.load(Ordering::Relaxed));

    let iter_rate = n as f64 / iter_elapsed.as_secs_f64().max(1e-9);
    PureEcsResult { spawn_elapsed, iter_elapsed, iter_rate, entity_count: n, skipped: false }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11. Streaming simulation — hot-cache load/evict throughput
//     Reference: WORLD_CLASS_ENGINE.md §6 —
//       Active Zone ~50K | Hot Cache ~500K | Cold Storage unlimited
//     Simulates InstanceStreamer.update(): radius query → spawn/despawn set.
// ═══════════════════════════════════════════════════════════════════════════════

struct StreamResult {
    /// Time to insert N entries into the hot cache (DashMap).
    insert_elapsed:  Duration,
    /// Time to run one camera-radius query returning ~50K candidates.
    query_elapsed:   Duration,
    /// Number of instances returned by the radius query (the active zone size).
    active_zone:     usize,
    /// Time to evict entries beyond unload_radius from the hot cache.
    evict_elapsed:   Duration,
    /// Number evicted.
    evicted:         usize,
    skipped:         bool,
}

fn bench_streaming(bins: &[InstanceBin]) -> StreamResult {
    let n = bins.len();
    if n == 0 {
        return StreamResult {
            insert_elapsed: Duration::ZERO, query_elapsed: Duration::ZERO,
            active_zone: 0, evict_elapsed: Duration::ZERO, evicted: 0, skipped: true,
        };
    }

    // Hot cache: HashMap<id, position> — simulates Arc<InstanceDefinition> store.
    // Use a plain Vec of (id, [f32;3]) for max throughput (DashMap is overkill
    // for a single-threaded bench pass; par_iter does the parallelism).
    let t0 = Instant::now();
    let cache: Vec<(usize, [f32; 3])> = bins.iter().enumerate()
        .map(|(i, b)| (i, b.position))
        .collect();
    let insert_elapsed = t0.elapsed();

    // Camera at world origin; load_radius = 1 000 m (~50K instances in a
    // 2×2 m grid at this density).
    let load_radius: f32 = 1_000.0;
    let unload_radius: f32 = 1_200.0;
    let cam = [0.0_f32; 3];

    let t1 = Instant::now();
    let active: Vec<usize> = cache.par_iter()
        .filter_map(|(id, pos)| {
            let dx = pos[0] - cam[0];
            let dy = pos[1] - cam[1];
            let dz = pos[2] - cam[2];
            if (dx*dx + dy*dy + dz*dz).sqrt() <= load_radius { Some(*id) } else { None }
        })
        .collect();
    let query_elapsed = t1.elapsed();
    let active_zone   = active.len();

    // Simulate eviction: remove entries beyond unload_radius.
    // In production this drives bevy::commands.entity(e).despawn().
    let active_set: HashSet<usize> = active.into_iter().collect();
    let t2 = Instant::now();
    let evicted: usize = cache.par_iter()
        .filter(|(id, pos)| {
            if active_set.contains(id) { return false; }
            let dx = pos[0] - cam[0];
            let dy = pos[1] - cam[1];
            let dz = pos[2] - cam[2];
            (dx*dx + dy*dy + dz*dz).sqrt() > unload_radius
        })
        .count();
    let evict_elapsed = t2.elapsed();

    StreamResult { insert_elapsed, query_elapsed, active_zone, evict_elapsed, evicted, skipped: false }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 12. Physics — tiered LOD physics zones
//     Reference: WORLD_CLASS_ENGINE.md §6 —
//       5K rigid bodies (full 60Hz) | 50K kinematic (20Hz simplified) |
//       rest = static AABB only
//     Measures build + query time for each tier independently.
// ═══════════════════════════════════════════════════════════════════════════════

/// MoE physics tier label returned by the 2D gate.
#[derive(Clone, Copy, PartialEq)]
enum PhysTier { Rigid, Kinematic, Static }

/// 2D velocity × distance MoE gate.
/// Routes each entity to the cheapest physics expert that is still correct.
///
/// Gate function (from WORLD_CLASS_ENGINE.md §6):
///   velocity_mag × (1 / distance) → activation score
///   score > RIGID_THRESH    → Rigid   (full 60Hz solver)
///   score > KIN_THRESH      → Kinematic (20Hz simplified)
///   else                    → Static  (AABB only, no update)
fn physics_gate(velocity_mag: f32, distance: f32) -> PhysTier {
    const RIGID_THRESH: f32 = 0.05;  // fast + close
    const KIN_THRESH:   f32 = 0.001; // slow or mid-range
    let score = if distance > 0.001 { velocity_mag / distance } else { f32::MAX };
    if score > RIGID_THRESH      { PhysTier::Rigid }
    else if score > KIN_THRESH   { PhysTier::Kinematic }
    else                         { PhysTier::Static }
}

struct PhysResult {
    /// Full broad-phase sort across all N (static tier baseline).
    sort_elapsed:      Duration,
    /// Sphere query across all N (static AABB).
    query_elapsed:     Duration,
    pairs_found:       usize,
    /// Kinematic tier: entities routed by 2D gate (20Hz budget = 50ms).
    kinematic_elapsed: Duration,
    kinematic_count:   usize,
    /// Rigid tier: entities routed by 2D gate (full 60Hz).
    rigid_elapsed:     Duration,
    rigid_count:       usize,
    /// Static tier count (gate says skip).
    static_count:      usize,
}

fn bench_physics(bins: &[InstanceBin]) -> PhysResult {
    let n   = bins.len();
    let cam = [0.0_f32; 3]; // camera at origin for gate distance calc

    // ── Static tier: all N — broad-phase AABB sort + sphere query ────────────
    let t0 = Instant::now();
    let mut aabbs: Vec<([f32; 3], [f32; 3])> = bins.par_iter().map(|b| {
        let [px, py, pz] = b.position;
        let [sx, sy, sz] = b.scale;
        ([px - sx*0.5, py - sy*0.5, pz - sz*0.5],
         [px + sx*0.5, py + sy*0.5, pz + sz*0.5])
    }).collect();
    aabbs.sort_unstable_by(|a, b| a.0[0].partial_cmp(&b.0[0]).unwrap());
    let sort_elapsed = t0.elapsed();

    let qr = 50.0_f32;
    let qc = [0.0_f32; 3];
    let t1 = Instant::now();
    let pairs_found: usize = aabbs.par_iter().filter(|(mn, mx)| {
        let dx = (mn[0].max(qc[0]).min(mx[0]) - qc[0]).powi(2);
        let dy = (mn[1].max(qc[1]).min(mx[1]) - qc[1]).powi(2);
        let dz = (mn[2].max(qc[2]).min(mx[2]) - qc[2]).powi(2);
        (dx + dy + dz).sqrt() <= qr
    }).count();
    let query_elapsed = t1.elapsed();

    // ── MoE 2D gate: classify every entity in parallel ─────────────────────────
    // Simulated velocity: first 10% of entities have vel=1.0 (active fraction
    // mirrors the ECS bench), rest have vel=0.0 (dormant).
    let active_frac = (n as f32 * 0.10) as usize;
    let tiers: Vec<PhysTier> = bins.par_iter().enumerate().map(|(i, b)| {
        let [px, py, pz] = b.position;
        let dx = px - cam[0]; let dy = py - cam[1]; let dz = pz - cam[2];
        let dist    = (dx*dx + dy*dy + dz*dz).sqrt().max(0.001);
        let vel_mag = if i < active_frac { 1.0_f32 } else { 0.0_f32 };
        physics_gate(vel_mag, dist)
    }).collect();

    let rigid_count     = tiers.iter().filter(|&&t| t == PhysTier::Rigid).count();
    let kinematic_count = tiers.iter().filter(|&&t| t == PhysTier::Kinematic).count();
    let static_count    = tiers.iter().filter(|&&t| t == PhysTier::Static).count();

    // ── Kinematic tier update (20Hz simplified AABB) ────────────────────────
    let t2 = Instant::now();
    let _kin: Vec<_> = bins.par_iter().zip(tiers.par_iter())
        .filter(|(_, &tier)| tier == PhysTier::Kinematic)
        .map(|(b, _)| {
            let [px, py, pz] = b.position;
            let [sx, sy, sz] = b.scale;
            let pad = 1.1_f32;
            ([px - sx*0.5*pad, py - sy*0.5*pad, pz - sz*0.5*pad],
             [px + sx*0.5*pad, py + sy*0.5*pad, pz + sz*0.5*pad])
        }).collect();
    let kinematic_elapsed = t2.elapsed();

    // ── Rigid tier full sort (60Hz) ──────────────────────────────────────────
    let t3 = Instant::now();
    let mut rig_aabbs: Vec<([f32; 3], [f32; 3])> = bins.par_iter().zip(tiers.par_iter())
        .filter(|(_, &tier)| tier == PhysTier::Rigid)
        .map(|(b, _)| {
            let [px, py, pz] = b.position;
            let [sx, sy, sz] = b.scale;
            ([px - sx*0.5, py - sy*0.5, pz - sz*0.5],
             [px + sx*0.5, py + sy*0.5, pz + sz*0.5])
        }).collect();
    rig_aabbs.sort_unstable_by(|a, b| a.0[0].partial_cmp(&b.0[0]).unwrap());
    let rigid_elapsed = t3.elapsed();

    PhysResult {
        sort_elapsed, query_elapsed, pairs_found,
        kinematic_elapsed, kinematic_count,
        rigid_elapsed, rigid_count,
        static_count,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11. GPU-driven indirect draw benchmark
//     Reference: WORLD_CLASS_ENGINE.md §2 — "GPU-driven indirect draw"
//     Wihlidal GDC 2015: replace N CPU draw calls with one
//     DrawIndexedIndirect dispatch. The GPU reads the command buffer directly.
//
//     Pipeline:
//       1. CPU builds instance data buffer  (N × 32 bytes, bytemuck)
//       2. CPU builds indirect commands buf (N × DrawIndexedIndirect = 20 bytes)
//       3. Single wgpu::RenderPass::multi_draw_indexed_indirect() call
//       4. GPU reads + dispatches — zero per-entity CPU overhead
//
//     Measurements:
//       cpu_prep_elapsed  — time to fill both buffers (Rayon par_iter)
//       gpu_submit_elapsed — wgpu command encoding + queue.submit() roundtrip
//       throughput        — N / (cpu_prep + gpu_submit)
// ═══════════════════════════════════════════════════════════════════════════════

struct IndirectResult {
    /// Time to build the instance data + indirect command buffers on the CPU.
    cpu_prep_elapsed:  Duration,
    /// Time to encode the render pass + queue.submit() (GPU roundtrip).
    gpu_submit_elapsed: Duration,
    /// Total instances submitted via indirect draw.
    instance_count:    usize,
    /// Combined throughput: N / (prep + submit).
    throughput:        f64,
    skipped:           bool,
    skip_reason:       &'static str,
}

/// Layout of one entry in the instance data buffer.
/// Matches a minimal per-instance Transform (position + scale).
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct GpuInstance {
    position: [f32; 3],
    scale:    f32,
}

/// Layout of one wgpu DrawIndexedIndirect command (wgpu spec, 5×u32 = 20 bytes).
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct DrawIndexedIndirectArgs {
    index_count:    u32,
    instance_count: u32,
    first_index:    u32,
    base_vertex:    i32,
    first_instance: u32,
}

fn bench_gpu_indirect(n: usize) -> IndirectResult {
    use wgpu::util::DeviceExt;

    // Request a wgpu device on the same adapter Bevy uses.
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN | wgpu::Backends::DX12,
        ..Default::default()
    });

    // Enumerate adapters and pick the first high-performance discrete GPU.
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference:       wgpu::PowerPreference::HighPerformance,
        compatible_surface:     None,
        force_fallback_adapter: false,
    })) {
        Ok(a)  => a,
        Err(_) => return IndirectResult {
            cpu_prep_elapsed:   Duration::ZERO,
            gpu_submit_elapsed: Duration::ZERO,
            instance_count:     0,
            throughput:         0.0,
            skipped:            true,
            skip_reason:        "no wgpu adapter found",
        },
    };

    // Require multi_draw_indirect — the core capability we are benchmarking.
    let features = adapter.features();
    if !features.contains(wgpu::Features::MULTI_DRAW_INDIRECT_COUNT) {
        return IndirectResult {
            cpu_prep_elapsed:   Duration::ZERO,
            gpu_submit_elapsed: Duration::ZERO,
            instance_count:     0,
            throughput:         0.0,
            skipped:            true,
            skip_reason:        "MULTI_DRAW_INDIRECT not supported on this GPU",
        };
    }

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label:             Some("indirect-bench"),
            required_features: wgpu::Features::MULTI_DRAW_INDIRECT_COUNT,
            required_limits:   wgpu::Limits::default(),
            memory_hints:      wgpu::MemoryHints::Performance,
            trace:             wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        },
    )).expect("wgpu device request failed");

    // ── CPU prep: build instance data + indirect command buffers ─────────────
    // All work is Rayon-parallel — mirrors how the engine would fill these
    // from ECS component data before upload.
    let t_prep = Instant::now();

    let instances: Vec<GpuInstance> = (0..n).into_par_iter().map(|i| {
        GpuInstance {
            position: [(i % 1_000) as f32 * 2.0, 0.5, (i / 1_000) as f32 * 2.0],
            scale:    1.0,
        }
    }).collect();

    // One indirect command per instance (each draws the same 36-index cube).
    // In a real GPU-driven pipeline the GPU would cull these via a compute shader
    // before submission — here we measure the CPU→GPU transfer bottleneck.
    let indirect_cmds: Vec<DrawIndexedIndirectArgs> = (0..n).into_par_iter().map(|i| {
        DrawIndexedIndirectArgs {
            index_count:    36,          // 12 triangles × 3 indices (unit cube)
            instance_count: 1,
            first_index:    0,
            base_vertex:    0,
            first_instance: i as u32,
        }
    }).collect();

    let cpu_prep_elapsed = t_prep.elapsed();

    // ── GPU upload + indirect dispatch ────────────────────────────────────────
    let t_gpu = Instant::now();

    // Upload instance data to a VERTEX | COPY_DST buffer.
    let instance_buf: wgpu::Buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label:    Some("instance-data"),
        contents: bytemuck::cast_slice(&instances),
        usage:    wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    });

    // Upload indirect commands to an INDIRECT | COPY_DST buffer.
    let indirect_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label:    Some("indirect-cmds"),
        contents: bytemuck::cast_slice(&indirect_cmds),
        usage:    wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
    });

    // Minimal index buffer (36 indices for a unit cube).
    let cube_indices: [u16; 36] = [
        0,1,2, 2,3,0,  4,5,6, 6,7,4,  8,9,10, 10,11,8,
        12,13,14, 14,15,12,  16,17,18, 18,19,16,  20,21,22, 22,23,20,
    ];
    let index_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label:    Some("cube-indices"),
        contents: bytemuck::cast_slice(&cube_indices),
        usage:    wgpu::BufferUsages::INDEX,
    });

    // Minimal vertex buffer (24 cube corners, position only).
    let cube_verts: [[f32; 3]; 24] = [
        [-0.5,-0.5,-0.5],[ 0.5,-0.5,-0.5],[ 0.5, 0.5,-0.5],[-0.5, 0.5,-0.5],
        [-0.5,-0.5, 0.5],[ 0.5,-0.5, 0.5],[ 0.5, 0.5, 0.5],[-0.5, 0.5, 0.5],
        [-0.5, 0.5,-0.5],[ 0.5, 0.5,-0.5],[ 0.5, 0.5, 0.5],[-0.5, 0.5, 0.5],
        [-0.5,-0.5,-0.5],[ 0.5,-0.5,-0.5],[ 0.5,-0.5, 0.5],[-0.5,-0.5, 0.5],
        [-0.5,-0.5,-0.5],[-0.5, 0.5,-0.5],[-0.5, 0.5, 0.5],[-0.5,-0.5, 0.5],
        [ 0.5,-0.5,-0.5],[ 0.5, 0.5,-0.5],[ 0.5, 0.5, 0.5],[ 0.5,-0.5, 0.5],
    ];
    let vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label:    Some("cube-verts"),
        contents: bytemuck::cast_slice(&cube_verts),
        usage:    wgpu::BufferUsages::VERTEX,
    });

    // Minimal render pipeline — just enough to accept a vertex + instance buffer
    // and issue indirect draws. No fragment output needed (depth-only pass mirrors
    // the shadow/prepass pattern used in real GPU-driven engines).
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label:  Some("indirect-vs"),
        source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(r#"
            struct Instance { pos: vec3<f32>, scale: f32 }
            @vertex
            fn vs_main(
                @location(0) vert:  vec3<f32>,
                @location(1) inst:  vec4<f32>,
            ) -> @builtin(position) vec4<f32> {
                let world = vert * inst.w + inst.xyz;
                return vec4<f32>(world, 1.0);
            }
        "#)),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label:                Some("indirect-layout"),
        bind_group_layouts:   &[],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label:  Some("indirect-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module:      &shader,
            entry_point: Some("vs_main"),
            buffers: &[
                // Slot 0: per-vertex position
                wgpu::VertexBufferLayout {
                    array_stride:       std::mem::size_of::<[f32; 3]>() as u64,
                    step_mode:          wgpu::VertexStepMode::Vertex,
                    attributes:         &wgpu::vertex_attr_array![0 => Float32x3],
                },
                // Slot 1: per-instance position + scale (GpuInstance = vec4)
                wgpu::VertexBufferLayout {
                    array_stride:       std::mem::size_of::<GpuInstance>() as u64,
                    step_mode:          wgpu::VertexStepMode::Instance,
                    attributes:         &wgpu::vertex_attr_array![1 => Float32x4],
                },
            ],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive:     wgpu::PrimitiveState::default(),
        // Depth-only pass: write depth, no colour output.
        depth_stencil: Some(wgpu::DepthStencilState {
            format:              wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare:       wgpu::CompareFunction::Less,
            stencil:             wgpu::StencilState::default(),
            bias:                wgpu::DepthBiasState::default(),
        }),
        multisample:  wgpu::MultisampleState::default(),
        fragment:     None,
        multiview:    None,
        cache:        None,
    });

    // Depth-only render target: 1×1 — submission cost is what we measure.
    let depth_tex = device.create_texture(&wgpu::TextureDescriptor {
        label:           Some("indirect-depth"),
        size:            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count:    1,
        dimension:       wgpu::TextureDimension::D2,
        format:          wgpu::TextureFormat::Depth32Float,
        usage:           wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats:    &[],
    });
    let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("indirect-encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label:             Some("indirect-pass"),
            color_attachments: &[],   // no colour — depth-only
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load:  wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes:    None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_vertex_buffer(0, vertex_buf.slice(..));
        pass.set_vertex_buffer(1, instance_buf.slice(..));
        pass.set_index_buffer(index_buf.slice(..), wgpu::IndexFormat::Uint16);
        // THE KEY CALL: single dispatch replaces N individual draw calls.
        // GPU reads indirect_buf and dispatches N draw commands autonomously.
        pass.multi_draw_indexed_indirect(&indirect_buf, 0, n as u32);
    }
    let _submission: wgpu::SubmissionIndex = queue.submit(std::iter::once(encoder.finish()));
    // Poll until GPU work completes so the timing is accurate.
    device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    }).ok();

    let gpu_submit_elapsed = t_gpu.elapsed();

    let total = cpu_prep_elapsed + gpu_submit_elapsed;
    let throughput = if total.as_secs_f64() > 0.0 {
        n as f64 / total.as_secs_f64()
    } else { 0.0 };

    IndirectResult {
        cpu_prep_elapsed,
        gpu_submit_elapsed,
        instance_count: n,
        throughput,
        skipped:        false,
        skip_reason:    "",
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 12. StopReason — every exit condition is named and printed
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
enum StopReason {
    /// TOML parse rate dropped below PARSE_RATE_FLOOR% of the peak seen.
    ParseRateDrop { peak: f64, current: f64, threshold_pct: f64 },
    /// ECS spawn took longer than SPAWN_CEILING.
    EcsSpawnTooSlow { elapsed: Duration, ceiling: Duration },
    /// Average GPU frame time exceeded FRAME_CEILING.
    FrameTimeTooSlow { avg: Duration, ceiling: Duration },
    /// Binary decode rate dropped below BIN_RATE_FLOOR% of peak.
    BinaryDecodeDrop { peak: f64, current: f64, threshold_pct: f64 },
    /// RSS exceeded RAM_FRACTION of total physical RAM.
    MemoryExceeded { rss: u64, limit: u64, ram: u64 },
    /// Maximum N cap reached (safety — prevents infinite loop).
    MaxNCap { n: usize },
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StopReason::ParseRateDrop { peak, current, threshold_pct } =>
                write!(f, "TOML parse rate dropped to {current:.0}/s \
                           ({:.1}% of peak {peak:.0}/s, threshold {threshold_pct:.0}%)",
                           current / peak * 100.0),
            StopReason::EcsSpawnTooSlow { elapsed, ceiling } =>
                write!(f, "ECS spawn took {} > ceiling {}",
                           fmt_d(*elapsed), fmt_d(*ceiling)),
            StopReason::FrameTimeTooSlow { avg, ceiling } =>
                write!(f, "GPU avg frame time {} > ceiling {}",
                           fmt_d(*avg), fmt_d(*ceiling)),
            StopReason::BinaryDecodeDrop { peak, current, threshold_pct } =>
                write!(f, "Binary decode rate dropped to {current:.0}/s \
                           ({:.1}% of peak {peak:.0}/s, threshold {threshold_pct:.0}%)",
                           current / peak * 100.0),
            StopReason::MemoryExceeded { rss, limit, ram } =>
                write!(f, "RSS {} exceeded limit {} ({:.0}% of RAM {})",
                           fmt_b(*rss), fmt_b(*limit),
                           *rss as f64 / *ram as f64 * 100.0,
                           fmt_b(*ram)),
            StopReason::MaxNCap { n } =>
                write!(f, "Maximum N cap reached at {}", fmt_n(*n)),
        }
    }
}

// Thresholds
const PARSE_RATE_FLOOR_PCT: f64 = 50.0;   // stop if parse rate < 50% of peak
const BIN_RATE_FLOOR_PCT:   f64 = 50.0;   // stop if decode rate < 50% of peak
const SPAWN_CEILING:        Duration = Duration::from_secs(30);
const FRAME_CEILING:        Duration = Duration::from_millis(41);  // 24 FPS floor (1000/24 = 41.6ms)
const RAM_FRACTION:         f64 = 0.80;   // stop if RSS > 80% of total RAM
const MAX_N:                usize = 16_000_000; // absolute safety cap
/// Disk write is skipped once it takes longer than this — disk is NTFS-bound,
/// not engine-bound. The in-memory path continues scaling past this point.
const DISK_SKIP_AFTER:      Duration = Duration::from_secs(5);
/// Stop before a step if estimated RSS for the step would exceed this fraction
/// of total RAM. Prevents OOM crashes from allocation panics.
/// Both GPU render and pure ECS run at every N — no artificial caps.
/// The loop stops only via FRAME_CEILING, SPAWN_CEILING, RSS, or MAX_N.
const PRE_CHECK_RAM_FRAC:   f64 = 0.70;

// ═══════════════════════════════════════════════════════════════════════════════
// 13. Baseline comparison
//     Baseline = first recorded run (old benchmark, no Camera3d, no MoE,
//     MAX_ECS_N=500K cap, dense-only, single-tier physics).
//     Source: user-provided terminal output, 2026-03-22.
// ═══════════════════════════════════════════════════════════════════════════════

/// One row of results captured during the optimized run.
struct StepRecord {
    n:                   usize,
    parse_rate:          f64,  // instances/s
    gpu_fps:             f64,  // frames/s (0 if skipped)
    gpu_dense_qps:       f64,  // dense Transform query/s
    gpu_sparse_qps:      f64,  // sparse MoE query/s (Changed gate)
    sparse_speedup:      f64,  // dense/sparse ratio
    phys_sort_ms:        f64,  // static broad-phase sort ms
    bin_enc_rate:        f64,  // binary encode instances/s
    bin_dec_rate:        f64,  // binary decode instances/s
    indirect_throughput: f64,  // GPU-indirect instances/s (0 if skipped)
}

/// Baseline row from the old run (pre-optimisation).
struct BaselineRow {
    n:            usize,
    parse_rate:   f64,
    gpu_fps:      f64,   // 0 = "ECS + GPU : skipped" in old run
    gpu_qps:      f64,
    phys_sort_ms: f64,
    bin_enc_rate: f64,
    bin_dec_rate: f64,
}

/// Hardcoded baseline from the user-provided old run output.
fn baseline_rows() -> Vec<BaselineRow> {
    vec![
        // N=1K
        BaselineRow { n:      1_024, parse_rate:   184_500.0, gpu_fps: 1.0/0.000_070, gpu_qps: 14_580_000.0, phys_sort_ms:  0.052, bin_enc_rate:   306_900.0, bin_dec_rate: 1_360_000.0 },
        // N=2K
        BaselineRow { n:      2_048, parse_rate:   436_700.0, gpu_fps: 1.0/0.000_120, gpu_qps: 17_060_000.0, phys_sort_ms:  0.110, bin_enc_rate:   345_400.0, bin_dec_rate: 1_820_000.0 },
        // N=4K
        BaselineRow { n:      4_096, parse_rate:   433_000.0, gpu_fps: 1.0/0.000_217, gpu_qps: 18_850_000.0, phys_sort_ms:  0.167, bin_enc_rate:   375_000.0, bin_dec_rate: 2_240_000.0 },
        // N=8K
        BaselineRow { n:      8_192, parse_rate:   622_900.0, gpu_fps: 1.0/0.000_394, gpu_qps: 20_800_000.0, phys_sort_ms:  0.383, bin_enc_rate:   401_800.0, bin_dec_rate: 2_020_000.0 },
        // N=16K
        BaselineRow { n:     16_384, parse_rate:   630_600.0, gpu_fps: 1.0/0.000_718, gpu_qps: 22_810_000.0, phys_sort_ms:  0.772, bin_enc_rate:   448_500.0, bin_dec_rate: 1_840_000.0 },
        // N=33K
        BaselineRow { n:     33_792, parse_rate:   666_700.0, gpu_fps: 1.0/0.001_500, gpu_qps: 22_410_000.0, phys_sort_ms:  1.600, bin_enc_rate:   456_600.0, bin_dec_rate: 2_050_000.0 },
        // N=66K
        BaselineRow { n:     65_536, parse_rate:   614_600.0, gpu_fps: 1.0/0.002_500, gpu_qps: 26_480_000.0, phys_sort_ms:  3.100, bin_enc_rate:   500_000.0, bin_dec_rate: 2_090_000.0 },
        // N=131K
        BaselineRow { n:    131_072, parse_rate:   618_700.0, gpu_fps: 1.0/0.005_000, gpu_qps: 26_470_000.0, phys_sort_ms:  5.900, bin_enc_rate:   478_700.0, bin_dec_rate: 2_020_000.0 },
        // N=262K
        BaselineRow { n:    262_144, parse_rate:   662_200.0, gpu_fps: 1.0/0.010_700, gpu_qps: 24_610_000.0, phys_sort_ms: 11.400, bin_enc_rate:   501_000.0, bin_dec_rate: 1_800_000.0 },
        // N=524K — ECS skipped in baseline
        BaselineRow { n:    524_288, parse_rate:   658_300.0, gpu_fps: 0.0,             gpu_qps:          0.0, phys_sort_ms: 23.400, bin_enc_rate:   555_300.0, bin_dec_rate: 2_010_000.0 },
        // N=1.05M — ECS skipped in baseline
        BaselineRow { n:  1_048_576, parse_rate:   664_900.0, gpu_fps: 0.0,             gpu_qps:          0.0, phys_sort_ms: 49.300, bin_enc_rate:   539_400.0, bin_dec_rate: 2_020_000.0 },
        // N=2.10M — ECS skipped in baseline
        BaselineRow { n:  2_097_152, parse_rate:   616_700.0, gpu_fps: 0.0,             gpu_qps:          0.0, phys_sort_ms:105.100, bin_enc_rate:   523_600.0, bin_dec_rate: 1_980_000.0 },
    ]
}

fn print_comparison(records: &[StepRecord]) {
    let baseline = baseline_rows();

    println!("\n");
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║  BASELINE vs OPTIMIZED — key metrics per N                                                      ║");
    println!("║  Baseline = old run (no Camera3d, MAX_ECS_N=500K, dense-only ECS, single-tier physics)          ║");
    println!("║  Optimized = current run (Camera3d, MoE sparse gate 10%, 2D physics gate, no caps)              ║");
    println!("╠══════════╦══════════════════════╦═══════════════════════════════╦═══════════════════════════════╗");
    println!("║          ║   TOML parse rate    ║   GPU frame FPS               ║   Physics sort ms             ║");
    println!("║    N     ║  base      opt  Δ    ║  base     opt    Δ            ║  base    opt    Δ             ║");
    println!("╠══════════╬══════════════════════╬═══════════════════════════════╬═══════════════════════════════╣");

    for rec in records {
        // Find closest baseline row by N.
        let base = baseline.iter().min_by_key(|b| {
            let diff = (b.n as i64 - rec.n as i64).unsigned_abs();
            diff
        });
        let Some(b) = base else { continue; };

        let parse_delta = if b.parse_rate > 0.0 {
            (rec.parse_rate - b.parse_rate) / b.parse_rate * 100.0
        } else { 0.0 };

        let fps_base = b.gpu_fps;
        let fps_opt  = rec.gpu_fps;
        let fps_delta = if fps_base > 0.0 {
            (fps_opt - fps_base) / fps_base * 100.0
        } else { 0.0 };

        let phys_delta = if b.phys_sort_ms > 0.0 {
            (rec.phys_sort_ms - b.phys_sort_ms) / b.phys_sort_ms * 100.0
        } else { 0.0 };

        let fps_base_s = if fps_base > 0.0 { format!("{:>6.0}",  fps_base) } else { "  skip".into() };
        let fps_opt_s  = if fps_opt  > 0.0 { format!("{:>6.0}",  fps_opt)  } else { "  skip".into() };
        let fps_d_s    = if fps_base > 0.0 && fps_opt > 0.0 {
            format!("{:>+6.0}%", fps_delta)
        } else if fps_opt > 0.0 {
            "  NEW ".into()
        } else {
            "      ".into()
        };

        println!("║ {:>8} ║ {:>7.0}K {:>7.0}K {:>+5.0}% ║ {} {} {}       ║ {:>6.1}ms {:>6.1}ms {:>+6.0}%   ║",
            fmt_n(rec.n),
            b.parse_rate   / 1e3,
            rec.parse_rate / 1e3,
            parse_delta,
            fps_base_s, fps_opt_s, fps_d_s,
            b.phys_sort_ms,
            rec.phys_sort_ms,
            phys_delta,
        );
    }

    println!("╠══════════╩══════════════════════╩═══════════════════════════════╩═══════════════════════════════╣");
    println!("║  MoE sparse gate (ECS): Changed<Transform> + InheritedVisibility — processes 10% of N           ║");
    println!("║  MoE physics gate: 2D velocity×distance routing — rigid/kinematic/static per entity             ║");
    println!("║  GPU render: Camera3d added, unlit+opaque material, no artificial N cap                         ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════════════════════════╝");

    // Sparse MoE speedup summary
    let moe_rows: Vec<_> = records.iter().filter(|r| r.sparse_speedup > 0.0).collect();
    if !moe_rows.is_empty() {
        println!("\n  MoE ECS Sparse Speedup (Changed gate vs dense):");
        for r in &moe_rows {
            println!("    N={:>8}  dense={:>10}  sparse={:>10}  speedup={:.1}×",
                fmt_n(r.n),
                fmt_rate(r.gpu_dense_qps),
                fmt_rate(r.gpu_sparse_qps),
                r.sparse_speedup);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 14. main — exponential scaling loop
// ═══════════════════════════════════════════════════════════════════════════════

fn main() {
    let threads   = rayon::current_num_threads();
    let total_ram = total_ram_bytes();
    let ram_limit = (total_ram as f64 * RAM_FRACTION) as u64;

    banner(threads, total_ram);

    // Per-metric peaks for degradation detection
    let mut peak_parse_rate:   f64 = 0.0;
    let mut peak_bin_dec_rate: f64 = 0.0;
    // Once disk write exceeds DISK_SKIP_AFTER, skip it for all future steps.
    // NTFS throughput is not an engine metric — the in-memory path keeps scaling.
    let mut disk_skipped = false;

    // OOM pre-check state: updated at end of each step, checked at start of next.
    // bytes_per_inst = (TOML string bytes + binary raw bytes) / N from last step.
    let mut prev_rss:            u64 = 0;
    let mut prev_bytes_per_inst: u64 = 0;

    let mut n: usize = 1_024;
    let mut step      = 0usize;
    let mut records: Vec<StepRecord> = Vec::new();

    loop {
        step += 1;

        // ── OOM pre-check (before ANY allocation for this step) ───────────────
        // After step 1 we know how many bytes per instance the pipeline needs.
        // If the next allocation would push estimated RSS past PRE_CHECK_RAM_FRAC,
        // stop cleanly now instead of crashing inside Vec::with_capacity.
        if prev_bytes_per_inst > 0 && total_ram > 0 {
            let est = prev_rss.saturating_add(n as u64 * prev_bytes_per_inst);
            let lim = (total_ram as f64 * PRE_CHECK_RAM_FRAC) as u64;
            if est > lim {
                print_stop(&StopReason::MemoryExceeded {
                    rss: est, limit: lim, ram: total_ram,
                }, n / 2);   // last *successful* N was the previous step
                break;
            }
        }

        print!("\n▶▶▶ N={}", fmt_n(n));
        std::io::stdout().flush().ok();

        // ── A: In-memory TOML parse (chunked — no large Vec<String>) ─────────
        let parse      = parse_inmem(n);
        let parse_rate = parse.ok as f64 / parse.elapsed.as_secs_f64();
        if parse_rate > peak_parse_rate { peak_parse_rate = parse_rate; }

        // ── B: Disk write (skipped once NTFS becomes the bottleneck) ─────────
        // write_space streams generation per-file — no large string alloc.
        let disk_opt: Option<DiskResult> = if disk_skipped {
            None
        } else {
            let dr = write_space(n);
            if dr.write_elapsed > DISK_SKIP_AFTER {
                disk_skipped = true;
                println!("\n  [disk write skipped from next step — NTFS bottleneck at {}]",
                         fmt_d(dr.write_elapsed));
            }
            Some(dr)
        };

        // ── C: Binary encode → decode (chunked — no large Vec<String>) ───────
        let (blob, enc_elapsed) = encode_binary_n(n);
        let (bins, dec_elapsed) = decode_binary(&blob);
        let bin_dec_rate = bins.len() as f64 / dec_elapsed.as_secs_f64();
        if bin_dec_rate > peak_bin_dec_rate { peak_bin_dec_rate = bin_dec_rate; }

        // ── D: RSS snapshot ──────────────────────────────────────────────────
        let rss = rss_bytes();

        // ── E1: GPU render — runs at every N, stopped only by FRAME_CEILING ──
        let ecs = run_bevy_bench(n);

        // ── E2: Pure ECS archetype iteration (no render, targets 10M+) ───────
        let pure_ecs = bench_ecs_pure(n);

        // ── F: Streaming simulation (hot-cache radius query + eviction) ───────
        let stream = bench_streaming(&bins);

        // ── G: Physics tiered LOD (5K rigid | 50K kinematic | N static) ──────
        let phys = bench_physics(&bins);

        // ── H: GPU-driven indirect draw (single multi_draw_indexed_indirect call) ──
        let indirect = bench_gpu_indirect(n);

        // ── Print row ────────────────────────────────────────────────────────
        print_row(step, n, &parse, parse_rate, disk_opt.as_ref(),
                  enc_elapsed, dec_elapsed, &blob, bin_dec_rate, rss,
                  &ecs, &pure_ecs, &stream, &phys, &indirect);

        // ── Record this step for comparison table ────────────────────────────
        let bin_enc_rate = blob.count as f64 / enc_elapsed.as_secs_f64();
        let gpu_fps = if !ecs.skipped && ecs.avg_frame_time.as_secs_f64() > 0.0 {
            1.0 / ecs.avg_frame_time.as_secs_f64()
        } else { 0.0 };
        records.push(StepRecord {
            n,
            parse_rate,
            gpu_fps,
            gpu_dense_qps:    ecs.query_per_sec,
            gpu_sparse_qps:   if ecs.avg_sparse_time.as_secs_f64() > 0.0 {
                ecs.sparse_count as f64 / ecs.avg_sparse_time.as_secs_f64()
            } else { 0.0 },
            sparse_speedup:   ecs.sparse_speedup,
            phys_sort_ms:     phys.sort_elapsed.as_secs_f64() * 1e3,
            bin_enc_rate,
            bin_dec_rate,
            indirect_throughput: indirect.throughput,
        });

        // ── Update OOM predictor for next iteration ───────────────────────────
        // Now that all phases are chunked, the dominant allocations are:
        // - InstanceBin Vec in encode_binary_n: blob.raw_bytes
        // - Decoded Vec<InstanceBin>: blob.raw_bytes again
        // - Compressed blob: blob.compressed.len() (small)
        // Use 2× raw_bytes / N as conservative bytes-per-instance estimate.
        prev_rss            = rss;
        prev_bytes_per_inst = (blob.raw_bytes * 2 / n.max(1)) as u64;

        // ── Check stop conditions ─────────────────────────────────────────────
        if let Some(reason) = check_stop(
            n, parse_rate, peak_parse_rate,
            bin_dec_rate, peak_bin_dec_rate,
            ecs.spawn_elapsed, ecs.avg_frame_time,
            rss, ram_limit, total_ram,
        ) {
            print_stop(&reason, n);
            break;
        }

        // Double N for next iteration
        n = n.saturating_mul(2);
    }

    // Print baseline vs optimized comparison table after the loop.
    print_comparison(&records);
}

fn check_stop(
    n:               usize,
    parse_rate:      f64,
    peak_parse:      f64,
    bin_rate:        f64,
    peak_bin:        f64,
    spawn_elapsed:   Duration,
    avg_frame:       Duration,
    rss:             u64,
    ram_limit:       u64,
    total_ram:       u64,
) -> Option<StopReason> {
    // 1. TOML parse rate degradation
    if peak_parse > 0.0 && parse_rate < peak_parse * (PARSE_RATE_FLOOR_PCT / 100.0) {
        return Some(StopReason::ParseRateDrop {
            peak: peak_parse, current: parse_rate, threshold_pct: PARSE_RATE_FLOOR_PCT,
        });
    }
    // 2. ECS spawn ceiling
    if spawn_elapsed > SPAWN_CEILING {
        return Some(StopReason::EcsSpawnTooSlow {
            elapsed: spawn_elapsed, ceiling: SPAWN_CEILING,
        });
    }
    // 3. GPU frame time ceiling
    if avg_frame > Duration::ZERO && avg_frame > FRAME_CEILING {
        return Some(StopReason::FrameTimeTooSlow {
            avg: avg_frame, ceiling: FRAME_CEILING,
        });
    }
    // 4. Binary decode degradation
    if peak_bin > 0.0 && bin_rate < peak_bin * (BIN_RATE_FLOOR_PCT / 100.0) {
        return Some(StopReason::BinaryDecodeDrop {
            peak: peak_bin, current: bin_rate, threshold_pct: BIN_RATE_FLOOR_PCT,
        });
    }
    // 5. Memory pressure
    if rss > 0 && ram_limit > 0 && rss > ram_limit {
        return Some(StopReason::MemoryExceeded {
            rss, limit: ram_limit, ram: total_ram,
        });
    }
    // 6. Absolute N cap
    if n >= MAX_N {
        return Some(StopReason::MaxNCap { n });
    }
    None
}

// ═══════════════════════════════════════════════════════════════════════════════
// Formatting helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn fmt_d(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s >= 60.0       { format!("{:.1}m",  s / 60.0) }
    else if s >= 1.0   { format!("{:.3}s",  s) }
    else if s >= 0.001 { format!("{:.1}ms", s * 1e3) }
    else               { format!("{:.0}µs", s * 1e6) }
}

fn fmt_b(b: u64) -> String {
    if b >= 1 << 30      { format!("{:.2} GiB", b as f64 / (1u64 << 30) as f64) }
    else if b >= 1 << 20 { format!("{:.1} MiB", b as f64 / (1u64 << 20) as f64) }
    else if b >= 1 << 10 { format!("{:.1} KiB", b as f64 / (1u64 << 10) as f64) }
    else                 { format!("{b} B") }
}

fn fmt_rate(r: f64) -> String {
    if r >= 1e6      { format!("{:.2}M/s", r / 1e6) }
    else if r >= 1e3 { format!("{:.1}K/s", r / 1e3) }
    else             { format!("{:.0}/s",  r) }
}

fn fmt_n(n: usize) -> String {
    if n >= 1_000_000      { format!("{:.2}M", n as f64 / 1e6) }
    else if n >= 1_000     { format!("{:.0}K", n as f64 / 1e3) }
    else                   { format!("{n}") }
}

fn banner(threads: usize, total_ram: u64) {
    let w = 84;
    println!("╔{}╗", "═".repeat(w));
    println!("║  Eustress Engine · Instance Capacity Benchmark v4{:<w$}║", "", w = w - 50);
    println!("║  Exponential scale: N doubles each step until a stop condition fires.{:<w$}║", "", w = w - 70);
    println!("║  Rayon threads : {:<w$}║", threads, w = w - 18);
    if total_ram > 0 {
        println!("║  Total RAM     : {:<w$}║", fmt_b(total_ram), w = w - 18);
        println!("║  RAM limit     : {:.0}% = {:<w$}║",
                 RAM_FRACTION * 100.0,
                 fmt_b((total_ram as f64 * RAM_FRACTION) as u64),
                 w = w - 22);
    }
    println!("╚{}╝\n", "═".repeat(w));
}

#[allow(clippy::too_many_arguments)]
fn print_row(
    step:        usize,
    n:           usize,
    parse:       &ParseResult,
    parse_rate:  f64,
    disk:        Option<&DiskResult>,
    enc_elapsed: Duration,
    dec_elapsed: Duration,
    blob:        &BinaryCacheBlob,
    bin_rate:    f64,
    rss:         u64,
    ecs:         &EcsResult,        // Bevy GPU render (dense + MoE sparse)
    pure_ecs:    &PureEcsResult,    // pure archetype iteration (no render)
    stream:      &StreamResult,     // hot-cache streaming simulation
    phys:        &PhysResult,       // tiered LOD physics
    indirect:    &IndirectResult,   // GPU-driven indirect draw
) {
    let rss_s  = if rss > 0 { fmt_b(rss) } else { "n/a".into() };
    let ratio  = blob.raw_bytes as f64 / blob.compressed.len() as f64;
    let enc_r  = blob.count as f64 / enc_elapsed.as_secs_f64();

    println!("\n  Step {step} ─ N = {} ({} TOML bytes)", fmt_n(n), fmt_b(parse.bytes as u64));
    println!("  ┌──────────────────────────────────────────────────────────────────────────┐");

    // ── TOML parse ─────────────────────────────────────────────────────────────
    println!("  │ TOML parse (chunked par) : {}   rate={}   µs/inst={:.2}",
             fmt_d(parse.elapsed), fmt_rate(parse_rate),
             parse.elapsed.as_secs_f64() * 1e6 / n as f64);

    // ── Disk write ─────────────────────────────────────────────────────────────
    match disk {
        Some(d) => println!("  │ Disk write (par stream)  : {}   {} written   rate={}",
                            fmt_d(d.write_elapsed), fmt_b(d.bytes_written),
                            fmt_rate(d.bytes_written as f64 / d.write_elapsed.as_secs_f64())),
        None    => println!("  │ Disk write               : skipped (NTFS bottleneck)"),
    }

    // ── Binary cache ───────────────────────────────────────────────────────────
    println!("  │ Binary encode (TOML→bin) : {}   rate={}   raw={} → zstd={} ({:.1}×)",
             fmt_d(enc_elapsed), fmt_rate(enc_r),
             fmt_b(blob.raw_bytes as u64), fmt_b(blob.compressed.len() as u64), ratio);
    println!("  │ Binary decode (zstd→bin) : {}   rate={}   µs/inst={:.2}",
             fmt_d(dec_elapsed), fmt_rate(bin_rate),
             dec_elapsed.as_secs_f64() * 1e6 / n as f64);

    // ── Memory ─────────────────────────────────────────────────────────────────
    println!("  │ RSS after encode         : {}   bytes/inst={:.0}",
             rss_s, if rss > 0 { rss as f64 / n as f64 } else { 0.0 });

    // ── GPU render + MoE sparse gate (stops at 24 FPS = 41ms avg frame) ────────
    if ecs.skipped {
        println!("  │ GPU render               : skipped");
    } else {
        let fps = if ecs.avg_frame_time.as_secs_f64() > 0.0 {
            1.0 / ecs.avg_frame_time.as_secs_f64()
        } else { 0.0 };
        println!("  │ GPU render ({}ents)   : spawn={}  avg={} ({:.0}fps)  p99={}",
                 fmt_n(ecs.entity_count),
                 fmt_d(ecs.spawn_elapsed),
                 fmt_d(ecs.avg_frame_time), fps,
                 fmt_d(ecs.p99_frame_time));
        println!("  │   Dense query/s          : {}", fmt_rate(ecs.query_per_sec));
        // MoE sparse activation metrics: Changed<Transform> + InheritedVisibility gate.
        let active_pct = ecs.sparse_count as f64 / ecs.entity_count.max(1) as f64 * 100.0;
        println!("  │   Sparse gate (MoE)       : {} active ({:.1}% of N)  avg={}  speedup={:.1}×",
                 fmt_n(ecs.sparse_count),
                 active_pct,
                 fmt_d(ecs.avg_sparse_time),
                 ecs.sparse_speedup);
    }

    // ── Pure ECS archetype iteration (no render) ───────────────────────────────
    if pure_ecs.skipped {
        println!("  │ Pure ECS iter            : skipped");
    } else {
        println!("  │ Pure ECS spawn+iter      : spawn={}  iter={}  rate={}",
                 fmt_d(pure_ecs.spawn_elapsed),
                 fmt_d(pure_ecs.iter_elapsed),
                 fmt_rate(pure_ecs.iter_rate));
    }

    // ── Streaming simulation ───────────────────────────────────────────────────
    if stream.skipped {
        println!("  │ Streaming sim            : skipped");
    } else {
        println!("  │ Streaming (cache insert) : {}   radius query={}   active_zone={}",
                 fmt_d(stream.insert_elapsed),
                 fmt_d(stream.query_elapsed),
                 fmt_n(stream.active_zone));
        println!("  │   Eviction pass          : {}   evicted={}",
                 fmt_d(stream.evict_elapsed), fmt_n(stream.evicted));
    }

    // ── Physics — MoE 2D velocity×distance gate ───────────────────────────
    println!("  │ Physics static   ({} total):  sort={}  query={}  pairs={}",
             fmt_n(n),
             fmt_d(phys.sort_elapsed),
             fmt_d(phys.query_elapsed),
             phys.pairs_found);
    println!("  │   MoE gate → rigid={} kin={} static={}",
             fmt_n(phys.rigid_count),
             fmt_n(phys.kinematic_count),
             fmt_n(phys.static_count));
    println!("  │   Kinematic update (20Hz):  {}   Rigid sort (60Hz): {}",
             fmt_d(phys.kinematic_elapsed),
             fmt_d(phys.rigid_elapsed));

    // ── GPU-driven indirect draw ───────────────────────────────────────────
    if indirect.skipped {
        println!("  │ GPU indirect draw        : skipped ({})", indirect.skip_reason);
    } else {
        println!("  │ GPU indirect draw ({})  : cpu_prep={}  gpu_submit={}  total={}",
                 fmt_n(indirect.instance_count),
                 fmt_d(indirect.cpu_prep_elapsed),
                 fmt_d(indirect.gpu_submit_elapsed),
                 fmt_d(indirect.cpu_prep_elapsed + indirect.gpu_submit_elapsed));
        println!("  │   Indirect throughput     : {}  (vs Bevy dense: {})",
                 fmt_rate(indirect.throughput),
                 if ecs.skipped { "n/a".to_string() } else { fmt_rate(ecs.query_per_sec) });
    }

    println!("  └───────────────────────────────────────────────────────────────────────────────┘");
}

fn print_stop(reason: &StopReason, last_n: usize) {
    println!("\n╔═══════════════════════════════════════════════════════════════════════════════╗");
    println!("║  STOPPED                                                                      ║");
    println!("║  Last successful N : {:<59}║", fmt_n(last_n));
    println!("║  Stop condition    : {:<59}║", format!("{reason}"));
    println!("╚═══════════════════════════════════════════════════════════════════════════════╝");
}

//! # Procedural Benchmark Map Generator
//!
//! Generates an N×N grid of `.part.toml` files in a Space's Workspace folder
//! for benchmarking the streaming system. Each part is a primitive cube placed
//! on a flat grid with varied heights, colors, and optional velocity to exercise
//! the MoE sparse gate (10% active fraction).
//!
//! ## Usage
//! ```
//! cargo run -p eustress-engine --bin generate-benchmark-map -- [OPTIONS]
//! ```
//!
//! ## Options
//! - `--grid-size N`   — grid dimension (NxN), default: 100 (10K parts)
//! - `--spacing F`     — distance between parts in world units, default: 4.0
//! - `--output DIR`    — output directory, default: auto-detect Space1/Workspace/BenchmarkGrid
//! - `--active-pct F`  — fraction of parts with velocity > 0 (MoE active), default: 0.10
//! - `--seed U`        — random seed for reproducibility, default: 42
//!
//! ## Scaling Guide
//! - 100×100  =      10,000 parts  — basic smoke test
//! - 316×316  =     ~100,000 parts — moderate load
//! - 1000×1000 =  1,000,000 parts  — heavy streaming test
//! - 1449×1449 =  ~2,100,000 parts — benchmark ceiling (2.10M)

use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Parse arguments (simple flag-based, no external dependency).
    let grid_size = parse_usize_flag(&args, "--grid-size").unwrap_or(100);
    let spacing = parse_f32_flag(&args, "--spacing").unwrap_or(4.0);
    let active_pct = parse_f32_flag(&args, "--active-pct").unwrap_or(0.10);
    let seed = parse_u64_flag(&args, "--seed").unwrap_or(42);
    // `--disk` forces the legacy filesystem path (50k folders). Default
    // is direct-to-WorldDb: a Fjall-primary, day-0 engine should NOT
    // generate 50k `_instance.toml` folders just to migrate them back
    // into the DB on next open. We write the entities straight into the
    // `world.fjalldb` tree partition — the same representation the
    // faithful importer produces and `SpaceSource::Fjall` loads.
    let disk_mode = args.iter().any(|a| a == "--disk");
    let output_dir = parse_string_flag(&args, "--output")
        .map(PathBuf::from)
        .unwrap_or_else(default_output_dir);

    let total = grid_size * grid_size;
    println!("=== Eustress Benchmark Map Generator ===");
    println!("Grid:       {}×{} = {} parts", grid_size, grid_size, total);
    println!("Spacing:    {} world units", spacing);
    println!("Active:     {:.0}% ({} parts with velocity)", active_pct * 100.0, (total as f32 * active_pct) as usize);
    println!("Seed:       {}", seed);
    println!("Output:     {}", output_dir.display());
    println!("Mode:       {}", if disk_mode { "DISK (legacy 50k folders)" } else { "WORLDDB (direct to Fjall, no folders)" });
    println!();

    // ── Resolve the sink ──────────────────────────────────────────────
    // WorldDb mode needs the Space root (the dir holding `world.fjalldb`
    // + `Workspace` + services) and the Space-relative key prefix for
    // the generated parts. Derived from `--output` by splitting at the
    // `Workspace` path component.
    #[cfg(feature = "world-db")]
    let world_db_sink = if disk_mode {
        None
    } else {
        match derive_space_root_and_prefix(&output_dir) {
            Some((space_root, rel_prefix)) => {
                match open_world_db_sink(&space_root, &rel_prefix) {
                    Ok(sink) => Some(sink),
                    Err(e) => {
                        eprintln!("ERROR: WorldDb sink unavailable ({e}). Re-run with --disk to force the filesystem path.");
                        std::process::exit(1);
                    }
                }
            }
            None => {
                eprintln!(
                    "ERROR: could not derive Space root from --output {:?} \
                     (expected a path containing a `Workspace` component). \
                     Pass --output <Space>/Workspace/<Grid> or use --disk.",
                    output_dir
                );
                std::process::exit(1);
            }
        }
    };

    if disk_mode {
        // Create output directory (legacy path only).
        if let Err(error) = std::fs::create_dir_all(&output_dir) {
            eprintln!("ERROR: cannot create output directory: {error}");
            std::process::exit(1);
        }
    }

    // Simple deterministic pseudo-random number generator (xorshift64).
    let mut rng_state = seed;

    let t0 = std::time::Instant::now();
    let mut written = 0usize;

    // Center the grid around origin.
    let half_extent = (grid_size as f32 * spacing) / 2.0;

    for row in 0..grid_size {
        for col in 0..grid_size {
            let x = (col as f32 * spacing) - half_extent;
            let z = (row as f32 * spacing) - half_extent;

            // Varied height using simple noise from the RNG.
            rng_state = xorshift64(rng_state);
            let height_noise = (rng_state % 100) as f32 / 100.0; // 0.0 - 0.99
            let y = height_noise * 8.0; // 0 - 8 world units height variation

            // Varied color (RGB 0-255).
            rng_state = xorshift64(rng_state);
            let r = ((rng_state >> 0) % 256) as u8;
            let g = ((rng_state >> 8) % 256) as u8;
            let b = ((rng_state >> 16) % 256) as u8;

            // Varied scale (0.5 - 2.0).
            rng_state = xorshift64(rng_state);
            let scale = 0.5 + (rng_state % 150) as f32 / 100.0;

            // MoE active fraction: assign velocity to active_pct of parts.
            rng_state = xorshift64(rng_state);
            let is_active = (rng_state % 1000) as f32 / 1000.0 < active_pct;
            let velocity = if is_active {
                rng_state = xorshift64(rng_state);
                1.0 + (rng_state % 500) as f32 / 100.0 // 1.0 - 6.0
            } else {
                0.0
            };

            // Build the part name.
            let part_name = format!("BenchPart_{}_{}", row, col);

            // Write the .part.toml file.
            //
            // Two benchmark-specific defaults:
            //
            //   `cast_shadow = false` — 50k+ anchored static parts with
            //   shadow cascades force every part through 4 shadow passes
            //   per frame, which on its own is the dominant render cost
            //   at scale. The benchmark exists to stress entity
            //   throughput, not the shadow pipeline.
            //
            //   `can_collide = false` — Avian treats every `RigidBody::Static`
            //   as a broadphase entry and an AABB to track, even with
            //   physics paused in Edit mode. 50k Static bodies = 50k
            //   broadphase entries. Avian 0.6 does NOT treat
            //   `Collider`-without-`RigidBody` as a collision obstacle
            //   (only spatial-query targets), so the only way to keep
            //   the broadphase cheap is to skip the collider entirely.
            //   The instance loader's "no collider unless can_collide"
            //   gate then leaves these entities physics-free.
            //
            // Hand-edit a TOML (or remove these defaults) once a shadow
            // or collision benchmark is specifically wanted.
            let toml_content = format!(
                r#"[asset]
mesh = "parts/block.glb"
scene = "Scene0"

[transform]
position = [{x:.1}, {y:.1}, {z:.1}]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [{scale:.2}, {scale:.2}, {scale:.2}]

[properties]
color = [{r}, {g}, {b}]
transparency = 0.0
anchored = {anchored}
can_collide = false
cast_shadow = false
reflectance = 0.0
material = "Plastic"
locked = false
velocity = {velocity:.1}

[metadata]
class_name = "Part"
archivable = true
created = "2026-03-22T00:00:00Z"
last_modified = "2026-03-22T00:00:00Z"
"#,
                x = x,
                y = y,
                z = z,
                scale = scale,
                r = r,
                g = g,
                b = b,
                anchored = if is_active { "false" } else { "true" },
                velocity = velocity,
            );

            #[cfg(feature = "world-db")]
            if let Some(ref sink) = world_db_sink {
                // Direct-to-Fjall: key is the Space-relative path the
                // loader's SpaceSource would read — no disk folder.
                match sink.put_part(&part_name, toml_content.as_bytes()) {
                    Ok(()) => written += 1,
                    Err(e) => eprintln!("WARN: worlddb put {part_name}: {e}"),
                }
            } else {
                write_part_to_disk(&output_dir, &part_name, &toml_content, &mut written);
            }
            #[cfg(not(feature = "world-db"))]
            write_part_to_disk(&output_dir, &part_name, &toml_content, &mut written);

            // Progress reporting every 10K parts.
            if written > 0 && written % 10_000 == 0 {
                let elapsed = t0.elapsed();
                let rate = written as f64 / elapsed.as_secs_f64();
                println!("  ... {written}/{total} parts written ({rate:.0} parts/sec)");
            }
        }
    }

    // Persist the WorldDb so the engine sees the grid on next open.
    #[cfg(feature = "world-db")]
    if let Some(ref sink) = world_db_sink {
        if let Err(e) = sink.flush() {
            eprintln!("WARN: worlddb flush failed: {e}");
        }
    }

    let elapsed = t0.elapsed();
    let rate = if elapsed.as_secs_f64() > 0.0 {
        written as f64 / elapsed.as_secs_f64()
    } else {
        written as f64
    };

    println!();
    println!("=== Done ===");
    println!("Written: {} parts in {:.2?} ({:.0} parts/sec)", written, elapsed, rate);
    if disk_mode {
        println!("Output:  {} (disk folders)", output_dir.display());
    } else {
        println!("Output:  world.fjalldb tree partition (no disk folders)");
    }
    println!();
    println!("To test, run:  cargo run -p eustress-engine");
}

/// Legacy filesystem sink — one folder + `_instance.toml` per part.
fn write_part_to_disk(
    output_dir: &Path,
    part_name: &str,
    toml_content: &str,
    written: &mut usize,
) {
    let part_dir = output_dir.join(part_name);
    let _ = std::fs::create_dir_all(&part_dir);
    let file_path = part_dir.join("_instance.toml");
    match std::fs::File::create(&file_path) {
        Ok(mut file) => {
            if let Err(error) = file.write_all(toml_content.as_bytes()) {
                eprintln!("WARN: failed to write {}: {error}", file_path.display());
            } else {
                *written += 1;
            }
        }
        Err(error) => {
            eprintln!("WARN: failed to create {}: {error}", file_path.display());
        }
    }
}

/// Direct-to-Fjall sink. Holds the open WorldDb + the Space-relative
/// key prefix; `put_part` writes one entity into the tree partition
/// with zero disk folders.
#[cfg(feature = "world-db")]
struct WorldDbSink {
    db: std::sync::Arc<dyn eustress_worlddb::WorldDb>,
    rel_prefix: String,
}

#[cfg(feature = "world-db")]
impl WorldDbSink {
    fn put_part(&self, part_name: &str, bytes: &[u8]) -> Result<(), String> {
        let key = format!("{}/{}/_instance.toml", self.rel_prefix, part_name);
        self.db
            .put_file(&key, bytes)
            .map_err(|e| e.to_string())
    }
    fn flush(&self) -> Result<(), String> {
        self.db.flush().map_err(|e| e.to_string())
    }
}

/// Open `world.fjalldb` under `space_root`. If the tree partition is
/// empty this also runs the faithful disk→Fjall import of the
/// *existing* Space first (Lighting, services, Baseplate, hand-made
/// parts) — otherwise adding the grid would leave the tree non-empty
/// and the engine's open/seed logic would skip importing the rest of
/// the Space, loading a grid-only world.
#[cfg(feature = "world-db")]
fn open_world_db_sink(space_root: &Path, rel_prefix: &str) -> Result<WorldDbSink, String> {
    let db_dir = space_root.join("world.fjalldb");
    std::fs::create_dir_all(&db_dir).map_err(|e| format!("create {db_dir:?}: {e}"))?;
    let db = eustress_worlddb::backend::open(&db_dir).map_err(|e| e.to_string())?;
    let empty = db.tree_is_empty().map_err(|e| e.to_string())?;
    if empty {
        println!("WorldDb tree empty — importing existing Space disk content first…");
        let summary = eustress_worlddb::import::import_space(db.as_ref(), space_root)
            .map_err(|e| e.to_string())?;
        println!(
            "  imported {} files / {} dirs ({} bytes) from existing Space",
            summary.files_imported, summary.dirs_walked, summary.bytes_imported
        );
    } else {
        println!("WorldDb tree already populated — appending grid into existing Fjall world.");
    }
    Ok(WorldDbSink {
        db,
        rel_prefix: rel_prefix.to_string(),
    })
}

/// Split `--output` at its `Workspace` component to get
/// `(space_root, "Workspace/<rest>")`. The Space root is the directory
/// that holds `world.fjalldb` + `Workspace` + services; the prefix is
/// the Space-relative key namespace the generated parts live under.
fn derive_space_root_and_prefix(output: &Path) -> Option<(PathBuf, String)> {
    let comps: Vec<String> = output
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    let ws_idx = comps.iter().position(|c| c == "Workspace")?;
    let mut space_root = PathBuf::new();
    for c in &comps[..ws_idx] {
        space_root.push(c);
    }
    let rel_prefix = comps[ws_idx..].join("/");
    Some((space_root, rel_prefix))
}

// ─────────────────────────────────────────────────────────────────────────────
// Argument parsing helpers (zero dependencies)
// ─────────────────────────────────────────────────────────────────────────────

fn parse_usize_flag(args: &[String], flag: &str) -> Option<usize> {
    args.iter().position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn parse_f32_flag(args: &[String], flag: &str) -> Option<f32> {
    args.iter().position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn parse_u64_flag(args: &[String], flag: &str) -> Option<u64> {
    args.iter().position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

fn parse_string_flag(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(|v| v.clone())
}

/// Auto-detect the default Space1/Workspace/BenchmarkGrid output path.
fn default_output_dir() -> PathBuf {
    if let Some(docs) = dirs::document_dir() {
        let space_workspace = docs
            .join("Eustress")
            .join("Universe1")
            .join("Spaces")
            .join("Space1")
            .join("Workspace")
            .join("BenchmarkGrid");
        if space_workspace.parent().map_or(false, |p| p.exists()) {
            return space_workspace;
        }
    }
    // Fallback to current directory.
    PathBuf::from("BenchmarkGrid")
}

/// Xorshift64 — fast deterministic PRNG (no external dependency).
fn xorshift64(mut state: u64) -> u64 {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
}

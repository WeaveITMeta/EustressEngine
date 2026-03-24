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
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Parse arguments (simple flag-based, no external dependency).
    let grid_size = parse_usize_flag(&args, "--grid-size").unwrap_or(100);
    let spacing = parse_f32_flag(&args, "--spacing").unwrap_or(4.0);
    let active_pct = parse_f32_flag(&args, "--active-pct").unwrap_or(0.10);
    let seed = parse_u64_flag(&args, "--seed").unwrap_or(42);
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
    println!();

    // Create output directory.
    if let Err(error) = std::fs::create_dir_all(&output_dir) {
        eprintln!("ERROR: cannot create output directory: {error}");
        std::process::exit(1);
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
can_collide = true
cast_shadow = true
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

            let file_path = output_dir.join(format!("{}.part.toml", part_name));
            match std::fs::File::create(&file_path) {
                Ok(mut file) => {
                    if let Err(error) = file.write_all(toml_content.as_bytes()) {
                        eprintln!("WARN: failed to write {}: {error}", file_path.display());
                    } else {
                        written += 1;
                    }
                }
                Err(error) => {
                    eprintln!("WARN: failed to create {}: {error}", file_path.display());
                }
            }

            // Progress reporting every 10K parts.
            if written > 0 && written % 10_000 == 0 {
                let elapsed = t0.elapsed();
                let rate = written as f64 / elapsed.as_secs_f64();
                println!("  ... {written}/{total} parts written ({rate:.0} parts/sec)");
            }
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
    println!("Output:  {}", output_dir.display());
    println!();
    println!("To test streaming, run:");
    println!("  cargo run -p eustress-engine");
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
            .join("spaces")
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

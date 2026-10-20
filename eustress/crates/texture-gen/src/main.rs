//! # texture-gen
//!
//! Generates the bundled material library into
//! `common/assets/materials/textures/`, three maps per textured material
//! (base colour, normal, ORM). Every map is periodic over the tile, so it
//! repeats without a seam; `check` measures that.
//!
//! ```text
//! cargo run -p texture-gen                         # all 18 materials, 2048²
//! cargo run -p texture-gen -- --only brick,grass   # a subset
//! cargo run -p texture-gen -- --preview <dir>      # also write lit review images
//! cargo run -p texture-gen -- check <png|dir>...   # seam / repetition report
//! ```
//!
//! Options: `--size N` (texels per side, default 2048), `--tile-m M` (metres
//! one tile covers, default 4 = the engine's `TILE_WORLD_SIZE`), `--out DIR`.

mod procgen;

use procgen::materials::LIBRARY;
use procgen::{bake, write_png_rgb, write_previews, Ctx};
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("check") {
        std::process::exit(procgen::check::run(&args[1..]));
    }

    let mut size = 2048usize;
    let mut tile_m = 4.0f32;
    let mut out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../common/assets/materials/textures");
    let mut only: Option<Vec<String>> = None;
    let mut preview: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().unwrap_or_else(|| die(&format!("{a} needs a value")));
        match a.as_str() {
            "--size" => size = value().parse().unwrap_or_else(|_| die("--size must be a number")),
            "--tile-m" => tile_m = value().parse().unwrap_or_else(|_| die("--tile-m must be a number")),
            "--out" => out = PathBuf::from(value()),
            "--only" => only = Some(value().split(',').map(|s| s.trim().to_string()).collect()),
            "--preview" => preview = Some(PathBuf::from(value())),
            other => die(&format!("unknown argument {other}")),
        }
    }
    if !size.is_power_of_two() || size < 64 {
        die("--size must be a power of two, at least 64");
    }
    if let Some(names) = &only {
        for n in names {
            if !LIBRARY.iter().any(|m| m.stem == n) {
                die(&format!("unknown material {n}"));
            }
        }
    }
    std::fs::create_dir_all(&out).unwrap_or_else(|e| die(&format!("{}: {e}", out.display())));
    if let Some(p) = &preview {
        std::fs::create_dir_all(p).unwrap_or_else(|e| die(&format!("{}: {e}", p.display())));
    }
    procgen::noise::set_resolution(size);
    let ctx = Ctx { n: size, tile_m };
    let started = Instant::now();
    let mut failed = Vec::new();
    for m in LIBRARY {
        if only.as_ref().is_some_and(|o| !o.iter().any(|n| n == m.stem)) {
            continue;
        }
        let t = Instant::now();
        let baked = bake(&ctx, (m.build)(&ctx));
        let maps = [
            (format!("{}_base_color.png", m.stem), &baked.base),
            (format!("{}_normal.png", m.stem), &baked.normal),
            (format!("{}_metallic_roughness.png", m.stem), &baked.orm),
        ];
        let errors: Vec<String> = std::thread::scope(|s| {
            let jobs: Vec<_> = maps
                .iter()
                .map(|(name, data)| {
                    let path = out.join(name);
                    s.spawn(move || write_png_rgb(&path, size, data).map_err(|e| format!("{}: {e}", path.display())))
                })
                .collect();
            jobs.into_iter().filter_map(|j| j.join().ok().and_then(|r| r.err())).collect()
        });
        if let Some(p) = &preview {
            if let Err(e) = write_previews(p, m.stem, &baked) {
                eprintln!("texture-gen: preview {}: {e}", m.stem);
            }
        }
        if errors.is_empty() {
            println!("  {:<16} {:>6.1} s", m.stem, t.elapsed().as_secs_f32());
        } else {
            for e in &errors {
                eprintln!("texture-gen: {e}");
            }
            failed.push(m.stem);
        }
    }
    println!("done in {:.1} s -> {}", started.elapsed().as_secs_f32(), out.display());
    if !failed.is_empty() {
        die(&format!("failed to write: {}", failed.join(", ")));
    }
}

fn die(msg: &str) -> ! {
    eprintln!("texture-gen: {msg}");
    std::process::exit(2);
}

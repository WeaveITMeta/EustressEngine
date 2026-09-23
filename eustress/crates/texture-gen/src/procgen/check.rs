//! `texture-gen check <png|dir>...` - measures whether maps tile.
//!
//! * **seam** - the mean difference across the wrap line (last column
//!   against the first; last row against the first), divided by the mean
//!   difference between neighbouring columns (rows) in the 16 on either side
//!   of it. A map that tiles has nothing special at its edge, so this sits
//!   near 1. A map that does not tile has a discontinuity there and scores
//!   well above it.
//! * **repeat** - the mean difference between the map and itself shifted a
//!   quarter tile, divided by the same for an off-grid shift. A picture
//!   copied 4×4 scores 0; unique content scores near 1. Regular structures
//!   with per-element variation (a weave, tread-plate lugs, courses of brick)
//!   land in between, because part of their content really does repeat.
//!
//! A map fails if either seam exceeds 1.6 or either repeat is below 0.1.

use std::path::{Path, PathBuf};

pub fn run(args: &[String]) -> i32 {
    let mut files: Vec<PathBuf> = Vec::new();
    for a in args {
        let p = PathBuf::from(a);
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                let mut v: Vec<PathBuf> = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "png"))
                    .collect();
                v.sort();
                files.extend(v);
            }
        } else {
            files.push(p);
        }
    }
    if files.is_empty() {
        eprintln!("usage: texture-gen check <file.png|dir>...");
        return 2;
    }
    println!("{:<44} {:>6} {:>7} {:>7} {:>8} {:>8}  verdict", "map", "size", "seam-u", "seam-v", "repeat-u", "repeat-v");
    let mut failures = 0;
    for f in &files {
        match measure(f) {
            Ok(m) => {
                let pass = m.seam_u <= 1.6 && m.seam_v <= 1.6 && m.repeat_u >= 0.1 && m.repeat_v >= 0.1;
                if !pass {
                    failures += 1;
                }
                println!(
                    "{:<44} {:>6} {:>7.2} {:>7.2} {:>8.2} {:>8.2}  {}",
                    f.file_name().unwrap_or_default().to_string_lossy(),
                    m.width,
                    m.seam_u,
                    m.seam_v,
                    m.repeat_u,
                    m.repeat_v,
                    if pass { "PASS" } else { "FAIL" }
                );
            }
            Err(e) => {
                failures += 1;
                println!("{:<44} error: {e}", f.display());
            }
        }
    }
    println!("{} of {} maps failed", failures, files.len());
    if failures > 0 { 1 } else { 0 }
}

struct Metrics {
    width: usize,
    seam_u: f32,
    seam_v: f32,
    repeat_u: f32,
    repeat_v: f32,
}

fn measure(path: &Path) -> Result<Metrics, String> {
    let (w, h, px) = load_rgb(path)?;
    if w < 64 || h < 64 {
        return Err("too small to measure".into());
    }
    let at = |x: usize, y: usize, c: usize| px[((y % h) * w + (x % w)) * 3 + c] as f32;
    let col_diff = |x: usize| -> f32 {
        let mut s = 0.0;
        for y in 0..h {
            for c in 0..3 {
                s += (at(x + 1, y, c) - at(x, y, c)).abs();
            }
        }
        s / (h * 3) as f32
    };
    let row_diff = |y: usize| -> f32 {
        let mut s = 0.0;
        for x in 0..w {
            for c in 0..3 {
                s += (at(x, y + 1, c) - at(x, y, c)).abs();
            }
        }
        s / (w * 3) as f32
    };
    let seam = |len: usize, diff: &dyn Fn(usize) -> f32| -> f32 {
        let wrap = diff(len - 1);
        let mut near = 0.0;
        let mut k = 0.0;
        for d in 1..=16 {
            near += diff(len - 1 - d) + diff(d - 1);
            k += 2.0;
        }
        let near = near / k;
        if near < 1e-3 { if wrap < 1e-3 { 1.0 } else { 99.0 } } else { wrap / near }
    };
    let shift_diff = |dx: usize, dy: usize| -> f32 {
        let mut s = 0.0;
        let mut k = 0.0;
        for y in (0..h).step_by(3) {
            for x in (0..w).step_by(3) {
                for c in 0..3 {
                    s += (at(x + dx, y + dy, c) - at(x, y, c)).abs();
                }
                k += 3.0;
            }
        }
        s / k
    };
    let ratio = |a: f32, b: f32| if b < 1e-3 { 1.0 } else { a / b };
    Ok(Metrics {
        width: w,
        seam_u: seam(w, &col_diff),
        seam_v: seam(h, &row_diff),
        repeat_u: ratio(shift_diff(w / 4, 0), shift_diff(w / 4 + 7, 0)),
        repeat_v: ratio(shift_diff(0, h / 4), shift_diff(0, h / 4 + 7)),
    })
}

fn load_rgb(path: &Path) -> Result<(usize, usize, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        other => return Err(format!("unsupported colour type {other:?}")),
    };
    let mut rgb = Vec::with_capacity(w * h * 3);
    for p in buf[..w * h * ch].chunks(ch) {
        match ch {
            1 | 2 => rgb.extend_from_slice(&[p[0], p[0], p[0]]),
            _ => rgb.extend_from_slice(&p[..3]),
        }
    }
    Ok((w, h, rgb))
}

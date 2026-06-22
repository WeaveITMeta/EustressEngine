//! Per-part color manifest — emitted once per imported place, one row per
//! colored `BasePart`. Feeds the `tools/color_wheel` study (the Rust/Python
//! loop): the importer is the encoder's *extract* stage.
//!
//! The default emit is newline-delimited JSON (`.ndjson`) — zero new
//! dependencies (the crate already pulls `serde_json`). A columnar parquet
//! emit can be added later behind a feature without changing this row
//! contract.
//!
//! The Morton encoder is copied locally (a few lines) so the manifest stays
//! in the crate's default, engine-free build instead of pulling
//! `eustress-worlddb`; it mirrors that crate's encoder (`chunk_size = 256.0`)
//! so a manifest row is spatially joinable against a worlddb core.

use std::path::Path;

use serde::Serialize;

/// One row of the color manifest: a single colored part.
#[derive(Debug, Clone, Serialize)]
pub struct ColorRow {
    /// Source place name (the Space root directory name).
    pub world_id: String,
    /// First 8 bytes of the part's deterministic UUID as a `u64` — matches
    /// the binary-ECS `stored_id` and the worlddb `EntityId`, so a row joins
    /// against an entity core.
    pub part_id: u64,
    /// sRGB 0-255 (`round(color_rgba * 255)`).
    pub srgb: [u8; 3],
    /// OKLCH `[L, C, hue_deg]` (sRGB-decoded then Ottosson).
    pub oklch: [f32; 3],
    /// Roblox BrickColor token when the color came from a `BrickColor`
    /// property; `None` for raw `Color3` / `Color3uint8`.
    pub roblox_brick: Option<u16>,
    /// Eustress class name (`Part`, `MeshPart`, ...).
    pub class: String,
    /// Morton (Z-order) code of the part's cell — chunk size 256.0, matching
    /// the worlddb key encoder so the manifest is spatially joinable.
    pub morton: u64,
}

/// Spread the low 21 bits of `x` across every third bit (Morton interleave).
/// Mirrors `eustress_worlddb`'s encoder so codes line up for a join.
fn part1by2(x: u32) -> u64 {
    let mut x = (x as u64) & 0x1f_ffff;
    x = (x | (x << 32)) & 0x1f_0000_0000_ffff;
    x = (x | (x << 16)) & 0x1f_0000_ff00_00ff;
    x = (x | (x << 8)) & 0x100f_00f0_0f00_f00f;
    x = (x | (x << 4)) & 0x10c3_0c30_c30c_30c3;
    x = (x | (x << 2)) & 0x1249_2492_4924_9249;
    x
}

/// World coordinate -> non-negative cell index for one axis (chunk `chunk`,
/// biased by 2^20 so negative world space stays in range).
fn world_to_cell(coord: f32, chunk: f32) -> u32 {
    let cell = (coord / chunk).floor() as i64 + (1 << 20);
    cell.clamp(0, 0x1f_ffff) as u32
}

/// Morton code for a world position (chunk size 256.0, matching worlddb).
pub fn morton_for(pos: [f32; 3]) -> u64 {
    let c = |v: f32| world_to_cell(v, 256.0);
    part1by2(c(pos[0])) | (part1by2(c(pos[1])) << 1) | (part1by2(c(pos[2])) << 2)
}

/// sRGB-encoded 0..1 -> OKLCH `[L, C, hue_deg]` (Ottosson). Decodes the sRGB
/// EOTF first — `color_rgba` is sRGB-encoded, NOT linear. Mirror of
/// `eustress_common::brick_palette::srgb_u8_to_oklch` and the Python
/// `tools/color_wheel/oklch.py`, so all three agree.
pub fn srgb_to_oklch(r: f32, g: f32, b: f32) -> [f32; 3] {
    fn lin(u: f32) -> f32 {
        if u <= 0.04045 {
            u / 12.92
        } else {
            ((u + 0.055) / 1.055).powf(2.4)
        }
    }
    let r = lin(r);
    let g = lin(g);
    let b = lin(b);
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();
    let ok_l = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_;
    let ok_a = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_;
    let ok_b = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_;
    let chroma = (ok_a * ok_a + ok_b * ok_b).sqrt();
    let hue = if chroma < 1e-4 {
        0.0
    } else {
        ok_b.atan2(ok_a).to_degrees().rem_euclid(360.0)
    };
    [ok_l, chroma, hue]
}

/// Accumulates [`ColorRow`]s during one import and flushes them once.
#[derive(Default)]
pub struct ColorManifestWriter {
    rows: Vec<ColorRow>,
}

impl ColorManifestWriter {
    /// Append one part's row.
    pub fn push(&mut self, row: ColorRow) {
        self.rows.push(row);
    }

    /// True when no colored parts were seen (skip the flush + report path).
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Number of rows accumulated.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Write the manifest as newline-delimited JSON (one [`ColorRow`] per
    /// line) to `path`. Always available — no columnar deps.
    pub fn flush_ndjson(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
        for row in &self.rows {
            let line = serde_json::to_string(row)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            writeln!(f, "{line}")?;
        }
        f.flush()
    }

    /// Write the manifest as a columnar Parquet file via the `eustress-data`
    /// leaf — the deferred `color_manifest.parquet` from this module's header
    /// note. Each [`ColorRow`] field becomes a column; `srgb`/`oklch` are
    /// flattened into scalar columns (`srgb_r/g/b`, `oklch_l/c/h`) — the flat
    /// table the `tools/color_wheel` encoder consumes. Same row contract as
    /// [`ColorManifestWriter::flush_ndjson`]; available only with the `parquet`
    /// feature.
    ///
    /// `part_id` and `morton` are `u64`; Parquet/Arrow have no `u64`, so they
    /// are stored as `i64` with the bits preserved (`x as i64`) — exact for an
    /// equality join against the binary-ECS `stored_id` / worlddb key.
    #[cfg(feature = "parquet")]
    pub fn flush_parquet(&self, path: &Path) -> std::io::Result<()> {
        use eustress_data::{frame_from_columns, write_parquet, ColumnData, ColumnDtype, ColumnSpec};

        let n = self.rows.len();
        let mut world_id = Vec::with_capacity(n);
        let mut part_id = Vec::with_capacity(n);
        let mut srgb_r = Vec::with_capacity(n);
        let mut srgb_g = Vec::with_capacity(n);
        let mut srgb_b = Vec::with_capacity(n);
        let mut oklch_l = Vec::with_capacity(n);
        let mut oklch_c = Vec::with_capacity(n);
        let mut oklch_h = Vec::with_capacity(n);
        let mut roblox_brick = Vec::with_capacity(n);
        let mut class = Vec::with_capacity(n);
        let mut morton = Vec::with_capacity(n);
        for row in &self.rows {
            world_id.push(Some(row.world_id.clone()));
            part_id.push(Some(row.part_id as i64));
            srgb_r.push(Some(row.srgb[0] as i64));
            srgb_g.push(Some(row.srgb[1] as i64));
            srgb_b.push(Some(row.srgb[2] as i64));
            oklch_l.push(Some(row.oklch[0] as f64));
            oklch_c.push(Some(row.oklch[1] as f64));
            oklch_h.push(Some(row.oklch[2] as f64));
            roblox_brick.push(row.roblox_brick.map(|v| v as i64));
            class.push(Some(row.class.clone()));
            morton.push(Some(row.morton as i64));
        }
        let frame = frame_from_columns(vec![
            (ColumnSpec::new("world_id", ColumnDtype::Str), ColumnData::Str(world_id)),
            (ColumnSpec::new("part_id", ColumnDtype::I64), ColumnData::I64(part_id)),
            (ColumnSpec::new("srgb_r", ColumnDtype::I64), ColumnData::I64(srgb_r)),
            (ColumnSpec::new("srgb_g", ColumnDtype::I64), ColumnData::I64(srgb_g)),
            (ColumnSpec::new("srgb_b", ColumnDtype::I64), ColumnData::I64(srgb_b)),
            (ColumnSpec::new("oklch_l", ColumnDtype::F64), ColumnData::F64(oklch_l)),
            (ColumnSpec::new("oklch_c", ColumnDtype::F64), ColumnData::F64(oklch_c)),
            (ColumnSpec::new("oklch_h", ColumnDtype::F64).with_unit("deg"), ColumnData::F64(oklch_h)),
            (ColumnSpec::new("roblox_brick", ColumnDtype::I64), ColumnData::I64(roblox_brick)),
            (ColumnSpec::new("class", ColumnDtype::Str), ColumnData::Str(class)),
            (ColumnSpec::new("morton", ColumnDtype::I64), ColumnData::I64(morton)),
        ])
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        write_parquet(path, &frame)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklch_black_and_white() {
        let black = srgb_to_oklch(0.0, 0.0, 0.0);
        let white = srgb_to_oklch(1.0, 1.0, 1.0);
        assert!(black[0] < 0.05, "black L = {}", black[0]);
        assert!(white[0] > 0.95, "white L = {}", white[0]);
    }

    #[test]
    fn morton_is_deterministic_and_distinct() {
        let a = morton_for([0.0, 0.0, 0.0]);
        let b = morton_for([1000.0, 0.0, 0.0]);
        assert_eq!(a, morton_for([0.0, 0.0, 0.0]));
        assert_ne!(a, b);
    }

    #[test]
    fn ndjson_round_trips() {
        let mut w = ColorManifestWriter::default();
        w.push(ColorRow {
            world_id: "Place".into(),
            part_id: 42,
            srgb: [196, 40, 28],
            oklch: srgb_to_oklch(196.0 / 255.0, 40.0 / 255.0, 28.0 / 255.0),
            roblox_brick: Some(21),
            class: "Part".into(),
            morton: morton_for([1.0, 2.0, 3.0]),
        });
        assert_eq!(w.len(), 1);
        let dir = std::env::temp_dir().join("eustress_color_manifest_test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("m.ndjson");
        w.flush_ndjson(&p).unwrap();
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("\"roblox_brick\":21"));
        assert_eq!(body.lines().count(), 1);
    }

    #[cfg(feature = "parquet")]
    #[test]
    fn parquet_emit_round_trips() {
        let mut w = ColorManifestWriter::default();
        w.push(ColorRow {
            world_id: "Place".into(),
            part_id: 42,
            srgb: [196, 40, 28],
            oklch: srgb_to_oklch(196.0 / 255.0, 40.0 / 255.0, 28.0 / 255.0),
            roblox_brick: Some(21),
            class: "Part".into(),
            morton: morton_for([1.0, 2.0, 3.0]),
        });
        let dir = std::env::temp_dir().join("eustress_color_manifest_parquet_test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("m.parquet");
        w.flush_parquet(&p).unwrap();
        // Read it back through the leaf to confirm the row + schema survived.
        let frame = eustress_data::read_parquet(&p).unwrap();
        assert_eq!(frame.n_rows(), 1);
        assert_eq!(frame.n_cols(), 11);
    }
}

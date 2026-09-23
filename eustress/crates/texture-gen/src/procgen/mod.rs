//! The material library generator.
//!
//! A material is a function of tile coordinates that returns albedo, height,
//! roughness and metalness. [`bake`] derives the normal map and ambient
//! occlusion from the height field and packs the three output maps:
//!
//! * `<stem>_base_color.png` - sRGB albedo, lightly darkened in crevices;
//! * `<stem>_normal.png` - tangent-space normals, OpenGL convention;
//! * `<stem>_metallic_roughness.png` - glTF ORM packing: R = ambient
//!   occlusion, G = roughness, B = metallic. The library materials point both
//!   their `metallic_roughness` and `occlusion` slots at this one file.

pub mod check;
pub mod color;
pub mod field;
pub mod materials;
pub mod noise;
pub mod raster;

use color::Rgb;
use field::{par_rows, Field};
use std::path::Path;

/// Resolution and physical size of one tile.
pub struct Ctx {
    /// Texels along each side.
    pub n: usize,
    /// World size one tile covers, metres. The engine maps one tile onto this
    /// much of a part's face.
    pub tile_m: f32,
}

impl Ctx {
    pub fn texel_m(&self) -> f32 {
        self.tile_m / self.n as f32
    }
}

/// One texel of a material before baking.
#[derive(Clone, Copy, Default)]
pub struct Px {
    /// Linear albedo.
    pub c: Rgb,
    /// Height, metres.
    pub h: f32,
    /// Perceptual roughness.
    pub r: f32,
    /// Metalness.
    pub m: f32,
}

/// Everything a material produces before normals and occlusion are derived.
pub struct Surface {
    pub albedo: Vec<Rgb>,
    pub height: Field,
    pub roughness: Field,
    pub metallic: Field,
    /// Horizon search radius for ambient occlusion, metres.
    pub ao_radius_m: f32,
    pub ao_strength: f32,
    /// How much occlusion darkens the albedo, so crevices stay dark under
    /// direct light too (the occlusion map only affects ambient light).
    pub cavity: f32,
    pub normal_strength: f32,
    /// Occlusion the height field cannot express, multiplied in.
    pub occlusion: Option<Field>,
}

impl Surface {
    /// Build from a per-texel function evaluated at texel centres.
    pub fn from_fn(ctx: &Ctx, f: impl Fn(f32, f32) -> Px + Sync) -> Surface {
        let n = ctx.n;
        let mut px = vec![Px::default(); n * n];
        par_rows(&mut px, n, |y, row| {
            let v = (y as f32 + 0.5) / n as f32;
            for (x, out) in row.iter_mut().enumerate() {
                *out = f((x as f32 + 0.5) / n as f32, v);
            }
        });
        Surface {
            albedo: px.iter().map(|p| p.c).collect(),
            height: Field { n, data: px.iter().map(|p| p.h).collect() },
            roughness: Field { n, data: px.iter().map(|p| p.r).collect() },
            metallic: Field { n, data: px.iter().map(|p| p.m).collect() },
            ao_radius_m: 0.005,
            ao_strength: 0.5,
            cavity: 0.3,
            normal_strength: 1.0,
            occlusion: None,
        }
    }

    pub fn ao(mut self, radius_m: f32, strength: f32, cavity: f32) -> Self {
        self.ao_radius_m = radius_m;
        self.ao_strength = strength;
        self.cavity = cavity;
        self
    }
}

/// The three packed maps, 8-bit RGB.
pub struct Baked {
    pub n: usize,
    pub base: Vec<[u8; 3]>,
    pub normal: Vec<[u8; 3]>,
    pub orm: Vec<[u8; 3]>,
}

pub fn bake(ctx: &Ctx, s: Surface) -> Baked {
    let n = ctx.n;
    let normals = field::normal_map(&s.height, ctx.texel_m(), s.normal_strength);
    let mut ao = field::ambient_occlusion(&s.height, ctx.texel_m(), s.ao_radius_m, s.ao_strength);
    if let Some(extra) = &s.occlusion {
        for (a, e) in ao.data.iter_mut().zip(&extra.data) {
            *a *= e.clamp(0.0, 1.0);
        }
    }
    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let mut base = vec![[0u8; 3]; n * n];
    let mut normal = vec![[0u8; 3]; n * n];
    let mut orm = vec![[0u8; 3]; n * n];
    for i in 0..n * n {
        let occ = ao.data[i];
        let dim = 1.0 - s.cavity * (1.0 - occ);
        base[i] = color::to_srgb8(color::scale(s.albedo[i], dim));
        let nv = normals[i];
        normal[i] = [q(nv[0] * 0.5 + 0.5), q(nv[1] * 0.5 + 0.5), q(nv[2] * 0.5 + 0.5)];
        orm[i] = [q(occ), q(s.roughness.data[i]), q(s.metallic.data[i])];
    }
    Baked { n, base, normal, orm }
}

pub fn write_png_rgb(path: &Path, n: usize, data: &[[u8; 3]]) -> std::io::Result<()> {
    write_png(path, n, n, data)
}

/// Encode to a sibling temp file, then rename over the target, so an
/// interrupted run never leaves a truncated map behind. The rename is retried
/// briefly because Windows refuses to replace a file another process (an
/// indexer, a thumbnail previewer) has mapped for a moment.
fn write_png(path: &Path, w: usize, h: usize, data: &[[u8; 3]]) -> std::io::Result<()> {
    let tmp = path.with_extension("png.tmp");
    {
        let file = std::fs::File::create(&tmp)?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Best);
        enc.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
        let mut writer = enc.write_header().map_err(std::io::Error::other)?;
        let flat: Vec<u8> = data.iter().flat_map(|p| p.iter().copied()).collect();
        writer.write_image_data(&flat).map_err(std::io::Error::other)?;
        writer.finish().map_err(std::io::Error::other)?;
    }
    let mut attempt = 0;
    loop {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(_) if attempt < 20 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        }
    }
}

/// Two review images per material, lit from the upper left with the baked
/// normal and occlusion maps so relief and its direction are visible:
///
/// * `<stem>_tiled.png` - the tile repeated 2×2 at half resolution;
/// * `<stem>_seam.png` - a full-resolution crop centred on the tile corner,
///   so both wrap seams run through the middle of the frame.
pub fn write_previews(dir: &Path, stem: &str, b: &Baked) -> std::io::Result<()> {
    let n = b.n;
    let lit = |i: usize| -> [u8; 3] {
        let decode = |c: u8| c as f32 / 255.0 * 2.0 - 1.0;
        let nv = [decode(b.normal[i][0]), decode(b.normal[i][1]), decode(b.normal[i][2])];
        let l = [-0.45f32, 0.5, 0.74];
        let ll = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        let ndl = ((nv[0] * l[0] + nv[1] * l[1] + nv[2] * l[2]) / ll).max(0.0);
        let occ = b.orm[i][0] as f32 / 255.0;
        let k = 0.35 * occ + 0.8 * ndl;
        let lin = color::srgb(b.base[i][0], b.base[i][1], b.base[i][2]);
        color::to_srgb8(color::scale(lin, k))
    };
    // 2x2 tiling of the half-resolution tile.
    let mut tiled = vec![[0u8; 3]; n * n];
    for y in 0..n {
        for x in 0..n {
            let sx = (x * 2) % n;
            let sy = (y * 2) % n;
            tiled[y * n + x] = lit(sy * n + sx);
        }
    }
    write_png(&dir.join(format!("{stem}_tiled.png")), n, n, &tiled)?;
    // Full-resolution crop around the corner.
    let c = (n / 4).min(512);
    let mut seam = vec![[0u8; 3]; c * c];
    for y in 0..c {
        for x in 0..c {
            let sx = (x + n - c / 2) % n;
            let sy = (y + n - c / 2) % n;
            seam[y * c + x] = lit(sy * n + sx);
        }
    }
    write_png(&dir.join(format!("{stem}_seam.png")), c, c, &seam)
}

//! Deterministic multi-agent terrain generation (Phase B).
//!
//! A landscape is partitioned into regions; each region is produced by a
//! sequence of deterministic *passes* operating on a plain heightfield +
//! per-cell material array:
//!
//! 1. **base elevation** — domain-warped fbm blended with ridged noise *(this file)*
//! 2. hydrology — D8 flow direction + accumulation *(follows)*
//! 3. erosion — hydraulic + thermal *(follows)*
//! 4. climate / biome — moisture × temperature *(follows)*
//! 5. material — slope × altitude × moisture → [`TerrainMaterial`] *(follows)*
//! 6. detail / foliage hints *(follows)*
//!
//! Because every value is a pure function of `(seed, world coordinates)`,
//! regions are both **reproducible** and **seam-free**: two adjacent
//! regions that sample the same global field agree *exactly* on their
//! shared edge. That exact-edge-agreement is the invariant the gang
//! boundary handshake (Phase C, via `eustress-worlddb` branch + rollout)
//! builds on — neighbours only need to reconcile cross-region *flow*, not
//! the base surface, which already matches.
//!
//! This module is engine-free and Bevy-free (the math is plain `Vec<f32>`
//! / `Vec<u8>`); the engine seam (Phase E) lifts a [`GeneratedRegion`] into
//! `TerrainData.height_cache` + `splat_cache`.

pub mod noise;

/// Parameters for generating ONE region. World coordinates are in engine
/// units (1 unit = 1 metre). A region is an axis-aligned rectangle of the
/// landscape sampled on a `res_x × res_z` grid; sample `(ix, iz)` sits at
/// world `(origin + frac · size)`, with `frac = i / (res - 1)` so the
/// **last column/row lands exactly on the far edge** — that is what makes a
/// neighbouring region's first column coincide and seams vanish.
#[derive(Clone, Debug)]
pub struct GenParams {
    pub seed: u64,
    /// Min-corner world coordinates of the region.
    pub origin_x: f64,
    pub origin_z: f64,
    /// World-space extent of the region (metres).
    pub size_x: f64,
    pub size_z: f64,
    /// Grid resolution (samples per axis); `heights.len() == res_x*res_z`.
    pub res_x: u32,
    pub res_z: u32,
    /// Wavelength of the lowest fbm octave in metres (larger ⇒ broader
    /// landforms). The frequency fed to the noise is `1 / feature_wavelength`.
    pub feature_wavelength: f64,
    pub octaves: u32,
    pub lacunarity: f64,
    pub gain: f64,
    /// Vertical scale (metres) the normalised `[0,1]` field maps onto.
    pub height_scale: f64,
    /// World-Y of sea level — the floor the normalised field is offset from.
    pub sea_level: f64,
    /// Blend `0..1` of ridged (mountain) vs fbm (rolling) base: 0 = all
    /// rolling, 1 = all ridge. The MoE landform expert (Phase D) sets this
    /// per region (alpine → high, plains → low).
    pub ridge_blend: f64,
}

impl Default for GenParams {
    fn default() -> Self {
        Self {
            seed: 0,
            origin_x: 0.0,
            origin_z: 0.0,
            size_x: 256.0,
            size_z: 256.0,
            res_x: 256,
            res_z: 256,
            feature_wavelength: 512.0,
            octaves: 6,
            lacunarity: 2.0,
            gain: 0.5,
            height_scale: 120.0,
            sea_level: 0.0,
            ridge_blend: 0.4,
        }
    }
}

impl GenParams {
    /// World X of grid column `ix` (`0..res_x`); the far column lands on the
    /// region's far edge so neighbours coincide.
    #[inline]
    pub fn world_x(&self, ix: u32) -> f64 {
        if self.res_x <= 1 {
            self.origin_x
        } else {
            self.origin_x + (ix as f64 / (self.res_x - 1) as f64) * self.size_x
        }
    }

    /// World Z of grid row `iz` (`0..res_z`).
    #[inline]
    pub fn world_z(&self, iz: u32) -> f64 {
        if self.res_z <= 1 {
            self.origin_z
        } else {
            self.origin_z + (iz as f64 / (self.res_z - 1) as f64) * self.size_z
        }
    }
}

/// A generated region: heightfield + per-cell material id, row-major
/// (`idx = iz * res_x + ix`).
#[derive(Clone, Debug)]
pub struct GeneratedRegion {
    pub res_x: u32,
    pub res_z: u32,
    /// Height in metres at each sample.
    pub heights: Vec<f32>,
    /// [`TerrainMaterial`](crate::terrain::material::TerrainMaterial)
    /// discriminant per sample. Defaults to `Grass = 0` until the material
    /// pass runs.
    pub materials: Vec<u8>,
}

impl GeneratedRegion {
    pub fn new(res_x: u32, res_z: u32) -> Self {
        let n = (res_x as usize) * (res_z as usize);
        Self {
            res_x,
            res_z,
            heights: vec![0.0; n],
            materials: vec![0u8; n],
        }
    }

    #[inline]
    pub fn idx(&self, ix: u32, iz: u32) -> usize {
        (iz as usize) * (self.res_x as usize) + ix as usize
    }

    #[inline]
    pub fn height(&self, ix: u32, iz: u32) -> f32 {
        self.heights[self.idx(ix, iz)]
    }

    /// Deterministic 64-bit content digest (FNV-1a over heights then
    /// materials) for dedup / regenerate-skip — the terrain-content analogue
    /// of the `eustress-worlddb` branch digest. Identical content hashes
    /// equal; a single changed sample flips it.
    pub fn digest(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
        let mut h = FNV_OFFSET;
        for v in &self.heights {
            for b in v.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
        }
        for b in &self.materials {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }

    /// [`Self::digest`] as a zero-padded 16-char hex string.
    pub fn digest_hex(&self) -> String {
        format!("{:016x}", self.digest())
    }
}

/// Pass 1 — base elevation at a single world point. Domain-warped fbm
/// (rolling) blended with ridged noise (mountains), mapped to metres. A
/// pure function of `(seed, wx, wz)` only — so it is identical no matter
/// which region samples it, which is exactly why region seams disappear.
pub fn base_elevation(p: &GenParams, wx: f64, wz: f64) -> f32 {
    let inv = 1.0 / p.feature_wavelength.max(1e-6);

    // Domain warp: nudge the sample coordinates by a low-frequency noise so
    // landforms aren't axis-aligned. The warp field is itself global, so
    // warped coordinates still agree across region borders.
    let warp_seed = p.seed ^ 0xA5A5_5A5A_1234_9876;
    let warp_amt = p.feature_wavelength * 0.5;
    let wxw = wx + warp_amt * noise::fbm(warp_seed, wx * inv * 0.5, wz * inv * 0.5, 2, p.lacunarity, p.gain);
    let wzw = wz + warp_amt * noise::fbm(warp_seed ^ 0x1111, wx * inv * 0.5, wz * inv * 0.5, 2, p.lacunarity, p.gain);

    // Rolling base in [0,1].
    let rolling = 0.5 * (noise::fbm(p.seed, wxw * inv, wzw * inv, p.octaves, p.lacunarity, p.gain) + 1.0);
    // Mountain belts in [0,1].
    let mountains = noise::ridged(p.seed ^ 0x7777, wxw * inv, wzw * inv, p.octaves, p.lacunarity, p.gain);

    let t = p.ridge_blend.clamp(0.0, 1.0);
    let field = rolling * (1.0 - t) + mountains * t; // [0,1]
    (p.sea_level + field * p.height_scale) as f32
}

/// Run pass 1 over a region's grid, returning a fresh [`GeneratedRegion`]
/// with `heights` filled and `materials` defaulted to Grass.
pub fn generate_base(p: &GenParams) -> GeneratedRegion {
    let mut region = GeneratedRegion::new(p.res_x.max(1), p.res_z.max(1));
    for iz in 0..region.res_z {
        let wz = p.world_z(iz);
        for ix in 0..region.res_x {
            let wx = p.world_x(ix);
            let i = region.idx(ix, iz);
            region.heights[i] = base_elevation(p, wx, wz);
        }
    }
    region
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_params() -> GenParams {
        GenParams {
            res_x: 64,
            res_z: 64,
            size_x: 256.0,
            size_z: 256.0,
            ..Default::default()
        }
    }

    #[test]
    fn base_generation_is_bit_deterministic() {
        let p = small_params();
        let a = generate_base(&p);
        let b = generate_base(&p);
        assert_eq!(a.heights.len(), (p.res_x * p.res_z) as usize);
        assert_eq!(
            a.digest(),
            b.digest(),
            "same params must produce identical terrain"
        );
        for (x, y) in a.heights.iter().zip(b.heights.iter()) {
            assert_eq!(x.to_bits(), y.to_bits(), "height not bit-identical");
        }
    }

    #[test]
    fn different_seed_changes_terrain() {
        let a = generate_base(&small_params());
        let b = generate_base(&GenParams {
            seed: 99,
            ..small_params()
        });
        assert_ne!(a.digest(), b.digest(), "seed must change the world");
    }

    #[test]
    fn heights_within_scale_band() {
        let p = small_params();
        let r = generate_base(&p);
        let lo = p.sea_level as f32;
        let hi = (p.sea_level + p.height_scale) as f32;
        for &h in &r.heights {
            assert!(h >= lo - 1e-3 && h <= hi + 1e-3, "height {h} outside [{lo},{hi}]");
        }
    }

    #[test]
    fn adjacent_regions_are_seamless() {
        // Region A spans world X [0,256]; region B spans [256,512]. With the
        // far column landing exactly on the edge, A's last column and B's
        // first column sample the SAME world line — base_elevation is global,
        // so the shared edge must match to the bit. This is the property the
        // gang boundary handshake assumes for the base surface.
        let base = GenParams {
            res_x: 65,
            res_z: 65,
            size_x: 256.0,
            size_z: 256.0,
            ..Default::default()
        };
        let region_a = generate_base(&base);
        let region_b = generate_base(&GenParams {
            origin_x: 256.0,
            ..base.clone()
        });

        let last = region_a.res_x - 1;
        for iz in 0..base.res_z {
            let edge_a = region_a.height(last, iz);
            let edge_b = region_b.height(0, iz);
            assert_eq!(
                edge_a.to_bits(),
                edge_b.to_bits(),
                "seam mismatch at row {iz}: A={edge_a} B={edge_b}"
            );
        }
    }

    #[test]
    fn ridge_blend_raises_mean_relief() {
        // A fully-ridged region should not be identical to a fully-rolling
        // one (sanity that the blend knob actually does something).
        let rolling = generate_base(&GenParams { ridge_blend: 0.0, ..small_params() });
        let alpine = generate_base(&GenParams { ridge_blend: 1.0, ..small_params() });
        assert_ne!(rolling.digest(), alpine.digest());
    }
}

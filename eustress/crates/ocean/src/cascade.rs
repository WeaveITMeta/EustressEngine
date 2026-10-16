//! One cascade: a square tile of the sea, synthesised by inverse FFT from
//! the wavevectors of one band.
//!
//! # Amplitudes
//!
//! On an N x N lattice of wavevectors k = 2 pi (fx, fz) / L, with fx and fz
//! in [-N/2, N/2) and L the tile size, every mode the cascade owns gets
//!
//! h0(k) = (xi_r + i xi_i) sqrt(S(kx, kz) dkx dkz) / 2,
//!
//! xi_r and xi_i independent standard normals keyed by (seed, cascade, fx,
//! fz) (see [`crate::rng`]). The surface is the plain sum of its modes,
//! eta(x) = sum_k h(k) e^{i k.x}, and the evolution below pairs h0(k) with
//! conj(h0(-k)), so the wave on lattice point k has expected variance
//! E|h0(k)|^2 + E|h0(-k)|^2. The factor 1/2 makes the lattice sum
//! sum_k S(k) dk^2, the spectrum's variance, and not twice it: with
//! sqrt(S dk^2 / 2), the form often quoted from Tessendorf, Hs would come
//! out sqrt(2) too large. Modes outside the band, the zero mode, and the
//! Nyquist row and column (which have no partner -k on the lattice) are 0.
//!
//! # Time
//!
//! h(k, t) = h0(k) e^{-i w t} + conj(h0(-k)) e^{+i w t},   w = w(|k|).
//!
//! It is Hermitian, h(-k) = conj(h(k)), so the surface is real. Tessendorf
//! writes e^{+iwt} on h0(k); with the e^{+ik.x} synthesis sum that sends
//! h0(k) toward -k, against the direction the spectrum gave it. With the
//! sign here a mode is a cos(k.x - w t + phase): it travels toward +k at the
//! phase speed w / k, which the tests measure. The phase w t is reduced
//! modulo 2 pi in f64, so long runs keep full precision.
//!
//! # Fields
//!
//! With lambda the choppiness (1 is physical):
//!
//! - height: h
//! - horizontal displacement (Dx, Dz): lambda i (k / |k|) h
//! - slopes (dh/dx, dh/dz): i kx h, i kz h
//! - dDx/dx, dDz/dz, dDx/dz: -lambda (kx^2, kz^2, kx kz) h / |k|
//!
//! For a single wave eta = a cos(theta) the displacement is -a sin(theta)
//! along k: water moves toward the crests, which sharpen, and away from the
//! troughs, which flatten (Gerstner's trochoid at lambda = 1). Tessendorf
//! writes -i rather than +i, which with this synthesis sum would sharpen
//! the troughs instead; implementations flip the sign of lambda. The
//! Jacobian of the horizontal map x -> x + D(x),
//!
//! J = (1 + dDx/dx)(1 + dDz/dz) - (dDx/dz)^2,
//!
//! is below 1 where the surface is compressed toward a crest and below 0
//! where it folds over: breaking crests, where whitecaps go.
//!
//! # Packing
//!
//! All eight fields are real, so their spectra are Hermitian, and one
//! complex transform carries two of them: the synthesis of A + iB is a + ib.
//! Four inverse FFTs per cascade per update: (height, Dx), (Dz, dh/dx),
//! (dh/dz, dDx/dx), (dDz/dz, dDx/dz).
//!
//! # Layout
//!
//! Spectra are in FFT order: index i holds frequency i below N/2 and i - N
//! from N/2. Output texel (ix, iz), at index iz N + ix (row-major, rows along
//! z), holds the field at tile coordinates (ix L / N, iz L / N); the tile
//! repeats with period L in x and z and its origin is at world (0, 0).

use std::f64::consts::TAU;
use std::fmt;

use rayon::prelude::*;

use crate::fft::{Complex, Fft2d};
use crate::rng;
#[cfg(test)]
use crate::spectrum::Dispersion;
use crate::spectrum::Spectrum;

/// Displacement of still water.
const REST_DISPLACEMENT: [f32; 4] = [0.0; 4];
/// Slopes and Jacobian of still water: J = 1, nothing compressed.
const REST_DERIVATIVES: [f32; 4] = [0.0, 0.0, 1.0, 0.0];

/// Which displacement fields a sample reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fields {
    /// From the latest update.
    Current,
    /// From the update before it.
    Previous,
}

/// One tile of the sea and the band of wavenumbers it owns.
#[derive(Clone)]
pub struct Cascade {
    index: u32,
    resolution: usize,
    tile_size: f32,
    k_min: f32,
    k_max: f32,
    /// Any mode nonzero.
    active: bool,
    expected_variance: f64,
    h0: Vec<Complex>,
    /// conj(h0(-k)) at index k.
    h0_minus_conj: Vec<Complex>,
    /// w(|k|) (rad/s).
    omega: Vec<f32>,
    /// (Dx, height, Dz, 0) per texel.
    displacement: Vec<[f32; 4]>,
    /// The displacement of the update before.
    previous: Vec<[f32; 4]>,
    /// (dh/dx, dh/dz, J, 0) per texel.
    derivatives: Vec<[f32; 4]>,
    /// Four packed spectra of N^2 values each, transformed in place.
    packed: Vec<Complex>,
    scratch: Vec<Complex>,
}

impl Cascade {
    /// An empty cascade of resolution N (a power of two), still water.
    pub(crate) fn new(index: u32, resolution: usize) -> Self {
        assert!(
            resolution >= 2 && resolution.is_power_of_two(),
            "cascade resolution must be a power of two, got {resolution}"
        );
        let nn = resolution * resolution;
        Self {
            index,
            resolution,
            tile_size: 1.0,
            k_min: 0.0,
            k_max: 0.0,
            active: false,
            expected_variance: 0.0,
            h0: vec![Complex::ZERO; nn],
            h0_minus_conj: vec![Complex::ZERO; nn],
            omega: vec![0.0; nn],
            displacement: vec![REST_DISPLACEMENT; nn],
            previous: vec![REST_DISPLACEMENT; nn],
            derivatives: vec![REST_DERIVATIVES; nn],
            packed: vec![Complex::ZERO; 4 * nn],
            scratch: vec![Complex::ZERO; nn],
        }
    }

    /// Sets the tile size (m) and the owned band k_min <= |k| < k_max
    /// (rad/m). Takes effect at the next [`Cascade::build`].
    pub(crate) fn configure(&mut self, tile_size: f32, k_min: f32, k_max: f32) {
        self.tile_size = tile_size;
        self.k_min = k_min;
        self.k_max = k_max;
    }

    /// Draws the amplitudes h0 of the owned modes from `spectrum` and resets
    /// the fields to still water until the next evaluation. Reuses every
    /// buffer.
    pub(crate) fn build(&mut self, spectrum: &Spectrum, seed: u64) {
        let n = self.resolution;
        let dk = self.lattice_step();
        let cell = dk as f64 * dk as f64;
        let (index, k_min, k_max) = (self.index, self.k_min, self.k_max);
        // Per-row sums, added in row order: the total does not depend on
        // the thread count.
        let row_variance: Vec<f64> = self
            .h0
            .par_chunks_mut(n)
            .zip(self.omega.par_chunks_mut(n))
            .enumerate()
            .map(|(iz, (h0_row, omega_row))| {
                let fz = frequency(iz, n);
                let mut variance = 0.0f64;
                for (ix, (h0, omega)) in h0_row.iter_mut().zip(omega_row.iter_mut()).enumerate() {
                    let fx = frequency(ix, n);
                    let (kx, kz) = wavevector64(fx, fz, dk);
                    let k = (kx * kx + kz * kz).sqrt();
                    *omega = spectrum.dispersion.omega64(k) as f32;
                    *h0 = if in_band(fx, fz, k, n, k_min, k_max) {
                        let mode_variance = spectrum.wavenumber_density64(kx, kz) * cell;
                        variance += mode_variance;
                        let amplitude = (0.5 * mode_variance.sqrt()) as f32;
                        let (xr, xi) = rng::mode_normals(seed, index, fx, fz);
                        Complex::new(xr * amplitude, xi * amplitude)
                    } else {
                        Complex::ZERO
                    };
                }
                variance
            })
            .collect();
        self.expected_variance = row_variance.iter().sum();
        let h0 = &self.h0;
        self.h0_minus_conj.par_chunks_mut(n).enumerate().for_each(|(iz, row)| {
            let mirror_row = ((n - iz) % n) * n;
            for (ix, v) in row.iter_mut().enumerate() {
                *v = h0[mirror_row + (n - ix) % n].conj();
            }
        });
        self.active = self.h0.iter().any(|v| *v != Complex::ZERO);
        self.reset_fields();
    }

    fn reset_fields(&mut self) {
        self.displacement.fill(REST_DISPLACEMENT);
        self.previous.fill(REST_DISPLACEMENT);
        self.derivatives.fill(REST_DERIVATIVES);
    }

    /// Synthesises the fields at time `time_s` (s) with horizontal
    /// displacement scaled by `choppiness`; the fields of the last
    /// evaluation become the previous ones.
    pub(crate) fn evaluate(&mut self, time_s: f64, choppiness: f32, fft: &Fft2d) {
        if !self.active {
            return;
        }
        let n = self.resolution;
        let nn = n * n;
        assert_eq!(fft.size(), n, "cascade of resolution {n} given an FFT of size {}", fft.size());
        let dk = self.lattice_step();
        let lambda = choppiness;
        {
            let (h0, h0_minus_conj, omega) = (&self.h0, &self.h0_minus_conj, &self.omega);
            let (p0, rest) = self.packed.split_at_mut(nn);
            let (p1, rest) = rest.split_at_mut(nn);
            let (p2, p3) = rest.split_at_mut(nn);
            p0.par_chunks_mut(n)
                .zip(p1.par_chunks_mut(n))
                .zip(p2.par_chunks_mut(n))
                .zip(p3.par_chunks_mut(n))
                .enumerate()
                .for_each(|(iz, (((r0, r1), r2), r3))| {
                    let kz = frequency(iz, n) as f32 * dk;
                    for ix in 0..n {
                        let i = iz * n + ix;
                        let kx = frequency(ix, n) as f32 * dk;
                        let k = (kx * kx + kz * kz).sqrt();
                        let (a, b) = (h0[i], h0_minus_conj[i]);
                        if (a == Complex::ZERO && b == Complex::ZERO) || !(k > 0.0) {
                            r0[ix] = Complex::ZERO;
                            r1[ix] = Complex::ZERO;
                            r2[ix] = Complex::ZERO;
                            r3[ix] = Complex::ZERO;
                            continue;
                        }
                        let (ux, uz) = (kx / k, kz / k);
                        let h = evolve(a, b, omega[i], time_s);
                        let ih = h.mul_i();
                        let dx = ih.scale(lambda * ux);
                        let dz = ih.scale(lambda * uz);
                        let slope_x = ih.scale(kx);
                        let slope_z = ih.scale(kz);
                        let dxx = h.scale(-lambda * kx * ux);
                        let dzz = h.scale(-lambda * kz * uz);
                        let dxz = h.scale(-lambda * kx * uz);
                        r0[ix] = h + dx.mul_i();
                        r1[ix] = dz + slope_x.mul_i();
                        r2[ix] = slope_z + dxx.mul_i();
                        r3[ix] = dzz + dxz.mul_i();
                    }
                });
        }
        for plane in self.packed.chunks_exact_mut(nn) {
            fft.inverse(plane, &mut self.scratch);
        }
        std::mem::swap(&mut self.displacement, &mut self.previous);
        let packed = &self.packed;
        self.displacement
            .par_chunks_mut(n)
            .zip(self.derivatives.par_chunks_mut(n))
            .enumerate()
            .for_each(|(iz, (displacement, derivatives))| {
                for ix in 0..n {
                    let i = iz * n + ix;
                    // (height, Dx), (Dz, dh/dx), (dh/dz, dDx/dx), (dDz/dz, dDx/dz).
                    let (a, b, c, d) = (packed[i], packed[nn + i], packed[2 * nn + i], packed[3 * nn + i]);
                    displacement[ix] = [a.im, a.re, b.re, 0.0];
                    let jacobian = (1.0 + c.im) * (1.0 + d.re) - d.im * d.im;
                    derivatives[ix] = [b.im, c.re, jacobian, 0.0];
                }
            });
    }

    /// Position of this cascade among the ocean's, 0 the largest tile.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Lattice side N.
    pub fn resolution(&self) -> usize {
        self.resolution
    }

    /// Tile edge length L (m).
    pub fn tile_size(&self) -> f32 {
        self.tile_size
    }

    /// Texel spacing L / N (m).
    pub fn texel_size(&self) -> f32 {
        self.tile_size / self.resolution as f32
    }

    /// Lattice step dk = 2 pi / L (rad/m).
    pub fn lattice_step(&self) -> f32 {
        (TAU / self.tile_size as f64) as f32
    }

    /// The owned band [k_min, k_max) of |k| (rad/m).
    pub fn band(&self) -> [f32; 2] {
        [self.k_min, self.k_max]
    }

    /// Expected height variance of the owned modes, sum S(k) dk^2 (m^2):
    /// from the spectrum, not the realisation.
    pub fn expected_variance(&self) -> f64 {
        self.expected_variance
    }

    /// (Dx, height, Dz, 0) per texel (m), row-major N x N, for upload as a
    /// texture.
    pub fn displacement(&self) -> &[[f32; 4]] {
        &self.displacement
    }

    /// (dh/dx, dh/dz, J, 0) per texel, row-major N x N, for upload as a
    /// texture. J < 1 marks compressed crests and J < 0 folded ones.
    pub fn derivatives(&self) -> &[[f32; 4]] {
        &self.derivatives
    }

    /// Wavevector (kx, kz) of lattice frequency (fx, fz) (rad/m).
    pub fn wavevector(&self, fx: i32, fz: i32) -> [f32; 2] {
        let dk = self.lattice_step();
        [fx as f32 * dk, fz as f32 * dk]
    }

    /// Whether lattice mode (fx, fz) belongs to this cascade's band.
    pub fn owns(&self, fx: i32, fz: i32) -> bool {
        let half = (self.resolution / 2) as i32;
        if fx < -half || fx >= half || fz < -half || fz >= half {
            return false;
        }
        let (kx, kz) = wavevector64(fx, fz, self.lattice_step());
        in_band(fx, fz, (kx * kx + kz * kz).sqrt(), self.resolution, self.k_min, self.k_max)
    }

    /// This cascade's displacement (Dx, height, Dz) at world (x, z) (m),
    /// bilinear between texels, wrapping with the tile period.
    pub fn sample(&self, x: f32, z: f32) -> [f32; 3] {
        self.sample_fields(Fields::Current, x as f64, z as f64)
    }

    pub(crate) fn sample_fields(&self, fields: Fields, x: f64, z: f64) -> [f32; 3] {
        let buffer = match fields {
            Fields::Current => &self.displacement,
            Fields::Previous => &self.previous,
        };
        self.bilinear(buffer, x, z)
    }

    /// This cascade's slopes and folding Jacobian (dh/dx, dh/dz, J) at world
    /// (x, z), bilinear between texels, wrapping with the tile period: the
    /// derivatives' counterpart of [`Cascade::sample`]. The derivatives are
    /// taken with respect to the rest position, the coordinates the texels
    /// are laid out in, so (x, z) is a rest position too (see
    /// [`crate::Ocean::derivatives_at`] for the surface above a world point).
    /// Still water reads [0, 0, 1].
    pub fn sample_derivatives(&self, x: f32, z: f32) -> [f32; 3] {
        self.sample_derivatives64(x as f64, z as f64)
    }

    pub(crate) fn sample_derivatives64(&self, x: f64, z: f64) -> [f32; 3] {
        self.bilinear(&self.derivatives, x, z)
    }

    /// The first three channels of `buffer` (N x N texels, row-major) at
    /// world (x, z), bilinear between texels, wrapping with the tile period.
    fn bilinear(&self, buffer: &[[f32; 4]], x: f64, z: f64) -> [f32; 3] {
        let n = self.resolution;
        let per_metre = n as f64 / self.tile_size as f64;
        let (ix0, fx) = wrap(x * per_metre, n);
        let (iz0, fz) = wrap(z * per_metre, n);
        let (ix1, iz1) = ((ix0 + 1) % n, (iz0 + 1) % n);
        let t00 = buffer[iz0 * n + ix0];
        let t10 = buffer[iz0 * n + ix1];
        let t01 = buffer[iz1 * n + ix0];
        let t11 = buffer[iz1 * n + ix1];
        let mut out = [0.0f32; 3];
        for (c, o) in out.iter_mut().enumerate() {
            let near = t00[c] + (t10[c] - t00[c]) * fx;
            let far = t01[c] + (t11[c] - t01[c]) * fx;
            *o = near + (far - near) * fz;
        }
        out
    }

    /// Replaces the spectrum with one mode: h0 = `amplitude` at (fx, fz) and
    /// 0 elsewhere, so the height is 2 |amplitude| cos(k.x - w t + arg),
    /// with w from `dispersion`.
    #[cfg(test)]
    pub(crate) fn set_single_mode(&mut self, fx: i32, fz: i32, amplitude: Complex, dispersion: &Dispersion) {
        let n = self.resolution;
        let dk = self.lattice_step();
        self.h0.fill(Complex::ZERO);
        self.h0_minus_conj.fill(Complex::ZERO);
        for iz in 0..n {
            for ix in 0..n {
                let (kx, kz) = wavevector64(frequency(ix, n), frequency(iz, n), dk);
                self.omega[iz * n + ix] = dispersion.omega64((kx * kx + kz * kz).sqrt()) as f32;
            }
        }
        let at = |fx: i32, fz: i32| fz.rem_euclid(n as i32) as usize * n + fx.rem_euclid(n as i32) as usize;
        self.h0[at(fx, fz)] = amplitude;
        self.h0_minus_conj[at(-fx, -fz)] = amplitude.conj();
        self.active = true;
        self.expected_variance = 2.0 * amplitude.norm_sqr() as f64;
        self.reset_fields();
    }

    /// h(k, t) at every lattice point, in FFT order.
    #[cfg(test)]
    pub(crate) fn spectrum_at(&self, time_s: f64) -> Vec<Complex> {
        (0..self.h0.len()).map(|i| evolve(self.h0[i], self.h0_minus_conj[i], self.omega[i], time_s)).collect()
    }
}

impl fmt::Debug for Cascade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cascade")
            .field("index", &self.index)
            .field("resolution", &self.resolution)
            .field("tile_size", &self.tile_size)
            .field("band", &[self.k_min, self.k_max])
            .field("expected_variance", &self.expected_variance)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

/// Signed lattice frequency of FFT index `i`: i below n/2, i - n from n/2.
#[inline]
pub(crate) fn frequency(i: usize, n: usize) -> i32 {
    if i < n / 2 {
        i as i32
    } else {
        i as i32 - n as i32
    }
}

#[inline]
fn wavevector64(fx: i32, fz: i32, dk: f32) -> (f64, f64) {
    (fx as f64 * dk as f64, fz as f64 * dk as f64)
}

/// Whether a lattice of side `n` and band [k_min, k_max) owns mode
/// (fx, fz) of |k| = `k`. The Nyquist row and column (f = -n/2) have no
/// partner -k on the lattice and stay empty.
#[inline]
fn in_band(fx: i32, fz: i32, k: f64, n: usize, k_min: f32, k_max: f32) -> bool {
    let nyquist = -((n / 2) as i32);
    fx != nyquist && fz != nyquist && k > 0.0 && k >= k_min as f64 && k < k_max as f64
}

/// h(k, t) = h0(k) e^{-i w t} + conj(h0(-k)) e^{+i w t}, the phase reduced
/// modulo 2 pi in f64.
#[inline]
fn evolve(h0: Complex, h0_minus_conj: Complex, omega: f32, time_s: f64) -> Complex {
    let phase = (omega as f64 * time_s).rem_euclid(TAU);
    let (s, c) = phase.sin_cos();
    let (s, c) = (s as f32, c as f32);
    h0 * Complex::new(c, -s) + h0_minus_conj * Complex::new(c, s)
}

/// Texel and fraction of lattice coordinate `u`, wrapped into [0, n).
#[inline]
fn wrap(u: f64, n: usize) -> (usize, f32) {
    let u = u.rem_euclid(n as f64);
    let cell = u.floor();
    ((cell as usize) % n, (u - cell) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequencies_follow_fft_order() {
        let f: Vec<i32> = (0..8).map(|i| frequency(i, 8)).collect();
        assert_eq!(f, vec![0, 1, 2, 3, -4, -3, -2, -1]);
    }

    #[test]
    fn nyquist_and_zero_modes_are_never_owned() {
        let mut c = Cascade::new(0, 16);
        c.configure(100.0, 0.0, f32::INFINITY);
        assert!(!c.owns(0, 0));
        assert!(!c.owns(-8, 3) && !c.owns(3, -8));
        assert!(c.owns(7, -7) && c.owns(1, 0));
        assert!(!c.owns(8, 0), "outside the lattice");
    }

    /// The cell is always on the lattice; the fraction may round up to
    /// exactly 1 in f32, which bilinear weights handle (it reads the next
    /// texel, the same value the next cell would start from).
    #[test]
    fn wrap_stays_in_range() {
        for u in [-1e-17, -0.5, 0.0, 15.999_999_999_999_998, 16.0, 1.0e9 + 0.25] {
            let (cell, frac) = wrap(u, 16);
            assert!(cell < 16 && (0.0..=1.0).contains(&frac), "u {u}: {cell} {frac}");
        }
        assert_eq!(wrap(-0.5, 16), (15, 0.5));
        assert_eq!(wrap(1.0e9 + 0.25, 16), (0, 0.25));
    }

    /// The derivative sampler reads the derivative texels the way the
    /// displacement sampler reads the displacement ones: exactly on a texel
    /// (tile and lattice are powers of two, so texel positions are exact in
    /// binary), the mean halfway between two, the same a whole number of
    /// tiles away, and [0, 0, 1] for still water.
    #[test]
    fn derivative_sampling_matches_the_texels_and_wraps() {
        let (n, tile) = (16usize, 64.0f32);
        let mut c = Cascade::new(0, n);
        c.configure(tile, 0.0, f32::INFINITY);
        assert_eq!(c.sample_derivatives(12.3, -45.6), [0.0, 0.0, 1.0], "still water");
        c.set_single_mode(2, 1, Complex::new(0.4, 0.1), &Dispersion::deep(9.80665));
        c.evaluate(1.5, 1.0, &Fft2d::new(n));
        let texel = c.texel_size();
        let at = |ix: usize, iz: usize| c.derivatives()[iz * n + ix];
        let mut busy = false;
        for (ix, iz) in [(0usize, 0usize), (3, 7), (15, 15), (8, 1)] {
            let (x, z) = (ix as f32 * texel, iz as f32 * texel);
            let t = at(ix, iz);
            busy |= t[0].abs() > 1e-3 && (t[2] - 1.0).abs() > 1e-3;
            assert_eq!(c.sample_derivatives(x, z), [t[0], t[1], t[2]], "texel ({ix}, {iz})");
            assert_eq!(c.sample_derivatives(x + tile, z - 2.0 * tile), [t[0], t[1], t[2]], "texel ({ix}, {iz}) tiles away");
            let next = at((ix + 1) % n, iz);
            let mid = c.sample_derivatives(x + 0.5 * texel, z);
            for j in 0..3 {
                assert!((mid[j] - 0.5 * (t[j] + next[j])).abs() < 1e-6, "halfway after texel ({ix}, {iz}): {mid:?}");
            }
        }
        assert!(busy, "the test needs nonzero slopes and a Jacobian away from 1");
    }
}

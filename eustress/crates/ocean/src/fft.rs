//! Radix-2 fast Fourier transform for the wave synthesis.
//!
//! Iterative Cooley-Tukey, decimation in time, for power-of-two sizes: the
//! input is put in bit-reversed order, then log2(n) passes of butterflies
//! combine transforms of size 2, 4, ..., n. The twiddle factors (computed in
//! f64, stored in f32) and the bit-reversal permutation are built once per
//! size.
//!
//! The 2D transform runs the 1D one over every row, transposes, runs it over
//! every row again (the original columns) and transposes back. Rows go to
//! rayon in parallel; each row is transformed by one thread in a fixed order
//! of operations and the transposes only copy, so the result is bit-for-bit
//! the same for any thread count.
//!
//! Sign and scale: [`Fft::inverse`] computes
//! x[m] = sum_k X[k] e^{+2 pi i k m / n} without a 1/n factor, the synthesis
//! sum the ocean needs (a field is the plain sum of its Fourier modes);
//! [`Fft::forward`] computes X[k] = sum_m x[m] e^{-2 pi i k m / n}, also
//! unscaled, so a forward then an inverse pass multiplies by n (n^2 in 2D).

use std::ops::{Add, AddAssign, Mul, Neg, Sub};

use rayon::prelude::*;

/// A complex number in single precision.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex {
    pub re: f32,
    pub im: f32,
}

impl Complex {
    pub const ZERO: Self = Self { re: 0.0, im: 0.0 };

    #[inline]
    pub const fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    #[inline]
    pub fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }

    #[inline]
    pub fn scale(self, s: f32) -> Self {
        Self::new(self.re * s, self.im * s)
    }

    /// i z.
    #[inline]
    pub fn mul_i(self) -> Self {
        Self::new(-self.im, self.re)
    }

    /// |z|^2.
    #[inline]
    pub fn norm_sqr(self) -> f32 {
        self.re * self.re + self.im * self.im
    }
}

impl Add for Complex {
    type Output = Self;
    #[inline]
    fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }
}

impl AddAssign for Complex {
    #[inline]
    fn add_assign(&mut self, o: Self) {
        self.re += o.re;
        self.im += o.im;
    }
}

impl Sub for Complex {
    type Output = Self;
    #[inline]
    fn sub(self, o: Self) -> Self {
        Self::new(self.re - o.re, self.im - o.im)
    }
}

impl Mul for Complex {
    type Output = Self;
    #[inline]
    fn mul(self, o: Self) -> Self {
        Self::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
}

impl Neg for Complex {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self::new(-self.re, -self.im)
    }
}

/// A 1D transform of one power-of-two size, with its tables.
#[derive(Clone)]
pub struct Fft {
    n: usize,
    /// e^{+2 pi i j / n} for j in [0, n/2): the inverse transform's sign.
    twiddles: Vec<Complex>,
    /// Bit-reversed position of each index.
    bitrev: Vec<u32>,
}

impl Fft {
    /// Tables for size `n`, a power of two of at least 2.
    pub fn new(n: usize) -> Self {
        assert!(
            n >= 2 && n.is_power_of_two() && n <= 1 << 30,
            "FFT size must be a power of two from 2 to 2^30, got {n}"
        );
        let bits = n.trailing_zeros();
        let twiddles = (0..n / 2)
            .map(|j| {
                let a = std::f64::consts::TAU * j as f64 / n as f64;
                Complex::new(a.cos() as f32, a.sin() as f32)
            })
            .collect();
        let bitrev = (0..n as u32).map(|i| i.reverse_bits() >> (32 - bits)).collect();
        Self { n, twiddles, bitrev }
    }

    /// The transform size.
    pub fn size(&self) -> usize {
        self.n
    }

    /// In place: x[m] = sum_k X[k] e^{+2 pi i k m / n}, unscaled.
    pub fn inverse(&self, data: &mut [Complex]) {
        self.run(data, false);
    }

    /// In place: X[k] = sum_m x[m] e^{-2 pi i k m / n}, unscaled.
    pub fn forward(&self, data: &mut [Complex]) {
        self.run(data, true);
    }

    fn run(&self, data: &mut [Complex], forward: bool) {
        let n = self.n;
        assert_eq!(data.len(), n, "FFT of size {n} given {} values", data.len());
        for (i, &r) in self.bitrev.iter().enumerate() {
            let j = r as usize;
            if j > i {
                data.swap(i, j);
            }
        }
        let mut half = 1;
        while half < n {
            let stride = n / (2 * half);
            for block in data.chunks_exact_mut(2 * half) {
                let (lo, hi) = block.split_at_mut(half);
                for j in 0..half {
                    let w = self.twiddles[j * stride];
                    let w = if forward { w.conj() } else { w };
                    let t = hi[j] * w;
                    let u = lo[j];
                    lo[j] = u + t;
                    hi[j] = u - t;
                }
            }
            half *= 2;
        }
    }
}

impl std::fmt::Debug for Fft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fft").field("n", &self.n).finish_non_exhaustive()
    }
}

/// A 2D transform of n x n row-major arrays.
#[derive(Clone, Debug)]
pub struct Fft2d {
    fft: Fft,
}

impl Fft2d {
    /// Tables for n x n arrays, n a power of two of at least 2.
    pub fn new(n: usize) -> Self {
        Self { fft: Fft::new(n) }
    }

    /// The side n.
    pub fn size(&self) -> usize {
        self.fft.n
    }

    /// In place: x[mz][mx] = sum_k X[kz][kx] e^{+2 pi i (kx mx + kz mz) / n},
    /// unscaled. `scratch` must hold n^2 values; its contents are discarded.
    pub fn inverse(&self, data: &mut [Complex], scratch: &mut [Complex]) {
        self.run(data, scratch, false);
    }

    /// In place: X[kz][kx] = sum_m x[mz][mx] e^{-2 pi i (kx mx + kz mz) / n},
    /// unscaled. `scratch` must hold n^2 values; its contents are discarded.
    pub fn forward(&self, data: &mut [Complex], scratch: &mut [Complex]) {
        self.run(data, scratch, true);
    }

    fn run(&self, data: &mut [Complex], scratch: &mut [Complex], forward: bool) {
        let n = self.fft.n;
        assert_eq!(data.len(), n * n, "2D FFT of side {n} given {} values", data.len());
        assert_eq!(scratch.len(), n * n, "2D FFT of side {n} given {} scratch values", scratch.len());
        let pass = |row: &mut [Complex]| self.fft.run(row, forward);
        data.par_chunks_mut(n).for_each(pass);
        transpose(data, scratch, n);
        scratch.par_chunks_mut(n).for_each(pass);
        transpose(scratch, data, n);
    }
}

/// dst = src transposed, both n x n row-major.
fn transpose(src: &[Complex], dst: &mut [Complex], n: usize) {
    dst.par_chunks_mut(n).enumerate().for_each(|(r, row)| {
        for (c, v) in row.iter_mut().enumerate() {
            *v = src[c * n + r];
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::splitmix64;
    use std::f64::consts::TAU;

    fn unit(x: u64) -> f32 {
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Values with real and imaginary parts uniform in [-1, 1).
    fn random_values(len: usize, seed: u64) -> Vec<Complex> {
        (0..len as u64)
            .map(|i| {
                let a = splitmix64(seed.wrapping_mul(0x1000_0001) ^ (2 * i));
                let b = splitmix64(seed.wrapping_mul(0x1000_0001) ^ (2 * i + 1));
                Complex::new(2.0 * unit(a) - 1.0, 2.0 * unit(b) - 1.0)
            })
            .collect()
    }

    /// The 2D synthesis sum written out, in f64.
    fn naive_inverse(input: &[Complex], n: usize) -> Vec<(f64, f64)> {
        let mut out = vec![(0.0, 0.0); n * n];
        for mz in 0..n {
            for mx in 0..n {
                let (mut re, mut im) = (0.0f64, 0.0f64);
                for kz in 0..n {
                    for kx in 0..n {
                        let a = TAU * ((kx * mx + kz * mz) % n) as f64 / n as f64;
                        let (s, c) = a.sin_cos();
                        let v = input[kz * n + kx];
                        re += v.re as f64 * c - v.im as f64 * s;
                        im += v.re as f64 * s + v.im as f64 * c;
                    }
                }
                out[mz * n + mx] = (re, im);
            }
        }
        out
    }

    fn max_error(fast: &[Complex], reference: &[(f64, f64)]) -> f64 {
        fast.iter()
            .zip(reference)
            .map(|(a, b)| (a.re as f64 - b.0).abs().max((a.im as f64 - b.1).abs()))
            .fold(0.0, f64::max)
    }

    /// Unit inputs give outputs of size about n; f32 round-off over the
    /// log2(n^2) passes leaves errors near 1e-6 n, so 1e-5 n is a tenfold
    /// margin, while a wrong twiddle, sign or permutation errs by about n.
    #[test]
    fn inverse_2d_matches_the_naive_sum() {
        for n in [8usize, 16, 32] {
            let input = random_values(n * n, n as u64);
            let mut data = input.clone();
            let mut scratch = vec![Complex::ZERO; n * n];
            Fft2d::new(n).inverse(&mut data, &mut scratch);
            let err = max_error(&data, &naive_inverse(&input, n));
            assert!(err < 1e-5 * n as f64, "n = {n}: max error {err:e}");
        }
    }

    #[test]
    fn inverse_1d_matches_the_naive_sum_at_small_sizes() {
        for n in [2usize, 4, 8] {
            let input = random_values(n, 100 + n as u64);
            let mut data = input.clone();
            Fft::new(n).inverse(&mut data);
            for (m, got) in data.iter().enumerate() {
                let (mut re, mut im) = (0.0f64, 0.0f64);
                for (k, v) in input.iter().enumerate() {
                    let (s, c) = (TAU * ((k * m) % n) as f64 / n as f64).sin_cos();
                    re += v.re as f64 * c - v.im as f64 * s;
                    im += v.re as f64 * s + v.im as f64 * c;
                }
                assert!((got.re as f64 - re).abs() < 1e-5 && (got.im as f64 - im).abs() < 1e-5, "n = {n}, m = {m}");
            }
        }
    }

    /// Forward then inverse returns n^2 times the input; after scaling back,
    /// round-off is about 1e-6 for unit inputs.
    #[test]
    fn forward_then_inverse_round_trips() {
        for n in [8usize, 16, 32, 64] {
            let input = random_values(n * n, 77 + n as u64);
            let mut data = input.clone();
            let mut scratch = vec![Complex::ZERO; n * n];
            let fft = Fft2d::new(n);
            fft.forward(&mut data, &mut scratch);
            fft.inverse(&mut data, &mut scratch);
            let scale = 1.0 / (n * n) as f32;
            let err = data
                .iter()
                .zip(&input)
                .map(|(a, b)| ((a.re * scale - b.re).abs()).max((a.im * scale - b.im).abs()))
                .fold(0.0f32, f32::max);
            assert!(err < 1e-5, "n = {n}: round trip error {err:e}");
        }
    }

    /// A spectrum with X(-k) = conj(X(k)), whose synthesis is real.
    fn hermitian(n: usize, seed: u64) -> Vec<Complex> {
        let r = random_values(n * n, seed);
        let mut h = vec![Complex::ZERO; n * n];
        for kz in 0..n {
            for kx in 0..n {
                let mirror = ((n - kz) % n) * n + (n - kx) % n;
                h[kz * n + kx] = (r[kz * n + kx] + r[mirror].conj()).scale(0.5);
            }
        }
        h
    }

    /// Two real fields share one complex transform: the synthesis of A + iB
    /// has field a as its real part and field b as its imaginary part, each
    /// equal to its own transform; a single Hermitian spectrum synthesises to
    /// a real field (imaginary part at round-off). Tolerance as in the
    /// naive-sum test.
    #[test]
    fn two_real_fields_share_one_transform() {
        let n = 32;
        let (a, b) = (hermitian(n, 5), hermitian(n, 6));
        let fft = Fft2d::new(n);
        let mut scratch = vec![Complex::ZERO; n * n];
        let mut packed: Vec<Complex> = a.iter().zip(&b).map(|(&x, &y)| x + y.mul_i()).collect();
        fft.inverse(&mut packed, &mut scratch);
        let mut alone_a = a.clone();
        fft.inverse(&mut alone_a, &mut scratch);
        let mut alone_b = b.clone();
        fft.inverse(&mut alone_b, &mut scratch);
        let tol = 1e-5 * n as f32;
        for i in 0..n * n {
            assert!(alone_a[i].im.abs() < tol && alone_b[i].im.abs() < tol, "imaginary residue at {i}");
            assert!((packed[i].re - alone_a[i].re).abs() < tol, "field a at {i}");
            assert!((packed[i].im - alone_b[i].re).abs() < tol, "field b at {i}");
        }
        // The fields are not trivially small.
        assert!(alone_a.iter().map(|v| v.re.abs()).fold(0.0, f32::max) > 1.0);
    }
}

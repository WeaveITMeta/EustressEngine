//! Spectral analysis (Data Platform P4) — FFT magnitude spectrum of a
//! uniformly-sampled numeric column.
//!
//! Requires the `spectral` feature (pulls pure-Rust `rustfft`, no C toolchain).
//! The caller is responsible for uniform sampling: nulls/non-finite values are
//! rejected (interpolate first via [`crate::numerics::interpolate_linear`]).

use crate::numerics::as_f64_opt;
use crate::{ColumnData, DataError, Result};
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

/// Single-sided magnitude spectrum: `freqs[k]` in Hz, `magnitudes[k]` the
/// amplitude of that bin. Length is `n/2 + 1`.
#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    /// Bin centre frequencies, Hz.
    pub freqs: Vec<f64>,
    /// Single-sided amplitudes (interior bins doubled; DC/Nyquist not).
    pub magnitudes: Vec<f64>,
}

/// Compute the single-sided magnitude spectrum of `y`, sampled at
/// `sample_rate_hz`. Errors on nulls/non-finite samples, a non-positive sample
/// rate, or fewer than two samples.
pub fn magnitude_spectrum(y: &ColumnData, sample_rate_hz: f64) -> Result<Spectrum> {
    if !(sample_rate_hz > 0.0) {
        return Err(DataError::Schema("magnitude_spectrum: sample_rate_hz must be > 0".into()));
    }
    let raw = as_f64_opt(y)?;
    let mut signal = Vec::with_capacity(raw.len());
    for (i, v) in raw.iter().enumerate() {
        match v {
            Some(x) if x.is_finite() => signal.push(*x),
            _ => {
                return Err(DataError::Schema(format!(
                    "magnitude_spectrum: null/non-finite sample at row {i}; interpolate first"
                )))
            }
        }
    }
    let n = signal.len();
    if n < 2 {
        return Err(DataError::Schema("magnitude_spectrum needs >= 2 samples".into()));
    }

    let mut buf: Vec<Complex<f64>> = signal.iter().map(|&v| Complex::new(v, 0.0)).collect();
    let mut planner = FftPlanner::<f64>::new();
    planner.plan_fft_forward(n).process(&mut buf);

    let half = n / 2;
    let mut freqs = Vec::with_capacity(half + 1);
    let mut magnitudes = Vec::with_capacity(half + 1);
    let nf = n as f64;
    for (k, bin) in buf.iter().take(half + 1).enumerate() {
        freqs.push(k as f64 * sample_rate_hz / nf);
        // Single-sided amplitude: normalize by n; double the interior bins
        // (energy folded from the mirror), but not DC or, for even n, Nyquist.
        let scale = if k == 0 || (n % 2 == 0 && k == half) { 1.0 } else { 2.0 };
        magnitudes.push(bin.norm() / nf * scale);
    }
    Ok(Spectrum { freqs, magnitudes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_peaks_at_its_frequency_with_unit_amplitude() {
        // 10 Hz sine, 100 Hz sample rate, exactly 10 cycles in 100 samples →
        // no spectral leakage: a clean unit-amplitude peak at bin 10 (= 10 Hz).
        let sr = 100.0;
        let f = 10.0;
        let n = 100usize;
        let sig: Vec<Option<f64>> = (0..n)
            .map(|i| Some((2.0 * std::f64::consts::PI * f * i as f64 / sr).sin()))
            .collect();
        let spec = magnitude_spectrum(&ColumnData::F64(sig), sr).unwrap();

        let (peak_k, peak_mag) = spec
            .magnitudes
            .iter()
            .enumerate()
            .fold((0usize, 0.0f64), |(bk, bm), (k, &m)| if m > bm { (k, m) } else { (bk, bm) });
        assert_eq!(spec.freqs[peak_k], 10.0, "peak should be at 10 Hz");
        assert!((peak_mag - 1.0).abs() < 1e-6, "amplitude ~1.0, got {peak_mag}");
        // DC bin ~0 for a zero-mean sine.
        assert!(spec.magnitudes[0].abs() < 1e-6);
        assert_eq!(spec.freqs.len(), n / 2 + 1);
    }

    #[test]
    fn rejects_nulls() {
        let col = ColumnData::F64(vec![Some(1.0), None, Some(2.0)]);
        assert!(magnitude_spectrum(&col, 10.0).is_err());
    }
}

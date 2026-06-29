//! Pure-Rust CPU reference implementation of the PPISP transforms.
//!
//! This is the correctness oracle (f64) that mirrors `tests/torch_reference.py`
//! from the upstream repo. The production path will be f32 WGSL compute shaders
//! validated against this reference at the tolerances in
//! `PPISP_RUST_PORT_PROPOSAL.md` §6.

/// Per-frame **exposure** transform — the first of PPISP's four operations.
///
/// Forward: `rgb_out = rgb_in * 2^ev`, where `ev` is the per-frame exposure in
/// log2 / EV stops (identity at `ev = 0`). No clamp. Gated off when a frame is
/// not selected (handled by the caller).
pub mod exposure {
    use std::f64::consts::LN_2;

    /// Forward pass. `rgb` is a flat `[r,g,b, r,g,b, ...]` buffer; `ev` is the
    /// per-frame exposure in stops. Returns the corrected buffer.
    pub fn forward(rgb: &[f64], ev: f64) -> Vec<f64> {
        let gain = ev.exp2();
        rgb.iter().map(|c| c * gain).collect()
    }

    /// In-place forward pass.
    pub fn forward_in_place(rgb: &mut [f64], ev: f64) {
        let gain = ev.exp2();
        for c in rgb.iter_mut() {
            *c *= gain;
        }
    }

    /// Reverse-mode (adjoint) pass — the closed-form backward from
    /// `PPISP_RUST_PORT_PROPOSAL.md` §4. Given the upstream gradient `grad_out`
    /// (dL/d rgb_out) and the forward output `rgb_out`, returns
    /// `(grad_rgb_in, grad_ev)`:
    ///
    /// - `grad_rgb_in[i] = grad_out[i] * gain`   (since `d out_i / d in_i = gain`)
    /// - `grad_ev = ln2 * Σ_i grad_out[i] * rgb_out[i]`
    ///   (since `d out_i / d ev = out_i * ln2`)
    pub fn backward(rgb_out: &[f64], grad_out: &[f64], ev: f64) -> (Vec<f64>, f64) {
        debug_assert_eq!(rgb_out.len(), grad_out.len());
        let gain = ev.exp2();
        let grad_rgb_in = grad_out.iter().map(|g| g * gain).collect();
        let grad_ev = LN_2
            * grad_out
                .iter()
                .zip(rgb_out.iter())
                .map(|(g, o)| g * o)
                .sum::<f64>();
        (grad_rgb_in, grad_ev)
    }
}

// TODO(PPISP port §3.3): `pub mod vignetting` — per-camera, per-channel even
// radial polynomial about an optical center; pixel coords normalized by max(W,H).
// TODO(PPISP port §3.3): `pub mod color` — per-frame RGI chromaticity homography
// built from 8 latents via the ZCA blocks + nullspace selection (MUST CONFIRM
// the threshold-fallback kernel form, proposal Appendix A).
// TODO(PPISP port §3.3): `pub mod crf` — per-camera, per-channel piecewise
// power-law tone curve with the intentional `a,b`-only backward truncation.

#[cfg(test)]
mod tests {
    use super::exposure;
    use std::f64::consts::LN_2;

    #[test]
    fn exposure_identity_at_zero() {
        let rgb = vec![0.2, 0.5, 0.8, 1.0, 0.0, 0.3];
        let out = exposure::forward(&rgb, 0.0);
        for (a, b) in rgb.iter().zip(out.iter()) {
            assert!((a - b).abs() < 1e-15, "ev=0 must be identity");
        }
    }

    #[test]
    fn exposure_one_stop_doubles() {
        let rgb = vec![0.1, 0.25, 0.4];
        let out = exposure::forward(&rgb, 1.0);
        for (a, b) in rgb.iter().zip(out.iter()) {
            assert!((b - 2.0 * a).abs() < 1e-12, "+1 stop must double");
        }
    }

    /// Finite-difference gradient check of the closed-form backward against
    /// central differences in f64 (the proposal's validation strategy, §6).
    #[test]
    fn exposure_backward_matches_finite_difference() {
        let rgb_in = vec![0.2, 0.5, 0.8, 0.9, 0.1, 0.33];
        let ev = 0.4_f64;

        // Loss L = 0.5 * Σ out_i^2  ⇒  dL/dout_i = out_i.
        let loss = |rgb_in: &[f64], ev: f64| -> f64 {
            exposure::forward(rgb_in, ev).iter().map(|o| 0.5 * o * o).sum()
        };

        let rgb_out = exposure::forward(&rgb_in, ev);
        let grad_out = rgb_out.clone(); // dL/dout_i = out_i
        let (grad_in, grad_ev) = exposure::backward(&rgb_out, &grad_out, ev);

        let eps = 1e-6;

        // d L / d ev
        let g_ev_fd = (loss(&rgb_in, ev + eps) - loss(&rgb_in, ev - eps)) / (2.0 * eps);
        assert!(
            (grad_ev - g_ev_fd).abs() <= 1e-6 * (1.0 + g_ev_fd.abs()),
            "grad_ev analytic {grad_ev} vs fd {g_ev_fd}"
        );

        // d L / d rgb_in[i]
        for i in 0..rgb_in.len() {
            let mut up = rgb_in.clone();
            let mut dn = rgb_in.clone();
            up[i] += eps;
            dn[i] -= eps;
            let g_fd = (loss(&up, ev) - loss(&dn, ev)) / (2.0 * eps);
            assert!(
                (grad_in[i] - g_fd).abs() <= 1e-6 * (1.0 + g_fd.abs()),
                "grad_in[{i}] analytic {} vs fd {g_fd}",
                grad_in[i]
            );
        }

        // Sanity: closed-form grad_ev == ln2 * Σ out^2.
        let expect = LN_2 * rgb_out.iter().map(|o| o * o).sum::<f64>();
        assert!((grad_ev - expect).abs() < 1e-12);
    }
}

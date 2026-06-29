//! Physically-Plausible ISP (PPISP) — a differentiable, post-render photometric
//! correction module for radiance-field reconstruction (a pure-Rust port of
//! NVIDIA's `nv-tlabs/ppisp`, Apache-2.0).
//!
//! PPISP applies, in a fixed order, the four operations a real camera applies
//! between sensor irradiance and a stored pixel:
//!
//! ```text
//! exposure (per-frame) → vignetting (per-camera) → color (per-frame) → CRF (per-camera)
//! ```
//!
//! It is **not** a renderer or a splatting method; it is the photometric layer
//! that makes real multi-camera capture usable for 3D reconstruction, and the
//! front-end that makes relightable Gaussian Splatting physically grounded (it
//! disentangles capture-time exposure/white-balance/vignette/tone-curve from
//! true scene radiance, so an inverse-render decomposition into albedo + BRDF +
//! light is trustworthy).
//!
//! This module is the **CPU reference oracle** (see
//! `docs/architecture/PPISP_RUST_PORT_PROPOSAL.md` §5.a). The WGSL GPU path, the
//! burn controller MLP, and the optimizer land on top of it. Only the
//! **exposure** transform is implemented so far; vignetting/color/CRF are
//! scaffolded with their spec references.

pub mod reference;

pub use reference::exposure;

/// PPISP configuration (subset; mirrors the PyTorch `PPISPConfig`). Expanded as
/// the port progresses — see the proposal for the full field list and defaults.
#[derive(Debug, Clone)]
pub struct PpispConfig {
    /// Enable the per-frame exposure transform.
    pub enable_exposure: bool,
    /// Enable the per-camera vignetting transform (not yet implemented).
    pub enable_vignetting: bool,
    /// Enable the per-frame color homography (not yet implemented).
    pub enable_color: bool,
    /// Enable the per-camera CRF tone curve (not yet implemented).
    pub enable_crf: bool,
}

impl Default for PpispConfig {
    fn default() -> Self {
        Self {
            enable_exposure: true,
            enable_vignetting: true,
            enable_color: true,
            enable_crf: true,
        }
    }
}

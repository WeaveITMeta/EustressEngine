//! R1 — photoreal post-processing stack registration + settings.
//!
//! **Status:** the heavy post-stack (GTAO, TAA, Bloom, AutoExposure) is ON HOLD.
//! TAA/SMAA/FXAA live in bevy's `bevy_anti_alias` crate and Bloom/AutoExposure
//! in `bevy_post_process`; bevy 0.18.1 only published those two crates as
//! `0.18.0-rc.1` (no stable `0.18.x`), so enabling their cargo features breaks
//! crates.io dependency resolution for the whole workspace. R1 currently ships
//! only the parts available in always-published crates — filmic tonemapping
//! (`TonyMcMapface`, in `studio_camera_bundle`). GTAO (`bevy_pbr`,
//! `ScreenSpaceAmbientOcclusion`) IS available but requires `Msaa::Off`, which
//! without TAA/FXAA means aliased edges — held pending a decision.
//!
//! When those crates publish stable (or via a bevy git `[patch]`), the post
//! components go into `studio_camera_bundle` (so the editor + off-screen AI
//! camera stay in lockstep against `SharedLightingPlugin`'s shared mesh-view
//! bind-group layout) and `AutoExposurePlugin` gets registered here (Bevy's
//! `DefaultPlugins`/`PostProcessPlugin` does NOT include auto-exposure).
//!
//! [`PhotorealSettings`] is seeded now as the runtime escape hatch for that
//! future stack. NOTE the bind-group hazard: a future settings→components sync
//! system must keep `Msaa::Off` + `Hdr` permanent (toggling them at runtime
//! changes the view-bind-group shape → shared-layout panic) and apply changes
//! identically to BOTH cameras.

use bevy::prelude::*;

/// Runtime on/off flags for the (pending) photoreal post-stack. Seeded now so
/// the future stack has a settings surface; see module docs for the bind-group
/// hazard around toggling `Msaa`/`Hdr`.
#[derive(Resource, Clone, Copy, Debug)]
pub struct PhotorealSettings {
    /// Master switch for the whole stack.
    pub master: bool,
    /// Ground-contact ambient occlusion (GTAO).
    pub gtao: bool,
    /// Temporal anti-aliasing.
    pub taa: bool,
    /// Filmic bloom.
    pub bloom: bool,
    /// Auto-exposure adaptation.
    pub auto_exposure: bool,
}

impl Default for PhotorealSettings {
    fn default() -> Self {
        Self {
            master: true,
            gtao: true,
            taa: true,
            bloom: true,
            auto_exposure: true,
        }
    }
}

/// Seeds [`PhotorealSettings`]. Will also register `AutoExposurePlugin` once the
/// `bevy_post_process` crate is reachable (see module docs).
pub struct PhotorealPlugin;

impl Plugin for PhotorealPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PhotorealSettings>();
        // TODO(R1-followup): when bevy_anti_alias / bevy_post_process publish
        // stable (or via a git [patch]), add the post components to
        // studio_camera_bundle and `app.add_plugins(AutoExposurePlugin)` here.
    }
}

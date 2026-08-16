//! R1 — photoreal post-processing stack: the one place every camera image
//! stage is attached, and the one place each is switched on or off.
//!
//! [`crate::default_scene::studio_camera_bundle`] contributes only the pieces
//! that are always identical (`Camera3d`, filmic tonemapping, `DepthPrepass`,
//! `Msaa::Off`, the [`StudioCamera`] marker). Everything optional is attached
//! here by [`apply_camera_stages`], so a stage can be flipped without editing
//! two spawn sites and without the two cameras ever disagreeing.
//!
//! ## Why these are startup switches, not runtime toggles
//!
//! `SharedLightingPlugin` builds ONE `mesh_view_bind_group` layout for every
//! `Camera3d`. A camera whose view features differ (MSAA level, prepasses)
//! produces a bind group of a different shape against that shared layout, and
//! wgpu aborts with "N bindings != M bindings". So every knob is read **once**
//! per process via `OnceLock` and applied to **all** [`StudioCamera`]s in a
//! single system — a camera spawned later in the session cannot disagree with
//! one spawned at startup.
//!
//! ## Measured: the stack is not free
//!
//! On Space1 with no splat cloud resident, `Msaa::Off` + SMAA + bloom moved
//! `06_render+present` from 27.3 to 31.1 ms/frame. The 4×-MSAA saving scales
//! with overdraw while the post passes cost a fixed amount per frame, so the
//! stack wins on heavy or alpha-blended scenes and *loses* on sparse ones.
//! That is exactly why these are switches: the right answer is per-scene, and
//! it is measurable in one run instead of one rebuild.
//!
//! | env | default | effect |
//! |-----|---------|--------|
//! | `EUSTRESS_SMAA`          | on  | subpixel-morphological AA (3 passes) |
//! | `EUSTRESS_BLOOM`         | on  | filmic bloom (down/upsample chain) |
//! | `EUSTRESS_MSAA`          | off | `2`/`4`/`8` restores hardware MSAA |
//! | `EUSTRESS_GTAO`          | off | ground-contact AO; adds a `NormalPrepass` |
//! | `EUSTRESS_AUTO_EXPOSURE` | off | exposure adaptation |
//!
//! Set any flag to `0`/`false` to disable, or a non-empty value to enable.
//!
//! **`EUSTRESS_MSAA` is the one to be careful with:** hardware MSAA and SMAA
//! both anti-alias, so enabling MSAA while SMAA is on pays twice. It exists to
//! A/B the trade, not to stack.

use bevy::prelude::*;
use bevy::anti_alias::smaa::Smaa;
use bevy::pbr::ScreenSpaceAmbientOcclusion;
use bevy::post_process::auto_exposure::{AutoExposure, AutoExposurePlugin};
use bevy::post_process::bloom::Bloom;
use bevy::render::view::Msaa;
use std::sync::OnceLock;

use crate::default_scene::StudioCamera;

/// Read an on/off env flag exactly once. `default_on` is used when unset.
///
/// Reading once is not a micro-optimisation — see the module docs: a knob that
/// could change mid-session could desynchronise two cameras' bind groups.
fn flag(key: &'static str, default_on: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !(v.is_empty() || v == "0" || v == "false" || v == "off")
        }
        Err(_) => default_on,
    }
}

fn smaa_on() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| flag("EUSTRESS_SMAA", true))
}

fn bloom_on() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| flag("EUSTRESS_BLOOM", true))
}

fn gtao_on() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| flag("EUSTRESS_GTAO", false))
}

fn auto_exposure_on() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| flag("EUSTRESS_AUTO_EXPOSURE", false))
}

/// Hardware MSAA sample count, or `None` to leave the bundle's `Msaa::Off`.
/// Only 2/4/8 are valid; anything else is ignored with a warning rather than
/// panicking (`Msaa::from_samples` panics on an unsupported count).
fn msaa_override() -> Option<Msaa> {
    static V: OnceLock<Option<Msaa>> = OnceLock::new();
    *V.get_or_init(|| match std::env::var("EUSTRESS_MSAA").ok()?.trim() {
        "2" => Some(Msaa::Sample2),
        "4" => Some(Msaa::Sample4),
        "8" => Some(Msaa::Sample8),
        "" | "0" | "off" | "false" => None,
        other => {
            warn!("photoreal: EUSTRESS_MSAA={other:?} is not 2/4/8 — ignoring, staying at Msaa::Off");
            None
        }
    })
}

/// What the photoreal stack resolved to this run. Reporting only — see the
/// module docs on why these are not live toggles.
#[derive(Resource, Clone, Copy, Debug)]
pub struct PhotorealSettings {
    pub smaa: bool,
    pub bloom: bool,
    pub gtao: bool,
    pub auto_exposure: bool,
    /// `None` = `Msaa::Off` (the default); `Some(n)` = hardware MSAA at n samples.
    pub msaa_samples: Option<u32>,
}

impl Default for PhotorealSettings {
    fn default() -> Self {
        Self {
            smaa: smaa_on(),
            bloom: bloom_on(),
            gtao: gtao_on(),
            auto_exposure: auto_exposure_on(),
            msaa_samples: msaa_override().map(|m| m.samples()),
        }
    }
}

pub struct PhotorealPlugin;

impl Plugin for PhotorealPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PhotorealSettings>();

        // `PostProcessPlugin` (in DefaultPlugins) brings bloom / motion-blur /
        // DoF / effect-stack / MSAA-writeback — but NOT auto-exposure. Register
        // it only when asked for, so the everyday build never pays its
        // per-frame histogram cost.
        if auto_exposure_on() {
            app.add_plugins(AutoExposurePlugin);
        }

        app.add_systems(Update, apply_camera_stages);

        let s = PhotorealSettings::default();
        info!(
            "photoreal: smaa={} bloom={} gtao={} auto_exposure={} msaa={}",
            s.smaa,
            s.bloom,
            s.gtao,
            s.auto_exposure,
            s.msaa_samples.map_or("off".to_string(), |n| n.to_string()),
        );
    }
}

/// Attach every enabled stage to freshly-spawned Studio cameras.
///
/// `Added<StudioCamera>` keeps this to the frame a camera appears. Uniform
/// across all Studio cameras by construction — see the module docs on the
/// shared-layout hazard. `ScreenSpaceAmbientOcclusion` pulls its own
/// `NormalPrepass` in via its `#[require(..)]`.
fn apply_camera_stages(mut commands: Commands, cameras: Query<Entity, Added<StudioCamera>>) {
    for entity in &cameras {
        let mut ec = commands.entity(entity);
        if smaa_on() {
            // NOTE: SMAA needs bevy's `smaa_luts` feature. Without it
            // `bevy_anti_alias` compiles a `lut_placeholder()` whose texture
            // view is D3 where the blend-weight bind group declares D2, and
            // bevy_render QUITS the app on that validation error the first
            // frame SMAA runs. The feature is enabled in Cargo.toml; keep it.
            ec.insert(Smaa::default());
        }
        if bloom_on() {
            ec.insert(Bloom::NATURAL);
        }
        if let Some(msaa) = msaa_override() {
            ec.insert(msaa);
        }
        if gtao_on() {
            ec.insert(ScreenSpaceAmbientOcclusion::default());
        }
        if auto_exposure_on() {
            ec.insert(AutoExposure::default());
        }
    }
}

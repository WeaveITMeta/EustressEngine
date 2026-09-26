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
//! | `EUSTRESS_BLOOM_INTENSITY` | 0.15 | bloom strength (bevy's `NATURAL`) |
//! | `EUSTRESS_MSAA`          | off | `2`/`4`/`8` restores hardware MSAA |
//! | `EUSTRESS_GTAO`          | off | ground-contact AO; adds a `NormalPrepass` |
//! | `EUSTRESS_AUTO_EXPOSURE` | off | histogram exposure adaptation (the sky's own night adaptation is always on; see `sky_atmosphere::SkyExposure`) |
//! | `EUSTRESS_CONTACT_SHADOWS` | off | screen-space contact shadows from the sun |
//!
//! Set any flag to `0`/`false` to disable, or a non-empty value to enable.
//!
//! **Contact shadows are a view-layout switch like MSAA.** `ContactShadows`
//! is part of the mesh view bind-group layout key, which is why it is read
//! once here and applied to every Studio camera together, never toggled on
//! one camera at runtime.
//!
//! **`EUSTRESS_MSAA` is the one to be careful with:** hardware MSAA and SMAA
//! both anti-alias, so enabling MSAA while SMAA is on pays twice. It exists to
//! A/B the trade, not to stack.

use bevy::prelude::*;
use bevy::anti_alias::smaa::Smaa;
use bevy::pbr::{ContactShadows, ScreenSpaceAmbientOcclusion};
use bevy::post_process::auto_exposure::{AutoExposure, AutoExposurePlugin};
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use eustress_common::classes::BloomEffect;
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

/// Bloom's strength, `EUSTRESS_BLOOM_INTENSITY` (default: bevy's `NATURAL`,
/// 0.15). Energy-conserving, so raising it spreads more of every bright
/// pixel into its glow rather than brightening the frame. The sun's disc is
/// capped for the HDR target (see `lighting_plugin::SUN_DISC_PEAK`), so this
/// is what sets how far its glare reaches.
fn bloom() -> Bloom {
    static V: OnceLock<f32> = OnceLock::new();
    let natural = Bloom::NATURAL;
    let intensity = *V.get_or_init(|| {
        std::env::var("EUSTRESS_BLOOM_INTENSITY")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(natural.intensity)
    });
    Bloom { intensity, ..natural }
}

/// The bloom an authored Bloom object (Roblox's `BloomEffect`) asks for.
///
/// Roblox's `Size` (0-56, default 24) is how far the glow reaches, so it
/// scales bevy's wide, low-frequency part. A `Threshold` above 0 means only
/// what is brighter than it glows, which is bevy's additive, thresholded
/// bloom (its `OLD_SCHOOL` look); a lit white surface here sits near 1, the
/// sky below it, the sun, lamps at night and silver-lined clouds far above.
/// With no threshold it stays energy-conserving, `Intensity` scaling the
/// baseline (Roblox's default 0.4 is the baseline itself).
fn bloom_from_effect(effect: &BloomEffect) -> Bloom {
    let base = bloom();
    let reach = (effect.size.max(0.0) / 24.0).min(2.3);
    let low_frequency_boost = (base.low_frequency_boost * reach).clamp(0.0, 1.0);
    if effect.threshold > 0.0 {
        Bloom {
            intensity: (0.1 * effect.intensity.max(0.0)).min(1.0),
            low_frequency_boost,
            prefilter: BloomPrefilter { threshold: effect.threshold, threshold_softness: 0.3 },
            composite_mode: BloomCompositeMode::Additive,
            ..base
        }
    } else {
        Bloom {
            intensity: (base.intensity * effect.intensity.max(0.0) / 0.4).min(1.0),
            low_frequency_boost,
            ..base
        }
    }
}

/// Carry an enabled Bloom object's settings to every Studio camera, and
/// restore the baseline when it is disabled or deleted.
fn apply_bloom_effect(
    effects: Query<&BloomEffect>,
    changed: Query<(), Changed<BloomEffect>>,
    mut removed: RemovedComponents<BloomEffect>,
    mut cameras: Query<&mut Bloom, With<StudioCamera>>,
) {
    let removed_any = removed.read().count() > 0;
    // A camera that just got its Bloom takes the effect too. Asked through
    // the one mutable query: a second query filtering on `Added<Bloom>` reads
    // Bloom while this one writes it, which Bevy rejects at startup (B0001).
    // `is_added` on `Mut` does not mark the component changed.
    let new_bloom = cameras.iter_mut().any(|b| b.is_added());
    if changed.is_empty() && !removed_any && !new_bloom {
        return;
    }
    let desired = effects
        .iter()
        .find(|e| e.enabled)
        .map(bloom_from_effect)
        .unwrap_or_else(bloom);
    for mut camera_bloom in cameras.iter_mut() {
        *camera_bloom = desired.clone();
    }
}

fn gtao_on() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| flag("EUSTRESS_GTAO", false))
}

fn auto_exposure_on() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| flag("EUSTRESS_AUTO_EXPOSURE", false))
}

/// Screen-space contact shadows: the fine shadow where a part meets the
/// ground, below what the sun's shadow maps resolve. Read by the sun's
/// hydration too, which must set `contact_shadows_enabled` on the light for
/// the camera component to have anything to draw.
pub fn contact_shadows_on() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| flag("EUSTRESS_CONTACT_SHADOWS", false))
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
    pub contact_shadows: bool,
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
            contact_shadows: contact_shadows_on(),
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
        if bloom_on() {
            app.add_systems(Update, apply_bloom_effect.after(apply_camera_stages));
        }

        let s = PhotorealSettings::default();
        info!(
            "photoreal: smaa={} bloom={} gtao={} auto_exposure={} contact_shadows={} msaa={}",
            s.smaa,
            s.bloom,
            s.gtao,
            s.auto_exposure,
            s.contact_shadows,
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
            ec.insert(bloom());
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
        if contact_shadows_on() {
            // Requires the `DepthPrepass` every Studio camera already has.
            ec.insert(ContactShadows::default());
        }
    }
}

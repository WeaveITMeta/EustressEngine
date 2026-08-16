//! # Reflections
//!
//! Local reflection probes and screen-space reflections: the two things that
//! reflect *the scene* rather than the sky.
//!
//! [`crate::plugins::sky_atmosphere`] gives every surface a correctly filtered
//! reflection of the sky, which is the right answer outdoors and the wrong one
//! the moment a camera walks indoors, where a room should reflect the room.
//! This module covers that gap from both ends:
//!
//! - **Reflection probes** ([`LightProbe`]) bound a volume and hand everything
//!   inside it a specific environment map instead of the scene-wide one. Static,
//!   cheap, and correct for anything off screen.
//! - **Screen-space reflections** ([`ScreenSpaceReflections`]) march the depth
//!   buffer, so they reflect whatever is actually on screen, including moving
//!   geometry, and lose it the moment it leaves frame.
//!
//! They compose: SSR handles what is visible, probes catch what is not.
//!
//! ## Why SSR is off by default
//!
//! Bevy only implements SSR in the deferred path, so switching it on flips
//! [`DefaultOpaqueRendererMethod`] to deferred for the whole app and adds a
//! `DeferredPrepass` to every camera. That is not a per-camera decision: as
//! `photoreal.rs` documents, all `Camera3d`s share one `mesh_view_bind_group`
//! layout, and a camera whose view features differ produces a bind group of a
//! different shape against that shared layout, which makes wgpu abort. So this
//! is resolved **once** per process from the environment, exactly like the rest
//! of the photoreal stack, and applied to every camera uniformly.
//!
//! Deferred also drops forward-only material features (notably transmission),
//! which is a real trade rather than a free upgrade. Hence opt-in.
//!
//! | env | default | effect |
//! |-----|---------|--------|
//! | `EUSTRESS_SSR` | off | screen-space reflections, forces deferred rendering |
//! | `EUSTRESS_SSR_QUALITY` | `medium` | `low` / `medium` / `high` march budget |

use bevy::prelude::*;
use bevy::core_pipeline::prepass::DeferredPrepass;
use bevy::light::{EnvironmentMapLight, GeneratedEnvironmentMapLight, LightProbe};
use bevy::pbr::{DefaultOpaqueRendererMethod, ScreenSpaceReflections};
// This crate's bevy prelude does not re-export the log macros.
use tracing::{info, warn};
use std::sync::OnceLock;

use crate::attributes::{AttributeValue, Attributes};
use crate::plugins::sky_atmosphere::{NoAtmosphere, SkyCamera};

// ============================================================================
// Plugin
// ============================================================================

pub struct ReflectionsPlugin;

impl Plugin for ReflectionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ReflectionSettings>()
            .register_type::<ReflectionProbe>();

        if ssr_enabled() {
            // Whole-app switch, not per camera. See the module docs.
            app.insert_resource(DefaultOpaqueRendererMethod::deferred());
        }

        app.add_systems(
            Update,
            (
                attach_ssr_to_cameras,
                // Seed before hydrating, so the first hydration already sees the
                // authored values rather than defaults.
                seed_probes_from_attributes,
                hydrate_reflection_probes.after(seed_probes_from_attributes),
            ),
        );

        let s = ReflectionSettings::default();
        info!(
            "reflections: ssr={} quality={:?}",
            s.ssr, s.quality
        );
    }
}

// ============================================================================
// Settings
// ============================================================================

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SsrQuality {
    /// 8 linear steps. Cheap; misses thin geometry.
    Low,
    /// 16 linear steps with secant refinement. The default.
    #[default]
    Medium,
    /// 32 linear steps, more bisection. Cinematic captures.
    High,
}

/// What the reflection stack resolved to this run. Reporting only: these are
/// startup switches for the bind-group reason in the module docs.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ReflectionSettings {
    pub ssr: bool,
    pub quality: SsrQuality,
}

impl Default for ReflectionSettings {
    fn default() -> Self {
        Self { ssr: ssr_enabled(), quality: ssr_quality() }
    }
}

fn ssr_enabled() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| match std::env::var("EUSTRESS_SSR") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !(v.is_empty() || v == "0" || v == "false" || v == "off")
        }
        Err(_) => false,
    })
}

fn ssr_quality() -> SsrQuality {
    static V: OnceLock<SsrQuality> = OnceLock::new();
    *V.get_or_init(|| {
        match std::env::var("EUSTRESS_SSR_QUALITY")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "low" => SsrQuality::Low,
            "" | "medium" | "med" => SsrQuality::Medium,
            "high" | "ultra" => SsrQuality::High,
            other => {
                warn!("reflections: EUSTRESS_SSR_QUALITY={other:?} is not low/medium/high — using medium");
                SsrQuality::Medium
            }
        }
    })
}

impl SsrQuality {
    fn to_component(self) -> ScreenSpaceReflections {
        let base = ScreenSpaceReflections::default();
        match self {
            SsrQuality::Low => ScreenSpaceReflections {
                linear_steps: 8,
                bisection_steps: 2,
                use_secant: false,
                // Rough surfaces are where SSR noise is worst and where the sky
                // probe is closest to right, so the cheap tier hands them over.
                max_perceptual_roughness: 0.2..0.4,
                ..base
            },
            SsrQuality::Medium => base,
            SsrQuality::High => ScreenSpaceReflections {
                linear_steps: 32,
                bisection_steps: 6,
                use_secant: true,
                max_perceptual_roughness: 0.5..0.8,
                ..base
            },
        }
    }
}

// ============================================================================
// Screen-space reflections
// ============================================================================

/// Attach SSR to every managed camera, uniformly.
///
/// `SkyCamera` is the marker the sky plugin puts on cameras it owns, which is
/// exactly the set that must keep matching view features. Overlay and off-screen
/// cameras carry [`NoAtmosphere`] and are skipped.
fn attach_ssr_to_cameras(
    mut commands: Commands,
    settings: Res<ReflectionSettings>,
    cameras: Query<
        Entity,
        (
            With<Camera3d>,
            With<SkyCamera>,
            Without<ScreenSpaceReflections>,
            Without<NoAtmosphere>,
        ),
    >,
) {
    if !settings.ssr {
        return;
    }
    for camera in cameras.iter() {
        // `ScreenSpaceReflections` requires DepthPrepass and DeferredPrepass.
        // DepthPrepass is already in the Studio bundle; DeferredPrepass is
        // pulled in by the require, but inserting it explicitly keeps the
        // camera's component set readable at the spawn site.
        commands
            .entity(camera)
            .insert((settings.quality.to_component(), DeferredPrepass));
        info!("🪞 Screen-space reflections attached to camera {camera:?}");
    }
}

// ============================================================================
// Reflection probes
// ============================================================================

/// A bounded volume that overrides the scene-wide environment map.
///
/// The probe's [`Transform`] scale is its extent: bevy treats a light probe as a
/// 1x1x1 cube in local space, so a `Transform` scaled to `(20, 5, 12)` covers a
/// room of those dimensions centred on the probe.
///
/// With `texture` empty the probe reflects the sky, which is only useful for
/// carving out a region with a different intensity. Point it at a cubemap to
/// reflect an actual interior.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct ReflectionProbe {
    /// Cubemap asset path. Empty means "inherit the scene environment".
    pub texture: String,
    /// Scale factor on the reflected light, in cd/m^2.
    pub intensity: f32,
    /// Per-axis proportion of the volume over which influence fades out, so
    /// probes can overlap without a visible seam. `0.25` means the inner
    /// 0.75x0.75x0.75 of the box is at full strength.
    pub falloff: f32,
    /// World-space rotation applied to the cubemap, in degrees about Y. Lets an
    /// author line a captured interior up with the room it belongs to.
    pub rotation_degrees: f32,
}

impl Default for ReflectionProbe {
    fn default() -> Self {
        Self {
            texture: String::new(),
            intensity: 1000.0,
            falloff: 0.25,
            rotation_degrees: 0.0,
        }
    }
}

/// Marks a probe whose bevy components are already attached.
#[derive(Component, Debug, Clone, Copy)]
pub struct ReflectionProbeHydrated;

/// Seed a freshly-spawned probe from its authored `[attributes]` table.
///
/// Probes persist through the generic instance machinery, which round-trips
/// `[attributes]`. Reading them here is what stops an authored probe from being
/// silently ignored, which is the exact failure this whole module set out to
/// fix elsewhere: a value the author writes that nothing downstream reads.
///
/// Runs on `Added` only. After the first frame the component is authoritative,
/// so a Properties-panel edit is not fought by a stale attribute.
fn seed_probes_from_attributes(
    mut probes: Query<(&mut ReflectionProbe, &Attributes), Added<ReflectionProbe>>,
) {
    for (mut probe, attrs) in probes.iter_mut() {
        // Accept both the PascalCase property spelling and the snake_case field
        // name, because both show up in authored files.
        let num = |a: &Attributes, keys: [&str; 2]| -> Option<f32> {
            keys.iter().find_map(|k| match a.get(k) {
                Some(AttributeValue::Number(v)) => Some(*v as f32),
                Some(AttributeValue::Int(v)) => Some(*v as f32),
                _ => None,
            })
        };
        let text = |a: &Attributes, keys: [&str; 2]| -> Option<String> {
            keys.iter().find_map(|k| match a.get(k) {
                Some(AttributeValue::String(v)) if !v.is_empty() => Some(v.clone()),
                _ => None,
            })
        };

        if let Some(v) = text(attrs, ["Texture", "texture"]) {
            probe.texture = v;
        }
        if let Some(v) = num(attrs, ["Intensity", "intensity"]) {
            probe.intensity = v;
        }
        if let Some(v) = num(attrs, ["Falloff", "falloff"]) {
            probe.falloff = v;
        }
        if let Some(v) = num(attrs, ["RotationDegrees", "rotation_degrees"]) {
            probe.rotation_degrees = v;
        }
    }
}

/// Turn authored [`ReflectionProbe`]s into bevy light probes.
///
/// Re-runs on edit: `Changed<ReflectionProbe>` catches Properties-panel changes,
/// so retargeting a probe's cubemap or nudging its intensity takes effect
/// without a reload.
fn hydrate_reflection_probes(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    probes: Query<
        (Entity, &ReflectionProbe),
        Or<(Changed<ReflectionProbe>, Without<ReflectionProbeHydrated>)>,
    >,
) {
    for (entity, probe) in probes.iter() {
        let rotation = Quat::from_rotation_y(probe.rotation_degrees.to_radians());
        let mut ec = commands.entity(entity);

        ec.insert((
            LightProbe { falloff: Vec3::splat(probe.falloff.clamp(0.0, 0.5)) },
            ReflectionProbeHydrated,
        ));

        if probe.texture.is_empty() {
            // No cubemap: drop any stale map so the probe cleanly inherits the
            // scene environment rather than keeping a previous texture alive.
            ec.remove::<GeneratedEnvironmentMapLight>();
            ec.remove::<EnvironmentMapLight>();
        } else {
            // Route through bevy's GPU filtering chain rather than binding the
            // cubemap raw. A raw cubemap has one mip, so `mip = roughness *
            // (mip_count - 1)` is always 0 and every surface reflects it
            // mirror-sharp. That bug is the reason this module exists.
            ec.insert(GeneratedEnvironmentMapLight {
                environment_map: asset_server.load(probe.texture.clone()),
                intensity: probe.intensity.max(0.0),
                rotation,
                affects_lightmapped_mesh_diffuse: false,
            });
        }

        info!(
            "🪞 Reflection probe {entity:?} hydrated (texture {:?}, intensity {})",
            if probe.texture.is_empty() { "<scene>" } else { probe.texture.as_str() },
            probe.intensity
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_falloff_stays_in_bevy_domain() {
        // LightProbe::falloff is a ratio of the box extent; values at or past
        // 0.5 collapse the full-strength interior to nothing.
        for authored in [-1.0f32, 0.0, 0.25, 0.5, 4.0] {
            let clamped = authored.clamp(0.0, 0.5);
            assert!((0.0..=0.5).contains(&clamped), "{authored} -> {clamped}");
        }
    }

    #[test]
    fn ssr_quality_tiers_are_ordered_by_cost() {
        let low = SsrQuality::Low.to_component();
        let medium = SsrQuality::Medium.to_component();
        let high = SsrQuality::High.to_component();
        assert!(low.linear_steps < medium.linear_steps);
        assert!(medium.linear_steps < high.linear_steps);
        assert!(low.bisection_steps <= high.bisection_steps);
    }

    #[test]
    fn low_quality_hands_rough_surfaces_to_the_probe() {
        // SSR noise is worst on rough surfaces and the environment map is
        // closest to correct there, so the cheap tier should bail out earlier.
        let low = SsrQuality::Low.to_component();
        let high = SsrQuality::High.to_component();
        assert!(low.max_perceptual_roughness.end < high.max_perceptual_roughness.end);
    }

    #[test]
    fn default_probe_inherits_the_scene_environment() {
        let p = ReflectionProbe::default();
        assert!(p.texture.is_empty(), "an unconfigured probe must not bind a texture");
        assert!(p.falloff > 0.0, "zero falloff makes overlapping probes seam");
    }
}

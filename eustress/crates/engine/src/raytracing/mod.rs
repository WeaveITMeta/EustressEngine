//! Hardware ray-traced lighting with Bevy Solari: decided at launch, on by
//! default wherever the GPU can run it well and DLSS Ray Reconstruction can
//! denoise it.
//!
//! # The switch and the guards
//!
//! Auto (the default), On or Off, from `EUSTRESS_RAYTRACING` or Settings >
//! Graphics, read once before the app exists because the renderer is made
//! before any plugin. Auto ray traces when the build has the `raytracing`
//! feature, the GPU passes the gate below, the GPU is real hardware, DLSS Ray
//! Reconstruction is there to denoise it, and no guard has turned it off for
//! this GPU: a launch that crashed before it had drawn its first frames, a
//! render error, or frames measured over budget. On ray traces whenever the
//! build and the GPU can, denoised or not. [`policy`] holds the switch, the
//! guards and their records. The renderer is never switched inside a
//! session; every change applies at the next launch.
//!
//! # The denoiser
//!
//! Bevy Solari 0.19 has no denoiser of its own. Its one denoising path is
//! DLSS Ray Reconstruction (the `dlss` branches of bevy_solari's
//! `realtime/node.rs`); without it the image is grainy. So Auto ray traces
//! only in a build that compiles DLSS (the `dlss` feature without
//! `force_disable_dlss`, [`DLSS_IN_BUILD`]), on a device that reports DLSS
//! Ray Reconstruction when [`render_creation`] makes it
//! ([`policy::Gate::denoiser`]). Bevy starts DLSS itself later, in
//! `DlssPlugin::finish`; when it does not start there, an Auto session falls
//! back to raster before its first frame ([`require_denoiser`]). Everywhere
//! else Auto draws in raster, as Off does.
//!
//! # The gate
//!
//! With Auto or On and the `raytracing` feature built in, [`render_creation`]
//! creates the renderer itself, through Bevy's own `initialize_renderer`,
//! with the engine's exact `WgpuSettings`, and hands that device to
//! `RenderPlugin` as `RenderCreation::Manual`. The gate reads the features of
//! that device, so it is right by construction: there is no second adapter
//! that could differ from the one Bevy renders with. Bevy's automatic path
//! makes the same call with no window surface on desktop (`RenderPlugin::build`
//! runs before winit creates the window, so the primary window's handle
//! holder is still empty), which is why the probe passes `None` too. With the
//! `dlss` feature, the probe also passes the Vulkan instance and device
//! callbacks Bevy's `DlssInitPlugin` registers, built by that same plugin, so
//! DLSS is available exactly as it would be on the automatic path.
//!
//! With Off, or a build without the feature, nothing here runs before the
//! renderer: `RenderPlugin` gets the same `Automatic` creation the engine
//! always used. Every GPU, ray tracing or not, renders as before.
//!
//! [`GpuCapabilities`] (adapter, vendor, device type, backend, driver, and
//! whether the device could trace) is logged once at startup and kept as a
//! resource for the Settings dialog and telemetry.
//!
//! # When ray tracing is on
//!
//! - `SolariPlugins` is added. Its lighting plugin switches every material to
//!   deferred (`DefaultOpaqueRendererMethod::deferred()`) in `build`, before
//!   it checks the GPU, which is why the plugin is added only past the gate.
//! - Every sky camera (the editor and Play cameras) gets `SolariLighting`,
//!   `Msaa::Off` and a storage-capable main texture. The AI capture camera,
//!   which skips the sky, gets the `DeferredPrepass` a deferred material
//!   needs to draw at all from [`sky`] (`deferred_prepass_for_capture_cameras`),
//!   the one owner of that rule: it applies whenever opaque materials default
//!   to deferred, under ray tracing or SSR.
//! - Each scene mesh is traced through a copy laid out the way Solari reads
//!   it ([`conform`]); the mesh the raster path draws is never changed.
//! - A part whose material waited on a texture map is specialized again in
//!   every view once its maps are in ([`LateMaps`]). Bevy 0.19 otherwise
//!   leaves it out of a Solari camera's deferred prepass for good: no depth
//!   and no G-buffer texel, so nothing is drawn there and the sky shows
//!   through.
//! - A part these rules take out of the traced scene leaves Solari's render
//!   world too ([`add_untrace_sync`]): bevy_solari 0.19's extraction only
//!   ever inserts its marker.
//! - The sun's shadow maps are cleared in the render world's copy of each
//!   directional light: Solari traces the sun's visibility instead. A
//!   directional light at zero lux (a moon that has set) leaves the render
//!   world, so it takes none of Solari's light samples.
//! - [`sky`], [`lights`] and [`surfaces`] add what Solari 0.19 does not trace
//!   by itself. The sky lights the traced scene as a hemisphere light: a
//!   directional light with a 180 degree sun disk on render layer 47, which
//!   no camera may use (Solari traces every mesh as an opaque occluder, so a
//!   dome mesh would block the sun). Point, spot and surface lights each get
//!   an emissive stand-in that Solari does trace. Terrain, water and scatter
//!   are traced through proxies. [`sky`] also draws the AI camera's sky and
//!   celestial bodies in both modes.
//! - With the `dlss` feature, every Solari camera gets DLSS Ray
//!   Reconstruction when the GPU and driver support it ([`DLSS_PROJECT_ID`]).
//! - The live fallback ([`RasterFallback`]): when the materials near
//!   Solari's texture limit, or Settings turns ray tracing Off, Solari leaves
//!   every camera, every traced entity leaves the traced scene (main and
//!   render world), the proxies and the traced sky light go, and nothing
//!   traces again this session. The rest of the session draws through Bevy's
//!   deferred raster path.
//! - [`policy`]'s guards run: the crash marker comes down after
//!   [`policy::SURVIVAL_FRAMES`] frames, a render error puts it back, and the
//!   frame-time guard watches the budget.

pub mod policy;

// The traced parts, each its own file. Each exposes
// `pub fn add_systems<M: TraceMarker>(app: &mut App)`, called once with
// Solari's marker: `lights` from `add_tracing_systems` (its proxies run
// exactly when the mesh sync does), `sky` and `surfaces` from `solari::add`.
// `sky` also exposes `pub fn add_always(app: &mut App)`, called from
// `RaytracingPlugin::build` in every build and mode, for the AI camera's sky.
// They compile in every build (generic over `TraceMarker`) and gate their own
// systems on [`raytracing_active`].
pub mod lights;
pub mod sky;
pub mod surfaces;

pub use policy::{
    FallbackReason, RasterFallback, RasterFallbackRequest, RayTracingMode, SwitchSource, ENV_SWITCH, SETTINGS_KEY,
};

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use bevy::app::SubApp;
use bevy::asset::uuid::Uuid;
use bevy::asset::{AssetEvent, RenderAssetUsages};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::CameraMainTextureUsages;
use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::ExtractedDirectionalLight;
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;
use bevy::render::renderer::{RenderAdapterInfo, RenderDevice};
use bevy::render::settings::{RenderCreation, RenderResources, WgpuFeatures, WgpuSettings};
use bevy::render::sync_world::RenderEntity;
use bevy::render::view::Msaa;
use bevy::render::{ExtractSchedule, MainWorld, Render, RenderApp, RenderSystems};
use eustress_common::plugins::lighting_plugin::{NoAtmosphere, SkyCamera};

/// The order of the module's work in `Last`: the tracing rules first, then
/// the fallback decision (texture count, limit, Settings' Off), then the
/// fallback itself, so a fallback decided this frame takes effect before the
/// render world extracts it.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RaytracingSystems {
    Trace,
    Decide,
    Fallback,
}

fn order_sets(app: &mut App) {
    app.configure_sets(
        Last,
        (RaytracingSystems::Trace, RaytracingSystems::Decide, RaytracingSystems::Fallback).chain(),
    );
}

// ============================================================================
// The decision
// ============================================================================

/// What became of the switch this run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaytracingStatus {
    /// Off by choice.
    Off,
    /// Auto or On, but this build has no `raytracing` feature.
    NotInBuild,
    /// Auto or On, but the GPU lacks a feature Solari needs (or no GPU could
    /// be tested).
    Unsupported,
    /// Auto, on a software rasterizer.
    SoftwareGpu,
    /// Auto, and nothing can denoise Solari's output: the build does not
    /// compile DLSS, or the GPU and driver offer no DLSS Ray Reconstruction.
    NoDenoiser,
    /// Auto, and a launch with ray tracing on crashed on this GPU.
    AfterCrash,
    /// Auto, and ray tracing was measured too slow on this GPU.
    TooSlow,
    /// Auto, and the crash guard's marker could not be written.
    NoGuard,
    /// Auto, and the launch Space is recorded as over Solari's texture limit.
    SpaceTooManyTextures,
    /// Ray-traced lighting is running.
    Active,
}

impl RaytracingStatus {
    pub fn is_active(self) -> bool {
        self == Self::Active
    }

    /// Short, for the log.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::NotInBuild => "Not in this build",
            Self::Unsupported => "Not supported by this GPU",
            Self::SoftwareGpu => "Off on a software GPU",
            Self::NoDenoiser => "Off, no denoiser (DLSS Ray Reconstruction)",
            Self::AfterCrash => "Off after a crash at launch",
            Self::TooSlow => "Off, too slow on this GPU",
            Self::NoGuard => "Off, crash guard unavailable",
            Self::SpaceTooManyTextures => "Off for this Space, too many textures",
            Self::Active => "On",
        }
    }
}

/// The wgpu features Bevy Solari 0.19 needs, copied from
/// `SolariPlugins::required_wgpu_features`. A build with the `raytracing`
/// feature asks Solari itself (see [`required_features`]); this copy lets
/// every other build still report whether the hardware could trace.
#[cfg_attr(feature = "raytracing", allow(dead_code))]
fn solari_required_features_copy() -> WgpuFeatures {
    WgpuFeatures::EXPERIMENTAL_RAY_QUERY
        | WgpuFeatures::BUFFER_BINDING_ARRAY
        | WgpuFeatures::TEXTURE_BINDING_ARRAY
        | WgpuFeatures::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
        | WgpuFeatures::PARTIALLY_BOUND_BINDING_ARRAY
}

/// The features the gate requires.
pub fn required_features() -> WgpuFeatures {
    #[cfg(feature = "raytracing")]
    return solari::required_features();
    #[cfg(not(feature = "raytracing"))]
    solari_required_features_copy()
}

/// The gate: a device can run Solari when it has every feature Solari needs.
/// Solari's own check, in each of its plugins' `finish`, is the same
/// `contains` over the render device's features.
pub fn supports(device_features: WgpuFeatures, required: WgpuFeatures) -> bool {
    device_features.contains(required)
}

/// The GPU this run renders with, for the Settings dialog and telemetry.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuCapabilities {
    pub adapter: String,
    /// PCI vendor id (0x10DE is NVIDIA).
    pub vendor_id: u32,
    pub vendor: &'static str,
    pub device_id: u32,
    /// wgpu's device type: `DiscreteGpu`, `IntegratedGpu`, `Cpu`, ...
    pub device_type: String,
    /// The graphics API: `Vulkan`, `Dx12`, `Metal`, `Gl`.
    pub backend: String,
    pub driver: String,
    pub driver_info: String,
    /// The device has every feature Solari needs.
    pub ray_tracing: bool,
    /// The features it lacks for Solari, empty when it has them all.
    pub missing: String,
}

impl GpuCapabilities {
    /// Read from the renderer's resources.
    pub fn read(info: &RenderAdapterInfo, device: &RenderDevice) -> Self {
        let mut caps = Self {
            adapter: info.name.clone(),
            vendor_id: info.vendor,
            vendor: vendor_name(info.vendor),
            device_id: info.device,
            device_type: format!("{:?}", info.device_type),
            backend: format!("{:?}", info.backend),
            driver: info.driver.clone(),
            driver_info: info.driver_info.clone(),
            ..default()
        };
        caps.set_features(device.features());
        caps
    }

    /// Apply the gate to a device's features.
    pub fn set_features(&mut self, features: WgpuFeatures) {
        let required = required_features();
        self.ray_tracing = supports(features, required);
        let missing = required - features;
        self.missing = if missing.is_empty() { String::new() } else { missing.to_string() };
    }
}

/// The vendor behind a PCI vendor id, for people reading the log.
pub fn vendor_name(vendor_id: u32) -> &'static str {
    match vendor_id {
        0x10DE => "NVIDIA",
        0x1002 | 0x1022 => "AMD",
        0x8086 => "Intel",
        0x106B => "Apple",
        0x1414 => "Microsoft",
        0x5143 => "Qualcomm",
        0x13B5 => "Arm",
        0x10005 => "Mesa",
        _ => "unknown",
    }
}

/// What `main` decided before the app was built.
#[derive(Resource, Clone, Debug)]
pub struct RaytracingLaunch {
    pub mode: RayTracingMode,
    pub source: SwitchSource,
    pub status: RaytracingStatus,
    /// The GPU as read before the app, when it was tested (Auto or On in a
    /// build with the feature).
    pub probed: Option<GpuCapabilities>,
    /// This GPU's guard record at launch.
    pub verdict: Option<policy::Verdict>,
    /// This launch found that the last ray-traced launch on this GPU ended
    /// before it had drawn its first frames.
    pub crash_found: bool,
    /// Messages from before logging starts, logged at startup.
    pub notes: Vec<String>,
}

impl RaytracingLaunch {
    /// Nothing tested yet: what `render_creation` starts from.
    pub fn new(mode: RayTracingMode, source: SwitchSource, status: RaytracingStatus) -> Self {
        Self { mode, source, status, probed: None, verdict: None, crash_found: false, notes: Vec::new() }
    }

    /// A launch decided as `status` in Auto, for tests and tools that only
    /// ask whether ray tracing is on (Phase 1's `RaytracingLaunch { status,
    /// probed: None }`).
    pub fn for_status(status: RaytracingStatus) -> Self {
        Self::new(RayTracingMode::Auto, SwitchSource::Default, status)
    }
}

/// Run condition for systems that only matter while ray tracing is on (the
/// traced sky, light and surface proxies). False from the frame a live
/// fallback begins.
pub fn raytracing_active(launch: Option<Res<RaytracingLaunch>>, fallback: Option<Res<RasterFallback>>) -> bool {
    fallback.is_none() && launch.is_some_and(|launch| launch.status.is_active())
}

/// The line under Settings > Graphics > Ray tracing: what this run does and
/// why. Kept current by [`update_note`] and pushed to the dialog by
/// `preferences.rs`.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct RaytracingNote(pub String);

/// The Settings note: what this run did and why, in the form
/// "Auto: on (NVIDIA GeForce RTX 2080 Ti)." The choice itself applies at the
/// next launch, except Off, which stops a running session at once.
/// `events`: what the guards did this session (measured too slow, or fell
/// back to raster).
pub fn settings_note(launch: &RaytracingLaunch, caps: Option<&GpuCapabilities>, events: &policy::GuardEvents) -> String {
    use RaytracingStatus as S;
    let caps = caps.or(launch.probed.as_ref());
    let gpu = caps.map(|c| c.adapter.as_str()).filter(|name| !name.is_empty()).unwrap_or("this GPU");
    let auto = launch.mode == RayTracingMode::Auto;
    let slow_recorded = events.slow_recorded;
    let mode = launch.mode.label();
    let mut note = match (&events.fallback, launch.status) {
        (Some(FallbackReason::TurnedOff), _) => {
            "Off now, for the rest of this session. Auto and On apply at the next launch.".to_string()
        }
        (Some(FallbackReason::TooManyTextures { distinct }), _) => format!(
            "{mode}: off now. This Space uses {distinct} distinct textures, close to the {} ray tracing can bind; \
             it opens without ray tracing next time.",
            policy::SOLARI_TEXTURE_LIMIT
        ),
        (Some(FallbackReason::NoDenoiser), _) => format!(
            "Auto: off, DLSS Ray Reconstruction did not start on {gpu}, so nothing would denoise it; \
             turn On to use it anyway."
        ),
        (None, status) => match status {
        S::Active if auto && slow_recorded => {
            format!("Auto: on ({gpu}) for now, but too slow here, so off from the next launch. Choose On to keep it.")
        }
        S::Active if auto => format!("Auto: on ({gpu})."),
        S::Active => format!("On ({gpu})."),
        S::Off => match caps {
            Some(c) if c.ray_tracing && !cfg!(feature = "raytracing") => {
                format!("Off. {gpu} supports it; this build was made without it.")
            }
            Some(c) if c.ray_tracing => format!("Off. {gpu} supports it."),
            Some(_) => format!("Off. {gpu} does not support it."),
            None => "Off.".to_string(),
        },
        S::NotInBuild => "This build was made without ray tracing.".to_string(),
        S::Unsupported if auto => format!("Auto: off, {}.", lacks(caps)),
        S::Unsupported => format!("On, but {}, so it draws without it.", lacks(caps)),
        S::SoftwareGpu => format!("Auto: off, {gpu} is a software renderer."),
        S::NoDenoiser if DLSS_IN_BUILD => {
            format!("Auto: off, {gpu} offers no DLSS Ray Reconstruction to denoise it; turn On to use it anyway.")
        }
        S::NoDenoiser => {
            "Auto: off, this build has no DLSS Ray Reconstruction to denoise it; turn On to use it anyway.".to_string()
        }
        S::AfterCrash => "Auto: off after a crash at launch; turn On to retry.".to_string(),
        S::TooSlow => "Auto: off, too slow on this GPU; turn On to use it anyway.".to_string(),
        S::NoGuard => "Auto: off, the crash guard could not be saved; turn On to use it anyway.".to_string(),
        S::SpaceTooManyTextures => {
            "Auto: off for this Space, which has more textures than ray tracing can bind; turn On to retry."
                .to_string()
        }
        },
    };
    if launch.source == SwitchSource::Env {
        note.push_str(&format!(" Set by {ENV_SWITCH}, which wins over this setting."));
    }
    note
}

/// Why the gate failed, in words.
fn lacks(caps: Option<&GpuCapabilities>) -> String {
    match caps {
        Some(c) if c.missing.contains("EXPERIMENTAL_RAY_QUERY") => "GPU lacks ray queries".to_string(),
        Some(c) if !c.missing.is_empty() => format!("GPU lacks {}", c.missing),
        _ => "the GPU could not be tested".to_string(),
    }
}

/// The one-time notice when Auto turned ray tracing off because the last
/// ray-traced launch on this GPU crashed. `None` on every other launch.
pub fn launch_notification(launch: &RaytracingLaunch) -> Option<String> {
    (launch.crash_found && launch.status == RaytracingStatus::AfterCrash).then(|| {
        "Ray tracing is off for this launch. The last launch with it on did not finish starting on this GPU, \
         so Auto turned it off. To try again, choose On in Settings > Graphics > Ray tracing."
            .to_string()
    })
}

// ============================================================================
// DLSS (only with the `dlss` feature)
// ============================================================================

/// This application's DLSS project id, generated once (UUID v4, 2026-10-15)
/// for Eustress and never changed: NVIDIA's driver identifies the
/// application by it (DLSS Programming Guide 5.2.1: a "GUID-like" id for a
/// custom engine). Not the id in Bevy's Solari example, which that example
/// says not to copy.
pub const DLSS_PROJECT_ID: Uuid = Uuid::from_u128(0xe197ed2c_5094_4335_a75e_35f4d7da832d);

/// For `main`, before `DefaultPlugins`: Bevy's `DlssInitPlugin`, inside
/// `DefaultPlugins` and ahead of `RenderPlugin`, reads `DlssProjectId` when
/// it builds and panics without it. Nothing in a build without DLSS.
pub fn insert_dlss_project_id(app: &mut App) {
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    app.insert_resource(bevy::anti_alias::dlss::DlssProjectId(DLSS_PROJECT_ID));
    #[cfg(not(all(feature = "dlss", not(feature = "force_disable_dlss"))))]
    let _ = app;
}

/// The Vulkan callbacks `initialize_renderer` takes in a `dlss` build (Bevy's
/// `raw_vulkan_init`, which the `dlss` feature turns on). Bevy's automatic
/// path reads them from the app's `RawVulkanInitSettings`, which
/// `DlssInitPlugin` fills; the probe runs before that plugin exists, so it
/// builds the same plugin on a scratch app and takes the result. Without
/// them DLSS's Vulkan extensions are never enabled and DLSS reports itself
/// unsupported on every GPU.
#[cfg(feature = "dlss")]
fn raw_vulkan_init_settings() -> bevy::render::renderer::raw_vulkan_init::RawVulkanInitSettings {
    use bevy::render::renderer::raw_vulkan_init::RawVulkanInitSettings;
    #[cfg(not(feature = "force_disable_dlss"))]
    {
        let mut scratch = App::empty();
        scratch.insert_resource(bevy::anti_alias::dlss::DlssProjectId(DLSS_PROJECT_ID));
        bevy::anti_alias::dlss::DlssInitPlugin.build(&mut scratch);
        scratch.world_mut().remove_resource::<RawVulkanInitSettings>().unwrap_or_default()
    }
    // `force_disable_dlss` keeps `raw_vulkan_init` on but compiles DLSS out.
    #[cfg(feature = "force_disable_dlss")]
    RawVulkanInitSettings::default()
}

/// The build compiles DLSS: the `dlss` feature, without `force_disable_dlss`.
pub const DLSS_IN_BUILD: bool = cfg!(all(feature = "dlss", not(feature = "force_disable_dlss")));

/// Whether the device [`render_creation`] just made offers DLSS Ray
/// Reconstruction, as Bevy's own DLSS callbacks (`DlssInitPlugin`, passed in
/// by `raw_vulkan_init_settings`) recorded it while they created the Vulkan
/// instance and device. Always false in a build that does not compile DLSS.
fn ray_reconstruction_reported(resources: &RenderResources) -> bool {
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    return reported_by_device(&resources.5);
    #[cfg(not(all(feature = "dlss", not(feature = "force_disable_dlss"))))]
    {
        let _ = resources;
        false
    }
}

/// The two DLSS features the callbacks record per device, both of which
/// Bevy's `DlssPlugin::finish` requires before it starts DLSS and inserts
/// `DlssRayReconstructionSupported` (bevy_anti_alias 0.19, `dlss/mod.rs`:
/// Super Resolution first, then Ray Reconstruction).
#[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
fn reported_by_device(features: &bevy::render::renderer::raw_vulkan_init::AdditionalVulkanFeatures) -> bool {
    use bevy::anti_alias::dlss::{DlssRayReconstructionSupported, DlssSuperResolutionSupported};
    features.has::<DlssSuperResolutionSupported>() && features.has::<DlssRayReconstructionSupported>()
}

// ============================================================================
// The renderer
// ============================================================================

/// The renderer `RenderPlugin` should use, and the ray-tracing decision.
///
/// `settings` is the engine's `WgpuSettings`, built in `main` exactly as it
/// was written inline before.
///
/// - Off, or no `raytracing` feature: `RenderCreation::Automatic` with
///   `settings`, the creation the engine always used.
/// - Auto or On: the renderer is created here with Bevy's own
///   `initialize_renderer`, the same backends and `settings`, and no window
///   surface, as `RenderPlugin`'s automatic path does on desktop. It goes to
///   `RenderPlugin` as `RenderCreation::Manual`, so the features the gate
///   reads are the features of the device Bevy renders with, and so is its
///   report of DLSS Ray Reconstruction, which Auto needs. The guard
///   records for this GPU are read, and a launch that will ray trace puts up
///   its crash marker before returning, so before Solari builds a pipeline.
///   A missing GPU panics here exactly as it would in `RenderPlugin::build`.
/// `space_root` is the Space this launch opens: one recorded as over the
/// texture limit opens without ray tracing in Auto.
pub fn render_creation(settings: WgpuSettings, space_root: &Path) -> (RenderCreation, RaytracingLaunch) {
    let env = std::env::var(ENV_SWITCH).ok();
    let saved = policy::saved_settings_text();
    let (mode, source) = policy::requested_mode(env.as_deref(), saved.as_deref());
    let compiled = cfg!(feature = "raytracing");
    let mut launch = RaytracingLaunch::new(mode, source, RaytracingStatus::Off);
    launch.notes.extend(policy::ignored_env_value(env.as_deref()));
    if mode == RayTracingMode::Off || !compiled {
        launch.status = RaytracingStatus::decide(mode, compiled, None, None);
        return (RenderCreation::Automatic(Box::new(settings)), launch);
    }
    // Bevy's automatic path creates no renderer without backends; keep that.
    let Some(backends) = settings.backends else {
        launch.status = RaytracingStatus::Unsupported;
        return (RenderCreation::Automatic(Box::new(settings)), launch);
    };
    let resources = bevy::tasks::block_on(bevy::render::renderer::initialize_renderer(
        backends,
        None,
        &settings,
        #[cfg(feature = "dlss")]
        raw_vulkan_init_settings(),
    ));
    let caps = GpuCapabilities::read(&resources.2, &resources.0);
    let denoiser = ray_reconstruction_reported(&resources);
    let identity = policy::GpuIdentity::of(&caps);
    let store = policy::GuardStore::default_location();
    let check = match &store {
        Some(store) => store.launch_check(&identity, Some(&policy::space_key(space_root)), policy::now_unix()).unwrap_or_else(|e| {
            launch.notes.push(format!("could not save the guard records: {e}"));
            policy::LaunchCheck::default()
        }),
        None => {
            launch.notes.push("no local data folder, so the guards keep no records".to_string());
            policy::LaunchCheck::default()
        }
    };
    let status = RaytracingStatus::decide(
        mode,
        true,
        Some(policy::Gate::of(&caps, denoiser)),
        check.verdict.as_ref().map(|verdict| verdict.kind),
    )
    .after_space_verdict(mode, check.space_verdict.is_some());
    let armed = status.is_active()
        && match store {
            Some(store) => match policy::begin_session(store, identity) {
                Ok(()) => true,
                Err(e) => {
                    launch.notes.push(e);
                    false
                }
            },
            None => false,
        };
    launch.status = status.after_arming(mode, armed);
    launch.probed = Some(caps);
    launch.verdict = check.verdict;
    launch.crash_found = check.crash_found;
    (RenderCreation::Manual(resources), launch)
}

// ============================================================================
// Plugin
// ============================================================================

/// Added by `main` after `DefaultPlugins` (Solari builds on `RenderPlugin`
/// and `PbrPlugin`), with the decision [`render_creation`] made.
pub struct RaytracingPlugin {
    launch: RaytracingLaunch,
}

impl RaytracingPlugin {
    pub fn new(launch: RaytracingLaunch) -> Self {
        Self { launch }
    }
}

impl Plugin for RaytracingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.launch.clone())
            .init_resource::<RaytracingNote>()
            .init_resource::<policy::GuardEvents>()
            .add_systems(Startup, (report_gpu, notify_launch_guard))
            .add_systems(Update, update_note);
        if let Some(store) = policy::GuardStore::default_location() {
            app.insert_resource(policy::GuardFolder(store));
        }
        order_sets(app);
        policy::add_fallback_systems(app);
        // The AI camera's sky and celestial bodies, raster or traced.
        sky::add_always(app);
        if self.launch.status.is_active() {
            policy::add_session_systems(app, &self.launch);
            #[cfg(feature = "raytracing")]
            solari::add(app);
        }
    }

    fn finish(&self, app: &mut App) {
        if !self.launch.status.is_active() {
            return;
        }
        // Bevy's `DlssPlugin` is in DefaultPlugins, so its `finish` has run:
        // DLSS has started, or failed to.
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        require_denoiser::<bevy::anti_alias::dlss::DlssRayReconstructionSupported>(app, &self.launch);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        add_render_rules(render_app);
    }
}

/// The render-world rules of a ray-traced session: the sun's shadow maps
/// off (Solari traces its visibility) and dark directional lights out.
/// Both stop after a live fallback, so the sun's shadow maps come back.
fn add_render_rules(render_app: &mut SubApp) {
    render_app.add_systems(
        Render,
        (suppress_sun_shadow_maps, drop_dark_directional_lights)
            .after(RenderSystems::ExtractCommands)
            .before(RenderSystems::CreateViews)
            .run_if(not(resource_exists::<RenderRasterFallback>)),
    );
}

/// The systems that apply Solari to cameras and meshes, generic over the two
/// Solari components so they compile and are tested in every build. Also the
/// one call site of [`lights`] (its proxies run exactly when the mesh sync
/// does); [`sky`] and [`surfaces`] are added by `solari::add`.
pub fn add_tracing_systems<M: TraceMarker, L: Component + Default>(app: &mut App) {
    lights::add_systems::<M>(app);
    add_untrace_sync::<M>(app);
    order_sets(app);
    app.init_resource::<TracedMeshes>()
        .add_systems(
            Last,
            (
                // Last: after every system that inserts `Msaa` or swaps a
                // material this frame, and before the render world extracts.
                (attach_lighting_to_sky_cameras::<L>, keep_msaa_off::<L>).chain(),
                (sync_traced_meshes::<M>, guard_emissive_limit::<M>).chain(),
            )
                .in_set(RaytracingSystems::Trace)
                .run_if(not(resource_exists::<RasterFallback>)),
        )
        .add_systems(
            Last,
            sweep_traced_scene::<M, L>
                .in_set(RaytracingSystems::Fallback)
                .run_if(resource_exists::<RasterFallback>),
        );
}

/// The live fallback's sweep, every frame of a fallen-back session: Solari
/// leaves every camera, a drawn part keeps its mesh and loses the marker,
/// a trace-only entity (the light and surface proxies have a marker and no
/// `Mesh3d`) is despawned, and so is the traced sky light. Every frame, not
/// once: a proxy system that ran this frame before the fallback was decided
/// can still add one, and Solari binds every material's textures as long as
/// anything at all is traced. (The render world is emptied as well, by
/// `strip_traced_render_entities`.)
fn sweep_traced_scene<M: TraceMarker, L: Component>(
    mut commands: Commands,
    lit_cameras: Query<Entity, With<L>>,
    traced: Query<(Entity, Has<Mesh3d>), With<M>>,
    sky_lights: Query<Entity, With<sky::TracedSkyLight>>,
) {
    for camera in &lit_cameras {
        commands.entity(camera).remove::<L>();
    }
    for (entity, drawn) in &traced {
        if drawn {
            commands.entity(entity).try_remove::<M>();
        } else {
            commands.entity(entity).try_despawn();
        }
    }
    for light in &sky_lights {
        commands.entity(light).try_despawn();
    }
}

/// Puts the denoiser `D` on every camera with the lighting component `L`,
/// while the resource `R` that says the GPU supports `D` exists. Generic so
/// it compiles and is tested in every build; a `dlss` build uses it for DLSS
/// Ray Reconstruction on Solari cameras, which Bevy's Solari example calls
/// highly recommended (Solari 0.19 has no denoiser of its own). Cameras
/// without `L` (raster cameras, the AI capture camera) never get it.
pub fn add_denoiser_systems<L: Component, D: Component, R: Resource>(app: &mut App, make: fn() -> D) {
    app.add_systems(
        Last,
        (
            (move |mut commands: Commands, cameras: Query<Entity, (With<L>, Without<D>)>| {
                for entity in &cameras {
                    commands.entity(entity).insert(make());
                }
            })
            .run_if(resource_exists::<R>)
            .run_if(not(resource_exists::<RasterFallback>)),
            // And it leaves with the lighting (the live fallback): Ray
            // Reconstruction reads Solari's buffers.
            |mut commands: Commands, cameras: Query<Entity, (With<D>, Without<L>)>| {
                for entity in &cameras {
                    commands.entity(entity).remove::<D>();
                }
            },
        ),
    );
}

/// Auto ray traces only with a denoiser. [`render_creation`] decided on the
/// device's report of DLSS Ray Reconstruction; DLSS itself starts later, in
/// Bevy's `DlssPlugin::finish`, which inserts `R`
/// (`DlssRayReconstructionSupported`) only when it started. Without `R`, an
/// Auto session falls back to raster ([`FallbackReason::NoDenoiser`]) before
/// its first frame, so nothing is ever traced or drawn grainy. On keeps
/// tracing, for testing. Called from `RaytracingPlugin::finish`, after
/// `DlssPlugin`'s; generic so it compiles and is tested in every build.
pub fn require_denoiser<R: Resource>(app: &mut App, launch: &RaytracingLaunch) {
    if launch.mode == RayTracingMode::Auto && launch.status.is_active() && !app.world().contains_resource::<R>() {
        app.insert_resource(RasterFallback { reason: FallbackReason::NoDenoiser });
    }
}

/// Log the GPU once, check it against the launch decision, and keep it.
fn report_gpu(
    mut commands: Commands,
    launch: Res<RaytracingLaunch>,
    info: Option<Res<RenderAdapterInfo>>,
    device: Option<Res<RenderDevice>>,
) {
    for note in &launch.notes {
        warn!("Ray tracing: {note}");
    }
    let (Some(info), Some(device)) = (info, device) else {
        warn!("GPU: no renderer, so no GPU capabilities to report");
        return;
    };
    let caps = GpuCapabilities::read(&info, &device);
    if let Some(probed) = &launch.probed {
        // Cannot differ: `RenderCreation::Manual` hands Bevy the probed device.
        if probed.ray_tracing != caps.ray_tracing || probed.adapter != caps.adapter {
            error!(
                "Ray tracing: the GPU Bevy renders with ({}, ray tracing {}) is not the one the gate read ({}, ray tracing {})",
                caps.adapter, caps.ray_tracing, probed.adapter, probed.ray_tracing
            );
        }
    }
    info!(
        "GPU: {} ({} {:04x}:{:04x}, {}, {}), driver {} {}; can ray trace: {}",
        caps.adapter,
        caps.vendor,
        caps.vendor_id,
        caps.device_id,
        caps.device_type,
        caps.backend,
        caps.driver,
        caps.driver_info,
        caps.ray_tracing
    );
    let mode = launch.mode.label();
    let detail = launch.verdict.as_ref().map_or("", |verdict| verdict.detail.as_str());
    match launch.status {
        RaytracingStatus::Off => {}
        RaytracingStatus::NotInBuild => {
            warn!("Ray tracing ({mode}): this build has no `raytracing` feature; drawing as usual")
        }
        RaytracingStatus::Unsupported => warn!(
            "Ray tracing ({mode}): this GPU lacks {}; drawing as usual",
            if caps.missing.is_empty() { "a required backend" } else { caps.missing.as_str() }
        ),
        RaytracingStatus::SoftwareGpu => info!("Ray tracing (Auto): off on a software renderer"),
        RaytracingStatus::NoDenoiser => info!(
            "Ray tracing (Auto): off; Solari needs a denoiser (DLSS Ray Reconstruction), which this {} lacks. \
             Settings On turns it on anyway.",
            if DLSS_IN_BUILD { "GPU or its driver" } else { "build" }
        ),
        RaytracingStatus::AfterCrash => warn!("Ray tracing (Auto): off on this GPU after a crash ({detail})"),
        RaytracingStatus::TooSlow => info!("Ray tracing (Auto): off on this GPU, recorded too slow ({detail})"),
        RaytracingStatus::NoGuard => warn!("Ray tracing (Auto): off, the crash guard could not be saved"),
        RaytracingStatus::SpaceTooManyTextures => info!(
            "Ray tracing (Auto): off for this Space, recorded as over the {}-texture limit",
            policy::SOLARI_TEXTURE_LIMIT
        ),
        RaytracingStatus::Active => info!(
            "Ray tracing ({mode}): on (Bevy Solari, experimental); sun shadow maps off, Solari traces them"
        ),
    }
    commands.insert_resource(caps);
}

/// Tell the person once when Auto turned ray tracing off after a crash.
fn notify_launch_guard(
    launch: Res<RaytracingLaunch>,
    notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    if let (Some(text), Some(mut notifications)) = (launch_notification(&launch), notifications) {
        notifications.warning(text);
    }
}

/// Keep [`RaytracingNote`] current: once at startup, when the GPU's
/// capabilities arrive, and when a guard records something this session.
fn update_note(
    launch: Res<RaytracingLaunch>,
    caps: Option<Res<GpuCapabilities>>,
    events: Res<policy::GuardEvents>,
    mut note: ResMut<RaytracingNote>,
) {
    let caps_changed = caps.as_ref().is_some_and(|caps| caps.is_changed());
    if !(note.0.is_empty() || caps_changed || events.is_changed()) {
        return;
    }
    let text = settings_note(&launch, caps.as_deref(), &events);
    if note.0 != text {
        note.0 = text;
    }
}

// ============================================================================
// Cameras
// ============================================================================

/// Solari on every sky camera: the same set the sky plugin manages and SSR
/// uses, which is every scene camera a person looks through (editor, Play).
/// `SolariLighting` brings its own requirements (Hdr, the deferred, depth and
/// motion-vector prepasses and their history buffers); Solari also needs
/// MSAA off and a main texture it can write as storage.
fn attach_lighting_to_sky_cameras<L: Component + Default>(
    mut commands: Commands,
    cameras: Query<
        (Entity, Option<&CameraMainTextureUsages>),
        (With<Camera3d>, With<SkyCamera>, Without<NoAtmosphere>, Without<L>),
    >,
) {
    for (entity, usages) in &cameras {
        let usages = usages.cloned().unwrap_or_default().with(TextureUsages::STORAGE_BINDING);
        commands.entity(entity).insert((L::default(), Msaa::Off, usages));
        info!("Ray tracing: Solari lighting attached to camera {entity:?}");
    }
}

/// `EUSTRESS_MSAA` asks the view look for hardware MSAA, which Solari cannot
/// use: put it back to off whenever anything changes it.
fn keep_msaa_off<L: Component>(mut cameras: Query<&mut Msaa, (With<L>, Changed<Msaa>)>) {
    for mut msaa in &mut cameras {
        if *msaa != Msaa::Off {
            *msaa = Msaa::Off;
        }
    }
}

// ============================================================================
// Leaving the traced scene
// ============================================================================

/// Main-world entities whose trace marker was removed since the last
/// extraction, each with its render entity.
#[derive(Resource, Default, Debug)]
pub struct UntracedEntities(Vec<(Entity, Entity)>);

/// Makes a removed marker leave Solari's render world too.
///
/// bevy_solari 0.19's `extract_raytracing_scene` only inserts: for every
/// main-world entity with the marker, it inserts the marker, the material
/// and the transforms on the entity's render entity. Nothing removes them,
/// and the render entity outlives the marker (the marker's required
/// `SyncToRenderWorld` stays). Without this, every part the rules here take
/// out of the traced scene (hidden, glass, `NotShadowCaster`,
/// `TraceOverride::Never`, a material swap) would stay traced, and an
/// emissive mesh over Solari's triangle limit would reach its panic anyway.
///
/// - A main-world observer records the render entity of each entity whose
///   marker is removed.
/// - An `ExtractSchedule` system removes the marker, and only the marker
///   (other extractors share the render entity), from those render entities,
///   unless the main-world entity has a marker again by then (taken out and
///   put back within one frame: the extraction keeps it current). It empties
///   the list as it goes, so nothing is missed or removed twice.
///
/// A replaced marker (a new copy for an edited mesh) needs nothing: the
/// extraction overwrites the render entity's marker with the new one. A
/// despawned entity needs nothing either: its render entity goes with it.
pub fn add_untrace_sync<M: TraceMarker>(app: &mut App) {
    app.init_resource::<UntracedEntities>().add_observer(record_untraced::<M>);
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app
            .add_systems(ExtractSchedule, (remove_untraced_markers::<M>, extract_raster_fallback))
            .add_systems(
                Render,
                strip_traced_render_entities::<M>
                    .after(RenderSystems::ExtractCommands)
                    .before(RenderSystems::PrepareAssets)
                    .run_if(resource_exists::<RenderRasterFallback>),
            );
    }
}

/// The render world's copy of [`RasterFallback`].
#[derive(Resource)]
pub struct RenderRasterFallback;

fn extract_raster_fallback(
    fallback: bevy::render::Extract<Option<Res<RasterFallback>>>,
    current: Option<Res<RenderRasterFallback>>,
    mut commands: Commands,
) {
    match (fallback.is_some(), current.is_some()) {
        (true, false) => commands.insert_resource(RenderRasterFallback),
        (false, true) => commands.remove_resource::<RenderRasterFallback>(),
        _ => {}
    }
}

/// After a fallback, no render entity keeps the marker, whatever extracted
/// it, before Solari builds its acceleration structures or binds a texture:
/// its binder returns before gathering textures when nothing is traced.
fn strip_traced_render_entities<M: TraceMarker>(mut commands: Commands, traced: Query<Entity, With<M>>) {
    for entity in &traced {
        commands.entity(entity).remove::<M>();
    }
}

fn record_untraced<M: TraceMarker>(
    removed: On<Remove, M>,
    render_entities: Query<&RenderEntity>,
    mut untraced: ResMut<UntracedEntities>,
) {
    // No render entity yet: it was never extracted, so nothing to undo.
    if let Ok(render_entity) = render_entities.get(removed.entity) {
        untraced.0.push((removed.entity, render_entity.id()));
    }
}

fn remove_untraced_markers<M: TraceMarker>(mut main_world: ResMut<MainWorld>, mut commands: Commands) {
    let pending = match main_world.get_resource_mut::<UntracedEntities>() {
        Some(mut untraced) if !untraced.0.is_empty() => std::mem::take(&mut untraced.0),
        _ => return,
    };
    for (main_entity, render_entity) in pending {
        if main_world.get::<M>(main_entity).is_some() {
            continue;
        }
        if let Ok(mut entity) = commands.get_entity(render_entity) {
            entity.try_remove::<M>();
        }
    }
}

// ============================================================================
// Lights
// ============================================================================

/// Solari traces the sun's visibility, so its shadow maps would be drawn for
/// nothing. Cleared in the render world's copy, as the Shadows setting does
/// in `preferences.rs`, so each light keeps its own flag. Point and spot
/// lights are not traced by Solari 0.19 and keep their maps.
fn suppress_sun_shadow_maps(mut suns: Query<&mut ExtractedDirectionalLight>) {
    for mut sun in &mut suns {
        if sun.shadow_maps_enabled {
            sun.shadow_maps_enabled = false;
        }
    }
}

/// A directional light that lights nothing: zero lux, or a value that is
/// not a positive number.
pub fn is_dark(illuminance: f32) -> bool {
    !(illuminance > 0.0)
}

/// Solari makes every `ExtractedDirectionalLight` one of its light sources
/// and samples them uniformly (`prepare_raytracing_scene_bindings`), so a
/// moon that has set, at zero lux, would still take its share of the light
/// samples: a third, beside the sun and the traced sky. It lights nothing in
/// raster either, so it leaves the render world the way Bevy takes out an
/// invisible directional light: its `ExtractedDirectionalLight` goes. The
/// extraction puts it back the first frame it has light again.
fn drop_dark_directional_lights(mut commands: Commands, lights: Query<(Entity, &ExtractedDirectionalLight)>) {
    for (entity, light) in &lights {
        if is_dark(light.illuminance) {
            commands.entity(entity).remove::<ExtractedDirectionalLight>();
        }
    }
}

// ============================================================================
// Mesh conformance
// ============================================================================

/// Solari panics on an emissive mesh with more triangles than this
/// (`GpuLightSource::new_emissive_mesh_light`: "Too many triangles ... in an
/// emissive mesh, maximum is 65535").
pub const EMISSIVE_TRIANGLE_LIMIT: u32 = u16::MAX as u32;

/// Why a mesh is left out of ray tracing. Counted once per distinct mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SkipReason {
    /// Only the GPU holds its data (asset usage `RENDER_WORLD` alone, already
    /// uploaded), so it cannot be read or copied.
    GpuOnly,
    /// Not a triangle list.
    NotTriangleList,
    /// No positions or no normals.
    MissingPositionOrNormal,
    /// Positions, normals, UVs or tangents in another format than Solari
    /// reads, or attributes of different lengths.
    UnexpectedFormat,
    /// Carries an attribute the raster path draws with: vertex colors, a
    /// second UV set, skinning joints or weights, or a custom attribute.
    /// Solari reads only positions, normals, UVs and tangents, so a traced
    /// copy would light a different surface than the one drawn, and a
    /// skinned mesh would be traced in its bind pose.
    RasterOnlyAttribute,
    /// Morph targets: the drawn shape moves, a traced copy would not.
    MorphTargets,
    /// Indices that are not whole triangles or point past the vertices.
    BadIndices,
    /// No vertices.
    Empty,
    /// mikktspace could not build tangents.
    TangentsFailed,
}

/// What tracing uses for one raster mesh.
#[derive(Debug)]
pub enum Conformance {
    /// The mesh is already what Solari traces: trace it as it is.
    AsIs { triangles: u32 },
    /// A copy laid out for Solari. The raster mesh is untouched.
    Copy(Mesh),
    Skip(SkipReason),
}

/// Build what Solari needs from a raster mesh, without changing it.
///
/// Solari traces a mesh only with exactly `POSITION`, `NORMAL`, `UV_0` and
/// `TANGENT`, `U32` indices, a triangle list and `enable_raytracing`; any
/// other mesh it skips silently. Changing the drawn mesh to match is not
/// lossless: tangents turn on normal and parallax maps the raster path
/// ignores without them, and change how a double-sided material lights its
/// back faces (`prepare_world_normal` only flips the normal without
/// tangents); zero UVs make a textured material sample texel (0, 0). So the
/// raster mesh stays as it is and Solari gets a copy, the use
/// `Mesh::enable_raytracing`'s documentation describes for proxy meshes.
///
/// The copy keeps the positions and normals, keeps the UVs or fills zeros as
/// Bevy's Solari example does, keeps the tangents or builds them with
/// mikktspace, widens `U16` indices to `U32` and indexes a non-indexed mesh.
pub fn conform(mesh: &Mesh) -> Conformance {
    use Conformance::Skip;
    use SkipReason::*;

    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return Skip(NotTriangleList);
    }
    let Ok(attributes) = mesh.try_attributes() else {
        return Skip(GpuOnly);
    };
    match mesh.try_has_morph_targets() {
        Ok(false) => {}
        Ok(true) => return Skip(MorphTargets),
        Err(_) => return Skip(GpuOnly),
    }

    let (mut positions, mut normals, mut uvs, mut tangents) = (None, None, None, None);
    for (attribute, values) in attributes {
        let id = attribute.id;
        if id == Mesh::ATTRIBUTE_POSITION.id {
            let VertexAttributeValues::Float32x3(v) = values else { return Skip(UnexpectedFormat) };
            positions = Some(v);
        } else if id == Mesh::ATTRIBUTE_NORMAL.id {
            let VertexAttributeValues::Float32x3(v) = values else { return Skip(UnexpectedFormat) };
            normals = Some(v);
        } else if id == Mesh::ATTRIBUTE_UV_0.id {
            let VertexAttributeValues::Float32x2(v) = values else { return Skip(UnexpectedFormat) };
            uvs = Some(v);
        } else if id == Mesh::ATTRIBUTE_TANGENT.id {
            let VertexAttributeValues::Float32x4(v) = values else { return Skip(UnexpectedFormat) };
            tangents = Some(v);
        } else {
            return Skip(RasterOnlyAttribute);
        }
    }
    let (Some(positions), Some(normals)) = (positions, normals) else {
        return Skip(MissingPositionOrNormal);
    };
    let vertices = positions.len();
    if vertices == 0 {
        return Skip(Empty);
    }
    if normals.len() != vertices
        || uvs.is_some_and(|v| v.len() != vertices)
        || tangents.is_some_and(|v| v.len() != vertices)
    {
        return Skip(UnexpectedFormat);
    }

    let source_indices = match mesh.try_indices_option() {
        Ok(indices) => indices,
        Err(_) => return Skip(GpuOnly),
    };
    let index_count = source_indices.map_or(vertices, Indices::len);
    if index_count == 0
        || index_count % 3 != 0
        || u32::try_from(vertices).is_err()
        || source_indices.is_some_and(|indices| indices.iter().any(|i| i >= vertices))
    {
        return Skip(BadIndices);
    }
    let Ok(triangles) = u32::try_from(index_count / 3) else {
        return Skip(BadIndices);
    };

    let wide = matches!(source_indices, Some(Indices::U32(_)));
    if wide && uvs.is_some() && tangents.is_some() && mesh.enable_raytracing {
        return Conformance::AsIs { triangles };
    }

    // Keep the CPU copy (default usage) even when the source is GPU-only:
    // other systems read meshes from asset events after upload, and
    // `Mesh::indices()` panics on a mesh whose data went to the GPU.
    let mut copy = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    copy.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone());
    copy.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone());
    copy.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        uvs.cloned().unwrap_or_else(|| vec![[0.0, 0.0]; vertices]),
    );
    copy.insert_indices(Indices::U32(match source_indices {
        Some(indices) => indices.iter().map(|i| i as u32).collect(),
        None => (0..vertices as u32).collect(),
    }));
    match tangents {
        Some(t) => copy.insert_attribute(Mesh::ATTRIBUTE_TANGENT, t.clone()),
        None => {
            if copy.generate_tangents().is_err() {
                return Skip(TangentsFailed);
            }
        }
    }
    // The default, stated because Solari skips a mesh without it.
    copy.enable_raytracing = true;
    Conformance::Copy(copy)
}

fn triangle_count(mesh: &Mesh) -> u32 {
    mesh.try_indices_option()
        .ok()
        .flatten()
        .map_or(0, |indices| u32::try_from(indices.len() / 3).unwrap_or(u32::MAX))
}

// ============================================================================
// Which entities are traced
// ============================================================================

/// The component that puts an entity in Solari's scene (`RaytracingMesh3d`
/// in builds with the `raytracing` feature). A trait so the rules below
/// compile and are tested in every build.
pub trait TraceMarker: Component + Sized {
    fn new(mesh: Handle<Mesh>) -> Self;
    fn mesh_id(&self) -> AssetId<Mesh>;
}

/// One entity's exception to the automatic rules.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceOverride {
    /// Never traced (tool visuals, previews, anything that should not block
    /// or bounce light).
    Never,
    /// Traced whenever its mesh can be, past the shadow-caster, lit and
    /// opaque rules: an emissive light proxy must emit traced light without
    /// casting a raster shadow. Visibility, render layers and the emissive
    /// triangle limit still apply.
    Always,
}

/// What the rules read from a `StandardMaterial`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterialFacts {
    /// `AlphaMode::Opaque`. Solari treats every traced surface as opaque, so
    /// glass, fades and cut-outs would block light they let through.
    pub opaque: bool,
    /// Not `unlit`. Solari would light an unlit surface.
    pub lit: bool,
    /// Emissive color not black: Solari makes the mesh a light, which is
    /// where its triangle limit applies. Same test as Solari's
    /// `material.emissive != Vec3::ZERO`.
    pub emissive: bool,
}

impl MaterialFacts {
    pub fn of(material: &StandardMaterial) -> Self {
        let e = material.emissive;
        Self {
            opaque: matches!(material.alpha_mode, AlphaMode::Opaque),
            lit: !material.unlit,
            emissive: e.red != 0.0 || e.green != 0.0 || e.blue != 0.0,
        }
    }
}

/// What the rules read from an entity.
#[derive(Clone, Copy, Debug)]
pub struct EntityFacts {
    /// Visible in its hierarchy. Solari ignores visibility, so a hidden part
    /// would still cast traced shadows unless it leaves the traced scene.
    pub visible: bool,
    /// No `NotShadowCaster`: what the raster path lets light through, the
    /// traced scene does too.
    pub casts_shadows: bool,
    /// On render layer 0 with the scene cameras. The Slint overlay quad is a
    /// StandardMaterial on layer 31 and must not become geometry.
    pub main_layer: bool,
    pub over: Option<TraceOverride>,
    /// `None` until the material asset has loaded.
    pub material: Option<MaterialFacts>,
}

/// Whether an entity whose mesh is traceable, with `triangles` triangles,
/// is traced.
pub fn is_traced(facts: &EntityFacts, triangles: u32) -> bool {
    if facts.over == Some(TraceOverride::Never) || !facts.visible || !facts.main_layer {
        return false;
    }
    let Some(material) = facts.material else {
        return false;
    };
    // Crash guard: no override lifts it.
    if material.emissive && triangles > EMISSIVE_TRIANGLE_LIMIT {
        return false;
    }
    if facts.over == Some(TraceOverride::Always) {
        return true;
    }
    facts.casts_shadows && material.opaque && material.lit
}

fn on_main_layer(layers: &RenderLayers) -> bool {
    layers.intersects(&RenderLayers::default())
}

/// Insert the marker only if, when the command runs, the entity still draws
/// a mesh with a StandardMaterial. `RaytracingMesh3d` requires
/// `MeshMaterial3d<StandardMaterial>`: inserted on an entity that has just
/// swapped to another material (terrain chunks do), it would add a default
/// StandardMaterial and the mesh would draw twice.
pub fn insert_if_drawn<M: TraceMarker>(mesh: Handle<Mesh>) -> impl FnOnce(EntityWorldMut) + Send + 'static {
    move |mut entity: EntityWorldMut| {
        if entity.contains::<Mesh3d>() && entity.contains::<MeshMaterial3d<StandardMaterial>>() {
            entity.insert(M::new(mesh));
        }
    }
}

// ============================================================================
// Bookkeeping
// ============================================================================

#[derive(Debug)]
enum Entry {
    AsIs { triangles: u32 },
    Copy { handle: Handle<Mesh>, triangles: u32 },
    Skip(SkipReason),
}

/// One entry per distinct raster mesh, so a mesh shared by thousands of
/// parts is converted once.
#[derive(Resource, Default)]
pub struct TracedMeshes {
    entries: HashMap<AssetId<Mesh>, Entry>,
    /// The copies made here, whose own asset events are not raster changes.
    copies: HashSet<AssetId<Mesh>>,
    /// Meshes an entity needed before they had loaded.
    waiting_meshes: HashSet<AssetId<Mesh>>,
    /// The facts each material had when an entity using it was last
    /// decided, so an edit that changes nothing the rules read costs nothing.
    materials: HashMap<AssetId<StandardMaterial>, MaterialFacts>,
    /// Materials an entity needed before they had loaded.
    waiting_materials: HashSet<AssetId<StandardMaterial>>,
    /// Traced entities whose traced mesh is over the emissive limit.
    large: HashSet<Entity>,
    counts_changed: bool,
    /// Frames that scanned every candidate entity (a used mesh or material
    /// changed). For profiling: steady state should add none.
    full_scans: u64,
}

/// Distinct meshes by outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraceCounts {
    pub as_is: usize,
    pub copied: usize,
    pub skipped: BTreeMap<SkipReason, usize>,
}

impl TracedMeshes {
    pub fn counts(&self) -> TraceCounts {
        let mut counts = TraceCounts::default();
        for entry in self.entries.values() {
            match entry {
                Entry::AsIs { .. } => counts.as_is += 1,
                Entry::Copy { .. } => counts.copied += 1,
                Entry::Skip(reason) => *counts.skipped.entry(*reason).or_default() += 1,
            }
        }
        counts
    }

    pub fn full_scans(&self) -> u64 {
        self.full_scans
    }

    /// The copy made for a raster mesh, if one was.
    pub fn copy_of(&self, raster: AssetId<Mesh>) -> Option<AssetId<Mesh>> {
        match self.entries.get(&raster) {
            Some(Entry::Copy { handle, .. }) => Some(handle.id()),
            _ => None,
        }
    }

    /// The handle and triangle count tracing uses for a raster mesh, decided
    /// on first use. `None` while the mesh is loading or when it is skipped.
    fn target(&mut self, raster: &Handle<Mesh>, meshes: &mut Assets<Mesh>) -> Option<(Handle<Mesh>, u32)> {
        let id = raster.id();
        if !self.entries.contains_key(&id) {
            let Some(mesh) = meshes.get(id) else {
                self.waiting_meshes.insert(id);
                return None;
            };
            let conformance = conform(mesh);
            let entry = self.make_entry(conformance, meshes);
            self.entries.insert(id, entry);
            self.counts_changed = true;
        }
        match self.entries.get(&id)? {
            Entry::AsIs { triangles } => Some((raster.clone(), *triangles)),
            Entry::Copy { handle, triangles } => Some((handle.clone(), *triangles)),
            Entry::Skip(_) => None,
        }
    }

    fn make_entry(&mut self, conformance: Conformance, meshes: &mut Assets<Mesh>) -> Entry {
        match conformance {
            Conformance::AsIs { triangles } => Entry::AsIs { triangles },
            Conformance::Copy(mesh) => {
                let triangles = triangle_count(&mesh);
                let handle = meshes.add(mesh);
                self.copies.insert(handle.id());
                Entry::Copy { handle, triangles }
            }
            Conformance::Skip(reason) => Entry::Skip(reason),
        }
    }

    /// The raster mesh changed: decide again. A copy is rewritten under the
    /// same handle, so entities keep their marker and Solari rebuilds the
    /// acceleration structure from the new data.
    fn refresh(&mut self, id: AssetId<Mesh>, meshes: &mut Assets<Mesh>) {
        let Some(conformance) = meshes.get(id).map(conform) else {
            self.forget(id);
            return;
        };
        let entry = match (self.entries.remove(&id), conformance) {
            (Some(Entry::Copy { handle, .. }), Conformance::Copy(mesh)) => {
                let triangles = triangle_count(&mesh);
                match meshes.insert(handle.id(), mesh) {
                    Ok(()) => Entry::Copy { handle, triangles },
                    Err(_) => {
                        let mesh = meshes.get(id).map(conform);
                        match mesh {
                            Some(conformance) => self.make_entry(conformance, meshes),
                            None => return,
                        }
                    }
                }
            }
            (_, conformance) => self.make_entry(conformance, meshes),
        };
        self.entries.insert(id, entry);
        self.counts_changed = true;
    }

    /// The raster mesh is gone. Dropping the entry drops this resource's
    /// handle to the copy; entities still holding it are decided again.
    fn forget(&mut self, id: AssetId<Mesh>) -> bool {
        let gone = self.entries.remove(&id).is_some();
        self.counts_changed |= gone;
        gone
    }
}

// ============================================================================
// Systems
// ============================================================================

#[derive(SystemParam)]
struct Removed<'w, 's> {
    material: RemovedComponents<'w, 's, MeshMaterial3d<StandardMaterial>>,
    mesh: RemovedComponents<'w, 's, Mesh3d>,
    no_shadow: RemovedComponents<'w, 's, NotShadowCaster>,
    layers: RemovedComponents<'w, 's, RenderLayers>,
    over: RemovedComponents<'w, 's, TraceOverride>,
}

type Candidate<M> = (
    Entity,
    &'static Mesh3d,
    &'static MeshMaterial3d<StandardMaterial>,
    &'static InheritedVisibility,
    Has<NotShadowCaster>,
    Option<&'static RenderLayers>,
    Option<&'static TraceOverride>,
    Option<&'static M>,
);

type Touched = Or<(
    Changed<Mesh3d>,
    Changed<MeshMaterial3d<StandardMaterial>>,
    Changed<InheritedVisibility>,
    Changed<RenderLayers>,
    Changed<TraceOverride>,
    Added<NotShadowCaster>,
)>;

/// Keep each entity's marker in step with its mesh, material, visibility and
/// overrides. Steady state costs the change-filtered query only; a frame in
/// which a mesh asset or a material's facts change also scans the
/// candidates for the entities that use them.
#[allow(clippy::too_many_arguments)]
fn sync_traced_meshes<M: TraceMarker>(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    mut material_events: MessageReader<AssetEvent<StandardMaterial>>,
    mut traced: ResMut<TracedMeshes>,
    candidates: Query<Candidate<M>>,
    touched: Query<Entity, (With<Mesh3d>, With<MeshMaterial3d<StandardMaterial>>, Touched)>,
    marked: Query<(), With<M>>,
    mut removed: Removed,
) {
    let traced = &mut *traced;

    // Only meshes and materials some candidate entity uses make a frame
    // dirty: terrain chunks, gizmos and UI quads add meshes all the time.
    let mut dirty_meshes = HashSet::new();
    for event in mesh_events.read() {
        match *event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                if traced.copies.contains(&id) {
                    continue;
                }
                if traced.entries.contains_key(&id) {
                    traced.refresh(id, &mut meshes);
                    dirty_meshes.insert(id);
                } else if traced.waiting_meshes.remove(&id) {
                    dirty_meshes.insert(id);
                }
            }
            AssetEvent::Removed { id } => {
                if traced.copies.remove(&id) {
                    continue;
                }
                traced.waiting_meshes.remove(&id);
                if traced.forget(id) {
                    dirty_meshes.insert(id);
                }
            }
            _ => {}
        }
    }

    let mut dirty_materials = HashSet::new();
    for event in material_events.read() {
        let id = match *event {
            AssetEvent::Added { id }
            | AssetEvent::Modified { id }
            | AssetEvent::LoadedWithDependencies { id }
            | AssetEvent::Removed { id } => id,
            _ => continue,
        };
        if traced.waiting_materials.remove(&id) {
            dirty_materials.insert(id);
        }
        let Some(seen) = traced.materials.get(&id).copied() else {
            continue;
        };
        match materials.get(id).map(MaterialFacts::of) {
            Some(now) if now == seen => {}
            Some(now) => {
                traced.materials.insert(id, now);
                dirty_materials.insert(id);
            }
            None => {
                traced.materials.remove(&id);
                dirty_materials.insert(id);
            }
        }
    }

    let mut todo: HashSet<Entity> = touched.iter().collect();
    todo.extend(removed.material.read());
    todo.extend(removed.mesh.read());
    todo.extend(removed.no_shadow.read());
    todo.extend(removed.layers.read());
    todo.extend(removed.over.read());
    if !dirty_meshes.is_empty() || !dirty_materials.is_empty() {
        traced.full_scans += 1;
        for (entity, mesh, material, ..) in &candidates {
            if dirty_meshes.contains(&mesh.id()) || dirty_materials.contains(&material.id()) {
                todo.insert(entity);
            }
        }
    }

    for entity in todo {
        let Ok((_, mesh, material, visibility, no_shadow, layers, over, current)) = candidates.get(entity)
        else {
            // No mesh or no StandardMaterial any more (or despawned).
            if marked.contains(entity) {
                commands.entity(entity).try_remove::<M>();
            }
            traced.large.remove(&entity);
            continue;
        };
        let material_facts = materials.get(material.id()).map(MaterialFacts::of);
        match material_facts {
            Some(facts) => {
                traced.materials.insert(material.id(), facts);
            }
            None => {
                traced.waiting_materials.insert(material.id());
            }
        }
        let facts = EntityFacts {
            visible: visibility.get(),
            casts_shadows: !no_shadow,
            main_layer: layers.is_none_or(on_main_layer),
            over: over.copied(),
            material: material_facts,
        };
        let target = traced
            .target(&mesh.0, &mut meshes)
            .filter(|(_, triangles)| is_traced(&facts, *triangles));
        match target {
            None => {
                if current.is_some() {
                    commands.entity(entity).try_remove::<M>();
                }
                traced.large.remove(&entity);
            }
            Some((handle, triangles)) => {
                if current.map(M::mesh_id) != Some(handle.id()) {
                    commands.entity(entity).queue_silenced(insert_if_drawn::<M>(handle));
                }
                if triangles > EMISSIVE_TRIANGLE_LIMIT {
                    traced.large.insert(entity);
                } else {
                    traced.large.remove(&entity);
                }
            }
        }
    }

    if traced.counts_changed && dirty_meshes.is_empty() {
        traced.counts_changed = false;
        let counts = traced.counts();
        info!(
            "Ray tracing: {} meshes traced as they are, {} through a copy, skipped {:?}",
            counts.as_is, counts.copied, counts.skipped
        );
    }
}

/// The crash guard, every frame, from the live material: a traced entity
/// over the emissive limit leaves the traced scene the frame its material
/// turns emissive. Runs last so it sees every material change made this
/// frame before the render world extracts. A material not loaded counts as
/// emissive.
fn guard_emissive_limit<M: TraceMarker>(
    mut commands: Commands,
    mut traced: ResMut<TracedMeshes>,
    materials: Res<Assets<StandardMaterial>>,
    entities: Query<&MeshMaterial3d<StandardMaterial>, With<M>>,
) {
    if traced.large.is_empty() {
        return;
    }
    let mut dropped = Vec::new();
    for &entity in &traced.large {
        let Ok(material) = entities.get(entity) else {
            dropped.push(entity);
            continue;
        };
        if materials.get(material.id()).is_none_or(|m| MaterialFacts::of(m).emissive) {
            commands.entity(entity).try_remove::<M>();
            dropped.push(entity);
        }
    }
    for entity in dropped {
        traced.large.remove(&entity);
    }
}

// ============================================================================
// Parts whose maps arrive after them
// ============================================================================

/// Frames between the last map of a waiting material settling and its parts
/// being touched. A map enters `Assets<Image>` (`LoadState::Loaded`) in a
/// main-world frame, is extracted at the end of that frame, and is prepared,
/// with the material that waited on it, in the render frame that follows,
/// which pipelined rendering runs beside the next main-world frame. The
/// touch made two frames later is extracted after the material is ready.
pub const LATE_MAPS_TOUCH_AFTER_FRAMES: u32 = 2;

/// Materials whose texture maps were still loading when they were added or
/// changed, and the parts to put back in every view once the maps are in.
///
/// Under Solari a part draws in a ray-traced view only through that view's
/// deferred prepass: its depth and G-buffer texel are where Solari lights it.
/// Bevy 0.19 specializes and queues every part per view and keeps what it
/// decided until the part changes. A part first specialized for a Solari
/// camera while its material still waited on a map (a library material's
/// 2048-pixel base color map takes seconds to decode and mip on the IO pool)
/// stays out of that camera's deferred prepass after the map arrives: no
/// depth, no G-buffer texel, nothing lit, the sky behind it shows. The same
/// part draws in a camera that starts rendering later (the AI camera), a new
/// part with the same material draws at once, and the part itself draws after
/// any change to it. Raster deferred (SSR) on the same build keeps none of
/// these parts out.
///
/// So each material whose maps were loading when it was added or changed is
/// watched. When the last of them settles (loaded, or failed, so nothing
/// waits on it any more), every part drawn with it has its `MeshMaterial3d`
/// marked changed, [`LATE_MAPS_TOUCH_AFTER_FRAMES`] frames later. Bevy then
/// specializes and queues those parts again in every view, as it does for a
/// part whose material was just swapped. The handle stays the same; the other
/// readers of that change (texture hydration, the trace rules here) find
/// nothing to do.
#[derive(Resource, Default, Debug)]
pub struct LateMaps {
    /// Materials with a map still loading.
    waiting: HashSet<AssetId<StandardMaterial>>,
    /// Materials whose maps have settled, with the frames left before their
    /// parts are touched.
    due: HashMap<AssetId<StandardMaterial>, u32>,
}

impl LateMaps {
    /// `material` was added or changed while one of its maps was loading.
    pub fn wait_for(&mut self, material: AssetId<StandardMaterial>) {
        self.waiting.insert(material);
        self.due.remove(&material);
    }

    /// Every map of `material` has settled: its parts are touched after the
    /// margin. Nothing happens for a material that was not waiting.
    pub fn settled(&mut self, material: AssetId<StandardMaterial>) {
        if self.waiting.remove(&material) {
            self.due.insert(material, LATE_MAPS_TOUCH_AFTER_FRAMES);
        }
    }

    /// `material` is gone.
    pub fn forget(&mut self, material: AssetId<StandardMaterial>) {
        self.waiting.remove(&material);
        self.due.remove(&material);
    }

    /// One frame: the materials whose parts are touched this frame.
    pub fn tick(&mut self) -> HashSet<AssetId<StandardMaterial>> {
        let mut now = HashSet::new();
        self.due.retain(|&material, frames| {
            if *frames == 0 {
                now.insert(material);
                false
            } else {
                *frames -= 1;
                true
            }
        });
        now
    }
}

/// The maps of a `StandardMaterial` the engine loads: base color, normal,
/// metallic-roughness, emissive, occlusion and depth.
fn material_maps(material: &StandardMaterial) -> [Option<AssetId<Image>>; 6] {
    [
        &material.base_color_texture,
        &material.normal_map_texture,
        &material.metallic_roughness_texture,
        &material.emissive_texture,
        &material.occlusion_texture,
        &material.depth_map,
    ]
    .map(|map| map.as_ref().map(Handle::id))
}

/// Whether one of `material`'s maps is still loading, by `loading`.
pub fn maps_loading(material: &StandardMaterial, loading: &dyn Fn(AssetId<Image>) -> bool) -> bool {
    material_maps(material).into_iter().flatten().any(loading)
}

/// Note each material added or changed while a map of it loads, and move
/// each waiting material whose maps have settled to [`LateMaps`]'s due list.
/// A map the asset server does not track (made in code) holds nothing up.
fn watch_late_maps(
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    materials: Res<Assets<StandardMaterial>>,
    server: Res<AssetServer>,
    mut late: ResMut<LateMaps>,
) {
    let loading =
        |map: AssetId<Image>| matches!(server.get_load_state(map), Some(bevy::asset::LoadState::Loading));
    for event in events.read() {
        match *event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                if materials.get(id).is_some_and(|m| maps_loading(m, &loading)) {
                    late.wait_for(id);
                }
            }
            AssetEvent::Removed { id } => late.forget(id),
            _ => {}
        }
    }
    if late.waiting.is_empty() {
        return;
    }
    let settled: Vec<AssetId<StandardMaterial>> = late
        .waiting
        .iter()
        .copied()
        .filter(|&id| materials.get(id).is_none_or(|m| !maps_loading(m, &loading)))
        .collect();
    for id in settled {
        late.settled(id);
    }
}

/// Mark the `MeshMaterial3d` of every part drawn with a material that is due
/// this frame as changed. Scans the parts only in a frame that has one.
fn touch_late_map_parts(
    mut late: ResMut<LateMaps>,
    mut parts: Query<&mut MeshMaterial3d<StandardMaterial>, With<Mesh3d>>,
) {
    if late.due.is_empty() {
        return;
    }
    let now = late.tick();
    if now.is_empty() {
        return;
    }
    for mut material in &mut parts {
        if now.contains(&material.id()) {
            material.set_changed();
        }
    }
}

/// [`LateMaps`] and its systems, while ray tracing is active. Added with
/// Solari only, so the raster path never runs them.
pub fn add_late_map_systems(app: &mut App) {
    app.init_resource::<LateMaps>().add_systems(
        Update,
        (watch_late_maps, touch_late_map_parts).chain().run_if(raytracing_active),
    );
}

#[cfg(test)]
mod late_map_tests {
    use super::*;
    use bevy::asset::AssetPlugin;

    fn material_id(n: u128) -> AssetId<StandardMaterial> {
        AssetId::Uuid { uuid: Uuid::from_u128(n) }
    }

    #[test]
    fn a_waiting_material_is_due_once_its_maps_settle_and_not_before() {
        let mut late = LateMaps::default();
        let a = material_id(1);
        late.wait_for(a);
        for _ in 0..10 {
            assert!(late.tick().is_empty(), "nothing is touched while a map loads");
        }
        late.settled(a);
        for _ in 0..LATE_MAPS_TOUCH_AFTER_FRAMES {
            assert!(late.tick().is_empty(), "the margin passes first");
        }
        assert_eq!(late.tick(), HashSet::from([a]));
        assert!(late.tick().is_empty(), "touched once");
    }

    #[test]
    fn only_a_waiting_material_is_ever_due() {
        let mut late = LateMaps::default();
        late.settled(material_id(2));
        late.wait_for(material_id(3));
        late.forget(material_id(3));
        late.settled(material_id(3));
        for _ in 0..(LATE_MAPS_TOUCH_AFTER_FRAMES + 3) {
            assert!(late.tick().is_empty());
        }
    }

    #[test]
    fn a_material_changed_while_due_waits_again() {
        let mut late = LateMaps::default();
        let a = material_id(4);
        late.wait_for(a);
        late.settled(a);
        late.wait_for(a);
        for _ in 0..(LATE_MAPS_TOUCH_AFTER_FRAMES + 3) {
            assert!(late.tick().is_empty(), "its new maps are still loading");
        }
    }

    #[test]
    fn maps_loading_reads_every_map_the_engine_loads() {
        let map: Handle<Image> = Handle::default();
        let id = map.id();
        let with = |set: fn(&mut StandardMaterial, Handle<Image>)| {
            let mut material = StandardMaterial::default();
            set(&mut material, map.clone());
            material
        };
        let materials = [
            with(|m, h| m.base_color_texture = Some(h)),
            with(|m, h| m.normal_map_texture = Some(h)),
            with(|m, h| m.metallic_roughness_texture = Some(h)),
            with(|m, h| m.emissive_texture = Some(h)),
            with(|m, h| m.occlusion_texture = Some(h)),
            with(|m, h| m.depth_map = Some(h)),
        ];
        for material in &materials {
            assert!(maps_loading(material, &|m| m == id));
            assert!(!maps_loading(material, &|_| false));
        }
        assert!(!maps_loading(&StandardMaterial::default(), &|_| true), "no maps, nothing to wait for");
    }

    #[derive(Resource, Default)]
    struct Touched(Vec<Vec<Entity>>);

    fn record_touched(
        parts: Query<Entity, Changed<MeshMaterial3d<StandardMaterial>>>,
        mut touched: ResMut<Touched>,
    ) {
        let mut now: Vec<Entity> = parts.iter().collect();
        now.sort();
        touched.0.push(now);
    }

    fn app(status: RaytracingStatus) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .insert_resource(RaytracingLaunch::for_status(status))
            .init_resource::<Touched>()
            .add_systems(PostUpdate, record_touched);
        add_late_map_systems(&mut app);
        app
    }

    /// A part drawn with a material whose base color map is loading, and a
    /// part drawn with a plain material. Returns (material, waiting part,
    /// plain part). The map's load never finishes.
    fn scene(app: &mut App) -> (Handle<StandardMaterial>, Entity, Entity) {
        let map: Handle<Image> = app
            .world()
            .resource::<AssetServer>()
            .add_async(std::future::pending::<Result<Image, std::convert::Infallible>>());
        let (textured, plain) = {
            let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            (
                materials.add(StandardMaterial { base_color_texture: Some(map), ..default() }),
                materials.add(StandardMaterial::default()),
            )
        };
        let waiting = app.world_mut().spawn((Mesh3d(Handle::default()), MeshMaterial3d(textured.clone()))).id();
        let other = app.world_mut().spawn((Mesh3d(Handle::default()), MeshMaterial3d(plain))).id();
        (textured, waiting, other)
    }

    #[test]
    fn parts_of_a_material_are_touched_once_after_its_maps_settle() {
        let mut app = app(RaytracingStatus::Active);
        let (textured, waiting, _) = scene(&mut app);
        // The spawn frame marks both parts changed; the map loads from then on.
        app.update();
        let loading_from = app.world().resource::<Touched>().0.len();
        for _ in 0..10 {
            app.update();
        }
        let touched = &app.world().resource::<Touched>().0;
        assert!(touched[loading_from..].iter().all(Vec::is_empty), "touched while its map loads: {touched:?}");
        assert!(app.world().resource::<LateMaps>().waiting.contains(&textured.id()));

        // Its map settles: here the material drops it, the one change that
        // ends the wait in a test (the load itself never finishes).
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&textured).unwrap().base_color_texture =
            None;
        let settled_from = app.world().resource::<Touched>().0.len();
        for _ in 0..(LATE_MAPS_TOUCH_AFTER_FRAMES + 4) {
            app.update();
        }
        let frames = &app.world().resource::<Touched>().0[settled_from..];
        for (n, frame) in frames.iter().enumerate() {
            if n == LATE_MAPS_TOUCH_AFTER_FRAMES as usize {
                assert_eq!(frame, &vec![waiting], "frame {n}: {frames:?}");
            } else {
                assert!(frame.is_empty(), "frame {n}: {frames:?}");
            }
        }
    }

    #[test]
    fn nothing_is_touched_unless_ray_tracing_is_active() {
        let mut app = app(RaytracingStatus::Off);
        let (textured, _, _) = scene(&mut app);
        app.update();
        app.world_mut().resource_mut::<LateMaps>().wait_for(textured.id());
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&textured).unwrap().base_color_texture =
            None;
        let from = app.world().resource::<Touched>().0.len();
        for _ in 0..(LATE_MAPS_TOUCH_AFTER_FRAMES + 4) {
            app.update();
        }
        let touched = &app.world().resource::<Touched>().0[from..];
        assert!(touched.iter().all(Vec::is_empty), "{touched:?}");
    }
}

// ============================================================================
// Solari (only with the `raytracing` feature)
// ============================================================================

/// Everything that names a `bevy_solari` type, and the DLSS camera component.
/// Not compiled until a build with the `raytracing` feature, since the crate
/// is fetched only then.
#[cfg(feature = "raytracing")]
mod solari {
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    use bevy::anti_alias::dlss::{Dlss, DlssRayReconstructionFeature, DlssRayReconstructionSupported};
    use bevy::prelude::*;
    use bevy::render::settings::WgpuFeatures;
    use bevy::solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins};

    impl super::TraceMarker for RaytracingMesh3d {
        fn new(mesh: Handle<Mesh>) -> Self {
            RaytracingMesh3d(mesh)
        }

        fn mesh_id(&self) -> AssetId<Mesh> {
            self.0.id()
        }
    }

    pub(super) fn required_features() -> WgpuFeatures {
        SolariPlugins::required_wgpu_features()
    }

    pub(super) fn add(app: &mut App) {
        app.add_plugins(SolariPlugins);
        super::add_tracing_systems::<RaytracingMesh3d, SolariLighting>(app);
        super::sky::add_systems::<RaytracingMesh3d>(app);
        super::lights::add_systems::<RaytracingMesh3d>(app);
        super::surfaces::add_systems::<RaytracingMesh3d>(app);
        super::add_late_map_systems(app);
        // DLSS Ray Reconstruction denoises (and upscales) Solari's output.
        // `DlssRayReconstructionSupported` exists only when `DlssPlugin` found
        // the driver, the GPU and the DLSS libraries able to run it.
        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        super::add_denoiser_systems::<
            SolariLighting,
            Dlss<DlssRayReconstructionFeature>,
            DlssRayReconstructionSupported,
        >(app, ray_reconstruction);
    }

    /// `Dlss<DlssRayReconstructionFeature>` has no `Default` in Bevy 0.19
    /// (only the super resolution one does), so its fields are set as Bevy's
    /// Solari example sets them: the default (Auto) quality mode, no reset.
    #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
    fn ray_reconstruction() -> Dlss<DlssRayReconstructionFeature> {
        Dlss {
            perf_quality_mode: Default::default(),
            reset: Default::default(),
            _phantom_data: Default::default(),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::RaytracingMesh3d;

        #[test]
        fn copied_feature_list_matches_solari() {
            assert_eq!(super::super::solari_required_features_copy(), super::required_features());
        }

        #[test]
        fn removed_raytracing_mesh_leaves_the_render_world() {
            use super::super::tests::{exercise_untrace, render_test_app};
            let mut app = render_test_app::<RaytracingMesh3d>(super::super::add_untrace_sync::<RaytracingMesh3d>);
            exercise_untrace::<RaytracingMesh3d>(&mut app);
        }

        #[cfg(all(feature = "dlss", not(feature = "force_disable_dlss")))]
        #[test]
        fn denoiser_reported_only_with_both_dlss_features() {
            use super::super::reported_by_device;
            use bevy::anti_alias::dlss::{DlssRayReconstructionSupported, DlssSuperResolutionSupported};
            use bevy::render::renderer::raw_vulkan_init::AdditionalVulkanFeatures;
            let mut features = AdditionalVulkanFeatures::default();
            assert!(!reported_by_device(&features));
            features.insert::<DlssRayReconstructionSupported>();
            assert!(!reported_by_device(&features), "DlssPlugin starts nothing without Super Resolution");
            features.insert::<DlssSuperResolutionSupported>();
            assert!(reported_by_device(&features));
            features.remove::<DlssRayReconstructionSupported>();
            assert!(!reported_by_device(&features), "Super Resolution alone is no denoiser");
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_scene::StudioCamera;
    use bevy::asset::AssetPlugin;
    use bevy::mesh::MeshVertexAttribute;
    use bevy::render::render_resource::VertexFormat;

    // ---- the gate and the note

    #[test]
    fn gate_needs_every_required_feature() {
        let required = solari_required_features_copy();
        assert!(supports(required, required));
        assert!(supports(WgpuFeatures::all(), required));
        assert!(!supports(WgpuFeatures::empty(), required));
        for one in [
            WgpuFeatures::EXPERIMENTAL_RAY_QUERY,
            WgpuFeatures::BUFFER_BINDING_ARRAY,
            WgpuFeatures::TEXTURE_BINDING_ARRAY,
            WgpuFeatures::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
            WgpuFeatures::PARTIALLY_BOUND_BINDING_ARRAY,
        ] {
            assert!(required.contains(one));
            assert!(!supports(required - one, required), "passed without {one}");
            assert!(!supports(WgpuFeatures::all() - one, required), "passed without {one}");
        }
    }

    #[test]
    fn capabilities_name_what_is_missing() {
        let mut caps = GpuCapabilities::default();
        caps.set_features(required_features());
        assert!(caps.ray_tracing);
        assert!(caps.missing.is_empty());
        caps.set_features(required_features() - WgpuFeatures::EXPERIMENTAL_RAY_QUERY);
        assert!(!caps.ray_tracing);
        assert!(caps.missing.contains("EXPERIMENTAL_RAY_QUERY"), "{}", caps.missing);
        assert_eq!(vendor_name(0x10DE), "NVIDIA");
        assert_eq!(vendor_name(0x1234), "unknown");
    }

    fn launch(mode: RayTracingMode, status: RaytracingStatus) -> RaytracingLaunch {
        RaytracingLaunch::new(mode, SwitchSource::Settings, status)
    }

    fn rtx() -> GpuCapabilities {
        let mut caps = GpuCapabilities { adapter: "NVIDIA GeForce RTX 2080 Ti".into(), ..default() };
        caps.set_features(required_features());
        caps
    }

    fn pascal() -> GpuCapabilities {
        let mut caps = GpuCapabilities { adapter: "NVIDIA GeForce GTX 1080 Ti".into(), ..default() };
        caps.set_features(required_features() - WgpuFeatures::EXPERIMENTAL_RAY_QUERY);
        caps
    }

    #[test]
    fn settings_note_says_why() {
        use RayTracingMode::{Auto, Off, On};
        use RaytracingStatus as S;
        let quiet = policy::GuardEvents::default();
        let slow_now = policy::GuardEvents { slow_recorded: true, ..default() };
        let note = |mode, status, caps: Option<&GpuCapabilities>| settings_note(&launch(mode, status), caps, &quiet);
        assert_eq!(note(Auto, S::Active, Some(&rtx())), "Auto: on (NVIDIA GeForce RTX 2080 Ti).");
        assert_eq!(note(On, S::Active, Some(&rtx())), "On (NVIDIA GeForce RTX 2080 Ti).");
        assert_eq!(note(Auto, S::Unsupported, Some(&pascal())), "Auto: off, GPU lacks ray queries.");
        assert_eq!(note(On, S::Unsupported, Some(&pascal())), "On, but GPU lacks ray queries, so it draws without it.");
        let mut no_arrays = rtx();
        no_arrays.set_features(required_features() - WgpuFeatures::BUFFER_BINDING_ARRAY);
        assert_eq!(note(Auto, S::Unsupported, Some(&no_arrays)), "Auto: off, GPU lacks BUFFER_BINDING_ARRAY.");
        assert_eq!(note(Auto, S::Unsupported, None), "Auto: off, the GPU could not be tested.");
        assert_eq!(note(Auto, S::AfterCrash, Some(&rtx())), "Auto: off after a crash at launch; turn On to retry.");
        assert_eq!(note(Auto, S::TooSlow, Some(&rtx())), "Auto: off, too slow on this GPU; turn On to use it anyway.");
        assert!(note(Auto, S::NoGuard, Some(&rtx())).starts_with("Auto: off, the crash guard"));
        let mut cpu = rtx();
        cpu.adapter = "llvmpipe".into();
        assert_eq!(note(Auto, S::SoftwareGpu, Some(&cpu)), "Auto: off, llvmpipe is a software renderer.");
        let no_denoiser = note(Auto, S::NoDenoiser, Some(&rtx()));
        if DLSS_IN_BUILD {
            assert_eq!(
                no_denoiser,
                "Auto: off, NVIDIA GeForce RTX 2080 Ti offers no DLSS Ray Reconstruction to denoise it; turn On to use it anyway."
            );
        } else {
            assert_eq!(
                no_denoiser,
                "Auto: off, this build has no DLSS Ray Reconstruction to denoise it; turn On to use it anyway."
            );
        }
        assert_eq!(note(Off, S::Off, Some(&pascal())), "Off. NVIDIA GeForce GTX 1080 Ti does not support it.");
        assert!(note(Off, S::Off, Some(&rtx())).starts_with("Off. NVIDIA GeForce RTX 2080 Ti supports it"));
        assert_eq!(note(Off, S::Off, None), "Off.");
        assert_eq!(note(Auto, S::NotInBuild, None), "This build was made without ray tracing.");
        // The probed GPU stands in until the renderer's own read arrives.
        let mut probed = launch(Auto, S::Active);
        probed.probed = Some(rtx());
        assert_eq!(settings_note(&probed, None, &quiet), "Auto: on (NVIDIA GeForce RTX 2080 Ti).");
        // Measured too slow this session.
        let slow = settings_note(&probed, None, &slow_now);
        assert!(slow.starts_with("Auto: on (NVIDIA GeForce RTX 2080 Ti) for now") && slow.contains("next launch"), "{slow}");
        assert_eq!(settings_note(&launch(On, S::Active), Some(&rtx()), &slow_now), "On (NVIDIA GeForce RTX 2080 Ti).");
        // The launch variable wins over the setting, and the note says so.
        let mut env = launch(Off, S::Off);
        env.source = SwitchSource::Env;
        assert!(settings_note(&env, None, &quiet).ends_with("Set by EUSTRESS_RAYTRACING, which wins over this setting."));
        // A Space recorded as over the texture limit, and the live fallbacks.
        assert!(note(Auto, S::SpaceTooManyTextures, Some(&rtx())).starts_with("Auto: off for this Space"));
        let heavy = policy::GuardEvents { fallback: Some(FallbackReason::TooManyTextures { distinct: 4_812 }), ..default() };
        let heavy_note = settings_note(&probed, None, &heavy);
        assert!(heavy_note.starts_with("Auto: off now. This Space uses 4812 distinct textures"), "{heavy_note}");
        let off = policy::GuardEvents { fallback: Some(FallbackReason::TurnedOff), ..default() };
        assert_eq!(settings_note(&probed, None, &off), "Off now, for the rest of this session. Auto and On apply at the next launch.");
        let unstarted = policy::GuardEvents { fallback: Some(FallbackReason::NoDenoiser), ..default() };
        assert_eq!(
            settings_note(&probed, None, &unstarted),
            "Auto: off, DLSS Ray Reconstruction did not start on NVIDIA GeForce RTX 2080 Ti, so nothing would denoise it; \
             turn On to use it anyway."
        );
    }

    #[test]
    fn crash_notice_only_on_the_launch_that_found_the_crash() {
        let mut fresh = launch(RayTracingMode::Auto, RaytracingStatus::AfterCrash);
        assert_eq!(launch_notification(&fresh), None, "a crash found on an earlier launch was announced again");
        fresh.crash_found = true;
        assert!(launch_notification(&fresh).unwrap().starts_with("Ray tracing is off for this launch. "));
        let mut forced = launch(RayTracingMode::On, RaytracingStatus::Active);
        forced.crash_found = true;
        assert_eq!(launch_notification(&forced), None);
    }

    #[test]
    fn active_run_condition_follows_the_launch() {
        let mut app = App::new();
        app.add_systems(Update, (|mut n: Local<u32>, mut out: ResMut<Ran>| { *n += 1; out.0 = *n; }).run_if(raytracing_active));
        app.init_resource::<Ran>();
        app.update();
        assert_eq!(app.world().resource::<Ran>().0, 0, "ran with no launch decision");
        app.insert_resource(launch(RayTracingMode::Auto, RaytracingStatus::TooSlow));
        app.update();
        assert_eq!(app.world().resource::<Ran>().0, 0, "ran with ray tracing off");
        app.insert_resource(launch(RayTracingMode::Auto, RaytracingStatus::NoDenoiser));
        app.update();
        assert_eq!(app.world().resource::<Ran>().0, 0, "ran with no denoiser");
        app.insert_resource(launch(RayTracingMode::Auto, RaytracingStatus::Active));
        app.update();
        assert_eq!(app.world().resource::<Ran>().0, 1);
        app.insert_resource(RasterFallback { reason: FallbackReason::TurnedOff });
        app.update();
        assert_eq!(app.world().resource::<Ran>().0, 1, "ran after a live fallback");
    }

    #[derive(Resource, Default)]
    struct Ran(u32);

    #[test]
    fn dlss_project_id_is_ours() {
        assert_eq!(DLSS_PROJECT_ID.get_version_num(), 4, "not a random (v4) UUID");
        assert_eq!(DLSS_PROJECT_ID.to_string(), "e197ed2c-5094-4335-a75e-35f4d7da832d");
        assert_ne!(DLSS_PROJECT_ID.to_string(), "5417916c-0291-4e3f-8f65-326c1858ab96", "Bevy's example id");
        let mut app = App::new();
        insert_dlss_project_id(&mut app);
    }

    #[derive(Component, Default)]
    struct Lit;

    #[derive(Component, Clone)]
    struct Denoiser;

    #[derive(Resource)]
    struct DenoiserSupported;

    #[test]
    fn denoiser_only_on_lit_cameras_and_only_when_supported() {
        let mut app = App::new();
        add_denoiser_systems::<Lit, Denoiser, DenoiserSupported>(&mut app, || Denoiser);
        let lit = app.world_mut().spawn((Camera3d::default(), Lit)).id();
        let raster = app.world_mut().spawn(Camera3d::default()).id();
        app.update();
        assert!(app.world().get::<Denoiser>(lit).is_none(), "added without support");
        app.insert_resource(DenoiserSupported);
        app.update();
        assert!(app.world().get::<Denoiser>(lit).is_some());
        assert!(app.world().get::<Denoiser>(raster).is_none(), "a raster camera got the denoiser");
        let later = app.world_mut().spawn((Camera3d::default(), Lit)).id();
        app.update();
        assert!(app.world().get::<Denoiser>(later).is_some());
    }

    #[test]
    fn note_follows_the_gpu_and_the_guards() {
        let mut app = App::new();
        let mut decided = launch(RayTracingMode::Auto, RaytracingStatus::Active);
        decided.probed = Some(rtx());
        app.insert_resource(decided)
            .init_resource::<RaytracingNote>()
            .init_resource::<policy::GuardEvents>()
            .add_systems(Update, update_note);
        app.update();
        assert_eq!(app.world().resource::<RaytracingNote>().0, "Auto: on (NVIDIA GeForce RTX 2080 Ti).");
        let mut renamed = rtx();
        renamed.adapter = "NVIDIA GeForce RTX 2080 Ti Rev. A".into();
        app.insert_resource(renamed);
        app.update();
        assert_eq!(app.world().resource::<RaytracingNote>().0, "Auto: on (NVIDIA GeForce RTX 2080 Ti Rev. A).");
        app.world_mut().resource_mut::<policy::GuardEvents>().slow_recorded = true;
        app.update();
        assert!(app.world().resource::<RaytracingNote>().0.contains("too slow"));
    }

    // ---- mesh conformance

    /// A unit quad: two triangles, `U16` indices, positions, normals, UVs.
    fn quad() -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_UV_0,
                vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            )
            .with_inserted_indices(Indices::U16(vec![0, 1, 2, 0, 2, 3]))
    }

    fn ids(mesh: &Mesh) -> Vec<MeshVertexAttribute> {
        mesh.attributes().map(|(a, _)| *a).collect()
    }

    /// Solari's own test (`is_mesh_raytracing_compatible`), restated.
    fn solari_would_trace(mesh: &Mesh) -> bool {
        let layout = mesh.attributes().map(|(a, _)| a.id).eq([
            Mesh::ATTRIBUTE_POSITION.id,
            Mesh::ATTRIBUTE_NORMAL.id,
            Mesh::ATTRIBUTE_UV_0.id,
            Mesh::ATTRIBUTE_TANGENT.id,
        ]);
        mesh.enable_raytracing
            && mesh.primitive_topology() == PrimitiveTopology::TriangleList
            && layout
            && matches!(mesh.indices(), Some(Indices::U32(_)))
    }

    #[test]
    fn copy_gets_tangents_and_wide_indices_and_the_source_is_untouched() {
        let source = quad();
        let before = source.clone();
        let Conformance::Copy(copy) = conform(&source) else { panic!("expected a copy") };
        assert!(solari_would_trace(&copy));
        assert_eq!(source, before, "the raster mesh changed");
        assert!(source.attribute(Mesh::ATTRIBUTE_TANGENT).is_none());
        let Some(Indices::U32(wide)) = copy.indices() else { panic!("not U32") };
        assert_eq!(wide, &vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(copy.attribute(Mesh::ATTRIBUTE_POSITION), source.attribute(Mesh::ATTRIBUTE_POSITION));
        assert_eq!(copy.attribute(Mesh::ATTRIBUTE_NORMAL), source.attribute(Mesh::ATTRIBUTE_NORMAL));
        assert_eq!(copy.attribute(Mesh::ATTRIBUTE_UV_0), source.attribute(Mesh::ATTRIBUTE_UV_0));
        let Some(VertexAttributeValues::Float32x4(t)) = copy.attribute(Mesh::ATTRIBUTE_TANGENT) else {
            panic!("no tangents")
        };
        // A flat quad with U along +X: mikktspace gives +X tangents.
        for tangent in t {
            assert!((tangent[0] - 1.0).abs() < 1e-4 && tangent[1].abs() < 1e-4, "{tangent:?}");
        }
    }

    #[test]
    fn traceable_mesh_is_used_as_it_is() {
        let mut mesh = quad();
        mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
        mesh.generate_tangents().unwrap();
        assert!(solari_would_trace(&mesh));
        assert!(matches!(conform(&mesh), Conformance::AsIs { triangles: 2 }));
        mesh.enable_raytracing = false;
        let Conformance::Copy(copy) = conform(&mesh) else { panic!("expected a copy") };
        assert!(solari_would_trace(&copy));
    }

    #[test]
    fn missing_uvs_become_zeros_and_missing_indices_are_made() {
        let mut mesh = quad();
        mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0);
        let Conformance::Copy(copy) = conform(&mesh) else { panic!("expected a copy") };
        assert!(solari_would_trace(&copy));
        assert_eq!(
            copy.attribute(Mesh::ATTRIBUTE_UV_0),
            Some(&VertexAttributeValues::Float32x2(vec![[0.0, 0.0]; 4]))
        );

        let unindexed = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        let Conformance::Copy(copy) = conform(&unindexed) else { panic!("expected a copy") };
        assert_eq!(copy.indices(), Some(&Indices::U32(vec![0, 1, 2])));
        assert!(solari_would_trace(&copy));
    }

    #[test]
    fn raster_only_attributes_skip_the_mesh() {
        let custom = MeshVertexAttribute::new("Vertex_Custom", 988_540_917, VertexFormat::Float32);
        let cases: Vec<(&str, Mesh)> = vec![
            ("color", quad().with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0, 0.0, 0.0, 1.0]; 4])),
            ("uv1", quad().with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0, 0.0]; 4])),
            ("weights", quad().with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[1.0, 0.0, 0.0, 0.0]; 4])),
            ("joints", quad().with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(vec![[0, 0, 0, 0]; 4]))),
            ("custom", quad().with_inserted_attribute(custom, vec![0.5f32; 4])),
        ];
        for (name, mesh) in cases {
            let before = ids(&mesh);
            assert!(
                matches!(conform(&mesh), Conformance::Skip(SkipReason::RasterOnlyAttribute)),
                "{name} was not skipped"
            );
            assert_eq!(ids(&mesh), before, "{name}: the raster mesh lost an attribute");
        }
    }

    #[test]
    fn unusable_meshes_are_skipped_with_a_reason() {
        let lines = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        assert!(matches!(conform(&lines), Conformance::Skip(SkipReason::NotTriangleList)));

        let mut no_normals = quad();
        no_normals.remove_attribute(Mesh::ATTRIBUTE_NORMAL);
        assert!(matches!(conform(&no_normals), Conformance::Skip(SkipReason::MissingPositionOrNormal)));

        let mut past_the_end = quad();
        past_the_end.insert_indices(Indices::U16(vec![0, 1, 9]));
        assert!(matches!(conform(&past_the_end), Conformance::Skip(SkipReason::BadIndices)));

        let mut not_triangles = quad();
        not_triangles.insert_indices(Indices::U16(vec![0, 1, 2, 3]));
        assert!(matches!(conform(&not_triangles), Conformance::Skip(SkipReason::BadIndices)));

        let mut short_uvs = quad();
        short_uvs.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; 3]);
        assert!(matches!(conform(&short_uvs), Conformance::Skip(SkipReason::UnexpectedFormat)));

        // Bevy checks a format against its attribute on insert, so a mesh can
        // carry UV_0's id in another format only through an attribute declared
        // that way (a custom loader can do it).
        let packed_uv = MeshVertexAttribute::new("Vertex_Uv", 2, VertexFormat::Unorm16x2);
        assert_eq!(packed_uv.id, Mesh::ATTRIBUTE_UV_0.id);
        let mut packed_uvs = quad();
        packed_uvs.insert_attribute(packed_uv, VertexAttributeValues::Unorm16x2(vec![[0, 0]; 4]));
        assert!(matches!(conform(&packed_uvs), Conformance::Skip(SkipReason::UnexpectedFormat)));
    }

    // ---- the rules per entity

    fn lit_opaque() -> EntityFacts {
        EntityFacts {
            visible: true,
            casts_shadows: true,
            main_layer: true,
            over: None,
            material: Some(MaterialFacts { opaque: true, lit: true, emissive: false }),
        }
    }

    #[test]
    fn entity_rules() {
        assert!(is_traced(&lit_opaque(), 12));
        assert!(!is_traced(&EntityFacts { visible: false, ..lit_opaque() }, 12));
        assert!(!is_traced(&EntityFacts { casts_shadows: false, ..lit_opaque() }, 12));
        assert!(!is_traced(&EntityFacts { main_layer: false, ..lit_opaque() }, 12));
        assert!(!is_traced(&EntityFacts { material: None, ..lit_opaque() }, 12));
        let glass = MaterialFacts { opaque: false, lit: true, emissive: false };
        assert!(!is_traced(&EntityFacts { material: Some(glass), ..lit_opaque() }, 12));
        let unlit = MaterialFacts { opaque: true, lit: false, emissive: false };
        assert!(!is_traced(&EntityFacts { material: Some(unlit), ..lit_opaque() }, 12));
        assert!(!is_traced(&EntityFacts { over: Some(TraceOverride::Never), ..lit_opaque() }, 12));
        // A light proxy: emissive, casts no raster shadow, unlit.
        let bulb = MaterialFacts { opaque: true, lit: false, emissive: true };
        let proxy = EntityFacts { casts_shadows: false, material: Some(bulb), over: Some(TraceOverride::Always), ..lit_opaque() };
        assert!(is_traced(&proxy, 80));
        assert!(!is_traced(&EntityFacts { visible: false, ..proxy }, 80));
    }

    #[test]
    fn emissive_limit_matches_solari_and_nothing_lifts_it() {
        let emissive = EntityFacts {
            material: Some(MaterialFacts { opaque: true, lit: true, emissive: true }),
            ..lit_opaque()
        };
        assert!(is_traced(&emissive, 65_535));
        assert!(!is_traced(&emissive, 65_536));
        assert!(!is_traced(&EntityFacts { over: Some(TraceOverride::Always), ..emissive }, 65_536));
        assert!(is_traced(&lit_opaque(), 65_536), "a big mesh that is not emissive is traced");
        let mut m = StandardMaterial::default();
        assert!(!MaterialFacts::of(&m).emissive);
        m.emissive = LinearRgba::new(0.0, 0.0, 0.001, 1.0);
        assert!(MaterialFacts::of(&m).emissive);
        m.emissive = LinearRgba::new(0.0, 0.0, 0.0, 1.0);
        assert!(!MaterialFacts::of(&m).emissive, "alpha alone is not light");
    }

    // ---- the systems, in a real app

    #[derive(Component)]
    struct TestTraced(Handle<Mesh>);

    impl TraceMarker for TestTraced {
        fn new(mesh: Handle<Mesh>) -> Self {
            Self(mesh)
        }
        fn mesh_id(&self) -> AssetId<Mesh> {
            self.0.id()
        }
    }

    #[derive(Component, Default)]
    struct TestLighting;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>();
        add_tracing_systems::<TestTraced, TestLighting>(&mut app);
        app
    }

    fn add_mesh(app: &mut App, mesh: Mesh) -> Handle<Mesh> {
        app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh)
    }

    fn add_material(app: &mut App, material: StandardMaterial) -> Handle<StandardMaterial> {
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(material)
    }

    fn part(app: &mut App, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>) -> Entity {
        app.world_mut()
            .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), InheritedVisibility::VISIBLE))
            .id()
    }

    fn traced_mesh(app: &App, entity: Entity) -> Option<AssetId<Mesh>> {
        app.world().get::<TestTraced>(entity).map(|t| t.0.id())
    }

    #[test]
    fn shared_mesh_is_converted_once() {
        let mut app = app();
        let mesh = add_mesh(&mut app, quad());
        let material = add_material(&mut app, StandardMaterial::default());
        let a = part(&mut app, &mesh, &material);
        let b = part(&mut app, &mesh, &material);
        app.update();
        app.update();
        let copy = app.world().resource::<TracedMeshes>().copy_of(mesh.id()).expect("no copy");
        assert_eq!(traced_mesh(&app, a), Some(copy));
        assert_eq!(traced_mesh(&app, b), Some(copy));
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 2, "raster mesh plus one copy");
        let c = part(&mut app, &mesh, &material);
        app.update();
        assert_eq!(traced_mesh(&app, c), Some(copy));
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 2, "a third part made another copy");
        let counts = app.world().resource::<TracedMeshes>().counts();
        assert_eq!((counts.as_is, counts.copied), (0, 1));
        // The raster mesh the parts draw is still the original.
        let raster = app.world().resource::<Assets<Mesh>>().get(&mesh).unwrap();
        assert!(matches!(raster.indices(), Some(Indices::U16(_))));
        assert!(raster.attribute(Mesh::ATTRIBUTE_TANGENT).is_none());
    }

    #[test]
    fn mesh_that_loads_later_is_traced_when_it_arrives() {
        let mut app = app();
        let handle = app.world().resource::<Assets<Mesh>>().reserve_handle();
        let material = add_material(&mut app, StandardMaterial::default());
        let e = part(&mut app, &handle, &material);
        app.update();
        assert_eq!(traced_mesh(&app, e), None);
        app.world_mut().resource_mut::<Assets<Mesh>>().insert(handle.id(), quad()).unwrap();
        app.update();
        app.update();
        let copy = app.world().resource::<TracedMeshes>().copy_of(handle.id());
        assert!(copy.is_some());
        assert_eq!(traced_mesh(&app, e), copy);
    }

    #[test]
    fn edited_raster_mesh_rewrites_its_copy_in_place() {
        let mut app = app();
        let mesh = add_mesh(&mut app, quad());
        let material = add_material(&mut app, StandardMaterial::default());
        let e = part(&mut app, &mesh, &material);
        app.update();
        app.update();
        let copy = app.world().resource::<TracedMeshes>().copy_of(mesh.id()).unwrap();
        let mut moved = quad();
        moved.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [6.0, 1.0, 0.0], [5.0, 1.0, 0.0]]);
        app.world_mut().resource_mut::<Assets<Mesh>>().insert(mesh.id(), moved).unwrap();
        app.update();
        app.update();
        assert_eq!(app.world().resource::<TracedMeshes>().copy_of(mesh.id()), Some(copy), "copy handle changed");
        assert_eq!(traced_mesh(&app, e), Some(copy));
        let meshes = app.world().resource::<Assets<Mesh>>();
        let Some(VertexAttributeValues::Float32x3(p)) = meshes.get(copy).unwrap().attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("no positions")
        };
        assert_eq!(p[0], [5.0, 0.0, 0.0]);
        assert_eq!(meshes.len(), 2);
    }

    #[test]
    fn skipped_mesh_is_counted_and_not_marked() {
        let mut app = app();
        let colored = quad().with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0, 1.0, 1.0, 1.0]; 4]);
        let mesh = add_mesh(&mut app, colored);
        let material = add_material(&mut app, StandardMaterial::default());
        let a = part(&mut app, &mesh, &material);
        let b = part(&mut app, &mesh, &material);
        app.update();
        assert_eq!(traced_mesh(&app, a), None);
        assert_eq!(traced_mesh(&app, b), None);
        let counts = app.world().resource::<TracedMeshes>().counts();
        assert_eq!(counts.skipped.get(&SkipReason::RasterOnlyAttribute), Some(&1));
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1, "a copy was made for a skipped mesh");
    }

    #[test]
    fn entity_rules_follow_changes() {
        let mut app = app();
        let mesh = add_mesh(&mut app, quad());
        let material = add_material(&mut app, StandardMaterial::default());
        let glass = add_material(&mut app, StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() });
        let e = part(&mut app, &mesh, &material);
        app.update();
        assert!(traced_mesh(&app, e).is_some());

        app.world_mut().entity_mut(e).insert(InheritedVisibility::HIDDEN);
        app.update();
        assert!(traced_mesh(&app, e).is_none(), "hidden part still traced");
        app.world_mut().entity_mut(e).insert(InheritedVisibility::VISIBLE);
        app.update();
        assert!(traced_mesh(&app, e).is_some());

        app.world_mut().entity_mut(e).insert(NotShadowCaster);
        app.update();
        assert!(traced_mesh(&app, e).is_none(), "NotShadowCaster part still traced");
        app.world_mut().entity_mut(e).remove::<NotShadowCaster>();
        app.update();
        assert!(traced_mesh(&app, e).is_some());

        app.world_mut().entity_mut(e).insert(MeshMaterial3d(glass));
        app.update();
        assert!(traced_mesh(&app, e).is_none(), "glass still traced");
        app.world_mut().entity_mut(e).insert(MeshMaterial3d(material.clone()));
        app.update();
        assert!(traced_mesh(&app, e).is_some());

        app.world_mut().entity_mut(e).insert(RenderLayers::layer(31));
        app.update();
        assert!(traced_mesh(&app, e).is_none(), "overlay layer still traced");
        app.world_mut().entity_mut(e).remove::<RenderLayers>();
        app.update();
        assert!(traced_mesh(&app, e).is_some());

        app.world_mut().entity_mut(e).insert(TraceOverride::Never);
        app.update();
        assert!(traced_mesh(&app, e).is_none());
        app.world_mut().entity_mut(e).remove::<TraceOverride>();
        app.update();
        assert!(traced_mesh(&app, e).is_some());

        // A material swap away from StandardMaterial (as terrain chunks do).
        app.world_mut().entity_mut(e).remove::<MeshMaterial3d<StandardMaterial>>();
        app.update();
        assert!(traced_mesh(&app, e).is_none(), "marker outlived the StandardMaterial");
    }

    #[test]
    fn material_edit_reaches_every_part_using_it() {
        let mut app = app();
        let mesh = add_mesh(&mut app, quad());
        let material = add_material(&mut app, StandardMaterial::default());
        let a = part(&mut app, &mesh, &material);
        let b = part(&mut app, &mesh, &material);
        app.update();
        app.update();
        assert!(traced_mesh(&app, a).is_some() && traced_mesh(&app, b).is_some());
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&material).unwrap().unlit = true;
        app.update();
        assert!(traced_mesh(&app, a).is_none() && traced_mesh(&app, b).is_none());
    }

    #[test]
    fn unrelated_assets_cost_no_scan() {
        let mut app = app();
        let mesh = add_mesh(&mut app, quad());
        let material = add_material(&mut app, StandardMaterial::default());
        part(&mut app, &mesh, &material);
        app.update();
        app.update();
        let before = app.world().resource::<TracedMeshes>().full_scans();
        // Terrain chunks, gizmos and UI quads add meshes and materials all
        // the time; none of these is used by a part.
        for _ in 0..10 {
            add_mesh(&mut app, quad());
        }
        let other = add_material(&mut app, StandardMaterial::default());
        app.update();
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&other).unwrap().unlit = true;
        app.update();
        // An edit to the part's own material that changes nothing the rules read.
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&material).unwrap().perceptual_roughness = 0.9;
        app.update();
        assert_eq!(app.world().resource::<TracedMeshes>().full_scans(), before);
        // One that does.
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&material).unwrap().unlit = true;
        app.update();
        assert_eq!(app.world().resource::<TracedMeshes>().full_scans(), before + 1);
    }

    #[test]
    fn insert_never_fabricates_a_standard_material() {
        let mut app = app();
        let mesh = add_mesh(&mut app, quad());
        let e = app.world_mut().spawn((Mesh3d(mesh.clone()), InheritedVisibility::VISIBLE)).id();
        insert_if_drawn::<TestTraced>(mesh.clone())(app.world_mut().entity_mut(e));
        assert!(app.world().get::<TestTraced>(e).is_none());
        let material = add_material(&mut app, StandardMaterial::default());
        app.world_mut().entity_mut(e).insert(MeshMaterial3d(material));
        insert_if_drawn::<TestTraced>(mesh)(app.world_mut().entity_mut(e));
        assert!(app.world().get::<TestTraced>(e).is_some());
    }

    /// 65,536 triangles, one past Solari's emissive limit.
    fn big_mesh() -> Mesh {
        let triangles = EMISSIVE_TRIANGLE_LIMIT as usize + 1;
        let mut positions = Vec::with_capacity(triangles * 3);
        for t in 0..triangles {
            let x = t as f32;
            positions.extend([[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x, 1.0, 0.0]]);
        }
        let n = positions.len();
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; n])
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0, 0.0, 0.0, 1.0]; n])
            .with_inserted_indices(Indices::U32((0..n as u32).collect()))
    }

    #[test]
    fn big_mesh_leaves_the_traced_scene_the_frame_it_turns_emissive() {
        let mut app = app();
        let mesh = add_mesh(&mut app, big_mesh());
        let material = add_material(&mut app, StandardMaterial::default());
        let e = part(&mut app, &mesh, &material);
        app.update();
        assert_eq!(traced_mesh(&app, e), Some(mesh.id()), "traceable big mesh is traced as it is");
        // Turned emissive in a system that runs after the asset events are
        // sent this frame: only the per-frame guard sees it before extraction.
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&material).unwrap().emissive =
            LinearRgba::rgb(4.0, 4.0, 4.0);
        app.world_mut().run_schedule(Last);
        assert_eq!(traced_mesh(&app, e), None, "emissive mesh over the limit still traced");
        app.update();
        assert_eq!(traced_mesh(&app, e), None);
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&material).unwrap().emissive =
            LinearRgba::BLACK;
        app.update();
        app.update();
        assert_eq!(traced_mesh(&app, e), Some(mesh.id()), "not traced again once it is dark");
    }

    // ---- cameras

    #[test]
    fn cameras_get_what_solari_needs() {
        use eustress_common::plugins::lighting_plugin::SkyMode;
        let mut app = app();
        let sky = app
            .world_mut()
            .spawn((Camera3d::default(), StudioCamera, SkyCamera { mode: SkyMode::Atmosphere }, Msaa::Sample4))
            .id();
        let ai = app.world_mut().spawn((Camera3d::default(), StudioCamera, NoAtmosphere)).id();
        let overlay = app.world_mut().spawn((Camera3d::default(), NoAtmosphere)).id();
        app.update();
        let world = app.world();
        assert!(world.get::<TestLighting>(sky).is_some());
        assert_eq!(world.get::<Msaa>(sky), Some(&Msaa::Off));
        let usages = world.get::<CameraMainTextureUsages>(sky).expect("no main texture usages").0;
        assert!(usages.contains(TextureUsages::STORAGE_BINDING));
        assert!(usages.contains(CameraMainTextureUsages::default().0), "lost the default usages");
        // The capture camera and the overlay are not lit by Solari. (The
        // capture camera's DeferredPrepass is sky.rs's rule, tested there.)
        assert!(world.get::<TestLighting>(ai).is_none());
        assert!(world.get::<TestLighting>(overlay).is_none());
        // Something asks for MSAA again later.
        app.world_mut().entity_mut(sky).insert(Msaa::Sample4);
        app.update();
        assert_eq!(app.world().get::<Msaa>(sky), Some(&Msaa::Off));
    }

    // ---- leaving the traced scene, in a real render world

    /// A trace marker that, like Solari's `RaytracingMesh3d`, syncs its
    /// entity to the render world.
    #[derive(Component, Clone)]
    #[require(bevy::render::sync_world::SyncToRenderWorld)]
    struct SyncedTrace(Handle<Mesh>);

    impl TraceMarker for SyncedTrace {
        fn new(mesh: Handle<Mesh>) -> Self {
            Self(mesh)
        }
        fn mesh_id(&self) -> AssetId<Mesh> {
            self.0.id()
        }
    }

    /// bevy_solari 0.19's `extract_raytracing_scene` (src/scene/extract.rs),
    /// reduced to what matters here: the marker goes onto the render entity
    /// of every main-world entity that has one, and nothing is ever removed.
    fn insert_only_extract<M: TraceMarker + Clone>(
        instances: bevy::render::Extract<Query<(RenderEntity, &M)>>,
        mut commands: Commands,
    ) {
        for (render_entity, marker) in &instances {
            commands.entity(render_entity).insert(marker.clone());
        }
    }

    /// An app with a render world: Bevy's own `ExtractPlugin` (entity sync,
    /// `ExtractSchedule`) and the render schedule that applies the
    /// extraction's commands, plus the insert-only extraction above.
    pub(crate) fn render_test_app<M: TraceMarker + Clone>(setup: fn(&mut App)) -> App {
        use bevy::ecs::schedule::ScheduleLabel;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::render::extract_plugin::ExtractPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>();
        let render_app = app.sub_app_mut(RenderApp);
        // RenderPlugin, not ExtractPlugin, makes the render schedule run.
        render_app.update_schedule = Some(Render.intern());
        // The worst order for the sync: Solari's insert applied first, its
        // removal after. In the engine the two are unordered, so the sync
        // must be right in either; this order is the one that can catch it.
        render_app.add_systems(ExtractSchedule, insert_only_extract::<M>.before(remove_untraced_markers::<M>));
        setup(&mut app);
        app
    }

    pub(crate) fn render_marker<M: TraceMarker>(app: &App, entity: Entity) -> Option<AssetId<Mesh>> {
        let render = app.world().get::<RenderEntity>(entity)?.id();
        app.sub_app(RenderApp).world().get::<M>(render).map(M::mesh_id)
    }

    /// Out, back in, out and in within one frame, replaced, despawned.
    pub(crate) fn exercise_untrace<M: TraceMarker + Clone>(app: &mut App) {
        let mesh = add_mesh(app, quad());
        let other = add_mesh(app, quad());
        let e = app.world_mut().spawn(M::new(mesh.clone())).id();
        app.update();
        assert_eq!(render_marker::<M>(app, e), Some(mesh.id()), "never traced");
        app.world_mut().entity_mut(e).remove::<M>();
        app.update();
        assert_eq!(render_marker::<M>(app, e), None, "a removed marker left the entity traced");
        app.world_mut().entity_mut(e).insert(M::new(mesh.clone()));
        app.update();
        assert_eq!(render_marker::<M>(app, e), Some(mesh.id()), "not traced again");
        app.world_mut().entity_mut(e).remove::<M>();
        app.world_mut().entity_mut(e).insert(M::new(mesh.clone()));
        app.update();
        assert_eq!(render_marker::<M>(app, e), Some(mesh.id()), "out and back in within a frame dropped it");
        app.world_mut().entity_mut(e).insert(M::new(other.clone()));
        app.update();
        assert_eq!(render_marker::<M>(app, e), Some(other.id()), "a replaced marker is not the new one");
        assert!(app.world().resource::<UntracedEntities>().0.is_empty(), "the list was not emptied");
        app.world_mut().entity_mut(e).despawn();
        app.update();
        app.update();
    }

    #[test]
    fn removed_marker_leaves_the_render_world() {
        let mut app = render_test_app::<SyncedTrace>(add_untrace_sync::<SyncedTrace>);
        exercise_untrace::<SyncedTrace>(&mut app);
    }

    #[test]
    fn without_the_sync_a_removed_marker_stays_traced() {
        // The bug the sync fixes, reproduced in the same harness.
        let mut app = render_test_app::<SyncedTrace>(|_| {});
        let mesh = add_mesh(&mut app, quad());
        let e = app.world_mut().spawn(SyncedTrace(mesh.clone())).id();
        app.update();
        app.world_mut().entity_mut(e).remove::<SyncedTrace>();
        app.update();
        assert_eq!(render_marker::<SyncedTrace>(&app, e), Some(mesh.id()));
    }

    #[test]
    fn a_hidden_part_leaves_the_render_world() {
        // The whole path: the rules take the marker off, the sync takes it
        // out of the render world.
        let mut app = render_test_app::<SyncedTrace>(add_tracing_systems::<SyncedTrace, TestLighting>);
        let mesh = add_mesh(&mut app, quad());
        let material = add_material(&mut app, StandardMaterial::default());
        let e = part(&mut app, &mesh, &material);
        app.update();
        app.update();
        let copy = app.world().resource::<TracedMeshes>().copy_of(mesh.id()).expect("no copy");
        assert_eq!(render_marker::<SyncedTrace>(&app, e), Some(copy));
        app.world_mut().entity_mut(e).insert(InheritedVisibility::HIDDEN);
        app.update();
        assert_eq!(render_marker::<SyncedTrace>(&app, e), None, "a hidden part is still traced");
        app.world_mut().entity_mut(e).insert(InheritedVisibility::VISIBLE);
        app.update();
        app.update();
        assert_eq!(render_marker::<SyncedTrace>(&app, e), Some(copy));
    }

    // ---- dark directional lights

    fn extracted_sun(illuminance: f32) -> ExtractedDirectionalLight {
        ExtractedDirectionalLight {
            color: LinearRgba::WHITE,
            illuminance,
            transform: GlobalTransform::default(),
            shadow_maps_enabled: true,
            contact_shadows_enabled: false,
            volumetric: false,
            affects_lightmapped_mesh_diffuse: true,
            shadow_depth_bias: 0.0,
            shadow_normal_bias: 0.0,
            cascade_shadow_config: default(),
            cascades: default(),
            frusta: default(),
            render_layers: default(),
            soft_shadow_size: None,
            occlusion_culling: false,
            sun_disk_angular_size: 0.0,
            sun_disk_intensity: 1.0,
        }
    }

    // ---- the live fallback

    /// An app with a render world, the tracing systems, the fallback systems
    /// and a launch decided as `status`.
    fn fallback_app(status: RaytracingStatus) -> App {
        let mut app = render_test_app::<SyncedTrace>(add_tracing_systems::<SyncedTrace, TestLighting>);
        app.insert_resource(RaytracingLaunch::for_status(status)).init_resource::<policy::GuardEvents>();
        policy::add_fallback_systems(&mut app);
        app
    }

    /// A camera lit by Solari, a traced part, a trace-only proxy and the
    /// traced sky light, all traced in the render world too.
    fn traced_scene(app: &mut App) -> (Entity, Entity, Entity, Entity) {
        use eustress_common::plugins::lighting_plugin::SkyMode;
        let mesh = add_mesh(app, quad());
        let material = add_material(app, StandardMaterial::default());
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), StudioCamera, SkyCamera { mode: SkyMode::Atmosphere }))
            .id();
        let part = part(app, &mesh, &material);
        let proxy = app.world_mut().spawn((SyncedTrace(mesh.clone()), MeshMaterial3d(material.clone()))).id();
        let sky_light = app
            .world_mut()
            .spawn((DirectionalLight::default(), RenderLayers::layer(47), sky::TracedSkyLight))
            .id();
        app.update();
        app.update();
        assert!(app.world().get::<TestLighting>(camera).is_some());
        assert!(render_marker::<SyncedTrace>(app, part).is_some(), "the part was never traced");
        assert!(render_marker::<SyncedTrace>(app, proxy).is_some(), "the proxy was never traced");
        (camera, part, proxy, sky_light)
    }

    fn render_traced_count(app: &mut App) -> usize {
        let world = app.sub_app_mut(RenderApp).world_mut();
        world.query_filtered::<(), With<SyncedTrace>>().iter(world).count()
    }

    fn assert_fallen_back(app: &mut App, camera: Entity, part: Entity, proxy: Entity, sky_light: Entity) {
        let world = app.world();
        assert!(world.get::<TestLighting>(camera).is_none(), "Solari stayed on a camera");
        assert!(world.get::<SyncedTrace>(part).is_none(), "a part stayed traced");
        assert!(world.get::<Mesh3d>(part).is_some(), "the part stopped being drawn");
        assert!(world.get_entity(proxy).is_err(), "a proxy survived the fallback");
        assert!(world.get_entity(sky_light).is_err(), "the traced sky light survived the fallback");
        assert_eq!(render_traced_count(app), 0, "the render world still traces something");
    }

    #[test]
    fn texture_limit_falls_back_live_and_nothing_traces_again() {
        let mut app = fallback_app(RaytracingStatus::Active);
        let dir = std::env::temp_dir().join(format!("eustress_rt_fallback_{}_{}", std::process::id(), policy::now_unix()));
        app.insert_resource(policy::GuardFolder(policy::GuardStore::at(&dir)))
            .insert_resource(crate::space::SpaceRoot(std::path::PathBuf::from("C:/Spaces/Heavy")));
        let (camera, part, proxy, sky_light) = traced_scene(&mut app);
        // One image short of the threshold: still traced.
        let mut materials = Vec::new();
        for n in 0..(policy::TEXTURE_FALLBACK_AT as u128 - 1) {
            let image = Handle::<Image>::from(Uuid::from_u128(10_000 + n));
            materials.push(add_material(&mut app, StandardMaterial { base_color_texture: Some(image), ..default() }));
        }
        app.update();
        assert!(app.world().get_resource::<RasterFallback>().is_none(), "fell back under the threshold");
        assert!(render_marker::<SyncedTrace>(&app, part).is_some());
        // The one that reaches it: the fallback lands before this frame is extracted.
        let image = Handle::<Image>::from(Uuid::from_u128(9_999));
        // Held: a dropped handle would remove the material again this frame.
        let _last = add_material(&mut app, StandardMaterial { base_color_texture: Some(image), ..default() });
        app.update();
        assert_eq!(
            app.world().get_resource::<RasterFallback>().map(|f| f.reason.clone()),
            Some(FallbackReason::TooManyTextures { distinct: policy::TEXTURE_FALLBACK_AT })
        );
        assert_fallen_back(&mut app, camera, part, proxy, sky_light);
        // The Space is recorded for the next launch.
        let records = policy::GuardStore::at(&dir).load();
        let key = policy::space_key(Path::new("C:/Spaces/Heavy"));
        assert_eq!(records.space_verdict(&key).map(|v| v.distinct_textures), Some(policy::TEXTURE_FALLBACK_AT));
        assert!(app.world().resource::<policy::GuardEvents>().fallback.is_some());
        // Nothing traces again: an edit the rules would trace, a new part, a
        // marker put back by hand, a new sky camera.
        app.world_mut().entity_mut(part).insert(InheritedVisibility::VISIBLE);
        let mesh = add_mesh(&mut app, quad());
        let material = add_material(&mut app, StandardMaterial::default());
        let late_part = part_fn(&mut app, &mesh, &material);
        let stray = app.world_mut().spawn((SyncedTrace(mesh.clone()), MeshMaterial3d(material.clone()))).id();
        let late_camera = app
            .world_mut()
            .spawn((Camera3d::default(), StudioCamera, SkyCamera { mode: eustress_common::plugins::lighting_plugin::SkyMode::Atmosphere }))
            .id();
        app.update();
        app.update();
        assert!(
            app.world().resource::<TracedMeshes>().copy_of(mesh.id()).is_none(),
            "the tracing rules still ran after the fallback"
        );
        assert!(app.world().get::<SyncedTrace>(part).is_none(), "re-traced after the fallback");
        assert!(app.world().get::<SyncedTrace>(late_part).is_none(), "a new part was traced after the fallback");
        assert!(app.world().get_entity(stray).is_err(), "a stray proxy survived");
        assert!(app.world().get::<TestLighting>(late_camera).is_none(), "Solari came back on a new camera");
        assert_eq!(render_traced_count(&mut app), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_off_applies_live() {
        let mut app = fallback_app(RaytracingStatus::Active);
        let (camera, part, proxy, sky_light) = traced_scene(&mut app);
        app.world_mut().write_message(RasterFallbackRequest(FallbackReason::TurnedOff));
        app.update();
        assert_eq!(
            app.world().get_resource::<RasterFallback>().map(|f| f.reason.clone()),
            Some(FallbackReason::TurnedOff)
        );
        assert_fallen_back(&mut app, camera, part, proxy, sky_light);
    }

    #[test]
    fn off_does_nothing_to_a_raster_session() {
        let mut app = fallback_app(RaytracingStatus::Off);
        app.world_mut().write_message(RasterFallbackRequest(FallbackReason::TurnedOff));
        app.update();
        assert!(app.world().get_resource::<RasterFallback>().is_none());
    }

    #[test]
    fn auto_without_a_started_denoiser_falls_back_before_the_first_frame() {
        use eustress_common::plugins::lighting_plugin::SkyMode;
        // The device reported Ray Reconstruction (the launch is Active), but
        // DLSS did not start: `DenoiserSupported` stands for Bevy's
        // `DlssRayReconstructionSupported`, and is absent.
        let mut app = fallback_app(RaytracingStatus::Active);
        let launch = app.world().resource::<RaytracingLaunch>().clone();
        let mesh = add_mesh(&mut app, quad());
        let material = add_material(&mut app, StandardMaterial::default());
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), StudioCamera, SkyCamera { mode: SkyMode::Atmosphere }))
            .id();
        let part = part(&mut app, &mesh, &material);
        require_denoiser::<DenoiserSupported>(&mut app, &launch);
        app.update();
        app.update();
        assert_eq!(
            app.world().get_resource::<RasterFallback>().map(|f| f.reason.clone()),
            Some(FallbackReason::NoDenoiser)
        );
        assert!(app.world().get::<TestLighting>(camera).is_none(), "Solari reached a camera with no denoiser");
        assert!(app.world().get::<SyncedTrace>(part).is_none(), "a part was traced with no denoiser");
        assert!(app.world().get::<Mesh3d>(part).is_some(), "the part stopped being drawn");
        assert_eq!(render_traced_count(&mut app), 0, "the render world traces something");
        assert_eq!(
            app.world().resource::<policy::GuardEvents>().fallback,
            Some(FallbackReason::NoDenoiser),
            "the fallback was never announced"
        );
    }

    #[test]
    fn a_started_denoiser_or_on_keeps_tracing() {
        // Auto with the denoiser started, and On with none: both trace.
        for (mode, started) in [(RayTracingMode::Auto, true), (RayTracingMode::On, false)] {
            let mut app = fallback_app(RaytracingStatus::Active);
            let launch = RaytracingLaunch::new(mode, SwitchSource::Settings, RaytracingStatus::Active);
            app.insert_resource(launch.clone());
            if started {
                app.insert_resource(DenoiserSupported);
            }
            require_denoiser::<DenoiserSupported>(&mut app, &launch);
            assert!(app.world().get_resource::<RasterFallback>().is_none(), "{mode:?} fell back");
            // Lights a camera and traces a part, or fails.
            traced_scene(&mut app);
        }
        // A launch that already draws in raster gets no fallback.
        let mut app = fallback_app(RaytracingStatus::NoDenoiser);
        let launch = app.world().resource::<RaytracingLaunch>().clone();
        require_denoiser::<DenoiserSupported>(&mut app, &launch);
        assert!(app.world().get_resource::<RasterFallback>().is_none(), "a raster launch got a fallback");
    }

    #[test]
    fn the_render_world_is_emptied_even_of_late_markers() {
        // A system that runs after the sweep (as a proxy system unordered
        // with it can) marks a new entity in the frame the fallback begins.
        #[derive(Resource)]
        struct LateMesh(Handle<Mesh>);
        let mut app = fallback_app(RaytracingStatus::Active);
        let (_, part, _, _) = traced_scene(&mut app);
        let mesh = add_mesh(&mut app, quad());
        app.insert_resource(LateMesh(mesh));
        app.add_systems(
            Last,
            (|mut commands: Commands, late: Res<LateMesh>, mut done: Local<bool>, fallback: Option<Res<RasterFallback>>| {
                if fallback.is_some() && !*done {
                    *done = true;
                    commands.spawn(SyncedTrace(late.0.clone()));
                }
            })
            .after(RaytracingSystems::Fallback),
        );
        app.world_mut().write_message(RasterFallbackRequest(FallbackReason::TurnedOff));
        app.update();
        assert!(app.world().get::<SyncedTrace>(part).is_none());
        assert_eq!(render_traced_count(&mut app), 0, "a marker set after the sweep reached Solari");
    }

    #[test]
    fn shadow_maps_come_back_after_a_fallback() {
        let mut app = render_test_app::<SyncedTrace>(add_untrace_sync::<SyncedTrace>);
        add_render_rules(app.sub_app_mut(RenderApp));
        let early = app.sub_app_mut(RenderApp).world_mut().spawn(extracted_sun(100_000.0)).id();
        app.update();
        let shadows = |app: &App, e: Entity| app.sub_app(RenderApp).world().get::<ExtractedDirectionalLight>(e).map(|l| l.shadow_maps_enabled);
        assert_eq!(shadows(&app, early), Some(false), "the sun's shadow maps were not suppressed");
        app.insert_resource(RasterFallback { reason: FallbackReason::TurnedOff });
        // The flag reaches the render world at this frame's extraction.
        app.update();
        let late = app.sub_app_mut(RenderApp).world_mut().spawn(extracted_sun(100_000.0)).id();
        app.update();
        assert_eq!(shadows(&app, late), Some(true), "shadow maps stayed off after the fallback");
    }

    #[test]
    fn denoiser_leaves_with_the_lighting() {
        let mut app = App::new();
        add_denoiser_systems::<Lit, Denoiser, DenoiserSupported>(&mut app, || Denoiser);
        app.insert_resource(DenoiserSupported);
        let camera = app.world_mut().spawn((Camera3d::default(), Lit)).id();
        app.update();
        assert!(app.world().get::<Denoiser>(camera).is_some());
        app.world_mut().entity_mut(camera).remove::<Lit>();
        app.update();
        assert!(app.world().get::<Denoiser>(camera).is_none(), "the denoiser outlived the lighting");
    }

    fn part_fn(app: &mut App, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>) -> Entity {
        part(app, mesh, material)
    }

    #[test]
    fn dark_directional_lights_leave_the_render_world() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let sun = world.spawn(extracted_sun(100_000.0)).id();
        let dim = world.spawn(extracted_sun(0.25)).id();
        let set_moon = world.spawn(extracted_sun(0.0)).id();
        let broken = world.spawn(extracted_sun(f32::NAN)).id();
        world.run_system_once(drop_dark_directional_lights).unwrap();
        assert!(world.get::<ExtractedDirectionalLight>(sun).is_some());
        assert!(world.get::<ExtractedDirectionalLight>(dim).is_some(), "a dim light is still light");
        assert!(world.get::<ExtractedDirectionalLight>(set_moon).is_none(), "a zero-lux light stayed");
        assert!(world.get::<ExtractedDirectionalLight>(broken).is_none());
        assert!(is_dark(-1.0) && is_dark(0.0) && !is_dark(f32::MIN_POSITIVE));
    }
}

//! The look of every camera a person sees the world through, the same in
//! Studio and the Player: Studio's editor camera, the avatar camera and the
//! scripted Play camera all carry [`ViewCamera`] and get one image stack.
//!
//! Opt-in by the marker, never "every `Camera3d`": the Slint overlay, the HUD
//! camera and the AI capture camera keep their own settings.
//!
//! ## The stack (feature `view-look`)
//!
//! Filmic tonemapping (TonyMcMapface, which the sky's night tuning is set
//! against), SMAA in place of hardware MSAA, bloom, and the opt-in stages.
//! Every switch is read once per process, so every view camera gets the
//! same set (a camera whose view features differ from another's can fail
//! the shared view layout).
//!
//! | env | default | effect |
//! |-----|---------|--------|
//! | `EUSTRESS_SMAA`            | on   | subpixel-morphological AA |
//! | `EUSTRESS_BLOOM`           | on   | filmic bloom |
//! | `EUSTRESS_BLOOM_INTENSITY` | 0.15 | bloom strength (bevy's `NATURAL`) |
//! | `EUSTRESS_MSAA`            | off  | `2`/`4`/`8` restores hardware MSAA |
//! | `EUSTRESS_GTAO`            | off  | ground-contact ambient occlusion |
//! | `EUSTRESS_AUTO_EXPOSURE`   | off  | histogram exposure adaptation |
//! | `EUSTRESS_CONTACT_SHADOWS` | off  | screen-space contact shadows from the sun |
//!
//! The stack needs bevy's bloom, SMAA and tonemapping lookup tables, so it
//! is behind common's optional `view-look` feature, which Studio and the
//! Player turn on. A build without it (the web Player, a luau-only test
//! build) gives view cameras Reinhard, which needs no lookup table (bevy's
//! default, TonyMcMapface, renders magenta without one), and no post stack.
//!
//! ## Objects in the Space
//!
//! An enabled `BloomEffect` sets every view camera's bloom. The enabled
//! `ColorCorrectionEffect`s and `ColorGradingEffect`s grade the image after
//! tonemapping, as Roblox's do, in a pass of our own ([`ViewGrade`]): the
//! Brightness offset, the Contrast around mid grey, the Saturation around
//! the luma, then the TintColor multiplied in, all in display space.
//! Several combine (offsets add, factors multiply). With none enabled the
//! pass does not run. A `ColorGradingEffect`'s Tonemapper picks the view's
//! tonemapper; "Default" is the engine's own.

use bevy::asset::{embedded_asset, load_embedded_asset};
use bevy::core_pipeline::tonemapping::{tonemapping, Tonemapping};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::query::QueryItem;
use bevy::ecs::world::DeferredWorld;
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::{
    binding_types::{sampler, texture_2d, uniform_buffer},
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, DynamicUniformBuffer, FilterMode, FragmentState, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType,
    SamplerDescriptor, ShaderStages, ShaderType, SpecializedRenderPipeline, SpecializedRenderPipelines,
    TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::sync_component::SyncComponent;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{GpuResourceAppExt, Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::Shader;
use tracing::info;

use crate::classes::{ColorCorrectionEffect, ColorGradingEffect};

/// Marks a camera a person looks through: Studio's editor camera, the avatar
/// camera and the scripted Play camera. They get the view look.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
#[component(on_insert = set_view_tonemapper)]
pub struct ViewCamera;

/// The view's tonemapper, written the moment the marker lands, so the
/// camera's first frame already has it: TonyMcMapface with `view-look`,
/// Reinhard without it. `Camera3d`'s own required default is TonyMcMapface,
/// which renders magenta without the lookup tables `view-look` brings, so
/// an Update system would leave at least the spawn frame magenta. A
/// ColorGradingEffect's Tonemapper changes it afterwards.
fn set_view_tonemapper(mut world: DeferredWorld, ctx: HookContext) {
    #[cfg(feature = "view-look")]
    let wanted = Tonemapping::TonyMcMapface;
    #[cfg(not(feature = "view-look"))]
    let wanted = Tonemapping::Reinhard;
    if let Some(mut tonemapping) = world.get_mut::<Tonemapping>(ctx.entity) {
        if *tonemapping != wanted {
            *tonemapping = wanted;
        }
    } else {
        world.commands().entity(ctx.entity).insert(wanted);
    }
}

// ============================================================================
// Switches, read once
// ============================================================================

/// Read an on/off env flag exactly once. `default_on` is used when unset.
fn flag(key: &'static str, default_on: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !(v.is_empty() || v == "0" || v == "false" || v == "off")
        }
        Err(_) => default_on,
    }
}

macro_rules! switch {
    ($name:ident, $key:literal, $default:expr, $doc:literal) => {
        #[doc = $doc]
        pub fn $name() -> bool {
            static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
            *V.get_or_init(|| flag($key, $default))
        }
    };
}

switch!(smaa_on, "EUSTRESS_SMAA", true, "SMAA on view cameras (`EUSTRESS_SMAA`, default on).");
switch!(bloom_on, "EUSTRESS_BLOOM", true, "Bloom on view cameras (`EUSTRESS_BLOOM`, default on).");
switch!(gtao_on, "EUSTRESS_GTAO", false, "Ground-contact AO (`EUSTRESS_GTAO`, default off).");
switch!(
    auto_exposure_on,
    "EUSTRESS_AUTO_EXPOSURE",
    false,
    "Histogram exposure adaptation (`EUSTRESS_AUTO_EXPOSURE`, default off; the sky's own night adaptation is always on)."
);
switch!(
    contact_shadows_on,
    "EUSTRESS_CONTACT_SHADOWS",
    false,
    "Screen-space contact shadows from the sun (`EUSTRESS_CONTACT_SHADOWS`, default off). The sun's light reads it too: the camera component has nothing to draw unless the light enables it."
);

/// Hardware MSAA samples from `EUSTRESS_MSAA` (2, 4 or 8), or `None` to keep
/// MSAA off (SMAA anti-aliases instead). Any other value is ignored.
pub fn msaa_samples() -> Option<u32> {
    static V: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let raw = std::env::var("EUSTRESS_MSAA").ok()?;
        match raw.trim().parse::<u32>() {
            Ok(n @ (2 | 4 | 8)) => Some(n),
            _ => {
                tracing::warn!("EUSTRESS_MSAA={raw:?} ignored: expected 2, 4 or 8");
                None
            }
        }
    })
}

// ============================================================================
// Plugin
// ============================================================================

/// The view look: the camera stack, bloom objects and the colour grade.
/// Added by `SharedLightingPlugin`, so Studio and the Player share it.
pub struct CameraLookPlugin;

impl Plugin for CameraLookPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<ViewCamera>()
            .add_systems(Update, apply_grade_objects);
        #[cfg(feature = "view-look")]
        {
            // Bevy's post-process plugin does not bring auto-exposure; it
            // is added only when asked for, so the everyday build never
            // pays its per-frame histogram.
            if auto_exposure_on() {
                app.add_plugins(bevy::post_process::auto_exposure::AutoExposurePlugin);
            }
            app.add_systems(Update, look::apply_view_camera_stages.before(apply_grade_objects));
            if bloom_on() {
                app.add_systems(Update, look::apply_bloom_effect.after(look::apply_view_camera_stages));
            }
            info!(
                "camera look: smaa={} bloom={} gtao={} auto_exposure={} contact_shadows={} msaa={}",
                smaa_on(),
                bloom_on(),
                gtao_on(),
                auto_exposure_on(),
                contact_shadows_on(),
                msaa_samples().map_or("off".to_string(), |n| n.to_string()),
            );
        }
        #[cfg(not(feature = "view-look"))]
        info!("camera look: view-look feature off; view cameras get Reinhard and no post stack");

        embedded_asset!(app, "camera_look.wgsl");
        app.add_plugins(ExtractComponentPlugin::<ViewGrade>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_gpu_resource::<SpecializedRenderPipelines<GradePipeline>>()
            .init_gpu_resource::<GradeUniforms>()
            .add_systems(RenderStartup, init_grade_pipeline)
            .add_systems(
                Render,
                (prepare_grade_pipelines, prepare_grade_uniforms).in_set(RenderSystems::Prepare),
            )
            .add_systems(Core3d, grade.after(tonemapping).in_set(Core3dSystems::PostProcess));
    }
}

// ============================================================================
// The camera stack (view-look)
// ============================================================================


#[cfg(feature = "view-look")]
mod look {
    use super::*;
    use crate::classes::BloomEffect;
    use bevy::anti_alias::smaa::Smaa;
    use bevy::pbr::{ContactShadows, ScreenSpaceAmbientOcclusion};
    use bevy::post_process::auto_exposure::AutoExposure;
    use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
    use bevy::render::view::Msaa;

    /// Bloom's strength, `EUSTRESS_BLOOM_INTENSITY` (default: bevy's
    /// `NATURAL`, 0.15). Energy-conserving, so raising it spreads more of
    /// every bright pixel into its glow rather than brightening the frame.
    pub(super) fn bloom() -> Bloom {
        static V: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
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
    /// scales bevy's wide, low-frequency part. A `Threshold` above 0 means
    /// only what is brighter than it glows, which is bevy's additive,
    /// thresholded bloom. With no threshold it stays energy-conserving,
    /// `Intensity` scaling the baseline (Roblox's default 0.4 is the
    /// baseline itself).
    pub(super) fn bloom_from_effect(effect: &BloomEffect) -> Bloom {
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

    /// Attach the stack to a view camera the frame it appears. The
    /// tonemapper is the marker's hook's, set at insert time.
    pub(super) fn apply_view_camera_stages(mut commands: Commands, cameras: Query<Entity, Added<ViewCamera>>) {
        for entity in &cameras {
            let mut ec = commands.entity(entity);
            // SMAA replaces hardware MSAA unless EUSTRESS_MSAA asks for it.
            ec.insert(match msaa_samples() {
                Some(n) => Msaa::from_samples(n),
                None => Msaa::Off,
            });
            if smaa_on() {
                // SMAA needs bevy's `smaa_luts` feature (part of view-look):
                // without it the placeholder LUT fails validation and bevy
                // quits the first frame SMAA runs.
                ec.insert(Smaa::default());
            }
            if bloom_on() {
                ec.insert(bloom());
            }
            if gtao_on() {
                ec.insert(ScreenSpaceAmbientOcclusion::default());
            }
            if auto_exposure_on() {
                ec.insert(AutoExposure::default());
            }
            if contact_shadows_on() {
                ec.insert((bevy::core_pipeline::prepass::DepthPrepass, ContactShadows::default()));
            }
        }
    }

    /// Carry an enabled Bloom object's settings to every view camera, and
    /// restore the baseline when it is disabled or deleted.
    pub(super) fn apply_bloom_effect(
        effects: Query<&BloomEffect>,
        changed: Query<(), Changed<BloomEffect>>,
        mut removed: RemovedComponents<BloomEffect>,
        mut cameras: Query<&mut Bloom, With<ViewCamera>>,
    ) {
        let removed_any = removed.read().count() > 0;
        // A camera that just got its Bloom takes the effect too, asked
        // through the one mutable query (`is_added` does not mark it changed).
        let new_bloom = cameras.iter_mut().any(|b| b.is_added());
        if changed.is_empty() && !removed_any && !new_bloom {
            return;
        }
        let desired = effects.iter().find(|e| e.enabled).map(bloom_from_effect).unwrap_or_else(bloom);
        for mut camera_bloom in cameras.iter_mut() {
            *camera_bloom = desired.clone();
        }
    }
}

// ============================================================================
// The colour grade
// ============================================================================

/// The tonemappers a ColorGradingEffect may choose, in the order the
/// Properties panel offers them. "Default" is the engine's own look.
pub const TONEMAPPERS: [&str; 8] = [
    "Default",
    "AgX",
    "ACES",
    "TonyMcMapface",
    "BlenderFilmic",
    "Reinhard",
    "ReinhardLuminance",
    "None",
];

/// The canonical name for a tonemapper typed in any case, or `None`.
pub fn tonemapper_named(name: &str) -> Option<&'static str> {
    let flat = |s: &str| s.replace([' ', '_', '-'], "").to_ascii_lowercase();
    TONEMAPPERS.iter().copied().find(|t| flat(t) == flat(name))
}

/// The bevy tonemapper for a canonical name ("Default" and anything unknown
/// are the engine's TonyMcMapface).
pub fn tonemapping_for(name: &str) -> Tonemapping {
    match tonemapper_named(name).unwrap_or("Default") {
        "AgX" => Tonemapping::AgX,
        "ACES" => Tonemapping::AcesFitted,
        "BlenderFilmic" => Tonemapping::BlenderFilmic,
        "Reinhard" => Tonemapping::Reinhard,
        "ReinhardLuminance" => Tonemapping::ReinhardLuminance,
        "None" => Tonemapping::None,
        _ => Tonemapping::TonyMcMapface,
    }
}

/// The grade a view camera applies after tonemapping: the enabled
/// ColorCorrectionEffects and ColorGradingEffects, combined. Present only
/// while there is something to do; its absence skips the pass.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct ViewGrade {
    /// Added to every channel, display space.
    pub brightness: f32,
    /// Scales the distance from mid grey (1 is unchanged).
    pub contrast: f32,
    /// Scales the distance from the luma (1 is unchanged, 0 is grey).
    pub saturation: f32,
    /// Multiplied in last.
    pub tint: Vec3,
}

impl ViewGrade {
    /// No change at all.
    pub const IDENTITY: Self = Self { brightness: 0.0, contrast: 1.0, saturation: 1.0, tint: Vec3::ONE };

    /// Combine another effect's grade into this one.
    fn and(self, other: Self) -> Self {
        Self {
            brightness: self.brightness + other.brightness,
            contrast: self.contrast * other.contrast,
            saturation: self.saturation * other.saturation,
            tint: self.tint * other.tint,
        }
    }

    /// Roblox's ColorCorrectionEffect: Contrast and Saturation are centred on
    /// 0 (-1 to 1), Brightness is an offset, TintColor a multiplier.
    pub fn from_correction(e: &ColorCorrectionEffect) -> Self {
        Self {
            brightness: e.brightness.clamp(-1.0, 1.0),
            contrast: (1.0 + e.contrast).max(0.0),
            saturation: (1.0 + e.saturation).max(0.0),
            tint: Vec3::from_array(e.tint_color).max(Vec3::ZERO),
        }
    }

    /// A ColorGradingEffect: Contrast and Saturation are factors (1 is
    /// unchanged), Brightness an offset, TintColor a multiplier.
    pub fn from_grading(e: &ColorGradingEffect) -> Self {
        Self {
            brightness: e.brightness.clamp(-1.0, 1.0),
            contrast: e.contrast.max(0.0),
            saturation: e.saturation.max(0.0),
            tint: Vec3::from_array(e.tint_color).max(Vec3::ZERO),
        }
    }

    fn is_identity(&self) -> bool {
        (self.brightness).abs() < 1e-5
            && (self.contrast - 1.0).abs() < 1e-5
            && (self.saturation - 1.0).abs() < 1e-5
            && (self.tint - Vec3::ONE).abs().max_element() < 1e-5
    }
}

/// The Space's grade: every enabled effect, combined. `None` when there is
/// nothing to change.
pub fn space_grade<'a>(
    corrections: impl IntoIterator<Item = &'a ColorCorrectionEffect>,
    gradings: impl IntoIterator<Item = &'a ColorGradingEffect>,
) -> Option<ViewGrade> {
    let grade = corrections
        .into_iter()
        .filter(|e| e.enabled)
        .map(ViewGrade::from_correction)
        .chain(gradings.into_iter().filter(|e| e.enabled).map(ViewGrade::from_grading))
        .fold(ViewGrade::IDENTITY, ViewGrade::and);
    (!grade.is_identity()).then_some(grade)
}

/// A grade removed from a camera takes its render-world pipeline and uniform
/// offset with it, so nothing stale reaches the pass.
impl SyncComponent for ViewGrade {
    type Target = (Self, GradePipelineId, GradeUniformOffset);
}

impl ExtractComponent for ViewGrade {
    type QueryData = &'static ViewGrade;
    type QueryFilter = With<Camera>;
    type Out = Self;

    fn extract_component(grade: QueryItem<'_, '_, Self::QueryData>) -> Option<Self::Out> {
        Some(*grade)
    }
}

/// Keep every view camera's [`ViewGrade`] (and, with `view-look`, its
/// tonemapper) in step with the Space's grade objects.
fn apply_grade_objects(
    mut commands: Commands,
    corrections: Query<&ColorCorrectionEffect>,
    gradings: Query<&ColorGradingEffect>,
    changed: Query<(), Or<(Changed<ColorCorrectionEffect>, Changed<ColorGradingEffect>)>>,
    mut removed_corrections: RemovedComponents<ColorCorrectionEffect>,
    mut removed_gradings: RemovedComponents<ColorGradingEffect>,
    cameras: Query<(Entity, Option<&ViewGrade>, Ref<ViewCamera>)>,
    #[cfg(feature = "view-look")] mut tonemappers: Query<&mut Tonemapping, With<ViewCamera>>,
) {
    let removed = removed_corrections.read().count() + removed_gradings.read().count() > 0;
    let new_camera = cameras.iter().any(|(_, _, marker)| marker.is_added());
    if changed.is_empty() && !removed && !new_camera {
        return;
    }
    let grade = space_grade(corrections.iter(), gradings.iter());
    for (entity, current, _) in cameras.iter() {
        match grade {
            Some(g) if current != Some(&g) => {
                commands.entity(entity).insert(g);
            }
            None if current.is_some() => {
                commands.entity(entity).remove::<ViewGrade>();
            }
            _ => {}
        }
    }
    #[cfg(feature = "view-look")]
    {
        let wanted = gradings
            .iter()
            .find(|g| g.enabled)
            .map_or(Tonemapping::TonyMcMapface, |g| tonemapping_for(&g.tonemapper));
        for mut tonemapping in tonemappers.iter_mut() {
            if *tonemapping != wanted {
                *tonemapping = wanted;
            }
        }
    }
    if let Some(g) = grade {
        info!(
            "🎨 Colour grade: brightness {:.2}, contrast {:.2}, saturation {:.2}, tint {:.2}/{:.2}/{:.2}",
            g.brightness, g.contrast, g.saturation, g.tint.x, g.tint.y, g.tint.z
        );
    }
}

// ============================================================================
// The grade pass (render world)
// ============================================================================

/// The grade as the shader reads it.
#[derive(ShaderType, Clone, Copy, Default)]
struct GradeUniform {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    unused: f32,
    tint: Vec4,
}

#[derive(Resource)]
struct GradePipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen_shader: FullscreenShader,
    shader: Handle<Shader>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GradePipelineKey {
    target_format: TextureFormat,
}

/// A graded view's specialised pipeline (render world).
#[derive(Component)]
pub struct GradePipelineId(CachedRenderPipelineId);

#[derive(Resource, Default)]
struct GradeUniforms {
    buffer: DynamicUniformBuffer<GradeUniform>,
}

/// A graded view's offset into this frame's grade uniforms (render world).
#[derive(Component)]
pub struct GradeUniformOffset(u32);

fn init_grade_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    asset_server: Res<AssetServer>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "view grade bind group layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<GradeUniform>(true),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        min_filter: FilterMode::Linear,
        mag_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(GradePipeline {
        layout,
        sampler,
        fullscreen_shader: fullscreen_shader.clone(),
        shader: load_embedded_asset!(asset_server.as_ref(), "camera_look.wgsl"),
    });
}

impl SpecializedRenderPipeline for GradePipeline {
    type Key = GradePipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("view grade".into()),
            layout: vec![self.layout.clone()],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                targets: vec![Some(ColorTargetState {
                    format: key.target_format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

fn prepare_grade_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<GradePipeline>>,
    grade_pipeline: Res<GradePipeline>,
    views: Query<(Entity, &ExtractedView), With<ViewGrade>>,
) {
    for (entity, view) in &views {
        let id = pipelines.specialize(
            &pipeline_cache,
            &grade_pipeline,
            GradePipelineKey { target_format: view.target_format },
        );
        commands.entity(entity).insert(GradePipelineId(id));
    }
}

fn prepare_grade_uniforms(
    mut commands: Commands,
    mut uniforms: ResMut<GradeUniforms>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    views: Query<(Entity, &ViewGrade)>,
) {
    uniforms.buffer.clear();
    for (entity, grade) in &views {
        let offset = uniforms.buffer.push(&GradeUniform {
            brightness: grade.brightness,
            contrast: grade.contrast,
            saturation: grade.saturation,
            unused: 0.0,
            tint: grade.tint.extend(1.0),
        });
        commands.entity(entity).insert(GradeUniformOffset(offset));
    }
    uniforms.buffer.write_buffer(&render_device, &render_queue);
}

/// The full-screen grade, after tonemapping.
fn grade(
    view: ViewQuery<(&ViewTarget, Option<&ViewGrade>, Option<&GradePipelineId>, Option<&GradeUniformOffset>)>,
    pipeline_cache: Res<PipelineCache>,
    grade_pipeline: Res<GradePipeline>,
    uniforms: Res<GradeUniforms>,
    mut ctx: RenderContext,
) {
    // A view with no grade skips the pass; the pipeline id and offset from
    // an earlier grade are only trusted while the grade is there, since the
    // offsets are rewritten each frame for graded views alone.
    let (view_target, grade, pipeline_id, offset) = view.into_inner();
    let (Some(_), Some(pipeline_id), Some(offset)) = (grade, pipeline_id, offset) else { return };
    let Some(pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else { return };
    let Some(uniform_binding) = uniforms.buffer.binding() else { return };

    let post_process = view_target.post_process_write();
    let bind_group = ctx.render_device().create_bind_group(
        Some("view grade bind group"),
        &pipeline_cache.get_bind_group_layout(&grade_pipeline.layout),
        &BindGroupEntries::sequential((post_process.source, &grade_pipeline.sampler, uniform_binding)),
    );
    let mut render_pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("view grade"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post_process.destination,
            depth_slice: None,
            resolve_target: None,
            ops: Operations::default(),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    render_pass.set_render_pipeline(pipeline);
    render_pass.set_bind_group(0, &bind_group, &[offset.0]);
    render_pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_enabled_effect_means_no_pass() {
        let off = ColorCorrectionEffect { enabled: false, brightness: 0.5, ..ColorCorrectionEffect::default() };
        assert_eq!(space_grade([&off], []), None);
        // An enabled effect at its defaults changes nothing either.
        assert_eq!(space_grade([&ColorCorrectionEffect::default()], [&ColorGradingEffect::default()]), None);
    }

    #[test]
    fn effects_combine_offsets_add_and_factors_multiply() {
        let a = ColorCorrectionEffect { brightness: 0.1, contrast: 0.5, tint_color: [1.0, 0.5, 1.0], ..Default::default() };
        let b = ColorGradingEffect { brightness: 0.2, saturation: 0.5, tint_color: [0.5, 1.0, 1.0], ..Default::default() };
        let g = space_grade([&a], [&b]).expect("a grade");
        assert!((g.brightness - 0.3).abs() < 1e-6);
        assert!((g.contrast - 1.5).abs() < 1e-6, "Roblox's 0-centred contrast");
        assert!((g.saturation - 0.5).abs() < 1e-6, "a grading factor");
        assert!((g.tint - Vec3::new(0.5, 0.5, 1.0)).abs().max_element() < 1e-6);
    }

    #[test]
    fn tonemappers_are_read_by_name_and_default_to_the_engines() {
        assert_eq!(tonemapper_named("agx"), Some("AgX"));
        assert_eq!(tonemapping_for("Default"), Tonemapping::TonyMcMapface);
        assert_eq!(tonemapping_for("something else"), Tonemapping::TonyMcMapface);
        assert_eq!(tonemapping_for("aces"), Tonemapping::AcesFitted);
        assert_eq!(tonemapping_for("None"), Tonemapping::None);
    }

    #[test]
    fn a_view_camera_has_its_tonemapper_from_the_moment_it_spawns() {
        let mut world = World::new();
        let camera = world.spawn((Tonemapping::TonyMcMapface, ViewCamera)).id();
        #[cfg(feature = "view-look")]
        let wanted = Tonemapping::TonyMcMapface;
        #[cfg(not(feature = "view-look"))]
        let wanted = Tonemapping::Reinhard;
        assert_eq!(world.get::<Tonemapping>(camera), Some(&wanted), "set by the hook, no system run");
    }

    #[test]
    fn the_uniform_is_std140_sized() {
        assert_eq!(GradeUniform::min_size().get(), 32);
    }
}

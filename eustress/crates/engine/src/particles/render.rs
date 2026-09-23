//! Instanced renderer for particle simulations.
//!
//! One `Transparent3d` item per `ParticleSimulation`: its particles live in
//! a storage buffer (re-uploaded only when the simulation's cloud revision
//! changes) and one `draw(0..6, 0..count)` call expands each into a
//! camera-facing quad that the shader turns into a depth-writing sphere.
//! Opaque spheres with true depth need no sorting and no blending, which
//! matters because Bevy sorts transparent items per draw, not per particle.
//!
//! Structure follows [`crate::billboard_pipeline`] (the engine's other
//! custom pipeline): shader baked in with `include_str!`, own view bind
//! group, pipeline keyed on each view's MSAA and attachment format (a
//! pipeline built for the wrong format is a fatal wgpu validation error),
//! and views filtered by `RenderLayers` so the Slint overlay camera never
//! draws particles.

use std::sync::Arc;

use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::core_3d::{Transparent3d, TransparentSortingInfo3d};
use bevy::ecs::query::ROQueryItem;
use bevy::ecs::system::{lifetimeless::*, SystemParamItem};
use bevy::math::Mat4;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use bevy::render::render_phase::{
    AddRenderCommand, DrawFunctions, PhaseItemExtraIndex, RenderCommand, RenderCommandResult, SetItemPipeline,
    TrackedRenderPass, ViewSortedRenderPhases,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType,
    Buffer, BufferBindingType, BufferDescriptor, BufferUsages, ColorTargetState, ColorWrites, CompareFunction,
    DepthStencilState, FragmentState, FrontFace, MultisampleState, PipelineCache, PolygonMode, PrimitiveState,
    RenderPipelineDescriptor, ShaderStages, ShaderType, SpecializedRenderPipeline, SpecializedRenderPipelines,
    TextureFormat, VertexState,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::sync_world::{MainEntity, RenderEntity, SyncToRenderWorld};
use bevy::render::view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};

use eustress_common::realism::particle_sim::{ParticleCloud, ParticleInstance};

const SHADER_WGSL: &str = include_str!("../../assets/shaders/particle_cloud.wgsl");

/// Bytes of the per-cloud uniform: `world_from_local` (mat4) + `params` (vec4).
const CLOUD_UNIFORM_BYTES: u64 = 80;

/// Strong handle to the baked shader, present in both worlds.
#[derive(Resource, Clone)]
struct ParticleCloudShader(Handle<Shader>);

/// Render-world copy of one simulation's cloud.
#[derive(Component)]
struct ExtractedParticleCloud {
    instances: Arc<Vec<ParticleInstance>>,
    revision: u64,
    world_from_local: Mat4,
    center: Vec3,
}

/// GPU buffers of one cloud, kept across frames and grown on demand.
#[derive(Component)]
struct ParticleCloudGpu {
    storage: Buffer,
    capacity: u64,
    uniform: Buffer,
    bind_group: BindGroup,
    count: u32,
    revision: u64,
}

#[derive(Component)]
struct ParticleViewBindGroup(BindGroup);

#[derive(Resource)]
struct ParticleCloudPipeline {
    view_layout: BindGroupLayout,
    cloud_layout: BindGroupLayout,
    view_layout_desc: BindGroupLayoutDescriptor,
    cloud_layout_desc: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
}

impl FromWorld for ParticleCloudPipeline {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        let view_entries = vec![BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: Some(ViewUniform::min_size()),
            },
            count: None,
        }];
        let cloud_entries = vec![
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(CLOUD_UNIFORM_BYTES),
                },
                count: None,
            },
        ];
        let view_layout = device.create_bind_group_layout("particle_cloud_view_layout", &view_entries);
        let cloud_layout = device.create_bind_group_layout("particle_cloud_layout", &cloud_entries);
        let shader = world.resource::<ParticleCloudShader>().0.clone();
        Self {
            view_layout,
            cloud_layout,
            view_layout_desc: BindGroupLayoutDescriptor {
                label: "particle_cloud_view_layout".into(),
                entries: view_entries,
            },
            cloud_layout_desc: BindGroupLayoutDescriptor {
                label: "particle_cloud_layout".into(),
                entries: cloud_entries,
            },
            shader,
        }
    }
}

/// Pipeline variant: the view's MSAA sample count and colour attachment
/// format (HDR, the Bgra8 surface, or the Rgba8 intermediate).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ParticleCloudKey {
    samples: u32,
    format: TextureFormat,
}

impl SpecializedRenderPipeline for ParticleCloudPipeline {
    type Key = ParticleCloudKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        RenderPipelineDescriptor {
            label: Some("particle_cloud_pipeline".into()),
            layout: vec![self.view_layout_desc.clone(), self.cloud_layout_desc.clone()],
            vertex: VertexState {
                shader: self.shader.clone(),
                entry_point: Some("vertex".into()),
                buffers: vec![],
                shader_defs: vec![],
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                entry_point: Some("fragment".into()),
                shader_defs: vec![],
                targets: vec![Some(ColorTargetState {
                    format: key.format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            // Reverse-Z: nearer is greater. Spheres write depth so they
            // occlude each other and anything drawn after them.
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::Greater),
                stencil: default(),
                bias: default(),
            }),
            multisample: MultisampleState {
                count: key.samples,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            immediate_size: 0,
            zero_initialize_workgroup_memory: false,
        }
    }
}

// ============================================================================
// Main world
// ============================================================================

/// Give every simulation a render-world twin as soon as it has a cloud.
fn sync_particle_clouds_to_render_world(
    mut commands: Commands,
    clouds: Query<Entity, (With<ParticleCloud>, Without<SyncToRenderWorld>)>,
) {
    for entity in &clouds {
        commands.entity(entity).insert(SyncToRenderWorld);
    }
}

// ============================================================================
// Render world
// ============================================================================

fn extract_particle_clouds(
    mut commands: Commands,
    clouds: Extract<
        Query<(&RenderEntity, &ParticleCloud, &GlobalTransform, Option<&InheritedVisibility>)>,
    >,
) {
    for (render_entity, cloud, global, visibility) in &clouds {
        let visible = visibility.map_or(true, |v| v.get());
        if !visible || cloud.instances.is_empty() {
            commands.entity(render_entity.id()).remove::<ExtractedParticleCloud>();
            continue;
        }
        // Rotation and translation only: a simulation parented under a Part
        // must not inherit the part's size-as-scale.
        let (_, rotation, translation) = global.to_scale_rotation_translation();
        let world_from_local =
            Mat4::from_scale_rotation_translation(Vec3::splat(cloud.display_scale.max(1e-30)), rotation, translation);
        commands.entity(render_entity.id()).insert(ExtractedParticleCloud {
            instances: cloud.instances.clone(),
            revision: cloud.revision,
            world_from_local,
            center: translation,
        });
    }
}

fn cloud_uniform_words(cloud: &ExtractedParticleCloud) -> [f32; 20] {
    let mut words = [0.0f32; 20];
    words[..16].copy_from_slice(&cloud.world_from_local.to_cols_array());
    words[16] = 1.0;
    words
}

fn prepare_particle_clouds(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    pipeline: Res<ParticleCloudPipeline>,
    mut clouds: Query<(Entity, &ExtractedParticleCloud, Option<&mut ParticleCloudGpu>)>,
) {
    let stride = std::mem::size_of::<ParticleInstance>() as u64;
    for (entity, cloud, gpu) in &mut clouds {
        let count = cloud.instances.len() as u32;
        let bytes = count as u64 * stride;
        let words = cloud_uniform_words(cloud);
        match gpu {
            Some(mut gpu) if gpu.capacity >= bytes => {
                if gpu.revision != cloud.revision {
                    queue.write_buffer(&gpu.storage, 0, bytemuck::cast_slice(cloud.instances.as_slice()));
                    gpu.revision = cloud.revision;
                }
                queue.write_buffer(&gpu.uniform, 0, bytemuck::cast_slice(&words));
                gpu.count = count;
            }
            _ => {
                // Half again as much room so a growing cloud does not
                // reallocate every frame.
                let capacity = ((bytes.max(stride) * 3 / 2).div_ceil(256)) * 256;
                let storage = device.create_buffer(&BufferDescriptor {
                    label: Some("particle_cloud_instances"),
                    size: capacity,
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let uniform = device.create_buffer(&BufferDescriptor {
                    label: Some("particle_cloud_uniform"),
                    size: CLOUD_UNIFORM_BYTES,
                    usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&storage, 0, bytemuck::cast_slice(cloud.instances.as_slice()));
                queue.write_buffer(&uniform, 0, bytemuck::cast_slice(&words));
                let bind_group = device.create_bind_group(
                    Some("particle_cloud_bind_group"),
                    &pipeline.cloud_layout,
                    &[
                        BindGroupEntry { binding: 0, resource: storage.as_entire_binding() },
                        BindGroupEntry { binding: 1, resource: uniform.as_entire_binding() },
                    ],
                );
                commands.entity(entity).insert(ParticleCloudGpu {
                    storage,
                    capacity,
                    uniform,
                    bind_group,
                    count,
                    revision: cloud.revision,
                });
            }
        }
    }
}

fn prepare_particle_view_bind_groups(
    mut commands: Commands,
    device: Res<RenderDevice>,
    pipeline: Res<ParticleCloudPipeline>,
    view_uniforms: Res<ViewUniforms>,
    views: Query<Entity, With<ExtractedView>>,
    clouds: Query<(), With<ExtractedParticleCloud>>,
) {
    if clouds.is_empty() {
        return;
    }
    let Some(binding) = view_uniforms.uniforms.binding() else { return };
    for view in &views {
        commands.entity(view).insert(ParticleViewBindGroup(device.create_bind_group(
            Some("particle_cloud_view_bind_group"),
            &pipeline.view_layout,
            &[BindGroupEntry { binding: 0, resource: binding.clone() }],
        )));
    }
}

#[allow(clippy::too_many_arguments)]
fn queue_particle_clouds(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    pipeline: Res<ParticleCloudPipeline>,
    mut pipelines: ResMut<SpecializedRenderPipelines<ParticleCloudPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    views: Query<(&ExtractedView, Option<&Msaa>, Option<&ViewTarget>, Option<&RenderLayers>)>,
    clouds: Query<(Entity, &MainEntity, &ExtractedParticleCloud)>,
) {
    if clouds.is_empty() {
        return;
    }
    let Some(draw) = draw_functions.read().get_id::<DrawParticleCloud>() else { return };
    let scene_layer = RenderLayers::default();
    for (view, msaa, target, layers) in &views {
        // The Slint overlay camera renders layer 31 only.
        if layers.is_some_and(|l| !l.intersects(&scene_layer)) {
            continue;
        }
        let Some(target) = target else { continue };
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else { continue };
        let key = ParticleCloudKey {
            samples: msaa.copied().unwrap_or_default().samples(),
            format: target.main_texture_format(),
        };
        let pipeline_id = pipelines.specialize(&pipeline_cache, &pipeline, key);
        let rangefinder = view.rangefinder3d();
        for (entity, main_entity, cloud) in &clouds {
            phase.add_transient(Transparent3d {
                sorting_info: TransparentSortingInfo3d::Sorted { mesh_center: cloud.center, depth_bias: 0.0 },
                pipeline: pipeline_id,
                entity: (entity, *main_entity),
                draw_function: draw,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                distance: rangefinder.distance(&cloud.center),
                indexed: false,
            });
        }
    }
}

struct SetParticleViewBindGroup<const I: usize>;
impl<const I: usize> RenderCommand<Transparent3d> for SetParticleViewBindGroup<I> {
    type Param = ();
    type ViewQuery = (Read<ViewUniformOffset>, Read<ParticleViewBindGroup>);
    type ItemQuery = ();

    fn render<'w>(
        _item: &Transparent3d,
        (offset, bind_group): ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(I, &bind_group.0, &[offset.offset]);
        RenderCommandResult::Success
    }
}

struct DrawParticleCloudInstances;
impl RenderCommand<Transparent3d> for DrawParticleCloudInstances {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = Read<ParticleCloudGpu>;

    fn render<'w>(
        _item: &Transparent3d,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        gpu: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(gpu) = gpu else { return RenderCommandResult::Skip };
        if gpu.count == 0 {
            return RenderCommandResult::Skip;
        }
        pass.set_bind_group(1, &gpu.bind_group, &[]);
        pass.draw(0..6, 0..gpu.count);
        RenderCommandResult::Success
    }
}

type DrawParticleCloud = (SetItemPipeline, SetParticleViewBindGroup<0>, DrawParticleCloudInstances);

/// Draws every `ParticleSimulation` (editor and player; never headless).
pub struct ParticleRenderPlugin;

impl Plugin for ParticleRenderPlugin {
    fn build(&self, app: &mut App) {
        let shader: Handle<Shader> = {
            let mut shaders = app.world_mut().resource_mut::<Assets<Shader>>();
            shaders.add(Shader::from_wgsl(SHADER_WGSL, "particle_cloud.wgsl"))
        };
        app.insert_resource(ParticleCloudShader(shader.clone()))
            .add_systems(PostUpdate, sync_particle_clouds_to_render_world);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .insert_resource(ParticleCloudShader(shader))
            .init_resource::<SpecializedRenderPipelines<ParticleCloudPipeline>>()
            .add_render_command::<Transparent3d, DrawParticleCloud>()
            .add_systems(ExtractSchedule, extract_particle_clouds)
            .add_systems(
                Render,
                (
                    queue_particle_clouds.in_set(RenderSystems::Queue),
                    prepare_particle_clouds.in_set(RenderSystems::PrepareResources),
                    prepare_particle_view_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.init_resource::<ParticleCloudPipeline>();
        }
    }
}

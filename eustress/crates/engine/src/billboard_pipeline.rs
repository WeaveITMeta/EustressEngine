//! # Custom billboard render pipeline (Slint-rendered texture → 3D quad)
//!
//! Vendored + adapted from `kulkalkul/bevy_mod_billboard`
//! (<https://github.com/kulkalkul/bevy_mod_billboard>, MIT/Apache-2.0).
//!
//! ## Why we don't use `StandardMaterial`
//!
//! The previous billboard implementation in [`billboard_gui`] painted the
//! Slint-rendered card into an `Image`, attached it to a `StandardMaterial`
//! with `AlphaMode::Blend + depth_bias`, and drew it as a regular Bevy mesh.
//! That works for a hello-world demo but breaks the moment you want
//! "billboards behave like real geometry":
//!
//! - `AlphaMode::Blend` puts the mesh into the transparent pass, where
//!   `depth_bias` is silently ignored on Bevy 0.18 (the bias is honoured by
//!   the prepass + opaque pipelines, not the blend one). Result: a billboard
//!   set to "always on top" was at the mercy of phase sort order, which is
//!   distance-based — labels far from the camera would render BEHIND
//!   foreground geometry even though they were flagged as on-top.
//! - The opposite case — billboards that should be occluded by a wall the
//!   camera moved behind — also failed because `depth_bias` was being applied
//!   even when we wanted plain depth tests.
//! - `StandardMaterial` runs through the PBR shader: lighting, fog, tonemap.
//!   None of that matters for a software-rendered text card. We were paying
//!   for shader work whose only effect was to slightly tint our pixels.
//!
//! ## What this pipeline does instead
//!
//! - **Custom WGSL shader** ([`assets/shaders/billboard.wgsl`]). Vertex stage
//!   does camera-facing math by reading right + up vectors from
//!   `view.clip_from_world`'s columns; fragment stage samples the texture
//!   directly (no PBR roundtrip).
//! - **Specialised render pipeline** with explicit `DepthStencilState`.
//!   `depth_compare` is keyed on a `BillboardDepth` component:
//!   - `BillboardDepth(true)` → `CompareFunction::Greater` (Bevy 0.18 reverse-Z;
//!     the billboard's fragment passes only if its depth is closer than what's
//!     already in the depth buffer — i.e. real occlusion).
//!   - `BillboardDepth(false)` → `CompareFunction::Always` (always wins the
//!     depth test; the "always on top" mode for things like map markers).
//!   `depth_write_enabled` is always `false` so billboards don't occlude each
//!   other or solid geometry behind them.
//! - **Renders in the `Transparent3d` phase** so billboards sort back-to-front
//!   against each other and against translucent scene geometry, while still
//!   being depth-tested against opaque scene geometry.
//! - **Camera facing in the shader, not on CPU**. The previous
//!   `billboard_face_camera` system that did `Quat::from_rotation_arc` per
//!   billboard per frame is no longer needed — the shader generates the
//!   quad's vertex positions in clip space directly from `view.clip_from_world`.
//!
//! ## Components consumed
//!
//! - [`BillboardMesh`] — handle to a 2-tri quad mesh (the unit billboard).
//!   Not the StandardMaterial mesh — this is a plain `Handle<Mesh>` that
//!   carries `ATTRIBUTE_POSITION` and `ATTRIBUTE_UV_0`.
//! - [`BillboardAtlasTexture`] — handle to the shared atlas `Image` every
//!   billboard samples. The atlas is managed by
//!   `crate::billboard_gui::BillboardAtlas`.
//! - [`BillboardUv`] — per-entity `uv_min`/`uv_max` selecting this
//!   billboard's tile inside the shared atlas.
//! - [`BillboardDepth`] — flips depth-test mode. Driven from the
//!   `BillboardGui::always_on_top` class field.
//! - [`BillboardLockAxis`] — optional. `y_axis` keeps billboard upright as the
//!   camera rolls; `rotation` disables billboarding entirely so the quad uses
//!   the entity's `Transform.rotation` literally.
//!
//! ## Bevy 0.18 adaptations vs. upstream crate
//!
//! - `Transparent3d.entity` is now `(Entity, MainEntity)` (was `Entity`).
//! - `Transparent3d` carries an `indexed: bool` field that `add(...)` requires.
//! - `Msaa` is read from per-view component, not a global resource.
//! - `Read<...>` was renamed to `lifetimeless::Read` (same import path though).
//! - `Mesh::ATTRIBUTE_POSITION.at_shader_location(N)` is unchanged.
//! - `MeshVertexBufferLayoutRef` API is unchanged.
//!
//! ## Lifecycle
//!
//! Main world spawns: `(Mesh3d, BillboardMesh, BillboardAtlasTexture, BillboardUv,
//! BillboardDepth, Transform, Visibility, Billboard)`. The `Mesh3d` is what makes Bevy's
//! `VisibleEntities` pick it up (we filter by `With<Billboard>` later).
//! `extract_billboards` copies the components into the render world; the
//! pipeline + draw functions take it from there. No `StandardMaterial`,
//! no `MeshMaterial3d`, no `face_camera` CPU system.

use bevy::asset::{AssetId, Assets, Handle};
// `RenderVisibleEntities` was used here to filter billboards via the
// `check_visibility` system, but Bevy 0.18's check_visibility only
// tracks specific render classes (Mesh3d, Sprite, …) and doesn't know
// about our custom `Billboard` marker. We now iterate billboards
// directly in `queue_billboards`; visibility is filtered earlier in
// `extract_billboards` via `InheritedVisibility`.
use bevy::core_pipeline::core_3d::Transparent3d;
use bevy::ecs::query::ROQueryItem;
use bevy::ecs::system::{lifetimeless::*, SystemParamItem};
use bevy::image::BevyDefault;
use bevy::math::Mat4;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::render::mesh::{RenderMesh, RenderMeshBufferInfo, allocator::MeshAllocator};
use bevy::prelude::*;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{
    AddRenderCommand, DrawFunctions, PhaseItemExtraIndex, RenderCommand,
    RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingResource, BindingType, BlendComponent, BlendFactor, BlendOperation, BlendState,
    BufferBindingType, ColorTargetState, ColorWrites, CompareFunction, DepthStencilState,
    FragmentState, FrontFace, MultisampleState, PipelineCache, PolygonMode, PrimitiveState,
    RenderPipelineDescriptor, SamplerBindingType, ShaderStages, ShaderType,
    SpecializedMeshPipeline, SpecializedMeshPipelineError, SpecializedMeshPipelines,
    TextureFormat, TextureSampleType, TextureViewDimension, VertexState,
};
use bevy::render::renderer::RenderDevice;
use bevy::render::sync_world::{MainEntity, RenderEntity};
use bevy::render::texture::GpuImage;
use bevy::render::camera::ExtractedCamera; // 0.19: hdr moved off ExtractedView onto ExtractedCamera
use bevy::render::view::{
    ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms,
};
use bevy::render::{Extract, Render, RenderApp, RenderSystems, ExtractSchedule};

// NOTE: We previously used a `weak_handle!` UUID for the billboard
// shader and tried to reference it directly in the pipeline descriptor.
// That fails on Bevy 0.18 because `embedded_asset!` registers the
// shader with an auto-generated `AssetId` that has no relation to the
// weak handle's UUID — the pipeline ends up pointing at a UUID that
// nothing ever loads, producing `ShaderNotLoaded` errors and silently
// aborting `SetItemPipeline`.
//
// Replaced with a Strong `Handle<Shader>` loaded at plugin finish via
// `asset_server.load("embedded://...")` and stored on
// `BillboardPipeline.shader_handle`. The pipeline descriptor's
// `shader:` field reads from there so the actual loaded shader matches
// the referenced handle.

// ============================================================================
// Components (main world)
// ============================================================================

/// Marker component placed on every billboard entity. Render systems filter
/// VisibleEntities by `With<Billboard>` to find what to draw.
///
/// Required components, all on the main-world entity:
/// - `Transform` + `Visibility`: standard Bevy renderable scaffolding.
/// - `SyncToRenderWorld` (Bevy 0.18+) — without it the main-world entity
///   has no matching render-world counterpart, so `extract_billboards`'s
///   `try_insert_batch` warns "entity does not exist" every frame and
///   rendering silently no-ops.
/// - `NoFrustumCulling` — billboards have no meaningful static `Aabb`
///   (the on-screen quad is built per-frame in the vertex shader from
///   camera basis vectors). Without `NoFrustumCulling`, Bevy's
///   `check_visibility` system fails to add the entity to
///   `RenderVisibleEntities`, so `queue_billboards` never sees it and
///   nothing renders. `NoFrustumCulling` tells `check_visibility` to
///   skip the frustum test and always include the entity — the
///   distance-cull system in `billboard_gui` handles range culling
///   independently via `Visibility::Hidden`.
#[derive(Component, Default, Clone, Copy)]
#[require(
    Transform,
    Visibility,
    bevy::render::sync_world::SyncToRenderWorld,
    bevy::camera::visibility::NoFrustumCulling,
)]
pub struct Billboard;

/// Per-billboard depth-test mode. `true` (default) → real occlusion;
/// `false` → always renders on top of everything.
///
/// Wired from `BillboardGui::always_on_top` in `billboard_gui.rs`:
/// `BillboardDepth(!always_on_top)`.
#[derive(Component, Clone, Copy, Debug)]
pub struct BillboardDepth(pub bool);

impl Default for BillboardDepth {
    fn default() -> Self { Self(true) }
}

impl ExtractComponent for BillboardDepth {
    type QueryData = &'static BillboardDepth;
    type QueryFilter = With<Billboard>;
    type Out = BillboardDepth;
    fn extract_component(item: bevy::ecs::query::QueryItem<'_, '_, Self::QueryData>) -> Option<Self::Out> {
        Some(*item)
    }
}

/// Optional axis lock. `y_axis = true` keeps the billboard upright (camera
/// roll doesn't roll the label). `rotation = true` disables billboarding
/// entirely — the quad uses the entity's `Transform.rotation`.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct BillboardLockAxis {
    pub y_axis: bool,
    pub rotation: bool,
}

impl ExtractComponent for BillboardLockAxis {
    type QueryData = &'static BillboardLockAxis;
    type QueryFilter = With<Billboard>;
    type Out = BillboardLockAxis;
    fn extract_component(item: bevy::ecs::query::QueryItem<'_, '_, Self::QueryData>) -> Option<Self::Out> {
        Some(*item)
    }
}

/// Mesh handle the billboard pipeline draws. Decoupled from `Mesh3d` because
/// we don't want our entity to render through the standard PBR mesh path
/// AND through ours — only ours. The presence of `Billboard` filters it out
/// of the standard mesh draw.
#[derive(Component, Clone)]
pub struct BillboardMesh(pub Handle<Mesh>);

/// Atlas texture handle. Every billboard points at the same shared atlas
/// (managed by [`crate::billboard_gui::BillboardAtlas`]); per-entity
/// differentiation comes from [`BillboardUv`].
#[derive(Component, Clone)]
pub struct BillboardAtlasTexture(pub Handle<Image>);

/// Per-billboard UV bounds within the shared atlas plus a depth bias
/// driving `BillboardGui.z_index`. The fragment shader uses `uv_min/max`
/// to remap the quad's `[0,1]×[0,1]` UV onto its atlas tile; the vertex
/// shader uses `z_bias` to shift the quad along the camera-toward
/// direction so a label can win the depth test against the part it's
/// pinned to without becoming `AlwaysOnTop`.
///
/// `_padding` brings the struct to a 16-byte std140 boundary (vec2+vec2
/// = 16, plus f32+f32 = 8, padded to 24 → rounded to 32 by encase). The
/// padding field exists so layout is explicit rather than implicit.
#[derive(Component, Clone, Copy, ShaderType)]
pub struct BillboardUv {
    pub uv_min: Vec2,
    pub uv_max: Vec2,
    pub z_bias: f32,
    pub _padding: f32,
}

impl Default for BillboardUv {
    fn default() -> Self {
        Self {
            uv_min: Vec2::ZERO,
            uv_max: Vec2::ONE,
            z_bias: 0.0,
            _padding: 0.0,
        }
    }
}

// ============================================================================
// Render-world components (extracted)
// ============================================================================

/// Per-billboard uniform — the model matrix that positions the billboard's
/// pivot in world space. Camera-facing rotation happens in the shader, so
/// this matrix doesn't carry rotation when `BillboardLockAxis` is absent.
#[derive(Clone, Copy, ShaderType, Component)]
pub struct BillboardUniform {
    pub transform: Mat4,
}

#[derive(Component, Clone, Copy)]
pub struct RenderBillboardMesh {
    pub id: AssetId<Mesh>,
}

#[derive(Component, Clone, Copy)]
pub struct RenderBillboardImage {
    pub id: AssetId<Image>,
}

#[derive(Component, Clone, Copy)]
pub struct RenderBillboard {
    pub depth: BillboardDepth,
    pub lock_axis: Option<BillboardLockAxis>,
}

// ============================================================================
// Resources (render world)
// ============================================================================

#[derive(Resource, Default)]
pub struct BillboardImageBindGroups {
    values: bevy::platform::collections::HashMap<AssetId<Image>, BindGroup>,
}

#[derive(Resource)]
pub struct BillboardBindGroup {
    value: BindGroup,
}

#[derive(Component)]
pub struct BillboardViewBindGroup {
    value: BindGroup,
}

// ============================================================================
// Pipeline + key
// ============================================================================

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    #[repr(transparent)]
    pub struct BillboardPipelineKey: u32 {
        const DEPTH         = (1 << 0);
        const LOCK_Y        = (1 << 1);
        const LOCK_ROTATION = (1 << 2);
        const HDR           = (1 << 3);
        const MSAA_BITS     = Self::MSAA_MASK_BITS << Self::MSAA_SHIFT_BITS;
    }
}

impl BillboardPipelineKey {
    const MSAA_MASK_BITS: u32 = 0b111;
    const MSAA_SHIFT_BITS: u32 = 32 - Self::MSAA_MASK_BITS.count_ones();

    pub fn from_msaa_samples(samples: u32) -> Self {
        let bits = (samples.trailing_zeros() & Self::MSAA_MASK_BITS) << Self::MSAA_SHIFT_BITS;
        Self::from_bits_retain(bits)
    }
    pub fn msaa_samples(&self) -> u32 {
        1 << ((self.bits() >> Self::MSAA_SHIFT_BITS) & Self::MSAA_MASK_BITS)
    }
}

/// Bevy 0.18 changed `RenderPipelineDescriptor.layout` to take
/// `Vec<BindGroupLayoutDescriptor>` (declarative, hashable) instead of the
/// previously-built `Vec<BindGroupLayout>` — the pipeline cache builds the
/// real layouts from the descriptors. We hold both forms: the descriptors
/// for `specialize()` returns, and the built layouts for runtime
/// `create_bind_group` calls inside `prepare_*` and `queue_*`.
#[derive(Resource, Clone)]
pub struct BillboardPipeline {
    view_layout: BindGroupLayout,
    billboard_layout: BindGroupLayout,
    texture_layout: BindGroupLayout,
    view_layout_desc: BindGroupLayoutDescriptor,
    billboard_layout_desc: BindGroupLayoutDescriptor,
    texture_layout_desc: BindGroupLayoutDescriptor,
    /// Strong handle to the embedded `billboard.wgsl`. Loaded in the
    /// plugin's `finish()` once the AssetServer is available, then
    /// stored here so `specialize()` can reference the actual loaded
    /// asset rather than a weak handle whose UUID won't match.
    shader_handle: Handle<Shader>,
}

/// Embedded shader source — `include_str!` baked into the binary so
/// the pipeline never depends on the embedded-asset URL resolution
/// (which was returning `ShaderNotLoaded` because the path we passed
/// didn't match what `embedded_asset!` registered in the render
/// world's AssetServer).
///
/// Registered in the MAIN world's `Assets<Shader>` during plugin
/// `build()` (the render world has no `Assets<Shader>` — that storage
/// is main-world only; assets are extracted across each frame). The
/// resulting Handle is stored in a `BillboardShaderHandle` Resource
/// inserted into BOTH worlds so `BillboardPipeline::from_world` (which
/// runs in the render world) can read it.
const BILLBOARD_SHADER_WGSL: &str = include_str!("../assets/shaders/billboard.wgsl");

/// Resource carrying the strong shader handle from main → render world.
#[derive(Resource, Clone)]
pub struct BillboardShaderHandle(pub Handle<Shader>);

impl FromWorld for BillboardPipeline {
    fn from_world(world: &mut World) -> Self {
        let render_device = world.resource::<RenderDevice>();

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
        let billboard_entries = vec![
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: Some(BillboardUniform::min_size()),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                // Vertex stage reads `z_bias` for depth biasing; fragment
                // stage reads `uv_min/uv_max` for atlas sampling.
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: Some(BillboardUv::min_size()),
                },
                count: None,
            },
        ];
        let texture_entries = vec![
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    multisampled: false,
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ];

        let view_layout = render_device.create_bind_group_layout("billboard_view_layout", &view_entries);
        let billboard_layout = render_device.create_bind_group_layout("billboard_layout", &billboard_entries);
        let texture_layout = render_device.create_bind_group_layout("billboard_texture_layout", &texture_entries);

        // Strong handle pre-registered in the main world's
        // `Assets<Shader>`; copied into the render world by the plugin
        // as a `BillboardShaderHandle` resource so we can read it
        // here. The render world has no `Assets<Shader>` of its own.
        let shader_handle = world
            .resource::<BillboardShaderHandle>()
            .0
            .clone();

        Self {
            view_layout,
            billboard_layout,
            texture_layout,
            view_layout_desc: BindGroupLayoutDescriptor {
                label: "billboard_view_layout".into(),
                entries: view_entries,
            },
            billboard_layout_desc: BindGroupLayoutDescriptor {
                label: "billboard_layout".into(),
                entries: billboard_entries,
            },
            texture_layout_desc: BindGroupLayoutDescriptor {
                label: "billboard_texture_layout".into(),
                entries: texture_entries,
            },
            shader_handle,
        }
    }
}

impl SpecializedMeshPipeline for BillboardPipeline {
    type Key = BillboardPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        layout: &MeshVertexBufferLayoutRef,
    ) -> Result<RenderPipelineDescriptor, SpecializedMeshPipelineError> {
        const DEF_LOCK_Y: &str = "LOCK_Y";
        const DEF_LOCK_ROTATION: &str = "LOCK_ROTATION";

        let mut shader_defs = Vec::with_capacity(2);
        let attributes = vec![
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
        ];
        let vertex_buffer_layout = layout.0.get_layout(&attributes)?;

        let depth_compare = if key.contains(BillboardPipelineKey::DEPTH) {
            // Reverse-Z in Bevy 0.18 → "closer" = larger depth value, so
            // billboards must be GREATER than what's in the depth buffer
            // to win the test (i.e. they're occluded by closer geometry).
            CompareFunction::Greater
        } else {
            CompareFunction::Always
        };

        if key.contains(BillboardPipelineKey::LOCK_Y) {
            shader_defs.push(DEF_LOCK_Y.into());
        }
        if key.contains(BillboardPipelineKey::LOCK_ROTATION) {
            shader_defs.push(DEF_LOCK_ROTATION.into());
        }

        Ok(RenderPipelineDescriptor {
            label: Some("billboard_pipeline".into()),
            layout: vec![
                self.view_layout_desc.clone(),
                self.billboard_layout_desc.clone(),
                self.texture_layout_desc.clone(),
            ],
            vertex: VertexState {
                shader: self.shader_handle.clone(),
                entry_point: Some("vertex".into()),
                buffers: vec![vertex_buffer_layout],
                shader_defs: shader_defs.clone(),
            },
            fragment: Some(FragmentState {
                shader: self.shader_handle.clone(),
                entry_point: Some("fragment".into()),
                shader_defs,
                targets: vec![Some(ColorTargetState {
                    format: if key.contains(BillboardPipelineKey::HDR) {
                        bevy::render::view::ViewTarget::TEXTURE_FORMAT_HDR
                    } else {
                        TextureFormat::bevy_default()
                    },
                    blend: Some(BlendState {
                        color: BlendComponent {
                            src_factor: BlendFactor::SrcAlpha,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            operation: BlendOperation::Add,
                        },
                        alpha: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::One,
                            operation: BlendOperation::Add,
                        },
                    }),
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
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: false, // never write — don't occlude future draws
                depth_compare,
                stencil: default(),
                bias: default(),
            }),
            multisample: MultisampleState {
                count: key.msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            push_constant_ranges: vec![],
            zero_initialize_workgroup_memory: false,
        })
    }
}

// ============================================================================
// Extract — main world → render world
// ============================================================================

/// Build the billboard's model matrix. When the billboard is camera-facing
/// (no `BillboardLockAxis`), the matrix carries only translation + scale —
/// the shader applies camera rotation per-vertex. When locked, we use the
/// full `GlobalTransform` so the user's authored rotation is honoured.
fn calculate_billboard_uniform(
    global_transform: &GlobalTransform,
    transform: &Transform,
    lock_axis: Option<&BillboardLockAxis>,
) -> BillboardUniform {
    let matrix = if lock_axis.is_some() {
        global_transform.to_matrix()
    } else {
        let global_matrix = global_transform.to_matrix();
        // Strip rotation: keep only scale (per-axis) and translation.
        Mat4::from_cols(
            Mat4::IDENTITY.x_axis * transform.scale.x,
            Mat4::IDENTITY.y_axis * transform.scale.y,
            Mat4::IDENTITY.z_axis * transform.scale.z,
            global_matrix.w_axis,
        )
    };
    BillboardUniform { transform: matrix }
}

/// Extract billboard entities into the render world.
///
/// Bevy 0.18 separated main-world and render-world entity IDs entirely.
/// `SyncToRenderWorld` (set via `Billboard`'s `#[require]`) tells Bevy to
/// spawn a corresponding render-world entity and store its ID on the
/// main-world entity as `RenderEntity`. We query `RenderEntity` here so
/// `try_insert_batch` targets the render-world counterpart — passing the
/// main-world `Entity` would warn "entity does not exist" every frame.
///
/// Visibility filter uses `InheritedVisibility` (the post-parent-chain
/// user-facing visibility) rather than `ViewVisibility` (which requires
/// an `Aabb` for frustum culling — billboards don't have one because
/// their on-screen footprint is computed in the vertex shader). The
/// distance-based cull system in `billboard_gui` already handles "this
/// billboard is too far / too close" by toggling `Visibility::Hidden`,
/// which propagates into `InheritedVisibility`.
pub fn extract_billboards(
    mut commands: Commands,
    mut previous_len: Local<usize>,
    query: Extract<
        Query<(
            &RenderEntity,
            &InheritedVisibility,
            &GlobalTransform,
            &Transform,
            &BillboardMesh,
            &BillboardAtlasTexture,
            &BillboardUv,
            Option<&BillboardDepth>,
            Option<&BillboardLockAxis>,
        ), With<Billboard>>,
    >,
) {
    let mut batch: Vec<(Entity, _)> = Vec::with_capacity(*previous_len);
    for (render_entity, inherited, global_tf, transform, mesh, texture, uv, depth, lock_axis) in &query {
        if !inherited.get() { continue; }
        let uniform = calculate_billboard_uniform(global_tf, transform, lock_axis);
        let depth_val = depth.copied().unwrap_or_default();
        batch.push((
            render_entity.id(),
            (
                Billboard,
                uniform,
                *uv,
                RenderBillboardMesh { id: mesh.0.id() },
                RenderBillboardImage { id: texture.0.id() },
                RenderBillboard {
                    depth: depth_val,
                    lock_axis: lock_axis.copied(),
                },
            ),
        ));
    }
    *previous_len = batch.len();
    commands.try_insert_batch(batch);
}

// ============================================================================
// Prepare — bind groups
// ============================================================================

pub fn prepare_billboard_view_bind_groups(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    pipeline: Res<BillboardPipeline>,
    view_uniforms: Res<ViewUniforms>,
    views: Query<Entity, With<ExtractedView>>,
) {
    let Some(binding) = view_uniforms.uniforms.binding() else { return };

    for entity in &views {
        commands.entity(entity).insert(BillboardViewBindGroup {
            value: render_device.create_bind_group(
                Some("billboard_view_bind_group"),
                &pipeline.view_layout,
                &[BindGroupEntry { binding: 0, resource: binding.clone() }],
            ),
        });
    }
}

pub fn prepare_billboard_bind_group(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    pipeline: Res<BillboardPipeline>,
    uniforms: Res<ComponentUniforms<BillboardUniform>>,
    uv_uniforms: Res<ComponentUniforms<BillboardUv>>,
) {
    let Some(binding) = uniforms.uniforms().binding() else { return };
    let Some(uv_binding) = uv_uniforms.uniforms().binding() else { return };
    commands.insert_resource(BillboardBindGroup {
        value: render_device.create_bind_group(
            Some("billboard_bind_group"),
            &pipeline.billboard_layout,
            &[
                BindGroupEntry { binding: 0, resource: binding },
                BindGroupEntry { binding: 1, resource: uv_binding },
            ],
        ),
    });
}

// ============================================================================
// Queue — add billboards to the Transparent3d phase
// ============================================================================

pub fn queue_billboards(
    mut transparent_phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut pipeline_cache: ResMut<PipelineCache>,
    mut image_bind_groups: ResMut<BillboardImageBindGroups>,
    mut pipelines: ResMut<SpecializedMeshPipelines<BillboardPipeline>>,
    render_device: Res<RenderDevice>,
    transparent_draw_functions: Res<DrawFunctions<Transparent3d>>,
    pipeline: Res<BillboardPipeline>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    gpu_meshes: Res<RenderAssets<RenderMesh>>,
    views: Query<(Entity, &ExtractedView, Option<&ExtractedCamera>, Option<&Msaa>)>,
    // All billboards in the render world (from `extract_billboards`).
    // Visibility was already filtered there via `InheritedVisibility`, so
    // every entity here should be drawn. We use `MainEntity` to populate
    // `Transparent3d::entity` (Bevy 0.18 phase items carry both).
    billboards: Query<(
        Entity,
        &MainEntity,
        &BillboardUniform,
        &RenderBillboardMesh,
        &RenderBillboardImage,
        &RenderBillboard,
    )>,
) {
    // Clear the bind-group cache each frame. When the atlas grows
    // (`BillboardAtlas::try_grow`), the underlying GpuImage is rebuilt
    // with new dimensions and the cached bind group's `TextureView`
    // references the OLD texture — sampling stale data. Rebuilding
    // every frame is cheap (we share one atlas, so it's a single
    // `create_bind_group` call) and guarantees correctness on resize.
    image_bind_groups.values.clear();

    for (_view_entity, view, extracted_camera, msaa) in views.iter() {
        // Bevy 0.18: `ViewSortedRenderPhases` is keyed by
        // `RetainedViewEntity` (a stable identifier that survives the
        // main→render extract roundtrip), not the render-world Entity.
        // Pull it from `ExtractedView.retained_view_entity`.
        let Some(transparent_phase) = transparent_phases.get_mut(&view.retained_view_entity) else { continue };
        let draw_billboard = transparent_draw_functions
            .read()
            .get_id::<DrawBillboard>()
            .unwrap();
        let rangefinder = view.rangefinder3d();
        let msaa_samples: u32 = msaa.copied().unwrap_or_default().samples();

        // Iterate all extracted billboards directly. We bypass
        // `RenderVisibleEntities` because Bevy's `check_visibility`
        // system tracks specific render classes (Mesh3d, Sprite, …) and
        // doesn't know about our custom `Billboard` marker — the entity
        // would never appear in `RenderVisibleEntities::iter::<With<Billboard>>()`
        // and rendering would silently no-op. Visibility was already
        // filtered in `extract_billboards` via `InheritedVisibility`.
        for (entity, main_entity, uniform, mesh, image, billboard) in billboards.iter() {
            let Some(gpu_image) = gpu_images.get(image.id) else { continue };
            let Some(gpu_mesh) = gpu_meshes.get(mesh.id) else { continue };

            let mut key = BillboardPipelineKey::from_msaa_samples(msaa_samples);
            if billboard.depth.0 { key |= BillboardPipelineKey::DEPTH; }
            if let Some(lock) = billboard.lock_axis {
                if lock.y_axis { key |= BillboardPipelineKey::LOCK_Y; }
                if lock.rotation { key |= BillboardPipelineKey::LOCK_ROTATION; }
            }
            if extracted_camera.map_or(false, |c| c.hdr) { key |= BillboardPipelineKey::HDR; }

            let pipeline_id = match pipelines.specialize(&pipeline_cache, &pipeline, key, &gpu_mesh.layout) {
                Ok(id) => id,
                Err(err) => { error!("billboard pipeline specialize failed: {:?}", err); continue; }
            };

            // Distance for back-to-front sort. Pull world-space translation
            // from the model matrix's 4th column (the w_axis).
            let distance = rangefinder.distance(&uniform.transform.col(3).truncate());

            image_bind_groups.values.entry(image.id).or_insert_with(|| {
                render_device.create_bind_group(
                    Some("billboard_texture_bind_group"),
                    &pipeline.texture_layout,
                    &[
                        BindGroupEntry { binding: 0, resource: BindingResource::TextureView(&gpu_image.texture_view) },
                        BindGroupEntry { binding: 1, resource: BindingResource::Sampler(&gpu_image.sampler) },
                    ],
                )
            });

            // Bevy 0.18: `entity` is `(Entity, MainEntity)` and `indexed`
            // is required so phase sorting knows the draw call shape.
            transparent_phase.add(Transparent3d {
                pipeline: pipeline_id,
                entity: (entity, *main_entity),
                draw_function: draw_billboard,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                distance,
                indexed: true,
            });
        }
    }
}

// ============================================================================
// Draw commands
// ============================================================================

pub struct SetBillboardViewBindGroup<const I: usize>;
impl<const I: usize> RenderCommand<Transparent3d> for SetBillboardViewBindGroup<I> {
    type Param = ();
    type ViewQuery = (Read<ViewUniformOffset>, Read<BillboardViewBindGroup>);
    type ItemQuery = ();

    fn render<'w>(
        _item: &Transparent3d,
        (view_offset, view_bg): ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(I, &view_bg.value, &[view_offset.offset]);
        RenderCommandResult::Success
    }
}

pub struct SetBillboardBindGroup<const I: usize>;
impl<const I: usize> RenderCommand<Transparent3d> for SetBillboardBindGroup<I> {
    type Param = SRes<BillboardBindGroup>;
    type ViewQuery = ();
    type ItemQuery = (
        Read<DynamicUniformIndex<BillboardUniform>>,
        Read<DynamicUniformIndex<BillboardUv>>,
    );

    fn render<'w>(
        _item: &Transparent3d,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        indices: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        bg: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some((bb_index, uv_index)) = indices else {
            return RenderCommandResult::Failure("billboard dynamic index missing".into());
        };
        // Two dynamic offsets, in declared binding order (binding 0 first,
        // binding 1 second).
        pass.set_bind_group(I, &bg.into_inner().value, &[bb_index.index(), uv_index.index()]);
        RenderCommandResult::Success
    }
}

pub struct SetBillboardTextureBindGroup<const I: usize>;
impl<const I: usize> RenderCommand<Transparent3d> for SetBillboardTextureBindGroup<I> {
    type Param = SRes<BillboardImageBindGroups>;
    type ViewQuery = ();
    type ItemQuery = Read<RenderBillboardImage>;

    fn render<'w>(
        _item: &Transparent3d,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        texture: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        groups: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(texture) = texture else {
            return RenderCommandResult::Failure("billboard image missing".into());
        };
        let Some(bg) = groups.into_inner().values.get(&texture.id) else {
            return RenderCommandResult::Failure("billboard texture bind group missing".into());
        };
        pass.set_bind_group(I, bg, &[]);
        RenderCommandResult::Success
    }
}

/// Bevy 0.18 separates GPU mesh buffers from `RenderMesh`. The buffers
/// live in `MeshAllocator`'s slabs, which we look up by `AssetId<Mesh>`
/// to get the actual `Buffer` + `range` for vertex and index data.
pub struct DrawBillboardMesh;
impl RenderCommand<Transparent3d> for DrawBillboardMesh {
    type Param = (SRes<RenderAssets<RenderMesh>>, SRes<MeshAllocator>);
    type ViewQuery = ();
    type ItemQuery = Read<RenderBillboardMesh>;

    fn render<'w>(
        _item: &Transparent3d,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        mesh: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (meshes, mesh_allocator): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(mesh) = mesh else {
            return RenderCommandResult::Failure("billboard mesh missing".into());
        };
        let meshes = meshes.into_inner();
        let mesh_allocator = mesh_allocator.into_inner();
        let Some(gpu_mesh) = meshes.get(mesh.id) else {
            return RenderCommandResult::Failure("billboard gpu mesh not ready".into());
        };
        let Some(vertex_slice) = mesh_allocator.mesh_vertex_slice(&mesh.id) else {
            return RenderCommandResult::Failure("billboard vertex slab missing".into());
        };
        pass.set_vertex_buffer(0, vertex_slice.buffer.slice(..));
        match &gpu_mesh.buffer_info {
            RenderMeshBufferInfo::Indexed { count, index_format } => {
                let Some(index_slice) = mesh_allocator.mesh_index_slice(&mesh.id) else {
                    return RenderCommandResult::Failure("billboard index slab missing".into());
                };
                pass.set_index_buffer(index_slice.buffer.slice(..), *index_format);
                // Indices are drawn from the slab's element range, not
                // 0..count, since multiple meshes can share a slab.
                pass.draw_indexed(index_slice.range.start..(index_slice.range.start + *count), vertex_slice.range.start as i32, 0..1);
            }
            RenderMeshBufferInfo::NonIndexed => {
                pass.draw(vertex_slice.range.clone(), 0..1);
            }
        }
        RenderCommandResult::Success
    }
}

pub type DrawBillboard = (
    SetItemPipeline,
    SetBillboardViewBindGroup<0>,
    SetBillboardBindGroup<1>,
    SetBillboardTextureBindGroup<2>,
    DrawBillboardMesh,
);

// ============================================================================
// Plugin
// ============================================================================

pub struct BillboardPipelinePlugin;

impl Plugin for BillboardPipelinePlugin {
    fn build(&self, app: &mut App) {
        // Register the baked shader source in the MAIN world's
        // `Assets<Shader>` and stash the resulting strong handle in a
        // resource so the render world can read it. Bevy's render-asset
        // extraction will propagate the shader to the render side on
        // the first frame.
        let shader_handle: Handle<Shader> = {
            let mut shaders = app.world_mut().resource_mut::<Assets<Shader>>();
            shaders.add(Shader::from_wgsl(BILLBOARD_SHADER_WGSL, "billboard.wgsl"))
        };
        app.insert_resource(BillboardShaderHandle(shader_handle.clone()));
        app.sub_app_mut(RenderApp).insert_resource(BillboardShaderHandle(shader_handle));

        app.add_plugins((
            ExtractComponentPlugin::<BillboardDepth>::default(),
            ExtractComponentPlugin::<BillboardLockAxis>::default(),
            UniformComponentPlugin::<BillboardUniform>::default(),
            UniformComponentPlugin::<BillboardUv>::default(),
        ));

        let render_app = app.sub_app_mut(RenderApp);
        render_app
            .init_resource::<BillboardImageBindGroups>()
            .init_resource::<SpecializedMeshPipelines<BillboardPipeline>>()
            .add_render_command::<Transparent3d, DrawBillboard>()
            .add_systems(ExtractSchedule, extract_billboards)
            .add_systems(
                Render,
                (
                    queue_billboards.in_set(RenderSystems::Queue),
                    prepare_billboard_view_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                    prepare_billboard_bind_group.in_set(RenderSystems::PrepareBindGroups),
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        // Pipeline construction needs RenderDevice which only exists after
        // RenderApp finishes its own setup — that's why this lives in
        // `finish` and not `build`. The Strong shader handle is loaded
        // inside `BillboardPipeline::from_world` via the render-app's
        // AssetServer, so there's no extra load step here.
        app.sub_app_mut(RenderApp).init_resource::<BillboardPipeline>();
    }
}

// ============================================================================
// Mesh helpers (called from billboard_gui)
// ============================================================================

/// Build the unit billboard quad. Centred at origin in the XY plane,
/// vertices at ±0.5. The shader scales/positions per-vertex, so this mesh
/// is resolution-agnostic — the same handle drives every billboard.
pub fn build_billboard_quad_mesh() -> Mesh {
    use bevy::asset::RenderAssetUsages;

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5, -0.5, 0.0],
            [ 0.5, -0.5, 0.0],
            [ 0.5,  0.5, 0.0],
            [-0.5,  0.5, 0.0],
        ],
    );
    // UV origin is top-left in wgpu; row 0 of our pixel buffer is the top
    // row, so v=0 maps to the top verts (y=+0.5).
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    );
    mesh.insert_indices(Indices::U16(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

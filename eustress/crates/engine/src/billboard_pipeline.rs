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
//! - **Instanced draws**. Every billboard is an instance of the one shared
//!   quad; its placement, atlas tile and depth bias are per-instance vertex
//!   attributes. Each billboard is still its own sorted phase item, and after
//!   the sort every run of adjacent billboards draws with one call
//!   (`batch_billboard_instances`).
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
//! `extract_billboards` gathers every visible billboard into the render
//! world's `ExtractedBillboards` list; queueing, batching and drawing read that
//! list. No `StandardMaterial`, no `MeshMaterial3d`, no `face_camera` CPU
//! system.

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
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology, VertexBufferLayout};
use bevy::render::mesh::{RenderMesh, RenderMeshBufferInfo, allocator::MeshAllocator};
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{
    AddRenderCommand, DrawFunctions, PhaseItemExtraIndex, RenderCommand,
    RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
};
use bevy::render::render_resource::{
    BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingResource, BindingType, BlendComponent, BlendFactor, BlendOperation, BlendState,
    BufferBindingType, BufferUsages, ColorTargetState, ColorWrites, CompareFunction, DepthStencilState,
    FragmentState, FrontFace, MultisampleState, PipelineCache, PolygonMode, PrimitiveState,
    RenderPipelineDescriptor, SamplerBindingType, ShaderStages, ShaderType,
    SpecializedMeshPipeline, SpecializedMeshPipelineError, SpecializedMeshPipelines,
    TextureFormat, TextureSampleType, TextureViewDimension, VertexState,
    RawBufferVec, VertexAttribute, VertexFormat, VertexStepMode,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
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
/// - `SyncToRenderWorld` (Bevy 0.18+) — gives the billboard a render-world
///   counterpart, which its `Transparent3d` phase item names.
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

// 0.19: ExtractComponent now requires SyncComponent (main->render world sync).
impl bevy::render::sync_component::SyncComponent for BillboardDepth {
    type Target = Self;
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

impl bevy::render::sync_component::SyncComponent for BillboardLockAxis {
    type Target = Self;
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

/// One visible billboard, as extracted for this frame.
#[derive(Clone, Copy)]
pub struct ExtractedBillboard {
    render_entity: Entity,
    main_entity: MainEntity,
    instance: BillboardInstance,
    translation: Vec3,
    depth: bool,
    lock_axis: Option<BillboardLockAxis>,
    mesh: AssetId<Mesh>,
    image: AssetId<Image>,
}

/// Every visible billboard this frame, in extraction order. A billboard's
/// phase item carries its index here in `extra_index`
/// (`PhaseItemExtraIndex::DynamicOffset`), so queueing, batching and drawing
/// read one flat list. Per-entity render-world components cost a component
/// insert per billboard per frame to set up and a lookup per billboard per
/// step to read, on a list that is rebuilt every frame anyway.
#[derive(Resource, Default)]
pub struct ExtractedBillboards {
    items: Vec<ExtractedBillboard>,
}

impl ExtractedBillboards {
    /// The billboard a phase item was queued for.
    fn of(&self, item: &Transparent3d) -> Option<&ExtractedBillboard> {
        match &item.extra_index {
            PhaseItemExtraIndex::DynamicOffset(index) => self.items.get(*index as usize),
            _ => None,
        }
    }
}

// ============================================================================
// Resources (render world)
// ============================================================================

#[derive(Resource, Default)]
pub struct BillboardImageBindGroups {
    values: bevy::platform::collections::HashMap<AssetId<Image>, BindGroup>,
}

/// One billboard's per-instance vertex data (vertex buffer 1, step mode
/// Instance). The layout matches `Vertex` locations 2 to 7 in
/// `billboard.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BillboardInstance {
    /// Model matrix, by columns.
    model: [[f32; 4]; 4],
    /// Atlas tile: `uv_min` in xy, `uv_max` in zw.
    uv_rect: [f32; 4],
    /// x: depth bias in metres (`BillboardUv::z_bias`); yzw unused.
    params: [f32; 4],
}

impl BillboardInstance {
    fn new(uniform: &BillboardUniform, uv: &BillboardUv) -> Self {
        Self {
            model: uniform.transform.to_cols_array_2d(),
            uv_rect: [uv.uv_min.x, uv.uv_min.y, uv.uv_max.x, uv.uv_max.y],
            params: [uv.z_bias, 0.0, 0.0, 0.0],
        }
    }

    /// Six `vec4<f32>` attributes at shader locations 2 to 7.
    fn layout() -> VertexBufferLayout {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: VertexStepMode::Instance,
            attributes: (0..6u32)
                .map(|i| VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: u64::from(i) * 16,
                    shader_location: 2 + i,
                })
                .collect(),
        }
    }
}

/// Every billboard instance drawn this frame, across all views, in draw
/// order. Rebuilt each frame by `batch_billboard_instances`.
#[derive(Resource)]
pub struct BillboardInstanceBuffer {
    instances: RawBufferVec<BillboardInstance>,
}

impl Default for BillboardInstanceBuffer {
    fn default() -> Self {
        Self {
            instances: RawBufferVec::new(BufferUsages::VERTEX),
        }
    }
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
        /// The view's main texture is the swapchain surface in
        /// `Bgra8UnormSrgb` (Windows/Vulkan default) rather than the
        /// `bevy_default()` Rgba8 intermediate. Without this bit the
        /// pipeline's color target mismatched the pass and wgpu KILLED
        /// the app ("Incompatible color attachments … Bgra8UnormSrgb vs
        /// Rgba8UnormSrgb → Quitting due to Validation RenderError") the
        /// first time a billboard rendered on a non-HDR direct-to-surface
        /// view.
        const SURFACE_BGRA  = (1 << 4);
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
    texture_layout: BindGroupLayout,
    view_layout_desc: BindGroupLayoutDescriptor,
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
            texture_layout,
            view_layout_desc: BindGroupLayoutDescriptor {
                label: "billboard_view_layout".into(),
                entries: view_entries,
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
                self.texture_layout_desc.clone(),
            ],
            vertex: VertexState {
                shader: self.shader_handle.clone(),
                entry_point: Some("vertex".into()),
                // Buffer 0: the shared quad. Buffer 1: one `BillboardInstance`
                // per billboard, so a run of billboards is one draw.
                buffers: vec![vertex_buffer_layout, BillboardInstance::layout()],
                shader_defs: shader_defs.clone(),
            },
            fragment: Some(FragmentState {
                shader: self.shader_handle.clone(),
                entry_point: Some("fragment".into()),
                shader_defs,
                targets: vec![Some(ColorTargetState {
                    // MUST match the render pass's actual attachment format:
                    // HDR intermediate, the Bgra8 swapchain surface (Windows/
                    // Vulkan direct-to-surface views — see SURFACE_BGRA), or
                    // the Rgba8 bevy_default intermediate.
                    format: if key.contains(BillboardPipelineKey::HDR) {
                        bevy::render::view::ViewTarget::TEXTURE_FORMAT_HDR
                    } else if key.contains(BillboardPipelineKey::SURFACE_BGRA) {
                        TextureFormat::Bgra8UnormSrgb
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
                depth_write_enabled: Some(false), // never write — don't occlude future draws (wgpu 29: now Option<bool>)
                depth_compare: Some(depth_compare), // wgpu 29: now Option<CompareFunction>
                stencil: default(),
                bias: default(),
            }),
            multisample: MultisampleState {
                count: key.msaa_samples(),
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            immediate_size: 0, // 0.19: replaced push_constant_ranges (we used none)
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

/// Extract visible billboards into `ExtractedBillboards`.
///
/// Bevy separates main-world and render-world entity IDs. `SyncToRenderWorld`
/// (set via `Billboard`'s `#[require]`) keeps a render-world counterpart for
/// each billboard and stores its ID on the main-world entity as
/// `RenderEntity`; a billboard's phase item names that render entity.
///
/// Visibility filter uses `InheritedVisibility` (the post-parent-chain
/// user-facing visibility) rather than `ViewVisibility` (which requires
/// an `Aabb` for frustum culling — billboards don't have one because
/// their on-screen footprint is computed in the vertex shader). The
/// distance-based cull system in `billboard_gui` already handles "this
/// billboard is too far / too close" by toggling `Visibility::Hidden`,
/// which propagates into `InheritedVisibility`.
pub fn extract_billboards(
    mut extracted: ResMut<ExtractedBillboards>,
    query: Extract<
        Query<
            (
                Entity,
                &RenderEntity,
                &InheritedVisibility,
                &GlobalTransform,
                &Transform,
                &BillboardMesh,
                &BillboardAtlasTexture,
                &BillboardUv,
                Option<&BillboardDepth>,
                Option<&BillboardLockAxis>,
            ),
            With<Billboard>,
        >,
    >,
) {
    extracted.items.clear();
    for (entity, render_entity, inherited, global_tf, transform, mesh, texture, uv, depth, lock_axis) in &query {
        if !inherited.get() {
            continue;
        }
        let uniform = calculate_billboard_uniform(global_tf, transform, lock_axis);
        extracted.items.push(ExtractedBillboard {
            render_entity: render_entity.id(),
            main_entity: MainEntity::from(entity),
            instance: BillboardInstance::new(&uniform, uv),
            translation: uniform.transform.col(3).truncate(),
            depth: depth.copied().unwrap_or_default().0,
            lock_axis: lock_axis.copied(),
            mesh: mesh.0.id(),
            image: texture.0.id(),
        });
    }
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

/// Merge each run of consecutive billboards in the sorted `Transparent3d`
/// phase into one instanced draw.
///
/// Billboards stay individual phase items, so they sort against every other
/// transparent item exactly as before. After the sort, adjacent billboard
/// items that share a pipeline, mesh and atlas texture become one draw: the
/// run's first item gets a `batch_range` covering all of its instances, and
/// the sorted-phase renderer skips the rest of the run after drawing it. The
/// instances are written in the same order, so blending order within a run
/// is unchanged. A draw per billboard cost the render thread about 5 us
/// each, and a label-heavy Space draws several thousand.
fn batch_billboard_instances(
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    extracted: Res<ExtractedBillboards>,
    mut buffer: ResMut<BillboardInstanceBuffer>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    buffer.instances.clear();
    let Some(draw_billboard) = draw_functions.read().get_id::<DrawBillboard>() else {
        return;
    };
    for phase in phases.values_mut() {
        let len = phase.items.len();
        let mut start = 0;
        while start < len {
            let Some((_, first)) = phase.items.get_index(start) else { break };
            if first.draw_function != draw_billboard {
                start += 1;
                continue;
            }
            let pipeline = first.pipeline;
            let Some(first_billboard) = extracted.of(first) else {
                // No extracted billboard behind it: skip the item rather
                // than draw a different billboard's instance in its place.
                if let Some((_, item)) = phase.items.get_index_mut(start) {
                    item.batch_range = 0..0;
                }
                start += 1;
                continue;
            };
            let (mesh, image) = (first_billboard.mesh, first_billboard.image);
            let base = buffer.instances.len() as u32;
            let mut end = start;
            while end < len {
                let Some((_, item)) = phase.items.get_index(end) else { break };
                if item.draw_function != draw_billboard || item.pipeline != pipeline {
                    break;
                }
                let Some(billboard) = extracted.of(item) else { break };
                if billboard.mesh != mesh || billboard.image != image {
                    break;
                }
                buffer.instances.push(billboard.instance);
                end += 1;
            }
            let count = (end - start) as u32;
            if let Some((_, item)) = phase.items.get_index_mut(start) {
                item.batch_range = base..base + count;
            }
            start = end;
        }
    }
    buffer.instances.write_buffer(&render_device, &render_queue);
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
    views: Query<(
        Entity,
        &ExtractedView,
        Option<&ExtractedCamera>,
        Option<&Msaa>,
        // ViewTarget exists by Queue (created in RenderSet::ManageViews);
        // its main texture format drives the SURFACE_BGRA pipeline-key bit.
        Option<&bevy::render::view::ViewTarget>,
    )>,
    // Visibility was already filtered at extraction via `InheritedVisibility`.
    // Billboards bypass `RenderVisibleEntities`: Bevy's `check_visibility`
    // tracks specific render classes (Mesh3d, Sprite, …) and doesn't know
    // our `Billboard` marker, so it would never list them.
    extracted: Res<ExtractedBillboards>,
) {
    // Clear the bind-group cache each frame. When the atlas grows
    // (`BillboardAtlas::try_grow`), the underlying GpuImage is rebuilt
    // with new dimensions and the cached bind group's `TextureView`
    // references the OLD texture — sampling stale data. Rebuilding
    // every frame is cheap (we share one atlas, so it's a single
    // `create_bind_group` call) and guarantees correctness on resize.
    image_bind_groups.values.clear();
    let draw_billboard = transparent_draw_functions.read().id::<DrawBillboard>();

    for (_view_entity, view, extracted_camera, msaa, view_target) in views.iter() {
        // `ViewSortedRenderPhases` is keyed by `RetainedViewEntity` (stable
        // across the main→render extract), not the render-world Entity.
        let Some(transparent_phase) = transparent_phases.get_mut(&view.retained_view_entity) else { continue };
        let rangefinder = view.rangefinder3d();

        let mut view_key =
            BillboardPipelineKey::from_msaa_samples(msaa.copied().unwrap_or_default().samples());
        if extracted_camera.map_or(false, |c| c.hdr) {
            view_key |= BillboardPipelineKey::HDR;
        }
        // Key on the view's ACTUAL attachment format: non-HDR views can
        // render straight to the Bgra8 swapchain surface, and a pipeline
        // built for Rgba8 there is a fatal wgpu validation error (the
        // "engine closes itself 5s after load" crash).
        if view_target.map_or(false, |t| t.main_texture_format() == TextureFormat::Bgra8UnormSrgb) {
            view_key |= BillboardPipelineKey::SURFACE_BGRA;
        }
        // A handful of variants serve every billboard: specialize each
        // (key, mesh) pair once per view, not once per billboard.
        let mut specialized: Vec<(
            BillboardPipelineKey,
            AssetId<Mesh>,
            bevy::render::render_resource::CachedRenderPipelineId,
        )> = Vec::new();

        for (index, billboard) in extracted.items.iter().enumerate() {
            let Some(gpu_image) = gpu_images.get(billboard.image) else { continue };
            let Some(gpu_mesh) = gpu_meshes.get(billboard.mesh) else { continue };

            let mut key = view_key;
            if billboard.depth {
                key |= BillboardPipelineKey::DEPTH;
            }
            if let Some(lock) = billboard.lock_axis {
                if lock.y_axis {
                    key |= BillboardPipelineKey::LOCK_Y;
                }
                if lock.rotation {
                    key |= BillboardPipelineKey::LOCK_ROTATION;
                }
            }
            let cached = specialized
                .iter()
                .find(|(k, mesh, _)| *k == key && *mesh == billboard.mesh)
                .map(|(_, _, id)| *id);
            let pipeline_id = match cached {
                Some(id) => id,
                None => match pipelines.specialize(&pipeline_cache, &pipeline, key, &gpu_mesh.layout) {
                    Ok(id) => {
                        specialized.push((key, billboard.mesh, id));
                        id
                    }
                    Err(err) => {
                        error!("billboard pipeline specialize failed: {:?}", err);
                        continue;
                    }
                },
            };

            image_bind_groups.values.entry(billboard.image).or_insert_with(|| {
                render_device.create_bind_group(
                    Some("billboard_texture_bind_group"),
                    &pipeline.texture_layout,
                    &[
                        BindGroupEntry { binding: 0, resource: BindingResource::TextureView(&gpu_image.texture_view) },
                        BindGroupEntry { binding: 1, resource: BindingResource::Sampler(&gpu_image.sampler) },
                    ],
                )
            });

            // Back-to-front by distance. `extra_index` names this billboard
            // in `ExtractedBillboards` for the batch and draw steps.
            transparent_phase.add_transient(Transparent3d {
                sorting_info: bevy::core_pipeline::core_3d::TransparentSortingInfo3d::Sorted {
                    mesh_center: billboard.translation,
                    depth_bias: 0.0,
                },
                pipeline: pipeline_id,
                entity: (billboard.render_entity, billboard.main_entity),
                draw_function: draw_billboard,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::DynamicOffset(index as u32),
                distance: rangefinder.distance(&billboard.translation),
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

pub struct SetBillboardTextureBindGroup<const I: usize>;
impl<const I: usize> RenderCommand<Transparent3d> for SetBillboardTextureBindGroup<I> {
    type Param = (SRes<BillboardImageBindGroups>, SRes<ExtractedBillboards>);
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        item: &Transparent3d,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (groups, extracted): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(billboard) = extracted.into_inner().of(item) else {
            return RenderCommandResult::Failure("billboard not extracted".into());
        };
        let Some(bg) = groups.into_inner().values.get(&billboard.image) else {
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
    type Param = (
        SRes<RenderAssets<RenderMesh>>,
        SRes<MeshAllocator>,
        SRes<BillboardInstanceBuffer>,
        SRes<ExtractedBillboards>,
    );
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        item: &Transparent3d,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (meshes, mesh_allocator, instances, extracted): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(billboard) = extracted.into_inner().of(item) else {
            return RenderCommandResult::Failure("billboard not extracted".into());
        };
        let mesh_id = billboard.mesh;
        let meshes = meshes.into_inner();
        let mesh_allocator = mesh_allocator.into_inner();
        let Some(gpu_mesh) = meshes.get(mesh_id) else {
            return RenderCommandResult::Failure("billboard gpu mesh not ready".into());
        };
        let Some(vertex_slice) = mesh_allocator.mesh_vertex_slice(&mesh_id) else {
            return RenderCommandResult::Failure("billboard vertex slab missing".into());
        };
        let Some(instance_buffer) = instances.into_inner().instances.buffer() else {
            return RenderCommandResult::Failure("billboard instance buffer missing".into());
        };
        // This item's run of billboards (see `batch_billboard_instances`).
        let instance_range = item.batch_range.clone();
        pass.set_vertex_buffer(0, vertex_slice.buffer.slice(..));
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        match &gpu_mesh.buffer_info {
            RenderMeshBufferInfo::Indexed { count, index_format } => {
                let Some(index_slice) = mesh_allocator.mesh_index_slice(&mesh_id) else {
                    return RenderCommandResult::Failure("billboard index slab missing".into());
                };
                pass.set_index_buffer(index_slice.buffer.slice(..), *index_format);
                // Indices are drawn from the slab's element range, not
                // 0..count, since multiple meshes can share a slab.
                pass.draw_indexed(index_slice.range.start..(index_slice.range.start + *count), vertex_slice.range.start as i32, instance_range);
            }
            RenderMeshBufferInfo::NonIndexed => {
                pass.draw(vertex_slice.range.clone(), instance_range);
            }
        }
        RenderCommandResult::Success
    }
}

pub type DrawBillboard = (
    SetItemPipeline,
    SetBillboardViewBindGroup<0>,
    SetBillboardTextureBindGroup<1>,
    DrawBillboardMesh,
);

// ============================================================================
// Plugin
// ============================================================================

pub struct BillboardPipelinePlugin;

/// Render-world half of the per-tile atlas upload: `write_texture` each
/// staged 192×192 tile straight onto the GPU atlas. This replaces the old
/// whole-Image-asset mutation, which re-uploaded the ENTIRE atlas
/// (16384-wide × rows ≈ ~100 MB at 8 rows) for a single repainted tile —
/// and flying through a dense world repaints tiles every frame.
/// Quality-identical: the same premultiplied RGBA8 bytes reach the same
/// texels; only the transfer granularity changes.
fn write_billboard_atlas_tiles(
    pending: Res<crate::billboard_gui::PendingAtlasTiles>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    queue: Res<bevy::render::renderer::RenderQueue>,
) {
    if pending.tiles.is_empty() {
        return;
    }
    let Some(atlas_id) = pending.atlas else { return };
    // GpuImage not prepared yet (first frames / grow frames use the
    // full-asset path anyway) — skip; nothing is lost because the full
    // upload covers every tile.
    let Some(gpu) = gpu_images.get(atlas_id) else { return };

    use bevy::render::render_resource::{
        Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
    };
    const TILE_W: u32 = crate::billboard_gui::TILE_W;
    const TILE_H: u32 = crate::billboard_gui::TILE_H;

    for tile in &pending.tiles {
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &gpu.texture,
                mip_level: 0,
                origin: Origin3d { x: tile.x, y: tile.y, z: 0 },
                aspect: TextureAspect::All,
            },
            &tile.data,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TILE_W * 4),
                rows_per_image: Some(TILE_H),
            },
            Extent3d {
                width: TILE_W,
                height: TILE_H,
                depth_or_array_layers: 1,
            },
        );
    }
}

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
            // Per-tile atlas uploads staged by billboard_gui's
            // `upload_atlas_to_gpu`; written by `write_billboard_atlas_tiles`
            // below via direct `write_texture` (no Image-asset mutation → no
            // full-atlas re-upload per repainted tile).
            bevy::render::extract_resource::ExtractResourcePlugin::<
                crate::billboard_gui::PendingAtlasTiles,
            >::default(),
        ));

        let render_app = app.sub_app_mut(RenderApp);
        render_app
            .init_resource::<BillboardImageBindGroups>()
            .init_resource::<BillboardInstanceBuffer>()
            .init_resource::<ExtractedBillboards>()
            .init_resource::<SpecializedMeshPipelines<BillboardPipeline>>()
            .add_render_command::<Transparent3d, DrawBillboard>()
            .add_systems(ExtractSchedule, extract_billboards)
            .add_systems(
                Render,
                (
                    queue_billboards.in_set(RenderSystems::Queue),
                    // After the phase sort, so a run is adjacent in draw order.
                    batch_billboard_instances.in_set(RenderSystems::PrepareResourcesBatchPhases),
                    prepare_billboard_view_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                    // After PrepareAssets (GpuImage current, incl. post-grow
                    // recreation); queue-ordered writes land before draws.
                    write_billboard_atlas_tiles.in_set(RenderSystems::PrepareResources),
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

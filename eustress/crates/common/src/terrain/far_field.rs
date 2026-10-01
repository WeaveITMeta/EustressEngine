//! The far field: the rest of a streaming terrain, drawn on the GPU.
//!
//! A terrain too large to keep every chunk (see `raster_fully_resident`)
//! keeps its CPU chunks, with their meshes and colliders, only near the scene
//! camera. Everything beyond is drawn by its far field: a geometry clipmap of
//! a few static grid meshes that follow the camera, lifted on the GPU onto the
//! root's height texture and shaded by the same material map, slot records
//! and texture arrays as the chunks, so the whole map shows at any distance
//! for a small, fixed cost.
//!
//! ## Levels
//!
//! Level 0 is a square of 2M x 2M quads (M = [`FAR_FIELD_HALF_QUADS`]); every
//! further level is a ring of the same outline with a hole of 2H x 2H quads
//! (H = [`FAR_FIELD_HOLE_HALF_QUADS`]). The two meshes are built once, in
//! unit spacing, and every far field shares them ([`far_field_level_mesh`]).
//! Level L has quads of s_L = s_0 * 2^L metres and is centred on the camera
//! snapped to multiples of 2 * s_L, so its vertices never swim and lie on
//! the next level's lattice ([`far_field_level_transform`]). s_0 makes
//! level 0 about as wide as the near disc and is never finer than a raster
//! cell; levels are added until the outermost reaches the footprint corner
//! farthest from the camera, [`MAX_FAR_FIELD_LEVELS`] at most
//! ([`far_field_layout`]).
//!
//! Level L - 1 reaches M/2 of level L's quads from its own centre, and the
//! two centres can sit one level-L quad apart, since each is snapped to its
//! own step. A hole of M/2 - 2 quads therefore leaves at least one quad of
//! overlap on every side, where the finer level draws over the coarser one.
//! A hole of M/2 - 1 would leave none on the side the centres part toward,
//! and the two edges would meet at T-junctions that crack once lifted.
//!
//! ## Drawing
//!
//! [`TerrainFarFieldMaterial`] is the textured surface material with the
//! height texture and [`TerrainFarFieldParams`] added. Its vertex stage,
//! `terrain_far_field.wgsl`, lifts each vertex (held to the terrain's
//! footprint, so the far field ends where the terrain does) to the height
//! texture's bilinear height there, and takes the normal from the heights a
//! raster cell to either side. Its fragment stage is `terrain_surface.wgsl`
//! compiled with [`TERRAIN_FAR_FIELD_SHADER_DEF`], which drops the pixels
//! inside the near disc, where the chunks draw, and over a sparse raster's
//! holes (`TerrainData::sparse_surface`). The far field's surface params
//! always carry `TERRAIN_SURFACE_FLAG_TEXTURED`: it has no vertex colours to
//! fall back on, and until the texture arrays are bound every slot record is
//! flat, so the material-map path paints each slot in the swatch the chunks'
//! vertex colours show.
//!
//! Level L sits (L + 1) sinks ([`far_field_sink`]) plus a small share of its
//! own quad ([`far_field_level_drop`]) below the ground it samples, so where
//! it overlaps the chunks or a finer level, those win the depth test even
//! where a coarse quad bridges a valley. The lowering travels in the level's translation Y, which the
//! vertex stage adds to the height it samples. The far field takes no part
//! in the depth prepass, the deferred G-buffer or the shadow passes, whose
//! default vertex stages would draw it flat; it casts no shadow, skips
//! frustum culling (its flat bounds are wrong once lifted), has no collider
//! and no `Instance`, and its level entities are not children of the root,
//! since the surface systems treat every child of a root as a chunk.
//!
//! ## When a root has one
//!
//! A root draws a far field while [`TerrainFarFieldSettings::enabled`], it
//! has a height raster that is not fully resident, its `TerrainSurface`'s
//! material map is on the GPU and its height texture is ready
//! ([`far_field_state`]). Its height texture is asked for
//! (`TerrainHeightTextureRequest::far_field_roots`) as soon as it has a
//! surface, so the texture settles alongside the map. The material binds
//! what the surface material binds (the bound arrays and slot records of
//! `TerrainSurfaceBindings`, the settled map and height texture) and follows
//! them every frame. Once the far field has drawn for
//! `SURFACE_SETTLE_FRAMES` frames the root carries [`TerrainFarFieldActive`],
//! so chunk streaming may stop at the near radius.

use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::material::OpaqueRendererMethod;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
// Explicit, so the log macros do not depend on the prelude's `bevy_log`
// feature (see `avatar::boot`).
use tracing::{debug, info};

use super::surface_material::{
    sync_terrain_height_textures, sync_terrain_surfaces, TerrainHeightTexture, TerrainHeightTextureRequest,
    TerrainSurface, TerrainSurfaceBindings, TerrainSurfaceParams, SURFACE_SETTLE_FRAMES, TERRAIN_SURFACE_SHADER_PATH,
};
use super::texture_arrays::TerrainTextureArraySet;
use super::{
    raster_fully_resident, scene_camera_translation, surface_data, terrain_standard_material, TerrainBaked,
    TerrainConfig, TerrainData, TerrainRoot,
};

/// Asset path of the embedded `terrain_far_field.wgsl`.
pub const TERRAIN_FAR_FIELD_SHADER_PATH: &str = "embedded://eustress_common/terrain/terrain_far_field.wgsl";

/// Shader def both stages of the far-field pipeline are compiled with: it
/// switches on the far field's blocks of `terrain_surface.wgsl`.
pub const TERRAIN_FAR_FIELD_SHADER_DEF: &str = "TERRAIN_FAR_FIELD";

/// Quads from a level's centre to its edge (M): every level spans 2M quads a
/// side.
pub const FAR_FIELD_HALF_QUADS: u32 = 32;

/// Quads from a ring's centre to the edge of its hole, M/2 - 2, which leaves
/// at least one quad of overlap with the level inside it (see the module
/// docs).
pub const FAR_FIELD_HOLE_HALF_QUADS: u32 = FAR_FIELD_HALF_QUADS / 2 - 2;

/// Most levels a far field has.
pub const MAX_FAR_FIELD_LEVELS: u32 = 14;

/// Least quad size of level 0, and least raster cell, metres: a floor under
/// a degenerate raster.
const MIN_SPACING: f32 = 0.01;

/// Each level's lowering as a share of a chunk quad
/// (`chunk_size / chunk_resolution`), and its least value in metres.
const SINK_PER_CHUNK_QUAD: f32 = 0.05;
const MIN_SINK: f32 = 0.02;

/// Share of a level's own quad width it is lowered by, on top of its sinks
/// (see [`far_field_level_drop`]).
pub const SINK_PER_LEVEL_QUAD: f32 = 0.02;

/// How far the camera may move, as a share of a chunk, before the near disc
/// follows it. Moving the disc rewrites the material, which re-prepares its
/// bind group, so it follows in steps; the disc then trails the camera by up
/// to an eighth of a chunk, well inside the chunk of margin the near radius
/// leaves the chunks (see [`far_field_near_radius`]).
const NEAR_CENTRE_STEP: f32 = 0.125;

// ============================================================================
// GPU layout
// ============================================================================

/// The far field's own shader inputs: `TerrainFarFieldParams` in
/// `terrain_far_field.wgsl` and in `terrain_surface.wgsl`, binding 108.
/// `far_field_params_layout_matches_both_wgsl_structs` holds the three
/// layouts together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect, ShaderType)]
pub struct TerrainFarFieldParams {
    /// World XZ of the scene camera: the centre of the near disc, which
    /// follows the camera in steps of an eighth of a chunk. The levels are
    /// centred on the camera itself, each snapped to its own step.
    pub camera_xz: Vec2,
    /// Pixels nearer `camera_xz` than this, in XZ, are dropped: the chunks
    /// draw there ([`far_field_near_radius`]).
    pub near_radius: f32,
    /// Metres each level sits below the one inside it ([`far_field_sink`]);
    /// level L's whole lowering ([`far_field_level_drop`]) is carried in its
    /// translation's Y.
    pub sink: f32,
    /// World size of one raster cell, the spacing of the heights the vertex
    /// stage takes each normal from.
    pub cell: Vec2,
    /// 1 when the raster is sparse (`TerrainData::sparse_surface`): a cell
    /// whose id_a is "no material" is a hole, and the far field draws nothing
    /// over it.
    pub sparse: u32,
    pub _pad: u32,
}

// ============================================================================
// The material
// ============================================================================

/// What the far field adds to the `StandardMaterial` bindings: the surface
/// extension's bindings 100 to 106 with the same types, samplers and
/// visibility, so `terrain_surface.wgsl` runs unchanged, then the height
/// texture and the far field's params. The binding numbers and sample types
/// are the ones `terrain_far_field.wgsl` and `terrain_surface.wgsl` declare;
/// change them together.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, PartialEq)]
pub struct TerrainFarFieldExtension {
    /// sRGB albedo array, `None` until the arrays are bound (a fallback image
    /// stands in, and the flat slot records never read it). Its sampler
    /// serves all three arrays.
    #[texture(100, dimension = "2d_array", visibility(fragment))]
    #[sampler(103, visibility(fragment))]
    pub albedo: Option<Handle<Image>>,
    /// Tangent-space normal array.
    #[texture(101, dimension = "2d_array", visibility(fragment))]
    pub normal: Option<Handle<Image>>,
    /// Occlusion, roughness, metallic array.
    #[texture(102, dimension = "2d_array", visibility(fragment))]
    pub orm: Option<Handle<Image>>,
    /// The root's material map, the one its `TerrainSurface` uploads.
    #[texture(104, visibility(fragment))]
    pub material_map: Option<Handle<Image>>,
    /// The slot records every surface shares.
    #[storage(105, read_only, visibility(fragment))]
    pub slot_records: Handle<ShaderBuffer>,
    /// Where the material map and the height texture lie. The vertex stage
    /// reads it too; a uniform is visible to every stage.
    #[uniform(106)]
    pub params: TerrainSurfaceParams,
    /// The root's height texture, read texel by texel with `textureLoad` by
    /// the vertex stage: R32Float cannot be filtered on every adapter, so the
    /// binding is non-filterable.
    #[texture(107, sample_type = "float", filterable = false, visibility(vertex))]
    pub heights: Option<Handle<Image>>,
    /// The far field's own inputs.
    #[uniform(108)]
    pub far: TerrainFarFieldParams,
}

impl MaterialExtension for TerrainFarFieldExtension {
    fn vertex_shader() -> ShaderRef {
        TERRAIN_FAR_FIELD_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        TERRAIN_SURFACE_SHADER_PATH.into()
    }

    // The default prepass, deferred and shadow vertex stages would draw the
    // levels flat, so the far field stays out of all three.
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.shader_defs.push(TERRAIN_FAR_FIELD_SHADER_DEF.into());
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader_defs.push(TERRAIN_FAR_FIELD_SHADER_DEF.into());
        }
        Ok(())
    }
}

/// The far-field material. See the module docs.
pub type TerrainFarFieldMaterial = ExtendedMaterial<StandardMaterial, TerrainFarFieldExtension>;

/// The `StandardMaterial` under the far field: the terrain's, drawn forward,
/// so an app that renders deferred never puts the far field in a G-buffer
/// pass.
fn far_field_base_material() -> StandardMaterial {
    StandardMaterial { opaque_render_method: OpaqueRendererMethod::Forward, ..terrain_standard_material() }
}

/// The far-field extension binding `arrays` (none when `None`), the root's
/// `material_map` and `heights`, the shared `slot_records`, and the params.
fn far_field_extension(
    arrays: Option<&TerrainTextureArraySet>,
    material_map: Handle<Image>,
    slot_records: Handle<ShaderBuffer>,
    params: TerrainSurfaceParams,
    heights: Handle<Image>,
    far: TerrainFarFieldParams,
) -> TerrainFarFieldExtension {
    TerrainFarFieldExtension {
        albedo: arrays.map(|set| set.albedo.clone()),
        normal: arrays.map(|set| set.normal.clone()),
        orm: arrays.map(|set| set.orm.clone()),
        material_map: Some(material_map),
        slot_records,
        params,
        heights: Some(heights),
        far,
    }
}

// ============================================================================
// State
// ============================================================================

/// Whether terrain roots may draw far fields at all. Off, every root loses
/// its far field (and with it [`TerrainFarFieldActive`], so its chunks reach
/// the whole map again); on, a root draws one whenever it qualifies (see the
/// module docs). It starts on unless the environment variable named by
/// [`FAR_FIELD_ENV`] is `0`, `off`, `false` or `no`: a switch that needs no
/// rebuild, for a GPU whose driver cannot run the far-field shader (a
/// shader that fails to build draws nothing, and the main world cannot see
/// that).
#[derive(Resource, Clone, Debug)]
pub struct TerrainFarFieldSettings {
    pub enabled: bool,
}

/// Environment variable that can switch the far field off at startup (see
/// [`TerrainFarFieldSettings`]).
pub const FAR_FIELD_ENV: &str = "EUSTRESS_TERRAIN_FAR_FIELD";

impl Default for TerrainFarFieldSettings {
    fn default() -> Self {
        Self { enabled: far_field_enabled_by(std::env::var(FAR_FIELD_ENV).ok().as_deref()) }
    }
}

/// Whether a [`FAR_FIELD_ENV`] value leaves the far field on: unset, or
/// anything but `0`, `off`, `false` or `no` (case-insensitive).
fn far_field_enabled_by(value: Option<&str>) -> bool {
    !value.is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "0" | "off" | "false" | "no"))
}

/// On a terrain root while its far field is drawn: the chunks it needs up
/// close stop at the near radius, and the far field shows the rest.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct TerrainFarFieldActive {
    /// XZ distance from the scene camera inside which the far field draws
    /// nothing, so the chunks must cover that disc. The disc's centre
    /// follows the camera in steps of an eighth of a chunk, which the one
    /// chunk this radius stays short of `TerrainConfig::view_distance`
    /// absorbs ([`far_field_near_radius`]).
    pub near_radius: f32,
}

/// A terrain root's far field: its material and level entities. Present
/// while the root draws one; removed, with its levels despawned, when it
/// stops.
#[derive(Component, Debug)]
pub struct TerrainFarField {
    material: Handle<TerrainFarFieldMaterial>,
    /// The level entities, level 0 first.
    levels: Vec<Entity>,
    layout: FarFieldLayout,
    /// World XZ the near disc is centred on (see [`NEAR_CENTRE_STEP`]).
    near_centre: Vec2,
    /// Frames since the far field was spawned.
    age_frames: u32,
}

impl TerrainFarField {
    /// The material every level of this far field wears.
    pub fn material(&self) -> &Handle<TerrainFarFieldMaterial> {
        &self.material
    }

    /// The level entities, level 0 first.
    pub fn levels(&self) -> &[Entity] {
        &self.levels
    }

    /// The levels' base spacing and count.
    pub fn layout(&self) -> FarFieldLayout {
        self.layout
    }
}

/// One level of a far field: its root and its level number. The entity is
/// not a child of the root (see the module docs); a level whose root no
/// longer lists it is despawned.
#[derive(Component, Clone, Copy, Debug)]
pub struct TerrainFarFieldLevel {
    pub root: Entity,
    pub level: u32,
}

/// The two meshes every far field shares (see [`far_field_level_mesh`]),
/// made once by [`sync_terrain_far_fields`] and kept in its `Local`. Public
/// because that system's signature names it.
pub struct FarFieldMeshes {
    /// Level 0: the whole square.
    square: Handle<Mesh>,
    /// Every level past 0: the ring.
    ring: Handle<Mesh>,
}

impl FarFieldMeshes {
    fn new(meshes: &mut Assets<Mesh>) -> Self {
        Self {
            square: meshes.add(far_field_level_mesh(FAR_FIELD_HALF_QUADS, 0)),
            ring: meshes.add(far_field_level_mesh(FAR_FIELD_HALF_QUADS, FAR_FIELD_HOLE_HALF_QUADS)),
        }
    }
}

// ============================================================================
// Geometry
// ============================================================================

/// A far-field level's mesh in unit spacing: the 2 `half` x 2 `half` quads of
/// [-`half`, `half`]^2 in local XZ at Y 0, less the quads inside
/// [-`hole`, `hole`]^2 (none when `hole` is 0), two up-facing triangles per
/// quad. Every lattice point is a vertex, so the hole's interior points go
/// unused.
///
/// Positions only: with UVs, `StandardMaterial`'s implicit-gradient texture
/// reads would compile into the far field's fragment stage after its discard,
/// where they are not allowed.
pub fn far_field_level_mesh(half: u32, hole: u32) -> Mesh {
    let side = 2 * half + 1;
    let offset = half as f32;
    let mut positions = Vec::with_capacity((side * side) as usize);
    for z in 0..side {
        for x in 0..side {
            positions.push([x as f32 - offset, 0.0, z as f32 - offset]);
        }
    }
    let (half, hole) = (i64::from(half), i64::from(hole));
    let mut indices = Vec::new();
    for z in 0..2 * half {
        for x in 0..2 * half {
            // The quad's lower corner, from the centre.
            let (qx, qz) = (x - half, z - half);
            if qx >= -hole && qx < hole && qz >= -hole && qz < hole {
                continue;
            }
            let v00 = (z * i64::from(side) + x) as u32;
            let v10 = v00 + 1;
            let v01 = v00 + side;
            let v11 = v01 + 1;
            // Counter-clockwise seen from above (+Y), Bevy's front face.
            indices.extend_from_slice(&[v00, v01, v10, v10, v01, v11]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// How a far field's levels are laid out: level 0's quad size and how many
/// levels there are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FarFieldLayout {
    /// s_0, metres.
    pub base_spacing: f32,
    /// Levels drawn, 1 to [`MAX_FAR_FIELD_LEVELS`].
    pub levels: u32,
}

/// The layout for a near disc of `near_radius` metres, a raster cell of
/// `raster_cell` metres and a footprint whose farthest corner is `farthest`
/// metres from the camera. Level 0 spans about the near disc (M quads of
/// `near_radius / M`), never finer than a raster cell. A level reaches M - 1
/// of its quads from the camera whatever the snapping, since its centre sits
/// up to a quad from it; levels are added until one reaches `farthest`, or
/// [`MAX_FAR_FIELD_LEVELS`] are.
pub fn far_field_layout(near_radius: f32, raster_cell: f32, farthest: f32) -> FarFieldLayout {
    let base_spacing = (near_radius / FAR_FIELD_HALF_QUADS as f32).max(raster_cell).max(MIN_SPACING);
    let reach = (FAR_FIELD_HALF_QUADS - 1) as f32 * base_spacing;
    let levels = (0..MAX_FAR_FIELD_LEVELS)
        .find(|&level| reach * 2f32.powi(level as i32) >= farthest)
        .map_or(MAX_FAR_FIELD_LEVELS, |level| level + 1);
    FarFieldLayout { base_spacing, levels }
}

/// Quad size of level `level`, s_0 * 2^level.
pub fn level_spacing(base_spacing: f32, level: u32) -> f32 {
    base_spacing * 2f32.powi(level as i32)
}

/// `value` rounded to the nearest multiple of `step`; `value` itself when
/// `step` is not a positive number.
pub fn snap_to_step(value: f32, step: f32) -> f32 {
    if step > 0.0 && step.is_finite() {
        (value / step).round() * step
    } else {
        value
    }
}

/// Level `level`'s transform for a camera at world `camera_xz`: scaled to
/// quads of its spacing, centred on the camera snapped to twice its spacing
/// (so its vertices stay on the next level's lattice, and it only moves in
/// whole steps of two quads), and lowered by [`far_field_level_drop`], which
/// the vertex stage adds to the height it samples.
pub fn far_field_level_transform(camera_xz: Vec2, base_spacing: f32, level: u32, sink: f32) -> Transform {
    let spacing = level_spacing(base_spacing, level);
    let step = 2.0 * spacing;
    Transform {
        translation: Vec3::new(
            snap_to_step(camera_xz.x, step),
            -far_field_level_drop(spacing, level, sink),
            snap_to_step(camera_xz.y, step),
        ),
        rotation: Quat::IDENTITY,
        scale: Vec3::new(spacing, 1.0, spacing),
    }
}

/// Metres level `level`, of quads `spacing` metres wide, sits below the
/// ground it samples: (`level` + 1) `sink`s, so each level sits under the one
/// inside it, plus [`SINK_PER_LEVEL_QUAD`] of its own quad. A coarse quad
/// spans a valley with one straight edge that can rise above the finer ground
/// there, by more the wider it is; the share of its width keeps it under the
/// chunks and finer levels where they overlap, and a drop that small cannot
/// be seen at the distances such a level draws.
pub fn far_field_level_drop(spacing: f32, level: u32, sink: f32) -> f32 {
    let drop = sink * (level + 1) as f32 + SINK_PER_LEVEL_QUAD * spacing;
    if drop.is_finite() {
        drop.max(0.0)
    } else {
        sink.max(0.0)
    }
}

/// Radius of the near disc, where the chunks draw and the far field draws
/// nothing: `TerrainConfig::view_distance` (how far the chunks stream) less
/// one chunk, never below zero.
pub fn far_field_near_radius(config: &TerrainConfig) -> f32 {
    let radius = config.view_distance - config.chunk_size;
    if radius.is_finite() {
        radius.max(0.0)
    } else {
        0.0
    }
}

/// Metres each level sits below the one inside it: a twentieth of a chunk
/// quad, at least 2 cm.
pub fn far_field_sink(config: &TerrainConfig) -> f32 {
    let sink = SINK_PER_CHUNK_QUAD * config.chunk_size / config.chunk_resolution.max(1) as f32;
    if sink.is_finite() {
        sink.max(MIN_SINK)
    } else {
        MIN_SINK
    }
}

/// Distance in XZ from `from` to the corner of `config`'s footprint farthest
/// from it.
pub fn farthest_corner_distance(config: &TerrainConfig, from: Vec2) -> f32 {
    let (min, max) = config.footprint_xz();
    [min, Vec2::new(max.x, min.y), Vec2::new(min.x, max.y), max]
        .into_iter()
        .map(|corner| corner.distance(from))
        .fold(0.0, f32::max)
}

/// World size of one raster cell under `params`: the footprint over the
/// cells between its first and last texel on each axis.
pub fn raster_cell_size(params: &TerrainSurfaceParams) -> Vec2 {
    let cells = params.cache_size.saturating_sub(UVec2::ONE).max(UVec2::ONE).as_vec2();
    (params.world_extent / cells).max(Vec2::splat(MIN_SPACING))
}

/// The near disc's centre: `centre` while the camera stays within `step` of
/// it, else the camera.
fn follow_camera(centre: Vec2, camera: Vec2, step: f32) -> Vec2 {
    if centre.distance(camera) <= step {
        centre
    } else {
        camera
    }
}

// ============================================================================
// Activation
// ============================================================================

/// What a terrain root's far field does this frame (see [`far_field_state`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FarFieldState {
    /// No far field: switched off, its chunks cover the whole terrain, it has
    /// no raster to lift one onto, or no textured surface to shade it with.
    Off,
    /// A far field is wanted, but its material map or height texture is not
    /// on the GPU yet: the height texture is asked for, nothing is drawn.
    Waiting,
    /// The far field draws.
    Drawing,
}

/// What decides a root's [`FarFieldState`].
#[derive(Clone, Copy, Debug)]
pub struct FarFieldInputs {
    /// [`TerrainFarFieldSettings::enabled`], and there is a scene camera to
    /// centre on.
    pub enabled: bool,
    /// The root has a height raster (procedural terrain has none).
    pub has_raster: bool,
    /// `raster_fully_resident`: every chunk stays spawned.
    pub fully_resident: bool,
    /// The root's material map: its size, and whether it is on the GPU.
    /// `None` without a `TerrainSurface`.
    pub material_map: Option<(UVec2, bool)>,
    /// The root's height texture: its size, and whether a material may bind
    /// it. `None` without one.
    pub height_texture: Option<(UVec2, bool)>,
}

/// Whether a root draws a far field, waits for one, or has none. A root that
/// streams its chunks, has a height raster and a textured surface wants one;
/// it draws once its material map and height texture are both on the GPU at
/// the same size (right after the raster is resized, one of them is still
/// the old size).
pub fn far_field_state(inputs: &FarFieldInputs) -> FarFieldState {
    if !inputs.enabled || !inputs.has_raster || inputs.fully_resident {
        return FarFieldState::Off;
    }
    let Some((map_size, map_ready)) = inputs.material_map else {
        return FarFieldState::Off;
    };
    match inputs.height_texture {
        Some((size, ready)) if map_ready && ready && size == map_size => FarFieldState::Drawing,
        _ => FarFieldState::Waiting,
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Spawn the levels of a far field laid out as `layout` around a camera at
/// `camera_xz`, all wearing `material`, and return them, level 0 first.
fn spawn_levels(
    commands: &mut Commands,
    root: Entity,
    meshes: &FarFieldMeshes,
    material: &Handle<TerrainFarFieldMaterial>,
    layout: FarFieldLayout,
    camera_xz: Vec2,
    sink: f32,
) -> Vec<Entity> {
    (0..layout.levels)
        .map(|level| {
            let mesh = if level == 0 { &meshes.square } else { &meshes.ring };
            commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    far_field_level_transform(camera_xz, layout.base_spacing, level, sink),
                    Visibility::default(),
                    NotShadowCaster,
                    NoFrustumCulling,
                    TerrainFarFieldLevel { root, level },
                    Name::new(format!("TerrainFarField L{level}")),
                ))
                .id()
        })
        .collect()
}

fn despawn_levels(commands: &mut Commands, levels: &[Entity]) {
    for &level in levels {
        commands.entity(level).try_despawn();
    }
}

/// Give every terrain root that qualifies a far field and keep it current
/// (see the module docs): ask for the height texture of every root that
/// wants a far field, spawn the levels and material once the root's textures
/// are ready, move the levels with the camera, rebuild them when their count
/// or base spacing changes, rebind the material to the root's current images
/// and params, mark the root [`TerrainFarFieldActive`] once the far field has
/// settled, and take it all away when the root stops qualifying. Levels
/// whose root is gone are despawned. Runs after the surface and height
/// texture systems, so it sees this frame's surfaces and textures.
pub fn sync_terrain_far_fields(
    mut commands: Commands,
    settings: Option<Res<TerrainFarFieldSettings>>,
    bindings: Option<Res<TerrainSurfaceBindings>>,
    request: Option<ResMut<TerrainHeightTextureRequest>>,
    materials: Option<ResMut<Assets<TerrainFarFieldMaterial>>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut roots: Query<
        (
            Entity,
            &TerrainConfig,
            &TerrainData,
            Option<&TerrainBaked>,
            Option<&TerrainSurface>,
            Option<&TerrainHeightTexture>,
            Option<&mut TerrainFarField>,
            Option<&TerrainFarFieldActive>,
        ),
        With<TerrainRoot>,
    >,
    mut level_query: Query<(Entity, &TerrainFarFieldLevel, &mut Transform)>,
    mut level_meshes: Local<Option<FarFieldMeshes>>,
    mut wanted: Local<Vec<Entity>>,
) {
    let (Some(bindings), Some(mut materials), Some(mut meshes)) = (bindings, materials, meshes) else {
        return;
    };
    let enabled = settings.is_none_or(|settings| settings.enabled);
    let camera = scene_camera_translation(&cameras).map(|position| Vec2::new(position.x, position.z));

    // Levels whose root is gone (despawned with a Space) or no longer lists
    // them.
    for (entity, level, _) in &level_query {
        let owned = roots
            .get(level.root)
            .is_ok_and(|(.., far_field, _)| far_field.is_some_and(|far_field| far_field.levels.contains(&entity)));
        if !owned {
            commands.entity(entity).try_despawn();
        }
    }

    wanted.clear();
    for (entity, config, base, baked, surface, texture, far_field, active) in &mut roots {
        let data = surface_data(base, baked);
        let state = far_field_state(&FarFieldInputs {
            enabled: enabled && camera.is_some(),
            has_raster: !data.height_cache.is_empty(),
            fully_resident: raster_fully_resident(config, data),
            material_map: surface.map(|surface| (surface.map_size(), surface.is_map_ready())),
            height_texture: texture.map(|texture| (texture.size(), texture.is_ready())),
        });
        if state != FarFieldState::Off {
            wanted.push(entity);
        }
        let drawing = match (state, surface, texture, bindings.slot_records(), camera) {
            (FarFieldState::Drawing, Some(surface), Some(texture), Some(slot_records), Some(camera)) => {
                Some((surface, texture, slot_records.clone(), camera))
            }
            _ => None,
        };
        let Some((surface, texture, slot_records, camera)) = drawing else {
            if let Some(far_field) = far_field {
                despawn_levels(&mut commands, &far_field.levels);
                commands.entity(entity).try_remove::<(TerrainFarField, TerrainFarFieldActive)>();
                info!(
                    target: "eustress::terrain::far_field",
                    root = ?entity,
                    levels = far_field.layout.levels,
                    base_spacing = far_field.layout.base_spacing,
                    "terrain far field off"
                );
            } else if active.is_some() {
                commands.entity(entity).try_remove::<TerrainFarFieldActive>();
            }
            continue;
        };

        let params = TerrainSurfaceParams::new(config, surface.map_size(), true);
        let cell = raster_cell_size(&params);
        let near_radius = far_field_near_radius(config);
        let sink = far_field_sink(config);
        let layout = far_field_layout(near_radius, cell.max_element(), farthest_corner_distance(config, camera));
        let sparse = u32::from(data.sparse_surface);
        let far_params = |near_centre: Vec2| TerrainFarFieldParams {
            camera_xz: near_centre,
            near_radius,
            sink,
            cell,
            sparse,
            _pad: 0,
        };
        let extension_with = |far: TerrainFarFieldParams| {
            far_field_extension(
                bindings.arrays(),
                surface.material_map().clone(),
                slot_records.clone(),
                params,
                texture.image().clone(),
                far,
            )
        };

        let Some(mut far_field) = far_field else {
            let material = materials
                .add(TerrainFarFieldMaterial { base: far_field_base_material(), extension: extension_with(far_params(camera)) });
            let shared = level_meshes.get_or_insert_with(|| FarFieldMeshes::new(&mut meshes));
            let level_entities = spawn_levels(&mut commands, entity, shared, &material, layout, camera, sink);
            commands.entity(entity).try_insert(TerrainFarField {
                material,
                levels: level_entities,
                layout,
                near_centre: camera,
                age_frames: 0,
            });
            info!(
                target: "eustress::terrain::far_field",
                root = ?entity,
                levels = layout.levels,
                base_spacing = layout.base_spacing,
                near_radius,
                "terrain far field on"
            );
            continue;
        };

        far_field.age_frames = far_field.age_frames.saturating_add(1);
        let near_step = (config.chunk_size * NEAR_CENTRE_STEP).max(MIN_SPACING);
        let near_centre = follow_camera(far_field.near_centre, camera, near_step);
        if near_centre != far_field.near_centre {
            far_field.near_centre = near_centre;
        }

        // Written only when something differs, so an idle frame does not
        // re-prepare the material.
        let bound = extension_with(far_params(near_centre));
        let stale = materials.get(&far_field.material).is_some_and(|material| material.extension != bound);
        if stale {
            if let Some(mut material) = materials.get_mut(&far_field.material) {
                material.extension = bound;
            }
        }

        // Move the levels with the camera; lay them out afresh when their
        // count or spacing changes, or when something else despawned one.
        let mut rebuild = far_field.layout != layout;
        if !rebuild {
            for (level, &level_entity) in far_field.levels.iter().enumerate() {
                let Ok((_, _, mut transform)) = level_query.get_mut(level_entity) else {
                    rebuild = true;
                    break;
                };
                let target = far_field_level_transform(camera, layout.base_spacing, level as u32, sink);
                if *transform != target {
                    *transform = target;
                }
            }
        }
        if rebuild {
            despawn_levels(&mut commands, &far_field.levels);
            let shared = level_meshes.get_or_insert_with(|| FarFieldMeshes::new(&mut meshes));
            far_field.levels = spawn_levels(&mut commands, entity, shared, &far_field.material, layout, camera, sink);
            far_field.layout = layout;
            debug!(
                target: "eustress::terrain::far_field",
                root = ?entity,
                levels = layout.levels,
                base_spacing = layout.base_spacing,
                "terrain far field levels laid out afresh"
            );
        }

        // Once the material and meshes have had their settle frames to reach
        // the GPU, the chunks may leave the distance to the far field.
        if far_field.age_frames >= SURFACE_SETTLE_FRAMES && active.is_none_or(|active| active.near_radius != near_radius) {
            commands.entity(entity).try_insert(TerrainFarFieldActive { near_radius });
        }
    }

    // In a fixed order, since the query's changes when a root gains or loses
    // a component.
    wanted.sort_unstable();
    if let Some(mut request) = request {
        if request.far_field_roots != *wanted {
            request.far_field_roots = wanted.to_vec();
        }
    }
}

/// The far field: its shader, its `MaterialPlugin`, [`TerrainFarFieldSettings`]
/// and [`sync_terrain_far_fields`]. Added by `TerrainSurfacePlugin`, whose
/// surfaces, bindings and height textures it draws from, beside that
/// plugin's own material; it adds nothing of that plugin itself.
pub struct TerrainFarFieldPlugin;

impl Plugin for TerrainFarFieldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TerrainFarFieldSettings>();
        // A material plugin sets up its render side in `build`, so the render
        // sub-app must exist by now (see `TerrainSurfacePlugin`). Without one
        // there is nothing to draw, and no root is ever marked
        // `TerrainFarFieldActive`, so streaming chunks keep reaching the
        // whole map.
        if app.get_sub_app(bevy::render::RenderApp).is_none() {
            debug!(target: "eustress::terrain::far_field", "no renderer: no terrain far field");
            return;
        }
        embedded_asset!(app, "terrain_far_field.wgsl");
        app.add_plugins(MaterialPlugin::<TerrainFarFieldMaterial>::default())
            .add_systems(Update, sync_terrain_far_fields.after(sync_terrain_surfaces).after(sync_terrain_height_textures));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::surface_material::wgsl_layout::{f32_at, layout as wgsl_layout, offset_in, struct_fields, u32_at};
    use bevy::mesh::VertexAttributeValues;
    use bevy::render::render_resource::encase::UniformBuffer;
    use std::collections::HashMap;

    const VERTEX_SHADER: &str = include_str!("terrain_far_field.wgsl");
    const SURFACE_SHADER: &str = include_str!("terrain_surface.wgsl");

    /// Every triangle of `mesh`, as its three corners.
    fn triangles(mesh: &Mesh) -> Vec<[Vec3; 3]> {
        let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("the mesh has float positions");
        };
        let indices: Vec<usize> = mesh.indices().expect("the mesh is indexed").iter().collect();
        indices
            .chunks_exact(3)
            .map(|t| [Vec3::from(positions[t[0]]), Vec3::from(positions[t[1]]), Vec3::from(positions[t[2]])])
            .collect()
    }

    /// The triangles of `mesh` per quad, keyed by the quad's lower corner,
    /// after checking every triangle lies flat, faces up and spans half of
    /// one lattice quad.
    fn triangles_per_quad(mesh: &Mesh) -> HashMap<(i32, i32), u32> {
        let mut per_quad = HashMap::new();
        for [a, b, c] in triangles(mesh) {
            assert!(a.y == 0.0 && b.y == 0.0 && c.y == 0.0, "flat at local Y 0");
            assert!((b - a).cross(c - a).y > 0.0, "{a} {b} {c} faces up, Bevy's front face seen from above");
            let lo = a.min(b).min(c);
            let hi = a.max(b).max(c);
            assert_eq!((hi - lo).x, 1.0, "one quad wide");
            assert_eq!((hi - lo).z, 1.0, "one quad deep");
            *per_quad.entry((lo.x as i32, lo.z as i32)).or_insert(0) += 1;
        }
        per_quad
    }

    #[test]
    fn the_environment_switch_turns_the_far_field_off_only_when_asked() {
        assert!(far_field_enabled_by(None), "on by default");
        for off in ["0", "off", "OFF", " false ", "No"] {
            assert!(!far_field_enabled_by(Some(off)), "{off:?} switches it off");
        }
        for on in ["1", "on", "true", "yes", ""] {
            assert!(far_field_enabled_by(Some(on)), "{on:?} leaves it on");
        }
    }

    #[test]
    fn a_ring_has_every_quad_but_its_hole_and_level_0_has_them_all() {
        let (m, h) = (FAR_FIELD_HALF_QUADS as i32, FAR_FIELD_HOLE_HALF_QUADS as i32);
        assert_eq!(h, m / 2 - 2);
        let side = (2 * m + 1) as usize;
        let ring = far_field_level_mesh(FAR_FIELD_HALF_QUADS, FAR_FIELD_HOLE_HALF_QUADS);
        let square = far_field_level_mesh(FAR_FIELD_HALF_QUADS, 0);
        assert_eq!(ring.count_vertices(), side * side, "a vertex on every lattice point");
        assert_eq!(square.count_vertices(), side * side);
        assert!(ring.attribute(Mesh::ATTRIBUTE_UV_0).is_none() && ring.attribute(Mesh::ATTRIBUTE_NORMAL).is_none());

        let ring_quads = triangles_per_quad(&ring);
        let square_quads = triangles_per_quad(&square);
        let quads = (2 * m * 2 * m) as usize;
        let hole_quads = (2 * h * 2 * h) as usize;
        assert_eq!(ring_quads.len(), quads - hole_quads, "{} quads less the {hole_quads} of the hole", quads);
        assert_eq!(square_quads.len(), quads);
        assert_eq!(ring.indices().unwrap().len(), 6 * (quads - hole_quads));
        for x in -m..m {
            for z in -m..m {
                let in_hole = (-h..h).contains(&x) && (-h..h).contains(&z);
                let expected = if in_hole { None } else { Some(&2u32) };
                assert_eq!(ring_quads.get(&(x, z)), expected, "ring quad ({x}, {z})");
                assert_eq!(square_quads.get(&(x, z)), Some(&2u32), "level 0 quad ({x}, {z})");
            }
        }
    }

    #[test]
    fn every_level_covers_the_hole_of_the_next_with_a_quad_to_spare() {
        let (m, h) = (FAR_FIELD_HALF_QUADS as f32, FAR_FIELD_HOLE_HALF_QUADS as f32);
        let base = 1.5;
        for i in 0..500 {
            let camera = Vec2::new(i as f32 * 0.731 - 180.0, 95.0 - i as f32 * 0.417);
            for level in 1..6u32 {
                let inner = far_field_level_transform(camera, base, level - 1, 0.1);
                let outer = far_field_level_transform(camera, base, level, 0.1);
                let quad = level_spacing(base, level);
                assert_eq!(outer.scale.x, quad);
                for axis in [0usize, 2] {
                    let inner_reach = m * inner.scale[axis];
                    let hole = h * quad;
                    let (inner_centre, outer_centre) = (inner.translation[axis], outer.translation[axis]);
                    assert!(
                        inner_centre - inner_reach <= outer_centre - hole - quad + 1e-3
                            && inner_centre + inner_reach >= outer_centre + hole + quad - 1e-3,
                        "camera {camera}: level {} around {inner_centre} leaves level {level}'s hole around {outer_centre} a quad short",
                        level - 1
                    );
                }
                // And the camera stays within a quad of every level's centre.
                assert!((outer.translation.x - camera.x).abs() <= quad + 1e-3);
                assert!((outer.translation.z - camera.y).abs() <= quad + 1e-3);
            }
        }
    }

    #[test]
    fn level_0_spans_the_near_disc_and_the_levels_reach_the_farthest_corner() {
        // s_0 = 448 / 32 = 14 m; level L reaches 31 * 14 * 2^L m.
        let layout = far_field_layout(448.0, 2.0, 1000.0);
        assert_eq!(layout.base_spacing, 14.0);
        assert_eq!(layout.levels, 3, "434 m and 868 m fall short, 1736 m reaches");
        assert_eq!(far_field_layout(448.0, 2.0, 868.0).levels, 2, "reaching exactly is enough");
        assert_eq!(far_field_layout(448.0, 2.0, 0.0).levels, 1);
        // Never finer than a raster cell.
        assert_eq!(far_field_layout(10.0, 2.0, 10.0), FarFieldLayout { base_spacing: 2.0, levels: 1 });
        assert_eq!(far_field_layout(0.0, 0.5, 5.0).base_spacing, 0.5);
        // Capped, and a distance that is not a number takes every level.
        assert_eq!(far_field_layout(448.0, 2.0, 1.0e9).levels, MAX_FAR_FIELD_LEVELS);
        assert_eq!(far_field_layout(448.0, 2.0, f32::NAN).levels, MAX_FAR_FIELD_LEVELS);
        assert!(far_field_layout(f32::NAN, f32::NAN, 1.0).base_spacing > 0.0);

        // 41 x 41 chunks of 64 m at 32 cells: footprint -1280..1344, a
        // 2.0015 m raster cell, chunks out to 512 m.
        let config = TerrainConfig {
            chunk_size: 64.0,
            chunk_resolution: 32,
            chunks_x: 20,
            chunks_z: 20,
            view_distance: 512.0,
            ..TerrainConfig::default()
        };
        let params = TerrainSurfaceParams::new(&config, UVec2::splat(41 * 32), true);
        let cell = raster_cell_size(&params);
        assert!((cell.x - 2624.0 / 1311.0).abs() < 1e-4 && (cell.y - cell.x).abs() < 1e-6, "{cell}");
        let from_centre = farthest_corner_distance(&config, Vec2::ZERO);
        assert!((from_centre - 1344.0 * std::f32::consts::SQRT_2).abs() < 1e-2, "{from_centre}");
        let layout = far_field_layout(far_field_near_radius(&config), cell.max_element(), from_centre);
        assert_eq!(layout, FarFieldLayout { base_spacing: 14.0, levels: 4 }, "1736 m < 1901 m <= 3472 m");
        // From one corner the opposite corner is farther: one more level.
        let from_corner = farthest_corner_distance(&config, Vec2::splat(1344.0));
        assert!((from_corner - 2624.0 * std::f32::consts::SQRT_2).abs() < 1e-2, "{from_corner}");
        assert_eq!(far_field_layout(448.0, cell.max_element(), from_corner).levels, 5);
    }

    #[test]
    fn a_level_holds_still_until_the_camera_crosses_half_its_step_then_moves_one_step() {
        let (base, sink) = (3.0, 0.1);
        for level in [0u32, 1, 3] {
            let quad = level_spacing(base, level);
            let step = 2.0 * quad;
            let anchor = Vec2::new(5.0 * step, -7.0 * step);
            let held = far_field_level_transform(anchor, base, level, sink);
            assert_eq!(held.translation, Vec3::new(anchor.x, -far_field_level_drop(quad, level, sink), anchor.y));
            let drop = sink * (level + 1) as f32 + SINK_PER_LEVEL_QUAD * quad;
            assert!((far_field_level_drop(quad, level, sink) - drop).abs() < 1e-6, "sinks plus a share of the quad");
            assert_eq!(held.scale, Vec3::new(quad, 1.0, quad), "quads of s_L = s_0 * 2^L");
            assert_eq!(held.rotation, Quat::IDENTITY);
            // Anywhere in a window just under 2 * s_L wide around the anchor.
            for i in -10..=10 {
                let offset = i as f32 / 10.0 * 0.999 * quad;
                let moved = far_field_level_transform(anchor + Vec2::new(offset, -offset), base, level, sink);
                assert_eq!(moved.translation, held.translation, "level {level}, camera {offset} m off");
            }
            // Past it, exactly one step of 2 * s_L.
            let crossed = far_field_level_transform(anchor + Vec2::new(1.001 * quad, -1.001 * quad), base, level, sink);
            assert_eq!(crossed.translation.x - held.translation.x, step);
            assert_eq!(held.translation.z - crossed.translation.z, step);
            assert_eq!(crossed.translation.y, held.translation.y);
        }
        // Finer levels sit higher, so they win the depth test.
        assert!(far_field_level_transform(Vec2::ZERO, base, 0, sink).translation.y > far_field_level_transform(Vec2::ZERO, base, 1, sink).translation.y);
        assert!(far_field_level_transform(Vec2::ZERO, base, 0, sink).translation.y < 0.0, "under the chunks too");
        assert_eq!(snap_to_step(7.3, 0.0), 7.3);
    }

    #[test]
    fn the_near_radius_leaves_the_chunks_a_chunk_and_the_sink_follows_the_chunk_quad() {
        let config = TerrainConfig { chunk_size: 64.0, chunk_resolution: 32, view_distance: 512.0, ..TerrainConfig::default() };
        assert_eq!(far_field_near_radius(&config), 448.0);
        assert_eq!(far_field_near_radius(&TerrainConfig { view_distance: 40.0, ..config.clone() }), 0.0);
        assert_eq!(far_field_near_radius(&TerrainConfig { view_distance: f32::INFINITY, ..config.clone() }), 0.0);
        assert!((far_field_sink(&config) - 0.1).abs() < 1e-6, "a twentieth of a 2 m quad");
        assert_eq!(far_field_sink(&TerrainConfig { chunk_size: 8.0, chunk_resolution: 64, ..config.clone() }), 0.02);
        assert!((far_field_sink(&TerrainConfig { chunk_resolution: 0, ..config.clone() }) - 3.2).abs() < 1e-5);

        // The near disc follows the camera in steps.
        assert_eq!(follow_camera(Vec2::ZERO, Vec2::new(5.0, 5.0), 8.0), Vec2::ZERO);
        assert_eq!(follow_camera(Vec2::ZERO, Vec2::new(6.0, 6.0), 8.0), Vec2::new(6.0, 6.0));
        assert_eq!(follow_camera(Vec2::NAN, Vec2::ONE, 8.0), Vec2::ONE);
    }

    #[test]
    fn far_field_params_layout_matches_both_wgsl_structs() {
        for shader in [VERTEX_SHADER, SURFACE_SHADER] {
            let (members, size, _) = wgsl_layout(&struct_fields(shader, "TerrainFarFieldParams"));
            assert_eq!(
                members.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(),
                ["camera_xz", "near_radius", "sink", "cell", "sparse", "_pad"],
                "the shader's fields, in the Rust order"
            );
            assert_eq!(TerrainFarFieldParams::min_size().get() as usize, size, "encase and the shader agree on the size");

            let params = TerrainFarFieldParams {
                camera_xz: Vec2::new(-1.5, 2.5),
                near_radius: 3.5,
                sink: 0.25,
                cell: Vec2::new(4.5, 5.5),
                sparse: 1,
                _pad: 0,
            };
            let mut buffer = UniformBuffer::new(Vec::<u8>::new());
            buffer.write(&params).unwrap();
            let bytes = buffer.into_inner();
            assert!(bytes.len() >= size);
            let at = |name: &str| offset_in(&members, name);
            assert_eq!(f32_at(&bytes, at("camera_xz") + 4), 2.5);
            assert_eq!(f32_at(&bytes, at("near_radius")), 3.5);
            assert_eq!(f32_at(&bytes, at("sink")), 0.25);
            assert_eq!(f32_at(&bytes, at("cell")), 4.5);
            assert_eq!(f32_at(&bytes, at("cell") + 4), 5.5);
            assert_eq!(u32_at(&bytes, at("sparse")), 1);

            // The bindings the Rust side declares.
            assert!(shader.contains("@binding(107) var terrain_far_heights: texture_2d<f32>;"));
            assert!(shader.contains("@binding(108) var<uniform> terrain_far: TerrainFarFieldParams;"));
        }
        // The vertex stage reads the surface params the fragment stage does.
        assert_eq!(struct_fields(VERTEX_SHADER, "TerrainSurfaceParams"), struct_fields(SURFACE_SHADER, "TerrainSurfaceParams"));
        assert!(VERTEX_SHADER.contains("@binding(106) var<uniform> terrain_params: TerrainSurfaceParams;"));
        assert!(SURFACE_SHADER.contains("@binding(106) var<uniform> terrain_params: TerrainSurfaceParams;"));
    }

    #[test]
    fn the_chunk_surface_compiles_none_of_the_far_field() {
        // Every line of `terrain_surface.wgsl` that names the far field sits
        // inside `#ifdef TERRAIN_FAR_FIELD`, so the chunks' surface compiles
        // to what it would without the far field.
        let mut open: Vec<bool> = Vec::new();
        let mut far_field_lines = 0;
        for line in SURFACE_SHADER.lines() {
            let directive = line.trim();
            if let Some(def) = directive.strip_prefix("#ifdef ") {
                open.push(def.trim() == TERRAIN_FAR_FIELD_SHADER_DEF);
                continue;
            }
            if directive.starts_with("#ifndef ") || directive.starts_with("#if ") {
                open.push(false);
                continue;
            }
            if directive.starts_with("#else") {
                if let Some(branch) = open.last_mut() {
                    *branch = false;
                }
                continue;
            }
            if directive.starts_with("#endif") {
                assert!(open.pop().is_some(), "every #endif closes a block");
                continue;
            }
            let names_far_field = ["terrain_far", "far_field_hides", "TerrainFarFieldParams"].iter().any(|name| line.contains(name));
            if names_far_field {
                far_field_lines += 1;
                assert!(open.contains(&true), "{line:?} is outside #ifdef {TERRAIN_FAR_FIELD_SHADER_DEF}");
            }
        }
        assert!(open.is_empty(), "every block is closed");
        assert!(far_field_lines >= 6, "the far field's struct, bindings, hide test and discard are there");
        assert!(SURFACE_SHADER.contains("if far_field_hides(in.world_position.xz) {"));
    }

    #[test]
    fn only_a_streaming_raster_with_its_textures_on_the_gpu_draws_a_far_field() {
        let size = UVec2::new(1312, 1312);
        let ready = FarFieldInputs {
            enabled: true,
            has_raster: true,
            fully_resident: false,
            material_map: Some((size, true)),
            height_texture: Some((size, true)),
        };
        assert_eq!(far_field_state(&ready), FarFieldState::Drawing);

        // A raster small enough to stay resident keeps every chunk.
        assert_eq!(far_field_state(&FarFieldInputs { fully_resident: true, ..ready }), FarFieldState::Off);
        // Procedural terrain has no raster, so no surface or height texture.
        let procedural = FarFieldInputs { has_raster: false, material_map: None, height_texture: None, ..ready };
        assert_eq!(far_field_state(&procedural), FarFieldState::Off);
        // Switched off, or no camera to centre on.
        assert_eq!(far_field_state(&FarFieldInputs { enabled: false, ..ready }), FarFieldState::Off);
        // No textured surface to shade it with (arrays failed, raster too
        // wide for a texture): no far field, and no height texture asked for.
        assert_eq!(far_field_state(&FarFieldInputs { material_map: None, ..ready }), FarFieldState::Off);

        // A streaming raster whose height texture is missing asks for it.
        assert_eq!(far_field_state(&FarFieldInputs { height_texture: None, ..ready }), FarFieldState::Waiting);
        // Textures still settling.
        assert_eq!(far_field_state(&FarFieldInputs { height_texture: Some((size, false)), ..ready }), FarFieldState::Waiting);
        assert_eq!(far_field_state(&FarFieldInputs { material_map: Some((size, false)), ..ready }), FarFieldState::Waiting);
        // Just resized: the height texture is still the old size.
        let resized = FarFieldInputs { height_texture: Some((UVec2::new(1024, 1024), true)), ..ready };
        assert_eq!(far_field_state(&resized), FarFieldState::Waiting);
    }

    #[test]
    fn the_system_runs_and_despawns_levels_whose_root_is_gone() {
        let mut world = World::new();
        world.init_resource::<TerrainSurfaceBindings>();
        world.init_resource::<TerrainHeightTextureRequest>();
        world.init_resource::<TerrainFarFieldSettings>();
        world.init_resource::<Assets<TerrainFarFieldMaterial>>();
        world.init_resource::<Assets<Mesh>>();
        // Procedural terrain: no raster, so no far field and no height texture.
        let root = world.spawn((TerrainRoot, TerrainConfig::default(), TerrainData::procedural())).id();
        // A level left behind by a root that is gone.
        let gone = world.spawn_empty().id();
        assert!(world.despawn(gone));
        let orphan = world.spawn((TerrainFarFieldLevel { root: gone, level: 0 }, Transform::default())).id();

        let system = world.register_system(sync_terrain_far_fields);
        assert!(world.run_system(system).is_ok(), "its queries and resources do not conflict");
        assert!(world.get_entity(orphan).is_err(), "a level whose root is gone is despawned");
        assert!(world.get::<TerrainFarField>(root).is_none());
        assert!(world.get::<TerrainFarFieldActive>(root).is_none());
        assert!(world.resource::<TerrainHeightTextureRequest>().far_field_roots.is_empty());
    }
}

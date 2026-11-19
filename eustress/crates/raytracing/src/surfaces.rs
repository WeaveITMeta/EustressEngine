//! Terrain, water and scatter in the traced scene.
//!
//! Solari 0.19 traces only `StandardMaterial` meshes laid out as positions,
//! normals, UVs and tangents, and never reads vertex colours. Terrain chunks
//! draw with `TerrainSurfaceMaterial` or a vertex-colour `StandardMaterial`,
//! water with the alpha-blended `WaterSurfaceMaterial`, and the built-in
//! scatter meshes paint themselves with vertex colours, so none of them is in
//! the traced scene by itself: under ray tracing the ground would cast no
//! shadow and bounce no light, and trees would cast no shadow at all (the
//! sun's shadow maps are off while Solari traces the sun).
//!
//! Each of them gets a traced stand-in here, a proxy: an entity with the
//! trace marker and a `StandardMaterial` but no `Mesh3d`. Solari traces it
//! and the raster path never draws it, so what the camera sees directly is
//! untouched; the proxy is what shadow rays, bounce light and reflections
//! hit. Its look is the raster shader's own look reduced to what a ray hit
//! can use: one colour, roughness, metallic and reflectance per surface, and
//! no texture, so proxies add nothing to Solari's 5,000-texture array.
//!
//! A proxy names its source with [`TraceProxyOf`], a relationship whose
//! target ([`TraceProxies`]) despawns the proxies with the source, so a chunk
//! culled, a scatter batch dropped or a water body removed takes its proxies
//! with it without a scan. Proxies are not children: nothing in the terrain
//! hierarchy changes.
//!
//! A proxy leaves the traced scene by being despawned, never by losing its
//! marker. Solari 0.19 copies `RaytracingMesh3d` into the retained render
//! world every frame and registers nothing that removes it there, so an
//! entity that loses the marker in the main world stays traced; despawning
//! the entity despawns its render-world copy.
//!
//! ## Terrain
//! A chunk's proxy is its own render mesh (same LOD, same skirts), split by
//! the material slot that covers each triangle into at most
//! [`SurfaceTraceBudget::chunk_parts`] meshes, one per slot. The geometry
//! has to be the raster mesh's: Solari lights the camera's own pixels from
//! the deferred gbuffer and starts their shadow rays 1 mm above that
//! surface, so a proxy at another LOD would shadow or leak light through the
//! very ground it stands for. A triangle's slot is the brick material its
//! vertices carry (`UV_1`, marching-cubes chunks), else the material-map cell
//! under its centre, else, for terrain without a material layer, the height
//! band the vertex-colour mesher colours it by. Each slot draws with one
//! material shared by every chunk ([`terrain_slot_look`]): the slot's swatch
//! while the chunk wears the vertex colours, its tint times its texture
//! layer's mean albedo while it wears the textured surface, the slot's
//! roughness and metallic, and the terrain's reflectance. Road wedges
//! (`TerrainDraped`) are ground and are traced the same way. At most
//! [`SurfaceTraceBudget::terrain_triangles`] are traced; past it the
//! farthest chunks are left out, so a large resident terrain loses the
//! shadows of distant ground rather than memory or frame time.
//!
//! What a traced hit on terrain loses against the raster shader: the texture
//! detail and normal maps (a hit reads the slot's mean), the per-pixel
//! height blend between slots (a triangle is one slot; rarer slots past the
//! part limit fold into the chunk's commonest), and the baked shade in the
//! vertex alpha (Solari traces the occlusion that shade imitates). The far
//! field beyond the chunks is not traced.
//!
//! ## Water
//! Solari treats every traced surface as opaque, and the raster water is
//! drawn see-through over shallow ground (and casts no shadow, by
//! `NotShadowCaster`). A water proxy everywhere would black out every
//! shallow seabed the camera sees through the water. So a water surface is
//! traced only where the raster draws it at least
//! [`SurfaceTraceBudget::water_opacity`] opaque ([`traced_water_depth`],
//! the shader's own depth-to-opacity curve read from the surface's
//! material): its mesh is cut into pieces of at most
//! [`SurfaceTraceBudget::water_cell`] and the pieces deeper than that at
//! every corner are kept. The proxy is the deep colour, the water's
//! roughness (0.06) and reflectance (0.35, F0 about 0.02). Lost:
//! transparency and the seabed under it, the depth tint and shore foam, the
//! ripples and the river flow (the proxy is flat), and any refraction.
//! Shallow water is not traced at all. With the default colour and opacity
//! water is traced from about 4.5 m deep.
//!
//! ## Scatter
//! - Trees, large rocks and other scatter standing as entities: one proxy
//!   per entity per colour group, all entities of a prototype sharing its
//!   meshes and materials. A prototype is split by its vertex colours into
//!   at most [`SurfaceTraceBudget::prototype_groups`] groups (bark and
//!   crown), each drawn in its mean colour. Only the nearest
//!   [`SurfaceTraceBudget::scatter_instances`] within
//!   [`SurfaceTraceBudget::scatter_radius`] are traced, chosen again when
//!   the view moves [`SurfaceTraceBudget::scatter_reselect_step`].
//! - A Custom layer's mesh that the core module can trace as it is goes
//!   through the core path; it is kept out of the traced scene with
//!   `TraceOverride::Never` while it is outside the same budget.
//! - Shrubs and small rocks merged into a tile's mesh: one proxy per tile
//!   within [`SurfaceTraceBudget::tile_radius`], under a per-tile and a total
//!   triangle budget.
//! - Grass is not traced. Its merged tiles carry `NotShadowCaster` in the
//!   raster path ("their shadows cost far more than they show"), and the
//!   same holds for rays and acceleration structures.
//!
//! ## Keeping in step
//! Everything is change-driven: a chunk whose `Mesh3d` or visibility
//! changes (spawn, LOD, edit, bake, hide) is rebuilt; one whose material
//! swaps between the vertex colours and the textured surface only changes
//! its proxies' materials; a slot edit rewrites the shared slot materials in
//! place; a water surface is rebuilt when its mesh, transform, visibility or
//! material changes, and once the ground's heights have gone quiet after an
//! edit; scatter batches are indexed once when they spawn, and the budget is
//! chosen again only when the view moves a step or the index changes.
//!
//! The heavy builds (a chunk's split, a water surface's cut, a merged
//! tile) run on the async compute pool from copies of their inputs, made on
//! the main thread: the source mesh, and only the material cells or ground
//! heights under it. Unoptimised, as every dev engine runs this crate, a
//! LOD-0 chunk takes tens of milliseconds, which would otherwise be a hitch
//! on every LOD change. Builds are also rate limited, nearest first: Solari
//! builds a BLAS for every new traceable mesh in the frame it arrives, with
//! no budget of its own, so a Space loading hundreds of chunks would
//! otherwise build them all in one frame.

use std::collections::{HashMap, HashSet};

use bevy::asset::{AssetEvent, RenderAssetUsages};
use bevy::camera::primitives::Aabb;
use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::tasks::{futures::check_ready, AsyncComputeTaskPool, Task};
use eustress_common::terrain::texture_arrays::slot_surface_in;
use eustress_common::terrain::{
    cache_cell_at_world, calculate_splat_weights, material_cell_weights, scene_camera_translation, world_to_uv,
    surface_data, Chunk, HeightBlendParams, MaterialCell, ScatterBatch, TerrainBaked, TerrainConfig, TerrainData,
    TerrainDirtyChunks, TerrainDraped, TerrainMaterial, TerrainMaterialSlots, TerrainRoot, TerrainSlotSurface,
    TerrainSurfaceBindings, TerrainSurfaceMaterial, TerrainTextureArraySet, WaterSurface, WaterSurfaceMaterial,
    WaterSurfaceParams, MATERIAL_SLOT_NONE,
};

use super::{conform, raytracing_active, Conformance, SkipReason, TraceMarker, TraceOverride};

// ============================================================================
// Budgets
// ============================================================================

/// How much of each surface kind is traced, and how fast proxies are built.
/// A resource so a Space or a test can tighten it.
#[derive(Resource, Clone, Debug)]
pub struct SurfaceTraceBudget {
    /// Most proxy meshes one chunk is split into. Each is a TLAS instance
    /// and a BLAS; a chunk rarely shows more than three slots.
    pub chunk_parts: usize,
    /// Most chunk proxies being built at once (off the main thread), so
    /// also the most that arrive, and so the most BLAS Solari builds for
    /// terrain, in one frame.
    pub chunk_builds_per_frame: usize,
    /// Most terrain triangles traced at once. Past it the farthest chunks
    /// are left out (no traced shadow from distant ground) rather than
    /// traced at a coarser LOD than they are drawn, which would shadow the
    /// ground that is drawn.
    pub terrain_triangles: u64,
    /// Water is traced where the raster draws it at least this opaque.
    pub water_opacity: f32,
    /// Longest edge, metres, a water triangle is cut down to before its
    /// shallow pieces are dropped: the ocean is one quad over the terrain.
    pub water_cell: f32,
    /// Most pieces one water triangle's edge is cut into.
    pub water_max_subdivision: u32,
    /// Most pieces one water surface is cut into; past it the cell grows.
    pub water_triangles: usize,
    /// Frames the ground's heights must go unchanged before water is
    /// measured against them again, so a brush stroke beside a lake does not
    /// rebuild its proxy every frame.
    pub water_quiet_frames: u32,
    /// Scatter entities farther than this from the view are not traced,
    /// metres.
    pub scatter_radius: f32,
    /// Most scatter entities traced at once, nearest first.
    pub scatter_instances: usize,
    /// How far the view moves, metres, before the traced scatter is chosen
    /// again.
    pub scatter_reselect_step: f32,
    /// Most scatter entities that start being traced per frame.
    pub scatter_spawns_per_frame: usize,
    /// Most colour groups (so proxy meshes, and TLAS instances per entity)
    /// a scatter prototype is split into.
    pub prototype_groups: usize,
    /// Merged scatter tiles (shrubs, small rocks) farther than this are not
    /// traced, metres.
    pub tile_radius: f32,
    /// A merged tile with more triangles than this is never traced.
    pub tile_triangles: u32,
    /// Most triangles of merged tiles traced at once.
    pub tile_triangle_budget: u32,
    /// Most merged tiles whose proxy is being built at once (off the main
    /// thread).
    pub tile_builds_per_frame: usize,
}

impl Default for SurfaceTraceBudget {
    fn default() -> Self {
        Self {
            chunk_parts: 6,
            chunk_builds_per_frame: 4,
            terrain_triangles: 3_000_000,
            water_opacity: 0.8,
            water_cell: 8.0,
            water_max_subdivision: 256,
            water_triangles: 131_072,
            water_quiet_frames: 20,
            scatter_radius: 250.0,
            scatter_instances: 4096,
            scatter_reselect_step: 8.0,
            scatter_spawns_per_frame: 512,
            prototype_groups: 2,
            tile_radius: 48.0,
            tile_triangles: 65_536,
            tile_triangle_budget: 1 << 20,
            tile_builds_per_frame: 2,
        }
    }
}

/// What is traced through this module, and the work it did, for the log and
/// for profiling. Live counts follow sources as they come and go; the
/// counters only rise.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct SurfaceTraceStats {
    pub terrain_proxies: usize,
    pub terrain_triangles: u64,
    pub terrain_vertices: u64,
    pub water_proxies: usize,
    pub water_triangles: u64,
    pub water_vertices: u64,
    /// Scatter entities traced (through proxies or the core path).
    pub scatter_instances: usize,
    /// Proxy entities standing for them.
    pub scatter_proxies: usize,
    /// Triangles and vertices of the prototype meshes, which every entity
    /// of a prototype shares.
    pub prototype_triangles: u64,
    pub prototype_vertices: u64,
    pub tiles: usize,
    pub tile_triangles: u64,
    pub tile_vertices: u64,
    /// Chunks (and draped meshes) built.
    pub chunk_builds: u64,
    /// Water surfaces built.
    pub water_builds: u64,
    /// Times the traced scatter was chosen.
    pub scatter_passes: u64,
    /// Shared terrain slot materials rewritten.
    pub look_writes: u64,
}

/// Bytes one proxy vertex takes (positions, normals, UVs and tangents:
/// 12 + 12 + 8 + 16, Solari's 48-byte stride), and one triangle's indices.
pub const PROXY_VERTEX_BYTES: u64 = 48;
pub const PROXY_TRIANGLE_BYTES: u64 = 12;

impl SurfaceTraceStats {
    /// TLAS instances this module adds.
    pub fn tlas_instances(&self) -> usize {
        self.terrain_proxies + self.water_proxies + self.scatter_proxies + self.tiles
    }

    /// Bytes of proxy mesh data, held once on the CPU and once on the GPU
    /// (the acceleration structures come on top).
    pub fn mesh_bytes(&self) -> u64 {
        let vertices = self.terrain_vertices + self.water_vertices + self.prototype_vertices + self.tile_vertices;
        let triangles = self.terrain_triangles + self.water_triangles + self.prototype_triangles + self.tile_triangles;
        vertices * PROXY_VERTEX_BYTES + triangles * PROXY_TRIANGLE_BYTES
    }
}

// ============================================================================
// Proxies
// ============================================================================

/// On a proxy: the source it stands for in the traced scene.
#[derive(Component, Debug)]
#[relationship(relationship_target = TraceProxies)]
pub struct TraceProxyOf(pub Entity);

/// On a source: its proxies. Despawning the source despawns them.
#[derive(Component, Debug, Default)]
#[relationship_target(relationship = TraceProxyOf, linked_spawn)]
pub struct TraceProxies(Vec<Entity>);

impl TraceProxies {
    pub fn proxies(&self) -> &[Entity] {
        &self.0
    }
}

/// What a proxy stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProxyKind {
    /// Part of a chunk (or a draped mesh) drawn as one material slot.
    Terrain { slot: u8 },
    Water,
    /// One colour group of a scatter entity.
    ScatterInstance,
    /// A merged scatter tile.
    ScatterTile,
}

/// On every proxy, beside the marker: what it is and what it traces.
#[derive(Component, Clone, Debug)]
pub struct SurfaceProxy {
    pub kind: ProxyKind,
    pub mesh: Handle<Mesh>,
    pub triangles: u32,
    pub vertices: u32,
}

/// What one proxy is made of.
struct ProxySpec {
    kind: ProxyKind,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    triangles: u32,
    vertices: u32,
}

/// Spawn a traced proxy of `source` once the commands run, and only if
/// `source` still exists then: a relationship to a missing entity would be
/// dropped and leave the proxy traced forever. The entity is reserved now so
/// the caller can keep its id.
fn spawn_proxy<M: TraceMarker>(commands: &mut Commands, source: Entity, at: GlobalTransform, spec: ProxySpec) -> Entity {
    let proxy = commands.spawn_empty().id();
    commands.queue(move |world: &mut World| {
        let alive = world.get_entity(source).is_ok();
        let Ok(mut entity) = world.get_entity_mut(proxy) else {
            return;
        };
        if !alive {
            entity.despawn();
            return;
        }
        let ProxySpec { kind, mesh, material, triangles, vertices } = spec;
        entity.insert((
            TraceProxyOf(source),
            SurfaceProxy { kind, mesh: mesh.clone(), triangles, vertices },
            MeshMaterial3d(material),
            at.compute_transform(),
            at,
            M::new(mesh),
        ));
    });
    proxy
}

/// Despawn every proxy of `source` once the commands run.
fn despawn_proxies_of(commands: &mut Commands, source: Entity) {
    commands.queue(move |world: &mut World| {
        let proxies: Vec<Entity> = world.get::<TraceProxies>(source).map(|p| p.proxies().to_vec()).unwrap_or_default();
        for proxy in proxies {
            if let Ok(entity) = world.get_entity_mut(proxy) {
                entity.despawn();
            }
        }
    });
}

/// A traced surface reduced to what a ray hit reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceLook {
    /// Linear RGB.
    pub albedo: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    pub reflectance: f32,
}

impl SurfaceLook {
    pub fn material(&self) -> StandardMaterial {
        let mut material = StandardMaterial::default();
        self.apply(&mut material);
        material
    }

    /// Opaque, lit, not emissive and without textures are the defaults, and
    /// what the rules and Solari need: an emissive proxy would become a
    /// light, and a texture that has not loaded would drop it.
    pub fn apply(&self, material: &mut StandardMaterial) {
        let [r, g, b] = self.albedo;
        material.base_color = Color::linear_rgb(r, g, b);
        material.perceptual_roughness = self.roughness;
        material.metallic = self.metallic;
        material.reflectance = self.reflectance;
    }

    /// Bit pattern, so equal looks can share one material.
    fn key(&self) -> [u32; 6] {
        let [r, g, b] = self.albedo;
        [r, g, b, self.roughness, self.metallic, self.reflectance].map(f32::to_bits)
    }
}

/// A unit normal; a zero or broken one becomes up.
fn unit_normal(normal: [f32; 3]) -> Vec3 {
    Vec3::from(normal).try_normalize().unwrap_or(Vec3::Y)
}

/// Any unit tangent at right angles to `normal`. No proxy material has a
/// normal map, so the tangent's direction never shows; Solari only needs the
/// attribute. Gram-Schmidt against X (Z when the normal lies along X) is
/// exact and costs far less than mikktspace over a chunk.
fn tangent_for(normal: Vec3) -> [f32; 4] {
    let axis = if normal.x.abs() < 0.9 { Vec3::X } else { Vec3::Z };
    let tangent = (axis - normal * normal.dot(axis)).try_normalize().unwrap_or(Vec3::X);
    [tangent.x, tangent.y, tangent.z, 1.0]
}

/// A mesh in exactly the layout Solari traces, from `triangles` over
/// `positions` and `normals`: only the vertices the triangles use, zero UVs
/// (no proxy material has a texture), tangents from the normals, `U32`
/// indices. Checked with the core's [`conform`], which must take it as it
/// is: a mesh it would copy again is a layout bug here, and a second copy
/// would double the memory. `None` when no triangle is left or an index is
/// out of range.
pub fn proxy_mesh(positions: &[[f32; 3]], normals: &[[f32; 3]], triangles: &[[u32; 3]]) -> Option<Mesh> {
    if triangles.is_empty() || normals.len() != positions.len() {
        return None;
    }
    let mut remap = vec![u32::MAX; positions.len()];
    let (mut kept_positions, mut kept_normals) = (Vec::new(), Vec::new());
    let mut indices = Vec::with_capacity(triangles.len() * 3);
    for triangle in triangles {
        for &vertex in triangle {
            let v = vertex as usize;
            if v >= positions.len() {
                return None;
            }
            if remap[v] == u32::MAX {
                remap[v] = u32::try_from(kept_positions.len()).ok()?;
                kept_positions.push(positions[v]);
                kept_normals.push(unit_normal(normals[v]).to_array());
            }
            indices.push(remap[v]);
        }
    }
    let tangents: Vec<[f32; 4]> = kept_normals.iter().map(|n| tangent_for(Vec3::from(*n))).collect();
    let uvs = vec![[0.0f32, 0.0]; kept_positions.len()];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, kept_positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, kept_normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
    mesh.insert_indices(Indices::U32(indices));
    // The default, stated: Solari skips a mesh without it.
    mesh.enable_raytracing = true;
    match conform(&mesh) {
        Conformance::AsIs { .. } => Some(mesh),
        other => {
            debug_assert!(false, "a proxy mesh not in Solari's layout: {other:?}");
            None
        }
    }
}

fn mesh_counts(mesh: &Mesh) -> (u32, u32) {
    let vertices = u32::try_from(mesh.count_vertices()).unwrap_or(u32::MAX);
    let triangles = mesh
        .try_indices_option()
        .ok()
        .flatten()
        .map_or(0, |indices| u32::try_from(indices.len() / 3).unwrap_or(u32::MAX));
    (triangles, vertices)
}

/// A raster mesh as triangles, read without changing it.
struct SourceMesh<'a> {
    positions: &'a [[f32; 3]],
    normals: &'a [[f32; 3]],
    triangles: Vec<[u32; 3]>,
    /// `UV_1`: a marching-cubes chunk's brick material.
    brick: Option<&'a [[f32; 2]]>,
    colors: Option<&'a [[f32; 4]]>,
}

fn read_source(mesh: &Mesh) -> Option<SourceMesh<'_>> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let positions = match mesh.try_attribute_option(Mesh::ATTRIBUTE_POSITION).ok()?? {
        VertexAttributeValues::Float32x3(values) => values.as_slice(),
        _ => return None,
    };
    let normals = match mesh.try_attribute_option(Mesh::ATTRIBUTE_NORMAL).ok()?? {
        VertexAttributeValues::Float32x3(values) => values.as_slice(),
        _ => return None,
    };
    let count = positions.len();
    if count == 0 || normals.len() != count {
        return None;
    }
    let brick = match mesh.try_attribute_option(Mesh::ATTRIBUTE_UV_1).ok()? {
        Some(VertexAttributeValues::Float32x2(values)) if values.len() == count => Some(values.as_slice()),
        _ => None,
    };
    let colors = match mesh.try_attribute_option(Mesh::ATTRIBUTE_COLOR).ok()? {
        Some(VertexAttributeValues::Float32x4(values)) if values.len() == count => Some(values.as_slice()),
        _ => None,
    };
    let triangles: Vec<[u32; 3]> = match mesh.try_indices_option().ok()? {
        Some(indices) => {
            let flat: Vec<u32> = indices.iter().map(|i| i as u32).collect();
            if flat.len() % 3 != 0 || flat.iter().any(|&i| i as usize >= count) {
                return None;
            }
            flat.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect()
        }
        None => {
            let whole = u32::try_from(count - count % 3).ok()?;
            (0..whole).step_by(3).map(|i| [i, i + 1, i + 2]).collect()
        }
    };
    Some(SourceMesh { positions, normals, triangles, brick, colors })
}

fn centroid(positions: &[[f32; 3]], triangle: [u32; 3]) -> Vec3 {
    triangle.iter().map(|&v| Vec3::from(positions[v as usize])).sum::<Vec3>() / 3.0
}

/// The floor Bevy's lighting (and the terrain shader) clamps perceptual
/// roughness to.
const MIN_PERCEPTUAL_ROUGHNESS: f32 = 0.089;

fn unit_color(c: f32) -> f32 {
    if c.is_finite() {
        c.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

// ============================================================================
// Terrain
// ============================================================================

/// Reflectance of the `StandardMaterial` terrain is lit with, both the
/// vertex-colour material and the base of `TerrainSurfaceMaterial`:
/// `terrain_standard_material` in `eustress_common::terrain`, which is
/// crate-private. F0 0.16 * 0.3^2, about 0.014.
pub const TERRAIN_REFLECTANCE: f32 = 0.3;
/// What a point with no material around it draws, as the terrain shader and
/// the CPU mesher do.
const GRASS_SLOT: u8 = TerrainMaterial::Grass as u8;
/// The slots the height-band colours of terrain without a material layer
/// stand for, in `calculate_splat_weights`' order.
const BAND_SLOTS: [u8; 4] = [
    TerrainMaterial::Grass as u8,
    TerrainMaterial::Rock as u8,
    TerrainMaterial::Dirt as u8,
    TerrainMaterial::Snow as u8,
];

/// A rectangle of raster cells: its first cell and its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterRect {
    pub origin: UVec2,
    pub size: UVec2,
}

impl RasterRect {
    /// The cells under world XZ `area` (`min`, `max`) and one more on every
    /// side, so a triangle's nearest cell and a bilinear read's four cells
    /// are all inside; the whole raster for `None`. `None` without a raster.
    pub fn covering(config: &TerrainConfig, data: &TerrainData, area: Option<(Vec2, Vec2)>) -> Option<Self> {
        let full = UVec2::new(data.cache_width, data.cache_height);
        if full.min_element() == 0 {
            return None;
        }
        let Some((min, max)) = area else {
            return Some(Self { origin: UVec2::ZERO, size: full });
        };
        let a = cache_cell_at_world(config, data, min.x, min.y)?;
        let b = cache_cell_at_world(config, data, max.x, max.y)?;
        let lo = a.min(b).saturating_sub(UVec2::ONE);
        let hi = (a.max(b) + UVec2::ONE).min(full - UVec2::ONE);
        Some(Self { origin: lo, size: hi - lo + UVec2::ONE })
    }

    fn index(&self, cell: UVec2) -> Option<usize> {
        if cell.x < self.origin.x || cell.y < self.origin.y {
            return None;
        }
        let local = cell - self.origin;
        (local.x < self.size.x && local.y < self.size.y).then(|| local.y as usize * self.size.x as usize + local.x as usize)
    }
}

/// A raster's dimensions alone: what `cache_cell_at_world` reads to name a
/// cell, without its cells.
fn raster_dims(data: &TerrainData) -> TerrainData {
    TerrainData { cache_width: data.cache_width, cache_height: data.cache_height, ..default() }
}

/// The world XZ bounds of `aabb` (local) placed by `at`.
fn world_xz_bounds(min: Vec3, max: Vec3, at: &GlobalTransform) -> (Vec2, Vec2) {
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for corner in 0..8 {
        let local = Vec3::new(
            if corner & 1 == 0 { min.x } else { max.x },
            if corner & 2 == 0 { min.y } else { max.y },
            if corner & 4 == 0 { min.z } else { max.z },
        );
        let p = at.transform_point(local);
        lo = lo.min(Vec2::new(p.x, p.z));
        hi = hi.max(Vec2::new(p.x, p.z));
    }
    (lo, hi)
}

/// The local bounds of a mesh: from its `Aabb` component when Bevy has
/// computed one, else from its positions.
fn local_bounds(mesh: &Mesh, aabb: Option<&Aabb>) -> Option<(Vec3, Vec3)> {
    if let Some(aabb) = aabb {
        let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
        return Some((c - h, c + h));
    }
    match mesh.try_attribute_option(Mesh::ATTRIBUTE_POSITION).ok()?? {
        VertexAttributeValues::Float32x3(p) if !p.is_empty() => Some(p.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), v| (lo.min(Vec3::from(*v)), hi.max(Vec3::from(*v))),
        )),
        _ => None,
    }
}

/// Where a terrain triangle's material slot comes from, owned so a build can
/// run off the main thread: a copy of the material-map cells under the mesh,
/// or, for terrain without a material layer, the height bands the
/// vertex-colour mesher colours procedural terrain by (`HeightfieldShading`
/// in the terrain mesher).
pub enum SlotGround {
    MaterialMap { config: TerrainConfig, raster: TerrainData, rect: RasterRect, cells: Vec<MaterialCell> },
    HeightBands(HeightBlendParams),
}

impl SlotGround {
    /// The ground under the whole raster.
    pub fn of(config: &TerrainConfig, data: &TerrainData) -> Self {
        Self::capture(config, data, None)
    }

    /// The ground under world XZ `area` (all of it for `None`): the cells
    /// copied row by row, a few hundred for a chunk.
    pub fn capture(config: &TerrainConfig, data: &TerrainData, area: Option<(Vec2, Vec2)>) -> Self {
        let rect = (!data.height_cache.is_empty() && data.has_material_layer())
            .then(|| RasterRect::covering(config, data, area))
            .flatten();
        let Some(rect) = rect else {
            return Self::HeightBands(HeightBlendParams::default());
        };
        let width = data.cache_width as usize;
        let mut cells = Vec::with_capacity(rect.size.x as usize * rect.size.y as usize);
        for z in rect.origin.y..rect.origin.y + rect.size.y {
            let start = z as usize * width + rect.origin.x as usize;
            cells.extend_from_slice(&data.material_cache[start..start + rect.size.x as usize]);
        }
        Self::MaterialMap { config: config.clone(), raster: raster_dims(data), rect, cells }
    }

    /// The slot covering world point `at`: the heavier material of the
    /// nearest raster cell (the blend inside a triangle is lost anyway), or
    /// the heaviest height band at its height, read with no slope as the
    /// mesher reads it.
    pub fn slot_at(&self, at: Vec3) -> u8 {
        match self {
            Self::MaterialMap { config, raster, rect, cells } => cache_cell_at_world(config, raster, at.x, at.z)
                .and_then(|cell| rect.index(cell))
                .and_then(|index| cells.get(index))
                .and_then(|cell| dominant_cell_slot(*cell))
                .unwrap_or(GRASS_SLOT),
            Self::HeightBands(params) => {
                let weights = calculate_splat_weights(at.y, 0.0, params);
                let mut best = 0;
                for (band, weight) in weights.iter().enumerate() {
                    // Strictly greater, so a tie keeps the lower band.
                    if *weight > weights[best] {
                        best = band;
                    }
                }
                BAND_SLOTS[best]
            }
        }
    }
}

/// The heavier material of a cell; `None` for a cell with none.
fn dominant_cell_slot(cell: MaterialCell) -> Option<u8> {
    let mut best: Option<(u8, f32)> = None;
    for (slot, weight) in material_cell_weights(cell) {
        if slot != MATERIAL_SLOT_NONE && weight > 0.0 && best.is_none_or(|(_, most)| weight > most) {
            best = Some((slot, weight));
        }
    }
    best.map(|(slot, _)| slot)
}

/// The brick material a marching-cubes triangle carries in `UV_1`
/// (`[slot + 1, 1]` on brick vertices, `[0, 0]` where the material map
/// applies; see `marching::brick_material_uv`): its slot when its vertices
/// are on average more brick than map, decoded as the shader decodes it.
fn brick_slot(brick: &[[f32; 2]], triangle: [u32; 3]) -> Option<u8> {
    let (mut sum_x, mut sum_y) = (0.0f32, 0.0f32);
    for vertex in triangle {
        let [x, y] = brick[vertex as usize];
        sum_x += x;
        sum_y += y;
    }
    if sum_y < 1.5 {
        return None;
    }
    let ratio = sum_x / sum_y;
    let code = ratio.round();
    ((1.0..=255.0).contains(&code) && (ratio - code).abs() < 0.01).then(|| code as u8 - 1)
}

/// One slot's share of a terrain mesh, laid out for Solari.
pub struct TerrainPart {
    pub slot: u8,
    pub mesh: Mesh,
    pub triangles: u32,
    pub vertices: u32,
}

/// Split a chunk's (or a draped mesh's) render mesh by the slot covering
/// each triangle, keeping the `max_parts` commonest slots and folding the
/// rest into the commonest. `to_world` places the mesh's vertices, so each
/// triangle finds its material-map cell. Empty when the mesh cannot be read.
pub fn split_terrain_mesh(mesh: &Mesh, to_world: &GlobalTransform, ground: &SlotGround, max_parts: usize) -> Vec<TerrainPart> {
    let Some(source) = read_source(mesh) else {
        return Vec::new();
    };
    let mut counts = [0usize; 256];
    let slots: Vec<u8> = source
        .triangles
        .iter()
        .map(|&triangle| {
            let slot = source
                .brick
                .and_then(|brick| brick_slot(brick, triangle))
                .unwrap_or_else(|| ground.slot_at(to_world.transform_point(centroid(source.positions, triangle))));
            counts[slot as usize] += 1;
            slot
        })
        .collect();
    let mut order: Vec<u8> = (0..=u8::MAX).filter(|slot| counts[*slot as usize] > 0).collect();
    // Commonest first; a tie keeps the lower slot, so a chunk splits the
    // same way every time.
    order.sort_by(|a, b| counts[*b as usize].cmp(&counts[*a as usize]).then(a.cmp(b)));
    order.truncate(max_parts.max(1));
    if order.is_empty() {
        return Vec::new();
    }
    let mut bucket_of = [0usize; 256];
    for (bucket, slot) in order.iter().enumerate() {
        bucket_of[*slot as usize] = bucket;
    }
    // Every slot past the limit stays at bucket 0: the commonest.
    let mut buckets: Vec<Vec<[u32; 3]>> = order.iter().map(|slot| Vec::with_capacity(counts[*slot as usize])).collect();
    for (triangle, slot) in source.triangles.iter().zip(&slots) {
        buckets[bucket_of[*slot as usize]].push(*triangle);
    }
    order
        .iter()
        .zip(buckets)
        .filter_map(|(slot, triangles)| {
            let mesh = proxy_mesh(source.positions, source.normals, &triangles)?;
            let (triangles, vertices) = mesh_counts(&mesh);
            Some(TerrainPart { slot: *slot, mesh, triangles, vertices })
        })
        .collect()
}

/// The look of one slot's surface: its flat colour, or its tint times its
/// texture layer's mean albedo (`layer_mean_albedo`) when it is drawn from a
/// layer, as the terrain shader averages over a pixel footprint; the slot's
/// roughness (the shader scales the layer's roughness to average there) and
/// metallic; the terrain's reflectance.
pub fn slot_look(surface: &TerrainSlotSurface, layer_mean_albedo: Option<[f32; 3]>) -> SurfaceLook {
    let albedo = match (surface.layer, layer_mean_albedo) {
        (Some(_), Some(mean)) => std::array::from_fn(|i| surface.albedo[i] * mean[i]),
        _ => surface.albedo,
    };
    SurfaceLook {
        albedo: albedo.map(unit_color),
        roughness: surface.roughness.clamp(MIN_PERCEPTUAL_ROUGHNESS, 1.0),
        metallic: surface.metallic.clamp(0.0, 1.0),
        reflectance: TERRAIN_REFLECTANCE,
    }
}

/// The look of `slot` on a chunk that draws `textured` (wearing the
/// textured surface material, whose bound arrays are `arrays`) or with the
/// vertex colours (each slot's swatch).
pub fn terrain_slot_look(
    slots: &TerrainMaterialSlots,
    arrays: Option<&TerrainTextureArraySet>,
    slot: u8,
    textured: bool,
) -> SurfaceLook {
    let arrays = arrays.filter(|_| textured);
    let surface = slot_surface_in(slots, arrays, slot);
    let mean = surface
        .layer
        .and_then(|layer| arrays?.layers.get(layer as usize))
        .map(|layer| layer.mean_albedo_linear);
    slot_look(&surface, mean)
}

/// One chunk's (or draped mesh's) proxies.
struct TerrainSource {
    textured: bool,
    parts: Vec<(Entity, u8)>,
    triangles: u64,
    vertices: u64,
}

/// The terrain proxies, the chunks waiting for a build, and the shared slot
/// materials.
#[derive(Resource, Default)]
pub struct TerrainProxies {
    sources: HashMap<Entity, TerrainSource>,
    pending: HashSet<Entity>,
    /// Builds running off the main thread. A newer build of the same chunk
    /// replaces (and so cancels) an older one.
    building: HashMap<Entity, Task<Vec<TerrainPart>>>,
    /// Chunks left out by the triangle budget, and the triangles each needs.
    /// One is queued again only once that many are free, so a full budget
    /// costs nothing per frame.
    over_budget: HashMap<Entity, u64>,
    /// One material per slot and look (vertex colours or textured).
    looks: HashMap<(u8, bool), (Handle<StandardMaterial>, SurfaceLook)>,
}

impl TerrainProxies {
    /// Chunks waiting for a build.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Chunks whose build is running.
    pub fn in_flight(&self) -> usize {
        self.building.len()
    }

    /// Whether `source` waits for a build to start.
    pub fn is_queued(&self, source: Entity) -> bool {
        self.pending.contains(&source)
    }

    /// Work waits: a build queued or running.
    pub fn busy(&self) -> bool {
        !self.pending.is_empty() || !self.building.is_empty()
    }

    /// The proxies of `source` and the slot each draws.
    pub fn parts_of(&self, source: Entity) -> Vec<(Entity, u8)> {
        self.sources.get(&source).map(|s| s.parts.clone()).unwrap_or_default()
    }

    /// Whether `source` is left out by the triangle budget.
    pub fn is_over_budget(&self, source: Entity) -> bool {
        self.over_budget.contains_key(&source)
    }

    /// The shared material of `slot` drawn `textured`, once one exists.
    pub fn slot_material(&self, slot: u8, textured: bool) -> Option<Handle<StandardMaterial>> {
        self.looks.get(&(slot, textured)).map(|(handle, _)| handle.clone())
    }

    fn look_handle(
        &mut self,
        key: (u8, bool),
        slots: &TerrainMaterialSlots,
        arrays: Option<&TerrainTextureArraySet>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        if let Some((handle, _)) = self.looks.get(&key) {
            return handle.clone();
        }
        let look = terrain_slot_look(slots, arrays, key.0, key.1);
        let handle = materials.add(look.material());
        self.looks.insert(key, (handle.clone(), look));
        handle
    }

    fn drop_source(&mut self, commands: &mut Commands, stats: &mut SurfaceTraceStats, entity: Entity) {
        if let Some(old) = self.sources.remove(&entity) {
            for (proxy, _) in &old.parts {
                commands.entity(*proxy).try_despawn();
            }
            stats.terrain_proxies -= old.parts.len();
            stats.terrain_triangles -= old.triangles;
            stats.terrain_vertices -= old.vertices;
        }
    }
}

type TerrainSourceFilter = Or<(With<Chunk>, With<TerrainDraped>)>;

/// The slot table, or the built-ins on a host without one, and the texture
/// arrays the surface materials bind.
#[derive(SystemParam)]
struct SlotTable<'w, 's> {
    slots: Option<Res<'w, TerrainMaterialSlots>>,
    bindings: Option<Res<'w, TerrainSurfaceBindings>>,
    fallback: Local<'s, Option<TerrainMaterialSlots>>,
}

impl SlotTable<'_, '_> {
    fn changed(&self) -> bool {
        self.slots.as_ref().is_some_and(|slots| slots.is_changed())
            || self.bindings.as_ref().is_some_and(|bindings| bindings.is_changed())
    }

    fn get(&mut self) -> (&TerrainMaterialSlots, Option<&TerrainTextureArraySet>) {
        let arrays = self.bindings.as_deref().and_then(TerrainSurfaceBindings::arrays);
        let slots = match self.slots.as_deref() {
            Some(slots) => slots,
            None => self.fallback.get_or_insert_with(TerrainMaterialSlots::default),
        };
        (slots, arrays)
    }
}

/// Queue every chunk and draped mesh whose render mesh or visibility
/// changed, and move the proxies of one whose material swapped between the
/// vertex colours and the textured surface onto the other look.
#[allow(clippy::type_complexity)]
fn queue_terrain_sources(
    mut commands: Commands,
    mut state: ResMut<TerrainProxies>,
    mut table: SlotTable,
    mut materials: ResMut<Assets<StandardMaterial>>,
    changed: Query<
        Entity,
        (TerrainSourceFilter, With<Mesh3d>, Or<(Changed<Mesh3d>, Changed<InheritedVisibility>)>),
    >,
    swapped: Query<
        (Entity, Has<MeshMaterial3d<TerrainSurfaceMaterial>>),
        (
            TerrainSourceFilter,
            Or<(Added<MeshMaterial3d<TerrainSurfaceMaterial>>, Added<MeshMaterial3d<StandardMaterial>>)>,
        ),
    >,
) {
    let state = &mut *state;
    for entity in &changed {
        state.pending.insert(entity);
    }
    for (entity, textured) in &swapped {
        let Some(source) = state.sources.get_mut(&entity) else {
            continue;
        };
        if source.textured == textured {
            continue;
        }
        source.textured = textured;
        let parts = source.parts.clone();
        let (slots, arrays) = table.get();
        for (proxy, slot) in parts {
            let handle = state.look_handle((slot, textured), slots, arrays, &mut materials);
            commands.entity(proxy).try_insert(MeshMaterial3d(handle));
        }
    }
}

/// The terrain every chunk reads: the first root (the chunk spawner keeps
/// to one), through its bake.
type RootQuery<'w, 's> =
    Query<'w, 's, (&'static TerrainConfig, &'static TerrainData, Option<&'static TerrainBaked>), With<TerrainRoot>>;

/// Start building the queued chunks' proxies off the main thread, nearest
/// the view first, at most [`SurfaceTraceBudget::chunk_builds_per_frame`]
/// at once. Splitting a LOD-0 chunk takes milliseconds (tens of them in the
/// unoptimised dev build), so it runs on the async compute pool from a copy
/// of the chunk's mesh and of the material cells under it; only the copies
/// are made here. A hidden chunk keeps no proxy and needs no build.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn start_terrain_builds(
    mut commands: Commands,
    mut state: ResMut<TerrainProxies>,
    mut stats: ResMut<SurfaceTraceStats>,
    budget: Res<SurfaceTraceBudget>,
    meshes: Res<Assets<Mesh>>,
    sources: Query<(&Mesh3d, &GlobalTransform, Option<&InheritedVisibility>, Option<&Aabb>), TerrainSourceFilter>,
    roots: RootQuery,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    let state = &mut *state;
    // Chunks the budget left out come back once there is room for them.
    if !state.over_budget.is_empty() {
        let free = budget.terrain_triangles.saturating_sub(stats.terrain_triangles);
        let fits: Vec<Entity> = state.over_budget.iter().filter(|(_, need)| **need <= free).map(|(e, _)| *e).collect();
        for entity in fits {
            state.over_budget.remove(&entity);
            state.pending.insert(entity);
        }
    }
    if state.pending.is_empty() {
        return;
    }
    // Gone, or no longer a terrain mesh: nothing to build.
    state.pending.retain(|entity| sources.contains(*entity));
    state.over_budget.retain(|entity, _| sources.contains(*entity));
    let viewer = scene_camera_translation(&cameras).unwrap_or(Vec3::ZERO);
    let mut queue: Vec<(f32, Entity)> = state
        .pending
        .iter()
        .filter_map(|&entity| Some((sources.get(entity).ok()?.1.translation().distance_squared(viewer), entity)))
        .collect();
    queue.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));

    let ground = roots.iter().next().map(|(config, data, baked)| (config, surface_data(data, baked)));
    let pool = AsyncComputeTaskPool::get();
    for (_, entity) in queue {
        let Ok((mesh, at, visibility, aabb)) = sources.get(entity) else {
            continue;
        };
        if !visibility.is_some_and(|v| v.get()) {
            state.pending.remove(&entity);
            state.building.remove(&entity);
            state.over_budget.remove(&entity);
            let textured = state.sources.get(&entity).is_some_and(|source| source.textured);
            state.drop_source(&mut commands, &mut stats, entity);
            state.sources.insert(entity, TerrainSource { textured, parts: Vec::new(), triangles: 0, vertices: 0 });
            stats.chunk_builds += 1;
            continue;
        }
        if !state.building.contains_key(&entity) && state.building.len() >= budget.chunk_builds_per_frame.max(1) {
            break;
        }
        // A mesh not there yet stays queued and is looked for next frame.
        let Some(raster) = meshes.get(&mesh.0) else {
            continue;
        };
        let area = local_bounds(raster, aabb).map(|(min, max)| world_xz_bounds(min, max, at));
        let slot_ground = match ground {
            Some((config, data)) => SlotGround::capture(config, data, area),
            None => SlotGround::HeightBands(HeightBlendParams::default()),
        };
        let (source, to_world, parts) = (raster.clone(), *at, budget.chunk_parts);
        let task = pool.spawn(async move { split_terrain_mesh(&source, &to_world, &slot_ground, parts) });
        state.building.insert(entity, task);
        state.pending.remove(&entity);
    }
}

/// Take the finished chunk builds and trace them: the budget first (room
/// is made by leaving out chunks farther than this one, farthest first; if
/// that is not enough, this one is left out), then one proxy per part on the
/// shared slot material of the look the chunk wears now.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn finish_terrain_builds<M: TraceMarker>(
    mut commands: Commands,
    mut state: ResMut<TerrainProxies>,
    mut stats: ResMut<SurfaceTraceStats>,
    budget: Res<SurfaceTraceBudget>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut table: SlotTable,
    sources: Query<(&GlobalTransform, Has<MeshMaterial3d<TerrainSurfaceMaterial>>), TerrainSourceFilter>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    let state = &mut *state;
    if state.building.is_empty() {
        return;
    }
    let mut done: Vec<(Entity, Vec<TerrainPart>)> = Vec::new();
    for (entity, task) in state.building.iter_mut() {
        if let Some(parts) = check_ready(task) {
            done.push((*entity, parts));
        }
    }
    if done.is_empty() {
        return;
    }
    let viewer = scene_camera_translation(&cameras).unwrap_or(Vec3::ZERO);
    let distance_of = |entity: Entity| sources.get(entity).map_or(f32::MAX, |s| s.0.translation().distance_squared(viewer));
    // Nearest first, so ground arriving together takes the budget nearest
    // first too.
    done.sort_by(|a, b| distance_of(a.0).total_cmp(&distance_of(b.0)).then(a.0.cmp(&b.0)));
    for (entity, parts) in done {
        state.building.remove(&entity);
        let Ok((at, textured)) = sources.get(entity) else {
            continue;
        };
        stats.chunk_builds += 1;
        state.drop_source(&mut commands, &mut stats, entity);

        let need: u64 = parts.iter().map(|part| u64::from(part.triangles)).sum();
        let mut parts = parts;
        if stats.terrain_triangles + need > budget.terrain_triangles {
            let here = distance_of(entity);
            let mut farther: Vec<(f32, Entity)> = state
                .sources
                .iter()
                .filter(|(_, source)| source.triangles > 0)
                .map(|(other, _)| (distance_of(*other), *other))
                .filter(|(distance, _)| *distance > here)
                .collect();
            farther.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            for (_, other) in farther {
                if stats.terrain_triangles + need <= budget.terrain_triangles {
                    break;
                }
                let freed = state.sources.get(&other).map_or(0, |source| source.triangles);
                let textured = state.sources.get(&other).is_some_and(|source| source.textured);
                state.drop_source(&mut commands, &mut stats, other);
                state.sources.insert(other, TerrainSource { textured, parts: Vec::new(), triangles: 0, vertices: 0 });
                state.over_budget.insert(other, freed);
            }
            if stats.terrain_triangles + need > budget.terrain_triangles {
                state.over_budget.insert(entity, need);
                parts = Vec::new();
            }
        }

        let (slots, arrays) = table.get();
        let mut source = TerrainSource { textured, parts: Vec::with_capacity(parts.len()), triangles: 0, vertices: 0 };
        for part in parts {
            let material = state.look_handle((part.slot, textured), slots, arrays, &mut materials);
            let spec = ProxySpec {
                kind: ProxyKind::Terrain { slot: part.slot },
                mesh: meshes.add(part.mesh),
                material,
                triangles: part.triangles,
                vertices: part.vertices,
            };
            source.parts.push((spawn_proxy::<M>(&mut commands, entity, *at, spec), part.slot));
            source.triangles += u64::from(part.triangles);
            source.vertices += u64::from(part.vertices);
        }
        stats.terrain_proxies += source.parts.len();
        stats.terrain_triangles += source.triangles;
        stats.terrain_vertices += source.vertices;
        state.sources.insert(entity, source);
    }
}

/// Rewrite the shared slot materials whose look changed (a slot edited, the
/// texture arrays bound or released). Only materials whose values differ are
/// written, so an idle frame re-prepares nothing, and Solari, which copies
/// every `StandardMaterial` into the render world when any changes, copies
/// nothing.
fn refresh_terrain_looks(
    mut state: ResMut<TerrainProxies>,
    mut table: SlotTable,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut stats: ResMut<SurfaceTraceStats>,
) {
    // `TerrainSurfaceBindings` is written every frame, so this compares
    // every frame there; it reads one slot record per shared material.
    if state.looks.is_empty() || !table.changed() {
        return;
    }
    let (slots, arrays) = table.get();
    for (&(slot, textured), (handle, look)) in state.looks.iter_mut() {
        let now = terrain_slot_look(slots, arrays, slot, textured);
        if now == *look {
            continue;
        }
        if let Some(mut material) = materials.get_mut(&*handle) {
            now.apply(&mut material);
        }
        *look = now;
        stats.look_writes += 1;
    }
}

// ============================================================================
// Water
// ============================================================================

/// Depth over which the water shader fades the waterline in, metres
/// (`SHORE_FADE` in `water_surface.wgsl`).
const WATER_SHORE_FADE: f32 = 0.05;

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How opaque the raster draws water `depth` metres deep, as
/// `water_surface.wgsl` computes it (foam left out: it only covers the
/// first half metre).
pub fn water_opacity(params: &WaterSurfaceParams, depth: f32) -> f32 {
    let t = smoothstep(0.0, params.deep_depth.max(1e-3), depth);
    let alpha = params.shallow_color.w + (params.deep_color.w - params.shallow_color.w) * t;
    alpha * smoothstep(0.0, WATER_SHORE_FADE, depth)
}

/// The shallowest depth from which on the raster draws water at least
/// `opacity` opaque; `None` when it never does (clear water, whose bed must
/// stay lit).
pub fn traced_water_depth(params: &WaterSurfaceParams, opacity: f32) -> Option<f32> {
    let deep = params.deep_depth.max(1e-3);
    if !(water_opacity(params, deep) >= opacity) {
        return None;
    }
    // The opacity blends the shallow and deep alphas by a weight that rises
    // with depth, so once it reaches `opacity` it stays there.
    let (mut low, mut high) = (0.0f32, deep);
    for _ in 0..40 {
        let middle = 0.5 * (low + high);
        if water_opacity(params, middle) >= opacity {
            high = middle;
        } else {
            low = middle;
        }
    }
    Some(high)
}

/// The look of water drawn with `base` and `params`: the deep colour (the
/// traced parts are deep), and the water's own roughness, metallic and
/// reflectance (0.35, F0 0.16 * 0.35^2, about 0.02).
pub fn water_look(base: &StandardMaterial, params: &WaterSurfaceParams) -> SurfaceLook {
    let color = params.deep_color;
    SurfaceLook {
        albedo: [color.x, color.y, color.z].map(unit_color),
        roughness: base.perceptual_roughness.clamp(MIN_PERCEPTUAL_ROUGHNESS, 1.0),
        metallic: base.metallic.clamp(0.0, 1.0),
        reflectance: base.reflectance.clamp(0.0, 1.0),
    }
}

/// The parts of a water surface at least `min_depth` deep, laid out for
/// Solari. Each triangle is cut into pieces whose edges are at most `cell`
/// metres in the world (no more than `max_subdivision` to an edge, fewer
/// when the whole surface would pass `max_triangles` pieces), and a piece is
/// kept when every corner is that deep. `depth_at` measures a world point.
pub fn deep_water_mesh(
    mesh: &Mesh,
    to_world: &GlobalTransform,
    depth_at: &dyn Fn(Vec3) -> f32,
    min_depth: f32,
    cell: f32,
    max_subdivision: u32,
    max_triangles: usize,
) -> Option<Mesh> {
    let source = read_source(mesh)?;
    let cell = cell.max(0.01);
    let world = |p: [f32; 3]| to_world.transform_point(Vec3::from(p));
    let mut cuts: Vec<u32> = source
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|v| world(source.positions[v as usize]));
            let longest = a.distance(b).max(b.distance(c)).max(c.distance(a));
            ((longest / cell).ceil() as u32).clamp(1, max_subdivision.max(1))
        })
        .collect();
    let total: f64 = cuts.iter().map(|n| f64::from(*n) * f64::from(*n)).sum();
    let cap = max_triangles.max(1) as f64;
    if total > cap {
        let scale = (cap / total).sqrt();
        for n in &mut cuts {
            *n = ((f64::from(*n) * scale).floor() as u32).max(1);
        }
    }

    let (mut positions, mut normals, mut kept) = (Vec::new(), Vec::new(), Vec::new());
    for (triangle, &n) in source.triangles.iter().zip(&cuts) {
        let [a, b, c] = triangle.map(|v| Vec3::from(source.positions[v as usize]));
        let [na, nb, nc] = triangle.map(|v| Vec3::from(source.normals[v as usize]));
        let base = u32::try_from(positions.len()).ok()?;
        // Vertex (i, j) of the triangular grid lies i/n of the way from a to
        // b and j/n from a to c, i + j <= n; row i holds n + 1 - i of them,
        // so it starts i * (2n + 3 - i) / 2 in.
        let at = |i: u32, j: u32| i * (2 * n + 3 - i) / 2 + j;
        let mut deep = Vec::with_capacity(((n + 1) * (n + 2) / 2) as usize);
        for i in 0..=n {
            for j in 0..=(n - i) {
                let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                let local = a + (b - a) * u + (c - a) * v;
                let normal = (na + (nb - na) * u + (nc - na) * v).try_normalize().unwrap_or(Vec3::Y);
                positions.push(local.to_array());
                normals.push(normal.to_array());
                deep.push(depth_at(to_world.transform_point(local)) >= min_depth);
            }
        }
        let mut keep = |corners: [u32; 3]| {
            if corners.iter().all(|&k| deep[k as usize]) {
                kept.push(corners.map(|k| base + k));
            }
        };
        for i in 0..n {
            for j in 0..(n - i) {
                // Both pieces wind as the source triangle does.
                keep([at(i, j), at(i + 1, j), at(i, j + 1)]);
                if i + j + 1 < n {
                    keep([at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]);
                }
            }
        }
    }
    proxy_mesh(&positions, &normals, &kept)
}

/// Most ground samples a water build reads along one side. A lake reads
/// every raster cell under it; the ocean over a large raster reads every
/// n-th, which is finer than its cut anyway.
const MAX_GROUND_SAMPLES: u32 = 512;

/// The ground under a water surface as world heights, owned so the build
/// can run off the main thread: the raster cells under the surface, every
/// `stride`-th one when there are more than [`MAX_GROUND_SAMPLES`] a side.
/// Read bilinearly as `TerrainData::sample_height` reads the raster; at
/// stride 1 it is the same read.
pub struct GroundHeights {
    config: TerrainConfig,
    raster: UVec2,
    rect: RasterRect,
    stride: u32,
    samples: UVec2,
    heights: Vec<f32>,
}

impl GroundHeights {
    /// The ground under world XZ `area` (all of it for `None`); `None`
    /// without a height raster.
    pub fn capture(config: &TerrainConfig, data: &TerrainData, area: Option<(Vec2, Vec2)>) -> Option<Self> {
        if data.height_cache.is_empty() {
            return None;
        }
        let rect = RasterRect::covering(config, data, area)?;
        let stride = rect.size.max_element().div_ceil(MAX_GROUND_SAMPLES).max(1);
        let samples = (rect.size - UVec2::ONE) / stride + UVec2::ONE;
        let width = data.cache_width as usize;
        let mut heights = Vec::with_capacity(samples.x as usize * samples.y as usize);
        for j in 0..samples.y {
            let row = (rect.origin.y + j * stride) as usize * width;
            for i in 0..samples.x {
                let sample = data.height_cache.get(row + (rect.origin.x + i * stride) as usize).copied().unwrap_or(0.0);
                heights.push(config.world_height(sample));
            }
        }
        Some(Self {
            config: config.clone(),
            raster: UVec2::new(data.cache_width, data.cache_height),
            rect,
            stride,
            samples,
            heights,
        })
    }

    /// World height of the ground at world `x`, `z`.
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        let (u, v) = world_to_uv(&self.config, x, z);
        let raster = Vec2::new(u * (self.raster.x - 1) as f32, v * (self.raster.y - 1) as f32);
        let local = ((raster - self.rect.origin.as_vec2()) / self.stride as f32)
            .clamp(Vec2::ZERO, (self.samples - UVec2::ONE).as_vec2());
        let (x0, z0) = (local.x.floor() as u32, local.y.floor() as u32);
        let (x1, z1) = ((x0 + 1).min(self.samples.x - 1), (z0 + 1).min(self.samples.y - 1));
        let (fx, fz) = (local.x - x0 as f32, local.y - z0 as f32);
        let at = |x: u32, z: u32| self.heights[(z * self.samples.x + x) as usize];
        let near = at(x0, z0) + (at(x1, z0) - at(x0, z0)) * fx;
        let far = at(x0, z1) + (at(x1, z1) - at(x0, z1)) * fx;
        near + (far - near) * fz
    }
}

/// One water surface's proxy.
struct WaterSource {
    proxy: Option<Entity>,
    material: AssetId<WaterSurfaceMaterial>,
    triangles: u64,
    vertices: u64,
}

/// A water build running off the main thread, and the look its proxy takes.
struct WaterBuild {
    task: Task<Option<Mesh>>,
    look: SurfaceLook,
    material: AssetId<WaterSurfaceMaterial>,
}

/// The water proxies, the surfaces waiting for a build or being built, and
/// the ground's height stamp they were last measured against.
#[derive(Resource, Default)]
pub struct WaterProxies {
    sources: HashMap<Entity, WaterSource>,
    pending: HashSet<Entity>,
    building: HashMap<Entity, WaterBuild>,
    looks: HashMap<[u32; 6], Handle<StandardMaterial>>,
    seen_height_seq: u64,
    /// Frames since the ground's heights last changed, while a change waits
    /// to be measured.
    quiet_for: Option<u32>,
}

impl WaterProxies {
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Water surfaces whose build is running.
    pub fn in_flight(&self) -> usize {
        self.building.len()
    }

    /// Work waits: a build queued or running, or the ground going quiet.
    pub fn busy(&self) -> bool {
        !self.pending.is_empty() || !self.building.is_empty() || self.quiet_for.is_some()
    }

    /// The proxy of water surface `source`, `None` when none of it is deep
    /// enough to trace.
    pub fn proxy_of(&self, source: Entity) -> Option<Entity> {
        self.sources.get(&source).and_then(|s| s.proxy)
    }

    fn drop_source(&mut self, commands: &mut Commands, stats: &mut SurfaceTraceStats, entity: Entity) {
        if let Some(old) = self.sources.remove(&entity) {
            if let Some(proxy) = old.proxy {
                commands.entity(proxy).try_despawn();
                stats.water_proxies -= 1;
            }
            stats.water_triangles -= old.triangles;
            stats.water_vertices -= old.vertices;
        }
    }
}

/// Queue the water surfaces whose mesh, place, visibility or material
/// changed, and all of them once the ground has gone quiet after its
/// heights moved.
#[allow(clippy::type_complexity)]
fn queue_water_sources(
    mut state: ResMut<WaterProxies>,
    budget: Res<SurfaceTraceBudget>,
    dirty: Option<Res<TerrainDirtyChunks>>,
    changed: Query<
        Entity,
        (
            With<WaterSurface>,
            With<Mesh3d>,
            Or<(
                Changed<Mesh3d>,
                Changed<GlobalTransform>,
                Changed<InheritedVisibility>,
                Changed<MeshMaterial3d<WaterSurfaceMaterial>>,
            )>,
        ),
    >,
    roots: Query<(), (With<TerrainRoot>, Or<(Changed<TerrainConfig>, Added<TerrainBaked>)>)>,
    mut material_events: MessageReader<AssetEvent<WaterSurfaceMaterial>>,
) {
    let state = &mut *state;
    for entity in &changed {
        state.pending.insert(entity);
    }
    for event in material_events.read() {
        if let AssetEvent::Modified { id } = *event {
            let built = state.sources.iter().map(|(entity, source)| (*entity, source.material));
            let running = state.building.iter().map(|(entity, build)| (*entity, build.material));
            let using: Vec<Entity> = built.chain(running).filter(|(_, material)| *material == id).map(|(e, _)| e).collect();
            state.pending.extend(using);
        }
    }
    let seq = dirty.as_ref().map_or(0, |dirty| dirty.height_seq());
    if seq != state.seen_height_seq || !roots.is_empty() {
        state.seen_height_seq = seq;
        state.quiet_for = Some(0);
    } else if let Some(frames) = state.quiet_for.as_mut() {
        *frames += 1;
        if *frames >= budget.water_quiet_frames {
            state.quiet_for = None;
            // Running builds measured the old ground: they start again too.
            let all: Vec<Entity> = state.sources.keys().chain(state.building.keys()).copied().collect();
            state.pending.extend(all);
        }
    }
}

/// Start building the queued water surfaces' proxies off the main thread
/// (the ocean is cut into tens of thousands of pieces, each measured against
/// the ground), from a copy of the surface's mesh and of the ground heights
/// under it. Water that is hidden, or never opaque enough to trace, gets no
/// proxy at once. There are few water surfaces, so every queued one starts.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn start_water_builds(
    mut commands: Commands,
    mut state: ResMut<WaterProxies>,
    mut stats: ResMut<SurfaceTraceStats>,
    budget: Res<SurfaceTraceBudget>,
    meshes: Res<Assets<Mesh>>,
    water_materials: Res<Assets<WaterSurfaceMaterial>>,
    sources: Query<
        (&Mesh3d, &GlobalTransform, Option<&InheritedVisibility>, &MeshMaterial3d<WaterSurfaceMaterial>),
        With<WaterSurface>,
    >,
    roots: RootQuery,
) {
    if state.pending.is_empty() {
        return;
    }
    let state = &mut *state;
    let ground = roots.iter().next().map(|(config, data, baked)| (config, surface_data(data, baked)));
    let pool = AsyncComputeTaskPool::get();
    let pending: Vec<Entity> = state.pending.drain().collect();
    for entity in pending {
        let Ok((mesh, at, visibility, water)) = sources.get(entity) else {
            continue;
        };
        let (Some(raster), Some(water_material)) = (meshes.get(&mesh.0), water_materials.get(&water.0)) else {
            // Not there yet: try again next frame.
            state.pending.insert(entity);
            continue;
        };
        let params = water_material.extension.params;
        let look = water_look(&water_material.base, &params);
        let visible = visibility.is_some_and(|v| v.get());
        let Some(min_depth) = traced_water_depth(&params, budget.water_opacity).filter(|_| visible) else {
            state.building.remove(&entity);
            state.drop_source(&mut commands, &mut stats, entity);
            state.sources.insert(entity, WaterSource { proxy: None, material: water.0.id(), triangles: 0, vertices: 0 });
            stats.water_builds += 1;
            continue;
        };
        // As the shader: with no height raster every pixel is the fallback
        // depth.
        let area = local_bounds(raster, None).map(|(min, max)| world_xz_bounds(min, max, at));
        let heights = ground.and_then(|(config, data)| GroundHeights::capture(config, data, area));
        let fallback = params.fallback_depth;
        let (source, to_world) = (raster.clone(), *at);
        let (cell, subdivision, cap) = (budget.water_cell, budget.water_max_subdivision, budget.water_triangles);
        let task = pool.spawn(async move {
            let depth_at = |p: Vec3| heights.as_ref().map_or(fallback, |ground| p.y - ground.height_at(p.x, p.z));
            deep_water_mesh(&source, &to_world, &depth_at, min_depth, cell, subdivision, cap)
        });
        state.building.insert(entity, WaterBuild { task, look, material: water.0.id() });
    }
}

/// Take the finished water builds and trace them.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn finish_water_builds<M: TraceMarker>(
    mut commands: Commands,
    mut state: ResMut<WaterProxies>,
    mut stats: ResMut<SurfaceTraceStats>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sources: Query<&GlobalTransform, With<WaterSurface>>,
) {
    let state = &mut *state;
    if state.building.is_empty() {
        return;
    }
    let mut done: Vec<(Entity, Option<Mesh>)> = Vec::new();
    for (entity, build) in state.building.iter_mut() {
        if let Some(mesh) = check_ready(&mut build.task) {
            done.push((*entity, mesh));
        }
    }
    for (entity, proxy_mesh) in done {
        let Some(build) = state.building.remove(&entity) else {
            continue;
        };
        let Ok(at) = sources.get(entity) else {
            continue;
        };
        stats.water_builds += 1;
        state.drop_source(&mut commands, &mut stats, entity);
        let mut source = WaterSource { proxy: None, material: build.material, triangles: 0, vertices: 0 };
        if let Some(proxy_mesh) = proxy_mesh {
            let (triangles, vertices) = mesh_counts(&proxy_mesh);
            let look = build.look;
            let material = state.looks.entry(look.key()).or_insert_with(|| materials.add(look.material())).clone();
            let spec = ProxySpec { kind: ProxyKind::Water, mesh: meshes.add(proxy_mesh), material, triangles, vertices };
            source.proxy = Some(spawn_proxy::<M>(&mut commands, entity, *at, spec));
            source.triangles = u64::from(triangles);
            source.vertices = u64::from(vertices);
            stats.water_proxies += 1;
            stats.water_triangles += source.triangles;
            stats.water_vertices += source.vertices;
        }
        state.sources.insert(entity, source);
    }
}

// ============================================================================
// Scatter
// ============================================================================

/// A group of a mesh's triangles that share roughly one colour.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorGroup {
    /// Area-weighted mean linear RGB.
    pub color: [f32; 3],
    pub triangles: Vec<[u32; 3]>,
}

/// Colour distance, linear RGB, under which two groups are one.
const SAME_COLOR: f32 = 0.06;

/// The triangles of a vertex-coloured mesh grouped by colour into at most
/// `groups` groups (bark and crown), heaviest first, each in its
/// area-weighted mean colour; one white group without colours. A small
/// k-means seeded by the farthest colour, so the split is the same every
/// time.
pub fn group_by_color(
    positions: &[[f32; 3]],
    colors: Option<&[[f32; 4]]>,
    triangles: &[[u32; 3]],
    groups: usize,
) -> Vec<ColorGroup> {
    if triangles.is_empty() {
        return Vec::new();
    }
    let Some(colors) = colors else {
        return vec![ColorGroup { color: [1.0; 3], triangles: triangles.to_vec() }];
    };
    let samples: Vec<(Vec3, f32)> = triangles
        .iter()
        .map(|t| {
            let color = t.iter().map(|&v| Vec3::from_slice(&colors[v as usize][..3])).sum::<Vec3>() / 3.0;
            let [a, b, c] = t.map(|v| Vec3::from(positions[v as usize]));
            (color, ((b - a).cross(c - a).length() * 0.5).max(1e-9))
        })
        .collect();
    let mean_of = |members: &mut dyn Iterator<Item = usize>| -> Option<Vec3> {
        let (sum, weight) =
            members.fold((Vec3::ZERO, 0.0f32), |(sum, weight), t| (sum + samples[t].0 * samples[t].1, weight + samples[t].1));
        (weight > 0.0).then(|| sum / weight)
    };
    let nearest = |centers: &[Vec3], color: Vec3| {
        let mut best = 0;
        for (k, center) in centers.iter().enumerate() {
            if center.distance(color) < centers[best].distance(color) {
                best = k;
            }
        }
        best
    };

    let mut centers = vec![mean_of(&mut (0..samples.len())).unwrap_or(Vec3::ONE)];
    while centers.len() < groups.max(1) {
        let (mut farthest, mut far) = (None, SAME_COLOR);
        for (color, _) in &samples {
            let d = centers.iter().map(|c| c.distance(*color)).fold(f32::MAX, f32::min);
            if d > far {
                far = d;
                farthest = Some(*color);
            }
        }
        match farthest {
            Some(color) => centers.push(color),
            None => break,
        }
    }
    let mut assignment = vec![0usize; samples.len()];
    for _ in 0..6 {
        for (t, slot) in assignment.iter_mut().enumerate() {
            *slot = nearest(&centers, samples[t].0);
        }
        for (k, center) in centers.iter_mut().enumerate() {
            if let Some(mean) = mean_of(&mut (0..samples.len()).filter(|t| assignment[*t] == k)) {
                *center = mean;
            }
        }
    }
    for (t, slot) in assignment.iter_mut().enumerate() {
        *slot = nearest(&centers, samples[t].0);
    }

    // Heaviest first; a group that ended up one colour with a heavier one
    // joins it.
    let weight_of = |k: usize| (0..samples.len()).filter(|t| assignment[*t] == k).map(|t| samples[t].1).sum::<f32>();
    let mut order: Vec<usize> = (0..centers.len()).filter(|k| weight_of(*k) > 0.0).collect();
    order.sort_by(|a, b| weight_of(*b).total_cmp(&weight_of(*a)).then(a.cmp(b)));
    let mut into: Vec<usize> = (0..centers.len()).collect();
    for (i, &k) in order.iter().enumerate() {
        if let Some(&j) = order[..i].iter().find(|&&j| into[j] == j && centers[j].distance(centers[k]) < SAME_COLOR) {
            into[k] = j;
        }
    }
    order
        .iter()
        .filter(|&&k| into[k] == k)
        .map(|&k| {
            let members: Vec<usize> = (0..samples.len()).filter(|t| into[assignment[*t]] == k).collect();
            let color = mean_of(&mut members.iter().copied()).unwrap_or(centers[k]);
            ColorGroup { color: color.to_array(), triangles: members.iter().map(|&t| triangles[t]).collect() }
        })
        .collect()
}

/// A source material's look in one colour group: its colour times the
/// group's, and its own roughness, metallic and reflectance.
fn group_look(base: &StandardMaterial, color: [f32; 3]) -> SurfaceLook {
    let tint = base.base_color.to_linear();
    SurfaceLook {
        albedo: [tint.red * color[0], tint.green * color[1], tint.blue * color[2]].map(unit_color),
        roughness: base.perceptual_roughness.clamp(MIN_PERCEPTUAL_ROUGHNESS, 1.0),
        metallic: base.metallic.clamp(0.0, 1.0),
        reflectance: base.reflectance.clamp(0.0, 1.0),
    }
}

/// A source material a proxy may stand for, by the core module's own rules:
/// opaque (Solari makes every traced surface opaque), lit, and not emissive
/// (a proxy is never a light).
fn proxy_worthy(material: &StandardMaterial) -> bool {
    let e = material.emissive;
    matches!(material.alpha_mode, AlphaMode::Opaque) && !material.unlit && e.red == 0.0 && e.green == 0.0 && e.blue == 0.0
}

/// One shared proxy mesh and material of a prototype.
#[derive(Clone, Debug)]
struct PrototypePart {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    triangles: u32,
    vertices: u32,
}

/// How a scatter mesh is traced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScatterClass {
    /// Through this module's colour-grouped proxies: the mesh carries an
    /// attribute only the raster path reads (the built-in meshes' vertex
    /// colours), so the core module skips it.
    Proxy,
    /// Through the core module, which traces the mesh as it is (a Custom
    /// layer's asset); this module only applies the distance budget.
    Core,
    /// Not traced.
    Untraced,
    /// Its mesh has not loaded.
    Loading,
}

/// Which path a scatter mesh takes, by the core module's own [`conform`].
pub fn scatter_class(mesh: &Mesh) -> ScatterClass {
    match conform(mesh) {
        Conformance::AsIs { .. } | Conformance::Copy(_) => ScatterClass::Core,
        Conformance::Skip(SkipReason::RasterOnlyAttribute) => ScatterClass::Proxy,
        Conformance::Skip(_) => ScatterClass::Untraced,
    }
}

type PrototypeKey = (AssetId<Mesh>, AssetId<StandardMaterial>);

struct Prototype {
    class: ScatterClass,
    /// Built the first time an entity of it is traced.
    parts: Option<Vec<PrototypePart>>,
    /// The parts were tried (built, or found untraceable by the material).
    attempted: bool,
    /// The material they were tried with (`None`: it had not loaded), so a
    /// material event that changes nothing a proxy reads rebuilds nothing.
    material: Option<MaterialKey>,
}

/// What a proxy reads from a source material.
type MaterialKey = (bool, [u32; 6]);

fn material_key(material: &StandardMaterial) -> MaterialKey {
    (proxy_worthy(material), group_look(material, [1.0; 3]).key())
}

struct InstanceEntry {
    entity: Entity,
    position: Vec3,
    key: PrototypeKey,
}

struct TileEntry {
    triangles: u32,
    center: Vec3,
    reach: f32,
}

/// One scatter batch as indexed when it spawned: its entities and its
/// merged tile.
struct BatchEntry {
    center: Vec3,
    reach: f32,
    instances: Vec<InstanceEntry>,
    tile: Option<TileEntry>,
}

/// What this module did to a traced scatter entity, to undo it.
#[derive(Clone, Copy)]
struct TracedInstance {
    key: PrototypeKey,
    class: ScatterClass,
    proxies: usize,
}

/// The scatter index, the prototypes, and what is traced now.
#[derive(Resource, Default)]
pub struct ScatterProxies {
    batches: HashMap<Entity, BatchEntry>,
    prototypes: HashMap<PrototypeKey, Prototype>,
    traced: HashMap<Entity, TracedInstance>,
    /// Traced merged tiles and their triangles and vertices.
    tiles: HashMap<Entity, (u32, u32)>,
    /// Entities and tiles chosen but not yet traced, nearest first.
    pending_instances: Vec<(Entity, PrototypeKey)>,
    pending_tiles: Vec<Entity>,
    /// Tile proxies being built off the main thread: their meshes and looks.
    tile_building: HashMap<Entity, Task<Vec<(Mesh, SurfaceLook)>>>,
    last_view: Option<Vec3>,
    /// The index changed since the budget was last chosen.
    dirty: bool,
    /// Core-path entities this module holds out of the traced scene.
    held: HashSet<Entity>,
}

impl ScatterProxies {
    /// Whether scatter entity `instance` is traced now.
    pub fn is_traced(&self, instance: Entity) -> bool {
        self.traced.contains_key(&instance)
    }

    pub fn traced_count(&self) -> usize {
        self.traced.len()
    }

    /// Whether this module holds core-path entity `instance` out.
    pub fn is_held(&self, instance: Entity) -> bool {
        self.held.contains(&instance)
    }

    pub fn is_tile_traced(&self, batch: Entity) -> bool {
        self.tiles.contains_key(&batch)
    }

    /// Entities and tiles chosen but not yet traced, tile builds running
    /// included.
    pub fn pending(&self) -> usize {
        self.pending_instances.len() + self.pending_tiles.len() + self.tile_building.len()
    }

    fn class_of(&mut self, key: PrototypeKey, meshes: &Assets<Mesh>) -> ScatterClass {
        if let Some(prototype) = self.prototypes.get(&key) {
            return prototype.class;
        }
        let class = match meshes.get(key.0) {
            Some(mesh) => scatter_class(mesh),
            None => ScatterClass::Loading,
        };
        self.prototypes.insert(key, Prototype { class, parts: None, attempted: false, material: None });
        class
    }

    /// Take `instance` out of the traced scene: its proxies go, or it is
    /// held out of the core path again.
    fn untrace(&mut self, commands: &mut Commands, instance: Entity) {
        let Some(traced) = self.traced.remove(&instance) else {
            return;
        };
        match traced.class {
            ScatterClass::Proxy => despawn_proxies_of(commands, instance),
            ScatterClass::Core => {
                commands.entity(instance).try_insert(TraceOverride::Never);
                self.held.insert(instance);
            }
            ScatterClass::Untraced | ScatterClass::Loading => {}
        }
    }

    fn untrace_tile(&mut self, commands: &mut Commands, stats: &mut SurfaceTraceStats, batch: Entity) {
        self.tile_building.remove(&batch);
        if let Some((triangles, vertices)) = self.tiles.remove(&batch) {
            despawn_proxies_of(commands, batch);
            stats.tiles -= 1;
            stats.tile_triangles -= u64::from(triangles);
            stats.tile_vertices -= u64::from(vertices);
        }
    }

    fn count_into(&self, stats: &mut SurfaceTraceStats) {
        stats.scatter_instances = self.traced.len();
        stats.scatter_proxies = self.traced.values().map(|t| t.proxies).sum();
    }
}

/// The meshes and materials the scatter proxies are made in.
#[derive(SystemParam)]
struct ProxyAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
}

/// The shared parts of prototype `key`, built on first use. `None` (and the
/// prototype becomes untraced) when its material is see-through, unlit or
/// emissive, or nothing of it can be traced.
fn prototype_parts(
    state: &mut ScatterProxies,
    key: PrototypeKey,
    groups: usize,
    assets: &mut ProxyAssets,
    stats: &mut SurfaceTraceStats,
) -> Option<Vec<PrototypePart>> {
    let prototype = state.prototypes.get(&key)?;
    if prototype.class != ScatterClass::Proxy {
        return None;
    }
    if let Some(parts) = &prototype.parts {
        return Some(parts.clone());
    }
    let base = assets.materials.get(key.1).cloned();
    let built = (|| {
        let base = base.clone()?;
        if !proxy_worthy(&base) {
            return None;
        }
        let source = read_source(assets.meshes.get(key.0)?)?;
        let mut made = Vec::new();
        for group in group_by_color(source.positions, source.colors, &source.triangles, groups) {
            if let Some(proxy) = proxy_mesh(source.positions, source.normals, &group.triangles) {
                made.push((proxy, group_look(&base, group.color)));
            }
        }
        let parts: Vec<PrototypePart> = made
            .into_iter()
            .map(|(proxy, look)| {
                let (triangles, vertices) = mesh_counts(&proxy);
                PrototypePart {
                    mesh: assets.meshes.add(proxy),
                    material: assets.materials.add(look.material()),
                    triangles,
                    vertices,
                }
            })
            .collect();
        (!parts.is_empty()).then_some(parts)
    })();
    let prototype = state.prototypes.get_mut(&key)?;
    prototype.attempted = true;
    prototype.material = base.as_ref().map(material_key);
    match &built {
        Some(parts) => {
            for part in parts {
                stats.prototype_triangles += u64::from(part.triangles);
                stats.prototype_vertices += u64::from(part.vertices);
            }
            prototype.parts = Some(parts.clone());
        }
        None => prototype.class = ScatterClass::Untraced,
    }
    built
}

/// The XZ bounding circle (centre and radius) of `points`, widened by
/// `margin`.
fn bounding_circle(points: impl Iterator<Item = Vec3>, margin: f32) -> Option<(Vec3, f32)> {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut any = false;
    for p in points {
        lo = lo.min(p);
        hi = hi.max(p);
        any = true;
    }
    any.then(|| ((lo + hi) * 0.5, (hi - lo).length() * 0.5 + margin))
}

/// Index every scatter batch the frame it spawns (its entities, where they
/// stand and what they draw, and its merged tile), hold core-path entities
/// out of the traced scene until the budget takes them, and decide a
/// prototype again when its mesh or material arrives or changes.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn index_scatter_batches(
    mut commands: Commands,
    mut state: ResMut<ScatterProxies>,
    mut stats: ResMut<SurfaceTraceStats>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    batches: Query<
        (
            Entity,
            &GlobalTransform,
            Option<&Children>,
            Option<&Mesh3d>,
            Option<&MeshMaterial3d<StandardMaterial>>,
            Has<NotShadowCaster>,
        ),
        Added<ScatterBatch>,
    >,
    instances: Query<(&GlobalTransform, &Mesh3d, &MeshMaterial3d<StandardMaterial>)>,
    hidden: Query<(Entity, &InheritedVisibility), (Changed<InheritedVisibility>, With<TraceProxies>)>,
    shown: Query<(), (With<ScatterBatch>, Changed<InheritedVisibility>)>,
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    mut material_events: MessageReader<AssetEvent<StandardMaterial>>,
) {
    let state = &mut *state;
    for (batch, at, children, mesh, material, no_shadow) in &batches {
        let mut entry = BatchEntry { center: at.translation(), reach: 0.0, instances: Vec::new(), tile: None };
        for &child in children.map(|c| &c[..]).unwrap_or(&[]) {
            let Ok((child_at, mesh, material)) = instances.get(child) else {
                continue;
            };
            let key = (mesh.0.id(), material.0.id());
            if matches!(state.class_of(key, &meshes), ScatterClass::Core | ScatterClass::Loading) {
                commands.entity(child).try_insert(TraceOverride::Never);
                state.held.insert(child);
            }
            entry.instances.push(InstanceEntry { entity: child, position: child_at.translation(), key });
        }
        if let Some((center, reach)) = bounding_circle(entry.instances.iter().map(|i| i.position), 1.0) {
            entry.center = center;
            entry.reach = reach;
        }
        // The merged tile is traced as the raster shadows it: a tile with
        // NotShadowCaster (grass) never is.
        if let (Some(mesh), Some(_), false) = (mesh, material, no_shadow) {
            if let Some(source) = meshes.get(&mesh.0) {
                let (triangles, _) = mesh_counts(source);
                let bounds = match source.try_attribute_option(Mesh::ATTRIBUTE_POSITION) {
                    Ok(Some(VertexAttributeValues::Float32x3(p))) => {
                        bounding_circle(p.iter().map(|v| at.transform_point(Vec3::from(*v))), 0.0)
                    }
                    _ => None,
                };
                if let Some((center, reach)) = bounds {
                    entry.tile = Some(TileEntry { triangles, center, reach });
                }
            }
        }
        state.batches.insert(batch, entry);
        state.dirty = true;
    }

    // A traced entity or tile that is hidden leaves the traced scene; the
    // next choice skips it until its batch shows again.
    for (entity, visibility) in &hidden {
        if visibility.get() {
            continue;
        }
        if state.traced.contains_key(&entity) {
            state.untrace(&mut commands, entity);
            state.dirty = true;
        }
        if state.tiles.contains_key(&entity) {
            state.untrace_tile(&mut commands, &mut stats, entity);
            state.dirty = true;
        }
    }
    if !shown.is_empty() {
        state.dirty = true;
    }

    // A prototype is decided again when its mesh arrives (it was loading),
    // changes or goes, or when its material changes something a proxy
    // reads. Comparing the material, not just hearing an event, keeps a
    // system that touches the shared scatter material every frame from
    // retracing every tree every frame.
    let mut stale: HashSet<PrototypeKey> = HashSet::new();
    for event in mesh_events.read() {
        match *event {
            AssetEvent::Added { id } | AssetEvent::LoadedWithDependencies { id } => stale.extend(
                state.prototypes.iter().filter(|(key, p)| key.0 == id && p.class == ScatterClass::Loading).map(|(key, _)| *key),
            ),
            AssetEvent::Modified { id } | AssetEvent::Removed { id } => {
                stale.extend(state.prototypes.keys().filter(|key| key.0 == id))
            }
            _ => {}
        }
    }
    for event in material_events.read() {
        if let AssetEvent::Added { id } | AssetEvent::Modified { id } | AssetEvent::Removed { id } = *event {
            let now = materials.get(id).map(material_key);
            stale.extend(
                state
                    .prototypes
                    .iter()
                    .filter(|(key, p)| key.1 == id && p.attempted && p.material != now)
                    .map(|(key, _)| *key),
            );
        }
    }
    if !stale.is_empty() {
        for key in &stale {
            let parts = state.prototypes.remove(key).and_then(|p| p.parts).unwrap_or_default();
            for part in parts {
                stats.prototype_triangles -= u64::from(part.triangles);
                stats.prototype_vertices -= u64::from(part.vertices);
            }
        }
        // Traced entities of a stale prototype are untraced, so the next
        // choice traces them again from its new parts.
        let gone: Vec<Entity> = state.traced.iter().filter(|(_, t)| stale.contains(&t.key)).map(|(e, _)| *e).collect();
        for instance in gone {
            state.untrace(&mut commands, instance);
        }
        state.dirty = true;
    }
    state.count_into(&mut stats);
}

/// Choose which scatter is traced: when the view has moved a step or the
/// index changed, the nearest visible entities within the radius up to the
/// count budget, and the nearest merged tiles within theirs up to the
/// triangle budget. What leaves the choice is untraced at once; what joins
/// it is queued nearest first.
#[allow(clippy::too_many_arguments)]
fn choose_scatter(
    mut commands: Commands,
    mut state: ResMut<ScatterProxies>,
    mut stats: ResMut<SurfaceTraceStats>,
    budget: Res<SurfaceTraceBudget>,
    meshes: Res<Assets<Mesh>>,
    visibility: Query<&InheritedVisibility>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    let state = &mut *state;
    let Some(view) = scene_camera_translation(&cameras) else {
        return;
    };
    let moved = state.last_view.is_none_or(|last| last.distance(view) >= budget.scatter_reselect_step);
    if !moved && !state.dirty {
        return;
    }
    state.last_view = Some(view);
    state.dirty = false;
    stats.scatter_passes += 1;
    let visible = |entity: Entity| visibility.get(entity).is_ok_and(|v| v.get());

    // Entities: only batches whose circle reaches the radius are opened.
    let radius = budget.scatter_radius;
    let mut near: Vec<(f32, Entity, PrototypeKey)> = Vec::new();
    for batch in state.batches.values() {
        if batch.instances.is_empty() || batch.center.distance(view) - batch.reach > radius {
            continue;
        }
        for instance in &batch.instances {
            let distance = instance.position.distance(view);
            if distance <= radius {
                near.push((distance, instance.entity, instance.key));
            }
        }
    }
    near.retain(|(_, entity, key)| {
        matches!(state.class_of(*key, &meshes), ScatterClass::Proxy | ScatterClass::Core) && visible(*entity)
    });
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    near.truncate(budget.scatter_instances);
    let chosen: HashSet<Entity> = near.iter().map(|(_, entity, _)| *entity).collect();
    let leaving: Vec<Entity> = state.traced.keys().filter(|e| !chosen.contains(e)).copied().collect();
    for instance in leaving {
        state.untrace(&mut commands, instance);
    }
    state.pending_instances =
        near.iter().filter(|(_, entity, _)| !state.traced.contains_key(entity)).map(|(_, e, key)| (*e, *key)).collect();

    // Merged tiles, nearest first, while the triangle budget lasts.
    let mut tiles: Vec<(f32, Entity, u32)> = state
        .batches
        .iter()
        .filter_map(|(batch, entry)| {
            let tile = entry.tile.as_ref()?;
            let distance = (tile.center.distance(view) - tile.reach).max(0.0);
            (distance <= budget.tile_radius && tile.triangles <= budget.tile_triangles && visible(*batch))
                .then_some((distance, *batch, tile.triangles))
        })
        .collect();
    tiles.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut spent = 0u32;
    let mut chosen_tiles = Vec::new();
    for (_, batch, triangles) in tiles {
        if spent.saturating_add(triangles) <= budget.tile_triangle_budget {
            spent += triangles;
            chosen_tiles.push(batch);
        }
    }
    let leaving: Vec<Entity> =
        state.tiles.keys().chain(state.tile_building.keys()).filter(|b| !chosen_tiles.contains(b)).copied().collect();
    for batch in leaving {
        state.untrace_tile(&mut commands, &mut stats, batch);
    }
    state.pending_tiles = chosen_tiles
        .into_iter()
        .filter(|b| !state.tiles.contains_key(b) && !state.tile_building.contains_key(b))
        .collect();
    state.count_into(&mut stats);
}

/// Trace the queued scatter: entities up to the per-frame count, tiles up to
/// theirs.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn trace_scatter<M: TraceMarker>(
    mut commands: Commands,
    mut state: ResMut<ScatterProxies>,
    mut stats: ResMut<SurfaceTraceStats>,
    budget: Res<SurfaceTraceBudget>,
    mut assets: ProxyAssets,
    instances: Query<&GlobalTransform>,
    batches: Query<
        (&GlobalTransform, &Mesh3d, &MeshMaterial3d<StandardMaterial>, Option<&InheritedVisibility>),
        With<ScatterBatch>,
    >,
) {
    let state = &mut *state;
    if state.pending_instances.is_empty() && state.pending_tiles.is_empty() && state.tile_building.is_empty() {
        return;
    }
    let take = state.pending_instances.len().min(budget.scatter_spawns_per_frame.max(1));
    let now: Vec<(Entity, PrototypeKey)> = state.pending_instances.drain(..take).collect();
    for (instance, key) in now {
        let Ok(at) = instances.get(instance) else {
            continue;
        };
        let class = state.prototypes.get(&key).map_or(ScatterClass::Loading, |p| p.class);
        let proxies = match class {
            ScatterClass::Proxy => {
                let Some(parts) = prototype_parts(state, key, budget.prototype_groups, &mut assets, &mut stats) else {
                    continue;
                };
                for part in &parts {
                    let spec = ProxySpec {
                        kind: ProxyKind::ScatterInstance,
                        mesh: part.mesh.clone(),
                        material: part.material.clone(),
                        triangles: part.triangles,
                        vertices: part.vertices,
                    };
                    spawn_proxy::<M>(&mut commands, instance, *at, spec);
                }
                parts.len()
            }
            ScatterClass::Core => {
                commands.entity(instance).try_remove::<TraceOverride>();
                state.held.remove(&instance);
                0
            }
            ScatterClass::Untraced | ScatterClass::Loading => continue,
        };
        state.traced.insert(instance, TracedInstance { key, class, proxies });
    }

    // Tiles: finished builds are traced, then more start, at most
    // `tile_builds_per_frame` running at once. A dense tile is tens of
    // thousands of triangles, so it is grouped and laid out off the main
    // thread from a copy of its mesh.
    let mut done: Vec<(Entity, Vec<(Mesh, SurfaceLook)>)> = Vec::new();
    for (batch, task) in state.tile_building.iter_mut() {
        if let Some(made) = check_ready(task) {
            done.push((*batch, made));
        }
    }
    for (batch, made) in done {
        state.tile_building.remove(&batch);
        // Hidden while it was built: the next choice decides again.
        let Ok((at, _, _, Some(visibility))) = batches.get(batch) else {
            continue;
        };
        if !visibility.get() {
            state.dirty = true;
            continue;
        }
        let (mut triangles_total, mut vertices_total) = (0u32, 0u32);
        for (proxy, look) in made {
            let (triangles, vertices) = mesh_counts(&proxy);
            let spec = ProxySpec {
                kind: ProxyKind::ScatterTile,
                mesh: assets.meshes.add(proxy),
                material: assets.materials.add(look.material()),
                triangles,
                vertices,
            };
            spawn_proxy::<M>(&mut commands, batch, *at, spec);
            triangles_total += triangles;
            vertices_total += vertices;
        }
        state.tiles.insert(batch, (triangles_total, vertices_total));
        stats.tiles += 1;
        stats.tile_triangles += u64::from(triangles_total);
        stats.tile_vertices += u64::from(vertices_total);
    }
    let room = budget.tile_builds_per_frame.max(1).saturating_sub(state.tile_building.len());
    let take = state.pending_tiles.len().min(room);
    let now: Vec<Entity> = state.pending_tiles.drain(..take).collect();
    let pool = AsyncComputeTaskPool::get();
    for batch in now {
        let Ok((_, mesh, material, _)) = batches.get(batch) else {
            continue;
        };
        let (Some(source), Some(base)) =
            (assets.meshes.get(&mesh.0), assets.materials.get(&material.0).filter(|m| proxy_worthy(m)))
        else {
            continue;
        };
        let (source, base) = (source.clone(), base.clone());
        let task = pool.spawn(async move {
            let Some(source) = read_source(&source) else {
                return Vec::new();
            };
            // Small things: one group is enough for their bounce colour.
            group_by_color(source.positions, source.colors, &source.triangles, 1)
                .into_iter()
                .filter_map(|group| {
                    let proxy = proxy_mesh(source.positions, source.normals, &group.triangles)?;
                    Some((proxy, group_look(&base, group.color)))
                })
                .collect()
        });
        state.tile_building.insert(batch, task);
    }
    state.count_into(&mut stats);
}

// ============================================================================
// Sources that go or move
// ============================================================================

/// Forget sources that are gone. Their proxies went with them (the
/// relationship despawns them); this keeps the books, and drops the proxies
/// of a source that only stopped being one (a draped mesh that is no longer
/// ground).
#[allow(clippy::too_many_arguments)]
fn forget_gone_sources(
    mut commands: Commands,
    mut terrain: ResMut<TerrainProxies>,
    mut water: ResMut<WaterProxies>,
    mut scatter: ResMut<ScatterProxies>,
    mut stats: ResMut<SurfaceTraceStats>,
    mut chunks: RemovedComponents<Chunk>,
    mut draped: RemovedComponents<TerrainDraped>,
    mut waters: RemovedComponents<WaterSurface>,
    mut batches: RemovedComponents<ScatterBatch>,
) {
    for entity in chunks.read().chain(draped.read()) {
        terrain.pending.remove(&entity);
        terrain.building.remove(&entity);
        terrain.over_budget.remove(&entity);
        terrain.drop_source(&mut commands, &mut stats, entity);
    }
    for entity in waters.read() {
        water.pending.remove(&entity);
        water.building.remove(&entity);
        water.drop_source(&mut commands, &mut stats, entity);
    }
    let scatter = &mut *scatter;
    let mut any = false;
    for batch in batches.read() {
        any = true;
        let Some(entry) = scatter.batches.remove(&batch) else {
            continue;
        };
        for instance in &entry.instances {
            scatter.held.remove(&instance.entity);
            scatter.traced.remove(&instance.entity);
        }
        scatter.untrace_tile(&mut commands, &mut stats, batch);
        scatter.tile_building.remove(&batch);
        let gone: HashSet<Entity> = entry.instances.iter().map(|i| i.entity).collect();
        scatter.pending_instances.retain(|(e, _)| !gone.contains(e));
        scatter.pending_tiles.retain(|b| *b != batch);
        scatter.dirty = true;
    }
    if any {
        scatter.count_into(&mut stats);
    }
}

/// Keep proxies where their sources are. Change-driven: static sources
/// (chunks, scatter) cost nothing.
#[allow(clippy::type_complexity)]
fn follow_sources(
    moved: Query<(&GlobalTransform, &TraceProxies), (Changed<GlobalTransform>, Without<SurfaceProxy>)>,
    mut proxies: Query<(&mut Transform, &mut GlobalTransform), With<SurfaceProxy>>,
) {
    for (at, sources) in &moved {
        for &proxy in sources.proxies() {
            if let Ok((mut transform, mut global)) = proxies.get_mut(proxy) {
                *transform = at.compute_transform();
                *global = *at;
            }
        }
    }
}

/// One line when what is traced changes, at most every 240 frames.
fn log_surface_stats(stats: Res<SurfaceTraceStats>, mut last: Local<Option<(SurfaceTraceStats, u64)>>, mut frame: Local<u64>) {
    *frame += 1;
    let live = |s: &SurfaceTraceStats| {
        (s.terrain_proxies, s.water_proxies, s.scatter_instances, s.tiles, s.terrain_triangles, s.water_triangles)
    };
    if last.as_ref().is_some_and(|(seen, at)| live(seen) == live(&stats) || *frame - *at < 240) {
        return;
    }
    info!(
        "Ray tracing surfaces: terrain {} proxies / {} tris, water {} / {} tris, scatter {} entities ({} proxies), {} tiles / {} tris; {} TLAS instances, {:.1} MB of proxy meshes",
        stats.terrain_proxies,
        stats.terrain_triangles,
        stats.water_proxies,
        stats.water_triangles,
        stats.scatter_instances,
        stats.scatter_proxies,
        stats.tiles,
        stats.tile_triangles,
        stats.tlas_instances(),
        stats.mesh_bytes() as f64 / 1_048_576.0
    );
    *last = Some((stats.clone(), *frame));
}

// ============================================================================
// Entry
// ============================================================================

/// Add terrain, water and scatter to the traced scene through proxies marked
/// with `M` (Solari's `RaytracingMesh3d` in the real build). Called only
/// when ray tracing is active; the systems check too, so a host that adds
/// them unconditionally pays nothing with it off.
pub fn add_systems<M: TraceMarker>(app: &mut App) {
    app.init_resource::<SurfaceTraceBudget>()
        .init_resource::<SurfaceTraceStats>()
        .init_resource::<TerrainProxies>()
        .init_resource::<WaterProxies>()
        .init_resource::<ScatterProxies>()
        .add_systems(
            Last,
            (
                forget_gone_sources,
                // Start before finish: a build queued this frame replaces (and
                // so cancels) an older one before that one can land, instead
                // of the older landing and being rebuilt at once.
                (queue_terrain_sources, start_terrain_builds, finish_terrain_builds::<M>, refresh_terrain_looks).chain(),
                (queue_water_sources, start_water_builds, finish_water_builds::<M>).chain(),
                (index_scatter_batches, choose_scatter, trace_scatter::<M>).chain(),
                follow_sources,
                log_surface_stats,
            )
                .chain()
                .run_if(raytracing_active),
        );
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::super::{RaytracingLaunch, RaytracingStatus};
    use super::*;
    use bevy::asset::AssetPlugin;
    use eustress_common::terrain::{height_at_world, WaterConfig, WaterPlane, WaterSurfaceExtension};

    // ---- rig

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

    fn app_with(status: RaytracingStatus) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<WaterSurfaceMaterial>()
            .init_resource::<TerrainDirtyChunks>()
            .init_resource::<TerrainMaterialSlots>()
            .insert_resource(RaytracingLaunch::for_status(status));
        add_systems::<TestTraced>(&mut app);
        app
    }

    fn app() -> App {
        app_with(RaytracingStatus::Active)
    }

    /// Run frames until every queued and running build has landed (builds
    /// run on the async compute pool), then two more for their commands.
    fn settle(app: &mut App) {
        for _ in 0..4 {
            app.update();
        }
        for _ in 0..200_000 {
            let world = app.world();
            let busy = world.resource::<TerrainProxies>().busy()
                || world.resource::<WaterProxies>().busy()
                || world.resource::<ScatterProxies>().pending() > 0;
            if !busy {
                break;
            }
            std::thread::yield_now();
            app.update();
        }
        app.update();
        app.update();
    }

    fn camera(app: &mut App, at: Vec3) -> Entity {
        app.world_mut().spawn((Camera3d::default(), GlobalTransform::from_translation(at))).id()
    }

    fn stats(app: &App) -> SurfaceTraceStats {
        app.world().resource::<SurfaceTraceStats>().clone()
    }

    fn proxies(app: &App, source: Entity) -> Vec<Entity> {
        app.world().get::<TraceProxies>(source).map(|p| p.proxies().to_vec()).unwrap_or_default()
    }

    fn gone(app: &App, entities: &[Entity]) -> bool {
        entities.iter().all(|e| app.world().get_entity(*e).is_err())
    }

    /// Solari's own test (`is_mesh_raytracing_compatible` in its blas.rs).
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

    /// Every proxy is traced, never drawn, and a plain opaque lit material
    /// without textures; returns its mesh and material.
    fn traced_proxy(app: &App, proxy: Entity) -> (Mesh, StandardMaterial) {
        let world = app.world();
        assert!(world.get::<Mesh3d>(proxy).is_none(), "a proxy must never be drawn");
        let surface = world.get::<SurfaceProxy>(proxy).expect("a proxy");
        let marker = world.get::<TestTraced>(proxy).expect("a proxy carries the marker");
        assert_eq!(marker.0.id(), surface.mesh.id());
        let mesh = world.resource::<Assets<Mesh>>().get(&surface.mesh).expect("its mesh").clone();
        assert!(solari_would_trace(&mesh), "Solari would skip the proxy mesh");
        assert!(matches!(conform(&mesh), Conformance::AsIs { .. }));
        let material = world.get::<MeshMaterial3d<StandardMaterial>>(proxy).expect("a StandardMaterial");
        let material = world.resource::<Assets<StandardMaterial>>().get(&material.0).expect("its material").clone();
        assert!(matches!(material.alpha_mode, AlphaMode::Opaque) && !material.unlit);
        assert_eq!(material.emissive, LinearRgba::BLACK, "a proxy is never a light");
        assert!(material.base_color_texture.is_none() && material.normal_map_texture.is_none());
        assert!(material.metallic_roughness_texture.is_none() && material.emissive_texture.is_none());
        (mesh, material)
    }

    fn positions(mesh: &Mesh) -> Vec<Vec3> {
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(p)) => p.iter().map(|v| Vec3::from(*v)).collect(),
            _ => panic!("no positions"),
        }
    }

    fn triangles(mesh: &Mesh) -> Vec<[u32; 3]> {
        let Some(Indices::U32(i)) = mesh.indices() else { panic!("not U32") };
        i.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect()
    }

    /// World centroids of a proxy's triangles.
    fn world_centroids(app: &App, proxy: Entity, mesh: &Mesh) -> Vec<Vec3> {
        let at = *app.world().get::<GlobalTransform>(proxy).expect("placed");
        let p = positions(mesh);
        triangles(mesh)
            .iter()
            .map(|t| at.transform_point((p[t[0] as usize] + p[t[1] as usize] + p[t[2] as usize]) / 3.0))
            .collect()
    }

    fn rgb(material: &StandardMaterial) -> [f32; 3] {
        let c = material.base_color.to_linear();
        [c.red, c.green, c.blue]
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    /// A chunk-like grid `res` cells across `size` metres, positions local
    /// to the chunk corner, with the vertex colours real chunk meshes carry.
    fn grid_mesh(res: u32, size: f32) -> Mesh {
        let n = res + 1;
        let (mut p, mut uv) = (Vec::new(), Vec::new());
        for z in 0..n {
            for x in 0..n {
                p.push([x as f32 * size / res as f32, 0.0, z as f32 * size / res as f32]);
                uv.push([x as f32 / res as f32, z as f32 / res as f32]);
            }
        }
        let mut i = Vec::new();
        for z in 0..res {
            for x in 0..res {
                let a = z * n + x;
                i.extend([a, a + n, a + 1, a + 1, a + n, a + n + 1]);
            }
        }
        let count = p.len();
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0, 1.0, 1.0, 1.0]; count])
            .with_inserted_indices(Indices::U32(i))
    }

    /// 3 x 3 chunks of 16 m at 8 cells: a 24 x 24 raster over -16..32.
    fn config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 16.0,
            chunk_resolution: 8,
            chunks_x: 1,
            chunks_z: 1,
            height_scale: 100.0,
            height_offset: -50.0,
            ..default()
        }
    }

    /// The raster of `config`, heights and materials as functions of world XZ.
    fn raster(config: &TerrainConfig, height: impl Fn(Vec2) -> f32, slot: impl Fn(Vec2) -> u8) -> TerrainData {
        let w = (config.chunks_x * 2 + 1) * config.chunk_resolution;
        let h = (config.chunks_z * 2 + 1) * config.chunk_resolution;
        let (min, max) = config.footprint_xz();
        let (mut heights, mut cells) = (Vec::new(), Vec::new());
        for z in 0..h {
            for x in 0..w {
                let at = min + (max - min) * Vec2::new(x as f32 / (w - 1) as f32, z as f32 / (h - 1) as f32);
                heights.push((height(at) - config.height_offset) / config.height_scale);
                cells.push([slot(at), MATERIAL_SLOT_NONE, 0, 0]);
            }
        }
        TerrainData { height_cache: heights, cache_width: w, cache_height: h, material_cache: cells, ..default() }
    }

    fn root(app: &mut App, config: TerrainConfig, data: TerrainData) -> Entity {
        app.world_mut().spawn((TerrainRoot, config, data)).id()
    }

    fn chunk(app: &mut App, root: Entity, at: IVec2, mesh: Mesh) -> Entity {
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let material = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
        let corner = Vec3::new(at.x as f32 * 16.0, 0.0, at.y as f32 * 16.0);
        app.world_mut()
            .spawn((
                Chunk { position: at, lod: 0 },
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(corner),
                GlobalTransform::from_translation(corner),
                InheritedVisibility::VISIBLE,
                ChildOf(root),
            ))
            .id()
    }

    const ROCK: u8 = TerrainMaterial::Rock as u8;
    const SAND: u8 = TerrainMaterial::Sand as u8;

    // ---- the proxy mesh

    #[test]
    fn proxy_mesh_is_exactly_what_solari_traces_and_keeps_only_used_vertices() {
        let p = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [9.0, 9.0, 9.0], [1.0, 1.0, 1.0], [2.0, 1.0, 0.0]];
        let n = vec![[0.0, 1.0, 0.0], [0.0, 1.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.6, 0.8, 0.0]];
        let mesh = proxy_mesh(&p, &n, &[[0, 2, 1], [1, 2, 4], [1, 4, 5]]).expect("a mesh");
        assert!(solari_would_trace(&mesh));
        assert!(matches!(conform(&mesh), Conformance::AsIs { triangles: 3 }), "the core would copy it again");
        assert_eq!(mesh.count_vertices(), 5, "vertex 3 is unused and must not be kept");
        assert_eq!(triangles(&mesh), vec![[0, 1, 2], [2, 1, 3], [2, 3, 4]]);
        let Some(VertexAttributeValues::Float32x4(t)) = mesh.attribute(Mesh::ATTRIBUTE_TANGENT) else { panic!() };
        let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
        for (t, n) in t.iter().zip(normals) {
            let (t3, n3) = (Vec3::new(t[0], t[1], t[2]), Vec3::from(*n));
            assert!((t3.length() - 1.0).abs() < 1e-5 && t3.dot(n3).abs() < 1e-5 && (n3.length() - 1.0).abs() < 1e-5);
        }
        assert_eq!(normals[3], [0.0, 1.0, 0.0], "a zero normal becomes up");
        assert!(proxy_mesh(&p, &n, &[]).is_none());
        assert!(proxy_mesh(&p, &n, &[[0, 1, 7]]).is_none(), "an index past the end");
    }

    #[test]
    fn source_meshes_go_the_right_path() {
        // Chunks and built-in scatter carry vertex colours: the core skips
        // them, so the proxy is their only traced form (no double geometry).
        assert!(matches!(conform(&grid_mesh(2, 4.0)), Conformance::Skip(SkipReason::RasterOnlyAttribute)));
        assert_eq!(scatter_class(&grid_mesh(2, 4.0)), ScatterClass::Proxy);
        let mut plain = grid_mesh(2, 4.0);
        plain.remove_attribute(Mesh::ATTRIBUTE_COLOR);
        assert_eq!(scatter_class(&plain), ScatterClass::Core);
        let lines = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 2]);
        assert_eq!(scatter_class(&lines), ScatterClass::Untraced);
    }

    // ---- terrain

    #[test]
    fn a_chunk_is_split_by_the_slot_under_each_triangle() {
        let mut app = app();
        // Rock west of x = 24, sand east of it: the boundary runs through
        // chunk (1, 0), which spans x 16..32.
        let config = config();
        let data = raster(&config, |_| 0.0, |p| if p.x < 24.0 { ROCK } else { SAND });
        let root = root(&mut app, config, data);
        let c = chunk(&mut app, root, IVec2::new(1, 0), grid_mesh(8, 16.0));
        settle(&mut app);

        let parts = proxies(&app, c);
        assert_eq!(parts.len(), 2, "one proxy per slot");
        let state = app.world().resource::<TerrainProxies>();
        let slots = TerrainMaterialSlots::builtins();
        // A triangle takes its nearest raster cell, half a cell either side.
        let half_cell = 0.5 * 48.0 / 23.0 + 1e-3;
        let mut total = 0;
        for (proxy, slot) in state.parts_of(c) {
            assert!(parts.contains(&proxy));
            let (mesh, material) = traced_proxy(&app, proxy);
            assert_eq!(app.world().get::<SurfaceProxy>(proxy).unwrap().kind, ProxyKind::Terrain { slot });
            let worn = app.world().get::<MeshMaterial3d<StandardMaterial>>(proxy).map(|m| m.0.id());
            assert_eq!(worn, state.slot_material(slot, false).map(|h| h.id()));
            let look = terrain_slot_look(&slots, None, slot, false);
            assert!(close(rgb(&material), look.albedo), "slot {slot} draws its swatch");
            assert_eq!(material.perceptual_roughness, look.roughness);
            assert_eq!(material.reflectance, TERRAIN_REFLECTANCE);
            let centres = world_centroids(&app, proxy, &mesh);
            assert!(centres.len() >= 32, "each side holds a good share: {}", centres.len());
            for centre in &centres {
                match slot {
                    ROCK => assert!(centre.x < 24.0 + half_cell, "rock triangle at {centre}"),
                    SAND => assert!(centre.x > 24.0 - half_cell, "sand triangle at {centre}"),
                    other => panic!("unexpected slot {other}"),
                }
            }
            total += centres.len();
        }
        assert_eq!(total, 8 * 8 * 2, "every triangle is in exactly one part");
        assert_eq!(app.world().get::<GlobalTransform>(parts[0]), app.world().get::<GlobalTransform>(c));
    }

    #[test]
    fn brick_material_wins_over_the_map_and_bands_colour_procedural_ground() {
        let config = config();
        let data = raster(&config, |_| 0.0, |_| SAND);
        let ground = SlotGround::of(&config, &data);
        // Two triangles: the first is brick (basalt) at every vertex, the
        // second at one vertex only.
        let basalt = TerrainMaterial::Basalt as u8;
        let code = f32::from(basalt) + 1.0;
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.0, 0.0, 1.0]],
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 6])
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_UV_1,
                vec![[code, 1.0], [code, 1.0], [code, 1.0], [0.0, 0.0], [0.0, 0.0], [code, 1.0]],
            )
            .with_inserted_indices(Indices::U32(vec![0, 1, 2, 3, 4, 5]));
        let parts = split_terrain_mesh(&mesh, &GlobalTransform::IDENTITY, &ground, 6);
        let slots: Vec<(u8, u32)> = parts.iter().map(|p| (p.slot, p.triangles)).collect();
        // One triangle each, so the tie keeps the lower slot first.
        assert_eq!(slots, vec![(SAND, 1), (basalt, 1)], "a triangle one third brick reads the map");

        // Terrain without a material layer: the mesher's height bands.
        let procedural = TerrainData::default();
        let bands = SlotGround::of(&config, &procedural);
        assert_eq!(bands.slot_at(Vec3::new(0.0, 0.0, 0.0)), TerrainMaterial::Grass as u8);
        assert_eq!(bands.slot_at(Vec3::new(0.0, 35.0, 0.0)), TerrainMaterial::Rock as u8);
        assert_eq!(bands.slot_at(Vec3::new(0.0, 90.0, 0.0)), TerrainMaterial::Snow as u8);
        // A cell with no material reads as grass, as the shader draws it.
        let mut empty = data.clone();
        empty.material_cache.iter_mut().for_each(|c| *c = [MATERIAL_SLOT_NONE, MATERIAL_SLOT_NONE, 0, 0]);
        assert_eq!(SlotGround::of(&config, &empty).slot_at(Vec3::ZERO), TerrainMaterial::Grass as u8);
        // A two-material cell reads as its heavier material.
        let mut mixed = data.clone();
        mixed.material_cache.iter_mut().for_each(|c| *c = [ROCK, SAND, 200, 0]);
        assert_eq!(SlotGround::of(&config, &mixed).slot_at(Vec3::ZERO), SAND);
    }

    #[test]
    fn slots_past_the_part_limit_fold_into_the_commonest() {
        let config = config();
        // Stripes along x over chunk (0, 0): rock 0..8, sand 8..12, mud
        // 12..14, dirt 14..16.
        let mud = TerrainMaterial::Mud as u8;
        let dirt = TerrainMaterial::Dirt as u8;
        let data = raster(&config, |_| 0.0, |p| match p.x {
            x if x < 8.0 => ROCK,
            x if x < 12.0 => SAND,
            x if x < 14.0 => mud,
            _ => dirt,
        });
        let ground = SlotGround::of(&config, &data);
        let all = split_terrain_mesh(&grid_mesh(8, 16.0), &GlobalTransform::IDENTITY, &ground, 6);
        assert!(all.len() >= 3, "{} parts", all.len());
        let alone = |slot: u8| all.iter().find(|p| p.slot == slot).map_or(0, |p| p.triangles);
        let two = split_terrain_mesh(&grid_mesh(8, 16.0), &GlobalTransform::IDENTITY, &ground, 2);
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].slot, ROCK, "the commonest first");
        assert_eq!(two[0].triangles + two[1].triangles, 128, "no triangle is dropped");
        assert!(two[0].triangles > alone(ROCK), "the rare slots fold into the commonest");
        assert_eq!(two[1].triangles, alone(two[1].slot), "the second slot keeps only its own");
    }

    #[test]
    fn a_textured_slot_reads_as_its_tint_times_its_layer_mean() {
        let textured = TerrainSlotSurface { layer: Some(3), albedo: [0.5, 1.0, 0.8], tiling: 4.0, roughness: 0.01, roughness_scale: 2.0, metallic: 0.25 };
        let look = slot_look(&textured, Some([0.4, 0.2, 0.1]));
        assert!(close(look.albedo, [0.2, 0.2, 0.08]), "{:?}", look.albedo);
        assert_eq!(look.roughness, MIN_PERCEPTUAL_ROUGHNESS, "clamped to Bevy's floor");
        assert_eq!((look.metallic, look.reflectance), (0.25, TERRAIN_REFLECTANCE));
        let flat = TerrainSlotSurface { layer: None, roughness: 0.7, ..textured };
        let look = slot_look(&flat, Some([0.4, 0.2, 0.1]));
        assert!(close(look.albedo, [0.5, 1.0, 0.8]), "a flat slot is its colour");
        assert_eq!(look.roughness, 0.7);
        // Through the table: an untextured chunk draws every slot's swatch.
        let slots = TerrainMaterialSlots::builtins();
        let rock = slot_surface_in(&slots, None, ROCK);
        assert!(close(terrain_slot_look(&slots, None, ROCK, false).albedo, rock.albedo));
    }

    #[test]
    fn a_proxy_of_a_vanished_source_is_never_spawned() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = app();
        let dead = app.world_mut().spawn_empty().id();
        app.world_mut().despawn(dead);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(grid_mesh(1, 1.0));
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                let spec = ProxySpec { kind: ProxyKind::Water, mesh: mesh.clone(), material: Handle::default(), triangles: 2, vertices: 4 };
                spawn_proxy::<TestTraced>(&mut commands, dead, GlobalTransform::IDENTITY, spec);
            })
            .unwrap();
        assert_eq!(app.world_mut().query::<&TestTraced>().iter(app.world()).count(), 0, "a proxy of nothing would be traced forever");
        assert_eq!(app.world_mut().query::<&TraceProxyOf>().iter(app.world()).count(), 0);
    }

    #[test]
    fn chunk_proxies_follow_lod_material_swaps_slot_edits_hiding_and_despawn() {
        let mut app = app();
        let config = config();
        let data = raster(&config, |_| 0.0, |p| if p.x < 24.0 { ROCK } else { SAND });
        let root = root(&mut app, config, data);
        let c = chunk(&mut app, root, IVec2::new(1, 0), grid_mesh(8, 16.0));
        settle(&mut app);
        let first = proxies(&app, c);
        let builds = stats(&app).chunk_builds;
        assert_eq!(stats(&app).terrain_triangles, 128);

        // LOD: the chunk gets a coarser mesh.
        let coarse = app.world_mut().resource_mut::<Assets<Mesh>>().add(grid_mesh(4, 16.0));
        app.world_mut().entity_mut(c).insert(Mesh3d(coarse));
        settle(&mut app);
        let second = proxies(&app, c);
        assert!(gone(&app, &first), "old proxies outlived the LOD change");
        assert_eq!(stats(&app).terrain_triangles, 32, "the proxies follow the new LOD");
        assert_eq!(stats(&app).chunk_builds, builds + 1);
        assert_eq!(second.len(), 2);

        // The chunk moves onto the textured surface: same proxies, on the
        // textured slot materials.
        app.world_mut().entity_mut(c).remove::<MeshMaterial3d<StandardMaterial>>();
        app.world_mut().entity_mut(c).insert(MeshMaterial3d(Handle::<TerrainSurfaceMaterial>::default()));
        settle(&mut app);
        assert_eq!(proxies(&app, c), second, "a material swap must not rebuild");
        assert_eq!(stats(&app).chunk_builds, builds + 1);
        let state = app.world().resource::<TerrainProxies>();
        for (proxy, slot) in state.parts_of(c) {
            let worn = app.world().get::<MeshMaterial3d<StandardMaterial>>(proxy).unwrap().0.id();
            assert_eq!(Some(worn), state.slot_material(slot, true).map(|h| h.id()), "slot {slot} not on its textured look");
        }

        // A slot edit rewrites the shared material in place.
        let mut rock = app.world().resource::<TerrainMaterialSlots>().get(ROCK).unwrap().clone();
        rock.roughness = 0.33;
        rock.tint = [0.2, 0.4, 0.6];
        rock.texture_set = None;
        app.world_mut().resource_mut::<TerrainMaterialSlots>().set_slot(ROCK, Some(rock));
        settle(&mut app);
        let handle = app.world().resource::<TerrainProxies>().slot_material(ROCK, true).unwrap();
        let material = app.world().resource::<Assets<StandardMaterial>>().get(&handle).unwrap().clone();
        assert!((material.perceptual_roughness - 0.33).abs() < 1e-6, "the edit did not reach the material");
        assert!(close(rgb(&material), [0.2, 0.4, 0.6]));
        assert!(stats(&app).look_writes >= 1);
        assert_eq!(stats(&app).chunk_builds, builds + 1, "a slot edit must not rebuild");

        // Hidden: out of the traced scene, by despawning (Solari keeps a
        // render-world copy of a marker that is only removed).
        app.world_mut().entity_mut(c).insert(InheritedVisibility::HIDDEN);
        settle(&mut app);
        assert!(proxies(&app, c).is_empty(), "a hidden chunk is still traced");
        assert!(gone(&app, &second));
        assert_eq!(stats(&app).terrain_proxies, 0);
        app.world_mut().entity_mut(c).insert(InheritedVisibility::VISIBLE);
        settle(&mut app);
        assert_eq!(proxies(&app, c).len(), 2);

        // Moved (the root moved): the proxies follow without a rebuild.
        let builds = stats(&app).chunk_builds;
        let moved = GlobalTransform::from_translation(Vec3::new(16.0, 3.0, 0.0));
        app.world_mut().entity_mut(c).insert(moved);
        settle(&mut app);
        assert_eq!(stats(&app).chunk_builds, builds);
        for proxy in proxies(&app, c) {
            assert_eq!(app.world().get::<GlobalTransform>(proxy), Some(&moved), "a proxy stayed behind");
            assert_eq!(app.world().get::<Transform>(proxy).map(|t| t.translation), Some(moved.translation()));
        }

        // Despawned (culled): its proxies go with it.
        let last = proxies(&app, c);
        app.world_mut().entity_mut(c).despawn();
        settle(&mut app);
        assert!(gone(&app, &last), "proxies outlived their chunk");
        assert_eq!(stats(&app).terrain_proxies, 0);
        assert_eq!(stats(&app).terrain_triangles, 0);
    }

    #[test]
    fn the_triangle_budget_keeps_the_nearest_ground_and_frees_without_churn() {
        let mut app = app();
        app.world_mut().resource_mut::<SurfaceTraceBudget>().terrain_triangles = 300;
        let config = config();
        let data = raster(&config, |_| 0.0, |_| ROCK);
        let root = root(&mut app, config, data);
        let view = camera(&mut app, Vec3::new(-100.0, 10.0, 0.0));
        // 128 triangles each: two fit, the farthest is left out.
        let chunks: Vec<Entity> =
            [-1, 0, 1].iter().map(|x| chunk(&mut app, root, IVec2::new(*x, 0), grid_mesh(8, 16.0))).collect();
        settle(&mut app);
        let traced = |app: &App| chunks.iter().map(|c| !proxies(app, *c).is_empty()).collect::<Vec<bool>>();
        assert_eq!(traced(&app), vec![true, true, false]);
        assert!(app.world().resource::<TerrainProxies>().is_over_budget(chunks[2]));
        assert_eq!(stats(&app).terrain_triangles, 256);
        // A full budget costs nothing per frame.
        let builds = stats(&app).chunk_builds;
        settle(&mut app);
        assert_eq!(stats(&app).chunk_builds, builds, "a chunk left out was retried with no room for it");

        // The view crosses to the east and the eastern chunk changes LOD: it
        // is nearer than the western one, which makes room for it.
        app.world_mut().entity_mut(view).insert(GlobalTransform::from_translation(Vec3::new(100.0, 10.0, 0.0)));
        let fine = app.world_mut().resource_mut::<Assets<Mesh>>().add(grid_mesh(8, 16.0));
        app.world_mut().entity_mut(chunks[2]).insert(Mesh3d(fine));
        settle(&mut app);
        assert_eq!(traced(&app), vec![false, true, true], "the nearest ground keeps the budget");
        assert!(app.world().resource::<TerrainProxies>().is_over_budget(chunks[0]));

        // A traced chunk goes: the one left out comes back.
        app.world_mut().entity_mut(chunks[1]).despawn();
        settle(&mut app);
        assert!(!proxies(&app, chunks[0]).is_empty(), "freed room was not used");
        assert_eq!(stats(&app).terrain_triangles, 256);
    }

    #[test]
    fn a_build_overtaken_by_a_newer_mesh_or_hidden_lands_nothing_stale() {
        // Each case needs its build still running a frame after it started.
        // A big mesh makes that all but certain; a try that finds it already
        // landed (its thread finished during that one frame) is run again,
        // rather than passing without testing anything.
        let running = |app: &App| app.world().resource::<TerrainProxies>().in_flight() == 1;
        let fresh = || {
            let mut app = app();
            let config = config();
            let data = raster(&config, |_| 0.0, |_| ROCK);
            let root = root(&mut app, config, data);
            (app, root)
        };
        let mut tested = 0;
        for _ in 0..5 {
            let (mut app, root) = fresh();
            let c = chunk(&mut app, root, IVec2::new(0, 0), grid_mesh(128, 16.0));
            app.update();
            if !running(&app) {
                continue;
            }
            // A LOD change while it runs: only the newer mesh may land.
            let coarse = app.world_mut().resource_mut::<Assets<Mesh>>().add(grid_mesh(4, 16.0));
            app.world_mut().entity_mut(c).insert(Mesh3d(coarse));
            settle(&mut app);
            assert_eq!(stats(&app).terrain_triangles, 32, "the older LOD's build landed");
            tested += 1;
            break;
        }
        for _ in 0..5 {
            let (mut app, root) = fresh();
            let c = chunk(&mut app, root, IVec2::new(0, 0), grid_mesh(128, 16.0));
            app.update();
            if !running(&app) {
                continue;
            }
            // Hidden while a build runs: nothing lands.
            app.world_mut().entity_mut(c).insert(InheritedVisibility::HIDDEN);
            settle(&mut app);
            assert!(proxies(&app, c).is_empty(), "a build landed for a hidden chunk");
            assert_eq!(stats(&app).terrain_triangles, 0);
            tested += 1;
            break;
        }
        assert_eq!(tested, 2, "never caught a build running");
    }

    #[test]
    fn chunk_builds_are_rate_limited_nearest_first() {
        let mut app = app();
        app.world_mut().resource_mut::<SurfaceTraceBudget>().chunk_builds_per_frame = 2;
        let config = config();
        let data = raster(&config, |_| 0.0, |_| ROCK);
        let root = root(&mut app, config, data);
        camera(&mut app, Vec3::new(-100.0, 10.0, 0.0));
        let chunks: Vec<Entity> =
            [1, -1, 0].iter().map(|x| chunk(&mut app, root, IVec2::new(*x, 0), grid_mesh(4, 16.0))).collect();
        app.update();
        let state = app.world().resource::<TerrainProxies>();
        let queued: Vec<bool> = chunks.iter().map(|c| state.is_queued(*c)).collect();
        assert_eq!(queued, vec![true, false, false], "the two nearest the view start first");
        let landed = chunks.iter().filter(|c| !proxies(&app, **c).is_empty()).count();
        assert_eq!(state.in_flight() + landed, 2, "no more than two run at once");
        settle(&mut app);
        assert!(chunks.iter().all(|c| !proxies(&app, *c).is_empty()));
    }

    // ---- water

    fn default_water_params() -> WaterSurfaceParams {
        WaterSurfaceParams::new(&WaterConfig::default(), None)
    }

    #[test]
    fn water_is_traced_where_the_raster_draws_it_opaque() {
        let params = default_water_params();
        // Default water: deep alpha 0.9, shallow 0.27, deep at 6 m. 0.8 is
        // reached where smoothstep(0, 6, d) = (0.8 - 0.27) / 0.63, d = 4.487.
        let depth = traced_water_depth(&params, 0.8).expect("default water is opaque enough");
        assert!((depth - 4.487).abs() < 0.02, "traced from {depth} m");
        assert!(water_opacity(&params, depth) >= 0.8 - 1e-5);
        assert!(water_opacity(&params, depth - 0.05) < 0.8);
        let clear = WaterSurfaceParams::new(&WaterConfig { opacity: 0.5, ..default() }, None);
        assert_eq!(traced_water_depth(&clear, 0.8), None, "clear water must leave its bed lit");
        assert!(traced_water_depth(&params, 0.95).is_none());
    }

    /// The water's StandardMaterial base, as `water_base_material` in
    /// eustress_common makes it (crate-private there).
    fn water_base() -> StandardMaterial {
        StandardMaterial {
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.06,
            reflectance: 0.35,
            double_sided: true,
            cull_mode: None,
            ..default()
        }
    }

    fn ocean(app: &mut App, params: WaterSurfaceParams, level: f32) -> Entity {
        // Over the footprint (-16..32), as the ocean plane is laid.
        let mut mesh: Mesh = Plane3d::default().mesh().size(48.0, 48.0).into();
        let n = mesh.count_vertices();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0f32, 0.0]; n]);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        let extension = WaterSurfaceExtension { terrain_height: None, params };
        let material =
            app.world_mut().resource_mut::<Assets<WaterSurfaceMaterial>>().add(WaterSurfaceMaterial { base: water_base(), extension });
        let at = Vec3::new(8.0, level, 8.0);
        app.world_mut()
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(at),
                GlobalTransform::from_translation(at),
                InheritedVisibility::VISIBLE,
                WaterPlane,
                WaterSurface,
                NotShadowCaster,
            ))
            .id()
    }

    /// Ground falling 1 m per metre east: world height = -x.
    fn sloped(config: &TerrainConfig) -> TerrainData {
        raster(config, |p| -p.x, |_| SAND)
    }

    #[test]
    fn ocean_proxy_keeps_only_deep_water_in_the_water_look() {
        let mut app = app();
        let config = config();
        let data = sloped(&config);
        root(&mut app, config.clone(), data.clone());
        let params = default_water_params();
        let water = ocean(&mut app, params, 0.0);
        settle(&mut app);

        let proxy = app.world().resource::<WaterProxies>().proxy_of(water).expect("deep water is traced");
        assert_eq!(proxies(&app, water), vec![proxy]);
        let (mesh, material) = traced_proxy(&app, proxy);
        // The look: deep colour, the water's roughness and F0 0.02.
        assert!(close(rgb(&material), [params.deep_color.x, params.deep_color.y, params.deep_color.z]));
        assert!((material.perceptual_roughness - 0.089).abs() < 1e-6, "0.06 clamped to Bevy's floor");
        let f0 = 0.16 * material.reflectance * material.reflectance;
        assert!((f0 - 0.02).abs() < 0.001, "F0 {f0}");
        assert_eq!(material.metallic, 0.0);
        // Every kept corner is at least the traced depth: depth = 0 - (-x) = x.
        let min_depth = traced_water_depth(&params, 0.8).unwrap();
        let at = *app.world().get::<GlobalTransform>(proxy).unwrap();
        let corners: Vec<Vec3> = positions(&mesh).iter().map(|p| at.transform_point(*p)).collect();
        for corner in &corners {
            let depth = corner.y - height_at_world(&config, &data, corner.x, corner.z);
            assert!(depth >= min_depth - 1e-3, "shallow corner {corner} ({depth} m)");
        }
        // And it covers the deep water, x from about 4.5 to 32 and z over
        // -16..32, less at most one 8 m cell along the line.
        let area: f32 = triangles(&mesh)
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| corners[i as usize]);
                (b - a).cross(c - a).length() * 0.5
            })
            .sum();
        let deep = (32.0 - min_depth) * 48.0;
        assert!(area > deep - 8.0 * 48.0 && area <= deep + 1.0, "proxy area {area} against deep water {deep}");
        assert_eq!(stats(&app).water_proxies, 1);
    }

    #[test]
    fn clear_or_hidden_water_is_not_traced() {
        let mut app = app();
        let config = config();
        root(&mut app, config.clone(), sloped(&config));
        let clear = ocean(&mut app, WaterSurfaceParams::new(&WaterConfig { opacity: 0.5, ..default() }, None), 0.0);
        let deep = ocean(&mut app, default_water_params(), 0.0);
        settle(&mut app);
        assert!(proxies(&app, clear).is_empty(), "clear water traced");
        assert_eq!(proxies(&app, deep).len(), 1);
        app.world_mut().entity_mut(deep).insert(InheritedVisibility::HIDDEN);
        settle(&mut app);
        assert!(proxies(&app, deep).is_empty(), "hidden water traced");
        assert_eq!(stats(&app).water_proxies, 0);
    }

    #[test]
    fn water_is_measured_again_once_the_ground_goes_quiet() {
        let mut app = app();
        app.world_mut().resource_mut::<SurfaceTraceBudget>().water_quiet_frames = 5;
        let config = config();
        let root = root(&mut app, config.clone(), sloped(&config));
        ocean(&mut app, default_water_params(), 0.0);
        settle(&mut app);
        let builds = stats(&app).water_builds;
        let before = stats(&app).water_triangles;
        // Raise the whole floor by 10 m while a stroke marks it frame after
        // frame: no rebuild until it stops.
        app.world_mut().entity_mut(root).insert(raster(&config, |p| 10.0 - p.x, |_| SAND));
        for _ in 0..8 {
            app.world_mut().resource_mut::<TerrainDirtyChunks>().mark(IVec2::ZERO);
            app.update();
        }
        assert_eq!(stats(&app).water_builds, builds, "water rebuilt mid-stroke");
        assert_eq!(app.world().resource::<WaterProxies>().in_flight(), 0, "a build started mid-stroke");
        settle(&mut app);
        assert_eq!(stats(&app).water_builds, builds + 1, "water not measured once the ground went quiet");
        assert!(stats(&app).water_triangles < before, "less water is deep over a higher floor");
    }

    #[test]
    fn a_water_build_running_when_the_ground_settles_is_measured_again() {
        // A fine cut, so the first build is still running two frames on.
        let slow = |app: &mut App| {
            let mut budget = app.world_mut().resource_mut::<SurfaceTraceBudget>();
            budget.water_quiet_frames = 1;
            budget.water_cell = 0.05;
        };
        let config = config();
        // What the raised floor measures when it is there from the start.
        let mut fresh = app();
        slow(&mut fresh);
        root(&mut fresh, config.clone(), raster(&config, |p| 10.0 - p.x, |_| SAND));
        ocean(&mut fresh, default_water_params(), 0.0);
        settle(&mut fresh);
        let expected = stats(&fresh).water_triangles;

        assert_ne!(expected, 0);
        // The first build must still be running a frame on; a try that finds
        // it already landed is run again (see the overtaken-build test).
        let mut tested = false;
        for _ in 0..5 {
            let mut app = app();
            slow(&mut app);
            let root = root(&mut app, config.clone(), sloped(&config));
            ocean(&mut app, default_water_params(), 0.0);
            app.update();
            if app.world().resource::<WaterProxies>().in_flight() != 1 {
                continue;
            }
            app.world_mut().entity_mut(root).insert(raster(&config, |p| 10.0 - p.x, |_| SAND));
            app.world_mut().resource_mut::<TerrainDirtyChunks>().mark(IVec2::ZERO);
            settle(&mut app);
            assert_eq!(stats(&app).water_triangles, expected, "a build of the old ground landed and stayed");
            tested = true;
            break;
        }
        assert!(tested, "never caught a build running");
    }

    #[test]
    fn a_water_material_edit_reaches_its_proxy() {
        let mut app = app();
        let config = config();
        root(&mut app, config.clone(), sloped(&config));
        let water = ocean(&mut app, default_water_params(), 0.0);
        settle(&mut app);
        assert_eq!(proxies(&app, water).len(), 1);
        // The water made clear (Terrain properties): its bed shows, so it
        // leaves the traced scene.
        let handle = app.world().get::<MeshMaterial3d<WaterSurfaceMaterial>>(water).unwrap().0.clone();
        let clear = WaterSurfaceParams::new(&WaterConfig { opacity: 0.5, ..default() }, None);
        app.world_mut().resource_mut::<Assets<WaterSurfaceMaterial>>().get_mut(&handle).unwrap().extension.params = clear;
        settle(&mut app);
        assert!(proxies(&app, water).is_empty(), "water made clear is still traced");
    }

    #[test]
    fn a_moved_water_surface_is_rebuilt_at_its_new_level() {
        let mut app = app();
        let config = config();
        root(&mut app, config.clone(), sloped(&config));
        let water = ocean(&mut app, default_water_params(), 0.0);
        settle(&mut app);
        let low = stats(&app).water_triangles;
        // Sea level up 5 m: more of it is deep.
        app.world_mut().entity_mut(water).insert(GlobalTransform::from_translation(Vec3::new(8.0, 5.0, 8.0)));
        settle(&mut app);
        assert!(stats(&app).water_triangles > low);
        let proxy = proxies(&app, water)[0];
        assert_eq!(app.world().get::<GlobalTransform>(proxy).unwrap().translation().y, 5.0);
    }

    // ---- scatter

    /// A tree: a brown trunk (one quad) and a green crown (four triangles in
    /// close shades), vertex coloured like the built-in meshes.
    fn tree_mesh() -> Mesh {
        let brown = [0.30, 0.18, 0.08, 1.0];
        let greens = [[0.10, 0.40, 0.08, 1.0], [0.11, 0.42, 0.08, 1.0], [0.09, 0.39, 0.09, 1.0], [0.10, 0.41, 0.07, 1.0]];
        let mut p = vec![[-0.2, 0.0, 0.0], [0.2, 0.0, 0.0], [0.2, 2.0, 0.0], [-0.2, 2.0, 0.0]];
        let mut c = vec![brown; 4];
        let mut i = vec![0u32, 1, 2, 0, 2, 3];
        for (k, green) in greens.iter().enumerate() {
            let a = std::f32::consts::TAU * k as f32 / 4.0;
            let b = a + std::f32::consts::TAU / 4.0;
            let base = p.len() as u32;
            p.extend([[a.cos() * 1.5, 2.0, a.sin() * 1.5], [b.cos() * 1.5, 2.0, b.sin() * 1.5], [0.0, 5.0, 0.0]]);
            c.extend([*green; 3]);
            i.extend([base, base + 1, base + 2]);
        }
        let n = p.len();
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, c)
            .with_inserted_indices(Indices::U32(i))
    }

    /// The scatter's shared Solid material.
    fn solid(app: &mut App) -> Handle<StandardMaterial> {
        let material = StandardMaterial { perceptual_roughness: 0.95, reflectance: 0.18, ..default() };
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(material)
    }

    /// A batch at the origin with an entity of `mesh` at each of `at`.
    fn batch(app: &mut App, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: &[Vec3]) -> (Entity, Vec<Entity>) {
        let batch = app
            .world_mut()
            .spawn((
                ScatterBatch { layer: 1, chunk: IVec2::ZERO, tile: IVec2::ZERO },
                Transform::default(),
                GlobalTransform::IDENTITY,
                InheritedVisibility::VISIBLE,
            ))
            .id();
        let children = at
            .iter()
            .map(|p| {
                app.world_mut()
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_translation(*p),
                        GlobalTransform::from_translation(*p),
                        InheritedVisibility::VISIBLE,
                        ChildOf(batch),
                    ))
                    .id()
            })
            .collect();
        (batch, children)
    }

    #[test]
    fn colour_groups_split_bark_from_crown() {
        let mesh = tree_mesh();
        let source = read_source(&mesh).unwrap();
        let groups = group_by_color(source.positions, source.colors, &source.triangles, 2);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].triangles.len(), 4, "the crown is heavier and comes first");
        assert!(groups[0].color[1] > 0.38 && groups[0].color[0] < 0.12, "crown {:?}", groups[0].color);
        assert!(close(groups[1].color, [0.30, 0.18, 0.08]), "trunk {:?}", groups[1].color);
        assert_eq!(group_by_color(source.positions, source.colors, &source.triangles, 1).len(), 1);
        // Shades of one colour stay one group whatever the limit.
        let crown: Vec<[u32; 3]> = groups[0].triangles.clone();
        assert_eq!(group_by_color(source.positions, source.colors, &crown, 3).len(), 1);
        assert_eq!(group_by_color(source.positions, None, &crown, 3)[0].color, [1.0; 3]);
    }

    #[test]
    fn scatter_entities_share_prototype_proxies_within_the_budget() {
        let mut app = app();
        {
            let mut budget = app.world_mut().resource_mut::<SurfaceTraceBudget>();
            budget.scatter_instances = 2;
            budget.scatter_radius = 100.0;
        }
        camera(&mut app, Vec3::ZERO);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(tree_mesh());
        let material = solid(&mut app);
        let at = [Vec3::new(30.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0), Vec3::new(150.0, 0.0, 0.0)];
        let (_, trees) = batch(&mut app, &mesh, &material, &at);
        settle(&mut app);

        let traced: Vec<bool> = trees.iter().map(|t| app.world().resource::<ScatterProxies>().is_traced(*t)).collect();
        assert_eq!(traced, vec![false, true, true, false], "the nearest two within the radius");
        let (a, b) = (proxies(&app, trees[1]), proxies(&app, trees[2]));
        assert_eq!((a.len(), b.len()), (2, 2), "one proxy per colour group");
        let part = |p: Entity| {
            let w = app.world();
            (w.get::<SurfaceProxy>(p).unwrap().mesh.id(), w.get::<MeshMaterial3d<StandardMaterial>>(p).unwrap().0.id())
        };
        let mut shared_a: Vec<_> = a.iter().map(|p| part(*p)).collect();
        let mut shared_b: Vec<_> = b.iter().map(|p| part(*p)).collect();
        shared_a.sort();
        shared_b.sort();
        assert_eq!(shared_a, shared_b, "entities of one prototype share its meshes and materials");
        let mut colours = Vec::new();
        for p in &a {
            let (_, material) = traced_proxy(&app, *p);
            assert_eq!(material.perceptual_roughness, 0.95);
            assert_eq!(material.reflectance, 0.18);
            colours.push(rgb(&material));
        }
        assert!(colours.iter().any(|c| close(*c, [0.30, 0.18, 0.08])), "no bark proxy: {colours:?}");
        assert!(colours.iter().any(|c| c[1] > 0.38), "no crown proxy: {colours:?}");
        assert_eq!(app.world().get::<GlobalTransform>(a[0]), app.world().get::<GlobalTransform>(trees[1]));
        let s = stats(&app);
        assert_eq!((s.scatter_instances, s.scatter_proxies), (2, 4));
        assert_eq!(s.prototype_triangles, 6, "the prototype is counted once");
    }

    #[test]
    fn traced_scatter_follows_the_view_in_steps() {
        let mut app = app();
        {
            let mut budget = app.world_mut().resource_mut::<SurfaceTraceBudget>();
            budget.scatter_radius = 50.0;
            budget.scatter_reselect_step = 8.0;
        }
        let view = camera(&mut app, Vec3::ZERO);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(tree_mesh());
        let material = solid(&mut app);
        let (_, trees) = batch(&mut app, &mesh, &material, &[Vec3::new(10.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 0.0)]);
        settle(&mut app);
        let passes = stats(&app).scatter_passes;
        assert!(app.world().resource::<ScatterProxies>().is_traced(trees[0]));
        let near = proxies(&app, trees[0]);

        // A small move chooses nothing again.
        app.world_mut().entity_mut(view).insert(GlobalTransform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
        settle(&mut app);
        assert_eq!(stats(&app).scatter_passes, passes, "chose again for a 3 m move");

        // Across to the far tree: the near one leaves, the far one joins.
        app.world_mut().entity_mut(view).insert(GlobalTransform::from_translation(Vec3::new(95.0, 0.0, 0.0)));
        settle(&mut app);
        assert_eq!(stats(&app).scatter_passes, passes + 1);
        let scatter = app.world().resource::<ScatterProxies>();
        assert!(!scatter.is_traced(trees[0]) && scatter.is_traced(trees[1]));
        assert!(gone(&app, &near), "proxies of a tree out of range remain");
        assert_eq!(proxies(&app, trees[1]).len(), 2);
    }

    #[test]
    fn scatter_spawns_are_rate_limited_nearest_first() {
        let mut app = app();
        app.world_mut().resource_mut::<SurfaceTraceBudget>().scatter_spawns_per_frame = 2;
        camera(&mut app, Vec3::ZERO);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(tree_mesh());
        let material = solid(&mut app);
        let at: Vec<Vec3> = [40.0, 10.0, 30.0, 20.0].iter().map(|x| Vec3::new(*x, 0.0, 0.0)).collect();
        let (_, trees) = batch(&mut app, &mesh, &material, &at);
        app.update();
        let traced: Vec<bool> = trees.iter().map(|t| app.world().resource::<ScatterProxies>().is_traced(*t)).collect();
        assert_eq!(traced, vec![false, true, false, true]);
        app.update();
        assert_eq!(app.world().resource::<ScatterProxies>().traced_count(), 4);
    }

    #[test]
    fn merged_tiles_are_traced_as_the_raster_shadows_them() {
        let mut app = app();
        {
            let mut budget = app.world_mut().resource_mut::<SurfaceTraceBudget>();
            budget.tile_radius = 40.0;
            budget.tile_triangles = 100;
        }
        camera(&mut app, Vec3::ZERO);
        let material = solid(&mut app);
        let tile = |app: &mut App, mesh: Mesh, at: Vec3, grass: bool| {
            let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
            let mut e = app.world_mut().spawn((
                ScatterBatch { layer: 2, chunk: IVec2::ZERO, tile: IVec2::ZERO },
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(at),
                GlobalTransform::from_translation(at),
                InheritedVisibility::VISIBLE,
            ));
            if grass {
                e.insert(NotShadowCaster);
            }
            e.id()
        };
        let shrubs = tile(&mut app, grid_mesh(4, 8.0), Vec3::new(5.0, 0.0, 5.0), false);
        let grass = tile(&mut app, grid_mesh(4, 8.0), Vec3::new(5.0, 0.0, -15.0), true);
        let far = tile(&mut app, grid_mesh(4, 8.0), Vec3::new(200.0, 0.0, 0.0), false);
        let dense = tile(&mut app, grid_mesh(8, 8.0), Vec3::new(-15.0, 0.0, 5.0), false);
        settle(&mut app);
        let scatter = app.world().resource::<ScatterProxies>();
        assert!(scatter.is_tile_traced(shrubs), "a shrub tile in range is not traced");
        assert!(!scatter.is_tile_traced(grass), "grass casts no raster shadow and must not be traced");
        assert!(!scatter.is_tile_traced(far), "a tile past the radius is traced");
        assert!(!scatter.is_tile_traced(dense), "a tile over the triangle cap is traced");
        assert!(proxies(&app, grass).is_empty() && proxies(&app, far).is_empty() && proxies(&app, dense).is_empty());
        let parts = proxies(&app, shrubs);
        assert_eq!(parts.len(), 1, "small things: one group");
        let (mesh, _) = traced_proxy(&app, parts[0]);
        assert_eq!(triangles(&mesh).len(), 32);
        assert_eq!(stats(&app).tile_triangles, 32);
    }

    #[test]
    fn core_path_scatter_is_held_out_until_the_budget_takes_it() {
        let mut app = app();
        app.world_mut().resource_mut::<SurfaceTraceBudget>().scatter_radius = 50.0;
        let view = camera(&mut app, Vec3::ZERO);
        // A Custom layer's asset the core traces as it is: no vertex colours.
        let mut plain = grid_mesh(2, 4.0);
        plain.remove_attribute(Mesh::ATTRIBUTE_COLOR);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(plain);
        let material = solid(&mut app);
        let (_, items) = batch(&mut app, &mesh, &material, &[Vec3::new(10.0, 0.0, 0.0), Vec3::new(80.0, 0.0, 0.0)]);
        settle(&mut app);
        assert_eq!(app.world().get::<TraceOverride>(items[0]), None, "in range: left to the core");
        assert_eq!(app.world().get::<TraceOverride>(items[1]), Some(&TraceOverride::Never), "out of range: held out");
        assert!(proxies(&app, items[0]).is_empty(), "the core path gets no proxy");
        app.world_mut().entity_mut(view).insert(GlobalTransform::from_translation(Vec3::new(80.0, 0.0, 0.0)));
        settle(&mut app);
        assert_eq!(app.world().get::<TraceOverride>(items[0]), Some(&TraceOverride::Never));
        assert_eq!(app.world().get::<TraceOverride>(items[1]), None);
        assert!(app.world().resource::<ScatterProxies>().is_held(items[0]));
    }

    #[test]
    fn a_scatter_material_edit_retraces_only_when_the_look_changes() {
        let mut app = app();
        camera(&mut app, Vec3::ZERO);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(tree_mesh());
        let material = solid(&mut app);
        let (_, trees) = batch(&mut app, &mesh, &material, &[Vec3::new(10.0, 0.0, 0.0)]);
        settle(&mut app);
        let first = proxies(&app, trees[0]);
        let passes = stats(&app).scatter_passes;
        // Something only the raster reads: nothing is rebuilt.
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&material).unwrap().cull_mode = None;
        settle(&mut app);
        assert_eq!(proxies(&app, trees[0]), first, "an edit no proxy reads retraced the tree");
        assert_eq!(stats(&app).scatter_passes, passes);
        // A darker tint: the proxies are rebuilt in it.
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().get_mut(&material).unwrap().base_color =
            Color::linear_rgb(0.5, 0.5, 0.5);
        settle(&mut app);
        let second = proxies(&app, trees[0]);
        assert!(gone(&app, &first), "the old proxies remain after a look change");
        let colours: Vec<[f32; 3]> = second.iter().map(|p| rgb(&traced_proxy(&app, *p).1)).collect();
        assert!(colours.iter().any(|c| close(*c, [0.15, 0.09, 0.04])), "bark not in the new tint: {colours:?}");
        assert_eq!(stats(&app).prototype_triangles, 6, "the old prototype is still counted");
    }

    #[test]
    fn a_prototype_whose_material_arrives_late_is_traced_then() {
        let mut app = app();
        camera(&mut app, Vec3::ZERO);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(tree_mesh());
        // A material still loading: reserved, not there yet.
        let material = app.world().resource::<Assets<StandardMaterial>>().reserve_handle();
        let (_, trees) = batch(&mut app, &mesh, &material, &[Vec3::new(10.0, 0.0, 0.0)]);
        settle(&mut app);
        assert!(!app.world().resource::<ScatterProxies>().is_traced(trees[0]));
        let passes = stats(&app).scatter_passes;
        settle(&mut app);
        assert_eq!(stats(&app).scatter_passes, passes, "an untraceable prototype is retried every frame");
        let solid = StandardMaterial { perceptual_roughness: 0.95, reflectance: 0.18, ..default() };
        app.world_mut().resource_mut::<Assets<StandardMaterial>>().insert(material.id(), solid).unwrap();
        settle(&mut app);
        assert!(app.world().resource::<ScatterProxies>().is_traced(trees[0]), "not traced once its material loaded");
        assert_eq!(proxies(&app, trees[0]).len(), 2);
    }

    #[test]
    fn a_dropped_batch_takes_its_proxies_and_books() {
        let mut app = app();
        camera(&mut app, Vec3::ZERO);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(tree_mesh());
        let material = solid(&mut app);
        let (b, trees) = batch(&mut app, &mesh, &material, &[Vec3::new(10.0, 0.0, 0.0)]);
        settle(&mut app);
        let parts = proxies(&app, trees[0]);
        assert_eq!(parts.len(), 2);
        app.world_mut().entity_mut(b).despawn();
        settle(&mut app);
        assert!(gone(&app, &parts));
        let s = stats(&app);
        assert_eq!((s.scatter_instances, s.scatter_proxies), (0, 0));
    }

    /// What one build costs on this machine, printed for the report and for
    /// tuning the per-frame budgets; asserts nothing about time. Run with
    /// `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn measure_build_costs() {
        let time = |label: &str, runs: u32, mut f: Box<dyn FnMut() -> usize + '_>| {
            let started = std::time::Instant::now();
            let mut out = 0;
            for _ in 0..runs {
                out = f();
            }
            let each = started.elapsed() / runs;
            println!("{label}: {each:?} each ({out} out)");
        };
        // A LOD-0 chunk at 128 cells (32,768 triangles) over a 384 x 384
        // raster striped with four materials.
        let config = TerrainConfig { chunk_resolution: 128, chunk_size: 400.0, ..config() };
        let data = raster(&config, |p| (p.x * 0.01).sin() * 20.0, |p| [ROCK, SAND, 0, 2][((p.x + p.y).abs() as usize / 37) % 4]);
        let ground = SlotGround::of(&config, &data);
        let chunk = grid_mesh(128, 400.0);
        time("terrain chunk 128 x 128 (off the main thread)", 5, Box::new(|| {
            split_terrain_mesh(&chunk, &GlobalTransform::IDENTITY, &ground, 6).iter().map(|p| p.triangles as usize).sum()
        }));
        // What stays on the main thread: the copies a build starts from.
        time("terrain chunk copies (main thread)", 20, Box::new(|| {
            let area = local_bounds(&chunk, None).map(|(min, max)| world_xz_bounds(min, max, &GlobalTransform::IDENTITY));
            let copy = chunk.clone();
            let ground = SlotGround::capture(&config, &data, area);
            copy.count_vertices() + matches!(ground, SlotGround::MaterialMap { .. }) as usize
        }));
        time("ocean ground copy, 384 x 384 raster (main thread)", 20, Box::new(|| {
            GroundHeights::capture(&config, &data, None).map_or(0, |g| g.heights.len())
        }));
        // The ocean over a 10 km terrain: one quad cut to 256 a side.
        let mut ocean: Mesh = Plane3d::default().mesh().size(10_000.0, 10_000.0).into();
        ocean.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0f32, 0.0]; 4]);
        let depth = |p: Vec3| 10.0 - (p.x * 0.001).sin() * 20.0;
        time("ocean 10 km, 8 m cell, 256 cap", 3, Box::new(|| {
            deep_water_mesh(&ocean, &GlobalTransform::IDENTITY, &depth, 4.5, 8.0, 256, 131_072).map_or(0, |m| mesh_counts(&m).0 as usize)
        }));
        // A dense shrub tile: 25,600 triangles of two colours.
        let mut tile = grid_mesh(113, 32.0);
        let n = tile.count_vertices();
        tile.insert_attribute(Mesh::ATTRIBUTE_COLOR, (0..n).map(|i| if i % 7 == 0 { [0.3, 0.2, 0.1, 1.0] } else { [0.1, 0.4, 0.1, 1.0] }).collect::<Vec<_>>());
        let source = read_source(&tile).unwrap();
        time("merged tile 25.5k triangles, 1 group", 3, Box::new(|| {
            group_by_color(source.positions, source.colors, &source.triangles, 1).iter().map(|g| proxy_mesh(source.positions, source.normals, &g.triangles).map_or(0, |m| mesh_counts(&m).0 as usize)).sum()
        }));
    }

    // ---- everything together

    fn scene(app: &mut App) -> (Entity, Entity, Vec<Entity>) {
        camera(app, Vec3::new(0.0, 10.0, 0.0));
        let config = config();
        let data = raster(&config, |p| -p.x, |p| if p.x < 24.0 { ROCK } else { SAND });
        let r = root(app, config, data);
        let c = chunk(app, r, IVec2::new(1, 0), grid_mesh(8, 16.0));
        let w = ocean(app, default_water_params(), 0.0);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(tree_mesh());
        let material = solid(app);
        let (_, trees) = batch(app, &mesh, &material, &[Vec3::new(10.0, 0.0, 0.0), Vec3::new(20.0, 0.0, 5.0)]);
        (c, w, trees)
    }

    #[test]
    fn nothing_is_traced_while_ray_tracing_is_off() {
        let mut app = app_with(RaytracingStatus::Off);
        let (c, w, trees) = scene(&mut app);
        settle(&mut app);
        assert!(proxies(&app, c).is_empty() && proxies(&app, w).is_empty() && proxies(&app, trees[0]).is_empty());
        assert_eq!(app.world_mut().query::<&SurfaceProxy>().iter(app.world()).count(), 0);
    }

    #[test]
    fn steady_state_does_no_work() {
        let mut app = app();
        let (c, w, trees) = scene(&mut app);
        settle(&mut app);
        assert!(!proxies(&app, c).is_empty() && !proxies(&app, w).is_empty() && !proxies(&app, trees[0]).is_empty());
        let before = stats(&app);
        let all: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<SurfaceProxy>>().iter(app.world()).collect();
        for _ in 0..30 {
            app.update();
        }
        let after = stats(&app);
        assert_eq!(after, before, "an idle scene did proxy work");
        let terrain = app.world().resource::<TerrainProxies>().pending();
        let water = app.world().resource::<WaterProxies>().pending();
        let scatter = app.world().resource::<ScatterProxies>().pending();
        assert_eq!((terrain, water, scatter), (0, 0, 0));
        let still: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<SurfaceProxy>>().iter(app.world()).collect();
        assert_eq!(all, still, "proxies were respawned in an idle scene");
        assert_eq!(after.tlas_instances(), still.len());
        assert!(after.mesh_bytes() > 0);
        // Every proxy, of every kind, is traced and never drawn.
        for proxy in still {
            traced_proxy(&app, proxy);
        }
    }
}

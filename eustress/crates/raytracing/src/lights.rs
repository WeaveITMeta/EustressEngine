//! Point, spot and surface lights under ray tracing.
//!
//! A camera with Solari lighting skips Bevy's deferred lighting pass, and
//! Solari 0.19 traces two kinds of light only: `DirectionalLight`s and meshes
//! whose `StandardMaterial` is emissive (each one an area light). Every
//! `PointLight` and `SpotLight`, and the `SpotLight` a SurfaceLight becomes,
//! would light nothing in a ray-traced view. This module keeps one emissive
//! stand-in, a proxy, for each lit light, which Solari traces as an area
//! light:
//!
//! | Bevy light | Proxy | Luminance |
//! |---|---|---|
//! | `PointLight` | sphere at the light, radius max(light radius, 0.1 m) | `Phi / (pi A)` |
//! | `SpotLight` | disc facing the beam, radius max(light radius, 0.1 m) | `Phi / (4 pi A)` |
//! | a SurfaceLight's `SpotLight` | rectangle the size of the face it shines from | `Phi / (4 pi A)` |
//!
//! `Phi` is the light's `intensity` and `A` the area of the traced proxy.
//!
//! # Units, and why these luminances
//!
//! - `PointLight::intensity` and `SpotLight::intensity` are lumens. Bevy's
//!   renderer divides both by 4 pi (`bevy_pbr` `extract_lights`): a spot is a
//!   point light masked to a cone, `Phi / (4 pi)` candela on its axis, the
//!   same as a point light of the same lumens.
//! - `StandardMaterial::emissive` is luminance in cd/m² (its doc). Solari
//!   takes it as the radiance a triangle emits from its front face
//!   (`saturate(dot(-wi, normal))` in `sampling.wgsl`), ignores
//!   `emissive_exposure_weight`, and multiplies its whole result by the
//!   view's exposure, as Bevy does with direct light. A proxy is therefore in
//!   the units of the light it stands for, including the exposure-following
//!   lumens `light_classes` computes from a light's brightness.
//! - A one-sided Lambertian surface of area A and luminance L emits pi L A
//!   lumens; a flat one emits L A candela along its normal. A sphere with
//!   L = Phi / (pi A) emits the light's Phi lumens, Phi / (4 pi) candela in
//!   every direction on average: Bevy's point light. A disc or face with
//!   L = Phi / (4 pi A) gives Bevy's Phi / (4 pi) candela along its axis, so
//!   the centre of a beam is as bright as the raster light's. (Phi / (pi A)
//!   would make it four times brighter.)
//! - A is the area of the triangles Solari traces, measured from the shared
//!   mesh times the proxy's scale. Solari's light sampling uses the same
//!   world-space triangle areas, so the faceting costs no brightness.
//!
//! # What ray tracing loses
//!
//! - Range. Emissive light falls off with the square of the distance and
//!   never stops; Bevy fades a light out by its range, `(1 - (d/r)^4)^2`. At
//!   half the range the raster light is 0.88 of the traced one, at the range
//!   0, while the traced one still lights.
//! - The cone. A disc lights the whole half-space in front of it with cosine
//!   falloff: no cut-off at the outer angle, no soft edge from the inner one.
//!   The centre of the beam matches; a 90 degree Roblox cone (outer 45
//!   degrees) puts about 2.2 times the raster light's lumens into the room,
//!   the extra outside the cone.
//! - Shadow switches. Solari traces every proxy's visibility, so a light
//!   with `shadows = false` casts traced shadows.
//! - Small sources. A light radius under 0.1 m is raised to 0.1 m: a
//!   smaller, brighter source makes fireflies.
//!
//! # Which lights, and how many
//!
//! A proxy follows its Bevy light as `light_classes` and the engine's
//! culler leave it: there is none while the light's intensity is zero
//! (switched off, or dimmed by the culler for being far or past its
//! budget), while it is hidden, or while it is off render layer 0.
//! Following the culler keeps the traced lights the raster ones, and keeps
//! their count down: Solari picks the light for each sample uniformly among
//! all of its lights, the sun included, so every proxy takes samples from
//! the sun.
//!
//! Solari panics with more than 65,535 lights (suns, emissive parts and
//! proxies together). Proxies are capped at [`PROXY_LIMIT`] and at what the
//! other lights leave of that limit; past the cap the lights nearest the
//! camera keep theirs, and a warning says how many went without.
//!
//! # A point light inside its part
//!
//! A Roblox-style PointLight sits at the centre of its part, so its sphere is
//! inside the part, and Solari has no "the part this light belongs to"
//! exception: the part would block all of its light. A part up to
//! [`HOST_EXCLUDE_MAX_SIDE_M`] on its longest side that a point light's
//! sphere touches therefore leaves the traced scene (`TraceOverride::Never`,
//! marked [`ExcludedForLight`] so it is the only override this module ever
//! takes away). It is still drawn and lit; it casts no traced shadow and
//! bounces no light. A bigger part stays and blocks its light; the info log
//! counts those. A SpotLight's or SurfaceLight's proxy sits on the face it
//! shines from, [`FACE_OFFSET_M`] in front of it, and needs no exception.
//!
//! # Never drawn, picked or saved
//!
//! A proxy is an entity of its own with [`LightProxy`], `TraceOverride::Always`,
//! the trace marker, its material and a transform. It has no `Mesh3d`, so
//! Bevy draws nothing for it and the mesh sync in `super` (which only
//! considers entities with one) leaves it alone; no `Instance`,
//! `InstanceFile` or `Name`, which the Explorer, the viewport picker and
//! Space saving start from; no collider. Proxies share three unit meshes
//! (sphere, disc, square) scaled per light, and each light has a material of
//! its own, since its emissive differs. A non-uniform scale is only ever in
//! the plane of a disc or face, where it leaves the normal alone (Solari
//! transforms normals by the plain instance matrix).
//!
//! A proxy leaves the traced scene by being despawned, never by losing its
//! marker: Solari 0.19 copies `RaytracingMesh3d` to the render world in its
//! own extract system and never removes it there, while a despawn takes the
//! render entity with it.
//!
//! # Cost
//!
//! Steady state: the change-filtered light queries and a count of the
//! wanted proxies. A light is evaluated again when its Bevy light,
//! transform, visibility, layers or parent change, and every light once
//! every [`REFRESH_FRAMES`] frames, which catches what no change filter
//! sees (a part resized without moving its light, a proxy despawned by
//! someone else).

use std::collections::{HashMap, HashSet};
use std::f32::consts::{PI, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::system::SystemParam;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use eustress_common::classes::{BasePart, SurfaceLight};
use eustress_common::plugins::light_classes::{face_rotation, LightEmitter, LightFaceHost};

use super::{
    conform, is_traced, raytracing_active, Conformance, EntityFacts, MaterialFacts, SkipReason,
    TraceMarker, TraceOverride,
};

// ============================================================================
// Constants
// ============================================================================

/// The smallest proxy radius. A smaller source of the same power is
/// brighter per square metre and makes fireflies.
pub const MIN_RADIUS_M: f32 = 0.1;

/// The shortest side of a face proxy: the diameter of the smallest disc.
pub const MIN_FACE_SIDE_M: f32 = 2.0 * MIN_RADIUS_M;

/// How far a disc or face proxy sits in front of the light. A face light's
/// emitter is on the surface of its part; ten times Solari's `RAY_T_MIN`
/// (0.001 m) in front of it, the part's own face is never hit by a ray
/// between a lit point and the proxy.
pub const FACE_OFFSET_M: f32 = 0.01;

/// The longest side of a part that leaves the traced scene for a point
/// light inside it. A lamp, bulb or panel is smaller; a bigger part keeps
/// casting its shadows and blocks the light instead.
pub const HOST_EXCLUDE_MAX_SIDE_M: f32 = 4.0;

/// The most proxies at once. The engine's culler keeps 64 lights lit, so
/// this is reached only without it; it bounds how far the proxies dilute
/// the sun's share of Solari's light samples.
pub const PROXY_LIMIT: usize = 1024;

/// Solari's limit on light sources of every kind together
/// (`prepare_raytracing_scene_bindings` panics above it).
pub const LIGHT_SOURCE_LIMIT: usize = u16::MAX as usize;

/// Kept free below [`LIGHT_SOURCE_LIMIT`] when the other light sources are
/// counted one by one, which happens at most every [`REFRESH_FRAMES`]
/// frames: room for emissive parts added in between.
pub const SOURCE_MARGIN: usize = 512;

/// Every light is evaluated again this often, in frames.
pub const REFRESH_FRAMES: u32 = 60;

/// Times the icosahedron's triangles are split in four: 80 triangles of
/// nearly equal area. Solari picks a mesh light's triangles uniformly, so
/// even areas keep the noise down; the area left out of the ideal sphere
/// (7 percent) is counted in the luminance.
const SPHERE_SUBDIVISIONS: usize = 1;

/// Triangles in the disc, a fan of equal slices.
const DISC_SEGMENTS: usize = 16;

// ============================================================================
// Shapes
// ============================================================================

/// What a proxy looks like to Solari.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProxyShape {
    /// A point light.
    Sphere,
    /// A spot light, or a SurfaceLight that is not in a part.
    Disc,
    /// A SurfaceLight on a part's face.
    Face,
}

/// A triangle list with one flat normal per triangle (its winding gives the
/// side that emits), one UV triangle each (enough for mikktspace) and
/// `U32` indices: the raster layout [`conform`] turns into Solari's.
fn flat_mesh(triangles: &[[Vec3; 3]]) -> Mesh {
    let mut positions = Vec::with_capacity(triangles.len() * 3);
    let mut normals = Vec::with_capacity(triangles.len() * 3);
    let mut uvs = Vec::with_capacity(triangles.len() * 3);
    for [a, b, c] in triangles {
        let normal = (*b - *a).cross(*c - *a).normalize_or_zero().to_array();
        positions.extend([a.to_array(), b.to_array(), c.to_array()]);
        normals.extend([normal; 3]);
        uvs.extend([[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
    }
    let count = positions.len() as u32;
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32((0..count).collect()))
}

/// A sphere of radius 1 (its corners on the sphere), faces pointing out.
pub fn unit_sphere() -> Mesh {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let corners = [
        Vec3::new(-1.0, t, 0.0),
        Vec3::new(1.0, t, 0.0),
        Vec3::new(-1.0, -t, 0.0),
        Vec3::new(1.0, -t, 0.0),
        Vec3::new(0.0, -1.0, t),
        Vec3::new(0.0, 1.0, t),
        Vec3::new(0.0, -1.0, -t),
        Vec3::new(0.0, 1.0, -t),
        Vec3::new(t, 0.0, -1.0),
        Vec3::new(t, 0.0, 1.0),
        Vec3::new(-t, 0.0, -1.0),
        Vec3::new(-t, 0.0, 1.0),
    ]
    .map(Vec3::normalize);
    const FACES: [[usize; 3]; 20] = [
        [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
        [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
        [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
        [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
    ];
    let mut triangles: Vec<[Vec3; 3]> = FACES.iter().map(|f| f.map(|i| corners[i])).collect();
    for _ in 0..SPHERE_SUBDIVISIONS {
        let mid = |p: Vec3, q: Vec3| ((p + q) * 0.5).normalize();
        triangles = triangles
            .iter()
            .flat_map(|&[a, b, c]| {
                let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
                [[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]
            })
            .collect();
    }
    // Wound to face out, so the flat normal points away from the centre.
    for [a, b, c] in &mut triangles {
        if (*b - *a).cross(*c - *a).dot(*a + *b + *c) < 0.0 {
            std::mem::swap(b, c);
        }
    }
    flat_mesh(&triangles)
}

/// A disc of radius 1 in the XY plane, emitting along -Z: the direction a
/// Bevy spot light shines, so a proxy takes its emitter's rotation as is.
pub fn unit_disc() -> Mesh {
    let rim = |i: usize| {
        let angle = i as f32 * TAU / DISC_SEGMENTS as f32;
        Vec3::new(angle.cos(), angle.sin(), 0.0)
    };
    let triangles: Vec<[Vec3; 3]> =
        (0..DISC_SEGMENTS).map(|i| [Vec3::ZERO, rim(i + 1), rim(i)]).collect();
    flat_mesh(&triangles)
}

/// A square of side 1 in the XY plane, emitting along -Z like the disc.
pub fn unit_face() -> Mesh {
    let (a, b, c, d) = (
        Vec3::new(-0.5, -0.5, 0.0),
        Vec3::new(0.5, -0.5, 0.0),
        Vec3::new(0.5, 0.5, 0.0),
        Vec3::new(-0.5, 0.5, 0.0),
    );
    flat_mesh(&[[a, c, b], [a, d, c]])
}

/// The summed area of a triangle list's triangles.
pub fn mesh_area(mesh: &Mesh) -> f32 {
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        return 0.0;
    };
    let corner = |i: usize| Vec3::from_array(positions[i]);
    let indices: Vec<usize> = match mesh.indices() {
        Some(indices) => indices.iter().collect(),
        None => (0..positions.len()).collect(),
    };
    indices
        .chunks_exact(3)
        .map(|t| (corner(t[1]) - corner(t[0])).cross(corner(t[2]) - corner(t[0])).length() * 0.5)
        .sum()
}

/// One shared proxy mesh, in Solari's layout.
#[derive(Clone, Debug)]
pub struct SharedMesh {
    pub handle: Handle<Mesh>,
    /// Area at scale 1, m².
    pub unit_area: f32,
    pub triangles: u32,
}

/// The three meshes every proxy uses, made once.
#[derive(Clone, Debug)]
pub struct ProxyMeshes {
    pub sphere: SharedMesh,
    pub disc: SharedMesh,
    pub face: SharedMesh,
}

impl ProxyMeshes {
    fn build(meshes: &mut Assets<Mesh>) -> Result<Self, SkipReason> {
        let mut shared = |mesh: Mesh| -> Result<SharedMesh, SkipReason> {
            let traced = match conform(&mesh) {
                Conformance::AsIs { .. } => mesh,
                Conformance::Copy(copy) => copy,
                Conformance::Skip(reason) => return Err(reason),
            };
            let unit_area = mesh_area(&traced);
            let triangles = traced.indices().map_or(0, |i| (i.len() / 3) as u32);
            Ok(SharedMesh { handle: meshes.add(traced), unit_area, triangles })
        };
        Ok(Self { sphere: shared(unit_sphere())?, disc: shared(unit_disc())?, face: shared(unit_face())? })
    }

    pub fn get(&self, shape: ProxyShape) -> &SharedMesh {
        match shape {
            ProxyShape::Sphere => &self.sphere,
            ProxyShape::Disc => &self.disc,
            ProxyShape::Face => &self.face,
        }
    }
}

// ============================================================================
// Calibration
// ============================================================================

/// The luminance (cd/m², Bevy's emissive unit) at which a one-sided
/// Lambertian surface of `area` m² emits `lumens` in total: Phi = pi L A.
pub fn luminance_for_flux(lumens: f32, area: f32) -> f32 {
    lumens / (PI * area)
}

/// The luminance at which a flat one-sided Lambertian surface of `area` m²
/// emits Bevy's luminous intensity for `lumens` along its normal: Bevy
/// shades a point or spot light of Phi lumens with Phi / (4 pi) candela,
/// and a flat emitter gives I = L A on its axis.
pub fn luminance_for_axis_intensity(lumens: f32, area: f32) -> f32 {
    lumens / (4.0 * PI * area)
}

/// The emissive for a light's colour at `luminance`, or `None` when it would
/// emit nothing (off, black, negative or not finite). Bevy multiplies the
/// light's linear colour by its intensity; the proxy does the same.
fn emission(color: Color, luminance: f32) -> Option<LinearRgba> {
    let c = color.to_linear();
    let e = LinearRgba::rgb(
        c.red.max(0.0) * luminance,
        c.green.max(0.0) * luminance,
        c.blue.max(0.0) * luminance,
    );
    let finite = e.red.is_finite() && e.green.is_finite() && e.blue.is_finite();
    (luminance > 0.0 && finite && (e.red > 0.0 || e.green > 0.0 || e.blue > 0.0)).then_some(e)
}

fn proxy_radius(light_radius: f32) -> f32 {
    if light_radius.is_finite() { light_radius.max(MIN_RADIUS_M) } else { MIN_RADIUS_M }
}

/// The size of a face, along the emitter's own X and Y, for a part of
/// `host_size`: the part's size turned by the face's rotation (which only
/// ever swaps and flips axes).
pub fn face_size(face: &str, host_size: Vec3) -> Vec2 {
    let local = (face_rotation(face).inverse() * host_size).abs();
    Vec2::new(local.x, local.y)
}

/// A proxy as it should be this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProxySpec {
    pub shape: ProxyShape,
    /// World pose; the scale is the proxy's size.
    pub transform: Transform,
    pub emissive: LinearRgba,
}

/// The sphere for a point light at `position`, or `None` when it is dark.
pub fn point_proxy(light: &PointLight, position: Vec3, unit_area: f32) -> Option<ProxySpec> {
    let radius = proxy_radius(light.radius);
    let area = unit_area * radius * radius;
    let emissive = emission(light.color, luminance_for_flux(light.intensity, area))?;
    Some(ProxySpec {
        shape: ProxyShape::Sphere,
        transform: Transform { translation: position, rotation: Quat::IDENTITY, scale: Vec3::splat(radius) },
        emissive,
    })
}

/// The disc (or, with `face`, the rectangle of that size) for a spot light
/// at `position` shining along `rotation * -Z`, or `None` when it is dark.
pub fn spot_proxy(
    light: &SpotLight,
    position: Vec3,
    rotation: Quat,
    face: Option<Vec2>,
    disc_unit_area: f32,
    face_unit_area: f32,
) -> Option<ProxySpec> {
    let (shape, scale, area) = match face {
        Some(size) => {
            let (w, h) = (size.x.max(MIN_FACE_SIDE_M), size.y.max(MIN_FACE_SIDE_M));
            (ProxyShape::Face, Vec3::new(w, h, 1.0), face_unit_area * w * h)
        }
        None => {
            let r = proxy_radius(light.radius);
            (ProxyShape::Disc, Vec3::new(r, r, 1.0), disc_unit_area * r * r)
        }
    };
    let emissive = emission(light.color, luminance_for_axis_intensity(light.intensity, area))?;
    let translation = position + rotation * Vec3::NEG_Z * FACE_OFFSET_M;
    Some(ProxySpec { shape, transform: Transform { translation, rotation, scale }, emissive })
}

/// Whether Phase 1's rules let a proxy with `triangles` triangles be traced:
/// visible, on the main layer, and under Solari's emissive triangle limit
/// (the crash guard no override lifts). `TraceOverride::Always` lifts the
/// shadow-caster, lit and opaque rules a light proxy fails by design.
pub fn proxy_traceable(visible: bool, main_layer: bool, triangles: u32) -> bool {
    let facts = EntityFacts {
        visible,
        casts_shadows: false,
        main_layer,
        over: Some(TraceOverride::Always),
        material: Some(MaterialFacts { opaque: true, lit: false, emissive: true }),
    };
    is_traced(&facts, triangles)
}

fn on_main_layer(layers: Option<&RenderLayers>) -> bool {
    layers.is_none_or(|layers| layers.intersects(&RenderLayers::default()))
}

/// Whether a sphere touches a part's box (`size` in metres, posed by the
/// part's transform).
pub fn sphere_touches_part(center: Vec3, radius: f32, part: &GlobalTransform, size: Vec3) -> bool {
    let (_, rotation, translation) = part.to_scale_rotation_translation();
    let local = rotation.inverse() * (center - translation);
    let half = size.abs() * 0.5;
    local.distance_squared(local.clamp(-half, half)) <= radius * radius
}

fn proxy_material(emissive: LinearRgba) -> StandardMaterial {
    StandardMaterial {
        // Black, with no specular: a proxy is a light, not a surface, and
        // reflects nothing.
        base_color: Color::BLACK,
        reflectance: 0.0,
        perceptual_roughness: 1.0,
        emissive,
        unlit: true,
        ..default()
    }
}

// ============================================================================
// Components and state
// ============================================================================

/// Which Bevy light component a proxy stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LightKind {
    Point,
    Spot,
}

/// On a proxy: the light it stands for.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightProxy {
    pub light: Entity,
    pub kind: LightKind,
}

/// On a part this module took out of the traced scene for a point light
/// inside it, beside the `TraceOverride::Never` it inserted.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ExcludedForLight;

#[derive(Default)]
struct Record {
    /// Made the first time the light wants a proxy.
    material: Option<Handle<StandardMaterial>>,
    proxy: Option<Entity>,
    /// What the light asks for, before the cap.
    want: Option<ProxySpec>,
    /// What its proxy shows.
    shown: Option<ProxySpec>,
    /// The part it took out of the traced scene.
    host: Option<Entity>,
    /// Inside a part too big to take out.
    in_big_host: bool,
}

/// Proxies by outcome, for the log and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProxyCounts {
    pub spheres: usize,
    pub discs: usize,
    pub faces: usize,
    /// Lights that want a proxy, traced or capped.
    pub wanting: usize,
    pub hosts_excluded: usize,
    pub in_big_hosts: usize,
}

/// The proxies and what they stand for.
#[derive(Resource)]
pub struct LightProxies {
    meshes: Option<ProxyMeshes>,
    mesh_failed: bool,
    records: HashMap<(Entity, LightKind), Record>,
    /// Parts taken out of the traced scene, with how many lights need it.
    hosts: HashMap<Entity, u32>,
    frame: u32,
    proxy_limit: usize,
    source_limit: usize,
    source_margin: usize,
    /// The other light sources counted one by one, and the frame it was done.
    exact_others: Option<(usize, u32)>,
    capped: bool,
    logged: ProxyCounts,
    evaluations: u64,
}

impl Default for LightProxies {
    fn default() -> Self {
        Self {
            meshes: None,
            mesh_failed: false,
            records: HashMap::new(),
            hosts: HashMap::new(),
            frame: 0,
            proxy_limit: PROXY_LIMIT,
            source_limit: LIGHT_SOURCE_LIMIT,
            source_margin: SOURCE_MARGIN,
            exact_others: None,
            capped: false,
            logged: ProxyCounts::default(),
            evaluations: 0,
        }
    }
}

impl LightProxies {
    /// The proxy entity standing for a light, if it has one.
    pub fn proxy_of(&self, light: Entity, kind: LightKind) -> Option<Entity> {
        self.records.get(&(light, kind)).and_then(|r| r.proxy)
    }

    pub fn meshes(&self) -> Option<&ProxyMeshes> {
        self.meshes.as_ref()
    }

    /// True while more lights want a proxy than may have one.
    pub fn is_capped(&self) -> bool {
        self.capped
    }

    /// Lights evaluated so far. For profiling: a frame with no change and no
    /// refresh adds none.
    pub fn evaluations(&self) -> u64 {
        self.evaluations
    }

    pub fn counts(&self) -> ProxyCounts {
        let mut counts = ProxyCounts { hosts_excluded: self.hosts.len(), ..default() };
        for record in self.records.values() {
            counts.wanting += usize::from(record.want.is_some());
            counts.in_big_hosts += usize::from(record.in_big_host);
            if record.proxy.is_some() {
                match record.shown.map(|s| s.shape) {
                    Some(ProxyShape::Sphere) => counts.spheres += 1,
                    Some(ProxyShape::Disc) => counts.discs += 1,
                    Some(ProxyShape::Face) => counts.faces += 1,
                    None => {}
                }
            }
        }
        counts
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Adds the proxy sync. Called by `super` when ray tracing is active, with
/// Solari's `RaytracingMesh3d` as `M`; the run condition repeats the check.
/// A second call adds nothing: two syncs would each spawn a proxy for a new
/// light in the frame the other's spawn is still a queued command.
pub fn add_systems<M: TraceMarker>(app: &mut App) {
    if app.world().contains_resource::<LightProxies>() {
        return;
    }
    app.init_resource::<LightProxies>()
        .add_systems(Last, sync_light_proxies::<M>.run_if(raytracing_active));
}

type PointChanged = Or<(
    Changed<PointLight>,
    Changed<GlobalTransform>,
    Changed<InheritedVisibility>,
    Changed<RenderLayers>,
    Changed<ChildOf>,
)>;

type SpotChanged =
    Or<(Changed<SpotLight>, Changed<GlobalTransform>, Changed<InheritedVisibility>, Changed<RenderLayers>)>;

#[derive(SystemParam)]
struct Lights<'w, 's> {
    points: Query<
        'w,
        's,
        (
            Entity,
            &'static PointLight,
            &'static GlobalTransform,
            &'static InheritedVisibility,
            Option<&'static RenderLayers>,
            Option<&'static ChildOf>,
        ),
        Without<LightProxy>,
    >,
    spots: Query<
        'w,
        's,
        (
            Entity,
            &'static SpotLight,
            &'static GlobalTransform,
            &'static InheritedVisibility,
            Option<&'static RenderLayers>,
            Option<&'static LightEmitter>,
        ),
        Without<LightProxy>,
    >,
    changed_points: Query<'w, 's, Entity, (With<PointLight>, Without<LightProxy>, PointChanged)>,
    changed_spots: Query<'w, 's, Entity, (With<SpotLight>, Without<LightProxy>, SpotChanged)>,
    removed_points: RemovedComponents<'w, 's, PointLight>,
    removed_spots: RemovedComponents<'w, 's, SpotLight>,
    removed_layers: RemovedComponents<'w, 's, RenderLayers>,
}

#[derive(SystemParam)]
struct Surroundings<'w, 's> {
    /// A SpotLight emitter's owner: a SurfaceLight's face, and its part.
    owners: Query<'w, 's, (Option<&'static SurfaceLight>, Option<&'static ChildOf>, Option<&'static LightFaceHost>)>,
    parts: Query<'w, 's, (&'static BasePart, &'static GlobalTransform), Without<LightProxy>>,
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform), Without<LightProxy>>,
    directional: Query<'w, 's, (), With<DirectionalLight>>,
}

/// What one light needs this frame.
struct Evaluation {
    want: Option<ProxySpec>,
    host: Option<Entity>,
    in_big_host: bool,
}

/// `None` when the light no longer has the component (or is gone).
fn evaluate(key: (Entity, LightKind), lights: &Lights, around: &Surroundings, shapes: &ProxyMeshes) -> Option<Evaluation> {
    match key.1 {
        LightKind::Point => {
            let (_, light, transform, visibility, layers, child_of) = lights.points.get(key.0).ok()?;
            let center = transform.translation();
            let mut host = None;
            let mut in_big_host = false;
            if let Some(parent) = child_of.map(ChildOf::parent) {
                if let Ok((part, part_transform)) = around.parts.get(parent) {
                    if sphere_touches_part(center, proxy_radius(light.radius), part_transform, part.size) {
                        if part.size.abs().max_element() <= HOST_EXCLUDE_MAX_SIDE_M {
                            host = Some(parent);
                        } else {
                            in_big_host = true;
                        }
                    }
                }
            }
            let want = point_proxy(light, center, shapes.sphere.unit_area)
                .filter(|_| proxy_traceable(visibility.get(), on_main_layer(layers), shapes.sphere.triangles));
            Some(Evaluation { want, host, in_big_host })
        }
        LightKind::Spot => {
            let (_, light, transform, visibility, layers, emitter) = lights.spots.get(key.0).ok()?;
            let (_, rotation, position) = transform.to_scale_rotation_translation();
            let face = emitter.and_then(|e| face_of(e.owner, around));
            let triangles = if face.is_some() { shapes.face.triangles } else { shapes.disc.triangles };
            let want = spot_proxy(light, position, rotation, face, shapes.disc.unit_area, shapes.face.unit_area)
                .filter(|_| proxy_traceable(visibility.get(), on_main_layer(layers), triangles));
            Some(Evaluation { want, host: None, in_big_host: false })
        }
    }
}

/// A SurfaceLight's face size, when its emitter's owner is one and sits in
/// a part (its hierarchy parent, or the host the Player recorded), as
/// `light_classes` places the emitter.
fn face_of(owner: Entity, around: &Surroundings) -> Option<Vec2> {
    let (surface, child_of, face_host) = around.owners.get(owner).ok()?;
    let surface = surface?;
    let size = child_of
        .and_then(|c| around.parts.get(c.parent()).ok())
        .map(|(part, _)| part.size)
        .or(face_host.map(|h| h.size))?;
    Some(face_size(&surface.face, size))
}

/// Keep a part out of the traced scene while a light needs it, and put it
/// back when none does.
fn set_host(hosts: &mut HashMap<Entity, u32>, commands: &mut Commands, current: &mut Option<Entity>, next: Option<Entity>) {
    if *current == next {
        return;
    }
    if let Some(old) = current.take() {
        if let Some(count) = hosts.get_mut(&old) {
            *count -= 1;
            if *count == 0 {
                hosts.remove(&old);
                commands.entity(old).queue_silenced(restore_host);
            }
        }
    }
    if let Some(new) = next {
        let count = hosts.entry(new).or_insert(0);
        *count += 1;
        if *count == 1 {
            commands.entity(new).queue_silenced(exclude_host);
        }
    }
    *current = next;
}

/// Only a part with no override of its own: someone's `Always` or `Never`
/// stays as it is.
fn exclude_host(mut part: EntityWorldMut) {
    if !part.contains::<TraceOverride>() {
        part.insert((TraceOverride::Never, ExcludedForLight));
    }
}

/// Only what [`exclude_host`] inserted, and only while it is still `Never`.
fn restore_host(mut part: EntityWorldMut) {
    if part.take::<ExcludedForLight>().is_some() && part.get::<TraceOverride>() == Some(&TraceOverride::Never) {
        part.remove::<TraceOverride>();
    }
}

fn drop_record(state: &mut LightProxies, commands: &mut Commands, key: (Entity, LightKind)) {
    if let Some(mut record) = state.records.remove(&key) {
        if let Some(proxy) = record.proxy.take() {
            commands.entity(proxy).try_despawn();
        }
        set_host(&mut state.hosts, commands, &mut record.host, None);
    }
}

type Shown<'w, 's> = Query<'w, 's, (&'static mut Transform, &'static mut GlobalTransform), With<LightProxy>>;

/// Make a light's proxy what `target` says: spawn, update or despawn it.
fn apply<M: TraceMarker>(
    key: (Entity, LightKind),
    record: &mut Record,
    target: Option<ProxySpec>,
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    shown: &mut Shown,
    shapes: &ProxyMeshes,
) {
    let Some(spec) = target else {
        if let Some(proxy) = record.proxy.take() {
            commands.entity(proxy).try_despawn();
        }
        record.shown = None;
        return;
    };
    let fresh_material = !record.material.as_ref().is_some_and(|m| materials.contains(m));
    if fresh_material {
        record.material = Some(materials.add(proxy_material(spec.emissive)));
    } else if let Some(mut material) = record.material.as_ref().and_then(|m| materials.get_mut(m)) {
        if material.emissive != spec.emissive {
            material.emissive = spec.emissive;
        }
    }
    let Some(material) = record.material.clone() else { return };
    let mesh = shapes.get(spec.shape).handle.clone();
    match record.proxy.filter(|p| shown.contains(*p)) {
        Some(proxy) => {
            let new_shape = record.shown.map(|s| s.shape) != Some(spec.shape);
            if new_shape || fresh_material {
                commands.entity(proxy).insert((M::new(mesh), MeshMaterial3d(material)));
            }
            if let Ok((mut transform, mut global)) = shown.get_mut(proxy) {
                if *transform != spec.transform {
                    *transform = spec.transform;
                    // A root entity: its global pose is its pose. Written
                    // here because transform propagation has already run
                    // this frame and Solari extracts the global one.
                    *global = GlobalTransform::from(spec.transform);
                }
            }
        }
        None => {
            let proxy = commands
                .spawn((
                    LightProxy { light: key.0, kind: key.1 },
                    TraceOverride::Always,
                    MeshMaterial3d(material),
                    spec.transform,
                    GlobalTransform::from(spec.transform),
                    M::new(mesh),
                ))
                .id();
            record.proxy = Some(proxy);
        }
    }
    record.shown = Some(spec);
}

/// How many proxies may exist: [`PROXY_LIMIT`], and no more than the other
/// light sources leave of Solari's limit. Returns that and the other
/// sources counted. Their count is an upper bound for free (directional
/// lights plus every traced entity, emissive or not); only when that bound
/// comes near the limit are the emissive ones counted, at most every
/// [`REFRESH_FRAMES`] frames, with [`SOURCE_MARGIN`] kept free.
fn allowed_proxies<M: Component>(
    state: &mut LightProxies,
    directional: &Query<(), With<DirectionalLight>>,
    traced: &Query<&MeshMaterial3d<StandardMaterial>, (With<M>, Without<LightProxy>)>,
    materials: &Assets<StandardMaterial>,
) -> (usize, usize) {
    let bound = directional.count() + traced.count();
    let others = if bound + state.proxy_limit + state.source_margin <= state.source_limit {
        state.exact_others = None;
        bound
    } else {
        match state.exact_others {
            Some((n, at)) if state.frame.wrapping_sub(at) < REFRESH_FRAMES => n,
            _ => {
                // A material not loaded yet counts as emissive.
                let emissive = traced
                    .iter()
                    .filter(|m| materials.get(m.id()).is_none_or(|m| MaterialFacts::of(m).emissive))
                    .count();
                let n = directional.count() + emissive + state.source_margin;
                state.exact_others = Some((n, state.frame));
                n
            }
        }
    };
    (state.proxy_limit.min(state.source_limit.saturating_sub(others)), others)
}

/// Keep one proxy per lit point and spot light, in step with it.
#[allow(clippy::too_many_arguments)]
fn sync_light_proxies<M: TraceMarker>(
    mut commands: Commands,
    mut state: ResMut<LightProxies>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lights: Lights,
    around: Surroundings,
    mut shown: Shown,
    traced: Query<&MeshMaterial3d<StandardMaterial>, (With<M>, Without<LightProxy>)>,
) {
    let state = &mut *state;
    if state.meshes.is_none() {
        if state.mesh_failed {
            return;
        }
        match ProxyMeshes::build(&mut meshes) {
            Ok(built) => state.meshes = Some(built),
            Err(reason) => {
                // Cannot happen with the meshes above; said once if it does.
                error!("Ray tracing: light proxy meshes cannot be traced ({reason:?}); point and spot lights stay dark");
                state.mesh_failed = true;
                return;
            }
        }
    }
    let Some(shapes) = state.meshes.clone() else { return };
    state.frame = state.frame.wrapping_add(1);
    let full = state.frame % REFRESH_FRAMES == 1;

    // Lights that are gone, or are no longer that kind of light.
    let gone: Vec<(Entity, LightKind)> = lights
        .removed_points
        .read()
        .map(|e| (e, LightKind::Point))
        .chain(lights.removed_spots.read().map(|e| (e, LightKind::Spot)))
        .collect();
    for key in gone {
        if !lights.points.contains(key.0) && key.1 == LightKind::Point
            || !lights.spots.contains(key.0) && key.1 == LightKind::Spot
        {
            drop_record(state, &mut commands, key);
        }
    }
    let unlayered: Vec<Entity> = lights.removed_layers.read().collect();

    let mut todo: HashSet<(Entity, LightKind)> = HashSet::new();
    if full {
        todo.extend(lights.points.iter().map(|(e, ..)| (e, LightKind::Point)));
        todo.extend(lights.spots.iter().map(|(e, ..)| (e, LightKind::Spot)));
        todo.extend(state.records.keys().copied());
    } else {
        todo.extend(lights.changed_points.iter().map(|e| (e, LightKind::Point)));
        todo.extend(lights.changed_spots.iter().map(|e| (e, LightKind::Spot)));
        for e in unlayered {
            if lights.points.contains(e) {
                todo.insert((e, LightKind::Point));
            }
            if lights.spots.contains(e) {
                todo.insert((e, LightKind::Spot));
            }
        }
    }

    let mut evaluated = Vec::with_capacity(todo.len());
    for key in todo {
        state.evaluations += 1;
        let Some(eval) = evaluate(key, &lights, &around, &shapes) else {
            drop_record(state, &mut commands, key);
            continue;
        };
        let record = state.records.entry(key).or_default();
        record.want = eval.want;
        record.in_big_host = eval.in_big_host;
        let mut host = record.host;
        set_host(&mut state.hosts, &mut commands, &mut host, eval.host);
        if let Some(record) = state.records.get_mut(&key) {
            record.host = host;
        }
        evaluated.push(key);
    }

    let (allowed, others) = allowed_proxies::<M>(state, &around.directional, &traced, &materials);
    let wanting = state.records.values().filter(|r| r.want.is_some()).count();
    let showing = state.records.values().filter(|r| r.proxy.is_some()).count();
    let materials = &mut *materials;

    if wanting <= allowed {
        // Everyone who wants a proxy has one. After a capped spell every
        // light may have been held back, so all are applied once.
        let all = full || state.capped;
        if state.capped {
            info!("Ray tracing: every lit light is traced again ({wanting} of {allowed})");
        }
        state.capped = false;
        let keys: Vec<_> = if all { state.records.keys().copied().collect() } else { evaluated };
        for key in keys {
            if let Some(record) = state.records.get_mut(&key) {
                let target = record.want;
                apply::<M>(key, record, target, &mut commands, materials, &mut shown, &shapes);
            }
        }
    } else if full || !state.capped || showing > allowed {
        // Over the cap: the nearest lights to the camera keep theirs.
        let eye = around
            .cameras
            .iter()
            .find(|(camera, _)| camera.order == 0)
            .map(|(_, t)| t.translation())
            .unwrap_or(Vec3::ZERO);
        let mut ranked: Vec<((Entity, LightKind), f32)> = state
            .records
            .iter()
            .filter_map(|(key, r)| r.want.map(|w| (*key, w.transform.translation.distance_squared(eye))))
            .collect();
        ranked.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0 .0.cmp(&b.0 .0)));
        let permitted: HashSet<(Entity, LightKind)> = ranked.iter().take(allowed).map(|(k, _)| *k).collect();
        if !state.capped {
            warn!(
                "Ray tracing: {wanting} lights are lit but {allowed} can be traced (at most {} proxies, and {others} other light sources of Solari's {}); the {allowed} nearest the camera are",
                state.proxy_limit, state.source_limit
            );
        }
        state.capped = true;
        let keys: Vec<_> = state.records.keys().copied().collect();
        for key in keys {
            if let Some(record) = state.records.get_mut(&key) {
                let target = record.want.filter(|_| permitted.contains(&key));
                apply::<M>(key, record, target, &mut commands, materials, &mut shown, &shapes);
            }
        }
    } else {
        // Still over the cap between rankings: a light keeps its proxy while
        // it wants one, and a newly lit light takes a free slot if any.
        let mut free = allowed.saturating_sub(showing);
        for key in evaluated {
            if let Some(record) = state.records.get_mut(&key) {
                let target = match record.want {
                    Some(want) if record.proxy.is_some() => Some(want),
                    Some(want) if free > 0 => {
                        free -= 1;
                        Some(want)
                    }
                    _ => None,
                };
                apply::<M>(key, record, target, &mut commands, materials, &mut shown, &shapes);
            }
        }
    }

    if full {
        let counts = state.counts();
        if counts != state.logged {
            state.logged = counts;
            info!(
                "Ray tracing: lights traced as emissive proxies: {} point, {} spot, {} surface; {} parts left out of the traced scene for the point light inside them; {} point lights inside a part too big to leave out",
                counts.spheres, counts.discs, counts.faces, counts.hosts_excluded, counts.in_big_hosts
            );
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::super::{
        add_tracing_systems, RaytracingLaunch, RaytracingStatus, TracedMeshes, EMISSIVE_TRIANGLE_LIMIT,
    };
    use super::*;
    use bevy::asset::AssetPlugin;
    use bevy::transform::TransformPlugin;
    use eustress_common::plugins::light_classes::emitter_transform;

    /// Stands in for Solari's `RaytracingMesh3d`.
    #[derive(Component)]
    struct Traced(Handle<Mesh>);

    impl TraceMarker for Traced {
        fn new(mesh: Handle<Mesh>) -> Self {
            Self(mesh)
        }
        fn mesh_id(&self) -> AssetId<Mesh> {
            self.0.id()
        }
    }

    #[derive(Component, Default)]
    struct Lighting;

    fn bare_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), TransformPlugin))
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>();
        app
    }

    fn app() -> App {
        let mut app = bare_app();
        app.insert_resource(RaytracingLaunch::for_status(RaytracingStatus::Active));
        add_systems::<Traced>(&mut app);
        app
    }

    fn settle(app: &mut App) {
        app.update();
        app.update();
    }

    fn lamp(app: &mut App, light: PointLight, at: Vec3) -> Entity {
        app.world_mut().spawn((light, Transform::from_translation(at), InheritedVisibility::VISIBLE)).id()
    }

    fn spot(app: &mut App, light: SpotLight, at: Vec3, toward: Vec3) -> Entity {
        let up = if toward.y.abs() > 0.9 { Vec3::Z } else { Vec3::Y };
        app.world_mut()
            .spawn((light, Transform::from_translation(at).looking_to(toward, up), InheritedVisibility::VISIBLE))
            .id()
    }

    /// A SurfaceLight as `light_classes` builds it: the class on an entity in
    /// a part (a unit mesh scaled to its size), the `SpotLight` on an emitter
    /// child on the face. Returns (part, emitter).
    fn surface_light(app: &mut App, at: Vec3, size: Vec3, face: &str) -> (Entity, Entity) {
        let world = app.world_mut();
        let part = world
            .spawn((
                BasePart { size, ..default() },
                Transform::from_translation(at).with_scale(size),
                InheritedVisibility::VISIBLE,
            ))
            .id();
        let owner = world.spawn((SurfaceLight { face: face.into(), ..default() }, Transform::default(), ChildOf(part))).id();
        let emitter = world
            .spawn((
                LightEmitter { owner },
                SpotLight { intensity: 800.0, ..default() },
                emitter_transform(face, Some((size, size))),
                InheritedVisibility::VISIBLE,
                ChildOf(owner),
            ))
            .id();
        (part, emitter)
    }

    fn proxies(app: &mut App) -> Vec<(Entity, LightProxy)> {
        let mut q = app.world_mut().query::<(Entity, &LightProxy)>();
        q.iter(app.world()).map(|(e, p)| (e, *p)).collect()
    }

    fn proxy_for(app: &mut App, light: Entity) -> Option<Entity> {
        let found: Vec<Entity> = proxies(app).into_iter().filter(|(_, p)| p.light == light).map(|(e, _)| e).collect();
        assert!(found.len() <= 1, "{} proxies for one light", found.len());
        found.first().copied()
    }

    fn shapes(app: &App) -> ProxyMeshes {
        app.world().resource::<LightProxies>().meshes().expect("no shared meshes").clone()
    }

    fn material_of(app: &App, proxy: Entity) -> Handle<StandardMaterial> {
        app.world().get::<MeshMaterial3d<StandardMaterial>>(proxy).expect("proxy has no material").0.clone()
    }

    fn emissive(app: &App, proxy: Entity) -> LinearRgba {
        let handle = material_of(app, proxy);
        app.world().resource::<Assets<StandardMaterial>>().get(&handle).expect("material gone").emissive
    }

    fn close(a: f32, b: f32, relative: f32) -> bool {
        (a - b).abs() <= relative * b.abs().max(1e-6)
    }

    // ---- shapes and placement

    #[test]
    fn every_light_gets_one_proxy_of_its_shape() {
        let mut app = app();
        let point = lamp(&mut app, PointLight { intensity: 800.0, ..default() }, Vec3::new(1.0, 2.0, 3.0));
        let beam = spot(&mut app, SpotLight { intensity: 800.0, ..default() }, Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y);
        let (_, emitter) = surface_light(&mut app, Vec3::new(10.0, 0.0, 0.0), Vec3::new(4.0, 2.0, 6.0), "Bottom");
        settle(&mut app);
        let shapes = shapes(&app);
        assert_eq!(proxies(&mut app).len(), 3);
        let p = proxy_for(&mut app, point).expect("point light has no proxy");
        let s = proxy_for(&mut app, beam).expect("spot light has no proxy");
        let f = proxy_for(&mut app, emitter).expect("surface light has no proxy");
        let world = app.world();
        let mesh = |e: Entity| world.get::<Traced>(e).map(|t| t.0.id());
        assert_eq!(mesh(p), Some(shapes.sphere.handle.id()));
        assert_eq!(mesh(s), Some(shapes.disc.handle.id()));
        assert_eq!(mesh(f), Some(shapes.face.handle.id()));

        let tp = *world.get::<Transform>(p).unwrap();
        assert_eq!(tp.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(tp.scale, Vec3::splat(MIN_RADIUS_M), "radius 0 is raised to the minimum");

        let ts = *world.get::<Transform>(s).unwrap();
        assert!((ts.rotation * Vec3::NEG_Z - Vec3::NEG_Y).length() < 1e-5, "disc does not face the beam");
        assert!((ts.translation - Vec3::new(0.0, 5.0 - FACE_OFFSET_M, 0.0)).length() < 1e-5, "{}", ts.translation);
        assert_eq!(ts.scale, Vec3::new(MIN_RADIUS_M, MIN_RADIUS_M, 1.0));

        // The bottom face of a 4 x 2 x 6 part at (10, 0, 0): 4 m along X by
        // 6 m along Z, 1 m down, facing down, just in front of the face.
        let tf = *world.get::<Transform>(f).unwrap();
        assert!((tf.rotation * Vec3::NEG_Z - Vec3::NEG_Y).length() < 1e-5, "face does not face out");
        assert!((tf.translation - Vec3::new(10.0, -1.0 - FACE_OFFSET_M, 0.0)).length() < 1e-4, "{}", tf.translation);
        assert!((tf.scale - Vec3::new(4.0, 6.0, 1.0)).length() < 1e-4, "{}", tf.scale);
        assert!((tf.rotation * Vec3::X).abs().abs_diff_eq(Vec3::X, 1e-5), "the 4 m side is not along X");
        assert!((tf.rotation * Vec3::Y).abs().abs_diff_eq(Vec3::Z, 1e-5), "the 6 m side is not along Z");

        // Traced, never drawn, never an instance; one material per light.
        let mut materials = HashSet::new();
        for e in [p, s, f] {
            let entity = world.entity(e);
            assert_eq!(entity.get::<TraceOverride>(), Some(&TraceOverride::Always));
            assert!(!entity.contains::<Mesh3d>(), "a proxy would be drawn");
            assert!(!entity.contains::<Name>());
            let pose = *entity.get::<Transform>().unwrap();
            assert_eq!(entity.get::<GlobalTransform>(), Some(&GlobalTransform::from(pose)));
            materials.insert(material_of(&app, e).id());
        }
        assert_eq!(materials.len(), 3);
    }

    #[test]
    fn shared_meshes_are_what_solari_traces() {
        let mut app = app();
        lamp(&mut app, PointLight::default(), Vec3::ZERO);
        settle(&mut app);
        let shapes = shapes(&app);
        let meshes = app.world().resource::<Assets<Mesh>>();
        for (name, shared, triangles) in
            [("sphere", &shapes.sphere, 80), ("disc", &shapes.disc, 16), ("face", &shapes.face, 2)]
        {
            let mesh = meshes.get(&shared.handle).unwrap();
            let layout = mesh.attributes().map(|(a, _)| a.id).eq([
                Mesh::ATTRIBUTE_POSITION.id,
                Mesh::ATTRIBUTE_NORMAL.id,
                Mesh::ATTRIBUTE_UV_0.id,
                Mesh::ATTRIBUTE_TANGENT.id,
            ]);
            assert!(layout, "{name}: not Solari's attribute set");
            assert!(matches!(mesh.indices(), Some(Indices::U32(_))), "{name}: not U32");
            assert!(mesh.enable_raytracing && mesh.primitive_topology() == PrimitiveTopology::TriangleList);
            assert_eq!(shared.triangles, triangles, "{name}");
            assert!(shared.triangles <= EMISSIVE_TRIANGLE_LIMIT);
            assert!(close(shared.unit_area, mesh_area(mesh), 1e-6));
        }
        assert!(close(shapes.disc.unit_area, 8.0 * (PI / 8.0).sin(), 1e-5), "{}", shapes.disc.unit_area);
        assert!(close(shapes.face.unit_area, 1.0, 1e-6));
        // 92.8 percent of the ideal sphere's 4 pi (rt_lights/calc/ico.py).
        assert!(close(shapes.sphere.unit_area, 11.665931, 1e-4), "{}", shapes.sphere.unit_area);

        // Flat normals: out of the sphere, along -Z for the disc and face.
        for (name, shared) in [("sphere", &shapes.sphere), ("disc", &shapes.disc), ("face", &shapes.face)] {
            let mesh = meshes.get(&shared.handle).unwrap();
            let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
            let Some(VertexAttributeValues::Float32x3(n)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
            for t in 0..p.len() / 3 {
                let centroid = (0..3).map(|k| Vec3::from_array(p[t * 3 + k])).sum::<Vec3>() / 3.0;
                let normal = Vec3::from_array(n[t * 3]);
                match name {
                    "sphere" => assert!(normal.dot(centroid) > 0.9 * centroid.length(), "inward sphere face {t}"),
                    _ => assert!(normal.abs_diff_eq(Vec3::NEG_Z, 1e-6), "{name} face {t}: {normal}"),
                }
            }
        }
    }

    #[test]
    fn a_face_is_sized_along_the_emitter() {
        let size = Vec3::new(4.0, 2.0, 6.0);
        for (face, want) in [
            ("Bottom", Vec2::new(4.0, 6.0)),
            ("Top", Vec2::new(4.0, 6.0)),
            ("Front", Vec2::new(4.0, 2.0)),
            ("Back", Vec2::new(4.0, 2.0)),
            ("Right", Vec2::new(6.0, 2.0)),
            ("Left", Vec2::new(6.0, 2.0)),
        ] {
            let got = face_size(face, size);
            assert!(got.abs_diff_eq(want, 1e-5), "{face}: {got}");
        }
    }

    // ---- following the light

    #[test]
    fn proxy_follows_transform_colour_and_intensity() {
        let mut app = app();
        let l = lamp(&mut app, PointLight { intensity: 800.0, radius: 0.5, ..default() }, Vec3::ZERO);
        settle(&mut app);
        let p = proxy_for(&mut app, l).unwrap();
        let area = shapes(&app).sphere.unit_area * 0.25;
        let e0 = emissive(&app, p);
        assert!(close(e0.red, 800.0 / (PI * area), 1e-5), "{} vs {}", e0.red, 800.0 / (PI * area));
        assert_eq!((e0.red, e0.red), (e0.green, e0.blue), "white light, grey emissive");
        assert_eq!(app.world().get::<Transform>(p).unwrap().scale, Vec3::splat(0.5));

        app.world_mut().get_mut::<Transform>(l).unwrap().translation = Vec3::new(4.0, 0.0, -2.0);
        app.update();
        assert_eq!(app.world().get::<Transform>(p).unwrap().translation, Vec3::new(4.0, 0.0, -2.0));
        assert_eq!(app.world().get::<GlobalTransform>(p).unwrap().translation(), Vec3::new(4.0, 0.0, -2.0));
        assert_eq!(proxy_for(&mut app, l), Some(p), "a moved light got a new proxy");

        app.world_mut().get_mut::<PointLight>(l).unwrap().intensity = 1600.0;
        app.update();
        assert!(close(emissive(&app, p).red, 2.0 * e0.red, 1e-6));

        // Bevy lights with the linear colour: sRGB 0.5 is 0.21404 linear.
        app.world_mut().get_mut::<PointLight>(l).unwrap().color = Color::srgb(1.0, 0.5, 0.0);
        app.update();
        let orange = emissive(&app, p);
        assert!(close(orange.red, 2.0 * e0.red, 1e-6), "{orange:?}");
        assert!(close(orange.green, 2.0 * e0.red * 0.214_041_14, 1e-5), "{orange:?}");
        assert_eq!(orange.blue, 0.0);

        app.world_mut().get_mut::<PointLight>(l).unwrap().radius = 1.0;
        app.update();
        assert_eq!(app.world().get::<Transform>(p).unwrap().scale, Vec3::ONE);
        assert!(close(emissive(&app, p).red, 1600.0 / (PI * shapes(&app).sphere.unit_area), 1e-5));
    }

    #[test]
    fn a_turning_spot_turns_its_disc() {
        let mut app = app();
        let s = spot(&mut app, SpotLight { intensity: 500.0, ..default() }, Vec3::ZERO, Vec3::NEG_Y);
        settle(&mut app);
        let p = proxy_for(&mut app, s).unwrap();
        app.world_mut().entity_mut(s).insert(Transform::from_xyz(0.0, 1.0, 0.0).looking_to(Vec3::X, Vec3::Y));
        app.update();
        let t = *app.world().get::<Transform>(p).unwrap();
        assert!((t.rotation * Vec3::NEG_Z - Vec3::X).length() < 1e-5);
        assert!((t.translation - Vec3::new(FACE_OFFSET_M, 1.0, 0.0)).length() < 1e-5);
        // On-axis calibration: L A = Phi / (4 pi).
        let area = shapes(&app).disc.unit_area * MIN_RADIUS_M * MIN_RADIUS_M;
        assert!(close(emissive(&app, p).red * area, 500.0 / (4.0 * PI), 1e-5));
    }

    #[test]
    fn a_resized_part_resizes_its_face() {
        let mut app = app();
        let (part, emitter) = surface_light(&mut app, Vec3::ZERO, Vec3::new(4.0, 2.0, 6.0), "Top");
        settle(&mut app);
        let f = proxy_for(&mut app, emitter).unwrap();
        let size = Vec3::new(2.0, 2.0, 2.0);
        app.world_mut().entity_mut(part).insert((BasePart { size, ..default() }, Transform::from_scale(size)));
        app.update();
        let t = *app.world().get::<Transform>(f).unwrap();
        assert!((t.scale - Vec3::new(2.0, 2.0, 1.0)).length() < 1e-5, "{}", t.scale);
        assert!((t.translation - Vec3::new(0.0, 1.0 + FACE_OFFSET_M, 0.0)).length() < 1e-5, "{}", t.translation);
    }

    #[test]
    fn dark_hidden_and_overlay_lights_have_no_proxy() {
        let mut app = app();
        let l = lamp(&mut app, PointLight { intensity: 800.0, ..default() }, Vec3::ZERO);
        settle(&mut app);
        let first = proxy_for(&mut app, l).unwrap();
        let material = material_of(&app, first);

        // Off (switched off, or dimmed by the culler): the proxy is despawned.
        app.world_mut().get_mut::<PointLight>(l).unwrap().intensity = 0.0;
        app.update();
        assert_eq!(proxy_for(&mut app, l), None);
        assert!(app.world().get_entity(first).is_err(), "an off light's proxy was kept, not despawned");
        app.world_mut().get_mut::<PointLight>(l).unwrap().intensity = 800.0;
        app.update();
        let again = proxy_for(&mut app, l).expect("back on, no proxy");
        assert_eq!(material_of(&app, again), material, "a light keeps its material");

        app.world_mut().entity_mut(l).insert(InheritedVisibility::HIDDEN);
        app.update();
        assert_eq!(proxy_for(&mut app, l), None, "a hidden light is traced");
        app.world_mut().entity_mut(l).insert(InheritedVisibility::VISIBLE);
        app.update();
        assert!(proxy_for(&mut app, l).is_some());

        app.world_mut().entity_mut(l).insert(RenderLayers::layer(31));
        app.update();
        assert_eq!(proxy_for(&mut app, l), None, "an overlay-layer light is traced");
        app.world_mut().entity_mut(l).remove::<RenderLayers>();
        app.update();
        assert!(proxy_for(&mut app, l).is_some(), "back on layer 0, no proxy");

        app.world_mut().get_mut::<PointLight>(l).unwrap().color = Color::BLACK;
        app.update();
        assert_eq!(proxy_for(&mut app, l), None, "a black light is traced");
        app.world_mut().get_mut::<PointLight>(l).unwrap().color = Color::WHITE;
        app.world_mut().get_mut::<PointLight>(l).unwrap().intensity = f32::NAN;
        app.update();
        assert_eq!(proxy_for(&mut app, l), None, "a NaN light is traced");
    }

    #[test]
    fn a_despawned_light_takes_its_proxy_and_material() {
        let mut app = app();
        settle(&mut app);
        let materials_before = app.world().resource::<Assets<StandardMaterial>>().len();
        let l = lamp(&mut app, PointLight { intensity: 800.0, ..default() }, Vec3::ZERO);
        let s = spot(&mut app, SpotLight { intensity: 800.0, ..default() }, Vec3::Y, Vec3::NEG_Y);
        settle(&mut app);
        assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), materials_before + 2);
        app.world_mut().entity_mut(l).despawn();
        app.world_mut().entity_mut(s).remove::<SpotLight>();
        for _ in 0..3 {
            app.update();
        }
        assert!(proxies(&mut app).is_empty(), "a proxy outlived its light");
        assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), materials_before, "material leaked");
        assert_eq!(app.world().resource::<LightProxies>().counts().wanting, 0);
    }

    #[test]
    fn a_proxy_despawned_elsewhere_comes_back() {
        let mut app = app();
        let l = lamp(&mut app, PointLight { intensity: 800.0, ..default() }, Vec3::ZERO);
        settle(&mut app);
        let p = proxy_for(&mut app, l).unwrap();
        app.world_mut().entity_mut(p).despawn();
        for _ in 0..REFRESH_FRAMES {
            app.update();
        }
        assert!(proxy_for(&mut app, l).is_some(), "not respawned by the refresh");
    }

    #[test]
    fn nothing_changes_nothing_is_evaluated() {
        let mut app = app();
        for i in 0..5 {
            lamp(&mut app, PointLight { intensity: 800.0, ..default() }, Vec3::X * i as f32);
        }
        settle(&mut app);
        let before = app.world().resource::<LightProxies>().evaluations();
        // Frames 3 to 12 of the refresh cycle: no change, no refresh.
        for _ in 0..10 {
            app.update();
        }
        assert_eq!(app.world().resource::<LightProxies>().evaluations(), before, "idle frames evaluated lights");
        // The refresh evaluates every light once.
        for _ in 0..REFRESH_FRAMES {
            app.update();
        }
        assert_eq!(app.world().resource::<LightProxies>().evaluations(), before + 5);
    }

    #[test]
    fn nothing_runs_without_ray_tracing() {
        for launch in [
            None,
            Some(RaytracingStatus::Off),
            Some(RaytracingStatus::Unsupported),
            Some(RaytracingStatus::NoDenoiser),
        ] {
            let mut app = bare_app();
            if let Some(status) = launch {
                app.insert_resource(RaytracingLaunch::for_status(status));
            }
            add_systems::<Traced>(&mut app);
            lamp(&mut app, PointLight { intensity: 800.0, ..default() }, Vec3::ZERO);
            settle(&mut app);
            assert!(proxies(&mut app).is_empty(), "{launch:?}: proxies without ray tracing");
            assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0, "{launch:?}: meshes made");
        }
    }

    #[test]
    fn adding_the_systems_twice_runs_them_once() {
        let mut app = app();
        add_systems::<Traced>(&mut app);
        let l = lamp(&mut app, PointLight { intensity: 800.0, ..default() }, Vec3::ZERO);
        settle(&mut app);
        settle(&mut app);
        assert!(proxy_for(&mut app, l).is_some());
    }

    // ---- calibration

    #[test]
    fn calibration_values() {
        // A sphere of radius 0.5 (area pi): 1000 lm at 1000 / pi^2 cd/m².
        assert!(close(luminance_for_flux(1000.0, 4.0 * PI * 0.25), 1000.0 / (PI * PI), 1e-6));
        // That sphere emits its 1000 lm, and 1000 / (4 pi) cd seen from anywhere.
        let l = luminance_for_flux(1000.0, PI);
        assert!(close(PI * l * PI, 1000.0, 1e-6));
        assert!(close(l * PI * 0.25, 1000.0 / (4.0 * PI), 1e-6));
        // A flat 0.5 m² emitter: 1000 / (2 pi) cd/m², 1000 / (4 pi) cd on its axis.
        let l = luminance_for_axis_intensity(1000.0, 0.5);
        assert!(close(l, 1000.0 / (2.0 * PI), 1e-6));
        assert!(close(l * 0.5, 1000.0 / (4.0 * PI), 1e-6));
        // Through the proxy builders, with given unit areas.
        let point = point_proxy(&PointLight { intensity: 1000.0, radius: 0.5, ..default() }, Vec3::ZERO, 4.0 * PI)
            .unwrap();
        assert!(close(point.emissive.red, 1000.0 / (PI * PI), 1e-6));
        let disc = spot_proxy(
            &SpotLight { intensity: 1000.0, radius: 0.5, ..default() },
            Vec3::ZERO,
            Quat::IDENTITY,
            None,
            2.0,
            1.0,
        )
        .unwrap();
        assert!(close(disc.emissive.red, 1000.0 / (4.0 * PI * 0.5), 1e-6));
        let face = spot_proxy(
            &SpotLight { intensity: 1000.0, ..default() },
            Vec3::ZERO,
            Quat::IDENTITY,
            Some(Vec2::new(2.0, 0.05)),
            2.0,
            1.0,
        )
        .unwrap();
        assert!(close(face.emissive.red, 1000.0 / (4.0 * PI * 2.0 * MIN_FACE_SIDE_M), 1e-6), "short side not raised");
        assert_eq!(face.transform.scale, Vec3::new(2.0, MIN_FACE_SIDE_M, 1.0));
    }

    /// Irradiance at `at` (a surface facing `facing`) from a proxy mesh of
    /// luminance `l` posed by `pose`, by Solari's rules: one-sided emission
    /// from flat triangles, the instance matrix applied to positions and
    /// normals. Each triangle is split into 64 pieces.
    fn irradiance(mesh: &Mesh, pose: &Transform, l: f32, at: Vec3, facing: Vec3) -> f32 {
        let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        let Some(VertexAttributeValues::Float32x3(n)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
        let matrix = pose.to_matrix();
        let indices: Vec<usize> = mesh.indices().unwrap().iter().collect();
        const N: usize = 8;
        let mut e = 0.0;
        for t in indices.chunks_exact(3) {
            let w = [t[0], t[1], t[2]].map(|i| matrix.transform_point3(Vec3::from_array(p[i])));
            let normal = matrix.transform_vector3(Vec3::from_array(n[t[0]])).normalize();
            let area = (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5;
            for i in 0..N {
                for j in 0..N - i {
                    for upper in [false, true] {
                        if upper && i + j + 1 >= N {
                            continue;
                        }
                        let k = if upper { 2.0 / 3.0 } else { 1.0 / 3.0 };
                        let (u, v) = ((i as f32 + k) / N as f32, (j as f32 + k) / N as f32);
                        let point = w[0] + (w[1] - w[0]) * u + (w[2] - w[0]) * v;
                        let to = at - point;
                        let d2 = to.length_squared();
                        let dir = to / d2.sqrt();
                        let cos_emit = normal.dot(dir).max(0.0);
                        let cos_recv = facing.dot(-dir).max(0.0);
                        e += l * cos_emit * cos_recv / d2 * area / (N * N) as f32;
                    }
                }
            }
        }
        e
    }

    /// The traced proxy lights a surface as Bevy's raster light does: its
    /// candela, Phi / (4 pi), over the distance squared (the range window
    /// left out), checked by integrating the meshes Solari traces.
    #[test]
    fn traced_irradiance_matches_bevys_light() {
        let mut app = app();
        let lumens = 1200.0;
        let l = lamp(&mut app, PointLight { intensity: lumens, radius: 0.2, ..default() }, Vec3::ZERO);
        let s = spot(&mut app, SpotLight { intensity: lumens, radius: 0.2, ..default() }, Vec3::new(50.0, 0.0, 0.0), Vec3::NEG_Y);
        let (_, f) = surface_light(&mut app, Vec3::new(-50.0, 0.0, 0.0), Vec3::new(0.3, 0.5, 0.25), "Bottom");
        app.world_mut().get_mut::<SpotLight>(f).unwrap().intensity = lumens;
        settle(&mut app);
        let d = 10.0;
        let bevy = lumens / (4.0 * PI) / (d * d);
        let shapes = shapes(&app);
        let cases = [
            // The 80-face sphere's outline differs from its average by under 1%.
            (l, shapes.sphere.handle.clone(), Vec3::ZERO, Vec3::NEG_Y, 0.015),
            (l, shapes.sphere.handle.clone(), Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0).normalize(), 0.015),
            (s, shapes.disc.handle.clone(), Vec3::new(50.0, -FACE_OFFSET_M, 0.0), Vec3::NEG_Y, 0.003),
            (f, shapes.face.handle.clone(), Vec3::new(-50.0, -0.25 - FACE_OFFSET_M, 0.0), Vec3::NEG_Y, 0.003),
        ];
        let mut measured = Vec::new();
        for (light, mesh, origin, toward, tolerance) in cases {
            let p = proxy_for(&mut app, light).unwrap();
            let pose = *app.world().get::<Transform>(p).unwrap();
            assert!((pose.translation - origin).length() < 1e-4, "{light:?} proxy at {}", pose.translation);
            measured.push((light, mesh, pose, emissive(&app, p).red, origin, toward, tolerance));
        }
        let meshes = app.world().resource::<Assets<Mesh>>();
        for (light, mesh, pose, luminance, origin, toward, tolerance) in measured {
            let at = origin + toward * d;
            let e = irradiance(meshes.get(&mesh).unwrap(), &pose, luminance, at, -toward);
            assert!(close(e, bevy, tolerance), "{light:?} toward {toward}: {e} lux traced, {bevy} raster");
        }
    }

    // ---- caps

    #[test]
    fn traceable_only_under_the_triangle_limit_and_visible() {
        assert!(proxy_traceable(true, true, EMISSIVE_TRIANGLE_LIMIT));
        assert!(!proxy_traceable(true, true, EMISSIVE_TRIANGLE_LIMIT + 1));
        assert!(!proxy_traceable(false, true, 80));
        assert!(!proxy_traceable(true, false, 80));
        assert_eq!(LIGHT_SOURCE_LIMIT, 65_535);
    }

    #[test]
    fn past_the_cap_the_nearest_lights_are_traced() {
        let mut app = app();
        app.world_mut().resource_mut::<LightProxies>().proxy_limit = 2;
        let camera = app.world_mut().spawn((Camera3d::default(), Transform::default())).id();
        let near = lamp(&mut app, PointLight { intensity: 800.0, ..default() }, Vec3::new(1.0, 0.0, 0.0));
        let mid = lamp(&mut app, PointLight { intensity: 800.0, ..default() }, Vec3::new(5.0, 0.0, 0.0));
        let far = lamp(&mut app, PointLight { intensity: 800.0, ..default() }, Vec3::new(9.0, 0.0, 0.0));
        settle(&mut app);
        assert!(app.world().resource::<LightProxies>().is_capped());
        assert!(proxy_for(&mut app, near).is_some() && proxy_for(&mut app, mid).is_some());
        assert_eq!(proxy_for(&mut app, far), None);
        assert_eq!(proxies(&mut app).len(), 2);

        // The camera moves to the far light: the next ranking follows it.
        app.world_mut().entity_mut(camera).insert(Transform::from_xyz(10.0, 0.0, 0.0));
        for _ in 0..REFRESH_FRAMES {
            app.update();
        }
        assert!(proxy_for(&mut app, far).is_some() && proxy_for(&mut app, mid).is_some());
        assert_eq!(proxy_for(&mut app, near), None);

        // One light fewer: everyone fits again.
        app.world_mut().get_mut::<PointLight>(mid).unwrap().intensity = 0.0;
        app.update();
        assert!(!app.world().resource::<LightProxies>().is_capped());
        assert!(proxy_for(&mut app, near).is_some() && proxy_for(&mut app, far).is_some());
        assert_eq!(proxies(&mut app).len(), 2);
    }

    #[test]
    fn other_light_sources_shrink_the_cap() {
        let mut app = app();
        {
            let mut state = app.world_mut().resource_mut::<LightProxies>();
            state.source_limit = 8;
            state.source_margin = 0;
        }
        app.world_mut().spawn(DirectionalLight::default());
        // Five traced parts, three of them emissive: four other sources.
        let glow = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial { emissive: LinearRgba::rgb(5.0, 5.0, 5.0), ..default() });
        let plain = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
        let any_mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(unit_face());
        for material in [&glow, &glow, &glow, &plain, &plain] {
            app.world_mut().spawn((Traced(any_mesh.clone()), MeshMaterial3d(material.clone())));
        }
        for i in 0..6 {
            lamp(&mut app, PointLight { intensity: 800.0, ..default() }, Vec3::X * i as f32);
        }
        settle(&mut app);
        assert_eq!(proxies(&mut app).len(), 4, "8 sources, 4 taken: 4 proxies");
        assert!(app.world().resource::<LightProxies>().is_capped());
    }

    // ---- a point light inside its part, and the mesh sync in `super`

    fn quad() -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.5, 0.5, 0.0], [-0.5, 0.5, 0.0]],
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
            .with_inserted_indices(Indices::U16(vec![0, 1, 2, 0, 2, 3]))
    }

    /// The tracing systems of `super` (which add the proxies), with one
    /// drawn part of `size` at `at`.
    fn scene(size: Vec3, at: Vec3) -> (App, Entity) {
        let mut app = bare_app();
        app.insert_resource(RaytracingLaunch::for_status(RaytracingStatus::Active));
        add_tracing_systems::<Traced, Lighting>(&mut app);
        assert!(app.world().contains_resource::<LightProxies>(), "add_tracing_systems does not add the light proxies");
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(quad());
        let material = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
        let part = app
            .world_mut()
            .spawn((
                BasePart { size, ..default() },
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(at).with_scale(size),
                InheritedVisibility::VISIBLE,
            ))
            .id();
        (app, part)
    }

    fn bulb(app: &mut App, part: Entity) -> Entity {
        app.world_mut()
            .spawn((
                PointLight { intensity: 800.0, ..default() },
                Transform::default(),
                InheritedVisibility::VISIBLE,
                ChildOf(part),
            ))
            .id()
    }

    fn run(app: &mut App, frames: usize) {
        for _ in 0..frames {
            app.update();
        }
    }

    #[test]
    fn a_lamp_part_leaves_the_traced_scene_while_its_light_is_in_it() {
        let (mut app, part) = scene(Vec3::splat(0.5), Vec3::new(0.0, 3.0, 0.0));
        settle(&mut app);
        assert!(app.world().get::<Traced>(part).is_some(), "the part is not traced to begin with");
        let light = bulb(&mut app, part);
        run(&mut app, 3);
        assert_eq!(app.world().get::<TraceOverride>(part), Some(&TraceOverride::Never));
        assert!(app.world().get::<ExcludedForLight>(part).is_some());
        assert!(app.world().get::<Traced>(part).is_none(), "the lamp part still blocks its light");
        assert_eq!(app.world().resource::<LightProxies>().counts().hosts_excluded, 1);
        let proxy = proxy_for(&mut app, light).unwrap();
        // The proxy survives the mesh sync and is not one of its meshes.
        assert!(app.world().get::<Traced>(proxy).is_some(), "the mesh sync took the proxy's marker");
        let counts = app.world().resource::<TracedMeshes>().counts();
        assert_eq!(counts.as_is + counts.copied, 1, "proxy meshes went through the mesh sync: {counts:?}");

        app.world_mut().entity_mut(light).despawn();
        run(&mut app, 3);
        assert!(app.world().get::<TraceOverride>(part).is_none(), "the part never came back");
        assert!(app.world().get::<ExcludedForLight>(part).is_none());
        assert!(app.world().get::<Traced>(part).is_some(), "the part is not traced again");
        assert!(proxies(&mut app).is_empty());
    }

    #[test]
    fn a_part_keeps_its_own_override_and_a_big_part_stays() {
        let (mut app, part) = scene(Vec3::splat(0.5), Vec3::ZERO);
        app.world_mut().entity_mut(part).insert(TraceOverride::Always);
        let light = bulb(&mut app, part);
        run(&mut app, 3);
        assert_eq!(app.world().get::<TraceOverride>(part), Some(&TraceOverride::Always));
        assert!(app.world().get::<ExcludedForLight>(part).is_none());
        app.world_mut().entity_mut(light).despawn();
        run(&mut app, 3);
        assert_eq!(
            app.world().get::<TraceOverride>(part),
            Some(&TraceOverride::Always),
            "someone else's override was taken"
        );

        let (mut app, part) = scene(Vec3::new(10.0, 1.0, 1.0), Vec3::ZERO);
        bulb(&mut app, part);
        run(&mut app, 3);
        assert!(app.world().get::<TraceOverride>(part).is_none(), "a 10 m part left the traced scene");
        assert_eq!(app.world().resource::<LightProxies>().counts().in_big_hosts, 1);

        // A light above its part (outside it) needs nothing.
        let (mut app, part) = scene(Vec3::splat(0.5), Vec3::ZERO);
        let light = bulb(&mut app, part);
        app.world_mut().entity_mut(light).insert(Transform::from_xyz(0.0, 2.0, 0.0));
        run(&mut app, 3);
        assert!(app.world().get::<TraceOverride>(part).is_none(), "a light outside its part took it out");
    }
}

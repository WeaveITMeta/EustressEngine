//! The drivable surface of every road: a ribbon mesh and a chain of collision
//! boxes along each enabled `TerrainSpline` in Road mode.
//!
//! The layer bake carves a road's corridor into the terrain at raster
//! resolution (see `layers`). A wheel wants a smoother surface than a
//! heightfield's cells give, so each road also gets one entity holding a
//! quad-strip ribbon a few centimetres above the carved bed and a static
//! compound collider of boxes whose top faces meet the ribbon. Both are laid
//! along the stations the bake carved the corridor along
//! ([`TerrainBaked::baked_spline`]) rather than a second evaluation of the
//! spline: the stations give the XZ line, and their heights are read from the
//! finished bake, so a layer ordered after the road that reshapes the
//! corridor is followed too. They are rebuilt whenever those placed heights
//! change: a point moved, a property edited, a base edit under one of the
//! profile's knots, or a later layer over the corridor. The ribbon carries
//! one height per station, so a later layer that clips only one edge of the
//! road is followed along the centreline only.
//!
//! The ribbon wears the swatch of the road's bed material, the material the
//! bake paints exactly under it, and asphalt grey when the bed is unpainted.
//!
//! The surface is derived state, like the bake: it is not an instance, is
//! never saved and does not show in the Explorer; the spline is what
//! persists. Every Road spline gets one, however it was made (the Studio's
//! road tool, Insert, a hand-written file), in the Studio and the Client
//! alike.
//!
//! ## Designed roads
//! A Road with a `RoadClass` is drawn from its design
//! ([`TerrainBaked::baked_design`]) instead: the class's whole cross-section
//! (carriageway, shoulders, kerbs, sidewalks) and its painted markings, each
//! surface a child of the surface entity with its own material, all laid on
//! the very formation the bake carved [`ROAD_BED_SINK`] under them. The
//! carriageway wears the asphalt textures, the shoulders worn asphalt when
//! paved. Its collider is one triangle mesh of the paved surfaces, crown,
//! banking and kerbs included. A designed road follows its design, not a
//! later layer over its corridor.
//!
//! Beyond the formation it draws what each side of the design does
//! ([`RoadStructurePart`]):
//! - earth wedges down the first metres of every slope, closing the step
//!   the terrain raster leaves at the formation's edge;
//! - retaining walls where no slope reaches the ground;
//! - its bridges (deck, parapets, piers, abutments);
//! - its tunnels (lining and portal frames).
//!
//! These collide as one more child. [`carve_road_tunnels`] opens each tunnel
//! through the terrain's volume near its portals.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::height_query::height_at_world;
use super::layer_instances::{layer_id, TerrainSpline};
use super::layers::{SplineMode, TerrainBaked, ROAD_BED_SINK};
use super::material::MATERIAL_SLOT_NONE;
use super::material_slots::TerrainMaterialSlots;
use super::road::{build_ribbon_mesh, RoadPath, RoadProfile};
use super::road_design::RoadClass;
use super::road_furniture::{junction_furniture, road_streetlights, road_trees, Finish, FurnitureMeshes};
use super::api::{apply_terrain_command, TerrainCommand, TerrainFill};
use super::dirty::TerrainDirtyChunks;
use super::road_junction::{NetworkJunction, CROSSWALK_LENGTH, STOP_BAR_GAP, STOP_BAR_WIDTH};
use super::road_mesh::{
    build_designed_zoned, build_sides, build_sides_zoned, build_structures, tunnel_carves, DesignedRoad, MeshData, RoadZones, Surface,
};
use super::volume::TerrainVolume;
use super::{TerrainConfig, TerrainData, TerrainRoot};
use crate::classes::Instance;

/// How far the ribbon floats above the carved bed, metres: the bed's sink
/// under the profile ([`ROAD_BED_SINK`]) and 5 cm more, so the ribbon stands
/// just over the profile and clears the terrain mesh between raster cells
/// without z-fighting.
pub const ROAD_SURFACE_LIFT: f32 = ROAD_BED_SINK + 0.05;
/// Depth bias of the ribbon's material: pulls it toward the camera by about
/// a tenth of a percent of its distance (32-bit depth), which settles the
/// fight with a far terrain mesh simplified under it and never lets the road
/// show through a hill in front of it.
const ROAD_SURFACE_DEPTH_BIAS: f32 = 1.0e4;
/// Thickness of each collision box, metres. Its top face meets the ribbon
/// and the rest sits in the ground.
pub const ROAD_SURFACE_THICKNESS: f32 = 0.3;
/// sRGB of a ribbon whose bed is left unpainted: asphalt grey.
pub const ROAD_SURFACE_DEFAULT_SRGB: [f32; 3] = [0.16, 0.16, 0.18];
/// How far a designed road's meshes stand above its design, metres: the
/// carved formation is [`ROAD_BED_SINK`] below that, so they clear it.
pub const DESIGNED_SURFACE_LIFT: f64 = 0.02;
/// Metres of road one tile of the asphalt textures covers.
const ASPHALT_TILE_METRES: f32 = 4.0;
/// Paint stands a centimetre over the carriageway; its larger bias keeps it
/// on top at a distance.
const MARKING_DEPTH_BIAS: f32 = 2.0e4;
/// sRGB of kerbs, sidewalks and the two paints.
const KERB_SRGB: [f32; 3] = [0.62, 0.62, 0.60];
const SIDEWALK_SRGB: [f32; 3] = [0.52, 0.52, 0.50];
const MARKING_WHITE_SRGB: [f32; 3] = [0.92, 0.92, 0.90];
const MARKING_YELLOW_SRGB: [f32; 3] = [0.95, 0.72, 0.10];

/// Marks the surface entity built for the road spline `spline`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadSurface {
    pub spline: Entity,
}

/// Marks one surface of a designed road (its carriageway, a shoulder, the
/// markings), a child of the road's [`RoadSurface`] entity.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadSurfacePart {
    pub surface: Surface,
}

/// What a designed road's structure part is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoadStructureKind {
    /// Earth wedges down the first metres of its slopes.
    Wedge,
    /// Retaining walls.
    Wall,
    /// Bridge decks, parapets, piers and abutments, and tunnel portals.
    Concrete,
    /// The inside of a tunnel.
    Lining,
}

impl RoadStructureKind {
    /// The colour it is drawn in, sRGB.
    pub fn srgb(self) -> [f32; 3] {
        match self {
            RoadStructureKind::Wedge => [0.44, 0.40, 0.33],
            RoadStructureKind::Wall | RoadStructureKind::Concrete => [0.60, 0.59, 0.56],
            RoadStructureKind::Lining => [0.38, 0.38, 0.40],
        }
    }
}

/// Marks one structure part of a designed road (a wedge, wall, bridge or
/// tunnel mesh), a child of the road's [`RoadSurface`] entity.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadStructurePart {
    pub kind: RoadStructureKind,
}

/// What a road's surface entity was last built from.
#[derive(Debug)]
struct BuiltSurface {
    surface: Entity,
    /// The stations as laid on the finished surface.
    stations: Vec<Vec3>,
    half_width: f32,
    /// The bed material slot the ribbon wears, `None` when unpainted.
    bed: Option<u8>,
    /// sRGB the ribbon was drawn in.
    color: [f32; 3],
    /// A designed road's design, and the shoulder colour it was drawn with.
    design: Option<(DesignedRoad, Option<[f32; 3]>, RoadZones)>,
    /// A designed road's surface entities, children of `surface`.
    parts: Vec<Entity>,
}

/// How one surface of a designed road looks, for sharing its material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PartLook {
    Asphalt,
    WornAsphalt,
    /// An unpaved shoulder's gravel (the granite aggregate texture).
    Gravel,
    /// Grassed earth: the slopes down from a road's edge.
    Grass,
    /// Cast concrete: bridges, retaining walls, portals.
    Concrete,
    /// An untextured colour (sRGB bits) with this depth bias (bits).
    Plain([u32; 3], u32),
}

/// The surface built for each road spline, keyed by the spline's entity, and
/// the ribbon materials, one per colour, keyed by the colour's bits so two
/// slots with the same swatch share one and a redefined slot gets a new one.
#[derive(Resource, Debug, Default)]
pub struct RoadSurfaces {
    built: HashMap<Entity, BuiltSurface>,
    materials: HashMap<[u32; 3], Handle<StandardMaterial>>,
    /// Designed roads' materials, one per look.
    part_materials: HashMap<PartLook, Handle<StandardMaterial>>,
    /// The junction surfaces built, each with its junction's fingerprint.
    junctions: Vec<(u64, Entity)>,
    /// Something changed that has not been looked at yet: the bake was
    /// waiting on a replaced layer list when it was noticed.
    pending: bool,
}

impl RoadSurfaces {
    /// The surface entity of road spline `spline`, if one is built.
    pub fn surface_of(&self, spline: Entity) -> Option<Entity> {
        self.built.get(&spline).map(|built| built.surface)
    }
}

/// Keep one surface per enabled Road spline, matching the stations its
/// corridor was baked along (see the module docs). Runs after
/// `apply_terrain_dirty_chunks`, which re-bakes, so a moved point's surface
/// follows in the frame its corridor does.
#[allow(clippy::too_many_arguments)]
pub fn sync_road_surfaces(
    mut commands: Commands,
    mut surfaces: ResMut<RoadSurfaces>,
    roots: Query<(&TerrainConfig, Ref<TerrainBaked>), With<TerrainRoot>>,
    splines: Query<(Entity, Option<&Instance>), With<TerrainSpline>>,
    // A spline respawned as it was (a Space rescan) bakes to the same layer
    // list, so the bake does not change; its new entity still needs a
    // surface.
    touched: Query<(), Changed<TerrainSpline>>,
    live: Query<(), With<RoadSurface>>,
    live_junctions: Query<(), With<RoadJunctionSurface>>,
    mut removed: RemovedComponents<TerrainSpline>,
    mut meshes: Option<ResMut<Assets<Mesh>>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    // Optional: a host without the slot table draws every ribbon grey.
    slots: Option<Res<TerrainMaterialSlots>>,
    // Optional: a host without assets draws designed roads untextured.
    asset_server: Option<Res<AssetServer>>,
) {
    let spline_removed = !removed.is_empty();
    removed.clear();
    let root = roots.iter().next();
    // Read through `Deref` first so an idle frame leaves the resource alone.
    let lost = surfaces.built.values().any(|built| !live.contains(built.surface));
    let noticed = spline_removed
        || lost
        || !touched.is_empty()
        || root.as_ref().is_some_and(|(_, baked)| baked.is_changed())
        || (root.is_none() && !surfaces.built.is_empty())
        || slots.as_ref().is_some_and(|s| s.is_changed());
    if !noticed && !surfaces.pending {
        return;
    }
    let surfaces = &mut *surfaces;

    // A surface whose spline is gone goes at once, bake or no bake.
    surfaces.built.retain(|spline, built| {
        let keep = splines.contains(*spline);
        if !keep {
            commands.entity(built.surface).try_despawn();
        }
        keep
    });

    let Some((config, baked)) = root else {
        // No bake means no layers, so no roads.
        for (_, built) in surfaces.built.drain() {
            commands.entity(built.surface).try_despawn();
        }
        for (_, junction) in surfaces.junctions.drain(..) {
            commands.entity(junction).try_despawn();
        }
        surfaces.pending = false;
        return;
    };
    // A replaced layer list is laid only by the next bake; until then the
    // stations are not there to read, which is not the same as no road.
    if baked.wants_bake() {
        surfaces.pending = true;
        return;
    }
    surfaces.pending = false;

    let mut kept: HashMap<Entity, BuiltSurface> = HashMap::with_capacity(surfaces.built.len());
    for (spline_entity, instance) in &splines {
        let id = layer_id(instance.map(|i| i.uuid.as_str()), spline_entity);
        let Some((spline, stations)) = baked.baked_spline(id) else { continue };
        let half_width = (spline.width * 0.5).max(0.0);
        if spline.mode != SplineMode::Road || !(half_width > 0.0) {
            continue;
        }
        let bed = spline.bed_material.filter(|s| *s != MATERIAL_SLOT_NONE);
        // The ribbon covers exactly the band the bake paints with the bed
        // material, so it wears that material's swatch; a bed left
        // unpainted keeps asphalt grey.
        let color = match (bed, slots.as_deref()) {
            (Some(slot), Some(slots)) => slots.swatch_srgb(slot),
            _ => ROAD_SURFACE_DEFAULT_SRGB,
        };
        if let Some(design) = baked.baked_design(id) {
            // Unpaved shoulders wear the shoulder material's swatch, or gravel.
            let shoulder = match (spline.shoulder_material.filter(|s| *s != MATERIAL_SLOT_NONE), slots.as_deref()) {
                (Some(slot), Some(slots)) => Some(slots.swatch_srgb(slot)),
                _ => None,
            };
            // Where the junctions it meets cover or paint it.
            let zones = baked.prepared().map_or_else(RoadZones::default, |p| junction_zones(id, p.junctions()));
            let previous = surfaces.built.remove(&spline_entity);
            let reuse = previous.as_ref().map(|built| built.surface).filter(|surface| live.contains(*surface));
            let unchanged = previous.as_ref().is_some_and(|built| {
                built.design.as_ref().is_some_and(|(d, s, z)| d == design && *s == shoulder && *z == zones)
                    && built.bed == bed
                    && built.color == color
            });
            if unchanged && reuse.is_some() {
                kept.extend(previous.map(|built| (spline_entity, built)));
                continue;
            }
            for part in previous.iter().flat_map(|built| built.parts.iter()) {
                commands.entity(*part).try_despawn();
            }
            let surface = match reuse {
                Some(surface) => surface,
                None => commands
                    .spawn((
                        Name::new("RoadSurface"),
                        RoadSurface { spline: spline_entity },
                        Transform::IDENTITY,
                        Visibility::default(),
                    ))
                    .id(),
            };
            let parts = build_designed_surface(
                &mut commands,
                surface,
                design,
                &zones,
                DesignedLooks { bed, color, shoulder },
                config.seed,
                meshes.as_deref_mut(),
                materials.as_deref_mut(),
                asset_server.as_deref(),
                &mut surfaces.part_materials,
            );
            kept.insert(
                spline_entity,
                BuiltSurface {
                    surface,
                    stations: stations.to_vec(),
                    half_width,
                    bed,
                    color,
                    design: Some((design.clone(), shoulder, zones)),
                    parts,
                },
            );
            continue;
        }
        // The stations on the finished surface. The layers ordered after the
        // road are what the bake applied over its carve; a station off the
        // raster keeps its profile, as the sampler would clamp it to the
        // footprint's edge, and so does every station of a terrain with no
        // raster at all, which the bake cannot carve.
        let (lo, hi) = config.footprint_xz();
        let has_raster = !baked.data.height_cache.is_empty();
        let placed: Vec<Vec3> = stations
            .iter()
            .map(|s| {
                let on_raster = has_raster && s.x >= lo.x && s.x <= hi.x && s.z >= lo.y && s.z <= hi.y;
                let y = if on_raster { height_at_world(config, &baked.data, s.x, s.z) } else { s.y };
                Vec3::new(s.x, if y.is_finite() { y } else { s.y }, s.z)
            })
            .collect();
        let previous = surfaces.built.remove(&spline_entity);
        let reuse = previous.as_ref().map(|built| built.surface).filter(|surface| live.contains(*surface));
        let unchanged = previous.as_ref().is_some_and(|built| {
            built.design.is_none()
                && built.half_width == half_width
                && built.stations.as_slice() == placed.as_slice()
                && built.bed == bed
                && built.color == color
        });
        if unchanged && reuse.is_some() {
            kept.extend(previous.map(|built| (spline_entity, built)));
            continue;
        }
        // A road that was designed loses its surfaces for the ribbon.
        for part in previous.iter().flat_map(|built| built.parts.iter()) {
            commands.entity(*part).try_despawn();
        }
        let Some(path) = RoadPath::from_positions(&placed) else {
            if let Some(surface) = reuse {
                commands.entity(surface).try_despawn();
            }
            continue;
        };
        let profile = RoadProfile { half_width, shoulder_falloff: spline.shoulder_width.max(0.0) };
        let surface = match reuse {
            Some(surface) => surface,
            None => commands
                .spawn((
                    Name::new("RoadSurface"),
                    RoadSurface { spline: spline_entity },
                    Transform::IDENTITY,
                    Visibility::default(),
                ))
                .id(),
        };
        build_surface(
            &mut commands.entity(surface),
            &path,
            profile,
            color,
            meshes.as_deref_mut(),
            materials.as_deref_mut(),
            &mut surfaces.materials,
        );
        kept.insert(
            spline_entity,
            BuiltSurface { surface, stations: placed, half_width, bed, color, design: None, parts: Vec::new() },
        );
    }

    // Whatever was built and is not wanted any more: a spline disabled,
    // switched out of Road mode, or left with fewer than two points.
    for (_, built) in surfaces.built.drain() {
        commands.entity(built.surface).try_despawn();
    }
    surfaces.built = kept;

    // Junction surfaces: one per junction of the designed roads, rebuilt
    // only when the junction changes.
    let wanted: Vec<(u64, &NetworkJunction)> = baked
        .prepared()
        .map(|p| p.junctions().map(|j| (junction_fingerprint(j), j)).collect())
        .unwrap_or_default();
    surfaces.junctions.retain(|(fingerprint, entity)| {
        let keep = live_junctions.contains(*entity) && wanted.iter().any(|(w, _)| w == fingerprint);
        if !keep {
            commands.entity(*entity).try_despawn();
        }
        keep
    });
    for (fingerprint, junction) in wanted {
        if surfaces.junctions.iter().any(|(f, _)| *f == fingerprint) {
            continue;
        }
        let entity = build_junction_surface(
            &mut commands,
            junction,
            meshes.as_deref_mut(),
            materials.as_deref_mut(),
            asset_server.as_deref(),
            &mut surfaces.part_materials,
        );
        surfaces.junctions.push((fingerprint, entity));
    }
}

/// Marks the surface entity of one junction of the designed roads.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadJunctionSurface;

/// One finish of a road's or a junction's furniture (poles, signal heads,
/// signs, luminaires), a child of its surface.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadFurniturePart {
    pub finish: Finish,
}

/// A streetlight's lamp: a spot light, a child of its road's surface.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadLamp;

/// A streetlight lamp's output (lumens, a modern LED cobra head), reach and
/// cone, radians from its axis.
const LAMP_LUMENS: f32 = 9_000.0;
const LAMP_RANGE: f32 = 28.0;
const LAMP_INNER: f32 = 0.55;
const LAMP_OUTER: f32 = 1.05;
/// A lamp's warm white (about 3000 K), sRGB.
const LAMP_SRGB: [f32; 3] = [1.0, 0.86, 0.68];

/// sRGB of each furniture finish.
fn finish_srgb(f: Finish) -> [f32; 3] {
    match f {
        Finish::Steel => [0.42, 0.43, 0.44],
        Finish::SignalHousing => [0.13, 0.12, 0.05],
        Finish::LensRed => [0.78, 0.07, 0.05],
        Finish::LensAmber => [0.80, 0.50, 0.05],
        Finish::LensGreen => [0.08, 0.62, 0.30],
        Finish::SignRed => [0.70, 0.04, 0.04],
        Finish::SignWhite => [0.94, 0.94, 0.92],
        Finish::Luminaire => [0.78, 0.78, 0.74],
        Finish::Bark => [0.30, 0.22, 0.15],
        Finish::Foliage => [0.24, 0.42, 0.18],
    }
}

/// Spawn `f`'s furniture under `parent`: one child mesh per finish (the
/// steel one also its collider, so a car meets a pole) and a spot light per
/// lamp, shadows off. Returns the children.
fn spawn_furniture(
    commands: &mut Commands,
    parent: Entity,
    f: &FurnitureMeshes,
    meshes: Option<&mut Assets<Mesh>>,
    materials: Option<&mut Assets<StandardMaterial>>,
    asset_server: Option<&AssetServer>,
    cache: &mut HashMap<PartLook, Handle<StandardMaterial>>,
) -> Vec<Entity> {
    let mut children = Vec::new();
    if let (Some(meshes), Some(materials)) = (meshes, materials) {
        for (finish, data) in f.parts.iter().filter(|(_, d)| d.triangle_count() > 0) {
            let look = PartLook::Plain(finish_srgb(*finish).map(f32::to_bits), 0f32.to_bits());
            let material = cache.entry(look).or_insert_with(|| materials.add(part_material(look, asset_server))).clone();
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone());
            mesh.insert_indices(Indices::U32(data.indices.clone()));
            let mut child = commands.spawn((
                Name::new("RoadFurniturePart"),
                RoadFurniturePart { finish: *finish },
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                Visibility::default(),
                ChildOf(parent),
            ));
            #[cfg(feature = "physics")]
            if *finish == Finish::Steel {
                use avian3d::prelude::{Collider, RigidBody, TrimeshFlags};
                let vertices: Vec<Vec3> = data.positions.iter().map(|p| Vec3::from_array(*p)).collect();
                let triangles: Vec<[u32; 3]> = data.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
                if let Ok(collider) = Collider::try_trimesh_with_config(vertices, triangles, TrimeshFlags::DELETE_DEGENERATE_TRIANGLES) {
                    child.insert((RigidBody::Static, collider));
                }
            }
            children.push(child.id());
        }
    }
    for lamp in &f.lamps {
        let p = Vec3::new(lamp.position[0] as f32, lamp.position[1] as f32, lamp.position[2] as f32);
        let d = Vec3::new(lamp.direction[0] as f32, lamp.direction[1] as f32, lamp.direction[2] as f32).normalize_or(Vec3::NEG_Y);
        // An up that is never along the beam.
        let up = if d.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        let child = commands
            .spawn((
                Name::new("RoadLamp"),
                RoadLamp,
                SpotLight {
                    color: Color::srgb(LAMP_SRGB[0], LAMP_SRGB[1], LAMP_SRGB[2]),
                    intensity: LAMP_LUMENS,
                    range: LAMP_RANGE,
                    radius: 0.15,
                    shadow_maps_enabled: false,
                    inner_angle: LAMP_INNER,
                    outer_angle: LAMP_OUTER,
                    ..default()
                },
                Transform::from_translation(p).looking_to(d, up),
                Visibility::default(),
                ChildOf(parent),
            ))
            .id();
        children.push(child);
    }
    children
}

/// Length of solid centre line on each approach to a junction, metres.
const JUNCTION_SOLID: f64 = 30.0;

/// Where road `id`'s junctions cover or paint it: over each leg, the
/// junction's box (from the meeting point out to the leg's setback) is a
/// gap; past the box, its crosswalk and stop bar are bare; and the next
/// [`JUNCTION_SOLID`] metres are no-passing.
pub fn junction_zones<'a>(id: u64, junctions: impl Iterator<Item = &'a NetworkJunction>) -> RoadZones {
    let mut zones = RoadZones::default();
    for j in junctions {
        for (k, leg) in j.legs.iter().enumerate().filter(|(_, l)| l.road == id) {
            let setback = j.junction.setbacks.get(k).copied().unwrap_or(0.0);
            let marked = j.junction.crosswalks.get(k).copied().unwrap_or(false) || j.junction.stops.get(k).copied().unwrap_or(false);
            let bare = if marked { CROSSWALK_LENGTH + STOP_BAR_GAP + STOP_BAR_WIDTH + 0.3 } else { 0.5 };
            let dir = if leg.outward { 1.0 } else { -1.0 };
            let at = |d: f64| leg.s + dir * d;
            zones.gaps.push((at(0.0).min(at(setback)), at(0.0).max(at(setback))));
            zones.bare.push((at(setback).min(at(setback + bare)), at(setback).max(at(setback + bare))));
            let far = setback + bare + JUNCTION_SOLID;
            zones.no_passing.push((at(setback).min(at(far)), at(setback).max(at(far))));
        }
    }
    zones
}

/// A fingerprint of a junction's place and legs, to rebuild it only when it changes.
fn junction_fingerprint(j: &NetworkJunction) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for v in [j.centre.x, j.centre.z, j.elevation] {
        ((v * 100.0).round() as i64).hash(&mut h);
    }
    for leg in &j.legs {
        leg.road.hash(&mut h);
        ((leg.s * 100.0).round() as i64).hash(&mut h);
        ((leg.heading * 1000.0).round() as i64).hash(&mut h);
    }
    // The roads' surfaces it is draped over, to the millimetre.
    for surface in &j.surfaces {
        for v in surface.heights.iter().flatten().chain(&surface.offsets) {
            ((v * 1000.0).round() as i64).hash(&mut h);
        }
    }
    h.finish()
}

/// Spawn junction `j`'s surface: its box, kerb returns, sidewalks,
/// crosswalks and stop bars, each surface a child with its material, and
/// one collider of the box, kerbs and sidewalks.
fn build_junction_surface(
    commands: &mut Commands,
    j: &NetworkJunction,
    meshes: Option<&mut Assets<Mesh>>,
    materials: Option<&mut Assets<StandardMaterial>>,
    asset_server: Option<&AssetServer>,
    cache: &mut HashMap<PartLook, Handle<StandardMaterial>>,
) -> Entity {
    let built = j.meshes(DESIGNED_SURFACE_LIFT);
    let root = commands
        .spawn((Name::new("RoadJunction"), RoadJunctionSurface, Transform::IDENTITY, Visibility::default()))
        .id();
    let mut meshes = meshes;
    let mut materials = materials;
    let furniture = junction_furniture(&j.junction, &j.surfaces);
    spawn_furniture(commands, root, &furniture, meshes.as_deref_mut(), materials.as_deref_mut(), asset_server, cache);
    if let (Some(meshes), Some(materials)) = (meshes, materials) {
        for (kind, data) in built.parts.iter().filter(|(_, d)| d.triangle_count() > 0) {
            let plain = |srgb: [f32; 3], bias: f32| PartLook::Plain(srgb.map(f32::to_bits), bias.to_bits());
            let look = match kind {
                Surface::Carriageway | Surface::Shoulder => PartLook::Asphalt,
                Surface::Kerb => plain(KERB_SRGB, ROAD_SURFACE_DEPTH_BIAS),
                Surface::Sidewalk => plain(SIDEWALK_SRGB, ROAD_SURFACE_DEPTH_BIAS),
                Surface::MarkingWhite => plain(MARKING_WHITE_SRGB, MARKING_DEPTH_BIAS),
                Surface::MarkingYellow => plain(MARKING_YELLOW_SRGB, MARKING_DEPTH_BIAS),
            };
            let material = cache.entry(look).or_insert_with(|| materials.add(part_material(look, asset_server))).clone();
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone());
            mesh.insert_indices(Indices::U32(data.indices.clone()));
            if matches!(look, PartLook::Asphalt) {
                if let Err(error) = mesh.generate_tangents() {
                    tracing::debug!("road junction: no tangents for the asphalt ({error})");
                }
            }
            commands.spawn((
                Name::new("RoadJunctionPart"),
                RoadSurfacePart { surface: *kind },
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                Visibility::default(),
                ChildOf(root),
            ));
        }
    }
    #[cfg(feature = "physics")]
    {
        use avian3d::prelude::{Collider, RigidBody, TrimeshFlags};
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        for (_, data) in built.parts.iter().filter(|(k, _)| !is_marking(*k)) {
            let first = vertices.len() as u32;
            vertices.extend(data.positions.iter().map(|p| Vec3::from_array(*p)));
            triangles.extend(data.indices.chunks_exact(3).map(|t| [first + t[0], first + t[1], first + t[2]]));
        }
        if !triangles.is_empty() {
            let flags = TrimeshFlags::FIX_INTERNAL_EDGES | TrimeshFlags::DELETE_DEGENERATE_TRIANGLES;
            let collider = Collider::try_trimesh_with_config(vertices.clone(), triangles.clone(), flags)
                .or_else(|_| Collider::try_trimesh_with_config(vertices, triangles, TrimeshFlags::empty()));
            if let Ok(collider) = collider {
                commands.entity(root).try_insert((RigidBody::Static, collider));
            }
        }
    }
    root
}

/// The bore carves applied for each designed road, keyed by its layer id:
/// a fingerprint of the boxes, so a road is carved once per change of its
/// tunnels rather than every frame.
#[derive(Resource, Debug, Default)]
pub struct RoadTunnelCarves {
    applied: HashMap<u64, u64>,
}

/// Open every designed road's tunnels through the terrain: carve the boxes
/// of [`tunnel_carves`] (near each portal, where the mountain's surface comes
/// down to the bore) out of the root's volume as air, and mark what they
/// changed for remeshing. The heightfield is left whole; the volume opens the
/// portal faces and the stretch under thin cover, and its collider follows.
/// A road's carves run again only when its tunnels change. Carves of a
/// tunnel that moved stay until the volume is edited back.
pub fn carve_road_tunnels(
    mut carved: ResMut<RoadTunnelCarves>,
    mut roots: Query<
        (Entity, &TerrainConfig, &mut TerrainData, Option<&mut TerrainVolume>, Ref<TerrainBaked>),
        With<TerrainRoot>,
    >,
    splines: Query<(Entity, Option<&Instance>), With<TerrainSpline>>,
    dirty: Option<ResMut<TerrainDirtyChunks>>,
    mut commands: Commands,
) {
    use std::hash::{Hash, Hasher};
    let Some((root, config, mut data, volume, baked)) = roots.iter_mut().next() else {
        return;
    };
    if !baked.is_changed() || baked.wants_bake() {
        return;
    }
    let Some(mut volume) = volume else {
        // A root without a volume gets an empty one; the carve runs next frame.
        commands.entity(root).try_insert(TerrainVolume::new());
        return;
    };
    let mut dirty = dirty;
    for (spline, instance) in &splines {
        let id = layer_id(instance.map(|i| i.uuid.as_str()), spline);
        let Some(design) = baked.baked_design(id) else { continue };
        let carves = tunnel_carves(design);
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for c in &carves {
            for v in c.center.iter().chain(&c.along).chain(&c.size) {
                ((v * 100.0).round() as i64).hash(&mut hasher);
            }
        }
        let fingerprint = hasher.finish();
        if carves.is_empty() || carved.applied.get(&id) == Some(&fingerprint) {
            continue;
        }
        for c in &carves {
            let v = |a: [f64; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
            let rotation = Quat::from_mat3(&Mat3::from_cols(v(c.across), v(c.up), v(c.along))).normalize();
            let cmd = TerrainCommand::FillBlock {
                center: v(c.center),
                rotation,
                size: v(c.size),
                fill: TerrainFill::Air,
            };
            match apply_terrain_command(config, &mut data, &mut volume, None, &cmd) {
                Ok(effect) => {
                    if let (Some(edit), Some(dirty)) = (&effect.volume_edit, dirty.as_deref_mut()) {
                        dirty.mark_volume_edit(config, edit);
                    }
                }
                Err(error) => tracing::warn!("road tunnel: a bore carve was refused ({error})"),
            }
        }
        carved.applied.insert(id, fingerprint);
    }
}

/// Give `surface` the ribbon mesh, drawn in sRGB `color`, and the collider of
/// `path`, replacing any it had. The ribbon and every box sit in world space:
/// the entity stays at the identity transform. `materials_cache` holds one
/// ribbon material per colour.
fn build_surface(
    surface: &mut EntityCommands,
    path: &RoadPath,
    profile: RoadProfile,
    color: [f32; 3],
    meshes: Option<&mut Assets<Mesh>>,
    materials: Option<&mut Assets<StandardMaterial>>,
    materials_cache: &mut HashMap<[u32; 3], Handle<StandardMaterial>>,
) {
    // A host without the render assets still gets the collider.
    if let (Some(meshes), Some(materials)) = (meshes, materials) {
        let ribbon = build_ribbon_mesh(path, profile, ROAD_SURFACE_LIFT);
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, ribbon.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, ribbon.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, ribbon.uvs);
        mesh.insert_indices(Indices::U32(ribbon.indices));
        let material = materials_cache
            .entry(color.map(f32::to_bits))
            .or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: Color::srgb(color[0], color[1], color[2]),
                    perceptual_roughness: 0.85,
                    depth_bias: ROAD_SURFACE_DEPTH_BIAS,
                    ..default()
                })
            })
            .clone();
        // The old mesh asset goes with the handle this replaces.
        surface.try_insert((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material)));
    }

    #[cfg(feature = "physics")]
    {
        use super::road::ribbon_segments;
        use avian3d::prelude::{Collider, RigidBody};
        // Avian's `cuboid` takes full side lengths.
        let boxes: Vec<(Vec3, Quat, Collider)> = ribbon_segments(path, profile, ROAD_SURFACE_LIFT, ROAD_SURFACE_THICKNESS)
            .into_iter()
            .map(|segment| (segment.center, segment.rotation, Collider::cuboid(segment.size.x, segment.size.y, segment.size.z)))
            .collect();
        if boxes.is_empty() {
            surface.try_remove::<Collider>();
        } else {
            surface.try_insert((RigidBody::Static, Collider::compound(boxes)));
        }
    }
}

/// The colours a designed road is drawn with: its bed slot and swatch, and
/// the swatch its unpaved shoulders wear.
#[derive(Clone, Copy, Debug)]
struct DesignedLooks {
    bed: Option<u8>,
    color: [f32; 3],
    /// The shoulder material's swatch, if the spline paints its shoulders.
    shoulder: Option<[f32; 3]>,
}

/// Give `surface` the meshes and collider of `design`: one child per surface
/// of its cross-section and per paint, and one triangle-mesh collider of the
/// paved surfaces. Returns the children. The surface entity itself loses any
/// ribbon it had.
#[allow(clippy::too_many_arguments)]
fn build_designed_surface(
    commands: &mut Commands,
    surface: Entity,
    design: &DesignedRoad,
    zones: &RoadZones,
    looks: DesignedLooks,
    seed: u32,
    mut meshes: Option<&mut Assets<Mesh>>,
    mut materials: Option<&mut Assets<StandardMaterial>>,
    asset_server: Option<&AssetServer>,
    cache: &mut HashMap<PartLook, Handle<StandardMaterial>>,
) -> Vec<Entity> {
    let built = build_designed_zoned(design, DESIGNED_SURFACE_LIFT, zones);
    // Unpaved roads and driveways carry no paint.
    let painted = !matches!(design.standard.class, RoadClass::Unpaved | RoadClass::Driveway);
    let parts: Vec<&(Surface, MeshData)> = built
        .parts
        .iter()
        .filter(|(kind, mesh)| mesh.triangle_count() > 0 && (painted || !is_marking(*kind)))
        .collect();
    commands.entity(surface).try_remove::<(Mesh3d, MeshMaterial3d<StandardMaterial>)>();

    let mut children = Vec::new();
    if let (Some(meshes), Some(materials)) = (meshes.as_deref_mut(), materials.as_deref_mut()) {
        for (kind, data) in &parts {
            let look = part_look(*kind, design, looks);
            let material = cache
                .entry(look)
                .or_insert_with(|| materials.add(part_material(look, asset_server)))
                .clone();
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone());
            mesh.insert_indices(Indices::U32(data.indices.clone()));
            if matches!(look, PartLook::Asphalt | PartLook::WornAsphalt) {
                // The asphalt's normal map needs tangents; without them it
                // draws flat, which is still a road.
                if let Err(error) = mesh.generate_tangents() {
                    tracing::debug!("road surface: no tangents for the asphalt ({error})");
                }
            }
            let child = commands
                .spawn((
                    Name::new("RoadSurfacePart"),
                    RoadSurfacePart { surface: *kind },
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material),
                    Transform::IDENTITY,
                    Visibility::default(),
                    ChildOf(surface),
                ))
                .id();
            children.push(child);
        }
    }

    // Beyond the formation: the earth wedges down each slope, the retaining
    // walls, and the concrete and linings of bridges and tunnels. They collide
    // as one more child, apart from the carriageway, whose collider keeps its
    // internal-edge smoothing (these two-faced walls would break it).
    let sides = build_sides_zoned(design, DESIGNED_SURFACE_LIFT, zones);
    let structures = build_structures(design, DESIGNED_SURFACE_LIFT);
    let extras: [(RoadStructureKind, &MeshData); 4] = [
        (RoadStructureKind::Wedge, &sides.wedges),
        (RoadStructureKind::Wall, &sides.walls),
        (RoadStructureKind::Concrete, &structures.concrete),
        (RoadStructureKind::Lining, &structures.lining),
    ];
    let mut extra_collider: Option<Entity> = None;
    for (kind, data) in extras.iter().filter(|(_, data)| data.triangle_count() > 0) {
        let mut entity = commands.spawn((
            Name::new("RoadStructurePart"),
            RoadStructurePart { kind: *kind },
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(surface),
        ));
        if let (Some(meshes), Some(materials)) = (meshes.as_deref_mut(), materials.as_deref_mut()) {
            let look = match kind {
                RoadStructureKind::Wedge => PartLook::Grass,
                RoadStructureKind::Wall | RoadStructureKind::Concrete => PartLook::Concrete,
                RoadStructureKind::Lining => PartLook::Plain(kind.srgb().map(f32::to_bits), ROAD_SURFACE_DEPTH_BIAS.to_bits()),
            };
            let material = cache.entry(look).or_insert_with(|| materials.add(part_material(look, asset_server))).clone();
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone());
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs.clone());
            if *kind == RoadStructureKind::Wedge {
                // Earth wedges are ground: they wear the terrain's surface
                // material (its material map, textures and lighting) whenever
                // the chunks do, shaded as the heightfield is, and their grass
                // look only until then.
                let colors: Vec<[f32; 4]> = data
                    .positions
                    .iter()
                    .zip(&data.normals)
                    .map(|(p, n)| crate::terrain::mesh::draped_vertex_color(Vec3::from_array(*n), p[0], p[2], seed))
                    .collect();
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
                entity.insert(crate::terrain::surface_material::TerrainDraped { fallback: material.clone() });
            }
            mesh.insert_indices(Indices::U32(data.indices.clone()));
            entity.insert((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material)));
        }
        let id = entity.id();
        extra_collider.get_or_insert(id);
        children.push(id);
    }

    // Streetlights and street trees along a kerbed road, clear of its junction boxes.
    let lights = road_streetlights(design, zones, DESIGNED_SURFACE_LIFT);
    children.extend(spawn_furniture(commands, surface, &lights, meshes.as_deref_mut(), materials.as_deref_mut(), asset_server, cache));
    let trees = road_trees(design, zones, DESIGNED_SURFACE_LIFT);
    children.extend(spawn_furniture(commands, surface, &trees, meshes.as_deref_mut(), materials.as_deref_mut(), asset_server, cache));

    #[cfg(feature = "physics")]
    if let Some(holder) = extra_collider {
        use avian3d::prelude::{Collider, TrimeshFlags};
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        for (_, data) in &extras {
            let first = vertices.len() as u32;
            vertices.extend(data.positions.iter().map(|p| Vec3::from_array(*p)));
            triangles.extend(data.indices.chunks_exact(3).map(|t| [first + t[0], first + t[1], first + t[2]]));
        }
        if let Ok(collider) = Collider::try_trimesh_with_config(vertices, triangles, TrimeshFlags::DELETE_DEGENERATE_TRIANGLES) {
            // A child collider joins the surface's static body.
            commands.entity(holder).try_insert(collider);
        }
    }

    #[cfg(feature = "physics")]
    {
        use avian3d::prelude::{Collider, RigidBody, TrimeshFlags};
        // The paved surfaces, paint excluded: it lies a centimetre over the
        // carriageway and would only add triangles.
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        for (_, data) in parts.iter().filter(|(kind, _)| !is_marking(*kind)) {
            let first = vertices.len() as u32;
            vertices.extend(data.positions.iter().map(|p| Vec3::from_array(*p)));
            triangles.extend(data.indices.chunks_exact(3).map(|t| [first + t[0], first + t[1], first + t[2]]));
        }
        let collider = (!triangles.is_empty())
            .then(|| {
                let flags = TrimeshFlags::FIX_INTERNAL_EDGES | TrimeshFlags::DELETE_DEGENERATE_TRIANGLES;
                Collider::try_trimesh_with_config(vertices.clone(), triangles.clone(), flags)
                    .or_else(|_| Collider::try_trimesh_with_config(vertices, triangles, TrimeshFlags::empty()))
                    .ok()
            })
            .flatten();
        match collider {
            Some(collider) => {
                commands.entity(surface).try_insert((RigidBody::Static, collider));
            }
            None => {
                commands.entity(surface).try_remove::<Collider>();
            }
        }
    }
    children
}

fn is_marking(kind: Surface) -> bool {
    matches!(kind, Surface::MarkingWhite | Surface::MarkingYellow)
}

/// How surface `kind` of `design` looks. A carriageway whose bed is asphalt
/// (or unpainted) wears the asphalt textures, any other bed its swatch;
/// shoulders wear worn asphalt when the class paves them, the shoulder
/// material's swatch when it does not.
fn part_look(kind: Surface, design: &DesignedRoad, looks: DesignedLooks) -> PartLook {
    let plain = |srgb: [f32; 3], bias: f32| PartLook::Plain(srgb.map(f32::to_bits), bias.to_bits());
    let asphalt_bed = looks.bed.is_none_or(|slot| slot == super::material::TerrainMaterial::Asphalt.to_u8());
    match kind {
        Surface::Carriageway if asphalt_bed => PartLook::Asphalt,
        Surface::Carriageway => plain(looks.color, ROAD_SURFACE_DEPTH_BIAS),
        Surface::Shoulder if design.standard.shoulder_paved && asphalt_bed => PartLook::WornAsphalt,
        Surface::Shoulder => match looks.shoulder {
            Some(srgb) => plain(srgb, ROAD_SURFACE_DEPTH_BIAS),
            None => PartLook::Gravel,
        },
        Surface::Kerb => plain(KERB_SRGB, ROAD_SURFACE_DEPTH_BIAS),
        Surface::Sidewalk => plain(SIDEWALK_SRGB, ROAD_SURFACE_DEPTH_BIAS),
        Surface::MarkingWhite => plain(MARKING_WHITE_SRGB, MARKING_DEPTH_BIAS),
        Surface::MarkingYellow => plain(MARKING_YELLOW_SRGB, MARKING_DEPTH_BIAS),
    }
}

/// The material of `look`. The asphalt textures repeat every
/// [`ASPHALT_TILE_METRES`] (the meshes' UVs are metres across and along);
/// without an asset server they fall back to their grey.
fn part_material(look: PartLook, asset_server: Option<&AssetServer>) -> StandardMaterial {
    let texture = |name: &str, srgb: bool| {
        asset_server.map(|server| -> Handle<Image> {
            server
                .load_builder()
                .with_settings(move |s: &mut ImageLoaderSettings| {
                    s.is_srgb = srgb;
                    s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                        address_mode_u: ImageAddressMode::Repeat,
                        address_mode_v: ImageAddressMode::Repeat,
                        address_mode_w: ImageAddressMode::Repeat,
                        ..ImageSamplerDescriptor::linear()
                    });
                })
                .load(format!("bundled://materials/textures/{name}.png"))
        })
    };
    let textured = |stem: &str| {
        let base = texture(&format!("{stem}_base_color"), true);
        StandardMaterial {
            base_color: if base.is_some() {
                Color::WHITE
            } else {
                let [r, g, b] = ROAD_SURFACE_DEFAULT_SRGB;
                Color::srgb(r, g, b)
            },
            base_color_texture: base,
            normal_map_texture: texture(&format!("{stem}_normal"), false),
            metallic_roughness_texture: texture(&format!("{stem}_metallic_roughness"), false),
            // The texture's green channel is the roughness; asphalt is no metal.
            perceptual_roughness: 1.0,
            metallic: 0.0,
            uv_transform: Affine2::from_scale(Vec2::splat(1.0 / ASPHALT_TILE_METRES)),
            depth_bias: ROAD_SURFACE_DEPTH_BIAS,
            ..default()
        }
    };
    match look {
        PartLook::Asphalt => textured("asphalt"),
        PartLook::WornAsphalt => textured("asphalt_worn"),
        PartLook::Gravel => textured("granite"),
        PartLook::Grass => textured("grass"),
        PartLook::Concrete => textured("concrete"),
        PartLook::Plain(bits, bias) => {
            let [r, g, b] = bits.map(f32::from_bits);
            StandardMaterial {
                base_color: Color::srgb(r, g, b),
                perceptual_roughness: 0.85,
                depth_bias: f32::from_bits(bias),
                ..default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::SystemId;
    use bevy::mesh::VertexAttributeValues;

    use super::*;
    use crate::classes::ClassName;
    use crate::terrain::layers::{LayerDesc, LayerKind, SplineLayer, StampBlend, StampLayer, StampShape};
    use crate::terrain::{TerrainConfig, TerrainData, TerrainRebake};

    /// The id of the one layer, and of the spline instance whose uuid starts
    /// with it.
    const ROAD_ID: u64 = 1;

    fn config() -> TerrainConfig {
        TerrainConfig {
            chunk_size: 32.0,
            chunk_resolution: 16,
            chunks_x: 1,
            chunks_z: 1,
            height_scale: 100.0,
            ..TerrainConfig::default()
        }
    }

    fn spline(mode: SplineMode, z: f32) -> LayerDesc {
        LayerDesc {
            id: ROAD_ID,
            order: 0,
            kind: LayerKind::Spline(SplineLayer {
                mode,
                points: vec![Vec3::new(-20.0, 5.0, z), Vec3::new(20.0, 5.0, z)],
                width: 8.0,
                shoulder_width: 4.0,
                depth: 2.0,
                smoothing: 0.5,
                bed_material: None,
                shoulder_material: None,
                design: None,
            }),
        }
    }

    struct Rig {
        world: World,
        system: SystemId,
        root: Entity,
        spline: Entity,
        base: TerrainData,
    }

    impl Rig {
        fn new(layer: LayerDesc) -> Self {
            let mut world = World::new();
            world.init_resource::<RoadSurfaces>();
            world.init_resource::<Assets<Mesh>>();
            world.init_resource::<Assets<StandardMaterial>>();
            let config = config();
            let mut base = TerrainData::procedural();
            base.resize_cache(&config);
            let mut baked = TerrainBaked::new(&base, vec![layer]);
            baked.rebake(&config, &base, &TerrainRebake::default());
            let root = world.spawn((TerrainRoot, config, base.clone(), baked)).id();
            let spline = world
                .spawn((
                    Instance {
                        name: "Road".into(),
                        class_name: ClassName::TerrainSpline,
                        archivable: true,
                        id: 0,
                        uuid: format!("{:016x}{}", ROAD_ID, "0".repeat(16)),
                        ai: false,
                    },
                    TerrainSpline::default(),
                ))
                .id();
            // Registered once, so change detection and the removal reader
            // carry over from run to run as they do in a schedule.
            let system = world.register_system(sync_road_surfaces);
            Self { world, system, root, spline, base }
        }

        fn run(&mut self) {
            assert!(self.world.run_system(self.system).is_ok(), "the surface system runs");
        }

        fn surfaces(&mut self) -> Vec<(Entity, RoadSurface)> {
            let mut query = self.world.query::<(Entity, &RoadSurface)>();
            query.iter(&self.world).map(|(entity, surface)| (entity, *surface)).collect()
        }

        fn rebake(&mut self) {
            let config = config();
            let base = self.base.clone();
            self.world.get_mut::<TerrainBaked>(self.root).expect("a bake").rebake(&config, &base, &TerrainRebake::default());
        }

        fn relayer(&mut self, layer: LayerDesc) {
            self.relayers(vec![layer]);
        }

        fn relayers(&mut self, layers: Vec<LayerDesc>) {
            self.world.get_mut::<TerrainBaked>(self.root).expect("a bake").set_layers(layers);
            self.rebake();
        }

        fn stations(&self) -> Vec<Vec3> {
            let baked = self.world.get::<TerrainBaked>(self.root).expect("a bake");
            baked.baked_spline(ROAD_ID).expect("the road is baked").1.to_vec()
        }

        /// The finished ground under `station`, which the ribbon is laid on.
        fn ground(&self, station: Vec3) -> f32 {
            let baked = self.world.get::<TerrainBaked>(self.root).expect("a bake");
            height_at_world(&config(), &baked.data, station.x, station.z)
        }

        fn ribbon(&self, surface: Entity) -> Handle<Mesh> {
            self.world.get::<Mesh3d>(surface).expect("a ribbon").0.clone()
        }

        fn ribbon_heights(&self, surface: Entity) -> Vec<f32> {
            let handle = self.ribbon(surface);
            let mesh = self.world.resource::<Assets<Mesh>>().get(&handle).expect("the ribbon mesh");
            match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
                Some(VertexAttributeValues::Float32x3(positions)) => positions.iter().map(|p| p[1]).collect(),
                other => panic!("ribbon positions {other:?}"),
            }
        }
    }

    #[test]
    fn a_road_gets_a_surface_on_its_baked_stations_that_follows_it_and_goes_with_it() {
        let mut rig = Rig::new(spline(SplineMode::Road, 0.0));
        rig.run();
        let surfaces = rig.surfaces();
        assert_eq!(surfaces.len(), 1, "one surface per road");
        let (surface, marker) = surfaces[0];
        assert_eq!(marker.spline, rig.spline);
        assert_eq!(rig.world.resource::<RoadSurfaces>().surface_of(rig.spline), Some(surface));
        #[cfg(feature = "physics")]
        assert!(rig.world.get::<avian3d::prelude::Collider>(surface).is_some(), "the ribbon collides");

        // Both edges of the ribbon stand the lift above the finished ground
        // under every station the corridor was carved along.
        let stations = rig.stations();
        let heights = rig.ribbon_heights(surface);
        assert_eq!(heights.len(), stations.len() * 2);
        for (i, station) in stations.iter().enumerate() {
            let ground = rig.ground(*station);
            for y in &heights[i * 2..i * 2 + 2] {
                assert!((y - (ground + ROAD_SURFACE_LIFT)).abs() < 1e-5, "ribbon at {y} over {station:?} on ground {ground}");
            }
        }

        // Nothing changed, nothing rebuilt.
        let first = rig.ribbon(surface);
        rig.run();
        assert_eq!(rig.ribbon(surface), first);

        // A replaced layer list waiting for its bake is not a road gone.
        rig.world.get_mut::<TerrainBaked>(rig.root).expect("a bake").set_layers(vec![spline(SplineMode::Road, 10.0)]);
        rig.run();
        assert_eq!(rig.surfaces(), vec![(surface, marker)]);
        assert_eq!(rig.ribbon(surface), first);
        // Once baked, the same entity gets the moved road's ribbon.
        rig.rebake();
        rig.run();
        assert_eq!(rig.surfaces(), vec![(surface, marker)]);
        assert_ne!(rig.ribbon(surface), first, "the moved road has a new ribbon");

        // Out of Road mode, no surface.
        rig.relayer(spline(SplineMode::River, 10.0));
        rig.run();
        assert!(rig.surfaces().is_empty(), "a river is not driven on");

        // A road again, then the spline goes, and its surface with it.
        rig.relayer(spline(SplineMode::Road, 10.0));
        rig.run();
        assert_eq!(rig.surfaces().len(), 1);
        rig.world.despawn(rig.spline);
        rig.run();
        assert!(rig.surfaces().is_empty(), "the removed road's surface is gone");
        assert!(rig.world.resource::<RoadSurfaces>().surface_of(rig.spline).is_none());
    }

    #[test]
    fn a_layer_ordered_after_the_road_reshapes_its_ribbon_too() {
        let mut rig = Rig::new(spline(SplineMode::Road, 0.0));
        rig.run();
        let (surface, marker) = rig.surfaces()[0];
        let stations = rig.stations();
        let before = rig.ribbon_heights(surface);
        // The road runs from x = -20 to 20 along z = 0; its middle station is
        // the one nearest x = 0.
        let middle = (0..stations.len()).min_by(|a, b| stations[*a].x.abs().total_cmp(&stations[*b].x.abs())).unwrap();

        // A crater 3 m deep at the road's middle, ordered after it.
        let crater = LayerDesc {
            id: 2,
            order: 1,
            kind: LayerKind::Stamp(StampLayer {
                center: Vec3::ZERO,
                yaw: 0.0,
                shape: StampShape::Crater,
                radius: 6.0,
                height: 3.0,
                falloff: 2.0,
                blend: StampBlend::Add,
                strength: 1.0,
            }),
        };
        rig.relayers(vec![spline(SplineMode::Road, 0.0), crater]);
        rig.run();
        assert_eq!(rig.surfaces(), vec![(surface, marker)], "the same surface is rebuilt");
        assert_eq!(rig.stations(), stations, "a layer after the road leaves the road's own profile alone");

        let after = rig.ribbon_heights(surface);
        assert_eq!(after.len(), before.len());
        let dropped = before[middle * 2] - after[middle * 2];
        assert!(dropped > 1.5, "the ribbon over the crater dropped only {dropped} m");
        for (i, station) in stations.iter().enumerate() {
            let ground = rig.ground(*station);
            for y in &after[i * 2..i * 2 + 2] {
                assert!((y - (ground + ROAD_SURFACE_LIFT)).abs() < 1e-5, "ribbon at {y} over {station:?} on ground {ground}");
            }
        }
        // The road's end, 20 m from the crater, is where it was.
        assert!((after[0] - before[0]).abs() < 1e-6, "the far end moved from {} to {}", before[0], after[0]);
    }

    /// A Local street along z, designed to graded ground.
    fn designed(z: f32) -> LayerDesc {
        use crate::terrain::road_design::{ProfileMode, RoadSpec};
        let mut desc = spline(SplineMode::Road, z);
        if let LayerKind::Spline(layer) = &mut desc.kind {
            layer.design = Some(RoadSpec::new(RoadClass::Local, ProfileMode::Auto));
        }
        desc
    }

    fn parts_of(rig: &mut Rig, surface: Entity) -> Vec<(Entity, Surface)> {
        let mut query = rig.world.query::<(Entity, &RoadSurfacePart, &ChildOf)>();
        query
            .iter(&rig.world)
            .filter(|(_, _, parent)| parent.parent() == surface)
            .map(|(entity, part, _)| (entity, part.surface))
            .collect()
    }

    #[test]
    fn a_designed_road_draws_its_cross_section_on_its_design_and_gives_it_back() {
        let mut rig = Rig::new(designed(0.0));
        rig.run();
        let surfaces = rig.surfaces();
        assert_eq!(surfaces.len(), 1, "one surface per road");
        let (surface, marker) = surfaces[0];
        assert!(rig.world.get::<Mesh3d>(surface).is_none(), "a designed road's surfaces are children, not a ribbon");
        let parts = parts_of(&mut rig, surface);
        let kinds: Vec<Surface> = parts.iter().map(|(_, kind)| *kind).collect();
        for kind in [Surface::Carriageway, Surface::Shoulder, Surface::Kerb, Surface::Sidewalk, Surface::MarkingYellow] {
            assert!(kinds.contains(&kind), "no {kind:?} among {kinds:?}");
        }
        #[cfg(feature = "physics")]
        assert!(rig.world.get::<avian3d::prelude::Collider>(surface).is_some(), "the paved surfaces collide");

        // The carriageway lies on the design: within its crown of the
        // profile, the lift over it.
        let design = rig.world.get::<TerrainBaked>(rig.root).unwrap().baked_design(ROAD_ID).expect("designed").clone();
        let (lo, hi) = design.stations.iter().fold((f64::MAX, f64::MIN), |(lo, hi), st| {
            let y = design.profile.elevation(st.s);
            (lo.min(y), hi.max(y))
        });
        let carriageway = parts.iter().find(|(_, kind)| *kind == Surface::Carriageway).unwrap().0;
        let handle = rig.world.get::<Mesh3d>(carriageway).unwrap().0.clone();
        let mesh = rig.world.resource::<Assets<Mesh>>().get(&handle).expect("the carriageway mesh");
        let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("carriageway positions");
        };
        let crown = design.standard.crown * design.standard.lane_width + 1e-3;
        for p in positions {
            let y = f64::from(p[1]) - DESIGNED_SURFACE_LIFT;
            assert!(y >= lo - crown && y <= hi + 1e-3, "carriageway vertex at {y} off the profile {lo}..{hi}");
        }

        // Nothing changed, nothing rebuilt.
        rig.run();
        assert_eq!(parts_of(&mut rig, surface), parts);

        // A plain road again: the same entity gets its ribbon, the parts go.
        rig.relayer(spline(SplineMode::Road, 0.0));
        rig.run();
        assert_eq!(rig.surfaces(), vec![(surface, marker)]);
        assert!(rig.world.get::<Mesh3d>(surface).is_some(), "the plain road has its ribbon back");
        assert!(parts_of(&mut rig, surface).is_empty(), "and none of the designed surfaces");
    }
}

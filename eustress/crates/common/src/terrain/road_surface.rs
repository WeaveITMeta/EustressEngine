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

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::height_query::height_at_world;
use super::layer_instances::{layer_id, TerrainSpline};
use super::layers::{SplineMode, TerrainBaked};
use super::material::MATERIAL_SLOT_NONE;
use super::material_slots::TerrainMaterialSlots;
use super::road::{build_ribbon_mesh, RoadPath, RoadProfile};
use super::{TerrainConfig, TerrainRoot};
use crate::classes::Instance;

/// How far the ribbon floats above the carved bed, metres: enough to clear
/// the terrain mesh without z-fighting, little enough that a wheel does not
/// feel the step onto the shoulder.
pub const ROAD_SURFACE_LIFT: f32 = 0.05;
/// Thickness of each collision box, metres. Its top face meets the ribbon
/// and the rest sits in the ground.
pub const ROAD_SURFACE_THICKNESS: f32 = 0.3;
/// sRGB of a ribbon whose bed is left unpainted: asphalt grey.
pub const ROAD_SURFACE_DEFAULT_SRGB: [f32; 3] = [0.16, 0.16, 0.18];

/// Marks the surface entity built for the road spline `spline`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadSurface {
    pub spline: Entity,
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
}

/// The surface built for each road spline, keyed by the spline's entity, and
/// the ribbon materials, one per colour, keyed by the colour's bits so two
/// slots with the same swatch share one and a redefined slot gets a new one.
#[derive(Resource, Debug, Default)]
pub struct RoadSurfaces {
    built: HashMap<Entity, BuiltSurface>,
    materials: HashMap<[u32; 3], Handle<StandardMaterial>>,
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
    mut removed: RemovedComponents<TerrainSpline>,
    mut meshes: Option<ResMut<Assets<Mesh>>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    // Optional: a host without the slot table draws every ribbon grey.
    slots: Option<Res<TerrainMaterialSlots>>,
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
            built.half_width == half_width
                && built.stations.as_slice() == placed.as_slice()
                && built.bed == bed
                && built.color == color
        });
        if unchanged && reuse.is_some() {
            kept.extend(previous.map(|built| (spline_entity, built)));
            continue;
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
        kept.insert(spline_entity, BuiltSurface { surface, stations: placed, half_width, bed, color });
    }

    // Whatever was built and is not wanted any more: a spline disabled,
    // switched out of Road mode, or left with fewer than two points.
    for (_, built) in surfaces.built.drain() {
        commands.entity(built.surface).try_despawn();
    }
    surfaces.built = kept;
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
}

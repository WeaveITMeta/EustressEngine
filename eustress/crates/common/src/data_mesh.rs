//! # What a DataMesh draws
//!
//! A Roblox DataMesh (`SpecialMesh`, `BlockMesh`, `CylinderMesh`, `FileMesh`)
//! is a child of a part that changes what the part DRAWS, never what it
//! collides as: a ball wheel with a `SpecialMesh` Cylinder rolls on a sphere
//! and looks like a disc. The part keeps its own shape for its collider, its
//! `Shape` and its CFrame; this module only swaps the mesh it is drawn with.
//!
//! One look, one function, both apps. [`DataMeshLook`] on a part says what it
//! draws; [`draw_look`] bakes it into a mesh in the part's unit space (the
//! entity's scale is the part's Size): the kind's primitive, turned to the
//! kind's orientation, times `Scale`, moved by `Offset`. Studio links a
//! DataMesh entity's component to its part ([`link_data_mesh_children`]); the
//! Player builds the look from the tree ([`look_from_tree`]). Removing the
//! DataMesh draws the part's own shape again.
//!
//! How each kind draws, in its part's Roblox frame:
//!
//! | DataMesh | drawn as |
//! |---|---|
//! | `BlockMesh`, `SpecialMesh` Brick | a block |
//! | `CylinderMesh` | a cylinder standing on the part's Y axis |
//! | `SpecialMesh` Cylinder | a cylinder lying along the part's X axis |
//! | `SpecialMesh` Sphere | a ball |
//! | `SpecialMesh` Head | a ball (approximation) |
//! | `SpecialMesh` Wedge, CornerWedge | a wedge, a corner wedge |
//! | `SpecialMesh` Torso, Prism, Pyramid, ParallelRamp, RightAngleRamp | a block (approximation) |
//! | `SpecialMesh` FileMesh, `FileMesh`, with no mesh | nothing |
//! | `SpecialMesh` FileMesh, `FileMesh`, with a mesh | not drawn here: the part keeps its mesh |
//!
//! The two cylinders' axes were read from Vehicle Simulator: a wheel's axle
//! runs along its X with a `SpecialMesh` Cylinder thin along X; checkpoint
//! pillars with a `CylinderMesh` are round in X and Z and tall in Y.
//!
//! **A Cylinder part's Roblox frame.** Eustress's cylinder stands on Y where
//! Roblox's lies along X, so an imported Cylinder part is turned a quarter
//! turn about Z and its file Size is Roblox's `(Y, X, Z)`. A look on a
//! Cylinder part is drawn in the part's Roblox frame: look X is the entity's
//! +Y (the cylinder's axis), look Y the entity's -X, look Z the entity's Z,
//! and the part's box there is Roblox's Size. `Scale` and `Offset` are
//! Roblox's, as written.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::classes::{BasePart, BlockMesh, CylinderMesh, FileMesh, Instance, MeshType, Part, PartType, SpecialMesh};
use crate::datamodel::{DataModel, DmValue, InstanceId};
use crate::space_read::primitive_mesh;

/// Which DataMesh a part has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DataMeshKind {
    Block,
    Cylinder,
    Special(MeshType),
    File,
}

impl DataMeshKind {
    /// The kind a DataMesh class and (for a `SpecialMesh`) `MeshType` name.
    pub fn of(class: &str, mesh_type: Option<MeshType>) -> Option<DataMeshKind> {
        match class {
            "BlockMesh" => Some(DataMeshKind::Block),
            "CylinderMesh" => Some(DataMeshKind::Cylinder),
            "FileMesh" => Some(DataMeshKind::File),
            "SpecialMesh" => Some(DataMeshKind::Special(mesh_type.unwrap_or(MeshType::Head))),
            _ => None,
        }
    }
}

/// What a part draws in place of its own shape.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct DataMeshLook {
    pub kind: DataMeshKind,
    /// Roblox `Scale`: a ratio of the part's Size.
    pub scale: Vec3,
    /// Roblox `Offset`, in metres along the part's Roblox axes.
    pub offset: Vec3,
    /// A file mesh's `MeshId` (empty for the primitive kinds).
    pub mesh_id: String,
}

impl DataMeshLook {
    pub fn special(m: &SpecialMesh) -> Self {
        Self { kind: DataMeshKind::Special(m.mesh_type), scale: m.scale, offset: m.offset, mesh_id: m.mesh_id.clone() }
    }
    pub fn block(m: &BlockMesh) -> Self {
        Self { kind: DataMeshKind::Block, scale: m.scale, offset: m.offset, mesh_id: String::new() }
    }
    pub fn cylinder(m: &CylinderMesh) -> Self {
        Self { kind: DataMeshKind::Cylinder, scale: m.scale, offset: m.offset, mesh_id: String::new() }
    }
    pub fn file(m: &FileMesh) -> Self {
        Self { kind: DataMeshKind::File, scale: m.scale, offset: m.offset, mesh_id: m.mesh_id.clone() }
    }
}

/// The primitive a DataMesh draws and its turn in the part's Roblox frame, or
/// `None` for a file mesh.
pub fn drawn_primitive(kind: DataMeshKind) -> Option<(PartType, Quat)> {
    use MeshType::*;
    // Eustress's unit cylinder stands on Y; a quarter turn about Z lays it
    // along X.
    let along_x = Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2);
    Some(match kind {
        DataMeshKind::Block => (PartType::Block, Quat::IDENTITY),
        DataMeshKind::Cylinder => (PartType::Cylinder, Quat::IDENTITY),
        DataMeshKind::File => return None,
        DataMeshKind::Special(t) => match t {
            Brick | Torso | Prism | Pyramid | ParallelRamp | RightAngleRamp => (PartType::Block, Quat::IDENTITY),
            Sphere | Head => (PartType::Ball, Quat::IDENTITY),
            Wedge => (PartType::Wedge, Quat::IDENTITY),
            CornerWedge => (PartType::CornerWedge, Quat::IDENTITY),
            Cylinder => (PartType::Cylinder, along_x),
            FileMesh => return None,
        },
    })
}

/// A part's Roblox frame in its entity: on a Cylinder part a quarter turn
/// about Z (look X is the entity's +Y), else none.
fn roblox_frame(part_shape: PartType) -> Quat {
    if part_shape == PartType::Cylinder {
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
    } else {
        Quat::IDENTITY
    }
}

/// A part's look as a mesh in its unit space (the entity's scale is `size`).
/// `None` when the look is a file mesh with a mesh, which the part draws
/// itself; a file mesh with no mesh draws nothing.
pub fn draw_look(look: &DataMeshLook, part_shape: PartType, size: Vec3) -> Option<Mesh> {
    let Some((shape, turn)) = drawn_primitive(look.kind) else {
        return look.mesh_id.trim().is_empty().then(nothing);
    };
    let frame = roblox_frame(part_shape);
    // The part's box in its Roblox frame: Roblox's Size.
    let roblox_size = (frame.inverse() * size).abs();
    Some(
        primitive_mesh(shape)
            .rotated_by(turn)
            .scaled_by(look.scale * roblox_size)
            .translated_by(look.offset)
            .rotated_by(frame)
            .scaled_by(size.map(nonzero).recip()),
    )
}

fn nonzero(v: f32) -> f32 {
    if v.abs() > 1e-6 { v } else { 1.0 }
}

/// A mesh that draws nothing: one triangle of no area.
fn nothing() -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 3])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2]))
}

/// A part's look from the Play tree: its first DataMesh child's class and
/// properties, as the Player draws it.
pub fn look_from_tree(g: &DataModel, part: InstanceId) -> Option<DataMeshLook> {
    g.children(part).iter().find_map(|&child| {
        let class = g.class_of(child)?;
        let mesh_type = g
            .get_prop(child, "MeshType")
            .and_then(|v| v.as_enum_name().map(str::to_string))
            .and_then(|n| MeshType::from_name(&n));
        let kind = DataMeshKind::of(class, mesh_type)?;
        let vec = |name: &str, default: Vec3| {
            g.get_prop(child, name).and_then(|v: DmValue| v.as_vector3()).map_or(default, |v| v.to_vec3())
        };
        let mesh_id = g.get_prop(child, "MeshId").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        Some(DataMeshLook { kind, scale: vec("Scale", Vec3::ONE), offset: vec("Offset", Vec3::ZERO), mesh_id })
    })
}

/// The mesh a part drew before a DataMesh took over, the mesh baked for it,
/// and the look, shape and size that mesh was baked for.
#[derive(Component, Clone, Debug)]
pub struct DataMeshDrawn {
    own: Handle<Mesh>,
    baked: Handle<Mesh>,
    look: DataMeshLook,
    shape: PartType,
    size: Vec3,
}

/// Draws every part's [`DataMeshLook`]. Studio and the Player both add it.
pub struct DataMeshPlugin;

impl Plugin for DataMeshPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (link_data_mesh_children, draw_data_mesh_looks, draw_own_shapes_again).chain());
    }
}

/// The part a DataMesh entity changes: its parent, as in Roblox, or the part
/// above the unscaled anchor a sized part hangs its children from (an anchor
/// is no instance of its own).
fn part_of(
    e: Entity,
    parents: &Query<&ChildOf>,
    parts: &Query<(), With<BasePart>>,
    instances: &Query<(), With<Instance>>,
) -> Option<Entity> {
    let parent = parents.get(e).ok()?.0;
    if parts.contains(parent) {
        return Some(parent);
    }
    let above = parents.get(parent).ok()?.0;
    (!instances.contains(parent) && parts.contains(above)).then_some(above)
}

/// Studio: a DataMesh entity's component becomes its part's look, and goes
/// when the DataMesh does.
#[allow(clippy::type_complexity)]
pub fn link_data_mesh_children(
    mut commands: Commands,
    changed: Query<
        (Entity, Option<&SpecialMesh>, Option<&BlockMesh>, Option<&CylinderMesh>, Option<&FileMesh>),
        Or<(Changed<SpecialMesh>, Changed<BlockMesh>, Changed<CylinderMesh>, Changed<FileMesh>, Changed<ChildOf>)>,
    >,
    parents: Query<&ChildOf>,
    parts: Query<(), With<BasePart>>,
    instances: Query<(), With<Instance>>,
    mut gone: (
        RemovedComponents<SpecialMesh>,
        RemovedComponents<BlockMesh>,
        RemovedComponents<CylinderMesh>,
        RemovedComponents<FileMesh>,
    ),
    mut links: Local<HashMap<Entity, Entity>>,
) {
    let removed: Vec<Entity> = gone.0.read().chain(gone.1.read()).chain(gone.2.read()).chain(gone.3.read()).collect();
    for dm in removed {
        if let Some(part) = links.remove(&dm) {
            if let Ok(mut ec) = commands.get_entity(part) {
                ec.try_remove::<DataMeshLook>();
            }
        }
    }
    for (dm, special, block, cylinder, file) in &changed {
        let look = special
            .map(DataMeshLook::special)
            .or_else(|| block.map(DataMeshLook::block))
            .or_else(|| cylinder.map(DataMeshLook::cylinder))
            .or_else(|| file.map(DataMeshLook::file));
        let Some(look) = look else { continue };
        let part = part_of(dm, &parents, &parts, &instances);
        let old = match part {
            Some(p) => links.insert(dm, p).filter(|old| *old != p),
            None => links.remove(&dm),
        };
        if let Some(old) = old {
            if let Ok(mut ec) = commands.get_entity(old) {
                ec.try_remove::<DataMeshLook>();
            }
        }
        if let Some(part) = part {
            commands.entity(part).try_insert(look);
        }
    }
}

/// Bakes each changed look into its part's mesh. A part only moving keeps its
/// mesh; a part whose own mesh someone else replaced (a Shape edit) takes that
/// as its own mesh and is baked again.
#[allow(clippy::type_complexity)]
pub fn draw_data_mesh_looks(
    mut commands: Commands,
    mut looks: Query<
        (Entity, &DataMeshLook, &Transform, &mut Mesh3d, Option<&Part>, Option<&mut DataMeshDrawn>),
        Or<(Changed<DataMeshLook>, Changed<Transform>, Changed<Part>, Changed<Mesh3d>)>,
    >,
    mut meshes: ResMut<Assets<Mesh>>,
    mut baked: Local<HashMap<(DataMeshKind, u8, bool, [u32; 9]), Handle<Mesh>>>,
) {
    for (entity, look, transform, mut mesh, part, mut drawn) in &mut looks {
        let shape = part.map_or(PartType::Block, |p| p.shape);
        let size = transform.scale;
        let replaced = drawn.as_ref().is_some_and(|d| mesh.0 != d.baked);
        if let (true, Some(d)) = (replaced, drawn.as_mut()) {
            d.own = mesh.0.clone();
        }
        if !replaced && drawn.as_ref().is_some_and(|d| d.look == *look && d.shape == shape && d.size == size) {
            continue;
        }
        let key = (
            look.kind,
            shape as u8,
            look.mesh_id.trim().is_empty(),
            [look.scale.x, look.scale.y, look.scale.z, look.offset.x, look.offset.y, look.offset.z, size.x, size.y, size.z]
                .map(f32::to_bits),
        );
        let handle = match baked.get(&key) {
            Some(h) => Some(h.clone()),
            None => draw_look(look, shape, size).map(|m| baked.entry(key).or_insert_with(|| meshes.add(m)).clone()),
        };
        let Some(handle) = handle else {
            // A file mesh with a mesh: the part draws its own mesh.
            if let Some(d) = drawn {
                if mesh.0 == d.baked {
                    mesh.0 = d.own.clone();
                }
                commands.entity(entity).try_remove::<DataMeshDrawn>();
            }
            continue;
        };
        match drawn {
            Some(mut d) => {
                d.baked = handle.clone();
                d.look = look.clone();
                d.shape = shape;
                d.size = size;
            }
            None => {
                commands.entity(entity).try_insert(DataMeshDrawn {
                    own: mesh.0.clone(),
                    baked: handle.clone(),
                    look: look.clone(),
                    shape,
                    size,
                });
            }
        }
        if mesh.0 != handle {
            mesh.0 = handle;
        }
    }
}

/// A part whose DataMesh went draws its own shape again, unless something
/// else has drawn it since.
pub fn draw_own_shapes_again(
    mut commands: Commands,
    mut gone: RemovedComponents<DataMeshLook>,
    mut parts: Query<(&DataMeshDrawn, &mut Mesh3d)>,
) {
    for part in gone.read() {
        if let Ok((drawn, mut mesh)) = parts.get_mut(part) {
            if mesh.0 == drawn.baked {
                mesh.0 = drawn.own.clone();
            }
            commands.entity(part).try_remove::<DataMeshDrawn>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::primitives::MeshAabb;
    use bevy::mesh::VertexAttributeValues;

    fn look(kind: DataMeshKind, scale: Vec3, offset: Vec3) -> DataMeshLook {
        DataMeshLook { kind, scale, offset, mesh_id: String::new() }
    }

    fn extents(mesh: &Mesh) -> Vec3 {
        mesh.compute_aabb().expect("a primitive has an AABB").half_extents.into()
    }

    /// Whether the mesh has flat caps facing along `axis`: a fifth or more of
    /// its vertices face that way. One vertex is not enough, since a cylinder's
    /// side has a vertex facing each way round it, +X and -X included; the two
    /// caps are about half of all its vertices.
    fn has_cap_along(mesh: &Mesh, axis: Vec3) -> bool {
        let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { return false };
        let facing = normals.iter().filter(|n| Vec3::from_array(**n).dot(axis).abs() > 0.999).count();
        facing * 5 >= normals.len()
    }

    #[test]
    fn a_special_mesh_cylinder_lies_along_x_and_a_cylinder_mesh_stands_on_y() {
        let wheel = look(DataMeshKind::Special(MeshType::Cylinder), Vec3::new(0.33, 1.0, 1.0), Vec3::ZERO);
        let disc = draw_look(&wheel, PartType::Ball, Vec3::splat(3.0)).unwrap();
        assert!(extents(&disc).distance(Vec3::new(0.165, 0.5, 0.5)) < 1e-3, "{:?}", extents(&disc));
        assert!(has_cap_along(&disc, Vec3::X), "the wheel's flat faces face along its axle");

        let pillar = look(DataMeshKind::Cylinder, Vec3::ONE, Vec3::ZERO);
        let standing = draw_look(&pillar, PartType::Block, Vec3::new(50.0, 1200.0, 50.0)).unwrap();
        assert!(has_cap_along(&standing, Vec3::Y));
        assert!(!has_cap_along(&standing, Vec3::X));
    }

    #[test]
    fn on_a_cylinder_part_the_look_is_drawn_in_its_roblox_frame() {
        // Roblox Size (4, 2, 2): a cylinder 4 long on X. Imported, the entity
        // is turned and its Size is (2, 4, 2), its axis the entity's Y.
        let size = Vec3::new(2.0, 4.0, 2.0);
        let rod = look(DataMeshKind::Special(MeshType::Cylinder), Vec3::ONE, Vec3::ZERO);
        let m = draw_look(&rod, PartType::Cylinder, size).unwrap();
        assert!(has_cap_along(&m, Vec3::Y), "a SpecialMesh cylinder runs along the part's own axis");
        assert!(extents(&m).distance(Vec3::splat(0.5)) < 1e-3, "{:?}", extents(&m));
        // Offset 1 m along Roblox X is 1 m along the entity's Y: a quarter of
        // the 4 m length.
        let moved = look(DataMeshKind::Special(MeshType::Brick), Vec3::ONE, Vec3::new(1.0, 0.0, 0.0));
        let aabb = draw_look(&moved, PartType::Cylinder, size).unwrap().compute_aabb().unwrap();
        assert!(Vec3::from(aabb.center).distance(Vec3::new(0.0, 0.25, 0.0)) < 1e-5, "{:?}", aabb.center);
    }

    #[test]
    fn offset_is_metres_and_the_part_size_is_not_applied_twice() {
        let moved = look(DataMeshKind::Block, Vec3::ONE, Vec3::new(1.0, 0.0, 0.0));
        let aabb = draw_look(&moved, PartType::Block, Vec3::new(4.0, 1.0, 2.0)).unwrap().compute_aabb().unwrap();
        // 1 m along a 4 m part is a quarter of its unit space.
        assert!((aabb.center.x - 0.25).abs() < 1e-5);
    }

    #[test]
    fn a_file_mesh_draws_its_mesh_or_nothing() {
        assert!(drawn_primitive(DataMeshKind::File).is_none());
        let with_mesh = DataMeshLook { mesh_id: "meshes/rim.glb".into(), ..look(DataMeshKind::File, Vec3::ONE, Vec3::ZERO) };
        assert!(draw_look(&with_mesh, PartType::Block, Vec3::ONE).is_none(), "the part draws it");
        let empty = look(DataMeshKind::Special(MeshType::FileMesh), Vec3::ONE, Vec3::ZERO);
        let m = draw_look(&empty, PartType::Block, Vec3::ONE).unwrap();
        assert_eq!(extents(&m), Vec3::ZERO, "a FileMesh with no mesh draws nothing");
    }

    fn app_with_part(shape: PartType) -> (App, Entity, Handle<Mesh>) {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>().add_plugins(DataMeshPlugin);
        let own = app.world_mut().resource_mut::<Assets<Mesh>>().add(primitive_mesh(shape));
        let part = app
            .world_mut()
            .spawn((BasePart::default(), Part { shape }, Transform::from_scale(Vec3::splat(3.0)), Mesh3d(own.clone())))
            .id();
        (app, part, own)
    }

    #[test]
    fn a_part_draws_its_data_mesh_and_its_own_shape_when_it_goes() {
        let (mut app, wheel, own) = app_with_part(PartType::Ball);
        let mesh = app
            .world_mut()
            .spawn((
                SpecialMesh { mesh_type: MeshType::Cylinder, scale: Vec3::new(0.33, 1.0, 1.0), ..default() },
                ChildOf(wheel),
            ))
            .id();
        app.update();
        let drawn = app.world().get::<Mesh3d>(wheel).unwrap().0.clone();
        assert_ne!(drawn, own, "the wheel draws its SpecialMesh");
        let m = app.world().resource::<Assets<Mesh>>().get(&drawn).unwrap();
        assert!(has_cap_along(m, Vec3::X));

        // Moving the wheel keeps the baked mesh.
        app.world_mut().get_mut::<Transform>(wheel).unwrap().translation = Vec3::new(5.0, 0.0, 0.0);
        app.update();
        assert_eq!(app.world().get::<Mesh3d>(wheel).unwrap().0, drawn);

        app.world_mut().despawn(mesh);
        app.update();
        app.update();
        assert_eq!(app.world().get::<Mesh3d>(wheel).unwrap().0, own, "without it, the wheel's own ball");
    }

    #[test]
    fn a_shape_edit_under_a_look_is_the_shape_that_comes_back() {
        let (mut app, part, _ball) = app_with_part(PartType::Ball);
        let mesh = app
            .world_mut()
            .spawn((SpecialMesh { mesh_type: MeshType::Brick, ..default() }, ChildOf(part)))
            .id();
        app.update();
        // A Shape edit draws the part as a block (Studio's Properties panel,
        // the Player's tree apply).
        let block = app.world_mut().resource_mut::<Assets<Mesh>>().add(primitive_mesh(PartType::Block));
        app.world_mut().get_mut::<Part>(part).unwrap().shape = PartType::Block;
        app.world_mut().get_mut::<Mesh3d>(part).unwrap().0 = block.clone();
        app.update();
        assert_ne!(app.world().get::<Mesh3d>(part).unwrap().0, block, "the look is drawn again over it");
        app.world_mut().despawn(mesh);
        app.update();
        app.update();
        assert_eq!(app.world().get::<Mesh3d>(part).unwrap().0, block, "the block, not the old ball");
    }

    #[test]
    fn a_data_mesh_changes_only_its_parent() {
        let (mut app, part, own) = app_with_part(PartType::Block);
        // A SpecialMesh inside a Model inside the part changes nothing.
        let model = app.world_mut().spawn((Instance::default(), ChildOf(part))).id();
        app.world_mut().spawn((SpecialMesh { mesh_type: MeshType::Sphere, ..default() }, ChildOf(model)));
        app.update();
        assert_eq!(app.world().get::<Mesh3d>(part).unwrap().0, own);
        // Through an anchor (no instance of its own) it does.
        let anchor = app.world_mut().spawn(ChildOf(part)).id();
        app.world_mut().spawn((SpecialMesh { mesh_type: MeshType::Sphere, ..default() }, ChildOf(anchor)));
        app.update();
        assert_ne!(app.world().get::<Mesh3d>(part).unwrap().0, own);
    }

    #[test]
    fn the_player_reads_the_look_from_the_tree() {
        let mut g = DataModel::new();
        let ws = g.get_service("Workspace").unwrap();
        let part = g.create("Part");
        g.set_parent(part, Some(ws)).unwrap();
        let mesh = g.create("SpecialMesh");
        g.set_parent(mesh, Some(part)).unwrap();
        g.set_prop(mesh, "MeshType", DmValue::Enum(crate::datamodel::EnumItem::new("MeshType", "Cylinder"))).unwrap();
        g.set_prop(mesh, "Scale", DmValue::Vector3(crate::scripting::Vector3::new(0.33, 1.0, 1.0))).unwrap();
        let look = look_from_tree(&g, part).unwrap();
        assert_eq!(look.kind, DataMeshKind::Special(MeshType::Cylinder));
        assert!((look.scale.x - 0.33).abs() < 1e-6);
        assert!(look_from_tree(&g, ws).is_none());
    }
}

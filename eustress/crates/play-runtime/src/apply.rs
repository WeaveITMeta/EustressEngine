//! DataModel -> ECS, once per frame after scripts run.

use std::collections::HashSet;

use bevy::prelude::*;

use avian3d::prelude::{
    AngularVelocity, Collider, CollisionEventsEnabled, ColliderDisabled, ComputedMass, Friction, LinearVelocity,
    Restitution, RigidBody, Sensor,
};

use eustress_common::avatar::abilities::AvatarAbilities;
use eustress_common::avatar::control::AvatarScriptControl;
use eustress_common::avatar::spawn::AvatarBody;
use eustress_common::classes::{BasePart, BillboardGui, ClassName, Instance, Material, Part, PartType, PropertyValue};
use eustress_common::datamodel::{
    is_base_part, is_gui_object, DataModel, DmValue, InstanceId, PhysicsCommand,
};
use eustress_common::gui::billboard_renderer::{BillboardGuiMarker, GuiElementDisplay};
use eustress_common::scripting::CFrame;

use eustress_common::play_session::{DataModelSpawned, FromTree, PlayDataModel, PlayEventCursor, PlayRole};

/// Marks an entity a script detached (`Parent = nil`): hidden and not
/// colliding until it is parented back into Workspace.
#[derive(Component, Debug, Clone, Copy)]
pub struct DetachedByScript;

/// Contact reporting was switched on for this part because a script
/// listens to its `Touched`; it is switched off again on Stop.
#[derive(Component, Debug, Clone, Copy)]
pub struct TouchReportingForPlay;

/// The position a script last wrote to an avatar's root `CFrame`, and
/// whether a repeat of it has been reported. A write that repeats it turns
/// the body and never moves it.
#[derive(Component, Debug, Clone, Copy)]
pub struct ScriptRootWrite {
    pub position: Vec3,
    pub repeat_reported: bool,
}

/// Classes that get an entity. Everything else (values, bindables,
/// humanoids, sounds, scripts) lives in the tree only; the systems that
/// need them read the tree. The Terrain is a BasePart, but the terrain root
/// draws and collides it.
fn spawns_entity(class: &str) -> bool {
    (is_base_part(class) && class != "Terrain")
        || matches!(
            class,
            "Model" | "Folder" | "Configuration" | "Actor" | "ScreenGui" | "BillboardGui" | "PointLight" | "SpotLight" | "SurfaceLight"
        )
        || (is_gui_object(class) && class != "ViewportFrame")
}

/// What a replica's draw step reads once a frame: the Models some Player's
/// `Character` points at (the avatar runtime draws those), and the rigged
/// Models, whose parts nobody climbs by default, found when first needed.
struct ReplicaView {
    characters: HashSet<InstanceId>,
    rigs: Option<HashSet<InstanceId>>,
}

impl ReplicaView {
    fn new(g: &DataModel) -> Self {
        Self { characters: eustress_common::space_read::character_models(g), rigs: None }
    }

    /// A character Model, or inside one.
    fn in_character(&self, g: &DataModel, id: InstanceId) -> bool {
        self.characters.contains(&id) || eustress_common::space_read::inside_any(g, id, &self.characters)
    }

    fn in_rig(&mut self, g: &DataModel, id: InstanceId) -> bool {
        let rigs = self.rigs.get_or_insert_with(|| eustress_common::space_read::rigged_models(g));
        eustress_common::space_read::inside_any(g, id, rigs)
    }
}

/// A replica's DataMeshes (`SpecialMesh`, `BlockMesh`, `CylinderMesh`,
/// `FileMesh`), each with the part it dresses, so one destroyed or moved
/// gives its part its own shape back. Checked only in frames where the
/// tree changed shape. Studio links loaded DataMeshes through their own
/// entities (`data_mesh::link_data_mesh_children`).
#[derive(Resource, Default)]
struct ReplicaDataMeshes {
    version: u64,
    parts: std::collections::HashMap<InstanceId, InstanceId>,
}

fn is_data_mesh(g: &DataModel, id: InstanceId) -> bool {
    g.class_of(id).is_some_and(|class| eustress_common::data_mesh::DataMeshKind::of(class, None).is_some())
}

/// Each part to look again wears its DataMesh (`eustress_common::data_mesh`),
/// or its own shape. Only parts this replica drew.
fn relook_data_meshes(
    world: &mut World,
    dm: &eustress_common::datamodel::SharedDataModel,
    parts: &HashSet<InstanceId>,
    spawned_now: &HashSet<InstanceId>,
    watch: &mut ReplicaDataMeshes,
) {
    let g = dm.lock();
    for &part in parts {
        if !g.class_of(part).is_some_and(|c| is_base_part(c)) {
            continue;
        }
        let Some(e) = g.entity_of(part).map(Entity::from_bits) else { continue };
        if world.get::<FromTree>(e).is_none() {
            continue;
        }
        for child in g.children(part) {
            if is_data_mesh(&g, *child) {
                watch.parts.insert(*child, part);
            }
        }
        let look = eustress_common::data_mesh::look_from_tree(&g, part);
        let Ok(mut ec) = world.get_entity_mut(e) else { continue };
        match look {
            Some(look) => {
                ec.insert(look);
            }
            // A part drawn this frame has no look to take off.
            None if spawned_now.contains(&part) => {}
            None => {
                ec.remove::<eustress_common::data_mesh::DataMeshLook>();
            }
        }
    }
}

/// The body a part gets. Anchored parts are static on both sides; an
/// unanchored one simulates on the authority, and on a replica moves only
/// where the host's writes and the motion lane put it.
fn body_for(anchored: bool, role: PlayRole) -> RigidBody {
    match (anchored, role) {
        (true, _) => RigidBody::Static,
        (false, PlayRole::Authority) => RigidBody::Dynamic,
        (false, PlayRole::Replica) => RigidBody::Kinematic,
    }
}

/// The ECS parent an instance hangs from. A replica's parts are flat: a
/// part's `CFrame` is its world pose, as the motion lane writes it. GUI
/// objects and billboards keep their parent on both sides, since layout and
/// anchoring read it.
fn ecs_parent(g: &DataModel, id: InstanceId, class: &str, role: PlayRole) -> Option<Entity> {
    if class == "BillboardGui" {
        return billboard_anchor(g, id);
    }
    if role == PlayRole::Replica && is_base_part(class) {
        return None;
    }
    parent_entity(g, id)
}

/// A replica gives back an entity it drew, and only such an entity; the
/// instance is unbound so a later destroy queues nothing. An entity bound
/// from elsewhere (an avatar) is left alone.
fn undraw(world: &mut World, g: &mut DataModel, id: InstanceId) {
    let Some(e) = g.entity_of(id).map(Entity::from_bits) else { return };
    if world.get::<FromTree>(e).is_none() {
        return;
    }
    g.unbind_entity(id);
    if let Ok(ent) = world.get_entity_mut(e) {
        ent.despawn();
    }
}

/// A pose Avian accepts: a non-finite position or rotation, or a zero or
/// non-finite extent, panics its solver. A bad write draws at the origin,
/// unrotated, at least a micrometre across.
fn sane(t: Transform) -> Transform {
    eustress_common::space_read::clean_pose(t.translation.to_array(), t.rotation.to_array(), t.scale.to_array())
}

/// The frame's end: the tree's event log trimmed to what the session's
/// script VM has read ([`PlayEventCursor`]), or emptied when no VM runs,
/// and the destroyed slots released.
pub fn end_frame(dm: Option<Res<PlayDataModel>>, cursor: Option<Res<PlayEventCursor>>) {
    let Some(dm) = dm else { return };
    let mut g = dm.dm.lock();
    let upto = cursor.and_then(|c| c.0).unwrap_or_else(|| g.event_cursor());
    g.trim_events(upto);
    g.end_frame();
}

pub fn apply_frame(world: &mut World) {
    let Some(dm) = world.get_resource::<PlayDataModel>().map(|r| r.dm.clone()) else { return };
    let role = PlayRole::of(world);
    let authority = role == PlayRole::Authority;
    let (spawns, reparents, dirty, despawns, physics, touch, version) = {
        let mut g = dm.lock();
        if !authority {
            // A replica simulates nothing: NPC moves and terrain edits are the
            // host's, so their queues are emptied each frame, or they would
            // grow for as long as the world is open.
            g.humanoid_commands.clear();
            g.terrain_commands.clear();
        }
        (
            g.take_spawns(),
            g.take_reparents(),
            g.take_dirty(),
            g.take_despawns(),
            std::mem::take(&mut g.physics_commands),
            std::mem::take(&mut g.touch_watch),
            g.structure_version,
        )
    };
    // A hosted session replicates the same writes to its players.
    if !dirty.is_empty() {
        if let Some(mut tap) = world.get_resource_mut::<eustress_networking::repl::ReplicationTap>() {
            tap.dirty.extend(dirty.iter().cloned());
        }
    }
    if spawns.is_empty()
        && reparents.is_empty()
        && dirty.is_empty()
        && despawns.is_empty()
        && physics.is_empty()
        && touch.is_empty()
        // A replica's DataMesh destroyed or moved changes only the tree's shape.
        && world.get_resource::<ReplicaDataMeshes>().map_or(true, |w| w.version == version)
    {
        return;
    }

    let mut view = (!authority).then(|| ReplicaView::new(&dm.lock()));
    let mut meshes = (!authority).then(|| world.remove_resource::<ReplicaDataMeshes>().unwrap_or_default());
    // Parts whose DataMesh look is read again at the end of this step.
    let mut relook: HashSet<InstanceId> = HashSet::new();
    let mut spawned_now: HashSet<InstanceId> = HashSet::new();
    for id in spawns {
        if let Some(e) = spawn_instance(world, &dm, id, view.as_mut()) {
            dm.lock().bind_entity(id, e.to_bits());
            spawned_now.insert(id);
            if meshes.is_some() {
                relook.insert(id);
            }
        } else if meshes.is_some() {
            // A DataMesh draws nothing itself: it changes its part's look.
            let g = dm.lock();
            if is_data_mesh(&g, id) {
                if let Some(parent) = g.parent(id) {
                    relook.insert(parent);
                }
            }
        }
    }

    // A DataMesh destroyed or moved: its old part, and any new one, look again.
    if let Some(watch) = meshes.as_mut() {
        let g = dm.lock();
        if g.structure_version != watch.version {
            watch.version = g.structure_version;
            let gone: Vec<(InstanceId, InstanceId)> = watch
                .parts
                .iter()
                .filter(|(mesh, part)| !g.exists(**mesh) || g.parent(**mesh) != Some(**part))
                .map(|(mesh, part)| (*mesh, *part))
                .collect();
            for (mesh, part) in gone {
                watch.parts.remove(&mesh);
                relook.insert(part);
                if let Some(parent) = g.parent(mesh).filter(|_| g.exists(mesh)) {
                    relook.insert(parent);
                }
            }
        }
    }

    // Physics reports contacts only for colliders that ask, so a part gets
    // reporting once a script listens to its Touched. A replica reports none:
    // the host runs Touched.
    for id in touch.into_iter().filter(|_| authority) {
        let Some(e) = dm.lock().entity_of(id).map(Entity::from_bits) else { continue };
        let Ok(mut ec) = world.get_entity_mut(e) else { continue };
        if !ec.contains::<CollisionEventsEnabled>() {
            ec.insert((CollisionEventsEnabled, TouchReportingForPlay));
        }
    }

    for id in reparents {
        relink(world, &dm, id, view.as_ref());
    }

    for (id, props) in dirty {
        // A write to a DataMesh (Scale, Offset, MeshType, MeshId).
        if let Some(part) = meshes.as_ref().and_then(|w| w.parts.get(&id).copied()) {
            relook.insert(part);
        }
        if spawned_now.contains(&id) {
            continue; // spawned from its current props already
        }
        apply_props(world, &dm, id, &props, view.as_mut());
    }

    // Impulses move bodies the authority simulates; a replica has none.
    for cmd in physics.into_iter().filter(|_| authority) {
        let (part, impulse, angular) = match cmd {
            PhysicsCommand::ApplyImpulse { part, impulse } => (part, impulse, false),
            PhysicsCommand::ApplyAngularImpulse { part, impulse } => (part, impulse, true),
        };
        let Some(e) = dm.lock().entity_of(part).map(Entity::from_bits) else { continue };
        let inv_mass = world.get::<ComputedMass>(e).map(|m| m.inverse()).unwrap_or(1.0 / 50.0);
        let dv = impulse.to_vec3() * inv_mass;
        if angular {
            if let Some(mut w) = world.get_mut::<AngularVelocity>(e) {
                w.0 += dv;
            }
        } else if let Some(mut v) = world.get_mut::<LinearVelocity>(e) {
            v.0 += dv;
        }
    }

    if let Some(watch) = meshes.as_mut() {
        relook_data_meshes(world, &dm, &relook, &spawned_now, watch);
    }

    for bits in despawns {
        let e = Entity::from_bits(bits);
        // The local avatar's body stands in for HumanoidRootPart, and the
        // avatar runtime owns its lifetime: a script destroying the local
        // character (`player.Character:Destroy()`) asks the runtime to remove
        // the body and its camera, and the play camera takes over.
        if world.get::<AvatarBody>(e).is_some() {
            if world.get::<eustress_common::avatar::LocalAvatar>(e).is_some() {
                world.write_message(eustress_common::avatar::DespawnAllAvatars);
            }
            continue;
        }
        // A replica gives back only what it drew (rule 3).
        if !authority && world.get::<FromTree>(e).is_none() {
            continue;
        }
        if let Ok(ent) = world.get_entity_mut(e) {
            ent.despawn();
        }
    }
    if let Some(watch) = meshes {
        world.insert_resource(watch);
    }
}

/// The ECS parent for an instance: its nearest ancestor with an entity.
fn parent_entity(g: &DataModel, id: InstanceId) -> Option<Entity> {
    let mut cur = g.parent(id);
    while let Some(p) = cur {
        if let Some(e) = g.entity_of(p) {
            return Some(Entity::from_bits(e));
        }
        cur = g.parent(p);
    }
    None
}

/// Where a BillboardGui hangs in the ECS: its `Adornee` when that has an
/// entity (Roblox draws the billboard there, wherever the gui itself sits,
/// which is how a PlayerGui billboard labels a part), else its nearest
/// ancestor with one.
fn billboard_anchor(g: &DataModel, id: InstanceId) -> Option<Entity> {
    g.get_prop(id, "Adornee")
        .and_then(|v| v.as_instance())
        .and_then(|a| g.entity_of(a))
        .map(Entity::from_bits)
        .or_else(|| parent_entity(g, id))
}

/// Local transform for a world pose under `parent`, with `scale` as the
/// part's size (unit meshes).
/// The local transform that puts a part at world pose `cf` with world scale
/// `scale` under `parent`.
fn local_transform(world: &World, parent: Option<Entity>, cf: &CFrame, scale: Vec3) -> Transform {
    let world_tf = cf.to_transform();
    match parent.and_then(|p| world_affine(world, p)) {
        Some(pa) => {
            let m = pa.inverse() * world_tf.compute_affine();
            let (_, r, t) = m.to_scale_rotation_translation();
            Transform { translation: t, rotation: r, scale: scale / parent_scale(world, parent) }
        }
        None => Transform { scale, ..world_tf },
    }
}

/// An entity's world transform, composed up its `Transform` chain rather than
/// read from `GlobalTransform`: a parent spawned earlier in the same apply
/// pass has not been propagated yet, and its identity `GlobalTransform` put
/// the child at the wrong place.
fn world_affine(world: &World, entity: Entity) -> Option<bevy::math::Affine3A> {
    let mut affine = world.get::<Transform>(entity)?.compute_affine();
    let mut cur = world.get::<ChildOf>(entity).map(|c| c.parent());
    for _ in 0..256 {
        let Some(p) = cur else { break };
        let Some(t) = world.get::<Transform>(p) else { break };
        affine = t.compute_affine() * affine;
        cur = world.get::<ChildOf>(p).map(|c| c.parent());
    }
    Some(affine)
}

/// The world scale a child inherits from `parent`. A part's scale IS its
/// size, so a part under a part came out that many times too big (a 4 x 1 x 2
/// base stretched every child to match) until child scales divided it back
/// out. Exact while the two are axis-aligned; Bevy has no shear for a rotated
/// child of a stretched parent.
fn parent_scale(world: &World, parent: Option<Entity>) -> Vec3 {
    let s = parent
        .and_then(|p| world_affine(world, p))
        .map(|a| a.to_scale_rotation_translation().0)
        .unwrap_or(Vec3::ONE);
    let safe = |v: f32| if v.abs() > 1.0e-6 { v } else { 1.0 };
    Vec3::new(safe(s.x), safe(s.y), safe(s.z))
}

fn shape_of(name: Option<&str>, class: &str) -> PartType {
    match class {
        "WedgePart" => return PartType::Wedge,
        "CornerWedgePart" => return PartType::CornerWedge,
        _ => {}
    }
    name.and_then(PartType::from_str).unwrap_or(PartType::Block)
}

pub use eustress_common::play_session::PlayAssetRoot;

fn play_asset_root(world: &World) -> std::path::PathBuf {
    world.get_resource::<PlayAssetRoot>().map(|r| r.0.clone()).unwrap_or_default()
}

/// `MeshId` as an asset URL: Space-relative paths go through `space://`,
/// a missing `#Mesh…` label gets the first primitive.
pub fn mesh_url(mesh_id: &str, space_root: &std::path::Path) -> String {
    let mut path = mesh_id.trim().replace('\\', "/");
    let label = if path.contains('#') { String::new() } else { "#Mesh0/Primitive0".to_string() };
    if !path.contains("://") && !path.starts_with("parts/") {
        let root_s = space_root.to_string_lossy().replace('\\', "/");
        if let Some(rel) = path.strip_prefix(&format!("{}/", root_s.trim_end_matches('/'))) {
            path = rel.to_string();
        }
        path = format!("space://{}", path.trim_start_matches('/'));
    }
    format!("{}{}", path, label)
}

/// The engine's unit primitives (`parts/block.glb` ...). Their
/// `Transform.scale` IS the part's size; a custom mesh's scale multiplies
/// the mesh's own (natural) size instead.
fn is_primitive_mesh(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    let file = p.split('#').next().unwrap_or("").rsplit('/').next().unwrap_or("");
    matches!(file, "block.glb" | "ball.glb" | "cylinder.glb" | "wedge.glb" | "corner_wedge.glb" | "cone.glb")
}

/// The Transform scale that shows a part at `size`. Primitives are unit
/// meshes; a custom mesh's natural size is recovered from its current
/// size and scale.
fn scale_for_size(size: Vec3, primitive: bool, current_size: Vec3, current_scale: Vec3) -> Vec3 {
    if primitive {
        return size;
    }
    let natural = (current_size / current_scale.max(Vec3::splat(1.0e-6))).max(Vec3::splat(1.0e-6));
    size / natural
}

fn num(p: &DmValue) -> Option<f32> {
    p.as_number().map(|n| n as f32)
}

fn color_of(c: &DmValue) -> Option<Color> {
    c.as_color3().map(|c| Color::srgb(c.r as f32, c.g as f32, c.b as f32))
}

fn spawn_instance(
    world: &mut World,
    dm: &eustress_common::datamodel::SharedDataModel,
    id: InstanceId,
    mut view: Option<&mut ReplicaView>,
) -> Option<Entity> {
    let role = if view.is_some() { PlayRole::Replica } else { PlayRole::Authority };
    let (class, name, props, clone_src, parent, visible, climb) = {
        let g = dm.lock();
        let inst = g.get(id)?;
        let class = inst.class_name.clone();
        if !spawns_entity(&class) {
            return None;
        }
        // A replica leaves characters to the avatar runtime (rule 2).
        if view.as_deref().is_some_and(|v| v.in_character(&g, id)) {
            return None;
        }
        // A replica's climb mark comes from the tree's attributes and rigs:
        // the entity gets empty Attributes, and a Humanoid no entity.
        let climb = match view.as_deref_mut() {
            Some(v) if is_base_part(&class) => eustress_common::space_read::SpacePart::from_tree(&g, id).and_then(|mut part| {
                part.in_character = v.in_rig(&g, id);
                eustress_common::space_read::climb_mark(&part).filter(|_| part.can_collide)
            }),
            _ => None,
        };
        let clone_src = inst.clone_of.and_then(|s| g.entity_of(s)).map(Entity::from_bits);
        let parent = ecs_parent(&g, id, &class, role);
        let service = g.service_of(id).and_then(|s| g.class_of(s).map(str::to_string)).unwrap_or_default();
        let visible = matches!(service.as_str(), "Workspace" | "Players" | "Lighting" | "StarterGui");
        (class, inst.name.clone(), inst.props.clone(), clone_src, parent, visible, climb)
    };
    let clone_src = clone_src.filter(|e| world.get_entity(*e).is_ok());
    let get = |k: &str| props.get(k);

    let entity = if is_base_part(&class) {
        let size = get("Size").and_then(DmValue::as_vector3).map(|v| v.to_vec3()).unwrap_or(Vec3::new(4.0, 1.0, 2.0));
        let cf = get("CFrame").and_then(DmValue::as_cframe).unwrap_or_default();
        let shape = shape_of(get("Shape").and_then(DmValue::as_enum_name), &class);
        let mesh_id = get("MeshId").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        let material = get("Material").and_then(DmValue::as_enum_name).unwrap_or("Plastic").to_string();
        let transparency = get("Transparency").and_then(num).unwrap_or(0.0);
        let anchored = get("Anchored").and_then(DmValue::as_bool).unwrap_or(false);
        let can_collide = get("CanCollide").and_then(DmValue::as_bool).unwrap_or(true);
        // A clone weighs as its template does; any other part, as its material.
        let density = clone_src
            .and_then(|s| world.get::<BasePart>(s))
            .map(|b| b.density)
            .filter(|d| d.is_finite() && *d > 0.0)
            .unwrap_or_else(|| BasePart::material_default_density(&Material::from_string(&material)));
        let base = BasePart {
            cframe: cf.to_transform(),
            size,
            color: get("Color").and_then(color_of).unwrap_or(Color::srgb_u8(163, 162, 165)),
            material: Material::from_string(&material),
            material_name: material.clone(),
            transparency,
            reflectance: get("Reflectance").and_then(num).unwrap_or(0.0),
            anchored,
            can_collide,
            can_touch: get("CanTouch").and_then(DmValue::as_bool).unwrap_or(true),
            cast_shadow: get("CastShadow").and_then(DmValue::as_bool).unwrap_or(true),
            density,
            mass: density * size.x * size.y * size.z,
            custom_physical_properties: clone_src.and_then(|s| world.get::<BasePart>(s)).and_then(|b| b.custom_physical_properties),
            ..Default::default()
        };

        // Mesh, material and collider: the clone source's when there is one
        // (so a template MeshPart keeps its mesh), else from the properties.
        let (mesh, mat, collider, mesh_source) = {
            let src_mesh = clone_src.and_then(|s| world.get::<Mesh3d>(s).cloned());
            let src_mat = clone_src.and_then(|s| world.get::<MeshMaterial3d<StandardMaterial>>(s).cloned());
            let src_collider = clone_src.and_then(|s| world.get::<Collider>(s).cloned());
            let src_source = clone_src.and_then(|s| world.get::<eustress_common::part_draw::MeshSource>(s).cloned());
            let asset_server = world.resource::<AssetServer>().clone();
            let (mesh, source) = match (src_mesh, src_source) {
                (Some(m), Some(s)) if mesh_id.is_empty() || s.path == mesh_id => (m, s),
                _ => {
                    if !mesh_id.is_empty() {
                        (Mesh3d(asset_server.load(mesh_url(&mesh_id, &play_asset_root(world)))), eustress_common::part_draw::MeshSource::new(mesh_id.clone()))
                    } else {
                        let glb = eustress_common::part_draw::part_type_to_glb_path(&shape);
                        (Mesh3d(asset_server.load(format!("{}#Mesh0/Primitive0", glb))), eustress_common::part_draw::MeshSource::new(glb))
                    }
                }
            };
            let mat = match src_mat {
                Some(m) => m,
                None => {
                    let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: base.color,
                        alpha_mode: if transparency > 0.0 { AlphaMode::Blend } else { AlphaMode::Opaque },
                        ..default()
                    }))
                }
            };
            let primitive = is_primitive_mesh(&source.path);
            let collider = src_collider.unwrap_or_else(|| match shape {
                // A custom mesh with no template to copy from: a box of the
                // part's size (its scale stays 1 below).
                _ if !primitive => Collider::cuboid(size.x, size.y, size.z),
                PartType::Ball => Collider::sphere(0.5),
                PartType::Cylinder | PartType::Cone => Collider::cylinder(0.5, 1.0),
                _ => Collider::cuboid(1.0, 1.0, 1.0),
            });
            (mesh, mat, collider, source)
        };

        // Primitives: scale = size. A cloned custom mesh keeps its
        // template's natural size; one made from a bare MeshId shows at its
        // natural size.
        let primitive = is_primitive_mesh(&mesh_source.path);
        let scale = if primitive {
            size
        } else {
            match clone_src.and_then(|s| Some((world.get::<BasePart>(s)?.size, world.get::<Transform>(s)?.scale))) {
                Some((src_size, src_scale)) => scale_for_size(size, false, src_size, src_scale),
                None => Vec3::ONE,
            }
        };
        let transform = sane(local_transform(world, parent, &cf, scale));
        // A clone grips and bounces as its template does.
        let src_contact = clone_src.map(|s| (world.get::<Friction>(s).copied(), world.get::<Restitution>(s).copied()));
        let class_enum = ClassName::from_str(&class).unwrap_or(ClassName::Part);
        let mut ec = world.spawn((
            mesh,
            mat,
            transform,
            if visible { Visibility::Inherited } else { Visibility::Hidden },
            Instance { name: name.clone(), class_name: class_enum, archivable: true, id: 0, uuid: String::new(), ai: false },
            base,
            Part { shape },
            Name::new(name.clone()),
            mesh_source,
            eustress_common::Attributes::new(),
            eustress_common::Tags::new(),
            DataModelSpawned,
        ));
        // A part that neither collides, nor reports touches, nor answers
        // queries is not in the physics world at all (Roblox semantics): a
        // pooled tracer or gib costs a mesh and nothing else.
        let can_touch = get("CanTouch").and_then(DmValue::as_bool).unwrap_or(true);
        let can_query = get("CanQuery").and_then(DmValue::as_bool).unwrap_or(true);
        let physical = can_collide || can_touch || can_query;
        if physical {
            ec.insert((collider, body_for(anchored, role), CollisionEventsEnabled));
            if let Some((friction, restitution)) = src_contact {
                if let Some(f) = friction {
                    ec.insert(f);
                }
                if let Some(r) = restitution {
                    ec.insert(r);
                }
            }
            if !can_collide {
                ec.insert(Sensor);
            }
            if !visible {
                ec.insert((ColliderDisabled, DetachedByScript));
            }
            if !anchored && role == PlayRole::Authority {
                ec.insert(eustress_common::play_session::PlayModePhysicsActivated);
            }
        } else if !visible {
            ec.insert(DetachedByScript);
        }
        let cast_shadow = get("CastShadow").and_then(DmValue::as_bool).unwrap_or(true);
        if transparency >= 0.5 || !cast_shadow {
            ec.insert(bevy::light::NotShadowCaster);
        }
        let e = ec.id();
        let part_id = format!("{}v{}", e.index(), e.generation());
        world.entity_mut(e).insert(eustress_common::part_draw::PartEntity { part_id });
        e
    } else if is_gui_object(&class) || class == "ScreenGui" {
        let mut display = clone_src
            .and_then(|s| world.get::<GuiElementDisplay>(s).cloned())
            .unwrap_or_else(|| blank_display(&class));
        display.class_type = class.clone();
        // Roblox only lets buttons (and Active objects) sink clicks; a label
        // over the world must not swallow the mouse.
        display.mouse_filter = if matches!(class.as_str(), "TextButton" | "ImageButton") {
            "stop".into()
        } else {
            "ignore".into()
        };
        for (k, v) in props.iter() {
            apply_gui_prop(&mut display, &class, k, v);
        }
        let class_enum = ClassName::from_str(&class).unwrap_or(ClassName::Frame);
        world
            .spawn((
                Instance { name: name.clone(), class_name: class_enum, archivable: true, id: 0, uuid: String::new(), ai: false },
                display,
                Name::new(name.clone()),
                DataModelSpawned,
            ))
            .id()
    } else if class == "BillboardGui" {
        // The class component is all the billboard systems need: they add the
        // marker's derived fields, the transform, the atlas slot and the quad,
        // and paint its GUI children (a TextLabel under it gets `ChildOf` to
        // this entity) into the atlas. No `GuiElementDisplay` here: the
        // renderer would paint it as the billboard's background.
        let mut bb = clone_src.and_then(|s| world.get::<BillboardGui>(s).cloned()).unwrap_or_default();
        for (k, v) in props.iter() {
            apply_billboard_prop(&mut bb, k, v);
        }
        if !visible {
            bb.enabled = false;
        }
        let mut ec = world.spawn((
            Transform::IDENTITY,
            Visibility::Inherited,
            Instance { name: name.clone(), class_name: ClassName::BillboardGui, archivable: true, id: 0, uuid: String::new(), ai: false },
            bb,
            BillboardGuiMarker::default(),
            Name::new(name.clone()),
            DataModelSpawned,
        ));
        if !visible {
            ec.insert(DetachedByScript);
        }
        ec.id()
    } else if matches!(class.as_str(), "PointLight" | "SpotLight" | "SurfaceLight") {
        // Each class keeps its own component: a SpotLight or SurfaceLight
        // shines a cone out of its part's Face (it used to become an omni
        // PointLight, so a later Angle or Face write went nowhere).
        let brightness = get("Brightness").and_then(num).unwrap_or(1.0).max(0.0);
        let color = get("Color").and_then(color_of).unwrap_or(Color::WHITE);
        let range = get("Range").and_then(num).unwrap_or(8.0).max(0.0);
        let shadows = get("Shadows").and_then(DmValue::as_bool).unwrap_or(false);
        let enabled = get("Enabled").and_then(DmValue::as_bool).unwrap_or(true);
        let angle = get("Angle").and_then(num).unwrap_or(90.0).clamp(0.0, 180.0);
        let face = match get("Face") {
            Some(DmValue::Enum(e)) => eustress_common::plugins::light_classes::normalize_face(&e.name),
            Some(DmValue::String(s)) => eustress_common::plugins::light_classes::normalize_face(s),
            Some(DmValue::Number(n)) => {
                eustress_common::plugins::light_classes::normalize_face(&(*n as i64).to_string())
            }
            _ => "Front",
        }
        .to_string();
        let class_enum = ClassName::from_str(&class).unwrap_or(ClassName::PointLight);
        let mut ec = world.spawn((
            Transform::IDENTITY,
            Visibility::Inherited,
            Instance { name: name.clone(), class_name: class_enum, archivable: true, id: 0, uuid: String::new(), ai: false },
            Name::new(name.clone()),
            DataModelSpawned,
        ));
        match class_enum {
            ClassName::SpotLight => {
                ec.insert(eustress_common::classes::EustressSpotLight {
                    brightness,
                    color,
                    range,
                    angle,
                    shadows,
                    enabled,
                    face,
                    texture: None,
                });
            }
            ClassName::SurfaceLight => {
                ec.insert(eustress_common::classes::SurfaceLight {
                    brightness,
                    color,
                    range,
                    face,
                    angle,
                    shadows,
                    enabled,
                    texture: None,
                });
            }
            _ => {
                ec.insert(eustress_common::classes::EustressPointLight {
                    brightness,
                    color,
                    range,
                    shadows,
                    enabled,
                    ..Default::default()
                });
            }
        }
        ec.id()
    } else {
        // Model / Folder / Configuration / Actor.
        let class_enum = ClassName::from_str(&class).unwrap_or(ClassName::Folder);
        let mut ec = world.spawn((
            Transform::IDENTITY,
            Visibility::Inherited,
            Instance { name: name.clone(), class_name: class_enum, archivable: true, id: 0, uuid: String::new(), ai: false },
            Name::new(name.clone()),
            eustress_common::Attributes::new(),
            eustress_common::Tags::new(),
            DataModelSpawned,
        ));
        if class == "Model" || class == "Actor" {
            ec.insert(eustress_common::classes::Model::default());
        } else {
            ec.insert(eustress_common::classes::Folder::default());
        }
        ec.id()
    };

    if let Some(p) = parent {
        if world.get_entity(p).is_ok() {
            world.entity_mut(entity).insert(ChildOf(p));
        }
    }
    if role == PlayRole::Replica {
        let mut ec = world.entity_mut(entity);
        ec.insert(FromTree(id));
        if let Some(mark) = climb {
            ec.insert(mark);
        }
    }
    Some(entity)
}

/// A bound instance changed parent: follow it in the ECS, keep its world
/// pose, and hide it while it is out of Workspace.
fn relink(world: &mut World, dm: &eustress_common::datamodel::SharedDataModel, id: InstanceId, view: Option<&ReplicaView>) {
    let role = if view.is_some() { PlayRole::Replica } else { PlayRole::Authority };
    // A replica: an instance moved into a character goes to the avatar runtime.
    if view.is_some_and(|v| v.in_character(&dm.lock(), id)) {
        undraw(world, &mut dm.lock(), id);
        return;
    }
    let (entity, parent, in_world, is_part, cf, billboard_enabled) = {
        let g = dm.lock();
        let Some(bits) = g.entity_of(id) else { return };
        let in_world = g.in_tree(id)
            && g.service_of(id)
                .and_then(|s| g.class_of(s))
                .map_or(false, |c| matches!(c, "Workspace" | "Players" | "Lighting"));
        let is_part = g.get(id).map_or(false, |i| is_base_part(&i.class_name));
        let cf = g.get(id).and_then(|i| i.cframe());
        // A billboard follows its Adornee, and is hidden through `Enabled`:
        // the billboard systems rewrite its `Visibility` from that every frame.
        let billboard = g.get(id).map_or(false, |i| i.class_name == "BillboardGui");
        let class = g.class_of(id).unwrap_or_default();
        let parent = ecs_parent(&g, id, class, role);
        let billboard_enabled =
            billboard.then(|| g.get_prop(id, "Enabled").and_then(|v| v.as_bool()).unwrap_or(true) && in_world);
        (Entity::from_bits(bits), parent, in_world, is_part, cf, billboard_enabled)
    };
    if world.get_entity(entity).is_err() || world.get::<AvatarBody>(entity).is_some() {
        return;
    }
    // A reparent never resizes: keep the part's WORLD scale, read while it is
    // still under the old parent (its local scale is relative to that one).
    let world_scale = world_affine(world, entity)
        .map(|a| a.to_scale_rotation_translation().0)
        .unwrap_or(Vec3::ONE);
    match parent {
        Some(p) if world.get_entity(p).is_ok() => {
            world.entity_mut(entity).insert(ChildOf(p));
        }
        _ => {
            world.entity_mut(entity).remove::<ChildOf>();
        }
    }
    if is_part {
        if let Some(cf) = cf {
            let t = sane(local_transform(world, parent, &cf, world_scale));
            if let Some(mut tf) = world.get_mut::<Transform>(entity) {
                *tf = t;
            }
        }
    }
    let mut ec = world.entity_mut(entity);
    if in_world {
        if ec.contains::<DetachedByScript>() {
            ec.remove::<(DetachedByScript, ColliderDisabled)>();
            if let Some(mut v) = ec.get_mut::<Visibility>() {
                *v = Visibility::Inherited;
            }
        }
    } else if !ec.contains::<DetachedByScript>() {
        ec.insert((DetachedByScript, ColliderDisabled));
        if let Some(mut v) = ec.get_mut::<Visibility>() {
            *v = Visibility::Hidden;
        }
    }
    if let Some(want) = billboard_enabled {
        if let Some(mut bb) = ec.get_mut::<BillboardGui>() {
            if bb.enabled != want {
                bb.enabled = want;
            }
        }
    }
}

fn apply_props(
    world: &mut World,
    dm: &eustress_common::datamodel::SharedDataModel,
    id: InstanceId,
    names: &[String],
    view: Option<&mut ReplicaView>,
) {
    let role = if view.is_some() { PlayRole::Replica } else { PlayRole::Authority };
    // A replica: a Player's `Character` now names a Model whose parts may be
    // drawn already; the avatar runtime draws them from now on (rule 2).
    if let Some(view) = view {
        if names.iter().any(|n| n == "Character") {
            let mut g = dm.lock();
            let model = (g.class_of(id) == Some("Player"))
                .then(|| g.get_prop(id, "Character").and_then(|v| v.as_instance()))
                .flatten();
            if let Some(model) = model {
                view.characters.insert(model);
                for d in g.descendants(model) {
                    undraw(world, &mut g, d);
                }
                undraw(world, &mut g, model);
            }
        }
    }
    let (class, entity, values, parent, humanoid_body) = {
        let g = dm.lock();
        let Some(inst) = g.get(id) else { return };
        let class = inst.class_name.clone();
        let values: Vec<(String, DmValue)> = names
            .iter()
            .filter_map(|n| g.get_prop(id, n).map(|v| (n.clone(), v)))
            .collect();
        // A Humanoid in the local character drives the avatar body.
        let humanoid_body = if class == "Humanoid" {
            g.parent(id)
                .and_then(|m| g.find_first_child(m, "HumanoidRootPart", false))
                .and_then(|r| g.entity_of(r))
                .map(Entity::from_bits)
        } else {
            None
        };
        let parent = ecs_parent(&g, id, &class, role);
        (class, inst.entity.map(Entity::from_bits), values, parent, humanoid_body)
    };

    // `game.Lighting` drives the renderer's lighting directly (a day-night
    // cycle); it has no entity of its own. Stop restores the snapshot the
    // session took.
    if class == "Lighting" {
        for (name, v) in &values {
            apply_lighting_prop(world, name, v);
        }
        return;
    }

    // `workspace.Gravity`, m/s² downward: a script's write, or a host's
    // replicated, sets Workspace.gravity, gravity's one owner; parts and
    // avatars then fall at it. Stop restores the Space's own.
    if class == "Workspace" {
        if let Some(down) = values.iter().find(|(name, _)| name == "Gravity").and_then(|(_, v)| num(v)) {
            if down.is_finite() {
                if let Some(mut ws) = world.get_resource_mut::<eustress_common::services::workspace::Workspace>() {
                    ws.gravity = Vec3::new(0.0, -down, 0.0);
                }
            }
        }
    }

    if let Some(body) = humanoid_body.filter(|b| world.get::<AvatarBody>(*b).is_some()) {
        for (name, v) in &values {
            match name.as_str() {
                "WalkSpeed" => {
                    if let (Some(speed), Some(mut ab)) = (num(v), world.get_mut::<AvatarBody>(body)) {
                        // Sprint keeps its ratio to walking (1.6 when walking
                        // was stopped and the ratio is gone).
                        let ratio = if ab.motion.walk_speed > 0.0 {
                            ab.motion.run_speed / ab.motion.walk_speed
                        } else {
                            1.6
                        };
                        let speed = speed.max(0.0);
                        ab.motion.walk_speed = speed;
                        ab.motion.run_speed = speed * ratio;
                    }
                }
                "AutoRotate" => {
                    let auto = v.as_bool().unwrap_or(true);
                    let mut ent = world.entity_mut(body);
                    match ent.get_mut::<AvatarScriptControl>() {
                        Some(mut c) => c.auto_rotate = auto,
                        None => {
                            ent.insert(AvatarScriptControl { auto_rotate: auto, facing_yaw: None });
                        }
                    }
                }
                // Metres from the ground to the top of the jump.
                "JumpHeight" => {
                    if let (Some(h), Some(mut ab)) = (num(v), world.get_mut::<AvatarBody>(body)) {
                        ab.motion.jump_apex_m = h.max(0.0);
                    }
                }
                // Under UseJumpPower, JumpPower is the take-off speed, m/s
                // (Roblox's rule); either change re-reads both.
                "JumpPower" | "UseJumpPower" => {
                    let launch = {
                        let g = dm.lock();
                        let on = g.get_prop(id, "UseJumpPower").and_then(|v| v.as_bool()).unwrap_or(false);
                        on.then(|| g.get_prop(id, "JumpPower").and_then(|v| v.as_number()).unwrap_or(0.0) as f32)
                    };
                    if let Some(mut ab) = world.get_mut::<AvatarBody>(body) {
                        ab.motion.jump_speed_mps = launch;
                    }
                }
                // JumpEnabled, ClimbingEnabled, VaultingEnabled, ...: this
                // character's movement verbs (StarterPlayer sets everyone's).
                n if AvatarAbilities::PROPERTIES.contains(&n) => {
                    if let Some(on) = v.as_bool() {
                        let mut ent = world.entity_mut(body);
                        match ent.get_mut::<AvatarAbilities>() {
                            Some(mut a) => {
                                a.set(n, on);
                            }
                            None => {
                                let mut a = AvatarAbilities::default();
                                a.set(n, on);
                                ent.insert(a);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        return;
    }

    let Some(entity) = entity else { return };
    if world.get_entity(entity).is_err() {
        return;
    }

    // The local character's root: position teleports, rotation faces.
    if world.get::<AvatarBody>(entity).is_some() {
        for (name, v) in &values {
            if name != "CFrame" {
                continue;
            }
            let Some(cf) = v.as_cframe() else { continue };
            let target = cf.position.to_vec3();
            let look = cf.look_vector();
            let yaw = if look.x.abs() + look.z.abs() > 1e-6 { (-(look.x as f32)).atan2(-(look.z as f32)) } else { 0.0 };
            // Only a position the script CHANGED moves the body. A script
            // that turns its character (`CFrame.lookAt(root.Position, ...)`)
            // writes back the position it read; if that read ever trails the
            // body, re-applying it would snap the body back each frame and
            // cancel its fall.
            let previous = world.get::<ScriptRootWrite>(entity).copied();
            let changed = previous.map_or(true, |w| w.position.distance(target) > 0.001);
            let mut teleported = false;
            let mut repeat_reported = previous.is_some_and(|w| w.repeat_reported);
            if let Some(mut tf) = world.get_mut::<Transform>(entity) {
                let away = tf.translation.distance(target);
                if away > 0.05 && changed {
                    info!(
                        "avatar root: a script moved the body to {target:?} from {:?} ({away:.3} m)",
                        tf.translation
                    );
                    tf.translation = target;
                    teleported = true;
                } else if away > 0.05 && !repeat_reported {
                    repeat_reported = true;
                    info!(
                        "avatar root: a script wrote its previous position {target:?} again while the body is at {:?} ({away:.3} m away): turned, not moved",
                        tf.translation
                    );
                }
            }
            world
                .entity_mut(entity)
                .insert(ScriptRootWrite { position: target, repeat_reported });
            if teleported {
                if let Some(mut lv) = world.get_mut::<LinearVelocity>(entity) {
                    lv.0 = Vec3::ZERO;
                }
            }
            let mut ent = world.entity_mut(entity);
            match ent.get_mut::<AvatarScriptControl>() {
                Some(mut c) => c.facing_yaw = Some(yaw),
                None => {
                    ent.insert(AvatarScriptControl { auto_rotate: true, facing_yaw: Some(yaw) });
                }
            }
        }
        return;
    }

    let mut fallback: Vec<(String, DmValue)> = Vec::new();
    let mut reanchor = false;
    for (name, v) in values {
        let handled = if is_base_part(&class) {
            apply_part_prop(world, entity, parent, &name, &v, role)
        } else if is_gui_object(&class) || class == "ScreenGui" {
            match world.get_mut::<GuiElementDisplay>(entity) {
                Some(mut d) => apply_gui_prop(&mut d, &class, &name, &v),
                None => false,
            }
        } else if name == "Name" {
            if let Some(s) = v.as_str() {
                if let Some(mut i) = world.get_mut::<Instance>(entity) {
                    i.name = s.to_string();
                }
                if let Some(mut n) = world.get_mut::<Name>(entity) {
                    n.set(s.to_string());
                }
            }
            true
        } else if class == "BillboardGui" {
            if name == "Adornee" {
                reanchor = true;
            }
            // A billboard out of the world stays off whatever `Enabled` says.
            let detached = world.get::<DetachedByScript>(entity).is_some();
            match world.get_mut::<BillboardGui>(entity) {
                Some(mut bb) => {
                    let known = apply_billboard_prop(&mut bb, &name, &v);
                    if detached && bb.enabled {
                        bb.enabled = false;
                    }
                    known
                }
                None => false,
            }
        } else {
            false
        };
        if !handled {
            fallback.push((name, v));
        }
    }

    // A new Adornee moves the billboard onto it (or back under its parent).
    if reanchor {
        let anchor = billboard_anchor(&dm.lock(), id);
        match anchor {
            Some(p) if p != entity && world.get_entity(p).is_ok() => {
                world.entity_mut(entity).insert(ChildOf(p));
            }
            _ => {
                world.entity_mut(entity).remove::<ChildOf>();
            }
        }
    }

    for (name, v) in fallback {
        if name.starts_with("__") {
            continue;
        }
        let Some(pv) = dm_to_property(&v) else { continue };
        let old = eustress_common::property_command::PropertyCommand::read_property(world, entity, &name).unwrap_or_else(|| pv.clone());
        let cmd = eustress_common::property_command::PropertyCommand::new(entity, name.clone(), old, pv);
        if let Err(e) = cmd.execute(world) {
            debug!("DataModel: {}.{} not applied to the scene: {}", class, name, e);
        }
    }
}

/// One `game.Lighting` property onto the renderer's `LightingService`.
fn apply_lighting_prop(world: &mut World, name: &str, v: &DmValue) {
    let Some(mut l) = world.get_resource_mut::<eustress_common::services::lighting::LightingService>() else {
        return;
    };
    let rgba = |v: &DmValue| v.as_color3().map(|c| [c.r as f32, c.g as f32, c.b as f32, 1.0]);
    match name {
        "ClockTime" => {
            if let Some(hours) = num(v) {
                let tod = (hours / 24.0).rem_euclid(1.0);
                let total = (tod * 86_400.0).round() as u32 % 86_400;
                l.time_of_day = tod;
                l.clock_time = format!("{:02}:{:02}:{:02}", total / 3600, (total / 60) % 60, total % 60);
            }
        }
        "Brightness" => {
            if let Some(b) = num(v) {
                l.brightness = b.max(0.0);
            }
        }
        "ExposureCompensation" => {
            if let Some(e) = num(v) {
                l.exposure_compensation = e;
            }
        }
        "GlobalShadows" => {
            if let Some(b) = v.as_bool() {
                l.shadows_enabled = b;
            }
        }
        "FogStart" => {
            if let Some(f) = num(v) {
                l.fog_start = f;
            }
        }
        "FogEnd" => {
            if let Some(f) = num(v) {
                l.fog_end = f;
                l.fog_enabled = f < 100_000.0;
            }
        }
        "FogColor" => {
            if let Some(c) = rgba(v) {
                l.fog_color = c;
            }
        }
        "Ambient" => {
            if let Some(c) = rgba(v) {
                l.ambient = c;
            }
        }
        "OutdoorAmbient" => {
            if let Some(c) = rgba(v) {
                l.outdoor_ambient = c;
            }
        }
        _ => {}
    }
}

/// Part properties. Returns false for ones the generic path should take.
fn apply_part_prop(world: &mut World, e: Entity, parent: Option<Entity>, name: &str, v: &DmValue, role: PlayRole) -> bool {
    match name {
        "CFrame" => {
            let Some(cf) = v.as_cframe() else { return true };
            let scale = world.get::<Transform>(e).map(|t| t.scale).unwrap_or(Vec3::ONE);
            let t = sane(local_transform(world, parent, &cf, scale));
            if let Some(mut tf) = world.get_mut::<Transform>(e) {
                tf.translation = t.translation;
                tf.rotation = t.rotation;
            }
            // `BasePart.cframe` is left alone: writing BasePart every frame a
            // script moves a part would wake every Changed<BasePart> consumer
            // (material sync, collider rebuild) for a pose change.
            true
        }
        "Size" => {
            let Some(size) = v.as_vector3() else { return true };
            let s = size.to_vec3().max(Vec3::splat(1.0e-3));
            let primitive = world.get::<eustress_common::part_draw::MeshSource>(e).map_or(true, |m| is_primitive_mesh(&m.path));
            let current_scale = world.get::<Transform>(e).map(|t| t.scale).unwrap_or(Vec3::ONE);
            let current_size = world.get::<BasePart>(e).map(|b| b.size).unwrap_or(current_scale);
            let mut scale = scale_for_size(s, primitive, current_size, current_scale);
            if primitive {
                // Size is a world size; the parent's scale reaches the part
                // through propagation (a custom mesh's ratio is already local).
                scale /= parent_scale(world, parent);
            }
            if let Some(mut tf) = world.get_mut::<Transform>(e) {
                tf.scale = scale;
            }
            if let Some(mut bp) = world.get_mut::<BasePart>(e) {
                bp.size = s;
            }
            true
        }
        "Color" => {
            if let (Some(c), Some(mut bp)) = (color_of(v), world.get_mut::<BasePart>(e)) {
                bp.color = c;
            }
            true
        }
        "Transparency" => {
            let t = num(v).unwrap_or(0.0);
            if let Some(mut bp) = world.get_mut::<BasePart>(e) {
                bp.transparency = t;
            }
            true
        }
        "Reflectance" => {
            if let (Some(r), Some(mut bp)) = (num(v), world.get_mut::<BasePart>(e)) {
                bp.reflectance = r;
            }
            true
        }
        "Material" => {
            if let Some(m) = v.as_enum_name().map(str::to_string) {
                if let Some(mut bp) = world.get_mut::<BasePart>(e) {
                    bp.material = Material::from_string(&m);
                    bp.material_name = m;
                    // Its authored density stays; without one, it weighs its new material (Roblox).
                    let density = bp.effective_density();
                    bp.set_density(density);
                }
            }
            true
        }
        "Anchored" => {
            let Some(b) = v.as_bool() else { return true };
            if let Some(mut bp) = world.get_mut::<BasePart>(e) {
                bp.anchored = b;
            }
            // The body type follows: an anchored part stops simulating (a
            // dying zombie a script tweens), an unanchored one starts (on a
            // replica it stays kinematic: the host simulates).
            if world.get::<RigidBody>(e).is_some() {
                let mut ec = world.entity_mut(e);
                ec.insert(body_for(b, role));
                if !b && role == PlayRole::Authority {
                    ec.insert(eustress_common::play_session::PlayModePhysicsActivated);
                }
            }
            if b {
                if let Some(mut lv) = world.get_mut::<LinearVelocity>(e) {
                    lv.0 = Vec3::ZERO;
                }
                if let Some(mut av) = world.get_mut::<AngularVelocity>(e) {
                    av.0 = Vec3::ZERO;
                }
            }
            true
        }
        "CanCollide" => {
            let Some(b) = v.as_bool() else { return true };
            if let Some(mut bp) = world.get_mut::<BasePart>(e) {
                bp.can_collide = b;
            }
            let mut ec = world.entity_mut(e);
            if b {
                ec.remove::<Sensor>();
            } else {
                ec.insert(Sensor);
            }
            true
        }
        "CanTouch" => {
            if let (Some(b), Some(mut bp)) = (v.as_bool(), world.get_mut::<BasePart>(e)) {
                bp.can_touch = b;
            }
            true
        }
        "CastShadow" => {
            if let (Some(b), Some(mut bp)) = (v.as_bool(), world.get_mut::<BasePart>(e)) {
                bp.cast_shadow = b;
            }
            true
        }
        "AssemblyLinearVelocity" => {
            if let (Some(vel), Some(mut lv)) = (v.as_vector3(), world.get_mut::<LinearVelocity>(e)) {
                lv.0 = vel.to_vec3();
            }
            true
        }
        "AssemblyAngularVelocity" => {
            if let (Some(vel), Some(mut av)) = (v.as_vector3(), world.get_mut::<AngularVelocity>(e)) {
                av.0 = vel.to_vec3();
            }
            true
        }
        "Name" => {
            if let Some(s) = v.as_str() {
                if let Some(mut i) = world.get_mut::<Instance>(e) {
                    i.name = s.to_string();
                }
                if let Some(mut n) = world.get_mut::<Name>(e) {
                    n.set(s.to_string());
                }
            }
            true
        }
        "MeshId" => {
            let Some(id) = v.as_str().map(str::to_string) else { return true };
            if id.is_empty() {
                return true;
            }
            let url = mesh_url(&id, &play_asset_root(world));
            let handle: Handle<Mesh> = world.resource::<AssetServer>().load(url);
            let mut ec = world.entity_mut(e);
            ec.insert((Mesh3d(handle), eustress_common::part_draw::MeshSource::new(id)));
            true
        }
        "Shape" => {
            let Some(s) = v.as_enum_name().and_then(PartType::from_str) else { return true };
            let glb = eustress_common::part_draw::part_type_to_glb_path(&s);
            let handle: Handle<Mesh> = world.resource::<AssetServer>().load(format!("{}#Mesh0/Primitive0", glb));
            let collider = match s {
                PartType::Ball => Collider::sphere(0.5),
                PartType::Cylinder | PartType::Cone => Collider::cylinder(0.5, 1.0),
                _ => Collider::cuboid(1.0, 1.0, 1.0),
            };
            if let Some(mut p) = world.get_mut::<Part>(e) {
                p.shape = s;
            }
            world.entity_mut(e).insert((Mesh3d(handle), eustress_common::part_draw::MeshSource::new(glb), collider));
            true
        }
        // Tree-only or derived.
        "Position" | "Orientation" | "Rotation" | "Mass" | "Massless" | "Locked" | "CanQuery" | "CollisionGroup" => true,
        _ => false,
    }
}

/// An empty display component for a GUI class.
fn blank_display(class: &str) -> GuiElementDisplay {
    GuiElementDisplay {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
        position_udim2: [0.0, 0.0, 0.0, 0.0],
        size_udim2: [0.0, 100.0, 0.0, 100.0],
        anchor_point: [0.0, 0.0],
        z_order: 1,
        visible: true,
        clip_children: class == "ScrollingFrame",
        scroll_x: 0.0,
        scroll_y: 0.0,
        bg_color: [1.0, 1.0, 1.0, 1.0],
        border_size: 0.0,
        border_color: [0.1, 0.1, 0.1, 1.0],
        corner_radius: 0.0,
        text: String::new(),
        text_color: [0.0, 0.0, 0.0, 1.0],
        font: String::new(),
        font_size: 14.0,
        font_weight: 400,
        text_align: "Center".into(),
        text_y_align: "Center".into(),
        text_stroke_color: [0.0, 0.0, 0.0, 0.0],
        text_scaled: false,
        image_path: String::new(),
        class_type: class.to_string(),
        mouse_filter: "ignore".into(),
    }
}

/// One BillboardGui property onto its class component. The billboard systems
/// derive the marker, the transform and the raster from it on
/// `Changed<BillboardGui>`. Roblox's `StudsOffset` names and the Eustress
/// `UnitsOffset` ones are both accepted; `Size` is a UDim2 whose Scale is
/// metres and whose Offset is pixels (50 per metre).
fn apply_billboard_prop(bb: &mut BillboardGui, name: &str, v: &DmValue) -> bool {
    let v3 = |v: &DmValue| v.as_vector3().map(|p| [p.x as f32, p.y as f32, p.z as f32]);
    // Roblox's MaxDistance defaults to infinity; 0 is "no limit" here.
    let distance = |v: &DmValue| num(v).map(|d| if d.is_finite() { d.max(0.0) } else { 0.0 });
    match name {
        "Size" => {
            if let Some(u) = v.as_udim2() {
                bb.size = eustress_common::ui_types::UDim2::new(
                    u.x.scale as f32,
                    u.x.offset as f32,
                    u.y.scale as f32,
                    u.y.offset as f32,
                );
            }
        }
        "SizeOffset" => {
            if let DmValue::Vector2(s) = v {
                bb.size_offset = [s.x as f32, s.y as f32];
            }
        }
        "StudsOffset" | "UnitsOffset" => {
            if let Some(p) = v3(v) {
                bb.units_offset = p;
            }
        }
        "StudsOffsetWorldSpace" | "UnitsOffsetWorldSpace" => {
            if let Some(p) = v3(v) {
                bb.units_offset_world_space = p;
            }
        }
        "ExtentsOffset" => {
            if let Some(p) = v3(v) {
                bb.extents_offset = p;
            }
        }
        "ExtentsOffsetWorldSpace" => {
            if let Some(p) = v3(v) {
                bb.extents_offset_world_space = p;
            }
        }
        "AlwaysOnTop" => bb.always_on_top = v.as_bool().unwrap_or(false),
        "Enabled" => bb.enabled = v.as_bool().unwrap_or(true),
        "Active" => bb.active = v.as_bool().unwrap_or(true),
        "ClipsDescendants" => bb.clips_descendants = v.as_bool().unwrap_or(false),
        "ResetOnSpawn" => bb.reset_on_spawn = v.as_bool().unwrap_or(true),
        "MaxDistance" => {
            if let Some(d) = distance(v) {
                bb.max_distance = d;
            }
        }
        "DistanceUpperLimit" => {
            if let Some(d) = distance(v) {
                bb.distance_upper_limit = d;
            }
        }
        "DistanceLowerLimit" => {
            if let Some(d) = num(v) {
                bb.distance_lower_limit = d.max(0.0);
            }
        }
        "DistanceStep" => {
            if let Some(d) = num(v) {
                bb.distance_step = d.max(0.0);
            }
        }
        "Brightness" => {
            if let Some(b) = num(v) {
                bb.brightness = b.max(0.0);
            }
        }
        "LightInfluence" => {
            if let Some(l) = num(v) {
                bb.light_influence = l.clamp(0.0, 1.0);
            }
        }
        // `Adornee` re-parents (see `apply_props`); the rest have no effect here.
        "Adornee" | "ZIndexBehavior" | "Name" => {}
        _ => return false,
    }
    true
}

/// GUI properties onto the overlay's display component.
fn apply_gui_prop(d: &mut GuiElementDisplay, class: &str, name: &str, v: &DmValue) -> bool {
    // Scripts can write any number: keep what the overlay lays out finite and
    // bounded, or Slint's software renderer overflows its pixel maths (a panic).
    let sane = |x: f32, limit: f32| if x.is_finite() { x.clamp(-limit, limit) } else { 0.0 };
    let u = |v: &DmValue| {
        v.as_udim2().map(|u| {
            [
                sane(u.x.scale as f32, 100.0),
                sane(u.x.offset as f32, 100_000.0),
                sane(u.y.scale as f32, 100.0),
                sane(u.y.offset as f32, 100_000.0),
            ]
        })
    };
    let rgb = |v: &DmValue| v.as_color3().map(|c| [c.r as f32, c.g as f32, c.b as f32]);
    match name {
        "Position" => {
            if let Some(p) = u(v) {
                d.position_udim2 = p;
                // The legacy pixel rect: offset part only, like the loader.
                d.x = p[1];
                d.y = p[3];
            }
        }
        "Size" => {
            if let Some(s) = u(v) {
                d.size_udim2 = s;
                d.width = s[1].max(1.0);
                d.height = s[3].max(1.0);
            }
        }
        "AnchorPoint" => {
            if let DmValue::Vector2(a) = v {
                d.anchor_point = [sane(a.x as f32, 10.0), sane(a.y as f32, 10.0)];
            }
        }
        "Visible" => d.visible = v.as_bool().unwrap_or(true),
        "Enabled" if class == "ScreenGui" => d.visible = v.as_bool().unwrap_or(true),
        "ZIndex" | "DisplayOrder" => d.z_order = sane(num(v).unwrap_or(1.0), 100_000.0) as i32,
        "BackgroundColor3" => {
            if let Some(c) = rgb(v) {
                d.bg_color = [c[0], c[1], c[2], d.bg_color[3]];
            }
        }
        "BackgroundTransparency" => d.bg_color[3] = 1.0 - num(v).unwrap_or(0.0).clamp(0.0, 1.0),
        "BorderSizePixel" => d.border_size = sane(num(v).unwrap_or(0.0), 256.0).max(0.0),
        "BorderColor3" => {
            if let Some(c) = rgb(v) {
                d.border_color = [c[0], c[1], c[2], d.border_color[3].max(1.0)];
            }
        }
        "Text" => d.text = v.as_str().unwrap_or("").to_string(),
        "TextColor3" => {
            if let Some(c) = rgb(v) {
                d.text_color = [c[0], c[1], c[2], d.text_color[3]];
            }
        }
        "TextTransparency" => d.text_color[3] = 1.0 - num(v).unwrap_or(0.0).clamp(0.0, 1.0),
        "TextStrokeColor3" => {
            if let Some(c) = rgb(v) {
                d.text_stroke_color = [c[0], c[1], c[2], d.text_stroke_color[3]];
            }
        }
        "TextStrokeTransparency" => d.text_stroke_color[3] = 1.0 - num(v).unwrap_or(1.0).clamp(0.0, 1.0),
        "TextSize" => d.font_size = sane(num(v).unwrap_or(14.0), 512.0).max(1.0),
        "TextScaled" => d.text_scaled = v.as_bool().unwrap_or(false),
        "Font" => {
            if let Some(f) = v.as_enum_name() {
                d.font = f.to_string();
                d.font_weight = if f.contains("Bold") || f.contains("Black") || f.contains("Heavy") { 700 } else { 400 };
            }
        }
        "TextXAlignment" => d.text_align = v.as_enum_name().unwrap_or("Center").to_string(),
        "TextYAlignment" => d.text_y_align = v.as_enum_name().unwrap_or("Center").to_string(),
        "Image" => d.image_path = v.as_str().unwrap_or("").to_string(),
        "Active" => {
            if v.as_bool().unwrap_or(false) {
                d.mouse_filter = "stop".into();
            } else if !matches!(class, "TextButton" | "ImageButton") {
                d.mouse_filter = "ignore".into();
            }
        }
        "ClipsDescendants" => d.clip_children = v.as_bool().unwrap_or(false),
        "Name" => {}
        _ => return false,
    }
    true
}

/// A script value as the Properties-panel value type.
fn dm_to_property(v: &DmValue) -> Option<PropertyValue> {
    Some(match v {
        DmValue::Bool(b) => PropertyValue::Bool(*b),
        DmValue::Number(n) => PropertyValue::Float(*n as f32),
        DmValue::String(s) => PropertyValue::String(s.clone()),
        DmValue::Vector2(v) => PropertyValue::Vector2([v.x as f32, v.y as f32]),
        DmValue::Vector3(v) => PropertyValue::Vector3(v.to_vec3()),
        DmValue::CFrame(cf) => PropertyValue::Transform(cf.to_transform()),
        DmValue::Color3(c) => PropertyValue::Color(Color::srgb(c.r as f32, c.g as f32, c.b as f32)),
        DmValue::UDim2(u) => PropertyValue::UDim2(eustress_common::ui_types::UDim2 {
            x: eustress_common::ui_types::UDim { scale: u.x.scale as f32, offset: u.x.offset as f32 },
            y: eustress_common::ui_types::UDim { scale: u.y.scale as f32, offset: u.y.offset as f32 },
        }),
        DmValue::Enum(e) => PropertyValue::Enum(e.name.clone()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::datamodel::DataModel;
    use eustress_common::luau::play::terrain::ensure_terrain_instance;

    #[test]
    fn the_terrain_never_spawns_a_part() {
        assert!(!spawns_entity("Terrain"));
        assert!(spawns_entity("Part"));
        let mut dm = DataModel::new();
        let workspace = dm.get_service("Workspace").expect("Workspace");
        let terrain = ensure_terrain_instance(&mut dm, workspace);
        // A script cannot move it out and back in to get it spawned.
        assert!(dm.set_parent(terrain, None).is_err());
        assert!(dm.take_spawns().is_empty());
    }

    #[test]
    fn a_replica_never_simulates_a_part() {
        assert_eq!(body_for(true, PlayRole::Authority), RigidBody::Static);
        assert_eq!(body_for(true, PlayRole::Replica), RigidBody::Static);
        assert_eq!(body_for(false, PlayRole::Authority), RigidBody::Dynamic);
        assert_eq!(body_for(false, PlayRole::Replica), RigidBody::Kinematic);
    }

    /// A part that neither collides, touches nor answers queries, so the
    /// test needs no physics world.
    fn loose_part(dm: &mut DataModel, parent: InstanceId, at: [f64; 3]) -> InstanceId {
        let id = dm.create("Part");
        for flag in ["CanCollide", "CanTouch", "CanQuery"] {
            dm.set_prop(id, flag, DmValue::Bool(false)).unwrap();
        }
        dm.set_prop(id, "CFrame", DmValue::CFrame(CFrame::new(at[0], at[1], at[2]))).unwrap();
        dm.set_parent(id, Some(parent)).unwrap();
        id
    }

    fn model(dm: &mut DataModel, parent: InstanceId) -> InstanceId {
        let id = dm.create("Model");
        dm.set_parent(id, Some(parent)).unwrap();
        id
    }

    fn entity(dm: &eustress_common::datamodel::SharedDataModel, id: InstanceId) -> Option<Entity> {
        dm.lock().entity_of(id).map(Entity::from_bits)
    }

    /// A replica draws flat parts marked `FromTree`, leaves characters to the
    /// avatar runtime (and hands one over when a `Character` write names a
    /// drawn Model), and gives back only what it drew.
    #[test]
    fn a_replica_draws_what_the_host_owns() {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let players = dm.get_service("Players").unwrap();
        let crate_model = model(&mut dm, ws);
        let lid = loose_part(&mut dm, crate_model, [1.0, 2.0, 3.0]);
        let other = dm.create_virtual("Player", "Other", Some(players));
        let body = model(&mut dm, ws);
        let arm = loose_part(&mut dm, body, [0.0; 3]);
        dm.set_prop(other, "Character", DmValue::Instance(body)).unwrap();
        let late = model(&mut dm, ws);
        let late_part = loose_part(&mut dm, late, [0.0; 3]);

        let shared: eustress_common::datamodel::SharedDataModel = std::sync::Arc::new(parking_lot::Mutex::new(dm));
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<Mesh>().init_asset::<StandardMaterial>();
        app.insert_resource(PlayDataModel { dm: shared.clone() }).insert_resource(PlayRole::Replica);
        app.add_systems(Update, apply_frame);
        app.update();

        let lid_e = entity(&shared, lid).expect("the crate's lid is drawn");
        assert!(entity(&shared, arm).is_none(), "a character's part is the avatar runtime's");
        let w = app.world();
        assert!(w.get::<FromTree>(lid_e).is_some());
        assert!(w.get::<ChildOf>(lid_e).is_none(), "a replica's part is flat");
        assert_eq!(w.get::<Transform>(lid_e).unwrap().translation, Vec3::new(1.0, 2.0, 3.0), "at its world pose");
        let crate_e = entity(&shared, crate_model).expect("the Model is drawn");
        assert!(w.get::<FromTree>(crate_e).is_some());

        // A Character write hands a drawn Model to the avatar runtime.
        let late_e = entity(&shared, late_part).expect("drawn before it is a character");
        shared.lock().set_prop(other, "Character", DmValue::Instance(late)).unwrap();
        app.update();
        assert!(entity(&shared, late_part).is_none(), "unbound");
        assert!(app.world().get_entity(late_e).is_err(), "and despawned");

        // An entity bound from elsewhere stays when its instance goes; one
        // the replica drew goes with its instance.
        let foreign = app.world_mut().spawn_empty().id();
        shared.lock().bind_entity(arm, foreign.to_bits());
        {
            let mut g = shared.lock();
            g.destroy(arm);
            g.destroy(lid);
        }
        app.update();
        assert!(app.world().get_entity(foreign).is_ok(), "an avatar's entity is never despawned here");
        assert!(app.world().get_entity(lid_e).is_err());
    }

    /// A replica's wheel wears its SpecialMesh, follows writes to it, and
    /// draws its own shape again once the SpecialMesh is destroyed.
    #[test]
    fn a_replica_part_wears_its_data_mesh_until_the_mesh_goes() {
        use eustress_common::classes::MeshType;
        use eustress_common::data_mesh::{DataMeshKind, DataMeshLook};
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let wheel = loose_part(&mut dm, ws, [0.0; 3]);
        let mesh = dm.create("SpecialMesh");
        dm.set_prop(mesh, "MeshType", DmValue::Enum(eustress_common::datamodel::EnumItem::new("MeshType", "Cylinder"))).unwrap();
        dm.set_parent(mesh, Some(wheel)).unwrap();

        let shared: eustress_common::datamodel::SharedDataModel = std::sync::Arc::new(parking_lot::Mutex::new(dm));
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()));
        app.init_asset::<Mesh>().init_asset::<StandardMaterial>();
        app.insert_resource(PlayDataModel { dm: shared.clone() }).insert_resource(PlayRole::Replica);
        app.add_systems(Update, apply_frame);
        app.update();

        let e = entity(&shared, wheel).expect("the wheel is drawn");
        assert!(entity(&shared, mesh).is_none(), "a SpecialMesh draws nothing of its own");
        let look = app.world().get::<DataMeshLook>(e).cloned().expect("the wheel wears its SpecialMesh");
        assert_eq!(look.kind, DataMeshKind::Special(MeshType::Cylinder));

        shared.lock().set_prop(mesh, "Scale", DmValue::Vector3(eustress_common::scripting::Vector3::new(0.5, 1.0, 1.0))).unwrap();
        app.update();
        let scale = app.world().get::<DataMeshLook>(e).map(|l| l.scale.x);
        assert!(scale.is_some_and(|x| (x - 0.5).abs() < 1e-6), "a write to the SpecialMesh reaches the look: {scale:?}");

        shared.lock().destroy(mesh);
        app.update();
        assert!(app.world().get::<DataMeshLook>(e).is_none(), "without its SpecialMesh the wheel draws its own shape");
    }
}

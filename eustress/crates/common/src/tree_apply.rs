//! # Drawing the tree: the apply step of a shell without scripts
//!
//! The Player keeps a world it joined as a DataModel tree, because the tree is
//! where replication writes: every change the host makes reaches it through
//! the tree's own writes (`eustress-networking`'s `Replica`). This step draws
//! the tree each frame from its four drains, as Studio's
//! `play_datamodel::apply` does for a Play session:
//!
//! * **Spawns**: a BasePart that enters Workspace gets an entity with
//!   [`crate::space_read`]'s mesh, material and collider for its shape, and
//!   is bound to it (`DataModel::bind_entity`). A destroy despawns an entity
//!   through that binding, and the motion lane finds it the same way.
//! * **Writes**: `CFrame`, `Size`, `Shape`, `MeshId`, `Color`,
//!   `Transparency`, `Material`, `Reflectance`, `Anchored` and `CanCollide`
//!   reach the entity in the frame they land.
//! * **Reparents**: a part that leaves Workspace despawns, and spawns again
//!   if it comes back.
//! * **Destroys** despawn.
//!
//! The step despawns and unbinds only what it drew itself (`FromTree`). An
//! entity something else bound to an instance, such as the avatar standing
//! for a character's `HumanoidRootPart`, belongs to whatever bound it: a
//! destroy, a reparent or a `Character` write leaves it alive and bound.
//!
//! Only Workspace draws: a part in ReplicatedStorage, or detached, lives in
//! the tree alone. So do the parts of a Model some Player's `Character` points
//! at, since the avatar runtime draws and moves characters. A part's `CFrame`
//! is its world pose whatever its parent, so entities have no ECS parent. The
//! host simulates every body, so a part is `Static` when anchored and
//! `Kinematic` otherwise, never `Dynamic`: the local avatar collides with it,
//! and only the host's writes and the motion lane move it.
//!
//! With `physics` on, each drawn part also carries the climb rules' mark for
//! what the Player's entities cannot show: the part's `Climbable` attribute,
//! whether it belongs to an NPC (a Model holding a `Humanoid` or an
//! `AnimationController`), and whether it is invisible. See
//! `avatar::climbable`.
//!
//! With `physics` on, the step also hands the view to the Space's camera while
//! its `CameraType` is `Scriptable`, as Studio's `play_datamodel::camera`
//! does. A Space declares a fixed view by saving a `Camera` in its Workspace.
//! [`PlayerTree`] keeps the camera the world opened with, so a
//! `Workspace.CurrentCamera` written over the network never retargets it.
//! While no avatar camera exists (a Space with `Players.CharacterAutoLoads`
//! off, or before the avatar arrives), the step's own [`TreeViewCamera`]
//! shows the Space's camera, as Studio's `ScriptedPlayCamera` does.
//!
//! The step trims the tree's events and ends the tree's frame. Script output
//! goes to the log. The queues this shell has no consumer for (sounds,
//! particle emits, NPC moves, impulses, terrain edits, touch watches) are
//! emptied every frame, so they never grow.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;

use crate::classes::PartType;
use crate::datamodel::{DataModel, InstanceId, SharedDataModel};
use crate::space_read::{look_key, part_material, primitive_mesh, LookKey, SpacePart};

/// The world's tree on a shell that runs no scripts of its own. The world's
/// reader creates it; replication writes into it; [`TreeApply`] draws it.
#[derive(Resource, Clone)]
pub struct PlayerTree {
    pub dm: SharedDataModel,
    /// The Space's own camera, as the tree had it when the world opened.
    pub camera: Option<InstanceId>,
}

impl PlayerTree {
    /// Wrap a tree built from a world. The tree gets the `Players.LocalPlayer`
    /// replication needs, so the host's `Player` for this peer lands on it
    /// instead of making a second one, and a `Workspace.CurrentCamera` (the
    /// Space's camera, or a local one), which is kept as the Space's.
    pub fn new(mut dm: DataModel, player_name: &str) -> Self {
        if dm.local_player.filter(|p| dm.exists(*p)).is_none() {
            let players = dm.get_service("Players");
            let me = dm.create_virtual("Player", player_name, players);
            dm.local_player = Some(me);
        }
        // Scripts read `Players.LocalPlayer` as the service's property, as
        // Studio's Play sets it; the tree's own field alone left it nil.
        if let (Some(players), Some(me)) = (dm.get_service("Players"), dm.local_player) {
            let _ = dm.set_prop(players, "LocalPlayer", crate::datamodel::DmValue::Instance(me));
        }
        let camera = ensure_current_camera(&mut dm);
        session_defaults(&mut dm);
        Self { dm: Arc::new(parking_lot::Mutex::new(dm)), camera }
    }

    /// The Space's own `Players.CharacterAutoLoads`, read from its folder.
    /// A host's value, when it sends one, replaces it.
    pub fn with_space(self, space_root: &std::path::Path) -> Self {
        let on = crate::play_session::space_character_auto_loads(space_root);
        {
            let mut g = self.dm.lock();
            if let Some(players) = g.get_service("Players") {
                let _ = g.set_prop(players, "CharacterAutoLoads", crate::datamodel::DmValue::Bool(on));
            }
        }
        self
    }
}

/// The service properties Studio's Play sets as it starts, at the values it
/// sets them to, where the tree does not have them yet: a Space never saves
/// its gravity, so it opens at standard gravity; characters respawn after
/// [`RESPAWN_TIME_SECS`](crate::play_session::RESPAWN_TIME_SECS); and
/// characters load unless [`PlayerTree::with_space`] reads otherwise.
fn session_defaults(dm: &mut DataModel) {
    use crate::datamodel::DmValue;
    let unset = |dm: &DataModel, id: InstanceId, name: &str| dm.get_prop(id, name).is_none();
    if let Some(ws) = dm.get_service("Workspace") {
        if unset(dm, ws, "Gravity") {
            let g = crate::play_session::workspace_gravity(crate::services::workspace::DEFAULT_GRAVITY);
            let _ = dm.set_prop(ws, "Gravity", DmValue::Number(g));
        }
    }
    if let Some(players) = dm.get_service("Players") {
        if unset(dm, players, "RespawnTime") {
            let _ = dm.set_prop(players, "RespawnTime", DmValue::Number(crate::play_session::RESPAWN_TIME_SECS));
        }
        if unset(dm, players, "CharacterAutoLoads") {
            let _ = dm.set_prop(players, "CharacterAutoLoads", DmValue::Bool(true));
        }
    }
}

/// Every client has a `Workspace.CurrentCamera`, as in Roblox: the Space's
/// own camera when it saved one, else a local one made here. It never
/// leaves this machine; LocalScripts set its `CameraType`, `Projection` and
/// `CFrame`, and the camera step shows it.
fn ensure_current_camera(dm: &mut DataModel) -> Option<InstanceId> {
    let workspace = dm.get_service("Workspace")?;
    let camera = match dm.current_camera() {
        Some(camera) => camera,
        None => dm.create_virtual("Camera", "Camera", Some(workspace)),
    };
    let _ = dm.set_prop(workspace, "CurrentCamera", crate::datamodel::DmValue::Instance(camera));
    Some(camera)
}

/// The apply step's place in `Update`. Whatever writes the tree each frame
/// (replication) runs before it, so a write draws in the frame it lands.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TreeApply;

/// Draws [`PlayerTree`] whenever one exists.
pub struct TreeApplyPlugin;

impl Plugin for TreeApplyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TreeLooks>().add_systems(Update, apply_tree.in_set(TreeApply));
        #[cfg(feature = "physics")]
        app.add_systems(
            Update,
            (show_tree_view_camera, apply_tree_camera).chain().after(apply_tree).in_set(TreeApply),
        );
    }
}

/// An entity drawn from the tree, and the instance it draws.
#[derive(Component, Debug, Clone, Copy)]
pub struct FromTree(pub InstanceId);

/// A drawn part whose look changed after it spawned. It owns its material,
/// and later changes rewrite that in place, while parts whose look never
/// changes go on sharing one material per look.
#[derive(Component, Debug, Clone, Copy)]
struct OwnMaterial;

/// The meshes and materials drawn parts share.
#[derive(Resource, Default)]
struct TreeLooks {
    /// One per shape, by `PartType` discriminant.
    meshes: [Option<Handle<Mesh>>; 6],
    shared: HashMap<LookKey, Handle<StandardMaterial>>,
}

impl TreeLooks {
    fn mesh(&mut self, shape: PartType, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.meshes[shape as usize]
            .get_or_insert_with(|| meshes.add(primitive_mesh(shape)))
            .clone()
    }

    fn material(&mut self, part: &SpacePart, materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
        self.shared
            .entry(look_key(part))
            .or_insert_with(|| materials.add(part_material(part)))
            .clone()
    }
}

/// Writes that move or size a part.
const POSE: &[&str] = &["CFrame", "Size"];
/// Writes that change its mesh (and so its collider).
const SHAPE: &[&str] = &["Shape", "MeshId"];
/// Writes that change its material.
const LOOK: &[&str] = &["Color", "Transparency", "Material", "Reflectance"];
/// Writes that change how it collides.
#[cfg(feature = "physics")]
const BODY: &[&str] = &["Anchored", "CanCollide"];

#[allow(clippy::type_complexity)]
fn apply_tree(
    mut commands: Commands,
    tree: Option<Res<PlayerTree>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut looks: ResMut<TreeLooks>,
    mut drawn: Query<
        (&mut Transform, &mut Mesh3d, &mut MeshMaterial3d<StandardMaterial>, Has<OwnMaterial>),
        With<FromTree>,
    >,
    marked: Query<(), With<FromTree>>,
) {
    let Some(tree) = tree else { return };
    let mut g = tree.dm.lock();
    let characters = characters(&g);
    // The rigged Models, found once in a frame that needs them.
    #[cfg(feature = "physics")]
    let mut rigs: Option<HashSet<InstanceId>> = None;

    let mut spawned_now = HashSet::new();
    // This frame's parts, whose `FromTree` has not reached the world yet.
    let mut spawned_entities: HashSet<Entity> = HashSet::new();
    for id in g.take_spawns() {
        if !drawn_here(&g, id, &characters) {
            continue;
        }
        #[allow(unused_mut)]
        let Some(mut part) = SpacePart::from_tree(&g, id) else { continue };
        #[cfg(feature = "physics")]
        {
            part.in_character = in_rig(&g, id, rigs.get_or_insert_with(|| rigged_models(&g)));
        }
        let entity = spawn_part(&mut commands, &mut looks, &mut meshes, &mut materials, id, &part);
        g.bind_entity(id, entity.to_bits());
        spawned_now.insert(id);
        spawned_entities.insert(entity);
    }

    // What this step drew, and so may despawn.
    let ours = |e: Entity| spawned_entities.contains(&e) || marked.contains(e);

    for id in g.take_reparents() {
        if g.entity_of(id).is_some() && !drawn_here(&g, id, &characters) {
            undraw(&mut commands, &mut g, id, &ours);
        }
    }

    for (id, props) in g.take_dirty() {
        // A Player's `Character` now points at a Model whose parts may be
        // drawn already: the avatar runtime draws them from now on.
        if props.iter().any(|p| p == "Character") {
            if let Some(model) = g.get_prop(id, "Character").and_then(|v| v.as_instance()) {
                for d in g.descendants(model) {
                    if g.entity_of(d).is_some() {
                        undraw(&mut commands, &mut g, d, &ours);
                    }
                }
            }
            continue;
        }
        if spawned_now.contains(&id) {
            continue; // spawned from its current properties already
        }
        let Some(entity) = g.entity_of(id).map(Entity::from_bits) else { continue };
        #[allow(unused_mut)]
        let Some(mut part) = SpacePart::from_tree(&g, id) else { continue };
        let Ok((mut transform, mut mesh, mut material, own)) = drawn.get_mut(entity) else { continue };
        let touches = |names: &[&str]| props.iter().any(|p| names.contains(&p.as_str()));

        if touches(POSE) {
            *transform = part.transform;
        }
        if touches(SHAPE) {
            mesh.0 = looks.mesh(part.shape, &mut meshes);
        }
        if touches(LOOK) {
            if own {
                if let Some(mut m) = materials.get_mut(&material.0) {
                    *m = part_material(&part);
                }
            } else {
                material.0 = materials.add(part_material(&part));
                commands.entity(entity).insert(OwnMaterial);
            }
        }
        #[cfg(feature = "physics")]
        if touches(SHAPE) || touches(BODY) {
            use avian3d::prelude::{Collider, RigidBody};
            match body_of(&part) {
                Some(body) => {
                    commands.entity(entity).insert(body);
                }
                None => {
                    commands.entity(entity).remove::<(Collider, RigidBody)>();
                }
            }
        }
        #[cfg(feature = "physics")]
        if touches(BODY) || touches(&["Transparency"]) {
            use crate::avatar::climbable::Climbable;
            part.in_character = in_rig(&g, id, rigs.get_or_insert_with(|| rigged_models(&g)));
            match crate::space_read::climb_mark(&part).filter(|_| part.can_collide) {
                Some(mark) => {
                    commands.entity(entity).insert(mark);
                }
                None => {
                    commands.entity(entity).remove::<Climbable>();
                }
            }
        }
    }

    for bits in g.take_despawns() {
        let entity = Entity::from_bits(bits);
        if !ours(entity) {
            continue; // bound from elsewhere: its binder despawns it
        }
        if let Ok(mut ec) = commands.get_entity(entity) {
            ec.try_despawn();
        }
    }

    // Script output goes to the log; the queues nothing here consumes are
    // emptied, or they would grow for as long as the world is open.
    for line in std::mem::take(&mut g.output) {
        match line.level {
            crate::datamodel::OutputLevel::Error => tracing::warn!("[{}] {}", line.source, line.text),
            _ => tracing::info!("[{}] {}", line.source, line.text),
        }
    }
    g.sound_commands.clear();
    g.particle_emits.clear();
    g.humanoid_commands.clear();
    g.physics_commands.clear();
    g.terrain_commands.clear();
    g.touch_watch.clear();

    let cursor = g.event_cursor();
    g.trim_events(cursor);
    g.end_frame();
}

/// The Models some Player's `Character` points at.
fn characters(g: &DataModel) -> HashSet<InstanceId> {
    let Some(players) = g.find_service("Players") else { return HashSet::new() };
    g.children(players)
        .iter()
        .filter_map(|&p| g.get_prop(p, "Character").and_then(|v| v.as_instance()))
        .collect()
}

/// The Models holding a `Humanoid` or an `AnimationController`: NPCs and other
/// characters, whose parts characters do not climb by default. Only a Model is
/// a character, so a rig left loose in Workspace or a Folder marks nothing.
#[cfg(feature = "physics")]
fn rigged_models(g: &DataModel) -> HashSet<InstanceId> {
    let Some(workspace) = g.find_service("Workspace") else { return HashSet::new() };
    g.descendants(workspace)
        .into_iter()
        .filter(|&d| matches!(g.class_of(d), Some("Humanoid" | "AnimationController")))
        .filter_map(|d| g.parent(d))
        .filter(|&m| g.class_of(m) == Some("Model"))
        .collect()
}

/// Whether an instance is inside one of `rigs`.
#[cfg(feature = "physics")]
fn in_rig(g: &DataModel, id: InstanceId, rigs: &HashSet<InstanceId>) -> bool {
    let mut at = g.parent(id);
    // Bounded, as the tree's own ancestry walks are.
    for _ in 0..4096 {
        let Some(here) = at else { return false };
        if rigs.contains(&here) {
            return true;
        }
        at = g.parent(here);
    }
    false
}

/// Whether an instance draws here: somewhere under Workspace, and not inside
/// a character.
fn drawn_here(g: &DataModel, id: InstanceId, characters: &HashSet<InstanceId>) -> bool {
    let root = g.root();
    let mut cur = id;
    // Bounded, as the tree's own ancestry walks are.
    for _ in 0..4096 {
        let Some(parent) = g.parent(cur) else { return false };
        if parent == root {
            return g.class_of(cur) == Some("Workspace");
        }
        if characters.contains(&parent) {
            return false;
        }
        cur = parent;
    }
    false
}

fn spawn_part(
    commands: &mut Commands,
    looks: &mut TreeLooks,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    id: InstanceId,
    part: &SpacePart,
) -> Entity {
    let entity = commands
        .spawn((
            Mesh3d(looks.mesh(part.shape, meshes)),
            MeshMaterial3d(looks.material(part, materials)),
            part.transform,
            Name::new(part.name.clone()),
            FromTree(id),
        ))
        .id();
    #[cfg(feature = "physics")]
    if let Some(body) = body_of(part) {
        commands.entity(entity).insert(body);
        if let Some(mark) = crate::space_read::climb_mark(part) {
            commands.entity(entity).insert(mark);
        }
    }
    entity
}

/// Despawn an instance's entity and unbind it, so a later destroy of the
/// instance queues nothing, when this step drew it. An entity bound from
/// elsewhere is left alive and bound.
fn undraw(commands: &mut Commands, g: &mut DataModel, id: InstanceId, ours: impl Fn(Entity) -> bool) {
    let Some(entity) = g.entity_of(id).map(Entity::from_bits) else { return };
    if !ours(entity) {
        return;
    }
    g.unbind_entity(id);
    if let Ok(mut ec) = commands.get_entity(entity) {
        ec.try_despawn();
    }
}

/// The collider and body a part gets: none when it does not collide.
#[cfg(feature = "physics")]
fn body_of(part: &SpacePart) -> Option<(avian3d::prelude::Collider, avian3d::prelude::RigidBody)> {
    use avian3d::prelude::RigidBody;
    part.can_collide.then(|| {
        let body = if part.anchored { RigidBody::Static } else { RigidBody::Kinematic };
        (crate::space_read::primitive_collider(part.shape), body)
    })
}

/// The Space camera's view while it is `Scriptable`: its pose, and a vertical
/// size for an orthographic projection or a field of view in degrees.
#[cfg(feature = "physics")]
struct ScriptedView {
    cframe: crate::scripting::CFrame,
    orthographic: Option<f32>,
    fov_deg: f32,
}

/// The Space camera's view: only while it is `Scriptable` when `scriptable_only`
/// (an avatar camera leads otherwise), whatever its `CameraType` when not.
#[cfg(feature = "physics")]
fn space_view(tree: &PlayerTree, scriptable_only: bool) -> Option<ScriptedView> {
    let cam = tree.camera?;
    let g = tree.dm.lock();
    let scriptable = g.get_prop(cam, "CameraType").and_then(|v| v.as_enum_name().map(|n| n == "Scriptable"));
    if scriptable_only && scriptable != Some(true) {
        return None;
    }
    let number = |k: &str| g.get_prop(cam, k).and_then(|v| v.as_number()).map(|n| n as f32);
    let ortho = g.get_prop(cam, "Projection").and_then(|v| v.as_enum_name().map(|n| n == "Orthographic"));
    Some(ScriptedView {
        cframe: g.get(cam)?.cframe()?,
        orthographic: (ortho == Some(true)).then(|| number("OrthographicSize").unwrap_or(40.0).max(0.1)),
        fov_deg: number("FieldOfView").unwrap_or(70.0),
    })
}

/// The camera a tree's world shows through while no avatar camera exists.
/// [`show_tree_view_camera`] keeps it active exactly then, and
/// [`apply_tree_camera`] gives it the Space camera's view.
#[cfg(feature = "physics")]
#[derive(Component, Debug, Clone, Copy)]
pub struct TreeViewCamera;

/// Keep one [`TreeViewCamera`] while a tree is open, active exactly while no
/// avatar camera exists, so the world always renders through one camera: the
/// avatar's when it has one.
#[cfg(feature = "physics")]
fn show_tree_view_camera(
    mut commands: Commands,
    tree: Option<Res<PlayerTree>>,
    host: Option<Res<crate::avatar::AvatarHostConfig>>,
    avatar_cams: Query<(), With<crate::avatar::control::AvatarCamera>>,
    mut view_cams: Query<(Entity, &mut Camera), (With<TreeViewCamera>, Without<crate::avatar::control::AvatarCamera>)>,
) {
    use crate::avatar::control::AVATAR_FOV_DEG;

    let Some(tree) = tree else {
        for (entity, _) in view_cams.iter() {
            commands.entity(entity).try_despawn();
        }
        return;
    };
    let want = avatar_cams.is_empty();
    if want && view_cams.is_empty() {
        // Opens on the Space camera's pose, or looking at the origin from
        // where Studio's play camera starts without an editor view.
        let start = space_view(&tree, false).map_or_else(
            || Transform::from_xyz(0.0, 12.0, 24.0).looking_at(Vec3::ZERO, Vec3::Y),
            |v| v.cframe.to_transform().with_scale(Vec3::ONE),
        );
        commands.spawn((
            Camera3d::default(),
            Camera { order: host.map_or(0, |h| h.seams.camera_order), ..default() },
            Projection::Perspective(PerspectiveProjection { fov: AVATAR_FOV_DEG.to_radians(), ..default() }),
            // The avatar camera's tonemapper, so both look the same.
            bevy::core_pipeline::tonemapping::Tonemapping::Reinhard,
            start,
            TreeViewCamera,
            Name::new("TreeViewCamera"),
        ));
        return;
    }
    for (_, mut camera) in view_cams.iter_mut() {
        if camera.is_active != want {
            camera.is_active = want;
        }
    }
}

/// The Space's `Scriptable` camera takes the avatar camera's view, whose
/// orbit and follow systems stand down; otherwise the avatar camera leads.
/// Without an avatar camera, the [`TreeViewCamera`] shows the Space camera
/// whatever its `CameraType`, since nothing else would move the view.
#[cfg(feature = "physics")]
#[allow(clippy::type_complexity)]
fn apply_tree_camera(
    mut commands: Commands,
    tree: Option<Res<PlayerTree>>,
    mut cams: Query<
        (
            Entity,
            &mut Transform,
            &mut Projection,
            Option<&mut crate::avatar::control::AvatarCamera>,
            Has<crate::avatar::control::AvatarCameraScripted>,
        ),
        Or<(With<crate::avatar::control::AvatarCamera>, With<TreeViewCamera>)>,
    >,
) {
    use crate::avatar::control::{AvatarCameraScripted, AVATAR_FOV_DEG};
    use bevy::camera::ScalingMode;

    // The avatar's camera when there is one, else the tree's own.
    let target = cams
        .iter()
        .find(|(_, _, _, avatar, _)| avatar.is_some())
        .or_else(|| cams.iter().next())
        .map(|(entity, ..)| entity);
    let Some(Ok((entity, mut tf, mut projection, mut avatar_cam, scripted))) = target.map(|e| cams.get_mut(e)) else {
        return;
    };
    let follows_avatar = avatar_cam.is_some();
    let Some(view) = tree.as_deref().and_then(|t| space_view(t, follows_avatar)) else {
        if scripted {
            commands.entity(entity).remove::<AvatarCameraScripted>();
            *projection = Projection::Perspective(PerspectiveProjection {
                fov: AVATAR_FOV_DEG.to_radians(),
                ..default()
            });
        }
        return;
    };
    if follows_avatar && !scripted {
        commands.entity(entity).insert(AvatarCameraScripted);
    }
    let t = view.cframe.to_transform();
    tf.translation = t.translation;
    tf.rotation = t.rotation;

    // Keep movement relative to the screen: forward is the view direction on
    // the ground, or the screen's up for a camera looking straight down.
    let look = view.cframe.look_vector();
    let up = view.cframe.up_vector();
    let (fx, fz) = if look.y.abs() < 0.98 { (look.x, look.z) } else { (up.x, up.z) };
    if fx.abs() + fz.abs() > 1e-6 {
        if let Some(avatar_cam) = avatar_cam.as_mut() {
            avatar_cam.yaw = (-(fx as f32)).atan2(-(fz as f32));
        }
    }

    match view.orthographic {
        Some(height) => {
            let current = match &*projection {
                Projection::Orthographic(o) => match o.scaling_mode {
                    ScalingMode::FixedVertical { viewport_height } => Some(viewport_height),
                    _ => None,
                },
                _ => None,
            };
            if current.map_or(true, |h| (h - height).abs() > 1e-4) {
                let mut o = OrthographicProjection::default_3d();
                o.scaling_mode = ScalingMode::FixedVertical { viewport_height: height };
                o.near = -500.0;
                o.far = 5000.0;
                *projection = Projection::Orthographic(o);
            }
        }
        None => {
            let want = view.fov_deg.clamp(1.0, 120.0).to_radians();
            let current = match &*projection {
                Projection::Perspective(p) => Some(p.fov),
                _ => None,
            };
            if current.map_or(true, |f| (f - want).abs() > 1e-4) {
                *projection = Projection::Perspective(PerspectiveProjection { fov: want, ..default() });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datamodel::DmValue;
    use crate::scripting::{CFrame, Vector3};

    fn app(tree: &PlayerTree) -> App {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_plugins(TreeApplyPlugin)
            .insert_resource(tree.clone());
        app
    }

    /// A part that does not collide, so the test needs no physics world.
    fn part(dm: &mut DataModel, parent: InstanceId, at: [f64; 3], size: [f64; 3]) -> InstanceId {
        let id = dm.create("Part");
        dm.set_prop(id, "CanCollide", DmValue::Bool(false)).unwrap();
        dm.set_prop(id, "CFrame", DmValue::CFrame(CFrame::new(at[0], at[1], at[2]))).unwrap();
        dm.set_prop(id, "Size", DmValue::Vector3(Vector3::new(size[0], size[1], size[2]))).unwrap();
        dm.set_parent(id, Some(parent)).unwrap();
        id
    }

    fn drawn(app: &mut App) -> Vec<(InstanceId, Entity, Transform)> {
        let mut q = app.world_mut().query::<(Entity, &FromTree, &Transform)>();
        q.iter(app.world()).map(|(e, f, t)| (f.0, e, *t)).collect()
    }

    #[test]
    fn workspace_parts_draw_and_follow_their_writes() {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let storage = dm.get_service("ReplicatedStorage").unwrap();
        let ball = part(&mut dm, ws, [1.0, 2.0, 3.0], [2.0, 3.0, 4.0]);
        let stored = part(&mut dm, storage, [0.0; 3], [1.0; 3]);
        let tree = PlayerTree::new(dm, "Tester");
        let mut app = app(&tree);

        app.update();
        let rows = drawn(&mut app);
        assert_eq!(rows.len(), 1, "only the Workspace part draws");
        let (id, entity, t) = rows[0];
        assert_eq!(id, ball);
        assert_eq!(t.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(t.scale, Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(tree.dm.lock().entity_of(ball), Some(entity.to_bits()), "bound for destroys and motion");
        assert_eq!(tree.dm.lock().entity_of(stored), None);

        {
            let mut g = tree.dm.lock();
            g.set_prop(ball, "CFrame", DmValue::CFrame(CFrame::new(5.0, 0.0, 0.0))).unwrap();
            g.set_prop(ball, "Transparency", DmValue::Number(0.5)).unwrap();
        }
        app.update();
        assert_eq!(drawn(&mut app)[0].2.translation, Vec3::new(5.0, 0.0, 0.0));
        let handle = app.world().get::<MeshMaterial3d<StandardMaterial>>(entity).unwrap().0.clone();
        let alpha = app.world().resource::<Assets<StandardMaterial>>().get(&handle).unwrap().base_color.alpha();
        assert!((alpha - 0.5).abs() < 1e-6, "transparency reached the material, got alpha {alpha}");

        tree.dm.lock().set_parent(ball, Some(storage)).unwrap();
        app.update();
        assert!(drawn(&mut app).is_empty(), "a part that leaves Workspace despawns");
        assert_eq!(tree.dm.lock().entity_of(ball), None);

        tree.dm.lock().set_parent(ball, Some(ws)).unwrap();
        app.update();
        assert_eq!(drawn(&mut app).len(), 1, "and draws again when it comes back");

        tree.dm.lock().destroy(ball);
        app.update();
        assert!(drawn(&mut app).is_empty(), "a destroyed part despawns");
        assert!(tree.dm.lock().events_since(0).0.is_empty(), "events are trimmed every frame");
    }

    #[test]
    fn characters_are_left_to_the_avatar_runtime() {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let tree_players = dm.get_service("Players").unwrap();
        let host = dm.create_virtual("Player", "Host", Some(tree_players));
        let model = dm.create_virtual("Model", "Host", Some(ws));
        dm.set_prop(host, "Character", DmValue::Instance(model)).unwrap();
        let root = part(&mut dm, model, [0.0, 3.0, 0.0], [0.6, 2.0, 0.4]);
        let floor = part(&mut dm, ws, [0.0, -0.5, 0.0], [20.0, 1.0, 20.0]);
        let tree = PlayerTree::new(dm, "Tester");
        let mut app = app(&tree);

        app.update();
        let ids: Vec<_> = drawn(&mut app).into_iter().map(|(id, ..)| id).collect();
        assert_eq!(ids, vec![floor], "the host's HumanoidRootPart gets no entity");
        assert_eq!(tree.dm.lock().entity_of(root), None);
        assert!(tree.dm.lock().local_player.is_some(), "the tree has its LocalPlayer");
    }

    /// An NPC's parts carry the mark that keeps characters off them, a statue
    /// of one marked `Climbable` carries the opposite, and a plain wall needs
    /// no mark at all.
    #[cfg(feature = "physics")]
    #[test]
    fn an_npc_is_not_climbed_unless_it_says_so() {
        use crate::avatar::climbable::{Climbable, CLIMBABLE_ATTRIBUTE};
        let solid = |dm: &mut DataModel, parent: InstanceId, x: f64| {
            let id = part(dm, parent, [x, 1.0, 0.0], [1.0, 2.0, 1.0]);
            dm.set_prop(id, "CanCollide", DmValue::Bool(true)).unwrap();
            dm.set_prop(id, "Anchored", DmValue::Bool(true)).unwrap();
            id
        };
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let npc = dm.create_virtual("Model", "Npc", Some(ws));
        dm.create_virtual("Humanoid", "Humanoid", Some(npc));
        let torso = solid(&mut dm, npc, 0.0);
        let statue = dm.create_virtual("Model", "Statue", Some(ws));
        dm.create_virtual("Humanoid", "Humanoid", Some(statue));
        dm.set_attribute(statue, CLIMBABLE_ATTRIBUTE, DmValue::Bool(true)).unwrap();
        let plinth = solid(&mut dm, statue, 4.0);
        let wall = solid(&mut dm, ws, 8.0);
        let tree = PlayerTree::new(dm, "Tester");
        let mut app = app(&tree);

        app.update();
        let mark = |app: &App, id: InstanceId| {
            let e = Entity::from_bits(tree.dm.lock().entity_of(id).expect("drawn"));
            app.world().get::<Climbable>(e).copied()
        };
        assert_eq!(mark(&app, torso), Some(Climbable(false)), "an NPC's torso is not for climbing");
        assert_eq!(mark(&app, plinth), Some(Climbable(true)), "the statue says it is");
        assert_eq!(mark(&app, wall), None, "a wall is climbable by its body alone");
    }

    #[cfg(feature = "physics")]
    #[test]
    fn a_scriptable_space_camera_takes_the_view() {
        use crate::avatar::control::{AvatarCamera, AvatarCameraScripted};
        use crate::datamodel::EnumItem;
        use bevy::camera::ScalingMode;

        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let cam = dm.create("Camera");
        let eye = Transform::from_xyz(0.0, 60.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z);
        dm.set_prop(cam, "CFrame", DmValue::CFrame(CFrame::from_transform(&eye))).unwrap();
        dm.set_prop(cam, "CameraType", DmValue::Enum(EnumItem::new("CameraType", "Scriptable"))).unwrap();
        dm.set_prop(cam, "Projection", DmValue::Enum(EnumItem::new("CameraProjection", "Orthographic"))).unwrap();
        dm.set_prop(cam, "OrthographicSize", DmValue::Number(30.0)).unwrap();
        dm.set_parent(cam, Some(ws)).unwrap();
        let tree = PlayerTree::new(dm, "Tester");
        assert_eq!(tree.camera, Some(cam), "the Workspace camera is the Space's");
        let mut app = app(&tree);
        let viewer = app
            .world_mut()
            .spawn((AvatarCamera::new(Entity::PLACEHOLDER), Transform::default(), Projection::default()))
            .id();

        app.update();
        assert!(app.world().get::<AvatarCameraScripted>(viewer).is_some());
        let t = *app.world().get::<Transform>(viewer).unwrap();
        assert!((t.translation - Vec3::new(0.0, 60.0, 0.0)).length() < 1e-4);
        assert!(t.forward().dot(Vec3::NEG_Y) > 0.999, "looks straight down");
        assert!(t.up().dot(Vec3::NEG_Z) > 0.999, "north at the top of the screen");
        match app.world().get::<Projection>(viewer).unwrap() {
            Projection::Orthographic(o) => assert!(matches!(
                o.scaling_mode,
                ScalingMode::FixedVertical { viewport_height } if (viewport_height - 30.0).abs() < 1e-4
            )),
            other => panic!("expected an orthographic projection, got {other:?}"),
        }

        tree.dm.lock().set_prop(cam, "CameraType", DmValue::Enum(EnumItem::new("CameraType", "Custom"))).unwrap();
        app.update();
        assert!(app.world().get::<AvatarCameraScripted>(viewer).is_none(), "the avatar camera leads again");
        assert!(matches!(app.world().get::<Projection>(viewer).unwrap(), Projection::Perspective(_)));
    }

    #[test]
    fn the_session_starts_with_studios_service_values() {
        let tree = PlayerTree::new(DataModel::new(), "Tester");
        let g = tree.dm.lock();
        let ws = g.find_service("Workspace").unwrap();
        let players = g.find_service("Players").unwrap();
        assert_eq!(g.get_prop(ws, "Gravity").and_then(|v| v.as_number()), Some(9.80665));
        assert_eq!(g.get_prop(players, "RespawnTime").and_then(|v| v.as_number()), Some(5.0));
        assert_eq!(g.get_prop(players, "CharacterAutoLoads").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn a_space_that_turns_characters_off_says_so() {
        let dir = std::env::temp_dir().join(format!("tree_apply_autoload_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Players")).unwrap();
        std::fs::write(dir.join("Players").join("_service.toml"), "[properties]\ncharacter_auto_loads = false\n").unwrap();
        let tree = PlayerTree::new(DataModel::new(), "Tester").with_space(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let g = tree.dm.lock();
        let players = g.find_service("Players").unwrap();
        assert_eq!(g.get_prop(players, "CharacterAutoLoads").and_then(|v| v.as_bool()), Some(false));
    }

    #[test]
    fn local_player_is_readable_from_players() {
        let tree = PlayerTree::new(DataModel::new(), "Tester");
        let g = tree.dm.lock();
        let players = g.find_service("Players").unwrap();
        let me = g.local_player.expect("a local player");
        assert_eq!(g.get_prop(players, "LocalPlayer").and_then(|v| v.as_instance()), Some(me));
        assert_eq!(g.name_of(me), Some("Tester"));
    }

    #[test]
    fn every_tree_has_a_current_camera() {
        // A Space that saved no camera gets a local one.
        let tree = PlayerTree::new(DataModel::new(), "Tester");
        let g = tree.dm.lock();
        let ws = g.find_service("Workspace").unwrap();
        let current = g.get_prop(ws, "CurrentCamera").and_then(|v| v.as_instance());
        assert!(current.is_some(), "workspace.CurrentCamera is never nil");
        assert_eq!(tree.camera, current, "and it is the camera the view follows");
        assert_eq!(g.class_of(current.unwrap()), Some("Camera"));
        drop(g);

        // A Space's own camera is the current one.
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let saved = dm.create("Camera");
        dm.set_parent(saved, Some(ws)).unwrap();
        let tree = PlayerTree::new(dm, "Tester");
        assert_eq!(tree.camera, Some(saved));
        assert_eq!(tree.dm.lock().get_prop(ws, "CurrentCamera").and_then(|v| v.as_instance()), Some(saved));
    }

    #[cfg(feature = "physics")]
    #[test]
    fn without_an_avatar_the_space_camera_shows_the_world() {
        use crate::avatar::control::{AvatarCamera, AvatarCameraScripted};

        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let cam = dm.create("Camera");
        let eye = Transform::from_xyz(0.0, 40.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y);
        dm.set_prop(cam, "CFrame", DmValue::CFrame(CFrame::from_transform(&eye))).unwrap();
        dm.set_parent(cam, Some(ws)).unwrap();
        let tree = PlayerTree::new(dm, "Tester");
        let mut app = app(&tree);

        app.update();
        app.update();
        let mut q = app.world_mut().query_filtered::<(Entity, &Camera, &Transform), With<TreeViewCamera>>();
        let views: Vec<(Entity, bool, Transform)> = q.iter(app.world()).map(|(e, c, t)| (e, c.is_active, *t)).collect();
        assert_eq!(views.len(), 1, "one camera while no avatar exists");
        let (view, active, t) = views[0];
        assert!(active);
        assert!((t.translation - eye.translation).length() < 1e-4, "a Custom camera still gives its pose");
        assert!(t.forward().dot(*eye.forward()) > 0.999);
        assert!(app.world().get::<AvatarCameraScripted>(view).is_none(), "no avatar camera to stand down");

        let viewer = app
            .world_mut()
            .spawn((AvatarCamera::new(Entity::PLACEHOLDER), Transform::default(), Projection::default()))
            .id();
        app.update();
        assert!(!app.world().get::<Camera>(view).unwrap().is_active, "the avatar camera takes over");
        assert_eq!(*app.world().get::<Transform>(viewer).unwrap(), Transform::default(), "a Custom camera leaves the avatar's alone");
        let mut q = app.world_mut().query_filtered::<(), With<TreeViewCamera>>();
        assert_eq!(q.iter(app.world()).count(), 1, "never a second one");
    }

    #[test]
    fn an_entity_bound_from_elsewhere_is_never_despawned() {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let model = dm.create("Model");
        dm.set_parent(model, Some(ws)).unwrap();
        let root = part(&mut dm, model, [0.0; 3], [2.0, 2.0, 1.0]);
        let tree = PlayerTree::new(dm, "Tester");
        let mut app = app(&tree);
        app.update();
        let drawn_root = tree.dm.lock().entity_of(root).map(Entity::from_bits).expect("drawn before it is a character");

        // What a binder does: take the root from the drawn part and hand it
        // to the avatar standing for the character.
        let avatar = app.world_mut().spawn(Transform::default()).id();
        {
            let mut g = tree.dm.lock();
            g.unbind_entity(root);
            g.bind_entity(root, avatar.to_bits());
            let me = g.local_player.unwrap();
            g.set_prop(me, "Character", DmValue::Instance(model)).unwrap();
        }
        app.world_mut().despawn(drawn_root);
        app.update();
        assert!(app.world().get_entity(avatar).is_ok(), "a Character write leaves the avatar alive");
        assert_eq!(tree.dm.lock().entity_of(root), Some(avatar.to_bits()), "and bound");

        let storage = tree.dm.lock().get_service("ReplicatedStorage").unwrap();
        tree.dm.lock().set_parent(model, Some(storage)).unwrap();
        app.update();
        assert!(app.world().get_entity(avatar).is_ok(), "a reparent leaves it alive");

        tree.dm.lock().destroy(model);
        app.update();
        assert!(app.world().get_entity(avatar).is_ok(), "a destroy leaves it alive");
    }

    #[test]
    fn queues_with_no_consumer_here_never_grow() {
        use crate::datamodel::{OutputLevel, OutputLine};

        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let wall = part(&mut dm, ws, [0.0; 3], [1.0; 3]);
        let tree = PlayerTree::new(dm, "Tester");
        let mut app = app(&tree);
        {
            let mut g = tree.dm.lock();
            g.output.push(OutputLine { level: OutputLevel::Info, source: "Hud".into(), text: "hi".into() });
            g.particle_emits.push((wall, 4));
            g.touch_watch.push(wall);
        }
        app.update();
        let g = tree.dm.lock();
        assert!(g.output.is_empty() && g.particle_emits.is_empty() && g.touch_watch.is_empty());
    }
}

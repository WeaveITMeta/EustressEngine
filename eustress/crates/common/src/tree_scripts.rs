//! LocalScripts on the Player, in a Play VM of its own.
//!
//! The Player keeps a tree of its own, [`PlayDataModel`]: the world's reader
//! fills it, the host's replication writes into it, and the shared play
//! runtime draws it. This runs that tree's LocalScripts in a Play VM, so a
//! Space's client code (its HUD, its input, its camera work) behaves on the
//! Player as it does in Studio's Play session.
//!
//! What runs here:
//!
//! - LocalScripts in `ReplicatedFirst`, ahead of the rest.
//! - LocalScripts the host replicated into this player: `PlayerScripts`,
//!   `PlayerGui`, `Backpack`, and the character.
//! - LocalScripts under `StarterPlayer`, leaving the `StarterCharacterScripts`
//!   templates alone, and in `Workspace`, which is what Studio runs too.
//!   Inside a character, a LocalScript runs only in this player's own, as in
//!   Roblox. A character is the nearest Model above the script that is some
//!   Player's `Character` or that holds a Humanoid, so another player's
//!   character (with or without a Humanoid) and an NPC both run nothing here.
//!
//! A `Script` never runs here: the host owns the server, and a `Script`'s
//! `Source` is never replicated anyway. ModuleScripts start through `require`.
//!
//! Scripts that arrive later start later. The host owns each player's
//! `PlayerGui` and replicates it once they have joined, so the launch list is
//! rebuilt whenever the tree's shape changes, and [`PlayLuau::run_scripts`]
//! skips whatever is already running.
//!
//! ## Where this sits in the frame
//!
//! The Player runs Studio's chain, [`PlayScriptSet`] `Pull`, `Scripts`,
//! `Apply`, `End`, with replication ordered before `Pull`. This VM runs in
//! `Scripts`, so it reads every event replication and the pull wrote this
//! frame, and a script's own writes draw in `Apply` the same frame. After each
//! frame the VM records how far it has read in [`PlayEventCursor`], and `End`
//! trims the tree's events only that far, so an event this VM has not read is
//! never dropped. With no VM (no world open, or its VM failed to build) the
//! cursor is `None` and `End` trims everything. [`RunTreeScripts`] is a set
//! inside `PlayScriptSet::Scripts`, for whatever still orders itself against
//! it.
//!
//! [`TreeInputPull`], in `Pull`, fills the tree's input, its `CurrentCamera`
//! and its mouse from this machine through [`crate::machine_input`], the
//! per-machine pull Studio's Play shares, with the whole window as the view.
//!
//! A new world is a new [`PlayDataModel`]. The VM is bound to the tree it was
//! made for, so a new tree drops it and the next frame builds one for the new
//! tree.
//!
//! With `physics` on, `workspace:Raycast` answers against the Player's own
//! Avian world, and a hit maps back to an instance because the draw step binds
//! every part it draws to its entity. Terrain is not handed to the VM yet, so
//! a terrain read sees none.

use bevy::prelude::*;

use crate::datamodel::{DataModel, InstanceId, OutputLevel};
use crate::luau::play::{PlayLuau, RayHit, RayQuery, ScriptLaunch, TerrainView};
use crate::play_session::{PlayDataModel, PlayEventCursor, PlayRole, PlayScriptSet};

#[cfg(feature = "physics")]
use crate::classes::BasePart;
#[cfg(feature = "physics")]
use crate::scripting::Vector3;
#[cfg(feature = "physics")]
use crate::terrain::TerrainChunkCollider;
#[cfg(feature = "physics")]
use avian3d::prelude::{Sensor, SpatialQuery, SpatialQueryFilter};

/// The Player's Luau VM, made once a [`PlayDataModel`] exists.
#[derive(Resource)]
pub struct TreeScripts {
    vm: PlayLuau,
    /// The tree shape and the local character the launch list was built
    /// from. The walk reruns when either moves: a join or a leave changes the
    /// shape, while a respawn points `Player.Character` at a new model, which
    /// is only a property write and leaves the shape alone. Without the
    /// character in the key, a respawn whose `Character` write lands a frame
    /// after the model would leave the new character's scripts unstarted.
    seen: (u64, Option<InstanceId>),
}

impl TreeScripts {
    /// The VM, for a shell that wants to drive it further.
    pub fn vm(&mut self) -> &mut PlayLuau {
        &mut self.vm
    }
}

/// Running the tree's LocalScripts: a set inside `PlayScriptSet::Scripts`,
/// kept while anything still orders itself against it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunTreeScripts;

/// Runs a [`PlayDataModel`]'s LocalScripts in `PlayScriptSet::Scripts`.
pub struct TreeScriptsPlugin;

impl Plugin for TreeScriptsPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(Update, RunTreeScripts.in_set(PlayScriptSet::Scripts))
            .configure_sets(Update, TreeInputPull.in_set(PlayScriptSet::Pull))
            .add_systems(Update, pull_tree_input.in_set(TreeInputPull))
            .add_systems(Update, run_tree_scripts.in_set(RunTreeScripts));
        #[cfg(feature = "physics")]
        app.add_systems(Update, pull_tree_view.in_set(TreeInputPull).after(pull_tree_input));
    }
}

/// Filling the tree's input, camera and mouse from this machine, in
/// `PlayScriptSet::Pull`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TreeInputPull;

/// Keys, buttons, cursor, wheel and motion into the tree, through the pull
/// Studio's Play shares. The whole window is the view. There are no editor
/// panels here; what the HUD took (a click on a button, a TextBox holding the
/// keyboard) still reaches scripts, marked game-processed, as in Studio.
fn pull_tree_input(
    tree: Option<Res<PlayDataModel>>,
    keyboard: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    motion: Option<Res<bevy::input::mouse::AccumulatedMouseMotion>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    keyboard_focus: Option<Res<crate::play_session::GuiKeyboardFocus>>,
    hud: Option<Res<crate::play_session::HudPointer>>,
) {
    use crate::machine_input::{write_input, DeviceFrame, Taken, ViewRect};

    let Some(tree) = tree else {
        wheel.clear();
        return;
    };
    let window = windows.single().ok();
    let rect = window.map(ViewRect::full).unwrap_or(ViewRect { origin: Vec2::ZERO, size: Vec2::new(1280.0, 720.0) });
    let frame = DeviceFrame {
        keyboard: keyboard.as_deref(),
        mouse: mouse.as_deref(),
        cursor: window.and_then(|w| w.cursor_position()).map(|c| rect.local(c)),
        motion: motion.as_deref().map_or(Vec2::ZERO, |m| m.delta),
        wheel: wheel.read().map(|w| w.y as f64).sum(),
        extra_motion: Vec2::ZERO,
        rect,
        taken: Taken {
            over_gui: hud.as_deref().is_some_and(|h| h.over_gui),
            typing: keyboard_focus.is_some_and(|f| f.0) || hud.as_deref().is_some_and(|h| h.typing),
            ..default()
        },
    };
    write_input(&mut tree.dm.lock(), &frame);
}

/// The rendering camera into `CurrentCamera`, and `Mouse.Hit`, `Target` and
/// `UnitRay` from the cursor through it. The camera is the active one of the
/// avatar's and the scripted play camera, the rule Studio's pull uses, never
/// any other 3D camera (an overlay camera would bend the ray), and the local
/// avatar never blocks its own cursor.
#[cfg(feature = "physics")]
fn pull_tree_view(
    tree: Option<Res<PlayDataModel>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<
        (&Camera, &GlobalTransform, &Projection),
        Or<(With<crate::avatar::control::AvatarCamera>, With<crate::play_session::ScriptedPlayCamera>)>,
    >,
    spatial: SpatialQuery,
    colliders: crate::machine_input::MouseColliders,
    avatars: Query<Entity, With<crate::avatar::LocalAvatar>>,
) {
    use crate::machine_input::{write_camera, write_mouse, ViewRect};

    let Some(tree) = tree else { return };
    let Ok(window) = windows.single() else { return };
    let Some((camera, pose, projection)) = cameras.iter().find(|(c, ..)| c.is_active) else { return };
    let rect = ViewRect::full(window);
    write_camera(&mut tree.dm.lock(), pose, projection, rect);
    let cursor = window.cursor_position().map(|c| rect.local(c));
    write_mouse(&tree.dm, camera, pose, rect, cursor, &spatial, &colliders, avatars.iter());
}

fn run_tree_scripts(
    mut commands: Commands,
    // The tree a VM failed to build for, so that tree is not retried every
    // frame while a newly opened world still gets its own attempt.
    mut gave_up_on: Local<Option<usize>>,
    tree: Option<Res<PlayDataModel>>,
    scripts: Option<ResMut<TreeScripts>>,
    role: Option<Res<PlayRole>>,
    mut cursor: Option<ResMut<PlayEventCursor>>,
    #[cfg(feature = "physics")] spatial: SpatialQuery,
    #[cfg(feature = "physics")] colliders: Query<(Option<&BasePart>, Has<Sensor>, Has<TerrainChunkCollider>)>,
    #[cfg(test)] fail_build: Option<Res<tests::FailVmBuild>>,
) {
    let Some(tree) = tree else {
        // No world open: the last world's VM stops with it.
        if scripts.is_some() {
            commands.remove_resource::<TreeScripts>();
        }
        if let Some(c) = cursor.as_mut() {
            c.0 = None;
        }
        return;
    };

    // Opening a Space inserts a new tree. A VM is bound to the tree it was
    // made for, so a new tree drops it, and the next frame makes one for the
    // new tree. Without this the old world's scripts would keep running
    // against the old tree while the new world's never started.
    if let Some(s) = &scripts {
        if !std::sync::Arc::ptr_eq(s.vm.datamodel(), &tree.dm) {
            commands.remove_resource::<TreeScripts>();
            if let Some(c) = cursor.as_mut() {
                c.0 = None;
            }
            return;
        }
    }

    let Some(mut scripts) = scripts else {
        let this_tree = std::sync::Arc::as_ptr(&tree.dm) as usize;
        if *gave_up_on == Some(this_tree) {
            return;
        }
        #[cfg(test)]
        let built = if fail_build.is_some() {
            Err("a build failure the test asked for".to_string())
        } else {
            PlayLuau::new(tree.dm.clone())
        };
        #[cfg(not(test))]
        let built = PlayLuau::new(tree.dm.clone());
        match built {
            Ok(vm) => {
                commands.insert_resource(TreeScripts { vm, seen: (u64::MAX, None) });
            }
            Err(e) => {
                *gave_up_on = Some(this_tree);
                tree.dm.lock().print(OutputLevel::Error, "Luau", e);
                if let Some(c) = cursor.as_mut() {
                    c.0 = None;
                }
            }
        }
        return;
    };

    #[cfg(feature = "physics")]
    let raycast = |q: &RayQuery| cast_ray(&spatial, &colliders, q);
    #[cfg(not(feature = "physics"))]
    let raycast = |_q: &RayQuery| -> Option<RayHit> { None };

    // Rebuild the launch list only when the tree's shape moved. The lock is
    // released before the VM runs, because the VM takes it itself.
    let key = {
        let mut g = tree.dm.lock();
        // The role decides it: a Replica is not the server, the Authority is.
        if let Some(role) = &role {
            g.is_server = matches!(**role, PlayRole::Authority);
        }
        let character = g.local_player.and_then(|p| g.get_prop(p, "Character")).and_then(|v| v.as_instance());
        (g.structure_version, character)
    };
    if key != scripts.seen {
        scripts.seen = key;
        let launches = {
            let g = tree.dm.lock();
            collect_client_launches(&g)
        };
        if !launches.is_empty() {
            scripts.vm.run_scripts(launches, &raycast, &no_terrain);
        }
    }

    scripts.vm.frame(&raycast, &no_terrain);
    if let Some(c) = cursor.as_mut() {
        c.0 = Some(scripts.vm.event_cursor());
    }
}

/// `workspace:Raycast` against the Player's own physics world, with Roblox's
/// filter rules. The hit reports its entity, which the VM maps back to an
/// instance through the binding the draw step made when it drew the part.
#[cfg(feature = "physics")]
fn cast_ray(
    spatial: &SpatialQuery,
    colliders: &Query<(Option<&BasePart>, Has<Sensor>, Has<TerrainChunkCollider>)>,
    q: &RayQuery,
) -> Option<RayHit> {
    let origin = q.origin.to_vec3();
    let dir = q.direction.to_vec3();
    let len = dir.length();
    if !len.is_finite() || len < 1e-6 {
        return None;
    }
    let direction = Dir3::new(dir / len).ok()?;
    let listed: std::collections::HashSet<u64> = q.filter.iter().copied().collect();
    let mut filter = SpatialQueryFilter::default();
    if !q.include && !listed.is_empty() {
        filter = filter.with_excluded_entities(listed.iter().map(|b| Entity::from_bits(*b)));
    }
    let hit = spatial.cast_ray_predicate(origin, direction, len, true, &filter, &|e| {
        let (part, sensor, terrain) = colliders.get(e).unwrap_or((None, false, false));
        if terrain {
            // Include lists take the Terrain only when they name it; exclude
            // lists skip it only when they do.
            return q.include == q.terrain_listed;
        }
        if q.include && !listed.contains(&e.to_bits()) {
            return false;
        }
        if q.respect_can_collide && (sensor || part.map_or(false, |b| !b.can_collide)) {
            return false;
        }
        true
    })?;
    let p = origin + *direction * hit.distance;
    Some(RayHit {
        entity: hit.entity.to_bits(),
        position: Vector3::from_vec3(p),
        normal: Vector3::from_vec3(hit.normal),
        distance: hit.distance as f64,
        terrain: colliders.get(hit.entity).map_or(false, |(_, _, terrain)| terrain),
    })
}

/// This shell hands the VM no terrain, so a read sees none.
fn no_terrain(_visit: &mut dyn for<'t> FnMut(TerrainView<'t>)) {}

/// The LocalScripts this shell should be running, `ReplicatedFirst` ahead of
/// the rest and each group ordered by full name so starts are deterministic.
fn collect_client_launches(g: &DataModel) -> Vec<ScriptLaunch> {
    let mut first = Vec::new();
    let mut rest = Vec::new();

    let characters = player_characters(g);
    let local_character = g.local_player.and_then(|p| g.get_prop(p, "Character")).and_then(|v| v.as_instance());

    // The player may sit outside the tree walk, so walk its subtree too.
    let mut stack = vec![g.root()];
    if let Some(p) = g.local_player {
        stack.push(p);
    }
    while let Some(id) = stack.pop() {
        for c in g.children(id) {
            stack.push(*c);
        }
        let Some(inst) = g.get(id) else { continue };
        // A Script belongs to the host, and arrives with no Source anyway.
        if inst.class_name != "LocalScript" {
            continue;
        }
        if g.get_prop(id, "Disabled").and_then(|v| v.as_bool()).unwrap_or(false)
            || !g.get_prop(id, "Enabled").and_then(|v| v.as_bool()).unwrap_or(true)
        {
            continue;
        }
        let source = g.get_prop(id, "Source").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        if source.trim().is_empty() {
            continue;
        }

        let in_player = g.local_player.map_or(false, |p| g.is_descendant_of(id, p));
        let service = g.service_of(id).and_then(|s| g.class_of(s).map(str::to_string)).unwrap_or_default();
        let runs = in_player
            || match service.as_str() {
                "ReplicatedFirst" => true,
                // Inside a character, only this player's own.
                "Workspace" => match character_of(g, id, &characters) {
                    Some(c) => Some(c) == local_character,
                    None => true,
                },
                // Templates under StarterCharacterScripts are cloned into a
                // character; they run there, not where they are kept.
                "StarterPlayer" => g.find_first_ancestor(id, "StarterCharacterScripts").is_none(),
                "StarterPlayerScripts" => true,
                _ => false,
            };
        if !runs {
            continue;
        }

        let launch = ScriptLaunch { instance: id, source, chunk_name: g.full_name(id) };
        if service == "ReplicatedFirst" {
            first.push(launch);
        } else {
            rest.push(launch);
        }
    }

    first.sort_by(|a, b| a.chunk_name.cmp(&b.chunk_name));
    rest.sort_by(|a, b| a.chunk_name.cmp(&b.chunk_name));
    first.extend(rest);
    first
}

/// Every Model some Player's `Character` points at.
fn player_characters(g: &DataModel) -> std::collections::HashSet<InstanceId> {
    let Some(players) = g.find_service("Players") else { return Default::default() };
    g.children(players)
        .iter()
        .filter_map(|&p| g.get_prop(p, "Character").and_then(|v| v.as_instance()))
        .collect()
}

/// The character `id` sits in: the nearest Model above it that is some
/// Player's `Character` or that holds a Humanoid. The Humanoid test catches
/// an NPC, and a new character before its `Character` write lands; the
/// Player test catches a character built without a Humanoid.
fn character_of(
    g: &DataModel,
    id: InstanceId,
    characters: &std::collections::HashSet<InstanceId>,
) -> Option<InstanceId> {
    let mut cur = g.parent(id);
    while let Some(p) = cur {
        if characters.contains(&p)
            || (g.class_of(p) == Some("Model") && g.find_first_child_of_class(p, "Humanoid", false).is_some())
        {
            return Some(p);
        }
        cur = g.parent(p);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datamodel::{DmValue, SharedDataModel};
    use std::sync::{Arc, Weak};

    /// While present, every VM build fails, for the failure path.
    #[derive(Resource)]
    pub(super) struct FailVmBuild;

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Time>()
            .add_message::<bevy::input::mouse::MouseWheel>()
            .add_plugins(TreeScriptsPlugin);
        // `SpatialQuery` needs the collider trees; empty ones are enough.
        #[cfg(feature = "physics")]
        app.init_resource::<avian3d::collider_tree::ColliderTrees>();
        app
    }

    /// A world as the Player opens one: its local Player, then the session's
    /// start through `begin_session`, the one function Studio and the Player
    /// both call.
    fn world(mut dm: DataModel) -> PlayDataModel {
        use crate::play_session::{begin_session, SessionCamera, SessionStart};
        let players = dm.get_service("Players");
        let me = dm.create_virtual("Player", "Tester", players);
        let start = SessionStart {
            camera: SessionCamera::SpaceOrFresh,
            gravity: crate::services::workspace::DEFAULT_GRAVITY,
            character_auto_loads: true,
        };
        begin_session(&mut dm, me, &start);
        PlayDataModel { dm: Arc::new(parking_lot::Mutex::new(dm)) }
    }

    /// A world whose one LocalScript, in ReplicatedFirst, prints `says`.
    fn world_saying(says: &str) -> PlayDataModel {
        let mut dm = DataModel::new();
        let first = dm.get_service("ReplicatedFirst").unwrap();
        let script = dm.create("LocalScript");
        dm.set_prop(script, "Source", DmValue::String(format!("print({says:?})"))).unwrap();
        dm.set_parent(script, Some(first)).unwrap();
        world(dm)
    }

    fn printed(dm: &SharedDataModel, text: &str) -> bool {
        dm.lock().output.iter().any(|l| l.text == text)
    }

    fn errors(dm: &SharedDataModel) -> usize {
        dm.lock().output.iter().filter(|l| l.level == OutputLevel::Error).count()
    }

    fn updates(app: &mut App, n: usize) {
        for _ in 0..n {
            app.update();
        }
    }

    #[test]
    fn a_new_world_drops_the_old_vm_and_starts_its_own_scripts() {
        let mut app = app();
        let a = world_saying("world A");
        let old: Weak<_> = Arc::downgrade(&a.dm);
        app.insert_resource(a);
        updates(&mut app, 2);
        assert!(printed(&old.upgrade().unwrap(), "world A"), "the first world's script ran");

        let b = world_saying("world B");
        let b_dm = b.dm.clone();
        app.insert_resource(b);
        app.update();
        assert!(old.upgrade().is_none(), "the old world's VM let go of its tree");

        updates(&mut app, 2);
        assert!(printed(&b_dm, "world B"), "the new world's script ran");
    }

    #[test]
    fn a_failed_build_is_not_retried_and_does_not_block_the_next_world() {
        let mut app = app();
        app.insert_resource(FailVmBuild);
        let a = world_saying("world A");
        let a_dm = a.dm.clone();
        app.insert_resource(a);
        updates(&mut app, 3);
        assert_eq!(errors(&a_dm), 1, "the failed world is tried once, not every frame");
        assert!(app.world().get_resource::<TreeScripts>().is_none());

        // Remembered per world: the same world is still not retried.
        app.world_mut().remove_resource::<FailVmBuild>();
        updates(&mut app, 2);
        assert_eq!(errors(&a_dm), 1, "a world whose VM failed stays as it was");
        assert!(!printed(&a_dm, "world A"));

        let b = world_saying("world B");
        let b_dm = b.dm.clone();
        app.insert_resource(b);
        updates(&mut app, 3);
        assert!(printed(&b_dm, "world B"), "the next world builds its own VM and runs");
    }

    /// A joined Player's tree: the world as read, plus what the host
    /// replicates onto the local Player, and one LocalScript in its PlayerGui
    /// reading what every client script starts from.
    #[test]
    fn a_joined_players_local_script_sees_its_player_mouse_camera_and_gui() {
        let mut app = app();
        let tree = world(DataModel::new());
        let dm = tree.dm.clone();
        {
            let mut g = dm.lock();
            let me = g.local_player.expect("the tree has a local player");
            let mut gui = None;
            for class in ["PlayerGui", "Backpack", "PlayerScripts"] {
                let id = g.create_virtual(class, class, Some(me));
                if class == "PlayerGui" {
                    gui = Some(id);
                }
            }
            let script = g.create("LocalScript");
            g.set_prop(script, "Source", DmValue::String(r#"
                local me = game:GetService("Players").LocalPlayer
                print("LocalPlayer " .. tostring(me ~= nil))
                print("Mouse " .. tostring(me ~= nil and me:GetMouse() ~= nil))
                print("CurrentCamera " .. tostring(workspace.CurrentCamera ~= nil))
                print("PlayerGui " .. tostring(me ~= nil and me.PlayerGui ~= nil))
                print("Gravity " .. tostring(workspace.Gravity))
            "#.into())).unwrap();
            g.set_parent(script, gui).unwrap();
        }
        app.insert_resource(tree);
        updates(&mut app, 2);
        let out: Vec<String> = dm.lock().output.iter().map(|l| l.text.clone()).collect();
        for line in ["LocalPlayer true", "Mouse true", "CurrentCamera true", "PlayerGui true", "Gravity 9.80665"] {
            assert!(printed(&dm, line), "{line}, from {out:?}");
        }
        assert_eq!(errors(&dm), 0, "{out:?}");
    }

    #[test]
    fn device_input_reaches_the_tree_with_studios_names() {
        use crate::datamodel::InputPhase;
        let mut app = app();
        app.init_resource::<ButtonInput<KeyCode>>().init_resource::<ButtonInput<MouseButton>>();
        let w = world(DataModel::new());
        let dm = w.dm.clone();
        app.insert_resource(w);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();

        let g = dm.lock();
        assert!(g.input.keys.contains("W"), "held keys use Roblox KeyCode names");
        assert!(g.input.buttons.contains("MouseButton1"));
        let began = |ty: &str, key: &str| {
            g.input.events.iter().any(|e| e.phase == InputPhase::Began && e.input_type == ty && e.key_code == key)
        };
        assert!(began("Keyboard", "W"), "a key press is an InputBegan");
        assert!(began("MouseButton1", "Unknown"), "a left click is what Button1Down fires from");
    }
}

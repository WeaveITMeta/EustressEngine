//! # A Play session's shared vocabulary
//!
//! What both apps name while a Space plays: the session's DataModel tree,
//! the four sets a Play frame runs in, the markers the draw step leaves on
//! what it makes, and the part of the window the 3D view fills. Studio's
//! Play and the Player run the same frame over them
//! (`docs/architecture/SHARED_PLAY_RUNTIME.md`).

use bevy::prelude::*;

use crate::datamodel::{DataModel, InstanceId, SharedDataModel};

/// The running session's tree.
#[derive(Resource, Clone)]
pub struct PlayDataModel {
    pub dm: SharedDataModel,
}

/// Systems of one Play frame, in order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlayScriptSet {
    Pull,
    Scripts,
    Apply,
    End,
}

/// Entities spawned for script-made instances. Despawned on Stop.
#[derive(Component, Debug, Clone, Copy)]
pub struct DataModelSpawned;

/// Set while a GUI TextBox holds the keyboard: the avatar and the camera
/// leave the keys to it.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct GuiKeyboardFocus(pub bool);

/// `workspace.Gravity` as scripts read it: the downward magnitude of the
/// live gravity, so a Space set to the Moon reads 1.62. Read through the
/// f32's shortest decimal form, or standard gravity would print as
/// 9.806650161743164, and weightless as 0 rather than -0.
pub fn workspace_gravity(live: Vec3) -> f64 {
    let down = if live.y == 0.0 { 0.0 } else { -live.y };
    down.to_string().parse::<f64>().unwrap_or(down as f64)
}

/// `Players.RespawnTime` as Play starts: seconds from a character's death
/// to its respawn.
pub const RESPAWN_TIME_SECS: f64 = 5.0;

/// The Space's `Players.CharacterAutoLoads`, from `Players/_service.toml`
/// (`character_auto_loads` in `[properties]`, at the top level, or in
/// `[service]`, the first of those winning): on unless the Space turns it
/// off, as for a paddle game played through its camera and input alone.
pub fn space_character_auto_loads(space_root: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(space_root.join("Players").join("_service.toml")) else { return true };
    let Ok(doc) = text.parse::<toml::Table>() else { return true };
    let pick = |t: &toml::Table| {
        t.get("character_auto_loads").or_else(|| t.get("CharacterAutoLoads")).and_then(toml::Value::as_bool)
    };
    let section = |name: &str| doc.get(name).and_then(toml::Value::as_table).and_then(|t| pick(t));
    section("properties").or_else(|| pick(&doc)).or_else(|| section("service")).unwrap_or(true)
}

/// The gravity a Space runs at, m/s², from `Workspace/_service.toml`
/// (`gravity` in `[properties]`, or at the top level), by the rule Studio
/// applies ([`crate::services::workspace::authored_gravity`]). A stud-era
/// value is Roblox's gravity only in a Roblox import, which marks its
/// Workspace file with a `[properties.extras]` table and wrote its lengths in
/// feet. No file, or no key, is standard gravity.
pub fn space_gravity(space_root: &std::path::Path) -> Vec3 {
    use crate::services::workspace::{authored_gravity, AuthoredGravity};
    let doc = std::fs::read_to_string(space_root.join("Workspace").join("_service.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok());
    let Some(doc) = doc else { return authored_gravity(None, None) };
    let props = doc.get("properties").and_then(toml::Value::as_table);
    let number = |v: &toml::Value| v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
    let key = match props.and_then(|p| p.get("gravity")).or_else(|| doc.get("gravity")) {
        Some(toml::Value::Array(xyz)) if xyz.len() == 3 => {
            let xyz: Option<Vec<f64>> = xyz.iter().map(number).collect();
            xyz.map(|v| AuthoredGravity::from_vector([v[0], v[1], v[2]]))
        }
        Some(value) => number(value).map(AuthoredGravity::Legacy),
        None => None,
    };
    let import_stud = if matches!(key, Some(AuthoredGravity::Legacy(_))) {
        props
            .and_then(|p| p.get("extras"))
            .and_then(toml::Value::as_table)
            .map(|_| crate::units::Unit::Foot)
    } else {
        None
    };
    authored_gravity(key, import_stud)
}

/// Which side of a session this app is. Studio's Play is the `Authority`: it
/// simulates, and hosts when it hosts. A joined Player is a `Replica`: the
/// host simulates, and the draw step shows what replication writes.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayRole {
    #[default]
    Authority,
    Replica,
}

impl PlayRole {
    /// The role an app runs as: `Authority` unless it inserted `Replica`.
    pub fn of(world: &World) -> PlayRole {
        world.get_resource::<PlayRole>().copied().unwrap_or_default()
    }
}

/// How far the session's script VM has read the tree's events. The frame's
/// end trims the log to it, so no event a script has not seen is dropped;
/// `None` (no VM) trims everything.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct PlayEventCursor(pub Option<u64>);

/// The open Space's folder, which a `MeshId` under it is written relative
/// to. Each app keeps it current: Studio from its Space asset source, the
/// Player from the world it opened.
#[derive(Resource, Debug, Clone, Default)]
pub struct PlayAssetRoot(pub std::path::PathBuf);

/// An entity a replica's draw step made for an instance, and the instance
/// it draws. The draw step gives back only these; the motion lane moves
/// only these.
#[derive(Component, Debug, Clone, Copy)]
pub struct FromTree(pub InstanceId);

/// The play camera for every moment without an avatar: the whole session
/// when the Space loads no character, otherwise only after a script
/// destroys it. Studio's `handle_start_play` spawns it at the editor's view;
/// a replica's draw step spawns it at the Space's camera. Active exactly
/// while no avatar camera exists.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ScriptedPlayCamera;

/// What the HUD did with this frame's input. Its input step writes it each
/// frame; each app's pull reads it for what a script sees as
/// game-processed.
#[derive(Resource, Debug, Default, Clone)]
pub struct HudPointer {
    /// The cursor is over a HUD element that takes the mouse.
    pub over_gui: bool,
    /// A TextBox holds the keyboard.
    pub typing: bool,
    /// Buttons pressed this frame, by entity.
    pub activated: Vec<Entity>,
}

/// How a session picks its `Workspace.CurrentCamera`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCamera {
    /// A new camera every session: Studio's Play, where the Space's saved
    /// camera is the editor's and Stop throws the session's away.
    Fresh,
    /// The Space's own camera when it saved one, else a new one: a joined
    /// Player, whose world has no editor.
    SpaceOrFresh,
}

/// What a session starts from, the same on every machine.
#[derive(Debug, Clone, Copy)]
pub struct SessionStart {
    pub camera: SessionCamera,
    /// The live gravity, which `workspace.Gravity` reports as its downward
    /// magnitude.
    pub gravity: Vec3,
    /// `Players.CharacterAutoLoads`.
    pub character_auto_loads: bool,
}

/// What [`begin_session`] set up.
#[derive(Debug, Clone, Copy)]
pub struct SessionIds {
    pub local_player: InstanceId,
    pub camera: Option<InstanceId>,
}

/// The Play setup every app runs on its session tree, so a LocalScript
/// starts from the same things in Studio's Play and on a joined Player:
/// `Players.LocalPlayer` (and the tree's own local player), a
/// `Workspace.CurrentCamera`, `workspace.Gravity`, `Players.RespawnTime` and
/// `Players.CharacterAutoLoads`. Each app makes its local `Player` first,
/// its own way (Studio's joins after the server scripts start; a Player's
/// sits under Players before replication binds), and passes it in.
pub fn begin_session(dm: &mut DataModel, local_player: InstanceId, start: &SessionStart) -> SessionIds {
    use crate::datamodel::DmValue;
    dm.local_player = Some(local_player);
    if let Some(players) = dm.get_service("Players") {
        let _ = dm.set_prop(players, "LocalPlayer", DmValue::Instance(local_player));
        let _ = dm.set_prop(players, "RespawnTime", DmValue::Number(RESPAWN_TIME_SECS));
        let _ = dm.set_prop(players, "CharacterAutoLoads", DmValue::Bool(start.character_auto_loads));
    }
    let mut camera = None;
    if let Some(workspace) = dm.get_service("Workspace") {
        let cam = match (start.camera, dm.current_camera()) {
            (SessionCamera::SpaceOrFresh, Some(saved)) => saved,
            _ => dm.create_virtual("Camera", "Camera", Some(workspace)),
        };
        let _ = dm.set_prop(workspace, "CurrentCamera", DmValue::Instance(cam));
        let _ = dm.set_prop(workspace, "Gravity", DmValue::Number(workspace_gravity(start.gravity)));
        camera = Some(cam);
    }
    SessionIds { local_player, camera }
}

/// Marker component to track parts that had their physics activated during play mode
#[derive(Component)]
pub struct PlayModePhysicsActivated;

/// Viewport bounds reported by Slint layout (in PHYSICAL pixels from top-left).
/// Used by the camera controller to clip 3D rendering to the viewport area.
///
/// IMPORTANT: these fields are physical pixels (logical × scale_factor). When
/// comparing against `Window::cursor_position()` — which returns LOGICAL
/// pixels — call `contains_logical` or divide by the window scale factor
/// first. Forgetting this is the bug that made 3D click selection silently
/// reject every click on any display with DPI scaling ≠ 1.0.
#[derive(Resource, Default, Clone, Copy)]
pub struct ViewportBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl ViewportBounds {
    /// Test whether a cursor point (in LOGICAL pixels, as returned by
    /// `Window::cursor_position()`) falls inside the viewport rectangle.
    /// Converts physical bounds to logical using the provided scale factor.
    pub fn contains_logical(&self, cursor: bevy::math::Vec2, scale_factor: f32) -> bool {
        if self.width <= 0.0 || self.height <= 0.0 { return true; }
        let s = scale_factor.max(0.0001);
        let x = self.x / s;
        let y = self.y / s;
        let w = self.width / s;
        let h = self.height / s;
        cursor.x >= x && cursor.x <= x + w && cursor.y >= y && cursor.y <= y + h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_space_runs_at_its_authored_gravity() {
        let space = |name: &str, workspace: Option<&str>| {
            let root = std::env::temp_dir().join(format!("eustress_space_gravity_{name}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("Workspace")).unwrap();
            if let Some(text) = workspace {
                std::fs::write(root.join("Workspace").join("_service.toml"), text).unwrap();
            }
            let g = space_gravity(&root);
            let _ = std::fs::remove_dir_all(&root);
            g
        };
        let standard = crate::services::workspace::DEFAULT_GRAVITY;
        assert_eq!(space("none", None), standard, "no Workspace file is standard gravity");
        assert_eq!(space("nokey", Some("[properties]\nname = \"Workspace\"\n")), standard);
        let moon = space("moon", Some("[properties]\ngravity = [0.0, -1.62, 0.0]\n"));
        assert!((moon - Vec3::new(0.0, -1.62, 0.0)).length() < 1e-5, "a vector is taken as written, got {moon}");
        assert_eq!(space("native", Some("[properties]\ngravity = 196.2\n")), standard, "a native stud-era number was never read");
        let import = space("import", Some("[properties]\ngravity = 196.2\n\n[properties.extras]\nFallenPartsDestroyHeight = -500\n"));
        assert!((import.y + 196.2 * 0.3048).abs() < 1e-3, "a Roblox import's studs are its feet, got {import}");
    }

    fn number(dm: &DataModel, id: InstanceId, name: &str) -> Option<f64> {
        dm.get_prop(id, name).and_then(|v| v.as_number())
    }

    /// Studio's Play and a joined Player start a session the same way; only
    /// the camera rule differs, and each keeps its own.
    #[test]
    fn both_apps_begin_a_session_the_same_way() {
        for rule in [SessionCamera::Fresh, SessionCamera::SpaceOrFresh] {
            let mut dm = DataModel::new();
            let ws = dm.get_service("Workspace").unwrap();
            let players = dm.get_service("Players").unwrap();
            let saved = dm.create_virtual("Camera", "Camera", Some(ws));
            let me = dm.create_virtual("Player", "Tester", Some(players));
            let start = SessionStart { camera: rule, gravity: Vec3::new(0.0, -1.62, 0.0), character_auto_loads: false };
            let ids = begin_session(&mut dm, me, &start);

            assert_eq!(dm.local_player, Some(me));
            assert_eq!(dm.get_prop(players, "LocalPlayer").and_then(|v| v.as_instance()), Some(me));
            assert_eq!(number(&dm, ws, "Gravity"), Some(1.62), "the Moon reads 1.62");
            assert_eq!(number(&dm, players, "RespawnTime"), Some(RESPAWN_TIME_SECS));
            assert_eq!(dm.get_prop(players, "CharacterAutoLoads").and_then(|v| v.as_bool()), Some(false));
            let current = dm.get_prop(ws, "CurrentCamera").and_then(|v| v.as_instance());
            assert_eq!(current, ids.camera);
            match rule {
                SessionCamera::Fresh => assert_ne!(current, Some(saved), "Studio's Play leaves the editor's camera alone"),
                SessionCamera::SpaceOrFresh => assert_eq!(current, Some(saved), "a Player shows the Space's own camera"),
            }
        }
    }
}

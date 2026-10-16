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

/// A Space's MaxPlayers when its Players service sets none.
pub const DEFAULT_MAX_PLAYERS: u16 = 10;
/// The largest MaxPlayers the gallery carries: its live heartbeat refuses a
/// session size above it. The only upper bound; the field says so.
pub const MAX_PLAYERS_LIMIT: u16 = 10_000;
/// Sessions larger than this have not been load-tested yet; the Publish
/// dialog says so, and never refuses or lowers the number for it.
pub const LOAD_TESTED_PLAYERS: u16 = 4;

/// A Space's `Players.MaxPlayers`: the most players one of its sessions
/// holds, everyone counted, the host's own character included when the
/// host plays (`docs/networking/MAX_PLAYERS.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceMaxPlayers {
    /// What the Space sets, when it sets a whole number of at least 1.
    pub authored: Option<u32>,
    /// What a session holds: the authored value (at most
    /// [`MAX_PLAYERS_LIMIT`], the gallery's own bound), or
    /// [`DEFAULT_MAX_PLAYERS`] when it sets none.
    pub players: u16,
}

impl SpaceMaxPlayers {
    /// From the property's value as read (a number, or none). A fraction,
    /// zero, a negative or a non-finite number counts as unset.
    pub fn from_value(value: Option<f64>) -> Self {
        let authored = value
            .filter(|v| v.is_finite() && v.fract() == 0.0 && *v >= 1.0)
            .map(|v| v.min(u32::MAX as f64) as u32);
        let players = authored.map_or(DEFAULT_MAX_PLAYERS, |n| n.min(MAX_PLAYERS_LIMIT as u32) as u16);
        Self { authored, players }
    }

    /// Larger than any session load-tested so far: worth a note, never a
    /// refusal.
    pub fn untested(&self) -> bool {
        self.players > LOAD_TESTED_PLAYERS
    }
}

/// The Space's `Players.MaxPlayers`, from `Players/_service.toml`
/// (`max_players` or `MaxPlayers`, in `[properties]`, at the top level, or
/// in `[service]`, the first of those winning). The one place every reader
/// takes it from: the host's session, the Publish dialog and the publish.
pub fn space_max_players(space_root: &std::path::Path) -> SpaceMaxPlayers {
    let doc = std::fs::read_to_string(space_root.join("Players").join("_service.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok());
    let Some(doc) = doc else { return SpaceMaxPlayers::from_value(None) };
    let number = |v: &toml::Value| v.as_integer().map(|i| i as f64).or_else(|| v.as_float());
    let pick = |t: &toml::Table| t.get("max_players").or_else(|| t.get("MaxPlayers")).and_then(number);
    let section = |name: &str| doc.get(name).and_then(toml::Value::as_table).and_then(|t| pick(t));
    SpaceMaxPlayers::from_value(section("properties").or_else(|| pick(&doc)).or_else(|| section("service")))
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
    fill_backpack(dm, local_player);
    SessionIds { local_player, camera }
}

/// Roblox's StarterPack: a copy of each child of the Space's StarterPack goes
/// into the player's Backpack when the player spawns. The originals stay in
/// the StarterPack. Called once per session, at spawn; `find_service` never
/// creates the StarterPack, so a Space without one leaves the Backpack empty.
pub fn fill_backpack(dm: &mut DataModel, player: InstanceId) {
    let (Some(pack), Some(backpack)) = (dm.find_service("StarterPack"), dm.find_first_child(player, "Backpack", false)) else {
        return;
    };
    for tool in dm.children(pack).to_vec() {
        if let Some(copy) = dm.clone_instance(tool) {
            let _ = dm.set_parent(copy, Some(backpack));
        }
    }
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

    /// A Space's StarterPack tools reach the player's Backpack when the session
    /// begins, as copies: the originals stay in the StarterPack.
    #[test]
    fn a_player_starts_with_a_copy_of_each_starter_pack_tool() {
        let mut dm = DataModel::new();
        let pack = dm.get_service("StarterPack").expect("StarterPack");
        let sword = dm.create_virtual("Tool", "Sword", Some(pack));
        let bat = dm.create_virtual("Tool", "Bat", Some(pack));
        let players = dm.get_service("Players").expect("Players");
        let me = dm.create_virtual("Player", "Tester", Some(players));
        dm.create_virtual("Backpack", "Backpack", Some(me));
        let start = SessionStart {
            camera: SessionCamera::Fresh,
            gravity: crate::services::workspace::DEFAULT_GRAVITY,
            character_auto_loads: true,
        };
        begin_session(&mut dm, me, &start);

        let backpack = dm.find_first_child(me, "Backpack", false).expect("Backpack");
        let mut names: Vec<String> = dm
            .children(backpack)
            .iter()
            .filter_map(|c| dm.get(*c).map(|i| i.name.clone()))
            .collect();
        names.sort();
        assert_eq!(names, ["Bat", "Sword"], "each StarterPack tool is copied into the Backpack");
        let copies_of: Vec<Option<InstanceId>> = dm.children(backpack).iter().map(|c| dm.get(*c).and_then(|i| i.clone_of)).collect();
        assert!(copies_of.iter().all(|c| c.is_some()), "the Backpack holds copies, not the originals: {copies_of:?}");
        assert_eq!(dm.children(pack), &[sword, bat], "the originals stay in the StarterPack");
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

    /// A Space's MaxPlayers is read from its Players service as file key or
    /// Roblox name and taken as authored, with no cap but the gallery's own
    /// bound; a Space that sets none, or something that is not a whole
    /// number of at least 1, holds the default.
    #[test]
    fn a_space_says_how_many_players_a_session_holds() {
        let space = |tag: &str, text: Option<&str>| {
            let root = std::env::temp_dir().join(format!("eustress-max-players-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("Players")).unwrap();
            if let Some(text) = text {
                std::fs::write(root.join("Players").join("_service.toml"), text).unwrap();
            }
            let read = space_max_players(&root);
            let _ = std::fs::remove_dir_all(&root);
            read
        };
        let four = space("four", Some("[properties]\ncharacter_auto_loads = true\nmax_players = 4\n"));
        assert_eq!(four, SpaceMaxPlayers { authored: Some(4), players: 4 });
        assert!(!four.untested());
        assert_eq!(space("roblox", Some("[properties]\nMaxPlayers = 2\n")).players, 2);
        let template = space("template", Some("[properties]\nmax_players = 12\n"));
        assert_eq!(template, SpaceMaxPlayers { authored: Some(12), players: 12 }, "taken as authored");
        assert!(template.untested(), "a note, not a cap");
        let huge = space("huge", Some("[properties]\nmax_players = 50000\n"));
        assert_eq!((huge.authored, huge.players), (Some(50_000), MAX_PLAYERS_LIMIT), "the gallery's bound");
        assert_eq!(space("none", None), SpaceMaxPlayers { authored: None, players: DEFAULT_MAX_PLAYERS });
        for odd in ["0", "-3", "2.5", "\"four\"", "nan"] {
            let read = space("odd", Some(&format!("[properties]\nmax_players = {odd}\n")));
            assert_eq!(read.authored, None, "{odd}");
            assert_eq!(read.players, DEFAULT_MAX_PLAYERS, "{odd}");
        }
    }
}

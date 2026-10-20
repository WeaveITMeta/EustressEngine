// A release Player is a window, not a console program: a clicked
// eustress-player:// link must not open a console beside it. Its log goes to
// a file instead (`player_log_fmt_layer`).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Eustress Client - Generative Player & Renderer
//!
//! This is the client that plays Eustress scenes with AI-enhanced rendering,
//! procedural generation, and next-gen visual effects.
//!
//! ## Features
//! - Uses the SAME default scene as Eustress Studio (shared code)
//! - Universal character that works in any scene
//! - Physics-based movement with Avian3D
//!
//! ## Plugins (Roblox-style services)
//! - PlayerServicePlugin: Player spawning, character controller, local player
//! - LightingServicePlugin: Skybox, sun, ambient, fog
//! - PhysicsPlugins: Avian3D physics

mod components;
mod systems;
mod plugins;
mod soul;

use bevy::prelude::*;
use avian3d::prelude::*;
use eustress_common::{spawn_baseplate, spawn_welcome_cube};
use eustress_common::services::TeamServicePlugin;
use eustress_networking::JoinLink;
use eustress_networking::join_link::{parse_join_link, split_link_extras, LinkExtras};
use plugins::{
    EnhancementPlugin, PlayerServicePlugin, LightingServicePlugin,
    PauseMenuPlugin, ClientTerrainPlugin, CharacterAnimationPlugin,
};
use eustress_common::plugins::SkinnedCharacterPlugin;
use eustress_common::avatar::{
    AvatarHost, AvatarRuntimePlugin,
};
use eustress_common::avatar::control::AvatarEscapePressed;
use systems::LoadSceneEvent;
use std::path::PathBuf;

const USAGE: &str = "\
usage:
  eustress-client                          open the local Space (EUSTRESS_SPACE, else the movement course)
  eustress-client <scene file>             open a legacy scene file
  eustress-client --connect <host:port>    join a Studio host; on this computer the key and pin are found automatically
      [--key <join key>] [--pin <64 hex>] [--name <display name>]
  eustress-client eustress-player://join/<host:port>?key=...&pin=...
  eustress-client --sim <simulation id>    play a published simulation
  eustress-client eustress-player://play/<simulation id>
  eustress-client <Space directory>       open that Space (a directory that contains Workspace/)
  eustress-client --help                   print this usage and exit
  eustress-client --version                print the version and exit
  --api-url <url>                          use this Eustress API (as EUSTRESS_API_URL does); the
                                           eustress-player:// link a staging Player registers carries it
links written with eustress:// in place of eustress-player:// open the same way
a path that is not a scene file or a Space directory is an error";

/// `from_args` returns this, and nothing else, when the player asked for
/// usage. `main` prints [`USAGE`] and exits 0.
const USAGE_REQUEST: &str = "usage";

/// The Player's own URL scheme. On Windows one application owns a scheme, and
/// `eustress://` belongs to Studio, so a Player installer registers this one;
/// both spell the same links.
const PLAYER_SCHEME: &str = "eustress-player://";
/// Studio's scheme. A link in either scheme is read in this one.
const STUDIO_SCHEME: &str = "eustress://";

/// What follows the scheme when `arg` is a link to the Player: it starts with
/// [`PLAYER_SCHEME`] or [`STUDIO_SCHEME`], in any case, with any spaces
/// around it. Such an argument is always read as a link, so it never becomes
/// a scene file's path, which the log shows.
fn player_link(arg: &str) -> Option<&str> {
    let arg = arg.trim();
    for scheme in [PLAYER_SCHEME, STUDIO_SCHEME] {
        if arg.get(..scheme.len()).is_some_and(|head| head.eq_ignore_ascii_case(scheme)) {
            return Some(&arg[scheme.len()..]);
        }
    }
    None
}

/// An unknown option's name, without whatever follows it, for its error: an
/// argument such as `--connect=<link>` must not put the link in a message.
fn option_name(arg: &str) -> String {
    arg.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').take(40).collect()
}

/// A positional argument that names something on disk.
enum LocalPath {
    Scene(PathBuf),
    Space(PathBuf),
}

fn is_scene_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("eustress" | "eustressengine" | "ron" | "escene" | "json")
    )
}

/// A scene file, or a Space directory (`Workspace/` inside it). Anything
/// else is an error, so a typo does not open the movement course.
fn local_path(raw: &str) -> Result<LocalPath, String> {
    let path = PathBuf::from(raw);
    if path.is_dir() && path.join("Workspace").is_dir() {
        return Ok(LocalPath::Space(path));
    }
    if path.is_file() && is_scene_file(&path) {
        return Ok(LocalPath::Scene(path));
    }
    if !path.exists() {
        return Err(format!("no such scene file or Space directory: {raw}"));
    }
    Err(format!("not a scene file or a Space directory (a Space contains Workspace/): {raw}"))
}

/// How the Player was launched. Not `Clone`: a join can carry the ticket a
/// gallery's Play page put in its link, which is never copied
/// (`docs/networking/PLAYER_JOIN_TICKET.md`). Its `Debug` prints no ticket
/// text, since [`LinkExtras`]'s prints none.
#[derive(Debug)]
pub enum Launch {
    /// A Space from this computer, or a legacy scene file.
    ///
    /// `space_dir` is a Space directory the argument named (it contains
    /// `Workspace/`). `main` points `EUSTRESS_SPACE` at it, so the movement
    /// course is not opened instead. `scene_path` is a legacy scene file.
    Local { scene_path: Option<PathBuf>, space_dir: Option<PathBuf> },
    /// Join a Studio host. `extras`: the ticket and API an
    /// `eustress-player://` link carries; none for `--connect`.
    Join { link: JoinLink, name: String, extras: LinkExtras },
    /// Play a published simulation. `extras`: the API its link names.
    Published { sim_id: String, extras: LinkExtras },
}

impl Launch {
    fn from_args(args: &[String]) -> Result<Self, String> {
        // `--help` and `-h` before anything else, so a bad path on the same
        // command still prints usage, and `-h` is never a scene path.
        if args.iter().skip(1).any(|a| matches!(a.as_str(), "--help" | "-h" | "-?")) {
            return Err(USAGE_REQUEST.into());
        }
        // Each kind of launch keeps the extras of the link that named it.
        let mut link: Option<(JoinLink, LinkExtras)> = None;
        let mut sim: Option<(String, LinkExtras)> = None;
        let (mut key, mut pin, mut name, mut scene, mut space_dir) = (None, None, None, None, None);
        let mut it = args.iter().skip(1);
        while let Some(raw) = it.next() {
            let mut value = |flag: &str| it.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
            // A link gives up its ticket before anything reads it, and only
            // the link without it is parsed below, so no error, usage text or
            // log line can quote the ticket. `--connect` and `https://`
            // arguments carry no ticket the Player reads.
            let without_ticket;
            let (arg, extras): (&str, LinkExtras) = match player_link(raw) {
                Some(rest) => {
                    let (link_text, found) = split_link_extras(&format!("{STUDIO_SCHEME}{rest}"))?;
                    without_ticket = link_text;
                    (without_ticket.as_str(), found)
                }
                None => (raw.as_str(), LinkExtras::default()),
            };
            match arg {
                "--connect" => link = Some((parse_join_link(&value("--connect")?)?, LinkExtras::default())),
                "--key" => key = Some(value("--key")?),
                "--pin" => pin = Some(value("--pin")?),
                "--name" => name = Some(value("--name")?),
                "--sim" => sim = Some((value("--sim")?, LinkExtras::default())),
                // Taken by `main` before anything reads the API.
                "--api-url" => drop(value("--api-url")?),
                // `play/<id>`, or `play/<id>?api=<name>`: the id is the path
                // alone.
                other if other.starts_with("eustress://play/") => {
                    let id = other["eustress://play/".len()..].split('?').next().unwrap_or("");
                    sim = Some((id.trim_end_matches('/').to_string(), extras));
                }
                other if other.starts_with(STUDIO_SCHEME) || other.starts_with("https://") => {
                    link = Some((parse_join_link(other)?, extras));
                }
                other if other.starts_with('-') => return Err(format!("unknown option {}", option_name(other))),
                other => {
                    if scene.is_some() || space_dir.is_some() {
                        return Err("only one scene file or Space directory".into());
                    }
                    match local_path(other)? {
                        LocalPath::Scene(path) => scene = Some(path),
                        LocalPath::Space(path) => space_dir = Some(path),
                    }
                }
            }
        }

        if let Some((sim_id, extras)) = sim {
            let valid = !sim_id.is_empty() && sim_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
            if !valid {
                // Not quoted: whatever was pasted after `--sim` stays out of
                // the message.
                return Err("the simulation id must be hex digits and dashes".into());
            }
            return Ok(Launch::Published { sim_id, extras });
        }
        if let Some((mut link, extras)) = link {
            if key.is_some() {
                link.key = key;
            }
            if let Some(p) = pin {
                link.pin = Some(
                    eustress_networking::join_link::parse_hex32(&p).ok_or("--pin must be 64 hex characters")?,
                );
            }
            // A Studio host on this computer leaves its full link behind, so
            // the address alone is enough here.
            if link.key.is_none() && link.pin.is_none() && link.is_loopback() {
                let local = systems::space_world::workspace_root()
                    .and_then(|w| eustress_networking::native::read_host_file(&w, link.port));
                if let Some(local) = local {
                    link.key = local.key;
                    link.pin = local.pin;
                }
            }
            let name = name
                .or_else(|| std::env::var("EUSTRESS_PLAYER_NAME").ok())
                .or_else(|| std::env::var("USERNAME").ok())
                .or_else(|| std::env::var("USER").ok())
                .unwrap_or_else(|| "Player".to_string());
            return Ok(Launch::Join { link, name, extras });
        }
        Ok(Launch::Local { scene_path: scene, space_dir })
    }

    fn is_network(&self) -> bool {
        !matches!(self, Launch::Local { .. })
    }

    /// A link names the API it belongs to, and this Player opens it only on
    /// `player_api`, the API it is set up for: its `--api-url`, else
    /// production. A link that names none takes the Player's own.
    fn check_api(&self, player_api: &str) -> Result<(), String> {
        match self {
            Launch::Join { extras, .. } | Launch::Published { extras, .. } => extras.check_api(player_api),
            Launch::Local { .. } => Ok(()),
        }
    }
}

/// Command line arguments
#[derive(Resource, Default)]
struct ClientArgs {
    scene_path: Option<PathBuf>,
}

/// The published simulation to play, when launched with `--sim`.
#[derive(Resource)]
struct PublishedTarget(String);

/// Player logs kept, newest first; older ones are deleted at launch.
const PLAYER_LOG_RETAIN: usize = 8;

/// The folder the Player writes its logs to: `~/.eustress_engine/logs`, or a
/// `logs` folder beside the exe when the home folder is unknown.
fn player_log_dir() -> Option<PathBuf> {
    dirs::home_dir()
        .map(|home| home.join(".eustress_engine").join("logs"))
        .or_else(|| std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join("logs"))))
}

/// This run's log file, the one `player_log_fmt_layer` writes. The panic hook
/// appends its PANIC lines to the same file.
fn player_log_path() -> Option<PathBuf> {
    player_log_dir().map(|dir| dir.join(format!("player-{}.log", std::process::id())))
}

/// A message box in plain words: a panic, or a link the Player cannot open. A
/// release Player has no console, so this is the only place it can be read.
fn show_message(title: &str, body: &str) {
    let _ = rfd::MessageDialog::new()
        .set_title(title)
        .set_description(body)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

/// `LogPlugin::fmt_layer`: Bevy's usual stderr layer plus one over
/// `~/.eustress_engine/logs/player-<pid>.log`, because a release Player has
/// no console (see `windows_subsystem` above). Studio does the same with
/// `engine-<pid>.log`. `None` on any I/O failure hands `LogPlugin` back its
/// own layer, so an unwritable home folder costs the file and nothing else.
fn player_log_fmt_layer(_app: &mut App) -> Option<bevy::log::BoxedFmtLayer> {
    use bevy::log::tracing_subscriber::fmt;

    let dir = player_log_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    // One file per run, so two Players never truncate each other's; a log
    // another Player still holds open cannot be deleted on Windows, so
    // pruning never takes a live one.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut logs: Vec<(std::time::SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with("player-") && name.ends_with(".log")
            })
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        logs.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, stale) in logs.into_iter().skip(PLAYER_LOG_RETAIN) {
            let _ = std::fs::remove_file(stale);
        }
    }
    let file = std::fs::File::create(dir.join(format!("player-{}.log", std::process::id()))).ok()?;
    let console: bevy::log::BoxedFmtLayer = Box::new(fmt::Layer::default().with_writer(std::io::stderr));
    let to_file: bevy::log::BoxedFmtLayer =
        Box::new(fmt::Layer::default().with_ansi(false).with_writer(std::sync::Mutex::new(file)));
    Some(Box::new(vec![console, to_file]))
}

/// Build the Client app without running it.
///
/// Extracted so the golden parity test can construct both shells as *values*
/// and diff their registered systems, resources and component sets. Two
/// binaries that can only be launched cannot be compared; two `App`s can.
pub fn build_app(launch: Launch) -> App {
    let mut app = App::new();
    let network = launch.is_network();
    // A world that arrives over the network (a host's Space, or a published
    // simulation) is written into this process's own folder before opening.
    let live = network.then(systems::live_world::LiveWorld::for_this_process);
    if let Some(live) = &live {
        live.prepare();
    }
    let scene_path = match &launch {
        Launch::Local { scene_path, .. } => scene_path.clone(),
        _ => None,
    };

    // ── Asset sources MUST be registered before AssetPlugin ────────────────
    //
    // Bevy freezes the asset-source table when AssetPlugin builds. The Client
    // never registered `bundled://` at all, so every character GLB and all
    // eight animation clips 404'd and the player was invisible. Three separate
    // audit findings were this one omission.
    //
    // The default source (the engine's assets) and `bundled://` open only
    // inside their folders (play-runtime `ContainedFileReader`): the Player
    // runs with UnapprovedPathMode::Allow, so a raw path a Space or a host
    // carries (`//host/share/x`, `C:/x`, `../x`) would otherwise reach the
    // disk or the network. The one absolute path the Player loads on purpose
    // is its enhancement cache (`systems::asset_applicator`).
    {
        use bevy::asset::io::{AssetSourceBuilder, AssetSourceId, ErasedAssetReader};
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(|| -> Box<dyn ErasedAssetReader> { Box::new(systems::asset_sources::default_reader()) }),
        );
        app.register_asset_source(
            "bundled",
            AssetSourceBuilder::new(|| -> Box<dyn ErasedAssetReader> { Box::new(systems::asset_sources::bundled_reader()) }),
        );
        eustress_common::avatar::boot::note_asset_sources_registered_externally();
    }
    eustress_common::avatar::boot::register_avatar_asset_sources(&mut app);

    // `space://` resolves against the Space `space_world` opens, the same
    // folder its terrain and scatter layer instances are read from. Custom
    // scatter layers (and any Space mesh) load through this source; only the
    // engine shell registered it, so those loads failed here. The Client opens
    // at most one Space, resolved at launch, so a plain file source rooted
    // there is enough (the engine needs a swappable reader only because Studio
    // switches Spaces at runtime).
    // A network launch registers the folder its world will be written to;
    // the world has not arrived yet, but the source must exist now.
    let space_source = match &live {
        Some(live) => Some(live.space_root.clone()),
        None => systems::space_world::resolve_space(),
    };
    if let Some(space_root) = space_source {
        // Opens only what lies inside the Space (play-runtime `space_paths`):
        // the Player runs with UnapprovedPathMode::Allow, so Bevy itself
        // refuses no path.
        app.register_asset_source(
            "space",
            bevy::asset::io::AssetSourceBuilder::new(move || -> Box<dyn bevy::asset::io::ErasedAssetReader> {
                Box::new(systems::asset_sources::space_reader(space_root.clone()))
            }),
        );
    }

    app
        // Core Bevy plugins
        .add_plugins(DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Eustress Client".to_string(),
                    resolution: bevy::window::WindowResolution::new(1920, 1080),
                    present_mode: bevy::window::PresentMode::Fifo, // VSync
                    ..default()
                }),
                ..default()
            })
            // `permissive_asset_plugin` sets UnapprovedPathMode::Allow: Bevy
            // refuses no absolute or escaping path here, so every path a Space
            // names is contained by its builder (play-runtime `space_paths`).
            //
            // The root is `common/assets` beside the exe in an installed
            // Player, else the source tree. A relative root resolves against
            // the exe's directory whenever cargo is not the launcher, which
            // is `<install dir>/../common/assets` for an installed Player and
            // `target/common/assets` for a dev build started directly.
            .set(eustress_common::avatar::boot::permissive_asset_plugin(
                eustress_common::assets_dir().to_string_lossy(),
            ))
            .set(bevy::log::LogPlugin { fmt_layer: player_log_fmt_layer, ..default() })
        )

        // Physics (Avian3D) - Realistic Earth gravity: 9.80665 m/s²
        .add_plugins(PhysicsPlugins::default())
        .insert_resource(Gravity(Vec3::NEG_Y * eustress_common::avatar::GRAVITY_MPS2))
        // Workspace.gravity owns gravity, as in Studio: each Space sets it,
        // and this one system carries it into Avian.
        .init_resource::<eustress_common::services::workspace::Workspace>()
        .add_systems(Update, eustress_common::services::workspace::sync_workspace_gravity_to_avian)
        // Match Studio's fixed timestep. The Client ran at Bevy's 64 Hz default
        // while Studio pinned 60 Hz, so identical inputs integrated differently.
        .insert_resource(Time::<Fixed>::from_hz(60.0))

        // Services (Roblox-style)
        .add_plugins(LightingServicePlugin)  // Skybox, sun, ambient, fog
        .add_plugins(PlayerServicePlugin)    // Player, character, camera
        .add_plugins(ClientTerrainPlugin)    // Terrain rendering with physics
        .add_plugins(PauseMenuPlugin)        // ESC menu with Resume/Reset/Settings/Exit
        .add_plugins(TeamServicePlugin)      // Team system (colors, spawns, etc.)

        // ── The sealed avatar runtime ─────────────────────────────────────
        // Same plugin, same descriptor, same physics body as Studio Play Mode.
        .add_plugins(AvatarRuntimePlugin::new(AvatarHost::Client))

        // Skinned character animation system (GLB models)
        .add_plugins(SkinnedCharacterPlugin)
        .add_plugins(CharacterAnimationPlugin)
        // Animation built from instances: every Animator in the Player's tree
        // plays its tracks on its rig (its frame order is below).
        .add_plugins(eustress_common::animation::AnimatorPlugin)

        // Window / taskbar icon. The .ico in the PE resource table only
        // reaches Explorer; winit's window class ships with no icon at all.
        .add_plugins(systems::window_icon::WindowIconPlugin)
        // Frame-burst capture so an agent can SEE the running client.
        .add_plugins(systems::frame_capture::FrameCapturePlugin)
        // Agent control surface (opt-in via EUSTRESS_AGENT_PORT).
        .add_plugins(systems::agent_control::AgentControlPlugin)
        // Published-content fetch: a simulation's .echk chunks (or an older
        // .pak) from R2.
        .add_plugins(systems::space_fetch::SpaceFetchPlugin)
        // Open a real Eustress Space (Movement/Climbing by default) instead of
        // the two hardcoded demo primitives. A network launch opens its world
        // when it arrives instead.
        .add_plugins(systems::space_world::SpaceWorldPlugin { open_local: !network })
        // Multiplayer: join a Studio host, and open any world that arrives.
        .add_plugins(systems::net_play::NetPlayPlugin)
        // A joined world is a DataModel tree that replication writes into;
        // the Play runtime Studio shares draws it, as a replica: parts,
        // lights, the HUD, billboards, sounds, particles and the camera.
        .add_plugins(eustress_play_runtime::PlayRuntimePlugin {
            role: eustress_common::play_session::PlayRole::Replica,
        })
        // A drawn part's DataMesh child (SpecialMesh, BlockMesh,
        // CylinderMesh) is what it draws; its collider stays its own shape.
        .add_plugins(eustress_common::data_mesh::DataMeshPlugin)
        // That tree's LocalScripts, in a Play VM of their own. Runs before
        // the apply step, so a script's writes draw in the frame it made them.
        .add_plugins(eustress_common::tree_scripts::TreeScriptsPlugin)
        // The local avatar as the local player's Character, before the
        // scripts that listen to it.
        .add_systems(
            Update,
            eustress_common::animation::character::sync_local_character
                .after(eustress_common::avatar::AvatarSystems::Locomotion)
                .before(eustress_common::play_session::PlayScriptSet::Scripts),
        )
        // Buying in a host's session: only from the listing's creator.
        .add_plugins(systems::commerce::PlayerCommercePlugin)

        // Enhancement pipeline
        .add_plugins(EnhancementPlugin)
        
        // Soul scripting — Rune + Luau + GUI bridge
        .add_plugins(soul::ClientSoulPlugin)
        .add_plugins(soul::ClientPhysicsBridgePlugin)

        // Register types needed for glTF scene spawning.
        //
        // Was three types; the spawner needs the full set (Gltf* extras,
        // hierarchy, visibility, skinning, animation) or it panics with
        // "unregistered type" the moment a rigged character glTF loads.
        .add_plugins(|app: &mut App| {
            eustress_common::avatar::boot::register_gltf_scene_types(app)
        })
        
        // Resources
        .insert_resource(ClientArgs { scene_path })

        // World setup - loads default scene (same as Studio)
        // Only when no Space is open: otherwise the demo baseplate and cube
        // spawn inside the authored level. A network launch has neither: its
        // world, and then its avatar, arrive later (systems::net_play).
        .add_systems(
            Startup,
            setup_default_scene.run_if(move || !network && !systems::space_world::space_is_available()),
        )
        // Every frame until the avatar is placed, which waits for the local
        // Space's terrain (see `spawn_local_avatar`).
        .add_systems(Update, spawn_local_avatar.run_if(move || !network))
        .add_systems(Update, handle_escape)
        // A gallery's Play link opens this Player.
        .add_systems(Startup, register_link_handler);

    if let Some(live) = live {
        app.insert_resource(live);
    }
    match launch {
        Launch::Join { link, name, mut extras } => {
            // The identity the host is told: the ticket the gallery's Play
            // page put in the link, else, for a signed-in player, one minted
            // from this Player's sign-in and bound to `link.pin`, so the host
            // knows whose purchases are whose.
            app.insert_resource(systems::net_play::JoinTarget::new(
                link,
                name,
                extras.ticket.take(),
                systems::commerce::join_ticket,
            ));
        }
        Launch::Published { sim_id, .. } => {
            app.insert_resource(PublishedTarget(sim_id));
            app.add_systems(Startup, request_published_world);
        }
        Launch::Local { .. } => {}
    }

    app
}

/// A line for the terminal that started the Player, written so it can never
/// panic. A release Player on Windows is a GUI program with no console of its
/// own: a shell that captures its output and stops reading closes the pipe,
/// and `println!` then panics ("The pipe is being closed", os error 232)
/// before any message box or crash hook exists. So this first borrows the
/// parent's console when there is one, then ignores a failed write.
fn say(to_stderr: bool, text: &str) {
    use std::io::Write;
    attach_parent_console();
    let _ = if to_stderr { writeln!(std::io::stderr(), "{text}") } else { writeln!(std::io::stdout(), "{text}") };
}

/// Windows release builds: attach to the console of the terminal that started
/// the Player, so `--version` or a refused link shows there. It fails quietly
/// when there is none (a browser or Explorer started it), and a handle the
/// shell redirected to a file or pipe is left as it is.
#[cfg(all(windows, not(debug_assertions)))]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    /// `ATTACH_PARENT_PROCESS`, `(DWORD)-1`.
    const PARENT: u32 = u32::MAX;
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: AttachConsole takes a plain value and changes only this
    // process's console attachment; its failure is not an error here.
    ONCE.call_once(|| unsafe {
        AttachConsole(PARENT);
    });
}

/// Elsewhere the Player already has its terminal (or none to find).
#[cfg(not(all(windows, not(debug_assertions))))]
fn attach_parent_console() {}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Not a way to launch, so it never reaches `Launch`. The release pipeline
    // checks this against the tag it builds.
    if args.iter().skip(1).any(|a| a == "--version" || a == "-V") {
        say(false, &format!("eustress-client {}", env!("CARGO_PKG_VERSION")));
        return;
    }
    // `--api-url`: before anything reads the API base, which is read once.
    if let Some(url) = args.iter().position(|a| a == "--api-url").and_then(|i| args.get(i + 1)) {
        std::env::set_var(eustress_common::api_base::API_URL_VAR, url);
    }
    let launch = match Launch::from_args(&args) {
        Ok(launch) => launch,
        Err(e) if e == USAGE_REQUEST => {
            say(false, USAGE);
            return;
        }
        Err(e) => {
            say(true, &format!("eustress-client: {e}\n\n{USAGE}"));
            // A release Player has no console, so the reason would go nowhere:
            // show it in plain words (C3).
            if !cfg!(debug_assertions) {
                show_message(
                    "Eustress Player could not open this link",
                    &format!("{e}\n\nCopy the link again from the page or the host that gave it to you, then open it."),
                );
            }
            std::process::exit(2);
        }
    };
    // A link names the API it belongs to, and this Player opens only its
    // own: a link for the other is refused here, before anything is fetched,
    // minted or connected. A link never sets the API; only `--api-url` above
    // does (docs/networking/PLAYER_JOIN_TICKET.md).
    if let Err(why) = launch.check_api(eustress_common::api_base::api_base()) {
        say(true, &format!("eustress-client: {why}"));
        if !cfg!(debug_assertions) {
            show_message("Eustress Player could not open this link", &why);
        }
        std::process::exit(2);
    }
    if let Launch::Local { space_dir: Some(dir), .. } = &launch {
        // `resolve_space` reads this before the movement course, so the
        // directory on the command line is the one that opens.
        std::env::set_var("EUSTRESS_SPACE", dir);
    }
    // A panic is written to this run's log and shown once, instead of ending
    // the window silently: a release Player has no console to show it.
    eustress_play_runtime::crash_report::install(player_log_path, show_message);
    let mut app = build_app(launch);
    // The API was first read above, before this run's log existed, so the
    // line `api_base` writes for an API other than production is lost; this
    // one takes its place.
    if !eustress_common::api_base::is_production() {
        warn!("Eustress API: {}, NOT production", eustress_common::api_base::api_base());
    }
    app.run();
}

/// Register this exe for `eustress-player://` links, for the current user
/// (`systems::link_handler`). Unchanged values are left alone.
fn register_link_handler() {
    use systems::link_handler;
    if !link_handler::enabled(std::env::var(link_handler::REGISTER_VAR).ok().as_deref()) {
        info!("eustress-player:// links: left as they are ({}=0)", link_handler::REGISTER_VAR);
        return;
    }
    #[cfg(windows)]
    {
        let base = eustress_common::api_base::api_base();
        let api = (base != eustress_common::api_base::PRODUCTION_API).then_some(base);
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                warn!("eustress-player:// links: could not find this Player's path: {e}");
                return;
            }
        };
        match link_handler::register(&exe, api) {
            Ok(0) => debug!("eustress-player:// links already open {}", exe.display()),
            Ok(n) => info!("eustress-player:// links now open {} ({n} values written, this user only)", exe.display()),
            Err(e) => warn!("eustress-player:// links: could not register this Player: {e}"),
        }
    }
}

/// `--sim <id>`: download the published world; it opens on arrival.
fn request_published_world(target: Res<PublishedTarget>, mut fetch: MessageWriter<systems::space_fetch::FetchSpace>) {
    let token = systems::commerce::auth_token();
    fetch.write(systems::space_fetch::FetchSpace { simulation_id: target.0.clone(), token });
}

/// Where the avatar starts when a Space gives no SpawnLocation.
const LOCAL_START: Vec3 = Vec3::new(0.0, 2.0, 8.0);

/// Spawn the player's avatar through the sealed runtime, once the open
/// Space's terrain is in the world ([`systems::space_world::SpaceTerrainLoad`]).
/// The local Space opens at startup, but its imported voxel terrain builds in
/// the background, and a character placed before it lands falls through.
///
/// Its feet go on the Space's SpawnLocation, as Studio's are. Without one they
/// go on whatever lies under [`LOCAL_START`], found once the Space's colliders
/// have had a moment to reach the physics world, which steps on the fixed
/// clock; with nothing there, at [`LOCAL_START`] itself.
///
/// The descriptor is the ONLY input. There is no model path and no sex here —
/// the old code hardcoded `BiologicalSex::Male` on this side and
/// `BiologicalSex::Female` in Studio, which selected different bodies AND
/// different animation clip sets in the two shells.
fn spawn_local_avatar(
    mut placed: Local<bool>,
    mut waited: Local<f32>,
    time: Res<Time>,
    terrain: Res<systems::space_world::SpaceTerrainLoad>,
    open: Res<systems::space_world::OpenSpace>,
    spatial: SpatialQuery,
    mut spawn: MessageWriter<eustress_common::avatar::profile::SpawnSavedAvatar>,
) {
    if *placed || !terrain.ready() {
        return;
    }
    let at = match open.spawn {
        Some(pad) => pad,
        None => {
            *waited += time.delta_secs();
            if *waited < 0.25 {
                return;
            }
            ground_under(&spatial, LOCAL_START).unwrap_or(LOCAL_START)
        }
    };
    *placed = true;
    let token = systems::commerce::auth_token();
    spawn.write(eustress_common::avatar::profile::SpawnSavedAvatar { token, at });
}

/// A little above the top of whatever lies under `point`, for a pair of feet.
fn ground_under(spatial: &SpatialQuery, point: Vec3) -> Option<Vec3> {
    const TOP: f32 = 1000.0;
    let hit = spatial.cast_ray(
        Vec3::new(point.x, TOP, point.z),
        Dir3::NEG_Y,
        2.0 * TOP,
        true,
        &SpatialQueryFilter::default(),
    )?;
    Some(Vec3::new(point.x, TOP - hit.distance + 0.1, point.z))
}

/// The Client's meaning of Escape. `HostSeams` guarantees Studio's differs.
fn handle_escape(mut events: MessageReader<AvatarEscapePressed>) {
    for e in events.read() {
        debug_assert_eq!(e.0, eustress_common::avatar::EscapeAction::PauseMenu);
        // PauseMenuPlugin owns the menu itself; this is the single seam that
        // tells it Escape happened, replacing a third competing Escape binding.
        info!("⏸️ Escape → pause menu");
    }
}

/// Setup default scene - uses the SAME code as Eustress Studio
/// Spawns baseplate and welcome cube from shared eustress_common
fn setup_default_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    args: Res<ClientArgs>,
    mut load_events: MessageWriter<LoadSceneEvent>,
) {
    info!("🌍 Setting up default scene (same as Studio)...");
    
    // =========================================================================
    // SPAWN DEFAULT SCENE - Same code as engine/src/default_scene.rs
    // Uses shared functions from eustress_common::default_scene
    // =========================================================================
    
    // Spawn baseplate (512x1x512 dark gray) and add physics collider
    let baseplate = spawn_baseplate(&mut commands, &mut meshes, &mut materials);
    commands.entity(baseplate).insert((
        RigidBody::Static,
        // FULL extents. `Collider::cuboid(x, y, z)` halves its arguments
        // internally (avian3d-0.7.0 .../parry/mod.rs:747), so passing
        // half-extents built a 256x0.5x256 box: its top sat at +0.25 while the
        // visible baseplate top is at +0.5, sinking every character a quarter
        // metre into the floor — and leaving three quarters of the plate with
        // no collision at all.
        Collider::cuboid(512.0, 1.0, 512.0),
    ));
    
    // Spawn welcome cube (2x2x2 green) and add physics collider
    // Same full-extent convention as the baseplate above.
    let cube = spawn_welcome_cube(&mut commands, &mut meshes, &mut materials);
    commands.entity(cube).insert((
        RigidBody::Static,
        Collider::cuboid(2.0, 2.0, 2.0),  // FULL extents of the 2x2x2 cube
    ));
    
    // =========================================================================
    // SCENE LOADING - Load custom scene if provided via command line
    // =========================================================================
    
    if let Some(path) = &args.scene_path {
        info!("📂 Loading custom scene: {:?}", path);
        load_events.write(LoadSceneEvent { path: path.clone() });
    }
    
    info!("✅ Default scene ready (Baseplate + Welcome Cube)!");
}

#[cfg(test)]
mod launch_link_tests {
    use super::*;

    const SIM: &str = "07d084d8-9098-4192-bb48-84d2c428db36";
    /// A ticket's shape (`A-Z a-z 0-9 . _ -`). No real ticket is ever written
    /// into a test.
    const TICKET: &str = "eit1.c2VjcmV0LXRpY2tldA.c2ln-_x";
    const PRODUCTION: &str = "https://api.eustress.dev";
    const STAGING: &str = "https://api-staging.eustress.dev";

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// The launch for one argument, as Windows passes a clicked link.
    fn from_link(arg: &str) -> Result<Launch, String> {
        Launch::from_args(&args(&["eustress-client", arg]))
    }

    /// A gallery's join link as the Play page writes it: the Worker's link,
    /// with its key and pin, then `extra` (`&ticket=..&api=..`).
    fn gallery_join(extra: &str) -> String {
        format!("eustress-player://join/203.0.113.7:7777?key=abcdef&pin={}{extra}", "ab".repeat(32))
    }

    /// The gallery's Play button hands the Player `eustress-player://play/<id>`
    /// through the registered handler; that opens the published simulation.
    #[test]
    fn a_gallery_play_link_opens_the_published_simulation() {
        let launch = Launch::from_args(&args(&["eustress-client", &format!("eustress-player://play/{SIM}")]))
            .expect("the gallery's link parses");
        match launch {
            Launch::Published { sim_id, .. } => assert_eq!(sim_id, SIM),
            other => panic!("expected a published simulation, got {other:?}"),
        }
    }

    /// A link with a trailing slash opens the same simulation.
    #[test]
    fn a_trailing_slash_is_the_same_link() {
        let launch = Launch::from_args(&args(&["eustress-client", &format!("eustress-player://play/{SIM}/")]))
            .expect("the link parses");
        assert!(matches!(launch, Launch::Published { sim_id, .. } if sim_id == SIM));
    }

    /// The command the link handler registers passes the API before the link,
    /// and the link still opens the simulation.
    #[test]
    fn the_registered_command_keeps_the_api_and_opens_the_link() {
        let launch = Launch::from_args(&args(&[
            "eustress-client",
            "--api-url",
            "https://api-staging.eustress.dev",
            &format!("eustress-player://play/{SIM}"),
        ]))
        .expect("the registered command parses");
        assert!(matches!(launch, Launch::Published { sim_id, .. } if sim_id == SIM));
    }

    /// Something that is not a simulation id is refused, not launched.
    #[test]
    fn a_link_that_is_not_a_simulation_is_refused() {
        assert!(Launch::from_args(&args(&["eustress-client", "eustress-player://play/not-an-id"])).is_err());
        assert!(Launch::from_args(&args(&["eustress-client", "eustress-player://play/"])).is_err());
    }

    /// A gallery's join link: its ticket and API belong to the launch, apart
    /// from the join link, which never holds the ticket.
    #[test]
    fn a_gallery_join_link_carries_its_ticket_and_api() {
        let launch = from_link(&gallery_join(&format!("&ticket={TICKET}&api=staging"))).expect("the gallery's join link parses");
        let Launch::Join { link, extras, .. } = &launch else { panic!("expected a join, got {launch:?}") };
        assert!(extras.ticket.is_some(), "the link's ticket is the launch's");
        assert_eq!(extras.api, Some(STAGING));
        assert_eq!(
            (link.host.as_str(), link.port, link.key.as_deref(), link.pin),
            ("203.0.113.7", 7777, Some("abcdef"), Some([0xab; 32]))
        );
        assert!(!link.to_link().contains(TICKET), "the join link held the ticket");
    }

    /// A launch's `Debug` shows that it holds a ticket, never the ticket.
    #[test]
    fn a_launch_never_prints_its_ticket() {
        let launch = from_link(&gallery_join(&format!("&ticket={TICKET}&api=staging"))).expect("parses");
        let shown = format!("{launch:?}");
        assert!(!shown.contains(TICKET), "the launch's Debug printed its ticket");
        assert!(shown.contains("JoinTicket(..)"), "the launch holds no ticket");
    }

    /// A link for an API this Player does not know is refused, in words that
    /// do not quote its ticket.
    #[test]
    fn a_link_for_an_unknown_api_is_refused() {
        let e = from_link(&gallery_join(&format!("&ticket={TICKET}&api=nightly"))).expect_err("an unknown API refuses the link");
        assert!(!e.contains(TICKET), "the refusal quoted the ticket");
        assert!(from_link(&format!("eustress-player://play/{SIM}?api=nightly")).is_err());
    }

    /// `--connect` takes a link as typed, and reads no ticket or API from it.
    #[test]
    fn a_connect_argument_carries_no_ticket() {
        let typed = gallery_join(&format!("&ticket={TICKET}&api=staging"));
        let launch = Launch::from_args(&args(&["eustress-client", "--connect", &typed])).expect("--connect parses");
        let Launch::Join { link, extras, .. } = &launch else { panic!("expected a join, got {launch:?}") };
        assert_eq!(*extras, LinkExtras::default());
        assert_eq!(link.pin, Some([0xab; 32]));
        assert_eq!(launch.check_api(PRODUCTION), Ok(()), "a --connect link names no API");
    }

    /// A production Player refuses a staging link, and a staging Player takes
    /// it, for a join link and a play link alike.
    #[test]
    fn a_link_for_the_other_server_is_refused() {
        let join = from_link(&gallery_join(&format!("&ticket={TICKET}&api=staging"))).expect("parses");
        let e = join.check_api(PRODUCTION).expect_err("a production Player refuses a staging link");
        assert!(!e.contains(TICKET), "the refusal quoted the ticket");
        assert_eq!(join.check_api(STAGING), Ok(()));
        let solo = from_link(&format!("eustress-player://play/{SIM}?api=staging")).expect("parses");
        assert!(solo.check_api(PRODUCTION).is_err(), "a play link is checked too");
        assert_eq!(solo.check_api(STAGING), Ok(()));
        let production = from_link(&format!("eustress-player://play/{SIM}?api=production")).expect("parses");
        assert!(production.check_api(STAGING).is_err());
        assert_eq!(production.check_api(PRODUCTION), Ok(()));
    }

    /// A link that cannot be opened is refused in words that never quote its
    /// ticket: the ticket comes out before the link is read, and no launch
    /// error quotes a whole argument.
    #[test]
    fn a_malformed_link_never_quotes_its_ticket() {
        let pin = "ab".repeat(32);
        for bad in [
            // A pin that is not one.
            format!("eustress-player://join/203.0.113.7:7777?key=abcdef&pin=zz&ticket={TICKET}"),
            format!("eustress-player://join/203.0.113.7:99999?key=abcdef&pin={pin}&ticket={TICKET}"),
            format!("eustress-player://join/203.0.113.7:7777?key=a&pin={pin}&ticket={TICKET}"),
            format!("eustress-player://join/203.0.113.7:7777?pin={pin}&ticket={TICKET}&ticket={TICKET}"),
            format!("eustress-player://play/not-an-id?ticket={TICKET}"),
            // A ticket where no ticket goes: no `?`, so it stays in the address.
            format!("eustress-player://join/203.0.113.7:7777&ticket={TICKET}"),
            // A pin and a key where no `?` goes sit in the address the same way.
            format!("eustress-player://join/203.0.113.7:7777&pin={pin}"),
            format!("eustress-player://join/203.0.113.7:7777&key=abcdef"),
        ] {
            let e = from_link(&bad).expect_err("a malformed link is refused");
            assert!([TICKET, pin.as_str(), "abcdef"].iter().all(|s| !e.contains(*s)), "a launch error quoted a secret: {e}");
        }
        // Not links, yet they carry one: a link typed after `--connect` or
        // `--sim`, and an option written with `=`.
        for argv in [
            args(&["eustress-client", "--connect", &format!("eustress-player://join/203.0.113.7:7777&ticket={TICKET}")]),
            args(&["eustress-client", "--sim", &format!("eustress-player://play/{SIM}?ticket={TICKET}")]),
            args(&["eustress-client", &format!("--connect={}", gallery_join(&format!("&ticket={TICKET}")))]),
        ] {
            let e = Launch::from_args(&argv).expect_err("refused");
            assert!(!e.contains(TICKET), "a launch error quoted the ticket");
        }
    }

    /// A play link may end in `?api=<name>`; the id is read without it.
    #[test]
    fn a_play_link_names_its_api_after_the_id() {
        let launch = from_link(&format!("eustress-player://play/{SIM}?api=staging")).expect("parses");
        let Launch::Published { sim_id, extras } = &launch else { panic!("expected a published simulation, got {launch:?}") };
        assert_eq!(sim_id, SIM);
        assert_eq!(extras.api, Some(STAGING));
        let slash = from_link(&format!("eustress-player://play/{SIM}/?api=production")).expect("parses");
        assert!(matches!(&slash, Launch::Published { sim_id, .. } if sim_id == SIM));
    }

    /// A join link with neither: no ticket, and a Player on any API takes it.
    #[test]
    fn a_plain_join_link_has_no_extras() {
        let launch = from_link(&gallery_join("")).expect("parses");
        let Launch::Join { extras, .. } = &launch else { panic!("expected a join, got {launch:?}") };
        assert_eq!(*extras, LinkExtras::default());
        assert_eq!(launch.check_api(PRODUCTION), Ok(()));
        assert_eq!(launch.check_api(STAGING), Ok(()));
    }

    /// An `https://` link carries no ticket or API the Player reads.
    #[test]
    fn an_https_link_carries_no_ticket() {
        let launch = from_link(&format!("https://203.0.113.7:7777/join/abcdef?ticket={TICKET}&api=staging")).expect("parses");
        assert!(matches!(&launch, Launch::Join { extras, .. } if *extras == LinkExtras::default()));
    }

    /// A link with its scheme in capitals, or with spaces around it, is still
    /// a link: its ticket comes out, and it never becomes a scene file's path.
    #[test]
    fn a_link_in_capitals_is_still_a_link() {
        for written in ["EUSTRESS-PLAYER://", "Eustress://"] {
            let link = gallery_join(&format!("&ticket={TICKET}")).replacen("eustress-player://", written, 1);
            let launch = from_link(&format!(" {link} ")).expect("parses");
            assert!(matches!(&launch, Launch::Join { extras, .. } if extras.ticket.is_some()), "{written} was not read as a link");
        }
    }

    /// `--help` and `-h` ask for usage. They are not a scene path, so they
    /// cannot fall through to the movement course. `main` exits 0 for this.
    #[test]
    fn help_asks_for_usage_instead_of_opening_a_space() {
        for flag in ["--help", "-h", "-?"] {
            let e = Launch::from_args(&args(&["eustress-client", flag])).expect_err(flag);
            assert_eq!(e, USAGE_REQUEST, "{flag} must be usage, not a path");
        }
        let with_junk = Launch::from_args(&args(&["eustress-client", "no-such-space", "--help"])).expect_err("help wins");
        assert_eq!(with_junk, USAGE_REQUEST);
    }

    /// A positional argument that is not a scene file or a Space directory
    /// is refused. It must not become a launch that opens the movement course.
    #[test]
    fn a_bad_positional_argument_is_refused() {
        let missing = Launch::from_args(&args(&["eustress-client", "no-such-space"])).expect_err("missing");
        assert!(missing.contains("no such"), "{missing}");
        assert_ne!(missing, USAGE_REQUEST);

        let dir = std::env::temp_dir().join(format!("eustress-not-a-space-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let not_space = Launch::from_args(&args(&["eustress-client", dir.to_str().unwrap()])).expect_err("not a space");
        assert!(not_space.contains("not a scene file"), "{not_space}");
        let _ = std::fs::remove_dir_all(&dir);

        let file = std::env::temp_dir().join(format!("eustress-notes-{}.txt", std::process::id()));
        std::fs::write(&file, "hello").unwrap();
        let not_scene = Launch::from_args(&args(&["eustress-client", file.to_str().unwrap()])).expect_err("not a scene");
        assert!(not_scene.contains("not a scene file"), "{not_scene}");
        let _ = std::fs::remove_file(&file);
    }

    /// A directory with `Workspace/` is that Space, not a scene file.
    #[test]
    fn a_space_directory_is_the_space_that_opens() {
        let root = std::env::temp_dir().join(format!("eustress-space-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Workspace")).unwrap();
        let launch = Launch::from_args(&args(&["eustress-client", root.to_str().unwrap()])).expect("a space");
        match launch {
            Launch::Local { scene_path, space_dir } => {
                assert!(scene_path.is_none());
                assert_eq!(space_dir.as_deref(), Some(root.as_path()));
            }
            other => panic!("expected a local space, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// No positional argument still opens whatever Space the environment
    /// names, which is the movement course when that variable is unset.
    #[test]
    fn no_positional_argument_stays_the_default_space() {
        let launch = Launch::from_args(&args(&["eustress-client"])).expect("default");
        assert!(matches!(launch, Launch::Local { scene_path: None, space_dir: None }));
    }
}

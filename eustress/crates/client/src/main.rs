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
  eustress-client --version                print the version and exit
links written with eustress:// in place of eustress-player:// open the same way";

/// The Player's own URL scheme. On Windows one application owns a scheme, and
/// `eustress://` belongs to Studio, so a Player installer registers this one;
/// both spell the same links.
const PLAYER_SCHEME: &str = "eustress-player://";

/// How the Player was launched.
#[derive(Debug, Clone)]
pub enum Launch {
    /// A Space from this computer, or a legacy scene file.
    Local { scene_path: Option<PathBuf> },
    /// Join a Studio host.
    Join { link: JoinLink, name: String },
    /// Play a published simulation.
    Published { sim_id: String },
}

impl Launch {
    fn from_args(args: &[String]) -> Result<Self, String> {
        let mut link: Option<JoinLink> = None;
        let (mut key, mut pin, mut name, mut sim, mut scene) = (None, None, None, None, None);
        let mut it = args.iter().skip(1);
        while let Some(arg) = it.next() {
            let mut value = |flag: &str| it.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
            let normalized;
            let arg: &str = match arg.strip_prefix(PLAYER_SCHEME) {
                Some(rest) => {
                    normalized = format!("eustress://{rest}");
                    &normalized
                }
                None => arg.as_str(),
            };
            match arg {
                "--connect" => link = Some(JoinLink::parse(&value("--connect")?)?),
                "--key" => key = Some(value("--key")?),
                "--pin" => pin = Some(value("--pin")?),
                "--name" => name = Some(value("--name")?),
                "--sim" => sim = Some(value("--sim")?),
                other if other.starts_with("eustress://play/") => {
                    sim = Some(other["eustress://play/".len()..].trim_end_matches('/').to_string());
                }
                other if other.starts_with("eustress://") || other.starts_with("https://") => {
                    link = Some(JoinLink::parse(other)?);
                }
                other if other.starts_with("--") => return Err(format!("unknown option {other}")),
                other => scene = Some(PathBuf::from(other)),
            }
        }

        if let Some(sim_id) = sim {
            let valid = !sim_id.is_empty() && sim_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
            if !valid {
                return Err(format!("{sim_id:?} is not a simulation id"));
            }
            return Ok(Launch::Published { sim_id });
        }
        if let Some(mut link) = link {
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
            return Ok(Launch::Join { link, name });
        }
        Ok(Launch::Local { scene_path: scene })
    }

    fn is_network(&self) -> bool {
        !matches!(self, Launch::Local { .. })
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

/// `LogPlugin::fmt_layer`: Bevy's usual stderr layer plus one over
/// `~/.eustress_engine/logs/player-<pid>.log`, because a release Player has
/// no console (see `windows_subsystem` above). Studio does the same with
/// `engine-<pid>.log`. `None` on any I/O failure hands `LogPlugin` back its
/// own layer, so an unwritable home folder costs the file and nothing else.
fn player_log_fmt_layer(_app: &mut App) -> Option<bevy::log::BoxedFmtLayer> {
    use bevy::log::tracing_subscriber::fmt;

    let dir = dirs::home_dir()
        .map(|home| home.join(".eustress_engine").join("logs"))
        .or_else(|| std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join("logs"))))?;
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
        Launch::Local { scene_path } => scene_path.clone(),
        _ => None,
    };

    // ── Asset sources MUST be registered before AssetPlugin ────────────────
    //
    // Bevy freezes the asset-source table when AssetPlugin builds. The Client
    // never registered `bundled://` at all, so every character GLB and all
    // eight animation clips 404'd and the player was invisible. Three separate
    // audit findings were this one omission.
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
        app.register_asset_source(
            "space",
            bevy::asset::io::AssetSourceBuilder::platform_default(&space_root.to_string_lossy(), None),
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
            // Absolute paths (imported meshes, user content) are refused under
            // Bevy 0.19's default UnapprovedPathMode::Forbid.
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
        .add_systems(Update, handle_escape);

    if let Some(live) = live {
        app.insert_resource(live);
    }
    match launch {
        Launch::Join { link, name } => {
            // A signed-in player joins as its account (a ticket bound to
            // `link.pin`), so the host knows whose purchases are whose.
            let identity = systems::commerce::join_ticket(&link);
            app.insert_resource(systems::net_play::JoinTarget { link, name, identity });
        }
        Launch::Published { sim_id } => {
            app.insert_resource(PublishedTarget(sim_id));
            app.add_systems(Startup, request_published_world);
        }
        Launch::Local { .. } => {}
    }

    app
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Not a way to launch, so it never reaches `Launch`. The release pipeline
    // checks this against the tag it builds.
    if args.iter().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("eustress-client {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let launch = match Launch::from_args(&args) {
        Ok(launch) => launch,
        Err(e) => {
            eprintln!("eustress-client: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    build_app(launch).run();
}

/// `--sim <id>`: download the published world; it opens on arrival.
fn request_published_world(target: Res<PublishedTarget>, mut fetch: MessageWriter<systems::space_fetch::FetchSpace>) {
    let token = dirs::data_local_dir()
        .and_then(|dir| std::fs::read_to_string(dir.join("EustressEngine/auth_token")).ok())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
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
    let token = dirs::data_local_dir()
        .and_then(|dir| std::fs::read_to_string(dir.join("EustressEngine/auth_token")).ok());
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

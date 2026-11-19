//! The browser Player's Bevy app: the desktop Player's play path (client
//! `main.rs` `build_app`) for a world that arrives in memory. The shared Play
//! runtime draws the tree. The avatar runtime spawns the local character and
//! follows it.
//!
//! Scripts run as they do in the desktop Player. Played alone, this Player is
//! the session's Authority: its Luau VM (common `tree_scripts`, mlua's Luau
//! compiled against wasi-sysroot) runs the Space's Scripts and LocalScripts,
//! physics is live, and Rune runs every SoulScript. Joined to a host, it is a
//! Replica: the host runs the Scripts, this Player runs its LocalScripts and
//! its own Rune.
//!
//! Left out, because a page has no use for them: files on disk, the window
//! icon, frame capture, agent control, enhancement, Rune, and the shop.

use std::cell::Cell;
use std::rc::Rc;

use avian3d::prelude::{Gravity, PhysicsPlugins};
use bevy::asset::io::memory::{Dir, MemoryAssetReader};
use bevy::asset::io::{AssetSourceBuilder, ErasedAssetReader};
use bevy::asset::AssetMetaCheck;
use bevy::prelude::*;

use crate::join::JoinPlan;
use crate::page::{stop_when_asked, PageState, StopSwitch};

/// What `start_player` hands the app.
pub(crate) struct Start {
    /// The canvas's CSS selector.
    pub canvas: String,
    /// Where the Player's own assets are served: the default source and
    /// `bundled://` (the library a Space can name).
    pub asset_base: String,
    /// The API a published world is downloaded from. Ignored when `plan` is
    /// set: a join plays the host's world.
    pub api_base: String,
    /// The hosted session, when this page is joining one.
    pub plan: Option<JoinPlan>,
    /// The Gallery id of a published world played alone.
    pub sim_id: Option<String>,
    /// The name this player joins as.
    pub name: String,
    pub on_state: js_sys::Function,
    pub stop: Rc<Cell<bool>>,
}

pub(crate) fn build(start: Start) -> App {
    let Start {
        canvas,
        asset_base,
        api_base,
        plan,
        sim_id,
        name,
        on_state,
        stop,
    } = start;
    let mut app = App::new();
    // Joined to a host, this Player draws the host's session (a replica);
    // played alone, nobody else will run it, so it runs it itself, as Studio's
    // Play and the desktop Player do: server scripts, live physics, `Touched`.
    let joined = plan.is_some();
    let play_role = if joined {
        eustress_common::play_session::PlayRole::Replica
    } else {
        eustress_common::play_session::PlayRole::Authority
    };

    // Sources first: AssetPlugin freezes the table when it builds.
    // `space://` is the opened world's own files, in memory, filled when the
    // world arrives (crate::world).
    let space = Dir::default();
    {
        let space = space.clone();
        app.register_asset_source(
            "space",
            AssetSourceBuilder::new(move || -> Box<dyn ErasedAssetReader> {
                Box::new(MemoryAssetReader {
                    root: space.clone(),
                })
            }),
        );
    }
    app.register_asset_source(
        "bundled",
        AssetSourceBuilder::platform_default(&asset_base, None),
    );
    // The avatar runtime refuses to boot until `bundled://` is registered.
    // This page serves that source over HTTP, so the disk registrar must not
    // run and register it a second time.
    eustress_common::avatar::boot::note_asset_sources_registered_externally();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Eustress Player".to_string(),
                    canvas: Some(canvas),
                    fit_canvas_to_parent: true,
                    // The arrow keys, the space bar and Tab play the world
                    // instead of scrolling or leaving the page.
                    prevent_default_event_handling: true,
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_base,
                // The site serves no `.meta` files; asking for one per asset
                // would cost a failed request each.
                meta_check: AssetMetaCheck::Never,
                ..default()
            }),
    )
    // Physics as the desktop Player has it: Workspace.gravity owns gravity,
    // and the fixed step is Studio's 60 Hz.
    .add_plugins(PhysicsPlugins::default())
    .insert_resource(Gravity(Vec3::NEG_Y * eustress_common::avatar::GRAVITY_MPS2))
    .init_resource::<eustress_common::services::workspace::Workspace>()
    .add_systems(
        Update,
        eustress_common::services::workspace::sync_workspace_gravity_to_avian,
    )
    .insert_resource(Time::<Fixed>::from_hz(60.0))
    // The sky, the sun and moon, ambient light and fog, and the light
    // classes' Bevy lights, as the desktop Player's LightingServicePlugin
    // adds them (crate::world fills them from the Space's records).
    .add_plugins(eustress_common::plugins::lighting_plugin::SharedLightingPlugin)
    // The session, and the tree replication writes into.
    .add_plugins((
        eustress_networking::session::NetPlugin,
        eustress_networking::player_replica::NetReplicaPlugin,
    ))
    .insert_resource(eustress_networking::session::AppVersion(
        concat!("eustress-player-web ", env!("CARGO_PKG_VERSION")).to_string(),
    ))
    // The Play runtime Studio shares draws that tree: parts, lights, the HUD,
    // billboards, sounds, particles and the camera. Joined, replication writes
    // into it; played alone, this Player simulates it.
    .add_plugins(eustress_play_runtime::PlayRuntimePlugin { role: play_role })
    .add_plugins(eustress_common::data_mesh::DataMeshPlugin)
    // The sealed avatar runtime: the same plugin, descriptor and physics body
    // as the desktop Player. It spawns the follow camera; the Space's
    // scripted camera stands down while that camera exists.
    .add_plugins(eustress_common::avatar::AvatarRuntimePlugin::new(
        eustress_common::avatar::AvatarHost::Client,
    ))
    .add_plugins(eustress_common::plugins::SkinnedCharacterPlugin)
    .add_plugins(eustress_common::plugins::SharedAnimationPlugin)
    .add_plugins(eustress_common::animation::AnimatorPlugin)
    // The tree's scripts, in the Play VM the desktop Player runs (common
    // `tree_scripts`): LocalScripts always, and the Scripts too when this
    // Player is the Authority. Before the apply step, so a script's writes
    // draw in the frame it made them.
    .add_plugins(eustress_common::tree_scripts::TreeScriptsPlugin)
    // The tree's Rune SoulScripts, after the Luau ones, as Studio runs them:
    // all of them played alone, only this player's own when joined.
    .add_plugins(eustress_common::soul::rune_play::TreeRunePlugin)
    .insert_resource(if joined {
        eustress_common::soul::rune_play::RuneScope::ClientOnly
    } else {
        eustress_common::soul::rune_play::RuneScope::Everything
    })
    .add_systems(Startup, register_player_rune_modules)
    .add_systems(
        Update,
        eustress_common::animation::character::sync_local_character
            .after(eustress_common::avatar::AvatarSystems::Locomotion)
            .before(eustress_common::play_session::PlayScriptSet::Scripts),
    )
    .add_plugins(|app: &mut App| {
        eustress_common::avatar::boot::register_gltf_scene_types(app);
    })
    // This page's own.
    .insert_non_send_resource(PageState::new(on_state))
    .insert_non_send_resource(StopSwitch(stop))
    // A Space with no character shows its own camera, moved only by its
    // scripts, as on the desktop Player: this page adds no free-look camera.
    .add_systems(Last, stop_when_asked);
    if let Some(plan) = plan {
        crate::link::add(&mut app, plan, name.clone());
    } else if let Some(sim_id) = sim_id {
        crate::fetch::add(&mut app, api_base, sim_id);
    }
    crate::world::add(&mut app, space, name, joined);
    // Esc in the browser frees the mouse without the page seeing the key.
    crate::mouse::add(&mut app);
    // WebGL2 panics where `texStorage2DMultisample` is missing, and this
    // engine's WebGPU cameras are MSAA-off. Turn it off as each camera arrives,
    // before the first frame prepares its targets.
    app.add_observer(msaa_off);
    app
}

/// The Rune modules a Player's scripts compile against, the shared Player set
/// the desktop Player registers (`eustress::dm`, `eustress::Vector3`, the GUI
/// and log functions, `event_bus`).
fn register_player_rune_modules(
    mut registry: ResMut<eustress_common::soul::rune_runtime::RuneModuleRegistry>,
) {
    let modules = eustress_common::soul::rune_play::player_rune_modules();
    let count = modules.len();
    for module in modules {
        registry.add_module(module);
    }
    info!("Registered {count} Rune module(s) for the Player");
}

fn msaa_off(
    add: On<bevy::ecs::lifecycle::Add, bevy::render::view::Msaa>,
    mut cameras: Query<&mut bevy::render::view::Msaa>,
) {
    let Ok(mut msaa) = cameras.get_mut(add.event().entity) else {
        return;
    };
    if *msaa != bevy::render::view::Msaa::Off {
        *msaa = bevy::render::view::Msaa::Off;
    }
}

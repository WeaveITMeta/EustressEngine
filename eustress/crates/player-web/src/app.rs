//! The browser Player's Bevy app: what the desktop Player runs for a joined
//! world (client `main.rs` `build_app`), less what a browser cannot run or
//! has no use for: files on disk, Luau (mlua builds C++), the avatar runtime,
//! the window icon, frame capture, agent control, enhancement, Rune, and the
//! shop, which waits for a browser purchase flow.

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
    pub plan: JoinPlan,
    /// The name this player joins as.
    pub name: String,
    pub on_state: js_sys::Function,
    pub stop: Rc<Cell<bool>>,
}

pub(crate) fn build(start: Start) -> App {
    let Start {
        canvas,
        asset_base,
        plan,
        name,
        on_state,
        stop,
    } = start;
    let mut app = App::new();

    // Sources first: AssetPlugin freezes the table when it builds.
    // `space://` is the opened world's own files, in memory, filled when the
    // world arrives (crate::world).
    let space = Dir::default();
    {
        let space = space.clone();
        app.register_asset_source(
            "space",
            AssetSourceBuilder::new(move || -> Box<dyn ErasedAssetReader> {
                Box::new(MemoryAssetReader { root: space.clone() })
            }),
        );
    }
    app.register_asset_source("bundled", AssetSourceBuilder::platform_default(&asset_base, None));

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
    // The Play runtime Studio shares draws that tree as a replica: parts,
    // lights, the HUD, billboards, sounds, particles and the camera.
    .add_plugins(eustress_play_runtime::PlayRuntimePlugin {
        role: eustress_common::play_session::PlayRole::Replica,
    })
    .add_plugins(eustress_common::data_mesh::DataMeshPlugin)
    // This page's own.
    .insert_non_send_resource(PageState::new(on_state))
    .insert_non_send_resource(StopSwitch(stop))
    .add_systems(Last, stop_when_asked);
    crate::link::add(&mut app, plan, name.clone());
    crate::world::add(&mut app, space, name);
    app
}

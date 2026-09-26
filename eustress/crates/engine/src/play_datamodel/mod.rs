//! # Play DataModel: scripts drive the running scene
//!
//! Binds the live [`DataModel`](eustress_common::datamodel::DataModel) to
//! the ECS for the length of a Play session and runs Luau against it.
//!
//! ```text
//! OnEnter(Playing)  seed::seed_session   ECS -> tree (services, parts, GUI,
//!                                        scripts, the Terrain), hide
//!                                        storage services
//! Update, Playing   remote_players::sync_remote_players
//!                                        a Player per joined player
//!                   pull::*              poses, input, mouse ray + hit,
//!                                        camera, collisions, GUI clicks,
//!                                        the local character
//!                   luau::drive_luau     scripts run against the tree,
//!                                        reading physics and terrain live
//!                   apply::apply_frame   tree -> ECS: spawns, reparents,
//!                                        property writes, destroys,
//!                                        impulses, terrain edits; then
//!                                        sounds, particles, NPC humanoids,
//!                                        the scripted camera
//!                   commerce::drive_commerce
//!                                        MarketplaceService purchases
//!                                        through the Commerce API: test
//!                                        mode for Studio's own player, live
//!                                        for players who joined a host
//!                   end_frame            trim events, free destroyed slots
//! OnEnter(Editing)  stop_session         stop threads, despawn what scripts
//!                                        spawned, restore hidden storage
//! ```
//!
//! Every script-visible effect goes through the tree, so the same frame
//! protocol serves Rune (which reads the tree through
//! [`eustress_common::datamodel::active`]).

pub mod apply;
pub mod audio;
pub mod camera;
pub mod commerce;
pub mod npc;
pub mod particles;
pub mod pull;
pub mod remote_players;
pub mod seats;
pub mod seed;
pub mod terrain_edits;
pub mod vehicle_seat_input;

use bevy::prelude::*;

use eustress_common::datamodel::OutputLevel;
use eustress_common::luau::play::{TerrainReadFn, TerrainView};
use eustress_common::terrain::{
    surface_data, TerrainBaked, TerrainChunkCollider, TerrainConfig, TerrainData, TerrainMaterialSlots, TerrainRoot,
    TerrainVolume, TerrainVoxelWater,
};

use crate::play_mode::PlayModeState;

// The session's tree, the frame's sets and the spawn marker are shared with
// the Player (`eustress_common::play_session`).
pub use eustress_common::play_session::{DataModelSpawned, PlayDataModel, PlayScriptSet};

/// The Luau VM for the running session, plus what the first frame still
/// has to do, in Roblox's order: start the server scripts, let the player
/// join (`PlayerAdded`), start the client scripts. The character follows
/// on the next frame (`CharacterAdded`).
#[derive(Resource)]
pub struct PlayLuauHost {
    pub vm: eustress_common::luau::play::PlayLuau,
    pub pending_server: Vec<eustress_common::luau::play::ScriptLaunch>,
    pub pending_client: Vec<eustress_common::luau::play::ScriptLaunch>,
    pub joining_player: Option<eustress_common::datamodel::InstanceId>,
}

/// The lighting as Play found it, restored on Stop.
#[derive(Resource, Clone)]
pub struct PlayLightingSnapshot(pub eustress_common::services::lighting::LightingService);

/// A storage-service part hidden for the session (Roblox never renders
/// ServerStorage / ReplicatedStorage). Its visibility and collider state are
/// restored on Stop.
#[derive(Component, Debug, Clone, Copy)]
pub struct HiddenForPlay {
    pub previous: Visibility,
    pub collider_was_disabled: bool,
}

/// Where a Play frame's script time goes, reported beside the phase
/// profile (`EUSTRESS_PROFILE=1`, window `EUSTRESS_PROFILE_FRAMES`): pull
/// (ECS to tree), scripts (Luau), apply (tree to ECS, sounds, particles,
/// NPCs, output). All three sit inside the profile's `03_Update`.
///
/// Each figure is the WALL SPAN between two markers, so it also counts any
/// unrelated system the executor ran in between. It says when a stage
/// finished, not what the stage cost; the per-system profile (`profiling`
/// feature) is the measure of cost.
#[derive(Resource, Default)]
struct PlayStageClock {
    mark: Option<std::time::Instant>,
    total: [std::time::Duration; 3],
    worst: [std::time::Duration; 3],
    frames: u64,
}

const PLAY_STAGES: [&str; 3] = ["pull", "scripts", "apply"];

fn stage_open(mut clock: ResMut<PlayStageClock>) {
    if crate::profiler::phase_armed() {
        clock.mark = Some(std::time::Instant::now());
    }
}

/// Close stage `I` (and open the next); the last one ends the frame.
fn stage_close<const I: usize>(mut clock: ResMut<PlayStageClock>) {
    if !crate::profiler::phase_armed() {
        return;
    }
    let now = std::time::Instant::now();
    if let Some(start) = clock.mark.replace(now) {
        let d = now.saturating_duration_since(start);
        clock.total[I] += d;
        clock.worst[I] = clock.worst[I].max(d);
    }
    if I + 1 < PLAY_STAGES.len() {
        return;
    }
    clock.mark = None;
    clock.frames += 1;
    if clock.frames < crate::profiler::phase_window() {
        return;
    }
    let frames = clock.frames as f64;
    let mut text = format!(
        "Eustress Play script stages over {} frame(s) (inside 03_Update; wall span, includes interleaved systems)\n",
        clock.frames
    );
    text.push_str("stage      mean ms   worst ms\n");
    for (i, stage) in PLAY_STAGES.iter().enumerate() {
        let mean = clock.total[i].as_secs_f64() * 1000.0 / frames;
        let worst = clock.worst[i].as_secs_f64() * 1000.0;
        text.push_str(&format!("{stage:<9} {mean:>8.2} {worst:>10.2}\n"));
    }
    if let Err(e) = std::fs::write("eustress_profile_play.txt", &text) {
        warn!("profiler(play): failed writing eustress_profile_play.txt: {e}");
    }
    info!("profiler(play): {}", text.lines().skip(2).collect::<Vec<_>>().join(" | "));
    *clock = PlayStageClock::default();
}

pub struct PlayDataModelPlugin;

impl Plugin for PlayDataModelPlugin {
    fn build(&self, app: &mut App) {
        let playing = in_state(PlayModeState::Playing);
        // The HUD: the local player's ScreenGuis drawn, clicked and typed
        // into by the runtime the Player shares.
        app.add_plugins((eustress_play_runtime::hud::HudPlugin, eustress_play_runtime::hud_input::HudInputPlugin));
        app.init_resource::<npc::NpcControllers>()
            .init_resource::<pull::MouseRayState>()
            .init_resource::<pull::InjectedInput>()
            .init_resource::<PlayStageClock>()
            .init_resource::<commerce::PlayCommerce>()
            .init_resource::<remote_players::RemotePlayers>()
            .configure_sets(
                Update,
                (PlayScriptSet::Pull, PlayScriptSet::Scripts, PlayScriptSet::Apply, PlayScriptSet::End)
                    .chain()
                    .run_if(playing.clone()),
            )
            // The avatar has moved for this frame before scripts read it, so
            // a script writing the character's CFrame back (to face the
            // mouse) never races the walk and reads as a teleport.
            .configure_sets(
                Update,
                PlayScriptSet::Pull.after(eustress_common::avatar::AvatarSystems::Locomotion),
            )
            // `seed::seed_session` is registered by PlayModeCorePlugin, which
            // owns the OnEnter(Playing) systems it has to be ordered against.
            .add_systems(
                OnEnter(PlayModeState::Editing),
                (stop_session, commerce::reset_commerce, remote_players::forget_remote_instances),
            )
            // Joins, leaves and the host's identity ticket are followed every
            // frame, Play or not, so no notice is missed; joined players get
            // their `Player` before the scripts run.
            .add_systems(
                Update,
                (
                    remote_players::track_remote_players.before(PlayScriptSet::Pull),
                    commerce::announce_host_identity,
                ),
            )
            .add_systems(
                Update,
                remote_players::sync_remote_players.in_set(PlayScriptSet::Pull).before(pull::pull_frame_state),
            )
            .add_systems(
                Update,
                (
                    pull::pull_frame_state,
                    pull::pull_poses,
                    pull::pull_collisions,
                    pull::pull_character,
                    remote_players::pull_remote_characters,
                    pull::pull_mouse_hit,
                )
                    .chain()
                    .in_set(PlayScriptSet::Pull),
            )
            .add_systems(Update, drive_luau.in_set(PlayScriptSet::Scripts))
            .add_systems(Update, sync_play_asset_root.in_set(PlayScriptSet::Pull))
            // Each part's `AssemblyMass`, from its body's mass, when it changes.
            .add_systems(
                Update,
                eustress_play_runtime::assembly_mass::pull_assembly_mass.in_set(PlayScriptSet::Pull),
            )
            // After the draw step, so joined players' GUIs never show here.
            .add_systems(Update, remote_players::hide_joined_player_guis.after(PlayScriptSet::Apply))
            // Who sits where: the host decides (docs/networking/SEATS.md).
            .add_plugins(seats::SeatPlugin)
            .add_systems(
                PreUpdate,
                pull::apply_injected_buttons.after(bevy::input::InputSystems).run_if(playing.clone()),
            )
            .add_systems(
                Update,
                (
                    apply::apply_frame,
                    terrain_edits::apply_script_terrain_edits,
                    npc::drive_npc_humanoids,
                    camera::show_script_camera_without_avatar,
                    camera::apply_scripted_camera,
                    audio::apply_sound_commands,
                    particles::drive_particles,
                    drain_output,
                )
                    .chain()
                    .in_set(PlayScriptSet::Apply),
            )
            // Its Output lines show the same frame.
            .add_systems(Update, commerce::drive_commerce.in_set(PlayScriptSet::Apply).before(drain_output))
            .add_systems(Update, end_frame.in_set(PlayScriptSet::End))
            // Stage timing for the profiler, at the seams of the chain.
            .add_systems(Update, stage_open.in_set(PlayScriptSet::Pull).before(pull::pull_frame_state))
            .add_systems(Update, stage_close::<0>.in_set(PlayScriptSet::Scripts).before(drive_luau))
            // A VehicleSeat's Throttle and Steer from its occupant's keys,
            // once the Pull set has seated this frame's characters.
            .add_systems(
                Update,
                vehicle_seat_input::drive_vehicle_seats.in_set(PlayScriptSet::Scripts).before(stage_close::<0>),
            )
            .add_systems(Update, stage_close::<1>.in_set(PlayScriptSet::Apply).before(apply::apply_frame))
            .add_systems(Update, stage_close::<2>.in_set(PlayScriptSet::End).after(end_frame));
    }
}

/// Run one Luau frame. On the first one, the session starts first.
///
/// Scripts read the terrain (`workspace.Terrain:ReadVoxels`, the material of
/// a raycast hit) as it stands before this frame's terrain edits, which
/// `apply::apply_frame` applies after the scripts have run. Like the Rune
/// module and `terrain_commands`, they see the Space's one root: none while
/// two roots coexist mid-replacement.
pub(crate) fn drive_luau(
    host: Option<ResMut<PlayLuauHost>>,
    spatial: avian3d::prelude::SpatialQuery,
    colliders: Query<(
        Option<&eustress_common::classes::BasePart>,
        Has<avian3d::prelude::Sensor>,
        Has<TerrainChunkCollider>,
    )>,
    terrain: Query<
        (&TerrainConfig, &TerrainData, Option<&TerrainVolume>, Option<&TerrainBaked>, Option<&TerrainVoxelWater>),
        With<TerrainRoot>,
    >,
    slots: Option<Res<TerrainMaterialSlots>>,
    mut seen_shape: Local<(usize, u64)>,
) {
    let Some(mut host) = host else { return };
    let raycaster = |q: &eustress_common::luau::play::RayQuery| pull::cast_ray(&spatial, &colliders, q);
    let slots = slots.as_deref();
    let terrain_reader: &TerrainReadFn<'_> = &|visit| {
        if let Ok((config, data, volume, baked, water)) = terrain.single() {
            visit(TerrainView {
                config,
                data: surface_data(data, baked),
                volume: volume.unwrap_or(TerrainVolume::empty()),
                water,
                slots,
            });
        }
    };
    if !host.pending_server.is_empty() || host.joining_player.is_some() || !host.pending_client.is_empty() {
        let server = std::mem::take(&mut host.pending_server);
        host.vm.run_scripts(server, &raycaster, terrain_reader);
        if let Some(player) = host.joining_player.take() {
            let dm = host.vm.datamodel().clone();
            let mut g = dm.lock();
            if let Some(players) = g.get_service("Players") {
                if let Err(e) = g.set_parent(player, Some(players)) {
                    g.print(OutputLevel::Error, "Players", format!("the local player could not join: {}", e));
                }
            }
            g.push_event(eustress_common::datamodel::DmEvent::PlayerAdded { player });
        }
        // PlayerAdded handlers run before any client script starts.
        let client = std::mem::take(&mut host.pending_client);
        host.vm.run_scripts(Vec::new(), &raycaster, terrain_reader);
        host.vm.run_scripts(client, &raycaster, terrain_reader);
    }
    // The scripts that arrive with a character start with it. The list is
    // rebuilt only when the tree's shape changed, and the VM skips whatever
    // already runs.
    let shape = {
        let dm = host.vm.datamodel().clone();
        let version = dm.lock().structure_version;
        (std::sync::Arc::as_ptr(&dm) as usize, version)
    };
    if shape != *seen_shape {
        *seen_shape = shape;
        let launches = character_launches(&host.vm.datamodel().lock());
        if !launches.is_empty() {
            host.vm.run_scripts(launches, &raycaster, terrain_reader);
        }
    }
    host.vm.frame(&raycaster, terrain_reader);
}

/// The scripts that arrive with players' characters, as Roblox starts them:
/// every Script in a player's character, and the LocalScripts in the local
/// player's own. A character's scripts are the Space's
/// StarterCharacterScripts, or the default Animate.
fn character_launches(g: &eustress_common::datamodel::DataModel) -> Vec<eustress_common::luau::play::ScriptLaunch> {
    use eustress_common::datamodel::DmValue;
    let mut out = Vec::new();
    let Some(players) = g.find_service("Players") else { return out };
    for &player in g.children(players) {
        if g.class_of(player) != Some("Player") {
            continue;
        }
        let Some(DmValue::Instance(character)) = g.get_prop(player, "Character") else { continue };
        if !g.exists(character) {
            continue;
        }
        let local = g.local_player == Some(player);
        for id in g.descendants(character) {
            let runs = match g.class_of(id) {
                Some("Script") => true,
                Some("LocalScript") => local,
                _ => false,
            };
            if !runs
                || g.get_prop(id, "Disabled").and_then(|v| v.as_bool()).unwrap_or(false)
                || !g.get_prop(id, "Enabled").and_then(|v| v.as_bool()).unwrap_or(true)
            {
                continue;
            }
            let source = g.get_prop(id, "Source").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            if source.trim().is_empty() {
                continue;
            }
            out.push(eustress_common::luau::play::ScriptLaunch { instance: id, source, chunk_name: g.full_name(id) });
        }
    }
    out.sort_by(|a, b| a.chunk_name.cmp(&b.chunk_name));
    out
}

/// The Space folder `MeshId`s are relative to, for the draw step.
fn sync_play_asset_root(root: Option<ResMut<apply::PlayAssetRoot>>, mut commands: Commands) {
    let now = crate::space::space_asset_source::space_asset_root();
    match root {
        Some(mut r) => {
            if r.0 != now {
                r.0 = now;
            }
        }
        None => commands.insert_resource(apply::PlayAssetRoot(now)),
    }
}

/// Forward script output to the Output panel and the engine log.
fn drain_output(dm: Option<Res<PlayDataModel>>, mut output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>) {
    let Some(dm) = dm else { return };
    let lines = std::mem::take(&mut dm.dm.lock().output);
    for line in lines {
        let level = match line.level {
            OutputLevel::Info => crate::ui::slint_ui::LogLevel::Info,
            OutputLevel::Warn => crate::ui::slint_ui::LogLevel::Warn,
            OutputLevel::Error => crate::ui::slint_ui::LogLevel::Error,
        };
        // The log gets an error's stack too, a frame a line.
        let stack: String = line.stack.iter().map(|frame| format!("\n    {frame}")).collect();
        match line.level {
            OutputLevel::Error => warn!("[{}] {}{}", line.source, line.text, stack),
            _ => info!("[{}] {}", line.source, line.text),
        }
        if let Some(out) = output.as_deref_mut() {
            // The text keeps its "[path]" prefix for readers of the plain
            // text; `script` carries the same path for the panel's row, and
            // `file`, `line` and `stack` say where in the code it came from.
            out.push_script(
                level,
                "luau",
                format!("[{}] {}", line.source, line.text),
                &line.source,
                &line.file,
                line.line,
                line.stack,
            );
        }
    }
}

/// Trim events every reader has seen and release destroyed slots.
fn end_frame(dm: Option<Res<PlayDataModel>>, host: Option<Res<PlayLuauHost>>) {
    let Some(dm) = dm else { return };
    let mut g = dm.dm.lock();
    let luau_cursor = host.map(|h| h.vm.event_cursor()).unwrap_or_else(|| g.event_cursor());
    g.trim_events(luau_cursor);
    g.end_frame();
}

/// OnEnter(Editing): stop every script thread, despawn what scripts made,
/// show the storage services again.
fn stop_session(
    mut commands: Commands,
    host: Option<ResMut<PlayLuauHost>>,
    spawned: Query<Entity, With<DataModelSpawned>>,
    mut hidden: Query<(Entity, &HiddenForPlay, &mut Visibility)>,
    touch_reporting: Query<Entity, (With<apply::TouchReportingForPlay>, Without<DataModelSpawned>)>,
    mut npcs: ResMut<npc::NpcControllers>,
    lighting: Option<Res<PlayLightingSnapshot>>,
) {
    if let Some(snapshot) = lighting {
        commands.insert_resource(snapshot.0.clone());
        commands.remove_resource::<PlayLightingSnapshot>();
    }
    if let Some(mut host) = host {
        host.vm.stop();
        info!("🧹 Luau Play VM stopped ({} KiB heap)", host.vm.memory_used() / 1024);
    }
    commands.remove_resource::<PlayLuauHost>();
    commands.remove_resource::<PlayDataModel>();
    commands.remove_resource::<particles::PlayParticles>();
    // Nothing a play-test injected stays held into the next session.
    commands.insert_resource(pull::InjectedInput::default());
    eustress_common::datamodel::set_active(None);
    let mut removed = 0usize;
    // A spawned Model takes its spawned children with it, so a child's own
    // despawn can find it gone: `try_despawn` passes over those quietly.
    for e in spawned.iter() {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.try_despawn();
            removed += 1;
        }
    }
    for (e, h, mut vis) in hidden.iter_mut() {
        *vis = h.previous;
        let mut ec = commands.entity(e);
        ec.remove::<HiddenForPlay>();
        if !h.collider_was_disabled {
            ec.remove::<avian3d::prelude::ColliderDisabled>();
        }
    }
    for e in touch_reporting.iter() {
        commands
            .entity(e)
            .remove::<(apply::TouchReportingForPlay, avian3d::prelude::CollisionEventsEnabled)>();
    }
    npcs.clear();
    if removed > 0 {
        info!("🧹 Despawned {} script-spawned entit(ies) on Stop", removed);
    }
}

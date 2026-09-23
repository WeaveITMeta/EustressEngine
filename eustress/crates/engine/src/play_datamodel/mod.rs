//! # Play DataModel: scripts drive the running scene
//!
//! Binds the live [`DataModel`](eustress_common::datamodel::DataModel) to
//! the ECS for the length of a Play session and runs Luau against it.
//!
//! ```text
//! OnEnter(Playing)  seed::seed_session   ECS -> tree (services, parts, GUI,
//!                                        scripts), hide storage services
//! Update, Playing   pull::*              poses, input, mouse ray + hit,
//!                                        camera, collisions, GUI clicks,
//!                                        the local character
//!                   luau::drive_luau     scripts run against the tree
//!                   apply::apply_frame   tree -> ECS: spawns, reparents,
//!                                        property writes, destroys,
//!                                        sounds, particles, impulses, NPC
//!                                        humanoids, the scripted camera
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
pub mod npc;
pub mod particles;
pub mod pull;
pub mod seed;

use bevy::prelude::*;

use eustress_common::datamodel::{OutputLevel, SharedDataModel};

use crate::play_mode::PlayModeState;

/// The running session's tree.
#[derive(Resource, Clone)]
pub struct PlayDataModel {
    pub dm: SharedDataModel,
}

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

/// Entities spawned for script-made instances. Despawned on Stop.
#[derive(Component, Debug, Clone, Copy)]
pub struct DataModelSpawned;

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

/// Systems of one Play frame, in order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlayScriptSet {
    Pull,
    Scripts,
    Apply,
    End,
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
        app.init_resource::<npc::NpcControllers>()
            .init_resource::<pull::MouseRayState>()
            .init_resource::<pull::InjectedInput>()
            .init_resource::<PlayStageClock>()
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
            .add_systems(OnEnter(PlayModeState::Editing), stop_session)
            .add_systems(
                Update,
                (
                    pull::pull_frame_state,
                    pull::pull_poses,
                    pull::pull_collisions,
                    pull::pull_character,
                    pull::pull_gui_clicks,
                    pull::pull_mouse_hit,
                )
                    .chain()
                    .in_set(PlayScriptSet::Pull),
            )
            .add_systems(Update, drive_luau.in_set(PlayScriptSet::Scripts))
            .add_systems(
                PreUpdate,
                pull::apply_injected_buttons.after(bevy::input::InputSystems).run_if(playing.clone()),
            )
            .add_systems(
                Update,
                (
                    apply::apply_frame,
                    npc::drive_npc_humanoids,
                    camera::apply_scripted_camera,
                    audio::apply_sound_commands,
                    particles::drive_particles,
                    drain_output,
                )
                    .chain()
                    .in_set(PlayScriptSet::Apply),
            )
            .add_systems(Update, end_frame.in_set(PlayScriptSet::End))
            // Stage timing for the profiler, at the seams of the chain.
            .add_systems(Update, stage_open.in_set(PlayScriptSet::Pull).before(pull::pull_frame_state))
            .add_systems(Update, stage_close::<0>.in_set(PlayScriptSet::Scripts).before(drive_luau))
            .add_systems(Update, stage_close::<1>.in_set(PlayScriptSet::Apply).before(apply::apply_frame))
            .add_systems(Update, stage_close::<2>.in_set(PlayScriptSet::End).after(end_frame));
    }
}

/// Run one Luau frame. On the first one, the session starts first.
fn drive_luau(
    host: Option<ResMut<PlayLuauHost>>,
    spatial: avian3d::prelude::SpatialQuery,
    colliders: Query<(Option<&eustress_common::classes::BasePart>, Has<avian3d::prelude::Sensor>)>,
) {
    let Some(mut host) = host else { return };
    let raycaster = |q: &eustress_common::luau::play::RayQuery| pull::cast_ray(&spatial, &colliders, q);
    if !host.pending_server.is_empty() || host.joining_player.is_some() || !host.pending_client.is_empty() {
        let server = std::mem::take(&mut host.pending_server);
        host.vm.run_scripts(server, &raycaster);
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
        host.vm.run_scripts(Vec::new(), &raycaster);
        host.vm.run_scripts(client, &raycaster);
    }
    host.vm.frame(&raycaster);
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
        match line.level {
            OutputLevel::Error => warn!("[{}] {}", line.source, line.text),
            _ => info!("[{}] {}", line.source, line.text),
        }
        if let Some(out) = output.as_deref_mut() {
            out.push_with_source(level, format!("[{}] {}", line.source, line.text), "luau");
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
    for e in spawned.iter() {
        if let Ok(mut ec) = commands.get_entity(e) {
            ec.despawn();
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

//! # The shared Play runtime
//!
//! The draw side of a Play session, which Studio's Play and the Player both
//! run, so a joined player sees what the host sees
//! (`docs/architecture/SHARED_PLAY_RUNTIME.md`): the step that draws the
//! session's DataModel tree into the ECS, the scripted camera, sounds,
//! particles, part materials and part colliders.

pub mod apply;
pub mod assembly_mass;
pub mod audio;
pub mod billboard_pipeline;
pub mod billboards;
pub mod camera;
pub mod gui_images;
pub mod hud;
pub mod hud_input;
pub mod joints;
pub mod material_registry;
pub mod materials;
pub mod part_colliders;
pub mod part_mass;
pub mod particle_render;
pub mod particles;
pub mod sound_player;

use bevy::prelude::*;
use eustress_common::play_session::{PlayDataModel, PlayRole, PlayScriptSet};

/// Everything a Play session draws, for an app that runs the shared runtime
/// whole: the Player, as a [`PlayRole::Replica`]. It owns the order of the
/// frame's sets (`Pull`, `Scripts`, `Apply`, `End`, after the avatar moves),
/// draws the tree, and shows its camera, HUD, billboards, sounds and
/// particles. Studio registers the same systems itself, around its own seed,
/// scripts and Output panel.
pub struct PlayRuntimePlugin {
    pub role: PlayRole,
}

impl Plugin for PlayRuntimePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.role)
            .init_resource::<material_registry::MaterialRegistry>()
            .add_plugins((
                hud::HudPlugin,
                hud_input::HudInputPlugin,
                billboards::BillboardDrawPlugin,
                billboard_pipeline::BillboardPipelinePlugin,
                materials::MaterialSyncPlugin,
                particle_render::ParticleRenderPlugin,
            ))
            .configure_sets(
                Update,
                (PlayScriptSet::Pull, PlayScriptSet::Scripts, PlayScriptSet::Apply, PlayScriptSet::End)
                    .chain()
                    .run_if(resource_exists::<PlayDataModel>),
            )
            // The avatar has moved for this frame before scripts read it.
            .configure_sets(Update, PlayScriptSet::Pull.after(eustress_common::avatar::AvatarSystems::Locomotion))
            .add_systems(Update, advance_play_clock.in_set(PlayScriptSet::Pull))
            .add_systems(
                Update,
                (
                    apply::apply_frame,
                    camera::ensure_replica_play_camera,
                    camera::show_script_camera_without_avatar,
                    camera::apply_scripted_camera,
                    audio::apply_sound_commands,
                    particles::drive_particles,
                    log_play_output,
                )
                    .chain()
                    .in_set(PlayScriptSet::Apply),
            )
            // A part the host resized collides at its new size.
            .add_systems(
                Update,
                part_colliders::rebuild_collider_on_size_change.after(PlayScriptSet::Apply).before(PlayScriptSet::End),
            )
            .add_systems(Update, apply::end_frame.in_set(PlayScriptSet::End));
    }
}

/// The session clock, which `time()`, the task scheduler, Heartbeat and the
/// animation tracks read. Studio's pull advances Studio's; this advances a
/// replica's.
fn advance_play_clock(dm: Option<Res<PlayDataModel>>, time: Res<Time>, role: Res<PlayRole>) {
    if *role != PlayRole::Replica {
        return;
    }
    let Some(dm) = dm else { return };
    let mut g = dm.dm.lock();
    g.frame.dt = time.delta_secs_f64();
    g.frame.time += g.frame.dt;
    g.frame.frame += 1;
}

/// Script output to the log, on an app with no Output panel. Studio's
/// panel drains it there instead.
fn log_play_output(dm: Option<Res<PlayDataModel>>, role: Res<PlayRole>) {
    if *role != PlayRole::Replica {
        return;
    }
    let Some(dm) = dm else { return };
    let lines = std::mem::take(&mut dm.dm.lock().output);
    for line in lines {
        match line.level {
            eustress_common::datamodel::OutputLevel::Error => warn!("[{}] {}", line.source, line.text),
            _ => info!("[{}] {}", line.source, line.text),
        }
    }
}

//! A lost graphics device, recovered without closing the Player.
//!
//! The display turning off after Windows' idle timer, a driver update or a
//! driver reset can take the GPU device away (wgpu `DeviceLost`, "driver
//! implementation is at fault"). Bevy's default answer quits the app, and the
//! Player died on the same frame with a `Buffer::get_mapped_range` panic, behind
//! a generic "unexpected error" box.
//!
//! The recovery itself is shared with Studio
//! (`eustress_play_runtime::gpu_recovery`, whose notes give the cause from
//! Bevy's source): the frame that notices the loss draws nothing more, the
//! renderer is rebuilt in place once the driver is back, and the session keeps
//! its state. `main` adds its log layer to `LogPlugin::custom_layer`, which is
//! how the loss is seen inside that frame.
//!
//! What stays the Player's own: the settings the renderer is rebuilt with
//! (Bevy's defaults, which `systems::raytracing::decide` starts it with), the
//! message when recovery gives up (a Player that hosts nothing then closes),
//! and GPU light clustering off, as Studio has it in Play.

use bevy::light::cluster::GlobalClusterSettings;
use bevy::prelude::*;
use bevy::render::settings::WgpuSettings;
use eustress_play_runtime::gpu_recovery as shared;

pub struct GpuRecoveryPlugin;

impl Plugin for GpuRecoveryPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(shared::GpuRecoveryPlugin {
            rebuild_with: WgpuSettings::default(),
            close_when_given_up: Some(tell_the_player),
        })
        .add_systems(Update, prefer_cpu_clustering);
    }
}

/// Recovery gave up and the Player closes: say why, in plain words.
fn tell_the_player(_world: &mut World) {
    crate::show_message(
        "Eustress Player lost the graphics card",
        "The graphics card stopped responding several times in a row, so the Player has closed.\n\n\
         Start the Player again. If this keeps happening, update your graphics driver.",
    );
}

/// Turn GPU light clustering off once Bevy has chosen it, as Studio does in
/// Play. It is not what keeps a lost device from panicking (the render-set gate
/// in the shared recovery is); it keeps the Player's lighting path the same as
/// Studio's.
fn prefer_cpu_clustering(settings: Option<ResMut<GlobalClusterSettings>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Some(mut settings) = settings else { return };
    if settings.gpu_clustering.take().is_some() {
        info!("GPU light clustering is off, as in Studio's Play");
    }
    *done = true;
}

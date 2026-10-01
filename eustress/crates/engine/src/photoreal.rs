//! Studio's cameras take the shared view look.
//!
//! The image stack every camera a person looks through gets (filmic
//! tonemapping, SMAA, bloom and the opt-in stages, each read once from its
//! `EUSTRESS_` switch) lives in [`eustress_common::plugins::camera_look`], so
//! Studio's editor camera, the avatar camera and the scripted Play camera,
//! in Studio and in the Player, all look the same. A camera opts in with
//! [`ViewCamera`]; this plugin gives every [`StudioCamera`] that marker.
//! `studio_camera_bundle` still carries the pieces that are Studio's own
//! (`DepthPrepass`, the marker).
//!
//! | env | default | effect |
//! |-----|---------|--------|
//! | `EUSTRESS_SMAA`            | on   | subpixel-morphological AA |
//! | `EUSTRESS_BLOOM`           | on   | filmic bloom |
//! | `EUSTRESS_BLOOM_INTENSITY` | 0.15 | bloom strength (bevy's `NATURAL`) |
//! | `EUSTRESS_MSAA`            | off  | `2`/`4`/`8` restores hardware MSAA |
//! | `EUSTRESS_GTAO`            | off  | ground-contact AO |
//! | `EUSTRESS_AUTO_EXPOSURE`   | off  | histogram exposure adaptation |
//! | `EUSTRESS_CONTACT_SHADOWS` | off  | screen-space contact shadows from the sun |
//!
//! ## Measured: the stack is not free
//!
//! On Space1 with no splat cloud resident, `Msaa::Off` + SMAA + bloom moved
//! `06_render+present` from 27.3 to 31.1 ms/frame. The 4x-MSAA saving scales
//! with overdraw while the post passes cost a fixed amount per frame, so the
//! stack wins on heavy or alpha-blended scenes and loses on sparse ones.

use bevy::prelude::*;
use eustress_common::plugins::camera_look::{
    auto_exposure_on, bloom_on, gtao_on, msaa_samples, smaa_on, ViewCamera,
};

use crate::default_scene::StudioCamera;

pub use eustress_common::plugins::camera_look::contact_shadows_on;

/// What the view look resolved to this run. Reporting only: each switch is
/// read once per process, never toggled live.
#[derive(Resource, Clone, Copy, Debug)]
pub struct PhotorealSettings {
    pub smaa: bool,
    pub bloom: bool,
    pub gtao: bool,
    pub auto_exposure: bool,
    pub contact_shadows: bool,
    /// `None` = `Msaa::Off` (the default); `Some(n)` = hardware MSAA at n samples.
    pub msaa_samples: Option<u32>,
}

impl Default for PhotorealSettings {
    fn default() -> Self {
        Self {
            smaa: smaa_on(),
            bloom: bloom_on(),
            gtao: gtao_on(),
            auto_exposure: auto_exposure_on(),
            contact_shadows: contact_shadows_on(),
            msaa_samples: msaa_samples(),
        }
    }
}

pub struct PhotorealPlugin;

impl Plugin for PhotorealPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PhotorealSettings>().add_systems(Update, mark_studio_cameras);
    }
}

/// A Studio camera is a view camera: it takes the shared look the frame
/// after it appears.
fn mark_studio_cameras(
    mut commands: Commands,
    cameras: Query<Entity, (Added<StudioCamera>, Without<ViewCamera>)>,
) {
    for entity in &cameras {
        commands.entity(entity).insert(ViewCamera);
    }
}

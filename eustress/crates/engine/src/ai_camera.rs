//! # Independent AI camera
//!
//! A second, **off-screen** camera the AI drives and captures so it can "see
//! what it's doing" without displacing the user's viewport.
//!
//! ## Why it can't displace the real camera
//!
//! A Bevy camera renders to a target. The user's editor camera targets the
//! **window**; this camera carries a `RenderTarget::Image` component pointing
//! at an off-screen GPU texture that's never shown. They render in parallel to
//! different destinations, so adding this camera never changes who owns the
//! window. The AI "sees" by reading that image back to a PNG — the same
//! `Screenshot` readback `viewport.capture` uses for the window, pointed at the
//! image instead.
//!
//! ## Always-on, off-screen
//!
//! It is modeled on the **working editor camera** (camera_controller.rs): a
//! plain active `Camera3d`. An *inactive* image-target camera is extracted as a
//! view but never prepared by Bevy's light/cluster prep, so it lacks
//! `ViewClusterBindings`/`ViewShadowBindings` and panics
//! `prepare_mesh_view_bind_groups`. Keeping it active (like the editor camera)
//! makes it a fully-prepared view. It renders to its image every frame, so a
//! capture is just "screenshot the always-current image." (Cost is one extra
//! off-screen pass; if that ever bites framerate we can throttle it, but it
//! never touches the window.)
//!
//! ## Explorer object
//!
//! Spawned with the same `classes::Instance { class_name: Camera }` the editor
//! camera carries, so it appears in the Explorer as a Camera named
//! "AI Camera". It is an **independent root entity** — deliberately NOT a Bevy
//! `ChildOf` the editor camera, because childing would transform-couple them
//! and destroy the independence that's the whole point.

use bevy::prelude::*;
use bevy::camera::RenderTarget;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use std::path::PathBuf;

/// Off-screen render resolution for the AI camera.
pub const AI_CAM_WIDTH: u32 = 1280;
pub const AI_CAM_HEIGHT: u32 = 720;

/// Marker for the AI camera entity.
#[derive(Component)]
pub struct AiCamera;

/// A queued capture (just the output path — the camera renders its image every
/// frame, so the texture is always current).
pub struct PendingCapture {
    pub path: PathBuf,
}

/// Off-screen render-target handle + pending-capture state for the AI camera.
#[derive(Resource, Default)]
pub struct AiCameraState {
    pub image: Option<Handle<Image>>,
    pub pending: Option<PendingCapture>,
}

pub struct AiCameraPlugin;

impl Plugin for AiCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AiCameraState>()
            .add_systems(Startup, spawn_ai_camera)
            .add_systems(Update, process_ai_capture);
    }
}

/// Create the off-screen image and spawn the AI camera — modeled on the editor
/// camera (a plain active `Camera3d`), differing only in its render target
/// (an Image instead of the window) and `Msaa::Off` (the image is single-sample).
fn spawn_ai_camera(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut state: ResMut<AiCameraState>,
) {
    let size = Extent3d {
        width: AI_CAM_WIDTH,
        height: AI_CAM_HEIGHT,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // RENDER_ATTACHMENT so the camera can draw into it; COPY_SRC so the
    // screenshot readback can copy it out.
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_SRC
        | TextureUsages::RENDER_ATTACHMENT;
    let handle = images.add(image);
    state.image = Some(handle.clone());

    commands.spawn((
        // SAME method as the editor camera (`studio_camera_bundle`) → identical
        // by construction, so `SharedLightingPlugin` attaches the same
        // skybox/atmosphere/env-map to both and the shared mesh-view bind-group
        // layout matches. The ONLY intended differences are the off-screen
        // render target and the `AiCamera` marker.
        crate::default_scene::studio_camera_bundle(
            "AI Camera",
            Transform::from_xyz(12.0, 12.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
        ),
        AiCamera,
        // Off-screen: render to our image, never the window — so it can't
        // displace the editor camera. (Bevy resolves MSAA into the image, so we
        // keep the editor camera's default MSAA rather than forcing it off.)
        RenderTarget::Image(handle.into()),
        // Exclude from SharedLightingPlugin's skybox/atmosphere/env-map auto-attach
        // — the engine's OWN marker for special/overlay cameras. A second camera
        // that picks up Atmosphere+EnvironmentMap hits a GPU timing race: the
        // layout expects those bindings (23) before their textures are prepared,
        // so its bind group is transiently smaller (20) → wgpu validation panic.
        // Keeping the AI camera a PLAIN view (self-consistent bindings) avoids it.
        // It still renders the scene lit by the Sun; sky/ambient can be added
        // later once the prepare-order is controlled.
        eustress_common::plugins::lighting_plugin::NoAtmosphere,
    ));
    info!(
        "📷 AI Camera spawned (off-screen {AI_CAM_WIDTH}x{AI_CAM_HEIGHT}, via studio_camera_bundle) — \
         independent of the editor camera"
    );
}

/// Queue a capture to `path`. Called by the bridge handler.
pub fn request_capture(state: &mut AiCameraState, path: PathBuf) {
    state.pending = Some(PendingCapture { path });
}

/// On a queued request, screenshot the off-screen image (the camera renders it
/// every frame, so it's always current) and save it to the requested path.
fn process_ai_capture(mut commands: Commands, mut state: ResMut<AiCameraState>) {
    let Some(pending) = state.pending.take() else {
        return;
    };
    let Some(image) = state.image.clone() else {
        return;
    };
    let out_path = pending.path;
    commands
        .spawn(bevy::render::view::screenshot::Screenshot::image(image))
        .observe(
            move |trigger: bevy::ecs::observer::On<
                bevy::render::view::screenshot::ScreenshotCaptured,
            >| {
                let img = trigger.image.clone();
                let out = out_path.clone();
                // Off the render thread: GPU readback → PNG encode.
                bevy::tasks::AsyncComputeTaskPool::get()
                    .spawn(async move {
                        match img.try_into_dynamic() {
                            Ok(d) => {
                                if let Err(e) = d.save(&out) {
                                    tracing::warn!("ai_camera.capture save failed: {}", e);
                                } else {
                                    tracing::info!("ai_camera.capture → {:?}", out);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("ai_camera.capture conversion failed: {}", e)
                            }
                        }
                    })
                    .detach();
            },
        );
}

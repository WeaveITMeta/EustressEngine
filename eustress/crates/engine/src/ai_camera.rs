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
use bevy::core_pipeline::Skybox;
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
            .add_systems(Update, (process_ai_capture, attach_skybox_to_ai_camera));
    }
}

/// Give the AI camera the SKY it was missing.
///
/// The AI camera carries `NoAtmosphere`, so `SharedLightingPlugin`'s
/// `attach_skybox_to_cameras` skips it — which is why its background was a
/// black void. That shared system attaches `Skybox` AND `EnvironmentMapLight`
/// together, and it's the env-map (image-based-lighting) bindings that triggered
/// the "bind group 20 != layout 23" wgpu race on a second camera. So here we
/// attach ONLY the `Skybox` (the cubemap drawn behind geometry — a separate
/// render pass, no mesh-view env-map binding) and leave the env-map/atmosphere
/// off. Result: the AI sees the same sky gradient the user does (and the moon
/// disc + star field, which are world meshes any camera renders), without the
/// crash. The sun DISC is atmosphere-rendered and still absent — a deliberate,
/// separate follow-up. Fill light still comes from the per-camera `AmbientLight`
/// added in `spawn_ai_camera`.
///
/// Runs every frame so it (a) attaches once the skybox handle exists (it's
/// created in `setup_lighting`, after our Startup spawn) and (b) tracks the
/// handle when the skybox is regenerated on time-of-day change.
fn attach_skybox_to_ai_camera(
    mut commands: Commands,
    skybox_handle: Res<eustress_common::plugins::lighting_plugin::SkyboxHandle>,
    mut cams: Query<(Entity, Option<&mut Skybox>), With<AiCamera>>,
) {
    let Some(img) = skybox_handle.handle.as_ref() else {
        return;
    };
    for (entity, existing) in cams.iter_mut() {
        match existing {
            Some(mut sky) => {
                if sky.image.id() != img.id() {
                    sky.image = img.clone();
                }
            }
            None => {
                commands.entity(entity).insert(Skybox {
                    image: img.clone(),
                    brightness: 1000.0,
                    rotation: Quat::IDENTITY,
                });
                info!("🌅 AI Camera: attached skybox (sky now visible to the AI)");
            }
        }
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
        // Distinct render order so the editor tools' `find(camera.order == 0)`
        // never grab THIS off-screen camera. Both the editor and AI camera come
        // from `studio_camera_bundle`, which defaults to order 0 — that collision
        // made select/move/scale/rotate cast their picking ray from the AI
        // camera's pose, so selection was offset for every tool. -1 renders this
        // image pass before the window camera and keeps it out of order 0.
        Camera { order: -1, ..default() },
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
        // ── My eyes' fill light ──────────────────────────────────────────
        // The engine sets `GlobalAmbientLight::NONE` (all fill comes from the
        // Atmosphere's image-based lighting), but this camera opts OUT of
        // Atmosphere — so without this, every surface not facing the Sun
        // renders pure black and the AI can't judge color or material.
        //
        // `AmbientLight` is a PER-CAMERA component in Bevy 0.18 (distinct from
        // the `GlobalAmbientLight` Resource): it brightens ONLY this view, never
        // the user's editor camera. It adds no texture bindings, so it does NOT
        // re-trigger the Atmosphere/env-map bind-group race that `NoAtmosphere`
        // sidesteps. Sky-tinted + bright so shadows read as deep color, not
        // void — giving the AI usable "eyes" for critiquing its own builds.
        AmbientLight {
            color: Color::srgb(0.78, 0.85, 1.0),
            brightness: 1400.0,
            affects_lightmapped_meshes: true,
        },
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

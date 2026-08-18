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

/// Frames to let the AI camera render before screenshotting it.
///
/// The camera is powered down between captures (see [`process_ai_capture`]),
/// so its render target holds a stale frame — or none at all on the first
/// capture — at the moment a request arrives. The render graph needs a couple
/// of frames to produce a current image before the readback is meaningful.
const CAPTURE_WARMUP_FRAMES: u32 = 6;

/// Frames to keep the camera POWERED UP after the screenshot is queued.
///
/// This is the fix for captures coming back as a uniform white image (the
/// `Image::new_fill` colour — i.e. a readback of a target nothing ever rendered
/// into). The previous code set `is_active = false` in the SAME frame it
/// spawned the `Screenshot`, so the readback was serviced on a frame where no
/// active camera targeted that image. Bevy only prepares a view target for
/// ACTIVE cameras, so the copy ran against a texture that was never prepared
/// and produced the fill colour every time.
///
/// Holding the camera on for a few extra frames lets the render graph actually
/// service the copy. The camera still powers down immediately afterwards, so
/// the "off-screen camera costs nothing while idle" property is preserved.
const CAPTURE_COOLDOWN_FRAMES: u32 = 4;

/// Off-screen render-target handle + pending-capture state for the AI camera.
#[derive(Resource, Default)]
pub struct AiCameraState {
    pub image: Option<Handle<Image>>,
    pub pending: Option<PendingCapture>,
    /// Frames the camera has been powered up for the current request; 0 when
    /// idle (camera inactive). See [`CAPTURE_WARMUP_FRAMES`].
    warmup: u32,
    /// Frames remaining to keep the camera live AFTER a screenshot was queued,
    /// so the render graph can service the readback. See
    /// [`CAPTURE_COOLDOWN_FRAMES`].
    cooldown: u32,
}

pub struct AiCameraPlugin;

impl Plugin for AiCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AiCameraState>()
            .add_systems(Startup, spawn_ai_camera)
            // The manual `attach_skybox_to_ai_camera` system is gone; the AI
            // camera carries `NoAtmosphere` (set in spawn_ai_camera), so it opts
            // OUT of SharedLightingPlugin's atmosphere + skybox attach — required
            // to avoid the multi-camera atmosphere prepare-race (a hard wgpu
            // panic; see the marker comment in spawn_ai_camera).
            .add_systems(Update, process_ai_capture);
    }
}

/// Create the off-screen image and spawn the AI camera. Built from the SAME
/// `studio_camera_bundle` as the editor camera (identical projection, tonemap,
/// depth prepass). It differs in: render target (an off-screen Image, not the
/// window), the `AiCamera` marker, `Camera { order: -1 }` (so editor tools'
/// `order == 0` lookups never grab it), and — critically — `NoAtmosphere`, so it
/// is NOT a second atmosphere camera (see the marker comment below for the wgpu
/// prepare-race that forces this).
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
        // `is_active: false` — the camera is powered up only for the few frames
        // of an actual capture (see `process_ai_capture`). Bevy's default is
        // `true`, which had this off-screen 1280×720 camera rendering the whole
        // scene every frame for the entire session.
        Camera { order: -1, is_active: false, ..default() },
        AiCamera,
        // Off-screen: render to our image, never the window — so it can't
        // displace the editor camera. R1: MSAA is now Off (inherited from
        // `studio_camera_bundle`, required by TAA); the image is single-sampled
        // and TAA does the anti-aliasing.
        RenderTarget::Image(handle.into()),
        // ── AI-camera atmosphere opt-out (REQUIRED — fixes a hard wgpu panic) ──
        // The AI camera MUST stay off Bevy `Atmosphere`. Two `Camera3d`s both
        // carrying `Atmosphere` (the editor camera + this one) hit a multi-camera
        // atmosphere prepare-race: the atmosphere LUT bind group isn't ready for
        // one view when `prepare_mesh_view_bind_groups` builds it, so the
        // transient bind group is short by exactly the atmosphere bindings and
        // wgpu aborts ("bind group descriptor (21) != layout (24)"). An R1 attempt
        // to drop this marker for an "identical look" reintroduced that exact
        // panic on Bevy 0.18 (it did NOT auto-resolve, as had been hoped).
        // `NoAtmosphere` keeps the EDITOR as the single atmosphere camera → no
        // race. It also makes SharedLightingPlugin skip skybox attach for this
        // camera, so captures render the scene lit by the Sun / scene lights on a
        // plain background (no dynamic sky). Race-free capture parity (an explicit
        // `Skybox` + static `EnvironmentMapLight` + flat `AmbientLight` attached
        // directly here) is a clean follow-up — none of those three cause the
        // multi-camera race; only Bevy's dynamic `Atmosphere` does.
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

/// Drive a queued capture: power the camera up, let it render, screenshot the
/// off-screen image, then power it back down.
///
/// PERF — this is why the camera is not simply left on. It was spawned with
/// Bevy's default `is_active: true` and nothing ever cleared it, so this
/// off-screen 1280×720 camera rendered a COMPLETE second view of the scene
/// every frame for the entire session: depth prepass, shadows, opaque,
/// transparent, and (once the post stack landed) SMAA + bloom as well. On a
/// 3,348-entity Space that doubled render cost permanently — `06_render+
/// present` measured 33.9 ms/frame on a scene with almost nothing in it —
/// to keep a render target warm for the rare `ai_camera.capture` call. The
/// tool's own documentation already promised the opposite ("the off-screen
/// camera powers up only for this shot, so it doesn't tax the user's
/// framerate"); this makes that true.
fn process_ai_capture(
    mut commands: Commands,
    mut state: ResMut<AiCameraState>,
    mut cam: Query<&mut Camera, With<AiCamera>>,
) {
    // Cooldown: a screenshot is in flight. Keep the camera ACTIVE so the render
    // graph prepares its view target and services the readback, then power down.
    if state.cooldown > 0 {
        state.cooldown -= 1;
        if state.cooldown == 0 {
            if let Ok(mut c) = cam.single_mut() {
                c.is_active = false;
            }
        }
        return;
    }

    if state.pending.is_none() {
        return; // idle: camera stays powered down, costing nothing
    }

    // Power up on the first frame of a request, then yield so the render graph
    // actually runs with the camera active.
    if state.warmup == 0 {
        if let Ok(mut c) = cam.single_mut() {
            c.is_active = true;
        }
        state.warmup = 1;
        return;
    }

    // Give it a few frames to produce a current image before reading back.
    if state.warmup < CAPTURE_WARMUP_FRAMES {
        state.warmup += 1;
        return;
    }

    // Ready: consume the request. The camera deliberately stays ACTIVE here —
    // powering it down in this frame (as this used to) meant the screenshot was
    // serviced with no active camera on that render target, and every capture
    // came back as the flat `Image::new_fill` colour. `cooldown` powers it down
    // a few frames later instead.
    let Some(pending) = state.pending.take() else {
        return;
    };
    state.warmup = 0;
    // Set BEFORE the early return below so a missing image can never leave the
    // camera stuck on.
    state.cooldown = CAPTURE_COOLDOWN_FRAMES;
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

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
//! window. The AI "sees" by reading that image back to a PNG, the same
//! `Screenshot` readback `viewport.capture` uses for the window, pointed at the
//! image instead.
//!
//! ## Powered down between captures
//!
//! It is modeled on the **working editor camera** (camera_controller.rs): a
//! plain `Camera3d` built from the same bundle. It is inactive while idle and
//! powered up only for the frames of a capture ([`process_ai_capture`]).
//!
//! ## One camera, in every Space
//!
//! It is a runtime service of the engine, not part of any Space: nothing in a
//! Space's files defines it, and a save never writes it (it has no
//! `BasePart`). There is always exactly one, named "AI Camera".
//! [`ensure_ai_camera_exists`] puts it back at its starting pose whenever none
//! exists, which covers every way one goes away: opening, reverting or
//! creating a Space (`space_ops::open_space` despawns every `Instance`), Stop
//! after Play, and a delete in the Explorer. A bridge call puts it back at
//! once ([`ensure_in_world`]) rather than wait a frame. The image it renders
//! into is made once and survives all of these.
//!
//! ## Explorer object
//!
//! It carries the same `classes::Instance { class_name: Camera }` the editor
//! camera carries, so the Explorer shows it as a Camera named "AI Camera". It
//! is an **independent root entity**, deliberately NOT a Bevy `ChildOf` the
//! editor camera, because childing would transform-couple them and destroy the
//! independence that is the whole point.

use bevy::prelude::*;
use bevy::camera::RenderTarget;
use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use std::path::PathBuf;

/// Off-screen render resolution for the AI camera.
pub const AI_CAM_WIDTH: u32 = 1280;
pub const AI_CAM_HEIGHT: u32 = 720;

/// The AI camera's name, in the Explorer and to every tool.
pub const AI_CAMERA_NAME: &str = "AI Camera";

/// Why a request for the AI camera fails in an engine with no renderer.
pub const UNAVAILABLE: &str = "the AI camera needs Studio's renderer, and this engine has none \
     (eustress-headless runs without a window or GPU). Open the Space in Studio to aim or capture \
     the AI camera.";

/// Where the AI camera starts, and where it is put back.
pub fn start_pose() -> Transform {
    Transform::from_xyz(12.0, 12.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y)
}

/// Marker for the AI camera entity.
#[derive(Component)]
pub struct AiCamera;

/// A queued capture (just the output path).
pub struct PendingCapture {
    pub path: PathBuf,
}

/// Frames to let the AI camera render before screenshotting it.
///
/// The camera is powered down between captures (see [`process_ai_capture`]),
/// so its render target holds a stale frame, or none at all on the first
/// capture, at the moment a request arrives. The render graph needs a couple
/// of frames to produce a current image before the readback is meaningful.
const CAPTURE_WARMUP_FRAMES: u32 = 6;

/// Frames to keep the camera POWERED UP after the screenshot is queued.
///
/// This is the fix for captures coming back as a uniform image of the
/// `Image::new_fill` colour, a readback of a target nothing ever rendered
/// into. Powering down in the SAME frame the `Screenshot` spawned meant the
/// readback was serviced on a frame where no active camera targeted that
/// image. Bevy only prepares a view target for ACTIVE cameras, so the copy ran
/// against a texture that was never prepared.
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
    /// Frames the camera has rendered for the current request; 0 when idle.
    /// See [`CAPTURE_WARMUP_FRAMES`].
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
            .add_systems(Startup, create_ai_camera_target)
            // The camera carries `NoAtmosphere` (see `ai_camera_bundle`), so it
            // opts OUT of SharedLightingPlugin's atmosphere + skybox attach,
            // which avoids the two-camera atmosphere panic (see the marker
            // comment there). Its sky is the editor view's, copied without the
            // atmosphere by `raytracing::sky::mirror_sky_to_capture_cameras`.
            .add_systems(Update, (ensure_ai_camera_exists, process_ai_capture).chain());
    }
}

/// Make the off-screen image the AI camera renders into. Made once: every
/// camera [`ensure_ai_camera_exists`] puts back renders into the same image.
fn create_ai_camera_target(mut images: ResMut<Assets<Image>>, mut state: ResMut<AiCameraState>) {
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
    state.image = Some(images.add(image));
}

/// The AI camera at its starting pose, rendering into `image`. Built from the
/// SAME `studio_camera_bundle` as the editor camera (identical projection,
/// tonemap, depth prepass). It differs in: render target (an off-screen Image,
/// not the window), the `AiCamera` marker, `Camera { order: -1 }` (so editor
/// tools' `order == 0` lookups never grab it), and, critically,
/// `NoAtmosphere`, so it is NOT a second atmosphere camera.
fn ai_camera_bundle(image: Handle<Image>) -> impl Bundle {
    (
        // SAME method as the editor camera (`studio_camera_bundle`): identical
        // by construction, so `SharedLightingPlugin` attaches the same
        // skybox/atmosphere/env-map to both and the shared mesh-view bind-group
        // layout matches. The ONLY intended differences are the off-screen
        // render target and the `AiCamera` marker.
        crate::default_scene::studio_camera_bundle(AI_CAMERA_NAME, start_pose()),
        // Distinct render order so the editor tools' `find(camera.order == 0)`
        // never grab THIS off-screen camera. Both the editor and AI camera come
        // from `studio_camera_bundle`, which defaults to order 0; that collision
        // made select/move/scale/rotate cast their picking ray from the AI
        // camera's pose. -1 renders this image pass before the window camera.
        // `is_active: false`: the camera is powered up only for the few frames
        // of an actual capture (see `process_ai_capture`). Bevy's default is
        // `true`, which had this off-screen 1280x720 camera rendering the whole
        // scene every frame for the entire session.
        Camera { order: -1, is_active: false, ..default() },
        AiCamera,
        // Off-screen: render to our image, never the window, so it can't
        // displace the editor camera. MSAA is Off (inherited from
        // `studio_camera_bundle`, required by TAA); the image is single-sampled
        // and TAA does the anti-aliasing.
        RenderTarget::Image(image.into()),
        // ── AI-camera atmosphere opt-out (REQUIRED: fixes a hard wgpu panic) ──
        // Bevy 0.19's atmosphere serves one camera. `write_atmosphere_buffer`
        // takes `single()` over the cameras with an atmosphere (active or
        // not) and writes nothing when there are two, so the atmosphere
        // buffer has no binding, the view bind group leaves its three entries
        // out, and the pipelines that expect them make wgpu abort ("bind group
        // descriptor (21) != layout (24)"). `NoAtmosphere` keeps the EDITOR as
        // the single atmosphere camera. It also makes SharedLightingPlugin
        // skip the sky attach for this camera; `raytracing::sky` gives it the
        // editor's sky instead (a Skybox of the live sky cubemap and the
        // filtered environment map), neither of which is an atmosphere.
        eustress_common::plugins::lighting_plugin::NoAtmosphere,
    )
}

/// Keep exactly one AI camera: put it back, at its starting pose and on the
/// same image, whenever none exists. The editor camera's
/// `ensure_camera_exists` does the same for the window camera; this is the
/// one place the AI camera is spawned.
fn ensure_ai_camera_exists(
    mut commands: Commands,
    state: Res<AiCameraState>,
    cameras: Query<(), With<AiCamera>>,
) {
    if !cameras.is_empty() {
        return;
    }
    let Some(image) = state.image.clone() else {
        return;
    };
    commands.spawn(ai_camera_bundle(image));
    info!("📷 AI Camera ready (off-screen {AI_CAM_WIDTH}x{AI_CAM_HEIGHT}, independent of the editor camera)");
}

/// The AI camera, put back now when none exists, for a caller that holds the
/// World (a bridge handler) and cannot wait for [`ensure_ai_camera_exists`]'s
/// next frame. `Err` says why there can be none: an engine without a renderer.
pub fn ensure_in_world(world: &mut World) -> Result<Entity, String> {
    let image = match world.get_resource::<AiCameraState>() {
        None => return Err(UNAVAILABLE.to_string()),
        Some(state) => state.image.clone(),
    };
    let existing = {
        let mut cameras = world.query_filtered::<Entity, With<AiCamera>>();
        cameras.iter(world).next()
    };
    if let Some(camera) = existing {
        return Ok(camera);
    }
    let Some(image) = image else {
        return Err("the AI camera's render target is not ready yet; try again in a moment".to_string());
    };
    Ok(world.spawn(ai_camera_bundle(image)).id())
}

/// Queue a capture to `path`. Called by the bridge handler.
pub fn request_capture(state: &mut AiCameraState, path: PathBuf) {
    state.pending = Some(PendingCapture { path });
}

/// Drive a queued capture: power the camera up, let it render, screenshot the
/// off-screen image, then power it back down.
///
/// PERF: this is why the camera is not simply left on. Left active, this
/// off-screen 1280x720 camera rendered a COMPLETE second view of the scene
/// every frame for the entire session (depth prepass, shadows, opaque,
/// transparent, post), doubling render cost to keep a target warm for the
/// rare capture.
///
/// A request with no camera to render (a Space just opened, or the camera was
/// deleted) waits for [`ensure_ai_camera_exists`] to put one back and starts
/// its warm-up over, so the screenshot is never taken of an image nothing
/// renders into.
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

    let Ok(mut camera) = cam.single_mut() else {
        state.warmup = 0;
        return;
    };
    // Power up (again, for a camera put back mid-request) and let it render a
    // few frames before reading back.
    if !camera.is_active {
        camera.is_active = true;
    }
    if state.warmup < CAPTURE_WARMUP_FRAMES {
        state.warmup += 1;
        return;
    }

    // Ready: consume the request. The camera deliberately stays ACTIVE here;
    // `cooldown` powers it down a few frames later.
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
                // Off the render thread: GPU readback, then PNG encode.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_image() -> AiCameraState {
        AiCameraState { image: Some(Handle::<Image>::default()), ..default() }
    }

    fn ai_cameras(app: &mut App) -> Vec<(Entity, Transform, String, isize, bool)> {
        let world = app.world_mut();
        let mut query = world.query_filtered::<
            (Entity, &Transform, &eustress_common::classes::Instance, &Camera, &RenderTarget),
            With<AiCamera>,
        >();
        query
            .iter(world)
            .map(|(entity, tf, instance, camera, target)| {
                assert!(matches!(target, RenderTarget::Image(_)), "the AI camera must render off-screen");
                (entity, *tf, instance.name.clone(), camera.order, camera.is_active)
            })
            .collect()
    }

    /// Whatever removes it (a Space opening despawns every `Instance`, the
    /// Explorer deletes it), exactly one AI camera comes back at its starting
    /// pose, named, off-screen, powered down and out of the editor's order.
    #[test]
    fn the_ai_camera_comes_back_whenever_it_is_gone() {
        let mut app = App::new();
        app.insert_resource(state_with_image());
        app.add_systems(Update, ensure_ai_camera_exists);

        app.update();
        let first = ai_cameras(&mut app);
        assert_eq!(first.len(), 1, "one at startup");
        let (first_entity, pose, name, order, active) = first[0].clone();
        assert_eq!(name, AI_CAMERA_NAME);
        assert_eq!(order, -1);
        assert!(!active, "powered down while idle");
        assert!(pose.translation.abs_diff_eq(start_pose().translation, 1e-4));

        // A Space opens: `open_space` despawns every Instance, children first.
        app.world_mut().entity_mut(first_entity).insert(Transform::from_xyz(1.0, 2.0, 3.0));
        let instances: Vec<Entity> = {
            let world = app.world_mut();
            let mut q = world.query_filtered::<Entity, With<eustress_common::classes::Instance>>();
            q.iter(world).collect()
        };
        for entity in instances {
            app.world_mut().despawn(entity);
        }
        app.update();
        let after_open = ai_cameras(&mut app);
        assert_eq!(after_open.len(), 1, "one after a Space opens");
        assert_ne!(after_open[0].0, first_entity);
        assert!(after_open[0].1.translation.abs_diff_eq(start_pose().translation, 1e-4), "back at the starting pose");

        // A delete in the Explorer.
        app.world_mut().despawn(after_open[0].0);
        app.update();
        assert_eq!(ai_cameras(&mut app).len(), 1, "one after a delete");

        // And it stays one.
        app.update();
        app.update();
        assert_eq!(ai_cameras(&mut app).len(), 1, "never a second");
    }

    /// A bridge call finds the camera, or puts it back at once, and never
    /// makes a second.
    #[test]
    fn a_bridge_call_puts_the_camera_back_at_once() {
        let mut world = World::new();
        world.insert_resource(state_with_image());
        let made = ensure_in_world(&mut world).expect("a camera");
        assert!(world.get::<AiCamera>(made).is_some());
        assert_eq!(ensure_in_world(&mut world).unwrap(), made, "the same camera, not a second");
        let mut q = world.query_filtered::<Entity, With<AiCamera>>();
        assert_eq!(q.iter(&world).count(), 1);
    }

    /// An engine with no renderer (no AI camera plugin) says so, instead of
    /// "entity not found".
    #[test]
    fn without_a_renderer_the_camera_is_unavailable() {
        let mut world = World::new();
        let why = ensure_in_world(&mut world).unwrap_err();
        assert!(why.contains("headless"), "{why}");
        let mut q = world.query_filtered::<Entity, With<AiCamera>>();
        assert_eq!(q.iter(&world).count(), 0);
    }

    /// A capture asked for while no camera exists waits for one, warms it up
    /// from the start, and only then reads back; a camera removed mid-warm-up
    /// restarts the count.
    #[test]
    fn a_capture_waits_for_a_camera_to_render() {
        let mut app = App::new();
        let mut state = state_with_image();
        request_capture(&mut state, std::env::temp_dir().join("eus_ai_camera_unused.png"));
        app.insert_resource(state);
        app.add_systems(Update, process_ai_capture);

        // No camera: the request waits.
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().resource::<AiCameraState>().pending.is_some());
        assert_eq!(app.world().resource::<AiCameraState>().warmup, 0);

        // A camera appears: it powers up and warms up.
        let camera = app.world_mut().spawn(ai_camera_bundle(Handle::<Image>::default())).id();
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().get::<Camera>(camera).unwrap().is_active, "powered up for the capture");
        assert_eq!(app.world().resource::<AiCameraState>().warmup, 3);

        // It is removed mid-warm-up: the count starts over, the request waits.
        app.world_mut().despawn(camera);
        app.update();
        assert_eq!(app.world().resource::<AiCameraState>().warmup, 0);
        assert!(app.world().resource::<AiCameraState>().pending.is_some());

        // A new camera warms up the full count before any readback.
        let camera = app.world_mut().spawn(ai_camera_bundle(Handle::<Image>::default())).id();
        for _ in 0..CAPTURE_WARMUP_FRAMES {
            app.update();
        }
        assert!(app.world().get::<Camera>(camera).unwrap().is_active);
        assert_eq!(app.world().resource::<AiCameraState>().warmup, CAPTURE_WARMUP_FRAMES);
        assert!(app.world().resource::<AiCameraState>().pending.is_some(), "no readback before the warm-up ends");
    }
}

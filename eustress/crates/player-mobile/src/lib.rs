//! Phase 0 spike. Pattern (a): Bevy owns the window and the wgpu device.
//! Slint's `SkiaWGPURenderer` draws a label and a button into a texture on
//! that same device, and Bevy samples the texture on an unlit quad in front
//! of a lit cube.
//!
//! On Android the instance, surface, and device are created only after winit
//! `Resumed` (GameActivity `onSurfaceCreated`) has put a `RawHandleWrapper` on
//! the primary window. `RenderPlugin::build` would otherwise request an
//! adapter before any surface exists, and GLES finds none.
//!
//! Vulkan is the default (`Backends::VULKAN | Backends::GL`, or `WGPU_BACKEND`).
//! `PowerPreference::HighPerformance` ranks a hardware Vulkan adapter ahead of
//! GLES, which reports device type Other. If the surface filter rejects every
//! adapter, the same instance is asked again with no `compatible_surface`.
//! GLES-only is the next try. `WGPU_BACKEND=gl` forces that path. On suspend
//! the swapchain is dropped and presentation is held. The Skia renderer stays
//! alive: it was associated with the Slint window when the component was
//! shown, and a replacement renderer is not. Resume clears the hold, Bevy
//! creates a new surface on the same device, and the overlay texture is
//! allocated again and drawn by that same renderer. Rotation swaps the
//! texture between 640×280 and 280×640. Destroying the GLES device from
//! `Update` panics inside wgpu, so the device stays. A real device-lost
//! callback still rebuilds it and re-associates Skia. See `PHASE0_SPIKE.md`.
//!
//! Pattern (b) lives behind the `pattern-b` feature (`pattern_b` module) and
//! is not the activity entry. FemtoVG-wgpu is not used: Slint does not build
//! that renderer for Android.

#[cfg(feature = "pattern-b")]
mod pattern_b;

mod auth;
mod env;
mod gallery;
mod links;
mod platform;
mod session;
mod shell;
mod space;

use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, ClearColorConfig, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::touch::TouchInput;
use bevy::input::{mouse::MouseButtonInput, ButtonState};
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy::render::view::Msaa;
use bevy::render::error_handler::{ErrorType, RenderError, RenderErrorHandler, RenderErrorPolicy};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::{RenderAdapter, RenderDevice, RenderInstance, RenderQueue};
#[cfg(target_os = "android")]
use bevy::render::renderer::{RenderAdapterInfo, WgpuWrapper};
#[cfg(target_os = "android")]
use bevy::render::settings::WgpuSettingsPriority;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderPlugin};
#[cfg(target_os = "android")]
use bevy::window::RawHandleWrapper;
use bevy::window::{AppLifecycle, PrimaryWindow};
use i_slint_core::renderer::RendererSealed;
use slint::platform::skia_renderer::SkiaWGPURenderer;
use slint::platform::WindowEvent;
use slint::wgpu_29::wgpu;
use slint::{ComponentHandle, LogicalPosition, PhysicalSize};

const UI_WIDTH: u32 = 640;
const UI_HEIGHT: u32 = 280;

/// Bumped when the GLES device is destroyed so the render world publishes the
/// replacement handles. Starts at 0; the first device is epoch 0.
static GPU_EPOCH: AtomicU64 = AtomicU64::new(0);

/// `WindowAdapter::renderer` returns a shared reference, so the Skia renderer
/// cannot live behind `RefCell`. It is swapped only on the main thread,
/// between frames, and only when the wgpu device itself is replaced.
/// Replacing it requires [`bind_renderer`]: Slint records the window adapter
/// on the renderer inside `Window::set_component`, and a fresh renderer does
/// not have that link.
struct RendererSlot {
    slot: UnsafeCell<Option<SkiaWGPURenderer>>,
}

impl RendererSlot {
    fn new(renderer: SkiaWGPURenderer) -> Self {
        Self {
            slot: UnsafeCell::new(Some(renderer)),
        }
    }

    fn get(&self) -> Option<&SkiaWGPURenderer> {
        // SAFETY: the slot is touched only on the main thread, and the
        // returned reference is not held across `set`.
        unsafe { (*self.slot.get()).as_ref() }
    }

    fn set(&self, renderer: SkiaWGPURenderer) {
        // SAFETY: same main-thread rule as `get`. No renderer reference is live.
        unsafe {
            *self.slot.get() = Some(renderer);
        }
    }
}

struct BevyWindowAdapter {
    size: Cell<slint::PhysicalSize>,
    /// Set by Slint when a property, animation, or input event needs paint.
    /// `render_slint` draws on this and then clears it.
    dirty: Cell<bool>,
    slint_window: slint::Window,
    renderer: RendererSlot,
}

impl slint::platform::WindowAdapter for BevyWindowAdapter {
    fn window(&self) -> &slint::Window {
        &self.slint_window
    }

    fn size(&self) -> slint::PhysicalSize {
        self.size.get()
    }

    fn renderer(&self) -> &dyn slint::platform::Renderer {
        self.renderer.get().expect("slint renderer is down")
    }

    fn set_visible(&self, _visible: bool) -> Result<(), slint::PlatformError> {
        Ok(())
    }

    fn request_redraw(&self) {
        self.dirty.set(true);
    }

    fn internal(
        &self,
        _: i_slint_core::InternalToken,
    ) -> Option<&dyn i_slint_core::window::WindowAdapterInternal> {
        Some(self)
    }
}

impl i_slint_core::window::WindowAdapterInternal for BevyWindowAdapter {
    fn input_method_request(&self, request: i_slint_core::window::InputMethodRequest) {
        // Slint calls this when a TextInput gains or loses focus. The default
        // adapter method is empty, so GameActivity never showed the keyboard
        // (`mInputShown=false`) and hardware keys stayed in the InputConnection.
        apply_input_method(&request);
    }
}

impl BevyWindowAdapter {
    fn new(handles: &WgpuHandles) -> Rc<Self> {
        let renderer = make_skia(handles).expect("SkiaWGPURenderer::new");
        Rc::new_cyclic(|self_weak: &Weak<Self>| Self {
            size: Cell::new(slint::PhysicalSize::new(UI_WIDTH, UI_HEIGHT)),
            dirty: Cell::new(true),
            slint_window: slint::Window::new(self_weak.clone()),
            renderer: RendererSlot::new(renderer),
        })
    }
}

struct SlintBevyPlatform {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl slint::platform::Platform for SlintBevyPlatform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        let handles = (
            self.instance.clone(),
            self.adapter.clone(),
            self.device.clone(),
            self.queue.clone(),
        );
        let adapter = BevyWindowAdapter::new(&handles);
        SLINT_WINDOWS.with(|windows| windows.borrow_mut().push(Rc::downgrade(&adapter)));
        Ok(adapter)
    }
}

/// `Window::set_component` is what stores this adapter on the renderer.
/// `render_to_texture` fails with "Renderer must be associated with component
/// before use" until that call has been made on the renderer currently in the slot.
fn bind_renderer(adapter: &Rc<BevyWindowAdapter>) {
    let as_dyn: Rc<dyn slint::platform::WindowAdapter> = adapter.clone();
    if let Some(renderer) = adapter.renderer.get() {
        renderer.set_window_adapter(&as_dyn);
    }
}

fn dispatch_resized(adapter: &BevyWindowAdapter) {
    let physical = adapter.size.get();
    let scale = adapter.slint_window.scale_factor();
    adapter.slint_window.dispatch_event(WindowEvent::Resized {
        size: physical.to_logical(scale),
    });
}

struct SlintContext {
    adapter: Rc<BevyWindowAdapter>,
    /// Last handles Skia was built with. Cleared when the GLES device is dropped
    /// so resume does not rebuild Skia on a dead context.
    handles: RefCell<Option<WgpuHandles>>,
}

/// The gallery camera. Turned off once a play camera is drawing, so the two
/// do not both clear the window.
#[derive(Component)]
struct GalleryCamera;

/// The gallery's floor and cube. Removed when a downloaded Space opens.
#[derive(Component)]
pub(crate) struct StandInYard;

/// Draws the Slint texture above the world. It does not clear.
#[derive(Component)]
struct OverlayCamera;

#[derive(Component)]
struct LitCube;

#[derive(Component)]
struct SlintQuad;

#[derive(Resource)]
struct TextureSender(std::sync::mpsc::Sender<wgpu::Texture>);

#[derive(Resource, Clone, ExtractResource)]
struct SlintImageHandle(Handle<Image>);

#[derive(Resource, Clone, Copy, ExtractResource, PartialEq, Eq)]
struct OverlayEpoch(u64);

#[derive(Resource)]
struct SlintSharedTexture {
    receiver: Mutex<std::sync::mpsc::Receiver<wgpu::Texture>>,
    texture: Arc<Mutex<Option<wgpu::Texture>>>,
}

type WgpuHandles = (wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue);

#[derive(Resource, Clone)]
struct DeviceSender(std::sync::mpsc::Sender<WgpuHandles>);

#[derive(Resource)]
struct DeviceReceiver(Mutex<std::sync::mpsc::Receiver<WgpuHandles>>);

#[derive(Resource, Clone)]
struct StoredWgpuSettings(WgpuSettings);

#[derive(Resource, Default)]
struct SurfaceLoss {
    /// `Suspended` was delivered. The next `Running` is a resume, not startup.
    suspended: bool,
    /// Resume (or a device replacement) should allocate a new overlay texture
    /// and draw into it. The Skia renderer itself is kept.
    pending_overlay: bool,
}

/// Pixel size of the overlay texture. Landscape is 640×280. Portrait swaps
/// those axes so a rotation rebuilds the texture instead of stretching it.
#[derive(Resource)]
struct OverlaySize {
    width: u32,
    height: u32,
}

/// Extracted into the render world. While true, the patched `bevy_render`
/// skips `create_surfaces` and `prepare_windows`.
#[derive(Resource, Default, Clone, Copy, ExtractResource)]
struct PlayerSwapchainHold(bool);

thread_local! {
    static SLINT_WINDOWS: RefCell<Vec<Weak<BevyWindowAdapter>>> = RefCell::new(Vec::new());
}

fn clone_wgpu(
    instance: &RenderInstance,
    adapter: &RenderAdapter,
    device: &RenderDevice,
    queue: &RenderQueue,
) -> WgpuHandles {
    (
        (**instance.0).clone(),
        (**adapter.0).clone(),
        device.wgpu_device().clone(),
        (**queue.0).clone(),
    )
}

fn make_skia(handles: &WgpuHandles) -> Result<SkiaWGPURenderer, String> {
    let (instance, adapter, device, queue) = handles;
    let info = adapter.get_info();
    eprintln!(
        "phase0: Skia renderer on {:?} ({})",
        info.backend, info.name
    );
    // Skia's wgpu backend is Vulkan/Metal/DX12. It returns an error for GL,
    // so a GLES device still renders the Bevy scene without the overlay.
    SkiaWGPURenderer::new(
        instance.clone(),
        adapter.clone(),
        device.clone(),
        queue.clone(),
    )
    .map_err(|err| {
        format!(
            "Skia wgpu renderer unavailable on {:?}: {err}",
            info.backend
        )
    })
}

fn overlay_image(width: u32, height: u32) -> Image {
    let size = Extent3d {
        width: width.max(1),
        height: height.max(1),
        depth_or_array_layers: 1,
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("slint-overlay"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size);
    image
}

fn setup_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let image_handle = images.add(overlay_image(UI_WIDTH, UI_HEIGHT));
    commands.insert_resource(SlintImageHandle(image_handle.clone()));
    commands.insert_resource(OverlayEpoch(1));

    let ui_material = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    let cube_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.86, 0.38, 0.16),
        ..default()
    });

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.15, 1.15, 1.15))),
        MeshMaterial3d(cube_material),
        Transform::from_xyz(0.0, -0.15, 0.0),
        LitCube,
        StandInYard,
    ));
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial3d(ui_material),
        Transform::from_xyz(0.0, 0.0, 0.0),
        RenderLayers::layer(1),
        SlintQuad,
    ));

    commands.spawn((
        avian3d::prelude::RigidBody::Static,
        avian3d::prelude::Collider::cuboid(40.0, 0.5, 40.0),
        Mesh3d(meshes.add(Cuboid::new(80.0, 1.0, 80.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.22, 0.2),
            ..default()
        })),
        Transform::from_xyz(0.0, -0.5, 0.0),
        StandInYard,
    ));

    commands.spawn((
        Camera3d::default(),
        Camera { order: 0, ..default() },
        Transform::from_xyz(0.0, 1.15, 6.2).looking_at(Vec3::new(0.0, 0.35, 0.0), Vec3::Y),
        GalleryCamera,
        Name::new("GalleryCamera"),
    ));
    commands.spawn((
        Camera3d::default(),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed { width: UI_WIDTH as f32, height: UI_HEIGHT as f32 },
            ..OrthographicProjection::default_3d()
        }),
        Camera {
            order: 20,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::Custom(Color::NONE),
            },
            ..default()
        },
        Tonemapping::None,
        Msaa::Off,
        RenderLayers::layer(1),
        Transform::from_xyz(0.0, 0.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
        OverlayCamera,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 80_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn install_render_error_handler(mut handler: ResMut<RenderErrorHandler>) {
    *handler = RenderErrorHandler(on_render_error);
}

fn on_render_error(
    error: &RenderError,
    main_world: &mut World,
    _render_world: &mut World,
) -> RenderErrorPolicy {
    if error.ty == ErrorType::DeviceLost {
        let settings = main_world.resource::<StoredWgpuSettings>().0.clone();
        #[cfg(target_os = "android")]
        {
            // Automatic would call `initialize_renderer`, which panics when the
            // surface filter rejects GLES. Build the replacement here, against
            // the window that resume just installed.
            match create_android_gpu(main_world, &settings) {
                Ok(creation) => {
                    GPU_EPOCH.fetch_add(1, Ordering::SeqCst);
                    if let Some(mut hold) = main_world.get_resource_mut::<PlayerSwapchainHold>() {
                        // Extracted next frame, after this device is unpacked.
                        hold.0 = false;
                    }
                    eprintln!("phase0: wgpu device lost, recreating against the current window");
                    return RenderErrorPolicy::Recover(creation);
                }
                Err(err) => {
                    eprintln!("phase0: device lost and recreate failed: {err}");
                    return RenderErrorPolicy::StopRendering;
                }
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            eprintln!("phase0: wgpu device lost, recreating against the current window");
            return RenderErrorPolicy::Recover(RenderCreation::Automatic(Box::new(settings)));
        }
    }
    eprintln!("phase0: render error {:?}: {}", error.ty, error.description);
    RenderErrorPolicy::StopRendering
}

fn publish_shared_device(
    instance: Option<Res<RenderInstance>>,
    adapter: Option<Res<RenderAdapter>>,
    device: Option<Res<RenderDevice>>,
    queue: Option<Res<RenderQueue>>,
    sender: Res<DeviceSender>,
    mut sent_epoch: Local<Option<u64>>,
) {
    let epoch = GPU_EPOCH.load(Ordering::SeqCst);
    if *sent_epoch == Some(epoch) {
        return;
    }
    let (Some(instance), Some(adapter), Some(device), Some(queue)) =
        (instance, adapter, device, queue)
    else {
        return;
    };
    let _ = sender
        .0
        .send(clone_wgpu(&instance, &adapter, &device, &queue));
    *sent_epoch = Some(epoch);
}

fn init_slint(world: &mut World) {
    let pending = {
        let receiver = world.resource::<DeviceReceiver>();
        receiver.0.lock().expect("device receiver").try_recv().ok()
    };

    if let Some(handles) = pending {
        let renderer = match make_skia(&handles) {
            Ok(renderer) => renderer,
            Err(err) => {
                eprintln!("phase0: {err}. Bevy scene continues without the Slint overlay");
                return;
            }
        };
        if world.contains_non_send::<SlintContext>() {
            let ctx = world.non_send::<SlintContext>();
            *ctx.handles.borrow_mut() = Some(handles.clone());
            ctx.adapter.renderer.set(renderer);
            bind_renderer(&ctx.adapter);
            dispatch_resized(&ctx.adapter);
            if let Some(mut loss) = world.get_resource_mut::<SurfaceLoss>() {
                loss.pending_overlay = true;
            }
            eprintln!("phase0: Skia renderer recreated on the new device");
            return;
        }
        // `BevyWindowAdapter::new` builds its own Skia renderer. This one only
        // proved the backend is accepted.
        drop(renderer);

        slint::platform::set_platform(Box::new(SlintBevyPlatform {
            instance: handles.0.clone(),
            adapter: handles.1.clone(),
            device: handles.2.clone(),
            queue: handles.3.clone(),
        }))
        .expect("set_platform");

        let ui = shell::PlayerShell::new().expect("PlayerShell::new");
        shell::wire(&ui);
        ui.window().show().expect("show");
        let adapter = SLINT_WINDOWS
            .with(|windows| windows.borrow().first().and_then(|weak| weak.upgrade()))
            .expect("window adapter");
        ui.window()
            .dispatch_event(WindowEvent::WindowActiveChanged(true));
        // The first `publish_overlay` runs before this adapter exists, so it
        // cannot send `ScaleFactorChanged`. Applying scale 1 here laid the
        // gallery out in physical pixels. Resume looked right only because
        // that later publish finally sent the window scale.
        let (width, height, scale) = slint_size_now(world);
        apply_slint_size(&adapter, width, height, scale);
        // Slint's platform types are `!Send`. This exclusive system runs on the
        // schedule thread. Inserting them from `Plugin::build` or `finish`
        // would record the constructing thread, and the next frame's NonSend
        // systems would abort.
        world.insert_non_send(shell::ShellUi { ui });
        world.insert_non_send(SlintContext {
            adapter,
            handles: RefCell::new(Some(handles)),
        });
        return;
    }

    if !world.contains_non_send::<SlintContext>() {
        return;
    }
    let ctx = world.non_send::<SlintContext>();
    if ctx.adapter.renderer.get().is_some() {
        return;
    }
    let Some(handles) = ctx.handles.borrow().clone() else {
        return;
    };
    match make_skia(&handles) {
        Ok(renderer) => {
            ctx.adapter.renderer.set(renderer);
            bind_renderer(&ctx.adapter);
            dispatch_resized(&ctx.adapter);
            if let Some(mut loss) = world.get_resource_mut::<SurfaceLoss>() {
                loss.pending_overlay = true;
            }
            eprintln!("phase0: Skia renderer recreated on the stored device");
        }
        Err(err) => eprintln!("phase0: {err}"),
    }
}

fn receive_texture(shared: Res<SlintSharedTexture>) {
    if let Ok(texture) = shared.receiver.lock().expect("texture receiver").try_recv() {
        *shared.texture.lock().expect("texture slot") = Some(texture);
    }
}

fn send_slint_texture(
    handle: Option<Res<SlintImageHandle>>,
    epoch: Option<Res<OverlayEpoch>>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    sender: Res<TextureSender>,
    mut sent_epoch: Local<Option<u64>>,
) {
    let Some(epoch) = epoch else {
        return;
    };
    if *sent_epoch == Some(epoch.0) {
        return;
    }
    let Some(handle) = handle else {
        return;
    };
    let Some(gpu_image) = gpu_images.get(&handle.0) else {
        return;
    };
    let texture = (*gpu_image.texture).clone();
    let _ = sender.0.send(texture);
    *sent_epoch = Some(epoch.0);
}

fn render_slint(ctx: Option<NonSend<SlintContext>>, shared: Res<SlintSharedTexture>) {
    let Some(ctx) = ctx else {
        return;
    };
    if ctx.adapter.renderer.get().is_none() {
        return;
    }
    slint::platform::update_timers_and_animations();
    let dirty = ctx.adapter.dirty.get() || ctx.adapter.slint_window.has_active_animations();
    if !dirty {
        return;
    }
    let slot = shared.texture.lock().expect("texture slot");
    let Some(texture) = slot.as_ref() else {
        // Keep `dirty` set until the render world publishes the image.
        return;
    };
    let size = ctx.adapter.size.get();
    if let Err(err) = ctx
        .adapter
        .renderer
        .get()
        .expect("slint renderer")
        .render_to_texture(texture)
    {
        eprintln!("slint render_to_texture: {err}");
    } else {
        ctx.adapter.dirty.set(false);
        eprintln!(
            "phase0: slint overlay drawn {}x{} scale {:.3}",
            size.width,
            size.height,
            ctx.adapter.slint_window.scale_factor()
        );
    }
}

fn spin_cube(time: Res<Time>, mut cubes: Query<&mut Transform, With<LitCube>>) {
    for mut transform in &mut cubes {
        transform.rotate_y(0.6 * time.delta_secs());
        transform.rotate_x(0.15 * time.delta_secs());
    }
}

/// Bevy 0.19 already reports touch positions in logical pixels. Dividing by
/// `scale_factor` again (about 2.625 at 420 dpi) mapped every tap off the widget.
fn logical_from_touch(position: Vec2) -> LogicalPosition {
    LogicalPosition::new(position.x, position.y)
}

/// Bevy's window starts at 1280×720. `create_windows` multiplies that by the
/// monitor scale before the native size is applied, so the first resolution
/// on a 420 dpi phone is 3360×1890. That is not the window.
fn is_scaled_default_window(width: u32, height: u32, scale: f32) -> bool {
    if !(scale.is_finite() && scale > 1.01) {
        return false;
    }
    let expected_w = (1280.0 * scale).round() as u32;
    let expected_h = (720.0 * scale).round() as u32;
    width == expected_w && height == expected_h
}

fn window_scale(window: &Window) -> f32 {
    let scale = window.resolution.scale_factor();
    if scale.is_finite() && scale > 0.01 {
        scale
    } else {
        1.0
    }
}

/// Physical overlay size. Prefer the winit window: on Android its `inner_size`
/// is the native window (2400×1080) while Bevy's component can still be the
/// scaled 1280×720 default.
fn overlay_pixels(entity: Entity, window: &Window) -> Option<(u32, u32, f32)> {
    let scale = window_scale(window);
    let native = bevy::winit::WINIT_WINDOWS.with(|windows| {
        windows.borrow().get_window(entity).map(|winit_window| {
            let size = winit_window.inner_size();
            (size.width, size.height)
        })
    });
    if let Some((width, height)) = native {
        if width > 0 && height > 0 {
            return Some((width, height, scale));
        }
    }
    let width = window.resolution.physical_width();
    let height = window.resolution.physical_height();
    if width == 0 || height == 0 || is_scaled_default_window(width, height, scale) {
        return None;
    }
    Some((width, height, scale))
}

fn slint_size_now(world: &mut World) -> (u32, u32, f32) {
    let from_window = {
        let mut windows = world.query_filtered::<(Entity, &Window), With<PrimaryWindow>>();
        windows
            .iter(world)
            .find_map(|(entity, window)| overlay_pixels(entity, window))
    };
    if let Some(size) = from_window {
        return size;
    }
    let scale = {
        let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
        windows.iter(world).next().map(window_scale).unwrap_or(1.0)
    };
    let overlay = world.resource::<OverlaySize>();
    (overlay.width, overlay.height, scale)
}

fn apply_slint_size(adapter: &BevyWindowAdapter, width: u32, height: u32, scale: f32) {
    let scale = if scale.is_finite() && scale > 0.01 { scale } else { 1.0 };
    let physical = PhysicalSize::new(width.max(1), height.max(1));
    adapter.size.set(physical);
    let logical = physical.to_logical(scale);
    if (adapter.slint_window.scale_factor() - scale).abs() > 0.01 {
        adapter
            .slint_window
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
    }
    adapter
        .slint_window
        .dispatch_event(WindowEvent::Resized { size: logical });
    slint::platform::WindowAdapter::request_redraw(adapter);
    eprintln!(
        "phase0: slint window {}x{} scale {scale:.3} logical {:.1}x{:.1}",
        physical.width, physical.height, logical.width, logical.height
    );
}

/// Copy the native window size onto Bevy before `input_actions` reads the
/// logical viewport. Runs in `First`, ahead of `PreUpdate`.
fn sync_native_window_size(mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>) {
    for (entity, mut window) in &mut windows {
        let Some((width, height, _)) = overlay_pixels(entity, &window) else {
            continue;
        };
        if window.resolution.physical_width() == width && window.resolution.physical_height() == height {
            continue;
        }
        eprintln!(
            "phase0: native window {width}x{height} replaces {}x{}",
            window.resolution.physical_width(),
            window.resolution.physical_height()
        );
        window.resolution.set_physical_resolution(width, height);
    }
}

fn dispatch_pointer(
    adapter: &BevyWindowAdapter,
    position: Option<LogicalPosition>,
    pressed: Option<bool>,
) {
    let Some(position) = position else {
        adapter
            .slint_window
            .dispatch_event(WindowEvent::PointerExited);
        return;
    };
    adapter
        .slint_window
        .dispatch_event(WindowEvent::PointerMoved { position });
    if let Some(down) = pressed {
        let button = slint::platform::PointerEventButton::Left;
        let event = if down {
            WindowEvent::PointerPressed { button, position }
        } else {
            WindowEvent::PointerReleased { button, position }
        };
        adapter.slint_window.dispatch_event(event);
    }
}

fn pointer_input(
    mut mouse: MessageReader<MouseButtonInput>,
    mut touches: MessageReader<TouchInput>,
    windows: Query<&Window>,
    input: Option<Res<eustress_common::input_actions::InputState>>,
    ctx: Option<NonSend<SlintContext>>,
    mut ui_fingers: Local<HashSet<u64>>,
) {
    let Some(ctx) = ctx else {
        return;
    };
    if ctx.adapter.renderer.get().is_none() {
        return;
    }
    let Some(window) = windows.iter().next() else {
        return;
    };
    let scale = window_scale(window);

    if let Some(cursor) = window.cursor_position() {
        // `cursor_position` is already in logical pixels.
        let over = Some(LogicalPosition::new(cursor.x, cursor.y));
        let mut press = None;
        for event in mouse.read() {
            if event.button == MouseButton::Left {
                press = Some(event.state == ButtonState::Pressed);
            }
        }
        if press.is_some() {
            eprintln!(
                "phase0: slint pointer mouse logical=({:.1}, {:.1}) scale={scale:.3} pressed={press:?}",
                cursor.x, cursor.y
            );
        }
        dispatch_pointer(&ctx.adapter, over, press);
    }

    for touch in touches.read() {
        let over = logical_from_touch(touch.position);
        // `input_actions` runs in PreUpdate and assigns the finger. Gallery
        // sets `capture_all`, so every finger is UI. Stick and camera fingers
        // stay with the world. `Ended` has already removed the finger; the
        // id is in `released_ui` or in the set recorded on `Started`.
        //
        // While playing, `capture_all` is off and the Gallery button is not
        // in `ui_rects`, so that tap was `FingerOwner::World` and never
        // reached Slint (parity8, after resume). A TouchArea or TextInput
        // under the point is still the shell's. A stick, camera, or pinch
        // finger is not, even if a widget sits under it.
        let gesture = input.as_ref().is_some_and(|input| input.finger_is_world_gesture(touch.id));
        let chrome = !gesture && slint_chrome_contains(&ctx.adapter.slint_window, over);
        let tracked = match touch.phase {
            bevy::input::touch::TouchPhase::Started => input.as_ref().is_none_or(|input| {
                input.finger_is_ui(touch.id) || input.released_ui.contains(&touch.id)
            }),
            bevy::input::touch::TouchPhase::Moved => {
                ui_fingers.contains(&touch.id)
                    || input.as_ref().is_some_and(|input| input.finger_is_ui(touch.id))
            }
            bevy::input::touch::TouchPhase::Ended | bevy::input::touch::TouchPhase::Canceled => {
                ui_fingers.remove(&touch.id)
                    || input.as_ref().is_some_and(|input| input.released_ui.contains(&touch.id))
            }
        };
        let ui = !gesture && (tracked || chrome);
        if ui && matches!(touch.phase, bevy::input::touch::TouchPhase::Started) {
            ui_fingers.insert(touch.id);
        }
        if !ui {
            if !matches!(touch.phase, bevy::input::touch::TouchPhase::Moved) {
                eprintln!(
                    "phase0: touch id={} {:?} logical=({:.1}, {:.1}) scale={scale:.3} kept by the world",
                    touch.id, touch.phase, over.x, over.y
                );
            }
            continue;
        }
        let press = match touch.phase {
            bevy::input::touch::TouchPhase::Started => Some(true),
            bevy::input::touch::TouchPhase::Ended | bevy::input::touch::TouchPhase::Canceled => {
                Some(false)
            }
            bevy::input::touch::TouchPhase::Moved => None,
        };
        if !matches!(touch.phase, bevy::input::touch::TouchPhase::Moved) {
            eprintln!(
                "phase0: slint pointer id={} {:?} logical=({:.1}, {:.1}) scale={scale:.3}",
                touch.id, touch.phase, over.x, over.y
            );
        }
        dispatch_pointer(&ctx.adapter, Some(over), press);
    }
}

fn slint_key_text(event: &KeyboardInput) -> Option<slint::SharedString> {
    if let Some(text) = &event.text {
        if !text.is_empty() {
            return Some(text.as_str().into());
        }
    }
    use bevy::input::keyboard::Key as BevyKey;
    use slint::platform::Key as SlintKey;
    let named = match &event.logical_key {
        BevyKey::Backspace => SlintKey::Backspace,
        BevyKey::Delete => SlintKey::Delete,
        BevyKey::Enter => SlintKey::Return,
        BevyKey::Tab => SlintKey::Tab,
        BevyKey::ArrowLeft => SlintKey::LeftArrow,
        BevyKey::ArrowRight => SlintKey::RightArrow,
        BevyKey::ArrowUp => SlintKey::UpArrow,
        BevyKey::ArrowDown => SlintKey::DownArrow,
        BevyKey::Escape => SlintKey::Escape,
        BevyKey::Home => SlintKey::Home,
        BevyKey::End => SlintKey::End,
        BevyKey::PageUp => SlintKey::PageUp,
        BevyKey::PageDown => SlintKey::PageDown,
        _ => return None,
    };
    Some(named.into())
}

/// A Slint `TouchArea` or `TextInput` covers this window point.
///
/// The playing shell's Gallery button and the gallery search field are
/// those items. The jump circle and the thumbstick are drawn without one,
/// so a point in the world does not hit.
fn slint_chrome_contains(window: &slint::Window, pos: LogicalPosition) -> bool {
    let inner = i_slint_core::window::WindowInner::from_pub(window);
    let tree = inner.component();
    let mut hit = false;
    i_slint_core::item_tree::visit_items(
        &tree,
        i_slint_core::item_tree::TraversalOrder::BackToFront,
        |item_tree, _item, index, _state| {
            let item = i_slint_core::item_tree::ItemRc::new(item_tree.clone(), index);
            let interactive = item.downcast::<i_slint_core::items::TouchArea>().is_some()
                || item.downcast::<i_slint_core::items::TextInput>().is_some();
            if interactive && slint_item_contains(&item, pos) {
                hit = true;
                return i_slint_core::item_tree::ItemVisitorResult::Abort;
            }
            i_slint_core::item_tree::ItemVisitorResult::Continue(())
        },
        (),
    );
    hit
}

fn slint_item_contains(item: &i_slint_core::item_tree::ItemRc, pos: LogicalPosition) -> bool {
    let geom = item.geometry();
    if geom.size.width <= 0.0 || geom.size.height <= 0.0 {
        return false;
    }
    // `map_to_window` adds ancestor positions and not this item's own x/y.
    let origin = item.map_to_window(i_slint_core::lengths::LogicalPoint::new(geom.origin.x, geom.origin.y));
    pos.x >= origin.x
        && pos.y >= origin.y
        && pos.x <= origin.x + geom.size.width
        && pos.y <= origin.y + geom.size.height
}

fn dispatch_key(adapter: &BevyWindowAdapter, text: slint::SharedString, state: ButtonState, repeat: bool) {
    let event = match state {
        ButtonState::Pressed if repeat => WindowEvent::KeyPressRepeated { text: text.clone() },
        ButtonState::Pressed => WindowEvent::KeyPressed { text: text.clone() },
        ButtonState::Released => WindowEvent::KeyReleased { text: text.clone() },
    };
    eprintln!("phase0: slint key {state:?} repeat={repeat} text={text:?}");
    adapter.slint_window.dispatch_event(event);
}

/// Hardware keys arrive as `KeyboardInput`. GameActivity's text buffer arrives
/// as `Ime`, except on Android where winit drops `InputEvent::TextEvent` after
/// clearing the dirty flag. The buffer itself is still readable.
fn slint_keyboard(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut keys: MessageReader<KeyboardInput>,
    mut ime: MessageReader<Ime>,
    ctx: Option<NonSend<SlintContext>>,
    mut ime_text: Local<String>,
) {
    let Some(ctx) = ctx else {
        return;
    };
    if ctx.adapter.renderer.get().is_none() {
        return;
    }
    let focused = i_slint_core::window::WindowInner::from_pub(&ctx.adapter.slint_window)
        .text_input_focused();
    for mut window in &mut windows {
        if window.ime_enabled != focused {
            eprintln!("phase0: soft keyboard via ime_enabled={focused}");
            window.ime_enabled = focused;
        }
    }

    #[cfg(target_os = "android")]
    let mut ime_consumed_chars = poll_android_text(&ctx.adapter, focused, &mut ime_text);
    #[cfg(not(target_os = "android"))]
    let mut ime_consumed_chars = {
        let _ = (&mut *ime_text, focused);
        false
    };

    for event in ime.read() {
        if let Ime::Commit { value, .. } = event {
            if value.is_empty() {
                continue;
            }
            eprintln!("phase0: slint ime commit {value:?}");
            dispatch_key(&ctx.adapter, value.as_str().into(), ButtonState::Pressed, false);
            dispatch_key(&ctx.adapter, value.as_str().into(), ButtonState::Released, false);
            ime_consumed_chars = true;
        }
    }

    for event in keys.read() {
        let Some(text) = slint_key_text(event) else {
            continue;
        };
        // The Android text buffer already contains this frame's letters.
        // Forwarding the key as well would insert it twice.
        if ime_consumed_chars && event.text.is_some() {
            eprintln!("phase0: slint key skipped (ime buffer has the text) {text:?}");
            continue;
        }
        dispatch_key(&ctx.adapter, text, event.state, event.repeat);
    }
}

#[cfg(target_os = "android")]
fn poll_android_text(adapter: &BevyWindowAdapter, focused: bool, last: &mut String) -> bool {
    if !focused {
        last.clear();
        return false;
    }
    let Some(app) = bevy::android::ANDROID_APP.get() else {
        return false;
    };
    let state = app.text_input_state();
    if state.text == *last {
        return false;
    }
    eprintln!(
        "phase0: slint ime text {:?} selection={:?}..{:?}",
        state.text, state.selection.start, state.selection.end
    );
    *last = state.text.clone();
    let mut key_event = i_slint_core::input::KeyEvent::default();
    key_event.text = state.text.as_str().into();
    let event = i_slint_core::input::InternalKeyEvent {
        event_type: i_slint_core::input::KeyEventType::CommitComposition,
        replacement_range: Some(i32::MIN..i32::MAX),
        cursor_position: Some(state.selection.end as i32),
        anchor_position: Some(state.selection.start as i32),
        key_event,
        ..Default::default()
    };
    i_slint_core::window::WindowInner::from_pub(&adapter.slint_window).process_key_input(event);
    true
}

fn apply_input_method(request: &i_slint_core::window::InputMethodRequest) {
    use i_slint_core::window::InputMethodRequest;
    match request {
        InputMethodRequest::Enable(props) => {
            eprintln!("phase0: soft keyboard show");
            show_soft_keyboard(true, Some(props));
        }
        InputMethodRequest::Update(props) => {
            show_soft_keyboard(true, Some(props));
        }
        InputMethodRequest::Disable => {
            eprintln!("phase0: soft keyboard hide");
            show_soft_keyboard(false, None);
        }
        _ => {}
    }
}

fn show_soft_keyboard(show: bool, props: Option<&i_slint_core::window::InputMethodProperties>) {
    #[cfg(target_os = "android")]
    {
        let Some(app) = bevy::android::ANDROID_APP.get() else {
            eprintln!("phase0: soft keyboard requested and ANDROID_APP is missing");
            return;
        };
        if show {
            // The first EditorInfo parity8 delivered was inputType NULL, so
            // the IME never treated the field as text (`mInputShown` stayed
            // false). Set TYPE_CLASS_TEXT before show. `GameActivity_showSoftInput`
            // posts the show onto the UI thread itself (`mainWorkCallback`
            // in that log); a second JNI hop is not required.
            use bevy::android::android_activity::input::{
                ImeOptions, InputType, TextInputAction, TextInputState, TextSpan,
            };
            app.set_ime_editor_info(InputType::TYPE_CLASS_TEXT, TextInputAction::Done, ImeOptions::empty());
            let state = if let Some(props) = props {
                let mut text = props.text.to_string();
                if !props.preedit_text.is_empty() {
                    text.insert_str(props.preedit_offset, props.preedit_text.as_str());
                }
                TextInputState {
                    text,
                    selection: TextSpan {
                        start: props.anchor_position.unwrap_or(props.cursor_position),
                        end: props.cursor_position,
                    },
                    compose_region: (!props.preedit_text.is_empty()).then_some(TextSpan {
                        start: props.preedit_offset,
                        end: props.preedit_offset + props.preedit_text.len(),
                    }),
                }
            } else {
                TextInputState {
                    text: String::new(),
                    selection: TextSpan { start: 0, end: 0 },
                    compose_region: None,
                }
            };
            // Gives GameTextInput a buffer before the first `getTextInputState`.
            app.set_text_input_state(state);
            // SHOW_IMPLICIT, matching winit's `set_ime_allowed(true)`.
            app.show_soft_input(true);
            eprintln!("phase0: soft keyboard showSoftInput inputType=TYPE_CLASS_TEXT");
        } else {
            // HIDE_IMPLICIT_ONLY, matching the show flag above.
            app.hide_soft_input(true);
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (show, props);
    }
}

/// Tell Slint the panel size, allocate a matching texture, and scale the quad
/// so the mesh aspect matches. The Skia renderer is the one already associated
/// with the window; this only replaces the texture it draws into.
fn publish_overlay(
    width: u32,
    height: u32,
    scale: f32,
    ctx: Option<&SlintContext>,
    shared: &SlintSharedTexture,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    overlay: &mut SlintImageHandle,
    epoch: &mut OverlayEpoch,
    quads: &Query<&MeshMaterial3d<StandardMaterial>, With<SlintQuad>>,
) {
    if let Some(ctx) = ctx {
        apply_slint_size(&ctx.adapter, width, height, scale);
    }
    let image_handle = images.add(overlay_image(width, height));
    let mat_handle = quads.iter().next().map(|mat| mat.0.clone());
    if let Some(mat_handle) = mat_handle {
        if let Some(mut material) = materials.get_mut(&mat_handle) {
            material.base_color_texture = Some(image_handle.clone());
        }
    }
    overlay.0 = image_handle;
    epoch.0 += 1;
    while shared
        .receiver
        .lock()
        .expect("texture receiver")
        .try_recv()
        .is_ok()
    {}
    *shared.texture.lock().expect("texture slot") = None;
    eprintln!("phase0: slint overlay texture {width}x{height} scale {scale:.3} scheduled");
}

fn fit_overlay(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut quads: Query<&mut Transform, With<SlintQuad>>,
    mut cameras: Query<&mut Projection, With<OverlayCamera>>,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };
    let scale = window.resolution.scale_factor().max(0.01);
    let width = window.resolution.physical_width() as f32 / scale;
    let height = window.resolution.physical_height() as f32 / scale;
    if width < 1.0 || height < 1.0 {
        return;
    }
    for mut transform in &mut quads {
        transform.translation = Vec3::ZERO;
        transform.scale = Vec3::new(width, height, 1.0);
    }
    for mut projection in &mut cameras {
        *projection = Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed { width, height },
            ..OrthographicProjection::default_3d()
        });
    }
}

fn gate_gallery_camera(
    avatars: Query<&Camera, With<eustress_common::avatar::control::AvatarCamera>>,
    scripted: Query<&Camera, With<eustress_common::play_session::ScriptedPlayCamera>>,
    mut cameras: Query<&mut Camera, (With<GalleryCamera>, Without<eustress_common::avatar::control::AvatarCamera>, Without<eustress_common::play_session::ScriptedPlayCamera>)>,
) {
    let live = avatars.iter().any(|camera| camera.is_active) || scripted.iter().any(|camera| camera.is_active);
    for mut camera in &mut cameras {
        camera.is_active = !live;
    }
}

/// `Suspended` holds the swapchain and drops the overlay texture slot. The
/// Skia renderer stays: it is already associated with the Slint window, and
/// the wgpu device is still the one it was created with. `Running` clears the
/// hold so Bevy builds a new surface and [`refresh_overlay`] allocates a new
/// texture for that same renderer.
fn sync_surface_loss(
    mut events: MessageReader<AppLifecycle>,
    mut loss: ResMut<SurfaceLoss>,
    mut hold: ResMut<PlayerSwapchainHold>,
    adapter: Option<Res<RenderAdapter>>,
    shared: Res<SlintSharedTexture>,
    mut world_input: Option<ResMut<eustress_common::input_actions::InputState>>,
    mut ambient: Option<ResMut<GlobalAmbientLight>>,
    units: Option<Res<eustress_common::plugins::light_classes::LightUnits>>,
    lights: Query<(&DirectionalLight, &GlobalTransform, Option<&Name>)>,
    cameras: Query<(&Camera, Option<&bevy::camera::Exposure>, Option<&Tonemapping>, Option<&Name>, &GlobalTransform)>,
) {
    let mut saw_suspended = false;
    let mut saw_running = false;
    for event in events.read() {
        match event {
            AppLifecycle::Suspended | AppLifecycle::WillSuspend => saw_suspended = true,
            AppLifecycle::Running | AppLifecycle::WillResume => saw_running = true,
            _ => {}
        }
    }

    if saw_suspended {
        *shared.texture.lock().expect("texture slot") = None;
        loss.suspended = true;
        hold.0 = true;
        if let Some(input) = world_input.as_mut() {
            input.release_touch();
        }
        let gles = adapter
            .as_ref()
            .map(|adapter| adapter.0.get_info().backend == wgpu::Backend::Gl)
            .unwrap_or(false);
        eprintln!(
            "phase0: surface lost (Suspended). swapchain hold on. skia renderer kept. gles={gles}"
        );
    }

    if saw_running && loss.suspended {
        loss.suspended = false;
        hold.0 = false;
        loss.pending_overlay = true;
        let gles = adapter
            .as_ref()
            .map(|adapter| adapter.0.get_info().backend == wgpu::Backend::Gl)
            .unwrap_or(false);
        eprintln!(
            "phase0: resume (Running). new surface on the same device. skia renderer kept. gles={gles}"
        );
        log_scene_lighting("resume", ambient.as_deref(), units.as_deref(), &lights, &cameras);
        if let Some(ambient) = ambient.as_deref_mut() {
            if !ambient.brightness.is_finite() || ambient.brightness < 1.0 {
                eprintln!(
                    "phase0: resume ambient brightness {:.1} is gone; restoring 280",
                    ambient.brightness
                );
                ambient.brightness = 280.0;
                ambient.color = Color::WHITE;
            }
        }
    }
}

/// One line after the play lights exist, so a resume log has something to
/// compare with. Suspend does not remove these components; if the picture
/// goes dark and these numbers did not move, the loss is in the surface or
/// the shadow maps rather than the ECS light.
fn log_lighting_baseline(
    mut logged: Local<bool>,
    ambient: Option<Res<GlobalAmbientLight>>,
    units: Option<Res<eustress_common::plugins::light_classes::LightUnits>>,
    lights: Query<(&DirectionalLight, &GlobalTransform, Option<&Name>)>,
    cameras: Query<(&Camera, Option<&bevy::camera::Exposure>, Option<&Tonemapping>, Option<&Name>, &GlobalTransform)>,
) {
    if *logged || lights.is_empty() {
        return;
    }
    if !cameras.iter().any(|(camera, _, _, _, _)| camera.is_active) {
        return;
    }
    *logged = true;
    log_scene_lighting("open", ambient.as_deref(), units.as_deref(), &lights, &cameras);
}

fn log_scene_lighting(
    tag: &str,
    ambient: Option<&GlobalAmbientLight>,
    units: Option<&eustress_common::plugins::light_classes::LightUnits>,
    lights: &Query<(&DirectionalLight, &GlobalTransform, Option<&Name>)>,
    cameras: &Query<(&Camera, Option<&bevy::camera::Exposure>, Option<&Tonemapping>, Option<&Name>, &GlobalTransform)>,
) {
    let brightness = ambient.map(|light| light.brightness).unwrap_or(f32::NAN);
    let ev = units.map(|units| units.ev100).unwrap_or(f32::NAN);
    eprintln!("phase0: lighting {tag} ambient={brightness:.1} light_units_ev={ev:.2}");
    for (light, transform, name) in lights.iter() {
        let at = transform.translation();
        eprintln!(
            "phase0: lighting {tag} directional {} illuminance={:.0} shadows={} at=({:.1}, {:.1}, {:.1})",
            name.map(|n| n.as_str()).unwrap_or("unnamed"),
            light.illuminance,
            light.shadow_maps_enabled,
            at.x,
            at.y,
            at.z,
        );
    }
    for (camera, exposure, tonemap, name, transform) in cameras.iter() {
        if !camera.is_active {
            continue;
        }
        let at = transform.translation();
        eprintln!(
            "phase0: lighting {tag} camera {} order={} exposure_ev100={:.2} tonemap={} at=({:.1}, {:.1}, {:.1})",
            name.map(|n| n.as_str()).unwrap_or("unnamed"),
            camera.order,
            exposure.map(|e| e.ev100).unwrap_or(f32::NAN),
            tonemap.map(|tone| format!("{tone:?}")).unwrap_or_else(|| "none".into()),
            at.x,
            at.y,
            at.z,
        );
    }
}

fn refresh_overlay(
    mut loss: ResMut<SurfaceLoss>,
    hold: Res<PlayerSwapchainHold>,
    mut size: ResMut<OverlaySize>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    ctx: Option<NonSend<SlintContext>>,
    shared: Res<SlintSharedTexture>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut overlay: Option<ResMut<SlintImageHandle>>,
    mut epoch: Option<ResMut<OverlayEpoch>>,
    quads: Query<&MeshMaterial3d<StandardMaterial>, With<SlintQuad>>,
) {
    if hold.0 {
        return;
    }
    let Some((entity, window)) = windows.iter().next() else {
        return;
    };
    let Some((width, height, scale)) = overlay_pixels(entity, window) else {
        return;
    };
    let size_changed = size.width != width || size.height != height;
    if !loss.pending_overlay && !size_changed {
        return;
    }
    let Some(overlay) = overlay.as_mut() else {
        return;
    };
    let Some(epoch) = epoch.as_mut() else {
        return;
    };
    loss.pending_overlay = false;
    size.width = width;
    size.height = height;
    let reason = if size_changed {
        "window size"
    } else {
        "resume"
    };
    eprintln!("phase0: overlay refresh ({reason})");
    publish_overlay(
        width,
        height,
        scale,
        ctx.as_deref(),
        &shared,
        &mut images,
        &mut materials,
        overlay,
        epoch,
        &quads,
    );
}

/// Runs in the render world, before `create_surfaces`. Drops the swapchain
/// once when the hold comes on, and publishes [`SwapchainHold`] so the patched
/// prepare/create systems do not touch a dead `ANativeWindow`.
fn drop_held_swapchain(world: &mut World) {
    let hold = world
        .get_resource::<PlayerSwapchainHold>()
        .map(|hold| hold.0)
        .unwrap_or(false);
    let current = world
        .get_resource::<bevy::render::view::window::SwapchainHold>()
        .map(|hold| hold.0);
    if hold && current != Some(true) {
        world.remove_resource::<bevy::render::view::window::WindowSurfaces>();
        world.init_resource::<bevy::render::view::window::WindowSurfaces>();
        eprintln!("phase0: dropped wgpu swapchain (Suspended / surface loss)");
    }
    world.insert_resource(bevy::render::view::window::SwapchainHold(hold));
}

#[cfg(target_os = "android")]
fn primary_window_handle(world: &World) -> Option<RawHandleWrapper> {
    world.iter_entities().find_map(|entity| {
        if entity.contains::<PrimaryWindow>() {
            entity.get::<RawHandleWrapper>().cloned()
        } else {
            None
        }
    })
}

/// Flags for the Vulkan instance on Android.
///
/// A debug build's default is `DEBUG | VALIDATION | VALIDATION_INDIRECT_CALL`.
/// `Device::new` then builds the indirect-validation bind-group layout and
/// names it. The emulator's ranchu driver (`vulkan.ranchu.so` on API 36,
/// `-gpu host`) SIGSEGVs in `vkSetDebugUtilsObjectNameEXT` on that call
/// (`fault addr 0x0`), about a second after launch, after the log line
/// `adapter via with-surface: Vulkan …`. Phase 0's APKs were release-profile
/// native code, so Bevy had already stripped `VALIDATION_INDIRECT_CALL` and
/// `from_build_config` had not turned `DEBUG` on; the same emulator stayed
/// up. The device-creation calls themselves are unchanged. Android keeps
/// those release-shaped flags in a debug APK. `WGPU_DEBUG` / `WGPU_VALIDATION`
/// cannot turn them back on here.
#[cfg(target_os = "android")]
fn android_instance_flags(mut flags: wgpu::InstanceFlags) -> wgpu::InstanceFlags {
    flags.remove(wgpu::InstanceFlags::DEBUG);
    flags.remove(wgpu::InstanceFlags::VALIDATION);
    flags.remove(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL);
    flags.remove(wgpu::InstanceFlags::GPU_BASED_VALIDATION);
    flags
}

#[cfg(target_os = "android")]
fn make_instance(settings: &WgpuSettings, backends: wgpu::Backends) -> wgpu::Instance {
    let flags = android_instance_flags(settings.instance_flags);
    eprintln!("phase0: instance flags {backends:?} {flags:?}");
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        flags,
        memory_budget_thresholds: settings.instance_memory_budget_thresholds,
        backend_options: wgpu::BackendOptions {
            gl: wgpu::GlBackendOptions {
                gles_minor_version: settings.gles3_minor_version,
                fence_behavior: wgpu::GlFenceBehavior::Normal,
                debug_fns: wgpu::GlDebugFns::Auto,
            },
            ..default()
        },
        display: None,
    })
}

#[cfg(target_os = "android")]
fn log_enumerated(
    instance: &wgpu::Instance,
    backends: wgpu::Backends,
    surface: Option<&wgpu::Surface>,
) {
    let adapters = spin_on::spin_on(instance.enumerate_adapters(backends));
    eprintln!(
        "phase0: enumerate {backends:?} -> {} adapter(s)",
        adapters.len()
    );
    for adapter in &adapters {
        let info = adapter.get_info();
        let surface_ok = surface
            .map(|surface| adapter.is_surface_supported(surface))
            .unwrap_or(true);
        eprintln!(
            "phase0:   {:?} {} {:?} surface_ok={surface_ok}",
            info.backend, info.name, info.device_type
        );
    }
}

#[cfg(target_os = "android")]
fn request_one(
    instance: &wgpu::Instance,
    settings: &WgpuSettings,
    surface: Option<&wgpu::Surface>,
    label: &str,
) -> Result<wgpu::Adapter, String> {
    let options = wgpu::RequestAdapterOptions {
        power_preference: settings.power_preference,
        force_fallback_adapter: std::env::var("WGPU_FORCE_FALLBACK_ADAPTER")
            .map_or(settings.force_fallback_adapter, |value| {
                !(value.is_empty() || value == "0" || value == "false")
            }),
        compatible_surface: surface,
    };
    match spin_on::spin_on(instance.request_adapter(&options)) {
        Ok(adapter) => {
            let info = adapter.get_info();
            eprintln!(
                "phase0: adapter via {label}: {:?} {} ({:?})",
                info.backend, info.name, info.device_type
            );
            Ok(adapter)
        }
        Err(err) => {
            eprintln!("phase0: no adapter via {label}: {err}");
            Err(format!("{label}: {err}"))
        }
    }
}

#[cfg(target_os = "android")]
fn bevy_device_parts(
    adapter: &wgpu::Adapter,
    settings: &WgpuSettings,
) -> (wgpu::Features, wgpu::Limits) {
    let info = adapter.get_info();
    let mut features = wgpu::Features::empty();
    let mut limits = settings.limits.clone();
    if matches!(settings.priority, WgpuSettingsPriority::Functionality) {
        features = adapter.features();
        if info.device_type == wgpu::DeviceType::DiscreteGpu {
            features.remove(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
        }
        limits = adapter.limits();
    }
    if let Some(disabled) = settings.disabled_features {
        features.remove(disabled);
    }
    features |= settings.features;
    if let Some(constrained) = settings.constrained_limits.as_ref() {
        limits = limits.or_worse_values_from(constrained);
    }
    (features, limits)
}

#[cfg(target_os = "android")]
fn request_device_pair(
    adapter: &wgpu::Adapter,
    settings: &WgpuSettings,
) -> Result<(wgpu::Device, wgpu::Queue), String> {
    let descriptor = |features: wgpu::Features, limits: wgpu::Limits| wgpu::DeviceDescriptor {
        label: settings.device_label.as_ref().map(|label| label.as_ref()),
        required_features: features,
        required_limits: limits,
        // Same call Bevy's initialize_renderer makes.
        experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
        memory_hints: settings.memory_hints.clone(),
        trace: wgpu::Trace::Off,
    };
    let (features, limits) = bevy_device_parts(adapter, settings);
    match spin_on::spin_on(adapter.request_device(&descriptor(features, limits))) {
        Ok(pair) => Ok(pair),
        Err(err) => {
            eprintln!("phase0: request_device failed ({err:?}); retrying with adapter limits only");
            let features = settings.features.intersection(adapter.features());
            let limits = adapter.limits();
            spin_on::spin_on(adapter.request_device(&descriptor(features, limits)))
                .map_err(|err| format!("request_device: {err:?}"))
        }
    }
}

#[cfg(target_os = "android")]
fn manual_creation(
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
) -> RenderCreation {
    let render_device: RenderDevice = device.into();
    let render_queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let render_adapter_info = RenderAdapterInfo(WgpuWrapper::new(adapter.get_info()));
    let render_adapter = RenderAdapter(Arc::new(WgpuWrapper::new(adapter)));
    let render_instance = RenderInstance(Arc::new(WgpuWrapper::new(instance)));
    RenderCreation::manual(
        render_device,
        render_queue,
        render_adapter_info,
        render_adapter,
        render_instance,
    )
}

#[cfg(target_os = "android")]
fn try_instance(
    wrapper: &RawHandleWrapper,
    settings: &WgpuSettings,
    backends: wgpu::Backends,
    errors: &mut Vec<String>,
) -> Option<RenderCreation> {
    let instance = make_instance(settings, backends);
    // SAFETY: called on the main thread while the GameActivity window is live.
    let surface = match instance.create_surface(unsafe { wrapper.get_handle() }) {
        Ok(surface) => Some(surface),
        Err(err) => {
            let message = format!("create_surface {backends:?}: {err}");
            eprintln!("phase0: {message}");
            errors.push(message);
            None
        }
    };
    log_enumerated(&instance, backends, surface.as_ref());
    let adapter = match request_one(&instance, settings, surface.as_ref(), "with-surface") {
        Ok(adapter) => adapter,
        Err(err) => {
            errors.push(err);
            match request_one(&instance, settings, None, "no-surface") {
                Ok(adapter) => adapter,
                Err(err) => {
                    errors.push(err);
                    return None;
                }
            }
        }
    };
    // The selection surface is not the swapchain. Bevy creates that later
    // from the same window handle. Dropping this one releases the GLES
    // window binding before `request_device`.
    drop(surface);
    match request_device_pair(&adapter, settings) {
        Ok((device, queue)) => Some(manual_creation(instance, adapter, device, queue)),
        Err(err) => {
            eprintln!("phase0: {err}");
            errors.push(err);
            None
        }
    }
}

#[cfg(target_os = "android")]
fn probe_hal_gles(settings: &WgpuSettings) -> String {
    let desc = wgpu::hal::InstanceDescriptor {
        name: "phase0-gles-probe",
        flags: android_instance_flags(settings.instance_flags),
        memory_budget_thresholds: settings.instance_memory_budget_thresholds,
        backend_options: wgpu::BackendOptions {
            gl: wgpu::GlBackendOptions {
                gles_minor_version: settings.gles3_minor_version,
                fence_behavior: wgpu::GlFenceBehavior::Normal,
                debug_fns: wgpu::GlDebugFns::Auto,
            },
            ..default()
        },
        telemetry: None,
        display: None,
    };
    // SAFETY: probe only. The instance is dropped before wgpu creates its own.
    match unsafe { <wgpu::hal::gles::Instance as wgpu::hal::Instance>::init(&desc) } {
        Ok(_) => "hal GLES instance created".to_string(),
        Err(err) => {
            let mut message = format!("hal GLES instance failed: {err}");
            let mut source = std::error::Error::source(&err);
            while let Some(next) = source {
                message.push_str(": ");
                message.push_str(&next.to_string());
                source = next.source();
            }
            message
        }
    }
}

/// Asks EGL whether a GLES 3 context exists at all, then tears that context
/// down so wgpu can create its own. Does not call `eglTerminate`.
#[cfg(target_os = "android")]
fn probe_egl(wrapper: &RawHandleWrapper) {
    use std::ffi::{c_char, c_void, CStr};
    use std::os::raw::c_int;

    const EGL_NONE: i32 = 0x3038;
    const EGL_WIDTH: i32 = 0x3057;
    const EGL_HEIGHT: i32 = 0x3056;
    const EGL_VENDOR: i32 = 0x3053;
    const EGL_VERSION: i32 = 0x3054;
    const EGL_EXTENSIONS: i32 = 0x3055;
    const EGL_SURFACE_TYPE: i32 = 0x3033;
    const EGL_PBUFFER_BIT: i32 = 0x0001;
    const EGL_RENDERABLE_TYPE: i32 = 0x3040;
    const EGL_OPENGL_ES2_BIT: i32 = 0x0004;
    const EGL_OPENGL_ES3_BIT: i32 = 0x0040;
    const EGL_CONTEXT_MAJOR_VERSION: i32 = 0x3098;
    const EGL_CONTEXT_MINOR_VERSION: i32 = 0x30FB;
    const EGL_OPENGL_ES_API: u32 = 0x30A0;
    const GL_RENDERER: u32 = 0x1F01;
    const GL_VERSION: u32 = 0x1F02;

    #[link(name = "EGL")]
    unsafe extern "C" {
        fn eglGetDisplay(display_id: *mut c_void) -> *mut c_void;
        fn eglInitialize(display: *mut c_void, major: *mut c_int, minor: *mut c_int) -> u32;
        fn eglGetError() -> i32;
        fn eglQueryString(display: *mut c_void, name: i32) -> *const c_char;
        fn eglBindAPI(api: u32) -> u32;
        fn eglChooseConfig(
            display: *mut c_void,
            attribs: *const i32,
            configs: *mut *mut c_void,
            config_size: i32,
            num_config: *mut i32,
        ) -> u32;
        fn eglCreateContext(
            display: *mut c_void,
            config: *mut c_void,
            share: *mut c_void,
            attribs: *const i32,
        ) -> *mut c_void;
        fn eglCreatePbufferSurface(
            display: *mut c_void,
            config: *mut c_void,
            attribs: *const i32,
        ) -> *mut c_void;
        fn eglMakeCurrent(
            display: *mut c_void,
            draw: *mut c_void,
            read: *mut c_void,
            context: *mut c_void,
        ) -> u32;
        fn eglDestroyContext(display: *mut c_void, context: *mut c_void) -> u32;
        fn eglDestroySurface(display: *mut c_void, surface: *mut c_void) -> u32;
    }

    #[link(name = "GLESv2")]
    unsafe extern "C" {
        fn glGetString(name: u32) -> *const c_char;
    }

    fn cstr<'a>(ptr: *const c_char) -> &'a str {
        if ptr.is_null() {
            return "";
        }
        unsafe { CStr::from_ptr(ptr) }.to_str().unwrap_or("")
    }

    unsafe {
        let display = eglGetDisplay(std::ptr::null_mut());
        if display.is_null() {
            eprintln!(
                "phase0: egl probe: eglGetDisplay failed ({:#x})",
                eglGetError()
            );
            return;
        }
        let mut major = 0;
        let mut minor = 0;
        if eglInitialize(display, &mut major, &mut minor) != 1 {
            eprintln!(
                "phase0: egl probe: eglInitialize failed ({:#x})",
                eglGetError()
            );
            return;
        }
        eprintln!(
            "phase0: egl probe: EGL {}.{} vendor={} version={}",
            major,
            minor,
            cstr(eglQueryString(display, EGL_VENDOR)),
            cstr(eglQueryString(display, EGL_VERSION))
        );
        let extensions = cstr(eglQueryString(display, EGL_EXTENSIONS));
        eprintln!(
            "phase0: egl probe: surfaceless={} colorspace={}",
            extensions.contains("EGL_KHR_surfaceless_context"),
            extensions.contains("EGL_KHR_gl_colorspace")
        );
        if eglBindAPI(EGL_OPENGL_ES_API) != 1 {
            eprintln!(
                "phase0: egl probe: eglBindAPI(GLES) failed ({:#x})",
                eglGetError()
            );
            return;
        }

        if !matches!(
            wrapper.get_window_handle(),
            wgpu::rwh::RawWindowHandle::AndroidNdk(_)
        ) {
            eprintln!(
                "phase0: egl probe: window is not AndroidNdk ({:?})",
                wrapper.get_window_handle()
            );
            return;
        }

        // Pbuffer only. A window surface connects the ANativeWindow, and a
        // later Vulkan or GLES surface then fails with "already connected".
        for (label, surface_bit, renderable) in [
            ("pbuffer-es3", EGL_PBUFFER_BIT, EGL_OPENGL_ES3_BIT),
            ("pbuffer-es2", EGL_PBUFFER_BIT, EGL_OPENGL_ES2_BIT),
        ] {
            let attribs = [
                EGL_SURFACE_TYPE,
                surface_bit,
                EGL_RENDERABLE_TYPE,
                renderable,
                EGL_NONE,
            ];
            let mut config = std::ptr::null_mut();
            let mut count = 0;
            if eglChooseConfig(display, attribs.as_ptr(), &mut config, 1, &mut count) != 1
                || count == 0
                || config.is_null()
            {
                eprintln!(
                    "phase0: egl probe: no config for {label} ({:#x})",
                    eglGetError()
                );
                continue;
            }
            let ctx_attribs = [
                EGL_CONTEXT_MAJOR_VERSION,
                3,
                EGL_CONTEXT_MINOR_VERSION,
                0,
                EGL_NONE,
            ];
            let context =
                eglCreateContext(display, config, std::ptr::null_mut(), ctx_attribs.as_ptr());
            if context.is_null() {
                eprintln!(
                    "phase0: egl probe: eglCreateContext {label} failed ({:#x})",
                    eglGetError()
                );
                continue;
            }
            let draw = eglCreatePbufferSurface(
                display,
                config,
                [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE].as_ptr(),
            );
            if draw.is_null() {
                eprintln!(
                    "phase0: egl probe: surface {label} failed ({:#x})",
                    eglGetError()
                );
                eglDestroyContext(display, context);
                continue;
            }
            let current = eglMakeCurrent(display, draw, draw, context);
            if current == 1 {
                eprintln!(
                    "phase0: egl probe: {label} current renderer={} version={}",
                    cstr(glGetString(GL_RENDERER)),
                    cstr(glGetString(GL_VERSION))
                );
            } else {
                eprintln!(
                    "phase0: egl probe: eglMakeCurrent {label} failed ({:#x})",
                    eglGetError()
                );
            }
            eglMakeCurrent(
                display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            eglDestroySurface(display, draw);
            eglDestroyContext(display, context);
            if current == 1 {
                return;
            }
        }
    }
}

/// Instance, surface, and device. The window handle is already live.
#[cfg(target_os = "android")]
fn create_android_gpu(world: &World, settings: &WgpuSettings) -> Result<RenderCreation, String> {
    let wrapper = primary_window_handle(world)
        .ok_or_else(|| "primary window has no RawHandleWrapper".to_string())?;
    probe_egl(&wrapper);

    let backends = settings
        .backends
        .unwrap_or(wgpu::Backends::VULKAN | wgpu::Backends::GL);
    eprintln!("phase0: creating wgpu instance, surface, and device (backends {backends:?})");

    // Survey adapters before touching the window. `create_surface` on a
    // Vulkan+GLES instance connects the ANativeWindow to Vulkan. GLES
    // `eglCreateWindowSurface` then fails with "already connected to another
    // API". The instance Bevy keeps is a single backend.
    let survey = make_instance(settings, backends);
    log_enumerated(&survey, backends, None);
    let found: Vec<wgpu::Backend> = spin_on::spin_on(survey.enumerate_adapters(backends))
        .iter()
        .map(|adapter| adapter.get_info().backend)
        .collect();
    drop(survey);

    let mut order = Vec::new();
    if backends.contains(wgpu::Backends::VULKAN) && found.contains(&wgpu::Backend::Vulkan) {
        order.push(wgpu::Backends::VULKAN);
    }
    if backends.contains(wgpu::Backends::GL)
        && (found.contains(&wgpu::Backend::Gl) || order.is_empty())
    {
        order.push(wgpu::Backends::GL);
    }
    if order.is_empty() {
        order.push(backends);
    }

    let mut errors = Vec::new();
    for backend in order {
        eprintln!("phase0: single-backend instance {backend:?}");
        if let Some(creation) = try_instance(&wrapper, settings, backend, &mut errors) {
            return Ok(creation);
        }
    }
    let probe = probe_hal_gles(settings);
    eprintln!("phase0: {probe}");
    errors.push(probe);
    Err(errors.join(" | "))
}

/// Delays `RenderPlugin` until the primary window has a real handle.
///
/// `ready` stays false on Android until winit's `Resumed` (GameActivity
/// `onSurfaceCreated`). `finish` then adds the renderer. The patched
/// `bevy_app` finish loop finishes those plugins in the same pass, so the
/// device is unpacked on the main thread with the holder populated.
struct SurfaceGate {
    settings: WgpuSettings,
    texture_tx: std::sync::mpsc::Sender<wgpu::Texture>,
    device_tx: std::sync::mpsc::Sender<WgpuHandles>,
    /// Host tests have no display. `finish` still runs for every other plugin;
    /// this one does not create a device.
    headless: bool,
}

impl Plugin for SurfaceGate {
    fn build(&self, _app: &mut App) {}

    fn ready(&self, app: &App) -> bool {
        #[cfg(target_os = "android")]
        {
            let ready = app.world().iter_entities().any(|entity| {
                entity.contains::<PrimaryWindow>() && entity.contains::<RawHandleWrapper>()
            });
            if ready {
                static LOGGED: AtomicBool = AtomicBool::new(false);
                if !LOGGED.swap(true, Ordering::Relaxed) {
                    eprintln!(
                        "phase0: window handle is live (Resumed / onSurfaceCreated), wgpu init can start"
                    );
                }
            }
            ready
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = app;
            true
        }
    }

    fn finish(&self, app: &mut App) {
        if self.headless || app.is_plugin_added::<RenderPlugin>() {
            return;
        }
        #[cfg(target_os = "android")]
        let render_creation = match create_android_gpu(app.world(), &self.settings) {
            Ok(creation) => creation,
            Err(err) => panic!("phase0: wgpu init failed after onSurfaceCreated: {err}"),
        };
        #[cfg(not(target_os = "android"))]
        let render_creation = RenderCreation::Automatic(Box::new(self.settings.clone()));
        app.add_plugins((
            RenderPlugin {
                render_creation,
                ..default()
            },
            bevy::image::ImagePlugin::default(),
            bevy::mesh::MeshPlugin,
            bevy::camera::CameraPlugin,
            bevy::light::LightPlugin,
            bevy::core_pipeline::CorePipelinePlugin,
            bevy::pbr::PbrPlugin::default(),
        ));
        // Terrain, water, the far field, and the celestial materials queued
        // their `MaterialPlugin`s while RenderApp was missing. This drains
        // that queue while plugins can still be added.
        app.add_plugins(eustress_common::render_later::InstallDeferredRenderPlugin);
        // Extract plugins no-op if they are added before `RenderApp` exists.
        app.add_plugins((
            ExtractResourcePlugin::<SlintImageHandle>::default(),
            ExtractResourcePlugin::<OverlayEpoch>::default(),
            ExtractResourcePlugin::<PlayerSwapchainHold>::default(),
        ));
        let render_app = app.sub_app_mut(RenderApp);
        render_app.insert_resource(TextureSender(self.texture_tx.clone()));
        render_app.insert_resource(DeviceSender(self.device_tx.clone()));
        render_app.add_systems(
            Render,
            drop_held_swapchain.before(bevy::render::view::window::create_surfaces),
        );
        render_app.add_systems(Render, (publish_shared_device, send_slint_texture).chain());
        eprintln!("phase0: render plugins added; their finish unpacks the device");
    }
}

/// Android GameActivity entry, and the desktop body used by `cargo check`.
#[bevy_main]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(err) = env::install(&args) {
        eprintln!("eustress player: {err}");
    }
    #[cfg(all(target_os = "android", feature = "android"))]
    {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("EustressPlayer"),
        );
        std::panic::set_hook(Box::new(|info| {
            eprintln!("phase0 panic: {info}");
            log::error!("phase0 panic: {info}");
        }));
        eprintln!("eustress phase0 android_main");
    }
    run_game();
}

pub fn run_game() {
    build_game(false).run();
}

/// The mobile Player's `App`, before `run`.
///
/// `headless` drops `WinitPlugin` so a host test can `update` without a
/// window. `SurfaceGate::finish` then skips the renderer. A test that wants
/// the same plugin `finish`/`cleanup` Android runs before the first frame
/// calls `App::finish` and `App::cleanup` itself.
fn build_game(headless: bool) -> App {
    let (texture_tx, texture_rx) = std::sync::mpsc::channel();
    let (device_tx, device_rx) = std::sync::mpsc::channel();
    // Vulkan is the default. GLES is compiled in only on Android
    // (`bevy_render/gles`; Bevy does not enable it itself) and is selected
    // when Vulkan enumerates nothing, or when `WGPU_BACKEND` overrides this.
    // wgpu ranks a hardware Vulkan adapter ahead of GLES (device type Other).
    // See PHASE0_SPIKE.md.
    let mut settings = WgpuSettings::default();
    settings.backends =
        Some(wgpu::Backends::from_env().unwrap_or(wgpu::Backends::VULKAN | wgpu::Backends::GL));
    // See `android_instance_flags`. Bevy's default keeps debug labels and
    // indirect validation on whenever `debug_assertions` is set, which is
    // this debug APK. The instance is created from these settings.
    #[cfg(target_os = "android")]
    {
        settings.instance_flags = android_instance_flags(settings.instance_flags);
    }

    let mut app = App::new();
    // A missing `Res` / `ResMut` is a system-param validation error. Bevy's
    // default handler panics, which is how parity6 died on the first frame.
    // On Android, log it and skip that system. A `panic!` in a system body
    // is still a panic: the executor resumes the unwind after the handler.
    // Host tests keep the panic handler so the param sweep fails the build.
    #[cfg(target_os = "android")]
    {
        app.set_error_handler(bevy::ecs::error::warn);
    }
    // Before AssetPlugin freezes the source table. On Android the character
    // GLBs are copied out of the APK first; a host build finds the checkout.
    // The self-check logs each Bridge signature. A type mismatch returns
    // instead of the CheckJNI abort that killed parity3 at installBundledAssets.
    #[cfg(target_os = "android")]
    eustress_bridge_android::startup_self_check();
    if let Some(root) = platform::install_bundled_assets() {
        std::env::set_var("EUSTRESS_BUNDLED_ASSETS", &root);
        let character = root.join("characters/y_bot.glb");
        eprintln!(
            "eustress player: bundled assets at {} y_bot={} exists={}",
            root.display(),
            character.display(),
            character.is_file(),
        );
        // Android `platform_default` ignores this path and reads the APK
        // AssetManager (`NotFound("characters/y_bot.glb")` while the file is
        // in files/bundled). A filesystem reader rooted at the absolute
        // folder is what desktop uses for `bundled://`.
        let bundled = root.clone();
        app.register_asset_source(
            "bundled",
            bevy::asset::io::AssetSourceBuilder::new(move || -> Box<dyn bevy::asset::io::ErasedAssetReader> {
                Box::new(eustress_play_runtime::space_paths::ContainedFileReader::new(bundled.clone()))
            }),
        );
        eustress_common::avatar::boot::note_asset_sources_registered_externally();
        eprintln!("eustress player: bundled:// reads {}", root.display());
    }
    eustress_common::avatar::boot::register_avatar_asset_sources(&mut app);
    // `space://` is the live folder a downloaded world is written into.
    // The directory has to exist before the first read; the world arrives later.
    {
        use bevy::asset::io::{AssetSourceBuilder, ErasedAssetReader};
        let space_root = space::live_space_root();
        if let Err(err) = std::fs::create_dir_all(&space_root) {
            eprintln!("eustress player: live space folder {}: {err}", space_root.display());
        }
        app.register_asset_source(
            "space",
            AssetSourceBuilder::new(move || -> Box<dyn ErasedAssetReader> {
                Box::new(eustress_play_runtime::space_paths::ContainedFileReader::new(space_root.clone()))
            }),
        );
    }
    let mut defaults = DefaultPlugins
        .build()
        .disable::<RenderPlugin>()
        .disable::<bevy::image::ImagePlugin>()
        .disable::<bevy::mesh::MeshPlugin>()
        .disable::<bevy::camera::CameraPlugin>()
        .disable::<bevy::light::LightPlugin>()
        .disable::<bevy::core_pipeline::CorePipelinePlugin>()
        .disable::<bevy::pbr::PbrPlugin>();
    // `android_logger::init_once` already installed the process logger.
    // LogPlugin tries again and errors "Could not set global logger".
    #[cfg(target_os = "android")]
    {
        defaults = defaults.disable::<bevy::log::LogPlugin>();
    }
    if headless {
        defaults = defaults.disable::<bevy::winit::WinitPlugin>();
    }
    // `eustress-common` turns on `bevy_gizmos`, so DefaultPlugins includes
    // `GizmoPlugin`. Its skinned-mesh system requires `Assets<Mesh>`, which
    // mobile does not insert until SurfaceGate. The Player does not draw
    // editor gizmos. `LightPlugin` (added later, from `SurfaceGate`) still
    // registers `LightGizmoPlugin` while this feature is on. That plugin's
    // mesh system needs `Assets<GizmoAsset>`, which only `GizmoPlugin`
    // inserts. The patched `bevy_light` skips `LightGizmoPlugin` when this
    // variable is set, so those systems are never scheduled.
    std::env::set_var("EUSTRESS_DISABLE_LIGHT_GIZMOS", "1");
    defaults = defaults.disable::<bevy::gizmos::GizmoPlugin>();
    app.add_plugins(defaults)
    .add_plugins(avian3d::prelude::PhysicsPlugins::default())
    .insert_resource(avian3d::prelude::Gravity(Vec3::NEG_Y * eustress_common::avatar::GRAVITY_MPS2))
    .add_plugins(eustress_common::avatar::AvatarRuntimePlugin::new(
        eustress_common::avatar::AvatarHost::Client,
    ))
    .add_plugins(eustress_networking::NetPlugin)
    .add_plugins(eustress_networking::player_replica::NetReplicaPlugin)
    // Draw the opened tree. RenderApp does not exist yet: billboard and
    // particle shaders wait until `Assets<Shader>` exists, and terrain
    // materials wait in `when_render_app` until SurfaceGate adds the renderer.
    .add_plugins(eustress_play_runtime::PlayRuntimePlugin {
        role: eustress_common::play_session::PlayRole::Replica,
    })
    .add_plugins(eustress_common::data_mesh::DataMeshPlugin)
    .add_plugins(eustress_common::terrain::TerrainPlugin)
    .add_plugins(eustress_common::plugins::light_classes::LightClassSyncPlugin)
    .add_plugins(space::SpaceOpenPlugin)
    .init_resource::<eustress_common::play_session::GuiKeyboardFocus>()
    .init_resource::<session::MobileApp>()
    .add_plugins(SurfaceGate {
        settings: settings.clone(),
        texture_tx,
        device_tx,
        headless,
    })
    .insert_resource(ClearColor(Color::srgb(0.07, 0.09, 0.12)))
    .insert_resource(SlintSharedTexture {
        receiver: Mutex::new(texture_rx),
        texture: Arc::new(Mutex::new(None)),
    })
    .insert_resource(DeviceReceiver(Mutex::new(device_rx)))
    .insert_resource(StoredWgpuSettings(settings))
    .init_resource::<SurfaceLoss>()
    .init_resource::<PlayerSwapchainHold>()
    .insert_resource(OverlaySize {
        width: UI_WIDTH,
        height: UI_HEIGHT,
    })
    .insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 280.0,
        ..default()
    })
    .add_systems(First, sync_native_window_size)
    .add_systems(Startup, (
        install_render_error_handler.run_if(resource_exists::<RenderErrorHandler>),
        setup_scene
            .run_if(resource_exists::<Assets<Image>>)
            .run_if(resource_exists::<Assets<Mesh>>)
            .run_if(resource_exists::<Assets<StandardMaterial>>),
        session::startup,
    ))
    .add_systems(
        Update,
        (
            sync_surface_loss,
            refresh_overlay
                .run_if(resource_exists::<Assets<Image>>)
                .run_if(resource_exists::<Assets<StandardMaterial>>),
            fit_overlay,
            init_slint,
            receive_texture,
            session::drive,
            render_slint,
            pointer_input,
            slint_keyboard,
            gate_gallery_camera,
            spin_cube,
        )
            .chain(),
    )
    .add_systems(Update, log_lighting_baseline);
    eustress_common::avatar::boot::register_gltf_scene_types(&mut app);

    app
}

#[cfg(test)]
mod headless_startup {
    use super::*;

    /// The plugin set `android_main` builds, with RenderApp still deferred,
    /// updated a few times and with no window. A panic here is the failure
    /// the emulator used to find one launch at a time.
    #[test]
    fn mobile_app_updates_without_a_window_while_render_is_deferred() {
        let mut app = build_game(true);
        assert!(
            !app.is_plugin_added::<RenderPlugin>(),
            "RenderPlugin stays deferred until SurfaceGate::finish",
        );
        // Android's runner calls these once the surface exists, before the
        // first `update`. Headless `SurfaceGate` does not create a device.
        app.finish();
        app.cleanup();
        assert!(!app.is_plugin_added::<RenderPlugin>());
        for _ in 0..3 {
            app.update();
        }
        assert!(!app.is_plugin_added::<RenderPlugin>());
    }

    /// `App` is `!Send` (its runner), so it cannot move to another thread.
    /// Bevy's compute pool is the other thread: after a `Send` system
    /// finishes, that worker evaluates `atlas_ready`. parity5 aborted there
    /// because the atlas was `NonSend`. `Assets<Image>` is what `SurfaceGate`
    /// registers before the first frame. Registering it here does not create
    /// a GPU. Font state must still be absent after `build`, because
    /// `Plugin::build` runs on the constructing thread and a `!Send` value
    /// inserted there cannot be read from a later update thread.
    #[test]
    fn frame_schedule_reads_the_billboard_atlas_from_a_pool_thread() {
        let mut app = build_game(true);
        assert!(
            app.world()
                .get_non_send::<eustress_play_runtime::billboards::BillboardTextState>()
                .is_none(),
            "BillboardTextState must be inserted by Startup, not Plugin::build",
        );
        app.finish();
        app.cleanup();
        assert!(!app.is_plugin_added::<RenderPlugin>());
        app.init_asset::<Image>();
        for _ in 0..3 {
            app.update();
        }
        assert!(
            app.world()
                .contains_resource::<eustress_play_runtime::billboards::BillboardAtlas>(),
            "atlas init did not run, so no worker read it",
        );
        assert!(
            app.world()
                .get_non_send::<eustress_play_runtime::billboards::BillboardTextState>()
                .is_some(),
            "Startup did not insert BillboardTextState on the update thread",
        );
        assert!(!app.is_plugin_added::<RenderPlugin>());
    }

    /// Records Bevy system-param validation failures. Anything else still panics.
    fn record_validation_failure(
        err: bevy::ecs::error::BevyError,
        ctx: bevy::ecs::error::ErrorContext,
    ) {
        let message = err.to_string();
        if message.contains("failed validation") {
            let first = message.lines().next().unwrap_or(message.as_str());
            VALIDATION_FAILURES
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(format!("{}: {first}", ctx.name()));
        } else {
            bevy::ecs::error::panic(err, ctx);
        }
    }

    /// After `finish` / `cleanup` and Startup, every system Bevy actually
    /// attempts must pass param validation. The plugins `SurfaceGate` adds
    /// on device are included here except `RenderPlugin` (no display) and
    /// the pipeline plugins whose `build` requires `RenderApp`.
    #[test]
    fn every_scheduled_system_passes_param_validation() {
        let mut app = build_game(true);
        app.add_plugins((
            bevy::image::ImagePlugin::default(),
            bevy::mesh::MeshPlugin,
            bevy::camera::CameraPlugin,
            bevy::light::LightPlugin,
        ));
        app.finish();
        app.cleanup();
        assert!(
            !app.is_plugin_added::<bevy::light::gizmos::LightGizmoPlugin>(),
            "LightGizmoPlugin must stay off when GizmoPlugin is off",
        );
        app.insert_resource(bevy::ecs::error::FallbackErrorHandler(
            record_validation_failure,
        ));
        VALIDATION_FAILURES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        // Startup inserts resources before `Last`, where gizmo meshes update.
        app.update();
        // One fixed step, so `FixedFirst` / `FixedLast` gizmo systems run.
        app.world_mut()
            .resource_mut::<bevy::time::Time<bevy::time::Fixed>>()
            .accumulate_overstep(std::time::Duration::from_secs(1));
        app.update();
        app.update();
        let mut failures = VALIDATION_FAILURES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        failures.sort();
        failures.dedup();
        assert!(
            failures.is_empty(),
            "system param validation failed:\n{}",
            failures.join("\n"),
        );
    }

    #[test]
    fn scaled_default_window_is_not_the_phone() {
        // 1280×720 × (420/160) = 3360×1890, the texture parity7 scheduled first.
        assert!(is_scaled_default_window(3360, 1890, 2.625));
        assert!(!is_scaled_default_window(2400, 1080, 2.625));
        // Scale 1 keeps the real 1280×720 desktop default.
        assert!(!is_scaled_default_window(1280, 720, 1.0));
    }

    #[test]
    fn touch_position_stays_in_logical_pixels() {
        let logical = logical_from_touch(Vec2::new(800.0, 200.0));
        assert_eq!((logical.x, logical.y), (800.0, 200.0));
    }

    /// Press and release on the Sign in button. The Slint callback must run
    /// and the window must ask to redraw. The software renderer needs no GPU.
    #[test]
    fn pointer_on_sign_in_fires_the_callback_and_redraws() {
        use std::rc::Rc;

        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
        use slint::platform::{Platform, WindowAdapter, WindowEvent};

        struct TestPlatform {
            window: Rc<MinimalSoftwareWindow>,
        }

        impl Platform for TestPlatform {
            fn create_window_adapter(
                &self,
            ) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
                Ok(self.window.clone())
            }
        }

        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        slint::platform::set_platform(Box::new(TestPlatform {
            window: window.clone(),
        }))
        .expect("set_platform");
        let ui = shell::PlayerShell::new().expect("PlayerShell");
        shell::wire(&ui);
        ui.show().expect("show");
        ui.window()
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
        window.set_size(slint::PhysicalSize::new(900, 500));
        ui.window()
            .dispatch_event(WindowEvent::WindowActiveChanged(true));
        slint::platform::update_timers_and_animations();
        // A real render subscribes the redraw tracker. An empty callback does
        // not, so a later click would change pixels and never call
        // `request_redraw`.
        let _ = paint_shell(&window);

        let center = sign_in_button_center(&ui);
        let _ = paint_shell(&window);
        ui.window()
            .dispatch_event(WindowEvent::PointerMoved { position: center });
        ui.window().dispatch_event(WindowEvent::PointerPressed {
            position: center,
            button: slint::platform::PointerEventButton::Left,
        });
        // The press paints the button's pressed state. Release may return
        // that property to its old value in the same turn.
        let redraw_on_press = paint_shell(&window);
        ui.window().dispatch_event(WindowEvent::PointerReleased {
            position: center,
            button: slint::platform::PointerEventButton::Left,
        });
        let redraw_on_release = paint_shell(&window);
        let commands = shell::take_commands();
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, session::Command::SignIn)),
            "Sign in at ({}, {}) window {}x{} produced {commands:?}",
            center.x,
            center.y,
            ui.window().size().width,
            ui.window().size().height,
        );
        assert!(
            redraw_on_press || redraw_on_release,
            "the click must request a redraw",
        );

        // Playing hides the gallery and shows the Gallery button. That button
        // is a TouchArea. The middle of the view is not, so a world touch
        // stays with the world.
        ui.set_playing(true);
        slint::platform::update_timers_and_animations();
        let _ = paint_shell(&window);
        let gallery = item_center(&ui, "Gallery");
        assert!(
            slint_chrome_contains(ui.window(), gallery),
            "Gallery at ({}, {}) is a TouchArea",
            gallery.x,
            gallery.y,
        );
        assert!(
            !slint_chrome_contains(ui.window(), slint::LogicalPosition::new(450.0, 250.0)),
            "the play view has no full-screen TouchArea",
        );
    }

    /// Headless: no GPU, so this does not render a frame. It opens a fixture
    /// Space the way a download does and checks the world still has a camera,
    /// a light, and a mesh, and that an avatar was spawned at the pad.
    #[test]
    fn opened_fixture_space_has_a_camera_a_light_and_a_mesh() {
        let mut app = build_game(true);
        app.add_plugins((
            bevy::image::ImagePlugin::default(),
            bevy::mesh::MeshPlugin,
            bevy::camera::CameraPlugin,
            bevy::light::LightPlugin,
        ));
        app.init_asset::<StandardMaterial>();
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f32(1.0 / 60.0),
        ));
        app.finish();
        app.cleanup();
        app.update();

        let part = "\
[metadata]\nclass_name = \"Part\"\nname = \"Block\"\n\n\
[transform]\nposition = [40.0, 2.0, -30.0]\nscale = [4.0, 2.0, 4.0]\n\n\
[properties]\nanchored = true\n";
        let pad = "\
[metadata]\nclass_name = \"SpawnLocation\"\nname = \"Pad\"\n\n\
[transform]\nposition = [40.0, 0.5, -30.0]\nscale = [6.0, 1.0, 6.0]\n\n\
[properties]\nanchored = true\n";
        let camera = "\
[metadata]\nclass_name = \"Camera\"\nname = \"Camera\"\n\n\
[transform]\nposition = [0.0, 20.0, 20.0]\n\n\
[camera]\ncamera_type = \"Custom\"\n";
        let world = eustress_networking::session::DownloadedWorld {
            universe: "Fixture".into(),
            start_space: "Yard".into(),
            spaces: vec![(
                "Yard".into(),
                vec![
                    ("Workspace/_service.toml".into(), b"[service]\nclass_name = \"Workspace\"\n".to_vec()),
                    ("Workspace/Block/_instance.toml".into(), part.as_bytes().to_vec()),
                    ("Workspace/Pad/_instance.toml".into(), pad.as_bytes().to_vec()),
                    ("Workspace/Camera/_instance.toml".into(), camera.as_bytes().to_vec()),
                ],
            )],
            assets: Vec::new(),
            spawn: [40.0, 0.5, -30.0],
        };
        app.world_mut().write_message(eustress_networking::session::WorldDownloaded(std::sync::Arc::new(world)));
        // A published open sets playing before the world arrives. This
        // fixture writes the download directly, so match that: capture_all
        // stays off and the stick is not swallowed as UI.
        {
            let mut mobile = app.world_mut().resource_mut::<session::MobileApp>();
            mobile.playing = true;
            mobile.mode = session::Mode::Playing;
        }
        for _ in 0..8 {
            app.update();
        }

        let status = app.world().resource::<session::MobileApp>().status.clone();
        assert!(status.starts_with("Opened "), "{status}");

        let mesh_count = {
            let mut query = app.world_mut().query_filtered::<Entity, (With<Mesh3d>, With<eustress_common::play_session::FromTree>)>();
            query.iter(app.world()).count()
        };
        assert!(mesh_count >= 1, "opened parts did not spawn a mesh ({status})");

        let light_count = {
            let mut query = app.world_mut().query::<&DirectionalLight>();
            query.iter(app.world()).count()
        };
        assert!(light_count >= 1, "the yard light or a fill light should still be in the world");

        let gallery_kept = {
            let mut query = app.world_mut().query::<&GalleryCamera>();
            query.iter(app.world()).count()
        };
        assert!(gallery_kept >= 1, "removing the yard despawned the world camera");

        let avatar_at = avatar_translation(&mut app).expect("CharacterAutoLoads is on and no avatar spawned");
        let spawn = Vec3::new(40.0, 0.5, -30.0);
        assert!(
            (avatar_at.x - spawn.x).abs() < 2.0 && (avatar_at.z - spawn.z).abs() < 2.0,
            "avatar at {avatar_at} is not on the SpawnLocation {spawn}",
        );

        // The follow camera is spawned the frame after the avatar. Land, so
        // the stick and the jump have a floor.
        let mut settled_y = avatar_at.y;
        let mut stable = 0u32;
        for _ in 0..180 {
            app.update();
            let y = avatar_translation(&mut app).expect("avatar despawned").y;
            if (y - settled_y).abs() < 0.002 {
                stable += 1;
            } else {
                stable = 0;
            }
            settled_y = y;
            if stable >= 8 && follow_is_the_view(&mut app) {
                break;
            }
        }
        let (follow_active, scripted_active, gallery_active, target) = camera_flags(&mut app);
        assert!(follow_active, "the follow camera is not the active view");
        assert!(!scripted_active, "the Space camera stayed active after the avatar spawned");
        assert!(!gallery_active, "the gallery camera is covering the follow camera");
        let avatar_entity = {
            let mut query = app
                .world_mut()
                .query_filtered::<Entity, With<eustress_common::avatar::SpawnedByAvatarRuntime>>();
            query.iter(app.world()).next().expect("avatar")
        };
        assert_eq!(target, Some(avatar_entity), "the follow camera is not aimed at the avatar");

        let mut body_meshes = 0usize;
        for _ in 0..180 {
            app.update();
            body_meshes = avatar_mesh_count(&mut app);
            if body_meshes >= 1 {
                break;
            }
        }
        assert!(
            body_meshes >= 1,
            "y_bot.glb did not spawn a mesh under the avatar (count {body_meshes})"
        );

        let start = avatar_translation(&mut app).unwrap();
        let yaw = follow_yaw(&mut app);
        let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        // Finger up in the thumbstick zone. Screen Y is down, so this is -Y.
        let viewport = app.world().resource::<eustress_common::input_actions::InputState>().viewport;
        let origin = Vec2::new((viewport.x * 0.12).max(80.0), viewport.y * 0.78);
        send_touch(&mut app, bevy::input::touch::TouchPhase::Started, origin, 1);
        app.update();
        send_touch(
            &mut app,
            bevy::input::touch::TouchPhase::Moved,
            origin + Vec2::new(0.0, -eustress_common::input_actions::STICK_RADIUS),
            1,
        );
        for _ in 0..60 {
            app.update();
        }
        let moved = avatar_translation(&mut app).unwrap();
        let delta = moved - start;
        let along = Vec3::new(delta.x, 0.0, delta.z).dot(forward);
        assert!(
            along > 0.3,
            "stick forward moved {along:.3} m along camera-forward {forward:?} (from {start} to {moved}, yaw {yaw})"
        );

        send_touch(&mut app, bevy::input::touch::TouchPhase::Ended, origin, 1);
        app.update();
        let before_jump = avatar_translation(&mut app).unwrap().y;
        let viewport = app.world().resource::<eustress_common::input_actions::InputState>().viewport;
        let insets = app.world().resource::<eustress_common::input_actions::InputState>().insets;
        let (jump_at, _) = eustress_common::input_actions::jump_button(viewport, insets);
        send_touch(&mut app, bevy::input::touch::TouchPhase::Started, jump_at, 2);
        app.update();
        send_touch(&mut app, bevy::input::touch::TouchPhase::Ended, jump_at, 2);
        let mut peak = before_jump;
        for _ in 0..30 {
            app.update();
            peak = peak.max(avatar_translation(&mut app).unwrap().y);
        }
        assert!(
            peak > before_jump + 0.15,
            "jump raised y from {before_jump:.3} to {peak:.3}"
        );
    }

    fn avatar_translation(app: &mut App) -> Option<Vec3> {
        let mut query = app
            .world_mut()
            .query_filtered::<&Transform, With<eustress_common::avatar::SpawnedByAvatarRuntime>>();
        query.iter(app.world()).next().map(|tf| tf.translation)
    }

    fn camera_flags(app: &mut App) -> (bool, bool, bool, Option<Entity>) {
        let follow = {
            let mut query = app.world_mut().query_filtered::<(&Camera, &eustress_common::avatar::control::AvatarCamera), With<eustress_common::avatar::control::AvatarCamera>>();
            let world = app.world();
            query.iter(world).next().map(|(camera, follow)| (camera.is_active, follow.target))
        };
        let scripted = {
            let mut query = app.world_mut().query_filtered::<&Camera, With<eustress_common::play_session::ScriptedPlayCamera>>();
            query.iter(app.world()).any(|camera| camera.is_active)
        };
        let gallery = {
            let mut query = app.world_mut().query_filtered::<&Camera, With<GalleryCamera>>();
            query.iter(app.world()).any(|camera| camera.is_active)
        };
        match follow {
            Some((active, target)) => (active, scripted, gallery, Some(target)),
            None => (false, scripted, gallery, None),
        }
    }

    fn follow_is_the_view(app: &mut App) -> bool {
        let (follow, scripted, gallery, _) = camera_flags(app);
        follow && !scripted && !gallery
    }

    fn follow_yaw(app: &mut App) -> f32 {
        let mut query = app.world_mut().query::<&eustress_common::avatar::control::AvatarCamera>();
        query.iter(app.world()).next().map(|cam| cam.yaw).unwrap_or(0.0)
    }

    fn avatar_mesh_count(app: &mut App) -> usize {
        let root = {
            let mut query = app.world_mut().query_filtered::<&eustress_common::avatar::spawn::AvatarMeshChild, With<eustress_common::avatar::SpawnedByAvatarRuntime>>();
            query.iter(app.world()).next().map(|child| child.0)
        };
        let Some(root) = root else { return 0 };
        let mut count = 0usize;
        let mut stack = vec![root];
        let mut children = app.world_mut().query::<&Children>();
        let mut meshes = app.world_mut().query::<&Mesh3d>();
        while let Some(entity) = stack.pop() {
            if meshes.get(app.world(), entity).is_ok() {
                count += 1;
            }
            if let Ok(kids) = children.get(app.world(), entity) {
                stack.extend(kids.iter());
            }
        }
        count
    }

    fn send_touch(app: &mut App, phase: bevy::input::touch::TouchPhase, position: Vec2, id: u64) {
        let window = {
            let mut query = app.world_mut().query_filtered::<Entity, With<bevy::window::PrimaryWindow>>();
            query.iter(app.world()).next().unwrap_or(Entity::PLACEHOLDER)
        };
        app.world_mut().write_message(bevy::input::touch::TouchInput {
            phase,
            position,
            window,
            force: None,
            id,
        });
    }

    fn paint_shell(window: &slint::platform::software_renderer::MinimalSoftwareWindow) -> bool {
        window.draw_if_needed(|renderer| {
            let mut buffer = i_slint_core::graphics::SharedPixelBuffer::<
                i_slint_core::graphics::Rgb8Pixel,
            >::new(900, 500);
            let stride = buffer.width() as usize;
            let _ = renderer.render(buffer.make_mut_slice(), stride);
        })
    }

    fn sign_in_button_center(ui: &shell::PlayerShell) -> slint::LogicalPosition {
        item_center(ui, "Sign in")
    }

    fn item_center(ui: &shell::PlayerShell, label: &str) -> slint::LogicalPosition {
        let inner = i_slint_core::window::WindowInner::from_pub(ui.window());
        let tree = inner.component();
        let mut found: Option<(slint::LogicalPosition, f32)> = None;
        i_slint_core::item_tree::visit_items(
            &tree,
            i_slint_core::item_tree::TraversalOrder::BackToFront,
            |item_tree, _item, index, _state| {
                let item = i_slint_core::item_tree::ItemRc::new(item_tree.clone(), index);
                let accessible = item.accessible_string_property(
                    i_slint_core::accessibility::AccessibleStringProperty::Label,
                );
                if accessible.as_deref() == Some(label) {
                    let geom = item.geometry();
                    // `map_to_window` adds ancestor positions and not this item's own x/y.
                    let center = item.map_to_window(i_slint_core::lengths::LogicalPoint::new(
                        geom.origin.x + geom.size.width / 2.0,
                        geom.origin.y + geom.size.height / 2.0,
                    ));
                    let area = geom.size.width * geom.size.height;
                    let better = found.as_ref().is_none_or(|(_, best)| area > *best);
                    if better {
                        found = Some((slint::LogicalPosition::new(center.x, center.y), area));
                    }
                }
                i_slint_core::item_tree::ItemVisitorResult::Continue(())
            },
            (),
        );
        found.expect("button has a layout rect").0
    }

    /// `bundled://characters/y_bot.glb` is a path relative to the files
    /// folder. Android's platform reader ignores that folder; this is the
    /// reader `build_game` registers instead.
    #[test]
    fn bundled_reader_reads_the_character_from_an_absolute_folder() {
        use bevy::asset::io::AssetReader;
        let dir = std::env::temp_dir().join(format!("eustress_bundled_reader_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("characters")).unwrap();
        let file = dir.join("characters/y_bot.glb");
        std::fs::write(&file, b"glb-bytes").unwrap();
        let reader = eustress_play_runtime::space_paths::ContainedFileReader::new(dir.clone());
        assert!(
            reader.opens(std::path::Path::new("characters/y_bot.glb")),
            "the files-folder reader must open characters/y_bot.glb"
        );
        let mut bytes = Vec::new();
        bevy::tasks::block_on(async {
            let mut opened = reader.read(std::path::Path::new("characters/y_bot.glb")).await.expect("read");
            bevy::tasks::futures_lite::AsyncReadExt::read_to_end(&mut opened, &mut bytes)
                .await
                .expect("body");
        });
        assert_eq!(bytes, b"glb-bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
static VALIDATION_FAILURES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

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

use std::cell::{RefCell, UnsafeCell};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bevy::input::touch::TouchInput;
use bevy::input::{mouse::MouseButtonInput, ButtonState};
use bevy::prelude::*;
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
use slint::{LogicalPosition, PhysicalSize};

const UI_WIDTH: u32 = 640;
const UI_HEIGHT: u32 = 280;

/// Bumped when the GLES device is destroyed so the render world publishes the
/// replacement handles. Starts at 0; the first device is epoch 0.
static GPU_EPOCH: AtomicU64 = AtomicU64::new(0);

slint::slint! {
    import { VerticalBox, Button } from "std-widgets.slint";
    export component Demo inherits Window {
        background: rgba(16, 32, 48, 93%);
        in-out property <int> click-count: 0;

        VerticalBox {
            padding: 18px;
            alignment: start;
            spacing: 10px;
            Text {
                text: "Eustress phase 0";
                color: #f4efe6;
                font-size: 26px;
            }
            Text {
                text: "Slint on Bevy's wgpu device";
                color: #c5d0d8;
                font-size: 16px;
            }
            Button {
                text: "Clicks: " + click-count;
                clicked => {
                    click-count += 1;
                }
            }
        }
    }
}

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
    size: std::cell::Cell<slint::PhysicalSize>,
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

    fn request_redraw(&self) {}
}

impl BevyWindowAdapter {
    fn new(handles: &WgpuHandles) -> Rc<Self> {
        let renderer = make_skia(handles).expect("SkiaWGPURenderer::new");
        Rc::new_cyclic(|self_weak: &Weak<Self>| Self {
            size: std::cell::Cell::new(slint::PhysicalSize::new(UI_WIDTH, UI_HEIGHT)),
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
    _instance: Demo,
    adapter: Rc<BevyWindowAdapter>,
    /// Last handles Skia was built with. Cleared when the GLES device is dropped
    /// so resume does not rebuild Skia on a dead context.
    handles: RefCell<Option<WgpuHandles>>,
}

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
    ));
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(2.9, 1.27))),
        MeshMaterial3d(ui_material),
        Transform::from_xyz(0.0, 1.55, 2.6),
        SlintQuad,
    ));

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 1.15, 6.2).looking_at(Vec3::new(0.0, 0.35, 0.0), Vec3::Y),
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

        let ui = Demo::new().expect("Demo::new");
        ui.window().show().expect("show");
        let adapter = SLINT_WINDOWS
            .with(|windows| windows.borrow().first().and_then(|weak| weak.upgrade()))
            .expect("window adapter");
        ui.window()
            .dispatch_event(WindowEvent::WindowActiveChanged(true));
        let overlay = world.resource::<OverlaySize>();
        let physical = PhysicalSize::new(overlay.width, overlay.height);
        adapter.size.set(physical);
        adapter.slint_window.dispatch_event(WindowEvent::Resized {
            size: physical.to_logical(1.0),
        });
        world.insert_non_send(SlintContext {
            _instance: ui,
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
    if let Some(texture) = shared.texture.lock().expect("texture slot").as_ref() {
        if let Err(err) = ctx
            .adapter
            .renderer
            .get()
            .expect("slint renderer")
            .render_to_texture(texture)
        {
            eprintln!("slint render_to_texture: {err}");
        } else {
            static LOGGED: AtomicBool = AtomicBool::new(false);
            if !LOGGED.swap(true, Ordering::Relaxed) {
                eprintln!("phase0: slint overlay drawn into the shared texture");
            }
        }
    }
}

fn spin_cube(time: Res<Time>, mut cubes: Query<&mut Transform, With<LitCube>>) {
    for mut transform in &mut cubes {
        transform.rotate_y(0.6 * time.delta_secs());
        transform.rotate_x(0.15 * time.delta_secs());
    }
}

fn pointer_on_quad(
    window_pos: Vec2,
    window: &Window,
    camera: (&Camera, &GlobalTransform),
    quad: &GlobalTransform,
    texture_width: f32,
    texture_height: f32,
) -> Option<LogicalPosition> {
    let (camera, camera_transform) = camera;
    let ray = camera
        .viewport_to_world(camera_transform, window_pos)
        .ok()?;
    let plane = InfinitePlane3d::new(quad.back());
    let hit = ray.plane_intersection_point(quad.translation(), plane)?;
    let local = quad.affine().inverse().transform_point3(hit);
    // Rectangle::new(2.9, 1.27) is centered on the origin.
    if local.x.abs() > 1.45 || local.y.abs() > 0.635 {
        return None;
    }
    let u = (local.x + 1.45) / 2.9;
    let v = 1.0 - (local.y + 0.635) / 1.27;
    let _ = window;
    Some(LogicalPosition::new(u * texture_width, v * texture_height))
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
    ctx: Option<NonSend<SlintContext>>,
    quads: Query<&GlobalTransform, With<SlintQuad>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
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
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    let Some(quad) = quads.iter().next() else {
        return;
    };

    let panel = ctx.adapter.size.get();
    if let Some(cursor) = window.cursor_position() {
        let over = pointer_on_quad(
            cursor,
            window,
            camera,
            quad,
            panel.width as f32,
            panel.height as f32,
        );
        let mut press = None;
        for event in mouse.read() {
            if event.button == MouseButton::Left {
                press = Some(event.state == ButtonState::Pressed);
            }
        }
        dispatch_pointer(&ctx.adapter, over, press);
    }

    for touch in touches.read() {
        let over = pointer_on_quad(
            touch.position,
            window,
            camera,
            quad,
            panel.width as f32,
            panel.height as f32,
        );
        let press = match touch.phase {
            bevy::input::touch::TouchPhase::Started => Some(true),
            bevy::input::touch::TouchPhase::Ended | bevy::input::touch::TouchPhase::Canceled => {
                Some(false)
            }
            bevy::input::touch::TouchPhase::Moved => None,
        };
        dispatch_pointer(&ctx.adapter, over, press);
    }
}

/// Landscape panel is 640×280. A taller window swaps those axes so rotation
/// rebuilds the texture instead of stretching the landscape image.
fn overlay_size_for_window(window: &Window) -> Option<(u32, u32)> {
    let width = window.resolution.physical_width();
    let height = window.resolution.physical_height();
    if width == 0 || height == 0 {
        return None;
    }
    if width >= height {
        Some((UI_WIDTH, UI_HEIGHT))
    } else {
        Some((UI_HEIGHT, UI_WIDTH))
    }
}

/// Tell Slint the panel size, allocate a matching texture, and scale the quad
/// so the mesh aspect matches. The Skia renderer is the one already associated
/// with the window; this only replaces the texture it draws into.
fn publish_overlay(
    width: u32,
    height: u32,
    ctx: Option<&SlintContext>,
    shared: &SlintSharedTexture,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    overlay: &mut SlintImageHandle,
    epoch: &mut OverlayEpoch,
    quads: &Query<&MeshMaterial3d<StandardMaterial>, With<SlintQuad>>,
    quad_transforms: &mut Query<&mut Transform, With<SlintQuad>>,
) {
    if let Some(ctx) = ctx {
        let physical = PhysicalSize::new(width, height);
        ctx.adapter.size.set(physical);
        let scale = ctx.adapter.slint_window.scale_factor();
        ctx.adapter
            .slint_window
            .dispatch_event(WindowEvent::Resized {
                size: physical.to_logical(scale),
            });
    }
    let aspect = width as f32 / height as f32;
    let height_world = 1.27;
    let width_world = height_world * aspect;
    for mut transform in quad_transforms.iter_mut() {
        transform.scale = Vec3::new(width_world / 2.9, height_world / 1.27, 1.0);
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
    eprintln!("phase0: slint overlay texture {width}x{height} scheduled");
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
    }
}

fn refresh_overlay(
    mut loss: ResMut<SurfaceLoss>,
    hold: Res<PlayerSwapchainHold>,
    mut size: ResMut<OverlaySize>,
    windows: Query<&Window, With<PrimaryWindow>>,
    ctx: Option<NonSend<SlintContext>>,
    shared: Res<SlintSharedTexture>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut overlay: Option<ResMut<SlintImageHandle>>,
    mut epoch: Option<ResMut<OverlayEpoch>>,
    quads: Query<&MeshMaterial3d<StandardMaterial>, With<SlintQuad>>,
    mut quad_transforms: Query<&mut Transform, With<SlintQuad>>,
) {
    if hold.0 {
        return;
    }
    let Some(window) = windows.iter().next() else {
        return;
    };
    let Some((width, height)) = overlay_size_for_window(window) else {
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
        ctx.as_deref(),
        &shared,
        &mut images,
        &mut materials,
        overlay,
        epoch,
        &quads,
        &mut quad_transforms,
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

#[cfg(target_os = "android")]
fn make_instance(settings: &WgpuSettings, backends: wgpu::Backends) -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        flags: settings.instance_flags,
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
        flags: settings.instance_flags,
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
        if app.is_plugin_added::<RenderPlugin>() {
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

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<RenderPlugin>()
            .disable::<bevy::image::ImagePlugin>()
            .disable::<bevy::mesh::MeshPlugin>()
            .disable::<bevy::camera::CameraPlugin>()
            .disable::<bevy::light::LightPlugin>()
            .disable::<bevy::core_pipeline::CorePipelinePlugin>()
            .disable::<bevy::pbr::PbrPlugin>(),
    )
    .add_plugins(SurfaceGate {
        settings: settings.clone(),
        texture_tx,
        device_tx,
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
    .add_systems(Startup, (install_render_error_handler, setup_scene))
    .add_systems(
        Update,
        (
            sync_surface_loss,
            refresh_overlay,
            init_slint,
            receive_texture,
            render_slint,
            pointer_input,
            spin_cube,
        )
            .chain(),
    );

    app.run();
}

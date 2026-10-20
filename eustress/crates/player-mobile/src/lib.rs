//! Phase 0 spike. Pattern (a): Bevy owns the window and the wgpu device.
//! Slint's `SkiaWGPURenderer` draws a label and a button into a texture on
//! that same device, and Bevy samples the texture on an unlit quad in front
//! of a lit cube.
//!
//! Pattern (b) lives behind the `pattern-b` feature (`pattern_b` module) and
//! is not the activity entry. FemtoVG-wgpu is not used: Slint does not build
//! that renderer for Android. See `PHASE0_SPIKE.md`.

#[cfg(feature = "pattern-b")]
mod pattern_b;

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};

use bevy::input::touch::TouchInput;
use bevy::input::{ButtonState, mouse::MouseButtonInput};
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::{RenderAdapter, RenderDevice, RenderInstance, RenderQueue};
use bevy::render::settings::RenderCreation;
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderPlugin};
use slint::platform::skia_renderer::SkiaWGPURenderer;
use slint::platform::WindowEvent;
use slint::wgpu_29::wgpu;
use slint::{LogicalPosition, PhysicalSize};

const UI_WIDTH: u32 = 640;
const UI_HEIGHT: u32 = 280;

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

struct BevyWindowAdapter {
    size: std::cell::Cell<slint::PhysicalSize>,
    slint_window: slint::Window,
    renderer: SkiaWGPURenderer,
}

impl slint::platform::WindowAdapter for BevyWindowAdapter {
    fn window(&self) -> &slint::Window {
        &self.slint_window
    }

    fn size(&self) -> slint::PhysicalSize {
        self.size.get()
    }

    fn renderer(&self) -> &dyn slint::platform::Renderer {
        &self.renderer
    }

    fn set_visible(&self, _visible: bool) -> Result<(), slint::PlatformError> {
        Ok(())
    }

    fn request_redraw(&self) {}
}

impl BevyWindowAdapter {
    fn new(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Rc<Self> {
        let renderer = SkiaWGPURenderer::new(instance, adapter, device, queue)
            .expect("SkiaWGPURenderer::new");
        Rc::new_cyclic(|self_weak: &Weak<Self>| Self {
            size: std::cell::Cell::new(slint::PhysicalSize::new(UI_WIDTH, UI_HEIGHT)),
            slint_window: slint::Window::new(self_weak.clone()),
            renderer,
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
        let adapter = BevyWindowAdapter::new(
            self.instance.clone(),
            self.adapter.clone(),
            self.device.clone(),
            self.queue.clone(),
        );
        SLINT_WINDOWS.with(|windows| windows.borrow_mut().push(Rc::downgrade(&adapter)));
        Ok(adapter)
    }
}

struct SlintContext {
    _instance: Demo,
    adapter: Rc<BevyWindowAdapter>,
}

#[derive(Component)]
struct LitCube;

#[derive(Component)]
struct SlintQuad;

#[derive(Resource)]
struct TextureSender(std::sync::mpsc::Sender<wgpu::Texture>);

#[derive(Resource, Clone, ExtractResource)]
struct SlintImageHandle(Handle<Image>);

#[derive(Resource)]
struct SlintSharedTexture {
    receiver: Mutex<std::sync::mpsc::Receiver<wgpu::Texture>>,
    texture: Arc<Mutex<Option<wgpu::Texture>>>,
}

type WgpuHandles = (wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue);

/// Render world publishes the device once Bevy has created it against the
/// real Android surface. Creating it earlier finds no GLES adapter.
#[derive(Resource, Clone)]
struct DeviceSender(std::sync::mpsc::Sender<WgpuHandles>);

#[derive(Resource)]
struct DeviceReceiver(Mutex<std::sync::mpsc::Receiver<WgpuHandles>>);

thread_local! {
    static SLINT_WINDOWS: RefCell<Vec<Weak<BevyWindowAdapter>>> = RefCell::new(Vec::new());
}

fn clone_wgpu(instance: &RenderInstance, adapter: &RenderAdapter, device: &RenderDevice, queue: &RenderQueue) -> (wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue) {
    (
        (**instance.0).clone(),
        (**adapter.0).clone(),
        device.wgpu_device().clone(),
        (**queue.0).clone(),
    )
}

fn setup_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let size = Extent3d {
        width: UI_WIDTH,
        height: UI_HEIGHT,
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
    let image_handle = images.add(image);
    commands.insert_resource(SlintImageHandle(image_handle.clone()));

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

fn publish_shared_device(
    instance: Option<Res<RenderInstance>>,
    adapter: Option<Res<RenderAdapter>>,
    device: Option<Res<RenderDevice>>,
    queue: Option<Res<RenderQueue>>,
    sender: Res<DeviceSender>,
    mut sent: Local<bool>,
) {
    if *sent {
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
    *sent = true;
}

fn init_slint(world: &mut World) {
    if world.contains_non_send::<SlintContext>() {
        return;
    }
    let (instance, adapter, device, queue) = {
        let receiver = world.resource::<DeviceReceiver>();
        let Ok(handles) = receiver.0.lock().expect("device receiver").try_recv() else {
            return;
        };
        handles
    };

    slint::platform::set_platform(Box::new(SlintBevyPlatform {
        instance,
        adapter,
        device,
        queue,
    }))
    .expect("set_platform");

    let ui = Demo::new().expect("Demo::new");
    ui.window().show().expect("show");
    let adapter = SLINT_WINDOWS
        .with(|windows| windows.borrow().first().and_then(|weak| weak.upgrade()))
        .expect("window adapter");
    ui.window()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));
    adapter.size.set(PhysicalSize::new(UI_WIDTH, UI_HEIGHT));
    adapter
        .slint_window
        .dispatch_event(WindowEvent::Resized {
            size: PhysicalSize::new(UI_WIDTH, UI_HEIGHT).to_logical(1.0),
        });
    world.insert_non_send(SlintContext {
        _instance: ui,
        adapter,
    });
}

fn receive_texture(shared: Res<SlintSharedTexture>) {
    if let Ok(texture) = shared.receiver.lock().expect("texture receiver").try_recv() {
        *shared.texture.lock().expect("texture slot") = Some(texture);
    }
}

fn send_slint_texture(
    handle: Option<Res<SlintImageHandle>>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    sender: Res<TextureSender>,
    mut sent: Local<bool>,
) {
    if *sent {
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
    *sent = true;
}

fn render_slint(ctx: Option<NonSend<SlintContext>>, shared: Res<SlintSharedTexture>) {
    let Some(ctx) = ctx else {
        return;
    };
    slint::platform::update_timers_and_animations();
    if let Some(texture) = shared.texture.lock().expect("texture slot").as_ref() {
        if let Err(err) = ctx.adapter.renderer.render_to_texture(texture) {
            eprintln!("slint render_to_texture: {err}");
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
) -> Option<LogicalPosition> {
    let (camera, camera_transform) = camera;
    let ray = camera.viewport_to_world(camera_transform, window_pos).ok()?;
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
    Some(LogicalPosition::new(u * UI_WIDTH as f32, v * UI_HEIGHT as f32))
}

fn dispatch_pointer(adapter: &BevyWindowAdapter, position: Option<LogicalPosition>, pressed: Option<bool>) {
    let Some(position) = position else {
        adapter.slint_window.dispatch_event(WindowEvent::PointerExited);
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
    let Some(window) = windows.iter().next() else {
        return;
    };
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    let Some(quad) = quads.iter().next() else {
        return;
    };

    if let Some(cursor) = window.cursor_position() {
        let over = pointer_on_quad(cursor, window, camera, quad);
        let mut press = None;
        for event in mouse.read() {
            if event.button == MouseButton::Left {
                press = Some(event.state == ButtonState::Pressed);
            }
        }
        dispatch_pointer(&ctx.adapter, over, press);
    }

    for touch in touches.read() {
        let over = pointer_on_quad(touch.position, window, camera, quad);
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
    // `WGPU_BACKEND` (wrap.sh sets `gl` on the emulator) wins. GLES only
    // enumerates once the Android surface exists, so Bevy creates the device.
    let backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::VULKAN | wgpu::Backends::GL);
    let mut settings = bevy::render::settings::WgpuSettings::default();
    settings.backends = Some(backends);

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(RenderPlugin {
        render_creation: RenderCreation::Automatic(Box::new(settings)),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.07, 0.09, 0.12)))
    .insert_resource(SlintSharedTexture {
        receiver: Mutex::new(texture_rx),
        texture: Arc::new(Mutex::new(None)),
    })
    .insert_resource(DeviceReceiver(Mutex::new(device_rx)))
    .insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 280.0,
        ..default()
    })
    .add_plugins(ExtractResourcePlugin::<SlintImageHandle>::default())
    .add_systems(Startup, setup_scene)
    .add_systems(
        Update,
        (init_slint, receive_texture, render_slint, pointer_input, spin_cube).chain(),
    );

    let render_app = app.sub_app_mut(RenderApp);
    render_app.insert_resource(TextureSender(texture_tx));
    render_app.insert_resource(DeviceSender(device_tx));
    render_app.add_systems(Render, (publish_shared_device, send_slint_texture).chain());

    app.run();
}

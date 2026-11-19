//! One Bevy 0.19 window with a moving 3D scene and a Slint UI over it, the UI
//! drawn either by Slint's software renderer (Studio's path today, copied from
//! `engine/src/ui/slint_ui.rs`) or by Slint 1.19's `SkiaWGPU29Renderer` on
//! Bevy's own wgpu device. It measures the UI's cost and the frame time, then
//! exits and prints one JSON line.
//!
//! Run `ui-gpu-spike --help` for the options.

use std::cell::{Cell, RefCell};
use std::io::Write as _;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::Mutex;
use std::time::Instant;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::input::{mouse::MouseButtonInput, ButtonState};
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    BlendState, Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::settings::{
    Backends, PowerPreference, RenderCreation, RenderResources, WgpuSettings,
};
use bevy::render::texture::GpuImage;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::render::view::Msaa;
use bevy::render::{Render, RenderApp, RenderPlugin};
use bevy::window::{PresentMode, PrimaryWindow, WindowResolution};

use slint::platform::skia_renderer::SkiaWGPU29Renderer;
use slint::platform::software_renderer::{
    PremultipliedRgbaColor, RepaintBufferType, SoftwareRenderer,
};
use slint::platform::{PointerEventButton, WindowAdapter, WindowEvent};
use slint::wgpu_29::wgpu;
use slint::{ComponentHandle, LogicalPosition, PhysicalSize};

mod ui {
    slint::include_modules!();
}

// ============================================================================
// Options
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RendererKind {
    Software,
    Gpu,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum UiKind {
    Studio,
    Glass,
    TextProbe,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scenario {
    /// Nothing changes in the UI: both paths should skip drawing.
    Idle,
    /// The pointer sweeps over the window every frame, as a user moving the
    /// mouse does: hover states change, the UI repaints what changed.
    Sweep,
    /// The whole UI is redrawn every frame (the software path with
    /// `RepaintBufferType::NewBuffer`, the GPU path every frame).
    Full,
}

#[derive(Resource, Clone, Debug)]
struct Config {
    renderer: RendererKind,
    ui: UiKind,
    scenario: Scenario,
    scene_under: bool,
    width: u32,
    height: u32,
    warmup: u32,
    frames: u32,
    pipelined: bool,
    vulkan: bool,
    sync: bool,
    blur: f32,
    screenshot: Option<PathBuf>,
    screenshot_frame: u32,
    results: Option<PathBuf>,
    interactive: bool,
    label: String,
}

const HELP: &str = "ui-gpu-spike [options]
  --renderer software|gpu     UI renderer (default gpu)
  --ui studio|glass|textprobe Studio's StudioWindow, the backdrop-blur demo, or the
                              oversized-text shapes of display_text.rs's render test (default studio)
  --scenario idle|sweep|full  what changes in the UI each frame (default sweep)
  --scene-under               gpu only: draw the 3D scene inside Slint, under the UI, so blur sees it
  --size WxH                  window size in physical px (default 3456x929, Studio's window here)
  --warmup N --frames N       frames before measuring, frames measured (default 180, 600)
  --pipelined on|off          Bevy pipelined rendering (default on for software, off for gpu)
  --backend vulkan|dx12       wgpu backend (default vulkan, as Studio uses here)
  --sync                      gpu: wait for the GPU after each UI draw, to time CPU+GPU
  --blur PX                   glass: backdrop-blur radius (default 24)
  --screenshot PATH           save a screenshot of the window
  --screenshot-frame N        frame to take it at (default warmup)
  --results PATH              append the JSON result line to this file
  --label TEXT                free text copied into the result
  --interactive               keep running, forward the real mouse, no measuring";

impl Config {
    fn from_args() -> Self {
        let mut cfg = Config {
            renderer: RendererKind::Gpu,
            ui: UiKind::Studio,
            scenario: Scenario::Sweep,
            scene_under: false,
            width: 3456,
            height: 929,
            warmup: 180,
            frames: 600,
            pipelined: true,
            vulkan: true,
            sync: false,
            blur: 24.0,
            screenshot: None,
            screenshot_frame: 0,
            results: None,
            interactive: false,
            label: String::new(),
        };
        let mut pipelined: Option<bool> = None;
        let mut shot_frame: Option<u32> = None;
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        let value = |i: &mut usize| -> String {
            *i += 1;
            args.get(*i).cloned().unwrap_or_else(|| {
                eprintln!("missing value after {}\n{HELP}", args[*i - 1]);
                std::process::exit(2)
            })
        };
        while i < args.len() {
            match args[i].as_str() {
                "--renderer" => {
                    cfg.renderer = match value(&mut i).as_str() {
                        "software" => RendererKind::Software,
                        "gpu" => RendererKind::Gpu,
                        other => bad(other),
                    }
                }
                "--ui" => {
                    cfg.ui = match value(&mut i).as_str() {
                        "studio" => UiKind::Studio,
                        "glass" => UiKind::Glass,
                        "textprobe" => UiKind::TextProbe,
                        other => bad(other),
                    }
                }
                "--scenario" => {
                    cfg.scenario = match value(&mut i).as_str() {
                        "idle" => Scenario::Idle,
                        "sweep" => Scenario::Sweep,
                        "full" => Scenario::Full,
                        other => bad(other),
                    }
                }
                "--scene-under" => cfg.scene_under = true,
                "--size" => {
                    let v = value(&mut i);
                    let (w, h) = v.split_once(['x', 'X']).unwrap_or_else(|| bad(&v));
                    cfg.width = w.parse().unwrap_or_else(|_| bad(&v));
                    cfg.height = h.parse().unwrap_or_else(|_| bad(&v));
                }
                "--warmup" => {
                    cfg.warmup = value(&mut i)
                        .parse()
                        .unwrap_or_else(|e| bad(&format!("{e}")))
                }
                "--frames" => {
                    cfg.frames = value(&mut i)
                        .parse()
                        .unwrap_or_else(|e| bad(&format!("{e}")))
                }
                "--pipelined" => {
                    pipelined = Some(match value(&mut i).as_str() {
                        "on" => true,
                        "off" => false,
                        other => bad(other),
                    })
                }
                "--backend" => {
                    cfg.vulkan = match value(&mut i).as_str() {
                        "vulkan" => true,
                        "dx12" => false,
                        other => bad(other),
                    }
                }
                "--sync" => cfg.sync = true,
                "--blur" => {
                    cfg.blur = value(&mut i)
                        .parse()
                        .unwrap_or_else(|e| bad(&format!("{e}")))
                }
                "--screenshot" => cfg.screenshot = Some(PathBuf::from(value(&mut i))),
                "--screenshot-frame" => {
                    shot_frame = Some(
                        value(&mut i)
                            .parse()
                            .unwrap_or_else(|e| bad(&format!("{e}"))),
                    )
                }
                "--results" => cfg.results = Some(PathBuf::from(value(&mut i))),
                "--label" => cfg.label = value(&mut i),
                "--interactive" => cfg.interactive = true,
                "--help" | "-h" => {
                    println!("{HELP}");
                    std::process::exit(0)
                }
                other => bad(other),
            }
            i += 1;
        }
        // Skia's Vulkan backend submits on wgpu's raw VkQueue from the main
        // thread. With pipelined rendering, Bevy's render thread submits on
        // the same queue at the same time, and a VkQueue must not be used by
        // two threads at once. So the GPU path turns pipelining off unless
        // told otherwise.
        cfg.pipelined = pipelined.unwrap_or(cfg.renderer == RendererKind::Software);
        cfg.screenshot_frame = shot_frame.unwrap_or(cfg.warmup);
        if cfg.scene_under && cfg.renderer == RendererKind::Software {
            eprintln!("--scene-under needs --renderer gpu: the software renderer cannot sample a wgpu texture");
            std::process::exit(2);
        }
        cfg
    }

    fn last_frame(&self) -> u32 {
        let measured = self.warmup + self.frames;
        if self.screenshot.is_some() {
            measured.max(self.screenshot_frame + 30)
        } else {
            measured
        }
    }
}

fn bad<T>(what: &str) -> T {
    eprintln!("bad option or value: {what}\n{HELP}");
    std::process::exit(2)
}

// ============================================================================
// Slint platform and window adapter
// ============================================================================

#[derive(Resource, Clone)]
struct GpuHandles {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

enum SpikeRenderer {
    Software(SoftwareRenderer),
    Gpu(SkiaWGPU29Renderer),
}

struct SpikeWindowAdapter {
    slint_window: slint::Window,
    size: Cell<PhysicalSize>,
    scale_factor: Cell<f32>,
    renderer: SpikeRenderer,
    /// Set by Slint's `request_redraw` (property changes, input, animations),
    /// cleared when the UI is drawn. Same rule as Studio's adapter.
    needs_redraw: Cell<bool>,
}

impl WindowAdapter for SpikeWindowAdapter {
    fn window(&self) -> &slint::Window {
        &self.slint_window
    }

    fn size(&self) -> PhysicalSize {
        self.size.get()
    }

    fn renderer(&self) -> &dyn slint::platform::Renderer {
        match &self.renderer {
            SpikeRenderer::Software(r) => r,
            SpikeRenderer::Gpu(r) => r,
        }
    }

    fn set_visible(&self, _visible: bool) -> Result<(), slint::PlatformError> {
        Ok(())
    }

    fn request_redraw(&self) {
        self.needs_redraw.set(true);
    }
}

impl SpikeWindowAdapter {
    fn new(renderer: SpikeRenderer, size: PhysicalSize) -> Rc<Self> {
        Rc::new_cyclic(|self_weak: &Weak<Self>| Self {
            slint_window: slint::Window::new(self_weak.clone()),
            size: Cell::new(size),
            scale_factor: Cell::new(1.0),
            renderer,
            needs_redraw: Cell::new(true),
        })
    }

    fn resize(&self, new_size: PhysicalSize, scale_factor: f32) {
        self.size.set(new_size);
        self.scale_factor.set(scale_factor);
        self.slint_window.dispatch_event(WindowEvent::Resized {
            size: new_size.to_logical(scale_factor),
        });
        self.slint_window
            .dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor });
        self.needs_redraw.set(true);
    }
}

thread_local! {
    static ADAPTERS: RefCell<Vec<Rc<SpikeWindowAdapter>>> = const { RefCell::new(Vec::new()) };
}

struct SpikePlatform {
    kind: RendererKind,
    gpu: GpuHandles,
    size: PhysicalSize,
}

impl slint::platform::Platform for SpikePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let renderer = match self.kind {
            RendererKind::Software => SpikeRenderer::Software(
                SoftwareRenderer::new_with_repaint_buffer_type(RepaintBufferType::ReusedBuffer),
            ),
            RendererKind::Gpu => {
                let g = self.gpu.clone();
                SpikeRenderer::Gpu(SkiaWGPU29Renderer::new(
                    g.instance, g.adapter, g.device, g.queue,
                )?)
            }
        };
        let adapter = SpikeWindowAdapter::new(renderer, self.size);
        ADAPTERS.with(|a| a.borrow_mut().push(adapter.clone()));
        Ok(adapter)
    }
}

// ============================================================================
// Bevy resources
// ============================================================================

enum UiInstance {
    Studio(ui::StudioWindow),
    Glass(ui::GlassDemo),
    TextProbe(ui::TextProbe),
}

/// Main-thread Slint state (NonSend: Slint is `Rc`-based).
struct SlintState {
    ui: UiInstance,
    adapter: Rc<SpikeWindowAdapter>,
    gpu: GpuHandles,
    /// Software path: the buffer Slint paints into, like Studio's
    /// `SlintStagingBuffer`.
    staging: Vec<PremultipliedRgbaColor>,
    /// GPU path: the wgpu texture behind the UI `Image`, once the render
    /// world has created it.
    ui_texture: Option<wgpu::Texture>,
    scene_set: bool,
    render_error_reported: bool,
}

/// Image handles the render world turns into wgpu textures.
#[derive(Resource, Clone, ExtractResource)]
struct SpikeImages {
    ui: Handle<Image>,
    scene: Option<Handle<Image>>,
}

#[derive(Resource)]
struct UiMaterial(Handle<StandardMaterial>);

type TexturePair = (wgpu::Texture, Option<wgpu::Texture>);

/// Render world: sends the textures to the main world once.
#[derive(Resource)]
struct TextureSender(std::sync::mpsc::Sender<TexturePair>);

/// Main world: receives them.
#[derive(Resource)]
struct TextureReceiver(Mutex<std::sync::mpsc::Receiver<TexturePair>>);

#[derive(Resource)]
struct AdapterLabel(String);

#[derive(Component)]
struct Spinner(f32);

#[derive(Component)]
struct MainCamera;

#[derive(Resource, Default)]
struct Measure {
    frame: u32,
    last_frame_start: Option<Instant>,
    frame_times: Vec<f64>,
    /// UI cost per drawn frame inside the measured window, in ms.
    ui_times: Vec<f64>,
    /// Software path only: of `ui_times`, the staging-to-image copy.
    copy_times: Vec<f64>,
    /// GPU path with `--sync`: draw call plus the wait for the GPU.
    synced_times: Vec<f64>,
    /// Frames in the window where Slint asked for a redraw by itself
    /// (input, property change, animation), whatever the scenario forces.
    redraw_requests: u32,
    painted: u32,
    /// Software path: time spent in `render` calls that painted nothing.
    noop_ms_sum: f64,
    /// GPU path: `render_to_texture` calls that returned an error.
    render_errors: u32,
    dirty_share_sum: f64,
    clicks: Option<i32>,
    caret: Option<i32>,
    screenshot_taken: bool,
    reported: bool,
}

impl Measure {
    fn in_window(&self, cfg: &Config) -> bool {
        self.frame > cfg.warmup && self.frame <= cfg.warmup + cfg.frames
    }
}

// ============================================================================
// Entry point
// ============================================================================

pub fn run() {
    let cfg = Config::from_args();

    // The device Bevy renders with, made here so Slint can be handed the same
    // one (Studio already creates its RenderResources itself when ray tracing
    // is Auto or On; see engine `raytracing::render_creation`).
    let backends = if cfg.vulkan {
        Backends::VULKAN
    } else {
        Backends::DX12
    };
    let settings = WgpuSettings {
        backends: Some(backends),
        power_preference: PowerPreference::HighPerformance,
        ..default()
    };
    let RenderResources(render_device, render_queue, adapter_info, render_adapter, render_instance) =
        bevy::tasks::block_on(bevy::render::renderer::initialize_renderer(
            backends, None, &settings,
        ));
    let adapter_label = format!(
        "{} ({:?}, driver {})",
        adapter_info.name, adapter_info.backend, adapter_info.driver_info
    );
    eprintln!("[spike] adapter: {adapter_label}");

    let gpu = GpuHandles {
        instance: (**render_instance.0).clone(),
        adapter: (**render_adapter.0).clone(),
        device: render_device.wgpu_device().clone(),
        queue: (**render_queue.0).clone(),
    };

    slint::platform::set_platform(Box::new(SpikePlatform {
        kind: cfg.renderer,
        gpu: gpu.clone(),
        size: PhysicalSize::new(cfg.width, cfg.height),
    }))
    .expect("the Slint platform was already set");

    let mut app = App::new();
    let window = Window {
        title: format!(
            "ui-gpu-spike: {:?} {:?} {:?}",
            cfg.renderer, cfg.ui, cfg.scenario
        ),
        resolution: WindowResolution::new(cfg.width, cfg.height),
        present_mode: PresentMode::AutoNoVsync,
        resizable: false,
        position: bevy::window::WindowPosition::Centered(bevy::window::MonitorSelection::Primary),
        ..default()
    };
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(window),
            ..default()
        })
        .set(RenderPlugin {
            render_creation: RenderCreation::manual(
                render_device,
                render_queue,
                adapter_info,
                render_adapter,
                render_instance,
            ),
            synchronous_pipeline_compilation: true,
            ..default()
        });
    if cfg.pipelined {
        app.add_plugins(plugins);
    } else {
        app.add_plugins(plugins.disable::<PipelinedRenderingPlugin>());
    }

    let (tx, rx) = std::sync::mpsc::channel::<TexturePair>();
    app.insert_resource(cfg.clone())
        .insert_resource(AdapterLabel(adapter_label))
        .insert_resource(TextureReceiver(Mutex::new(rx)))
        .insert_resource(gpu)
        .init_resource::<Measure>()
        .add_plugins(ExtractResourcePlugin::<SpikeImages>::default())
        .add_systems(Startup, (setup_scene, setup_slint).chain())
        .add_systems(First, frame_clock)
        .add_systems(Update, (spin, forward_real_input, drive_ui).chain())
        .add_systems(Last, report);
    app.sub_app_mut(RenderApp)
        .insert_resource(TextureSender(tx))
        .add_systems(Render, send_textures);
    app.run();
}

// ============================================================================
// Scene
// ============================================================================

fn setup_scene(
    mut commands: Commands,
    cfg: Res<Config>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // The UI texture.
    let size = Extent3d {
        width: cfg.width,
        height: cfg.height,
        depth_or_array_layers: 1,
    };
    let ui_image = match cfg.renderer {
        // Studio's overlay image, field for field: the CPU copy stays alive so
        // the software renderer's pixels can be written into it.
        RendererKind::Software => {
            let mut image = Image {
                texture_descriptor: TextureDescriptor {
                    label: Some("SlintOverlay"),
                    size,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8UnormSrgb,
                    mip_level_count: 1,
                    sample_count: 1,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                asset_usage: bevy::asset::RenderAssetUsages::MAIN_WORLD
                    | bevy::asset::RenderAssetUsages::RENDER_WORLD,
                ..default()
            };
            image.resize(size);
            image
        }
        // GPU only: Skia draws straight into it. Skia's Vulkan backend needs
        // COPY_SRC and COPY_DST as well as RENDER_ATTACHMENT.
        RendererKind::Gpu => gpu_target_image(cfg.width, cfg.height, "SlintOverlayGpu"),
    };
    let ui_handle = images.add(ui_image);

    let scene_handle = cfg
        .scene_under
        .then(|| images.add(gpu_target_image(cfg.width, cfg.height, "SceneUnderUi")));

    commands.insert_resource(SpikeImages {
        ui: ui_handle.clone(),
        scene: scene_handle.clone(),
    });

    // The 3D scene: a floor and rows of spinning cubes, bright enough that a
    // blur over them is plain to see.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(60.0, 60.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.22, 0.24, 0.28),
            perceptual_roughness: 0.9,
            ..default()
        })),
    ));
    let cube = meshes.add(Cuboid::new(1.4, 1.4, 1.4));
    for row in 0..3 {
        for col in 0..9 {
            let hue = (col as f32 * 40.0 + row as f32 * 15.0) % 360.0;
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::hsl(hue, 0.85, 0.55),
                    perceptual_roughness: 0.35,
                    ..default()
                })),
                Transform::from_xyz(
                    col as f32 * 3.0 - 12.0,
                    1.2 + row as f32 * 2.2,
                    -row as f32 * 3.0,
                ),
                Spinner(0.6 + 0.15 * (col + row) as f32),
            ));
        }
    }
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(8.0, 14.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 400.0,
        ..default()
    });

    let mut main_camera = commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.07, 0.08, 0.11)),
            ..default()
        },
        Msaa::Off,
        Transform::from_xyz(0.0, 7.0, 17.0).looking_at(Vec3::new(0.0, 2.0, 0.0), Vec3::Y),
        MainCamera,
    ));
    if let Some(scene) = &scene_handle {
        main_camera.insert(RenderTarget::Image(scene.clone().into()));
    }

    // The overlay: Studio's camera and quad (slint_ui.rs, setup_slint_overlay),
    // minus the atmosphere and HDR markers, with tonemapping and dithering off
    // so both renderers' pixels reach the window unchanged.
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(ui_handle),
        unlit: true,
        alpha_mode: AlphaMode::Premultiplied,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(UiMaterial(material.clone()));
    let overlay_layer = RenderLayers::layer(31);
    commands.spawn((
        Camera3d::default(),
        Projection::from(OrthographicProjection {
            near: -1.0,
            far: 10.0,
            scaling_mode: ScalingMode::Fixed {
                width: cfg.width as f32,
                height: cfg.height as f32,
            },
            ..OrthographicProjection::default_3d()
        }),
        Camera {
            order: 300,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                clear_color: ClearColorConfig::Custom(Color::NONE),
            },
            ..default()
        },
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
        Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        overlay_layer.clone(),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Rectangle::new(cfg.width as f32, cfg.height as f32))),
        MeshMaterial3d(material),
        Transform::default(),
        overlay_layer,
    ));
}

fn gpu_target_image(width: u32, height: u32, label: &'static str) -> Image {
    let mut image = Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.label = Some(label);
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::RENDER_ATTACHMENT
        | TextureUsages::COPY_SRC
        | TextureUsages::COPY_DST;
    // GPU only: no CPU copy kept, nothing re-uploaded.
    image.asset_usage = bevy::asset::RenderAssetUsages::RENDER_WORLD;
    image
}

fn spin(time: Res<Time>, mut q: Query<(&mut Transform, &Spinner)>) {
    let dt = time.delta_secs();
    for (mut t, s) in &mut q {
        t.rotate_y(dt * s.0);
        t.rotate_x(dt * s.0 * 0.4);
    }
}

// ============================================================================
// Slint setup and drawing
// ============================================================================

fn setup_slint(world: &mut World) {
    let cfg = world.resource::<Config>().clone();
    let scale_factor = {
        let mut q = world.query_filtered::<&Window, With<PrimaryWindow>>();
        q.iter(world)
            .next()
            .map(|w| w.scale_factor())
            .unwrap_or(1.0)
    };
    let gpu = world.resource::<GpuHandles>().clone();

    let ui = match cfg.ui {
        UiKind::Studio => {
            let ui = ui::StudioWindow::new().expect("StudioWindow::new");
            ui.window().show().expect("show");
            UiInstance::Studio(ui)
        }
        UiKind::Glass => {
            let ui = ui::GlassDemo::new().expect("GlassDemo::new");
            ui.set_blur(cfg.blur);
            ui.set_scene_under(cfg.scene_under);
            ui.window().show().expect("show");
            UiInstance::Glass(ui)
        }
        UiKind::TextProbe => {
            let ui = ui::TextProbe::new().expect("TextProbe::new");
            ui.set_lines("line of text\n".repeat(10_000).into());
            ui.set_long("a".repeat(131_000).into());
            ui.window().show().expect("show");
            UiInstance::TextProbe(ui)
        }
    };
    let adapter = ADAPTERS
        .with(|a| a.borrow().first().cloned())
        .expect("the window adapter is created with the component");
    adapter
        .slint_window
        .dispatch_event(WindowEvent::WindowActiveChanged(true));
    adapter.resize(PhysicalSize::new(cfg.width, cfg.height), scale_factor);
    if cfg.scenario == Scenario::Full {
        if let SpikeRenderer::Software(sw) = &adapter.renderer {
            sw.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
        }
    }
    eprintln!(
        "[spike] {:?} renderer, {:?} UI, {:?}, {}x{} at scale {scale_factor}, pipelined {}",
        cfg.renderer, cfg.ui, cfg.scenario, cfg.width, cfg.height, cfg.pipelined
    );

    world.insert_non_send_resource(SlintState {
        ui,
        adapter,
        gpu,
        staging: vec![PremultipliedRgbaColor::default(); (cfg.width * cfg.height) as usize],
        ui_texture: None,
        scene_set: false,
        render_error_reported: false,
    });
}

/// Render world: hand the UI (and scene) textures to the main world once they
/// exist on the GPU.
fn send_textures(
    images: Option<Res<SpikeImages>>,
    gpu_images: Res<RenderAssets<GpuImage>>,
    sender: Res<TextureSender>,
    mut sent: Local<bool>,
) {
    if *sent {
        return;
    }
    let Some(images) = images else { return };
    let Some(ui) = gpu_images.get(&images.ui) else {
        return;
    };
    let scene = match &images.scene {
        Some(handle) => match gpu_images.get(handle) {
            Some(g) => Some((*g.texture).clone()),
            None => return,
        },
        None => None,
    };
    let _ = sender.0.send(((*ui.texture).clone(), scene));
    *sent = true;
}

fn frame_clock(mut m: ResMut<Measure>, cfg: Res<Config>) {
    let now = Instant::now();
    m.frame += 1;
    if let Some(prev) = m.last_frame_start {
        if m.in_window(&cfg) {
            m.frame_times.push((now - prev).as_secs_f64() * 1000.0);
        }
    }
    m.last_frame_start = Some(now);
}

/// The pointer path for `--scenario sweep`: the same position for the same
/// frame in every run, covering the whole window.
fn sweep_position(frame: u32, w: f32, h: f32) -> LogicalPosition {
    let t = frame as f32;
    let x = (0.5 + 0.48 * (t * std::f32::consts::TAU / 240.0).sin()) * w;
    let y = (0.5 + 0.48 * (t * std::f32::consts::TAU / 170.0).sin()) * h;
    LogicalPosition::new(x, y)
}

fn click(window: &slint::Window, position: LogicalPosition) {
    window.dispatch_event(WindowEvent::PointerMoved { position });
    window.dispatch_event(WindowEvent::PointerPressed {
        button: PointerEventButton::Left,
        position,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        button: PointerEventButton::Left,
        position,
    });
}

fn drive_ui(
    cfg: Res<Config>,
    state: Option<NonSendMut<SlintState>>,
    receiver: Res<TextureReceiver>,
    spike_images: Option<Res<SpikeImages>>,
    ui_material: Option<Res<UiMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut m: ResMut<Measure>,
) {
    let Some(mut state_res) = state else { return };
    let state: &mut SlintState = &mut state_res;
    let frame = m.frame;
    let in_window = m.in_window(&cfg);

    // Textures from the render world.
    let received = receiver.0.lock().unwrap().try_recv();
    if let Ok((ui_tex, scene_tex)) = received {
        state.ui_texture = Some(ui_tex);
        if let (Some(scene_tex), UiInstance::Glass(glass)) = (scene_tex, &state.ui) {
            match slint::Image::try_from(scene_tex) {
                Ok(image) => {
                    glass.set_scene(image);
                    state.scene_set = true;
                }
                Err(e) => eprintln!("[spike] scene texture import failed: {e}"),
            }
        }
    }

    let adapter = state.adapter.clone();
    let window = &adapter.slint_window;
    let scale = adapter.scale_factor.get();
    let (lw, lh) = (cfg.width as f32 / scale, cfg.height as f32 / scale);

    // Scripted input, identical for both renderers.
    if !cfg.interactive {
        if let UiInstance::Glass(glass) = &state.ui {
            // Input check during warmup: one click on the button, one click in
            // the text field 120 logical px from its left edge.
            if frame == 60 {
                click(
                    window,
                    LogicalPosition::new(glass.get_button_x(), glass.get_button_y()),
                );
            }
            if frame == 80 {
                click(
                    window,
                    LogicalPosition::new(glass.get_input_x() + 120.0, glass.get_input_y()),
                );
            }
            if frame == 100 {
                m.clicks = Some(glass.get_clicks());
                m.caret = Some(glass.get_caret());
                // Move the pointer away so hover state is the same afterwards.
                window.dispatch_event(WindowEvent::PointerExited);
            }
        }
        if cfg.scenario == Scenario::Sweep && frame > cfg.warmup / 2 {
            window.dispatch_event(WindowEvent::PointerMoved {
                position: sweep_position(frame, lw, lh),
            });
        }
    }

    slint::platform::update_timers_and_animations();

    let requested = adapter.needs_redraw.get();
    if in_window && requested {
        m.redraw_requests += 1;
    }
    let forced =
        cfg.scenario == Scenario::Full || (cfg.renderer == RendererKind::Gpu && cfg.scene_under);
    if !(forced || requested || frame < 8) {
        return;
    }

    match &adapter.renderer {
        SpikeRenderer::Software(sw) => {
            adapter.needs_redraw.set(false);
            // Studio's render_slint_to_texture: paint into the staging buffer,
            // and when anything was painted copy the whole buffer into the
            // Image and touch the material so Bevy re-uploads it.
            let t0 = Instant::now();
            let region = sw.render(&mut state.staging, cfg.width as usize);
            let dirty = region.bounding_box_size();
            if dirty.width == 0 || dirty.height == 0 {
                // A redraw was asked for but nothing changed on screen: Slint
                // still walked the tree. Counted in the per-frame cost only.
                if in_window {
                    m.noop_ms_sum += t0.elapsed().as_secs_f64() * 1000.0;
                }
                return;
            }
            let share = (dirty.width as f64 * dirty.height as f64)
                / (cfg.width as f64 * cfg.height as f64)
                * 100.0;
            let t1 = Instant::now();
            if let (Some(si), Some(mat)) = (&spike_images, &ui_material) {
                if let Some(mut image) = images.get_mut(&si.ui) {
                    if let Some(data) = image.data.as_mut() {
                        let bytes: &[u8] = bytemuck_cast(&state.staging);
                        let len = data.len().min(bytes.len());
                        data[..len].copy_from_slice(&bytes[..len]);
                    }
                }
                materials.get_mut(&mat.0);
            }
            let t2 = Instant::now();
            if in_window {
                m.painted += 1;
                m.dirty_share_sum += share;
                m.ui_times.push((t2 - t0).as_secs_f64() * 1000.0);
                m.copy_times.push((t2 - t1).as_secs_f64() * 1000.0);
            }
        }
        SpikeRenderer::Gpu(gpu_renderer) => {
            let Some(texture) = state.ui_texture.clone() else {
                return;
            };
            if cfg.scene_under && !state.scene_set {
                return;
            }
            adapter.needs_redraw.set(false);
            if cfg.sync {
                // Drain earlier work so the timed wait covers only this draw.
                let _ = state.gpu.device.poll(wgpu::PollType::wait_indefinitely());
            }
            let t0 = Instant::now();
            if let Err(e) = gpu_renderer.render_to_texture(&texture) {
                m.render_errors += 1;
                if !state.render_error_reported {
                    eprintln!("[spike] render_to_texture failed: {e}");
                    state.render_error_reported = true;
                }
                return;
            }
            let t1 = Instant::now();
            if cfg.sync {
                let _ = state.gpu.device.poll(wgpu::PollType::wait_indefinitely());
            }
            let t2 = Instant::now();
            if in_window {
                m.painted += 1;
                m.dirty_share_sum += 100.0;
                m.ui_times.push((t1 - t0).as_secs_f64() * 1000.0);
                if cfg.sync {
                    m.synced_times.push((t2 - t0).as_secs_f64() * 1000.0);
                }
            }
        }
    }
}

fn bytemuck_cast(pixels: &[PremultipliedRgbaColor]) -> &[u8] {
    // PremultipliedRgbaColor is #[repr(C)] { red, green, blue, alpha: u8 }.
    unsafe {
        std::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), std::mem::size_of_val(pixels))
    }
}

/// `--interactive`: the real mouse drives the UI.
fn forward_real_input(
    cfg: Res<Config>,
    state: Option<NonSend<SlintState>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut last: Local<Option<LogicalPosition>>,
) {
    if !cfg.interactive {
        buttons.clear();
        return;
    }
    let Some(state) = state else { return };
    let Some(window) = windows.iter().next() else {
        return;
    };
    let adapter = &state.adapter;
    let scale = adapter.scale_factor.get();
    if let Some(p) = window.physical_cursor_position() {
        let position = LogicalPosition::new(p.x / scale, p.y / scale);
        if last.map_or(true, |l| {
            (l.x - position.x).abs() > 0.1 || (l.y - position.y).abs() > 0.1
        }) {
            adapter
                .slint_window
                .dispatch_event(WindowEvent::PointerMoved { position });
        }
        *last = Some(position);
    } else if last.take().is_some() {
        adapter
            .slint_window
            .dispatch_event(WindowEvent::PointerExited);
    }
    for ev in buttons.read() {
        let Some(position) = *last else { continue };
        let button = match ev.button {
            MouseButton::Left => PointerEventButton::Left,
            MouseButton::Right => PointerEventButton::Right,
            MouseButton::Middle => PointerEventButton::Middle,
            _ => PointerEventButton::Other,
        };
        let event = match ev.state {
            ButtonState::Pressed => WindowEvent::PointerPressed { button, position },
            ButtonState::Released => WindowEvent::PointerReleased { button, position },
        };
        adapter.slint_window.dispatch_event(event);
    }
}

// ============================================================================
// Report
// ============================================================================

struct Summary {
    mean: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

fn summarize(values: &[f64]) -> Summary {
    if values.is_empty() {
        return Summary {
            mean: 0.0,
            p50: 0.0,
            p95: 0.0,
            p99: 0.0,
            max: 0.0,
        };
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f64| v[(((v.len() - 1) as f64) * p).round() as usize];
    Summary {
        mean: v.iter().sum::<f64>() / v.len() as f64,
        p50: pct(0.50),
        p95: pct(0.95),
        p99: pct(0.99),
        max: *v.last().unwrap(),
    }
}

fn json(s: &Summary) -> String {
    format!(
        "{{\"mean\":{:.3},\"p50\":{:.3},\"p95\":{:.3},\"p99\":{:.3},\"max\":{:.3}}}",
        s.mean, s.p50, s.p95, s.p99, s.max
    )
}

fn report(
    mut commands: Commands,
    cfg: Res<Config>,
    label: Res<AdapterLabel>,
    mut m: ResMut<Measure>,
    mut exit: MessageWriter<AppExit>,
) {
    if let Some(path) = &cfg.screenshot {
        if !m.screenshot_taken && m.frame >= cfg.screenshot_frame {
            m.screenshot_taken = true;
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
            eprintln!(
                "[spike] screenshot requested at frame {} -> {}",
                m.frame,
                path.display()
            );
        }
    }
    if cfg.interactive || m.reported || m.frame < cfg.last_frame() {
        return;
    }
    m.reported = true;

    let frames = summarize(&m.frame_times);
    let ui = summarize(&m.ui_times);
    let copy = summarize(&m.copy_times);
    let synced = summarize(&m.synced_times);
    let measured = cfg.frames.max(1) as f64;
    let ui_per_frame = (m.ui_times.iter().sum::<f64>() + m.noop_ms_sum) / measured;
    let dirty_mean = if m.painted > 0 {
        m.dirty_share_sum / m.painted as f64
    } else {
        0.0
    };
    let line = format!(
        "{{\"label\":{:?},\"renderer\":\"{:?}\",\"ui\":\"{:?}\",\"scenario\":\"{:?}\",\"scene_under\":{},\
\"adapter\":{:?},\"pipelined\":{},\"sync\":{},\"size\":\"{}x{}\",\"frames\":{},\"painted\":{},\
\"redraw_requests\":{},\"dirty_share_mean_pct\":{:.1},\"ui_ms_per_painted_frame\":{},\
\"ui_ms_per_frame\":{:.3},\"copy_ms\":{},\"synced_ms\":{},\"frame_ms\":{},\"fps_mean\":{:.1},\
\"clicks\":{},\"caret\":{},\"render_errors\":{}}}",
        cfg.label,
        cfg.renderer,
        cfg.ui,
        cfg.scenario,
        cfg.scene_under,
        label.0,
        cfg.pipelined,
        cfg.sync,
        cfg.width,
        cfg.height,
        m.frame_times.len(),
        m.painted,
        m.redraw_requests,
        dirty_mean,
        json(&ui),
        ui_per_frame,
        json(&copy),
        json(&synced),
        json(&frames),
        if frames.mean > 0.0 { 1000.0 / frames.mean } else { 0.0 },
        m.clicks.map_or("null".to_string(), |c| c.to_string()),
        m.caret.map_or("null".to_string(), |c| c.to_string()),
        m.render_errors,
    );
    println!("{line}");
    if let Some(path) = &cfg.results {
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(mut f) => {
                let _ = writeln!(f, "{line}");
            }
            Err(e) => eprintln!("[spike] cannot write {}: {e}", path.display()),
        }
    }
    exit.write(AppExit::Success);
}

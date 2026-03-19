// ============================================================================
// Slint-Bevy Adapter — Shared wgpu Device + Headless Bevy Rendering
// ============================================================================
//
// Ported from the official Slint example:
//   https://github.com/slint-ui/slint/tree/master/examples/bevy/slint-hosts-bevy
//
// Architecture:
//   1. initialize_renderer() creates the wgpu device/queue/adapter/instance
//   2. BackendSelector::require_wgpu_27(Manual { ... }) gives Slint the shared device
//   3. Bevy runs in a separate thread with a custom runner, rendering to ManualTextureView
//   4. Triple-buffered wgpu::Textures are exchanged via smol channels
//   5. Slint imports Bevy's output via slint::Image::try_from(wgpu::Texture)
//
// Table of Contents:
//   - ControlMessage: channel messages between Slint main thread and Bevy render thread
//   - run_bevy_app_with_slint(): entry point — initializes shared device, spawns Bevy thread
//   - SlintRenderToTexturePlugin: Bevy render graph node that sends rendered textures back
//   - BackBuffer: Bevy resource holding the current render target texture
//   - SlintSwapChainDriver: render graph node that sends finished frames to Slint
// ============================================================================

#![allow(dead_code)]

use slint::wgpu_27::wgpu;

use bevy::{
    camera::RenderTarget,
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{self, NodeRunError, RenderGraph, RenderGraphContext, RenderLabel},
        renderer::RenderContext,
        settings::RenderCreation,
    },
};

// ============================================================================
// Channel Messages
// ============================================================================

/// Messages sent from the Slint main thread to the Bevy render thread.
pub enum ControlMessage {
    /// Return a previously received texture so Bevy can reuse it as a render target.
    ReleaseFrontBufferTexture { texture: wgpu::Texture },
    /// Request Bevy to resize its render target textures.
    ResizeBuffers { width: u32, height: u32 },
}

// ============================================================================
// Entry Point — Shared wgpu Device + Bevy Thread Spawn
// ============================================================================

/// Initializes a shared wgpu device, configures Slint's backend to use it,
/// spawns Bevy in a background thread, and returns channels for texture exchange.
///
/// - `bevy_app_pre_default_plugins_callback`: called before DefaultPlugins to add early plugins
/// - `bevy_main`: called with the fully configured App to add systems/plugins and call run()
///
/// Returns:
/// - `Receiver<wgpu::Texture>`: receive rendered frames from Bevy
/// - `Sender<ControlMessage>`: send control messages (release texture, resize) to Bevy
pub async fn run_bevy_app_with_slint(
    bevy_app_pre_default_plugins_callback: impl FnOnce(&mut App) + Send + 'static,
    bevy_main: impl FnOnce(App) + Send + 'static,
) -> Result<
    (smol::channel::Receiver<wgpu::Texture>, smol::channel::Sender<ControlMessage>),
    slint::PlatformError,
> {
    // Slint's Skia renderer requires specific wgpu backends per platform:
    //   Windows → D3D12 (Vulkan causes "Unsupported WGPU backend for use with Skia: vulkan")
    //   macOS   → Metal (only native GPU backend on Apple platforms)
    //   Linux   → Vulkan (primary GPU backend; Slint Skia supports Vulkan on Linux)
    // from_env() allows override via WGPU_BACKEND env var for debugging.
    #[cfg(target_os = "windows")]
    let backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::DX12);
    #[cfg(target_os = "macos")]
    let backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::METAL);
    #[cfg(target_os = "linux")]
    let backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::VULKAN);

    // Create the shared wgpu device, queue, adapter, and instance.
    // Both Slint and Bevy will use this exact same device.
    let bevy::render::settings::RenderResources(
        render_device,
        render_queue,
        adapter_info,
        adapter,
        instance,
    ) = bevy::render::renderer::initialize_renderer(
        backends,
        None,
        &bevy::render::settings::WgpuSettings::default(),
    )
    .await;

    // Log which backend was actually selected (critical for Slint Skia compatibility)
    eprintln!("[slint_bevy_adapter] Adapter: {} ({:?})", adapter_info.name, adapter_info.backend);

    // Configure Slint to use the shared wgpu device via Manual configuration.
    // This gives Slint's Skia renderer the same GPU device Bevy uses.
    let selector =
        slint::BackendSelector::new().require_wgpu_27(slint::wgpu_27::WGPUConfiguration::Manual {
            instance: (**instance.0).clone(),
            adapter: (**adapter.0).clone(),
            device: render_device.wgpu_device().clone(),
            queue: (**render_queue.0).clone(),
        });
    selector.select()?;

    // Triple-buffered texture exchange channels (bounded capacity 2)
    let (control_message_sender, control_message_receiver) =
        smol::channel::bounded::<ControlMessage>(2);
    let (bevy_front_buffer_sender, bevy_front_buffer_receiver) =
        smol::channel::bounded::<wgpu::Texture>(2);

    let wgpu_device = render_device.wgpu_device().clone();

    // Factory for creating render target textures with correct format and usage flags.
    // Rgba8UnormSrgb is required — Bevy can only render to sRGB textures.
    // See: https://github.com/bevyengine/bevy/issues/15201
    let create_texture = move |label, width, height| {
        wgpu_device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    };

    // Create initial triple buffers (front, back, inflight)
    let front_buffer = create_texture("Front Buffer", 640, 480);
    let back_buffer = create_texture("Back Buffer", 640, 480);
    let inflight_buffer = create_texture("Inflight Buffer", 640, 480);

    let mut buffer_width = 640u32;
    let mut buffer_height = 480u32;

    // Spawn Bevy in a dedicated thread with a custom runner.
    // The runner manually calls app.update() each frame and manages texture targets.
    let _bevy_thread = std::thread::spawn(move || {
        let runner = move |mut app: bevy::app::App| {
            app.finish();
            app.cleanup();

            let mut next_texture_view_id: u32 = 0;

            loop {
                // Block until Slint returns a texture for us to render into.
                // This synchronizes the frame rate with Slint's rendering notifier.
                let mut next_back_buffer = match control_message_receiver.recv_blocking() {
                    Ok(ControlMessage::ReleaseFrontBufferTexture { texture }) => texture,
                    Ok(ControlMessage::ResizeBuffers { width, height }) => {
                        buffer_width = width;
                        buffer_height = height;
                        continue;
                    }
                    Err(_) => break, // Channel closed — Slint shut down
                };

                // Resize texture if dimensions changed
                if next_back_buffer.width() != buffer_width
                    || next_back_buffer.height() != buffer_height
                {
                    next_back_buffer = create_texture("back buffer", buffer_width, buffer_height);
                }

                // Create a texture view and register it as Bevy's render target
                let texture_view = next_back_buffer.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("bevy back buffer texture view"),
                    format: None,
                    dimension: None,
                    ..Default::default()
                });
                let texture_view_handle =
                    bevy::camera::ManualTextureViewHandle(next_texture_view_id);
                next_texture_view_id += 1;
                {
                    let world = app.world_mut();

                    // Store the back buffer so the render graph node can send it to Slint
                    let Some(mut back_buffer_resource) = world.get_resource_mut::<BackBuffer>() else {
                        continue; // Resource not yet initialized
                    };
                    back_buffer_resource.0 = Some(next_back_buffer.clone());

                    // Register the texture view so Bevy's camera renders into it
                    let Some(mut manual_texture_views) = world
                        .get_resource_mut::<bevy::render::texture::ManualTextureViews>() else {
                        continue; // Resource not yet initialized
                    };
                    manual_texture_views.clear();
                    manual_texture_views.insert(
                        texture_view_handle,
                        bevy::render::texture::ManualTextureView {
                            texture_view: texture_view.into(),
                            size: (next_back_buffer.width(), next_back_buffer.height()).into(),
                            view_format:
                                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                        },
                    );

                    // Point the camera at our manual texture view.
                    // In Bevy 0.18, RenderTarget is a standalone component.
                    let mut cameras = world.query::<(&mut Camera, &mut RenderTarget)>();
                    if let Some(mut c) = cameras.iter_mut(world).next() {
                        *c.1 = bevy::camera::RenderTarget::TextureView(texture_view_handle);
                    }
                }

                // Run one Bevy frame
                app.update();
            }

            bevy::app::AppExit::Success
        };

        // Build the Bevy App with shared render resources (no window needed)
        let mut app = App::new();
        app.set_runner(runner);
        app.insert_resource(BackBuffer(None));
        bevy_app_pre_default_plugins_callback(&mut app);
        app.add_plugins(DefaultPlugins
            .set(RenderPlugin {
                render_creation: RenderCreation::manual(
                    render_device,
                    render_queue,
                    adapter_info,
                    adapter,
                    instance,
                ),
                ..default()
            })
            // Disable window creation — Slint owns the window on the main thread.
            // Bevy runs headless, rendering to a ManualTextureView.
            .disable::<bevy::winit::WinitPlugin>()
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                close_when_requested: false,
                ..default()
            })
        );
        app.add_plugins(SlintRenderToTexturePlugin(bevy_front_buffer_sender));
        app.add_plugins(ExtractResourcePlugin::<BackBuffer>::default());

        bevy_main(app)
    });

    // Seed the channel with all three buffers so Bevy can start rendering immediately
    // Seed the channel — if send fails, the Bevy thread hasn't started yet (non-fatal)
    let _ = control_message_sender
        .send_blocking(ControlMessage::ReleaseFrontBufferTexture { texture: back_buffer });
    let _ = control_message_sender
        .send_blocking(ControlMessage::ReleaseFrontBufferTexture { texture: inflight_buffer });
    let _ = control_message_sender
        .send_blocking(ControlMessage::ReleaseFrontBufferTexture { texture: front_buffer });

    Ok((bevy_front_buffer_receiver, control_message_sender))
}

// ============================================================================
// Bevy Render Graph — Send Rendered Frames to Slint
// ============================================================================

/// Resource wrapping the channel sender for returning rendered frames to Slint.
#[derive(Resource, Deref)]
struct FrontBufferReturnSender(smol::channel::Sender<wgpu::Texture>);

/// Plugin that adds the swap chain render graph node to Bevy's render pipeline.
/// This node runs after Bevy's camera driver and sends the rendered texture to Slint.
struct SlintRenderToTexturePlugin(smol::channel::Sender<wgpu::Texture>);

impl Plugin for SlintRenderToTexturePlugin {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app_mut(RenderApp);

        // Add our swap chain node to the render graph, after the camera driver
        let mut graph = render_app.world_mut().resource_mut::<RenderGraph>();
        graph.add_node(SlintSwapChain, SlintSwapChainDriver);
        graph.add_node_edge(bevy::render::graph::CameraDriverLabel, SlintSwapChain);

        render_app.insert_resource(FrontBufferReturnSender(self.0.clone()));
    }
}

/// Resource holding the current back buffer texture that Bevy renders into.
/// Extracted to the render world each frame via ExtractResourcePlugin.
#[derive(Clone, Resource, ExtractResource, Deref, DerefMut)]
struct BackBuffer(pub Option<wgpu::Texture>);

/// Render graph label for the Slint swap chain node.
#[derive(Debug, PartialEq, Eq, Clone, Hash, RenderLabel)]
struct SlintSwapChain;

/// Render graph node that sends the rendered back buffer to Slint via the channel.
#[derive(Default)]
struct SlintSwapChainDriver;

impl render_graph::Node for SlintSwapChainDriver {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        _render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let Some(front_buffer_sender) = world.get_resource::<FrontBufferReturnSender>() else {
            return Ok(()); // Resource not available — skip this frame
        };
        let Some(back_buffer) = world.get_resource::<BackBuffer>() else {
            return Ok(()); // Resource not available — skip this frame
        };

        if let Some(bb) = &back_buffer.0 {
            // Send the rendered texture to Slint. Silently ignore closed channel
            // (indicates shutdown — panicking here would crash Bevy's render thread).
            front_buffer_sender.0.send_blocking(bb.clone()).ok();
        }

        Ok(())
    }
}

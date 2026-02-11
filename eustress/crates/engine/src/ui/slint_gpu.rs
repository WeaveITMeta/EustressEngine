// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: MIT
// Adapted for Eustress Engine - GPU Renderer version

//! This module provides GPU-accelerated Slint UI rendering integrated with Bevy.
//!
//! The integration uses Slint's wgpu_28 renderer to render UI directly to a wgpu texture
//! that is shared with Bevy's render pipeline.

use bevy::{
    prelude::*,
    render::{
        renderer::{RenderDevice, RenderQueue},
        Render,
    },
    window::PrimaryWindow,
};
use bevy::log::{info, warn, error};

use slint::wgpu_28::{
    wgpu::{self, TextureFormat, TextureUsages},
    WGPUConfiguration,
};

/// Resource holding the Slint UI instance with GPU renderer
pub struct SlintGpuState {
    /// The Slint window instance
    pub window: slint::Window,
    /// WGPU texture for Slint rendering
    pub texture: wgpu::Texture,
    /// Texture view for rendering
    pub texture_view: wgpu::TextureView,
    /// Texture dimensions
    pub size: (u32, u32),
    /// Whether UI needs redraw
    pub needs_redraw: bool,
}

/// Resource to track if GPU renderer has been initialized
#[derive(Resource, Default)]
pub struct SlintGpuInitialized(pub bool);

/// Resource holding the overlay texture handle for Bevy
#[derive(Resource)]
pub struct SlintOverlayTexture(pub Handle<Image>);

/// Plugin for Slint GPU renderer integration
pub struct SlintGpuPlugin;

impl Plugin for SlintGpuPlugin {
    fn build(&self, app: &mut App) {
        info!("🎨 Initializing Slint GPU renderer plugin");

        app.init_resource::<SlintGpuInitialized>()
            .add_systems(Startup, setup_slint_gpu)
            .add_systems(Update, (
                update_slint_rendering,
                handle_window_resize_gpu,
            ));
    }
}

/// Setup function to initialize Slint with GPU renderer
fn setup_slint_gpu(
    mut commands: Commands,
    mut initialized: ResMut<SlintGpuInitialized>,
    windows: Query<&Window, With<PrimaryWindow>>,
    render_device: Res<RenderDevice>,
    _render_queue: Res<RenderQueue>,
    mut images: ResMut<Assets<Image>>,
) {
    if initialized.0 {
        return;
    }

    let window = match windows.iter().next() {
        Some(w) => w,
        None => {
            warn!("No primary window found, retrying next frame...");
            return;
        }
    };

    let width = window.width() as u32;
    let height = window.height() as u32;

    if width == 0 || height == 0 {
        warn!("Window has zero size, retrying next frame...");
        return;
    }

    info!("🎨 Setting up Slint GPU renderer ({}x{})", width, height);

    // Create wgpu texture for Slint rendering
    let texture = create_slint_texture(&render_device, width, height);
    let _texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Create Bevy Image from the wgpu texture
    let size = bevy::render::render_resource::Extent3d { width, height, depth_or_array_layers: 1 };
    let mut image = Image {
        texture_descriptor: bevy::render::render_resource::TextureDescriptor {
            label: Some("SlintGpuOverlay"),
            size,
            dimension: bevy::render::render_resource::TextureDimension::D2,
            format: bevy::render::render_resource::TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: 1,
            usage: bevy::render::render_resource::TextureUsages::TEXTURE_BINDING
                | bevy::render::render_resource::TextureUsages::COPY_DST
                | bevy::render::render_resource::TextureUsages::COPY_SRC
                | bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size);
    
    // Fill with initial transparent test pattern
    if let Some(data) = image.data.as_mut() {
        for chunk in data.chunks_exact_mut(4) {
            chunk[0] = 0;   // R
            chunk[1] = 100; // G (slight tint to show it's working)
            chunk[2] = 0;   // B
            chunk[3] = 100; // A (semi-transparent)
        }
    }
    
    let texture_handle = images.add(image);

    // TODO: Initialize Slint with wgpu 26 configuration
    // This requires access to the adapter and instance which are not directly available
    // from Bevy's RenderDevice/RenderQueue. We'll need to use a different approach.
    
    // For now, create a placeholder Slint window using the software renderer
    // and render to the texture
    let slint_window = create_slint_window_software();

    // Store the Slint state as NonSend resource
    let slint_state = SlintGpuState {
        window: slint_window,
        texture,
        texture_view: _texture_view,
        size: (width, height),
        needs_redraw: true,
    };

    commands.insert_non_send_resource(slint_state);
    commands.insert_resource(SlintOverlayTexture(texture_handle.clone()));
    
    // Spawn overlay camera (renders on top of 3D scene)
    commands.spawn((
        Camera2d,
        Camera {
            order: 100, // Render after 3D camera
            clear_color: ClearColorConfig::None, // Don't clear - overlay on top
            ..default()
        },
        Name::new("Slint GPU Overlay Camera"),
    ));
    info!("✅ Spawned overlay camera (order: 100)");
    
    // Spawn overlay sprite (fullscreen)
    commands.spawn((
        Sprite {
            image: texture_handle,
            custom_size: Some(Vec2::new(width as f32, height as f32)),
            color: Color::WHITE,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, -1.0),
        Visibility::Visible,
        Name::new("Slint GPU Overlay Sprite"),
    ));
    info!("✅ Spawned overlay sprite ({}x{})", width, height);

    initialized.0 = true;
    info!("✅ Slint GPU renderer initialized");
}

/// Create a wgpu texture for Slint rendering
fn create_slint_texture(
    render_device: &RenderDevice,
    width: u32,
    height: u32,
) -> wgpu::Texture {
    render_device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Slint GPU Texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_DST
            | TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Create Slint window using software renderer (fallback until GPU renderer is fully implemented)
fn create_slint_window_software() -> slint::Window {
    // Initialize software platform
    let platform_window = slint::platform::software_renderer::MinimalSoftwareWindow::new(
        slint::platform::software_renderer::RepaintBufferType::ReusedBuffer
    );
    
    slint::platform::set_platform(Box::new(SlintSoftwarePlatform {
        window: platform_window.clone(),
    })).expect("Failed to set Slint platform");
    
    // Create the window
    slint::Window::default()
}

/// Software platform for Slint (fallback)
struct SlintSoftwarePlatform {
    window: std::rc::Rc<slint::platform::software_renderer::MinimalSoftwareWindow>,
}

impl slint::platform::Platform for SlintSoftwarePlatform {
    fn create_window_adapter(&self) -> Result<std::rc::Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
    
    fn duration_since_start(&self) -> core::time::Duration {
        core::time::Duration::from_millis(0)
    }
}

/// Update system to render Slint UI
fn update_slint_rendering(
    mut slint_state: Option<NonSendMut<SlintGpuState>>,
    overlay_texture: Option<Res<SlintOverlayTexture>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(slint_state) = slint_state else { return };
    let Some(overlay_texture) = overlay_texture else { return };

    if !slint_state.needs_redraw {
        return;
    }

    // Get the image to render to
    let Some(image) = images.get_mut(&overlay_texture.0) else { return };
    
    // TODO: Implement actual Slint rendering
    // For now, draw a test pattern to show the overlay is working
    if let Some(data) = image.data.as_mut() {
        let width = slint_state.size.0 as usize;
        let height = slint_state.size.1 as usize;
        
        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) * 4;
                if idx + 3 < data.len() {
                    // Blue-green gradient test pattern
                    data[idx] = (x * 255 / width.max(1)) as u8;     // R
                    data[idx + 1] = (y * 255 / height.max(1)) as u8; // G
                    data[idx + 2] = 200;                            // B
                    data[idx + 3] = 200;                            // A
                }
            }
        }
    }

    // slint_state.needs_redraw = false;
}

/// Handle window resize for GPU renderer
fn handle_window_resize_gpu(
    mut slint_state: Option<NonSendMut<SlintGpuState>>,
    windows: Query<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    render_device: Res<RenderDevice>,
) {
    let Some(mut slint_state) = slint_state else { return };
    let Some(window) = windows.iter().next() else { return };

    let new_width = window.width() as u32;
    let new_height = window.height() as u32;

    if new_width == slint_state.size.0 && new_height == slint_state.size.1 {
        return;
    }

    if new_width == 0 || new_height == 0 {
        return;
    }

    info!("📐 Resizing Slint GPU texture: {}x{} -> {}x{}",
        slint_state.size.0, slint_state.size.1, new_width, new_height);

    // Recreate texture with new size
    slint_state.texture = create_slint_texture(&render_device, new_width, new_height);
    slint_state.texture_view = slint_state.texture.create_view(&wgpu::TextureViewDescriptor::default());
    slint_state.size = (new_width, new_height);
    slint_state.needs_redraw = true;
}

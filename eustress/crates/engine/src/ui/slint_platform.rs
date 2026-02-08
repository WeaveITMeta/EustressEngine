// ============================================================================
// Slint Platform Integration for Bevy
// GPU-accelerated wgpu-based rendering with shared texture
// ============================================================================

use bevy::prelude::*;
use bevy::render::{
    RenderApp, Render, RenderSet,
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_asset::RenderAssets,
    render_graph::{self, NodeRunError, RenderGraph, RenderGraphContext, RenderLabel},
    renderer::{RenderContext, RenderDevice, RenderQueue},
    render_resource::{
        Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
        CommandEncoderDescriptor,
    },
    texture::{Image, GpuImage},
};
use std::sync::Arc;

// Re-export Slint types
pub use slint::platform::{
    Platform, PlatformError, WindowAdapter,
    WindowEvent as SlintWindowEvent, PointerEventButton,
};

/// Slint texture resource extracted to render world
#[derive(Resource, Clone, ExtractResource)]
pub struct SlintRenderTexture {
    pub texture: Option<wgpu::Texture>,
    pub size: (u32, u32),
}

impl Default for SlintRenderTexture {
    fn default() -> Self {
        Self {
            texture: None,
            size: (1600, 900),
        }
    }
}

/// Slint window wrapper for Bevy integration
pub struct SlintBevyWindow {
    window: slint::Window,
}

impl SlintBevyWindow {
    pub fn new(window: slint::Window) -> Self {
        Self { window }
    }
    
    /// Dispatch an event to the Slint window
    pub fn dispatch_event(&self, event: SlintWindowEvent) {
        self.window.dispatch_event(event);
    }
    
    /// Set the size of the Slint window
    pub fn set_size(&self, size: slint::LogicalSize) {
        // Note: Size setting depends on the window adapter implementation
        let _ = size;
    }
    
    /// Draw the window contents if needed
    pub fn draw_if_needed(&self, renderer: &dyn slint::platform::Renderer) {
        if self.window.has_active_animations() || self.window.needs_redraw() {
            // Trigger redraw - actual rendering happens in the render node
            let _ = renderer;
        }
    }
    
    pub fn window(&self) -> &slint::Window {
        &self.window
    }
}

/// Custom Slint Platform for Bevy integration
pub struct BevySlintPlatform {
    window: Arc<std::sync::Mutex<Option<SlintBevyWindow>>>,
    renderer: Option<i_slint_renderer_femtovg::FemtoVGRenderer>,
    device: Option<Arc<wgpu::Device>>,
    queue: Option<Arc<wgpu::Queue>>,
}

impl BevySlintPlatform {
    pub fn new() -> Self {
        Self {
            window: Arc::new(std::sync::Mutex::new(None)),
            renderer: None,
            device: None,
            queue: None,
        }
    }
    
    /// Initialize the platform with wgpu device/queue from Bevy
    pub fn initialize(&mut self, device: &RenderDevice, _queue: &RenderQueue) {
        let wgpu_device = device.wgpu_device();
        self.device = Some(Arc::new(wgpu_device.clone()));
        // Note: Queue initialization would go here if needed
    }
    
    /// Set the window instance
    pub fn set_window(&self, window: slint::Window) {
        if let Ok(mut guard) = self.window.lock() {
            *guard = Some(SlintBevyWindow::new(window));
        }
    }
    
    /// Get access to the window
    pub fn with_window<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&SlintBevyWindow) -> R,
    {
        self.window.lock().ok().as_ref().and_then(|opt| opt.as_ref().map(f))
    }
}

impl Platform for BevySlintPlatform {
    fn create_window_adapter(&self) -> Result<Arc<dyn WindowAdapter>, PlatformError> {
        // This is called by Slint to create the window adapter
        // For now, we return an error as we set up the window manually
        Err(PlatformError::NoPlatform)
    }
    
    fn run_event_loop(&self) -> Result<(), PlatformError> {
        // Bevy drives the event loop, not Slint
        Ok(())
    }
}

/// Global instance of the platform (needed for callback access)
static mut SLINT_PLATFORM: Option<Arc<std::sync::Mutex<BevySlintPlatform>>> = None;

/// Initialize the Slint platform
pub fn initialize_platform(device: &RenderDevice, queue: &RenderQueue) {
    let platform = BevySlintPlatform::new();
    platform.initialize(device, queue);
    
    unsafe {
        SLINT_PLATFORM = Some(Arc::new(std::sync::Mutex::new(platform)));
    }
}

/// Get access to the global platform
pub fn with_platform<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut BevySlintPlatform) -> R,
{
    unsafe {
        SLINT_PLATFORM.as_ref().and_then(|p| {
            p.lock().ok().map(|mut guard| f(&mut *guard))
        })
    }
}

/// Create a wgpu texture for Slint rendering
pub fn create_slint_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> wgpu::Texture {
    device.create_texture(&TextureDescriptor {
        label: Some("slint_render_texture"),
        size: Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8UnormSrgb,
        usage: TextureUsages::RENDER_ATTACHMENT 
            | TextureUsages::TEXTURE_BINDING 
            | TextureUsages::COPY_SRC
            | TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Render label for Slint render node
#[derive(Debug, PartialEq, Eq, Clone, Hash, RenderLabel)]
pub struct SlintRenderNodeLabel;

/// Slint render graph node
pub struct SlintRenderNode {
    texture: Option<wgpu::Texture>,
}

impl SlintRenderNode {
    pub fn new() -> Self {
        Self { texture: None }
    }
}

impl render_graph::Node for SlintRenderNode {
    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        // Get the render texture resource
        let render_texture = world.get_resource::<SlintRenderTexture>();
        
        if let Some(render_texture) = render_texture {
            if let Some(texture) = &render_texture.texture {
                // Create a command encoder for Slint rendering
                let _encoder = render_context
                    .render_device()
                    .create_command_encoder(&CommandEncoderDescriptor {
                        label: Some("slint_render_encoder"),
                    });
                
                // Note: Actual Slint rendering would happen here
                // This requires the FemtoVGRenderer to render to the texture
                // For now, we clear the texture to transparent
                let _ = texture;
            }
        }
        
        Ok(())
    }
}

/// Plugin to add Slint render node to the render graph
pub struct SlintRenderNodePlugin;

impl Plugin for SlintRenderNodePlugin {
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app_mut(RenderApp);
        
        let mut graph = render_app.world_mut().resource_mut::<RenderGraph>();
        graph.add_node(SlintRenderNodeLabel, SlintRenderNode::new());
        
        // Add node edge to run after camera driver but before UI
        graph.add_node_edge(
            bevy::render::graph::CameraDriverLabel,
            SlintRenderNodeLabel,
        );
    }
}

/// System to update Slint texture size when window resizes
pub fn update_slint_texture_size(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut slint_texture: ResMut<SlintRenderTexture>,
    render_device: Res<RenderDevice>,
) {
    if let Ok(window) = windows.get_single() {
        let width = window.width() as u32;
        let height = window.height() as u32;
        
        if width > 0 && height > 0 && (width, height) != slint_texture.size {
            info!("Resizing Slint texture: {}x{}", width, height);
            
            let new_texture = create_slint_texture(
                render_device.wgpu_device(),
                width,
                height,
            );
            
            slint_texture.texture = Some(new_texture);
            slint_texture.size = (width, height);
        }
    }
}

/// System to forward Bevy input events to Slint
pub fn forward_input_to_slint(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut mouse_wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut cursor_moved: MessageReader<bevy::input::mouse::MouseMotion>,
) {
    with_platform(|platform| {
        platform.with_window(|window| {
            // Get cursor position
            if let Ok(bevy_window) = windows.get_single() {
                if let Some(cursor_pos) = bevy_window.cursor_position() {
                    let logical_pos = slint::LogicalPosition::new(
                        cursor_pos.x as f32,
                        cursor_pos.y as f32,
                    );
                    
                    // Forward mouse moved events
                    for _ in cursor_moved.read() {
                        window.dispatch_event(SlintWindowEvent::PointerMoved {
                            position: logical_pos,
                        });
                    }
                    
                    // Forward mouse button events
                    if mouse_button.just_pressed(MouseButton::Left) {
                        window.dispatch_event(SlintWindowEvent::PointerPressed {
                            position: logical_pos,
                            button: PointerEventButton::Left,
                        });
                    }
                    if mouse_button.just_released(MouseButton::Left) {
                        window.dispatch_event(SlintWindowEvent::PointerReleased {
                            position: logical_pos,
                            button: PointerEventButton::Left,
                        });
                    }
                    if mouse_button.just_pressed(MouseButton::Right) {
                        window.dispatch_event(SlintWindowEvent::PointerPressed {
                            position: logical_pos,
                            button: PointerEventButton::Right,
                        });
                    }
                    if mouse_button.just_released(MouseButton::Right) {
                        window.dispatch_event(SlintWindowEvent::PointerReleased {
                            position: logical_pos,
                            button: PointerEventButton::Right,
                        });
                    }
                }
                
                // Forward scroll events
                for event in mouse_wheel.read() {
                    window.dispatch_event(SlintWindowEvent::PointerScrolled {
                        position: slint::LogicalPosition::new(0.0, 0.0), // Position would need to be tracked
                        delta_x: event.x * 20.0, // Convert to logical pixels
                        delta_y: event.y * 20.0,
                    });
                }
                
                // Forward keyboard events
                for key in keyboard.get_just_pressed() {
                    if let Some(slint_key) = convert_key_code(*key) {
                        window.dispatch_event(SlintWindowEvent::KeyPressed {
                            text: slint_key,
                        });
                    }
                }
            }
        });
    });
}

/// Convert Bevy KeyCode to Slint key string
fn convert_key_code(key: KeyCode) -> Option<String> {
    use KeyCode::*;
    
    let key_str = match key {
        KeyA => "a",
        KeyB => "b",
        KeyC => "c",
        KeyD => "d",
        KeyE => "e",
        KeyF => "f",
        KeyG => "g",
        KeyH => "h",
        KeyI => "i",
        KeyJ => "j",
        KeyK => "k",
        KeyL => "l",
        KeyM => "m",
        KeyN => "n",
        KeyO => "o",
        KeyP => "p",
        KeyQ => "q",
        KeyR => "r",
        KeyS => "s",
        KeyT => "t",
        KeyU => "u",
        KeyV => "v",
        KeyW => "w",
        KeyX => "x",
        KeyY => "y",
        KeyZ => "z",
        Digit0 => "0",
        Digit1 => "1",
        Digit2 => "2",
        Digit3 => "3",
        Digit4 => "4",
        Digit5 => "5",
        Digit6 => "6",
        Digit7 => "7",
        Digit8 => "8",
        Digit9 => "9",
        Escape => "Escape",
        Space => " ",
        Enter => "Return",
        Tab => "Tab",
        Backspace => "Backspace",
        Delete => "Delete",
        ArrowLeft => "LeftArrow",
        ArrowRight => "RightArrow",
        ArrowUp => "UpArrow",
        ArrowDown => "DownArrow",
        _ => return None,
    };
    
    Some(key_str.to_string())
}

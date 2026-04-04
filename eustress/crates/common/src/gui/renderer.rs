//! Slint GUI renderer — per-instance adapter, texture creation, and frame rendering.

use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use std::cell::Cell;
use std::rc::{Rc, Weak};

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

/// Attached to every rendered GUI entity. Tracks the texture and rendering state.
#[derive(Component)]
pub struct SlintGuiInstance {
    pub image: Handle<Image>,
    pub material: Handle<StandardMaterial>,
    pub width: u32,
    pub height: u32,
    pub gui_type: SlintGuiType,
}

/// Rendering mode for a GUI instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlintGuiType {
    /// 2D screen overlay (Bevy UI ImageNode)
    Screen,
    /// 3D billboard quad that always faces the camera
    Billboard,
    /// 3D quad attached to a specific face of a part
    Surface,
}

/// Marker component for GUI quads that receive input via raycasting.
#[derive(Component)]
pub struct SlintGuiQuad;

// ---------------------------------------------------------------------------
// Per-instance Slint Window Adapter
// ---------------------------------------------------------------------------

/// Lightweight Slint window adapter for a single GUI instance.
/// Each ScreenGui, BillboardGui, or SurfaceGui gets its own adapter.
pub struct GuiWindowAdapter {
    pub size: Cell<slint::PhysicalSize>,
    pub scale_factor: Cell<f32>,
    pub slint_window: slint::Window,
    pub software_renderer: slint::platform::software_renderer::SoftwareRenderer,
}

impl slint::platform::WindowAdapter for GuiWindowAdapter {
    fn window(&self) -> &slint::Window { &self.slint_window }
    fn size(&self) -> slint::PhysicalSize { self.size.get() }
    fn renderer(&self) -> &dyn slint::platform::Renderer { &self.software_renderer }
    fn set_visible(&self, _visible: bool) -> Result<(), slint::PlatformError> { Ok(()) }
    fn request_redraw(&self) {}
}

impl GuiWindowAdapter {
    /// Create a new adapter with the given pixel dimensions and scale factor.
    pub fn new(width: u32, height: u32, scale_factor: f32) -> Rc<Self> {
        Rc::new_cyclic(|self_weak: &Weak<Self>| {
            let adapter = Self {
                size: Cell::new(slint::PhysicalSize::new(width, height)),
                scale_factor: Cell::new(scale_factor),
                slint_window: slint::Window::new(self_weak.clone()),
                software_renderer: Default::default(),
            };
            adapter.slint_window.dispatch_event(slint::platform::WindowEvent::Resized {
                size: adapter.size.get().to_logical(scale_factor),
            });
            adapter.slint_window.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
                scale_factor,
            });
            adapter.slint_window.dispatch_event(slint::platform::WindowEvent::WindowActiveChanged(true));
            adapter
        })
    }

    /// Resize the adapter (called when texture size changes).
    pub fn resize(&self, width: u32, height: u32) {
        let new_size = slint::PhysicalSize::new(width, height);
        let sf = self.scale_factor.get();
        self.size.set(new_size);
        self.slint_window.dispatch_event(slint::platform::WindowEvent::Resized {
            size: new_size.to_logical(sf),
        });
    }
}

// ---------------------------------------------------------------------------
// Adapter Registry (NonSend — Slint uses Rc, not Arc)
// ---------------------------------------------------------------------------

/// Tracks all active GUI adapters. Stored as NonSend because Slint is not thread-safe.
#[derive(Default)]
pub struct SlintGuiAdapters {
    pub entries: Vec<(Entity, Rc<GuiWindowAdapter>)>,
}

impl SlintGuiAdapters {
    /// Register a new adapter for an entity.
    pub fn insert(&mut self, entity: Entity, adapter: Rc<GuiWindowAdapter>) {
        self.entries.push((entity, adapter));
    }

    /// Get the adapter for an entity.
    pub fn get(&self, entity: Entity) -> Option<&Rc<GuiWindowAdapter>> {
        self.entries.iter().find(|(e, _)| *e == entity).map(|(_, a)| a)
    }

    /// Remove the adapter for a despawned entity.
    pub fn remove(&mut self, entity: Entity) {
        self.entries.retain(|(e, _)| *e != entity);
    }
}

// ---------------------------------------------------------------------------
// Texture Creation
// ---------------------------------------------------------------------------

/// Create a Bevy Image + StandardMaterial for a GUI instance.
/// The material is unlit, alpha-blended, and double-sided.
pub fn create_gui_texture(
    width: u32,
    height: u32,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> (Handle<Image>, Handle<StandardMaterial>) {
    let size = Extent3d { width, height, depth_or_array_layers: 1 };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("SlintGUI"),
            size,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(size);
    let image_handle = images.add(image);

    let material_handle = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle.clone()),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });

    (image_handle, material_handle)
}

// ---------------------------------------------------------------------------
// Per-frame Rendering System
// ---------------------------------------------------------------------------

/// Render all active Slint GUI instances to their textures.
pub fn render_slint_guis(
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    gui_instances: Query<(Entity, &SlintGuiInstance)>,
    adapters: Option<NonSend<SlintGuiAdapters>>,
) {
    let Some(adapters) = adapters else { return };

    slint::platform::update_timers_and_animations();

    for (entity, gui) in &gui_instances {
        let Some(adapter) = adapters.get(entity) else { continue };

        let current = adapter.size.get();
        if current.width != gui.width || current.height != gui.height {
            adapter.resize(gui.width, gui.height);
        }

        let Some(image) = images.get_mut(&gui.image) else { continue };
        if let Some(data) = image.data.as_mut() {
            adapter.software_renderer.render(
                bytemuck::cast_slice_mut::<u8, slint::platform::software_renderer::PremultipliedRgbaColor>(data),
                gui.width as usize,
            );
        }

        // Force GPU re-upload
        materials.get_mut(&gui.material);
    }
}

/// Rotate billboard GUI quads to always face the main camera.
pub fn billboard_face_camera(
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    mut billboard_query: Query<(&mut Transform, &GlobalTransform, &SlintGuiInstance), With<SlintGuiQuad>>,
) {
    let Some(cam_transform) = camera_query.iter().next() else { return };
    let cam_pos = cam_transform.translation();

    for (mut transform, global, gui) in &mut billboard_query {
        if gui.gui_type != SlintGuiType::Billboard { continue; }
        let pos = global.translation();
        let direction = (cam_pos - pos).normalize_or_zero();
        if direction.length_squared() > 0.01 {
            transform.look_to(direction, Vec3::Y);
        }
    }
}

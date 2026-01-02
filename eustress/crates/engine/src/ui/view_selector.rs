#![allow(dead_code)]

use bevy::prelude::*;
use bevy_egui::egui;

/// Camera view modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Perspective,
    Orthographic,
    Top,
    Front,
    Side,      // Left side (Numpad 4)
    SideRight, // Right side (Numpad 6)
}

/// View selector state
#[derive(Resource)]
pub struct ViewSelectorState {
    pub current_mode: ViewMode,
    pub show_grid: bool,
    pub show_wireframe: bool,
    pub show_gizmos: bool,
    /// Saved camera position before switching to orthographic view
    pub saved_camera_pos: Option<Vec3>,
    /// Saved camera rotation before switching to orthographic view  
    pub saved_camera_rot: Option<Quat>,
}

impl Default for ViewSelectorState {
    fn default() -> Self {
        Self {
            current_mode: ViewMode::Perspective,
            show_grid: true,
            show_wireframe: false,
            show_gizmos: true,
            saved_camera_pos: None,
            saved_camera_rot: None,
        }
    }
}

/// Event emitted when view mode changes
#[derive(Message)]
pub struct ViewModeChanged {
    pub new_mode: ViewMode,
}

/// View selector UI component (for toolbar)
pub struct ViewSelectorWidget;

impl ViewSelectorWidget {
    pub fn show(ui: &mut egui::Ui, state: &mut ViewSelectorState) -> bool {
        let mut changed = false;
        
        ui.menu_button("👁 View", |ui| {
            ui.label("Camera Mode:");
            ui.separator();
            
            // Perspective mode
            let persp_selected = state.current_mode == ViewMode::Perspective;
            if ui.selectable_label(persp_selected, if persp_selected { "✓ 📐 Perspective" } else { "  📐 Perspective" }).clicked() {
                state.current_mode = ViewMode::Perspective;
                changed = true;
                ui.close();
            }
            
            // Orthographic mode (keeps current camera direction)
            let ortho_selected = state.current_mode == ViewMode::Orthographic;
            if ui.selectable_label(ortho_selected, if ortho_selected { "✓ ⬜ Orthographic" } else { "  ⬜ Orthographic" }).clicked() {
                state.current_mode = ViewMode::Orthographic;
                changed = true;
                ui.close();
            }
            
            ui.separator();
            ui.label("Orthographic Views:");
            
            // Top view
            let top_selected = state.current_mode == ViewMode::Top;
            if ui.selectable_label(top_selected, if top_selected { "✓ ⬇ Top" } else { "  ⬇ Top" }).clicked() {
                state.current_mode = ViewMode::Top;
                changed = true;
                ui.close();
            }
            
            // Front view
            let front_selected = state.current_mode == ViewMode::Front;
            if ui.selectable_label(front_selected, if front_selected { "✓ ➡ Front" } else { "  ➡ Front" }).clicked() {
                state.current_mode = ViewMode::Front;
                changed = true;
                ui.close();
            }
            
            // Side view
            let side_selected = state.current_mode == ViewMode::Side;
            if ui.selectable_label(side_selected, if side_selected { "✓ ⬅ Side" } else { "  ⬅ Side" }).clicked() {
                state.current_mode = ViewMode::Side;
                changed = true;
                ui.close();
            }
            
            ui.separator();
            ui.label("Overlays:");
            
            // Grid checkbox with checkmark
            let grid_label = if state.show_grid { "✓ Show Grid" } else { "  Show Grid" };
            if ui.selectable_label(state.show_grid, grid_label).clicked() {
                state.show_grid = !state.show_grid;
                changed = true;
            }
            
            // Wireframe checkbox with checkmark
            let wire_label = if state.show_wireframe { "✓ Wireframe Mode" } else { "  Wireframe Mode" };
            if ui.selectable_label(state.show_wireframe, wire_label).clicked() {
                state.show_wireframe = !state.show_wireframe;
                changed = true;
            }
            
            // Gizmos checkbox with checkmark
            let gizmo_label = if state.show_gizmos { "✓ Show Gizmos" } else { "  Show Gizmos" };
            if ui.selectable_label(state.show_gizmos, gizmo_label).clicked() {
                state.show_gizmos = !state.show_gizmos;
                changed = true;
            }
        });
        
        // Show current mode as a label
        ui.label(format!("{:?}", state.current_mode));
        
        changed
    }
}

/// System to handle view mode changes - preserves camera position for orthographic views
pub fn handle_view_mode_changes(
    mut view_state: ResMut<ViewSelectorState>,
    mut camera_query: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
) {
    if !view_state.is_changed() {
        return;
    }
    
    for (mut transform, mut projection) in camera_query.iter_mut() {
        // Get current camera position to use as reference point
        let current_pos = transform.translation;
        let current_rot = transform.rotation;
        
        // Calculate ortho scale based on camera height/distance from origin
        // This keeps the view at a similar "zoom level" to what the user was seeing
        let camera_height = current_pos.y.abs().max(5.0);
        let camera_distance = current_pos.length().max(10.0);
        let ortho_scale = (camera_height * 0.5).max(camera_distance * 0.15).clamp(5.0, 100.0);
        
        match view_state.current_mode {
            ViewMode::Perspective => {
                // Restore saved position if available
                if let Some(saved_pos) = view_state.saved_camera_pos.take() {
                    transform.translation = saved_pos;
                }
                if let Some(saved_rot) = view_state.saved_camera_rot.take() {
                    transform.rotation = saved_rot;
                }
                
                *projection = Projection::Perspective(PerspectiveProjection {
                    fov: std::f32::consts::PI / 4.0,
                    aspect_ratio: 16.0 / 9.0,
                    near: 0.1,
                    far: 1000.0,
                });
            }
            ViewMode::Orthographic => {
                // Save current position before switching (only if coming from perspective)
                if view_state.saved_camera_pos.is_none() {
                    view_state.saved_camera_pos = Some(current_pos);
                    view_state.saved_camera_rot = Some(current_rot);
                }
                
                // Keep current camera position and rotation, just change projection
                let mut ortho = OrthographicProjection::default_3d();
                ortho.scale = ortho_scale;
                ortho.near = -1000.0;
                ortho.far = 1000.0;
                *projection = Projection::Orthographic(ortho);
            }
            ViewMode::Top => {
                // Save position if coming from perspective
                if view_state.saved_camera_pos.is_none() {
                    view_state.saved_camera_pos = Some(current_pos);
                    view_state.saved_camera_rot = Some(current_rot);
                }
                
                let mut ortho = OrthographicProjection::default_3d();
                ortho.scale = ortho_scale;
                ortho.near = -1000.0;
                ortho.far = 1000.0;
                *projection = Projection::Orthographic(ortho);
                
                // Keep camera at same XZ position, just move to look straight down
                // Use current Y height (or reasonable default) to maintain view
                let y_height = current_pos.y.abs().max(50.0);
                transform.translation = Vec3::new(current_pos.x, y_height, current_pos.z);
                transform.look_at(Vec3::new(current_pos.x, 0.0, current_pos.z), Vec3::NEG_Z);
            }
            ViewMode::Front => {
                // Save position if coming from perspective
                if view_state.saved_camera_pos.is_none() {
                    view_state.saved_camera_pos = Some(current_pos);
                    view_state.saved_camera_rot = Some(current_rot);
                }
                
                let mut ortho = OrthographicProjection::default_3d();
                ortho.scale = ortho_scale;
                ortho.near = -1000.0;
                ortho.far = 1000.0;
                *projection = Projection::Orthographic(ortho);
                
                // Keep camera at same XY position, look along -Z axis
                let z_distance = current_pos.z.abs().max(50.0);
                transform.translation = Vec3::new(current_pos.x, current_pos.y, z_distance);
                transform.look_at(Vec3::new(current_pos.x, current_pos.y, 0.0), Vec3::Y);
            }
            ViewMode::Side => {
                // Save position if coming from perspective
                if view_state.saved_camera_pos.is_none() {
                    view_state.saved_camera_pos = Some(current_pos);
                    view_state.saved_camera_rot = Some(current_rot);
                }
                
                let mut ortho = OrthographicProjection::default_3d();
                ortho.scale = ortho_scale;
                ortho.near = -1000.0;
                ortho.far = 1000.0;
                *projection = Projection::Orthographic(ortho);
                
                // Keep camera at same YZ position, look along -X axis (from left)
                let x_distance = current_pos.x.abs().max(50.0);
                transform.translation = Vec3::new(x_distance, current_pos.y, current_pos.z);
                transform.look_at(Vec3::new(0.0, current_pos.y, current_pos.z), Vec3::Y);
            }
            ViewMode::SideRight => {
                // Save position if coming from perspective
                if view_state.saved_camera_pos.is_none() {
                    view_state.saved_camera_pos = Some(current_pos);
                    view_state.saved_camera_rot = Some(current_rot);
                }
                
                let mut ortho = OrthographicProjection::default_3d();
                ortho.scale = ortho_scale;
                ortho.near = -1000.0;
                ortho.far = 1000.0;
                *projection = Projection::Orthographic(ortho);
                
                // Keep camera at same YZ position, look along +X axis (from right)
                let x_distance = current_pos.x.abs().max(50.0);
                transform.translation = Vec3::new(-x_distance, current_pos.y, current_pos.z);
                transform.look_at(Vec3::new(0.0, current_pos.y, current_pos.z), Vec3::Y);
            }
        }
    }
}

/// System to apply wireframe mode
pub fn apply_wireframe_mode(
    view_state: Res<ViewSelectorState>,
    mut wireframe_config: Option<ResMut<bevy::pbr::wireframe::WireframeConfig>>,
) {
    if !view_state.is_changed() {
        return;
    }
    
    if let Some(ref mut config) = wireframe_config {
        config.global = view_state.show_wireframe;
    }
}

/// System to handle Blender-style numpad shortcuts for camera view modes
/// Numpad 5: Toggle Perspective/Orthographic
/// Numpad 8: Top view
/// Numpad 2: Front view  
/// Numpad 4: Left side view
/// Numpad 6: Right side view
pub fn handle_view_mode_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    keybindings: Res<crate::keybindings::KeyBindings>,
    mut view_state: ResMut<ViewSelectorState>,
    egui_state: Res<super::EguiInputState>,
) {
    // Don't process shortcuts when UI has keyboard focus
    if egui_state.wants_keyboard {
        return;
    }
    
    use crate::keybindings::Action;
    
    // Numpad 5: Toggle Perspective/Orthographic
    if keybindings.check(Action::ViewPerspectiveToggle, &keys) {
        match view_state.current_mode {
            ViewMode::Perspective => {
                view_state.current_mode = ViewMode::Orthographic;
            }
            _ => {
                // Any ortho mode -> back to perspective
                view_state.current_mode = ViewMode::Perspective;
            }
        }
    }
    
    // Numpad 8: Top view
    if keybindings.check(Action::ViewTop, &keys) {
        view_state.current_mode = ViewMode::Top;
    }
    
    // Numpad 2: Front view
    if keybindings.check(Action::ViewFront, &keys) {
        view_state.current_mode = ViewMode::Front;
    }
    
    // Numpad 4: Left side view
    if keybindings.check(Action::ViewSideLeft, &keys) {
        view_state.current_mode = ViewMode::Side;
    }
    
    // Numpad 6: Right side view
    if keybindings.check(Action::ViewSideRight, &keys) {
        view_state.current_mode = ViewMode::SideRight;
    }
}

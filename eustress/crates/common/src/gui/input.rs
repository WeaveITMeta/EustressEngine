//! GUI input handling — raycast mouse position against 3D GUI quads.

use bevy::prelude::*;
use bevy::input::ButtonState;
use bevy::input::mouse::MouseButtonInput;
use bevy::math::primitives::InfinitePlane3d;

use super::renderer::{SlintGuiInstance, SlintGuiQuad, SlintGuiAdapters, SlintGuiType};

/// Raycast mouse position against all 3D GUI quads and forward Slint events.
pub fn handle_gui_input(
    mut mouse_button: MessageReader<MouseButtonInput>,
    windows: Query<&Window>,
    gui_instances: Query<(Entity, &SlintGuiInstance)>,
    quad_query: Query<(Entity, &GlobalTransform, &ChildOf), With<SlintGuiQuad>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    adapters: Option<NonSend<SlintGuiAdapters>>,
) {
    let Some(adapters) = adapters else { return };
    let Ok(window) = windows.single() else { return };
    // Find the main 3D camera (order 0)
    let Some((camera, cam_transform)) = camera_query.iter().find(|(c, _)| c.order == 0) else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };

    let button_events: Vec<_> = mouse_button.read().cloned().collect();

    for (quad_entity, quad_transform, child_of) in &quad_query {
        // The quad is a child of the GUI entity — find the parent's SlintGuiInstance
        let parent_entity = child_of.parent();
        let Some((gui_entity, gui)) = gui_instances.iter().find(|(e, _)| *e == parent_entity) else {
            continue;
        };

        // Only raycast against 3D GUIs (Billboard + Surface)
        if gui.gui_type == SlintGuiType::Screen { continue; }

        let Some(adapter) = adapters.get(gui_entity) else { continue };

        let Ok(ray) = camera.viewport_to_world(cam_transform, cursor_pos) else { continue };

        let plane_normal = quad_transform.back();
        let plane_origin = quad_transform.translation();
        let plane = InfinitePlane3d::new(*plane_normal);

        if let Some(hit_point) = ray.plane_intersection_point(plane_origin, plane) {
            let local = quad_transform.affine().inverse().transform_point3(hit_point);

            // Quad is 1x1 centered at origin in local space
            if local.x.abs() <= 0.5 && local.y.abs() <= 0.5 {
                let u = local.x + 0.5;
                let v = 1.0 - (local.y + 0.5);
                let sf = adapter.scale_factor.get();
                let px = u * gui.width as f32;
                let py = v * gui.height as f32;
                let position = slint::LogicalPosition::new(px / sf, py / sf);

                adapter.slint_window.dispatch_event(
                    slint::platform::WindowEvent::PointerMoved { position }
                );

                for event in &button_events {
                    let button = match event.button {
                        MouseButton::Left => slint::platform::PointerEventButton::Left,
                        MouseButton::Right => slint::platform::PointerEventButton::Right,
                        MouseButton::Middle => slint::platform::PointerEventButton::Middle,
                        _ => slint::platform::PointerEventButton::Other,
                    };
                    match event.state {
                        ButtonState::Pressed => {
                            adapter.slint_window.dispatch_event(
                                slint::platform::WindowEvent::PointerPressed { button, position }
                            );
                        }
                        ButtonState::Released => {
                            adapter.slint_window.dispatch_event(
                                slint::platform::WindowEvent::PointerReleased { button, position }
                            );
                        }
                    }
                }
            }
        }
    }
}

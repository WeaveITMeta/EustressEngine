#![allow(dead_code)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

// Camera mode enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraMode {
    Orbit,       // Rotate around focus point (default)
    Free,        // Free-fly (WASD + mouse)
    FirstPerson, // Attached to entity
}

impl Default for CameraMode {
    fn default() -> Self {
        CameraMode::Orbit
    }
}

// Studio camera component
#[derive(Component)]
pub struct StudioCamera {
    pub mode: CameraMode,
    pub speed: f32,
    pub sensitivity: f32,
    pub focus_point: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for StudioCamera {
    fn default() -> Self {
        Self {
            mode: CameraMode::Orbit,
            speed: 5.0,
            sensitivity: 0.2,
            focus_point: Vec3::ZERO,
            distance: 20.0,
            yaw: 0.0,
            pitch: -30.0f32.to_radians(),
        }
    }
}

// Camera state for serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraState {
    pub position: [f32; 3],
    pub rotation: [f32; 3], // Euler angles (pitch, yaw, roll)
    pub fov: f32,
    pub mode: CameraMode,
    pub speed: f32,
    pub focus_point: [f32; 3],
}

// Global camera state manager
pub struct CameraManager {
    pub state: Arc<Mutex<CameraState>>,
}

impl Default for CameraManager {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(CameraState {
                position: [0.0, 10.0, -20.0],
                rotation: [330.0, 0.0, 0.0], // Looking down slightly
                fov: 70.0,
                mode: CameraMode::Orbit,
                speed: 5.0,
                focus_point: [0.0, 0.0, 0.0],
            })),
        }
    }
}

// Camera control system for orbit mode
pub fn orbit_camera_system(
    mut camera_query: Query<(&mut Transform, &mut StudioCamera), With<Camera>>,
    mouse_motion: Res<ButtonInput<MouseButton>>,
    mut mouse_motion_events: MessageReader<bevy::input::mouse::MouseMotion>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut mouse_wheel_events: MessageReader<bevy::input::mouse::MouseWheel>,
    _time: Res<Time>,
) {
    for (mut transform, mut camera) in camera_query.iter_mut() {
        if camera.mode != CameraMode::Orbit {
            continue;
        }

        let mut rotation_delta = Vec2::ZERO;
        let mut zoom_delta = 0.0;
        let mut pan_delta = Vec2::ZERO;

        // Right mouse button - orbit
        if mouse_motion.pressed(MouseButton::Right) {
            for event in mouse_motion_events.read() {
                rotation_delta += event.delta * camera.sensitivity * 0.01;
            }
        }

        // Middle mouse button or Shift+Right - pan
        if mouse_motion.pressed(MouseButton::Middle)
            || (mouse_motion.pressed(MouseButton::Right)
                && (keyboard_input.pressed(KeyCode::ShiftLeft)
                    || keyboard_input.pressed(KeyCode::ShiftRight)))
        {
            for event in mouse_motion_events.read() {
                pan_delta += event.delta * camera.sensitivity * 0.05;
            }
        }

        // Mouse wheel - zoom
        for event in mouse_wheel_events.read() {
            zoom_delta += event.y * camera.speed * 0.1;
        }

        // Apply rotation
        camera.yaw -= rotation_delta.x;
        camera.pitch = (camera.pitch - rotation_delta.y).clamp(-89.0f32.to_radians(), 89.0f32.to_radians());

        // Apply zoom
        camera.distance = (camera.distance - zoom_delta).max(1.0);

        // Apply pan (move focus point)
        if pan_delta.length() > 0.0 {
            let right = transform.right() * pan_delta.x * camera.distance * 0.01;
            let up = transform.up() * pan_delta.y * camera.distance * 0.01;
            camera.focus_point += right + up;
        }

        // Calculate camera position from spherical coordinates
        let x = camera.distance * camera.pitch.cos() * camera.yaw.sin();
        let y = camera.distance * camera.pitch.sin();
        let z = camera.distance * camera.pitch.cos() * camera.yaw.cos();

        transform.translation = camera.focus_point + Vec3::new(x, y, z);
        transform.look_at(camera.focus_point, Vec3::Y);
    }
}

// Camera control system for free mode
pub fn free_camera_system(
    mut camera_query: Query<(&mut Transform, &StudioCamera), With<Camera>>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mouse_motion: Res<ButtonInput<MouseButton>>,
    mut mouse_motion_events: MessageReader<bevy::input::mouse::MouseMotion>,
    time: Res<Time>,
) {
    for (mut transform, camera) in camera_query.iter_mut() {
        if camera.mode != CameraMode::Free {
            continue;
        }

        let mut velocity = Vec3::ZERO;
        let forward = transform.forward();
        let right = transform.right();

        // WASD movement
        if keyboard_input.pressed(KeyCode::KeyW) {
            velocity += *forward;
        }
        if keyboard_input.pressed(KeyCode::KeyS) {
            velocity -= *forward;
        }
        if keyboard_input.pressed(KeyCode::KeyD) {
            velocity += *right;
        }
        if keyboard_input.pressed(KeyCode::KeyA) {
            velocity -= *right;
        }
        if keyboard_input.pressed(KeyCode::KeyE) || keyboard_input.pressed(KeyCode::Space) {
            velocity += Vec3::Y;
        }
        if keyboard_input.pressed(KeyCode::KeyQ) {
            velocity -= Vec3::Y;
        }

        // Apply speed multiplier (Shift = fast, Ctrl = slow)
        let mut speed_multiplier = 1.0;
        if keyboard_input.pressed(KeyCode::ShiftLeft) || keyboard_input.pressed(KeyCode::ShiftRight) {
            speed_multiplier = 2.0;
        }
        if keyboard_input.pressed(KeyCode::ControlLeft) || keyboard_input.pressed(KeyCode::ControlRight) {
            speed_multiplier = 0.5;
        }

        // Move camera
        if velocity.length() > 0.0 {
            transform.translation += velocity.normalize() * camera.speed * speed_multiplier * time.delta_secs();
        }

        // Right mouse button - look around
        if mouse_motion.pressed(MouseButton::Right) {
            for event in mouse_motion_events.read() {
                let delta = event.delta * camera.sensitivity * 0.01;
                
                let (mut yaw, mut pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
                yaw -= delta.x;
                pitch -= delta.y;
                pitch = pitch.clamp(-89.0f32.to_radians(), 89.0f32.to_radians());
                
                transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
            }
        }
    }
}

// Focus camera on entity by ID
pub fn focus_camera_on_entity(
    entity_id: u32,
    camera_query: &mut Query<(&mut Transform, &mut StudioCamera), With<Camera>>,
    entity_query: &Query<(&Transform, &GlobalTransform), Without<Camera>>,
) -> Result<(), String> {
    // Find entity by index (simplified - in production, use proper entity lookup)
    let entities: Vec<_> = entity_query.iter().collect();
    if entity_id as usize >= entities.len() {
        return Err("Entity not found".to_string());
    }

    let (_entity_transform, entity_global) = entities[entity_id as usize];
    
    for (mut cam_transform, mut camera) in camera_query.iter_mut() {
        if camera.mode == CameraMode::Orbit {
            // Set focus point to entity position
            camera.focus_point = entity_global.translation();
            
            // Calculate appropriate distance based on entity size
            // For now, use a default distance
            camera.distance = 10.0;
        } else {
            // For free mode, just move camera to look at entity
            let target = entity_global.translation();
            cam_transform.look_at(target, Vec3::Y);
        }
    }

    Ok(())
}

// Update camera state for frontend sync
// NOTE: Disabled because CameraManager is a Tauri State, not a Bevy Resource.
// For proper integration, CameraManager needs to be wrapped in Arc<RwLock<>> and shared
// between Tauri and Bevy, similar to how BevyPartManager wraps PartManager in rendering.rs.
#[allow(dead_code)]
pub fn update_camera_state(
    _camera_query: Query<(&Transform, &StudioCamera), With<Camera>>,
) {
    // TODO: Implement when Arc<RwLock<CameraManager>> is properly integrated
}

// Plugin to set up camera systems
pub struct StudioCameraPlugin;

impl Plugin for StudioCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
                orbit_camera_system,
                free_camera_system,
                // TODO: update_camera_state requires Arc<RwLock<>> sharing between Tauri and Bevy
            ));
    }
}

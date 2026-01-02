#![deny(unsafe_code)]

use bevy::prelude::*;
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::input::ButtonInput;
use bevy::window::{CursorGrabMode, CursorOptions, Window};
use core::f32::consts::*;

/// Multi-mode camera controller plugin with Flycam, RTS, and Touch support
#[derive(Default)]
pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, attach_default_controller)
           .add_systems(Update, (
               run_camera_controller,
               handle_touch_input,
           ));
    }
}

/// Based on Valorant's default sensitivity: 1.0 / 180.0 radians per dot.
pub const RADIANS_PER_DOT: f32 = 1.0 / 180.0;

/// Camera control modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
pub enum CameraMode {
    /// Free-flying camera with WASD + mouse (FPS-style)
    #[default]
    Flycam,
    /// RTS-style camera with pan, zoom, and orbit
    RTS,
    /// Touch-based camera for mobile devices
    Touch,
}

/// Camera controller Component with multi-mode support
#[derive(Component, Reflect)]
pub struct CameraController {
    /// Enables this controller when `true`.
    pub enabled: bool,
    /// Indicates if this controller has been initialized by the plugin.
    pub initialized: bool,
    /// Current camera mode
    pub mode: CameraMode,
    
    // === FLYCAM MODE ===
    /// Multiplier for pitch and yaw rotation speed (Flycam).
    pub sensitivity: f32,
    /// Key for forward translation.
    pub key_forward: KeyCode,
    /// Key for backward translation.
    pub key_back: KeyCode,
    /// Key for left translation.
    pub key_left: KeyCode,
    /// Key for right translation.
    pub key_right: KeyCode,
    /// Key for up translation.
    pub key_up: KeyCode,
    /// Key for down translation.
    pub key_down: KeyCode,
    /// Key to use run_speed instead of walk_speed.
    pub key_run: KeyCode,
    /// Mouse button for grabbing the mouse focus.
    pub mouse_key_cursor_grab: MouseButton,
    /// Keyboard key for toggling cursor grab.
    pub keyboard_key_toggle_cursor_grab: KeyCode,
    /// Base movement speed.
    pub walk_speed: f32,
    /// Running movement speed.
    pub run_speed: f32,
    /// Multiplier for scroll adjustments to walk/run speeds.
    pub scroll_factor: f32,
    /// Friction factor used to exponentially decay velocity over time.
    pub friction: f32,
    /// Pitch rotation.
    pub pitch: f32,
    /// Yaw rotation.
    pub yaw: f32,
    /// Translation velocity.
    pub velocity: Vec3,
    
    // === RTS MODE ===
    /// Pan speed for RTS mode
    pub rts_pan_speed: f32,
    /// Zoom speed for RTS mode
    pub rts_zoom_speed: f32,
    /// Rotation speed for RTS mode
    pub rts_rotate_speed: f32,
    /// Minimum zoom distance
    pub rts_min_zoom: f32,
    /// Maximum zoom distance
    pub rts_max_zoom: f32,
    /// Current zoom distance
    pub rts_zoom_distance: f32,
    /// Focus point for RTS camera
    pub rts_focus_point: Vec3,
    /// Edge scroll margin in pixels (0 to disable)
    pub rts_edge_scroll_margin: f32,
    /// Mouse button for RTS pan
    pub rts_pan_button: MouseButton,
    /// Mouse button for RTS rotate
    pub rts_rotate_button: MouseButton,
    
    // === TOUCH MODE ===
    /// Pan speed multiplier for touch
    pub touch_pan_speed: f32,
    /// Zoom speed multiplier for touch
    pub touch_zoom_speed: f32,
    /// Rotation speed multiplier for touch
    pub touch_rotate_speed: f32,
    /// Last touch positions for gesture detection
    pub touch_start_positions: Vec<Vec2>,
}

impl Default for CameraController {
    fn default() -> Self {
        use KeyCode::*;
        Self {
            enabled: true,
            initialized: false,
            mode: CameraMode::Flycam,
            
            // Flycam defaults
            sensitivity: 1.0,
            key_forward: KeyW,
            key_back: KeyS,
            key_left: KeyA,
            key_right: KeyD,
            key_up: KeyE,
            key_down: KeyQ,
            key_run: ShiftLeft,
            mouse_key_cursor_grab: MouseButton::Left,
            keyboard_key_toggle_cursor_grab: KeyM,
            walk_speed: 5.0,
            run_speed: 15.0,
            scroll_factor: 0.1,
            friction: 0.5,
            pitch: 0.0,
            yaw: 0.0,
            velocity: Vec3::ZERO,
            
            // RTS defaults
            rts_pan_speed: 10.0,
            rts_zoom_speed: 5.0,
            rts_rotate_speed: 1.0,
            rts_min_zoom: 5.0,
            rts_max_zoom: 100.0,
            rts_zoom_distance: 30.0,
            rts_focus_point: Vec3::ZERO,
            rts_edge_scroll_margin: 50.0,
            rts_pan_button: MouseButton::Middle,
            rts_rotate_button: MouseButton::Right,
            
            // Touch defaults
            touch_pan_speed: 1.0,
            touch_zoom_speed: 0.5,
            touch_rotate_speed: 1.0,
            touch_start_positions: Vec::new(),
        }
    }
}

impl core::fmt::Display for CameraController {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "\nFreecam Controls:\n    Mouse\t- Move camera orientation\n    Scroll\t- Adjust movement speed\n    {:?}\t- Hold to grab cursor\n    {:?}\t- Toggle cursor grab\n    {:?} & {:?}\t- Fly forward & backwards\n    {:?} & {:?}\t- Fly sideways left & right\n    {:?} & {:?}\t- Fly up & down\n    {:?}\t- Fly faster while held",
            self.mouse_key_cursor_grab,
            self.keyboard_key_toggle_cursor_grab,
            self.key_forward,
            self.key_back,
            self.key_left,
            self.key_right,
            self.key_up,
            self.key_down,
            self.key_run,
        )
    }
}

fn run_camera_controller(
    time: Res<Time>,
    windows: Query<&Window>,
    mut cursor_options: Query<&mut CursorOptions, With<Window>>,
    mut mouse_motion_events: MessageReader<MouseMotion>,
    mut mouse_wheel_events: MessageReader<MouseWheel>,
    mouse_button_input: Res<ButtonInput<MouseButton>>,
    key_input: Res<ButtonInput<KeyCode>>,
    mut toggle_cursor_grab: Local<bool>,
    mut mouse_cursor_grab: Local<bool>,
    mut query: Query<(&mut Transform, &mut CameraController), With<Camera>>,
) {
    let dt = time.delta_secs();

    let Ok((mut transform, mut controller)) = query.single_mut() else {
        return;
    };

    if !controller.initialized {
        let (yaw, pitch, _roll) = transform.rotation.to_euler(EulerRot::YXZ);
        controller.yaw = yaw;
        controller.pitch = pitch;
        controller.initialized = true;
    }
    if !controller.enabled {
        return;
    }

    // Handle mouse wheel scrolling for speed adjustment
    let mut scroll = 0.0;
    for event in mouse_wheel_events.read() {
        let amount = match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 16.0,
        };
        scroll += amount;
    }
    controller.walk_speed += scroll * controller.scroll_factor * controller.walk_speed;
    controller.run_speed = controller.walk_speed * 3.0;

    // Key input -> axis
    let mut axis_input = Vec3::ZERO;
    if key_input.pressed(controller.key_forward) {
        axis_input.z += 1.0;
    }
    if key_input.pressed(controller.key_back) {
        axis_input.z -= 1.0;
    }
    if key_input.pressed(controller.key_right) {
        axis_input.x += 1.0;
    }
    if key_input.pressed(controller.key_left) {
        axis_input.x -= 1.0;
    }
    if key_input.pressed(controller.key_up) {
        axis_input.y += 1.0;
    }
    if key_input.pressed(controller.key_down) {
        axis_input.y -= 1.0;
    }

    let mut cursor_grab_change = false;
    if key_input.just_pressed(controller.keyboard_key_toggle_cursor_grab) {
        *toggle_cursor_grab = !*toggle_cursor_grab;
        cursor_grab_change = true;
    }
    if mouse_button_input.just_pressed(controller.mouse_key_cursor_grab) {
        *mouse_cursor_grab = true;
        cursor_grab_change = true;
    }
    if mouse_button_input.just_released(controller.mouse_key_cursor_grab) {
        *mouse_cursor_grab = false;
        cursor_grab_change = true;
    }
    let cursor_grab = *mouse_cursor_grab || *toggle_cursor_grab;

    // Update velocity
    if axis_input != Vec3::ZERO {
        let max_speed = if key_input.pressed(controller.key_run) {
            controller.run_speed
        } else {
            controller.walk_speed
        };
        controller.velocity = axis_input.normalize() * max_speed;
    } else {
        let friction = controller.friction.clamp(0.0, 1.0);
        controller.velocity *= 1.0 - friction;
        if controller.velocity.length_squared() < 1e-6 {
            controller.velocity = Vec3::ZERO;
        }
    }

    // Apply movement update
    if controller.velocity != Vec3::ZERO {
        let forward = *transform.forward();
        let right = *transform.right();
        transform.translation += controller.velocity.x * dt * right
            + controller.velocity.y * dt * Vec3::Y
            + controller.velocity.z * dt * forward;
    }

    // Handle cursor grab visuals
    if cursor_grab_change {
        for window in &windows {
            if !window.focused {
                continue;
            }
            if let Ok(mut opts) = cursor_options.single_mut() {
                if cursor_grab {
                    opts.grab_mode = CursorGrabMode::Locked;
                    opts.visible = false;
                } else {
                    opts.grab_mode = CursorGrabMode::None;
                    opts.visible = true;
                }
            }
        }
    }

    // Mouse look - accumulate motion events
    let mut mouse_delta = Vec2::ZERO;
    for event in mouse_motion_events.read() {
        mouse_delta += event.delta;
    }
    
    if mouse_delta != Vec2::ZERO && cursor_grab {
        controller.pitch = (controller.pitch
            - mouse_delta.y * RADIANS_PER_DOT * controller.sensitivity)
            .clamp(-PI / 2., PI / 2.);
        controller.yaw -=
            mouse_delta.x * RADIANS_PER_DOT * controller.sensitivity;
        transform.rotation = Quat::from_euler(EulerRot::ZYX, 0.0, controller.yaw, controller.pitch);
    }
}

/// If no `CameraController` exists, attach a default one to the first camera found.
fn attach_default_controller(
    mut commands: Commands,
    q_cam: Query<Entity, With<Camera>>,
    q_has: Query<Entity, With<CameraController>>,
) {
    if q_has.is_empty() {
        if let Some(entity) = q_cam.iter().next() {
            commands.entity(entity).insert(CameraController::default());
        }
    }
}

/// Handle touch input for mobile camera control
fn handle_touch_input(
    mut touch_events: MessageReader<TouchInput>,
    mut query: Query<(&mut Transform, &mut CameraController), With<Camera>>,
    time: Res<Time>,
) {
    let Ok((mut transform, mut controller)) = query.single_mut() else {
        return;
    };
    
    if !controller.enabled || controller.mode != CameraMode::Touch {
        return;
    }
    
    let dt = time.delta_secs();
    
    for touch in touch_events.read() {
        match touch.phase {
            TouchPhase::Started => {
                // Store initial touch position
                if controller.touch_start_positions.len() <= touch.id as usize {
                    controller.touch_start_positions.resize(touch.id as usize + 1, Vec2::ZERO);
                }
                controller.touch_start_positions[touch.id as usize] = touch.position;
            }
            TouchPhase::Moved => {
                let touches: Vec<_> = controller.touch_start_positions.iter()
                    .enumerate()
                    .filter(|(_, pos)| **pos != Vec2::ZERO)
                    .collect();
                
                match touches.len() {
                    1 => {
                        // Single finger - pan camera
                        let delta = touch.position - controller.touch_start_positions[touch.id as usize];
                        let right = *transform.right();
                        let up = Vec3::Y;
                        
                        transform.translation -= right * delta.x * controller.touch_pan_speed * dt;
                        transform.translation -= up * delta.y * controller.touch_pan_speed * dt;
                        
                        controller.touch_start_positions[touch.id as usize] = touch.position;
                    }
                    2 => {
                        // Two fingers - pinch zoom or rotate
                        if touch.id < 2 {
                            let pos0 = controller.touch_start_positions[0];
                            let pos1 = controller.touch_start_positions[1];
                            
                            let prev_distance = pos0.distance(pos1);
                            let mut current_pos = controller.touch_start_positions.clone();
                            current_pos[touch.id as usize] = touch.position;
                            let curr_distance = current_pos[0].distance(current_pos[1]);
                            
                            // Zoom based on pinch
                            let zoom_delta = (prev_distance - curr_distance) * controller.touch_zoom_speed * dt;
                            let forward = *transform.forward();
                            transform.translation += forward * zoom_delta;
                            
                            controller.touch_start_positions[touch.id as usize] = touch.position;
                        }
                    }
                    _ => {}
                }
            }
            TouchPhase::Ended | TouchPhase::Canceled => {
                // Clear touch position
                if touch.id < controller.touch_start_positions.len() as u64 {
                    controller.touch_start_positions[touch.id as usize] = Vec2::ZERO;
                }
            }
        }
    }
}

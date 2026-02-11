use bevy::prelude::*;
use bevy::input::mouse::{MouseMotion, MouseWheel, MouseScrollUnit};
use bevy::input::touch::TouchInput;
use std::f32::consts::{FRAC_PI_2, PI};

// Camera distance constraints - effectively infinite zoom
const MIN_CAMERA_DISTANCE: f32 = 0.001;   // Allow extremely close zoom (1mm)
const MAX_CAMERA_DISTANCE: f32 = 1000000.0;  // Allow extremely far zoom (1000km)

// ============================================================================
// Blender-Like View System (Y-Up Coordinate System)
// ============================================================================
//
// Eustress Engine uses Y-up (Bevy default): +X right, +Y up, +Z forward
// Blender uses Z-up: +X right, +Y forward, +Z up
//
// Axis Mapping (Blender → Y-up):
// - Blender Top (X/Y floor) → Y-up Top (X/Z floor)
// - Blender Front (X/Z elevation) → Y-up Front (X/Y elevation)
// - Blender Right (Y/Z side) → Y-up Right (Z/Y side)
//
// Numpad Keys:
// - Num2: Front View (+Z looking toward -Z)
// - Num4: Left View (-X looking toward +X)
// - Num6: Right View (+X looking toward -X)
// - Num8: Top View (+Y looking toward -Y)
// - Num5: Toggle Orthographic/Perspective
// - Num.: Frame Selected (zoom to fit)
// - Ctrl+Num: Opposite views (Back, Right, Left, Bottom)
// ============================================================================

/// Predefined camera view angles (Blender-style for Y-up)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraView {
    /// Front: +Z looking toward -Z (see X/Y plane)
    Front,
    /// Back: -Z looking toward +Z
    Back,
    /// Left: -X looking toward +X (see Z/Y plane)
    Left,
    /// Right: +X looking toward -X (see Z/Y plane)
    Right,
    /// Top: +Y looking toward -Y (see X/Z plane)
    Top,
    /// Bottom: -Y looking toward +Y
    Bottom,
    /// Custom/Free view
    #[default]
    Custom,
}

impl CameraView {
    /// Get yaw and pitch for this view (in radians)
    /// Returns (yaw, pitch) where:
    /// - yaw: rotation around Y axis (0 = looking toward -Z)
    /// - pitch: rotation around X axis (0 = horizontal, +90 = looking down)
    pub fn angles(&self) -> (f32, f32) {
        match self {
            // Front: Camera at +Z, looking toward -Z
            // yaw = 0 (facing -Z), pitch = 0 (horizontal)
            CameraView::Front => (0.0, 0.0),
            
            // Back: Camera at -Z, looking toward +Z
            // yaw = PI (180°), pitch = 0
            CameraView::Back => (PI, 0.0),
            
            // Right: Camera at +X, looking toward -X
            // yaw = PI/2 (90°), pitch = 0
            CameraView::Right => (FRAC_PI_2, 0.0),
            
            // Left: Camera at -X, looking toward +X
            // yaw = -PI/2 (-90°), pitch = 0
            CameraView::Left => (-FRAC_PI_2, 0.0),
            
            // Top: Camera at +Y, looking down toward -Y
            // yaw = 0, pitch = PI/2 - small epsilon (looking down)
            CameraView::Top => (0.0, FRAC_PI_2 - 0.001),
            
            // Bottom: Camera at -Y, looking up toward +Y
            // yaw = 0, pitch = -PI/2 + small epsilon (looking up)
            CameraView::Bottom => (0.0, -FRAC_PI_2 + 0.001),
            
            // Custom: Return default isometric-ish view
            CameraView::Custom => (45.0_f32.to_radians(), 30.0_f32.to_radians()),
        }
    }
    
    /// Get display name for this view
    pub fn name(&self) -> &'static str {
        match self {
            CameraView::Front => "Front",
            CameraView::Back => "Back",
            CameraView::Left => "Left",
            CameraView::Right => "Right",
            CameraView::Top => "Top",
            CameraView::Bottom => "Bottom",
            CameraView::Custom => "Custom",
        }
    }
    
    /// Get the opposite view
    pub fn opposite(&self) -> CameraView {
        match self {
            CameraView::Front => CameraView::Back,
            CameraView::Back => CameraView::Front,
            CameraView::Left => CameraView::Right,
            CameraView::Right => CameraView::Left,
            CameraView::Top => CameraView::Bottom,
            CameraView::Bottom => CameraView::Top,
            CameraView::Custom => CameraView::Custom,
        }
    }
}

/// Camera projection mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectionMode {
    #[default]
    Perspective,
    Orthographic,
}

/// Message to snap camera to a predefined view
#[derive(Message, Debug, Clone)]
pub struct SnapToViewEvent {
    pub view: CameraView,
    pub animate: bool,
}

/// Message to toggle projection mode
#[derive(Message, Debug, Clone)]
pub struct ToggleProjectionEvent;

/// Message to frame/zoom to selection or scene
#[derive(Message, Debug, Clone)]
pub struct FrameSelectionEvent {
    /// If None, frame entire scene
    pub target_bounds: Option<(Vec3, Vec3)>,
}

/// Eustress Camera: Empowering focus and flow navigation
/// Pivot-based system that builds positive momentum and keeps you centered on your vision
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct EustressCamera {
    pub enabled: bool,            // When true, energizes navigation
    pub initialized: bool,        // Tracks setup for positive starts
    pub pivot: Vec3,              // Dynamic focus for growth-oriented views
    pub distance: f32,            // Empowering zoom level
    pub yaw: f32,                 // Horizontal flow angle
    pub pitch: f32,               // Vertical motivation angle
    pub base_speed: f32,          // Base speed, adapts to user intent
    pub sensitivity: f32,         // Responsive feel for positive control
    pub zoom_speed: f32,          // Growth zoom factor
    pub pan_speed: f32,           // Smooth pan for exploration
    pub smooth_factor: f32,       // Fluid transitions to reduce stress
    pub flow_velocity: Vec3,      // Momentum for engaging, eustress-building movement
    pub friction: f32,            // Gentle decay for controlled flow
    // Touch fields for mobile empowerment
    pub touch_pan_speed: f32,
    pub touch_zoom_speed: f32,
    #[reflect(ignore)]
    pub touch_start_positions: Vec<Vec2>,
    // Smoothing targets for rotation
    pub target_yaw: f32,          // Target yaw for smooth rotation
    pub target_pitch: f32,        // Target pitch for smooth rotation
    // View state (Blender-like)
    #[reflect(ignore)]
    pub current_view: CameraView, // Current view mode
    #[reflect(ignore)]
    pub projection_mode: ProjectionMode, // Perspective or Orthographic
    pub ortho_scale: f32,         // Orthographic zoom scale
    // Animation state for smooth view transitions
    pub animating: bool,          // True during view transition
    pub anim_start_yaw: f32,
    pub anim_start_pitch: f32,
    pub anim_target_yaw: f32,
    pub anim_target_pitch: f32,
    pub anim_progress: f32,
    pub anim_duration: f32,
}

impl Default for EustressCamera {
    fn default() -> Self {
        let yaw = 45.0_f32.to_radians();
        let pitch = 30.0_f32.to_radians();
        Self {
            enabled: true,
            initialized: false,
            pivot: Vec3::ZERO,
            distance: 20.0,
            yaw,
            pitch,
            base_speed: 20.0,        // Direct WASD movement speed
            sensitivity: 0.003,      // Mouse sensitivity for rotation
            zoom_speed: 2.0,         // Faster zoom response
            pan_speed: 0.01,         // Direct pan speed
            smooth_factor: 1.0,      // NO SMOOTHING - instant response
            flow_velocity: Vec3::ZERO,
            friction: 1.0,           // NO FRICTION - instant stop
            touch_pan_speed: 0.002,
            touch_zoom_speed: 0.005,
            touch_start_positions: vec![Vec2::ZERO; 10],  // Support multi-touch
            target_yaw: yaw,         // Initialize to current
            target_pitch: pitch,     // Initialize to current
            // View state defaults
            current_view: CameraView::Custom,
            projection_mode: ProjectionMode::Perspective,
            ortho_scale: 10.0,
            // Animation defaults
            animating: false,
            anim_start_yaw: 0.0,
            anim_start_pitch: 0.0,
            anim_target_yaw: 0.0,
            anim_target_pitch: 0.0,
            anim_progress: 0.0,
            anim_duration: 0.2, // 200ms transition
        }
    }
}

/// Eustress Camera Plugin: Empowering focus and flow
pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app
            .register_type::<EustressCamera>()
            .add_message::<SnapToViewEvent>()
            .add_message::<ToggleProjectionEvent>()
            .add_message::<FrameSelectionEvent>()
            .add_systems(Update, (
                ensure_camera_exists,
                update_camera_viewport_for_ui,
                camera_view_input_system,
                handle_snap_to_view,
                handle_toggle_projection,
                handle_frame_selection,
                animate_view_transition,
                eustress_camera_controls,
                update_eustress_camera_transform,
            ).chain());
    }
}

/// Update the 3D camera viewport to fit within the Slint UI layout.
/// Reads actual viewport bounds from the ViewportBounds resource (populated by Slint each frame).
/// This constrains 3D rendering to only the visible viewport area, avoiding wasted GPU work
/// behind opaque UI panels (ribbon, explorer, properties, output).
fn update_camera_viewport_for_ui(
    mut camera_query: Query<&mut Camera, With<Camera3d>>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
) {
    let Some(vb) = viewport_bounds else { return };
    let Some(mut camera) = camera_query.iter_mut().next() else { return };
    
    // Only clip if Slint has reported valid viewport dimensions
    if vb.width < 10.0 || vb.height < 10.0 {
        return;
    }
    
    camera.viewport = Some(bevy::camera::Viewport {
        physical_position: UVec2::new(vb.x.max(0.0) as u32, vb.y.max(0.0) as u32),
        physical_size: UVec2::new(vb.width as u32, vb.height as u32),
        ..default()
    });
}

/// Ensure at least one camera exists - auto-spawn if all cameras are deleted
fn ensure_camera_exists(
    mut commands: Commands,
    camera_query: Query<Entity, With<Camera3d>>,
) {
    // If no cameras exist, spawn a new one at the origin
    if camera_query.is_empty() {
        info!("📷 No camera found - spawning new camera at origin");
        
        // Create EustressCamera with proper initialization
        let mut cam = EustressCamera::default();
        cam.pivot = Vec3::ZERO;
        cam.distance = 20.0;
        cam.yaw = std::f32::consts::FRAC_PI_4;
        cam.pitch = -0.5;
        cam.enabled = true;
        
        commands.spawn((
            Camera3d::default(),
            bevy::core_pipeline::tonemapping::Tonemapping::Reinhard,
            Transform::from_xyz(10.0, 10.0, 15.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
            Projection::Perspective(PerspectiveProjection {
                fov: 70.0_f32.to_radians(),
                ..default()
            }),
            cam,
            // Instance component so Camera appears in Explorer under Workspace
            crate::classes::Instance {
                name: "Camera".to_string(),
                class_name: crate::classes::ClassName::Camera,
                archivable: true,
                id: 0,
                ..Default::default()
            },
            Name::new("Camera"),
        ));
    }
}

// ============================================================================
// View Input System - Numpad Controls (Blender-like)
// ============================================================================

/// Handle numpad input for view snapping
fn camera_view_input_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut snap_events: MessageWriter<SnapToViewEvent>,
    mut toggle_events: MessageWriter<ToggleProjectionEvent>,
    mut frame_events: MessageWriter<FrameSelectionEvent>,
) {
    // TODO: Check Slint UI focus state to block input when UI has focus
    
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    
    // Num2: Front View (or Back with Ctrl)
    if keys.just_pressed(KeyCode::Numpad2) {
        let view = if ctrl { CameraView::Back } else { CameraView::Front };
        snap_events.write(SnapToViewEvent { view, animate: true });
    }
    
    // Num8: Top View (or Bottom with Ctrl)
    if keys.just_pressed(KeyCode::Numpad8) {
        let view = if ctrl { CameraView::Bottom } else { CameraView::Top };
        snap_events.write(SnapToViewEvent { view, animate: true });
    }
    
    // Num4: Left View (or Right with Ctrl)
    if keys.just_pressed(KeyCode::Numpad4) {
        let view = if ctrl { CameraView::Right } else { CameraView::Left };
        snap_events.write(SnapToViewEvent { view, animate: true });
    }
    
    // Num6: Right View (or Left with Ctrl)
    if keys.just_pressed(KeyCode::Numpad6) {
        let view = if ctrl { CameraView::Left } else { CameraView::Right };
        snap_events.write(SnapToViewEvent { view, animate: true });
    }
    
    // Num5: Toggle Orthographic/Perspective
    if keys.just_pressed(KeyCode::Numpad5) {
        toggle_events.write(ToggleProjectionEvent);
    }
    
    // Num. (NumpadDecimal): Frame Selection / Zoom to Fit
    if keys.just_pressed(KeyCode::NumpadDecimal) {
        frame_events.write(FrameSelectionEvent { target_bounds: None });
    }
    
    // Num0: Camera View (future: switch to scene camera)
    // Num1: Could be used for custom view 1
    // Num3: Could be used for custom view 2
    // Num7: Could be used for isometric view
    // Num9: Could be used for another preset
}

/// Handle snap to view events
fn handle_snap_to_view(
    mut events: MessageReader<SnapToViewEvent>,
    mut query: Query<&mut EustressCamera, With<Camera3d>>,
) {
    for event in events.read() {
        for mut cam in query.iter_mut() {
            let (target_yaw, target_pitch) = event.view.angles();
            
            if event.animate {
                // Start animation
                cam.animating = true;
                cam.anim_start_yaw = cam.yaw;
                cam.anim_start_pitch = cam.pitch;
                cam.anim_target_yaw = target_yaw;
                cam.anim_target_pitch = target_pitch;
                cam.anim_progress = 0.0;
            } else {
                // Instant snap
                cam.yaw = target_yaw;
                cam.pitch = target_pitch;
                cam.target_yaw = target_yaw;
                cam.target_pitch = target_pitch;
            }
            
            cam.current_view = event.view;
            
            // Log view change
            info!("📷 Camera: {} View", event.view.name());
        }
    }
}

/// Handle projection toggle events
fn handle_toggle_projection(
    mut events: MessageReader<ToggleProjectionEvent>,
    mut cam_query: Query<&mut EustressCamera, With<Camera3d>>,
    mut proj_query: Query<&mut Projection, With<Camera3d>>,
) {
    for _event in events.read() {
        for mut cam in cam_query.iter_mut() {
            cam.projection_mode = match cam.projection_mode {
                ProjectionMode::Perspective => ProjectionMode::Orthographic,
                ProjectionMode::Orthographic => ProjectionMode::Perspective,
            };
            
            // Update the actual projection component
            for mut projection in proj_query.iter_mut() {
                match cam.projection_mode {
                    ProjectionMode::Perspective => {
                        *projection = Projection::Perspective(PerspectiveProjection {
                            fov: 60.0_f32.to_radians(),
                            ..default()
                        });
                        info!("📷 Camera: Perspective Mode");
                    }
                    ProjectionMode::Orthographic => {
                        // Calculate ortho scale based on current distance
                        cam.ortho_scale = cam.distance * 0.5;
                        *projection = Projection::Orthographic(OrthographicProjection {
                            scale: cam.ortho_scale,
                            ..OrthographicProjection::default_3d()
                        });
                        info!("📷 Camera: Orthographic Mode");
                    }
                }
            }
        }
    }
}

/// Handle frame selection events (zoom to fit)
fn handle_frame_selection(
    mut events: MessageReader<FrameSelectionEvent>,
    mut query: Query<&mut EustressCamera, With<Camera3d>>,
    // Query for scene bounds (all meshes)
    mesh_query: Query<&GlobalTransform, With<Mesh3d>>,
) {
    for event in events.read() {
        // Calculate bounds
        let bounds = if let Some(b) = event.target_bounds {
            b
        } else {
            // Calculate scene bounds from all meshes
            let mut min = Vec3::splat(f32::MAX);
            let mut max = Vec3::splat(f32::MIN);
            let mut has_meshes = false;
            
            for transform in mesh_query.iter() {
                let pos = transform.translation();
                min = min.min(pos - Vec3::splat(1.0)); // Assume 1 unit padding
                max = max.max(pos + Vec3::splat(1.0));
                has_meshes = true;
            }
            
            if !has_meshes {
                // Default to origin with some extent
                min = Vec3::splat(-5.0);
                max = Vec3::splat(5.0);
            }
            
            (min, max)
        };
        
        let center = (bounds.0 + bounds.1) * 0.5;
        let extent = (bounds.1 - bounds.0).length();
        
        for mut cam in query.iter_mut() {
            // Move pivot to center of bounds
            cam.pivot = center;
            
            // Set distance to fit the extent (with some padding)
            // For perspective: distance = extent / (2 * tan(fov/2))
            // Simplified: distance ≈ extent * 1.5
            cam.distance = (extent * 1.5).max(MIN_CAMERA_DISTANCE).min(MAX_CAMERA_DISTANCE);
            
            // Update ortho scale if in orthographic mode
            if cam.projection_mode == ProjectionMode::Orthographic {
                cam.ortho_scale = extent * 0.6;
            }
            
            info!("📷 Camera: Framed to bounds (center: {:?}, extent: {:.1})", center, extent);
        }
    }
}

/// Animate view transitions for smooth snapping
fn animate_view_transition(
    time: Res<Time>,
    mut query: Query<&mut EustressCamera, With<Camera3d>>,
) {
    for mut cam in query.iter_mut() {
        if !cam.animating {
            continue;
        }
        
        cam.anim_progress += time.delta_secs() / cam.anim_duration;
        
        if cam.anim_progress >= 1.0 {
            // Animation complete
            cam.yaw = cam.anim_target_yaw;
            cam.pitch = cam.anim_target_pitch;
            cam.target_yaw = cam.anim_target_yaw;
            cam.target_pitch = cam.anim_target_pitch;
            cam.animating = false;
        } else {
            // Smooth interpolation (ease-out cubic)
            let t = 1.0 - (1.0 - cam.anim_progress).powi(3);
            
            // Interpolate yaw (handle wrap-around)
            let yaw_diff = angle_diff(cam.anim_start_yaw, cam.anim_target_yaw);
            cam.yaw = cam.anim_start_yaw + yaw_diff * t;
            cam.target_yaw = cam.yaw;
            
            // Interpolate pitch (no wrap-around needed)
            cam.pitch = cam.anim_start_pitch + (cam.anim_target_pitch - cam.anim_start_pitch) * t;
            cam.target_pitch = cam.pitch;
        }
    }
}

/// Calculate shortest angle difference (handles wrap-around)
fn angle_diff(from: f32, to: f32) -> f32 {
    let diff = to - from;
    // Normalize to [-PI, PI]
    if diff > PI {
        diff - 2.0 * PI
    } else if diff < -PI {
        diff + 2.0 * PI
    } else {
        diff
    }
}

/// Energizing controls for Eustress flow - builds positive momentum
fn eustress_camera_controls(
    mut ev_motion: MessageReader<MouseMotion>,
    mut ev_wheel: MessageReader<MouseWheel>,
    mut ev_touch: MessageReader<TouchInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut cam_query: Query<(&mut EustressCamera, &Transform, &Camera, &GlobalTransform), With<Camera3d>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let (mut cam, transform, camera, global_transform) = match cam_query.single_mut() {
        Ok(c) => c,
        Err(_) => return,
    };
    
    if !cam.enabled {
        return;
    }
    
    // TODO: Check Slint UI focus state to block input when UI has focus
    let ui_wants_keyboard = false;
    let ui_wants_pointer = false;
    
    // ALWAYS consume ALL mouse events to prevent buildup
    // Read mouse motion ONCE per frame
    let mut mouse_delta = Vec2::ZERO;
    for ev in ev_motion.read() {
        mouse_delta += ev.delta;
    }
    
    // ALWAYS consume ALL wheel events to prevent buildup
    let mut scroll_delta = 0.0;
    for ev in ev_wheel.read() {
        scroll_delta += if ev.unit == MouseScrollUnit::Line {
            ev.y
        } else {
            ev.y * 0.1
        };
    }
    
    // If UI wants pointer input, don't apply any camera changes
    // (but we already consumed the events above)
    if ui_wants_pointer {
        return;
    }
    
    // If UI wants keyboard, skip keyboard controls but allow mouse
    let dt = time.delta_secs();
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    
    // LOCAL SPACE: Get camera's actual local axes for intuitive movement
    let cam_forward = transform.forward();
    let cam_right = transform.right();
    let cam_up = transform.up();

    // Determine which mouse mode is active (mutually exclusive)
    let panning = mouse.pressed(MouseButton::Middle) || 
                  (mouse.pressed(MouseButton::Left) && shift);  // Only Shift+Left or Middle for pan
    
    let dollying = mouse.pressed(MouseButton::Right) && ctrl;
    
    // Right-click orbits (with or without Shift for precise mode)
    let orbiting = (mouse.pressed(MouseButton::Right) || 
                   (mouse.pressed(MouseButton::Left) && alt)) &&
                   !ctrl;  // Only orbit if not dollying
    
    // Apply the appropriate camera control (mutually exclusive)
    if orbiting && mouse_delta != Vec2::ZERO {
        // Orbit (Right-drag or Alt+Left-drag)
        // Shift slows down rotation for precise movements
        let sensitivity_mod = if shift { 0.25 } else { 1.0 };
        let sensitivity = cam.sensitivity * sensitivity_mod;
        
        // XZ plane (yaw) needs to be inverted for natural rotation
        cam.target_yaw -= mouse_delta.x * sensitivity;  // Mouse right = orbit right around object
        cam.target_pitch += mouse_delta.y * sensitivity; // Mouse up = look up
        cam.target_pitch = cam.target_pitch.clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
        
        // Mark as custom view when user manually orbits
        cam.current_view = CameraView::Custom;
        cam.animating = false; // Cancel any ongoing animation
    } else if panning && mouse_delta != Vec2::ZERO {
        // Pan (Middle-drag or Shift + Left-drag)
        let pan_speed = cam.pan_speed;
        let distance = cam.distance;
        // Use local camera axes for intuitive panning
        cam.pivot += cam_right * mouse_delta.x * pan_speed * distance;
        cam.pivot -= cam_up * mouse_delta.y * pan_speed * distance;
    } else if dollying && mouse_delta.y != 0.0 {
        // Dolly (Ctrl + Right-drag) - exponential for consistent feel
        let dolly_factor = (1.0 + mouse_delta.y * cam.sensitivity * 0.5).max(0.5).min(2.0);
        cam.distance *= dolly_factor;
        cam.distance = cam.distance.clamp(MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE);
    }

    // Zoom (Mouse Wheel) - zoom TOWARD the mouse cursor position
    // This provides intuitive zoom behavior like in Blender/Maya
    if scroll_delta != 0.0 {
        // Get cursor position for zoom-toward-cursor
        let cursor_pos = windows.single().ok().and_then(|w| w.cursor_position());
        
        // Calculate zoom factor (exponential for consistent feel at all distances)
        let zoom_factor = (1.0 - scroll_delta * 0.1 * cam.zoom_speed).max(0.5).min(2.0);
        let old_distance = cam.distance;
        let new_distance = (old_distance * zoom_factor).clamp(MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE);
        
        // Zoom toward cursor: move pivot toward the point under the cursor
        if let Some(cursor) = cursor_pos {
            // Get ray from camera through cursor
            if let Ok(ray) = camera.viewport_to_world(global_transform, cursor) {
                // Find a point along the ray at the current distance (approximate target point)
                // This is the point we want to zoom toward
                let zoom_target = ray.origin + *ray.direction * old_distance;
                
                // Calculate how much to move the pivot toward the zoom target
                // When zooming in (new_distance < old_distance), move pivot toward target
                // When zooming out (new_distance > old_distance), move pivot away from target
                let zoom_amount = 1.0 - (new_distance / old_distance);
                
                // Move pivot toward the zoom target proportionally
                let pivot_delta = (zoom_target - cam.pivot) * zoom_amount * 0.5;
                cam.pivot += pivot_delta;
            }
        }
        
        cam.distance = new_distance;
    }

    // Skip keyboard controls if UI wants keyboard input
    if ui_wants_keyboard {
        return;
    }
    
    // Skip WASD movement when Ctrl is pressed (Ctrl+D, Ctrl+C, etc. are shortcuts)
    if ctrl {
        return;
    }
    
    // Keyboard Pan (WASD/QE/Space) - DIRECT movement with NO momentum
    let base_speed = cam.base_speed;
    let speed_mod = if shift { 0.25 } else { 1.0 }; // Shift for PRECISE movement (slower)
    let move_speed = base_speed * speed_mod * dt;
    
    // Project forward onto horizontal plane for intuitive ground movement
    let forward_horizontal = Vec3::new(cam_forward.x, 0.0, cam_forward.z).normalize_or_zero();
    let right_horizontal = Vec3::new(cam_right.x, 0.0, cam_right.z).normalize_or_zero();
    
    // DIRECT pivot movement - NO velocity accumulation
    if keys.pressed(KeyCode::KeyW) { 
        cam.pivot += forward_horizontal * move_speed;
    }
    if keys.pressed(KeyCode::KeyS) { 
        cam.pivot -= forward_horizontal * move_speed;
    }
    if keys.pressed(KeyCode::KeyA) { 
        cam.pivot -= right_horizontal * move_speed;
    }
    if keys.pressed(KeyCode::KeyD) { 
        cam.pivot += right_horizontal * move_speed;
    }
    if keys.pressed(KeyCode::KeyQ) { 
        cam.pivot.y -= move_speed; // Down
    }
    if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::Space) { 
        cam.pivot.y += move_speed; // Up
    }
    if keys.pressed(KeyCode::Minus) {
        cam.pivot.y -= move_speed; // Down (with - key)
    }
    if keys.pressed(KeyCode::Equal) {
        cam.pivot.y += move_speed; // Up (with =/+ key)
    }

    // Touch handling for mobile empowerment
    for touch in ev_touch.read() {
        // Basic touch support - can be expanded
        match touch.phase {
            bevy::input::touch::TouchPhase::Started => {
                if (touch.id as usize) < cam.touch_start_positions.len() {
                    cam.touch_start_positions[touch.id as usize] = touch.position;
                }
            }
            bevy::input::touch::TouchPhase::Moved => {
                // Touch orbit/pan could be implemented here
            }
            _ => {}
        }
    }
    
    // Clear events if not used
    if !orbiting && !panning && !mouse.pressed(MouseButton::Right) {
        ev_motion.clear();
    }
}

/// Transform Update - INSTANT, RAW response with NO smoothing
fn update_eustress_camera_transform(
    mut query: Query<(&mut EustressCamera, &mut Transform), With<Camera3d>>,
    _time: Res<Time>,
) {
    for (mut cam, mut trans) in query.iter_mut() {
        // INSTANT rotation - NO interpolation
        cam.yaw = cam.target_yaw;
        cam.pitch = cam.target_pitch;
        
        let pitch = cam.pitch;
        let yaw = cam.yaw;
        let pivot = cam.pivot;
        let distance = cam.distance;
        
        // Safety check for NaN/infinity - silently fix without logging
        if !pivot.is_finite() || !distance.is_finite() || !pitch.is_finite() || !yaw.is_finite() {
            cam.pivot = Vec3::ZERO;
            cam.distance = 20.0;
            cam.pitch = 30.0_f32.to_radians();
            cam.yaw = 45.0_f32.to_radians();
            cam.target_pitch = cam.pitch;
            cam.target_yaw = cam.yaw;
            continue;
        }

        // Calculate camera position from pivot-based spherical coordinates
        let camera_pos = pivot + Vec3::new(
            distance * pitch.cos() * yaw.sin(),
            distance * pitch.sin(),
            distance * pitch.cos() * yaw.cos(),
        );

        // INSTANT position update - NO lerp
        trans.translation = camera_pos;
        trans.look_at(pivot, Vec3::Y);
    }
}

/// Add EustressCamera component to the main camera - empowering your creative vision
pub fn setup_camera_controller(
    mut commands: Commands,
    camera_query: Query<Entity, (With<Camera3d>, Without<EustressCamera>)>,
) {
    for entity in camera_query.iter() {
        commands.entity(entity).insert(EustressCamera::default());
        println!("✅ Eustress Camera: Empowering focus and flow enabled");
    }
}

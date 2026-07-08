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

/// Message to move the editor camera to look THROUGH another camera's pose
/// (fired by pressing F on a selected `Camera` object, e.g. the AI camera).
/// Unlike `FrameSelectionEvent` (which frames bounding-box bounds), this
/// reproduces a specific camera's viewpoint.
#[derive(Message, Debug, Clone)]
pub struct GoToCameraEvent {
    pub target: Entity,
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
            base_speed: 9.81,        // Direct WASD movement speed
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
            .add_message::<GoToCameraEvent>()
            .add_systems(Update, (
                ensure_camera_exists,
                update_camera_viewport_for_ui,
                camera_view_input_system
                    .after(crate::ui::slint_ui::update_slint_ui_focus),
                handle_snap_to_view,
                handle_toggle_projection,
                handle_frame_selection,
                handle_go_to_camera,
                animate_view_transition,
                eustress_camera_controls,
                update_eustress_camera_transform,
            ).chain());
    }
}

/// Keeps the 3D editor camera UN-clipped (full window). It renders DIRECTLY
/// to the window surface; the Slint overlay composites the chrome on top with
/// a transparent viewport hole, so the hole simply shows a CROP of the
/// full-window 3D render.
///
/// Why NOT clip the camera to the Slint hole: a crop does not distort — a
/// sphere stays round whether you view the whole framebuffer or a sub-rect of
/// it (proven by the fact that click-selection stays pixel-accurate through
/// `viewport_to_world` with `viewport == None`). So clipping buys no aspect
/// correctness; it only risks the "black boxes at the viewport edges" bug:
/// the clip rect comes from `ViewportBounds` (`viewport-sizer.absolute-
/// position`), and whenever that lags or disagrees with where the Slint hole
/// actually renders, the hole's top/left EDGES fall OUTSIDE the clipped rect
/// and expose the black clear-color. Un-clipped, every window pixel has 3D
/// behind the chrome, so a transparent gap or a hole edge shows the scene,
/// never black.
///
/// The genuine "box within a box" that once tempted us to clip was a STALE
/// overlay-texture bug (in-place `Image::resize()` not re-extracting to a new
/// wgpu::Texture) — fixed at the source in `handle_window_resize`, not here.
///
/// This system only clears any stale clip a previous build left on the window
/// camera; off-screen image cameras (the AI camera) keep their own full-image
/// target and are never touched.
fn update_camera_viewport_for_ui(
    mut camera_query: Query<(&mut Camera, &bevy::camera::RenderTarget), (With<Camera3d>, Without<crate::ui::slint_ui::SlintOverlayCamera>)>,
) {
    for (mut camera, target) in camera_query.iter_mut() {
        // Image-target cameras (e.g. the AI camera) manage their own full
        // image and must not be forced to the window's None viewport.
        if matches!(target, bevy::camera::RenderTarget::Image(_)) {
            continue;
        }
        if camera.viewport.is_some() {
            camera.viewport = None;
        }
    }
}

/// Ensure at least one camera exists - auto-spawn if all cameras are deleted
fn ensure_camera_exists(
    mut commands: Commands,
    camera_query: Query<
        Option<&bevy::camera::RenderTarget>,
        (With<Camera3d>, Without<crate::ui::slint_ui::SlintOverlayCamera>),
    >,
) {
    // Count only the WINDOW-targeted editor camera. Off-screen cameras (the AI
    // camera carries `RenderTarget::Image`) must NOT keep this guard satisfied:
    // otherwise deleting the editor camera while the AI camera exists leaves the
    // window with no controllable `EustressCamera`, so WASD, F-to-frame and
    // F-to-camera all die and the view is stuck at the origin. A camera with no
    // `RenderTarget` component defaults to the primary window, so `None` counts
    // as an editor camera; only `Some(Image)` is excluded.
    let has_window_camera = camera_query
        .iter()
        .any(|target| !matches!(target, Some(bevy::camera::RenderTarget::Image(_))));
    if !has_window_camera {
        info!("📷 No window camera found - spawning new editor camera");
        
        // Create EustressCamera with proper initialization
        let mut cam = EustressCamera::default();
        cam.pivot = Vec3::ZERO;
        cam.distance = 20.0;
        cam.yaw = std::f32::consts::FRAC_PI_4;
        cam.pitch = -0.5;
        cam.enabled = true;
        
        // Route through the ONE canonical bundle constructor (see its doc
        // comment) instead of hand-rolling Camera3d/Tonemapping/Projection
        // here: a second, independently-drifting camera build is exactly the
        // mesh_view_bind_group hazard that function exists to prevent (this
        // fallback previously used Tonemapping::AcesFitted + a different
        // Projection far-plane than every other Studio camera).
        commands.spawn((
            crate::default_scene::studio_camera_bundle(
                "Camera",
                Transform::from_xyz(10.0, 10.0, 15.0).looking_at(Vec3::ZERO, Vec3::Y),
            ),
            cam,
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
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
) {
    // Block view shortcuts when a text input has focus or overlay modal is open
    if ui_focus.as_ref().map(|f| f.text_input_focused).unwrap_or(false) { return; }
    if crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) { return; }
    
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
    
    // 5 / Num5: Toggle Orthographic/Perspective. Accept both the top-row and
    // numpad keys since most keyboards don't have a numpad.
    if keys.just_pressed(KeyCode::Numpad5) || keys.just_pressed(KeyCode::Digit5) {
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
    mut cam_query: Query<(Entity, &mut EustressCamera, &mut Projection), With<Camera3d>>,
) {
    for _event in events.read() {
        for (_entity, mut cam, mut projection) in cam_query.iter_mut() {
            cam.projection_mode = match cam.projection_mode {
                ProjectionMode::Perspective => ProjectionMode::Orthographic,
                ProjectionMode::Orthographic => ProjectionMode::Perspective,
            };

            match cam.projection_mode {
                ProjectionMode::Perspective => {
                    *projection = Projection::Perspective(PerspectiveProjection {
                        fov: 60.0_f32.to_radians(),
                        ..default()
                    });
                    info!("📷 Camera: Perspective Mode");
                }
                ProjectionMode::Orthographic => {
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
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    studio_state: Option<Res<crate::ui::StudioState>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    select_state: Option<Res<crate::select_tool::SelectToolState>>,
) {
    let (mut cam, transform, camera, global_transform) = match cam_query.single_mut() {
        Ok(c) => c,
        Err(_) => return,
    };
    
    if !cam.enabled {
        return;
    }

    // Block ALL camera input when ANY modal dialog is open.
    // Consume events to prevent buildup, then return early.
    let modal_open = studio_state.as_ref().map_or(false, |s| {
        s.show_settings_window
            || s.show_soul_settings_window
            || s.show_keybindings_window
            || s.show_publish_dialog
            || s.show_forge_connect_window
            || s.show_stress_test_window
            || s.show_global_sources_window
            || s.show_domains_window
            || s.show_global_variables_window
            || s.show_sync_domain_modal
            || s.show_exit_confirmation
            || s.show_find_dialog
    });
    if modal_open {
        ev_motion.clear();
        ev_wheel.clear();
        ev_touch.clear();
        return;
    }
    
    // Check if cursor is inside the 3D viewport (not over Explorer,
    // Properties, Output, or any other Slint panel). Delegate to
    // `ui_focus.has_focus` — it's already the authoritative
    // cursor-over-UI signal and uses the scale-corrected viewport
    // bounds (see update_slint_ui_focus). Rolling our own bounds
    // check here previously compared `window.cursor_position()`
    // (logical pixels) to ViewportBounds (physical pixels), which
    // mis-classified the Output panel area as "inside viewport" on
    // high-DPI displays — so wheel events hovering the Output
    // scrolled its text AND zoomed the 3D scene.
    //
    // Fall back to the raw bounds check when SlintUIFocus isn't
    // available (pre-UI-init frames) so camera input still works.
    let cursor_in_viewport = if let Some(f) = ui_focus.as_ref() {
        !f.has_focus
    } else if let (Some(vb), Ok(window)) = (viewport_bounds.as_deref(), windows.single()) {
        window.cursor_position().map(|pos| {
            vb.contains_logical(pos, window.scale_factor() as f32)
        }).unwrap_or(true)
    } else {
        true
    };
    
    // Block keyboard camera controls when a Slint text input has focus
    // (typing in Workshop chat, command bar, Properties, etc.)
    let ui_wants_keyboard = ui_focus.as_ref().map(|f| f.text_input_focused).unwrap_or(false)
        || crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed);
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

    // Ctrl+Shift+Alt+wheel is reserved for the hover-resize gesture
    // (`part_selection::hover_resize_system`) — neutralize the camera's zoom
    // contribution for that chord so the wheel resizes ONLY the part under
    // the cursor, not the camera distance too. The wheel events were already
    // drained above (to prevent buildup); we just zero their zoom effect.
    if ctrl && shift && alt {
        scroll_delta = 0.0;
    }

    // Same neutralization, for the same reason, while a node is being
    // dragged through empty space: `select_tool::handle_drag_distance_wheel`
    // has its own MessageReader<MouseWheel> and reads these same notches to
    // change the drag leash length instead. Without this, one scroll notch
    // would both change the leash AND fly the camera forward/back.
    let dragging_node = select_state.as_ref().is_some_and(|s| s.dragging && s.drag_started);
    if dragging_node {
        scroll_delta = 0.0;
    }

    // LOCAL SPACE: Get camera's actual local axes for intuitive movement
    let cam_forward = transform.forward();
    let cam_right = transform.right();
    let cam_up = transform.up();

    // Determine which mouse mode is active (mutually exclusive)
    let panning = mouse.pressed(MouseButton::Middle) || 
                  (mouse.pressed(MouseButton::Left) && shift);  // Only Shift+Left or Middle for pan
    
    let dollying = mouse.pressed(MouseButton::Right) && ctrl;

    // Roblox-Studio-style split:
    //  - Right-drag = LOOK IN PLACE (first-person): the camera position
    //    stays fixed and only the view direction turns. Previously
    //    right-drag ORBITED the pivot, so at large distances the camera
    //    swung on a huge arc — the "ball on a track" feel.
    //  - Alt+Left-drag = classic ORBIT around the pivot (kept for
    //    inspect-an-object workflows).
    let looking  = mouse.pressed(MouseButton::Right) && !ctrl;
    let orbiting = mouse.pressed(MouseButton::Left) && alt && !ctrl;

    /// Spherical offset from pivot to camera — MUST match
    /// `update_eustress_camera_transform`'s position formula.
    fn orbit_offset(yaw: f32, pitch: f32) -> Vec3 {
        Vec3::new(
            pitch.cos() * yaw.sin(),
            pitch.sin(),
            pitch.cos() * yaw.cos(),
        )
    }

    // Apply the appropriate camera control (mutually exclusive)
    if (looking || orbiting) && mouse_delta != Vec2::ZERO {
        // Shift slows down rotation for precise movements
        let sensitivity_mod = if shift { 0.25 } else { 1.0 };
        let sensitivity = cam.sensitivity * sensitivity_mod;

        // Capture the camera's world position BEFORE the angles change
        // (from the TARGET angles — the transform system applies targets
        // instantly, so they are the authoritative pose).
        let cam_pos = cam.pivot + cam.distance * orbit_offset(cam.target_yaw, cam.target_pitch);

        // XZ plane (yaw) needs to be inverted for natural rotation
        cam.target_yaw -= mouse_delta.x * sensitivity;  // Mouse right = look/orbit right
        cam.target_pitch += mouse_delta.y * sensitivity; // Mouse up = look up
        cam.target_pitch = cam.target_pitch.clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);

        if looking {
            // First-person look: keep the camera where it is and swing the
            // PIVOT to sit `distance` ahead along the new view direction.
            // Zoom, F-focus, and orbit still have a sensible pivot ahead of
            // the camera, but turning no longer translates the camera.
            cam.pivot = cam_pos - cam.distance * orbit_offset(cam.target_yaw, cam.target_pitch);
        }

        // Mark as custom view when user manually rotates
        cam.current_view = CameraView::Custom;
        cam.animating = false; // Cancel any ongoing animation
    } else if panning && mouse_delta != Vec2::ZERO {
        // Pan (Middle-drag or Shift + Left-drag)
        // Shift slows down panning for precise movements
        let pan_mod = if shift { 0.25 } else { 1.0 };
        let pan_speed = cam.pan_speed * pan_mod;
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

    // Zoom (Mouse Wheel) - only when cursor is inside the 3D viewport and
    // no Slint text input/editor has focus. This lets panels scroll freely
    // and prevents zoom while editing scripts in the center tab.
    if scroll_delta != 0.0 && (!cursor_in_viewport || ui_wants_keyboard) {
        scroll_delta = 0.0;
    }
    
    // CONTINUOUS fly-zoom (mouse wheel) — translate the whole camera rig
    // THROUGH the world, never a bounded orbit radius.
    //
    // Every prior version zoomed by changing `cam.distance` (the orbit radius),
    // clamped to MIN/MAX_CAMERA_DISTANCE. That is inherently "min/max based":
    // because `update_eustress_camera_transform` derives the camera position as
    // `pivot + distance * offset` and always `look_at(pivot)`, shrinking
    // `distance` only crawls the camera toward a FIXED pivot and slams into the
    // MIN wall — you can never fly *through* what you're looking at. That is the
    // "ball on a track / can't zoom further" feel. Making the per-step factor
    // exponential (the last attempt) didn't change this — it still bottoms out
    // at MIN_CAMERA_DISTANCE.
    //
    // The right model: scroll TRANSLATES `cam.pivot` along the view (the pivot
    // moves through the world), holding `distance` constant. Since the camera
    // position tracks `pivot + distance * offset`, the entire rig slides by the
    // same vector — a genuine, unbounded dolly. Scroll in and you fly straight
    // past objects; scroll out and you retreat forever. No clamp, no wall,
    // continuous at any scale because the step is a constant FRACTION of the
    // current view distance (so it feels identical at 1 m or 10 km).
    if scroll_delta != 0.0 {
        // Per-notch travel as a fraction of the current view distance. `1 - 0.9^n`
        // ⇒ scroll-in (+) moves forward, scroll-out (−) moves back, symmetric and
        // smooth across a burst of merged wheel events in one frame.
        const ZOOM_STEP: f32 = 0.9; // ~10% of view distance per line at zoom_speed = 1.0
        let travel = cam.distance * (1.0 - ZOOM_STEP.powf(scroll_delta * cam.zoom_speed));

        // Fly along the cursor ray when we have one (keeps whatever is under the
        // cursor roughly pinned on screen — the Blender/Unreal feel); otherwise
        // straight along the view forward. `distance` is deliberately untouched,
        // so orbit (Alt+drag) and pan keep a sensible radius after flying.
        let dir = windows
            .single()
            .ok()
            .and_then(|w| w.cursor_position())
            .and_then(|cursor| camera.viewport_to_world(global_transform, cursor).ok())
            .map(|ray| *ray.direction)
            .unwrap_or(*cam_forward);

        cam.pivot += dir * travel;
        cam.current_view = CameraView::Custom;
        cam.animating = false;
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
    let speed_mod = if shift { 0.075 } else { 1.0 }; // Shift for PRECISE movement (slower)
    let move_speed = base_speed * speed_mod * dt;
    
    // Roblox-Studio-style fly: W/S move along the camera's ACTUAL look
    // direction (including vertical — look down + W descends toward what
    // you're looking at), A/D strafe along the camera's right vector
    // (no roll, so it's already horizontal). The old code projected W/S
    // onto the ground plane, which fought the "fly where I look" instinct
    // and contributed to the tracked-ball feel.
    let strafe_right = Vec3::new(cam_right.x, 0.0, cam_right.z).normalize_or_zero();

    // DIRECT pivot movement - NO velocity accumulation
    // (the camera position is derived from the pivot, so it moves 1:1)
    if keys.pressed(KeyCode::KeyW) {
        cam.pivot += *cam_forward * move_speed;
    }
    if keys.pressed(KeyCode::KeyS) {
        cam.pivot -= *cam_forward * move_speed;
    }
    if keys.pressed(KeyCode::KeyA) {
        cam.pivot -= strafe_right * move_speed;
    }
    if keys.pressed(KeyCode::KeyD) {
        cam.pivot += strafe_right * move_speed;
    }
    // Q/E (+ Space) move along the CAMERA's up axis, not world-Y. So the
    // vertical keys are relative to where you're looking: pitched level, E
    // rises straight up; pitched fully down, the camera's up vector points
    // forward, so E flies you forward toward the ground you're looking at
    // (and Q backward/up), instead of always sliding straight up the world
    // Y axis. `cam_up` (transform.up()) is already unit-length and is the
    // same vector the middle-drag pan uses for its vertical, so the fly and
    // pan verticals stay consistent.
    if keys.pressed(KeyCode::KeyQ) {
        cam.pivot -= *cam_up * move_speed; // Down (camera-relative)
    }
    if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::Space) {
        cam.pivot += *cam_up * move_speed; // Up (camera-relative)
    }
    // `-` / `=` are intentionally NOT bound to camera vertical here.
    // Those keys belong to `Action::NudgeUp` / `Action::NudgeDown` —
    // moving the selected PART up / down by one grid unit (handled in
    // `keybindings.rs::handle_nudge_keys`). Binding the camera to the
    // same keys ran both systems every frame: the part nudged up by
    // `snap` while the camera pivoted down by `move_speed × dt`,
    // visually cancelling the nudge — the user reads that as
    // "Move Up / Move Down don't work". Q/E/Space remain the
    // camera-vertical keys.

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

/// F on a selected `Camera` object → move the editor camera to look THROUGH it.
///
/// Inverts the orbit math in `update_eustress_camera_transform` (which places
/// the camera at `pivot + distance·(cos·sin, sin, cos·cos)` looking at `pivot`).
/// To reproduce the target camera's pose (position `P`, forward `F`) we set the
/// orbit `dir = -F`, derive yaw/pitch from it, and put the pivot `distance`
/// ahead of `P` (`pivot = P + F·distance`) — so the editor camera ends up at
/// `P` looking along `F`. After this, normal WASD/right-click resumes from that
/// pose (a continuous "follow" mode is a future addition on top of this).
fn handle_go_to_camera(
    mut events: MessageReader<GoToCameraEvent>,
    targets: Query<&GlobalTransform>,
    mut editor: Query<&mut EustressCamera, With<Camera3d>>,
) {
    let Some(ev) = events.read().last() else { return };
    let Ok(target_gt) = targets.get(ev.target) else { return };
    let Ok(mut cam) = editor.single_mut() else { return };

    let (_scale, rot, pos) = target_gt.to_scale_rotation_translation();
    let fwd = (rot * Vec3::NEG_Z).normalize_or_zero();
    if fwd == Vec3::ZERO {
        return;
    }
    let dir = -fwd;
    let pitch = dir.y.clamp(-1.0, 1.0).asin();
    let yaw = dir.x.atan2(dir.z);
    let distance = cam.distance.max(1.0);

    cam.pivot = pos + fwd * distance;
    cam.yaw = yaw;
    cam.pitch = pitch;
    cam.target_yaw = yaw;
    cam.target_pitch = pitch;
    info!("📷 F → editor camera now looking through camera {:?}", ev.target);
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

/// Add `EustressCamera` (WASD/right-click controls) to the WINDOW camera only.
///
/// Off-screen image cameras — e.g. the independent AI camera — are driven
/// programmatically via their own bridge methods, NOT the editor controls.
/// Giving one `EustressCamera` would create a second controlled camera, which
/// makes the input systems' `single_mut::<EustressCamera>()` return `Err`
/// (ambiguous) and silently kills WASD/right-click on the real camera. So we
/// skip any camera whose render target is an Image.
pub fn setup_camera_controller(
    mut commands: Commands,
    camera_query: Query<(Entity, &bevy::camera::RenderTarget), (With<Camera3d>, Without<EustressCamera>, Without<crate::ui::slint_ui::SlintOverlayCamera>)>,
) {
    for (entity, target) in camera_query.iter() {
        if matches!(target, bevy::camera::RenderTarget::Image(_)) {
            continue; // off-screen camera (AI camera) — not user-controlled
        }
        commands.entity(entity).insert(EustressCamera::default());
        println!("✅ Eustress Camera: controls enabled on entity {:?}", entity);
    }
}

use bevy::prelude::*;
use bevy::camera::ScalingMode;
use bevy::input::mouse::{MouseMotion, MouseWheel, MouseScrollUnit};
use bevy::input::touch::TouchInput;
use std::f32::consts::{FRAC_PI_2, PI};

// Camera distance constraints - effectively infinite zoom
const MIN_CAMERA_DISTANCE: f32 = 0.001;   // Allow extremely close zoom (1mm)
const MAX_CAMERA_DISTANCE: f32 = 1000000.0;  // Allow extremely far zoom (1000km)

/// Seconds a projection switch or a 2D/3D switch takes to animate.
const VIEW_TRANSITION_SECS: f32 = 0.35;
/// Seconds a snap to an axis view takes.
const SNAP_SECS: f32 = 0.2;
/// Seconds the flight to a saved viewpoint takes.
const VIEWPOINT_SECS: f32 = 0.45;
/// Narrowest field of view (2 degrees) the dolly zoom reaches before the true
/// orthographic projection takes over. At 2 degrees a scene as deep as the
/// pivot distance renders within about 2.5% of its orthographic size, so the
/// hand-over is invisible, while the camera only backs away about 40 pivot
/// distances, which keeps aerial haze out of the transition.
const DOLLY_MIN_FOV: f32 = 0.034_906_585;
/// Depth kept visible on each side of the 2D working plane.
pub(crate) const PLANE_2D_DEPTH: f32 = 5_000.0;
/// Depth kept visible beyond the pivot in 3D orthographic.
pub(crate) const ORTHO_3D_DEPTH: f32 = 20_000.0;

// ============================================================================
// Perspective: 3D, 2D, perspective and orthographic
// ============================================================================
//
// Two switches decide how the editor camera shows the world:
//
// - `ViewDimension`: 3D (orbit, look, fly) or 2D (locked to one axis plane,
//   pan and zoom only). 2D is always orthographic.
// - `ProjectionMode`: perspective (things shrink with distance) or
//   orthographic (parallel rays, sizes read true at every depth).
//
// Bevy does not model projection as a boolean. It is the `Projection`
// component, an enum of `Perspective(PerspectiveProjection)` and
// `Orthographic(OrthographicProjection)`, because each mode carries its own
// parameters: a field of view for one, a view size for the other. This
// controller owns that component and rewrites it from the state below.
//
// ONE zoom quantity drives both projections: the orbit `distance`. The
// orthographic view height is derived from it,
//
//     ortho_height = 2 * distance * tan(fov / 2)
//
// which is exactly the height of the pivot plane in the perspective view. So
// switching projection never changes the size of what sits at the pivot, and
// everything that already reads `distance` (framing, spawn focus, portals,
// level of detail) keeps working in orthographic without knowing about it.
//
// The switch animates as a dolly zoom: the field of view narrows while the
// camera backs away so the pivot plane holds its size, and the true
// orthographic projection takes over once perspective has flattened out.
// Leaving orthographic plays the same move in reverse.
//
// 2D follows Bevy's own 2D convention by default: the XY plane, +Y up, the
// camera looking down -Z, so depth is draw order. Bevy's 2D renderer
// (`Camera2d`, `Sprite`, `Mesh2d`) can therefore stack on top of this view
// with a matching orthographic projection and register one to one.
//
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
// Keys (plain numpad keys and 5 go through the key-binding table):
// - Num2: Front View (+Z looking toward -Z)
// - Num4: Left View (-X looking toward +X)
// - Num6: Right View (+X looking toward -X)
// - Num8: Top View (+Y looking toward -Y)
// - Num5 / 5: Toggle Orthographic/Perspective
// - Alt+2 / Alt+3: 2D / 3D
// - Num.: Frame everything
// - Ctrl+Num: Opposite views (Back, Right, Left, Bottom)
// In 2D the view keys pick the working plane.
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

    /// Parse a view name as written by `name` (any case).
    pub fn from_name(name: &str) -> Option<CameraView> {
        match name.trim().to_ascii_lowercase().as_str() {
            "front" => Some(CameraView::Front),
            "back" => Some(CameraView::Back),
            "left" => Some(CameraView::Left),
            "right" => Some(CameraView::Right),
            "top" => Some(CameraView::Top),
            "bottom" => Some(CameraView::Bottom),
            "custom" | "user" => Some(CameraView::Custom),
            _ => None,
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

    /// Exact camera basis `(forward, up)` for an axis view; `None` for Custom.
    ///
    /// Axis views render from this basis rather than from `angles()`, because
    /// Top and Bottom stop 0.001 rad short of vertical (a `look_at`
    /// singularity) and that tilt shows in orthographic: a background grid a
    /// few kilometres behind the pivot slides several metres against the
    /// parts. The ups match what `look_at` produces for those angles, so the
    /// hand-over at the end of a snap animation does not roll the view.
    pub fn basis(&self) -> Option<(Vec3, Vec3)> {
        match self {
            CameraView::Front => Some((Vec3::NEG_Z, Vec3::Y)),
            CameraView::Back => Some((Vec3::Z, Vec3::Y)),
            CameraView::Right => Some((Vec3::NEG_X, Vec3::Y)),
            CameraView::Left => Some((Vec3::X, Vec3::Y)),
            CameraView::Top => Some((Vec3::NEG_Y, Vec3::NEG_Z)),
            CameraView::Bottom => Some((Vec3::Y, Vec3::Z)),
            CameraView::Custom => None,
        }
    }

    /// The world plane an axis view looks at.
    pub fn plane_label(&self) -> &'static str {
        match self {
            CameraView::Front | CameraView::Back => "XY",
            CameraView::Top | CameraView::Bottom => "XZ",
            CameraView::Left | CameraView::Right => "YZ",
            CameraView::Custom => "",
        }
    }
}

/// Camera projection. The two `Projection` variants the editor uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectionMode {
    #[default]
    Perspective,
    Orthographic,
}

impl ProjectionMode {
    pub fn label(self) -> &'static str {
        match self {
            ProjectionMode::Perspective => "Perspective",
            ProjectionMode::Orthographic => "Orthographic",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            ProjectionMode::Perspective => ProjectionMode::Orthographic,
            ProjectionMode::Orthographic => ProjectionMode::Perspective,
        }
    }
}

/// Whether the editor camera views the whole 3D world or works on a plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewDimension {
    #[default]
    ThreeD,
    /// Orthographic, locked to `EustressCamera::plane_view`, pan and zoom only.
    TwoD,
}

impl ViewDimension {
    pub fn label(self) -> &'static str {
        match self {
            ViewDimension::ThreeD => "3D",
            ViewDimension::TwoD => "2D",
        }
    }
}

/// The 3D view to return to when leaving 2D.
#[derive(Debug, Clone, Copy)]
pub struct Saved3dView {
    pub yaw: f32,
    pub pitch: f32,
    pub projection: ProjectionMode,
    pub view: CameraView,
}

/// A complete editor view: where the orbit sits, how it faces, and the
/// Perspective it is seen in. What a saved viewpoint stores and restores
/// (`EustressCamera::pose` / `EustressCamera::go_to`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewPose {
    pub pivot: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub dimension: ViewDimension,
    pub projection: ProjectionMode,
    /// The axis view on screen, or Custom.
    pub view: CameraView,
    /// The 2D working plane.
    pub plane: CameraView,
}

impl ViewPose {
    /// Unit look direction at rest: the exact axis for an axis view (and
    /// always in 2D), else from the orbit angles.
    pub fn forward(&self) -> Vec3 {
        self.axis_basis()
            .map(|(forward, _)| forward)
            .unwrap_or_else(|| -orbit_offset(self.yaw, self.pitch))
    }

    /// Where the camera sits and how it is turned, at rest.
    pub fn camera_transform(&self) -> Transform {
        let (forward, up) = self
            .axis_basis()
            .unwrap_or_else(|| (-orbit_offset(self.yaw, self.pitch), Vec3::Y));
        Transform::from_translation(self.pivot - forward * self.distance).looking_to(forward, up)
    }

    /// A 3D perspective pose rebuilt from a camera transform alone, with the
    /// pivot `distance` ahead of the camera: how a viewpoint saved before
    /// the orbit was recorded comes back.
    pub fn from_camera_transform(transform: &Transform, distance: f32) -> ViewPose {
        let forward = *transform.forward();
        let back = -forward;
        ViewPose {
            pivot: transform.translation + forward * distance,
            distance,
            yaw: back.x.atan2(back.z),
            pitch: back.y.clamp(-1.0, 1.0).asin(),
            dimension: ViewDimension::ThreeD,
            projection: ProjectionMode::Perspective,
            view: CameraView::Custom,
            plane: CameraView::Front,
        }
    }

    fn axis_basis(&self) -> Option<(Vec3, Vec3)> {
        match self.dimension {
            ViewDimension::TwoD => self.plane.basis(),
            ViewDimension::ThreeD => self.view.basis(),
        }
    }
}

/// Pivot and orbit distance easing alongside the angle animation, for moves
/// that travel (going to a viewpoint) rather than only turn (a view snap).
#[derive(Debug, Clone, Copy)]
pub struct OrbitTween {
    pub from_pivot: Vec3,
    pub to_pivot: Vec3,
    pub from_distance: f32,
    pub to_distance: f32,
}

/// A request to change how the editor camera shows the world.
///
/// Keyboard shortcuts (through `Action::View*`), the View menu, the
/// Perspective control in the viewport tab bar, the Properties panel and the
/// bridge's `invoke_action` all arrive here, so every switch runs through one
/// set of transitions.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub enum ViewCommand {
    SetDimension(ViewDimension),
    ToggleDimension,
    /// Ignored in 2D, which is always orthographic.
    SetProjection(ProjectionMode),
    ToggleProjection,
    /// Snap to an axis view. In 2D this picks the working plane.
    Snap(CameraView),
    /// Perspective vertical field of view, in degrees.
    SetFieldOfView(f32),
    /// Orthographic view height, in metres.
    SetOrthographicSize(f32),
    /// Frame the whole scene.
    FrameAll,
}

impl ViewCommand {
    /// Parse the string form the Slint UI sends through `set-view-mode`:
    /// `2d`, `3d`, `toggle-dimension`, `perspective`, `orthographic`,
    /// `toggle-projection`, an axis view name (`top`, `front`, ...),
    /// `fov:<degrees>`, `ortho-size:<metres>` or `frame-all`.
    pub fn parse(command: &str) -> Option<Self> {
        let command = command.trim().to_ascii_lowercase();
        let number = |v: &str| v.trim().parse::<f32>().ok().filter(|v| v.is_finite());
        if let Some(v) = command.strip_prefix("fov:") {
            return number(v).map(ViewCommand::SetFieldOfView);
        }
        if let Some(v) = command.strip_prefix("ortho-size:") {
            return number(v).map(ViewCommand::SetOrthographicSize);
        }
        match command.as_str() {
            "2d" => Some(ViewCommand::SetDimension(ViewDimension::TwoD)),
            "3d" => Some(ViewCommand::SetDimension(ViewDimension::ThreeD)),
            "toggle-dimension" => Some(ViewCommand::ToggleDimension),
            "perspective" => Some(ViewCommand::SetProjection(ProjectionMode::Perspective)),
            "orthographic" => Some(ViewCommand::SetProjection(ProjectionMode::Orthographic)),
            "toggle-projection" => Some(ViewCommand::ToggleProjection),
            "frame-all" => Some(ViewCommand::FrameAll),
            other => CameraView::from_name(other)
                .filter(|v| *v != CameraView::Custom)
                .map(ViewCommand::Snap),
        }
    }
}

/// Commands raised outside a system that can hold a `MessageWriter`: the
/// Slint `set-view-mode` callback and the Properties panel rows, both handled
/// inside `drain_slint_actions`, which is at Bevy's parameter limit. Drained
/// once per frame by `apply_view_commands`, after the message queue.
static VIEW_COMMAND_INBOX: std::sync::Mutex<Vec<ViewCommand>> = std::sync::Mutex::new(Vec::new());

/// Queue a `ViewCommand` from code that has no `MessageWriter<ViewCommand>`.
pub fn queue_view_command(command: ViewCommand) {
    if let Ok(mut inbox) = VIEW_COMMAND_INBOX.lock() {
        inbox.push(command);
    }
}

/// The working plane while the editor is in 2D; `None` in 3D.
///
/// Published every frame for the tools that must stay on it: a drag in 2D
/// slides along this plane instead of snapping onto surfaces behind it, and
/// Insert drops new parts onto it at the view centre.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct ActiveViewPlane(pub Option<ViewPlane>);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewPlane {
    /// Unit normal, pointing at the camera.
    pub normal: Vec3,
    /// The view centre, on the plane.
    pub center: Vec3,
}

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
    /// Orbit radius. Also the zoom of the orthographic view: see
    /// `ortho_height` and the module header.
    pub distance: f32,
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
    /// Projection asked for in 3D. 2D is orthographic regardless.
    #[reflect(ignore)]
    pub projection_mode: ProjectionMode,
    #[reflect(ignore)]
    pub dimension: ViewDimension,
    /// Axis view 2D works in. Front (the XY plane) matches Bevy's 2D convention.
    #[reflect(ignore)]
    pub plane_view: CameraView,
    #[reflect(ignore)]
    pub saved_3d: Option<Saved3dView>,
    /// Perspective vertical field of view, radians. Seeded from the spawned
    /// `Projection`, and it follows edits made straight to that component.
    pub fov: f32,
    /// Perspective near clip distance.
    pub near: f32,
    /// Perspective far distance (frustum culling only: the projection itself
    /// is infinite reverse-Z).
    pub far: f32,
    /// 0 = perspective, 1 = orthographic. Animated between the two.
    pub ortho_blend: f32,
    /// The last perspective `(fov, near, far)` this controller wrote. A
    /// component that differs was edited by someone else and is adopted.
    #[reflect(ignore)]
    pub written_perspective: Option<(f32, f32, f32)>,
    /// Cursor position on the previous frame of a one-to-one pan drag.
    #[reflect(ignore)]
    pub pan_anchor: Option<Vec2>,
    // Animation state for smooth view transitions
    pub animating: bool,          // True during view transition
    pub anim_start_yaw: f32,
    pub anim_start_pitch: f32,
    pub anim_target_yaw: f32,
    pub anim_target_pitch: f32,
    pub anim_progress: f32,
    pub anim_duration: f32,
    /// Pivot and distance riding the same animation, when it travels.
    /// Cleared by every angle-only animation (`animate_angles_to`).
    #[reflect(ignore)]
    pub anim_orbit: Option<OrbitTween>,
    /// Set by F (`handle_frame_selection`) whenever it snaps `distance` to
    /// fit a selection's bounds. Cleared — resetting `distance` back to the
    /// default orbit radius — the next time the SELECTION changes (see
    /// `reset_focus_distance_on_selection_change`). Without this, Alt+Left
    /// orbit (which swings the camera around `pivot` at `distance` without
    /// re-deriving either from the camera's current pose — unlike right-
    /// drag look, which re-tethers `pivot` every frame) keeps using
    /// whatever radius the last F-focused object happened to fit, so
    /// orbiting after selecting something unrelated feels like a giant
    /// sweep (radius fit to a building) or a tight pirouette (radius fit to
    /// a bolt) instead of the normal default orbit feel.
    pub focus_anchored: bool,
}

impl EustressCamera {
    /// Visible height of the orthographic view, in metres. The same number is
    /// the height of the pivot plane in the perspective view.
    pub fn ortho_height(&self) -> f32 {
        2.0 * self.distance * (self.fov * 0.5).tan()
    }

    /// Zoom the orthographic view to `height` metres (moves the orbit radius).
    pub fn set_ortho_height(&mut self, height: f32) {
        let tan_half = (self.fov * 0.5).tan().max(1e-4);
        self.distance = (height / (2.0 * tan_half)).clamp(MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE);
    }

    pub fn is_2d(&self) -> bool {
        self.dimension == ViewDimension::TwoD
    }

    /// Whether the projection is headed for (or settled in) orthographic.
    pub fn wants_ortho(&self) -> bool {
        self.is_2d() || self.projection_mode == ProjectionMode::Orthographic
    }

    /// The axis view the camera currently renders from exactly, if any.
    pub(crate) fn exact_view(&self) -> Option<CameraView> {
        if self.animating {
            None
        } else if self.is_2d() {
            Some(self.plane_view)
        } else {
            Some(self.current_view).filter(|v| *v != CameraView::Custom)
        }
    }

    /// Unit forward, and the camera position at rest (no dolly pull-back).
    pub fn frame(&self) -> (Vec3, Vec3) {
        let forward = self
            .exact_view()
            .and_then(|v| v.basis())
            .map(|(forward, _)| forward)
            .unwrap_or_else(|| -orbit_offset(self.target_yaw, self.target_pitch));
        (forward, self.pivot - forward * self.distance)
    }

    /// The view as it will rest once any animation in flight finishes.
    pub fn pose(&self) -> ViewPose {
        let (yaw, pitch) = if self.animating {
            (self.anim_target_yaw, self.anim_target_pitch)
        } else {
            (self.target_yaw, self.target_pitch)
        };
        let (pivot, distance) = match self.anim_orbit.filter(|_| self.animating) {
            Some(orbit) => (orbit.to_pivot, orbit.to_distance),
            None => (self.pivot, self.distance),
        };
        ViewPose {
            pivot,
            distance,
            yaw,
            pitch,
            dimension: self.dimension,
            projection: self.projection_mode,
            view: self.current_view,
            plane: self.plane_view,
        }
    }

    /// Move to `pose`, easing there like a view snap when `animate`, and
    /// taking its Perspective along: a 2D pose enters 2D (remembering the
    /// 3D view to come back to), an orthographic one flattens the view.
    pub fn go_to(&mut self, pose: ViewPose, animate: bool) {
        if !(pose.pivot.is_finite() && pose.distance.is_finite() && pose.yaw.is_finite() && pose.pitch.is_finite()) {
            return;
        }
        match pose.dimension {
            ViewDimension::TwoD if !self.is_2d() => {
                self.saved_3d = Some(Saved3dView {
                    yaw: self.target_yaw,
                    pitch: self.target_pitch,
                    projection: self.projection_mode,
                    view: self.current_view,
                });
            }
            ViewDimension::ThreeD => self.saved_3d = None,
            _ => {}
        }
        self.dimension = pose.dimension;
        self.projection_mode = pose.projection;
        if pose.plane != CameraView::Custom {
            self.plane_view = pose.plane;
        }
        let distance = pose.distance.clamp(MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE);
        // 2D always faces its plane square on, whatever angles came along.
        let (yaw, pitch) = if self.is_2d() {
            self.plane_view.angles()
        } else {
            (pose.yaw, pose.pitch.clamp(-FRAC_PI_2 + 0.001, FRAC_PI_2 - 0.001))
        };
        if animate {
            animate_angles_to(self, yaw, pitch, VIEWPOINT_SECS);
            self.anim_orbit = Some(OrbitTween {
                from_pivot: self.pivot,
                to_pivot: pose.pivot,
                from_distance: self.distance,
                to_distance: distance,
            });
        } else {
            self.animating = false;
            self.anim_orbit = None;
            self.yaw = yaw;
            self.pitch = pitch;
            self.target_yaw = yaw;
            self.target_pitch = pitch;
            self.pivot = pose.pivot;
            self.distance = distance;
        }
        self.current_view = if self.is_2d() { self.plane_view } else { pose.view };
        self.focus_anchored = false;
    }

    /// What the viewport labels the view: "User Perspective",
    /// "Top Orthographic", or in 2D the plane, "XY Plane".
    pub fn view_label(&self) -> String {
        if self.is_2d() {
            return format!("{} Plane", self.plane_view.plane_label());
        }
        let axis = match self.current_view {
            CameraView::Custom => "User",
            view => view.name(),
        };
        let projection = if self.wants_ortho() { "Orthographic" } else { "Perspective" };
        format!("{axis} {projection}")
    }
}

/// Startup orbit override for scripted / headless captures.
///
/// `EUSTRESS_CAMERA_ORBIT="yaw_deg,pitch_deg,distance[,pivot_x,pivot_y,pivot_z]"`.
/// Pitch follows this controller's convention: **positive looks down**, so a
/// negative pitch aims at the sky.
///
/// This has to seed the controller rather than the camera's `Transform`, because
/// `EustressCamera` re-derives that Transform from `pivot`/`yaw`/`pitch`/
/// `distance` every frame — a pose written at spawn is silently discarded on the
/// first update.
///
/// Read once: it is a startup pose, and re-reading it would fight the user's
/// mouse. Malformed values warn and fall back rather than panicking, so a typo
/// in a capture script cannot stop the editor booting.
fn orbit_override() -> Option<(f32, f32, f32, Vec3)> {
    static V: std::sync::OnceLock<Option<(f32, f32, f32, Vec3)>> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let raw = std::env::var("EUSTRESS_CAMERA_ORBIT").ok()?;
        let parts: Vec<f32> = raw
            .split(',')
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .collect();
        let pivot = match parts.len() {
            3 => Vec3::ZERO,
            6 => Vec3::new(parts[3], parts[4], parts[5]),
            _ => {
                warn!(
                    "EUSTRESS_CAMERA_ORBIT={raw:?} is not 3 or 6 finite comma-separated \
                     numbers (yaw_deg,pitch_deg,distance[,pivot_x,pivot_y,pivot_z]) — ignoring"
                );
                return None;
            }
        };
        // A non-positive distance collapses the orbit onto the pivot, which makes
        // the derived look-at rotation undefined.
        let distance = if parts[2] > 0.01 { parts[2] } else {
            warn!("EUSTRESS_CAMERA_ORBIT distance {} is not positive — using 20", parts[2]);
            20.0
        };
        info!(
            "📷 Editor camera orbit override: yaw {}°, pitch {}°, distance {distance}, pivot {pivot:?}",
            parts[0], parts[1]
        );
        Some((parts[0].to_radians(), parts[1].to_radians(), distance, pivot))
    })
}

impl Default for EustressCamera {
    fn default() -> Self {
        let (yaw, pitch, distance, pivot) = orbit_override().unwrap_or((
            45.0_f32.to_radians(),
            30.0_f32.to_radians(),
            20.0,
            Vec3::ZERO,
        ));
        Self {
            enabled: true,
            initialized: false,
            pivot,
            distance,
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
            dimension: ViewDimension::ThreeD,
            plane_view: CameraView::Front,
            saved_3d: None,
            // Matches `default_scene::studio_camera_bundle`; replaced by the
            // spawned projection's own values on the first frame.
            fov: 70.0_f32.to_radians(),
            near: 0.1,
            far: 10_000.0,
            ortho_blend: 0.0,
            written_perspective: None,
            pan_anchor: None,
            // Animation defaults
            animating: false,
            anim_start_yaw: 0.0,
            anim_start_pitch: 0.0,
            anim_target_yaw: 0.0,
            anim_target_pitch: 0.0,
            anim_progress: 0.0,
            anim_duration: SNAP_SECS,
            anim_orbit: None,
            focus_anchored: false,
        }
    }
}

/// Eustress Camera Plugin: Empowering focus and flow
pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app
            .register_type::<EustressCamera>()
            .init_resource::<ActiveViewPlane>()
            .add_message::<FrameSelectionEvent>()
            .add_message::<GoToCameraEvent>()
            .add_message::<ViewCommand>()
            // Read by `handle_view_actions`. Registered by the UI plugin too;
            // registering here keeps the reader valid in any app that mounts
            // the camera without the Slint chrome.
            .add_message::<crate::ui::MenuActionEvent>()
            .add_plugins(crate::view_grid::ViewGridPlugin)
            .add_systems(Update, (
                ensure_camera_exists,
                update_camera_viewport_for_ui,
                camera_view_input_system
                    .after(crate::ui::slint_ui::update_slint_ui_focus)
                    .run_if(crate::play_mode::editor_input_enabled),
                handle_view_actions,
                apply_view_commands,
                handle_frame_selection.run_if(crate::play_mode::editor_input_enabled),
                reset_focus_distance_on_selection_change,
                handle_go_to_camera.run_if(crate::play_mode::editor_input_enabled),
                animate_view_transition,
                // Orbit / pan / fly / wheel only while the editor camera is on
                // screen: in Play with a character the avatar camera takes
                // over, and WASD, the wheel and right-drag belong to the game.
                eustress_camera_controls.run_if(editor_camera_on_screen),
                advance_projection_blend,
                update_eustress_camera_transform,
                publish_active_view_plane,
                hide_sky_billboards_in_orthographic,
                sync_space_view_file,
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
/// Run condition: a window-targeted editor camera is the active view.
fn editor_camera_on_screen(cams: Query<(&Camera, Option<&bevy::camera::RenderTarget>), With<EustressCamera>>) -> bool {
    cams.iter()
        .any(|(c, target)| c.is_active && !matches!(target, Some(bevy::camera::RenderTarget::Image(_))))
}

fn update_camera_viewport_for_ui(
    // The Play camera is excluded: it renders into the visible viewport rect
    // (see `play_datamodel::camera`), so the player sits in the middle of
    // what the user sees rather than behind the Explorer.
    mut camera_query: Query<
        (&mut Camera, &bevy::camera::RenderTarget),
        (
            With<Camera3d>,
            Without<crate::ui::slint_ui::SlintOverlayCamera>,
            Without<eustress_common::avatar::control::AvatarCamera>,
        ),
    >,
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
// View input: keys the binding table cannot express, and Action routing
// ============================================================================

/// Ctrl+numpad (opposite views) and Num. (frame everything).
///
/// Plain numpad keys and 5 go through the key-binding table as
/// `Action::View*` (see `handle_view_actions`), which makes them rebindable
/// and reachable over the bridge's `invoke_action`. The table matches
/// modifiers exactly, so the Ctrl variants are read here instead.
fn camera_view_input_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut view_commands: MessageWriter<ViewCommand>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
) {
    // Block view shortcuts when a text input has focus or overlay modal is open
    if ui_focus.as_ref().map(|f| f.text_input_focused).unwrap_or(false) { return; }
    if crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) { return; }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if ctrl {
        for (key, view) in [
            (KeyCode::Numpad2, CameraView::Back),
            (KeyCode::Numpad8, CameraView::Bottom),
            (KeyCode::Numpad4, CameraView::Right),
            (KeyCode::Numpad6, CameraView::Left),
        ] {
            if keys.just_pressed(key) {
                view_commands.write(ViewCommand::Snap(view));
            }
        }
    }

    if keys.just_pressed(KeyCode::NumpadDecimal) {
        view_commands.write(ViewCommand::FrameAll);
    }
}

/// Turn the view `Action`s (keyboard, command bar, bridge) into commands.
fn handle_view_actions(
    mut menu_events: MessageReader<crate::ui::MenuActionEvent>,
    mut view_commands: MessageWriter<ViewCommand>,
) {
    use crate::keybindings::Action;
    for event in menu_events.read() {
        let command = match event.action {
            Action::ViewPerspectiveToggle => ViewCommand::ToggleProjection,
            Action::ViewTop => ViewCommand::Snap(CameraView::Top),
            Action::ViewFront => ViewCommand::Snap(CameraView::Front),
            Action::ViewSideLeft => ViewCommand::Snap(CameraView::Left),
            Action::ViewSideRight => ViewCommand::Snap(CameraView::Right),
            Action::ViewMode2D => ViewCommand::SetDimension(ViewDimension::TwoD),
            Action::ViewMode3D => ViewCommand::SetDimension(ViewDimension::ThreeD),
            _ => continue,
        };
        view_commands.write(command);
    }
}

/// Apply every queued `ViewCommand` to the editor camera.
fn apply_view_commands(
    mut incoming: MessageReader<ViewCommand>,
    mut cameras: Query<(&mut EustressCamera, &Camera), With<Camera3d>>,
    mut frame_events: MessageWriter<FrameSelectionEvent>,
    spatial: avian3d::prelude::SpatialQuery,
) {
    let mut pending: Vec<ViewCommand> = incoming.read().copied().collect();
    if let Ok(mut inbox) = VIEW_COMMAND_INBOX.lock() {
        pending.append(&mut inbox);
    }
    if pending.is_empty() {
        return;
    }
    let Ok((mut cam, camera)) = cameras.single_mut() else { return };
    // Play with a character switches the editor camera off and renders from
    // the character's own camera; the top-row 5 then means "tool slot 5", so
    // nothing here may touch a camera nobody is looking through.
    if !camera.is_active {
        return;
    }

    for command in pending {
        match command {
            ViewCommand::SetDimension(ViewDimension::TwoD) => enter_2d(&mut cam, &spatial),
            ViewCommand::SetDimension(ViewDimension::ThreeD) => leave_2d(&mut cam),
            ViewCommand::ToggleDimension => {
                if cam.is_2d() { leave_2d(&mut cam) } else { enter_2d(&mut cam, &spatial) }
            }
            ViewCommand::SetProjection(mode) => set_projection(&mut cam, mode, &spatial),
            ViewCommand::ToggleProjection => {
                let mode = cam.projection_mode.toggled();
                set_projection(&mut cam, mode, &spatial);
            }
            ViewCommand::Snap(view) => {
                if view == CameraView::Custom {
                    continue;
                }
                if cam.is_2d() {
                    cam.plane_view = view;
                }
                start_view_animation(&mut cam, view, SNAP_SECS);
                info!("📷 Camera: {} View", view.name());
            }
            ViewCommand::SetFieldOfView(degrees) => {
                let fov = degrees.clamp(1.0, 170.0).to_radians();
                if cam.wants_ortho() {
                    // Keep the orthographic view exactly where it is; only the
                    // perspective it returns to changes.
                    let height = cam.ortho_height();
                    cam.fov = fov;
                    cam.set_ortho_height(height);
                } else {
                    cam.fov = fov;
                }
            }
            ViewCommand::SetOrthographicSize(height) => {
                if height.is_finite() && height > 0.0 {
                    cam.set_ortho_height(height);
                }
            }
            ViewCommand::FrameAll => {
                frame_events.write(FrameSelectionEvent { target_bounds: None });
            }
        }
    }
}

/// Start an eased turn to `view`'s angles. The exact basis takes over when
/// the turn completes.
fn start_view_animation(cam: &mut EustressCamera, view: CameraView, seconds: f32) {
    let (yaw, pitch) = view.angles();
    animate_angles_to(cam, yaw, pitch, seconds);
    cam.current_view = view;
}

fn animate_angles_to(cam: &mut EustressCamera, yaw: f32, pitch: f32, seconds: f32) {
    // An angle-only move: drop any travel left over from a cancelled one.
    cam.anim_orbit = None;
    cam.animating = true;
    cam.anim_start_yaw = cam.yaw;
    cam.anim_start_pitch = cam.pitch;
    cam.anim_target_yaw = yaw;
    cam.anim_target_pitch = pitch;
    cam.anim_progress = 0.0;
    cam.anim_duration = seconds.max(0.01);
}

/// Re-anchor the pivot on whatever sits under the centre of the screen,
/// keeping the camera where it is.
///
/// Perspective fly-zoom moves the pivot along with the camera, so after
/// flying around it is usually a point in empty air `distance` ahead, not the
/// wall being looked at. Going orthographic preserves the size of what is at
/// the pivot, so without this a wall two metres away would shrink tenfold at
/// the switch. With it, the thing in the middle of the screen keeps its size.
fn refocus_on_center(cam: &mut EustressCamera, spatial: &avian3d::prelude::SpatialQuery) {
    let (forward, position) = cam.frame();
    let Ok(direction) = Dir3::new(forward) else { return };
    let filter = avian3d::prelude::SpatialQueryFilter::default();
    let Some(hit) = spatial.cast_ray(position, direction, 100_000.0, true, &filter) else { return };
    // Zero means the camera sits inside a collider; keep the old pivot then.
    if !(hit.distance.is_finite() && hit.distance > 0.05) {
        return;
    }
    cam.distance = hit.distance.clamp(MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE);
    cam.pivot = position + forward * cam.distance;
}

fn set_projection(
    cam: &mut EustressCamera,
    mode: ProjectionMode,
    spatial: &avian3d::prelude::SpatialQuery,
) {
    if cam.is_2d() {
        info!("📷 2D views are always orthographic; switch to 3D (Alt+3) for perspective");
        return;
    }
    if cam.projection_mode == mode {
        return;
    }
    if mode == ProjectionMode::Orthographic && cam.ortho_blend <= 0.0 {
        refocus_on_center(cam, spatial);
    }
    cam.projection_mode = mode;
    info!("📷 Camera: {} projection", mode.label());
}

/// Enter 2D: turn onto an axis plane and flatten to orthographic.
///
/// The plane is the axis view already being looked along, if any (Top view
/// becomes a top-down 2D view), else the last 2D plane, Front (XY) at first.
fn enter_2d(cam: &mut EustressCamera, spatial: &avian3d::prelude::SpatialQuery) {
    if cam.is_2d() {
        return;
    }
    let plane = cam.exact_view().unwrap_or(cam.plane_view);
    cam.saved_3d = Some(Saved3dView {
        yaw: cam.target_yaw,
        pitch: cam.target_pitch,
        projection: cam.projection_mode,
        view: cam.current_view,
    });
    if cam.ortho_blend <= 0.0 {
        refocus_on_center(cam, spatial);
    }
    cam.dimension = ViewDimension::TwoD;
    cam.plane_view = plane;
    start_view_animation(cam, plane, VIEW_TRANSITION_SECS);
    info!("📷 Camera: 2D, {} plane", plane.plane_label());
}

/// Leave 2D: return to the 3D view and projection held before entering it,
/// centred on wherever the 2D view was panned to.
fn leave_2d(cam: &mut EustressCamera) {
    if !cam.is_2d() {
        return;
    }
    let saved = cam.saved_3d.take().unwrap_or(Saved3dView {
        yaw: 45.0_f32.to_radians(),
        pitch: 30.0_f32.to_radians(),
        projection: ProjectionMode::Perspective,
        view: CameraView::Custom,
    });
    cam.dimension = ViewDimension::ThreeD;
    cam.projection_mode = saved.projection;
    animate_angles_to(cam, saved.yaw, saved.pitch, VIEW_TRANSITION_SECS);
    cam.current_view = saved.view;
    info!("📷 Camera: 3D, {}", saved.projection.label());
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
            // Orthographic reads its view height from this same distance, so
            // one fit serves both projections.
            cam.distance = (extent * 1.5).max(MIN_CAMERA_DISTANCE).min(MAX_CAMERA_DISTANCE);
            // Marks this fit-distance as selection-specific — see the field
            // doc comment. Cleared (resetting `distance`, not `pivot` — no
            // camera jump) the next time the selection changes.
            cam.focus_anchored = true;

            info!("📷 Camera: Framed to bounds (center: {:?}, extent: {:.1})", center, extent);
        }
    }
}

/// Spherical offset from pivot to camera — MUST match
/// `update_eustress_camera_transform`'s position formula. Module-level (not
/// nested in `eustress_camera_controls`) since `reset_focus_distance_on_selection_change`
/// needs it too.
fn orbit_offset(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(
        pitch.cos() * yaw.sin(),
        pitch.sin(),
        pitch.cos() * yaw.cos(),
    )
}

/// Clears a lingering F-focus fit-distance once the SELECTION moves on —
/// see `EustressCamera::focus_anchored`'s doc comment for why this only
/// matters for Alt+Left orbit (right-drag look re-tethers `pivot` every
/// frame regardless, so it never goes stale).
///
/// Resets `distance` back to the default orbit radius WITHOUT moving the
/// camera: back-solve the camera's current world position from
/// `pivot + distance*orbit_offset(yaw, pitch)` (the same relation
/// `update_eustress_camera_transform` renders from), then re-derive `pivot`
/// at the new default `distance` so `camera_pos` — and therefore what's
/// on screen — doesn't jump. Only the invisible orbit radius changes.
///
/// Orthographic is skipped: there `distance` is the zoom, so resetting it
/// would visibly zoom the view every time the selection changed. The anchor
/// stays set and is honoured once the view is back in perspective.
fn reset_focus_distance_on_selection_change(
    mut query: Query<&mut EustressCamera, With<Camera3d>>,
    selection_manager: Option<Res<crate::selection_sync::SelectionSyncManager>>,
    mut last_selection: Local<Option<std::collections::HashSet<String>>>,
) {
    let current: std::collections::HashSet<String> = selection_manager
        .as_ref()
        .map(|sm| sm.0.read().get_selected().into_iter().collect())
        .unwrap_or_default();

    let changed = last_selection.as_ref() != Some(&current);
    *last_selection = Some(current);
    if !changed {
        return;
    }

    for mut cam in query.iter_mut() {
        if !cam.focus_anchored || cam.wants_ortho() || cam.ortho_blend > 0.0 {
            continue;
        }
        cam.focus_anchored = false;

        let default_distance = EustressCamera::default().distance;
        if (cam.distance - default_distance).abs() < f32::EPSILON {
            continue; // Already at default — nothing to reset.
        }

        let offset = orbit_offset(cam.target_yaw, cam.target_pitch);
        let cam_pos = cam.pivot + cam.distance * offset;
        cam.distance = default_distance;
        cam.pivot = cam_pos - cam.distance * offset;
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
            if let Some(orbit) = cam.anim_orbit.take() {
                cam.pivot = orbit.to_pivot;
                cam.distance = orbit.to_distance;
            }
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

            // Travel with the turn. Distance eases geometrically, so going
            // from 10 m to 1 km out feels as even as 10 m to 20 m.
            if let Some(orbit) = cam.anim_orbit {
                cam.pivot = orbit.from_pivot.lerp(orbit.to_pivot, t);
                cam.distance = if orbit.from_distance > 0.0 && orbit.to_distance > 0.0 {
                    orbit.from_distance * (orbit.to_distance / orbit.from_distance).powf(t)
                } else {
                    orbit.from_distance + (orbit.to_distance - orbit.from_distance) * t
                };
            }
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
///
/// The same gestures mean what the projection allows:
///
/// | Input            | 3D perspective      | 3D orthographic     | 2D                 |
/// |------------------|---------------------|---------------------|--------------------|
/// | Right-drag       | look in place       | orbit the pivot     | pan (grab)         |
/// | Middle-drag      | pan                 | pan (grab, 1:1)     | pan (grab, 1:1)    |
/// | Alt+Left-drag    | orbit the pivot     | orbit the pivot     | pan (grab, 1:1)    |
/// | Wheel            | fly along the ray   | zoom about cursor   | zoom about cursor  |
/// | W / S            | fly forward / back  | zoom in / out       | pan up / down      |
/// | A / D            | strafe              | pan                 | pan left / right   |
/// | Q / E            | down / up           | down / up           | zoom out / in      |
///
/// In orthographic, moving along the view axis changes nothing on screen,
/// which is why the forward gestures become zoom there.
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
            // NOTE: the Data dialogs (global sources / domains / global
            // variables / sync domain) are gated by Slint properties, not by
            // these StudioState bools. Nothing ever cleared the bools, so
            // including them here meant one click on a Data menu item killed
            // camera input for the rest of the session. Visibility and input
            // blocking must read the same source of truth.
            || s.show_exit_confirmation
            || s.show_find_dialog
    });
    if modal_open {
        ev_motion.clear();
        ev_wheel.clear();
        ev_touch.clear();
        cam.pan_anchor = None;
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

    // Same neutralization, for the same reason, while a billboard node is
    // being dragged through empty space: `select_tool::handle_drag_distance_wheel`
    // has its own MessageReader<MouseWheel> and reads these same notches to
    // change the drag leash length instead. Without this, one scroll notch
    // would both change the leash AND fly the camera forward/back. Gated on
    // entity type (drag_is_billboard_node), not a modifier key — Shift is
    // reserved for box-select-additive, see SelectToolState's doc comment.
    // Dragging an ordinary Part is untouched surface-snap, which has no
    // "leash" concept, so the wheel should zoom normally there.
    let dragging_billboard_node = select_state
        .as_ref()
        .is_some_and(|s| s.drag_is_billboard_node && s.dragging && s.drag_started);
    if dragging_billboard_node {
        scroll_delta = 0.0;
    }

    // LOCAL SPACE: Get camera's actual local axes for intuitive movement
    let cam_forward = transform.forward();
    let cam_right = transform.right();
    let cam_up = transform.up();

    let is_2d = cam.is_2d();
    // Controls follow where the projection is headed, so a gesture made
    // during the 0.35 s transition already behaves the way the view will.
    let ortho = cam.wants_ortho();
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());

    // Determine which mouse mode is active (mutually exclusive)
    // Shift+Left used to pan too, but that collided with Shift+Left now
    // meaning "camera-relative node drag" in select_tool.rs — Middle-drag
    // is the only remaining 3D pan gesture.
    let right_drag = mouse.pressed(MouseButton::Right) && !ctrl;
    let alt_left = mouse.pressed(MouseButton::Left) && alt && !ctrl;
    let dollying = mouse.pressed(MouseButton::Right) && ctrl;

    // Roblox-Studio-style split:
    //  - Right-drag = LOOK IN PLACE (first-person): the camera position
    //    stays fixed and only the view direction turns. Previously
    //    right-drag ORBITED the pivot, so at large distances the camera
    //    swung on a huge arc — the "ball on a track" feel.
    //  - Alt+Left-drag = classic ORBIT around the pivot (kept for
    //    inspect-an-object workflows).
    // In orthographic there is no "in place" to look from (the camera
    // position only clips), so right-drag orbits the pivot instead, the
    // way Blender does. 2D has no rotation at all: both drags pan.
    let looking = right_drag && !ortho;
    let orbiting = !is_2d && (alt_left || (right_drag && ortho));
    let panning = mouse.pressed(MouseButton::Middle) || (is_2d && (right_drag || alt_left));

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
    } else if panning && ortho {
        // Grab pan: the world point under the cursor stays under it. Both
        // rays come from the same (last frame's) camera, so their origins
        // differ by exactly the in-plane distance the cursor travelled.
        if let Some(now) = cursor {
            if let Some(before) = cam.pan_anchor.filter(|b| *b != now) {
                if let (Ok(a), Ok(b)) = (
                    camera.viewport_to_world(global_transform, before),
                    camera.viewport_to_world(global_transform, now),
                ) {
                    let delta = a.origin - b.origin;
                    let forward = *cam_forward;
                    cam.pivot += delta - forward * delta.dot(forward);
                }
            }
            cam.pan_anchor = Some(now);
        }
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
        // Dolly (Ctrl + Right-drag) - exponential for consistent feel.
        // In orthographic this is a zoom: the view height follows distance.
        let dolly_factor = (1.0 + mouse_delta.y * cam.sensitivity * 0.5).max(0.5).min(2.0);
        cam.distance *= dolly_factor;
        cam.distance = cam.distance.clamp(MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE);
    }
    if !(panning && ortho) {
        cam.pan_anchor = None;
    }

    // Zoom (Mouse Wheel) - only when cursor is inside the 3D viewport and
    // no Slint text input/editor has focus. This lets panels scroll freely
    // and prevents zoom while editing scripts in the center tab.
    if scroll_delta != 0.0 && (!cursor_in_viewport || ui_wants_keyboard) {
        scroll_delta = 0.0;
    }

    // Per-notch travel as a fraction of the current view distance. `1 - 0.9^n`
    // ⇒ scroll-in (+) moves forward, scroll-out (−) moves back, symmetric and
    // smooth across a burst of merged wheel events in one frame.
    const ZOOM_STEP: f32 = 0.9; // ~10% of view distance per line at zoom_speed = 1.0

    if scroll_delta != 0.0 && ortho {
        // Orthographic zoom about the cursor. Flying toward the cursor (the
        // perspective branch below) changes nothing on screen here, so the
        // wheel scales the view instead, and the pivot slides toward the
        // cursor by the matching fraction so the point under it stays put.
        let old_distance = cam.distance;
        let new_distance = (old_distance * ZOOM_STEP.powf(scroll_delta * cam.zoom_speed))
            .clamp(MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE);
        let kept = new_distance / old_distance;
        if let Some(ray) = cursor.and_then(|c| camera.viewport_to_world(global_transform, c).ok()) {
            let offset = ray.origin - global_transform.translation();
            let forward = *cam_forward;
            cam.pivot += (offset - forward * offset.dot(forward)) * (1.0 - kept);
        }
        cam.distance = new_distance;
    } else if scroll_delta != 0.0 {
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
        let travel = cam.distance * (1.0 - ZOOM_STEP.powf(scroll_delta * cam.zoom_speed));

        // Fly along the cursor ray when we have one (keeps whatever is under the
        // cursor roughly pinned on screen — the Blender/Unreal feel); otherwise
        // straight along the view forward. `distance` is deliberately untouched,
        // so orbit (Alt+drag) and pan keep a sensible radius after flying.
        let dir = cursor
            .and_then(|c| camera.viewport_to_world(global_transform, c).ok())
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

    if ortho {
        // Orthographic keys work in view heights per second, so the feel is
        // the same whether the view spans a bolt or a city.
        let height = cam.ortho_height();
        let pan = height * (if shift { 0.25 } else { 1.0 }) * dt;
        let zoom = ((if shift { 0.5 } else { 1.5 }) * dt).exp();
        let right = *cam_right;
        let up = *cam_up;
        let (zoom_in, zoom_out) = if is_2d {
            // 2D: WASD pan the plane, E/Q zoom.
            if keys.pressed(KeyCode::KeyW) { cam.pivot += up * pan; }
            if keys.pressed(KeyCode::KeyS) { cam.pivot -= up * pan; }
            (keys.pressed(KeyCode::KeyE), keys.pressed(KeyCode::KeyQ))
        } else {
            // 3D orthographic: W/S zoom, Q/E/Space pan vertically.
            if keys.pressed(KeyCode::KeyQ) { cam.pivot -= up * pan; }
            if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::Space) { cam.pivot += up * pan; }
            (keys.pressed(KeyCode::KeyW), keys.pressed(KeyCode::KeyS))
        };
        if keys.pressed(KeyCode::KeyA) { cam.pivot -= right * pan; }
        if keys.pressed(KeyCode::KeyD) { cam.pivot += right * pan; }
        if zoom_in { cam.distance = (cam.distance / zoom).max(MIN_CAMERA_DISTANCE); }
        if zoom_out { cam.distance = (cam.distance * zoom).min(MAX_CAMERA_DISTANCE); }
    } else {
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
    }
    // `-` / `=` are intentionally NOT bound to camera vertical here.
    // Those keys belong to `Action::LiftSelection` /
    // `Action::SettleSelection` — lifting the selected PART by one grid unit
    // or settling it onto the surface below (handled in
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
///
/// Looking through a camera is a perspective, free-orientation act, so it
/// leaves 2D and orthographic first.
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

    cam.dimension = ViewDimension::ThreeD;
    cam.saved_3d = None;
    cam.projection_mode = ProjectionMode::Perspective;
    cam.current_view = CameraView::Custom;
    cam.animating = false;
    cam.pivot = pos + fwd * distance;
    cam.distance = distance;
    cam.yaw = yaw;
    cam.pitch = pitch;
    cam.target_yaw = yaw;
    cam.target_pitch = pitch;
    info!("📷 F → editor camera now looking through camera {:?}", ev.target);
}

/// Move `ortho_blend` toward the projection the camera wants, and hold the
/// 2D pivot on its plane once the view has settled.
fn advance_projection_blend(
    time: Res<Time>,
    mut query: Query<&mut EustressCamera, With<Camera3d>>,
) {
    for mut cam in query.iter_mut() {
        let target = if cam.wants_ortho() { 1.0 } else { 0.0 };
        if cam.ortho_blend != target {
            let step = time.delta_secs() / VIEW_TRANSITION_SECS;
            cam.ortho_blend = if target > cam.ortho_blend {
                (cam.ortho_blend + step).min(1.0)
            } else {
                (cam.ortho_blend - step).max(0.0)
            };
        }

        // 2D works on the axis plane through the world origin (z = 0 for XY),
        // where Bevy's 2D content lives and where Insert drops new parts.
        // Sliding the pivot along the view axis moves nothing on screen in
        // orthographic, so this waits until the transition has finished.
        if cam.is_2d() && cam.ortho_blend >= 1.0 && !cam.animating {
            if let Some((forward, _)) = cam.plane_view.basis() {
                let off_plane = cam.pivot.dot(forward);
                if off_plane.abs() > 1e-5 {
                    cam.pivot -= forward * off_plane;
                }
            }
        }
    }
}

/// Perspective parameters partway through the dolly zoom between projections:
/// `(pull_back, tan_half_fov)`.
///
/// The field of view eases from the camera's own down to `DOLLY_MIN_FOV`
/// while the camera backs away by `pull_back`, chosen so the plane at
/// `distance` (the pivot) keeps exactly its size on screen:
///
///     (distance + pull_back) * tan_half = distance * tan(fov / 2)
fn dolly_for_blend(blend: f32, fov: f32, distance: f32) -> (f32, f32) {
    let t = blend.clamp(0.0, 1.0);
    let eased = t * t * (3.0 - 2.0 * t);
    let tan_start = (fov * 0.5).tan();
    let tan_end = (DOLLY_MIN_FOV * 0.5).tan();
    let tan_half = tan_start + (tan_end - tan_start) * eased;
    let pull_back = (distance * (tan_start / tan_half - 1.0)).max(0.0);
    (pull_back, tan_half)
}

/// Transform Update - INSTANT, RAW response with NO smoothing
///
/// Also the one writer of the editor camera's `Projection` (see the module
/// header): settled perspective, the dolly zoom between projections, or
/// orthographic at `ortho_height`.
fn update_eustress_camera_transform(
    mut query: Query<(&mut EustressCamera, &mut Transform, &mut Projection), With<Camera3d>>,
) {
    for (mut cam, mut trans, mut projection) in query.iter_mut() {
        // A perspective component this controller did not write came from
        // somewhere else: the spawned bundle on the first frame, or a
        // FieldOfView / clip-plane edit in the Properties panel. Adopt it.
        if let Projection::Perspective(p) = projection.as_ref() {
            let current = (p.fov, p.near, p.far);
            if cam.ortho_blend <= 0.0 && cam.written_perspective != Some(current) {
                if p.fov.is_finite() && p.fov > 0.0 {
                    cam.fov = p.fov;
                }
                if p.near.is_finite() && p.near > 0.0 {
                    cam.near = p.near;
                }
                if p.far.is_finite() && p.far > p.near {
                    cam.far = p.far;
                }
                cam.written_perspective = Some(current);
            }
        }

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

        let blend = cam.ortho_blend;
        let (pull_back, tan_half) = if blend > 0.0 && blend < 1.0 {
            dolly_for_blend(blend, cam.fov, distance)
        } else {
            (0.0, (cam.fov * 0.5).tan())
        };

        // Axis views render from their exact basis; everything else from the
        // orbit angles (pivot-based spherical coordinates).
        match cam.exact_view().and_then(|v| v.basis()) {
            Some((forward, up)) => {
                trans.translation = pivot - forward * (distance + pull_back);
                trans.look_to(forward, up);
            }
            None => {
                trans.translation = pivot + orbit_offset(yaw, pitch) * (distance + pull_back);
                trans.look_at(pivot, Vec3::Y);
            }
        }

        let desired = if blend >= 1.0 {
            // 3D keeps perspective's rule that nothing behind the camera
            // draws. 2D draws a slab centred on its plane instead, so layers
            // at any depth stay visible however far the view is zoomed out.
            let (near, far) = if cam.is_2d() {
                (distance - PLANE_2D_DEPTH, distance + PLANE_2D_DEPTH)
            } else {
                (0.0, distance + ORTHO_3D_DEPTH)
            };
            Projection::Orthographic(OrthographicProjection {
                near,
                far,
                scaling_mode: ScalingMode::FixedVertical { viewport_height: cam.ortho_height() },
                scale: 1.0,
                ..OrthographicProjection::default_3d()
            })
        } else {
            // During the dolly the near plane rides at the rest position, so
            // what the pulled-back camera sees is exactly what the resting
            // camera would.
            Projection::Perspective(PerspectiveProjection {
                fov: 2.0 * tan_half.atan(),
                near: cam.near + pull_back,
                far: cam.far + pull_back,
                ..default()
            })
        };
        if projection_differs(&projection, &desired) {
            if let Projection::Perspective(p) = &desired {
                cam.written_perspective = Some((p.fov, p.near, p.far));
            }
            *projection = desired;
        }
    }
}

/// Whether writing `desired` would change anything the renderer uses.
/// Ignores the fields Bevy recomputes itself (`aspect_ratio`, `area`), so a
/// settled view does not dirty `Projection` every frame.
fn projection_differs(current: &Projection, desired: &Projection) -> bool {
    match (current, desired) {
        (Projection::Perspective(a), Projection::Perspective(b)) => {
            a.fov != b.fov || a.near != b.near || a.far != b.far
        }
        (Projection::Orthographic(a), Projection::Orthographic(b)) => {
            let height = |o: &OrthographicProjection| match o.scaling_mode {
                ScalingMode::FixedVertical { viewport_height } => Some(viewport_height),
                _ => None,
            };
            a.near != b.near || a.far != b.far || a.scale != b.scale || height(a) != height(b)
        }
        _ => true,
    }
}

/// Keep `ActiveViewPlane` in step with the camera.
fn publish_active_view_plane(
    cameras: Query<&EustressCamera, With<Camera3d>>,
    mut plane: ResMut<ActiveViewPlane>,
) {
    let next = cameras.iter().next().and_then(|cam| {
        if !cam.is_2d() {
            return None;
        }
        let (forward, _) = cam.plane_view.basis()?;
        Some(ViewPlane { normal: -forward, center: cam.pivot })
    });
    if plane.0 != next {
        plane.0 = next;
    }
}

/// Hide the moon and star billboards whenever the view is not plain
/// perspective.
///
/// They sit a few kilometres out along their sky direction and are sized for
/// a perspective camera. Orthographic has no "far away" (the moon would draw
/// as a 400 m disc wherever the view points at it), and the narrow field of
/// view in the middle of the dolly zoom would magnify them.
fn hide_sky_billboards_in_orthographic(
    cameras: Query<&EustressCamera, With<Camera3d>>,
    mut sky: Query<
        &mut Visibility,
        Or<(With<crate::shaders::StarFieldRoot>, With<crate::shaders::MoonDiscMarker>)>,
    >,
) {
    let flat = cameras.iter().next().is_some_and(|cam| cam.ortho_blend > 0.0);
    let want = if flat { Visibility::Hidden } else { Visibility::Inherited };
    for mut visibility in sky.iter_mut() {
        if *visibility != want {
            *visibility = want;
        }
    }
}

// ============================================================================
// Per-Space memory of the Perspective
// ============================================================================

/// `<space>/.eustress/view.toml`: the 2D/3D mode, projection and 2D plane a
/// Space was last viewed with, so a 2D project reopens in 2D. Written only
/// when those change (never the camera pose, which changes every frame), and
/// not at all for a Space that has only ever used the default 3D perspective.
#[derive(serde::Serialize, serde::Deserialize)]
struct SpaceViewFile {
    mode: String,
    projection: String,
    plane: String,
}

type SpaceViewState = (ViewDimension, ProjectionMode, CameraView);

const DEFAULT_SPACE_VIEW: SpaceViewState =
    (ViewDimension::ThreeD, ProjectionMode::Perspective, CameraView::Front);

fn space_view_path(space_root: &std::path::Path) -> std::path::PathBuf {
    space_root.join(".eustress").join("view.toml")
}

fn read_space_view(space_root: &std::path::Path) -> Option<SpaceViewState> {
    let text = std::fs::read_to_string(space_view_path(space_root)).ok()?;
    let file: SpaceViewFile = toml::from_str(&text).ok()?;
    let mode = if file.mode.eq_ignore_ascii_case("2d") { ViewDimension::TwoD } else { ViewDimension::ThreeD };
    let projection = if file.projection.eq_ignore_ascii_case("orthographic") {
        ProjectionMode::Orthographic
    } else {
        ProjectionMode::Perspective
    };
    let plane = CameraView::from_name(&file.plane)
        .filter(|v| *v != CameraView::Custom)
        .unwrap_or(CameraView::Front);
    Some((mode, projection, plane))
}

fn write_space_view(space_root: &std::path::Path, state: SpaceViewState) -> std::io::Result<()> {
    let (mode, projection, plane) = state;
    let file = SpaceViewFile {
        mode: mode.label().to_string(),
        projection: projection.label().to_string(),
        plane: plane.name().to_string(),
    };
    let body = toml::to_string_pretty(&file).map_err(|e| std::io::Error::other(e.to_string()))?;
    let path = space_view_path(space_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        path,
        format!("# The Perspective this Space opens in. Written by the editor.\n{body}"),
    )
}

/// Restore a saved Perspective at once (no transition): the Space is only
/// just opening, so there is nothing on screen to animate from.
fn apply_space_view(cam: &mut EustressCamera, state: SpaceViewState) {
    let (mode, projection, plane) = state;
    cam.projection_mode = projection;
    cam.plane_view = plane;
    cam.dimension = mode;
    if mode == ViewDimension::TwoD {
        let (yaw, pitch) = plane.angles();
        cam.yaw = yaw;
        cam.pitch = pitch;
        cam.target_yaw = yaw;
        cam.target_pitch = pitch;
        cam.current_view = plane;
        cam.animating = false;
        cam.saved_3d = None;
    }
    cam.ortho_blend = if cam.wants_ortho() { 1.0 } else { 0.0 };
}

fn sync_space_view_file(
    space_root: Option<Res<crate::space::SpaceRoot>>,
    mut cameras: Query<&mut EustressCamera, With<Camera3d>>,
    mut applied_for: Local<Option<std::path::PathBuf>>,
    mut last_saved: Local<Option<SpaceViewState>>,
) {
    let Some(space_root) = space_root else { return };
    let root = &space_root.0;
    if !crate::space::looks_like_space_root(root) {
        return;
    }
    let Ok(mut cam) = cameras.single_mut() else { return };

    if applied_for.as_ref() != Some(root) {
        *applied_for = Some(root.clone());
        if let Some(state) = read_space_view(root) {
            apply_space_view(&mut cam, state);
            info!("📷 Restored {} {} view for this Space", state.0.label(), state.1.label());
        }
        *last_saved = Some((cam.dimension, cam.projection_mode, cam.plane_view));
        return;
    }

    let now = (cam.dimension, cam.projection_mode, cam.plane_view);
    if *last_saved == Some(now) {
        return;
    }
    *last_saved = Some(now);
    if now == DEFAULT_SPACE_VIEW && !space_view_path(root).exists() {
        return;
    }
    if let Err(e) = write_space_view(root, now) {
        warn!("📷 Could not save the Space's view mode: {e}");
    }
}

/// Field of view for screen-constant gizmo sizing.
///
/// Every gizmo in the editor sizes itself as `distance * tan(fov / 2) * k`:
/// the world height of the view at that distance, times a screen fraction.
/// Orthographic has no field of view and its view height does not depend on
/// distance, so this returns the angle that makes the same expression give
/// the orthographic half height at `target`. Callers keep their formula and
/// handles stay the same size on screen in both projections.
pub fn gizmo_fov(projection: &Projection, camera_position: Vec3, target: Vec3) -> f32 {
    match projection {
        Projection::Perspective(p) => p.fov,
        Projection::Orthographic(o) => {
            let half_height = match o.scaling_mode {
                ScalingMode::FixedVertical { viewport_height } => viewport_height * o.scale * 0.5,
                _ => (o.area.max.y - o.area.min.y).abs() * 0.5,
            };
            let distance = (target - camera_position).length().max(0.1);
            2.0 * (half_height / distance).atan()
        }
        _ => std::f32::consts::FRAC_PI_4,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_commands_parse_from_ui_strings() {
        assert_eq!(ViewCommand::parse("2D"), Some(ViewCommand::SetDimension(ViewDimension::TwoD)));
        assert_eq!(ViewCommand::parse(" 3d "), Some(ViewCommand::SetDimension(ViewDimension::ThreeD)));
        assert_eq!(ViewCommand::parse("orthographic"), Some(ViewCommand::SetProjection(ProjectionMode::Orthographic)));
        assert_eq!(ViewCommand::parse("toggle-projection"), Some(ViewCommand::ToggleProjection));
        assert_eq!(ViewCommand::parse("Top"), Some(ViewCommand::Snap(CameraView::Top)));
        assert_eq!(ViewCommand::parse("fov:65"), Some(ViewCommand::SetFieldOfView(65.0)));
        assert_eq!(ViewCommand::parse("ortho-size:12.5"), Some(ViewCommand::SetOrthographicSize(12.5)));
        assert_eq!(ViewCommand::parse("frame-all"), Some(ViewCommand::FrameAll));
        assert_eq!(ViewCommand::parse("custom"), None);
        assert_eq!(ViewCommand::parse("fov:nan"), None);
        assert_eq!(ViewCommand::parse("sideways"), None);
    }

    #[test]
    fn ortho_height_matches_the_pivot_plane_and_round_trips() {
        let mut cam = EustressCamera::default();
        cam.distance = 20.0;
        let height = cam.ortho_height();
        // The pivot plane's height in perspective, the other way round.
        let expected = 2.0 * 20.0 * (cam.fov * 0.5).tan();
        assert!((height - expected).abs() < 1e-4);
        cam.set_ortho_height(5.0);
        assert!((cam.ortho_height() - 5.0).abs() < 1e-4);
    }

    #[test]
    fn dolly_zoom_holds_the_pivot_plane_size() {
        let fov = 70.0_f32.to_radians();
        let distance = 20.0;
        let pivot_height = distance * (fov * 0.5).tan();
        for step in 0..=10 {
            let blend = step as f32 / 10.0;
            let (pull_back, tan_half) = dolly_for_blend(blend, fov, distance);
            let seen = (distance + pull_back) * tan_half;
            assert!((seen - pivot_height).abs() < 1e-3, "blend {blend}: {seen} vs {pivot_height}");
        }
        assert_eq!(dolly_for_blend(0.0, fov, distance).0, 0.0);
    }

    #[test]
    fn go_to_restores_a_pose_exactly_when_not_animated() {
        let mut cam = EustressCamera::default();
        let pose = ViewPose {
            pivot: Vec3::new(3.0, 1.0, -2.0),
            distance: 42.0,
            yaw: 0.7,
            pitch: 0.4,
            dimension: ViewDimension::ThreeD,
            projection: ProjectionMode::Orthographic,
            view: CameraView::Custom,
            plane: CameraView::Front,
        };
        cam.go_to(pose, false);
        assert_eq!(cam.pose(), pose);
        assert!(cam.wants_ortho());
    }

    #[test]
    fn go_to_a_2d_pose_enters_2d_facing_its_plane() {
        let mut cam = EustressCamera::default();
        let pose = ViewPose {
            pivot: Vec3::new(5.0, 0.0, 0.0),
            distance: 12.0,
            // Stray angles: 2D must face its plane anyway.
            yaw: 1.0,
            pitch: 0.2,
            dimension: ViewDimension::TwoD,
            projection: ProjectionMode::Perspective,
            view: CameraView::Custom,
            plane: CameraView::Top,
        };
        cam.go_to(pose, true);
        assert!(cam.animating && cam.is_2d() && cam.saved_3d.is_some());
        let dest = cam.pose();
        assert_eq!((dest.pivot, dest.distance), (pose.pivot, 12.0));
        assert_eq!((dest.yaw, dest.pitch), CameraView::Top.angles());
        assert_eq!(cam.current_view, CameraView::Top);
    }

    #[test]
    fn gizmo_fov_gives_the_orthographic_half_height_at_any_distance() {
        let projection = Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical { viewport_height: 30.0 },
            ..OrthographicProjection::default_3d()
        });
        for d in [1.0_f32, 10.0, 250.0] {
            let fov = gizmo_fov(&projection, Vec3::ZERO, Vec3::new(0.0, 0.0, -d));
            assert!((d * (fov * 0.5).tan() - 15.0).abs() < 1e-3);
        }
    }

    #[test]
    fn axis_bases_agree_with_their_orbit_angles() {
        for view in [
            CameraView::Front, CameraView::Back, CameraView::Left,
            CameraView::Right, CameraView::Top, CameraView::Bottom,
        ] {
            let (yaw, pitch) = view.angles();
            let (forward, up) = view.basis().unwrap();
            assert!((-orbit_offset(yaw, pitch)).dot(forward) > 0.999, "{view:?} forward");
            // `look_at(pivot, Y)` from the same angles yields this up.
            let from_angles = Transform::from_translation(orbit_offset(yaw, pitch))
                .looking_at(Vec3::ZERO, Vec3::Y);
            assert!(from_angles.up().dot(up) > 0.99, "{view:?} up");
        }
    }
}

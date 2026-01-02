// ============================================================================
// Window Focus & Idle Power Management
// ============================================================================
//
// Reduces CPU/GPU usage when:
// 1. Window is unfocused (in background)
// 2. User is idle (no input for extended period)
// 3. Window is minimized
//
// ## Power Modes
// - **Active**: Full 60+ FPS, all systems running
// - **Background**: 4 FPS (250ms), reduced rendering
// - **Idle**: 2 FPS (500ms), minimal updates
// - **Minimized**: 1 FPS (1000ms), near-zero GPU usage
//
// ## Table of Contents
// 1. PowerMode - Current power state enum
// 2. WindowFocusState - Focus and idle tracking resource
// 3. IdleSettings - Configurable idle thresholds
// 4. WindowFocusPlugin - Plugin registration
// 5. Systems - Focus handling, idle detection, power management

use bevy::prelude::*;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::window::{WindowFocused, WindowMode};
use bevy::winit::{UpdateMode, WinitSettings};
use std::time::{Duration, Instant};

// ============================================================================
// 1. PowerMode - Current Power State
// ============================================================================

/// Current power mode for the engine
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
pub enum PowerMode {
    /// Full speed - window focused and user active
    #[default]
    Active,
    /// Reduced speed - window unfocused but visible
    Background,
    /// Minimal updates - user idle for extended period
    Idle,
    /// Near-zero updates - window minimized
    Minimized,
}

impl PowerMode {
    /// Get the update interval for this power mode
    pub fn update_interval(&self) -> Duration {
        match self {
            PowerMode::Active => Duration::ZERO, // Continuous
            PowerMode::Background => Duration::from_millis(250), // 4 FPS
            PowerMode::Idle => Duration::from_millis(500), // 2 FPS
            PowerMode::Minimized => Duration::from_millis(1000), // 1 FPS
        }
    }
    
    /// Get display name for UI/logging
    pub fn display_name(&self) -> &'static str {
        match self {
            PowerMode::Active => "Active",
            PowerMode::Background => "Background",
            PowerMode::Idle => "Idle",
            PowerMode::Minimized => "Minimized",
        }
    }
}

// ============================================================================
// 2. WindowFocusState - Focus and Idle Tracking
// ============================================================================

/// Resource tracking window focus state and idle detection
#[derive(Resource, Reflect)]
#[reflect(Resource)]
pub struct WindowFocusState {
    /// Whether the window currently has focus
    pub focused: bool,
    /// Whether the window is minimized
    pub minimized: bool,
    /// Current power mode
    pub power_mode: PowerMode,
    /// Last time user provided input (mouse move, key press, etc.)
    pub last_input_time: Option<Instant>,
    /// Time when window lost focus
    pub unfocused_since: Option<Instant>,
    /// Whether idle mode is enabled
    pub idle_detection_enabled: bool,
}

impl Default for WindowFocusState {
    fn default() -> Self {
        Self {
            focused: true,
            minimized: false,
            power_mode: PowerMode::Active,
            last_input_time: Some(Instant::now()),
            unfocused_since: None,
            idle_detection_enabled: true,
        }
    }
}

impl WindowFocusState {
    /// Check if user has been idle for the given duration
    pub fn is_idle_for(&self, duration: Duration) -> bool {
        self.last_input_time
            .map(|t| t.elapsed() >= duration)
            .unwrap_or(true)
    }
    
    /// Check if window has been unfocused for the given duration
    pub fn is_unfocused_for(&self, duration: Duration) -> bool {
        self.unfocused_since
            .map(|t| t.elapsed() >= duration)
            .unwrap_or(false)
    }
    
    /// Reset idle timer (called on user input)
    pub fn reset_idle(&mut self) {
        self.last_input_time = Some(Instant::now());
    }
}

// ============================================================================
// 3. IdleSettings - Configurable Thresholds
// ============================================================================

/// Configurable settings for idle and power management
#[derive(Resource, Reflect, Clone)]
#[reflect(Resource)]
pub struct IdleSettings {
    /// Time before entering idle mode when focused (seconds)
    pub idle_threshold_focused: f32,
    /// Time before entering deep idle when unfocused (seconds)
    pub idle_threshold_unfocused: f32,
    /// Whether to reduce frame rate when unfocused
    pub throttle_unfocused: bool,
    /// Whether to reduce frame rate when idle
    pub throttle_idle: bool,
    /// Whether to pause physics when minimized
    pub pause_physics_minimized: bool,
    /// Whether to pause animations when minimized
    pub pause_animations_minimized: bool,
    /// Background update rate in milliseconds
    pub background_update_ms: u64,
    /// Idle update rate in milliseconds
    pub idle_update_ms: u64,
    /// Minimized update rate in milliseconds
    pub minimized_update_ms: u64,
}

impl Default for IdleSettings {
    fn default() -> Self {
        Self {
            idle_threshold_focused: 60.0,      // 1 minute of no input while focused
            idle_threshold_unfocused: 10.0,    // 10 seconds after losing focus
            throttle_unfocused: true,
            throttle_idle: true,
            pause_physics_minimized: true,
            pause_animations_minimized: true,
            background_update_ms: 250,         // 4 FPS when unfocused
            idle_update_ms: 500,               // 2 FPS when idle
            minimized_update_ms: 1000,         // 1 FPS when minimized
        }
    }
}

// ============================================================================
// 4. WindowFocusPlugin - Plugin Registration
// ============================================================================

/// Plugin for window focus and idle power management
pub struct WindowFocusPlugin;

impl Plugin for WindowFocusPlugin {
    fn build(&self, app: &mut App) {
        app
            // Register types for reflection
            .register_type::<PowerMode>()
            .register_type::<WindowFocusState>()
            .register_type::<IdleSettings>()
            // Initialize resources
            .init_resource::<WindowFocusState>()
            .init_resource::<IdleSettings>()
            // Set initial WinitSettings - start in active mode
            .insert_resource(WinitSettings {
                focused_mode: UpdateMode::Continuous,
                unfocused_mode: UpdateMode::reactive_low_power(Duration::from_millis(250)),
            })
            // Systems
            .add_systems(Update, (
                handle_window_focus,
                detect_user_input,
                update_power_mode,
                apply_power_settings,
            ).chain());
    }
}

// ============================================================================
// 5. Systems - Focus, Idle Detection, Power Management
// ============================================================================

/// Handle window focus change events
fn handle_window_focus(
    mut focus_events: MessageReader<WindowFocused>,
    mut focus_state: ResMut<WindowFocusState>,
    windows: Query<&Window>,
) {
    for event in focus_events.read() {
        let was_focused = focus_state.focused;
        focus_state.focused = event.focused;
        
        if event.focused {
            // Window gained focus
            focus_state.unfocused_since = None;
            focus_state.reset_idle();
            info!("🔆 Window focused - switching to Active mode");
        } else if was_focused {
            // Window just lost focus
            focus_state.unfocused_since = Some(Instant::now());
            info!("🔅 Window unfocused - switching to Background mode");
        }
    }
    
    // Check if window is minimized
    for window in windows.iter() {
        let is_minimized = window.window_level == bevy::window::WindowLevel::Normal 
            && window.mode == WindowMode::Windowed
            && window.resolution.width() == 0.0;
        
        if focus_state.minimized != is_minimized {
            focus_state.minimized = is_minimized;
            if is_minimized {
                info!("📦 Window minimized - switching to Minimized mode");
            }
        }
    }
}

/// Detect user input to reset idle timer
fn detect_user_input(
    mut focus_state: ResMut<WindowFocusState>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_wheel: Res<AccumulatedMouseScroll>,
) {
    // Check for any input activity
    let has_input = 
        mouse_motion.delta != Vec2::ZERO ||
        mouse_button.get_just_pressed().count() > 0 ||
        keyboard.get_just_pressed().count() > 0 ||
        mouse_wheel.delta != Vec2::ZERO;
    
    if has_input {
        focus_state.reset_idle();
    }
}

/// Update power mode based on focus and idle state
fn update_power_mode(
    mut focus_state: ResMut<WindowFocusState>,
    settings: Res<IdleSettings>,
) {
    let old_mode = focus_state.power_mode;
    
    // Determine new power mode
    let new_mode = if focus_state.minimized {
        PowerMode::Minimized
    } else if !focus_state.focused {
        // Window is unfocused
        if settings.throttle_idle && focus_state.is_unfocused_for(Duration::from_secs_f32(settings.idle_threshold_unfocused)) {
            PowerMode::Idle
        } else if settings.throttle_unfocused {
            PowerMode::Background
        } else {
            PowerMode::Active
        }
    } else {
        // Window is focused
        if settings.throttle_idle 
            && focus_state.idle_detection_enabled 
            && focus_state.is_idle_for(Duration::from_secs_f32(settings.idle_threshold_focused)) 
        {
            PowerMode::Idle
        } else {
            PowerMode::Active
        }
    };
    
    // Log mode changes
    if new_mode != old_mode {
        info!("⚡ Power mode: {} → {}", old_mode.display_name(), new_mode.display_name());
        focus_state.power_mode = new_mode;
    }
}

/// Apply power settings based on current mode
fn apply_power_settings(
    focus_state: Res<WindowFocusState>,
    settings: Res<IdleSettings>,
    mut winit_settings: ResMut<WinitSettings>,
) {
    // Only update if changed
    if !focus_state.is_changed() {
        return;
    }
    
    match focus_state.power_mode {
        PowerMode::Active => {
            winit_settings.focused_mode = UpdateMode::Continuous;
            winit_settings.unfocused_mode = UpdateMode::reactive_low_power(
                Duration::from_millis(settings.background_update_ms)
            );
        }
        PowerMode::Background => {
            winit_settings.focused_mode = UpdateMode::Continuous;
            winit_settings.unfocused_mode = UpdateMode::reactive_low_power(
                Duration::from_millis(settings.background_update_ms)
            );
        }
        PowerMode::Idle => {
            // Even when focused but idle, reduce update rate
            winit_settings.focused_mode = UpdateMode::reactive_low_power(
                Duration::from_millis(settings.idle_update_ms)
            );
            winit_settings.unfocused_mode = UpdateMode::reactive_low_power(
                Duration::from_millis(settings.idle_update_ms)
            );
        }
        PowerMode::Minimized => {
            // Minimal updates when minimized
            winit_settings.focused_mode = UpdateMode::reactive_low_power(
                Duration::from_millis(settings.minimized_update_ms)
            );
            winit_settings.unfocused_mode = UpdateMode::reactive_low_power(
                Duration::from_millis(settings.minimized_update_ms)
            );
        }
    }
}

// ============================================================================
// Run Conditions for Other Systems
// ============================================================================

/// Run condition: only run when window is focused
pub fn when_focused(focus_state: Res<WindowFocusState>) -> bool {
    focus_state.focused
}

/// Run condition: only run when in active power mode
pub fn when_active(focus_state: Res<WindowFocusState>) -> bool {
    focus_state.power_mode == PowerMode::Active
}

/// Run condition: only run when NOT minimized
pub fn when_not_minimized(focus_state: Res<WindowFocusState>) -> bool {
    !focus_state.minimized
}

/// Run condition: only run when NOT idle
pub fn when_not_idle(focus_state: Res<WindowFocusState>) -> bool {
    focus_state.power_mode != PowerMode::Idle && focus_state.power_mode != PowerMode::Minimized
}

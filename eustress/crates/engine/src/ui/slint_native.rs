// ============================================================================
// Eustress Engine Studio - Slint Native Window Integration
// Runs Slint in its own native window with Bevy as a borderless child window
// ============================================================================
//
// Architecture (Qt-style embedding):
// 1. Slint runs with its native backend (GPU-accelerated) as the main UI window
// 2. Bevy runs as a borderless window positioned inside Slint's viewport area
// 3. Window positions are synchronized when Slint reports viewport changes
//
// ============================================================================

#![allow(dead_code)]
#![allow(unused_variables)]

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowPosition, WindowResolution, WindowLevel};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::Duration;

// Include Slint modules directly - this creates StudioWindow type
slint::include_modules!();

/// Shared state between Slint thread and Bevy main thread
#[derive(Clone)]
pub struct SharedBridgeState {
    /// Viewport bounds (x, y, width, height) relative to Slint window
    pub viewport_bounds: Arc<Mutex<(f32, f32, f32, f32)>>,
    /// Slint window screen position
    pub slint_window_pos: Arc<Mutex<(i32, i32)>>,
    /// Flag: viewport bounds changed, Bevy needs to reposition
    pub needs_reposition: Arc<AtomicBool>,
    /// Flag: application should close
    pub should_close: Arc<AtomicBool>,
    /// Flag: Slint window is ready
    pub slint_ready: Arc<AtomicBool>,
}

impl Default for SharedBridgeState {
    fn default() -> Self {
        Self {
            viewport_bounds: Arc::new(Mutex::new((280.0, 100.0, 1040.0, 650.0))),
            slint_window_pos: Arc::new(Mutex::new((100, 100))),
            needs_reposition: Arc::new(AtomicBool::new(true)),
            should_close: Arc::new(AtomicBool::new(false)),
            slint_ready: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Bevy resource wrapping the shared state
#[derive(Resource)]
pub struct SlintBevyBridge(pub SharedBridgeState);

/// Marker for tracking if we've done initial positioning
#[derive(Resource, Default)]
pub struct BevyWindowPositioned(pub bool);

/// Start Slint in a separate thread and return the shared bridge state
pub fn start_slint_ui_thread() -> SharedBridgeState {
    let state = SharedBridgeState::default();
    let state_clone = state.clone();
    
    thread::spawn(move || {
        run_slint_native_window(state_clone);
    });
    
    // Wait for Slint to be ready (up to 5 seconds)
    for _ in 0..50 {
        if state.slint_ready.load(Ordering::SeqCst) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    
    state
}

/// Run the Slint native window (called in separate thread)
fn run_slint_native_window(state: SharedBridgeState) {
    // Create Slint UI with native backend
    let ui = match StudioWindow::new() {
        Ok(ui) => ui,
        Err(e) => {
            eprintln!("Failed to create Slint window: {}", e);
            return;
        }
    };
    
    // Set up viewport bounds callback
    let vb = state.viewport_bounds.clone();
    let nr = state.needs_reposition.clone();
    ui.on_viewport_bounds_changed(move |x, y, w, h| {
        if let Ok(mut bounds) = vb.lock() {
            *bounds = (x, y, w, h);
        }
        nr.store(true, Ordering::SeqCst);
    });
    
    // Set up close callback
    let sc = state.should_close.clone();
    ui.on_close_requested(move || {
        sc.store(true, Ordering::SeqCst);
    });
    
    // Set initial UI state
    ui.set_dark_theme(true);
    ui.set_show_explorer(true);
    ui.set_show_properties(true);
    ui.set_show_output(true);
    ui.set_show_toolbox(true);
    
    // Signal that Slint is ready
    state.slint_ready.store(true, Ordering::SeqCst);
    
    // Run the Slint event loop (blocks until window closes)
    if let Err(e) = ui.run() {
        eprintln!("Slint event loop error: {}", e);
    }
    
    // Signal close when Slint exits
    state.should_close.store(true, Ordering::SeqCst);
}

/// Plugin for Slint native window + Bevy child window architecture
pub struct SlintNativePlugin;

impl Plugin for SlintNativePlugin {
    fn build(&self, app: &mut App) {
        // Start Slint in separate thread
        let bridge_state = start_slint_ui_thread();
        
        app
            .insert_resource(SlintBevyBridge(bridge_state))
            .insert_resource(BevyWindowPositioned::default())
            .add_systems(Update, sync_bevy_window_position)
            .add_systems(Update, check_slint_close_request);
        
        info!("SlintNativePlugin: Started Slint in separate thread");
    }
}

/// System to sync Bevy window position with Slint viewport
fn sync_bevy_window_position(
    bridge: Res<SlintBevyBridge>,
    mut positioned: ResMut<BevyWindowPositioned>,
    mut windows: Query<&mut bevy::window::Window, With<PrimaryWindow>>,
) {
    let state = &bridge.0;
    
    // Check if we need to reposition
    if !state.needs_reposition.swap(false, Ordering::SeqCst) && positioned.0 {
        return;
    }
    
    // Get viewport bounds
    let (vx, vy, vw, vh) = state.viewport_bounds.lock()
        .map(|b| *b)
        .unwrap_or((280.0, 100.0, 1040.0, 650.0));
    
    // Get Slint window position
    let (wx, wy) = state.slint_window_pos.lock()
        .map(|p| *p)
        .unwrap_or((100, 100));
    
    // Calculate absolute screen position
    let abs_x = wx + vx as i32;
    let abs_y = wy + vy as i32;
    
    // Update Bevy window
    if let Some(mut window) = windows.iter_mut().next() {
        window.position = WindowPosition::At(IVec2::new(abs_x, abs_y));
        window.resolution.set(vw, vh);
        window.decorations = false; // Borderless
        window.window_level = WindowLevel::Normal;
        
        if !positioned.0 {
            info!("📐 Positioned Bevy window at ({}, {}) size {}x{}", abs_x, abs_y, vw, vh);
            positioned.0 = true;
        }
    }
}

/// System to check if Slint requested close
fn check_slint_close_request(
    bridge: Res<SlintBevyBridge>,
    mut exit: bevy::ecs::message::MessageWriter<bevy::app::AppExit>,
) {
    if bridge.0.should_close.load(Ordering::SeqCst) {
        info!("Slint requested close - exiting Bevy");
        exit.write(bevy::app::AppExit::Success);
    }
}

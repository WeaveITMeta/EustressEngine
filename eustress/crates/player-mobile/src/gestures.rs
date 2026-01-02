// =============================================================================
// Eustress Player Mobile - Gesture Recognition
// =============================================================================
// Table of Contents:
// 1. Plugin Definition
// 2. Gesture Types
// 3. Gesture Detection
// =============================================================================

use bevy::prelude::*;
use bevy::input::touch::{TouchInput, TouchPhase};
use std::collections::HashMap;

// -----------------------------------------------------------------------------
// 1. Plugin Definition
// -----------------------------------------------------------------------------

/// Plugin for recognizing multi-touch gestures.
pub struct GesturePlugin;

impl Plugin for GesturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GestureState>()
            .add_event::<GestureEvent>()
            .add_systems(Update, detect_gestures);
    }
}

// -----------------------------------------------------------------------------
// 2. Gesture Types
// -----------------------------------------------------------------------------

/// Recognized gesture events.
#[derive(Event, Clone, Debug)]
pub enum GestureEvent {
    /// Single tap at position
    Tap(Vec2),
    /// Double tap at position
    DoubleTap(Vec2),
    /// Long press at position
    LongPress(Vec2),
    /// Swipe with direction and velocity
    Swipe { direction: SwipeDirection, velocity: f32 },
    /// Pinch zoom (scale factor, center point)
    Pinch { scale: f32, center: Vec2 },
    /// Two-finger rotation (angle in radians)
    Rotate { angle: f32, center: Vec2 },
}

/// Swipe direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Internal gesture tracking state.
#[derive(Resource, Default)]
pub struct GestureState {
    /// Active touches with start time and position
    touches: HashMap<u64, TouchInfo>,
    /// Last tap time for double-tap detection
    last_tap_time: f64,
    /// Last tap position
    last_tap_pos: Vec2,
    /// Initial distance for pinch detection
    initial_pinch_distance: Option<f32>,
}

struct TouchInfo {
    start_pos: Vec2,
    current_pos: Vec2,
    start_time: f64,
}

// -----------------------------------------------------------------------------
// 3. Gesture Detection
// -----------------------------------------------------------------------------

/// Detect gestures from touch input.
fn detect_gestures(
    mut touch_events: EventReader<TouchInput>,
    mut state: ResMut<GestureState>,
    mut gestures: EventWriter<GestureEvent>,
    time: Res<Time>,
) {
    let current_time = time.elapsed_seconds_f64();
    
    for event in touch_events.read() {
        match event.phase {
            TouchPhase::Started => {
                state.touches.insert(event.id, TouchInfo {
                    start_pos: event.position,
                    current_pos: event.position,
                    start_time: current_time,
                });
            }
            TouchPhase::Moved => {
                if let Some(info) = state.touches.get_mut(&event.id) {
                    info.current_pos = event.position;
                }
                
                // Check for pinch gesture (2 fingers)
                if state.touches.len() == 2 {
                    let positions: Vec<Vec2> = state.touches.values()
                        .map(|t| t.current_pos)
                        .collect();
                    
                    let current_distance = positions[0].distance(positions[1]);
                    let center = (positions[0] + positions[1]) / 2.0;
                    
                    if let Some(initial) = state.initial_pinch_distance {
                        let scale = current_distance / initial;
                        if (scale - 1.0).abs() > 0.05 {
                            gestures.send(GestureEvent::Pinch { scale, center });
                        }
                    } else {
                        state.initial_pinch_distance = Some(current_distance);
                    }
                }
            }
            TouchPhase::Ended => {
                if let Some(info) = state.touches.remove(&event.id) {
                    let duration = current_time - info.start_time;
                    let distance = event.position.distance(info.start_pos);
                    
                    // Tap detection (short duration, small movement)
                    if duration < 0.3 && distance < 20.0 {
                        // Check for double tap
                        if current_time - state.last_tap_time < 0.3 
                            && event.position.distance(state.last_tap_pos) < 50.0 
                        {
                            gestures.send(GestureEvent::DoubleTap(event.position));
                        } else {
                            gestures.send(GestureEvent::Tap(event.position));
                        }
                        state.last_tap_time = current_time;
                        state.last_tap_pos = event.position;
                    }
                    // Long press detection
                    else if duration > 0.5 && distance < 20.0 {
                        gestures.send(GestureEvent::LongPress(event.position));
                    }
                    // Swipe detection
                    else if distance > 50.0 && duration < 0.5 {
                        let delta = event.position - info.start_pos;
                        let velocity = distance / duration as f32;
                        
                        let direction = if delta.x.abs() > delta.y.abs() {
                            if delta.x > 0.0 { SwipeDirection::Right } else { SwipeDirection::Left }
                        } else {
                            if delta.y > 0.0 { SwipeDirection::Down } else { SwipeDirection::Up }
                        };
                        
                        gestures.send(GestureEvent::Swipe { direction, velocity });
                    }
                }
                
                // Reset pinch when touch ends
                if state.touches.len() < 2 {
                    state.initial_pinch_distance = None;
                }
            }
            TouchPhase::Canceled => {
                state.touches.remove(&event.id);
                if state.touches.len() < 2 {
                    state.initial_pinch_distance = None;
                }
            }
        }
    }
}

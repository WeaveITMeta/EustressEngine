//! # Property Overlay
//!
//! Real-time property display overlays for particles and bodies.
//!
//! ## Table of Contents
//!
//! 1. **OverlaySettings** - Configuration for what to display
//! 2. **PropertyOverlay** - Component to enable overlay on entity
//! 3. **Systems** - Rendering systems

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::realism::particles::components::{Particle, ThermodynamicState, KineticState};
use crate::realism::materials::stress_strain::StressTensor;
use crate::realism::materials::fracture::FractureState;

// ============================================================================
// Settings
// ============================================================================

/// Global settings for property overlays
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct OverlaySettings {
    /// Show temperature
    pub show_temperature: bool,
    /// Show pressure
    pub show_pressure: bool,
    /// Show velocity
    pub show_velocity: bool,
    /// Show entropy
    pub show_entropy: bool,
    /// Show internal energy
    pub show_energy: bool,
    /// Show stress (von Mises)
    pub show_stress: bool,
    /// Show damage
    pub show_damage: bool,
    /// Maximum distance to show overlays
    pub max_distance: f32,
    /// Text scale
    pub text_scale: f32,
    /// Show units
    pub show_units: bool,
    /// Decimal places
    pub decimal_places: usize,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            show_temperature: true,
            show_pressure: true,
            show_velocity: false,
            show_entropy: false,
            show_energy: false,
            show_stress: true,
            show_damage: true,
            max_distance: 50.0,
            text_scale: 1.0,
            decimal_places: 1,
        }
    }
}

// ============================================================================
// Property Overlay Component
// ============================================================================

/// Component to enable property overlay on an entity
#[derive(Component, Reflect, Clone, Debug, Default, Serialize, Deserialize)]
#[reflect(Component)]
pub struct PropertyOverlay {
    /// Override global settings for this entity
    pub custom_settings: Option<OverlaySettingsOverride>,
    /// Offset from entity position
    pub offset: Vec3,
    /// Always show (ignore distance)
    pub always_visible: bool,
}

/// Per-entity settings override
#[derive(Clone, Debug, Default, Reflect, Serialize, Deserialize)]
pub struct OverlaySettingsOverride {
    pub show_temperature: Option<bool>,
    pub show_pressure: Option<bool>,
    pub show_velocity: Option<bool>,
    pub show_stress: Option<bool>,
}

// ============================================================================
// Overlay Data
// ============================================================================

/// Collected property data for display
#[derive(Debug, Clone, Default)]
pub struct PropertyData {
    pub temperature: Option<f32>,
    pub pressure: Option<f32>,
    pub velocity: Option<Vec3>,
    pub entropy: Option<f32>,
    pub energy: Option<f32>,
    pub stress: Option<f32>,
    pub damage: Option<f32>,
}

impl PropertyData {
    /// Format as display string
    pub fn format(&self, settings: &OverlaySettings) -> String {
        let mut lines = Vec::new();
        let dp = settings.decimal_places;
        
        if settings.show_temperature {
            if let Some(t) = self.temperature {
                let unit = if settings.show_units { " K" } else { "" };
                lines.push(format!("T: {:.dp$}{}", t, unit));
            }
        }
        
        if settings.show_pressure {
            if let Some(p) = self.pressure {
                let (value, unit) = if p > 1e6 {
                    (p / 1e6, if settings.show_units { " MPa" } else { "" })
                } else if p > 1e3 {
                    (p / 1e3, if settings.show_units { " kPa" } else { "" })
                } else {
                    (p, if settings.show_units { " Pa" } else { "" })
                };
                lines.push(format!("P: {:.dp$}{}", value, unit));
            }
        }
        
        if settings.show_velocity {
            if let Some(v) = self.velocity {
                let speed = v.length();
                let unit = if settings.show_units { " m/s" } else { "" };
                lines.push(format!("v: {:.dp$}{}", speed, unit));
            }
        }
        
        if settings.show_entropy {
            if let Some(s) = self.entropy {
                let unit = if settings.show_units { " J/K" } else { "" };
                lines.push(format!("S: {:.dp$}{}", s, unit));
            }
        }
        
        if settings.show_energy {
            if let Some(u) = self.energy {
                let (value, unit) = if u > 1e6 {
                    (u / 1e6, if settings.show_units { " MJ" } else { "" })
                } else if u > 1e3 {
                    (u / 1e3, if settings.show_units { " kJ" } else { "" })
                } else {
                    (u, if settings.show_units { " J" } else { "" })
                };
                lines.push(format!("U: {:.dp$}{}", value, unit));
            }
        }
        
        if settings.show_stress {
            if let Some(sigma) = self.stress {
                let (value, unit) = if sigma > 1e9 {
                    (sigma / 1e9, if settings.show_units { " GPa" } else { "" })
                } else if sigma > 1e6 {
                    (sigma / 1e6, if settings.show_units { " MPa" } else { "" })
                } else {
                    (sigma / 1e3, if settings.show_units { " kPa" } else { "" })
                };
                lines.push(format!("σ: {:.dp$}{}", value, unit));
            }
        }
        
        if settings.show_damage {
            if let Some(d) = self.damage {
                if d > 0.01 {
                    lines.push(format!("D: {:.0}%", d * 100.0));
                }
            }
        }
        
        lines.join("\n")
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Update property overlays (placeholder - actual rendering depends on UI system)
pub fn update_property_overlays(
    query: Query<(
        Entity,
        &Transform,
        Option<&PropertyOverlay>,
        Option<&ThermodynamicState>,
        Option<&KineticState>,
        Option<&StressTensor>,
        Option<&FractureState>,
    )>,
    settings: Res<OverlaySettings>,
    camera_query: Query<&Transform, With<Camera>>,
) {
    // Get camera position for distance culling
    let camera_pos = camera_query.iter().next().map(|t| t.translation).unwrap_or(Vec3::ZERO);
    
    for (entity, transform, overlay, thermo, kinetic, stress, fracture) in query.iter() {
        // Skip if no overlay component and no relevant data
        if overlay.is_none() && thermo.is_none() && stress.is_none() {
            continue;
        }
        
        // Distance check
        let distance = (transform.translation - camera_pos).length();
        let always_visible = overlay.map(|o| o.always_visible).unwrap_or(false);
        if !always_visible && distance > settings.max_distance {
            continue;
        }
        
        // Collect property data
        let data = PropertyData {
            temperature: thermo.map(|t| t.temperature),
            pressure: thermo.map(|t| t.pressure),
            velocity: kinetic.map(|k| k.velocity),
            entropy: thermo.map(|t| t.entropy),
            energy: thermo.map(|t| t.internal_energy),
            stress: stress.map(|s| s.von_mises),
            damage: fracture.map(|f| f.damage),
        };
        
        // Format for display
        let _text = data.format(&settings);
        
        // Actual rendering would be done via egui or bevy_ui
        // This is a placeholder - the engine's UI system would consume this data
    }
}

// ============================================================================
// Color Utilities
// ============================================================================

/// Convert temperature to color (blue = cold, red = hot)
pub fn temperature_to_color(temperature: f32, min_temp: f32, max_temp: f32) -> Color {
    let t = ((temperature - min_temp) / (max_temp - min_temp)).clamp(0.0, 1.0);
    
    // Blue (cold) -> Cyan -> Green -> Yellow -> Red (hot)
    if t < 0.25 {
        let s = t / 0.25;
        Color::srgb(0.0, s, 1.0)
    } else if t < 0.5 {
        let s = (t - 0.25) / 0.25;
        Color::srgb(0.0, 1.0, 1.0 - s)
    } else if t < 0.75 {
        let s = (t - 0.5) / 0.25;
        Color::srgb(s, 1.0, 0.0)
    } else {
        let s = (t - 0.75) / 0.25;
        Color::srgb(1.0, 1.0 - s, 0.0)
    }
}

/// Convert stress to color (green = safe, red = yield)
pub fn stress_to_color(stress: f32, yield_stress: f32) -> Color {
    let ratio = (stress / yield_stress).clamp(0.0, 1.5);
    
    if ratio < 0.5 {
        // Green (safe)
        Color::srgb(0.0, 0.8, 0.0)
    } else if ratio < 0.8 {
        // Yellow (caution)
        let t = (ratio - 0.5) / 0.3;
        Color::srgb(t, 0.8, 0.0)
    } else if ratio < 1.0 {
        // Orange (warning)
        let t = (ratio - 0.8) / 0.2;
        Color::srgb(1.0, 0.8 - t * 0.5, 0.0)
    } else {
        // Red (yielded/failed)
        Color::srgb(1.0, 0.0, 0.0)
    }
}

/// Convert velocity to color (slow = blue, fast = red)
pub fn velocity_to_color(speed: f32, max_speed: f32) -> Color {
    let t = (speed / max_speed).clamp(0.0, 1.0);
    // HSL interpolation from blue (240°) to red (0°)
    let hue = 240.0 * (1.0 - t);
    Color::hsl(hue, 1.0, 0.5)
}

/// Convert damage to color (green = intact, red = damaged)
pub fn damage_to_color(damage: f32) -> Color {
    let t = damage.clamp(0.0, 1.0);
    Color::srgb(t, 1.0 - t, 0.0)
}

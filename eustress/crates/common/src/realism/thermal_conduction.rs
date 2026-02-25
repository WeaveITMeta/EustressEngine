//! # Thermal Conduction System
//!
//! Bevy ECS system that transfers heat between adjacent entities per frame
//! using Fourier's law via `heat_conduction_rate()`.
//!
//! ## Table of Contents
//!
//! 1. **ThermalContact** — Component marking two entities as thermally coupled
//! 2. **ThermalConductionConfig** — Resource for simulation parameters
//! 3. **thermal_conduction_system** — Per-frame heat transfer between contacts
//! 4. **auto_thermal_contacts_system** — Auto-detect contacts by proximity

use bevy::prelude::*;
use tracing::info;
use crate::realism::laws::thermodynamics::heat_conduction_rate;
use crate::realism::materials::properties::MaterialProperties;
use crate::realism::particles::components::ThermodynamicState;

// ============================================================================
// 1. Thermal Contact Component
// ============================================================================

/// Marks a thermal coupling between two entities.
/// Attach to a dedicated "contact" entity or to one of the pair.
#[derive(Component, Debug, Clone)]
pub struct ThermalContact {
    /// First entity in the thermal pair
    pub entity_a: Entity,
    /// Second entity in the thermal pair
    pub entity_b: Entity,
    /// Contact area (m²) — cross-section through which heat flows
    pub contact_area: f32,
    /// Contact thickness / distance (m) — conduction path length
    pub contact_thickness: f32,
}

// ============================================================================
// 2. Configuration Resource
// ============================================================================

/// Global configuration for thermal conduction simulation
#[derive(Resource, Debug, Clone)]
pub struct ThermalConductionConfig {
    /// Whether the system is enabled
    pub enabled: bool,
    /// Maximum temperature change per frame (K) — prevents instability
    pub max_delta_t_per_frame: f32,
    /// Minimum temperature difference to trigger conduction (K)
    pub min_delta_t_threshold: f32,
    /// Auto-contact detection radius (m) — entities closer than this get contacts
    pub auto_contact_radius: f32,
    /// Whether to auto-detect contacts by proximity
    pub auto_detect_contacts: bool,
}

impl Default for ThermalConductionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_delta_t_per_frame: 50.0,
            min_delta_t_threshold: 0.01,
            auto_contact_radius: 0.5,
            auto_detect_contacts: true,
        }
    }
}

// ============================================================================
// 3. Thermal Conduction System
// ============================================================================

/// Per-frame heat transfer between thermally-coupled entity pairs.
///
/// For each `ThermalContact`, computes heat flow using Fourier's law:
/// `Q = k_eff × A × ΔT / L` where k_eff is the harmonic mean of both
/// materials' thermal conductivities.
///
/// Then updates each entity's temperature: `ΔT = Q × dt / (m × c_p)`
pub fn thermal_conduction_system(
    time: Res<Time>,
    config: Res<ThermalConductionConfig>,
    contacts: Query<&ThermalContact>,
    mut thermal_entities: Query<(&MaterialProperties, &mut ThermodynamicState, &Transform)>,
) {
    if !config.enabled {
        return;
    }
    
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    
    for contact in contacts.iter() {
        // Get both entities' data — need unsafe-free two-entity access
        let Ok([entity_a_data, entity_b_data]) = thermal_entities.get_many_mut([contact.entity_a, contact.entity_b]) else {
            continue;
        };
        
        let (mat_a, mut thermo_a, transform_a) = entity_a_data;
        let (mat_b, mut thermo_b, transform_b) = entity_b_data;
        
        // Skip if temperature difference is negligible
        let delta_t = thermo_a.temperature - thermo_b.temperature;
        if delta_t.abs() < config.min_delta_t_threshold {
            continue;
        }
        
        // Effective thermal conductivity: harmonic mean of both materials
        // k_eff = 2 × k_a × k_b / (k_a + k_b)
        let k_sum = mat_a.thermal_conductivity + mat_b.thermal_conductivity;
        if k_sum <= 0.0 {
            continue;
        }
        let k_eff = 2.0 * mat_a.thermal_conductivity * mat_b.thermal_conductivity / k_sum;
        
        // Heat transfer rate (W) using Fourier's law
        let q_dot = heat_conduction_rate(
            k_eff,
            contact.contact_area,
            delta_t,
            contact.contact_thickness,
        );
        
        // Energy transferred this frame (J)
        let q_frame = q_dot * dt;
        
        // Compute mass from density and volume (approximate from scale)
        let vol_a = transform_a.scale.x * transform_a.scale.y * transform_a.scale.z;
        let vol_b = transform_b.scale.x * transform_b.scale.y * transform_b.scale.z;
        let mass_a = mat_a.density * vol_a;
        let mass_b = mat_b.density * vol_b;
        
        // Temperature change: ΔT = Q / (m × c_p)
        let heat_cap_a = mass_a * mat_a.specific_heat;
        let heat_cap_b = mass_b * mat_b.specific_heat;
        
        if heat_cap_a <= 0.0 || heat_cap_b <= 0.0 {
            continue;
        }
        
        let dt_a = -(q_frame / heat_cap_a).clamp(-config.max_delta_t_per_frame, config.max_delta_t_per_frame);
        let dt_b = (q_frame / heat_cap_b).clamp(-config.max_delta_t_per_frame, config.max_delta_t_per_frame);
        
        // Apply temperature changes (heat flows from hot to cold)
        thermo_a.temperature += dt_a;
        thermo_b.temperature += dt_b;
    }
}

// ============================================================================
// 4. Auto-Contact Detection System
// ============================================================================

/// Auto-detect thermal contacts between nearby entities that have both
/// MaterialProperties and ThermodynamicState components.
/// Runs at low frequency (every 0.5s) to avoid per-frame overhead.
pub fn auto_thermal_contacts_system(
    mut commands: Commands,
    config: Res<ThermalConductionConfig>,
    thermal_entities: Query<(Entity, &Transform, &MaterialProperties), With<ThermodynamicState>>,
    existing_contacts: Query<(Entity, &ThermalContact)>,
) {
    if !config.auto_detect_contacts {
        return;
    }
    
    let radius_sq = config.auto_contact_radius * config.auto_contact_radius;
    
    // Collect entity positions
    let entities: Vec<_> = thermal_entities.iter().collect();
    
    // Build set of existing contact pairs for deduplication
    let mut existing_pairs = std::collections::HashSet::new();
    for (_, contact) in existing_contacts.iter() {
        let pair = if contact.entity_a < contact.entity_b {
            (contact.entity_a, contact.entity_b)
        } else {
            (contact.entity_b, contact.entity_a)
        };
        existing_pairs.insert(pair);
    }
    
    // O(n²) proximity check — acceptable for <1000 thermal entities at 2Hz
    for i in 0..entities.len() {
        for j in (i + 1)..entities.len() {
            let (entity_a, transform_a, mat_a) = entities[i];
            let (entity_b, transform_b, _mat_b) = entities[j];
            
            let dist_sq = transform_a.translation.distance_squared(transform_b.translation);
            if dist_sq > radius_sq {
                continue;
            }
            
            // Canonical pair ordering for dedup
            let pair = if entity_a < entity_b {
                (entity_a, entity_b)
            } else {
                (entity_b, entity_a)
            };
            
            if existing_pairs.contains(&pair) {
                continue;
            }
            
            // Estimate contact area from smaller entity's face
            let min_scale = transform_a.scale.min(transform_b.scale);
            let contact_area = min_scale.x * min_scale.z; // Approximate face area
            
            // Contact thickness = distance between centers
            let thickness = dist_sq.sqrt().max(0.001);
            
            commands.spawn(ThermalContact {
                entity_a: pair.0,
                entity_b: pair.1,
                contact_area,
                contact_thickness: thickness,
            });
            
            existing_pairs.insert(pair);
        }
    }
}

// ============================================================================
// 5. Plugin
// ============================================================================

/// Thermal conduction plugin — registers systems and config resource
pub struct ThermalConductionPlugin;

impl Plugin for ThermalConductionPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<ThermalConductionConfig>()
            .add_systems(Update, thermal_conduction_system)
            .add_systems(
                Update,
                auto_thermal_contacts_system
                    .run_if(bevy::time::common_conditions::on_real_timer(
                        std::time::Duration::from_millis(500),
                    )),
            );
        
        info!("ThermalConductionPlugin initialized");
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_sane() {
        let cfg = ThermalConductionConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.max_delta_t_per_frame > 0.0);
        assert!(cfg.auto_contact_radius > 0.0);
    }
}

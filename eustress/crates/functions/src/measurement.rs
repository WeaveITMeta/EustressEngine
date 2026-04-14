//! # Stage 6: Measurement — Physics Value Reading, Entropy & Aggregation
//!
//! Reads simulation/physics values from ECS components through a pre-populated
//! snapshot bridge. The bridge is filled by a Bevy system before Rune execution
//! and cleared after — no live ECS access inside Rune.
//!
//! ## Table of Contents
//! 1. PropertySnapshot  — per-entity physics property map
//! 2. MeasurementBridge — thread-local snapshot store
//! 3. Rune functions    — measure / entropy / stats
//! 4. Module registration
//!
//! ## Functions
//!
//! | Function                     | Purpose                                             |
//! |------------------------------|-----------------------------------------------------|
//! | `measure(entity_bits, prop)` | Read a single physics property from an entity       |
//! | `entropy(entity_bits)`       | Shannon entropy of an entity's normalised state     |
//! | `stats(entity_bits_csv, prop)` | Aggregate stats (mean/min/max/variance) across N entities |
//!
//! ## Backed by
//! `SurfaceTemperature`, `ThermalDynamics`, `SpacetimeStress`, `Mass`,
//! `GravitationalForce`, `Reactor115`, `KinematicBuffer` — all from
//! `eustress_common::physics`.
//!
//! ## Bridge population (in a Bevy system, before calling Rune)
//!
//! ```rust,ignore
//! use eustress_functions::measurement::{MeasurementBridge, PropertySnapshot, set_measurement_bridge};
//!
//! fn populate_measurement_bridge(
//!     query: Query<(Entity, Option<&Mass>, Option<&SurfaceTemperature>, Option<&ThermalDynamics>, Option<&SpacetimeStress>)>,
//! ) {
//!     let mut snapshots = std::collections::HashMap::new();
//!     for (entity, mass, temp, thermal, stress) in &query {
//!         let mut snap = PropertySnapshot::default();
//!         if let Some(m) = mass         { snap.mass_kg = Some(m.kg); }
//!         if let Some(t) = temp         { snap.hull_temperature_k = Some(t.hull_temperature); }
//!         if let Some(th) = thermal     { snap.core_temperature_k = Some(th.core_temperature as f64); snap.shell_temperature_k = Some(th.shell_temperature as f64); }
//!         if let Some(s) = stress       { snap.spacetime_stress = Some(s.stress_level as f64); snap.snap_energy_j = Some(s.snap_energy); }
//!         snapshots.insert(entity.to_bits(), snap);
//!     }
//!     set_measurement_bridge(MeasurementBridge { snapshots });
//! }
//! ```
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::measurement;
//!
//! pub fn check_reactor_health(entity_bits) {
//!     let temp = measurement::measure(entity_bits, "core_temperature_k");
//!     let stress = measurement::measure(entity_bits, "spacetime_stress");
//!     if stress > 0.8 {
//!         eustress::log_warn("Spacetime stress critical!");
//!     }
//!     let h = measurement::entropy(entity_bits);
//!     eustress::log_info(&format!("entropy={}", h));
//! }
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use tracing::{info, warn};

// ============================================================================
// 1. PropertySnapshot — per-entity physics value snapshot
// ============================================================================

/// Snapshot of physics component values for a single entity.
///
/// Populated by a Bevy system before Rune execution. All values are `Option`
/// so missing components gracefully return `0.0` from `measure()`.
///
/// ## Supported Properties (string keys for `measure()`)
///
/// | Key                   | Source component         | Unit          |
/// |-----------------------|--------------------------|---------------|
/// | `mass_kg`             | `Mass.kg`                | kg            |
/// | `hull_temperature_k`  | `SurfaceTemperature`     | Kelvin        |
/// | `core_temperature_k`  | `ThermalDynamics`        | Kelvin        |
/// | `shell_temperature_k` | `ThermalDynamics`        | Kelvin        |
/// | `spacetime_stress`    | `SpacetimeStress`        | 0.0–1.0       |
/// | `snap_energy_j`       | `SpacetimeStress`        | Joules        |
/// | `reactor_output_w`    | `Reactor115`             | Watts         |
/// | `reactor_fuel_kg`     | `Reactor115`             | kg            |
/// | `gravity_force_n`     | `GravitationalForce`     | Newtons       |
/// | `kinematic_velocity`  | `KinematicBuffer`        | m/s magnitude |
#[derive(Debug, Clone, Default)]
pub struct PropertySnapshot {
    /// Mass (kg) from `Mass` component
    pub mass_kg: Option<f64>,
    /// External hull temperature (K) from `SurfaceTemperature`
    pub hull_temperature_k: Option<f64>,
    /// Internal core temperature (K) from `ThermalDynamics`
    pub core_temperature_k: Option<f64>,
    /// External shell temperature (K) from `ThermalDynamics`
    pub shell_temperature_k: Option<f64>,
    /// Accumulated spacetime stress (0–1) from `SpacetimeStress`
    pub spacetime_stress: Option<f64>,
    /// Snap event energy (J) from `SpacetimeStress`
    pub snap_energy_j: Option<f64>,
    /// Current reactor power output (W) from `Reactor115`
    pub reactor_output_w: Option<f64>,
    /// Remaining fuel mass (kg) from `Reactor115`
    pub reactor_fuel_kg: Option<f64>,
    /// Gravitational force magnitude (N) from `GravitationalForce`
    pub gravity_force_n: Option<f64>,
    /// Kinematic velocity magnitude (m/s) from `KinematicBuffer`
    pub kinematic_velocity: Option<f64>,
}

impl PropertySnapshot {
    /// Read a named property value. Returns `None` if not present.
    pub fn get(&self, key: &str) -> Option<f64> {
        match key {
            "mass_kg"             => self.mass_kg,
            "hull_temperature_k"  => self.hull_temperature_k,
            "core_temperature_k"  => self.core_temperature_k,
            "shell_temperature_k" => self.shell_temperature_k,
            "spacetime_stress"    => self.spacetime_stress,
            "snap_energy_j"       => self.snap_energy_j,
            "reactor_output_w"    => self.reactor_output_w,
            "reactor_fuel_kg"     => self.reactor_fuel_kg,
            "gravity_force_n"     => self.gravity_force_n,
            "kinematic_velocity"  => self.kinematic_velocity,
            _ => None,
        }
    }

    /// Collect all present values as a normalised probability distribution
    /// for entropy calculation. Values are normalised to sum = 1.0.
    pub fn normalised_distribution(&self) -> Vec<f64> {
        let raw: Vec<f64> = [
            self.mass_kg,
            self.hull_temperature_k,
            self.core_temperature_k,
            self.shell_temperature_k,
            self.spacetime_stress,
            self.snap_energy_j,
            self.reactor_output_w,
            self.reactor_fuel_kg,
            self.gravity_force_n,
            self.kinematic_velocity,
        ]
        .iter()
        .filter_map(|v| *v)
        .filter(|v| *v > 0.0)
        .collect();

        if raw.is_empty() {
            return Vec::new();
        }

        let total: f64 = raw.iter().sum();
        if total <= 0.0 {
            return raw.iter().map(|_| 1.0 / raw.len() as f64).collect();
        }

        raw.iter().map(|v| v / total).collect()
    }
}

// ============================================================================
// 2. MeasurementBridge — thread-local snapshot store
// ============================================================================

/// Bridge holding snapshots of all entities' physics values.
///
/// Populated by a Bevy system before Rune execution, cleared after.
pub struct MeasurementBridge {
    /// Entity bits → physics snapshot
    pub snapshots: HashMap<u64, PropertySnapshot>,
}

impl Default for MeasurementBridge {
    fn default() -> Self {
        Self { snapshots: HashMap::new() }
    }
}

impl MeasurementBridge {
    /// Create an empty bridge
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a snapshot for an entity
    pub fn insert(&mut self, entity_bits: u64, snapshot: PropertySnapshot) {
        self.snapshots.insert(entity_bits, snapshot);
    }
}

thread_local! {
    static MEASUREMENT_BRIDGE: RefCell<Option<MeasurementBridge>> = RefCell::new(None);
}

/// Install the measurement bridge before Rune execution.
pub fn set_measurement_bridge(bridge: MeasurementBridge) {
    MEASUREMENT_BRIDGE.with(|cell| {
        *cell.borrow_mut() = Some(bridge);
    });
}

/// Remove and return the bridge after Rune execution.
pub fn take_measurement_bridge() -> Option<MeasurementBridge> {
    MEASUREMENT_BRIDGE.with(|cell| cell.borrow_mut().take())
}

fn with_bridge<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&MeasurementBridge) -> R,
{
    MEASUREMENT_BRIDGE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bridge) => f(bridge),
            None => {
                warn!("[Measurement] Bridge not available — returning fallback");
                fallback
            }
        }
    })
}

// ============================================================================
// 3. Rune Functions
// ============================================================================

/// Read a single physics/simulation property from an entity.
///
/// Returns the value as `f64`, or `0.0` if the entity has no such component
/// or the bridge is unavailable.
///
/// # Arguments
/// * `entity_bits` — Bevy `Entity::to_bits()` as u64
/// * `property`    — Property name (see `PropertySnapshot` key table)
///
/// # Example
/// ```rune
/// let temp = measurement::measure(entity_bits, "core_temperature_k");
/// ```
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn measure(entity_bits: u64, property: &str) -> f64 {
    with_bridge(0.0, |bridge| {
        let value = bridge
            .snapshots
            .get(&entity_bits)
            .and_then(|snap| snap.get(property))
            .unwrap_or(0.0);

        info!(
            "[Measurement] measure({}, {}) = {}",
            entity_bits, property, value
        );

        value
    })
}

/// Calculate Shannon entropy of an entity's normalised physics state.
///
/// Entropy is high when many properties are active with roughly equal values
/// (chaotic state), and low when one property dominates (ordered state).
///
/// Uses the standard formula: H = -Σ pᵢ·log₂(pᵢ)
///
/// # Arguments
/// * `entity_bits` — Bevy `Entity::to_bits()` as u64
///
/// # Returns
/// Entropy in bits (0.0 = fully ordered, ~3.32 = max for 10 equal components)
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn entropy(entity_bits: u64) -> f64 {
    with_bridge(0.0, |bridge| {
        let Some(snap) = bridge.snapshots.get(&entity_bits) else {
            warn!("[Measurement] entropy: entity {} not in bridge", entity_bits);
            return 0.0;
        };

        let dist = snap.normalised_distribution();
        if dist.is_empty() {
            return 0.0;
        }

        // Shannon entropy: H = -Σ pᵢ · log₂(pᵢ)
        let h = dist
            .iter()
            .filter(|&&p| p > 0.0)
            .map(|&p| -p * p.log2())
            .sum::<f64>();

        info!("[Measurement] entropy({}) = {:.4} bits", entity_bits, h);

        h
    })
}

/// Aggregate statistics (mean, min, max, variance) for a property across N entities.
///
/// # Arguments
/// * `entity_bits_csv` — Comma-separated u64 entity bits string
///   (e.g. `"123456,789012,345678"`)
/// * `property` — Property name (see `PropertySnapshot` key table)
///
/// # Returns
/// A `MeasurementStats` value with mean, min, max, variance, and sample count.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn stats(entity_bits_csv: &str, property: &str) -> MeasurementStats {
    with_bridge(MeasurementStats::empty(), |bridge| {
        // Parse comma-separated entity bits
        let values: Vec<f64> = entity_bits_csv
            .split(',')
            .filter_map(|s| s.trim().parse::<u64>().ok())
            .filter_map(|bits| {
                bridge.snapshots.get(&bits)?.get(property)
            })
            .collect();

        if values.is_empty() {
            warn!(
                "[Measurement] stats: no data for property '{}' in {} entities",
                property, entity_bits_csv.split(',').count()
            );
            return MeasurementStats::empty();
        }

        let count = values.len();
        let sum: f64 = values.iter().sum();
        let mean = sum / count as f64;
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;

        info!(
            "[Measurement] stats({} entities, {}) → mean={:.4} min={:.4} max={:.4} var={:.4}",
            count, property, mean, min, max, variance
        );

        MeasurementStats { mean, min, max, variance, count: count as i64 }
    })
}

// ============================================================================
// MeasurementStats — returned to Rune from stats()
// ============================================================================

/// Aggregate statistics result from `stats()`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct MeasurementStats {
    /// Arithmetic mean of the property across all entities
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub mean: f64,
    /// Minimum observed value
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub min: f64,
    /// Maximum observed value
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub max: f64,
    /// Population variance
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub variance: f64,
    /// Number of entities that had this property
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub count: i64,
}

impl MeasurementStats {
    fn empty() -> Self {
        Self { mean: 0.0, min: 0.0, max: 0.0, variance: 0.0, count: 0 }
    }

    /// Standard deviation (sqrt of variance)
    pub fn std_dev(&self) -> f64 {
        self.variance.sqrt()
    }
}

// ============================================================================
// 4. Module Registration
// ============================================================================

/// Create the `measurement` Rune module.
#[cfg(feature = "rune-dsl")]
pub fn create_measurement_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "measurement"])?;

    module.ty::<MeasurementStats>()?;

    module.function_meta(measure)?;
    module.function_meta(entropy)?;
    module.function_meta(stats)?;

    Ok(module)
}

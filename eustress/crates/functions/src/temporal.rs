//! # Stage 9: Temporal — Tick, Diff, Evolve & Snapshot
//!
//! Provides time-aware operations over ECS entity state. The bridge is pre-populated
//! by a Bevy system before Rune execution with a tick counter, entity state snapshots,
//! and a history ring (last N ticks per entity).
//!
//! ## Table of Contents
//! 1. TemporalSnapshot  — per-entity multi-tick state history
//! 2. TemporalBridge    — thread-local clock + history store
//! 3. Rune functions    — tick / diff / evolve / snapshot
//! 4. Module registration
//!
//! ## Functions
//!
//! | Function                        | Purpose                                              |
//! |---------------------------------|------------------------------------------------------|
//! | `tick()`                        | Get current simulation tick as i64                  |
//! | `diff(entity_bits, ticks_ago)`  | Compute property deltas over N ticks                 |
//! | `evolve(entity_bits, prop, t)`  | Linearly interpolate a property toward a target      |
//! | `snapshot()`                    | Return a summary of the current world state tick     |
//!
//! ## Bridge population (Bevy system)
//!
//! ```rust,ignore
//! use eustress_functions::temporal::{TemporalBridge, EntityTickRecord, set_temporal_bridge};
//!
//! fn populate_temporal_bridge(
//!     tick: Res<SimClock>,
//!     query: Query<(Entity, &Transform)>,
//! ) {
//!     let mut bridge = TemporalBridge::new(tick.current_tick);
//!     for (entity, transform) in &query {
//!         let mut record = EntityTickRecord::default();
//!         record.properties.insert("pos_x".into(), transform.translation.x as f64);
//!         record.properties.insert("pos_y".into(), transform.translation.y as f64);
//!         record.properties.insert("pos_z".into(), transform.translation.z as f64);
//!         bridge.insert(entity.to_bits(), record);
//!     }
//!     set_temporal_bridge(bridge);
//! }
//! ```
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::temporal;
//!
//! pub fn track_entity(entity_bits) {
//!     let t = temporal::tick();
//!     let d = temporal::diff(entity_bits, 10);
//!     for entry in d.changes {
//!         eustress::log_info(&format!("[{}] Δ{}", entry.property, entry.delta));
//!     }
//! }
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use tracing::{info, warn};

// ============================================================================
// 1. TemporalSnapshot — per-entity per-tick property record
// ============================================================================

/// A record of an entity's property values at one tick.
#[derive(Debug, Clone, Default)]
pub struct EntityTickRecord {
    /// Property name → value at this tick
    pub properties: HashMap<String, f64>,
    /// The tick number this record was captured at
    pub tick: i64,
}

impl EntityTickRecord {
    /// Create a record at a specific tick
    pub fn at_tick(tick: i64) -> Self {
        Self { properties: HashMap::new(), tick }
    }

    /// Insert a property value
    pub fn set(&mut self, property: impl Into<String>, value: f64) {
        self.properties.insert(property.into(), value);
    }
}

/// Per-entity ring of recent tick records (up to `TemporalBridge::history_depth`).
#[derive(Debug, Default)]
pub struct EntityHistory {
    /// Ordered records, most recent last
    pub records: Vec<EntityTickRecord>,
    /// Current-tick properties (convenience alias for last record)
    pub current: HashMap<String, f64>,
}

impl EntityHistory {
    /// Add a new tick record. Keeps up to `max_depth` records.
    pub fn push(&mut self, record: EntityTickRecord, max_depth: usize) {
        self.current = record.properties.clone();
        self.records.push(record);
        if self.records.len() > max_depth {
            self.records.remove(0);
        }
    }

    /// Get record that is `ticks_ago` before the most recent.
    pub fn record_ago(&self, ticks_ago: usize) -> Option<&EntityTickRecord> {
        let len = self.records.len();
        if ticks_ago >= len {
            self.records.first()
        } else {
            self.records.get(len - 1 - ticks_ago)
        }
    }
}

// ============================================================================
// 2. TemporalBridge
// ============================================================================

/// Bridge holding the current tick and per-entity state history.
pub struct TemporalBridge {
    /// Current simulation tick (monotonically increasing integer)
    pub current_tick: i64,
    /// Entity histories keyed by entity bits
    pub histories: HashMap<u64, EntityHistory>,
    /// How many ticks of history to retain per entity
    pub history_depth: usize,
    /// Total entity count tracked this tick (for snapshot summary)
    pub entity_count: usize,
}

impl TemporalBridge {
    /// Create a new bridge at the given tick.
    pub fn new(current_tick: i64) -> Self {
        Self {
            current_tick,
            histories: HashMap::new(),
            history_depth: 64,
            entity_count: 0,
        }
    }

    /// Insert a tick record for an entity.
    pub fn insert(&mut self, entity_bits: u64, record: EntityTickRecord) {
        let depth = self.history_depth;
        self.histories
            .entry(entity_bits)
            .or_default()
            .push(record, depth);
        self.entity_count = self.histories.len();
    }

    /// Set history depth (max ticks retained per entity).
    pub fn with_history_depth(mut self, depth: usize) -> Self {
        self.history_depth = depth;
        self
    }
}

thread_local! {
    static TEMPORAL_BRIDGE: RefCell<Option<TemporalBridge>> = RefCell::new(None);
}

/// Install the temporal bridge before Rune execution.
pub fn set_temporal_bridge(bridge: TemporalBridge) {
    TEMPORAL_BRIDGE.with(|cell| {
        *cell.borrow_mut() = Some(bridge);
    });
}

/// Remove and return the bridge after Rune execution.
pub fn take_temporal_bridge() -> Option<TemporalBridge> {
    TEMPORAL_BRIDGE.with(|cell| cell.borrow_mut().take())
}

fn with_bridge<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&TemporalBridge) -> R,
{
    TEMPORAL_BRIDGE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bridge) => f(bridge),
            None => {
                warn!("[Temporal] Bridge not available — returning fallback");
                fallback
            }
        }
    })
}

// ============================================================================
// 3. Rune Functions
// ============================================================================

/// Get the current simulation tick counter.
///
/// Returns the monotonically increasing tick number. One tick corresponds
/// to one Bevy `Update` frame when the simulation is running.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn tick() -> i64 {
    with_bridge(-1, |bridge| {
        info!("[Temporal] tick() = {}", bridge.current_tick);
        bridge.current_tick
    })
}

/// Compute property deltas for an entity over the last N ticks.
///
/// For every property present in both the current tick record and the record
/// `ticks_ago` ticks earlier, emits a `PropertyDelta` with the difference.
///
/// # Arguments
/// * `entity_bits` — Bevy `Entity::to_bits()` as u64
/// * `ticks_ago`   — How many ticks back to compare (clamped to history depth)
///
/// # Returns
/// A `DiffResult` containing the list of property changes.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn diff(entity_bits: u64, ticks_ago: i64) -> DiffResult {
    let ticks_ago = ticks_ago.max(1) as usize;

    with_bridge(DiffResult::empty(), |bridge| {
        let Some(history) = bridge.histories.get(&entity_bits) else {
            warn!("[Temporal] diff: entity {} not in bridge", entity_bits);
            return DiffResult::empty();
        };

        let current = match history.records.last() {
            Some(r) => r,
            None => return DiffResult::empty(),
        };

        let past = match history.record_ago(ticks_ago) {
            Some(r) => r,
            None => return DiffResult::empty(),
        };

        let tick_span = current.tick - past.tick;
        let mut changes = rune::runtime::Vec::new();

        for (prop, &current_val) in &current.properties {
            if let Some(&past_val) = past.properties.get(prop) {
                let delta = current_val - past_val;
                let rate = if tick_span > 0 { delta / tick_span as f64 } else { 0.0 };

                let pd = PropertyDelta {
                    property: prop.clone(),
                    from: past_val,
                    to: current_val,
                    delta,
                    rate_per_tick: rate,
                };

                if let Ok(v) = rune::to_value(pd) {
                    let _ = changes.push(v);
                }
            }
        }

        let count = changes.len() as i64;
        info!(
            "[Temporal] diff(entity={}, ticks_ago={}) → {} property deltas over {} ticks",
            entity_bits, ticks_ago, count, tick_span
        );

        DiffResult {
            changes,
            tick_span,
            change_count: count,
        }
    })
}

/// Linearly interpolate a property value toward a target over time.
///
/// Computes where a property would be after `t` ticks given constant velocity
/// from its current rate of change (from `diff`). Returns the projected value.
///
/// This is stateless — it reads current velocity from recent history and
/// extrapolates. It does not write to ECS; that must be done by the caller.
///
/// # Arguments
/// * `entity_bits` — Entity to project
/// * `property`    — Property name
/// * `t`           — Number of ticks to project forward
///
/// # Returns
/// Projected value as f64, or current value if no rate can be computed.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn evolve(entity_bits: u64, property: &str, t: f64) -> f64 {
    with_bridge(0.0, |bridge| {
        let Some(history) = bridge.histories.get(&entity_bits) else {
            warn!("[Temporal] evolve: entity {} not in bridge", entity_bits);
            return 0.0;
        };

        let current_val = history.current.get(property).copied().unwrap_or(0.0);

        // Need at least 2 records for velocity
        if history.records.len() < 2 {
            return current_val;
        }

        let last = history.records.last().unwrap();
        let prev = &history.records[history.records.len() - 2];
        let tick_span = (last.tick - prev.tick).max(1) as f64;

        let velocity = match (last.properties.get(property), prev.properties.get(property)) {
            (Some(&v1), Some(&v0)) => (v1 - v0) / tick_span,
            _ => 0.0,
        };

        let projected = current_val + velocity * t;

        info!(
            "[Temporal] evolve(entity={}, '{}', t={}) → {} (velocity={:.4}/tick)",
            entity_bits, property, t, projected, velocity
        );

        projected
    })
}

/// Capture a summary of the current world state at this tick.
///
/// Returns a `WorldSnapshot` with tick number, entity count, and
/// a CSV string of all tracked entity bits for inspection.
///
/// This is a lightweight read — does not clone full state.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn snapshot() -> WorldSnapshot {
    with_bridge(WorldSnapshot::empty(), |bridge| {
        let entity_csv = bridge
            .histories
            .keys()
            .map(|bits| bits.to_string())
            .collect::<Vec<_>>()
            .join(",");

        info!(
            "[Temporal] snapshot() tick={} entities={}",
            bridge.current_tick, bridge.entity_count
        );

        WorldSnapshot {
            tick: bridge.current_tick,
            entity_count: bridge.entity_count as i64,
            entity_bits_csv: entity_csv,
        }
    })
}

// ============================================================================
// Return types
// ============================================================================

/// A single property change record from `diff()`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct PropertyDelta {
    /// Property name
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub property: String,
    /// Value N ticks ago
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub from: f64,
    /// Current value
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub to: f64,
    /// Absolute change (to - from)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub delta: f64,
    /// Change per tick (delta / tick_span)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub rate_per_tick: f64,
}

/// Result from `diff()`.
#[derive(Debug)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct DiffResult {
    /// List of `PropertyDelta` values
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub changes: rune::runtime::Vec,
    /// Number of ticks spanned by the comparison
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub tick_span: i64,
    /// Number of properties that changed
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub change_count: i64,
}

impl DiffResult {
    fn empty() -> Self {
        Self {
            changes: rune::runtime::Vec::new(),
            tick_span: 0,
            change_count: 0,
        }
    }
}

/// Summary snapshot from `snapshot()`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct WorldSnapshot {
    /// Current simulation tick
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub tick: i64,
    /// Total number of entities in the bridge
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub entity_count: i64,
    /// Comma-separated entity bits for all tracked entities
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub entity_bits_csv: String,
}

impl WorldSnapshot {
    fn empty() -> Self {
        Self { tick: -1, entity_count: 0, entity_bits_csv: String::new() }
    }
}

// ============================================================================
// 4. Module Registration
// ============================================================================

/// Create the `temporal` Rune module.
#[cfg(feature = "rune-dsl")]
pub fn create_temporal_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "temporal"])?;

    module.ty::<PropertyDelta>()?;
    module.ty::<DiffResult>()?;
    module.ty::<WorldSnapshot>()?;

    module.function_meta(tick)?;
    module.function_meta(diff)?;
    module.function_meta(evolve)?;
    module.function_meta(snapshot)?;

    Ok(module)
}

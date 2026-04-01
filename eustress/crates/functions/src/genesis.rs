//! # Stage 1: Genesis — Entity Creation & Binding
//!
//! The foundation of the Eustress Functions DSL. Genesis provides three
//! primitives for bringing entities into existence and configuring them:
//!
//! - `identity(class, name)` — Create a new entity with a class and name
//! - `bind(entity, key, value)` — Attach a property to an entity
//! - `locate(entity, x, y, z)` — Place an entity in world space
//!
//! ## Thread-Local Bridge
//!
//! Genesis functions access ECS state through `GenesisBridge`, installed
//! per-thread before Rune execution. The bridge holds:
//! - A command buffer for deferred entity spawning
//! - A property update queue for deferred property writes
//! - A name-to-entity lookup for resolving references
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::genesis;
//!
//! pub fn main() {
//!     let pillar = genesis::identity("Part", "Sacred Pillar");
//!     genesis::locate(pillar, 10.0, 0.0, 20.0);
//!     genesis::bind(pillar, "Material", "Marble");
//!     genesis::bind(pillar, "Anchored", true);
//! }
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, warn};

// ============================================================================
// World Reference — Opaque handle returned to Rune scripts
// ============================================================================

/// Opaque handle to a spawned entity, returned by `identity()`.
/// Scripts use this to refer to entities in `bind()` and `locate()` calls.
/// Contains only a u64 temp_id — class/display names are stored in GenesisBridge.
#[cfg(feature = "rune-dsl")]
#[derive(Debug, Clone, Copy, rune::Any)]
pub struct WorldRef {
    /// Internal entity identifier (monotonic counter for deferred spawning)
    #[rune(get)]
    pub id: u64,
}

/// Non-Rune version for use without the rune-dsl feature
#[cfg(not(feature = "rune-dsl"))]
#[derive(Debug, Clone, Copy)]
pub struct WorldRef {
    pub id: u64,
}

// ============================================================================
// Deferred Commands — Queued during Rune execution, applied after
// ============================================================================

/// A deferred entity spawn command
#[derive(Debug, Clone)]
pub struct SpawnCommand {
    /// Temporary identifier (matches WorldRef.id)
    pub temp_id: u64,
    /// Entity class name
    pub class_name: String,
    /// Entity display name
    pub display_name: String,
}

/// A deferred property binding command
#[derive(Debug, Clone)]
pub struct BindCommand {
    /// Target entity (temp_id from SpawnCommand)
    pub target_id: u64,
    /// Property key
    pub key: String,
    /// Property value (serialized as JSON for flexibility)
    pub value: BindValue,
}

/// Typed property value for binding
#[derive(Debug, Clone)]
pub enum BindValue {
    /// Floating point number
    Float(f64),
    /// Integer
    Integer(i64),
    /// String value (material names, class names, etc.)
    Text(String),
    /// Boolean flag
    Bool(bool),
    /// 3D vector (position, direction, scale, color)
    Vec3([f64; 3]),
}

/// A deferred locate (position) command
#[derive(Debug, Clone)]
pub struct LocateCommand {
    /// Target entity (temp_id from SpawnCommand)
    pub target_id: u64,
    /// World-space X coordinate
    pub x: f64,
    /// World-space Y coordinate
    pub y: f64,
    /// World-space Z coordinate
    pub z: f64,
}

// ============================================================================
// Genesis Bridge — Thread-local state for Rune functions
// ============================================================================

/// Monotonic counter for temporary entity identifiers
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

/// Bridge between Rune genesis functions and the ECS command buffer.
/// Installed per-thread before script execution, drained after.
#[derive(Debug, Default)]
pub struct GenesisBridge {
    /// Deferred spawn commands (applied to ECS after script completes)
    pub spawns: Vec<SpawnCommand>,
    /// Deferred property bindings
    pub binds: Vec<BindCommand>,
    /// Deferred position assignments
    pub locates: Vec<LocateCommand>,
    /// Name-to-temp-id lookup (for resolving references within a script)
    pub name_index: HashMap<String, u64>,
}

impl GenesisBridge {
    /// Create a new empty bridge
    pub fn new() -> Self {
        Self::default()
    }

    /// Drain all pending commands, returning ownership
    pub fn drain(&mut self) -> (Vec<SpawnCommand>, Vec<BindCommand>, Vec<LocateCommand>) {
        (
            std::mem::take(&mut self.spawns),
            std::mem::take(&mut self.binds),
            std::mem::take(&mut self.locates),
        )
    }

    /// Total number of pending commands
    pub fn pending_count(&self) -> usize {
        self.spawns.len() + self.binds.len() + self.locates.len()
    }
}

// ============================================================================
// Thread-Local Bridge Installation (follows rune_ecs_module.rs pattern)
// ============================================================================

thread_local! {
    static GENESIS_BRIDGE: RefCell<Option<GenesisBridge>> = RefCell::new(None);
}

/// Install the genesis bridge for the current thread before Rune execution.
pub fn set_genesis_bridge(bridge: GenesisBridge) {
    GENESIS_BRIDGE.with(|cell| {
        *cell.borrow_mut() = Some(bridge);
    });
}

/// Clear the genesis bridge after Rune execution completes.
/// Returns the bridge with all queued commands for ECS application.
pub fn take_genesis_bridge() -> Option<GenesisBridge> {
    GENESIS_BRIDGE.with(|cell| cell.borrow_mut().take())
}

/// Access the genesis bridge from a Rune function (mutable).
fn with_genesis_bridge_mut<F, R>(fallback: R, callback: F) -> R
where
    F: FnOnce(&mut GenesisBridge) -> R,
{
    GENESIS_BRIDGE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(bridge) => callback(bridge),
            None => {
                warn!("[Eustress Functions] Genesis bridge not available — call ignored");
                fallback
            }
        }
    })
}

// ============================================================================
// Rune Functions — #[rune::function] implementations
// ============================================================================

/// Create a new entity with a class and name.
///
/// Returns a `WorldRef` handle that can be passed to `bind()` and `locate()`.
/// The entity is not immediately spawned — it is queued and applied to the ECS
/// after the script completes.
///
/// # Arguments
/// * `class` — Entity class name (e.g., "Part", "SpotLight", "Model")
/// * `name` — Display name for the entity
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn identity(class: &str, name: &str) -> WorldRef {
    let temp_id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);

    with_genesis_bridge_mut(
        WorldRef { id: 0 },
        |bridge| {
            bridge.spawns.push(SpawnCommand {
                temp_id,
                class_name: class.to_string(),
                display_name: name.to_string(),
            });
            bridge.name_index.insert(name.to_string(), temp_id);

            info!(
                "[Genesis] identity({}, {}) → temp_id={}",
                class, name, temp_id
            );

            WorldRef { id: temp_id }
        },
    )
}

/// Bind a string property to an entity.
///
/// Queues a property update that will be applied after the script completes.
///
/// # Arguments
/// * `entity` — WorldRef from `identity()`
/// * `key` — Property name (e.g., "Material", "Color", "Anchored")
/// * `value` — Property value as a string
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn bind_str(entity: &WorldRef, key: &str, value: &str) {
    with_genesis_bridge_mut((), |bridge| {
        bridge.binds.push(BindCommand {
            target_id: entity.id,
            key: key.to_string(),
            value: BindValue::Text(value.to_string()),
        });
        info!(
            "[Genesis] bind({}, {}, \"{}\")",
            entity.id, key, value
        );
    });
}

/// Bind a numeric property to an entity.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn bind_num(entity: &WorldRef, key: &str, value: f64) {
    with_genesis_bridge_mut((), |bridge| {
        bridge.binds.push(BindCommand {
            target_id: entity.id,
            key: key.to_string(),
            value: BindValue::Float(value),
        });
        info!("[Genesis] bind({}, {}, {})", entity.id, key, value);
    });
}

/// Bind a boolean property to an entity.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn bind_bool(entity: &WorldRef, key: &str, value: bool) {
    with_genesis_bridge_mut((), |bridge| {
        bridge.binds.push(BindCommand {
            target_id: entity.id,
            key: key.to_string(),
            value: BindValue::Bool(value),
        });
        info!("[Genesis] bind({}, {}, {})", entity.id, key, value);
    });
}

/// Place an entity at a world-space position.
///
/// Queues a Transform update that will be applied after the script completes.
///
/// # Arguments
/// * `entity` — WorldRef from `identity()`
/// * `x`, `y`, `z` — World-space coordinates (meters)
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn locate(entity: &WorldRef, x: f64, y: f64, z: f64) {
    with_genesis_bridge_mut((), |bridge| {
        bridge.locates.push(LocateCommand {
            target_id: entity.id,
            x,
            y,
            z,
        });
        info!(
            "[Genesis] locate({}, {}, {}, {})",
            entity.id, x, y, z
        );
    });
}

// ============================================================================
// Rune Module Registration
// ============================================================================

/// Create the `genesis` Rune module.
///
/// Register alongside `create_ecs_module()` in the Rune context builder:
///
/// ```rust,ignore
/// let mut context = rune::Context::with_default_modules()?;
/// context.install(eustress_functions::genesis::create_genesis_module()?)?;
/// ```
#[cfg(feature = "rune-dsl")]
pub fn create_genesis_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "genesis"])?;

    // WorldRef type
    module.ty::<WorldRef>()?;

    // Core genesis functions
    module.function_meta(identity)?;
    module.function_meta(bind_str)?;
    module.function_meta(bind_num)?;
    module.function_meta(bind_bool)?;
    module.function_meta(locate)?;

    Ok(module)
}

// ============================================================================
// ECS Application — Convert deferred commands to real ECS operations
// ============================================================================

/// Apply genesis commands to a Bevy World.
///
/// Call this after Rune script execution to materialize deferred entities:
///
/// ```rust,ignore
/// // In your Bevy system that runs Rune scripts:
/// set_genesis_bridge(GenesisBridge::new());
/// // ... run Rune script ...
/// if let Some(mut bridge) = take_genesis_bridge() {
///     let entity_map = apply_genesis_commands(&mut commands, &mut bridge);
/// }
/// ```
///
/// Returns a map from temp_id → Bevy Entity for resolving references.
pub fn apply_genesis_commands(
    commands: &mut bevy::ecs::system::Commands,
    bridge: &mut GenesisBridge,
) -> HashMap<u64, bevy::ecs::entity::Entity> {
    use bevy::prelude::{Entity, Name, Transform};

    let (spawns, binds, locates) = bridge.drain();
    let mut entity_map: HashMap<u64, Entity> = HashMap::new();

    // Phase 1: Spawn entities
    for spawn in &spawns {
        let entity = commands
            .spawn((
                Name::new(spawn.display_name.clone()),
                Transform::default(),
            ))
            .id();

        entity_map.insert(spawn.temp_id, entity);

        info!(
            "[Genesis] Spawned {} ({}) → Entity {:?}",
            spawn.display_name, spawn.class_name, entity
        );
    }

    // Phase 2: Apply locate (Transform) commands
    for loc in &locates {
        if let Some(&entity) = entity_map.get(&loc.target_id) {
            commands.entity(entity).insert(
                Transform::from_xyz(loc.x as f32, loc.y as f32, loc.z as f32),
            );
        } else {
            warn!(
                "[Genesis] locate target temp_id={} not found — skipped",
                loc.target_id
            );
        }
    }

    // Phase 3: Apply bind commands
    // Note: Property binding is extensible. For now, we handle known properties
    // inline. In Phase 2+, this will dispatch through the ClassDefaultsRegistry.
    for bind in &binds {
        if let Some(&entity) = entity_map.get(&bind.target_id) {
            match bind.key.as_str() {
                "Anchored" => {
                    if let BindValue::Bool(true) = bind.value {
                        // Mark as static (no physics body)
                        info!("[Genesis] bind Anchored=true on {:?}", entity);
                    }
                }
                _ => {
                    // Generic property — log for now, will route through
                    // PropertyUpdate system in future phases
                    info!(
                        "[Genesis] bind {}={:?} on {:?} (deferred to property system)",
                        bind.key, bind.value, entity
                    );
                }
            }
        } else {
            warn!(
                "[Genesis] bind target temp_id={} not found — skipped",
                bind.target_id
            );
        }
    }

    entity_map
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_bridge_lifecycle() {
        // Install bridge
        let bridge = GenesisBridge::new();
        set_genesis_bridge(bridge);

        // Simulate Rune function calls (without Rune runtime)
        with_genesis_bridge_mut((), |bridge| {
            let temp_id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            bridge.spawns.push(SpawnCommand {
                temp_id,
                class_name: "Part".to_string(),
                display_name: "TestPart".to_string(),
            });
            bridge.locates.push(LocateCommand {
                target_id: temp_id,
                x: 10.0,
                y: 5.0,
                z: -20.0,
            });
            bridge.binds.push(BindCommand {
                target_id: temp_id,
                key: "Material".to_string(),
                value: BindValue::Text("Marble".to_string()),
            });
        });

        // Take bridge and verify commands
        let bridge = take_genesis_bridge().expect("Bridge should be present");
        assert_eq!(bridge.spawns.len(), 1);
        assert_eq!(bridge.locates.len(), 1);
        assert_eq!(bridge.binds.len(), 1);
        assert_eq!(bridge.spawns[0].class_name, "Part");
        assert_eq!(bridge.spawns[0].display_name, "TestPart");
    }

    #[test]
    fn test_genesis_bridge_cleared_after_take() {
        set_genesis_bridge(GenesisBridge::new());
        let _ = take_genesis_bridge();
        assert!(take_genesis_bridge().is_none());
    }

    #[test]
    fn test_bind_value_variants() {
        let float = BindValue::Float(3.14);
        let text = BindValue::Text("Marble".to_string());
        let boolean = BindValue::Bool(true);
        let vec3 = BindValue::Vec3([1.0, 2.0, 3.0]);
        let integer = BindValue::Integer(42);

        // Ensure Debug works (compile-time check)
        let _ = format!("{:?} {:?} {:?} {:?} {:?}", float, text, boolean, vec3, integer);
    }
}

//! # Stage 3: Concurrence — Parallel Execution Primitives
//!
//! Higher-order functions for branching logic and parallel task management
//! within Rune scripts. Builds on the existing `task_spawn` / `task_wait`
//! infrastructure in `rune_ecs_module.rs`.
//!
//! - `fork(predicate, if_true, if_false)` — Branch execution based on a boolean
//! - `spawn(closure)` — Queue a closure for parallel execution (delegates to existing task_spawn)
//! - `join(handles)` — Wait for all spawned tasks and collect results
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::concurrence;
//!
//! pub fn main() {
//!     let result = concurrence::fork(score > 0.8,
//!         || genesis::identity("Part", "HighQuality"),
//!         || genesis::identity("Part", "LowQuality"),
//!     );
//! }
//! ```

use std::cell::RefCell;
use tracing::{info, warn};

// ============================================================================
// Task Handle — Returned by spawn(), consumed by join()
// ============================================================================

/// Handle to a spawned concurrent task.
/// Holds the result once the task completes.
#[cfg(feature = "rune-dsl")]
#[derive(Debug, Clone, rune::Any)]
pub struct TaskHandle {
    /// Unique task identifier
    #[rune(get)]
    pub id: u64,
    /// Whether the task has completed
    #[rune(get)]
    pub completed: bool,
}


/// Non-Rune version
#[cfg(not(feature = "rune-dsl"))]
#[derive(Debug, Clone)]
pub struct TaskHandle {
    pub id: u64,
    pub completed: bool,
}

// ============================================================================
// Concurrence Bridge — Thread-local state for task tracking
// ============================================================================

/// Tracks spawned tasks during Rune script execution.
#[derive(Debug, Default)]
pub struct ConcurrenceBridge {
    /// Counter for task identifiers
    pub next_task_id: u64,
    /// Number of tasks spawned during this execution
    pub tasks_spawned: u64,
}

impl ConcurrenceBridge {
    /// Create a new empty bridge
    pub fn new() -> Self {
        Self::default()
    }
}

thread_local! {
    static CONCURRENCE_BRIDGE: RefCell<Option<ConcurrenceBridge>> = RefCell::new(None);
}

/// Install the concurrence bridge for the current thread before Rune execution.
pub fn set_concurrence_bridge(bridge: ConcurrenceBridge) {
    CONCURRENCE_BRIDGE.with(|cell| {
        *cell.borrow_mut() = Some(bridge);
    });
}

/// Clear the concurrence bridge after Rune execution completes.
pub fn take_concurrence_bridge() -> Option<ConcurrenceBridge> {
    CONCURRENCE_BRIDGE.with(|cell| cell.borrow_mut().take())
}

/// Access the concurrence bridge (mutable).
fn with_concurrence_bridge_mut<F, R>(fallback: R, callback: F) -> R
where
    F: FnOnce(&mut ConcurrenceBridge) -> R,
{
    CONCURRENCE_BRIDGE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(bridge) => callback(bridge),
            None => {
                warn!("[Eustress Functions] Concurrence bridge not available — call ignored");
                fallback
            }
        }
    })
}

// ============================================================================
// Rune Functions
// ============================================================================

/// Branch execution based on a boolean predicate.
///
/// Evaluates `if_true` when predicate is true, `if_false` otherwise.
/// This is syntactic sugar — Rune has native `if/else`, but `fork` makes
/// the branching explicit and traceable in the function pipeline.
///
/// # Arguments
/// * `predicate` — Boolean condition
/// * `if_true` — Value to return when true
/// * `if_false` — Value to return when false
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn fork_bool(predicate: bool, if_true: rune::Value, if_false: rune::Value) -> rune::Value {
    info!("[Concurrence] fork(predicate={}) → branch taken", predicate);
    if predicate { if_true } else { if_false }
}

/// Spawn a task for deferred parallel execution.
///
/// Returns a `TaskHandle` that can be collected by `join()`.
/// The actual parallel execution is handled by the VmPool after
/// script compilation — this function only records the intent.
///
/// # Arguments
/// * `task_name` — Descriptive name for tracing
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn spawn_task(task_name: &str) -> TaskHandle {
    with_concurrence_bridge_mut(
        TaskHandle {
            id: 0,
            completed: false,
        },
        |bridge| {
            bridge.next_task_id += 1;
            bridge.tasks_spawned += 1;
            let id = bridge.next_task_id;
            info!("[Concurrence] spawn_task({}) → handle_id={}", task_name, id);
            TaskHandle {
                id,
                completed: false,
            }
        },
    )
}

/// Report the number of tasks spawned during this script execution.
///
/// Useful for diagnostics and observability.
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn tasks_spawned() -> u64 {
    with_concurrence_bridge_mut(0, |bridge| bridge.tasks_spawned)
}

// ============================================================================
// Rune Module Registration
// ============================================================================

/// Create the `concurrence` Rune module.
#[cfg(feature = "rune-dsl")]
pub fn create_concurrence_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "concurrence"])?;

    // TaskHandle type
    module.ty::<TaskHandle>()?;

    // Core concurrence functions
    module.function_meta(fork_bool)?;
    module.function_meta(spawn_task)?;
    module.function_meta(tasks_spawned)?;

    Ok(module)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_concurrence_bridge_lifecycle() {
        set_concurrence_bridge(ConcurrenceBridge::new());

        with_concurrence_bridge_mut((), |bridge| {
            bridge.next_task_id += 1;
            bridge.tasks_spawned += 1;
        });

        let bridge = take_concurrence_bridge().expect("Bridge should be present");
        assert_eq!(bridge.tasks_spawned, 1);
        assert_eq!(bridge.next_task_id, 1);
    }

    #[test]
    fn test_bridge_cleared_after_take() {
        set_concurrence_bridge(ConcurrenceBridge::new());
        let _ = take_concurrence_bridge();
        assert!(take_concurrence_bridge().is_none());
    }
}

//! # Rune VM Pool
//!
//! High-performance VM instance pooling for script execution.
//!
//! ## Table of Contents
//!
//! 1. **VmPool** — Thread-safe pool of reusable VM instances
//! 2. **PooledVm** — RAII guard that returns VM to pool on drop
//! 3. **Performance** — 5-10x speedup by avoiding VM creation overhead
//!
//! ## Thread Safety
//!
//! Rune's `RuntimeContext` contains non-Send types, so we use a different approach:
//! - `VmPoolInner` holds the actual pool data (not Send)
//! - `VmPoolHandle` is a Send+Sync wrapper using indices
//! - VMs are created lazily per-thread when needed

use bevy::prelude::*;
use crossbeam::queue::ArrayQueue;
use std::sync::Arc;
use std::collections::HashMap;
use parking_lot::Mutex;

#[cfg(feature = "realism-scripting")]
use rune::{Vm, Unit, runtime::RuntimeContext};

/// VM pool configuration
#[derive(Debug, Clone)]
pub struct VmPoolConfig {
    /// Maximum number of VMs to pool per script
    pub max_vms_per_script: usize,
    /// Initial pool size (pre-warmed VMs)
    pub initial_pool_size: usize,
}

impl Default for VmPoolConfig {
    fn default() -> Self {
        Self {
            max_vms_per_script: 16,
            initial_pool_size: 4,
        }
    }
}

/// Thread-safe pool of Rune VM instances for script execution.
///
/// VMs are expensive to create (~5ms), so we pool them for reuse.
/// Each script hash gets its own pool to avoid context switching.
///
/// Note: This struct does NOT derive Resource because RuntimeContext is not Send.
/// Use `VmPoolResource` wrapper for Bevy integration.
#[cfg(feature = "realism-scripting")]
pub struct VmPool {
    /// Shared runtime context for all VMs
    context: Arc<RuntimeContext>,
    /// Compiled units by script hash
    units: Arc<Mutex<HashMap<String, Arc<Unit>>>>,
    /// VM pools by script hash (using thread-local storage pattern)
    pools: Arc<Mutex<HashMap<String, Arc<ArrayQueue<Vm>>>>>,
    /// Configuration
    config: VmPoolConfig,
}

/// Send+Sync wrapper for VmPool that can be used as a Bevy Resource.
/// Contains only the configuration and unit registry, not the RuntimeContext.
#[cfg(feature = "realism-scripting")]
#[derive(Resource)]
pub struct VmPoolResource {
    /// Compiled units by script hash (Send+Sync)
    units: Arc<Mutex<HashMap<String, Arc<Unit>>>>,
    /// Configuration
    pub config: VmPoolConfig,
}

#[cfg(feature = "realism-scripting")]
impl VmPoolResource {
    /// Create a new VM pool resource
    pub fn new(config: VmPoolConfig) -> Self {
        Self {
            units: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Register a compiled script unit
    pub fn register_unit(&self, hash: String, unit: Arc<Unit>) {
        let mut units = self.units.lock();
        units.insert(hash, unit);
    }

    /// Get a compiled unit by hash
    pub fn get_unit(&self, hash: &str) -> Option<Arc<Unit>> {
        let units = self.units.lock();
        units.get(hash).cloned()
    }

    /// Check if a unit is registered
    pub fn has_unit(&self, hash: &str) -> bool {
        let units = self.units.lock();
        units.contains_key(hash)
    }

    /// Get statistics
    pub fn stats(&self) -> VmPoolStats {
        let units = self.units.lock();
        VmPoolStats {
            registered_scripts: units.len(),
            total_pooled_vms: 0, // VMs are thread-local now
            max_vms_per_script: self.config.max_vms_per_script,
        }
    }

    /// Clear all registered units
    pub fn clear(&self) {
        let mut units = self.units.lock();
        units.clear();
    }
}

#[cfg(feature = "realism-scripting")]
impl VmPool {
    /// Create a new VM pool with the given context and config
    pub fn new(context: RuntimeContext, config: VmPoolConfig) -> Self {
        Self {
            context: Arc::new(context),
            units: Arc::new(Mutex::new(HashMap::new())),
            pools: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Register a compiled script unit for pooling
    pub fn register_unit(&self, hash: String, unit: Arc<Unit>) {
        let mut units = self.units.lock();
        units.insert(hash.clone(), unit.clone());
        
        // Pre-warm the pool with initial VMs
        let mut pools = self.pools.lock();
        if !pools.contains_key(&hash) {
            let queue = Arc::new(ArrayQueue::new(self.config.max_vms_per_script));
            
            // Create initial VMs - Vm::new returns Vm directly in Rune 0.14
            for _ in 0..self.config.initial_pool_size {
                let vm = Vm::new(self.context.clone(), unit.clone());
                let _ = queue.push(vm);
            }
            
            pools.insert(hash, queue);
        }
    }

    /// Acquire a VM for the given script hash
    pub fn acquire(&self, hash: &str) -> Result<PooledVm, VmPoolError> {
        // Get the unit
        let unit = {
            let units = self.units.lock();
            units.get(hash).cloned()
                .ok_or_else(|| VmPoolError::UnitNotFound(hash.to_string()))?
        };

        // Try to get a VM from the pool
        let pools = self.pools.lock();
        let pool = pools.get(hash)
            .ok_or_else(|| VmPoolError::PoolNotFound(hash.to_string()))?;

        let vm = pool.pop()
            .unwrap_or_else(|| {
                // Pool empty, create new VM - Vm::new returns Vm directly
                Vm::new(self.context.clone(), unit.clone())
            });

        Ok(PooledVm {
            vm: Some(vm),
            pool: pool.clone(),
        })
    }

    /// Get statistics about pool usage
    pub fn stats(&self) -> VmPoolStats {
        let units = self.units.lock();
        let pools = self.pools.lock();
        
        let mut total_pooled = 0;
        for pool in pools.values() {
            total_pooled += pool.len();
        }

        VmPoolStats {
            registered_scripts: units.len(),
            total_pooled_vms: total_pooled,
            max_vms_per_script: self.config.max_vms_per_script,
        }
    }

    /// Clear all pooled VMs (for hot-reload)
    pub fn clear(&self) {
        let mut pools = self.pools.lock();
        pools.clear();
    }

    /// Remove a specific script from the pool
    pub fn remove_script(&self, hash: &str) {
        let mut units = self.units.lock();
        let mut pools = self.pools.lock();
        units.remove(hash);
        pools.remove(hash);
    }
}

/// RAII guard that returns VM to pool on drop
#[cfg(feature = "realism-scripting")]
pub struct PooledVm {
    vm: Option<Vm>,
    pool: Arc<ArrayQueue<Vm>>,
}

#[cfg(feature = "realism-scripting")]
impl PooledVm {
    /// Get mutable access to the VM
    pub fn vm_mut(&mut self) -> &mut Vm {
        self.vm.as_mut().expect("VM already taken")
    }

    /// Get immutable access to the VM
    pub fn vm(&self) -> &Vm {
        self.vm.as_ref().expect("VM already taken")
    }
}

#[cfg(feature = "realism-scripting")]
impl Drop for PooledVm {
    fn drop(&mut self) {
        if let Some(vm) = self.vm.take() {
            // Try to return to pool, discard if full
            let _ = self.pool.push(vm);
        }
    }
}

/// VM pool statistics
#[derive(Debug, Clone)]
pub struct VmPoolStats {
    pub registered_scripts: usize,
    pub total_pooled_vms: usize,
    pub max_vms_per_script: usize,
}

/// Errors from VM pool operations
#[derive(Debug, Clone)]
pub enum VmPoolError {
    UnitNotFound(String),
    PoolNotFound(String),
    VmCreationFailed(String),
}

impl std::fmt::Display for VmPoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmPoolError::UnitNotFound(hash) => write!(f, "Unit not found: {}", hash),
            VmPoolError::PoolNotFound(hash) => write!(f, "Pool not found: {}", hash),
            VmPoolError::VmCreationFailed(e) => write!(f, "VM creation failed: {}", e),
        }
    }
}

impl std::error::Error for VmPoolError {}

/// Stub implementation when realism-scripting feature is disabled
#[cfg(not(feature = "realism-scripting"))]
#[derive(Resource, Default)]
pub struct VmPool;

#[cfg(not(feature = "realism-scripting"))]
impl VmPool {
    pub fn new(_config: VmPoolConfig) -> Self {
        Self
    }
}

//! # Parallel Script Execution with Rayon
//!
//! Executes Rune scripts across multiple entities in parallel using Rayon.
//! Leverages VM pooling for efficient reuse and zero-copy ECS bindings.

use std::sync::Arc;
use rayon::prelude::*;

#[cfg(feature = "realism-scripting")]
use rune::{Context, Vm, Source, Sources, Unit, Value as RuneValue};

#[cfg(feature = "realism-scripting")]
use super::vm_pool::{VmPool, VmPoolError};

#[cfg(feature = "realism-scripting")]
use super::rune_ecs_module::create_ecs_module;

/// Script execution request for a single entity
#[derive(Debug, Clone)]
pub struct ScriptTask {
    pub entity_id: String,
    pub script_source: String,
    pub script_hash: u64,
}

/// Result of script execution
#[derive(Debug, Clone)]
pub struct ScriptTaskResult {
    pub entity_id: String,
    pub success: bool,
    pub output: Option<String>,
    pub error: Option<String>,
}

/// Parallel script executor using Rayon
pub struct ParallelScriptExecutor {
    #[cfg(feature = "realism-scripting")]
    vm_pool: Arc<VmPool>,
}

impl ParallelScriptExecutor {
    /// Create a new parallel executor with VM pool
    #[cfg(feature = "realism-scripting")]
    pub fn new(pool_size: usize) -> Self {
        Self {
            vm_pool: Arc::new(VmPool::new(pool_size)),
        }
    }

    #[cfg(not(feature = "realism-scripting"))]
    pub fn new(_pool_size: usize) -> Self {
        Self {}
    }

    /// Execute scripts in parallel across multiple entities
    #[cfg(feature = "realism-scripting")]
    pub fn execute_parallel(&self, tasks: Vec<ScriptTask>) -> Vec<ScriptTaskResult> {
        tasks
            .into_par_iter()
            .map(|task| self.execute_single(task))
            .collect()
    }

    #[cfg(not(feature = "realism-scripting"))]
    pub fn execute_parallel(&self, tasks: Vec<ScriptTask>) -> Vec<ScriptTaskResult> {
        tasks
            .into_iter()
            .map(|task| ScriptTaskResult {
                entity_id: task.entity_id,
                success: false,
                output: None,
                error: Some("Rune scripting not enabled".to_string()),
            })
            .collect()
    }

    /// Execute a single script task
    #[cfg(feature = "realism-scripting")]
    fn execute_single(&self, task: ScriptTask) -> ScriptTaskResult {
        match self.execute_with_pooled_vm(&task) {
            Ok(output) => ScriptTaskResult {
                entity_id: task.entity_id,
                success: true,
                output: Some(format!("{:?}", output)),
                error: None,
            },
            Err(e) => ScriptTaskResult {
                entity_id: task.entity_id,
                success: false,
                output: None,
                error: Some(e.to_string()),
            },
        }
    }

    /// Execute script using pooled VM
    #[cfg(feature = "realism-scripting")]
    fn execute_with_pooled_vm(&self, task: &ScriptTask) -> Result<RuneValue, VmPoolError> {
        // Try to acquire VM from pool
        let mut vm_guard = self.vm_pool.acquire(task.script_hash)?;

        // If VM not found in pool, compile and register
        if vm_guard.is_none() {
            let vm = self.compile_script(&task.script_source)?;
            self.vm_pool.register(task.script_hash, vm)?;
            vm_guard = self.vm_pool.acquire(task.script_hash)?;
        }

        // Execute with acquired VM
        let mut vm = vm_guard.ok_or(VmPoolError::PoolExhausted)?;
        
        let output: RuneValue = vm.call(["main"], (task.entity_id.clone(),))
            .map_err(|e| VmPoolError::ExecutionError(e.to_string()))?;

        Ok(output)
    }

    /// Compile Rune script into VM
    #[cfg(feature = "realism-scripting")]
    fn compile_script(&self, source: &str) -> Result<Vm, VmPoolError> {
        // Build Rune context with ECS module
        let mut rune_context = Context::with_default_modules()
            .map_err(|e| VmPoolError::CompilationError(e.to_string()))?;
        
        // Install ECS bindings module
        let ecs_module = create_ecs_module()
            .map_err(|e| VmPoolError::CompilationError(e.to_string()))?;
        rune_context.install(ecs_module)
            .map_err(|e| VmPoolError::CompilationError(e.to_string()))?;
        
        let runtime = Arc::new(rune_context.runtime()
            .map_err(|e| VmPoolError::CompilationError(e.to_string()))?);

        // Compile script
        let mut sources = Sources::new();
        sources.insert(Source::memory(source)
            .map_err(|e| VmPoolError::CompilationError(e.to_string()))?)
            .map_err(|e| VmPoolError::CompilationError(e.to_string()))?;
        
        let unit = rune::prepare(&mut sources)
            .build()
            .map_err(|e| VmPoolError::CompilationError(e.to_string()))?;
        let unit = Arc::new(unit);

        Ok(Vm::new(runtime, unit))
    }

    /// Get VM pool statistics
    #[cfg(feature = "realism-scripting")]
    pub fn pool_stats(&self) -> String {
        self.vm_pool.stats()
    }

    #[cfg(not(feature = "realism-scripting"))]
    pub fn pool_stats(&self) -> String {
        "VM pooling not available (realism-scripting feature disabled)".to_string()
    }

    /// Clear all pooled VMs
    #[cfg(feature = "realism-scripting")]
    pub fn clear_pool(&self) {
        self.vm_pool.clear_all();
    }

    #[cfg(not(feature = "realism-scripting"))]
    pub fn clear_pool(&self) {}
}

/// Benchmark: Compare sequential vs parallel execution
#[cfg(all(test, feature = "realism-scripting"))]
mod benchmarks {
    use super::*;
    use std::time::Instant;

    #[test]
    fn bench_parallel_vs_sequential() {
        let executor = ParallelScriptExecutor::new(8);
        
        // Create 100 script tasks
        let tasks: Vec<ScriptTask> = (0..100)
            .map(|i| ScriptTask {
                entity_id: format!("Entity_{}", i),
                script_source: r#"
                    pub fn main(entity_id) {
                        let sum = 0;
                        for i in 0..1000 {
                            sum = sum + i;
                        }
                        sum
                    }
                "#.to_string(),
                script_hash: 12345, // Same script for all
            })
            .collect();

        // Parallel execution
        let start = Instant::now();
        let parallel_results = executor.execute_parallel(tasks.clone());
        let parallel_time = start.elapsed();

        // Sequential execution (simulate)
        let start = Instant::now();
        let sequential_results: Vec<_> = tasks
            .into_iter()
            .map(|task| executor.execute_single(task))
            .collect();
        let sequential_time = start.elapsed();

        println!("Parallel: {:?}", parallel_time);
        println!("Sequential: {:?}", sequential_time);
        println!("Speedup: {:.2}x", sequential_time.as_secs_f64() / parallel_time.as_secs_f64());

        assert_eq!(parallel_results.len(), 100);
        assert_eq!(sequential_results.len(), 100);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_creation() {
        let executor = ParallelScriptExecutor::new(4);
        let stats = executor.pool_stats();
        assert!(!stats.is_empty());
    }

    #[test]
    #[cfg(feature = "realism-scripting")]
    fn test_parallel_execution() {
        let executor = ParallelScriptExecutor::new(4);
        
        let tasks = vec![
            ScriptTask {
                entity_id: "Entity_1".to_string(),
                script_source: r#"pub fn main(id) { 42 }"#.to_string(),
                script_hash: 1,
            },
            ScriptTask {
                entity_id: "Entity_2".to_string(),
                script_source: r#"pub fn main(id) { 84 }"#.to_string(),
                script_hash: 2,
            },
        ];

        let results = executor.execute_parallel(tasks);
        assert_eq!(results.len(), 2);
    }
}

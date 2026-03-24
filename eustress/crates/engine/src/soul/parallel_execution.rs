//! # Parallel Script Execution with Rayon
//!
//! Executes Rune scripts across multiple entities in parallel using Rayon.
//! Leverages VM pooling for efficient reuse and zero-copy ECS bindings.

use std::sync::Arc;
use rayon::prelude::*;

#[cfg(feature = "realism-scripting")]
use rune::{Context, Vm, Source, Sources, Value as RuneValue};

#[cfg(feature = "realism-scripting")]
use super::vm_pool::{VmPool, VmPoolConfig, VmPoolError};

#[cfg(feature = "realism-scripting")]
use super::rune_ecs_module::create_ecs_module;

/// Script execution request for a single entity
#[derive(Debug, Clone)]
pub struct ScriptTask {
    pub entity_id: String,
    pub script_source: String,
    pub script_hash: String,
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
    pub fn new(_pool_size: usize) -> Result<Self, String> {
        // Build Rune context with ECS module
        let mut rune_context = Context::with_default_modules()
            .map_err(|e| e.to_string())?;
        
        // Install ECS bindings module
        let ecs_module = create_ecs_module()
            .map_err(|e| e.to_string())?;
        rune_context.install(ecs_module)
            .map_err(|e| e.to_string())?;
        
        let runtime = rune_context.runtime()
            .map_err(|e| e.to_string())?;

        let config = VmPoolConfig::default();
        Ok(Self {
            vm_pool: Arc::new(VmPool::new(runtime, config)),
        })
    }

    #[cfg(not(feature = "realism-scripting"))]
    pub fn new(_pool_size: usize) -> Result<Self, String> {
        Ok(Self {})
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
                error: Some(e),
            },
        }
    }

    /// Execute script using pooled VM
    #[cfg(feature = "realism-scripting")]
    fn execute_with_pooled_vm(&self, task: &ScriptTask) -> Result<RuneValue, String> {
        // Try to acquire VM from pool
        let mut vm_guard = self.vm_pool.acquire(&task.script_hash)
            .map_err(|e| e.to_string())?;

        // Execute with acquired VM
        let vm = vm_guard.vm_mut();
        let output: RuneValue = vm.call(["main"], (task.entity_id.clone(),))
            .into_result()
            .map_err(|e| e.to_string())?;

        Ok(output)
    }

    /// Register a compiled script unit for pooling
    #[cfg(feature = "realism-scripting")]
    pub fn register_script(&self, hash: String, source: &str) -> Result<(), String> {
        // Compile script
        let mut sources = Sources::new();
        sources.insert(Source::memory(source).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        
        let unit = rune::prepare(&mut sources)
            .build()
            .map_err(|e| e.to_string())?;
        let unit = Arc::new(unit);

        self.vm_pool.register_unit(hash, unit);
        Ok(())
    }

    /// Get VM pool statistics
    #[cfg(feature = "realism-scripting")]
    pub fn pool_stats(&self) -> String {
        let stats = self.vm_pool.stats();
        format!("Scripts: {}, Pooled VMs: {}, Max per script: {}", 
            stats.registered_scripts, stats.total_pooled_vms, stats.max_vms_per_script)
    }

    #[cfg(not(feature = "realism-scripting"))]
    pub fn pool_stats(&self) -> String {
        "VM pooling not available (realism-scripting feature disabled)".to_string()
    }

    /// Clear all pooled VMs
    #[cfg(feature = "realism-scripting")]
    pub fn clear_pool(&self) {
        self.vm_pool.clear();
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
        let executor = ParallelScriptExecutor::new(8).expect("Failed to create executor");
        
        let script_source = r#"
            pub fn main(entity_id) {
                let sum = 0;
                for i in 0..1000 {
                    sum = sum + i;
                }
                sum
            }
        "#;
        
        // Register script once
        executor.register_script("bench_script".to_string(), script_source)
            .expect("Failed to register script");
        
        // Create 100 script tasks
        let tasks: Vec<ScriptTask> = (0..100)
            .map(|i| ScriptTask {
                entity_id: format!("Entity_{}", i),
                script_source: script_source.to_string(),
                script_hash: "bench_script".to_string(),
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
        assert!(executor.is_ok());
        let stats = executor.unwrap().pool_stats();
        assert!(!stats.is_empty());
    }

    #[test]
    #[cfg(feature = "realism-scripting")]
    fn test_parallel_execution() {
        let executor = ParallelScriptExecutor::new(4).expect("Failed to create executor");
        
        let script1 = r#"pub fn main(id) { 42 }"#;
        let script2 = r#"pub fn main(id) { 84 }"#;
        
        executor.register_script("script1".to_string(), script1).unwrap();
        executor.register_script("script2".to_string(), script2).unwrap();
        
        let tasks = vec![
            ScriptTask {
                entity_id: "Entity_1".to_string(),
                script_source: script1.to_string(),
                script_hash: "script1".to_string(),
            },
            ScriptTask {
                entity_id: "Entity_2".to_string(),
                script_source: script2.to_string(),
                script_hash: "script2".to_string(),
            },
        ];

        let results = executor.execute_parallel(tasks);
        assert_eq!(results.len(), 2);
    }
}

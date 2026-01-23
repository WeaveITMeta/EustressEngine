//! Scheduler Algorithm Benchmarks
//!
//! Comprehensive benchmarks comparing:
//! - Bin-packing vs Spread vs Learned scheduling
//! - Scheduling throughput (decisions/second)
//! - Resource utilization efficiency
//! - Learning convergence speed

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use forge_orchestration::scheduler::{
    Scheduler, BinPackScheduler, SpreadScheduler, GpuLocalityScheduler,
    algorithms::{LearnedScheduler, SchedulingAlgorithm, SchedulingFeedback},
    NodeResources, Workload, ResourceRequirements,
};
use forge_orchestration::types::{NodeId, GpuResources};
use std::time::Instant;

/// Create a test cluster with N nodes
fn create_cluster(node_count: usize, gpus_per_node: usize) -> Vec<NodeResources> {
    (0..node_count)
        .map(|i| {
            let mut node = NodeResources::new(
                NodeId::new(),
                8000,  // 8 CPU cores
                32768, // 32GB RAM
            );
            
            // Add GPUs
            for g in 0..gpus_per_node {
                node = node.with_gpu(
                    GpuResources::new(g as u32, "NVIDIA A100", 40960)
                        .with_tensor_cores(true)
                        .with_compute_capability(8.0)
                );
            }
            
            // Add labels
            node = node
                .with_label("zone", format!("zone-{}", i % 3))
                .with_label("tier", if i % 2 == 0 { "high" } else { "standard" });
            
            // Pre-allocate some resources to simulate realistic cluster
            node.cpu_allocated = (i as u64 * 500) % 4000;
            node.memory_allocated = (i as u64 * 2048) % 16384;
            
            node
        })
        .collect()
}

/// Create test workloads
fn create_workloads(count: usize) -> Vec<Workload> {
    (0..count)
        .map(|i| {
            let cpu = 100 + (i as u64 % 10) * 100;
            let memory = 256 + (i as u64 % 8) * 256;
            let gpu = if i % 5 == 0 { 1 } else { 0 };
            
            Workload::new(format!("workload-{}", i), format!("test-{}", i))
                .with_resources(
                    ResourceRequirements::new()
                        .cpu(cpu)
                        .memory(memory)
                        .gpu(gpu, 8192)
                )
                .with_priority((i % 100) as i32)
        })
        .collect()
}

/// Benchmark scheduling throughput
fn bench_scheduling_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("scheduling_throughput");
    
    for node_count in [10, 50, 100, 500].iter() {
        let nodes = create_cluster(*node_count, 2);
        let workloads = create_workloads(1000);
        
        // Bin-pack scheduler
        group.bench_with_input(
            BenchmarkId::new("bin_pack", node_count),
            node_count,
            |b, _| {
                let scheduler = Scheduler::with_algorithm(BinPackScheduler::new());
                for node in &nodes {
                    scheduler.register_node(node.clone());
                }
                
                b.iter(|| {
                    for workload in &workloads[..100] {
                        black_box(scheduler.schedule(workload));
                    }
                });
            },
        );
        
        // Spread scheduler
        group.bench_with_input(
            BenchmarkId::new("spread", node_count),
            node_count,
            |b, _| {
                let scheduler = Scheduler::with_algorithm(SpreadScheduler::new());
                for node in &nodes {
                    scheduler.register_node(node.clone());
                }
                
                b.iter(|| {
                    for workload in &workloads[..100] {
                        black_box(scheduler.schedule(workload));
                    }
                });
            },
        );
        
        // Learned scheduler
        group.bench_with_input(
            BenchmarkId::new("learned", node_count),
            node_count,
            |b, _| {
                let scheduler = Scheduler::with_algorithm(LearnedScheduler::new());
                for node in &nodes {
                    scheduler.register_node(node.clone());
                }
                
                b.iter(|| {
                    for workload in &workloads[..100] {
                        black_box(scheduler.schedule(workload));
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark resource utilization efficiency
fn bench_utilization_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("utilization_efficiency");
    
    let nodes = create_cluster(50, 2);
    let workloads = create_workloads(500);
    
    group.bench_function("bin_pack_utilization", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = std::time::Duration::ZERO;
            
            for _ in 0..iters {
                let scheduler = Scheduler::with_algorithm(BinPackScheduler::new());
                for node in &nodes {
                    scheduler.register_node(node.clone());
                }
                
                let start = Instant::now();
                let mut scheduled = 0;
                
                for workload in &workloads {
                    let decision = scheduler.schedule(workload);
                    if decision.node_id.is_some() {
                        scheduled += 1;
                    }
                }
                
                total_duration += start.elapsed();
                black_box(scheduled);
            }
            
            total_duration
        });
    });
    
    group.bench_function("spread_utilization", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = std::time::Duration::ZERO;
            
            for _ in 0..iters {
                let scheduler = Scheduler::with_algorithm(SpreadScheduler::new());
                for node in &nodes {
                    scheduler.register_node(node.clone());
                }
                
                let start = Instant::now();
                let mut scheduled = 0;
                
                for workload in &workloads {
                    let decision = scheduler.schedule(workload);
                    if decision.node_id.is_some() {
                        scheduled += 1;
                    }
                }
                
                total_duration += start.elapsed();
                black_box(scheduled);
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark GPU-aware scheduling
fn bench_gpu_scheduling(c: &mut Criterion) {
    let mut group = c.benchmark_group("gpu_scheduling");
    
    let nodes = create_cluster(20, 8); // 20 nodes with 8 GPUs each
    
    // GPU-heavy workloads
    let gpu_workloads: Vec<_> = (0..100)
        .map(|i| {
            Workload::new(format!("gpu-workload-{}", i), "ml-training")
                .with_resources(
                    ResourceRequirements::new()
                        .cpu(2000)
                        .memory(16384)
                        .gpu(1 + (i % 4) as u32, 20480)
                )
        })
        .collect();
    
    group.bench_function("gpu_locality_scheduler", |b| {
        let scheduler = Scheduler::with_algorithm(GpuLocalityScheduler::new());
        for node in &nodes {
            scheduler.register_node(node.clone());
        }
        
        b.iter(|| {
            for workload in &gpu_workloads {
                black_box(scheduler.schedule(workload));
            }
        });
    });
    
    group.bench_function("bin_pack_gpu", |b| {
        let scheduler = Scheduler::with_algorithm(BinPackScheduler::new());
        for node in &nodes {
            scheduler.register_node(node.clone());
        }
        
        b.iter(|| {
            for workload in &gpu_workloads {
                black_box(scheduler.schedule(workload));
            }
        });
    });
    
    group.finish();
}

/// Benchmark learned scheduler convergence
fn bench_learned_convergence(c: &mut Criterion) {
    let mut group = c.benchmark_group("learned_convergence");
    group.sample_size(10); // Fewer samples for longer benchmarks
    
    group.bench_function("learning_1000_iterations", |b| {
        b.iter_custom(|iters| {
            let mut total_duration = std::time::Duration::ZERO;
            
            for _ in 0..iters {
                let scheduler = LearnedScheduler::new();
                let nodes = create_cluster(20, 2);
                let workloads = create_workloads(100);
                
                let start = Instant::now();
                
                // Simulate 1000 scheduling decisions with feedback
                for i in 0..1000 {
                    let workload = &workloads[i % workloads.len()];
                    let node = &nodes[i % nodes.len()];
                    
                    // Score and record feedback
                    let _score = scheduler.score(workload, node);
                    
                    // Simulate performance feedback (random for benchmark)
                    let performance = if i % 3 == 0 { 0.9 } else { 0.5 };
                    let features = vec![0.5, 0.5, 0.0, 0.25, 0.25, 0.0, 0.0, 1.0];
                    
                    scheduler.record_feedback(
                        forge_orchestration::scheduler::algorithms::SchedulingFeedback {
                            features,
                            performance,
                        }
                    );
                }
                
                total_duration += start.elapsed();
                black_box(scheduler.weights());
            }
            
            total_duration
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_scheduling_throughput,
    bench_utilization_efficiency,
    bench_gpu_scheduling,
    bench_learned_convergence,
);

criterion_main!(benches);

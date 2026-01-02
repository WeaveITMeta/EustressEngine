//! # Stress Test - 100,000 Entities
//!
//! Benchmarks the networking system with massive entity counts.
//!
//! ## Run
//!
//! ```bash
//! cargo bench --package eustress-networking --bench stress_test
//! ```
//!
//! ## Metrics
//!
//! - Entity spawn rate
//! - Replication update throughput
//! - AOI query performance
//! - Memory usage

use bevy::prelude::*;
use std::time::{Duration, Instant};

// Target: 100,000 entities
const TARGET_ENTITIES: usize = 100_000;

// Batch size for spawning
const SPAWN_BATCH: usize = 1000;

/// Simple networked entity for stress testing
#[derive(Component, Clone)]
struct StressEntity {
    id: u32,
    position: Vec3,
    velocity: Vec3,
}

/// Spatial grid for AOI testing
struct SpatialGrid {
    cell_size: f32,
    cells: std::collections::HashMap<(i32, i32, i32), Vec<u32>>,
}

impl SpatialGrid {
    fn new(cell_size: f32) -> Self {
        Self {
            cell_size,
            cells: std::collections::HashMap::new(),
        }
    }
    
    fn cell_key(&self, pos: Vec3) -> (i32, i32, i32) {
        (
            (pos.x / self.cell_size).floor() as i32,
            (pos.y / self.cell_size).floor() as i32,
            (pos.z / self.cell_size).floor() as i32,
        )
    }
    
    fn insert(&mut self, id: u32, pos: Vec3) {
        let key = self.cell_key(pos);
        self.cells.entry(key).or_default().push(id);
    }
    
    fn query_radius(&self, center: Vec3, radius: f32) -> Vec<u32> {
        let mut results = Vec::new();
        let cells_to_check = (radius / self.cell_size).ceil() as i32;
        let center_key = self.cell_key(center);
        
        for dx in -cells_to_check..=cells_to_check {
            for dy in -cells_to_check..=cells_to_check {
                for dz in -cells_to_check..=cells_to_check {
                    let key = (
                        center_key.0 + dx,
                        center_key.1 + dy,
                        center_key.2 + dz,
                    );
                    if let Some(entities) = self.cells.get(&key) {
                        results.extend(entities.iter().copied());
                    }
                }
            }
        }
        
        results
    }
    
    fn clear(&mut self) {
        self.cells.clear();
    }
}

fn main() {
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║         Eustress Networking Stress Test                    ║");
    println!("║         Target: {} entities                          ║", TARGET_ENTITIES);
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    
    // Test 1: Entity Spawning
    println!("📊 Test 1: Entity Spawning");
    let (entities, spawn_time) = test_entity_spawning();
    println!("   ✓ Spawned {} entities in {:.2}ms", entities.len(), spawn_time.as_secs_f64() * 1000.0);
    println!("   ✓ Rate: {:.0} entities/sec", entities.len() as f64 / spawn_time.as_secs_f64());
    println!();
    
    // Test 2: Spatial Grid Insertion
    println!("📊 Test 2: Spatial Grid Insertion");
    let (grid, insert_time) = test_spatial_insertion(&entities);
    println!("   ✓ Inserted {} entities in {:.2}ms", entities.len(), insert_time.as_secs_f64() * 1000.0);
    println!("   ✓ Rate: {:.0} inserts/sec", entities.len() as f64 / insert_time.as_secs_f64());
    println!();
    
    // Test 3: AOI Queries
    println!("📊 Test 3: AOI Queries (radius=100 studs)");
    let (query_count, avg_results, query_time) = test_aoi_queries(&grid, &entities);
    println!("   ✓ Performed {} queries in {:.2}ms", query_count, query_time.as_secs_f64() * 1000.0);
    println!("   ✓ Rate: {:.0} queries/sec", query_count as f64 / query_time.as_secs_f64());
    println!("   ✓ Avg results per query: {:.1}", avg_results);
    println!();
    
    // Test 4: Position Updates
    println!("📊 Test 4: Position Updates (simulating 1 tick)");
    let update_time = test_position_updates(&entities);
    println!("   ✓ Updated {} positions in {:.2}ms", entities.len(), update_time.as_secs_f64() * 1000.0);
    println!("   ✓ Rate: {:.0} updates/sec", entities.len() as f64 / update_time.as_secs_f64());
    println!();
    
    // Test 5: Delta Compression Simulation
    println!("📊 Test 5: Delta Compression");
    let (changed, unchanged, delta_time) = test_delta_compression(&entities);
    println!("   ✓ Processed {} entities in {:.2}ms", entities.len(), delta_time.as_secs_f64() * 1000.0);
    println!("   ✓ Changed: {}, Unchanged: {}", changed, unchanged);
    println!("   ✓ Compression ratio: {:.1}%", (unchanged as f64 / entities.len() as f64) * 100.0);
    println!();
    
    // Test 6: Memory Usage
    println!("📊 Test 6: Memory Estimation");
    let entity_size = std::mem::size_of::<StressEntity>();
    let total_entity_mem = entity_size * entities.len();
    let grid_overhead = grid.cells.len() * (std::mem::size_of::<(i32, i32, i32)>() + std::mem::size_of::<Vec<u32>>());
    println!("   ✓ Entity size: {} bytes", entity_size);
    println!("   ✓ Total entity memory: {:.2} MB", total_entity_mem as f64 / (1024.0 * 1024.0));
    println!("   ✓ Spatial grid cells: {}", grid.cells.len());
    println!("   ✓ Grid overhead: {:.2} KB", grid_overhead as f64 / 1024.0);
    println!();
    
    // Summary
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║                      SUMMARY                               ║");
    println!("╠════════════════════════════════════════════════════════════╣");
    
    let tick_budget_ms = 1000.0 / 120.0; // 120 Hz = 8.33ms per tick
    let total_tick_time = update_time.as_secs_f64() * 1000.0;
    let tick_utilization = (total_tick_time / tick_budget_ms) * 100.0;
    
    println!("║  Tick budget (120 Hz): {:.2}ms                            ║", tick_budget_ms);
    println!("║  Update time: {:.2}ms                                     ║", total_tick_time);
    println!("║  Tick utilization: {:.1}%                                 ║", tick_utilization);
    
    if tick_utilization < 50.0 {
        println!("║  Status: ✅ EXCELLENT - Plenty of headroom               ║");
    } else if tick_utilization < 80.0 {
        println!("║  Status: ✓ GOOD - Acceptable performance                 ║");
    } else if tick_utilization < 100.0 {
        println!("║  Status: ⚠ WARNING - Near budget limit                   ║");
    } else {
        println!("║  Status: ❌ OVER BUDGET - Optimization needed            ║");
    }
    
    println!("╚════════════════════════════════════════════════════════════╝");
}

fn test_entity_spawning() -> (Vec<StressEntity>, Duration) {
    let start = Instant::now();
    
    let mut entities = Vec::with_capacity(TARGET_ENTITIES);
    
    for i in 0..TARGET_ENTITIES {
        // Distribute entities in a 1000x100x1000 stud volume
        let x = (i % 1000) as f32 - 500.0;
        let y = ((i / 1000) % 100) as f32;
        let z = (i / 100000) as f32 * 10.0 - 500.0;
        
        entities.push(StressEntity {
            id: i as u32,
            position: Vec3::new(x, y, z),
            velocity: Vec3::new(
                (i as f32 * 0.1).sin(),
                0.0,
                (i as f32 * 0.1).cos(),
            ),
        });
    }
    
    (entities, start.elapsed())
}

fn test_spatial_insertion(entities: &[StressEntity]) -> (SpatialGrid, Duration) {
    let start = Instant::now();
    
    let mut grid = SpatialGrid::new(50.0); // 50 stud cells
    
    for entity in entities {
        grid.insert(entity.id, entity.position);
    }
    
    (grid, start.elapsed())
}

fn test_aoi_queries(grid: &SpatialGrid, entities: &[StressEntity]) -> (usize, f64, Duration) {
    let start = Instant::now();
    
    let query_count = 1000;
    let mut total_results = 0;
    
    // Query from random positions
    for i in 0..query_count {
        let center = entities[i * (entities.len() / query_count)].position;
        let results = grid.query_radius(center, 100.0);
        total_results += results.len();
    }
    
    let avg_results = total_results as f64 / query_count as f64;
    
    (query_count, avg_results, start.elapsed())
}

fn test_position_updates(entities: &[StressEntity]) -> Duration {
    let start = Instant::now();
    
    let dt = 1.0 / 120.0; // 120 Hz tick
    
    // Simulate position updates (would modify in place in real code)
    let _updated: Vec<Vec3> = entities.iter()
        .map(|e| e.position + e.velocity * dt)
        .collect();
    
    start.elapsed()
}

fn test_delta_compression(entities: &[StressEntity]) -> (usize, usize, Duration) {
    let start = Instant::now();
    
    // Simulate delta detection (30% of entities changed)
    let mut changed = 0;
    let mut unchanged = 0;
    
    for (i, _entity) in entities.iter().enumerate() {
        // Simulate checking if entity changed
        if i % 3 == 0 {
            changed += 1;
        } else {
            unchanged += 1;
        }
    }
    
    (changed, unchanged, start.elapsed())
}

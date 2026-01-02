//! Level of Detail (LOD) system for terrain chunks
//!
//! When `physics` feature is enabled, colliders are updated 1:1 with visual mesh.

use bevy::prelude::*;
use super::{Chunk, TerrainConfig, TerrainData, TerrainRoot, generate_chunk_mesh};

#[cfg(feature = "physics")]
use avian3d::prelude::*;

/// Resource to track LOD update state for throttling
/// 
/// # Performance Tuning Guide
/// 
/// Adjust these values based on your terrain size and target hardware:
/// 
/// | Terrain Size | check_interval | max_updates_per_frame | camera_move_threshold |
/// |--------------|----------------|----------------------|----------------------|
/// | Small (<100 chunks) | 0.1s | 8 | 5.0 |
/// | Medium (100-500 chunks) | 0.25s | 4 | 10.0 |
/// | Large (500-2000 chunks) | 0.5s | 2 | 20.0 |
/// | Massive (2000+ chunks) | 1.0s | 1 | 50.0 |
/// 
/// **Trade-offs:**
/// - Lower `check_interval` = more responsive LOD but higher CPU usage
/// - Higher `max_updates_per_frame` = faster LOD transitions but frame spikes
/// - Lower `camera_move_threshold` = more frequent recalcs but smoother LOD
#[derive(Resource)]
pub struct LodUpdateState {
    /// Last time LOD was checked (internal, don't modify)
    pub last_check: f64,
    
    /// Interval between LOD distance checks (seconds)
    /// 
    /// **Recommended values:**
    /// - 0.1 (10 FPS): Very responsive, high CPU - use for small terrains
    /// - 0.25 (4 FPS): Balanced - good default for most terrains
    /// - 0.5 (2 FPS): Low CPU - use for large terrains or low-end hardware
    /// - 1.0 (1 FPS): Minimal CPU - use for massive terrains (2000+ chunks)
    pub check_interval: f64,
    
    /// Maximum chunks to regenerate meshes per frame
    /// 
    /// Mesh regeneration is expensive. This spreads the work across frames.
    /// 
    /// **Recommended values:**
    /// - 8: Fast LOD transitions, may cause frame spikes on large chunks
    /// - 4: Balanced - good default
    /// - 2: Smooth frames, slower LOD transitions
    /// - 1: Minimal frame impact, slowest LOD transitions
    pub max_updates_per_frame: usize,
    
    /// Queue of chunks needing LOD update (internal, don't modify)
    pub pending_updates: Vec<(Entity, u32)>,
    
    /// Last camera position for hysteresis (internal, don't modify)
    pub last_camera_pos: Vec3,
    
    /// Minimum camera movement (studs) to trigger LOD recalculation
    /// 
    /// Prevents unnecessary recalcs when camera is stationary or moving slowly.
    /// 
    /// **Recommended values:**
    /// - 5.0: Very responsive, more CPU usage
    /// - 10.0: Balanced - good default
    /// - 20.0: Less responsive, lower CPU
    /// - 50.0: Only recalc on significant movement (flying/teleporting)
    pub camera_move_threshold: f32,
}

impl Default for LodUpdateState {
    fn default() -> Self {
        Self {
            last_check: 0.0,
            check_interval: 0.25,        // 4 FPS LOD checks - balanced default
            max_updates_per_frame: 4,    // 4 chunks/frame - balanced default
            pending_updates: Vec::new(),
            last_camera_pos: Vec3::ZERO,
            camera_move_threshold: 10.0, // 10 studs - balanced default
        }
    }
}

/// System to update chunk LOD based on camera distance
/// 
/// Throttled to reduce CPU usage with massive terrains.
/// When `physics` feature is enabled, colliders are regenerated 1:1 with visual mesh.
pub fn update_lod_system(
    mut commands: Commands,
    camera_query: Query<&GlobalTransform, With<Camera3d>>,
    terrain_query: Query<(&TerrainConfig, &TerrainData), With<TerrainRoot>>,
    mut chunk_query: Query<(Entity, &mut Chunk, &GlobalTransform, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
    mut lod_state: ResMut<LodUpdateState>,
) {
    let Ok(camera_transform) = camera_query.single() else { return };
    let Ok((config, data)) = terrain_query.single() else { return };
    
    let camera_pos = camera_transform.translation();
    let current_time = time.elapsed_secs_f64();
    
    // Process any pending updates first (spread across frames)
    let mut updates_this_frame = 0;
    while !lod_state.pending_updates.is_empty() && updates_this_frame < lod_state.max_updates_per_frame {
        let (entity, new_lod) = lod_state.pending_updates.pop().unwrap();
        
        if let Ok((_, mut chunk, _, _)) = chunk_query.get_mut(entity) {
            if chunk.lod != new_lod {
                let new_mesh_handle = generate_chunk_mesh(
                    chunk.position,
                    new_lod,
                    config,
                    data,
                    &mut meshes,
                );
                
                commands.entity(entity).insert(Mesh3d(new_mesh_handle.clone()));
                
                #[cfg(feature = "physics")]
                {
                    if let Some(mesh) = meshes.get(&new_mesh_handle) {
                        if let Some(collider) = Collider::trimesh_from_mesh(mesh) {
                            commands.entity(entity).insert(collider);
                        }
                    }
                }
                
                chunk.lod = new_lod;
                updates_this_frame += 1;
            }
        }
    }
    
    // Check if we should scan for LOD changes (throttled)
    let camera_moved = camera_pos.distance(lod_state.last_camera_pos) > lod_state.camera_move_threshold;
    let time_elapsed = current_time - lod_state.last_check >= lod_state.check_interval;
    
    if !camera_moved && !time_elapsed {
        return;
    }
    
    lod_state.last_check = current_time;
    lod_state.last_camera_pos = camera_pos;
    
    // Scan chunks for LOD changes and queue updates
    for (entity, chunk, transform, _mesh) in chunk_query.iter() {
        let distance = camera_pos.distance(transform.translation());
        let new_lod = config.lod_for_distance(distance);
        
        // Queue update if LOD changed
        if new_lod != chunk.lod {
            // Prioritize closer chunks (insert at front for lower LOD = more detail)
            if new_lod < chunk.lod {
                lod_state.pending_updates.insert(0, (entity, new_lod));
            } else {
                lod_state.pending_updates.push((entity, new_lod));
            }
        }
    }
}

/// Quadtree node for hierarchical LOD (future optimization)
#[derive(Debug, Clone)]
pub struct QuadtreeNode {
    /// Bounds of this node in world space
    pub bounds: Rect,
    
    /// LOD level for this node
    pub lod: u32,
    
    /// Children (NW, NE, SW, SE) if subdivided
    pub children: Option<Box<[QuadtreeNode; 4]>>,
}

impl QuadtreeNode {
    /// Create a new quadtree node
    pub fn new(bounds: Rect, lod: u32) -> Self {
        Self {
            bounds,
            lod,
            children: None,
        }
    }
    
    /// Subdivide this node into 4 children
    pub fn subdivide(&mut self) {
        if self.children.is_some() {
            return;
        }
        
        let center = self.bounds.center();
        let half_size = self.bounds.half_size();
        let quarter_size = half_size * 0.5;
        let child_lod = self.lod.saturating_sub(1);
        
        self.children = Some(Box::new([
            // NW
            QuadtreeNode::new(
                Rect::from_center_half_size(
                    Vec2::new(center.x - quarter_size.x, center.y + quarter_size.y),
                    quarter_size,
                ),
                child_lod,
            ),
            // NE
            QuadtreeNode::new(
                Rect::from_center_half_size(
                    Vec2::new(center.x + quarter_size.x, center.y + quarter_size.y),
                    quarter_size,
                ),
                child_lod,
            ),
            // SW
            QuadtreeNode::new(
                Rect::from_center_half_size(
                    Vec2::new(center.x - quarter_size.x, center.y - quarter_size.y),
                    quarter_size,
                ),
                child_lod,
            ),
            // SE
            QuadtreeNode::new(
                Rect::from_center_half_size(
                    Vec2::new(center.x + quarter_size.x, center.y - quarter_size.y),
                    quarter_size,
                ),
                child_lod,
            ),
        ]));
    }
    
    /// Check if a point is within this node's bounds
    pub fn contains(&self, point: Vec2) -> bool {
        self.bounds.contains(point)
    }
    
    /// Get the leaf node containing a point
    pub fn get_leaf(&self, point: Vec2) -> Option<&QuadtreeNode> {
        if !self.contains(point) {
            return None;
        }
        
        if let Some(ref children) = self.children {
            for child in children.iter() {
                if let Some(leaf) = child.get_leaf(point) {
                    return Some(leaf);
                }
            }
        }
        
        Some(self)
    }
}

/// LOD transition helper for seamless blending
#[derive(Component, Default)]
pub struct LodTransition {
    /// Current blend factor (0 = current LOD, 1 = next LOD)
    pub blend: f32,
    
    /// Target LOD level
    pub target_lod: u32,
    
    /// Transition speed
    pub speed: f32,
}

impl LodTransition {
    pub fn new(target_lod: u32) -> Self {
        Self {
            blend: 0.0,
            target_lod,
            speed: 2.0,
        }
    }
    
    /// Update transition, returns true when complete
    pub fn update(&mut self, delta: f32) -> bool {
        self.blend += delta * self.speed;
        self.blend >= 1.0
    }
}

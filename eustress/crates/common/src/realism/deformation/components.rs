//! # Deformation Components
//!
//! ECS components for mesh deformation state.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// DeformableMesh Component
// ============================================================================

/// Marks an entity as having deformable mesh vertices
/// Added automatically when BasePart.deformation = true
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct DeformableMesh {
    /// Original mesh handle (undeformed reference)
    pub original_mesh: Handle<Mesh>,
    /// Deformed mesh handle (runtime modified)
    pub deformed_mesh: Handle<Mesh>,
    /// Number of vertices
    pub vertex_count: usize,
    /// Whether mesh needs GPU sync
    pub dirty: bool,
    /// Deformation quality level (affects vertex update frequency)
    pub quality: DeformationQuality,
}

impl Default for DeformableMesh {
    fn default() -> Self {
        Self {
            original_mesh: Handle::default(),
            deformed_mesh: Handle::default(),
            vertex_count: 0,
            dirty: false,
            quality: DeformationQuality::Medium,
        }
    }
}

/// Deformation quality settings
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum DeformationQuality {
    /// Update every frame, full precision
    High,
    /// Update every 2 frames
    #[default]
    Medium,
    /// Update every 4 frames
    Low,
    /// GPU compute shader (best for large meshes)
    Gpu,
}

// ============================================================================
// DeformationState Component
// ============================================================================

/// Per-entity deformation state tracking
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct DeformationState {
    /// Elastic displacement (recoverable)
    pub elastic_displacement: Vec<Vec3>,
    /// Plastic displacement (permanent)
    pub plastic_displacement: Vec<Vec3>,
    /// Thermal displacement
    pub thermal_displacement: Vec<Vec3>,
    /// Total displacement (sum of all)
    pub total_displacement: Vec<Vec3>,
    /// Maximum elastic strain before plastic deformation
    pub yield_strain: f32,
    /// Current maximum displacement magnitude
    pub max_displacement: f32,
    /// Reference temperature for thermal expansion (K)
    pub reference_temperature: f32,
    /// Thermal expansion coefficient (1/K)
    pub thermal_expansion_coeff: f32,
    /// Enable plastic (permanent) deformation
    pub allow_plastic: bool,
    /// Enable thermal deformation
    pub allow_thermal: bool,
}

impl Default for DeformationState {
    fn default() -> Self {
        Self {
            elastic_displacement: Vec::new(),
            plastic_displacement: Vec::new(),
            thermal_displacement: Vec::new(),
            total_displacement: Vec::new(),
            yield_strain: 0.002, // 0.2% typical for steel
            max_displacement: 0.0,
            reference_temperature: 293.15, // 20°C
            thermal_expansion_coeff: 12e-6, // Steel
            allow_plastic: true,
            allow_thermal: true,
        }
    }
}

impl DeformationState {
    /// Initialize for given vertex count
    pub fn init(&mut self, vertex_count: usize) {
        self.elastic_displacement = vec![Vec3::ZERO; vertex_count];
        self.plastic_displacement = vec![Vec3::ZERO; vertex_count];
        self.thermal_displacement = vec![Vec3::ZERO; vertex_count];
        self.total_displacement = vec![Vec3::ZERO; vertex_count];
    }
    
    /// Update total displacement from components
    pub fn update_total(&mut self) {
        self.max_displacement = 0.0;
        
        for i in 0..self.total_displacement.len() {
            self.total_displacement[i] = self.elastic_displacement[i]
                + self.plastic_displacement[i]
                + self.thermal_displacement[i];
            
            let mag = self.total_displacement[i].length();
            if mag > self.max_displacement {
                self.max_displacement = mag;
            }
        }
    }
    
    /// Apply elastic displacement at vertex
    pub fn apply_elastic(&mut self, vertex_idx: usize, displacement: Vec3) {
        if vertex_idx < self.elastic_displacement.len() {
            self.elastic_displacement[vertex_idx] += displacement;
        }
    }
    
    /// Convert elastic to plastic if exceeds yield
    pub fn check_yield(&mut self, vertex_idx: usize, size: Vec3) {
        if !self.allow_plastic || vertex_idx >= self.elastic_displacement.len() {
            return;
        }
        
        let elastic = self.elastic_displacement[vertex_idx];
        let strain = elastic.length() / size.min_element().max(0.001);
        
        if strain > self.yield_strain {
            // Transfer excess to plastic
            let excess_ratio = (strain - self.yield_strain) / strain;
            let plastic_part = elastic * excess_ratio;
            
            self.plastic_displacement[vertex_idx] += plastic_part;
            self.elastic_displacement[vertex_idx] -= plastic_part;
        }
    }
    
    /// Apply thermal displacement based on temperature delta
    pub fn apply_thermal(&mut self, vertex_idx: usize, position: Vec3, temperature: f32) {
        if !self.allow_thermal || vertex_idx >= self.thermal_displacement.len() {
            return;
        }
        
        let delta_t = temperature - self.reference_temperature;
        let strain = self.thermal_expansion_coeff * delta_t;
        
        // Thermal expansion is radial from center
        self.thermal_displacement[vertex_idx] = position * strain;
    }
    
    /// Reset elastic deformation (spring back)
    pub fn reset_elastic(&mut self, damping: f32) {
        for disp in &mut self.elastic_displacement {
            *disp *= 1.0 - damping;
        }
    }
    
    /// Get total displacement at vertex
    pub fn get_displacement(&self, vertex_idx: usize) -> Vec3 {
        self.total_displacement.get(vertex_idx).copied().unwrap_or(Vec3::ZERO)
    }
}

// ============================================================================
// Events
// ============================================================================

/// Event triggered when mesh should fracture
#[derive(Message, Clone, Debug)]
pub struct FractureMeshEvent {
    /// Entity to fracture
    pub entity: Entity,
    /// Fracture plane origin
    pub origin: Vec3,
    /// Fracture plane normal
    pub normal: Vec3,
    /// Crack propagation direction
    pub direction: Vec3,
    /// Fracture energy
    pub energy: f32,
}

/// Event triggered on impact deformation
#[derive(Message, Clone, Debug)]
pub struct ImpactDeformEvent {
    /// Entity that was impacted
    pub entity: Entity,
    /// Impact point in local space
    pub point: Vec3,
    /// Impact force vector
    pub force: Vec3,
    /// Impact radius
    pub radius: f32,
    /// Is this a permanent (plastic) deformation
    pub permanent: bool,
}

// ============================================================================
// Vertex Influence
// ============================================================================

/// Per-vertex influence data for deformation
#[derive(Clone, Debug, Default)]
pub struct VertexInfluence {
    /// Stress contribution to this vertex
    pub stress: Vec3,
    /// Temperature at this vertex
    pub temperature: f32,
    /// Distance from impact point (if any)
    pub impact_distance: f32,
    /// Bone/joint weights (for skeletal deformation)
    pub bone_weights: [f32; 4],
    /// Bone/joint indices
    pub bone_indices: [u32; 4],
}

/// Deformation configuration resource
#[derive(Resource, Reflect, Clone, Debug)]
#[reflect(Resource)]
pub struct DeformationConfig {
    /// Global deformation scale
    pub scale: f32,
    /// Maximum displacement as fraction of mesh size
    pub max_displacement_ratio: f32,
    /// Elastic spring constant (stiffness)
    pub stiffness: f32,
    /// Damping factor for elastic recovery
    pub damping: f32,
    /// Enable GPU compute for large meshes
    pub use_gpu: bool,
    /// Vertex count threshold for GPU compute
    pub gpu_threshold: usize,
    /// Update frequency (frames between updates)
    pub update_interval: u32,
}

impl Default for DeformationConfig {
    fn default() -> Self {
        Self {
            scale: 1.0,
            max_displacement_ratio: 0.1, // 10% of mesh size
            stiffness: 1000.0,
            damping: 0.1,
            use_gpu: true,
            gpu_threshold: 10000,
            update_interval: 1,
        }
    }
}

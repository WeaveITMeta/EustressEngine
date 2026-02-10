//! # Deformation Systems
//!
//! ECS systems for updating mesh deformation.

use bevy::prelude::*;
use tracing::info;
use bevy::mesh::{Mesh, VertexAttributeValues};

use super::components::*;
use crate::classes::BasePart;
use crate::realism::materials::stress_strain::StressTensor;
use crate::realism::materials::properties::MaterialProperties;
use crate::realism::particles::components::ThermodynamicState;

// ============================================================================
// Initialization
// ============================================================================

/// Initialize deformable mesh components for entities with deformation enabled
pub fn init_deformable_meshes(
    mut commands: Commands,
    query: Query<(Entity, &BasePart, &Mesh3d), (Changed<BasePart>, Without<DeformableMesh>)>,
    meshes: Res<Assets<Mesh>>,
) {
    for (entity, base_part, mesh3d) in query.iter() {
        if !base_part.deformation {
            continue;
        }
        
        // Get vertex count from mesh
        let mesh_handle = &mesh3d.0;
        let vertex_count = if let Some(mesh) = meshes.get(mesh_handle) {
            mesh.count_vertices()
        } else {
            0
        };
        
        if vertex_count == 0 {
            continue;
        }
        
        // Create deformation state
        let mut deformation_state = DeformationState::default();
        deformation_state.init(vertex_count);
        
        // Add components
        commands.entity(entity).insert((
            DeformableMesh {
                original_mesh: mesh_handle.clone(),
                deformed_mesh: mesh_handle.clone(), // Will be replaced with clone
                vertex_count,
                dirty: false,
                quality: DeformationQuality::Medium,
            },
            deformation_state,
        ));
        
        info!("Initialized deformable mesh with {} vertices", vertex_count);
    }
}

/// Remove deformation components when deformation is disabled
pub fn cleanup_deformable_meshes(
    mut commands: Commands,
    query: Query<(Entity, &BasePart), (Changed<BasePart>, With<DeformableMesh>)>,
) {
    for (entity, base_part) in query.iter() {
        if !base_part.deformation {
            commands.entity(entity).remove::<DeformableMesh>();
            commands.entity(entity).remove::<DeformationState>();
        }
    }
}

// ============================================================================
// Stress-Based Deformation
// ============================================================================

/// Update vertex displacement from stress tensor
pub fn update_stress_deformation(
    mut query: Query<(
        &BasePart,
        &StressTensor,
        &MaterialProperties,
        &mut DeformationState,
        &mut DeformableMesh,
    )>,
    config: Res<DeformationConfig>,
) {
    for (base_part, stress, material, mut deform_state, mut deform_mesh) in query.iter_mut() {
        if !base_part.deformation {
            continue;
        }
        
        let vertex_count = deform_state.elastic_displacement.len();
        if vertex_count == 0 {
            continue;
        }
        
        // Calculate strain from stress using Hooke's law: ε = σ/E
        let young_modulus = material.young_modulus;
        let poisson = material.poisson_ratio;
        
        // Principal strains from principal stresses
        let strain_x = (stress.principal[0] - poisson * (stress.principal[1] + stress.principal[2])) / young_modulus;
        let strain_y = (stress.principal[1] - poisson * (stress.principal[0] + stress.principal[2])) / young_modulus;
        let strain_z = (stress.principal[2] - poisson * (stress.principal[0] + stress.principal[1])) / young_modulus;
        
        let strain_vec = Vec3::new(strain_x, strain_y, strain_z) * config.scale;
        
        // Apply strain to vertices (simplified: uniform strain field)
        // In full implementation, would interpolate stress field across mesh
        for i in 0..vertex_count {
            // Estimate vertex position from index (would use actual positions)
            let t = i as f32 / vertex_count as f32;
            let local_pos = Vec3::new(
                (t * 2.0 - 1.0) * base_part.size.x * 0.5,
                ((i % 100) as f32 / 100.0 * 2.0 - 1.0) * base_part.size.y * 0.5,
                ((i % 10) as f32 / 10.0 * 2.0 - 1.0) * base_part.size.z * 0.5,
            );
            
            // Displacement = strain × position
            let displacement = strain_vec * local_pos;
            
            // Clamp to max displacement
            let max_disp = base_part.size.min_element() * config.max_displacement_ratio;
            let clamped = displacement.clamp_length_max(max_disp);
            
            deform_state.elastic_displacement[i] = clamped;
            
            // Check for plastic yield
            deform_state.check_yield(i, base_part.size);
        }
        
        deform_state.update_total();
        deform_mesh.dirty = true;
    }
}

// ============================================================================
// Thermal Deformation
// ============================================================================

/// Update vertex displacement from temperature
pub fn update_thermal_deformation(
    mut query: Query<(
        &BasePart,
        &ThermodynamicState,
        &mut DeformationState,
        &mut DeformableMesh,
    )>,
    config: Res<DeformationConfig>,
) {
    for (base_part, thermo, mut deform_state, mut deform_mesh) in query.iter_mut() {
        if !base_part.deformation || !deform_state.allow_thermal {
            continue;
        }
        
        let vertex_count = deform_state.thermal_displacement.len();
        if vertex_count == 0 {
            continue;
        }
        
        let temperature = thermo.temperature;
        let delta_t = temperature - deform_state.reference_temperature;
        let thermal_strain = deform_state.thermal_expansion_coeff * delta_t;
        
        // Apply thermal expansion (isotropic)
        for i in 0..vertex_count {
            // Estimate vertex position
            let t = i as f32 / vertex_count as f32;
            let local_pos = Vec3::new(
                (t * 2.0 - 1.0) * base_part.size.x * 0.5,
                ((i % 100) as f32 / 100.0 * 2.0 - 1.0) * base_part.size.y * 0.5,
                ((i % 10) as f32 / 10.0 * 2.0 - 1.0) * base_part.size.z * 0.5,
            );
            
            // Thermal expansion is radial
            deform_state.thermal_displacement[i] = local_pos * thermal_strain * config.scale;
        }
        
        deform_state.update_total();
        deform_mesh.dirty = true;
    }
}

// ============================================================================
// Impact Deformation
// ============================================================================

/// Apply deformation from impact events
pub fn apply_impact_deformation(
    mut events: MessageReader<ImpactDeformEvent>,
    mut query: Query<(&BasePart, &mut DeformationState, &mut DeformableMesh)>,
    meshes: Res<Assets<Mesh>>,
    config: Res<DeformationConfig>,
) {
    for event in events.read() {
        let Ok((_base_part, mut deform_state, mut deform_mesh)) = query.get_mut(event.entity) else {
            continue;
        };
        
        let Some(mesh) = meshes.get(&deform_mesh.original_mesh) else {
            continue;
        };
        
        let Some(VertexAttributeValues::Float32x3(positions)) = 
            mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            continue;
        };
        
        // Apply radial deformation from impact point
        for (i, pos) in positions.iter().enumerate() {
            let vertex_pos = Vec3::new(pos[0], pos[1], pos[2]);
            let dist = (vertex_pos - event.point).length();
            
            if dist < event.radius {
                let falloff = 1.0 - (dist / event.radius);
                let displacement = event.force.normalize() * falloff * event.force.length() * config.scale;
                
                if event.permanent {
                    // Directly add to plastic displacement
                    if i < deform_state.plastic_displacement.len() {
                        deform_state.plastic_displacement[i] += displacement;
                    }
                } else {
                    deform_state.apply_elastic(i, displacement);
                }
            }
        }
        
        deform_state.update_total();
        deform_mesh.dirty = true;
    }
}

// ============================================================================
// Mesh Update
// ============================================================================

/// Apply total displacement to mesh vertices
pub fn update_mesh_vertices(
    mut query: Query<(&DeformableMesh, &DeformationState)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (deform_mesh, deform_state) in query.iter_mut() {
        if !deform_mesh.dirty {
            continue;
        }
        
        // First, get original positions from the original mesh
        let original_positions: Vec<[f32; 3]> = {
            let Some(original_mesh) = meshes.get(&deform_mesh.original_mesh) else {
                continue;
            };
            
            let Some(VertexAttributeValues::Float32x3(positions)) = 
                original_mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
                continue;
            };
            
            positions.clone()
        };
        
        // Calculate new positions
        let mut new_positions: Vec<[f32; 3]> = Vec::with_capacity(original_positions.len());
        
        for (i, pos) in original_positions.iter().enumerate() {
            let displacement = deform_state.get_displacement(i);
            new_positions.push([
                pos[0] + displacement.x,
                pos[1] + displacement.y,
                pos[2] + displacement.z,
            ]);
        }
        
        // Now get mutable reference to deformed mesh and update it
        let Some(mesh) = meshes.get_mut(&deform_mesh.deformed_mesh) else {
            continue;
        };
        
        // Update mesh
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            VertexAttributeValues::Float32x3(new_positions),
        );
        
        // Recalculate normals
        // mesh.compute_normals(); // Would need to be called
    }
}

// ============================================================================
// Fracture Mesh
// ============================================================================

/// Handle mesh fracture events
pub fn handle_fracture_mesh(
    mut events: MessageReader<FractureMeshEvent>,
    _commands: Commands,
    _query: Query<(&BasePart, &DeformableMesh, &Transform)>,
    _meshes: ResMut<Assets<Mesh>>,
) {
    for event in events.read() {
        // TODO: Implement mesh splitting along fracture plane
        // For now, log the fracture event
        tracing::info!("Fracture event on entity {:?} at {:?} along {:?}", 
            event.entity, event.origin, event.direction);
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Get vertex position from mesh
fn get_vertex_position(mesh: &Mesh, index: usize) -> Option<Vec3> {
    if let Some(VertexAttributeValues::Float32x3(positions)) = 
        mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        positions.get(index).map(|p| Vec3::new(p[0], p[1], p[2]))
    } else {
        None
    }
}

/// Split mesh by plane (simplified - returns None for now)
/// Full implementation would use mesh boolean operations
fn split_mesh_by_plane(
    _mesh: &Mesh,
    _origin: Vec3,
    _normal: Vec3,
) -> (Option<Mesh>, Option<Mesh>) {
    // TODO: Implement proper mesh splitting
    // This would involve:
    // 1. Classify vertices as above/below plane
    // 2. Find edges that cross the plane
    // 3. Create new vertices at intersection points
    // 4. Triangulate the cut surface
    // 5. Build two separate meshes
    
    (None, None)
}

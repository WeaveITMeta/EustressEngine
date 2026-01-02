//! Entity Commands - Create, Delete, Duplicate

#![allow(dead_code)]

use bevy::prelude::*;
use crate::classes::{Instance, BasePart, Part};

/// Command to delete entities
#[derive(Clone, Debug)]
pub struct DeleteCommand {
    pub description: String,
    pub entities: Vec<DeletedEntity>,
}

#[derive(Clone, Debug)]
pub struct DeletedEntity {
    pub entity_id: Entity,
    pub instance: Instance,
    pub basepart: Option<BasePart>,
    pub part: Option<Part>,
    pub name: String,
}

impl DeleteCommand {
    pub fn new(entities: Vec<DeletedEntity>) -> Self {
        let description = format!("Delete {} entity(ies)", entities.len());
        Self { description, entities }
    }
    
    pub fn execute(&self, world: &mut World) -> Result<(), String> {
        for deleted_entity in &self.entities {
            if world.get_entity(deleted_entity.entity_id).is_ok() {
                world.despawn(deleted_entity.entity_id);
            }
        }
        Ok(())
    }
    
    pub fn undo(&self, world: &mut World) -> Result<(), String> {
        use bevy::pbr::StandardMaterial;
        use bevy::prelude::{Mesh3d, MeshMaterial3d};
        use bevy::mesh::{Mesh, Meshable};
        use bevy::math::primitives::{Cuboid, Sphere, Cylinder};
        use bevy::asset::Assets;
        
        // Prepare all mesh and material handles first  
        let mut mesh_handles = Vec::new();
        let mut material_handles = Vec::new();
        let entities_to_restore: Vec<_> = self.entities.clone();
        
        // Create all meshes
        {
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            for deleted_entity in &entities_to_restore {
                if let (Some(bp), Some(p)) = (&deleted_entity.basepart, &deleted_entity.part) {
                    let mesh = match p.shape {
                        crate::classes::PartType::Block => meshes.add(Cuboid::from_size(bp.size)),
                        crate::classes::PartType::Ball => meshes.add(Sphere::new(bp.size.x / 2.0).mesh().ico(5).unwrap()),
                        crate::classes::PartType::Cylinder => meshes.add(Cylinder::new(bp.size.x / 2.0, bp.size.y)),
                        crate::classes::PartType::Wedge => meshes.add(Cuboid::from_size(bp.size)),
                        crate::classes::PartType::CornerWedge => meshes.add(Cuboid::from_size(bp.size)),
                        crate::classes::PartType::Cone => meshes.add(Cylinder::new(bp.size.x / 2.0, bp.size.y)),
                    };
                    mesh_handles.push(mesh);
                }
            }
        }
        
        // Create all materials
        {
            let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
            for deleted_entity in &entities_to_restore {
                if let (Some(bp), Some(_p)) = (&deleted_entity.basepart, &deleted_entity.part) {
                    let (roughness, metallic, reflectance) = bp.material.pbr_params();
                    let material = materials.add(StandardMaterial {
                        base_color: bp.color,
                        perceptual_roughness: roughness,
                        metallic,
                        reflectance,
                        alpha_mode: if bp.transparency > 0.0 {
                            bevy::prelude::AlphaMode::Blend
                        } else {
                            bevy::prelude::AlphaMode::Opaque
                        },
                        ..default()
                    });
                    material_handles.push(material);
                }
            }
        }
        
        // Now spawn entities with the prepared handles
        for ((mesh, material), deleted_entity) in mesh_handles.into_iter().zip(material_handles).zip(entities_to_restore) {
            if let (Some(bp), Some(p)) = (&deleted_entity.basepart, &deleted_entity.part) {
                let entity = world.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    bp.cframe,
                    deleted_entity.instance.clone(),
                    bp.clone(),
                    p.clone(),
                    Name::new(deleted_entity.name.clone()),
                    crate::rendering::PartEntity {
                        part_id: format!("{:?}", Entity::PLACEHOLDER),
                    },
                )).id();
                
                // Update PartEntity with real entity ID
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    if let Some(mut part_entity) = entity_mut.get_mut::<crate::rendering::PartEntity>() {
                        part_entity.part_id = format!("{:?}", entity);
                    }
                }
            }
        }
        
        Ok(())
    }
}

/// Command to duplicate entities
#[derive(Clone, Debug)]
pub struct DuplicateCommand {
    pub description: String,
    pub source_entities: Vec<DuplicatedEntity>,
    pub created_entities: Vec<Entity>,
}

#[derive(Clone, Debug)]
pub struct DuplicatedEntity {
    pub instance: Instance,
    pub basepart: Option<BasePart>,
    pub part: Option<Part>,
    pub name: String,
}

impl DuplicateCommand {
    pub fn new(source_entities: Vec<DuplicatedEntity>) -> Self {
        let description = format!("Duplicate {} entity(ies)", source_entities.len());
        Self {
            description,
            source_entities,
            created_entities: Vec::new(),
        }
    }
    
    pub fn execute(&mut self, world: &mut World) -> Result<(), String> {
        use bevy::pbr::StandardMaterial;
        use bevy::prelude::{Mesh3d, MeshMaterial3d};
        use bevy::mesh::{Mesh, Meshable};
        use bevy::math::primitives::{Cuboid, Sphere, Cylinder};
        use bevy::asset::Assets;
        
        self.created_entities.clear();
        
        let mut mesh_handles = Vec::new();
        let mut material_handles = Vec::new();
        let entities_to_duplicate: Vec<_> = self.source_entities.clone();
        
        // Create all meshes
        {
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            for dup_entity in &entities_to_duplicate {
                if let (Some(bp), Some(p)) = (&dup_entity.basepart, &dup_entity.part) {
                    let mesh = match p.shape {
                        crate::classes::PartType::Block => meshes.add(Cuboid::from_size(bp.size)),
                        crate::classes::PartType::Ball => meshes.add(Sphere::new(bp.size.x / 2.0).mesh().ico(5).unwrap()),
                        crate::classes::PartType::Cylinder => meshes.add(Cylinder::new(bp.size.x / 2.0, bp.size.y)),
                        crate::classes::PartType::Wedge => meshes.add(Cuboid::from_size(bp.size)),
                        crate::classes::PartType::CornerWedge => meshes.add(Cuboid::from_size(bp.size)),
                        crate::classes::PartType::Cone => meshes.add(Cylinder::new(bp.size.x / 2.0, bp.size.y)),
                    };
                    mesh_handles.push(mesh);
                }
            }
        }
        
        // Create all materials
        {
            let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
            for dup_entity in &entities_to_duplicate {
                if let (Some(bp), Some(_p)) = (&dup_entity.basepart, &dup_entity.part) {
                    let (roughness, metallic, reflectance) = bp.material.pbr_params();
                    let material = materials.add(StandardMaterial {
                        base_color: bp.color,
                        perceptual_roughness: roughness,
                        metallic,
                        reflectance,
                        alpha_mode: if bp.transparency > 0.0 {
                            bevy::prelude::AlphaMode::Blend
                        } else {
                            bevy::prelude::AlphaMode::Opaque
                        },
                        ..default()
                    });
                    material_handles.push(material);
                }
            }
        }
        
        // Now spawn entities
        for ((mesh, material), dup_entity) in mesh_handles.into_iter().zip(material_handles).zip(entities_to_duplicate) {
            if let (Some(bp), Some(p)) = (&dup_entity.basepart, &dup_entity.part) {
                let entity = world.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    bp.cframe,
                    dup_entity.instance.clone(),
                    bp.clone(),
                    p.clone(),
                    Name::new(dup_entity.name.clone()),
                    crate::rendering::PartEntity {
                        part_id: format!("{:?}", Entity::PLACEHOLDER),
                    },
                )).id();
                
                // Update PartEntity with real entity ID
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    if let Some(mut part_entity) = entity_mut.get_mut::<crate::rendering::PartEntity>() {
                        part_entity.part_id = format!("{:?}", entity);
                    }
                }
                
                self.created_entities.push(entity);
            }
        }
        
        Ok(())
    }
    
    pub fn undo(&self, world: &mut World) -> Result<(), String> {
        // Delete the duplicated entities
        for &entity in &self.created_entities {
            if world.get_entity(entity).is_ok() {
                world.despawn(entity);
            }
        }
        Ok(())
    }
}

/// Command to create new entities
/// Note: Currently not fully implemented due to event system complexity
/// Parts created from toolbox don't push to history yet
#[derive(Clone, Debug)]
pub struct CreateCommand {
    pub description: String,
    pub entity_data: DuplicatedEntity, // Reuse same structure
    pub created_entity: Option<Entity>,
}

impl CreateCommand {
    pub fn new(entity_data: DuplicatedEntity) -> Self {
        let description = format!("Create {}", entity_data.name);
        Self {
            description,
            entity_data,
            created_entity: None,
        }
    }
    
    pub fn execute(&mut self, world: &mut World) -> Result<(), String> {
        use bevy::pbr::StandardMaterial;
        use bevy::prelude::{Mesh3d, MeshMaterial3d};
        use bevy::mesh::{Mesh, Meshable};
        use bevy::math::primitives::{Cuboid, Sphere, Cylinder};
        use bevy::asset::Assets;
        
        if let (Some(bp), Some(p)) = (&self.entity_data.basepart, &self.entity_data.part) {
            // Create mesh
            let mesh = {
                let mut meshes = world.resource_mut::<Assets<Mesh>>();
                match p.shape {
                    crate::classes::PartType::Block => meshes.add(Cuboid::from_size(bp.size)),
                    crate::classes::PartType::Ball => meshes.add(Sphere::new(bp.size.x / 2.0).mesh().ico(5).unwrap()),
                    crate::classes::PartType::Cylinder => meshes.add(Cylinder::new(bp.size.x / 2.0, bp.size.y)),
                    crate::classes::PartType::Wedge => meshes.add(Cuboid::from_size(bp.size)),
                    crate::classes::PartType::CornerWedge => meshes.add(Cuboid::from_size(bp.size)),
                    crate::classes::PartType::Cone => meshes.add(Cylinder::new(bp.size.x / 2.0, bp.size.y)),
                }
            };
            
            // Create material  
            let material = {
                let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
                let (roughness, metallic, reflectance) = bp.material.pbr_params();
                materials.add(StandardMaterial {
                    base_color: bp.color,
                    perceptual_roughness: roughness,
                    metallic,
                    reflectance,
                    alpha_mode: if bp.transparency > 0.0 {
                        bevy::prelude::AlphaMode::Blend
                    } else {
                        bevy::prelude::AlphaMode::Opaque
                    },
                    ..default()
                })
            };
            
            let entity = world.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                bp.cframe,
                self.entity_data.instance.clone(),
                bp.clone(),
                p.clone(),
                Name::new(self.entity_data.name.clone()),
                crate::rendering::PartEntity {
                    part_id: format!("{:?}", Entity::PLACEHOLDER),
                },
            )).id();
            
            if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                if let Some(mut part_entity) = entity_mut.get_mut::<crate::rendering::PartEntity>() {
                    part_entity.part_id = format!("{:?}", entity);
                }
            }
            
            self.created_entity = Some(entity);
        }
        
        Ok(())
    }
    
    pub fn undo(&self, world: &mut World) -> Result<(), String> {
        if let Some(entity) = self.created_entity {
            if world.get_entity(entity).is_ok() {
                world.despawn(entity);
            }
        }
        Ok(())
    }
}

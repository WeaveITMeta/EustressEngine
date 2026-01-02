// Material Sync - Real-time synchronization of BasePart properties to StandardMaterial
// Keeps visual appearance in sync with property changes

use bevy::prelude::*;
use bevy::light::{NotShadowCaster, TransmittedShadowReceiver};
use crate::classes::{BasePart, Material as RobloxMaterial};

/// Plugin for syncing BasePart properties to StandardMaterial in real-time
pub struct MaterialSyncPlugin;

impl Plugin for MaterialSyncPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_basepart_to_material);
    }
}

/// System to sync BasePart properties (Color, Material, Reflectance, Transparency) to StandardMaterial
fn sync_basepart_to_material(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    query: Query<(
        Entity, 
        &BasePart, 
        &MeshMaterial3d<StandardMaterial>, 
        Option<&NotShadowCaster>,
        Option<&TransmittedShadowReceiver>,
    ), Changed<BasePart>>,
) {
    for (entity, basepart, material_handle, has_no_shadow, has_transmission) in query.iter() {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            // Sync Color and Transparency
            let alpha = 1.0 - basepart.transparency.clamp(0.0, 1.0);
            material.base_color = basepart.color.with_alpha(alpha);
            
            // Check if this is Glass material
            let is_glass = matches!(basepart.material, RobloxMaterial::Glass);
            
            // Sync Reflectance - affects metallic and perceptual_roughness
            let reflectance = basepart.reflectance.clamp(0.0, 1.0);
            material.metallic = reflectance;
            material.perceptual_roughness = match basepart.material {
                RobloxMaterial::Plastic => 0.7,
                RobloxMaterial::Wood => 0.8,
                RobloxMaterial::Slate => 0.6,
                RobloxMaterial::Concrete => 0.9,
                RobloxMaterial::CorrodedMetal => 0.8,
                RobloxMaterial::DiamondPlate => 0.3,
                RobloxMaterial::Foil => 0.2,
                RobloxMaterial::Grass => 0.9,
                RobloxMaterial::Ice => 0.1,
                RobloxMaterial::Marble => 0.4,
                RobloxMaterial::Granite => 0.7,
                RobloxMaterial::Brick => 0.8,
                RobloxMaterial::Sand => 0.9,
                RobloxMaterial::Fabric => 0.9,
                RobloxMaterial::SmoothPlastic => 0.5,
                RobloxMaterial::Metal => 0.3,
                RobloxMaterial::WoodPlanks => 0.7,
                RobloxMaterial::Neon => 0.1,
                RobloxMaterial::Glass => 0.0,
            };
            
            // Adjust roughness based on reflectance (more reflectance = less rough)
            material.perceptual_roughness = material.perceptual_roughness * (1.0 - reflectance * 0.5);
            
            // Sync Transparency and alpha mode
            if basepart.transparency > 0.0 {
                material.alpha_mode = AlphaMode::Blend;
            } else {
                material.alpha_mode = AlphaMode::Opaque;
            }
            
            // Glass material gets specular/diffuse transmission for colored shadows
            if is_glass {
                material.specular_transmission = 0.9;
                material.diffuse_transmission = 0.3;
                material.thickness = 0.5;
                material.ior = 1.5;
            } else {
                material.specular_transmission = 0.0;
                material.diffuse_transmission = 0.0;
                material.thickness = 0.0;
            }
            
            // Shadow casting threshold: >= 50% transparency = no shadow
            let should_cast_shadow = basepart.transparency < 0.5;
            if should_cast_shadow {
                // Should cast shadow - remove NotShadowCaster if present
                if has_no_shadow.is_some() {
                    commands.entity(entity).remove::<NotShadowCaster>();
                }
            } else {
                // Should NOT cast shadow - add NotShadowCaster if not present
                if has_no_shadow.is_none() {
                    commands.entity(entity).insert(NotShadowCaster);
                }
            }
            
            // Glass with < 50% transparency gets TransmittedShadowReceiver for colored shadows
            let needs_transmission = is_glass && basepart.transparency < 0.5;
            if needs_transmission {
                if has_transmission.is_none() {
                    commands.entity(entity).insert(TransmittedShadowReceiver);
                }
            } else {
                if has_transmission.is_some() {
                    commands.entity(entity).remove::<TransmittedShadowReceiver>();
                }
            }
            
            // Emissive for Neon material
            if matches!(basepart.material, RobloxMaterial::Neon) {
                material.emissive = LinearRgba::from(basepart.color) * 2.0;
            } else {
                material.emissive = LinearRgba::NONE;
            }
        }
    }
}

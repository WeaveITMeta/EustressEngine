// Material Sync - Real-time synchronization of BasePart properties to StandardMaterial
// Keeps visual appearance in sync with property changes

use bevy::prelude::*;
use bevy::light::{NotShadowCaster, TransmittedShadowReceiver};
use crate::classes::{BasePart, Material as RobloxMaterial};

/// Tracks the last-known size of the MaterialRegistry to detect when new materials load.
#[derive(Resource, Default)]
struct MaterialRegistryTracker {
    last_count: usize,
}

/// Plugin for syncing BasePart properties to StandardMaterial in real-time
pub struct MaterialSyncPlugin;

impl Plugin for MaterialSyncPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaterialRegistryTracker>()
            .add_systems(Update, (
                reapply_materials_on_registry_change,
                sync_basepart_to_material,
            ).chain());
    }
}

/// When MaterialService finishes loading (registry grows), mark all BaseParts as changed
/// so `sync_basepart_to_material` re-resolves them with the full registry.
fn reapply_materials_on_registry_change(
    material_registry: Option<Res<crate::space::material_loader::MaterialRegistry>>,
    mut tracker: ResMut<MaterialRegistryTracker>,
    mut base_parts: Query<&mut BasePart>,
) {
    let Some(ref registry) = material_registry else { return };
    let current_count = registry.names().len();
    if current_count > tracker.last_count {
        info!("🎨 MaterialRegistry grew ({} → {}), re-applying materials to all parts",
              tracker.last_count, current_count);
        tracker.last_count = current_count;
        // Touch all BaseParts so Changed<BasePart> triggers sync
        for mut bp in base_parts.iter_mut() {
            bp.set_changed();
        }
    }
}

/// System to sync BasePart properties (Color, Material, Reflectance, Transparency) to StandardMaterial
fn sync_basepart_to_material(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    material_registry: Option<Res<crate::space::material_loader::MaterialRegistry>>,
    query: Query<(
        Entity,
        &BasePart,
        &MeshMaterial3d<StandardMaterial>,
        Option<&NotShadowCaster>,
        Option<&TransmittedShadowReceiver>,
    ), Changed<BasePart>>,
) {
    for (entity, basepart, material_handle, has_no_shadow, has_transmission) in query.iter() {
        // If the MaterialRegistry has a pre-built material with textures for this
        // preset name, swap the handle entirely (gets textures on all faces).
        let mat_name = format!("{:?}", basepart.material);
        if let Some(ref registry) = material_registry {
            if let Some(registry_handle) = registry.get(&mat_name) {
                // Clone the registry material so we can tint it with the part's properties
                if let Some(base_mat) = materials.get(&registry_handle) {
                    let mut cloned = base_mat.clone();
                    let alpha = 1.0 - basepart.transparency.clamp(0.0, 1.0);
                    cloned.base_color = basepart.color.with_alpha(alpha);
                    if basepart.transparency > 0.0 {
                        cloned.alpha_mode = AlphaMode::Blend;
                    }
                    // Apply reflectance — increases metallic sheen and reduces roughness
                    let reflectance = basepart.reflectance.clamp(0.0, 1.0);
                    cloned.reflectance = reflectance;
                    cloned.metallic = (cloned.metallic + reflectance).min(1.0);
                    cloned.perceptual_roughness *= 1.0 - reflectance * 0.5;
                    // Emissive for Neon
                    if matches!(basepart.material, RobloxMaterial::Neon) {
                        cloned.emissive = LinearRgba::from(basepart.color) * 2.0;
                    }
                    let new_handle = materials.add(cloned);
                    commands.entity(entity).insert(MeshMaterial3d(new_handle));
                    continue;
                }
            }
        }

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
            
            // Texture repeat (UV tiling)
            let [u_repeat, v_repeat] = basepart.texture_repeat;
            if u_repeat != 1.0 || v_repeat != 1.0 {
                material.uv_transform = bevy::math::Affine2::from_scale(bevy::math::Vec2::new(u_repeat, v_repeat));
            } else {
                material.uv_transform = bevy::math::Affine2::IDENTITY;
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

// Material Sync - Real-time synchronization of BasePart properties to StandardMaterial
// Keeps visual appearance in sync with property changes

use bevy::prelude::*;
use bevy::light::{NotShadowCaster, TransmittedShadowReceiver};
use crate::classes::{BasePart, Material as EustressMaterial};

/// Tracks the last-known size of the MaterialRegistry to detect when new materials load.
#[derive(Resource, Default)]
struct MaterialRegistryTracker {
    last_count: usize,
}

/// Tracks which registry materials have already had their textures patched to Repeat.
/// Avoids re-scanning every material × 6 texture slots on every single frame.
#[derive(Resource, Default)]
struct PatchedTextureTracker {
    patched_materials: std::collections::HashSet<String>,
}

/// Plugin for syncing BasePart properties to StandardMaterial in real-time
pub struct MaterialSyncPlugin;

impl Plugin for MaterialSyncPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaterialRegistryTracker>()
            .init_resource::<PatchedTextureTracker>()
            .add_systems(Update, (
                reapply_materials_on_registry_change,
                set_material_textures_to_repeat,
                sync_basepart_to_material,
            ).chain());
    }
}

/// Ensure all material textures use Repeat address mode for proper tiling.
/// Bevy defaults to ClampToEdge, which causes visible seams at UV tile
/// boundaries on textured parts. This system runs once per texture when
/// the Image asset finishes loading.
fn set_material_textures_to_repeat(
    material_registry: Option<Res<crate::space::material_loader::MaterialRegistry>>,
    materials: Res<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut tracker: ResMut<PatchedTextureTracker>,
) {
    let Some(ref registry) = material_registry else { return };
    // Early out: if we've patched every known material, nothing to do.
    if tracker.patched_materials.len() >= registry.len() {
        return;
    }
    // Only iterate materials we haven't patched yet.
    for name in registry.names() {
        if tracker.patched_materials.contains(&name) {
            continue;
        }
        let Some(mat_handle) = registry.get(&name) else { continue };
        let Some(mat) = materials.get(&mat_handle) else { continue };
        // Process all texture slots on this material
        let texture_handles = [
            mat.base_color_texture.clone(),
            mat.normal_map_texture.clone(),
            mat.metallic_roughness_texture.clone(),
            mat.emissive_texture.clone(),
            mat.occlusion_texture.clone(),
            mat.depth_map.clone(),
        ];
        let mut all_patched = true;
        for opt_handle in texture_handles.into_iter().flatten() {
            // Read-only check first to avoid marking the image as changed
            let needs_update = if let Some(image) = images.get(&opt_handle) {
                match &image.sampler {
                    bevy::image::ImageSampler::Default => true,
                    bevy::image::ImageSampler::Descriptor(desc) => {
                        desc.address_mode_u != bevy::image::ImageAddressMode::Repeat
                    }
                }
            } else {
                // Image not loaded yet — can't patch, try again next frame
                all_patched = false;
                continue;
            };
            if needs_update {
                if let Some(image) = images.get_mut(&opt_handle) {
                    image.sampler = bevy::image::ImageSampler::Descriptor(
                        bevy::image::ImageSamplerDescriptor {
                            address_mode_u: bevy::image::ImageAddressMode::Repeat,
                            address_mode_v: bevy::image::ImageAddressMode::Repeat,
                            address_mode_w: bevy::image::ImageAddressMode::Repeat,
                            ..Default::default()
                        }
                    );
                }
            }
        }
        if all_patched {
            tracker.patched_materials.insert(name);
        }
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
    let current_count = registry.len();
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
        &Transform,
        &MeshMaterial3d<StandardMaterial>,
        Option<&NotShadowCaster>,
        Option<&TransmittedShadowReceiver>,
    ), Changed<BasePart>>,
    // NOTE: Previously also triggered on Changed<Transform>, which caused
    // a full material clone + GPU allocation EVERY FRAME for every entity
    // whose transform changed (camera orbit, physics, animations, etc.).
    // UV transform is derived from BasePart.size, not Transform.scale,
    // so Changed<BasePart> is sufficient.
) {
    for (entity, basepart, transform, material_handle, has_no_shadow, has_transmission) in query.iter() {
        // Look up material by custom name first, then fall back to enum preset name.
        // Custom materials (e.g. "BrushedMetal" from .mat.toml) use material_name.
        // Built-in presets (Plastic, Metal, etc.) use the enum debug format.
        let mat_name = if basepart.material_name.is_empty() {
            format!("{:?}", basepart.material)
        } else {
            basepart.material_name.clone()
        };
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
                    if matches!(basepart.material, EustressMaterial::Neon) {
                        cloned.emissive = LinearRgba::from(basepart.color) * 2.0;
                    }
                    cloned.uv_transform = compute_uv_transform(basepart, Some(transform.scale));
                    let new_handle = materials.add(cloned);
                    commands.entity(entity).insert(MeshMaterial3d(new_handle));
                    continue;
                }
            }
        }

        // Fallback path: the entity's current material handle was created
        // without textures (e.g. by resolve_material before the registry
        // loaded). Clone the registry's textured material if one exists now,
        // otherwise mutate in-place with PBR scalars only.
        let is_glass = matches!(basepart.material, EustressMaterial::Glass);
        let alpha = 1.0 - basepart.transparency.clamp(0.0, 1.0);
        let reflectance = basepart.reflectance.clamp(0.0, 1.0);
        let (preset_roughness, _preset_metallic, preset_reflectance) = basepart.material.pbr_params();

        // Try to upgrade to a textured material from the registry using
        // the preset enum name (e.g. "Brick", "Metal"). This catches
        // entities whose material was resolved before the MaterialService
        // .mat.toml files finished loading.
        let upgraded_from_registry = if let Some(ref registry) = material_registry {
            let preset_name = basepart.material.as_str();
            if let Some(registry_handle) = registry.get(preset_name) {
                if let Some(base_mat) = materials.get(&registry_handle) {
                    let mut cloned = base_mat.clone();
                    cloned.base_color = basepart.color.with_alpha(alpha);
                    if basepart.transparency > 0.0 {
                        cloned.alpha_mode = AlphaMode::Blend;
                    }
                    cloned.reflectance = reflectance;
                    cloned.metallic = (cloned.metallic + reflectance).min(1.0);
                    cloned.perceptual_roughness *= 1.0 - reflectance * 0.5;
                    if matches!(basepart.material, EustressMaterial::Neon) {
                        cloned.emissive = LinearRgba::from(basepart.color) * 2.0;
                    }
                    cloned.uv_transform = compute_uv_transform(basepart, Some(transform.scale));
                    let new_handle = materials.add(cloned);
                    commands.entity(entity).insert(MeshMaterial3d(new_handle));
                    true
                } else { false }
            } else { false }
        } else { false };

        if !upgraded_from_registry {
            if let Some(material) = materials.get_mut(&material_handle.0) {
                material.base_color = basepart.color.with_alpha(alpha);
                material.metallic = reflectance;
                material.perceptual_roughness = preset_roughness * (1.0 - reflectance * 0.5);
                material.reflectance = preset_reflectance.max(reflectance);
                if basepart.transparency > 0.0 {
                    material.alpha_mode = AlphaMode::Blend;
                } else {
                    material.alpha_mode = AlphaMode::Opaque;
                }
                material.uv_transform = compute_uv_transform(basepart, Some(transform.scale));
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
                if matches!(basepart.material, EustressMaterial::Neon) {
                    material.emissive = LinearRgba::from(basepart.color) * 2.0;
                } else {
                    material.emissive = LinearRgba::NONE;
                }
            }
        }

        // Shadow casting threshold: >= 50% transparency = no shadow
        let should_cast_shadow = basepart.transparency < 0.5;
        if should_cast_shadow {
            if has_no_shadow.is_some() {
                commands.entity(entity).remove::<NotShadowCaster>();
            }
        } else {
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
    }
}

/// World-space UV scale for a part, factoring in both `BasePart.size`
/// and the optional `Transform.scale` override.
///
/// A single `uv_transform` Affine2 applies to all six cuboid faces.
/// Bevy's unit `.glb` meshes and `Cuboid::from_size` primitives both
/// emit [0,1] UVs per face — the texture only tiles if `uv_transform`
/// scales those coordinates beyond 1.0.
///
/// **Per-axis tiling**: we use the two largest world-space dimensions
/// of the part to set U and V independently so a 20×1×20 baseplate
/// tiles the texture 5× on its large faces and 0.25× on its thin
/// edges (assuming TILE_WORLD_SIZE = 4). This is more physically
/// correct than the old uniform-average approach which produced
/// identical density on every face.
///
/// **`transform_scale` parameter**: for file-system-first parts
/// `Transform.scale = size`, and for legacy parts `Transform.scale`
/// may temporarily differ from `BasePart.size` during a mid-drag
/// resize. Passing the actual transform scale lets us compute the
/// true visual dimensions the GPU is rendering. When `None`, the
/// function falls back to `BasePart.size` alone.
///
/// `BasePart.texture_repeat` overrides this entirely when set to
/// anything other than `[1.0, 1.0]` — scripts and the asset pipeline
/// can dial in exact tile counts when they know the surface they're on.
/// Public convenience wrapper for spawn-time UV transform calculation.
/// Takes a raw `Vec3` size (no `BasePart` needed) and returns the
/// Affine2 that tiles textures proportionally to the part's world
/// dimensions. Used by `spawn_part_glb` so the first frame renders
/// with correct texture density before `sync_basepart_to_material`
/// picks up `Changed<BasePart>`.
pub fn compute_uv_transform_from_size(size: Vec3) -> bevy::math::Affine2 {
    const TILE_WORLD_SIZE: f32 = 4.0;
    let mut dims = [
        size.x.abs().max(0.1),
        size.y.abs().max(0.1),
        size.z.abs().max(0.1),
    ];
    dims.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let u_scale = (dims[0] / TILE_WORLD_SIZE).max(0.1);
    let v_scale = (dims[1] / TILE_WORLD_SIZE).max(0.1);
    bevy::math::Affine2::from_scale(bevy::math::Vec2::new(u_scale, v_scale))
}

fn compute_uv_transform(basepart: &BasePart, transform_scale: Option<Vec3>) -> bevy::math::Affine2 {
    const TILE_WORLD_SIZE: f32 = 4.0;
    let [u_repeat, v_repeat] = basepart.texture_repeat;
    if u_repeat != 1.0 || v_repeat != 1.0 {
        return bevy::math::Affine2::from_scale(bevy::math::Vec2::new(u_repeat, v_repeat));
    }
    // Effective world-space size: if the caller provides the live
    // Transform.scale we multiply it element-wise with BasePart.size
    // so that mid-drag legacy parts (scale = size/baked) combined with
    // the baked mesh dimensions produce the correct visual size. For
    // file-system-first parts (unit mesh, scale = size) this is just
    // size * 1 = size. When no transform_scale is available, use
    // BasePart.size directly — correct for newly spawned parts whose
    // Transform hasn't been queried yet.
    let world_size = match transform_scale {
        Some(ts) => {
            // File-system-first: unit mesh × scale = world dims.
            // Legacy final: mesh baked at size, scale = ONE → size × 1 = size.
            // Legacy mid-drag: mesh baked at old_size, scale = size/old_size
            //   → old_size isn't in BasePart anymore (overwritten), but
            //   BasePart.size IS the target size the user is dragging to,
            //   so just use BasePart.size to keep UVs predictive of the
            //   final result rather than the transient mesh ratio.
            Vec3::new(
                basepart.size.x.abs().max(ts.x.abs()).max(0.1),
                basepart.size.y.abs().max(ts.y.abs()).max(0.1),
                basepart.size.z.abs().max(ts.z.abs()).max(0.1),
            )
        }
        None => Vec3::new(
            basepart.size.x.abs().max(0.1),
            basepart.size.y.abs().max(0.1),
            basepart.size.z.abs().max(0.1),
        ),
    };
    // Sort dimensions descending — the two largest drive U and V scale.
    let mut dims = [world_size.x, world_size.y, world_size.z];
    dims.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let u_scale = (dims[0] / TILE_WORLD_SIZE).max(0.1);
    let v_scale = (dims[1] / TILE_WORLD_SIZE).max(0.1);
    bevy::math::Affine2::from_scale(bevy::math::Vec2::new(u_scale, v_scale))
}

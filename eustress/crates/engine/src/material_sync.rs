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

/// When MaterialService finishes loading (registry grows OR shrinks), mark
/// all BaseParts as changed so `sync_basepart_to_material` re-resolves them
/// against the current registry.
///
/// Detecting size CHANGES (`!=`) rather than only GROWTH (`>`) is critical
/// after a Space switch: if the new Space has fewer materials than the
/// previous one, `current_count` is smaller than `last_count` and a
/// growth-only check never fires — leaving every Part in the new Space
/// rendered with the default fallback material. The user's "materials
/// don't load when switching Spaces" report traced to exactly this case.
/// Also reset `PatchedTextureTracker` so the texture-repeat patcher
/// re-runs against the new registry's materials.
fn reapply_materials_on_registry_change(
    material_registry: Option<Res<crate::space::material_loader::MaterialRegistry>>,
    mut tracker: ResMut<MaterialRegistryTracker>,
    mut patched: ResMut<PatchedTextureTracker>,
    mut base_parts: Query<&mut BasePart>,
    // Gate: while a Space load is still streaming, the MaterialRegistry grows on
    // EVERY spawn batch, so touching ALL parts on each incremental change was
    // O(N²) across the load (re-resolving every already-spawned part, per
    // batch). On Vehicle Simulator (387K parts) that dominated the frame and
    // made frame time GROW as parts accumulated (29s → 39s and climbing).
    // Freshly-spawned parts already resolve their material via Changed<BasePart>
    // at spawn, so the catch-all reapply only needs to run ONCE — after the load
    // settles (`LoadInProgress.active` flips false). Collapses O(N²) → O(N).
    load: Option<Res<crate::space::file_loader::LoadInProgress>>,
) {
    if load.map_or(false, |l| l.active) {
        return;
    }
    let Some(ref registry) = material_registry else { return };
    let current_count = registry.len();
    if current_count != tracker.last_count {
        info!("🎨 MaterialRegistry changed ({} → {}), re-applying materials to all parts",
              tracker.last_count, current_count);
        tracker.last_count = current_count;
        patched.patched_materials.clear();
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
    // R2.1: ResMut (was Res) — `sync` now resolves SHARED, deduplicated material
    // handles via the registry's dedup cache instead of cloning a unique
    // material per entity. A unique handle per entity defeats Bevy's batching
    // (batch key = material bind group + mesh) → one draw call per entity, the
    // ~60K-entity scale wall. Sharing collapses identical-appearance parts into
    // a handful of batched/indirect draws.
    mut material_registry: Option<ResMut<crate::space::material_loader::MaterialRegistry>>,
    query: Query<(
        Entity,
        &BasePart,
        &Transform,
        // Required so we only sync entities that already have a material (a
        // guard, not used directly now that we insert a fresh shared handle).
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
    for (entity, basepart, transform, _material_handle, has_no_shadow, has_transmission) in query.iter() {
        let is_glass = matches!(basepart.material, EustressMaterial::Glass);

        // ── R2.1: resolve a SHARED material handle (copy-on-write) ──
        // Replaces the former three branches (registry-clone ×2 + in-place
        // `get_mut`). `resolve_part_material` reproduces that exact tint math
        // (so visuals are unchanged — C2) but returns a deduplicated handle:
        // identical-appearance parts share ONE handle → Bevy batches them.
        // Editing one part re-resolves it to the handle matching its NEW look,
        // never mutating others (the deleted in-place `get_mut` was the classic
        // "edit one, change all" bug once handles are shared).
        // Gap 5 — respect embedded glTF materials. When the part opts in,
        // skip applying the single engine-derived StandardMaterial entirely
        // so the mesh keeps the material it loaded with (its own glTF
        // material). The shadow / transmission bookkeeping below still runs.
        if basepart.respect_gltf_materials {
            // fall through to shadow/transmission handling without touching
            // MeshMaterial3d
        } else if let Some(ref mut registry) = material_registry {
            // Custom `.mat.toml` name first, then the preset enum name — the
            // (possibly textured) base material to clone+tint, if registered.
            let mat_name = if basepart.material_name.is_empty() {
                format!("{:?}", basepart.material)
            } else {
                basepart.material_name.clone()
            };
            let base_template = registry
                .get(&mat_name)
                .or_else(|| registry.get(basepart.material.as_str()));
            let uv = compute_uv_transform(basepart, Some(transform.scale));
            let handle = registry.resolve_part_material(
                &mut materials,
                &mat_name,
                basepart.material,
                base_template,
                basepart.color,
                basepart.transparency,
                basepart.reflectance,
                uv,
            );
            commands.entity(entity).insert(MeshMaterial3d(handle));
        }
        // No registry resource yet (very early frames): leave the existing
        // handle; `reapply_materials_on_registry_change` re-runs sync once the
        // registry loads.

        // Shadow casting: respect the explicit `cast_shadow` property AND
        // the >= 50% transparency threshold. Either condition opts the
        // entity out of shadow cascades. At 50k+ anchored static parts the
        // shadow pass is the dominant render cost, so honouring the TOML
        // `cast_shadow = false` is a first-class perf knob, not a nice-to-have.
        let should_cast_shadow = basepart.cast_shadow && basepart.transparency < 0.5;
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

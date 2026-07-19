//! # Surface-placement tool — Decal / Texture
//!
//! Chosen from the radial media-import menu, this drives a "paste onto a
//! surface" interaction: a ghost preview follows the raycasted face under
//! the cursor, and a left-click applies the Decal/Texture to that part —
//! but ONLY if the part is a BasePart that isn't Locked. Esc / right-click
//! cancels (the modal-tool framework handles that + the cursor badge).
//!
//! ## Design
//!
//! The placement state lives in the [`SurfacePlacement`] resource. A
//! trivial [`SurfacePlaceGate`] `ModalTool` is activated purely so the
//! rest of the input stack (selection, gizmos) stands down while we own
//! the cursor, and so `cursor_badge` shows the paste badge — it does no
//! work itself. All the real logic (raycast, validity, preview quad,
//! click-to-place) is in [`run_surface_placement`], which has the query +
//! asset access the `ModalTool` callbacks lack.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::modal_tool::{
    ActivateModalToolEvent, ActiveModalTool, CancelModalToolEvent, ModalTool,
    ModalToolRegistry, ToolContext, ToolOptionControl, ToolStepResult, ViewportHit,
};

// ============================================================================
// State
// ============================================================================

/// Active surface-placement session. Set by [`begin_surface_placement`];
/// cleared when the user places or cancels.
#[derive(Resource, Default)]
pub struct SurfacePlacement {
    pub active: bool,
    /// Universe-relative asset path of the image being applied.
    pub rel_path: String,
    /// Texture (tiled) vs Decal (single projected image).
    pub is_texture: bool,
    /// The ghost preview quad + its material handle (spawned lazily).
    pub preview: Option<Entity>,
    pub preview_material: Option<Handle<StandardMaterial>>,
}

impl SurfacePlacement {
    fn reset(&mut self) {
        self.active = false;
        self.rel_path.clear();
        self.is_texture = false;
        self.preview = None;
        self.preview_material = None;
    }
}

/// Begin a surface-placement session for the given asset. Called from the
/// radial-choice dispatch (a `&mut World` command closure).
pub fn begin_surface_placement(world: &mut World, rel_path: String, is_texture: bool) {
    if let Some(mut sp) = world.get_resource_mut::<SurfacePlacement>() {
        // A fresh session — drop any stale preview handle (the entity, if
        // any, is despawned by the run system when active flips).
        sp.reset();
        sp.active = true;
        sp.rel_path = rel_path;
        sp.is_texture = is_texture;
    }
}

// ============================================================================
// Trivial gate ModalTool — owns the cursor + drives the badge, no logic
// ============================================================================

struct SurfacePlaceGate;

impl ModalTool for SurfacePlaceGate {
    fn id(&self) -> &'static str { "surface_place" }
    fn name(&self) -> &'static str { "Place on Surface" }
    fn step_label(&self) -> String { "click a surface to apply · Esc to cancel".to_string() }
    fn icon_path(&self) -> &'static str { "assets/icons/ui/cursor-badge-material-flip.svg" }
    fn options(&self) -> Vec<ToolOptionControl> { Vec::new() }
    // No-op: the dedicated `run_surface_placement` system does everything.
    fn on_click(&mut self, _hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        ToolStepResult::Continue
    }
    fn commit(&mut self, _world: &mut World) {}
    // Cancel (Esc / RMB / re-activate) tears the placement session down.
    fn cancel(&mut self, commands: &mut Commands) {
        commands.queue(|world: &mut World| {
            despawn_preview(world);
            if let Some(mut sp) = world.get_resource_mut::<SurfacePlacement>() {
                sp.reset();
            }
        });
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct SurfacePlacementPlugin;

impl Plugin for SurfacePlacementPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SurfacePlacement>()
            .add_systems(Startup, register_surface_tool)
            .add_systems(Update, (arm_surface_placement, run_surface_placement).chain())
            // Texture render + live UV tiling (see `sync_texture_surfaces`).
            .add_systems(Update, sync_texture_surfaces);
    }
}

fn register_surface_tool(mut registry: ResMut<ModalToolRegistry>) {
    registry.register("surface_place", || Box::new(SurfacePlaceGate));
}

/// When a placement session is active but the gate tool isn't the active
/// modal tool yet, activate it (once). Keeps the input stack gated + the
/// cursor badge showing for the duration of the session.
fn arm_surface_placement(
    placement: Res<SurfacePlacement>,
    active: Res<ActiveModalTool>,
    mut activate: MessageWriter<ActivateModalToolEvent>,
) {
    if placement.active && active.id() != Some("surface_place") {
        activate.write(ActivateModalToolEvent { tool_id: "surface_place".to_string() });
    }
}

// ============================================================================
// The driver — raycast, preview, validity, click-to-place
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn run_surface_placement(
    mut placement: ResMut<SurfacePlacement>,
    spatial_query: avian3d::prelude::SpatialQuery,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    mouse: Res<ButtonInput<MouseButton>>,
    base_parts: Query<(&eustress_common::classes::BasePart, &GlobalTransform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
    mut cancel_events: MessageWriter<CancelModalToolEvent>,
) {
    if !placement.active {
        return;
    }

    // Don't raycast / place while the pointer is in a UI text field.
    if ui_focus.as_ref().map(|f| f.text_input_focused).unwrap_or(false) {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else {
        set_preview_visible(&mut commands, &placement, false);
        return;
    };
    if let Some(vb) = viewport_bounds.as_deref() {
        let scale = window.scale_factor() as f32;
        if !vb.contains_logical(cursor_pos, scale) {
            set_preview_visible(&mut commands, &placement, false);
            return;
        }
    }

    let Some((camera, cam_tf)) = cameras.iter().find(|(c, _)| c.order == 0) else { return };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor_pos) else { return };

    // Raycast against scene colliders.
    let hit = {
        use avian3d::prelude::SpatialQueryFilter;
        use bevy::math::Dir3;
        Dir3::new(*ray.direction).ok().and_then(|dir| {
            spatial_query
                .ray_hits(ray.origin, dir, 10_000.0, 1, true, &SpatialQueryFilter::default())
                .first()
                .map(|h| (h.entity, ray.origin + *ray.direction * h.distance, h.normal))
        })
    };

    let Some((hit_entity, hit_point, hit_normal)) = hit else {
        // Nothing under the cursor — hide the preview.
        set_preview_visible(&mut commands, &placement, false);
        return;
    };

    // Validity: the hit must be a BasePart that isn't Locked.
    let part = base_parts.get(hit_entity).ok();
    let valid = part.map(|(bp, _)| !bp.locked).unwrap_or(false);

    // Update / spawn the ghost preview quad at the surface, tinted by validity.
    update_preview(
        &mut placement,
        &mut commands,
        &mut meshes,
        &mut materials,
        &asset_server,
        hit_point,
        hit_normal,
        valid,
    );

    // Left-click applies — but only on a valid unlocked part.
    if mouse.just_pressed(MouseButton::Left) && valid {
        if let Some((_, part_gt)) = part {
            let face = face_from_normal(hit_normal, part_gt.rotation());
            let rel_path = placement.rel_path.clone();
            let is_texture = placement.is_texture;
            commands.queue(move |world: &mut World| {
                place_on_part(world, hit_entity, rel_path, is_texture, face);
                despawn_preview(world);
                if let Some(mut sp) = world.get_resource_mut::<SurfacePlacement>() {
                    sp.reset();
                }
            });
            // Exit the gate tool now that we've placed.
            cancel_events.write(CancelModalToolEvent);
        }
    }
}

// ============================================================================
// Preview quad
// ============================================================================

/// Marker for the ghost preview quad so it's easy to find + despawn.
#[derive(Component)]
struct SurfacePreviewQuad;

fn update_preview(
    placement: &mut SurfacePlacement,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    point: Vec3,
    normal: Vec3,
    valid: bool,
) {
    // Orient a unit quad (normal +Z) to lie on the surface, nudged out
    // along the normal to avoid z-fighting.
    let n = normal.normalize_or_zero();
    if n == Vec3::ZERO {
        return;
    }
    // A `Rectangle`'s face normal is +Z; rotate +Z onto the surface normal.
    let rot = Quat::from_rotation_arc(Vec3::Z, n);
    let transform = Transform {
        translation: point + n * 0.02,
        rotation: rot,
        scale: Vec3::new(2.0, 2.0, 1.0),
    };

    let tint = if valid {
        Color::srgba(0.3, 1.0, 0.5, 0.55)
    } else {
        Color::srgba(1.0, 0.35, 0.35, 0.45)
    };

    match placement.preview {
        Some(e) => {
            // Move + retint the existing preview.
            commands.entity(e).insert((transform, Visibility::Visible));
            if let Some(mut mat) = placement.preview_material.as_ref().and_then(|h| materials.get_mut(h)) {
                mat.base_color = tint;
            }
        }
        None => {
            let mesh = meshes.add(Rectangle::new(1.0, 1.0));
            let material = materials.add(StandardMaterial {
                base_color: tint,
                base_color_texture: Some(asset_server.load(&placement.rel_path)),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                cull_mode: None,
                ..default()
            });
            let e = commands
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material.clone()),
                    transform,
                    SurfacePreviewQuad,
                    Name::new("SurfacePlacePreview"),
                ))
                .id();
            placement.preview = Some(e);
            placement.preview_material = Some(material);
        }
    }
}

fn set_preview_visible(commands: &mut Commands, placement: &SurfacePlacement, visible: bool) {
    if let Some(e) = placement.preview {
        commands.entity(e).insert(if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
    }
}

/// Despawn any live preview quad (called on place / cancel). Runs in a
/// `&mut World` context so it can find the marker without the resource.
fn despawn_preview(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<SurfacePreviewQuad>>();
    let entities: Vec<Entity> = q.iter(world).collect();
    for e in entities {
        world.entity_mut(e).despawn();
    }
}

// ============================================================================
// Placement — persist a Decal/Texture child under the part
// ============================================================================

/// Map a world-space surface normal to the target part's local Face enum
/// by picking the dominant axis of the normal in the part's local frame.
fn face_from_normal(
    world_normal: Vec3,
    part_rotation: Quat,
) -> eustress_common::classes::Face {
    use eustress_common::classes::Face;
    let local = part_rotation.inverse() * world_normal.normalize_or_zero();
    let ax = local.x.abs();
    let ay = local.y.abs();
    let az = local.z.abs();
    if ax >= ay && ax >= az {
        if local.x >= 0.0 { Face::Right } else { Face::Left }
    } else if ay >= ax && ay >= az {
        if local.y >= 0.0 { Face::Top } else { Face::Bottom }
    } else if local.z >= 0.0 {
        Face::Back
    } else {
        Face::Front
    }
}

/// Write a Decal/Texture child `_instance.toml` under the target part's
/// on-disk folder so it persists and the file-watcher spawns it as a child.
/// Falls back to a transient runtime spawn if the part has no on-disk folder.
fn place_on_part(
    world: &mut World,
    part_entity: Entity,
    rel_path: String,
    is_texture: bool,
    face: eustress_common::classes::Face,
) {
    let class_name = if is_texture { "Texture" } else { "Decal" };

    // Resolve the part's on-disk folder from its InstanceFile.
    let part_folder = world
        .get::<crate::space::instance_loader::InstanceFile>(part_entity)
        .and_then(|f| f.toml_path.parent().map(|p| p.to_path_buf()));

    let Some(part_folder) = part_folder else {
        // No backing folder (pure-runtime part) — skip persistent placement.
        notify(world, format!(
            "Placed {} needs a saved part; select a file-backed part.",
            class_name
        ));
        return;
    };

    let name = format!("{}{}", class_name, short_suffix(part_entity));
    let overrides = eustress_common::instance_create::InstanceOverrides {
        display_name: Some(name.clone()),
        ..Default::default()
    };
    let created = match eustress_common::instance_create::create_instance(
        &part_folder,
        class_name,
        Some(&name),
        overrides,
    ) {
        Ok(c) => c,
        Err(e) => {
            notify(world, format!("Failed to place {}: {}", class_name, e));
            return;
        }
    };

    // Patch the texture path + face into the class section. Both Decal and
    // Texture store the asset in `<section>.texture` with a string `face`.
    let section = if is_texture { "texture" } else { "decal" };
    let _ = crate::ui::file_event_handler::patch_toml_string_field(
        &created.toml_path, section, "texture", &rel_path,
    );
    let _ = crate::ui::file_event_handler::patch_toml_string_field(
        &created.toml_path, section, "face", face.as_str(),
    );

    notify(world, format!("Applied {} to surface ({} face)", class_name, face.as_str()));
    info!(
        "🖼️ Placed {} '{}' on part {:?} face {} (asset {})",
        class_name, created.folder_name, part_entity, face.as_str(), rel_path
    );
}

/// Short deterministic suffix from an entity so repeated placements on the
/// same part get distinct folder names before `create_instance`'s own
/// uniqueness pass.
fn short_suffix(e: Entity) -> String {
    format!("_{:x}", e.index().index() & 0xfff)
}

fn notify(world: &mut World, msg: String) {
    if let Some(mut n) = world.get_resource_mut::<crate::notifications::NotificationManager>() {
        n.info(msg);
    }
}

// ============================================================================
// Texture render — tiled quad on a face, with LIVE dynamic UV mapping
// ============================================================================

use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::math::{Affine2, Vec2};
use eustress_common::classes::{BasePart, Face, Texture};

/// Renders every `Texture` instance as a tiled quad laid on the chosen face
/// of its parent `BasePart`, and keeps its UV mapping LIVE: the quad size,
/// tile count (face-size ÷ studs-per-tile), UV offset, tint, and alpha are
/// recomputed EVERY frame from the current `Texture` fields + parent size,
/// so Properties-panel edits (StudsPerTileU/V, OffsetStudsU/V, Face, Color,
/// Transparency) and part resizes reflect in real time. The quad mesh +
/// material are created lazily on first sight (the loader only attaches the
/// `Texture` component). Textures are rare + user-placed, so recomputing
/// unconditionally is cheap and avoids missing parent-size changes that a
/// `Changed<Texture>` gate wouldn't catch.
fn sync_texture_surfaces(
    parents: Query<&BasePart>,
    mut q: Query<(
        Entity,
        &Texture,
        &ChildOf,
        Option<&MeshMaterial3d<StandardMaterial>>,
        &mut Transform,
    )>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    for (entity, texture, child_of, mat_opt, mut tf) in q.iter_mut() {
        let Ok(parent_bp) = parents.get(child_of.parent()) else { continue };
        let size = parent_bp.size;
        let (translation, rotation, dims) = face_placement(parse_face(&texture.face), size);

        // Position + size the quad on the face (local to the parent part).
        *tf = Transform {
            translation,
            rotation,
            scale: Vec3::new(dims.x.max(0.01), dims.y.max(0.01), 1.0),
        };

        // Lazily build the quad mesh + Repeat-sampled material.
        let mat_handle = match mat_opt {
            Some(m) => m.0.clone(),
            None => {
                let mesh = meshes.add(Rectangle::new(1.0, 1.0));
                let tex = asset_server.load_with_settings(
                    texture.texture.clone(),
                    |s: &mut ImageLoaderSettings| {
                        s.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                            address_mode_u: ImageAddressMode::Repeat,
                            address_mode_v: ImageAddressMode::Repeat,
                            address_mode_w: ImageAddressMode::Repeat,
                            ..ImageSamplerDescriptor::linear()
                        });
                    },
                );
                let handle = materials.add(StandardMaterial {
                    base_color_texture: Some(tex),
                    alpha_mode: AlphaMode::Blend,
                    cull_mode: None,
                    ..default()
                });
                commands.entity(entity).insert((
                    Mesh3d(mesh),
                    MeshMaterial3d(handle.clone()),
                    Visibility::Visible,
                ));
                handle
            }
        };

        // Live UV tiling + tint. Tiles = face extent ÷ studs-per-tile.
        if let Some(mut mat) = materials.get_mut(&mat_handle) {
            let spu = texture.studs_per_tile_u.max(0.01);
            let spv = texture.studs_per_tile_v.max(0.01);
            let tiles = Vec2::new(dims.x / spu, dims.y / spv);
            let offset = Vec2::new(texture.offset_studs_u / spu, texture.offset_studs_v / spv);
            mat.uv_transform = Affine2::from_scale_angle_translation(tiles, 0.0, offset);
            mat.base_color = Color::srgba(
                texture.color3[0],
                texture.color3[1],
                texture.color3[2],
                (1.0 - texture.transparency).clamp(0.0, 1.0),
            );
        }
    }
}

fn parse_face(s: &str) -> Face {
    match s {
        "Top" => Face::Top,
        "Bottom" => Face::Bottom,
        "Back" => Face::Back,
        "Left" => Face::Left,
        "Right" => Face::Right,
        _ => Face::Front,
    }
}

/// Local (to the parent part) placement of a face quad: its centre offset,
/// the rotation that turns a +Z-normal `Rectangle` to face outward, and the
/// face's (width, height) in studs. A small epsilon lifts the quad off the
/// surface to avoid z-fighting.
fn face_placement(face: Face, size: Vec3) -> (Vec3, Quat, Vec2) {
    use std::f32::consts::{FRAC_PI_2, PI};
    let e = 0.02;
    let (hx, hy, hz) = (size.x * 0.5, size.y * 0.5, size.z * 0.5);
    match face {
        Face::Back => (Vec3::new(0.0, 0.0, hz + e), Quat::IDENTITY, Vec2::new(size.x, size.y)),
        Face::Front => (Vec3::new(0.0, 0.0, -hz - e), Quat::from_rotation_y(PI), Vec2::new(size.x, size.y)),
        Face::Right => (Vec3::new(hx + e, 0.0, 0.0), Quat::from_rotation_y(FRAC_PI_2), Vec2::new(size.z, size.y)),
        Face::Left => (Vec3::new(-hx - e, 0.0, 0.0), Quat::from_rotation_y(-FRAC_PI_2), Vec2::new(size.z, size.y)),
        Face::Top => (Vec3::new(0.0, hy + e, 0.0), Quat::from_rotation_x(-FRAC_PI_2), Vec2::new(size.x, size.z)),
        Face::Bottom => (Vec3::new(0.0, -hy - e, 0.0), Quat::from_rotation_x(FRAC_PI_2), Vec2::new(size.x, size.z)),
    }
}

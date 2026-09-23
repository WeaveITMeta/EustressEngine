//! # Surface-placement tool: Decal, Texture, Image
//!
//! Chosen from the radial media-import menu, this drives a "paste onto a
//! surface" interaction: a ghost preview follows the raycasted surface under
//! the cursor, tinted green where a click would land and red where it would
//! not, and a left-click applies the image there. Esc or right-click cancels
//! (the modal-tool framework handles that and the cursor badge).
//!
//! ## Targets
//!
//! - **An unlocked BasePart**: a Decal or Texture becomes a surface property
//!   of the face under the cursor, an Image a quad parented to the part. The
//!   part must be saved, since the new instance is written into its folder.
//! - **The terrain** (a `TerrainChunkCollider`, or the drivable surface of a
//!   road on it): a Decal only. It is a `Decal` instance written directly
//!   under the Workspace, since the terrain root is respawned by
//!   regenerating and importing and would take its children with it. The
//!   decal lies in the plane fitted to the finished ground (the layer bake,
//!   through `surface_data`) under its footprint, centred on the cursor,
//!   with the image's top edge toward the top of the view. It is 4 m along
//!   its longer side, following the image's aspect. A Texture tiles across a
//!   part face and an Image hangs from a part, so neither goes on the
//!   terrain.
//!
//! Anything else under the cursor (a locked part, a scatter tree or rock) is
//! refused, and a click there says why in a notification.
//!
//! ## How a decal on the ground conforms to it
//!
//! Bevy's `ForwardDecal` is a flat quad drawn over the depth prepass: each
//! pixel shifts its UV by the parallax between the quad and the surface
//! behind it and fades with the distance between them, reaching zero at
//! `depth_fade_factor` metres. So the decal needs a depth prepass on the
//! camera, which every Studio camera has (`default_scene::studio_camera_bundle`
//! carries `DepthPrepass` and `Msaa::Off`), and it only lands on surfaces
//! that write the prepass: the terrain surface material, roads and scatter
//! are opaque and do, alpha-blended water does not. The placement fits the
//! plane over the footprint by least squares and sets the fade to four times
//! the farthest the ground strays from it (1 m to 8 m), so the image still
//! shows at three quarters strength over the roughest point and bleeds as
//! little as it can onto things standing on the ground.
//!
//! Bevy's shader turns that parallax into UVs by dividing by the model
//! matrix applied to (1, 1, 1), which is the scale only while the model is
//! unrotated; a decal tilted to a slope and turned to the view would get a
//! garbage parallax. So a standalone decal is drawn by a separate entity
//! with a translation-only transform, whose quad carries the decal's turn
//! and size in its vertices and its UVs in metres
//! ([`sync_standalone_decals`]). The same system draws every Decal that is
//! neither a part nor on one, however it was made.
//!
//! ## Design
//!
//! The placement state lives in the [`SurfacePlacement`] resource. A
//! trivial [`SurfacePlaceGate`] `ModalTool` is activated purely so the rest
//! of the input stack (selection, gizmos) stands down while we own the
//! cursor, and so `cursor_badge` shows the paste badge; it does no work
//! itself. All the real logic (raycast, validity, preview quad,
//! click-to-place) is in [`run_surface_placement`], which has the query and
//! asset access the `ModalTool` callbacks lack.

use std::path::Path;

use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::ecs::system::SystemParam;
use bevy::mesh::VertexAttributeValues;
use bevy::pbr::decal::{ForwardDecal, ForwardDecalMaterial, ForwardDecalMaterialExt};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use eustress_common::terrain::road_surface::RoadSurface;
use eustress_common::terrain::{
    height_at_world, surface_data, TerrainBaked, TerrainChunkCollider, TerrainConfig, TerrainData, TerrainRoot,
};

use crate::modal_tool::{
    ActivateModalToolEvent, ActiveModalTool, CancelModalToolEvent, ModalTool,
    ModalToolRegistry, ToolContext, ToolOptionControl, ToolStepResult, ViewportHit,
};

// ============================================================================
// State
// ============================================================================

/// What a surface placement produces.
///
/// All three project the same asset onto a face; they differ in how the
/// result is authored. Decal and Texture become surface properties of the
/// part, while Image becomes a real 3D quad parented to it, so it can be
/// moved, rotated and scaled with the normal Studio tools afterwards.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SurfaceMediaKind {
    /// Single projected image, stored as a `[decal]` surface property.
    #[default]
    Decal,
    /// Repeating image, stored as a `[texture]` surface property with tiling.
    Texture,
    /// Standalone textured quad, written as an `Image` child of the part.
    Image,
}

impl SurfaceMediaKind {
    /// Class name written to disk.
    pub fn class_name(self) -> &'static str {
        match self {
            Self::Decal => "Decal",
            Self::Texture => "Texture",
            Self::Image => "Image",
        }
    }

    /// Parse the radial menu's choice. Unknown values fall back to Decal.
    pub fn from_class_name(name: &str) -> Self {
        match name {
            "Texture" => Self::Texture,
            "Image" => Self::Image,
            _ => Self::Decal,
        }
    }
}

/// Active surface-placement session. Set by [`begin_surface_placement`];
/// cleared when the user places or cancels.
#[derive(Resource, Default)]
pub struct SurfacePlacement {
    pub active: bool,
    /// Universe-relative asset path of the image being applied.
    pub rel_path: String,
    /// What the placement produces on commit.
    pub kind: SurfaceMediaKind,
    /// The ghost preview quad + its material handle (spawned lazily).
    pub preview: Option<Entity>,
    pub preview_material: Option<Handle<StandardMaterial>>,
}

impl SurfacePlacement {
    fn reset(&mut self) {
        self.active = false;
        self.rel_path.clear();
        self.kind = SurfaceMediaKind::default();
        self.preview = None;
        self.preview_material = None;
    }
}

/// Begin a surface-placement session for the given asset. Called from the
/// radial-choice dispatch (a `&mut World` command closure).
pub fn begin_surface_placement(world: &mut World, rel_path: String, kind: SurfaceMediaKind) {
    if let Some(mut sp) = world.get_resource_mut::<SurfacePlacement>() {
        // A fresh session — drop any stale preview handle (the entity, if
        // any, is despawned by the run system when active flips).
        sp.reset();
        sp.active = true;
        sp.rel_path = rel_path;
        sp.kind = kind;
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
            .add_systems(Update, sync_texture_surfaces)
            // Decals that stand on their own, such as those on the terrain.
            .add_systems(Update, sync_standalone_decals);
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
    ground: TerrainGround,
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

    // What the click would land on: an unlocked part, the terrain (a Decal
    // only), or nothing, with the reason.
    let kind = placement.kind;
    let target = if let Ok((bp, part_gt)) = base_parts.get(hit_entity) {
        if bp.locked {
            SurfaceTarget::Refused(format!(
                "{} not applied: the part is locked. Unlock it or pick another surface.",
                kind.class_name()
            ))
        } else {
            SurfaceTarget::Part(*part_gt)
        }
    } else if ground.colliders.contains(hit_entity) {
        if kind == SurfaceMediaKind::Decal {
            // The image's aspect, once the preview has loaded it.
            let image_size = placement
                .preview_material
                .as_ref()
                .and_then(|handle| materials.get(handle))
                .and_then(|material| material.base_color_texture.as_ref())
                .and_then(|texture| ground.images.get(texture))
                .map(|image| image.size());
            let height = ground.roots.iter().next().map(|(config, base, baked)| {
                let data = surface_data(base, baked);
                move |x: f32, z: f32| height_at_world(config, data, x, z)
            });
            SurfaceTarget::Ground(fit_ground_pose(
                hit_point,
                hit_normal,
                decal_size(image_size),
                *cam_tf.up(),
                *cam_tf.forward(),
                height.as_ref().map(|h| h as &dyn Fn(f32, f32) -> f32),
            ))
        } else {
            SurfaceTarget::Refused(format!(
                "{} not applied: it needs a part face. Only a Decal goes on the terrain.",
                kind.class_name()
            ))
        }
    } else if kind == SurfaceMediaKind::Decal {
        SurfaceTarget::Refused("Decal not applied: pick an unlocked part or the terrain.".to_string())
    } else {
        SurfaceTarget::Refused(format!("{} not applied: pick an unlocked part.", kind.class_name()))
    };

    // Update / spawn the ghost preview quad at the surface, tinted by
    // validity. On the terrain it shows the decal's own footprint and turn,
    // lifted clear of the ground under it.
    let preview = match &target {
        SurfaceTarget::Ground(pose) => Some(ground_preview_transform(pose)),
        SurfaceTarget::Part(_) | SurfaceTarget::Refused(_) => part_preview_transform(hit_point, hit_normal),
    };
    if let Some(transform) = preview {
        update_preview(
            &mut placement,
            &mut commands,
            &mut meshes,
            &mut materials,
            &asset_server,
            transform,
            !matches!(target, SurfaceTarget::Refused(_)),
        );
    }

    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    match target {
        SurfaceTarget::Part(part_gt) => {
            let face = face_from_normal(hit_normal, part_gt.rotation());
            let rel_path = placement.rel_path.clone();
            commands.queue(move |world: &mut World| {
                place_on_part(world, hit_entity, rel_path, kind, face);
                despawn_preview(world);
                if let Some(mut sp) = world.get_resource_mut::<SurfacePlacement>() {
                    sp.reset();
                }
            });
            // Exit the gate tool now that we've placed.
            cancel_events.write(CancelModalToolEvent);
        }
        SurfaceTarget::Ground(pose) => {
            let rel_path = placement.rel_path.clone();
            commands.queue(move |world: &mut World| {
                place_on_terrain(world, rel_path, pose);
                despawn_preview(world);
                if let Some(mut sp) = world.get_resource_mut::<SurfacePlacement>() {
                    sp.reset();
                }
            });
            cancel_events.write(CancelModalToolEvent);
        }
        // The session stays open for another try.
        SurfaceTarget::Refused(reason) => {
            commands.queue(move |world: &mut World| refuse(world, reason));
        }
    }
}

/// The ground a Decal can land on besides a part: the terrain's chunk
/// colliders and the drivable surface of every road laid on it (a road
/// marking is a decal), the terrain whose finished surface the decal's plane
/// is fitted to, and the images whose aspect sizes the decal.
#[derive(SystemParam)]
struct TerrainGround<'w, 's> {
    colliders: Query<'w, 's, (), Or<(With<TerrainChunkCollider>, With<RoadSurface>)>>,
    roots: Query<
        'w,
        's,
        (&'static TerrainConfig, &'static TerrainData, Option<&'static TerrainBaked>),
        With<TerrainRoot>,
    >,
    images: Res<'w, Assets<Image>>,
}

/// What a click under the cursor lands on.
enum SurfaceTarget {
    /// An unlocked part, posed as given.
    Part(GlobalTransform),
    /// The terrain, for a Decal posed as given.
    Ground(GroundPose),
    /// Nothing placeable, and why.
    Refused(String),
}

// ============================================================================
// Decals on the terrain
// ============================================================================

/// Longer side of a decal placed on the terrain, metres; the shorter follows
/// the image's aspect.
const TERRAIN_DECAL_SIZE: f32 = 4.0;
/// Shortest side a decal is given, metres, so a sliver of an image still
/// makes a quad the mesh transforms can scale.
const MIN_DECAL_SIZE: f32 = 0.05;
/// Samples per side of the grid the ground plane is fitted over.
const GROUND_FIT_SAMPLES: usize = 7;
/// Farthest the raycast hit may lie from the heightfield surface before the
/// fit is not trusted, metres: a hit on an overhang or cave wall carved by
/// the terrain volume, or on a terrain without a height raster, keeps the
/// raycast's own point and normal.
const GROUND_FIT_TOLERANCE: f32 = 0.5;
/// The decal's fade distance per metre of relief under it. Its alpha falls
/// linearly to zero at the fade distance from its plane, so four times the
/// farthest the ground strays leaves three quarters of the image there.
const GROUND_FADE_PER_RELIEF: f32 = 4.0;
/// Limits of that fade, metres: the Decal class default, which also keeps a
/// decal on flat ground off things standing on it, and Bevy's own default.
const GROUND_FADE_MIN: f32 = 1.0;
const GROUND_FADE_MAX: f32 = 8.0;
/// How far the preview floats above the highest ground under it, metres.
const PREVIEW_LIFT: f32 = 0.02;

/// Where and how a Decal lands on the terrain.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GroundPose {
    /// Centre of the decal's plane: over the cursor, at the fitted height.
    center: Vec3,
    /// The plane's normal, which the decal's local +Y takes.
    normal: Vec3,
    /// The decal's rotation: +Y on the normal, the image's top edge (local
    /// -Z) toward the top of the view.
    rotation: Quat,
    /// Footprint along the image's width (local X) and height (local Z),
    /// metres.
    size: Vec2,
    /// Farthest the ground under the footprint strays from the plane, metres.
    relief: f32,
    /// The `depth_fade_factor` the decal projects with, metres.
    depth_fade: f32,
}

/// A decal's footprint for an image of `image_size` pixels:
/// [`TERRAIN_DECAL_SIZE`] along its longer side, the shorter in proportion;
/// square while the image is still loading.
fn decal_size(image_size: Option<UVec2>) -> Vec2 {
    let Some(image_size) = image_size else { return Vec2::splat(TERRAIN_DECAL_SIZE) };
    let (w, h) = (image_size.x.max(1) as f32, image_size.y.max(1) as f32);
    let size = if w >= h {
        Vec2::new(TERRAIN_DECAL_SIZE, TERRAIN_DECAL_SIZE * h / w)
    } else {
        Vec2::new(TERRAIN_DECAL_SIZE * w / h, TERRAIN_DECAL_SIZE)
    };
    size.max(Vec2::splat(MIN_DECAL_SIZE))
}

/// Pose a decal of `size` hit at `hit_point` on the ground. With `height_at`
/// (world XZ to the finished surface height) and a hit on that surface, the
/// plane is fitted to the ground under the footprint and the fade sized to
/// its relief; otherwise the raycast's point and `hit_normal` are the plane
/// and the fade is the minimum. `view_up` and `view_forward` turn the image
/// upright in the view.
fn fit_ground_pose(
    hit_point: Vec3,
    hit_normal: Vec3,
    size: Vec2,
    view_up: Vec3,
    view_forward: Vec3,
    height_at: Option<&dyn Fn(f32, f32) -> f32>,
) -> GroundPose {
    let (center, normal, relief) = height_at
        .and_then(|height_at| fit_ground_plane(hit_point, size, height_at))
        .unwrap_or((hit_point, hit_normal.normalize_or(Vec3::Y), 0.0));
    GroundPose {
        center,
        normal,
        rotation: decal_rotation(normal, view_up, view_forward),
        size,
        relief,
        depth_fade: (relief * GROUND_FADE_PER_RELIEF).clamp(GROUND_FADE_MIN, GROUND_FADE_MAX),
    }
}

/// The least-squares plane through the surface heights on a square grid
/// covering a footprint of `size` centred under `hit`: its centre over the
/// hit, its normal, and the farthest a sample strays from it measured along
/// that normal. `None` when the hit is not on the surface `height_at` holds.
fn fit_ground_plane(hit: Vec3, size: Vec2, height_at: &dyn Fn(f32, f32) -> f32) -> Option<(Vec3, Vec3, f32)> {
    if (height_at(hit.x, hit.z) - hit.y).abs() > GROUND_FIT_TOLERANCE {
        return None;
    }
    // The square covers the footprint whichever way the view turns it.
    let half = size.max_element() * 0.5;
    let n = GROUND_FIT_SAMPLES;
    let step = 2.0 * half / (n - 1) as f32;
    // Heights relative to the hit keep the sums small next to large
    // altitudes. On a grid symmetric about the hit the offsets sum to zero
    // and are uncorrelated, so the two slopes solve independently.
    let mut samples = Vec::with_capacity(n * n);
    let (mut sum_h, mut sum_xh, mut sum_zh, mut sum_xx, mut sum_zz) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for i in 0..n {
        for j in 0..n {
            let dx = -half + i as f32 * step;
            let dz = -half + j as f32 * step;
            let h = height_at(hit.x + dx, hit.z + dz) - hit.y;
            samples.push((dx, dz, h));
            sum_h += h;
            sum_xh += dx * h;
            sum_zh += dz * h;
            sum_xx += dx * dx;
            sum_zz += dz * dz;
        }
    }
    let mean = sum_h / (n * n) as f32;
    let slope_x = if sum_xx > 0.0 { sum_xh / sum_xx } else { 0.0 };
    let slope_z = if sum_zz > 0.0 { sum_zh / sum_zz } else { 0.0 };
    let normal = Vec3::new(-slope_x, 1.0, -slope_z).normalize();
    // A vertical gap times the normal's Y is the distance along the normal.
    let relief = samples
        .iter()
        .map(|&(dx, dz, h)| (h - (mean + slope_x * dx + slope_z * dz)).abs())
        .fold(0.0f32, f32::max)
        * normal.y;
    Some((Vec3::new(hit.x, hit.y + mean, hit.z), normal, relief))
}

/// The rotation of a decal lying on a plane with `normal`. Bevy's forward
/// decal quad lies in its local XZ plane facing +Y, with the image's top edge
/// toward local -Z, so +Y goes to the normal and -Z to the view's up
/// flattened onto the plane: the image reads upright from where it was
/// placed. A view looking along the plane uses its forward instead, which
/// flattens to the same direction whenever both are defined.
fn decal_rotation(normal: Vec3, view_up: Vec3, view_forward: Vec3) -> Quat {
    let n = normal.normalize_or(Vec3::Y);
    let flatten = |v: Vec3| {
        let v = v - n * v.dot(n);
        (v.length_squared() > 1e-6).then(|| v.normalize())
    };
    let top = flatten(view_up)
        .or_else(|| flatten(view_forward))
        .unwrap_or_else(|| n.any_orthonormal_vector());
    let z = -top;
    Quat::from_mat3(&Mat3::from_cols(n.cross(z), n, z))
}

/// The preview quad (a `Rectangle`, facing +Z with the image's top edge
/// toward +Y) laid over a decal's footprint on the ground.
fn ground_preview_transform(pose: &GroundPose) -> Transform {
    Transform {
        translation: pose.center + pose.normal * (pose.relief + PREVIEW_LIFT),
        // Turns the rectangle's +Z onto the decal's +Y and its +Y onto the
        // decal's -Z, the image's top edge.
        rotation: pose.rotation * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        scale: Vec3::new(pose.size.x, pose.size.y, 1.0),
    }
}

/// Write a Decal instance on the terrain under the Workspace, posed by
/// `pose`, projecting the image at `rel_path`. The file watcher spawns it
/// and [`sync_standalone_decals`] draws it.
fn place_on_terrain(world: &mut World, rel_path: String, pose: GroundPose) {
    let Some(workspace) = world
        .get_resource::<crate::space::SpaceRoot>()
        .map(|root| root.0.join("Workspace"))
    else {
        refuse(world, "Decal not applied: no Space is open.".to_string());
        return;
    };
    let stem = Path::new(&rel_path).file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let name = if stem.is_empty() { "Decal".to_string() } else { format!("Decal_{stem}") };
    let overrides = crate::space::instance_create::InstanceOverrides {
        display_name: Some(name.clone()),
        position: Some(pose.center),
        rotation: Some(pose.rotation.to_array()),
        // A decal is flat: its size is X and Z, and Y scales nothing.
        scale: Some(Vec3::new(pose.size.x, 1.0, pose.size.y)),
        ..Default::default()
    };
    let created = match crate::space::instance_create::create_instance(&workspace, "Decal", Some(name.as_str()), overrides) {
        Ok(created) => created,
        Err(e) => {
            notify(world, format!("Failed to place Decal: {}", e));
            return;
        }
    };
    if let Err(e) = write_decal_section(&created.toml_path, &rel_path, Face::Top, pose.depth_fade) {
        warn!("placed a Decal on the terrain but could not write its [decal] section: {}", e);
    }
    notify(world, format!("Applied Decal to the terrain ({:.1} x {:.1} m)", pose.size.x, pose.size.y));
    info!(
        "Placed Decal '{}' on the terrain at {:?} (asset {}, fade {:.2} m)",
        created.folder_name, pose.center, rel_path, pose.depth_fade
    );
}

/// Write the `[decal]` keys a placement decides: the image, the face it looks
/// out of and the fade distance. The template's other keys (colour,
/// transparency, z-index) are left as they are.
fn write_decal_section(toml_path: &Path, texture: &str, face: Face, depth_fade: f32) -> Result<(), String> {
    let text = std::fs::read_to_string(toml_path).map_err(|e| format!("read {:?}: {}", toml_path, e))?;
    let mut doc: toml::Value = text
        .parse()
        .map_err(|e: toml::de::Error| format!("parse {:?}: {}", toml_path, e))?;
    let root = doc
        .as_table_mut()
        .ok_or_else(|| format!("TOML root is not a table: {:?}", toml_path))?;
    let section = root
        .entry("decal".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or("decal is not a table")?;
    section.insert("texture".into(), toml::Value::String(texture.to_string()));
    section.insert("face".into(), toml::Value::String(face.as_str().to_string()));
    section.insert("depth_fade_factor".into(), toml::Value::Float(depth_fade as f64));
    let out = toml::to_string_pretty(&doc).map_err(|e| format!("serialize {:?}: {}", toml_path, e))?;
    std::fs::write(toml_path, out).map_err(|e| format!("write {:?}: {}", toml_path, e))
}

// ============================================================================
// Preview quad
// ============================================================================

/// Marker for the ghost preview quad so it's easy to find + despawn.
#[derive(Component)]
struct SurfacePreviewQuad;

/// The preview quad on a part's surface (or wherever a placement is
/// refused): a unit quad (normal +Z) lying on the surface, nudged out along
/// the normal to avoid z-fighting. `None` for a degenerate normal.
fn part_preview_transform(point: Vec3, normal: Vec3) -> Option<Transform> {
    let n = normal.normalize_or_zero();
    if n == Vec3::ZERO {
        return None;
    }
    // A `Rectangle`'s face normal is +Z; rotate +Z onto the surface normal.
    let rot = Quat::from_rotation_arc(Vec3::Z, n);
    Some(Transform {
        translation: point + n * 0.02,
        rotation: rot,
        scale: Vec3::new(2.0, 2.0, 1.0),
    })
}

/// Move the ghost preview quad to `transform`, spawning it on first use, and
/// tint it by whether a click there would place.
fn update_preview(
    placement: &mut SurfacePlacement,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    transform: Transform,
    valid: bool,
) {
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
    kind: SurfaceMediaKind,
    face: eustress_common::classes::Face,
) {
    let class_name = kind.class_name();

    // Resolve the part's on-disk folder from its InstanceFile.
    let part_folder = world
        .get::<crate::space::instance_loader::InstanceFile>(part_entity)
        .and_then(|f| f.toml_path.parent().map(|p| p.to_path_buf()));

    let Some(part_folder) = part_folder else {
        // No backing folder (pure-runtime part) — skip persistent placement.
        let elsewhere = if kind == SurfaceMediaKind::Decal { ", or the terrain" } else { "" };
        refuse(world, format!(
            "{} not applied: the part has no file of its own. Pick a saved part{}.",
            class_name, elsewhere
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

    match kind {
        // Decal / Texture are SURFACE PROPERTIES: the asset lives in
        // `<section>.texture` with a string `face`, and the renderer builds
        // the quad from the parent's own dimensions.
        SurfaceMediaKind::Decal | SurfaceMediaKind::Texture => {
            let section = if kind == SurfaceMediaKind::Texture { "texture" } else { "decal" };
            let _ = crate::ui::file_event_handler::patch_toml_string_field(
                &created.toml_path, section, "texture", &rel_path,
            );
            let _ = crate::ui::file_event_handler::patch_toml_string_field(
                &created.toml_path, section, "face", face.as_str(),
            );
        }
        // Image is a REAL 3D QUAD parented to the part. It carries its own
        // Transform, so once placed it is an ordinary object the Move /
        // Rotate / Scale tools operate on — which is the point of offering
        // it as a surface target rather than only a Decal.
        //
        // The transform is written in the PARENT'S LOCAL SPACE because the
        // instance folder is created underneath the part: `face_placement`
        // returns the local offset (already nudged clear of the surface to
        // avoid z-fighting), the rotation that turns the quad's +Z to face
        // outward, and the face's own width/height so the image defaults to
        // covering the whole face. Scale x/y ARE the world size for this
        // mesh (a unit quad), which is also what the Scale gizmo maintains.
        SurfaceMediaKind::Image => {
            let part_size = world
                .get::<eustress_common::classes::BasePart>(part_entity)
                .map(|b| b.size)
                .unwrap_or(Vec3::ONE);
            let (offset, rot, dims) = face_placement(face, part_size);
            let _ = crate::ui::file_event_handler::patch_toml_string_field(
                &created.toml_path, "asset", "path", &rel_path,
            );
            if let Err(e) = write_quad_transform(&created.toml_path, offset, rot, dims) {
                warn!("placed Image but could not write its transform: {}", e);
            }
        }
    }

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

/// Say why a click placed nothing, as a warning.
fn refuse(world: &mut World, msg: String) {
    if let Some(mut n) = world.get_resource_mut::<crate::notifications::NotificationManager>() {
        n.warning(msg);
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
/// Write `[transform]` for a placed Image quad, in the parent part's local
/// space. Values that are not transform keys are left untouched, so the
/// class template's own defaults survive.
fn write_quad_transform(
    toml_path: &std::path::Path,
    position: Vec3,
    rotation: Quat,
    dims: Vec2,
) -> Result<(), String> {
    let text = std::fs::read_to_string(toml_path)
        .map_err(|e| format!("read {:?}: {}", toml_path, e))?;
    let mut doc: toml::Value = text
        .parse()
        .map_err(|e: toml::de::Error| format!("parse {:?}: {}", toml_path, e))?;
    let root = doc
        .as_table_mut()
        .ok_or_else(|| format!("TOML root is not a table: {:?}", toml_path))?;
    let tf = root
        .entry("transform".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or("transform is not a table")?;

    let f = |v: f32| toml::Value::Float(v as f64);
    tf.insert("position".into(), toml::Value::Array(vec![f(position.x), f(position.y), f(position.z)]));
    tf.insert(
        "rotation".into(),
        toml::Value::Array(vec![f(rotation.x), f(rotation.y), f(rotation.z), f(rotation.w)]),
    );
    // z stays 1: the quad mesh is flat, so only x/y carry size.
    tf.insert("scale".into(), toml::Value::Array(vec![f(dims.x), f(dims.y), f(1.0)]));

    let out = toml::to_string_pretty(&doc)
        .map_err(|e| format!("serialize {:?}: {}", toml_path, e))?;
    std::fs::write(toml_path, out).map_err(|e| format!("write {:?}: {}", toml_path, e))
}

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

// ============================================================================
// Standalone decals: a Decal that is neither a part nor on one
// ============================================================================

use eustress_common::classes::{Decal, Instance};

/// Rotation and scale changes smaller than this leave a standalone decal's
/// quad as it is.
const DECAL_POSE_EPSILON: f32 = 1e-5;

/// On a standalone Decal instance: the entity drawing it and what its quad
/// was built for.
#[derive(Component)]
struct StandaloneDecalRender {
    visual: Entity,
    /// World rotation and scale the quad's vertices carry.
    rotation: Quat,
    scale: Vec3,
    /// Whether the visual was last shown.
    shown: bool,
    mesh: Handle<Mesh>,
    material: Handle<ForwardDecalMaterial<StandardMaterial>>,
}

/// On the entity drawing a standalone decal: the Decal instance it draws.
/// Not an instance itself, so the Explorer, picking and save ignore it.
#[derive(Component)]
struct StandaloneDecalVisual {
    owner: Entity,
}

/// Draw every Decal instance that stands on its own: one that is not a part
/// and whose parent is not a part (a decal on the terrain, under the
/// Workspace, a Folder or a Model), projected from its own Transform, while
/// its texture is set. Its `Face` means nothing here; the Transform's
/// rotation turns it, and its X and Z scale are its footprint.
///
/// Each gets a separate visual entity, a `ForwardDecal` whose transform is
/// only the decal's world translation and whose quad carries its rotation
/// and scale in the vertices, with UVs in metres that the material's
/// `uv_transform` brings back to the image. Bevy's forward-decal shader
/// derives its parallax scale from the model matrix applied to (1, 1, 1),
/// which is the scale only for an unrotated model, and a decal turned onto a
/// slope would otherwise get a garbage parallax (see the module docs). The
/// quad is rebuilt when the decal turns or is resized, the material when its
/// `Decal` changes, and the visual follows the decal's position and
/// visibility. A visual whose decal is gone, or stopped standing on its own,
/// is despawned.
///
/// Decals that already draw themselves (the importer's decals on a part,
/// each a `ForwardDecal` child of its own) are left alone, and a decal on a
/// part is drawn by the part-face path, not here.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_standalone_decals(
    parts: Query<(), With<BasePart>>,
    mut decals: Query<
        (
            Entity,
            Ref<Decal>,
            Ref<GlobalTransform>,
            Option<&InheritedVisibility>,
            Option<&ChildOf>,
            Option<&mut StandaloneDecalRender>,
        ),
        (With<Instance>, Without<BasePart>, Without<ForwardDecal>),
    >,
    visuals: Query<(Entity, &StandaloneDecalVisual)>,
    cameras: Query<(&Camera, Has<DepthPrepass>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    decal_materials: Option<ResMut<Assets<ForwardDecalMaterial<StandardMaterial>>>>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
    mut warned_no_prepass: Local<bool>,
) {
    let Some(mut decal_materials) = decal_materials else { return };

    // Visuals whose decal is gone, or no longer draws through this one.
    for (visual, link) in &visuals {
        let current = decals
            .get(link.owner)
            .ok()
            .and_then(|(_, _, _, _, _, render)| render.map(|render| render.visual));
        if current != Some(visual) {
            commands.entity(visual).try_despawn();
        }
    }

    for (entity, decal, transform, visibility, child_of, render) in &mut decals {
        let standalone = !child_of.is_some_and(|child_of| parts.contains(child_of.parent()));
        let wanted = standalone && !decal.texture.is_empty();
        let shown = visibility.is_none_or(|visibility| visibility.get());
        let (scale, rotation, translation) = transform.to_scale_rotation_translation();
        match render {
            Some(render) if !wanted => {
                commands.entity(render.visual).try_despawn();
                commands.entity(entity).try_remove::<StandaloneDecalRender>();
            }
            Some(mut render) if visuals.contains(render.visual) => {
                let reshaped = !rotation.abs_diff_eq(render.rotation, DECAL_POSE_EPSILON)
                    || !scale.abs_diff_eq(render.scale, DECAL_POSE_EPSILON);
                if reshaped {
                    if let Some(mut mesh) = meshes.get_mut(&render.mesh) {
                        *mesh = standalone_decal_mesh(rotation, scale);
                    }
                    render.rotation = rotation;
                    render.scale = scale;
                }
                if reshaped || decal.is_changed() {
                    if let Some(mut material) = decal_materials.get_mut(&render.material) {
                        *material = standalone_decal_material(&decal, decal_footprint(scale), &asset_server);
                    }
                }
                if transform.is_changed() || render.shown != shown {
                    commands
                        .entity(render.visual)
                        .try_insert((Transform::from_translation(translation), visibility_for(shown)));
                    render.shown = shown;
                }
            }
            // Not drawn; or spawned since the last run, when transform
            // propagation may not have placed it yet and the quad would flash
            // at the origin for a frame.
            _ if !wanted || transform.is_added() => {}
            // New, or its visual was despawned by something else.
            stale => {
                let mesh = meshes.add(standalone_decal_mesh(rotation, scale));
                let material =
                    decal_materials.add(standalone_decal_material(&decal, decal_footprint(scale), &asset_server));
                let visual = commands
                    .spawn((
                        ForwardDecal,
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_translation(translation),
                        visibility_for(shown),
                        StandaloneDecalVisual { owner: entity },
                        Name::new("DecalVisual"),
                    ))
                    .id();
                let fresh = StandaloneDecalRender { visual, rotation, scale, shown, mesh, material };
                match stale {
                    Some(mut render) => *render = fresh,
                    None => {
                        commands.entity(entity).try_insert(fresh);
                    }
                }
                // A forward decal reads the depth prepass; without one on the
                // scene camera it draws as a flat, unfaded quad.
                let prepass = cameras.iter().find(|(camera, _)| camera.order == 0).map(|(_, prepass)| prepass);
                if prepass == Some(false) && !*warned_no_prepass {
                    warn!("the scene camera has no DepthPrepass, so decals do not conform to the surfaces under them");
                    *warned_no_prepass = true;
                }
            }
        }
    }
}

fn visibility_for(shown: bool) -> Visibility {
    if shown {
        Visibility::Visible
    } else {
        Visibility::Hidden
    }
}

/// A decal's footprint from its world scale: X across the image, Z down it.
fn decal_footprint(scale: Vec3) -> Vec2 {
    Vec2::new(scale.x.abs(), scale.z.abs()).max(Vec2::splat(MIN_DECAL_SIZE))
}

/// The quad a standalone decal draws with: Bevy's own forward-decal quad (1
/// m, in local XZ, facing +Y, the image's top edge toward -Z) sized to the
/// footprint of `scale` and turned by `rotation`, with its UVs in metres.
fn standalone_decal_mesh(rotation: Quat, scale: Vec3) -> Mesh {
    let size = decal_footprint(scale);
    let mut mesh = Mesh::from(Rectangle::from_size(Vec2::ONE))
        .rotated_by(Quat::from_rotation_arc(Vec3::Z, Vec3::Y))
        .scaled_by(Vec3::new(size.x, 1.0, size.y))
        .rotated_by(rotation);
    if let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        for uv in uvs.iter_mut() {
            uv[0] *= size.x;
            uv[1] *= size.y;
        }
    }
    // The shader builds its tangent frame from these; a quad always has them.
    if let Err(e) = mesh.generate_tangents() {
        warn!("decal quad tangents: {}", e);
    }
    mesh
}

/// The forward-decal material of a standalone decal whose quad has UVs in
/// metres over a footprint of `size`: `uv_transform` scales them back onto
/// the image, after the shader's parallax (in metres too, since the visual's
/// model matrix is unscaled) has shifted them. Tint and transparency as the
/// part-face decals have them.
fn standalone_decal_material(
    decal: &Decal,
    size: Vec2,
    asset_server: &AssetServer,
) -> ForwardDecalMaterial<StandardMaterial> {
    ForwardDecalMaterial {
        base: StandardMaterial {
            base_color: Color::srgba(decal.color[0], decal.color[1], decal.color[2], decal.color[3] * decal.alpha()),
            base_color_texture: Some(asset_server.load(&decal.texture)),
            alpha_mode: AlphaMode::Blend,
            uv_transform: Affine2::from_scale(Vec2::ONE / size),
            ..default()
        },
        extension: ForwardDecalMaterialExt {
            depth_fade_factor: decal.depth_fade_factor,
        },
    }
}

/// Hot reload: re-read the `[decal]` section of a Decal instance whose file
/// changed on disk (an MCP edit, a text editor, git), for a decal that
/// stands on its own and so carries a `Decal` component; its visual redraws
/// from it. Queued as an entity command so the file watcher needs no new
/// system parameters. A file without the section changes nothing.
pub fn queue_decal_reload(commands: &mut Commands, entity: Entity, toml_text: &str) {
    // Every instance file edit comes through here; most name no Decal.
    if !toml_text.contains("Decal") {
        return;
    }
    let Ok(mut doc) = toml_text.parse::<toml::Value>() else { return };
    // The loader accepts PascalCase keys too.
    eustress_common::class_schema::normalise_keys(&mut doc);
    let is_decal = doc
        .get("metadata")
        .and_then(|m| m.get("class_name"))
        .and_then(|v| v.as_str())
        == Some("Decal");
    let Some(section) = doc.get("decal").and_then(|v| v.as_table()) else { return };
    if !is_decal {
        return;
    }
    let fresh = crate::space::instance_loader::decal_from_section(section);
    commands.entity(entity).queue(move |mut entity: EntityWorldMut| {
        if entity.contains::<Decal>() {
            entity.insert(fresh);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn a_decal_on_a_slope_lies_in_its_plane_and_reads_upright() {
        // A plane rising half a metre per metre along +X.
        let slope = |x: f32, _z: f32| 10.0 + 0.5 * x;
        let hit = Vec3::new(2.0, slope(2.0, 3.0), 3.0);
        // Looking along -Z from above, across the slope.
        let up = Vec3::new(0.0, 0.8, -0.6);
        let forward = Vec3::new(0.0, -0.6, -0.8);
        let pose = fit_ground_pose(hit, Vec3::Y, Vec2::splat(4.0), up, forward, Some(&slope));

        let normal = Vec3::new(-0.5, 1.0, 0.0).normalize();
        assert!(close(pose.normal, normal), "normal {}", pose.normal);
        assert!(close(pose.center, hit), "centre {}", pose.center);
        assert!(pose.relief < 1e-3, "a plane has no relief: {}", pose.relief);
        assert_eq!(pose.depth_fade, GROUND_FADE_MIN);
        // Local +Y on the normal, the image's top (local -Z) toward the view's
        // up flattened onto the plane.
        assert!(close(pose.rotation * Vec3::Y, normal));
        let top = (up - normal * up.dot(normal)).normalize();
        assert!(close(pose.rotation * Vec3::NEG_Z, top), "top {}", pose.rotation * Vec3::NEG_Z);
    }

    #[test]
    fn looking_straight_down_leaves_a_flat_decal_unturned() {
        let flat = |_x: f32, _z: f32| -> f32 { 0.0 };
        let pose = fit_ground_pose(Vec3::ZERO, Vec3::Y, Vec2::splat(4.0), Vec3::NEG_Z, Vec3::NEG_Y, Some(&flat));
        assert!(pose.rotation.abs_diff_eq(Quat::IDENTITY, 1e-5), "rotation {:?}", pose.rotation);
    }

    #[test]
    fn rough_ground_widens_the_fade_within_its_limits() {
        // A 0.3 m bump in the middle of flat ground.
        let bump = |x: f32, z: f32| -> f32 { if x.abs() < 0.5 && z.abs() < 0.5 { 0.3 } else { 0.0 } };
        let pose = fit_ground_pose(Vec3::ZERO, Vec3::Y, Vec2::splat(4.0), Vec3::NEG_Z, Vec3::NEG_Y, Some(&bump));
        assert!(pose.relief > 0.2 && pose.relief < 0.3, "relief {}", pose.relief);
        assert!((pose.depth_fade - pose.relief * GROUND_FADE_PER_RELIEF).abs() < 1e-5);

        // A 3 m spire, clicked on its top: the plane stays level near the
        // ground around it, and the fade stops at its limit.
        let spire = |x: f32, z: f32| -> f32 { if x.abs() < 0.1 && z.abs() < 0.1 { 3.0 } else { 0.0 } };
        let hit = Vec3::new(0.0, 3.0, 0.0);
        let pose = fit_ground_pose(hit, Vec3::Y, Vec2::splat(4.0), Vec3::NEG_Z, Vec3::NEG_Y, Some(&spire));
        assert!(close(pose.normal, Vec3::Y));
        assert!(pose.center.y < 0.1, "centre {}", pose.center);
        assert_eq!(pose.depth_fade, GROUND_FADE_MAX);
    }

    #[test]
    fn a_hit_off_the_heightfield_keeps_the_raycast_pose() {
        // An overhang carved above ground whose heightfield reads 0.
        let ground = |_x: f32, _z: f32| -> f32 { 0.0 };
        let hit = Vec3::new(1.0, 5.0, 1.0);
        let wall = Vec3::X;
        let pose = fit_ground_pose(hit, wall, Vec2::splat(4.0), Vec3::Y, Vec3::NEG_X, Some(&ground));
        assert!(close(pose.center, hit));
        assert!(close(pose.normal, wall));
        assert_eq!(pose.depth_fade, GROUND_FADE_MIN);
        // No terrain raster at all: the same.
        let pose = fit_ground_pose(hit, wall, Vec2::splat(4.0), Vec3::Y, Vec3::NEG_X, None);
        assert!(close(pose.center, hit) && close(pose.normal, wall));
    }

    #[test]
    fn a_decal_takes_the_images_aspect() {
        assert_eq!(decal_size(None), Vec2::splat(TERRAIN_DECAL_SIZE));
        assert_eq!(decal_size(Some(UVec2::new(200, 100))), Vec2::new(4.0, 2.0));
        assert_eq!(decal_size(Some(UVec2::new(100, 400))), Vec2::new(1.0, 4.0));
        assert_eq!(decal_size(Some(UVec2::new(4000, 1))), Vec2::new(4.0, MIN_DECAL_SIZE));
    }

    #[test]
    fn a_turned_decal_quad_carries_its_turn_and_size_with_uvs_in_metres() {
        let rotation = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.3);
        let mesh = standalone_decal_mesh(rotation, Vec3::new(4.0, 1.0, 2.0));
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|v| v.as_float3()).expect("positions");
        let normals = mesh.attribute(Mesh::ATTRIBUTE_NORMAL).and_then(|v| v.as_float3()).expect("normals");
        for p in positions {
            let local = rotation.inverse() * Vec3::from_array(*p);
            assert!(local.y.abs() < 1e-5, "flat in its plane: {local}");
            assert!((local.x.abs() - 2.0).abs() < 1e-4 && (local.z.abs() - 1.0).abs() < 1e-4, "corner {local}");
        }
        for n in normals {
            assert!(close(Vec3::from_array(*n), rotation * Vec3::Y));
        }
        let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {
            panic!("the quad has UVs");
        };
        let max = uvs.iter().fold(Vec2::ZERO, |m, uv| m.max(Vec2::from_array(*uv)));
        assert_eq!(max, Vec2::new(4.0, 2.0));
        assert!(mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some(), "tangents for the decal shader");
    }

    #[test]
    fn a_decal_section_round_trips_its_fade() {
        let mut section = toml::value::Table::new();
        section.insert("texture".into(), toml::Value::String("Assets/crack.png".into()));
        section.insert("depth_fade_factor".into(), toml::Value::Float(2.5));
        let decal = crate::space::instance_loader::decal_from_section(&section);
        assert_eq!(decal.texture, "Assets/crack.png");
        assert_eq!(decal.depth_fade_factor, 2.5);
        assert_eq!(decal.face, Face::Front);
    }
}

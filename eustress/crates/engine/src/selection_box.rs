// ============================================================================
// selection_box.rs — Mesh-based selection adornments
// ============================================================================
//
// Spawns wireframe mesh entities as children of selected parts.
// Adornments are non-interactive (no BasePart, no PartEntityMarker) —
// all click events pass through to the part underneath.
//
// Meshes are generated once on selection and only rebuilt when the part's
// transform or size changes. Despawned on deselect.
// ============================================================================

use bevy::prelude::*;
use bevy::light::NotShadowCaster;
use bevy::mesh::PrimitiveTopology;
use bevy::asset::RenderAssetUsages;
use std::collections::HashMap;

use crate::classes::{BasePart, Part, PartType};

// ============================================================================
// Constants
// ============================================================================

/// Scale factor for wireframe mesh (slightly larger than part to avoid z-fight)
const WIREFRAME_SCALE: f32 = 1.03;

/// Selection outline color (Eustress blue)
const SELECTION_COLOR: Color = Color::srgb(0.0, 0.6, 1.0);

/// Hover highlight color
const HOVER_COLOR: Color = Color::srgb(1.0, 0.85, 0.2);

/// Corner dot sphere radius as fraction of part size
const CORNER_DOT_FRACTION: f32 = 0.025;
const CORNER_DOT_MIN: f32 = 0.04;
const CORNER_DOT_MAX: f32 = 0.18;

/// Number of segments for circle approximation
const CIRCLE_SEGMENTS: u32 = 32;

// ============================================================================
// Components
// ============================================================================

/// Marker on the PART entity indicating it is selected.
/// SelectionSyncPlugin adds/removes this based on SelectionManager state.
#[derive(Component)]
pub struct Selected;

/// Marker on the PART entity indicating it is hovered (but not selected).
#[derive(Component)]
pub struct Hovered;

/// Marker on adornment child entities — identifies them for cleanup.
/// These entities are invisible to the Explorer (filtered by Adornment { meta: true }).
#[derive(Component)]
pub struct SelectionAdornment;

/// Marker on hover adornment child entities.
#[derive(Component)]
pub struct HoverAdornment;

// ============================================================================
// Resources
// ============================================================================

/// Shared material handles for selection adornments (created once).
#[derive(Resource)]
pub struct SelectionMaterials {
    pub selection: Handle<StandardMaterial>,
    pub selection_bright: Handle<StandardMaterial>,
    pub hover: Handle<StandardMaterial>,
    pub corner_dot: Handle<StandardMaterial>,
}

/// Cache of wireframe meshes keyed by (shape, quantized_size).
/// Identical parts share the same mesh handle.
#[derive(Resource, Default)]
pub struct WireframeMeshCache {
    cache: HashMap<WireframeCacheKey, Handle<Mesh>>,
    dot_mesh: Option<Handle<Mesh>>,
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct WireframeCacheKey {
    shape: ShapeKind,
    /// Size quantized to 2 decimal places to allow sharing
    sx: i32,
    sy: i32,
    sz: i32,
}

#[derive(Hash, Eq, PartialEq, Clone, Copy)]
enum ShapeKind {
    Box,
    Ball,
    Cylinder,
}

impl WireframeCacheKey {
    fn new(shape: ShapeKind, size: Vec3) -> Self {
        Self {
            shape,
            sx: (size.x * 100.0) as i32,
            sy: (size.y * 100.0) as i32,
            sz: (size.z * 100.0) as i32,
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct SelectionBoxPlugin;

impl Plugin for SelectionBoxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WireframeMeshCache>()
            .add_systems(Startup, create_selection_materials)
            .add_systems(
                PostUpdate,
                (
                    spawn_selection_adornments,
                    despawn_selection_adornments,
                    spawn_hover_adornments,
                    despawn_hover_adornments,
                    update_changed_adornments,
                ).chain(),
            );
    }
}

// ============================================================================
// Startup: Create shared materials
// ============================================================================

fn create_selection_materials(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let selection = materials.add(StandardMaterial {
        base_color: SELECTION_COLOR.with_alpha(0.4),
        emissive: LinearRgba::from(SELECTION_COLOR) * 5.0,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });

    let selection_bright = materials.add(StandardMaterial {
        base_color: Color::srgba(0.5, 0.9, 1.0, 0.3),
        emissive: LinearRgba::from(Color::srgb(0.5, 0.9, 1.0)) * 3.0,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });

    let hover = materials.add(StandardMaterial {
        base_color: HOVER_COLOR.with_alpha(0.12),
        emissive: LinearRgba::from(HOVER_COLOR) * 2.0,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });

    let corner_dot = materials.add(StandardMaterial {
        base_color: Color::srgba(0.9, 0.97, 1.0, 0.9),
        emissive: LinearRgba::from(Color::srgb(0.9, 0.97, 1.0)) * 3.0,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    commands.insert_resource(SelectionMaterials {
        selection,
        selection_bright,
        hover,
        corner_dot,
    });
}

// ============================================================================
// Systems: Spawn/Despawn adornments on selection change
// ============================================================================

/// Spawn wireframe adornment children for newly selected entities.
/// Runs only when `Selected` is added (event-driven, not every frame).
fn spawn_selection_adornments(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cache: ResMut<WireframeMeshCache>,
    materials: Option<Res<SelectionMaterials>>,
    query: Query<(Entity, &GlobalTransform, Option<&BasePart>, Option<&Part>), Added<Selected>>,
) {
    let Some(mats) = materials else { return };
    if query.is_empty() { return; }

    for (entity, _, base_part, part) in &query {
        let size = base_part.map(|bp| bp.size).unwrap_or(Vec3::ONE);
        let shape = part.map(|p| shape_kind(p.shape)).unwrap_or(ShapeKind::Box);

        // Get or create wireframe mesh
        let wireframe_handle = get_or_create_wireframe(
            &mut cache, &mut meshes, shape, size,
        );

        // Spawn wireframe child entity
        commands.spawn((
            Mesh3d(wireframe_handle),
            MeshMaterial3d(mats.selection.clone()),
            Transform::from_scale(Vec3::splat(WIREFRAME_SCALE)),
            SelectionAdornment,
            eustress_common::adornments::Adornment { meta: true },
            NotShadowCaster,
            Name::new("SelectionWireframe"),
            ChildOf(entity),
        ));

        // Spawn corner dots for box shapes
        if shape == ShapeKind::Box {
            spawn_corner_dots(
                &mut commands,
                &mut cache,
                &mut meshes,
                &mats,
                entity,
                size,
            );
        }
    }
}

/// Despawn adornment children when `Selected` is removed.
fn despawn_selection_adornments(
    mut commands: Commands,
    mut removed: RemovedComponents<Selected>,
    adornment_query: Query<(Entity, &ChildOf), With<SelectionAdornment>>,
) {
    for removed_entity in removed.read() {
        // Find and despawn all adornment children of this entity
        for (adornment_entity, child_of) in &adornment_query {
            if child_of.parent() == removed_entity {
                commands.entity(adornment_entity).despawn();
            }
        }
    }
}

/// Spawn hover adornment for newly hovered entities.
fn spawn_hover_adornments(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cache: ResMut<WireframeMeshCache>,
    materials: Option<Res<SelectionMaterials>>,
    query: Query<(Entity, Option<&BasePart>, Option<&Part>), (Added<Hovered>, Without<Selected>)>,
) {
    let Some(mats) = materials else { return };

    for (entity, base_part, part) in &query {
        let size = base_part.map(|bp| bp.size).unwrap_or(Vec3::ONE);
        let shape = part.map(|p| shape_kind(p.shape)).unwrap_or(ShapeKind::Box);

        let wireframe_handle = get_or_create_wireframe(
            &mut cache, &mut meshes, shape, size,
        );

        commands.spawn((
            Mesh3d(wireframe_handle),
            MeshMaterial3d(mats.hover.clone()),
            Transform::from_scale(Vec3::splat(WIREFRAME_SCALE)),
            HoverAdornment,
            eustress_common::adornments::Adornment { meta: true },
            NotShadowCaster,
            Name::new("HoverWireframe"),
            ChildOf(entity),
        ));
    }
}

/// Despawn hover adornments when `Hovered` is removed.
fn despawn_hover_adornments(
    mut commands: Commands,
    mut removed: RemovedComponents<Hovered>,
    adornment_query: Query<(Entity, &ChildOf), With<HoverAdornment>>,
) {
    for removed_entity in removed.read() {
        for (adornment_entity, child_of) in &adornment_query {
            if child_of.parent() == removed_entity {
                commands.entity(adornment_entity).despawn();
            }
        }
    }
}

/// Update adornment meshes when the adorned part's shape type changes.
/// Size changes are handled automatically by parent transform propagation.
/// Only rebuilds when Part.shape actually changes (Block → Ball etc.).
fn update_changed_adornments(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cache: ResMut<WireframeMeshCache>,
    materials: Option<Res<SelectionMaterials>>,
    changed_parts: Query<(Entity, &BasePart, &Part), (With<Selected>, Changed<Part>)>,
    adornment_query: Query<(Entity, &ChildOf), With<SelectionAdornment>>,
) {
    let Some(mats) = materials else { return };

    for (part_entity, base_part, part) in &changed_parts {
        let size = base_part.size;
        let shape = shape_kind(part.shape);

        // Despawn old adornments
        for (adornment_entity, child_of) in &adornment_query {
            if child_of.parent() == part_entity {
                commands.entity(adornment_entity).despawn();
            }
        }

        // Spawn new with updated size
        let wireframe_handle = get_or_create_wireframe(
            &mut cache, &mut meshes, shape, size,
        );

        commands.spawn((
            Mesh3d(wireframe_handle),
            MeshMaterial3d(mats.selection.clone()),
            Transform::from_scale(Vec3::splat(WIREFRAME_SCALE)),
            SelectionAdornment,
            eustress_common::adornments::Adornment { meta: true },
            NotShadowCaster,
            Name::new("SelectionWireframe"),
            ChildOf(part_entity),
        ));

        if shape == ShapeKind::Box {
            spawn_corner_dots(
                &mut commands,
                &mut cache,
                &mut meshes,
                &mats,
                part_entity,
                size,
            );
        }
    }
}

// ============================================================================
// Wireframe Mesh Generation
// ============================================================================

fn get_or_create_wireframe(
    cache: &mut WireframeMeshCache,
    meshes: &mut Assets<Mesh>,
    shape: ShapeKind,
    size: Vec3,
) -> Handle<Mesh> {
    let key = WireframeCacheKey::new(shape, size);
    if let Some(handle) = cache.cache.get(&key) {
        return handle.clone();
    }

    let mesh = match shape {
        ShapeKind::Box => generate_box_wireframe(size),
        ShapeKind::Ball => generate_sphere_wireframe(size.x / 2.0),
        ShapeKind::Cylinder => generate_cylinder_wireframe(size.x / 2.0, size.y),
    };

    let handle = meshes.add(mesh);
    cache.cache.insert(key, handle.clone());
    handle
}

/// Generate a wireframe box mesh (12 edges = 24 vertices as LineList).
/// Size is in LOCAL space — the mesh is centered at origin.
/// The parent part's transform positions it in world space.
fn generate_box_wireframe(_size: Vec3) -> Mesh {
    // Wireframe is at unit scale since the parent transform already has the size.
    // We use half-extents of 0.5 (unit cube) because the parent's scale = BasePart.size.
    let h = Vec3::splat(0.5);

    let corners = [
        Vec3::new(-h.x, -h.y, -h.z), // 0
        Vec3::new( h.x, -h.y, -h.z), // 1
        Vec3::new(-h.x,  h.y, -h.z), // 2
        Vec3::new( h.x,  h.y, -h.z), // 3
        Vec3::new(-h.x, -h.y,  h.z), // 4
        Vec3::new( h.x, -h.y,  h.z), // 5
        Vec3::new(-h.x,  h.y,  h.z), // 6
        Vec3::new( h.x,  h.y,  h.z), // 7
    ];

    let edges: [(usize, usize); 12] = [
        (0,1),(2,3),(4,5),(6,7), // horizontal
        (0,2),(1,3),(4,6),(5,7), // vertical
        (0,4),(1,5),(2,6),(3,7), // depth
    ];

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(24);
    for (a, b) in &edges {
        positions.push(corners[*a].into());
        positions.push(corners[*b].into());
    }

    Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
}

/// Generate a wireframe sphere mesh (3 great circles).
fn generate_sphere_wireframe(_radius: f32) -> Mesh {
    // Unit sphere (radius 0.5) — parent scale handles actual size
    let r = 0.5;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(CIRCLE_SEGMENTS as usize * 2 * 3);

    // XY plane circle
    append_circle_line_list(&mut positions, Vec3::ZERO, Vec3::Z, r, CIRCLE_SEGMENTS);
    // XZ plane circle
    append_circle_line_list(&mut positions, Vec3::ZERO, Vec3::Y, r, CIRCLE_SEGMENTS);
    // YZ plane circle
    append_circle_line_list(&mut positions, Vec3::ZERO, Vec3::X, r, CIRCLE_SEGMENTS);

    Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
}

/// Generate a wireframe cylinder mesh (2 end circles + vertical lines).
fn generate_cylinder_wireframe(_radius: f32, _height: f32) -> Mesh {
    // Unit cylinder — parent scale handles actual size
    let r = 0.5;
    let half_h = 0.5;
    let mut positions: Vec<[f32; 3]> = Vec::new();

    // Top circle
    let top = Vec3::new(0.0, half_h, 0.0);
    append_circle_line_list(&mut positions, top, Vec3::Y, r, CIRCLE_SEGMENTS);

    // Bottom circle
    let bottom = Vec3::new(0.0, -half_h, 0.0);
    append_circle_line_list(&mut positions, bottom, Vec3::Y, r, CIRCLE_SEGMENTS);

    // 8 vertical lines connecting top to bottom
    for i in 0..8u32 {
        let angle = (i as f32 / 8.0) * std::f32::consts::TAU;
        let x = angle.cos() * r;
        let z = angle.sin() * r;
        positions.push([x, half_h, z]);
        positions.push([x, -half_h, z]);
    }

    Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
}

/// Append a circle as LineList segments to a positions buffer.
fn append_circle_line_list(
    positions: &mut Vec<[f32; 3]>,
    center: Vec3,
    normal: Vec3,
    radius: f32,
    segments: u32,
) {
    let up = if normal.dot(Vec3::Y).abs() > 0.99 { Vec3::X } else { Vec3::Y };
    let tangent = normal.cross(up).normalize();
    let bitangent = tangent.cross(normal).normalize();

    for i in 0..segments {
        let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
        let p0 = center + (tangent * a0.cos() + bitangent * a0.sin()) * radius;
        let p1 = center + (tangent * a1.cos() + bitangent * a1.sin()) * radius;
        positions.push(p0.into());
        positions.push(p1.into());
    }
}

// ============================================================================
// Corner Dot Spawning
// ============================================================================

/// Spawn 8 small cross-shaped meshes at box corners.
fn spawn_corner_dots(
    commands: &mut Commands,
    cache: &mut WireframeMeshCache,
    meshes: &mut Assets<Mesh>,
    mats: &SelectionMaterials,
    parent: Entity,
    size: Vec3,
) {
    let dot_mesh = get_or_create_dot_mesh(cache, meshes);
    let h = Vec3::splat(0.5); // Unit space

    let corners = [
        Vec3::new(-h.x, -h.y, -h.z),
        Vec3::new( h.x, -h.y, -h.z),
        Vec3::new(-h.x,  h.y, -h.z),
        Vec3::new( h.x,  h.y, -h.z),
        Vec3::new(-h.x, -h.y,  h.z),
        Vec3::new( h.x, -h.y,  h.z),
        Vec3::new(-h.x,  h.y,  h.z),
        Vec3::new( h.x,  h.y,  h.z),
    ];

    // Corner dot scale: fraction of part size, clamped.
    // Since parent transform has scale = size, we need to counter-scale.
    // Dot radius in world = size.max_element() * CORNER_DOT_FRACTION, clamped.
    let world_radius = (size.max_element() * CORNER_DOT_FRACTION)
        .max(CORNER_DOT_MIN)
        .min(CORNER_DOT_MAX);
    // In parent-local space, we need to divide by parent scale per axis
    // to get uniform world-space dots despite non-uniform parent scale.
    let local_scale = Vec3::new(
        world_radius / size.x.max(0.001),
        world_radius / size.y.max(0.001),
        world_radius / size.z.max(0.001),
    );

    for corner in &corners {
        commands.spawn((
            Mesh3d(dot_mesh.clone()),
            MeshMaterial3d(mats.corner_dot.clone()),
            Transform {
                translation: *corner,
                scale: local_scale,
                ..default()
            },
            SelectionAdornment,
            eustress_common::adornments::Adornment { meta: true },
            NotShadowCaster,
            Name::new("CornerDot"),
            ChildOf(parent),
        ));
    }
}

/// Get or create the corner dot mesh (a small 3-axis cross).
fn get_or_create_dot_mesh(
    cache: &mut WireframeMeshCache,
    meshes: &mut Assets<Mesh>,
) -> Handle<Mesh> {
    if let Some(ref handle) = cache.dot_mesh {
        return handle.clone();
    }

    // Cross shape: 3 lines (6 vertices) centered at origin, length 1.0
    let half = 0.5;
    let positions: Vec<[f32; 3]> = vec![
        [-half, 0.0, 0.0], [half, 0.0, 0.0], // X axis
        [0.0, -half, 0.0], [0.0, half, 0.0], // Y axis
        [0.0, 0.0, -half], [0.0, 0.0, half], // Z axis
    ];

    let mesh = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions);

    let handle = meshes.add(mesh);
    cache.dot_mesh = Some(handle.clone());
    handle
}

// ============================================================================
// Helpers
// ============================================================================

/// Cleanup: remove all selection adornments.
/// Called when deselecting everything or on app shutdown.
pub fn cleanup_all_selections(
    mut commands: Commands,
    selected: Query<Entity, With<Selected>>,
    hovered: Query<Entity, With<Hovered>>,
    adornments: Query<Entity, Or<(With<SelectionAdornment>, With<HoverAdornment>)>>,
) {
    for entity in &selected {
        commands.entity(entity).remove::<Selected>();
    }
    for entity in &hovered {
        commands.entity(entity).remove::<Hovered>();
    }
    for entity in &adornments {
        commands.entity(entity).despawn();
    }
}

fn shape_kind(part_type: PartType) -> ShapeKind {
    match part_type {
        PartType::Ball => ShapeKind::Ball,
        PartType::Cylinder | PartType::Cone => ShapeKind::Cylinder,
        _ => ShapeKind::Box,
    }
}

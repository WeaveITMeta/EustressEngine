//! # Terrain Water System — Hybrid Static + Dynamic
//!
//! ## Table of Contents
//! 1. Water Configuration — runtime resource for water state
//! 2. Water Plane Entity — static translucent plane at sea level
//! 3. Water Systems — spawn/update/remove water based on terrain config
//!
//! ## Design
//! - **Static water** (default): single translucent plane at `sea_level` Y
//!   - PBR material: blue-tinted, alpha blended, high reflectance, low roughness
//!   - Scales to cover entire terrain footprint
//!   - Efficient: single draw call
//! - **Dynamic water** (future): realism crate hydro simulation
//!   - Activated when water is "in motion" (rivers, waterfalls)
//!   - Particle-based for splashes and impacts
//!   - Transitions static ↔ dynamic based on velocity threshold

use bevy::prelude::*;

// ============================================================================
// 1. Water Configuration
// ============================================================================

/// Runtime water configuration resource
#[derive(Resource, Clone, Debug)]
pub struct WaterConfig {
    /// Whether water is enabled
    pub enabled: bool,
    /// Sea level in world Y coordinates
    pub sea_level: f32,
    /// Water mode: Static (plane) or Dynamic (simulation)
    pub mode: WaterMode,
    /// Water tint color [r, g, b, a]
    pub color: [f32; 4],
    /// Water plane opacity (0.0 = invisible, 1.0 = opaque)
    pub opacity: f32,
    /// Wave animation speed (0.0 = still water)
    pub wave_speed: f32,
    /// Wave amplitude in world units
    pub wave_amplitude: f32,
}

impl Default for WaterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sea_level: 0.0,
            mode: WaterMode::Static,
            color: [0.1, 0.3, 0.6, 0.8],
            opacity: 0.7,
            wave_speed: 0.0,
            wave_amplitude: 0.0,
        }
    }
}

/// Water rendering mode
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WaterMode {
    /// Static translucent plane at sea_level — single draw call
    #[default]
    Static,
    /// Dynamic simulation via realism crate (future)
    Dynamic,
}

// ============================================================================
// 2. Water Plane Entity
// ============================================================================

/// Marker component for the water plane entity
#[derive(Component, Default)]
pub struct WaterPlane;

/// Spawn a static water plane covering the terrain footprint
///
/// The plane is a flat quad at `sea_level` Y, sized to cover all terrain chunks.
/// Uses alpha-blended PBR material for translucent blue water appearance.
pub fn spawn_water_plane(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    config: &WaterConfig,
    terrain_size: f32, // Total terrain width/depth in world units
) -> Entity {
    // Create a flat plane mesh scaled to terrain size
    let mesh = meshes.add(Plane3d::default().mesh().size(terrain_size, terrain_size));

    // PBR water material: translucent, reflective, smooth
    let [r, g, b, a] = config.color;
    let alpha = a * config.opacity;
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(r, g, b, alpha),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.05,  // Very smooth (reflective water surface)
        metallic: 0.0,
        reflectance: 0.8,            // High reflectance for water
        double_sided: true,          // Visible from below (underwater view)
        cull_mode: None,             // Render both faces
        ..default()
    });

    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(Vec3::new(0.0, config.sea_level, 0.0)),
        Visibility::default(),
        WaterPlane,
        Name::new("WaterPlane"),
    )).id()
}

// ============================================================================
// 3. Water Systems
// ============================================================================

/// System to spawn/update water plane when terrain exists and water is enabled
pub fn water_sync_system(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    water_config: Option<Res<WaterConfig>>,
    terrain_query: Query<&super::TerrainConfig, With<super::TerrainRoot>>,
    water_query: Query<Entity, With<WaterPlane>>,
) {
    let Some(config) = water_config else { return };

    if !config.enabled {
        // Remove water plane if disabled
        for entity in water_query.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }

    // Only spawn water if terrain exists and no water plane yet
    let Ok(terrain_config) = terrain_query.single() else { return };
    if !water_query.is_empty() {
        return; // Water already exists
    }

    // Calculate terrain footprint
    let terrain_size = terrain_config.chunk_size * (terrain_config.chunks_x * 2) as f32;

    let entity = spawn_water_plane(
        &mut commands,
        &mut meshes,
        &mut materials,
        &config,
        terrain_size,
    );
    tracing::info!("Water plane spawned at Y={:.1}, size={:.0}x{:.0}", config.sea_level, terrain_size, terrain_size);
}

/// System to update water plane position when sea_level changes
pub fn water_update_system(
    water_config: Option<Res<WaterConfig>>,
    mut water_query: Query<&mut Transform, With<WaterPlane>>,
) {
    let Some(config) = water_config else { return };
    if !config.is_changed() { return; }

    for mut transform in water_query.iter_mut() {
        transform.translation.y = config.sea_level;
    }
}

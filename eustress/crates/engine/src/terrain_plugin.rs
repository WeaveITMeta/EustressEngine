//! # Terrain Plugin for Engine Studio
//!
//! Engine-side terrain editing with brush tools, heightmap import/export,
//! and integration with the Explorer/Properties panels.
//!
//! Note: UI is now handled by Slint - see ui/slint/terrain_editor.slint

use bevy::prelude::*;
use bevy::ecs::schedule::common_conditions::resource_equals;
use eustress_common::terrain::{
    TerrainConfig, TerrainData, TerrainMode, TerrainBrush, BrushMode,
    spawn_terrain, TerrainRoot, Chunk,
    TerrainHistory,
    AdvancedBrushState,
};
use eustress_common::classes::Terrain;
use std::path::PathBuf;

// ============================================================================
// Plugin
// ============================================================================

/// Engine terrain plugin - adds editor UI and tools
pub struct EngineTerrainPlugin;

impl Plugin for EngineTerrainPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<TerrainMode>()
            .init_resource::<TerrainBrush>()
            .init_resource::<eustress_common::terrain::TerrainGenerationQueue>()
            .register_type::<TerrainConfig>()
            .register_type::<TerrainData>()
            .register_type::<Chunk>()
            .init_resource::<eustress_common::terrain::LodUpdateState>()
            .init_resource::<eustress_common::terrain::ChunkSpawnThrottle>()
            .add_systems(Update, (
                eustress_common::terrain::process_terrain_generation_queue,
                eustress_common::terrain::update_lod_system,
                eustress_common::terrain::chunk_spawn_system,
                eustress_common::terrain::chunk_cull_system,
            ).chain())
            .init_resource::<TerrainEditorState>()
            .init_resource::<TerrainSelection>()
            .init_resource::<TerrainHistory>()
            .init_resource::<AdvancedBrushState>()
            .init_resource::<BrushPreviewState>()
            .add_systems(Update, (
                sync_terrain_class_to_system,
                handle_editor_shortcuts,
                update_selection_gizmos,
                handle_undo_redo_shortcuts,
                update_brush_preview,
                // terrain_paint_system is registered in common::terrain::TerrainPlugin
            ).run_if(resource_equals(TerrainMode::Editor)));

        // Disk-terrain auto-loader — on Space open, when
        // `Workspace/Terrain/_terrain.toml` exists (worldgen export or
        // heightmap import), hydrate + spawn it once per Space. UNGATED:
        // disk terrain is a default engine capability; migrated Spaces
        // stand down at runtime (the voxel loader below owns those).
        crate::terrain_disk_load::register(app);

        // Wave 9.C — imported-terrain voxel loader (migrated Spaces read
        // Fjall voxels → runtime heightfield, gated on `space_is_migrated`).
        // Feature-gated: only present when the Fjall WorldDb is compiled in.
        // It spawns a `TerrainRoot` the `chunk_spawn_system` above already
        // meshes, so no extra render wiring is needed.
        #[cfg(feature = "world-db")]
        crate::terrain_voxel_load::register(app);
    }
}

// ============================================================================
// Resources
// ============================================================================

/// Editor state for terrain tools
#[derive(Resource)]
#[allow(dead_code)]
pub struct TerrainEditorState {
    pub pending_import: Option<PathBuf>,
    pub last_export_path: Option<PathBuf>,
    pub show_advanced: bool,
    pub show_advanced_brushes: bool,
    pub is_editing: bool,
    pub last_mesh_regen: std::time::Instant,
    pub pending_regen_chunks: Vec<Entity>,
    pub mesh_regen_interval: f32,
    pub last_brush_apply: std::time::Instant,
    pub brush_apply_interval: f32,
}

impl Default for TerrainEditorState {
    fn default() -> Self {
        Self {
            pending_import: None,
            last_export_path: None,
            show_advanced: false,
            show_advanced_brushes: false,
            is_editing: false,
            last_mesh_regen: std::time::Instant::now(),
            pending_regen_chunks: Vec::new(),
            mesh_regen_interval: 0.1,
            last_brush_apply: std::time::Instant::now(),
            brush_apply_interval: 0.016,
        }
    }
}

/// Selection state for Explorer integration
#[derive(Resource, Default)]
pub struct TerrainSelection {
    pub selected_chunk: Option<Entity>,
    pub hovered_chunk: Option<Entity>,
}

/// Brush preview state — tracks where the brush circle should render
#[derive(Resource, Default)]
pub struct BrushPreviewState {
    /// World-space position of the brush center (terrain hit point)
    pub position: Option<Vec3>,
    /// Whether the brush is actively painting (LMB held)
    pub is_painting: bool,
}

// ============================================================================
// Systems
// ============================================================================

/// Sync Terrain class component to terrain system
///
/// Do-not-fight guard: when the live Space has an on-disk terrain
/// (`Workspace/Terrain/_terrain.toml` — worldgen export or heightmap
/// import), an `Added<Terrain>` class instance re-spawns the DISK terrain
/// instead of clobbering it with procedural noise. Migrated Spaces are the
/// voxel loader's domain, so they keep the procedural fallback here.
fn sync_terrain_class_to_system(
    mut commands: Commands,
    query: Query<(Entity, &Terrain), Added<Terrain>>,
    existing_terrain: Query<Entity, With<TerrainRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
) {
    for (_entity, terrain_class) in query.iter() {
        for existing in existing_terrain.iter() {
            commands.entity(existing).despawn();
        }

        // Prefer the Space's on-disk terrain over procedural regeneration.
        let disk = space_root.as_ref().and_then(|sr| {
            if crate::space::space_ops::space_is_migrated(&sr.0) {
                return None;
            }
            let terrain_dir = sr.0.join("Workspace").join("Terrain");
            if !terrain_dir.join("_terrain.toml").exists() {
                return None;
            }
            crate::terrain_disk_load::hydrate_terrain_from_disk(&terrain_dir).ok()
        });

        let (config, data, from_disk) = match disk {
            Some((config, data, _chunk_files)) => (config, data, true),
            None => (terrain_class.to_config(), TerrainData::procedural(), false),
        };

        let terrain_entity = spawn_terrain(
            &mut commands,
            &mut meshes,
            &mut materials,
            config,
            data,
        );
        if from_disk {
            commands
                .entity(terrain_entity)
                .insert(crate::terrain_disk_load::DiskSourcedTerrain);
            info!("🏔️ Engine terrain spawned from Terrain class (hydrated from Workspace/Terrain)");
        } else {
            info!("🏔️ Engine terrain spawned from Terrain class");
        }
    }
}

/// Handle keyboard shortcuts for terrain editing
fn handle_editor_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<TerrainMode>,
    mut brush: ResMut<TerrainBrush>,
    terrain_query: Query<Entity, With<TerrainRoot>>,
) {
    if terrain_query.is_empty() {
        return;
    }

    // Brush shortcuts only apply while the terrain editor is active. Without
    // this guard, 1-5 would mutate brush state (and fight shortcuts like
    // camera-ortho on Digit5) even though the user never opened the editor.
    let editor_active = matches!(*mode, TerrainMode::Editor);
    if editor_active {
        if keys.just_pressed(KeyCode::Digit1) {
            brush.mode = BrushMode::Raise;
            info!("🖌️ Brush: Raise");
        }
        if keys.just_pressed(KeyCode::Digit2) {
            brush.mode = BrushMode::Lower;
            info!("🖌️ Brush: Lower");
        }
        if keys.just_pressed(KeyCode::Digit3) {
            brush.mode = BrushMode::Smooth;
            info!("🖌️ Brush: Smooth");
        }
        if keys.just_pressed(KeyCode::Digit4) {
            brush.mode = BrushMode::Flatten;
            info!("🖌️ Brush: Flatten");
        }
        if keys.just_pressed(KeyCode::Digit5) {
            brush.mode = BrushMode::PaintTexture;
            info!("🖌️ Brush: Paint Texture");
        }
    }
    
    if keys.just_pressed(KeyCode::KeyT) {
        *mode = match *mode {
            TerrainMode::Render => {
                info!("🎨 Terrain Editor: ENABLED");
                TerrainMode::Editor
            }
            TerrainMode::Editor => {
                info!("🎨 Terrain Editor: DISABLED");
                TerrainMode::Render
            }
        };
    }
    
    if keys.just_pressed(KeyCode::BracketLeft) {
        brush.radius = (brush.radius - 2.0).max(1.0);
        info!("🖌️ Brush size: {:.1}", brush.radius);
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        brush.radius = (brush.radius + 2.0).min(50.0);
        info!("🖌️ Brush size: {:.1}", brush.radius);
    }
}

/// Update selection gizmos for terrain chunks
fn update_selection_gizmos(
    selection: Res<TerrainSelection>,
    mut gizmos: Gizmos,
    chunk_query: Query<(&Chunk, &GlobalTransform)>,
    config_query: Query<&TerrainConfig, With<TerrainRoot>>,
) {
    let Ok(config) = config_query.single() else { return };
    
    if let Some(selected) = selection.selected_chunk {
        if let Ok((_chunk, transform)) = chunk_query.get(selected) {
            let pos = transform.translation();
            let size = config.chunk_size;
            gizmos.cube(
                Transform::from_translation(pos + Vec3::Y * 0.5)
                    .with_scale(Vec3::new(size, 1.0, size)),
                bevy::color::Color::srgba(0.0, 1.0, 0.0, 0.5),
            );
        }
    }
}

/// Update brush preview gizmo — renders a circle on the terrain surface at cursor position
fn update_brush_preview(
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    terrain_query: Query<&TerrainConfig, With<TerrainRoot>>,
    brush: Res<TerrainBrush>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut preview: ResMut<BrushPreviewState>,
    mut gizmos: Gizmos,
) {
    let Ok(window) = windows.single() else { return };
    let Ok((camera, camera_transform)) = camera_query.single() else { return };
    let Ok(_config) = terrain_query.single() else { return };

    let Some(cursor_pos) = window.cursor_position() else {
        preview.position = None;
        preview.is_painting = false;
        return;
    };

    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
        preview.position = None;
        return;
    };

    // Raycast to terrain ground plane (Y = 0)
    // TODO: Replace with proper terrain heightmap raycast for accuracy on sculpted terrain
    if ray.direction.y.abs() < 0.001 {
        preview.position = None;
        return;
    }
    let t = -ray.origin.y / ray.direction.y;
    if t < 0.0 {
        preview.position = None;
        return;
    }

    let hit = ray.origin + ray.direction * t;
    preview.position = Some(hit);
    preview.is_painting = buttons.pressed(MouseButton::Left);

    // Draw brush circle on terrain surface
    let radius = brush.radius;
    let color = if preview.is_painting {
        // Active painting: bright mode-specific color
        match brush.mode {
            BrushMode::Raise => bevy::color::Color::srgba(0.2, 1.0, 0.2, 0.9),
            BrushMode::Lower => bevy::color::Color::srgba(1.0, 0.2, 0.2, 0.9),
            BrushMode::Smooth => bevy::color::Color::srgba(0.2, 0.6, 1.0, 0.9),
            BrushMode::Flatten => bevy::color::Color::srgba(1.0, 1.0, 0.2, 0.9),
            BrushMode::PaintTexture => bevy::color::Color::srgba(1.0, 0.5, 0.0, 0.9),
            _ => bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.9),
        }
    } else {
        // Hovering: semi-transparent white
        bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.5)
    };

    // Outer brush circle
    gizmos.circle(
        Isometry3d::new(
            hit + Vec3::Y * 0.05, // Slight Y offset to avoid z-fighting
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        ),
        radius,
        color,
    );

    // Inner falloff circle (shows where full-strength brush starts fading)
    if brush.falloff > 0.01 {
        let inner_radius = radius * (1.0 - brush.falloff);
        if inner_radius > 0.1 {
            let inner_color = bevy::color::Color::srgba(
                color.to_srgba().red,
                color.to_srgba().green,
                color.to_srgba().blue,
                0.25,
            );
            gizmos.circle(
                Isometry3d::new(
                    hit + Vec3::Y * 0.05,
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                ),
                inner_radius,
                inner_color,
            );
        }
    }

    // Crosshair at center
    let cross_size = radius * 0.1;
    let cross_color = bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.3);
    gizmos.line(
        hit + Vec3::new(-cross_size, 0.05, 0.0),
        hit + Vec3::new(cross_size, 0.05, 0.0),
        cross_color,
    );
    gizmos.line(
        hit + Vec3::new(0.0, 0.05, -cross_size),
        hit + Vec3::new(0.0, 0.05, cross_size),
        cross_color,
    );
}

/// Handle undo/redo shortcuts for terrain
fn handle_undo_redo_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut history: ResMut<TerrainHistory>,
    mut terrain_query: Query<&mut TerrainData, With<TerrainRoot>>,
) {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    
    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        if let Ok(mut data) = terrain_query.single_mut() {
            if let Some(snapshot) = history.undo() {
                data.height_cache = snapshot.height_cache.clone();
                info!("↩️ Terrain undo");
            }
        }
    }
    
    if ctrl && keys.just_pressed(KeyCode::KeyY) {
        if let Ok(mut data) = terrain_query.single_mut() {
            if let Some(snapshot) = history.redo() {
                data.height_cache = snapshot.height_cache.clone();
                info!("↪️ Terrain redo");
            }
        }
    }
}

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
            .add_systems(Update, (
                sync_terrain_class_to_system,
                handle_editor_shortcuts,
                update_selection_gizmos,
                handle_undo_redo_shortcuts,
            ).run_if(resource_equals(TerrainMode::Editor)));
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

// ============================================================================
// Systems
// ============================================================================

/// Sync Terrain class component to terrain system
fn sync_terrain_class_to_system(
    mut commands: Commands,
    query: Query<(Entity, &Terrain), Added<Terrain>>,
    existing_terrain: Query<Entity, With<TerrainRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (_entity, terrain_class) in query.iter() {
        for existing in existing_terrain.iter() {
            commands.entity(existing).despawn();
        }
        
        let config = terrain_class.to_config();
        let data = TerrainData::procedural();
        
        let _terrain_entity = spawn_terrain(
            &mut commands,
            &mut meshes,
            &mut materials,
            config,
            data,
        );
        
        info!("🏔️ Engine terrain spawned from Terrain class");
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

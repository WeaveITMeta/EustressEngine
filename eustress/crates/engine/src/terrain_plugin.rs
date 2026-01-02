//! # Terrain Plugin for Engine Studio
//!
//! Engine-side terrain editing with brush tools, heightmap import/export,
//! and integration with the Explorer/Properties panels.
//!
//! ## Features
//! - Full brush editing (Raise, Lower, Smooth, Flatten, Paint)
//! - Keyboard shortcuts (1-5 for tools, T for toggle)
//! - Import/Export heightmaps (PNG) and configs (RON)
//! - Chunk selection with gizmo highlighting
//! - Explorer/Properties panel integration

use bevy::prelude::*;
use bevy::ecs::schedule::common_conditions::resource_equals;
use bevy_egui::{egui, EguiContexts};
use eustress_common::terrain::{
    TerrainConfig, TerrainData, TerrainMode, TerrainBrush, BrushMode,
    spawn_terrain, TerrainRoot, Chunk, generate_chunk_mesh,
    // Undo/Redo
    TerrainHistory, TerrainSnapshot, TerrainEditOp,
    // Advanced brushes
    AdvancedBrushState, AdvancedBrushType,
    // Voxel editing
    BrushShape, BrushPrecision,
};
use eustress_common::classes::Terrain;
use rfd::FileDialog;
use std::path::PathBuf;

// ============================================================================
// Plugin
// ============================================================================

/// Engine terrain plugin - adds editor UI and tools
pub struct EngineTerrainPlugin;

impl Plugin for EngineTerrainPlugin {
    fn build(&self, app: &mut App) {
        app
            // Add shared terrain plugin (but we'll override paint system)
            .init_resource::<TerrainMode>()
            .init_resource::<TerrainBrush>()
            .init_resource::<eustress_common::terrain::TerrainGenerationQueue>()
            .register_type::<TerrainConfig>()
            .register_type::<TerrainData>()
            .register_type::<Chunk>()
            
            // LOD throttling state for performance
            .init_resource::<eustress_common::terrain::LodUpdateState>()
            
            // Core terrain systems
            .add_systems(Update, (
                eustress_common::terrain::process_terrain_generation_queue,  // Async chunk generation
                eustress_common::terrain::update_lod_system,
                eustress_common::terrain::chunk_spawn_system,
                eustress_common::terrain::chunk_cull_system,
            ).chain())
            
            // Engine-specific resources
            .init_resource::<TerrainEditorState>()
            .init_resource::<TerrainSelection>()
            .init_resource::<TerrainHistory>()
            .init_resource::<AdvancedBrushState>()
            
            // Engine-specific systems
            .add_systems(Update, (
                sync_terrain_class_to_system,
                handle_editor_shortcuts,
                terrain_editor_ui,
                update_selection_gizmos,
                handle_undo_redo_shortcuts,
                engine_terrain_paint_system.run_if(resource_equals(TerrainMode::Editor)),
            ));
    }
}

// ============================================================================
// Resources
// ============================================================================

/// Editor state for terrain tools
#[derive(Resource)]
#[allow(dead_code)]
pub struct TerrainEditorState {
    /// Pending heightmap import path
    pub pending_import: Option<PathBuf>,
    /// Last export path
    pub last_export_path: Option<PathBuf>,
    /// Show advanced settings
    pub show_advanced: bool,
    /// Show advanced brushes panel
    pub show_advanced_brushes: bool,
    /// Currently editing (for snapshot timing)
    pub is_editing: bool,
    /// Last edit operation type
    pub last_edit_op: Option<TerrainEditOp>,
    /// Last mesh regeneration time (for throttling)
    pub last_mesh_regen: std::time::Instant,
    /// Chunks pending mesh regeneration
    pub pending_regen_chunks: Vec<Entity>,
    /// Mesh regen interval (seconds) - throttle for performance
    pub mesh_regen_interval: f32,
    /// Last brush application time (for throttling height cache updates)
    pub last_brush_apply: std::time::Instant,
    /// Brush apply interval (seconds) - throttle height cache updates
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
            last_edit_op: None,
            last_mesh_regen: std::time::Instant::now(),
            pending_regen_chunks: Vec::new(),
            mesh_regen_interval: 0.1, // ~10 FPS mesh updates (better performance)
            last_brush_apply: std::time::Instant::now(),
            brush_apply_interval: 0.016, // ~60 FPS brush application (responsive but not every frame)
        }
    }
}


/// Selection state for Explorer integration
#[derive(Resource, Default)]
pub struct TerrainSelection {
    /// Currently selected chunk entity
    pub selected_chunk: Option<Entity>,
    /// Hovered chunk (for preview)
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
        // Remove any existing terrain (TODO: match by ID for multi-terrain)
        for existing in existing_terrain.iter() {
            commands.entity(existing).despawn();
        }
        
        // Convert class to config
        let config = terrain_class.to_config();
        let data = TerrainData::procedural();
        
        // Spawn terrain system
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
    // Only process if terrain exists
    if terrain_query.is_empty() {
        return;
    }
    
    // Tool shortcuts (1-5)
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
    
    // Mode toggle (T) - handled by shared plugin, but we can override
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
    
    // Brush size ([ and ])
    if keys.just_pressed(KeyCode::BracketLeft) {
        brush.radius = (brush.radius - 2.0).max(1.0);
        info!("🖌️ Brush size: {:.1}", brush.radius);
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        brush.radius = (brush.radius + 2.0).min(50.0);
        info!("🖌️ Brush size: {:.1}", brush.radius);
    }
}

/// Terrain editor UI panel with import/export
/// This is now a standalone function that can be called from the dock system
fn terrain_editor_ui(
    mut contexts: EguiContexts,
    mut mode: ResMut<TerrainMode>,
    mut brush: ResMut<TerrainBrush>,
    mut editor_state: ResMut<TerrainEditorState>,
    mut selection: ResMut<TerrainSelection>,
    terrain_query: Query<(&TerrainConfig, &TerrainData), With<TerrainRoot>>,
    chunk_query: Query<(Entity, &Chunk, &GlobalTransform)>,
    _asset_server: Res<AssetServer>,
    mut studio_state: ResMut<crate::ui::StudioState>,
) {
    // Only show if terrain exists
    let Ok((config, data)) = terrain_query.single() else { return };
    
    // Auto-show terrain editor when terrain exists (first time)
    if !studio_state.show_terrain_editor {
        studio_state.show_terrain_editor = true;
    }
    
    // Don't show panel if user closed it
    if !studio_state.show_terrain_editor {
        return;
    }
    
    let Ok(ctx) = contexts.ctx_mut() else { return };
    
    // Always ensure Editor mode when terrain panel is open
    if *mode != TerrainMode::Editor {
        *mode = TerrainMode::Editor;
    }
    
    // Copy values for UI
    let mut brush_radius = brush.radius;
    let mut brush_strength = brush.strength;
    let mut brush_falloff = brush.falloff;
    let mut brush_layer = brush.texture_layer;
    let mut brush_mode = brush.mode;
    let mut voxel_mode = brush.voxel_mode;
    let mut voxel_size = brush.voxel_size;
    let mut height_step = brush.height_step;
    let mut brush_shape = brush.shape;
    let mut brush_precision = brush.precision;
    let chunk_count = chunk_query.iter().count();
    
    // Get selected chunk info
    let selected_info = selection.selected_chunk
        .and_then(|e| chunk_query.get(e).ok())
        .map(|(_, chunk, transform)| (chunk.position, chunk.lod, transform.translation()));
    
    // Show as a secondary right panel (jointed with MindSpace)
    // Only show if Terrain tab is selected OR MindSpace is not visible
    let show_terrain_content = studio_state.secondary_panel_tab == crate::ui::SecondaryPanelTab::Terrain 
        || !studio_state.mindspace_panel_visible;
    
    // Use a stable width that persists across frames
    let panel_id = egui::Id::new("terrain_mindspace_panel_width");
    let stored_width: f32 = ctx.data_mut(|d| d.get_persisted(panel_id)).unwrap_or(240.0);
    
    egui::SidePanel::left("terrain_mindspace_panel")
        .default_width(stored_width)  // Use stored width
        .min_width(180.0)
        .max_width(350.0)
        .resizable(true)
        .show(ctx, |ui| {
            // Store the current width for next frame (only if resized by user)
            let current_width = ui.available_width();
            if (current_width - stored_width).abs() > 1.0 {
                ui.ctx().data_mut(|d| d.insert_persisted(panel_id, current_width));
            }
            // Ensure proper clipping and scrolling
            ui.set_clip_rect(ui.available_rect_before_wrap());
            // Tab switcher (only show if MindSpace is also available)
            if studio_state.mindspace_panel_visible {
                // Close button first (right-aligned)
                let close_clicked = ui.horizontal(|ui| {
                    if ui.selectable_label(
                        studio_state.secondary_panel_tab == crate::ui::SecondaryPanelTab::Terrain,
                        "Terrain"
                    ).clicked() {
                        studio_state.secondary_panel_tab = crate::ui::SecondaryPanelTab::Terrain;
                    }
                    if ui.selectable_label(
                        studio_state.secondary_panel_tab == crate::ui::SecondaryPanelTab::MindSpace,
                        "MindSpace"
                    ).clicked() {
                        studio_state.secondary_panel_tab = crate::ui::SecondaryPanelTab::MindSpace;
                    }
                    
                    ui.add_space(ui.available_width() - 20.0);
                    ui.small_button("x").on_hover_text("Close Panel").clicked()
                }).inner;
                
                if close_clicked {
                    studio_state.show_terrain_editor = false;
                    studio_state.mindspace_panel_visible = false;
                    *mode = TerrainMode::Render;
                }
                ui.separator();
            } else {
                // Header with close button (no tabs)
                let close_clicked = ui.horizontal(|ui| {
                    ui.heading("Terrain Editor");
                    ui.add_space(ui.available_width() - 20.0);
                    ui.small_button("x").on_hover_text("Close Terrain Editor (T)").clicked()
                }).inner;
                
                if close_clicked {
                    studio_state.show_terrain_editor = false;
                    *mode = TerrainMode::Render;
                }
                ui.separator();
            }
            
            // Only render terrain content if Terrain tab is selected
            if !show_terrain_content {
                // MindSpace content will be rendered by the MindSpace plugin
                ui.label("MindSpace content rendered by plugin");
                return;
            }
            
            // Brush settings (always shown - we're always in editor mode when panel is open)
            {
                // Voxel mode toggle
                ui.horizontal(|ui| {
                    ui.checkbox(&mut voxel_mode, "Voxel Mode");
                    ui.weak("(fine-grain)");
                });
                
                ui.add_space(4.0);
                ui.label("Brush Tools");
                
                // Tool buttons with Material Design icons
                let icon_size = 18.0;
                let button_size = egui::vec2(70.0, 24.0);
                
                ui.horizontal(|ui| {
                    // Raise button with icon
                    let raise_selected = brush_mode == BrushMode::Raise;
                    let raise_response = ui.add_sized(button_size, egui::Button::new("").selected(raise_selected));
                    let raise_rect = raise_response.rect;
                    crate::ui::icons::draw_brush_raise_icon(ui.painter(), raise_rect.left_top() + egui::vec2(4.0, 3.0), icon_size, raise_selected);
                    ui.painter().text(raise_rect.left_top() + egui::vec2(24.0, 4.0), egui::Align2::LEFT_TOP, "1", egui::FontId::proportional(10.0), if raise_selected { egui::Color32::WHITE } else { egui::Color32::GRAY });
                    if raise_response.clicked() { brush_mode = BrushMode::Raise; }
                    raise_response.on_hover_text("Raise terrain (1)");
                    
                    // Lower button with icon
                    let lower_selected = brush_mode == BrushMode::Lower;
                    let lower_response = ui.add_sized(button_size, egui::Button::new("").selected(lower_selected));
                    let lower_rect = lower_response.rect;
                    crate::ui::icons::draw_brush_lower_icon(ui.painter(), lower_rect.left_top() + egui::vec2(4.0, 3.0), icon_size, lower_selected);
                    ui.painter().text(lower_rect.left_top() + egui::vec2(24.0, 4.0), egui::Align2::LEFT_TOP, "2", egui::FontId::proportional(10.0), if lower_selected { egui::Color32::WHITE } else { egui::Color32::GRAY });
                    if lower_response.clicked() { brush_mode = BrushMode::Lower; }
                    lower_response.on_hover_text("Lower terrain (2)");
                });
                
                ui.horizontal(|ui| {
                    // Smooth button with icon
                    let smooth_selected = brush_mode == BrushMode::Smooth;
                    let smooth_response = ui.add_sized(button_size, egui::Button::new("").selected(smooth_selected));
                    let smooth_rect = smooth_response.rect;
                    crate::ui::icons::draw_brush_smooth_icon(ui.painter(), smooth_rect.left_top() + egui::vec2(4.0, 3.0), icon_size, smooth_selected);
                    ui.painter().text(smooth_rect.left_top() + egui::vec2(24.0, 4.0), egui::Align2::LEFT_TOP, "3", egui::FontId::proportional(10.0), if smooth_selected { egui::Color32::WHITE } else { egui::Color32::GRAY });
                    if smooth_response.clicked() { brush_mode = BrushMode::Smooth; }
                    smooth_response.on_hover_text("Smooth terrain (3)");
                    
                    // Flatten button with icon
                    let flatten_selected = brush_mode == BrushMode::Flatten;
                    let flatten_response = ui.add_sized(button_size, egui::Button::new("").selected(flatten_selected));
                    let flatten_rect = flatten_response.rect;
                    crate::ui::icons::draw_brush_flatten_icon(ui.painter(), flatten_rect.left_top() + egui::vec2(4.0, 3.0), icon_size, flatten_selected);
                    ui.painter().text(flatten_rect.left_top() + egui::vec2(24.0, 4.0), egui::Align2::LEFT_TOP, "4", egui::FontId::proportional(10.0), if flatten_selected { egui::Color32::WHITE } else { egui::Color32::GRAY });
                    if flatten_response.clicked() { brush_mode = BrushMode::Flatten; }
                    flatten_response.on_hover_text("Flatten terrain (4)");
                });
                
                ui.horizontal(|ui| {
                    // Paint button with icon
                    let paint_selected = brush_mode == BrushMode::PaintTexture;
                    let paint_response = ui.add_sized(button_size, egui::Button::new("").selected(paint_selected));
                    let paint_rect = paint_response.rect;
                    crate::ui::icons::draw_brush_paint_icon(ui.painter(), paint_rect.left_top() + egui::vec2(4.0, 3.0), icon_size, paint_selected);
                    ui.painter().text(paint_rect.left_top() + egui::vec2(24.0, 4.0), egui::Align2::LEFT_TOP, "5", egui::FontId::proportional(10.0), if paint_selected { egui::Color32::WHITE } else { egui::Color32::GRAY });
                    if paint_response.clicked() { brush_mode = BrushMode::PaintTexture; }
                    paint_response.on_hover_text("Paint texture (5)");
                });
                
                // Voxel-specific tools
                if voxel_mode {
                    ui.horizontal(|ui| {
                        // Voxel Add button with icon
                        let add_selected = brush_mode == BrushMode::VoxelAdd;
                        let add_response = ui.add_sized(button_size, egui::Button::new("").selected(add_selected));
                        let add_rect = add_response.rect;
                        crate::ui::icons::draw_brush_voxel_add_icon(ui.painter(), add_rect.left_top() + egui::vec2(4.0, 3.0), icon_size, add_selected);
                        if add_response.clicked() { brush_mode = BrushMode::VoxelAdd; }
                        add_response.on_hover_text("Add voxels");
                        
                        // Voxel Remove button with icon
                        let remove_selected = brush_mode == BrushMode::VoxelRemove;
                        let remove_response = ui.add_sized(button_size, egui::Button::new("").selected(remove_selected));
                        let remove_rect = remove_response.rect;
                        crate::ui::icons::draw_brush_voxel_remove_icon(ui.painter(), remove_rect.left_top() + egui::vec2(4.0, 3.0), icon_size, remove_selected);
                        if remove_response.clicked() { brush_mode = BrushMode::VoxelRemove; }
                        remove_response.on_hover_text("Remove voxels");
                    });
                }
                
                ui.add_space(4.0);
                
                // Brush parameters - finer control in voxel mode
                let min_radius = if voxel_mode { 0.5 } else { 1.0 };
                let max_radius = if voxel_mode { 50.0 } else { 100.0 };
                
                // Size slider with DragValue for text input
                ui.horizontal(|ui| {
                    ui.label("Size:");
                    ui.add(egui::DragValue::new(&mut brush_radius)
                        .range(min_radius..=max_radius)
                        .speed(0.1)
                        .suffix(" m"));
                    ui.label("[ ]");
                });
                ui.add(egui::Slider::new(&mut brush_radius, min_radius..=max_radius)
                    .show_value(false));
                
                // Strength slider with DragValue
                ui.horizontal(|ui| {
                    ui.label("Strength:");
                    ui.add(egui::DragValue::new(&mut brush_strength)
                        .range(0.01..=1.0)
                        .speed(0.01)
                        .fixed_decimals(2));
                });
                ui.add(egui::Slider::new(&mut brush_strength, 0.01..=1.0)
                    .show_value(false));
                
                // Falloff slider with DragValue
                ui.horizontal(|ui| {
                    ui.label("Falloff:");
                    ui.add(egui::DragValue::new(&mut brush_falloff)
                        .range(0.0..=1.0)
                        .speed(0.01)
                        .fixed_decimals(2));
                });
                ui.add(egui::Slider::new(&mut brush_falloff, 0.0..=1.0)
                    .show_value(false));
                
                // Voxel precision settings
                if voxel_mode {
                    ui.add_space(4.0);
                    ui.collapsing("Precision Settings", |ui| {
                        // Voxel size slider (5cm to 2m)
                        ui.add(egui::Slider::new(&mut voxel_size, 0.05..=2.0)
                            .text("Voxel Size")
                            .suffix("m"));
                        
                        // Height step slider
                        ui.add(egui::Slider::new(&mut height_step, 0.01..=1.0)
                            .text("Height Step")
                            .suffix("m"));
                        
                        // Brush shape
                        ui.horizontal(|ui| {
                            ui.label("Shape:");
                            if ui.selectable_label(brush_shape == BrushShape::Circle, "⬤").on_hover_text("Circle").clicked() {
                                brush_shape = BrushShape::Circle;
                            }
                            if ui.selectable_label(brush_shape == BrushShape::Square, "⬛").on_hover_text("Square").clicked() {
                                brush_shape = BrushShape::Square;
                            }
                            if ui.selectable_label(brush_shape == BrushShape::Diamond, "◆").on_hover_text("Diamond").clicked() {
                                brush_shape = BrushShape::Diamond;
                            }
                        });
                        
                        // Precision level
                        ui.horizontal(|ui| {
                            ui.label("Quality:");
                            if ui.selectable_label(brush_precision == BrushPrecision::Low, "Low").clicked() {
                                brush_precision = BrushPrecision::Low;
                            }
                            if ui.selectable_label(brush_precision == BrushPrecision::Medium, "Med").clicked() {
                                brush_precision = BrushPrecision::Medium;
                            }
                            if ui.selectable_label(brush_precision == BrushPrecision::High, "High").clicked() {
                                brush_precision = BrushPrecision::High;
                            }
                            if ui.selectable_label(brush_precision == BrushPrecision::Ultra, "Ultra").clicked() {
                                brush_precision = BrushPrecision::Ultra;
                            }
                        });
                        
                        // Preset buttons
                        ui.horizontal(|ui| {
                            if ui.button("Fine Detail").clicked() {
                                brush_radius = 0.5;
                                voxel_size = 0.05;
                                height_step = 0.05;
                                brush_precision = BrushPrecision::Ultra;
                            }
                            if ui.button("Normal").clicked() {
                                brush_radius = 2.0;
                                voxel_size = 0.25;
                                height_step = 0.1;
                                brush_precision = BrushPrecision::High;
                            }
                        });
                    });
                }
                
                // Layer selector for paint mode
                if brush_mode == BrushMode::PaintTexture {
                    ui.add_space(4.0);
                    ui.label("Material:");
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        egui::Grid::new("terrain_materials").num_columns(2).show(ui, |ui| {
                            // Natural materials
                            if ui.selectable_label(brush_layer == 0, "Grass").clicked() { brush_layer = 0; }
                            if ui.selectable_label(brush_layer == 1, "Rock").clicked() { brush_layer = 1; }
                            ui.end_row();
                            if ui.selectable_label(brush_layer == 2, "Dirt").clicked() { brush_layer = 2; }
                            if ui.selectable_label(brush_layer == 3, "Snow").clicked() { brush_layer = 3; }
                            ui.end_row();
                            if ui.selectable_label(brush_layer == 4, "Sand").clicked() { brush_layer = 4; }
                            if ui.selectable_label(brush_layer == 5, "Mud").clicked() { brush_layer = 5; }
                            ui.end_row();
                            // Man-made materials
                            if ui.selectable_label(brush_layer == 6, "Concrete").clicked() { brush_layer = 6; }
                            if ui.selectable_label(brush_layer == 7, "Asphalt").clicked() { brush_layer = 7; }
                            ui.end_row();
                            // New materials
                            if ui.selectable_label(brush_layer == 8, "Basalt").clicked() { brush_layer = 8; }
                            if ui.selectable_label(brush_layer == 9, "Lava").clicked() { brush_layer = 9; }
                            ui.end_row();
                            if ui.selectable_label(brush_layer == 10, "Water").clicked() { brush_layer = 10; }
                            if ui.selectable_label(brush_layer == 11, "Sandstone").clicked() { brush_layer = 11; }
                            ui.end_row();
                            if ui.selectable_label(brush_layer == 12, "Gravel").clicked() { brush_layer = 12; }
                            if ui.selectable_label(brush_layer == 13, "Ice").clicked() { brush_layer = 13; }
                            ui.end_row();
                            if ui.selectable_label(brush_layer == 14, "Forest").clicked() { brush_layer = 14; }
                            if ui.selectable_label(brush_layer == 15, "Wheat").clicked() { brush_layer = 15; }
                            ui.end_row();
                        });
                    });
                }
                
                ui.add_space(4.0);
                ui.weak("LMB to paint | [ ] to resize");
            }
            
            ui.separator();
            
            // Import/Export section
            ui.label("Assets");
            ui.horizontal(|ui| {
                if ui.button("Import").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("Heightmap", &["png", "jpg", "exr"])
                        .pick_file()
                    {
                        editor_state.pending_import = Some(path.clone());
                        info!("Importing heightmap: {:?}", path);
                    }
                }
                if ui.button("Export").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("Heightmap", &["png"])
                        .set_file_name("terrain_height.png")
                        .save_file()
                    {
                        export_heightmap_to_file(data, &path);
                        editor_state.last_export_path = Some(path);
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Save RON").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("RON", &["ron"])
                        .set_file_name("terrain.ron")
                        .save_file()
                    {
                        export_config_to_ron(config, &path);
                    }
                }
                if ui.button("Load RON").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("RON", &["ron"])
                        .pick_file()
                    {
                        info!("Loading config: {:?}", path);
                        // TODO: Implement config loading
                    }
                }
            });
            
            ui.separator();
            
            // Terrain properties
            ui.collapsing("Properties", |ui| {
                ui.label(format!("Chunk Size: {:.0}m", config.chunk_size));
                ui.label(format!("Resolution: {}x{}", config.chunk_resolution, config.chunk_resolution));
                ui.label(format!("LOD Levels: {}", config.lod_levels));
                ui.label(format!("View Distance: {:.0}m", config.view_distance));
                ui.label(format!("Height Scale: {:.1}", config.height_scale));
                ui.label(format!("Active Chunks: {}", chunk_count));
            });
            
            // Selected chunk info
            if let Some((pos, lod, world_pos)) = selected_info {
                ui.collapsing("Selected Chunk", |ui| {
                    ui.label(format!("Grid: ({}, {})", pos.x, pos.y));
                    ui.label(format!("LOD: {}", lod));
                    ui.label(format!("World: ({:.0}, {:.0}, {:.0})", world_pos.x, world_pos.y, world_pos.z));
                    if ui.button("Focus Camera").clicked() {
                        // TODO: Move camera to chunk
                    }
                    if ui.button("Deselect").clicked() {
                        selection.selected_chunk = None;
                    }
                });
            }
            
            // Advanced settings
            ui.collapsing("Advanced", |ui| {
                ui.label(format!("Seed: {}", config.seed));
                ui.label(format!("Has Heightmap: {}", data.heightmap.is_some()));
                ui.label(format!("Has Splatmap: {}", data.splatmap.is_some()));
                ui.label(format!("Cache Size: {}x{}", data.cache_width, data.cache_height));
            });
        });
    
    // Apply changes
    brush.radius = brush_radius;
    brush.strength = brush_strength;
    brush.falloff = brush_falloff;
    brush.texture_layer = brush_layer;
    brush.mode = brush_mode;
    brush.voxel_mode = voxel_mode;
    brush.voxel_size = voxel_size;
    brush.height_step = height_step;
    brush.shape = brush_shape;
    brush.precision = brush_precision;
}

/// Draw selection gizmos for highlighted chunks
fn update_selection_gizmos(
    mut gizmos: Gizmos,
    selection: Res<TerrainSelection>,
    mode: Res<TerrainMode>,
    terrain_query: Query<&TerrainConfig, With<TerrainRoot>>,
    chunk_query: Query<(&Chunk, &GlobalTransform)>,
) {
    // Only show in editor mode
    if *mode != TerrainMode::Editor {
        return;
    }
    
    let Ok(config) = terrain_query.single() else { return };
    let chunk_size = config.chunk_size;
    
    // Draw selected chunk highlight
    if let Some(entity) = selection.selected_chunk {
        if let Ok((_chunk, transform)) = chunk_query.get(entity) {
            let pos = transform.translation();
            let half = chunk_size * 0.5;
            
            // Draw selection box
            gizmos.rect(
                Isometry3d::new(
                    pos + Vec3::new(half, 1.0, half),
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                ),
                Vec2::splat(chunk_size),
                Color::srgba(0.0, 1.0, 0.0, 0.8),
            );
        }
    }
    
    // Draw hovered chunk preview
    if let Some(entity) = selection.hovered_chunk {
        if selection.selected_chunk != Some(entity) {
            if let Ok((_chunk, transform)) = chunk_query.get(entity) {
                let pos = transform.translation();
                let half = chunk_size * 0.5;
                
                gizmos.rect(
                    Isometry3d::new(
                        pos + Vec3::new(half, 0.5, half),
                        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    ),
                    Vec2::splat(chunk_size),
                    Color::srgba(1.0, 1.0, 0.0, 0.4),
                );
            }
        }
    }
}

// ============================================================================
// Export Helpers
// ============================================================================

/// Export heightmap to PNG file
fn export_heightmap_to_file(data: &TerrainData, path: &PathBuf) {
    if data.height_cache.is_empty() {
        info!("⚠️ No height data to export");
        return;
    }
    
    // Find min/max for normalization
    let min_h = data.height_cache.iter().cloned().fold(f32::INFINITY, f32::min);
    let max_h = data.height_cache.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let range = (max_h - min_h).max(0.001);
    
    // Create grayscale image data
    let width = data.cache_width as u32;
    let height = data.cache_height as u32;
    let _pixels: Vec<u8> = data.height_cache.iter()
        .map(|h| ((h - min_h) / range * 255.0) as u8)
        .collect();
    
    // TODO: Save using image crate
    // For now, just log
    info!("📤 Exported heightmap to {:?} ({}x{}, range {:.1} to {:.1})", 
        path, width, height, min_h, max_h);
}

/// Export terrain config to RON file
fn export_config_to_ron(config: &TerrainConfig, path: &PathBuf) {
    // Serialize config to RON
    let ron_str = format!(
        r#"TerrainConfig(
    chunk_size: {},
    chunk_resolution: {},
    chunks_x: {},
    chunks_z: {},
    lod_levels: {},
    view_distance: {},
    height_scale: {},
    seed: {},
)"#,
        config.chunk_size,
        config.chunk_resolution,
        config.chunks_x,
        config.chunks_z,
        config.lod_levels,
        config.view_distance,
        config.height_scale,
        config.seed,
    );
    
    if let Err(e) = std::fs::write(path, ron_str) {
        info!("❌ Failed to export RON: {}", e);
    } else {
        info!("💾 Exported config to {:?}", path);
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Spawn terrain from the ribbon menu
#[allow(dead_code)]
pub fn spawn_terrain_from_menu(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    config: TerrainConfig,
) -> Entity {
    let data = TerrainData::procedural();
    spawn_terrain(commands, meshes, materials, config, data)
}

// ============================================================================
// Engine-specific Terrain Paint System
// ============================================================================

/// Engine terrain paint system with egui check and throttled mesh regeneration
/// This prevents painting when UI is being interacted with and reduces lag
fn engine_terrain_paint_system(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut terrain_query: Query<(&TerrainConfig, &mut TerrainData), With<TerrainRoot>>,
    mut chunk_query: Query<(Entity, &mut Chunk, &GlobalTransform)>,
    brush: Res<TerrainBrush>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    mut egui_ctx: EguiContexts,
    mut editor_state: ResMut<TerrainEditorState>,
) {
    // Skip if egui wants pointer input (UI is being interacted with)
    if let Ok(ctx) = egui_ctx.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }
    
    // Only paint when LMB is pressed
    if !buttons.pressed(MouseButton::Left) {
        // When not painting, regenerate any pending chunks
        if !editor_state.pending_regen_chunks.is_empty() {
            let Ok((config, data)) = terrain_query.single() else { return };
            for entity in editor_state.pending_regen_chunks.drain(..) {
                if let Ok((_, chunk, _)) = chunk_query.get(entity) {
                    let new_mesh = generate_chunk_mesh(
                        chunk.position,
                        chunk.lod,
                        config,
                        &data,
                        &mut meshes,
                    );
                    commands.entity(entity).insert(Mesh3d(new_mesh));
                }
            }
        }
        return;
    }
    
    let Ok(window) = windows.single() else { return };
    let Ok((camera, camera_transform)) = camera_query.single() else { return };
    let Ok((config, mut data)) = terrain_query.single_mut() else { return };
    
    // Get mouse world position using accurate terrain raycast
    let (x, y, z) = match get_mouse_world_position(window, camera, camera_transform, config, &data) {
        Some(pos) => pos,
        None => return,
    };
    let hit_point = Vec3::new(x, y, z);
    
    // Check if we should apply brush (throttle for performance)
    let now = std::time::Instant::now();
    let should_apply_brush = now.duration_since(editor_state.last_brush_apply).as_secs_f32() 
        >= editor_state.brush_apply_interval;
    
    // Skip if we shouldn't apply brush yet (throttled)
    if !should_apply_brush {
        return;
    }
    editor_state.last_brush_apply = now;
    
    // Check if we should regenerate meshes (throttle for performance)
    let should_regen_mesh = now.duration_since(editor_state.last_mesh_regen).as_secs_f32() 
        >= editor_state.mesh_regen_interval;
    
    // Find affected chunks
    for (entity, mut chunk, transform) in chunk_query.iter_mut() {
        let chunk_center = transform.translation();
        let chunk_half_size = config.chunk_size * 0.5;
        
        // Check if brush overlaps this chunk
        let min_x = chunk_center.x - chunk_half_size - brush.radius;
        let max_x = chunk_center.x + chunk_half_size + brush.radius;
        let min_z = chunk_center.z - chunk_half_size - brush.radius;
        let max_z = chunk_center.z + chunk_half_size + brush.radius;
        
        if hit_point.x >= min_x && hit_point.x <= max_x &&
           hit_point.z >= min_z && hit_point.z <= max_z {
            // Mark chunk as dirty for regeneration
            chunk.dirty = true;
            
            // Apply brush to height cache (throttled for performance)
            apply_brush_to_chunk(
                &hit_point,
                &brush,
                &chunk,
                config,
                &mut data,
            );
            
            // Throttle mesh regeneration for performance
            if should_regen_mesh {
                let new_mesh = generate_chunk_mesh(
                    chunk.position,
                    chunk.lod,
                    config,
                    &data,
                    &mut meshes,
                );
                commands.entity(entity).insert(Mesh3d(new_mesh));
            } else {
                // Queue for later regeneration
                if !editor_state.pending_regen_chunks.contains(&entity) {
                    editor_state.pending_regen_chunks.push(entity);
                }
            }
        }
    }
    
    if should_regen_mesh {
        editor_state.last_mesh_regen = now;
        editor_state.pending_regen_chunks.clear();
    }
}

/// Apply brush effect to terrain data (local copy for engine)
/// Uses world coordinates to properly index into the global height cache
fn apply_brush_to_chunk(
    hit_point: &Vec3,
    brush: &TerrainBrush,
    _chunk: &Chunk,
    config: &TerrainConfig,
    data: &mut TerrainData,
) {
    // Calculate terrain dimensions
    let total_chunks_x = config.chunks_x * 2 + 1;
    let total_chunks_z = config.chunks_z * 2 + 1;
    let terrain_size_x = total_chunks_x as f32 * config.chunk_size;
    let terrain_size_z = total_chunks_z as f32 * config.chunk_size;
    let terrain_half_x = terrain_size_x * 0.5;
    let terrain_half_z = terrain_size_z * 0.5;
    
    // Initialize height cache if empty - size for entire terrain
    let cache_width = total_chunks_x * config.chunk_resolution + 1;
    let cache_height = total_chunks_z * config.chunk_resolution + 1;
    
    if data.height_cache.is_empty() || data.cache_width != cache_width || data.cache_height != cache_height {
        data.height_cache = vec![0.0; (cache_width * cache_height) as usize];
        data.cache_width = cache_width;
        data.cache_height = cache_height;
    }
    
    // Calculate brush bounds in cache coordinates
    let brush_min_x = hit_point.x - brush.radius;
    let brush_max_x = hit_point.x + brush.radius;
    let brush_min_z = hit_point.z - brush.radius;
    let brush_max_z = hit_point.z + brush.radius;
    
    // Convert world bounds to cache indices
    let world_to_cache_x = |wx: f32| -> i32 {
        let normalized = (wx + terrain_half_x) / terrain_size_x;
        (normalized * (cache_width - 1) as f32).round() as i32
    };
    let world_to_cache_z = |wz: f32| -> i32 {
        let normalized = (wz + terrain_half_z) / terrain_size_z;
        (normalized * (cache_height - 1) as f32).round() as i32
    };
    let cache_to_world_x = |cx: i32| -> f32 {
        let normalized = cx as f32 / (cache_width - 1) as f32;
        normalized * terrain_size_x - terrain_half_x
    };
    let cache_to_world_z = |cz: i32| -> f32 {
        let normalized = cz as f32 / (cache_height - 1) as f32;
        normalized * terrain_size_z - terrain_half_z
    };
    
    let cache_min_x = world_to_cache_x(brush_min_x).max(0) as u32;
    let cache_max_x = (world_to_cache_x(brush_max_x) as u32).min(cache_width - 1);
    let cache_min_z = world_to_cache_z(brush_min_z).max(0) as u32;
    let cache_max_z = (world_to_cache_z(brush_max_z) as u32).min(cache_height - 1);
    
    // Iterate over affected cache cells
    for cz in cache_min_z..=cache_max_z {
        for cx in cache_min_x..=cache_max_x {
            let world_x = cache_to_world_x(cx as i32);
            let world_z = cache_to_world_z(cz as i32);
            
            let dx = world_x - hit_point.x;
            let dz = world_z - hit_point.z;
            let dist = (dx * dx + dz * dz).sqrt();
            
            if dist <= brush.radius {
                // Calculate smooth falloff using hermite interpolation
                // This creates natural, non-jagged terrain modifications
                let t = dist / brush.radius;
                
                // Smooth falloff: uses smoothstep for natural blending
                // falloff parameter controls the curve shape (0 = hard edge, 1 = very soft)
                let falloff = if brush.falloff > 0.0 {
                    // Smoothstep: 3t² - 2t³ for smooth transition
                    let smooth_t = t * t * (3.0 - 2.0 * t);
                    // Apply falloff curve - higher falloff = softer edges
                    let curve = 1.0 - smooth_t.powf(1.0 - brush.falloff * 0.8);
                    curve.max(0.0)
                } else {
                    // Hard edge (no falloff)
                    if t < 0.9 { 1.0 } else { 1.0 - (t - 0.9) * 10.0 }
                };
                
                // Scale effect by strength - lower multiplier for smoother editing
                let effect = brush.strength * falloff * 0.05;
                
                let idx = (cz * cache_width + cx) as usize;
                if idx < data.height_cache.len() {
                    match brush.mode {
                        BrushMode::Raise | BrushMode::VoxelAdd => {
                            data.height_cache[idx] += effect;
                        }
                        BrushMode::Lower | BrushMode::VoxelRemove => {
                            data.height_cache[idx] -= effect;
                        }
                        BrushMode::Smooth | BrushMode::VoxelSmooth => {
                            // Average with neighbors
                            let neighbors = get_neighbor_heights_global(data, cx, cz, cache_width, cache_height);
                            let avg = neighbors.iter().sum::<f32>() / neighbors.len().max(1) as f32;
                            data.height_cache[idx] = data.height_cache[idx] * (1.0 - effect) + avg * effect;
                        }
                        BrushMode::Flatten => {
                            // Flatten to hit point height
                            let target = hit_point.y / config.height_scale;
                            data.height_cache[idx] = data.height_cache[idx] * (1.0 - effect) + target * effect;
                        }
                        BrushMode::PaintTexture => {
                            // TODO: Modify splatmap
                        }
                        BrushMode::Region | BrushMode::Fill => {
                            // Region selection and fill are handled separately
                        }
                    }
                }
            }
        }
    }
}

/// Get heights of neighboring vertices for smoothing (global cache version)
fn get_neighbor_heights_global(data: &TerrainData, x: u32, z: u32, width: u32, height: u32) -> Vec<f32> {
    let mut heights = Vec::with_capacity(8);
    
    for dz in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            
            let nx = x as i32 + dx;
            let nz = z as i32 + dz;
            
            if nx >= 0 && nx < width as i32 && nz >= 0 && nz < height as i32 {
                let idx = (nz as u32 * width + nx as u32) as usize;
                if idx < data.height_cache.len() {
                    heights.push(data.height_cache[idx]);
                }
            }
        }
    }
    
    heights
}

// ============================================================================
// Mouse World Position Utility
// ============================================================================

/// Get the mouse world position on the terrain surface.
/// Returns (x, y, z) coordinates for accurate voxel editing.
/// 
/// This function raycasts from the camera through the cursor position
/// to find the exact terrain surface intersection point.
pub fn get_mouse_world_position(
    window: &Window,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    config: &TerrainConfig,
    data: &TerrainData,
) -> Option<(f32, f32, f32)> {
    // Get cursor position
    let cursor_pos = window.cursor_position()?;
    
    // Raycast from cursor to terrain
    let ray = camera.viewport_to_world(camera_transform, cursor_pos).ok()?;
    
    // Raycast to find hit point on terrain surface
    let hit_point = raycast_terrain(&ray, config, data)?;
    
    Some((hit_point.x, hit_point.y, hit_point.z))
}

/// Raycast against terrain surface using ray marching
/// Returns the hit point on the terrain surface, or None if no hit
fn raycast_terrain(ray: &bevy::math::Ray3d, config: &TerrainConfig, data: &TerrainData) -> Option<Vec3> {
    // Calculate terrain bounds (consistent with apply_brush_to_chunk and sample_terrain_height)
    let total_chunks_x = config.chunks_x * 2 + 1;
    let total_chunks_z = config.chunks_z * 2 + 1;
    let terrain_size_x = total_chunks_x as f32 * config.chunk_size;
    let terrain_size_z = total_chunks_z as f32 * config.chunk_size;
    let terrain_half_x = terrain_size_x * 0.5;
    let terrain_half_z = terrain_size_z * 0.5;
    
    let terrain_min = Vec3::new(-terrain_half_x, -config.height_scale, -terrain_half_z);
    let terrain_max = Vec3::new(terrain_half_x, config.height_scale * 2.0, terrain_half_z);
    
    // Find ray entry point into terrain bounding box
    let mut t_min = 0.0f32;
    let mut t_max = 10000.0f32;
    
    // Check X bounds
    if ray.direction.x.abs() > 0.0001 {
        let t1 = (terrain_min.x - ray.origin.x) / ray.direction.x;
        let t2 = (terrain_max.x - ray.origin.x) / ray.direction.x;
        t_min = t_min.max(t1.min(t2));
        t_max = t_max.min(t1.max(t2));
    }
    
    // Check Z bounds
    if ray.direction.z.abs() > 0.0001 {
        let t1 = (terrain_min.z - ray.origin.z) / ray.direction.z;
        let t2 = (terrain_max.z - ray.origin.z) / ray.direction.z;
        t_min = t_min.max(t1.min(t2));
        t_max = t_max.min(t1.max(t2));
    }
    
    if t_min > t_max || t_max < 0.0 {
        return None; // Ray misses terrain bounds
    }
    
    t_min = t_min.max(0.0);
    
    // Ray march with adaptive step size
    let mut t = t_min;
    let base_step = 0.5; // Base step size in world units
    let max_iterations = 500;
    
    for _ in 0..max_iterations {
        if t > t_max {
            break;
        }
        
        let pos = ray.origin + ray.direction * t;
        let terrain_height = sample_terrain_height(pos.x, pos.z, config, data);
        
        // Check if we're below terrain surface
        if pos.y <= terrain_height {
            // Binary search refinement for precise hit point
            let mut lo = t - base_step;
            let mut hi = t;
            
            for _ in 0..8 {
                let mid = (lo + hi) * 0.5;
                let mid_pos = ray.origin + ray.direction * mid;
                let mid_height = sample_terrain_height(mid_pos.x, mid_pos.z, config, data);
                
                if mid_pos.y <= mid_height {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            
            let hit_t = (lo + hi) * 0.5;
            let hit_pos = ray.origin + ray.direction * hit_t;
            let final_height = sample_terrain_height(hit_pos.x, hit_pos.z, config, data);
            
            return Some(Vec3::new(hit_pos.x, final_height, hit_pos.z));
        }
        
        // Adaptive step: larger steps when far from surface
        let height_diff = pos.y - terrain_height;
        let step = (base_step * (1.0 + height_diff * 0.1)).min(5.0).max(0.1);
        t += step;
    }
    
    // Fallback: intersect with Y=0 plane if no terrain hit found
    // This ensures brush works even on flat/empty terrain
    if ray.direction.y.abs() > 0.001 {
        let t_ground = -ray.origin.y / ray.direction.y;
        if t_ground > 0.0 {
            let hit = ray.origin + ray.direction * t_ground;
            // Expand bounds slightly to ensure we can paint at edges
            let margin = 5.0;
            if hit.x >= terrain_min.x - margin && hit.x <= terrain_max.x + margin &&
               hit.z >= terrain_min.z - margin && hit.z <= terrain_max.z + margin {
                return Some(Vec3::new(hit.x, 0.0, hit.z));
            }
        }
    }
    
    None
}

/// Sample terrain height at a world XZ position
fn sample_terrain_height(world_x: f32, world_z: f32, config: &TerrainConfig, data: &TerrainData) -> f32 {
    if data.height_cache.is_empty() {
        return 0.0;
    }
    
    // Calculate terrain dimensions (same as apply_brush_to_chunk)
    let total_chunks_x = config.chunks_x * 2 + 1;
    let total_chunks_z = config.chunks_z * 2 + 1;
    let terrain_size_x = total_chunks_x as f32 * config.chunk_size;
    let terrain_size_z = total_chunks_z as f32 * config.chunk_size;
    let terrain_half_x = terrain_size_x * 0.5;
    let terrain_half_z = terrain_size_z * 0.5;
    
    // Convert world position to normalized coordinates (0-1)
    let u = ((world_x + terrain_half_x) / terrain_size_x).clamp(0.0, 1.0);
    let v = ((world_z + terrain_half_z) / terrain_size_z).clamp(0.0, 1.0);
    
    // Convert to cache coordinates
    let cache_width = data.cache_width as f32;
    let cache_height = data.cache_height as f32;
    
    let fx = u * (cache_width - 1.0);
    let fz = v * (cache_height - 1.0);
    
    let x0 = (fx as u32).min(data.cache_width.saturating_sub(2));
    let z0 = (fz as u32).min(data.cache_height.saturating_sub(2));
    let x1 = x0 + 1;
    let z1 = z0 + 1;
    
    let tx = fx - x0 as f32;
    let tz = fz - z0 as f32;
    
    // Bilinear interpolation
    let idx00 = (z0 * data.cache_width + x0) as usize;
    let idx10 = (z0 * data.cache_width + x1) as usize;
    let idx01 = (z1 * data.cache_width + x0) as usize;
    let idx11 = (z1 * data.cache_width + x1) as usize;
    
    let h00 = data.height_cache.get(idx00).copied().unwrap_or(0.0);
    let h10 = data.height_cache.get(idx10).copied().unwrap_or(0.0);
    let h01 = data.height_cache.get(idx01).copied().unwrap_or(0.0);
    let h11 = data.height_cache.get(idx11).copied().unwrap_or(0.0);
    
    let h0 = h00 * (1.0 - tx) + h10 * tx;
    let h1 = h01 * (1.0 - tx) + h11 * tx;
    let height = h0 * (1.0 - tz) + h1 * tz;
    
    height * config.height_scale
}

/// Get heights of neighboring vertices for smoothing
fn get_neighbor_heights(data: &TerrainData, x: u32, z: u32, resolution: u32) -> Vec<f32> {
    let mut heights = Vec::with_capacity(8);
    let stride = resolution + 1;
    
    for dz in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            
            let nx = x as i32 + dx;
            let nz = z as i32 + dz;
            
            if nx >= 0 && nx <= resolution as i32 && nz >= 0 && nz <= resolution as i32 {
                let idx = (nz as u32 * stride + nx as u32) as usize;
                if idx < data.height_cache.len() {
                    heights.push(data.height_cache[idx]);
                }
            }
        }
    }
    
    heights
}

// ============================================================================
// Undo/Redo System
// ============================================================================

/// Handle Ctrl+Z (undo) and Ctrl+Y/Ctrl+Shift+Z (redo) shortcuts
fn handle_undo_redo_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    mut history: ResMut<TerrainHistory>,
    mut terrain_query: Query<(&TerrainConfig, &mut TerrainData), With<TerrainRoot>>,
    mut chunk_query: Query<(Entity, &Chunk)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    mode: Res<TerrainMode>,
) {
    // Only handle in editor mode
    if *mode != TerrainMode::Editor {
        return;
    }
    
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    
    // Ctrl+Z = Undo
    if ctrl && keys.just_pressed(KeyCode::KeyZ) && !shift {
        if let Some(snapshot) = history.undo() {
            apply_snapshot_to_terrain(snapshot, &mut terrain_query, &chunk_query, &mut meshes, &mut commands);
            info!("Terrain: Undo - {}", snapshot.description);
        }
    }
    
    // Ctrl+Y or Ctrl+Shift+Z = Redo
    if ctrl && (keys.just_pressed(KeyCode::KeyY) || (shift && keys.just_pressed(KeyCode::KeyZ))) {
        if let Some(snapshot) = history.redo() {
            apply_snapshot_to_terrain(snapshot, &mut terrain_query, &chunk_query, &mut meshes, &mut commands);
            info!("Terrain: Redo - {}", snapshot.description);
        }
    }
}

/// Apply a snapshot to restore terrain state
fn apply_snapshot_to_terrain(
    snapshot: &TerrainSnapshot,
    terrain_query: &mut Query<(&TerrainConfig, &mut TerrainData), With<TerrainRoot>>,
    chunk_query: &Query<(Entity, &Chunk)>,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) {
    let Ok((config, mut data)) = terrain_query.single_mut() else { return };
    
    // Restore height cache
    data.height_cache = snapshot.height_cache.clone();
    data.cache_width = snapshot.cache_width;
    data.cache_height = snapshot.cache_height;
    
    // Regenerate affected chunk meshes
    for (entity, chunk) in chunk_query.iter() {
        // Check if this chunk was affected
        let affected = snapshot.affected_chunks.is_empty() || 
            snapshot.affected_chunks.contains(&chunk.position);
        
        if affected {
            let new_mesh = generate_chunk_mesh(
                chunk.position,
                chunk.lod,
                config,
                &data,
                meshes,
            );
            commands.entity(entity).insert(Mesh3d(new_mesh));
        }
    }
}

// ============================================================================
// Advanced Brush UI Extension
// ============================================================================

/// Show advanced brushes UI section
#[allow(dead_code)]
fn show_advanced_brushes_ui(
    ui: &mut egui::Ui,
    advanced_state: &mut AdvancedBrushState,
    brush: &mut TerrainBrush,
) {
    ui.heading("Advanced Brushes");
    ui.separator();
    
    // Brush type selector
    egui::ComboBox::from_label("Brush Type")
        .selected_text(advanced_state.selected_type.name())
        .show_ui(ui, |ui| {
            for brush_type in AdvancedBrushType::all() {
                ui.selectable_value(
                    &mut advanced_state.selected_type,
                    *brush_type,
                    brush_type.name(),
                );
            }
        });
    
    ui.add_space(8.0);
    
    match advanced_state.selected_type {
        AdvancedBrushType::None => {
            ui.label("Select a brush type above");
        }
        AdvancedBrushType::Erosion => {
            ui.label("Erosion Settings");
            ui.add(egui::Slider::new(&mut advanced_state.erosion_config.iterations, 1..=200)
                .text("Iterations"));
            ui.add(egui::Slider::new(&mut advanced_state.erosion_config.hydraulic_strength, 0.0..=1.0)
                .text("Hydraulic"));
            ui.add(egui::Slider::new(&mut advanced_state.erosion_config.thermal_strength, 0.0..=1.0)
                .text("Thermal"));
            ui.add(egui::Slider::new(&mut advanced_state.erosion_config.rain_amount, 0.0..=0.1)
                .text("Rain"));
            
            if ui.button("Apply Erosion").clicked() {
                // TODO: Apply erosion to terrain
            }
        }
        AdvancedBrushType::Terrace => {
            ui.label("Terrace Settings");
            // TODO: Terrace settings
        }
        AdvancedBrushType::Cliff => {
            ui.label("Cliff Settings");
            // TODO: Cliff settings
        }
        _ => {
            // Noise brush types
            if let Some(noise_brush) = advanced_state.selected_type.create_brush(brush.radius, brush.strength) {
                advanced_state.noise_brush = Some(noise_brush);
                ui.label("Click and drag to apply noise pattern");
            }
        }
    }
}

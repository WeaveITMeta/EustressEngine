//! # Asset Manager / Browser UI
//!
//! Integrates with `AssetService` for content-addressable asset management.
//!
//! ## Features
//!
//! - Browse local and remote assets
//! - Upload new assets (generates AssetId)
//! - Search and filter by type/tag
//! - View asset details (hash, size, sources)
//! - Drag-drop to viewport

use bevy::prelude::*;
use bevy_egui::egui;
use std::path::PathBuf;
use eustress_common::assets::{ContentHash, AssetInfo, AssetLoadState};

/// Asset entry in the manager (bridges AssetInfo to UI)
#[derive(Clone, Debug)]
pub struct AssetEntry {
    pub name: String,
    pub path: PathBuf,
    pub asset_type: AssetType,
    pub size: u64,
    /// Content-addressable ID (if uploaded to AssetService)
    pub asset_id: Option<ContentHash>,
    /// Load state from AssetService
    pub load_state: AssetLoadState,
    /// MIME type
    pub mime_type: String,
    /// Tags for organization
    pub tags: Vec<String>,
}

impl From<AssetInfo> for AssetEntry {
    fn from(info: AssetInfo) -> Self {
        Self {
            name: info.name.clone(),
            path: PathBuf::from(&info.name),
            asset_type: AssetType::from_mime(&info.mime_type),
            size: info.size as u64,
            asset_id: Some(info.id),
            load_state: AssetLoadState::NotLoaded,
            mime_type: info.mime_type,
            tags: info.tags,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AssetType {
    Model,
    Texture,
    Material,
    Scene,
    Script,
    Audio,
    Video,
    Unknown,
}

impl AssetType {
    /// Determine asset type from MIME type
    pub fn from_mime(mime: &str) -> Self {
        if mime.starts_with("model/") || mime.contains("gltf") || mime.contains("fbx") {
            Self::Model
        } else if mime.starts_with("image/") {
            Self::Texture
        } else if mime.starts_with("audio/") {
            Self::Audio
        } else if mime.starts_with("video/") {
            Self::Video
        } else if mime.contains("ron") || mime.contains("scene") {
            Self::Scene
        } else if mime.contains("lua") || mime.contains("script") {
            Self::Script
        } else if mime.contains("material") {
            Self::Material
        } else {
            Self::Unknown
        }
    }
}

/// View mode for asset browser
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum AssetViewMode {
    #[default]
    List,
    Grid,
    Details,
}

/// Filter options
#[derive(Clone, Debug, Default)]
pub struct AssetFilter {
    pub asset_type: Option<AssetType>,
    pub tag: Option<String>,
    pub cached_only: bool,
}

/// Asset Manager state
#[derive(Resource)]
pub struct AssetManagerState {
    pub show: bool,
    pub assets: Vec<AssetEntry>,
    pub search_filter: String,
    pub selected_asset: Option<usize>,
    pub current_directory: PathBuf,
    /// View mode (list/grid/details)
    pub view_mode: AssetViewMode,
    /// Type/tag filter
    pub filter: AssetFilter,
    /// Show upload dialog
    pub show_upload_dialog: bool,
    /// Upload file path (from file picker)
    pub upload_path: Option<PathBuf>,
    /// Cache statistics
    pub cache_stats: (usize, usize, f64), // (count, bytes, hit_ratio)
    /// Is synced with AssetService?
    pub synced: bool,
}

impl Default for AssetManagerState {
    fn default() -> Self {
        Self {
            show: true,
            assets: Vec::new(),
            search_filter: String::new(),
            selected_asset: None,
            current_directory: PathBuf::from("assets"),
            view_mode: AssetViewMode::List,
            filter: AssetFilter::default(),
            show_upload_dialog: false,
            upload_path: None,
            cache_stats: (0, 0, 0.0),
            synced: false,
        }
    }
}

/// Asset Manager panel
pub struct AssetManagerPanel;

impl AssetManagerPanel {
    /// Show as standalone side panel (primary mode)
    pub fn show_as_panel(ctx: &egui::Context, state: &mut AssetManagerState) {
        egui::SidePanel::left("asset_manager_panel")
            .min_width(250.0)
            .default_width(350.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("📦 Asset Manager");
                
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").clicked() {
                            state.show = false;
                        }
                    });
                });
                
                ui.separator();
                Self::show_content(ui, state);
            });
    }
    
    /// Show as nested panel within Explorer (secondary mode)
    pub fn show_nested(ui: &mut egui::Ui, state: &mut AssetManagerState) {
        ui.push_id("asset_nested", |ui| {
            ui.heading("📦 Asset Manager");
            ui.separator();
            
            Self::show_content(ui, state);
        });
    }
    
    /// Shared content rendering (public for dock system)
    pub fn show_content(ui: &mut egui::Ui, state: &mut AssetManagerState) {
        // Toolbar
        ui.horizontal(|ui| {
            if ui.button("📁 Import").on_hover_text("Import asset from file").clicked() {
                state.show_upload_dialog = true;
            }
            
            if ui.button("📤 Export").on_hover_text("Export selected asset").clicked() {
                Self::export_selected(state);
            }
            
            ui.separator();
            
            if ui.button("🔄 Sync").on_hover_text("Sync with AssetService").clicked() {
                state.synced = false; // Trigger resync
            }
            
            ui.separator();
            
            // View mode toggle
            ui.selectable_value(&mut state.view_mode, AssetViewMode::List, "☰")
                .on_hover_text("List view");
            ui.selectable_value(&mut state.view_mode, AssetViewMode::Grid, "▦")
                .on_hover_text("Grid view");
            ui.selectable_value(&mut state.view_mode, AssetViewMode::Details, "≡")
                .on_hover_text("Details view");
        });
        
        ui.separator();
        
        // Search bar
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut state.search_filter)
                    .hint_text("Search assets...")
                    .desired_width(ui.available_width() - 80.0)
            );
            
            // Type filter dropdown
            egui::ComboBox::from_id_salt("type_filter")
                .selected_text(match &state.filter.asset_type {
                    Some(t) => format!("{:?}", t),
                    None => "All".to_string(),
                })
                .width(60.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.filter.asset_type, None, "All");
                    ui.selectable_value(&mut state.filter.asset_type, Some(AssetType::Model), "Models");
                    ui.selectable_value(&mut state.filter.asset_type, Some(AssetType::Texture), "Textures");
                    ui.selectable_value(&mut state.filter.asset_type, Some(AssetType::Audio), "Audio");
                    ui.selectable_value(&mut state.filter.asset_type, Some(AssetType::Scene), "Scenes");
                });
        });
        
        // Cache stats
        ui.horizontal(|ui| {
            ui.weak(format!(
                "Cache: {} assets, {:.1} MB, {:.0}% hit",
                state.cache_stats.0,
                state.cache_stats.1 as f64 / (1024.0 * 1024.0),
                state.cache_stats.2 * 100.0
            ));
        });
        
        ui.separator();
        
        // Asset list/grid
        egui::ScrollArea::vertical().show(ui, |ui| {
            // Collect filtered indices first to avoid borrow issues
            let filtered_indices: Vec<usize> = state.assets.iter().enumerate()
                .filter(|(_, asset)| {
                    // Text filter
                    let text_match = state.search_filter.is_empty() 
                        || asset.name.to_lowercase().contains(&state.search_filter.to_lowercase());
                    
                    // Type filter
                    let type_match = state.filter.asset_type.is_none() 
                        || state.filter.asset_type.as_ref() == Some(&asset.asset_type);
                    
                    // Cached filter
                    let cache_match = !state.filter.cached_only 
                        || matches!(asset.load_state, AssetLoadState::Loaded);
                    
                    text_match && type_match && cache_match
                })
                .map(|(idx, _)| idx)
                .collect();
            
            if filtered_indices.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.weak("No assets found");
                });
            } else {
                match state.view_mode {
                    AssetViewMode::List => Self::show_list_view_by_indices(ui, state, &filtered_indices),
                    AssetViewMode::Grid => Self::show_grid_view_by_indices(ui, state, &filtered_indices),
                    AssetViewMode::Details => Self::show_details_view_by_indices(ui, state, &filtered_indices),
                }
            }
        });
        
        ui.separator();
        
        // Asset details panel
        if let Some(idx) = state.selected_asset {
            if let Some(asset) = state.assets.get(idx) {
                Self::show_asset_details(ui, asset);
            }
        }
        
        // Upload dialog
        if state.show_upload_dialog {
            Self::show_upload_dialog(ui, state);
        }
    }
    
    /// List view (compact)
    fn show_list_view(ui: &mut egui::Ui, state: &mut AssetManagerState, assets: &[(usize, &AssetEntry)]) {
        for (idx, asset) in assets {
            let is_selected = state.selected_asset == Some(*idx);
            
            let response = ui.selectable_label(
                is_selected,
                format!("{} {} {}", 
                    Self::asset_icon(&asset.asset_type), 
                    asset.name,
                    Self::load_state_icon(&asset.load_state)
                )
            );
            
            if response.clicked() {
                state.selected_asset = Some(*idx);
            }
            
            Self::show_context_menu(&response, asset);
        }
    }
    
    /// Grid view (thumbnails)
    fn show_grid_view(ui: &mut egui::Ui, state: &mut AssetManagerState, assets: &[(usize, &AssetEntry)]) {
        let available_width = ui.available_width();
        let item_size = 80.0;
        let items_per_row = (available_width / item_size).floor() as usize;
        
        egui::Grid::new("asset_grid")
            .num_columns(items_per_row.max(1))
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                for (i, (idx, asset)) in assets.iter().enumerate() {
                    let is_selected = state.selected_asset == Some(*idx);
                    
                    ui.vertical(|ui| {
                        // Thumbnail placeholder
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(60.0, 60.0),
                            egui::Sense::click()
                        );
                        
                        // Draw background
                        let bg_color = if is_selected {
                            egui::Color32::from_rgb(60, 100, 150)
                        } else {
                            egui::Color32::from_rgb(50, 50, 50)
                        };
                        ui.painter().rect_filled(rect, 4.0, bg_color);
                        
                        // Draw icon
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            Self::asset_icon(&asset.asset_type),
                            egui::FontId::proportional(24.0),
                            egui::Color32::WHITE,
                        );
                        
                        if response.clicked() {
                            state.selected_asset = Some(*idx);
                        }
                        
                        Self::show_context_menu(&response, asset);
                        
                        // Name (truncated)
                        let name = if asset.name.len() > 10 {
                            format!("{}...", &asset.name[..8])
                        } else {
                            asset.name.clone()
                        };
                        ui.label(name);
                    });
                    
                    if (i + 1) % items_per_row == 0 {
                        ui.end_row();
                    }
                }
            });
    }
    
    /// Details view (table with all info)
    fn show_details_view(ui: &mut egui::Ui, state: &mut AssetManagerState, assets: &[(usize, &AssetEntry)]) {
        egui::Grid::new("asset_details_grid")
            .num_columns(5)
            .striped(true)
            .show(ui, |ui| {
                // Header
                ui.strong("Type");
                ui.strong("Name");
                ui.strong("Size");
                ui.strong("Status");
                ui.strong("ID");
                ui.end_row();
                
                for (idx, asset) in assets {
                    let is_selected = state.selected_asset == Some(*idx);
                    
                    // Type icon
                    ui.label(Self::asset_icon(&asset.asset_type));
                    
                    // Name (selectable)
                    if ui.selectable_label(is_selected, &asset.name).clicked() {
                        state.selected_asset = Some(*idx);
                    }
                    
                    // Size
                    ui.label(Self::format_size(asset.size));
                    
                    // Status
                    ui.label(Self::load_state_icon(&asset.load_state));
                    
                    // Asset ID (truncated)
                    if let Some(id) = &asset.asset_id {
                        let id_str = id.to_base58();
                        ui.label(&id_str[..12.min(id_str.len())]);
                    } else {
                        ui.weak("—");
                    }
                    
                    ui.end_row();
                }
            });
    }
    
    /// List view by indices (avoids borrow issues)
    fn show_list_view_by_indices(ui: &mut egui::Ui, state: &mut AssetManagerState, indices: &[usize]) {
        for &idx in indices {
            let Some(asset) = state.assets.get(idx) else { continue };
            let is_selected = state.selected_asset == Some(idx);
            
            let response = ui.selectable_label(
                is_selected,
                format!("{} {} {}", 
                    Self::asset_icon(&asset.asset_type), 
                    asset.name.clone(),
                    Self::load_state_icon(&asset.load_state)
                )
            );
            
            if response.clicked() {
                state.selected_asset = Some(idx);
            }
        }
    }
    
    /// Grid view by indices (avoids borrow issues)
    fn show_grid_view_by_indices(ui: &mut egui::Ui, state: &mut AssetManagerState, indices: &[usize]) {
        let available_width = ui.available_width();
        let item_size = 80.0;
        let items_per_row = (available_width / item_size).floor() as usize;
        
        egui::Grid::new("asset_grid")
            .num_columns(items_per_row.max(1))
            .spacing([8.0, 8.0])
            .show(ui, |ui| {
                for (i, &idx) in indices.iter().enumerate() {
                    let Some(asset) = state.assets.get(idx) else { continue };
                    let is_selected = state.selected_asset == Some(idx);
                    let asset_type = asset.asset_type.clone();
                    let asset_name = asset.name.clone();
                    
                    ui.vertical(|ui| {
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(60.0, 60.0),
                            egui::Sense::click()
                        );
                        
                        let bg_color = if is_selected {
                            egui::Color32::from_rgb(60, 100, 150)
                        } else {
                            egui::Color32::from_rgb(50, 50, 50)
                        };
                        ui.painter().rect_filled(rect, 4.0, bg_color);
                        
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            Self::asset_icon(&asset_type),
                            egui::FontId::proportional(24.0),
                            egui::Color32::WHITE,
                        );
                        
                        if response.clicked() {
                            state.selected_asset = Some(idx);
                        }
                        
                        let name = if asset_name.len() > 10 {
                            format!("{}...", &asset_name[..8])
                        } else {
                            asset_name
                        };
                        ui.label(name);
                    });
                    
                    if (i + 1) % items_per_row == 0 {
                        ui.end_row();
                    }
                }
            });
    }
    
    /// Details view by indices (avoids borrow issues)
    fn show_details_view_by_indices(ui: &mut egui::Ui, state: &mut AssetManagerState, indices: &[usize]) {
        egui::Grid::new("asset_details_grid")
            .num_columns(5)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Type");
                ui.strong("Name");
                ui.strong("Size");
                ui.strong("Status");
                ui.strong("ID");
                ui.end_row();
                
                for &idx in indices {
                    let Some(asset) = state.assets.get(idx) else { continue };
                    let is_selected = state.selected_asset == Some(idx);
                    
                    ui.label(Self::asset_icon(&asset.asset_type));
                    
                    if ui.selectable_label(is_selected, &asset.name).clicked() {
                        state.selected_asset = Some(idx);
                    }
                    
                    ui.label(Self::format_size(asset.size));
                    ui.label(Self::load_state_icon(&asset.load_state));
                    
                    if let Some(id) = &asset.asset_id {
                        let id_str = id.to_base58();
                        ui.label(&id_str[..12.min(id_str.len())]);
                    } else {
                        ui.weak("—");
                    }
                    
                    ui.end_row();
                }
            });
    }
    
    /// Show asset details panel
    fn show_asset_details(ui: &mut egui::Ui, asset: &AssetEntry) {
        ui.group(|ui| {
            ui.heading(format!("{} {}", Self::asset_icon(&asset.asset_type), asset.name));
            ui.separator();
            
            egui::Grid::new("asset_detail_grid")
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Type:");
                    ui.label(format!("{:?}", asset.asset_type));
                    ui.end_row();
                    
                    ui.label("MIME:");
                    ui.label(&asset.mime_type);
                    ui.end_row();
                    
                    ui.label("Size:");
                    ui.label(Self::format_size(asset.size));
                    ui.end_row();
                    
                    ui.label("Status:");
                    ui.label(format!("{} {:?}", 
                        Self::load_state_icon(&asset.load_state),
                        asset.load_state
                    ));
                    ui.end_row();
                    
                    if let Some(id) = &asset.asset_id {
                        ui.label("Asset ID:");
                        let id_str = id.to_base58();
                        if ui.small_button("📋").on_hover_text("Copy to clipboard").clicked() {
                            ui.ctx().copy_text(id_str.clone());
                        }
                        ui.label(&id_str[..20.min(id_str.len())]);
                        ui.end_row();
                    }
                    
                    if !asset.tags.is_empty() {
                        ui.label("Tags:");
                        ui.label(asset.tags.join(", "));
                        ui.end_row();
                    }
                });
        });
    }
    
    /// Show upload dialog
    fn show_upload_dialog(ui: &mut egui::Ui, state: &mut AssetManagerState) {
        egui::Window::new("📁 Import Asset")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label("Select a file to import:");
                
                if let Some(path) = &state.upload_path {
                    ui.label(format!("Selected: {}", path.display()));
                } else {
                    ui.weak("No file selected");
                }
                
                ui.horizontal(|ui| {
                    if ui.button("Browse...").clicked() {
                        // TODO: Open file picker (rfd crate)
                        // For now, use a placeholder
                        state.upload_path = Some(PathBuf::from("assets/new_asset.glb"));
                    }
                    
                    if ui.button("Import").clicked() {
                        if state.upload_path.is_some() {
                            Self::import_asset(state);
                            state.show_upload_dialog = false;
                            state.upload_path = None;
                        }
                    }
                    
                    if ui.button("Cancel").clicked() {
                        state.show_upload_dialog = false;
                        state.upload_path = None;
                    }
                });
            });
    }
    
    /// Context menu for assets
    fn show_context_menu(response: &egui::Response, asset: &AssetEntry) {
        response.context_menu(|ui| {
            if ui.button("📂 Open").clicked() {
                // TODO: Open asset in editor
                ui.close();
            }
            
            if let Some(id) = &asset.asset_id {
                if ui.button("📋 Copy ID").clicked() {
                    ui.ctx().copy_text(id.to_base58());
                    ui.close();
                }
            }
            
            ui.separator();
            
            if ui.button("✏️ Rename").clicked() {
                // TODO: Rename asset
                ui.close();
            }
            
            if ui.button("🗑 Delete").clicked() {
                // TODO: Delete asset
                ui.close();
            }
        });
    }
    
    fn asset_icon(asset_type: &AssetType) -> &'static str {
        match asset_type {
            AssetType::Model => "🎨",
            AssetType::Texture => "🖼",
            AssetType::Material => "✨",
            AssetType::Scene => "🌍",
            AssetType::Script => "📜",
            AssetType::Audio => "🔊",
            AssetType::Video => "🎬",
            AssetType::Unknown => "📄",
        }
    }
    
    fn load_state_icon(state: &AssetLoadState) -> &'static str {
        match state {
            AssetLoadState::NotLoaded => "○",
            AssetLoadState::Loading => "◐",
            AssetLoadState::Loaded => "●",
            AssetLoadState::Failed(_) => "✗",
        }
    }
    
    fn format_size(bytes: u64) -> String {
        if bytes < 1024 {
            format!("{} B", bytes)
        } else if bytes < 1024 * 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.0)
        } else if bytes < 1024 * 1024 * 1024 {
            format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }
    
    fn import_asset(state: &mut AssetManagerState) {
        // TODO: Integrate with AssetService.upload()
        // For now, add a placeholder entry
        if let Some(path) = &state.upload_path {
            let name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("New Asset")
                .to_string();
            
            let mime_type = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            
            state.assets.push(AssetEntry {
                name,
                path: path.clone(),
                asset_type: AssetType::from_mime(&mime_type),
                size: 0, // Will be filled after upload
                asset_id: None, // Will be filled after upload
                load_state: AssetLoadState::NotLoaded,
                mime_type,
                tags: Vec::new(),
            });
        }
    }
    
    fn export_selected(state: &AssetManagerState) {
        if let Some(idx) = state.selected_asset {
            if let Some(asset) = state.assets.get(idx) {
                if let Some(id) = &asset.asset_id {
                    // TODO: Integrate with AssetService.load() + file save
                    println!("Exporting asset: {} ({})", asset.name, id.to_base58());
                }
            }
        }
    }
    
    /// Sync assets from AssetService
    pub fn sync_from_service(state: &mut AssetManagerState, assets: Vec<AssetInfo>) {
        state.assets = assets.into_iter().map(AssetEntry::from).collect();
        state.synced = true;
    }
    
    /// Update cache stats from AssetService
    pub fn update_cache_stats(state: &mut AssetManagerState, stats: (usize, usize, f64)) {
        state.cache_stats = stats;
    }
}

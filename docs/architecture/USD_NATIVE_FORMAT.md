//! # Soul Script Editor
//!
//! VS Code-style tabbed editor for Soul scripts that overlays the 3D viewport.
//! - Scene tab is always pinned left
//! - Double-click Soul Script in Explorer opens a new tab
//! - Monaco-style markdown editor with syntax highlighting

use bevy::prelude::*;
use bevy_egui::egui;
use std::collections::HashMap;

use crate::soul::{SoulScriptData, SoulBuildStatus};
use crate::classes::Instance;

// ============================================================================
// Editor Tab Types
// ============================================================================

/// A tab in the script editor
#[derive(Debug, Clone)]
pub struct EditorTab {
    /// Entity ID of the Soul Script (None for Scene tab)
    pub entity: Option<Entity>,
    /// Display name
    pub name: String,
    /// Whether this tab is pinned (Scene tab is always pinned)
    pub pinned: bool,
    /// Whether the content has unsaved changes
    pub dirty: bool,
    /// Tab type
    pub tab_type: EditorTabType,
}

/// Type of editor tab
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorTabType {
    /// 3D Scene viewport (always pinned left)
    Scene,
    /// Soul Script markdown editor
    SoulScript,
    /// Parameters/Data Source editor for an entity
    ParametersEditor,
    /// Document viewer (PDF, DOCX, PPTX, XLSX)
    Document { doc_type: DocumentType },
    /// Image viewer (PNG, JPG, GIF, WebP, SVG)
    ImageViewer,
    /// Video player (MP4, WebM, etc.)
    VideoPlayer,
    /// Web browser tab
    WebBrowser,
}

/// Document types supported in the tabbed viewer
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentType {
    /// PDF document
    Pdf,
    /// Microsoft Word document
    Docx,
    /// Microsoft PowerPoint presentation
    Pptx,
    /// Microsoft Excel spreadsheet
    Xlsx,
    /// Google Docs (via URL)
    GoogleDoc,
    /// Google Sheets (via URL)
    GoogleSheet,
    /// Google Slides (via URL)
    GoogleSlides,
    /// Plain text / Markdown
    Text,
}

/// View mode for Soul Script editor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScriptViewMode {
    /// Show markdown source (editable)
    #[default]
    Markdown,
    /// Show generated Rune code (read-only)
    Rune,
}

impl EditorTab {
    /// Create the Space tab (always exists, always pinned)
    pub fn scene() -> Self {
        Self {
            entity: None,
            name: "Space".to_string(),
            pinned: true,
            dirty: false,
            tab_type: EditorTabType::Scene,
        }
    }
    
    /// Create a Soul Script tab
    pub fn soul_script(entity: Entity, name: String) -> Self {
        Self {
            entity: Some(entity),
            name,
            pinned: false,
            dirty: false,
            tab_type: EditorTabType::SoulScript,
        }
    }
    
    /// Create a Parameters Editor tab for an entity
    pub fn parameters_editor(entity: Entity, name: String) -> Self {
        Self {
            entity: Some(entity),
            name: format!("{} - Parameters", name),
            pinned: false,
            dirty: false,
            tab_type: EditorTabType::ParametersEditor,
        }
    }
    
    /// Create a Document viewer tab
    pub fn document(entity: Entity, name: String, doc_type: DocumentType) -> Self {
        Self {
            entity: Some(entity),
            name,
            pinned: false,
            dirty: false,
            tab_type: EditorTabType::Document { doc_type },
        }
    }
    
    /// Create an Image viewer tab
    pub fn image_viewer(entity: Entity, name: String) -> Self {
        Self {
            entity: Some(entity),
            name,
            pinned: false,
            dirty: false,
            tab_type: EditorTabType::ImageViewer,
        }
    }
    
    /// Create a Video player tab
    pub fn video_player(entity: Entity, name: String) -> Self {
        Self {
            entity: Some(entity),
            name,
            pinned: false,
            dirty: false,
            tab_type: EditorTabType::VideoPlayer,
        }
    }
    
    /// Get icon for this tab type
    pub fn icon(&self) -> &'static str {
        match &self.tab_type {
            EditorTabType::Scene => "üé¨",
            EditorTabType::SoulScript => "üìú",
            EditorTabType::ParametersEditor => "üîó",
            EditorTabType::Document { doc_type } => match doc_type {
                DocumentType::Pdf => "üìï",
                DocumentType::Docx => "üìò",
                DocumentType::Pptx => "üìô",
                DocumentType::Xlsx => "üìó",
                DocumentType::GoogleDoc => "üìÑ",
                DocumentType::GoogleSheet => "üìä",
                DocumentType::GoogleSlides => "üìΩ",
                DocumentType::Text => "üìù",
            },
            EditorTabType::ImageViewer => "üñº",
            EditorTabType::VideoPlayer => "üé•",
            EditorTabType::WebBrowser => "üåê",
        }
    }
    
    /// Create a Web Browser tab
    pub fn web_browser(url: &str, title: &str) -> Self {
        Self {
            entity: None,
            name: title.to_string(),
            pinned: false,
            dirty: false,
            tab_type: EditorTabType::WebBrowser,
        }
    }
}

// ============================================================================
// Web Browser State
// ============================================================================

/// State for a single browser tab
#[derive(Debug, Clone)]
pub struct BrowserTabState {
    /// Current URL
    pub url: String,
    /// Page title
    pub title: String,
    /// Loading state
    pub loading: bool,
    /// Can go back in history
    pub can_go_back: bool,
    /// Can go forward in history
    pub can_go_forward: bool,
    /// Navigation history (URLs)
    pub history: Vec<String>,
    /// Current position in history
    pub history_index: usize,
    /// URL bar text (may differ from url while typing)
    pub url_bar_text: String,
    /// Is URL bar focused for editing
    pub url_bar_editing: bool,
    /// Fetched page content (HTML or text)
    pub content: String,
    /// Content type (text/html, text/plain, etc.)
    pub content_type: String,
    /// Error message if fetch failed
    pub error: Option<String>,
    /// Whether content needs to be fetched
    pub needs_fetch: bool,
}

impl Default for BrowserTabState {
    fn default() -> Self {
        Self {
            url: "about:blank".to_string(),
            title: "New Tab".to_string(),
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            history: vec!["about:blank".to_string()],
            history_index: 0,
            url_bar_text: String::new(),
            url_bar_editing: false,
            content: String::new(),
            content_type: String::new(),
            error: None,
            needs_fetch: false,
        }
    }
}

impl BrowserTabState {
    /// Create a new browser tab with a URL
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            title: url.to_string(),
            loading: true,
            can_go_back: false,
            can_go_forward: false,
            history: vec![url.to_string()],
            history_index: 0,
            url_bar_text: url.to_string(),
            url_bar_editing: false,
            content: String::new(),
            content_type: String::new(),
            error: None,
            needs_fetch: true,
        }
    }
    
    /// Navigate to a new URL
    pub fn navigate(&mut self, url: &str) {
        // Truncate forward history
        self.history.truncate(self.history_index + 1);
        self.history.push(url.to_string());
        self.history_index = self.history.len() - 1;
        self.url = url.to_string();
        self.url_bar_text = url.to_string();
        self.loading = true;
        self.needs_fetch = true;
        self.content.clear();
        self.error = None;
        self.update_nav_state();
    }
    
    /// Go back in history
    pub fn go_back(&mut self) {
        if self.can_go_back {
            self.history_index -= 1;
            self.url = self.history[self.history_index].clone();
            self.url_bar_text = self.url.clone();
            self.loading = true;
            self.needs_fetch = true;
            self.content.clear();
            self.error = None;
            self.update_nav_state();
        }
    }
    
    /// Go forward in history
    pub fn go_forward(&mut self) {
        if self.can_go_forward {
            self.history_index += 1;
            self.url = self.history[self.history_index].clone();
            self.url_bar_text = self.url.clone();
            self.loading = true;
            self.needs_fetch = true;
            self.content.clear();
            self.error = None;
            self.update_nav_state();
        }
    }
    
    /// Refresh current page
    pub fn refresh(&mut self) {
        self.loading = true;
        self.needs_fetch = true;
        self.content.clear();
        self.error = None;
    }
    
    /// Fetch content from URL (blocking - should be called from background thread)
    pub fn fetch_content(&mut self) {
        if !self.needs_fetch {
            return;
        }
        self.needs_fetch = false;
        
        // Handle special URLs
        if self.url == "about:blank" {
            self.content = String::new();
            self.loading = false;
            return;
        }
        
        // Try to fetch the URL
        match ureq::get(&self.url)
            .timeout(std::time::Duration::from_secs(10))
            .call()
        {
            Ok(response) => {
                self.content_type = response.content_type().to_string();
                
                // Try to extract title from response
                if let Some(title) = response.header("title") {
                    self.title = title.to_string();
                }
                
                match response.into_string() {
                    Ok(body) => {
                        // Try to extract title from HTML
                        if self.content_type.contains("html") {
                            if let Some(start) = body.find("<title>") {
                                if let Some(end) = body[start..].find("</title>") {
                                    self.title = body[start + 7..start + end].to_string();
                                }
                            }
                        }
                        self.content = body;
                        self.error = None;
                    }
                    Err(e) => {
                        self.error = Some(format!("Failed to read response: {}", e));
                        self.content.clear();
                    }
                }
            }
            Err(e) => {
                self.error = Some(format!("Failed to fetch: {}", e));
                self.content.clear();
            }
        }
        self.loading = false;
    }
    
    /// Update navigation state
    fn update_nav_state(&mut self) {
        self.can_go_back = self.history_index > 0;
        self.can_go_forward = self.history_index < self.history.len() - 1;
    }
}

/// Browser bookmarks
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BrowserBookmarks {
    /// Bookmark folders
    pub folders: Vec<BookmarkFolder>,
    /// Quick access bookmarks (shown in toolbar)
    pub quick_access: Vec<Bookmark>,
}

/// A bookmark folder
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BookmarkFolder {
    pub name: String,
    pub bookmarks: Vec<Bookmark>,
}

/// A single bookmark
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
    pub favicon: Option<String>,
}

/// Browser history entry
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub timestamp: u64,
    pub visit_count: u32,
}

/// Global browser state resource
#[derive(Resource, Default)]
pub struct BrowserState {
    /// Browser tab states (keyed by tab index in ScriptEditorState)
    pub tab_states: HashMap<usize, BrowserTabState>,
    /// Bookmarks
    pub bookmarks: BrowserBookmarks,
    /// History (recent first)
    pub history: Vec<HistoryEntry>,
    /// Default home page
    pub home_page: String,
    /// Show bookmarks bar
    pub show_bookmarks_bar: bool,
}

// ============================================================================
// Editor State Resource
// ============================================================================

/// Resource managing the tabbed editor state
#[derive(Resource)]
pub struct ScriptEditorState {
    /// All open tabs (Scene tab is always first)
    pub tabs: Vec<EditorTab>,
    /// Index of the currently active tab
    pub active_tab: usize,
    /// Cached script sources for editing (entity -> source)
    pub edit_buffers: HashMap<Entity, String>,
    /// Whether the editor overlay is visible (false = only 3D viewport)
    pub show_tabs: bool,
    /// Entity to highlight in Explorer (set by double-click on tab, consumed by UI system)
    pub entity_to_highlight: Option<Entity>,
    /// Current view mode for script editor (Markdown or Rune)
    pub view_mode: ScriptViewMode,
    /// Index of tab being dragged (for drag-and-drop reordering)
    pub dragging_tab: Option<usize>,
}

impl Default for ScriptEditorState {
    fn default() -> Self {
        Self {
            tabs: vec![EditorTab::scene()],
            active_tab: 0,
            edit_buffers: HashMap::new(),
            show_tabs: true,
            entity_to_highlight: None,
            view_mode: ScriptViewMode::default(),
            dragging_tab: None,
        }
    }
}

impl ScriptEditorState {
    /// Move a tab from one index to another (respects pinned/unpinned sections)
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from == 0 || to == 0 {
            return; // Cannot move Scene tab
        }
        if from >= self.tabs.len() || to >= self.tabs.len() {
            return;
        }
        if from == to {
            return;
        }
        
        let from_pinned = self.tabs[from].pinned;
        let to_pinned = self.tabs[to].pinned;
        
        // Only allow moving within the same section (pinned or unpinned)
        if from_pinned != to_pinned {
            return;
        }
        
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        
        // Adjust active tab index
        if self.active_tab == from {
            self.active_tab = to;
        } else if from < self.active_tab && to >= self.active_tab {
            self.active_tab -= 1;
        } else if from > self.active_tab && to <= self.active_tab {
            self.active_tab += 1;
        }
    }
    
    /// Open a Soul Script in a new tab (or focus existing tab)
    pub fn open_script(&mut self, entity: Entity, name: &str, source: &str) {
        // Check if already open
        if let Some(idx) = self.tabs.iter().position(|t| t.entity == Some(entity) && t.tab_type == EditorTabType::SoulScript) {
            self.active_tab = idx;
            return;
        }
        
        // Add new tab
        let tab = EditorTab::soul_script(entity, name.to_string());
        self.tabs.push(tab);
        self.edit_buffers.insert(entity, source.to_string());
        self.active_tab = self.tabs.len() - 1;
    }
    
    /// Open a Parameters Editor tab for an entity (or focus existing tab)
    pub fn open_parameters_editor(&mut self, entity: Entity, name: &str) {
        // Check if already open
        if let Some(idx) = self.tabs.iter().position(|t| t.entity == Some(entity) && t.tab_type == EditorTabType::ParametersEditor) {
            self.active_tab = idx;
            return;
        }
        
        // Add new tab
        let tab = EditorTab::parameters_editor(entity, name.to_string());
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }
    
    /// Open an Image viewer tab (or focus existing tab)
    pub fn open_image(&mut self, entity: Entity, name: &str) {
        if let Some(idx) = self.tabs.iter().position(|t| t.entity == Some(entity) && t.tab_type == EditorTabType::ImageViewer) {
            self.active_tab = idx;
            return;
        }
        let tab = EditorTab::image_viewer(entity, name.to_string());
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }
    
    /// Open a Video player tab (or focus existing tab)
    pub fn open_video(&mut self, entity: Entity, name: &str) {
        if let Some(idx) = self.tabs.iter().position(|t| t.entity == Some(entity) && t.tab_type == EditorTabType::VideoPlayer) {
            self.active_tab = idx;
            return;
        }
        let tab = EditorTab::video_player(entity, name.to_string());
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }
    
    /// Open a Document viewer tab (or focus existing tab)
    pub fn open_document(&mut self, entity: Entity, name: &str, doc_type: DocumentType) {
        if let Some(idx) = self.tabs.iter().position(|t| {
            t.entity == Some(entity) && matches!(t.tab_type, EditorTabType::Document { .. })
        }) {
            self.active_tab = idx;
            return;
        }
        let tab = EditorTab::document(entity, name.to_string(), doc_type);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }
    
    /// Open a Web Browser tab (or focus existing tab with same URL)
    pub fn open_browser(&mut self, url: &str, title: &str) -> usize {
        // Check if already open with same URL
        if let Some(idx) = self.tabs.iter().position(|t| {
            t.tab_type == EditorTabType::WebBrowser && t.name == title
        }) {
            self.active_tab = idx;
            return idx;
        }
        
        // Add new tab
        let tab = EditorTab::web_browser(url, title);
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.active_tab
    }
    
    /// Open a new empty browser tab
    pub fn open_new_browser_tab(&mut self) -> usize {
        let tab = EditorTab::web_browser("about:blank", "New Tab");
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.active_tab
    }
    
    /// Close a tab by index (cannot close Scene tab)
    pub fn close_tab(&mut self, index: usize) {
        if index == 0 || index >= self.tabs.len() {
            return; // Cannot close Scene tab
        }
        
        // Remove edit buffer if it's a script
        if let Some(entity) = self.tabs[index].entity {
            self.edit_buffers.remove(&entity);
        }
        
        self.tabs.remove(index);
        
        // Adjust active tab
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if self.active_tab > index {
            self.active_tab -= 1;
        }
    }
    
    /// Get the active tab
    pub fn active(&self) -> Option<&EditorTab> {
        self.tabs.get(self.active_tab)
    }
    
    /// Check if showing 3D viewport (Scene tab active or no tabs)
    pub fn is_scene_active(&self) -> bool {
        self.active_tab == 0 || self.tabs.get(self.active_tab).map(|t| t.tab_type == EditorTabType::Scene).unwrap_or(true)
    }
    
    /// Mark a script tab as dirty
    pub fn mark_dirty(&mut self, entity: Entity) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.entity == Some(entity)) {
            tab.dirty = true;
        }
    }
    
    /// Pin a tab by index (moves it to the pinned section)
    pub fn pin_tab(&mut self, index: usize) {
        if index == 0 || index >= self.tabs.len() {
            return; // Scene tab is always pinned, can't pin it again
        }
        
        if self.tabs[index].pinned {
            return; // Already pinned
        }
        
        self.tabs[index].pinned = true;
        
        // Move to end of pinned section
        let tab = self.tabs.remove(index);
        let pinned_count = self.tabs.iter().filter(|t| t.pinned).count();
        self.tabs.insert(pinned_count, tab);
        
        // Adjust active tab index
        if self.active_tab == index {
            self.active_tab = pinned_count;
        } else if self.active_tab > index && self.active_tab <= pinned_count {
            // Tab moved from after active to before/at active
        } else if self.active_tab < index && self.active_tab >= pinned_count {
            self.active_tab += 1;
        }
    }
    
    /// Unpin a tab by index (moves it to the unpinned section)
    pub fn unpin_tab(&mut self, index: usize) {
        if index == 0 || index >= self.tabs.len() {
            return; // Scene tab cannot be unpinned
        }
        
        if !self.tabs[index].pinned {
            return; // Already unpinned
        }
        
        self.tabs[index].pinned = false;
        
        // Move to start of unpinned section (right after pinned tabs)
        let tab = self.tabs.remove(index);
        let pinned_count = self.tabs.iter().filter(|t| t.pinned).count();
        self.tabs.insert(pinned_count, tab);
        
        // Adjust active tab index
        if self.active_tab == index {
            self.active_tab = pinned_count;
        }
    }
    
    /// Get count of pinned tabs
    pub fn pinned_count(&self) -> usize {
        self.tabs.iter().filter(|t| t.pinned).count()
    }
}

// ============================================================================
// Event for opening scripts
// ============================================================================

/// Event to open a Soul Script in the editor
#[derive(Event, Message)]
pub struct OpenScriptEvent {
    pub entity: Entity,
}

// ============================================================================
// UI Rendering
// ============================================================================

/// Render the tab bar at the top of the viewport area
pub fn render_tab_bar(
    ui: &mut egui::Ui,
    state: &mut ScriptEditorState,
) -> bool {
    let mut scene_active = state.is_scene_active();
    let mut tab_to_close: Option<usize> = None;
    let mut tab_to_pin: Option<usize> = None;
    let mut tab_to_unpin: Option<usize> = None;
    let mut close_others_except: Option<usize> = None;
    let mut close_all_unpinned = false;
    let mut new_active_tab: Option<usize> = None;
    let mut highlight_entity: Option<Entity> = None;
    let mut drag_drop_move: Option<(usize, usize)> = None;
    
    let pinned_count = state.pinned_count();
    let active_tab = state.active_tab;
    let dragging_tab = state.dragging_tab;
    
    // Collect tab info to avoid borrow issues
    let tab_info: Vec<_> = state.tabs.iter().enumerate().map(|(idx, tab)| {
        (idx, tab.name.clone(), tab.icon().to_string(), tab.pinned, tab.dirty, tab.entity)
    }).collect();
    
    ui.horizontal(|ui| {
        ui.set_height(28.0);
        ui.style_mut().spacing.item_spacing = egui::vec2(2.0, 0.0);
        
        for (idx, name, icon, pinned, dirty, entity) in tab_info.iter() {
            let idx = *idx;
            let is_active = idx == active_tab;
            let is_scene_tab = idx == 0; // Scene tab is always first and always pinned
            let is_being_dragged = dragging_tab == Some(idx);
            
            // Add separator between pinned and unpinned tabs
            if idx == pinned_count && pinned_count > 0 {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            }
            
            // Tab styling - pinned tabs have a subtle accent, dragged tabs have highlight
            let bg_color = if is_being_dragged {
                egui::Color32::from_rgb(60, 80, 100) // Highlight when dragging
            } else if is_active {
                egui::Color32::from_rgb(45, 45, 48)
            } else if *pinned && !is_scene_tab {
                egui::Color32::from_rgb(35, 38, 45) // Slight blue tint for pinned
            } else {
                egui::Color32::from_rgb(30, 30, 30)
            };
            
            let text_color = if is_active || is_being_dragged {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_rgb(180, 180, 180)
            };
            
            // Tab frame
            let frame_response = egui::Frame::new()
                .fill(bg_color)
                .inner_margin(8.0)
                .corner_radius(egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Pin indicator for pinned tabs (except Scene)
                        if *pinned && !is_scene_tab {
                            ui.label(egui::RichText::new("üìå").size(10.0));
                        }
                        
                        // Tab label (clickable)
                        let label = if *dirty {
                            format!("{} {}*", icon, name)
                        } else {
                            format!("{} {}", icon, name)
                        };
                        
                        let response = ui.selectable_label(is_active, egui::RichText::new(&label).color(text_color));
                        if response.clicked() {
                            new_active_tab = Some(idx);
                        }
                        
                        // Double-click to highlight in Explorer
                        if response.double_clicked() {
                            if let Some(e) = entity {
                                highlight_entity = Some(*e);
                            }
                        }
                        
                        // Close button (not for pinned tabs)
                        if !*pinned {
                            if ui.small_button("√ó").clicked() {
                                tab_to_close = Some(idx);
                            }
                        }
                    });
                }).response;
            
            // Drag-and-drop handling (not for Scene tab)
            if !is_scene_tab {
                let response = frame_response.interact(egui::Sense::drag());
                
                // Start dragging
                if response.drag_started() {
                    state.dragging_tab = Some(idx);
                }
                
                // Show drop indicator when hovering over a valid drop target
                if let Some(drag_idx) = dragging_tab {
                    if drag_idx != idx && response.hovered() {
                        // Check if drop is valid (same pinned state)
                        let drag_pinned = tab_info.get(drag_idx).map(|t| t.3).unwrap_or(false);
                        if drag_pinned == *pinned {
                            // Draw drop indicator
                            let rect = response.rect;
                            let painter = ui.painter();
                            let indicator_x = if drag_idx < idx {
                                rect.right()
                            } else {
                                rect.left()
                            };
                            painter.line_segment(
                                [egui::pos2(indicator_x, rect.top()), egui::pos2(indicator_x, rect.bottom())],
                                egui::Stroke::new(2.0, egui::Color32::from_rgb(100, 150, 255))
                            );
                        }
                    }
                }
                
                // Handle drop
                if response.hovered() && ui.input(|i| i.pointer.any_released()) {
                    if let Some(drag_idx) = dragging_tab {
                        if drag_idx != idx {
                            drag_drop_move = Some((drag_idx, idx));
                        }
                    }
                }
            }
            
            // Right-click context menu (not for Scene tab)
            if !is_scene_tab {
                frame_response.context_menu(|ui| {
                    if *pinned {
                        if ui.button("üìå Unpin Tab").clicked() {
                            tab_to_unpin = Some(idx);
                            ui.close();
                        }
                    } else {
                        if ui.button("üìå Pin Tab").clicked() {
                            tab_to_pin = Some(idx);
                            ui.close();
                        }
                    }
                    
                    ui.separator();
                    
                    if ui.button("√ó Close Tab").clicked() {
                        tab_to_close = Some(idx);
                        ui.close();
                    }
                    
                    if ui.button("√ó Close Other Tabs").clicked() {
                        close_others_except = Some(idx);
                        ui.close();
                    }
                    
                    if ui.button("√ó Close All Unpinned").clicked() {
                        close_all_unpinned = true;
                        ui.close();
                    }
                });
            }
        }
        
        // Add spacer to push tabs left
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |_ui| {
            // Could add "+" button here to create new script
        });
    });
    
    // Handle active tab change
    if let Some(idx) = new_active_tab {
        state.active_tab = idx;
    }
    
    // Handle highlight entity
    if let Some(entity) = highlight_entity {
        state.entity_to_highlight = Some(entity);
    }
    
    // Handle pin/unpin actions
    if let Some(idx) = tab_to_pin {
        state.pin_tab(idx);
    }
    if let Some(idx) = tab_to_unpin {
        state.unpin_tab(idx);
    }
    
    // Handle close others
    if let Some(keep_idx) = close_others_except {
        for i in (1..state.tabs.len()).rev() {
            if i != keep_idx && !state.tabs[i].pinned {
                state.close_tab(i);
            }
        }
    }
    
    // Handle close all unpinned
    if close_all_unpinned {
        for i in (1..state.tabs.len()).rev() {
            if !state.tabs[i].pinned {
                state.close_tab(i);
            }
        }
    }
    
    // Close tab if requested
    if let Some(idx) = tab_to_close {
        state.close_tab(idx);
    }
    
    // Handle drag-and-drop move
    if let Some((from, to)) = drag_drop_move {
        state.move_tab(from, to);
        state.dragging_tab = None;
    }
    
    // Clear dragging state when mouse released
    if ui.input(|i| i.pointer.any_released()) {
        state.dragging_tab = None;
    }
    
    scene_active = state.is_scene_active();
    scene_active
}

/// Render the script editor content (when a script tab is active)
pub fn render_script_editor(
    ui: &mut egui::Ui,
    state: &mut ScriptEditorState,
    script_query: &mut Query<(&Instance, &mut SoulScriptData)>,
) {
    let active_tab = match state.tabs.get(state.active_tab) {
        Some(tab) => tab.clone(),
        None => return,
    };
    
    // Only render for script tabs
    if active_tab.tab_type != EditorTabType::SoulScript {
        return;
    }
    
    let entity = match active_tab.entity {
        Some(e) => e,
        None => return,
    };
    
    // Get the edit buffer
    let source = state.edit_buffers.get(&entity).cloned().unwrap_or_default();
    let mut new_source = source.clone();
    
    // Editor panel
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(30, 30, 30))
        .show(ui, |ui| {
            // Toolbar
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&active_tab.name).strong());
                ui.separator();
                
                if ui.button("üî® Build").clicked() {
                    // TODO: Trigger build
                }
                
                // Build status
                if let Ok((_instance, script_data)) = script_query.get(entity) {
                    let status_text = match script_data.build_status {
                        SoulBuildStatus::NotBuilt => "‚ö™ Not Built",
                        SoulBuildStatus::Building => "üîÑ Building...",
                        SoulBuildStatus::Built => "‚úÖ Built",
                        SoulBuildStatus::Failed => "‚ùå Failed",
                        SoulBuildStatus::Stale => "‚ö†Ô∏è Stale",
                    };
                    ui.label(status_text);
                }
                
                // Push toggle to right corner
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // View mode toggle - Markdown | Rune (reversed order for right-to-left)
                    let rune_selected = state.view_mode == ScriptViewMode::Rune;
                    let md_selected = state.view_mode == ScriptViewMode::Markdown;
                    
                    if ui.selectable_label(rune_selected, "ü¶Ä Rune").clicked() {
                        state.view_mode = ScriptViewMode::Rune;
                    }
                    if ui.selectable_label(md_selected, "üìù Markdown").clicked() {
                        state.view_mode = ScriptViewMode::Markdown;
                    }
                });
            });
            
            ui.separator();
            
            // Get generated code for preview
            let generated_code = if let Ok((_instance, script_data)) = script_query.get(entity) {
                script_data.generated_code.clone()
            } else {
                None
            };
            
            // Show content based on view mode
            match state.view_mode {
                ScriptViewMode::Markdown => {
                    // Editable markdown source
                    egui::ScrollArea::vertical()
                        .id_salt("soul_script_editor")
                        .show(ui, |ui| {
                            let response = egui::TextEdit::multiline(&mut new_source)
                                .font(egui::TextStyle::Monospace)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(35)
                                .show(ui);
                            
                            if response.response.changed() {
                                // Update buffer and mark dirty
                                state.edit_buffers.insert(entity, new_source.clone());
                                state.mark_dirty(entity);
                                
                                // Mark build status as Stale if code was previously generated
                                if let Ok((_inst, mut script_data)) = script_query.get_mut(entity) {
                                    if script_data.generated_code.is_some() {
                                        script_data.build_status = SoulBuildStatus::Stale;
                                    }
                                }
                            }
                        });
                }
                ScriptViewMode::Rune => {
                    // Read-only generated Rune code
                    if let Some(ref code) = generated_code {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Generated Rune Code").small().color(egui::Color32::GRAY));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("üìã Copy").clicked() {
                                    ui.ctx().copy_text(code.clone());
                                }
                            });
                        });
                        
                        egui::ScrollArea::vertical()
                            .id_salt("generated_code_preview")
                            .show(ui, |ui| {
                                let mut code_display = code.clone();
                                egui::TextEdit::multiline(&mut code_display)
                                    .font(egui::TextStyle::Monospace)
                                    .code_editor()
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(35)
                                    .interactive(false)
                                    .show(ui);
                            });
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label(egui::RichText::new("No Rune code generated yet.\nClick 'Build' to generate code from your Markdown.")
                                .color(egui::Color32::GRAY));
                        });
                    }
                }
            }
        });
}

// ============================================================================
// Systems
// ============================================================================

/// System to handle opening scripts from Explorer double-click
pub fn handle_open_script_events(
    mut events: MessageReader<OpenScriptEvent>,
    mut state: ResMut<ScriptEditorState>,
    query: Query<(&Instance, &SoulScriptData)>,
) {
    for event in events.read() {
        if let Ok((instance, script_data)) = query.get(event.entity) {
            state.open_script(event.entity, &instance.name, &script_data.source);
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct ScriptEditorPlugin;

impl Plugin for ScriptEditorPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<ScriptEditorState>()
            .init_resource::<BrowserState>()
            .add_message::<OpenScriptEvent>()
            .add_message::<OpenBrowserEvent>()
            .add_systems(Update, handle_open_script_events)
            .add_systems(Update, handle_open_browser_events);
    }
}

// ============================================================================
// Browser Events
// ============================================================================

/// Event to open a URL in a browser tab
#[derive(Event, Clone, bevy::prelude::Message)]
pub struct OpenBrowserEvent {
    pub url: String,
    pub title: String,
}

impl OpenBrowserEvent {
    pub fn new(url: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            title: title.into(),
        }
    }
    
    /// Open documentation
    pub fn documentation() -> Self {
        Self::new("https://docs.eustress.dev", "Documentation")
    }
    
    /// Open API reference
    pub fn api_reference() -> Self {
        Self::new("https://docs.eustress.dev/api", "API Reference")
    }
    
    /// Open tutorials
    pub fn tutorials() -> Self {
        Self::new("https://docs.eustress.dev/tutorials", "Tutorials")
    }
}

/// System to handle opening browser tabs
fn handle_open_browser_events(
    mut events: MessageReader<OpenBrowserEvent>,
    mut script_state: ResMut<ScriptEditorState>,
    mut browser_state: ResMut<BrowserState>,
) {
    for event in events.read() {
        let tab_idx = script_state.open_browser(&event.url, &event.title);
        
        // Initialize browser tab state
        browser_state.tab_states.insert(tab_idx, BrowserTabState::new(&event.url));
        
        // Add to history
        browser_state.history.push(HistoryEntry {
            url: event.url.clone(),
            title: event.title.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            visit_count: 1,
        });
        
        info!("Opened browser tab: {} -> {}", event.title, event.url);
    }
}

// ============================================================================
// Browser UI Rendering
// ============================================================================

/// Render browser controls (URL bar, navigation buttons)
pub fn render_browser_controls(
    ui: &mut egui::Ui,
    tab_idx: usize,
    browser_state: &mut BrowserState,
) -> Option<String> {
    let mut navigate_to: Option<String> = None;
    
    let tab_state = browser_state.tab_states.entry(tab_idx).or_default();
    
    ui.horizontal(|ui| {
        // Back button
        ui.add_enabled_ui(tab_state.can_go_back, |ui| {
            if ui.button("<").on_hover_text("Go back").clicked() {
                tab_state.go_back();
                navigate_to = Some(tab_state.url.clone());
            }
        });
        
        // Forward button
        ui.add_enabled_ui(tab_state.can_go_forward, |ui| {
            if ui.button(">").on_hover_text("Go forward").clicked() {
                tab_state.go_forward();
                navigate_to = Some(tab_state.url.clone());
            }
        });
        
        // Refresh button
        if ui.button("R").on_hover_text("Refresh").clicked() {
            tab_state.refresh();
            navigate_to = Some(tab_state.url.clone());
        }
        
        // Home button
        if ui.button("H").on_hover_text("Home").clicked() {
            let home = if browser_state.home_page.is_empty() {
                "https://docs.eustress.dev".to_string()
            } else {
                browser_state.home_page.clone()
            };
            tab_state.navigate(&home);
            navigate_to = Some(home);
        }
        
        ui.add_space(8.0);
        
        // URL bar
        let url_response = ui.add(
            egui::TextEdit::singleline(&mut tab_state.url_bar_text)
                .desired_width(ui.available_width() - 100.0)
                .hint_text("Enter URL...")
        );
        
        if url_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let url = if tab_state.url_bar_text.starts_with("http://") || tab_state.url_bar_text.starts_with("https://") {
                tab_state.url_bar_text.clone()
            } else {
                format!("https://{}", tab_state.url_bar_text)
            };
            tab_state.navigate(&url);
            navigate_to = Some(url);
        }
        
        // Bookmark button
        if ui.button("*").on_hover_text("Bookmark this page").clicked() {
            browser_state.bookmarks.quick_access.push(Bookmark {
                title: tab_state.title.clone(),
                url: tab_state.url.clone(),
                favicon: None,
            });
        }
    });
    
    // Bookmarks bar (if enabled)
    if browser_state.show_bookmarks_bar && !browser_state.bookmarks.quick_access.is_empty() {
        ui.horizontal(|ui| {
            for bookmark in &browser_state.bookmarks.quick_access {
                if ui.small_button(&bookmark.title).on_hover_text(&bookmark.url).clicked() {
                    if let Some(state) = browser_state.tab_states.get_mut(&tab_idx) {
                        state.navigate(&bookmark.url);
                        navigate_to = Some(bookmark.url.clone());
                    }
                }
            }
        });
    }
    
    navigate_to
}

/// Render browser content placeholder (actual web rendering requires wry/webview integration)
pub fn render_browser_content(
    ui: &mut egui::Ui,
    tab_idx: usize,
    browser_state: &BrowserState,
) {
    let tab_state = browser_state.tab_states.get(&tab_idx);
    
    ui.vertical_centered(|ui| {
        ui.add_space(50.0);
        
        if let Some(state) = tab_state {
            if state.loading {
                ui.spinner();
                ui.label("Loading...");
            } else {
                ui.label(egui::RichText::new("Web Browser").size(24.0).strong());
                ui.add_space(20.0);
                ui.label(format!("URL: {}", state.url));
                ui.add_space(20.0);
                
                // Placeholder message
                ui.label(egui::RichText::new(
                    "Web content rendering requires native webview integration.\n\
                     For now, click the button below to open in your system browser."
                ).color(egui::Color32::GRAY));
                
                ui.add_space(20.0);
                
                if ui.button("Open in System Browser").clicked() {
                    let _ = open::that(&state.url);
                }
            }
        } else {
            ui.label("No browser state");
        }
    });
}
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           //! # Quantum Effects
//!
//! Quantum statistical mechanics for advanced simulations.
//!
//! ## Table of Contents
//!
//! 1. **Statistics** - Bose-Einstein, Fermi-Dirac distributions
//! 2. **Condensates** - Bose-Einstein condensate simulation
//! 3. **Tunneling** - Quantum tunneling effects
//!
//! ## Architecture
//!
//! Quantum effects are integrated via Symbolica for exact arithmetic:
//! - Partition functions with rational coefficients
//! - Bose-Einstein distribution for bosons (photons, phonons)
//! - Fermi-Dirac distribution for fermions (electrons)
//! - Quantum corrections to classical thermodynamics

pub mod statistics;
pub mod condensates;

pub mod prelude {
    pub use super::statistics::*;
    pub use super::condensates::*;
    pub use super::QuantumPlugin;
}

use bevy::prelude::*;

/// Quantum effects plugin
pub struct QuantumPlugin;

impl Plugin for QuantumPlugin {
    fn build(&self, app: &mut App) {
        app
            .register_type::<statistics::QuantumState>()
            .add_systems(Update, statistics::update_quantum_statistics);
        
        info!("QuantumPlugin initialized");
    }
}
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        x’[Õn#«µŒzÄyá
ò4eâ•»Nò— G3l) HN`qß’,Jçiv”›Õ+c≤…›ﬂU∂˜º…*?Ä˝yí|_ù™ÍÍ&%OyB…Æ™SÁˇØäóyy©F?›Ÿ›ˇ—ˆˆè’#ıºJ“fYiu¢”Î§»“˙·åòjÇ¡WjQïã‰*i≤≤ÿT37?≠≤FWY≤©íb™fIñJR$˘Mù’√  £GÍ"πÃµ*gjRç.öpì—Pml84Œõ§—jÀÓ›É¨∏Ri9_îñ
fª·"Ã\Ã¬U6õeÕı¶™õJ◊µ ∏eù57≤tèKÖ≤≥ñ2≥Ù,©≤ZÂ…–	zØñ˙·ÉáñµVó˙ıÕxº®tæúÍÒx„Ú∏÷øæ}¶Ò)KÚÏzSùªè_añù∑\ËäÎKº7ôÆ«„ %ƒAû9ò2WêèÒñd≈x|nhπ !ee¿no´É{|ë7≠21@XœÔy7Ó∑≠.(ﬁ∫U™ö‚7
ï HV®DÕ-´>xÙÈL{≠#è⁄¶z©gπNõM5…° õÍôæ\^Òmñ,s<ˆ‚‡3/¶¡Ô≠íµ-8>^,/©:À¥Ò~º}¯@·Eƒ? ÍÜ˙„:J∞ïa.óÔcıù>1™ˆt≥]˚À§VÕµˆdyÚßFÀs›Ë∫ ≠+´cgt”±∫,À<ÄuëÕç]π*™≥˘27∆™Z@nNÃÁcı´-˙…lo7DÓ0MóÄû™i2OÆ¥äv∂Fy:VX Ú\ÏE•7),=-ó¥TGÇ5¶ÿ÷cµÏ,=Iæ ŒsgÆ˙ÿC¶ãTO[Û‰öÃ9æÎÀÍ∆!m,õ/Ú€Ö5©4’™–o<´±µgá£Å⁄z
Ö…g  ö˚x<}ä,'∞-«‰?5‚p:Ö™¡w¿&”ilûFèÁÀF’ ∂)”∆‚„·^äˆÀ˙:2ü∞xÓ∂≤È›®¶lí\ ™\WÕug3,ƒ2=Ê.ÜT»∂›ÈQ£¡pû,¢/”/U:î≈ÉaΩúﬂ Ñ…µF¿»`≈çEIëj=≠Ò-k≤òÆ¡Ò:©Å°å;f9FŸß≤ ›`N;¯‘ÅCuıÙ¿caYq´0ΩX#Ë∞4Ò£±C(⁄©Œõƒ‚(ë¨5®X)ÂløæßÃ:+¢—p«Iõ+√©† B∫9…Ã›Ñ:P_⁄jßXÕYQ†ó:-´©3ΩYYπ¿ y;îWf∫µƒêj±ÕU≤âøD·ßÇÊ™!Ø•fÕ¥i-U L" q1Õ^g”•7â[#»˙h·£…Ìa#à2…@/pV"—Ä[el0¶'ºPQ^RÛ”ºŒ
8û:ßªƒDãΩ¿•>À¿Ó.∞ kÂ2uÛV LñUÖî…b"&§¢y∞y`V¡ŒB÷UUæiÆUEﬂÕ∑≈≥*Äë©1ß:øL≠#GrúÇi‹4´yíÍπ…‰8á‹¥£˝•«Ùd∞DŸ¸FqÄiiá÷€g>4U0A∂∂F(∏ò@êòP`@∑êlËI[!XÊ"õÑÄì‹:´Ò∑Dß§ì/‹≤Z™pø›∞(´πI(„≤äˇ†´2Ùñ9Ÿu1ÍMÍHfg∏”ˆú_
Y;KÚZKos'åG"2™Ω#∑√dƒn àD»îˆBvüÊÊ©˜¨t¬PÚ@X W|õÀÓmz¶.°?H÷êQŒ{ÈI·k:5„ñªÇÓ{tkSÔà∑ìÂ|≥’RxÊ™z™¿x;›Ö—‹cb`2˚sq¬«ÆRœaSeeÍ∞{ﬁM¸I9’Íÿ9_Ç!ºp_9á0«
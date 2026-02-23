//! # Center Tab Manager
//!
//! Manages the VS Code-style tabbed viewer in the center content area.
//! Supports multiple tab types: Scene, SoulScript, ParametersEditor,
//! Document, ImageViewer, VideoPlayer, and WebBrowser.
//!
//! ## Table of Contents
//! - CenterTabType: Enum discriminating tab content types
//! - CenterTabEntry: Data for a single tab instance
//! - CenterTabManager: Bevy Resource managing all open tabs
//! - Tab lifecycle: open, close, select, reorder, pin/unpin
//! - File extension routing: maps file extensions to tab types

use bevy::prelude::*;
use std::path::{Path, PathBuf};

// ============================================================================
// Tab Type Enum
// ============================================================================

/// Discriminator for center tab content type
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CenterTabType {
    /// 3D Scene viewport (always pinned, index 0)
    Scene,
    /// Soul Script editor (Monaco-backed for syntax highlighting)
    SoulScript,
    /// Entity parameters / data source editor
    ParametersEditor,
    /// Code file editor (Monaco-backed, any supported language)
    CodeEditor { language: String },
    /// Document viewer (PDF, DOCX, PPTX, XLSX)
    Document { doc_type: DocumentType },
    /// Image viewer (PNG, JPG, GIF, WebP, SVG)
    ImageViewer,
    /// Video player (MP4, WebM)
    VideoPlayer,
    /// Web browser tab (Wry WebView)
    WebBrowser,
}

/// Document sub-types for the Document tab
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DocumentType {
    Pdf,
    Docx,
    Pptx,
    Xlsx,
    Text,
    Markdown,
}

impl CenterTabType {
    /// Slint-compatible string identifier for tab type routing
    pub fn type_string(&self) -> &'static str {
        match self {
            CenterTabType::Scene => "scene",
            CenterTabType::SoulScript => "script",
            CenterTabType::ParametersEditor => "parameters",
            CenterTabType::CodeEditor { .. } => "code",
            CenterTabType::Document { .. } => "document",
            CenterTabType::ImageViewer => "image",
            CenterTabType::VideoPlayer => "video",
            CenterTabType::WebBrowser => "web",
        }
    }

    /// Icon name for this tab type (maps to assets/icons/ui/*.svg)
    pub fn icon_name(&self) -> &'static str {
        match self {
            CenterTabType::Scene => "viewport",
            CenterTabType::SoulScript => "script",
            CenterTabType::ParametersEditor => "settings",
            CenterTabType::CodeEditor { .. } => "code",
            CenterTabType::Document { doc_type } => match doc_type {
                DocumentType::Pdf => "pdf",
                DocumentType::Docx => "word",
                DocumentType::Pptx => "powerpoint",
                DocumentType::Xlsx => "excel",
                DocumentType::Text => "text",
                DocumentType::Markdown => "markdown",
            },
            CenterTabType::ImageViewer => "image",
            CenterTabType::VideoPlayer => "video",
            CenterTabType::WebBrowser => "globe",
        }
    }
}

// ============================================================================
// Tab Entry
// ============================================================================

/// Data for a single center tab instance
#[derive(Debug, Clone)]
pub struct CenterTabEntry {
    /// Unique tab identifier (auto-incremented)
    pub id: u32,
    /// Display name shown in tab bar
    pub name: String,
    /// Tab content type
    pub tab_type: CenterTabType,
    /// Associated entity (for SoulScript, ParametersEditor tabs)
    pub entity: Option<Entity>,
    /// Associated file path (for code, document, image, video tabs)
    pub file_path: Option<PathBuf>,
    /// URL (for web browser tabs)
    pub url: Option<String>,
    /// Whether tab is pinned (Scene tab is always pinned)
    pub pinned: bool,
    /// Whether content has unsaved changes
    pub dirty: bool,
    /// Whether content is loading (web tabs)
    pub loading: bool,
    /// Edit buffer content (for script/code tabs before Monaco is ready)
    pub content: String,
}

// ============================================================================
// Center Tab Manager Resource
// ============================================================================

/// Bevy Resource managing all open center tabs
#[derive(Resource)]
pub struct CenterTabManager {
    /// All open tabs (Scene tab is always first)
    pub tabs: Vec<CenterTabEntry>,
    /// Index of the currently active tab
    pub active_tab: usize,
    /// Next auto-increment ID for new tabs
    next_id: u32,
    /// Whether the tab state has changed and needs Slint sync
    pub dirty: bool,
}

impl Default for CenterTabManager {
    fn default() -> Self {
        // Scene tab is always present and pinned
        let scene_tab = CenterTabEntry {
            id: 0,
            name: "Space1".to_string(),
            tab_type: CenterTabType::Scene,
            entity: None,
            file_path: None,
            url: None,
            pinned: true,
            dirty: false,
            loading: false,
            content: String::new(),
        };
        Self {
            tabs: vec![scene_tab],
            active_tab: 0,
            next_id: 1,
            dirty: true,
        }
    }
}

impl CenterTabManager {
    // ====================================================================
    // Tab Lifecycle
    // ====================================================================

    /// Open a Soul Script tab (or focus existing)
    pub fn open_soul_script(&mut self, entity: Entity, name: &str, source: &str) -> usize {
        if let Some(idx) = self.find_tab_by_entity(entity, &CenterTabType::SoulScript) {
            self.active_tab = idx;
            self.dirty = true;
            return idx;
        }
        let id = self.next_id();
        self.push_tab(CenterTabEntry {
            id,
            name: name.to_string(),
            tab_type: CenterTabType::SoulScript,
            entity: Some(entity),
            file_path: None,
            url: None,
            pinned: false,
            dirty: false,
            loading: false,
            content: source.to_string(),
        })
    }

    /// Open a parameters editor tab (or focus existing)
    pub fn open_parameters_editor(&mut self, entity: Entity, name: &str) -> usize {
        if let Some(idx) = self.find_tab_by_entity(entity, &CenterTabType::ParametersEditor) {
            self.active_tab = idx;
            self.dirty = true;
            return idx;
        }
        let id = self.next_id();
        self.push_tab(CenterTabEntry {
            id,
            name: format!("{} - Parameters", name),
            tab_type: CenterTabType::ParametersEditor,
            entity: Some(entity),
            file_path: None,
            url: None,
            pinned: false,
            dirty: false,
            loading: false,
            content: String::new(),
        })
    }

    /// Open a file in the appropriate tab type based on extension
    pub fn open_file(&mut self, path: &Path) -> usize {
        // Check if already open
        if let Some(idx) = self.find_tab_by_path(path) {
            self.active_tab = idx;
            self.dirty = true;
            return idx;
        }

        let file_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();
        let tab_type = route_file_to_tab_type(path);

        // Read file content for code/text tabs
        let content = match &tab_type {
            CenterTabType::CodeEditor { .. } | CenterTabType::SoulScript => {
                std::fs::read_to_string(path).unwrap_or_default()
            }
            CenterTabType::Document { doc_type: DocumentType::Text | DocumentType::Markdown } => {
                std::fs::read_to_string(path).unwrap_or_default()
            }
            _ => String::new(),
        };

        let id = self.next_id();
        self.push_tab(CenterTabEntry {
            id,
            name: file_name,
            tab_type,
            entity: None,
            file_path: Some(path.to_path_buf()),
            url: None,
            pinned: false,
            dirty: false,
            loading: false,
            content,
        })
    }

    /// Open a web browser tab
    pub fn open_web_tab(&mut self, url: &str, title: &str) -> usize {
        let id = self.next_id();
        self.push_tab(CenterTabEntry {
            id,
            name: title.to_string(),
            tab_type: CenterTabType::WebBrowser,
            entity: None,
            file_path: None,
            url: Some(url.to_string()),
            pinned: false,
            dirty: false,
            loading: url != "about:blank",
            content: String::new(),
        })
    }

    /// Close a tab by index (cannot close Scene tab at index 0)
    pub fn close_tab(&mut self, index: usize) {
        if index == 0 || index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if self.active_tab > index {
            self.active_tab -= 1;
        }
        self.dirty = true;
    }

    /// Close all tabs except the given index (and Scene tab)
    pub fn close_others(&mut self, keep_index: usize) {
        for i in (1..self.tabs.len()).rev() {
            if i != keep_index && !self.tabs[i].pinned {
                self.tabs.remove(i);
            }
        }
        // Recalculate active tab
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        self.dirty = true;
    }

    /// Close all closable (non-pinned) tabs
    pub fn close_all_unpinned(&mut self) {
        self.tabs.retain(|t| t.pinned);
        self.active_tab = 0;
        self.dirty = true;
    }

    /// Select a tab by index
    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
            self.dirty = true;
        }
    }

    /// Reorder a tab from one index to another
    pub fn reorder_tab(&mut self, from: usize, to: usize) {
        if from == 0 || to == 0 { return; } // Cannot move Scene tab
        if from >= self.tabs.len() || to >= self.tabs.len() { return; }
        if from == to { return; }

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
        self.dirty = true;
    }

    /// Mark a tab as dirty (unsaved changes)
    pub fn mark_dirty(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.dirty = true;
            self.dirty = true;
        }
    }

    /// Mark a tab as clean (saved)
    pub fn mark_clean(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.dirty = false;
            self.dirty = true;
        }
    }

    /// Get the active tab
    pub fn active(&self) -> Option<&CenterTabEntry> {
        self.tabs.get(self.active_tab)
    }

    /// Get the active tab type string for Slint routing
    pub fn active_tab_type_string(&self) -> &'static str {
        self.active().map(|t| t.tab_type.type_string()).unwrap_or("scene")
    }

    /// Check if the 3D viewport (Scene tab) is active
    pub fn is_scene_active(&self) -> bool {
        self.active_tab == 0
    }

    // ====================================================================
    // Internal Helpers
    // ====================================================================

    /// Find a tab by entity and type
    fn find_tab_by_entity(&self, entity: Entity, tab_type: &CenterTabType) -> Option<usize> {
        self.tabs.iter().position(|t| {
            t.entity == Some(entity) && std::mem::discriminant(&t.tab_type) == std::mem::discriminant(tab_type)
        })
    }

    /// Find a tab by file path
    fn find_tab_by_path(&self, path: &Path) -> Option<usize> {
        self.tabs.iter().position(|t| {
            t.file_path.as_deref() == Some(path)
        })
    }

    /// Push a new tab and make it active
    fn push_tab(&mut self, tab: CenterTabEntry) -> usize {
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.dirty = true;
        self.active_tab
    }

    /// Get next unique ID
    fn next_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

// ============================================================================
// File Extension Routing
// ============================================================================

/// Route a file path to the appropriate tab type based on extension
pub fn route_file_to_tab_type(path: &Path) -> CenterTabType {
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        // Soul scripts
        "soul" => CenterTabType::SoulScript,

        // Code files (Monaco editor)
        "rs" => CenterTabType::CodeEditor { language: "rust".into() },
        "lua" => CenterTabType::CodeEditor { language: "lua".into() },
        "ts" | "tsx" => CenterTabType::CodeEditor { language: "typescript".into() },
        "js" | "jsx" => CenterTabType::CodeEditor { language: "javascript".into() },
        "py" => CenterTabType::CodeEditor { language: "python".into() },
        "go" => CenterTabType::CodeEditor { language: "go".into() },
        "c" | "h" => CenterTabType::CodeEditor { language: "c".into() },
        "cpp" | "cc" | "cxx" | "hpp" => CenterTabType::CodeEditor { language: "cpp".into() },
        "cs" => CenterTabType::CodeEditor { language: "csharp".into() },
        "java" => CenterTabType::CodeEditor { language: "java".into() },
        "json" => CenterTabType::CodeEditor { language: "json".into() },
        "toml" => CenterTabType::CodeEditor { language: "toml".into() },
        "yaml" | "yml" => CenterTabType::CodeEditor { language: "yaml".into() },
        "xml" => CenterTabType::CodeEditor { language: "xml".into() },
        "html" | "htm" => CenterTabType::CodeEditor { language: "html".into() },
        "css" => CenterTabType::CodeEditor { language: "css".into() },
        "scss" | "sass" => CenterTabType::CodeEditor { language: "scss".into() },
        "wgsl" => CenterTabType::CodeEditor { language: "wgsl".into() },
        "glsl" | "vert" | "frag" => CenterTabType::CodeEditor { language: "glsl".into() },
        "hlsl" => CenterTabType::CodeEditor { language: "hlsl".into() },
        "ron" => CenterTabType::CodeEditor { language: "ron".into() },
        "sh" | "bash" | "zsh" => CenterTabType::CodeEditor { language: "shell".into() },
        "ps1" | "psm1" => CenterTabType::CodeEditor { language: "powershell".into() },
        "sql" => CenterTabType::CodeEditor { language: "sql".into() },
        "dockerfile" => CenterTabType::CodeEditor { language: "dockerfile".into() },

        // Documents
        "pdf" => CenterTabType::Document { doc_type: DocumentType::Pdf },
        "docx" | "doc" => CenterTabType::Document { doc_type: DocumentType::Docx },
        "pptx" | "ppt" => CenterTabType::Document { doc_type: DocumentType::Pptx },
        "xlsx" | "xls" => CenterTabType::Document { doc_type: DocumentType::Xlsx },
        "md" | "markdown" => CenterTabType::Document { doc_type: DocumentType::Markdown },
        "txt" | "log" | "cfg" | "ini" | "env" => CenterTabType::Document { doc_type: DocumentType::Text },

        // Images
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" | "tiff" | "tga" => {
            CenterTabType::ImageViewer
        }

        // Video
        "mp4" | "webm" | "avi" | "mov" | "mkv" => CenterTabType::VideoPlayer,

        // Default: treat as text/code
        _ => CenterTabType::CodeEditor { language: "plaintext".into() },
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_file_to_tab_type() {
        assert!(matches!(
            route_file_to_tab_type(Path::new("main.rs")),
            CenterTabType::CodeEditor { language } if language == "rust"
        ));
        assert!(matches!(
            route_file_to_tab_type(Path::new("script.soul")),
            CenterTabType::SoulScript
        ));
        assert!(matches!(
            route_file_to_tab_type(Path::new("image.png")),
            CenterTabType::ImageViewer
        ));
        assert!(matches!(
            route_file_to_tab_type(Path::new("doc.pdf")),
            CenterTabType::Document { doc_type: DocumentType::Pdf }
        ));
        assert!(matches!(
            route_file_to_tab_type(Path::new("video.mp4")),
            CenterTabType::VideoPlayer
        ));
    }

    #[test]
    fn test_tab_manager_lifecycle() {
        let mut mgr = CenterTabManager::default();
        assert_eq!(mgr.tabs.len(), 1); // Scene tab
        assert!(mgr.is_scene_active());

        // Open a file
        let idx = mgr.open_file(Path::new("test.rs"));
        assert_eq!(idx, 1);
        assert_eq!(mgr.active_tab, 1);
        assert!(!mgr.is_scene_active());

        // Open web tab
        let idx = mgr.open_web_tab("https://eustress.dev", "Eustress");
        assert_eq!(idx, 2);
        assert_eq!(mgr.active_tab, 2);

        // Close tab
        mgr.close_tab(1);
        assert_eq!(mgr.tabs.len(), 2); // Scene + web
        assert_eq!(mgr.active_tab, 1); // Adjusted

        // Cannot close Scene tab
        mgr.close_tab(0);
        assert_eq!(mgr.tabs.len(), 2); // Still 2
    }
}

//! # Monaco Editor Bridge
//!
//! IPC bridge between Rust (Bevy) and Monaco Editor running inside a Wry WebView.
//! Handles bidirectional communication for code editing with syntax highlighting.
//!
//! ## Table of Contents
//! - MonacoIpcMessage: Enum for Monaco → Rust messages
//! - MonacoBridge: Bevy Resource managing Monaco WebView instances
//! - IPC Protocol: JSON-based postMessage communication
//! - Editor lifecycle: create, set content, save, close
//!
//! ## Architecture
//! ```text
//! ┌──────────────┐     IPC (JSON)      ┌──────────────┐
//! │  Bevy/Rust   │ ◄──────────────────► │  Wry WebView │
//! │              │                      │  (Monaco)    │
//! │ monaco_bridge│  save, cursor, dirty │              │
//! │   .rs        │ ────────────────────►│ editor.html  │
//! │              │  setContent(text,    │ + editor.js  │
//! │              │   language)          │              │
//! └──────────────┘                      └──────────────┘
//! ```

use bevy::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ============================================================================
// IPC Message Types
// ============================================================================

/// Messages sent from Monaco Editor → Rust via IPC postMessage
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type")]
pub enum MonacoIpcMessage {
    /// Editor is ready to receive content
    #[serde(rename = "ready")]
    Ready { tab_id: u32 },
    /// Content was saved (Ctrl+S)
    #[serde(rename = "save")]
    Save { tab_id: u32, content: String },
    /// Cursor position changed
    #[serde(rename = "cursor")]
    Cursor { tab_id: u32, line: u32, column: u32 },
    /// Content was modified (dirty state changed)
    #[serde(rename = "dirty")]
    Dirty { tab_id: u32, dirty: bool },
    /// Content changed (full text for sync)
    #[serde(rename = "content_changed")]
    ContentChanged { tab_id: u32, content: String },
}

/// Messages sent from Rust → Monaco Editor via JavaScript injection
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum MonacoCommand {
    /// Set editor content and language
    #[serde(rename = "setContent")]
    SetContent {
        content: String,
        language: String,
        tab_id: u32,
    },
    /// Mark content as saved (clear dirty indicator)
    #[serde(rename = "markSaved")]
    MarkSaved { tab_id: u32 },
    /// Set read-only mode
    #[serde(rename = "setReadOnly")]
    SetReadOnly { read_only: bool },
    /// Set editor theme
    #[serde(rename = "setTheme")]
    SetTheme { theme: String },
    /// Focus the editor
    #[serde(rename = "focus")]
    Focus,
}

// ============================================================================
// Monaco Editor Instance
// ============================================================================

/// State for a single Monaco editor WebView instance
pub struct MonacoInstance {
    /// Tab ID this editor belongs to
    pub tab_id: u32,
    /// File path being edited (if any)
    pub file_path: Option<PathBuf>,
    /// Monaco language identifier
    pub language: String,
    /// Whether the editor WebView is ready
    pub ready: bool,
    /// Whether the WebView is currently visible
    pub visible: bool,
    /// Current cursor position
    pub cursor_line: u32,
    pub cursor_column: u32,
    /// Wry WebView handle (only with webview feature)
    #[cfg(feature = "webview")]
    pub webview: Option<wry::WebView>,
}

impl MonacoInstance {
    /// Send a command to the Monaco editor via JavaScript injection
    pub fn send_command(&self, cmd: &MonacoCommand) {
        let json = serde_json::to_string(cmd).unwrap_or_default();
        let script = format!("window.handleRustMessage({})", json);
        
        #[cfg(feature = "webview")]
        if let Some(ref webview) = self.webview {
            let _ = webview.evaluate_script(&script);
        }
        
        #[cfg(not(feature = "webview"))]
        {
            // Log command when webview feature is not enabled
            let _ = script;
        }
    }

    /// Set content and language in the editor
    pub fn set_content(&self, content: &str, language: &str) {
        self.send_command(&MonacoCommand::SetContent {
            content: content.to_string(),
            language: language.to_string(),
            tab_id: self.tab_id,
        });
    }

    /// Mark content as saved
    pub fn mark_saved(&self) {
        self.send_command(&MonacoCommand::MarkSaved { tab_id: self.tab_id });
    }
}

// ============================================================================
// Monaco Bridge Resource
// ============================================================================

/// Shared IPC message queue (Monaco → Rust direction)
pub type IpcMessageQueue = Arc<Mutex<Vec<MonacoIpcMessage>>>;

/// Bevy Resource managing all Monaco editor WebView instances
#[derive(Resource)]
pub struct MonacoBridge {
    /// Map of tab_id → Monaco editor instance
    pub editors: HashMap<u32, MonacoInstance>,
    /// Shared IPC message queue (populated by WebView IPC handler)
    pub ipc_queue: IpcMessageQueue,
    /// Path to bundled Monaco assets directory
    pub monaco_assets_path: PathBuf,
    /// Whether Monaco assets are available
    pub assets_available: bool,
    /// Current editor theme
    pub theme: String,
}

impl Default for MonacoBridge {
    fn default() -> Self {
        // Look for Monaco assets in the standard location
        let monaco_path = PathBuf::from("assets/monaco");
        let available = monaco_path.join("vs").exists();
        
        Self {
            editors: HashMap::new(),
            ipc_queue: Arc::new(Mutex::new(Vec::new())),
            monaco_assets_path: monaco_path,
            assets_available: available,
            theme: "vs-dark".to_string(),
        }
    }
}

impl MonacoBridge {
    /// Create a new Monaco editor instance for a tab
    pub fn create_editor(&mut self, tab_id: u32, language: &str, file_path: Option<PathBuf>) {
        let instance = MonacoInstance {
            tab_id,
            file_path,
            language: language.to_string(),
            ready: false,
            visible: false,
            cursor_line: 1,
            cursor_column: 1,
            #[cfg(feature = "webview")]
            webview: None,
        };
        self.editors.insert(tab_id, instance);
        
        #[cfg(feature = "webview")]
        {
            // TODO: Create actual Wry WebView with:
            // 1. Load editor.html from monaco_assets_path
            // 2. Set up IPC handler to push messages to ipc_queue
            // 3. Position WebView to match Slint content area
            info!("Created Monaco editor for tab {} (language: {})", tab_id, language);
        }
        #[cfg(not(feature = "webview"))]
        {
            info!("Monaco editor placeholder for tab {} (webview feature not enabled)", tab_id);
        }
    }

    /// Remove a Monaco editor instance
    pub fn remove_editor(&mut self, tab_id: u32) {
        self.editors.remove(&tab_id);
    }

    /// Show/hide editors based on active tab
    pub fn set_active_tab(&mut self, active_tab_id: Option<u32>) {
        for (id, editor) in self.editors.iter_mut() {
            let should_show = active_tab_id == Some(*id);
            if editor.visible != should_show {
                editor.visible = should_show;
                #[cfg(feature = "webview")]
                if let Some(ref webview) = editor.webview {
                    let _ = webview.set_visible(should_show);
                }
            }
        }
    }

    /// Drain IPC messages from Monaco editors
    pub fn drain_ipc_messages(&self) -> Vec<MonacoIpcMessage> {
        if let Ok(mut queue) = self.ipc_queue.lock() {
            queue.drain(..).collect()
        } else {
            Vec::new()
        }
    }

    /// Get the editor.html URL for loading Monaco
    pub fn editor_url(&self) -> String {
        let index_path = self.monaco_assets_path.join("editor.html");
        if index_path.exists() {
            format!("file:///{}", index_path.display().to_string().replace('\\', "/"))
        } else {
            // Fallback: inline minimal editor
            "about:blank".to_string()
        }
    }
}

// ============================================================================
// Bevy Plugin
// ============================================================================

/// Bevy plugin for Monaco Editor integration
pub struct MonacoBridgePlugin;

impl Plugin for MonacoBridgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MonacoBridge>()
            .add_systems(Update, process_monaco_ipc_messages);
    }
}

/// System that drains Monaco IPC messages and applies them to tab state
fn process_monaco_ipc_messages(
    bridge: Res<MonacoBridge>,
    mut tab_manager: Option<ResMut<super::center_tabs::CenterTabManager>>,
    mut output: Option<ResMut<super::OutputConsole>>,
) {
    let messages = bridge.drain_ipc_messages();
    if messages.is_empty() { return; }
    
    for msg in messages {
        match msg {
            MonacoIpcMessage::Ready { tab_id } => {
                info!("Monaco editor ready for tab {}", tab_id);
            }
            MonacoIpcMessage::Save { tab_id, content } => {
                // Find the tab and save content to file
                if let Some(ref mut mgr) = tab_manager {
                    if let Some(tab) = mgr.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.dirty = false;
                        tab.content = content.clone();
                        
                        // Write to file if file-backed
                        if let Some(ref path) = tab.file_path {
                            match std::fs::write(path, &content) {
                                Ok(_) => {
                                    if let Some(ref mut out) = output {
                                        out.info(format!("Saved: {}", path.display()));
                                    }
                                }
                                Err(e) => {
                                    if let Some(ref mut out) = output {
                                        out.error(format!("Save failed: {}", e));
                                    }
                                }
                            }
                        }
                        mgr.dirty = true;
                    }
                }
            }
            MonacoIpcMessage::Cursor { tab_id, line, column } => {
                // Update cursor position in bridge state
                if let Some(editor) = bridge.editors.get(&tab_id) {
                    // Note: bridge is immutable here; cursor tracking is informational
                    let _ = (line, column);
                }
            }
            MonacoIpcMessage::Dirty { tab_id, dirty } => {
                if let Some(ref mut mgr) = tab_manager {
                    if let Some(tab) = mgr.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.dirty = dirty;
                        mgr.dirty = true;
                    }
                }
            }
            MonacoIpcMessage::ContentChanged { tab_id, content } => {
                if let Some(ref mut mgr) = tab_manager {
                    if let Some(tab) = mgr.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.content = content;
                    }
                }
            }
        }
    }
}

// ============================================================================
// Language Mapping
// ============================================================================

/// Map file extension to Monaco language identifier
pub fn extension_to_monaco_language(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "rs" => "rust",
        "lua" => "lua",
        "soul" => "soul",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "cs" => "csharp",
        "java" => "java",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "xml" => "xml",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "wgsl" => "wgsl",
        "glsl" | "vert" | "frag" => "glsl",
        "hlsl" => "hlsl",
        "ron" => "rust", // RON uses Rust-like syntax
        "md" | "markdown" => "markdown",
        "sh" | "bash" | "zsh" => "shell",
        "ps1" | "psm1" => "powershell",
        "sql" => "sql",
        "dockerfile" => "dockerfile",
        "txt" | "log" | "cfg" | "ini" | "env" => "plaintext",
        _ => "plaintext",
    }
}

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

/// Display mode for a SoulScript-style tab
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SoulScriptMode {
    /// Rendered markdown summary of a Soul Script — has `_instance.toml`
    /// sibling with `class_name = "Script" / "SoulScript"`. Shows the
    /// Summary / Code toggle + Build button.
    Summary,
    /// Raw code editor view (default for .rune / .soul).
    Code,
    /// Plain markdown file that is NOT a Soul Script (README, PATENT,
    /// SOTA docs, etc.). Renders the same editor surface as `Summary`
    /// but the script-specific controls (Code/Summary toggle, Build)
    /// are hidden — those only make sense for a real Soul Script where
    /// code is compiled from the summary.
    Markdown,
}

impl SoulScriptMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SoulScriptMode::Summary => "summary",
            SoulScriptMode::Code => "code",
            SoulScriptMode::Markdown => "markdown",
        }
    }

    pub fn toggled(&self) -> Self {
        match self {
            SoulScriptMode::Summary => SoulScriptMode::Code,
            SoulScriptMode::Code => SoulScriptMode::Summary,
            // Plain markdown has no opposite — the toggle is hidden in
            // the UI, but if the callback ever fires we no-op (stay in
            // Markdown) rather than flip to a broken Code view.
            SoulScriptMode::Markdown => SoulScriptMode::Markdown,
        }
    }
}

/// Discriminator for center tab content type
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CenterTabType {
    /// 3D Scene viewport (always pinned, index 0)
    Scene,
    /// Soul Script editor/preview — .soul and .rune open in Code mode, .md opens in Summary mode
    SoulScript { mode: SoulScriptMode },
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
    /// API Reference browser
    ApiBrowser,
    /// Services Browser (catalog of all default Space services)
    ServicesBrowser,
    /// Data Platform chart viewer (entity-keyed; opens from the Explorer like a
    /// script and closes the same way).
    DataChart,
    /// RFQ Builder — compose a request for quotation against a manufacturer.
    /// Singleton, like the API and Services browsers, because it edits the
    /// Space-wide order registry rather than one entity.
    RfqBuilder,
    /// Purchase Order Tracker — every order in the Space with its state and the
    /// transitions currently legal for it. Singleton for the same reason.
    PurchaseOrderTracker,
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
    /// Returns the mode string for SoulScript tabs ("summary" or "code"), empty for others
    pub fn mode_string(&self) -> &'static str {
        match self {
            CenterTabType::SoulScript { mode } => mode.as_str(),
            _ => "",
        }
    }

    pub fn type_string(&self) -> &'static str {
        match self {
            CenterTabType::Scene => "scene",
            CenterTabType::SoulScript { .. } => "script",
            CenterTabType::ParametersEditor => "parameters",
            CenterTabType::CodeEditor { .. } => "code",
            CenterTabType::Document { .. } => "document",
            CenterTabType::ImageViewer => "image",
            CenterTabType::VideoPlayer => "video",
            CenterTabType::WebBrowser => "web",
            CenterTabType::ApiBrowser => "api",
            CenterTabType::ServicesBrowser => "services",
            CenterTabType::DataChart => "chart",
            CenterTabType::RfqBuilder => "rfq",
            CenterTabType::PurchaseOrderTracker => "purchase-orders",
        }
    }

    /// Icon name for this tab type (maps to assets/icons/ui/*.svg)
    pub fn icon_name(&self) -> &'static str {
        match self {
            CenterTabType::Scene => "viewport",
            CenterTabType::SoulScript { .. } => "script",
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
            CenterTabType::ApiBrowser => "code",
            CenterTabType::ServicesBrowser => "package",
            CenterTabType::DataChart => "viewport",
            CenterTabType::RfqBuilder => "handshake",
            CenterTabType::PurchaseOrderTracker => "clipboard",
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
    /// Edit buffer content — Code view for scripts, raw text for others
    pub content: String,
    /// Summary markdown content — Summary view for SoulScript tabs
    pub summary_content: String,
}

// ============================================================================
// Center Tab Manager Resource
// ============================================================================

/// Snapshot of a space's open tabs. Captured on `open_space` and restored
/// when the user navigates back to the same space.
#[derive(Debug, Clone, Default)]
pub struct CenterTabSpaceSnapshot {
    /// Non-Scene tabs (Scene is always rebuilt as index 0 by the manager).
    pub tabs: Vec<CenterTabEntry>,
    /// Active tab index in the original layout (clamped on restore).
    pub active_tab: usize,
    /// Recently closed tabs at time of snapshot.
    pub closed_tabs: Vec<CenterTabEntry>,
}

/// Orders `CenterTabManager::sort_tabs` offers from the tab context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabSortKey {
    /// Case-insensitive by tab name.
    Name,
    /// Grouped by kind (code, script, web, ...), then by name within a kind.
    Type,
}

/// A close waiting on the unsaved-changes prompt. Tabs are named by id, so
/// the request still means the same tabs if others open or move while the
/// prompt is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabCloseRequest {
    /// One tab.
    One(u32),
    /// Every unpinned tab except this one.
    Others(u32),
    /// Every unpinned tab after this one.
    ToRight(u32),
    /// Every unpinned tab.
    All,
    /// Studio is exiting, which closes every tab.
    AppExit,
}

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
    /// True when only the active tab changed (refocus), not the tab list itself
    pub focus_only: bool,
    /// Recently closed tabs (stack — most recent on top, max 20)
    pub closed_tabs: Vec<CenterTabEntry>,
    /// Per-space tab snapshots. Keyed by absolute space path. Restored on
    /// `open_space`. Tabs referencing entities are filtered to drop stale
    /// references (entity IDs are not stable across world reloads).
    pub space_snapshots: std::collections::HashMap<PathBuf, CenterTabSpaceSnapshot>,
    /// A close that found unsaved edits and waits on the prompt's answer.
    pub pending_close: Option<TabCloseRequest>,
}

impl Default for CenterTabManager {
    fn default() -> Self {
        // Scene tab is always present and pinned
        let scene_tab = CenterTabEntry {
            id: 0,
            name: "Space".to_string(),
            tab_type: CenterTabType::Scene,
            entity: None,
            file_path: None,
            url: None,
            pinned: true,
            dirty: false,
            loading: false,
            content: String::new(),
            summary_content: String::new(),
        };
        Self {
            tabs: vec![scene_tab],
            active_tab: 0,
            next_id: 1,
            dirty: true,
            focus_only: false,
            closed_tabs: Vec::new(),
            space_snapshots: std::collections::HashMap::new(),
            pending_close: None,
        }
    }
}

impl CenterTabManager {
    // ====================================================================
    // Per-Space Snapshots
    // ====================================================================

    /// Capture the current non-Scene tabs into a snapshot keyed by `space_root`.
    /// Called from `open_space` BEFORE the world is cleared. The Scene tab
    /// (index 0) is always pinned and rebuilt by `Default::default()`, so we
    /// skip it here.
    pub fn snapshot_for_space(&mut self, space_root: &Path) {
        let non_scene_tabs: Vec<CenterTabEntry> = self.tabs.iter()
            .filter(|t| !matches!(t.tab_type, CenterTabType::Scene))
            .cloned()
            .collect();
        // Active tab index in original space — translate to the same space
        // when restored. If the Scene tab was active (index 0), the snapshot
        // records 0 and the restore keeps Scene active.
        let snapshot = CenterTabSpaceSnapshot {
            tabs: non_scene_tabs,
            active_tab: self.active_tab,
            closed_tabs: self.closed_tabs.clone(),
        };
        self.space_snapshots.insert(space_root.to_path_buf(), snapshot);
    }

    /// Restore tabs from a previous snapshot of `space_root`. If no snapshot
    /// exists (first visit), leaves the Default-only Scene tab in place.
    /// Tabs whose `entity` reference would be stale after the world reload
    /// are dropped — file-based tabs (CodeEditor, Document, ImageViewer,
    /// VideoPlayer, WebBrowser, ApiBrowser, ServicesBrowser) survive.
    /// Entity-based tabs (SoulScript, ParametersEditor) survive only when
    /// they also carry a `file_path`; otherwise they're filtered.
    pub fn restore_for_space(&mut self, space_root: &Path) {
        // Always start with a fresh Scene tab in slot 0, but preserve the
        // snapshots map across the reset so other spaces' state survives.
        let preserved_snapshots = std::mem::take(&mut self.space_snapshots);
        *self = Self::default();
        self.space_snapshots = preserved_snapshots;
        let Some(snapshot) = self.space_snapshots_take(space_root) else {
            return;
        };
        for tab in snapshot.tabs.into_iter() {
            let keep = match tab.tab_type {
                CenterTabType::Scene => false, // Scene is always slot 0; never duplicate.
                CenterTabType::SoulScript { .. }
                | CenterTabType::ParametersEditor => tab.file_path.is_some(),
                _ => true,
            };
            if !keep { continue; }
            // Drop the stale entity reference; any consumer that needs an
            // Entity must re-resolve it from `file_path` on first paint.
            let mut restored = tab;
            restored.entity = None;
            // Re-stamp the id from this manager's counter so it doesn't
            // collide with the rebuilt Scene tab (id=0).
            restored.id = self.next_id();
            self.tabs.push(restored);
        }
        // Restore active-tab clamped to current tab count. Snapshot's
        // active_tab was an index into [Scene, ...] in the previous space;
        // since we rebuilt with Scene at 0 plus the same non-Scene tabs in
        // order, the index stays valid as long as nothing was filtered.
        let target = snapshot.active_tab.min(self.tabs.len().saturating_sub(1));
        self.active_tab = target;
        self.closed_tabs = snapshot.closed_tabs;
        self.dirty = true;
        self.focus_only = false;
    }

    /// Helper — pull a snapshot out of the map without holding a borrow on
    /// `self.space_snapshots` while we mutate `self.tabs`.
    fn space_snapshots_take(&mut self, space_root: &Path) -> Option<CenterTabSpaceSnapshot> {
        self.space_snapshots.remove(space_root)
    }

    // ====================================================================
    // Tab Lifecycle
    // ====================================================================

    /// Open a Soul Script tab (or focus existing)
    pub fn open_soul_script(&mut self, entity: Entity, name: &str, source: &str) -> usize {
        let soul_type = CenterTabType::SoulScript { mode: SoulScriptMode::Code };
        if let Some(idx) = self.find_tab_by_entity(entity, &soul_type) {
            self.active_tab = idx;
            self.focus_only = true;
            self.dirty = true;
            return idx;
        }
        let id = self.next_id();
        self.push_tab(CenterTabEntry {
            id,
            name: name.to_string(),
            tab_type: soul_type,
            entity: Some(entity),
            file_path: None,
            url: None,
            pinned: false,
            dirty: false,
            loading: false,
            content: source.to_string(),
            summary_content: String::new(),
        })
    }

    /// Toggle the Summary/Code mode for a tab by index
    pub fn toggle_mode(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get_mut(index) {
            if let CenterTabType::SoulScript { ref mut mode } = tab.tab_type {
                *mode = mode.toggled();
                self.dirty = true;
            }
        }
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
            summary_content: String::new(),
        })
    }

    /// Open a Data Platform chart tab for an entity (or focus existing). Opens
    /// from the Explorer like a script and closes the same way.
    pub fn open_data_chart(&mut self, entity: Entity, name: &str) -> usize {
        if let Some(idx) = self.find_tab_by_entity(entity, &CenterTabType::DataChart) {
            self.active_tab = idx;
            self.dirty = true;
            return idx;
        }
        let id = self.next_id();
        self.push_tab(CenterTabEntry {
            id,
            name: format!("{} - Chart", name),
            tab_type: CenterTabType::DataChart,
            entity: Some(entity),
            file_path: None,
            url: None,
            pinned: false,
            dirty: false,
            loading: false,
            content: String::new(),
            summary_content: String::new(),
        })
    }

    /// Open a file in the appropriate tab type based on extension
    pub fn open_file(&mut self, path: &Path) -> usize {
        // Check if already open — just focus it, don't rebuild model
        if let Some(idx) = self.find_tab_by_path(path) {
            self.active_tab = idx;
            self.focus_only = true; // Signal: only update active index, don't re-push model
            self.dirty = true;
            return idx;
        }

        let file_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();
        let tab_type = route_file_to_tab_type(path);

        // Read file content for code/text tabs
        // For folder-based scripts, resolve both code source and Summary.md
        let content = match &tab_type {
            CenterTabType::CodeEditor { .. } | CenterTabType::SoulScript { .. } => {
                resolve_script_source(path).unwrap_or_default()
            }
            CenterTabType::Document { doc_type: DocumentType::Text | DocumentType::Markdown } => {
                std::fs::read_to_string(path).unwrap_or_default()
            }
            _ => String::new(),
        };

        // For SoulScript tabs, also load Summary.md if it exists
        let summary_content = match &tab_type {
            CenterTabType::SoulScript { .. } => {
                resolve_script_summary(path).unwrap_or_default()
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
            summary_content,
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
            summary_content: String::new(),
        })
    }

    /// Close a tab by index (cannot close Scene tab at index 0)
    /// Open or focus the API Reference browser tab.
    pub fn open_api_browser(&mut self) -> usize {
        // Check if already open
        if let Some(idx) = self.tabs.iter().position(|t| t.tab_type == CenterTabType::ApiBrowser) {
            self.active_tab = idx;
            self.focus_only = true;
            self.dirty = true;
            return idx;
        }
        let id = self.next_id();
        self.push_tab(CenterTabEntry {
            id,
            name: "API Reference".to_string(),
            tab_type: CenterTabType::ApiBrowser,
            entity: None,
            file_path: None,
            url: None,
            pinned: false,
            dirty: false,
            loading: false,
            content: String::new(),
            summary_content: String::new(),
        })
    }

    /// Open or focus the Services Browser tab.
    pub fn open_services_browser(&mut self) -> usize {
        if let Some(idx) = self.tabs.iter().position(|t| t.tab_type == CenterTabType::ServicesBrowser) {
            self.active_tab = idx;
            self.focus_only = true;
            self.dirty = true;
            return idx;
        }
        let id = self.next_id();
        self.push_tab(CenterTabEntry {
            id,
            name: "Services Browser".to_string(),
            tab_type: CenterTabType::ServicesBrowser,
            entity: None,
            file_path: None,
            url: None,
            pinned: false,
            dirty: false,
            loading: false,
            content: String::new(),
            summary_content: String::new(),
        })
    }

    /// Open or focus the RFQ Builder tab.
    ///
    /// Singleton by design. An RFQ is a `PurchaseOrder` in `draft`, so this
    /// panel edits the Space-wide order registry rather than one entity, and a
    /// second instance would only be a second view onto the same records.
    pub fn open_rfq_builder(&mut self) -> usize {
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.tab_type == CenterTabType::RfqBuilder)
        {
            self.active_tab = idx;
            self.focus_only = true;
            self.dirty = true;
            return idx;
        }
        let id = self.next_id();
        self.push_tab(CenterTabEntry {
            id,
            name: "RFQ Builder".to_string(),
            tab_type: CenterTabType::RfqBuilder,
            entity: None,
            file_path: None,
            url: None,
            pinned: false,
            dirty: false,
            loading: false,
            content: String::new(),
            summary_content: String::new(),
        })
    }

    /// Open or focus the Purchase Order Tracker tab.
    pub fn open_purchase_order_tracker(&mut self) -> usize {
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.tab_type == CenterTabType::PurchaseOrderTracker)
        {
            self.active_tab = idx;
            self.focus_only = true;
            self.dirty = true;
            return idx;
        }
        let id = self.next_id();
        self.push_tab(CenterTabEntry {
            id,
            name: "Purchase Orders".to_string(),
            tab_type: CenterTabType::PurchaseOrderTracker,
            entity: None,
            file_path: None,
            url: None,
            pinned: false,
            dirty: false,
            loading: false,
            content: String::new(),
            summary_content: String::new(),
        })
    }

    pub fn close_tab(&mut self, index: usize) {
        if index == 0 || index >= self.tabs.len() {
            return;
        }
        // Save to closed stack before removing (max 20)
        let closed = self.tabs.remove(index);
        self.closed_tabs.push(closed);
        if self.closed_tabs.len() > 20 {
            self.closed_tabs.remove(0);
        }
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if self.active_tab > index {
            self.active_tab -= 1;
        }
        self.dirty = true;
    }

    /// Reopen the most recently closed tab (Ctrl+Shift+T)
    pub fn reopen_last_closed(&mut self) -> bool {
        if let Some(mut tab) = self.closed_tabs.pop() {
            tab.id = self.next_id();
            self.tabs.push(tab);
            self.active_tab = self.tabs.len() - 1;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Close every unpinned tab `close(index)` picks. Closed tabs go on the
    /// reopen stack in their strip order, so repeated Ctrl+Shift+T brings them
    /// back right to left. The active tab stays active when it survives;
    /// otherwise the nearest survivor to its left takes over.
    fn close_where(&mut self, close: impl Fn(usize) -> bool) {
        let active_id = self.tabs.get(self.active_tab).map(|t| t.id);
        let mut kept = Vec::with_capacity(self.tabs.len());
        let mut nearest_left = 0;
        for (i, tab) in std::mem::take(&mut self.tabs).into_iter().enumerate() {
            if !tab.pinned && close(i) {
                self.closed_tabs.push(tab);
            } else {
                if i <= self.active_tab {
                    nearest_left = kept.len();
                }
                kept.push(tab);
            }
        }
        self.tabs = kept;
        let overflow = self.closed_tabs.len().saturating_sub(20);
        self.closed_tabs.drain(..overflow);
        self.active_tab = active_id
            .and_then(|id| self.tabs.iter().position(|t| t.id == id))
            .unwrap_or(nearest_left);
        self.dirty = true;
    }

    /// Close all tabs except the given index (and pinned tabs). The kept tab
    /// becomes active, since it is the one the user pointed at.
    pub fn close_others(&mut self, keep_index: usize) {
        if keep_index >= self.tabs.len() {
            return;
        }
        let keep_id = self.tabs[keep_index].id;
        self.close_where(|i| i != keep_index);
        self.active_tab = self.tabs.iter().position(|t| t.id == keep_id).unwrap_or(0);
    }

    /// Close every unpinned tab to the right of `index`.
    pub fn close_to_right(&mut self, index: usize) {
        self.close_where(|i| i > index);
    }

    /// Close all closable (non-pinned) tabs
    pub fn close_all_unpinned(&mut self) {
        self.close_where(|_| true);
    }

    /// Reorder the unpinned tabs, keeping pinned tabs where they are and the
    /// active tab active. The sort is stable, so equal keys keep the order
    /// the user gave them.
    pub fn sort_tabs(&mut self, by: TabSortKey) {
        let active_id = self.tabs.get(self.active_tab).map(|t| t.id);
        let first_unpinned = self.tabs.iter().position(|t| !t.pinned).unwrap_or(self.tabs.len());
        let tail = &mut self.tabs[first_unpinned..];
        match by {
            TabSortKey::Name => tail.sort_by_cached_key(|t| t.name.to_lowercase()),
            TabSortKey::Type => tail.sort_by_cached_key(|t| (t.tab_type.type_string(), t.name.to_lowercase())),
        }
        if let Some(pos) = active_id.and_then(|id| self.tabs.iter().position(|t| t.id == id)) {
            self.active_tab = pos;
        }
        self.dirty = true;
    }

    /// Select a tab by index
    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
            self.dirty = true;
        }
    }

    /// Move tab `from` into the gap before `slot`, where slot `k` is the gap
    /// in front of tab `k` and `tabs.len()` is the gap after the last tab.
    /// This is what a drag drop means: the caret the user saw. Pinned tabs
    /// never move and nothing lands among them; the active tab stays active.
    pub fn move_tab_to_slot(&mut self, from: usize, slot: usize) {
        if from >= self.tabs.len() || self.tabs[from].pinned {
            return;
        }
        let first_unpinned = self.tabs.iter().position(|t| !t.pinned).unwrap_or(self.tabs.len());
        let slot = slot.clamp(first_unpinned, self.tabs.len());
        // The gaps on either side of the tab itself put it back where it was.
        if slot == from || slot == from + 1 {
            return;
        }
        let active_id = self.tabs.get(self.active_tab).map(|t| t.id);
        let tab = self.tabs.remove(from);
        let to = if slot > from { slot - 1 } else { slot };
        self.tabs.insert(to, tab);
        if let Some(pos) = active_id.and_then(|id| self.tabs.iter().position(|t| t.id == id)) {
            self.active_tab = pos;
        }
        self.dirty = true;
    }

    /// Whether the tab at `index` is one `req` closes. The Scene tab never
    /// closes.
    pub fn close_covers(&self, req: TabCloseRequest, index: usize) -> bool {
        let Some(tab) = self.tabs.get(index) else { return false };
        if index == 0 {
            return false;
        }
        let at = |id: u32| self.tabs.iter().position(|t| t.id == id);
        match req {
            TabCloseRequest::One(id) => tab.id == id,
            TabCloseRequest::Others(id) => tab.id != id && !tab.pinned,
            TabCloseRequest::ToRight(id) => !tab.pinned && at(id).is_some_and(|p| index > p),
            TabCloseRequest::All => !tab.pinned,
            TabCloseRequest::AppExit => true,
        }
    }

    /// Indices of the tabs with unsaved edits that `req` would close.
    pub fn dirty_tabs_closed_by(&self, req: TabCloseRequest) -> Vec<usize> {
        (0..self.tabs.len())
            .filter(|&i| self.tabs[i].dirty && self.close_covers(req, i))
            .collect()
    }

    /// Close the tabs `req` names. Exiting closes nothing here; the app
    /// exit does that.
    pub fn apply_close(&mut self, req: TabCloseRequest) {
        let at = |mgr: &Self, id: u32| mgr.tabs.iter().position(|t| t.id == id);
        match req {
            TabCloseRequest::One(id) => {
                if let Some(i) = at(self, id) {
                    self.close_tab(i);
                }
            }
            TabCloseRequest::Others(id) => {
                if let Some(i) = at(self, id) {
                    self.close_others(i);
                }
            }
            TabCloseRequest::ToRight(id) => {
                if let Some(i) = at(self, id) {
                    self.close_to_right(i);
                }
            }
            TabCloseRequest::All => self.close_all_unpinned(),
            TabCloseRequest::AppExit => {}
        }
    }

    /// Close now when nothing `req` covers has unsaved edits; otherwise hold
    /// the request for the unsaved-changes prompt. Returns true when the
    /// close ran.
    pub fn request_close(&mut self, req: TabCloseRequest) -> bool {
        if self.dirty_tabs_closed_by(req).is_empty() {
            self.apply_close(req);
            true
        } else {
            self.pending_close = Some(req);
            false
        }
    }

    /// Mark a tab as having unsaved changes. Returns true when the flag
    /// changed, so the caller can refresh that one tab in the strip.
    ///
    /// The manager's own `dirty` stays untouched: it triggers a full rebuild
    /// of the tab strip and a re-push of the editor text a few frames later,
    /// which must never happen while someone is typing.
    pub fn mark_dirty(&mut self, index: usize) -> bool {
        match self.tabs.get_mut(index) {
            Some(tab) if !tab.dirty => {
                tab.dirty = true;
                true
            }
            _ => false,
        }
    }

    /// Mark a tab as saved. Returns true when the flag changed. Leaves the
    /// manager's rebuild flag alone, for the reason given on [`Self::mark_dirty`].
    pub fn mark_clean(&mut self, index: usize) -> bool {
        match self.tabs.get_mut(index) {
            Some(tab) if tab.dirty => {
                tab.dirty = false;
                true
            }
            _ => false,
        }
    }

    /// Get the active tab
    pub fn active(&self) -> Option<&CenterTabEntry> {
        self.tabs.get(self.active_tab)
    }

    pub fn active_mut(&mut self) -> Option<&mut CenterTabEntry> {
        self.tabs.get_mut(self.active_tab)
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

/// Resolve the source code content for a script path.
/// If path is a directory (folder-based script), finds the .rune/.luau/.soul file inside.
/// If path is a file, reads it directly.
fn resolve_script_source(path: &Path) -> Option<String> {
    if path.is_dir() {
        let source_path = script_source_path(path)?;
        std::fs::read_to_string(source_path).ok()
    } else {
        std::fs::read_to_string(path).ok()
    }
}

/// Resolve the summary (`<name>.md`) content for a script folder.
/// Returns None if no summary exists or path is not a directory.
fn resolve_script_summary(path: &Path) -> Option<String> {
    if path.is_dir() {
        let summary_path = script_summary_path(path);
        if summary_path.exists() {
            return std::fs::read_to_string(summary_path).ok();
        }
    }
    None
}

// ─── Canonical script-file path helpers ─────────────────────────────
//
// Scripts are folder-based. The canonical layout is
//   <folder>/<folder>.rune   ← source
//   <folder>/<folder>.md     ← summary
// so the files carry the same identity as the script itself — rename
// the folder and users immediately see the matching file rename.
//
// For backwards compatibility we fall back to legacy names
// (`Source.rune`, any other `.rune`/`.luau`/`.soul`/`.lua`, `Summary.md`)
// when the canonical file doesn't exist. New writes always use the
// canonical path.

/// Source-file path for a script folder. Returns `None` if neither the
/// canonical nor any legacy source file exists. Callers that are about
/// to *write* should use `script_source_path_canonical` instead.
pub fn script_source_path(folder: &Path) -> Option<std::path::PathBuf> {
    let canonical = script_source_path_canonical(folder);
    if canonical.exists() { return Some(canonical) }

    // Legacy: accept any script-ish file. This is what the old codebase
    // shipped and we don't want to break existing projects.
    std::fs::read_dir(folder).ok()
        .and_then(|entries| entries.flatten().find(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.ends_with(".rune") || n.ends_with(".luau")
                || n.ends_with(".soul") || n.ends_with(".lua")
        }))
        .map(|e| e.path())
}

/// Canonical source path — `<folder>/<folder_name>.rune`. Always
/// returns a path, even if the file doesn't exist yet. Use for writes.
pub fn script_source_path_canonical(folder: &Path) -> std::path::PathBuf {
    let name = folder.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Source");
    folder.join(format!("{}.rune", name))
}

/// Summary-file path for a script folder. Prefers `<folder>/<folder>.md`
/// and falls back to the legacy `Summary.md`. Always returns a path —
/// the caller checks `exists()` for reads. For writes, use
/// `script_summary_path_canonical` to always land on the new layout.
pub fn script_summary_path(folder: &Path) -> std::path::PathBuf {
    let canonical = script_summary_path_canonical(folder);
    if canonical.exists() { return canonical }
    let legacy = folder.join("Summary.md");
    if legacy.exists() { return legacy }
    // Neither exists — hand back the canonical so a subsequent
    // `read_to_string` fails cleanly (returning None in the caller).
    canonical
}

/// Canonical summary path — `<folder>/<folder_name>.md`. Used for writes
/// and new-script creation so names stay in lock-step with the folder.
pub fn script_summary_path_canonical(folder: &Path) -> std::path::PathBuf {
    let name = folder.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Summary");
    folder.join(format!("{}.md", name))
}

// ─── Saving a code tab ──────────────────────────────────────────────
//
// A tab writes back to the file it was read from. For a tab opened from the
// Explorer that is the entity's `LoadedFromFile.path`, which the loader sets
// to the exact source file (honouring an `_instance.toml` `[script] source`
// override). For a tab opened by path it is the same file
// `resolve_script_source` read. `script_source_path_canonical` names
// `<folder>.rune`, so it is used only to create a source file for a folder
// that has none: used unconditionally it would write a new `.rune` beside a
// `.client.luau` and leave the real script untouched.

/// The source file behind a tab's Code view. `entity_source` is the
/// `LoadedFromFile.path` of the tab's entity, when it has one.
pub fn code_source_file(tab: &CenterTabEntry, entity_source: Option<&Path>) -> Option<PathBuf> {
    fn in_folder(folder: &Path) -> PathBuf {
        script_source_path(folder).unwrap_or_else(|| script_source_path_canonical(folder))
    }
    if let Some(src) = entity_source {
        if src.is_dir() {
            return Some(in_folder(src));
        }
        if src.file_name().and_then(|n| n.to_str()) == Some("_instance.toml") {
            return src.parent().map(in_folder);
        }
        return Some(src.to_path_buf());
    }
    let path = tab.file_path.as_ref()?;
    Some(if path.is_dir() { in_folder(path) } else { path.clone() })
}

/// Where Save writes a tab's code buffer (`content`): its source file, for a
/// script tab in either view or a plain code tab. A script's Summary saves
/// itself as it is edited, so Save from the Summary view writes the code.
/// `None` for a Markdown document, which also saves as it is edited.
pub fn code_save_target(tab: &CenterTabEntry, entity_source: Option<&Path>) -> Option<PathBuf> {
    let is_code = matches!(
        tab.tab_type,
        CenterTabType::SoulScript { mode: SoulScriptMode::Code | SoulScriptMode::Summary }
            | CenterTabType::CodeEditor { .. }
    );
    if is_code { code_source_file(tab, entity_source) } else { None }
}

/// Write a script's text to `path`, keeping the line endings the file already
/// uses, then put the same bytes into the Space's Fjall `tree`.
///
/// The write is an ordinary edit as far as the file watcher is concerned:
/// no `RecentlyWrittenFiles` mark. A marked path is skipped entirely, which
/// would leave the running `SoulScriptData` on the old source and skip the
/// watcher's disk-to-tree mirror. The direct `put_tree_file` makes the save
/// durable even when the watcher drops the event (its start-up grace period
/// after a Space opens); it is a no-op when no database is active.
pub fn write_script_file(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let crlf = std::fs::read(path)
        .map(|b| b.windows(2).any(|w| w == b"\r\n"))
        .unwrap_or(false);
    let lf = text.replace("\r\n", "\n");
    let out = if crlf { lf.replace('\n', "\r\n") } else { lf };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(path)?;
    file.write_all(out.as_bytes())?;
    file.sync_all()?;
    crate::space::active_db::put_tree_file(path, out.as_bytes());
    Ok(())
}

/// Save one tab's Code view to its source file. `Ok(None)` when the tab has
/// no Code view to save.
pub fn save_code_tab(tab: &CenterTabEntry, entity_source: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let Some(target) = code_save_target(tab, entity_source) else {
        return Ok(None);
    };
    write_script_file(&target, &tab.content)
        .map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(Some(target))
}

/// Save every code tab with unsaved edits to its source file, and mark it
/// clean. First each tab is given the path of the file its entity was loaded
/// from: entity ids do not survive a Space reload, but paths do, so a tab
/// snapshot or a later save can still find its file. Returns one
/// "<tab>: <reason>" per save that failed; those tabs stay dirty.
///
/// Switching Space calls this before its tabs leave with the outgoing Space,
/// and a snapshot revert calls it before its safety snapshot, so the edits are
/// in that snapshot rather than overwritten by the restore.
pub fn save_dirty_code_tabs(world: &mut World) -> Vec<String> {
    let entity_paths: std::collections::HashMap<Entity, PathBuf> = {
        let mut q = world.query::<(Entity, &crate::space::file_loader::LoadedFromFile)>();
        q.iter(world).map(|(e, lff)| (e, lff.path.clone())).collect()
    };
    let Some(mut tab_mgr) = world.get_resource_mut::<CenterTabManager>() else {
        return Vec::new();
    };
    for tab in tab_mgr.tabs.iter_mut() {
        if tab.file_path.is_none() {
            if let Some(path) = tab.entity.and_then(|e| entity_paths.get(&e)) {
                tab.file_path = Some(path.clone());
            }
        }
    }
    let mut failed = Vec::new();
    for tab in tab_mgr.tabs.iter_mut().filter(|t| t.dirty) {
        match save_code_tab(tab, None) {
            Ok(Some(path)) => {
                tab.dirty = false;
                info!("Saved {:?}", path);
            }
            Ok(None) => {}
            Err(e) => {
                warn!("Could not save {}: {e}", tab.name);
                failed.push(format!("{}: {e}", tab.name));
            }
        }
    }
    failed
}

/// Route a file path to the appropriate tab type based on extension
pub fn route_file_to_tab_type(path: &Path) -> CenterTabType {
    // Folder-based scripts: check _instance.toml for class_name = "Script"
    if path.is_dir() {
        let inst = path.join("_instance.toml");
        if inst.exists() {
            if let Ok(content) = std::fs::read_to_string(&inst) {
                if content.contains("\"Script\"") || content.contains("\"SoulScript\"") {
                    return CenterTabType::SoulScript { mode: SoulScriptMode::Code };
                }
            }
        }
    }

    // Helper used by the `.md` branch below — declared as an inner fn so
    // the `tab_type_from_path` top-level stays readable.
    fn is_soul_script_summary(md_path: &Path) -> bool {
        // A Soul Script folder has a sibling `_instance.toml` whose
        // `class_name` says "Script" / "SoulScript". Any other `.md`
        // (README, PATENT, SOTA docs, etc.) is just documentation.
        let Some(parent) = md_path.parent() else { return false };
        let inst = parent.join("_instance.toml");
        if !inst.exists() {
            return false;
        }
        let Ok(content) = std::fs::read_to_string(&inst) else { return false };
        content.contains("\"Script\"") || content.contains("\"SoulScript\"")
    }

    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        // Soul scripts: .rune and .soul always open in Code mode.
        "soul" | "rune" => CenterTabType::SoulScript { mode: SoulScriptMode::Code },

        // Markdown opens two different ways depending on context:
        //   * sitting alongside a Soul Script (same folder has an
        //     `_instance.toml` whose `class_name` is "Script" /
        //     "SoulScript") → SoulScript Summary tab with Build + Code
        //     toggle, because it's the script's summary brief;
        //   * anywhere else (README.md under a Part folder, loose docs
        //     under Workspace, etc.) → plain Markdown document — no
        //     Build button, no Code tab.
        "md" | "markdown" => {
            if is_soul_script_summary(path) {
                CenterTabType::SoulScript { mode: SoulScriptMode::Summary }
            } else {
                // Plain markdown — same SoulScript tab container (so we
                // reuse the editor surface + syntax highlighting) but
                // `Markdown` mode hides the Code toggle + Build button.
                CenterTabType::SoulScript { mode: SoulScriptMode::Markdown }
            }
        }

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
        // .md is now routed to SoulScript Summary above — skip here
        "txt" | "log" | "cfg" | "ini" | "env" => CenterTabType::CodeEditor { language: "plaintext".into() },

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
            CenterTabType::SoulScript { .. }
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

    /// A fresh scratch folder under the system temp dir, removed on drop.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "eustress-center-tabs-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Scratch(p)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn code_tab(file_path: Option<PathBuf>, content: &str) -> CenterTabEntry {
        CenterTabEntry {
            id: 1,
            name: "ClientController".into(),
            tab_type: CenterTabType::SoulScript { mode: SoulScriptMode::Code },
            entity: None,
            file_path,
            url: None,
            pinned: false,
            dirty: false,
            loading: false,
            content: content.into(),
            summary_content: String::new(),
        }
    }

    #[test]
    fn save_writes_the_luau_file_the_folder_holds_never_a_new_rune() {
        let s = Scratch::new("luau");
        let folder = s.0.join("ClientController");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("_instance.toml"), "[metadata]\n").unwrap();
        std::fs::write(folder.join("ClientController.client.luau"), "print(1)\n").unwrap();

        let tab = code_tab(Some(folder.clone()), "print(2)\n");
        let saved = save_code_tab(&tab, None).unwrap().unwrap();

        assert_eq!(saved, folder.join("ClientController.client.luau"));
        assert_eq!(std::fs::read_to_string(&saved).unwrap(), "print(2)\n");
        assert!(!folder.join("ClientController.rune").exists());
    }

    #[test]
    fn save_prefers_the_entitys_loaded_source_file() {
        let s = Scratch::new("entity");
        let folder = s.0.join("Door");
        std::fs::create_dir_all(&folder).unwrap();
        let named = folder.join("Door.server.luau");
        std::fs::write(&named, "-- old\n").unwrap();
        std::fs::write(folder.join("Other.luau"), "-- other\n").unwrap();

        let tab = code_tab(None, "-- new\n");
        let saved = save_code_tab(&tab, Some(&named)).unwrap().unwrap();

        assert_eq!(saved, named);
        assert_eq!(std::fs::read_to_string(&named).unwrap(), "-- new\n");
        assert_eq!(std::fs::read_to_string(folder.join("Other.luau")).unwrap(), "-- other\n");
    }

    #[test]
    fn save_keeps_crlf_files_crlf() {
        let s = Scratch::new("crlf");
        let file = s.0.join("a.rune");
        std::fs::write(&file, "pub fn main() {\r\n}\r\n").unwrap();

        let mut tab = code_tab(Some(file.clone()), "pub fn main() {\n    1\n}\n");
        tab.tab_type = CenterTabType::CodeEditor { language: "rune".into() };
        save_code_tab(&tab, None).unwrap();

        assert_eq!(
            std::fs::read(&file).unwrap(),
            b"pub fn main() {\r\n    1\r\n}\r\n".to_vec()
        );
    }

    #[test]
    fn save_from_the_summary_view_writes_the_code_and_a_markdown_document_has_no_target() {
        let s = Scratch::new("views");
        let file = s.0.join("x.rune");
        let mut tab = code_tab(Some(file.clone()), "code");
        tab.tab_type = CenterTabType::SoulScript { mode: SoulScriptMode::Summary };
        assert_eq!(code_save_target(&tab, None), Some(file));

        tab.tab_type = CenterTabType::SoulScript { mode: SoulScriptMode::Markdown };
        assert_eq!(code_save_target(&tab, None), None);
    }

    /// Scene plus four web tabs "a".."d"; returns the manager and the ids.
    fn four_tabs() -> (CenterTabManager, Vec<u32>) {
        let mut mgr = CenterTabManager::default();
        let ids = ["a", "b", "c", "d"]
            .iter()
            .map(|n| {
                let i = mgr.open_web_tab(&format!("https://{n}.test"), n);
                mgr.tabs[i].id
            })
            .collect();
        (mgr, ids)
    }

    #[test]
    fn a_clean_close_runs_at_once() {
        let (mut mgr, ids) = four_tabs();
        assert!(mgr.request_close(TabCloseRequest::One(ids[1])));
        assert_eq!(names(&mgr), ["a", "c", "d"]);
        assert_eq!(mgr.pending_close, None);
    }

    #[test]
    fn a_dirty_tab_holds_the_close_for_the_prompt() {
        let (mut mgr, ids) = four_tabs();
        mgr.tabs[3].dirty = true; // "c"
        assert!(!mgr.request_close(TabCloseRequest::ToRight(ids[0])));
        assert_eq!(mgr.pending_close, Some(TabCloseRequest::ToRight(ids[0])));
        assert_eq!(names(&mgr), ["a", "b", "c", "d"], "nothing closes before the answer");
        assert_eq!(mgr.dirty_tabs_closed_by(TabCloseRequest::ToRight(ids[0])), [3]);

        // Don't Save: the held close runs as asked.
        let req = mgr.pending_close.take().unwrap();
        mgr.apply_close(req);
        assert_eq!(names(&mgr), ["a"]);
    }

    #[test]
    fn requests_follow_tab_ids_when_tabs_move() {
        let (mut mgr, ids) = four_tabs();
        let req = TabCloseRequest::Others(ids[2]); // keep "c"
        mgr.close_tab(1); // "a" goes first; indices shift
        mgr.apply_close(req);
        assert_eq!(names(&mgr), ["c"]);
    }

    #[test]
    fn pinned_tabs_survive_bulk_closes_but_not_exit() {
        let (mut mgr, ids) = four_tabs();
        mgr.tabs[2].pinned = true; // "b"
        mgr.tabs[2].dirty = true;
        assert!(mgr.dirty_tabs_closed_by(TabCloseRequest::All).is_empty());
        assert!(mgr.dirty_tabs_closed_by(TabCloseRequest::Others(ids[0])).is_empty());
        assert_eq!(mgr.dirty_tabs_closed_by(TabCloseRequest::AppExit), [2]);
        assert!(!mgr.close_covers(TabCloseRequest::AppExit, 0), "the Scene tab never closes");
        mgr.apply_close(TabCloseRequest::All);
        assert_eq!(names(&mgr), ["b"]);
    }

    #[test]
    fn dirty_and_clean_report_only_real_transitions() {
        let mut mgr = CenterTabManager::default();
        let idx = mgr.open_web_tab("https://a.test", "a");
        mgr.dirty = false;

        assert!(mgr.mark_dirty(idx));
        assert!(!mgr.mark_dirty(idx), "already dirty");
        assert!(mgr.tabs[idx].dirty);
        assert!(mgr.mark_clean(idx));
        assert!(!mgr.mark_clean(idx), "already clean");
        assert!(!mgr.dirty, "marking a tab never rebuilds the whole strip");
    }

    /// Scene plus web tabs named `names`, in order, with the last one active.
    fn strip(names: &[&str]) -> CenterTabManager {
        let mut mgr = CenterTabManager::default();
        for n in names {
            mgr.open_web_tab(&format!("https://{n}.test"), n);
        }
        mgr
    }

    fn names(mgr: &CenterTabManager) -> Vec<&str> {
        mgr.tabs[1..].iter().map(|t| t.name.as_str()).collect()
    }

    #[test]
    fn drop_slots_land_where_the_caret_was() {
        // Dragging right: the gap before D puts A between C and D.
        let mut mgr = strip(&["A", "B", "C", "D"]);
        mgr.move_tab_to_slot(1, 4);
        assert_eq!(names(&mgr), ["B", "C", "A", "D"]);

        // The gap after the last tab is a real slot (it used to be refused).
        let mut mgr = strip(&["A", "B", "C"]);
        mgr.move_tab_to_slot(1, 4);
        assert_eq!(names(&mgr), ["B", "C", "A"]);

        // Dragging left.
        let mut mgr = strip(&["A", "B", "C"]);
        mgr.move_tab_to_slot(3, 1);
        assert_eq!(names(&mgr), ["C", "A", "B"]);

        // Either gap beside the tab itself is a no-op.
        let mut mgr = strip(&["A", "B", "C"]);
        mgr.dirty = false;
        mgr.move_tab_to_slot(2, 2);
        mgr.move_tab_to_slot(2, 3);
        assert_eq!(names(&mgr), ["A", "B", "C"]);
        assert!(!mgr.dirty);
    }

    #[test]
    fn pinned_scene_never_moves_and_nothing_lands_before_it() {
        let mut mgr = strip(&["A", "B"]);
        mgr.move_tab_to_slot(0, 2);
        mgr.move_tab_to_slot(2, 0);
        assert_eq!(mgr.tabs[0].tab_type, CenterTabType::Scene);
        assert_eq!(names(&mgr), ["B", "A"]);
    }

    #[test]
    fn active_tab_follows_its_tab_through_moves_and_sorts() {
        let mut mgr = strip(&["c", "B", "a"]);
        mgr.select_tab(2); // B
        mgr.move_tab_to_slot(1, 4); // c to the end
        assert_eq!(mgr.tabs[mgr.active_tab].name, "B");
        mgr.sort_tabs(TabSortKey::Name);
        assert_eq!(names(&mgr), ["a", "B", "c"]);
        assert_eq!(mgr.tabs[mgr.active_tab].name, "B");
    }

    #[test]
    fn sort_by_type_groups_kinds_then_names() {
        let mut mgr = strip(&["zeta"]);
        mgr.open_file(Path::new("beta.rs"));
        mgr.open_web_tab("https://alpha.test", "alpha");
        mgr.open_file(Path::new("alpha.rs"));
        mgr.sort_tabs(TabSortKey::Type);
        assert_eq!(names(&mgr), ["alpha.rs", "beta.rs", "alpha", "zeta"]);
    }

    #[test]
    fn bulk_closes_are_reopenable_and_keep_the_right_tab_active() {
        let mut mgr = strip(&["A", "B", "C", "D"]);
        mgr.close_others(2); // B
        assert_eq!(names(&mgr), ["B"]);
        assert_eq!(mgr.tabs[mgr.active_tab].name, "B");
        assert!(mgr.reopen_last_closed());
        assert_eq!(mgr.tabs.last().unwrap().name, "D");

        let mut mgr = strip(&["A", "B", "C", "D"]); // D active
        mgr.close_to_right(2);
        assert_eq!(names(&mgr), ["A", "B"]);
        assert_eq!(mgr.tabs[mgr.active_tab].name, "B");

        mgr.close_all_unpinned();
        assert_eq!(mgr.tabs.len(), 1);
        assert_eq!(mgr.active_tab, 0);
    }
}

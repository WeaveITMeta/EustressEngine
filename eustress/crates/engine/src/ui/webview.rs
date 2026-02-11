//! # WebView Browser Integration
//!
//! Manages wry WebView2 instances as child windows overlaid on the Bevy window.
//! Each web tab gets its own WebView instance, positioned to match the center
//! content area. Only the active web tab's WebView is visible.
//!
//! Architecture:
//! - WebView instances are native child windows (WebView2 on Windows)
//! - Positioned/resized each frame to match the Slint content area bounds
//! - Hidden when a non-web tab is active, shown when a web tab is active
//! - Title/URL changes are forwarded back to StudioState via channels

use bevy::prelude::*;
use std::collections::HashMap;

/// Bevy plugin for wry-based web browser tabs
pub struct WebViewPlugin;

impl Plugin for WebViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WebViewManager>()
            .add_systems(Update, sync_webviews);
    }
}

/// Manages all active WebView instances
#[derive(Resource, Default)]
pub struct WebViewManager {
    /// Map of tab index -> WebView state
    pub views: HashMap<usize, WebViewInstance>,
    /// Whether wry is initialized
    pub initialized: bool,
}

/// State for a single WebView instance
pub struct WebViewInstance {
    /// Current URL
    pub url: String,
    /// Page title (updated by WebView callbacks)
    pub title: String,
    /// Whether the page is loading
    pub loading: bool,
    /// Can navigate back
    pub can_go_back: bool,
    /// Can navigate forward
    pub can_go_forward: bool,
    /// Whether the WebView is currently visible
    pub visible: bool,
    /// The wry WebView handle (only available with webview feature)
    #[cfg(feature = "webview")]
    pub webview: Option<wry::WebView>,
}

impl Default for WebViewInstance {
    fn default() -> Self {
        Self {
            url: "about:blank".to_string(),
            title: "New Tab".to_string(),
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            visible: false,
            #[cfg(feature = "webview")]
            webview: None,
        }
    }
}

impl WebViewManager {
    /// Create a new WebView for a tab
    pub fn create_webview(&mut self, tab_index: usize, url: &str) {
        let instance = WebViewInstance {
            url: url.to_string(),
            title: if url == "about:blank" { "New Tab".to_string() } else { url.to_string() },
            loading: url != "about:blank",
            ..Default::default()
        };
        self.views.insert(tab_index, instance);
        
        #[cfg(feature = "webview")]
        {
            // TODO: Create actual wry::WebView child window using Bevy's window handle
            info!("Created WebView for tab {} with URL: {}", tab_index, url);
        }
        #[cfg(not(feature = "webview"))]
        {
            info!("Created WebView placeholder for tab {} (webview feature not enabled)", tab_index);
        }
    }

    /// Navigate a WebView to a URL
    pub fn navigate(&mut self, tab_index: usize, url: &str) {
        if let Some(view) = self.views.get_mut(&tab_index) {
            view.url = url.to_string();
            view.loading = true;
            #[cfg(feature = "webview")]
            if let Some(ref webview) = view.webview {
                let _ = webview.load_url(url);
            }
        }
    }

    /// Go back in a WebView's history
    pub fn go_back(&mut self, tab_index: usize) {
        if let Some(view) = self.views.get_mut(&tab_index) {
            #[cfg(feature = "webview")]
            if let Some(ref webview) = view.webview {
                let _ = webview.evaluate_script("window.history.back()");
            }
            let _ = view; // suppress unused warning without webview feature
        }
    }

    /// Go forward in a WebView's history
    pub fn go_forward(&mut self, tab_index: usize) {
        if let Some(view) = self.views.get_mut(&tab_index) {
            #[cfg(feature = "webview")]
            if let Some(ref webview) = view.webview {
                let _ = webview.evaluate_script("window.history.forward()");
            }
            let _ = view;
        }
    }

    /// Refresh a WebView
    pub fn refresh(&mut self, tab_index: usize) {
        if let Some(view) = self.views.get_mut(&tab_index) {
            view.loading = true;
            #[cfg(feature = "webview")]
            if let Some(ref webview) = view.webview {
                let _ = webview.evaluate_script("window.location.reload()");
            }
            let _ = view;
        }
    }

    /// Remove a WebView for a closed tab
    pub fn remove_webview(&mut self, tab_index: usize) {
        self.views.remove(&tab_index);
    }

    /// Show/hide WebViews based on active tab
    pub fn set_active_tab(&mut self, active_tab_index: Option<usize>) {
        for (idx, view) in self.views.iter_mut() {
            let should_show = active_tab_index == Some(*idx);
            if view.visible != should_show {
                view.visible = should_show;
                #[cfg(feature = "webview")]
                if let Some(ref webview) = view.webview {
                    let _ = webview.set_visible(should_show);
                }
            }
        }
    }
}

/// Bevy system that syncs WebView state with StudioState
fn sync_webviews(
    mut webview_mgr: ResMut<WebViewManager>,
    mut state: Option<ResMut<super::StudioState>>,
) {
    let Some(ref mut state) = state else { return };

    // Determine which tab index (0-based in center_tabs) is the active web tab
    let active_web_idx = if state.active_center_tab > 0 {
        let idx = (state.active_center_tab - 1) as usize;
        if idx < state.center_tabs.len() && state.center_tabs[idx].tab_type == "web" {
            Some(idx)
        } else {
            None
        }
    } else {
        None
    };

    // Show/hide WebViews
    webview_mgr.set_active_tab(active_web_idx);

    // Process pending web navigation
    if let Some(url) = state.pending_web_navigate.take() {
        if let Some(idx) = active_web_idx {
            webview_mgr.navigate(idx, &url);
            // Update tab data
            if let Some(tab) = state.center_tabs.get_mut(idx) {
                tab.url = url.clone();
                tab.name = url;
                tab.loading = true;
            }
        }
    }

    // Process pending back/forward/refresh
    if state.pending_web_back {
        state.pending_web_back = false;
        if let Some(idx) = active_web_idx {
            webview_mgr.go_back(idx);
        }
    }
    if state.pending_web_forward {
        state.pending_web_forward = false;
        if let Some(idx) = active_web_idx {
            webview_mgr.go_forward(idx);
        }
    }
    if state.pending_web_refresh {
        state.pending_web_refresh = false;
        if let Some(idx) = active_web_idx {
            webview_mgr.refresh(idx);
        }
    }

    // Ensure WebViews exist for all web tabs
    for (idx, tab) in state.center_tabs.iter().enumerate() {
        if tab.tab_type == "web" && !webview_mgr.views.contains_key(&idx) {
            webview_mgr.create_webview(idx, &tab.url);
        }
    }

    // Remove WebViews for tabs that no longer exist
    let valid_indices: Vec<usize> = state.center_tabs.iter().enumerate()
        .filter(|(_, t)| t.tab_type == "web")
        .map(|(i, _)| i)
        .collect();
    webview_mgr.views.retain(|k, _| valid_indices.contains(k));

    // Sync WebView state back to tab data
    for (idx, view) in webview_mgr.views.iter() {
        if let Some(tab) = state.center_tabs.get_mut(*idx) {
            if tab.tab_type == "web" {
                tab.loading = view.loading;
                if !view.title.is_empty() && view.title != tab.name {
                    tab.name = view.title.clone();
                }
            }
        }
    }
}

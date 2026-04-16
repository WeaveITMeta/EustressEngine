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

#[cfg(feature = "webview")]
use bevy::winit::WinitWindows;

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
    /// Create a new WebView for a tab.
    /// `raw_window` is the parent HWND/NSWindow obtained from Bevy's WinitWindows.
    #[cfg(feature = "webview")]
    pub fn create_webview(&mut self, tab_index: usize, url: &str, raw_window: raw_window_handle::RawWindowHandle) {
        use wry::WebViewBuilder;

        let webview_result = match raw_window {
            #[cfg(target_os = "windows")]
            raw_window_handle::RawWindowHandle::Win32(handle) => {
                use wry::raw_window_handle::{RawWindowHandle, Win32WindowHandle};
                let mut wh = Win32WindowHandle::new(handle.hwnd);
                wh.hinstance = handle.hinstance;
                let parent = RawWindowHandle::Win32(wh);
                unsafe {
                    WebViewBuilder::new()
                        .with_url(url)
                        .with_visible(true)
                        .with_bounds(wry::Rect { position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)), size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(800.0, 600.0)) })
                        .build_as_child(&parent)
                }
            }
            _ => {
                warn!("Unsupported window handle for WebView");
                return;
            }
        };

        match webview_result {
            Ok(webview) => {
                let instance = WebViewInstance {
                    url: url.to_string(),
                    title: url.to_string(),
                    loading: url != "about:blank",
                    visible: true,
                    webview: Some(webview),
                    ..Default::default()
                };
                self.views.insert(tab_index, instance);
                info!("🌐 Created WebView for tab {} with URL: {}", tab_index, url);
            }
            Err(e) => {
                error!("Failed to create WebView: {}", e);
                let instance = WebViewInstance {
                    url: url.to_string(),
                    title: "Error".to_string(),
                    loading: false,
                    ..Default::default()
                };
                self.views.insert(tab_index, instance);
            }
        }
    }

    /// Create a placeholder WebView (no webview feature)
    #[cfg(not(feature = "webview"))]
    pub fn create_webview(&mut self, tab_index: usize, url: &str) {
        let instance = WebViewInstance {
            url: url.to_string(),
            title: if url == "about:blank" { "New Tab".to_string() } else { url.to_string() },
            loading: false,
            ..Default::default()
        };
        self.views.insert(tab_index, instance);
        info!("Created WebView placeholder for tab {} (webview feature not enabled)", tab_index);
    }

    /// Navigate a WebView to a URL
    pub fn navigate(&mut self, tab_index: usize, url: &str) {
        if let Some(view) = self.views.get_mut(&tab_index) {
            view.url = url.to_string();
            // Only show loading state when a real webview can actually load the page
            #[cfg(feature = "webview")]
            {
                view.loading = true;
                if let Some(ref webview) = view.webview {
                    let _ = webview.load_url(url);
                }
            }
            #[cfg(not(feature = "webview"))]
            {
                view.loading = false;
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
            #[cfg(feature = "webview")]
            {
                view.loading = true;
                if let Some(ref webview) = view.webview {
                    let _ = webview.evaluate_script("window.location.reload()");
                }
            }
            let _ = view;
        }
    }

    /// Reposition and resize a WebView to match the center content area
    #[cfg(feature = "webview")]
    pub fn set_bounds(&mut self, tab_index: usize, x: f64, y: f64, width: f64, height: f64) {
        if let Some(view) = self.views.get_mut(&tab_index) {
            if let Some(ref webview) = view.webview {
                let bounds = wry::Rect {
                    position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(x, y)),
                    size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(width, height)),
                };
                let _ = webview.set_bounds(bounds);
            }
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
    #[cfg(feature = "webview")]
    winit_windows: Option<NonSend<WinitWindows>>,
    #[cfg(feature = "webview")]
    primary_window: Query<Entity, With<bevy::window::PrimaryWindow>>,
    #[cfg(feature = "webview")]
    viewport_bounds: Option<Res<super::ViewportBounds>>,
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

    // Create WebView instances for tabs that don't have one yet
    #[cfg(feature = "webview")]
    if let Some(idx) = active_web_idx {
        if !webview_mgr.views.contains_key(&idx) {
            if let Some(ref winit) = winit_windows {
                if let Ok(entity) = primary_window.single() {
                    if let Some(winit_window) = winit.get_window(entity) {
                        use raw_window_handle::HasWindowHandle;
                        if let Ok(handle) = winit_window.window_handle() {
                            let raw = handle.as_raw();
                            let url = state.center_tabs.get(idx).map(|t| t.url.as_str()).unwrap_or("about:blank");
                            webview_mgr.create_webview(idx, url, raw);
                        }
                    }
                }
            }
        }
        // Update bounds to match viewport area
        if let Some(ref vb) = viewport_bounds {
            webview_mgr.set_bounds(idx, vb.x as f64, vb.y as f64, vb.width as f64, vb.height as f64);
        }
    }

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
                // Only set loading when webview feature is active and can actually clear it
                #[cfg(feature = "webview")]
                { tab.loading = true; }
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

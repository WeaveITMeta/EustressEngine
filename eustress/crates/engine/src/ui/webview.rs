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
        app.insert_non_send_resource(WebViewManager::default())
            .add_systems(Update, sync_webviews);
    }
}

/// Manages all active WebView instances.
/// NonSend because wry::WebView contains raw window handles (not Send/Sync).
#[derive(Default)]
pub struct WebViewManager {
    /// Map of tab index -> WebView state
    pub views: HashMap<usize, WebViewInstance>,
    /// Whether wry is initialized
    pub initialized: bool,
}

/// Live page state written by wry's callbacks and read by `sync_webviews`.
///
/// wry fires `on_page_load` / `document_title_changed` on the UI thread from
/// inside WebView2's message handling, where the Bevy world is not reachable,
/// so they write here and the system copies it across each frame. Without
/// this, `loading` was set once at creation and never cleared: the progress
/// bar ran forever and the address bar kept its Stop button instead of Reload.
#[derive(Default)]
pub struct PageSignals {
    pub loading: bool,
    pub title: Option<String>,
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
    /// Written by wry's page-load and title callbacks; see `PageSignals`.
    pub signals: std::sync::Arc<std::sync::Mutex<PageSignals>>,
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
            signals: std::sync::Arc::new(std::sync::Mutex::new(PageSignals::default())),
            #[cfg(feature = "webview")]
            webview: None,
        }
    }
}

impl WebViewManager {
    /// Create a new WebView for a tab as a child of the given window.
    #[cfg(feature = "webview")]
    pub fn create_webview(&mut self, tab_index: usize, url: &str, window: &winit::window::Window) {
        use wry::WebViewBuilder;

        let signals = std::sync::Arc::new(std::sync::Mutex::new(PageSignals {
            loading: url != "about:blank",
            title: None,
        }));
        let on_load = signals.clone();
        let on_title = signals.clone();

        let webview_result = WebViewBuilder::new()
            .with_url(url)
            .with_visible(true)
            .with_bounds(wry::Rect {
                position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
                size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(800.0, 600.0)),
            })
            .with_on_page_load_handler(move |event, _url| {
                if let Ok(mut s) = on_load.lock() {
                    s.loading = matches!(event, wry::PageLoadEvent::Started);
                }
            })
            .with_document_title_changed_handler(move |title| {
                if let Ok(mut s) = on_title.lock() {
                    s.title = Some(title);
                }
            })
            .build_as_child(window);

        match webview_result {
            Ok(webview) => {
                let instance = WebViewInstance {
                    url: url.to_string(),
                    title: url.to_string(),
                    loading: url != "about:blank",
                    visible: true,
                    signals,
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
    mut webview_mgr: NonSendMut<WebViewManager>,
    mut state: Option<ResMut<super::StudioState>>,
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
        // Bevy 0.19 keeps winit windows in the `bevy::winit::WINIT_WINDOWS`
        // thread-local and never inserts `WinitWindows` as a resource. This
        // system used to take `Option<NonSend<WinitWindows>>`, which still
        // compiled but was `None` on every frame, so the WebView was never
        // created: the tab strip and address bar drew, the page area stayed
        // empty, and nothing was logged. `NonSendMut<WebViewManager>` pins this
        // system to the main thread, which is the thread whose copy of the
        // thread-local bevy_winit fills in.
        let primary = primary_window.single().ok();
        if !webview_mgr.views.contains_key(&idx) {
            match primary {
                Some(entity) => {
                    let url = state
                        .center_tabs
                        .get(idx)
                        .map(|t| t.url.clone())
                        .unwrap_or_else(|| "about:blank".to_string());
                    let created = bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
                        match windows.get_window(entity) {
                            Some(winit_window) => {
                                webview_mgr.create_webview(idx, &url, winit_window);
                                true
                            }
                            None => false,
                        }
                    });
                    if !created {
                        warn!("Web tab {} is active but the primary window has no winit window yet; retrying next frame", idx);
                    }
                }
                None => warn!("Web tab {} is active but there is no primary window", idx),
            }
        }
        // Update bounds to match viewport area.
        //
        // `ViewportBounds` is PHYSICAL pixels (see `ui/mod.rs`), but
        // `set_bounds` hands wry `Position::Logical` / `Size::Logical`.
        // Feeding physical values straight in placed and sized the native
        // WebView2 child window in the wrong space on any display with DPI
        // scaling != 1.0 — at 150% it lands 1.5x too far right/down and 1.5x
        // too large, so the page renders off the visible content area (the
        // tab chrome around it still draws, which is why this looked like
        // "the browser is blank"), and the oversized child window covers the
        // tab strip and swallows clicks meant for the tab and its X.
        //
        // Same physical-vs-logical trap `ViewportBounds::contains_logical`
        // exists to prevent for cursor hit-testing.
        if let Some(ref vb) = viewport_bounds {
            let scale = primary
                .and_then(|e| {
                    bevy::winit::WINIT_WINDOWS
                        .with_borrow(|w| w.get_window(e).map(|w| w.scale_factor()))
                })
                .unwrap_or(1.0)
                .max(0.0001);
            webview_mgr.set_bounds(
                idx,
                vb.x as f64 / scale,
                vb.y as f64 / scale,
                vb.width as f64 / scale,
                vb.height as f64 / scale,
            );
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
    #[cfg(not(feature = "webview"))]
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

    // Pull what wry's callbacks reported since last frame.
    for view in webview_mgr.views.values_mut() {
        let (loading, title) = match view.signals.lock() {
            Ok(s) => (s.loading, s.title.clone()),
            Err(_) => continue,
        };
        view.loading = loading;
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            view.title = t;
        }
    }

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

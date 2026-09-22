// =============================================================================
// Eustress Web - Main Library Entry Point
// =============================================================================
// Table of Contents:
// 1. Module Declarations
// 2. Re-exports
// 3. WASM Entry Point
// =============================================================================

// The `ssr` build streams each view through `into_any`, whose async closures
// carry the full nested view type; the login page alone exceeds the default
// query depth of 128 when its layout is computed. This costs nothing in the
// browser build.
#![recursion_limit = "512"]

// -----------------------------------------------------------------------------
// 1. Module Declarations
// -----------------------------------------------------------------------------

pub mod api;
pub mod app;
pub mod capture;
pub mod components;
pub mod pages;
pub mod services;
pub mod state;
pub mod utils;
pub mod wallet;

// -----------------------------------------------------------------------------
// 2. Re-exports
// -----------------------------------------------------------------------------

pub use app::App;
pub use state::AppState;

// -----------------------------------------------------------------------------
// 3. WASM Entry Point (for library usage)
// -----------------------------------------------------------------------------

use wasm_bindgen::prelude::*;

/// Mount the live app over whatever the page shipped with.
///
/// The prerender bin (src/bin/prerender.rs) writes each public route with its
/// markup already inside `#app`, so crawlers and agents get the page without
/// running WASM. The live app renders the same tree into `<body>`, so the
/// static copy is emptied first; otherwise the page would show twice.
pub fn mount_app() {
    if let Some(app) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("app"))
    {
        app.set_inner_html("");
    }
    leptos::mount::mount_to_body(app::App);
}

/// Mount the Leptos app to the DOM (for external callers).
#[wasm_bindgen]
pub fn mount() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Debug);
    log::info!("Mounting Eustress Web app...");
    mount_app();
}

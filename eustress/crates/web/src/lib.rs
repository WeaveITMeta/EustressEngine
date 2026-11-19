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
pub mod critical_css;
pub mod identity_file;
pub mod pages;
pub mod seo;
pub mod services;
pub mod state;
pub mod tags;
pub mod time_words;
pub mod utils;
pub mod wallet;
pub mod webauthn;

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
    let document = web_sys::window().and_then(|w| w.document());
    if let Some(document) = &document {
        // A prerendered page parks the full stylesheets (address in data-href,
        // media="print") so they do not compete with its first paint (see
        // critical_css). assets/loader.js puts them back before this runs; this
        // is the guarantee, so the live app is never drawn with only one page's
        // critical CSS.
        let selector = format!("link[{}][media=\"print\"]", critical_css::DEFERRED);
        while let Ok(Some(sheet)) = document.query_selector(&selector) {
            if let Some(href) = sheet.get_attribute("data-href") {
                if sheet.get_attribute("href").is_none() {
                    let _ = sheet.set_attribute("href", &href);
                }
            }
            if sheet.set_attribute("media", "all").is_err() {
                break;
            }
        }
        if let Some(app) = document.get_element_by_id("app") {
            app.set_inner_html("");
        }
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

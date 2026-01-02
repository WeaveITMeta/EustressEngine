// =============================================================================
// Eustress Web - Main Library Entry Point
// =============================================================================
// Table of Contents:
// 1. Module Declarations
// 2. Re-exports
// 3. WASM Entry Point
// =============================================================================

// -----------------------------------------------------------------------------
// 1. Module Declarations
// -----------------------------------------------------------------------------

pub mod api;
pub mod app;
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

/// Mount the Leptos app to the DOM (for external callers).
#[wasm_bindgen]
pub fn mount() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Debug);
    log::info!("Mounting Eustress Web app...");
    leptos::mount::mount_to_body(app::App);
}

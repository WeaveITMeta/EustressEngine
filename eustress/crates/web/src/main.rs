// =============================================================================
// Eustress Web - WASM Entry Point
// =============================================================================
// This is the main entry point for the WASM binary.
// Trunk compiles this and injects it into index.html.
// =============================================================================

use eustress_web::App;

fn main() {
    // Initialize panic hook for better error messages
    console_error_panic_hook::set_once();
    
    // Initialize logger (ignore if already initialized by wasm_bindgen start)
    let _ = console_log::init_with_level(log::Level::Debug);
    
    log::info!("🚀 Starting Eustress Web...");
    
    // Mount the Leptos app to the body
    leptos::mount::mount_to_body(App);
}

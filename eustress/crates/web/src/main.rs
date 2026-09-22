// =============================================================================
// Eustress Web - WASM Entry Point
// =============================================================================
// This is the main entry point for the WASM binary.
// Trunk compiles this and injects it into index.html.
// =============================================================================

fn main() {
    // Initialize panic hook for better error messages
    console_error_panic_hook::set_once();
    
    // Initialize logger (ignore if already initialized by wasm_bindgen start)
    let _ = console_log::init_with_level(log::Level::Debug);
    
    log::info!("🚀 Starting Eustress Web...");
    
    // Mount the Leptos app to the body, clearing any prerendered markup first
    eustress_web::mount_app();
}

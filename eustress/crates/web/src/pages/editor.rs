// =============================================================================
// Eustress Web - Editor Page
// =============================================================================
// This page will host the WASM-compiled Eustress engine in a canvas.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::hooks::use_params_map;
use crate::components::{LoadingSpinner, Button, ButtonVariant};

/// 3D Editor page - hosts the Eustress engine canvas.
#[component]
pub fn EditorPage() -> impl IntoView {
    let params = use_params_map();
    let project_id = move || params.get().get("id").unwrap_or_default();
    
    // Editor state
    let loading = RwSignal::new(true);
    let engine_ready = RwSignal::new(false);
    
    // Simulate engine loading
    Effect::new(move |_| {
        let loading = loading.clone();
        let engine_ready = engine_ready.clone();
        
        spawn_local(async move {
            // TODO: Actually load the Eustress WASM engine here
            gloo_timers::future::TimeoutFuture::new(1500).await;
            loading.set(false);
            engine_ready.set(true);
        });
    });
    
    view! {
        <div class="page page-editor">
            // Editor toolbar
            <div class="editor-toolbar">
                <div class="toolbar-left">
                    <span class="project-id">"Project: " {project_id}</span>
                </div>
                <div class="toolbar-center">
                    <Button label="Select" variant=ButtonVariant::Ghost />
                    <Button label="Move" variant=ButtonVariant::Ghost />
                    <Button label="Rotate" variant=ButtonVariant::Ghost />
                    <Button label="Scale" variant=ButtonVariant::Ghost />
                </div>
                <div class="toolbar-right">
                    <Button label="Play" variant=ButtonVariant::Primary />
                    <Button label="Save" variant=ButtonVariant::Secondary />
                </div>
            </div>
            
            // Main editor area
            <div class="editor-main">
                // Left panel - Explorer
                <aside class="editor-panel panel-left">
                    <div class="panel-header">"Explorer"</div>
                    <div class="panel-content">
                        <div class="tree-item">"📁 Workspace"</div>
                        <div class="tree-item indent">"📦 Part"</div>
                        <div class="tree-item indent">"💡 PointLight"</div>
                        <div class="tree-item indent">"📷 Camera"</div>
                    </div>
                </aside>
                
                // Viewport
                <div class="editor-viewport">
                    <Show
                        when=move || !loading.get()
                        fallback=|| view! {
                            <LoadingSpinner message="Loading Eustress Engine..." />
                        }
                    >
                        // Canvas for Eustress WASM engine
                        <canvas id="eustress-canvas" class="viewport-canvas">
                            "Your browser does not support WebGL2"
                        </canvas>
                        
                        // Viewport overlay
                        <div class="viewport-overlay">
                            <div class="viewport-info">
                                <span>"FPS: 60"</span>
                                <span>"Entities: 3"</span>
                            </div>
                        </div>
                    </Show>
                </div>
                
                // Right panel - Properties
                <aside class="editor-panel panel-right">
                    <div class="panel-header">"Properties"</div>
                    <div class="panel-content">
                        <div class="property-group">
                            <div class="property-group-header">"Transform"</div>
                            <div class="property-row">
                                <span class="property-label">"Position"</span>
                                <span class="property-value">"0, 0, 0"</span>
                            </div>
                            <div class="property-row">
                                <span class="property-label">"Rotation"</span>
                                <span class="property-value">"0, 0, 0"</span>
                            </div>
                            <div class="property-row">
                                <span class="property-label">"Scale"</span>
                                <span class="property-value">"1, 1, 1"</span>
                            </div>
                        </div>
                    </div>
                </aside>
            </div>
            
            // Bottom panel - Output
            <div class="editor-panel panel-bottom">
                <div class="panel-header">"Output"</div>
                <div class="panel-content output-log">
                    <div class="log-entry info">"[INFO] Eustress Engine initialized"</div>
                    <div class="log-entry info">"[INFO] Project loaded successfully"</div>
                </div>
            </div>
        </div>
    }
}

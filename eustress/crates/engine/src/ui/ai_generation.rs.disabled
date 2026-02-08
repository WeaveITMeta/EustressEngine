//! # AI Generation Panel
//!
//! Studio UI for AI asset generation.
//!
//! ## Features
//!
//! - Generate button in properties panel
//! - Generation queue display
//! - Progress tracking
//! - Backend configuration

use bevy::prelude::*;
use bevy_egui::egui;

use eustress_common::scene::{DetailLevel, NodeCategory, GenerationStatus};
use eustress_common::services::generation::{
    AIGenerationService, GenerationRequest, RequestGenerationEvent,
    CancelGenerationEvent, AIBackend, AIGenerationConfig,
};

/// AI Generation panel state
#[derive(Resource, Default)]
pub struct AIGenerationPanel {
    /// Show settings window
    pub show_settings: bool,
    /// Custom prompt override
    pub custom_prompt: String,
    /// Use custom prompt instead of entity prompt
    pub use_custom_prompt: bool,
    /// Selected detail level override
    pub detail_override: Option<DetailLevel>,
    /// Generate LODs
    pub generate_lods: bool,
    /// Generate textures
    pub generate_textures: bool,
}

impl AIGenerationPanel {
    /// Show AI generation section in properties panel
    pub fn show_in_properties(
        &mut self,
        ui: &mut egui::Ui,
        entity_id: u32,
        entity_name: &str,
        prompt: &str,
        category: NodeCategory,
        detail_level: DetailLevel,
        generation_status: &GenerationStatus,
        global_theme: &str,
        generation_events: &mut MessageWriter<RequestGenerationEvent>,
        cancel_events: &mut MessageWriter<CancelGenerationEvent>,
    ) {
        // Only show for generatable categories
        if !Self::is_generatable_category(category) {
            return;
        }
        
        ui.separator();
        
        egui::CollapsingHeader::new("🤖 AI Generation")
            .default_open(true)
            .show(ui, |ui| {
                // Current status
                Self::show_status(ui, generation_status);
                
                ui.add_space(4.0);
                
                // Prompt display/edit
                ui.horizontal(|ui| {
                    ui.label("Prompt:");
                    if ui.small_button("✏️").on_hover_text("Edit prompt").clicked() {
                        self.use_custom_prompt = true;
                        self.custom_prompt = prompt.to_string();
                    }
                });
                
                if self.use_custom_prompt {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.custom_prompt)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY)
                            .hint_text("Describe the asset...")
                    );
                    
                    ui.horizontal(|ui| {
                        if ui.small_button("Reset").clicked() {
                            self.use_custom_prompt = false;
                            self.custom_prompt.clear();
                        }
                    });
                } else if !prompt.is_empty() {
                    ui.label(egui::RichText::new(prompt).italics().weak());
                } else {
                    ui.label(egui::RichText::new("No prompt set").weak());
                }
                
                ui.add_space(4.0);
                
                // Category and detail level
                ui.horizontal(|ui| {
                    ui.label("Category:");
                    ui.label(format!("{:?}", category));
                });
                
                ui.horizontal(|ui| {
                    ui.label("Detail:");
                    egui::ComboBox::from_id_salt("detail_level")
                        .selected_text(format!("{:?}", self.detail_override.unwrap_or(detail_level)))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.detail_override, None, "Default");
                            ui.selectable_value(&mut self.detail_override, Some(DetailLevel::Low), "Low (fast)");
                            ui.selectable_value(&mut self.detail_override, Some(DetailLevel::Medium), "Medium");
                            ui.selectable_value(&mut self.detail_override, Some(DetailLevel::High), "High (slow)");
                        });
                });
                
                // Options
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.generate_lods, "Generate LODs");
                    ui.checkbox(&mut self.generate_textures, "Generate Textures");
                });
                
                ui.add_space(8.0);
                
                // Generate button
                let can_generate = !prompt.is_empty() || !self.custom_prompt.is_empty();
                let is_generating = matches!(generation_status, GenerationStatus::InProgress { .. } | GenerationStatus::Pending);
                
                ui.horizontal(|ui| {
                    if is_generating {
                        if ui.button("🚫 Cancel").clicked() {
                            cancel_events.write(CancelGenerationEvent { entity_id });
                        }
                    } else {
                        let button = egui::Button::new("🤖 Generate Asset");
                        let response = ui.add_enabled(can_generate, button);
                        
                        if response.clicked() {
                            let final_prompt = if self.use_custom_prompt && !self.custom_prompt.is_empty() {
                                self.custom_prompt.clone()
                            } else {
                                prompt.to_string()
                            };
                            
                            let request = GenerationRequest::new(entity_id, &final_prompt, category)
                                .with_detail(self.detail_override.unwrap_or(detail_level))
                                .with_theme(global_theme)
                                .with_priority(5);
                            
                            generation_events.write(RequestGenerationEvent { request });
                        }
                        
                        if !can_generate {
                            response.on_hover_text("Set a prompt first");
                        }
                    }
                    
                    if ui.small_button("⚙️").on_hover_text("AI Settings").clicked() {
                        self.show_settings = true;
                    }
                });
                
                // Show generated asset info if complete
                if let GenerationStatus::Complete { completed_at, generation_time_ms } = generation_status {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("✓ Generated").color(egui::Color32::GREEN));
                        ui.weak(format!("({:.1}s)", *generation_time_ms as f64 / 1000.0));
                    });
                }
            });
    }
    
    /// Show generation status
    fn show_status(ui: &mut egui::Ui, status: &GenerationStatus) {
        match status {
            GenerationStatus::NotRequested => {
                // Don't show anything
            }
            GenerationStatus::Pending => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Queued...");
                });
            }
            GenerationStatus::InProgress { progress, stage } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(stage);
                });
                ui.add(egui::ProgressBar::new(*progress).show_percentage());
            }
            GenerationStatus::Complete { .. } => {
                // Shown separately
            }
            GenerationStatus::Failed { error, .. } => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("✗ Failed").color(egui::Color32::RED));
                });
                ui.label(egui::RichText::new(error).small().weak());
            }
        }
    }
    
    /// Check if category supports AI generation
    fn is_generatable_category(category: NodeCategory) -> bool {
        matches!(
            category,
            NodeCategory::Structure |
            NodeCategory::Prop |
            NodeCategory::Character |
            NodeCategory::Terrain |
            NodeCategory::NPC
        )
    }
    
    /// Show settings window
    pub fn show_settings_window(
        &mut self,
        ctx: &egui::Context,
        config: &mut AIGenerationConfig,
    ) {
        if !self.show_settings {
            return;
        }
        
        egui::Window::new("🤖 AI Generation Settings")
            .open(&mut self.show_settings)
            .resizable(true)
            .default_width(400.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Mesh Backend
                    ui.heading("Mesh Generation");
                    Self::backend_selector(ui, "mesh_backend", &mut config.mesh_backend);
                    
                    ui.add_space(8.0);
                    
                    // Texture Backend
                    ui.heading("Texture Generation");
                    Self::backend_selector(ui, "texture_backend", &mut config.texture_backend);
                    
                    ui.add_space(8.0);
                    
                    // Text Backend
                    ui.heading("Text/Narrative Generation");
                    Self::backend_selector(ui, "text_backend", &mut config.text_backend);
                    
                    ui.add_space(8.0);
                    ui.separator();
                    
                    // General settings
                    ui.heading("General");
                    
                    ui.horizontal(|ui| {
                        ui.label("Timeout (seconds):");
                        ui.add(egui::DragValue::new(&mut config.timeout_secs).range(30..=600));
                    });
                    
                    ui.checkbox(&mut config.auto_lods, "Auto-generate LODs");
                    ui.checkbox(&mut config.cache_results, "Cache generated assets");
                    
                    if config.auto_lods {
                        ui.horizontal(|ui| {
                            ui.label("LOD factors:");
                            for (i, factor) in config.lod_factors.iter_mut().enumerate() {
                                ui.add(egui::DragValue::new(factor)
                                    .range(0.05..=1.0)
                                    .speed(0.01)
                                    .prefix(format!("LOD{}: ", i)));
                            }
                        });
                    }
                });
            });
    }
    
    /// Backend selector widget
    fn backend_selector(ui: &mut egui::Ui, id: &str, backend: &mut AIBackend) {
        let backend_name = match backend {
            AIBackend::None => "None",
            AIBackend::Ollama { .. } => "Ollama (Local LLM)",
            AIBackend::ComfyUI { .. } => "ComfyUI (Stable Diffusion)",
            AIBackend::Meshy { .. } => "Meshy (Cloud 3D)",
            AIBackend::Tripo3D { .. } => "Tripo3D (Cloud 3D)",
            AIBackend::OpenAI { .. } => "OpenAI",
            AIBackend::Custom { .. } => "Custom HTTP",
        };
        
        egui::ComboBox::from_id_salt(id)
            .selected_text(backend_name)
            .show_ui(ui, |ui| {
                if ui.selectable_label(matches!(backend, AIBackend::None), "None").clicked() {
                    *backend = AIBackend::None;
                }
                if ui.selectable_label(matches!(backend, AIBackend::Ollama { .. }), "Ollama").clicked() {
                    *backend = AIBackend::Ollama {
                        endpoint: "http://127.0.0.1:11434".to_string(),
                        model: "llama3.2".to_string(),
                    };
                }
                if ui.selectable_label(matches!(backend, AIBackend::ComfyUI { .. }), "ComfyUI").clicked() {
                    *backend = AIBackend::ComfyUI {
                        endpoint: "http://127.0.0.1:8188".to_string(),
                        workflow: "txt2img".to_string(),
                    };
                }
                if ui.selectable_label(matches!(backend, AIBackend::Meshy { .. }), "Meshy").clicked() {
                    *backend = AIBackend::Meshy { api_key: None };
                }
                if ui.selectable_label(matches!(backend, AIBackend::Tripo3D { .. }), "Tripo3D").clicked() {
                    *backend = AIBackend::Tripo3D { api_key: None };
                }
            });
        
        // Show backend-specific settings
        match backend {
            AIBackend::Ollama { endpoint, model } => {
                ui.horizontal(|ui| {
                    ui.label("Endpoint:");
                    ui.text_edit_singleline(endpoint);
                });
                ui.horizontal(|ui| {
                    ui.label("Model:");
                    ui.text_edit_singleline(model);
                });
            }
            AIBackend::ComfyUI { endpoint, workflow } => {
                ui.horizontal(|ui| {
                    ui.label("Endpoint:");
                    ui.text_edit_singleline(endpoint);
                });
                ui.horizontal(|ui| {
                    ui.label("Workflow:");
                    ui.text_edit_singleline(workflow);
                });
            }
            AIBackend::Meshy { api_key } => {
                ui.horizontal(|ui| {
                    ui.label("API Key:");
                    let mut key = api_key.clone().unwrap_or_default();
                    if ui.add(egui::TextEdit::singleline(&mut key).password(true)).changed() {
                        *api_key = if key.is_empty() { None } else { Some(key) };
                    }
                });
            }
            AIBackend::Tripo3D { api_key } => {
                ui.horizontal(|ui| {
                    ui.label("API Key:");
                    let mut key = api_key.clone().unwrap_or_default();
                    if ui.add(egui::TextEdit::singleline(&mut key).password(true)).changed() {
                        *api_key = if key.is_empty() { None } else { Some(key) };
                    }
                });
            }
            AIBackend::OpenAI { api_key, model } => {
                ui.horizontal(|ui| {
                    ui.label("API Key:");
                    ui.add(egui::TextEdit::singleline(api_key).password(true));
                });
                ui.horizontal(|ui| {
                    ui.label("Model:");
                    ui.text_edit_singleline(model);
                });
            }
            AIBackend::Custom { endpoint, headers: _ } => {
                ui.horizontal(|ui| {
                    ui.label("Endpoint:");
                    ui.text_edit_singleline(endpoint);
                });
            }
            AIBackend::None => {}
        }
        
        // Status indicator
        let available = backend.is_available();
        ui.horizontal(|ui| {
            if available {
                ui.label(egui::RichText::new("✓ Configured").color(egui::Color32::GREEN));
            } else {
                ui.label(egui::RichText::new("✗ Not configured").color(egui::Color32::RED));
            }
        });
    }
}

/// Show generation queue panel
pub fn show_generation_queue(
    ui: &mut egui::Ui,
    service: &AIGenerationService,
    cancel_events: &mut MessageWriter<CancelGenerationEvent>,
) {
    let queue_len = service.queue_len();
    let is_busy = service.is_busy();
    
    if queue_len == 0 && !is_busy {
        ui.weak("No pending generations");
        return;
    }
    
    ui.heading(format!("Generation Queue ({})", queue_len));
    
    // Stats
    ui.horizontal(|ui| {
        ui.label(format!(
            "Completed: {} | Failed: {} | Avg time: {:.1}s",
            service.stats.total_completed,
            service.stats.total_failed,
            service.avg_generation_time_ms() as f64 / 1000.0
        ));
    });
    
    // Current generation
    if is_busy {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Generating...");
        });
    }
}

/// Plugin for AI generation UI
pub struct AIGenerationUIPlugin;

impl Plugin for AIGenerationUIPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AIGenerationPanel>();
    }
}

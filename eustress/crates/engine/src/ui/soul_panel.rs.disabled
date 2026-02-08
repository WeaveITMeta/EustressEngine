//! # Soul Script Panel
//!
//! UI panel for creating, editing, and building Soul scripts (.md files).
//! Integrates with the Explorer to show scripts under their respective services.
//!
//! ## Table of Contents
//! 1. SoulPanelState - Panel state and script management
//! 2. SoulScriptEntry - Individual script entry with source and status
//! 3. Panel rendering - Editor UI with syntax highlighting
//! 4. Build integration - Compile and run Soul scripts

use bevy::prelude::*;
use bevy_egui::egui;
use std::collections::HashMap;
use eustress_common::soul::{
    SoulAST, ScriptService, SoulConfig, SoulParser,
    RequiredInstance, BindingStatus,
};

// ============================================================================
// Soul Panel State
// ============================================================================

/// State for the Soul Script panel
#[derive(Resource, Default)]
pub struct SoulPanelState {
    /// All scripts in the current scene
    pub scripts: HashMap<String, SoulScriptEntry>,
    
    /// Currently selected script (for editing)
    pub selected_script: Option<String>,
    
    /// New script dialog open
    pub show_new_script_dialog: bool,
    
    /// New script name input
    pub new_script_name: String,
    
    /// New script service type
    pub new_script_service: ScriptService,
    
    /// Build in progress
    pub building: bool,
    
    /// Last build error
    pub last_error: Option<String>,
    
    /// Show required instances panel
    pub show_required_instances: bool,
}

/// Individual Soul script entry
#[derive(Clone)]
pub struct SoulScriptEntry {
    /// Script name (without extension)
    pub name: String,
    
    /// Full path to .md file
    pub path: String,
    
    /// Target service (ServerScriptService, ReplicatedStorage, etc.)
    pub service: ScriptService,
    
    /// Script type (Script, LocalScript, ModuleScript)
    pub script_type: ScriptType,
    
    /// Source content (markdown)
    pub source: String,
    
    /// Parsed AST (if valid)
    pub ast: Option<SoulAST>,
    
    /// Build status
    pub status: ScriptBuildStatus,
    
    /// Generated Rust code (if built)
    pub generated_code: Option<String>,
    
    /// Parse/build errors
    pub errors: Vec<String>,
    
    /// Unsaved changes
    pub dirty: bool,
}

/// Script type enum
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ScriptType {
    /// Server-side script (runs on server only)
    #[default]
    Script,
    /// Client-side script (runs on each client)
    LocalScript,
    /// Reusable module (can be required by other scripts)
    ModuleScript,
}

impl ScriptType {
    pub fn icon(&self) -> &'static str {
        match self {
            ScriptType::Script => "📜",
            ScriptType::LocalScript => "📱",
            ScriptType::ModuleScript => "📦",
        }
    }
    
    pub fn name(&self) -> &'static str {
        match self {
            ScriptType::Script => "Script",
            ScriptType::LocalScript => "LocalScript",
            ScriptType::ModuleScript => "ModuleScript",
        }
    }
}

/// Build status for a script
#[derive(Clone, PartialEq, Eq, Default)]
pub enum ScriptBuildStatus {
    /// Not yet built
    #[default]
    NotBuilt,
    /// Currently building
    Building,
    /// Built successfully
    Built,
    /// Build failed
    Failed,
    /// Needs rebuild (source changed)
    Stale,
}

impl ScriptBuildStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            ScriptBuildStatus::NotBuilt => "⚪",
            ScriptBuildStatus::Building => "🔄",
            ScriptBuildStatus::Built => "✅",
            ScriptBuildStatus::Failed => "❌",
            ScriptBuildStatus::Stale => "🔶",
        }
    }
}

impl Default for SoulScriptEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            path: String::new(),
            service: ScriptService::ServerScriptService,
            script_type: ScriptType::Script,
            source: default_script_template(),
            ast: None,
            status: ScriptBuildStatus::NotBuilt,
            generated_code: None,
            errors: Vec::new(),
            dirty: false,
        }
    }
}

/// Default template for new Soul scripts
fn default_script_template() -> String {
    r#"---
name: NewScript
service: ServerScriptService
author: Studio User
---

# NewScript

A new Soul script. Describe what this script does in natural language.

## When the game starts

- Print "Hello from Soul!" to the output

## When a player joins

- Welcome them with a message
- Give them 100 starting gold

"#.to_string()
}

// ============================================================================
// Panel Rendering
// ============================================================================

/// Render the Soul Script panel
pub fn render_soul_panel(
    ui: &mut egui::Ui,
    state: &mut SoulPanelState,
    parser: &mut SoulParser,
    config: &SoulConfig,
) {
    // Header with actions
    ui.horizontal(|ui| {
        ui.heading("📝 Soul Scripts");
        
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Build All button
            if ui.button("🔨 Build All").on_hover_text("Build all scripts (Ctrl+B)").clicked() {
                state.building = true;
                // Build will be handled by system
            }
            
            // New Script button
            if ui.button("➕ New").on_hover_text("Create a new Soul script").clicked() {
                state.show_new_script_dialog = true;
                state.new_script_name = "NewScript".to_string();
            }
        });
    });
    
    ui.separator();
    
    // Script list (left side) and editor (right side)
    egui::SidePanel::left("soul_script_list")
        .resizable(true)
        .default_width(200.0)
        .min_width(150.0)
        .show_inside(ui, |ui| {
            render_script_list(ui, state);
        });
    
    // Main editor area
    let selected = state.selected_script.clone();
    let mut show_required = state.show_required_instances;
    
    egui::CentralPanel::default().show_inside(ui, |ui| {
        if let Some(script_name) = &selected {
            if let Some(script) = state.scripts.get_mut(script_name) {
                render_script_editor(ui, script, parser, config, &mut show_required);
            }
        } else {
            // No script selected
            ui.centered_and_justified(|ui| {
                ui.label("Select a script from the list or create a new one");
            });
        }
    });
    
    state.show_required_instances = show_required;
    
    // New script dialog
    if state.show_new_script_dialog {
        render_new_script_dialog(ui.ctx(), state);
    }
}

/// Render the script list sidebar
fn render_script_list(ui: &mut egui::Ui, state: &mut SoulPanelState) {
    ui.label(egui::RichText::new("Scripts").strong());
    
    egui::ScrollArea::vertical().show(ui, |ui| {
        // Group by service
        let mut by_service: HashMap<ScriptService, Vec<String>> = HashMap::new();
        for (name, script) in &state.scripts {
            by_service.entry(script.service).or_default().push(name.clone());
        }
        
        // Render each service group
        for service in [
            ScriptService::ServerScriptService,
            ScriptService::ReplicatedStorage,
            ScriptService::ReplicatedFirst,
            ScriptService::StarterPlayer,
            ScriptService::StarterGui,
            ScriptService::Workspace,
        ] {
            if let Some(scripts) = by_service.get(&service) {
                if scripts.is_empty() {
                    continue;
                }
                
                egui::CollapsingHeader::new(format!("{}", service_name(&service)))
                    .default_open(true)
                    .show(ui, |ui| {
                        for script_name in scripts {
                            if let Some(script) = state.scripts.get(script_name) {
                                let selected = state.selected_script.as_ref() == Some(script_name);
                                let label = format!(
                                    "{} {} {}{}",
                                    script.status.icon(),
                                    script.script_type.icon(),
                                    script_name,
                                    if script.dirty { " •" } else { "" }
                                );
                                
                                if ui.selectable_label(selected, label).clicked() {
                                    state.selected_script = Some(script_name.clone());
                                }
                            }
                        }
                    });
            }
        }
        
        // Empty state
        if state.scripts.is_empty() {
            ui.weak("No scripts yet. Click '➕ New' to create one.");
        }
    });
}

/// Render the script editor
fn render_script_editor(
    ui: &mut egui::Ui,
    script: &mut SoulScriptEntry,
    parser: &mut SoulParser,
    _config: &SoulConfig,
    show_required: &mut bool,
) {
    // Script header
    ui.horizontal(|ui| {
        ui.label(format!("{} {}", script.script_type.icon(), script.name));
        
        if script.dirty {
            ui.label(egui::RichText::new("(unsaved)").weak());
        }
        
        ui.label(format!("Status: {}", script.status.icon()));
        
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Build this script
            if ui.button("🔨 Build").clicked() {
                // Parse and validate
                match parser.parse(&script.source, &script.path) {
                    Ok(ast) => {
                        script.ast = Some(ast);
                        script.errors.clear();
                        script.status = ScriptBuildStatus::Built;
                    }
                    Err(e) => {
                        script.errors = vec![format!("{:?}", e)];
                        script.status = ScriptBuildStatus::Failed;
                    }
                }
            }
            
            // Save button
            if ui.button("💾 Save").clicked() {
                script.dirty = false;
                // Save will be handled by file system
            }
            
            // Toggle required instances panel
            if ui.toggle_value(show_required, "📍 Required").changed() {
                // Toggle panel
            }
        });
    });
    
    ui.separator();
    
    // Show errors if any
    if !script.errors.is_empty() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("❌ Errors:").color(egui::Color32::RED));
        });
        for error in &script.errors {
            ui.label(egui::RichText::new(format!("  • {}", error)).color(egui::Color32::LIGHT_RED));
        }
        ui.separator();
    }
    
    // Required instances panel (if visible and AST exists)
    if *show_required {
        if let Some(ast) = &script.ast {
            render_required_instances_panel(ui, ast);
            ui.separator();
        }
    }
    
    // Main editor
    egui::ScrollArea::vertical()
        .id_salt("soul_editor_scroll")
        .show(ui, |ui| {
            // Simple code editor (no syntax highlighting for now)
            let response = egui::TextEdit::multiline(&mut script.source)
                .font(egui::TextStyle::Monospace)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(30)
                .show(ui);
            
            if response.response.changed() {
                script.dirty = true;
                script.status = ScriptBuildStatus::Stale;
            }
        });
}

/// Render the required instances panel
fn render_required_instances_panel(ui: &mut egui::Ui, ast: &SoulAST) {
    ui.label(egui::RichText::new("📍 Required Instances").strong());
    
    let status = ast.binding_status();
    
    // Status summary
    ui.horizontal(|ui| {
        if status.ready_to_compile {
            ui.label(egui::RichText::new("✅ All instances bound").color(egui::Color32::GREEN));
        } else {
            ui.label(egui::RichText::new(format!(
                "⚠ {}/{} bound",
                status.bound, status.required_non_optional
            )).color(egui::Color32::YELLOW));
        }
    });
    
    // List required instances
    for required in &ast.required_instances {
        ui.horizontal(|ui| {
            // Check if bound
            let is_bound = ast.bound_instances.iter()
                .any(|b| b.required_name == required.name);
            
            let icon = if is_bound { "✅" } else if required.optional { "⚪" } else { "❌" };
            
            ui.label(format!("{} {}: {}", icon, required.name, required.expected_class));
            
            if !required.description.is_empty() {
                ui.weak(format!("- {}", required.description));
            }
            
            if !is_bound && !required.optional {
                if ui.small_button("Bind...").clicked() {
                    // Open entity picker
                }
            }
        });
    }
    
    if ast.required_instances.is_empty() {
        ui.weak("No required instances declared.");
    }
}

/// Render the new script dialog
fn render_new_script_dialog(ctx: &egui::Context, state: &mut SoulPanelState) {
    egui::Window::new("New Soul Script")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.text_edit_singleline(&mut state.new_script_name);
            });
            
            ui.horizontal(|ui| {
                ui.label("Type:");
                egui::ComboBox::from_id_salt("new_script_type")
                    .selected_text(match state.new_script_service {
                        ScriptService::ServerScriptService => "Script (Server)",
                        ScriptService::ReplicatedFirst => "LocalScript (Client)",
                        ScriptService::ReplicatedStorage => "ModuleScript (Shared)",
                        _ => "Script",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut state.new_script_service,
                            ScriptService::ServerScriptService,
                            "Script (Server)"
                        );
                        ui.selectable_value(
                            &mut state.new_script_service,
                            ScriptService::ReplicatedFirst,
                            "LocalScript (Client)"
                        );
                        ui.selectable_value(
                            &mut state.new_script_service,
                            ScriptService::ReplicatedStorage,
                            "ModuleScript (Shared)"
                        );
                    });
            });
            
            ui.add_space(10.0);
            
            ui.horizontal(|ui| {
                if ui.button("Create").clicked() {
                    // Create the new script
                    let script_type = match state.new_script_service {
                        ScriptService::ServerScriptService => ScriptType::Script,
                        ScriptService::ReplicatedFirst => ScriptType::LocalScript,
                        ScriptService::ReplicatedStorage => ScriptType::ModuleScript,
                        _ => ScriptType::Script,
                    };
                    
                    let mut source = default_script_template();
                    source = source.replace("NewScript", &state.new_script_name);
                    source = source.replace(
                        "service: ServerScriptService",
                        &format!("service: {:?}", state.new_script_service)
                    );
                    
                    let entry = SoulScriptEntry {
                        name: state.new_script_name.clone(),
                        path: format!("scripts/{}.md", state.new_script_name),
                        service: state.new_script_service,
                        script_type,
                        source,
                        dirty: true,
                        ..Default::default()
                    };
                    
                    state.scripts.insert(state.new_script_name.clone(), entry);
                    state.selected_script = Some(state.new_script_name.clone());
                    state.show_new_script_dialog = false;
                }
                
                if ui.button("Cancel").clicked() {
                    state.show_new_script_dialog = false;
                }
            });
        });
}

/// Get display name for a service
fn service_name(service: &ScriptService) -> &'static str {
    match service {
        ScriptService::ServerScriptService => "📜 ServerScriptService",
        ScriptService::ReplicatedStorage => "📦 ReplicatedStorage",
        ScriptService::ReplicatedFirst => "⚡ ReplicatedFirst",
        ScriptService::StarterPlayer => "🏃 StarterPlayer",
        ScriptService::StarterGui => "🖼 StarterGui",
        ScriptService::Workspace => "🌍 Workspace",
        ScriptService::Lighting => "💡 Lighting",
        ScriptService::Players => "👥 Players",
        ScriptService::SoundService => "🔊 SoundService",
        ScriptService::Chat => "💬 Chat",
        ScriptService::Teams => "🏁 Teams",
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Soul Panel plugin
pub struct SoulPanelPlugin;

impl Plugin for SoulPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoulPanelState>();
    }
}

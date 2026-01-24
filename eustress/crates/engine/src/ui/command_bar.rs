#![allow(dead_code)]

use bevy::prelude::*;
use bevy_egui::egui;
use super::BevySelectionManager;
use crate::classes::{Instance, BasePart, Part, PartType as ClassPartType, ClassName, Material};
use crate::generative_pipeline::{
    GenerativeMode, GenerativePipelineConfig, GenerativePipelineState,
    ConfirmAbstractEvent, AbstractTag, AbstractStatus,
};
// Project Korah imports - conditionally compiled to avoid circular deps
#[cfg(feature = "korah")]
use crate::korah::{BuildPhase, LiveSceneContext, ScreenshotState};

// Fallback types when korah feature is not enabled
#[cfg(not(feature = "korah"))]
mod korah_fallback {
    use bevy::prelude::*;
    
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Resource)]
    pub enum BuildPhase {
        #[default]
        Foundation,
        Structure,
        Objects,
        Detail,
    }
    
    impl BuildPhase {
        pub fn display_name(&self) -> &'static str {
            match self {
                BuildPhase::Foundation => "Foundation",
                BuildPhase::Structure => "Structure",
                BuildPhase::Objects => "Objects",
                BuildPhase::Detail => "Detail",
            }
        }
        
        pub fn icon(&self) -> &'static str {
            match self {
                BuildPhase::Foundation => "🏗️",
                BuildPhase::Structure => "🧱",
                BuildPhase::Objects => "🪑",
                BuildPhase::Detail => "✨",
            }
        }
        
        pub fn next(&self) -> Self {
            match self {
                BuildPhase::Foundation => BuildPhase::Structure,
                BuildPhase::Structure => BuildPhase::Objects,
                BuildPhase::Objects => BuildPhase::Detail,
                BuildPhase::Detail => BuildPhase::Detail,
            }
        }
        
        pub fn prompt_context(&self) -> &'static str { "" }
    }
    
    #[derive(Resource, Default)]
    pub struct LiveSceneContext;
    
    impl LiveSceneContext {
        pub fn to_toon(&self) -> String { String::new() }
        pub fn to_toon_region(&self, _min: Vec3, _max: Vec3) -> String { String::new() }
    }
    
    #[derive(Resource, Default)]
    pub struct ScreenshotState {
        pub pending: bool,
    }
    
    impl ScreenshotState {
        pub fn request_capture(&mut self) { self.pending = true; }
        pub fn get_capture(&self) -> Option<&String> { None }
    }
}

#[cfg(not(feature = "korah"))]
use korah_fallback::{BuildPhase, LiveSceneContext, ScreenshotState};

/// A history entry that stores both the English input and the generated Rune script
/// This allows re-executing cached scripts without calling Claude API again
#[derive(Clone, Debug)]
pub struct CommandHistoryEntry {
    /// The original English input from the user
    pub english_input: String,
    /// The generated Rune script from Claude (None if not yet built or was a simple command)
    pub rune_script: Option<String>,
    /// Whether this was an AI build command (vs simple command like "create cube")
    pub is_ai_build: bool,
}

/// Command bar state and history
#[derive(Resource)]
pub struct CommandBarState {
    pub show: bool,
    pub input: String,
    /// History entries with cached Rune scripts for re-execution
    pub history: Vec<CommandHistoryEntry>,
    pub history_index: Option<usize>,
    /// The original input when navigating history (to detect modifications)
    pub history_original_input: Option<String>,
    pub autocomplete_visible: bool,
    pub autocomplete_suggestions: Vec<String>,
    pub last_result: Option<CommandResult>,
    /// Pending command to be sent to Claude API
    pub pending_command: Option<String>,
    /// Whether a build is in progress
    pub building: bool,
    /// Status message shown during build
    pub status: String,
    
    // === Project Korah Fields ===
    /// Captured screenshot (base64 PNG data URL)
    pub captured_screenshot: Option<String>,
    /// Attached reference image (base64 PNG data URL)
    pub attached_image: Option<String>,
    /// Selected region bounds (min, max) for context filtering
    pub region_bounds: Option<(Vec3, Vec3)>,
    /// Current build phase
    pub build_phase: BuildPhase,
    /// Whether to include screenshot in next prompt
    pub include_screenshot: bool,
    /// Whether to include reference image in next prompt
    pub include_reference: bool,
    
    // === VIGA Fields ===
    /// VIGA reference image for inverse graphics (base64 PNG data URL)
    pub viga_reference_image: Option<String>,
    /// Whether VIGA mode is active (image-to-scene generation)
    pub viga_mode: bool,
    /// VIGA processing status message
    pub viga_status: Option<String>,
    /// Whether drag-and-drop is being hovered
    pub drag_hover: bool,
}

impl Default for CommandBarState {
    fn default() -> Self {
        Self {
            show: true,
            input: String::new(),
            history: Vec::new(),
            history_index: None,
            history_original_input: None,
            autocomplete_visible: false,
            autocomplete_suggestions: Vec::new(),
            last_result: None,
            pending_command: None,
            building: false,
            status: String::new(),
            // Project Korah defaults
            captured_screenshot: None,
            attached_image: None,
            region_bounds: None,
            build_phase: BuildPhase::default(),
            include_screenshot: false,
            include_reference: false,
            // VIGA defaults
            viga_reference_image: None,
            viga_mode: false,
            viga_status: None,
            drag_hover: false,
        }
    }
}

#[derive(Clone)]
pub enum CommandResult {
    Success(String),
    Error(String),
}

/// Command bar panel at the bottom
pub struct CommandBarPanel;

impl CommandBarPanel {
    pub fn show(
        ctx: &egui::Context,
        state: &mut CommandBarState,
        selection_manager: &BevySelectionManager,
        world: &mut World,
    ) {
        if !state.show {
            return;
        }

        egui::TopBottomPanel::bottom("command_bar")
            .min_height(40.0)
            .max_height(150.0)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // Header with toggle and Korah controls
                    ui.horizontal(|ui| {
                        ui.label("💻 Command Bar");
                        
                        ui.separator();
                        
                        // === Project Korah Buttons ===
                        // Screenshot capture button
                        let screenshot_label = if state.include_screenshot { "📷 ✓" } else { "📷" };
                        if ui.button(screenshot_label).on_hover_text("Capture screenshot for AI context").clicked() {
                            if let Some(mut screenshot_state) = world.get_resource_mut::<ScreenshotState>() {
                                screenshot_state.request_capture();
                                state.status = "Capturing screenshot...".to_string();
                            }
                        }
                        
                        // Check if screenshot is ready
                        if let Some(screenshot_state) = world.get_resource::<ScreenshotState>() {
                            if let Some(capture) = screenshot_state.get_capture() {
                                if state.captured_screenshot.is_none() {
                                    state.captured_screenshot = Some(capture.clone());
                                    state.include_screenshot = true;
                                    state.status = "Screenshot captured!".to_string();
                                }
                            }
                        }
                        
                        // Attach reference image button
                        let attach_label = if state.include_reference { "📎 ✓" } else { "📎" };
                        if ui.button(attach_label).on_hover_text("Attach reference image").clicked() {
                            // Toggle reference inclusion if already attached
                            if state.attached_image.is_some() {
                                state.include_reference = !state.include_reference;
                            } else {
                                // TODO: Open file picker dialog
                                state.status = "File picker not yet implemented".to_string();
                            }
                        }
                        
                        // Region selector button
                        let region_label = if state.region_bounds.is_some() { "🌍 ✓" } else { "🌍" };
                        if ui.button(region_label).on_hover_text("Set region filter (select a Part or use 'region' command)").clicked() {
                            // Try to get region from selected part
                            let selected = selection_manager.0.read().get_selected();
                            if let Some(first_selected) = selected.first() {
                                if let Some(entity) = parse_entity_from_debug(first_selected) {
                                    if let Some(base_part) = world.get::<BasePart>(entity) {
                                        if let Some(transform) = world.get::<Transform>(entity) {
                                            let half_size = base_part.size * 0.5;
                                            let min = transform.translation - half_size;
                                            let max = transform.translation + half_size;
                                            state.region_bounds = Some((min, max));
                                            state.status = format!("Region set from selected part: ({:.1}, {:.1}, {:.1}) to ({:.1}, {:.1}, {:.1})", 
                                                min.x, min.y, min.z, max.x, max.y, max.z);
                                        }
                                    }
                                }
                            } else if state.region_bounds.is_some() {
                                // Clear region if clicking again with no selection
                                state.region_bounds = None;
                                state.status = "Region filter cleared".to_string();
                            } else {
                                state.status = "Select a Part to use as region, or use 'region x1,y1,z1 x2,y2,z2'".to_string();
                            }
                        }
                        
                        // Build phase indicator/selector
                        ui.separator();
                        let phase_label = format!("{} {}", state.build_phase.icon(), state.build_phase.display_name());
                        if ui.button(&phase_label).on_hover_text("Click to advance build phase").clicked() {
                            state.build_phase = state.build_phase.next();
                            state.status = format!("Build phase: {}", state.build_phase.display_name());
                        }
                        
                        // === Generative Mode Selection Bubble ===
                        ui.separator();
                        if let Some(mut config) = world.get_resource_mut::<GenerativePipelineConfig>() {
                            let mode_label = format!("{} {}", config.mode.icon(), config.mode.display_name());
                            let mode_response = ui.add(
                                egui::Button::new(egui::RichText::new(&mode_label).strong())
                                    .fill(match config.mode {
                                        GenerativeMode::Soul => egui::Color32::from_rgb(75, 0, 130),    // Purple for Soul
                                        GenerativeMode::Abstract => egui::Color32::from_rgb(0, 100, 150), // Teal for Abstract
                                    })
                            );
                            if mode_response.clicked() {
                                config.mode = config.mode.toggle();
                                state.status = format!("Generative mode: {} - {}", 
                                    config.mode.display_name(), 
                                    config.mode.description());
                            }
                            mode_response.on_hover_text(format!(
                                "Current: {}\nClick to switch to {}\n\n{}",
                                config.mode.display_name(),
                                config.mode.toggle().display_name(),
                                config.mode.description()
                            ));
                        }
                        
                        // === Abstract Confirmation Button ===
                        let awaiting_count = world.get_resource::<GenerativePipelineState>()
                            .map(|s| s.awaiting_confirmation.len())
                            .unwrap_or(0);
                        
                        if awaiting_count > 0 {
                            ui.separator();
                            let confirm_label = format!("✓ Confirm ({})", awaiting_count);
                            if ui.button(egui::RichText::new(&confirm_label).color(egui::Color32::GREEN))
                                .on_hover_text("Confirm and finalize all generated Abstract parts.\nThis replaces placeholders with the generated content.")
                                .clicked() 
                            {
                                world.write_message(ConfirmAbstractEvent { entities: vec![] });
                                state.status = format!("Confirming {} Abstract part(s)...", awaiting_count);
                            }
                        }
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").clicked() {
                                state.show = false;
                            }
                            ui.label("Ctrl+K to toggle");
                        });
                    });
                    
                    // Status bar (if there's a status message)
                    if !state.status.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&state.status).small().color(egui::Color32::GRAY));
                            if ui.small_button("×").clicked() {
                                state.status.clear();
                            }
                        });
                    }
                    
                    ui.separator();
                    
                    // Main input area
                    ui.horizontal(|ui| {
                        ui.label(">");
                        
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut state.input)
                                .font(egui::TextStyle::Monospace)
                                .hint_text("Describe what to build... (AI-powered with Korah)")
                                .desired_width(f32::INFINITY)
                        );
                        
                        // Handle Enter key
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            Self::execute_command(state, selection_manager, world);
                            response.request_focus();
                        }
                        
                        // Handle up/down for history
                        if response.has_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                                Self::history_up(state);
                            }
                            if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                                Self::history_down(state);
                            }
                        }
                        
                        // Execute button
                        if ui.button("Execute").clicked() {
                            Self::execute_command(state, selection_manager, world);
                        }
                        
                        // AI Build button (sends to Claude with context)
                        if ui.button("🏛️ Build").on_hover_text("Send to Claude AI with scene context").clicked() {
                            Self::execute_korah_build(state, world);
                        }
                        
                        // === Generate Button (Image-to-Scene) ===
                        let generate_label = if state.viga_reference_image.is_some() { "✨ Generate ✓" } else { "✨ Generate" };
                        let generate_color = if state.viga_mode {
                            egui::Color32::from_rgb(138, 43, 226) // Purple when active
                        } else {
                            egui::Color32::from_rgb(100, 100, 100)
                        };
                        if ui.add(egui::Button::new(egui::RichText::new(generate_label).color(generate_color)))
                            .on_hover_text("Generate 3D scene from image\nDrag & drop an image or click to upload\nAI will recreate the scene in 3D")
                            .clicked()
                        {
                            if state.viga_reference_image.is_some() {
                                // Start generation
                                Self::start_viga_generation(state, world);
                            } else {
                                // Open file picker for image
                                Self::open_viga_file_picker(state);
                            }
                        }
                        
                        // Clear reference image button (if image attached)
                        if state.viga_reference_image.is_some() {
                            if ui.small_button("×").on_hover_text("Clear reference image").clicked() {
                                state.viga_reference_image = None;
                                state.viga_mode = false;
                                state.status = "Reference image cleared".to_string();
                            }
                        }
                    });
                    
                    // Autocomplete suggestions (if typing)
                    if !state.input.is_empty() {
                        Self::update_autocomplete(state);
                        if !state.autocomplete_suggestions.is_empty() {
                            ui.separator();
                            ui.label("Suggestions:");
                            egui::ScrollArea::vertical().max_height(60.0).show(ui, |ui| {
                                for suggestion in &state.autocomplete_suggestions.clone() {
                                    if ui.selectable_label(false, suggestion).clicked() {
                                        state.input = suggestion.clone();
                                    }
                                }
                            });
                        }
                    }
                    
                    // Show last result
                    if let Some(result) = &state.last_result {
                        ui.separator();
                        match result {
                            CommandResult::Success(msg) => {
                                ui.colored_label(egui::Color32::GREEN, format!("✓ {}", msg));
                            }
                            CommandResult::Error(msg) => {
                                ui.colored_label(egui::Color32::RED, format!("✗ {}", msg));
                            }
                        }
                    }
                });
            });
    }
    
    fn execute_command(
        state: &mut CommandBarState,
        selection_manager: &BevySelectionManager,
        world: &mut World,
    ) {
        let cmd = state.input.trim();
        
        if cmd.is_empty() {
            return;
        }
        
        // Add to history (simple commands don't have Rune scripts)
        state.history.push(CommandHistoryEntry {
            english_input: cmd.to_string(),
            rune_script: None,
            is_ai_build: false,
        });
        state.history_index = None;
        state.history_original_input = None;
        
        // Parse and execute
        let result = Self::parse_and_execute(cmd, selection_manager, world);
        state.last_result = Some(result);
        
        // Clear input
        state.input.clear();
    }
    
    fn parse_and_execute(
        cmd: &str,
        selection_manager: &BevySelectionManager,
        world: &mut World,
    ) -> CommandResult {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        
        if parts.is_empty() {
            return CommandResult::Error("Empty command".to_string());
        }
        
        match parts[0].to_lowercase().as_str() {
            "help" => {
                CommandResult::Success(
                    "Commands: create [cube|ball|cylinder], delete, select [all|none], clear, list".to_string()
                )
            }
            
            "create" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: create <type>".to_string());
                }
                
                let part_type = match parts[1].to_lowercase().as_str() {
                    "cube" | "block" => ClassPartType::Block,
                    "sphere" | "ball" => ClassPartType::Ball,
                    "cylinder" => ClassPartType::Cylinder,
                    "wedge" => ClassPartType::Wedge,
                    _ => return CommandResult::Error(format!("Unknown part type: {}", parts[1])),
                };
                
                // Create entity with class components
                let instance = Instance {
                    name: format!("{}", parts[1]),
                    class_name: ClassName::Part,
                    archivable: true,
                    id: 0, // Will be assigned by ECS
                    ai: false, // Not opted into AI training by default
                };
                
                let base_part = BasePart {
                    cframe: Transform::from_xyz(0.0, 2.0, 0.0),
                    size: Vec3::new(4.0, 1.0, 2.0),
                    color: Color::srgba(0.6, 0.6, 0.6, 1.0),
                    material: Material::Plastic,
                    ..Default::default()
                };
                
                let part = Part {
                    shape: part_type,
                };
                
                // Spawn entity
                let entity = world.spawn((instance, base_part, part, Name::new(parts[1].to_string()))).id();
                
                CommandResult::Success(format!("Created {} (Entity: {:?})", parts[1], entity))
            }
            
            "delete" => {
                let selected = selection_manager.0.read().get_selected();
                if !selected.is_empty() {
                    let mut deleted_count = 0;
                    
                    for id_str in &selected {
                        // Parse Entity from debug format "Entity(XvY)"
                        if let Some(entity) = parse_entity_from_debug(id_str) {
                            if world.get_entity(entity).is_ok() {
                                world.despawn(entity);
                                deleted_count += 1;
                            }
                        }
                    }
                    
                    selection_manager.0.write().clear();
                    CommandResult::Success(format!("Deleted {} entity(ies)", deleted_count))
                } else {
                    CommandResult::Error("No entity selected".to_string())
                }
            }
            
            "select" => {
                if parts.len() < 2 {
                    return CommandResult::Error("Usage: select <all|none>".to_string());
                }
                
                match parts[1].to_lowercase().as_str() {
                    "all" => {
                        // Select all entities with Instance
                        let mut count = 0;
                        let sm = selection_manager.0.write();
                        sm.clear();
                        
                        let mut query = world.query::<(Entity, &Instance)>();
                        for (entity, _) in query.iter(world) {
                            sm.select(format!("{:?}", entity));
                            count += 1;
                        }
                        
                        CommandResult::Success(format!("Selected {} entities", count))
                    }
                    "none" => {
                        selection_manager.0.write().clear();
                        CommandResult::Success("Cleared selection".to_string())
                    }
                    _ => {
                        CommandResult::Error("Usage: select <all|none>".to_string())
                    }
                }
            }
            
            "clear" => {
                // Delete all entities with Instance
                let mut to_delete = Vec::new();
                let mut query = world.query::<(Entity, &Instance)>();
                for (entity, _) in query.iter(world) {
                    to_delete.push(entity);
                }
                
                let count = to_delete.len();
                for entity in to_delete {
                    world.despawn(entity);
                }
                
                CommandResult::Success(format!("Cleared {} entities", count))
            }
            
            "list" => {
                let mut query = world.query::<(Entity, &Instance)>();
                let entities: Vec<_> = query.iter(world).collect();
                
                if entities.is_empty() {
                    CommandResult::Success("No entities in scene".to_string())
                } else {
                    let count = entities.len();
                    let names: Vec<String> = entities.iter()
                        .take(5)
                        .map(|(entity, instance)| format!("{} ({:?})", instance.name, entity))
                        .collect();
                    CommandResult::Success(
                        format!("{} entities: {}{}", count, names.join(", "),
                            if count > 5 { "..." } else { "" })
                    )
                }
            }
            
            // Soul scripting commands
            "soul" | "script" => {
                if parts.len() < 2 {
                    return CommandResult::Error(
                        "Usage: soul <build|run|validate|status> [script_name]".to_string()
                    );
                }
                
                match parts[1].to_lowercase().as_str() {
                    "build" => {
                        let script_name = parts.get(2).map(|s| s.to_string());
                        // Send build request event
                        world.write_message(crate::soul::BuildRequestEvent {
                            scene: "current".to_string(),
                            script: script_name.clone(),
                            force: false,
                        });
                        CommandResult::Success(format!(
                            "Build requested for {}",
                            script_name.unwrap_or_else(|| "all scripts".to_string())
                        ))
                    }
                    "rebuild" => {
                        let script_name = parts.get(2).map(|s| s.to_string());
                        world.write_message(crate::soul::BuildRequestEvent {
                            scene: "current".to_string(),
                            script: script_name.clone(),
                            force: true, // Force rebuild
                        });
                        CommandResult::Success(format!(
                            "Forced rebuild requested for {}",
                            script_name.unwrap_or_else(|| "all scripts".to_string())
                        ))
                    }
                    "status" => {
                        // Check build status from SoulBuilder resource
                        if let Some(builder) = world.get_resource::<crate::soul::SoulBuilder>() {
                            let status = builder.status();
                            CommandResult::Success(format!("Soul build status: {}", status))
                        } else {
                            CommandResult::Error("Soul builder not initialized".to_string())
                        }
                    }
                    "validate" => {
                        let script_name = parts.get(2).map(|s| s.to_string());
                        CommandResult::Success(format!(
                            "Validation requested for {} (not yet implemented)",
                            script_name.unwrap_or_else(|| "all scripts".to_string())
                        ))
                    }
                    "help" => {
                        CommandResult::Success(
                            "Soul commands:\n\
                             - soul build [name]    Build script(s)\n\
                             - soul rebuild [name]  Force rebuild\n\
                             - soul status          Show build status\n\
                             - soul validate [name] Validate script(s)".to_string()
                        )
                    }
                    _ => CommandResult::Error(format!(
                        "Unknown soul command: {}. Try 'soul help'", parts[1]
                    )),
                }
            }
            
            // Generative mode commands
            "mode" | "gen" | "generative" => {
                if parts.len() < 2 {
                    // Show current mode
                    if let Some(config) = world.get_resource::<GenerativePipelineConfig>() {
                        return CommandResult::Success(format!(
                            "Current mode: {} {}\n{}\n\nUsage: mode <soul|abstract>",
                            config.mode.icon(),
                            config.mode.display_name(),
                            config.mode.description()
                        ));
                    }
                    return CommandResult::Error("Generative pipeline not initialized".to_string());
                }
                
                match parts[1].to_lowercase().as_str() {
                    "soul" => {
                        if let Some(mut config) = world.get_resource_mut::<GenerativePipelineConfig>() {
                            config.mode = GenerativeMode::Soul;
                            CommandResult::Success("🔮 Switched to Soul mode (Claude → Rune scripts)".to_string())
                        } else {
                            CommandResult::Error("Generative pipeline not initialized".to_string())
                        }
                    }
                    "abstract" => {
                        if let Some(mut config) = world.get_resource_mut::<GenerativePipelineConfig>() {
                            config.mode = GenerativeMode::Abstract;
                            CommandResult::Success("🎨 Switched to Abstract mode (Flux.1 → Mesh/Texture)".to_string())
                        } else {
                            CommandResult::Error("Generative pipeline not initialized".to_string())
                        }
                    }
                    "toggle" => {
                        if let Some(mut config) = world.get_resource_mut::<GenerativePipelineConfig>() {
                            config.mode = config.mode.toggle();
                            CommandResult::Success(format!(
                                "Switched to {} {} mode",
                                config.mode.icon(),
                                config.mode.display_name()
                            ))
                        } else {
                            CommandResult::Error("Generative pipeline not initialized".to_string())
                        }
                    }
                    _ => CommandResult::Error("Usage: mode <soul|abstract|toggle>".to_string()),
                }
            }
            
            // Abstract confirmation command
            "confirm" => {
                let awaiting = world.get_resource::<GenerativePipelineState>()
                    .map(|s| s.awaiting_confirmation.len())
                    .unwrap_or(0);
                
                if awaiting == 0 {
                    return CommandResult::Success("No Abstract parts awaiting confirmation".to_string());
                }
                
                world.write_message(ConfirmAbstractEvent { entities: vec![] });
                CommandResult::Success(format!("✓ Confirming {} Abstract part(s)...", awaiting))
            }
            
            // Abstract tag command (add/remove Abstract tag from selection)
            "abstract" => {
                if parts.len() < 2 {
                    return CommandResult::Error(
                        "Usage: abstract <add|remove|list> [prompt]\n\
                         Examples:\n\
                         - abstract add a red sports car\n\
                         - abstract remove\n\
                         - abstract list".to_string()
                    );
                }
                
                match parts[1].to_lowercase().as_str() {
                    "add" => {
                        let prompt = if parts.len() > 2 {
                            parts[2..].join(" ")
                        } else {
                            return CommandResult::Error("Usage: abstract add <prompt>".to_string());
                        };
                        
                        let selected = selection_manager.0.read().get_selected();
                        if selected.is_empty() {
                            return CommandResult::Error("Select an entity first".to_string());
                        }
                        
                        let mut count = 0;
                        for id_str in &selected {
                            if let Some(entity) = parse_entity_from_debug(id_str) {
                                if world.get_entity(entity).is_ok() {
                                    world.entity_mut(entity).insert(AbstractTag::new(prompt.clone()));
                                    count += 1;
                                }
                            }
                        }
                        
                        CommandResult::Success(format!(
                            "Added Abstract tag to {} entity(ies) with prompt: \"{}\"",
                            count, prompt
                        ))
                    }
                    "remove" => {
                        let selected = selection_manager.0.read().get_selected();
                        if selected.is_empty() {
                            return CommandResult::Error("Select an entity first".to_string());
                        }
                        
                        let mut count = 0;
                        for id_str in &selected {
                            if let Some(entity) = parse_entity_from_debug(id_str) {
                                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                                    entity_mut.remove::<AbstractTag>();
                                    count += 1;
                                }
                            }
                        }
                        
                        CommandResult::Success(format!("Removed Abstract tag from {} entity(ies)", count))
                    }
                    "list" => {
                        let mut abstracts = Vec::new();
                        let mut query = world.query::<(Entity, &AbstractTag)>();
                        for (entity, tag) in query.iter(world) {
                            let status = match &tag.status {
                                AbstractStatus::Pending => "⏳",
                                AbstractStatus::Generating => "🔄",
                                AbstractStatus::Complete => "✅",
                                AbstractStatus::Failed(_) => "❌",
                                AbstractStatus::Confirmed => "✓",
                            };
                            abstracts.push(format!(
                                "{} {:?}: \"{}\"",
                                status, entity, 
                                if tag.prompt.len() > 30 { &tag.prompt[..30] } else { &tag.prompt }
                            ));
                        }
                        
                        if abstracts.is_empty() {
                            CommandResult::Success("No Abstract parts in scene".to_string())
                        } else {
                            CommandResult::Success(format!(
                                "{} Abstract part(s):\n{}",
                                abstracts.len(),
                                abstracts.join("\n")
                            ))
                        }
                    }
                    _ => CommandResult::Error("Usage: abstract <add|remove|list> [prompt]".to_string()),
                }
            }
            
            // Print/echo command for debugging
            "print" | "echo" => {
                if parts.len() < 2 {
                    CommandResult::Success("".to_string())
                } else {
                    CommandResult::Success(parts[1..].join(" "))
                }
            }
            
            // Set property command
            "set" => {
                if parts.len() < 3 {
                    return CommandResult::Error(
                        "Usage: set <property> <value> (e.g., 'set snap 0.5')".to_string()
                    );
                }
                
                match parts[1].to_lowercase().as_str() {
                    "snap" => {
                        if let Ok(size) = parts[2].parse::<f32>() {
                            if let Some(mut settings) = world.get_resource_mut::<crate::editor_settings::EditorSettings>() {
                                settings.snap_size = size.max(0.1);
                                settings.snap_enabled = size > 0.0;
                                CommandResult::Success(format!("Snap size set to {}", settings.snap_size))
                            } else {
                                CommandResult::Error("Editor settings not found".to_string())
                            }
                        } else {
                            CommandResult::Error("Invalid snap size".to_string())
                        }
                    }
                    "grid" => {
                        if let Some(mut settings) = world.get_resource_mut::<crate::editor_settings::EditorSettings>() {
                            settings.show_grid = parts[2].to_lowercase() == "on" || parts[2] == "1" || parts[2].to_lowercase() == "true";
                            CommandResult::Success(format!("Grid {}", if settings.show_grid { "enabled" } else { "disabled" }))
                        } else {
                            CommandResult::Error("Editor settings not found".to_string())
                        }
                    }
                    _ => CommandResult::Error(format!("Unknown property: {}", parts[1])),
                }
            }
            
            _ => CommandResult::Error(format!("Unknown command: {}. Type 'help' for available commands.", parts[0])),
        }
    }
    
    fn update_autocomplete(state: &mut CommandBarState) {
        let input = state.input.to_lowercase();
        let commands = vec![
            "help",
            "create cube",
            "create ball", 
            "create cylinder",
            "create wedge",
            "delete",
            "select all",
            "select none",
            "clear",
            "list",
            // Soul commands
            "soul build",
            "soul rebuild",
            "soul status",
            "soul validate",
            "soul help",
            // Settings commands
            "set snap 0.5",
            "set snap 1.0",
            "set grid on",
            "set grid off",
            // Generative pipeline commands
            "mode",
            "mode soul",
            "mode abstract",
            "mode toggle",
            "confirm",
            "abstract add",
            "abstract remove",
            "abstract list",
            // Debug commands
            "print",
            "echo",
        ];
        
        state.autocomplete_suggestions = commands
            .into_iter()
            .filter(|cmd| cmd.starts_with(&input))
            .map(|s| s.to_string())
            .collect();
    }
    
    fn history_up(state: &mut CommandBarState) {
        if state.history.is_empty() {
            return;
        }
        
        match state.history_index {
            None => {
                state.history_index = Some(state.history.len() - 1);
                let entry = state.history.last().unwrap();
                state.input = entry.english_input.clone();
                state.history_original_input = Some(entry.english_input.clone());
            }
            Some(idx) if idx > 0 => {
                state.history_index = Some(idx - 1);
                let entry = &state.history[idx - 1];
                state.input = entry.english_input.clone();
                state.history_original_input = Some(entry.english_input.clone());
            }
            _ => {}
        }
    }
    
    fn history_down(state: &mut CommandBarState) {
        if let Some(idx) = state.history_index {
            if idx < state.history.len() - 1 {
                state.history_index = Some(idx + 1);
                let entry = &state.history[idx + 1];
                state.input = entry.english_input.clone();
                state.history_original_input = Some(entry.english_input.clone());
            } else {
                state.history_index = None;
                state.history_original_input = None;
                state.input.clear();
            }
        }
    }
    
    /// Execute a Korah AI build request
    /// 
    /// If navigating history and input is unchanged, re-execute the cached Rune script.
    /// If input is modified or new, send to Claude API for a fresh build.
    fn execute_korah_build(state: &mut CommandBarState, world: &mut World) {
        let prompt = state.input.trim();
        if prompt.is_empty() {
            state.last_result = Some(CommandResult::Error("Enter a description of what to build".to_string()));
            return;
        }
        
        // Check if we're re-executing a cached command from history
        let use_cached_script = if let Some(idx) = state.history_index {
            if let Some(entry) = state.history.get(idx) {
                // Check if input is unchanged from the original history entry
                if let Some(ref original) = state.history_original_input {
                    if prompt == original && entry.rune_script.is_some() && entry.is_ai_build {
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        
        if use_cached_script {
            // Re-execute cached Rune script without calling Claude API
            let idx = state.history_index.unwrap();
            let entry = &state.history[idx];
            let rune_script = entry.rune_script.clone().unwrap();
            
            info!("🔄 Re-executing cached Rune script for: {}", prompt);
            info!("  Script: {} chars", rune_script.len());
            
            // Execute the cached Rune script directly
            // Send as a special "cached" build that skips Claude API
            state.pending_command = None; // Don't send to Claude
            state.status = format!("🔄 Re-running: {}...", 
                if prompt.len() > 30 { &prompt[..30] } else { prompt });
            
            // Queue the cached script for execution via the build pipeline
            world.write_message(crate::soul::CommandBarBuildEvent {
                command: rune_script.clone(),
                use_cached: true,
            });
            
            state.last_result = Some(CommandResult::Success(
                "Re-executing cached script (no API call)".to_string()
            ));
            
            // Reset history navigation state
            state.history_index = None;
            state.history_original_input = None;
            state.input.clear();
            return;
        }
        
        // Input is new or modified - need to call Claude API
        
        // Get scene context in TOON format
        let scene_context = if let Some(live_context) = world.get_resource::<LiveSceneContext>() {
            match &state.region_bounds {
                Some((min, max)) => live_context.to_toon_region(*min, *max),
                None => live_context.to_toon(),
            }
        } else {
            "Scene:Empty\nIndex:0\nDetails:0/0\n".to_string()
        };
        
        // Get build phase context
        let phase_context = state.build_phase.prompt_context();
        
        // Prepare screenshot (if enabled)
        let screenshot = if state.include_screenshot {
            state.captured_screenshot.as_deref()
        } else {
            None
        };
        
        // Prepare reference image (if enabled)
        let reference = if state.include_reference {
            state.attached_image.as_deref()
        } else {
            None
        };
        
        // Log what we're sending
        info!("🏛️ Korah Build Request:");
        info!("  Prompt: {}", prompt);
        info!("  Phase: {}", state.build_phase.display_name());
        info!("  Scene entities: {} chars", scene_context.len());
        info!("  Screenshot: {}", screenshot.is_some());
        info!("  Reference: {}", reference.is_some());
        if let Some((min, max)) = &state.region_bounds {
            info!("  Region: ({:.1},{:.1},{:.1}) to ({:.1},{:.1},{:.1})", 
                min.x, min.y, min.z, max.x, max.y, max.z);
        }
        
        // Store the pending command for async processing
        // The actual Claude API call will be handled by the build pipeline
        state.pending_command = Some(prompt.to_string());
        state.building = true;
        state.status = format!("🏛️ Building: {}...", 
            if prompt.len() > 30 { &prompt[..30] } else { prompt });
        
        // Add to history as a new AI build entry (script will be filled in when build completes)
        state.history.push(CommandHistoryEntry {
            english_input: prompt.to_string(),
            rune_script: None, // Will be populated when Claude responds
            is_ai_build: true,
        });
        state.history_index = None;
        state.history_original_input = None;
        state.input.clear();
        
        // Clear screenshot after use (optional - could keep for iteration)
        // state.captured_screenshot = None;
        // state.include_screenshot = false;
        
        state.last_result = Some(CommandResult::Success(
            "Build request queued. Check Output panel for results.".to_string()
        ));
    }
    
    /// Update the most recent history entry with the generated Rune script
    /// Called by the build pipeline when Claude responds
    pub fn cache_rune_script(state: &mut CommandBarState, rune_script: String) {
        // Find the most recent AI build entry without a cached script
        for entry in state.history.iter_mut().rev() {
            if entry.is_ai_build && entry.rune_script.is_none() {
                entry.rune_script = Some(rune_script);
                info!("📝 Cached Rune script for: {}", entry.english_input);
                break;
            }
        }
    }
    
    /// Open file picker for Generate reference image
    fn open_viga_file_picker(state: &mut CommandBarState) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::path::PathBuf;
            
            // Use rfd (Rust File Dialog) for native file picker
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
                .set_title("Select Reference Image for Generation")
                .pick_file()
            {
                match Self::load_image_as_data_url(&path) {
                    Ok(data_url) => {
                        state.viga_reference_image = Some(data_url);
                        state.viga_mode = true;
                        state.status = format!("✨ Generate: Loaded reference image from {:?}", path.file_name().unwrap_or_default());
                        info!("✨ Generate: Loaded reference image from {:?}", path);
                    }
                    Err(e) => {
                        state.status = format!("Failed to load image: {}", e);
                        error!("✨ Generate: Failed to load image: {}", e);
                    }
                }
            }
        }
        
        #[cfg(target_arch = "wasm32")]
        {
            state.status = "Drag & drop an image onto the command bar to generate".to_string();
        }
    }
    
    /// Load image file and convert to base64 data URL
    #[cfg(not(target_arch = "wasm32"))]
    fn load_image_as_data_url(path: &std::path::Path) -> Result<String, String> {
        use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
        
        let bytes = std::fs::read(path)
            .map_err(|e| format!("Failed to read file: {}", e))?;
        
        // Determine MIME type from extension
        let mime_type = match path.extension().and_then(|e| e.to_str()) {
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("webp") => "image/webp",
            Some("gif") => "image/gif",
            _ => "image/png", // Default
        };
        
        let base64_data = BASE64.encode(&bytes);
        Ok(format!("data:{};base64,{}", mime_type, base64_data))
    }
    
    /// Start scene generation from reference image
    fn start_viga_generation(state: &mut CommandBarState, world: &mut World) {
        let Some(ref image) = state.viga_reference_image else {
            state.status = "No reference image attached. Drag & drop or click Generate to upload.".to_string();
            return;
        };
        
        // Get optional description from input
        let description = if state.input.trim().is_empty() {
            None
        } else {
            Some(state.input.trim().to_string())
        };
        
        info!("✨ Generate: Starting scene generation");
        info!("  Reference image: {} chars", image.len());
        if let Some(ref desc) = description {
            info!("  Description: {}", desc);
        }
        
        // Send generation request event
        world.write_message(crate::viga::pipeline::VigaRequestEvent {
            reference_image: image.clone(),
            description,
        });
        
        state.viga_mode = true;
        state.status = "✨ Generate: Creating 3D scene from image...".to_string();
        state.input.clear();
        
        state.last_result = Some(CommandResult::Success(
            "Generation started. Watch the Output panel for progress.".to_string()
        ));
    }
    
    /// Handle dropped files (for drag-and-drop support)
    pub fn handle_dropped_files(state: &mut CommandBarState, ctx: &egui::Context) {
        // Check for dropped files
        ctx.input(|i| {
            if !i.raw.dropped_files.is_empty() {
                for file in &i.raw.dropped_files {
                    // Check if it's an image
                    let is_image = file.name.ends_with(".png")
                        || file.name.ends_with(".jpg")
                        || file.name.ends_with(".jpeg")
                        || file.name.ends_with(".webp")
                        || file.name.ends_with(".gif");
                    
                    if is_image {
                        // Try to get bytes
                        if let Some(bytes) = &file.bytes {
                            use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
                            
                            // Determine MIME type
                            let mime_type = if file.name.ends_with(".png") {
                                "image/png"
                            } else if file.name.ends_with(".jpg") || file.name.ends_with(".jpeg") {
                                "image/jpeg"
                            } else if file.name.ends_with(".webp") {
                                "image/webp"
                            } else {
                                "image/png"
                            };
                            
                            let base64_data = BASE64.encode(bytes.as_ref());
                            let data_url = format!("data:{};base64,{}", mime_type, base64_data);
                            
                            state.viga_reference_image = Some(data_url);
                            state.viga_mode = true;
                            state.status = format!("✨ Generate: Loaded reference image: {}", file.name);
                            info!("✨ Generate: Loaded dropped image: {}", file.name);
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(ref path) = file.path {
                            match Self::load_image_as_data_url(path) {
                                Ok(data_url) => {
                                    state.viga_reference_image = Some(data_url);
                                    state.viga_mode = true;
                                    state.status = format!("✨ Generate: Loaded reference image: {}", file.name);
                                    info!("✨ Generate: Loaded dropped image: {}", file.name);
                                }
                                Err(e) => {
                                    state.status = format!("Failed to load image: {}", e);
                                }
                            }
                        }
                    }
                }
            }
            
            // Update drag hover state
            state.drag_hover = i.raw.hovered_files.iter().any(|f| {
                f.name.ends_with(".png")
                    || f.name.ends_with(".jpg")
                    || f.name.ends_with(".jpeg")
                    || f.name.ends_with(".webp")
                    || f.name.ends_with(".gif")
            });
        });
    }
}

/// Parse Entity from debug string format 'Entity(XvY)' or '0v1'
fn parse_entity_from_debug(s: &str) -> Option<bevy::prelude::Entity> {
    let s = s.trim();
    let s = if s.starts_with("Entity(") && s.ends_with(')') {
        &s[7..s.len()-1]
    } else {
        s
    };
    
    if let Some(v_pos) = s.find('v') {
        let index_str = &s[..v_pos];
        let gen_str = &s[v_pos+1..];
        
        if let (Ok(index), Ok(generation)) = (index_str.parse::<u32>(), gen_str.parse::<u32>()) {
            // Entity::from_bits expects (generation << 32) | index
            let bits = ((generation as u64) << 32) | (index as u64);
            return Some(bevy::prelude::Entity::from_bits(bits));
        }
    }
    
    None
}

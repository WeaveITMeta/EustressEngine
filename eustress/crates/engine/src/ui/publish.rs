// =============================================================================
// Eustress Engine - Publish Dialog
// =============================================================================
// UI for publishing experiences to the Eustress platform
// =============================================================================

use bevy::prelude::*;
use bevy_egui::egui;
use std::sync::{Arc, Mutex};

/// API base URL for experiences service
const EXPERIENCES_API_URL: &str = "https://experiences.eustress.dev";

/// State for the publish dialog
#[derive(Resource)]
pub struct PublishState {
    /// Experience name
    pub name: String,
    /// Experience description
    pub description: String,
    /// Tags (replaces genre system)
    pub tags: Vec<String>,
    /// Available tags to choose from
    pub available_tags: Vec<String>,
    /// Tag search/input field
    pub tag_input: String,
    /// Maximum players
    pub max_players: u32,
    /// Is the experience public
    pub is_public: bool,
    /// Open source (allow copying)
    pub open_source: bool,
    /// Experience ID if already published
    pub experience_id: Option<String>,
    /// Current version (for updates)
    pub version: u32,
    /// Publishing state
    pub publish_status: PublishStatus,
    /// Last error message
    pub error: Option<String>,
    /// Last success message
    pub success: Option<String>,
    /// Thumbnail path
    pub thumbnail_path: Option<String>,
    /// Scene file path to publish
    pub scene_path: Option<String>,
    /// JWT auth token
    pub auth_token: Option<String>,
    /// Async result receiver
    pub async_result: Arc<Mutex<Option<PublishResult>>>,
}

impl Default for PublishState {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            tags: Vec::new(),
            available_tags: vec![
                "Adventure".to_string(),
                "Simulation".to_string(),
                "Roleplay".to_string(),
                "Tycoon".to_string(),
                "Obby".to_string(),
                "Fighting".to_string(),
                "Sports".to_string(),
                "Racing".to_string(),
                "Shooter".to_string(),
                "Horror".to_string(),
                "Educational".to_string(),
                "Social".to_string(),
                "Puzzle".to_string(),
                "Sandbox".to_string(),
                "Survival".to_string(),
                "Building".to_string(),
                "Showcase".to_string(),
                "Multiplayer".to_string(),
                "Singleplayer".to_string(),
                "PvP".to_string(),
                "PvE".to_string(),
                "Co-op".to_string(),
            ],
            tag_input: String::new(),
            max_players: 10,
            is_public: true,
            open_source: false,
            experience_id: None,
            version: 1,
            publish_status: PublishStatus::Idle,
            error: None,
            success: None,
            thumbnail_path: None,
            scene_path: None,
            auth_token: None,
            async_result: Arc::new(Mutex::new(None)),
        }
    }
}

/// Publishing status
#[derive(Default, Clone, PartialEq)]
pub enum PublishStatus {
    #[default]
    Idle,
    Initiating,
    UploadingScene,
    UploadingThumbnail,
    Committing,
    Complete,
    Failed,
}

impl PublishStatus {
    pub fn is_busy(&self) -> bool {
        matches!(self, 
            PublishStatus::Initiating | 
            PublishStatus::UploadingScene | 
            PublishStatus::UploadingThumbnail |
            PublishStatus::Committing
        )
    }
    
    pub fn status_text(&self) -> &'static str {
        match self {
            PublishStatus::Idle => "Ready",
            PublishStatus::Initiating => "Initiating publish...",
            PublishStatus::UploadingScene => "Uploading scene...",
            PublishStatus::UploadingThumbnail => "Uploading thumbnail...",
            PublishStatus::Committing => "Finalizing...",
            PublishStatus::Complete => "Published!",
            PublishStatus::Failed => "Failed",
        }
    }
}

/// Result from async publish operation
#[derive(Clone)]
pub enum PublishResult {
    Success { experience_id: String, version: u32, url: String },
    Error(String),
}

// Tags are used instead of genres - see PublishState.tags and PublishState.available_tags

/// Show the publish dialog
pub fn show_publish_dialog(
    ctx: &egui::Context,
    state: &mut super::StudioState,
    publish_state: &mut PublishState,
    scene_name: &str,
    scene_path: Option<&str>,
) {
    if !state.show_publish_dialog {
        return;
    }
    
    // Check for async results
    if let Ok(mut result_guard) = publish_state.async_result.try_lock() {
        if let Some(result) = result_guard.take() {
            match result {
                PublishResult::Success { experience_id, version, url } => {
                    publish_state.experience_id = Some(experience_id);
                    publish_state.version = version;
                    publish_state.publish_status = PublishStatus::Complete;
                    publish_state.success = Some(format!(
                        "Published successfully!\nURL: {}", url
                    ));
                    publish_state.error = None;
                }
                PublishResult::Error(msg) => {
                    publish_state.publish_status = PublishStatus::Failed;
                    publish_state.error = Some(msg);
                    publish_state.success = None;
                }
            }
        }
    }
    
    // Don't auto-fill name - let user enter their own title
    // Name stays empty until user types something
    
    // Store scene path
    if publish_state.scene_path.is_none() {
        publish_state.scene_path = scene_path.map(|s| s.to_string());
    }
    
    let title = if state.publish_as_new {
        "Publish As New Experience"
    } else if publish_state.experience_id.is_some() {
        "Update Experience"
    } else {
        "Publish Experience"
    };
    
    let is_busy = publish_state.publish_status.is_busy();
    
    egui::Window::new(title)
        .collapsible(false)
        .resizable(true)
        .default_width(500.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(480.0);
            
            // Publishing progress
            if is_busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(publish_state.publish_status.status_text());
                });
                ui.add_space(8.0);
            }
            
            // Error/Success messages
            if let Some(error) = &publish_state.error {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::RED, "⚠");
                    ui.colored_label(egui::Color32::RED, error);
                });
                ui.add_space(8.0);
            }
            
            if let Some(success) = &publish_state.success {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::GREEN, "✓");
                    ui.colored_label(egui::Color32::GREEN, success);
                });
                ui.add_space(8.0);
            }
            
// Disable inputs while publishing
            ui.add_enabled_ui(!is_busy, |ui| {
                // Basic Info Section
                ui.heading("Basic Information");
                ui.separator();
                
                egui::Grid::new("publish_basic_info")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Name:");
                        ui.add(egui::TextEdit::singleline(&mut publish_state.name)
                            .hint_text("Enter Title")
                            .desired_width(350.0));
                        ui.end_row();
                        
                        ui.label("Description:");
                        ui.add(egui::TextEdit::multiline(&mut publish_state.description)
                            .hint_text("Describe your experience...")
                            .desired_width(350.0)
                            .desired_rows(4));
                        ui.end_row();
                        
                        // Tags system (replaces genre)
                        ui.label("Tags:");
                        ui.vertical(|ui| {
                            // Show selected tags as removable chips
                            ui.horizontal_wrapped(|ui| {
                                let mut tag_to_remove: Option<usize> = None;
                                for (i, tag) in publish_state.tags.iter().enumerate() {
                                    let chip = ui.add(
                                        egui::Button::new(format!("{} ✕", tag))
                                            .fill(egui::Color32::from_rgb(60, 100, 150))
                                    );
                                    if chip.clicked() {
                                        tag_to_remove = Some(i);
                                    }
                                }
                                if let Some(i) = tag_to_remove {
                                    publish_state.tags.remove(i);
                                }
                            });
                            
                            // Tag input with suggestions
                            ui.horizontal(|ui| {
                                let response = ui.add(egui::TextEdit::singleline(&mut publish_state.tag_input)
                                    .hint_text("Add tag...")
                                    .desired_width(200.0));
                                
                                // Show filtered suggestions
                                if !publish_state.tag_input.is_empty() {
                                    let input_lower = publish_state.tag_input.to_lowercase();
                                    let suggestions: Vec<_> = publish_state.available_tags.iter()
                                        .filter(|t| t.to_lowercase().contains(&input_lower) && !publish_state.tags.contains(t))
                                        .take(5)
                                        .cloned()
                                        .collect();
                                    
                                    if !suggestions.is_empty() {
                                        egui::ComboBox::from_id_salt("tag_suggestions")
                                            .selected_text("Select...")
                                            .show_ui(ui, |ui| {
                                                for suggestion in suggestions {
                                                    if ui.selectable_label(false, &suggestion).clicked() {
                                                        publish_state.tags.push(suggestion);
                                                        publish_state.tag_input.clear();
                                                    }
                                                }
                                            });
                                    }
                                }
                                
                                // Add custom tag button
                                if ui.button("+").clicked() && !publish_state.tag_input.is_empty() {
                                    let new_tag = publish_state.tag_input.trim().to_string();
                                    if !new_tag.is_empty() && !publish_state.tags.contains(&new_tag) {
                                        publish_state.tags.push(new_tag);
                                        publish_state.tag_input.clear();
                                    }
                                }
                            });
                            
                            // Quick tag buttons for common tags
                            ui.horizontal_wrapped(|ui| {
                                ui.weak("Quick add:");
                                for tag in ["Multiplayer", "Singleplayer", "Sandbox", "Adventure", "Puzzle"] {
                                    if !publish_state.tags.contains(&tag.to_string()) {
                                        if ui.small_button(tag).clicked() {
                                            publish_state.tags.push(tag.to_string());
                                        }
                                    }
                                }
                            });
                        });
                        ui.end_row();
                    });
                
                ui.add_space(16.0);
                
                // Settings Section
                ui.heading("Settings");
                ui.separator();
                
                egui::Grid::new("publish_settings")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Max Players:");
                        ui.add(egui::DragValue::new(&mut publish_state.max_players)
                            .range(1..=250)
                            .speed(1));
                        ui.end_row();
                        
                        ui.label("Visibility:");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut publish_state.is_public, true, "🌐 Public");
                            ui.selectable_value(&mut publish_state.is_public, false, "🔒 Private");
                        });
                        ui.end_row();
                        
                        ui.label("Open Source:");
                        ui.checkbox(&mut publish_state.open_source, "Allow others to view and copy source");
                        ui.end_row();
                    });
                
                ui.add_space(16.0);
                
                // Thumbnail Section
                ui.heading("Thumbnail");
                ui.separator();
                
                ui.horizontal(|ui| {
                    if let Some(path) = &publish_state.thumbnail_path {
                        ui.label(format!("📷 {}", path));
                    } else {
                        ui.label("No thumbnail selected (will use auto-generated)");
                    }
                    
                    if ui.button("Browse...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                            .set_title("Select Thumbnail")
                            .pick_file()
                        {
                            publish_state.thumbnail_path = Some(path.display().to_string());
                        }
                    }
                    
                    if publish_state.thumbnail_path.is_some() {
                        if ui.button("Clear").clicked() {
                            publish_state.thumbnail_path = None;
                        }
                    }
                });
                
                // Show experience ID if already published
                if let Some(exp_id) = &publish_state.experience_id {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Experience ID:");
                        ui.monospace(exp_id);
                        if ui.small_button("📋").on_hover_text("Copy ID").clicked() {
                            ui.ctx().copy_text(exp_id.clone());
                        }
                    });
                }
                
                ui.add_space(24.0);
                
                // Action Buttons
                ui.separator();
            }); // Re-enable for buttons
            
            ui.horizontal(|ui| {
                // Cancel button
                let cancel_text = if is_busy { "Close" } else { "Cancel" };
                if ui.button(cancel_text).clicked() {
                    if !is_busy {
                        state.show_publish_dialog = false;
                        publish_state.error = None;
                        publish_state.success = None;
                    }
                }
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Publish button
                    let button_text = if is_busy {
                        publish_state.publish_status.status_text()
                    } else if publish_state.experience_id.is_some() && !state.publish_as_new {
                        "Update"
                    } else {
                        "Publish"
                    };
                    
                    let publish_enabled = !is_busy 
                        && !publish_state.name.is_empty()
                        && publish_state.auth_token.is_some();
                    
                    // Show login button if no token
                    if publish_state.auth_token.is_none() && !is_busy {
                        if ui.button("🔑 Sign In to Publish").clicked() {
                            // Trigger login - this will be handled by the auth system
                            // The publish dialog will stay open and update when login completes
                            state.trigger_login = true;
                        }
                    }
                    
                    if ui.add_enabled(publish_enabled, egui::Button::new(format!("🚀 {}", button_text)))
                        .clicked() 
                    {
                        // Validate
                        if publish_state.name.trim().is_empty() {
                            publish_state.error = Some("Experience name is required".to_string());
                        } else if publish_state.scene_path.is_none() {
                            publish_state.error = Some("No scene file to publish. Save your scene first.".to_string());
                        } else {
                            publish_state.error = None;
                            publish_state.success = None;
                            
                            // Start async publish
                            start_publish(publish_state, state.publish_as_new);
                        }
                    }
                });
            });
        });
}

/// Start the async publish process
fn start_publish(publish_state: &mut PublishState, as_new: bool) {
    publish_state.publish_status = PublishStatus::Initiating;
    
    let result_arc = publish_state.async_result.clone();
    let name = publish_state.name.clone();
    let description = publish_state.description.clone();
    let tags = publish_state.tags.clone();
    let max_players = publish_state.max_players;
    let is_public = publish_state.is_public;
    let open_source = publish_state.open_source;
    let experience_id = if as_new { None } else { publish_state.experience_id.clone() };
    let auth_token = publish_state.auth_token.clone().unwrap_or_default();
    let scene_path = publish_state.scene_path.clone();
    let thumbnail_path = publish_state.thumbnail_path.clone();
    
    // Spawn async task
    std::thread::spawn(move || {
        let result = execute_publish(
            &auth_token,
            experience_id.as_deref(),
            &name,
            &description,
            &tags,
            max_players,
            is_public,
            open_source,
            scene_path.as_deref(),
            thumbnail_path.as_deref(),
        );
        
        if let Ok(mut guard) = result_arc.lock() {
            *guard = Some(result);
        }
    });
}

/// Execute the publish workflow (blocking)
fn execute_publish(
    auth_token: &str,
    experience_id: Option<&str>,
    name: &str,
    description: &str,
    tags: &[String],
    max_players: u32,
    is_public: bool,
    open_source: bool,
    scene_path: Option<&str>,
    thumbnail_path: Option<&str>,
) -> PublishResult {
    // Use ureq for blocking HTTP (simpler than async in this context)
    let client = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(60))
        .build();
    
    // Step 1: Initiate publish or update
    let init_url = if let Some(exp_id) = experience_id {
        format!("{}/api/experience/{}", EXPERIENCES_API_URL, exp_id)
    } else {
        format!("{}/api/experience/publish", EXPERIENCES_API_URL)
    };
    
    let method = if experience_id.is_some() { "PUT" } else { "POST" };
    
    let init_body = serde_json::json!({
        "name": name,
        "description": description,
        "tags": tags,
        "max_players": max_players,
        "is_public": is_public,
        "open_source": open_source,
    });
    
    let init_response = if method == "PUT" {
        client.put(&init_url)
            .set("Authorization", &format!("Bearer {}", auth_token))
            .set("Content-Type", "application/json")
            .send_json(&init_body)
    } else {
        client.post(&init_url)
            .set("Authorization", &format!("Bearer {}", auth_token))
            .set("Content-Type", "application/json")
            .send_json(&init_body)
    };
    
    let init_data: serde_json::Value = match init_response {
        Ok(resp) => match resp.into_json() {
            Ok(json) => json,
            Err(e) => return PublishResult::Error(format!("Failed to parse response: {}", e)),
        },
        Err(e) => return PublishResult::Error(format!("Failed to initiate publish: {}", e)),
    };
    
    let exp_id = init_data["experience_id"].as_str()
        .unwrap_or_default()
        .to_string();
    let version = init_data["version"].as_u64().unwrap_or(1) as u32;
    
    if exp_id.is_empty() {
        return PublishResult::Error("No experience ID returned".to_string());
    }
    
    // Step 2: Upload scene file
    if let Some(scene_file) = scene_path {
        let scene_data = match std::fs::read(scene_file) {
            Ok(data) => data,
            Err(e) => return PublishResult::Error(format!("Failed to read scene file: {}", e)),
        };
        
        let upload_url = format!("{}/api/experience/upload/{}/scene.eustress", 
            EXPERIENCES_API_URL, exp_id);
        
        if let Err(e) = client.post(&upload_url)
            .set("Authorization", &format!("Bearer {}", auth_token))
            .set("Content-Type", "application/octet-stream")
            .send_bytes(&scene_data)
        {
            return PublishResult::Error(format!("Failed to upload scene: {}", e));
        }
    }
    
    // Step 3: Upload thumbnail (optional)
    if let Some(thumb_file) = thumbnail_path {
        if let Ok(thumb_data) = std::fs::read(thumb_file) {
            let upload_url = format!("{}/api/experience/upload/{}/thumbnail.webp", 
                EXPERIENCES_API_URL, exp_id);
            
            // Ignore thumbnail upload errors (not critical)
            let _ = client.post(&upload_url)
                .set("Authorization", &format!("Bearer {}", auth_token))
                .set("Content-Type", "image/webp")
                .send_bytes(&thumb_data);
        }
    }
    
    // Step 4: Commit the publish
    let commit_url = format!("{}/api/experience/{}/commit", EXPERIENCES_API_URL, exp_id);
    
    match client.post(&commit_url)
        .set("Authorization", &format!("Bearer {}", auth_token))
        .set("Content-Type", "application/json")
        .send_json(&serde_json::json!({}))
    {
        Ok(resp) => {
            let commit_data: serde_json::Value = resp.into_json().unwrap_or_default();
            let url = commit_data["url"].as_str()
                .unwrap_or(&format!("{}/api/experience/{}", EXPERIENCES_API_URL, exp_id))
                .to_string();
            
            PublishResult::Success {
                experience_id: exp_id,
                version,
                url,
            }
        }
        Err(e) => PublishResult::Error(format!("Failed to commit publish: {}", e)),
    }
}

/// Plugin for the publish system
pub struct PublishPlugin;

impl Plugin for PublishPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PublishState>();
    }
}

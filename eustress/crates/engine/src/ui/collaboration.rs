#![allow(dead_code)]

use bevy::prelude::*;
use bevy_egui::egui;

/// Collaboration user info
#[derive(Clone, Debug)]
pub struct CollaborationUser {
    pub id: String,
    pub name: String,
    pub color: egui::Color32,
    pub status: UserStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UserStatus {
    Online,
    Editing,
    Idle,
    Offline,
}

/// Connection status for Team Create
#[derive(Clone, Debug, PartialEq, Default)]
pub enum ConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// Collaboration panel state
#[derive(Resource)]
pub struct CollaborationState {
    pub show: bool,
    pub users: Vec<CollaborationUser>,
    pub current_user_id: String,
    pub connection_status: ConnectionStatus,
    pub server_address: String,
    pub session_id: Option<String>,
}

impl Default for CollaborationState {
    fn default() -> Self {
        Self {
            show: true,
            users: vec![
                CollaborationUser {
                    id: "local".to_string(),
                    name: "You (Local)".to_string(),
                    color: egui::Color32::from_rgb(98, 217, 138),
                    status: UserStatus::Editing,
                },
            ],
            current_user_id: "local".to_string(),
            connection_status: ConnectionStatus::Disconnected,
            server_address: "localhost:7000".to_string(),
            session_id: None,
        }
    }
}

/// Collaboration panel
pub struct CollaborationPanel;

impl CollaborationPanel {
    /// Show as standalone side panel (primary mode)
    pub fn show_as_panel(ctx: &egui::Context, state: &mut CollaborationState) {
        egui::SidePanel::right("collaboration_panel")
            .min_width(250.0)
            .default_width(350.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("👥 Collaboration");
                
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").clicked() {
                            state.show = false;
                        }
                    });
                });
                
                ui.separator();
                Self::show_content(ui, state);
            });
    }
    
    /// Show as nested panel within Properties (secondary mode)
    pub fn show_nested(ui: &mut egui::Ui, state: &mut CollaborationState) {
        ui.push_id("collab_nested", |ui| {
            ui.add_space(4.0);  // Top padding
            ui.horizontal(|ui| {
                ui.add_space(8.0);  // Left padding
                ui.heading("👥 Collaboration");
            });
            ui.separator();
            
            Self::show_content(ui, state);
        });
    }
    
    /// Shared content rendering (public for dock system)
    pub fn show_content(ui: &mut egui::Ui, state: &mut CollaborationState) {
        // Use unique ID to avoid ScrollArea conflicts
        ui.push_id("collaboration_panel_content", |ui| {
            // Connection status header
            ui.horizontal(|ui| {
                ui.add_space(8.0);  // Left padding
                let (status_icon, status_color, status_text) = match &state.connection_status {
                    ConnectionStatus::Disconnected => ("⚫", egui::Color32::GRAY, "Disconnected"),
                    ConnectionStatus::Connecting => ("🔄", egui::Color32::YELLOW, "Connecting..."),
                    ConnectionStatus::Connected => ("🟢", egui::Color32::GREEN, "Connected"),
                    ConnectionStatus::Error(_) => ("🔴", egui::Color32::RED, "Error"),
                };
                ui.colored_label(status_color, status_icon);
                ui.label(status_text);
            });
            
            if let ConnectionStatus::Error(ref msg) = state.connection_status {
                ui.colored_label(egui::Color32::RED, format!("Error: {}", msg));
            }
            
            ui.separator();
            
            // Session info (if connected)
            if let Some(ref session_id) = state.session_id {
                ui.horizontal(|ui| {
                    ui.label("Session:");
                    ui.monospace(session_id);
                    if ui.small_button("📋").on_hover_text("Copy session ID").clicked() {
                        ui.ctx().copy_text(session_id.clone());
                    }
                });
                ui.separator();
            }
            
            // Online Users section
            ui.horizontal(|ui| {
                ui.add_space(8.0);  // Left padding
                ui.label(egui::RichText::new("👥 Team Members").strong());
            });
            ui.add_space(4.0);
            
            // Users list with unique scroll area ID
            egui::ScrollArea::vertical()
                .id_salt("collab_users_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    for user in &state.users {
                        ui.horizontal(|ui| {
                            // Status indicator
                            let (status_icon, status_color) = match user.status {
                                UserStatus::Online => ("●", egui::Color32::GREEN),
                                UserStatus::Editing => ("✏", egui::Color32::from_rgb(100, 200, 255)),
                                UserStatus::Idle => ("●", egui::Color32::YELLOW),
                                UserStatus::Offline => ("●", egui::Color32::GRAY),
                            };
                            ui.colored_label(status_color, status_icon);
                            
                            // User avatar (colored circle)
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(16.0, 16.0),
                                egui::Sense::hover()
                            );
                            ui.painter().circle_filled(rect.center(), 8.0, user.color);
                            
                            // User name
                            let is_current = user.id == state.current_user_id;
                            if is_current {
                                ui.label(egui::RichText::new(&user.name).strong());
                            } else {
                                ui.label(&user.name);
                            }
                            
                            // Status text
                            let status_text = match user.status {
                                UserStatus::Online => "online",
                                UserStatus::Editing => "editing",
                                UserStatus::Idle => "idle",
                                UserStatus::Offline => "offline",
                            };
                            ui.label(egui::RichText::new(format!("({})", status_text)).weak().small());
                        });
                    }
                });
            
            ui.separator();
            
            // Connection controls
            ui.label(egui::RichText::new("🔗 Connection").strong());
            ui.add_space(4.0);
            
            // Server address input
            ui.horizontal(|ui| {
                ui.label("Server:");
                ui.add(
                    egui::TextEdit::singleline(&mut state.server_address)
                        .hint_text("localhost:7000")
                        .desired_width(150.0)
                );
            });
            
            ui.add_space(8.0);
            
            // Connect/Disconnect buttons
            ui.horizontal(|ui| {
                let is_connected = matches!(state.connection_status, ConnectionStatus::Connected);
                let is_connecting = matches!(state.connection_status, ConnectionStatus::Connecting);
                
                ui.add_enabled_ui(!is_connected && !is_connecting, |ui| {
                    if ui.button("🔗 Connect").clicked() {
                        state.connection_status = ConnectionStatus::Connecting;
                        // TODO: Implement actual connection via networking
                        // For now, simulate connection
                    }
                });
                
                ui.add_enabled_ui(is_connected, |ui| {
                    if ui.button("🔌 Disconnect").clicked() {
                        state.connection_status = ConnectionStatus::Disconnected;
                        state.session_id = None;
                        // Keep only local user
                        state.users.retain(|u| u.id == state.current_user_id);
                    }
                });
            });
            
            ui.add_space(8.0);
            
            // Create/Join session
            ui.horizontal(|ui| {
                if ui.button("➕ Create Session").on_hover_text("Start a new collaboration session").clicked() {
                    // TODO: Create new session
                    state.session_id = Some(format!("session-{}", rand_id()));
                    state.connection_status = ConnectionStatus::Connected;
                }
                
                if ui.button("📥 Join Session").on_hover_text("Join an existing session").clicked() {
                    // TODO: Show join dialog
                }
            });
            
            ui.add_space(16.0);
            ui.separator();
            
            // Info section
            ui.label(egui::RichText::new("Team Create").small().weak());
            ui.label(egui::RichText::new("Collaborate in real-time with your team.").small().weak());
            ui.label(egui::RichText::new("Changes sync automatically when connected.").small().weak());
        });
    }
}

/// Generate a simple random ID for sessions
fn rand_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    format!("{:x}", duration.as_millis() % 0xFFFFFF)
}

/// System to handle collaboration cursor sharing
/// Draws other users' cursors in the 3D viewport when connected
pub fn update_collaboration_cursors(
    collab_state: Res<CollaborationState>,
    mut gizmos: Gizmos,
) {
    // Only draw cursors when connected
    if !matches!(collab_state.connection_status, ConnectionStatus::Connected) {
        return;
    }
    
    // Draw cursors for other users who are actively editing
    for user in &collab_state.users {
        if user.id != collab_state.current_user_id {
            // Only show cursors for users who are online or editing
            if matches!(user.status, UserStatus::Online | UserStatus::Editing) {
                // TODO: Track actual cursor positions via networking
                // For now, draw a placeholder sphere at origin
                let bevy_color = bevy::prelude::Color::srgba_u8(
                    user.color.r(),
                    user.color.g(),
                    user.color.b(),
                    user.color.a()
                );
                gizmos.sphere(Isometry3d::from_translation(Vec3::ZERO), 0.1, bevy_color);
            }
        }
    }
}

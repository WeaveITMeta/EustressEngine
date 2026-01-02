//! Pause Menu Plugin
//! 
//! Provides a universal pause menu accessible via ESC key with:
//! - Resume: Continue playing
//! - Reset Character: Respawn at spawn point
//! - Settings: Modular settings screens (Gameplay, Graphics, Audio, Controls)
//! - Exit: Quit the game

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use avian3d::prelude::LinearVelocity;

use super::player_plugin::{PlayerService, PlayerCamera, SpawnLocation, get_spawn_position_or_default};

// ============================================================================
// Settings Data
// ============================================================================

/// Game settings that persist
#[derive(Resource, Clone, Debug)]
pub struct GameSettings {
    // Gameplay
    pub auto_sprint: bool,
    pub toggle_crouch: bool,
    pub invert_y_axis: bool,
    pub show_crosshair: bool,
    pub show_fps: bool,
    
    // Graphics
    pub vsync: bool,
    pub fullscreen: bool,
    pub shadow_quality: QualityLevel,
    pub texture_quality: QualityLevel,
    pub render_distance: f32,
    pub ambient_occlusion: bool,
    pub bloom: bool,
    
    // Audio
    pub master_volume: f32,
    pub music_volume: f32,
    pub sfx_volume: f32,
    pub ambient_volume: f32,
    pub voice_volume: f32,
    pub mute_when_unfocused: bool,
    
    // Controls
    pub mouse_sensitivity: f32,
    pub mouse_smoothing: bool,
    pub controller_vibration: bool,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            // Gameplay
            auto_sprint: false,
            toggle_crouch: true,
            invert_y_axis: false,
            show_crosshair: true,
            show_fps: false,
            
            // Graphics
            vsync: true,
            fullscreen: false,
            shadow_quality: QualityLevel::High,
            texture_quality: QualityLevel::High,
            render_distance: 1000.0,
            ambient_occlusion: true,
            bloom: true,
            
            // Audio
            master_volume: 1.0,
            music_volume: 0.7,
            sfx_volume: 1.0,
            ambient_volume: 0.8,
            voice_volume: 1.0,
            mute_when_unfocused: false,
            
            // Controls
            mouse_sensitivity: 1.0,
            mouse_smoothing: false,
            controller_vibration: true,
        }
    }
}

/// Quality level for graphics settings
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityLevel {
    Low,
    Medium,
    High,
    Ultra,
}

impl QualityLevel {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Ultra => "Ultra",
        }
    }
    
    fn all() -> &'static [QualityLevel] {
        &[Self::Low, Self::Medium, Self::High, Self::Ultra]
    }
}

// ============================================================================
// Menu State
// ============================================================================

/// Current settings screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsScreen {
    #[default]
    None,
    Gameplay,
    Graphics,
    Audio,
    Controls,
}

/// Pause menu state
#[derive(Resource, Default)]
pub struct PauseMenuState {
    /// Whether the pause menu is open
    pub is_open: bool,
    /// Current settings screen (None = main menu)
    pub settings_screen: SettingsScreen,
    /// Temporary settings being edited (applied on save)
    pub temp_settings: Option<GameSettings>,
}

/// Event to reset the character
#[derive(bevy::prelude::Message, Clone)]
pub struct ResetCharacterEvent;

/// Event when settings are applied
#[derive(bevy::prelude::Message, Clone)]
pub struct SettingsChangedEvent;

// ============================================================================
// Plugin
// ============================================================================

/// Pause Menu Plugin
pub struct PauseMenuPlugin;

impl Plugin for PauseMenuPlugin {
    fn build(&self, app: &mut App) {
        // Only add EguiPlugin if not already added
        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }
        
        app
            .init_resource::<PauseMenuState>()
            .init_resource::<GameSettings>()
            .add_message::<ResetCharacterEvent>()
            .add_message::<SettingsChangedEvent>()
            .add_systems(Update, toggle_pause_menu)
            .add_systems(EguiPrimaryContextPass, draw_pause_menu)
            .add_systems(Update, (handle_reset_character, apply_settings));
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Toggle pause menu with ESC
fn toggle_pause_menu(
    keys: Res<ButtonInput<KeyCode>>,
    mut pause_state: ResMut<PauseMenuState>,
    mut player_service: ResMut<PlayerService>,
    mut cursor_options: Query<&mut bevy::window::CursorOptions, With<bevy::window::Window>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        // If in settings, go back to main menu
        if pause_state.settings_screen != SettingsScreen::None {
            pause_state.settings_screen = SettingsScreen::None;
            pause_state.temp_settings = None; // Discard unsaved changes
            return;
        }
        
        pause_state.is_open = !pause_state.is_open;
        pause_state.settings_screen = SettingsScreen::None;
        pause_state.temp_settings = None;
        
        // Update cursor based on pause state
        if let Ok(mut cursor) = cursor_options.single_mut() {
            if pause_state.is_open {
                cursor.grab_mode = bevy::window::CursorGrabMode::None;
                cursor.visible = true;
                player_service.cursor_locked = false;
            } else {
                cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
                cursor.visible = false;
                player_service.cursor_locked = true;
            }
        }
    }
}

/// Draw the pause menu UI
fn draw_pause_menu(
    mut contexts: EguiContexts,
    mut pause_state: ResMut<PauseMenuState>,
    mut settings: ResMut<GameSettings>,
    mut player_service: ResMut<PlayerService>,
    mut cursor_options: Query<&mut bevy::window::CursorOptions, With<bevy::window::Window>>,
    mut reset_events: MessageWriter<ResetCharacterEvent>,
    mut settings_events: MessageWriter<SettingsChangedEvent>,
    mut exit_events: MessageWriter<AppExit>,
) {
    if !pause_state.is_open {
        return;
    }
    
    let Ok(ctx) = contexts.ctx_mut() else { return };
    
    // Semi-transparent overlay
    egui::Area::new(egui::Id::new("pause_overlay"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(ctx, |ui| {
            let screen_rect = ctx.available_rect();
            ui.allocate_space(screen_rect.size());
            ui.painter().rect_filled(
                screen_rect,
                0.0,
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180),
            );
        });
    
    // Route to appropriate screen
    match pause_state.settings_screen {
        SettingsScreen::None => {
            draw_main_menu(
                ctx,
                &mut pause_state,
                &mut player_service,
                &mut cursor_options,
                &mut reset_events,
                &mut exit_events,
                &settings,
            );
        }
        SettingsScreen::Gameplay => {
            draw_settings_gameplay(ctx, &mut pause_state, &mut settings, &mut settings_events);
        }
        SettingsScreen::Graphics => {
            draw_settings_graphics(ctx, &mut pause_state, &mut settings, &mut settings_events);
        }
        SettingsScreen::Audio => {
            draw_settings_audio(ctx, &mut pause_state, &mut settings, &mut settings_events);
        }
        SettingsScreen::Controls => {
            draw_settings_controls(ctx, &mut pause_state, &mut settings, &mut settings_events);
        }
    }
}

/// Draw main pause menu
fn draw_main_menu(
    ctx: &egui::Context,
    pause_state: &mut PauseMenuState,
    player_service: &mut PlayerService,
    cursor_options: &mut Query<&mut bevy::window::CursorOptions, With<bevy::window::Window>>,
    reset_events: &mut MessageWriter<ResetCharacterEvent>,
    exit_events: &mut MessageWriter<AppExit>,
    settings: &GameSettings,
) {
    egui::Window::new("⏸ PAUSED")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(300.0)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.heading("Eustress");
                ui.add_space(20.0);
                
                let button_size = egui::vec2(200.0, 40.0);
                
                // Resume
                if ui.add_sized(button_size, egui::Button::new("▶ Resume")).clicked() {
                    pause_state.is_open = false;
                    if let Ok(mut cursor) = cursor_options.single_mut() {
                        cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
                        cursor.visible = false;
                        player_service.cursor_locked = true;
                    }
                }
                
                ui.add_space(10.0);
                
                // Reset Character
                if ui.add_sized(button_size, egui::Button::new("🔄 Reset Character")).clicked() {
                    reset_events.write(ResetCharacterEvent);
                    pause_state.is_open = false;
                    if let Ok(mut cursor) = cursor_options.single_mut() {
                        cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
                        cursor.visible = false;
                        player_service.cursor_locked = true;
                    }
                }
                
                ui.add_space(10.0);
                
                // Settings button
                if ui.add_sized(button_size, egui::Button::new("⚙ Settings")).clicked() {
                    pause_state.settings_screen = SettingsScreen::Gameplay;
                    pause_state.temp_settings = Some(settings.clone());
                }
                
                ui.add_space(10.0);
                
                // Exit
                if ui.add_sized(button_size, egui::Button::new("🚪 Exit Game")).clicked() {
                    info!("👋 Exiting game...");
                    exit_events.write(AppExit::Success);
                }
                
                ui.add_space(20.0);
                ui.label(egui::RichText::new("Press ESC to resume").small().weak());
            });
        });
}

/// Draw settings header with back button and tabs
fn draw_settings_header(
    ui: &mut egui::Ui,
    pause_state: &mut PauseMenuState,
    current_screen: SettingsScreen,
) {
    ui.horizontal(|ui| {
        // Back button
        if ui.button("← Back").clicked() {
            pause_state.settings_screen = SettingsScreen::None;
            pause_state.temp_settings = None;
        }
        
        ui.separator();
        
        // Tab buttons
        let tabs = [
            (SettingsScreen::Gameplay, "🎮 Gameplay"),
            (SettingsScreen::Graphics, "🖥 Graphics"),
            (SettingsScreen::Audio, "🔊 Audio"),
            (SettingsScreen::Controls, "🎯 Controls"),
        ];
        
        for (screen, label) in tabs {
            let selected = current_screen == screen;
            if ui.selectable_label(selected, label).clicked() {
                pause_state.settings_screen = screen;
            }
        }
    });
    
    ui.separator();
}

/// Draw Gameplay settings screen
fn draw_settings_gameplay(
    ctx: &egui::Context,
    pause_state: &mut PauseMenuState,
    settings: &mut GameSettings,
    settings_events: &mut MessageWriter<SettingsChangedEvent>,
) {
    egui::Window::new("Settings")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(450.0)
        .show(ctx, |ui| {
            draw_settings_header(ui, pause_state, SettingsScreen::Gameplay);
            
            ui.add_space(10.0);
            ui.heading("🎮 Gameplay Settings");
            ui.add_space(10.0);
            
            egui::Grid::new("gameplay_grid")
                .num_columns(2)
                .spacing([40.0, 8.0])
                .show(ui, |ui| {
                    // Auto Sprint
                    ui.label("Auto Sprint:");
                    ui.checkbox(&mut settings.auto_sprint, "");
                    ui.end_row();
                    
                    // Toggle Crouch
                    ui.label("Toggle Crouch:");
                    ui.checkbox(&mut settings.toggle_crouch, "");
                    ui.end_row();
                    
                    // Invert Y Axis
                    ui.label("Invert Y Axis:");
                    ui.checkbox(&mut settings.invert_y_axis, "");
                    ui.end_row();
                    
                    // Show Crosshair
                    ui.label("Show Crosshair:");
                    ui.checkbox(&mut settings.show_crosshair, "");
                    ui.end_row();
                    
                    // Show FPS
                    ui.label("Show FPS Counter:");
                    ui.checkbox(&mut settings.show_fps, "");
                    ui.end_row();
                });
            
            ui.add_space(20.0);
            draw_settings_footer(ui, pause_state, settings, settings_events);
        });
}

/// Draw Graphics settings screen
fn draw_settings_graphics(
    ctx: &egui::Context,
    pause_state: &mut PauseMenuState,
    settings: &mut GameSettings,
    settings_events: &mut MessageWriter<SettingsChangedEvent>,
) {
    egui::Window::new("Settings")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(450.0)
        .show(ctx, |ui| {
            draw_settings_header(ui, pause_state, SettingsScreen::Graphics);
            
            ui.add_space(10.0);
            ui.heading("🖥 Graphics Settings");
            ui.add_space(10.0);
            
            egui::Grid::new("graphics_grid")
                .num_columns(2)
                .spacing([40.0, 8.0])
                .show(ui, |ui| {
                    // VSync
                    ui.label("VSync:");
                    ui.checkbox(&mut settings.vsync, "");
                    ui.end_row();
                    
                    // Fullscreen
                    ui.label("Fullscreen:");
                    ui.checkbox(&mut settings.fullscreen, "");
                    ui.end_row();
                    
                    // Shadow Quality
                    ui.label("Shadow Quality:");
                    egui::ComboBox::from_id_salt("shadow_quality")
                        .selected_text(settings.shadow_quality.as_str())
                        .show_ui(ui, |ui| {
                            for level in QualityLevel::all() {
                                ui.selectable_value(&mut settings.shadow_quality, *level, level.as_str());
                            }
                        });
                    ui.end_row();
                    
                    // Texture Quality
                    ui.label("Texture Quality:");
                    egui::ComboBox::from_id_salt("texture_quality")
                        .selected_text(settings.texture_quality.as_str())
                        .show_ui(ui, |ui| {
                            for level in QualityLevel::all() {
                                ui.selectable_value(&mut settings.texture_quality, *level, level.as_str());
                            }
                        });
                    ui.end_row();
                    
                    // Render Distance
                    ui.label("Render Distance:");
                    ui.add(egui::Slider::new(&mut settings.render_distance, 100.0..=2000.0)
                        .suffix(" m")
                        .logarithmic(true));
                    ui.end_row();
                    
                    // Ambient Occlusion
                    ui.label("Ambient Occlusion:");
                    ui.checkbox(&mut settings.ambient_occlusion, "");
                    ui.end_row();
                    
                    // Bloom
                    ui.label("Bloom:");
                    ui.checkbox(&mut settings.bloom, "");
                    ui.end_row();
                });
            
            ui.add_space(20.0);
            draw_settings_footer(ui, pause_state, settings, settings_events);
        });
}

/// Draw Audio settings screen
fn draw_settings_audio(
    ctx: &egui::Context,
    pause_state: &mut PauseMenuState,
    settings: &mut GameSettings,
    settings_events: &mut MessageWriter<SettingsChangedEvent>,
) {
    egui::Window::new("Settings")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(450.0)
        .show(ctx, |ui| {
            draw_settings_header(ui, pause_state, SettingsScreen::Audio);
            
            ui.add_space(10.0);
            ui.heading("🔊 Audio Settings");
            ui.add_space(10.0);
            
            egui::Grid::new("audio_grid")
                .num_columns(2)
                .spacing([40.0, 8.0])
                .show(ui, |ui| {
                    // Master Volume
                    ui.label("Master Volume:");
                    ui.add(egui::Slider::new(&mut settings.master_volume, 0.0..=1.0)
                        .show_value(true)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                    ui.end_row();
                    
                    // Music Volume
                    ui.label("Music Volume:");
                    ui.add(egui::Slider::new(&mut settings.music_volume, 0.0..=1.0)
                        .show_value(true)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                    ui.end_row();
                    
                    // SFX Volume
                    ui.label("Sound Effects:");
                    ui.add(egui::Slider::new(&mut settings.sfx_volume, 0.0..=1.0)
                        .show_value(true)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                    ui.end_row();
                    
                    // Ambient Volume
                    ui.label("Ambient Sounds:");
                    ui.add(egui::Slider::new(&mut settings.ambient_volume, 0.0..=1.0)
                        .show_value(true)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                    ui.end_row();
                    
                    // Voice Volume
                    ui.label("Voice Volume:");
                    ui.add(egui::Slider::new(&mut settings.voice_volume, 0.0..=1.0)
                        .show_value(true)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                    ui.end_row();
                    
                    // Mute when unfocused
                    ui.label("Mute When Unfocused:");
                    ui.checkbox(&mut settings.mute_when_unfocused, "");
                    ui.end_row();
                });
            
            ui.add_space(20.0);
            draw_settings_footer(ui, pause_state, settings, settings_events);
        });
}

/// Draw Controls settings screen
fn draw_settings_controls(
    ctx: &egui::Context,
    pause_state: &mut PauseMenuState,
    settings: &mut GameSettings,
    settings_events: &mut MessageWriter<SettingsChangedEvent>,
) {
    egui::Window::new("Settings")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(450.0)
        .show(ctx, |ui| {
            draw_settings_header(ui, pause_state, SettingsScreen::Controls);
            
            ui.add_space(10.0);
            ui.heading("🎯 Controls Settings");
            ui.add_space(10.0);
            
            egui::Grid::new("controls_grid")
                .num_columns(2)
                .spacing([40.0, 8.0])
                .show(ui, |ui| {
                    // Mouse Sensitivity
                    ui.label("Mouse Sensitivity:");
                    ui.add(egui::Slider::new(&mut settings.mouse_sensitivity, 0.1..=3.0)
                        .show_value(true)
                        .custom_formatter(|v, _| format!("{:.2}", v)));
                    ui.end_row();
                    
                    // Mouse Smoothing
                    ui.label("Mouse Smoothing:");
                    ui.checkbox(&mut settings.mouse_smoothing, "");
                    ui.end_row();
                    
                    // Controller Vibration
                    ui.label("Controller Vibration:");
                    ui.checkbox(&mut settings.controller_vibration, "");
                    ui.end_row();
                });
            
            ui.add_space(15.0);
            ui.separator();
            ui.add_space(10.0);
            
            // Keybindings section (display only for now)
            ui.label(egui::RichText::new("Keybindings").strong());
            ui.add_space(5.0);
            
            egui::Grid::new("keybindings_grid")
                .num_columns(2)
                .spacing([60.0, 4.0])
                .show(ui, |ui| {
                    let bindings = [
                        ("Move Forward", "W"),
                        ("Move Backward", "S"),
                        ("Move Left", "A"),
                        ("Move Right", "D"),
                        ("Jump", "Space"),
                        ("Sprint", "Shift"),
                        ("Crouch", "Ctrl"),
                        ("Pause Menu", "Escape"),
                        ("Toggle Cursor", "Tab"),
                    ];
                    
                    for (action, key) in bindings {
                        ui.label(action);
                        ui.label(egui::RichText::new(key).monospace().strong());
                        ui.end_row();
                    }
                });
            
            ui.add_space(20.0);
            draw_settings_footer(ui, pause_state, settings, settings_events);
        });
}

/// Draw settings footer with Apply/Cancel buttons
fn draw_settings_footer(
    ui: &mut egui::Ui,
    pause_state: &mut PauseMenuState,
    settings: &mut GameSettings,
    settings_events: &mut MessageWriter<SettingsChangedEvent>,
) {
    ui.separator();
    ui.add_space(10.0);
    
    ui.horizontal(|ui| {
        // Reset to Defaults
        if ui.button("Reset to Defaults").clicked() {
            *settings = GameSettings::default();
            settings_events.write(SettingsChangedEvent);
        }
        
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Apply button
            if ui.button("Apply").clicked() {
                pause_state.temp_settings = None;
                settings_events.write(SettingsChangedEvent);
                info!("✅ Settings applied");
            }
            
            // Cancel button
            if ui.button("Cancel").clicked() {
                // Restore original settings
                if let Some(original) = pause_state.temp_settings.take() {
                    *settings = original;
                }
                pause_state.settings_screen = SettingsScreen::None;
            }
        });
    });
}

/// Apply settings to game systems
fn apply_settings(
    mut settings_events: MessageReader<SettingsChangedEvent>,
    settings: Res<GameSettings>,
    mut camera_query: Query<&mut PlayerCamera>,
) {
    for _ in settings_events.read() {
        // Apply mouse sensitivity to camera
        for mut camera in camera_query.iter_mut() {
            camera.sensitivity = settings.mouse_sensitivity * 0.003; // Base sensitivity
        }
        
        // TODO: Apply other settings
        // - VSync: window.present_mode
        // - Fullscreen: window.mode
        // - Audio volumes: audio system
        // - Graphics quality: render settings
        
        info!("🔧 Settings applied: sensitivity={:.2}", settings.mouse_sensitivity);
    }
}

/// Handle character reset
fn handle_reset_character(
    mut reset_events: MessageReader<ResetCharacterEvent>,
    player_service: Res<PlayerService>,
    spawn_locations: Query<(&Transform, &SpawnLocation)>,
    mut character_query: Query<(&mut Transform, &mut LinearVelocity), (With<super::player_plugin::CharacterRoot>, Without<SpawnLocation>)>,
) {
    for _ in reset_events.read() {
        info!("🔄 Resetting character to spawn position...");
        
        // Find spawn position from SpawnLocation entities, or use default
        let (spawn_pos, _) = get_spawn_position_or_default(
            spawn_locations.iter(),
            None, // TODO: Get player team
            player_service.spawn_position,
        );
        
        if let Ok((mut transform, mut velocity)) = character_query.single_mut() {
            transform.translation = spawn_pos;
            transform.rotation = Quat::IDENTITY;
            *velocity = LinearVelocity::default();
            info!("✅ Character reset to {:?}", transform.translation);
        }
    }
}

//! Pause Menu Plugin
//! 
//! Provides a universal pause menu accessible via ESC key with:
//! - Resume: Continue playing
//! - Reset Character: Respawn at spawn point
//! - Settings: Modular settings screens (Gameplay, Graphics, Audio, Controls)
//! - Exit: Quit the game

use bevy::prelude::*;
use avian3d::prelude::LinearVelocity;

use super::player_plugin::{PlayerService, PlayerCamera, SpawnLocation, get_spawn_position_or_default};
use eustress_play_runtime::screen_menu::{pause_card, row_at, settings_card, step, MenuCard, ScreenMenu};

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
    /// The highlighted row of the menu on screen (see `screen_menu`).
    pub selected: usize,
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
        app
            .init_resource::<PauseMenuState>()
            .init_resource::<GameSettings>()
            .add_message::<ResetCharacterEvent>()
            .add_message::<SettingsChangedEvent>()
            .add_systems(Update, (toggle_pause_menu, drive_pause_menu).chain())
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
            pause_state.selected = 1; // back on the row that opened Settings
            return;
        }
        
        pause_state.is_open = !pause_state.is_open;
        if pause_state.is_open {
            pause_state.selected = 0; // Resume, as every opening
        }
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

/// What the menu shows for this state: the main rows, or the settings screen.
fn current_card(pause: &PauseMenuState, settings: &GameSettings) -> MenuCard {
    if pause.settings_screen != SettingsScreen::None {
        settings_card(settings.mouse_sensitivity, pause.selected)
    } else {
        pause_card(pause.selected)
    }
}

/// Close the menu and give the cursor back to play (the Esc path does the same).
fn close_pause_menu(
    pause: &mut PauseMenuState,
    cursor_options: &mut Query<&mut bevy::window::CursorOptions, With<bevy::window::Window>>,
    player_service: &mut PlayerService,
) {
    pause.is_open = false;
    pause.settings_screen = SettingsScreen::None;
    pause.temp_settings = None;
    pause.selected = 0;
    if let Ok(mut cursor) = cursor_options.single_mut() {
        cursor.grab_mode = bevy::window::CursorGrabMode::Locked;
        cursor.visible = false;
    }
    player_service.cursor_locked = true;
}

/// The pause menu's rows, and the keys and clicks that choose them. Drawn by
/// `eustress_play_runtime::screen_menu`: Up and Down, or a click, highlight a
/// row; Enter or a click selects it. On Settings, Left and Right change the
/// mouse sensitivity, the one setting the Player applies so far.
fn drive_pause_menu(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<bevy::input::mouse::MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut pause: ResMut<PauseMenuState>,
    mut settings: ResMut<GameSettings>,
    mut player_service: ResMut<PlayerService>,
    mut cursor_options: Query<&mut bevy::window::CursorOptions, With<bevy::window::Window>>,
    mut settings_events: MessageWriter<SettingsChangedEvent>,
    mut exit: MessageWriter<AppExit>,
    mut screen: ResMut<ScreenMenu>,
) {
    if !pause.is_open {
        screen.0 = None;
        return;
    }
    let in_settings = pause.settings_screen != SettingsScreen::None;
    let card = current_card(&pause, &settings);
    let rows = card.rows.len();

    if keys.just_pressed(KeyCode::ArrowDown) {
        pause.selected = step(pause.selected, rows, 1);
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        pause.selected = step(pause.selected, rows, -1);
    }

    // A click on a row chooses it. The cursor and the menu are both in window
    // logical px.
    let mut choice: Option<usize> = None;
    if buttons.just_pressed(bevy::input::mouse::MouseButton::Left) {
        if let Ok(window) = windows.single() {
            if let Some(point) = window.cursor_position() {
                let size = Vec2::new(window.width(), window.height());
                if let Some(row) = row_at(&card, size, point) {
                    pause.selected = row;
                    choice = Some(row);
                }
            }
        }
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        choice = Some(pause.selected);
    }

    if in_settings {
        // Left and Right move the sensitivity, on the row it sits on, in steps
        // of 0.1 kept between 0.1 and 5.0.
        let direction = if keys.just_pressed(KeyCode::ArrowRight) {
            1.0
        } else if keys.just_pressed(KeyCode::ArrowLeft) {
            -1.0
        } else {
            0.0
        };
        if direction != 0.0 && pause.selected == 0 {
            let next = ((settings.mouse_sensitivity + 0.1 * direction).clamp(0.1, 5.0) * 10.0).round() / 10.0;
            if (next - settings.mouse_sensitivity).abs() > 1e-4 {
                settings.mouse_sensitivity = next;
                settings_events.write(SettingsChangedEvent);
            }
        }
        // Back is the only other row: choosing it returns to the main menu.
        if matches!(choice, Some(row) if row != 0) {
            pause.settings_screen = SettingsScreen::None;
            pause.selected = 1;
        }
    } else {
        match choice {
            Some(0) => close_pause_menu(&mut pause, &mut cursor_options, &mut player_service),
            Some(1) => {
                pause.settings_screen = SettingsScreen::Controls;
                pause.selected = 0;
            }
            Some(_) => {
                exit.write(AppExit::Success);
            }
            None => {}
        }
    }

    screen.0 = if pause.is_open { Some(current_card(&pause, &settings)) } else { None };
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

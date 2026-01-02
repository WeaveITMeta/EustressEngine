// Service Properties - Properties for Roblox-like services
// Workspace, Players, Lighting, ReplicatedStorage, etc.

use bevy::prelude::*;
use bevy_egui::egui;
use super::explorer::ServiceType;
use eustress_common::services::workspace::Workspace;

// ============================================================================
// Service Data Resources
// ============================================================================

/// Workspace service properties (like Roblox's Workspace)
#[derive(Resource, Clone)]
pub struct WorkspaceService {
    /// Name of the workspace
    pub name: String,
    /// Gravity vector (default: -9.81 m/s² on Y axis)
    pub gravity: f32,
    /// Whether parts fall when not anchored
    pub fallen_parts_destroy_height: f32,
    /// Air density for aerodynamics
    pub air_density: f32,
    /// Allow third-person camera
    pub allow_third_party_sales: bool,
    /// Client animator throttling
    pub client_animator_throttling: bool,
    /// Streaming enabled
    pub streaming_enabled: bool,
    /// Streaming target radius
    pub streaming_target_radius: f32,
    /// Streaming minimum radius
    pub streaming_min_radius: f32,
}

impl Default for WorkspaceService {
    fn default() -> Self {
        Self {
            name: "Workspace".to_string(),
            gravity: 196.2,
            fallen_parts_destroy_height: -500.0,
            air_density: 0.0012,
            allow_third_party_sales: false,
            client_animator_throttling: true,
            streaming_enabled: false,
            streaming_target_radius: 1024.0,
            streaming_min_radius: 64.0,
        }
    }
}

/// Lighting service properties
#[derive(Resource, Clone)]
pub struct LightingService {
    pub name: String,
    /// Ambient light color
    pub ambient: Color,
    /// Outdoor ambient light
    pub outdoor_ambient: Color,
    /// Brightness multiplier (0-10)
    pub brightness: f32,
    /// Color shift for bottom surfaces
    pub color_shift_bottom: Color,
    /// Color shift for top surfaces
    pub color_shift_top: Color,
    /// Environment diffuse scale
    pub environment_diffuse_scale: f32,
    /// Environment specular scale
    pub environment_specular_scale: f32,
    /// Global shadows enabled
    pub global_shadows: bool,
    /// Time of day (0-24 hours)
    pub clock_time: f32,
    /// Geographic latitude (-90 to 90)
    pub geographic_latitude: f32,
    /// Exposure compensation
    pub exposure_compensation: f32,
    /// Shadow softness
    pub shadow_softness: f32,
}

impl Default for LightingService {
    fn default() -> Self {
        Self {
            name: "Lighting".to_string(),
            ambient: Color::srgb(0.5, 0.5, 0.5),
            outdoor_ambient: Color::srgb(0.5, 0.5, 0.5),
            brightness: 1.0,
            color_shift_bottom: Color::BLACK,
            color_shift_top: Color::BLACK,
            environment_diffuse_scale: 1.0,
            environment_specular_scale: 1.0,
            global_shadows: true,
            clock_time: 14.0, // 2 PM
            geographic_latitude: 41.7,
            exposure_compensation: 0.0,
            shadow_softness: 0.2,
        }
    }
}

/// Players service properties
#[derive(Resource, Clone)]
pub struct PlayersService {
    pub name: String,
    /// Maximum players allowed
    pub max_players: u32,
    /// Preferred players (for matchmaking)
    pub preferred_players: u32,
    /// Character auto-loads
    pub character_auto_loads: bool,
    /// Respawn time in seconds
    pub respawn_time: f32,
}

impl Default for PlayersService {
    fn default() -> Self {
        Self {
            name: "Players".to_string(),
            max_players: 50,
            preferred_players: 10,
            character_auto_loads: true,
            respawn_time: 5.0,
        }
    }
}

/// ReplicatedStorage service properties
#[derive(Resource, Clone)]
pub struct ReplicatedStorageService {
    pub name: String,
}

impl Default for ReplicatedStorageService {
    fn default() -> Self {
        Self {
            name: "ReplicatedStorage".to_string(),
        }
    }
}

/// ServerStorage service properties
#[derive(Resource, Clone)]
pub struct ServerStorageService {
    pub name: String,
}

impl Default for ServerStorageService {
    fn default() -> Self {
        Self {
            name: "ServerStorage".to_string(),
        }
    }
}

/// ServerScriptService properties
#[derive(Resource, Clone)]
pub struct ServerScriptServiceService {
    pub name: String,
    /// Load string enabled
    pub load_string_enabled: bool,
}

impl Default for ServerScriptServiceService {
    fn default() -> Self {
        Self {
            name: "ServerScriptService".to_string(),
            load_string_enabled: false,
        }
    }
}

/// StarterGui service properties
#[derive(Resource, Clone)]
pub struct StarterGuiService {
    pub name: String,
    /// Show development GUI
    pub show_development_gui: bool,
    /// Screen orientation
    pub screen_orientation: ScreenOrientation,
    /// Reset player gui on spawn
    pub reset_player_gui_on_spawn: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScreenOrientation {
    LandscapeLeft,
    LandscapeRight,
    LandscapeSensor,
    Portrait,
    Sensor,
}

impl Default for StarterGuiService {
    fn default() -> Self {
        Self {
            name: "StarterGui".to_string(),
            show_development_gui: false,
            screen_orientation: ScreenOrientation::LandscapeSensor,
            reset_player_gui_on_spawn: true,
        }
    }
}

/// StarterPack service properties
#[derive(Resource, Clone)]
pub struct StarterPackService {
    pub name: String,
}

impl Default for StarterPackService {
    fn default() -> Self {
        Self {
            name: "StarterPack".to_string(),
        }
    }
}

/// StarterPlayer service properties
#[derive(Resource, Clone)]
pub struct StarterPlayerService {
    pub name: String,
    /// Allow custom animations
    pub allow_custom_animations: bool,
    /// Camera max zoom distance
    pub camera_max_zoom_distance: f32,
    /// Camera min zoom distance
    pub camera_min_zoom_distance: f32,
    /// Camera mode
    pub camera_mode: CameraMode,
    /// Character walk speed
    pub character_walk_speed: f32,
    /// Character jump power
    pub character_jump_power: f32,
    /// Character max slope angle
    pub character_max_slope_angle: f32,
    /// Health display distance
    pub health_display_distance: f32,
    /// Name display distance
    pub name_display_distance: f32,
    /// Load character appearance
    pub load_character_appearance: bool,
    /// User choice of camera
    pub user_choice_of_camera: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMode {
    Classic,
    LockFirstPerson,
}

impl Default for StarterPlayerService {
    fn default() -> Self {
        Self {
            name: "StarterPlayer".to_string(),
            allow_custom_animations: true,
            camera_max_zoom_distance: 400.0,
            camera_min_zoom_distance: 0.5,
            camera_mode: CameraMode::Classic,
            character_walk_speed: 16.0,
            character_jump_power: 50.0,
            character_max_slope_angle: 89.0,
            health_display_distance: 100.0,
            name_display_distance: 100.0,
            load_character_appearance: true,
            user_choice_of_camera: true,
        }
    }
}

/// SoundService properties
#[derive(Resource, Clone)]
pub struct SoundServiceService {
    pub name: String,
    /// Ambient reverb
    pub ambient_reverb: ReverbType,
    /// Distance factor
    pub distance_factor: f32,
    /// Doppler scale
    pub doppler_scale: f32,
    /// Rolloff scale
    pub rolloff_scale: f32,
    /// Respect filtering enabled
    pub respect_filtering_enabled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReverbType {
    NoReverb,
    GenericReverb,
    PaddedCell,
    Room,
    Bathroom,
    LivingRoom,
    StoneRoom,
    Auditorium,
    ConcertHall,
    Cave,
    Arena,
    Hangar,
    CarpetedHallway,
    Hallway,
    StoneCorridor,
    Alley,
    Forest,
    City,
    Mountains,
    Quarry,
    Plain,
    ParkingLot,
    SewerPipe,
    UnderWater,
}

impl Default for SoundServiceService {
    fn default() -> Self {
        Self {
            name: "SoundService".to_string(),
            ambient_reverb: ReverbType::NoReverb,
            distance_factor: 3.33,
            doppler_scale: 1.0,
            rolloff_scale: 1.0,
            respect_filtering_enabled: false,
        }
    }
}

/// Teams service properties
#[derive(Resource, Clone)]
pub struct TeamsService {
    pub name: String,
}

impl Default for TeamsService {
    fn default() -> Self {
        Self {
            name: "Teams".to_string(),
        }
    }
}

/// Chat service properties
#[derive(Resource, Clone)]
pub struct ChatService {
    pub name: String,
    /// Bubble chat enabled
    pub bubble_chat_enabled: bool,
    /// Load default chat
    pub load_default_chat: bool,
}

impl Default for ChatService {
    fn default() -> Self {
        Self {
            name: "Chat".to_string(),
            bubble_chat_enabled: true,
            load_default_chat: true,
        }
    }
}

// ============================================================================
// Service Properties Panel
// ============================================================================

/// Render properties for a selected service
pub fn render_service_properties(
    ui: &mut egui::Ui,
    service: ServiceType,
    world: &mut World,
) {
    match service {
        ServiceType::Workspace => render_workspace_properties(ui, world),
        ServiceType::Lighting => render_lighting_properties(ui, world),
        ServiceType::Players => render_players_properties(ui, world),
        ServiceType::SoulService => render_soul_service_properties(ui, world),
        ServiceType::ServerStorage => render_server_storage_properties(ui, world),
        ServiceType::StarterGui => render_starter_gui_properties(ui, world),
        ServiceType::StarterPack => render_starter_pack_properties(ui, world),
        ServiceType::StarterPlayer => render_starter_player_properties(ui, world),
        ServiceType::SoundService => render_sound_service_properties(ui, world),
        ServiceType::Teams => render_teams_properties(ui, world),
        ServiceType::Chat => render_chat_properties(ui, world),
        ServiceType::LocalizationService => render_localization_service_properties(ui, world),
        ServiceType::TestService => render_test_service_properties(ui, world),
    }
}

fn render_workspace_properties(ui: &mut egui::Ui, world: &mut World) {
    // Get the actual Workspace resource (runtime)
    let mut workspace_runtime = world.get_resource_or_insert_with(Workspace::default).clone();
    let mut workspace_ui = world.get_resource_or_insert_with(WorkspaceService::default).clone();
    let mut changed = false;
    
    ui.heading("⚙ Workspace");
    ui.separator();
    
    egui::CollapsingHeader::new("🌍 Physics")
        .default_open(true)
        .show(ui, |ui| {
            // Gravity - edit the Y component magnitude (positive value, applied as negative)
            let mut gravity_magnitude = workspace_runtime.gravity.y.abs();
            ui.horizontal(|ui| {
                ui.label("Gravity:");
                if ui.add(egui::DragValue::new(&mut gravity_magnitude).speed(0.1).suffix(" m/s²").range(0.0..=100.0)).changed() {
                    workspace_runtime.gravity.y = -gravity_magnitude;
                    workspace_ui.gravity = gravity_magnitude;
                    changed = true;
                }
            });
            
            // FallenPartsDestroyHeight - edit the fall_height
            ui.horizontal(|ui| {
                ui.label("FallenPartsDestroyHeight:");
                if ui.add(egui::DragValue::new(&mut workspace_runtime.fall_height).speed(1.0).suffix(" m")).changed() {
                    workspace_ui.fallen_parts_destroy_height = workspace_runtime.fall_height;
                    changed = true;
                }
            });
            ui.indent("fpd_help", |ui| {
                ui.weak("Parts below this Y position will be destroyed");
            });
            
            // Max Entity Speed
            ui.horizontal(|ui| {
                ui.label("Max Entity Speed:");
                changed |= ui.add(egui::DragValue::new(&mut workspace_runtime.max_entity_speed).speed(1.0).suffix(" m/s").range(0.0..=1000.0)).changed();
            });
            
            // Teleport Threshold
            ui.horizontal(|ui| {
                ui.label("Teleport Threshold:");
                changed |= ui.add(egui::DragValue::new(&mut workspace_runtime.teleport_threshold).speed(1.0).suffix(" m").range(1.0..=1000.0)).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Air Density:");
                changed |= ui.add(egui::DragValue::new(&mut workspace_ui.air_density).speed(0.0001).range(0.0..=1.0)).changed();
            });
        });
    
    egui::CollapsingHeader::new("📡 Streaming")
        .default_open(false)
        .show(ui, |ui| {
            if ui.checkbox(&mut workspace_runtime.streaming_enabled, "Streaming Enabled").changed() {
                workspace_ui.streaming_enabled = workspace_runtime.streaming_enabled;
                changed = true;
            }
            
            if workspace_runtime.streaming_enabled {
                ui.horizontal(|ui| {
                    ui.label("Target Radius:");
                    if ui.add(egui::DragValue::new(&mut workspace_runtime.streaming_target_radius).speed(10.0).suffix(" m").range(64.0..=4096.0)).changed() {
                        workspace_ui.streaming_target_radius = workspace_runtime.streaming_target_radius;
                        changed = true;
                    }
                });
                
                ui.horizontal(|ui| {
                    ui.label("Min Radius:");
                    if ui.add(egui::DragValue::new(&mut workspace_runtime.streaming_min_radius).speed(10.0).suffix(" m").range(32.0..=1024.0)).changed() {
                        workspace_ui.streaming_min_radius = workspace_runtime.streaming_min_radius;
                        changed = true;
                    }
                });
            }
        });
    
    egui::CollapsingHeader::new("🌐 World Bounds")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Min X:");
                changed |= ui.add(egui::DragValue::new(&mut workspace_runtime.world_bounds_min.x).speed(100.0)).changed();
                ui.label("Max X:");
                changed |= ui.add(egui::DragValue::new(&mut workspace_runtime.world_bounds_max.x).speed(100.0)).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Min Y:");
                changed |= ui.add(egui::DragValue::new(&mut workspace_runtime.world_bounds_min.y).speed(100.0)).changed();
                ui.label("Max Y:");
                changed |= ui.add(egui::DragValue::new(&mut workspace_runtime.world_bounds_max.y).speed(100.0)).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Min Z:");
                changed |= ui.add(egui::DragValue::new(&mut workspace_runtime.world_bounds_min.z).speed(100.0)).changed();
                ui.label("Max Z:");
                changed |= ui.add(egui::DragValue::new(&mut workspace_runtime.world_bounds_max.z).speed(100.0)).changed();
            });
        });
    
    egui::CollapsingHeader::new("⚙ Behavior")
        .default_open(false)
        .show(ui, |ui| {
            changed |= ui.checkbox(&mut workspace_ui.client_animator_throttling, "Client Animator Throttling").changed();
            changed |= ui.checkbox(&mut workspace_ui.allow_third_party_sales, "Allow Third Party Sales").changed();
        });
    
    if changed {
        world.insert_resource(workspace_runtime);
        world.insert_resource(workspace_ui);
    }
}

fn render_lighting_properties(ui: &mut egui::Ui, world: &mut World) {
    let mut lighting = world.get_resource_or_insert_with(LightingService::default).clone();
    let mut changed = false;
    
    ui.heading("💡 Lighting");
    ui.separator();
    
    egui::CollapsingHeader::new("🌅 Time & Position")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Clock Time:");
                changed |= ui.add(egui::Slider::new(&mut lighting.clock_time, 0.0..=24.0).suffix("h")).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Geographic Latitude:");
                changed |= ui.add(egui::Slider::new(&mut lighting.geographic_latitude, -90.0..=90.0).suffix("°")).changed();
            });
        });
    
    egui::CollapsingHeader::new("🎨 Colors")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Ambient:");
                let [r, g, b, _] = lighting.ambient.to_srgba().to_f32_array();
                let mut color = [r, g, b];
                if ui.color_edit_button_rgb(&mut color).changed() {
                    lighting.ambient = Color::srgb(color[0], color[1], color[2]);
                    changed = true;
                }
            });
            
            ui.horizontal(|ui| {
                ui.label("Outdoor Ambient:");
                let [r, g, b, _] = lighting.outdoor_ambient.to_srgba().to_f32_array();
                let mut color = [r, g, b];
                if ui.color_edit_button_rgb(&mut color).changed() {
                    lighting.outdoor_ambient = Color::srgb(color[0], color[1], color[2]);
                    changed = true;
                }
            });
            
            ui.horizontal(|ui| {
                ui.label("Color Shift Top:");
                let [r, g, b, _] = lighting.color_shift_top.to_srgba().to_f32_array();
                let mut color = [r, g, b];
                if ui.color_edit_button_rgb(&mut color).changed() {
                    lighting.color_shift_top = Color::srgb(color[0], color[1], color[2]);
                    changed = true;
                }
            });
            
            ui.horizontal(|ui| {
                ui.label("Color Shift Bottom:");
                let [r, g, b, _] = lighting.color_shift_bottom.to_srgba().to_f32_array();
                let mut color = [r, g, b];
                if ui.color_edit_button_rgb(&mut color).changed() {
                    lighting.color_shift_bottom = Color::srgb(color[0], color[1], color[2]);
                    changed = true;
                }
            });
        });
    
    egui::CollapsingHeader::new("☀ Intensity")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Brightness:");
                changed |= ui.add(egui::Slider::new(&mut lighting.brightness, 0.0..=10.0)).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Exposure Compensation:");
                changed |= ui.add(egui::Slider::new(&mut lighting.exposure_compensation, -3.0..=3.0)).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Environment Diffuse Scale:");
                changed |= ui.add(egui::Slider::new(&mut lighting.environment_diffuse_scale, 0.0..=1.0)).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Environment Specular Scale:");
                changed |= ui.add(egui::Slider::new(&mut lighting.environment_specular_scale, 0.0..=1.0)).changed();
            });
        });
    
    egui::CollapsingHeader::new("🌑 Shadows")
        .default_open(false)
        .show(ui, |ui| {
            changed |= ui.checkbox(&mut lighting.global_shadows, "Global Shadows").changed();
            
            ui.horizontal(|ui| {
                ui.label("Shadow Softness:");
                changed |= ui.add(egui::Slider::new(&mut lighting.shadow_softness, 0.0..=1.0)).changed();
            });
        });
    
    if changed {
        world.insert_resource(lighting);
    }
}

fn render_players_properties(ui: &mut egui::Ui, world: &mut World) {
    let mut players = world.get_resource_or_insert_with(PlayersService::default).clone();
    let mut changed = false;
    
    ui.heading("👥 Players");
    ui.separator();
    
    egui::CollapsingHeader::new("⚙ Configuration")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Max Players:");
                changed |= ui.add(egui::DragValue::new(&mut players.max_players).range(1..=100)).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Preferred Players:");
                changed |= ui.add(egui::DragValue::new(&mut players.preferred_players).range(1..=100)).changed();
            });
            
            changed |= ui.checkbox(&mut players.character_auto_loads, "Character Auto Loads").changed();
            
            ui.horizontal(|ui| {
                ui.label("Respawn Time:");
                changed |= ui.add(egui::DragValue::new(&mut players.respawn_time).speed(0.5).suffix("s").range(0.0..=60.0)).changed();
            });
        });
    
    if changed {
        world.insert_resource(players);
    }
}

fn render_replicated_storage_properties(ui: &mut egui::Ui, _world: &mut World) {
    ui.heading("📦 ReplicatedStorage");
    ui.separator();
    ui.label("Container for objects replicated to all clients.");
    ui.add_space(10.0);
    ui.weak("Drag objects here to make them available to all players.");
}

fn render_server_storage_properties(ui: &mut egui::Ui, _world: &mut World) {
    ui.heading("🔒 ServerStorage");
    ui.separator();
    ui.label("Container for server-only objects.");
    ui.add_space(10.0);
    ui.weak("Objects here are not replicated to clients.");
}

fn render_server_script_service_properties(ui: &mut egui::Ui, world: &mut World) {
    let mut service = world.get_resource_or_insert_with(ServerScriptServiceService::default).clone();
    let mut changed = false;
    
    ui.heading("📜 ServerScriptService");
    ui.separator();
    
    egui::CollapsingHeader::new("⚙ Configuration")
        .default_open(true)
        .show(ui, |ui| {
            changed |= ui.checkbox(&mut service.load_string_enabled, "LoadString Enabled").changed();
            if service.load_string_enabled {
                ui.colored_label(egui::Color32::YELLOW, "⚠ Security risk: LoadString is enabled");
            }
        });
    
    if changed {
        world.insert_resource(service);
    }
}

fn render_starter_gui_properties(ui: &mut egui::Ui, world: &mut World) {
    let mut service = world.get_resource_or_insert_with(StarterGuiService::default).clone();
    let mut changed = false;
    
    ui.heading("🖼 StarterGui");
    ui.separator();
    
    egui::CollapsingHeader::new("⚙ Configuration")
        .default_open(true)
        .show(ui, |ui| {
            changed |= ui.checkbox(&mut service.show_development_gui, "Show Development GUI").changed();
            changed |= ui.checkbox(&mut service.reset_player_gui_on_spawn, "Reset Player GUI On Spawn").changed();
            
            ui.horizontal(|ui| {
                ui.label("Screen Orientation:");
                egui::ComboBox::from_id_salt("screen_orientation")
                    .selected_text(format!("{:?}", service.screen_orientation))
                    .show_ui(ui, |ui| {
                        changed |= ui.selectable_value(&mut service.screen_orientation, ScreenOrientation::LandscapeLeft, "Landscape Left").changed();
                        changed |= ui.selectable_value(&mut service.screen_orientation, ScreenOrientation::LandscapeRight, "Landscape Right").changed();
                        changed |= ui.selectable_value(&mut service.screen_orientation, ScreenOrientation::LandscapeSensor, "Landscape Sensor").changed();
                        changed |= ui.selectable_value(&mut service.screen_orientation, ScreenOrientation::Portrait, "Portrait").changed();
                        changed |= ui.selectable_value(&mut service.screen_orientation, ScreenOrientation::Sensor, "Sensor").changed();
                    });
            });
        });
    
    if changed {
        world.insert_resource(service);
    }
}

fn render_starter_pack_properties(ui: &mut egui::Ui, _world: &mut World) {
    ui.heading("🎒 StarterPack");
    ui.separator();
    ui.label("Container for tools given to players on spawn.");
    ui.add_space(10.0);
    ui.weak("Add Tool objects here to give them to all players.");
}

fn render_starter_player_properties(ui: &mut egui::Ui, world: &mut World) {
    let mut service = world.get_resource_or_insert_with(StarterPlayerService::default).clone();
    let mut changed = false;
    
    ui.heading("🏃 StarterPlayer");
    ui.separator();
    
    egui::CollapsingHeader::new("📷 Camera")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Camera Mode:");
                egui::ComboBox::from_id_salt("camera_mode")
                    .selected_text(format!("{:?}", service.camera_mode))
                    .show_ui(ui, |ui| {
                        changed |= ui.selectable_value(&mut service.camera_mode, CameraMode::Classic, "Classic").changed();
                        changed |= ui.selectable_value(&mut service.camera_mode, CameraMode::LockFirstPerson, "Lock First Person").changed();
                    });
            });
            
            ui.horizontal(|ui| {
                ui.label("Max Zoom Distance:");
                changed |= ui.add(egui::DragValue::new(&mut service.camera_max_zoom_distance).speed(1.0).range(0.5..=1000.0)).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Min Zoom Distance:");
                changed |= ui.add(egui::DragValue::new(&mut service.camera_min_zoom_distance).speed(0.1).range(0.5..=100.0)).changed();
            });
            
            changed |= ui.checkbox(&mut service.user_choice_of_camera, "User Choice of Camera").changed();
        });
    
    egui::CollapsingHeader::new("🏃 Character")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Walk Speed:");
                changed |= ui.add(egui::DragValue::new(&mut service.character_walk_speed).speed(0.5).range(0.0..=100.0)).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Jump Power:");
                changed |= ui.add(egui::DragValue::new(&mut service.character_jump_power).speed(1.0).range(0.0..=500.0)).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Max Slope Angle:");
                changed |= ui.add(egui::DragValue::new(&mut service.character_max_slope_angle).speed(1.0).suffix("°").range(0.0..=89.0)).changed();
            });
            
            changed |= ui.checkbox(&mut service.allow_custom_animations, "Allow Custom Animations").changed();
            changed |= ui.checkbox(&mut service.load_character_appearance, "Load Character Appearance").changed();
        });
    
    egui::CollapsingHeader::new("🏷 Display")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Name Display Distance:");
                changed |= ui.add(egui::DragValue::new(&mut service.name_display_distance).speed(1.0).range(0.0..=1000.0)).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Health Display Distance:");
                changed |= ui.add(egui::DragValue::new(&mut service.health_display_distance).speed(1.0).range(0.0..=1000.0)).changed();
            });
        });
    
    if changed {
        world.insert_resource(service);
    }
}

fn render_sound_service_properties(ui: &mut egui::Ui, world: &mut World) {
    let mut service = world.get_resource_or_insert_with(SoundServiceService::default).clone();
    let mut changed = false;
    
    ui.heading("🔊 SoundService");
    ui.separator();
    
    egui::CollapsingHeader::new("🎵 Audio Settings")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Distance Factor:");
                changed |= ui.add(egui::DragValue::new(&mut service.distance_factor).speed(0.1).range(0.0..=100.0)).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Doppler Scale:");
                changed |= ui.add(egui::DragValue::new(&mut service.doppler_scale).speed(0.1).range(0.0..=10.0)).changed();
            });
            
            ui.horizontal(|ui| {
                ui.label("Rolloff Scale:");
                changed |= ui.add(egui::DragValue::new(&mut service.rolloff_scale).speed(0.1).range(0.0..=10.0)).changed();
            });
            
            changed |= ui.checkbox(&mut service.respect_filtering_enabled, "Respect Filtering Enabled").changed();
        });
    
    if changed {
        world.insert_resource(service);
    }
}

fn render_teams_properties(ui: &mut egui::Ui, _world: &mut World) {
    ui.heading("🏁 Teams");
    ui.separator();
    ui.label("Container for Team objects.");
    ui.add_space(10.0);
    ui.weak("Add Team objects here to create teams for players.");
}

fn render_chat_properties(ui: &mut egui::Ui, world: &mut World) {
    let mut service = world.get_resource_or_insert_with(ChatService::default).clone();
    let mut changed = false;
    
    ui.heading("💬 Chat");
    ui.separator();
    
    egui::CollapsingHeader::new("⚙ Configuration")
        .default_open(true)
        .show(ui, |ui| {
            changed |= ui.checkbox(&mut service.bubble_chat_enabled, "Bubble Chat Enabled").changed();
            changed |= ui.checkbox(&mut service.load_default_chat, "Load Default Chat").changed();
        });
    
    if changed {
        world.insert_resource(service);
    }
}

fn render_soul_service_properties(ui: &mut egui::Ui, _world: &mut World) {
    ui.heading("📝 SoulService");
    ui.separator();
    ui.label("Primary container for Soul scripts.");
    ui.add_space(10.0);
    ui.weak("Soul scripts are markdown files that compile to Rust.");
    ui.weak("Claude API generates optimized client-server code.");
    ui.add_space(10.0);
    ui.label("Double-click a Soul Script to open the editor.");
}

fn render_localization_service_properties(ui: &mut egui::Ui, _world: &mut World) {
    ui.heading("🌐 LocalizationService");
    ui.separator();
    ui.label("Manages game localization and translations.");
    ui.add_space(10.0);
    ui.weak("Add LocalizationTable objects for multi-language support.");
}

fn render_test_service_properties(ui: &mut egui::Ui, _world: &mut World) {
    ui.heading("🧪 TestService");
    ui.separator();
    ui.label("Service for running automated tests.");
    ui.add_space(10.0);
    ui.weak("Add TestEz scripts here for unit testing.");
}

// ============================================================================
// Plugin
// ============================================================================

pub struct ServicePropertiesPlugin;

impl Plugin for ServicePropertiesPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<WorkspaceService>()
            .init_resource::<LightingService>()
            .init_resource::<PlayersService>()
            .init_resource::<ReplicatedStorageService>()
            .init_resource::<ServerStorageService>()
            .init_resource::<ServerScriptServiceService>()
            .init_resource::<StarterGuiService>()
            .init_resource::<StarterPackService>()
            .init_resource::<StarterPlayerService>()
            .init_resource::<SoundServiceService>()
            .init_resource::<TeamsService>()
            .init_resource::<ChatService>();
    }
}

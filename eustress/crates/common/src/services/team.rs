//! # Team Service
//! 
//! Team management for multiplayer games (like Roblox's Teams service).
//! 
//! ## Classes
//! - `TeamService`: Global team management resource
//! - `Team`: A team definition with color, spawn rules, etc.
//! - `TeamMember`: Component marking an entity's team membership
//! - `TeamComponent`: Bevy component for Team entities in the scene
//!
//! ## Features
//! - Team colors for player nametags and UI
//! - Auto-assignment with team balancing
//! - Spawn location filtering by team
//! - Friendly fire configuration
//! - Team-based scoring

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

// ============================================================================
// BrickColor - Roblox-style color palette
// ============================================================================

/// Predefined team colors (Roblox BrickColor style)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum TeamColor {
    #[default]
    White,
    Grey,
    Black,
    BrightRed,
    BrightOrange,
    BrightYellow,
    BrightGreen,
    BrightBlue,
    BrightViolet,
    ReallyRed,
    ReallyBlue,
    Cyan,
    Magenta,
    Pink,
    Lime,
    Navy,
    Teal,
    Maroon,
    Olive,
    Brown,
    Gold,
    Silver,
    Custom([u8; 3]),
}

impl TeamColor {
    /// Get RGBA color values (0.0-1.0)
    pub fn to_rgba(&self) -> [f32; 4] {
        match self {
            TeamColor::White => [1.0, 1.0, 1.0, 1.0],
            TeamColor::Grey => [0.5, 0.5, 0.5, 1.0],
            TeamColor::Black => [0.1, 0.1, 0.1, 1.0],
            TeamColor::BrightRed => [0.9, 0.2, 0.2, 1.0],
            TeamColor::BrightOrange => [0.9, 0.5, 0.1, 1.0],
            TeamColor::BrightYellow => [0.9, 0.9, 0.1, 1.0],
            TeamColor::BrightGreen => [0.2, 0.9, 0.2, 1.0],
            TeamColor::BrightBlue => [0.2, 0.4, 0.9, 1.0],
            TeamColor::BrightViolet => [0.6, 0.2, 0.9, 1.0],
            TeamColor::ReallyRed => [1.0, 0.0, 0.0, 1.0],
            TeamColor::ReallyBlue => [0.0, 0.0, 1.0, 1.0],
            TeamColor::Cyan => [0.0, 1.0, 1.0, 1.0],
            TeamColor::Magenta => [1.0, 0.0, 1.0, 1.0],
            TeamColor::Pink => [1.0, 0.6, 0.8, 1.0],
            TeamColor::Lime => [0.5, 1.0, 0.0, 1.0],
            TeamColor::Navy => [0.0, 0.1, 0.4, 1.0],
            TeamColor::Teal => [0.0, 0.5, 0.5, 1.0],
            TeamColor::Maroon => [0.5, 0.0, 0.0, 1.0],
            TeamColor::Olive => [0.5, 0.5, 0.0, 1.0],
            TeamColor::Brown => [0.4, 0.2, 0.1, 1.0],
            TeamColor::Gold => [1.0, 0.84, 0.0, 1.0],
            TeamColor::Silver => [0.75, 0.75, 0.75, 1.0],
            TeamColor::Custom([r, g, b]) => [
                *r as f32 / 255.0,
                *g as f32 / 255.0,
                *b as f32 / 255.0,
                1.0,
            ],
        }
    }
    
    /// Convert to Bevy Color
    pub fn to_bevy_color(&self) -> Color {
        let [r, g, b, a] = self.to_rgba();
        Color::srgba(r, g, b, a)
    }
    
    /// Create from RGB values (0-255)
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        TeamColor::Custom([r, g, b])
    }
}

// ============================================================================
// TeamService Resource
// ============================================================================

/// TeamService - manages all teams (like Roblox's Teams service)
#[derive(Resource, Reflect, Clone, Debug, Default)]
#[reflect(Resource)]
pub struct TeamService {
    /// All registered teams
    pub teams: Vec<Team>,
    
    /// Auto-assign players to teams on join
    pub auto_assign: bool,
    
    /// Balance teams when auto-assigning (put player on smallest team)
    pub balance_teams: bool,
    
    /// Allow friendly fire (damage teammates)
    pub friendly_fire: bool,
    
    /// Show team colors on player nametags
    pub show_team_colors: bool,
    
    /// Next team ID to assign
    next_team_id: u32,
}

impl TeamService {
    pub fn new() -> Self {
        Self {
            next_team_id: 1,
            show_team_colors: true,
            ..default()
        }
    }
    
    /// Add a team with auto-generated ID
    pub fn add_team(&mut self, mut team: Team) -> u32 {
        if team.id == 0 {
            team.id = self.next_team_id;
            self.next_team_id += 1;
        } else if team.id >= self.next_team_id {
            self.next_team_id = team.id + 1;
        }
        let id = team.id;
        self.teams.push(team);
        id
    }
    
    /// Create a new team with name and color
    pub fn create_team(&mut self, name: impl Into<String>, color: TeamColor) -> u32 {
        let team = Team::with_color(self.next_team_id, name, color);
        self.add_team(team)
    }
    
    /// Remove a team by ID
    pub fn remove_team(&mut self, team_id: u32) -> Option<Team> {
        if let Some(pos) = self.teams.iter().position(|t| t.id == team_id) {
            Some(self.teams.remove(pos))
        } else {
            None
        }
    }
    
    /// Get team by name
    pub fn get_team(&self, name: &str) -> Option<&Team> {
        self.teams.iter().find(|t| t.name == name)
    }
    
    /// Get mutable team by name
    pub fn get_team_mut(&mut self, name: &str) -> Option<&mut Team> {
        self.teams.iter_mut().find(|t| t.name == name)
    }
    
    /// Get team by ID
    pub fn get_team_by_id(&self, id: u32) -> Option<&Team> {
        self.teams.iter().find(|t| t.id == id)
    }
    
    /// Get mutable team by ID
    pub fn get_team_by_id_mut(&mut self, id: u32) -> Option<&mut Team> {
        self.teams.iter_mut().find(|t| t.id == id)
    }
    
    /// Get team color by ID
    pub fn get_team_color(&self, team_id: u32) -> Option<TeamColor> {
        self.get_team_by_id(team_id).map(|t| t.team_color)
    }
    
    /// Get team name by ID
    pub fn get_team_name(&self, team_id: u32) -> Option<&str> {
        self.get_team_by_id(team_id).map(|t| t.name.as_str())
    }
    
    /// Find the best team to auto-assign a player to
    pub fn find_auto_assign_team(&self) -> Option<u32> {
        let assignable: Vec<_> = self.teams.iter()
            .filter(|t| t.auto_assignable && !t.is_full())
            .collect();
        
        if assignable.is_empty() {
            return None;
        }
        
        if self.balance_teams {
            // Find team with fewest players
            assignable.iter()
                .min_by_key(|t| t.player_count)
                .map(|t| t.id)
        } else {
            // Just pick first available
            Some(assignable[0].id)
        }
    }
    
    /// Increment player count for a team
    pub fn player_joined(&mut self, team_id: u32) {
        if let Some(team) = self.get_team_by_id_mut(team_id) {
            team.player_count += 1;
        }
    }
    
    /// Decrement player count for a team
    pub fn player_left(&mut self, team_id: u32) {
        if let Some(team) = self.get_team_by_id_mut(team_id) {
            team.player_count = team.player_count.saturating_sub(1);
        }
    }
    
    /// Get all team IDs
    pub fn team_ids(&self) -> Vec<u32> {
        self.teams.iter().map(|t| t.id).collect()
    }
    
    /// Get team count
    pub fn team_count(&self) -> usize {
        self.teams.len()
    }
    
    /// Create default teams (Red vs Blue)
    pub fn with_default_teams() -> Self {
        let mut service = Self::new();
        service.create_team("Red Team", TeamColor::BrightRed);
        service.create_team("Blue Team", TeamColor::BrightBlue);
        service.auto_assign = true;
        service.balance_teams = true;
        service
    }
    
    /// Create FFA (Free For All) setup - no teams
    pub fn free_for_all() -> Self {
        Self {
            auto_assign: false,
            friendly_fire: true,
            ..Self::new()
        }
    }
}

// ============================================================================
// Team
// ============================================================================

/// Team - a team definition (like Roblox's Team class)
#[derive(Clone, Debug, Reflect, Serialize, Deserialize)]
pub struct Team {
    /// Unique team ID
    pub id: u32,
    
    /// Team name (displayed in UI)
    pub name: String,
    
    /// Team color (for nametags, UI, spawn locations)
    pub team_color: TeamColor,
    
    /// Legacy color field for compatibility
    #[serde(default)]
    pub color: [f32; 4],
    
    /// Can players be auto-assigned to this team
    pub auto_assignable: bool,
    
    /// Max players on this team (0 = unlimited)
    pub max_players: u32,
    
    /// Current player count (runtime, not serialized)
    #[serde(skip)]
    pub player_count: u32,
    
    /// Team score (for team-based games)
    #[serde(default)]
    pub score: i32,
}

impl Default for Team {
    fn default() -> Self {
        Self {
            id: 0,
            name: "Neutral".to_string(),
            team_color: TeamColor::Grey,
            color: [0.5, 0.5, 0.5, 1.0],
            auto_assignable: true,
            max_players: 0,
            player_count: 0,
            score: 0,
        }
    }
}

impl Team {
    /// Create team with raw RGBA color
    pub fn new(id: u32, name: impl Into<String>, color: [f32; 4]) -> Self {
        Self {
            id,
            name: name.into(),
            color,
            team_color: TeamColor::Custom([
                (color[0] * 255.0) as u8,
                (color[1] * 255.0) as u8,
                (color[2] * 255.0) as u8,
            ]),
            ..default()
        }
    }
    
    /// Create team with TeamColor
    pub fn with_color(id: u32, name: impl Into<String>, team_color: TeamColor) -> Self {
        let color = team_color.to_rgba();
        Self {
            id,
            name: name.into(),
            team_color,
            color,
            ..default()
        }
    }
    
    /// Get the team's Bevy Color
    pub fn get_bevy_color(&self) -> Color {
        self.team_color.to_bevy_color()
    }
    
    /// Check if team is full
    pub fn is_full(&self) -> bool {
        self.max_players > 0 && self.player_count >= self.max_players
    }
    
    /// Add score to team
    pub fn add_score(&mut self, points: i32) {
        self.score += points;
    }
    
    /// Reset team score
    pub fn reset_score(&mut self) {
        self.score = 0;
    }
}

// ============================================================================
// TeamMember Component
// ============================================================================

/// TeamMember - marks an entity as belonging to a team
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct TeamMember {
    /// Team ID
    pub team_id: u32,
}

impl Default for TeamMember {
    fn default() -> Self {
        Self { team_id: 0 }
    }
}

impl TeamMember {
    pub fn new(team_id: u32) -> Self {
        Self { team_id }
    }
}

// ============================================================================
// Team Relations
// ============================================================================

/// Team relation types
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum TeamRelation {
    /// Same team
    Ally,
    /// Different team
    #[default]
    Enemy,
    /// Neutral (no team or special)
    Neutral,
}

/// Check relation between two team IDs
pub fn get_team_relation(team_a: u32, team_b: u32) -> TeamRelation {
    if team_a == 0 || team_b == 0 {
        TeamRelation::Neutral
    } else if team_a == team_b {
        TeamRelation::Ally
    } else {
        TeamRelation::Enemy
    }
}

// ============================================================================
// Events
// ============================================================================

/// Event when player joins a team
#[derive(Message, Clone, Debug)]
pub struct PlayerJoinedTeamEvent {
    pub player: Entity,
    pub team_id: u32,
}

/// Event when player leaves a team
#[derive(Message, Clone, Debug)]
pub struct PlayerLeftTeamEvent {
    pub player: Entity,
    pub old_team_id: u32,
}

/// Event when teams are rebalanced
#[derive(Message, Clone, Debug)]
pub struct TeamsRebalancedEvent;

/// Request to change a player's team
#[derive(Message, Clone, Debug)]
pub struct ChangeTeamRequest {
    pub player: Entity,
    pub new_team_id: u32,
}

/// Request to set a player's team (from server)
#[derive(Message, Clone, Debug)]
pub struct SetPlayerTeamEvent {
    pub player: Entity,
    pub team_id: u32,
}

// ============================================================================
// TeamServicePlugin (Common/Shared)
// ============================================================================

/// Base TeamService plugin - registers types and resources
/// Use TeamServiceClientPlugin or TeamServiceServerPlugin for full functionality
pub struct TeamServicePlugin;

impl Plugin for TeamServicePlugin {
    fn build(&self, app: &mut App) {
        app
            // Register types for reflection
            .register_type::<TeamService>()
            .register_type::<Team>()
            .register_type::<TeamMember>()
            .register_type::<TeamColor>()
            .register_type::<TeamRelation>()
            
            // Resource
            .init_resource::<TeamService>()
            
            // Messages
            .add_message::<PlayerJoinedTeamEvent>()
            .add_message::<PlayerLeftTeamEvent>()
            .add_message::<TeamsRebalancedEvent>()
            .add_message::<ChangeTeamRequest>()
            .add_message::<SetPlayerTeamEvent>()
            
            // Systems
            .add_systems(Update, (
                handle_team_changes,
                update_team_member_colors,
            ));
        
        info!("🏁 TeamServicePlugin initialized");
    }
}

/// Handle team change requests
fn handle_team_changes(
    mut commands: Commands,
    mut requests: MessageReader<ChangeTeamRequest>,
    mut team_service: ResMut<TeamService>,
    mut join_events: MessageWriter<PlayerJoinedTeamEvent>,
    mut leave_events: MessageWriter<PlayerLeftTeamEvent>,
    mut team_members: Query<&mut TeamMember>,
) {
    for request in requests.read() {
        let old_team_id = team_members
            .get(request.player)
            .map(|m| m.team_id)
            .unwrap_or(0);
        
        // Leave old team
        if old_team_id != 0 {
            team_service.player_left(old_team_id);
            leave_events.write(PlayerLeftTeamEvent {
                player: request.player,
                old_team_id,
            });
        }
        
        // Join new team
        if request.new_team_id != 0 {
            team_service.player_joined(request.new_team_id);
            
            // Update or add TeamMember component
            if let Ok(mut member) = team_members.get_mut(request.player) {
                member.team_id = request.new_team_id;
            } else {
                commands.entity(request.player).insert(TeamMember::new(request.new_team_id));
            }
            
            join_events.write(PlayerJoinedTeamEvent {
                player: request.player,
                team_id: request.new_team_id,
            });
            
            if let Some(team) = team_service.get_team_by_id(request.new_team_id) {
                info!("👥 Player {:?} joined team '{}'", request.player, team.name);
            }
        } else {
            // Remove from team (neutral)
            commands.entity(request.player).remove::<TeamMember>();
        }
    }
}

/// Update team member visual colors (for nametags, etc.)
fn update_team_member_colors(
    team_service: Res<TeamService>,
    mut query: Query<(&TeamMember, &mut crate::classes::Instance), Changed<TeamMember>>,
) {
    if !team_service.show_team_colors {
        return;
    }
    
    for (member, mut _instance) in query.iter_mut() {
        if let Some(_team) = team_service.get_team_by_id(member.team_id) {
            // TODO: Update nametag color, character highlight, etc.
            // This would integrate with a UI/rendering system
        }
    }
}

//! # Session Management
//!
//! ## Table of Contents
//!
//! 1. **PlayerSession** - Individual player session data
//! 2. **SessionManager** - Manages player sessions and routing

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::types::{Region, PlayerInfo};

/// Individual player session data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSession {
    /// Unique session ID
    pub id: Uuid,
    /// Player ID
    pub player_id: Uuid,
    /// Player display name
    pub player_name: String,
    /// Experience the player is in
    pub experience_id: String,
    /// Server the player is connected to
    pub server_id: Option<Uuid>,
    /// Player's region
    pub region: Region,
    /// When the session started
    pub started_at: DateTime<Utc>,
    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,
    /// Session metadata (key-value pairs)
    pub metadata: HashMap<String, String>,
}

/// Manages player sessions and routing.
#[derive(Debug, Default)]
pub struct SessionManager {
    /// Active sessions indexed by session ID
    pub sessions: HashMap<Uuid, PlayerSession>,
    /// Player ID to session ID mapping
    pub player_sessions: HashMap<Uuid, Uuid>,
}

impl SessionManager {
    /// Create a new session manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new session for a player.
    pub fn create_session(
        &mut self,
        player_id: Uuid,
        player_name: String,
        experience_id: String,
        region: Region,
    ) -> &PlayerSession {
        let session_id = Uuid::new_v4();
        let now = Utc::now();

        let session = PlayerSession {
            id: session_id,
            player_id,
            player_name,
            experience_id,
            server_id: None,
            region,
            started_at: now,
            last_activity: now,
            metadata: HashMap::new(),
        };

        // Remove any existing session for this player
        if let Some(old_id) = self.player_sessions.remove(&player_id) {
            self.sessions.remove(&old_id);
        }

        self.player_sessions.insert(player_id, session_id);
        self.sessions.insert(session_id, session);
        self.sessions.get(&session_id).unwrap()
    }

    /// Get a session by ID.
    pub fn get_session(&self, session_id: &Uuid) -> Option<&PlayerSession> {
        self.sessions.get(session_id)
    }

    /// Get a session by player ID.
    pub fn get_player_session(&self, player_id: &Uuid) -> Option<&PlayerSession> {
        self.player_sessions
            .get(player_id)
            .and_then(|sid| self.sessions.get(sid))
    }

    /// Remove a session.
    pub fn remove_session(&mut self, session_id: &Uuid) -> Option<PlayerSession> {
        if let Some(session) = self.sessions.remove(session_id) {
            self.player_sessions.remove(&session.player_id);
            Some(session)
        } else {
            None
        }
    }

    /// Update last activity for a session.
    pub fn touch_session(&mut self, session_id: &Uuid) {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.last_activity = Utc::now();
        }
    }

    /// Assign a player to a server.
    pub fn assign_server(&mut self, session_id: &Uuid, server_id: Uuid) {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.server_id = Some(server_id);
            session.last_activity = Utc::now();
        }
    }

    /// Get the number of active sessions.
    pub fn active_count(&self) -> usize {
        self.sessions.len()
    }

    /// Convert a session to PlayerInfo.
    pub fn to_player_info(&self, session_id: &Uuid) -> Option<PlayerInfo> {
        self.sessions.get(session_id).map(|s| PlayerInfo {
            id: s.player_id,
            name: s.player_name.clone(),
            server_id: s.server_id,
            region: s.region,
            latency_ms: 0,
            connected_at: Some(s.started_at),
        })
    }
}

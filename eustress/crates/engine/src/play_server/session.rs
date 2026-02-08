// ============================================================================
// Play Server - Session Management
// ============================================================================

use bevy::prelude::*;
use dashmap::DashMap;
use std::time::Instant;

/// Player session information
#[derive(Debug, Clone)]
pub struct PlayerSession {
    /// Unique session ID
    pub session_id: u64,
    /// Player display name
    pub player_name: String,
    /// Remote address
    pub remote_addr: String,
    /// Session state
    pub state: SessionState,
    /// When the session was created
    pub created_at: Instant,
    /// Last activity timestamp
    pub last_activity: Instant,
    /// Player's character entity (if spawned)
    pub character_entity: Option<Entity>,
    /// Ping in milliseconds
    pub ping_ms: u32,
    /// Last acknowledged server tick
    pub last_ack_tick: u64,
    /// Input sequence number
    pub input_sequence: u32,
}

impl PlayerSession {
    /// Create a new session
    pub fn new(session_id: u64, player_name: String, remote_addr: String) -> Self {
        let now = Instant::now();
        Self {
            session_id,
            player_name,
            remote_addr,
            state: SessionState::Connected,
            created_at: now,
            last_activity: now,
            character_entity: None,
            ping_ms: 0,
            last_ack_tick: 0,
            input_sequence: 0,
        }
    }
    
    /// Update last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }
    
    /// Get session duration
    pub fn duration(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }
    
    /// Get time since last activity
    pub fn idle_time(&self) -> std::time::Duration {
        self.last_activity.elapsed()
    }
    
    /// Check if session is timed out
    pub fn is_timed_out(&self, timeout_secs: u64) -> bool {
        self.idle_time().as_secs() > timeout_secs
    }
}

/// Session state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Just connected, waiting for join message
    Connected,
    /// Join accepted, loading world
    Loading,
    /// Fully joined and playing
    Playing,
    /// Disconnecting
    Disconnecting,
}

/// Connection info for new connections
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// Assigned session ID
    pub session_id: u64,
    /// Player name from join message
    pub player_name: String,
    /// Remote address
    pub remote_addr: String,
}

/// Session manager resource
#[derive(Resource, Default)]
pub struct SessionManager {
    /// Active sessions by session ID
    pub sessions: DashMap<u64, PlayerSession>,
    /// Next session ID
    next_id: std::sync::atomic::AtomicU64,
    /// Session timeout in seconds
    pub timeout_secs: u64,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            sessions: DashMap::new(),
            next_id: std::sync::atomic::AtomicU64::new(1),
            timeout_secs,
        }
    }
    
    /// Create a new session from connection info
    pub fn create_session(&mut self, info: ConnectionInfo) -> u64 {
        let session = PlayerSession::new(
            info.session_id,
            info.player_name,
            info.remote_addr,
        );
        
        self.sessions.insert(info.session_id, session);
        info.session_id
    }
    
    /// Generate a new session ID
    pub fn next_session_id(&self) -> u64 {
        self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
    
    /// Get a session by ID
    pub fn get(&self, session_id: u64) -> Option<dashmap::mapref::one::Ref<'_, u64, PlayerSession>> {
        self.sessions.get(&session_id)
    }
    
    /// Get a mutable session by ID
    pub fn get_mut(&self, session_id: u64) -> Option<dashmap::mapref::one::RefMut<'_, u64, PlayerSession>> {
        self.sessions.get_mut(&session_id)
    }
    
    /// Remove a session
    pub fn remove(&self, session_id: u64) -> Option<(u64, PlayerSession)> {
        self.sessions.remove(&session_id)
    }
    
    /// Get all session IDs
    pub fn session_ids(&self) -> Vec<u64> {
        self.sessions.iter().map(|r| *r.key()).collect()
    }
    
    /// Get session count
    pub fn count(&self) -> usize {
        self.sessions.len()
    }
    
    /// Check for timed out sessions
    pub fn check_timeouts(&self) -> Vec<u64> {
        self.sessions
            .iter()
            .filter(|r| r.value().is_timed_out(self.timeout_secs))
            .map(|r| *r.key())
            .collect()
    }
    
    /// Update session activity
    pub fn touch(&self, session_id: u64) {
        if let Some(mut session) = self.sessions.get_mut(&session_id) {
            session.touch();
        }
    }
    
    /// Set session state
    pub fn set_state(&self, session_id: u64, state: SessionState) {
        if let Some(mut session) = self.sessions.get_mut(&session_id) {
            session.state = state;
        }
    }
    
    /// Set character entity for session
    pub fn set_character(&self, session_id: u64, entity: Entity) {
        if let Some(mut session) = self.sessions.get_mut(&session_id) {
            session.character_entity = Some(entity);
        }
    }
    
    /// Clear character entity for session
    pub fn clear_character(&self, session_id: u64) {
        if let Some(mut session) = self.sessions.get_mut(&session_id) {
            session.character_entity = None;
        }
    }
    
    /// Update ping for session
    pub fn update_ping(&self, session_id: u64, ping_ms: u32) {
        if let Some(mut session) = self.sessions.get_mut(&session_id) {
            session.ping_ms = ping_ms;
        }
    }
    
    /// Update last acknowledged tick
    pub fn update_ack_tick(&self, session_id: u64, tick: u64) {
        if let Some(mut session) = self.sessions.get_mut(&session_id) {
            session.last_ack_tick = tick;
        }
    }
    
    /// Get sessions that need a full snapshot (new or far behind)
    pub fn sessions_needing_snapshot(&self, current_tick: u64, max_delta: u64) -> Vec<u64> {
        self.sessions
            .iter()
            .filter(|r| {
                let session = r.value();
                session.state == SessionState::Playing &&
                (current_tick - session.last_ack_tick) > max_delta
            })
            .map(|r| *r.key())
            .collect()
    }
}

/// Session statistics
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    /// Total sessions created
    pub total_sessions: u64,
    /// Current active sessions
    pub active_sessions: u32,
    /// Peak concurrent sessions
    pub peak_sessions: u32,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Average ping across all sessions
    pub average_ping_ms: u32,
}

impl SessionStats {
    /// Update stats from session manager
    pub fn update(&mut self, manager: &SessionManager) {
        let count = manager.count() as u32;
        self.active_sessions = count;
        if count > self.peak_sessions {
            self.peak_sessions = count;
        }
        
        // Calculate average ping
        let total_ping: u32 = manager.sessions
            .iter()
            .map(|r| r.value().ping_ms)
            .sum();
        
        if count > 0 {
            self.average_ping_ms = total_ping / count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_session_creation() {
        let session = PlayerSession::new(
            1,
            "TestPlayer".to_string(),
            "127.0.0.1:12345".to_string(),
        );
        
        assert_eq!(session.session_id, 1);
        assert_eq!(session.player_name, "TestPlayer");
        assert_eq!(session.state, SessionState::Connected);
    }
    
    #[test]
    fn test_session_manager() {
        let manager = SessionManager::new(30);
        
        let info = ConnectionInfo {
            session_id: 1,
            player_name: "Player1".to_string(),
            remote_addr: "127.0.0.1:12345".to_string(),
        };
        
        let mut manager_mut = manager;
        manager_mut.create_session(info);
        
        assert_eq!(manager_mut.count(), 1);
        assert!(manager_mut.get(1).is_some());
    }
}

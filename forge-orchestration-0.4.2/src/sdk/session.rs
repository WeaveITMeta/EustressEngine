//! Session and connection tracking for game servers
//!
//! Provides primitives for tracking player sessions, connections, and graceful migration.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Unique session identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    /// Create a new session ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a random session ID
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Get the inner string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// Session state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Session is connecting
    Connecting,
    /// Session is active
    Active,
    /// Session is idle (no recent activity)
    Idle,
    /// Session is being migrated to another server
    Migrating,
    /// Session is disconnecting
    Disconnecting,
    /// Session has ended
    Ended,
}

/// Session metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Session ID
    pub id: SessionId,
    /// Current state
    pub state: SessionState,
    /// User/player identifier (optional)
    pub user_id: Option<String>,
    /// Client address
    pub client_addr: Option<String>,
    /// Session creation time
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last activity time
    pub last_activity: chrono::DateTime<chrono::Utc>,
    /// Custom metadata
    pub metadata: HashMap<String, String>,
}

impl Session {
    /// Create a new session
    pub fn new(id: SessionId) -> Self {
        let now = chrono::Utc::now();
        Self {
            id,
            state: SessionState::Connecting,
            user_id: None,
            client_addr: None,
            created_at: now,
            last_activity: now,
            metadata: HashMap::new(),
        }
    }

    /// Set user ID
    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Set client address
    pub fn with_addr(mut self, addr: impl Into<String>) -> Self {
        self.client_addr = Some(addr.into());
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Update last activity time
    pub fn touch(&mut self) {
        self.last_activity = chrono::Utc::now();
    }

    /// Get session duration
    pub fn duration(&self) -> chrono::Duration {
        chrono::Utc::now() - self.created_at
    }

    /// Get idle duration
    pub fn idle_duration(&self) -> chrono::Duration {
        chrono::Utc::now() - self.last_activity
    }
}

/// Session tracker for managing active sessions
#[derive(Clone)]
pub struct SessionTracker {
    sessions: Arc<RwLock<HashMap<SessionId, Session>>>,
    config: SessionConfig,
}

/// Session tracker configuration
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Maximum sessions allowed
    pub max_sessions: usize,
    /// Idle timeout before marking session as idle
    pub idle_timeout: Duration,
    /// Disconnect timeout after idle
    pub disconnect_timeout: Duration,
    /// Enable automatic cleanup of ended sessions
    pub auto_cleanup: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_sessions: 1000,
            idle_timeout: Duration::from_secs(60),
            disconnect_timeout: Duration::from_secs(300),
            auto_cleanup: true,
        }
    }
}

impl SessionTracker {
    /// Create a new session tracker
    pub fn new(config: SessionConfig) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Create with default config
    pub fn default_config() -> Self {
        Self::new(SessionConfig::default())
    }

    /// Create a new session
    pub fn create_session(&self) -> Option<Session> {
        let mut sessions = self.sessions.write();
        
        if sessions.len() >= self.config.max_sessions {
            warn!(max = self.config.max_sessions, "Session limit reached");
            return None;
        }

        let id = SessionId::generate();
        let session = Session::new(id.clone());
        sessions.insert(id, session.clone());
        
        info!(session_id = %session.id, "Session created");
        Some(session)
    }

    /// Create a session with a specific ID
    pub fn create_session_with_id(&self, id: SessionId) -> Option<Session> {
        let mut sessions = self.sessions.write();
        
        if sessions.len() >= self.config.max_sessions {
            warn!(max = self.config.max_sessions, "Session limit reached");
            return None;
        }

        if sessions.contains_key(&id) {
            warn!(session_id = %id, "Session ID already exists");
            return None;
        }

        let session = Session::new(id.clone());
        sessions.insert(id, session.clone());
        
        info!(session_id = %session.id, "Session created");
        Some(session)
    }

    /// Get a session by ID
    pub fn get(&self, id: &SessionId) -> Option<Session> {
        self.sessions.read().get(id).cloned()
    }

    /// Update session state
    pub fn set_state(&self, id: &SessionId, state: SessionState) -> bool {
        let mut sessions = self.sessions.write();
        if let Some(session) = sessions.get_mut(id) {
            session.state = state;
            session.touch();
            debug!(session_id = %id, state = ?state, "Session state updated");
            true
        } else {
            false
        }
    }

    /// Mark session as active
    pub fn activate(&self, id: &SessionId) -> bool {
        self.set_state(id, SessionState::Active)
    }

    /// Touch session (update last activity)
    pub fn touch(&self, id: &SessionId) -> bool {
        let mut sessions = self.sessions.write();
        if let Some(session) = sessions.get_mut(id) {
            session.touch();
            true
        } else {
            false
        }
    }

    /// End a session
    pub fn end_session(&self, id: &SessionId) -> Option<Session> {
        let mut sessions = self.sessions.write();
        if let Some(mut session) = sessions.remove(id) {
            session.state = SessionState::Ended;
            info!(session_id = %id, duration = ?session.duration(), "Session ended");
            Some(session)
        } else {
            None
        }
    }

    /// Get all active sessions
    pub fn active_sessions(&self) -> Vec<Session> {
        self.sessions
            .read()
            .values()
            .filter(|s| s.state == SessionState::Active)
            .cloned()
            .collect()
    }

    /// Get session count
    pub fn count(&self) -> usize {
        self.sessions.read().len()
    }

    /// Get active session count
    pub fn active_count(&self) -> usize {
        self.sessions
            .read()
            .values()
            .filter(|s| s.state == SessionState::Active)
            .count()
    }

    /// Check for idle sessions and update their state
    pub fn check_idle(&self) -> Vec<SessionId> {
        let mut idle_sessions = Vec::new();
        let idle_threshold = chrono::Duration::from_std(self.config.idle_timeout)
            .unwrap_or(chrono::Duration::seconds(60));

        let mut sessions = self.sessions.write();
        for (id, session) in sessions.iter_mut() {
            if session.state == SessionState::Active && session.idle_duration() > idle_threshold {
                session.state = SessionState::Idle;
                idle_sessions.push(id.clone());
                debug!(session_id = %id, "Session marked idle");
            }
        }

        idle_sessions
    }

    /// Cleanup ended/disconnected sessions
    pub fn cleanup(&self) -> usize {
        let mut sessions = self.sessions.write();
        let before = sessions.len();
        sessions.retain(|_, s| s.state != SessionState::Ended);
        let removed = before - sessions.len();
        if removed > 0 {
            debug!(removed = removed, "Cleaned up ended sessions");
        }
        removed
    }

    /// Prepare all sessions for migration (e.g., before server shutdown)
    pub fn prepare_migration(&self) -> Vec<Session> {
        let mut sessions = self.sessions.write();
        let mut migrating = Vec::new();

        for session in sessions.values_mut() {
            if session.state == SessionState::Active || session.state == SessionState::Idle {
                session.state = SessionState::Migrating;
                migrating.push(session.clone());
            }
        }

        info!(count = migrating.len(), "Sessions prepared for migration");
        migrating
    }

    /// Export all sessions for migration
    pub fn export_sessions(&self) -> Vec<Session> {
        self.sessions.read().values().cloned().collect()
    }

    /// Import sessions (e.g., after migration)
    pub fn import_sessions(&self, sessions: Vec<Session>) -> usize {
        let mut store = self.sessions.write();
        let mut imported = 0;

        for mut session in sessions {
            if !store.contains_key(&session.id) && store.len() < self.config.max_sessions {
                session.state = SessionState::Active;
                session.touch();
                store.insert(session.id.clone(), session);
                imported += 1;
            }
        }

        info!(imported = imported, "Sessions imported");
        imported
    }
}

/// Connection info for network tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    /// Session this connection belongs to
    pub session_id: SessionId,
    /// Remote address
    pub remote_addr: String,
    /// Local port
    pub local_port: u16,
    /// Protocol (TCP, UDP, QUIC)
    pub protocol: String,
    /// Connection established time
    pub connected_at: chrono::DateTime<chrono::Utc>,
    /// Bytes sent
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_received: u64,
    /// Packets sent (for UDP)
    pub packets_sent: u64,
    /// Packets received (for UDP)
    pub packets_received: u64,
    /// Round-trip time estimate
    pub rtt_ms: Option<u32>,
}

impl ConnectionInfo {
    /// Create new connection info
    pub fn new(session_id: SessionId, remote_addr: impl Into<String>, protocol: impl Into<String>) -> Self {
        Self {
            session_id,
            remote_addr: remote_addr.into(),
            local_port: 0,
            protocol: protocol.into(),
            connected_at: chrono::Utc::now(),
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            rtt_ms: None,
        }
    }

    /// Update traffic stats
    pub fn update_stats(&mut self, sent: u64, received: u64) {
        self.bytes_sent += sent;
        self.bytes_received += received;
    }

    /// Update packet stats
    pub fn update_packets(&mut self, sent: u64, received: u64) {
        self.packets_sent += sent;
        self.packets_received += received;
    }

    /// Set RTT
    pub fn set_rtt(&mut self, rtt_ms: u32) {
        self.rtt_ms = Some(rtt_ms);
    }

    /// Get connection duration
    pub fn duration(&self) -> chrono::Duration {
        chrono::Utc::now() - self.connected_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let tracker = SessionTracker::default_config();
        let session = tracker.create_session().unwrap();
        
        assert_eq!(session.state, SessionState::Connecting);
        assert_eq!(tracker.count(), 1);
    }

    #[test]
    fn test_session_lifecycle() {
        let tracker = SessionTracker::default_config();
        let session = tracker.create_session().unwrap();
        let id = session.id.clone();

        // Activate
        assert!(tracker.activate(&id));
        let session = tracker.get(&id).unwrap();
        assert_eq!(session.state, SessionState::Active);

        // End
        let ended = tracker.end_session(&id).unwrap();
        assert_eq!(ended.state, SessionState::Ended);
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn test_session_limit() {
        let config = SessionConfig {
            max_sessions: 2,
            ..Default::default()
        };
        let tracker = SessionTracker::new(config);

        assert!(tracker.create_session().is_some());
        assert!(tracker.create_session().is_some());
        assert!(tracker.create_session().is_none()); // Should fail
    }

    #[test]
    fn test_migration() {
        let tracker = SessionTracker::default_config();
        
        let s1 = tracker.create_session().unwrap();
        tracker.activate(&s1.id);
        
        let s2 = tracker.create_session().unwrap();
        tracker.activate(&s2.id);

        let migrating = tracker.prepare_migration();
        assert_eq!(migrating.len(), 2);

        for s in &migrating {
            assert_eq!(s.state, SessionState::Migrating);
        }
    }
}

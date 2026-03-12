//! # Workshop Conversation Persistence
//!
//! Stores ideation session history in a Windsurf-compatible format:
//! ~/.eustress_engine/workshop/history/{session_id}/entries.json
//!
//! ## Table of Contents
//!
//! 1. SessionEntry — individual message/event record
//! 2. SessionManifest — session-level metadata (version, resource, entries list)
//! 3. save_session / load_session — read/write session files
//! 4. list_sessions — enumerate past sessions for history browsing
//!
//! ## Format
//!
//! Modeled after Windsurf's History format:
//! ```json
//! {
//!     "version": 1,
//!     "resource": "workshop://ideation/{session_id}",
//!     "product_name": "V-Cell 4680",
//!     "entries": [
//!         {
//!             "id": "msg_001",
//!             "source": "user",
//!             "content": "Solid-state sodium-sulfur battery...",
//!             "timestamp": 1710000000000
//!         }
//!     ]
//! }
//! ```
//!
//! Cross-OS: uses dirs::home_dir() for ~/.eustress_engine/ on all platforms.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use bevy::log::{info, warn};

use super::{ChatMessage, IdeationPipeline};

// ============================================================================
// 1. SessionEntry — individual message record
// ============================================================================

/// A single entry in the session history file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    /// Entry identifier (message ID as string)
    pub id: String,
    /// Source role: "user", "system", "mcp", "artifact", "error"
    pub source: String,
    /// Message content
    pub content: String,
    /// Unix timestamp in milliseconds
    pub timestamp: u64,
    /// Optional MCP endpoint (for MCP command entries)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_endpoint: Option<String>,
    /// Optional MCP status
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_status: Option<String>,
    /// Optional artifact path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    /// Optional artifact type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_type: Option<String>,
    /// Cost of this entry's API call in USD
    #[serde(default)]
    pub cost: f64,
}

// ============================================================================
// 2. SessionManifest — session-level metadata
// ============================================================================

/// Top-level session file matching Windsurf History format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifest {
    /// Schema version (always 1 for now)
    pub version: u32,
    /// Resource URI: workshop://ideation/{session_id}
    pub resource: String,
    /// Product name (if known)
    #[serde(default)]
    pub product_name: String,
    /// Pipeline state when saved
    #[serde(default)]
    pub pipeline_state: String,
    /// Total BYOK API cost for this session
    #[serde(default)]
    pub total_cost: f64,
    /// All conversation entries
    pub entries: Vec<SessionEntry>,
}

// ============================================================================
// 3. Path helpers
// ============================================================================

/// Get the workshop history root directory
/// ~/.eustress_engine/workshop/history/
pub fn history_root() -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        home.join(".eustress_engine").join("workshop").join("history")
    })
}

/// Get the session directory for a specific session ID
/// ~/.eustress_engine/workshop/history/{session_id}/
pub fn session_dir(session_id: &str) -> Option<PathBuf> {
    history_root().map(|root| root.join(session_id))
}

/// Get the entries.json path for a specific session ID
pub fn session_entries_path(session_id: &str) -> Option<PathBuf> {
    session_dir(session_id).map(|dir| dir.join("entries.json"))
}

// ============================================================================
// 4. Save / Load
// ============================================================================

/// Convert a ChatMessage to a SessionEntry
fn message_to_entry(msg: &ChatMessage) -> SessionEntry {
    // Parse ISO 8601 timestamp to millis, fallback to 0
    let timestamp = chrono::DateTime::parse_from_rfc3339(&msg.timestamp)
        .map(|dt| dt.timestamp_millis() as u64)
        .unwrap_or(0);
    
    SessionEntry {
        id: format!("msg_{:04}", msg.id),
        source: msg.role.to_slint_string().to_string(),
        content: msg.content.clone(),
        timestamp,
        mcp_endpoint: msg.mcp_endpoint.clone(),
        mcp_status: msg.mcp_status.as_ref().map(|s| s.to_slint_string().to_string()),
        artifact_path: msg.artifact_path.as_ref().map(|p| p.display().to_string()),
        artifact_type: msg.artifact_type.as_ref().map(|t| t.to_slint_string().to_string()),
        cost: msg.estimated_cost,
    }
}

/// Save the current ideation session to disk
pub fn save_session(pipeline: &IdeationPipeline) -> Result<(), String> {
    let path = session_entries_path(&pipeline.session_id)
        .ok_or_else(|| "Cannot determine home directory".to_string())?;
    
    // Ensure directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create session directory: {}", e))?;
    }
    
    let manifest = SessionManifest {
        version: 1,
        resource: format!("workshop://ideation/{}", pipeline.session_id),
        product_name: pipeline.product_name.clone(),
        pipeline_state: pipeline.state_string().to_string(),
        total_cost: pipeline.total_cost,
        entries: pipeline.messages.iter().map(message_to_entry).collect(),
    };
    
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Failed to serialize session: {}", e))?;
    
    std::fs::write(&path, json)
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    
    info!("Workshop: Saved session {} ({} entries) to {:?}", 
          pipeline.session_id, manifest.entries.len(), path);
    
    Ok(())
}

/// Load a session from disk by session ID
pub fn load_session(session_id: &str) -> Result<SessionManifest, String> {
    let path = session_entries_path(session_id)
        .ok_or_else(|| "Cannot determine home directory".to_string())?;
    
    let json = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    
    let manifest: SessionManifest = serde_json::from_str(&json)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
    
    info!("Workshop: Loaded session {} ({} entries)", session_id, manifest.entries.len());
    
    Ok(manifest)
}

/// List all past session IDs with metadata (newest first)
pub fn list_sessions() -> Vec<SessionSummary> {
    let Some(root) = history_root() else {
        return Vec::new();
    };
    
    if !root.exists() {
        return Vec::new();
    }
    
    let mut sessions = Vec::new();
    
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            
            let session_id = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            
            if session_id.is_empty() {
                continue;
            }
            
            // Try to read the manifest for metadata
            match load_session(&session_id) {
                Ok(manifest) => {
                    let last_timestamp = manifest.entries.last()
                        .map(|e| e.timestamp)
                        .unwrap_or(0);
                    
                    sessions.push(SessionSummary {
                        session_id,
                        product_name: manifest.product_name,
                        pipeline_state: manifest.pipeline_state,
                        total_cost: manifest.total_cost,
                        entry_count: manifest.entries.len(),
                        last_timestamp,
                    });
                }
                Err(e) => {
                    warn!("Workshop: Failed to read session {}: {}", session_id, e);
                }
            }
        }
    }
    
    // Sort newest first
    sessions.sort_by(|a, b| b.last_timestamp.cmp(&a.last_timestamp));
    sessions
}

/// Summary of a past session for history browsing
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_id: String,
    pub product_name: String,
    pub pipeline_state: String,
    pub total_cost: f64,
    pub entry_count: usize,
    pub last_timestamp: u64,
}

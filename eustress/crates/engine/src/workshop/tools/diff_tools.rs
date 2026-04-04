//! Staged diff system — multi-file edits with accept/reject.
//!
//! When the AI proposes file changes, they're staged as DiffEntry items
//! instead of written directly. The user reviews each diff in the Workshop
//! panel and accepts or rejects individual changes.

use super::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Diff Entry (staged change)
// ---------------------------------------------------------------------------

/// A single staged file change awaiting user review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffEntry {
    /// Unique diff ID.
    pub id: u32,
    /// Relative path within the Universe folder.
    pub path: String,
    /// Type of change.
    pub change_type: DiffChangeType,
    /// New content (for Create and Modify).
    pub new_content: Option<String>,
    /// Original content before modification (for Modify — enables reject/restore).
    pub original_content: Option<String>,
    /// Human-readable summary of what changed.
    pub summary: String,
    /// Review status.
    pub status: DiffStatus,
}

/// Type of file change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffChangeType {
    /// New file creation.
    Create,
    /// Modification of existing file.
    Modify,
    /// File deletion.
    Delete,
}

/// Review status of a staged diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffStatus {
    /// Awaiting user review.
    Pending,
    /// User accepted — will be applied.
    Accepted,
    /// User rejected — will be discarded.
    Rejected,
    /// Already applied to disk.
    Applied,
}

// ---------------------------------------------------------------------------
// Staged Changes Resource
// ---------------------------------------------------------------------------

/// Bevy resource holding all staged file changes.
#[derive(bevy::prelude::Resource, Default)]
pub struct StagedChanges {
    pub entries: Vec<DiffEntry>,
    next_id: u32,
}

impl StagedChanges {
    /// Stage a new file change. Returns the diff ID.
    pub fn stage(&mut self, path: String, change_type: DiffChangeType, new_content: Option<String>, original_content: Option<String>, summary: String) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(DiffEntry {
            id,
            path,
            change_type,
            new_content,
            original_content,
            summary,
            status: DiffStatus::Pending,
        });
        id
    }

    /// Accept a diff by ID.
    pub fn accept(&mut self, id: u32) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.status = DiffStatus::Accepted;
        }
    }

    /// Reject a diff by ID.
    pub fn reject(&mut self, id: u32) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.status = DiffStatus::Rejected;
        }
    }

    /// Accept all pending diffs.
    pub fn accept_all(&mut self) {
        for entry in &mut self.entries {
            if entry.status == DiffStatus::Pending {
                entry.status = DiffStatus::Accepted;
            }
        }
    }

    /// Reject all pending diffs.
    pub fn reject_all(&mut self) {
        for entry in &mut self.entries {
            if entry.status == DiffStatus::Pending {
                entry.status = DiffStatus::Rejected;
            }
        }
    }

    /// Apply all accepted diffs to disk. Returns count of applied changes.
    pub fn apply_accepted(&mut self, universe_root: &std::path::Path) -> u32 {
        let mut applied = 0;
        for entry in &mut self.entries {
            if entry.status != DiffStatus::Accepted { continue; }

            let path = universe_root.join(&entry.path);
            let success = match entry.change_type {
                DiffChangeType::Create | DiffChangeType::Modify => {
                    if let Some(ref content) = entry.new_content {
                        if let Some(parent) = path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        std::fs::write(&path, content).is_ok()
                    } else { false }
                }
                DiffChangeType::Delete => {
                    std::fs::remove_file(&path).is_ok()
                }
            };

            if success {
                entry.status = DiffStatus::Applied;
                applied += 1;
            }
        }
        applied
    }

    /// Get pending diff count.
    pub fn pending_count(&self) -> usize {
        self.entries.iter().filter(|e| e.status == DiffStatus::Pending).count()
    }

    /// Clear all applied and rejected entries.
    pub fn clear_resolved(&mut self) {
        self.entries.retain(|e| e.status == DiffStatus::Pending);
    }
}

// ---------------------------------------------------------------------------
// Stage File Change Tool
// ---------------------------------------------------------------------------

pub struct StageFileChangeTool;

impl ToolHandler for StageFileChangeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "stage_file_change",
            description: "Stage a file change for user review instead of writing directly. The user sees a diff view and can accept or reject the change. Use this for multi-file edits where the user should review before applying. Supported change types: create (new file), modify (edit existing), delete (remove file).",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path within Universe folder" },
                    "change_type": { "type": "string", "description": "Change type: create, modify, delete" },
                    "content": { "type": "string", "description": "New file content (required for create and modify)" },
                    "summary": { "type": "string", "description": "One-line description of what this change does" }
                },
                "required": ["path", "change_type", "summary"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &["workshop.diff.staged"],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let change_type_str = input.get("change_type").and_then(|v| v.as_str()).unwrap_or("create");
        let content = input.get("content").and_then(|v| v.as_str()).map(|s| s.to_string());
        let summary = input.get("summary").and_then(|v| v.as_str()).unwrap_or("File change");

        let change_type = match change_type_str {
            "modify" => DiffChangeType::Modify,
            "delete" => DiffChangeType::Delete,
            _ => DiffChangeType::Create,
        };

        // Read original content for modify operations
        let original = if change_type == DiffChangeType::Modify {
            let full_path = ctx.universe_root.join(path);
            std::fs::read_to_string(&full_path).ok()
        } else {
            None
        };

        if path.is_empty() {
            return ToolResult {
                tool_name: "stage_file_change".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: "Missing required parameter: path".to_string(),
                structured_data: None,
                stream_topic: None,
            };
        }

        ToolResult {
            tool_name: "stage_file_change".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("Staged {} for '{}': {}", change_type_str, path, summary),
            structured_data: Some(serde_json::json!({
                "action": "stage_diff",
                "path": path,
                "change_type": change_type_str,
                "content": content,
                "original": original,
                "summary": summary,
            })),
            stream_topic: Some("workshop.diff.staged".to_string()),
        }
    }
}

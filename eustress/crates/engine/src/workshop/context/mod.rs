//! Workshop Context Manager — persistent memories, rules, and workflows.
//!
//! Memories are Universe-scoped facts the AI remembers across sessions:
//! - Auto-generated from conversations ("user prefers 900 Wh/kg batteries")
//! - User-defined via the `remember` tool
//! - Loaded from `.eustress/memories/` on session start
//!
//! Rules are coding standards and output format preferences loaded from
//! `.eustress/rules/*.md` files. They're injected into the system prompt.
//!
//! Workflows are multi-step instruction sequences loaded from
//! `SoulService/.Workflows/*.md`, triggered via slash commands.

pub mod sync;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Memory Entry
// ---------------------------------------------------------------------------

/// A single persistent memory — a fact, preference, or project state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique key (e.g. "preferred_material", "project:vcell:status").
    pub key: String,
    /// Human-readable value.
    pub value: String,
    /// When this memory was created/updated (ISO 8601).
    pub updated_at: String,
    /// Source: "user" (explicitly stated), "inferred" (AI extracted), "system".
    pub source: String,
    /// Category for organization: "preference", "fact", "project", "contact".
    pub category: String,
}

// ---------------------------------------------------------------------------
// Rule Entry
// ---------------------------------------------------------------------------

/// A coding standard or output format rule loaded from `.eustress/rules/`.
#[derive(Debug, Clone)]
pub struct RuleEntry {
    /// Filename (e.g. "coding-standards.md").
    pub filename: String,
    /// Full content of the rule file.
    pub content: String,
}

// ---------------------------------------------------------------------------
// Workflow Entry
// ---------------------------------------------------------------------------

/// A multi-step workflow loaded from `SoulService/.Workflows/`.
/// Triggered via slash commands (e.g. `/run manufacturing-pipeline`).
#[derive(Debug, Clone)]
pub struct WorkflowEntry {
    /// Workflow name derived from filename (e.g. "manufacturing-pipeline").
    pub name: String,
    /// Slash command to trigger this workflow.
    pub command: String,
    /// Full instruction content (Markdown).
    pub content: String,
}

// ---------------------------------------------------------------------------
// Context Manager
// ---------------------------------------------------------------------------

/// Manages persistent memories, rules, and workflows for the Workshop.
/// Universe-scoped: all data lives under `~/Documents/Eustress/`.
pub struct ContextManager {
    /// Persistent memories (survive across sessions).
    pub memories: Vec<MemoryEntry>,
    /// Rules loaded from `.eustress/rules/*.md`.
    pub rules: Vec<RuleEntry>,
    /// Workflows loaded from `SoulService/.Workflows/*.md`.
    pub workflows: Vec<WorkflowEntry>,
    /// Universe root path (sandbox boundary).
    pub universe_root: PathBuf,
    /// Whether memories need to be saved to disk.
    pub dirty: bool,
}

impl ContextManager {
    /// Create a new ContextManager for the given Universe root.
    pub fn new(universe_root: PathBuf) -> Self {
        let mut mgr = Self {
            memories: Vec::new(),
            rules: Vec::new(),
            workflows: Vec::new(),
            universe_root,
            dirty: false,
        };
        mgr.load_all();
        mgr
    }

    /// Load all memories, rules, and workflows from disk.
    pub fn load_all(&mut self) {
        self.load_memories();
        self.load_rules();
        self.load_workflows();
    }

    // ── Memories ──────────────────────────────────────────────────────

    /// Add or update a memory. If a memory with the same key exists, update it.
    pub fn remember(&mut self, key: String, value: String, source: &str, category: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(existing) = self.memories.iter_mut().find(|m| m.key == key) {
            existing.value = value;
            existing.updated_at = now;
            existing.source = source.to_string();
        } else {
            self.memories.push(MemoryEntry {
                key,
                value,
                updated_at: now,
                source: source.to_string(),
                category: category.to_string(),
            });
        }
        self.dirty = true;
    }

    /// Recall memories matching a query string (substring match on key + value).
    pub fn recall(&self, query: &str) -> Vec<&MemoryEntry> {
        let q = query.to_lowercase();
        self.memories
            .iter()
            .filter(|m| {
                m.key.to_lowercase().contains(&q)
                    || m.value.to_lowercase().contains(&q)
                    || m.category.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Format all memories for injection into the Claude system prompt.
    pub fn format_memories_for_prompt(&self) -> String {
        if self.memories.is_empty() {
            return "No stored memories.".to_string();
        }
        let mut out = String::new();
        for m in &self.memories {
            out.push_str(&format!("- [{}] {}: {}\n", m.category, m.key, m.value));
        }
        out
    }

    /// Format all rules for injection into the Claude system prompt.
    pub fn format_rules_for_prompt(&self) -> String {
        if self.rules.is_empty() {
            return String::new();
        }
        let mut out = String::from("## Workshop Rules\n\n");
        for rule in &self.rules {
            out.push_str(&format!("### {}\n{}\n\n", rule.filename, rule.content));
        }
        out
    }

    /// List available workflow slash commands.
    pub fn workflow_commands(&self) -> Vec<(&str, &str)> {
        self.workflows
            .iter()
            .map(|w| (w.command.as_str(), w.name.as_str()))
            .collect()
    }

    /// Get a workflow by slash command.
    pub fn get_workflow(&self, command: &str) -> Option<&WorkflowEntry> {
        self.workflows.iter().find(|w| w.command == command)
    }

    /// Save memories to disk (`.eustress/memories/memories.json`).
    pub fn save_memories(&self) {
        let dir = self.universe_root.join(".eustress").join("memories");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::error!("Failed to create memories dir: {}", e);
            return;
        }
        let path = dir.join("memories.json");
        match serde_json::to_string_pretty(&self.memories) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::error!("Failed to save memories: {}", e);
                }
            }
            Err(e) => tracing::error!("Failed to serialize memories: {}", e),
        }
    }

    // ── Private loaders ──────────────────────────────────────────────

    fn load_memories(&mut self) {
        let path = self.universe_root.join(".eustress").join("memories").join("memories.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(memories) = serde_json::from_str::<Vec<MemoryEntry>>(&content) {
                self.memories = memories;
                tracing::info!("Loaded {} Workshop memories", self.memories.len());
            }
        }
    }

    fn load_rules(&mut self) {
        let dir = self.universe_root.join(".eustress").join("rules");
        if !dir.exists() {
            return;
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "md").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        self.rules.push(RuleEntry { filename, content });
                    }
                }
            }
            if !self.rules.is_empty() {
                tracing::info!("Loaded {} Workshop rules", self.rules.len());
            }
        }
    }

    fn load_workflows(&mut self) {
        // Check both SoulService/.Workflows/ and .eustress/workflows/
        let dirs = [
            self.universe_root.join("SoulService").join(".Workflows"),
            self.universe_root.join(".eustress").join("workflows"),
        ];
        for dir in &dirs {
            if !dir.exists() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "md").unwrap_or(false) {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                            let command = format!("/run {}", stem);
                            self.workflows.push(WorkflowEntry {
                                name: stem,
                                command,
                                content,
                            });
                        }
                    }
                }
            }
        }
        if !self.workflows.is_empty() {
            tracing::info!("Loaded {} Workshop workflows", self.workflows.len());
        }
    }
}

impl Drop for ContextManager {
    fn drop(&mut self) {
        if self.dirty {
            self.save_memories();
        }
    }
}

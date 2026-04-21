//! Workshop Modes — domain-specific configurations for the AI agent.
//!
//! Each mode defines:
//! - A system prompt fragment (injected into Claude's context)
//! - Pipeline sidebar steps (if any — empty for chat-only modes)
//! - A greeting message when the mode activates
//!
//! The `General` mode is always active as a base layer. Domain modes
//! add specialized tools and prompts on top.
//!
//! ## Layering
//!
//! The `WorkshopMode` enum itself + every method (display_name, icon,
//! color, trigger_keywords, system_prompt_fragment, greeting) lives
//! in the shared `eustress-tools` crate so the MCP server can reason
//! about the same modes without depending on the engine. This module
//! re-exports it and layers on `ActiveModes` — the engine-side
//! orchestration that tracks which modes are on, composes the full
//! system prompt (including the live API reference catalog), and
//! auto-activates modes from keyword detection.

pub mod manufacturing;
pub mod simulation;
pub mod supply_chain;
pub mod warehousing;
pub mod finance;
pub mod fabrication;
pub mod shopping;
pub mod travel;

use serde::{Deserialize, Serialize};

// Canonical `WorkshopMode` — defined in `eustress-tools` so the MCP
// server shares the same enum + metadata. All inherent methods
// (display_name, icon, color, badge, trigger_keywords,
// system_prompt_fragment, greeting, all_domains) live with the type
// there.
pub use eustress_tools::WorkshopMode;

// ---------------------------------------------------------------------------
// Active Modes (inferred, stackable)
// ---------------------------------------------------------------------------

/// Tracks which modes are currently active based on conversation context.
/// General is always active. Domain modes activate when the AI detects
/// relevant topics and can stack (e.g. Manufacturing + SupplyChain).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveModes {
    /// Currently active domain modes (General is implicit).
    pub domains: Vec<WorkshopMode>,
}

impl Default for ActiveModes {
    fn default() -> Self {
        Self { domains: Vec::new() }
    }
}

impl ActiveModes {
    /// Get all active modes including General.
    pub fn all(&self) -> Vec<WorkshopMode> {
        let mut modes = vec![WorkshopMode::General];
        modes.extend_from_slice(&self.domains);
        modes
    }

    /// Check if a specific mode is active.
    pub fn is_active(&self, mode: WorkshopMode) -> bool {
        mode == WorkshopMode::General || self.domains.contains(&mode)
    }

    /// Activate a domain mode (no-op if already active).
    pub fn activate(&mut self, mode: WorkshopMode) {
        if mode != WorkshopMode::General && !self.domains.contains(&mode) {
            self.domains.push(mode);
        }
    }

    /// Deactivate a domain mode.
    pub fn deactivate(&mut self, mode: WorkshopMode) {
        self.domains.retain(|m| *m != mode);
    }

    /// Detect modes from a user message by keyword matching.
    /// Activates matching modes, returns which new modes were activated.
    pub fn detect_from_message(&mut self, message: &str) -> Vec<WorkshopMode> {
        let lower = message.to_lowercase();
        let mut newly_activated = Vec::new();

        for mode in WorkshopMode::all_domains() {
            if self.domains.contains(mode) { continue; }
            let triggered = mode.trigger_keywords().iter().any(|kw| lower.contains(&kw.to_lowercase()));
            if triggered {
                self.domains.push(*mode);
                newly_activated.push(*mode);
            }
        }

        newly_activated
    }

    /// Format active modes as badge text for chat display.
    /// e.g. "⚡ General  🏭 Manufacturing  🔗 Supply Chain"
    pub fn badges_text(&self) -> String {
        self.all().iter().map(|m| m.badge()).collect::<Vec<_>>().join("  ")
    }

    /// Format active modes as a compact system prompt fragment.
    ///
    /// Simulation mode's fragment is the shared-crate preamble plus
    /// the auto-generated API reference from `rune_ecs_module.rs` so
    /// the agent always sees every registered function — the live
    /// catalog replaces the stale hand-maintained constants that used
    /// to live in this file.
    pub fn system_prompt_fragments(&self) -> String {
        let mut out = String::new();
        for mode in &self.domains {
            out.push_str(&format!("\n## Active Mode: {} {}\n", mode.icon(), mode.display_name()));
            out.push_str(mode.system_prompt_fragment());
            if *mode == WorkshopMode::Simulation {
                // Append the live-built API catalog after the shared-crate
                // preamble. Only the engine has access to the catalog
                // builder, which is why this splice lives here and not
                // in the shared mode definition.
                out.push('\n');
                let catalog = super::api_reference::ApiCatalog::build();
                out.push_str(&catalog.format_full_reference());
            }
            out.push('\n');
        }
        out
    }
}

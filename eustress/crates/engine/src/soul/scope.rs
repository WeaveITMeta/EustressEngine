//! # Soul Script Scope
//!
//! Defines script locations and system prompt building.

use bevy::prelude::*;

/// Script location types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptLocation {
    Entity(Entity),
    Service,
    Workspace,
    SoulService,
}

impl ScriptLocation {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Entity(_) => "Entity",
            Self::Service => "Service",
            Self::Workspace => "Workspace",
            Self::SoulService => "Soul Service",
        }
    }
}

/// Script scope for context building
#[derive(Debug, Clone, Default)]
pub struct ScriptScope {
    pub location: Option<ScriptLocation>,
    pub entity_name: Option<String>,
    pub class_name: Option<String>,
}

/// Available events for a script scope
#[derive(Debug, Clone, Default)]
pub struct AvailableEvents {
    pub events: Vec<String>,
}

/// System prompt builder for Claude
#[derive(Debug, Clone, Default)]
pub struct SystemPromptBuilder {
    pub scope: ScriptScope,
    pub available_events: AvailableEvents,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn with_scope(mut self, scope: ScriptScope) -> Self {
        self.scope = scope;
        self
    }
    
    pub fn build(&self) -> String {
        // TODO: Build system prompt based on scope
        String::new()
    }
    
    /// Build system prompt from location
    pub fn build_system_prompt(location: &ScriptLocation) -> String {
        match location {
            ScriptLocation::Entity(_) => "You are a Soul Script assistant for entity scripting.".to_string(),
            ScriptLocation::Service => "You are a Soul Script assistant for service scripting.".to_string(),
            ScriptLocation::Workspace => "You are a Soul Script assistant for workspace scripting.".to_string(),
            ScriptLocation::SoulService => "You are a Soul Script assistant for Soul Service scripting.".to_string(),
        }
    }
}

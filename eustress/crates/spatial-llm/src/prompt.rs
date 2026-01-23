//! Prompt Engine - Structured prompt generation for spatial LLM
//!
//! ## Table of Contents
//! 1. PromptEngine - Prompt builder
//! 2. EnvironmentPrompt - Environment generation prompt
//! 3. EnvironmentStyle - Style options

use crate::context::SpatialContext;
use serde::{Deserialize, Serialize};

/// Style options for environment generation
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum EnvironmentStyle {
    /// Realistic, grounded environments
    #[default]
    Realistic,
    /// Stylized, artistic environments
    Stylized,
    /// Fantasy/magical environments
    Fantasy,
    /// Sci-fi/futuristic environments
    SciFi,
    /// Abstract/procedural environments
    Abstract,
}

/// Prompt for environment generation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvironmentPrompt {
    /// Natural language description
    pub description: String,
    /// Center point for generation
    pub center: [f64; 3],
    /// Radius of the area to generate
    pub radius: f64,
    /// Style of the environment
    pub style: EnvironmentStyle,
    /// Density hint (0.0 = sparse, 1.0 = dense)
    pub density: f64,
    /// Seed for reproducibility
    pub seed: Option<u64>,
}

impl Default for EnvironmentPrompt {
    fn default() -> Self {
        Self {
            description: String::new(),
            center: [0.0, 0.0, 0.0],
            radius: 50.0,
            style: EnvironmentStyle::default(),
            density: 0.5,
            seed: None,
        }
    }
}

impl EnvironmentPrompt {
    /// Create a new environment prompt
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            ..Default::default()
        }
    }

    /// Set center point
    pub fn at(mut self, x: f64, y: f64, z: f64) -> Self {
        self.center = [x, y, z];
        self
    }

    /// Set radius
    pub fn with_radius(mut self, radius: f64) -> Self {
        self.radius = radius;
        self
    }

    /// Set style
    pub fn with_style(mut self, style: EnvironmentStyle) -> Self {
        self.style = style;
        self
    }

    /// Set density
    pub fn with_density(mut self, density: f64) -> Self {
        self.density = density.clamp(0.0, 1.0);
        self
    }

    /// Set seed
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }
}

/// Prompt engine for building structured LLM prompts
pub struct PromptEngine {
    /// System prompt prefix
    system_prefix: String,
    /// Whether to include spatial coordinates
    include_coordinates: bool,
    /// Maximum entities to include in context
    max_context_entities: usize,
}

impl Default for PromptEngine {
    fn default() -> Self {
        Self {
            system_prefix: "You are a spatial reasoning AI for 3D world building.".to_string(),
            include_coordinates: true,
            max_context_entities: 100,
        }
    }
}

impl PromptEngine {
    /// Create a new prompt engine
    pub fn new() -> Self {
        Self::default()
    }

    /// Set system prefix
    pub fn with_system_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.system_prefix = prefix.into();
        self
    }

    /// Build a query prompt
    pub fn build_query_prompt(&self, context: &SpatialContext, query: &str) -> String {
        format!(
            "{}\n\n## Current Scene\n{}\n## Query\n{}",
            self.system_prefix,
            context.to_prompt_text(),
            query
        )
    }

    /// Build an environment generation prompt
    pub fn build_generation_prompt(&self, prompt: &EnvironmentPrompt, context: Option<&SpatialContext>) -> String {
        let mut text = format!(
            "{}\n\n## Task\nGenerate entities for a 3D environment.\n\n",
            self.system_prefix
        );

        text.push_str(&format!("## Description\n{}\n\n", prompt.description));

        text.push_str(&format!(
            "## Parameters\n- Center: ({:.1}, {:.1}, {:.1})\n- Radius: {:.1}\n- Style: {:?}\n- Density: {:.1}\n\n",
            prompt.center[0], prompt.center[1], prompt.center[2],
            prompt.radius,
            prompt.style,
            prompt.density
        ));

        if let Some(ctx) = context {
            text.push_str(&format!("## Existing Scene\n{}\n", ctx.to_prompt_text()));
        }

        text.push_str("## Output Format\nRespond with a JSON array of entities:\n");
        text.push_str(r#"[
  {
    "class": "EntityClass",
    "name": "optional_name",
    "position": [x, y, z],
    "rotation": [rx, ry, rz],
    "scale": [sx, sy, sz],
    "properties": {},
    "tags": []
  }
]"#);

        text
    }

    /// Build a behavior generation prompt
    pub fn build_behavior_prompt(&self, entity_class: &str, context: &SpatialContext) -> String {
        format!(
            "{}\n\n## Task\nGenerate behavior patterns for a {} entity.\n\n## Scene Context\n{}\n\n## Output Format\nRespond with a JSON object describing behaviors.",
            self.system_prefix,
            entity_class,
            context.to_prompt_text()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environment_prompt() {
        let prompt = EnvironmentPrompt::new("A dense forest with tall pine trees")
            .at(0.0, 0.0, 0.0)
            .with_radius(100.0)
            .with_style(EnvironmentStyle::Realistic)
            .with_density(0.8);

        assert_eq!(prompt.radius, 100.0);
        assert_eq!(prompt.density, 0.8);
    }

    #[test]
    fn test_prompt_engine() {
        let engine = PromptEngine::new();
        let context = SpatialContext::new();

        let prompt = engine.build_query_prompt(&context, "Where should I place a tree?");
        assert!(prompt.contains("Query"));
        assert!(prompt.contains("tree"));
    }
}

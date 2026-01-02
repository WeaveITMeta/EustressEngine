//! # Soul Configuration
//!
//! Configuration for the Soul scripting system.
//! 
//! ## Dynamic Model Selection
//! 
//! Soul uses a complexity-based model selection system that analyzes the AST
//! and assigns the appropriate Claude model:
//! 
//! | Model | Complexity Score | Use Case |
//! |-------|------------------|----------|
//! | Haiku 3.5 | 0-30 | Simple scripts, single handlers, basic logic |
//! | Sonnet 4 | 31-70 | Medium complexity, multiple handlers, queries |
//! | Opus 4.5 | 71+ | Complex scripts, many systems, advanced patterns |

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// Soul Config
// ============================================================================

/// Soul scripting configuration
#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct SoulConfig {
    /// Enable Soul scripting
    pub enabled: bool,
    
    /// Scripts directory
    pub scripts_dir: PathBuf,
    
    /// Cache directory for compiled scripts
    pub cache_dir: PathBuf,
    
    /// Output directory for generated Rust code
    pub output_dir: PathBuf,
    
    /// Enable hot reload
    pub hot_reload: bool,
    
    /// Hot reload poll interval (ms)
    pub hot_reload_interval_ms: u64,
    
    /// Claude API configuration
    pub claude: ClaudeConfig,
    
    /// Default unit for distances
    pub default_unit: String,
    
    /// Enable Miri UB checks
    pub miri_enabled: bool,
    
    /// Maximum retries for code generation
    pub max_retries: u32,
    
    /// Verbose logging
    pub verbose: bool,
}

impl Default for SoulConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scripts_dir: PathBuf::from("./scripts"),
            cache_dir: PathBuf::from("./.soul-cache"),
            output_dir: PathBuf::from("./target/soul"),
            hot_reload: true,
            hot_reload_interval_ms: 500,
            claude: ClaudeConfig::default(),
            default_unit: "studs".to_string(),
            miri_enabled: false, // Disabled by default (slow)
            max_retries: 3,
            verbose: false,
        }
    }
}

impl SoulConfig {
    /// Load from file or create default
    pub fn load_or_default(path: &std::path::Path) -> Self {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(config) = toml::from_str(&content) {
                    return config;
                }
            }
        }
        Self::default()
    }
    
    /// Save to file
    pub fn save(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, content)
    }
    
    /// Get cache path for a script
    pub fn cache_path(&self, scene: &str, script_name: &str) -> PathBuf {
        self.cache_dir.join(scene).join(format!("{}.cache", script_name))
    }
    
    /// Get output path for generated code
    pub fn output_path(&self, scene: &str, service: &str) -> PathBuf {
        self.output_dir.join(scene).join(format!("{}.rs", service.to_lowercase()))
    }
}

// ============================================================================
// Claude Config
// ============================================================================

/// Claude model tier for dynamic selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelTier {
    /// Haiku - Fast, simple tasks (complexity 0-30)
    Haiku,
    /// Sonnet - Balanced, medium complexity (complexity 31-70)
    Sonnet,
    /// Opus - Best quality, complex tasks (complexity 71+)
    Opus,
}

impl ModelTier {
    /// Get the model string for API calls
    pub fn model_id(&self) -> &'static str {
        match self {
            ModelTier::Haiku => "claude-haiku-4-5-20251001",
            ModelTier::Sonnet => "claude-sonnet-4-5-20250929",
            ModelTier::Opus => "claude-opus-4-5-20250929",
        }
    }
    
    /// Get display name
    pub fn name(&self) -> &'static str {
        match self {
            ModelTier::Haiku => "Haiku 4.5",
            ModelTier::Sonnet => "Sonnet 4.5",
            ModelTier::Opus => "Opus 4.5",
        }
    }
    
    /// Get max tokens for this tier
    pub fn max_tokens(&self) -> u32 {
        match self {
            ModelTier::Haiku => 4096,
            ModelTier::Sonnet => 8192,
            ModelTier::Opus => 16384,
        }
    }
    
    /// Get timeout for this tier (seconds)
    pub fn timeout_secs(&self) -> u64 {
        match self {
            ModelTier::Haiku => 60,
            ModelTier::Sonnet => 180,
            ModelTier::Opus => 300,
        }
    }
    
    /// Get temperature for this tier
    pub fn temperature(&self) -> f32 {
        match self {
            ModelTier::Haiku => 0.2,  // More deterministic for simple tasks
            ModelTier::Sonnet => 0.3,
            ModelTier::Opus => 0.4,   // Slightly more creative for complex tasks
        }
    }
    
    /// Select tier based on complexity score
    pub fn from_complexity(score: u32) -> Self {
        match score {
            0..=30 => ModelTier::Haiku,
            31..=70 => ModelTier::Sonnet,
            _ => ModelTier::Opus,
        }
    }
    
    /// Get the next tier up (for fallback on failure)
    pub fn upgrade(&self) -> Option<Self> {
        match self {
            ModelTier::Haiku => Some(ModelTier::Sonnet),
            ModelTier::Sonnet => Some(ModelTier::Opus),
            ModelTier::Opus => None,
        }
    }
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Claude API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    /// API endpoint
    pub endpoint: String,
    
    /// Haiku model ID
    pub haiku_model: String,
    
    /// Sonnet model ID
    pub sonnet_model: String,
    
    /// Opus model ID
    pub opus_model: String,
    
    /// API key (from environment or config)
    pub api_key: Option<String>,
    
    /// Enable dynamic model selection based on complexity
    pub dynamic_selection: bool,
    
    /// Force a specific tier (overrides dynamic selection)
    pub force_tier: Option<ModelTier>,
    
    /// Enable auto-upgrade on failure
    pub auto_upgrade: bool,
    
    /// Enable thinking mode for complex tasks
    pub thinking_mode: bool,
    
    /// Complexity thresholds
    pub complexity_thresholds: ComplexityThresholds,
}

/// Thresholds for complexity-based model selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityThresholds {
    /// Score below this uses Haiku
    pub haiku_max: u32,
    /// Score below this uses Sonnet (above uses Opus)
    pub sonnet_max: u32,
}

impl Default for ComplexityThresholds {
    fn default() -> Self {
        Self {
            haiku_max: 30,
            sonnet_max: 70,
        }
    }
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://api.anthropic.com/v1/messages".to_string(),
            haiku_model: "claude-haiku-4-5-20251001".to_string(),
            sonnet_model: "claude-sonnet-4-5-20250929".to_string(),
            opus_model: "claude-opus-4-5-20250929".to_string(),
            api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            dynamic_selection: true,
            force_tier: None,
            auto_upgrade: true,
            thinking_mode: true,
            complexity_thresholds: ComplexityThresholds::default(),
        }
    }
}

impl ClaudeConfig {
    /// Check if API key is configured
    pub fn has_api_key(&self) -> bool {
        self.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false)
    }
    
    /// Get API key from config or environment
    pub fn get_api_key(&self) -> Option<String> {
        self.api_key.clone()
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
    }
    
    /// Get model ID for a tier
    pub fn model_for_tier(&self, tier: ModelTier) -> &str {
        match tier {
            ModelTier::Haiku => &self.haiku_model,
            ModelTier::Sonnet => &self.sonnet_model,
            ModelTier::Opus => &self.opus_model,
        }
    }
    
    /// Select model tier based on complexity score
    pub fn select_tier(&self, complexity_score: u32) -> ModelTier {
        // Check for forced tier
        if let Some(forced) = self.force_tier {
            return forced;
        }
        
        // Dynamic selection based on thresholds
        if !self.dynamic_selection {
            return ModelTier::Sonnet; // Default to balanced
        }
        
        if complexity_score <= self.complexity_thresholds.haiku_max {
            ModelTier::Haiku
        } else if complexity_score <= self.complexity_thresholds.sonnet_max {
            ModelTier::Sonnet
        } else {
            ModelTier::Opus
        }
    }
    
    /// Get the model string for a complexity score
    pub fn model_for_complexity(&self, complexity_score: u32) -> &str {
        self.model_for_tier(self.select_tier(complexity_score))
    }
}

// ============================================================================
// Build Config
// ============================================================================

/// Configuration for a single build
#[derive(Debug, Clone)]
pub struct BuildConfig {
    /// Scene to build
    pub scene: String,
    
    /// Specific script (None = all)
    pub script: Option<String>,
    
    /// Force rebuild (ignore cache)
    pub force: bool,
    
    /// Watch for changes
    pub watch: bool,
    
    /// Service filter
    pub service: Option<String>,
    
    /// Verbose output
    pub verbose: bool,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            scene: String::new(),
            script: None,
            force: false,
            watch: false,
            service: None,
            verbose: false,
        }
    }
}

impl BuildConfig {
    /// Create for a specific scene
    pub fn for_scene(scene: &str) -> Self {
        Self {
            scene: scene.to_string(),
            ..Default::default()
        }
    }
    
    /// Set force rebuild
    pub fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }
    
    /// Set watch mode
    pub fn watch(mut self, watch: bool) -> Self {
        self.watch = watch;
        self
    }
    
    /// Set service filter
    pub fn service(mut self, service: &str) -> Self {
        self.service = Some(service.to_string());
        self
    }
}

// ============================================================================
// Prompt Templates
// ============================================================================

/// Prompt template for Claude code generation
pub const SOUL_PROMPT_TEMPLATE: &str = r#"You are Soul: Transpile per-scene English to Bevy Rust, scoped to services (Workspace for world, ServerScriptService for server-only).

Rules:
1. Classify Meta (strict ECS, server-authoritative) vs. Plausible (creative, entity-guarded)
2. Convert units to engine units/meters (e.g., "1 yard" → 3.266 units, "1 foot" → 1.0886 units)
3. Output ONLY valid Rust code using bevy::prelude::*
4. Use Queries with service filters (e.g., Query<&Transform, With<InWorkspace>>)
5. Be safe and concise - no unsafe code unless absolutely necessary
6. Generate impl blocks for Script trait
7. Service is determined by parent hierarchy, NOT frontmatter

Unit Conversions (1 unit = 0.28 meters):
- 1 unit = 0.28 meters
- 1 foot = 0.3048 meters = 1.0886 units
- 1 yard = 0.9144 meters = 3.266 units
- 1 km/h = 0.2778 m/s = 0.992 units/s
- 1 mph = 0.447 m/s = 1.596 units/s

AST: {ast_json}
Scene: {scene}
Service: {service}
Type: {script_type}

Generate a complete Rust module with:
1. Required imports
2. System functions for each event handler
3. Plugin registration
4. Proper error handling

Output ONLY the Rust code, no explanations."#;

/// Thinking mode prompt for fallback
pub const SOUL_THINKING_PROMPT: &str = r#"Think step-by-step to transpile this Soul script to Bevy Rust:

Step 1: Parse intent from the AST
Step 2: Map units to studs/meters
Step 3: Identify required components and queries
Step 4: Generate system functions
Step 5: Create plugin registration

AST: {ast_json}
Scene: {scene}
Service: {service}

Now generate the Rust code:"#;

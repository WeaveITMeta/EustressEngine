//! # Soul Code Generation (Stub)
//!
//! Legacy AST → Rust code generation. Claude now generates Rune directly.
//! Kept for interface compatibility.

#[allow(unused_imports)]
use eustress_common::soul::ClaudeConfig;

// ============================================================================
// Code Gen Error
// ============================================================================

/// Code generation error
#[derive(Debug)]
pub enum CodeGenError {
    NoApiKey,
    ApiError(String),
}

impl std::fmt::Display for CodeGenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodeGenError::NoApiKey => write!(f, "No API key configured"),
            CodeGenError::ApiError(msg) => write!(f, "API error: {}", msg),
        }
    }
}

impl std::error::Error for CodeGenError {}

// ============================================================================
// Code Generator (Stub)
// ============================================================================

/// Soul code generator (stub - Claude generates Rune directly now)
#[allow(dead_code)]
pub struct SoulCodeGen {
    config: ClaudeConfig,
}

impl Default for SoulCodeGen {
    fn default() -> Self {
        Self {
            config: ClaudeConfig::default(),
        }
    }
}

impl SoulCodeGen {
    pub fn new(config: ClaudeConfig) -> Self {
        Self { config }
    }
}

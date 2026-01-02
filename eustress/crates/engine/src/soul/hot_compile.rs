//! # Hot Compile System (Stub)
//!
//! This module is a stub - Rune scripts execute directly without compilation.
//! Kept for interface compatibility with build_pipeline.

use bevy::prelude::*;
use std::path::PathBuf;

use eustress_common::soul::GeneratedCode;

// ============================================================================
// Hot Compile Configuration
// ============================================================================

/// Configuration for hot compilation (stub)
#[derive(Resource, Clone)]
pub struct HotCompileConfig {
    pub source_dir: PathBuf,
    pub output_dir: PathBuf,
}

impl Default for HotCompileConfig {
    fn default() -> Self {
        let temp_dir = std::env::temp_dir().join("eustress_soul");
        Self {
            source_dir: temp_dir.join("src"),
            output_dir: temp_dir.join("target"),
        }
    }
}

// ============================================================================
// Compile Result
// ============================================================================

/// Result of a compilation (stub)
#[derive(Debug, Clone)]
pub struct CompileResult {
    pub module_name: String,
    pub success: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

// ============================================================================
// Hot Compiler (Stub)
// ============================================================================

/// Hot compiler for Soul-generated code (stub - Rune doesn't need compilation)
#[derive(Resource, Default)]
pub struct HotCompiler;

impl HotCompiler {
    pub fn new(_config: HotCompileConfig) -> Self {
        Self
    }
    
    /// Compile code (no-op for Rune)
    pub fn compile(&mut self, _code: &GeneratedCode) -> CompileResult {
        CompileResult {
            module_name: String::new(),
            success: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
    
    /// Start async compilation (no-op for Rune)
    pub fn compile_async(&mut self, _code: &GeneratedCode) {
        // No-op - Rune scripts don't need compilation
    }
    
    /// Poll for compilation result (always returns success for Rune)
    pub fn poll_result(&mut self) -> Option<CompileResult> {
        Some(CompileResult {
            module_name: String::new(),
            success: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        })
    }
    
    /// Check if module exists (stub)
    pub fn has_module(&self, _name: &str) -> bool {
        false
    }
    
    /// Cleanup module (no-op)
    pub fn cleanup_module(&mut self, _name: &str) {
        // No-op
    }
    
    /// Unload a module (no-op)
    pub fn unload_module(&mut self, _name: &str) {
        // No-op
    }
}

// ============================================================================
// Plugin (Stub)
// ============================================================================

/// Hot compile plugin (stub)
pub struct HotCompilePlugin;

impl Plugin for HotCompilePlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<HotCompileConfig>()
            .init_resource::<HotCompiler>();
    }
}

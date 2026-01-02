//! # Soul Builder (Stub)
//!
//! Legacy build job queue. Superseded by build_pipeline.rs.
//! Kept for interface compatibility.

use bevy::prelude::*;

// ============================================================================
// Build Job (Stub)
// ============================================================================

/// A build job in the queue (stub)
#[derive(Debug, Clone)]
pub struct BuildJob {
    pub scene: String,
    pub script: Option<String>,
    pub force: bool,
    pub status: BuildJobStatus,
    pub started_at: Option<std::time::Instant>,
}

/// Build job status (stub)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildJobStatus {
    Pending,
    Complete,
    Failed(String),
}

/// Build result (stub)
#[derive(Debug, Clone)]
pub struct BuildResult {
    pub scene: String,
    pub script: String,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
}

// ============================================================================
// Soul Builder (Stub)
// ============================================================================

/// Soul script builder (stub - use SoulBuildPipeline instead)
#[derive(Resource, Default)]
pub struct SoulBuilder;

impl SoulBuilder {
    /// Queue a build job (no-op stub)
    pub fn queue_build(&mut self, _job: BuildJob) {
        // No-op - use SoulBuildPipeline instead
    }
    
    /// Poll for build result (stub - always returns None)
    pub fn poll_result(&mut self) -> Option<BuildResult> {
        None
    }
    
    /// Get build status (stub)
    pub fn status(&self) -> &str {
        "Idle (use SoulBuildPipeline)"
    }
}

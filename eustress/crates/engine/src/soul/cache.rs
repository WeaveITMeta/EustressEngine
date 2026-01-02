//! # Soul Cache (Stub)
//!
//! Cache for generated code. Kept for interface compatibility.

use bevy::prelude::*;
use std::path::PathBuf;

// ============================================================================
// Soul Cache (Stub)
// ============================================================================

/// Soul script cache manager (stub)
#[derive(Resource, Default)]
pub struct SoulCache {
    cache_dir: PathBuf,
}

impl SoulCache {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }
}

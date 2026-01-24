//! # VIGA - Vision-as-Inverse-Graphics Agent
//!
//! Rust-first implementation of VIGA for Eustress Engine.
//! Converts reference images into 3D scenes through iterative multimodal reasoning.
//!
//! ## Architecture
//!
//! - **Generator**: Produces Rune scripts from image + feedback
//! - **Verifier**: Compares rendered scene to reference image
//! - **Pipeline**: Orchestrates the generate-render-verify loop
//!
//! ## Integration Points
//!
//! - Uses existing `ClaudeClient` for multimodal LLM calls
//! - Generates Rune scripts executed by `SoulBuildPipeline`
//! - Captures screenshots via Bevy for verification
//! - Runs as background process to not block UI

pub mod agent;
pub mod generator;
pub mod verifier;
pub mod pipeline;
pub mod image_utils;
pub mod context;

pub use agent::{VigaAgent, AgentRole, AgentState};
pub use generator::{VigaGenerator, GeneratorConfig};
pub use verifier::{VigaVerifier, VerifierConfig, VerificationResult};
pub use pipeline::{VigaPipeline, VigaPipelinePlugin, VigaRequest, VigaResult, VigaStatus};
pub use image_utils::{ImageData, encode_image_base64, decode_image_base64, compare_images};
pub use context::{VigaContext, IterationHistory, CodeDiff};

use bevy::prelude::*;

/// VIGA Plugin for Eustress Engine
pub struct VigaPlugin;

impl Plugin for VigaPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(VigaPipelinePlugin);
        
        info!("🎨 VIGA Plugin initialized - Vision-as-Inverse-Graphics Agent ready");
    }
}

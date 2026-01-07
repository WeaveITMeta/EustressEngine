//! # Spatial LLM Integration
//!
//! AI-powered spatial reasoning and generation for Eustress Engine.
//! Integrates with Large Language Models for intelligent spatial operations.
//!
//! ## Features
//!
//! - **Spatial Reasoning**: AI understands 3D space and relationships
//! - **Procedural Generation**: Generate environments based on natural language
//! - **Context-Aware NPCs**: NPCs that understand their spatial surroundings
//! - **Query Interface**: Natural language queries about spatial properties
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                        Spatial LLM System                              │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  Spatial Context Layer                                                  │
//! │  ├── SceneGraph: Hierarchical world representation                      │
//! │  ├── SpatialIndex: Fast nearest-neighbor queries                       │
//! │  └── ContextBuilder: Converts 3D data to LLM prompts                 │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  LLM Interface Layer                                                   │
//! │  ├── RemoteClient: OpenAI/Claude API integration                      │
//! │  ├── LocalModel: Candle-based local inference                         │
//! │  └── PromptEngine: Structured prompt generation                         │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  Generation Layer                                                      │
//! │  ├── ProceduralGenerator: Environment generation                       │
//! │  ├── BehaviorGenerator: NPC behavior patterns                         │
//! │  └── QueryProcessor: Natural language queries                          │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use eustress_spatial_llm::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), SpatialLlmError> {
//!     let llm = SpatialLlm::new(SpatialLlmConfig::default()).await?;
//!     
//!     // Generate a forest around the player
//!     let forest = llm.generate_environment(EnvironmentPrompt {
//!         description: "A dense pine forest with fog",
//!         center: Vec3::new(0.0, 0.0, 0.0),
//!         radius: 100.0,
//!         style: EnvironmentStyle::Realistic,
//!     }).await?;
//!     
//!     // Query spatial relationships
//!     let answer = llm.query_spatial("Where is the nearest cover from the player?").await?;
//!     println!("AI Answer: {}", answer);
//!     
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod context;
pub mod error;
pub mod generation;
pub mod indexing;
pub mod prompt;
pub mod query;

#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "remote")]
pub mod remote;

pub use client::SpatialLlm;
pub use context::{SpatialContext, SceneGraph};
pub use error::SpatialLlmError;
pub use generation::{EnvironmentGenerator, BehaviorGenerator};
pub use indexing::SpatialIndex;
pub use prompt::{PromptEngine, EnvironmentPrompt};
pub use query::{QueryProcessor, SpatialQuery};

// ============================================================================
// Prelude
// ============================================================================

/// Convenient re-exports for common Spatial LLM types.
pub mod prelude {
    pub use super::client::SpatialLlm;
    pub use super::client::SpatialLlmConfig;
    pub use super::context::{SpatialContext, SceneGraph, SpatialEntity};
    pub use super::error::SpatialLlmError;
    pub use super::generation::{EnvironmentGenerator, BehaviorGenerator, GeneratedContent};
    pub use super::indexing::SpatialIndex;
    pub use super::prompt::{PromptEngine, EnvironmentPrompt, EnvironmentStyle};
    pub use super::query::{QueryProcessor, SpatialQuery, QueryResult};
}
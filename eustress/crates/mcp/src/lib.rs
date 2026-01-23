//! # Eustress MCP (Model Control Protocol) Server
//!
//! AI-controllable 3D world server that enables AI models to perform precise CRUD
//! operations on entities via the Rune API on the native Rust platform.
//!
//! ## Architecture
//!
//! ```text
//! AI Model (Claude, GPT, Grok, etc.)
//!         ↓ (MCP calls over HTTP/WebSocket)
//! Eustress MCP Server (hosted on Eustress Forge)
//!         ↓
//! Rune API → Native Rust Engine (Bevy ECS)
//!         ↓
//! Create/Update/Delete Entities in Live Spaces
//!         ↓
//! Entities with AI = true → Automatically routed
//!         ↓
//! Parameter Router → External Export (via EEP)
//!         ↓
//! Back to AI Training Pipeline
//! ```
//!
//! ## Endpoints
//!
//! - `POST /mcp/create` - Create entity
//! - `POST /mcp/update` - Update entity
//! - `POST /mcp/delete` - Delete entity
//! - `GET /mcp/query` - Query entities
//! - `GET /mcp/space` - Get space info
//! - `WS /mcp/stream` - Real-time entity stream
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use eustress_mcp::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), McpError> {
//!     let server = McpServer::new(McpConfig::from_env()?);
//!     server.run().await?;
//!     Ok(())
//! }
//! ```

pub mod config;
pub mod error;
pub mod handlers;
pub mod protocol;
pub mod router;
pub mod server;
pub mod types;

#[cfg(feature = "embedvec")]
pub mod embedvec_target;

#[cfg(feature = "embedvec")]
pub use embedvec_target::{
    EmbedvecExportTarget, EmbedvecExportTargetBuilder, OntologyAwareExportTarget, TrainingRecord,
};

pub use config::McpConfig;
pub use error::McpError;
pub use protocol::*;
pub use server::McpServer;

// ============================================================================
// Prelude
// ============================================================================

/// Convenient re-exports for common MCP types.
pub mod prelude {
    pub use super::config::McpConfig;
    pub use super::error::McpError;
    pub use super::protocol::{
        CreateEntityRequest, UpdateEntityRequest, DeleteEntityRequest,
        QueryEntitiesRequest, EntityResponse, SpaceInfo,
        McpCapability, McpProtocolVersion,
    };
    pub use super::server::McpServer;
    pub use super::types::{EntityData, TransformData, PropertyMap};
}

//! MCP Server - Main server implementation.
//!
//! ## Architecture (EustressStream-backed)
//!
//! ```text
//! HTTP handler (create_entity)
//!     → stream.producer("mcp.entity.create").send(&entity)
//!         ↳ McpRouter subscriber    → AI consent check → export record → "mcp.exports"
//!         ↳ ChangeQueue subscriber  → spawn entity in Bevy ECS
//!         ↳ Properties panel        → refresh UI
//!         ↳ Any future subscriber   → zero-copy access, <1 µs
//! ```
//!
//! All inter-component communication uses named EustressStream topics instead of
//! point-to-point `tokio::sync::mpsc` channels. This gives fan-out (N subscribers),
//! replay (ring buffer), persistence (optional segments), and multi-transport
//! (in-process / SHM / TCP / QUIC) for free.

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use eustress_stream::{EustressStream, StreamConfig};

use crate::{
    config::McpConfig,
    error::{McpError, McpResult},
    handlers,
    router::McpRouter,
};

// ─────────────────────────────────────────────────────────────────────────────
// Well-known MCP topic names
// ─────────────────────────────────────────────────────────────────────────────

/// Well-known EustressStream topic names for MCP operations.
/// Subscribe to these on any `EustressStream` clone to observe MCP activity.
pub mod topics {
    /// Entity creation operations (payload: JSON-serialized `EntityData`).
    pub const ENTITY_CREATE: &str = "mcp.entity.create";
    /// Entity update operations (payload: JSON-serialized `UpdateEntityRequest`).
    pub const ENTITY_UPDATE: &str = "mcp.entity.update";
    /// Entity deletion operations (payload: JSON-serialized `DeleteEntityRequest`).
    pub const ENTITY_DELETE: &str = "mcp.entity.delete";
    /// EEP export records produced by the router (payload: JSON-serialized `EepExportRecord`).
    pub const EXPORTS: &str = "mcp.exports";
}

// ─────────────────────────────────────────────────────────────────────────────
// McpState — shared across Axum handlers
// ─────────────────────────────────────────────────────────────────────────────

/// MCP Server state shared across handlers.
///
/// Handlers publish to EustressStream topics. Any number of subscribers
/// (McpRouter, ChangeQueue, UI panels, export targets) can observe operations.
pub struct McpState {
    /// Configuration
    pub config: McpConfig,
    /// Shared EustressStream — handlers publish operations here.
    /// Clone this to subscribe from any context.
    pub stream: EustressStream,
}

// ─────────────────────────────────────────────────────────────────────────────
// McpServer
// ─────────────────────────────────────────────────────────────────────────────

/// MCP Server — Axum HTTP server backed by EustressStream.
pub struct McpServer {
    config: McpConfig,
    state: Arc<McpState>,
}

impl McpServer {
    /// Create a new MCP server with its own private EustressStream.
    ///
    /// Use [`McpServer::with_stream`] to share a stream with other subsystems
    /// (e.g. the engine's `ChangeQueue`).
    pub fn new(config: McpConfig) -> Self {
        let stream = EustressStream::new(StreamConfig::default().in_memory());
        Self::with_stream(config, stream)
    }

    /// Create a new MCP server that publishes to an existing EustressStream.
    ///
    /// Pass `change_queue.stream.clone()` here to unify MCP operations with
    /// the engine's scene delta pipeline.
    pub fn with_stream(config: McpConfig, stream: EustressStream) -> Self {
        let state = Arc::new(McpState {
            config: config.clone(),
            stream,
        });

        Self { config, state }
    }

    /// Access the underlying EustressStream (e.g. to register export targets).
    pub fn stream(&self) -> &EustressStream {
        &self.state.stream
    }

    /// Build the Axum router
    fn build_router(&self) -> Router {
        // CORS configuration
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        Router::new()
            // Health & Info
            .route("/mcp/health", get(handlers::health_check))
            .route("/mcp/capabilities", get(handlers::get_capabilities))
            // Entity CRUD
            .route("/mcp/create", post(handlers::create_entity))
            .route("/mcp/update", post(handlers::update_entity))
            .route("/mcp/delete", post(handlers::delete_entity))
            .route("/mcp/query", post(handlers::query_entities))
            // Space info
            .route("/mcp/space/:space_id", get(handlers::get_space_info))
            // Batch operations
            .route("/mcp/batch/create", post(handlers::batch_create))
            .route("/mcp/batch/delete", post(handlers::batch_delete))
            // Middleware
            .layer(cors)
            .layer(TraceLayer::new_for_http())
            .with_state(self.state.clone())
    }

    /// Run the MCP server.
    ///
    /// Registers the `McpRouter` as a stream subscriber, then starts the
    /// Axum HTTP listener. The router processes operations via zero-copy
    /// callbacks — no background task polling a channel.
    pub async fn run(self) -> McpResult<()> {
        let addr = self.config.address();
        tracing::info!("Starting MCP server on {}", addr);

        // Build the HTTP router before moving fields
        let app = self.build_router();

        // Register the McpRouter as a stream subscriber (handles AI consent + export)
        McpRouter::register(&self.state.stream);

        let listener = tokio::net::TcpListener::bind(&addr).await
            .map_err(|e| McpError::Internal(format!("Failed to bind: {}", e)))?;

        tracing::info!("MCP server listening on http://{}", addr);
        tracing::info!("Protocol version: {}", self.config.protocol_version);

        axum::serve(listener, app).await
            .map_err(|e| McpError::Internal(format!("Server error: {}", e)))?;

        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// McpServerBuilder
// ─────────────────────────────────────────────────────────────────────────────

/// Builder for MCP server with export targets.
///
/// Export targets register as independent EustressStream subscribers on the
/// `"mcp.exports"` topic. Each target receives records in parallel — a slow
/// webhook does not block the console logger or file writer.
pub struct McpServerBuilder {
    config: McpConfig,
    stream: Option<EustressStream>,
}

impl McpServerBuilder {
    /// Create a new builder
    pub fn new(config: McpConfig) -> Self {
        Self {
            config,
            stream: None,
        }
    }

    /// Share an existing EustressStream (e.g. from ChangeQueue).
    pub fn with_stream(mut self, stream: EustressStream) -> Self {
        self.stream = Some(stream);
        self
    }

    /// Register a webhook export target that subscribes to `"mcp.exports"`.
    pub fn with_webhook(self, name: &str, endpoint: &str, api_key: Option<&str>) -> Self {
        let target = crate::router::WebhookExportTarget::new(
            name.to_string(),
            endpoint.to_string(),
            api_key.map(String::from),
        );
        // Target will be registered as a stream subscriber in build()
        let stream = self.effective_stream();
        crate::router::register_webhook_subscriber(&stream, target);
        self
    }

    /// Register a console export target (for debugging) that subscribes to `"mcp.exports"`.
    pub fn with_console(self, name: &str) -> Self {
        let stream = self.effective_stream();
        crate::router::register_console_subscriber(&stream, name.to_string());
        self
    }

    /// Register a file export target that subscribes to `"mcp.exports"`.
    pub fn with_file(self, name: &str, output_dir: &std::path::Path) -> Self {
        let target = crate::router::FileExportTarget::new(
            name.to_string(),
            output_dir.to_path_buf(),
        );
        let stream = self.effective_stream();
        crate::router::register_file_subscriber(&stream, target);
        self
    }

    /// Build the server
    pub fn build(self) -> McpServer {
        let stream = self.stream.unwrap_or_else(||
            EustressStream::new(StreamConfig::default().in_memory())
        );
        McpServer::with_stream(self.config, stream)
    }

    /// Get or create the stream for registering subscribers during build.
    fn effective_stream(&self) -> EustressStream {
        self.stream.clone().unwrap_or_else(||
            EustressStream::new(StreamConfig::default().in_memory())
        )
    }
}

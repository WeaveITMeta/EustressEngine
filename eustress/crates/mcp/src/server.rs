//! MCP Server - Main server implementation.

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tokio::sync::mpsc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::{
    config::McpConfig,
    error::{McpError, McpResult},
    handlers::{self, EntityOperation},
    router::McpRouter,
};

/// MCP Server state shared across handlers
pub struct McpState {
    /// Configuration
    pub config: McpConfig,
    /// Channel for sending entity operations to router
    pub entity_tx: mpsc::Sender<EntityOperation>,
}

/// MCP Server
pub struct McpServer {
    config: McpConfig,
    state: Arc<McpState>,
    router_rx: mpsc::Receiver<EntityOperation>,
}

impl McpServer {
    /// Create a new MCP server
    pub fn new(config: McpConfig) -> Self {
        let (tx, rx) = mpsc::channel(1000);

        let state = Arc::new(McpState {
            config: config.clone(),
            entity_tx: tx,
        });

        Self {
            config,
            state,
            router_rx: rx,
        }
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

    /// Run the MCP server
    pub async fn run(self) -> McpResult<()> {
        let addr = self.config.address();
        tracing::info!("Starting MCP server on {}", addr);

        // Start the router in a background task
        let router = McpRouter::new(self.router_rx);
        tokio::spawn(async move {
            router.run().await;
        });

        // Build and run the HTTP server
        let app = self.build_router();
        let listener = tokio::net::TcpListener::bind(&addr).await
            .map_err(|e| McpError::Internal(format!("Failed to bind: {}", e)))?;

        tracing::info!("MCP server listening on http://{}", addr);
        tracing::info!("Protocol version: {}", self.config.protocol_version);

        axum::serve(listener, app).await
            .map_err(|e| McpError::Internal(format!("Server error: {}", e)))?;

        Ok(())
    }
}

/// Builder for MCP server with export targets
pub struct McpServerBuilder {
    config: McpConfig,
    export_targets: Vec<Arc<dyn crate::router::ExportTarget>>,
}

impl McpServerBuilder {
    /// Create a new builder
    pub fn new(config: McpConfig) -> Self {
        Self {
            config,
            export_targets: Vec::new(),
        }
    }

    /// Add an export target
    pub fn with_target(mut self, target: Arc<dyn crate::router::ExportTarget>) -> Self {
        self.export_targets.push(target);
        self
    }

    /// Add a webhook export target
    pub fn with_webhook(self, name: &str, endpoint: &str, api_key: Option<&str>) -> Self {
        let target = Arc::new(crate::router::WebhookExportTarget::new(
            name.to_string(),
            endpoint.to_string(),
            api_key.map(String::from),
        ));
        self.with_target(target)
    }

    /// Add a console export target (for debugging)
    pub fn with_console(self, name: &str) -> Self {
        let target = Arc::new(crate::router::ConsoleExportTarget::new(name.to_string()));
        self.with_target(target)
    }

    /// Add a file export target
    pub fn with_file(self, name: &str, output_dir: &std::path::Path) -> Self {
        let target = Arc::new(crate::router::FileExportTarget::new(
            name.to_string(),
            output_dir.to_path_buf(),
        ));
        self.with_target(target)
    }

    /// Build the server
    pub fn build(self) -> McpServer {
        McpServer::new(self.config)
    }
}

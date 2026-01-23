//! REST API Server for the Control Plane
//!
//! Provides a Kubernetes-style API with:
//! - RESTful CRUD operations
//! - Watch endpoints for real-time updates
//! - Subresource endpoints (status, scale)
//! - OpenAPI schema generation

use std::sync::Arc;
use axum::{
    Router,
    routing::{get, post, put, delete},
    extract::{Path, Query, State, Json},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::{ResourceKind, ResourceStore, StoreError};

/// API Server configuration
#[derive(Debug, Clone)]
pub struct ApiServerConfig {
    /// Listen address
    pub listen_addr: String,
    /// Enable TLS
    pub tls_enabled: bool,
    /// TLS cert path
    pub tls_cert: Option<String>,
    /// TLS key path
    pub tls_key: Option<String>,
    /// Enable authentication
    pub auth_enabled: bool,
    /// Enable admission controllers
    pub admission_enabled: bool,
}

impl Default for ApiServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:6443".to_string(),
            tls_enabled: false,
            tls_cert: None,
            tls_key: None,
            auth_enabled: false,
            admission_enabled: true,
        }
    }
}

/// API Server state
#[derive(Clone)]
pub struct ApiServerState {
    /// Resource store
    pub store: Arc<ResourceStore>,
}

/// API Server
pub struct ApiServer {
    config: ApiServerConfig,
    state: ApiServerState,
}

impl ApiServer {
    /// Create new API server
    pub fn new(config: ApiServerConfig, store: Arc<ResourceStore>) -> Self {
        Self {
            config,
            state: ApiServerState { store },
        }
    }

    /// Build the router
    pub fn router(&self) -> Router {
        Router::new()
            // Health endpoints
            .route("/healthz", get(health))
            .route("/readyz", get(ready))
            .route("/livez", get(live))
            
            // API discovery
            .route("/api", get(api_versions))
            .route("/apis", get(api_groups))
            
            // Core API v1
            .route("/api/v1/namespaces/:namespace/workloads", 
                get(list_workloads).post(create_workload))
            .route("/api/v1/namespaces/:namespace/workloads/:name",
                get(get_workload).put(update_workload).delete(delete_workload))
            .route("/api/v1/namespaces/:namespace/workloads/:name/status",
                get(get_workload_status).put(update_workload_status))
            
            // Nodes (cluster-scoped)
            .route("/api/v1/nodes", get(list_nodes).post(create_node))
            .route("/api/v1/nodes/:name", get(get_node).put(update_node).delete(delete_node))
            
            // Watch endpoints
            .route("/api/v1/watch/namespaces/:namespace/workloads", get(watch_workloads))
            .route("/api/v1/watch/nodes", get(watch_nodes))
            
            .with_state(self.state.clone())
    }

    /// Start the API server
    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let router = self.router();
        let listener = tokio::net::TcpListener::bind(&self.config.listen_addr).await?;
        
        info!(addr = %self.config.listen_addr, "API server starting");
        axum::serve(listener, router).await?;
        
        Ok(())
    }
}

// Health endpoints
async fn health() -> impl IntoResponse {
    StatusCode::OK
}

async fn ready() -> impl IntoResponse {
    StatusCode::OK
}

async fn live() -> impl IntoResponse {
    StatusCode::OK
}

// API discovery
async fn api_versions() -> impl IntoResponse {
    Json(serde_json::json!({
        "kind": "APIVersions",
        "versions": ["v1"],
        "serverAddressByClientCIDRs": []
    }))
}

async fn api_groups() -> impl IntoResponse {
    Json(serde_json::json!({
        "kind": "APIGroupList",
        "apiVersion": "v1",
        "groups": [
            {
                "name": "forge.io",
                "versions": [
                    {"groupVersion": "forge.io/v1", "version": "v1"}
                ],
                "preferredVersion": {"groupVersion": "forge.io/v1", "version": "v1"}
            }
        ]
    }))
}

/// Query parameters for list operations
#[derive(Debug, Deserialize)]
pub struct ListParams {
    /// Label selector
    #[serde(rename = "labelSelector")]
    pub label_selector: Option<String>,
    /// Field selector
    #[serde(rename = "fieldSelector")]
    pub field_selector: Option<String>,
    /// Limit results
    pub limit: Option<u32>,
    /// Continue token
    #[serde(rename = "continue")]
    pub continue_token: Option<String>,
    /// Resource version for watch
    #[serde(rename = "resourceVersion")]
    pub resource_version: Option<u64>,
}

/// API response wrapper
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: ListMeta,
    pub items: Vec<T>,
}

/// List metadata
#[derive(Debug, Serialize)]
pub struct ListMeta {
    #[serde(rename = "resourceVersion")]
    pub resource_version: String,
    #[serde(rename = "continue", skip_serializing_if = "Option::is_none")]
    pub continue_token: Option<String>,
}

/// API error response
#[derive(Debug, Serialize)]
pub struct ApiError {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub status: String,
    pub message: String,
    pub reason: String,
    pub code: u16,
}

impl ApiError {
    fn not_found(resource: &str, name: &str) -> Self {
        Self {
            api_version: "v1".to_string(),
            kind: "Status".to_string(),
            status: "Failure".to_string(),
            message: format!("{} \"{}\" not found", resource, name),
            reason: "NotFound".to_string(),
            code: 404,
        }
    }

    fn already_exists(resource: &str, name: &str) -> Self {
        Self {
            api_version: "v1".to_string(),
            kind: "Status".to_string(),
            status: "Failure".to_string(),
            message: format!("{} \"{}\" already exists", resource, name),
            reason: "AlreadyExists".to_string(),
            code: 409,
        }
    }

    fn conflict(message: &str) -> Self {
        Self {
            api_version: "v1".to_string(),
            kind: "Status".to_string(),
            status: "Failure".to_string(),
            message: message.to_string(),
            reason: "Conflict".to_string(),
            code: 409,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = StatusCode::from_u16(self.code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self)).into_response()
    }
}

// Workload handlers
async fn list_workloads(
    State(state): State<ApiServerState>,
    Path(namespace): Path<String>,
    Query(_params): Query<ListParams>,
) -> impl IntoResponse {
    let items = state.store.list(&ResourceKind::Workload, Some(&namespace));
    
    Json(ApiResponse {
        api_version: "forge.io/v1".to_string(),
        kind: "WorkloadList".to_string(),
        metadata: ListMeta {
            resource_version: state.store.current_version().to_string(),
            continue_token: None,
        },
        items,
    })
}

async fn create_workload(
    State(state): State<ApiServerState>,
    Path(namespace): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, ApiError> {
    let name = body.get("metadata")
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| ApiError {
            api_version: "v1".to_string(),
            kind: "Status".to_string(),
            status: "Failure".to_string(),
            message: "metadata.name is required".to_string(),
            reason: "Invalid".to_string(),
            code: 400,
        })?;

    let key = format!("{}/{}", namespace, name);
    
    state.store.create(ResourceKind::Workload, &key, body.clone())
        .map_err(|e| match e {
            StoreError::AlreadyExists(_) => ApiError::already_exists("workload", name),
            _ => ApiError {
                api_version: "v1".to_string(),
                kind: "Status".to_string(),
                status: "Failure".to_string(),
                message: e.to_string(),
                reason: "InternalError".to_string(),
                code: 500,
            },
        })?;

    Ok((StatusCode::CREATED, Json(body)))
}

async fn get_workload(
    State(state): State<ApiServerState>,
    Path((namespace, name)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let key = format!("{}/{}", namespace, name);
    
    state.store.get(&ResourceKind::Workload, &key)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("workload", &name))
}

async fn update_workload(
    State(state): State<ApiServerState>,
    Path((namespace, name)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, ApiError> {
    let key = format!("{}/{}", namespace, name);
    
    // Extract resource version for optimistic concurrency
    let resource_version = body.get("metadata")
        .and_then(|m| m.get("resourceVersion"))
        .and_then(|v| v.as_str())
        .and_then(|v| v.parse::<u64>().ok());

    state.store.update(ResourceKind::Workload, &key, body.clone(), resource_version)
        .map_err(|e| match e {
            StoreError::NotFound(_) => ApiError::not_found("workload", &name),
            StoreError::Conflict(expected, actual) => {
                ApiError::conflict(&format!("resource version mismatch: expected {}, got {}", expected, actual))
            }
            _ => ApiError {
                api_version: "v1".to_string(),
                kind: "Status".to_string(),
                status: "Failure".to_string(),
                message: e.to_string(),
                reason: "InternalError".to_string(),
                code: 500,
            },
        })?;

    Ok(Json(body))
}

async fn delete_workload(
    State(state): State<ApiServerState>,
    Path((namespace, name)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let key = format!("{}/{}", namespace, name);
    
    state.store.delete(&ResourceKind::Workload, &key)
        .map_err(|e| match e {
            StoreError::NotFound(_) => ApiError::not_found("workload", &name),
            _ => ApiError {
                api_version: "v1".to_string(),
                kind: "Status".to_string(),
                status: "Failure".to_string(),
                message: e.to_string(),
                reason: "InternalError".to_string(),
                code: 500,
            },
        })?;

    Ok(StatusCode::OK)
}

async fn get_workload_status(
    State(state): State<ApiServerState>,
    Path((namespace, name)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let key = format!("{}/{}", namespace, name);
    
    let workload = state.store.get(&ResourceKind::Workload, &key)
        .ok_or_else(|| ApiError::not_found("workload", &name))?;

    // Return just the status subresource
    let status = workload.get("status").cloned().unwrap_or(serde_json::json!({}));
    Ok(Json(status))
}

async fn update_workload_status(
    State(state): State<ApiServerState>,
    Path((namespace, name)): Path<(String, String)>,
    Json(status): Json<serde_json::Value>,
) -> Result<impl IntoResponse, ApiError> {
    let key = format!("{}/{}", namespace, name);
    
    let mut workload = state.store.get(&ResourceKind::Workload, &key)
        .ok_or_else(|| ApiError::not_found("workload", &name))?;

    // Update only the status field
    if let Some(obj) = workload.as_object_mut() {
        obj.insert("status".to_string(), status.clone());
    }

    state.store.update(ResourceKind::Workload, &key, workload.clone(), None)
        .map_err(|e| ApiError {
            api_version: "v1".to_string(),
            kind: "Status".to_string(),
            status: "Failure".to_string(),
            message: e.to_string(),
            reason: "InternalError".to_string(),
            code: 500,
        })?;

    Ok(Json(status))
}

// Node handlers
async fn list_nodes(
    State(state): State<ApiServerState>,
    Query(_params): Query<ListParams>,
) -> impl IntoResponse {
    let items = state.store.list(&ResourceKind::Node, None);
    
    Json(ApiResponse {
        api_version: "v1".to_string(),
        kind: "NodeList".to_string(),
        metadata: ListMeta {
            resource_version: state.store.current_version().to_string(),
            continue_token: None,
        },
        items,
    })
}

async fn create_node(
    State(state): State<ApiServerState>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, ApiError> {
    let name = body.get("metadata")
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        .ok_or_else(|| ApiError {
            api_version: "v1".to_string(),
            kind: "Status".to_string(),
            status: "Failure".to_string(),
            message: "metadata.name is required".to_string(),
            reason: "Invalid".to_string(),
            code: 400,
        })?;

    state.store.create(ResourceKind::Node, name, body.clone())
        .map_err(|e| match e {
            StoreError::AlreadyExists(_) => ApiError::already_exists("node", name),
            _ => ApiError {
                api_version: "v1".to_string(),
                kind: "Status".to_string(),
                status: "Failure".to_string(),
                message: e.to_string(),
                reason: "InternalError".to_string(),
                code: 500,
            },
        })?;

    Ok((StatusCode::CREATED, Json(body)))
}

async fn get_node(
    State(state): State<ApiServerState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    state.store.get(&ResourceKind::Node, &name)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("node", &name))
}

async fn update_node(
    State(state): State<ApiServerState>,
    Path(name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, ApiError> {
    state.store.update(ResourceKind::Node, &name, body.clone(), None)
        .map_err(|e| match e {
            StoreError::NotFound(_) => ApiError::not_found("node", &name),
            _ => ApiError {
                api_version: "v1".to_string(),
                kind: "Status".to_string(),
                status: "Failure".to_string(),
                message: e.to_string(),
                reason: "InternalError".to_string(),
                code: 500,
            },
        })?;

    Ok(Json(body))
}

async fn delete_node(
    State(state): State<ApiServerState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    state.store.delete(&ResourceKind::Node, &name)
        .map_err(|e| match e {
            StoreError::NotFound(_) => ApiError::not_found("node", &name),
            _ => ApiError {
                api_version: "v1".to_string(),
                kind: "Status".to_string(),
                status: "Failure".to_string(),
                message: e.to_string(),
                reason: "InternalError".to_string(),
                code: 500,
            },
        })?;

    Ok(StatusCode::OK)
}

// Watch handlers (simplified - real implementation would use SSE)
async fn watch_workloads(
    State(_state): State<ApiServerState>,
    Path(_namespace): Path<String>,
    Query(_params): Query<ListParams>,
) -> impl IntoResponse {
    // In a real implementation, this would return an SSE stream
    Json(serde_json::json!({
        "type": "ADDED",
        "object": {}
    }))
}

async fn watch_nodes(
    State(_state): State<ApiServerState>,
    Query(_params): Query<ListParams>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "type": "ADDED",
        "object": {}
    }))
}

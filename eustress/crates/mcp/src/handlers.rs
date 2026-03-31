//! HTTP handlers for MCP endpoints.
//!
//! Handlers publish entity operations to EustressStream topics for fan-out
//! dispatch. Any number of subscribers (McpRouter, ChangeQueue, UI panels,
//! export targets) can observe operations with <1 µs latency.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;

use crate::{
    error::McpError,
    protocol::*,
    server::{McpState, topics},
    types::*,
};

// ============================================================================
// Create Entity
// ============================================================================

/// POST /mcp/create - Create a new entity
pub async fn create_entity(
    State(state): State<Arc<McpState>>,
    Json(request): Json<CreateEntityRequest>,
) -> Result<Json<EntityResponse>, McpError> {
    tracing::info!(
        space_id = %request.space_id,
        class = %request.class,
        "Creating entity"
    );

    // Validate request
    if request.space_id.is_empty() {
        return Err(McpError::InvalidRequest("space_id is required".into()));
    }

    // Create entity data
    let entity = EntityData {
        id: uuid::Uuid::new_v4().to_string(),
        name: request.name.unwrap_or_else(|| request.class.clone()),
        class: request.class,
        parent: request.parent,
        children: Vec::new(),
        transform: TransformData {
            position: request.position.unwrap_or([0.0, 0.0, 0.0]),
            rotation: euler_to_quat(request.rotation.unwrap_or([0.0, 0.0, 0.0])),
            scale: request.scale.unwrap_or([1.0, 1.0, 1.0]),
        },
        properties: request.properties,
        tags: request.tags,
        attributes: std::collections::HashMap::new(),
        ai: request.ai,
        network_ownership: NetworkOwnership::ServerOnly,
        parameters: request.parameters,
    };

    // Publish to EustressStream — all subscribers notified synchronously (<1 µs)
    state.stream.producer(topics::ENTITY_CREATE)
        .send(&entity)
        .map_err(|e| McpError::Internal(format!("Stream publish failed: {e}")))?;

    Ok(Json(EntityResponse {
        success: true,
        entity: Some(entity),
        error: None,
    }))
}

// ============================================================================
// Update Entity
// ============================================================================

/// POST /mcp/update - Update an existing entity
pub async fn update_entity(
    State(state): State<Arc<McpState>>,
    Json(request): Json<UpdateEntityRequest>,
) -> Result<Json<EntityResponse>, McpError> {
    tracing::info!(
        space_id = %request.space_id,
        entity_id = %request.entity_id,
        "Updating entity"
    );

    // Validate request
    if request.space_id.is_empty() || request.entity_id.is_empty() {
        return Err(McpError::InvalidRequest("space_id and entity_id are required".into()));
    }

    // Publish to EustressStream — all subscribers notified synchronously
    state.stream.producer(topics::ENTITY_UPDATE)
        .send(&request)
        .map_err(|e| McpError::Internal(format!("Stream publish failed: {e}")))?;

    // Return success (actual update happens via stream subscribers)
    Ok(Json(EntityResponse {
        success: true,
        entity: None,
        error: None,
    }))
}

// ============================================================================
// Delete Entity
// ============================================================================

/// POST /mcp/delete - Delete an entity
pub async fn delete_entity(
    State(state): State<Arc<McpState>>,
    Json(request): Json<DeleteEntityRequest>,
) -> Result<Json<DeleteResponse>, McpError> {
    tracing::info!(
        space_id = %request.space_id,
        entity_id = %request.entity_id,
        recursive = %request.recursive,
        "Deleting entity"
    );

    // Validate request
    if request.space_id.is_empty() || request.entity_id.is_empty() {
        return Err(McpError::InvalidRequest("space_id and entity_id are required".into()));
    }

    // Publish to EustressStream — all subscribers notified synchronously
    state.stream.producer(topics::ENTITY_DELETE)
        .send(&request)
        .map_err(|e| McpError::Internal(format!("Stream publish failed: {e}")))?;

    Ok(Json(DeleteResponse {
        success: true,
        deleted_count: 1,
        error: None,
    }))
}

// ============================================================================
// Query Entities
// ============================================================================

/// POST /mcp/query - Query entities
pub async fn query_entities(
    State(_state): State<Arc<McpState>>,
    Json(request): Json<QueryEntitiesRequest>,
) -> Result<Json<QueryResponse>, McpError> {
    tracing::info!(
        space_id = %request.space_id,
        ai_only = %request.ai_only,
        "Querying entities"
    );

    // Validate request
    if request.space_id.is_empty() {
        return Err(McpError::InvalidRequest("space_id is required".into()));
    }

    // TODO: Implement actual query against Forge/Engine
    // For now, return empty results
    Ok(Json(QueryResponse {
        success: true,
        entities: Vec::new(),
        total: 0,
        error: None,
    }))
}

// ============================================================================
// Space Info
// ============================================================================

/// GET /mcp/space/:space_id - Get space information
pub async fn get_space_info(
    State(_state): State<Arc<McpState>>,
    Path(space_id): Path<String>,
) -> Result<Json<SpaceInfo>, McpError> {
    tracing::info!(space_id = %space_id, "Getting space info");

    if space_id.is_empty() {
        return Err(McpError::InvalidRequest("space_id is required".into()));
    }

    // TODO: Implement actual space lookup
    Ok(Json(SpaceInfo {
        success: true,
        space: Some(SpaceData {
            id: space_id.clone(),
            name: format!("Space {}", space_id),
            description: String::new(),
            entity_count: 0,
            player_count: 0,
            settings: SpaceSettings::default(),
            created_at: chrono::Utc::now(),
            modified_at: chrono::Utc::now(),
        }),
        error: None,
    }))
}

// ============================================================================
// Batch Operations
// ============================================================================

/// POST /mcp/batch/create - Batch create entities
pub async fn batch_create(
    State(state): State<Arc<McpState>>,
    Json(request): Json<BatchCreateRequest>,
) -> Result<Json<BatchResponse>, McpError> {
    tracing::info!(
        space_id = %request.space_id,
        count = %request.entities.len(),
        "Batch creating entities"
    );

    let mut results = Vec::new();
    let mut succeeded = 0;
    let mut failed = 0;
    let producer = state.stream.producer(topics::ENTITY_CREATE);

    for (index, create_req) in request.entities.into_iter().enumerate() {
        let entity_id = uuid::Uuid::new_v4().to_string();

        let entity = EntityData {
            id: entity_id.clone(),
            name: create_req.name.unwrap_or_else(|| create_req.class.clone()),
            class: create_req.class,
            parent: create_req.parent,
            children: Vec::new(),
            transform: TransformData {
                position: create_req.position.unwrap_or([0.0, 0.0, 0.0]),
                rotation: euler_to_quat(create_req.rotation.unwrap_or([0.0, 0.0, 0.0])),
                scale: create_req.scale.unwrap_or([1.0, 1.0, 1.0]),
            },
            properties: create_req.properties,
            tags: create_req.tags,
            attributes: std::collections::HashMap::new(),
            ai: create_req.ai,
            network_ownership: NetworkOwnership::ServerOnly,
            parameters: create_req.parameters,
        };

        match producer.send(&entity) {
            Ok(_) => {
                results.push(OperationResult {
                    index,
                    success: true,
                    entity_id: Some(entity_id),
                    error: None,
                });
                succeeded += 1;
            }
            Err(e) => {
                results.push(OperationResult {
                    index,
                    success: false,
                    entity_id: Some(entity_id),
                    error: Some(format!("Stream publish failed: {e}")),
                });
                failed += 1;
            }
        }
    }

    Ok(Json(BatchResponse {
        success: failed == 0,
        total: results.len(),
        succeeded,
        failed,
        results,
    }))
}

/// POST /mcp/batch/delete - Batch delete entities
pub async fn batch_delete(
    State(state): State<Arc<McpState>>,
    Json(request): Json<BatchDeleteRequest>,
) -> Result<Json<BatchResponse>, McpError> {
    tracing::info!(
        space_id = %request.space_id,
        count = %request.entity_ids.len(),
        "Batch deleting entities"
    );

    let mut results = Vec::new();
    let mut succeeded = 0;
    let mut failed = 0;
    let producer = state.stream.producer(topics::ENTITY_DELETE);

    for (index, entity_id) in request.entity_ids.into_iter().enumerate() {
        let delete_req = DeleteEntityRequest {
            space_id: request.space_id.clone(),
            entity_id: entity_id.clone(),
            recursive: request.recursive,
        };

        match producer.send(&delete_req) {
            Ok(_) => {
                results.push(OperationResult {
                    index,
                    success: true,
                    entity_id: Some(entity_id),
                    error: None,
                });
                succeeded += 1;
            }
            Err(e) => {
                results.push(OperationResult {
                    index,
                    success: false,
                    entity_id: Some(entity_id),
                    error: Some(format!("Stream publish failed: {e}")),
                });
                failed += 1;
            }
        }
    }

    Ok(Json(BatchResponse {
        success: failed == 0,
        total: results.len(),
        succeeded,
        failed,
        results,
    }))
}

// ============================================================================
// Health & Info
// ============================================================================

/// GET /mcp/health - Health check
pub async fn health_check(
    State(state): State<Arc<McpState>>,
) -> impl IntoResponse {
    // Include stream stats in health response
    let topic_count = state.stream.topics().len();
    (StatusCode::OK, Json(serde_json::json!({
        "status": "healthy",
        "protocol_version": "eep_v1",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "stream": {
            "topics": topic_count,
            "transport": "in_process"
        }
    })))
}

/// GET /mcp/capabilities - Get server capabilities
pub async fn get_capabilities() -> impl IntoResponse {
    Json(serde_json::json!({
        "protocol_version": {
            "major": 1,
            "minor": 0,
            "name": "eep_v1"
        },
        "capabilities": [
            { "name": "entity_crud", "supported": true },
            { "name": "spatial_export", "supported": true },
            { "name": "training_data", "supported": true },
            { "name": "rune_execution", "supported": false },
            { "name": "realtime_streaming", "supported": true },
            { "name": "batch_export", "supported": true },
            { "name": "query", "supported": true },
            { "name": "eustress_stream", "supported": true }
        ]
    }))
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Convert Euler angles (degrees) to quaternion
fn euler_to_quat(euler: [f32; 3]) -> [f32; 4] {
    let (roll, pitch, yaw) = (
        euler[0].to_radians() / 2.0,
        euler[1].to_radians() / 2.0,
        euler[2].to_radians() / 2.0,
    );

    let (sr, cr) = roll.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let (sy, cy) = yaw.sin_cos();

    [
        sr * cp * cy - cr * sp * sy, // x
        cr * sp * cy + sr * cp * sy, // y
        cr * cp * sy - sr * sp * cy, // z
        cr * cp * cy + sr * sp * sy, // w
    ]
}

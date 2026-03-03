// =============================================================================
// Eustress Backend - Projects API
// =============================================================================
// Table of Contents:
// 1. Imports
// 2. Request/Response Types
// 3. Handlers - get_projects, create_project, get_recent_projects, get_project,
//               update_project, delete_project, publish_project
// =============================================================================

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::{extract_token, validate_token};
use crate::error::AppError;
use crate::AppState;

// =============================================================================
// 2. Request/Response Types
// =============================================================================

/// Response for a single project.
#[derive(Debug, Serialize)]
pub struct ProjectResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub owner_id: String,
    pub is_published: bool,
    pub experience_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<crate::db::Project> for ProjectResponse {
    fn from(project: crate::db::Project) -> Self {
        Self {
            id: project.id,
            name: project.name,
            description: project.description,
            owner_id: project.owner_id,
            is_published: project.is_published,
            experience_id: project.experience_id,
            created_at: project.created_at,
            updated_at: project.updated_at,
        }
    }
}

/// Request body for creating a project.
#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
}

/// Request body for updating a project.
#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: String,
    pub description: Option<String>,
}

/// Request body for publishing a project.
#[derive(Debug, Deserialize)]
pub struct PublishProjectRequest {
    pub experience_id: String,
}

// =============================================================================
// 3. Handlers
// =============================================================================

/// Get all projects for the authenticated user.
/// GET /api/projects
pub async fn get_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    let projects = state.db.get_user_projects(&claims.sub).await?;
    let responses: Vec<ProjectResponse> = projects.into_iter().map(Into::into).collect();
    
    Ok(Json(serde_json::json!({ "projects": responses })))
}

/// Create a new project for the authenticated user.
/// POST /api/projects
pub async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<ProjectResponse>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    let id = uuid::Uuid::new_v4().to_string();
    let project = state.db.create_project(
        &id,
        &req.name,
        req.description.as_deref(),
        &claims.sub,
    ).await?;
    
    Ok(Json(project.into()))
}

/// Get recent projects for the authenticated user.
/// GET /api/projects/recent
pub async fn get_recent_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    let projects = state.db.get_recent_projects(&claims.sub, 10).await?;
    let responses: Vec<ProjectResponse> = projects.into_iter().map(Into::into).collect();
    
    Ok(Json(serde_json::json!({ "projects": responses })))
}

/// Get a single project by ID (authenticated, must be owner).
/// GET /api/projects/:id
pub async fn get_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ProjectResponse>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    let project = state.db.find_project_by_id(&id)
        .await?
        .ok_or(AppError::NotFound)?;
    
    // Verify ownership
    if project.owner_id != claims.sub {
        return Err(AppError::Auth("Not authorized to access this project".into()));
    }
    
    Ok(Json(project.into()))
}

/// Update a project (authenticated, must be owner).
/// PUT /api/projects/:id
pub async fn update_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<UpdateProjectRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    // Verify ownership
    let project = state.db.find_project_by_id(&id)
        .await?
        .ok_or(AppError::NotFound)?;
    
    if project.owner_id != claims.sub {
        return Err(AppError::Auth("Not authorized to modify this project".into()));
    }
    
    state.db.update_project(&id, &req.name, req.description.as_deref()).await?;
    
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Delete a project (authenticated, must be owner).
/// DELETE /api/projects/:id
pub async fn delete_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    // Verify ownership
    let project = state.db.find_project_by_id(&id)
        .await?
        .ok_or(AppError::NotFound)?;
    
    if project.owner_id != claims.sub {
        return Err(AppError::Auth("Not authorized to delete this project".into()));
    }
    
    state.db.delete_project(&id).await?;
    
    Ok(Json(serde_json::json!({ "success": true, "deleted_id": id })))
}

/// Publish a project as an experience (authenticated, must be owner).
/// POST /api/projects/:id/publish
pub async fn publish_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<PublishProjectRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    // Verify ownership
    let project = state.db.find_project_by_id(&id)
        .await?
        .ok_or(AppError::NotFound)?;
    
    if project.owner_id != claims.sub {
        return Err(AppError::Auth("Not authorized to publish this project".into()));
    }
    
    state.db.publish_project(&id, &req.experience_id).await?;
    
    Ok(Json(serde_json::json!({
        "success": true,
        "project_id": id,
        "experience_id": req.experience_id,
    })))
}

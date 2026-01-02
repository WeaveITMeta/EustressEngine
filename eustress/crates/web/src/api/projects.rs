// =============================================================================
// Eustress Web - Projects API
// =============================================================================
// Table of Contents:
// 1. Types
// 2. Projects API Functions
// =============================================================================

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use super::{ApiClient, ApiError};

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// Project summary for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub is_public: bool,
}

/// Full project details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub scene_data: Option<String>, // JSON scene data
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub is_public: bool,
    pub owner_id: Uuid,
}

/// Create project request.
#[derive(Debug, Serialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub description: Option<String>,
    pub is_public: bool,
}

/// Update project request.
#[derive(Debug, Serialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub scene_data: Option<String>,
    pub is_public: Option<bool>,
}

/// Paginated response wrapper.
#[derive(Debug, Deserialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
}

// -----------------------------------------------------------------------------
// 2. Projects API Functions
// -----------------------------------------------------------------------------

/// List user's projects.
pub async fn list_projects(
    client: &ApiClient,
    page: u32,
    per_page: u32,
) -> Result<PaginatedResponse<ProjectSummary>, ApiError> {
    client
        .get(&format!("/api/projects?page={}&per_page={}", page, per_page))
        .await
}

/// Get a single project by ID.
pub async fn get_project(client: &ApiClient, id: Uuid) -> Result<Project, ApiError> {
    client.get(&format!("/api/projects/{}", id)).await
}

/// Create a new project.
pub async fn create_project(
    client: &ApiClient,
    name: &str,
    description: Option<&str>,
    is_public: bool,
) -> Result<Project, ApiError> {
    let request = CreateProjectRequest {
        name: name.to_string(),
        description: description.map(|s| s.to_string()),
        is_public,
    };
    
    client.post("/api/projects", &request).await
}

/// Update an existing project.
pub async fn update_project(
    client: &ApiClient,
    id: Uuid,
    updates: UpdateProjectRequest,
) -> Result<Project, ApiError> {
    client.put(&format!("/api/projects/{}", id), &updates).await
}

/// Delete a project.
pub async fn delete_project(client: &ApiClient, id: Uuid) -> Result<(), ApiError> {
    client.delete(&format!("/api/projects/{}", id)).await
}

/// Save project scene data.
pub async fn save_scene(
    client: &ApiClient,
    project_id: Uuid,
    scene_data: &str,
) -> Result<Project, ApiError> {
    let updates = UpdateProjectRequest {
        name: None,
        description: None,
        scene_data: Some(scene_data.to_string()),
        is_public: None,
    };
    
    client.put(&format!("/api/projects/{}", project_id), &updates).await
}

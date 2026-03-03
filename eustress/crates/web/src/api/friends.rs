// =============================================================================
// Eustress Web - Friends API
// =============================================================================
// Table of Contents:
// 1. Types
// 2. Friends API Functions
// =============================================================================

use serde::{Deserialize, Serialize};
use super::{ApiClient, ApiError};

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// Friend relationship.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Friend {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub status: FriendStatus,
    pub online: bool,
    pub current_server_id: Option<String>,
    pub current_experience_id: Option<String>,
}

/// Friend in a specific server (for join buttons and friend lists).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendInServer {
    pub user_id: String,
    pub display_name: String,
    pub server_id: String,
}

/// Friend's private server info (used in experience server browser).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendPrivateServer {
    pub server_id: String,
    pub owner_username: String,
    pub experience_id: String,
    pub player_count: u32,
    pub max_players: u32,
    pub can_join: bool,
    pub friends_in_server: Vec<FriendInServer>,
}

/// Response from join_friend_server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinResponse {
    pub server_address: String,
    pub server_port: u16,
    pub join_token: String,
}

/// Friend request/relationship status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FriendStatus {
    Pending,
    Accepted,
    Blocked,
}

/// Friend request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendRequest {
    pub id: String,
    pub from_user_id: String,
    pub from_username: String,
    pub from_avatar_url: Option<String>,
    pub created_at: String,
}

// -----------------------------------------------------------------------------
// 2. Friends API Functions
// -----------------------------------------------------------------------------

/// Get the current user's friends list.
pub async fn get_friends(client: &ApiClient) -> Result<Vec<Friend>, ApiError> {
    client.get("/api/friends").await
}

/// Get pending friend requests.
pub async fn get_friend_requests(client: &ApiClient) -> Result<Vec<FriendRequest>, ApiError> {
    client.get("/api/friends/requests").await
}

/// Send a friend request.
pub async fn send_friend_request(
    client: &ApiClient,
    user_id: &str,
) -> Result<(), ApiError> {
    #[derive(Serialize)]
    struct SendRequest<'a> {
        user_id: &'a str,
    }
    client.post::<serde_json::Value, _>("/api/friends/request", &SendRequest { user_id }).await?;
    Ok(())
}

/// Accept a friend request.
pub async fn accept_friend_request(
    client: &ApiClient,
    request_id: &str,
) -> Result<(), ApiError> {
    let endpoint = format!("/api/friends/requests/{}/accept", request_id);
    client.post::<serde_json::Value, _>(&endpoint, &()).await?;
    Ok(())
}

/// Remove a friend.
pub async fn remove_friend(
    client: &ApiClient,
    user_id: &str,
) -> Result<(), ApiError> {
    let endpoint = format!("/api/friends/{}", user_id);
    client.delete::<serde_json::Value>(&endpoint).await?;
    Ok(())
}

/// Get friends' private servers for an experience.
pub async fn get_friend_private_servers(
    api_url: &str,
    token: &str,
    experience_id: &str,
) -> Result<Vec<FriendPrivateServer>, String> {
    let url = format!("{}/api/friends/private-servers?experience_id={}", api_url, experience_id);
    let response = gloo_net::http::Request::get(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    response.json().await.map_err(|e| e.to_string())
}

/// Get friends currently in an experience.
pub async fn get_friends_in_experience(
    api_url: &str,
    token: &str,
    experience_id: &str,
) -> Result<Vec<Friend>, String> {
    let url = format!("{}/api/friends/in-experience?experience_id={}", api_url, experience_id);
    let response = gloo_net::http::Request::get(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    response.json().await.map_err(|e| e.to_string())
}

/// Join a friend's server.
pub async fn join_friend_server(
    api_url: &str,
    token: &str,
    server_id: &str,
) -> Result<JoinResponse, String> {
    let url = format!("{}/api/friends/join-server/{}", api_url, server_id);
    let response = gloo_net::http::Request::post(&url)
        .header("Authorization", &format!("Bearer {}", token))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    response.json().await.map_err(|e| e.to_string())
}

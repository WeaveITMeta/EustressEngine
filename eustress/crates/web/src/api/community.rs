// =============================================================================
// Eustress Web - Community API
// =============================================================================
// Table of Contents:
// 1. Types
// 2. Response Types
// 3. Community API Functions
// =============================================================================

use serde::{Deserialize, Serialize};
use super::{ApiClient, ApiError};

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// Public user profile for community display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicUser {
    pub id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub joined_at: String,
    pub experience_count: u32,
    pub is_verified: bool,
    pub follower_count: u64,
}

/// Leaderboard user summary (nested in LeaderboardEntry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardUser {
    pub username: String,
    pub avatar_url: Option<String>,
}

/// Leaderboard entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub user: LeaderboardUser,
    pub score: u64,
    pub score_label: String,
    pub category: String,
}

/// Community statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunityStats {
    pub total_users: u64,
    pub active_users: u64,
    pub total_experiences: u64,
    pub total_visits: u64,
    pub total_plays: u64,
}

// -----------------------------------------------------------------------------
// 2. Response Types
// -----------------------------------------------------------------------------

/// Response from search_users endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchUsersResponse {
    pub users: Vec<PublicUser>,
}

/// Response from get_leaderboard endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct LeaderboardResponse {
    pub entries: Vec<LeaderboardEntry>,
}

// -----------------------------------------------------------------------------
// 3. Community API Functions
// -----------------------------------------------------------------------------

/// Search for users in the community.
pub async fn search_users(
    client: &ApiClient,
    query: &str,
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<SearchUsersResponse, ApiError> {
    let endpoint = format!(
        "/api/community/search?q={}&page={}&per_page={}",
        query,
        page.unwrap_or(1),
        per_page.unwrap_or(20)
    );
    client.get(&endpoint).await
}

/// Get the community leaderboard.
pub async fn get_leaderboard(
    client: &ApiClient,
    category: &str,
    sort: Option<&str>,
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<LeaderboardResponse, ApiError> {
    let mut endpoint = format!("/api/community/leaderboard?category={}", category);
    if let Some(s) = sort {
        endpoint.push_str(&format!("&sort={}", s));
    }
    if let Some(p) = page {
        endpoint.push_str(&format!("&page={}", p));
    }
    if let Some(pp) = per_page {
        endpoint.push_str(&format!("&per_page={}", pp));
    }
    client.get(&endpoint).await
}

/// Get community statistics.
pub async fn get_community_stats(
    client: &ApiClient,
) -> Result<CommunityStats, ApiError> {
    client.get("/api/community/stats").await
}

/// Get a user's public profile.
pub async fn get_user_profile(
    client: &ApiClient,
    user_id: &str,
) -> Result<PublicUser, ApiError> {
    let endpoint = format!("/api/community/users/{}", user_id);
    client.get(&endpoint).await
}

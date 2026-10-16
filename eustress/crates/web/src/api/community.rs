// =============================================================================
// Eustress Web - Community API
// =============================================================================
// Table of Contents:
// 1. Types
// 2. Response Types
// 3. Community API Functions
// =============================================================================

use serde::{Deserialize, Serialize};
use super::{iso_to_local_date, ApiClient, ApiError};

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// A person in the community, as `GET /api/community/search` answers: `username`, `display_name`, `avatar_url`, `is_verified`,
/// `follower_count` (the real count) and `joined_at` (the date the account was made). Everything but the username may be left
/// out of an answer, and what is left out stays absent (`None`), so a page hides it instead of drawing a 0 or an empty date.
/// The search sends no account id: a person is followed by username.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicUser {
    pub username: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    /// The Worker's timestamp for when the account was made (`2026-03-01T00:00:00.000Z`).
    #[serde(default)]
    pub joined_at: Option<String>,
    #[serde(default)]
    pub experience_count: Option<u32>,
    #[serde(default)]
    pub is_verified: bool,
    #[serde(default)]
    pub follower_count: Option<u64>,
}

impl PublicUser {
    /// "Joined March 1, 2026", the date on the reader's calendar. `None` when there is no date or it is not a time.
    pub fn joined_words(&self) -> Option<String> {
        let at = self.joined_at.as_deref()?.trim();
        chrono::DateTime::parse_from_rfc3339(at).ok()?;
        Some(format!("Joined {}", iso_to_local_date(at)))
    }

    /// "3 followers" ("1 follower"), from the count the Worker sent. `None` when it sent none.
    pub fn followers_words(&self) -> Option<String> {
        let n = self.follower_count?;
        Some(if n == 1 { "1 follower".to_string() } else { format!("{} followers", crate::utils::format_count(n)) })
    }

    /// What a search result says beside the name, in order, leaving out what the answer did not carry.
    pub fn facts(&self) -> Vec<String> {
        let mut facts = Vec::new();
        if self.is_verified {
            facts.push("Verified".to_string());
        }
        facts.extend(self.joined_words());
        facts.extend(self.followers_words());
        facts
    }
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

/// The route for a people search. The Worker reads `q` and `limit` (at most 20) and has no pages.
pub fn search_path(query: &str, limit: Option<u32>) -> String {
    format!("/api/community/search?q={}&limit={}", urlencoding::encode(query.trim()), limit.unwrap_or(20).clamp(1, 20))
}

/// Search for users in the community.
pub async fn search_users(
    client: &ApiClient,
    query: &str,
    limit: Option<u32>,
) -> Result<SearchUsersResponse, ApiError> {
    client.get(&search_path(query, limit)).await
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

// -----------------------------------------------------------------------------
// 4. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The Worker's own answer to `GET /api/community/search?q=ada`, copied from what tests/community_search.test.mjs gets back.
    const WORKER_ANSWER: &str = r#"{"users":[{"username":"ada_builds","display_name":"ada_builds","avatar_url":null,"is_verified":true,"follower_count":3,"created_at":"2026-03-01T00:00:00.000Z","joined_at":"2026-03-01T00:00:00.000Z"},{"username":"adam","display_name":"adam","avatar_url":null,"is_verified":false,"follower_count":0,"created_at":"2026-04-02T00:00:00.000Z","joined_at":"2026-04-02T00:00:00.000Z"}],"query":"ada"}"#;

    /// What the Worker answered before it sent `joined_at` and a real count: no date, a 0 that was never counted.
    const OLD_WORKER_ANSWER: &str = r#"{"users":[{"username":"ada_builds","display_name":"ada_builds","avatar_url":null,"is_verified":true,"follower_count":0,"created_at":"2026-03-01T00:00:00.000Z"}],"query":"ada"}"#;

    #[test]
    fn the_workers_search_answer_parses_into_people() {
        let answer: SearchUsersResponse = serde_json::from_str(WORKER_ANSWER).expect("the Worker's answer must parse");
        assert_eq!(answer.users.len(), 2);
        let ada = &answer.users[0];
        assert_eq!(ada.username, "ada_builds");
        assert_eq!(ada.display_name.as_deref(), Some("ada_builds"));
        assert_eq!(ada.avatar_url, None);
        assert!(ada.is_verified);
        assert_eq!(ada.follower_count, Some(3));
        assert_eq!(ada.joined_at.as_deref(), Some("2026-03-01T00:00:00.000Z"));
        assert_eq!(ada.bio, None);
        assert_eq!(ada.experience_count, None);
        assert_eq!(answer.users[1].follower_count, Some(0), "a real zero is kept");
    }

    #[test]
    fn a_result_says_what_the_answer_carried() {
        let answer: SearchUsersResponse = serde_json::from_str(WORKER_ANSWER).unwrap();
        assert_eq!(answer.users[0].facts(), vec!["Verified", "Joined March 1, 2026", "3 followers"]);
        assert_eq!(answer.users[1].facts(), vec!["Joined April 2, 2026", "0 followers"], "a person who is not verified is not labelled");
    }

    #[test]
    fn what_the_answer_leaves_out_is_not_drawn() {
        let bare: SearchUsersResponse = serde_json::from_str(r#"{"users":[{"username":"x"}]}"#).expect("only the username is required");
        let x = &bare.users[0];
        assert_eq!((x.display_name.clone(), x.avatar_url.clone(), x.bio.clone(), x.joined_at.clone(), x.experience_count, x.follower_count), (None, None, None, None, None, None));
        assert!(!x.is_verified);
        assert!(x.facts().is_empty(), "no made-up 0 followers, no empty date: {:?}", x.facts());
        assert_eq!(x.joined_words(), None);
        assert_eq!(x.followers_words(), None);
    }

    #[test]
    fn an_older_answer_with_no_date_still_parses_and_shows_no_date() {
        let old: SearchUsersResponse = serde_json::from_str(OLD_WORKER_ANSWER).expect("an answer with no joined_at must parse");
        assert_eq!(old.users[0].joined_at, None);
        assert!(old.users[0].facts().iter().all(|f| !f.starts_with("Joined")));
    }

    #[test]
    fn an_unreadable_date_is_hidden_not_shown() {
        for bad in ["", "   ", "garbage", "2026-13-01T00:00:00Z", "March 2026"] {
            let u = PublicUser { username: "x".into(), display_name: None, avatar_url: None, bio: None, joined_at: Some(bad.to_string()), experience_count: None, is_verified: false, follower_count: None };
            assert_eq!(u.joined_words(), None, "{bad:?}");
        }
    }

    #[test]
    fn one_follower_is_singular_and_a_crowd_is_grouped() {
        let mut u = PublicUser { username: "x".into(), display_name: None, avatar_url: None, bio: None, joined_at: None, experience_count: None, is_verified: false, follower_count: Some(1) };
        assert_eq!(u.followers_words().as_deref(), Some("1 follower"));
        u.follower_count = Some(12_500);
        assert_eq!(u.followers_words().as_deref(), Some("12,500 followers"));
    }

    #[test]
    fn a_suggestion_from_an_older_worker_is_ignored() {
        let with: SearchUsersResponse = serde_json::from_str(r#"{"users":[],"query":"q","ai_suggestion":"Try a username."}"#).expect("an unknown field must not break the answer");
        assert!(with.users.is_empty());
    }

    #[test]
    fn the_search_route_is_encoded_and_asks_the_worker_for_a_limit_it_reads() {
        assert_eq!(search_path("ada", Some(20)), "/api/community/search?q=ada&limit=20");
        assert_eq!(search_path("  a&b c ", None), "/api/community/search?q=a%26b%20c&limit=20");
        assert_eq!(search_path("ada", Some(500)), "/api/community/search?q=ada&limit=20", "the Worker answers at most 20");
        assert_eq!(search_path("ada", Some(0)), "/api/community/search?q=ada&limit=1");
    }
}

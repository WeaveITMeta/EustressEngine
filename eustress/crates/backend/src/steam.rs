// =============================================================================
// Eustress Backend - Steam OpenID Authentication
// =============================================================================
// Steam uses OpenID 2.0 for authentication. Flow:
// 1. User clicks "Login with Steam" -> redirect to Steam login page
// 2. User authenticates on Steam
// 3. Steam redirects back to our callback URL with identity assertion
// 4. We verify the assertion with Steam
// 5. We fetch user profile from Steam Web API
// 6. We create/update user and return JWT token
// =============================================================================

use axum::{
    extract::{Query, State},
    response::Redirect,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

use crate::auth::generate_token;
use crate::error::AppError;
use crate::AppState;

// -----------------------------------------------------------------------------
// Steam OpenID Constants
// -----------------------------------------------------------------------------

const STEAM_OPENID_URL: &str = "https://steamcommunity.com/openid/login";
const STEAM_API_URL: &str = "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v2/";

// -----------------------------------------------------------------------------
// Steam API Response Types
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SteamPlayerResponse {
    response: SteamPlayersWrapper,
}

#[derive(Debug, Deserialize)]
struct SteamPlayersWrapper {
    players: Vec<SteamPlayer>,
}

#[derive(Debug, Deserialize)]
struct SteamPlayer {
    steamid: String,
    personaname: String,
    avatarfull: Option<String>,
    profileurl: Option<String>,
}

// -----------------------------------------------------------------------------
// OpenID Callback Query Parameters
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SteamCallbackQuery {
    #[serde(rename = "openid.ns")]
    ns: Option<String>,
    #[serde(rename = "openid.mode")]
    mode: Option<String>,
    #[serde(rename = "openid.op_endpoint")]
    op_endpoint: Option<String>,
    #[serde(rename = "openid.claimed_id")]
    claimed_id: Option<String>,
    #[serde(rename = "openid.identity")]
    identity: Option<String>,
    #[serde(rename = "openid.return_to")]
    return_to: Option<String>,
    #[serde(rename = "openid.response_nonce")]
    response_nonce: Option<String>,
    #[serde(rename = "openid.assoc_handle")]
    assoc_handle: Option<String>,
    #[serde(rename = "openid.signed")]
    signed: Option<String>,
    #[serde(rename = "openid.sig")]
    sig: Option<String>,
    /// Studio callback port (for desktop app SSO)
    studio_port: Option<u16>,
}

// -----------------------------------------------------------------------------
// Handlers
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SteamLoginQuery {
    /// Studio callback port (for desktop app SSO)
    pub studio_port: Option<u16>,
}

/// Redirect user to Steam login page.
pub async fn steam_login_redirect(
    State(state): State<AppState>,
    Query(query): Query<SteamLoginQuery>,
) -> Redirect {
    // Build return URL, optionally including studio_port
    let return_url = if let Some(port) = query.studio_port {
        format!("{}&studio_port={}", state.config.steam_return_url, port)
    } else {
        state.config.steam_return_url.clone()
    };
    
    let mut params = HashMap::new();
    params.insert("openid.ns", "http://specs.openid.net/auth/2.0");
    params.insert("openid.mode", "checkid_setup");
    params.insert("openid.return_to", &return_url);
    params.insert("openid.realm", &state.config.steam_realm);
    params.insert("openid.identity", "http://specs.openid.net/auth/2.0/identifier_select");
    params.insert("openid.claimed_id", "http://specs.openid.net/auth/2.0/identifier_select");

    let url = Url::parse_with_params(STEAM_OPENID_URL, &params)
        .expect("Failed to build Steam OpenID URL");

    Redirect::temporary(url.as_str())
}

/// Redirect user to Steam for account linking (requires existing auth).
/// Note: In production, this should verify the user's token first.
/// For now, it uses the same flow as login - the callback will link if user exists.
pub async fn steam_link_redirect(State(state): State<AppState>) -> Redirect {
    // Use a different return URL for linking vs login
    let link_return_url = format!("{}/api/auth/steam/callback?mode=link", state.config.steam_realm);
    
    let mut params = HashMap::new();
    params.insert("openid.ns", "http://specs.openid.net/auth/2.0");
    params.insert("openid.mode", "checkid_setup");
    params.insert("openid.return_to", &link_return_url);
    params.insert("openid.realm", &state.config.steam_realm);
    params.insert("openid.identity", "http://specs.openid.net/auth/2.0/identifier_select");
    params.insert("openid.claimed_id", "http://specs.openid.net/auth/2.0/identifier_select");

    let url = Url::parse_with_params(STEAM_OPENID_URL, &params)
        .expect("Failed to build Steam OpenID URL");

    Redirect::temporary(url.as_str())
}

/// Handle Steam OpenID callback.
pub async fn steam_callback(
    State(state): State<AppState>,
    Query(query): Query<SteamCallbackQuery>,
) -> Result<Redirect, AppError> {
    // Verify the OpenID response with Steam
    let steam_id = verify_steam_response(&state, &query).await?;
    
    // Fetch Steam user profile
    let steam_profile = fetch_steam_profile(&state.config.steam_api_key, &steam_id).await?;
    
    // Find or create user
    let user = match state.db.find_user_by_steam_id(&steam_id).await? {
        Some(existing_user) => {
            // Update avatar if changed
            if let Some(ref avatar) = steam_profile.avatarfull {
                let _ = state.db.update_user_steam(&existing_user.id, &steam_id, Some(avatar)).await;
            }
            existing_user
        }
        None => {
            // Create new user from Steam profile
            let user_id = uuid::Uuid::new_v4().to_string();
            state
                .db
                .create_user(
                    &user_id,
                    &steam_profile.personaname,
                    None,
                    None,
                    Some(&steam_id),
                    steam_profile.avatarfull.as_deref(),
                )
                .await?
        }
    };
    
    // Generate JWT token
    let token = generate_token(&user.id, &state.config.jwt_secret, state.config.jwt_expiry_hours)?;
    
    // Check if this is a Studio SSO callback
    if let Some(studio_port) = query.studio_port {
        // Redirect to local Studio callback
        let redirect_url = format!(
            "http://127.0.0.1:{}/callback?token={}",
            studio_port,
            urlencoding::encode(&token)
        );
        return Ok(Redirect::temporary(&redirect_url));
    }
    
    // Redirect to frontend with token
    let redirect_url = format!(
        "{}?token={}&user_id={}",
        state.config.frontend_url,
        token,
        user.id
    );
    
    Ok(Redirect::temporary(&redirect_url))
}

// -----------------------------------------------------------------------------
// Steam API Functions
// -----------------------------------------------------------------------------

/// Verify the OpenID response with Steam.
async fn verify_steam_response(
    state: &AppState,
    query: &SteamCallbackQuery,
) -> Result<String, AppError> {
    // Check mode
    if query.mode.as_deref() != Some("id_res") {
        return Err(AppError::Steam("Invalid OpenID mode".into()));
    }
    
    // Extract Steam ID from claimed_id
    // Format: https://steamcommunity.com/openid/id/76561198012345678
    let claimed_id = query
        .claimed_id
        .as_ref()
        .ok_or_else(|| AppError::Steam("Missing claimed_id".into()))?;
    
    let steam_id = claimed_id
        .split('/')
        .last()
        .ok_or_else(|| AppError::Steam("Invalid claimed_id format".into()))?
        .to_string();
    
    // Verify with Steam by sending the same params back with mode=check_authentication
    let client = reqwest::Client::new();
    
    let mut verify_params = HashMap::new();
    verify_params.insert("openid.ns", query.ns.as_deref().unwrap_or(""));
    verify_params.insert("openid.mode", "check_authentication");
    verify_params.insert("openid.op_endpoint", query.op_endpoint.as_deref().unwrap_or(""));
    verify_params.insert("openid.claimed_id", query.claimed_id.as_deref().unwrap_or(""));
    verify_params.insert("openid.identity", query.identity.as_deref().unwrap_or(""));
    verify_params.insert("openid.return_to", query.return_to.as_deref().unwrap_or(""));
    verify_params.insert("openid.response_nonce", query.response_nonce.as_deref().unwrap_or(""));
    verify_params.insert("openid.assoc_handle", query.assoc_handle.as_deref().unwrap_or(""));
    verify_params.insert("openid.signed", query.signed.as_deref().unwrap_or(""));
    verify_params.insert("openid.sig", query.sig.as_deref().unwrap_or(""));
    
    let response = client
        .post(STEAM_OPENID_URL)
        .form(&verify_params)
        .send()
        .await
        .map_err(|e| AppError::Steam(format!("Failed to verify with Steam: {}", e)))?;
    
    let body = response
        .text()
        .await
        .map_err(|e| AppError::Steam(format!("Failed to read Steam response: {}", e)))?;
    
    // Check if Steam validated the response
    if !body.contains("is_valid:true") {
        tracing::warn!("Steam OpenID verification failed: {}", body);
        return Err(AppError::Steam("Steam verification failed".into()));
    }
    
    tracing::info!("Steam login verified for Steam ID: {}", steam_id);
    Ok(steam_id)
}

/// Fetch Steam user profile from Web API.
async fn fetch_steam_profile(api_key: &str, steam_id: &str) -> Result<SteamPlayer, AppError> {
    if api_key.is_empty() {
        // Return minimal profile if no API key
        return Ok(SteamPlayer {
            steamid: steam_id.to_string(),
            personaname: format!("Steam_{}", &steam_id[steam_id.len().saturating_sub(8)..]),
            avatarfull: None,
            profileurl: None,
        });
    }
    
    let url = format!("{}?key={}&steamids={}", STEAM_API_URL, api_key, steam_id);
    
    let client = reqwest::Client::new();
    let response: SteamPlayerResponse = client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::Steam(format!("Failed to fetch Steam profile: {}", e)))?
        .json()
        .await
        .map_err(|e| AppError::Steam(format!("Failed to parse Steam profile: {}", e)))?;
    
    response
        .response
        .players
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Steam("Steam user not found".into()))
}

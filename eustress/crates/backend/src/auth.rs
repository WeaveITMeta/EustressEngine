// =============================================================================
// Eustress Backend - Authentication Handlers
// =============================================================================

use axum::{
    async_trait,
    extract::{FromRequestParts, Query, State},
    http::{header, request::Parts, HeaderMap, StatusCode},
    response::{Html, Redirect},
    Json,
};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::db::UserResponse;
use crate::error::AppError;
use crate::AppState;

// -----------------------------------------------------------------------------
// JWT Claims
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,  // User ID
    pub exp: i64,     // Expiry timestamp
    pub iat: i64,     // Issued at
}

// -----------------------------------------------------------------------------
// Auth Extractor
// -----------------------------------------------------------------------------

/// Authenticated user extracted from JWT token.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Get Authorization header
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or((StatusCode::UNAUTHORIZED, "Missing authorization header"))?;
        
        // Extract Bearer token
        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or((StatusCode::UNAUTHORIZED, "Invalid authorization format"))?;
        
        // Get JWT secret from environment (fallback for extractor)
        let secret = std::env::var("JWT_SECRET")
            .unwrap_or_else(|_| "default-dev-secret".to_string());
        
        // Validate token
        let claims = decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;
        
        Ok(AuthUser {
            user_id: claims.claims.sub,
        })
    }
}

// -----------------------------------------------------------------------------
// Request/Response Types
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserResponse,
}

#[derive(Debug, Serialize)]
pub struct RefreshResponse {
    pub token: String,
}

// -----------------------------------------------------------------------------
// Helper Functions
// -----------------------------------------------------------------------------

/// Hash a password using Argon2.
fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| AppError::Internal)
}

/// Verify a password against a hash.
fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed_hash = PasswordHash::new(hash).map_err(|_| AppError::Internal)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

/// Generate a JWT token for a user.
pub fn generate_token(user_id: &str, secret: &str, expiry_hours: i64) -> Result<String, AppError> {
    let now = Utc::now();
    let exp = now + Duration::hours(expiry_hours);
    
    let claims = Claims {
        sub: user_id.to_string(),
        exp: exp.timestamp(),
        iat: now.timestamp(),
    };
    
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| AppError::Internal)
}

/// Validate a JWT token and extract claims.
pub fn validate_token(token: &str, secret: &str) -> Result<Claims, AppError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|e| match e.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => AppError::TokenExpired,
        _ => AppError::InvalidToken,
    })
}

/// Extract token from Authorization header.
pub fn extract_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

// -----------------------------------------------------------------------------
// Handlers
// -----------------------------------------------------------------------------

/// Register a new user.
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    // Check if user exists
    if state.db.find_user_by_email(&req.email).await?.is_some() {
        return Err(AppError::UserExists);
    }
    
    // Hash password
    let password_hash = hash_password(&req.password)?;
    
    // Create user
    let user_id = uuid::Uuid::new_v4().to_string();
    let user = state
        .db
        .create_user(
            &user_id,
            &req.username,
            Some(&req.email),
            Some(&password_hash),
            None,
            None,
        )
        .await?;
    
    // Generate token
    let token = generate_token(&user.id, &state.config.jwt_secret, state.config.jwt_expiry_hours)?;
    
    Ok(Json(AuthResponse {
        token,
        user: user.into(),
    }))
}

/// Login with email and password.
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    // Find user
    let user = state
        .db
        .find_user_by_email(&req.email)
        .await?
        .ok_or(AppError::InvalidCredentials)?;
    
    // Verify password
    let password_hash = user.password_hash.as_ref().ok_or(AppError::InvalidCredentials)?;
    if !verify_password(&req.password, password_hash)? {
        return Err(AppError::InvalidCredentials);
    }
    
    // Generate token
    let token = generate_token(&user.id, &state.config.jwt_secret, state.config.jwt_expiry_hours)?;
    
    Ok(Json(AuthResponse {
        token,
        user: user.into(),
    }))
}

/// Get current user from token.
pub async fn get_current_user(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<UserResponse>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    let user = state
        .db
        .find_user_by_id(&claims.sub)
        .await?
        .ok_or(AppError::UserNotFound)?;
    
    Ok(Json(user.into()))
}

/// Refresh auth token.
pub async fn refresh_token(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RefreshResponse>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    // Generate new token
    let new_token = generate_token(&claims.sub, &state.config.jwt_secret, state.config.jwt_expiry_hours)?;
    
    Ok(Json(RefreshResponse { token: new_token }))
}

// -----------------------------------------------------------------------------
// Account Linking
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AddEmailRequest {
    pub email: String,
    pub password: String,
}

/// Add email and password to an existing account (e.g., Steam-only user).
pub async fn add_email_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AddEmailRequest>,
) -> Result<Json<UserResponse>, AppError> {
    // Get current user from token
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    let user = state
        .db
        .find_user_by_id(&claims.sub)
        .await?
        .ok_or(AppError::UserNotFound)?;
    
    // Check if email is already taken by another user
    if let Some(existing) = state.db.find_user_by_email(&req.email).await? {
        if existing.id != user.id {
            return Err(AppError::UserExists);
        }
    }
    
    // Hash the password
    let password_hash = hash_password(&req.password)?;
    
    // Update user with email and password
    state.db.update_user_email_password(&user.id, &req.email, &password_hash).await?;
    
    // Fetch updated user
    let updated_user = state
        .db
        .find_user_by_id(&user.id)
        .await?
        .ok_or(AppError::UserNotFound)?;
    
    Ok(Json(updated_user.into()))
}

// -----------------------------------------------------------------------------
// Studio SSO
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct StudioLoginQuery {
    pub port: u16,
}

/// Redirect to Studio SSO login page
/// The Studio app opens this URL with a callback port
pub async fn studio_login_page(
    State(state): State<AppState>,
    Query(query): Query<StudioLoginQuery>,
) -> Html<String> {
    let callback_port = query.port;
    
    // Generate a state token to prevent CSRF
    let state_token = uuid::Uuid::new_v4().to_string();
    
    Html(format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Login to Eustress Engine</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
            min-height: 100vh;
            display: flex;
            justify-content: center;
            align-items: center;
            color: #fff;
        }}
        .container {{
            background: rgba(255,255,255,0.05);
            border-radius: 16px;
            padding: 40px;
            width: 100%;
            max-width: 400px;
            backdrop-filter: blur(10px);
            border: 1px solid rgba(255,255,255,0.1);
        }}
        h1 {{
            text-align: center;
            margin-bottom: 8px;
            font-size: 24px;
        }}
        .subtitle {{
            text-align: center;
            color: #8892b0;
            margin-bottom: 32px;
        }}
        .divider {{
            display: flex;
            align-items: center;
            margin: 24px 0;
            color: #8892b0;
        }}
        .divider::before, .divider::after {{
            content: '';
            flex: 1;
            height: 1px;
            background: rgba(255,255,255,0.1);
        }}
        .divider span {{ padding: 0 16px; }}
        .form-group {{
            margin-bottom: 16px;
        }}
        label {{
            display: block;
            margin-bottom: 8px;
            color: #ccd6f6;
            font-size: 14px;
        }}
        input {{
            width: 100%;
            padding: 12px 16px;
            border: 1px solid rgba(255,255,255,0.1);
            border-radius: 8px;
            background: rgba(0,0,0,0.2);
            color: #fff;
            font-size: 16px;
            transition: border-color 0.2s;
        }}
        input:focus {{
            outline: none;
            border-color: #64ffda;
        }}
        .btn {{
            width: 100%;
            padding: 14px;
            border: none;
            border-radius: 8px;
            font-size: 16px;
            font-weight: 600;
            cursor: pointer;
            transition: transform 0.2s, box-shadow 0.2s;
            display: flex;
            align-items: center;
            justify-content: center;
            gap: 10px;
        }}
        .btn:hover {{
            transform: translateY(-2px);
            box-shadow: 0 4px 12px rgba(0,0,0,0.3);
        }}
        .btn-primary {{
            background: linear-gradient(135deg, #64ffda 0%, #00bfa5 100%);
            color: #1a1a2e;
        }}
        .btn-steam {{
            background: #171a21;
            color: #fff;
            margin-bottom: 12px;
        }}
        .btn-discord {{
            background: #5865F2;
            color: #fff;
        }}
        .error {{
            background: rgba(255,107,107,0.1);
            border: 1px solid #ff6b6b;
            color: #ff6b6b;
            padding: 12px;
            border-radius: 8px;
            margin-bottom: 16px;
            display: none;
        }}
        .logo {{
            text-align: center;
            margin-bottom: 24px;
            font-size: 48px;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="logo">🎮</div>
        <h1>Eustress Engine</h1>
        <p class="subtitle">Sign in to publish your experiences</p>
        
        <div id="error" class="error"></div>
        
        <!-- SSO Buttons -->
        <a href="/api/auth/steam?studio_port={callback_port}" class="btn btn-steam">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                <path d="M12 2C6.48 2 2 6.48 2 12c0 4.84 3.44 8.87 8 9.8V15H8v-3h2V9.5C10 7.57 11.57 6 13.5 6H16v3h-2c-.55 0-1 .45-1 1v2h3v3h-3v6.95c5.05-.5 9-4.76 9-9.95 0-5.52-4.48-10-10-10z"/>
            </svg>
            Continue with Steam
        </a>
        
        <a href="/api/auth/discord?studio_port={callback_port}" class="btn btn-discord">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                <path d="M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057 19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028 14.09 14.09 0 0 0 1.226-1.994.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128 10.2 10.2 0 0 0 .372-.292.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03z"/>
            </svg>
            Continue with Discord
        </a>
        
        <div class="divider"><span>or</span></div>
        
        <!-- Email/Password Form -->
        <form id="loginForm" onsubmit="handleLogin(event)">
            <div class="form-group">
                <label for="email">Email</label>
                <input type="email" id="email" name="email" required placeholder="you@example.com">
            </div>
            <div class="form-group">
                <label for="password">Password</label>
                <input type="password" id="password" name="password" required placeholder="••••••••">
            </div>
            <button type="submit" class="btn btn-primary">Sign In</button>
        </form>
    </div>
    
    <script>
        const callbackPort = {callback_port};
        
        async function handleLogin(e) {{
            e.preventDefault();
            const errorEl = document.getElementById('error');
            errorEl.style.display = 'none';
            
            const email = document.getElementById('email').value;
            const password = document.getElementById('password').value;
            
            try {{
                const response = await fetch('/api/auth/login', {{
                    method: 'POST',
                    headers: {{ 'Content-Type': 'application/json' }},
                    body: JSON.stringify({{ email, password }})
                }});
                
                if (!response.ok) {{
                    const data = await response.json();
                    throw new Error(data.error || 'Login failed');
                }}
                
                const data = await response.json();
                
                // Redirect to Studio callback
                window.location.href = `http://127.0.0.1:${{callbackPort}}/callback?token=${{encodeURIComponent(data.token)}}`;
            }} catch (err) {{
                errorEl.textContent = err.message;
                errorEl.style.display = 'block';
            }}
        }}
    </script>
</body>
</html>"#))
}

/// Handle Studio SSO callback from Steam
pub async fn studio_steam_callback(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Result<Redirect, AppError> {
    // Get the studio port from the state parameter
    let studio_port = params.get("studio_port")
        .and_then(|p| p.parse::<u16>().ok())
        .ok_or(AppError::Auth("Missing studio port".to_string()))?;
    
    // The actual Steam callback handling is done by steam_callback
    // This just needs to redirect with the token
    // For now, redirect to error - the actual flow goes through steam_callback
    Ok(Redirect::temporary(&format!(
        "http://127.0.0.1:{}/callback?error={}",
        studio_port,
        urlencoding::encode("Steam callback not implemented for Studio yet")
    )))
}

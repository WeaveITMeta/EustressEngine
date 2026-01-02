// =============================================================================
// Eustress Web - Global Application State
// =============================================================================
// Table of Contents:
// 1. Imports
// 2. User State
// 3. App State
// 4. Auth Actions
// =============================================================================

use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use gloo_storage::Storage;
use uuid::Uuid;

// -----------------------------------------------------------------------------
// 2. User State
// -----------------------------------------------------------------------------

/// Represents an authenticated user.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub avatar_url: Option<String>,
    pub discord_id: Option<String>,
    pub bliss_balance: u64,  // Bliss currency balance
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Authentication state.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AuthState {
    #[default]
    Unknown,
    Authenticated(User),
    Unauthenticated,
}

impl AuthState {
    /// Check if user is authenticated.
    pub fn is_authenticated(&self) -> bool {
        matches!(self, AuthState::Authenticated(_))
    }
    
    /// Get the authenticated user, if any.
    pub fn user(&self) -> Option<&User> {
        match self {
            AuthState::Authenticated(user) => Some(user),
            _ => None,
        }
    }
}

// -----------------------------------------------------------------------------
// 3. App State
// -----------------------------------------------------------------------------

/// Global application state provided via Leptos context.
#[derive(Clone)]
pub struct AppState {
    /// Current authentication state.
    pub auth: RwSignal<AuthState>,
    
    /// API base URL.
    pub api_url: String,
    
    /// Whether the app is in dark mode.
    pub dark_mode: RwSignal<bool>,
    
    /// Global loading state.
    pub loading: RwSignal<bool>,
    
    /// Global error message.
    pub error: RwSignal<Option<String>>,
}

impl AppState {
    /// Create a new app state instance.
    pub fn new() -> Self {
        // Determine API URL based on ENVIRONMENT variable
        let environment = option_env!("ENVIRONMENT").unwrap_or("production");
        let api_url = if environment == "development" {
            "http://localhost:7000".to_string()
        } else {
            // Production or staging use the production API
            "https://api.eustress.dev".to_string()
        };
        
        // Check localStorage for saved preferences
        let dark_mode: bool = gloo_storage::LocalStorage::get("dark_mode")
            .unwrap_or(true); // Default to dark mode
        
        Self {
            auth: RwSignal::new(AuthState::Unknown),
            api_url,
            dark_mode: RwSignal::new(dark_mode),
            loading: RwSignal::new(false),
            error: RwSignal::new(None),
        }
    }
    
    /// Toggle dark mode and persist preference.
    pub fn toggle_dark_mode(&self) {
        let new_value = !self.dark_mode.get();
        self.dark_mode.set(new_value);
        let _ = gloo_storage::LocalStorage::set("dark_mode", new_value);
    }
    
    /// Set a global error message.
    pub fn set_error(&self, message: impl Into<String>) {
        self.error.set(Some(message.into()));
    }
    
    /// Clear the global error message.
    pub fn clear_error(&self) {
        self.error.set(None);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// -----------------------------------------------------------------------------
// 4. Auth Actions
// -----------------------------------------------------------------------------

impl AppState {
    /// Log in a user and save to localStorage.
    pub fn login(&self, user: User) {
        // Save user to localStorage for persistence
        let _ = gloo_storage::LocalStorage::set("auth_user", &user);
        self.auth.set(AuthState::Authenticated(user));
    }
    
    /// Log in with token (saves both token and user).
    pub fn login_with_token(&self, token: String, user: User) {
        let _ = gloo_storage::LocalStorage::set("auth_token", &token);
        let _ = gloo_storage::LocalStorage::set("auth_user", &user);
        self.auth.set(AuthState::Authenticated(user));
    }
    
    /// Try to restore session from localStorage.
    pub fn restore_session(&self) {
        if let Ok(user) = gloo_storage::LocalStorage::get::<User>("auth_user") {
            self.auth.set(AuthState::Authenticated(user));
        } else {
            self.auth.set(AuthState::Unauthenticated);
        }
    }
    
    /// Get stored auth token.
    pub fn get_token(&self) -> Option<String> {
        gloo_storage::LocalStorage::get("auth_token").ok()
    }
    
    /// Log out the current user.
    pub fn logout(&self) {
        self.auth.set(AuthState::Unauthenticated);
        // Clear stored token and user
        let _ = gloo_storage::LocalStorage::delete("auth_token");
        let _ = gloo_storage::LocalStorage::delete("auth_user");
    }
}

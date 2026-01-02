// =============================================================================
// Eustress Web - Desktop Notifications Service
// =============================================================================
// Handles browser push notifications for favorite updates
// =============================================================================

use gloo_storage::Storage;
use wasm_bindgen::prelude::*;
use web_sys::{Notification, NotificationOptions, NotificationPermission};

/// Check if notifications are supported.
pub fn is_supported() -> bool {
    js_sys::Reflect::has(
        &web_sys::window().unwrap(),
        &JsValue::from_str("Notification"),
    )
    .unwrap_or(false)
}

/// Get current notification permission status.
pub fn get_permission() -> NotificationPermission {
    Notification::permission()
}

/// Request notification permission from user.
pub async fn request_permission() -> Result<NotificationPermission, JsValue> {
    let promise = Notification::request_permission()?;
    let result = wasm_bindgen_futures::JsFuture::from(promise).await?;
    
    // Convert result to permission enum
    let permission_str = result.as_string().unwrap_or_default();
    match permission_str.as_str() {
        "granted" => Ok(NotificationPermission::Granted),
        "denied" => Ok(NotificationPermission::Denied),
        _ => Ok(NotificationPermission::Default),
    }
}

/// Show a desktop notification for a favorite update.
pub fn show_favorite_update(
    experience_name: &str,
    update_type: &str,
    icon_url: Option<&str>,
) -> Result<Notification, JsValue> {
    let mut options = NotificationOptions::new();
    
    let body = match update_type {
        "new_version" => format!("{} has been updated! Check out the new features.", experience_name),
        "new_content" => format!("{} has new content available.", experience_name),
        "event" => format!("{} has a special event happening now!", experience_name),
        _ => format!("{} has been updated.", experience_name),
    };
    
    options.set_body(&body);
    options.set_tag(&format!("favorite-update-{}", experience_name.to_lowercase().replace(' ', "-")));
    options.set_require_interaction(false);
    
    if let Some(icon) = icon_url {
        options.set_icon(icon);
    } else {
        options.set_icon("/assets/icons/eustress-gear.svg");
    }
    
    Notification::new_with_options(&format!("🎮 {} Updated", experience_name), &options)
}

/// Show a generic notification.
pub fn show_notification(
    title: &str,
    body: &str,
    icon_url: Option<&str>,
) -> Result<Notification, JsValue> {
    let mut options = NotificationOptions::new();
    options.set_body(body);
    
    if let Some(icon) = icon_url {
        options.set_icon(icon);
    } else {
        options.set_icon("/assets/icons/eustress-gear.svg");
    }
    
    Notification::new_with_options(title, &options)
}

/// Notification preferences stored in localStorage.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct NotificationPreferences {
    pub enabled: bool,
    pub favorite_updates: bool,
    pub friend_activity: bool,
    pub messages: bool,
    pub marketing: bool,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            enabled: true,
            favorite_updates: true,
            friend_activity: true,
            messages: true,
            marketing: false,
        }
    }
}

impl NotificationPreferences {
    /// Load preferences from localStorage.
    pub fn load() -> Self {
        gloo_storage::LocalStorage::get("notification_prefs")
            .unwrap_or_default()
    }
    
    /// Save preferences to localStorage.
    pub fn save(&self) {
        let _ = gloo_storage::LocalStorage::set("notification_prefs", self);
    }
}

/// Check for favorite updates (polling approach).
/// In production, this would connect to a WebSocket or use Server-Sent Events.
pub async fn check_favorite_updates(api_url: &str, user_id: &str) -> Result<Vec<FavoriteUpdate>, String> {
    use gloo_net::http::Request;
    
    let url = format!("{}/api/favorites/{}/updates", api_url, user_id);
    
    let response = Request::get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    
    if response.ok() {
        response
            .json::<Vec<FavoriteUpdate>>()
            .await
            .map_err(|e| e.to_string())
    } else {
        Err("Failed to fetch updates".to_string())
    }
}

/// Represents an update to a favorited experience.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FavoriteUpdate {
    pub experience_id: String,
    pub experience_name: String,
    pub update_type: String,
    pub description: String,
    pub timestamp: String,
    pub icon_url: Option<String>,
}

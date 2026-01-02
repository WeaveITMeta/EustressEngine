#![allow(dead_code)]

use bevy::prelude::*;
use bevy_egui::egui;
use egui_notify::Toasts;
use std::sync::{Arc, Mutex};
use chrono::{DateTime, Utc};

/// Backend API URL
const API_URL: &str = "https://api.eustress.dev";

/// Poll interval for favorite updates (5 minutes)
const POLL_INTERVAL_SECS: f32 = 300.0;

/// Resource for managing toast notifications
#[derive(Resource)]
pub struct NotificationManager {
    toasts: Toasts,
}

/// Resource for polling favorite updates
#[derive(Resource)]
pub struct FavoriteUpdatePoller {
    /// Timer for polling interval
    pub poll_timer: Timer,
    /// Last poll timestamp (ISO 8601)
    pub last_poll: Option<DateTime<Utc>>,
    /// Async result receiver
    pub async_result: Arc<Mutex<Option<Vec<ExperienceUpdate>>>>,
    /// Whether polling is in progress
    pub polling: bool,
    /// Whether polling is enabled
    pub enabled: bool,
}

impl Default for FavoriteUpdatePoller {
    fn default() -> Self {
        Self {
            poll_timer: Timer::from_seconds(POLL_INTERVAL_SECS, TimerMode::Repeating),
            last_poll: None,
            async_result: Arc::new(Mutex::new(None)),
            polling: false,
            enabled: true,
        }
    }
}

impl FavoriteUpdatePoller {
    /// Trigger an immediate poll (resets timer)
    pub fn poll_now(&mut self) {
        // Set timer to finished so next tick triggers poll
        self.poll_timer.set_elapsed(std::time::Duration::from_secs_f32(POLL_INTERVAL_SECS));
    }
}

/// Experience update from the API
#[derive(Clone, Debug)]
pub struct ExperienceUpdate {
    pub experience_id: String,
    pub name: String,
    pub version: i32,
    pub updated_at: String,
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self {
            toasts: Toasts::default()
                .with_anchor(egui_notify::Anchor::BottomRight)
                .with_margin(egui::vec2(8.0, 8.0)),
        }
    }
}

impl NotificationManager {
    /// Show a success notification (green)
    pub fn success(&mut self, message: impl Into<String>) {
        self.toasts.success(message.into());
    }
    
    /// Show an info notification (blue)
    pub fn info(&mut self, message: impl Into<String>) {
        self.toasts.info(message.into());
    }
    
    /// Show a warning notification (yellow)
    pub fn warning(&mut self, message: impl Into<String>) {
        self.toasts.warning(message.into());
    }
    
    /// Show an error notification (red)
    pub fn error(&mut self, message: impl Into<String>) {
        self.toasts.error(message.into());
    }
    
    /// Show a basic notification (gray)
    pub fn basic(&mut self, message: impl Into<String>) {
        self.toasts.basic(message.into());
    }
}

/// Plugin for notification system
pub struct NotificationPlugin;

impl Plugin for NotificationPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<NotificationManager>()
            .init_resource::<FavoriteUpdatePoller>()
            .init_resource::<PreviousAuthStatus>()
            .add_message::<NotificationEvent>()
            .add_systems(Update, (
                show_notifications,
                handle_notification_events,
                poll_favorite_updates,
                process_favorite_updates,
                trigger_poll_on_login,
            ));
    }
}

/// Track previous auth status to detect login
#[derive(Resource, Default)]
struct PreviousAuthStatus {
    was_logged_in: bool,
}

/// System to render notifications
fn show_notifications(
    mut contexts: bevy_egui::EguiContexts,
    mut notifications: ResMut<NotificationManager>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return; };
    
    // Wrap in catch_unwind to prevent panic if fonts aren't ready on first frame
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        notifications.toasts.show(ctx);
    }));
}

/// Event to trigger notifications from anywhere
#[derive(Message)]
pub enum NotificationEvent {
    Success(String),
    Info(String),
    Warning(String),
    Error(String),
    Basic(String),
}

/// System to handle notification events
pub fn handle_notification_events(
    mut events: MessageReader<NotificationEvent>,
    mut notifications: ResMut<NotificationManager>,
) {
    for event in events.read() {
        match event {
            NotificationEvent::Success(msg) => notifications.success(msg),
            NotificationEvent::Info(msg) => notifications.info(msg),
            NotificationEvent::Warning(msg) => notifications.warning(msg),
            NotificationEvent::Error(msg) => notifications.error(msg),
            NotificationEvent::Basic(msg) => notifications.basic(msg),
        }
    }
}

/// System to poll for favorite updates
fn poll_favorite_updates(
    time: Res<Time>,
    mut poller: ResMut<FavoriteUpdatePoller>,
    auth_state: Res<crate::auth::AuthState>,
) {
    // Don't poll if disabled, already polling, or not logged in
    if !poller.enabled || poller.polling || !auth_state.is_logged_in() {
        return;
    }
    
    // Check timer
    poller.poll_timer.tick(time.delta());
    if !poller.poll_timer.just_finished() {
        return;
    }
    
    // Get auth token
    let Some(token) = auth_state.token.clone() else {
        return;
    };
    
    // Get last poll time
    let since = poller.last_poll
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| {
            // Default to 24 hours ago on first poll
            (Utc::now() - chrono::Duration::hours(24)).to_rfc3339()
        });
    
    poller.polling = true;
    let result_arc = poller.async_result.clone();
    
    // Spawn async poll
    std::thread::spawn(move || {
        let updates = fetch_favorite_updates(&token, &since);
        if let Ok(mut guard) = result_arc.lock() {
            *guard = Some(updates);
        }
    });
}

/// Fetch favorite updates from API
fn fetch_favorite_updates(token: &str, since: &str) -> Vec<ExperienceUpdate> {
    let client = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    
    let url = format!("{}/api/favorites/updates?since={}", API_URL, urlencoding::encode(since));
    
    let response = client.get(&url)
        .set("Authorization", &format!("Bearer {}", token))
        .call();
    
    match response {
        Ok(resp) => {
            let json: serde_json::Value = match resp.into_json() {
                Ok(j) => j,
                Err(_) => return vec![],
            };
            
            let updates = json["updates"].as_array()
                .map(|arr| {
                    arr.iter().filter_map(|item| {
                        Some(ExperienceUpdate {
                            experience_id: item["experience_id"].as_str()?.to_string(),
                            name: item["name"].as_str()?.to_string(),
                            version: item["version"].as_i64()? as i32,
                            updated_at: item["updated_at"].as_str()?.to_string(),
                        })
                    }).collect()
                })
                .unwrap_or_default();
            
            updates
        }
        Err(_) => vec![],
    }
}

/// System to process favorite update results and show notifications
fn process_favorite_updates(
    mut poller: ResMut<FavoriteUpdatePoller>,
    mut notifications: ResMut<NotificationManager>,
) {
    // Check for async results
    let updates = {
        if let Ok(mut guard) = poller.async_result.try_lock() {
            guard.take()
        } else {
            None
        }
    };
    
    if let Some(updates) = updates {
        poller.polling = false;
        poller.last_poll = Some(Utc::now());
        
        // Show notifications for each update
        for update in updates {
            notifications.info(format!(
                "🎮 {} updated to v{}",
                update.name,
                update.version
            ));
        }
    }
}

/// System to trigger poll when user logs in
fn trigger_poll_on_login(
    auth_state: Res<crate::auth::AuthState>,
    mut prev_status: ResMut<PreviousAuthStatus>,
    mut poller: ResMut<FavoriteUpdatePoller>,
    mut notifications: ResMut<NotificationManager>,
) {
    let is_logged_in = auth_state.is_logged_in();
    
    // Detect login transition
    if is_logged_in && !prev_status.was_logged_in {
        // User just logged in - show welcome and trigger poll
        if let Some(user) = &auth_state.user {
            notifications.success(format!("Welcome back, {}!", user.username));
        }
        
        // Trigger immediate poll for updates
        poller.poll_now();
    }
    
    prev_status.was_logged_in = is_logged_in;
}

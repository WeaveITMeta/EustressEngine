//! Notification system - Slint-based (egui_notify removed)

#![allow(dead_code)]

use bevy::prelude::*;
use std::sync::{Arc, Mutex};
use chrono::{DateTime, Utc};

/// Backend API URL
const API_URL: &str = "https://api.eustress.dev";

/// Poll interval for favorite updates (5 minutes)
const POLL_INTERVAL_SECS: f32 = 300.0;

/// Notification level
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// A notification message
#[derive(Clone, Debug)]
pub struct Notification {
    pub level: NotificationLevel,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

/// Resource for managing notifications (Slint-based)
#[derive(Resource, Default)]
pub struct NotificationManager {
    pub notifications: Vec<Notification>,
    pub max_notifications: usize,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            notifications: Vec::new(),
            max_notifications: 10,
        }
    }
    
    pub fn info(&mut self, message: impl Into<String>) {
        self.add(NotificationLevel::Info, message.into());
    }
    
    pub fn success(&mut self, message: impl Into<String>) {
        self.add(NotificationLevel::Success, message.into());
    }
    
    pub fn warning(&mut self, message: impl Into<String>) {
        self.add(NotificationLevel::Warning, message.into());
    }
    
    pub fn error(&mut self, message: impl Into<String>) {
        self.add(NotificationLevel::Error, message.into());
    }
    
    fn add(&mut self, level: NotificationLevel, message: String) {
        self.notifications.push(Notification {
            level,
            message,
            timestamp: Utc::now(),
        });
        
        // Trim old notifications
        while self.notifications.len() > self.max_notifications {
            self.notifications.remove(0);
        }
    }
    
    pub fn clear(&mut self) {
        self.notifications.clear();
    }
}

/// Resource for polling favorite updates
#[derive(Resource)]
pub struct FavoriteUpdatePoller {
    pub poll_timer: Timer,
    pub last_poll: Option<DateTime<Utc>>,
    pub async_result: Arc<Mutex<Option<Vec<ExperienceUpdate>>>>,
    pub polling: bool,
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
    pub fn poll_now(&mut self) {
        self.poll_timer.reset();
        self.polling = true;
    }
}

/// Experience update info
#[derive(Clone, Debug)]
pub struct ExperienceUpdate {
    pub experience_id: String,
    pub name: String,
    pub updated_at: DateTime<Utc>,
}

/// Drains [`NotificationManager::notifications`] into the UI toast queue each
/// frame. Callers of `NotificationManager::success/info/warning/error` don't
/// need to know anything about toasts — this bridge converts every entry
/// into a [`crate::ui::notifications_impl::ActiveNotification`] and expires
/// them via the toast system's regular timer.
fn drain_manager_to_toasts(
    mut manager: ResMut<NotificationManager>,
    mut queue: Option<ResMut<crate::ui::notifications_impl::NotificationQueue>>,
    time: Res<Time>,
) {
    use crate::ui::notifications_impl::{ActiveNotification, NotificationCategory, NotificationLevel as ToastLevel};
    let Some(ref mut queue) = queue else { return };
    if manager.notifications.is_empty() { return; }

    let now = time.elapsed_secs_f64();
    for n in manager.notifications.drain(..) {
        let (level, duration_ms) = match n.level {
            NotificationLevel::Info    => (ToastLevel::Info,    4.0),
            NotificationLevel::Success => (ToastLevel::Success, 3.0),
            NotificationLevel::Warning => (ToastLevel::Warning, 6.0),
            NotificationLevel::Error   => (ToastLevel::Error,   8.0),
        };
        let id = queue.next_id;
        queue.next_id = queue.next_id.wrapping_add(1).max(1);
        // Split first line into title, rest into message.
        let mut parts = n.message.splitn(2, ". ");
        let first = parts.next().unwrap_or("").trim().to_string();
        let rest = parts.next().unwrap_or("").trim().to_string();
        let (title, message) = if rest.is_empty() { (first.clone(), String::new()) } else { (first, rest) };
        queue.push(ActiveNotification {
            id,
            level,
            category: NotificationCategory::General,
            title,
            message,
            dismissible: true,
            expires_at: now + duration_ms,
        });
    }
}

/// Notification plugin
pub struct NotificationPlugin;

impl Plugin for NotificationPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<NotificationManager>()
            .init_resource::<FavoriteUpdatePoller>()
            .add_systems(Update, drain_manager_to_toasts);
    }
}

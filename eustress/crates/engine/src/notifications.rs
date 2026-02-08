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

/// Notification plugin
pub struct NotificationPlugin;

impl Plugin for NotificationPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<NotificationManager>()
            .init_resource::<FavoriteUpdatePoller>();
    }
}

//! Notifications system — toast messages shown in the bottom-right of the
//! output panel. Emitted as events from anywhere in the engine, queued,
//! expired by timer, and pushed to Slint each frame.
//!
//! Granularity is controlled by [`NotificationSettings`], which is user-facing
//! via File > Settings > Notifications.

use bevy::prelude::*;
use bevy::ecs::message::Message;
use std::collections::HashSet;

/// Notification category — lets users silence whole classes of events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotificationCategory {
    /// File save / load / rename / delete
    File,
    /// Script build / compile / run
    Script,
    /// Selection / transform / viewport interaction
    Editor,
    /// Play mode / simulation state
    Simulation,
    /// Network / replication / auth
    Network,
    /// Background tasks / imports / exports
    Task,
    /// General info that doesn't fit elsewhere
    General,
}

impl NotificationCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Script => "script",
            Self::Editor => "editor",
            Self::Simulation => "simulation",
            Self::Network => "network",
            Self::Task => "task",
            Self::General => "general",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl NotificationLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Emit one of these to show a toast. Auto-expires after `duration_ms`.
#[derive(Event, Message, Clone, Debug)]
pub struct NotificationEvent {
    pub level: NotificationLevel,
    pub category: NotificationCategory,
    pub title: String,
    pub message: String,
    pub duration_ms: u32,
    pub dismissible: bool,
}

impl NotificationEvent {
    pub fn info(category: NotificationCategory, title: impl Into<String>, message: impl Into<String>) -> Self {
        Self { level: NotificationLevel::Info, category, title: title.into(), message: message.into(), duration_ms: 4000, dismissible: true }
    }
    pub fn success(category: NotificationCategory, title: impl Into<String>, message: impl Into<String>) -> Self {
        Self { level: NotificationLevel::Success, category, title: title.into(), message: message.into(), duration_ms: 3000, dismissible: true }
    }
    pub fn warning(category: NotificationCategory, title: impl Into<String>, message: impl Into<String>) -> Self {
        Self { level: NotificationLevel::Warning, category, title: title.into(), message: message.into(), duration_ms: 6000, dismissible: true }
    }
    pub fn error(category: NotificationCategory, title: impl Into<String>, message: impl Into<String>) -> Self {
        Self { level: NotificationLevel::Error, category, title: title.into(), message: message.into(), duration_ms: 8000, dismissible: true }
    }
}

#[derive(Clone, Debug)]
pub struct ActiveNotification {
    pub id: u32,
    pub level: NotificationLevel,
    pub category: NotificationCategory,
    pub title: String,
    pub message: String,
    pub dismissible: bool,
    pub expires_at: f64,
}

/// Holds currently-visible toasts, bounded to prevent runaway growth.
#[derive(Resource)]
pub struct NotificationQueue {
    pub items: Vec<ActiveNotification>,
    pub next_id: u32,
    pub max_visible: usize,
    pub dirty: bool,
}

impl Default for NotificationQueue {
    fn default() -> Self {
        Self { items: Vec::new(), next_id: 1, max_visible: 6, dirty: true }
    }
}

impl NotificationQueue {
    pub fn push(&mut self, active: ActiveNotification) {
        self.items.push(active);
        while self.items.len() > self.max_visible {
            self.items.remove(0);
        }
        self.dirty = true;
    }
    pub fn dismiss(&mut self, id: u32) {
        let before = self.items.len();
        self.items.retain(|n| n.id != id);
        if self.items.len() != before {
            self.dirty = true;
        }
    }
}

/// User preferences for which notification categories fire. Persisted via
/// `editor_settings` so toggles survive restarts.
#[derive(Resource, Clone, Debug)]
pub struct NotificationSettings {
    /// Master switch — if false, all notifications are suppressed.
    pub enabled: bool,
    /// Categories the user has explicitly muted.
    pub muted_categories: HashSet<String>,
    /// Muted levels (user might want to silence Info but keep Error).
    pub muted_levels: HashSet<String>,
    /// Show dismiss button on toasts.
    pub show_dismiss: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            muted_categories: HashSet::new(),
            muted_levels: HashSet::new(),
            show_dismiss: true,
        }
    }
}

impl NotificationSettings {
    pub fn allows(&self, ev: &NotificationEvent) -> bool {
        self.enabled
            && !self.muted_categories.contains(ev.category.as_str())
            && !self.muted_levels.contains(ev.level.as_str())
    }
}

/// Reads [`NotificationEvent`]s and queues them as active toasts.
fn receive_notifications(
    mut events: MessageReader<NotificationEvent>,
    mut queue: ResMut<NotificationQueue>,
    settings: Res<NotificationSettings>,
    time: Res<Time>,
) {
    for ev in events.read() {
        if !settings.allows(ev) {
            continue;
        }
        let id = queue.next_id;
        queue.next_id = queue.next_id.wrapping_add(1).max(1);
        queue.push(ActiveNotification {
            id,
            level: ev.level,
            category: ev.category,
            title: ev.title.clone(),
            message: ev.message.clone(),
            dismissible: ev.dismissible && settings.show_dismiss,
            expires_at: time.elapsed_secs_f64() + (ev.duration_ms as f64 / 1000.0),
        });
    }
}

/// Drops expired notifications from the queue. Runs every frame.
fn expire_notifications(
    mut queue: ResMut<NotificationQueue>,
    time: Res<Time>,
) {
    let now = time.elapsed_secs_f64();
    let before = queue.items.len();
    queue.items.retain(|n| n.expires_at > now);
    if queue.items.len() != before {
        queue.dirty = true;
    }
}

/// Dismiss request coming from Slint (user clicked the X).
#[derive(Event, Message, Clone, Debug)]
pub struct DismissNotificationEvent(pub u32);

fn handle_dismiss(
    mut events: MessageReader<DismissNotificationEvent>,
    mut queue: ResMut<NotificationQueue>,
) {
    for ev in events.read() {
        queue.dismiss(ev.0);
    }
}

pub struct NotificationsPlugin;

impl Plugin for NotificationsPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<NotificationQueue>()
            .init_resource::<NotificationSettings>()
            .add_message::<NotificationEvent>()
            .add_message::<DismissNotificationEvent>()
            .add_systems(Update, (receive_notifications, expire_notifications, handle_dismiss).chain());
    }
}

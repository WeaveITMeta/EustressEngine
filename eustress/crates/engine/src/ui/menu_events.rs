// Menu Action Events - Triggered by both ribbon menu and keyboard shortcuts
use bevy::prelude::*;
use crate::keybindings::Action;

/// Event for menu actions triggered by UI or keyboard
#[derive(Message)]
pub struct MenuActionEvent {
    pub action: Action,
}

impl MenuActionEvent {
    pub fn new(action: Action) -> Self {
        Self { action }
    }
}

//! Command History - Undo/Redo stack management

#![allow(dead_code)]

use bevy::prelude::*;
use super::property_command::{PropertyCommand, BatchCommand};
use super::entity_command::{DeleteCommand, DuplicateCommand, CreateCommand};
use std::time::{Duration, Instant};

const MAX_HISTORY: usize = 100;
const MERGE_WINDOW_MS: u64 = 300; // Merge commands within 300ms

/// Selection change command — stores previous and new selection sets.
/// Ctrl+Z / Ctrl+Y cycles through selection history.
#[derive(Clone, Debug)]
pub struct SelectionCommand {
    pub description: String,
    /// Selection IDs before this change
    pub previous: Vec<String>,
    /// Selection IDs after this change
    pub current: Vec<String>,
}

impl SelectionCommand {
    pub fn new(previous: Vec<String>, current: Vec<String>) -> Self {
        let desc = if current.is_empty() {
            "Deselect all".to_string()
        } else if current.len() == 1 {
            "Select entity".to_string()
        } else {
            format!("Select {} entities", current.len())
        };
        Self { description: desc, previous, current }
    }

    pub fn execute(&mut self, world: &mut World) -> Result<(), String> {
        if let Some(mgr) = world.get_resource::<crate::selection_sync::SelectionSyncManager>() {
            let sel = mgr.0.read();
            sel.set_selected(self.current.clone());
        }
        Ok(())
    }

    pub fn undo(&self, world: &mut World) -> Result<(), String> {
        if let Some(mgr) = world.get_resource::<crate::selection_sync::SelectionSyncManager>() {
            let sel = mgr.0.read();
            sel.set_selected(self.previous.clone());
        }
        Ok(())
    }
}

/// Represents any command that can be undone/redone
#[derive(Clone, Debug)]
pub enum Command {
    Property(PropertyCommand),
    Batch(BatchCommand),
    Delete(DeleteCommand),
    Duplicate(DuplicateCommand),
    Create(CreateCommand),
    Selection(SelectionCommand),
}

impl Command {
    pub fn execute(&mut self, world: &mut World) -> Result<(), String> {
        match self {
            Command::Property(cmd) => cmd.execute(world),
            Command::Batch(cmd) => cmd.execute(world),
            Command::Delete(cmd) => cmd.execute(world),
            Command::Duplicate(cmd) => cmd.execute(world),
            Command::Create(cmd) => cmd.execute(world),
            Command::Selection(cmd) => cmd.execute(world),
        }
    }

    pub fn undo(&self, world: &mut World) -> Result<(), String> {
        match self {
            Command::Property(cmd) => cmd.undo(world),
            Command::Batch(cmd) => cmd.undo(world),
            Command::Delete(cmd) => cmd.undo(world),
            Command::Duplicate(cmd) => cmd.undo(world),
            Command::Create(cmd) => cmd.undo(world),
            Command::Selection(cmd) => cmd.undo(world),
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Command::Property(cmd) => &cmd.description,
            Command::Batch(cmd) => &cmd.description,
            Command::Delete(cmd) => &cmd.description,
            Command::Duplicate(cmd) => &cmd.description,
            Command::Create(cmd) => &cmd.description,
            Command::Selection(cmd) => &cmd.description,
        }
    }

    /// Check if this command can merge with another PropertyCommand
    pub fn can_merge_property(&self, other: &PropertyCommand) -> bool {
        match self {
            Command::Property(cmd) => cmd.can_merge(other),
            Command::Batch(_) => false,
            Command::Delete(_) => false,
            Command::Duplicate(_) => false,
            Command::Create(_) => false,
            Command::Selection(_) => false,
        }
    }
    
    /// Merge with another PropertyCommand
    pub fn merge_property(&mut self, other: PropertyCommand) {
        if let Command::Property(cmd) = self {
            cmd.merge(other);
        }
    }
}

/// Command history with undo/redo stack
#[derive(Resource)]
pub struct CommandHistory {
    stack: Vec<Command>,
    current_index: usize,
    last_command_time: Option<Instant>,
}

impl Default for CommandHistory {
    fn default() -> Self {
        Self {
            stack: Vec::new(),
            current_index: 0,
            last_command_time: None,
        }
    }
}

impl CommandHistory {
    /// Execute and push a new command
    pub fn execute(&mut self, mut command: Command, world: &mut World) -> Result<(), String> {
        // Execute the command
        command.execute(world)?;
        
        // Try to merge with previous command if within merge window
        let now = Instant::now();
        let should_merge = if let Some(last_time) = self.last_command_time {
            now.duration_since(last_time) < Duration::from_millis(MERGE_WINDOW_MS)
                && self.current_index > 0
        } else {
            false
        };
        
        if should_merge {
            if let Command::Property(ref prop_cmd) = command {
                if let Some(last_cmd) = self.stack.get_mut(self.current_index - 1) {
                    if last_cmd.can_merge_property(prop_cmd) {
                        last_cmd.merge_property(prop_cmd.clone());
                        self.last_command_time = Some(now);
                        return Ok(());
                    }
                }
            }
        }
        
        // Clear any redo history
        self.stack.truncate(self.current_index);
        
        // Add command to stack
        self.stack.push(command);
        self.current_index += 1;
        self.last_command_time = Some(now);
        
        // Limit history size
        if self.stack.len() > MAX_HISTORY {
            self.stack.remove(0);
            self.current_index -= 1;
        }
        
        Ok(())
    }
    
    /// Push a selection command without executing it (selection already happened).
    /// Used by record_selection_history to track selection changes for undo/redo.
    pub fn push_selection(&mut self, cmd: SelectionCommand) {
        // Clear any redo history
        self.stack.truncate(self.current_index);
        self.stack.push(Command::Selection(cmd));
        self.current_index += 1;
        self.last_command_time = Some(Instant::now());

        // Limit history size
        if self.stack.len() > MAX_HISTORY {
            self.stack.remove(0);
            self.current_index -= 1;
        }
    }

    /// Undo the last command
    pub fn undo(&mut self, world: &mut World) -> Result<(), String> {
        if !self.can_undo() {
            return Err("Nothing to undo".to_string());
        }
        
        self.current_index -= 1;
        let command = &self.stack[self.current_index];
        command.undo(world)?;
        
        // Reset merge window
        self.last_command_time = None;
        
        Ok(())
    }
    
    /// Redo the next command
    pub fn redo(&mut self, world: &mut World) -> Result<(), String> {
        if !self.can_redo() {
            return Err("Nothing to redo".to_string());
        }
        
        let command = &mut self.stack[self.current_index];
        command.execute(world)?;
        self.current_index += 1;
        
        // Reset merge window
        self.last_command_time = None;
        
        Ok(())
    }
    
    /// Check if undo is available
    pub fn can_undo(&self) -> bool {
        self.current_index > 0
    }
    
    /// Check if redo is available
    pub fn can_redo(&self) -> bool {
        self.current_index < self.stack.len()
    }
    
    /// Get description of command that would be undone
    pub fn undo_description(&self) -> Option<&str> {
        if self.can_undo() {
            Some(self.stack[self.current_index - 1].description())
        } else {
            None
        }
    }
    
    /// Get description of command that would be redone
    pub fn redo_description(&self) -> Option<&str> {
        if self.can_redo() {
            Some(self.stack[self.current_index].description())
        } else {
            None
        }
    }
    
    /// Get all commands for display in history panel
    pub fn get_history(&self) -> Vec<(usize, &str, bool)> {
        self.stack
            .iter()
            .enumerate()
            .map(|(i, cmd)| (i, cmd.description(), i < self.current_index))
            .collect()
    }
    
    /// Clear all history
    pub fn clear(&mut self) {
        self.stack.clear();
        self.current_index = 0;
        self.last_command_time = None;
    }
    
    /// Get the last executed command (for accessing execution results)
    pub fn get_last_command(&self) -> Option<&Command> {
        if self.current_index > 0 {
            self.stack.get(self.current_index - 1)
        } else {
            None
        }
    }
    
    /// Jump to a specific point in history
    pub fn jump_to(&mut self, index: usize, world: &mut World) -> Result<(), String> {
        if index > self.stack.len() {
            return Err("Invalid history index".to_string());
        }
        
        // Undo or redo to reach target index
        while self.current_index > index {
            self.undo(world)?;
        }
        while self.current_index < index {
            self.redo(world)?;
        }
        
        Ok(())
    }
}

/// Events for undo/redo
#[derive(Message)]
pub struct UndoCommandEvent;

#[derive(Message)]
pub struct RedoCommandEvent;

/// Events for history panel actions (jump-to, clear)
#[derive(Message)]
pub enum HistoryActionEvent {
    JumpTo(i32),
    Clear,
}

/// Snapshot of history state for syncing to UI each frame.
/// Only rebuilt when history generation changes.
#[derive(Resource, Default)]
pub struct HistorySnapshot {
    pub entries: Vec<HistoryDisplayEntry>,
    pub current_index: usize,
    pub generation: u64,
}

/// A single entry for UI display
#[derive(Clone, Debug)]
pub struct HistoryDisplayEntry {
    pub id: i32,
    pub action: String,
    pub description: String,
    pub timestamp: String,
    pub is_current: bool,
}

impl CommandHistory {
    /// Build a snapshot of the history for UI display.
    pub fn snapshot(&self) -> (Vec<HistoryDisplayEntry>, usize) {
        let entries = self.stack.iter().enumerate().map(|(i, cmd)| {
            let action = match cmd {
                Command::Property(_) => "property",
                Command::Batch(_) => "property",
                Command::Delete(_) => "delete",
                Command::Duplicate(_) => "paste",
                Command::Create(_) => "create",
                Command::Selection(_) => "select",
            };
            HistoryDisplayEntry {
                id: i as i32,
                action: action.to_string(),
                description: cmd.description().to_string(),
                is_current: i == self.current_index.saturating_sub(1),
                timestamp: String::new(), // filled by sync system
            }
        }).collect();
        (entries, self.current_index)
    }
}

/// System that handles undo/redo/jump-to/clear events.
/// Uses SelectionSyncManager for selection commands.
pub fn process_history_events(
    mut undo_events: MessageReader<UndoCommandEvent>,
    mut redo_events: MessageReader<RedoCommandEvent>,
    mut history_events: MessageReader<HistoryActionEvent>,
    mut history: ResMut<CommandHistory>,
    selection_sync: Option<Res<crate::selection_sync::SelectionSyncManager>>,
    mut output: Option<ResMut<crate::ui::slint_ui::OutputConsole>>,
) {
    let sel_mgr = selection_sync.as_ref().map(|s| s.0.clone());

    for _ in undo_events.read() {
        if history.can_undo() {
            history.current_index -= 1;
            let cmd = &history.stack[history.current_index];
            // Only selection commands can be undone without &mut World
            if let Command::Selection(sel_cmd) = cmd {
                if let Some(ref mgr) = sel_mgr {
                    let m = mgr.read();
                    m.set_selected(sel_cmd.previous.clone());
                }
            }
            if let Some(ref mut out) = output {
                out.info(format!("Undo: {}", cmd.description()));
            }
            history.last_command_time = None;
        }
    }

    for _ in redo_events.read() {
        if history.can_redo() {
            let cmd = &history.stack[history.current_index];
            if let Command::Selection(sel_cmd) = cmd {
                if let Some(ref mgr) = sel_mgr {
                    let m = mgr.read();
                    m.set_selected(sel_cmd.current.clone());
                }
            }
            if let Some(ref mut out) = output {
                out.info(format!("Redo: {}", cmd.description()));
            }
            history.current_index += 1;
            history.last_command_time = None;
        }
    }

    for event in history_events.read() {
        match event {
            HistoryActionEvent::JumpTo(target_id) => {
                let target = *target_id as usize;
                // Jump by undoing/redoing to reach the target
                // target_id is the index of the entry to jump TO (make it current)
                let target_index = (target + 1).min(history.stack.len());
                while history.current_index > target_index {
                    history.current_index -= 1;
                    if let Command::Selection(sel_cmd) = &history.stack[history.current_index] {
                        if let Some(ref mgr) = sel_mgr {
                            mgr.read().set_selected(sel_cmd.previous.clone());
                        }
                    }
                }
                while history.current_index < target_index {
                    if let Command::Selection(sel_cmd) = &history.stack[history.current_index] {
                        if let Some(ref mgr) = sel_mgr {
                            mgr.read().set_selected(sel_cmd.current.clone());
                        }
                    }
                    history.current_index += 1;
                }
                if let Some(ref mut out) = output {
                    out.info(format!("Jumped to history entry {}", target_id));
                }
                history.last_command_time = None;
            }
            HistoryActionEvent::Clear => {
                history.clear();
                if let Some(ref mut out) = output {
                    out.info("History cleared".to_string());
                }
            }
        }
    }
}

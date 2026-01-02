//! Command History - Undo/Redo stack management

#![allow(dead_code)]

use bevy::prelude::*;
use super::property_command::{PropertyCommand, BatchCommand};
use super::entity_command::{DeleteCommand, DuplicateCommand, CreateCommand};
use std::time::{Duration, Instant};

const MAX_HISTORY: usize = 100;
const MERGE_WINDOW_MS: u64 = 300; // Merge commands within 300ms

/// Represents any command that can be undone/redone
#[derive(Clone, Debug)]
pub enum Command {
    Property(PropertyCommand),
    Batch(BatchCommand),
    Delete(DeleteCommand),
    Duplicate(DuplicateCommand),
    Create(CreateCommand),
}

impl Command {
    pub fn execute(&mut self, world: &mut World) -> Result<(), String> {
        match self {
            Command::Property(cmd) => cmd.execute(world),
            Command::Batch(cmd) => cmd.execute(world),
            Command::Delete(cmd) => cmd.execute(world),
            Command::Duplicate(cmd) => cmd.execute(world),
            Command::Create(cmd) => cmd.execute(world),
        }
    }
    
    pub fn undo(&self, world: &mut World) -> Result<(), String> {
        match self {
            Command::Property(cmd) => cmd.undo(world),
            Command::Batch(cmd) => cmd.undo(world),
            Command::Delete(cmd) => cmd.undo(world),
            Command::Duplicate(cmd) => cmd.undo(world),
            Command::Create(cmd) => cmd.undo(world),
        }
    }
    
    pub fn description(&self) -> &str {
        match self {
            Command::Property(cmd) => &cmd.description,
            Command::Batch(cmd) => &cmd.description,
            Command::Delete(cmd) => &cmd.description,
            Command::Duplicate(cmd) => &cmd.description,
            Command::Create(cmd) => &cmd.description,
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

/// System to handle undo/redo events
pub fn handle_undo_redo(
    mut undo_events: MessageReader<UndoCommandEvent>,
    mut redo_events: MessageReader<RedoCommandEvent>,
    mut history: ResMut<CommandHistory>,
    world: &mut World,
    // mut notifications: ResMut<crate::notifications::NotificationManager>, // TODO: Fix notifications module path
) {
    for _ in undo_events.read() {
        match history.undo(world) {
            Ok(_) => {
                if let Some(_desc) = history.undo_description() {
                    // notifications.info(format!("Undone: {}", desc));
                }
            }
            Err(_e) => {
                // notifications.error(e);
            }
        }
    }
    
    for _ in redo_events.read() {
        match history.redo(world) {
            Ok(_) => {
                if let Some(_desc) = history.redo_description() {
                    // notifications.info(format!("Redone: {}", desc));
                }
            }
            Err(_e) => {
                // notifications.error(e);
            }
        }
    }
}

// Commands modules - Production-ready managers for Eustress Engine
// Old Tauri command functions archived in _old_tauri_commands/

// Core scene and selection management
pub mod scene_commands;
pub mod scene_management;

// Phase 2 Week 4: Command system for undo/redo
pub mod property_command;
pub mod entity_command;
pub mod history;

// Re-export managers for use in main.rs and UI modules
pub use scene_commands::{SelectionManager, TransformManager};
#[allow(unused_imports)]
pub use scene_management::SceneManagerState;

// Re-export command system
#[allow(unused_imports)]
pub use property_command::{PropertyCommand, BatchCommand};
#[allow(unused_imports)]
pub use entity_command::{DeleteCommand, DuplicateCommand, CreateCommand, DeletedEntity, DuplicatedEntity};
#[allow(unused_imports)]
pub use history::{Command, CommandHistory, UndoCommandEvent, RedoCommandEvent, handle_undo_redo};

// These managers provide full-featured state tracking and command execution
// Integrated with egui UI for professional editing workflows

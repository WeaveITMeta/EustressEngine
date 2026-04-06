//! # GUI Script Commands — Shared Bridge
//!
//! Thread-local command queue and snapshot used by both Rune and Luau scripting
//! runtimes to manipulate GuiElementDisplay components at runtime.
//!
//! Flow:
//! 1. Before scripts run: `set_gui_snapshot()` populates name→text map
//! 2. Scripts call `gui_set_text()`, `gui_set_visible()`, etc. → pushes to `GUI_COMMANDS`
//! 3. After scripts run: `drain_gui_commands()` → Bevy system applies to GuiElementDisplay

use std::cell::RefCell;
use std::collections::HashMap;

/// Command to update a GUI element property (pushed by script, applied by Bevy system)
#[derive(Debug, Clone)]
pub enum GuiCommand {
    SetText { name: String, text: String },
    SetVisible { name: String, visible: bool },
    SetBgColor { name: String, r: f32, g: f32, b: f32, a: f32 },
    SetTextColor { name: String, r: f32, g: f32, b: f32, a: f32 },
    SetBorderColor { name: String, r: f32, g: f32, b: f32, a: f32 },
    SetPosition { name: String, x: f32, y: f32 },
    SetSize { name: String, w: f32, h: f32 },
    SetFontSize { name: String, size: f32 },
    OnClick { name: String, callback_id: String },
}

thread_local! {
    /// Pending GUI commands from scripts (drained each frame by Bevy system)
    pub static GUI_COMMANDS: RefCell<Vec<GuiCommand>> = RefCell::new(Vec::new());
    /// Read-only snapshot of GUI element text values (name → text) for gui_get_text()
    pub static GUI_SNAPSHOT: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Push a GUI command from any scripting runtime
pub fn push_gui_command(cmd: GuiCommand) {
    GUI_COMMANDS.with(|cmds| cmds.borrow_mut().push(cmd));
}

/// Drain all pending GUI commands (called by Bevy system after script execution)
pub fn drain_gui_commands() -> Vec<GuiCommand> {
    GUI_COMMANDS.with(|cmds| std::mem::take(&mut *cmds.borrow_mut()))
}

/// Set GUI text snapshot (called by Bevy system before script execution)
pub fn set_gui_snapshot(snapshot: HashMap<String, String>) {
    GUI_SNAPSHOT.with(|s| *s.borrow_mut() = snapshot);
}

/// Read a GUI element's text from the snapshot
pub fn gui_snapshot_get(name: &str) -> String {
    GUI_SNAPSHOT.with(|s| s.borrow().get(name).cloned().unwrap_or_default())
}

/// Clear GUI snapshot
pub fn clear_gui_snapshot() {
    GUI_SNAPSHOT.with(|s| s.borrow_mut().clear());
}

// ============================================================================
// Script Log Buffer — routes script print/log calls to the Output panel
// ============================================================================

/// Log level for script messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptLogLevel {
    Info,
    Warn,
    Error,
}

/// A log message from a script
#[derive(Debug, Clone)]
pub struct ScriptLogEntry {
    pub level: ScriptLogLevel,
    pub message: String,
}

thread_local! {
    /// Pending script log entries (drained each frame by Bevy system → OutputConsole)
    pub static SCRIPT_LOGS: RefCell<Vec<ScriptLogEntry>> = RefCell::new(Vec::new());
}

/// Push a script log message
pub fn push_script_log(level: ScriptLogLevel, message: String) {
    SCRIPT_LOGS.with(|logs| logs.borrow_mut().push(ScriptLogEntry { level, message }));
}

/// Drain all pending script log entries
pub fn drain_script_logs() -> Vec<ScriptLogEntry> {
    SCRIPT_LOGS.with(|logs| std::mem::take(&mut *logs.borrow_mut()))
}

//! # GUI Script Commands — Shared Bridge
//!
//! Process-wide command queue and snapshot used by both Rune and Luau scripting
//! runtimes to manipulate GuiElementDisplay components at runtime. Scripts run
//! on whichever thread Bevy's executor picks and the apply system on another,
//! so these are shared statics behind a Mutex, never per-thread.
//!
//! Flow:
//! 1. Before scripts run: `set_gui_snapshot()` populates name→text map
//! 2. Scripts call `gui_set_text()`, `gui_set_visible()`, etc. → pushes to `GUI_COMMANDS`
//! 3. After scripts run: `drain_gui_commands()` → Bevy system applies to GuiElementDisplay

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};

/// Lock, recovering the data if a panicking script thread poisoned the lock.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

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

/// Pending GUI commands from scripts (drained each frame by Bevy system)
static GUI_COMMANDS: Mutex<Vec<GuiCommand>> = Mutex::new(Vec::new());
/// Read-only snapshot of GUI element text values (name → text) for gui_get_text()
static GUI_SNAPSHOT: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Push a GUI command from any scripting runtime
pub fn push_gui_command(cmd: GuiCommand) {
    lock(&GUI_COMMANDS).push(cmd);
}

/// Drain all pending GUI commands (called by Bevy system after script execution)
pub fn drain_gui_commands() -> Vec<GuiCommand> {
    std::mem::take(&mut *lock(&GUI_COMMANDS))
}

/// Set GUI text snapshot (called by Bevy system before script execution)
pub fn set_gui_snapshot(snapshot: HashMap<String, String>) {
    *lock(&GUI_SNAPSHOT) = snapshot;
}

/// Read a GUI element's text from the snapshot
pub fn gui_snapshot_get(name: &str) -> String {
    lock(&GUI_SNAPSHOT).get(name).cloned().unwrap_or_default()
}

/// Clear GUI snapshot
pub fn clear_gui_snapshot() {
    lock(&GUI_SNAPSHOT).clear();
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
    /// The script that logged it, when the call came from a running script.
    pub script: Option<String>,
    /// The script's source file, as an absolute path.
    pub file: Option<String>,
    /// 1-based source line, when known.
    pub line: Option<u32>,
    /// Call frames, innermost first (`"on_update (line 12)"`); empty when
    /// not known.
    pub stack: Vec<String>,
}

thread_local! {
    /// The script (name, source file) whose VM call is running on this
    /// thread. The runtime sets it around each call, and the log functions
    /// a script calls run on that same thread, so a per-thread slot is
    /// exact here, unlike the log buffer, which crosses threads.
    static CURRENT_SCRIPT: std::cell::RefCell<Option<(String, Option<String>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Attribute this thread's script logs to `script` (and its source `file`)
/// until the returned guard drops. The guard restores whatever was set
/// before, so nested scopes and early returns never leave a stale script.
pub fn script_scope(script: &str, file: Option<&str>) -> ScriptScope {
    let previous = CURRENT_SCRIPT.with(|c| {
        c.replace(Some((script.to_string(), file.map(str::to_string))))
    });
    ScriptScope { previous }
}

/// Restores the previous current script when dropped. See [`script_scope`].
pub struct ScriptScope {
    previous: Option<(String, Option<String>)>,
}

impl Drop for ScriptScope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        CURRENT_SCRIPT.with(|c| *c.borrow_mut() = previous);
    }
}

/// The script (name, source file) running on this thread, if any.
pub fn current_script() -> Option<(String, Option<String>)> {
    CURRENT_SCRIPT.with(|c| c.borrow().clone())
}

/// Pending script log entries, drained each frame into the Output panel.
/// One buffer for the whole process: scripts run on whichever thread Bevy's
/// executor picks, and the drain on another, so a per-thread buffer only
/// delivered lines that happened to share a thread with the drain.
static SCRIPT_LOGS: Mutex<Vec<ScriptLogEntry>> = Mutex::new(Vec::new());

/// Push a script log message, attributed to the script running on this
/// thread (see [`script_scope`]).
pub fn push_script_log(level: ScriptLogLevel, message: String) {
    let (script, file) = match current_script() {
        Some((script, file)) => (Some(script), file),
        None => (None, None),
    };
    push_script_log_detail(level, message, script, file, None, Vec::new());
}

/// Push a script log message with everything known about where it came from.
pub fn push_script_log_detail(
    level: ScriptLogLevel,
    message: String,
    script: Option<String>,
    file: Option<String>,
    line: Option<u32>,
    stack: Vec<String>,
) {
    lock(&SCRIPT_LOGS).push(ScriptLogEntry { level, message, script, file, line, stack });
}

/// Drain all pending script log entries
pub fn drain_script_logs() -> Vec<ScriptLogEntry> {
    std::mem::take(&mut *lock(&SCRIPT_LOGS))
}

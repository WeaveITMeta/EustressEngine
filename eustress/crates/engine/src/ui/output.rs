#![allow(dead_code)]

use bevy::prelude::*;
use bevy_egui::egui;
use std::time::SystemTime;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;

/// Log entry with timestamp and message
#[derive(Clone)]
pub struct LogEntry {
    pub timestamp: SystemTime,
    pub level: LogLevel,
    pub message: String,
    /// Optional file path that can be Ctrl+clicked to open in explorer
    pub file_path: Option<String>,
}

/// Log level for filtering
#[derive(Clone, Copy, PartialEq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
    System,  // Internal debug/system messages
}

/// Maximum entries to display at once (prevents UI lag)
const MAX_DISPLAY_ENTRIES: usize = 500;
/// How many more entries to load when "Show More" is clicked
const LOAD_MORE_BATCH: usize = 500;
/// Maximum total entries to keep in memory
const MAX_TOTAL_ENTRIES: usize = 50_000;

// ============================================================================
// Global Log Capture (for Bevy tracing integration)
// ============================================================================

/// Global buffer for captured logs from tracing
static LOG_BUFFER: std::sync::OnceLock<Arc<Mutex<VecDeque<(LogLevel, String)>>>> = std::sync::OnceLock::new();

/// Get or initialize the global log buffer
fn get_log_buffer() -> Arc<Mutex<VecDeque<(LogLevel, String)>>> {
    LOG_BUFFER.get_or_init(|| Arc::new(Mutex::new(VecDeque::with_capacity(1000)))).clone()
}

/// Push a log to the global buffer (called from tracing layer)
pub fn push_to_log_buffer(level: LogLevel, message: String) {
    if let Ok(mut buffer) = get_log_buffer().lock() {
        buffer.push_back((level, message));
        // Keep buffer bounded
        while buffer.len() > 1000 {
            buffer.pop_front();
        }
    }
}

/// Output console state
#[derive(Resource)]
pub struct OutputConsole {
    pub logs: Vec<LogEntry>,
    pub auto_scroll: bool,
    pub show_info: bool,
    pub show_warning: bool,
    pub show_error: bool,
    pub show_system: bool,
    /// Number of entries currently visible (from the end)
    pub visible_count: usize,
}

impl Default for OutputConsole {
    fn default() -> Self {
        let mut console = Self {
            logs: Vec::new(),
            auto_scroll: true,
            show_info: true,
            show_warning: true,
            show_error: true,
            show_system: true,  // Show system logs by default so users can see activity
            visible_count: MAX_DISPLAY_ENTRIES,
        };
        
        // Add welcome messages
        console.info("🎮 Eustress Engine started");
        console.info("📋 Output panel ready - logs will appear here");
        console.system("💡 Tip: Click ⬇ Auto to auto-scroll to new logs");
        console.system("💡 Tip: Scroll to top to load older logs");
        
        console
    }
}

impl OutputConsole {
    /// Add a log entry and trim old entries if over limit
    fn push_log(&mut self, level: LogLevel, message: String, file_path: Option<String>) {
        self.logs.push(LogEntry {
            timestamp: SystemTime::now(),
            level,
            message,
            file_path,
        });
        
        // Trim old entries if we exceed max
        if self.logs.len() > MAX_TOTAL_ENTRIES {
            let excess = self.logs.len() - MAX_TOTAL_ENTRIES;
            self.logs.drain(0..excess);
        }
    }
    
    pub fn info(&mut self, message: impl Into<String>) {
        self.push_log(LogLevel::Info, message.into(), None);
    }
    
    pub fn warning(&mut self, message: impl Into<String>) {
        self.push_log(LogLevel::Warning, message.into(), None);
    }
    
    pub fn error(&mut self, message: impl Into<String>) {
        self.push_log(LogLevel::Error, message.into(), None);
    }
    
    pub fn system(&mut self, message: impl Into<String>) {
        self.push_log(LogLevel::System, message.into(), None);
    }
    
    /// Log info with a clickable file path (Ctrl+click to open in explorer)
    pub fn info_with_path(&mut self, message: impl Into<String>, path: impl Into<String>) {
        self.push_log(LogLevel::Info, message.into(), Some(path.into()));
    }
    
    /// Log system message with a clickable file path
    pub fn system_with_path(&mut self, message: impl Into<String>, path: impl Into<String>) {
        self.push_log(LogLevel::System, message.into(), Some(path.into()));
    }
    
    pub fn clear(&mut self) {
        self.logs.clear();
        self.visible_count = MAX_DISPLAY_ENTRIES;
        self.info("Output cleared");
    }
    
    /// Load more previous entries
    pub fn load_more(&mut self) {
        self.visible_count = (self.visible_count + LOAD_MORE_BATCH).min(self.logs.len());
    }
    
    /// Check if there are more entries to load
    pub fn has_more(&self) -> bool {
        self.visible_count < self.logs.len()
    }
    
    /// Get the number of hidden entries
    pub fn hidden_count(&self) -> usize {
        self.logs.len().saturating_sub(self.visible_count)
    }
    
    /// Drain logs from the global buffer into this console
    pub fn drain_log_buffer(&mut self) {
        if let Ok(mut buffer) = get_log_buffer().lock() {
            while let Some((level, message)) = buffer.pop_front() {
                self.push_log(level, message, None);
            }
        }
    }
}

// ============================================================================
// Log Capture System
// ============================================================================

/// System to periodically capture logs from Bevy's tracing
/// This polls stdout/stderr and parses log lines
pub fn capture_bevy_logs(mut console: ResMut<OutputConsole>) {
    // Drain any logs that were pushed to the global buffer
    console.drain_log_buffer();
}

/// Parse a log line from Bevy's tracing format and push to buffer
/// Format: [TIMESTAMP] LEVEL TARGET: MESSAGE
pub fn parse_and_push_log(line: &str) {
    // Skip empty lines
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    
    // Detect log level from common patterns
    let (level, message) = if line.contains(" INFO ") || line.contains("[INFO]") || line.starts_with("INFO") {
        (LogLevel::Info, line.to_string())
    } else if line.contains(" WARN ") || line.contains("[WARN]") || line.starts_with("WARN") {
        (LogLevel::Warning, line.to_string())
    } else if line.contains(" ERROR ") || line.contains("[ERROR]") || line.starts_with("ERROR") {
        (LogLevel::Error, line.to_string())
    } else if line.contains(" DEBUG ") || line.contains("[DEBUG]") || line.starts_with("DEBUG") {
        (LogLevel::System, line.to_string())
    } else if line.contains(" TRACE ") || line.contains("[TRACE]") {
        (LogLevel::System, line.to_string())
    } else {
        // Default to info for unrecognized lines
        (LogLevel::Info, line.to_string())
    };
    
    push_to_log_buffer(level, message);
}

/// Output panel (console/logs)
pub struct OutputPanel;

impl OutputPanel {
    /// Show output content (for dock system)
    pub fn show_content(ui: &mut egui::Ui, console: &mut OutputConsole) {
        // First, drain any pending logs from the global buffer
        console.drain_log_buffer();
        
        // Show log count in header
        let total_logs = console.logs.len();
        let visible_logs = console.visible_count.min(total_logs);
        
        ui.horizontal(|ui| {
            // Left side: log count
            ui.label(format!("📝 {} logs ({} shown)", total_logs, visible_logs));
            
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🗑 Clear").clicked() {
                    console.clear();
                }
                
                // Copy all logs to clipboard
                if ui.button("📋 Copy All").clicked() {
                    let all_text: String = console.logs.iter()
                        .map(|entry| {
                            let datetime = chrono::DateTime::<chrono::Local>::from(entry.timestamp);
                            let timestamp = datetime.format("%H:%M:%S").to_string();
                            let icon = match entry.level {
                                LogLevel::Info => "INFO",
                                LogLevel::Warning => "WARN",
                                LogLevel::Error => "ERROR",
                                LogLevel::System => "SYS",
                            };
                            format!("[{}] {} {}", timestamp, icon, entry.message)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    ui.ctx().copy_text(all_text);
                }
                
                // Auto-scroll toggle with better label
                let auto_label = if console.auto_scroll { "⬇ Auto-Scroll ON" } else { "⬇ Auto-Scroll OFF" };
                if ui.selectable_label(console.auto_scroll, auto_label).clicked() {
                    console.auto_scroll = !console.auto_scroll;
                }
                
                ui.separator();
                
                // Filter buttons with titles
                if ui.selectable_label(console.show_error, "✕ Errors").clicked() {
                    console.show_error = !console.show_error;
                }
                if ui.selectable_label(console.show_warning, "⚠ Warnings").clicked() {
                    console.show_warning = !console.show_warning;
                }
                if ui.selectable_label(console.show_info, "○ Info").clicked() {
                    console.show_info = !console.show_info;
                }
                if ui.selectable_label(console.show_system, "⚙ System").clicked() {
                    console.show_system = !console.show_system;
                }
            });
        });
        
        ui.separator();
        
        // Calculate which entries to display (most recent visible_count entries)
        let start_idx = total_logs.saturating_sub(console.visible_count);
        let has_more = console.has_more();
        let hidden_count = console.hidden_count();
        
        // Track if we need to load more
        let mut load_more_clicked = false;
        
        // Use a unique ID for scroll state tracking
        let scroll_id = ui.id().with("output_scroll");
        
        let scroll = egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .stick_to_bottom(console.auto_scroll);
            
        let scroll_output = scroll.show(ui, |ui| {
            ui.style_mut().override_text_style = Some(egui::TextStyle::Monospace);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            
            // Show "Load More" banner at top if there are hidden entries
            if has_more {
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0);
                    let btn_text = format!("⬆ Load {} more ({} hidden) ⬆", 
                        LOAD_MORE_BATCH.min(hidden_count), 
                        hidden_count
                    );
                    if ui.button(btn_text).clicked() {
                        load_more_clicked = true;
                    }
                    ui.add_space(4.0);
                });
                ui.separator();
            }
            
            // Display only visible logs (from start_idx to end)
            for entry in console.logs.iter().skip(start_idx) {
                // Filter by level
                let show = match entry.level {
                    LogLevel::Info => console.show_info,
                    LogLevel::Warning => console.show_warning,
                    LogLevel::Error => console.show_error,
                    LogLevel::System => console.show_system,
                };
                
                if !show {
                    continue;
                }
                
                // Format timestamp as HH:MM:SS (compact)
                let datetime = chrono::DateTime::<chrono::Local>::from(entry.timestamp);
                let timestamp = datetime.format("%H:%M:%S").to_string();
                
                // Color and icon by level
                let (icon, color) = match entry.level {
                    LogLevel::Info => ("i", egui::Color32::from_rgb(100, 200, 255)),
                    LogLevel::Warning => ("!", egui::Color32::YELLOW),
                    LogLevel::Error => ("X", egui::Color32::from_rgb(255, 100, 100)),
                    LogLevel::System => ("*", egui::Color32::from_rgb(150, 150, 150)),
                };
                
                // Strip leading emojis/symbols from message to avoid duplication
                let message = entry.message.trim_start_matches(|c: char| {
                    // Skip common emoji prefixes and variation selectors
                    c == '⚠' || c == '️' || c == '❌' || c == 'ℹ' || c == '🔧' || 
                    c == '📋' || c == '💡' || c == '🎮' || c == '📝' || c == '✅' ||
                    c == '🗑' || c == '⬇' || c == '⬆' || c == '\u{fe0f}' || // variation selector
                    c.is_whitespace()
                });
                
                // Single line with colored message
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(egui::RichText::new(&timestamp).color(egui::Color32::GRAY).small());
                    ui.label(egui::RichText::new(format!("[{}]", icon)).color(color));
                    ui.label(egui::RichText::new(message).color(color));
                    
                    // Show clickable file path if present
                    if let Some(ref path) = entry.file_path {
                        let path_response = ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("📁 {}", path))
                                    .color(egui::Color32::from_rgb(100, 180, 255))
                                    .underline()
                            ).sense(egui::Sense::click())
                        );
                        
                        if path_response.clicked() && ui.input(|i| i.modifiers.ctrl) {
                            // Ctrl+click: Open file in explorer/finder
                            let path_buf = std::path::PathBuf::from(path);
                            if let Some(parent) = path_buf.parent() {
                                #[cfg(target_os = "windows")]
                                { let _ = std::process::Command::new("explorer").arg("/select,").arg(path).spawn(); }
                                #[cfg(target_os = "macos")]
                                { let _ = std::process::Command::new("open").arg("-R").arg(path).spawn(); }
                                #[cfg(target_os = "linux")]
                                { let _ = std::process::Command::new("xdg-open").arg(parent).spawn(); }
                            }
                        }
                        
                        path_response.on_hover_text("Ctrl+click to open in file explorer");
                    }
                });
            }
            
            // If no logs to show
            if console.logs.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.label(egui::RichText::new("No logs yet").color(egui::Color32::GRAY));
                    ui.label(egui::RichText::new("Logs from the engine will appear here").small().color(egui::Color32::DARK_GRAY));
                });
            }
        });
        
        // Auto-load more when scrolled to top
        if has_more {
            let scroll_offset = scroll_output.state.offset.y;
            if scroll_offset < 10.0 {
                // User scrolled to top, load more
                load_more_clicked = true;
            }
        }
        
        // Handle load more after scroll area
        if load_more_clicked {
            console.load_more();
        }
    }
}

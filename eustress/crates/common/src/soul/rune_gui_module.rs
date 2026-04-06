//! # Rune GUI Module — Shared GUI scripting API
//!
//! Provides Rune functions for manipulating GUI elements at runtime.
//! Used by both engine and client via `install()` on their Rune context.
//!
//! Functions:
//! - gui_set_text(name, text)
//! - gui_get_text(name) -> String
//! - gui_set_visible(name, visible)
//! - gui_set_bg_color(name, r, g, b, a)
//! - gui_set_text_color(name, r, g, b, a)
//! - gui_set_border_color(name, r, g, b, a)
//! - gui_set_position(name, x, y)
//! - gui_set_size(name, w, h)
//! - gui_set_font_size(name, size)
//! - log_info(msg), log_warn(msg), log_error(msg)

#[cfg(feature = "realism-scripting")]
use rune::{Module, ContextError};

use crate::gui::{push_gui_command, gui_snapshot_get, GuiCommand};

/// Create a Rune module with GUI + logging functions.
/// Install this alongside any engine/client-specific modules.
#[cfg(feature = "realism-scripting")]
pub fn create_gui_module() -> Result<Module, ContextError> {
    let mut module = Module::with_crate("eustress")?;

    // GUI scripting API
    module.function_meta(gui_set_text)?;
    module.function_meta(gui_get_text)?;
    module.function_meta(gui_set_visible)?;
    module.function_meta(gui_set_bg_color)?;
    module.function_meta(gui_set_text_color)?;
    module.function_meta(gui_set_border_color)?;
    module.function_meta(gui_set_position)?;
    module.function_meta(gui_set_size)?;
    module.function_meta(gui_set_font_size)?;

    // Logging
    module.function_meta(log_info)?;
    module.function_meta(log_warn)?;
    module.function_meta(log_error)?;

    Ok(module)
}

// ── GUI Functions ──────────────────────────────────────────────────────────

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_text(name: &str, text: &str) {
    push_gui_command(GuiCommand::SetText {
        name: name.to_string(),
        text: text.to_string(),
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_get_text(name: &str) -> String {
    gui_snapshot_get(name)
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_visible(name: &str, visible: bool) {
    push_gui_command(GuiCommand::SetVisible {
        name: name.to_string(),
        visible,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_bg_color(name: &str, r: f64, g: f64, b: f64, a: f64) {
    push_gui_command(GuiCommand::SetBgColor {
        name: name.to_string(),
        r: r as f32, g: g as f32, b: b as f32, a: a as f32,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_text_color(name: &str, r: f64, g: f64, b: f64, a: f64) {
    push_gui_command(GuiCommand::SetTextColor {
        name: name.to_string(),
        r: r as f32, g: g as f32, b: b as f32, a: a as f32,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_border_color(name: &str, r: f64, g: f64, b: f64, a: f64) {
    push_gui_command(GuiCommand::SetBorderColor {
        name: name.to_string(),
        r: r as f32, g: g as f32, b: b as f32, a: a as f32,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_position(name: &str, x: f64, y: f64) {
    push_gui_command(GuiCommand::SetPosition {
        name: name.to_string(),
        x: x as f32, y: y as f32,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_size(name: &str, w: f64, h: f64) {
    push_gui_command(GuiCommand::SetSize {
        name: name.to_string(),
        w: w as f32, h: h as f32,
    });
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn gui_set_font_size(name: &str, size: f64) {
    push_gui_command(GuiCommand::SetFontSize {
        name: name.to_string(),
        size: size as f32,
    });
}

// ── Logging ────────────────────────────────────────────────────────────────

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn log_info(msg: &str) {
    tracing::info!("[Rune] {}", msg);
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn log_warn(msg: &str) {
    tracing::warn!("[Rune] {}", msg);
}

#[cfg(feature = "realism-scripting")]
#[rune::function]
fn log_error(msg: &str) {
    tracing::error!("[Rune] {}", msg);
}

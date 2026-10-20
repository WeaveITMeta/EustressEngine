//! # A Play session's shared vocabulary
//!
//! What both apps name while a Space plays: the session's DataModel tree,
//! the four sets a Play frame runs in, the markers the draw step leaves on
//! what it makes, and the part of the window the 3D view fills. Studio's
//! Play and the Player run the same frame over them
//! (`docs/architecture/SHARED_PLAY_RUNTIME.md`).

use bevy::prelude::*;

use crate::datamodel::SharedDataModel;

/// The running session's tree.
#[derive(Resource, Clone)]
pub struct PlayDataModel {
    pub dm: SharedDataModel,
}

/// Systems of one Play frame, in order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlayScriptSet {
    Pull,
    Scripts,
    Apply,
    End,
}

/// Entities spawned for script-made instances. Despawned on Stop.
#[derive(Component, Debug, Clone, Copy)]
pub struct DataModelSpawned;

/// Set while a GUI TextBox holds the keyboard: the avatar and the camera
/// leave the keys to it.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct GuiKeyboardFocus(pub bool);

/// `workspace.Gravity` as scripts read it: the downward magnitude of the
/// live gravity, so a Space set to the Moon reads 1.62. Read through the
/// f32's shortest decimal form, or standard gravity would print as
/// 9.806650161743164, and weightless as 0 rather than -0.
pub fn workspace_gravity(live: Vec3) -> f64 {
    let down = if live.y == 0.0 { 0.0 } else { -live.y };
    down.to_string().parse::<f64>().unwrap_or(down as f64)
}

/// `Players.RespawnTime` as Play starts: seconds from a character's death
/// to its respawn.
pub const RESPAWN_TIME_SECS: f64 = 5.0;

/// The Space's `Players.CharacterAutoLoads`, from `Players/_service.toml`
/// (`character_auto_loads` in `[properties]`, at the top level, or in
/// `[service]`, the first of those winning): on unless the Space turns it
/// off, as for a paddle game played through its camera and input alone.
pub fn space_character_auto_loads(space_root: &std::path::Path) -> bool {
    let Ok(text) = std::fs::read_to_string(space_root.join("Players").join("_service.toml")) else { return true };
    let Ok(doc) = text.parse::<toml::Table>() else { return true };
    let pick = |t: &toml::Table| {
        t.get("character_auto_loads").or_else(|| t.get("CharacterAutoLoads")).and_then(toml::Value::as_bool)
    };
    let section = |name: &str| doc.get(name).and_then(toml::Value::as_table).and_then(|t| pick(t));
    section("properties").or_else(|| pick(&doc)).or_else(|| section("service")).unwrap_or(true)
}

/// Marker component to track parts that had their physics activated during play mode
#[derive(Component)]
pub struct PlayModePhysicsActivated;

/// Viewport bounds reported by Slint layout (in PHYSICAL pixels from top-left).
/// Used by the camera controller to clip 3D rendering to the viewport area.
///
/// IMPORTANT: these fields are physical pixels (logical × scale_factor). When
/// comparing against `Window::cursor_position()` — which returns LOGICAL
/// pixels — call `contains_logical` or divide by the window scale factor
/// first. Forgetting this is the bug that made 3D click selection silently
/// reject every click on any display with DPI scaling ≠ 1.0.
#[derive(Resource, Default, Clone, Copy)]
pub struct ViewportBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl ViewportBounds {
    /// Test whether a cursor point (in LOGICAL pixels, as returned by
    /// `Window::cursor_position()`) falls inside the viewport rectangle.
    /// Converts physical bounds to logical using the provided scale factor.
    pub fn contains_logical(&self, cursor: bevy::math::Vec2, scale_factor: f32) -> bool {
        if self.width <= 0.0 || self.height <= 0.0 { return true; }
        let s = scale_factor.max(0.0001);
        let x = self.x / s;
        let y = self.y / s;
        let w = self.width / s;
        let h = self.height / s;
        cursor.x >= x && cursor.x <= x + w && cursor.y >= y && cursor.y <= y + h
    }
}

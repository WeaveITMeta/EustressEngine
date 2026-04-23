//! # Pivot Modes (Phase 1)
//!
//! Controls where the gizmo pivot lands for multi-selection transforms:
//!
//! - **Median** — AABB center of the selection (default; today's
//!   behaviour).
//! - **Active** — origin of the most-recently-selected entity.
//! - **Individual** — each entity transforms around its own origin
//!   (per-entity pivot).
//! - **Cursor** — user-placed 3D cursor point, persists across tool
//!   switches.
//!
//! ## Scope of v1
//!
//! This module ships the resource, keybinding support, and a helper
//! `resolve_group_pivot` that gizmo render + drag-math consumers can
//! call to pick a pivot point. The gizmo-handle + drag-math systems
//! in [`move_tool`], [`scale_tool`], [`rotate_tool`] are unchanged
//! in v1 — they default to Median via the resource. Integration to
//! Active / Individual / Cursor lands in a follow-up that touches
//! each tool's drag-math path.
//!
//! The Median / Cursor modes are fully usable today — switching pivot
//! to `Cursor` and placing the 3D cursor with Shift-Click will position
//! the gizmo at the cursor on the next frame for visualization. Drag
//! math still operates around group center unless a tool opts into
//! reading the resolved pivot.

use bevy::prelude::*;
use crate::selection_box::Selected;

// ============================================================================
// Resource
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PivotMode {
    Median,
    Active,
    Individual,
    Cursor,
}

impl PivotMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PivotMode::Median     => "median",
            PivotMode::Active     => "active",
            PivotMode::Individual => "individual",
            PivotMode::Cursor     => "cursor",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "median"     => Some(PivotMode::Median),
            "active"     => Some(PivotMode::Active),
            "individual" => Some(PivotMode::Individual),
            "cursor"     => Some(PivotMode::Cursor),
            _ => None,
        }
    }

    /// Cycle to the next mode — used by the `,` / `.` keybindings
    /// (Blender-style pivot cycling).
    pub fn cycle_forward(self) -> Self {
        match self {
            PivotMode::Median     => PivotMode::Active,
            PivotMode::Active     => PivotMode::Individual,
            PivotMode::Individual => PivotMode::Cursor,
            PivotMode::Cursor     => PivotMode::Median,
        }
    }

    pub fn cycle_backward(self) -> Self {
        match self {
            PivotMode::Median     => PivotMode::Cursor,
            PivotMode::Active     => PivotMode::Median,
            PivotMode::Individual => PivotMode::Active,
            PivotMode::Cursor     => PivotMode::Individual,
        }
    }
}

#[derive(Resource, Debug, Clone, Copy)]
pub struct PivotState {
    pub mode: PivotMode,
    /// User-placed 3D cursor. Placed by Shift-Click (or MCP) while
    /// editing; persists across tool switches.
    pub cursor_world: Vec3,
    /// True while the user is holding Shift and clicked in the viewport
    /// to place the cursor — latched one frame then cleared. (Actual
    /// shift-click input handling lives in the part_selection code;
    /// this resource is the store.)
    pub cursor_placed_this_frame: bool,
}

impl Default for PivotState {
    fn default() -> Self {
        Self {
            mode: PivotMode::Median,
            cursor_world: Vec3::ZERO,
            cursor_placed_this_frame: false,
        }
    }
}

// ============================================================================
// Events
// ============================================================================

/// Cycle pivot mode forward (Blender `.`) / backward (`,`).
#[derive(Event, Message, Debug, Clone, Copy)]
pub struct CyclePivotModeEvent { pub forward: bool }

/// Set an explicit pivot mode — from UI picker or MCP.
#[derive(Event, Message, Debug, Clone)]
pub struct SetPivotModeEvent { pub mode: PivotMode }

/// Place the 3D cursor at a world point. Shift-Click in the viewport
/// translates to this event via selection code (follow-up wiring).
#[derive(Event, Message, Debug, Clone, Copy)]
pub struct PlacePivotCursorEvent { pub world_pos: Vec3 }

// ============================================================================
// Plugin
// ============================================================================

pub struct PivotModePlugin;

impl Plugin for PivotModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PivotState>()
            .add_message::<CyclePivotModeEvent>()
            .add_message::<SetPivotModeEvent>()
            .add_message::<PlacePivotCursorEvent>()
            .add_systems(Update, (
                handle_cycle_pivot,
                handle_set_pivot,
                handle_place_cursor,
            ));
    }
}

fn handle_cycle_pivot(
    mut events: MessageReader<CyclePivotModeEvent>,
    mut state: ResMut<PivotState>,
) {
    for event in events.read() {
        state.mode = if event.forward { state.mode.cycle_forward() }
                     else { state.mode.cycle_backward() };
        info!("⊕ Pivot mode: {}", state.mode.as_str());
    }
}

fn handle_set_pivot(
    mut events: MessageReader<SetPivotModeEvent>,
    mut state: ResMut<PivotState>,
) {
    for event in events.read() {
        if state.mode != event.mode {
            state.mode = event.mode;
            info!("⊕ Pivot mode: {}", state.mode.as_str());
        }
    }
}

fn handle_place_cursor(
    mut events: MessageReader<PlacePivotCursorEvent>,
    mut state: ResMut<PivotState>,
) {
    state.cursor_placed_this_frame = false;
    for event in events.read() {
        state.cursor_world = event.world_pos;
        state.cursor_placed_this_frame = true;
        info!("⊕ Pivot cursor placed at {:?}", event.world_pos);
    }
}

// ============================================================================
// Helper — resolve the gizmo pivot for the current selection
// ============================================================================

/// Resolve the group-level pivot point for gizmo rendering + drag math.
/// Individual mode returns the group-center too (each tool handles
/// per-entity pivoting internally when it sees `PivotMode::Individual`).
pub fn resolve_group_pivot(
    state: &PivotState,
    group_center: Vec3,
    active_entity_pos: Option<Vec3>,
) -> Vec3 {
    match state.mode {
        PivotMode::Median     => group_center,
        PivotMode::Active     => active_entity_pos.unwrap_or(group_center),
        PivotMode::Individual => group_center,
        PivotMode::Cursor     => state.cursor_world,
    }
}

/// Convenience: pick the active entity's world position given the
/// selected-entity query, or None if selection is empty.
pub fn active_entity_world_pos(
    selected: &Query<(Entity, &GlobalTransform), With<Selected>>,
) -> Option<Vec3> {
    // "Active" = first iter entry; without a last-selected-tracking
    // system we use iteration order. A follow-up can read the explicit
    // "active" marker from SelectionManager.
    selected.iter().next().map(|(_, gt)| gt.translation())
}

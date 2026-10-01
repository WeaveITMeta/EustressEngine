//! The editor's named actions: what a keyboard shortcut, a menu item and the
//! engine bridge's `action.invoke` run.
//!
//! The type lives below the engine so the two sides that must agree on what a
//! name means share one definition: the engine, which runs an action, and the
//! tool registry's permission gate (`eustress_tools::capability`), which
//! decides whether a caller may run it. Both read a request with
//! [`parse_action`], so the gate always judges exactly the action the engine
//! then runs.

use serde::{Deserialize, Serialize};

/// Actions that can be bound to keys
///
/// No serde attributes (`rename`, `alias`, `rename_all`), on purpose: the
/// variant name is the only spelling that parses, and the permission gate
/// relies on that to read a name the way the engine does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    // Tools
    SelectTool,
    MoveTool,
    RotateTool,
    ScaleTool,
    
    // File
    /// Create a new Space inside the active Universe.
    NewSpace,
    /// Create a new Universe folder.
    NewUniverse,
    /// Open an existing Space / scene through the file picker.
    OpenFile,
    /// Manual save — writes ECS to disk + commits to git as a save point.
    SaveScene,
    /// Save the current Space under a new name.
    SaveSceneAs,
    /// Publish the whole Universe to the Eustress platform.
    PublishUniverse,
    /// Publish only the active Space (incremental update).
    PublishSpace,

    // Edit
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Duplicate,
    Delete,
    SelectAll,
    /// Add the direct children of the current selection (single level).
    SelectChildren,
    /// Recursively add every descendant of the current selection.
    SelectDescendants,
    /// Replace selection with the parent(s) of the current selection.
    SelectParent,
    /// Add siblings sharing the same parent.
    SelectSiblings,
    /// Flip selection to everything NOT currently selected.
    InvertSelection,
    Group,
    Ungroup,
    LockSelection,
    UnlockSelection,
    ToggleAnchor,
    
    // View Panels
    ToggleExplorer,
    ToggleProperties,
    ToggleOutput,
    
    // Windows
    ToggleCommandBar,
    ToggleAssets,
    ToggleCollaboration,
    
    // Transform
    ToggleTransformSpace, // Toggle World/Local space
    
    // Camera
    FocusSelection, // Focus camera on selected part (F key)
    
    // Camera View Modes (Blender-style numpad)
    ViewPerspectiveToggle, // Toggle Perspective/Orthographic (Numpad 5)
    ViewTop,               // Top view (Numpad 8)
    ViewFront,             // Front view (Numpad 2)
    ViewSideLeft,          // Left side view (Numpad 4)
    ViewSideRight,         // Right side view (Numpad 6)
    ViewMode2D,            // 2D view: orthographic, locked to an axis plane (Alt+2)
    ViewMode3D,            // 3D view (Alt+3)
    SaveViewpoint,         // Save the current view as the next "Viewpoint N" (no default chord)
    NextViewpoint,         // Go to the next saved viewpoint (no default chord)

    // Snapping
    SnapMode1,      // 1 unit snapping (1 key)
    SnapMode2,      // 0.2 unit snapping (2 key)
    SnapModeOff,    // No snapping (3 key)
    
    // Vertical placement: `-` lifts by grid unit, `+` is the smart settle
    // (raycast-down flush, or pop-on-top when inside a container). The
    // names say what the key DOES — the old `NudgeUp` / `NudgeDown` pair
    // read as a symmetric up/down nudge, which is not what `+` does.
    LiftSelection,     // Move selection up by one grid unit (- key)
    SettleSelection,   // Smart settle: raycast down + flush OR pop on top (+ key)

    // Quick Rotation
    RotateY90,      // Rotate 90° on Y axis (Ctrl+R)
    TiltZ90,        // Tilt 90° on Z axis (Ctrl+T)
    
    // Network
    StartServer,    // Start local server (F9)
    StopServer,     // Stop server
    ToggleNetworkPanel, // Toggle network panel (Ctrl+Alt+N)

    // Play mode. These exist so F5–F8 go through the same text-focus
    // gate + remapping table as every other shortcut; the behaviour
    // itself lives in `play_mode.rs`, which reads MenuActionEvent.
    PlayWithCharacter,  // F5 — enter play mode with a character
    PauseResume,        // F6 — pause / resume a running play session
    PlaySolo,           // F7 — enter play mode without a character
    StopPlay,           // F8 — stop play mode, restore the editor snapshot
    
    // CSG Operations
    CSGNegate,      // Negate selected part (CSG subtract)
    CSGUnion,       // Union selected parts
    CSGIntersect,   // Intersect selected parts
    CSGSeparate,    // Separate union into parts

    // Smart Build Modal Tools (activate via ModalToolRegistry)
    ToolPartSwap,   // Ctrl+Alt+P — swap two parts' positions
    ToolEdgeAlign,  // Ctrl+Alt+E — translate source to target's edge
    ToolModelReflect, // Ctrl+Alt+M — reflect selection across a plane
    ToolGapFill,    // Ctrl+Alt+G — fill the gap between two parts
    ToolResizeAlign,// Ctrl+Alt+A — resize source until its face meets target's
    ToolMaterialFlip,// Ctrl+Alt+F — flip texture UVs on selected parts

    // Array tools (Phase 1)
    ToolLinearArray, // Ctrl+Alt+L — N copies along a step vector
    ToolRadialArray, // Ctrl+Alt+R — N copies around a pivot axis
    ToolGridArray,   // Ctrl+Alt+K — Nx × Ny × Nz 3D pattern
    // Ctrl+Alt+H — N copies along a clicked polyline. `H` for patH: Ctrl+Alt+P
    // is ToolPartSwap and Ctrl+Alt+G is ToolGapFill, so both obvious letters
    // are taken.
    ToolPathArray,
    // Roblox Studio parity. Shipped as defaults so the muscle memory carries
    // over; the Roblox keymap preset covers the chords that differ.
    /// Ctrl+I: searchable class picker that inserts under the selection.
    InsertObject,
    /// Ctrl+F: find objects in the scene by name, and rename them.
    FindReplace,
    /// Ctrl+Shift+V: paste as children of the primary selection.
    PasteInto,
    /// Ctrl+Shift+X: put the keyboard in the Explorer search box.
    FocusExplorerSearch,
    /// Ctrl+Shift+E: put the keyboard in the Properties filter box.
    FocusPropertiesFilter,
    /// Ribbon toggle (no default chord): dragged parts stop at other parts.
    ToggleCollisions,

    // Terrain tools (docs/design/TERRAIN_TOOLS_UX.md section 6). Every one
    // but `TerrainTools` is in the terrain context (`ActionContext::Terrain`):
    // live only while the terrain tools are the current tool, where it wins
    // over a global binding on the same chord. `TerrainTools` is global, so
    // `T` enters the tools from anywhere.
    /// T: enter the terrain tools (the last tool used), or leave them.
    TerrainTools,
    /// 1: Draw.
    TerrainDraw,
    /// 2: Sculpt.
    TerrainSculpt,
    /// 3: Smooth.
    TerrainSmooth,
    /// 4: Flatten.
    TerrainFlatten,
    /// 5: Paint.
    TerrainPaint,
    /// 6: Sea Level.
    TerrainSeaLevel,
    /// 7: Region.
    TerrainRegion,
    /// [: brush size down.
    TerrainSizeDown,
    /// ]: brush size up.
    TerrainSizeUp,
    /// Shift+[: brush strength down.
    TerrainStrengthDown,
    /// Shift+]: brush strength up.
    TerrainStrengthUp,
    /// ,: previous pivot.
    TerrainPivotPrev,
    /// .: next pivot.
    TerrainPivotNext,
    /// P: plane lock on at the surface under the cursor, or off.
    TerrainPlaneLock,
    /// Shift+P: re-pick the locked plane's height from the surface.
    TerrainPlanePick,
    /// PageUp: locked plane up one grid step.
    TerrainPlaneUp,
    /// PageDown: locked plane down one grid step.
    TerrainPlaneDown,
    /// Shift+PageUp: locked plane up ten grid steps.
    TerrainPlaneUpFast,
    /// Shift+PageDown: locked plane down ten grid steps.
    TerrainPlaneDownFast,
    /// G: snap to grid on or off.
    TerrainSnap,
    /// Shift+G: next grid step.
    TerrainSnapStep,
    /// C: height contours on or off.
    TerrainContours,
    /// M: mirror on or off.
    TerrainMirror,
    /// Shift+M: next mirror axes.
    TerrainMirrorAxis,
    /// I: sample the material under the cursor.
    TerrainSampleMaterial,
    /// Ctrl+C: Region copies its box.
    TerrainRegionCopy,
    /// Ctrl+X: Region copies its box, then empties it.
    TerrainRegionCut,
    /// Ctrl+V: Region pastes the copy where a click places it.
    TerrainRegionPaste,
    /// Ctrl+D: Region copies its box one box width along X.
    TerrainRegionDuplicate,
    /// Delete: Region empties its box.
    TerrainRegionDelete,
}

/// Where an action's chord is live. A chord may be bound once per context:
/// a context's own bindings win over global ones while it is active, so the
/// terrain tools can take `1`-`7` from the snap and view keys without
/// either binding going dead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionContext {
    /// Everywhere (the editor's normal keyboard).
    Global,
    /// While the terrain tools are the current tool.
    Terrain,
}

impl Action {
    /// The context the action's chord is live in.
    pub fn context(self) -> ActionContext {
        if TERRAIN_ACTIONS.contains(&self) {
            ActionContext::Terrain
        } else {
            ActionContext::Global
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Action::SelectTool => "Select Tool",
            Action::MoveTool => "Move Tool",
            Action::RotateTool => "Rotate Tool",
            Action::ScaleTool => "Scale Tool",
            Action::NewSpace => "New Space",
            Action::NewUniverse => "New Universe",
            Action::OpenFile => "Open File",
            Action::SaveScene => "Save Scene",
            Action::SaveSceneAs => "Save Space As",
            Action::PublishUniverse => "Publish Universe",
            Action::PublishSpace => "Publish Space",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::Copy => "Copy",
            Action::Cut => "Cut",
            Action::Paste => "Paste",
            Action::Duplicate => "Duplicate",
            Action::Delete => "Delete",
            Action::SelectAll => "Select All",
            Action::SelectChildren => "Select Children",
            Action::SelectDescendants => "Select Descendants",
            Action::SelectParent => "Select Parent",
            Action::SelectSiblings => "Select Siblings",
            Action::InvertSelection => "Invert Selection",
            Action::Group => "Group",
            Action::Ungroup => "Ungroup",
            Action::LockSelection => "Lock Selection",
            Action::UnlockSelection => "Unlock Selection",
            Action::ToggleAnchor => "Toggle Anchor",
            Action::ToggleExplorer => "Toggle Explorer",
            Action::ToggleProperties => "Toggle Properties",
            Action::ToggleOutput => "Toggle Output",
            Action::ToggleCommandBar => "Toggle Command Bar",
            Action::ToggleAssets => "Toggle Assets",
            Action::ToggleCollaboration => "Toggle Collaboration",
            Action::ToggleTransformSpace => "Toggle Transform Space",
            Action::FocusSelection => "Focus Selection",
            Action::ViewPerspectiveToggle => "Toggle Perspective/Ortho",
            Action::ViewTop => "Top View",
            Action::ViewFront => "Front View",
            Action::ViewSideLeft => "Left Side View",
            Action::ViewSideRight => "Right Side View",
            Action::ViewMode2D => "2D View",
            Action::ViewMode3D => "3D View",
            Action::SaveViewpoint => "Save Viewpoint",
            Action::NextViewpoint => "Next Viewpoint",
            Action::SnapMode1 => "Snap Mode: 1m",
            Action::SnapMode2 => "Snap Mode: 0.2m",
            Action::SnapModeOff => "Snap Mode: Off",
            Action::LiftSelection => "Lift (grid unit)",
            Action::SettleSelection => "Settle onto Surface",
            Action::RotateY90 => "Rotate 90° (Y Axis)",
            Action::TiltZ90 => "Tilt 90° (Z Axis)",
            Action::StartServer => "Start Server",
            Action::StopServer => "Stop Server",
            Action::ToggleNetworkPanel => "Toggle Network Panel",
            Action::PlayWithCharacter => "Play (with Character)",
            Action::PauseResume => "Pause / Resume",
            Action::PlaySolo => "Play Solo",
            Action::StopPlay => "Stop Play",
            Action::CSGNegate => "CSG Negate",
            Action::CSGUnion => "CSG Union",
            Action::CSGIntersect => "CSG Intersect",
            Action::CSGSeparate => "CSG Separate",
            Action::ToolPartSwap => "Part Swap",
            Action::ToolEdgeAlign => "Edge Align",
            Action::ToolModelReflect => "Model Reflect",
            Action::ToolGapFill => "Gap Fill",
            Action::ToolResizeAlign => "Resize Align",
            Action::ToolMaterialFlip => "Material Flip",
            Action::ToolLinearArray => "Linear Array",
            Action::ToolRadialArray => "Radial Array",
            Action::ToolGridArray => "Grid Array",
            Action::ToolPathArray => "Path Array",
            Action::InsertObject => "Insert Object",
            Action::FindReplace => "Find & Replace",
            Action::PasteInto => "Paste Into",
            Action::FocusExplorerSearch => "Search Explorer",
            Action::FocusPropertiesFilter => "Filter Properties",
            Action::ToggleCollisions => "Toggle Collisions",
            Action::TerrainTools => "Terrain Tools",
            Action::TerrainDraw => "Terrain: Draw",
            Action::TerrainSculpt => "Terrain: Sculpt",
            Action::TerrainSmooth => "Terrain: Smooth",
            Action::TerrainFlatten => "Terrain: Flatten",
            Action::TerrainPaint => "Terrain: Paint",
            Action::TerrainSeaLevel => "Terrain: Sea Level",
            Action::TerrainRegion => "Terrain: Region",
            Action::TerrainSizeDown => "Terrain: Brush Size Down",
            Action::TerrainSizeUp => "Terrain: Brush Size Up",
            Action::TerrainStrengthDown => "Terrain: Strength Down",
            Action::TerrainStrengthUp => "Terrain: Strength Up",
            Action::TerrainPivotPrev => "Terrain: Previous Pivot",
            Action::TerrainPivotNext => "Terrain: Next Pivot",
            Action::TerrainPlaneLock => "Terrain: Plane Lock",
            Action::TerrainPlanePick => "Terrain: Pick Plane Height",
            Action::TerrainPlaneUp => "Terrain: Plane Up",
            Action::TerrainPlaneDown => "Terrain: Plane Down",
            Action::TerrainPlaneUpFast => "Terrain: Plane Up x10",
            Action::TerrainPlaneDownFast => "Terrain: Plane Down x10",
            Action::TerrainSnap => "Terrain: Snap to Grid",
            Action::TerrainSnapStep => "Terrain: Next Grid Step",
            Action::TerrainContours => "Terrain: Contours",
            Action::TerrainMirror => "Terrain: Mirror",
            Action::TerrainMirrorAxis => "Terrain: Next Mirror Axis",
            Action::TerrainSampleMaterial => "Terrain: Sample Material",
            Action::TerrainRegionCopy => "Terrain: Region Copy",
            Action::TerrainRegionCut => "Terrain: Region Cut",
            Action::TerrainRegionPaste => "Terrain: Region Paste",
            Action::TerrainRegionDuplicate => "Terrain: Region Duplicate",
            Action::TerrainRegionDelete => "Terrain: Region Delete",
        }
    }
}

/// The terrain-context actions ([`ActionContext::Terrain`]). While the terrain
/// tools are the current tool, the engine tests these before its global
/// shortcuts, so their chords win over global ones on the same keys; the
/// engine's terrain plugin handles them.
pub const TERRAIN_ACTIONS: &[Action] = &[
    Action::TerrainDraw, Action::TerrainSculpt, Action::TerrainSmooth, Action::TerrainFlatten,
    Action::TerrainPaint, Action::TerrainSeaLevel, Action::TerrainRegion,
    Action::TerrainSizeDown, Action::TerrainSizeUp,
    Action::TerrainStrengthDown, Action::TerrainStrengthUp,
    Action::TerrainPivotPrev, Action::TerrainPivotNext,
    Action::TerrainPlaneLock, Action::TerrainPlanePick,
    Action::TerrainPlaneUp, Action::TerrainPlaneDown,
    Action::TerrainPlaneUpFast, Action::TerrainPlaneDownFast,
    Action::TerrainSnap, Action::TerrainSnapStep, Action::TerrainContours,
    Action::TerrainMirror, Action::TerrainMirrorAxis, Action::TerrainSampleMaterial,
    Action::TerrainRegionCopy, Action::TerrainRegionCut, Action::TerrainRegionPaste,
    Action::TerrainRegionDuplicate, Action::TerrainRegionDelete,
];

/// The action a request's `action` field names, read exactly as the engine
/// bridge's `action.invoke` reads it: a string holding a variant name, case
/// and spacing exact. Anything else is `None`.
pub fn parse_action(params: &serde_json::Value) -> Option<Action> {
    let name = params.get("action")?.as_str()?;
    serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_variant_name_parses() {
        assert_eq!(parse_action(&json!({ "action": "ViewMode2D" })), Some(Action::ViewMode2D));
        assert_eq!(parse_action(&json!({ "action": "Delete" })), Some(Action::Delete));
    }

    #[test]
    fn only_the_exact_name_parses() {
        for action in [
            json!("delete"),
            json!("DELETE"),
            json!(" Delete"),
            json!("Delete "),
            json!(""),
            json!(5),
            json!(null),
            json!({ "Delete": null }),
            json!(["Delete"]),
        ] {
            assert_eq!(parse_action(&json!({ "action": action })), None, "{action}");
        }
        assert_eq!(parse_action(&json!({})), None);
        assert_eq!(parse_action(&json!("Delete")), None);
    }

    #[test]
    fn a_repeated_key_reads_as_its_last_value() {
        // serde_json keeps the last of a repeated key, so code that parses a
        // request once and hands the same value on sees one action only.
        let params: serde_json::Value =
            serde_json::from_str(r#"{"action":"ViewMode2D","action":"Delete"}"#).unwrap();
        assert_eq!(parse_action(&params), Some(Action::Delete));
    }

    #[test]
    fn every_terrain_action_is_in_the_terrain_context() {
        for action in TERRAIN_ACTIONS {
            assert_eq!(action.context(), ActionContext::Terrain, "{action:?}");
        }
        assert_eq!(Action::TerrainTools.context(), ActionContext::Global);
    }
}

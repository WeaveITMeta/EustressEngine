use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// The action type lives in `eustress_common::editor_action`, shared with the
// tool registry's permission gate, which has to judge exactly the action the
// engine runs. Re-exported so every `keybindings::Action` path stays put.
pub use eustress_common::editor_action::{Action, ActionContext, TERRAIN_ACTIONS};

/// Key combination with modifiers
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyBinding {
    pub key: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyBinding {
    pub fn new(key: KeyCode) -> Self {
        Self {
            key,
            ctrl: false,
            alt: false,
            shift: false,
        }
    }
    
    pub fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }
    
    pub fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }
    
    pub fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }
    
    pub fn matches(&self, keys: &ButtonInput<KeyCode>) -> bool {
        // Check modifiers
        let ctrl_pressed = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let alt_pressed = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
        let shift_pressed = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        
        if self.ctrl != ctrl_pressed || self.alt != alt_pressed || self.shift != shift_pressed {
            return false;
        }
        
        // Check key
        keys.just_pressed(self.key)
    }
    
    pub fn to_string_rep(&self) -> String {
        let mut parts = Vec::new();

        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }

        parts.push(format!("{:?}", self.key));

        parts.join("+")
    }

    /// User-facing display string — strips the `Key` / `Digit` enum-variant
    /// prefixes produced by `{:?}` on KeyCode so users see "Ctrl+Z" instead
    /// of "Ctrl+KeyZ". Used by the ribbon to render subtitle shortcuts.
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl  { parts.push("Ctrl".to_string()); }
        if self.alt   { parts.push("Alt".to_string()); }
        if self.shift { parts.push("Shift".to_string()); }
        parts.push(key_display(&self.key));
        parts.join("+")
    }
}

/// Map a Bevy [`bevy::input::keyboard::KeyCode`] to a short, user-readable
/// string. Handles the common letter/digit/arrow/function cases; falls back
/// to `{:?}` for anything exotic (media keys, IMEs, etc.).
fn key_display(key: &bevy::input::keyboard::KeyCode) -> String {
    use bevy::input::keyboard::KeyCode;
    let raw = format!("{:?}", key);
    // KeyA..KeyZ → A..Z
    if let Some(rest) = raw.strip_prefix("Key") {
        if rest.len() == 1 {
            return rest.to_string();
        }
    }
    // Digit0..Digit9 → 0..9
    if let Some(rest) = raw.strip_prefix("Digit") {
        return rest.to_string();
    }
    // Function keys stay as F1..F24
    // Arrows → ↑ ↓ ← → for compactness
    match key {
        KeyCode::ArrowUp    => "↑".to_string(),
        KeyCode::ArrowDown  => "↓".to_string(),
        KeyCode::ArrowLeft  => "←".to_string(),
        KeyCode::ArrowRight => "→".to_string(),
        KeyCode::Space      => "Space".to_string(),
        KeyCode::Escape     => "Esc".to_string(),
        KeyCode::Enter      => "Enter".to_string(),
        KeyCode::Backspace  => "Backspace".to_string(),
        KeyCode::Delete     => "Del".to_string(),
        KeyCode::Tab        => "Tab".to_string(),
        _ => raw,
    }
}

/// Resource for managing keybindings
///
/// Each action has exactly ONE *primary* binding (what the ribbon and the
/// remap dialog display) plus any number of *alternates* that also fire it.
/// The split matters because [`KeyBinding::matches`] requires exact modifier
/// equality — a single `Ctrl+Y` entry for Redo can never also answer to
/// `Ctrl+Shift+Z`, so the second convention has to be a real extra entry
/// rather than a looser match. Keeping alternates in their own map preserves
/// `get()`'s `Option<&KeyBinding>` signature for the Slint ribbon sync
/// (`slint_ui::sync_keybindings_to_slint`), which renders exactly one
/// shortcut string per button.
#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct KeyBindings {
    bindings: HashMap<Action, KeyBinding>,
    /// Extra chords that fire the same action. `#[serde(default)]` so a
    /// `keybindings.ron` written before alternates existed still loads.
    #[serde(default)]
    alternates: HashMap<Action, Vec<KeyBinding>>,
    /// Which [`KeymapPreset`] the map was last set from ("custom" once any
    /// chord is rebound by hand). Shown in the Keyboard Shortcuts dialog.
    #[serde(default)]
    preset: String,
}

impl Default for KeyBindings {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        let mut alternates: HashMap<Action, Vec<KeyBinding>> = HashMap::new();

        // Tool shortcuts (Alt-based to avoid text input conflicts)
        bindings.insert(Action::SelectTool, KeyBinding::new(KeyCode::KeyZ).with_alt());
        bindings.insert(Action::MoveTool, KeyBinding::new(KeyCode::KeyX).with_alt());
        bindings.insert(Action::ScaleTool, KeyBinding::new(KeyCode::KeyC).with_alt());
        bindings.insert(Action::RotateTool, KeyBinding::new(KeyCode::KeyV).with_alt());
        
        // File shortcuts — Ctrl+S writes ECS to disk + creates a git
        // commit so the user has an explicit save-point boundary on
        // top of the timer-driven autosave. The rest match the chords
        // the ribbon's File dropdown already prints next to each item
        // (ribbon.slint); they must agree or the menu lies.
        bindings.insert(Action::NewSpace, KeyBinding::new(KeyCode::KeyN).with_ctrl());
        bindings.insert(Action::NewUniverse, KeyBinding::new(KeyCode::KeyN).with_ctrl().with_shift());
        bindings.insert(Action::OpenFile, KeyBinding::new(KeyCode::KeyO).with_ctrl());
        bindings.insert(Action::SaveScene, KeyBinding::new(KeyCode::KeyS).with_ctrl());
        bindings.insert(Action::SaveSceneAs, KeyBinding::new(KeyCode::KeyS).with_ctrl().with_shift());
        bindings.insert(Action::PublishUniverse, KeyBinding::new(KeyCode::KeyP).with_ctrl());
        bindings.insert(Action::PublishSpace, KeyBinding::new(KeyCode::KeyP).with_ctrl().with_shift());

        // Edit shortcuts
        bindings.insert(Action::Undo, KeyBinding::new(KeyCode::KeyZ).with_ctrl());
        bindings.insert(Action::Redo, KeyBinding::new(KeyCode::KeyY).with_ctrl());
        // Ctrl+Shift+Z is the other half of the redo convention (macOS,
        // Photoshop, most browsers). `matches` compares modifiers for
        // exact equality, so this has to be its own entry — without it
        // Ctrl+Shift+Z was a silent no-op.
        alternates.entry(Action::Redo).or_default()
            .push(KeyBinding::new(KeyCode::KeyZ).with_ctrl().with_shift());
        bindings.insert(Action::Copy, KeyBinding::new(KeyCode::KeyC).with_ctrl());
        bindings.insert(Action::Cut, KeyBinding::new(KeyCode::KeyX).with_ctrl());
        bindings.insert(Action::Paste, KeyBinding::new(KeyCode::KeyV).with_ctrl());
        bindings.insert(Action::Duplicate, KeyBinding::new(KeyCode::KeyD).with_ctrl());
        bindings.insert(Action::Delete, KeyBinding::new(KeyCode::Delete));
        bindings.insert(Action::SelectAll, KeyBinding::new(KeyCode::KeyA).with_ctrl());
        // Hierarchy selection (Maya / Blender parity):
        //   Ctrl+Shift+V → Select Children (one level)
        //   Ctrl+Shift+D → Select Descendants (recursive)
        //   Ctrl+Shift+U → Select Parent (up one level)
        //   Ctrl+Shift+A → Select Siblings — reads as "select all siblings"
        //     next to Ctrl+A = Select All. It used to be Ctrl+Shift+S,
        //     which stole the conventional Save-As chord that the File
        //     menu prints next to "Save Space As...". A File shortcut the
        //     menu advertises always wins over a selection convenience.
        //   Ctrl+I       → Invert Selection
        // Ctrl+Alt is the PRIMARY namespace for the hierarchy commands: Ctrl+Shift+V
        // and Ctrl+Shift+D collide with the near-universal "paste special" and
        // "duplicate" chords, and the primary is what every menu renders. The old
        // Ctrl+Shift chords stay on as alternates so existing muscle memory keeps
        // working.
        bindings.insert(Action::SelectChildren, KeyBinding::new(KeyCode::KeyV).with_ctrl().with_alt());
        // Ctrl+Shift+V is Paste Into (Roblox Studio's chord), not a second
        // Select Children.
        bindings.insert(Action::SelectDescendants, KeyBinding::new(KeyCode::KeyD).with_ctrl().with_alt());
        alternates.entry(Action::SelectDescendants).or_default()
            .push(KeyBinding::new(KeyCode::KeyD).with_ctrl().with_shift());
        bindings.insert(Action::SelectParent, KeyBinding::new(KeyCode::KeyU).with_ctrl().with_shift());
        bindings.insert(Action::SelectSiblings, KeyBinding::new(KeyCode::KeyA).with_ctrl().with_shift());
        // Ctrl+I is Insert Object in Roblox Studio; Invert moves to Ctrl+Shift+I.
        bindings.insert(Action::InvertSelection, KeyBinding::new(KeyCode::KeyI).with_ctrl().with_shift());
        bindings.insert(Action::InsertObject, KeyBinding::new(KeyCode::KeyI).with_ctrl());
        bindings.insert(Action::FindReplace, KeyBinding::new(KeyCode::KeyF).with_ctrl());
        bindings.insert(Action::PasteInto, KeyBinding::new(KeyCode::KeyV).with_ctrl().with_shift());
        bindings.insert(Action::FocusExplorerSearch, KeyBinding::new(KeyCode::KeyX).with_ctrl().with_shift());
        // Roblox uses Ctrl+Shift+P here, which Eustress gives to Publish Space;
        // the Roblox preset swaps them.
        bindings.insert(Action::FocusPropertiesFilter, KeyBinding::new(KeyCode::KeyE).with_ctrl().with_shift());
        bindings.insert(Action::Group, KeyBinding::new(KeyCode::KeyG).with_ctrl());
        bindings.insert(Action::Ungroup, KeyBinding::new(KeyCode::KeyU).with_ctrl());

        // Object flags. Alt+A is Roblox's anchor chord, so it comes for
        // free to anyone arriving from there; Alt+L / Alt+Shift+L extend
        // the same bare-Alt namespace the tool-switch keys use. All three
        // were enum variants with no binding at all, which meant
        // `bindings.check()` returned false forever and the Anchor /
        // Lock / Unlock ribbon buttons had no keyboard route.
        bindings.insert(Action::ToggleAnchor, KeyBinding::new(KeyCode::KeyA).with_alt());
        bindings.insert(Action::LockSelection, KeyBinding::new(KeyCode::KeyL).with_alt());
        bindings.insert(Action::UnlockSelection, KeyBinding::new(KeyCode::KeyL).with_alt().with_shift());

        // Smart Build Tools — activate modal tools via the registry.
        // Ctrl+Alt for a distinct namespace from tool-switch shortcuts
        // (which are bare Alt+Letter for Select/Move/Scale/Rotate).
        bindings.insert(Action::ToolPartSwap, KeyBinding::new(KeyCode::KeyP).with_ctrl().with_alt());
        bindings.insert(Action::ToolEdgeAlign, KeyBinding::new(KeyCode::KeyE).with_ctrl().with_alt());
        bindings.insert(Action::ToolModelReflect, KeyBinding::new(KeyCode::KeyM).with_ctrl().with_alt());
        bindings.insert(Action::ToolGapFill, KeyBinding::new(KeyCode::KeyG).with_ctrl().with_alt());
        bindings.insert(Action::ToolResizeAlign, KeyBinding::new(KeyCode::KeyA).with_ctrl().with_alt());
        bindings.insert(Action::ToolMaterialFlip, KeyBinding::new(KeyCode::KeyF).with_ctrl().with_alt());
        bindings.insert(Action::ToolLinearArray, KeyBinding::new(KeyCode::KeyL).with_ctrl().with_alt());
        bindings.insert(Action::ToolRadialArray, KeyBinding::new(KeyCode::KeyR).with_ctrl().with_alt());
        bindings.insert(Action::ToolGridArray,   KeyBinding::new(KeyCode::KeyK).with_ctrl().with_alt());
        bindings.insert(Action::ToolPathArray,   KeyBinding::new(KeyCode::KeyH).with_ctrl().with_alt());
        
        // View shortcuts
        bindings.insert(Action::ToggleExplorer, KeyBinding::new(KeyCode::Digit1).with_ctrl());
        bindings.insert(Action::ToggleProperties, KeyBinding::new(KeyCode::Digit2).with_ctrl());
        bindings.insert(Action::ToggleOutput, KeyBinding::new(KeyCode::Digit3).with_ctrl());
        
        // Window shortcuts
        bindings.insert(Action::ToggleCommandBar, KeyBinding::new(KeyCode::KeyK).with_ctrl());
        bindings.insert(Action::ToggleAssets, KeyBinding::new(KeyCode::KeyF).with_ctrl().with_shift()); // Changed from A to avoid conflict
        bindings.insert(Action::ToggleCollaboration, KeyBinding::new(KeyCode::KeyL).with_ctrl().with_shift()); // Changed from C to avoid conflict
        
        // Transform shortcuts
        bindings.insert(Action::ToggleTransformSpace, KeyBinding::new(KeyCode::KeyL).with_ctrl()); // Ctrl+L for World/Local space toggle
        
        // Camera shortcuts
        bindings.insert(Action::FocusSelection, KeyBinding::new(KeyCode::KeyF)); // F to focus on selection
        
        // Camera View Mode shortcuts (Blender-style numpad)
        bindings.insert(Action::ViewPerspectiveToggle, KeyBinding::new(KeyCode::Numpad5)); // Numpad 5 toggles perspective/ortho
        bindings.insert(Action::ViewTop, KeyBinding::new(KeyCode::Numpad8));               // Numpad 8 for top view
        bindings.insert(Action::ViewFront, KeyBinding::new(KeyCode::Numpad2));             // Numpad 2 for front view
        bindings.insert(Action::ViewSideLeft, KeyBinding::new(KeyCode::Numpad4));          // Numpad 4 for left side view
        bindings.insert(Action::ViewSideRight, KeyBinding::new(KeyCode::Numpad6));         // Numpad 6 for right side view
        // Top-row 5 as well: most keyboards have no numpad.
        alternates.entry(Action::ViewPerspectiveToggle).or_default()
            .push(KeyBinding::new(KeyCode::Digit5));
        // 2D / 3D. Alt+digit is free (bare 1/2/3 are the snap modes, Ctrl+1/2/3
        // the panels) and reads as the mode it enters.
        bindings.insert(Action::ViewMode2D, KeyBinding::new(KeyCode::Digit2).with_alt());
        bindings.insert(Action::ViewMode3D, KeyBinding::new(KeyCode::Digit3).with_alt());
        
        // Snapping shortcuts
        bindings.insert(Action::SnapMode1, KeyBinding::new(KeyCode::Digit1));    // 1 for 1 unit snapping
        bindings.insert(Action::SnapMode2, KeyBinding::new(KeyCode::Digit2));    // 2 for 0.2 unit snapping
        bindings.insert(Action::SnapModeOff, KeyBinding::new(KeyCode::Digit3));  // 3 for no snapping
        bindings.insert(Action::LiftSelection,   KeyBinding::new(KeyCode::Minus));  // - key
        bindings.insert(Action::SettleSelection, KeyBinding::new(KeyCode::Equal));  // +/= key

        // Quick rotation shortcuts
        bindings.insert(Action::RotateY90, KeyBinding::new(KeyCode::KeyR).with_ctrl()); // Ctrl+R to rotate 90° on Y
        bindings.insert(Action::TiltZ90, KeyBinding::new(KeyCode::KeyT).with_ctrl());   // Ctrl+T to tilt 90° on Z

        // Network shortcuts. The panel toggle moved off Ctrl+Shift+N —
        // that chord belongs to File ▸ New Universe, which the ribbon has
        // always printed next to the item. Ctrl+Alt+N joins the
        // Ctrl+Alt+<letter> namespace the Smart Build Tools already use
        // and collides with none of them.
        bindings.insert(Action::StartServer, KeyBinding::new(KeyCode::F9)); // F9 to start server
        bindings.insert(Action::ToggleNetworkPanel, KeyBinding::new(KeyCode::KeyN).with_ctrl().with_alt()); // Ctrl+Alt+N

        // Play controls. These are the same F5–F8 keys `play_mode.rs`
        // has always answered to; routing them through the binding table
        // is what gives them the text-focus gate (typing "F5" into a
        // property field must not launch the game) and makes them
        // remappable like everything else.
        bindings.insert(Action::PlayWithCharacter, KeyBinding::new(KeyCode::F5));
        bindings.insert(Action::PauseResume, KeyBinding::new(KeyCode::F6));
        bindings.insert(Action::PlaySolo, KeyBinding::new(KeyCode::F7));
        bindings.insert(Action::StopPlay, KeyBinding::new(KeyCode::F8));

        // Terrain tools (docs/design/TERRAIN_TOOLS_UX.md section 6). `T` is
        // global and free (Ctrl+T is TiltZ90; exact modifiers keep them
        // apart). The rest are terrain-context chords: while the terrain
        // tools are the current tool they win over the global snap (1-3),
        // perspective (5) and nudge keys on the same chords.
        bindings.insert(Action::TerrainTools, KeyBinding::new(KeyCode::KeyT));
        bindings.insert(Action::TerrainDraw, KeyBinding::new(KeyCode::Digit1));
        bindings.insert(Action::TerrainSculpt, KeyBinding::new(KeyCode::Digit2));
        bindings.insert(Action::TerrainSmooth, KeyBinding::new(KeyCode::Digit3));
        bindings.insert(Action::TerrainFlatten, KeyBinding::new(KeyCode::Digit4));
        bindings.insert(Action::TerrainPaint, KeyBinding::new(KeyCode::Digit5));
        bindings.insert(Action::TerrainSeaLevel, KeyBinding::new(KeyCode::Digit6));
        bindings.insert(Action::TerrainRegion, KeyBinding::new(KeyCode::Digit7));
        bindings.insert(Action::TerrainSizeDown, KeyBinding::new(KeyCode::BracketLeft));
        bindings.insert(Action::TerrainSizeUp, KeyBinding::new(KeyCode::BracketRight));
        bindings.insert(Action::TerrainStrengthDown, KeyBinding::new(KeyCode::BracketLeft).with_shift());
        bindings.insert(Action::TerrainStrengthUp, KeyBinding::new(KeyCode::BracketRight).with_shift());
        bindings.insert(Action::TerrainPivotPrev, KeyBinding::new(KeyCode::Comma));
        bindings.insert(Action::TerrainPivotNext, KeyBinding::new(KeyCode::Period));
        bindings.insert(Action::TerrainPlaneLock, KeyBinding::new(KeyCode::KeyP));
        bindings.insert(Action::TerrainPlanePick, KeyBinding::new(KeyCode::KeyP).with_shift());
        bindings.insert(Action::TerrainPlaneUp, KeyBinding::new(KeyCode::PageUp));
        bindings.insert(Action::TerrainPlaneDown, KeyBinding::new(KeyCode::PageDown));
        bindings.insert(Action::TerrainPlaneUpFast, KeyBinding::new(KeyCode::PageUp).with_shift());
        bindings.insert(Action::TerrainPlaneDownFast, KeyBinding::new(KeyCode::PageDown).with_shift());
        bindings.insert(Action::TerrainSnap, KeyBinding::new(KeyCode::KeyG));
        bindings.insert(Action::TerrainSnapStep, KeyBinding::new(KeyCode::KeyG).with_shift());
        bindings.insert(Action::TerrainContours, KeyBinding::new(KeyCode::KeyC));
        bindings.insert(Action::TerrainMirror, KeyBinding::new(KeyCode::KeyM));
        bindings.insert(Action::TerrainMirrorAxis, KeyBinding::new(KeyCode::KeyM).with_shift());
        bindings.insert(Action::TerrainSampleMaterial, KeyBinding::new(KeyCode::KeyI));
        // Region's edits on the part-editing chords: in the terrain context
        // they are the terrain's, so a selected part is never copied or
        // deleted from under the terrain tools.
        bindings.insert(Action::TerrainRegionCopy, KeyBinding::new(KeyCode::KeyC).with_ctrl());
        bindings.insert(Action::TerrainRegionCut, KeyBinding::new(KeyCode::KeyX).with_ctrl());
        bindings.insert(Action::TerrainRegionPaste, KeyBinding::new(KeyCode::KeyV).with_ctrl());
        bindings.insert(Action::TerrainRegionDuplicate, KeyBinding::new(KeyCode::KeyD).with_ctrl());
        bindings.insert(Action::TerrainRegionDelete, KeyBinding::new(KeyCode::Delete));

        Self { bindings, alternates, preset: KeymapPreset::Eustress.id().to_string() }
    }
}

/// Legacy on-disk location: a bare relative filename, so the file landed
/// in whatever directory the binary happened to be launched from
/// (`target/debug/`, the desktop shortcut's working dir, …). Kept only as
/// a read-time fallback in [`KeyBindings::load`] so users who already have
/// one don't silently lose their remaps on upgrade.
/// A whole keymap the Keyboard Shortcuts dialog applies in one click.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeymapPreset {
    /// The Eustress defaults.
    Eustress,
    /// The Eustress defaults with Roblox Studio's chords where they differ:
    /// Shift+1..4 pick the tools, F8 runs, Shift+F5 stops, Ctrl+Shift+P
    /// filters Properties (Publish Space moves to Ctrl+Alt+Shift+P).
    Roblox,
}

impl KeymapPreset {
    pub const ALL: [KeymapPreset; 2] = [KeymapPreset::Eustress, KeymapPreset::Roblox];

    pub fn id(self) -> &'static str {
        match self {
            KeymapPreset::Eustress => "eustress",
            KeymapPreset::Roblox => "roblox",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The map this preset stands for.
    pub fn bindings(self) -> KeyBindings {
        let mut map = KeyBindings::default();
        if self == KeymapPreset::Roblox {
            map.bindings.insert(Action::SelectTool, KeyBinding::new(KeyCode::Digit1).with_shift());
            map.bindings.insert(Action::MoveTool, KeyBinding::new(KeyCode::Digit2).with_shift());
            map.bindings.insert(Action::ScaleTool, KeyBinding::new(KeyCode::Digit3).with_shift());
            map.bindings.insert(Action::RotateTool, KeyBinding::new(KeyCode::Digit4).with_shift());
            map.bindings.insert(Action::PlaySolo, KeyBinding::new(KeyCode::F8));
            map.bindings.insert(Action::StopPlay, KeyBinding::new(KeyCode::F5).with_shift());
            map.bindings.insert(Action::PublishSpace, KeyBinding::new(KeyCode::KeyP).with_ctrl().with_alt().with_shift());
            map.bindings.insert(Action::FocusPropertiesFilter, KeyBinding::new(KeyCode::KeyP).with_ctrl().with_shift());
        }
        map.preset = self.id().to_string();
        map
    }
}

impl KeyBindings {
    /// The preset this map was last set from, or "custom".
    pub fn preset(&self) -> &str {
        &self.preset
    }

    /// Replace the whole map with `preset` and persist it.
    pub fn apply_preset(&mut self, preset: KeymapPreset) -> Result<(), String> {
        *self = preset.bindings();
        self.save()
            .map_err(|e| format!("Could not save keybindings: {}", e))
    }

    /// Every chord in the map, primary and alternate, with its owner.
    fn all_chords(&self) -> Vec<(Action, KeyBinding)> {
        let mut out: Vec<(Action, KeyBinding)> =
            self.bindings.iter().map(|(a, b)| (*a, b.clone())).collect();
        for (a, alts) in &self.alternates {
            out.extend(alts.iter().map(|b| (*a, b.clone())));
        }
        out
    }
}

const LEGACY_BINDINGS_FILE: &str = "keybindings.ron";

impl KeyBindings {
    /// Absolute per-user config path: `~/.eustress_engine/keybindings.ron`.
    ///
    /// Same directory `EditorSettings::settings_path` resolves — one
    /// per-user config location for the whole editor, not a second
    /// convention. `None` only when the platform has no home directory,
    /// in which case we fall back to the process-relative legacy path.
    fn config_path() -> Option<std::path::PathBuf> {
        Some(dirs::home_dir()?.join(".eustress_engine").join("keybindings.ron"))
    }

    /// The action's *primary* binding — the one the ribbon renders and the
    /// remap dialog edits. Alternates are deliberately not visible here;
    /// see [`KeyBindings::alternates`].
    pub fn get(&self, action: Action) -> Option<&KeyBinding> {
        self.bindings.get(&action)
    }

    /// Extra chords that also fire `action` (e.g. `Ctrl+Shift+Z` for Redo).
    /// Empty for almost every action.
    pub fn alternates(&self, action: Action) -> &[KeyBinding] {
        self.alternates.get(&action).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn get_string(&self, action: Action) -> String {
        self.get(action)
            .map(|kb| kb.to_string_rep())
            .unwrap_or_else(|| "Not bound".to_string())
    }

    pub fn set(&mut self, action: Action, binding: KeyBinding) {
        self.bindings.insert(action, binding);
    }

    /// The action currently claiming `binding` (primary OR alternate),
    /// ignoring `except`. Used by [`KeyBindings::rebind`] to refuse a
    /// remap that would make two actions fire off one chord — with
    /// [`KeyBinding::matches`] demanding exact modifier equality, the
    /// loser of such a clash is simply dead, silently.
    ///
    /// Only actions of `except`'s own context count: a chord may be bound
    /// once per [`ActionContext`], and the context's binding wins while it
    /// is active.
    fn claimant(&self, binding: &KeyBinding, except: Action) -> Option<Action> {
        let context = except.context();
        self.bindings
            .iter()
            .find(|(a, b)| **a != except && a.context() == context && *b == binding)
            .map(|(a, _)| *a)
            .or_else(|| {
                self.alternates
                    .iter()
                    .find(|(a, v)| **a != except && a.context() == context && v.contains(binding))
                    .map(|(a, _)| *a)
            })
    }

    /// Point `action`'s primary binding at `binding` and persist.
    ///
    /// Rejects a chord another action already owns, naming that action so
    /// the settings dialog can say "Ctrl+G is already Group" rather than
    /// leaving the user with two buttons and one working key. Rebinding an
    /// action to a chord it already owns is a no-op success.
    pub fn rebind(&mut self, action: Action, binding: KeyBinding) -> Result<(), String> {
        if let Some(owner) = self.claimant(&binding, action) {
            return Err(format!(
                "{} is already bound to \"{}\"",
                binding.display(),
                owner.name(),
            ));
        }
        self.bindings.insert(action, binding);
        self.preset = "custom".to_string();
        self.save()
            .map_err(|e| format!("Could not save keybindings: {}", e))
    }

    /// True when `action`'s primary binding OR any of its alternates is the
    /// chord pressed this frame.
    pub fn check(&self, action: Action, keys: &ButtonInput<KeyCode>) -> bool {
        if self.get(action).map(|binding| binding.matches(keys)).unwrap_or(false) {
            return true;
        }
        self.alternates(action).iter().any(|binding| binding.matches(keys))
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let serialized = ron::to_string(&self)?;
        let path = Self::config_path()
            .unwrap_or_else(|| std::path::PathBuf::from(LEGACY_BINDINGS_FILE));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serialized)?;
        Ok(())
    }

    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        // Per-user config first; fall back to the legacy process-relative
        // file so an existing install keeps its remaps. The legacy file is
        // left in place — reading it is enough, and deleting a user's data
        // to tidy up a path migration is not our call.
        let contents = Self::config_path()
            .filter(|p| p.exists())
            .map(std::fs::read_to_string)
            .unwrap_or_else(|| std::fs::read_to_string(LEGACY_BINDINGS_FILE))?;
        let mut bindings: Self = ron::from_str(&contents)?;

        // Merge in any action the saved file predates. Without this, every
        // new shortcut ships dead for existing users — their `keybindings.ron`
        // is authoritative for the whole map, so an action added after they
        // last saved has no entry and `check()` returns false forever. The
        // saved file still wins for anything it does define, so real remaps
        // survive.
        let defaults = Self::default();
        for (action, binding) in defaults.bindings {
            bindings.bindings.entry(action).or_insert(binding);
        }
        for (action, alts) in defaults.alternates {
            bindings.alternates.entry(action).or_insert(alts);
        }
        Ok(bindings)
    }
}

/// Plugin for keybindings system
pub struct KeyBindingsPlugin;

impl Plugin for KeyBindingsPlugin {
    fn build(&self, app: &mut App) {
        // Try to load saved bindings, otherwise use defaults
        let bindings = KeyBindings::load().unwrap_or_default();
        app.insert_resource(bindings)
            .init_resource::<NudgeTimer>()
            .add_systems(Update, (
                dispatch_keyboard_shortcuts
                    .after(crate::ui::slint_ui::update_slint_ui_focus),
                handle_menu_action_events.after(dispatch_keyboard_shortcuts),
                handle_nudge_keys
                    .after(crate::ui::slint_ui::update_slint_ui_focus)
                    .run_if(crate::play_mode::editor_input_enabled),
            ));
    }
}

// ============================================================================
// Keyboard Shortcut Dispatch System
// ============================================================================

/// Every action [`dispatch_keyboard_shortcuts`] tests each frame, in the
/// order it tests them (first match wins and returns).
///
/// Hoisted out of the function body so `every_dispatched_action_is_bound`
/// can walk the real list. An action that reaches this array with no
/// default binding is dead code wearing a ribbon button — `ToggleAnchor`
/// sat here unbound and unhandled for months, and nothing caught it.
const DISPATCHED_ACTIONS: &[Action] = &[
    Action::NewSpace, Action::NewUniverse, Action::OpenFile,
    Action::SaveScene, Action::SaveSceneAs,
    Action::PublishUniverse, Action::PublishSpace,
    Action::Undo, Action::Redo,
    Action::Copy, Action::Cut, Action::Paste, Action::Duplicate, Action::Delete,
    Action::SelectAll,
    Action::SelectChildren, Action::SelectDescendants,
    Action::SelectParent, Action::SelectSiblings,
    Action::InvertSelection,
    Action::Group, Action::Ungroup,
    Action::LockSelection, Action::UnlockSelection, Action::ToggleAnchor,
    Action::ToggleExplorer, Action::ToggleProperties, Action::ToggleOutput,
    Action::ToggleCommandBar, Action::ToggleAssets, Action::ToggleCollaboration,
    Action::ToggleTransformSpace,
    Action::FocusSelection,
    Action::ViewPerspectiveToggle, Action::ViewTop, Action::ViewFront,
    Action::ViewSideLeft, Action::ViewSideRight,
    Action::ViewMode2D, Action::ViewMode3D,
    Action::SaveViewpoint, Action::NextViewpoint,
    Action::SnapMode1, Action::SnapMode2, Action::SnapModeOff,
    Action::LiftSelection, Action::SettleSelection,
    Action::RotateY90, Action::TiltZ90,
    Action::StartServer, Action::ToggleNetworkPanel,
    Action::PlayWithCharacter, Action::PauseResume, Action::PlaySolo, Action::StopPlay,
    Action::CSGNegate, Action::CSGUnion, Action::CSGIntersect, Action::CSGSeparate,
    Action::ToolPartSwap, Action::ToolEdgeAlign, Action::ToolModelReflect, Action::ToolGapFill,
    Action::ToolResizeAlign, Action::ToolMaterialFlip,
    Action::ToolLinearArray, Action::ToolRadialArray, Action::ToolGridArray,
    Action::ToolPathArray,
    Action::InsertObject, Action::PasteInto, Action::FindReplace,
    Action::FocusExplorerSearch, Action::FocusPropertiesFilter,
    Action::TerrainTools,
];

/// The shortcuts that stay live during a Play session: the session controls
/// and the panel toggles. Everything else in [`DISPATCHED_ACTIONS`] edits or
/// persists the scene, so it waits for Stop, and its keys belong to the
/// running game: F, Delete, the tool keys and the rest reach the game's
/// input unprocessed, as in Roblox Studio (`play_allowlist_tests`).
const PLAY_SESSION_ACTIONS: &[Action] = &[
    Action::PlayWithCharacter, Action::PauseResume, Action::PlaySolo, Action::StopPlay,
    Action::ToggleExplorer, Action::ToggleProperties, Action::ToggleOutput,
    Action::ToggleCommandBar,
];

/// Actions that are dispatched but deliberately have NO default chord.
///
/// Being on this list is a reviewed decision, not an oversight — the test
/// `every_dispatched_action_is_bound` fails for anything unbound that is
/// not listed here, which is exactly the check `ToggleAnchor` needed and
/// did not have.
///
/// The four CSG operations reach the dispatch loop only so the ribbon's
/// Boolean group and `csg::CsgPlugin` share one event type; they are
/// mouse-driven, multi-step operations with no accepted keyboard
/// convention, and inventing one would burn a chord for a button that is
/// always visible anyway.
///
/// Read only by that test — it is documentation with teeth, not runtime
/// data, hence the `dead_code` allowance in non-test builds.
#[allow(dead_code)]
const ALLOWED_UNBOUND: &[Action] = &[
    Action::CSGNegate,
    Action::CSGUnion,
    Action::CSGIntersect,
    Action::CSGSeparate,
    // A ribbon toggle; reachable over the bridge and rebindable, no chord by default.
    Action::ToggleCollisions,
    // In the View menu and over the bridge; rebindable, no chord by default:
    // every free single chord near the view keys is already taken.
    Action::SaveViewpoint,
    Action::NextViewpoint,
];

/// Reads keyboard input each frame and dispatches tool changes + MenuActionEvents.
/// Uses Option<ResMut> to avoid silent skip from error handler when resources are missing.
fn dispatch_keyboard_shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Option<Res<KeyBindings>>,
    studio_state: Option<ResMut<crate::ui::StudioState>>,
    mut menu_events: MessageWriter<crate::ui::MenuActionEvent>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    // Backspace has two owners. While a modal tool is armed it means "retract
    // the last pick" (`modal_tool::step_back_modal_tool_system`); only when no
    // tool owns the cursor does it fall through to delete-selection below.
    // DRAFTING_UX.md Law 1: one owner per keypress, innermost first.
    active_modal_tool: Option<Res<crate::modal_tool::ActiveModalTool>>,
    play_state: Option<Res<State<crate::play_mode::PlayModeState>>>,
    // A live part drag owns bare T (see `TerrainTools` below).
    (select_state, move_state): (
        Option<Res<crate::select_tool::SelectToolState>>,
        Option<Res<crate::move_tool::MoveToolState>>,
    ),
) {
    // Block keyboard shortcuts when a text input has focus or overlay modal is open
    // (typing in Properties, Settings dialog, Workshop chat, etc.)
    if let Some(ref focus) = ui_focus {
        if focus.text_input_focused {
            return;
        }
    }
    if crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let Some(mut studio_state) = studio_state else { return };
    let Some(bindings) = bindings else { return };

    // While a Play session runs, the keyboard belongs to the game: only the
    // session controls and the panel toggles stay live. Delete, Undo, tool
    // switching, snap modes, Save and the rest would edit (or persist) the
    // scene the game is playing in.
    if !crate::play_mode::editor_input_enabled(play_state) {
        for action in PLAY_SESSION_ACTIONS.iter().copied() {
            if bindings.check(action, &keys) {
                info!("⌨️ Shortcut (Play): {:?}", action);
                menu_events.write(crate::ui::MenuActionEvent::new(action));
                return;
            }
        }
        return;
    }

    // The terrain context: while the terrain tools are the current tool,
    // their chords win over global ones on the same keys (1-3 snap, 5
    // perspective), so one keypress has one owner.
    if studio_state.current_tool == crate::ui::Tool::Terrain {
        for action in TERRAIN_ACTIONS.iter().copied() {
            if bindings.check(action, &keys) {
                menu_events.write(crate::ui::MenuActionEvent::new(action));
                return;
            }
        }
    }

    // Tool switching — directly update StudioState for instant response
    if bindings.check(Action::SelectTool, &keys) {
        info!("⌨️ Shortcut: Select Tool (Alt+Z)");
        studio_state.current_tool = crate::ui::Tool::Select;
        return;
    }
    if bindings.check(Action::MoveTool, &keys) {
        info!("⌨️ Shortcut: Move Tool (Alt+X)");
        studio_state.current_tool = crate::ui::Tool::Move;
        return;
    }
    if bindings.check(Action::ScaleTool, &keys) {
        info!("⌨️ Shortcut: Scale Tool (Alt+C)");
        studio_state.current_tool = crate::ui::Tool::Scale;
        return;
    }
    if bindings.check(Action::RotateTool, &keys) {
        info!("⌨️ Shortcut: Rotate Tool (Alt+V)");
        studio_state.current_tool = crate::ui::Tool::Rotate;
        return;
    }

    // Delete key only — Backspace is reserved for text editing.
    //
    // Belt-and-braces guard: the early-return above already blocks
    // shortcuts when Slint's `text_input_focused` is true, but that
    // signal depends on `changed has-focus` bubbling out of every
    // LineEdit / dropdown — a chain that's silently broken when a
    // new property row type forgets to fire `focus-changed`. So we
    // ALSO require the cursor to be over the 3D viewport. The
    // Properties panel sits outside the viewport, so this single
    // condition blocks the entity-delete shortcut while the user is
    // typing into a property field, regardless of whether the
    // focus-bubbling chain is healthy. Cursor over viewport with a
    // selection is still the normal "press Delete to delete entity"
    // path.
    // `SlintUIFocus.has_focus` is set as `has_focus = !in_viewport` (slint_ui.rs),
    // i.e. it's TRUE when the cursor is over a UI panel. So "cursor over the 3D
    // viewport" is its inverse — `!has_focus`. This was previously written as
    // `f.has_focus`, which inverted the gate: Delete only fired while the cursor
    // was over a panel and never over the viewport (the delete-key regression).
    // Default to in-viewport when the focus resource isn't up yet (pre-UI-init
    // frames) so Delete still works; `text_input_focused` is already handled by
    // the early return above, so typing-in-a-field can't trigger a delete.
    let cursor_over_viewport = ui_focus.as_ref()
        .map(|f| !f.has_focus)
        .unwrap_or(true);
    // Backspace belongs to the active modal tool while one is armed: it retracts
    // that tool's last pick. Deleting the selection out from under a half-built
    // Gap Fill / Edge Align sequence is destructive AND unasked-for, so the
    // delete branch yields the key the same way the move/rotate/scale drag
    // handlers yield the cursor. Delete itself is unambiguous and stays live.
    let backspace_owned_by_modal_tool = keys.just_pressed(KeyCode::Backspace)
        && crate::modal_tool::should_suppress_non_modal(active_modal_tool.as_ref());
    if (keys.just_pressed(KeyCode::Delete)
        || (keys.just_pressed(KeyCode::Backspace) && !backspace_owned_by_modal_tool))
        && cursor_over_viewport
    {
        let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
        if !ctrl && !alt {
            info!("⌨️ Delete/Backspace in viewport → delete selection");
            menu_events.write(crate::ui::MenuActionEvent::new(Action::Delete));
            return;
        }
    }

    // All other actions → dispatch as MenuActionEvent. The list lives in
    // `DISPATCHED_ACTIONS` so the binding-coverage test can walk exactly
    // what this loop walks.
    // Bare T tilts the dragged part during a live Select or Move drag
    // (`select_tool::rotate_tilt_shortcuts`, same condition). Entering the
    // terrain tools on that press would switch tools mid-drag and commit the
    // tilted part, so the drag keeps the key.
    let part_drag_live = select_state.as_ref().is_some_and(|s| s.dragging && s.drag_started)
        || move_state.as_ref().is_some_and(|s| s.free_drag);
    for action in DISPATCHED_ACTIONS.iter().copied() {
        if action == Action::TerrainTools && part_drag_live {
            continue;
        }
        if bindings.check(action, &keys) {
            // Delete (like Cut / Duplicate / Group / Ungroup) must work from
            // the Explorer — that's the natural place to pick a named item and
            // delete it. The ONLY real typing hazard is pressing Delete while
            // editing a text field, and that's already fully covered by the
            // `text_input_focused` early-return at the top of this function
            // (every LineEdit / rename box / dropdown sets it). The old extra
            // `cursor-over-viewport` guard here ALSO blocked the entire
            // Explorer panel — so selecting a node and pressing Del did
            // nothing, the "Del key doesn't work" report. Removed: a focus
            // false-negative on some property row is a bug to fix in that row,
            // not a reason to cripple Delete everywhere but the viewport.
            info!("⌨️ Shortcut: {:?}", action);
            menu_events.write(crate::ui::MenuActionEvent::new(action));
            return;
        }
    }
}

// ============================================================================
// MenuActionEvent Handler System
// ============================================================================

/// Processes MenuActionEvents dispatched by keyboard shortcuts or Slint UI.
/// Handles actions that modify StudioState or trigger editor behavior.
/// Uses Option wrappers to prevent silent skip from error handler.
/// Parse a binary-ECS synthetic instance path
/// (`.../Workspace/__bin_{class}_{stored_id:016x}/_instance.toml`) into
/// `(stored_id, class_name)`. Returns `None` for a normal on-disk path, so
/// only true binary-ECS entities take the core-purge branch on delete.
/// Mirrors `world_db_binary::synthetic_path`'s folder format.
fn parse_synthetic_bin_path(toml_path: &std::path::Path) -> Option<(u64, String)> {
    let folder = toml_path.parent()?.file_name()?.to_str()?;
    let rest = folder.strip_prefix("__bin_")?;
    // `{class}_{16-hex id}` — split the trailing id off the (final) '_'.
    let (class, id_hex) = rest.rsplit_once('_')?;
    if id_hex.len() != 16 {
        return None;
    }
    let stored_id = u64::from_str_radix(id_hex, 16).ok()?;
    Some((stored_id, class.to_string()))
}

/// Walk up from an instance path to the Space root that owns it.
///
/// Trash belongs at `<space>/.eustress/trash`. Both delete branches used to
/// anchor it relative to the deleted item's own folder — "one level up from the
/// part's containing folder", which is only the right answer for a flat
/// `Workspace/Thing/` layout. Anything nested deeper (a column, inside a
/// rotunda, inside a wing) scattered `.eustress/trash` directories through the
/// authored tree at arbitrary depth, where they are invisible to the user and
/// get wiped by anything that rewrites a parent folder.
fn space_root_for(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if crate::space::looks_like_space_root(parent) {
            return Some(parent.to_path_buf());
        }
        cur = parent;
    }
    None
}

/// The name the Insert Object dialog shows as its destination: the primary
/// selection, else the Workspace root the insert handler falls back to.
fn insert_target_label(
    explorer_state: Option<&crate::ui::slint_ui::UnifiedExplorerState>,
    instances: &Query<&eustress_common::classes::Instance>,
) -> String {
    explorer_state
        .and_then(|es| match &es.selected {
            crate::ui::slint_ui::SelectedItem::Entity(e) => Some(*e),
            _ => None,
        })
        .and_then(|e| instances.get(e).ok().map(|i| i.name.clone()))
        .unwrap_or_else(|| "Workspace".to_string())
}

fn handle_menu_action_events(
    mut events: MessageReader<crate::ui::MenuActionEvent>,
    mut commands: Commands,
    studio_state: Option<ResMut<crate::ui::StudioState>>,
    // Event writers bundled as tuple (keeps total param count ≤ 16)
    mut event_writers: (
        MessageWriter<crate::commands::UndoCommandEvent>,
        MessageWriter<crate::commands::RedoCommandEvent>,
        MessageWriter<crate::camera_controller::FrameSelectionEvent>,
        MessageWriter<crate::camera_controller::GoToCameraEvent>,
        MessageWriter<crate::clipboard::CopyEvent>,
        MessageWriter<crate::clipboard::DuplicateEvent>,
        MessageWriter<crate::undo::UndoEvent>,
        MessageWriter<crate::undo::RedoEvent>,
        MessageWriter<crate::ui::FileEvent>,
    ),
    selection_manager: Option<Res<crate::selection_sync::SelectionSyncManager>>,
    // `BasePart` is `&mut` for the Anchor / Lock / Unlock arms. Read-only
    // callers keep using `.iter()`, which still yields `Option<&BasePart>`.
    // It has to be THIS query rather than a second `Query<&mut BasePart>`:
    // two queries in one system where one writes a component the other
    // reads is a hard Bevy access conflict (B0001) at system-init time.
    mut entity_query: Query<(Entity, Option<&GlobalTransform>, Option<&mut eustress_common::classes::BasePart>),
        Or<(With<crate::rendering::PartEntity>, With<eustress_common::classes::Instance>)>>,
    instance_query: Query<&eustress_common::classes::Instance>,
    instance_file_query: Query<&crate::space::instance_loader::InstanceFile>,
    loaded_from_file_query: Query<&crate::space::LoadedFromFile>,
    mut file_registry: Option<ResMut<crate::space::SpaceFileRegistry>>,
    mut undo_stack: ResMut<crate::undo::UndoStack>,
    mut editor_settings: Option<ResMut<crate::editor_settings::EditorSettings>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    mut explorer_state: Option<ResMut<crate::ui::slint_ui::UnifiedExplorerState>>,
    // Hierarchy-selection event writers (bundled to stay under Bevy's
    // 16-param limit on systems).
    mut selection_events: (
        MessageWriter<crate::selection_sync::SelectChildrenEvent>,
        MessageWriter<crate::selection_sync::SelectDescendantsEvent>,
        MessageWriter<crate::selection_sync::SelectParentEvent>,
        MessageWriter<crate::selection_sync::SelectSiblingsEvent>,
        MessageWriter<crate::selection_sync::InvertSelectionEvent>,
    ),
    // Modal-tool activation events (fired by the Ctrl+Alt+<Letter> Smart
    // Build Tool shortcuts) bundled with the signing identity the
    // Anchor / Lock / Unlock arms stamp their TOML writes with. Bundled
    // for the same reason the two tuples above are: this system already
    // sits on Bevy's 16-parameter ceiling, so a 17th argument does not
    // compile.
    mut modal_and_auth: (
        MessageWriter<crate::modal_tool::ActivateModalToolEvent>,
        Option<Res<crate::auth::AuthState>>,
    ),
) {
    let (
        ref mut _selection_undo_events,
        ref mut _selection_redo_events,
        ref mut frame_events,
        ref mut go_to_camera_events,
        ref mut copy_events,
        ref mut duplicate_events,
        ref mut undo_action_events,
        ref mut redo_action_events,
        ref mut file_events,
    ) = event_writers;
    let (ref mut activate_modal_tool, ref auth) = modal_and_auth;
    let Some(mut studio_state) = studio_state else { return };

    for event in events.read() {
        match event.action {
            // Tool switching (also reachable via MenuActionEvent from Slint)
            Action::SelectTool => { studio_state.current_tool = crate::ui::Tool::Select; }
            Action::MoveTool   => { studio_state.current_tool = crate::ui::Tool::Move; }
            Action::ScaleTool  => { studio_state.current_tool = crate::ui::Tool::Scale; }
            Action::RotateTool => { studio_state.current_tool = crate::ui::Tool::Rotate; }

            // Ctrl+L — flip the gizmo / transform-input frame between
            // world-axis (default) and the active entity's local-axis
            // basis. Matches Blender's `,` / Maya's `w` toggle. Affects
            // Move + Rotate + Scale tools uniformly via `studio_state`.
            Action::ToggleTransformSpace => {
                studio_state.transform_mode = match studio_state.transform_mode {
                    crate::ui::TransformMode::World => crate::ui::TransformMode::Local,
                    crate::ui::TransformMode::Local => crate::ui::TransformMode::World,
                };
                info!(
                    "⌨️ Shortcut: Toggle Transform Space → {:?}",
                    studio_state.transform_mode,
                );
            }

            // Manual save (Ctrl+S) — write ECS to disk + commit to git
            // as a recoverable save point. Routed through `FileEvent` so
            // it goes through the same exclusive-system path as the
            // Slint File→Save menu.
            Action::SaveScene => {
                file_events.write(crate::ui::FileEvent::SaveScene);
            }

            // The rest of the File menu, routed through the SAME
            // `FileEvent` variants the Slint dropdown items push (see the
            // `SlintAction::New*` / `OpenScene` / `SaveSceneAs` arms in
            // `slint_ui.rs`), so keyboard and menu land in one handler.
            // "New Space" is `NewScene` — the ribbon's `on-new-scene()`
            // callback is what that menu item fires.
            Action::NewSpace   => { file_events.write(crate::ui::FileEvent::NewScene); }
            Action::NewUniverse => { file_events.write(crate::ui::FileEvent::NewUniverse); }
            Action::OpenFile   => { file_events.write(crate::ui::FileEvent::OpenScene); }
            Action::SaveSceneAs => { file_events.write(crate::ui::FileEvent::SaveSceneAs); }

            // Undo/Redo walk the ONE edit history (UndoStack, the one the
            // History panel shows). They used to also step the separate
            // selection history, so a single Ctrl+Z reverted a selection
            // AND an unrelated edit, and a Ctrl+Z meant to get a selection
            // back undid real work. Undo now re-selects what it changed.
            Action::Undo => {
                undo_action_events.write(crate::undo::UndoEvent);
            }
            Action::Redo => {
                redo_action_events.write(crate::undo::RedoEvent);
            }

            // View panel toggles
            Action::ToggleExplorer   => { studio_state.show_explorer = !studio_state.show_explorer; }
            Action::ToggleProperties => { studio_state.show_properties = !studio_state.show_properties; }
            Action::ToggleOutput     => { studio_state.show_output = !studio_state.show_output; }

            // Copy / Paste
            Action::Copy => { copy_events.write(crate::clipboard::CopyEvent { is_cut: false }); }
            Action::Cut => { copy_events.write(crate::clipboard::CopyEvent { is_cut: true }); }
            Action::Paste => { studio_state.pending_paste = true; }

            // Command bar
            Action::ToggleCommandBar => {
                studio_state.toggle_command_bar_pulse = studio_state.toggle_command_bar_pulse.wrapping_add(1);
            }

            // Focus camera on selection (F key)
            // Reads from SelectionSyncManager directly so it works even on the same
            // frame an Explorer-click selection happens (no SelectionBox yet).
            Action::FocusSelection => {
                // Get the set of currently selected IDs
                let selected_ids: std::collections::HashSet<String> = selection_manager
                    .as_ref()
                    .map(|sm| sm.0.read().get_selected().into_iter().collect())
                    .unwrap_or_default();

                let mut min = Vec3::splat(f32::MAX);
                let mut max = Vec3::splat(f32::MIN);
                let mut has_selection = false;
                // If a selected entity is a Camera (e.g. the AI camera), F goes
                // to ITS viewpoint instead of framing it — a camera has no size,
                // so the old path just nudged the zoom on a point.
                let mut camera_target: Option<Entity> = None;

                if !selected_ids.is_empty() {
                    for (entity, transform, base_part) in entity_query.iter() {
                        let id = format!("{}v{}", entity.index(), entity.generation());
                        if !selected_ids.contains(&id) { continue; }

                        if instance_query.get(entity)
                            .map(|i| i.class_name == eustress_common::classes::ClassName::Camera)
                            .unwrap_or(false)
                        {
                            camera_target = Some(entity);
                            continue; // don't fold the camera into framing bounds
                        }

                        let pos = transform.map(|t| t.translation()).unwrap_or(Vec3::ZERO);
                        let half_size = base_part
                            .map(|bp| bp.size * 0.5)
                            .unwrap_or(Vec3::splat(0.5));
                        min = min.min(pos - half_size);
                        max = max.max(pos + half_size);
                        has_selection = true;
                    }
                }

                if let Some(cam_e) = camera_target {
                    // Go to the selected camera's viewpoint (the AI camera's eyes).
                    go_to_camera_events.write(crate::camera_controller::GoToCameraEvent {
                        target: cam_e,
                    });
                    info!("📷 Focus: go to camera viewpoint {:?}", cam_e);
                } else if has_selection {
                    frame_events.write(crate::camera_controller::FrameSelectionEvent {
                        target_bounds: Some((min, max)),
                    });
                    info!("📷 Focus on selection: bounds ({:?} to {:?})", min, max);
                }
                // No selection → no-op (framing an empty scene flew to sky).
            }

            // Snapping
            Action::SnapMode1 => {
                if let Some(ref mut es) = editor_settings {
                    es.snap_size = 1.0;
                    es.snap_enabled = true;
                }
            }
            Action::SnapMode2 => {
                if let Some(ref mut es) = editor_settings {
                    es.snap_size = 0.2;
                    es.snap_enabled = true;
                }
            }
            Action::SnapModeOff => {
                if let Some(ref mut es) = editor_settings {
                    es.snap_enabled = false;
                }
            }

            // Delete selected entities; respawn default camera at origin if Camera class deleted
            Action::Delete => {
                let sm_exists = selection_manager.is_some();
                let selected_ids: std::collections::HashSet<String> = selection_manager
                    .as_ref()
                    .map(|sm| sm.0.read().get_selected().into_iter().collect())
                    .unwrap_or_default();

                info!("🗑️ Delete action: sm_exists={}, selected_ids={:?}", sm_exists, selected_ids);

                if selected_ids.is_empty() {
                    info!("🗑️ Delete: nothing selected");
                } else {
                    let mut camera_deleted = false;
                    let mut trashed_paths: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
                    let mut skipped_services = 0u32;

                    for (entity, gtransform, _) in entity_query.iter() {
                        let id = format!("{}v{}", entity.index(), entity.generation());
                        if !selected_ids.contains(&id) { continue; }
                        // Watermark so the end of the loop can tell whether any
                        // branch actually trashed a file for THIS entity.
                        let trashed_before = trashed_paths.len();

                        // Core-service guard: Workspace, Lighting, SoulService,
                        // ReplicatedStorage, etc. are scaffolding for the entire
                        // Space — deleting one orphans every child and breaks the
                        // file watcher. Refuse silently-counted so a multi-select
                        // delete still removes the non-service items and the user
                        // gets a single explanatory toast at the end.
                        let path_for_protection = instance_file_query.get(entity)
                            .map(|inst| inst.toml_path.clone())
                            .ok()
                            .or_else(|| loaded_from_file_query.get(entity)
                                .ok().map(|l| l.path.clone()));
                        if let Some(ref p) = path_for_protection {
                            if crate::space::is_protected_service_path(p) {
                                skipped_services += 1;
                                info!("🔒 Skipping delete on protected service: {:?}", p);
                                continue;
                            }
                        }

                        if instance_query.get(entity)
                            .map(|inst| inst.class_name == eustress_common::classes::ClassName::Camera)
                            .unwrap_or(false)
                        {
                            camera_deleted = true;
                        }

                        // Binary-ECS entities (scalable Insert default, C1):
                        // their authoritative state is a rkyv core in Fjall
                        // (Morton + identity indices), reachable only by their
                        // synthetic `__bin_{class}_{id}` path — NOT a disk
                        // folder. The file-trash branch below would fail to
                        // trash a non-existent folder AND leave the core
                        // behind, so it resurrects on the next boot-load.
                        // Detect + purge all five stores here, then despawn.
                        if let Ok(inst_file) = instance_file_query.get(entity) {
                            if let Some((stored_id, class_name)) =
                                parse_synthetic_bin_path(&inst_file.toml_path)
                            {
                                let uuid_hex = instance_query
                                    .get(entity)
                                    .map(|i| i.uuid.clone())
                                    .unwrap_or_default();
                                let uuid_bytes =
                                    eustress_common::instance_create::uuid_hex_to_bytes(&uuid_hex)
                                        .unwrap_or([0u8; 16]);
                                // Morton key is position-derived; the live
                                // (global) translation matches the persisted
                                // position within the 256-unit Morton cell for
                                // any settled part.
                                let pos = gtransform
                                    .map(|g| {
                                        let t = g.translation();
                                        [t.x, t.y, t.z]
                                    })
                                    .unwrap_or([0.0, 0.0, 0.0]);
                                let synthetic_rel = format!(
                                    "Workspace/__bin_{}_{:016x}/_instance.toml",
                                    class_name, stored_id
                                );
                                crate::space::active_db::delete_binary_instance(
                                    stored_id,
                                    &uuid_bytes,
                                    &class_name,
                                    pos,
                                    &synthetic_rel,
                                );
                                commands.entity(entity).despawn();
                                info!(
                                    "🗑️ Deleted binary-ECS entity stored_id={:016x} (purged Fjall core + identity indices)",
                                    stored_id
                                );
                                continue;
                            }
                        }

                        // Move TOML (or the whole folder, for folder-based
                        // entities) to .eustress/trash/ so Ctrl+Z can restore.
                        if let Ok(inst_file) = instance_file_query.get(entity) {
                            let toml_path = inst_file.toml_path.clone();
                            // Non-removable folders: the Workshop folder
                            // under SoulService is the engine's chat
                            // history root — deleting it would scramble
                            // session persistence + trap the Workshop
                            // panel in a "no Space bound" state. Skip
                            // silently (match OS convention for
                            // system-protected paths) and continue so
                            // the rest of the multi-select delete still
                            // works.
                            let is_workshop_root = toml_path
                                .components()
                                .rev()
                                .take(3)
                                .collect::<Vec<_>>()
                                .iter()
                                .rev()
                                .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
                                .collect::<Vec<_>>()
                                .ends_with(&["soulservice".to_string(), "workshop".to_string(), "_instance.toml".to_string()]);
                            if is_workshop_root {
                                info!("🔒 Skipping delete on protected Workshop folder");
                                continue;
                            }
                            // Folder-based entities live in `Foo/_instance.toml`;
                            // trashing only the TOML leaves an empty folder on disk
                            // AND orphans sibling files (Summary.md, child instances).
                            // Trash the containing folder instead.
                            let is_folder_instance = toml_path
                                .file_name()
                                .map(|n| n.to_string_lossy() == "_instance.toml")
                                .unwrap_or(false);
                            let source_path = if is_folder_instance {
                                toml_path.parent().unwrap_or(toml_path.as_path()).to_path_buf()
                            } else {
                                toml_path.clone()
                            };

                            if source_path.exists() {
                                // Trash lives at the SPACE ROOT, never beside the
                                // deleted item — see `space_root_for`.
                                let trash_anchor = if is_folder_instance {
                                    source_path.parent()
                                } else {
                                    toml_path.parent()
                                };
                                let trash_dir = space_root_for(&source_path)
                                    .or_else(|| trash_anchor
                                        .and_then(|p| p.parent())
                                        .map(|p| p.to_path_buf()))
                                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                                    .join(".eustress").join("trash");
                                let _ = std::fs::create_dir_all(&trash_dir);

                                // De-collide the trash name. Two deletes of the
                                // same `Block` folder would otherwise see the
                                // second rename fail ("target exists") on
                                // Windows, the fallback `remove_dir_all` might
                                // also fail (open handles), the error is
                                // swallowed via `let _`, and the on-disk
                                // folder stays while the entity despawns —
                                // the Explorer/file-system desync the user
                                // hit. Appending a monotonic timestamp
                                // guarantees a unique trash path every time.
                                let trash_stem = source_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("entity");
                                let ts_ms = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis())
                                    .unwrap_or(0);
                                let trash_path = {
                                    let base = trash_dir.join(trash_stem);
                                    if base.exists() {
                                        trash_dir.join(format!("{}-{:x}", trash_stem, ts_ms))
                                    } else {
                                        base
                                    }
                                };

                                // Tell file-watcher to ignore the impending delete
                                // so it doesn't try to despawn an already-gone entity.
                                if let Some(ref mut registry) = file_registry {
                                    registry.rename_in_progress.insert(toml_path.clone());
                                    registry.rename_in_progress.insert(source_path.clone());
                                }

                                // Rename-to-trash is the ONLY allowed path —
                                // we deliberately do *not* fall back to
                                // `remove_dir_all` / `remove_file`. Earlier
                                // versions did, but Windows transiently locks
                                // a part folder for ~50–200 ms after Bevy's
                                // asset server drops a `.glb` handle; during
                                // that window `rename` returns `Os { code:
                                // 5, kind: PermissionDenied }`, the fallback
                                // `remove_dir_all` succeeds against the
                                // already-gone-from-Bevy handle, and the
                                // entity vanishes WITHOUT a trash entry —
                                // which means undo has nothing to restore.
                                // Both user reports ("delete doesn't go to
                                // trash" + "undo doesn't bring it back")
                                // were the same bug: the fallback path.
                                //
                                // Now: retry rename a few times with a short
                                // sleep so transient handle holds clear, and
                                // if it still fails after the retries, log
                                // loudly and SKIP the despawn. The file
                                // stays on disk, the entity stays in ECS,
                                // and the user can retry — better than
                                // silent permanent loss.
                                // Before renaming, gather every descendant
                                // entity registered under this folder. The
                                // rename moves the whole subtree on disk, but
                                // descendant ECS entities (e.g. a child
                                // BillboardGui Label under the deleted Part)
                                // would otherwise survive and their queued
                                // save-on-Changed writes flush AFTER the
                                // rename — failing because the parent dir is
                                // gone, occasionally racing the save into a
                                // resurrected file. Despawn them explicitly
                                // and add their paths to `rename_in_progress`
                                // so the watcher swallows the cascade of
                                // delete events without firing redundant
                                // despawns. The orphan
                                // `Workspace/Part-aaXX/Label/_instance.toml`
                                // user-bug 2026-05-13 was this race.
                                let descendant_paths: Vec<(std::path::PathBuf, Entity)> =
                                    if is_folder_instance {
                                        file_registry.as_ref()
                                            .map(|r| r.descendants_of(&source_path))
                                            .unwrap_or_default()
                                    } else {
                                        Vec::new()
                                    };
                                if let Some(ref mut registry) = file_registry {
                                    for (path, _) in &descendant_paths {
                                        registry.rename_in_progress.insert(path.clone());
                                    }
                                }

                                let moved = (|| {
                                    let attempts = 5u32;
                                    let mut last_err: Option<std::io::Error> = None;
                                    for i in 0..attempts {
                                        match std::fs::rename(&source_path, &trash_path) {
                                            Ok(_) => {
                                                // Store (source_path, trash_path) — NOT
                                                // toml_path. The rename moved `source_path`
                                                // (the folder) so undo must rename back to
                                                // `source_path`, not to `_instance.toml`.
                                                trashed_paths.push((source_path.clone(), trash_path.clone()));
                                                info!(
                                                    "🗑️ Moved {:?} to trash{}{}",
                                                    source_path.file_name().unwrap_or_default(),
                                                    if i > 0 { format!(" (after {} retr{})", i, if i == 1 { "y" } else { "ies" }) } else { String::new() },
                                                    if descendant_paths.len() > 1 {
                                                        format!(" + {} descendant{}", descendant_paths.len() - 1,
                                                            if descendant_paths.len() == 2 { "" } else { "s" })
                                                    } else { String::new() },
                                                );
                                                return true;
                                            }
                                            Err(e) => {
                                                last_err = Some(e);
                                                if i + 1 < attempts {
                                                    std::thread::sleep(std::time::Duration::from_millis(60));
                                                }
                                            }
                                        }
                                    }
                                    warn!(
                                        "❌ Could not move {:?} to trash after {} attempts ({}). \
                                         Leaving file on disk + entity in ECS — retry the delete.",
                                        source_path,
                                        attempts,
                                        last_err
                                            .map(|e| e.to_string())
                                            .unwrap_or_else(|| "<unknown>".to_string()),
                                    );
                                    if let Some(ref mut registry) = file_registry {
                                        registry.rename_in_progress.remove(&toml_path);
                                        registry.rename_in_progress.remove(&source_path);
                                        // Roll back the descendant gating we
                                        // staged above so the watcher resumes
                                        // tracking them.
                                        for (path, _) in &descendant_paths {
                                            registry.rename_in_progress.remove(path);
                                        }
                                    }
                                    false
                                })();
                                if !moved {
                                    // Skip ECS despawn so Explorer stays
                                    // in sync with the still-present file.
                                    continue;
                                }
                                // Rename succeeded — despawn every descendant
                                // entity and clean its registry entry so the
                                // ECS matches the on-disk subtree we just
                                // moved to trash. The parent entity itself
                                // gets despawned below (after the if-let-ok
                                // closure) via `commands.entity(entity)
                                // .despawn()`.
                                for (path, desc_entity) in &descendant_paths {
                                    if *desc_entity == entity { continue; }
                                    // Capture identity BEFORE despawn so we can
                                    // purge every store keyed on its UUID.
                                    let (d_uuid, d_class) = instance_query
                                        .get(*desc_entity)
                                        .map(|i| (i.uuid.clone(), i.class_name.as_str().to_string()))
                                        .unwrap_or_default();
                                    commands.entity(*desc_entity).despawn();
                                    // DB-primary: purge the descendant from ALL
                                    // stores (tree + uuid core + indices), or it
                                    // resurrects from world.fjalldb next session.
                                    crate::space::active_db::purge_path_all_stores(path, &d_uuid, &d_class);
                                    if let Some(ref mut registry) = file_registry {
                                        registry.unregister_file(path);
                                    }
                                }
                            }
                            if let Some(ref mut registry) = file_registry {
                                registry.unregister_file(&toml_path);
                            }
                            // DB-primary: purge the entity from EVERY Fjall store —
                            // the `tree` TOML + `#bin` twin AND the uuid-keyed core
                            // + identity indices `migrate_identity` wrote. Clearing
                            // only the tree records (old `delete_path`) left the
                            // `entities_uuid` core behind, which a reconcile/rebuild
                            // re-materialised → the "deleted CAD/GS objects come back
                            // / delete one of a twin pair and it resurrects" bug.
                            let (uuid_hex, class_str) = instance_query
                                .get(entity)
                                .map(|i| (i.uuid.clone(), i.class_name.as_str().to_string()))
                                .unwrap_or_default();
                            crate::space::active_db::purge_path_all_stores(&toml_path, &uuid_hex, &class_str);
                        }
                        // Fallback: entities loaded via file_loader (soul scripts,
                        // Rune/Luau files) have LoadedFromFile but no InstanceFile.
                        // Without this branch the ECS entity despawns but the file
                        // stays on disk, so the file watcher re-creates the entity
                        // on the next scan — making delete appear broken.
                        else if let Ok(loaded) = loaded_from_file_query.get(entity) {
                            let source_path = loaded.path.clone();
                            if source_path.exists() {
                                // Space root, not the item's parent — see
                                // `space_root_for`.
                                let trash_dir = space_root_for(&source_path)
                                    .or_else(|| source_path.parent().map(|p| p.to_path_buf()))
                                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                                    .join(".eustress").join("trash");
                                let _ = std::fs::create_dir_all(&trash_dir);

                                let trash_stem = source_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("entity");
                                let ts_ms = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis())
                                    .unwrap_or(0);
                                let trash_path = {
                                    let base = trash_dir.join(trash_stem);
                                    if base.exists() {
                                        trash_dir.join(format!("{}-{:x}", trash_stem, ts_ms))
                                    } else {
                                        base
                                    }
                                };

                                if let Some(ref mut registry) = file_registry {
                                    registry.rename_in_progress.insert(source_path.clone());
                                }

                                let moved = (|| {
                                    let attempts = 5u32;
                                    let mut last_err: Option<std::io::Error> = None;
                                    for i in 0..attempts {
                                        match std::fs::rename(&source_path, &trash_path) {
                                            Ok(_) => {
                                                trashed_paths.push((source_path.clone(), trash_path.clone()));
                                                info!(
                                                    "🗑️ Moved script {:?} to trash{}",
                                                    source_path.file_name().unwrap_or_default(),
                                                    if i > 0 { format!(" (after {} retr{})", i, if i == 1 { "y" } else { "ies" }) } else { String::new() },
                                                );
                                                return true;
                                            }
                                            Err(e) => {
                                                last_err = Some(e);
                                                if i + 1 < attempts {
                                                    std::thread::sleep(std::time::Duration::from_millis(60));
                                                }
                                            }
                                        }
                                    }
                                    warn!(
                                        "❌ Could not move {:?} to trash after {} attempts ({}). \
                                         Leaving file on disk + entity in ECS — retry the delete.",
                                        source_path,
                                        attempts,
                                        last_err
                                            .map(|e| e.to_string())
                                            .unwrap_or_else(|| "<unknown>".to_string()),
                                    );
                                    if let Some(ref mut registry) = file_registry {
                                        registry.rename_in_progress.remove(&source_path);
                                    }
                                    false
                                })();
                                if !moved {
                                    continue;
                                }
                            }
                            if let Some(ref mut registry) = file_registry {
                                registry.unregister_file(&source_path);
                            }
                            // DB-primary: drop the DB record for file-backed
                            // entities (scripts, etc.) so they don't resurrect.
                            crate::space::active_db::delete_path(&source_path);
                        }
                        // Fall-through: entities with NEITHER InstanceFile NOR
                        // LoadedFromFile. A bulk in-memory import writes binary
                        // cores for `node_is_binary_eligible` classes (Folder /
                        // Model) straight into the worlddb `entities` partition +
                        // identity indices — the live entity carries no disk path,
                        // so neither source branch above fires. Without purging the
                        // core here, the bare despawn below removes only the ECS
                        // entity and the core resurrects on the next Space open
                        // (the reported "delete doesn't stick" bug). Derive the
                        // persistence `stored_id` from the entity's identity UUID
                        // exactly as the importer does (roblox-import/src/sink.rs
                        // and world_db_binary.rs: first 8 bytes of the UUID,
                        // big-endian) and purge the binary core + identity indices
                        // via the same synthetic path shape the importer mints. A
                        // harmless no-op if no such core exists (delete of a missing
                        // key is best-effort). ECS descendants are removed by the
                        // recursive `despawn()` below (Bevy 0.18 recursive despawn).
                        else {
                            if let Ok(inst) = instance_query.get(entity) {
                                if let Some(uuid_bytes) =
                                    eustress_common::instance_create::uuid_hex_to_bytes(&inst.uuid)
                                {
                                    let stored_id = u64::from_be_bytes(
                                        uuid_bytes[0..8]
                                            .try_into()
                                            .expect("uuid_bytes is 16 long; [0..8] is 8"),
                                    );
                                    let class_name = inst.class_name.as_str().to_string();
                                    let pos = gtransform
                                        .map(|g| {
                                            let t = g.translation();
                                            [t.x, t.y, t.z]
                                        })
                                        .unwrap_or([0.0, 0.0, 0.0]);
                                    let synthetic_rel = format!(
                                        "Workspace/__bin_{}_{:016x}/_instance.toml",
                                        class_name, stored_id
                                    );
                                    let purged = crate::space::active_db::delete_binary_instance(
                                        stored_id,
                                        &uuid_bytes,
                                        &class_name,
                                        pos,
                                        &synthetic_rel,
                                    );
                                    if purged {
                                        info!(
                                            "🗑️ Purged source-less binary core stored_id={:016x} ({}) on delete",
                                            stored_id, class_name
                                        );
                                    }
                                }
                            }
                        }
                        // Did anything actually get trashed for this entity? If
                        // no branch above resolved a real on-disk source, the
                        // despawn is COSMETIC: nothing moved, so Ctrl+Z has
                        // nothing to restore (the `TrashEntities` push below is
                        // gated on a non-empty `trashed_paths`) and the file is
                        // still there to respawn on the next load. That read to
                        // the user as "delete is broken" with no clue why, so
                        // say it out loud instead of failing silently.
                        if trashed_paths.len() == trashed_before {
                            warn!(
                                "🗑️ Deleted entity {:?} ({}) from the scene ONLY — no on-disk \
                                 source could be resolved, so this is NOT undoable and it will \
                                 come back when the Space reloads. Usually means the entity is \
                                 stale: its folder was removed out from under the running engine.",
                                entity, id
                            );
                        }
                        commands.entity(entity).despawn();
                        info!("🗑️ Deleted entity {:?} ({})", entity, id);
                    }

                    // Push to undo stack so Ctrl+Z can restore
                    if !trashed_paths.is_empty() {
                        undo_stack.push(crate::undo::Action::TrashEntities { paths: trashed_paths });
                    }

                    // Clear selection after delete
                    if let Some(ref sm) = selection_manager {
                        sm.0.write().clear();
                    }

                    // Force Explorer re-sync next frame so the deleted entities
                    // disappear from the tree without waiting for the 30-frame
                    // throttle — avoids the "deleted but still shown" UX.
                    if let Some(ref mut es) = explorer_state {
                        es.needs_immediate_sync = true;
                    }
                    // No respawn here: `camera_controller::ensure_camera_exists`
                    // is the one place an editor camera comes back, built the
                    // way startup builds it, as soon as no window camera is left.
                    if camera_deleted {
                        info!("📷 Camera deleted — a new editor camera spawns at the origin");
                    }
                    if skipped_services > 0 {
                        warn!(
                            "🔒 Refused to delete {} core service{} — Workspace, Lighting, SoulService, etc. are protected scaffolding.",
                            skipped_services, if skipped_services == 1 { "" } else { "s" },
                        );
                    }
                }
            }

            // Select All (Ctrl+A) — select all unlocked BasePart entities
            // Also blocked when cursor is over UI panels (Properties text fields)
            // Select every selectable object in the Space.
            //
            // Defined by EXCLUSION, deliberately. This used to require a
            // `BasePart` component, which silently meant "primitives only" —
            // every class without one (Beam, PointLight, ParticleEmitter,
            // Attachment, Decal, GaussianSplats, CadPart, …) was invisible to
            // Ctrl+A, and each new class quietly inherited the same gap. An
            // allow-list has to be updated for every class ever added; a
            // deny-list is correct by default and only needs touching when a
            // genuinely non-selectable class appears.
            Action::SelectAll => {
                if ui_focus.as_ref().map(|f| f.has_focus).unwrap_or(false) { continue; }
                if let Some(ref sel_mgr) = selection_manager {
                    let sm = sel_mgr.0.write();
                    sm.clear();
                    let mut count = 0usize;
                    for (entity, _, bp) in entity_query.iter() {
                        // Locked parts stay out. The Select tool's hit-test
                        // already skips them, so letting Ctrl+A grab one would
                        // make the keyboard the one way to move something the
                        // user locked on purpose.
                        if bp.map(|b| b.locked).unwrap_or(false) { continue; }

                        // No `Instance` means no class identity — engine-internal
                        // entities (gizmo handles, preview meshes, cameras spawned
                        // by tools) live here and are never scene content.
                        let Ok(inst) = instance_query.get(entity) else { continue };

                        // The engine's own selection boxes / gizmo meshes.
                        if inst.class_name.is_adornment() { continue; }

                        // Containers and celestial backdrop — anchors, not
                        // targets. Shared with `selection_sync` so Select All
                        // and Select Children agree on what "selectable" means.
                        if crate::selection_sync::ABSTRACT_CLASSES.contains(&inst.class_name) {
                            continue;
                        }

                        // The viewport camera and the terrain voxel volume are
                        // singletons, not objects you mass-transform. Sweeping
                        // them into a Ctrl+A → drag would move the viewpoint and
                        // the whole world with the selection.
                        if matches!(
                            inst.class_name,
                            eustress_common::classes::ClassName::Camera
                                | eustress_common::classes::ClassName::Terrain
                        ) {
                            continue;
                        }

                        // Service roots (Workspace, Lighting, SoulService, …) are
                        // Space scaffolding — the Delete arm below refuses them
                        // for the same reason, and Ctrl+A must not be the way a
                        // user gets one into a selection to begin with.
                        let backing = instance_file_query.get(entity).ok()
                            .map(|f| f.toml_path.clone())
                            .or_else(|| loaded_from_file_query.get(entity).ok()
                                .map(|l| l.path.clone()));
                        if backing.as_deref()
                            .map(crate::space::is_protected_service_path)
                            .unwrap_or(false)
                        {
                            continue;
                        }

                        sm.add_to_selection(format!("{}v{}", entity.index(), entity.generation()));
                        count += 1;
                    }
                    info!("⌨️ Select All: {} object(s) selected", count);
                }
            }

            // Duplicate (Ctrl+D) — copy + paste in place
            Action::Duplicate => {
                duplicate_events.write(crate::clipboard::DuplicateEvent);
            }

            // Hierarchy-selection commands — emit the corresponding
            // event; handler systems in selection_sync.rs do the work.
            Action::SelectChildren => {
                selection_events.0.write(crate::selection_sync::SelectChildrenEvent);
            }
            Action::SelectDescendants => {
                selection_events.1.write(crate::selection_sync::SelectDescendantsEvent);
            }
            Action::SelectParent => {
                selection_events.2.write(crate::selection_sync::SelectParentEvent);
            }
            Action::SelectSiblings => {
                selection_events.3.write(crate::selection_sync::SelectSiblingsEvent);
            }
            Action::InvertSelection => {
                selection_events.4.write(crate::selection_sync::InvertSelectionEvent);
            }

            // Roblox parity: the dialog, the paste target and the focus
            // requests are all one-shot flags the UI sync consumes.
            Action::InsertObject => {
                studio_state.insert_target_name = insert_target_label(
                    explorer_state.as_deref(),
                    &instance_query,
                );
                studio_state.show_insert_object_dialog = true;
            }
            Action::PasteInto => { studio_state.pending_paste_into = true; }
            Action::FindReplace => {
                studio_state.open_find_pulse = studio_state.open_find_pulse.wrapping_add(1);
            }
            Action::FocusExplorerSearch => {
                studio_state.show_explorer = true;
                studio_state.focus_explorer_search_pulse =
                    studio_state.focus_explorer_search_pulse.wrapping_add(1);
            }
            Action::FocusPropertiesFilter => {
                studio_state.show_properties = true;
                studio_state.focus_properties_filter_pulse =
                    studio_state.focus_properties_filter_pulse.wrapping_add(1);
            }
            Action::ToggleCollisions => {
                if let Some(ref mut es) = editor_settings {
                    es.collisions_enabled = !es.collisions_enabled;
                    info!("⌨️ Collisions: {}", if es.collisions_enabled { "ON" } else { "OFF" });
                }
            }

            // Anchor (Alt+A) / Lock (Alt+L) / Unlock (Alt+Shift+L) —
            // flip a BasePart boolean across the whole selection.
            //
            // Anchor uses "any loose → anchor everything" rather than
            // per-part flipping. Flipping each part individually leaves a
            // mixed selection mixed no matter how many times you press the
            // key, so the shortcut would never converge on a state the
            // user can predict; this way one press always makes the
            // selection uniformly solid, and the next press releases it.
            //
            // Lock/Unlock are explicitly directional (not a toggle)
            // because they have separate ribbon buttons and separate
            // chords, and because the Select tool's hit-test skips locked
            // parts — a toggle that re-locked a part you were trying to
            // free would be very hard to recover from.
            Action::ToggleAnchor | Action::LockSelection | Action::UnlockSelection => {
                let flag = if matches!(event.action, Action::ToggleAnchor) {
                    PartFlag::Anchored
                } else {
                    PartFlag::Locked
                };
                let selected_ids: std::collections::HashSet<String> = selection_manager
                    .as_ref()
                    .map(|sm| sm.0.read().get_selected().into_iter().collect())
                    .unwrap_or_default();
                if selected_ids.is_empty() {
                    // Nothing selected is the COMMON case for these two, not an
                    // error: the user reaches for Lock or Anchor meaning "let me
                    // click the parts I want flipped". Bailing here made the
                    // chord look dead — it logged `nothing selected` and did
                    // nothing visible. Arm the matching paint mode instead, so
                    // the chord and the ribbon button end in the same state and
                    // the next click flips whatever is under the cursor.
                    //
                    // Unlock is deliberately excluded. `Tool::Unlock` is the
                    // one-shot "unlock every part in the Space" escape hatch,
                    // and arming that from a stray keystroke is not something a
                    // user can undo by looking at the screen. Lock mode already
                    // TOGGLES on click (see `lock_tool_toggle_click`), so
                    // clicking a locked part in Lock mode frees it anyway.
                    match event.action {
                        Action::ToggleAnchor => {
                            studio_state.current_tool = crate::ui::Tool::Anchor;
                            info!("⚓ Anchor mode armed — click parts to toggle, Esc to exit");
                        }
                        Action::LockSelection => {
                            studio_state.current_tool = crate::ui::Tool::Lock;
                            info!("🔒 Lock mode armed — click parts to toggle, Esc to exit");
                        }
                        _ => {
                            info!(
                                "⌨️ {}: nothing selected (Unlock All is on the ribbon)",
                                event.action.name(),
                            );
                        }
                    }
                    continue;
                }

                // Pass 1 (immutable): resolve the target value and the
                // entity set, so pass 2 can take the mutable borrow.
                let mut targets: Vec<Entity> = Vec::new();
                let mut any_off = false;
                for (entity, _, bp) in entity_query.iter() {
                    let id = format!("{}v{}", entity.index(), entity.generation());
                    if !selected_ids.contains(&id) { continue; }
                    let Some(bp) = bp else { continue };
                    if !flag.read(bp) { any_off = true; }
                    targets.push(entity);
                }
                if targets.is_empty() {
                    info!("⌨️ {}: selection holds no parts", event.action.name());
                    continue;
                }
                let new_value = match event.action {
                    Action::LockSelection => true,
                    Action::UnlockSelection => false,
                    _ => any_off, // ToggleAnchor
                };

                // Pass 2 (mutable): apply + persist, collecting the
                // pre-change values for a single undo entry.
                let mut multi_old: Vec<(u32, crate::undo::PropertyValueSnapshot)> = Vec::new();
                for entity in targets {
                    let Ok((_, _, Some(mut bp))) = entity_query.get_mut(entity) else { continue };
                    if flag.read(&bp) == new_value { continue; }
                    let old = crate::undo::PropertyValueSnapshot::Bool(flag.read(&bp));
                    flag.write(&mut bp, new_value);
                    if let Ok(inst) = instance_query.get(entity) {
                        multi_old.push((inst.id, old));
                    }
                    persist_part_flag(entity, flag, new_value, &instance_file_query, auth);
                }
                if multi_old.is_empty() {
                    continue; // whole selection was already at the target
                }
                info!(
                    "{} {} → {} on {} part(s)",
                    flag.log_icon(new_value),
                    flag.property(),
                    new_value,
                    multi_old.len(),
                );
                // ChangePropertyMulti (not N × ChangeProperty) so the
                // History panel shows ONE entry the user can undo in one
                // step — the same shape the Properties panel pushes for a
                // broadcast edit (`slint_ui::apply_property_value_to_entity`).
                undo_stack.push(crate::undo::Action::ChangePropertyMulti {
                    entities: multi_old,
                    property: flag.property().to_string(),
                    new_value: crate::undo::PropertyValueSnapshot::Bool(new_value),
                });
            }

            // Smart Build Tools — activate via the modal-tool registry.
            // The tool_id strings match the factories registered in
            // `tools_smart::register_smart_tools`.
            Action::ToolPartSwap => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "part_swap_positions".to_string(),
                });
            }
            Action::ToolEdgeAlign => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "edge_align".to_string(),
                });
            }
            Action::ToolModelReflect => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "model_reflect".to_string(),
                });
            }
            Action::ToolGapFill => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "gap_fill".to_string(),
                });
            }
            Action::ToolResizeAlign => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "resize_align".to_string(),
                });
            }
            Action::ToolMaterialFlip => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "material_flip".to_string(),
                });
            }
            Action::ToolLinearArray => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "linear_array".to_string(),
                });
            }
            Action::ToolRadialArray => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "radial_array".to_string(),
                });
            }
            Action::ToolGridArray => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "grid_array".to_string(),
                });
            }
            Action::ToolPathArray => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "path_array".to_string(),
                });
            }

            // Boolean (CSG) ribbon group — handled by `csg::CsgPlugin`
            // (MenuActionEvent reader). Leave these arms empty so we
            // don't double-fire; the plugin owns toasts + real ops.
            Action::CSGUnion | Action::CSGNegate | Action::CSGIntersect | Action::CSGSeparate => {}

            // Other actions are consumed by their respective systems.
            //
            // Deliberately owned elsewhere (do NOT add arms here, you'd
            // double-fire): the play controls `PlayWithCharacter` /
            // `PauseResume` / `PlaySolo` / `StopPlay` belong to
            // `play_mode.rs`, and `StartServer` / `StopServer` to the
            // networking plugin.
            //
            // `PublishUniverse` / `PublishSpace` are owned by
            // `slint_ui::route_publish_shortcuts`, which re-queues them as the
            // same `SlintAction`s the File menu pushes. Opening either dialog
            // needs the Slint window handle, which lives only in the drain, and
            // `FileEvent::Publish(PublishRequest)` is the POST-dialog commit —
            // firing that from a keystroke would publish with an empty
            // experience name. Routing through the drain keeps one dialog path
            // for both the keyboard and the menu, including the Space variant's
            // `sync.toml` experience-id precheck.
            _ => {}
        }
    }
}

/// Which `BasePart` boolean the Anchor / Lock / Unlock arms operate on.
///
/// One enum instead of two near-identical match arms keeps the read, the
/// write, the TOML field, and the undo property string from drifting apart
/// — the four places a copy-paste of this logic would have to stay in sync.
#[derive(Clone, Copy)]
enum PartFlag {
    Anchored,
    Locked,
}

impl PartFlag {
    fn read(self, bp: &eustress_common::classes::BasePart) -> bool {
        match self {
            PartFlag::Anchored => bp.anchored,
            PartFlag::Locked => bp.locked,
        }
    }

    fn write(self, bp: &mut eustress_common::classes::BasePart, value: bool) {
        match self {
            PartFlag::Anchored => bp.anchored = value,
            PartFlag::Locked => bp.locked = value,
        }
    }

    /// The `PropertyValueSnapshot` key `undo::apply_property_value_to_entity`
    /// matches on. Must stay spelled exactly as the Properties panel spells
    /// it or undo silently warns "Unknown property" and restores nothing.
    fn property(self) -> &'static str {
        match self {
            PartFlag::Anchored => "Anchored",
            PartFlag::Locked => "Locked",
        }
    }

    fn log_icon(self, value: bool) -> &'static str {
        match (self, value) {
            (PartFlag::Anchored, true) => "⚓",
            (PartFlag::Anchored, false) => "🎈",
            (PartFlag::Locked, true) => "🔒",
            (PartFlag::Locked, false) => "🔓",
        }
    }
}

/// Persist a flipped `BasePart` flag into the entity's `_instance.toml`,
/// signed when the user is logged in.
///
/// Mirrors `lock_tool::persist_locked` exactly — same load → mutate →
/// `write_instance_definition_signed` sequence — so a part locked with the
/// Lock tool and a part locked with Alt+L leave identical files on disk.
/// Entities with no `InstanceFile` are binary-ECS parts whose authoritative
/// state is a rkyv core in Fjall; `world_db_binary`'s `Changed<BasePart>`
/// mirror persists those, so skipping them here is correct rather than a gap.
fn persist_part_flag(
    entity: Entity,
    flag: PartFlag,
    value: bool,
    instance_files: &Query<&crate::space::instance_loader::InstanceFile>,
    auth: &Option<Res<crate::auth::AuthState>>,
) {
    let Ok(inst_file) = instance_files.get(entity) else { return };
    let stamp = auth.as_deref().and_then(crate::space::instance_loader::current_stamp);
    if let Ok(mut def) = crate::space::instance_loader::load_instance_definition(&inst_file.toml_path) {
        match flag {
            PartFlag::Anchored => def.properties.anchored = value,
            PartFlag::Locked => def.properties.locked = value,
        }
        let _ = crate::space::instance_loader::write_instance_definition_signed(
            &inst_file.toml_path,
            &mut def,
            stamp.as_ref(),
        );
    }
}

// ============================================================================
// Vertical placement — `-` lifts by a grid unit, `+` settles onto a surface
// ============================================================================
//
// The two keys are NOT symmetric and the action names say so
// ([`Action::LiftSelection`] / [`Action::SettleSelection`]): `-` is a plain
// +Y step, `+` steps down and flushes onto the first support surface it
// reaches. They are read as raw KeyCodes here rather than through
// `bindings.check()` because they need press-and-hold auto-repeat, which the
// one-shot `just_pressed` dispatch loop deliberately does not do.

/// State for the lift/settle keys — tracks hold time and whether the initial
/// press was consumed.
#[derive(Resource, Default)]
struct NudgeTimer {
    lift_held: bool,
    lift_timer: f32,
    settle_held: bool,
    settle_timer: f32,
}

/// Initial delay before auto-repeat starts (seconds). ~OS-standard
/// keyboard-repeat latency — short enough that holding the key feels
/// responsive, long enough that a deliberate single tap stays a single
/// nudge.
const NUDGE_DELAY_SECS: f32 = 0.30;
/// Repeat interval once auto-repeat is active (seconds). 12 nudges/sec.
const NUDGE_REPEAT_SECS: f32 = 0.08;

/// Queries + resources needed by `handle_nudge_keys`. Bundled into a
/// `SystemParam` so the handler itself stays under Bevy's 16-param
/// limit — the move-down step needs the selection transforms plus a
/// `SpatialQuery` for the downward raycast.
#[derive(bevy::ecs::system::SystemParam)]
pub struct NudgeContext<'w, 's> {
    pub selected: Query<
        'w, 's,
        (
            Entity,
            &'static mut Transform,
            &'static GlobalTransform,
            Option<&'static crate::classes::BasePart>,
        ),
        With<crate::selection_box::Selected>,
    >,
    pub spatial: avian3d::prelude::SpatialQuery<'w, 's>,
    /// Every nudge is one undo step (a held key folds into one).
    pub undo: ResMut<'w, crate::undo::UndoStack>,
    /// Children, parents and parents' world transforms: nudges move in
    /// world space, skip parts carried by a selected ancestor, and never
    /// settle a part onto its own children.
    pub children: Query<'w, 's, &'static Children>,
    pub parents: Query<'w, 's, &'static ChildOf>,
    pub parent_globals: Query<'w, 's, &'static GlobalTransform, Without<crate::selection_box::Selected>>,
}

fn handle_nudge_keys(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut timer: ResMut<NudgeTimer>,
    settings: Option<Res<crate::editor_settings::EditorSettings>>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    mut ctx: NudgeContext,
) {
    // Block when text input focused or overlay has focus
    if ui_focus.as_ref().map(|f| f.text_input_focused).unwrap_or(false) { return; }
    if crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) { return; }

    let snap = settings.as_ref().map(|s| if s.snap_enabled { s.snap_size } else { 1.0 }).unwrap_or(1.0);

    // Re-borrow through `ResMut`'s Deref *once* into a plain `&mut NudgeTimer`
    // so the borrow checker can see `lift_held` / `lift_timer` (and the settle
    // pair) as disjoint field borrows. Going through `ResMut` per-arg makes each
    // deref a separate method call and the disjointness is lost.
    let timer = &mut *timer;

    // Use `just_pressed` for the initial-press fire (guaranteed deterministic)
    // and a held-duration timer for auto-repeat. Earlier revisions used
    // `pressed` + a boolean gate which silently produced no fire on some
    // platforms when `pressed` and `just_pressed` raced inside the same frame.
    let lift_fire = nudge_should_fire(
        &keys, KeyCode::Minus, &time,
        &mut timer.lift_held, &mut timer.lift_timer,
    );
    let settle_fire = nudge_should_fire(
        &keys, KeyCode::Equal, &time,
        &mut timer.settle_held, &mut timer.settle_timer,
    );

    if !lift_fire && !settle_fire {
        return;
    }
    let before: Vec<(Entity, Vec3, Quat)> = ctx
        .selected
        .iter()
        .map(|(e, t, _, _)| (e, t.translation, t.rotation))
        .collect();
    if lift_fire {
        lift_selection(&mut ctx, snap);
    }
    if settle_fire {
        settle_selection(&mut ctx, snap);
    }
    record_nudge(&mut ctx, &before, if lift_fire { "Lift" } else { "Settle" });
}

/// Record a nudge as an undo step. A held key repeats every few frames;
/// the repeats fold into the one step (`push_coalesced`), so one Ctrl+Z
/// undoes the whole hold, however long.
fn record_nudge(ctx: &mut NudgeContext, before: &[(Entity, Vec3, Quat)], verb: &str) {
    let mut old_transforms = Vec::new();
    let mut new_transforms = Vec::new();
    for (entity, pos, rot) in before {
        let Ok((_, t, _, _)) = ctx.selected.get(*entity) else { continue };
        if (t.translation - *pos).length() > 1e-5 || t.rotation.angle_between(*rot) > 1e-5 {
            old_transforms.push((entity.to_bits(), pos.to_array(), rot.to_array()));
            new_transforms.push((entity.to_bits(), t.translation.to_array(), t.rotation.to_array()));
        }
    }
    let n = old_transforms.len();
    if n == 0 {
        return;
    }
    ctx.undo.push_coalesced(
        "nudge",
        format!("{} {} object{}", verb, n, if n == 1 { "" } else { "s" }),
        crate::undo::Action::TransformEntities { old_transforms, new_transforms },
    );
}

/// True when an ancestor of `entity` is also selected: it moves with that
/// ancestor and must not be nudged a second time.
fn carried_by_selection(
    entity: Entity,
    selected: &std::collections::HashSet<Entity>,
    parents: &Query<&ChildOf>,
) -> bool {
    let mut current = entity;
    while let Ok(child_of) = parents.get(current) {
        let parent = child_of.parent();
        if selected.contains(&parent) {
            return true;
        }
        current = parent;
    }
    false
}

/// Write a world-space position for `entity` back through its parent.
fn place_world(ctx: &mut NudgeContext, entity: Entity, world_pos: Vec3, world_rot: Quat) {
    let parent_gt = ctx
        .parents
        .get(entity)
        .ok()
        .and_then(|c| ctx.parent_globals.get(c.parent()).ok());
    let (local_pos, _) = crate::math_utils::world_to_local_pose(parent_gt, world_pos, world_rot);
    if let Ok((_, mut t, _, _)) = ctx.selected.get_mut(entity) {
        t.translation = local_pos;
    }
}

/// Returns `true` on the frame the key is first pressed, then again
/// every [`NUDGE_REPEAT_SECS`] after [`NUDGE_DELAY_SECS`] of being held.
/// Resets when the key is released.
fn nudge_should_fire(
    keys: &ButtonInput<KeyCode>,
    key: KeyCode,
    time: &Time,
    held: &mut bool,
    timer: &mut f32,
) -> bool {
    if keys.just_pressed(key) {
        *held = true;
        *timer = 0.0;
        return true;
    }
    if !keys.pressed(key) {
        *held = false;
        *timer = 0.0;
        return false;
    }
    if !*held {
        // Key was already down when we started observing it (focus took
        // control mid-press). Treat the next press as the initial.
        return false;
    }
    *timer += time.delta_secs();
    if *timer >= NUDGE_DELAY_SECS {
        *timer -= NUDGE_REPEAT_SECS;
        return true;
    }
    false
}

/// Simple lift: every selected entity moves up by `snap` in WORLD +Y,
/// written back through its parent, so a part inside a rotated Model still
/// rises straight up. Parts carried by a selected ancestor move with it.
fn lift_selection(ctx: &mut NudgeContext, snap: f32) {
    let selected: std::collections::HashSet<Entity> = ctx.selected.iter().map(|(e, ..)| e).collect();
    let moves: Vec<(Entity, Vec3, Quat)> = ctx
        .selected
        .iter()
        .filter(|(e, ..)| !carried_by_selection(*e, &selected, &ctx.parents))
        .map(|(e, _, gt, _)| {
            let world = gt.compute_transform();
            (e, world.translation + Vec3::Y * snap, world.rotation)
        })
        .collect();
    for (entity, world_pos, world_rot) in moves {
        place_world(ctx, entity, world_pos, world_rot);
    }
}

/// Per-press incremental drop with surface-snap. Each `+` press lowers
/// every selected part by one snap unit on +Y; if a support surface
/// sits within that snap distance directly below the part, the part
/// flushes onto the surface instead (so it doesn't pass through).
///
/// This is the reverse of [`lift_selection`]: simple, predictable, one
/// snap step per fire. The surface-flush only kicks in for the final step
/// that would otherwise land *inside or below* a real surface — so
/// holding `+` keeps the part stepping down through empty air, then
/// "clicks" onto the first surface it reaches.
fn settle_selection(ctx: &mut NudgeContext, snap: f32) {
    use bevy::math::Dir3;

    // Snapshot the selected set before mutating — we need to call
    // `ctx.spatial` while still being able to write back to
    // `ctx.selected`, and Bevy queries don't allow that simultaneously.
    let selected: std::collections::HashSet<Entity> = ctx.selected.iter().map(|(e, ..)| e).collect();
    // The downward ray must never find the part itself: its own children
    // (a MeshPart's mesh node, a Model's parts) are excluded with it, or a
    // settle "lands" on the part's own geometry and goes nowhere.
    let moving = crate::math_utils::moving_set(selected.iter().copied(), &ctx.children);
    let filter = avian3d::prelude::SpatialQueryFilter::default()
        .with_excluded_entities(moving.iter().copied());
    let mut snapshot: Vec<(Entity, Vec3, Quat, f32)> = Vec::new();
    for (entity, _, gt, bp) in ctx.selected.iter() {
        if carried_by_selection(entity, &selected, &ctx.parents) {
            continue;
        }
        let world = gt.compute_transform();
        // Half the part's height as it stands, rotation included.
        let half_height = bp
            .map(|b| {
                let (lo, hi) = crate::math_utils::calculate_rotated_aabb(world.translation, b.size * 0.5, world.rotation);
                (hi.y - lo.y) * 0.5
            })
            .unwrap_or(0.5);
        snapshot.push((entity, world.translation, world.rotation, half_height));
    }

    for (entity, center, rotation, half_height) in snapshot {
        // Cast straight down from the part's current center so we can
        // see how far the support surface is below the part's bottom.
        let support_y_world = {
            let Ok(down) = Dir3::new(Vec3::NEG_Y) else { continue };
            let hits = ctx.spatial.ray_hits(center, down, 10_000.0, 16, true, &filter);
            // The nearest surface below (hits arrive in no particular
            // order), or `None` if the part is hovering over empty space.
            hits.into_iter()
                .map(|h| h.distance)
                .filter(|d| d.is_finite())
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|d| center.y - d)
        };

        // Where the part *would* land if we just stepped down by `snap`.
        let stepped_center_y = center.y - snap;

        // If a surface is within the step distance below the bottom of
        // the part, snap the bottom flush onto that surface; otherwise
        // take the full snap step.
        let new_center_y = match support_y_world {
            Some(sy) => {
                let surface_aligned_center_y = sy + half_height;
                // The surface is "in range" when the proposed step would
                // either touch it or pass through it.
                if surface_aligned_center_y >= stepped_center_y {
                    surface_aligned_center_y
                } else {
                    stepped_center_y
                }
            }
            None => stepped_center_y,
        };

        // World-space target, written back through the parent.
        place_world(ctx, entity, Vec3::new(center.x, new_center_y, center.z), rotation);
    }
}

// ============================================================================
// Regression guards
// ============================================================================
//
// Everything here runs against plain data — no Bevy `App`, no window, no
// asset server — so `cargo test -p eustress-engine keybindings` is instant
// and works headless in CI.
//
// There is deliberately NO `every_action_has_a_handler_arm` test: a `match`
// in `handle_menu_action_events` is not introspectable from Rust without a
// proc-macro that owns the arm list, and a macro that generated the arms
// would make the handler far harder to read than the bug it prevents. The
// catch-all `_ => {}` there carries a comment naming every action that
// deliberately falls through and every action that is still missing one.

#[cfg(test)]
mod tests {
    use super::*;

    /// Two different actions on the same chord means one of them is dead:
    /// `dispatch_keyboard_shortcuts` returns on the first match, so the
    /// loser never fires and there is nothing on screen to explain why.
    /// Primaries and alternates share one namespace because `check()`
    /// tests both. The namespace is per [`ActionContext`]: a terrain chord on
    /// a global chord's key is the context's override, not a clash.
    #[test]
    fn no_duplicate_default_bindings() {
        let kb = KeyBindings::default();
        let mut seen: HashMap<(ActionContext, KeyBinding), Action> = HashMap::new();

        let all = kb
            .bindings
            .iter()
            .map(|(a, b)| (*a, b.clone()))
            .chain(kb.alternates.iter().flat_map(|(a, v)| {
                let action = *a;
                v.iter().map(move |b| (action, b.clone()))
            }));

        for (action, binding) in all {
            if let Some(previous) = seen.insert((action.context(), binding.clone()), action) {
                assert_eq!(
                    previous, action,
                    "duplicate default keybinding {}: it is claimed by both \
                     Action::{:?} (\"{}\") and Action::{:?} (\"{}\"). \
                     Only the one the dispatch loop reaches first will ever fire.",
                    binding.display(),
                    previous,
                    previous.name(),
                    action,
                    action.name(),
                );
            }
        }
    }

    /// An action in [`DISPATCHED_ACTIONS`] with no default binding is
    /// unreachable from the keyboard forever — `bindings.check()` just
    /// returns false and nothing logs. `Action::ToggleAnchor` shipped in
    /// that state; this test is what stops the next one.
    ///
    /// If an action is intentionally chordless, add it to
    /// [`ALLOWED_UNBOUND`] with a reason rather than loosening this test.
    #[test]
    fn every_dispatched_action_is_bound() {
        let kb = KeyBindings::default();
        for action in DISPATCHED_ACTIONS.iter().chain(TERRAIN_ACTIONS).copied() {
            if ALLOWED_UNBOUND.contains(&action) {
                assert!(
                    kb.get(action).is_none(),
                    "Action::{:?} (\"{}\") is listed in ALLOWED_UNBOUND but now HAS a \
                     default binding ({}). Remove it from ALLOWED_UNBOUND.",
                    action,
                    action.name(),
                    kb.get(action).map(|b| b.display()).unwrap_or_default(),
                );
                continue;
            }
            assert!(
                kb.get(action).is_some(),
                "Action::{:?} (\"{}\") is dispatched every frame but has no default \
                 binding, so it can never fire. Give it one in KeyBindings::default(), \
                 or add it to ALLOWED_UNBOUND with a comment saying why it has none.",
                action,
                action.name(),
            );
        }
    }

    /// The File dropdown in `ribbon.slint` PRINTS these chords next to its
    /// items. When the binding table disagrees the menu lies: Ctrl+Shift+N
    /// used to open the network panel and Ctrl+Shift+S used to select
    /// siblings, while the menu advertised New Universe and Save Space As.
    #[test]
    fn file_shortcuts_match_the_ribbon_labels() {
        let kb = KeyBindings::default();
        let printed = [
            (Action::NewUniverse, "Ctrl+Shift+N"),
            (Action::NewSpace, "Ctrl+N"),
            (Action::OpenFile, "Ctrl+O"),
            (Action::SaveScene, "Ctrl+S"),
            (Action::SaveSceneAs, "Ctrl+Shift+S"),
            (Action::PublishUniverse, "Ctrl+P"),
            (Action::PublishSpace, "Ctrl+Shift+P"),
        ];
        for (action, label) in printed {
            let actual = kb.get(action).map(|b| b.display()).unwrap_or_default();
            assert_eq!(
                actual, label,
                "ribbon.slint prints \"{}\" next to \"{}\" but the binding table says \"{}\"",
                label,
                action.name(),
                actual,
            );
        }
    }

    /// `Ctrl+Shift+Z` is redo everywhere outside Windows-only apps, and
    /// [`KeyBinding::matches`] demands exact modifier equality — so the
    /// second convention only works if it exists as a real alternate.
    #[test]
    fn redo_answers_to_both_conventions() {
        let kb = KeyBindings::default();
        assert_eq!(
            kb.get(Action::Redo).map(|b| b.display()).unwrap_or_default(),
            "Ctrl+Y",
            "the ribbon renders the PRIMARY binding; Edit ▸ Redo prints Ctrl+Y",
        );
        assert!(
            kb.alternates(Action::Redo)
                .contains(&KeyBinding::new(KeyCode::KeyZ).with_ctrl().with_shift()),
            "Ctrl+Shift+Z is missing from Redo's alternates — it would be a silent no-op",
        );
    }

    /// `rebind` must refuse a chord another action owns and name the
    /// offender, so the settings dialog can say which one. The rejection
    /// path returns before touching `self` or calling `save()`, which also
    /// keeps this test off the user's real config file.
    #[test]
    fn rebind_refuses_a_chord_another_action_owns() {
        let mut kb = KeyBindings::default();
        let ctrl_g = KeyBinding::new(KeyCode::KeyG).with_ctrl(); // Group

        let err = kb
            .rebind(Action::FocusSelection, ctrl_g.clone())
            .expect_err("Ctrl+G already belongs to Group — rebinding onto it must fail");
        assert!(
            err.contains("Group"),
            "the error must name the conflicting action so the dialog can show it, got: {err}",
        );

        // Rejected means rejected — the table is untouched.
        assert_eq!(kb.get(Action::Group), Some(&ctrl_g));
        assert_eq!(kb.get(Action::FocusSelection), Some(&KeyBinding::new(KeyCode::KeyF)));

        // An alternate is just as much a claim as a primary.
        let ctrl_shift_z = KeyBinding::new(KeyCode::KeyZ).with_ctrl().with_shift();
        let err = kb
            .rebind(Action::FocusSelection, ctrl_shift_z)
            .expect_err("Ctrl+Shift+Z is Redo's alternate — rebinding onto it must fail");
        assert!(err.contains("Redo"), "expected the Redo alternate to be reported, got: {err}");
    }
}

#[cfg(test)]
mod preset_tests {
    use super::*;

    /// Two actions on one chord in one context means one of them is dead; a
    /// preset must never ship that way.
    #[test]
    fn presets_have_no_chord_owned_twice() {
        for preset in KeymapPreset::ALL {
            let map = preset.bindings();
            let chords = map.all_chords();
            for (i, (a, chord)) in chords.iter().enumerate() {
                for (b, other) in chords.iter().skip(i + 1) {
                    assert!(
                        !(chord == other && a != b && a.context() == b.context()),
                        "{:?}: {} is bound to both {:?} and {:?}",
                        preset, chord.display(), a, b
                    );
                }
            }
        }
    }

    /// The terrain tools take `1`-`7` in their own context: the global snap
    /// keys keep their chords, a terrain chord may share a global one, and a
    /// clash inside the terrain context is still refused.
    #[test]
    fn terrain_chords_live_in_their_own_context() {
        let mut map = KeyBindings::default();
        assert_eq!(map.get(Action::TerrainDraw), Some(&KeyBinding::new(KeyCode::Digit1)));
        assert_eq!(map.get(Action::SnapMode1), Some(&KeyBinding::new(KeyCode::Digit1)));
        assert_eq!(Action::TerrainDraw.context(), ActionContext::Terrain);
        assert_eq!(Action::TerrainTools.context(), ActionContext::Global, "T enters the tools from anywhere");
        assert_eq!(Action::SnapMode1.context(), ActionContext::Global);
        // A terrain chord on a global chord's key is allowed...
        assert!(map.claimant(&KeyBinding::new(KeyCode::KeyF), Action::TerrainSnap).is_none());
        // ...a clash within the terrain context is not.
        assert_eq!(
            map.claimant(&KeyBinding::new(KeyCode::Digit2), Action::TerrainSnap),
            Some(Action::TerrainSculpt)
        );
        map.bindings.insert(Action::TerrainSnap, KeyBinding::new(KeyCode::KeyF));
        assert_eq!(map.get(Action::FocusSelection), Some(&KeyBinding::new(KeyCode::KeyF)), "Focus keeps F");
    }

    #[test]
    fn roblox_preset_moves_the_tools_and_play_keys() {
        let map = KeymapPreset::Roblox.bindings();
        assert_eq!(map.get(Action::MoveTool), Some(&KeyBinding::new(KeyCode::Digit2).with_shift()));
        assert_eq!(map.get(Action::PlaySolo), Some(&KeyBinding::new(KeyCode::F8)));
        assert_eq!(map.get(Action::StopPlay), Some(&KeyBinding::new(KeyCode::F5).with_shift()));
        assert_eq!(map.get(Action::InsertObject), Some(&KeyBinding::new(KeyCode::KeyI).with_ctrl()));
        assert_eq!(map.preset(), "roblox");
    }

    #[test]
    fn eustress_preset_is_the_default_map() {
        let map = KeymapPreset::Eustress.bindings();
        assert_eq!(map.get(Action::MoveTool), Some(&KeyBinding::new(KeyCode::KeyX).with_alt()));
        assert_eq!(map.preset(), "eustress");
    }
}

#[cfg(test)]
mod play_allowlist_tests {
    use super::*;
    use crate::play_mode::PlayModeState;

    /// Run the shortcut dispatcher once in `state` with `keys` held, and
    /// return the actions it sent and the tool afterwards.
    fn dispatch(state: PlayModeState, keys: &[KeyCode]) -> (Vec<Action>, crate::ui::Tool) {
        let mut app = App::new();
        app.add_message::<crate::ui::MenuActionEvent>();
        app.insert_resource(State::new(state));
        app.insert_resource(KeyBindings::default());
        app.insert_resource(crate::ui::StudioState::default());
        let mut input = ButtonInput::<KeyCode>::default();
        for key in keys {
            input.press(*key);
        }
        app.insert_resource(input);
        app.add_systems(Update, dispatch_keyboard_shortcuts);
        app.update();
        let sent = app
            .world_mut()
            .resource_mut::<Messages<crate::ui::MenuActionEvent>>()
            .drain()
            .map(|event| event.action)
            .collect();
        let tool = app.world().resource::<crate::ui::StudioState>().current_tool;
        (sent, tool)
    }

    /// The editor keys a game commonly binds reach the game: during Play
    /// Studio acts on none of them.
    #[test]
    fn play_leaves_editor_keys_to_the_game() {
        let start_tool = crate::ui::StudioState::default().current_tool;
        let presses: [&[KeyCode]; 5] = [
            &[KeyCode::KeyF],
            &[KeyCode::Delete],
            &[KeyCode::AltLeft, KeyCode::KeyZ],
            &[KeyCode::Equal],
            &[KeyCode::Digit1],
        ];
        for keys in presses {
            let (sent, tool) = dispatch(PlayModeState::Playing, keys);
            assert!(sent.is_empty(), "{keys:?} sent {sent:?} during Play");
            assert_eq!(tool, start_tool, "{keys:?} switched the tool during Play");
        }
    }

    /// The same F frames the selection while editing, so the Play test above
    /// is not passing because F does nothing anywhere.
    #[test]
    fn editing_keeps_the_editor_keys() {
        let (sent, _) = dispatch(PlayModeState::Editing, &[KeyCode::KeyF]);
        assert_eq!(sent, vec![Action::FocusSelection]);
    }

    /// The session controls stay live during Play.
    #[test]
    fn play_keeps_the_session_controls() {
        let (sent, _) = dispatch(PlayModeState::Playing, &[KeyCode::F8]);
        assert_eq!(sent, vec![Action::StopPlay]);
        assert!(PLAY_SESSION_ACTIONS.contains(&Action::StopPlay));
        assert!(!PLAY_SESSION_ACTIONS.contains(&Action::FocusSelection));
        assert!(!PLAY_SESSION_ACTIONS.contains(&Action::Delete));
    }
}

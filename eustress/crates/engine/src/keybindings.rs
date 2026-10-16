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
    
    /// True when the held modifiers are exactly this chord's.
    fn modifiers_match(&self, keys: &ButtonInput<KeyCode>) -> bool {
        let ctrl_pressed = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let alt_pressed = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
        let shift_pressed = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        self.ctrl == ctrl_pressed && self.alt == alt_pressed && self.shift == shift_pressed
    }

    /// The chord went down this frame.
    pub fn matches(&self, keys: &ButtonInput<KeyCode>) -> bool {
        self.modifiers_match(keys) && keys.just_pressed(self.key)
    }

    /// The chord is down now: what an auto-repeating action follows after
    /// the press that [`KeyBinding::matches`] reports.
    pub fn held(&self, keys: &ButtonInput<KeyCode>) -> bool {
        self.modifiers_match(keys) && keys.pressed(self.key)
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
        // Ctrl+Shift+F searches the text of every script in the Space, as Roblox
        // Studio's Find All does. The Assets panel sat on this chord until it
        // moved to Ctrl+4.
        bindings.insert(Action::FindInScripts, KeyBinding::new(KeyCode::KeyF).with_ctrl().with_shift());
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
        // Look picker: bare I samples the look under the cursor in any tool
        // (the terrain tools keep their own I, which does the same); the
        // bucket paints parts with it.
        bindings.insert(Action::SampleLook, KeyBinding::new(KeyCode::KeyI));
        bindings.insert(Action::ToolPaintBucket, KeyBinding::new(KeyCode::KeyB).with_ctrl().with_alt());
        
        // View shortcuts
        bindings.insert(Action::ToggleExplorer, KeyBinding::new(KeyCode::Digit1).with_ctrl());
        bindings.insert(Action::ToggleProperties, KeyBinding::new(KeyCode::Digit2).with_ctrl());
        bindings.insert(Action::ToggleOutput, KeyBinding::new(KeyCode::Digit3).with_ctrl());
        
        // Window shortcuts
        bindings.insert(Action::ToggleCommandBar, KeyBinding::new(KeyCode::KeyK).with_ctrl());
        // Ctrl+4 completes the Ctrl+1/2/3 panel row and is what the View menu
        // prints next to Assets. Assets sat on Ctrl+Shift+F until that chord
        // became Find in Scripts; a keybindings.ron saved with the old chord is
        // brought forward in `KeyBindings::retire_old_defaults`.
        bindings.insert(Action::ToggleAssets, KeyBinding::new(KeyCode::Digit4).with_ctrl());
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
        // "+" is Shift+= on most layouts; the old raw key read accepted it.
        alternates.entry(Action::SettleSelection).or_default()
            .push(KeyBinding::new(KeyCode::Equal).with_shift());

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

    /// True while `action`'s primary binding OR any alternate is held down.
    /// [`KeyBindings::check`] is the one-frame press; this is what an
    /// auto-repeating action (Lift, Settle) follows after it.
    pub fn held(&self, action: Action, keys: &ButtonInput<KeyCode>) -> bool {
        if self.get(action).map(|binding| binding.held(keys)).unwrap_or(false) {
            return true;
        }
        self.alternates(action).iter().any(|binding| binding.held(keys))
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
        Self::from_saved(&contents)
    }

    /// A saved map as this build reads it: parsed, brought forward from the
    /// defaults it was saved under, then topped up with the actions it predates.
    fn from_saved(contents: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut bindings: Self = ron::from_str(contents)?;
        bindings.retire_old_defaults();

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

impl KeyBindings {
    /// Drops a chord a saved map holds only because it was once the default.
    ///
    /// A saved `keybindings.ron` is a whole snapshot of the map, so it keeps the
    /// default of the day for every action. When a default chord is given to
    /// another action, the saved map would put both on it, and whichever the
    /// dispatch reaches first would answer. Ctrl+Shift+F was the Assets toggle's
    /// default and is Find in Scripts now: a saved map that still has Assets on
    /// it loses that entry, and the defaults merged in afterwards give Assets
    /// Ctrl+4. A map that moved Assets to another chord keeps its choice.
    fn retire_old_defaults(&mut self) {
        let old_assets = KeyBinding::new(KeyCode::KeyF).with_ctrl().with_shift();
        if self.bindings.get(&Action::ToggleAssets) == Some(&old_assets) {
            self.bindings.remove(&Action::ToggleAssets);
        }
        if let Some(alternates) = self.alternates.get_mut(&Action::ToggleAssets) {
            alternates.retain(|chord| *chord != old_assets);
        }
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
    Action::SampleLook, Action::ToolPaintBucket,
    Action::InsertObject, Action::PasteInto, Action::FindReplace, Action::FindInScripts,
    Action::FocusExplorerSearch, Action::FocusPropertiesFilter,
    Action::TerrainTools,
];

/// The actions that stay live during a Play session, the only ones
/// [`play_refusal`] lets through: the session controls, the panel toggles,
/// Find in Scripts, and the actions that only read the scene. Copy fills the
/// clipboard, the selection commands change the selection, and the Explorer
/// search, the Properties filter and the World/Local toggle change Studio's
/// own state; none of them writes the scene or the Space's files. Everything
/// else in [`DISPATCHED_ACTIONS`] edits or persists the scene, so it waits for
/// Stop, and its keys belong to the running game: F, Delete, the tool keys and
/// the rest reach the game's input unprocessed, as in Roblox Studio
/// (`play_allowlist_tests`).
const PLAY_SESSION_ACTIONS: &[Action] = &[
    Action::PlayWithCharacter, Action::PauseResume, Action::PlaySolo, Action::StopPlay,
    Action::ToggleExplorer, Action::ToggleProperties, Action::ToggleOutput,
    Action::ToggleCommandBar, Action::ToggleAssets, Action::FindInScripts,
    Action::Copy,
    Action::SelectAll,
    Action::SelectChildren,
    Action::SelectDescendants,
    Action::SelectParent,
    Action::SelectSiblings,
    Action::InvertSelection,
    Action::FocusExplorerSearch,
    Action::FocusPropertiesFilter,
    Action::ToggleTransformSpace,
];

/// What a Play session answers when `action` is asked for: `None` to run it,
/// or the notice that says it waits for Stop. The one rule for every path
/// that can run an action: the keyboard dispatcher, the menu and button
/// handler, Save, Save As, Publish and the History handlers all ask it, so a
/// key, a menu item and a button give the same answer. While editing
/// everything runs; during Play and Pause only [`PLAY_SESSION_ACTIONS`] does,
/// because anything else would edit or persist the scene the game is running
/// in, and Stop puts the authored scene back. A reader of `MenuActionEvent`
/// that runs an action itself asks it too, or runs outside Play only.
pub(crate) fn play_refusal(state: crate::play_mode::PlayModeState, action: Action) -> Option<String> {
    if state == crate::play_mode::PlayModeState::Editing || PLAY_SESSION_ACTIONS.contains(&action) {
        return None;
    }
    let notice = match action {
        Action::SaveScene => "Stop Play to save the Space. Saving during Play would write the running scene to disk.",
        Action::SaveSceneAs => "Stop Play to save the Space under a new name. Saving during Play would write the running scene to disk.",
        Action::PublishUniverse | Action::PublishSpace => "Stop Play to publish. Publishing during Play would publish the running scene.",
        Action::Undo => "Stop Play to undo. Undo works on the authored scene.",
        Action::Redo => "Stop Play to redo. Redo works on the authored scene.",
        Action::Cut => "Stop Play to cut.",
        Action::Paste | Action::PasteInto => "Stop Play to paste.",
        Action::Duplicate => "Stop Play to duplicate.",
        Action::Delete => "Stop Play to delete.",
        Action::Group => "Stop Play to group.",
        Action::Ungroup => "Stop Play to ungroup.",
        other => return play_refusal_to(state, &format!("use {}", other.name())),
    };
    Some(notice.to_string())
}

/// [`play_refusal`] for a request that is not an editor [`Action`]: a Studio
/// plugin's button, an Explorer insert or rename, a viewpoint delete. Like
/// every action outside [`PLAY_SESSION_ACTIONS`] it waits for Stop, and its
/// notice reads "Stop Play to `what`.".
pub(crate) fn play_refusal_to(state: crate::play_mode::PlayModeState, what: &str) -> Option<String> {
    (state != crate::play_mode::PlayModeState::Editing).then(|| format!("Stop Play to {what}."))
}

/// [`play_refusal`] for code that holds the `World`: Save, Save As, Publish,
/// the History handlers and the Delete that Remove Road runs. A refusal is
/// said ([`say_play_refusal`]) and returns `true`, and the caller returns
/// before it writes anything.
pub(crate) fn refuse_during_play(world: &mut World, action: Action) -> bool {
    let state = world
        .get_resource::<State<crate::play_mode::PlayModeState>>()
        .map_or(crate::play_mode::PlayModeState::Editing, |s| *s.get());
    let Some(notice) = play_refusal(state, action) else { return false };
    info!("{} refused during Play", action.name());
    say_play_refusal(world, notice);
    true
}

/// Say a Play refusal where the user sees it: a warning toast, and the same
/// line as a warning in the Output panel, which keeps it after the toast has
/// gone. Every refusal of the Play gate is said here; a system queues it with
/// its commands.
pub(crate) fn say_play_refusal(world: &mut World, notice: String) {
    if let Some(mut output) = world.get_resource_mut::<crate::ui::OutputConsole>() {
        output.warn(notice.clone());
    }
    if let Some(mut notifications) = world.get_resource_mut::<crate::notifications::NotificationManager>() {
        notifications.warning(notice);
    }
}

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
    // Which terrain tool is current: Region's copy, cut, paste, duplicate and
    // delete chords belong to it alone (see `TerrainTools` below).
    terrain_brush: Option<Res<eustress_common::terrain::TerrainBrush>>,
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

    // While a Play session runs, the keyboard belongs to the game: only what
    // `play_refusal` lets through stays live (the session controls, the panel
    // toggles and the actions that only read the scene). Delete, Undo, tool
    // switching, snap modes, Save and the rest would edit (or persist) the
    // scene the game is playing in, so their keys go to the game, with no
    // notice: the game may use them.
    let play_state = play_state.map_or(crate::play_mode::PlayModeState::Editing, |s| *s.get());
    if play_state != crate::play_mode::PlayModeState::Editing {
        for action in DISPATCHED_ACTIONS.iter().copied() {
            if bindings.check(action, &keys) && play_refusal(play_state, action).is_none() {
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
        // Ctrl C, X, V, D and Delete are Region's edits only while Region is
        // the tool. With any other terrain tool current nothing consumes
        // them, so they used to vanish: a layer or spline point selected in
        // the Explorer could not be copied, duplicated or deleted until the
        // terrain tools were closed. They fall through to the editor's own
        // chords then.
        let region_is_tool = terrain_brush
            .as_deref()
            .is_some_and(|brush| brush.tool == eustress_common::terrain::TerrainTool::Region);
        for action in TERRAIN_ACTIONS.iter().copied() {
            let region_edit = matches!(
                action,
                Action::TerrainRegionCopy
                    | Action::TerrainRegionCut
                    | Action::TerrainRegionPaste
                    | Action::TerrainRegionDuplicate
                    | Action::TerrainRegionDelete
            );
            if region_edit && !region_is_tool {
                continue;
            }
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

/// Whether [`handle_menu_action_events`] states the Play refusal of `action`:
/// for the actions it runs itself (keep this in step with its arms), and for
/// Group and Ungroup, whose handler (`grouping.rs`) runs outside Play only.
/// Either way the action does not run during Play, so the notice is true.
/// Every other action is answered by its own reader of the same event, which
/// states its own refusal (the CSG and mesh split readers do) or decides what
/// Play means for it (the server, publish, view and terrain readers); a
/// notice from here would speak for them.
fn play_refusal_said_here(action: Action) -> bool {
    matches!(
        action,
        Action::SelectTool
            | Action::MoveTool
            | Action::ScaleTool
            | Action::RotateTool
            | Action::ToggleTransformSpace
            | Action::SaveScene
            | Action::NewSpace
            | Action::NewUniverse
            | Action::OpenFile
            | Action::SaveSceneAs
            | Action::Undo
            | Action::Redo
            | Action::ToggleExplorer
            | Action::ToggleProperties
            | Action::ToggleOutput
            | Action::ToggleAssets
            | Action::Copy
            | Action::Cut
            | Action::Paste
            | Action::ToggleCommandBar
            | Action::FocusSelection
            | Action::SnapMode1
            | Action::SnapMode2
            | Action::SnapModeOff
            | Action::Delete
            | Action::SelectAll
            | Action::Duplicate
            | Action::SelectChildren
            | Action::SelectDescendants
            | Action::SelectParent
            | Action::SelectSiblings
            | Action::InvertSelection
            | Action::InsertObject
            | Action::PasteInto
            | Action::FindReplace
            | Action::FindInScripts
            | Action::FocusExplorerSearch
            | Action::FocusPropertiesFilter
            | Action::ToggleCollisions
            | Action::ToggleAnchor
            | Action::LockSelection
            | Action::UnlockSelection
            | Action::ToolPartSwap
            | Action::ToolEdgeAlign
            | Action::ToolModelReflect
            | Action::ToolGapFill
            | Action::ToolResizeAlign
            | Action::ToolMaterialFlip
            | Action::ToolLinearArray
            | Action::ToolRadialArray
            | Action::ToolGridArray
            | Action::ToolPathArray
            | Action::ToolPaintBucket
            | Action::Group
            | Action::Ungroup
    )
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
    // Declared as `DeletableQuery`, the type the Delete takes
    // (`delete_selected`): written out here with elided lifetimes, its item
    // references would borrow for this function's own lifetimes, and `Query`
    // is invariant, so it could not be passed as one (E0521).
    mut entity_query: DeletableQuery,
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
    // Anchor / Lock / Unlock arms stamp their TOML writes with, and the
    // Play state every action is checked against (`play_refusal`). Bundled
    // for the same reason the two tuples above are: this system already
    // sits on Bevy's 16-parameter ceiling, so a 17th argument does not
    // compile.
    mut modal_and_auth: (
        MessageWriter<crate::modal_tool::ActivateModalToolEvent>,
        Option<Res<crate::auth::AuthState>>,
        Option<Res<State<crate::play_mode::PlayModeState>>>,
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
    let (ref mut activate_modal_tool, ref auth, ref play_state) = modal_and_auth;
    let play_state = play_state.as_ref().map_or(crate::play_mode::PlayModeState::Editing, |s| *s.get());
    let Some(mut studio_state) = studio_state else { return };

    // The Play refusals stated this frame, each once.
    let mut refusals: Vec<String> = Vec::new();
    for event in events.read() {
        // During Play only what `play_refusal` allows runs here. The notice is
        // stated for the actions this handler runs, and for Group and
        // Ungroup, whose own handler runs outside Play only, so the refusal it
        // states is real. Every other action has its own reader of this
        // event, which answers for it during Play.
        if let Some(notice) = play_refusal(play_state, event.action) {
            if play_refusal_said_here(event.action) && !refusals.contains(&notice) {
                info!("{} refused during Play", event.action.name());
                refusals.push(notice);
            }
            continue;
        }
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
            // Assets has no StudioState flag (its visibility lives in Slint
            // only), so the toggle is a pulse `sync_bevy_to_slint` applies.
            Action::ToggleAssets => {
                studio_state.toggle_assets_pulse = studio_state.toggle_assets_pulse.wrapping_add(1);
            }

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
                    delete_selected(
                        &selected_ids,
                        &mut commands,
                        &entity_query,
                        &instance_query,
                        &instance_file_query,
                        &loaded_from_file_query,
                        file_registry.as_deref_mut(),
                        &mut undo_stack,
                        selection_manager.as_deref(),
                        explorer_state.as_deref_mut(),
                    );
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
            // Find in Scripts only reads: the modal searches the scripts' text
            // and opens a tab, and the only thing this arm writes is the pulse
            // that opens it. That is why it is live during a Play session too.
            Action::FindInScripts => {
                studio_state.open_script_search_pulse = studio_state.open_script_search_pulse.wrapping_add(1);
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
            Action::ToolPaintBucket => {
                activate_modal_tool.write(crate::modal_tool::ActivateModalToolEvent {
                    tool_id: "paint_bucket".to_string(),
                });
            }
            // `SampleLook` is answered by `look_picker::sample_look_on_action`,
            // which reads the same event; an arm here would double-fire.

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

    if !refusals.is_empty() {
        commands.queue(move |world: &mut World| {
            for notice in refusals {
                say_play_refusal(world, notice);
            }
        });
    }
}

/// What a Delete did, for a caller that tells the user what happened: Remove
/// Road, through [`delete_entities_now`].
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct DeleteOutcome {
    /// Refused during Play: nothing was deleted, and the refusal was said.
    pub refused: bool,
    /// The entities it despawned.
    pub removed: Vec<Entity>,
    /// Whether it pushed an undo entry, which it does when anything went to
    /// the trash.
    pub undoable: bool,
}

/// Every object a Delete can reach: the parts and the instances.
type DeletableQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, Option<&'static GlobalTransform>, Option<&'static mut eustress_common::classes::BasePart>),
    Or<(With<crate::rendering::PartEntity>, With<eustress_common::classes::Instance>)>,
>;

/// Delete the objects `selected_ids` names, as the selection names them
/// ("<index>v<generation>"): each one's folder or file goes to the Space's
/// trash, every store forgets it, and its entity is despawned, as one undo
/// entry. Services and the Workshop folder are skipped. The Delete action
/// runs it on the selection, and Remove Road on its road
/// ([`delete_entities_now`]).
#[allow(clippy::too_many_arguments)]
fn delete_selected(
    selected_ids: &std::collections::HashSet<String>,
    commands: &mut Commands,
    entity_query: &DeletableQuery,
    instance_query: &Query<&eustress_common::classes::Instance>,
    instance_file_query: &Query<&crate::space::instance_loader::InstanceFile>,
    loaded_from_file_query: &Query<&crate::space::LoadedFromFile>,
    mut file_registry: Option<&mut crate::space::SpaceFileRegistry>,
    undo_stack: &mut crate::undo::UndoStack,
    selection_manager: Option<&crate::selection_sync::SelectionSyncManager>,
    mut explorer_state: Option<&mut crate::ui::slint_ui::UnifiedExplorerState>,
) -> DeleteOutcome {
    let mut outcome = DeleteOutcome::default();
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
                outcome.removed.push(entity);
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
                // The trash is at the Space root, never beside the deleted
                // item; a taken name gets a timestamp suffix.
                let trash_path = crate::space::trash::trash_destination(&source_path);

                // One shared move (space::trash): guard the watcher, retry
                // the rename, then despawn, purge and unregister every
                // descendant and this record. Rename-to-trash is the ONLY
                // allowed path: there is deliberately no `remove_dir_all`
                // fallback, because a transient handle lock would then
                // delete the folder without a trash entry and undo would
                // have nothing to restore. On failure nothing changed, so
                // the entity stays beside its file and the delete can be
                // retried.
                let request = crate::space::trash::TrashRequest {
                    entity,
                    source: &source_path,
                    record: &toml_path,
                    destination: &trash_path,
                };
                match crate::space::trash::trash_instance(&request, commands, file_registry.as_deref_mut()) {
                    Ok(trashed) => {
                        // Store (source_path, trash_path), NOT toml_path: the
                        // rename moved `source_path` (the folder), so undo must
                        // rename back to `source_path`, not to `_instance.toml`.
                        trashed_paths.push((source_path.clone(), trash_path.clone()));
                        info!(
                            "🗑️ Moved {:?} to trash{}{}",
                            source_path.file_name().unwrap_or_default(),
                            if trashed.retries > 0 { format!(" (after {} retr{})", trashed.retries, if trashed.retries == 1 { "y" } else { "ies" }) } else { String::new() },
                            if trashed.descendants > 0 {
                                format!(" + {} descendant{}", trashed.descendants, if trashed.descendants == 1 { "" } else { "s" })
                            } else { String::new() },
                        );
                    }
                    Err(e) => {
                        warn!(
                            "❌ Could not move {:?} to trash: {}. Leaving file on disk + entity in ECS, retry the delete.",
                            source_path, e
                        );
                        // Skip the ECS despawn so Explorer stays in sync with
                        // the still-present file.
                        continue;
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
                // The trash is at the Space root, never beside the item.
                let trash_path = crate::space::trash::trash_destination(&source_path);

                // The file is its own record. Same shared move as above.
                let request = crate::space::trash::TrashRequest {
                    entity,
                    source: &source_path,
                    record: &source_path,
                    destination: &trash_path,
                };
                match crate::space::trash::trash_instance(&request, commands, file_registry.as_deref_mut()) {
                    Ok(trashed) => {
                        trashed_paths.push((source_path.clone(), trash_path.clone()));
                        info!(
                            "🗑️ Moved script {:?} to trash{}",
                            source_path.file_name().unwrap_or_default(),
                            if trashed.retries > 0 { format!(" (after {} retr{})", trashed.retries, if trashed.retries == 1 { "y" } else { "ies" }) } else { String::new() },
                        );
                    }
                    Err(e) => {
                        warn!(
                            "❌ Could not move {:?} to trash: {}. Leaving file on disk + entity in ECS, retry the delete.",
                            source_path, e
                        );
                        continue;
                    }
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
        outcome.removed.push(entity);
        info!("🗑️ Deleted entity {:?} ({})", entity, id);
    }

    // Push to undo stack so Ctrl+Z can restore
    outcome.undoable = !trashed_paths.is_empty();
    if outcome.undoable {
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
            "🔒 Refused to delete {} protected item{}: Workspace, Lighting, SoulService, etc. are protected scaffolding, and so is a terrain's data folder.",
            skipped_services, if skipped_services == 1 { "" } else { "s" },
        );
        commands.queue(|world: &mut World| {
            if let Some(mut notifications) = world.get_resource_mut::<crate::notifications::NotificationManager>() {
                notifications.warning(
                    "Delete refused: services (Workspace, Lighting, ...) and a terrain's data folder \
                     (Workspace/Terrain) cannot be deleted. Use Clear Terrain in the Terrain ribbon to remove \
                     a terrain, or delete its layers one by one.",
                );
            }
        });
    }
    outcome
}

/// Delete `entities` now, with the world, and say what the Delete did: the
/// Delete action's own work ([`delete_selected`]), for a caller that tells
/// the user at once, Remove Road. Refused during Play, as the Delete key is.
pub(crate) fn delete_entities_now(world: &mut World, entities: &[Entity]) -> DeleteOutcome {
    if refuse_during_play(world, Action::Delete) {
        return DeleteOutcome { refused: true, ..Default::default() };
    }
    let selected_ids: std::collections::HashSet<String> = entities.iter().map(|entity| format!("{}v{}", entity.index(), entity.generation())).collect();
    let mut state: bevy::ecs::system::SystemState<(
        Commands,
        DeletableQuery,
        Query<&eustress_common::classes::Instance>,
        Query<&crate::space::instance_loader::InstanceFile>,
        Query<&crate::space::LoadedFromFile>,
        Option<ResMut<crate::space::SpaceFileRegistry>>,
        ResMut<crate::undo::UndoStack>,
        Option<Res<crate::selection_sync::SelectionSyncManager>>,
        Option<ResMut<crate::ui::slint_ui::UnifiedExplorerState>>,
    )> = bevy::ecs::system::SystemState::new(world);
    let outcome = match state.get_mut(world) {
        Ok((mut commands, entity_query, instance_query, instance_file_query, loaded_from_file_query, mut file_registry, mut undo_stack, selection_manager, mut explorer_state)) => delete_selected(
            &selected_ids,
            &mut commands,
            &entity_query,
            &instance_query,
            &instance_file_query,
            &loaded_from_file_query,
            file_registry.as_deref_mut(),
            &mut undo_stack,
            selection_manager.as_deref(),
            explorer_state.as_deref_mut(),
        ),
        Err(e) => {
            warn!("Delete could not run: {e}");
            return DeleteOutcome::default();
        }
    };
    state.apply(world);
    outcome
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
// reaches. They follow their bindings (`check` for the press, `held` for the
// repeat), so rebinding either action moves the key. The repeat timer lives
// here because the one-shot `just_pressed` dispatch loop deliberately does
// not repeat.

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
    bindings: Option<Res<KeyBindings>>,
    mut ctx: NudgeContext,
) {
    // Block when text input focused or overlay has focus
    if ui_focus.as_ref().map(|f| f.text_input_focused).unwrap_or(false) { return; }
    if crate::ui::slint_ui::OVERLAY_INPUT_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) { return; }
    let Some(bindings) = bindings else { return };

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
        bindings.check(Action::LiftSelection, &keys),
        bindings.held(Action::LiftSelection, &keys),
        &time,
        &mut timer.lift_held, &mut timer.lift_timer,
    );
    let settle_fire = nudge_should_fire(
        bindings.check(Action::SettleSelection, &keys),
        bindings.held(Action::SettleSelection, &keys),
        &time,
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
    just_pressed: bool,
    pressed: bool,
    time: &Time,
    held: &mut bool,
    timer: &mut f32,
) -> bool {
    if just_pressed {
        *held = true;
        *timer = 0.0;
        return true;
    }
    if !pressed {
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

    /// A keybindings.ron saved while Assets sat on Ctrl+Shift+F would keep it
    /// there, on the same chord as Find in Scripts, and the dispatch loop would
    /// answer with whichever comes first. Loading it hands the chord to Find in
    /// Scripts and gives Assets its Ctrl+4 default.
    #[test]
    fn a_saved_map_with_assets_on_ctrl_shift_f_hands_the_chord_to_find_in_scripts() {
        let ctrl_shift_f = KeyBinding::new(KeyCode::KeyF).with_ctrl().with_shift();
        let ctrl_4 = KeyBinding::new(KeyCode::Digit4).with_ctrl();

        // The map as the build before this one saved it: Assets on the old
        // chord with Ctrl+4 as an alternate, and nothing for Find in Scripts.
        let mut old = KeyBindings::default();
        old.bindings.remove(&Action::FindInScripts);
        old.bindings.insert(Action::ToggleAssets, ctrl_shift_f.clone());
        old.alternates.insert(Action::ToggleAssets, vec![ctrl_4.clone()]);
        let saved = ron::to_string(&old).expect("a map serializes");

        let kb = KeyBindings::from_saved(&saved).expect("a saved map loads");
        assert_eq!(kb.get(Action::FindInScripts), Some(&ctrl_shift_f));
        assert_eq!(kb.get(Action::ToggleAssets), Some(&ctrl_4));

        let mut input = ButtonInput::<KeyCode>::default();
        for key in [KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::KeyF] {
            input.press(key);
        }
        assert!(kb.check(Action::FindInScripts, &input));
        assert!(!kb.check(Action::ToggleAssets, &input), "Assets still answers to Ctrl+Shift+F");

        // No chord is left on two actions.
        let chords = kb.all_chords();
        for (i, (a, chord)) in chords.iter().enumerate() {
            for (b, other) in chords.iter().skip(i + 1) {
                assert!(!(chord == other && a != b && a.context() == b.context()), "{} is bound to both {a:?} and {b:?}", chord.display());
            }
        }
    }

    /// A saved map that moved Assets to a chord of its own keeps it.
    #[test]
    fn a_saved_remap_of_assets_survives_the_move() {
        let moved = KeyBinding::new(KeyCode::KeyJ).with_ctrl().with_alt();
        let mut old = KeyBindings::default();
        old.bindings.remove(&Action::FindInScripts);
        old.bindings.insert(Action::ToggleAssets, moved.clone());
        let saved = ron::to_string(&old).expect("a map serializes");

        let kb = KeyBindings::from_saved(&saved).expect("a saved map loads");
        assert_eq!(kb.get(Action::ToggleAssets), Some(&moved));
        assert_eq!(kb.get(Action::FindInScripts), Some(&KeyBinding::new(KeyCode::KeyF).with_ctrl().with_shift()));
    }

    /// The defaults themselves: Find in Scripts on Ctrl+Shift+F, Assets on Ctrl+4.
    #[test]
    fn the_defaults_put_find_in_scripts_on_ctrl_shift_f_and_assets_on_ctrl_4() {
        let kb = KeyBindings::default();
        assert_eq!(kb.get(Action::FindInScripts), Some(&KeyBinding::new(KeyCode::KeyF).with_ctrl().with_shift()));
        assert_eq!(kb.get(Action::ToggleAssets), Some(&KeyBinding::new(KeyCode::Digit4).with_ctrl()));
        assert!(DISPATCHED_ACTIONS.contains(&Action::FindInScripts));
        assert!(PLAY_SESSION_ACTIONS.contains(&Action::FindInScripts));
        assert!(kb.alternates(Action::ToggleAssets).is_empty(), "Assets has one chord now");
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

    /// Every `Action` variant, in declaration order. serde reads a unit
    /// variant from its index, so the list grows with the enum.
    fn every_action() -> Vec<Action> {
        use serde::de::value::{Error, U32Deserializer};
        let all: Vec<Action> = (0u32..).map_while(|i| <Action as serde::Deserialize>::deserialize(U32Deserializer::<Error>::new(i)).ok()).collect();
        // A list that came back short would let the tests pass on nothing.
        assert_eq!(all.first(), Some(&Action::SelectTool));
        assert!(all.len() > 100 && all.contains(&Action::TerrainRegionDelete), "{} actions", all.len());
        all
    }

    /// The writes every path asks about: each waits for Stop during Play and
    /// Pause.
    const WRITE_ACTIONS: [Action; 11] = [
        Action::SaveScene,
        Action::SaveSceneAs,
        Action::Undo,
        Action::Redo,
        Action::Delete,
        Action::Paste,
        Action::Duplicate,
        Action::Group,
        Action::Ungroup,
        Action::PublishUniverse,
        Action::PublishSpace,
    ];

    /// The actions that only read the scene, live during Play.
    const READ_ONLY_IN_PLAY: [Action; 10] = [
        Action::Copy,
        Action::FocusExplorerSearch,
        Action::FocusPropertiesFilter,
        Action::SelectAll,
        Action::SelectChildren,
        Action::SelectDescendants,
        Action::SelectParent,
        Action::SelectSiblings,
        Action::InvertSelection,
        Action::ToggleTransformSpace,
    ];

    #[test]
    fn editing_refuses_nothing() {
        for action in every_action() {
            assert_eq!(play_refusal(PlayModeState::Editing, action), None, "{action:?}");
        }
    }

    #[test]
    fn play_and_pause_refuse_every_write() {
        for state in [PlayModeState::Playing, PlayModeState::Paused] {
            for action in WRITE_ACTIONS {
                let notice = play_refusal(state, action).unwrap_or_else(|| panic!("{state:?} ran {action:?}"));
                assert!(notice.starts_with("Stop Play to "), "{action:?}: {notice}");
            }
        }
    }

    /// The allow-list runs during Play and Pause, the read-only additions
    /// among it, and every other action is refused.
    #[test]
    fn play_runs_the_allow_list_and_refuses_the_rest() {
        for state in [PlayModeState::Playing, PlayModeState::Paused] {
            for action in READ_ONLY_IN_PLAY {
                assert_eq!(play_refusal(state, action), None, "{state:?} refused {action:?}");
            }
            for action in every_action() {
                let runs = play_refusal(state, action).is_none();
                assert_eq!(runs, PLAY_SESSION_ACTIONS.contains(&action), "{state:?} {action:?}");
            }
        }
        // Kept out on purpose: Replace writes, Start Server is the multiplayer
        // owner's call, and these two panels are not this list's to open.
        for action in [Action::FindReplace, Action::StartServer, Action::ToggleCollaboration, Action::ToggleNetworkPanel] {
            assert!(play_refusal(PlayModeState::Playing, action).is_some(), "{action:?}");
        }
    }

    /// A notice names what waits for Stop.
    #[test]
    fn the_notices_say_what_waits_for_stop() {
        let say = |action| play_refusal(PlayModeState::Playing, action).unwrap();
        assert_eq!(say(Action::SaveScene), "Stop Play to save the Space. Saving during Play would write the running scene to disk.");
        assert_eq!(say(Action::Undo), "Stop Play to undo. Undo works on the authored scene.");
        assert_eq!(say(Action::Delete), "Stop Play to delete.");
        assert_eq!(say(Action::ToolGapFill), "Stop Play to use Gap Fill.");
    }

    /// A request that is not an editor action gets the same rule, and the
    /// same sentence as an action's notice.
    #[test]
    fn a_tool_request_waits_for_stop_like_an_action() {
        assert_eq!(play_refusal_to(PlayModeState::Editing, "insert"), None);
        for state in [PlayModeState::Playing, PlayModeState::Paused] {
            assert_eq!(play_refusal_to(state, "insert").as_deref(), Some("Stop Play to insert."));
            assert_eq!(play_refusal_to(state, "use the Road Builder").as_deref(), Some("Stop Play to use the Road Builder."));
            assert_eq!(play_refusal(state, Action::ToolGapFill), play_refusal_to(state, "use Gap Fill"));
        }
    }

    /// The menu handler states a refusal only for what it runs, and for Group
    /// and Ungroup, refused by their own handler. Start Server, CSG, mesh
    /// splits, the publish dialogs, the views and the terrain tools are
    /// answered by their own readers.
    #[test]
    fn the_menu_states_only_its_own_refusals() {
        for action in WRITE_ACTIONS {
            let publish = matches!(action, Action::PublishUniverse | Action::PublishSpace);
            assert_eq!(play_refusal_said_here(action), !publish, "{action:?}");
        }
        for action in [
            Action::StartServer,
            Action::StopServer,
            Action::CSGUnion,
            Action::MeshSplitParts,
            Action::ViewTop,
            Action::TerrainTools,
            Action::SampleLook,
        ] {
            assert!(!play_refusal_said_here(action), "{action:?}");
        }
    }

    /// The keyboard reaches every allowed action during Play: its default
    /// chord, pressed while Play runs, sends exactly that action through the
    /// dispatcher's Play path.
    #[test]
    fn every_play_action_has_a_key_path() {
        let bindings = KeyBindings::default();
        for &action in PLAY_SESSION_ACTIONS {
            assert!(DISPATCHED_ACTIONS.contains(&action), "{action:?}");
            let chord = bindings.get(action).unwrap_or_else(|| panic!("{action:?} has no default chord")).clone();
            let mut keys = vec![chord.key];
            if chord.ctrl {
                keys.push(KeyCode::ControlLeft);
            }
            if chord.alt {
                keys.push(KeyCode::AltLeft);
            }
            if chord.shift {
                keys.push(KeyCode::ShiftLeft);
            }
            let (sent, _) = dispatch(PlayModeState::Playing, &keys);
            assert_eq!(sent, vec![action], "{} ({}) during Play", action.name(), chord.display());
        }
    }

    /// During Play the read-only chords run and the writing ones go to the
    /// game: Ctrl+C copies and Ctrl+A selects all, while Ctrl+S, Ctrl+Z,
    /// Ctrl+D and Ctrl+V send nothing (each sends its action while editing).
    #[test]
    fn play_keys_copy_and_select_but_never_write() {
        let ctrl = KeyCode::ControlLeft;
        assert_eq!(dispatch(PlayModeState::Playing, &[ctrl, KeyCode::KeyC]).0, vec![Action::Copy]);
        assert_eq!(dispatch(PlayModeState::Playing, &[ctrl, KeyCode::KeyA]).0, vec![Action::SelectAll]);
        for key in [KeyCode::KeyS, KeyCode::KeyZ, KeyCode::KeyD, KeyCode::KeyV] {
            let (sent, _) = dispatch(PlayModeState::Playing, &[ctrl, key]);
            assert!(sent.is_empty(), "Ctrl+{key:?} sent {sent:?} during Play");
            let (sent, _) = dispatch(PlayModeState::Editing, &[ctrl, key]);
            assert_eq!(sent.len(), 1, "Ctrl+{key:?} sent {sent:?} while editing");
        }
    }

    /// What the menu handler sent for one action.
    #[derive(Debug, Default, PartialEq)]
    struct Sent {
        files: usize,
        undos: usize,
        redos: usize,
        copies: usize,
        duplicates: usize,
        paste: bool,
    }

    /// Run the menu handler once in `state` on `action`, and return what it
    /// sent and the warnings it left. Each warning is also in the Output
    /// panel.
    fn menu(state: PlayModeState, action: Action) -> (Sent, Vec<String>) {
        let mut app = App::new();
        app.add_message::<crate::ui::MenuActionEvent>()
            .add_message::<crate::commands::UndoCommandEvent>()
            .add_message::<crate::commands::RedoCommandEvent>()
            .add_message::<crate::camera_controller::FrameSelectionEvent>()
            .add_message::<crate::camera_controller::GoToCameraEvent>()
            .add_message::<crate::clipboard::CopyEvent>()
            .add_message::<crate::clipboard::DuplicateEvent>()
            .add_message::<crate::undo::UndoEvent>()
            .add_message::<crate::undo::RedoEvent>()
            .add_message::<crate::ui::FileEvent>()
            .add_message::<crate::selection_sync::SelectChildrenEvent>()
            .add_message::<crate::selection_sync::SelectDescendantsEvent>()
            .add_message::<crate::selection_sync::SelectParentEvent>()
            .add_message::<crate::selection_sync::SelectSiblingsEvent>()
            .add_message::<crate::selection_sync::InvertSelectionEvent>()
            .add_message::<crate::modal_tool::ActivateModalToolEvent>();
        app.insert_resource(State::new(state));
        app.insert_resource(crate::ui::StudioState::default());
        app.init_resource::<crate::undo::UndoStack>();
        // `new()`, not the derived default, which keeps no notice.
        app.insert_resource(crate::notifications::NotificationManager::new());
        app.init_resource::<crate::ui::OutputConsole>();
        app.add_systems(Update, handle_menu_action_events);
        app.world_mut().write_message(crate::ui::MenuActionEvent::new(action));
        app.update();
        let world = app.world_mut();
        let studio = world.resource::<crate::ui::StudioState>();
        let paste = studio.pending_paste || studio.pending_paste_into;
        let sent = Sent {
            files: world.resource_mut::<Messages<crate::ui::FileEvent>>().drain().count(),
            undos: world.resource_mut::<Messages<crate::undo::UndoEvent>>().drain().count(),
            redos: world.resource_mut::<Messages<crate::undo::RedoEvent>>().drain().count(),
            copies: world.resource_mut::<Messages<crate::clipboard::CopyEvent>>().drain().count(),
            duplicates: world.resource_mut::<Messages<crate::clipboard::DuplicateEvent>>().drain().count(),
            paste,
        };
        let warnings: Vec<String> = world
            .resource::<crate::notifications::NotificationManager>()
            .notifications
            .iter()
            .filter(|n| n.level == crate::notifications::NotificationLevel::Warning)
            .map(|n| n.message.clone())
            .collect();
        let output: Vec<String> = world
            .resource::<crate::ui::OutputConsole>()
            .entries
            .iter()
            .filter(|e| e.level == crate::ui::LogLevel::Warn)
            .map(|e| e.message.clone())
            .collect();
        assert_eq!(output, warnings, "{action:?}: the Output panel and the toasts disagree");
        (sent, warnings)
    }

    /// The menu and the buttons get the keyboard's answer. During Play each
    /// write sends nothing and says why; while editing each sends its event.
    #[test]
    fn the_menu_refuses_writes_during_play_and_says_why() {
        let writes = [
            Action::SaveScene,
            Action::SaveSceneAs,
            Action::Undo,
            Action::Redo,
            Action::Cut,
            Action::Paste,
            Action::PasteInto,
            Action::Duplicate,
            Action::Delete,
            Action::Group,
            Action::Ungroup,
        ];
        for action in writes {
            let (sent, warnings) = menu(PlayModeState::Playing, action);
            assert_eq!(sent, Sent::default(), "{action:?} ran during Play");
            assert_eq!(warnings, [play_refusal(PlayModeState::Playing, action).unwrap()], "{action:?}");
            // Delete with nothing selected, and Group and Ungroup (run by
            // their own handler), send nothing from here while editing either.
            if !matches!(action, Action::Delete | Action::Group | Action::Ungroup) {
                let (sent, warnings) = menu(PlayModeState::Editing, action);
                assert_ne!(sent, Sent::default(), "{action:?} sent nothing while editing");
                assert!(warnings.is_empty(), "{action:?}: {warnings:?}");
            }
        }
        // Copy only reads the scene: it runs during Play, with no notice.
        let (sent, warnings) = menu(PlayModeState::Playing, Action::Copy);
        assert_eq!(sent.copies, 1);
        assert!(warnings.is_empty(), "{warnings:?}");
        // Start Server is the multiplayer reader's to answer: no notice here.
        let (_, warnings) = menu(PlayModeState::Playing, Action::StartServer);
        assert!(warnings.is_empty(), "{warnings:?}");
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

    /// The View menu prints Ctrl+4 next to Assets. The chord reaches the
    /// toggle, which stays live during Play like the other panel toggles.
    #[test]
    fn ctrl_4_toggles_assets_in_edit_and_in_play() {
        for state in [PlayModeState::Editing, PlayModeState::Playing] {
            let (sent, _) = dispatch(state, &[KeyCode::ControlLeft, KeyCode::Digit4]);
            assert_eq!(sent, vec![Action::ToggleAssets], "{state:?} did not reach the Assets toggle");
        }
    }

    /// Ctrl+Shift+F is Find in Scripts, and the Assets toggle does not answer
    /// to it any more. Searching only reads, so it stays live during Play.
    #[test]
    fn ctrl_shift_f_finds_in_scripts_in_edit_and_in_play() {
        for state in [PlayModeState::Editing, PlayModeState::Playing] {
            let (sent, _) = dispatch(state, &[KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::KeyF]);
            assert_eq!(sent, vec![Action::FindInScripts], "{state:?} did not reach Find in Scripts");
        }
        assert!(PLAY_SESSION_ACTIONS.contains(&Action::FindInScripts));
    }
}

#[cfg(test)]
mod look_picker_key_tests {
    use super::*;

    /// I samples in every tool; the terrain tools keep their own I.
    #[test]
    fn i_samples_everywhere_and_the_terrain_tools_keep_their_own_i() {
        let bindings = KeyBindings::default();
        let i = KeyBinding::new(KeyCode::KeyI);
        assert_eq!(bindings.get(Action::SampleLook), Some(&i));
        assert_eq!(bindings.get(Action::TerrainSampleMaterial), Some(&i));
        assert_eq!(Action::SampleLook.context(), ActionContext::Global);
        assert_eq!(Action::TerrainSampleMaterial.context(), ActionContext::Terrain);
        assert!(DISPATCHED_ACTIONS.contains(&Action::SampleLook));
        assert!(!PLAY_SESSION_ACTIONS.contains(&Action::SampleLook), "the game keeps I during Play");
    }

    #[test]
    fn the_paint_bucket_has_a_chord_and_an_activation() {
        let bindings = KeyBindings::default();
        assert_eq!(
            bindings.get(Action::ToolPaintBucket),
            Some(&KeyBinding::new(KeyCode::KeyB).with_ctrl().with_alt())
        );
        assert!(DISPATCHED_ACTIONS.contains(&Action::ToolPaintBucket));
    }
}

/// The Delete reports what it did, so Remove Road can say it, and refuses
/// during Play wherever it is asked.
#[cfg(test)]
mod delete_outcome_tests {
    use super::*;
    use crate::play_mode::PlayModeState;

    fn world_in(state: PlayModeState) -> World {
        let mut world = World::new();
        world.insert_resource(State::new(state));
        world.init_resource::<crate::undo::UndoStack>();
        // `new()`, not the derived default, which keeps no notice.
        world.insert_resource(crate::notifications::NotificationManager::new());
        world.init_resource::<crate::ui::OutputConsole>();
        world
    }

    /// A folder-backed instance goes to the Space's trash with an undo
    /// entry; one with no file is removed from the scene only, with none.
    #[test]
    fn the_delete_says_what_it_removed_and_whether_undo_brings_it_back() {
        let space = std::env::temp_dir().join(format!("eustress_delete_outcome_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&space);
        let folder = space.join("Workspace").join("Road");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("_instance.toml"), "[metadata]\nclass_name = \"TerrainSpline\"\n").unwrap();
        let mut world = world_in(PlayModeState::Editing);
        let road = world
            .spawn((
                eustress_common::classes::Instance::default(),
                crate::space::instance_loader::InstanceFile {
                    toml_path: folder.join("_instance.toml"),
                    mesh_path: std::path::PathBuf::new(),
                    name: "Road".to_string(),
                },
            ))
            .id();
        let outcome = delete_entities_now(&mut world, &[road]);
        assert_eq!(
            outcome,
            DeleteOutcome {
                refused: false,
                removed: vec![road],
                undoable: true
            }
        );
        assert!(world.get_entity(road).is_err(), "the road's entity is still there");
        assert!(!folder.exists(), "the road's folder is still in Workspace");
        assert!(
            space.join(".eustress").join("trash").join("Road").join("_instance.toml").is_file(),
            "the road's folder is not in the trash"
        );
        assert!(world.resource::<crate::undo::UndoStack>().can_undo(), "the Delete pushed no undo entry");

        let loose = world.spawn(eustress_common::classes::Instance::default()).id();
        let outcome = delete_entities_now(&mut world, &[loose]);
        assert_eq!(
            outcome,
            DeleteOutcome {
                refused: false,
                removed: vec![loose],
                undoable: false
            }
        );
        assert!(world.get_entity(loose).is_err(), "the loose entity is still there");
        let _ = std::fs::remove_dir_all(&space);
    }

    /// During Play the Delete is refused and said, and nothing is removed.
    #[test]
    fn the_delete_refuses_during_play() {
        for state in [PlayModeState::Playing, PlayModeState::Paused] {
            let mut world = world_in(state);
            let part = world.spawn(eustress_common::classes::Instance::default()).id();
            let outcome = delete_entities_now(&mut world, &[part]);
            assert_eq!(
                outcome,
                DeleteOutcome {
                    refused: true,
                    removed: Vec::new(),
                    undoable: false
                }
            );
            assert!(world.get_entity(part).is_ok(), "{state:?}: the part was deleted");
            let toasts: Vec<String> = world.resource::<crate::notifications::NotificationManager>().notifications.iter().map(|n| n.message.clone()).collect();
            let output: Vec<String> = world.resource::<crate::ui::OutputConsole>().entries.iter().map(|e| e.message.clone()).collect();
            assert_eq!(toasts, ["Stop Play to delete."], "{state:?}");
            assert_eq!(output, ["Stop Play to delete."], "{state:?}");
        }
    }
}

#[cfg(test)]
mod chord_hold_tests {
    use super::*;

    fn pressed(keys: &[KeyCode]) -> ButtonInput<KeyCode> {
        let mut input = ButtonInput::<KeyCode>::default();
        for key in keys {
            input.press(*key);
        }
        input
    }

    /// Lift and Settle follow their bindings: rebinding Lift moves the key
    /// (`-` no longer lifts), "+" (Shift+=) still settles, and a modifier the
    /// chord does not name (Ctrl+=) is not Settle.
    #[test]
    fn lift_and_settle_follow_their_bindings() {
        let mut kb = KeyBindings::default();
        let minus = pressed(&[KeyCode::Minus]);
        assert!(kb.check(Action::LiftSelection, &minus));
        assert!(kb.held(Action::LiftSelection, &minus));

        // In memory only: `rebind` would write the user's keybindings.ron.
        kb.bindings.insert(Action::LiftSelection, KeyBinding::new(KeyCode::KeyL).with_alt());
        assert!(!kb.held(Action::LiftSelection, &minus), "`-` still lifts after Lift was rebound");
        assert!(kb.held(Action::LiftSelection, &pressed(&[KeyCode::AltLeft, KeyCode::KeyL])));

        assert!(kb.check(Action::SettleSelection, &pressed(&[KeyCode::Equal])));
        assert!(kb.check(Action::SettleSelection, &pressed(&[KeyCode::ShiftLeft, KeyCode::Equal])));
        assert!(!kb.held(Action::SettleSelection, &pressed(&[KeyCode::ControlLeft, KeyCode::Equal])));
    }

    /// A nudge fires on the press, stays quiet through the repeat delay,
    /// repeats after it, and stops when the chord is released.
    #[test]
    fn a_held_nudge_repeats_only_after_the_delay() {
        let mut time = Time::default();
        let (mut held, mut timer) = (false, 0.0f32);
        assert!(nudge_should_fire(true, true, &time, &mut held, &mut timer), "the press fires");

        time.advance_by(std::time::Duration::from_secs_f32(NUDGE_DELAY_SECS * 0.5));
        assert!(!nudge_should_fire(false, true, &time, &mut held, &mut timer), "no repeat inside the delay");

        time.advance_by(std::time::Duration::from_secs_f32(NUDGE_DELAY_SECS));
        assert!(nudge_should_fire(false, true, &time, &mut held, &mut timer), "repeats after the delay");

        assert!(!nudge_should_fire(false, false, &time, &mut held, &mut timer), "a released chord does not fire");
        assert!(!held);
    }

    /// The View menu prints Ctrl+1 to Ctrl+4 for the four panels; each is bound.
    #[test]
    fn the_view_menu_panel_chords_are_bound() {
        let kb = KeyBindings::default();
        for (action, key) in [
            (Action::ToggleExplorer, KeyCode::Digit1),
            (Action::ToggleProperties, KeyCode::Digit2),
            (Action::ToggleOutput, KeyCode::Digit3),
        ] {
            assert_eq!(kb.get(action), Some(&KeyBinding::new(key).with_ctrl()), "{action:?}");
        }
        assert_eq!(
            kb.get(Action::ToggleAssets),
            Some(&KeyBinding::new(KeyCode::Digit4).with_ctrl()),
            "Ctrl+4 is printed next to Assets in the View menu but is not its chord",
        );
    }
}

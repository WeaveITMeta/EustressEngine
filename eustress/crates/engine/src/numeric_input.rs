//! # Floating Numeric Input
//!
//! Live, keyboard-driven numeric entry during an active gizmo drag.
//! Blender / Maya / Fusion parity — type `2.5 <Enter>` while dragging
//! a Move axis and the part snaps to exactly 2.5 along that axis.
//!
//! ## Lifecycle
//!
//! 1. User starts dragging a handle (Move axis / Scale face / Rotate
//!    ring). The relevant tool populates its `initial_*` HashMaps.
//! 2. User types a digit / minus / dot. `detect_numeric_input_start`
//!    sees there's an active drag (any of the three tool states
//!    reports one) and flips [`NumericInputState`] to active, routing
//!    the first character into the buffer.
//! 3. While active, `handle_numeric_input_keys` consumes further
//!    keypresses — digits, `.`, `-`, `+`, backspace, Tab, Enter, Esc.
//!    Enter parses the buffer and emits [`NumericInputCommittedEvent`];
//!    Esc emits [`NumericInputCancelledEvent`] without a value.
//! 4. The active tool's drag-update system checks
//!    [`NumericInputState::override_value`] — if present, it uses that
//!    exact delta instead of the cursor-derived delta. Once it sees
//!    the commit event it finalizes the drag (same code path as
//!    mouse-release).
//!
//! ## Rust-first
//!
//! The Slint layer reflects [`NumericInputState`] as read-only props
//! — `anchor_x`, `anchor_y`, `text`, `axis_label`, `unit`, `visible`.
//! All parsing + state transitions live here.

use bevy::prelude::*;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};

use crate::move_tool::{MoveToolState, Axis3d};
use crate::scale_tool::ScaleToolState;
use crate::rotate_tool::RotateToolState;

// ============================================================================
// State
// ============================================================================

/// Which tool owns the current numeric entry. Determines which
/// `numeric_override` field the commit flows into and which unit label
/// the UI shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericInputOwner {
    Move,
    Scale,
    Rotate,
}

impl NumericInputOwner {
    /// Human-readable unit suffix shown in the floating input.
    pub fn unit(self) -> &'static str {
        match self {
            // The live label is `NumericInputState::unit_label`, which
            // names the display unit.
            NumericInputOwner::Move   => "m",
            NumericInputOwner::Scale  => "×",
            NumericInputOwner::Rotate => "°",
        }
    }

    /// Display label for the current axis — e.g. "along X" for Move,
    /// "around Y" for Rotate. Axis is already world-or-local space
    /// resolved by the tool; this is just presentation.
    pub fn axis_label(self, axis: Option<Axis3d>) -> String {
        let axis_letter = match axis {
            Some(Axis3d::X) => "X",
            Some(Axis3d::Y) => "Y",
            Some(Axis3d::Z) => "Z",
            None            => return String::new(),
        };
        match self {
            NumericInputOwner::Move   => format!("along {} axis",  axis_letter),
            NumericInputOwner::Scale  => format!("on {} axis",     axis_letter),
            NumericInputOwner::Rotate => format!("around {} axis", axis_letter),
        }
    }
}

/// Active-entry state, driven by keyboard, read by Slint + tools.
#[derive(Resource, Debug, Clone, Default)]
pub struct NumericInputState {
    pub active: bool,
    pub text: String,
    pub owner: Option<NumericInputOwner>,
    pub axis: Option<Axis3d>,
    /// Relative vs absolute — `+5` vs `5`. Affects how tools apply the
    /// value: relative means delta from initial, absolute means the
    /// exact size/angle.
    pub relative: bool,
    /// Screen-space pixel anchor — where the popup draws. Captured on
    /// entry so the popup doesn't jitter with the cursor.
    pub anchor_x: f32,
    pub anchor_y: f32,
    /// Parsed override if the buffer is a valid number. Tools consume
    /// this every frame while numeric entry is active so drag
    /// visualization shows the typed value rather than cursor position.
    /// A Move distance is in metres, whatever unit it was typed in.
    pub override_value: Option<f32>,
    /// For a Move entry, the status bar's display unit when typing began:
    /// the unit of a number typed without one. `None` for Scale (a factor)
    /// and Rotate (degrees).
    pub length_unit: Option<eustress_common::units::Unit>,
}

impl NumericInputState {
    pub fn clear(&mut self) {
        self.active = false;
        self.text.clear();
        self.owner = None;
        self.axis = None;
        self.relative = false;
        self.override_value = None;
        self.length_unit = None;
    }

    fn reparse(&mut self) {
        self.relative = self.text.starts_with('+') || self.text.starts_with("-+");
        self.override_value = parse_numeric_buffer(&self.text, self.length_unit);
    }

    /// The unit the floating input shows beside the number: the display
    /// unit for a Move distance, `×` for Scale, `°` for Rotate.
    pub fn unit_label(&self) -> &'static str {
        match self.owner {
            Some(NumericInputOwner::Move) => self.length_unit.unwrap_or_default().symbol(),
            Some(owner) => owner.unit(),
            None => "",
        }
    }
}

/// Parse the typed buffer into an override value. Accepts (Phase 1):
/// - `2.5`         → 2.5 absolute
/// - `+2.5`        → 2.5 relative (leading `+` = delta from initial)
/// - `-2.5`        → -2.5 absolute
/// - `.5`          → 0.5
/// - `2.5`         → 2.5 in the display unit, for a Move distance
/// - `2.5m` / `2.5 m`     → 2.5 m
/// - `2.5ft`       → 0.762 m
/// - `2.5in`       → 0.0635 m
/// - `2.5cm`       → 0.025 m
/// - `2.5mm`       → 0.0025 m
/// - `2 studs`     → 0.56 m (`Unit::Stud`, 0.28 m)
/// - `90deg` / `90°`   → 90 degrees (Rotate tool consumes as-is)
/// - `1.57rad`     → 89.954 degrees (converts to Rotate's display unit)
/// - empty / just sign / just unit → None (no override yet)
///
/// Phase 2 additions — expression input when the buffer starts with `=`:
/// - `=2+3`            → 5
/// - `=(2+3)*4`        → 20
/// - `=sin(30deg)`     → 0.5
/// - `=sqrt(2)`        → 1.4142
/// - `=pi`             → 3.14159
/// - `=2.5m + 30cm`    → 2.8 m. With no length unit anywhere in the
///   expression, its result is in the display unit (`=2+3` with feet
///   showing is 5 ft); with one, suffixed numbers convert to metres and
///   plain numbers are plain factors (`=2m*3` is 6 m).
///
/// Expression mode supports `+ - * /`, parentheses, `^` power,
/// and functions `sin, cos, tan, asin, acos, atan, sqrt, abs, floor,
/// ceil, min(a,b), max(a,b), log, ln, exp`. Trig functions take radians
/// unless an inner literal carries a `deg` suffix.
///
/// Unit suffixes inside expressions are evaluated inline as multipliers.
fn parse_numeric_buffer(text: &str, length_unit: Option<eustress_common::units::Unit>) -> Option<f32> {
    if text.is_empty() { return None; }
    let trimmed = text.trim();
    if trimmed.is_empty() { return None; }
    // Metres per plain number: the display unit for a Move distance, 1 otherwise.
    let plain = length_unit.map_or(1.0, |u| u.to_meters() as f32);

    // Expression mode — the buffer starts with `=`.
    if let Some(expr) = trimmed.strip_prefix('=') {
        let (value, had_length_unit) = eval_expression_with_units(expr)?;
        return Some(if had_length_unit { value } else { value * plain });
    }

    let t = trimmed.trim_start_matches('+');
    if t.is_empty() || t == "-" || t == "." || t == "-." { return None; }

    // Split into numeric prefix + optional unit suffix. Walk chars to
    // find the first non-numeric-or-dot character; everything from
    // there on is the unit.
    let (num_part, unit_part): (String, String) = {
        let mut num = String::new();
        let mut unit = String::new();
        let mut in_unit = false;
        for c in t.chars() {
            if !in_unit && (c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' || c == 'E') {
                num.push(c);
            } else {
                in_unit = true;
                if !c.is_whitespace() { unit.push(c); }
            }
        }
        (num, unit.to_ascii_lowercase())
    };

    let raw: f32 = num_part.parse().ok()?;
    Some(if unit_part.is_empty() { raw * plain } else { raw * unit_multiplier(&unit_part) })
}

/// Metres per unit for a length suffix, through `Unit` (a stud is
/// `Unit::Stud`); degrees per unit for an angle suffix. An unknown suffix
/// counts as 1, so a typo leaves the number as typed.
fn unit_multiplier(unit: &str) -> f32 {
    if let Some(m) = length_multiplier(unit) {
        return m;
    }
    match unit {
        "deg" | "°" | "degree" | "degrees" => 1.0,
        "rad" | "radian" | "radians" => 180.0 / std::f32::consts::PI,
        _ => 1.0,
    }
}

/// Metres per unit when `unit` names a length, else `None`.
fn length_multiplier(unit: &str) -> Option<f32> {
    if let Some(u) = eustress_common::units::Unit::from_any(unit) {
        return Some(u.to_meters() as f32);
    }
    match unit {
        "km" => Some(1000.0),
        "yd" | "yard" | "yards" => Some(0.9144),
        _ => None,
    }
}

// ============================================================================
// Property-reference table — `=other.x` / `=other.size.y` / `=other.rot.y`
// ============================================================================
//
// Populated each frame by `refresh_property_ref_table`; the expression
// evaluator reads from the snapshot via a thread-local clone. Keeps the
// evaluator itself World-agnostic.

use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Resource, Default, Clone)]
pub struct PropertyRefTable {
    /// `name.field → value`. Field names: `x/y/z` (position),
    /// `size.x / .y / .z`, `rot.x / .y / .z / .w`.
    values: HashMap<String, f32>,
}

impl PropertyRefTable {
    fn insert(&mut self, name: &str, field: &str, value: f32) {
        self.values.insert(format!("{name}.{field}"), value);
    }
    pub fn get(&self, key: &str) -> Option<f32> {
        self.values.get(key).copied()
    }
}

thread_local! {
    /// Parser-local view of the table. Refreshed by `thread_local_sync`
    /// inside a system wrapper before every parse.
    static PROPERTY_REFS: RefCell<PropertyRefTable> = RefCell::new(PropertyRefTable::default());
}

fn property_ref_lookup(key: &str) -> Option<f32> {
    PROPERTY_REFS.with(|t| t.borrow().get(key))
}

/// Populates `PropertyRefTable` + the thread-local view every frame
/// from live entities. Keyed on `Instance.name`; fields: position
/// (`.x/.y/.z`), size (`size.x/.y/.z`), rotation (`rot.x/.y/.z/.w`).
fn refresh_property_ref_table(
    // Only rebuild while the user is actively typing a numeric expression.
    // Clearing + rebuilding the whole ref table (301K entities × 10 properties
    // ≈ 3M entries) and CLONING it every frame cost ~2.1 s/frame on Vehicle
    // Simulator — 31% of the frame. `NumericInputState.active` is true only
    // during a gizmo-drag numeric entry, so outside that the last-built table is
    // simply retained (fine for the rare Timeline/Rune expression consumers).
    state: Res<NumericInputState>,
    mut table: ResMut<PropertyRefTable>,
    query: Query<(
        &crate::classes::Instance,
        &GlobalTransform,
        Option<&crate::classes::BasePart>,
    )>,
) {
    if !state.active {
        return;
    }
    table.values.clear();
    for (inst, gt, bp) in query.iter() {
        let t = gt.compute_transform();
        let name = &inst.name;
        if name.is_empty() { continue; }
        table.insert(name, "x", t.translation.x);
        table.insert(name, "y", t.translation.y);
        table.insert(name, "z", t.translation.z);
        let size = bp.map(|b| b.size).unwrap_or(t.scale);
        table.insert(name, "size.x", size.x);
        table.insert(name, "size.y", size.y);
        table.insert(name, "size.z", size.z);
        table.insert(name, "rot.x", t.rotation.x);
        table.insert(name, "rot.y", t.rotation.y);
        table.insert(name, "rot.z", t.rotation.z);
        table.insert(name, "rot.w", t.rotation.w);
    }
    // Push the snapshot into the thread-local the parser reads from.
    let snap = table.clone();
    PROPERTY_REFS.with(|t| *t.borrow_mut() = snap);
}

// ============================================================================
// Expression evaluator — recursive descent
// ============================================================================
//
// Grammar:
//   expr    := addsub
//   addsub  := muldiv (('+'|'-') muldiv)*
//   muldiv  := power  (('*'|'/') power)*
//   power   := unary  ('^' unary)*
//   unary   := ('-')? atom
//   atom    := number[unit] | 'pi' | 'e' | '(' expr ')' | func '(' args ')'
//   args    := expr (',' expr)*

/// Public wrapper around `eval_expression` for reuse outside the
/// numeric-input keybinding path. Consumers: Timeline procedural
/// animation tracks, Rune bridge, any future expression-driven
/// property system. No leading `=` required — the caller has
/// already stripped it.
pub fn parse_expression_public(src: &str) -> Option<f32> {
    eval_expression_with_units(src).map(|(value, _)| value)
}

/// Evaluate an expression; also report whether any number in it carried a
/// length unit.
fn eval_expression_with_units(src: &str) -> Option<(f32, bool)> {
    let mut p = ExprParser { src, pos: 0, had_length_unit: false };
    p.skip_ws();
    let v = p.parse_addsub()?;
    p.skip_ws();
    if p.pos < p.src.len() { return None; } // trailing garbage
    v.is_finite().then_some((v, p.had_length_unit))
}

struct ExprParser<'a> { src: &'a str, pos: usize, had_length_unit: bool }

impl<'a> ExprParser<'a> {
    fn peek(&self) -> Option<char> { self.src[self.pos..].chars().next() }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() { self.bump(); } else { break; }
        }
    }
    fn eat(&mut self, expected: char) -> bool {
        self.skip_ws();
        if self.peek() == Some(expected) { self.bump(); true } else { false }
    }

    fn parse_addsub(&mut self) -> Option<f32> {
        let mut lhs = self.parse_muldiv()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => { self.bump(); lhs += self.parse_muldiv()?; }
                Some('-') => { self.bump(); lhs -= self.parse_muldiv()?; }
                _ => break,
            }
        }
        Some(lhs)
    }

    fn parse_muldiv(&mut self) -> Option<f32> {
        let mut lhs = self.parse_power()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => { self.bump(); lhs *= self.parse_power()?; }
                Some('/') => {
                    self.bump();
                    let rhs = self.parse_power()?;
                    if rhs == 0.0 { return None; }
                    lhs /= rhs;
                }
                _ => break,
            }
        }
        Some(lhs)
    }

    fn parse_power(&mut self) -> Option<f32> {
        let lhs = self.parse_unary()?;
        self.skip_ws();
        if self.peek() == Some('^') {
            self.bump();
            let rhs = self.parse_unary()?;
            Some(lhs.powf(rhs))
        } else {
            Some(lhs)
        }
    }

    fn parse_unary(&mut self) -> Option<f32> {
        self.skip_ws();
        if self.peek() == Some('-') {
            self.bump();
            Some(-self.parse_atom()?)
        } else {
            self.parse_atom()
        }
    }

    fn parse_atom(&mut self) -> Option<f32> {
        self.skip_ws();
        let c = self.peek()?;
        // Parenthesized.
        if c == '(' {
            self.bump();
            let v = self.parse_addsub()?;
            if !self.eat(')') { return None; }
            return Some(v);
        }
        // Identifier — constant, function, or property reference.
        if c.is_ascii_alphabetic() {
            let ident = self.read_ident();
            self.skip_ws();
            // Function call if followed by `(`.
            if self.peek() == Some('(') {
                self.bump();
                let a = self.parse_addsub()?;
                let b = if self.eat(',') { Some(self.parse_addsub()?) } else { None };
                if !self.eat(')') { return None; }
                return apply_func(&ident, a, b);
            }
            // Dotted property reference — `<name>.<field>` or
            // `<name>.<nested>.<field>` (e.g. `other.x`, `other.size.y`,
            // `other.rot.w`).
            if self.peek() == Some('.') {
                let mut key = ident.clone();
                while self.peek() == Some('.') {
                    self.bump();
                    let field = self.read_ident();
                    if field.is_empty() { return None; }
                    key.push('.');
                    key.push_str(&field);
                }
                // First segment is the entity name → lookup key is the
                // rest.
                let mut parts = key.splitn(2, '.');
                let _name = parts.next()?;
                // The full key has `name.field` — pass directly to the
                // table; it stores `name.field` formatted keys.
                return property_ref_lookup(&key);
            }
            // Constant.
            return match ident.as_str() {
                "pi" => Some(std::f32::consts::PI),
                "e"  => Some(std::f32::consts::E),
                _ => None,
            };
        }
        // Number with optional unit suffix.
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' { self.bump(); } else { break; }
        }
        let num_str = &self.src[start..self.pos];
        let base: f32 = num_str.parse().ok()?;
        // Unit suffix — accumulate alphabetic / degree chars, but stop
        // at operators / commas / parens.
        let unit_start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() || c == '°' { self.bump(); } else { break; }
        }
        let unit = self.src[unit_start..self.pos].to_ascii_lowercase();
        if length_multiplier(&unit).is_some() {
            self.had_length_unit = true;
        }
        Some(base * unit_multiplier(&unit))
    }

    fn read_ident(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' { self.bump(); } else { break; }
        }
        self.src[start..self.pos].to_ascii_lowercase()
    }
}

fn apply_func(name: &str, a: f32, b: Option<f32>) -> Option<f32> {
    match name {
        "sin"   => Some(a.sin()),
        "cos"   => Some(a.cos()),
        "tan"   => Some(a.tan()),
        "asin"  => Some(a.asin()),
        "acos"  => Some(a.acos()),
        "atan"  => Some(a.atan()),
        "sqrt"  => Some(a.sqrt()),
        "abs"   => Some(a.abs()),
        "floor" => Some(a.floor()),
        "ceil"  => Some(a.ceil()),
        "ln"    => Some(a.ln()),
        "log"   => Some(a.log10()),
        "exp"   => Some(a.exp()),
        "min"   => b.map(|bv| a.min(bv)),
        "max"   => b.map(|bv| a.max(bv)),
        _ => None,
    }
}

// ============================================================================
// Events
// ============================================================================

/// Emitted on Enter with a valid parsed value.
#[derive(Event, Message, Debug, Clone, Copy)]
pub struct NumericInputCommittedEvent {
    pub owner: NumericInputOwner,
    pub axis: Option<Axis3d>,
    pub value: f32,
    pub relative: bool,
}

/// Emitted on Esc or right-click — tool should keep its drag state
/// (nothing changed) and the UI should dismiss the input.
#[derive(Event, Message, Debug, Clone, Copy, Default)]
pub struct NumericInputCancelledEvent;

// ============================================================================
// Plugin
// ============================================================================

pub struct NumericInputPlugin;

impl Plugin for NumericInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NumericInputState>()
            .init_resource::<PropertyRefTable>()
            .add_message::<NumericInputCommittedEvent>()
            .add_message::<NumericInputCancelledEvent>()
            // Editor input: idle during a Play session (digits and Tab are
            // the game's keys then).
            .add_systems(Update, (
                refresh_property_ref_table,
                clear_numeric_input_on_drag_end,
                detect_numeric_input_start,
                handle_numeric_input_keys,
            ).chain().run_if(crate::play_mode::editor_input_enabled));
    }
}

// ============================================================================
// Systems
// ============================================================================

/// Auto-clears the floating numeric input if its owning tool's drag
/// ends (mouse released) without the user pressing Enter/Escape.
///
/// Without this, a numeric entry that activated mid-drag (typed a
/// digit while dragging a handle) stayed active indefinitely once the
/// drag itself was long over — `NumericInputState` only ever cleared
/// on an explicit Enter or Escape keypress, with nothing tying it back
/// to the drag that spawned it. The popup then kept swallowing every
/// subsequent keystroke as buffer text (digits AND letters, since
/// letters are accepted as unit suffixes once a digit is present) —
/// including WASD camera movement — until the user noticed and hit
/// Enter or Escape. Nothing is lost by clearing here instead: the
/// owning tool's own mouse-release handler already finalizes the
/// transform using `override_value` if one was present (same value the
/// drag-update system was already live-previewing every frame), so by
/// the time the drag state goes empty the numeric entry has already
/// done its job.
fn clear_numeric_input_on_drag_end(
    mut numeric: ResMut<NumericInputState>,
    move_state: Res<MoveToolState>,
    scale_state: Res<ScaleToolState>,
    rotate_state: Res<RotateToolState>,
) {
    if !numeric.active { return; }
    let drag_still_active = match numeric.owner {
        Some(NumericInputOwner::Move)   => !move_state.initial_positions.is_empty(),
        Some(NumericInputOwner::Scale)  => !scale_state.initial_scales.is_empty(),
        Some(NumericInputOwner::Rotate) => !rotate_state.initial_rotations.is_empty(),
        None => false,
    };
    if !drag_still_active {
        numeric.clear();
    }
}

/// When a drag is active on any of the three gizmo tools AND the user
/// types a digit / minus / dot, flip [`NumericInputState`] to active
/// and capture the cursor anchor. Does NOT push the first character —
/// `handle_numeric_input_keys` runs next in the chain and sees the
/// same keypress via its own reader, pushing it onto the buffer.
fn detect_numeric_input_start(
    mut numeric: ResMut<NumericInputState>,
    mut keys: MessageReader<KeyboardInput>,
    move_state: Res<MoveToolState>,
    scale_state: Res<ScaleToolState>,
    rotate_state: Res<RotateToolState>,
    windows: Query<&bevy::window::Window, With<bevy::window::PrimaryWindow>>,
    display_unit: Option<Res<eustress_common::units::DisplayUnit>>,
) {
    if numeric.active {
        // Drain our reader so stale events don't trigger reactivation
        // in a later frame. Other readers have their own cursors.
        for _ in keys.read() {}
        return;
    }

    // Determine which tool (if any) is actively dragging. Priority
    // Move → Scale → Rotate; only one of these three is active at a
    // time since the tools gate on `StudioState::current_tool`.
    let (owner, axis) = if !move_state.initial_positions.is_empty() {
        (Some(NumericInputOwner::Move), move_state.dragged_axis)
    } else if !scale_state.initial_scales.is_empty() {
        (Some(NumericInputOwner::Scale), scale_state.dragged_axis.map(|a| a.axis()))
    } else if !rotate_state.initial_rotations.is_empty() {
        (Some(NumericInputOwner::Rotate), rotate_state.dragged_axis)
    } else {
        (None, None)
    };

    let Some(owner) = owner else {
        for _ in keys.read() {}
        return;
    };

    // Look at pending keypresses for a numeric-starter. Don't mutate
    // the buffer here — just decide whether to flip to active.
    // `=` enters expression mode; `+` / `-` / `.` / digit enter
    // plain-number mode.
    let mut starter = false;
    for ev in keys.read() {
        if ev.state != ButtonState::Pressed { continue; }
        if let Key::Character(s) = &ev.logical_key {
            if let Some(c) = s.chars().next() {
                if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == '=' {
                    starter = true;
                    break;
                }
            }
        }
    }

    if !starter { return; }

    // Capture cursor anchor for the popup.
    let (ax, ay) = windows.single()
        .ok()
        .and_then(|w| w.cursor_position())
        .map(|p| (p.x, p.y))
        .unwrap_or((0.0, 0.0));

    numeric.clear();
    numeric.active = true;
    numeric.owner = Some(owner);
    numeric.axis = axis;
    // A Move distance typed without a unit is in the display unit.
    numeric.length_unit = (owner == NumericInputOwner::Move)
        .then(|| display_unit.map(|d| d.get()).unwrap_or_default());
    numeric.anchor_x = ax;
    numeric.anchor_y = ay;
    // handle_numeric_input_keys picks up the starter char via its own
    // reader in the same frame.
}

/// While numeric entry is active, consume keystrokes: digits / `.` /
/// `-` / `+` extend the buffer, Backspace pops, Enter commits, Esc
/// cancels, Tab cycles axis (Move/Scale only — Rotate is single-axis).
fn handle_numeric_input_keys(
    mut numeric: ResMut<NumericInputState>,
    mut keys: MessageReader<KeyboardInput>,
    mut committed: MessageWriter<NumericInputCommittedEvent>,
    mut cancelled: MessageWriter<NumericInputCancelledEvent>,
) {
    if !numeric.active { return; }

    let mut dirty = false;

    for ev in keys.read() {
        if ev.state != ButtonState::Pressed { continue; }
        match &ev.logical_key {
            Key::Enter => {
                if let (Some(value), Some(owner)) = (numeric.override_value, numeric.owner) {
                    committed.write(NumericInputCommittedEvent {
                        owner,
                        axis: numeric.axis,
                        value,
                        relative: numeric.relative,
                    });
                }
                numeric.clear();
                return;
            }
            Key::Escape => {
                cancelled.write(NumericInputCancelledEvent);
                numeric.clear();
                return;
            }
            Key::Backspace => {
                numeric.text.pop();
                dirty = true;
            }
            Key::Tab => {
                // Cycle axis. Only meaningful when the tool supports it
                // (Move + Scale). Rotate ignores — always single axis.
                if matches!(numeric.owner, Some(NumericInputOwner::Move) | Some(NumericInputOwner::Scale)) {
                    numeric.axis = Some(match numeric.axis {
                        Some(Axis3d::X) | None => Axis3d::Y,
                        Some(Axis3d::Y)        => Axis3d::Z,
                        Some(Axis3d::Z)        => Axis3d::X,
                    });
                }
            }
            Key::Character(s) => {
                if let Some(c) = s.chars().next() {
                    // Digits / sign / dot at numeric position, OR
                    // letter characters appended once we've committed
                    // a number (for unit suffixes like `m`, `ft`,
                    // `deg`). The parser tolerates unknown units by
                    // falling back to raw number.
                    let has_dot = numeric.text.contains('.');
                    let at_start = numeric.text.is_empty();
                    // "Has a digit" — used to decide whether letters
                    // count as a unit suffix rather than noise.
                    let has_digit = numeric.text.chars().any(|ch| ch.is_ascii_digit());
                    // Expression mode kicks in when the buffer starts
                    // with `=` — accept operators / parens / letters /
                    // commas freely.
                    let expr_mode = numeric.text.starts_with('=')
                        || (at_start && c == '=');
                    let accept = if expr_mode {
                        matches!(
                            c,
                            '=' | '+' | '-' | '*' | '/' | '^' | '(' | ')'
                            | ',' | '.' | ' ' | '°'
                        ) || c.is_ascii_digit() || c.is_ascii_alphabetic()
                    } else {
                        match c {
                            '.' => !has_dot && !numeric.text.chars().any(|ch| ch.is_ascii_alphabetic()),
                            '-' | '+' => at_start,
                            '=' => at_start, // enter expression mode
                            ' ' => has_digit,
                            _ if c.is_ascii_digit() => !numeric.text.chars().any(|ch| ch.is_ascii_alphabetic()),
                            _ if c.is_ascii_alphabetic() || c == '°' => has_digit,
                            _ => false,
                        }
                    };
                    if accept {
                        numeric.text.push(c);
                        dirty = true;
                    }
                }
            }
            _ => {}
        }
    }

    if dirty {
        numeric.reparse();
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use eustress_common::units::Unit;

    fn close(a: Option<f32>, b: f32) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-5)
    }

    #[test]
    fn two_studs_are_0_56_metres() {
        assert!(close(parse_numeric_buffer("2 studs", Some(Unit::Meter)), 0.56));
        assert!(close(parse_numeric_buffer("2stud", Some(Unit::Foot)), 0.56));
    }

    #[test]
    fn a_plain_number_is_in_every_display_unit() {
        for &unit in Unit::PICKABLE.iter() {
            let expected = 2.0 * unit.to_meters() as f32;
            assert!(close(parse_numeric_buffer("2", Some(unit)), expected), "{unit:?}");
            assert!(close(parse_numeric_buffer("=1+1", Some(unit)), expected), "{unit:?} expression");
        }
    }

    #[test]
    fn a_typed_unit_wins_over_the_display_unit() {
        for &unit in Unit::PICKABLE.iter() {
            assert!(close(parse_numeric_buffer("2ft", Some(unit)), 0.6096), "{unit:?}");
            assert!(close(parse_numeric_buffer("=2m + 30cm", Some(unit)), 2.3), "{unit:?}");
            assert!(close(parse_numeric_buffer("=2m*3", Some(unit)), 6.0), "{unit:?}");
        }
    }

    #[test]
    fn scale_and_rotate_take_plain_numbers() {
        assert!(close(parse_numeric_buffer("2", None), 2.0));
        assert!(close(parse_numeric_buffer("90deg", None), 90.0));
    }

    #[test]
    fn the_move_label_names_the_display_unit() {
        let state = NumericInputState {
            owner: Some(NumericInputOwner::Move),
            length_unit: Some(Unit::Foot),
            ..Default::default()
        };
        assert_eq!(state.unit_label(), Unit::Foot.symbol());
    }
}

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
            NumericInputOwner::Move   => "studs",
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
    pub override_value: Option<f32>,
}

impl NumericInputState {
    pub fn clear(&mut self) {
        self.active = false;
        self.text.clear();
        self.owner = None;
        self.axis = None;
        self.relative = false;
        self.override_value = None;
    }

    fn reparse(&mut self) {
        self.relative = self.text.starts_with('+') || self.text.starts_with("-+");
        self.override_value = parse_numeric_buffer(&self.text);
    }
}

/// Parse the typed buffer into an override value. Accepts (Phase 1):
/// - `2.5`         → 2.5 absolute
/// - `+2.5`        → 2.5 relative (leading `+` = delta from initial)
/// - `-2.5`        → -2.5 absolute
/// - `.5`          → 0.5
/// - `2.5m` / `2.5 m`     → 2.5 studs (`m` is a synonym for studs in v1)
/// - `2.5ft`       → 0.7620 studs  (0.3048 m per foot)
/// - `2.5in`       → 0.0635 studs
/// - `2.5cm`       → 0.025 studs
/// - `2.5mm`       → 0.0025 studs
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
/// - `=2.5m + 30cm`    → 2.8 studs (unit math via conversion-to-studs)
///
/// Expression mode supports `+ - * /`, parentheses, `^` power,
/// and functions `sin, cos, tan, asin, acos, atan, sqrt, abs, floor,
/// ceil, min(a,b), max(a,b), log, ln, exp`. Trig functions take radians
/// unless an inner literal carries a `deg` suffix.
///
/// Unit suffixes inside expressions are evaluated inline as multipliers.
fn parse_numeric_buffer(text: &str) -> Option<f32> {
    if text.is_empty() { return None; }
    let trimmed = text.trim();
    if trimmed.is_empty() { return None; }

    // Expression mode — the buffer starts with `=`.
    if let Some(expr) = trimmed.strip_prefix('=') {
        return eval_expression(expr);
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
    Some(raw * unit_multiplier(&unit_part))
}

fn unit_multiplier(unit: &str) -> f32 {
    match unit {
        ""             => 1.0,
        "m" | "stud" | "studs" => 1.0,
        "cm"           => 0.01,
        "mm"           => 0.001,
        "km"           => 1000.0,
        "in" | "inch" | "inches" => 0.0254,
        "ft" | "foot" | "feet"   => 0.3048,
        "yd" | "yard" | "yards"  => 0.9144,
        "deg" | "°" | "degree" | "degrees" => 1.0,
        "rad" | "radian" | "radians" => 180.0 / std::f32::consts::PI,
        _ => 1.0,
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
    mut table: ResMut<PropertyRefTable>,
    query: Query<(
        &crate::classes::Instance,
        &GlobalTransform,
        Option<&crate::classes::BasePart>,
    )>,
) {
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
    eval_expression(src)
}

fn eval_expression(src: &str) -> Option<f32> {
    let mut p = ExprParser { src, pos: 0 };
    p.skip_ws();
    let v = p.parse_addsub()?;
    p.skip_ws();
    if p.pos < p.src.len() { return None; } // trailing garbage
    v.is_finite().then_some(v)
}

struct ExprParser<'a> { src: &'a str, pos: usize }

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
        let unit = &self.src[unit_start..self.pos];
        let m = unit_multiplier(&unit.to_ascii_lowercase());
        Some(base * m)
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
            .add_systems(Update, (
                refresh_property_ref_table,
                detect_numeric_input_start,
                handle_numeric_input_keys,
            ).chain());
    }
}

// ============================================================================
// Systems
// ============================================================================

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

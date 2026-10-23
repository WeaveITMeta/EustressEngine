//! Typed input actions shared by every Player.
//!
//! Desktop keyboard and mouse, a gamepad, and touch zones all write the same
//! actions. Gameplay reads [`Actions`] instead of device buttons. The
//! resolution rules (priority, sink, one owner per finger) follow the mobile
//! input design: higher-priority contexts run first, a sinking context hides
//! every source it binds, and a finger keeps the owner it got on
//! `TouchPhase::Started` until it ends.

use std::collections::{HashMap, HashSet};

use bevy::input::gamepad::{Gamepad, GamepadAxis, GamepadButton};
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::input::ButtonInput;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// What an action stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionType {
    Bool,
    Direction1D,
    Direction2D,
    Direction3D,
    ViewportPosition,
}

/// The value of an action after one frame of resolution.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ActionValue {
    #[default]
    None,
    Bool(bool),
    D1(f32),
    D2(Vec2),
    D3(Vec3),
    Viewport(Vec2),
}

impl ActionValue {
    pub fn d2(self) -> Vec2 {
        match self {
            ActionValue::D2(v) => v,
            _ => Vec2::ZERO,
        }
    }

    pub fn d1(self) -> f32 {
        match self {
            ActionValue::D1(v) => v,
            ActionValue::Bool(true) => 1.0,
            _ => 0.0,
        }
    }

    pub fn pressed(self) -> bool {
        match self {
            ActionValue::Bool(v) => v,
            ActionValue::D1(v) => v.abs() > 0.0,
            ActionValue::D2(v) => v.length_squared() > 0.0,
            _ => false,
        }
    }
}

/// Which device the shell should show glyphs and the touch overlay for.
/// Hardware `touch_enabled` is separate and does not drive the overlay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum PreferredInput {
    Touch,
    Gamepad,
    #[default]
    KeyboardAndMouse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ActionId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TouchZoneId {
    Thumbstick,
    Jump,
    Camera,
    World,
}

/// Where a binding reads.
#[derive(Clone, Debug)]
pub enum BindingSource {
    Key(KeyCode),
    MouseButton(MouseButton),
    /// Pixel delta this frame. `scale` is radians per pixel when the camera
    /// consumes it directly; the desktop path keeps pixels and applies
    /// [`crate::avatar::control::AvatarCamera::sensitivity`] itself.
    MouseDelta,
    MouseWheel,
    GamepadButton(GamepadButton),
    /// Left stick, already a direction. Radial deadzone is the binding's.
    GamepadStick,
    /// Digital composite. Missing arms are ignored. Value is right-left,
    /// up-down, before the camera turns it into a world direction.
    Composite {
        up: KeyCode,
        down: KeyCode,
        left: KeyCode,
        right: KeyCode,
    },
    /// Current thumbstick value in -1..1.
    TouchZone(TouchZoneId),
    /// Camera-drag delta in logical pixels this frame. Scale 0.01 makes radians.
    TouchDelta(TouchZoneId),
    /// A Slint control that is held. `id` is assigned by the shell.
    UiButton(u32),
}

#[derive(Clone, Debug)]
pub struct InputBinding {
    pub source: BindingSource,
    pub scale: f32,
    pub deadzone: f32,
    pub invert_y: bool,
}

impl InputBinding {
    pub fn new(source: BindingSource) -> Self {
        Self { source, scale: 1.0, deadzone: 0.0, invert_y: false }
    }

    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    pub fn deadzone(mut self, deadzone: f32) -> Self {
        self.deadzone = deadzone;
        self
    }

    /// Screen-space touch Y grows downward. The thumbstick sets this so a
    /// finger pushed up is `+Move.y`, the same sign as W.
    pub fn invert_y(mut self, invert: bool) -> Self {
        self.invert_y = invert;
        self
    }
}

#[derive(Clone, Debug)]
pub struct InputContext {
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    /// When this context was last enabled. Ties resolve most-recent first.
    pub enabled_seq: u64,
    /// A source this context binds is hidden from every lower context,
    /// whether or not the binding fired this frame.
    pub sink: bool,
    /// Claim every key, the way a focused text field does.
    pub sink_all_keys: bool,
    pub actions: Vec<ActionId>,
}

#[derive(Clone, Debug)]
pub struct InputAction {
    pub name: String,
    pub ty: ActionType,
    pub enabled: bool,
    pub bindings: Vec<InputBinding>,
    pub state: ActionValue,
    pub prev: ActionValue,
    pub pressed_this_frame: bool,
    pub released_this_frame: bool,
}

/// Safe area in logical pixels. Filled from the platform bridge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SafeInsets {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FingerOwner {
    Ui,
    Stick,
    Camera,
    Pinch,
    World,
}

#[derive(Clone, Copy, Debug)]
struct Finger {
    owner: FingerOwner,
    pos: Vec2,
    start: Vec2,
    moved: f32,
}

#[derive(Clone, Debug)]
struct Stick {
    finger: Option<u64>,
    origin: Vec2,
    value: Vec2,
}

impl Default for Stick {
    fn default() -> Self {
        Self { finger: None, origin: Vec2::ZERO, value: Vec2::ZERO }
    }
}

/// Everything the resolver keeps between frames.
#[derive(Resource)]
pub struct InputState {
    pub contexts: Vec<InputContext>,
    pub actions: HashMap<ActionId, InputAction>,
    next_action: u16,
    enable_seq: u64,
    pub preferred: PreferredMachine,
    fingers: HashMap<u64, Finger>,
    stick: Stick,
    camera_finger: Option<u64>,
    pinch: Option<(u64, u64, f32)>,
    pub insets: SafeInsets,
    /// Logical window size. Touch zones are computed inside it.
    pub viewport: Vec2,
    /// When false, touch zones are not claimed (desktop with no overlay).
    pub touch_zones: bool,
    /// Shell-published rectangles that accept a pointer, in logical pixels.
    /// A finger that starts inside one is owned by the UI for its whole life.
    pub ui_rects: Vec<Rect>,
    /// Slint button ids held this frame (jump is 1).
    pub ui_held: HashSet<u32>,
    /// Gallery and sign-in cover the window, so every finger belongs to the UI.
    pub capture_all: bool,
    /// UI fingers released this frame. `Ended` removes the finger before the
    /// shell's pointer system runs, and a press and release can share a frame.
    pub released_ui: Vec<u64>,
    /// A jump-circle finger went down and up inside this `step`. A tap shorter
    /// than the frame (the debug APK is often under 2 fps) would otherwise
    /// insert the finger and remove it before `jump_held` looks, and Jump
    /// would never press.
    jump_edge: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self::player_defaults(PreferredInput::KeyboardAndMouse)
    }
}

impl InputState {
    pub fn player_defaults(preferred: PreferredInput) -> Self {
        let mut state = Self {
            contexts: Vec::new(),
            actions: HashMap::new(),
            next_action: 1,
            enable_seq: 1,
            preferred: PreferredMachine::new(preferred),
            fingers: HashMap::new(),
            stick: Stick::default(),
            camera_finger: None,
            pinch: None,
            insets: SafeInsets::default(),
            viewport: Vec2::new(1280.0, 720.0),
            touch_zones: matches!(preferred, PreferredInput::Touch),
            ui_rects: Vec::new(),
            ui_held: HashSet::new(),
            capture_all: false,
            released_ui: Vec::new(),
            jump_edge: false,
        };
        state.push_context("SystemMenu", 1000, true, false);
        state.push_context("TextFocus", 900, true, true);
        let character = state.push_context("CoreCharacter", 10, false, false);
        let camera = state.push_context("CoreCamera", 5, false, false);
        let world = state.push_context("World", 0, false, false);
        // Text focus and the system menu start off. Gameplay turns them on.
        for name in ["SystemMenu", "TextFocus"] {
            if let Some(ctx) = state.contexts.iter_mut().find(|c| c.name == name) {
                ctx.enabled = false;
            }
        }

        let move_id = state.add_action(character, "Move", ActionType::Direction2D, vec![
            InputBinding::new(BindingSource::Composite {
                up: KeyCode::KeyW,
                down: KeyCode::KeyS,
                left: KeyCode::KeyA,
                right: KeyCode::KeyD,
            }),
            InputBinding::new(BindingSource::GamepadStick).deadzone(0.1),
            InputBinding::new(BindingSource::TouchZone(TouchZoneId::Thumbstick))
                .deadzone(0.1)
                .invert_y(true),
        ]);
        let _ = move_id;
        state.add_action(character, "Jump", ActionType::Bool, vec![
            InputBinding::new(BindingSource::Key(KeyCode::Space)),
            InputBinding::new(BindingSource::GamepadButton(GamepadButton::South)),
            InputBinding::new(BindingSource::UiButton(JUMP_BUTTON)),
        ]);
        state.add_action(character, "Sprint", ActionType::Bool, vec![
            InputBinding::new(BindingSource::Key(KeyCode::ShiftLeft)),
            InputBinding::new(BindingSource::Key(KeyCode::ShiftRight)),
        ]);
        state.add_action(character, "Crouch", ActionType::Bool, vec![
            InputBinding::new(BindingSource::Key(KeyCode::ControlLeft)),
            InputBinding::new(BindingSource::Key(KeyCode::ControlRight)),
        ]);
        state.add_action(camera, "Look", ActionType::Direction2D, vec![
            InputBinding::new(BindingSource::MouseDelta),
            // 0.01 rad per logical pixel, same scale the touch camera uses.
            InputBinding::new(BindingSource::TouchDelta(TouchZoneId::Camera)).scale(0.01),
        ]);
        state.add_action(camera, "Zoom", ActionType::Direction1D, vec![
            InputBinding::new(BindingSource::MouseWheel),
            // Pinch apart is a negative notch so the camera's `distance -= notch`
            // moves outward. 100 logical px matches one wheel notch.
            InputBinding::new(BindingSource::TouchDelta(TouchZoneId::World)).scale(-0.01),
        ]);
        state.add_action(world, "WorldTap", ActionType::ViewportPosition, vec![]);
        let _ = world;
        state
    }

    fn push_context(&mut self, name: &str, priority: i32, sink: bool, sink_all_keys: bool) -> usize {
        self.enable_seq += 1;
        self.contexts.push(InputContext {
            name: name.to_string(),
            enabled: true,
            priority,
            enabled_seq: self.enable_seq,
            sink,
            sink_all_keys,
            actions: Vec::new(),
        });
        self.contexts.len() - 1
    }

    fn add_action(&mut self, context: usize, name: &str, ty: ActionType, bindings: Vec<InputBinding>) -> ActionId {
        let id = ActionId(self.next_action);
        self.next_action += 1;
        self.actions.insert(id, InputAction {
            name: name.to_string(),
            ty,
            enabled: true,
            bindings,
            state: ActionValue::None,
            prev: ActionValue::None,
            pressed_this_frame: false,
            released_this_frame: false,
        });
        self.contexts[context].actions.push(id);
        id
    }

    pub fn set_context_enabled(&mut self, name: &str, enabled: bool) {
        if let Some(ctx) = self.contexts.iter_mut().find(|c| c.name == name) {
            if ctx.enabled != enabled {
                self.enable_seq += 1;
                ctx.enabled_seq = self.enable_seq;
            }
            ctx.enabled = enabled;
        }
    }

    /// Drop every finger and zero touch-driven values. Suspend and a switch
    /// away from touch both call this.
    pub fn release_touch(&mut self) {
        self.fingers.clear();
        self.stick = Stick::default();
        self.camera_finger = None;
        self.pinch = None;
        self.ui_held.remove(&JUMP_BUTTON);
        self.jump_edge = false;
    }

    /// Thumbstick origin and knob, in logical pixels, while a finger owns it.
    /// The shell draws this. The resolver still owns the value.
    pub fn stick_visual(&self) -> Option<(Vec2, Vec2)> {
        self.stick.finger.map(|_| (self.stick.origin, self.stick.origin + self.stick.value * STICK_RADIUS))
    }
}

/// Slint jump control. The shell's jump button uses this id.
pub const JUMP_BUTTON: u32 = 1;

/// Camera drag scale: radians per logical pixel.
pub const TOUCH_LOOK_SCALE: f32 = 0.01;

/// Thumbstick radius in logical pixels.
pub const STICK_RADIUS: f32 = 60.0;

const STICK_DEADZONE: f32 = 0.1;

/// One sample the platform collected this frame, in logical pixels.
#[derive(Clone, Debug)]
pub struct TouchSample {
    pub id: u64,
    pub phase: TouchPhase,
    pub pos: Vec2,
}

/// Device edges for one frame. Built from Bevy, or by a test.
#[derive(Clone, Debug, Default)]
pub struct DeviceFrame {
    pub keys_down: HashSet<KeyCode>,
    pub keys_just: HashSet<KeyCode>,
    pub mouse_down: HashSet<MouseButton>,
    pub mouse_just: HashSet<MouseButton>,
    /// Physical pixel delta, already what the desktop camera used to read.
    pub mouse_delta_px: Vec2,
    pub wheel_y: f32,
    pub touches: Vec<TouchSample>,
    pub pad_buttons: HashSet<GamepadButton>,
    pub pad_stick: Vec2,
    pub now_ms: u64,
}

/// What gameplay reads. Filled by [`step`].
#[derive(Resource, Clone, Debug, Default)]
pub struct Actions {
    values: HashMap<String, ActionValue>,
    edges: HashMap<String, (bool, bool)>,
    /// Mouse delta in pixels. The desktop camera multiplies by its own
    /// sensitivity, so look speed stays what it was.
    pub mouse_look_px: Vec2,
    /// Touch look already in radians (`pixels * 0.01`).
    pub touch_look_rad: Vec2,
    /// Wheel notches plus pinch, in the same unit `camera_zoom` used for the wheel.
    pub zoom_notches: f32,
    pub preferred: PreferredInput,
    pub world_tap: Option<Vec2>,
}

impl Actions {
    pub fn value(&self, name: &str) -> ActionValue {
        self.values.get(name).copied().unwrap_or_default()
    }

    pub fn d2(&self, name: &str) -> Vec2 {
        self.value(name).d2()
    }

    pub fn pressed(&self, name: &str) -> bool {
        self.value(name).pressed()
    }

    pub fn pressed_this_frame(&self, name: &str) -> bool {
        self.edges.get(name).map(|e| e.0).unwrap_or(false)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum InputHint {
    Touch,
    Key,
    MouseButton,
    MouseMove { px: f32 },
    GamepadButton,
    GamepadStick { mag: f32 },
}

#[derive(Clone, Debug)]
pub struct PreferredMachine {
    pub current: PreferredInput,
    last_switch_ms: u64,
    switched: bool,
    stick_high_since: Option<u64>,
    mouse_accum: f32,
    mouse_window_start: u64,
}

impl PreferredMachine {
    pub fn new(current: PreferredInput) -> Self {
        Self {
            current,
            last_switch_ms: 0,
            switched: false,
            stick_high_since: None,
            mouse_accum: 0.0,
            mouse_window_start: 0,
        }
    }

    pub fn observe(&mut self, hint: InputHint, now_ms: u64) {
        let candidate = match hint {
            InputHint::Touch => Some(PreferredInput::Touch),
            InputHint::Key | InputHint::MouseButton => Some(PreferredInput::KeyboardAndMouse),
            InputHint::MouseMove { px } => {
                if self.mouse_window_start == 0 || now_ms.saturating_sub(self.mouse_window_start) > 250 {
                    self.mouse_window_start = now_ms;
                    self.mouse_accum = 0.0;
                }
                self.mouse_accum += px.abs();
                (self.mouse_accum > 8.0).then_some(PreferredInput::KeyboardAndMouse)
            }
            InputHint::GamepadButton => Some(PreferredInput::Gamepad),
            InputHint::GamepadStick { mag } => {
                if mag > 0.5 {
                    let since = *self.stick_high_since.get_or_insert(now_ms);
                    (now_ms.saturating_sub(since) >= 150).then_some(PreferredInput::Gamepad)
                } else {
                    self.stick_high_since = None;
                    None
                }
            }
        };
        let Some(next) = candidate else { return };
        if next == self.current {
            return;
        }
        if self.switched && next != PreferredInput::Touch && now_ms.saturating_sub(self.last_switch_ms) < 300 {
            return;
        }
        self.current = next;
        self.last_switch_ms = now_ms;
        self.switched = true;
        if next != PreferredInput::Gamepad {
            self.stick_high_since = None;
        }
    }
}

/// Resolve one frame. Touch ownership is decided here, then contexts.
pub fn step(state: &mut InputState, frame: &DeviceFrame) -> Actions {
    observe_preferred(state, frame);
    if state.preferred.current != PreferredInput::Touch {
        // A keyboard or gamepad takes over. Touch values do not linger.
        if !state.fingers.is_empty() || state.stick.finger.is_some() {
            state.release_touch();
        }
    }
    let touch_on = state.touch_zones || state.preferred.current == PreferredInput::Touch;
    // Cleared before the finger update, so a tap that starts and ends in this
    // frame can set it and `jump_held` still sees the press.
    state.jump_edge = false;
    let (touch_look_px, pinch_px, world_tap) = if touch_on {
        update_fingers(state, frame)
    } else {
        (Vec2::ZERO, 0.0, None)
    };

    let mut claimed_keys: HashSet<KeyCode> = HashSet::new();
    let mut claimed_mouse: HashSet<MouseButton> = HashSet::new();
    let mut claimed_pad: HashSet<GamepadButton> = HashSet::new();
    let mut claim_stick = false;
    let mut claim_mouse_delta = false;
    let mut claim_wheel = false;
    let mut claim_touch_stick = false;
    let mut claim_touch_look = false;
    let mut claim_ui: HashSet<u32> = HashSet::new();
    let mut all_keys = false;

    let mut order: Vec<usize> = (0..state.contexts.len()).filter(|&i| state.contexts[i].enabled).collect();
    order.sort_by(|&a, &b| {
        state.contexts[b].priority.cmp(&state.contexts[a].priority).then(state.contexts[b].enabled_seq.cmp(&state.contexts[a].enabled_seq))
    });

    let mut out = Actions { preferred: state.preferred.current, mouse_look_px: Vec2::ZERO, touch_look_rad: Vec2::ZERO, zoom_notches: 0.0, world_tap, ..default() };

    for index in order {
        let ctx_sink = state.contexts[index].sink;
        let sink_keys = state.contexts[index].sink_all_keys;
        let action_ids = state.contexts[index].actions.clone();
        if sink_keys {
            all_keys = true;
        }
        for id in action_ids {
            let Some(action) = state.actions.get(&id) else { continue };
            if !action.enabled {
                continue;
            }
            let prev = action.state;
            let mut value = ActionValue::None;
            let mut best_len = -1.0_f32;
            for binding in &action.bindings {
                if source_claimed(binding, &claimed_keys, &claimed_mouse, &claimed_pad, claim_stick, claim_mouse_delta, claim_wheel, claim_touch_stick, claim_touch_look, &claim_ui, all_keys) {
                    continue;
                }
                if let Some(next) = eval_binding(binding, frame, state, touch_look_px, pinch_px) {
                    let len = match next {
                        ActionValue::D2(v) => v.length(),
                        ActionValue::D1(v) => v.abs(),
                        ActionValue::Bool(true) => 1.0,
                        ActionValue::Bool(false) | ActionValue::None => 0.0,
                        ActionValue::D3(v) => v.length(),
                        ActionValue::Viewport(_) => 1.0,
                    };
                    if len > best_len {
                        best_len = len;
                        value = next;
                    }
                }
            }
            // A bool with no firing binding is released, not left stale.
            if matches!(action.ty, ActionType::Bool) && best_len <= 0.0 {
                value = ActionValue::Bool(false);
            }
            if matches!(action.ty, ActionType::Direction2D) && best_len < 0.0 {
                value = ActionValue::D2(Vec2::ZERO);
            }
            if matches!(action.ty, ActionType::Direction1D) && best_len < 0.0 {
                value = ActionValue::D1(0.0);
            }
            let name = action.name.clone();
            if let Some(action) = state.actions.get_mut(&id) {
                action.prev = prev;
                action.state = value;
                action.pressed_this_frame = value.pressed() && !prev.pressed();
                action.released_this_frame = !value.pressed() && prev.pressed();
                out.values.insert(name.clone(), value);
                out.edges.insert(name, (action.pressed_this_frame, action.released_this_frame));
            }
            if ctx_sink {
                claim_bindings(&state.actions[&id].bindings, &mut claimed_keys, &mut claimed_mouse, &mut claimed_pad, &mut claim_stick, &mut claim_mouse_delta, &mut claim_wheel, &mut claim_touch_stick, &mut claim_touch_look, &mut claim_ui);
            }
        }
        if ctx_sink && sink_keys {
            all_keys = true;
        }
    }

    // Look keeps the mouse in pixels and the touch delta in radians, side by
    // side, so the desktop sensitivity multiplier is unchanged.
    out.mouse_look_px = frame.mouse_delta_px;
    out.touch_look_rad = touch_look_px * TOUCH_LOOK_SCALE;
    // Pinch uses the World TouchDelta binding scale (-0.01) plus the raw wheel.
    let pinch_notches = pinch_px * -0.01;
    out.zoom_notches = frame.wheel_y + pinch_notches;
    if let Some(zoom) = out.values.get_mut("Zoom") {
        *zoom = ActionValue::D1(out.zoom_notches);
    }
    out
}

fn observe_preferred(state: &mut InputState, frame: &DeviceFrame) {
    if frame.touches.iter().any(|t| t.phase == TouchPhase::Started) {
        state.preferred.observe(InputHint::Touch, frame.now_ms);
    }
    if !frame.keys_just.is_empty() {
        state.preferred.observe(InputHint::Key, frame.now_ms);
    }
    if !frame.mouse_just.is_empty() {
        state.preferred.observe(InputHint::MouseButton, frame.now_ms);
    }
    if frame.mouse_delta_px.length_squared() > 0.0 {
        state.preferred.observe(InputHint::MouseMove { px: frame.mouse_delta_px.length() }, frame.now_ms);
    }
    if !frame.pad_buttons.is_empty() {
        state.preferred.observe(InputHint::GamepadButton, frame.now_ms);
    }
    state.preferred.observe(InputHint::GamepadStick { mag: frame.pad_stick.length() }, frame.now_ms);
}

fn source_claimed(
    binding: &InputBinding,
    keys: &HashSet<KeyCode>,
    mouse: &HashSet<MouseButton>,
    pad: &HashSet<GamepadButton>,
    stick: bool,
    mouse_delta: bool,
    wheel: bool,
    touch_stick: bool,
    touch_look: bool,
    ui: &HashSet<u32>,
    all_keys: bool,
) -> bool {
    match &binding.source {
        BindingSource::Key(k) => all_keys || keys.contains(k),
        BindingSource::Composite { up, down, left, right } => {
            all_keys || [up, down, left, right].into_iter().any(|k| keys.contains(k))
        }
        BindingSource::MouseButton(b) => mouse.contains(b),
        BindingSource::MouseDelta => mouse_delta,
        BindingSource::MouseWheel => wheel,
        BindingSource::GamepadButton(b) => pad.contains(b),
        BindingSource::GamepadStick => stick,
        BindingSource::TouchZone(_) => touch_stick,
        BindingSource::TouchDelta(_) => touch_look,
        BindingSource::UiButton(id) => ui.contains(id),
    }
}

fn claim_bindings(
    bindings: &[InputBinding],
    keys: &mut HashSet<KeyCode>,
    mouse: &mut HashSet<MouseButton>,
    pad: &mut HashSet<GamepadButton>,
    stick: &mut bool,
    mouse_delta: &mut bool,
    wheel: &mut bool,
    touch_stick: &mut bool,
    touch_look: &mut bool,
    ui: &mut HashSet<u32>,
) {
    for binding in bindings {
        match &binding.source {
            BindingSource::Key(k) => { keys.insert(*k); }
            BindingSource::Composite { up, down, left, right } => {
                keys.extend([*up, *down, *left, *right]);
            }
            BindingSource::MouseButton(b) => { mouse.insert(*b); }
            BindingSource::MouseDelta => *mouse_delta = true,
            BindingSource::MouseWheel => *wheel = true,
            BindingSource::GamepadButton(b) => { pad.insert(*b); }
            BindingSource::GamepadStick => *stick = true,
            BindingSource::TouchZone(_) => *touch_stick = true,
            BindingSource::TouchDelta(_) => *touch_look = true,
            BindingSource::UiButton(id) => { ui.insert(*id); }
        }
    }
}

fn eval_binding(binding: &InputBinding, frame: &DeviceFrame, state: &InputState, touch_look_px: Vec2, pinch_px: f32) -> Option<ActionValue> {
    let scale = binding.scale;
    let invert = if binding.invert_y { -1.0 } else { 1.0 };
    match &binding.source {
        BindingSource::Key(k) => Some(ActionValue::Bool(frame.keys_down.contains(k))),
        BindingSource::MouseButton(b) => Some(ActionValue::Bool(frame.mouse_down.contains(b))),
        BindingSource::GamepadButton(b) => Some(ActionValue::Bool(frame.pad_buttons.contains(b))),
        BindingSource::UiButton(id) => Some(ActionValue::Bool(state.ui_held.contains(id) || jump_held(state, *id))),
        BindingSource::Composite { up, down, left, right } => {
            let mut v = Vec2::ZERO;
            if frame.keys_down.contains(right) { v.x += 1.0; }
            if frame.keys_down.contains(left) { v.x -= 1.0; }
            if frame.keys_down.contains(up) { v.y += 1.0; }
            if frame.keys_down.contains(down) { v.y -= 1.0; }
            Some(ActionValue::D2(v * scale))
        }
        BindingSource::GamepadStick => {
            let v = radial(frame.pad_stick, binding.deadzone.max(0.1));
            Some(ActionValue::D2(Vec2::new(v.x, v.y * invert) * scale))
        }
        BindingSource::TouchZone(TouchZoneId::Thumbstick) => {
            let v = radial(state.stick.value, binding.deadzone.max(STICK_DEADZONE));
            // The finger's offset is in screen space (Y down). W is +Y.
            let y = if binding.invert_y { -v.y } else { v.y };
            Some(ActionValue::D2(Vec2::new(v.x * scale, y * scale)))
        }
        BindingSource::MouseDelta => Some(ActionValue::D2(frame.mouse_delta_px * scale)),
        BindingSource::TouchDelta(TouchZoneId::Camera) => Some(ActionValue::D2(touch_look_px * scale)),
        BindingSource::MouseWheel => Some(ActionValue::D1(frame.wheel_y * scale)),
        BindingSource::TouchDelta(TouchZoneId::World) => Some(ActionValue::D1(pinch_px * scale)),
        BindingSource::TouchZone(_) | BindingSource::TouchDelta(_) => None,
    }
}

fn jump_held(state: &InputState, id: u32) -> bool {
    if id != JUMP_BUTTON {
        return false;
    }
    state.jump_edge
        || state.fingers.values().any(|f| f.owner == FingerOwner::Ui && point_in_jump(state, f.pos))
}

fn radial(v: Vec2, dead: f32) -> Vec2 {
    let len = v.length();
    if len <= dead || len == 0.0 {
        Vec2::ZERO
    } else {
        let out = ((len - dead) / (1.0 - dead)).clamp(0.0, 1.0);
        v / len * out
    }
}

/// Bottom-left activation region, in logical pixels, inside the safe area.
pub fn thumbstick_zone(viewport: Vec2, insets: SafeInsets, pos: Vec2) -> bool {
    let safe_w = (viewport.x - insets.left - insets.right).max(1.0);
    let safe_h = (viewport.y - insets.top - insets.bottom).max(1.0);
    let x = pos.x - insets.left;
    let y = pos.y - insets.top;
    x >= 0.0 && y >= 0.0 && x < safe_w * 0.40 && y > safe_h * 0.35 && pos.x < viewport.x && pos.y < viewport.y
}

/// Where the thumbstick rests while no finger is down: the jump circle,
/// mirrored into the lower left, using the same safe-area inset.
pub fn thumbstick_rest(viewport: Vec2, insets: SafeInsets) -> Vec2 {
    Vec2::new(
        insets.left + 24.0 + STICK_RADIUS,
        viewport.y - insets.bottom - 24.0 - STICK_RADIUS,
    )
}

/// Jump diameter, clamped, and its center inset 24px from the safe edges.
pub fn jump_button(viewport: Vec2, insets: SafeInsets) -> (Vec2, f32) {
    let safe_w = (viewport.x - insets.left - insets.right).max(1.0);
    let safe_h = (viewport.y - insets.top - insets.bottom).max(1.0);
    let diameter = (0.12 * safe_w.min(safe_h)).clamp(64.0, 120.0);
    let radius = diameter * 0.5;
    let center = Vec2::new(
        viewport.x - insets.right - 24.0 - radius,
        viewport.y - insets.bottom - 24.0 - radius,
    );
    (center, radius)
}

fn point_in_jump(state: &InputState, pos: Vec2) -> bool {
    let (center, radius) = jump_button(state.viewport, state.insets);
    pos.distance(center) <= radius
}

fn in_ui(state: &InputState, pos: Vec2) -> bool {
    state.ui_rects.iter().any(|r| r.contains(pos)) || point_in_jump(state, pos)
}

impl InputState {
    /// This finger is owned by the UI for its whole life. A finger that ended
    /// this frame is in [`Self::released_ui`] instead.
    pub fn finger_is_ui(&self, id: u64) -> bool {
        self.fingers.get(&id).is_some_and(|finger| finger.owner == FingerOwner::Ui)
    }

    /// Thumbstick, camera drag, or pinch. The shell does not take these.
    pub fn finger_is_world_gesture(&self, id: u64) -> bool {
        self.stick.finger == Some(id)
            || self.camera_finger == Some(id)
            || self.pinch.is_some_and(|(a, b, _)| a == id || b == id)
            || self.fingers.get(&id).is_some_and(|finger| {
                matches!(finger.owner, FingerOwner::Stick | FingerOwner::Camera | FingerOwner::Pinch)
            })
    }
}

fn update_fingers(state: &mut InputState, frame: &DeviceFrame) -> (Vec2, f32, Option<Vec2>) {
    let mut look = Vec2::ZERO;
    let mut pinch_px = 0.0;
    let mut world_tap = None;
    state.released_ui.clear();
    for touch in &frame.touches {
        match touch.phase {
            TouchPhase::Started => {
                let owner = assign_owner(state, touch.pos);
                if owner == FingerOwner::Stick && state.stick.finger.is_none() {
                    state.stick.finger = Some(touch.id);
                    state.stick.origin = touch.pos;
                    state.stick.value = Vec2::ZERO;
                }
                if owner == FingerOwner::Camera {
                    state.camera_finger = Some(touch.id);
                }
                state.fingers.insert(touch.id, Finger { owner, pos: touch.pos, start: touch.pos, moved: 0.0 });
            }
            TouchPhase::Moved => {
                let Some(finger) = state.fingers.get_mut(&touch.id) else { continue };
                let delta = touch.pos - finger.pos;
                finger.moved += delta.length();
                finger.pos = touch.pos;
                let owner = finger.owner;
                if owner == FingerOwner::Stick && state.stick.finger == Some(touch.id) {
                    let offset = touch.pos - state.stick.origin;
                    let len = offset.length();
                    state.stick.value = if len <= STICK_RADIUS || len == 0.0 {
                        offset / STICK_RADIUS
                    } else {
                        offset / len
                    };
                }
                if owner == FingerOwner::Camera && state.camera_finger == Some(touch.id) {
                    look += delta;
                }
            }
            TouchPhase::Ended | TouchPhase::Canceled => {
                let finished = state.fingers.remove(&touch.id);
                if state.stick.finger == Some(touch.id) {
                    state.stick.finger = None;
                    state.stick.value = Vec2::ZERO;
                }
                if state.camera_finger == Some(touch.id) {
                    state.camera_finger = None;
                }
                if let Some((a, b, _)) = state.pinch {
                    if a == touch.id || b == touch.id {
                        state.pinch = None;
                    }
                }
                if let Some(finger) = finished {
                    if finger.owner == FingerOwner::Ui {
                        state.released_ui.push(touch.id);
                        // The circle test uses where the finger went down, so a
                        // release a few pixels outside the ring still counts.
                        if point_in_jump(state, finger.start) || point_in_jump(state, touch.pos) {
                            state.jump_edge = true;
                        }
                    }
                    if touch.phase == TouchPhase::Ended
                        && finger.owner == FingerOwner::World
                        && finger.moved < 10.0
                    {
                        world_tap = Some(finger.pos);
                    }
                }
            }
        }
    }
    // A second camera-zone finger turns both into a pinch.
    let camera_fingers: Vec<u64> = state.fingers.iter().filter(|(_, f)| f.owner == FingerOwner::Camera).map(|(id, _)| *id).collect();
    if camera_fingers.len() >= 2 {
        let a = camera_fingers[0].min(camera_fingers[1]);
        let b = camera_fingers[0].max(camera_fingers[1]);
        for id in [a, b] {
            if let Some(f) = state.fingers.get_mut(&id) {
                f.owner = FingerOwner::Pinch;
            }
        }
        state.camera_finger = None;
        look = Vec2::ZERO;
        let pa = state.fingers.get(&a).map(|f| f.pos).unwrap_or(Vec2::ZERO);
        let pb = state.fingers.get(&b).map(|f| f.pos).unwrap_or(Vec2::ZERO);
        let dist = pa.distance(pb);
        if let Some((pa_id, pb_id, prev)) = state.pinch {
            if pa_id == a && pb_id == b {
                pinch_px = dist - prev;
            }
        }
        state.pinch = Some((a, b, dist));
    }
    // After the first frame the two fingers are Pinch, not Camera, so the
    // block above no longer sees them. Keep measuring that pair.
    if let Some((a, b, prev)) = state.pinch {
        if let (Some(fa), Some(fb)) = (state.fingers.get(&a), state.fingers.get(&b)) {
            let dist = fa.pos.distance(fb.pos);
            pinch_px = dist - prev;
            look = Vec2::ZERO;
            state.pinch = Some((a, b, dist));
        }
    }
    // A stick whose finger is already gone still reported its last offset.
    if state.stick.finger.is_some_and(|id| !state.fingers.contains_key(&id)) {
        state.stick.finger = None;
        state.stick.value = Vec2::ZERO;
    }
    (look, pinch_px, world_tap)
}

fn assign_owner(state: &InputState, pos: Vec2) -> FingerOwner {
    if state.capture_all || in_ui(state, pos) {
        return FingerOwner::Ui;
    }
    if thumbstick_zone(state.viewport, state.insets, pos) {
        if state.stick.finger.is_none() {
            return FingerOwner::Stick;
        }
    }
    let right = pos.x >= state.viewport.x * 0.5;
    if right && !point_in_jump(state, pos) {
        if state.camera_finger.is_none() {
            return FingerOwner::Camera;
        }
        return FingerOwner::Camera;
    }
    FingerOwner::World
}

/// Bevy systems. `PreUpdate`, after device input is collected.
pub struct InputActionsPlugin;

impl Plugin for InputActionsPlugin {
    fn build(&self, app: &mut App) {
        let preferred = initial_preferred();
        app.insert_resource(InputState::player_defaults(preferred))
            .init_resource::<Actions>()
            .add_systems(PreUpdate, drive_input);
    }
}

fn initial_preferred() -> PreferredInput {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        PreferredInput::Touch
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        PreferredInput::KeyboardAndMouse
    }
}

fn drive_input(
    mut state: ResMut<InputState>,
    mut actions: ResMut<Actions>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheels: MessageReader<MouseWheel>,
    mut touches: MessageReader<TouchInput>,
    pads: Query<&Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    typing: Option<Res<crate::play_session::GuiKeyboardFocus>>,
    time: Res<Time>,
) {
    if let Some(window) = windows.iter().next() {
        state.viewport = Vec2::new(window.resolution.width(), window.resolution.height());
    }
    state.set_context_enabled("TextFocus", typing.as_deref().is_some_and(|t| t.0));

    let mut frame = DeviceFrame { now_ms: time.elapsed().as_millis() as u64, ..default() };
    for key in keys.get_pressed() {
        frame.keys_down.insert(*key);
    }
    for key in keys.get_just_pressed() {
        frame.keys_just.insert(*key);
    }
    for button in mouse_buttons.get_pressed() {
        frame.mouse_down.insert(*button);
    }
    for button in mouse_buttons.get_just_pressed() {
        frame.mouse_just.insert(*button);
    }
    for event in motion.read() {
        frame.mouse_delta_px += event.delta;
    }
    for event in wheels.read() {
        let y = match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 16.0,
        };
        frame.wheel_y += y;
    }
    for touch in touches.read() {
        // Bevy 0.19's winit backend already stores `TouchInput.position` in
        // logical pixels (`location.to_logical(scale_factor)`). Dividing again
        // sent the thumbstick and the UI hit test to 1/scale of the real point.
        frame.touches.push(TouchSample {
            id: touch.id,
            phase: touch.phase,
            pos: touch.position,
        });
    }
    if let Some(pad) = pads.iter().next() {
        frame.pad_stick = Vec2::new(
            pad.get(GamepadAxis::LeftStickX).unwrap_or(0.0),
            pad.get(GamepadAxis::LeftStickY).unwrap_or(0.0),
        );
        for button in [GamepadButton::South, GamepadButton::East, GamepadButton::West, GamepadButton::North] {
            if pad.pressed(button) {
                frame.pad_buttons.insert(button);
            }
        }
    }
    *actions = step(&mut state, &frame);
    // Mirror the live choice onto the older service when a shell has one.
}

/// Camera-relative movement. Length above 1 is normalised (keyboard
/// diagonals); a shorter stick value is kept so a half-tilt walks.
pub fn camera_relative_direction(local_x: f32, local_z: f32, yaw: f32) -> Vec3 {
    let mut local = Vec3::new(local_x, 0.0, local_z);
    let len = local.length();
    if len > 1.0 {
        local /= len;
    }
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    forward * -local.z + right * local.x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(keys: &[KeyCode]) -> DeviceFrame {
        DeviceFrame { keys_down: keys.iter().copied().collect(), ..default() }
    }

    #[test]
    fn keyboard_move_matches_wasd() {
        let mut state = InputState::player_defaults(PreferredInput::KeyboardAndMouse);
        let out = step(&mut state, &press(&[KeyCode::KeyW]));
        assert_eq!(out.d2("Move"), Vec2::new(0.0, 1.0));
        let yaw = 0.0;
        let dir = camera_relative_direction(out.d2("Move").x, -out.d2("Move").y, yaw);
        assert!((dir - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-5, "{dir}");

        let out = step(&mut state, &press(&[KeyCode::KeyW, KeyCode::KeyD]));
        let dir = camera_relative_direction(out.d2("Move").x, -out.d2("Move").y, yaw);
        assert!((dir.length() - 1.0).abs() < 1e-4, "a diagonal is full speed, not faster");
        assert!(dir.x > 0.5 && dir.z < -0.5, "{dir}");
    }

    #[test]
    fn jump_sprint_and_crouch_follow_the_desktop_keys() {
        let mut state = InputState::player_defaults(PreferredInput::KeyboardAndMouse);
        let first = step(&mut state, &press(&[KeyCode::Space, KeyCode::ShiftLeft]));
        assert!(first.pressed("Jump") && first.pressed_this_frame("Jump"));
        assert!(first.pressed("Sprint"));
        assert!(!first.pressed("Crouch"));
        let held = step(&mut state, &press(&[KeyCode::Space, KeyCode::ControlLeft]));
        assert!(held.pressed("Jump") && !held.pressed_this_frame("Jump"));
        assert!(held.pressed("Crouch"));
        assert!(!held.pressed("Sprint"));
    }

    #[test]
    fn a_sinking_context_hides_its_keys_from_movement() {
        let mut state = InputState::player_defaults(PreferredInput::KeyboardAndMouse);
        state.set_context_enabled("TextFocus", true);
        let out = step(&mut state, &press(&[KeyCode::KeyW, KeyCode::Space]));
        assert_eq!(out.d2("Move"), Vec2::ZERO, "a focused text field sinks every key");
        assert!(!out.pressed("Jump"));
    }

    #[test]
    fn higher_priority_sink_claims_only_its_bindings() {
        let mut state = InputState::player_defaults(PreferredInput::KeyboardAndMouse);
        // SystemMenu binds W and sinks. Space is not its binding, so Jump still sees it.
        let menu = state.contexts.iter().position(|c| c.name == "SystemMenu").unwrap();
        state.add_action(menu, "Back", ActionType::Bool, vec![InputBinding::new(BindingSource::Key(KeyCode::KeyW))]);
        state.set_context_enabled("SystemMenu", true);
        let out = step(&mut state, &press(&[KeyCode::KeyW, KeyCode::Space]));
        assert_eq!(out.d2("Move"), Vec2::ZERO);
        assert!(out.pressed("Jump"));
        assert!(out.pressed("Back"));
    }

    #[test]
    fn dynamic_thumbstick_spawns_at_the_finger_and_one_finger_owns_it() {
        let mut state = InputState::player_defaults(PreferredInput::Touch);
        state.touch_zones = true;
        state.viewport = Vec2::new(1000.0, 500.0);
        let origin = Vec2::new(80.0, 400.0);
        assert!(thumbstick_zone(state.viewport, state.insets, origin));
        let start = DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Started, pos: origin }],
            now_ms: 1,
            ..default()
        };
        step(&mut state, &start);
        assert_eq!(state.stick.finger, Some(1));
        assert_eq!(state.stick.origin, origin);
        let drag = DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Moved, pos: origin + Vec2::new(STICK_RADIUS, 0.0) }],
            now_ms: 2,
            ..default()
        };
        let out = step(&mut state, &drag);
        let v = out.d2("Move");
        assert!((v.x - 1.0).abs() < 0.05, "{v}");
        assert!(v.y.abs() < 0.05, "a sideways drag is not forward, got {v}");
        // A second finger in the zone does not steal the stick.
        let second = DeviceFrame {
            touches: vec![TouchSample { id: 2, phase: TouchPhase::Started, pos: origin + Vec2::new(10.0, 10.0) }],
            now_ms: 3,
            ..default()
        };
        step(&mut state, &second);
        assert_eq!(state.stick.finger, Some(1));
        assert_ne!(state.fingers.get(&2).map(|f| f.owner), Some(FingerOwner::Stick));
    }

    #[test]
    fn stick_up_is_forward_like_w() {
        let mut state = InputState::player_defaults(PreferredInput::Touch);
        state.viewport = Vec2::new(1000.0, 500.0);
        let origin = Vec2::new(80.0, 400.0);
        step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Started, pos: origin }],
            ..default()
        });
        // Screen Y grows downward, so the finger moving up is a negative delta.
        let out = step(&mut state, &DeviceFrame {
            touches: vec![TouchSample {
                id: 1,
                phase: TouchPhase::Moved,
                pos: origin + Vec2::new(0.0, -STICK_RADIUS),
            }],
            ..default()
        });
        let v = out.d2("Move");
        assert!(v.y > 0.9, "finger up is +Move.y, the same sign as W, got {v}");
        let dir = camera_relative_direction(v.x, -v.y, 0.0);
        assert!(
            (dir - Vec3::new(0.0, 0.0, -1.0)).length() < 0.05,
            "at yaw 0 that walk is camera-forward (-Z), got {dir}"
        );
    }

    #[test]
    fn stick_deadzone_zeroes_a_small_tilt() {
        let mut state = InputState::player_defaults(PreferredInput::Touch);
        state.viewport = Vec2::new(1000.0, 500.0);
        let origin = Vec2::new(80.0, 400.0);
        step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Started, pos: origin }],
            ..default()
        });
        let out = step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Moved, pos: origin + Vec2::new(STICK_RADIUS * 0.05, 0.0) }],
            ..default()
        });
        assert_eq!(out.d2("Move"), Vec2::ZERO);
    }

    #[test]
    fn a_ui_finger_never_drives_the_stick_or_the_camera() {
        let mut state = InputState::player_defaults(PreferredInput::Touch);
        state.viewport = Vec2::new(1000.0, 500.0);
        state.ui_rects.push(Rect::from_corners(Vec2::ZERO, Vec2::new(1000.0, 80.0)));
        let out = step(&mut state, &DeviceFrame {
            touches: vec![
                TouchSample { id: 7, phase: TouchPhase::Started, pos: Vec2::new(100.0, 20.0) },
                TouchSample { id: 7, phase: TouchPhase::Moved, pos: Vec2::new(180.0, 40.0) },
            ],
            ..default()
        });
        assert_eq!(state.fingers.get(&7).map(|f| f.owner), Some(FingerOwner::Ui));
        assert_eq!(out.d2("Move"), Vec2::ZERO);
        assert_eq!(out.touch_look_rad, Vec2::ZERO);
    }

    #[test]
    fn camera_drag_on_the_right_is_a_hundredth_of_a_radian_per_pixel() {
        let mut state = InputState::player_defaults(PreferredInput::Touch);
        state.viewport = Vec2::new(1000.0, 500.0);
        let start = Vec2::new(800.0, 200.0);
        assert!(!thumbstick_zone(state.viewport, state.insets, start));
        step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 3, phase: TouchPhase::Started, pos: start }],
            ..default()
        });
        assert_eq!(state.fingers.get(&3).map(|f| f.owner), Some(FingerOwner::Camera));
        let out = step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 3, phase: TouchPhase::Moved, pos: start + Vec2::new(100.0, -50.0) }],
            ..default()
        });
        assert!((out.touch_look_rad.x - 1.0).abs() < 1e-4, "{}", out.touch_look_rad.x);
        assert!((out.touch_look_rad.y - -0.5).abs() < 1e-4, "{}", out.touch_look_rad.y);
    }

    #[test]
    fn a_second_camera_finger_pinches_instead_of_looking() {
        let mut state = InputState::player_defaults(PreferredInput::Touch);
        state.viewport = Vec2::new(1000.0, 500.0);
        step(&mut state, &DeviceFrame {
            touches: vec![
                TouchSample { id: 1, phase: TouchPhase::Started, pos: Vec2::new(700.0, 200.0) },
                TouchSample { id: 2, phase: TouchPhase::Started, pos: Vec2::new(800.0, 200.0) },
            ],
            ..default()
        });
        assert_eq!(state.fingers.get(&1).map(|f| f.owner), Some(FingerOwner::Pinch));
        assert_eq!(state.fingers.get(&2).map(|f| f.owner), Some(FingerOwner::Pinch));
        let out = step(&mut state, &DeviceFrame {
            touches: vec![
                TouchSample { id: 1, phase: TouchPhase::Moved, pos: Vec2::new(650.0, 200.0) },
                TouchSample { id: 2, phase: TouchPhase::Moved, pos: Vec2::new(850.0, 200.0) },
            ],
            ..default()
        });
        assert_eq!(out.touch_look_rad, Vec2::ZERO);
        assert!(out.zoom_notches < 0.0, "fingers apart zoom out ({})", out.zoom_notches);
    }

    #[test]
    fn jump_layout_is_inset_from_the_safe_edges() {
        let insets = SafeInsets { top: 10.0, bottom: 20.0, left: 5.0, right: 30.0 };
        let viewport = Vec2::new(800.0, 400.0);
        let (center, radius) = jump_button(viewport, insets);
        assert!((radius * 2.0 - 64.0).abs() < 0.1 || (radius * 2.0 - (0.12 * 350.0)).abs() < 0.1);
        assert!((center.x - (800.0 - 30.0 - 24.0 - radius)).abs() < 1e-3);
        assert!((center.y - (400.0 - 20.0 - 24.0 - radius)).abs() < 1e-3);
        let diameter = radius * 2.0;
        assert!((64.0..=120.0).contains(&diameter));
    }

    #[test]
    fn preferred_input_hysteresis_lets_touch_through_and_holds_the_others() {
        let mut m = PreferredMachine::new(PreferredInput::KeyboardAndMouse);
        m.observe(InputHint::Touch, 1_000);
        assert_eq!(m.current, PreferredInput::Touch);
        m.observe(InputHint::Key, 1_100);
        assert_eq!(m.current, PreferredInput::Touch, "a key inside 300ms does not flicker back");
        m.observe(InputHint::Key, 1_400);
        assert_eq!(m.current, PreferredInput::KeyboardAndMouse);
        m.observe(InputHint::Touch, 1_450);
        assert_eq!(m.current, PreferredInput::Touch, "touch is immediate even inside the hold");
    }

    #[test]
    fn stick_drift_does_not_select_a_gamepad_and_a_real_deflection_does() {
        let mut m = PreferredMachine::new(PreferredInput::KeyboardAndMouse);
        m.observe(InputHint::GamepadStick { mag: 0.1 }, 0);
        m.observe(InputHint::GamepadStick { mag: 0.1 }, 5_000);
        assert_eq!(m.current, PreferredInput::KeyboardAndMouse);
        m.observe(InputHint::GamepadStick { mag: 0.8 }, 10_000);
        assert_eq!(m.current, PreferredInput::KeyboardAndMouse, "150ms has not elapsed");
        m.observe(InputHint::GamepadStick { mag: 0.8 }, 10_150);
        assert_eq!(m.current, PreferredInput::Gamepad);
    }

    #[test]
    fn mouse_jitter_under_eight_pixels_does_not_leave_touch() {
        let mut m = PreferredMachine::new(PreferredInput::Touch);
        m.switched = true;
        m.last_switch_ms = 0;
        m.observe(InputHint::MouseMove { px: 3.0 }, 1_000);
        m.observe(InputHint::MouseMove { px: 3.0 }, 1_100);
        assert_eq!(m.current, PreferredInput::Touch);
        m.observe(InputHint::MouseMove { px: 4.0 }, 1_150);
        assert_eq!(m.current, PreferredInput::KeyboardAndMouse);
    }

    #[test]
    fn cancel_releases_the_stick() {
        let mut state = InputState::player_defaults(PreferredInput::Touch);
        state.viewport = Vec2::new(1000.0, 500.0);
        let origin = Vec2::new(80.0, 400.0);
        step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Started, pos: origin }],
            ..default()
        });
        step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Canceled, pos: origin }],
            ..default()
        });
        assert!(state.stick.finger.is_none());
        assert_eq!(state.stick.value, Vec2::ZERO);
        assert!(state.fingers.is_empty());
    }

    #[test]
    fn lift_resets_the_stick() {
        let mut state = InputState::player_defaults(PreferredInput::Touch);
        state.viewport = Vec2::new(1000.0, 500.0);
        let origin = Vec2::new(80.0, 400.0);
        step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Started, pos: origin }],
            ..default()
        });
        step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Moved, pos: origin + Vec2::new(STICK_RADIUS, 0.0) }],
            ..default()
        });
        let (_, knob) = state.stick_visual().expect("knob while the finger is down");
        assert!(knob.x > origin.x + 1.0, "the knob follows the finger, got {knob}");
        step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Ended, pos: origin + Vec2::new(STICK_RADIUS, 0.0) }],
            ..default()
        });
        assert!(state.stick.finger.is_none());
        assert_eq!(state.stick.value, Vec2::ZERO);
        assert!(state.stick_visual().is_none(), "a lifted finger does not keep an offset knob");
        assert!(state.fingers.is_empty());
    }

    /// A finger dragged past the ring keeps the knob on the rim, in the
    /// direction of the finger. Screen Y is down, so a drag up is −Y.
    #[test]
    fn the_knob_tracks_the_finger_and_stays_inside_the_ring() {
        let mut state = InputState::player_defaults(PreferredInput::Touch);
        state.viewport = Vec2::new(1000.0, 500.0);
        let origin = Vec2::new(80.0, 400.0);
        step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Started, pos: origin }],
            ..default()
        });
        let (rest_origin, rest_knob) = state.stick_visual().expect("ring under the finger");
        assert_eq!(rest_origin, origin);
        assert_eq!(rest_knob, origin, "a finger that has not moved leaves the knob centred");
        let moved = origin + Vec2::new(10.0, -30.0);
        step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Moved, pos: moved }],
            ..default()
        });
        let (at, knob) = state.stick_visual().expect("knob while held");
        assert_eq!(at, origin, "the ring stays where the finger went down");
        assert!((knob - moved).length() < 1.0, "inside the ring the knob sits on the finger, got {knob} want {moved}");
        let far = origin + Vec2::new(0.0, -400.0);
        step(&mut state, &DeviceFrame {
            touches: vec![TouchSample { id: 1, phase: TouchPhase::Moved, pos: far }],
            ..default()
        });
        let (_, clamped) = state.stick_visual().expect("knob while held past the rim");
        let offset = clamped - origin;
        assert!(
            (offset.length() - STICK_RADIUS).abs() < 0.05,
            "past the rim the knob stays on the ring, offset {offset} len {}",
            offset.length()
        );
        assert!(offset.y < 0.0, "a finger above the ring puts the knob above it, got {clamped}");
    }

    /// The debug player often delivers Started and Ended in one frame. Jump
    /// still has to press, or the button does nothing on device.
    #[test]
    fn a_jump_tap_that_starts_and_ends_in_one_frame_still_presses() {
        let mut state = InputState::player_defaults(PreferredInput::Touch);
        state.touch_zones = true;
        state.viewport = Vec2::new(914.3, 411.4);
        let (center, radius) = jump_button(state.viewport, state.insets);
        let pos = center + Vec2::new(0.0, -24.0);
        assert!(pos.distance(center) <= radius, "the device tap is inside the circle");
        let out = step(&mut state, &DeviceFrame {
            touches: vec![
                TouchSample { id: 7, phase: TouchPhase::Started, pos },
                TouchSample { id: 7, phase: TouchPhase::Ended, pos },
            ],
            now_ms: 8,
            ..default()
        });
        assert!(
            out.pressed("Jump") && out.pressed_this_frame("Jump"),
            "a same-frame jump tap must reach the Jump action"
        );
        let next = step(&mut state, &DeviceFrame::default());
        assert!(!next.pressed("Jump"), "the tap does not stay held");
    }

    #[test]
    fn mouse_look_pixels_are_preserved_for_the_desktop_sensitivity() {
        let mut state = InputState::player_defaults(PreferredInput::KeyboardAndMouse);
        let out = step(&mut state, &DeviceFrame { mouse_delta_px: Vec2::new(10.0, -4.0), ..default() });
        assert_eq!(out.mouse_look_px, Vec2::new(10.0, -4.0));
        assert_eq!(out.zoom_notches, 0.0);
        let out = step(&mut state, &DeviceFrame { wheel_y: 2.0, ..default() });
        assert!((out.zoom_notches - 2.0).abs() < 1e-5);
    }
}

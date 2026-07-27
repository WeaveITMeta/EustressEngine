//! # Modal Tool Framework (Rust-first)
//!
//! A **modal tool** is a tool that owns the cursor until the user
//! explicitly commits or cancels — Gap Fill, Resize Align, Edge Align,
//! Part Swap, Model Reflect, and the CAD authoring tools all follow
//! this pattern. Contrast with non-modal tools (Select / Move / Rotate /
//! Scale) which stay loose: pressing another tool's shortcut flips
//! instantly.
//!
//! ## Design
//!
//! The framework is **entirely Rust** — the Slint surface is a
//! reflection of state, not a source of truth. Tool implementations
//! live in concrete types that implement [`ModalTool`]; the runtime
//! holds one boxed instance in [`ActiveModalTool`] at a time.
//!
//! A tool's lifecycle:
//!
//! 1. User activates via keybinding, ribbon button, or MCP — engine
//!    constructs the tool via its [`ModalToolRegistry`] factory entry,
//!    sets [`ActiveModalTool`] to `Some(..)`.
//! 2. Every frame, [`ActiveModalTool`]'s systems route viewport hover /
//!    click / drag / keyboard / numeric input to the tool via
//!    [`ModalTool`] callbacks. The tool mutates its own internal state,
//!    maintains any preview entities.
//! 3. Backspace calls [`ModalTool::on_step_back`] — retract the LAST
//!    pick without discarding the session. A four-click tool that
//!    mis-picked step three steps back to step three, not to zero;
//!    Escape remains the "throw the whole thing away" key.
//! 4. On [`ToolStepResult::Commit`] — the tool writes its result to the
//!    world via [`ModalTool::commit`], despawns preview entities, and
//!    the runtime clears [`ActiveModalTool`].
//! 5. On [`ToolStepResult::Cancel`] / Esc / right-click — `cancel()` is
//!    called, preview entities despawn, no world mutation.
//! 6. On successful commit, the runtime optionally auto-switches back
//!    to Select tool (opt-out per tool via [`ModalTool::auto_exit_on_commit`]).
//!
//! ## Pointer gestures
//!
//! A tool declares whether it cares about press-move-release by
//! overriding [`ModalTool::wants_drag`]. The two paths are entirely
//! separate so that opting in never changes the feel of the tools that
//! didn't.
//!
//! **`wants_drag() == false`** — the default, and what every pick-driven
//! tool uses. [`ModalTool::on_click`] fires on the frame the left button
//! goes down; [`ModalTool::on_hover`] fires on every other frame.
//! Nothing is deferred, nothing is buffered.
//!
//! **`wants_drag() == true`** — the press is *armed* rather than
//! delivered, and the gesture resolves afterwards:
//!
//! - Press arms the gesture and captures the [`ViewportHit`] under the
//!   cursor as the anchor. `on_click` does NOT fire yet.
//! - While the cursor stays inside a [`DRAG_THRESHOLD_PX`] dead zone the
//!   tool keeps receiving `on_hover`, so previews still track.
//! - Leaving the dead zone promotes the gesture:
//!   [`ModalTool::on_drag_start`] fires exactly once with the anchor
//!   hit, immediately followed by the first [`ModalTool::on_drag`] so
//!   the tool sees the live cursor on that same frame rather than one
//!   frame stale. `on_drag` then repeats every frame, always receiving
//!   the anchor as `start` — tools measure against the true origin,
//!   never against last frame's position.
//! - Release resolves it: [`ModalTool::on_drag_end`] if the dead zone
//!   was ever left, otherwise [`ModalTool::on_click`] — a click is a
//!   drag of zero length.
//!
//! Every `on_drag_start` is followed by exactly one `on_drag_end`,
//! unless the tool ended the session itself first by returning Commit or
//! Cancel. If the cursor wanders over UI or out of the viewport
//! mid-gesture, the drag closes out at the last in-viewport hit and the
//! press is forgotten, so a stale press can never resurrect a drag on
//! re-entry.
//! This is what makes drag-to-size, rubber-band select, sketch-a-stroke
//! and paint-along-surface expressible without each tool re-deriving
//! its own click bookkeeping.
//!
//! ## Why this trait pattern
//!
//! - **No Slint coupling.** A ModalTool can be driven from MCP, Rune,
//!   keyboard, or UI without changes.
//! - **Pure ECS interop.** Tools read + write the World via the `ctx`
//!   argument; no hidden global state.
//! - **Testable.** A tool can be instantiated in isolation, fed
//!   synthetic hover/click events, and its commit result inspected.
//! - **Scriptable.** Rune will wrap this trait for user-authored tools
//!   once the Rune ECS bindings settle (TOOLSET.md Phase 2).
//!
//! ## Relation to non-modal tools
//!
//! Non-modal tools (Move/Rotate/Scale) do NOT implement this trait.
//! They run as Bevy systems that execute conditionally on
//! `StudioState::current_tool`. Modal tools SUPERSEDE them — when a
//! modal tool is active, non-modal interaction handlers must early-exit
//! to avoid competing for the cursor. The [`is_modal_tool_active`]
//! helper is the single check point.

use bevy::prelude::*;
use crate::ui::{StudioState, Tool};

// ============================================================================
// Tool interaction types
// ============================================================================

/// How far (in logical pixels) the cursor must travel from the press
/// point before a left-mouse gesture counts as a drag rather than a
/// click. Below this the press is still "in the dead zone" and a
/// release delivers `on_click`.
///
/// Deliberately the same 4px the viewport right-click detector uses
/// (`ui::viewport_context_menu`) — the hand should not have to learn
/// two different notions of "did I move the mouse".
pub const DRAG_THRESHOLD_PX: f32 = 4.0;

/// Result of a single interaction step (click, keyboard, numeric input).
/// Tells the runtime whether the session continues, commits, or cancels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStepResult {
    /// Continue the session; wait for the next user input.
    Continue,
    /// Commit the tool's result to the world. Runtime will call
    /// `commit()` then clear the session.
    Commit,
    /// Cancel without committing. Runtime will call `cancel()` then
    /// clear the session.
    Cancel,
}

/// Which axis the user is targeting during numeric input / typed entry.
/// Allows a tool to route "type `2.5 Enter`" while dragging an axis
/// handle to the correct axis without ambiguity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAxis { X, Y, Z, W }

/// Mouse interaction payload — what the runtime passes to
/// [`ModalTool::on_hover`] / [`ModalTool::on_click`] etc.
#[derive(Debug, Clone)]
pub struct ViewportHit {
    /// World-space ray from camera through the cursor.
    pub ray_origin: Vec3,
    pub ray_direction: Vec3,
    /// First entity the physics raycast hit, if any. Can be None when
    /// the user clicks empty space.
    pub hit_entity: Option<Entity>,
    /// World-space hit point on that entity, or the closest point on
    /// the fallback ground plane if no entity was hit.
    pub hit_point: Vec3,
    /// Surface normal at the hit point, if known.
    pub hit_normal: Option<Vec3>,
}

// ============================================================================
// Option controls (data-driven Options Bar)
// ============================================================================

/// A single control rendered in the Tool Options Bar. Data-driven so
/// the Slint layer stays simple: it iterates a `Vec<ToolOptionControl>`
/// and renders one of three widget types per entry.
#[derive(Debug, Clone)]
pub struct ToolOptionControl {
    /// Stable identifier — tools receive this in `on_option_changed`
    /// to know which control the user touched.
    pub id: String,
    /// Human-readable label for the Options Bar.
    pub label: String,
    /// Widget kind + current value.
    pub kind: ToolOptionKind,
    /// True if this control is visible only in the advanced `⋯` popover.
    pub advanced: bool,
}

#[derive(Debug, Clone)]
pub enum ToolOptionKind {
    /// Numeric spin/slider with min/max + optional unit suffix.
    Number { value: f32, min: f32, max: f32, step: f32, unit: String },
    /// Toggle on/off.
    Bool { value: bool },
    /// Dropdown choice from a fixed list.
    Choice { options: Vec<String>, selected: String },
    /// Read-only text display (status / hint).
    Label { text: String },
}

// ============================================================================
// Tool context (passed to every callback)
// ============================================================================

/// Common resources a tool needs. Passed into every lifecycle callback
/// so tools don't need to collect their own system params.
///
/// Because Bevy's `World` can't be accessed through a plain `&mut` in
/// system parameters, tools that need broad world mutation should use
/// the `commands` queue or return `ToolStepResult::Commit` and perform
/// heavy work in `commit(&mut World)` which gets exclusive access.
pub struct ToolContext<'w, 's, 'a> {
    pub commands: &'a mut Commands<'w, 's>,
    pub time: &'a Time,
}

// ============================================================================
// ModalTool trait
// ============================================================================

/// The core trait. A type implementing this represents ONE session of
/// a tool from activation to commit/cancel. The runtime creates a fresh
/// instance via the [`ModalToolRegistry`] each time the user activates
/// the tool; state does not persist across sessions.
pub trait ModalTool: Send + Sync + 'static {
    /// Stable identifier for the tool (e.g., `"gap_fill"`). Used by
    /// MCP / Rune for scripted activation and as a telemetry key.
    fn id(&self) -> &'static str;

    /// Human-readable name shown in the Tool Options Bar title.
    fn name(&self) -> &'static str;

    /// Short phrase describing the CURRENT step of the multi-step flow,
    /// e.g. `"pick first edge"` → `"pick second edge"` → `"adjust thickness"`.
    /// Must reflect the tool's internal state so the Options Bar stays
    /// informative as the user progresses.
    fn step_label(&self) -> String;

    /// Optional: the tool's icon source (used by the cursor badge +
    /// active-ribbon button). Empty string = no badge.
    fn icon_path(&self) -> &'static str { "" }

    /// Build the control list for the Options Bar. Called every frame
    /// that the tool is active; should be cheap. Changes to the
    /// returned list are reflected automatically in the UI.
    fn options(&self) -> Vec<ToolOptionControl>;

    /// Called every frame with the current viewport ray — used for
    /// hover preview. The tool may maintain preview entities (e.g.,
    /// a ghost-geometry mesh) and update them in response. Does not
    /// change the step; return Continue.
    fn on_hover(&mut self, _hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        ToolStepResult::Continue
    }

    /// Called when the user left-clicks in the viewport while the tool
    /// is active. Most multi-step tools advance their internal state
    /// here and return `Continue` or, on the final click, `Commit`.
    ///
    /// For a tool with [`wants_drag`](ModalTool::wants_drag) off this
    /// fires the instant the button goes down. With drag on it fires on
    /// *release*, and only when the cursor never left the
    /// [`DRAG_THRESHOLD_PX`] dead zone — a click is a drag of zero
    /// length.
    fn on_click(&mut self, hit: &ViewportHit, ctx: &mut ToolContext) -> ToolStepResult;

    /// Opt in to press-move-release routing. Return true for tools that
    /// size, sweep, rubber-band, sketch, or paint — anything whose
    /// result depends on how far the cursor travelled while held.
    ///
    /// Leaving this false (the default) keeps the original click timing
    /// exactly: `on_click` on button-down, no deferral. Every pick-driven
    /// tool wants that, so do not flip this on speculatively.
    fn wants_drag(&self) -> bool { false }

    /// The gesture just crossed [`DRAG_THRESHOLD_PX`]. `hit` is the
    /// anchor — the hit captured on the frame the button went DOWN, not
    /// the current cursor — so the tool's origin is the pixel the user
    /// aimed at rather than 4px of slop later.
    ///
    /// Fires exactly once per gesture, and only when
    /// [`wants_drag`](ModalTool::wants_drag) is true.
    fn on_drag_start(&mut self, _hit: &ViewportHit, _ctx: &mut ToolContext) -> ToolStepResult {
        ToolStepResult::Continue
    }

    /// Fires every frame the drag is live, including the frame
    /// `on_drag_start` fired. `start` is always the anchor and `current`
    /// is this frame's cursor, so a tool computing a delta never has to
    /// remember anything itself.
    fn on_drag(
        &mut self,
        _start: &ViewportHit,
        _current: &ViewportHit,
        _ctx: &mut ToolContext,
    ) -> ToolStepResult {
        ToolStepResult::Continue
    }

    /// The drag finished — either the button came up, or the cursor left
    /// the viewport mid-gesture (in which case `end` is the last
    /// in-viewport hit). Follows every `on_drag_start` that didn't
    /// already end the session, so this is the reliable place to fold a
    /// preview into a result or drop it. Most drag tools return `Commit`
    /// here.
    fn on_drag_end(
        &mut self,
        _start: &ViewportHit,
        _end: &ViewportHit,
        _ctx: &mut ToolContext,
    ) -> ToolStepResult {
        ToolStepResult::Continue
    }

    /// Retract the most recent pick, keeping the session alive — bound
    /// to Backspace by the runtime.
    ///
    /// Multi-step pick tools (Gap Fill, Edge Align, Resize Align, Part
    /// Swap) should override this to pop one entry off their internal
    /// pick list, despawn that pick's preview, and return `Continue`.
    /// Stepping back from the FIRST step is the tool's choice: return
    /// `Continue` to sit at zero picks, or `Cancel` to end the session.
    /// The default ignores the key, which is right for single-step and
    /// drag tools.
    fn on_step_back(&mut self, _ctx: &mut ToolContext) -> ToolStepResult {
        ToolStepResult::Continue
    }

    /// Called when the user types a numeric value (e.g. Floating
    /// Numeric Input). `axis` is which axis they last constrained,
    /// `value` is the parsed number, `relative` is true for `+5`-style
    /// deltas. Default: ignore.
    fn on_numeric_input(
        &mut self,
        _axis: ToolAxis,
        _value: f32,
        _relative: bool,
        _ctx: &mut ToolContext,
    ) -> ToolStepResult {
        ToolStepResult::Continue
    }

    /// Called when a tool option control's value changes from the UI.
    /// `control_id` matches a `ToolOptionControl::id` from `options()`;
    /// `value` is a stringified form of the new value (`"2.5"`, `"true"`,
    /// `"Outer Touch"`). Default: ignore.
    fn on_option_changed(
        &mut self,
        _control_id: &str,
        _value: &str,
        _ctx: &mut ToolContext,
    ) -> ToolStepResult {
        ToolStepResult::Continue
    }

    /// Commit the tool's result to the world. Given exclusive `&mut World`
    /// so the tool can perform any ECS mutation (spawn parts, write
    /// TOML, push undo entries). Called exactly once on Commit.
    fn commit(&mut self, world: &mut World);

    /// Cancel and clean up any preview state. Called on Esc / RMB /
    /// repeat activation. Must not mutate the world beyond preview
    /// despawn.
    fn cancel(&mut self, commands: &mut Commands);

    /// If true, successful commit returns the user to Select. If false,
    /// the tool stays active (useful for CAD features where the user
    /// typically places many in a row — Extrude with "Continue placing"
    /// checked).
    fn auto_exit_on_commit(&self) -> bool { true }

    /// If true, the tool's controls render in a right-side dockable
    /// `ToolPanel` (vertical, multi-row, dedicated Apply button) instead
    /// of the floating top `ToolOptionsBar`.
    ///
    /// Use the panel for "form-only" tools that don't pick anything in
    /// the viewport — Pattern (Linear/Radial/Grid), Mirror, etc. The
    /// bar's horizontal pill is awkward when the user has 6+ controls
    /// and isn't doing a multi-step pick flow.
    ///
    /// Use the bar (default) for pick-driven tools where the step-label
    /// hint matters more than the controls — Gap Fill, Resize Align,
    /// Edge Align, Part Swap.
    fn prefers_panel(&self) -> bool { false }

    /// Entities currently owned by the tool as preview geometry.
    /// Runtime may despawn these on cancel; tool is responsible for
    /// keeping the list up to date. Empty = no preview.
    fn preview_entities(&self) -> Vec<Entity> { Vec::new() }
}

// ============================================================================
// ActiveModalTool — the singleton holder
// ============================================================================

/// Holds the currently-active modal tool, if any. Systems that care
/// about "is some modal tool eating the cursor right now" check this
/// resource via [`is_modal_tool_active`].
#[derive(Resource, Default)]
pub struct ActiveModalTool(Option<Box<dyn ModalTool>>);

impl ActiveModalTool {
    pub fn is_active(&self) -> bool { self.0.is_some() }
    pub fn id(&self) -> Option<&'static str> { self.0.as_ref().map(|t| t.id()) }
    pub fn name(&self) -> Option<&'static str> { self.0.as_ref().map(|t| t.name()) }
    pub fn step_label(&self) -> Option<String> { self.0.as_ref().map(|t| t.step_label()) }
    pub fn icon_path(&self) -> Option<&'static str> { self.0.as_ref().map(|t| t.icon_path()) }
    pub fn options(&self) -> Vec<ToolOptionControl> {
        self.0.as_ref().map(|t| t.options()).unwrap_or_default()
    }
    pub fn prefers_panel(&self) -> bool {
        self.0.as_ref().map(|t| t.prefers_panel()).unwrap_or(false)
    }
    /// Whether the active tool opted into press-move-release routing.
    /// Read by the per-frame pump to pick between the click path and the
    /// drag state machine. False when no tool is active.
    pub fn wants_drag(&self) -> bool {
        self.0.as_ref().map(|t| t.wants_drag()).unwrap_or(false)
    }
    pub fn preview_entities(&self) -> Vec<Entity> {
        self.0.as_ref().map(|t| t.preview_entities()).unwrap_or_default()
    }

    /// Replace the active tool. If one is already active, it's
    /// cancelled first (clean lifecycle). Returns true if a new tool
    /// was set.
    pub fn activate(&mut self, tool: Box<dyn ModalTool>, commands: &mut Commands) -> bool {
        if let Some(mut prev) = self.0.take() {
            prev.cancel(commands);
        }
        self.0 = Some(tool);
        true
    }

    /// Cancel the active tool and clear. Called on Esc / RMB / button-
    /// click-again.
    pub fn cancel(&mut self, commands: &mut Commands) {
        if let Some(mut t) = self.0.take() {
            t.cancel(commands);
        }
    }

    /// Take the active tool out (used by the commit flow which needs
    /// `&mut World` — can't hold both `&mut ActiveModalTool` and
    /// `&mut World`).
    pub fn take(&mut self) -> Option<Box<dyn ModalTool>> { self.0.take() }

    /// Put a tool back after exclusive-world work is done. Typically
    /// only used by internal commit plumbing.
    pub fn set(&mut self, tool: Option<Box<dyn ModalTool>>) { self.0 = tool; }

    /// Mutable borrow on the inner tool. Returns `None` when no tool
    /// is active.
    ///
    /// **Named `tool_mut` rather than `as_mut`** because callers hold
    /// this through a `ResMut<ActiveModalTool>` — Rust's method
    /// resolution goes through `Deref`, so `active.as_mut()` would
    /// resolve to `DerefMut::as_mut(&mut active)` (returning
    /// `&mut ActiveModalTool`) instead of this inherent method. The
    /// explicit name avoids that shadowing ambiguity.
    pub fn tool_mut(&mut self) -> Option<&mut (dyn ModalTool + 'static)> {
        self.0.as_deref_mut()
    }
}

/// Helper used by non-modal interaction systems to early-exit while a
/// modal tool is eating the cursor.
pub fn is_modal_tool_active(active: &ActiveModalTool) -> bool {
    active.is_active()
}

// ============================================================================
// ModalToolDragState — press/move/release bookkeeping
// ============================================================================

/// The runtime's memory of an in-flight left-mouse gesture. Lives in its
/// own resource rather than inside [`ActiveModalTool`] so that the tool
/// box stays a pure "which tool" holder and the gesture machine can be
/// cleared independently (tool switch, cancel, commit, cursor exit).
///
/// Only ever populated for tools whose [`ModalTool::wants_drag`] is
/// true; for everyone else it stays at its default and the pump takes
/// the original click path untouched.
#[derive(Resource, Default)]
pub struct ModalToolDragState {
    /// Cursor position (window-local, LOGICAL pixels — same space as
    /// `Window::cursor_position()`) on the frame the button went down.
    /// `None` means no gesture is armed.
    pub press_pos: Option<Vec2>,
    /// The [`ViewportHit`] sampled at press time. Handed to every
    /// `on_drag` / `on_drag_end` as `start` so tools always measure from
    /// the true anchor.
    pub press_hit: Option<ViewportHit>,
    /// Last hit sampled while the cursor was still inside the viewport.
    /// If the cursor wanders onto UI mid-drag this is the point the
    /// gesture closes out at.
    pub last_hit: Option<ViewportHit>,
    /// True once travel exceeded [`DRAG_THRESHOLD_PX`] and
    /// `on_drag_start` has fired. Decides click-vs-drag on release.
    pub dragging: bool,
}

impl ModalToolDragState {
    /// True while a promoted drag is live (threshold crossed, not yet
    /// released).
    pub fn is_dragging(&self) -> bool { self.dragging }

    /// Forget any armed press. Called on activate, cancel, commit, and
    /// whenever the cursor leaves the tool's reach — a stale press must
    /// never resurrect a drag when the cursor comes back.
    pub fn clear(&mut self) {
        self.press_pos = None;
        self.press_hit = None;
        self.last_hit = None;
        self.dragging = false;
    }
}

// ============================================================================
// ToolOptionsBarState — reflected to Slint
// ============================================================================

/// Mirrors ActiveModalTool state into Slint-friendly fields. Updated
/// every frame by `sync_tool_options_bar_state`. Slint reads this
/// resource via its binding system to render the Tool Options Bar.
///
/// Kept separate from `ActiveModalTool` so the Slint layer doesn't
/// hold a reference across frames (which would constrain borrow).
#[derive(Resource, Default, Clone)]
pub struct ToolOptionsBarState {
    pub visible: bool,
    pub tool_id: String,
    pub tool_name: String,
    pub step_label: String,
    pub icon_path: String,
    pub controls: Vec<ToolOptionControl>,
    /// When true, the tool's UI renders in the right-side dockable
    /// `ToolPanel` instead of the floating top `ToolOptionsBar`.
    /// Mirrors `ModalTool::prefers_panel()`.
    pub use_panel: bool,
}

// ============================================================================
// Registry — maps tool IDs to factory closures
// ============================================================================

/// Factory for constructing a fresh ModalTool instance. The runtime
/// calls this when the user activates a tool, so each session gets a
/// clean slate.
pub type ToolFactory = Box<dyn Fn() -> Box<dyn ModalTool> + Send + Sync>;

/// Catalog of all registered modal tools. Populated at startup by
/// each tool's plugin via `register_tool`. MCP + Rune + keybindings
/// all resolve tool activation through this registry.
#[derive(Resource, Default)]
pub struct ModalToolRegistry {
    factories: std::collections::HashMap<&'static str, ToolFactory>,
}

impl ModalToolRegistry {
    pub fn register<F>(&mut self, id: &'static str, factory: F)
    where
        F: Fn() -> Box<dyn ModalTool> + Send + Sync + 'static,
    {
        self.factories.insert(id, Box::new(factory));
    }

    pub fn tool_ids(&self) -> Vec<&'static str> {
        let mut v: Vec<_> = self.factories.keys().copied().collect();
        v.sort();
        v
    }

    /// Create a new instance of the named tool. Returns None if the
    /// tool wasn't registered.
    pub fn spawn(&self, id: &str) -> Option<Box<dyn ModalTool>> {
        self.factories.get(id).map(|f| f())
    }
}

// ============================================================================
// Activation events
// ============================================================================

/// Sent by keybindings / ribbon clicks / MCP to activate a named tool.
/// The `activate_modal_tool_system` handles this: looks up the factory
/// in `ModalToolRegistry`, instantiates, installs in `ActiveModalTool`.
///
/// Sending this while a tool is already active cancels the previous
/// session first — clean handoff.
#[derive(Event, Message, Debug, Clone)]
pub struct ActivateModalToolEvent {
    pub tool_id: String,
}

/// Explicit cancel event (Esc handler, right-click, button-click-again).
/// Could be replaced by direct `ActiveModalTool::cancel()` calls but
/// going through an event gives telemetry / logging a natural hook.
#[derive(Event, Message, Debug, Clone, Default)]
pub struct CancelModalToolEvent;

/// Retract the active tool's last pick without ending the session.
/// Sent by the Backspace handler in `step_back_modal_tool_system`, and
/// available to UI chrome (a "← Back" affordance on the Options Bar)
/// and MCP so the step-back path isn't keyboard-only.
#[derive(Event, Message, Debug, Clone, Default)]
pub struct StepBackModalToolEvent;

/// Emitted BY the runtime (not sent TO it) when a tool successfully
/// commits. Subscribers: toast notification system, telemetry.
#[derive(Event, Message, Debug, Clone)]
pub struct ModalToolCommittedEvent {
    pub tool_id: String,
    pub tool_name: String,
}

/// Fired from the Slint Options Bar when the user edits an option
/// control. Handled by `apply_tool_option_change` which calls
/// `on_option_changed` on the active tool.
#[derive(Event, Message, Debug, Clone)]
pub struct ToolOptionChangedEvent {
    pub control_id: String,
    pub value: String,
}

// ============================================================================
// Plugin
// ============================================================================

pub struct ModalToolPlugin;

impl Plugin for ModalToolPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActiveModalTool>()
            .init_resource::<ModalToolRegistry>()
            .init_resource::<ModalToolDragState>()
            .init_resource::<ToolOptionsBarState>()
            .add_message::<ActivateModalToolEvent>()
            .add_message::<CancelModalToolEvent>()
            .add_message::<StepBackModalToolEvent>()
            .add_message::<ModalToolCommittedEvent>()
            .add_message::<ToolOptionChangedEvent>()
            .add_systems(Update, (
                activate_modal_tool_system,
                apply_tool_option_change,
                cancel_modal_tool_system,
                // Step-back runs after cancel so a frame carrying both
                // Esc and Backspace resolves to the coarser gesture:
                // the session is already gone and step-back no-ops.
                step_back_modal_tool_system,
                run_active_modal_tool,
                sync_tool_options_bar_state,
            ).chain());
    }
}

// ============================================================================
// Shared step-result handling
// ============================================================================

/// Apply a [`ToolStepResult`] returned by ANY tool callback.
///
/// Every dispatch site — the per-frame pump, the option-changed handler,
/// the step-back handler — funnels through here so "what Commit means"
/// is defined exactly once: drop any in-flight gesture, take the tool
/// out of [`ActiveModalTool`], queue the deferred `commit(&mut World)`,
/// emit [`ModalToolCommittedEvent`], and optionally bounce back to
/// Select.
///
/// The commit-deferral trick: Bevy can't give us `&mut World` in a
/// normal system param, so we `commands.queue(|world| ...)` a closure
/// that takes the extracted tool box by move and calls `commit`. The
/// closure runs during the next command-flush, which happens before the
/// next system observes state.
///
/// `via` is a short phrase for the log line so a commit's origin stays
/// traceable ("click", "drag end", "option change", "step back").
fn handle_step_result(
    result: ToolStepResult,
    via: &str,
    active: &mut ActiveModalTool,
    drag: &mut ModalToolDragState,
    commands: &mut Commands,
    studio_state: &mut Option<ResMut<StudioState>>,
    committed_events: &mut MessageWriter<ModalToolCommittedEvent>,
) {
    match result {
        ToolStepResult::Continue => {}
        ToolStepResult::Cancel => {
            drag.clear();
            active.cancel(commands);
        }
        ToolStepResult::Commit => {
            drag.clear();
            let Some(mut taken) = active.take() else { return };
            let tool_id = taken.id().to_string();
            let tool_name = taken.name().to_string();
            let auto_exit = taken.auto_exit_on_commit();

            commands.queue(move |world: &mut World| {
                taken.commit(world);
            });
            committed_events.write(ModalToolCommittedEvent {
                tool_id: tool_id.clone(),
                tool_name,
            });
            if auto_exit {
                if let Some(state) = studio_state.as_deref_mut() {
                    switch_back_to_select(state);
                }
            }
            info!("🔧 Modal tool committed ({}): {}", via, tool_id);
        }
    }
}

/// Route `ToolOptionChangedEvent` to the active tool's
/// `on_option_changed` callback. Handles the returned
/// [`ToolStepResult`] the same way the per-frame run pump does —
/// Commit → deferred `commit(&mut World)`; Cancel → `active.cancel()`.
fn apply_tool_option_change(
    mut events: MessageReader<ToolOptionChangedEvent>,
    mut active: ResMut<ActiveModalTool>,
    mut drag: ResMut<ModalToolDragState>,
    time: Res<Time>,
    mut commands: Commands,
    mut studio_state: Option<ResMut<StudioState>>,
    mut committed_events: MessageWriter<ModalToolCommittedEvent>,
) {
    for event in events.read() {
        if !active.is_active() { continue; }

        let result = {
            let mut ctx = ToolContext { commands: &mut commands, time: &time };
            let Some(tool) = active.tool_mut() else { continue };
            tool.on_option_changed(&event.control_id, &event.value, &mut ctx)
        };

        handle_step_result(
            result,
            "option change",
            &mut active,
            &mut drag,
            &mut commands,
            &mut studio_state,
            &mut committed_events,
        );
    }
}

// ============================================================================
// Systems
// ============================================================================

fn activate_modal_tool_system(
    mut events: MessageReader<ActivateModalToolEvent>,
    registry: Res<ModalToolRegistry>,
    mut active: ResMut<ActiveModalTool>,
    mut drag: ResMut<ModalToolDragState>,
    mut studio_state: Option<ResMut<StudioState>>,
    mut commands: Commands,
    mut notifications: Option<ResMut<crate::notifications::NotificationManager>>,
) {
    for event in events.read() {
        match registry.spawn(&event.tool_id) {
            Some(tool) => {
                let id = tool.id();
                let name = tool.name().to_string();
                let step = tool.step_label();
                active.activate(tool, &mut commands);
                // A gesture armed by the OUTGOING tool must not leak
                // into the incoming one — swapping tools with the button
                // still held would otherwise hand the new tool a press
                // anchor it never saw.
                drag.clear();
                // Non-modal tools key off StudioState.current_tool to
                // decide whether to run their interaction systems. Kick
                // the current tool to Select so Move/Rotate/Scale stop
                // competing for the cursor while a modal tool owns it.
                if let Some(ref mut state) = studio_state {
                    state.current_tool = Tool::Select;
                }
                // Immediate toast so Drafting tools never feel "dead" —
                // users know the tool is armed and what to click next.
                if let Some(ref mut n) = notifications {
                    n.info(format!("{name}: {step}  (Esc to cancel)"));
                }
                info!("🔧 Modal tool activated: {}", id);
            }
            None => {
                warn!("⚠ Unknown modal tool id: '{}' — not in ModalToolRegistry", event.tool_id);
                if let Some(ref mut n) = notifications {
                    n.warning(format!("Unknown tool '{}'", event.tool_id));
                }
            }
        }
    }
}

fn cancel_modal_tool_system(
    mut events: MessageReader<CancelModalToolEvent>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut active: ResMut<ActiveModalTool>,
    mut drag: ResMut<ModalToolDragState>,
    mut commands: Commands,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
) {
    // Cancel triggers: Escape OR an explicit `CancelModalToolEvent`
    // (fired by the × button on the Tool Options Bar, the viewport
    // close button, etc.). **Right-click is NOT a cancel trigger** —
    // users expect RMB to orbit the camera while a tool is active
    // (Blender / Maya / Unreal all behave this way). Treating RMB as
    // cancel was a prior mis-wire that users called out 2026-04-23:
    // "right click makes the tool disappear". The explicit-event
    // path still covers intentional cancels from UI chrome.
    //
    // Esc while a text field is focused must only blur the field —
    // without this guard, pressing Esc inside a ToolOptionsBar
    // NumericField also cancelled the whole tool session, discarding
    // the in-progress pick sequence (DRAFTING_UX.md Law 1: one owner
    // per keypress, innermost first).
    let text_focused = ui_focus
        .as_ref()
        .map(|f| f.text_input_focused)
        .unwrap_or(false);
    let esc = keys.just_pressed(KeyCode::Escape) && !text_focused;
    let explicit = events.read().next().is_some();
    // Prevent `mouse` from being unused while we keep the reference
    // around — some call sites still read the button state upstream
    // and passing `ButtonInput<MouseButton>` is nearly free.
    let _ = mouse;

    if (esc || explicit) && active.is_active() {
        let id = active.id().unwrap_or("?");
        drag.clear();
        active.cancel(&mut commands);
        info!("🔧 Modal tool cancelled: {}", id);
    }
}

/// Backspace (or an explicit [`StepBackModalToolEvent`]) retracts the
/// active tool's LAST pick instead of the whole session.
///
/// Sibling of `cancel_modal_tool_system` and gated on the SAME
/// `text_input_focused` check: Backspace inside a ToolOptionsBar
/// NumericField must delete a character, not rewind the pick sequence
/// behind it (DRAFTING_UX.md Law 1: one owner per keypress, innermost
/// first).
///
/// That same law decides the other contender for the key. Backspace over
/// the viewport is normally delete-selection; while a modal tool owns
/// the cursor it owns the key, so any global handler for Backspace must
/// yield via [`should_suppress_non_modal`] — the same way the
/// move/rotate/scale drag handlers yield the cursor.
///
/// The tool decides what "one step" means; the runtime only routes the
/// key and handles the returned [`ToolStepResult`]. Tools that don't
/// override [`ModalTool::on_step_back`] see Backspace as a no-op.
fn step_back_modal_tool_system(
    mut events: MessageReader<StepBackModalToolEvent>,
    keys: Res<ButtonInput<KeyCode>>,
    mut active: ResMut<ActiveModalTool>,
    mut drag: ResMut<ModalToolDragState>,
    time: Res<Time>,
    mut commands: Commands,
    mut studio_state: Option<ResMut<StudioState>>,
    mut committed_events: MessageWriter<ModalToolCommittedEvent>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
) {
    let text_focused = ui_focus
        .as_ref()
        .map(|f| f.text_input_focused)
        .unwrap_or(false);
    let backspace = keys.just_pressed(KeyCode::Backspace) && !text_focused;
    let explicit = events.read().next().is_some();

    if !(backspace || explicit) || !active.is_active() { return; }

    let result = {
        let mut ctx = ToolContext { commands: &mut commands, time: &time };
        let Some(tool) = active.tool_mut() else { return };
        tool.on_step_back(&mut ctx)
    };

    handle_step_result(
        result,
        "step back",
        &mut active,
        &mut drag,
        &mut commands,
        &mut studio_state,
        &mut committed_events,
    );
}

/// Reflect `ActiveModalTool` into `ToolOptionsBarState` so the Slint
/// UI has a stable resource to bind against. Computed every frame
/// while a tool is active; cleared when no tool is active.
///
/// This is where "Rust first" bites: the Slint layer reads this
/// resource, doesn't hold references to `ActiveModalTool` directly.
fn sync_tool_options_bar_state(
    active: Res<ActiveModalTool>,
    mut bar: ResMut<ToolOptionsBarState>,
) {
    if active.is_active() {
        bar.visible = true;
        bar.tool_id    = active.id().unwrap_or("").to_string();
        bar.tool_name  = active.name().unwrap_or("").to_string();
        bar.step_label = active.step_label().unwrap_or_default();
        bar.controls   = active.options();
        bar.icon_path  = active.icon_path().unwrap_or("").to_string();
        bar.use_panel  = active.prefers_panel();
    } else if bar.visible {
        bar.visible = false;
        bar.tool_id.clear();
        bar.tool_name.clear();
        bar.step_label.clear();
        bar.icon_path.clear();
        bar.controls.clear();
        bar.use_panel = false;
    }
}

// ============================================================================
// Suppress non-modal tools while a modal tool is active
// ============================================================================

/// Convenience: call at the top of any non-modal interaction system
/// (move / rotate / scale drag handlers) to early-return if a modal
/// tool owns the cursor. Keeps the "one input owner at a time"
/// invariant without each tool system needing to know about each modal
/// tool explicitly.
pub fn should_suppress_non_modal(active: Option<&Res<ActiveModalTool>>) -> bool {
    match active {
        Some(a) => a.is_active(),
        None => false,
    }
}

// ============================================================================
// Run-active-tool — route viewport input to the active tool
// ============================================================================

/// The per-frame pump. Builds a `ViewportHit` from cursor + camera +
/// physics, then runs the pointer state machine that decides which
/// [`ModalTool`] callback the frame belongs to — `on_hover`, `on_click`,
/// or one of the three drag callbacks. On `Commit`, `handle_step_result`
/// takes the tool out of `ActiveModalTool` and queues a deferred
/// `commit()` that runs with exclusive `&mut World`.
///
/// ## Two paths, one pump
///
/// The `wants_drag()` branch is a hard fork, not a refinement. Tools
/// that leave it false see the exact original timing — `on_click` on the
/// frame the button goes down, `on_hover` otherwise — because every
/// pick-driven tool in the engine is built around that. Tools that opt
/// in trade that immediacy for the press → threshold → release sequence
/// described in the module docs.
fn run_active_modal_tool(
    mut active: ResMut<ActiveModalTool>,
    mut drag: ResMut<ModalToolDragState>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    spatial_query: avian3d::prelude::SpatialQuery,
    time: Res<Time>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
    viewport_bounds: Option<Res<crate::ui::ViewportBounds>>,
    mut studio_state: Option<ResMut<StudioState>>,
    mut commands: Commands,
    mut committed_events: MessageWriter<ModalToolCommittedEvent>,
) {
    if !active.is_active() {
        drag.clear();
        return;
    }

    // Resolve this frame's cursor + hit, or `None` if the cursor is out
    // of the tool's reach. Gathered in one closure so the several ways
    // that can fail share a single "cursor is unavailable" outcome —
    // the drag machine below has to react to that, not just bail.
    //
    // Reasons for `None`: the cursor is over a Slint panel or inside a
    // text field, it's outside the viewport rect, it's off the window
    // entirely, or there's no main camera. Modal tools still want their
    // Options Bar to render in all of those cases, hence a resource-only
    // early exit rather than skipping the whole system.
    let frame = (|| -> Option<(Vec2, ViewportHit)> {
        if ui_focus.as_ref().map(|f| f.has_focus || f.text_input_focused).unwrap_or(false) {
            return None;
        }

        let window = windows.single().ok()?;
        let cursor_pos = window.cursor_position()?;
        if let Some(vb) = viewport_bounds.as_deref() {
            let scale = window.scale_factor() as f32;
            if !vb.contains_logical(cursor_pos, scale) { return None; }
        }

        // Pick the main 3D camera (order=0) — same convention as every
        // other interaction system.
        let (camera, cam_transform) = cameras.iter().find(|(c, _)| c.order == 0)?;
        let ray = camera.viewport_to_world(cam_transform, cursor_pos).ok()?;

        // Physics raycast to find the first hit entity + point. Using
        // Avian's SpatialQuery so it respects the collision geometry the
        // rest of the engine uses.
        let raycast = {
            // Avian 0.6's `prelude::Dir` alias is `pub(crate)` only —
            // visible internally but not to downstream crates — so we
            // reach through `bevy::math::Dir3` directly. Avian's
            // `spatial_query.ray_hits` accepts a `Dir3` argument in 3d
            // mode, so this is the same underlying type.
            use avian3d::prelude::SpatialQueryFilter;
            use bevy::math::Dir3;
            if let Ok(dir) = Dir3::new(*ray.direction) {
                let hits = spatial_query.ray_hits(
                    ray.origin, dir, 10_000.0, 1, true, &SpatialQueryFilter::default()
                );
                hits.first().map(|h| {
                    let world_point = ray.origin + *ray.direction * h.distance;
                    ViewportHit {
                        ray_origin: ray.origin,
                        ray_direction: *ray.direction,
                        hit_entity: Some(h.entity),
                        hit_point: world_point,
                        hit_normal: Some(h.normal),
                    }
                })
            } else { None }
        };
        let hit = raycast.unwrap_or(ViewportHit {
            ray_origin: ray.origin,
            ray_direction: *ray.direction,
            hit_entity: None,
            hit_point: ray.origin + *ray.direction * 100.0,  // far projection
            hit_normal: None,
        });

        Some((cursor_pos, hit))
    })();

    let Some((cursor_pos, hit)) = frame else {
        // Cursor left the tool's reach. A promoted drag still has to be
        // closed out — otherwise the tool keeps a half-finished gesture
        // and its preview entities forever — so deliver `on_drag_end` at
        // the last in-viewport hit. Then forget the press, so coming
        // back into the viewport with the button still down can't
        // resurrect the gesture from stale state.
        let pending = if drag.dragging {
            let start = drag.press_hit.clone();
            let end = drag.last_hit.clone().or_else(|| start.clone());
            start.zip(end)
        } else {
            None
        };
        drag.clear();

        if let Some((start, end)) = pending {
            let result = {
                let mut ctx = ToolContext { commands: &mut commands, time: &time };
                let Some(tool) = active.tool_mut() else { return };
                tool.on_drag_end(&start, &end, &mut ctx)
            };
            handle_step_result(
                result,
                "drag end (cursor left viewport)",
                &mut active,
                &mut drag,
                &mut commands,
                &mut studio_state,
                &mut committed_events,
            );
        }
        return;
    };

    // Which callback this frame belongs to. `via` only reaches a log
    // line on Commit, but keeping it accurate is what makes a committed
    // tool's origin traceable after the fact.
    let mut via = "click";
    let wants_drag = active.wants_drag();

    let result = {
        let mut ctx = ToolContext { commands: &mut commands, time: &time };
        let Some(tool) = active.tool_mut() else { return };

        if !wants_drag {
            // Original click path, unchanged. Do not add drag
            // bookkeeping here — every existing pick tool depends on
            // `on_click` landing on the press frame.
            if mouse.just_pressed(MouseButton::Left) {
                tool.on_click(&hit, &mut ctx)
            } else {
                tool.on_hover(&hit, &mut ctx)
            }
        } else {
            // Where the cursor last was while still in reach — the point
            // a drag closes out at if the cursor wanders onto UI before
            // the button comes up.
            drag.last_hit = Some(hit.clone());

            let down = mouse.just_pressed(MouseButton::Left);
            let up = mouse.just_released(MouseButton::Left);
            let held = mouse.pressed(MouseButton::Left);

            if down && up {
                // Press and release landed in the same frame — a fast
                // click, or a hitch that swallowed the gap. There is no
                // travel to measure, so it's unambiguously a click.
                drag.clear();
                tool.on_click(&hit, &mut ctx)
            } else if down {
                // Arm the gesture and capture the anchor. Click-vs-drag
                // is decided later: on the frame the cursor leaves the
                // dead zone, or on release if it never does. This is the
                // one place a drag tool differs in feel — `on_click`
                // deliberately does NOT fire here.
                drag.press_pos = Some(cursor_pos);
                drag.press_hit = Some(hit.clone());
                drag.dragging = false;
                ToolStepResult::Continue
            } else if up {
                let was_dragging = drag.dragging;
                let start = drag.press_hit.clone();
                drag.clear();
                match (was_dragging, start) {
                    (true, Some(start)) => {
                        via = "drag end";
                        tool.on_drag_end(&start, &hit, &mut ctx)
                    }
                    // Never left the dead zone: a click is a drag of
                    // zero length.
                    (false, Some(_)) => tool.on_click(&hit, &mut ctx),
                    // Release with nothing armed — the button went down
                    // over UI, or the press was discarded when the
                    // cursor left the viewport. Swallow it rather than
                    // inventing a click the user didn't start here.
                    (_, None) => ToolStepResult::Continue,
                }
            } else if held {
                match (drag.press_pos, drag.press_hit.clone()) {
                    (Some(press_pos), Some(start)) => {
                        if drag.dragging {
                            via = "drag";
                            tool.on_drag(&start, &hit, &mut ctx)
                        } else if (cursor_pos - press_pos).length_squared()
                            > DRAG_THRESHOLD_PX * DRAG_THRESHOLD_PX
                        {
                            // Threshold crossed — promote the press.
                            // `on_drag_start` gets the ANCHOR hit (where
                            // the user aimed), then the first `on_drag`
                            // lands in the same frame so the tool sees
                            // the live cursor with no dead first frame.
                            // Square-norm comparison avoids a sqrt in
                            // the hot path.
                            drag.dragging = true;
                            via = "drag start";
                            let started = tool.on_drag_start(&start, &mut ctx);
                            if started == ToolStepResult::Continue {
                                via = "drag";
                                tool.on_drag(&start, &hit, &mut ctx)
                            } else {
                                started
                            }
                        } else {
                            // Still inside the dead zone — keep feeding
                            // hover so previews track the cursor.
                            tool.on_hover(&hit, &mut ctx)
                        }
                    }
                    // Button held with nothing armed: the press happened
                    // over UI and the cursor was dragged in. Hover only.
                    _ => tool.on_hover(&hit, &mut ctx),
                }
            } else {
                // Button up and idle. A press still armed here means its
                // release slipped past us — window focus loss, alt-tab.
                // Close out a promoted drag so the start/end pairing
                // holds, then forget the press either way.
                let stale = if drag.dragging { drag.press_hit.clone() } else { None };
                if drag.press_pos.is_some() { drag.clear(); }
                match stale {
                    Some(start) => {
                        via = "drag end (press lost)";
                        tool.on_drag_end(&start, &hit, &mut ctx)
                    }
                    None => tool.on_hover(&hit, &mut ctx),
                }
            }
        }
    };

    handle_step_result(
        result,
        via,
        &mut active,
        &mut drag,
        &mut commands,
        &mut studio_state,
        &mut committed_events,
    );
}

// ============================================================================
// Auto-exit-to-Select helper
// ============================================================================

/// Called by concrete tools after `commit()` to return the user to the
/// Select tool, matching the TOOLSET_UX.md §7.1 auto-exit-on-commit
/// default. Opt-out by overriding `ModalTool::auto_exit_on_commit`.
pub fn switch_back_to_select(studio_state: &mut StudioState) {
    studio_state.current_tool = Tool::Select;
}

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
//!    click / keyboard / numeric input to the tool via [`ModalTool`]
//!    callbacks. The tool mutates its own internal state, maintains any
//!    preview entities.
//! 3. On [`ToolStepResult::Commit`] — the tool writes its result to the
//!    world via [`ModalTool::commit`], despawns preview entities, and
//!    the runtime clears [`ActiveModalTool`].
//! 4. On [`ToolStepResult::Cancel`] / Esc / right-click — `cancel()` is
//!    called, preview entities despawn, no world mutation.
//! 5. On successful commit, the runtime optionally auto-switches back
//!    to Select tool (opt-out per tool via [`ModalTool::auto_exit_on_commit`]).
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
    fn on_click(&mut self, hit: &ViewportHit, ctx: &mut ToolContext) -> ToolStepResult;

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
            .init_resource::<ToolOptionsBarState>()
            .add_message::<ActivateModalToolEvent>()
            .add_message::<CancelModalToolEvent>()
            .add_message::<ModalToolCommittedEvent>()
            .add_message::<ToolOptionChangedEvent>()
            .add_systems(Update, (
                activate_modal_tool_system,
                apply_tool_option_change,
                cancel_modal_tool_system,
                run_active_modal_tool,
                sync_tool_options_bar_state,
            ).chain());
    }
}

/// Route `ToolOptionChangedEvent` to the active tool's
/// `on_option_changed` callback. Handles the returned
/// [`ToolStepResult`] the same way the per-frame run pump does —
/// Commit → deferred `commit(&mut World)`; Cancel → `active.cancel()`.
fn apply_tool_option_change(
    mut events: MessageReader<ToolOptionChangedEvent>,
    mut active: ResMut<ActiveModalTool>,
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

        match result {
            ToolStepResult::Continue => {}
            ToolStepResult::Cancel => {
                active.cancel(&mut commands);
            }
            ToolStepResult::Commit => {
                let Some(mut taken) = active.take() else { continue };
                let tool_id = taken.id().to_string();
                let tool_name = taken.name().to_string();
                let auto_exit = taken.auto_exit_on_commit();
                commands.queue(move |world: &mut World| {
                    taken.commit(world);
                });
                committed_events.write(ModalToolCommittedEvent {
                    tool_id: tool_id.clone(),
                    tool_name: tool_name.clone(),
                });
                if auto_exit {
                    if let Some(ref mut state) = studio_state {
                        switch_back_to_select(state);
                    }
                }
                info!("🔧 Modal tool committed (via option change): {}", tool_id);
            }
        }
    }
}

// ============================================================================
// Systems
// ============================================================================

fn activate_modal_tool_system(
    mut events: MessageReader<ActivateModalToolEvent>,
    registry: Res<ModalToolRegistry>,
    mut active: ResMut<ActiveModalTool>,
    mut studio_state: Option<ResMut<StudioState>>,
    mut commands: Commands,
) {
    for event in events.read() {
        match registry.spawn(&event.tool_id) {
            Some(tool) => {
                let id = tool.id();
                active.activate(tool, &mut commands);
                // Non-modal tools key off StudioState.current_tool to
                // decide whether to run their interaction systems. Kick
                // the current tool to Select so Move/Rotate/Scale stop
                // competing for the cursor while a modal tool owns it.
                if let Some(ref mut state) = studio_state {
                    state.current_tool = Tool::Select;
                }
                info!("🔧 Modal tool activated: {}", id);
            }
            None => {
                warn!("⚠ Unknown modal tool id: '{}' — not in ModalToolRegistry", event.tool_id);
            }
        }
    }
}

fn cancel_modal_tool_system(
    mut events: MessageReader<CancelModalToolEvent>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut active: ResMut<ActiveModalTool>,
    mut commands: Commands,
) {
    // Cancel triggers: Escape OR an explicit `CancelModalToolEvent`
    // (fired by the × button on the Tool Options Bar, the viewport
    // close button, etc.). **Right-click is NOT a cancel trigger** —
    // users expect RMB to orbit the camera while a tool is active
    // (Blender / Maya / Unreal all behave this way). Treating RMB as
    // cancel was a prior mis-wire that users called out 2026-04-23:
    // "right click makes the tool disappear". The explicit-event
    // path still covers intentional cancels from UI chrome.
    let esc = keys.just_pressed(KeyCode::Escape);
    let explicit = events.read().next().is_some();
    // Prevent `mouse` from being unused while we keep the reference
    // around — some call sites still read the button state upstream
    // and passing `ButtonInput<MouseButton>` is nearly free.
    let _ = mouse;

    if (esc || explicit) && active.is_active() {
        let id = active.id().unwrap_or("?");
        active.cancel(&mut commands);
        info!("🔧 Modal tool cancelled: {}", id);
    }
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
    } else if bar.visible {
        bar.visible = false;
        bar.tool_id.clear();
        bar.tool_name.clear();
        bar.step_label.clear();
        bar.icon_path.clear();
        bar.controls.clear();
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
/// physics, and hands it to the active tool's `on_hover` / `on_click`
/// callbacks. On `Commit`, takes the tool out of `ActiveModalTool`
/// and queues a deferred `commit()` that runs with exclusive `&mut World`.
///
/// The commit-deferral trick: Bevy can't give us `&mut World` in a
/// normal system param, so we `commands.queue(|world| ...)` a closure
/// that takes the extracted tool box by move, calls `commit`, and
/// fires the committed event. The closure runs during the next
/// command-flush, which happens before the next system observes state.
fn run_active_modal_tool(
    mut active: ResMut<ActiveModalTool>,
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
    if !active.is_active() { return; }

    // Early-exit if the cursor is over UI or outside the viewport.
    // Modal tools still want their Options Bar + hover-preview to
    // render, but no click routing while the user is actually in a
    // text field.
    if ui_focus.as_ref().map(|f| f.has_focus || f.text_input_focused).unwrap_or(false) {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor_pos) = window.cursor_position() else { return };
    if let Some(vb) = viewport_bounds.as_deref() {
        let scale = window.scale_factor() as f32;
        if !vb.contains_logical(cursor_pos, scale) { return; }
    }

    // Pick the main 3D camera (order=0) — same convention as every
    // other interaction system.
    let Some((camera, cam_transform)) = cameras.iter().find(|(c, _)| c.order == 0) else { return };
    let Ok(ray) = camera.viewport_to_world(cam_transform, cursor_pos) else { return };

    // Physics raycast to find the first hit entity + point. Using
    // Avian's SpatialQuery so it respects the collision geometry the
    // rest of the engine uses.
    let hit = {
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
    let hit = hit.unwrap_or(ViewportHit {
        ray_origin: ray.origin,
        ray_direction: *ray.direction,
        hit_entity: None,
        hit_point: ray.origin + *ray.direction * 100.0,  // far projection
        hit_normal: None,
    });

    // Route the event to the tool.
    let result = {
        let mut ctx = ToolContext { commands: &mut commands, time: &time };
        let Some(tool) = active.tool_mut() else { return };
        if mouse.just_pressed(MouseButton::Left) {
            tool.on_click(&hit, &mut ctx)
        } else {
            tool.on_hover(&hit, &mut ctx)
        }
    };

    match result {
        ToolStepResult::Continue => {}
        ToolStepResult::Cancel => {
            active.cancel(&mut commands);
        }
        ToolStepResult::Commit => {
            // Take the tool out; schedule a deferred commit that gets
            // exclusive World access. Exits back to Select unless the
            // tool opted out.
            let Some(mut taken) = active.take() else { return };
            let tool_id = taken.id().to_string();
            let tool_name = taken.name().to_string();
            let auto_exit = taken.auto_exit_on_commit();

            commands.queue(move |world: &mut World| {
                taken.commit(world);
            });
            committed_events.write(ModalToolCommittedEvent {
                tool_id: tool_id.clone(),
                tool_name: tool_name.clone(),
            });
            if auto_exit {
                if let Some(ref mut state) = studio_state {
                    switch_back_to_select(state);
                }
            }
            info!("🔧 Modal tool committed: {}", tool_id);
        }
    }
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

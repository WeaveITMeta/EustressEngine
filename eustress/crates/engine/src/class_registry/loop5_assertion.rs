//! LOOP-5 breaker — startup-time validator that catches the
//! `LabelEditState` class of bug before it kills a dev session.
//!
//! ## The bug this prevents
//!
//! `drain_slint_actions` is a Bevy `System` whose `Res<R>` / `ResMut<R>`
//! parameters Bevy validates EVERY frame. If even one parameter resolves
//! to a missing resource, Bevy **silently skips the whole system that
//! frame** and emits a single warn-level log line that's easy to miss
//! in the noise of a busy startup. The drain is the only path from
//! Slint callbacks back into Bevy — when it's skipped, every UI button
//! in the engine dies silently for the rest of the session.
//!
//! That's what `LabelEditState` did to the project earlier this week.
//! A new resource type was added to `drain_slint_actions`'s signature
//! (via `ResMut<LabelEditState>`) but the matching `init_resource`
//! call was added to the legacy `StudioUiPlugin` (which isn't even
//! mounted at runtime), not to `SlintUiPlugin` (which is). Bevy's drain
//! parameter validation failed every frame for hours of debugging until
//! someone read the rare warn line carefully.
//!
//! ## The breaker
//!
//! Per `docs/process/AGENT_DISPATCH.md` "LOOP 5" + this task's protocol
//! strategy (c) — the lowest-magic solution:
//!
//! 1. A `DrainResourceChecklist` resource holds a `Vec<DrainExpectation>`.
//! 2. Any plugin that owns a drain-class system (one that takes
//!    `Res<R>` / `ResMut<R>` for resources it must `init_resource` /
//!    `insert_resource` itself) calls
//!    [`App::add_drain_resource::<R>(system_name, plugin_name)`] at
//!    plugin-build time for each such `R`.
//! 3. The [`validate_drain_resources`] startup system walks the
//!    checklist and:
//!    - In **debug builds** — panics with a full multi-line diagnostic
//!      naming the missing resource, the system that needs it, and the
//!      plugin that registered the expectation.
//!    - In **release builds** — logs a `warn!` line so production
//!      doesn't crash on a missing resource (the engine survives at
//!      reduced functionality; the user sees the same warn Bevy already
//!      emits but with the actionable plugin / system context attached).
//!
//! ## Why a per-plugin checklist instead of full Bevy introspection
//!
//! Bevy 0.18 has no stable public API for walking a `System`'s declared
//! parameters at runtime — that would need an unstable
//! `world.archetypes()` walk plus pattern-matching on TypeId. A
//! `#[derive(SystemValidate)]` proc-macro is the long-term answer but is
//! TOO BIG for Wave 2.3 (per the task spec). The opt-in checklist is
//! ~50 lines of code, captures the LabelEditState lesson exactly, and
//! every future plugin that adds a drain resource can be one-line
//! protected:
//!
//! ```ignore
//! impl Plugin for MyPlugin {
//!     fn build(&self, app: &mut App) {
//!         app.init_resource::<MyResource>()
//!             .add_drain_resource::<MyResource>(
//!                 "my_drain_system",
//!                 "MyPlugin",
//!             )
//!             .add_systems(Update, my_drain_system);
//!     }
//! }
//! ```
//!
//! Future agents can replace this with a `#[derive(SystemValidate)]`
//! macro when (or if) the magic becomes worth the build-time cost. The
//! protocol shape stays identical; the checklist resource becomes the
//! output of the macro.

use std::any::TypeId;

use bevy::prelude::*;

// ============================================================================
// Public API: the checklist resource + the App extension trait
// ============================================================================

/// One entry on the [`DrainResourceChecklist`]. Identifies a single
/// resource a Bevy system needs in order to run, plus the system name
/// and the plugin that declared the expectation. The trio is enough to
/// emit a one-line "you forgot to `init_resource::<MissingResource>()`
/// in `OffendingPlugin` before adding `affected_drain_system`" message.
#[derive(Clone, Copy)]
pub struct DrainExpectation {
    /// `TypeId` of the resource the system needs. Compared against
    /// `app.world().contains_resource_by_id(...)` at validation time.
    pub resource_type_id: TypeId,

    /// Human-readable type name (from `std::any::type_name::<R>()`) —
    /// used only for diagnostics; the TypeId is the actual lookup key.
    pub resource_type_name: &'static str,

    /// Name of the system that takes `Res<R>` / `ResMut<R>` (e.g.
    /// `"drain_slint_actions"`). Used in the failure message so the
    /// developer can grep straight to the system.
    pub system_name: &'static str,

    /// Name of the plugin that registered the expectation (e.g.
    /// `"SlintUiPlugin"`). The plugin's `build` function is where the
    /// missing `init_resource` call belongs.
    pub plugin_name: &'static str,
}

/// Bevy [`Resource`] holding the list of `DrainExpectation`s registered
/// across the active plugin graph.
///
/// One global checklist (not one per plugin) is the deliberate choice:
/// the validation runs once at Startup, walking the whole list and
/// failing if anything's missing. Per-plugin checklists would require
/// either a sub-resource per plugin (heavier) or a side-channel for the
/// validator to enumerate plugin names (also heavier). The flat list
/// keeps the diagnostic precise (each entry names its plugin) without
/// the bookkeeping.
///
/// Inserted by [`ClassRegistryPlugin`]'s build step — any plugin that
/// wants to use [`App::add_drain_resource`] must run AFTER
/// `ClassRegistryPlugin` is added. The extension trait creates the
/// resource on demand if it's absent (defensive — see
/// [`App::add_drain_resource`] impl), so out-of-order plugin chains
/// don't deadlock.
#[derive(Resource, Default)]
pub struct DrainResourceChecklist {
    expectations: Vec<DrainExpectation>,
}

impl DrainResourceChecklist {
    /// Register a new expectation. Idempotent: re-registering the same
    /// `(TypeId, system_name, plugin_name)` triple is a no-op so that
    /// plugins which are inadvertently added twice don't double-trigger
    /// the validator (still wrong — but the user sees the
    /// `duplicate plugin add` panic from Bevy itself first, not a
    /// confusing checklist message).
    pub fn add(&mut self, expectation: DrainExpectation) {
        if self.expectations.iter().any(|e| {
            e.resource_type_id == expectation.resource_type_id
                && e.system_name == expectation.system_name
                && e.plugin_name == expectation.plugin_name
        }) {
            return;
        }
        self.expectations.push(expectation);
    }

    /// Number of registered expectations — exposed for diagnostics +
    /// the unit test below.
    pub fn len(&self) -> usize {
        self.expectations.len()
    }

    /// True when no expectations have been registered yet. The state
    /// of the resource immediately after `init_resource` and before any
    /// plugin has called `add_drain_resource`.
    pub fn is_empty(&self) -> bool {
        self.expectations.is_empty()
    }

    /// Read-only walk for the startup validator.
    pub fn iter(&self) -> impl Iterator<Item = &DrainExpectation> + '_ {
        self.expectations.iter()
    }
}

/// Extension trait so plugins call:
///
/// ```ignore
/// app.add_drain_resource::<MyResource>("drain_my_system", "MyPlugin");
/// ```
///
/// rather than reaching into [`DrainResourceChecklist`] directly. The
/// trait creates the checklist resource on demand so the call works
/// even when `ClassRegistryPlugin` hasn't run yet — important because
/// plugin build order in Bevy is not strictly guaranteed across the
/// many `add_plugins(...)` chains in the engine bootstrap.
pub trait AddDrainResourceExt {
    /// Register that the named system needs `Res<R>` / `ResMut<R>` and
    /// the named plugin must therefore `init_resource::<R>()` /
    /// `insert_resource(...)` itself.
    ///
    /// `R: Resource` is the same bound Bevy itself uses for
    /// `init_resource` and `Res<R>` — if it compiles in a system
    /// signature, it compiles here.
    fn add_drain_resource<R: Resource>(
        &mut self,
        system_name: &'static str,
        plugin_name: &'static str,
    ) -> &mut Self;
}

impl AddDrainResourceExt for App {
    fn add_drain_resource<R: Resource>(
        &mut self,
        system_name: &'static str,
        plugin_name: &'static str,
    ) -> &mut Self {
        // Create the checklist resource if no earlier plugin did.
        // `init_resource` is idempotent — safe to call even if the
        // resource already exists.
        self.init_resource::<DrainResourceChecklist>();

        let expectation = DrainExpectation {
            resource_type_id: TypeId::of::<R>(),
            resource_type_name: std::any::type_name::<R>(),
            system_name,
            plugin_name,
        };

        let mut checklist = self
            .world_mut()
            .resource_mut::<DrainResourceChecklist>();
        checklist.add(expectation);

        self
    }
}

// ============================================================================
// The startup validator system
// ============================================================================

/// Startup-schedule system that walks [`DrainResourceChecklist`] and
/// reports any expectation whose resource is missing from the world.
///
/// - **Debug builds (`#[cfg(debug_assertions)]`):** panics with a
///   formatted multi-line diagnostic. The panic message names every
///   missing resource, the system that needs it, and the plugin that
///   forgot to register it. Stops the engine immediately — that's the
///   point: a missing drain resource produces silent UI death, and a
///   loud Startup panic is strictly better than chasing it for hours.
///
/// - **Release builds:** logs a `warn!` line per missing resource so
///   production stays alive (degraded) rather than crashes. The user
///   still sees Bevy's own per-frame warn about the drain skip; this
///   adds the plugin/system context that makes the warn actionable.
///
/// This system reads — never mutates — both the checklist and the
/// world's resource set, so it's safe to run on any thread Bevy schedules.
pub fn validate_drain_resources(world: &mut World) {
    // Snapshot the checklist so we can drop the borrow before any
    // `world.contains_resource_by_id` calls (which take an immutable
    // world borrow themselves; defensive against future Bevy versions
    // that might tighten the borrow checker here).
    let expectations: Vec<DrainExpectation> = world
        .get_resource::<DrainResourceChecklist>()
        .map(|c| c.expectations.clone())
        .unwrap_or_default();

    if expectations.is_empty() {
        // No plugin registered any expectations — Wave 2.3 ships only
        // the resource and the validator; Wave 3 onward opts in.
        // Quiet info so a fresh boot isn't loud about an unused breaker.
        info!(
            "loop5_assertion: drain-resource checklist is empty (no plugins \
             have opted in yet — Wave 3 spawners populate this)"
        );
        return;
    }

    let mut missing: Vec<&DrainExpectation> = Vec::new();
    for expectation in &expectations {
        // The TypeId path is the safe way to test for resource
        // presence by `Any`-token; we don't have the concrete type
        // back from a TypeId. Bevy stores resource TypeIds in its
        // `Components` registry — match against that.
        if !contains_resource_type_id(world, expectation.resource_type_id) {
            missing.push(expectation);
        }
    }

    if missing.is_empty() {
        info!(
            "loop5_assertion: all {} drain-resource expectations satisfied",
            expectations.len()
        );
        return;
    }

    let report = format_missing_report(&missing);

    #[cfg(debug_assertions)]
    {
        // Debug build — panic loudly. The panic message becomes the
        // entire stderr output so it dominates the boot log.
        panic!("{}", report);
    }

    #[cfg(not(debug_assertions))]
    {
        // Release build — warn and let the engine continue. The user
        // will still see Bevy's per-frame validation skip warnings;
        // this one warn at startup gives them the actionable context.
        warn!("{}", report);
    }
}

/// Build the formatted multi-line diagnostic emitted by both the
/// debug-build panic and the release-build warn. Pulled into its own
/// function so the unit tests can compare against the format directly
/// without provoking a panic.
fn format_missing_report(missing: &[&DrainExpectation]) -> String {
    let mut buf = String::new();
    buf.push_str(
        "LOOP-5 BREAKER FIRED: drain_slint_actions or another drain-class \
         system is missing a required resource.\n",
    );
    buf.push_str(
        "This is the exact bug `LabelEditState` caused earlier this week — \
         a Bevy `ResMut<R>` parameter resolves to nothing every frame, so \
         Bevy silently skips the whole drain system and every Slint UI \
         callback dies.\n\n",
    );
    buf.push_str("Missing resources:\n");
    for expectation in missing {
        buf.push_str(&format!(
            "  - {}  (needed by `{}` in `{}`)\n",
            expectation.resource_type_name,
            expectation.system_name,
            expectation.plugin_name,
        ));
    }
    buf.push_str(
        "\nFix: add the matching `app.init_resource::<R>()` or \
         `app.insert_resource(R::default())` call inside the named \
         plugin's `build` function. See docs/process/AGENT_DISPATCH.md \
         LOOP 5 for the full breaker rationale.",
    );
    buf
}

/// Test whether the world contains a resource by its [`TypeId`].
///
/// Bevy 0.18's public `World` API doesn't expose a "contains by TypeId"
/// helper directly, so we resolve the TypeId to a `ComponentId` via
/// `World::components` (resources are registered as components in
/// Bevy 0.18) and then call `contains_resource_by_id(ComponentId)`.
/// Returns `false` if no ComponentId has been registered for the
/// TypeId — which means no system has ever used the resource, which
/// also means the resource isn't initialised. Either way, the answer
/// is "not present" from the checklist's perspective.
fn contains_resource_type_id(world: &World, type_id: TypeId) -> bool {
    let Some(component_id) = world.components().get_resource_id(type_id) else {
        // No ComponentId for the TypeId — the resource was never
        // registered with the world. Treat as missing.
        return false;
    };
    world.contains_resource_by_id(component_id)
}

// ============================================================================
// Tests — exercise the checklist + validator without booting a full App
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Sentinel resource used by the unit tests below. Lives inside
    /// `tests::` so it never leaks into the wider crate's type graph.
    #[derive(Resource, Default)]
    struct SentinelResource(#[allow(dead_code)] u32);

    /// A second sentinel — the validator must distinguish missing from
    /// present resources even when both TypeIds are registered.
    #[derive(Resource, Default)]
    struct OtherSentinel(#[allow(dead_code)] u32);

    /// `DrainResourceChecklist::default()` starts empty — invariant the
    /// validator depends on (an empty checklist short-circuits to the
    /// "no plugins opted in yet" info-log path).
    #[test]
    fn checklist_starts_empty() {
        let checklist = DrainResourceChecklist::default();
        assert!(checklist.is_empty());
        assert_eq!(checklist.len(), 0);
    }

    /// `add` dedupes on the full `(type_id, system_name, plugin_name)`
    /// triple — re-registering the same expectation must not double the
    /// checklist length.
    #[test]
    fn checklist_add_is_idempotent_on_full_triple() {
        let mut checklist = DrainResourceChecklist::default();
        let expectation = DrainExpectation {
            resource_type_id: TypeId::of::<SentinelResource>(),
            resource_type_name: std::any::type_name::<SentinelResource>(),
            system_name: "test_drain",
            plugin_name: "TestPlugin",
        };
        checklist.add(expectation);
        checklist.add(expectation);
        checklist.add(expectation);
        assert_eq!(checklist.len(), 1, "duplicates must be deduped");
    }

    /// `add` keeps separate entries for the SAME resource registered by
    /// DIFFERENT systems or plugins — each represents a separate
    /// drain-skip risk and gets its own line in the failure report.
    #[test]
    fn checklist_add_keeps_distinct_entries_for_distinct_systems() {
        let mut checklist = DrainResourceChecklist::default();
        checklist.add(DrainExpectation {
            resource_type_id: TypeId::of::<SentinelResource>(),
            resource_type_name: std::any::type_name::<SentinelResource>(),
            system_name: "drain_a",
            plugin_name: "PluginA",
        });
        checklist.add(DrainExpectation {
            resource_type_id: TypeId::of::<SentinelResource>(),
            resource_type_name: std::any::type_name::<SentinelResource>(),
            system_name: "drain_b",
            plugin_name: "PluginA",
        });
        assert_eq!(
            checklist.len(),
            2,
            "different systems must each get their own checklist entry"
        );
    }

    /// `App::add_drain_resource` populates the checklist AND lazily
    /// creates the checklist resource if no earlier plugin did — the
    /// out-of-order-plugin defence.
    #[test]
    fn add_drain_resource_creates_checklist_on_demand() {
        let mut app = App::new();
        // No `init_resource::<DrainResourceChecklist>()` call beforehand.
        app.add_drain_resource::<SentinelResource>(
            "test_drain",
            "TestPlugin",
        );
        let checklist = app
            .world()
            .get_resource::<DrainResourceChecklist>()
            .expect(
                "add_drain_resource must lazily create the checklist resource",
            );
        assert_eq!(checklist.len(), 1);
    }

    /// `contains_resource_type_id` is the load-bearing helper the
    /// validator uses. A resource that's been `init_resource`'d resolves
    /// to `true`; one that's only had its TypeId mentioned (e.g. by a
    /// checklist add) resolves to `false`.
    #[test]
    fn contains_resource_type_id_distinguishes_init_from_uninit() {
        let mut app = App::new();
        app.init_resource::<SentinelResource>();

        assert!(
            contains_resource_type_id(
                app.world(),
                TypeId::of::<SentinelResource>(),
            ),
            "SentinelResource was init_resource'd — must be present"
        );

        assert!(
            !contains_resource_type_id(
                app.world(),
                TypeId::of::<OtherSentinel>(),
            ),
            "OtherSentinel was never registered — must be absent"
        );
    }

    /// The "happy path" — every expectation has a matching resource —
    /// must not panic in either debug or release mode.
    #[test]
    fn validate_passes_when_all_resources_present() {
        let mut app = App::new();
        app.init_resource::<SentinelResource>();
        app.init_resource::<OtherSentinel>();
        app.add_drain_resource::<SentinelResource>(
            "test_drain",
            "TestPlugin",
        );
        app.add_drain_resource::<OtherSentinel>(
            "test_drain",
            "TestPlugin",
        );

        // Running validate_drain_resources directly via World::run_system_once
        // would be the closest match to the Startup-schedule invocation, but
        // the exclusive `&mut World` signature requires a different path —
        // here we call it directly with the world borrow.
        validate_drain_resources(app.world_mut());
        // If we reach this line, no panic fired — the assertion is implicit.
    }

    /// Confirm the diagnostic formatter produces a multi-line string
    /// that names the missing resource, system, and plugin. This is
    /// what the user sees in the panic message — the test pins the
    /// shape so a refactor can't accidentally drop the actionable
    /// context.
    #[test]
    fn format_missing_report_names_resource_system_and_plugin() {
        let expectation = DrainExpectation {
            resource_type_id: TypeId::of::<SentinelResource>(),
            resource_type_name: "test_module::SentinelResource",
            system_name: "drain_slint_actions",
            plugin_name: "SlintUiPlugin",
        };
        let report = format_missing_report(&[&expectation]);
        assert!(
            report.contains("test_module::SentinelResource"),
            "report must name the missing resource type"
        );
        assert!(
            report.contains("drain_slint_actions"),
            "report must name the affected system"
        );
        assert!(
            report.contains("SlintUiPlugin"),
            "report must name the offending plugin"
        );
        assert!(
            report.contains("LabelEditState"),
            "report must reference the LabelEditState lesson the breaker exists for"
        );
    }

    /// Debug-build behaviour: validator panics when an expectation has
    /// no matching resource. Wrapped in `#[cfg(debug_assertions)]` so
    /// the test only runs in debug mode (release would not panic).
    /// `should_panic` doesn't require an exact substring match — but we
    /// can sanity-check the panic message via `expected = ...` to
    /// confirm the LOOP-5 banner makes it through.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "LOOP-5 BREAKER FIRED")]
    fn validate_panics_in_debug_when_resource_missing() {
        let mut app = App::new();
        // Register an expectation for a resource we DO NOT initialise.
        app.add_drain_resource::<SentinelResource>(
            "test_drain",
            "TestPlugin",
        );
        validate_drain_resources(app.world_mut());
    }
}

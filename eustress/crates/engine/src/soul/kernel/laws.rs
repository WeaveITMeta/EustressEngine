//! # UniverseLaws — per-universe grammar configuration
//!
//! A [`UniverseLaws`] is the set of rules a Rune program must satisfy in a given
//! Universe. It is the static, deterministic, versioned policy the per-tick gate
//! checks against. There is exactly one active `UniverseLaws` per Universe.
//!
//! ## Default is permissive (existing scripts keep passing)
//!
//! [`UniverseLaws::eustress_core_default`] grants the FULL
//! [`CapabilityCatalog::eustress_core`] vocabulary, with the standard entrypoint
//! contract and the path-traversal scope rule on. Every park / V-Cell / nuclear
//! script already in the repo passes it unchanged. Only *named* universes (e.g.
//! "Matrix") opt into restriction by withholding capability classes.
//!
//! ## Where the laws live (state-layer ruling)
//!
//! The binding state decision is: **WorldDb is the local entity source-of-truth;
//! the Forge RaftStateStore owns ONLY the cross-node replicated slice.** A
//! `UniverseLaws` config is per-universe policy, not per-entity state and not
//! (by default) cross-node replicated, so its home is the Universe config that
//! WorldDb already owns. [`LawSource`] enumerates the resolution order; the
//! actual load is a TODO seam (`UniverseLaws::load_for_active_universe`) because
//! wiring it requires reading the active-Universe handle, which the validator
//! call sites do not yet thread through. Until then, callers use
//! `eustress_core_default()`.

use std::collections::HashSet;

use super::capability::{CapabilityCatalog, CapabilityClass};

/// Where a [`UniverseLaws`] config is resolved from. Documents the binding
/// state-layer decision and the intended precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LawSource {
    /// The compiled-in Eustress Core default (full vocabulary). Always available
    /// as the fallback so a universe with no explicit policy still validates.
    CoreDefault,
    /// Per-universe policy stored alongside the Universe config in WorldDb
    /// (the local entity source-of-truth). This is the normal source for a
    /// named universe like "Matrix".
    WorldDbUniverseConfig,
    /// The cross-node replicated slice owned by the Forge RaftStateStore. Only
    /// used for laws that MUST be identical across every node hosting a shared
    /// cell (rare; reserved for multi-node deployments).
    ForgeReplicatedSlice,
}

/// The kind of entrypoint a gated program is required (and permitted) to define.
///
/// Different call contexts accept different entrypoint shapes:
/// - per-frame play scripts use lifecycle hooks (`on_init` / `on_update(dt)` /
///   `on_ready` / `on_exit`);
/// - command-bar / one-shot scripts use `main()`.
///
/// The grammar rejects a program that defines none of the legal shapes for its
/// context, and rejects an entrypoint whose arity is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrypointContract {
    /// Lifecycle hook names accepted in this context, each paired with its
    /// required parameter count.
    pub allowed: Vec<(String, usize)>,
    /// If true, at least one of `allowed` must be defined or the program is
    /// rejected (`MissingEntrypoint`). If false (e.g. library-style includes),
    /// absence is permitted.
    pub require_at_least_one: bool,
}

impl EntrypointContract {
    /// The per-frame play context: lifecycle hooks.
    /// `on_init()`, `on_update(dt)`, `on_ready()`, `on_exit()`.
    pub fn play_lifecycle() -> Self {
        Self {
            allowed: vec![
                ("on_init".into(), 0),
                ("on_update".into(), 1),
                ("on_ready".into(), 0),
                ("on_exit".into(), 0),
            ],
            require_at_least_one: true,
        }
    }

    /// The command-bar / one-shot context: `main()` (preferred) or `on_init()`
    /// (the one-shot path tries both today — see
    /// `rune_runtime::execute_oneshot`).
    pub fn oneshot() -> Self {
        Self {
            allowed: vec![("main".into(), 0), ("on_init".into(), 0)],
            require_at_least_one: true,
        }
    }

    /// Union of play + one-shot, for contexts (like the RSI generate loop) where
    /// either shape is acceptable. This is the most permissive contract.
    pub fn any_recognized() -> Self {
        Self {
            allowed: vec![
                ("on_init".into(), 0),
                ("on_update".into(), 1),
                ("on_ready".into(), 0),
                ("on_exit".into(), 0),
                ("main".into(), 0),
            ],
            require_at_least_one: true,
        }
    }
}

/// A single scope rule — a named, context-bound constraint beyond raw capability
/// grant/withhold. These encode the "ReplicatedFirst must not touch Workspace"
/// style precedent from `common/src/soul/ast.rs`, generalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeRule {
    /// Reject any `FilesystemRead`/`FilesystemWrite` whose path argument escapes
    /// the space root (contains `..` or is absolute). The runtime ALSO guards
    /// this, but the gate rejects it pre-execution so an unvalidated write
    /// window never opens.
    NoPathTraversal,
    /// Reject mutation of world-level laws (`WorldLaw` class) — the universe's
    /// physics are immutable from scripts. (Distinct from merely withholding the
    /// class: this can carry a tailored message and a TODO for value-range
    /// checks.)
    ImmutablePhysics,
    /// Reserved: a forbidden raw-text pattern (e.g. a banned identifier). Carried
    /// as a literal substring for now; a future version compiles these to a
    /// matcher. TODO seam — not enforced by the first deliverable.
    ForbiddenPattern(String),
}

/// The active law set for one Universe.
#[derive(Debug, Clone)]
pub struct UniverseLaws {
    /// Human-readable universe name this policy applies to (e.g. "Matrix",
    /// "Eustress Core").
    pub universe: String,
    /// Monotonic version of this policy. Bumped on any edit. The verdict's
    /// determinism is tied to this — same `(source, registry_version)` always
    /// yields the same verdict for the same program.
    pub registry_version: u64,
    /// Where this policy was resolved from.
    pub source: LawSource,
    /// The full vocabulary the program is checked against. A call not in the
    /// catalog is an `UnknownCapability` regardless of the granted set.
    pub catalog: CapabilityCatalog,
    /// Capability classes this universe GRANTS. A call whose class is not in this
    /// set (and is not `is_always_allowed`) is a `WithheldCapability`.
    pub granted_classes: HashSet<CapabilityClass>,
    /// The entrypoint contract for the default (play) context.
    pub entrypoints: EntrypointContract,
    /// Additional scope rules.
    pub scope_rules: Vec<ScopeRule>,
}

impl UniverseLaws {
    /// The permissive default: full vocabulary, every class granted, standard
    /// play lifecycle entrypoints, path-traversal guard on. Existing scripts
    /// pass this unchanged.
    pub fn eustress_core_default() -> Self {
        let catalog = CapabilityCatalog::eustress_core();
        let granted_classes = catalog.classes();
        Self {
            universe: "Eustress Core".into(),
            registry_version: 1,
            source: LawSource::CoreDefault,
            catalog,
            granted_classes,
            entrypoints: EntrypointContract::play_lifecycle(),
            scope_rules: vec![ScopeRule::NoPathTraversal],
        }
    }

    /// A restricted universe profile — starts from the core default, then
    /// withholds the named classes and applies any extra scope rules. This is
    /// how "Matrix" declares e.g. no `Network`, no `Persistence`, immutable
    /// physics.
    pub fn restricted(
        universe: impl Into<String>,
        withheld: &[CapabilityClass],
        extra_scope: Vec<ScopeRule>,
    ) -> Self {
        let mut laws = Self::eustress_core_default();
        laws.universe = universe.into();
        laws.registry_version = 1;
        laws.source = LawSource::WorldDbUniverseConfig;
        for class in withheld {
            laws.granted_classes.remove(class);
        }
        laws.scope_rules.extend(extra_scope);
        laws
    }

    /// Whether the given capability class is granted (or is always-allowed).
    pub fn grants(&self, class: CapabilityClass) -> bool {
        class.is_always_allowed() || self.granted_classes.contains(&class)
    }

    /// Whether the path-traversal scope rule is active.
    pub fn enforces_no_path_traversal(&self) -> bool {
        self.scope_rules.iter().any(|r| matches!(r, ScopeRule::NoPathTraversal))
    }

    /// Whether physics is immutable in this universe (world-law mutation
    /// forbidden by scope rule, regardless of class grant).
    pub fn immutable_physics(&self) -> bool {
        self.scope_rules.iter().any(|r| matches!(r, ScopeRule::ImmutablePhysics))
    }

    // ====================================================================
    // TODO seams
    // ====================================================================

    /// TODO(kernel-laws-source): resolve the active Universe's `UniverseLaws`
    /// from WorldDb (the entity source-of-truth) per [`LawSource`] precedence:
    /// `ForgeReplicatedSlice` (if any cross-node slice applies) → then
    /// `WorldDbUniverseConfig` → then `CoreDefault`. The validator call sites
    /// (`validate_rune_script`, `compile_scripts_on_play`, `execute_rune_oneshot`)
    /// do not currently thread the active-Universe handle, so this is stubbed.
    /// Until wired, every caller uses [`Self::eustress_core_default`].
    pub fn load_for_active_universe(/* universe_handle, worlddb */) -> Self {
        // TODO: read the universe config from WorldDb; fall back to core default.
        Self::eustress_core_default()
    }
}

impl Default for UniverseLaws {
    fn default() -> Self {
        Self::eustress_core_default()
    }
}

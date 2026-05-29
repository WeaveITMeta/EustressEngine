//! # Engine-side `class_registry` — Wave 2.3 plumbing
//!
//! Plugin + LOOP-5 startup assertion for the `ClassSpawner` trait
//! registry. The trait, `ClassRegistry` resource type, `PropertyBag`,
//! `SpawnCtx`, and LOD types all live in
//! [`eustress_common::class_registry`] — they're re-exported here for
//! convenience so engine-side code can write:
//!
//! ```ignore
//! use crate::class_registry::{ClassRegistry, ClassRegistryPlugin};
//! ```
//!
//! instead of pulling each symbol from common explicitly.
//!
//! ## Module layout
//!
//! - [`plugin`] — [`ClassRegistryPlugin`] (registers the resource +
//!   mounts the LOOP-5 validator).
//! - [`loop5_assertion`] — the
//!   [`DrainResourceChecklist`] resource + [`AddDrainResourceExt`]
//!   trait + the [`validate_drain_resources`] startup system.
//! - [`spawn_ctx_engine`] — [`EngineSpawnExtras`], the typed downcast
//!   target for common's `SpawnCtx::extra` slot (engine-specific
//!   `ResMut<...>` borrows packed for spawner access).
//!
//! ## Plugin mount
//!
//! [`ClassRegistryPlugin`] is added inside `SlintUiPlugin::build` (see
//! `ui/slint_ui.rs` for the single-line `.add_plugins(...)` call). Per
//! `docs/process/AGENT_DISPATCH.md` LOOP 5: never add this to
//! `StudioUiPlugin` (the legacy plugin that isn't mounted at runtime)
//! — resources registered there are invisible to the live engine's
//! drain system, which is exactly the silent-failure mode the LOOP-5
//! breaker exists to catch.
//!
//! ## Wave 3 hook
//!
//! Wave 3 ships actual `ClassSpawner` impls under
//! `crates/engine/src/spawners/<group>/<class>.rs` (see
//! `CLASS_REGISTRY.md` §8). Each spawner registers itself inside
//! [`ClassRegistryPlugin::build`] behind the `class-registry` cargo
//! feature — see the commented hook in `plugin.rs` for the planned
//! shape.

pub mod loop5_assertion;
pub mod plugin;
pub mod spawn_ctx_engine;

// ── Engine-side re-exports of the common-side scaffold ───────────────
//
// Keeps engine-side imports terse and lets future refactors swap the
// common-side path without rippling through engine call sites.

pub use eustress_common::class_registry::{
    ClassRegistry, ClassSpawner, ComponentBundle, DynamicComponent,
    LodTier, PropertyBag, RegisterClassExt, RobloxInstance,
    RobloxPropertyValue, SpawnCtx,
};

// ── Engine-only public surface ────────────────────────────────────────

pub use loop5_assertion::{
    validate_drain_resources, AddDrainResourceExt, DrainExpectation,
    DrainResourceChecklist,
};
pub use plugin::{log_registry_validation, ClassRegistryPlugin};
pub use spawn_ctx_engine::EngineSpawnExtras;

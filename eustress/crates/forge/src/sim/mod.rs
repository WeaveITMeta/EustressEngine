//! # Phase-2 agents-in-sims orchestration (binding decision D2)
//!
//! This module is the ONE auditable boundary between three layers:
//!
//! * **`eustress_worlddb`** — the local entity source-of-truth. Entity
//!   component bytes NEVER leave it.
//! * **`forge_orchestration::scheduler::{sim,gang,deadline,reconcile}`** — the
//!   upstream gang/co-placement scheduler that decides *where* a cell's
//!   world + agents run (all-or-nothing).
//! * **`forge_orchestration::storage::RaftStateStore`** — the cross-node
//!   replicated slice. It owns ONLY the cell-shared slice + the SimCell
//!   submission channel; it is NOT a second copy of the world.
//!
//! ## The seam is store-mediated, not call-mediated
//!
//! The crucial shape (verified against the 0.6.0 source) is that
//! [`Reconciler`] does **not** expose a `submit_sim_cell` call. Each
//! `reconcile_once().await` *auto-discovers* desired cells by scanning
//! `list_prefix(keys::SIMCELLS)` and `store_get_json::<SimCell>` on every
//! hit. So the eustress side:
//!
//! 1. builds a [`SimCell`] from an engine residency cell
//!    ([`cell::to_sim_cell`]),
//! 2. writes it under [`forge_orchestration::storage::keys::simcell`] via
//!    [`cell_sync::submit_sim_cell`],
//! 3. registers a [`NodeResources`] on the [`Reconciler`],
//! 4. calls [`driver::reconcile_once`],
//! 5. reads [`Reconciler::sim_bindings`] and asserts the matching
//!    [`SimBinding`] committed *all* members (gang all-or-nothing).
//!
//! The same [`RaftStateStore`] is therefore both the WorldDb↔Forge
//! replicated-slice substrate AND the SimCell submission channel — exactly
//! the separate-stores-with-explicit-sync decision from the state-layer
//! design. The disjoint key prefixes (`forge/simcells`, `forge/cell/…`) keep
//! it out of the reserved `forge/jobs|nodes|config` namespaces.
//!
//! ## Status (scaffold)
//!
//! `cell.rs` is pure, unit-tested math + builders (no async, no I/O).
//! `cell_sync.rs` + `driver.rs` carry the real upstream signatures and the
//! one-directional WorldDb↔Raft adapter, with clearly-marked `TODO` seams
//! where the engine-side driver (Phase 0b Kernel-validated agent loop) and
//! the real per-cell apply latch land. Everything here compiles and is
//! testable WITHOUT Bevy.

pub mod cell;
pub mod cell_sync;
pub mod driver;

#[cfg(test)]
use eustress_worlddb::MortonKeyEncoder;

/// A 21-bit Morton cell triple at chunk size 256 — the SAME unit as the
/// engine's `residency::Cell`. Reused, never redefined, so the
/// residency-cell ↔ Forge-SimCell mapping shares one coordinate system.
pub type CellCoord = (u32, u32, u32);

/// Cell edge in world metres. MUST equal
/// [`MortonKeyEncoder::default()`]`.chunk_size`. A guard test in
/// [`cell`] asserts this at build time so a worlddb chunk-size change
/// breaks here loudly instead of silently mis-mapping cells.
pub const CELL_EDGE_M: f32 = 256.0;

/// Runtime check that [`CELL_EDGE_M`] still equals the worlddb encoder's
/// chunk size. Used by the guard test in [`cell`]; `cfg(test)` so the
/// release lib doesn't carry an unused helper.
#[cfg(test)]
#[inline]
pub(crate) fn worlddb_chunk_size() -> f32 {
    MortonKeyEncoder::default().chunk_size
}

// ── Re-export the upstream scheduler/storage types the consumers need ──────
//
// Surfacing them through `crate::sim` keeps every `forge_orchestration::`
// path inside this module (the auditable boundary).

pub use forge_orchestration::scheduler::sim::{
    AgentPolicy, CoPlacement, GangGroup, MemberRole, Region3D, SimCell, SimMember, SimWorld,
};
pub use forge_orchestration::scheduler::gang::{GangDecision, GangReservation, GangScheduler};
pub use forge_orchestration::scheduler::reconcile::{
    Assignment, MetricsSource, ReconcileReport, Reconciler, SimBinding, TaskStatus,
};
pub use forge_orchestration::scheduler::{NodeResources, ResourceRequirements};
pub use forge_orchestration::storage::{
    keys, memory_store, store_get_json, store_set_json, BoxedStateStore, MemoryStore, StateStore,
};
pub use forge_orchestration::types::{GpuResources, NodeId};

#[cfg(feature = "raft")]
pub use forge_orchestration::storage::RaftStateStore;

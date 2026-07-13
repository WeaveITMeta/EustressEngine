//! Engine-side `SpawnCtx` extension — the concrete downcast target for
//! the opaque `extra: Option<&mut dyn Any>` slot on common's
//! [`eustress_common::class_registry::SpawnCtx`].
//!
//! ## Why this lives engine-side
//!
//! `eustress-common` deliberately does not depend on `eustress-engine`
//! (see `class_registry/spawn_ctx.rs` module docs in common — Wave 1
//! design constraint). The common-side ctx carries Bevy core asset
//! stores (`Assets<Mesh>`, `Assets<StandardMaterial>`, `Assets<Image>`)
//! plus engine-agnostic Eustress resources (`ClassSchemaResource`,
//! `MeasureUnit`). Engine-specific resources — `MaterialRegistry`,
//! `PrimitiveMeshCache`, `Assets<ForwardDecalMaterial<...>>`, and any
//! future runtime caches — cannot be named there.
//!
//! Common solves generic-vs-engine-specific extras with a type-erased
//! `Option<&mut dyn Any>` field — but that slot requires `T: 'static`
//! to downcast (`Any`'s bound), and [`EngineSpawnExtras`] borrows
//! `ResMut`-sourced engine resources for the duration of one spawn call,
//! so its `'w` is never `'static`. A type with a live non-`'static`
//! lifetime parameter can never implement `Any`, so it cannot be packed
//! into `SpawnCtx::extra` — that would only type-check for genuinely
//! `'static` data, which spawn-time resource borrows aren't. Wave 3
//! spawn-site helpers instead pass `&mut EngineSpawnExtras<'_>` as a
//! direct argument to engine-registered spawners, alongside (not inside)
//! `SpawnCtx`. `SpawnCtx::extra` remains available for genuinely
//! `'static` payloads.
//!
//! ## Wave 2.3 scope
//!
//! - Defines [`EngineSpawnExtras`] — the bundle of engine-specific
//!   `ResMut`-sourced borrows a spawn site holds.
//! - Provides a constructor that takes the engine's `ResMut<...>`
//!   handles directly (Wave 3 spawn-site helpers build it).
//! - Does NOT construct or use `SpawnCtx` anywhere yet. The legacy spawn
//!   paths (`instance_loader::spawn_instance`, `gui_loader::spawn_gui_element`,
//!   `spawn.rs::spawn_*`) keep their existing argument lists; Wave 3
//!   plugs this in as a direct parameter per engine-registered spawner.
//!
//! Per spec §3 + LOOP-5 lesson: this file owns the extras shape but
//! never touches the drain or any existing spawn arm.

use bevy::pbr::decal::ForwardDecalMaterial;
use bevy::prelude::*;

use crate::space::instance_loader::PrimitiveMeshCache;
use crate::space::material_loader::MaterialRegistry;

/// Engine-specific resources a Wave-3 engine-registered spawner needs
/// alongside [`eustress_common::class_registry::SpawnCtx`].
///
/// Passed as a direct `&mut EngineSpawnExtras<'_>` argument, NOT through
/// `SpawnCtx::extra` — see the module doc for why the `dyn Any` slot
/// can't carry it (that needs `'static`; this borrows for one spawn
/// call). The borrows themselves live on the caller's stack frame — same
/// pattern as common's `SpawnCtx` lifetimes: `'w` is the world borrow,
/// no boxing or per-spawn allocation.
///
/// ## Lifetime
///
/// `'w` matches the world borrow that the calling system holds. The
/// `SpawnCtx` constructed around an `EngineSpawnExtras` shares the same
/// `'w`, so the borrow checker rejects any spawner that tries to hand
/// these references back into long-lived storage.
///
/// ## Why fields are `&'w mut` not owned
///
/// Same reason common's `SpawnCtx` exposes raw `&'w mut` references
/// rather than `ResMut` wrappers (see `class_registry/spawn_ctx.rs` in
/// common): borrow disjointness is checked at the construction site, so
/// the borrow checker rejects overlapping aliases before the spawner
/// runs rather than deep inside an `Option<ResMut<...>>` unwrap.
pub struct EngineSpawnExtras<'w> {
    /// Central material handle cache — populated on Space load from
    /// `MaterialService/*.mat.toml`. Spawners that resolve material
    /// names (e.g. `PartSpawner` reading `properties.material =
    /// "metal"`) reach through here.
    pub material_registry: &'w mut MaterialRegistry,

    /// Primitive mesh handle cache — avoids per-entity
    /// `asset_server.load()` calls for the same GLB path across
    /// thousands of entities. `PartSpawner` + `SpecialMeshSpawner` use
    /// it to share `parts/block.glb` etc.
    pub mesh_cache: &'w mut PrimitiveMeshCache,

    /// Forward-decal material asset store. `DecalSpawner` registers a
    /// fresh `ForwardDecalMaterial<StandardMaterial>` per spawned decal
    /// instance and inserts the handle on the entity.
    pub decal_materials: &'w mut Assets<ForwardDecalMaterial<StandardMaterial>>,
}

impl<'w> EngineSpawnExtras<'w> {
    /// Bundle the engine-side `ResMut<...>` handles a spawn site holds
    /// into an `EngineSpawnExtras`.
    ///
    /// Caller is expected to hold each `ResMut` for the duration of the
    /// spawn — the lifetimes ensure that's the case. Spawners that
    /// don't need engine state can ignore the extras slot entirely.
    pub fn new(
        material_registry: &'w mut MaterialRegistry,
        mesh_cache: &'w mut PrimitiveMeshCache,
        decal_materials: &'w mut Assets<ForwardDecalMaterial<StandardMaterial>>,
    ) -> Self {
        Self {
            material_registry,
            mesh_cache,
            decal_materials,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Confirms `EngineSpawnExtras` constructs from three independently
    /// `ResMut`-sourced borrows — the shape a Wave 3 spawn-site helper
    /// builds every spawn call. Successful compilation is itself part of
    /// the assertion: the borrow checker proves the three fields are
    /// disjoint (no aliasing) at the construction site.
    ///
    /// This does NOT (and per the module doc, cannot) round-trip through
    /// `dyn Any` — `EngineSpawnExtras<'w>` is never `'static`, so it can
    /// never implement `Any`. An earlier version of this test asserted
    /// that erasure, which cannot compile for real spawn-time borrows;
    /// see the module doc's "not routed through `SpawnCtx::extra`" note.
    #[test]
    fn engine_spawn_extras_constructs_from_disjoint_borrows() {
        let mut material_registry = MaterialRegistry::default();
        let mut mesh_cache = PrimitiveMeshCache::default();
        let mut decal_materials =
            Assets::<ForwardDecalMaterial<StandardMaterial>>::default();

        let extras = EngineSpawnExtras::new(
            &mut material_registry,
            &mut mesh_cache,
            &mut decal_materials,
        );
        drop(extras);
    }
}

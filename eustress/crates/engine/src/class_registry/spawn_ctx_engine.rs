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
//! Common solves this with a type-erased `Option<&mut dyn Any>` field.
//! When the engine-side helper builds a `SpawnCtx`, it packs the engine
//! borrows into an [`EngineSpawnExtras`] and stuffs a `&mut dyn Any`
//! pointing at it into that slot. Spawners that need engine state call
//! `ctx.extra_as::<EngineSpawnExtras>()` to recover the typed borrows.
//!
//! ## Wave 2.3 scope
//!
//! - Defines [`EngineSpawnExtras`] — the struct that lives behind
//!   `dyn Any` for downcasting.
//! - Provides a constructor that takes the engine's `ResMut<...>`
//!   handles directly (Wave 3 spawn-site helpers build it).
//! - Does NOT construct or use the ctx anywhere yet. The legacy spawn
//!   paths (`instance_loader::spawn_instance`, `gui_loader::spawn_gui_element`,
//!   `spawn.rs::spawn_*`) keep their existing argument lists; Wave 3
//!   plugs in the bridge per class.
//!
//! Per spec §3 + LOOP-5 lesson: this file owns the extras shape but
//! never touches the drain or any existing spawn arm.

use bevy::pbr::decal::ForwardDecalMaterial;
use bevy::prelude::*;

use crate::space::instance_loader::PrimitiveMeshCache;
use crate::space::material_loader::MaterialRegistry;

/// Engine-specific resources packed behind the [`dyn Any`][std::any::Any]
/// `extra` slot on [`eustress_common::class_registry::SpawnCtx`].
///
/// A spawner calls `ctx.extra_as::<EngineSpawnExtras>()` to recover the
/// typed borrows. The borrows themselves live on the caller's stack
/// frame — same pattern as common's `SpawnCtx` lifetimes: `'w` is the
/// world borrow, no boxing or per-spawn allocation.
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
    use std::any::Any;

    /// Confirms the downcast pattern works as designed — packing
    /// `&mut EngineSpawnExtras` into `&mut dyn Any` and unpacking via
    /// `downcast_mut` recovers the same struct. This is the exact dance
    /// common's `SpawnCtx::extra_as::<EngineSpawnExtras>()` performs.
    ///
    /// If this stops compiling, the `extra` slot's type contract has
    /// drifted and the Wave 3 spawners that downcast will silently
    /// receive `None`.
    #[test]
    fn engine_spawn_extras_downcasts_through_dyn_any() {
        // Build the underlying resources on the stack so the test
        // doesn't need a full Bevy app.
        let mut material_registry = MaterialRegistry::default();
        let mut mesh_cache = PrimitiveMeshCache::default();
        let mut decal_materials =
            Assets::<ForwardDecalMaterial<StandardMaterial>>::default();

        let mut extras = EngineSpawnExtras::new(
            &mut material_registry,
            &mut mesh_cache,
            &mut decal_materials,
        );

        // Pack into `&mut dyn Any` exactly like the engine-side helper
        // would for common's `SpawnCtx::extra`.
        let erased: &mut dyn Any = &mut extras;

        // Unpack — mirrors `SpawnCtx::extra_as::<EngineSpawnExtras>()`.
        let recovered = erased.downcast_mut::<EngineSpawnExtras>();
        assert!(
            recovered.is_some(),
            "downcast back to EngineSpawnExtras must succeed — \
             Wave 3 spawners depend on this contract"
        );
    }
}

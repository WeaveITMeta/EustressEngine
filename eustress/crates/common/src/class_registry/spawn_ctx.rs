//! `SpawnCtx` — the system-param bundle a [`super::ClassSpawner`] needs
//! to build an entity.
//!
//! Per `docs/architecture/CLASS_REGISTRY.md` §3:
//!
//! - **Not a Bevy `Resource`.** Passing it as a resource would force all
//!   asset mutations through a single mutable borrow per frame. Instead,
//!   the engine's loader systems already hold the right `ResMut<...>`
//!   borrows and build a `SpawnCtx` from them per spawn site.
//! - **Not a Bevy `SystemParam`.** `SystemParam` derives are not
//!   object-safe; the trait would break `Box<dyn ClassSpawner>`.
//! - **Engine-agnostic.** This common-side struct exposes only what
//!   `eustress-common` itself knows about: Bevy core types and
//!   `eustress-common`'s own resources (`ClassSchemaResource`,
//!   `MeasureUnit`). Engine-side registries (the engine crate's
//!   `MaterialRegistry`, `PrimitiveMeshCache`, and
//!   `ForwardDecalMaterial` assets) are passed through opaquely as
//!   [`extra`](SpawnCtx::extra) by the engine-side helper that constructs
//!   the ctx (Wave 2.3 ships that helper). This keeps `eustress-common`
//!   independent of `eustress-engine`.
//!
//! Construction lives in the engine crate (Wave 2.3, per spec §3.2);
//! Wave 2 only defines the struct shape.

use std::path::PathBuf;

use bevy::prelude::*;

use crate::class_schema::ClassSchemaResource;
use crate::units::MeasureUnit;

/// System-param bundle threaded through every [`super::ClassSpawner`]
/// method that touches the Bevy `World`.
///
/// Lifetimes mirror Bevy's `Commands<'w, 's>` — `'w` is the world
/// borrow, `'s` is the system state borrow. The struct stores everything
/// it needs by mutable reference so the borrow checker rejects
/// overlapping aliases at the construction site (the engine-side helper)
/// rather than deep inside a spawner.
pub struct SpawnCtx<'w, 's> {
    // ── Always required ──────────────────────────────────────────────
    /// Command queue for spawning the entity and any children.
    pub commands: &'w mut Commands<'w, 's>,

    /// Asset server for resolving `AssetPath` props (meshes, textures,
    /// audio, etc.).
    pub asset_server: &'w AssetServer,

    /// Per-class schema registry — the source of default values for
    /// keys the loaded `PropertyBag` omits.
    pub class_schema: &'w ClassSchemaResource,

    // ── Source provenance ────────────────────────────────────────────
    /// Path of the `_instance.toml` we're cold-loading from, when
    /// applicable. `None` for hot creates (Insert menu, MCP
    /// `create_entity`, Roblox import).
    ///
    /// Spawners that need to record this on the entity (`InstanceFile`
    /// component) read it from here.
    pub source_path: Option<PathBuf>,

    // ── Asset stores — Bevy core, common to many spawners ────────────
    /// Mesh asset store. Mutable because procedural mesh classes
    /// (`Part`, `SpecialMesh`, `Decal` quads) add freshly-built meshes.
    pub meshes: &'w mut Assets<Mesh>,

    /// PBR material asset store. Mutable because most visual classes
    /// register a fresh `StandardMaterial` instance per spawn (color
    /// + texture + emissive vary per entity).
    pub standard_materials: &'w mut Assets<StandardMaterial>,

    /// Image asset store. Mutable because GUI image leaves and decals
    /// register hot-loaded textures, and runtime-generated content
    /// (e.g. AI texture gen) registers fresh images.
    pub images: &'w mut Assets<Image>,

    // ── Hierarchy hints set by the caller ────────────────────────────
    /// Entity this spawned class should be parented to. `None` means
    /// "spawn detached"; the caller (file_loader, importer, Insert
    /// menu) handles the parent wiring after spawn.
    ///
    /// Per spec §12 Q5: keeping this optional preserves the existing
    /// post-spawn parenting pattern. Wave 3 may promote it to required
    /// for specific classes.
    pub parent_entity: Option<Entity>,

    /// Unit the source data was authored in. Spawners that read
    /// dimensional values convert via `eustress_common::units` at the
    /// load boundary — engine-native is meters, always.
    pub measure_unit: MeasureUnit,

    /// True during the cold-load phase of `file_loader`. Spawners
    /// check this to suppress write-back side-effects (the dirty-bit
    /// flag, audit log entries) that would otherwise fire on every
    /// boot.
    pub load_in_progress: bool,

    // ── Engine-specific resources passed through opaquely ────────────
    /// Type-erased extension slot for engine-side resources that
    /// `eustress-common` can't name (Wave 2.3 fills the engine wrapper).
    ///
    /// The engine's helper (Wave 2.3) packages its `MaterialRegistry`,
    /// `PrimitiveMeshCache`, and `Assets<ForwardDecalMaterial<...>>`
    /// borrows into a `dyn Any` here; spawners that need them downcast
    /// to a concrete `SpawnCtxEngineExt` struct defined engine-side.
    /// Spawners that only need Bevy core assets (most lights, Sound,
    /// Folder, …) ignore this field entirely.
    ///
    /// Kept `Option` so common-side tests that don't need any engine
    /// state can pass `None`.
    pub extra: Option<&'w mut dyn std::any::Any>,
}

impl<'w, 's> SpawnCtx<'w, 's> {
    /// Convenience accessor: downcast the [`extra`](Self::extra) slot
    /// to a concrete engine-side struct. Returns `None` when no extra
    /// was supplied or the type doesn't match.
    pub fn extra_as<T: 'static>(&mut self) -> Option<&mut T> {
        self.extra.as_deref_mut().and_then(|a| a.downcast_mut::<T>())
    }
}

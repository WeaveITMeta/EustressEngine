//! # eustress-roblox-import
//!
//! Roblox place file importer for Eustress Engine.
//!
//! Parses `.rbxl`, `.rbxlx`, `.rbxm`, `.rbxmx` (Roblox binary + XML
//! place / model formats) into the Eustress instance model, writing
//! one `_instance.toml` per node through the canonical
//! [`eustress_common::instance_create`] pipeline.
//!
//! ## Spec
//!
//! Full specification: `docs/architecture/ROBLOX_IMPORT_SPEC.md`.
//!
//! From the spec — pipeline shape:
//!
//! ```text
//! File → parser → RobloxDom → service_router → class_map → property_map
//!                                                              ↓
//!                                       asset_resolver → materializer
//!                                                              ↓
//!                          eustress_common::instance_create::create_instance
//!                                                              ↓
//!                          file_watcher → ECS → worlddb (fjall)
//! ```
//!
//! ## Status
//!
//! The core importer is live. [`parse`], [`import_into_space`],
//! and every module call into the real Roblox toolchain (`rbx_dom_weak`,
//! `rbx_binary`, `rbx_xml`).
//!
//! Wave 4.A.2 landed terrain SmoothGrid voxel decode ([`terrain`]) and
//! CSG baked-mesh extraction ([`csg`]): a `Terrain` instance's voxel
//! grid is decoded into LZ4 chunk files under
//! `Workspace/Terrain/voxel_chunks/`, and each
//! `UnionOperation`/`NegateOperation`/`IntersectOperation` has its baked
//! `MeshData` decoded → `csg.glb` and materialised as an asset-meshed
//! `Part` (AABB-block fallback when no mesh is present). CSG
//! re-execution from `ChildData` via `truck-shapeops` (§7.2) remains a
//! stub — the baked-mesh path covers the ~99% common case.
//!
//! Wave 4.A.3 lands the Studio modal + drop-target.
//!
//! ## Public surface
//!
//! ```ignore
//! use eustress_roblox_import::{parse, import_into_space, ImportOptions};
//! let dom = parse(path)?;
//! let report = import_into_space(&dom, space_root, ImportOptions::default())?;
//! ```

#![warn(missing_docs)]

pub mod asset_resolver;
pub mod class_map;
pub mod csg;
pub mod error;
pub mod identity;
pub mod import_report;
pub mod materializer;
pub mod parser;
pub mod property_map;
pub mod service_router;
pub mod terrain;

// ── Re-exports — the only stable public surface. ───────────────────────────
pub use crate::asset_resolver::{AssetReference, ResolvedAsset};
pub use crate::error::ImportError;
pub use crate::identity::entity_uuid;
pub use crate::import_report::{
    Approximation, AssetWarning, ClassCount, ImportReport, NameCollision, ScriptWarning,
    SkippedService, TerrainDecodeError, TerrainMaterialApproximation, UnmappedClass,
    UnmappedProperty, UnresolvedRef,
};
pub use crate::csg::{
    aabb_box_mesh, decode_mesh_data, encode_glb, import_csg, write_glb, CsgError, CsgMesh,
    CsgOutcome,
};
pub use crate::materializer::{import_into_space, ImportOptions, Materializer};
pub use crate::parser::{parse, RobloxDom, RobloxFormat};
pub use crate::property_map::{map_properties, PropertyBag, UnmappedRecord};
pub use crate::service_router::{RouteOutcome, ServiceRouter};
pub use crate::terrain::{
    decode_smooth_grid, import_terrain, DecodeResult, TerrainGlobals, VoxelChunk,
};

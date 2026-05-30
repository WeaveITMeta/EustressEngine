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
//! ## Wave 4.A.1 status
//!
//! The core importer is live. [`parse`], [`import_into_space`],
//! and every module call into the real Roblox toolchain (`rbx_dom_weak`,
//! `rbx_binary`, `rbx_xml`). Wave 4.A.2 wires terrain SmoothGrid decode
//! and CSG baked-mesh extraction (those are emitted as
//! `ImportReport::approximations` entries for now); Wave 4.A.3 lands the
//! Studio modal + drop-target.
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
pub mod error;
pub mod identity;
pub mod import_report;
pub mod materializer;
pub mod parser;
pub mod property_map;
pub mod service_router;

// ── Re-exports — the only stable public surface. ───────────────────────────
pub use crate::asset_resolver::{AssetReference, ResolvedAsset};
pub use crate::error::ImportError;
pub use crate::identity::entity_uuid;
pub use crate::import_report::{
    Approximation, AssetWarning, ClassCount, ImportReport, NameCollision, ScriptWarning,
    SkippedService, TerrainDecodeError, TerrainMaterialApproximation, UnmappedClass,
    UnmappedProperty, UnresolvedRef,
};
pub use crate::materializer::{import_into_space, ImportOptions, Materializer};
pub use crate::parser::{parse, RobloxDom, RobloxFormat};
pub use crate::property_map::{map_properties, PropertyBag, UnmappedRecord};
pub use crate::service_router::{RouteOutcome, ServiceRouter};

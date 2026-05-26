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
//! File → parser → RobloxDom → class_map → property_map → asset_resolver
//!                                            ↓
//!                          eustress_common::instance_create::create_instance
//!                                            ↓
//!                          file_watcher → ECS → worlddb (fjall)
//! ```
//!
//! ## Wave 1 status
//!
//! This is a **scaffold only**. The crate is not yet a workspace member
//! and the `rbx_*` external dependencies are commented-out in
//! `Cargo.toml` awaiting human approval. Function bodies in
//! [`parser`], [`class_map`], and [`property_map`] are `todo!()`.
//! [`import_report`] and [`error`] are real — those are pure type
//! definitions and don't need the external parser to compile.
//!
//! ## Public surface (planned)
//!
//! ```ignore
//! use eustress_roblox_import::{parse, import_into_space, ImportOptions};
//! let dom = parse(path)?;
//! let report = import_into_space(&dom, space_root, ImportOptions::default())?;
//! ```

#![warn(missing_docs)]
#![allow(dead_code)] // scaffold — APIs land in Wave 2

pub mod class_map;
pub mod error;
pub mod identity;
pub mod import_report;
pub mod parser;
pub mod property_map;

// Re-exports — the only stable public surface.
pub use crate::error::ImportError;
pub use crate::import_report::{
    Approximation, AssetWarning, ClassCount, ImportReport, ScriptWarning,
    UnmappedClass, UnmappedProperty,
};
pub use crate::parser::{parse, RobloxDom, RobloxFormat};

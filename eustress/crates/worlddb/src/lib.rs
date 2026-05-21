//! # eustress-worlddb — Fjall-backed authoritative ECS persistence
//!
//! The storage substrate behind the 2026-05-15 binary pivot
//! (see `memory/project_eustress_binary_pivot.md`). The crate provides:
//!
//! - A small [`WorldDb`] trait that fences storage behind an
//!   implementation boundary so the version of Fjall (or even the
//!   backend) can change with measurement, not with calling code.
//! - [`FjallWorldDb`] — the production backend on Fjall 2.x. The
//!   `world.fjalldb/` directory it owns lives inside the `.eustress`
//!   world container alongside `chunks/`, `assets/`, `schema/`.
//! - A [`schema`] module implementing the
//!   `header.bin + per-class schema_version + v{N}: key prefix`
//!   versioning scheme.
//! - A [`changestream`] module emitting per-commit deltas that the
//!   engine bridges into the existing `EustressStream` topics
//!   (`world.entity.*`, `world.commit`, …).
//! - [`keys`] — the `KeyEncoder` trait + a flat encoder today, a
//!   Morton/Hilbert encoder once chunk-streaming locality matters.
//! - [`import`] — one-shot TOML → Fjall migration for existing
//!   universes (Universe1, ARC-AGI-3, Benchmark, Test123).
//! - [`bake`] — Fjall snapshot → `.echk` chunked export per
//!   [05_SPACE_STREAMING] Feature 7.
//!
//! ## What lives in Fjall vs. elsewhere
//!
//! | Layer | Holds | This crate? |
//! |-------|-------|-------------|
//! | `world.fjalldb/` | live ECS state | yes |
//! | `header.bin` | identity + version refs | yes |
//! | `schema/*.toml` | class + service definitions | no (engine loads) |
//! | `chunks/*.echk` | baked spatial snapshots | output of [`bake`] |
//! | `assets/` | meshes, textures, audio | no (asset pipeline) |
//!
//! ## Stability
//!
//! The trait surface is the long-term commitment. The Fjall backend
//! itself is allowed to evolve (different fjall majors, different
//! compaction policies) so long as it preserves the trait semantics
//! and the on-disk schema migration story documented in [`schema`].

#![warn(missing_docs)]

pub mod backend;
pub mod bake;
pub mod changestream;
pub mod datastore;
pub mod error;
pub mod fjall_backend;
pub mod header;
pub mod import;
pub mod keys;
pub mod rkyv_values;
pub mod schema;
pub mod tracing_hooks;

pub use backend::{Commit, EntityId, TreeEntry, WorldDb};
pub use changestream::{ChangeStream, CommitDelta, EntityChange, Filter, Subscription, TxId};
pub use datastore::{DataStore, DataStorePages, DataStoreService, OrderedDataStore};
pub use error::{Error, Result};
pub use fjall_backend::FjallWorldDb;
pub use header::{EngineVersion, WorldHeader, WorldSchemaVersion};
pub use keys::{ComponentTypeId, KeyEncoder, FlatKeyEncoder, MortonKeyEncoder};
pub use rkyv_values::{ArchTransform, access_transform, decode_transform, encode_transform};
pub use schema::{ClassSchemaVersion, SchemaMigration, SchemaMigrationRegistry};

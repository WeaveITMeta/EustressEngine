//! Schema versioning + migration registry.
//!
//! Every persisted value carries an implicit schema version via the
//! `v{N}:` prefix in its [`crate::keys::KeyEncoder`] output. On open,
//! the engine compares the on-disk version against the build's
//! [`crate::header::WorldSchemaVersion::CURRENT`] and walks the
//! [`SchemaMigrationRegistry`] to step the world forward in pure
//! functions.
//!
//! ## The contract
//!
//! - Migrations are pure (no IO, no time, no entropy).
//! - Migrations are idempotent (re-running a migration on a
//!   half-migrated world yields the same end state).
//! - Migrations are tested in CI against a fixture-world for each
//!   prior schema version we've shipped.
//!
//! ## Per-class versioning (Phase 4 hook)
//!
//! Beyond the world-wide schema version, individual class TOMLs in
//! `schema/classes/*.toml` carry their own [`ClassSchemaVersion`].
//! That second axis lets a single world contain a mix of
//! `BasePart_v1` and `BasePart_v2` rows during long-running
//! migrations — the engine resolves each entity's class against the
//! class version stored alongside its `INSTANCE_META`.

use crate::error::Result;
use crate::header::WorldSchemaVersion;

/// Class-level schema version (separate from world-level).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClassSchemaVersion(pub u16);

/// Function pointer for a single migration step `(from) -> (from + 1)`.
/// `&mut dyn Migrator` exposes get / put / iter against the live
/// (mid-migration) DB so the registered function can rewrite values
/// without depending on the full `WorldDb` trait.
pub type SchemaMigrationFn = fn(&mut dyn Migrator) -> Result<()>;

/// Surface a migration function sees. Deliberately narrow so adding a
/// migration doesn't expand its blast radius — the engine wires a
/// `MigratorAdapter` over the live DB.
pub trait Migrator {
    /// Read one key.
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    /// Write one key.
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()>;
    /// Delete one key.
    fn delete(&mut self, key: &[u8]) -> Result<()>;
    /// Scan a prefix. Boxed iterator so trait object stays object-safe.
    fn iter_prefix<'a>(
        &'a self,
        prefix: &[u8],
    ) -> Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>)>> + Send + 'a>;
}

/// Pure record describing a single migration step in the registry.
pub struct SchemaMigration {
    /// Source version this migration runs against.
    pub from: WorldSchemaVersion,
    /// Destination version after this step completes.
    pub to: WorldSchemaVersion,
    /// One-sentence description for logs + the Studio about-panel.
    pub description: &'static str,
    /// Function applied to the DB.
    pub run: SchemaMigrationFn,
}

/// Registry of every known migration. Built once at engine startup;
/// `SchemaMigrationRegistry::plan` returns the linear sequence of
/// migrations to apply for a given `(from, to)` pair, or an error if
/// the chain is broken.
pub struct SchemaMigrationRegistry {
    entries: Vec<SchemaMigration>,
}

impl SchemaMigrationRegistry {
    /// Empty registry. Add migrations via [`Self::register`].
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add a migration step. Order doesn't matter — `plan` sorts by
    /// `from`. Duplicate `from` values panic on the second register
    /// (forking a chain is an explicit anti-pattern — we want a
    /// single linear history).
    pub fn register(&mut self, migration: SchemaMigration) {
        if self.entries.iter().any(|m| m.from == migration.from) {
            panic!(
                "SchemaMigrationRegistry: duplicate migration from {:?}",
                migration.from
            );
        }
        self.entries.push(migration);
    }

    /// Compute the sequence of migrations to apply to step a world
    /// from `from` to `to`. Returns an error if any step is missing.
    pub fn plan(
        &self,
        from: WorldSchemaVersion,
        to: WorldSchemaVersion,
    ) -> Result<Vec<&SchemaMigration>> {
        if from > to {
            return Err(crate::error::Error::SchemaUnsupported(format!(
                "cannot migrate from {} → {} (only forward migrations are supported)",
                from.0, to.0
            )));
        }
        let mut sorted: Vec<&SchemaMigration> = self.entries.iter().collect();
        sorted.sort_by_key(|m| m.from);

        let mut cursor = from;
        let mut plan = Vec::new();
        for step in sorted {
            if step.from == cursor {
                plan.push(step);
                cursor = step.to;
                if cursor == to {
                    return Ok(plan);
                }
            }
        }
        if cursor == to {
            Ok(plan)
        } else {
            Err(crate::error::Error::SchemaUnsupported(format!(
                "no migration path from {} → {} (stuck at {})",
                from.0, to.0, cursor.0
            )))
        }
    }
}

impl Default for SchemaMigrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Hard schema version this build emits. Keep in lockstep with
/// [`crate::header::WorldSchemaVersion::CURRENT`]; the helper is a
/// `const` so call sites don't accidentally hold an older copy.
pub const CURRENT: WorldSchemaVersion = WorldSchemaVersion::CURRENT;

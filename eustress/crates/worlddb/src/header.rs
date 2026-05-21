//! `header.bin` — world identity + version metadata.
//!
//! The first bytes the engine reads when opening an `.eustress`
//! directory. Carries `engine_version` + `world_schema_version` so a
//! loader can:
//!
//! 1. Refuse to open a world produced by a newer engine.
//! 2. Run migration registry entries against an older world.
//! 3. Surface the version to the Studio "About this world" panel.
//!
//! Layout is a small rkyv archive — fixed enough to be readable
//! without the rest of the schema being available, but versioned so
//! the layout itself can evolve. The first 8 bytes are always the
//! magic + format version, which the parser verifies before
//! deserialising the rest.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Magic bytes — `EUSWORLD` ASCII. Lets a hex dump immediately confirm
/// the file is a Eustress header.
pub const HEADER_MAGIC: &[u8; 8] = b"EUSWORLD";

/// Bumped only when the on-disk layout of `header.bin` itself changes
/// — separate from [`WorldSchemaVersion`] which tracks the SCHEMA
/// version of values inside Fjall.
pub const HEADER_FORMAT_VERSION: u32 = 1;

/// File name inside the `.eustress` directory.
pub const HEADER_FILE: &str = "header.bin";

/// Build identity stamp written by the engine that last opened this
/// world. `commit` is the short git SHA so support can reproduce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineVersion {
    /// Major.Minor.Patch following the engine crate version.
    pub semver: String,
    /// Short git SHA at the time `header.bin` was written; empty if
    /// the build wasn't a git checkout.
    pub commit: String,
    /// UTC ISO-8601 timestamp the engine wrote this header.
    pub written_at: String,
}

impl EngineVersion {
    /// Stamp the current engine build into the header. The git SHA is
    /// optional — callers without it pass an empty string.
    pub fn current(semver: impl Into<String>, commit: impl Into<String>) -> Self {
        Self {
            semver: semver.into(),
            commit: commit.into(),
            written_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Schema version of the data INSIDE Fjall — bumped when the value
/// archives or key layout change. Stored next to a registered
/// [`crate::schema::SchemaMigration`] chain that can step a world
/// up from any older version to the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WorldSchemaVersion(pub u16);

impl WorldSchemaVersion {
    /// The schema version this build emits for fresh worlds.
    pub const CURRENT: WorldSchemaVersion = WorldSchemaVersion(1);

    /// Returns `true` when the on-disk schema is older than ours and a
    /// migration registry entry exists for the step.
    pub fn needs_migration_to(self, target: WorldSchemaVersion) -> bool {
        self.0 < target.0
    }

    /// Returns `true` when the on-disk schema is NEWER than this
    /// build — refusal case ("update your engine, this world was
    /// authored on a later version").
    pub fn is_future(self, current: WorldSchemaVersion) -> bool {
        self.0 > current.0
    }
}

/// In-memory representation of `header.bin`. Serialised via serde to
/// a tagged binary form for stability; rkyv-archived value types live
/// inside Fjall, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldHeader {
    /// Format version of this header itself.
    pub header_format_version: u32,
    /// Schema version of the data inside `world.fjalldb/`.
    pub world_schema_version: WorldSchemaVersion,
    /// Build stamp from the engine that last wrote the header.
    pub engine: EngineVersion,
    /// Asset-registry pointer — relative path inside the `.eustress`
    /// directory where the asset manifest lives. Default: `assets/manifest.toml`.
    pub asset_manifest: String,
    /// Optional UUID giving this world a globally-unique identifier.
    /// Used for cloud sync / multiplayer routing.
    pub world_id: uuid::Uuid,
    /// UTC ISO-8601 timestamp at which the loose on-disk service trees
    /// were verified-imported into `world.fjalldb/` and moved out of
    /// the Space root (the "clean `.eustress` conversion"). `None` for
    /// a world that has never been converted. When set, the Space root
    /// is the canonical container shape (`header.bin` + `world.fjalldb/`
    /// + `assets/` + `schema/` + `.eustress/`) and the DB is fully
    /// authoritative — there are no loose `_service.toml` /
    /// `_instance.toml` trees left to diverge from.
    #[serde(default)]
    pub migrated_at: Option<String>,
}

impl Default for WorldHeader {
    fn default() -> Self {
        Self {
            header_format_version: HEADER_FORMAT_VERSION,
            world_schema_version: WorldSchemaVersion::CURRENT,
            engine: EngineVersion::current(env!("CARGO_PKG_VERSION"), ""),
            asset_manifest: "assets/manifest.toml".to_string(),
            world_id: uuid::Uuid::new_v4(),
            migrated_at: None,
        }
    }
}

impl WorldHeader {
    /// Read `header.bin` from inside a `.eustress` directory. Returns
    /// `Ok(None)` when the file is absent (caller's choice whether to
    /// treat that as "first open" and write a fresh header).
    pub fn read(world_root: &Path) -> Result<Option<Self>> {
        let path = world_root.join(HEADER_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path)?;
        Self::decode(&bytes).map(Some)
    }

    /// Stamp this header as fully converted to the canonical
    /// `.eustress` container shape (loose disk trees verified-imported
    /// and removed). Idempotent — re-stamping refreshes the timestamp.
    pub fn mark_migrated(&mut self) {
        self.migrated_at = Some(chrono::Utc::now().to_rfc3339());
    }

    /// True once the loose service trees have been verified-imported
    /// and the Space root is the canonical container shape.
    pub fn is_migrated(&self) -> bool {
        self.migrated_at.is_some()
    }

    /// Atomic write — temp-file + rename so a crash mid-write can
    /// never leave a half-written header.
    pub fn write(&self, world_root: &Path) -> Result<()> {
        let path = world_root.join(HEADER_FILE);
        let tmp = path.with_extension("bin.tmp");
        let bytes = self.encode()?;
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Magic+version-prefixed encoding. Layout:
    ///
    /// ```text
    /// [0..8]   HEADER_MAGIC (b"EUSWORLD")
    /// [8..12]  HEADER_FORMAT_VERSION (u32 le)
    /// [12..]   TOML body (small, human-inspectable in a pinch)
    /// ```
    ///
    /// TOML for the body so a future engine without the exact
    /// `WorldHeader` struct can still read identity fields out of a
    /// hex dump. Body is small (~hundreds of bytes); TOML cost is
    /// negligible and matches the rest of the schema/* convention.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let body = toml::to_string(self).map_err(|e| Error::Header(e.to_string()))?;
        let body_bytes = body.into_bytes();
        let mut out = Vec::with_capacity(12 + body_bytes.len());
        out.extend_from_slice(HEADER_MAGIC);
        out.extend_from_slice(&HEADER_FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&body_bytes);
        Ok(out)
    }

    /// Inverse of [`Self::encode`].
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 12 {
            return Err(Error::Header(format!(
                "header truncated: {} bytes",
                bytes.len()
            )));
        }
        if &bytes[..8] != HEADER_MAGIC {
            return Err(Error::Header(
                "header magic mismatch — file is not an Eustress world header".to_string(),
            ));
        }
        let mut fmt = [0u8; 4];
        fmt.copy_from_slice(&bytes[8..12]);
        let format_version = u32::from_le_bytes(fmt);
        if format_version > HEADER_FORMAT_VERSION {
            return Err(Error::Header(format!(
                "header format version {} is newer than supported ({})",
                format_version, HEADER_FORMAT_VERSION
            )));
        }
        let body_str = std::str::from_utf8(&bytes[12..])
            .map_err(|e| Error::Header(format!("body not utf-8: {e}")))?;
        let body: WorldHeader =
            toml::from_str(body_str).map_err(|e| Error::Header(e.to_string()))?;
        Ok(body)
    }
}

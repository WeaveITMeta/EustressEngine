//! Error type shared across the crate.

use thiserror::Error;

/// Result alias used by every public API in `eustress-worlddb`.
pub type Result<T> = std::result::Result<T, Error>;

/// Top-level error. Backend-specific errors are wrapped via
/// `#[from]` so callers can pattern-match on the high-level kind
/// without depending on the underlying storage crate's error type.
#[derive(Debug, Error)]
pub enum Error {
    /// Wrapper around [`fjall::Error`] for any failure originating
    /// in the LSM layer (compaction, flush, IO).
    #[error("fjall: {0}")]
    Fjall(#[from] fjall::Error),

    /// `std::io::Error` for header.bin / schema directory operations.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Schema migration refused — the on-disk world is newer than this
    /// build understands, or older than any migration can recover.
    #[error("schema migration unsupported: {0}")]
    SchemaUnsupported(String),

    /// rkyv archive layer (Tier 1 #2) reported a value bytes failure.
    #[error("archive: {0}")]
    Archive(String),

    /// `header.bin` parse / validation failure.
    #[error("header: {0}")]
    Header(String),

    /// Caller passed a key shape the [`crate::keys::KeyEncoder`] could
    /// not decode (foreign data, wrong schema version, truncated bytes).
    #[error("key decode: {0}")]
    KeyDecode(String),

    /// TOML parse / shape error during import.
    #[error("toml: {0}")]
    Toml(#[from] toml::de::Error),

    /// Catch-all for invariant violations that aren't worth their own
    /// variant. Use sparingly; promote anything user-facing.
    #[error("worlddb: {0}")]
    Other(String),
}

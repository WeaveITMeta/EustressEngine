//! Error type for `eustress-data`.
//!
//! Deliberately holds only owned strings (no `arrow` / `parquet` / `polars`
//! types), so the public [`Result`] keeps the backing columnar engine
//! swappable — the discipline `eustress-worlddb` uses for `fjall`
//! (DATA_PLATFORM_PLAN.md D2).

use std::fmt;

/// Errors from the columnar substrate.
#[derive(Debug)]
pub enum DataError {
    /// Filesystem I/O failure.
    Io(std::io::Error),
    /// Parquet reader/writer failure (message flattened from the backend).
    Parquet(String),
    /// Arrow array/schema failure (message flattened from the backend).
    Arrow(String),
    /// Frame/column shape or dtype mismatch, caught before touching a backend.
    Schema(String),
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Parquet(m) => write!(f, "parquet error: {m}"),
            Self::Arrow(m) => write!(f, "arrow error: {m}"),
            Self::Schema(m) => write!(f, "schema error: {m}"),
        }
    }
}

impl std::error::Error for DataError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for DataError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Crate result alias.
pub type Result<T> = std::result::Result<T, DataError>;

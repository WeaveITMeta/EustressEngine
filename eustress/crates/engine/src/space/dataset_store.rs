//! P1d — materialize a Series' columns (an `eustress-data` [`Frame`]) into the
//! WorldDb `datasets` partition, and load it back.
//!
//! This is the bridge where the off-by-default `data` feature (arrow/parquet,
//! via `eustress-data`) meets the always-on WorldDb storage layer. It lives on
//! the engine side ON PURPOSE: neither leaf crate depends on the other —
//! `eustress-data` knows nothing of Fjall, `eustress-worlddb` knows nothing of
//! arrow — so the columnar↔storage glue belongs above both (DATA_PLATFORM_PLAN
//! §A.6 / D2). The partition's bytes are an opaque Parquet blob; the schema +
//! unit + dimension ride inside it (Parquet field metadata), so a load fully
//! reconstructs the `Frame`.
//!
//! Gated on `all(feature = "data", feature = "world-db")` — it needs both the
//! columnar leaf and the storage trait.

use eustress_data::{chunks_to_frame, frame_to_chunks, Chunk, Frame};
use eustress_worlddb::WorldDb;

/// Folds the columnar-layer and storage-layer errors into one engine-side type
/// so a caller handles a single `Result`.
#[derive(Debug)]
pub enum DatasetStoreError {
    /// An `eustress-data` (arrow/parquet) failure.
    Data(eustress_data::DataError),
    /// A WorldDb storage failure.
    Db(eustress_worlddb::Error),
    /// The frame produced no blob (e.g. a frame with no columns).
    Empty,
}

impl std::fmt::Display for DatasetStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Data(e) => write!(f, "dataset columnar error: {e}"),
            Self::Db(e) => write!(f, "dataset storage error: {e}"),
            Self::Empty => write!(f, "frame produced no dataset blob"),
        }
    }
}

impl std::error::Error for DatasetStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Data(e) => Some(e),
            Self::Db(e) => Some(e),
            Self::Empty => None,
        }
    }
}

impl From<eustress_data::DataError> for DatasetStoreError {
    fn from(e: eustress_data::DataError) -> Self {
        Self::Data(e)
    }
}
impl From<eustress_worlddb::Error> for DatasetStoreError {
    fn from(e: eustress_worlddb::Error) -> Self {
        Self::Db(e)
    }
}

/// Engine-side dataset-bridge result.
pub type Result<T> = std::result::Result<T, DatasetStoreError>;

/// Materialize a whole [`Frame`] into the `datasets` partition under `id` as a
/// single self-describing Parquet blob.
///
/// One blob per dataset for P1d (the chunked multi-blob layout for very large
/// or high-rate series arrives with the timeseries-compaction path); the
/// `frame_to_chunks` target is "all rows", so the frame becomes exactly one
/// chunk whose bytes are stored under [`WorldDb::put_dataset_chunk`].
pub fn materialize_frame_to_dataset(db: &dyn WorldDb, id: [u8; 16], frame: &Frame) -> Result<()> {
    let blob = frame_to_chunks(frame, usize::MAX)?
        .into_iter()
        .next()
        .ok_or(DatasetStoreError::Empty)?
        .bytes;
    db.put_dataset_chunk(&id, &blob)?;
    Ok(())
}

/// Load a dataset blob written by [`materialize_frame_to_dataset`] back into a
/// [`Frame`], recovering dtypes, units, and dimensions from the Parquet
/// metadata. `Ok(None)` when no dataset exists at `id`.
pub fn load_dataset_frame(db: &dyn WorldDb, id: [u8; 16]) -> Result<Option<Frame>> {
    match db.get_dataset_chunk(&id)? {
        // `chunks_to_frame` reads the row count from the Parquet itself, so the
        // `n_rows` field here is just a placeholder.
        Some(bytes) => Ok(Some(chunks_to_frame(vec![Chunk { bytes, n_rows: 0 }])?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_data::{frame_from_columns, ColumnData, ColumnDtype, ColumnSpec};

    #[test]
    fn series_materializes_to_dataset_blob_and_loads_back() {
        // A two-column Series with units + dimensions, nulls included.
        let frame = frame_from_columns(vec![
            (
                ColumnSpec::new("t", ColumnDtype::F64)
                    .with_unit("s")
                    .with_dimension("si:T1"),
                ColumnData::F64(vec![Some(0.0), Some(0.5), None, Some(1.5)]),
            ),
            (
                ColumnSpec::new("psi", ColumnDtype::F64)
                    .with_unit("psi")
                    .with_dimension("si:M1L-1T-2"),
                ColumnData::F64(vec![Some(14.7), Some(15.0), Some(15.2), None]),
            ),
        ])
        .unwrap();

        let dir = std::env::temp_dir().join("eustress_p1d_dataset_store_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = eustress_worlddb::FjallWorldDb::open(&dir).unwrap();

        let id = [9u8; 16];
        // Absent before the write.
        assert!(load_dataset_frame(&db, id).unwrap().is_none());

        // Materialize → load back → identical Frame (schema + units + dims +
        // nulls all survive the Parquet blob round-trip through the partition).
        materialize_frame_to_dataset(&db, id, &frame).unwrap();
        let back = load_dataset_frame(&db, id)
            .unwrap()
            .expect("dataset present after materialize");
        assert_eq!(
            back, frame,
            "Series → datasets blob → Series round-trip changed the frame"
        );

        // And it shows up in the partition scan.
        assert_eq!(db.iter_dataset_chunks().unwrap().len(), 1);
    }
}

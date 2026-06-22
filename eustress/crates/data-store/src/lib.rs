//! # eustress-data-store — the Data Platform bridge (engine-free)
//!
//! Where the columnar layer ([`eustress_data::Frame`]) meets the storage layer
//! ([`eustress_worlddb::WorldDb`]). Two responsibilities:
//!
//! - **Datasets (P1d):** materialize a Series' columns into the `datasets`
//!   partition as a Parquet blob, and load it back ([`materialize_frame_to_dataset`],
//!   [`load_dataset_frame`], [`list_dataset_ids`]).
//! - **Timeseries (P2):** the durable **Recorder** seam — a bounded,
//!   drop-on-full batch buffer that commits f64 samples to the `timeseries`
//!   partition ([`RecorderBuffer`]) — and the query front door that reads a
//!   time window back as a `Frame` ([`query_timeseries_frame`]).
//!
//! This crate is engine-free (no bevy), so all of it is unit-tested against a
//! real on-disk `FjallWorldDb` in milliseconds. The engine mounts the Bevy
//! Recorder system (subscribing `sensor.<name>` stream topics) and the MCP
//! `query_stream_events` handler ON TOP of these functions — that wiring is the
//! only engine-build part of P2.

use eustress_data::{
    frame_from_columns, ColumnData, ColumnDtype, ColumnSpec, Frame,
};
use eustress_worlddb::WorldDb;

/// Folds the columnar-layer and storage-layer errors into one bridge error.
#[derive(Debug)]
pub enum DataStoreError {
    /// `eustress-data` (arrow/parquet) failure.
    Data(eustress_data::DataError),
    /// WorldDb storage failure.
    Db(eustress_worlddb::Error),
    /// A stored row could not be decoded (wrong byte length, etc.).
    Decode(String),
}

impl std::fmt::Display for DataStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Data(e) => write!(f, "data error: {e}"),
            Self::Db(e) => write!(f, "storage error: {e}"),
            Self::Decode(m) => write!(f, "decode error: {m}"),
        }
    }
}
impl std::error::Error for DataStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Data(e) => Some(e),
            Self::Db(e) => Some(e),
            Self::Decode(_) => None,
        }
    }
}
impl From<eustress_data::DataError> for DataStoreError {
    fn from(e: eustress_data::DataError) -> Self {
        Self::Data(e)
    }
}
impl From<eustress_worlddb::Error> for DataStoreError {
    fn from(e: eustress_worlddb::Error) -> Self {
        Self::Db(e)
    }
}

/// Bridge result.
pub type Result<T> = std::result::Result<T, DataStoreError>;

// ── Datasets (P1d) ───────────────────────────────────────────────────────────

/// Materialize a [`Frame`] into the `datasets` partition under `id` as a single
/// self-describing Parquet blob.
pub fn materialize_frame_to_dataset(db: &dyn WorldDb, id: [u8; 16], frame: &Frame) -> Result<()> {
    let blob = eustress_data::frame_to_chunks(frame, usize::MAX)?
        .into_iter()
        .next()
        .ok_or_else(|| DataStoreError::Decode("frame produced no dataset blob".into()))?
        .bytes;
    db.put_dataset_chunk(&id, &blob)?;
    Ok(())
}

/// Load a dataset blob back into a [`Frame`] (schema, units, dimensions
/// recovered from the Parquet metadata). `Ok(None)` when absent.
pub fn load_dataset_frame(db: &dyn WorldDb, id: [u8; 16]) -> Result<Option<Frame>> {
    match db.get_dataset_chunk(&id)? {
        Some(bytes) => Ok(Some(eustress_data::chunks_to_frame(vec![
            eustress_data::Chunk { bytes, n_rows: 0 },
        ])?)),
        None => Ok(None),
    }
}

/// Every materialized dataset id in the `datasets` partition.
pub fn list_dataset_ids(db: &dyn WorldDb) -> Result<Vec<[u8; 16]>> {
    Ok(db
        .iter_dataset_chunks()?
        .into_iter()
        .map(|(id, _)| id)
        .collect())
}

// ── Timeseries (P2): f64-sample codec, Recorder, query front door ────────────

/// Encode one f64 sample as the `timeseries` row payload (8 little-endian
/// bytes). The row format is a private contract between [`RecorderBuffer`] and
/// [`query_timeseries_frame`].
fn encode_sample(v: f64) -> [u8; 8] {
    v.to_le_bytes()
}

fn decode_sample(bytes: &[u8]) -> Result<f64> {
    if bytes.len() != 8 {
        return Err(DataStoreError::Decode(format!(
            "expected an 8-byte f64 sample, got {} bytes",
            bytes.len()
        )));
    }
    let mut a = [0u8; 8];
    a.copy_from_slice(bytes);
    Ok(f64::from_le_bytes(a))
}

/// The durable Recorder seam (D5): one bounded, **drop-on-full** batch buffer
/// per series. Samples accumulate until [`RecorderBuffer::flush`] commits them
/// to the `timeseries` partition. When full, [`RecorderBuffer::push`] drops the
/// sample and bumps [`RecorderBuffer::dropped`] — bounded loss with a counter,
/// never a stall (the plan's backpressure rule), so a dataset that lost samples
/// can say so.
pub struct RecorderBuffer {
    series: String,
    cap: usize,
    buf: Vec<(u64, u32, f64)>,
    dropped: u64,
}

impl RecorderBuffer {
    /// A recorder for `series` holding at most `cap` un-flushed samples.
    pub fn new(series: impl Into<String>, cap: usize) -> Self {
        Self {
            series: series.into(),
            cap: cap.max(1),
            buf: Vec::new(),
            dropped: 0,
        }
    }

    /// Buffer one sample `(ts, seq, value)`. Returns `false` (and increments
    /// [`RecorderBuffer::dropped`]) if the buffer is at capacity.
    pub fn push(&mut self, ts: u64, seq: u32, value: f64) -> bool {
        if self.buf.len() >= self.cap {
            self.dropped += 1;
            false
        } else {
            self.buf.push((ts, seq, value));
            true
        }
    }

    /// Number of buffered (un-flushed) samples.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// True when nothing is buffered.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Total samples dropped due to backpressure since creation.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Commit all buffered samples to the `timeseries` partition, returning the
    /// number flushed. On a storage error the buffer is left intact (retryable);
    /// only a fully-successful flush clears it.
    pub fn flush(&mut self, db: &dyn WorldDb) -> Result<usize> {
        let n = self.buf.len();
        for (ts, seq, v) in &self.buf {
            db.ts_append(&self.series, *ts, *seq, &encode_sample(*v))?;
        }
        self.buf.clear();
        Ok(n)
    }
}

/// Query one series for the inclusive time window `[min_ts, max_ts]`, returning
/// a two-column [`Frame`] — `t` (I64 timestamp) and `value` (F64) — ascending
/// in time. The query front door behind the engine's `query_stream_events`.
pub fn query_timeseries_frame(
    db: &dyn WorldDb,
    series: &str,
    min_ts: u64,
    max_ts: u64,
) -> Result<Frame> {
    let rows = db.ts_range(series, min_ts, max_ts)?;
    let mut t = Vec::with_capacity(rows.len());
    let mut val = Vec::with_capacity(rows.len());
    for (ts, _seq, bytes) in rows {
        t.push(Some(ts as i64));
        val.push(Some(decode_sample(&bytes)?));
    }
    Ok(frame_from_columns(vec![
        (ColumnSpec::new("t", ColumnDtype::I64), ColumnData::I64(t)),
        (ColumnSpec::new("value", ColumnDtype::F64), ColumnData::F64(val)),
    ])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_data::frame_from_columns as fc;
    use eustress_worlddb::FjallWorldDb;

    fn fresh_db(name: &str) -> (FjallWorldDb, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (FjallWorldDb::open(&dir).expect("open FjallWorldDb"), dir)
    }

    #[test]
    fn dataset_materialize_load_and_list() {
        let frame = fc(vec![
            (
                ColumnSpec::new("psi", ColumnDtype::F64)
                    .with_unit("psi")
                    .with_dimension("si:M1L-1T-2"),
                ColumnData::F64(vec![Some(14.7), None, Some(15.2)]),
            ),
        ])
        .unwrap();
        let (db, _d) = fresh_db("eustress_dstore_dataset");
        let id = [7u8; 16];
        assert!(load_dataset_frame(&db, id).unwrap().is_none());
        materialize_frame_to_dataset(&db, id, &frame).unwrap();
        assert_eq!(load_dataset_frame(&db, id).unwrap().unwrap(), frame);
        assert_eq!(list_dataset_ids(&db).unwrap(), vec![id]);
    }

    #[test]
    fn recorder_batches_drops_and_queries_back() {
        let (db, _d) = fresh_db("eustress_dstore_recorder");
        let mut rec = RecorderBuffer::new("sensor.psi", 3);
        assert!(rec.push(1000, 0, 14.7));
        assert!(rec.push(2000, 0, 15.0));
        assert!(rec.push(3000, 0, 15.2));
        assert!(!rec.push(4000, 0, 99.9), "4th push over cap=3 is dropped");
        assert_eq!(rec.dropped(), 1);
        assert_eq!(rec.len(), 3);

        assert_eq!(rec.flush(&db).unwrap(), 3);
        assert!(rec.is_empty());

        // Query the window back as a Frame.
        let frame = query_timeseries_frame(&db, "sensor.psi", 0, 10_000).unwrap();
        assert_eq!(frame.n_rows(), 3);
        match frame.column("value").unwrap() {
            ColumnData::F64(v) => {
                assert_eq!(v[0], Some(14.7));
                assert_eq!(v[2], Some(15.2));
            }
            _ => panic!("value should be F64"),
        }
        match frame.column("t").unwrap() {
            ColumnData::I64(v) => assert_eq!(v[0], Some(1000)),
            _ => panic!("t should be I64"),
        }
        // A narrower window excludes the first sample.
        assert_eq!(
            query_timeseries_frame(&db, "sensor.psi", 1500, 10_000)
                .unwrap()
                .n_rows(),
            2
        );
        // The dropped 99.9 never reached storage.
        assert!(!query_timeseries_frame(&db, "sensor.psi", 0, 10_000)
            .unwrap()
            .column("value")
            .map(|c| matches!(c, ColumnData::F64(v) if v.contains(&Some(99.9))))
            .unwrap());
    }
}

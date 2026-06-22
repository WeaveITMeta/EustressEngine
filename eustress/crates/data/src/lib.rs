//! # eustress-data — the columnar substrate (Data Platform, Phase P0)
//!
//! The off-by-default columnar leaf of the Eustress Data Platform
//! (`docs/architecture/DATA_PLATFORM_PLAN.md`, invariant **D2**). Arrow-rs and
//! Parquet live ONLY here; this crate must never enter the engine `core` /
//! `default` feature tier (enforced by the `data-graph-purity` CI guard). The
//! public surface names [`Frame`] / [`ColumnSpec`] / [`ColumnData`] — never
//! `arrow::*` or `parquet::*` — so the backing engine stays swappable.
//!
//! ## P0 scope
//! - In-memory columnar [`Frame`] of unit-tagged columns (the four
//!   Logger-Pro-grade scalars — f64 / i64 / bool / str — each nullable).
//! - Parquet round-trip ([`write_parquet`] / [`read_parquet`]); the unit symbol
//!   rides in Parquet field metadata, so it survives the round-trip.
//! - [`Frame`] ⇄ [`Chunk`] slicing ([`frame_to_chunks`] / [`chunks_to_frame`])
//!   — the encoder the `datasets` / `timeseries` WorldDb partitions consume in
//!   P1; each chunk is a self-describing Parquet blob.
//!
//! The eager `frames` (Polars) and `query` (Polars-lazy) tiers named in the
//! plan are intentionally absent until their compile cost is measured on this
//! hardware (plan Risk 1).

mod error;
pub use error::{DataError, Result};

// ── Analysis & transform (P3/P4/P6 computational cores) ──────────────────────
/// Descriptive stats, derivative, integral, interpolation, linear/poly fit
/// (P4). Pure std — always compiled.
pub mod numerics;
/// Min-max LOD decimation for million-point charts (P3 data side). Pure std.
pub mod decimate;
/// FFT / magnitude spectrum (P4). Requires the `spectral` feature.
#[cfg(feature = "spectral")]
pub mod spectral;
/// CSV / JSON(L) import → `Frame` with dtype + unit inference (P6). Requires
/// the `import` feature.
#[cfg(feature = "import")]
pub mod import;

// ── Core types — always compiled, no arrow/parquet dependency ────────────────

/// Logical column element type. P0 covers the four Logger-Pro-grade scalars;
/// datetime / decimal arrive with the timeseries partition (P1+).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnDtype {
    /// 64-bit float.
    F64,
    /// 64-bit signed integer.
    I64,
    /// Boolean.
    Bool,
    /// UTF-8 string.
    Str,
}

impl ColumnDtype {
    /// Stable token used in Parquet field metadata (and any future schema file).
    pub fn as_token(self) -> &'static str {
        match self {
            Self::F64 => "f64",
            Self::I64 => "i64",
            Self::Bool => "bool",
            Self::Str => "str",
        }
    }

    /// Parse [`ColumnDtype::as_token`].
    pub fn from_token(s: &str) -> Option<Self> {
        match s {
            "f64" => Some(Self::F64),
            "i64" => Some(Self::I64),
            "bool" => Some(Self::Bool),
            "str" => Some(Self::Str),
            _ => None,
        }
    }
}

/// Column header: name, element type, the unit symbol, and the SI dimension it
/// was authored in.
///
/// Both `unit` and `dimension` are free-form strings here — the lean leaf carries
/// no `common`/bevy dependency, so it stores them opaquely. `unit` is the human
/// symbol (`"psi"`, `"deg"`); `dimension` is the canonical SI exponent form from
/// `eustress_common::dimension::Dimension::to_si_string` (`"si:M1L-1T-2"`) or a
/// named symbol. The engine boundary populates `dimension` via
/// `Dimension::from_unit_symbol(unit)` (plan D3). Both ride Parquet field
/// metadata, so they round-trip losslessly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnSpec {
    /// Column name (unique within a [`Frame`]).
    pub name: String,
    /// Element type.
    pub dtype: ColumnDtype,
    /// Optional unit symbol, e.g. `"psi"`, `"deg"`, `"m/s"`.
    pub unit: Option<String>,
    /// Optional SI dimension token, e.g. `"si:M1L-1T-2"` or `"Pa"`. Opaque to
    /// the leaf; parsed/validated by `common::dimension::Dimension` at the
    /// engine boundary.
    pub dimension: Option<String>,
}

impl ColumnSpec {
    /// A column header with no unit or dimension.
    pub fn new(name: impl Into<String>, dtype: ColumnDtype) -> Self {
        Self { name: name.into(), dtype, unit: None, dimension: None }
    }

    /// Builder: attach a unit symbol.
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Builder: attach an SI dimension token (canonical `"si:…"` or a named
    /// symbol). Opaque to the leaf; the engine parses it via `common::Dimension`.
    pub fn with_dimension(mut self, dimension: impl Into<String>) -> Self {
        self.dimension = Some(dimension.into());
        self
    }
}

/// Column values. `None` marks a missing / null cell (Logger-Pro gaps).
#[derive(Clone, Debug, PartialEq)]
pub enum ColumnData {
    /// 64-bit floats.
    F64(Vec<Option<f64>>),
    /// 64-bit signed integers.
    I64(Vec<Option<i64>>),
    /// Booleans.
    Bool(Vec<Option<bool>>),
    /// UTF-8 strings.
    Str(Vec<Option<String>>),
}

impl ColumnData {
    /// Number of cells (rows) in the column.
    pub fn len(&self) -> usize {
        match self {
            Self::F64(v) => v.len(),
            Self::I64(v) => v.len(),
            Self::Bool(v) => v.len(),
            Self::Str(v) => v.len(),
        }
    }

    /// True when the column has no cells.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The element type of this column's data.
    pub fn dtype(&self) -> ColumnDtype {
        match self {
            Self::F64(_) => ColumnDtype::F64,
            Self::I64(_) => ColumnDtype::I64,
            Self::Bool(_) => ColumnDtype::Bool,
            Self::Str(_) => ColumnDtype::Str,
        }
    }
}

/// In-memory columnar working set. Opaque over its backing representation so
/// callers never name `arrow::*` (D2). Every column shares one row count.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    columns: Vec<(ColumnSpec, ColumnData)>,
    n_rows: usize,
}

impl Frame {
    /// Number of rows (shared across all columns).
    pub fn n_rows(&self) -> usize {
        self.n_rows
    }

    /// Number of columns.
    pub fn n_cols(&self) -> usize {
        self.columns.len()
    }

    /// Iterate the column headers in order.
    pub fn specs(&self) -> impl Iterator<Item = &ColumnSpec> {
        self.columns.iter().map(|(s, _)| s)
    }

    /// Borrow a column's data by name.
    pub fn column(&self, name: &str) -> Option<&ColumnData> {
        self.columns.iter().find(|(s, _)| s.name == name).map(|(_, d)| d)
    }

    /// Borrow all `(spec, data)` pairs in order.
    pub fn columns(&self) -> &[(ColumnSpec, ColumnData)] {
        &self.columns
    }

    /// Consume the frame into its `(spec, data)` pairs.
    pub fn into_columns(self) -> Vec<(ColumnSpec, ColumnData)> {
        self.columns
    }
}

/// Build a [`Frame`] from columns, validating that every declared dtype matches
/// its data, every column shares one row count, and names are unique.
pub fn frame_from_columns(columns: Vec<(ColumnSpec, ColumnData)>) -> Result<Frame> {
    let n_rows = columns.first().map(|(_, d)| d.len()).unwrap_or(0);
    let mut seen = std::collections::HashSet::new();
    for (spec, data) in &columns {
        if !seen.insert(spec.name.as_str()) {
            return Err(DataError::Schema(format!("duplicate column name `{}`", spec.name)));
        }
        if data.dtype() != spec.dtype {
            return Err(DataError::Schema(format!(
                "column `{}`: spec dtype {:?} does not match data {:?}",
                spec.name,
                spec.dtype,
                data.dtype()
            )));
        }
        if data.len() != n_rows {
            return Err(DataError::Schema(format!(
                "column `{}` has {} rows; expected {} (ragged frame)",
                spec.name,
                data.len(),
                n_rows
            )));
        }
    }
    Ok(Frame { columns, n_rows })
}

// ── Parquet I/O — only when the `parquet` feature is on ──────────────────────

#[cfg(feature = "parquet")]
mod pq {
    use super::{ColumnData, ColumnDtype, ColumnSpec, DataError, Frame, Result};
    use std::collections::HashMap;
    use std::fs::File;
    use std::path::Path;
    use std::sync::Arc;

    use arrow::array::{
        Array, ArrayRef, BooleanArray, Float64Array, Int64Array, StringArray,
    };
    use arrow::datatypes::{DataType, Field, Schema};
    use arrow::record_batch::RecordBatch;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    use parquet::arrow::ArrowWriter;
    use parquet::basic::Compression;
    use parquet::file::properties::WriterProperties;
    use parquet::file::reader::ChunkReader;

    /// Field-metadata key carrying the [`ColumnSpec::unit`] symbol.
    const UNIT_META_KEY: &str = "eustress.unit";
    /// Field-metadata key carrying the [`ColumnDtype`] token (so an `i64` read
    /// back as Arrow `Int64` keeps its exact logical type even though several
    /// logical dtypes could share one Arrow type in the future).
    const DTYPE_META_KEY: &str = "eustress.dtype";
    /// Field-metadata key carrying the [`ColumnSpec::dimension`] token.
    const DIM_META_KEY: &str = "eustress.dim";

    /// A self-describing Parquet blob for one row-range of a [`Frame`] — the
    /// unit the `datasets` / `timeseries` partitions store in P1.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Chunk {
        /// The complete Parquet file bytes for this row-range.
        pub bytes: Vec<u8>,
        /// Number of rows encoded in this chunk.
        pub n_rows: usize,
    }

    impl super::ColumnData {
        fn slice(&self, start: usize, len: usize) -> ColumnData {
            let end = start + len;
            match self {
                ColumnData::F64(v) => ColumnData::F64(v[start..end].to_vec()),
                ColumnData::I64(v) => ColumnData::I64(v[start..end].to_vec()),
                ColumnData::Bool(v) => ColumnData::Bool(v[start..end].to_vec()),
                ColumnData::Str(v) => ColumnData::Str(v[start..end].to_vec()),
            }
        }
    }

    impl super::Frame {
        fn slice_rows(&self, start: usize, len: usize) -> Frame {
            Frame {
                columns: self
                    .columns
                    .iter()
                    .map(|(s, d)| (s.clone(), d.slice(start, len)))
                    .collect(),
                n_rows: len,
            }
        }
    }

    fn frame_to_record_batch(frame: &Frame) -> Result<RecordBatch> {
        if frame.columns.is_empty() {
            return Err(DataError::Schema(
                "cannot serialize a frame with no columns".into(),
            ));
        }
        let mut fields = Vec::with_capacity(frame.columns.len());
        let mut arrays: Vec<ArrayRef> = Vec::with_capacity(frame.columns.len());
        for (spec, data) in &frame.columns {
            let (dt, arr): (DataType, ArrayRef) = match data {
                ColumnData::F64(v) => (DataType::Float64, Arc::new(Float64Array::from(v.clone()))),
                ColumnData::I64(v) => (DataType::Int64, Arc::new(Int64Array::from(v.clone()))),
                ColumnData::Bool(v) => (DataType::Boolean, Arc::new(BooleanArray::from(v.clone()))),
                ColumnData::Str(v) => {
                    let a: StringArray = v.iter().map(|o| o.as_deref()).collect();
                    (DataType::Utf8, Arc::new(a))
                }
            };
            let mut md = HashMap::new();
            md.insert(DTYPE_META_KEY.to_string(), spec.dtype.as_token().to_string());
            if let Some(u) = &spec.unit {
                md.insert(UNIT_META_KEY.to_string(), u.clone());
            }
            if let Some(d) = &spec.dimension {
                md.insert(DIM_META_KEY.to_string(), d.clone());
            }
            fields.push(Field::new(spec.name.as_str(), dt, true).with_metadata(md));
            arrays.push(arr);
        }
        let schema = Arc::new(Schema::new(fields));
        RecordBatch::try_new(schema, arrays).map_err(|e| DataError::Arrow(e.to_string()))
    }

    fn write_parquet_bytes(frame: &Frame) -> Result<Vec<u8>> {
        let batch = frame_to_record_batch(frame)?;
        let props = WriterProperties::builder()
            .set_compression(Compression::UNCOMPRESSED)
            .build();
        let mut writer = ArrowWriter::try_new(Vec::<u8>::new(), batch.schema(), Some(props))
            .map_err(|e| DataError::Parquet(e.to_string()))?;
        writer.write(&batch).map_err(|e| DataError::Parquet(e.to_string()))?;
        // `into_inner` finalizes the Parquet footer and returns the buffer.
        writer.into_inner().map_err(|e| DataError::Parquet(e.to_string()))
    }

    fn dtype_from_arrow(dt: &DataType) -> Option<ColumnDtype> {
        match dt {
            DataType::Float64 => Some(ColumnDtype::F64),
            DataType::Int64 => Some(ColumnDtype::I64),
            DataType::Boolean => Some(ColumnDtype::Bool),
            DataType::Utf8 => Some(ColumnDtype::Str),
            _ => None,
        }
    }

    fn read_column(batches: &[RecordBatch], ci: usize, dtype: ColumnDtype) -> Result<ColumnData> {
        macro_rules! gather {
            ($arr_ty:ty, $variant:path, $conv:expr) => {{
                let mut out = Vec::new();
                for b in batches {
                    let a = b
                        .column(ci)
                        .as_any()
                        .downcast_ref::<$arr_ty>()
                        .ok_or_else(|| {
                            DataError::Arrow(format!(
                                "column {ci} is not {}",
                                stringify!($arr_ty)
                            ))
                        })?;
                    for i in 0..a.len() {
                        out.push(if a.is_null(i) { None } else { Some($conv(a, i)) });
                    }
                }
                Ok($variant(out))
            }};
        }
        match dtype {
            ColumnDtype::F64 => gather!(Float64Array, ColumnData::F64, |a: &Float64Array, i| a.value(i)),
            ColumnDtype::I64 => gather!(Int64Array, ColumnData::I64, |a: &Int64Array, i| a.value(i)),
            ColumnDtype::Bool => gather!(BooleanArray, ColumnData::Bool, |a: &BooleanArray, i| a.value(i)),
            ColumnDtype::Str => {
                gather!(StringArray, ColumnData::Str, |a: &StringArray, i| a.value(i).to_string())
            }
        }
    }

    fn frame_from_arrow(schema: &Schema, batches: &[RecordBatch]) -> Result<Frame> {
        let mut columns = Vec::with_capacity(schema.fields().len());
        for (ci, field) in schema.fields().iter().enumerate() {
            let unit = field.metadata().get(UNIT_META_KEY).cloned();
            let dimension = field.metadata().get(DIM_META_KEY).cloned();
            let dtype = field
                .metadata()
                .get(DTYPE_META_KEY)
                .and_then(|s| ColumnDtype::from_token(s))
                .or_else(|| dtype_from_arrow(field.data_type()))
                .ok_or_else(|| {
                    DataError::Schema(format!(
                        "column `{}` has unsupported type {:?}",
                        field.name(),
                        field.data_type()
                    ))
                })?;
            let data = read_column(batches, ci, dtype)?;
            columns.push((
                ColumnSpec { name: field.name().clone(), dtype, unit, dimension },
                data,
            ));
        }
        let n_rows = batches.iter().map(|b| b.num_rows()).sum();
        Ok(Frame { columns, n_rows })
    }

    fn read_frame_from<R: ChunkReader + 'static>(r: R) -> Result<Frame> {
        let builder =
            ParquetRecordBatchReaderBuilder::try_new(r).map_err(|e| DataError::Parquet(e.to_string()))?;
        // Capture the schema up front so a 0-row file still round-trips its
        // column headers (the reader yields no batches for an empty file).
        let schema = builder.schema().clone();
        let reader = builder.build().map_err(|e| DataError::Parquet(e.to_string()))?;
        let mut batches = Vec::new();
        for b in reader {
            batches.push(b.map_err(|e| DataError::Arrow(e.to_string()))?);
        }
        frame_from_arrow(&schema, &batches)
    }

    fn concat_frames(frames: Vec<Frame>) -> Result<Frame> {
        let mut it = frames.into_iter();
        let Some(mut acc) = it.next() else {
            return Ok(Frame { columns: Vec::new(), n_rows: 0 });
        };
        for f in it {
            if f.columns.len() != acc.columns.len() {
                return Err(DataError::Schema(
                    "cannot concatenate chunks with differing column counts".into(),
                ));
            }
            for (i, (spec, data)) in f.columns.into_iter().enumerate() {
                let (acc_spec, acc_data) = &mut acc.columns[i];
                if acc_spec.name != spec.name || acc_spec.dtype != spec.dtype {
                    return Err(DataError::Schema(format!(
                        "chunk column `{}` does not match `{}`",
                        spec.name, acc_spec.name
                    )));
                }
                match (acc_data, data) {
                    (ColumnData::F64(a), ColumnData::F64(b)) => a.extend(b),
                    (ColumnData::I64(a), ColumnData::I64(b)) => a.extend(b),
                    (ColumnData::Bool(a), ColumnData::Bool(b)) => a.extend(b),
                    (ColumnData::Str(a), ColumnData::Str(b)) => a.extend(b),
                    _ => {
                        return Err(DataError::Schema(
                            "chunk column data variants disagree".into(),
                        ))
                    }
                }
            }
            acc.n_rows += f.n_rows;
        }
        Ok(acc)
    }

    /// Write a [`Frame`] to a Parquet file (UNCOMPRESSED, unit symbols carried
    /// in field metadata).
    pub fn write_parquet(path: &Path, frame: &Frame) -> Result<()> {
        std::fs::write(path, write_parquet_bytes(frame)?)?;
        Ok(())
    }

    /// Read a Parquet file written by [`write_parquet`] back into a [`Frame`],
    /// recovering dtypes and unit symbols.
    pub fn read_parquet(path: &Path) -> Result<Frame> {
        read_frame_from(File::open(path)?)
    }

    /// Slice a [`Frame`] into row-ranges of at most `target_rows`, each a
    /// self-describing Parquet [`Chunk`]. A zero-row frame yields one empty
    /// chunk so its schema is preserved.
    pub fn frame_to_chunks(frame: &Frame, target_rows: usize) -> Result<Vec<Chunk>> {
        let target = target_rows.max(1);
        if frame.n_rows == 0 {
            return Ok(vec![Chunk { bytes: write_parquet_bytes(frame)?, n_rows: 0 }]);
        }
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < frame.n_rows {
            let len = target.min(frame.n_rows - start);
            let slice = frame.slice_rows(start, len);
            chunks.push(Chunk { bytes: write_parquet_bytes(&slice)?, n_rows: len });
            start += len;
        }
        Ok(chunks)
    }

    /// Reassemble [`Chunk`]s produced by [`frame_to_chunks`] into one [`Frame`].
    pub fn chunks_to_frame(chunks: Vec<Chunk>) -> Result<Frame> {
        let mut frames = Vec::with_capacity(chunks.len());
        for c in chunks {
            frames.push(read_frame_from(bytes::Bytes::from(c.bytes))?);
        }
        concat_frames(frames)
    }
}

#[cfg(feature = "parquet")]
pub use pq::{chunks_to_frame, frame_to_chunks, read_parquet, write_parquet, Chunk};

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_from_columns_validates_ragged() {
        let err = frame_from_columns(vec![
            (ColumnSpec::new("a", ColumnDtype::F64), ColumnData::F64(vec![Some(1.0), Some(2.0)])),
            (ColumnSpec::new("b", ColumnDtype::I64), ColumnData::I64(vec![Some(1)])),
        ]);
        assert!(matches!(err, Err(DataError::Schema(_))), "expected ragged-frame error");
    }

    #[test]
    fn frame_from_columns_rejects_dtype_mismatch() {
        let err = frame_from_columns(vec![(
            ColumnSpec::new("a", ColumnDtype::F64),
            ColumnData::I64(vec![Some(1)]),
        )]);
        assert!(matches!(err, Err(DataError::Schema(_))), "expected dtype-mismatch error");
    }

    #[test]
    fn frame_from_columns_rejects_dupe_names() {
        let err = frame_from_columns(vec![
            (ColumnSpec::new("a", ColumnDtype::F64), ColumnData::F64(vec![Some(1.0)])),
            (ColumnSpec::new("a", ColumnDtype::I64), ColumnData::I64(vec![Some(1)])),
        ]);
        assert!(matches!(err, Err(DataError::Schema(_))), "expected duplicate-name error");
    }
}

#[cfg(all(test, feature = "parquet"))]
mod parquet_tests {
    use super::*;

    fn sample_frame() -> Frame {
        frame_from_columns(vec![
            (
                ColumnSpec::new("t", ColumnDtype::F64).with_unit("s").with_dimension("si:T1"),
                ColumnData::F64(vec![Some(0.0), Some(0.5), None, Some(1.5)]),
            ),
            (
                ColumnSpec::new("count", ColumnDtype::I64),
                ColumnData::I64(vec![Some(10), Some(20), Some(30), None]),
            ),
            (
                ColumnSpec::new("ok", ColumnDtype::Bool),
                ColumnData::Bool(vec![Some(true), None, Some(false), Some(true)]),
            ),
            (
                ColumnSpec::new("label", ColumnDtype::Str),
                ColumnData::Str(vec![Some("a".into()), Some("b".into()), None, Some("d".into())]),
            ),
        ])
        .unwrap()
    }

    #[test]
    fn parquet_round_trips_values_units_and_nulls() {
        let frame = sample_frame();
        let dir = std::env::temp_dir().join("eustress_data_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("roundtrip.parquet");
        write_parquet(&path, &frame).unwrap();
        let back = read_parquet(&path).unwrap();
        assert_eq!(back, frame, "parquet round-trip changed the frame");
        // Unit metadata survived.
        let t = back.specs().find(|s| s.name == "t").unwrap();
        assert_eq!(t.unit.as_deref(), Some("s"));
        assert_eq!(t.dimension.as_deref(), Some("si:T1"));
    }

    #[test]
    fn chunks_round_trip_reassembles_frame() {
        let frame = sample_frame();
        let chunks = frame_to_chunks(&frame, 2).unwrap();
        assert_eq!(chunks.len(), 2, "4 rows at target 2 → 2 chunks");
        assert_eq!(chunks.iter().map(|c| c.n_rows).sum::<usize>(), frame.n_rows());
        let back = chunks_to_frame(chunks).unwrap();
        assert_eq!(back, frame, "chunk reassembly changed the frame");
    }

    #[test]
    fn zero_row_frame_round_trips_schema_and_units() {
        // The load-bearing empty-frame path: a 0-row Parquet file carries no
        // batches, so the schema (column headers + unit metadata) must be
        // recovered from the file footer, not from the (absent) data.
        let frame = frame_from_columns(vec![
            (ColumnSpec::new("t", ColumnDtype::F64).with_unit("s"), ColumnData::F64(vec![])),
            (ColumnSpec::new("label", ColumnDtype::Str), ColumnData::Str(vec![])),
        ])
        .unwrap();
        assert_eq!(frame.n_rows(), 0);

        let dir = std::env::temp_dir().join("eustress_data_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.parquet");
        write_parquet(&path, &frame).unwrap();
        assert_eq!(read_parquet(&path).unwrap(), frame, "0-row parquet round-trip dropped schema/units");

        let chunks = frame_to_chunks(&frame, 4).unwrap();
        assert_eq!(chunks.len(), 1, "0-row frame → one schema-preserving chunk");
        assert_eq!(chunks[0].n_rows, 0);
        assert_eq!(chunks_to_frame(chunks).unwrap(), frame, "0-row chunk round-trip dropped schema/units");
    }
}

//! P1d round-trip, end-to-end against a REAL `FjallWorldDb`.
//!
//! This is the same materialize/load logic as the engine's
//! `space::dataset_store` bridge, but standalone (no bevy/engine) so it runs in
//! seconds: a Series' columns (an `eustress-data` `Frame`) → a Parquet blob →
//! the WorldDb `datasets` partition on disk → blob → `Frame`, asserting the
//! schema, units, dimensions, and nulls all survive. Also exercises the
//! `timeseries` partition's append + time-range scan (P1c).
//!
//! `eustress-worlddb` is a dev-dependency only; it never enters the shipped
//! graph (D2). It does not depend on `eustress-data`, so there is no cycle.

use eustress_data::{
    chunks_to_frame, frame_from_columns, frame_to_chunks, Chunk, ColumnData, ColumnDtype, ColumnSpec,
};
use eustress_worlddb::{FjallWorldDb, WorldDb};

fn sample_series() -> eustress_data::Frame {
    frame_from_columns(vec![
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
        (
            ColumnSpec::new("ok", ColumnDtype::Bool),
            ColumnData::Bool(vec![Some(true), Some(false), None, Some(true)]),
        ),
        (
            ColumnSpec::new("label", ColumnDtype::Str),
            ColumnData::Str(vec![Some("a".into()), None, Some("c".into()), Some("d".into())]),
        ),
    ])
    .unwrap()
}

fn fresh_db(name: &str) -> (FjallWorldDb, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = FjallWorldDb::open(&dir).expect("open FjallWorldDb");
    (db, dir)
}

#[test]
fn series_materializes_to_datasets_blob_and_loads_back() {
    let frame = sample_series();
    let (db, _dir) = fresh_db("eustress_p1d_roundtrip_datasets");

    let id = [9u8; 16];
    assert!(
        db.get_dataset_chunk(&id).unwrap().is_none(),
        "id must be absent before write"
    );

    // MATERIALIZE: Frame -> one self-describing Parquet blob -> datasets partition.
    let blob = frame_to_chunks(&frame, usize::MAX)
        .unwrap()
        .into_iter()
        .next()
        .expect("non-empty chunk set")
        .bytes;
    db.put_dataset_chunk(&id, &blob).unwrap();

    // LOAD: datasets partition -> blob -> Frame.
    let bytes = db
        .get_dataset_chunk(&id)
        .unwrap()
        .expect("dataset present after materialize");
    let back = chunks_to_frame(vec![Chunk { bytes, n_rows: 0 }]).unwrap();

    assert_eq!(
        back, frame,
        "Series -> datasets blob -> Series round-trip changed the frame"
    );
    assert_eq!(db.iter_dataset_chunks().unwrap().len(), 1);

    // Unit + dimension metadata survived the blob round-trip.
    let psi = back.specs().find(|s| s.name == "psi").unwrap();
    assert_eq!(psi.unit.as_deref(), Some("psi"));
    assert_eq!(psi.dimension.as_deref(), Some("si:M1L-1T-2"));

    println!(
        "P1D DATASETS ROUND-TRIP OK: {} rows x {} cols, units+dims preserved",
        back.n_rows(),
        back.n_cols()
    );
}

#[test]
fn timeseries_append_and_time_range_scan() {
    let (db, _dir) = fresh_db("eustress_p1d_roundtrip_timeseries");

    // Append out of order; the range scan must return ascending by (ts, seq).
    db.ts_append("sensor.psi", 2000, 0, b"second").unwrap();
    db.ts_append("sensor.psi", 1000, 0, b"first").unwrap();
    db.ts_append("sensor.psi", 1000, 1, b"first.b").unwrap();
    db.ts_append("other.series", 1500, 0, b"elsewhere").unwrap();

    let rows = db.ts_range("sensor.psi", 0, 5000).unwrap();
    let got: Vec<(u64, u32, &[u8])> = rows.iter().map(|(t, s, v)| (*t, *s, v.as_slice())).collect();
    assert_eq!(
        got,
        vec![
            (1000, 0, b"first".as_slice()),
            (1000, 1, b"first.b".as_slice()),
            (2000, 0, b"second".as_slice()),
        ],
        "timeseries range must be the in-window rows of ONLY this series, ascending"
    );

    // A narrower window excludes out-of-range rows.
    assert_eq!(db.ts_range("sensor.psi", 1500, 5000).unwrap().len(), 1);

    println!("P1C TIMESERIES RANGE OK: {} rows, ascending, series-scoped", rows.len());
}

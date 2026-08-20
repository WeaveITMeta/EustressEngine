//! Export a real CSV as an auditable HuggingFace dataset.
//!
//! ```text
//! cargo run -p eustress-data --features import,parquet,export \
//!   --example export_demo -- <input.csv> <out-dir>
//! ```
//!
//! Produces parquet shards, `manifest.json`, and a data card generated from the
//! manifest. Each CSV row becomes one content-addressed record, so the corpus
//! can answer "was this row used, and who holds the rights" after the fact.

use std::path::PathBuf;

use eustress_data::export::{export_huggingface, CardMeta, ExportOptions};
use eustress_data::provenance::{ManifestBuilder, Source};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = PathBuf::from(args.next().ok_or("usage: export_demo <input.csv> <out-dir>")?);
    let out = PathBuf::from(args.next().ok_or("usage: export_demo <input.csv> <out-dir>")?);

    let frame = eustress_data::import::frame_from_csv(std::fs::File::open(&input)?)?;
    println!("read {} rows x {} cols from {}", frame.n_rows(), frame.n_cols(), input.display());

    // Rights for this corpus. NASA GISTEMP is US-government work and therefore
    // public domain, which is exactly the kind of thing that should be stated
    // rather than assumed.
    let mut builder = ManifestBuilder::new("global-temperature-annual").source(
        Source::new("nasa-gistemp", "NASA Goddard Institute for Space Studies", "US-PD")
            .with_tier("reference")
            .with_payout_ref("public-domain")
            .with_terms(
                "GISTEMP v4, GLB.Ts+dSST. US Government work, no rights reserved. \
                 Cite NASA GISS when redistributing.",
            ),
    );

    // One record per row. The record text is the row rendered canonically, so a
    // reader can reproduce the hash from the published parquet.
    let cols = frame.columns();
    for r in 0..frame.n_rows() {
        let row = cols
            .iter()
            .map(|(spec, data)| format!("{}={}", spec.name, cell(data, r)))
            .collect::<Vec<_>>()
            .join(",");
        builder.record(&row, "nasa-gistemp");
    }
    builder.transform("csv_ingest", Some(input.display().to_string()), frame.n_rows(), builder.len());

    let manifest = builder.build();
    let summary = export_huggingface(
        &out,
        &frame,
        &manifest,
        &CardMeta {
            pretty_name: "Global Temperature Annual (GISTEMP v4)".into(),
            description: "Annual global land-ocean temperature index, anomalies against the \
                          1951-1980 mean. Exported with per-record provenance so membership \
                          and rights are verifiable by the reader."
                .into(),
            language: vec!["en".into()],
            tags: vec!["climate".into(), "time-series".into()],
            task_categories: vec!["tabular-regression".into()],
        },
        &ExportOptions::default(),
    )?;

    println!("\nwrote {}", out.display());
    println!("  shards      : {}", summary.shards);
    println!("  rows        : {}", summary.rows);
    println!("  records     : {}", summary.records);
    println!("  merkle root : {}", summary.merkle_root);
    Ok(())
}

fn cell(data: &eustress_data::ColumnData, r: usize) -> String {
    use eustress_data::ColumnData as C;
    match data {
        C::F64(v) => v.get(r).and_then(|o| *o).map(|x| x.to_string()).unwrap_or_default(),
        C::I64(v) => v.get(r).and_then(|o| *o).map(|x| x.to_string()).unwrap_or_default(),
        C::Bool(v) => v.get(r).and_then(|o| *o).map(|x| x.to_string()).unwrap_or_default(),
        C::Str(v) => v.get(r).and_then(|o| o.clone()).unwrap_or_default(),
    }
}

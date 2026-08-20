//! HuggingFace-shaped export: parquet shards, a data card, and the manifest.
//!
//! Anyone can publish parquet. What almost nobody publishes is a corpus whose
//! claims can be **checked by the person downloading it**: which sources it came
//! from, under what license, in what proportion, and a root hash that changes if
//! a single record is altered.
//!
//! The layout is the standard the Hub's loader expects, so
//! `datasets.load_dataset(...)` works with no special handling:
//!
//! ```text
//! my-corpus/
//!   data/train-00000-of-00002.parquet
//!   data/train-00001-of-00002.parquet
//!   manifest.json     <- sources, per-record lineage, Merkle root
//!   README.md         <- YAML frontmatter + a provenance section
//! ```
//!
//! The card is generated from the manifest rather than written by hand, so it
//! cannot drift from what actually shipped. A hand-written data card is a claim;
//! a derived one is a report.

use std::path::Path;

use crate::provenance::Manifest;
use crate::{Frame, Result};

/// Human-facing fields the manifest cannot infer.
#[derive(Clone, Debug, Default)]
pub struct CardMeta {
    /// Display name on the Hub.
    pub pretty_name: String,
    /// One or two sentences on what the corpus is and how it was collected.
    pub description: String,
    /// BCP-47 codes, e.g. `["en"]`. Omitted from the card when empty.
    pub language: Vec<String>,
    /// Hub tags.
    pub tags: Vec<String>,
    /// Hub task categories, e.g. `["text-generation"]`.
    pub task_categories: Vec<String>,
}

/// Export knobs.
#[derive(Clone, Debug)]
pub struct ExportOptions {
    /// Rows per parquet shard. Shards keep a large corpus streamable and let a
    /// reader fetch part of it.
    pub rows_per_shard: usize,
    /// Split name; `train` unless the corpus is an eval set.
    pub split: String,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { rows_per_shard: 50_000, split: "train".to_string() }
    }
}

/// What was written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportSummary {
    pub shards: usize,
    pub rows: usize,
    pub records: usize,
    pub merkle_root: String,
    /// Sources referenced by a record but never declared. Non-empty means the
    /// corpus contains data nobody can be paid for, so the export refuses.
    pub undeclared_sources: Vec<String>,
}

/// Write a HuggingFace-loadable dataset directory.
///
/// Refuses rather than writes when the manifest fails verification, or when a
/// record cites a source that was never declared. Publishing a corpus whose
/// rights cannot be resolved is the exact failure this whole layer exists to
/// prevent, so it is an error and not a warning.
pub fn export_huggingface(
    dir: &Path,
    frame: &Frame,
    manifest: &Manifest,
    card: &CardMeta,
    opts: &ExportOptions,
) -> Result<ExportSummary> {
    use crate::DataError;

    manifest
        .verify()
        .map_err(|e| DataError::Schema(format!("refusing to export: {e}")))?;

    let undeclared = manifest.undeclared_sources();
    if !undeclared.is_empty() {
        return Err(DataError::Schema(format!(
            "refusing to export: {} record source(s) have no declared rights holder: {}",
            undeclared.len(),
            undeclared.join(", ")
        )));
    }

    let data_dir = dir.join("data");
    std::fs::create_dir_all(&data_dir)?;

    let chunks = crate::frame_to_chunks(frame, opts.rows_per_shard)?;
    let total = chunks.len();
    for (i, chunk) in chunks.iter().enumerate() {
        // The Hub's own shard convention; the loader's glob depends on it.
        let name = format!("{}-{:05}-of-{:05}.parquet", opts.split, i, total);
        std::fs::write(data_dir.join(name), &chunk.bytes)?;
    }

    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(manifest)
            .map_err(|e| DataError::Schema(format!("manifest serialization failed: {e}")))?,
    )?;

    std::fs::write(dir.join("README.md"), render_card(manifest, card, opts, frame.n_rows()))?;

    Ok(ExportSummary {
        shards: total,
        rows: frame.n_rows(),
        records: manifest.records.len(),
        merkle_root: manifest.merkle_root.clone(),
        undeclared_sources: undeclared,
    })
}

/// The single license for the frontmatter, or `None` when sources disagree.
///
/// A mixed-rights corpus must not advertise one license: the Hub's field is
/// singular, so claiming one source's terms for all of them would misstate the
/// rights on every other record. Mixed corpora declare `other` and carry the
/// breakdown in the body.
fn unified_license(manifest: &Manifest) -> Option<String> {
    let mut it = manifest.sources.iter().map(|s| s.license.as_str());
    let first = it.next()?;
    it.all(|l| l == first).then(|| first.to_string())
}

/// HuggingFace's size bucket for the frontmatter.
fn size_category(rows: usize) -> &'static str {
    match rows {
        0..=999 => "n<1K",
        1_000..=9_999 => "1K<n<10K",
        10_000..=99_999 => "10K<n<100K",
        100_000..=999_999 => "100K<n<1M",
        1_000_000..=9_999_999 => "1M<n<10M",
        10_000_000..=99_999_999 => "10M<n<100M",
        _ => "n>100M",
    }
}

/// Escape a value for a single-line YAML scalar.
fn yaml(s: &str) -> String {
    if s.is_empty()
        || s.contains(':')
        || s.contains('#')
        || s.contains('"')
        || s.starts_with(' ')
        || s.ends_with(' ')
    {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Render the data card from the manifest.
fn render_card(manifest: &Manifest, card: &CardMeta, opts: &ExportOptions, rows: usize) -> String {
    let mut s = String::new();
    let license = unified_license(manifest);

    // ── YAML frontmatter ──────────────────────────────────────────────────
    s.push_str("---\n");
    if !card.pretty_name.is_empty() {
        s.push_str(&format!("pretty_name: {}\n", yaml(&card.pretty_name)));
    }
    s.push_str(&format!(
        "license: {}\n",
        yaml(&license.clone().unwrap_or_else(|| "other".to_string()))
    ));
    if !card.language.is_empty() {
        s.push_str("language:\n");
        for l in &card.language {
            s.push_str(&format!("- {}\n", yaml(l)));
        }
    }
    if !card.task_categories.is_empty() {
        s.push_str("task_categories:\n");
        for t in &card.task_categories {
            s.push_str(&format!("- {}\n", yaml(t)));
        }
    }
    s.push_str("tags:\n");
    for t in card.tags.iter().map(String::as_str).chain(["provenance", "auditable"]) {
        s.push_str(&format!("- {}\n", yaml(t)));
    }
    s.push_str(&format!("size_categories:\n- {}\n", size_category(rows)));
    s.push_str("configs:\n- config_name: default\n  data_files:\n");
    s.push_str(&format!(
        "  - split: {}\n    path: data/{}-*.parquet\n",
        opts.split, opts.split
    ));
    s.push_str("---\n\n");

    // ── Body ──────────────────────────────────────────────────────────────
    let title = if card.pretty_name.is_empty() { &manifest.dataset } else { &card.pretty_name };
    s.push_str(&format!("# {title}\n\n"));
    if !card.description.is_empty() {
        s.push_str(&format!("{}\n\n", card.description));
    }

    s.push_str("## Provenance\n\n");
    s.push_str(
        "Every record in this corpus is content-addressed, and every source \
         carries the rights it was supplied under. The claims below are derived \
         from `manifest.json`, not written by hand, so they cannot drift from \
         what actually shipped.\n\n",
    );
    s.push_str(&format!("- **Records:** {}\n", manifest.records.len()));
    s.push_str(&format!("- **Rows:** {rows}\n"));
    s.push_str(&format!("- **Sources:** {}\n", manifest.sources.len()));
    s.push_str(&format!("- **Merkle root:** `{}`\n\n", manifest.merkle_root));

    if license.is_none() && !manifest.sources.is_empty() {
        s.push_str(
            "> This corpus carries **mixed licenses**. The frontmatter declares \
             `other`; the per-source terms in the table below are authoritative, \
             and any reuse must satisfy all of them.\n\n",
        );
    }

    // ── Sources ───────────────────────────────────────────────────────────
    if !manifest.sources.is_empty() {
        let volumes = manifest.volume_by_source();
        let shares = manifest.volume_share();
        s.push_str("### Sources\n\n");
        s.push_str("| Source | Rights holder | License | Tier | Records | Share |\n");
        s.push_str("|---|---|---|---|---:|---:|\n");
        for src in &manifest.sources {
            let v = volumes.get(&src.id).cloned().unwrap_or_default();
            let share = shares.get(&src.id).copied().unwrap_or(0.0);
            s.push_str(&format!(
                "| `{}` | {} | {} | {} | {} | {:.1}% |\n",
                src.id,
                src.holder,
                src.license,
                src.tier.as_deref().unwrap_or("-"),
                v.records,
                share * 100.0,
            ));
        }
        s.push('\n');
        s.push_str(
            "Share is each source's proportion of the corpus **by bytes**. It is \
             the split within a quality tier, not a statement of value: volume \
             alone rewards bulk, so what a tier is worth is measured separately \
             by ablation.\n\n",
        );

        let with_terms: Vec<_> = manifest.sources.iter().filter(|s| s.terms.is_some()).collect();
        if !with_terms.is_empty() {
            s.push_str("### Additional terms\n\n");
            for src in with_terms {
                s.push_str(&format!(
                    "- **{}**: {}\n",
                    src.holder,
                    src.terms.as_deref().unwrap_or("")
                ));
            }
            s.push('\n');
        }
    }

    // ── Lineage ───────────────────────────────────────────────────────────
    if !manifest.transforms.is_empty() {
        s.push_str("### How this corpus was derived\n\n");
        s.push_str("| Step | Operation | In | Out | Detail |\n|---:|---|---:|---:|---|\n");
        for (i, t) in manifest.transforms.iter().enumerate() {
            s.push_str(&format!(
                "| {} | `{}` | {} | {} | {} |\n",
                i + 1,
                t.op,
                t.records_in,
                t.records_out,
                t.detail.as_deref().unwrap_or("")
            ));
        }
        s.push('\n');
    }

    // ── Verification ──────────────────────────────────────────────────────
    s.push_str("### Verifying this corpus\n\n");
    s.push_str(
        "`manifest.json` lists every record as the SHA-256 of its normalized \
         content (NFC, canonical line endings, trimmed trailing whitespace). \
         Those hashes roll into the Merkle root above over the sorted, deduplicated \
         set, so the root describes the contents rather than the order they were \
         assembled in.\n\n",
    );
    s.push_str("Two checks anyone can run without trusting the publisher:\n\n");
    s.push_str(
        "1. **Integrity.** Recompute the root from the record hashes. If a record \
         was added, removed, or altered after publication, it will not match.\n",
    );
    s.push_str(
        "2. **Membership.** Normalize and hash any document, then look for that \
         hash in the manifest. That answers whether a specific text is in this \
         corpus, which is what an eval-set contamination check needs.\n\n",
    );
    s.push_str(
        "The root is unsigned. It is tamper-evidence, not attestation: it shows \
         the corpus has not changed since the manifest was written, and says \
         nothing about who wrote the manifest.\n\n",
    );

    s.push_str("### Attribution and payment\n\n");
    s.push_str(
        "Sources carry a `payout_ref`, a contributor handle used to route \
         payment. No credential is stored in the manifest, and there is no field \
         that could hold one.\n",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provenance::{ManifestBuilder, Source};
    use crate::{frame_from_columns, ColumnData, ColumnDtype, ColumnSpec};

    fn frame(n: usize) -> Frame {
        let vals: Vec<Option<f64>> = (0..n).map(|i| Some(i as f64)).collect();
        let names: Vec<Option<String>> = (0..n).map(|i| Some(format!("row {i}"))).collect();
        frame_from_columns(vec![
            (
                ColumnSpec::new("value", ColumnDtype::F64).with_unit("C"),
                ColumnData::F64(vals),
            ),
            (ColumnSpec::new("text", ColumnDtype::Str), ColumnData::Str(names)),
        ])
        .unwrap()
    }

    fn manifest_with(n: usize) -> Manifest {
        let mut b = ManifestBuilder::new("demo-corpus").source(
            Source::new("acme", "Acme Archive", "CC-BY-4.0")
                .with_payout_ref("bliss:acme")
                .with_tier("curated"),
        );
        for i in 0..n {
            b.record(&format!("row {i}"), "acme");
        }
        b.transform("dedup", Some("exact duplicates".into()), n + 2, n);
        b.build()
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("eustress-export-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn it_writes_the_layout_the_hub_loader_expects() {
        let dir = tmp("layout");
        let out = export_huggingface(
            &dir,
            &frame(10),
            &manifest_with(10),
            &CardMeta { pretty_name: "Demo Corpus".into(), ..Default::default() },
            &ExportOptions::default(),
        )
        .unwrap();

        assert_eq!(out.shards, 1);
        assert_eq!(out.rows, 10);
        assert!(dir.join("README.md").is_file());
        assert!(dir.join("manifest.json").is_file());
        assert!(dir.join("data/train-00000-of-00001.parquet").is_file());
    }

    #[test]
    fn shards_follow_the_hub_naming_convention() {
        let dir = tmp("shards");
        let opts = ExportOptions { rows_per_shard: 4, ..Default::default() };
        let out = export_huggingface(
            &dir,
            &frame(10),
            &manifest_with(10),
            &CardMeta::default(),
            &opts,
        )
        .unwrap();

        assert_eq!(out.shards, 3, "10 rows at 4 per shard");
        for i in 0..3 {
            let p = dir.join(format!("data/train-{i:05}-of-00003.parquet"));
            assert!(p.is_file(), "missing {p:?}");
        }
    }

    #[test]
    fn the_shards_read_back_as_the_original_frame() {
        let dir = tmp("roundtrip");
        let original = frame(9);
        let opts = ExportOptions { rows_per_shard: 4, ..Default::default() };
        export_huggingface(&dir, &original, &manifest_with(9), &CardMeta::default(), &opts).unwrap();

        let mut chunks = Vec::new();
        for i in 0..3 {
            let bytes =
                std::fs::read(dir.join(format!("data/train-{i:05}-of-00003.parquet"))).unwrap();
            chunks.push(crate::Chunk { n_rows: 0, bytes });
        }
        let restored = crate::chunks_to_frame(chunks).unwrap();
        assert_eq!(restored, original, "exported data must survive the round-trip");
    }

    #[test]
    fn the_manifest_ships_and_still_verifies_after_export() {
        let dir = tmp("manifest");
        let m = manifest_with(5);
        export_huggingface(&dir, &frame(5), &m, &CardMeta::default(), &ExportOptions::default())
            .unwrap();

        let json = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
        assert!(back.verify().is_ok());
    }

    #[test]
    fn the_card_carries_the_frontmatter_the_loader_needs() {
        let dir = tmp("card");
        export_huggingface(
            &dir,
            &frame(3),
            &manifest_with(3),
            &CardMeta {
                pretty_name: "Demo Corpus".into(),
                description: "A small demo.".into(),
                language: vec!["en".into()],
                task_categories: vec!["text-generation".into()],
                ..Default::default()
            },
            &ExportOptions::default(),
        )
        .unwrap();

        let card = std::fs::read_to_string(dir.join("README.md")).unwrap();
        assert!(card.starts_with("---\n"), "frontmatter must lead the file");
        assert!(card.contains("license: cc-by-4.0") || card.contains("license: CC-BY-4.0"));
        assert!(card.contains("pretty_name: Demo Corpus"));
        assert!(card.contains("- split: train"));
        assert!(card.contains("path: data/train-*.parquet"), "the loader globs this");
        assert!(card.contains("size_categories:\n- n<1K"));
    }

    #[test]
    fn the_card_reports_sources_shares_and_the_root() {
        let dir = tmp("sources");
        let m = manifest_with(4);
        export_huggingface(&dir, &frame(4), &m, &CardMeta::default(), &ExportOptions::default())
            .unwrap();

        let card = std::fs::read_to_string(dir.join("README.md")).unwrap();
        assert!(card.contains("Acme Archive"));
        assert!(card.contains("CC-BY-4.0"));
        assert!(card.contains("curated"), "tier is disclosed");
        assert!(card.contains(&m.merkle_root), "the root is published in the card");
        assert!(card.contains("100.0%"), "a single source holds the whole share");
        assert!(card.contains("dedup"), "lineage is disclosed");
    }

    #[test]
    fn a_mixed_license_corpus_never_advertises_a_single_license() {
        let dir = tmp("mixed");
        let mut b = ManifestBuilder::new("mixed")
            .source(Source::new("a", "A", "CC-BY-4.0"))
            .source(Source::new("b", "B", "CC-BY-NC-4.0"));
        b.record("one", "a");
        b.record("two", "b");
        let m = b.build();

        export_huggingface(&dir, &frame(2), &m, &CardMeta::default(), &ExportOptions::default())
            .unwrap();
        let card = std::fs::read_to_string(dir.join("README.md")).unwrap();

        // Claiming one source's terms for all of them would misstate the rights
        // on every other record.
        assert!(card.contains("license: other"));
        assert!(card.contains("mixed licenses"));
        assert!(card.contains("CC-BY-NC-4.0"), "both licenses are disclosed in the table");
    }

    #[test]
    fn export_refuses_a_corpus_whose_rights_cannot_be_resolved() {
        let dir = tmp("orphan");
        let mut b = ManifestBuilder::new("orphan").source(Source::new("a", "A", "MIT"));
        b.record("declared", "a");
        b.record("undeclared", "ghost");
        let m = b.build();

        let err = export_huggingface(
            &dir,
            &frame(2),
            &m,
            &CardMeta::default(),
            &ExportOptions::default(),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("ghost"), "the offending source is named");
        assert!(!dir.join("README.md").exists(), "nothing is written on refusal");
    }

    #[test]
    fn export_refuses_a_tampered_manifest() {
        let dir = tmp("tampered");
        let mut m = manifest_with(3);
        m.records.pop(); // root no longer matches

        let err = export_huggingface(
            &dir,
            &frame(3),
            &m,
            &CardMeta::default(),
            &ExportOptions::default(),
        )
        .unwrap_err();
        assert!(format!("{err}").contains("root mismatch"));
    }

    #[test]
    fn the_card_states_that_the_root_is_unsigned() {
        let dir = tmp("honest");
        export_huggingface(
            &dir,
            &frame(2),
            &manifest_with(2),
            &CardMeta::default(),
            &ExportOptions::default(),
        )
        .unwrap();
        let card = std::fs::read_to_string(dir.join("README.md")).unwrap();
        // Overstating what the root proves would be the easiest way to mislead.
        assert!(card.contains("unsigned"));
        assert!(card.contains("tamper-evidence, not attestation"));
    }

    #[test]
    fn yaml_values_needing_quotes_get_them() {
        assert_eq!(yaml("plain"), "plain");
        assert_eq!(yaml("has: colon"), "\"has: colon\"");
        assert_eq!(yaml(""), "\"\"");
    }
}

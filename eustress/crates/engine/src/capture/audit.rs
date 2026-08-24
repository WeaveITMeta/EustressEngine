//! The on-disk audit trail: one directory per opportunity, readable with `cat`.
//!
//! The requirement is stricter than "we log things". An auditor with no engine,
//! no account, and no database must be able to open a folder and reconstruct
//! why a firm bid on one solicitation and passed on another. That rules out
//! anything that lives only in a binary store.
//!
//! Layout, under the Space root:
//!
//! ```text
//! Capture/
//!   sam-abc123/
//!     opportunity.json   normalized notice, as screened
//!     source.json        the raw API payload, unmodified
//!     screen.json        every gate, pass or fail, with its reason
//!     score.json         every criterion, its weight, and its evidence
//!     compliance.md      the requirement matrix
//!     compliance.csv     the same, for a spreadsheet
//!     draft.md           the seeded response
//!     manifest.json      content hashes and the Merkle root over all of it
//! ```
//!
//! `manifest.json` is what makes the folder tamper-evident: altering any file
//! changes its record hash, which changes the root. That is evidence of
//! internal consistency, not attestation of authorship. See
//! [`eustress_data::provenance`] for the distinction, which matters and is easy
//! to overstate.

use std::path::{Path, PathBuf};

use eustress_data::provenance::{ManifestBuilder, Source};
use serde::{Deserialize, Serialize};

use super::compliance::ComplianceMatrix;
use super::model::Opportunity;
use super::outline::ResponseOutline;
use super::screen::ScreenResult;
use super::score::FitScore;
use super::CaptureError;

/// Directory name for the capture trail inside a Space.
pub const CAPTURE_DIR: &str = "Capture";

/// Everything recorded about one opportunity in one decision cycle.
#[derive(Debug, Clone)]
pub struct CaptureRecord<'a> {
    pub opportunity: &'a Opportunity,
    /// The raw payload the notice was parsed from, verbatim.
    pub raw_source: Option<&'a str>,
    pub screen: Option<&'a ScreenResult>,
    pub score: Option<&'a FitScore>,
    pub matrix: Option<&'a ComplianceMatrix>,
    pub outline: Option<&'a ResponseOutline>,
    /// When this was fetched, RFC 3339. Passed in rather than read from the
    /// clock so a write is reproducible and testable.
    pub fetched_at: String,
}

impl<'a> CaptureRecord<'a> {
    /// A record holding only the notice itself.
    pub fn new(opportunity: &'a Opportunity, fetched_at: impl Into<String>) -> Self {
        Self {
            opportunity,
            raw_source: None,
            screen: None,
            score: None,
            matrix: None,
            outline: None,
            fetched_at: fetched_at.into(),
        }
    }
}

/// What a write produced, so a caller can report it without re-reading disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteReceipt {
    pub directory: PathBuf,
    /// File names written, in write order.
    pub files: Vec<String>,
    /// Merkle root over every file's content, hex.
    pub merkle_root: String,
}

/// Write one opportunity's full trail, returning the receipt.
///
/// Overwrites in place. A capture folder is the current state of a decision,
/// not an append-only log: the op log carries the history, and a directory that
/// grew a new copy on every poll would be unreadable within a week.
pub fn write_record(space_root: &Path, record: &CaptureRecord<'_>) -> Result<WriteReceipt, CaptureError> {
    let dir = space_root.join(CAPTURE_DIR).join(record.opportunity.slug());
    std::fs::create_dir_all(&dir).map_err(|e| CaptureError::Io(format!("{}: {e}", dir.display())))?;

    // `source` names the rights and origin of everything in this folder. The
    // license is the public-domain status of a US Government work product,
    // which is the actual legal position and worth stating rather than leaving
    // a reader to assume.
    let source = Source::new(
        record.opportunity.source.as_str(),
        format!("US Government ({})", record.opportunity.agency),
        "US Government work, 17 USC 105",
    )
    .with_terms(record.opportunity.url.clone());
    let mut manifest = ManifestBuilder::new(format!("capture/{}", record.opportunity.slug()))
        .source(source);

    let mut files = Vec::new();
    let source_id = record.opportunity.source.as_str();

    let opportunity_json = serde_json::to_string_pretty(record.opportunity)
        .map_err(|e| CaptureError::Io(format!("serializing opportunity: {e}")))?;
    write_file(&dir, "opportunity.json", &opportunity_json, &mut files)?;
    manifest.record(&opportunity_json, source_id);

    if let Some(raw) = record.raw_source {
        write_file(&dir, "source.json", raw, &mut files)?;
        manifest.record(raw, source_id);
    }
    if let Some(s) = record.screen {
        let json = serde_json::to_string_pretty(s)
            .map_err(|e| CaptureError::Io(format!("serializing screen: {e}")))?;
        write_file(&dir, "screen.json", &json, &mut files)?;
        manifest.record(&json, source_id);
    }
    if let Some(s) = record.score {
        let json = serde_json::to_string_pretty(s)
            .map_err(|e| CaptureError::Io(format!("serializing score: {e}")))?;
        write_file(&dir, "score.json", &json, &mut files)?;
        manifest.record(&json, source_id);
        // The human-readable derivation next to the machine-readable one, so
        // the folder answers "why this score" without a JSON viewer.
        let explain = s.explain();
        write_file(&dir, "score.txt", &explain, &mut files)?;
        manifest.record(&explain, source_id);
    }
    if let Some(m) = record.matrix {
        let md = m.to_markdown();
        write_file(&dir, "compliance.md", &md, &mut files)?;
        manifest.record(&md, source_id);
        let csv = m.to_csv();
        write_file(&dir, "compliance.csv", &csv, &mut files)?;
        manifest.record(&csv, source_id);
    }
    if let Some(o) = record.outline {
        let md = o.to_markdown();
        write_file(&dir, "draft.md", &md, &mut files)?;
        manifest.record(&md, source_id);
    }

    let built = manifest.build();
    let root = hex_root(&built);
    let manifest_json = serde_json::to_string_pretty(&ManifestFile {
        notice_id: record.opportunity.notice_id.clone(),
        source: record.opportunity.source.as_str().to_string(),
        url: record.opportunity.url.clone(),
        fetched_at: record.fetched_at.clone(),
        files: files.clone(),
        record_count: built.records.len(),
        merkle_root: root.clone(),
    })
    .map_err(|e| CaptureError::Io(format!("serializing manifest: {e}")))?;
    // The manifest is written last and is NOT one of its own records: a hash
    // cannot cover the file that carries it.
    std::fs::write(dir.join("manifest.json"), &manifest_json)
        .map_err(|e| CaptureError::Io(format!("manifest.json: {e}")))?;
    files.push("manifest.json".to_string());

    Ok(WriteReceipt { directory: dir, files, merkle_root: root })
}

/// The manifest as it appears on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFile {
    pub notice_id: String,
    pub source: String,
    pub url: String,
    pub fetched_at: String,
    pub files: Vec<String>,
    pub record_count: usize,
    pub merkle_root: String,
}

/// Re-derive the Merkle root from a folder's own files and compare it against
/// the recorded one.
///
/// This is the check an auditor runs. It answers "has anything in this folder
/// changed since it was written", and nothing more.
pub fn verify_record(dir: &Path) -> Result<Verification, CaptureError> {
    let manifest_path = dir.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| CaptureError::Io(format!("{}: {e}", manifest_path.display())))?;
    let manifest: ManifestFile = serde_json::from_str(&text)
        .map_err(|e| CaptureError::Parse(format!("manifest.json: {e}")))?;

    let source = Source::new(manifest.source.clone(), "recomputed", "n/a");
    let mut builder =
        ManifestBuilder::new(format!("capture/{}", manifest.notice_id)).source(source);

    let mut missing = Vec::new();
    for name in manifest.files.iter().filter(|f| f.as_str() != "manifest.json") {
        match std::fs::read_to_string(dir.join(name)) {
            Ok(content) => {
                builder.record(&content, &manifest.source);
            }
            Err(_) => missing.push(name.clone()),
        }
    }

    let recomputed = hex_root(&builder.build());
    Ok(Verification {
        intact: missing.is_empty() && recomputed == manifest.merkle_root,
        expected: manifest.merkle_root,
        recomputed,
        missing,
    })
}

/// Result of re-deriving a folder's root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    pub intact: bool,
    pub expected: String,
    pub recomputed: String,
    /// Files the manifest lists that are no longer present.
    pub missing: Vec<String>,
}

impl Verification {
    pub fn explain(&self) -> String {
        if self.intact {
            return format!("intact, root {}", &self.expected[..16.min(self.expected.len())]);
        }
        if !self.missing.is_empty() {
            return format!("ALTERED: {} file(s) missing: {}", self.missing.len(), self.missing.join(", "));
        }
        format!("ALTERED: root {} does not match recorded {}", self.recomputed, self.expected)
    }
}

/// List every capture folder in a Space.
pub fn list_records(space_root: &Path) -> Vec<PathBuf> {
    let dir = space_root.join(CAPTURE_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("manifest.json").is_file())
        .collect();
    out.sort();
    out
}

fn write_file(
    dir: &Path,
    name: &str,
    content: &str,
    files: &mut Vec<String>,
) -> Result<(), CaptureError> {
    std::fs::write(dir.join(name), content)
        .map_err(|e| CaptureError::Io(format!("{name}: {e}")))?;
    files.push(name.to_string());
    Ok(())
}

/// The manifest's Merkle root, already hex-encoded by the provenance layer.
fn hex_root(manifest: &eustress_data::provenance::Manifest) -> String {
    manifest.merkle_root.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::model::{NoticeSource, Opportunity};

    fn temp_dir(name: &str) -> PathBuf {
        // Process id keeps parallel test binaries from colliding, and the test
        // name keeps cases within one binary apart.
        let d = std::env::temp_dir().join(format!("eustress-capture-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    fn opp() -> Opportunity {
        let mut o = Opportunity::empty(NoticeSource::Sam, "abc123");
        o.title = "Network Modernization".into();
        o.agency = "ACC-APG".into();
        o.url = "https://sam.gov/opp/abc123/view".into();
        o
    }

    #[test]
    fn a_record_writes_a_folder_an_auditor_can_read() {
        let root = temp_dir("write");
        let o = opp();
        let rec = CaptureRecord {
            raw_source: Some(r#"{"noticeId":"abc123"}"#),
            ..CaptureRecord::new(&o, "2026-08-23T12:00:00Z")
        };
        let receipt = write_record(&root, &rec).expect("writes");

        assert!(receipt.directory.ends_with("sam-abc123"));
        assert!(receipt.files.contains(&"opportunity.json".to_string()));
        assert!(receipt.files.contains(&"source.json".to_string()));
        assert!(receipt.files.contains(&"manifest.json".to_string()));
        assert_eq!(receipt.merkle_root.len(), 64, "a sha-256 root is 64 hex chars");

        // The raw payload is byte-identical to what came off the wire.
        let raw = std::fs::read_to_string(receipt.directory.join("source.json")).unwrap();
        assert_eq!(raw, r#"{"noticeId":"abc123"}"#);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verification_passes_on_an_untouched_folder() {
        let root = temp_dir("verify-clean");
        let o = opp();
        let rec = CaptureRecord::new(&o, "2026-08-23T12:00:00Z");
        let receipt = write_record(&root, &rec).expect("writes");

        let v = verify_record(&receipt.directory).expect("verifies");
        assert!(v.intact, "{}", v.explain());
        assert_eq!(v.recomputed, receipt.merkle_root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn editing_any_file_breaks_the_root() {
        // This is the whole claim: tamper-evidence, not merely a checksum of
        // one file.
        let root = temp_dir("verify-tampered");
        let o = opp();
        let rec = CaptureRecord::new(&o, "2026-08-23T12:00:00Z");
        let receipt = write_record(&root, &rec).expect("writes");

        let target = receipt.directory.join("opportunity.json");
        let mut content = std::fs::read_to_string(&target).unwrap();
        content = content.replace("Network Modernization", "Something Else Entirely");
        std::fs::write(&target, content).unwrap();

        let v = verify_record(&receipt.directory).expect("verifies");
        assert!(!v.intact, "an edited file must fail verification");
        assert!(v.explain().starts_with("ALTERED"), "{}", v.explain());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn deleting_a_file_is_reported_as_missing_not_as_a_hash_mismatch() {
        let root = temp_dir("verify-missing");
        let o = opp();
        let rec = CaptureRecord {
            raw_source: Some("{}"),
            ..CaptureRecord::new(&o, "2026-08-23T12:00:00Z")
        };
        let receipt = write_record(&root, &rec).expect("writes");
        std::fs::remove_file(receipt.directory.join("source.json")).unwrap();

        let v = verify_record(&receipt.directory).expect("verifies");
        assert!(!v.intact);
        assert_eq!(v.missing, vec!["source.json".to_string()]);
        assert!(v.explain().contains("missing"), "{}", v.explain());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_notice_id_cannot_escape_the_capture_directory() {
        let root = temp_dir("traversal");
        let mut o = opp();
        o.notice_id = "../../../etc/passwd".into();
        let rec = CaptureRecord::new(&o, "2026-08-23T12:00:00Z");
        let receipt = write_record(&root, &rec).expect("writes");

        assert!(
            receipt.directory.starts_with(root.join(CAPTURE_DIR)),
            "a hostile notice id escaped the capture directory: {}",
            receipt.directory.display()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn listing_finds_every_written_record_in_a_stable_order() {
        let root = temp_dir("list");
        for id in ["ccc", "aaa", "bbb"] {
            let mut o = opp();
            o.notice_id = id.into();
            write_record(&root, &CaptureRecord::new(&o, "2026-08-23T12:00:00Z")).expect("writes");
        }
        let found = list_records(&root);
        assert_eq!(found.len(), 3);
        let names: Vec<String> =
            found.iter().map(|p| p.file_name().unwrap().to_string_lossy().into()).collect();
        assert_eq!(names, vec!["sam-aaa", "sam-bbb", "sam-ccc"], "sorted, so a diff is readable");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rewriting_a_record_replaces_it_rather_than_accumulating_copies() {
        let root = temp_dir("rewrite");
        let o = opp();
        let first = write_record(&root, &CaptureRecord::new(&o, "2026-08-23T12:00:00Z")).unwrap();
        let second = write_record(&root, &CaptureRecord::new(&o, "2026-08-24T12:00:00Z")).unwrap();
        assert_eq!(first.directory, second.directory);
        assert_eq!(list_records(&root).len(), 1, "one notice is one folder");
        let _ = std::fs::remove_dir_all(&root);
    }
}

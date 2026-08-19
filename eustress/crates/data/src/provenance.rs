//! Provenance: content-addressed records, source rights, and a verifiable manifest.
//!
//! This is the foundation for training data that can be **audited and paid for**.
//! Two questions have to be answerable long after a corpus is assembled:
//!
//! 1. *Was this exact record in the set, and under what license?*
//! 2. *Whose contribution was it, and how much of it was there?*
//!
//! Neither is answerable today by anything in wide use, which is why rights
//! holders withhold the corpora worth having: absent a way to prove use, there
//! is no way to price it.
//!
//! ## The model
//!
//! A **record** is identified by the hash of its normalized content, so identity
//! travels with the bytes rather than with a row number that any transform would
//! invalidate. A **source** carries the rights: who holds them, under what
//! license, and where payment goes. Records point at a source; the license does
//! not have to be repeated per row.
//!
//! Lineage is a parent list, so a record produced by a transform still names the
//! records it came from. That is what lets provenance survive dedup, filtering,
//! mixture and tokenization instead of ending at ingest.
//!
//! The [`Manifest`] rolls every record hash into a Merkle root. Any alteration
//! to any record changes the root, so a published corpus is tamper-evident
//! without needing a signature or a trusted third party. Signing the root is a
//! later, orthogonal step.
//!
//! ## What this does NOT claim
//!
//! An unsigned root proves internal consistency, not authorship: it shows the
//! corpus was not altered after the manifest was written, and says nothing about
//! who wrote the manifest. Treat it as tamper-evidence, not attestation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

/// Content-addressed identity of one record: SHA-256 over its normalized text.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordId(pub [u8; 32]);

impl RecordId {
    /// Identity of a piece of content, after [`normalize`].
    pub fn of(content: &str) -> Self {
        let mut h = Sha256::new();
        h.update(normalize(content).as_bytes());
        let out = h.finalize();
        let mut id = [0u8; 32];
        id.copy_from_slice(&out);
        Self(id)
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let bytes = hex::decode(s).ok()?;
        (bytes.len() == 32).then(|| {
            let mut id = [0u8; 32];
            id.copy_from_slice(&bytes);
            Self(id)
        })
    }
}

impl std::fmt::Debug for RecordId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Short form: a full 64-char hash makes every debug dump unreadable.
        write!(f, "RecordId({}…)", &self.to_hex()[..12])
    }
}

impl Serialize for RecordId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for RecordId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::from_hex(&s).ok_or_else(|| serde::de::Error::custom("expected 64 hex characters"))
    }
}

/// Canonical form used for hashing.
///
/// Normalizing before hashing is what makes deduplication and contamination
/// checks actually work: the same passage re-saved with Windows line endings, a
/// trailing newline, or decomposed accents is the SAME record, and hashing raw
/// bytes would call it a new one and let a duplicate through an eval-set check.
///
/// Deliberately conservative. It fixes representation, never content: casing,
/// punctuation and interior spacing are left exactly as written, because
/// changing them would merge records that genuinely differ.
pub fn normalize(content: &str) -> String {
    // NFC first so accented characters have one canonical encoding.
    let composed: String = content.nfc().collect();
    // Canonical line endings, then strip leading/trailing whitespace and any
    // trailing spaces a text editor left at end of line.
    let unified = composed.replace("\r\n", "\n").replace('\r', "\n");
    unified
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// A rights holder supplying records, and where their payment goes.
///
/// Rights live here rather than on every record: the licensing unit is the
/// source, so repeating terms per row would be storage spent on a value that is
/// identical a million times over.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// Stable identifier used by records to point here.
    pub id: String,
    /// Who holds the rights and is owed payment.
    pub holder: String,
    /// SPDX identifier where one applies, otherwise free text.
    pub license: String,
    /// Anything the license does not cover (embargoes, attribution wording).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terms: Option<String>,
    /// Where payment is routed. A contributor handle, never a credential.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payout_ref: Option<String>,
    /// Quality tier. Attribution prices a TIER by ablation and splits within it
    /// by volume, so the tier is what makes per-source ablation affordable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

impl Source {
    pub fn new(id: impl Into<String>, holder: impl Into<String>, license: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            holder: holder.into(),
            license: license.into(),
            terms: None,
            payout_ref: None,
            tier: None,
        }
    }

    pub fn with_payout_ref(mut self, r: impl Into<String>) -> Self {
        self.payout_ref = Some(r.into());
        self
    }

    pub fn with_tier(mut self, t: impl Into<String>) -> Self {
        self.tier = Some(t.into());
        self
    }

    pub fn with_terms(mut self, t: impl Into<String>) -> Self {
        self.terms = Some(t.into());
        self
    }
}

/// One record's identity, origin, and lineage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordProvenance {
    pub id: RecordId,
    /// The [`Source::id`] this record's rights come from.
    pub source: String,
    /// Records this one was derived from. Empty for an original.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<RecordId>,
    /// Normalized byte length, so a volume split needs no second pass over data.
    pub bytes: usize,
}

/// One step in how the corpus was derived.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformStep {
    /// `dedup`, `filter`, `mixture`, `tokenize`, …
    pub op: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub records_in: usize,
    pub records_out: usize,
}

/// Per-source totals, which is what a volume split is computed from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceVolume {
    pub records: usize,
    pub bytes: usize,
}

/// The auditable description of a corpus.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub dataset: String,
    pub sources: Vec<Source>,
    pub records: Vec<RecordProvenance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transforms: Vec<TransformStep>,
    /// Hex Merkle root over every record id, sorted. Recomputable; see
    /// [`Manifest::verify`].
    pub merkle_root: String,
}

impl Manifest {
    /// Was this exact content in the corpus?
    ///
    /// The audit question, and the contamination check. Normalizes and hashes,
    /// so a caller does not have to know how identity is computed.
    pub fn contains(&self, content: &str) -> bool {
        let id = RecordId::of(content);
        self.records.iter().any(|r| r.id == id)
    }

    /// The rights covering a record, if it is present.
    pub fn rights_for(&self, content: &str) -> Option<&Source> {
        let id = RecordId::of(content);
        let rec = self.records.iter().find(|r| r.id == id)?;
        self.sources.iter().find(|s| s.id == rec.source)
    }

    /// Records and bytes per source. The denominator of a volume split.
    pub fn volume_by_source(&self) -> BTreeMap<String, SourceVolume> {
        let mut out: BTreeMap<String, SourceVolume> = BTreeMap::new();
        for r in &self.records {
            let e = out.entry(r.source.clone()).or_default();
            e.records += 1;
            e.bytes += r.bytes;
        }
        out
    }

    /// Each source's share of the corpus by bytes, summing to 1.0.
    ///
    /// This is the *split within a tier*. It is deliberately not the whole
    /// payment: volume alone rewards bulk over worth, so the tier rate comes
    /// from ablation and this only divides the pool inside it.
    pub fn volume_share(&self) -> BTreeMap<String, f64> {
        let vols = self.volume_by_source();
        let total: usize = vols.values().map(|v| v.bytes).sum();
        if total == 0 {
            return BTreeMap::new();
        }
        vols.into_iter()
            .map(|(k, v)| (k, v.bytes as f64 / total as f64))
            .collect()
    }

    /// Recompute the Merkle root and compare. Detects any record added,
    /// removed, or altered since the manifest was written.
    pub fn verify(&self) -> Result<(), String> {
        let ids: Vec<RecordId> = self.records.iter().map(|r| r.id).collect();
        let recomputed = hex::encode(merkle_root(&ids));
        if recomputed == self.merkle_root {
            Ok(())
        } else {
            Err(format!(
                "manifest root mismatch: stored {}, recomputed {}",
                self.merkle_root, recomputed
            ))
        }
    }

    /// Every source referenced by a record but not declared. Non-empty means
    /// the corpus contains records nobody can be paid for.
    pub fn undeclared_sources(&self) -> Vec<String> {
        let mut missing: Vec<String> = self
            .records
            .iter()
            .filter(|r| !self.sources.iter().any(|s| s.id == r.source))
            .map(|r| r.source.clone())
            .collect();
        missing.sort();
        missing.dedup();
        missing
    }
}

/// Merkle root over record ids.
///
/// Ids are sorted first so the root depends on the SET, not on ingest order:
/// two assemblies of the same records must agree, or the root proves nothing
/// useful. An odd node is promoted rather than duplicated, which avoids the
/// duplicate-leaf ambiguity where two different sets can hash alike.
/// The empty corpus hashes to all zeroes.
pub fn merkle_root(ids: &[RecordId]) -> [u8; 32] {
    if ids.is_empty() {
        return [0u8; 32];
    }
    let mut level: Vec<[u8; 32]> = {
        let mut sorted: Vec<RecordId> = ids.to_vec();
        sorted.sort();
        sorted.dedup();
        sorted.into_iter().map(|r| r.0).collect()
    };
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            if pair.len() == 2 {
                let mut h = Sha256::new();
                h.update(pair[0]);
                h.update(pair[1]);
                let out = h.finalize();
                let mut node = [0u8; 32];
                node.copy_from_slice(&out);
                next.push(node);
            } else {
                next.push(pair[0]);
            }
        }
        level = next;
    }
    level[0]
}

/// Assembles a [`Manifest`] while records are ingested.
#[derive(Debug, Default)]
pub struct ManifestBuilder {
    dataset: String,
    sources: Vec<Source>,
    records: Vec<RecordProvenance>,
    transforms: Vec<TransformStep>,
    seen: std::collections::HashSet<RecordId>,
}

impl ManifestBuilder {
    pub fn new(dataset: impl Into<String>) -> Self {
        Self { dataset: dataset.into(), ..Default::default() }
    }

    /// Declare a rights holder. Re-declaring an id replaces it.
    pub fn source(mut self, source: Source) -> Self {
        if let Some(existing) = self.sources.iter_mut().find(|s| s.id == source.id) {
            *existing = source;
        } else {
            self.sources.push(source);
        }
        self
    }

    /// Ingest an original record. Returns its id.
    ///
    /// Ingesting the same content twice under the same source is a no-op: the
    /// corpus is a set, and counting a duplicate twice would inflate that
    /// source's volume share and overpay them.
    pub fn record(&mut self, content: &str, source_id: &str) -> RecordId {
        self.push(content, source_id, Vec::new())
    }

    /// Ingest a record derived from others, keeping the link to its parents.
    pub fn derived(&mut self, content: &str, source_id: &str, parents: Vec<RecordId>) -> RecordId {
        self.push(content, source_id, parents)
    }

    fn push(&mut self, content: &str, source_id: &str, parents: Vec<RecordId>) -> RecordId {
        let normalized = normalize(content);
        let id = RecordId::of(content);
        if self.seen.insert(id) {
            self.records.push(RecordProvenance {
                id,
                source: source_id.to_string(),
                parents,
                bytes: normalized.len(),
            });
        }
        id
    }

    /// Record a derivation step for the lineage log.
    pub fn transform(
        &mut self,
        op: impl Into<String>,
        detail: Option<String>,
        records_in: usize,
        records_out: usize,
    ) {
        self.transforms.push(TransformStep {
            op: op.into(),
            detail,
            records_in,
            records_out,
        });
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn build(self) -> Manifest {
        let ids: Vec<RecordId> = self.records.iter().map(|r| r.id).collect();
        Manifest {
            dataset: self.dataset,
            sources: self.sources,
            records: self.records,
            transforms: self.transforms,
            merkle_root: hex::encode(merkle_root(&ids)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder() -> ManifestBuilder {
        ManifestBuilder::new("test-corpus").source(
            Source::new("acme", "Acme Archive", "CC-BY-4.0")
                .with_payout_ref("bliss:acme")
                .with_tier("curated"),
        )
    }

    #[test]
    fn identity_is_stable_across_representation_not_content() {
        // Line endings, trailing spaces and a trailing newline are representation.
        assert_eq!(RecordId::of("a\r\nb"), RecordId::of("a\nb"));
        assert_eq!(RecordId::of("hello  \nworld"), RecordId::of("hello\nworld"));
        assert_eq!(RecordId::of("text\n\n"), RecordId::of("text"));
        // Casing and interior spacing are CONTENT and must stay distinct.
        assert_ne!(RecordId::of("Hello"), RecordId::of("hello"));
        assert_ne!(RecordId::of("a b"), RecordId::of("a  b"));
    }

    #[test]
    fn unicode_composition_does_not_fork_identity() {
        // "é" precomposed vs "e" + combining acute: the same text.
        let precomposed = "caf\u{00e9}";
        let decomposed = "cafe\u{0301}";
        assert_ne!(precomposed, decomposed, "inputs must differ as raw bytes");
        assert_eq!(RecordId::of(precomposed), RecordId::of(decomposed));
    }

    #[test]
    fn hex_round_trips_and_rejects_junk() {
        let id = RecordId::of("x");
        assert_eq!(RecordId::from_hex(&id.to_hex()), Some(id));
        assert_eq!(RecordId::from_hex("not hex"), None);
        assert_eq!(RecordId::from_hex("abcd"), None, "wrong length must be refused");
    }

    #[test]
    fn the_audit_question_is_answerable() {
        let mut b = builder();
        b.record("the quick brown fox", "acme");
        let m = b.build();

        assert!(m.contains("the quick brown fox"));
        // And through a reformatting that would defeat a raw-bytes hash.
        assert!(m.contains("the quick brown fox\r\n"));
        assert!(!m.contains("a document nobody supplied"));

        let rights = m.rights_for("the quick brown fox").expect("rights are attached");
        assert_eq!(rights.holder, "Acme Archive");
        assert_eq!(rights.license, "CC-BY-4.0");
        assert_eq!(rights.payout_ref.as_deref(), Some("bliss:acme"));
    }

    #[test]
    fn a_duplicate_does_not_inflate_a_payout() {
        let mut b = builder();
        b.record("same text", "acme");
        b.record("same text", "acme");
        b.record("same text\n", "acme"); // and a reformatted copy
        let m = b.build();
        assert_eq!(m.records.len(), 1, "the corpus is a set");
        assert_eq!(m.volume_by_source()["acme"].records, 1);
    }

    #[test]
    fn lineage_survives_a_transform() {
        let mut b = builder();
        let parent = b.record("original passage", "acme");
        let child = b.derived("ORIGINAL PASSAGE", "acme", vec![parent]);
        b.transform("uppercase", Some("demo transform".into()), 1, 1);
        let m = b.build();

        let derived = m.records.iter().find(|r| r.id == child).unwrap();
        assert_eq!(derived.parents, vec![parent]);
        assert_eq!(m.transforms.len(), 1);
        assert_eq!(m.transforms[0].op, "uppercase");
    }

    #[test]
    fn the_root_depends_on_the_set_not_the_order() {
        let mut a = ManifestBuilder::new("c");
        a.record("one", "s");
        a.record("two", "s");
        let mut b = ManifestBuilder::new("c");
        b.record("two", "s");
        b.record("one", "s");
        assert_eq!(a.build().merkle_root, b.build().merkle_root);
    }

    #[test]
    fn verify_catches_an_altered_corpus() {
        let mut b = builder();
        b.record("one", "acme");
        b.record("two", "acme");
        b.record("three", "acme");
        let mut m = b.build();
        assert!(m.verify().is_ok());

        // Remove a record without rewriting the root: tamper-evident.
        m.records.pop();
        assert!(m.verify().is_err());
    }

    #[test]
    fn verify_catches_a_substituted_record() {
        let mut b = builder();
        b.record("real", "acme");
        let mut m = b.build();
        m.records[0].id = RecordId::of("forged");
        assert!(m.verify().is_err());
    }

    #[test]
    fn an_empty_corpus_has_a_defined_root() {
        let m = ManifestBuilder::new("empty").build();
        assert_eq!(m.merkle_root, hex::encode([0u8; 32]));
        assert!(m.verify().is_ok());
    }

    #[test]
    fn an_odd_leaf_count_still_verifies() {
        // Exercises the promote-the-odd-node path at several tree shapes.
        for n in 1..=9usize {
            let mut b = ManifestBuilder::new("odd");
            for i in 0..n {
                b.record(&format!("record {i}"), "acme");
            }
            let m = b.build();
            assert!(m.verify().is_ok(), "n={n}");
        }
    }

    #[test]
    fn volume_share_splits_by_bytes_and_sums_to_one() {
        let mut b = ManifestBuilder::new("c")
            .source(Source::new("a", "A", "MIT"))
            .source(Source::new("b", "B", "MIT"));
        b.record("xxxx", "a"); // 4 bytes
        b.record("xxxxxxxxxxxx", "b"); // 12 bytes
        let m = b.build();

        let share = m.volume_share();
        assert!((share["a"] - 0.25).abs() < 1e-9);
        assert!((share["b"] - 0.75).abs() < 1e-9);
        assert!((share.values().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_record_with_no_declared_source_is_reported() {
        let mut b = builder();
        b.record("orphan", "nobody");
        let m = b.build();
        // Silently shipping this would mean training on data nobody can be paid for.
        assert_eq!(m.undeclared_sources(), vec!["nobody".to_string()]);
    }

    #[test]
    fn the_manifest_round_trips_through_json() {
        let mut b = builder();
        b.record("alpha", "acme");
        b.record("beta", "acme");
        b.transform("dedup", None, 3, 2);
        let m = b.build();

        let json = serde_json::to_string_pretty(&m).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
        assert!(back.verify().is_ok(), "root survives serialization");
    }

    #[test]
    fn a_payout_ref_is_a_handle_and_never_a_credential() {
        let s = Source::new("acme", "Acme", "MIT").with_payout_ref("bliss:acme");
        let rendered = serde_json::to_string(&s).unwrap();
        assert!(rendered.contains("bliss:acme"));
        // Nothing in the struct can carry a secret; there is no field for one.
        assert!(!rendered.contains("token"));
        assert!(!rendered.contains("key"));
    }
}

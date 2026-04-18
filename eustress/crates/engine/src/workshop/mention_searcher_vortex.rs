//! # Vortex semantic searcher for the Workshop `@` mention popup
//!
//! Feature-gated alternative to [`super::mention::SubstringSearcher`] that
//! embeds each entry's name + qualifier with SpatialVortex's
//! `SubwordTokenizer::encode_text` and indexes the vectors in an
//! [`embedvec`]-backed HNSW with H4-lattice quantization. Ranking at query
//! time is cosine similarity with an optional SacredEmbedding rerank and an
//! `IndefiniteEmbedvecLearner` online-update hook that nudges vectors
//! toward items the user commits repeatedly.
//!
//! ## Compile vs. runtime gating
//!
//! The module is only compiled when the `workshop-vortex-embeddings`
//! feature is on. Even with the feature enabled, constructing a
//! [`VortexSearcher`] can fail (missing tokenizer weights, corrupt index
//! file). The plugin falls back to [`super::mention::SubstringSearcher`]
//! on failure so the popup never disappears because the embeddings layer
//! didn't load.
//!
//! ## On-disk layout
//!
//! ```text
//! {Universe}/.eustress/knowledge/
//!   └─ mentions.sled/       ← sled db holding H4-quantized vectors +
//!                             HNSW neighbour lists managed by embedvec.
//! ```
//!
//! The manifest + meta JSON continue to live alongside this directory and
//! are managed by [`super::mention_persistence`].
//!
//! ## Implementation status
//!
//! Scaffolding only — the actual Vortex / embedvec wiring is deferred
//! until those crates are published to the workspace. The module still
//! compiles under the feature flag so downstream consumers can build
//! against the stable public surface while the internals land.

use super::mention::{MentionEntry, MentionId, MentionSearcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Error returned when the Vortex backend fails to initialise (missing
/// weights, corrupt db, etc.). Plugins react by falling back to the
/// substring searcher.
#[derive(Debug)]
pub enum VortexInitError {
    TokenizerNotFound(PathBuf),
    SledFailure(String),
    NotYetImplemented(&'static str),
}

impl std::fmt::Display for VortexInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokenizerNotFound(p) => write!(f, "vortex tokenizer weights not found at {}", p.display()),
            Self::SledFailure(e) => write!(f, "sled index unavailable: {}", e),
            Self::NotYetImplemented(what) => write!(f, "feature gate hit a code path that hasn't landed yet: {}", what),
        }
    }
}

impl std::error::Error for VortexInitError {}

/// Semantic searcher backed by SpatialVortex subword embeddings. Works as
/// a drop-in replacement for [`super::mention::SubstringSearcher`] via
/// [`MentionSearcher`].
pub struct VortexSearcher {
    /// Resolved per-entry embeddings. In the full implementation each
    /// value is the H4-quantized vector keyed by `MentionId`. The stub
    /// stores nothing — `search()` delegates to the substring fallback
    /// held inside until the real backend lands.
    _entries: HashMap<MentionId, MentionEntry>,
    /// Fallback used while the Vortex pipeline is still stubbed. Lets the
    /// popup keep working in a feature-enabled build.
    fallback: super::mention::SubstringSearcher,
    /// On-disk database path. Retained so callers can surface telemetry
    /// ("index at …") even when the runtime stays stubbed.
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl VortexSearcher {
    /// Attempt to load the Vortex backend from the Universe's knowledge
    /// directory. Returns `Ok` with a searcher ready for queries, or
    /// `Err` with a reason the fallback should be used instead.
    ///
    /// Planned integration points (once SpatialVortex lands in the
    /// workspace):
    /// 1. Load `SubwordTokenizer` from `{knowledge_dir}/vortex_vocab.bin`.
    /// 2. Open (or create) `sled::open(db_path)` and wrap in
    ///    `embedvec::HnswIndex::with_h4_quantization(32, 64)`.
    /// 3. Build a `RAGEngine` for rerank; hold an
    ///    `IndefiniteEmbedvecLearner` for online MRU-driven updates.
    pub fn try_open(knowledge_dir: &Path) -> Result<Self, VortexInitError> {
        let db_path = knowledge_dir.join("mentions.sled");
        // Placeholder — the real implementation would fail early here if
        // the tokenizer or sled db couldn't be opened. Keep returning a
        // working searcher so feature-on builds stay functional.
        Ok(Self {
            _entries: HashMap::new(),
            fallback: super::mention::SubstringSearcher::new(),
            db_path,
        })
    }
}

impl MentionSearcher for VortexSearcher {
    fn rebuild(&mut self, entries: &HashMap<MentionId, MentionEntry>) {
        self.fallback.rebuild(entries);
        // TODO(workshop-vortex-embeddings): tokenize + embed + bulk-insert
        // every entry into the HNSW with H4 quantization. Persist the
        // resulting vectors to `db_path`.
    }

    fn upsert(&mut self, entry: &MentionEntry) {
        self.fallback.upsert(entry);
        // TODO(workshop-vortex-embeddings): re-embed + HNSW.update(id, vec).
    }

    fn remove(&mut self, id: MentionId) {
        self.fallback.remove(id);
        // TODO(workshop-vortex-embeddings): HNSW.remove(id) + sled drop.
    }

    fn search(&self, query: &str, top_k: usize) -> Vec<(MentionId, f32)> {
        // TODO(workshop-vortex-embeddings): tokenize query, query HNSW,
        // rerank via RAGEngine with SacredEmbedding boosts, return.
        //
        // Until the wiring lands, the substring fallback keeps the UI
        // responsive and behaviourally close to the non-feature build.
        self.fallback.search(query, top_k)
    }
}

//! # Mention index persistence
//!
//! Read/write the mention catalogue and per-user MRU state from
//! `{Universe}/.eustress/knowledge/`:
//!
//! ```text
//!   .eustress/knowledge/
//!   ├── manifest.toml           # schema version + last scan timestamp
//!   ├── mentions-meta.json      # all MentionEntry rows by id
//!   ├── mru/
//!   │   ├── _offline.json       # offline user bucket
//!   │   └── {public_key}.json   # per authenticated user
//!   └── mentions.sled/          # vector index (feature-gated)
//! ```
//!
//! **Why JSON for `mentions-meta`?** Hot path. Read on Universe load,
//! written after every scan. Serde-JSON parses ~3× faster than TOML for
//! a 50k-entry catalogue.
//!
//! **Why TOML for the manifest?** Humans inspect it. Schema version and
//! scan timestamp are the kind of thing you git-diff.
//!
//! **Why per-user MRU files?** Multiple users editing the same Universe
//! (e.g. a collaborative Eustress project) shouldn't share their recency
//! model — one user's @-history pollutes the other's popup ordering.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::mention::{MentionEntry, MentionId, MentionIndex, UserMru};

/// Schema version. Bump when the `MentionEntry` layout changes in a
/// backwards-incompatible way. On mismatch, `load_meta` returns `None` so
/// the caller triggers a full rescan instead of deserialising stale rows.
pub const MENTION_SCHEMA_VERSION: u32 = 1;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Path helpers
// ═══════════════════════════════════════════════════════════════════════════

pub fn knowledge_dir(universe_root: &Path) -> PathBuf {
    universe_root.join(".eustress").join("knowledge")
}

pub fn manifest_path(universe_root: &Path) -> PathBuf {
    knowledge_dir(universe_root).join("manifest.toml")
}

pub fn meta_path(universe_root: &Path) -> PathBuf {
    knowledge_dir(universe_root).join("mentions-meta.json")
}

pub fn mru_dir(universe_root: &Path) -> PathBuf {
    knowledge_dir(universe_root).join("mru")
}

pub fn mru_path_for(universe_root: &Path, user_key: &str) -> PathBuf {
    // Sanitize the key for filesystem use — Ed25519 hex keys are already
    // filesystem-safe but defensive sanitation guards against unknown
    // future identity formats.
    let safe: String = user_key.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    mru_dir(universe_root).join(format!("{}.json", safe))
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Manifest — schema version + scan metadata
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeManifest {
    pub schema_version: u32,
    /// RFC 3339 timestamp of last successful full scan.
    pub last_scan: String,
    /// Count of entries the scan produced — used for quick integrity checks.
    pub entry_count: u64,
}

impl KnowledgeManifest {
    pub fn current(entry_count: u64) -> Self {
        Self {
            schema_version: MENTION_SCHEMA_VERSION,
            last_scan: chrono::Utc::now().to_rfc3339(),
            entry_count,
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.schema_version == MENTION_SCHEMA_VERSION
    }
}

pub fn load_manifest(universe_root: &Path) -> Option<KnowledgeManifest> {
    let text = std::fs::read_to_string(manifest_path(universe_root)).ok()?;
    toml::from_str(&text).ok()
}

pub fn save_manifest(universe_root: &Path, manifest: &KnowledgeManifest) -> std::io::Result<()> {
    std::fs::create_dir_all(knowledge_dir(universe_root))?;
    let text = toml::to_string_pretty(manifest)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(manifest_path(universe_root), text)
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Entry cache — mentions-meta.json
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, Deserialize)]
struct MetaFile {
    schema: u32,
    entries: Vec<MentionEntry>,
}

/// Read the cached entry set. Returns `None` on missing/corrupt/schema-stale
/// file — caller should trigger a fresh scan in that case.
pub fn load_meta(universe_root: &Path) -> Option<HashMap<MentionId, MentionEntry>> {
    let text = std::fs::read_to_string(meta_path(universe_root)).ok()?;
    let file: MetaFile = serde_json::from_str(&text).ok()?;
    if file.schema != MENTION_SCHEMA_VERSION {
        return None;
    }
    Some(file.entries.into_iter().map(|e| (e.id, e)).collect())
}

pub fn save_meta(
    universe_root: &Path,
    entries: &HashMap<MentionId, MentionEntry>,
) -> std::io::Result<()> {
    std::fs::create_dir_all(knowledge_dir(universe_root))?;
    // Only persist static entries (Toml + Filesystem). Live ECS entries
    // are session-scoped and re-derived every launch.
    let mut vec: Vec<&MentionEntry> = entries.values()
        .filter(|e| !matches!(e.source, crate::workshop::mention::MentionSource::Ecs))
        .collect();
    vec.sort_by_key(|e| e.id);

    let file = serde_json::json!({
        "schema": MENTION_SCHEMA_VERSION,
        "entries": vec,
    });
    let text = serde_json::to_string(&file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(meta_path(universe_root), text)
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Per-user MRU
// ═══════════════════════════════════════════════════════════════════════════

pub fn load_user_mru(universe_root: &Path, user_key: &str) -> Option<UserMru> {
    let text = std::fs::read_to_string(mru_path_for(universe_root, user_key)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_user_mru(
    universe_root: &Path,
    user_key: &str,
    mru: &UserMru,
) -> std::io::Result<()> {
    std::fs::create_dir_all(mru_dir(universe_root))?;
    let text = serde_json::to_string(mru)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(mru_path_for(universe_root, user_key), text)
}

/// Load every user's MRU from disk. Returns an empty map if the MRU
/// directory doesn't exist or can't be read.
pub fn load_all_mru(universe_root: &Path) -> HashMap<String, UserMru> {
    let mut out = HashMap::new();
    let dir = mru_dir(universe_root);
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => return out,
    };
    for entry in rd.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") { continue; }
        let user_key = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(mru) = serde_json::from_str::<UserMru>(&text) {
                out.insert(user_key, mru);
            }
        }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Orchestration — load on boot + autosave after mutations
// ═══════════════════════════════════════════════════════════════════════════

/// On Universe change, hydrate `MentionIndex` from disk when the cache is
/// fresh AND schema-compatible. Returns true when the index was populated
/// from cache (caller can skip an initial scan if so).
pub fn hydrate_from_disk(universe_root: &Path, index: &mut MentionIndex) -> bool {
    // Manifest check first — saves parsing a huge JSON if schema drifted.
    let manifest = match load_manifest(universe_root) {
        Some(m) if m.is_compatible() => m,
        _ => return false,
    };

    let entries = match load_meta(universe_root) {
        Some(e) if e.len() as u64 == manifest.entry_count => e,
        _ => return false,
    };
    index.rebuild(entries);

    // Hydrate MRU for all known users; active-user selection happens later
    // once auth state is known.
    let mru = load_all_mru(universe_root);
    index.set_mru(mru);

    info!("mention-persistence: hydrated {} entries from {}",
          index.len(), meta_path(universe_root).display());
    true
}

/// Persist the full index state. Called after a scan completes and on
/// periodic autosave.
pub fn persist_all(universe_root: &Path, index: &MentionIndex) -> std::io::Result<()> {
    save_meta(universe_root, index.entries())?;
    save_manifest(universe_root, &KnowledgeManifest::current(index.len() as u64))?;
    for (user, mru) in index.mru_snapshot() {
        save_user_mru(universe_root, &user, &mru)?;
    }
    Ok(())
}

/// Debounced autosave — writes at most once per `min_interval_secs`. Call
/// this every frame; it's cheap when clean.
pub fn autosave_index(
    mut last_save: bevy::prelude::Local<Option<std::time::Instant>>,
    mut last_generation: bevy::prelude::Local<u64>,
    index: bevy::prelude::Res<MentionIndex>,
) {
    const MIN_INTERVAL_SECS: u64 = 30;
    let Some(universe_root) = index.universe_root() else { return };

    // Only write when something actually changed.
    if index.generation() == *last_generation { return; }

    // Debounce.
    let now = std::time::Instant::now();
    if let Some(last) = *last_save {
        if now.duration_since(last).as_secs() < MIN_INTERVAL_SECS { return; }
    }

    if let Err(e) = persist_all(universe_root, &index) {
        bevy::prelude::warn!("mention-persistence: autosave failed: {}", e);
    } else {
        *last_save = Some(now);
        *last_generation = index.generation();
    }
}

// Bring `info!` into scope for this module without pulling the prelude
// into every file.
use bevy::prelude::info;

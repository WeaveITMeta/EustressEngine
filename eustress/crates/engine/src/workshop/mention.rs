//! # Workshop `@` Mention — Universe-scoped semantic index
//!
//! Backs the chat panel's `@` autocomplete. Indexes **every referenceable
//! item across every Space in the current Universe** — entities, files,
//! scripts, services — so the user working inside Space1 can @-reference
//! a battery component sitting in Space2 without leaving their chat.
//!
//! ## Architecture
//!
//! ```text
//!   Universe filesystem ─┐                      ┌── live ECS (current Space only)
//!                        │                      │
//!    scan_universe_mentions                   update_mention_index_live
//!    (TOML walker)                             (Bevy change detection)
//!                        │                      │
//!                        ▼                      ▼
//!                   ┌─────────────────────────────────┐
//!                   │ MentionIndex (Resource)         │
//!                   │  • entries: HashMap<id, entry>  │
//!                   │  • searcher: dyn MentionSearcher│
//!                   │  • mru:  per-user recency queue │
//!                   └─────────────────────────────────┘
//!                              │
//!                              ▼
//!                   ┌─────────────────────────────────┐
//!                   │ trait MentionSearcher           │
//!                   │  • SubstringSearcher   (A.0)    │
//!                   │  • VortexSearcher      (A.2)    │
//!                   │  • EmbedvecSearcher    (A.3)    │
//!                   └─────────────────────────────────┘
//! ```
//!
//! ## Canonical paths
//!
//! Every indexed item gets a stable, Universe-wide handle of the form:
//!
//! ```text
//!   @entity:Space1/Workspace/V-Cell/V1/VCell_Housing
//!   @file:Space2/Assets/images/board-diagram.png
//!   @script:Space1/SoulService/action_rules
//!   @service:Space1/Workspace
//! ```
//!
//! `MentionId` is a stable hash of `(kind, canonical_path)` so saved
//! conversations round-trip through session persistence even when the
//! target Space is currently unloaded.
//!
//! ## Persistence layout
//!
//! `{Universe}/.eustress/knowledge/` holds:
//! - `manifest.toml` — schema version + last-scan timestamp
//! - `mentions-meta.json` — all entries keyed by MentionId
//! - `mru/{public_key}.json` — per-user recency queue (unauth user → `"_offline"`)
//! - `mentions.sled/` (feature `workshop-vortex-embeddings`) — vector index
//!
//! ## Incremental updates
//!
//! Entities mutate constantly at runtime. Full rebuilds are too expensive
//! for 50k-entity Universes. The system uses two complementary sources:
//!
//! * **Static** (`scan_universe_mentions`) — runs on Universe load and when
//!   the file watcher fires create/delete on a TOML. Yields entries for
//!   every Space, loaded or not.
//! * **Live** (`update_mention_index_live`) — runs every frame, processes
//!   only `Added<Instance>` / `Changed<Instance>` / `Removed<Instance>`
//!   from the currently-loaded Space. Overrides the static entry for the
//!   same id when present; reverts to static on Space unload.

use bevy::prelude::*;
use eustress_common::classes::{ClassName, Instance};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

// ═══════════════════════════════════════════════════════════════════════════
// 1. Types — MentionId, MentionKind, MentionEntry, MentionSource
// ═══════════════════════════════════════════════════════════════════════════

/// Stable identifier for one item in the mention index. Computed as a hash
/// of `(kind, canonical_path)` so the same item produces the same id across
/// sessions — required for cross-session MRU persistence and mention-chip
/// round-tripping in saved conversations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MentionId(pub u64);

impl MentionId {
    pub fn from_canonical(kind: MentionKind, canonical: &str) -> Self {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        kind.hash(&mut h);
        canonical.hash(&mut h);
        MentionId(h.finish())
    }
}

/// Categorisation for UI filtering + icon selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MentionKind {
    /// A Part / Model / GUI child — anything with an `Instance` component.
    /// Excludes scripts and services, which have dedicated kinds.
    Entity,
    /// Any file under `{Universe}/Spaces/*/` — images, PDFs, docs, TOMLs.
    File,
    /// A SoulScript or Luau script. Separated so the popup can render a
    /// distinct icon + filter.
    Script,
    /// A Service folder (`Workspace`, `Lighting`, `SoulService`, …).
    Service,
}

impl MentionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Entity => "entity",
            Self::File => "file",
            Self::Script => "script",
            Self::Service => "service",
        }
    }
}

/// Where this entry's data came from — determines whether the resolver
/// reaches into the ECS (live) or reads from disk (static).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MentionSource {
    /// Read from a TOML file on disk. Valid for any Space in the Universe.
    Toml,
    /// Mirrored from a live ECS entity in the currently-loaded Space.
    /// Takes precedence over a `Toml` entry with the same id.
    Ecs,
    /// Plain filesystem entry (image, doc, etc.) — no `Instance` metadata.
    Filesystem,
}

/// One referenceable item. `canonical_path` is the stable handle inserted
/// into chat messages; `name` + `qualifier` are for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentionEntry {
    pub id: MentionId,
    pub kind: MentionKind,
    /// Display label — typically `Instance.name` or filename stem.
    pub name: String,
    /// Secondary text — class, path, file size. Shown below `name` in the
    /// popup to disambiguate items that share a name.
    pub qualifier: String,
    /// Machine-readable handle: `@kind:space/path-within-space`.
    /// Space-qualified so cross-Space references are unambiguous.
    pub canonical_path: String,
    /// Which Space owns this item (folder name under `Universe/Spaces/`).
    /// `""` for Universe-level resources (e.g. Universe-scoped services).
    pub space: String,
    /// Relative path from the Space root to this item. Used by the send-time
    /// resolver to open the TOML/file when the Space isn't loaded.
    pub rel_path: String,
    /// Icon key (no extension) used by Slint — e.g. `"part"`, `"soulservice"`.
    pub icon_hint: String,
    /// Source of this data — live ECS vs static TOML vs filesystem.
    pub source: MentionSource,
    /// Present when the owning Space is currently loaded and the entity
    /// still exists. `None` for filesystem-only and unloaded-Space entries.
    #[serde(skip)]
    pub entity: Option<Entity>,
}

impl MentionEntry {
    /// Construct a canonical path for a kind + space + in-space path.
    ///
    /// `space = ""` means Universe-scoped (rare — reserved for
    /// Universe-level services once they're introduced).
    pub fn canonical_for(kind: MentionKind, space: &str, rel_path: &str) -> String {
        let rel_normalised = rel_path.replace('\\', "/");
        let rel_trimmed = rel_normalised.trim_start_matches('/');
        if space.is_empty() {
            format!("{}:.{}{}",
                kind.as_str(),
                if rel_trimmed.is_empty() { "" } else { "/" },
                rel_trimmed)
        } else {
            format!("{}:{}/{}", kind.as_str(), space, rel_trimmed)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. MentionSearcher trait — pluggable backends
// ═══════════════════════════════════════════════════════════════════════════

/// Ranks matches. Implementations range from a trivial substring scan
/// (day-1, always available) to a Vortex-embedded H4-quantized HNSW index
/// (production, feature-gated). Queries are debounced at 80 ms on the UI
/// so blocking briefly in `search` is acceptable.
pub trait MentionSearcher: Send + Sync + 'static {
    /// Replace the searcher's state with the supplied entries. Called on
    /// Universe switch or full rebuild.
    fn rebuild(&mut self, entries: &HashMap<MentionId, MentionEntry>);

    /// Add or replace one entry. Called from the incremental update systems.
    fn upsert(&mut self, entry: &MentionEntry);

    /// Drop one entry by id.
    fn remove(&mut self, id: MentionId);

    /// Return the top-`k` matches for `query` as `(id, score)` pairs with
    /// score in `[0.0, 1.0]`. Higher = better. Empty query → empty vec;
    /// `MentionIndex::search` substitutes the MRU list in that case.
    fn search(&self, query: &str, top_k: usize) -> Vec<(MentionId, f32)>;
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. SubstringSearcher — day-1 fallback, always bundled
// ═══════════════════════════════════════════════════════════════════════════

/// Fallback that ranks by case-insensitive substring match on `name` then
/// `qualifier`. Works offline, no model dependency. Default when
/// `workshop-vortex-embeddings` is disabled and fallback when it is on but
/// the vector backend fails to initialise.
///
/// Scoring:
/// - Prefix match on `name`:  1.00
/// - Substring in `name`:     0.70
/// - Substring in `qualifier`: 0.30
pub struct SubstringSearcher {
    /// Compact layout: all lowercased strings contiguous, no hash lookup
    /// per entry in the search hot path.
    entries: Vec<(MentionId, String /* name_lc */, String /* qual_lc */)>,
}

impl SubstringSearcher {
    pub fn new() -> Self {
        Self { entries: Vec::with_capacity(1024) }
    }
}

impl Default for SubstringSearcher {
    fn default() -> Self { Self::new() }
}

impl MentionSearcher for SubstringSearcher {
    fn rebuild(&mut self, entries: &HashMap<MentionId, MentionEntry>) {
        self.entries.clear();
        self.entries.reserve(entries.len());
        for e in entries.values() {
            self.entries.push((e.id, e.name.to_lowercase(), e.qualifier.to_lowercase()));
        }
    }

    fn upsert(&mut self, entry: &MentionEntry) {
        self.entries.retain(|(id, _, _)| *id != entry.id);
        self.entries.push((entry.id, entry.name.to_lowercase(), entry.qualifier.to_lowercase()));
    }

    fn remove(&mut self, id: MentionId) {
        self.entries.retain(|(i, _, _)| *i != id);
    }

    fn search(&self, query: &str, top_k: usize) -> Vec<(MentionId, f32)> {
        let q = query.to_lowercase();
        if q.is_empty() { return Vec::new(); }

        let mut scored: Vec<(MentionId, f32)> = self.entries.iter()
            .filter_map(|(id, name, qual)| {
                let score = if name.starts_with(&q) {
                    1.0
                } else if name.contains(&q) {
                    0.70
                } else if qual.contains(&q) {
                    0.30
                } else {
                    return None;
                };
                Some((*id, score))
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. MRU — per-user recency queue
// ═══════════════════════════════════════════════════════════════════════════

/// Maximum MRU history length. The 16 most-recent selections surface as
/// zero-query defaults ("Recent" section atop the popup) and contribute a
/// small score boost to later ranking.
const MRU_CAPACITY: usize = 16;

/// Per-user recency state. Keyed by `public_key` from [`CreatorStamp`]
/// (the user's Ed25519 identity). Offline users share `"_offline"` as the
/// bucket key so their MRU doesn't pollute authenticated users' state.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct UserMru {
    /// Ordered oldest-first. Newest push_back; cap at `MRU_CAPACITY`.
    pub queue: VecDeque<MentionId>,
}

impl UserMru {
    pub fn bump(&mut self, id: MentionId) {
        self.queue.retain(|i| *i != id);
        self.queue.push_back(id);
        while self.queue.len() > MRU_CAPACITY {
            self.queue.pop_front();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. MentionIndex — the Bevy resource
// ═══════════════════════════════════════════════════════════════════════════

/// Universe-scoped live mention index. Owns the full entry map, the
/// pluggable [`MentionSearcher`], per-user MRU queues, and the Universe
/// root for disk persistence. Use [`Self::search`] from UI code;
/// [`Self::record_usage`] after the user commits a selection.
#[derive(Resource)]
pub struct MentionIndex {
    entries: HashMap<MentionId, MentionEntry>,
    searcher: Box<dyn MentionSearcher>,
    /// MRU per user (keyed by `public_key` or `"_offline"`).
    mru: HashMap<String, UserMru>,
    /// Current user key for read/write. Updated on login / logout by the
    /// plugin. Persisted MRU files are named `{public_key}.json`.
    active_user: String,
    /// Universe root whose `.eustress/knowledge/` holds persistence.
    /// Changes on Universe switch; triggers a full rescan + cache reload.
    universe_root: Option<PathBuf>,
    /// Bump on every mutation. UI consumers track this to skip redundant
    /// model rebuilds.
    generation: u64,
}

impl Default for MentionIndex {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            searcher: Box::new(SubstringSearcher::new()),
            mru: HashMap::new(),
            active_user: "_offline".to_string(),
            universe_root: None,
            generation: 0,
        }
    }
}

impl MentionIndex {
    /// Construct with a user-supplied searcher. Used by the plugin when
    /// a richer backend (Vortex / embedvec) is enabled at compile time.
    pub fn with_searcher(searcher: Box<dyn MentionSearcher>) -> Self {
        Self { searcher, ..Self::default() }
    }

    /// Replace the active searcher, rebuilding its state from the current
    /// entry set. Used when the Vortex backend becomes available mid-session
    /// (e.g. after the Universe root is set and the knowledge dir exists).
    pub fn swap_searcher(&mut self, mut searcher: Box<dyn MentionSearcher>) {
        searcher.rebuild(&self.entries);
        self.searcher = searcher;
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn generation(&self) -> u64 { self.generation }
    pub fn entries(&self) -> &HashMap<MentionId, MentionEntry> { &self.entries }
    pub fn get(&self, id: MentionId) -> Option<&MentionEntry> { self.entries.get(&id) }
    pub fn universe_root(&self) -> Option<&Path> { self.universe_root.as_deref() }

    /// Change the active user bucket for MRU read/write. Called on login.
    /// The MRU file for the new user is loaded lazily on first read.
    pub fn set_active_user(&mut self, key: impl Into<String>) {
        let key = key.into();
        if key != self.active_user {
            self.active_user = key;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub fn active_user(&self) -> &str { &self.active_user }

    /// Change which Universe's persistence files back this index. Invokes
    /// a full rescan on next tick.
    pub fn set_universe_root(&mut self, root: Option<PathBuf>) {
        self.universe_root = root;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Top-`k` entries for `query`. If `query` is empty, returns the
    /// current user's MRU list in most-recent-first order.
    pub fn search(&self, query: &str, top_k: usize) -> Vec<&MentionEntry> {
        if query.trim().is_empty() {
            return match self.mru.get(&self.active_user) {
                Some(mru) => mru.queue.iter().rev()
                    .filter_map(|id| self.entries.get(id))
                    .take(top_k)
                    .collect(),
                None => Vec::new(),
            };
        }

        // Over-fetch so MRU boosts can reorder without losing the tail.
        let raw = self.searcher.search(query, top_k.saturating_mul(2).max(10));
        let empty = VecDeque::new();
        let mru = self.mru.get(&self.active_user).map(|m| &m.queue).unwrap_or(&empty);

        // MRU boost: up to +0.10, decaying with position.
        let mut boosted: Vec<(MentionId, f32)> = raw.into_iter()
            .map(|(id, score)| {
                let bonus = mru.iter().position(|i| *i == id)
                    .map(|pos| 0.10 * (1.0 - pos as f32 / mru.len().max(1) as f32))
                    .unwrap_or(0.0);
                (id, score + bonus)
            })
            .collect();
        boosted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        boosted.truncate(top_k);

        boosted.into_iter()
            .filter_map(|(id, _)| self.entries.get(&id))
            .collect()
    }

    /// Record a user commit so the entry surfaces first next time.
    pub fn record_usage(&mut self, id: MentionId) {
        if !self.entries.contains_key(&id) { return; }
        let user = self.active_user.clone();
        let mru = self.mru.entry(user).or_default();
        mru.bump(id);
        self.generation = self.generation.wrapping_add(1);
    }

    /// Upsert one entry. Live ECS entries (source `Ecs`) override static
    /// TOML entries with the same id.
    pub(crate) fn upsert(&mut self, entry: MentionEntry) {
        // Ecs source overrides Toml; don't let a late Toml upsert clobber
        // a fresh Ecs mirror of the same entity.
        if let Some(existing) = self.entries.get(&entry.id) {
            if existing.source == MentionSource::Ecs && entry.source == MentionSource::Toml {
                return;
            }
        }
        self.searcher.upsert(&entry);
        self.entries.insert(entry.id, entry);
        self.generation = self.generation.wrapping_add(1);
    }

    /// Remove by id. MRU entries are retained across removals (since the
    /// same id might reappear if e.g. a file is restored from trash).
    pub(crate) fn remove(&mut self, id: MentionId) {
        self.entries.remove(&id);
        self.searcher.remove(id);
        self.generation = self.generation.wrapping_add(1);
    }

    /// Full rebuild. Called after a Universe scan finishes.
    pub fn rebuild(&mut self, new_entries: HashMap<MentionId, MentionEntry>) {
        self.searcher.rebuild(&new_entries);
        self.entries = new_entries;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Merge one batch of scan results into the existing index. Useful for
    /// incremental re-scan of a single Space without flushing the others.
    pub fn merge_batch(&mut self, batch: Vec<MentionEntry>) {
        for entry in batch {
            self.upsert(entry);
        }
    }

    // ── MRU I/O ───────────────────────────────────────────────────────────

    /// Serialisable snapshot of all MRU buckets.
    pub fn mru_snapshot(&self) -> HashMap<String, UserMru> {
        self.mru.clone()
    }

    /// Replace all MRU buckets. Called on Universe load.
    pub fn set_mru(&mut self, mru: HashMap<String, UserMru>) {
        self.mru = mru;
        self.generation = self.generation.wrapping_add(1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Live enumeration — ECS change detection (current Space only)
// ═══════════════════════════════════════════════════════════════════════════

/// Keeps [`MentionIndex`] in sync with the currently-loaded Space's ECS.
/// Runs every frame but only processes `Added`/`Changed`/`Removed` entities,
/// so quiescent Universes stay at zero cost once loaded.
///
/// This system handles the **live** portion. Cross-Space entities (loaded
/// from TOML scan) are managed by `scan_universe_mentions` (see
/// `mention_scanner` module).
pub fn update_mention_index_live(
    mut index: ResMut<MentionIndex>,
    space_root: Option<Res<crate::space::SpaceRoot>>,
    changed: Query<
        (Entity, &Instance, Option<&crate::space::service_loader::ServiceComponent>),
        (Changed<Instance>, Without<SkipMentionIndex>),
    >,
    mut removed: RemovedComponents<Instance>,
) {
    // We need the current Space's folder name to compute the canonical path.
    let space_name = match space_root.as_deref() {
        Some(sr) => sr.0.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string(),
        None => return, // No Space loaded → nothing live to index.
    };

    for (entity, inst, service_opt) in changed.iter() {
        if inst.class_name.is_adornment() { continue; }

        let kind = if service_opt.is_some() {
            MentionKind::Service
        } else if matches!(inst.class_name, ClassName::SoulScript) {
            MentionKind::Script
        } else {
            MentionKind::Entity
        };

        // Live entries use a synthetic rel_path keyed on the entity index.
        // The TOML scanner uses the real on-disk path; when both exist for
        // the same item the ECS entry wins via `MentionIndex::upsert` rule.
        let rel_path = format!("@ecs/{}", entity.index().index());
        let canonical = MentionEntry::canonical_for(kind, &space_name, &rel_path);
        let id = MentionId::from_canonical(kind, &canonical);

        let qualifier = match kind {
            MentionKind::Service => service_opt
                .map(|s| format!("Service · {}", s.class_name))
                .unwrap_or_else(|| format!("{:?}", inst.class_name)),
            MentionKind::Script => format!("Soul Script · {}", space_name),
            MentionKind::Entity => format!("{:?} · {}", inst.class_name, space_name),
            MentionKind::File => unreachable!(),
        };

        let icon_hint = service_opt
            .map(|s| s.icon.clone())
            .unwrap_or_else(|| class_icon_hint(&inst.class_name));

        index.upsert(MentionEntry {
            id, kind,
            name: inst.name.clone(),
            qualifier,
            canonical_path: canonical,
            space: space_name.clone(),
            rel_path,
            icon_hint,
            source: MentionSource::Ecs,
            entity: Some(entity),
        });
    }

    for entity in removed.read() {
        let idx = entity.index().index();
        let rel = format!("@ecs/{}", idx);
        for kind in [MentionKind::Entity, MentionKind::Script, MentionKind::Service] {
            let canonical = MentionEntry::canonical_for(kind, &space_name, &rel);
            let id = MentionId::from_canonical(kind, &canonical);
            index.remove(id);
        }
    }
}

/// Marker component that opts an entity OUT of the mention index. Use for
/// generated helpers, debug overlays, transient adornments you don't want
/// cluttering the popup. `is_adornment()` classes are auto-excluded;
/// this is for finer control.
#[derive(Component)]
pub struct SkipMentionIndex;

/// Map a `ClassName` to the icon key `load_class_icon` / `load_service_icon`
/// recognise. Falls back to "instance" for anything unknown.
fn class_icon_hint(class: &ClassName) -> String {
    use ClassName as C;
    match class {
        C::Part | C::BasePart => "part",
        C::Model => "model",
        C::Folder => "folder",
        C::Camera => "camera",
        C::SoulScript => "soulservice",
        C::BillboardGui => "billboardgui",
        C::ScreenGui => "screengui",
        C::SurfaceGui => "surfacegui",
        C::TextLabel => "textlabel",
        C::TextButton => "textbutton",
        C::Frame => "frame",
        _ => "instance",
    }
    .to_string()
}

pub(crate) fn space_name_from_root(root: &Path) -> String {
    root.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Active-user tracking — keeps MRU bucket keyed on the logged-in user
// ═══════════════════════════════════════════════════════════════════════════

/// Sync [`MentionIndex::active_user`] with the current [`AuthState`]. When
/// the user logs in, their MRU bucket is addressed by their stable user id
/// (`AuthUser.id`). Offline sessions share the `_offline` bucket so logging
/// out doesn't contaminate other users' MRU on disk.
pub fn sync_mention_active_user(
    auth: Option<Res<crate::auth::AuthState>>,
    mut index: ResMut<MentionIndex>,
) {
    let Some(auth) = auth else { return };
    if !auth.is_changed() { return; }
    let key = auth.user.as_ref()
        .map(|u| u.id.clone())
        .unwrap_or_else(|| "_offline".to_string());
    if key != index.active_user() {
        index.set_active_user(key);
    }
}

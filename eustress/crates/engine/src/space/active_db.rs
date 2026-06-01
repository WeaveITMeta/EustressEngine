//! Process-global handle to the active Space's Fjall `WorldDb` + Space
//! root.
//!
//! ## Why this exists
//!
//! The disk→DB "full conversion": every `.toml` becomes a binary
//! ECS-serialised record in Fjall, no disk TOML in the normal path
//! (disk write speed is the bottleneck the whole pivot exists to kill).
//!
//! The cold *load* path was already threaded through `ActiveSpaceSource`.
//! The hard part is the ~25 edit/tool/hot-reload sites
//! (`move_tool`, `scale_tool`, `select_tool`, `align_distribute`,
//! `tools_smart`, `duplicate_place_tool`, `lock_tool`, `array_tools`,
//! `billboard_gui`, `slint_ui`, `file_watcher`, …) that call
//! `load_instance_definition(&inst_file.toml_path)` /
//! `load_gui_definition(..)` / `write_instance_definition(..)`. They
//! only carry an absolute `toml_path` and have no Bevy resource in
//! scope, so threading a `WorldDb` through every signature is the
//! churn. Instead those few funnel functions consult this global,
//! which is set once when a Space's Fjall DB opens.
//!
//! ## Representation
//!
//! Records are `bincode` of the already-healed `InstanceDefinition` /
//! `GuiTomlFile` (both derive `Serialize`/`Deserialize`), stored in the
//! `tree` partition under `"<space-rel-path>#bin"`. The faithful
//! importer's TOML bytes at `"<space-rel-path>"` are kept as the
//! durable verified copy (the user mandate: never clear the tree — it
//! is the only copy once disk TOML is gone); the binary record is the
//! fast operational form and is lazily materialised from the TOML on
//! first read. After conversion the binary is canonical; TOML-in-tree
//! is the safety net, never on disk.
//!
//! Without the `world-db` feature this is inert (all getters `None`,
//! putters `false`) so a legacy pure-disk build is unchanged. With the
//! feature but no Space open (or a non-migrated disk Space — the global
//! is left unset), the funnel functions fall back to their original
//! disk behaviour, so existing un-converted worlds keep working until
//! `convert-to-eustress` migrates them.

#[cfg(feature = "world-db")]
mod imp {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, RwLock};

    use eustress_worlddb::WorldDb;

    use crate::space::gui_loader::{self, GuiTomlFile};
    use crate::space::instance_loader::{self, InstanceDefinition};

    struct Active {
        db: Arc<dyn WorldDb>,
        root: PathBuf,
    }

    // `RwLock::new` is const since Rust 1.63 — no once_cell needed.
    static ACTIVE: RwLock<Option<Active>> = RwLock::new(None);

    /// Binary-record key suffix. `#` never appears in a Space path
    /// segment and has no `/`, so a `Foo/_instance.toml#bin` key is a
    /// sibling leaf the loader scan already skips (unknown extension).
    const BIN_SUFFIX: &str = "#bin";

    // ── Observability ────────────────────────────────────────────────
    // Aggregated counters, NOT per-entity spam. One summary line every
    // 10k operations + on set/clear, so a single run of a 50k world
    // emits ~6 lines that prove whether the binary DB path is live and
    // whether reads hit binary vs. lazily upgrade from TOML.
    use std::sync::atomic::{AtomicU64, Ordering};
    static BIN_HITS: AtomicU64 = AtomicU64::new(0); // served from #bin record
    static TOML_UPGRADES: AtomicU64 = AtomicU64::new(0); // healed TOML→bincode this read
    static MISSES: AtomicU64 = AtomicU64::new(0); // not in DB → caller hit disk
    static INSTANCE_PUTS: AtomicU64 = AtomicU64::new(0); // binary instance written
    static GUI_HITS: AtomicU64 = AtomicU64::new(0);
    static GUI_PUTS: AtomicU64 = AtomicU64::new(0);

    fn note(counter: &AtomicU64, what: &str) {
        counter.fetch_add(1, Ordering::Relaxed);
        let total = BIN_HITS.load(Ordering::Relaxed)
            + TOML_UPGRADES.load(Ordering::Relaxed)
            + MISSES.load(Ordering::Relaxed)
            + INSTANCE_PUTS.load(Ordering::Relaxed)
            + GUI_HITS.load(Ordering::Relaxed)
            + GUI_PUTS.load(Ordering::Relaxed);
        if total % 10_000 == 0 {
            tracing::warn!(
                target: "eustress_engine::active_db",
                bin_hits = BIN_HITS.load(Ordering::Relaxed),
                toml_upgrades = TOML_UPGRADES.load(Ordering::Relaxed),
                misses = MISSES.load(Ordering::Relaxed),
                instance_puts = INSTANCE_PUTS.load(Ordering::Relaxed),
                gui_hits = GUI_HITS.load(Ordering::Relaxed),
                gui_puts = GUI_PUTS.load(Ordering::Relaxed),
                "active_db tally ({what} crossed a 10k boundary) — bin_hits high == binary-DB path live; toml_upgrades high == first-run lazy conversion; misses high == still hitting disk (NOT converted)"
            );
        }
    }

    /// One-shot snapshot of the counters for an end-of-load summary.
    pub fn stats_summary() -> String {
        format!(
            "bin_hits={} toml_upgrades={} misses={} instance_puts={} gui_hits={} gui_puts={}",
            BIN_HITS.load(Ordering::Relaxed),
            TOML_UPGRADES.load(Ordering::Relaxed),
            MISSES.load(Ordering::Relaxed),
            INSTANCE_PUTS.load(Ordering::Relaxed),
            GUI_HITS.load(Ordering::Relaxed),
            GUI_PUTS.load(Ordering::Relaxed),
        )
    }

    /// Install the active Space's DB. Called from `world_db_plugin`
    /// when it selects the Fjall source for a Space.
    pub fn set(db: Arc<dyn WorldDb>, root: PathBuf) {
        if let Ok(mut g) = ACTIVE.write() {
            tracing::warn!(
                target: "eustress_engine::active_db",
                space = %root.display(),
                "BINARY ECS STORE ACTIVE — load_instance_definition / write_instance_definition / load_gui_definition / write_gui_toml now read+write bincode from Fjall (disk only as legacy fallback)"
            );
            *g = Some(Active { db, root });
        }
    }

    /// Drop the active DB (Space switch, or fell back to a legacy disk
    /// source — funnels then resume their original disk behaviour).
    pub fn clear() {
        if let Ok(mut g) = ACTIVE.write() {
            if g.is_some() {
                tracing::warn!(
                    target: "eustress_engine::active_db",
                    final_tally = %stats_summary(),
                    "binary ECS store CLEARED (Space switch / disk fallback) — funnels revert to disk"
                );
            }
            *g = None;
        }
    }

    /// True when a Fjall DB is active (diagnostics / callers that want
    /// to skip a disk write entirely).
    pub fn is_active() -> bool {
        ACTIVE.read().map(|g| g.is_some()).unwrap_or(false)
    }

    fn rel_key(root: &Path, abs: &Path) -> Option<String> {
        abs.strip_prefix(root)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
    }

    /// True when an instance MUST stay on the filesystem and must never be
    /// collapsed into a DB `.bin` binary-ECS record:
    ///   - a file-natured class (scripts, GUI, documents — see the router), or
    ///   - a part carrying a custom (non-`parts/`) mesh whose RELATIVE path a
    ///     binary core cannot resolve (it has only a synthetic path, no folder).
    /// Binarising either strands its real-file artifacts — that's how V-Cell
    /// lost its custom housing meshes and a BillboardGui lost its child labels.
    fn instance_stays_filesystem(def: &InstanceDefinition) -> bool {
        let mesh = def.asset.as_ref().map(|a| a.mesh.as_str());
        crate::space::representation::class_is_file_natured(&def.metadata.class_name)
            || mesh.map(crate::space::representation::mesh_requires_filesystem).unwrap_or(false)
    }

    /// True when the entity's on-disk folder owns at least one CHILD entity
    /// (a subfolder carrying its own `_instance.toml`). Such a parent MUST
    /// stay folder-form (FileSystem): a binary `.bin` is a single flat
    /// record with no folder, so collapsing a parent-with-children strands
    /// and then trashes them. This is the same failure `put_gui` already
    /// guards file-natured GUI against — but a plain `Part` that has gained
    /// children (e.g. a MindSpace BillboardGui attached to it) hits it too:
    /// user-reported, moving such a Part trashed its billboard on every
    /// release (8 `Label-*` copies accumulated in the Part's `.eustress/trash`).
    /// The engine's own `.eustress/` trash dir and hidden dirs are skipped.
    fn folder_has_child_entities(abs: &Path) -> bool {
        let Some(folder) = abs.parent() else {
            return false;
        };
        // RECURSIVE (2026-05-24): detect a descendant entity at ANY depth, not
        // just direct children. A MindSpace label nests Block → BillboardGui →
        // TextLabel; the one-level check kept the Block (it sees BillboardGui)
        // and the BillboardGui (it sees TextLabel), but any path that collapsed
        // a mid-level folder could still strand the deeper TextLabel and the
        // stale-cleanup sweep then despawned it ("moving a block deletes the
        // billboard's text label"). Walking the whole subtree means a parent
        // with descendants at any depth always stays folder-form (FileSystem)
        // and never binarises into a flat record that orphans them.
        fn any_descendant(dir: &Path) -> bool {
            let Ok(read_dir) = std::fs::read_dir(dir) else {
                return false;
            };
            read_dir.flatten().any(|entry| {
                let p = entry.path();
                if !p.is_dir() {
                    return false;
                }
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with('.') {
                    return false; // skip `.eustress/` trash + hidden dirs
                }
                p.join("_instance.toml").is_file() || any_descendant(&p)
            })
        }
        any_descendant(folder)
    }

    /// Instance definition for an absolute in-Space path, from the DB.
    /// Binary fast-path → else the importer's TOML bytes (healed, then
    /// lazily upgraded to a binary record) → else `None` (caller falls
    /// back to disk for a not-yet-converted world).
    pub fn get_instance(abs: &Path) -> Option<InstanceDefinition> {
        let g = ACTIVE.read().ok()?;
        let a = g.as_ref()?;
        let rel = rel_key(&a.root, abs)?;

        // Binary fast-path — but NEVER let a stale `.bin` shadow a part that
        // must stay on the filesystem (custom mesh / file-natured). An older
        // build may have wrongly binarised it; prefer its authoritative TOML.
        if let Ok(Some(bytes)) = a.db.get_file(&format!("{rel}{BIN_SUFFIX}")) {
            if let Ok(def) = bincode::deserialize::<InstanceDefinition>(&bytes) {
                if !instance_stays_filesystem(&def) && !folder_has_child_entities(abs) {
                    note(&BIN_HITS, "instance bin-hit");
                    return Some(def);
                }
            }
        }
        if let Ok(Some(toml_bytes)) = a.db.get_file(&rel) {
            if let Ok(s) = std::str::from_utf8(&toml_bytes) {
                if let Ok(def) = instance_loader::load_instance_definition_from_str(s) {
                    // Only upgrade to a binary twin for entities that may live
                    // in binary-ECS. Custom-mesh / file-natured parts stay TOML.
                    if !instance_stays_filesystem(&def) && !folder_has_child_entities(abs) {
                        if let Ok(bin) = bincode::serialize(&def) {
                            let _ = a.db.put_file(&format!("{rel}{BIN_SUFFIX}"), &bin);
                        }
                        note(&TOML_UPGRADES, "instance toml→bin upgrade");
                    } else {
                        note(&TOML_UPGRADES, "instance toml (filesystem-kept)");
                    }
                    return Some(def);
                }
            }
        }
        note(&MISSES, "instance miss (caller falls back to disk)");
        None
    }

    /// Persist an instance definition as the binary ECS record. No
    /// disk, no TOML. Returns `false` when no DB is active (caller then
    /// does its legacy disk write).
    pub fn put_instance(abs: &Path, def: &InstanceDefinition) -> bool {
        let Ok(g) = ACTIVE.read() else {
            return false;
        };
        let Some(a) = g.as_ref() else {
            return false;
        };
        let Some(rel) = rel_key(&a.root, abs) else {
            return false;
        };
        // Custom-mesh / file-natured instances, OR any instance that HAS
        // CHILDREN, must persist as filesystem TOML — never a flat DB `.bin`
        // (a binary core can't hold a resolvable mesh path, and a flat record
        // has no folder so it strands child entities). Returning false routes
        // the caller to its disk-TOML write path, preserving the children.
        if instance_stays_filesystem(def) || folder_has_child_entities(abs) {
            // Undo any earlier wrong collapse: drop a stale binary twin so the
            // authoritative folder-form TOML (and its children) is served
            // again instead of the flat record that stranded them.
            let _ = a.db.delete_file(&format!("{rel}{BIN_SUFFIX}"));
            return false;
        }
        match bincode::serialize(def) {
            Ok(bin) => {
                let ok = a.db.put_file(&format!("{rel}{BIN_SUFFIX}"), &bin).is_ok();
                if ok {
                    note(&INSTANCE_PUTS, "instance binary write");
                }
                ok
            }
            Err(_) => false,
        }
    }

    // ── Binary-ECS instance cores (entities partition, Morton-keyed) ──
    //
    // These are the PURE binary-ECS representation: a rkyv
    // `ArchInstanceCore` keyed by spatial Morton position in the
    // `entities` partition, with NO disk path and NO `tree` entry. They
    // are the scalable Insert-menu default (a bare Part). The funnel
    // mirrors the `#bin` helpers above but lives behind these free
    // functions so non-feature-gated call sites (the Insert handler in
    // `slint_ui`) can reach the store without plumbing a `WorldDbHandle`
    // resource or `#[cfg]`-ing every call site — the same reason
    // `get_instance` / `put_instance` exist.
    //
    // `stored_id` is the STABLE persistence id, NOT the live Bevy
    // `Entity::to_bits()` (those are not stable across sessions). The
    // engine mints it once at create time and preserves it across load.

    /// Persist a binary-ECS core (rkyv `ArchInstanceCore` bytes),
    /// Morton-keyed by `pos`. `false` when no DB is active or the write
    /// failed (caller keeps the in-memory entity; the save mirror retries
    /// on the next change).
    pub fn put_instance_core(stored_id: u64, pos: [f32; 3], core: &[u8]) -> bool {
        let Ok(g) = ACTIVE.read() else {
            return false;
        };
        let Some(a) = g.as_ref() else {
            return false;
        };
        a.db
            .put_instance_core(
                eustress_worlddb::EntityId(stored_id),
                (pos[0], pos[1], pos[2]),
                core,
            )
            .is_ok()
    }

    /// Delete a binary-ECS core at the position its Morton key was last
    /// computed from. A *move* deletes at the OLD position before putting
    /// at the new one (the key is position-derived).
    pub fn delete_instance_core(stored_id: u64, pos: [f32; 3]) -> bool {
        let Ok(g) = ACTIVE.read() else {
            return false;
        };
        let Some(a) = g.as_ref() else {
            return false;
        };
        a.db
            .delete_instance_core(eustress_worlddb::EntityId(stored_id), (pos[0], pos[1], pos[2]))
            .is_ok()
    }

    /// Persist a live binary-ECS entity's edit to BOTH cores in one call —
    /// the Morton spatial core (boot-load / streaming reads this) AND the
    /// UUID-primary core in `entities_uuid` (`find_entity --uuid` and the
    /// bridge `entity.read` of a NON-resident entity read this). The
    /// `mirror_binary_ecs_changes` system previously wrote ONLY the Morton
    /// core, so after any resident edit the uuid-primary copy went stale
    /// (an edited-then-evicted entity read back its pre-edit values). A
    /// position change moves the Morton key (delete-old + put-new); the
    /// uuid key is position-independent so it's a plain overwrite. `uuid`
    /// is `None` only defensively (a binary entity always carries one).
    /// Returns whether the Morton write (the canonical persist) succeeded.
    pub fn mirror_binary_core(
        stored_id: u64,
        uuid: Option<&[u8; 16]>,
        old_pos: [f32; 3],
        new_pos: [f32; 3],
        core: &[u8],
    ) -> bool {
        let Ok(g) = ACTIVE.read() else {
            return false;
        };
        let Some(a) = g.as_ref() else {
            return false;
        };
        let eid = eustress_worlddb::EntityId(stored_id);
        if new_pos != old_pos {
            let _ = a
                .db
                .delete_instance_core(eid, (old_pos[0], old_pos[1], old_pos[2]));
        }
        let morton_ok = a
            .db
            .put_instance_core(eid, (new_pos[0], new_pos[1], new_pos[2]), core)
            .is_ok();
        if let Some(u) = uuid {
            if a.db.put_entity_core_by_uuid(u, core).is_err() {
                tracing::warn!(
                    target: "eustress_engine::active_db",
                    stored_id,
                    "mirror_binary_core: uuid-primary core write failed (find-by-uuid stale until next edit / rebuild_indexes)"
                );
            }
        }
        morton_ok
    }

    /// Remove a folder-form entity's DB records — the `{rel}` tree TOML AND its
    /// `{rel}.bin` binary twin — so a deleted/trashed entity does NOT resurrect
    /// from the DB on the next session. The scene is DB-primary, so trashing the
    /// disk `_instance.toml` alone leaves the authoritative record in
    /// `world.fjalldb` behind (the reported "delete comes back next session"
    /// bug). `abs` is the entity's `_instance.toml` path — the same key
    /// `put_instance`/`put_gui` write under.
    pub fn delete_path(abs: &Path) -> bool {
        let Ok(g) = ACTIVE.read() else {
            return false;
        };
        let Some(a) = g.as_ref() else {
            return false;
        };
        let Some(rel) = rel_key(&a.root, abs) else {
            return false;
        };
        let removed_toml = a.db.delete_file(&rel).is_ok();
        let removed_bin = a.db.delete_file(&format!("{rel}{BIN_SUFFIX}")).is_ok();
        removed_toml || removed_bin
    }

    /// Eager snapshot of every binary-ECS core in the active Space's
    /// `entities` partition — the boot-load path. Empty when no DB is
    /// active or the partition holds no cores (the common legacy case).
    pub fn iter_instance_cores() -> Vec<(u64, Vec<u8>)> {
        let Ok(g) = ACTIVE.read() else {
            return Vec::new();
        };
        let Some(a) = g.as_ref() else {
            return Vec::new();
        };
        a.db
            .iter_instance_cores()
            .map(|v| v.into_iter().map(|(e, b)| (e.0, b)).collect())
            .unwrap_or_default()
    }

    /// Region scan for camera-locality streaming: cores whose Morton cell
    /// lies in the inclusive cell box. Empty when no DB is active.
    pub fn iter_instance_cores_in_region(
        cx: (u32, u32),
        cy: (u32, u32),
        cz: (u32, u32),
    ) -> Vec<(u64, Vec<u8>)> {
        let Ok(g) = ACTIVE.read() else {
            return Vec::new();
        };
        let Some(a) = g.as_ref() else {
            return Vec::new();
        };
        a.db
            .iter_instance_cores_in_region(cx, cy, cz)
            .map(|v| v.into_iter().map(|(e, b)| (e.0, b)).collect())
            .unwrap_or_default()
    }

    /// Capped count of binary-ECS cores (stops at `cap`). Used to gate
    /// boot-load-all vs streaming. 0 when no DB is active.
    pub fn count_instance_cores_capped(cap: usize) -> usize {
        let Ok(g) = ACTIVE.read() else {
            return 0;
        };
        let Some(a) = g.as_ref() else {
            return 0;
        };
        a.db.count_instance_cores_capped(cap).unwrap_or(0)
    }

    /// Every distinct class in `class_index` with its entity count, sorted
    /// by class name. Powers the virtual DB-backed Explorer (Phase 4): list
    /// `Part (2000000)` etc. without materializing any cores. Empty when no
    /// DB is active (small/legacy disk Space — Explorer stays live-ECS-only).
    pub fn iter_all_classes() -> Vec<(String, usize)> {
        let Ok(g) = ACTIVE.read() else {
            return Vec::new();
        };
        let Some(a) = g.as_ref() else {
            return Vec::new();
        };
        a.db.iter_all_classes().unwrap_or_default()
    }

    /// Lowercase-hex (32-char) encode a 16-byte uuid — the canonical wire
    /// form `find_entity_by_uuid` validates and the bridge addresses by.
    fn uuid_bytes_to_hex(b: &[u8; 16]) -> String {
        let mut s = String::with_capacity(32);
        for byte in b {
            s.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
            s.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
        }
        s
    }

    /// A bounded page of DB-only entities in a class for the virtual
    /// Explorer (Phase 4): `(uuid_hex_32, display_name)` pairs, at most
    /// `cap`. Reads only `cap` cores — `iter_class_capped` early-exits, so a
    /// 10M-entity `Part` bucket never materializes 10M uuids just to show
    /// the first page. The name comes from the core's `metadata.name`
    /// (falling back to the class name). Empty when no DB is active.
    pub fn list_class_page(class_name: &str, cap: usize) -> Vec<(String, String)> {
        let Ok(g) = ACTIVE.read() else {
            return Vec::new();
        };
        let Some(a) = g.as_ref() else {
            return Vec::new();
        };
        let uuids = a.db.iter_class_capped(class_name, cap).unwrap_or_default();
        let mut out = Vec::with_capacity(uuids.len());
        for u in uuids {
            let hex = uuid_bytes_to_hex(&u);
            let name = a
                .db
                .get_entity_core_by_uuid(&u)
                .ok()
                .flatten()
                .and_then(|buf| eustress_worlddb::decode_instance_core(&buf).ok())
                .and_then(|core| {
                    crate::space::arch_instance::arch_to_instance(&core)
                        .metadata
                        .name
                })
                .unwrap_or_else(|| class_name.to_string());
            out.push((hex, name));
        }
        out
    }

    /// Create a binary-ECS entity in ALL FIVE stores in one call — the
    /// chokepoint for the "Insert defaults to scalable" create-flip
    /// (SCALING_ARCHITECTURE.md §0.5 C1). A binary entity must be findable
    /// by uuid / path / class exactly like a folder-form TOML entity, so
    /// creating one means writing:
    ///   1. the Morton-keyed spatial core (`entities`)  — boot-load reads it
    ///   2. the UUID-keyed primary core (`entities_uuid`) — IDENTITY §5.2
    ///   3. `path_to_uuid` (synthetic in-Space path → uuid)
    ///   4. `uuid_to_path` (reverse)
    ///   5. `class_index/<class>/<uuid>` (so `iter_class` returns it)
    ///
    /// `put_instance_core` alone (what the boot-load mirror uses) populates
    /// ONLY store 1, so a part written that way is invisible to
    /// `find_entity --uuid/--path/--class`. This helper closes that gap.
    ///
    /// There is no atomic core+index write in the trait (see
    /// `migrate_identity.rs`, which makes the same calls in sequence and
    /// relies on `rebuild_indexes()` for crash recovery). We therefore do
    /// best-effort writes and WARN on any partial failure rather than
    /// failing the whole create — the worst case is a missing secondary
    /// index, which `rebuild_indexes()` reconstructs from the primary core.
    ///
    /// `core` is the SAME tagged rkyv `ArchInstanceCore` bytes for stores
    /// 1 and 2 (the partitions store identical bytes — confirmed in
    /// `fjall_backend`). Returns `false` when no DB is active (caller then
    /// keeps the in-memory entity / falls back to its TOML create path).
    pub fn create_binary_instance(
        stored_id: u64,
        uuid: &[u8; 16],
        class_name: &str,
        pos: [f32; 3],
        core: &[u8],
        synthetic_rel: &str,
    ) -> bool {
        let Ok(g) = ACTIVE.read() else {
            return false;
        };
        let Some(a) = g.as_ref() else {
            return false;
        };
        let eid = eustress_worlddb::EntityId(stored_id);
        // Store 1: Morton spatial core (the boot-load source of truth).
        let core_ok = a
            .db
            .put_instance_core(eid, (pos[0], pos[1], pos[2]), core)
            .is_ok();
        // Stores 2–5: identity. Best-effort; warn (not fail) on partial
        // write — rebuild_indexes() can reconstruct any missing index.
        let mut index_failures: Vec<&str> = Vec::new();
        if a.db.put_entity_core_by_uuid(uuid, core).is_err() {
            index_failures.push("entities_uuid");
        }
        if a.db.put_path_to_uuid(synthetic_rel, uuid).is_err() {
            index_failures.push("path_to_uuid");
        }
        if a.db.put_uuid_to_path(uuid, synthetic_rel).is_err() {
            index_failures.push("uuid_to_path");
        }
        if a.db.put_class_index(class_name, uuid).is_err() {
            index_failures.push("class_index");
        }
        if !core_ok {
            tracing::warn!(
                target: "eustress_engine::active_db",
                stored_id, class_name,
                "create_binary_instance: Morton core write FAILED — entity not persisted (kept in ECS this session only)"
            );
        }
        if !index_failures.is_empty() {
            tracing::warn!(
                target: "eustress_engine::active_db",
                stored_id, class_name,
                failed = ?index_failures,
                "create_binary_instance: identity index write(s) failed — entity persisted but not yet fully indexed (rebuild_indexes recovers)"
            );
        } else if core_ok {
            note(&INSTANCE_PUTS, "binary instance create (5-store)");
        }
        core_ok && index_failures.is_empty()
    }

    /// Symmetric teardown for [`create_binary_instance`]: remove the entity
    /// from all five stores so a deleted binary part does NOT resurrect
    /// from the DB on the next boot-load (and is no longer found by uuid /
    /// path / class). Best-effort; a delete of a missing key is harmless.
    /// `pos` MUST be the entity's last-persisted (Morton-key) position.
    pub fn delete_binary_instance(
        stored_id: u64,
        uuid: &[u8; 16],
        class_name: &str,
        pos: [f32; 3],
        synthetic_rel: &str,
    ) -> bool {
        let Ok(g) = ACTIVE.read() else {
            return false;
        };
        let Some(a) = g.as_ref() else {
            return false;
        };
        let eid = eustress_worlddb::EntityId(stored_id);
        let core_removed = a
            .db
            .delete_instance_core(eid, (pos[0], pos[1], pos[2]))
            .is_ok();
        let _ = a.db.delete_entity_by_uuid(uuid);
        let _ = a.db.delete_path_to_uuid(synthetic_rel);
        let _ = a.db.delete_uuid_to_path(uuid);
        let _ = a.db.delete_class_index(class_name, uuid);
        core_removed
    }

    /// Phase 3.5 PROMOTE — write the REAL-path FileSystem identity for an
    /// entity being materialized from a binary core to an on-disk TOML folder.
    /// The one-shot `migrate_identity` pass (which populates these for TOML
    /// entities) is latched-done and never re-runs, so a runtime promote must
    /// write them itself. Pairs with `delete_binary_instance(synthetic_rel)`
    /// (call that FIRST to drop the binary Morton core + synthetic-path stores
    /// so the core does not resurrect on boot-load), then this to register the
    /// real path. `real_rel` is the Space-relative `Workspace/<Name>/_instance.toml`
    /// (forward-slashed). `core` = the same tagged rkyv bytes; `toml` = the
    /// folder's `_instance.toml` bytes (so a converted Space's `get_instance`
    /// serves it). Best-effort; warns on partial failure.
    pub fn write_filesystem_identity(
        uuid: &[u8; 16],
        class_name: &str,
        real_rel: &str,
        core: &[u8],
        toml: &[u8],
    ) -> bool {
        let Ok(g) = ACTIVE.read() else {
            return false;
        };
        let Some(a) = g.as_ref() else {
            return false;
        };
        let mut failures: Vec<&str> = Vec::new();
        if a.db.put_path_to_uuid(real_rel, uuid).is_err() {
            failures.push("path_to_uuid");
        }
        if a.db.put_uuid_to_path(uuid, real_rel).is_err() {
            failures.push("uuid_to_path");
        }
        if a.db.put_class_index(class_name, uuid).is_err() {
            failures.push("class_index");
        }
        if a.db.put_entity_core_by_uuid(uuid, core).is_err() {
            failures.push("entities_uuid");
        }
        if a.db.put_file(real_rel, toml).is_err() {
            failures.push("tree_toml");
        }
        if !failures.is_empty() {
            tracing::warn!(
                target: "eustress_engine::active_db",
                class_name, real_rel, failed = ?failures,
                "write_filesystem_identity: partial write (rebuild_indexes recovers)"
            );
        }
        failures.is_empty()
    }

    /// Phase 3.5 DEMOTE — drop the REAL-path FileSystem identity when folding
    /// a TOML folder back into a binary core. Pairs with
    /// `create_binary_instance(synthetic_rel)` (call that to re-create the
    /// binary stores; it rewrites `uuid_to_path`/`class_index`/`entities_uuid`
    /// to the synthetic path). This removes the real-path `path_to_uuid` + the
    /// tree TOML (+ any lazily-written `#bin` twin) so the disk path no longer
    /// resolves. Best-effort.
    pub fn remove_filesystem_identity(real_rel: &str) -> bool {
        let Ok(g) = ACTIVE.read() else {
            return false;
        };
        let Some(a) = g.as_ref() else {
            return false;
        };
        let _ = a.db.delete_path_to_uuid(real_rel);
        let _ = a.db.delete_file(real_rel);
        let _ = a.db.delete_file(&format!("{real_rel}{BIN_SUFFIX}"));
        true
    }

    /// GUI definition twin of [`get_instance`].
    pub fn get_gui(abs: &Path) -> Option<GuiTomlFile> {
        let g = ACTIVE.read().ok()?;
        let a = g.as_ref()?;
        let rel = rel_key(&a.root, abs)?;

        // Binary fast-path — but ONLY for non-file-natured GUI. A stale `.bin`
        // must never shadow the TOML of a file-natured class (BillboardGui/
        // TextLabel/…); those are authoritative on the filesystem and may have
        // been left behind by an older build that wrongly binarised them.
        if let Ok(Some(bytes)) = a.db.get_file(&format!("{rel}{BIN_SUFFIX}")) {
            if let Ok(def) = bincode::deserialize::<GuiTomlFile>(&bytes) {
                if !crate::space::representation::class_is_file_natured(&def.metadata.class_name) {
                    note(&GUI_HITS, "gui bin-hit");
                    return Some(def);
                }
            }
        }
        if let Ok(Some(toml_bytes)) = a.db.get_file(&rel) {
            if let Ok(s) = std::str::from_utf8(&toml_bytes) {
                if let Ok(def) = gui_loader::load_gui_definition_from_str(s) {
                    // Upgrade the TOML to a binary twin only for non-file-natured
                    // GUI. File-natured classes stay TOML-only so they remain
                    // FileSystem-represented (children intact).
                    if !crate::space::representation::class_is_file_natured(&def.metadata.class_name) {
                        if let Ok(bin) = bincode::serialize(&def) {
                            let _ = a.db.put_file(&format!("{rel}{BIN_SUFFIX}"), &bin);
                        }
                        note(&GUI_HITS, "gui toml→bin upgrade");
                    } else {
                        note(&GUI_HITS, "gui toml (file-natured, kept on filesystem)");
                    }
                    return Some(def);
                }
            }
        }
        None
    }

    /// GUI definition twin of [`put_instance`].
    pub fn put_gui(abs: &Path, def: &GuiTomlFile) -> bool {
        // File-natured GUI (BillboardGui, TextLabel, Frame, …) is authored and
        // edited as filesystem TOML and routinely owns child files — a
        // BillboardGui's TextLabel lives in a sibling/child folder. Collapsing
        // the parent into a single DB `.bin` strands those children (the
        // reported "edit a property and the label vanishes" bug). It must NEVER
        // become a binary twin: return false so `write_gui_toml` takes the
        // disk-TOML path, honoring the FileSystem representation the router
        // already mandates via `class_is_file_natured`.
        if crate::space::representation::class_is_file_natured(&def.metadata.class_name) {
            return false;
        }
        let Ok(g) = ACTIVE.read() else {
            return false;
        };
        let Some(a) = g.as_ref() else {
            return false;
        };
        let Some(rel) = rel_key(&a.root, abs) else {
            return false;
        };
        match bincode::serialize(def) {
            Ok(bin) => {
                let ok = a.db.put_file(&format!("{rel}{BIN_SUFFIX}"), &bin).is_ok();
                if ok {
                    note(&GUI_PUTS, "gui binary write");
                }
                ok
            }
            Err(_) => false,
        }
    }

    /// Peek `[metadata] class_name` for a folder-form `_instance.toml`
    /// from the DB (replaces the raw `std::fs::read_to_string` in
    /// `gui_class_from_extension`). `None` → caller falls back to disk.
    pub fn peek_class_name(abs: &Path) -> Option<String> {
        let g = ACTIVE.read().ok()?;
        let a = g.as_ref()?;
        let rel = rel_key(&a.root, abs)?;
        let bytes = a.db.get_file(&rel).ok().flatten()?;
        let s = std::str::from_utf8(&bytes).ok()?;
        let doc: toml::Value = toml::from_str(s).ok()?;
        eustress_common::class_schema::get_section_insensitive(&doc, "metadata")
            .and_then(|m| eustress_common::class_schema::get_section_insensitive(m, "class_name"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
    }
}

#[cfg(not(feature = "world-db"))]
mod imp {
    use std::path::Path;

    use crate::space::gui_loader::GuiTomlFile;
    use crate::space::instance_loader::InstanceDefinition;

    pub fn clear() {}
    pub fn is_active() -> bool {
        false
    }
    pub fn get_instance(_abs: &Path) -> Option<InstanceDefinition> {
        None
    }
    pub fn put_instance(_abs: &Path, _def: &InstanceDefinition) -> bool {
        false
    }
    pub fn put_instance_core(_stored_id: u64, _pos: [f32; 3], _core: &[u8]) -> bool {
        false
    }
    pub fn delete_instance_core(_stored_id: u64, _pos: [f32; 3]) -> bool {
        false
    }
    pub fn delete_path(_abs: &Path) -> bool {
        false
    }
    pub fn iter_instance_cores() -> Vec<(u64, Vec<u8>)> {
        Vec::new()
    }
    pub fn iter_instance_cores_in_region(
        _cx: (u32, u32),
        _cy: (u32, u32),
        _cz: (u32, u32),
    ) -> Vec<(u64, Vec<u8>)> {
        Vec::new()
    }
    pub fn count_instance_cores_capped(_cap: usize) -> usize {
        0
    }
    pub fn iter_all_classes() -> Vec<(String, usize)> {
        Vec::new()
    }
    pub fn list_class_page(_class_name: &str, _cap: usize) -> Vec<(String, String)> {
        Vec::new()
    }
    pub fn mirror_binary_core(
        _stored_id: u64,
        _uuid: Option<&[u8; 16]>,
        _old_pos: [f32; 3],
        _new_pos: [f32; 3],
        _core: &[u8],
    ) -> bool {
        false
    }
    pub fn create_binary_instance(
        _stored_id: u64,
        _uuid: &[u8; 16],
        _class_name: &str,
        _pos: [f32; 3],
        _core: &[u8],
        _synthetic_rel: &str,
    ) -> bool {
        false
    }
    pub fn delete_binary_instance(
        _stored_id: u64,
        _uuid: &[u8; 16],
        _class_name: &str,
        _pos: [f32; 3],
        _synthetic_rel: &str,
    ) -> bool {
        false
    }
    pub fn write_filesystem_identity(
        _uuid: &[u8; 16],
        _class_name: &str,
        _real_rel: &str,
        _core: &[u8],
        _toml: &[u8],
    ) -> bool {
        false
    }
    pub fn remove_filesystem_identity(_real_rel: &str) -> bool {
        false
    }
    pub fn get_gui(_abs: &Path) -> Option<GuiTomlFile> {
        None
    }
    pub fn put_gui(_abs: &Path, _def: &GuiTomlFile) -> bool {
        false
    }
    pub fn peek_class_name(_abs: &Path) -> Option<String> {
        None
    }
}

pub use imp::*;

// ── Non-gated streaming signal (Phase 4) ────────────────────────────────
//
// The Explorer (`slint_ui::sync_unified_explorer_to_slint`) compiles in
// BOTH feature modes and cannot reference the feature-gated `ResidencyState`.
// So the residency boot-load decision mirrors its enabled/disabled state
// into this process-global flag, which the Explorer reads to decide whether
// to render the virtual "Database (streamed)" section. False for small
// Spaces (everything boot-loaded → no DB-only rows), legacy disk Spaces, and
// whenever the `world-db` feature is off.
use std::sync::atomic::{AtomicBool, Ordering};

static STREAMING_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Is camera-locality streaming active for the current Space? (Large binary
/// Space whose cores are streamed by camera locality rather than all
/// boot-loaded.) Read by the Explorer to gate the virtual DB section.
pub fn streaming_active() -> bool {
    STREAMING_ACTIVE.load(Ordering::Relaxed)
}

/// Set by the residency boot-load decision (feature-gated) and the Space
/// teardown (reset to false). Non-gated so both callers compile.
pub fn set_streaming_active(v: bool) {
    STREAMING_ACTIVE.store(v, Ordering::Relaxed);
}

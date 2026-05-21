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

    /// Instance definition for an absolute in-Space path, from the DB.
    /// Binary fast-path → else the importer's TOML bytes (healed, then
    /// lazily upgraded to a binary record) → else `None` (caller falls
    /// back to disk for a not-yet-converted world).
    pub fn get_instance(abs: &Path) -> Option<InstanceDefinition> {
        let g = ACTIVE.read().ok()?;
        let a = g.as_ref()?;
        let rel = rel_key(&a.root, abs)?;

        if let Ok(Some(bytes)) = a.db.get_file(&format!("{rel}{BIN_SUFFIX}")) {
            if let Ok(def) = bincode::deserialize::<InstanceDefinition>(&bytes) {
                note(&BIN_HITS, "instance bin-hit");
                return Some(def);
            }
        }
        if let Ok(Some(toml_bytes)) = a.db.get_file(&rel) {
            if let Ok(s) = std::str::from_utf8(&toml_bytes) {
                if let Ok(def) = instance_loader::load_instance_definition_from_str(s) {
                    if let Ok(bin) = bincode::serialize(&def) {
                        let _ = a.db.put_file(&format!("{rel}{BIN_SUFFIX}"), &bin);
                    }
                    note(&TOML_UPGRADES, "instance toml→bin upgrade");
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

    /// GUI definition twin of [`get_instance`].
    pub fn get_gui(abs: &Path) -> Option<GuiTomlFile> {
        let g = ACTIVE.read().ok()?;
        let a = g.as_ref()?;
        let rel = rel_key(&a.root, abs)?;

        if let Ok(Some(bytes)) = a.db.get_file(&format!("{rel}{BIN_SUFFIX}")) {
            if let Ok(def) = bincode::deserialize::<GuiTomlFile>(&bytes) {
                note(&GUI_HITS, "gui bin-hit");
                return Some(def);
            }
        }
        if let Ok(Some(toml_bytes)) = a.db.get_file(&rel) {
            if let Ok(s) = std::str::from_utf8(&toml_bytes) {
                if let Ok(def) = gui_loader::load_gui_definition_from_str(s) {
                    if let Ok(bin) = bincode::serialize(&def) {
                        let _ = a.db.put_file(&format!("{rel}{BIN_SUFFIX}"), &bin);
                    }
                    note(&GUI_HITS, "gui toml→bin upgrade");
                    return Some(def);
                }
            }
        }
        None
    }

    /// GUI definition twin of [`put_instance`].
    pub fn put_gui(abs: &Path, def: &GuiTomlFile) -> bool {
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

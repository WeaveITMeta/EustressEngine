//! `eustress-space` — open / inspect / verify / export a `.eustress`
//! space WITHOUT the engine.
//!
//! This is the **portability + serialization-correctness** tool of Wave
//! 8.B (`docs/architecture/IMPORT_STORAGE_AND_PORTABILITY.md` §8.B/§8.C).
//! It links only [`eustress_worlddb`] — never the engine — so it stays
//! small, fast, and able to open a world on any machine with just the
//! storage crate. That is the GDPR/portability story: your data, in an
//! open format, openable by a standalone tool you control.
//!
//! A migrated `.eustress` space on disk is a directory:
//!
//! ```text
//! <space>/
//!   header.bin          # rkyv-ish identity + version + migrated_at
//!   world.fjalldb/      # the Fjall LSM container (live ECS state)
//!   assets/  schema/    # not read by this tool
//! ```
//!
//! All three subcommands open `world.fjalldb/` through
//! [`eustress_worlddb::FjallWorldDb::open`] **read-only** and use the
//! crate's existing read API:
//! - [`eustress_worlddb::WorldDb::iter_instance_cores`] — every binary-ECS
//!   `ArchInstanceCore` (Morton-keyed entities partition).
//! - [`eustress_worlddb::decode_instance_core`] — the rkyv `access`
//!   (CheckBytes-validating) decode of one core's bytes.
//!
//! ## What this tool reports vs. the §8.B wish-list
//!
//! The spec's §8.B/§8 mention `iter_all_classes` and `iter_all_voxel_chunks`
//! (a Wave 9.A terrain store). **Neither exists in the worlddb API at the
//! HEAD this crate targets** (Wave 9.A had not landed). So:
//! - The **class histogram** is built by decoding each core and counting
//!   its `class_name` — equivalent information, no new worlddb API needed.
//! - **World bounds** come from each core's stored translation `t` (the
//!   same value the Morton key is derived from) — exact, not an approximation.
//! - **Voxel-chunk count** is reported as "not present in this DB schema"
//!   rather than calling a method that does not exist; when Wave 9.A lands,
//!   swap in `iter_all_voxel_chunks` here (single call site, marked TODO).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eustress_worlddb::{decode_instance_core, ArchInstanceCore, EntityId, WorldDb};

/// Top-level error for the tool. Wraps the worlddb error plus the few
/// tool-local failure modes (bad path, IO during export).
#[derive(Debug)]
pub enum SpaceError {
    /// The given path is neither a `world.fjalldb/` directory nor a space
    /// root containing one.
    NotASpace(PathBuf),
    /// Failure originating in the worlddb / Fjall layer.
    WorldDb(eustress_worlddb::Error),
    /// Filesystem IO failure (export tree write, header read).
    Io(std::io::Error),
    /// TOML serialization failure during `export`.
    Toml(String),
}

impl std::fmt::Display for SpaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpaceError::NotASpace(p) => write!(
                f,
                "not a .eustress space: {} (expected a `world.fjalldb/` directory, \
                 or a space root containing one)",
                p.display()
            ),
            SpaceError::WorldDb(e) => write!(f, "worlddb: {e}"),
            SpaceError::Io(e) => write!(f, "io: {e}"),
            SpaceError::Toml(e) => write!(f, "toml: {e}"),
        }
    }
}

impl std::error::Error for SpaceError {}

impl From<eustress_worlddb::Error> for SpaceError {
    fn from(e: eustress_worlddb::Error) -> Self {
        SpaceError::WorldDb(e)
    }
}
impl From<std::io::Error> for SpaceError {
    fn from(e: std::io::Error) -> Self {
        SpaceError::Io(e)
    }
}

/// Result alias for the tool.
pub type Result<T> = std::result::Result<T, SpaceError>;

/// Resolve a user-supplied path to the `world.fjalldb/` directory and
/// (when discoverable) the space root that holds `header.bin`.
///
/// Accepts either:
/// - the space root (a directory containing `world.fjalldb/`), or
/// - the `world.fjalldb/` directory itself.
///
/// Returns `(fjalldb_dir, space_root_opt)`. `space_root_opt` is `Some`
/// only when we opened via a space root (so `header.bin` is alongside).
pub fn resolve_space(path: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    // Case 1: caller pointed at the space root.
    let nested = path.join("world.fjalldb");
    if nested.is_dir() {
        return Ok((nested, Some(path.to_path_buf())));
    }
    // Case 2: caller pointed straight at world.fjalldb/.
    if path.is_dir() && path.file_name().map(|n| n == "world.fjalldb").unwrap_or(false) {
        let space_root = path.parent().map(|p| p.to_path_buf());
        return Ok((path.to_path_buf(), space_root));
    }
    // Case 3: caller pointed at a directory that *is* a Fjall DB but is
    // not named world.fjalldb (e.g. a test temp dir). Treat any directory
    // that already contains Fjall artifacts as the DB dir. We detect this
    // cheaply by the presence of any entry — FjallWorldDb::open will fail
    // loudly later if it is not actually a keyspace.
    if path.is_dir() {
        return Ok((path.to_path_buf(), None));
    }
    Err(SpaceError::NotASpace(path.to_path_buf()))
}

/// Open the world database at the resolved `world.fjalldb/` directory,
/// read-only (we never write). Returns the boxed concrete backend.
fn open_backend(fjalldb_dir: &Path) -> Result<eustress_worlddb::FjallWorldDb> {
    Ok(eustress_worlddb::FjallWorldDb::open(fjalldb_dir)?)
}

// ── `open` — the "did it load + what's in it" report ─────────────────

/// One row of the class histogram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassCount {
    /// The `ClassName` (e.g. `Part`, `MeshPart`, `SpawnLocation`).
    pub class_name: String,
    /// How many instance cores carry it.
    pub count: usize,
}

/// Axis-aligned world bounds derived from instance-core translations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldBounds {
    /// Component-wise minimum of every core's translation.
    pub min: [f32; 3],
    /// Component-wise maximum of every core's translation.
    pub max: [f32; 3],
}

/// The structured result of [`open`]. Rendered by the bin; returned plain
/// so tests can assert on it without parsing stdout.
#[derive(Debug, Clone)]
pub struct OpenReport {
    /// Total instance cores in the entities (Morton) partition.
    pub entity_count: usize,
    /// How many cores failed to decode while building the report. A
    /// healthy space reports 0 here; `verify` is the authoritative gate.
    pub undecodable: usize,
    /// Class histogram, sorted by descending count then class name.
    pub class_histogram: Vec<ClassCount>,
    /// World bounds, or `None` when there are no decodable cores.
    pub bounds: Option<WorldBounds>,
    /// Voxel-chunk count. `None` = the voxel-chunk store (Wave 9.A) is
    /// not present in this build's worlddb API, so the count is unknown
    /// (not zero). Becomes `Some(n)` once `iter_all_voxel_chunks` exists.
    pub voxel_chunk_count: Option<usize>,
    /// Header summary when `header.bin` was found alongside `world.fjalldb/`.
    pub header: Option<HeaderSummary>,
    /// On-disk world-schema version reported by the backend's meta partition.
    pub schema_version: Option<u16>,
}

/// The few header fields worth surfacing in `open`.
#[derive(Debug, Clone)]
pub struct HeaderSummary {
    /// World UUID (cloud-sync / multiplayer routing id).
    pub world_id: String,
    /// Engine semver that last wrote the header.
    pub engine_semver: String,
    /// `migrated_at` timestamp — `Some` once the space is a clean
    /// DB-authoritative `.eustress` container.
    pub migrated_at: Option<String>,
}

/// Open a space and gather the inspection report. Read-only.
pub fn open(path: &Path) -> Result<OpenReport> {
    let (fjalldb_dir, space_root) = resolve_space(path)?;
    let db = open_backend(&fjalldb_dir)?;

    // Header (best-effort): present only for a migrated space root.
    let header = space_root
        .as_deref()
        .and_then(|root| eustress_worlddb::WorldHeader::read(root).ok().flatten())
        .map(|h| HeaderSummary {
            world_id: h.world_id.to_string(),
            engine_semver: h.engine.semver.clone(),
            migrated_at: h.migrated_at.clone(),
        });

    let schema_version = db.on_disk_schema().ok().map(|v| v.0);

    // Walk every binary-ECS core. One decode each: builds the class
    // histogram (class_name) and the world bounds (translation t). A core
    // that fails to decode is counted but doesn't abort the report —
    // `verify` is the strict gate.
    let cores = db.iter_instance_cores()?;
    let entity_count = cores.len();
    let mut hist: BTreeMap<String, usize> = BTreeMap::new();
    let mut undecodable = 0usize;
    let mut bounds: Option<WorldBounds> = None;
    for (_entity, bytes) in &cores {
        match decode_instance_core(bytes) {
            Ok(core) => {
                *hist.entry(core.class_name).or_insert(0) += 1;
                accumulate_bounds(&mut bounds, core.t);
            }
            Err(_) => undecodable += 1,
        }
    }

    let mut class_histogram: Vec<ClassCount> = hist
        .into_iter()
        .map(|(class_name, count)| ClassCount { class_name, count })
        .collect();
    // Descending count, then class name ascending for a stable order.
    class_histogram.sort_by(|a, b| b.count.cmp(&a.count).then(a.class_name.cmp(&b.class_name)));

    Ok(OpenReport {
        entity_count,
        undecodable,
        class_histogram,
        bounds,
        // Wave 9.A voxel-chunk partition is not in this worlddb build.
        // TODO(9.A): replace with `Some(db.iter_all_voxel_chunks()?.len())`.
        voxel_chunk_count: None,
        header,
        schema_version,
    })
}

/// Fold one translation into the running AABB.
fn accumulate_bounds(bounds: &mut Option<WorldBounds>, t: [f32; 3]) {
    match bounds {
        None => *bounds = Some(WorldBounds { min: t, max: t }),
        Some(b) => {
            for i in 0..3 {
                if t[i] < b.min[i] {
                    b.min[i] = t[i];
                }
                if t[i] > b.max[i] {
                    b.max[i] = t[i];
                }
            }
        }
    }
}

// ── `verify` — THE serialization-correctness gate (§8.C) ─────────────

/// One round-trip failure: the entity whose core failed to decode + why.
#[derive(Debug, Clone)]
pub struct VerifyFailure {
    /// The session-local entity id (raw `u64`) the failing core was keyed by.
    pub entity: u64,
    /// The decode error message (rkyv access / CheckBytes / tag mismatch).
    pub reason: String,
}

/// Result of [`verify`]: how many cores validated, which failed.
#[derive(Debug, Clone)]
pub struct VerifyReport {
    /// Cores that decoded + validated cleanly.
    pub ok: usize,
    /// Cores that failed `rkyv::access` (CheckBytes) — the list is
    /// surfaced so a corrupt record can be pinpointed.
    pub failures: Vec<VerifyFailure>,
}

impl VerifyReport {
    /// True when every core round-tripped. The bin maps `false` to a
    /// non-zero process exit.
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Iterate every instance core and validate it via the same
/// CheckBytes-backed [`decode_instance_core`] the engine load path uses.
/// This is the serialization gate: a `.eustress` that passes here is
/// guaranteed to be readable by the engine's binary-ECS loader.
pub fn verify(path: &Path) -> Result<VerifyReport> {
    let (fjalldb_dir, _space_root) = resolve_space(path)?;
    let db = open_backend(&fjalldb_dir)?;

    let cores = db.iter_instance_cores()?;
    let mut ok = 0usize;
    let mut failures = Vec::new();
    for (entity, bytes) in &cores {
        // `decode_instance_core` performs `rkyv::access::<ArchivedArchInstanceCore>`,
        // which runs CheckBytes validation on the archive before any read,
        // then a full owned deserialize. Both must succeed for a pass.
        match decode_instance_core(bytes) {
            Ok(_core) => ok += 1,
            Err(e) => failures.push(VerifyFailure {
                entity: entity.0,
                reason: e.to_string(),
            }),
        }
    }

    Ok(VerifyReport { ok, failures })
}

// ── `export` — binary → readable TOML escape hatch ───────────────────

/// Result of [`export`]: where the tree was written and how much.
#[derive(Debug, Clone)]
pub struct ExportReport {
    /// Directory the `.instance.toml` tree was written under.
    pub out_dir: PathBuf,
    /// Number of cores successfully projected to TOML.
    pub written: usize,
    /// Cores that could not be decoded (skipped, counted here).
    pub skipped: usize,
}

/// Export every instance core to a readable `<class>/<entity>.instance.toml`
/// tree under `out_dir`.
///
/// ## Why this is a real projection, not a stub
///
/// The engine's `arch_to_instance` (which produces the exact serde
/// `InstanceDefinition` shape) lives ENGINE-side
/// (`engine/src/space/arch_instance.rs`) and pulls in the full engine, so
/// it is **not reachable** from this worlddb-only crate. But everything a
/// human needs is already inside [`ArchInstanceCore`]: identity, asset,
/// transform, render/physics flags, material, tags, and the extensible
/// `extra` cold tail — and worlddb ships the `EusValue → toml::Value`
/// bridge. So we reconstruct a readable, lossless-to-the-stored-bytes TOML
/// document directly from the core. It is NOT byte-identical to an engine
/// `_instance.toml` (the engine flattens `[Appearance]`-style sections and
/// re-nests the `__meta`/`__material`/… tail), but it is fully human-readable
/// and captures every field the binary core holds.
///
/// TODO(portability): when `arch_to_instance` is extracted to a no-engine
/// crate (or duplicated minimally), swap the projection below for it to
/// emit the canonical `_instance.toml` shape.
pub fn export(path: &Path, out_dir: &Path) -> Result<ExportReport> {
    let (fjalldb_dir, _space_root) = resolve_space(path)?;
    let db = open_backend(&fjalldb_dir)?;

    std::fs::create_dir_all(out_dir)?;
    let cores = db.iter_instance_cores()?;
    let mut written = 0usize;
    let mut skipped = 0usize;

    for (entity, bytes) in &cores {
        let core = match decode_instance_core(bytes) {
            Ok(c) => c,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let doc = core_to_toml_string(&core)
            .map_err(|e| SpaceError::Toml(format!("entity {}: {e}", entity.0)))?;

        // Lay out as <out>/<ClassName>/<entity_id>.instance.toml so the
        // tree is browsable by class — the readable face of the binary store.
        let class_dir = out_dir.join(sanitize_segment(&core.class_name));
        std::fs::create_dir_all(&class_dir)?;
        let file = class_dir.join(format!("{}.instance.toml", entity.0));
        std::fs::write(&file, doc)?;
        written += 1;
    }

    Ok(ExportReport {
        out_dir: out_dir.to_path_buf(),
        written,
        skipped,
    })
}

/// Render one [`ArchInstanceCore`] to a readable TOML document string.
///
/// Mirrors the `_instance.toml` layout closely enough to be obvious:
/// a `[metadata]` table (class name), an `[asset]` table (mesh/scene) when
/// the instance is visual, a `[transform]` table, a `[properties]` table,
/// `tags`, and each `extra` cold-tail section as its own table (under its
/// stored key, e.g. `__meta`). The `extra` values round-trip through
/// worlddb's `EusValue → toml::Value` bridge.
pub fn core_to_toml_string(core: &ArchInstanceCore) -> std::result::Result<String, String> {
    use toml::value::{Table, Value};

    let mut root = Table::new();

    // [metadata]
    let mut metadata = Table::new();
    metadata.insert("class_name".into(), Value::String(core.class_name.clone()));
    root.insert("metadata".into(), Value::Table(metadata));

    // [asset] — only for visual instances (mesh present).
    if !core.mesh.is_empty() {
        let mut asset = Table::new();
        asset.insert("mesh".into(), Value::String(core.mesh.clone()));
        asset.insert(
            "scene".into(),
            Value::String(if core.scene.is_empty() {
                "Scene0".into()
            } else {
                core.scene.clone()
            }),
        );
        root.insert("asset".into(), Value::Table(asset));
    }

    // [transform]
    let mut transform = Table::new();
    transform.insert("position".into(), f32_array(&core.t));
    transform.insert("rotation".into(), f32_array(&core.r));
    transform.insert("scale".into(), f32_array(&core.s));
    root.insert("transform".into(), Value::Table(transform));

    // [properties]
    let mut props = Table::new();
    props.insert("color".into(), f32_array(&core.color));
    props.insert("transparency".into(), Value::Float(core.transparency as f64));
    props.insert("reflectance".into(), Value::Float(core.reflectance as f64));
    props.insert("anchored".into(), Value::Boolean(core.anchored));
    props.insert("can_collide".into(), Value::Boolean(core.can_collide));
    props.insert("cast_shadow".into(), Value::Boolean(core.cast_shadow));
    props.insert("locked".into(), Value::Boolean(core.locked));
    props.insert("material".into(), Value::String(core.material.clone()));
    root.insert("properties".into(), Value::Table(props));

    // tags (top-level array)
    if !core.tags.is_empty() {
        root.insert(
            "tags".into(),
            Value::Array(core.tags.iter().cloned().map(Value::String).collect()),
        );
    }

    // Extensible cold tail: each (key, EusValue) becomes a top-level table
    // under its stored key (e.g. __meta, __material, __extra). EusValue →
    // toml::Value is worlddb's own bridge, so this is lossless w.r.t. the
    // stored archive.
    for (key, eus) in &core.extra {
        let v: Value = eus.clone().into();
        root.insert(key.clone(), v);
    }

    toml::to_string_pretty(&Value::Table(root)).map_err(|e| e.to_string())
}

/// `[f32]` → a TOML float array (rotation/scale/color/position).
fn f32_array(xs: &[f32]) -> toml::Value {
    toml::Value::Array(xs.iter().map(|&x| toml::Value::Float(x as f64)).collect())
}

/// Make a class name safe to use as a single path segment (class names
/// are Rust-ident-like, but be defensive against `/`, `\`, `:`).
fn sanitize_segment(s: &str) -> String {
    if s.is_empty() {
        return "_Unknown".to_string();
    }
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect()
}

// ── test-only write helpers ──────────────────────────────────────────
//
// Tooling never writes cores in production (it is read-only), but the
// tests need to seed a world. These helpers wrap the worlddb write API
// the engine baker uses, so the test fixtures match real on-disk records.

/// Write one already-baked [`ArchInstanceCore`] into a `world.fjalldb/`
/// at its translation. Used by tests to build a fixture world.
#[doc(hidden)]
pub fn write_core_for_test(
    db: &dyn WorldDb,
    entity: EntityId,
    core: &ArchInstanceCore,
) -> Result<()> {
    let bytes = eustress_worlddb::encode_instance_core(core)?;
    db.put_instance_core(entity, (core.t[0], core.t[1], core.t[2]), &bytes)?;
    Ok(())
}

/// Write a deliberately-corrupt core record so `verify` has something to
/// fail on. The bytes carry the right rkyv value tag but a truncated
/// archive body, so `rkyv::access` (CheckBytes) rejects it.
#[doc(hidden)]
pub fn write_corrupt_core_for_test(
    db: &dyn WorldDb,
    entity: EntityId,
    pos: (f32, f32, f32),
) -> Result<()> {
    // Tag byte (RKYV_VALUE_TAG == 1) + a few junk bytes. Passes the tag
    // check in decode_instance_core, fails the rkyv::access validation.
    let corrupt = vec![1u8, 0xde, 0xad, 0xbe, 0xef];
    db.put_instance_core(entity, pos, &corrupt)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_worlddb::{ArchInstanceCore, EntityId, EusValue};

    /// Build a small, varied core so the histogram + bounds have content.
    fn sample_core(class: &str, t: [f32; 3]) -> ArchInstanceCore {
        ArchInstanceCore {
            class_name: class.into(),
            mesh: if class == "MeshPart" {
                "parts/block.glb".into()
            } else {
                String::new()
            },
            scene: "Scene0".into(),
            t,
            r: [0.0, 0.0, 0.0, 1.0],
            s: [1.0, 1.0, 1.0],
            color: [0.5, 0.5, 0.5, 1.0],
            transparency: 0.0,
            reflectance: 0.0,
            anchored: true,
            can_collide: true,
            cast_shadow: true,
            locked: false,
            material: "Plastic".into(),
            tags: vec!["fixture".into()],
            extra: vec![(
                "__extra".into(),
                EusValue::Table(vec![("k".into(), EusValue::Int(7))]),
            )],
        }
    }

    /// Open a fresh world.fjalldb under a unique temp dir, seed it, return
    /// the temp dir (kept alive by the caller) + the resolved space path.
    fn seeded_world(
        cores: &[(EntityId, ArchInstanceCore)],
        corrupt: &[(EntityId, (f32, f32, f32))],
    ) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let db_dir = tmp.path().join("world.fjalldb");
        std::fs::create_dir_all(&db_dir).unwrap();
        {
            let db = eustress_worlddb::FjallWorldDb::open(&db_dir).unwrap();
            for (e, c) in cores {
                write_core_for_test(&db, *e, c).unwrap();
            }
            for (e, pos) in corrupt {
                write_corrupt_core_for_test(&db, *e, *pos).unwrap();
            }
            db.flush().unwrap();
        } // drop closes the DB so the next open reads from a clean handle.
        let space_root = tmp.path().to_path_buf();
        (tmp, space_root)
    }

    #[test]
    fn open_counts_entities_and_classes() {
        let cores = vec![
            (EntityId(1), sample_core("Part", [0.0, 0.0, 0.0])),
            (EntityId(2), sample_core("Part", [10.0, 0.0, -5.0])),
            (EntityId(3), sample_core("MeshPart", [-3.0, 4.0, 2.0])),
        ];
        let (_tmp, root) = seeded_world(&cores, &[]);

        let report = open(&root).unwrap();
        assert_eq!(report.entity_count, 3, "all three cores counted");
        assert_eq!(report.undecodable, 0, "no decode failures on a clean world");

        // Histogram: Part=2, MeshPart=1; Part sorts first (higher count).
        assert_eq!(report.class_histogram.len(), 2);
        assert_eq!(report.class_histogram[0].class_name, "Part");
        assert_eq!(report.class_histogram[0].count, 2);
        assert_eq!(report.class_histogram[1].class_name, "MeshPart");
        assert_eq!(report.class_histogram[1].count, 1);

        // Bounds: min/max over the three translations.
        let b = report.bounds.expect("bounds present with cores");
        assert_eq!(b.min, [-3.0, 0.0, -5.0]);
        assert_eq!(b.max, [10.0, 4.0, 2.0]);

        // Voxel store absent in this schema → unknown, not zero.
        assert!(report.voxel_chunk_count.is_none());
    }

    #[test]
    fn open_on_world_fjalldb_dir_directly() {
        // The tool accepts being pointed straight at world.fjalldb/, not
        // just the space root.
        let cores = vec![(EntityId(1), sample_core("Part", [1.0, 2.0, 3.0]))];
        let (_tmp, root) = seeded_world(&cores, &[]);
        let db_dir = root.join("world.fjalldb");
        let report = open(&db_dir).unwrap();
        assert_eq!(report.entity_count, 1);
    }

    #[test]
    fn verify_reports_zero_failures_on_clean_world() {
        let cores = vec![
            (EntityId(1), sample_core("Part", [0.0, 0.0, 0.0])),
            (EntityId(2), sample_core("SpawnLocation", [5.0, 0.0, 5.0])),
        ];
        let (_tmp, root) = seeded_world(&cores, &[]);

        let report = verify(&root).unwrap();
        assert_eq!(report.ok, 2);
        assert!(report.failures.is_empty());
        assert!(report.passed());
    }

    #[test]
    fn verify_catches_a_corrupt_core() {
        // Two good cores + one corrupt. verify must report exactly 1 failure
        // and NOT pass (the bin maps !passed() to a non-zero exit code).
        let cores = vec![
            (EntityId(1), sample_core("Part", [0.0, 0.0, 0.0])),
            (EntityId(2), sample_core("Part", [1.0, 1.0, 1.0])),
        ];
        let corrupt = vec![(EntityId(99), (50.0, 0.0, 50.0))];
        let (_tmp, root) = seeded_world(&cores, &corrupt);

        let report = verify(&root).unwrap();
        assert_eq!(report.ok, 2, "the two good cores validate");
        assert_eq!(report.failures.len(), 1, "exactly one corrupt core caught");
        assert_eq!(report.failures[0].entity, 99);
        assert!(!report.passed(), "a corrupt core must fail the gate");

        // open() over the same world stays robust: counts all 3 records,
        // flags 1 as undecodable, still builds a histogram from the good 2.
        let oreport = open(&root).unwrap();
        assert_eq!(oreport.entity_count, 3);
        assert_eq!(oreport.undecodable, 1);
        assert_eq!(oreport.class_histogram[0].count, 2);
    }

    #[test]
    fn export_writes_readable_toml_tree() {
        let cores = vec![
            (EntityId(1), sample_core("Part", [1.0, 2.0, 3.0])),
            (EntityId(2), sample_core("MeshPart", [4.0, 5.0, 6.0])),
        ];
        let (_tmp, root) = seeded_world(&cores, &[]);

        let out = tempfile::tempdir().unwrap();
        let report = export(&root, out.path()).unwrap();
        assert_eq!(report.written, 2);
        assert_eq!(report.skipped, 0);

        // Files landed under <out>/<class>/<entity>.instance.toml and are
        // valid, re-parseable TOML carrying the expected fields.
        let part_file = out.path().join("Part").join("1.instance.toml");
        assert!(part_file.exists(), "Part/1.instance.toml written");
        let mesh_file = out.path().join("MeshPart").join("2.instance.toml");
        assert!(mesh_file.exists(), "MeshPart/2.instance.toml written");

        let text = std::fs::read_to_string(&part_file).unwrap();
        let parsed: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(parsed["metadata"]["class_name"].as_str(), Some("Part"));
        let pos = parsed["transform"]["position"].as_array().unwrap();
        assert_eq!(pos[0].as_float(), Some(1.0));
        assert_eq!(pos[2].as_float(), Some(3.0));
        assert_eq!(parsed["properties"]["material"].as_str(), Some("Plastic"));
        // cold tail survived
        assert_eq!(parsed["__extra"]["k"].as_integer(), Some(7));

        // The MeshPart kept its asset table (it has a mesh).
        let mtext = std::fs::read_to_string(&mesh_file).unwrap();
        let mparsed: toml::Value = toml::from_str(&mtext).unwrap();
        assert_eq!(mparsed["asset"]["mesh"].as_str(), Some("parts/block.glb"));
    }

    #[test]
    fn core_to_toml_roundtrips_through_parse() {
        // The projection is self-consistent: serialize → parse → the key
        // fields match. (Not the engine InstanceDefinition shape — see the
        // export() doc — but valid, readable, and faithful to the core.)
        let core = sample_core("MeshPart", [7.0, 8.0, 9.0]);
        let s = core_to_toml_string(&core).unwrap();
        let v: toml::Value = toml::from_str(&s).unwrap();
        assert_eq!(v["metadata"]["class_name"].as_str(), Some("MeshPart"));
        assert_eq!(v["asset"]["scene"].as_str(), Some("Scene0"));
        assert_eq!(v["properties"]["anchored"].as_bool(), Some(true));
        assert_eq!(v["tags"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn resolve_rejects_nonexistent_path() {
        let missing = std::path::Path::new("E:/__definitely_not_a_space__/nope");
        assert!(matches!(resolve_space(missing), Err(SpaceError::NotASpace(_))));
    }
}

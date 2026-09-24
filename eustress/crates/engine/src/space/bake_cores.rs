//! Phase 0 — bake the `tree` partition into Morton-keyed `entities` cores.
//!
//! ## The bug this fixes
//!
//! The streaming decision in [`super::world_db_binary`] reads
//! `active_db::count_instance_cores_capped`, which counts rows in the
//! **`entities`** partition. A Space imported from a TOML hierarchy has its
//! entities in the **`tree`** partition instead, so that count is ~0, the
//! Space is classified SMALL, residency never enables, and `file_loader`
//! eagerly spawns every entity.
//!
//! Measured consequence on a 1.34M-entity Space: ~4.4 ms per entity through
//! the eager path, a spill queue that grows faster than it drains, and a
//! projected ~98 minute load. The streaming system built for exactly this
//! case sits switched off.
//!
//! This pass converts eligible tree entities into binary cores so the count
//! reflects reality and residency takes over.
//!
//! ## Two constraints that make this non-trivial
//!
//! **Cores are FLAT.** `spawn_binary_core` inserts every core as
//! `ChildOf(workspace)` — there is no hierarchy on the binary path. The tree,
//! by contrast, is nested, and an `InstanceDefinition` transform is
//! parent-relative (it becomes a Bevy `Transform`). Baking a nested entity's
//! LOCAL position would key it into the wrong Morton cell and then render it
//! in the wrong place. So this pass accumulates ancestor transforms and bakes
//! the WORLD transform.
//!
//! **Only leaves are eligible.** A core has no tree entry and no children, so
//! converting a parent would strand its descendants — the same failure
//! `active_db::put_instance` guards against with `folder_has_child_entities`.
//! Parents, mesh-backed instances and file-natured classes stay in the tree
//! and continue to load through the existing path.
//!
//! Ineligible entities are not a problem: they are a small minority in an
//! imported Space, and the streaming gate only needs the count to clear
//! `big_space_threshold`.

#![cfg(feature = "world-db")]

use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::*;
use eustress_worlddb::{decode_instance_core, encode_instance_core, EntityId, WorldDb};

use super::arch_instance::instance_to_arch;
use super::instance_loader::{InstanceDefinition, TransformData};

/// Marker written under `.eustress/` recording WHICH VERSION of the bake last
/// ran. A file rather than a `WorldHeader` field so this needs no schema change
/// in `eustress-worlddb`; deleting it forces a re-bake, which is the intended
/// escape hatch after a failed or partial run.
const MARKER: &str = "cores_baked";

/// Bumped whenever the eligibility predicate changes.
///
/// A plain "already baked" boolean was wrong, and wrong in a way that hides:
/// v1 ran with an eligibility test that did not match `file_loader`'s skip
/// gate, so it converted `Seat` / `SpawnLocation` / `VehicleSeat` and any
/// instance whose mesh is referenced outside `[asset]` — entities the loader
/// still spawns, producing each of them twice. With a boolean marker the
/// corrected predicate would never re-run on an already-baked Space, so the
/// duplicates would be permanent and invisible.
///
/// Versioning turns the pass into a reconciliation (see [`bake_tree_to_cores`]):
/// a version bump re-derives eligibility for every tree entity and both writes
/// the newly-eligible AND removes the no-longer-eligible.
///
/// * v1 — initial bake, predicate local to this module (WRONG, see above).
/// * v2 — shares `representation::streams_from_db` with `file_loader`.
/// * v3 — stored ids use the engine's one derivation ([`stored_id_from_uuid`]),
///   and every baked core's synthetic path is registered in `path_to_uuid`.
///   v1 and v2 read the uuid bytes in the opposite order
///   ([`legacy_stored_id_from_uuid`]), so create, import, promote, the bridge
///   and Delete could never address a baked core by its uuid. Upgrading
///   re-keys those cores in place.
const BAKE_VERSION: u32 = 3;

/// What one bake run did. Logged at INFO so a slow first open is explicable.
#[derive(Debug, Default, Clone)]
pub struct BakeSummary {
    /// Tree rows examined.
    pub examined: usize,
    /// Cores written to the `entities` partition.
    pub baked: usize,
    /// Skipped because the entity has children (must stay folder-form).
    pub skipped_parent: usize,
    /// Skipped because the class or mesh requires filesystem form.
    pub skipped_filesystem: usize,
    /// Rows that failed to parse as an `InstanceDefinition`.
    pub unparsable: usize,
    /// Cores REMOVED because a predicate change made them ineligible. Non-zero
    /// only on a version upgrade, and the count that proves a re-bake actually
    /// undid the previous version's mistakes.
    pub removed: usize,
    /// Writes that returned an error.
    pub write_failures: usize,
}

/// Which bake version last ran for this Space, if any.
///
/// An unparsable marker reads as v1: the original stamped a bare `1` with no
/// version semantics, and treating anything unrecognised as "oldest" makes the
/// upgrade path safe by default.
fn baked_version(space_root: &Path) -> Option<u32> {
    let raw = std::fs::read_to_string(space_root.join(".eustress").join(MARKER)).ok()?;
    Some(raw.trim().parse::<u32>().unwrap_or(1))
}

fn stamp(space_root: &Path) {
    let dir = space_root.join(".eustress");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(MARKER), BAKE_VERSION.to_string().as_bytes());
}

/// One entity as read out of the `tree` partition, before world resolution.
struct Node {
    def: InstanceDefinition,
    /// Tree key of the parent entity, or `None` for a Workspace-level root.
    parent: Option<String>,
    /// Set when any other node names this one as its parent.
    has_children: bool,
    /// From the SAME text scan `file_loader` uses, computed here while the raw
    /// bytes are in hand — deriving it from parsed fields instead would answer
    /// differently for a mesh referenced outside `[asset]`.
    has_custom_mesh: bool,
}

/// The tree key of the entity that owns `key`, if any.
///
/// Keys look like `Workspace/A/B/_instance.toml`. The owning entity is the
/// nearest ancestor directory that itself has an `_instance.toml`, i.e.
/// `Workspace/A/_instance.toml`. Returns `None` at the service level
/// (`Workspace/_instance.toml` has no entity parent), which is correct: those
/// bake against the identity transform.
fn parent_key(key: &str) -> Option<String> {
    let folder = key.strip_suffix("/_instance.toml")?;
    let up = folder.rsplit_once('/')?.0;
    // A single remaining segment is the service folder, not an entity.
    if !up.contains('/') {
        return None;
    }
    Some(format!("{up}/_instance.toml"))
}

/// Compose a parent-relative definition transform onto a resolved parent.
///
/// The field is documented as a "world transform", but `spawn_instance` feeds
/// it to `Transform::from(instance.transform)` and parents the entity with
/// `ChildOf`, so Bevy interprets it as PARENT-RELATIVE. Runtime semantics win
/// over the comment: composing here is what makes a baked core land where the
/// tree-loaded entity currently renders, which is the invariant that matters.
fn compose(parent: &Transform, local: &TransformData) -> Transform {
    parent.mul_transform(Transform::from(local.clone()))
}

/// Derive a core's stable persistence id from its entity UUID: the first 8
/// bytes, read big-endian.
///
/// This is the engine's one derivation. `world_db_binary::create_binary_instance`,
/// the importer, promote, the bridge and Delete all compute exactly this, so a
/// core baked here is addressable by every one of them from the uuid alone. It
/// must stay bit-for-bit identical to those sites.
pub(crate) fn stored_id_from_uuid(uuid: &[u8; 16]) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&uuid[..8]);
    u64::from_be_bytes(b)
}

/// The id bakes v1 and v2 wrote: the same bytes read little-endian, with 0
/// nudged to 1. Used to find and re-key those cores, and so boot-load and
/// Delete still recognise one that an interrupted upgrade left behind.
pub(crate) fn legacy_stored_id_from_uuid(uuid: &[u8; 16]) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&uuid[..8]);
    match u64::from_le_bytes(b) {
        0 => 1,
        id => id,
    }
}

/// The Space-relative path a core spawns under
/// (`world_db_binary::synthetic_path`), which Delete parses back into its
/// stored id. A baked core registers it in `path_to_uuid`, as create and import
/// do, because a core carries no uuid of its own: this index is how Delete gets
/// from a core-spawned part back to its uuid and its tree rows.
pub(crate) fn synthetic_rel(class_name: &str, stored_id: u64) -> String {
    format!("Workspace/__bin_{class_name}_{stored_id:016x}/_instance.toml")
}

/// Bake eligible `tree` entities into Morton-keyed `entities` cores.
///
/// Idempotent by marker file. Additive: it never deletes a tree row, so a
/// Space that fails midway is still fully loadable through the existing path
/// and simply re-bakes on the next open.
pub fn bake_tree_to_cores(space_root: &Path, db: &dyn WorldDb) -> BakeSummary {
    let mut sum = BakeSummary::default();

    // ── Pass 1: read every entity row and record parent links ────────────
    let mut nodes: HashMap<String, Node> = HashMap::new();
    let rows = match db.iter_tree() {
        Ok(it) => it,
        Err(e) => {
            warn!(target: "eustress_engine::bake_cores", error = %e, "tree scan failed; skipping bake");
            return sum;
        }
    };
    for row in rows {
        let Ok((key, bytes)) = row else { continue };
        if !key.ends_with("/_instance.toml") {
            continue; // services, scripts, `#bin` twins, assets
        }
        sum.examined += 1;
        let Ok(text) = std::str::from_utf8(&bytes) else {
            sum.unparsable += 1;
            continue;
        };
        let Ok(def) = toml::from_str::<InstanceDefinition>(text) else {
            sum.unparsable += 1;
            continue;
        };
        let has_custom_mesh = super::representation::toml_mentions_custom_mesh(text);
        let parent = parent_key(&key);
        nodes.insert(
            key,
            Node {
                def,
                parent,
                has_children: false,
                has_custom_mesh,
            },
        );
    }

    // Mark parents. Done as a second sweep because a child can appear before
    // its parent in key order.
    let parents: Vec<String> = nodes
        .values()
        .filter_map(|n| n.parent.clone())
        .collect();
    for p in parents {
        if let Some(n) = nodes.get_mut(&p) {
            n.has_children = true;
        }
    }

    // ── Pass 2: resolve world transforms, root-down ──────────────────────
    // Memoised so a deep chain costs one composition per node, not one per
    // descendant. Iterative rather than recursive: an imported hierarchy can
    // be deep enough to blow a recursive stack.
    let mut world: HashMap<String, Transform> = HashMap::new();
    let keys: Vec<String> = nodes.keys().cloned().collect();
    for key in &keys {
        if world.contains_key(key) {
            continue;
        }
        // Walk up to the nearest resolved ancestor (or a root), then compose
        // back down the chain.
        let mut chain: Vec<String> = Vec::new();
        let mut cursor = Some(key.clone());
        while let Some(k) = cursor {
            if world.contains_key(&k) {
                break;
            }
            let Some(node) = nodes.get(&k) else { break };
            chain.push(k.clone());
            cursor = node.parent.clone();
            // A cycle would loop forever; the tree cannot contain one, but a
            // malformed key set could. Bail rather than hang.
            if chain.len() > 4096 {
                warn!(
                    target: "eustress_engine::bake_cores",
                    key = %k,
                    "ancestor chain over 4096 deep; treating as root"
                );
                cursor = None;
            }
        }
        for k in chain.iter().rev() {
            let Some(node) = nodes.get(k) else { continue };
            let base = node
                .parent
                .as_ref()
                .and_then(|p| world.get(p))
                .copied()
                .unwrap_or(Transform::IDENTITY);
            let t = compose(&base, &node.def.transform);
            world.insert(k.clone(), t);
        }
    }

    // ── Pass 3: write cores for eligible leaves ──────────────────────────
    for key in &keys {
        let Some(node) = nodes.get(key) else { continue };
        // ONE predicate, shared with `file_loader`'s streaming-primary skip.
        // Baking something the loader still spawns creates the entity twice;
        // this must be the exact complement of that gate, which is why it is a
        // shared function rather than a matching pair of local checks.
        let eligible = super::representation::streams_from_db(
            &node.def.metadata.class_name,
            node.has_children,
            node.has_custom_mesh,
        );
        if !eligible {
            // RECONCILE, do not merely skip. A previous bake version may have
            // written a core for this entity under a looser predicate; leaving
            // it would mean the loader spawns it AND residency streams it.
            // Removing here is what makes a predicate change self-correcting
            // rather than something that needs a manual purge.
            if let (Some(t), Ok(Some(uuid))) =
                (world.get(key), db.path_to_uuid(key))
            {
                let id = EntityId(stored_id_from_uuid(&uuid));
                if db
                    .delete_instance_core(id, (t.translation.x, t.translation.y, t.translation.z))
                    .is_ok()
                {
                    sum.removed += 1;
                }
                let _ = db.delete_path_to_uuid(&synthetic_rel(
                    &node.def.metadata.class_name,
                    id.0,
                ));
            }
            if node.has_children {
                sum.skipped_parent += 1;
            } else {
                sum.skipped_filesystem += 1;
            }
            continue;
        }
        let Some(t) = world.get(key) else { continue };

        // The UUID pass already recorded identity for every tree entity; a
        // missing mapping means this row predates it, so leave it alone
        // rather than minting a second identity for the same entity.
        let uuid = match db.path_to_uuid(key) {
            Ok(Some(u)) => u,
            _ => {
                sum.skipped_filesystem += 1;
                continue;
            }
        };

        // Bake the WORLD transform: cores spawn flat under Workspace, so a
        // parent-relative transform would place the entity wrongly and key it
        // into the wrong Morton cell.
        let mut core = instance_to_arch(&node.def);
        core.t = t.translation.to_array();
        core.r = t.rotation.to_array();
        core.s = t.scale.to_array();

        let Ok(bytes) = encode_instance_core(&core) else {
            sum.write_failures += 1;
            continue;
        };
        let id = EntityId(stored_id_from_uuid(&uuid));
        let pos = (t.translation.x, t.translation.y, t.translation.z);
        if db.put_instance_core(id, pos, &bytes).is_err() {
            sum.write_failures += 1;
            continue;
        }
        // Keep the UUID-primary copy in step, so a `find_entity --uuid` or a
        // bridge read of a non-resident entity sees the same bytes the Morton
        // core holds.
        let _ = db.put_entity_core_by_uuid(&uuid, &bytes);
        // The synthetic path → uuid index, so a part spawned from this core
        // (which carries no uuid) can be traced back to its tree rows on
        // Delete. `uuid_to_path` is left pointing at the tree row.
        let _ = db.put_path_to_uuid(&synthetic_rel(&core.class_name, id.0), &uuid);
        sum.baked += 1;
    }

    let _ = db.flush();
    stamp(space_root);
    sum
}

/// Run the bake once per Space, before the streaming decision reads the core
/// count. Cheap no-op on an already-baked Space (one `Path::exists`).
/// True when this Space already carries the current bake, i.e. `bake_once`
/// would return at once. The open worker uses it to decide whether the DB
/// can be installed before the disk reconcile (every open after the first)
/// or only after it (the one open that actually bakes).
pub fn is_baked(space_root: &Path) -> bool {
    baked_version(space_root) == Some(BAKE_VERSION)
}

/// Move cores that a v1 or v2 bake wrote under the legacy id to the canonical
/// id, and register each one's synthetic path. `false` when anything failed:
/// the caller then leaves the version unstamped so the next open retries,
/// and until it does, boot-load and Delete still recognise the legacy id.
fn rekey_legacy_cores(db: &dyn WorldDb) -> bool {
    let t0 = std::time::Instant::now();
    let keys = match db.iter_tree_keys() {
        Ok(k) => k,
        Err(e) => {
            warn!(target: "eustress_engine::bake_cores", error = %e, "tree key scan failed; bake id upgrade deferred");
            return false;
        }
    };
    // Every tree entity's legacy id → canonical id. Only entities that were
    // baked have a core under the legacy id; the rest match nothing and cost
    // one map entry.
    let mut rename: HashMap<EntityId, EntityId> = HashMap::new();
    let mut uuid_of: HashMap<EntityId, [u8; 16]> = HashMap::new();
    for key in keys {
        let Ok(key) = key else { continue };
        if !key.ends_with("/_instance.toml") {
            continue;
        }
        let Ok(Some(uuid)) = db.path_to_uuid(&key) else { continue };
        let old = EntityId(legacy_stored_id_from_uuid(&uuid));
        let new = EntityId(stored_id_from_uuid(&uuid));
        if old != new {
            rename.insert(old, new);
        }
        uuid_of.insert(new, uuid);
    }
    let report = match db.rekey_instance_cores(&rename) {
        Ok(r) => r,
        Err(e) => {
            warn!(target: "eustress_engine::bake_cores", error = %e, "core re-key failed; bake id upgrade deferred");
            return false;
        }
    };
    // Register the synthetic path of every core now under a canonical id. The
    // class names the path; the uuid-primary copy holds the same bytes as the
    // core, so read it there rather than scanning the partition again.
    let mut registered = 0usize;
    let mut failures = 0usize;
    for id in &report.present {
        let Some(uuid) = uuid_of.get(id) else { continue };
        let class = db
            .get_entity_core_by_uuid(uuid)
            .ok()
            .flatten()
            .and_then(|b| decode_instance_core(&b).ok())
            .map(|c| c.class_name);
        let Some(class) = class else { continue };
        match db.put_path_to_uuid(&synthetic_rel(&class, id.0), uuid) {
            Ok(()) => registered += 1,
            Err(_) => failures += 1,
        }
    }
    let _ = db.flush();
    info!(
        target: "eustress_engine::bake_cores",
        moved = report.moved,
        dropped_stale = report.dropped_stale,
        registered,
        failures,
        elapsed = ?t0.elapsed(),
        "baked cores re-keyed to the engine's stored-id derivation"
    );
    failures == 0
}

pub fn bake_once(space_root: &Path, db: &dyn WorldDb) {
    let prior = baked_version(space_root);
    if prior == Some(BAKE_VERSION) {
        return;
    }
    // Bakes before v3 keyed their cores by the legacy id. Move them first, so
    // the reconcile below and every later reader address them canonically.
    // This runs with no marker too: deleting the marker is the documented way
    // to force a re-bake, and on a Space that still holds legacy cores the
    // bake would otherwise write canonical ones beside them, two cores for
    // every part.
    if !rekey_legacy_cores(db) {
        warn!(
            target: "eustress_engine::bake_cores",
            version = ?prior,
            "bake id upgrade incomplete; left at the previous version, the next open retries"
        );
        return;
    }
    if prior == Some(2) {
        // v2 to v3 changes ids, not eligibility. Re-baking from the tree here
        // would overwrite the edits a streaming Space made to its cores and
        // re-create the cores of parts deleted since, so the re-key is the
        // whole upgrade.
        stamp(space_root);
        return;
    }
    if let Some(v) = prior {
        info!(
            target: "eustress_engine::bake_cores",
            from = v, to = BAKE_VERSION,
            "bake version changed: re-deriving eligibility and removing cores the previous version wrote for entities that should stay in the tree"
        );
    }
    let t0 = std::time::Instant::now();
    let sum = bake_tree_to_cores(space_root, db);
    info!(
        target: "eustress_engine::bake_cores",
        examined = sum.examined,
        baked = sum.baked,
        skipped_parent = sum.skipped_parent,
        skipped_filesystem = sum.skipped_filesystem,
        removed = sum.removed,
        unparsable = sum.unparsable,
        write_failures = sum.write_failures,
        elapsed = ?t0.elapsed(),
        "tree → entities bake complete (streaming gate now sees the real core count)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_key_walks_one_level_up() {
        assert_eq!(
            parent_key("Workspace/A/B/_instance.toml").as_deref(),
            Some("Workspace/A/_instance.toml")
        );
    }

    #[test]
    fn service_level_entity_has_no_entity_parent() {
        // `Workspace/A` is a top-level entity: its parent folder is the
        // service, which is not an entity.
        assert_eq!(parent_key("Workspace/A/_instance.toml"), None);
    }

    #[test]
    fn non_entity_keys_have_no_parent() {
        assert_eq!(parent_key("Workspace/_service.toml"), None);
        assert_eq!(parent_key("ServerScriptService/main.rune"), None);
    }

    /// The bake must derive exactly what `create_binary_instance`, the
    /// importer, promote, the bridge and Delete derive, or none of them can
    /// address a baked core from its uuid.
    #[test]
    fn stored_id_matches_the_engine_derivation() {
        let uuid: [u8; 16] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 9, 9, 9, 9, 9, 9, 9, 9,
        ];
        let canonical = u64::from_be_bytes(uuid[0..8].try_into().unwrap());
        assert_eq!(stored_id_from_uuid(&uuid), canonical);
        assert_eq!(stored_id_from_uuid(&uuid), 0x0123_4567_89ab_cdef);
    }

    /// The legacy id is what v1 and v2 wrote. It must differ from the
    /// canonical one for an ordinary uuid (that difference is the bug), and
    /// it keeps its never-zero nudge so pre-v3 cores stay findable.
    #[test]
    fn legacy_id_is_the_little_endian_reading() {
        let uuid: [u8; 16] = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 9, 9, 9, 9, 9, 9, 9, 9,
        ];
        assert_eq!(legacy_stored_id_from_uuid(&uuid), 0xefcd_ab89_6745_2301);
        assert_ne!(legacy_stored_id_from_uuid(&uuid), stored_id_from_uuid(&uuid));
        assert_eq!(legacy_stored_id_from_uuid(&[0u8; 16]), 1);
    }

    /// Delete parses this shape back into a stored id
    /// (`keybindings::parse_synthetic_bin_path`), and a core spawns under it
    /// (`world_db_binary::synthetic_path`).
    #[test]
    fn synthetic_rel_matches_the_spawn_path() {
        assert_eq!(
            synthetic_rel("Part", 0xff),
            "Workspace/__bin_Part_00000000000000ff/_instance.toml"
        );
    }

    #[test]
    fn distinct_uuids_give_distinct_ids() {
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        a[0] = 1;
        b[0] = 2;
        assert_ne!(stored_id_from_uuid(&a), stored_id_from_uuid(&b));
    }

    fn local(pos: [f32; 3]) -> TransformData {
        let mut t = TransformData::default();
        t.position = pos;
        t.rotation = [0.0, 0.0, 0.0, 1.0];
        t.scale = [1.0, 1.0, 1.0];
        t
    }

    #[test]
    fn compose_translates_child_into_parent_space() {
        let world = compose(&Transform::from_xyz(100.0, 0.0, 0.0), &local([10.0, 0.0, 0.0]));
        assert!((world.translation.x - 110.0).abs() < 0.001);
    }

    #[test]
    fn compose_applies_parent_rotation_to_child_offset() {
        // Parent yawed 90 degrees: a child at +X lands on -Z.
        let parent = Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        let world = compose(&parent, &local([10.0, 0.0, 0.0]));
        assert!(
            world.translation.z < -9.0,
            "expected the offset rotated onto -Z, got {:?}",
            world.translation
        );
    }

    #[test]
    fn compose_scales_child_offset_by_parent_scale() {
        let world = compose(&Transform::from_scale(Vec3::splat(2.0)), &local([10.0, 0.0, 0.0]));
        assert!((world.translation.x - 20.0).abs() < 0.001);
    }

    #[test]
    fn identity_parent_leaves_the_local_transform_alone() {
        let world = compose(&Transform::IDENTITY, &local([5.0, 6.0, 7.0]));
        assert!((world.translation - Vec3::new(5.0, 6.0, 7.0)).length() < 0.001);
    }
}

/// The bake, its v2 → v3 upgrade and both delete paths against a real Fjall
/// database. Each opens a keyspace and the delete tests install it as the
/// process-wide active DB, so they are `#[ignore]`d: run them with
/// `cargo test -p eustress-engine --lib space::bake_cores::db_tests -- --ignored --test-threads=1`.
#[cfg(test)]
mod db_tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;

    const UUID_A: &str = "0123456789abcdef0011223344556677";
    const UUID_B: &str = "fedcba98765432100011223344556677";
    const UUID_C: &str = "a1a2a3a4a5a6a7a8b1b2b3b4b5b6b7b8";

    fn uuid(hex: &str) -> [u8; 16] {
        eustress_common::instance_create::uuid_hex_to_bytes(hex).unwrap()
    }

    fn temp_space(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "eustress_bake_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("world.fjalldb")).unwrap();
        dir
    }

    fn open(space: &Path) -> Arc<dyn WorldDb> {
        eustress_worlddb::backend::open(&space.join("world.fjalldb")).unwrap()
    }

    /// A bare Part at `pos`, with the identity rows `migrate_identity` would
    /// have written, optionally also as a folder on disk. No "mesh" anywhere in
    /// the text, or the shared predicate keeps it in the tree.
    fn seed(db: &dyn WorldDb, space: &Path, name: &str, hex: &str, pos: [f32; 3], on_disk: bool) -> String {
        let rel = format!("Workspace/{name}/_instance.toml");
        let text = format!(
            "[metadata]\nclass_name = \"Part\"\nname = \"{name}\"\nuuid = \"{hex}\"\n\n[transform]\nposition = [{}, {}, {}]\n",
            pos[0], pos[1], pos[2]
        );
        db.put_file(&rel, text.as_bytes()).unwrap();
        db.put_path_to_uuid(&rel, &uuid(hex)).unwrap();
        db.put_uuid_to_path(&uuid(hex), &rel).unwrap();
        if on_disk {
            let folder = space.join("Workspace").join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("_instance.toml"), text).unwrap();
        }
        rel
    }

    fn core_ids(db: &dyn WorldDb) -> Vec<u64> {
        let mut ids: Vec<u64> = db
            .iter_instance_cores()
            .unwrap()
            .into_iter()
            .map(|(id, _)| id.0)
            .collect();
        ids.sort();
        ids
    }

    fn marker(space: &Path) -> String {
        std::fs::read_to_string(space.join(".eustress").join(MARKER)).unwrap_or_default()
    }

    #[test]
    #[ignore = "opens a Fjall keyspace; run with --ignored --test-threads=1"]
    fn fresh_bake_uses_the_canonical_id_and_registers_the_synthetic_path() {
        let space = temp_space("fresh");
        let db = open(&space);
        seed(db.as_ref(), &space, "Brick", UUID_A, [10.0, 2.0, 5.0], false);

        bake_once(&space, db.as_ref());

        let be = stored_id_from_uuid(&uuid(UUID_A));
        assert_eq!(core_ids(db.as_ref()), vec![be]);
        assert_eq!(
            db.path_to_uuid(&synthetic_rel("Part", be)).unwrap(),
            Some(uuid(UUID_A))
        );
        assert_eq!(marker(&space), BAKE_VERSION.to_string());
        drop(db);
        let _ = std::fs::remove_dir_all(&space);
    }

    /// A v2 Space: A was baked under the legacy id; B's core is gone (deleted
    /// in a streaming Space before this fix) but its tree row remains. The
    /// upgrade must move A and must NOT re-create B.
    #[test]
    #[ignore = "opens a Fjall keyspace; run with --ignored --test-threads=1"]
    fn v2_upgrade_rekeys_cores_and_does_not_rebake() {
        let space = temp_space("v2");
        let db = open(&space);
        seed(db.as_ref(), &space, "Brick", UUID_A, [10.0, 2.0, 5.0], false);
        seed(db.as_ref(), &space, "Gone", UUID_B, [300.0, 2.0, 5.0], false);
        let core = {
            let mut c = super::instance_to_arch(
                &toml::from_str::<InstanceDefinition>(
                    std::str::from_utf8(&db.get_file("Workspace/Brick/_instance.toml").unwrap().unwrap()).unwrap(),
                )
                .unwrap(),
            );
            c.t = [10.0, 2.0, 5.0];
            encode_instance_core(&c).unwrap()
        };
        let legacy = legacy_stored_id_from_uuid(&uuid(UUID_A));
        db.put_instance_core(EntityId(legacy), (10.0, 2.0, 5.0), &core).unwrap();
        db.put_entity_core_by_uuid(&uuid(UUID_A), &core).unwrap();
        std::fs::create_dir_all(space.join(".eustress")).unwrap();
        std::fs::write(space.join(".eustress").join(MARKER), "2").unwrap();

        bake_once(&space, db.as_ref());

        let be = stored_id_from_uuid(&uuid(UUID_A));
        let after: Vec<(u64, Vec<u8>)> = db
            .iter_instance_cores()
            .unwrap()
            .into_iter()
            .map(|(id, b)| (id.0, b))
            .collect();
        assert_eq!(after, vec![(be, core)], "moved to the canonical id, bytes intact, B not re-created");
        assert_eq!(
            db.path_to_uuid(&synthetic_rel("Part", be)).unwrap(),
            Some(uuid(UUID_A))
        );
        assert_eq!(marker(&space), BAKE_VERSION.to_string());
        drop(db);
        let _ = std::fs::remove_dir_all(&space);
    }

    /// Below the streaming threshold a baked part is spawned from its tree
    /// row, so Delete takes the tree path. Its core must go too, or boot-load
    /// spawns it again next session.
    #[test]
    #[ignore = "opens a Fjall keyspace and sets the process-wide active DB; run with --ignored --test-threads=1"]
    fn deleting_a_tree_spawned_baked_part_removes_its_core() {
        let space = temp_space("small");
        let db = open(&space);
        let rel = seed(db.as_ref(), &space, "Brick", UUID_A, [10.0, 2.0, 5.0], false);
        bake_once(&space, db.as_ref());
        let be = stored_id_from_uuid(&uuid(UUID_A));
        assert_eq!(core_ids(db.as_ref()), vec![be], "precondition: baked");

        crate::space::active_db::set(db.clone(), space.clone());
        crate::space::active_db::purge_path_all_stores(&space.join(&rel), UUID_A, "Part");
        crate::space::active_db::clear();

        assert!(core_ids(db.as_ref()).is_empty(), "the baked core is gone");
        assert!(db.get_file(&rel).unwrap().is_none());
        assert!(db.get_entity_core_by_uuid(&uuid(UUID_A)).unwrap().is_none());
        assert!(db.path_to_uuid(&synthetic_rel("Part", be)).unwrap().is_none());
        drop(db);
        let _ = std::fs::remove_dir_all(&space);
    }

    /// Above the threshold the same part is spawned from its core with no uuid
    /// and only its synthetic path, so Delete takes the binary path. Its tree
    /// half (rows, `#bin`, identity, folder on disk) must go too. An unrelated
    /// baked part must be untouched.
    #[test]
    #[ignore = "opens a Fjall keyspace and sets the process-wide active DB; run with --ignored --test-threads=1"]
    fn deleting_a_core_spawned_baked_part_removes_its_tree_half() {
        let space = temp_space("large");
        let db = open(&space);
        let rel = seed(db.as_ref(), &space, "Brick", UUID_A, [10.0, 2.0, 5.0], true);
        let other = seed(db.as_ref(), &space, "Keep", UUID_C, [-40.0, 2.0, 9.0], true);
        db.put_file(&format!("{rel}#bin"), b"edited").unwrap();
        bake_once(&space, db.as_ref());
        let be = stored_id_from_uuid(&uuid(UUID_A));
        let keep = stored_id_from_uuid(&uuid(UUID_C));
        assert_eq!(core_ids(db.as_ref()), { let mut v = vec![be, keep]; v.sort(); v });

        crate::space::active_db::set(db.clone(), space.clone());
        crate::space::active_db::delete_binary_instance(
            be,
            &[0u8; 16],
            "Part",
            [10.0, 2.0, 5.0],
            &synthetic_rel("Part", be),
        );
        crate::space::active_db::clear();

        assert_eq!(core_ids(db.as_ref()), vec![keep], "only the deleted part's core is gone");
        assert!(db.get_file(&rel).unwrap().is_none(), "tree row");
        assert!(db.get_file(&format!("{rel}#bin")).unwrap().is_none(), "#bin twin");
        assert!(db.get_entity_core_by_uuid(&uuid(UUID_A)).unwrap().is_none(), "uuid copy");
        assert!(db.uuid_to_path(&uuid(UUID_A)).unwrap().is_none(), "uuid_to_path");
        assert!(db.path_to_uuid(&rel).unwrap().is_none(), "path_to_uuid");
        assert!(!space.join("Workspace").join("Brick").exists(), "folder left Workspace");
        let trashed = walk(&space.join(".eustress").join("trash"))
            .into_iter()
            .any(|p| p.ends_with(PathBuf::from("Brick").join("_instance.toml")));
        assert!(trashed, "folder is in .eustress/trash");
        assert!(db.get_file(&other).unwrap().is_some(), "the other part's row survives");
        assert!(space.join("Workspace").join("Keep").exists(), "the other part's folder survives");
        drop(db);
        let _ = std::fs::remove_dir_all(&space);
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        }
        out
    }
}

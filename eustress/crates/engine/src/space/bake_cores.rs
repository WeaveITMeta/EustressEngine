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
use eustress_worlddb::{encode_instance_core, EntityId, WorldDb};

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
const BAKE_VERSION: u32 = 2;

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

/// Derive the stable persistence id from the entity UUID.
///
/// `stored_id` must be stable across sessions and must not collide with ids
/// the engine mints at create time. The first 8 bytes of the UUID give both
/// properties for free, and ties the core to the identity already recorded in
/// `path_to_uuid` by the UUID migration pass. `0` is reserved (some call sites
/// treat it as "unset"), so it is nudged to 1.
fn stored_id_from_uuid(uuid: &[u8; 16]) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&uuid[..8]);
    let id = u64::from_le_bytes(b);
    if id == 0 {
        1
    } else {
        id
    }
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
        sum.baked += 1;
    }

    let _ = db.flush();
    stamp(space_root);
    sum
}

/// Run the bake once per Space, before the streaming decision reads the core
/// count. Cheap no-op on an already-baked Space (one `Path::exists`).
pub fn bake_once(space_root: &Path, db: &dyn WorldDb) {
    let prior = baked_version(space_root);
    if prior == Some(BAKE_VERSION) {
        return;
    }
    if let Some(v) = prior {
        info!(
            target: "eustress_engine::bake_cores",
            from = v, to = BAKE_VERSION,
            "bake version changed — re-deriving eligibility and removing cores the              previous version wrote for entities that should stay in the tree"
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

    #[test]
    fn stored_id_is_stable_and_never_zero() {
        let a = [7u8; 16];
        assert_eq!(stored_id_from_uuid(&a), stored_id_from_uuid(&a));
        assert_ne!(stored_id_from_uuid(&[0u8; 16]), 0);
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

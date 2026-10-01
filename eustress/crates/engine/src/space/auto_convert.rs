//! Automatic, in-process TOML→Fjall **import** (dual model) + UUID migration.
//!
//! DIRECTION CHANGE 2026-05-17 — binary-first **with TOML import**:
//! the Documents `Eustress/<Universe>/<Space>/` TOML hierarchy and the
//! binary Fjall store COEXIST. This is now an IMPORT-ONLY seed: when a
//! Space's Fjall `tree` partition is empty (first open), bring the disk
//! TOML into Fjall so the binary runtime store reflects it. It NEVER
//! removes, relocates, or "migrates away" the TOML — the hierarchy
//! stays as the editable import source and is honored at runtime
//! (runtime TOML edits flow through the file-watcher; full
//! TOML-edit→Fjall sync is the remaining runtime-honor wire).
//!
//! Binary ECS Fjall remains the PRIMARY runtime store. The import is
//! additive (`import_space` never clears the tree), so direct-to-DB
//! content (e.g. a generated benchmark grid) is preserved.
//!
//! ## Wave 2.1 UUID migration (IDENTITY.md §6)
//!
//! After the seed, the new `migrate_tree_to_uuid` pass walks every
//! `_instance.toml` in the `tree` partition, derives or preserves a
//! 32-char-hex UUID per IDENTITY.md §3.1, and writes the new
//! UUID-keyed partitions (`entities_uuid`, `path_to_uuid`,
//! `uuid_to_path`, `class_index`). The `tree` partition rows stay
//! untouched — Wave 3 deletes them after verifying the indexes are
//! populated. Gated on `header.migrated_to_uuid_at` — second open is a
//! no-op (the migration's own `migration_checkpoint = "done"` meta key
//! short-circuits even faster than the header check).

use std::path::Path;

use eustress_common::instance_create::{
    derive_uuid_for_import, fresh_uuid_for_create, is_valid_uuid,
};
use eustress_worlddb::{
    encode_instance_core, header::WorldHeader, migrate_identity, WorldDb,
};

use crate::space::arch_instance::instance_to_arch;
use crate::space::instance_loader::InstanceDefinition;

/// Seed the binary store from the Space's TOML hierarchy if needed, then
/// run the IDENTITY.md UUID migration if it hasn't run yet for this Space.
/// Returns `true` when it's safe to source the Space from Fjall, and
/// `false` only when the tree is empty AND the seed import failed (the
/// caller then falls back to the disk/TOML source so the engine still
/// boots and reads TOML directly).
pub fn convert_space_if_needed(space_root: &Path, db: &dyn WorldDb) -> bool {
    let seeded = match db.tree_is_empty() {
        Ok(true) => {
            // First open of this Space's DB → seed it from the disk
            // TOML hierarchy (additive). TOML is KEPT.
            match eustress_worlddb::import::import_space(db, space_root) {
                Ok(s) => {
                    tracing::info!(
                        target: "eustress_engine::world_db",
                        files = s.files_imported,
                        dirs = s.dirs_walked,
                        bytes = s.bytes_imported,
                        "seed: TOML→Fjall import (dual model — TOML hierarchy kept as import source)"
                    );
                    true
                }
                Err(e) => {
                    tracing::warn!(
                        target: "eustress_engine::world_db",
                        error = %e,
                        "seed import failed — sourcing this Space from disk/TOML"
                    );
                    false
                }
            }
        }
        // Already seeded — Fjall is the runtime store; runtime TOML
        // edits are picked up via the file-watcher.
        Ok(false) => true,
        Err(e) => {
            tracing::warn!(
                target: "eustress_engine::world_db",
                error = %e,
                "tree_is_empty check failed — sourcing this Space from disk/TOML"
            );
            false
        }
    };

    if seeded {
        // IDENTITY.md §6 UUID migration — runs ONCE per Space, gated on the
        // header's `migrated_to_uuid_at` stamp. The migration's own
        // checkpoint sentinel ("done") makes the inner skip cheap; the
        // header check here keeps us from even looking at the meta partition
        // for already-migrated Spaces.
        run_uuid_migration_if_needed(space_root, db);
    }

    seeded
}

/// Bake one TOML body into a tagged rkyv `ArchInstanceCore` and produce
/// the migration's per-entity payload. The closure passed to
/// `migrate_tree_to_uuid`; bridges the worlddb crate (engine-free) to
/// the engine's parse path.
fn bake_for_migration(
    rel_path: &str,
    bytes: &[u8],
) -> Result<migrate_identity::BakedEntity, eustress_worlddb::Error> {
    let raw = std::str::from_utf8(bytes).map_err(|e| {
        eustress_worlddb::Error::Other(format!(
            "bake_for_migration utf-8 at {rel_path}: {e}",
        ))
    })?;
    // Reach the metadata table FIRST — extract uuid + class without
    // committing to a full parse, so a malformed-but-key-recoverable
    // TOML can still be migrated by-path.
    let mut doc: toml::Value = raw.parse::<toml::Value>().map_err(|e| {
        eustress_worlddb::Error::Other(format!(
            "bake_for_migration toml at {rel_path}: {e}",
        ))
    })?;

    // Identify the uuid. If absent / invalid, mint one via the path-based
    // seed (IDENTITY.md §3.1) and prepare to write it back. Both the read
    // and the stamp go through the metadata table in whichever spelling
    // the file uses (see `metadata_key`).
    let (uuid_hex, rewritten_toml) = {
        let on_disk = metadata_uuid(&doc);
        let valid = on_disk
            .as_deref()
            .map(is_valid_uuid)
            .unwrap_or(false);
        if valid {
            // Preserve verbatim.
            (on_disk.unwrap(), None)
        } else {
            // Mint fresh via the path-based seed.
            let now_nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let u = derive_uuid_for_import(rel_path, now_nanos);
            stamp_uuid(&mut doc, &u).map_err(|e| {
                eustress_worlddb::Error::Other(format!("{e} at {rel_path}"))
            })?;
            let serialised = toml::to_string_pretty(&doc).map_err(|e| {
                eustress_worlddb::Error::Other(format!(
                    "bake_for_migration serialise at {rel_path}: {e}",
                ))
            })?;
            (u, Some(serialised.into_bytes()))
        }
    };

    // Now parse the (possibly-stamped) document into the typed
    // `InstanceDefinition` so we can call `instance_to_arch`. The typed
    // fields are snake_case, so parse a key-normalised copy, the same
    // normalisation the loader's heal applies: a file written as
    // `[Metadata] ClassName = ...` keeps its class instead of failing the
    // parse or falling back to the default class. The file written back
    // (`rewritten_toml`) keeps the author's spelling.
    let mut typed = doc.clone();
    eustress_common::class_schema::normalise_keys(&mut typed);
    let def: InstanceDefinition = typed.try_into().map_err(|e| {
        eustress_worlddb::Error::Other(format!(
            "bake_for_migration parse InstanceDefinition at {rel_path}: {e}",
        ))
    })?;

    let class_name = def.metadata.class_name.clone();
    let arch = instance_to_arch(&def);
    let core_bytes = encode_instance_core(&arch)?;

    Ok(migrate_identity::BakedEntity {
        core_bytes,
        class_name,
        rewritten_toml,
        uuid_hex,
    })
}

/// The key of the document's metadata table, in whichever spelling the file
/// uses: `metadata`, or the older PascalCase `Metadata`. Stamping into a
/// hardcoded `metadata` gave a `[Metadata]` file a SECOND table holding only
/// the uuid; the loader then read that one, found no class, and loaded the
/// entity as a Folder, and a later typed write saved it as a Part. When a
/// file already holds both spellings, the lowercase one is preferred, which
/// is the table this function used to create.
fn metadata_key(doc: &toml::Value) -> Option<String> {
    let table = doc.as_table()?;
    if table.get("metadata").is_some_and(toml::Value::is_table) {
        return Some("metadata".to_string());
    }
    table
        .iter()
        .find(|(k, v)| k.eq_ignore_ascii_case("metadata") && v.is_table())
        .map(|(k, _)| k.clone())
}

/// The uuid the file already carries, under `uuid` or `Uuid`. Reading only
/// the lowercase spelling made a `[Metadata]` file look uuid-less, so the
/// migration minted a new identity for an entity that had one.
fn metadata_uuid(doc: &toml::Value) -> Option<String> {
    let key = metadata_key(doc)?;
    doc.get(&key)?
        .as_table()?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("uuid"))
        .and_then(|(_, v)| v.as_str())
        .map(str::to_owned)
}

/// Write `uuid` into the file's own metadata table, creating `[metadata]`
/// only when there is none. Any existing uuid key, in either spelling, is
/// replaced, so the table never holds two.
fn stamp_uuid(doc: &mut toml::Value, uuid: &str) -> Result<(), String> {
    let key = metadata_key(doc).unwrap_or_else(|| "metadata".to_string());
    let table = doc.as_table_mut().ok_or("TOML root is not a table")?;
    let meta = table
        .entry(key)
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
        .as_table_mut()
        .ok_or("metadata is not a table")?;
    let stale: Vec<String> = meta
        .keys()
        .filter(|k| k.eq_ignore_ascii_case("uuid"))
        .cloned()
        .collect();
    for k in stale {
        meta.remove(&k);
    }
    meta.insert("uuid".to_string(), toml::Value::String(uuid.to_string()));
    Ok(())
}

/// Compile-time check the import-only helper is referenced. (Avoids a
/// dead_code warning when feature flags trim away the migration path.)
#[allow(dead_code)]
fn _ensure_fresh_uuid_referenced() -> String {
    // Keeps `fresh_uuid_for_create` reachable from this module's import
    // list — used by other engine surfaces; importing it here lets the
    // dispatcher search be one-stop.
    fresh_uuid_for_create()
}

/// Run `migrate_tree_to_uuid` if and only if the header lacks
/// `migrated_to_uuid_at`. Idempotent + crash-safe; see
/// `eustress_worlddb::migrate_identity` module docs.
fn run_uuid_migration_if_needed(space_root: &Path, db: &dyn WorldDb) {
    // The `.eustress` container holds the `header.bin` (one directory up
    // from `world.fjalldb/`, which the DB layer owns). The space_root
    // passed in here is the on-disk Space directory; the engine's
    // canonical container layout puts `header.bin` directly inside it
    // (see project_db_primary_migration memory).
    let header_dir = space_root;
    let mut header = match WorldHeader::read(header_dir) {
        Ok(Some(h)) => h,
        Ok(None) => {
            tracing::debug!(
                target: "eustress_engine::identity_migrate",
                space = %space_root.display(),
                "no header.bin — fresh Space; skipping UUID migration (creates will stamp uuids inline)"
            );
            return;
        }
        Err(e) => {
            tracing::warn!(
                target: "eustress_engine::identity_migrate",
                space = %space_root.display(),
                error = %e,
                "header.bin read failed — skipping UUID migration"
            );
            return;
        }
    };

    if header.is_migrated_to_uuid() {
        tracing::trace!(
            target: "eustress_engine::identity_migrate",
            space = %space_root.display(),
            "header already stamped migrated_to_uuid_at — migration skipped"
        );
        return;
    }

    // The bake_fn closure calls back into the engine's parse path
    // (`InstanceDefinition` + `instance_to_arch`). Keeping `worlddb`
    // engine-free means this closure crosses the crate boundary at this
    // call site — the engine knows how to parse rich TOMLs, the worlddb
    // crate doesn't.
    let bake_fn = |rel_path: &str,
                   bytes: &[u8]|
     -> Result<migrate_identity::BakedEntity, eustress_worlddb::Error> {
        bake_for_migration(rel_path, bytes)
    };

    match migrate_identity::migrate_tree_to_uuid(db, space_root, bake_fn) {
        Ok(report) => {
            if report.completed {
                header.mark_migrated_to_uuid();
                if let Err(e) = header.write(header_dir) {
                    tracing::warn!(
                        target: "eustress_engine::identity_migrate",
                        error = %e,
                        "failed to stamp header.migrated_to_uuid_at — migration will re-run next open (idempotent)"
                    );
                } else {
                    tracing::info!(
                        target: "eustress_engine::identity_migrate",
                        entities = report.entities_visited,
                        preserved = report.uuid_preserved,
                        generated = report.uuid_generated,
                        failures = report.bake_failures,
                        collisions = report.collisions,
                        "UUID migration complete; header stamped"
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                target: "eustress_engine::identity_migrate",
                error = %e,
                "UUID migration failed; will retry next open (resumable from checkpoint)"
            );
        }
    }
}

#[cfg(test)]
mod uuid_stamp_tests {
    use super::*;

    const UUID: &str = "0123456789abcdef0011223344556677";

    /// The metadata tables of a document, in any spelling.
    fn metadata_tables(doc: &toml::Value) -> Vec<(String, toml::value::Table)> {
        doc.as_table()
            .unwrap()
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("metadata"))
            .map(|(k, v)| (k.clone(), v.as_table().unwrap().clone()))
            .collect()
    }

    /// A PascalCase file with no uuid: the stamp lands in `[Metadata]`, no
    /// second table appears, and the class survives into the bake.
    #[test]
    fn a_pascal_case_file_is_stamped_in_place_and_keeps_its_class() {
        let src = "[Metadata]\nClassName = \"Model\"\nName = \"Car\"\n";
        let baked = bake_for_migration("Workspace/Car/_instance.toml", src.as_bytes()).unwrap();
        assert_eq!(baked.class_name, "Model", "not the default class");

        let written = String::from_utf8(baked.rewritten_toml.expect("a uuid was minted")).unwrap();
        let doc: toml::Value = written.parse().unwrap();
        let tables = metadata_tables(&doc);
        assert_eq!(tables.len(), 1, "one metadata table, not two: {written}");
        let (key, table) = &tables[0];
        assert_eq!(key, "Metadata", "the author's spelling is kept");
        assert_eq!(table.get("ClassName").and_then(|v| v.as_str()), Some("Model"));
        assert_eq!(table.get("uuid").and_then(|v| v.as_str()), Some(baked.uuid_hex.as_str()));
    }

    /// A PascalCase file that already has a valid `Uuid` keeps that identity
    /// and is not rewritten.
    #[test]
    fn an_existing_pascal_case_uuid_is_preserved() {
        let src = format!("[Metadata]\nClassName = \"Part\"\nUuid = \"{UUID}\"\n");
        let baked = bake_for_migration("Workspace/Brick/_instance.toml", src.as_bytes()).unwrap();
        assert_eq!(baked.uuid_hex, UUID);
        assert!(baked.rewritten_toml.is_none());
        assert_eq!(baked.class_name, "Part");
    }

    /// No metadata at all: `[metadata]` is created. An invalid uuid in
    /// either spelling is replaced, leaving exactly one uuid key.
    #[test]
    fn stamp_creates_or_replaces_without_duplicating() {
        let mut bare: toml::Value = "[transform]\nposition = [0.0, 0.0, 0.0]\n".parse().unwrap();
        stamp_uuid(&mut bare, UUID).unwrap();
        assert_eq!(metadata_uuid(&bare).as_deref(), Some(UUID));
        assert_eq!(metadata_tables(&bare)[0].0, "metadata");

        let mut stale: toml::Value = "[metadata]\nclass_name = \"Part\"\nUuid = \"not-a-uuid\"\n"
            .parse()
            .unwrap();
        stamp_uuid(&mut stale, UUID).unwrap();
        let (_, table) = &metadata_tables(&stale)[0];
        let uuid_keys: Vec<&str> = table
            .keys()
            .map(String::as_str)
            .filter(|k| k.eq_ignore_ascii_case("uuid"))
            .collect();
        assert_eq!(uuid_keys, vec!["uuid"]);
        assert_eq!(metadata_uuid(&stale).as_deref(), Some(UUID));
    }
}

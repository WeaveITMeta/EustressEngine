//! Checkpoints: a consistent copy of every partition of an open world
//! database, taken while writers keep committing, and a fresh database built
//! from one.
//!
//! A checkpoint reads every partition at ONE sequence number
//! (`Keyspace::instant` + `snapshot_at`), so a commit that lands during the
//! copy is either wholly in it or wholly after it. A restore puts back exactly
//! the keys and values it holds; nothing is re-derived.
//!
//! The caller may elide values it can supply again at restore. The engine
//! elides the tree's copies of files git versions beside the database. An
//! elided entry keeps its key and a BLAKE3 hash of its value, and the restore
//! asks the caller for the bytes.
//!
//! File layout: `EWCK`, a little-endian u16 version, then one zstd stream of
//! records, each led by a tag byte:
//! - `P`: u16 name length, name. The partition the records after it go to.
//! - `V`: u32 key length, key, u32 value length, value.
//! - `E`: u32 key length, key, the 32-byte BLAKE3 hash of the elided value.
//! - `Z`: u64 record count. The end; a file without it is incomplete.
//!
//! A checkpoint is written to `<name>.partial` and renamed into place once
//! complete, so a crash never leaves a file that reads as whole.

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::fjall_backend::FjallWorldDb;

/// The file's first four bytes.
pub const MAGIC: &[u8; 4] = b"EWCK";
/// The layout version this build writes and reads.
pub const VERSION: u16 = 1;
/// Records written per restore batch.
const BATCH: usize = 4096;

/// What a checkpoint holds.
#[derive(Debug, Clone, Default)]
pub struct CheckpointInfo {
    /// The sequence number every partition was read at.
    pub seqno: u64,
    /// Records per partition, in the order written (partition names sorted).
    pub partitions: Vec<(String, u64)>,
    /// Records whose value was elided (key and hash kept).
    pub elided: u64,
    /// Size of the checkpoint file in bytes.
    pub bytes: u64,
}

/// What a restore wrote.
#[derive(Debug, Clone, Default)]
pub struct RestoreSummary {
    /// Records written per partition.
    pub partitions: Vec<(String, u64)>,
    /// Elided values the caller supplied again.
    pub resupplied: u64,
    /// Elided values the caller could not supply: `partition/key`. Their keys
    /// are absent from the restored database.
    pub missing: Vec<String>,
    /// Elided values supplied with bytes whose hash differs from the
    /// checkpoint's: `partition/key`. Written as supplied.
    pub changed: Vec<String>,
}

fn partial_path(out: &Path) -> PathBuf {
    let mut name = out.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".partial");
    out.with_file_name(name)
}

fn write_len_bytes<W: Write>(w: &mut W, bytes: &[u8]) -> Result<()> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| Error::Other(format!("checkpoint record of {} bytes is too large", bytes.len())))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(bytes)?;
    Ok(())
}

fn read_len_bytes<R: Read>(r: &mut R) -> Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let mut bytes = vec![0u8; u32::from_le_bytes(len) as usize];
    r.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn fjall_read(partition: &str, e: impl std::fmt::Display) -> Error {
    Error::Other(format!("checkpoint read of partition {partition}: {e}"))
}

impl FjallWorldDb {
    /// Write a checkpoint of every partition to `out`. `elide(partition,
    /// key)` names the records whose value the caller will supply again at
    /// restore. Safe while other threads commit: the copy is of one sequence
    /// number. Reached through [`crate::WorldDb::checkpoint_to`].
    pub fn write_checkpoint(&self, out: &Path, elide: &dyn Fn(&str, &[u8]) -> bool) -> Result<CheckpointInfo> {
        let _span = tracing::info_span!("worlddb.checkpoint", out = %out.display()).entered();
        let keyspace = self.keyspace();
        let instant = keyspace.instant();
        let mut names: Vec<String> = keyspace.list_partitions().iter().map(|n| n.to_string()).collect();
        names.sort();

        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = partial_path(out);
        let mut file = BufWriter::new(std::fs::File::create(&tmp)?);
        file.write_all(MAGIC)?;
        file.write_all(&VERSION.to_le_bytes())?;
        let mut z = zstd::stream::Encoder::new(file, 3)?;

        let mut info = CheckpointInfo { seqno: instant, ..Default::default() };
        let mut total = 0u64;
        for name in names {
            let partition = keyspace.open_partition(&name, fjall::PartitionCreateOptions::default())?;
            let name_len = u16::try_from(name.len())
                .map_err(|_| Error::Other(format!("partition name {name} is too long")))?;
            z.write_all(b"P")?;
            z.write_all(&name_len.to_le_bytes())?;
            z.write_all(name.as_bytes())?;
            let mut count = 0u64;
            for kv in partition.snapshot_at(instant).iter() {
                let (key, value) = kv.map_err(|e| fjall_read(&name, e))?;
                if elide(&name, &key) {
                    z.write_all(b"E")?;
                    write_len_bytes(&mut z, &key)?;
                    z.write_all(blake3::hash(&value).as_bytes())?;
                    info.elided += 1;
                } else {
                    z.write_all(b"V")?;
                    write_len_bytes(&mut z, &key)?;
                    write_len_bytes(&mut z, &value)?;
                }
                count += 1;
            }
            total += count;
            info.partitions.push((name, count));
        }
        z.write_all(b"Z")?;
        z.write_all(&total.to_le_bytes())?;
        let file = z.finish()?;
        let file = file.into_inner().map_err(|e| Error::Io(e.into_error()))?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, out)?;
        info.bytes = std::fs::metadata(out)?.len();
        tracing::info!(
            target: "eustress_worlddb",
            seqno = info.seqno,
            records = total,
            elided = info.elided,
            bytes = info.bytes,
            "checkpoint written"
        );
        Ok(info)
    }
}

/// Build a fresh database at `dir`, which must not exist, from `checkpoint`.
/// `resupply(partition, key)` returns an elided value's bytes, or `None` when
/// it cannot, in which case the key is left out and reported.
///
/// The database is created through [`FjallWorldDb::open`] first, so every
/// partition it owns has its own create-time options (the `datasets` value
/// separation, for one) before any record lands.
pub fn restore_checkpoint(
    checkpoint: &Path,
    dir: &Path,
    resupply: &mut dyn FnMut(&str, &[u8]) -> Option<Vec<u8>>,
) -> Result<RestoreSummary> {
    let _span = tracing::info_span!("worlddb.restore", dir = %dir.display()).entered();
    if dir.exists() {
        return Err(Error::Other(format!("restore target {} already exists", dir.display())));
    }
    let mut r = BufReader::new(std::fs::File::open(checkpoint)?);
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(Error::Other(format!("{} is not a checkpoint", checkpoint.display())));
    }
    let mut version = [0u8; 2];
    r.read_exact(&mut version)?;
    let version = u16::from_le_bytes(version);
    if version != VERSION {
        return Err(Error::Other(format!(
            "{} is checkpoint version {version}; this build reads {VERSION}",
            checkpoint.display()
        )));
    }
    let mut z = zstd::stream::Decoder::with_buffer(r)?;

    std::fs::create_dir_all(dir)?;
    let db = FjallWorldDb::open(dir)?;
    let keyspace = db.keyspace().clone();
    let mut summary = RestoreSummary::default();
    let mut current: Option<(String, fjall::PartitionHandle, u64)> = None;
    let mut batch = keyspace.batch();
    let mut pending = 0usize;
    let mut total = 0u64;
    loop {
        let mut tag = [0u8; 1];
        z.read_exact(&mut tag).map_err(|e| {
            Error::Other(format!("{} ends before its end record: {e}", checkpoint.display()))
        })?;
        match tag[0] {
            b'P' => {
                if let Some((name, _, count)) = current.take() {
                    summary.partitions.push((name, count));
                }
                let mut len = [0u8; 2];
                z.read_exact(&mut len)?;
                let mut name = vec![0u8; u16::from_le_bytes(len) as usize];
                z.read_exact(&mut name)?;
                let name = String::from_utf8(name)
                    .map_err(|_| Error::Other("checkpoint partition name is not UTF-8".to_string()))?;
                let partition = keyspace.open_partition(&name, fjall::PartitionCreateOptions::default())?;
                current = Some((name, partition, 0));
            }
            b'V' | b'E' => {
                let elided = tag[0] == b'E';
                let (name, partition, count) = current
                    .as_mut()
                    .ok_or_else(|| Error::Other("checkpoint record before any partition".to_string()))?;
                let key = read_len_bytes(&mut z)?;
                total += 1;
                let value = if elided {
                    let mut hash = [0u8; 32];
                    z.read_exact(&mut hash)?;
                    let label = format!("{name}/{}", String::from_utf8_lossy(&key));
                    match resupply(name, &key) {
                        Some(bytes) => {
                            if blake3::hash(&bytes).as_bytes() != &hash {
                                summary.changed.push(label);
                            }
                            summary.resupplied += 1;
                            bytes
                        }
                        None => {
                            summary.missing.push(label);
                            continue;
                        }
                    }
                } else {
                    read_len_bytes(&mut z)?
                };
                batch.insert(partition, key, value);
                *count += 1;
                pending += 1;
                if pending >= BATCH {
                    std::mem::replace(&mut batch, keyspace.batch()).commit()?;
                    pending = 0;
                }
            }
            b'Z' => {
                let mut count = [0u8; 8];
                z.read_exact(&mut count)?;
                let count = u64::from_le_bytes(count);
                if count != total {
                    return Err(Error::Other(format!(
                        "{} holds {total} records but its end record says {count}",
                        checkpoint.display()
                    )));
                }
                break;
            }
            other => {
                return Err(Error::Other(format!(
                    "{} has an unknown record tag {other:#04x}",
                    checkpoint.display()
                )));
            }
        }
    }
    batch.commit()?;
    if let Some((name, _, count)) = current.take() {
        summary.partitions.push((name, count));
    }
    // The handle's counters were loaded from the fresh database; its drop
    // writes them back, so they are read again from the restored `meta`.
    db.reload_counters()?;
    keyspace.persist(fjall::PersistMode::SyncAll)?;
    drop(db);
    tracing::info!(
        target: "eustress_worlddb",
        records = total,
        resupplied = summary.resupplied,
        missing = summary.missing.len(),
        changed = summary.changed.len(),
        "checkpoint restored"
    );
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::WorldDb;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "eustress_checkpoint_{name}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    /// A checkpoint holds what was committed before it and nothing after;
    /// a restore puts those bytes back and asks for the elided ones.
    #[test]
    #[ignore = "opens Fjall keyspaces; run with --ignored --test-threads=1 (memory: feedback_worlddb_test_threads)"]
    fn a_restore_brings_back_the_checkpoint_and_only_the_checkpoint() {
        let live_dir = temp("live");
        std::fs::create_dir_all(&live_dir).unwrap();
        let live = FjallWorldDb::open(&live_dir).unwrap();
        live.put_file("Workspace/A/_instance.toml", b"[metadata]\nclass_name = \"Part\"\n").unwrap();
        live.put_file("Workspace/A/mesh.glb", b"glb-bytes").unwrap();
        live.put_file("Workspace/B/_instance.toml", b"before").unwrap();

        let file = temp("file").join("one.ewck");
        let info = live
            .checkpoint_to(&file, &|partition, key| partition == "tree" && key.ends_with(b".glb"))
            .unwrap();
        assert!(file.exists() && !partial_path(&file).exists());
        assert_eq!(info.elided, 1);

        // After the checkpoint: neither may come back.
        live.put_file("Workspace/C/_instance.toml", b"after").unwrap();
        live.put_file("Workspace/B/_instance.toml", b"edited after").unwrap();
        drop(live);

        let restored_dir = temp("restored");
        let mut asked = Vec::new();
        let summary = restore_checkpoint(&file, &restored_dir, &mut |partition, key| {
            asked.push(format!("{partition}/{}", String::from_utf8_lossy(key)));
            Some(b"glb-bytes".to_vec())
        })
        .unwrap();
        assert_eq!(asked, vec!["tree/Workspace/A/mesh.glb".to_string()]);
        assert_eq!(summary.resupplied, 1);
        assert!(summary.missing.is_empty() && summary.changed.is_empty(), "{summary:?}");

        let restored = FjallWorldDb::open(&restored_dir).unwrap();
        let get = |k: &str| restored.get_file(k).unwrap();
        assert_eq!(get("Workspace/A/_instance.toml").as_deref(), Some(&b"[metadata]\nclass_name = \"Part\"\n"[..]));
        assert_eq!(get("Workspace/A/mesh.glb").as_deref(), Some(&b"glb-bytes"[..]));
        assert_eq!(get("Workspace/B/_instance.toml").as_deref(), Some(&b"before"[..]));
        assert_eq!(get("Workspace/C/_instance.toml"), None);
        drop(restored);
        for d in [&live_dir, &restored_dir, &file.parent().unwrap().to_path_buf()] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// An elided value that comes back different, or not at all, is reported.
    #[test]
    #[ignore = "opens Fjall keyspaces; run with --ignored --test-threads=1 (memory: feedback_worlddb_test_threads)"]
    fn changed_and_missing_resupplies_are_reported() {
        let live_dir = temp("live2");
        std::fs::create_dir_all(&live_dir).unwrap();
        let live = FjallWorldDb::open(&live_dir).unwrap();
        live.put_file("Workspace/A/a.glb", b"one").unwrap();
        live.put_file("Workspace/B/b.glb", b"two").unwrap();
        let file = temp("file2").join("two.ewck");
        live.checkpoint_to(&file, &|p, k| p == "tree" && k.ends_with(b".glb")).unwrap();
        drop(live);

        let restored_dir = temp("restored2");
        let summary = restore_checkpoint(&file, &restored_dir, &mut |_, key| {
            key.ends_with(b"a.glb").then(|| b"not one".to_vec())
        })
        .unwrap();
        assert_eq!(summary.changed, vec!["tree/Workspace/A/a.glb".to_string()]);
        assert_eq!(summary.missing, vec!["tree/Workspace/B/b.glb".to_string()]);
        for d in [&live_dir, &restored_dir, &file.parent().unwrap().to_path_buf()] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// A file cut short before its end record is refused, not half-restored
    /// as if whole.
    #[test]
    #[ignore = "opens Fjall keyspaces; run with --ignored --test-threads=1 (memory: feedback_worlddb_test_threads)"]
    fn a_truncated_checkpoint_is_refused() {
        let live_dir = temp("live3");
        std::fs::create_dir_all(&live_dir).unwrap();
        let live = FjallWorldDb::open(&live_dir).unwrap();
        live.put_file("Workspace/A/_instance.toml", b"x").unwrap();
        let file = temp("file3").join("three.ewck");
        live.checkpoint_to(&file, &|_, _| false).unwrap();
        drop(live);
        let bytes = std::fs::read(&file).unwrap();
        std::fs::write(&file, &bytes[..bytes.len() - 4]).unwrap();
        let restored_dir = temp("restored3");
        assert!(restore_checkpoint(&file, &restored_dir, &mut |_, _| None).is_err());
        for d in [&live_dir, &restored_dir, &file.parent().unwrap().to_path_buf()] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

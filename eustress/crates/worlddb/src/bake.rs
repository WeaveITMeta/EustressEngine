//! Phase 5 — Fjall tree → chunked `.echk` export.
//!
//! Walks the `tree` partition (the faithful Space mirror), buckets
//! every `_instance.toml` by its world-space chunk coord, and writes
//! one `chunks/<cx>_<cz>.echk` per non-empty cell plus a
//! `manifest.toml`. Each chunk carries a blake3 content hash so the
//! publish path uploads only changed chunks to R2 (delta publish).
//!
//! ## `.echk` v0 container
//!
//! ```text
//! magic   "ECHK"            (4 bytes)
//! version u32 le            (= 1)
//! count   u32 le            (instances in this chunk)
//! repeat count times:
//!   path_len u32 le, path bytes (Space-relative, utf-8)
//!   data_len u32 le, data bytes (raw `_instance.toml`)
//! ```
//!
//! This is worlddb's own container — deterministic, append-free,
//! byte-stable for hashing. The [05] 56-byte `PackedInstance` is a
//! tighter wire layout the *engine* adapts to on top (it owns the
//! `eustress_common::streaming` types; this crate stays engine-free).
//! v0 is the real, shippable bake; PackedInstance interop is the
//! engine-side encoder, not a bake.rs change.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use crate::backend::WorldDb;
use crate::error::{Error, Result};

/// Default chunk edge in world units. Mirrors the streaming
/// `StreamingConfig::chunk_size` default so a baked chunk maps 1:1 to
/// a stream request.
pub const DEFAULT_CHUNK_SIZE: f64 = 256.0;

const ECHK_MAGIC: &[u8; 4] = b"ECHK";
const ECHK_VERSION: u32 = 1;

/// Summary returned by [`bake_to_echk`].
#[derive(Debug, Clone, Default)]
pub struct BakeSummary {
    /// Chunks written this run (content changed or new).
    pub chunks_written: usize,
    /// Chunks skipped because the blake3 matched the prior manifest.
    pub chunks_skipped: usize,
    /// Total entities baked across all chunks.
    pub entities_baked: usize,
    /// Compressed-equivalent bytes written under `chunks/`.
    pub bytes_written: u64,
}

/// One row in `manifest.toml`.
#[derive(Debug, Clone)]
struct ManifestEntry {
    cx: i32,
    cz: i32,
    file: String,
    size: u64,
    blake3: String,
    count: u32,
}

/// Bake the world's tree into `output/chunks/*.echk` + `output/manifest.toml`.
/// `output` is the `.eustress` world directory. Delta-aware: a chunk
/// whose freshly-encoded bytes hash to the same blake3 as the existing
/// manifest entry is left untouched (no rewrite, counted as skipped).
pub fn bake_to_echk(db: &dyn WorldDb, output: &Path) -> Result<BakeSummary> {
    bake_to_echk_with(db, output, DEFAULT_CHUNK_SIZE)
}

/// As [`bake_to_echk`] with an explicit chunk size.
pub fn bake_to_echk_with(
    db: &dyn WorldDb,
    output: &Path,
    chunk_size: f64,
) -> Result<BakeSummary> {
    let _span =
        tracing::info_span!("worlddb.bake", out = %output.display(), chunk = chunk_size).entered();

    let chunks_dir = output.join("chunks");
    std::fs::create_dir_all(&chunks_dir)?;

    // Bucket tree entities by chunk coord. Only `_instance.toml`
    // files carry a transform; everything else (scripts, _service)
    // rides in chunk (0,0) so a chunk load still reconstructs the
    // full sub-tree.
    let mut buckets: BTreeMap<(i32, i32), Vec<(String, Vec<u8>)>> = BTreeMap::new();
    for kv in db.iter_tree()? {
        let (path, bytes) = kv?;
        let (cx, cz) = if path.ends_with("_instance.toml") {
            chunk_of_instance(&bytes, chunk_size)
        } else {
            (0, 0)
        };
        buckets.entry((cx, cz)).or_default().push((path, bytes));
    }

    // Load the prior manifest (if any) for delta comparison.
    let manifest_path = output.join("manifest.toml");
    let prior: BTreeMap<(i32, i32), String> = read_prior_hashes(&manifest_path);

    let mut summary = BakeSummary::default();
    let mut manifest: Vec<ManifestEntry> = Vec::new();

    for ((cx, cz), mut instances) in buckets {
        // Deterministic order so the same content always hashes the
        // same (delta-publish correctness).
        instances.sort_by(|a, b| a.0.cmp(&b.0));

        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(ECHK_MAGIC);
        buf.extend_from_slice(&ECHK_VERSION.to_le_bytes());
        buf.extend_from_slice(&(instances.len() as u32).to_le_bytes());
        for (path, data) in &instances {
            buf.extend_from_slice(&(path.len() as u32).to_le_bytes());
            buf.extend_from_slice(path.as_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
        }

        let hash = blake3::hash(&buf).to_hex().to_string();
        let file = format!("{cx}_{cz}.echk");

        if prior.get(&(cx, cz)) == Some(&hash) {
            summary.chunks_skipped += 1;
            summary.entities_baked += instances.len();
            manifest.push(ManifestEntry {
                cx,
                cz,
                file,
                size: buf.len() as u64,
                blake3: hash,
                count: instances.len() as u32,
            });
            continue;
        }

        // Atomic write: temp + rename so a crash mid-bake can't leave
        // a torn chunk the server would serve.
        let final_path = chunks_dir.join(&file);
        let tmp_path = chunks_dir.join(format!("{file}.tmp"));
        {
            let mut f = std::fs::File::create(&tmp_path)?;
            f.write_all(&buf)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp_path, &final_path)?;

        summary.chunks_written += 1;
        summary.entities_baked += instances.len();
        summary.bytes_written += buf.len() as u64;
        manifest.push(ManifestEntry {
            cx,
            cz,
            file,
            size: buf.len() as u64,
            blake3: hash,
            count: instances.len() as u32,
        });
    }

    write_manifest(&manifest_path, &manifest, chunk_size)?;

    tracing::info!(
        target: "eustress_worlddb::bake",
        written = summary.chunks_written,
        skipped = summary.chunks_skipped,
        entities = summary.entities_baked,
        bytes = summary.bytes_written,
        "bake complete"
    );
    Ok(summary)
}

/// Parse `[transform].position` out of an `_instance.toml` and floor
/// it to a chunk coord. Missing/short transform → origin chunk.
fn chunk_of_instance(bytes: &[u8], chunk_size: f64) -> (i32, i32) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return (0, 0);
    };
    let Ok(v) = text.parse::<toml::Value>() else {
        return (0, 0);
    };
    let pos = v
        .get("transform")
        .and_then(|t| t.get("position"))
        .and_then(|p| p.as_array());
    let Some(arr) = pos else { return (0, 0) };
    let get = |i: usize| arr.get(i).and_then(|n| n.as_float().or_else(|| n.as_integer().map(|x| x as f64))).unwrap_or(0.0);
    let x = get(0);
    let z = get(2);
    (
        (x / chunk_size).floor() as i32,
        (z / chunk_size).floor() as i32,
    )
}

fn read_prior_hashes(manifest_path: &Path) -> BTreeMap<(i32, i32), String> {
    let mut out = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(manifest_path) else {
        return out;
    };
    let Ok(v) = text.parse::<toml::Value>() else {
        return out;
    };
    if let Some(chunks) = v.get("chunk").and_then(|c| c.as_array()) {
        for c in chunks {
            let cx = c.get("cx").and_then(|n| n.as_integer()).unwrap_or(0) as i32;
            let cz = c.get("cz").and_then(|n| n.as_integer()).unwrap_or(0) as i32;
            if let Some(h) = c.get("blake3").and_then(|n| n.as_str()) {
                out.insert((cx, cz), h.to_string());
            }
        }
    }
    out
}

fn write_manifest(
    path: &Path,
    entries: &[ManifestEntry],
    chunk_size: f64,
) -> Result<()> {
    // Hand-written TOML, coord-sorted, so the manifest is itself
    // byte-stable for diffing / its own content hash.
    let mut s = String::new();
    s.push_str("# Auto-generated by eustress-worlddb bake. Do not edit.\n");
    s.push_str(&format!("chunk_size = {chunk_size}\n"));
    s.push_str(&format!("encoder_version = {ECHK_VERSION}\n\n"));
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| (a.cx, a.cz).cmp(&(b.cx, b.cz)));
    for e in sorted {
        s.push_str("[[chunk]]\n");
        s.push_str(&format!("cx = {}\n", e.cx));
        s.push_str(&format!("cz = {}\n", e.cz));
        s.push_str(&format!("file = \"{}\"\n", e.file));
        s.push_str(&format!("size = {}\n", e.size));
        s.push_str(&format!("count = {}\n", e.count));
        s.push_str(&format!("blake3 = \"{}\"\n\n", e.blake3));
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, s.as_bytes())?;
    std::fs::rename(&tmp, path).map_err(Error::Io)?;
    Ok(())
}

/// Decode a `.echk` v0 chunk back into `(rel_path, bytes)` pairs —
/// the inverse of the bake. The Client / Studio chunk loader uses
/// this; round-trips byte-exact with [`bake_to_echk`].
pub fn decode_echk(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    if bytes.len() < 12 || &bytes[..4] != ECHK_MAGIC {
        return Err(Error::Other("not an .echk file (bad magic)".into()));
    }
    let mut ver = [0u8; 4];
    ver.copy_from_slice(&bytes[4..8]);
    if u32::from_le_bytes(ver) != ECHK_VERSION {
        return Err(Error::Other(format!(
            "unsupported .echk version {}",
            u32::from_le_bytes(ver)
        )));
    }
    let mut cnt = [0u8; 4];
    cnt.copy_from_slice(&bytes[8..12]);
    let count = u32::from_le_bytes(cnt) as usize;
    let mut out = Vec::with_capacity(count);
    let mut off = 12;
    for _ in 0..count {
        if off + 4 > bytes.len() {
            return Err(Error::Other("truncated .echk (path_len)".into()));
        }
        let pl = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if off + pl > bytes.len() {
            return Err(Error::Other("truncated .echk (path)".into()));
        }
        let path = String::from_utf8_lossy(&bytes[off..off + pl]).to_string();
        off += pl;
        if off + 4 > bytes.len() {
            return Err(Error::Other("truncated .echk (data_len)".into()));
        }
        let dl = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if off + dl > bytes.len() {
            return Err(Error::Other("truncated .echk (data)".into()));
        }
        out.push((path, bytes[off..off + dl].to_vec()));
        off += dl;
    }
    Ok(out)
}

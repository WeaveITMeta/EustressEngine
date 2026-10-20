//! # `.echk` — the Eustress world chunk container
//!
//! A world leaves the machine it was authored on as content-addressed
//! chunks. The same chunks serve two readers:
//!
//! - **Publishing.** Studio bakes each Space into chunks, uploads only the
//!   chunks R2 does not already hold, and commits a [`WorldManifest`] that
//!   names them. The Player downloads the manifest, then the chunks.
//! - **Hosting.** A Studio dev server bakes the same chunks and serves them
//!   to each joining Player over the game connection.
//!
//! This crate is the container and nothing else: no storage engine and no
//! IO, and reading links no C. That is deliberate. `eustress-worlddb` owns the
//! bake (it walks the Fjall `tree` partition and buckets entities by world
//! position), and it links Fjall. A Player only has to *read* chunks, so it
//! links this crate instead, and so can a browser build.
//!
//! ## Container
//!
//! Version 1 stores the records raw:
//!
//! ```text
//! magic   "ECHK"            4 bytes
//! version u32 little-endian (= 1)
//! count   u32 little-endian (records in this chunk)
//! count times:
//!   path_len u32 LE, path bytes (Space-relative, forward slashes, UTF-8)
//!   data_len u32 LE, data bytes (the file, verbatim)
//! ```
//!
//! Version 2 stores the same record stream as one zstd frame:
//!
//! ```text
//! magic    "ECHK"           4 bytes
//! version  u32 LE (= 2)
//! count    u32 LE
//! codec    u8  (1 = zstd)
//! flags    u8  (0)
//! reserved u16 (0)
//! raw_len  u64 LE           length of the record stream
//! one zstd frame of the record stream, exactly as version 1 lays it out
//! ```
//!
//! [`encode_chunk`] writes version 2 (feature `encode`); [`decode_chunk`]
//! reads both. A chunk's content hash is always taken over its stored bytes.
//! Compression runs single-threaded at a fixed level, so the same records
//! always produce the same bytes and an unchanged chunk is never uploaded
//! twice. `flags` and `reserved` leave room for a trained dictionary without a
//! version 3.
//!
//! Version 1 is byte-identical to what `eustress_worlddb::bake` writes. A
//! test below runs worlddb's own decoder over [`encode_chunk_v1`]'s output.
//!
//! ## Trust
//!
//! Chunks come from a host or from R2, and either can be hostile. Decoding is
//! bounds-checked, a version 2 chunk may inflate to at most [`MAX_RAW_BYTES`]
//! and must inflate to exactly the length it declares, every record path must
//! pass [`is_safe_record_path`] before anything writes it to disk, a chunk's
//! bytes must hash to the name the manifest gave it ([`content_hash`]), and
//! [`WorldManifest::validate`] caps the sizes a reader will accept.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Container magic.
pub const MAGIC: &[u8; 4] = b"ECHK";
/// Newest container version: what [`encode_chunk`] writes, and the highest a
/// [`WorldManifest`] may declare.
pub const VERSION: u32 = 2;
/// Oldest container version readers still open.
pub const MIN_VERSION: u32 = 1;
/// Version 2's codec byte for zstd.
pub const CODEC_ZSTD: u8 = 1;
/// The zstd level every version 2 chunk is written at. Changing it changes
/// every chunk's bytes, and so costs one full re-upload of every world.
pub const ZSTD_LEVEL: i32 = 9;
/// Largest record stream a version 2 chunk may inflate to.
pub const MAX_RAW_BYTES: u64 = 256 * 1024 * 1024;
/// The `format` a [`WorldManifest`] carries.
pub const FORMAT: &str = "echk";

const V1_HEADER: usize = 12;
const V2_HEADER: usize = 24;

/// Longest record path accepted on decode.
pub const MAX_PATH_LEN: usize = 1024;
/// Most chunks one world may name.
pub const MAX_CHUNKS: usize = 65_536;
/// Largest single chunk a reader accepts.
pub const MAX_CHUNK_BYTES: u64 = 512 * 1024 * 1024;
/// Largest whole world a reader accepts. A hostile host could otherwise fill
/// the joining player's disk.
pub const MAX_WORLD_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// One file in a chunk: a Space-relative path and the file's bytes.
pub type Record = (String, Vec<u8>);

/// Why a chunk or manifest was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EchkError {
    /// The bytes do not start with [`MAGIC`].
    BadMagic,
    /// A container version this crate does not read.
    UnsupportedVersion(u32),
    /// The bytes end inside the named field.
    Truncated(&'static str),
    /// A record or Space name that could escape the destination directory.
    UnsafePath(String),
    /// A manifest that is malformed or exceeds a limit.
    Manifest(String),
    /// A version 2 chunk whose compressed block is unknown, oversized or
    /// corrupt.
    Compression(String),
}

impl fmt::Display for EchkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EchkError::BadMagic => write!(f, "not an .echk chunk (bad magic)"),
            EchkError::UnsupportedVersion(v) => write!(f, "unsupported .echk version {v}"),
            EchkError::Truncated(field) => write!(f, "truncated .echk ({field})"),
            EchkError::UnsafePath(p) => write!(f, "unsafe path in .echk: {p:?}"),
            EchkError::Manifest(m) => write!(f, "bad world manifest: {m}"),
            EchkError::Compression(m) => write!(f, "bad compressed .echk: {m}"),
        }
    }
}

impl std::error::Error for EchkError {}

// ─────────────────────────────────────────────────────────────────────────────
// The container
// ─────────────────────────────────────────────────────────────────────────────

/// The record stream both versions share: each record's path and data, each
/// behind its u32 length.
fn record_stream(records: &[Record]) -> Vec<u8> {
    let payload: usize = records.iter().map(|(p, d)| 8 + p.len() + d.len()).sum();
    let mut buf = Vec::with_capacity(payload);
    for (path, data) in records {
        buf.extend_from_slice(&(path.len() as u32).to_le_bytes());
        buf.extend_from_slice(path.as_bytes());
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(data);
    }
    buf
}

/// Encode records into one version 1 (uncompressed) chunk, the layout
/// `eustress_worlddb::bake` writes.
///
/// The caller orders the records. `bake` sorts by path so that unchanged
/// content always produces the same bytes, and therefore the same hash, which
/// is what lets a publish skip chunks R2 already holds.
pub fn encode_chunk_v1(records: &[Record]) -> Vec<u8> {
    let stream = record_stream(records);
    let mut buf = Vec::with_capacity(V1_HEADER + stream.len());
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&(records.len() as u32).to_le_bytes());
    buf.extend_from_slice(&stream);
    buf
}

/// Encode records into one version 2 chunk: the record stream as one zstd
/// frame at [`ZSTD_LEVEL`], compressed single-threaded so the same records
/// always give the same bytes.
///
/// A record stream too large to open again ([`MAX_RAW_BYTES`]), or a
/// compressor failure, falls back to version 1, which every reader opens.
#[cfg(feature = "encode")]
pub fn encode_chunk(records: &[Record]) -> Vec<u8> {
    let stream = record_stream(records);
    if stream.len() as u64 > MAX_RAW_BYTES {
        return encode_chunk_v1(records);
    }
    let Ok(frame) = zstd::bulk::compress(&stream, ZSTD_LEVEL) else {
        return encode_chunk_v1(records);
    };
    v2_chunk(records.len(), stream.len(), &frame)
}

/// Assemble a version 2 chunk around an already compressed frame.
#[cfg(any(feature = "encode", test))]
fn v2_chunk(count: usize, raw_len: usize, frame: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(V2_HEADER + frame.len());
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&2u32.to_le_bytes());
    buf.extend_from_slice(&(count as u32).to_le_bytes());
    buf.push(CODEC_ZSTD);
    buf.push(0); // flags
    buf.extend_from_slice(&0u16.to_le_bytes()); // reserved
    buf.extend_from_slice(&(raw_len as u64).to_le_bytes());
    buf.extend_from_slice(frame);
    buf
}

/// Decode one chunk, of either version, back into its records.
///
/// Refuses a record count the bytes cannot hold before allocating for it,
/// and a compressed block that inflates past [`MAX_RAW_BYTES`] or to any
/// length other than the one it declares, so a forged header cannot make a
/// reader reserve gigabytes.
pub fn decode_chunk(bytes: &[u8]) -> Result<Vec<Record>, EchkError> {
    if bytes.len() < V1_HEADER || &bytes[..4] != MAGIC {
        return Err(EchkError::BadMagic);
    }
    let count = read_u32(bytes, 8) as usize;
    match read_u32(bytes, 4) {
        1 => parse_records(&bytes[V1_HEADER..], count),
        2 => parse_records(&inflate_v2(bytes)?, count),
        version => Err(EchkError::UnsupportedVersion(version)),
    }
}

/// The record stream of a version 2 chunk.
fn inflate_v2(bytes: &[u8]) -> Result<Vec<u8>, EchkError> {
    use std::io::Read;

    if bytes.len() < V2_HEADER {
        return Err(EchkError::Truncated("version 2 header"));
    }
    let (codec, flags, reserved) = (bytes[12], bytes[13], u16::from_le_bytes([bytes[14], bytes[15]]));
    if codec != CODEC_ZSTD {
        return Err(EchkError::Compression(format!("unknown codec {codec}")));
    }
    if flags != 0 || reserved != 0 {
        return Err(EchkError::Compression(format!("unknown flags {flags:#04x}/{reserved:#06x}")));
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[16..24]);
    let raw_len = u64::from_le_bytes(raw);
    if raw_len > MAX_RAW_BYTES {
        return Err(EchkError::Compression(format!("declares {raw_len} bytes, over the {MAX_RAW_BYTES} limit")));
    }
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(&bytes[V2_HEADER..])
        .map_err(|e| EchkError::Compression(format!("{e:?}")))?;
    // Grown as the frame inflates, never reserved from the header, and cut
    // one byte past the declared length so an overlong frame is caught.
    let mut out = Vec::new();
    decoder
        .by_ref()
        .take(raw_len + 1)
        .read_to_end(&mut out)
        .map_err(|e| EchkError::Compression(e.to_string()))?;
    if out.len() as u64 != raw_len {
        return Err(EchkError::Compression(format!("inflates to {} bytes, declares {raw_len}", out.len())));
    }
    Ok(out)
}

/// Split a record stream into `count` records.
fn parse_records(bytes: &[u8], count: usize) -> Result<Vec<Record>, EchkError> {
    // Every record needs at least 8 bytes of length prefixes.
    if count > bytes.len() / 8 {
        return Err(EchkError::Truncated("record count"));
    }

    let mut out = Vec::with_capacity(count);
    let mut off = 0;
    for _ in 0..count {
        if off + 4 > bytes.len() {
            return Err(EchkError::Truncated("path_len"));
        }
        let path_len = read_u32(bytes, off) as usize;
        off += 4;
        if path_len > MAX_PATH_LEN || off + path_len > bytes.len() {
            return Err(EchkError::Truncated("path"));
        }
        let path = String::from_utf8_lossy(&bytes[off..off + path_len]).into_owned();
        off += path_len;
        if off + 4 > bytes.len() {
            return Err(EchkError::Truncated("data_len"));
        }
        let data_len = read_u32(bytes, off) as usize;
        off += 4;
        if off + data_len > bytes.len() {
            return Err(EchkError::Truncated("data"));
        }
        out.push((path, bytes[off..off + data_len].to_vec()));
        off += data_len;
    }
    Ok(out)
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&bytes[at..at + 4]);
    u32::from_le_bytes(b)
}

/// The content address of a chunk: blake3, lowercase hex. The same function
/// `bake` uses to fill a manifest's `blake3` field.
pub fn content_hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// True for a well-formed content address: 64 lowercase hex characters.
pub fn is_content_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The on-disk name `bake` gives the chunk at `(cx, cz)`.
pub fn chunk_file_name(cx: i32, cz: i32) -> String {
    format!("{cx}_{cz}.echk")
}

/// Would writing a record at this path stay inside the destination?
///
/// Checked as a string so the answer is the same on every platform. Refuses
/// absolute paths, `..` and `.` segments, empty segments, backslashes (a
/// Windows separator a forward-slash check would miss), `:` (drive letters
/// and NTFS alternate streams), and control characters.
pub fn is_safe_record_path(path: &str) -> bool {
    if path.is_empty() || path.len() > MAX_PATH_LEN || path.starts_with('/') {
        return false;
    }
    if path.contains('\\') || path.contains(':') || path.chars().any(|c| c.is_control()) {
        return false;
    }
    path.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

// ─────────────────────────────────────────────────────────────────────────────
// Bake manifest (one Space, written by `eustress_worlddb::bake`)
// ─────────────────────────────────────────────────────────────────────────────

/// One chunk as a manifest lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkEntry {
    pub cx: i32,
    pub cz: i32,
    /// The local file name the bake wrote. Informational on the wire; readers
    /// fetch by `blake3`.
    pub file: String,
    pub size: u64,
    /// Records in the chunk.
    pub count: u32,
    /// Content address, see [`content_hash`].
    pub blake3: String,
}

/// The `manifest.toml` a bake writes beside its `chunks/` directory.
#[derive(Debug, Clone, PartialEq)]
pub struct BakeManifest {
    pub chunk_size: f64,
    pub encoder_version: u32,
    pub chunks: Vec<ChunkEntry>,
}

/// Parse a bake's `manifest.toml`.
///
/// Reads through `toml::Value` rather than a derived struct because the bake
/// writes `chunk_size` with Rust's float formatting, which prints `256.0` as
/// `256`, an integer to TOML.
pub fn parse_bake_manifest(text: &str) -> Result<BakeManifest, EchkError> {
    let v: toml::Value = text
        .parse()
        .map_err(|e: toml::de::Error| EchkError::Manifest(e.to_string()))?;
    let num = |key: &str| -> Option<f64> {
        let x = v.get(key)?;
        x.as_float().or_else(|| x.as_integer().map(|i| i as f64))
    };
    let chunk_size = num("chunk_size").ok_or_else(|| EchkError::Manifest("chunk_size missing".into()))?;
    let encoder_version = v
        .get("encoder_version")
        .and_then(|x| x.as_integer())
        .ok_or_else(|| EchkError::Manifest("encoder_version missing".into()))? as u32;

    let mut chunks = Vec::new();
    if let Some(rows) = v.get("chunk").and_then(|c| c.as_array()) {
        for row in rows {
            let int = |key: &str| row.get(key).and_then(|x| x.as_integer());
            let string = |key: &str| row.get(key).and_then(|x| x.as_str()).map(str::to_owned);
            chunks.push(ChunkEntry {
                cx: int("cx").ok_or_else(|| EchkError::Manifest("chunk.cx missing".into()))? as i32,
                cz: int("cz").ok_or_else(|| EchkError::Manifest("chunk.cz missing".into()))? as i32,
                file: string("file").unwrap_or_default(),
                size: int("size").unwrap_or(0).max(0) as u64,
                count: int("count").unwrap_or(0).max(0) as u32,
                blake3: string("blake3").ok_or_else(|| EchkError::Manifest("chunk.blake3 missing".into()))?,
            });
        }
    }
    Ok(BakeManifest { chunk_size, encoder_version, chunks })
}

// ─────────────────────────────────────────────────────────────────────────────
// World manifest (one publish, or one host session)
// ─────────────────────────────────────────────────────────────────────────────

/// Everything a reader needs to reassemble a world: which chunks make each
/// Space, and which chunks carry the Universe's shared `assets/`.
///
/// Serialized as JSON on the wire and in R2. The Worker reads the same shape
/// (`infrastructure/cloudflare/api/src/index.js`, the `/world/` routes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldManifest {
    /// Always [`FORMAT`].
    pub format: String,
    /// The newest container version among the world's chunks. Readers refuse
    /// a version outside [`MIN_VERSION`]..=[`VERSION`] rather than half-load a
    /// world.
    pub encoder_version: u32,
    /// Chunk edge in metres for the Spaces' spatial chunks.
    pub chunk_size: f64,
    /// Engine version that baked the world.
    #[serde(default)]
    pub engine_version: String,
    /// Universe folder name.
    #[serde(default)]
    pub universe: String,
    /// The Space a player opens first. Empty means the first listed Space.
    #[serde(default)]
    pub start_space: String,
    /// One entry per Space. Record paths inside a Space's chunks are relative
    /// to that Space's folder.
    pub spaces: Vec<SpaceManifest>,
    /// Chunks carrying the Universe's `assets/` tree. Record paths are
    /// relative to the Universe folder and all start with `assets/`.
    #[serde(default)]
    pub assets: Vec<ChunkEntry>,
}

/// The chunks of one Space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceManifest {
    /// The Space's folder name. A single path segment.
    pub name: String,
    pub chunks: Vec<ChunkEntry>,
}

impl WorldManifest {
    pub fn new(universe: impl Into<String>, engine_version: impl Into<String>, chunk_size: f64) -> Self {
        Self {
            format: FORMAT.to_string(),
            encoder_version: VERSION,
            chunk_size,
            engine_version: engine_version.into(),
            universe: universe.into(),
            start_space: String::new(),
            spaces: Vec::new(),
            assets: Vec::new(),
        }
    }

    /// The Space a player should open: `start_space` when set, else the first.
    pub fn opening_space(&self) -> Option<&SpaceManifest> {
        if self.start_space.is_empty() {
            self.spaces.first()
        } else {
            self.spaces.iter().find(|s| s.name == self.start_space)
        }
    }

    /// Put every list in a fixed order, so the same world always serializes to
    /// the same bytes and therefore the same publish hash.
    pub fn canonicalize(&mut self) {
        self.spaces.sort_by(|a, b| a.name.cmp(&b.name));
        for s in &mut self.spaces {
            s.chunks.sort_by(|a, b| (a.cx, a.cz, &a.blake3).cmp(&(b.cx, b.cz, &b.blake3)));
        }
        self.assets.sort_by(|a, b| (a.cx, a.cz, &a.blake3).cmp(&(b.cx, b.cz, &b.blake3)));
    }

    /// Every chunk the world names, each listed once.
    pub fn hashes(&self) -> BTreeSet<String> {
        self.all_chunks().map(|c| c.blake3.clone()).collect()
    }

    /// Every chunk entry, Spaces first, then assets.
    pub fn all_chunks(&self) -> impl Iterator<Item = &ChunkEntry> {
        self.spaces.iter().flat_map(|s| s.chunks.iter()).chain(self.assets.iter())
    }

    /// Bytes a reader will download, counting each distinct chunk once.
    pub fn download_bytes(&self) -> u64 {
        let mut seen = BTreeSet::new();
        self.all_chunks()
            .filter(|c| seen.insert(c.blake3.as_str()))
            .map(|c| c.size)
            .sum()
    }

    /// Refuse a manifest a reader should not act on. Run on every manifest
    /// that crossed a network.
    pub fn validate(&self) -> Result<(), EchkError> {
        let bad = |m: String| Err(EchkError::Manifest(m));
        if self.format != FORMAT {
            return bad(format!("format {:?}, expected {FORMAT:?}", self.format));
        }
        if !(MIN_VERSION..=VERSION).contains(&self.encoder_version) {
            return Err(EchkError::UnsupportedVersion(self.encoder_version));
        }
        if !(self.chunk_size.is_finite() && self.chunk_size > 0.0) {
            return bad(format!("chunk_size {}", self.chunk_size));
        }
        let mut names = BTreeSet::new();
        for s in &self.spaces {
            if !is_safe_record_path(&s.name) || s.name.contains('/') || s.name.starts_with('.') {
                return Err(EchkError::UnsafePath(s.name.clone()));
            }
            if !names.insert(s.name.as_str()) {
                return bad(format!("Space {:?} listed twice", s.name));
            }
        }
        if self.spaces.is_empty() {
            return bad("no Spaces".into());
        }
        if !self.start_space.is_empty() && !names.contains(self.start_space.as_str()) {
            return bad(format!("start_space {:?} is not a listed Space", self.start_space));
        }
        let total = self.all_chunks().count();
        if total > MAX_CHUNKS {
            return bad(format!("{total} chunks exceeds {MAX_CHUNKS}"));
        }
        for c in self.all_chunks() {
            if !is_content_hash(&c.blake3) {
                return bad(format!("chunk hash {:?}", c.blake3));
            }
            if c.size == 0 || c.size > MAX_CHUNK_BYTES {
                return bad(format!("chunk {} size {}", c.blake3, c.size));
            }
        }
        let bytes = self.download_bytes();
        if bytes > MAX_WORLD_BYTES {
            return bad(format!("world is {bytes} bytes, over the {MAX_WORLD_BYTES} limit"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Record> {
        vec![
            ("Workspace/Floor/_instance.toml".into(), b"[metadata]\nclass_name = \"Part\"\n".to_vec()),
            ("Workspace/Floor/mesh.glb".into(), vec![0, 1, 2, 3, 255]),
            ("ServerScriptService/Main.luau".into(), Vec::new()),
        ]
    }

    /// A version 2 chunk built with the zstd dev-dependency, so the decoder
    /// is tested whether or not the `encode` feature is on.
    fn v2(records: &[Record]) -> Vec<u8> {
        let stream = record_stream(records);
        v2_chunk(records.len(), stream.len(), &zstd::bulk::compress(&stream, ZSTD_LEVEL).unwrap())
    }

    #[test]
    fn round_trips() {
        let recs = sample();
        assert_eq!(decode_chunk(&encode_chunk_v1(&recs)).unwrap(), recs);
        assert_eq!(decode_chunk(&v2(&recs)).unwrap(), recs);
    }

    #[cfg(feature = "encode")]
    #[test]
    fn encodes_version_2_the_same_way_every_time() {
        let recs = sample();
        let a = encode_chunk(&recs);
        assert_eq!(read_u32(&a, 4), 2);
        assert_eq!(a, encode_chunk(&recs), "same records, different bytes");
        assert_eq!(a, v2(&recs));
        assert_eq!(decode_chunk(&a).unwrap(), recs);
    }

    #[test]
    fn version_2_compresses_what_a_space_is_made_of() {
        let toml = "[metadata]\nclass_name = \"Part\"\narchivable = true\n\n[transform]\nposition = [0.0, 0.0, 0.0]\n";
        let recs: Vec<Record> = (0..500).map(|i| (format!("Workspace/Part{i}/_instance.toml"), toml.as_bytes().to_vec())).collect();
        let (raw, packed) = (encode_chunk_v1(&recs), v2(&recs));
        assert!(packed.len() * 5 < raw.len(), "{} bytes compressed from {}", packed.len(), raw.len());
        assert_eq!(decode_chunk(&packed).unwrap(), recs);
    }

    #[test]
    fn worlddb_decoder_reads_this_encoder() {
        // Two decoders exist while the bake keeps its own copy of the codec;
        // they must agree on every version 1 byte this crate writes.
        let recs = sample();
        let bytes = encode_chunk_v1(&recs);
        assert_eq!(eustress_worlddb::bake::decode_echk(&bytes).unwrap(), recs);
        assert_eq!(decode_chunk(&bytes).unwrap(), recs);
    }

    #[test]
    fn refuses_forged_counts_and_truncation() {
        let mut bytes = encode_chunk_v1(&sample());
        // Claim a billion records in a few hundred bytes.
        bytes[8..12].copy_from_slice(&1_000_000_000u32.to_le_bytes());
        assert_eq!(decode_chunk(&bytes), Err(EchkError::Truncated("record count")));

        let bytes = encode_chunk_v1(&sample());
        assert!(decode_chunk(&bytes[..bytes.len() - 1]).is_err());
        assert_eq!(decode_chunk(b"NOPE\x01\0\0\0\0\0\0\0"), Err(EchkError::BadMagic));
        assert_eq!(decode_chunk(b"ECHK\x03\0\0\0\0\0\0\0"), Err(EchkError::UnsupportedVersion(3)));
    }

    #[test]
    fn refuses_a_hostile_version_2_header() {
        let good = v2(&sample());
        let patched = |at: usize, with: &[u8]| {
            let mut b = good.clone();
            b[at..at + with.len()].copy_from_slice(with);
            b
        };
        // An unknown codec, and flags this reader does not know.
        assert!(matches!(decode_chunk(&patched(12, &[9])), Err(EchkError::Compression(_))));
        assert!(matches!(decode_chunk(&patched(13, &[1])), Err(EchkError::Compression(_))));
        // A declared length past the cap is refused before inflating anything.
        let huge = (MAX_RAW_BYTES + 1).to_le_bytes();
        assert!(matches!(decode_chunk(&patched(16, &huge)), Err(EchkError::Compression(_))));
        // A declared length that disagrees with the frame, either way.
        let raw_len = u64::from_le_bytes(good[16..24].try_into().unwrap());
        assert!(matches!(decode_chunk(&patched(16, &(raw_len - 1).to_le_bytes())), Err(EchkError::Compression(_))));
        assert!(matches!(decode_chunk(&patched(16, &(raw_len + 1).to_le_bytes())), Err(EchkError::Compression(_))));
        // A frame cut short.
        assert!(decode_chunk(&good[..good.len() - 4]).is_err());
        assert!(matches!(decode_chunk(&good[..20]), Err(EchkError::Truncated(_))));
    }

    #[test]
    fn record_paths_cannot_escape() {
        for bad in [
            "", "/etc/passwd", "../x", "a/../b", "a/./b", "a//b", "C:/x", "a\\b",
            "Workspace/x:stream", "a/\u{0}b",
        ] {
            assert!(!is_safe_record_path(bad), "accepted {bad:?}");
        }
        for ok in ["Workspace/Floor/_instance.toml", "assets/meshes/a.glb", ".eustress-free/x"] {
            assert!(is_safe_record_path(ok), "refused {ok:?}");
        }
    }

    #[test]
    fn parses_the_manifest_bake_writes() {
        // Verbatim shape of `bake::write_manifest`, including the integer
        // `chunk_size` Rust's float formatting produces.
        let text = "# Auto-generated by eustress-worlddb bake. Do not edit.\n\
            chunk_size = 256\n\
            encoder_version = 1\n\n\
            [[chunk]]\ncx = -1\ncz = 0\nfile = \"-1_0.echk\"\nsize = 42\ncount = 3\n\
            blake3 = \"aa\"\n";
        let m = parse_bake_manifest(text).unwrap();
        assert_eq!(m.chunk_size, 256.0);
        assert_eq!(m.encoder_version, 1);
        assert_eq!(m.chunks.len(), 1);
        assert_eq!((m.chunks[0].cx, m.chunks[0].cz, m.chunks[0].count), (-1, 0, 3));
    }

    #[test]
    fn manifest_is_canonical_and_validated() {
        let chunk = |cx, h: &str| ChunkEntry {
            cx, cz: 0, file: chunk_file_name(cx, 0), size: 10, count: 1, blake3: h.repeat(64),
        };
        let mut a = WorldManifest::new("U", "0.3.6", 256.0);
        a.spaces.push(SpaceManifest { name: "B".into(), chunks: vec![chunk(2, "b"), chunk(1, "a")] });
        a.spaces.push(SpaceManifest { name: "A".into(), chunks: vec![chunk(0, "c")] });
        let mut b = a.clone();
        b.spaces.reverse();
        a.canonicalize();
        b.canonicalize();
        assert_eq!(serde_json::to_vec(&a).unwrap(), serde_json::to_vec(&b).unwrap());
        assert!(a.validate().is_ok());

        let mut evil = a.clone();
        evil.spaces[0].name = "../escape".into();
        assert!(evil.validate().is_err());
        let mut older = a.clone();
        older.encoder_version = 1;
        assert!(older.validate().is_ok(), "a version 1 world must still open");
        let mut future = a;
        future.encoder_version = VERSION + 1;
        assert_eq!(future.validate(), Err(EchkError::UnsupportedVersion(VERSION + 1)));
    }
}

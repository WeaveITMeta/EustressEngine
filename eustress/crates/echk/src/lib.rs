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
//!
//! ## Keys
//!
//! Everyone who downloads a package can read every byte of it. Before a
//! package is written, [`screen_package`] takes SoulService's own key out of
//! its settings, reads every byte of every file and every file's name for
//! what looks like a provider's key, and judges what it found for the
//! package's [`Destination`]. Publishing and hosting share it, and like the
//! rest of this crate it does no IO.

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

/// Whether a path segment names a Windows device: `CON`, `PRN`, `AUX`, `NUL`,
/// `COM0` to `COM9`, `LPT0` to `LPT9` (with their superscript digits 1 to 3),
/// `CONIN$` and `CONOUT$`, in any case, with any extension, and with the
/// trailing dots and spaces Windows ignores. The same rule as the device check
/// in `eustress_common::space_path`; the vectors in the test below belong in
/// that crate's tests too, so the two cannot drift apart unnoticed.
fn is_reserved_device_name(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or(segment).trim_end_matches([' ', '.']);
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
        return true;
    }
    let mut chars = stem.chars();
    let head: String = chars.by_ref().take(3).collect::<String>().to_ascii_uppercase();
    let tail: Vec<char> = chars.collect();
    (head == "COM" || head == "LPT")
        && tail.len() == 1
        && (tail[0].is_ascii_digit() || matches!(tail[0], '\u{b9}' | '\u{b2}' | '\u{b3}'))
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
///
/// A segment that ends in a dot or a space is refused too: Windows strips
/// those, so `a.toml.` would write over `a.toml`. A segment that names a
/// Windows device (`CON`, `NUL`, `COM1`, and the rest) is refused: a write to
/// one opens the device, not a file.
pub fn is_safe_record_path(path: &str) -> bool {
    if path.is_empty() || path.len() > MAX_PATH_LEN || path.starts_with('/') {
        return false;
    }
    if path.contains('\\') || path.contains(':') || path.chars().any(|c| c.is_control()) {
        return false;
    }
    path.split('/').all(|seg| {
        !seg.is_empty()
            && seg != "."
            && seg != ".."
            && !seg.ends_with(['.', ' '])
            && !is_reserved_device_name(seg)
    })
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

// ─────────────────────────────────────────────────────────────────────────────
// Provider API keys
// ─────────────────────────────────────────────────────────────────────────────

/// Where a package goes, which decides what a key found in it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// Uploaded for anyone to download: a key refuses it.
    Publish,
    /// Served by a host that listens beyond this machine
    /// (`EUSTRESS_HOST_LAN`): a key refuses it.
    LanHost,
    /// Served by a host that listens on loopback only, solo Play included: a
    /// key is warned about, and the package goes ahead.
    LoopbackHost,
}

impl Destination {
    /// Whether the package leaves this machine.
    pub fn leaves_machine(self) -> bool {
        matches!(self, Destination::Publish | Destination::LanHost)
    }
}

/// Said when SoulService's own AI key was taken out of a package: as the
/// notice of a package that leaves this machine, and as a warning line on a
/// loopback host.
pub const SOUL_KEY_NOTICE: &str =
    "An AI key in SoulService was left out of this package. If this Space was published, shared or committed before, rotate that key.";

/// Where SoulService keeps its settings in a Space made from the template.
pub const SOUL_SERVICE_RECORD: &str = "SoulService/_service.toml";
/// The class SoulService's settings name in `[service] class_name`.
const SOUL_SERVICE_CLASS: &str = "SoulService";
/// The file every service keeps its settings in.
const SERVICE_FILE: &str = "_service.toml";
/// SoulService's credential fields: its own API key, and the switch between
/// that key and the machine's.
const SOUL_CREDENTIAL_KEYS: [&str; 2] = ["api_key", "use_global_key"];
/// How a message names the files of one package screened on its own.
const PACKAGE_SCOPE: &str = "this package";
/// How a message names a Space whose own name holds what looks like a key.
const UNNAMED_SPACE: &str = "a Space";

/// Shortest run after `sk-` that reads as a key.
const SK_MIN_RUN: usize = 20;
/// Shortest run after `xai-` that reads as a key.
const XAI_MIN_RUN: usize = 20;
/// The run after `AKIA` or `ASIA`, exactly.
const AWS_KEY_ID_LEN: usize = 16;
/// Shortest run after a Stripe prefix.
const STRIPE_MIN_RUN: usize = 20;
/// Shortest run after `AIza`.
const GOOGLE_MIN_RUN: usize = 35;
/// Shortest run after `ghp_` and its kin.
const GITHUB_TOKEN_MIN_RUN: usize = 36;
/// Shortest run after `github_pat_`.
const GITHUB_PAT_MIN_RUN: usize = 22;
/// Shortest run after a Slack prefix.
const SLACK_MIN_RUN: usize = 10;
/// The PEM private key header's first piece, before its optional words.
const PEM_BEGIN: &[u8] = b"-----BEGIN ";
/// The words that end every private key header, before its last piece.
const PEM_PRIVATE_KEY: &[u8] = b"PRIVATE KEY";
/// A private key header's last piece: PEM's, or a PGP key block's.
const PEM_ENDS: [&[u8]; 2] = [b"-----", b" BLOCK-----"];
/// How a private key is named to a person.
const PRIVATE_KEY: &str = "a private key";
/// Most files one refusal names; it counts the rest.
const REFUSAL_NAMES_AT_MOST: usize = 8;
/// Most key lines one package's warnings hold; one more line counts the rest.
const WARNING_LINES_AT_MOST: usize = 8;

/// Where in a file a key starts: its line in a UTF-8 file, its byte offset in
/// any other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    /// The 1-based line, counted by `\n`.
    Line(usize),
    /// The 0-based offset of the key's first byte in the file.
    Byte(usize),
}

/// Where a key was found, as a person may be told it. A file or folder name
/// that holds what looks like a key is never shown: only its scope is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// In a file whose path holds no key: the path, and where in the file.
    File { path: String, spot: Spot },
    /// In a file whose own name, or its Space's, holds what looks like a key.
    UnnamedFile { scope: String },
    /// In a file or folder name itself.
    Name { scope: String },
}

/// Which check found a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// A pattern rule; `refuses` is false for a rule that only warns (Stripe
    /// test mode).
    Pattern { refuses: bool },
    /// The exact bytes of a key the person saved.
    Saved,
}

/// A key found in a package. It holds nothing of the key, and shows nothing
/// of a file or folder name that holds one.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyHit {
    pub place: Place,
    /// Whose key it is or looks like: `"Anthropic"`, `"OpenAI"`, `"xAI"`,
    /// `"AWS"`, `"Stripe"`, `"Stripe test mode"`, `"Google"`, `"GitHub"`,
    /// `"Slack"`, `"a private key"`, or a saved key's provider.
    pub provider: &'static str,
    pub found: Found,
    /// The file it is in, so a message counts each file once. Never shown.
    file: String,
}

impl KeyHit {
    /// A hit in the file at `path`, whose path holds no key.
    pub fn in_file(path: impl Into<String>, spot: Spot, provider: &'static str, found: Found) -> Self {
        let path = path.into();
        Self { file: path.clone(), place: Place::File { path, spot }, provider, found }
    }

    /// Where the key is, for a person: `path:line` in a UTF-8 file, `path at
    /// byte offset n` in any other, and only the scope for a file whose name
    /// holds a key.
    pub fn location(&self) -> String {
        match &self.place {
            Place::File { path, spot: Spot::Line(line) } => format!("{path}:{line}"),
            Place::File { path, spot: Spot::Byte(offset) } => format!("{path} at byte offset {offset}"),
            Place::UnnamedFile { scope } => format!("a file in {scope}"),
            Place::Name { scope } => format!("a file or folder name in {scope}"),
        }
    }

    /// Whether this key refuses a package going to `destination`: any key
    /// whose rule refuses, in a package that leaves this machine.
    pub fn refuses_at(&self, destination: Destination) -> bool {
        let refuses = match self.found {
            Found::Pattern { refuses } => refuses,
            Found::Saved => true,
        };
        refuses && destination.leaves_machine()
    }

    /// What the place holds, each provider in its own words.
    fn phrase(&self) -> String {
        match (self.found, self.provider) {
            (Found::Saved, provider) => format!("contains your saved {provider} key"),
            (_, PRIVATE_KEY) => "contains what looks like a private key".to_string(),
            (_, provider) => format!("contains what looks like a key for {provider}"),
        }
    }

    /// The hit's name in a list of files.
    fn label(&self) -> String {
        match self.found {
            Found::Saved => format!("your saved {} key", self.provider),
            Found::Pattern { .. } => self.provider.to_string(),
        }
    }
}

impl fmt::Debug for KeyHit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyHit({} ({}))", self.location(), self.label())
    }
}

/// What screening found in a set of records, before it is judged for a
/// destination.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Findings {
    /// SoulService's credential fields were taken out.
    pub stripped: bool,
    /// The scopes whose SoulService settings could not be read, and ship
    /// empty.
    pub emptied: Vec<String>,
    /// Every key found, file by file, each file's name before its bytes.
    pub hits: Vec<KeyHit>,
}

impl Findings {
    /// Add `other` to these.
    pub fn extend(&mut self, other: Findings) {
        self.stripped |= other.stripped;
        for scope in other.emptied {
            if !self.emptied.contains(&scope) {
                self.emptied.push(scope);
            }
        }
        self.hits.extend(other.hits);
    }
}

/// What screening a package leaves to say once the package may go ahead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Screened {
    /// SoulService's credential fields were taken out.
    pub stripped: bool,
    /// [`SOUL_KEY_NOTICE`], when they came out of a package that leaves this
    /// machine.
    pub notice: Option<&'static str>,
    /// Lines to show as warnings: [`SOUL_KEY_NOTICE`] when they came out of a
    /// package that stays on this machine; one line for each scope whose
    /// SoulService settings ship empty; then one line per key the
    /// destination lets through, at most 8, then `and {k} more`. None shows
    /// a key, or a name that holds one.
    pub warnings: Vec<String>,
}

/// A key's start and length in some bytes, whose it is or looks like, and
/// which check found it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KeyMatch {
    pub(crate) at: usize,
    pub(crate) len: usize,
    pub(crate) provider: &'static str,
    pub(crate) found: Found,
}

/// What a pattern rule found at the start of the bytes it was given: the
/// key's length, whose it looks like, and whether the rule refuses.
type Rule = (usize, &'static str, bool);

/// Every key the pattern rules find in `bytes`, in order and none
/// overlapping.
///
/// Every rule but the PEM header's starts at a word boundary, as `\b` reads
/// one, and after an escaped `\n`, `\t` or `\r` too: no byte comes before the
/// key; or the byte before it is anything but an ASCII letter, digit or `_`
/// (so a `-` before it is a boundary); or a backslash and `n`, `t` or `r`
/// come right before it. A rule's run is the longest run of the bytes it
/// names.
///
/// - Anthropic and OpenAI: `sk-`, then a run of ASCII letters, digits, `_`
///   and `-` at least 20 long that holds an ASCII digit and an upper-case
///   ASCII letter. Anthropic's when the run starts `ant-`, else OpenAI's.
/// - xAI: `xai-`, then a run of ASCII letters and digits at least 20 long that
///   holds a digit and an upper-case letter.
/// - AWS: `AKIA` or `ASIA`, then exactly 16 upper-case ASCII letters or
///   digits, then the end of the bytes or a byte other than an ASCII letter,
///   digit or `_`.
/// - Stripe: `sk_live_` or `rk_live_`, then 20 or more ASCII letters and
///   digits. Stripe test mode: `sk_test_` or `rk_test_` and the same run; it
///   only warns, wherever the package goes.
/// - Google: `AIza`, then 35 or more ASCII letters, digits, `_` and `-`.
/// - GitHub: `ghp_`, `gho_`, `ghu_`, `ghs_` or `ghr_`, then 36 or more ASCII
///   letters and digits; or `github_pat_`, then 22 or more ASCII letters,
///   digits and `_`.
/// - Slack: `xox`, one of `a b p o s r`, `-`, then a run of ASCII letters,
///   digits and `-` at least 10 long that holds a digit.
/// - A private key: the header, [`PEM_BEGIN`], then any number of words of
///   upper-case ASCII letters and digits each followed by one space, then
///   [`PEM_PRIVATE_KEY`] and one of [`PEM_ENDS`]: PEM's dashes, or a PGP key
///   block's. It needs no boundary.
pub(crate) fn find_keys(bytes: &[u8]) -> Vec<KeyMatch> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        match key_at(bytes, at) {
            Some(key) => {
                at += key.len;
                found.push(key);
            }
            None => at += 1,
        }
    }
    found
}

/// The key that starts at `at`, if one does. The first byte picks the rules
/// worth trying, before any boundary is checked.
fn key_at(bytes: &[u8], at: usize) -> Option<KeyMatch> {
    let rest = &bytes[at..];
    let (len, provider, refuses) = match rest[0] {
        b'-' => (private_key_header(rest)?, PRIVATE_KEY, true),
        b's' | b'r' | b'x' | b'A' | b'g' if at_word_boundary(bytes, at) => word_key(rest)?,
        _ => return None,
    };
    Some(KeyMatch { at, len, provider, found: Found::Pattern { refuses } })
}

/// The rules that start at a word boundary, tried on `rest`.
fn word_key(rest: &[u8]) -> Option<Rule> {
    sk_key(rest)
        .or_else(|| stripe_key(rest))
        .or_else(|| xai_key(rest))
        .or_else(|| slack_key(rest))
        .or_else(|| aws_key(rest))
        .or_else(|| google_key(rest))
        .or_else(|| github_key(rest))
}

fn sk_key(rest: &[u8]) -> Option<Rule> {
    let run = run_of(rest.strip_prefix(b"sk-")?, is_word_or_hyphen);
    if run.len() < SK_MIN_RUN || !has_digit_and_upper(run) {
        return None;
    }
    let provider = if run.starts_with(b"ant-") { "Anthropic" } else { "OpenAI" };
    Some((3 + run.len(), provider, true))
}

fn stripe_key(rest: &[u8]) -> Option<Rule> {
    let mode = rest.strip_prefix(b"sk_").or_else(|| rest.strip_prefix(b"rk_"))?;
    let (after, provider, refuses) = if let Some(after) = mode.strip_prefix(b"live_") {
        (after, "Stripe", true)
    } else if let Some(after) = mode.strip_prefix(b"test_") {
        (after, "Stripe test mode", false)
    } else {
        return None;
    };
    let run = run_of(after, is_letter_or_digit);
    (run.len() >= STRIPE_MIN_RUN).then_some((8 + run.len(), provider, refuses))
}

fn xai_key(rest: &[u8]) -> Option<Rule> {
    let run = run_of(rest.strip_prefix(b"xai-")?, is_letter_or_digit);
    (run.len() >= XAI_MIN_RUN && has_digit_and_upper(run)).then_some((4 + run.len(), "xAI", true))
}

fn slack_key(rest: &[u8]) -> Option<Rule> {
    let (&kind, after) = rest.strip_prefix(b"xox")?.split_first()?;
    let after = after.strip_prefix(b"-").filter(|_| b"abposr".contains(&kind))?;
    let run = run_of(after, is_letter_digit_or_hyphen);
    (run.len() >= SLACK_MIN_RUN && run.iter().any(u8::is_ascii_digit)).then_some((5 + run.len(), "Slack", true))
}

fn aws_key(rest: &[u8]) -> Option<Rule> {
    let run = rest.strip_prefix(b"AKIA").or_else(|| rest.strip_prefix(b"ASIA"))?;
    let id = run.get(..AWS_KEY_ID_LEN)?;
    let ends = run.get(AWS_KEY_ID_LEN).is_none_or(|&b| !is_word_byte(b));
    (id.iter().all(|&b| is_upper_or_digit(b)) && ends).then_some((4 + AWS_KEY_ID_LEN, "AWS", true))
}

fn google_key(rest: &[u8]) -> Option<Rule> {
    let run = run_of(rest.strip_prefix(b"AIza")?, is_word_or_hyphen);
    (run.len() >= GOOGLE_MIN_RUN).then_some((4 + run.len(), "Google", true))
}

fn github_key(rest: &[u8]) -> Option<Rule> {
    if let Some(after) = rest.strip_prefix(b"github_pat_") {
        let run = run_of(after, is_word_byte);
        return (run.len() >= GITHUB_PAT_MIN_RUN).then_some((11 + run.len(), "GitHub", true));
    }
    let (&kind, after) = rest.strip_prefix(b"gh")?.split_first()?;
    let after = after.strip_prefix(b"_").filter(|_| b"pousr".contains(&kind))?;
    let run = run_of(after, is_letter_or_digit);
    (run.len() >= GITHUB_TOKEN_MIN_RUN).then_some((4 + run.len(), "GitHub", true))
}

/// The private key header at the start of `rest`: its length. At each word,
/// [`PEM_PRIVATE_KEY`] and an end finish the header; otherwise a word of
/// upper-case letters and digits and its one space carry on to the next.
fn private_key_header(rest: &[u8]) -> Option<usize> {
    if !rest.starts_with(PEM_BEGIN) {
        return None;
    }
    let mut at = PEM_BEGIN.len();
    loop {
        if let Some(after) = rest[at..].strip_prefix(PEM_PRIVATE_KEY) {
            if let Some(end) = PEM_ENDS.iter().find(|end| after.starts_with(end)) {
                return Some(at + PEM_PRIVATE_KEY.len() + end.len());
            }
        }
        let word = run_len(&rest[at..], is_upper_or_digit);
        if word == 0 || rest.get(at + word) != Some(&b' ') {
            return None;
        }
        at += word + 1;
    }
}

/// Whether a key may start at `at`: no byte comes before it, the byte before
/// it is not a word byte, or an escaped `\n`, `\t` or `\r` comes right before
/// it.
fn at_word_boundary(bytes: &[u8], at: usize) -> bool {
    at == 0 || !is_word_byte(bytes[at - 1]) || after_escape(bytes, at)
}

/// Whether a backslash and `n`, `t` or `r` come right before `at`: a line
/// break or tab as JSON and script strings write one.
fn after_escape(bytes: &[u8], at: usize) -> bool {
    at >= 2 && bytes[at - 2] == b'\\' && matches!(bytes[at - 1], b'n' | b't' | b'r')
}

/// ASCII letters, digits and `_`: the bytes `\w` matches.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// ASCII letters, digits, `_` and `-`.
fn is_word_or_hyphen(b: u8) -> bool {
    is_word_byte(b) || b == b'-'
}

/// ASCII letters and digits.
fn is_letter_or_digit(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

/// ASCII letters, digits and `-`.
fn is_letter_digit_or_hyphen(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-'
}

/// Upper-case ASCII letters and digits.
fn is_upper_or_digit(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit()
}

/// How many bytes at the start of `bytes` are in `class`.
fn run_len(bytes: &[u8], class: fn(u8) -> bool) -> usize {
    bytes.iter().take_while(|&&b| class(b)).count()
}

/// The bytes at the start of `bytes` that are in `class`.
fn run_of(bytes: &[u8], class: fn(u8) -> bool) -> &[u8] {
    &bytes[..run_len(bytes, class)]
}

/// Whether `run` holds an ASCII digit and an upper-case ASCII letter, as the
/// `sk-` and `xai-` keys do and words and slugs do not.
fn has_digit_and_upper(run: &[u8]) -> bool {
    let digit = run.iter().any(u8::is_ascii_digit);
    let upper = run.iter().any(u8::is_ascii_uppercase);
    digit && upper
}

/// A file that starts with a UTF-16 byte order mark (FF FE little-endian, FE
/// FF big-endian), as one byte per UTF-16 unit: the unit itself when it is
/// ASCII, else a zero, which no key holds. Unit `i` sits at byte offset
/// `2 + 2 * i` of the file. A trailing odd byte is left out.
fn utf16_units(bytes: &[u8]) -> Option<Vec<u8>> {
    let little = match bytes {
        [0xFF, 0xFE, ..] => true,
        [0xFE, 0xFF, ..] => false,
        _ => return None,
    };
    let units = bytes[2..].chunks_exact(2).map(|pair| {
        let unit = if little { u16::from_le_bytes([pair[0], pair[1]]) } else { u16::from_be_bytes([pair[0], pair[1]]) };
        u8::try_from(unit).ok().filter(u8::is_ascii).unwrap_or(0)
    });
    Some(units.collect())
}

/// Every key `find` finds in a file's bytes, each with its spot: a line in a
/// UTF-8 file, a byte offset in any other. A file that starts with a UTF-16
/// byte order mark is read as UTF-16 text too, each key there at the byte
/// offset of its first byte in the file.
fn content_matches(bytes: &[u8], find: &dyn Fn(&[u8]) -> Vec<KeyMatch>) -> Vec<(Spot, KeyMatch)> {
    let mut out = Vec::new();
    let found = find(bytes);
    if !found.is_empty() {
        let utf8 = std::str::from_utf8(bytes).is_ok();
        let (mut line, mut counted) = (1, 0);
        for key in found {
            let spot = if utf8 {
                line += bytes[counted..key.at].iter().filter(|&&b| b == b'\n').count();
                counted = key.at;
                Spot::Line(line)
            } else {
                Spot::Byte(key.at)
            };
            out.push((spot, key));
        }
    }
    if let Some(units) = utf16_units(bytes) {
        for key in find(&units) {
            let spot = Spot::Byte(2 + 2 * key.at);
            if !out.iter().any(|(seen, other)| *seen == spot && other.provider == key.provider) {
                out.push((spot, key));
            }
        }
    }
    out
}

/// How a message names `scope`: as it is, unless its own name holds what
/// looks like a key.
fn shown_scope<'a>(scope: &'a str, find: &dyn Fn(&[u8]) -> Vec<KeyMatch>) -> &'a str {
    if find(scope.as_bytes()).is_empty() { scope } else { UNNAMED_SPACE }
}

/// Every key `find` finds in `records`, placed for a person, file by file:
/// each file's name first, then its bytes. `prefix` goes before each path in
/// a message, and `scope` is how a message names the records' Space. Every
/// byte is read, and every name: the Space's own and each file's path. A name
/// that holds what looks like a key is never shown; its file is named only by
/// its scope.
pub(crate) fn hits_in(records: &[Record], prefix: &str, scope: &str, find: &dyn Fn(&[u8]) -> Vec<KeyMatch>) -> Vec<KeyHit> {
    let mut hits = Vec::new();
    let scope_named = find(scope.as_bytes());
    let shown = if scope_named.is_empty() { scope } else { UNNAMED_SPACE };
    for key in &scope_named {
        hits.push(KeyHit { place: Place::Name { scope: shown.to_string() }, provider: key.provider, found: key.found, file: prefix.to_string() });
    }
    for (path, bytes) in records {
        let file = format!("{prefix}{path}");
        let named = find(path.as_bytes());
        for key in &named {
            hits.push(KeyHit { place: Place::Name { scope: shown.to_string() }, provider: key.provider, found: key.found, file: file.clone() });
        }
        let unnamed = !scope_named.is_empty() || !named.is_empty();
        for (spot, key) in content_matches(bytes, find) {
            let place = if unnamed { Place::UnnamedFile { scope: shown.to_string() } } else { Place::File { path: file.clone(), spot } };
            hits.push(KeyHit { place, provider: key.provider, found: key.found, file: file.clone() });
        }
    }
    hits
}

/// The first key the pattern rules find in `bytes`, UTF-16 text included:
/// where it starts, and whose it looks like.
pub fn provider_key_at(bytes: &[u8]) -> Option<(Spot, &'static str)> {
    content_matches(bytes, &find_keys).into_iter().next().map(|(spot, key)| (spot, key.provider))
}

/// The message that refuses a package: each file a refusing key is in, named
/// once by its first such key's place and provider, at most 8, then the rest
/// counted. `None` when no key refuses. It never quotes a key, nor a name
/// that holds one, and its "try again" fits publishing and hosting alike.
fn refusal_message(refusing: &[&KeyHit]) -> Option<String> {
    let mut seen = BTreeSet::new();
    let files: Vec<&KeyHit> = refusing.iter().copied().filter(|hit| seen.insert(hit.file.as_str())).collect();
    let is_name = |hit: &&KeyHit| matches!(hit.place, Place::Name { .. });
    match files.as_slice() {
        [] => None,
        [hit] if is_name(hit) => Some(format!("Refused: {} {}. Rename it, then try again.", hit.location(), hit.phrase())),
        [hit] => Some(format!("Refused: {} {}. Remove the key from that file, then try again.", hit.location(), hit.phrase())),
        _ => {
            let named: Vec<String> = files
                .iter()
                .take(REFUSAL_NAMES_AT_MOST)
                .map(|hit| format!("{} ({})", hit.location(), hit.label()))
                .collect();
            let more = match files.len() - named.len() {
                0 => String::new(),
                k => format!(" and {k} more"),
            };
            let ask = if files.iter().any(is_name) {
                "Remove the keys from those files and rename the ones whose names hold one, then try again."
            } else {
                "Remove the keys from those files, then try again."
            };
            Some(format!("Refused: {} files contain what looks like keys: {}{more}. {ask}", files.len(), named.join(", ")))
        }
    }
}

/// One warning line per hit, at most 8, then one line counting the rest.
fn warning_lines(hits: &[&KeyHit]) -> Vec<String> {
    let mut lines: Vec<String> =
        hits.iter().take(WARNING_LINES_AT_MOST).map(|hit| format!("{} {}.", hit.location(), hit.phrase())).collect();
    if hits.len() > WARNING_LINES_AT_MOST {
        lines.push(format!("and {} more", hits.len() - WARNING_LINES_AT_MOST));
    }
    lines
}

/// SoulService's credentials out, then every key in what is left: the first
/// half of [`screen_package`], for a caller that judges several record sets
/// as one package. `prefix` goes before each path in a message (a Space's
/// name and `/`, say), and `scope` is how a message names the set (the
/// Space's name). The scan reads only the bytes after the strip, the bytes
/// that ship.
pub fn screen_records(records: &mut [Record], prefix: &str, scope: &str) -> Findings {
    let (stripped, emptied) = strip_soul_credentials(records);
    let hits = hits_in(records, prefix, scope, &find_keys);
    let emptied = if emptied { vec![shown_scope(scope, &find_keys).to_string()] } else { Vec::new() };
    Findings { stripped, emptied, hits }
}

/// Judge what [`screen_records`] found, for `destination`.
///
/// A package that leaves this machine ([`Destination::Publish`],
/// [`Destination::LanHost`]) is refused by any key whose rule refuses; the
/// error names each such file and where in it, or only the scope of a file
/// whose name holds a key, and the provider, never the key. Every other key,
/// and every key on a [`Destination::LoopbackHost`], becomes one warning
/// line. When the strip removed something, a package that leaves gets
/// [`SOUL_KEY_NOTICE`] as its notice, and one that stays gets it as its first
/// warning line. Settings that ship empty get one warning line per scope.
pub fn screen_outcome(findings: &Findings, destination: Destination) -> Result<Screened, String> {
    let (refusing, passing): (Vec<&KeyHit>, Vec<&KeyHit>) = findings.hits.iter().partition(|hit| hit.refuses_at(destination));
    if let Some(refusal) = refusal_message(&refusing) {
        return Err(refusal);
    }
    let notice = (findings.stripped && destination.leaves_machine()).then_some(SOUL_KEY_NOTICE);
    let mut warnings = Vec::new();
    if findings.stripped && notice.is_none() {
        warnings.push(SOUL_KEY_NOTICE.to_string());
    }
    for scope in &findings.emptied {
        warnings.push(format!("SoulService's settings in {scope} could not be read, so this package carries them empty."));
    }
    warnings.extend(warning_lines(&passing));
    Ok(Screened { stripped: findings.stripped, notice, warnings })
}

/// Screen one package for `destination`: take SoulService's credentials out,
/// always and first, then read every byte and every name in it for keys and
/// judge them ([`screen_outcome`]). `Err` is the refusal.
pub fn screen_package(records: &mut [Record], destination: Destination) -> Result<Screened, String> {
    screen_outcome(&screen_records(records, "", PACKAGE_SCOPE), destination)
}

/// A record, as the strip sees it.
enum SoulSettings {
    /// Not SoulService's settings.
    Other,
    /// SoulService's settings, which cannot be read as TOML.
    Unreadable,
    /// SoulService's settings.
    Read(toml::Table),
}

/// Take SoulService's credentials out of its settings among `records`: every
/// `_service.toml`, in any folder, whose `[service] class_name` is
/// SoulService, and the one at [`SOUL_SERVICE_RECORD`]. Settings that cannot
/// be read as TOML, or cannot be written back, ship empty, never raw. Returns
/// whether anything came out, and whether any settings were emptied.
fn strip_soul_credentials(records: &mut [Record]) -> (bool, bool) {
    let (mut stripped, mut emptied) = (false, false);
    for (path, bytes) in records.iter_mut() {
        match soul_service_settings(path, bytes) {
            SoulSettings::Other => {}
            SoulSettings::Unreadable => {
                bytes.clear();
                emptied = true;
            }
            SoulSettings::Read(mut doc) => {
                if remove_keys(&mut doc, &SOUL_CREDENTIAL_KEYS) > 0 {
                    stripped = true;
                    match toml::to_string(&doc) {
                        Ok(text) => *bytes = text.into_bytes(),
                        Err(_) => {
                            bytes.clear();
                            emptied = true;
                        }
                    }
                }
            }
        }
    }
    (stripped, emptied)
}

/// Whether the record at `path` is SoulService's settings. Any
/// `_service.toml` is, by class, when it reads as TOML; the one at
/// [`SOUL_SERVICE_RECORD`] is, by place. One that does not read as TOML is
/// taken as SoulService's when it is at that place, when its folder's name
/// starts with SoulService, or when its bytes name SoulService.
fn soul_service_settings(path: &str, bytes: &[u8]) -> SoulSettings {
    let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
    if name != SERVICE_FILE {
        return SoulSettings::Other;
    }
    let at_template_place = path == SOUL_SERVICE_RECORD;
    match std::str::from_utf8(bytes).ok().and_then(|text| text.parse::<toml::Table>().ok()) {
        Some(doc) if at_template_place || names_soul_service(&doc) => SoulSettings::Read(doc),
        Some(_) => SoulSettings::Other,
        None => {
            let folder_name = folder.rsplit('/').next().unwrap_or(folder);
            let named = bytes.windows(SOUL_SERVICE_CLASS.len()).any(|w| w == SOUL_SERVICE_CLASS.as_bytes());
            if at_template_place || folder_name.starts_with(SOUL_SERVICE_CLASS) || named {
                SoulSettings::Unreadable
            } else {
                SoulSettings::Other
            }
        }
    }
}

/// Whether a service's settings name SoulService as their class in
/// `[service] class_name`, the place the service loader reads it from,
/// spelled any way the loader reads (`[Service] ClassName` too).
fn names_soul_service(doc: &toml::Table) -> bool {
    doc.iter().filter(|(key, _)| same_key(key, "service")).filter_map(|(_, value)| value.as_table()).any(|service| {
        service.iter().any(|(key, value)| same_key(key, "class_name") && value.as_str() == Some(SOUL_SERVICE_CLASS))
    })
}

/// Whether `key` is `canonical` as the service loader reads a key: in any
/// case and with any underscores, so `ApiKey`, `API_KEY` and `api_key` are
/// one key, and `api_key_hint` another.
fn same_key(key: &str, canonical: &str) -> bool {
    let fold = |s: &str| s.chars().filter(|c| *c != '_').map(|c| c.to_ascii_lowercase()).collect::<String>();
    fold(key) == fold(canonical)
}

/// Take every entry `names` names out of `table` and out of every table
/// below it, arrays of tables included. Returns how many went.
fn remove_keys(table: &mut toml::Table, names: &[&str]) -> usize {
    let mut removed = 0;
    table.retain(|key, value| {
        let credential = names.iter().any(|name| same_key(key, name));
        if credential {
            removed += 1;
        } else {
            removed += remove_keys_below(value, names);
        }
        !credential
    });
    removed
}

/// [`remove_keys`] on every table inside `value`.
fn remove_keys_below(value: &mut toml::Value, names: &[&str]) -> usize {
    match value {
        toml::Value::Table(table) => remove_keys(table, names),
        toml::Value::Array(items) => items.iter_mut().map(|item| remove_keys_below(item, names)).sum(),
        _ => 0,
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
    fn record_paths_cannot_name_a_device_or_end_in_a_dot_or_space() {
        // Each of these opens a device, or a file Windows writes as another.
        for bad in [
            "Workspace/COM1/x", "Workspace/nul.txt", "Workspace/CON", "Workspace/lpt9.log/a",
            "Workspace/CONIN$", "Workspace/conout$.toml", "Workspace/NUL .txt", "Workspace/COM0",
            "Workspace/COM\u{b9}/x", "Workspace/LPT\u{b3}.log", "Workspace/a.toml.", "Workspace/a.toml ",
        ] {
            assert!(!is_safe_record_path(bad), "accepted {bad:?}");
        }
        for ok in [
            "Workspace/Console/x", "Workspace/COM10/x", "Workspace/nullable.txt",
            "Workspace/Part/_instance.toml", "Workspace/a.toml", "Workspace/Lpt/x",
        ] {
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

#[cfg(test)]
mod provider_key_tests {
    //! Every key-shaped string here, PEM headers included, is built at run
    //! time from pieces, so the source holds none, and every assertion that
    //! could fail on one prints a label, never the text it tested.
    use super::*;

    const EVERY_DESTINATION: [Destination; 3] = [Destination::Publish, Destination::LanHost, Destination::LoopbackHost];
    const PATTERN: Found = Found::Pattern { refuses: true };

    /// The pieces, joined.
    fn join(pieces: &[&str]) -> String {
        pieces.concat()
    }

    /// `n` characters taken in turn from `alphabet`.
    fn run(alphabet: &[u8], n: usize) -> String {
        alphabet.iter().cycle().take(n).map(|&b| char::from(b)).collect()
    }

    /// A lower-case letter, an upper-case letter and a digit in turn.
    fn strong(n: usize) -> String {
        (0..n)
            .map(|i| match i % 3 {
                0 => char::from(b'a' + (i % 26) as u8),
                1 => char::from(b'A' + (i * 7 % 26) as u8),
                _ => char::from(b'0' + (i % 10) as u8),
            })
            .collect()
    }

    /// A lower-case letter, an upper-case letter, a digit, `_` and `-` in turn.
    fn mixed(n: usize) -> String {
        (0..n)
            .map(|i| match i % 5 {
                0 => char::from(b'a' + (i % 26) as u8),
                1 => char::from(b'A' + (i * 7 % 26) as u8),
                2 => char::from(b'0' + (i % 10) as u8),
                3 => '_',
                _ => '-',
            })
            .collect()
    }

    /// A lower-case letter, an upper-case letter, a digit and `_` in turn.
    fn word_run(n: usize) -> String {
        (0..n)
            .map(|i| match i % 4 {
                0 => char::from(b'a' + (i % 26) as u8),
                1 => char::from(b'A' + (i * 7 % 26) as u8),
                2 => char::from(b'0' + (i % 10) as u8),
                _ => '_',
            })
            .collect()
    }

    /// Lower-case, then upper-case letters: no digit.
    fn letters(n: usize) -> String {
        run(&(b'a'..=b'z').chain(b'A'..=b'Z').collect::<Vec<u8>>(), n)
    }

    /// Lower-case letters, then digits: no upper-case letter.
    fn lower_digits(n: usize) -> String {
        run(&(b'a'..=b'z').chain(b'0'..=b'9').collect::<Vec<u8>>(), n)
    }

    /// Upper-case letters and digits.
    fn upper_digits(n: usize) -> String {
        run(&(b'A'..=b'Z').chain(b'0'..=b'9').collect::<Vec<u8>>(), n)
    }

    /// A PEM header made of its pieces, with `words` between them, and its
    /// end: PEM's dashes, or a PGP key block's.
    fn pem_with_end(words: &[&str], end: &str) -> String {
        let mut header = join(&["-----", "BEGIN", " "]);
        for word in words {
            header.push_str(word);
            header.push(' ');
        }
        header + &join(&["PRIVATE", " KEY"]) + end
    }

    fn pem(words: &[&str]) -> String {
        pem_with_end(words, "-----")
    }

    /// An Anthropic-like key: digits and upper-case letters in its run.
    fn anthropic() -> String {
        join(&["sk-", "ant-", "api03-", &mixed(80)])
    }

    /// One fake of each refusing shape: its provider, a label that describes
    /// it without quoting it, and the fake.
    fn refusing_fakes() -> Vec<(&'static str, &'static str, String)> {
        vec![
            ("Anthropic", "Anthropic-like, sk-ant-api03- and 80 mixed", anthropic()),
            ("OpenAI", "OpenAI project-like, sk-proj- and 100 mixed", join(&["sk-", "proj-", &mixed(100)])),
            ("OpenAI", "OpenAI legacy-like, sk- and 48", join(&["sk-", &strong(48)])),
            ("xAI", "xAI-like, xai- and 80", join(&["xai-", &strong(80)])),
            ("AWS", "AKIA and 16", join(&["AKIA", &upper_digits(16)])),
            ("AWS", "ASIA and 16", join(&["ASIA", &upper_digits(16)])),
            ("Stripe", "sk_live_ and 24", join(&["sk_", "live_", &strong(24)])),
            ("Stripe", "rk_live_ and 24", join(&["rk_", "live_", &strong(24)])),
            ("Google", "AIza and 35", join(&["AIza", &mixed(35)])),
            ("GitHub", "ghp_ and 36", join(&["ghp", "_", &strong(36)])),
            ("GitHub", "github_pat_ and 82", join(&["github", "_pat_", &word_run(82)])),
            ("Slack", "xoxb- and 24", join(&["xox", "b-", &strong(24)])),
            (PRIVATE_KEY, "the plain PEM header", pem(&[])),
            (PRIVATE_KEY, "the PEM header with RSA", pem(&["RSA"])),
            (PRIVATE_KEY, "the PGP key block header", pem_with_end(&["PGP"], " BLOCK-----")),
        ]
    }

    /// What a refusal or warning says a provider's key looks like.
    fn phrase(provider: &str) -> String {
        if provider == PRIVATE_KEY {
            "contains what looks like a private key".to_string()
        } else {
            format!("contains what looks like a key for {provider}")
        }
    }

    /// Whether 8 characters in a row of `key` appear in `text`.
    fn holds_a_piece(text: &str, key: &str) -> bool {
        let chars: Vec<char> = key.chars().collect();
        chars.windows(8).any(|piece| text.contains(&piece.iter().collect::<String>()))
    }

    /// One `.luau` record holding `text` on its third line.
    fn mlua_with(text: &str) -> Vec<Record> {
        let source = format!("-- settings\nlocal Config = {{}}\nConfig.key = \"{text}\"\nreturn Config\n");
        vec![("ReplicatedStorage/Config.luau".to_string(), source.into_bytes())]
    }

    fn findings(hits: Vec<KeyHit>) -> Findings {
        Findings { hits, ..Findings::default() }
    }

    fn hit(path: &str, line: usize, provider: &'static str) -> KeyHit {
        KeyHit::in_file(path, Spot::Line(line), provider, PATTERN)
    }

    /// None of `cases` reads as a key: bare, as a TOML string, or as a word
    /// in a line; and a package holding it goes through every destination
    /// with nothing to say.
    fn passes(cases: &[(&str, String)]) {
        for (label, text) in cases {
            let places = [text.clone(), format!("value = \"{text}\"\n"), format!("-- see {text} here\n")];
            for (place, bytes) in places.iter().enumerate() {
                assert_eq!(provider_key_at(bytes.as_bytes()), None, "{label}, placement {place}");
            }
            for destination in EVERY_DESTINATION {
                let mut records = mlua_with(text);
                assert_eq!(screen_package(&mut records, destination), Ok(Screened::default()), "{label} on {destination:?}");
            }
        }
    }

    #[test]
    fn the_sk_and_xai_fakes_are_refused() {
        for (provider, label, key) in refusing_fakes().into_iter().take(4) {
            assert_eq!(provider_key_at(key.as_bytes()).map(|(_, p)| p), Some(provider), "{label}");
            let mut records = mlua_with(&key);
            let Err(refusal) = screen_package(&mut records, Destination::Publish) else { panic!("{label} went through") };
            assert!(!holds_a_piece(&refusal, &key), "a piece of {label} reached the refusal");
            assert!(refusal.contains(&format!("Config.luau:3 contains what looks like a key for {provider}.")), "{refusal}");
        }
    }

    #[test]
    fn every_fake_is_found_after_any_boundary() {
        for (provider, label, key) in refusing_fakes() {
            for before in ["", " ", "\"", "=", "/"] {
                for after in ["", "\"", "\n", " "] {
                    let text = format!("{before}{key}{after}");
                    let found = provider_key_at(text.as_bytes()).map(|(_, p)| p);
                    assert_eq!(found, Some(provider), "{label}, after {before:?}, then {after:?}");
                }
            }
        }
    }

    /// A `-` before a prefix is a boundary, so a key behind one is found.
    #[test]
    fn a_hyphen_before_a_key_is_a_boundary() {
        for (provider, label, key) in refusing_fakes() {
            let text = format!("prod-{key}\n");
            assert_eq!(provider_key_at(text.as_bytes()).map(|(_, p)| p), Some(provider), "{label} after a hyphen");
        }
    }

    /// An escaped line break, tab or carriage return, as a JSON or script
    /// string writes one, is a boundary before every prefix.
    fn refused_after_escape(escape: &str) {
        for (provider, label, key) in refusing_fakes() {
            let json = format!("{{\"notes\": \"first line{escape}{key}{escape}last line\"}}\n");
            let mut records = vec![("Workspace/Note/notes.json".to_string(), json.into_bytes())];
            let Err(refusal) = screen_package(&mut records, Destination::Publish) else {
                panic!("{label} after an escaped {escape:?} went through")
            };
            assert!(!holds_a_piece(&refusal, &key), "a piece of {label} reached the refusal");
            assert!(refusal.contains(&phrase(provider)), "{label}: {refusal}");
        }
    }

    #[test]
    fn a_key_after_an_escaped_line_break_is_refused() {
        refused_after_escape("\\n");
    }

    #[test]
    fn a_key_after_an_escaped_tab_is_refused() {
        refused_after_escape("\\t");
    }

    #[test]
    fn a_key_after_an_escaped_carriage_return_is_refused() {
        refused_after_escape("\\r");
    }

    /// The escape is a boundary only right before a prefix: `task-` after it
    /// still holds `sk-` inside a word.
    #[test]
    fn a_prefix_inside_a_word_after_an_escape_passes() {
        passes(&[
            ("an escaped line break, then task- and 40", join(&["\\n", "task-", &strong(40)])),
            ("an escaped tab, then risk- and 40", join(&["\\t", "risk-", &strong(40)])),
        ]);
    }

    /// The `sk-` and `xai-` runs need a digit and an upper-case letter: a run
    /// of letters alone, or of lower-case letters and digits, is no key.
    #[test]
    fn a_run_without_a_digit_or_an_upper_case_letter_passes() {
        passes(&[
            ("sk- and 30 letters, no digit", join(&["sk-", &letters(30)])),
            ("sk- and 30 lower-case letters and digits", join(&["sk-", &lower_digits(30)])),
            ("xai- and 30 letters, no digit", join(&["xai-", &letters(30)])),
            ("xai- and 30 lower-case letters and digits", join(&["xai-", &lower_digits(30)])),
        ]);
    }

    /// The slug Security named: no digit and no upper-case letter, so no key
    /// on any destination.
    #[test]
    fn sk_learn_model_selection_utils_passes_on_every_destination() {
        let name = ["sk", "learn", "model", "selection", "utils"].join("-");
        assert_eq!(provider_key_at(name.as_bytes()), None);
        passes(&[("the hyphenated name", name)]);
    }

    /// One short of each rule, or a byte after it that breaks it.
    #[test]
    fn near_misses_pass() {
        passes(&[
            ("sk- and 19", join(&["sk-", &strong(19)])),
            ("sk-ant- and 10", join(&["sk-", "ant-", &strong(10)])),
            ("xai- and 19", join(&["xai-", &strong(19)])),
            ("AKIA and 15", join(&["AKIA", &upper_digits(15)])),
            ("AKIA and 16, then a lower-case letter", join(&["AKIA", &upper_digits(16), "x"])),
            ("AKIA and 16, then _", join(&["AKIA", &upper_digits(16), "_"])),
            ("AKIA and 17", join(&["AKIA", &upper_digits(17)])),
        ]);
    }

    /// A prefix inside a word is no key: `task-`, `risk-` and `desk-` hold
    /// `sk-`, `maxai-` holds `xai-`, and in `_sk-` the `_` is a word byte.
    #[test]
    fn a_prefix_inside_a_word_passes() {
        passes(&[
            ("task- and 40", join(&["task-", &strong(40)])),
            ("risk- and 40", join(&["risk-", &strong(40)])),
            ("desk- and 40", join(&["desk-", &strong(40)])),
            ("maxai- and 30", join(&["maxai-", &strong(30)])),
            ("_sk- and 30", join(&["_sk-", &strong(30)])),
        ]);
    }

    /// A fake of the rule is refused, in the provider's own words; its
    /// near-miss passes.
    fn refused_and_near_miss(provider: &str, fake: &str, near_miss: (&str, String)) {
        let mut records = mlua_with(fake);
        let Err(refusal) = screen_package(&mut records, Destination::Publish) else { panic!("the {provider} fake went through") };
        assert!(!holds_a_piece(&refusal, fake), "a piece of the {provider} fake reached the refusal");
        assert!(refusal.contains(&format!("{}.", phrase(provider))), "{refusal}");
        passes(&[near_miss]);
    }

    #[test]
    fn stripe_live_is_refused_and_10_characters_pass() {
        refused_and_near_miss("Stripe", &join(&["sk_", "live_", &strong(24)]), ("sk_live_ and 10", join(&["sk_", "live_", &strong(10)])));
        refused_and_near_miss("Stripe", &join(&["rk_", "live_", &strong(24)]), ("rk_live_ and 19", join(&["rk_", "live_", &strong(19)])));
    }

    #[test]
    fn google_is_refused_and_34_characters_pass() {
        refused_and_near_miss("Google", &join(&["AIza", &mixed(35)]), ("AIza and 34", join(&["AIza", &mixed(34)])));
    }

    #[test]
    fn github_is_refused_and_short_runs_pass() {
        refused_and_near_miss("GitHub", &join(&["ghp", "_", &strong(36)]), ("ghp_ and 35", join(&["ghp", "_", &strong(35)])));
        refused_and_near_miss("GitHub", &join(&["ghs", "_", &strong(40)]), ("gha_ and 40", join(&["gha", "_", &strong(40)])));
        refused_and_near_miss(
            "GitHub",
            &join(&["github", "_pat_", &word_run(82)]),
            ("github_pat_ and 21", join(&["github", "_pat_", &word_run(21)])),
        );
    }

    #[test]
    fn slack_is_refused_and_xoxo_hugs_passes() {
        refused_and_near_miss("Slack", &join(&["xox", "b-", &strong(24)]), ("xoxo-hugs", join(&["xox", "o-", "hugs"])));
        refused_and_near_miss("Slack", &join(&["xox", "p-", "1-", &letters(12)]), ("xoxb- and 10, no digit", join(&["xox", "b-", &letters(10)])));
    }

    /// The header alone is a private key, refused in its own words; a
    /// certificate's header is not.
    #[test]
    fn a_private_key_header_is_refused_and_a_certificate_passes() {
        let certificate = join(&["-----", "BEGIN", " ", "CERTIFICATE", "-----"]);
        refused_and_near_miss(PRIVATE_KEY, &pem(&[]), ("a certificate header", certificate));
        refused_and_near_miss(PRIVATE_KEY, &pem(&["ENCRYPTED"]), ("a public key header", join(&["-----", "BEGIN", " ", "PUBLIC", " KEY", "-----"])));
        refused_and_near_miss(PRIVATE_KEY, &pem(&["OPENSSH"]), ("PRIVATE KEY without its dashes", join(&["-----", "BEGIN", " ", "PRIVATE", " KEY"])));
        let mut records = mlua_with(&pem(&[]));
        let Err(refusal) = screen_package(&mut records, Destination::Publish) else { panic!("the PEM header went through") };
        assert_eq!(
            refusal,
            "Refused: ReplicatedStorage/Config.luau:3 contains what looks like a private key. Remove the key from that file, then try again."
        );
    }

    /// A PGP private key block's header is refused like PEM's.
    #[test]
    fn a_pgp_private_key_block_is_refused() {
        let certificate = join(&["-----", "BEGIN", " ", "CERTIFICATE", "-----"]);
        refused_and_near_miss(PRIVATE_KEY, &pem_with_end(&["PGP"], " BLOCK-----"), ("a certificate header", certificate));
        refused_and_near_miss(
            PRIVATE_KEY,
            &pem_with_end(&["PGP"], " BLOCK-----"),
            ("a PGP public key block", join(&["-----", "BEGIN", " PGP ", "PUBLIC", " KEY", " BLOCK-----"])),
        );
    }

    /// Stripe's test keys warn, wherever the package goes, and never refuse.
    #[test]
    fn stripe_test_keys_warn_and_never_refuse() {
        for prefix in ["sk_", "rk_"] {
            let key = join(&[prefix, "test_", &strong(24)]);
            for destination in EVERY_DESTINATION {
                let mut records = mlua_with(&key);
                let Ok(screened) = screen_package(&mut records, destination) else {
                    panic!("{prefix}test_ was refused on {destination:?}")
                };
                assert!(!holds_a_piece(&screened.warnings.join("\n"), &key), "a piece of the test key reached a warning");
                assert_eq!(
                    screened.warnings,
                    vec!["ReplicatedStorage/Config.luau:3 contains what looks like a key for Stripe test mode.".to_string()],
                    "{prefix}test_ on {destination:?}"
                );
            }
        }
    }

    #[test]
    fn a_mlua_key_is_refused_when_the_package_leaves_and_warned_on_a_loopback_host() {
        let key = anthropic();
        for destination in [Destination::Publish, Destination::LanHost] {
            let mut records = mlua_with(&key);
            let Err(refusal) = screen_package(&mut records, destination) else { panic!("{destination:?} let the key through") };
            assert!(!holds_a_piece(&refusal, &key), "a piece of the key reached the refusal");
            assert_eq!(
                refusal,
                "Refused: ReplicatedStorage/Config.luau:3 contains what looks like a key for Anthropic. Remove the key from that file, then try again."
            );
        }
        let mut records = mlua_with(&key);
        let Ok(screened) = screen_package(&mut records, Destination::LoopbackHost) else { panic!("the loopback host was refused") };
        assert!(!holds_a_piece(&format!("{screened:?}"), &key), "a piece of the key reached a warning");
        assert_eq!(
            screened,
            Screened {
                stripped: false,
                notice: None,
                warnings: vec!["ReplicatedStorage/Config.luau:3 contains what looks like a key for Anthropic.".to_string()],
            }
        );
    }

    /// A key in a mesh is read too, the JSON chunk inside a `.glb` included,
    /// and named by its byte offset, since the file is not UTF-8.
    #[test]
    fn a_key_in_a_glb_is_refused_with_its_byte_offset() {
        let key = join(&["sk-", "proj-", &mixed(100)]);
        let json = format!("{{\"extras\":{{\"token\":\"{key}\"}}}}");
        let mut glb = b"glTF\x02\x00\x00\x00\x00\x10\x00\x00".to_vec();
        glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(json.as_bytes());
        glb.extend_from_slice(&[0x10, 0x00, 0x00, 0x00, b'B', b'I', b'N', 0x00, 0xff, 0xfe, 0x80, 0x81]);
        let Some(at) = glb.windows(key.len()).position(|w| w == key.as_bytes()) else { panic!("the fake is not in the .glb") };
        let mut records = vec![("Workspace/Car/body.glb".to_string(), glb)];
        let Err(refusal) = screen_package(&mut records, Destination::Publish) else { panic!("the key in the .glb went through") };
        assert!(!holds_a_piece(&refusal, &key), "a piece of the key reached the refusal");
        assert_eq!(
            refusal,
            format!(
                "Refused: Workspace/Car/body.glb at byte offset {at} contains what looks like a key for OpenAI. Remove the key from that file, then try again."
            )
        );
    }

    /// `text` as a UTF-16 file: its byte order mark, then each character in
    /// two bytes, little-endian or big-endian, and an odd byte at the end.
    fn utf16_file(text: &str, little: bool) -> Vec<u8> {
        let mut bytes = if little { vec![0xFF, 0xFE] } else { vec![0xFE, 0xFF] };
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&if little { unit.to_le_bytes() } else { unit.to_be_bytes() });
        }
        bytes.push(0x41);
        bytes
    }

    /// A UTF-16 file is read as text: a key in it is refused, named by the
    /// byte offset of its first byte in the file.
    #[test]
    fn a_key_in_a_utf16_file_is_refused_with_its_byte_offset() {
        let key = join(&["xai-", &strong(40)]);
        let text = format!("caf\u{e9} notes\r\nkey: {key}\r\n");
        let Some(before) = text.find(key.as_str()) else { panic!("the fake is not in the text") };
        let at = 2 + 2 * text[..before].encode_utf16().count();
        for little in [true, false] {
            let mut records = vec![("Workspace/Note/notes.txt".to_string(), utf16_file(&text, little))];
            let Err(refusal) = screen_package(&mut records, Destination::Publish) else {
                panic!("the key in a UTF-16 file went through (little-endian: {little})")
            };
            assert!(!holds_a_piece(&refusal, &key), "a piece of the key reached the refusal");
            assert_eq!(
                refusal,
                format!("Refused: Workspace/Note/notes.txt at byte offset {at} contains what looks like a key for xAI. Remove the key from that file, then try again."),
                "little-endian: {little}"
            );
        }
    }

    #[test]
    fn a_utf16_file_without_a_key_passes() {
        let text = "Welcome to the harbor.\r\nPrices: caf\u{e9} 2, t\u{e9} 3.\r\n";
        for little in [true, false] {
            let mut records = vec![("Workspace/Note/notes.txt".to_string(), utf16_file(text, little))];
            assert_eq!(screen_package(&mut records, Destination::Publish), Ok(Screened::default()), "little-endian: {little}");
        }
    }

    /// A key used as a folder or file name is refused by the Space and the
    /// provider only, never by the name.
    #[test]
    fn a_key_in_a_file_name_is_refused_without_showing_the_name() {
        let key = anthropic();
        for path in [format!("Workspace/{key}/_instance.toml"), format!("Workspace/Notes/{key}.md")] {
            let mut records = vec![(path, b"[metadata]\nclass_name = \"Part\"\n".to_vec())];
            let findings = screen_records(&mut records, "Lobby/", "Lobby");
            let Err(refusal) = screen_outcome(&findings, Destination::Publish) else { panic!("a key in a name went through") };
            assert!(!holds_a_piece(&refusal, &key), "a piece of the key in the name reached the refusal");
            assert_eq!(
                refusal,
                "Refused: a file or folder name in Lobby contains what looks like a key for Anthropic. Rename it, then try again."
            );
            let Ok(screened) = screen_outcome(&findings, Destination::LoopbackHost) else { panic!("the loopback host was refused") };
            assert!(!holds_a_piece(&format!("{screened:?}"), &key), "a piece of the key in the name reached a warning");
            assert_eq!(screened.warnings, vec!["a file or folder name in Lobby contains what looks like a key for Anthropic.".to_string()]);
        }
    }

    /// A key inside a file whose name holds one, or inside a Space whose
    /// name does, is placed by its scope only.
    #[test]
    fn a_key_in_a_file_with_a_key_name_shows_neither() {
        let (in_name, in_text) = (anthropic(), join(&["xai-", &strong(40)]));
        let mut records = vec![(format!("Workspace/{in_name}.md"), format!("{in_text}\n").into_bytes())];
        let findings = screen_records(&mut records, "Lobby/", "Lobby");
        let shown = format!("{findings:?}");
        assert!(!holds_a_piece(&shown, &in_name) && !holds_a_piece(&shown, &in_text), "a piece of a key reached the findings");
        let Err(refusal) = screen_outcome(&findings, Destination::Publish) else { panic!("not refused") };
        assert_eq!(refusal, "Refused: a file or folder name in Lobby contains what looks like a key for Anthropic. Rename it, then try again.");
        let Ok(screened) = screen_outcome(&findings, Destination::LoopbackHost) else { panic!("refused on loopback") };
        assert_eq!(
            screened.warnings,
            vec![
                "a file or folder name in Lobby contains what looks like a key for Anthropic.".to_string(),
                "a file in Lobby contains what looks like a key for xAI.".to_string(),
            ]
        );
        let mut records = vec![("Workspace/notes.md".to_string(), format!("{in_text}\n").into_bytes())];
        let findings = screen_records(&mut records, &format!("{in_name}/"), &in_name);
        let Err(refusal) = screen_outcome(&findings, Destination::LanHost) else { panic!("a key-named Space went through") };
        assert!(!holds_a_piece(&refusal, &in_name) && !holds_a_piece(&refusal, &in_text), "a piece of a key reached the refusal");
        assert!(refusal.contains("a file or folder name in a Space") && refusal.contains("a file in a Space (xAI)"), "{refusal}");
    }

    /// Random bytes hold no key: every byte is read, and none is refused.
    #[test]
    fn random_bytes_hold_no_key() {
        let ramp: Vec<u8> = (0..=255u8).cycle().take(64 * 1024).collect();
        assert_eq!(provider_key_at(&ramp), None, "a byte ramp read as a key");
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let noise: Vec<u8> = (0..1 << 20)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 32) as u8
            })
            .collect();
        assert_eq!(provider_key_at(&noise), None, "a megabyte of noise read as a key");
    }

    /// SoulService's settings as Studio writes them, holding `key`.
    fn soul_service(key: &str) -> String {
        format!(
            "[service]\nclass_name = \"SoulService\"\nicon = \"soulservice\"\n\n\
             [properties]\nenable_soul_scripts = true\nuse_global_key = false\napi_key = \"{key}\"\n"
        )
    }

    /// Screen one record, as a package going to `destination`.
    fn screen_one(path: &str, text: &str, destination: Destination) -> (Result<Screened, String>, Vec<u8>) {
        let mut records = vec![(path.to_string(), text.as_bytes().to_vec())];
        let outcome = screen_package(&mut records, destination);
        (outcome, records.remove(0).1)
    }

    #[test]
    fn soul_service_key_is_stripped_with_the_notice_when_the_package_leaves() {
        let key = anthropic();
        for destination in [Destination::Publish, Destination::LanHost] {
            let (outcome, shipped) = screen_one(SOUL_SERVICE_RECORD, &soul_service(&key), destination);
            let Ok(screened) = outcome else { panic!("{destination:?}: SoulService's own key refused the package") };
            assert!(!holds_a_piece(&format!("{screened:?}"), &key), "a piece of the key reached the outcome");
            assert_eq!(screened, Screened { stripped: true, notice: Some(SOUL_KEY_NOTICE), warnings: Vec::new() }, "{destination:?}");
            let shipped = String::from_utf8_lossy(&shipped).into_owned();
            assert!(!holds_a_piece(&shipped, &key), "{destination:?}: the key shipped");
            assert!(shipped.contains("enable_soul_scripts") && !shipped.contains("api_key"), "{destination:?}: the strip");
        }
    }

    /// Security's rule: on a loopback host the notice is a warning line, word
    /// for word, and the package goes ahead.
    #[test]
    fn soul_service_key_on_a_loopback_host_is_stripped_and_warned_with_the_notice() {
        let key = anthropic();
        let (outcome, shipped) = screen_one(SOUL_SERVICE_RECORD, &soul_service(&key), Destination::LoopbackHost);
        let Ok(screened) = outcome else { panic!("the loopback host was refused") };
        assert!(!holds_a_piece(&format!("{screened:?}"), &key), "a piece of the key reached the outcome");
        assert_eq!(screened, Screened { stripped: true, notice: None, warnings: vec![SOUL_KEY_NOTICE.to_string()] });
        assert!(!holds_a_piece(&String::from_utf8_lossy(&shipped), &key), "the key shipped");
    }

    /// SoulService is found by its class, in any folder: a nested one and a
    /// renamed one are stripped like the template's.
    #[test]
    fn soul_service_is_found_by_its_class_in_any_folder() {
        let key = anthropic();
        for path in ["SoulService (2)/_service.toml", "Workspace/Tools/SoulService/_service.toml", "Brains/_service.toml"] {
            let (outcome, shipped) = screen_one(path, &soul_service(&key), Destination::Publish);
            let Ok(screened) = outcome else { panic!("{path}: the key was left for the scan") };
            assert!(screened.stripped && screened.notice == Some(SOUL_KEY_NOTICE), "{path}: not stripped");
            assert!(!holds_a_piece(&String::from_utf8_lossy(&shipped), &key), "{path}: the key shipped");
        }
    }

    /// The class is read as the service loader reads it, in any spelling; so
    /// are the credential fields, and only those.
    #[test]
    fn soul_service_spelled_as_the_loader_reads_it_is_stripped() {
        let key = anthropic();
        let text = format!(
            "[Service]\nClassName = \"SoulService\"\n\n[Properties]\nApiKey = \"{key}\"\nUseGlobalKey = false\nAPI_KEY = \"{key}\"\nApiKeyHint = \"paste it in Settings\"\n"
        );
        let (outcome, shipped) = screen_one("Brains/_service.toml", &text, Destination::Publish);
        let Ok(screened) = outcome else { panic!("the PascalCase key was left for the scan") };
        assert!(screened.stripped);
        let Ok(doc) = String::from_utf8_lossy(&shipped).parse::<toml::Table>() else { panic!("the stripped settings are not TOML") };
        let properties = doc.get("Properties").and_then(|v| v.as_table()).map(|t| t.keys().cloned().collect::<Vec<_>>());
        assert_eq!(properties, Some(vec!["ApiKeyHint".to_string()]), "only the hint stays");
    }

    /// Another service's settings keep an api_key: only SoulService's are
    /// stripped, and a key there is the scan's to find.
    #[test]
    fn another_services_api_key_is_not_stripped() {
        let website = "[service]\nclass_name = \"Website\"\n\n[properties]\napi_key = \"your key here\"\nuse_global_key = true\n";
        let (outcome, shipped) = screen_one("Website/_service.toml", website, Destination::Publish);
        assert_eq!(outcome, Ok(Screened::default()));
        assert!(shipped == website.as_bytes(), "another service's settings changed");
        let key = anthropic();
        let (outcome, _) = screen_one("Website/_service.toml", &website.replace("your key here", &key), Destination::Publish);
        let Err(refusal) = outcome else { panic!("an api_key on another service went through") };
        assert!(!holds_a_piece(&refusal, &key), "a piece of the key reached the refusal");
        assert!(refusal.starts_with("Refused: Website/_service.toml:5 contains what looks like a key for Anthropic."), "{refusal}");
    }

    /// At the template's place, settings with no class are SoulService's.
    #[test]
    fn the_template_place_is_stripped_without_a_class() {
        let key = anthropic();
        let (outcome, shipped) = screen_one(SOUL_SERVICE_RECORD, &format!("[properties]\napi_key = \"{key}\"\nmood = \"calm\"\n"), Destination::Publish);
        let Ok(screened) = outcome else { panic!("the key was left for the scan") };
        assert!(screened.stripped && String::from_utf8_lossy(&shipped).contains("mood"));
        assert!(!holds_a_piece(&String::from_utf8_lossy(&shipped), &key), "the key shipped");
    }

    /// SoulService settings that do not read as TOML ship empty, never raw,
    /// with one warning naming the scope.
    #[test]
    fn unreadable_soul_service_settings_ship_empty_with_one_warning() {
        let key = anthropic();
        let broken = format!("[service\nclass_name = \"SoulService\"\napi_key = \"{key}\"\n");
        for path in [SOUL_SERVICE_RECORD, "SoulService (2)/_service.toml", "Brains/_service.toml"] {
            for destination in EVERY_DESTINATION {
                let (outcome, shipped) = screen_one(path, &broken, destination);
                let Ok(screened) = outcome else { panic!("{path} on {destination:?}: refused") };
                assert!(shipped.is_empty(), "{path}: unreadable settings shipped raw");
                assert_eq!(
                    screened.warnings,
                    vec!["SoulService's settings in this package could not be read, so this package carries them empty.".to_string()],
                    "{path} on {destination:?}"
                );
            }
        }
        let (_, shipped) = screen_one("Website/_service.toml", "[service\nclass_name = \"Website\"\n", Destination::Publish);
        assert!(!shipped.is_empty(), "another service's unreadable settings were emptied");
    }

    /// SoulService's settings with both credential fields, a planted key in
    /// `api_key`, and `api_key_hint` beside them, in `place`.
    fn soul_with_credentials_in(place: &str, key: &str) -> String {
        let fields = format!("use_global_key = false\napi_key = \"{key}\"\napi_key_hint = \"paste it in Settings\"\n");
        let at = |here: &str| if place == here { fields.clone() } else { String::new() };
        format!(
            "{}name = \"Soul\"\n\n[service]\nclass_name = \"SoulService\"\n{}\n\
             [properties]\nenable_soul_scripts = true\n{}\n[properties.extras]\nmood = \"calm\"\n{}",
            at("top"),
            at("service"),
            at("properties"),
            at("extras")
        )
    }

    /// Both fields go from `place`; everything else stays, `api_key_hint`
    /// included.
    fn strips_at(place: &str) {
        let key = anthropic();
        let (outcome, shipped) = screen_one(SOUL_SERVICE_RECORD, &soul_with_credentials_in(place, &key), Destination::Publish);
        let Ok(screened) = outcome else { panic!("{place}: the key was left for the scan") };
        assert!(screened.stripped, "{place}: nothing was stripped");
        let Ok(text) = String::from_utf8(shipped) else { panic!("{place}: the stripped file is not UTF-8") };
        assert!(!holds_a_piece(&text, &key), "{place}: the key survived");
        let Ok(doc) = text.parse::<toml::Table>() else { panic!("{place}: the stripped file is not TOML") };
        let table = |name: &str| doc.get(name).and_then(|v| v.as_table());
        let extras = table("properties").and_then(|p| p.get("extras")).and_then(|v| v.as_table());
        for (name, found) in [("top", Some(&doc)), ("service", table("service")), ("properties", table("properties")), ("extras", extras)] {
            let Some(found) = found else { panic!("{place}: the table {name} was lost") };
            assert!(!found.contains_key("api_key") && !found.contains_key("use_global_key"), "{place}: a field survived in {name}");
            let hint = found.get("api_key_hint").and_then(|v| v.as_str());
            assert_eq!(hint, (name == place).then_some("paste it in Settings"), "{place}: api_key_hint in {name}");
        }
        assert_eq!(doc["name"].as_str(), Some("Soul"));
        assert_eq!(doc["service"]["class_name"].as_str(), Some("SoulService"));
        assert_eq!(doc["properties"]["enable_soul_scripts"].as_bool(), Some(true));
        assert_eq!(doc["properties"]["extras"]["mood"].as_str(), Some("calm"));
    }

    #[test]
    fn strip_removes_credentials_at_the_top_level() {
        strips_at("top");
    }

    #[test]
    fn strip_removes_credentials_in_service() {
        strips_at("service");
    }

    #[test]
    fn strip_removes_credentials_in_properties() {
        strips_at("properties");
    }

    #[test]
    fn strip_removes_credentials_in_properties_extras() {
        strips_at("extras");
    }

    /// An array of tables is a table too.
    #[test]
    fn strip_removes_credentials_in_an_array_of_tables() {
        let key = anthropic();
        let text = format!("[service]\nclass_name = \"SoulService\"\n\n[[properties.fallback]]\nname = \"grok\"\napi_key = \"{key}\"\n");
        let (outcome, shipped) = screen_one(SOUL_SERVICE_RECORD, &text, Destination::Publish);
        let Ok(screened) = outcome else { panic!("the array's key was left for the scan") };
        assert!(screened.stripped);
        let shipped = String::from_utf8_lossy(&shipped).into_owned();
        assert!(!holds_a_piece(&shipped, &key) && shipped.contains("grok"), "the array of tables");
    }

    /// Only SoulService's settings are stripped: an `api_key` on another
    /// class stays as it is, so a key there is the scan's to find.
    #[test]
    fn the_strip_leaves_other_classes_alone() {
        let instance = "[metadata]\nclass_name = \"Part\"\n\n[properties]\napi_key = \"your key here\"\nuse_global_key = true\n";
        let (outcome, shipped) = screen_one("Workspace/x/_instance.toml", instance, Destination::Publish);
        assert_eq!(outcome, Ok(Screened::default()));
        assert!(shipped == instance.as_bytes(), "another class's record changed");
        let key = anthropic();
        let (outcome, _) = screen_one("Workspace/x/_instance.toml", &instance.replace("your key here", &key), Destination::Publish);
        let Err(refusal) = outcome else { panic!("an api_key on another class went through") };
        assert!(!holds_a_piece(&refusal, &key), "a piece of the key reached the refusal");
        assert!(refusal.starts_with("Refused: Workspace/x/_instance.toml:5 contains what looks like a key for Anthropic."), "{refusal}");
    }

    #[test]
    fn lines_count_from_one() {
        let key = join(&["xai-", &strong(40)]);
        for (text, line) in [
            (key.clone(), 1),
            (format!("\n\n{key}"), 3),
            (format!("a\r\nb\r\nc = \"{key}\"\r\n"), 3),
            (format!("x\n{key}\n"), 2),
        ] {
            assert_eq!(provider_key_at(text.as_bytes()), Some((Spot::Line(line), "xAI")), "the key on line {line}");
        }
        let text = format!("one\n{key}\nthree\nfour\n{key}\n");
        let mut records = vec![("notes.md".to_string(), text.into_bytes())];
        let Ok(screened) = screen_package(&mut records, Destination::LoopbackHost) else { panic!("the loopback host was refused") };
        assert!(!holds_a_piece(&screened.warnings.join("\n"), &key), "a piece of the key reached a warning");
        assert_eq!(
            screened.warnings,
            vec![
                "notes.md:2 contains what looks like a key for xAI.".to_string(),
                "notes.md:5 contains what looks like a key for xAI.".to_string(),
            ]
        );
    }

    #[test]
    fn no_key_no_refusal_and_nothing_to_say() {
        for destination in EVERY_DESTINATION {
            assert_eq!(screen_outcome(&Findings::default(), destination), Ok(Screened::default()));
        }
    }

    #[test]
    fn one_file_is_refused_by_its_place_and_provider() {
        assert_eq!(
            screen_outcome(&findings(vec![hit("City/SoulService/_service.toml", 7, "Anthropic")]), Destination::Publish),
            Err("Refused: City/SoulService/_service.toml:7 contains what looks like a key for Anthropic. Remove the key from that file, then try again.".to_string())
        );
    }

    /// A saved key is named as the person's own, by its provider.
    #[test]
    fn a_saved_key_is_refused_as_the_persons_own() {
        let saved = KeyHit::in_file("ReplicatedStorage/Fetch.luau", Spot::Line(2), "SAM", Found::Saved);
        assert_eq!(
            screen_outcome(&findings(vec![saved.clone()]), Destination::Publish),
            Err("Refused: ReplicatedStorage/Fetch.luau:2 contains your saved SAM key. Remove the key from that file, then try again.".to_string())
        );
        let several = findings(vec![saved, hit("City/notes.md", 1, PRIVATE_KEY)]);
        assert_eq!(
            screen_outcome(&several, Destination::LanHost),
            Err("Refused: 2 files contain what looks like keys: ReplicatedStorage/Fetch.luau:2 (your saved SAM key), City/notes.md:1 (a private key). Remove the keys from those files, then try again.".to_string())
        );
    }

    /// Each file is named once, by its first refusing key.
    #[test]
    fn several_files_are_each_named_once() {
        let hits = vec![
            hit("City/Workspace/Note/_instance.toml", 3, "OpenAI"),
            hit("City/Workspace/Note/_instance.toml", 9, "Slack"),
            hit("City/ReplicatedStorage/Config.lua", 1, "xAI"),
            KeyHit::in_file("assets/meshes/car.glb", Spot::Byte(4096), "AWS", PATTERN),
        ];
        assert_eq!(
            screen_outcome(&findings(hits), Destination::LanHost),
            Err("Refused: 3 files contain what looks like keys: City/Workspace/Note/_instance.toml:3 (OpenAI), City/ReplicatedStorage/Config.lua:1 (xAI), assets/meshes/car.glb at byte offset 4096 (AWS). Remove the keys from those files, then try again.".to_string())
        );
    }

    #[test]
    fn more_than_8_files_names_8_and_counts_the_rest() {
        let hits: Vec<KeyHit> = (0..11).map(|i| hit(&format!("City/Notes/n{i}.md"), 1, "xAI")).collect();
        let named: Vec<String> = (0..8).map(|i| format!("City/Notes/n{i}.md:1 (xAI)")).collect();
        assert_eq!(
            screen_outcome(&findings(hits.clone()), Destination::Publish),
            Err(format!(
                "Refused: 11 files contain what looks like keys: {} and 3 more. Remove the keys from those files, then try again.",
                named.join(", ")
            ))
        );
        assert_eq!(
            screen_outcome(&findings(hits[..8].to_vec()), Destination::Publish),
            Err(format!("Refused: 8 files contain what looks like keys: {}. Remove the keys from those files, then try again.", named.join(", "))),
            "exactly 8 are all named, and none counted"
        );
    }

    #[test]
    fn warnings_name_8_keys_and_count_the_rest() {
        let hits: Vec<KeyHit> = (1..=11).map(|line| hit("notes.md", line, "GitHub")).collect();
        let mut expected: Vec<String> = (1..=8).map(|line| format!("notes.md:{line} contains what looks like a key for GitHub.")).collect();
        expected.push("and 3 more".to_string());
        assert_eq!(screen_outcome(&findings(hits), Destination::LoopbackHost), Ok(Screened { warnings: expected, ..Screened::default() }));
    }

    /// No refusal and no warning holds a piece of any key, for one file or many.
    #[test]
    fn no_message_holds_a_piece_of_a_key() {
        let fakes = refusing_fakes();
        let mut records: Vec<Record> = fakes
            .iter()
            .enumerate()
            .map(|(i, (_, _, key))| (format!("Workspace/Note{i}/notes.md"), format!("key = \"{key}\"\n").into_bytes()))
            .collect();
        let found = screen_records(&mut records, "City/", "City");
        assert_eq!(found.hits.len(), fakes.len(), "a fake was missed");
        for n in [1, 2, found.hits.len()] {
            let some = findings(found.hits[..n].to_vec());
            let refusal = screen_outcome(&some, Destination::Publish).err().unwrap_or_default();
            let warnings = screen_outcome(&some, Destination::LoopbackHost).map(|s| s.warnings.join("\n")).unwrap_or_default();
            assert!(!refusal.is_empty() && !warnings.is_empty(), "{n} hit(s): nothing was said");
            for (_, label, key) in &fakes {
                assert!(!holds_a_piece(&refusal, key) && !holds_a_piece(&warnings, key), "a piece of {label} reached a message");
            }
        }
    }
}

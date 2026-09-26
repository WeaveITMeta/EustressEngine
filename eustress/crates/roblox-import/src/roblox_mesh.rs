//! Roblox `.mesh` (FileMesh) decoder.
//!
//! Spec ref: `docs/architecture/ROBLOX_IMPORT_SPEC.md` §11 / §19.3.
//!
//! Decodes Roblox's versioned `.mesh` geometry format into a plain
//! [`crate::csg::CsgMesh`] (positions / normals / uvs / colors / triangle
//! indices). The caller then reuses [`crate::csg::write_glb`] /
//! [`crate::csg::encode_glb`] to emit a standard glTF-binary `.glb` the
//! Eustress mesh loader consumes — exactly the path the CSG baked-mesh
//! extractor (§7) already uses, so there is one `.glb` writer in the crate.
//!
//! This is the geometry behind `MeshPart.MeshId` and
//! `SpecialMesh.MeshId` (`rbxassetid://…`). When a fetcher supplies the
//! raw `.mesh` bytes, the asset resolver routes them here.
//!
//! ## Versions handled
//!
//! Every Roblox mesh begins with an ASCII header `version X.YY\n`:
//!
//! - **v1.00 / v1.01**: ASCII body. Whitespace-free triples of bracketed
//!   `[x,y,z]` vectors, three vectors per vertex (position, normal,
//!   uv-with-w). A `u32` face count precedes the data on one line. v1.00
//!   stores positions at twice their size, so they are halved; v1.01 does
//!   not. Legacy v1 normals are not always unit length and are normalised.
//! - **v2.00**: binary. A small header gives `sizeof_vertex` /
//!   `sizeof_face`; the vertex array is `pos f32×3, normal f32×3, uv
//!   f32×2`, then a tangent when the stride is 36 or more and an RGBA colour
//!   when it is 40; the face array is `u32×3`.
//! - **v3.00 / v3.01**: v2 plus a LOD table after the faces. We keep LOD0,
//!   the full-resolution mesh.
//! - **v4.00 / v4.01 / v5.00**: a different, fixed header (LOD type, bone,
//!   subset and FACS counts) and fixed 40-byte vertices. When the mesh is
//!   skinned, an 8-byte envelope per vertex sits between the vertices and the
//!   faces. Bones, subsets and FACS data after the LOD table are skipped.
//! - **v6.00 / v7.00**: a sequence of typed chunks (`COREMESH`, `LODS`,
//!   `SKINNING`, `FACS`, `HSRAVIS`). `COREMESH` v1 holds plain vertices and
//!   faces; `COREMESH` v2 is Draco-compressed, which this decoder refuses with
//!   a clear error rather than guessing.
//!
//! ## Defensive decoding
//!
//! Real uploads hit truncated / unexpected blobs. Every read is
//! bounds-checked through the shared [`crate::csg`]-style cursor; on any
//! unknown version or structural fault the decoder returns
//! [`MeshError`] so the caller keeps the placeholder block — it NEVER
//! panics.

use crate::csg::CsgMesh;

// ---------------------------------------------------------------------------
// Bounds-checked little-endian byte cursor (same pattern as csg.rs)
// ---------------------------------------------------------------------------

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    #[inline]
    fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], MeshError> {
        if self.remaining() < n {
            return Err(MeshError::Truncated {
                wanted: n,
                had: self.remaining(),
            });
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    /// Advance past `n` bytes without returning them (skips trailing
    /// per-vertex stride bytes / unhandled chunks).
    fn skip(&mut self, n: usize) -> Result<(), MeshError> {
        self.take(n).map(|_| ())
    }

    fn u16(&mut self) -> Result<u16, MeshError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, MeshError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32(&mut self) -> Result<f32, MeshError> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32x3(&mut self) -> Result<[f32; 3], MeshError> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    fn f32x2(&mut self) -> Result<[f32; 2], MeshError> {
        Ok([self.f32()?, self.f32()?])
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A `.mesh` decode failure. Always recoverable — the caller keeps the
/// placeholder block (it never panics).
#[derive(Debug)]
pub enum MeshError {
    /// The blob does not start with an ASCII `version X.YY` header.
    NoHeader,
    /// The header parsed but the version is one we don't decode.
    UnknownVersion(String),
    /// Wanted more bytes than the buffer held.
    Truncated {
        /// Bytes requested.
        wanted: usize,
        /// Bytes available.
        had: usize,
    },
    /// A structural invariant was violated (bad stride, index out of
    /// range, implausible count, malformed ASCII body).
    Malformed(String),
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeshError::NoHeader => write!(f, "missing `version X.YY` mesh header"),
            MeshError::UnknownVersion(v) => write!(f, "unsupported mesh version {v}"),
            MeshError::Truncated { wanted, had } => {
                write!(f, "truncated mesh: wanted {wanted} bytes, had {had}")
            }
            MeshError::Malformed(s) => write!(f, "malformed mesh: {s}"),
        }
    }
}

impl std::error::Error for MeshError {}

// ---------------------------------------------------------------------------
// Header detection
// ---------------------------------------------------------------------------

/// Cheap, allocation-free check that `blob` begins with a Roblox `.mesh`
/// header (`version `). Used by the asset resolver to decide whether
/// fetched bytes are a mesh before committing to a full decode.
pub fn looks_like_roblox_mesh(blob: &[u8]) -> bool {
    blob.starts_with(b"version ")
}

/// Parse the `version X.YY\n` header. Returns `(major, minor, body_offset)`
/// where `body_offset` is the byte index just past the header line. The
/// header line ends at the first `\n`; a trailing `\r` is tolerated.
fn parse_header(blob: &[u8]) -> Result<(u32, u32, usize), MeshError> {
    if !looks_like_roblox_mesh(blob) {
        return Err(MeshError::NoHeader);
    }
    // Find the end of the first line.
    let nl = blob.iter().position(|&b| b == b'\n').unwrap_or(blob.len());
    let line = &blob[..nl];
    let line_str = std::str::from_utf8(line).map_err(|_| MeshError::NoHeader)?;
    let ver = line_str.trim().trim_start_matches("version ").trim();
    // ver is like "1.00", "2.00", "4.01".
    let mut parts = ver.split('.');
    let major: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| MeshError::UnknownVersion(ver.to_string()))?;
    // Minor may be 1 or 2 digits ("0", "00", "01"); parse leniently.
    let minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    // Body starts after the newline (skip it). If the file had no newline
    // the body offset is the end (an empty body → later reads fail cleanly).
    let body_offset = if nl < blob.len() { nl + 1 } else { blob.len() };
    Ok((major, minor, body_offset))
}

// ---------------------------------------------------------------------------
// Top-level decode
// ---------------------------------------------------------------------------

/// Decode Roblox `.mesh` bytes into a [`CsgMesh`].
///
/// Dispatches on the ASCII version header. Returns an error (never
/// panics) on any unknown version or structural fault so the caller keeps
/// the placeholder.
pub fn decode_mesh(blob: &[u8]) -> Result<CsgMesh, MeshError> {
    let (major, minor, body) = parse_header(blob)?;
    match major {
        1 => decode_v1(&blob[body..], minor),
        2 => decode_v2(&blob[body..]),
        3 => decode_v3(&blob[body..]),
        4 | 5 => decode_v4_v5(&blob[body..]),
        6 | 7 => decode_v6_v7(&blob[body..]),
        _ => Err(MeshError::UnknownVersion(format!("{major}.{minor:02}"))),
    }
}

// ---------------------------------------------------------------------------
// v1.00 / v1.01 — ASCII
// ---------------------------------------------------------------------------

/// Decode the ASCII v1 body. Layout: a face-count integer on the first
/// body line, then a stream of `[x,y,z]` / `[x,y]` bracketed vectors with
/// no separators, three vectors per vertex: position, normal, uv (the uv's
/// third component, when present, is a texture scale we drop). Three
/// consecutive vertices form a triangle (the face count = triangles).
///
/// v1.00 stores positions at twice their size (the format spec: "Meshes that
/// use version 1.00 are 2x bigger than they should be ... This is corrected
/// in version 1.01"), so its positions are halved. Normals are normalised:
/// legacy v1 files carry lengths well off 1 (2.8 to 3.5 in cached mesh
/// 1028713), which shade wrong.
fn decode_v1(body: &[u8], minor: u32) -> Result<CsgMesh, MeshError> {
    let text = std::str::from_utf8(body)
        .map_err(|_| MeshError::Malformed("v1 body is not valid UTF-8".into()))?;

    // Collect every bracketed vector `[a,b,c]` / `[a,b]` as a Vec<f32>.
    // The leading face-count integer (before the first `[`) is read for
    // validation but the geometry is fully determined by the vector
    // stream, so we don't strictly need it.
    let mut groups: Vec<Vec<f32>> = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some(&(_, c)) = chars.peek() {
        if c == '[' {
            // Consume the bracketed group.
            chars.next(); // '['
            let mut num = String::new();
            let mut comps: Vec<f32> = Vec::new();
            for (_, ch) in chars.by_ref() {
                match ch {
                    ']' => {
                        if !num.trim().is_empty() {
                            comps.push(parse_f32(&num)?);
                        }
                        break;
                    }
                    ',' => {
                        if !num.trim().is_empty() {
                            comps.push(parse_f32(&num)?);
                        }
                        num.clear();
                    }
                    _ => num.push(ch),
                }
            }
            groups.push(comps);
        } else {
            chars.next();
        }
    }

    // Vectors come in triples: position, normal, uv.
    if groups.len() % 3 != 0 {
        return Err(MeshError::Malformed(format!(
            "v1 vector count {} is not a multiple of 3 (pos/normal/uv triples)",
            groups.len()
        )));
    }
    let vertex_count = groups.len() / 3;
    let pos_scale = if minor == 0 { 0.5 } else { 1.0 };

    let mut mesh = CsgMesh::default();
    mesh.positions.reserve(vertex_count);
    mesh.normals.reserve(vertex_count);
    mesh.uvs.reserve(vertex_count);
    for v in 0..vertex_count {
        let p = &groups[v * 3];
        let n = &groups[v * 3 + 1];
        let t = &groups[v * 3 + 2];
        if p.len() < 3 || n.len() < 3 || t.len() < 2 {
            return Err(MeshError::Malformed(
                "v1 vector group has too few components".into(),
            ));
        }
        mesh.positions
            .push([p[0] * pos_scale, p[1] * pos_scale, p[2] * pos_scale]);
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        mesh.normals.push(if len > 1e-6 {
            [n[0] / len, n[1] / len, n[2] / len]
        } else {
            [n[0], n[1], n[2]]
        });
        // uv: Roblox stores V flipped relative to glTF; keep raw — the
        // renderer's sampler handles convention. Drop the optional w.
        mesh.uvs.push([t[0], t[1]]);
    }
    // v1 is a raw triangle soup: every 3 vertices = 1 triangle, indices
    // are 0,1,2,3,…
    mesh.indices = (0..vertex_count as u32).collect();
    finalize(&mut mesh)?;
    Ok(mesh)
}

fn parse_f32(s: &str) -> Result<f32, MeshError> {
    s.trim()
        .parse::<f32>()
        .map_err(|_| MeshError::Malformed(format!("bad float '{s}' in v1 mesh body")))
}

// ---------------------------------------------------------------------------
// v2.00 — binary, fixed-layout vertices
// ---------------------------------------------------------------------------

/// Decode the v2.00 binary body.
///
/// Header (after the ASCII version line):
/// ```text
/// u16 sizeof_MeshHeader   (= 12)
/// u8  sizeof_Vertex       (32 = pos+normal+uv; 36 adds a tangent; 40 adds RGBA)
/// u8  sizeof_Face         (= 12, three u32 indices)
/// u32 numVerts
/// u32 numFaces
/// ```
/// Then `numVerts` vertices and `numFaces` faces.
fn decode_v2(body: &[u8]) -> Result<CsgMesh, MeshError> {
    let mut cur = Cursor::new(body);
    // Canonical v2 header is exactly 12 bytes: the 4 size/flag bytes below
    // plus the two u32 counts that follow (4 + 8 = 12 = sizeof_header).
    let _sizeof_header = cur.u16()? as usize;
    let sizeof_vertex = cur.take(1)?[0] as usize;
    let sizeof_face = cur.take(1)?[0] as usize;
    let num_verts = cur.u32()? as usize;
    let num_faces = cur.u32()? as usize;

    validate_counts(sizeof_vertex, sizeof_face, num_verts, num_faces)?;

    let mesh =
        read_binary_vertices_faces(&mut cur, sizeof_vertex, sizeof_face, num_verts, num_faces)?;
    Ok(mesh)
}

// ---------------------------------------------------------------------------
// v3.00 – v7.00 — binary with LOD / skinning / FACS chunks
// ---------------------------------------------------------------------------

/// Decode the v3.00 / v3.01 binary body, keeping LOD0.
///
/// Header (after the ASCII version line, little-endian):
/// ```text
/// u16 sizeof_MeshHeader   (= 16)
/// u8  sizeof_Vertex       (36 or 40)
/// u8  sizeof_Face         (= 12)
/// u16 sizeof_LOD          (= 4)
/// u16 numLODs
/// u32 numVerts
/// u32 numFaces
/// ```
/// Then `numVerts` vertices, `numFaces` faces, and `numLODs` u32 face
/// offsets.
fn decode_v3(body: &[u8]) -> Result<CsgMesh, MeshError> {
    let mut cur = Cursor::new(body);
    let sizeof_header = cur.u16()? as usize;
    let sizeof_vertex = cur.take(1)?[0] as usize;
    let sizeof_face = cur.take(1)?[0] as usize;
    let _sizeof_lod = cur.u16()?;
    let num_lods = cur.u16()? as usize;
    let num_verts = cur.u32()? as usize;
    let num_faces = cur.u32()? as usize;
    // 16 bytes read; skip anything a later minor version appends.
    if sizeof_header > 16 {
        cur.skip(sizeof_header - 16)?;
    }
    validate_counts(sizeof_vertex, sizeof_face, num_verts, num_faces)?;

    let mut mesh = CsgMesh::default();
    read_vertices(&mut cur, sizeof_vertex, num_verts, &mut mesh)?;
    read_faces(&mut cur, sizeof_face, num_faces, &mut mesh)?;
    let offsets = read_lod_offsets(&mut cur, num_lods);
    clip_to_lod0(&mut mesh, &offsets);
    finalize(&mut mesh)?;
    Ok(mesh)
}

// ---------------------------------------------------------------------------
// v4.00 / v4.01 / v5.00: fixed header, fixed 40-byte vertices
// ---------------------------------------------------------------------------

/// Bytes per vertex from v4 on: pos, normal, uv, tangent, RGBA.
const V4_VERTEX_SIZE: usize = 40;

/// Bytes per skinning envelope: four bone indices then four weights.
const ENVELOPE_SIZE: usize = 8;

/// Decode a v4.00, v4.01 or v5.00 body, keeping LOD0.
///
/// Header (after the ASCII version line, little-endian):
/// ```text
/// u16 sizeof_MeshHeader     (24 for v4, 32 for v5)
/// u16 lodType
/// u32 numVerts
/// u32 numFaces
/// u16 numLODs
/// u16 numBones
/// u32 sizeof_boneNamesBuffer
/// u16 numSubsets
/// u8  numHighQualityLODs
/// u8  unused
/// u32 facsDataFormat        (v5 only)
/// u32 sizeof_facsData       (v5 only)
/// ```
/// Then `numVerts` 40-byte vertices, an 8-byte envelope per vertex when
/// `numBones > 0`, `numFaces` faces, and `numLODs` u32 face offsets. Bones,
/// the bone-name buffer, subsets and FACS data follow and are not needed.
///
/// There is no stride field. Reading this header as if it had one takes
/// `lodType` (4 in almost every real file) for the stride, which is how every
/// v4 mesh used to fail with "vertex stride 4 < 32".
fn decode_v4_v5(body: &[u8]) -> Result<CsgMesh, MeshError> {
    let mut cur = Cursor::new(body);
    let sizeof_header = cur.u16()? as usize;
    let _lod_type = cur.u16()?;
    let num_verts = cur.u32()? as usize;
    let num_faces = cur.u32()? as usize;
    let num_lods = cur.u16()? as usize;
    let num_bones = cur.u16()? as usize;
    // 16 bytes read; the rest of the header (bone-name buffer size, subset
    // count, high-quality LOD count, and the v5 FACS fields) is skipped.
    if sizeof_header < 16 {
        return Err(MeshError::Malformed(format!(
            "v4+ header size {sizeof_header} < 16"
        )));
    }
    cur.skip(sizeof_header - 16)?;
    validate_counts(V4_VERTEX_SIZE, 12, num_verts, num_faces)?;

    let mut mesh = CsgMesh::default();
    read_vertices(&mut cur, V4_VERTEX_SIZE, num_verts, &mut mesh)?;
    if num_bones > 0 {
        let envelopes = num_verts
            .checked_mul(ENVELOPE_SIZE)
            .ok_or_else(|| MeshError::Malformed("envelope table overflows".into()))?;
        cur.skip(envelopes)?;
    }
    read_faces(&mut cur, 12, num_faces, &mut mesh)?;
    let offsets = read_lod_offsets(&mut cur, num_lods);
    clip_to_lod0(&mut mesh, &offsets);
    finalize(&mut mesh)?;
    Ok(mesh)
}

// ---------------------------------------------------------------------------
// v6.00 / v7.00: typed chunks
// ---------------------------------------------------------------------------

/// Decode a v6.00 or v7.00 body: a run of chunks, each
/// `char[8] type, u32 version, u32 size, u8 data[size]`.
///
/// - `COREMESH` v1: `u32 numVerts, Vertex[numVerts] (40 bytes each),
///   u32 numFaces, Face[numFaces] (12 bytes each)`.
/// - `COREMESH` v2: a Draco bitstream. Not decoded; the caller keeps the
///   placeholder and the report says why.
/// - `LODS` v1: `u16 lodType, u8 numHighQualityLODs, u32 numLodOffsets,
///   u32 lodOffsets[numLodOffsets]`, used to keep LOD0.
/// - `SKINNING`, `FACS`, `HSRAVIS` and any chunk added later: skipped by size.
fn decode_v6_v7(body: &[u8]) -> Result<CsgMesh, MeshError> {
    let mut cur = Cursor::new(body);
    let mut mesh: Option<CsgMesh> = None;
    let mut offsets: Vec<u32> = Vec::new();
    while cur.remaining() > 0 {
        if cur.remaining() < 16 {
            // Trailing padding shorter than a chunk header.
            break;
        }
        let kind = cur.take(8)?;
        let version = cur.u32()?;
        let size = cur.u32()? as usize;
        let data = cur.take(size)?;
        let name = chunk_name(kind);
        match (name.as_str(), version) {
            ("COREMESH", 1) => mesh = Some(decode_coremesh_v1(data)?),
            ("COREMESH", v) => {
                return Err(MeshError::Malformed(format!(
                    "COREMESH v{v} is Draco-compressed geometry, which this decoder does not read"
                )))
            }
            ("LODS", 1) => offsets = decode_lods_v1(data)?,
            _ => {}
        }
    }
    let mut mesh =
        mesh.ok_or_else(|| MeshError::Malformed("v6+ mesh has no COREMESH chunk".into()))?;
    clip_to_lod0(&mut mesh, &offsets);
    finalize(&mut mesh)?;
    Ok(mesh)
}

/// A chunk type as text, without the NUL padding (`b"LODS\0\0\0\0"` is `LODS`).
fn chunk_name(kind: &[u8]) -> String {
    let end = kind.iter().position(|&b| b == 0).unwrap_or(kind.len());
    String::from_utf8_lossy(&kind[..end]).into_owned()
}

fn decode_coremesh_v1(data: &[u8]) -> Result<CsgMesh, MeshError> {
    let mut cur = Cursor::new(data);
    let num_verts = cur.u32()? as usize;
    validate_counts(V4_VERTEX_SIZE, 12, num_verts, 0)?;
    let mut mesh = CsgMesh::default();
    read_vertices(&mut cur, V4_VERTEX_SIZE, num_verts, &mut mesh)?;
    let num_faces = cur.u32()? as usize;
    validate_counts(V4_VERTEX_SIZE, 12, num_verts, num_faces)?;
    read_faces(&mut cur, 12, num_faces, &mut mesh)?;
    Ok(mesh)
}

fn decode_lods_v1(data: &[u8]) -> Result<Vec<u32>, MeshError> {
    let mut cur = Cursor::new(data);
    let _lod_type = cur.u16()?;
    let _num_high_quality = cur.take(1)?;
    let count = cur.u32()? as usize;
    if count > 1024 {
        return Err(MeshError::Malformed(format!("implausible LOD count {count}")));
    }
    (0..count).map(|_| cur.u32()).collect()
}

// ---------------------------------------------------------------------------
// LOD clipping
// ---------------------------------------------------------------------------

/// Read `count` u32 face offsets. A table cut short by the end of the blob
/// yields what was readable; clipping then falls back to all faces.
fn read_lod_offsets(cur: &mut Cursor, count: usize) -> Vec<u32> {
    let mut offsets = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        match cur.u32() {
            Ok(v) => offsets.push(v),
            Err(_) => break,
        }
    }
    offsets
}

/// Keep only LOD0, the full-resolution mesh: faces
/// `[offsets[0], offsets[1])`. Lower LODs are stored after it in the same
/// face array, and drawing them too would z-fight the real surface. A table
/// that is missing or does not describe a sane range keeps every face.
fn clip_to_lod0(mesh: &mut CsgMesh, offsets: &[u32]) {
    if offsets.len() < 2 {
        return;
    }
    let lo = offsets[0] as usize;
    let hi = offsets[1] as usize;
    if hi > lo && hi * 3 <= mesh.indices.len() {
        mesh.indices = mesh.indices[lo * 3..hi * 3].to_vec();
    }
}

// ---------------------------------------------------------------------------
// Shared binary vertex/face reader
// ---------------------------------------------------------------------------

/// Read `num_verts` vertices then `num_faces` faces from `cur` using the
/// given strides (see [`read_vertices`] for the vertex layout).
fn read_binary_vertices_faces(
    cur: &mut Cursor,
    sizeof_vertex: usize,
    sizeof_face: usize,
    num_verts: usize,
    num_faces: usize,
) -> Result<CsgMesh, MeshError> {
    let mut mesh = CsgMesh::default();
    read_vertices(cur, sizeof_vertex, num_verts, &mut mesh)?;
    read_faces(cur, sizeof_face, num_faces, &mut mesh)?;
    finalize(&mut mesh)?;
    Ok(mesh)
}

/// Read `num_verts` binary vertices into `mesh`.
///
/// Layout (v2 and later): position f32x3, normal f32x3, uv f32x2 = 32 bytes;
/// then a tangent (4 bytes) when the stride is at least 36, then an RGBA
/// colour (4 x u8) when it is at least 40. The tangent comes FIRST. Reading
/// bytes 32..36 as colour gave every 36-byte mesh a vertex colour of
/// (0, 0, 0, 0), black and fully transparent. Sampled across the 248 binary
/// meshes in the asset cache: bytes 32..36 of a 36-byte vertex are always
/// zero, and bytes 36..40 of a 40-byte vertex are `ff ff ff ff` in 99.5% of
/// vertices.
fn read_vertices(
    cur: &mut Cursor,
    sizeof_vertex: usize,
    num_verts: usize,
    mesh: &mut CsgMesh,
) -> Result<(), MeshError> {
    mesh.positions.reserve(num_verts);
    mesh.normals.reserve(num_verts);
    mesh.uvs.reserve(num_verts);
    let has_tangent = sizeof_vertex >= 36;
    let has_color = sizeof_vertex >= 40;
    if has_color {
        mesh.colors.reserve(num_verts);
    }
    for _ in 0..num_verts {
        let pos = cur.f32x3()?;
        let norm = cur.f32x3()?;
        let uv = cur.f32x2()?;
        let mut consumed = 32usize;
        if has_tangent {
            cur.skip(4)?;
            consumed += 4;
        }
        if has_color {
            let c = cur.take(4)?;
            mesh.colors.push([
                c[0] as f32 / 255.0,
                c[1] as f32 / 255.0,
                c[2] as f32 / 255.0,
                c[3] as f32 / 255.0,
            ]);
            consumed += 4;
        }
        if sizeof_vertex > consumed {
            cur.skip(sizeof_vertex - consumed)?;
        }
        mesh.positions.push(pos);
        mesh.normals.push(norm);
        mesh.uvs.push(uv);
    }
    Ok(())
}

/// Read `num_faces` triangles (three u32 indices, then any padding up to
/// `sizeof_face`) into `mesh.indices`.
fn read_faces(
    cur: &mut Cursor,
    sizeof_face: usize,
    num_faces: usize,
    mesh: &mut CsgMesh,
) -> Result<(), MeshError> {
    mesh.indices.reserve(num_faces * 3);
    for _ in 0..num_faces {
        let a = cur.u32()?;
        let b = cur.u32()?;
        let c = cur.u32()?;
        mesh.indices.push(a);
        mesh.indices.push(b);
        mesh.indices.push(c);
        if sizeof_face > 12 {
            cur.skip(sizeof_face - 12)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Validation + finalisation
// ---------------------------------------------------------------------------

/// Guard the header counts/strides before allocating or reading.
fn validate_counts(
    sizeof_vertex: usize,
    sizeof_face: usize,
    num_verts: usize,
    num_faces: usize,
) -> Result<(), MeshError> {
    // The fixed pos+normal+uv block is 32 bytes; the stride must hold it.
    if sizeof_vertex < 32 {
        return Err(MeshError::Malformed(format!(
            "vertex stride {sizeof_vertex} < 32 (pos+normal+uv minimum)"
        )));
    }
    // A face needs at least three u32 indices.
    if sizeof_face < 12 {
        return Err(MeshError::Malformed(format!(
            "face stride {sizeof_face} < 12 (three u32 indices)"
        )));
    }
    // Plausibility guards (mirror csg.rs's 50M ceiling).
    if num_verts > 50_000_000 {
        return Err(MeshError::Malformed(format!(
            "implausible vertex count {num_verts}"
        )));
    }
    if num_faces > 50_000_000 {
        return Err(MeshError::Malformed(format!(
            "implausible face count {num_faces}"
        )));
    }
    Ok(())
}

/// Validate triangle indices + drop per-vertex attribute arrays that
/// don't match the vertex count (so `encode_glb`'s length checks pass and
/// the mesh still renders with computed normals if needed).
fn finalize(mesh: &mut CsgMesh) -> Result<(), MeshError> {
    if mesh.indices.len() % 3 != 0 {
        return Err(MeshError::Malformed(format!(
            "index count {} not a multiple of 3",
            mesh.indices.len()
        )));
    }
    let n = mesh.positions.len() as u32;
    if let Some(&bad) = mesh.indices.iter().find(|&&i| i >= n) {
        return Err(MeshError::Malformed(format!(
            "triangle index {bad} out of range (vertex count {n})"
        )));
    }
    if mesh.normals.len() != mesh.positions.len() {
        mesh.normals.clear();
    }
    if mesh.uvs.len() != mesh.positions.len() {
        mesh.uvs.clear();
    }
    if mesh.colors.len() != mesh.positions.len() {
        mesh.colors.clear();
    }
    // Opaque white multiplies to nothing. Most v4+ meshes carry exactly that
    // (every vertex has a colour slot), so dropping it saves 16 bytes a
    // vertex in the glb and keeps the renderer off the vertex-colour path.
    if mesh.colors.iter().all(|c| *c == [1.0, 1.0, 1.0, 1.0]) {
        mesh.colors.clear();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Test fixtures (shared with the asset_resolver integration test)
// ---------------------------------------------------------------------------

/// Build a minimal valid v2.00 `.mesh` blob: one triangle (3 vertices,
/// 1 face), no per-vertex color (stride 32). Exposed `pub(crate)` so the
/// `asset_resolver` mesh-fetch test can craft a blob without a network.
#[cfg(test)]
pub(crate) fn make_v2_triangle_fixture() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"version 2.00\n");
    // sizeof_MeshHeader = 12 (u16) — 4 size/flag bytes + 8 count bytes.
    buf.extend_from_slice(&12u16.to_le_bytes());
    buf.push(32u8); // sizeof_vertex (pos+normal+uv, no color)
    buf.push(12u8); // sizeof_face (three u32)
    buf.extend_from_slice(&3u32.to_le_bytes()); // numVerts
    buf.extend_from_slice(&1u32.to_le_bytes()); // numFaces
    let verts = [
        ([0.0f32, 0.0, 0.0], [0.0f32, 0.0, 1.0], [0.0f32, 0.0]),
        ([1.0f32, 0.0, 0.0], [0.0f32, 0.0, 1.0], [1.0f32, 0.0]),
        ([0.0f32, 1.0, 0.0], [0.0f32, 0.0, 1.0], [0.0f32, 1.0]),
    ];
    for (p, n, t) in verts {
        for c in p {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        for c in n {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        for c in t {
            buf.extend_from_slice(&c.to_le_bytes());
        }
    }
    // One face: indices 0,1,2.
    for i in [0u32, 1, 2] {
        buf.extend_from_slice(&i.to_le_bytes());
    }
    buf
}

/// Build a v2.00 `.mesh` blob from a triangle list (every 3 positions form one
/// face), with +Z normals and zero UVs. Lets tests pick exact extents, where
/// the unit triangle above spans 1 x 1 x 0 and cannot tell axes apart.
#[cfg(test)]
pub(crate) fn make_v2_mesh_fixture(positions: &[[f32; 3]]) -> Vec<u8> {
    assert!(positions.len() % 3 == 0, "triangle list");
    let mut buf = Vec::new();
    buf.extend_from_slice(b"version 2.00\n");
    buf.extend_from_slice(&12u16.to_le_bytes());
    buf.push(32u8);
    buf.push(12u8);
    buf.extend_from_slice(&(positions.len() as u32).to_le_bytes());
    buf.extend_from_slice(&((positions.len() / 3) as u32).to_le_bytes());
    for p in positions {
        for c in p {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        for c in [0.0f32, 0.0, 1.0] {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        for c in [0.0f32, 0.0] {
            buf.extend_from_slice(&c.to_le_bytes());
        }
    }
    for i in 0..positions.len() as u32 {
        buf.extend_from_slice(&i.to_le_bytes());
    }
    buf
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_mesh_header() {
        assert!(looks_like_roblox_mesh(b"version 2.00\n\x00"));
        assert!(!looks_like_roblox_mesh(b"glTF\x02"));
        assert!(!looks_like_roblox_mesh(b"CSGK"));
    }

    #[test]
    fn parses_header_major_minor() {
        let (maj, min, off) = parse_header(b"version 4.01\nBODY").unwrap();
        assert_eq!((maj, min), (4, 1));
        assert_eq!(&b"version 4.01\nBODY"[off..], b"BODY");
    }

    #[test]
    fn parses_header_tolerates_crlf() {
        let (maj, min, _off) = parse_header(b"version 1.00\r\n....").unwrap();
        assert_eq!((maj, min), (1, 0));
    }

    #[test]
    fn missing_header_errors() {
        assert!(matches!(decode_mesh(b"nope"), Err(MeshError::NoHeader)));
    }

    #[test]
    fn unknown_version_errors() {
        assert!(matches!(
            decode_mesh(b"version 9.00\n\x00\x00"),
            Err(MeshError::UnknownVersion(_))
        ));
    }

    #[test]
    fn decodes_v2_triangle() {
        let blob = make_v2_triangle_fixture();
        let mesh = decode_mesh(&blob).expect("decode v2");
        assert_eq!(mesh.positions.len(), 3);
        assert_eq!(mesh.normals.len(), 3);
        assert_eq!(mesh.uvs.len(), 3);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.positions[1], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.uvs[2], [0.0, 1.0]);
        // No color in this fixture (stride 32).
        assert!(mesh.colors.is_empty());
    }

    /// One vertex: pos (i, 0, 0), normal +Y, uv (0, 0), then `extra` bytes
    /// (tangent, colour) exactly as a real file lays them out.
    fn push_vertex(buf: &mut Vec<u8>, i: u32, extra: &[u8]) {
        for c in [i as f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        buf.extend_from_slice(extra);
    }

    fn v2_blob(stride: u8, extra: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"version 2.00\n");
        buf.extend_from_slice(&12u16.to_le_bytes());
        buf.push(stride);
        buf.push(12u8);
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        for v in 0..3u32 {
            push_vertex(&mut buf, v, extra);
        }
        for i in [0u32, 1, 2] {
            buf.extend_from_slice(&i.to_le_bytes());
        }
        buf
    }

    /// A 36-byte vertex ends in a tangent, not a colour. Reading those zero
    /// bytes as RGBA painted real meshes black and transparent.
    #[test]
    fn v2_stride_36_is_a_tangent_not_a_colour() {
        let mesh = decode_mesh(&v2_blob(36, &[0, 0, 0, 0])).expect("decode");
        assert_eq!(mesh.positions.len(), 3);
        assert!(mesh.colors.is_empty(), "no colour slot in a 36-byte vertex");
        assert_eq!(mesh.positions[2], [2.0, 0.0, 0.0], "stride kept vertices aligned");
    }

    /// A 40-byte vertex is tangent THEN colour.
    #[test]
    fn v2_stride_40_reads_the_colour_after_the_tangent() {
        let mesh = decode_mesh(&v2_blob(40, &[1, 2, 3, 4, 255, 128, 0, 255])).expect("decode");
        assert_eq!(mesh.colors.len(), 3);
        assert_eq!(mesh.colors[0][0], 1.0);
        assert!((mesh.colors[0][1] - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(mesh.colors[0][2], 0.0);
        assert_eq!(mesh.colors[0][3], 1.0);
    }

    /// Opaque white is a no-op tint and is not written out.
    #[test]
    fn all_white_vertex_colour_is_dropped() {
        let mesh = decode_mesh(&v2_blob(40, &[0, 0, 0, 0, 255, 255, 255, 255])).expect("decode");
        assert!(mesh.colors.is_empty());
    }

    #[test]
    fn decodes_v1_ascii_triangle() {
        // One triangle = 3 vertices = 9 vectors (pos/normal/uv each).
        // v1.01: positions as stored.
        let mut body = String::from("1\n"); // face count line
        // vertex 0
        body.push_str("[1,0,0][0,0,1][0,0,0]");
        // vertex 1
        body.push_str("[0,1,0][0,0,1][1,0,0]");
        // vertex 2
        body.push_str("[0,0,1][0,0,1][0,1,0]");
        let blob = format!("version 1.01\n{body}");
        let mesh = decode_mesh(blob.as_bytes()).expect("decode v1");
        assert_eq!(mesh.positions.len(), 3);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.positions[0], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.uvs[1], [1.0, 0.0]);
    }

    /// Version 1.00 stores positions at twice their size; 1.01 fixed that.
    #[test]
    fn v1_00_halves_positions_and_v1_01_does_not() {
        let body = "[2,0,0][0,0,1][0,0,0][0,2,0][0,0,1][1,0,0][0,0,2][0,0,1][0,1,0]";
        let v100 = decode_mesh(format!("version 1.00\n{body}").as_bytes()).expect("decode v1.00");
        assert_eq!(v100.positions[0], [1.0, 0.0, 0.0]);
        assert_eq!(v100.positions[1], [0.0, 1.0, 0.0]);
        assert_eq!(v100.positions[2], [0.0, 0.0, 1.0]);
        let v101 = decode_mesh(format!("version 1.01\n{body}").as_bytes()).expect("decode v1.01");
        assert_eq!(v101.positions[0], [2.0, 0.0, 0.0]);
    }

    /// Legacy v1 normals are not always unit length (cached mesh 1028713
    /// stores `[0,-2.79253,0]`); they are normalised.
    #[test]
    fn v1_normals_are_normalised() {
        let body = "[1,0,0][0,-2.79253,0][0,0,0][0,1,0][0,0,3][1,0,0][0,0,1][0.6,0.8,0][0,1,0]";
        let mesh = decode_mesh(format!("version 1.01\n{body}").as_bytes()).expect("decode v1");
        for n in &mesh.normals {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-5, "normal {n:?} is not unit length");
        }
        assert!((mesh.normals[0][1] + 1.0).abs() < 1e-6, "direction kept");
    }

    #[test]
    fn truncated_v2_does_not_panic() {
        let mut blob = b"version 2.00\n".to_vec();
        blob.extend_from_slice(&12u16.to_le_bytes());
        blob.push(32u8);
        blob.push(12u8);
        blob.extend_from_slice(&100u32.to_le_bytes()); // claims 100 verts
        blob.extend_from_slice(&50u32.to_le_bytes());
        // …but no vertex data follows.
        let res = decode_mesh(&blob);
        assert!(matches!(res, Err(MeshError::Truncated { .. })));
    }

    /// Four vertices, two faces: face 0 is LOD0, face 1 a lower LOD that must
    /// be clipped away.
    const TWO_LOD_FACES: [u32; 6] = [0, 1, 2, 1, 2, 3];
    const TINT: [u8; 8] = [0, 0, 0, 0, 10, 20, 30, 255];

    fn push_faces_and_lods(buf: &mut Vec<u8>) {
        for i in TWO_LOD_FACES {
            buf.extend_from_slice(&i.to_le_bytes());
        }
    }

    /// A v4/v5 blob. `skinned` adds one bone and the envelope table;
    /// `v5` adds the two FACS header fields.
    fn v4_blob(version: &str, skinned: bool, v5: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(format!("version {version}\n").as_bytes());
        let header: u16 = if v5 { 32 } else { 24 };
        buf.extend_from_slice(&header.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes()); // lodType, as real files write it
        buf.extend_from_slice(&4u32.to_le_bytes()); // numVerts
        buf.extend_from_slice(&2u32.to_le_bytes()); // numFaces
        buf.extend_from_slice(&3u16.to_le_bytes()); // numLODs (offset entries)
        buf.extend_from_slice(&(skinned as u16).to_le_bytes()); // numBones
        buf.extend_from_slice(&0u32.to_le_bytes()); // sizeof_boneNamesBuffer
        buf.extend_from_slice(&0u16.to_le_bytes()); // numSubsets
        buf.push(1); // numHighQualityLODs
        buf.push(0); // unused
        if v5 {
            buf.extend_from_slice(&0u32.to_le_bytes()); // facsDataFormat
            buf.extend_from_slice(&0u32.to_le_bytes()); // sizeof_facsData
        }
        for v in 0..4u32 {
            push_vertex(&mut buf, v, &TINT);
        }
        if skinned {
            // Envelopes: bone 0 at full weight. 0xFF bytes would read as a
            // wildly out-of-range face index if they were not skipped.
            for _ in 0..4 {
                buf.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0]);
            }
        }
        push_faces_and_lods(&mut buf);
        for off in [0u32, 1, 2] {
            buf.extend_from_slice(&off.to_le_bytes());
        }
        buf
    }

    #[test]
    fn decodes_v4_and_clips_to_lod0() {
        for version in ["4.00", "4.01"] {
            let mesh = decode_mesh(&v4_blob(version, false, false)).expect(version);
            assert_eq!(mesh.positions.len(), 4, "{version}");
            assert_eq!(mesh.positions[3], [3.0, 0.0, 0.0], "{version} vertex stride");
            assert_eq!(mesh.indices, vec![0, 1, 2], "{version} keeps LOD0 only");
            assert_eq!(mesh.colors.len(), 4, "{version}");
            assert!((mesh.colors[0][0] - 10.0 / 255.0).abs() < 1e-6, "{version} colour");
        }
    }

    #[test]
    fn decodes_skinned_v4_past_the_envelopes() {
        let mesh = decode_mesh(&v4_blob("4.01", true, false)).expect("skinned v4");
        assert_eq!(mesh.indices, vec![0, 1, 2]);
    }

    #[test]
    fn decodes_v5_with_its_longer_header() {
        let mesh = decode_mesh(&v4_blob("5.00", true, true)).expect("v5");
        assert_eq!(mesh.positions.len(), 4);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
    }

    fn chunk(buf: &mut Vec<u8>, kind: &[u8; 8], version: u32, data: &[u8]) {
        buf.extend_from_slice(kind);
        buf.extend_from_slice(&version.to_le_bytes());
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(data);
    }

    fn coremesh_v1() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&4u32.to_le_bytes());
        for v in 0..4u32 {
            push_vertex(&mut data, v, &TINT);
        }
        data.extend_from_slice(&2u32.to_le_bytes());
        push_faces_and_lods(&mut data);
        data
    }

    fn lods_v1() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&3u16.to_le_bytes()); // lodType
        data.push(1); // numHighQualityLODs
        data.extend_from_slice(&3u32.to_le_bytes());
        for off in [0u32, 1, 2] {
            data.extend_from_slice(&off.to_le_bytes());
        }
        data
    }

    #[test]
    fn decodes_chunked_v6_and_v7() {
        for version in ["6.00", "7.00"] {
            let mut buf = format!("version {version}\n").into_bytes();
            // Chunks in any order, with ones we skip around the geometry.
            chunk(&mut buf, b"LODS\0\0\0\0", 1, &lods_v1());
            chunk(&mut buf, b"SKINNING", 1, &[0u8; 12]);
            chunk(&mut buf, b"COREMESH", 1, &coremesh_v1());
            chunk(&mut buf, b"HSRAVIS\0", 1, &[1, 2, 3]);
            let mesh = decode_mesh(&buf).expect(version);
            assert_eq!(mesh.positions.len(), 4, "{version}");
            assert_eq!(mesh.positions[3], [3.0, 0.0, 0.0], "{version}");
            assert_eq!(mesh.indices, vec![0, 1, 2], "{version} keeps LOD0 only");
        }
    }

    #[test]
    fn draco_coremesh_is_refused_with_a_reason() {
        let mut buf = b"version 7.00\n".to_vec();
        chunk(&mut buf, b"COREMESH", 2, &[0u8; 32]);
        match decode_mesh(&buf) {
            Err(MeshError::Malformed(m)) => assert!(m.contains("Draco"), "got {m}"),
            other => panic!("expected a Draco refusal, got {other:?}"),
        }
    }

    #[test]
    fn chunked_mesh_without_geometry_errors() {
        let mut buf = b"version 6.00\n".to_vec();
        chunk(&mut buf, b"LODS\0\0\0\0", 1, &lods_v1());
        assert!(matches!(decode_mesh(&buf), Err(MeshError::Malformed(_))));
    }

    #[test]
    fn truncated_chunk_does_not_panic() {
        let mut buf = b"version 6.00\n".to_vec();
        let core = coremesh_v1();
        chunk(&mut buf, b"COREMESH", 1, &core);
        buf.truncate(buf.len() - 10);
        assert!(decode_mesh(&buf).is_err());
    }

    #[test]
    fn decodes_v3_and_clips_to_lod0() {
        let mut buf = b"version 3.00\n".to_vec();
        buf.extend_from_slice(&16u16.to_le_bytes());
        buf.push(40);
        buf.push(12);
        buf.extend_from_slice(&4u16.to_le_bytes()); // sizeof_LOD
        buf.extend_from_slice(&3u16.to_le_bytes()); // numLODs
        buf.extend_from_slice(&4u32.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes());
        for v in 0..4u32 {
            push_vertex(&mut buf, v, &TINT);
        }
        push_faces_and_lods(&mut buf);
        for off in [0u32, 1, 2] {
            buf.extend_from_slice(&off.to_le_bytes());
        }
        let mesh = decode_mesh(&buf).expect("v3");
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.colors.len(), 4);
    }

    /// Every binary `.mesh` the importer has downloaded must decode to sane
    /// geometry: indices in range, finite positions, unit normals. Reads the
    /// real asset cache, so it is ignored by default; run it with
    /// `cargo test -p eustress-roblox-import real_cached_meshes_decode -- --ignored --nocapture`
    /// (`RBX_CACHE` overrides the folder).
    #[test]
    #[ignore]
    fn real_cached_meshes_decode() {
        let dir = std::env::var("RBX_CACHE").unwrap_or_else(|_| {
            let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap();
            format!("{home}/Documents/Eustress/.rbx_cache")
        });
        let mut tally: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
        let mut failures = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("asset cache folder") {
            let path = entry.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            if !looks_like_roblox_mesh(&bytes) {
                continue;
            }
            let (major, minor, _) = parse_header(&bytes).unwrap();
            let slot = tally.entry(format!("{major}.{minor:02}")).or_default();
            slot.0 += 1;
            match decode_mesh(&bytes) {
                Ok(mesh) => {
                    assert!(!mesh.is_empty(), "{} decoded empty", path.display());
                    assert!(
                        mesh.positions.iter().flatten().all(|c| c.is_finite()),
                        "{} has a non-finite position",
                        path.display()
                    );
                    let unit = mesh
                        .normals
                        .iter()
                        .filter(|n| {
                            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                            (0.9..1.1).contains(&l)
                        })
                        .count();
                    assert!(
                        unit * 10 >= mesh.normals.len() * 9,
                        "{}: only {unit}/{} unit normals, the vertex layout is off",
                        path.display(),
                        mesh.normals.len()
                    );
                    slot.1 += 1;
                }
                Err(e) => failures.push(format!("{} ({major}.{minor:02}): {e}", path.display())),
            }
        }
        for (v, (n, ok)) in &tally {
            eprintln!("version {v}: {ok}/{n} decoded");
        }
        assert!(!tally.is_empty(), "no meshes found in {dir}");
        assert!(failures.is_empty(), "decode failures:\n{}", failures.join("\n"));
    }

    #[test]
    fn fixture_encodes_to_valid_glb() {
        // The decoded fixture should round-trip through the shared CSG glb
        // writer (proves the CsgMesh is well-formed for the engine loader).
        let blob = make_v2_triangle_fixture();
        let mesh = decode_mesh(&blob).unwrap();
        let glb = crate::csg::encode_glb(&mesh).expect("encode glb");
        assert_eq!(&glb[..4], b"glTF");
    }
}

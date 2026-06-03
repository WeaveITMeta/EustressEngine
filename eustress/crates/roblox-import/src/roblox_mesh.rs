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
//! - **v1.00 / v1.01** — ASCII body. Whitespace-free triples of bracketed
//!   `[x,y,z]` vectors, three vectors per vertex (position, normal,
//!   uv-with-w). A `u32` face count precedes the data on one line. v1.01
//!   positions are stored at 2× scale → multiplied by `0.5`.
//! - **v2.00** — binary. A small header gives `sizeof_vertex` /
//!   `sizeof_face`; the vertex array is `pos f32×3, normal f32×3, uv
//!   f32×2` (+ optional RGBA when the stride says so); the face array is
//!   `u32×3`.
//! - **v3.00 – v7.00** — binary with extra header fields plus optional
//!   LOD / mesh-subset / skinning / FACS chunks. We decode the BASE LOD's
//!   vertex + face arrays (the full-resolution mesh) and skip every
//!   trailing chunk (bones, skinning, FACS, mesh subsets, LOD tables).
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
        3..=7 => decode_v3plus(&blob[body..], major),
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
/// v1.01 stores positions at 2× the true scale → multiply by `0.5`.
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
    let pos_scale = if minor >= 1 { 0.5 } else { 1.0 };

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
        mesh.normals.push([n[0], n[1], n[2]]);
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
/// u8  sizeof_Vertex       (32 = pos+normal+uv; 36/40 add RGBA [+ tangent])
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

/// Decode the v3.00–v7.00 binary body, extracting the BASE LOD's vertex +
/// face arrays. Trailing chunks (LOD offset table, bones, skinning, mesh
/// subsets, FACS) are skipped.
///
/// Header shapes (all little-endian, after the ASCII version line):
/// - **v3.xx**: `u16 sizeof_MeshHeader; u8 sizeof_Vertex; u8 sizeof_Face;
///   u16 sizeof_LOD; u16 numLODs; u32 numVerts; u32 numFaces;`
/// - **v4.00 – v7.00**: `u16 sizeof_MeshHeader; u16 sizeof_Vertex;
///   u16 sizeof_Face; u16 sizeof_LOD; u16 numLODs; u32 numVerts;
///   u32 numFaces; u32 numBones; u32 sizeof_boneNamesBuffer;
///   u16 numSubsets; u8 numHighQualityLODs; u8 unused;` (FACS adds two
///   more u32 in v5+, which we don't need because the vertex/face arrays
///   come right after this header).
///
/// After the header: `numVerts` vertices, then `numFaces` faces, then the
/// LOD offset table (`numLODs` × u32) — which we use to clip to LOD0 —
/// then chunks we ignore.
fn decode_v3plus(body: &[u8], major: u32) -> Result<CsgMesh, MeshError> {
    let mut cur = Cursor::new(body);
    let sizeof_header = cur.u16()? as usize;

    let (sizeof_vertex, sizeof_face, num_lods, num_verts, num_faces, header_read);
    if major == 3 {
        let sv = cur.take(1)?[0] as usize;
        let sf = cur.take(1)?[0] as usize;
        let _sizeof_lod = cur.u16()? as usize;
        let nl = cur.u16()? as usize;
        let nv = cur.u32()? as usize;
        let nf = cur.u32()? as usize;
        sizeof_vertex = sv;
        sizeof_face = sf;
        num_lods = nl;
        num_verts = nv;
        num_faces = nf;
        // bytes consumed so far in the header block: u16 + u8+u8 + u16 + u16 + u32 + u32 = 16
        header_read = 16;
    } else {
        // v4 – v7
        let sv = cur.u16()? as usize;
        let sf = cur.u16()? as usize;
        let _sizeof_lod = cur.u16()? as usize;
        let nl = cur.u16()? as usize;
        let nv = cur.u32()? as usize;
        let nf = cur.u32()? as usize;
        sizeof_vertex = sv;
        sizeof_face = sf;
        num_lods = nl;
        num_verts = nv;
        num_faces = nf;
        // u16 ×5 (header/vertex/face/lod/numLODs) + u32 ×2 (verts/faces)
        // = 10 + 8 = 18 consumed.
        header_read = 18;
    }

    // Skip any header bytes beyond what we read (bone counts / subset
    // counts / FACS sizes live here in v4+; we don't need them because the
    // vertex array begins at `sizeof_header`).
    if sizeof_header > header_read {
        cur.skip(sizeof_header - header_read)?;
    }

    validate_counts(sizeof_vertex, sizeof_face, num_verts, num_faces)?;

    // Read all vertices + all faces, then clip faces to LOD0 via the LOD
    // offset table that follows the face array.
    let mut mesh =
        read_binary_vertices_faces(&mut cur, sizeof_vertex, sizeof_face, num_verts, num_faces)?;

    // LOD offset table: numLODs × u32 face-offsets. LOD0 occupies faces
    // [offsets[0], offsets[1]). offsets[0] is typically 0. If the table is
    // unreadable or absent we keep all faces (a superset that still
    // renders — lower LODs overlap the base mesh).
    if num_lods >= 2 {
        if let Ok(off0) = cur.u32() {
            if let Ok(off1) = cur.u32() {
                let lo = off0 as usize;
                let hi = off1 as usize;
                // Each face = 3 indices. Clip the flat index list to
                // [lo*3, hi*3) when the range is sane.
                if hi > lo && hi * 3 <= mesh.indices.len() {
                    mesh.indices = mesh.indices[lo * 3..hi * 3].to_vec();
                }
            }
        }
    }

    finalize(&mut mesh)?;
    Ok(mesh)
}

// ---------------------------------------------------------------------------
// Shared binary vertex/face reader
// ---------------------------------------------------------------------------

/// Read `num_verts` vertices then `num_faces` faces from `cur` using the
/// given strides. Each vertex's first 32 bytes are `pos f32×3, normal
/// f32×3, uv f32×2`; when `sizeof_vertex >= 36` the next 4 bytes are RGBA
/// (u8×4 → 0..1); any remaining stride bytes (tangent, etc.) are skipped.
/// Each face's first 12 bytes are `u32×3` indices; any remaining face
/// stride bytes are skipped.
fn read_binary_vertices_faces(
    cur: &mut Cursor,
    sizeof_vertex: usize,
    sizeof_face: usize,
    num_verts: usize,
    num_faces: usize,
) -> Result<CsgMesh, MeshError> {
    let mut mesh = CsgMesh::default();
    mesh.positions.reserve(num_verts);
    mesh.normals.reserve(num_verts);
    mesh.uvs.reserve(num_verts);

    let has_color = sizeof_vertex >= 36;
    if has_color {
        mesh.colors.reserve(num_verts);
    }

    for _ in 0..num_verts {
        let pos = cur.f32x3()?;
        let norm = cur.f32x3()?;
        let uv = cur.f32x2()?;
        let mut consumed = 32usize;
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
        // Skip any remaining per-vertex bytes (tangent in v4+, etc.).
        if sizeof_vertex > consumed {
            cur.skip(sizeof_vertex - consumed)?;
        }
        mesh.positions.push(pos);
        mesh.normals.push(norm);
        mesh.uvs.push(uv);
    }

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

    finalize(&mut mesh)?;
    Ok(mesh)
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

    #[test]
    fn v2_with_color_stride_reads_rgba() {
        // sizeof_vertex = 36 → pos+normal+uv+RGBA.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"version 2.00\n");
        buf.extend_from_slice(&12u16.to_le_bytes());
        buf.push(36u8);
        buf.push(12u8);
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        for v in 0..3u32 {
            for c in [v as f32, 0.0, 0.0] {
                buf.extend_from_slice(&c.to_le_bytes());
            }
            for c in [0.0f32, 1.0, 0.0] {
                buf.extend_from_slice(&c.to_le_bytes());
            }
            for c in [0.0f32, 0.0] {
                buf.extend_from_slice(&c.to_le_bytes());
            }
            buf.extend_from_slice(&[255u8, 128, 0, 255]); // RGBA
        }
        for i in [0u32, 1, 2] {
            buf.extend_from_slice(&i.to_le_bytes());
        }
        let mesh = decode_mesh(&buf).expect("decode v2+color");
        assert_eq!(mesh.colors.len(), 3);
        assert!((mesh.colors[0][0] - 1.0).abs() < 1e-6);
        assert!((mesh.colors[0][1] - 128.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn decodes_v1_ascii_triangle() {
        // One triangle = 3 vertices = 9 vectors (pos/normal/uv each).
        // v1.00 (no 0.5 scale).
        let mut body = String::from("1\n"); // face count line
        // vertex 0
        body.push_str("[1,0,0][0,0,1][0,0,0]");
        // vertex 1
        body.push_str("[0,1,0][0,0,1][1,0,0]");
        // vertex 2
        body.push_str("[0,0,1][0,0,1][0,1,0]");
        let blob = format!("version 1.00\n{body}");
        let mesh = decode_mesh(blob.as_bytes()).expect("decode v1");
        assert_eq!(mesh.positions.len(), 3);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.positions[0], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.uvs[1], [1.0, 0.0]);
    }

    #[test]
    fn v1_01_halves_positions() {
        let body = "[2,0,0][0,0,1][0,0,0][0,2,0][0,0,1][1,0,0][0,0,2][0,0,1][0,1,0]";
        let blob = format!("version 1.01\n{body}");
        let mesh = decode_mesh(blob.as_bytes()).expect("decode v1.01");
        // Positions are halved.
        assert_eq!(mesh.positions[0], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.positions[1], [0.0, 1.0, 0.0]);
        assert_eq!(mesh.positions[2], [0.0, 0.0, 1.0]);
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

    #[test]
    fn decodes_v4_with_color_and_clips_to_lod0() {
        // v4 header: u16 sizeof_header; u16 sizeof_vertex; u16 sizeof_face;
        // u16 sizeof_lod; u16 numLODs; u32 numVerts; u32 numFaces; then the
        // header padding up to sizeof_header, then verts, faces, LOD table.
        let mut buf = Vec::new();
        buf.extend_from_slice(b"version 4.00\n");
        let sizeof_header = 24u16; // 18 read + 6 padding (bones/subset fields)
        buf.extend_from_slice(&sizeof_header.to_le_bytes());
        buf.extend_from_slice(&40u16.to_le_bytes()); // sizeof_vertex (pos+norm+uv+RGBA+tangent)
        buf.extend_from_slice(&12u16.to_le_bytes()); // sizeof_face
        buf.extend_from_slice(&4u16.to_le_bytes()); // sizeof_lod
        buf.extend_from_slice(&2u16.to_le_bytes()); // numLODs
        buf.extend_from_slice(&3u32.to_le_bytes()); // numVerts
        buf.extend_from_slice(&1u32.to_le_bytes()); // numFaces (LOD0 only)
        buf.extend_from_slice(&[0u8; 6]); // header padding to sizeof_header=24
        for v in 0..3u32 {
            for c in [v as f32, 0.0, 0.0] {
                buf.extend_from_slice(&c.to_le_bytes());
            }
            for c in [0.0f32, 0.0, 1.0] {
                buf.extend_from_slice(&c.to_le_bytes());
            }
            for c in [0.0f32, 0.0] {
                buf.extend_from_slice(&c.to_le_bytes());
            }
            buf.extend_from_slice(&[10u8, 20, 30, 255]); // RGBA
            buf.extend_from_slice(&[0u8; 4]); // tangent (skipped)
        }
        for i in [0u32, 1, 2] {
            buf.extend_from_slice(&i.to_le_bytes());
        }
        // LOD offset table: [0, 1] face offsets → LOD0 = faces[0..1].
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());

        let mesh = decode_mesh(&buf).expect("decode v4");
        assert_eq!(mesh.positions.len(), 3);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.colors.len(), 3);
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

//! Roblox CSG (`UnionOperation` / `NegateOperation` / `IntersectOperation`)
//! baked-mesh extraction → `.glb`.
//!
//! Spec ref: `docs/architecture/ROBLOX_IMPORT_SPEC.md` §7.
//!
//! ## What this module does
//!
//! Every shipped CSG instance carries a triangulated render mesh inside
//! its `MeshData` property (a `BinaryString` or `SharedString`). This
//! module decodes that blob — Roblox's internal `CSGMDL` mesh format,
//! including the XOR obfuscation layer — into a plain
//! [`CsgMesh`] (positions / normals / uvs / colors / triangle indices)
//! and writes it as a glTF-binary `.glb` asset the Eustress mesh loader
//! consumes.
//!
//! [`import_csg`] is the entry point: it decodes `MeshData`, writes
//! `<csg-folder>/csg.glb`, and returns a [`CsgOutcome`] telling the
//! materializer whether to point the `Part` at the mesh
//! ([`CsgOutcome::Baked`]) or fall back to an AABB block
//! ([`CsgOutcome::Aabb`]).
//!
//! ## The CSGMDL format
//!
//! The format was reverse-engineered by Rhys Lloyd (krakow10) in the
//! MIT/Apache-licensed `rbx_mesh` crate
//! (<https://github.com/krakow10/rbx_mesh>). The decoders below are a
//! self-contained, hand-rolled port of the `union_graphics` reader
//! (v2 / v4 / v5 + the `CSGK` no-mesh marker) so the importer keeps a
//! minimal dependency set (no `binrw`). Attribution + license notice is
//! in [`OBFUSCATION_KEY`].
//!
//! Three render-mesh versions exist in the wild:
//! - **CSGMDL2** — plaintext body: a 32-byte hash, then `u32` vertex
//!   count, a `u32` vertex-stride magic (84), `count` × 84-byte vertices
//!   (pos f32×3, normal f32×3, color u8×4, normalId u32, uv f32×2, two
//!   `u128` zero magics around a tangent f32×3), then `u32` index count
//!   and `count/3` triangles of `u32×3`.
//! - **CSGMDL4** — CSGMDL2, XOR-obfuscated, plus a trailing `u32` list.
//! - **CSGMDL5** — XOR-obfuscated, struct-of-arrays with quantised
//!   normals/tangents and delta-encoded face indices (a small state
//!   machine + range markers).
//! - **CSGK** — `b"CSGK"` + 32 ascii-hex bytes; carries **no** mesh
//!   (an unbaked reference). Treated as "no usable mesh" → AABB fallback.
//!
//! ### Defensive decoding
//!
//! Real places hit malformed / truncated blobs. Every read is
//! bounds-checked; a decode failure returns `Err` and the caller falls
//! back to an AABB block + a logged approximation rather than panicking.

use std::path::Path;

// ---------------------------------------------------------------------------
// Obfuscation (ported from krakow10/rbx_mesh, MIT OR Apache-2.0)
// ---------------------------------------------------------------------------

/// The 31-byte repeating XOR key Roblox applies to obfuscated CSGMDL
/// payloads (CSGMDL4 / CSGMDL5). Sourced from the MIT/Apache-licensed
/// `rbx_mesh` crate by Rhys Lloyd (krakow10):
/// <https://github.com/krakow10/rbx_mesh/blob/master/src/union_graphics/obfuscate.rs>.
pub const OBFUSCATION_KEY: [u8; 31] = [
    86, 46, 110, 88, 49, 32, 48, 4, 52, 105, 12, 119, 12, 1, 94, 0, 26, 96, 55, 105, 29, 82, 43, 7,
    79, 36, 89, 101, 83, 4, 122,
];

/// Reversibly de-obfuscate `buf` in place. `offset` is the byte offset of
/// `buf[0]` within the full obfuscated stream (the XOR key cycles over
/// absolute stream position). Self-inverse.
fn deobfuscate(offset: usize, buf: &mut [u8]) {
    let len = OBFUSCATION_KEY.len();
    for (i, b) in buf.iter_mut().enumerate() {
        *b ^= OBFUSCATION_KEY[(offset + i) % len];
    }
}

// ---------------------------------------------------------------------------
// Bounds-checked little-endian byte cursor
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

    fn take(&mut self, n: usize) -> Result<&'a [u8], CsgError> {
        if self.remaining() < n {
            return Err(CsgError::Truncated {
                wanted: n,
                had: self.remaining(),
            });
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, CsgError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, CsgError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u16(&mut self) -> Result<u16, CsgError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn f32(&mut self) -> Result<f32, CsgError> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32x3(&mut self) -> Result<[f32; 3], CsgError> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    fn f32x2(&mut self) -> Result<[f32; 2], CsgError> {
        Ok([self.f32()?, self.f32()?])
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A CSG mesh decode failure. Always recoverable — the caller degrades
/// to an AABB block.
#[derive(Debug)]
pub enum CsgError {
    /// Wanted more bytes than the buffer held.
    Truncated {
        /// Bytes requested.
        wanted: usize,
        /// Bytes available.
        had: usize,
    },
    /// The leading magic did not match any known CSGMDL version.
    UnknownMagic([u8; 10]),
    /// The blob is a `CSGK` marker — carries no mesh.
    NoMesh,
    /// A structural invariant was violated (e.g. index out of range).
    Malformed(String),
}

impl std::fmt::Display for CsgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CsgError::Truncated { wanted, had } => {
                write!(f, "truncated CSG mesh: wanted {wanted} bytes, had {had}")
            }
            CsgError::UnknownMagic(m) => write!(f, "unknown CSG mesh magic {m:02x?}"),
            CsgError::NoMesh => write!(f, "CSGK marker carries no mesh data"),
            CsgError::Malformed(s) => write!(f, "malformed CSG mesh: {s}"),
        }
    }
}

impl std::error::Error for CsgError {}

// ---------------------------------------------------------------------------
// Decoded mesh
// ---------------------------------------------------------------------------

/// A decoded CSG render mesh in plain Eustress-friendly arrays.
#[derive(Debug, Clone, Default)]
pub struct CsgMesh {
    /// Vertex positions (studs = meters).
    pub positions: Vec<[f32; 3]>,
    /// Per-vertex normals (unit, may be empty if the source omitted them).
    pub normals: Vec<[f32; 3]>,
    /// Per-vertex UVs (may be empty).
    pub uvs: Vec<[f32; 2]>,
    /// Per-vertex RGBA colors in 0..1 (may be empty).
    pub colors: Vec<[f32; 4]>,
    /// Triangle indices (length is a multiple of 3).
    pub indices: Vec<u32>,
}

impl CsgMesh {
    /// True when there is nothing to write.
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty() || self.indices.is_empty()
    }

    fn validate(&self) -> Result<(), CsgError> {
        if self.indices.len() % 3 != 0 {
            return Err(CsgError::Malformed(format!(
                "index count {} not a multiple of 3",
                self.indices.len()
            )));
        }
        let n = self.positions.len() as u32;
        if let Some(&bad) = self.indices.iter().find(|&&i| i >= n) {
            return Err(CsgError::Malformed(format!(
                "triangle index {bad} out of range (vertex count {n})"
            )));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CSGMDL magic detection
// ---------------------------------------------------------------------------

/// Obfuscated magic for `CSGMDL` + version `u32`, i.e.
/// `deobfuscate(0, b"CSGMDL" ++ version.to_le_bytes())`. The magic in the
/// file is the *obfuscated* form regardless of whether the body is
/// obfuscated.
fn csgmdl_magic(version: u32) -> [u8; 10] {
    let mut m = [0u8; 10];
    m[..6].copy_from_slice(b"CSGMDL");
    m[6..].copy_from_slice(&version.to_le_bytes());
    deobfuscate(0, &mut m);
    m
}

// ---------------------------------------------------------------------------
// Top-level decode
// ---------------------------------------------------------------------------

/// Decode a Roblox CSG `MeshData` blob into a [`CsgMesh`].
///
/// Detects the version by the leading 10-byte magic and dispatches to the
/// matching decoder. Returns [`CsgError::NoMesh`] for a `CSGK` marker
/// (which carries no geometry) and [`CsgError::UnknownMagic`] for an
/// unrecognised version. Never panics.
pub fn decode_mesh_data(blob: &[u8]) -> Result<CsgMesh, CsgError> {
    if blob.len() >= 4 && &blob[..4] == b"CSGK" {
        return Err(CsgError::NoMesh);
    }
    if blob.len() < 10 {
        return Err(CsgError::Truncated {
            wanted: 10,
            had: blob.len(),
        });
    }
    let magic: [u8; 10] = blob[..10].try_into().unwrap();

    if magic == csgmdl_magic(2) {
        decode_csgmdl2(&blob[10..], false)
    } else if magic == csgmdl_magic(4) {
        decode_csgmdl4(&blob[10..])
    } else if magic == csgmdl_magic(5) {
        decode_csgmdl5(&blob[10..])
    } else {
        Err(CsgError::UnknownMagic(magic))
    }
}

// ---------------------------------------------------------------------------
// CSGMDL2 / CSGMDL4 — fixed-layout vertices
// ---------------------------------------------------------------------------

/// Decode the CSGMDL2 / CSGMDL4 shared body (hash + Mesh2). For CSGMDL4
/// the body bytes have already been de-obfuscated by the caller and
/// `_obfuscated` is irrelevant; CSGMDL2 bodies are plaintext.
///
/// `body` is the stream *after* the 10-byte magic.
fn decode_csgmdl2(body: &[u8], _obfuscated: bool) -> Result<CsgMesh, CsgError> {
    let mut cur = Cursor::new(body);
    // Hash: 16 bytes + 16 unknown bytes.
    let _ = cur.take(32)?;
    let mesh = read_mesh2(&mut cur)?;
    mesh.validate()?;
    Ok(mesh)
}

/// CSGMDL4: obfuscated CSGMDL2 + trailing u32 list. We de-obfuscate the
/// whole body, then parse it as Mesh2 (the trailing list is ignored).
fn decode_csgmdl4(body: &[u8]) -> Result<CsgMesh, CsgError> {
    // The body is obfuscated starting at absolute offset 10 (the magic
    // occupies bytes 0..10 and is not obfuscated in the body decode —
    // it was matched against the obfuscated-magic constant directly).
    let mut deob = body.to_vec();
    deobfuscate(10, &mut deob);
    decode_csgmdl2(&deob, true)
}

/// Read a `Mesh2` (the CSGMDL2/4 vertex + face block).
fn read_mesh2(cur: &mut Cursor) -> Result<CsgMesh, CsgError> {
    let vertex_count = cur.u32()? as usize;
    // Vertex-stride magic — Roblox writes 84 (the byte size of one vertex).
    let stride = cur.u32()?;
    if stride != 84 {
        return Err(CsgError::Malformed(format!(
            "unexpected CSGMDL2 vertex stride {stride} (expected 84)"
        )));
    }
    // Guard against absurd counts before allocating.
    if vertex_count > 50_000_000 {
        return Err(CsgError::Malformed(format!(
            "implausible vertex count {vertex_count}"
        )));
    }

    let mut mesh = CsgMesh::default();
    mesh.positions.reserve(vertex_count);
    mesh.normals.reserve(vertex_count);
    mesh.uvs.reserve(vertex_count);
    mesh.colors.reserve(vertex_count);

    for _ in 0..vertex_count {
        let pos = cur.f32x3()?;
        let norm = cur.f32x3()?;
        let color = cur.take(4)?;
        let color = [
            color[0] as f32 / 255.0,
            color[1] as f32 / 255.0,
            color[2] as f32 / 255.0,
            color[3] as f32 / 255.0,
        ];
        let _normal_id = cur.u32()?;
        let uv = cur.f32x2()?;
        let _magic0 = cur.take(16)?; // u128 zero
        let _tangent = cur.f32x3()?;
        let _magic1 = cur.take(16)?; // u128 zero
        mesh.positions.push(pos);
        mesh.normals.push(norm);
        mesh.uvs.push(uv);
        mesh.colors.push(color);
    }

    let face_index_count = cur.u32()? as usize;
    if face_index_count % 3 != 0 {
        return Err(CsgError::Malformed(format!(
            "CSGMDL2 face index count {face_index_count} not a multiple of 3"
        )));
    }
    mesh.indices.reserve(face_index_count);
    for _ in 0..face_index_count {
        mesh.indices.push(cur.u32()?);
    }
    Ok(mesh)
}

// ---------------------------------------------------------------------------
// CSGMDL5 — struct-of-arrays, quantised, delta-encoded faces
// ---------------------------------------------------------------------------

/// Decode CSGMDL5 (obfuscated struct-of-arrays). Port of
/// `rbx_mesh::union_graphics::v5`.
fn decode_csgmdl5(body: &[u8]) -> Result<CsgMesh, CsgError> {
    let mut deob = body.to_vec();
    deobfuscate(10, &mut deob);
    let mut cur = Cursor::new(&deob);

    // positions: u16 count, then count × f32×3
    let pos_count = cur.u16()? as usize;
    let mut positions = Vec::with_capacity(pos_count);
    for _ in 0..pos_count {
        positions.push(cur.f32x3()?);
    }

    // normals: u16 count, u32 byte-len, then count × quantised i16×3
    let normals_count = cur.u16()? as usize;
    let _normals_len = cur.u32()?;
    let mut normals = Vec::with_capacity(normals_count);
    for _ in 0..normals_count {
        normals.push(dequantize_i16x3(&mut cur)?);
    }

    // colors: u16 count, then count × u8×4
    let color_count = cur.u16()? as usize;
    let mut colors = Vec::with_capacity(color_count);
    for _ in 0..color_count {
        let c = cur.take(4)?;
        colors.push([
            c[0] as f32 / 255.0,
            c[1] as f32 / 255.0,
            c[2] as f32 / 255.0,
            c[3] as f32 / 255.0,
        ]);
    }

    // normal ids: u16 count, then count × u8
    let normal_id_count = cur.u16()? as usize;
    let _ = cur.take(normal_id_count)?;

    // tex: u16 count, then count × f32×2
    let tex_count = cur.u16()? as usize;
    let mut uvs = Vec::with_capacity(tex_count);
    for _ in 0..tex_count {
        uvs.push(cur.f32x2()?);
    }

    // tangents: u16 count, u32 byte-len, then count × quantised i16×3
    let tangents_count = cur.u16()? as usize;
    let _tangents_len = cur.u32()?;
    for _ in 0..tangents_count {
        let _ = dequantize_i16x3(&mut cur)?;
    }

    // faces: delta-encoded index stream
    let indices = read_faces5(&mut cur)?;

    let mut mesh = CsgMesh {
        positions,
        normals,
        uvs,
        colors,
        indices,
    };
    // CSGMDL5 stores positions and per-corner attribute indices in
    // separate arrays. The face stream we extract indexes positions; if
    // attribute arrays differ in length, drop them (the .glb still
    // renders with positions + computed flat normals).
    if mesh.normals.len() != mesh.positions.len() {
        mesh.normals.clear();
    }
    if mesh.uvs.len() != mesh.positions.len() {
        mesh.uvs.clear();
    }
    if mesh.colors.len() != mesh.positions.len() {
        mesh.colors.clear();
    }
    mesh.validate()?;
    Ok(mesh)
}

/// Read a quantised normal/tangent (`i16×3` → `f32×3`).
fn dequantize_i16x3(cur: &mut Cursor) -> Result<[f32; 3], CsgError> {
    const SCALE: f32 = 1.0 / 32_767.0;
    let read = |cur: &mut Cursor| -> Result<f32, CsgError> {
        let b = cur.take(2)?;
        let raw = i16::from_le_bytes([b[0], b[1]]);
        Ok((raw.wrapping_sub(0x7FFF) as f32) * SCALE)
    };
    Ok([read(cur)?, read(cur)?, read(cur)?])
}

/// Decode the CSGMDL5 delta-encoded face index stream into a flat index
/// list, taking only the first range (the actual render triangles; the
/// trailing ranges are LOD/unknown). Port of `rbx_mesh`'s `Faces5`.
fn read_faces5(cur: &mut Cursor) -> Result<Vec<u32>, CsgError> {
    let vertex_count = cur.u32()? as usize;
    let vertex_data_len = cur.u32()? as usize;
    let vertex_data = cur.take(vertex_data_len)?.to_vec();
    let range_marker_count = cur.u8()? as usize;
    let mut range_markers = Vec::with_capacity(range_marker_count);
    for _ in 0..range_marker_count {
        range_markers.push(cur.u32()?);
    }

    // State machine: accumulate signed deltas into a running index.
    let mut indices = Vec::with_capacity(vertex_count);
    let mut it = vertex_data.into_iter();
    let mut index_out: i64 = 0;
    for _ in 0..vertex_count {
        let v0 = it
            .next()
            .ok_or_else(|| CsgError::Malformed("faces5: unexpected EOF".into()))?;
        if v0 & (1 << 7) == 0 {
            if v0 & (1 << 6) == 0 {
                index_out += v0 as i64;
            } else {
                // 64..=127 maps to -64..=-1.
                index_out -= -((v0 | 0x80) as i8) as i64;
            }
        } else {
            let v1 = it
                .next()
                .ok_or_else(|| CsgError::Malformed("faces5: EOF in 3-byte delta".into()))?;
            let v2 = it
                .next()
                .ok_or_else(|| CsgError::Malformed("faces5: EOF in 3-byte delta".into()))?;
            index_out += u32::from_le_bytes([v2, v1, v0 & 0x7F, 0]) as i64;
        }
        indices.push((index_out & 0x7FFFFF) as u32);
    }

    // Use the range markers to slice out the primary triangle range.
    // marker0 = start offset to drop; marker1 = end of the primary range.
    if range_markers.is_empty() {
        return Ok(indices);
    }
    let marker0 = range_markers[0] as usize;
    if marker0 > indices.len() {
        return Err(CsgError::Malformed(format!(
            "faces5 marker0 {marker0} out of range (len {})",
            indices.len()
        )));
    }
    if marker0 > 0 {
        indices.drain(..marker0);
    }
    if range_markers.len() >= 2 {
        let marker1 = range_markers[1] as usize;
        let take = marker1.saturating_sub(range_markers[0] as usize);
        if take <= indices.len() {
            indices.truncate(take);
        }
    }
    Ok(indices)
}

// ---------------------------------------------------------------------------
// glTF-binary (.glb) writer — hand-rolled, no extra deps
// ---------------------------------------------------------------------------

/// Write `mesh` to a glTF 2.0 binary `.glb` file at `path`. One mesh, one
/// primitive, indexed triangles, with POSITION (required) plus optional
/// NORMAL / TEXCOORD_0 / COLOR_0. No external crate — emits the JSON via
/// `serde_json` and the binary container by hand per the glTF 2.0 spec.
pub fn write_glb(path: &Path, mesh: &CsgMesh) -> std::io::Result<()> {
    let glb = encode_glb(mesh)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    std::fs::write(path, glb)
}

/// Encode `mesh` into glb bytes (testable without touching the FS).
pub fn encode_glb(mesh: &CsgMesh) -> Result<Vec<u8>, CsgError> {
    mesh.validate()?;
    if mesh.is_empty() {
        return Err(CsgError::Malformed("cannot write empty mesh to glb".into()));
    }

    // ── Build the BIN buffer: indices, then each vertex attribute. ──
    // Component layout (all little-endian, 4-byte aligned per accessor):
    //   [0] indices            (u32 scalar)
    //   [1] POSITION           (f32 vec3)
    //   [2] NORMAL  (optional) (f32 vec3)
    //   [3] TEXCOORD_0 (opt)   (f32 vec2)
    //   [4] COLOR_0 (opt)      (f32 vec4)
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<BufferView> = Vec::new();
    let mut accessors: Vec<Accessor> = Vec::new();

    // Helper to align bin to 4 bytes.
    let align4 = |bin: &mut Vec<u8>| {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
    };

    // Indices.
    let idx_offset = bin.len();
    for &i in &mesh.indices {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let idx_view = views.len();
    views.push(BufferView {
        byte_offset: idx_offset,
        byte_length: bin.len() - idx_offset,
        target: 34963, // ELEMENT_ARRAY_BUFFER
    });
    let idx_accessor = accessors.len();
    accessors.push(Accessor {
        buffer_view: idx_view,
        component_type: 5125, // UNSIGNED_INT
        count: mesh.indices.len(),
        ty: "SCALAR",
        min: None,
        max: None,
    });
    align4(&mut bin);

    // POSITION (with min/max for validity).
    let pos_offset = bin.len();
    let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &mesh.positions {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_view = views.len();
    views.push(BufferView {
        byte_offset: pos_offset,
        byte_length: bin.len() - pos_offset,
        target: 34962, // ARRAY_BUFFER
    });
    let pos_accessor = accessors.len();
    accessors.push(Accessor {
        buffer_view: pos_view,
        component_type: 5126, // FLOAT
        count: mesh.positions.len(),
        ty: "VEC3",
        min: Some(min.to_vec()),
        max: Some(max.to_vec()),
    });
    align4(&mut bin);

    let mut normal_accessor = None;
    if mesh.normals.len() == mesh.positions.len() && !mesh.normals.is_empty() {
        let off = bin.len();
        for n in &mesh.normals {
            for c in n {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        let view = views.len();
        views.push(BufferView {
            byte_offset: off,
            byte_length: bin.len() - off,
            target: 34962,
        });
        normal_accessor = Some(accessors.len());
        accessors.push(Accessor {
            buffer_view: view,
            component_type: 5126,
            count: mesh.normals.len(),
            ty: "VEC3",
            min: None,
            max: None,
        });
        align4(&mut bin);
    }

    let mut uv_accessor = None;
    if mesh.uvs.len() == mesh.positions.len() && !mesh.uvs.is_empty() {
        let off = bin.len();
        for uv in &mesh.uvs {
            for c in uv {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        let view = views.len();
        views.push(BufferView {
            byte_offset: off,
            byte_length: bin.len() - off,
            target: 34962,
        });
        uv_accessor = Some(accessors.len());
        accessors.push(Accessor {
            buffer_view: view,
            component_type: 5126,
            count: mesh.uvs.len(),
            ty: "VEC2",
            min: None,
            max: None,
        });
        align4(&mut bin);
    }

    let mut color_accessor = None;
    if mesh.colors.len() == mesh.positions.len() && !mesh.colors.is_empty() {
        let off = bin.len();
        for c in &mesh.colors {
            for ch in c {
                bin.extend_from_slice(&ch.to_le_bytes());
            }
        }
        let view = views.len();
        views.push(BufferView {
            byte_offset: off,
            byte_length: bin.len() - off,
            target: 34962,
        });
        color_accessor = Some(accessors.len());
        accessors.push(Accessor {
            buffer_view: view,
            component_type: 5126,
            count: mesh.colors.len(),
            ty: "VEC4",
            min: None,
            max: None,
        });
        align4(&mut bin);
    }

    // ── Assemble the glTF JSON. ──
    let json = build_gltf_json(
        bin.len(),
        &views,
        &accessors,
        idx_accessor,
        pos_accessor,
        normal_accessor,
        uv_accessor,
        color_accessor,
    );
    let mut json_bytes = serde_json::to_vec(&json)
        .map_err(|e| CsgError::Malformed(format!("glTF json serialize: {e}")))?;
    // Pad JSON chunk to 4 bytes with spaces.
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    // Pad BIN chunk to 4 bytes with zeros.
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    // ── Write the GLB container. ──
    // Header (12) + JSON chunk header (8) + json + BIN chunk header (8) + bin.
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF"); // magic
    out.extend_from_slice(&2u32.to_le_bytes()); // version
    out.extend_from_slice(&(total as u32).to_le_bytes()); // total length
                                                           // JSON chunk
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    // BIN chunk
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    Ok(out)
}

struct BufferView {
    byte_offset: usize,
    byte_length: usize,
    target: u32,
}

struct Accessor {
    buffer_view: usize,
    component_type: u32,
    count: usize,
    ty: &'static str,
    min: Option<Vec<f32>>,
    max: Option<Vec<f32>>,
}

#[allow(clippy::too_many_arguments)]
fn build_gltf_json(
    bin_len: usize,
    views: &[BufferView],
    accessors: &[Accessor],
    idx_accessor: usize,
    pos_accessor: usize,
    normal_accessor: Option<usize>,
    uv_accessor: Option<usize>,
    color_accessor: Option<usize>,
) -> serde_json::Value {
    use serde_json::{json, Value};

    let views_json: Vec<Value> = views
        .iter()
        .map(|v| {
            json!({
                "buffer": 0,
                "byteOffset": v.byte_offset,
                "byteLength": v.byte_length,
                "target": v.target,
            })
        })
        .collect();

    let accessors_json: Vec<Value> = accessors
        .iter()
        .map(|a| {
            let mut obj = json!({
                "bufferView": a.buffer_view,
                "componentType": a.component_type,
                "count": a.count,
                "type": a.ty,
            });
            if let (Some(min), Some(max)) = (&a.min, &a.max) {
                obj["min"] = json!(min);
                obj["max"] = json!(max);
            }
            obj
        })
        .collect();

    let mut attributes = json!({ "POSITION": pos_accessor });
    if let Some(n) = normal_accessor {
        attributes["NORMAL"] = json!(n);
    }
    if let Some(t) = uv_accessor {
        attributes["TEXCOORD_0"] = json!(t);
    }
    if let Some(c) = color_accessor {
        attributes["COLOR_0"] = json!(c);
    }

    json!({
        "asset": { "version": "2.0", "generator": "eustress-roblox-import CSG extractor" },
        "scene": 0,
        "scenes": [ { "name": "Scene0", "nodes": [0] } ],
        "nodes": [ { "name": "csg", "mesh": 0 } ],
        "meshes": [ {
            "name": "csg",
            "primitives": [ {
                "attributes": attributes,
                "indices": idx_accessor,
                "mode": 4
            } ]
        } ],
        "buffers": [ { "byteLength": bin_len } ],
        "bufferViews": views_json,
        "accessors": accessors_json,
    })
}

// ---------------------------------------------------------------------------
// AABB fallback mesh
// ---------------------------------------------------------------------------

/// Build a unit-ish axis-aligned box mesh of the given `size` (studs),
/// centred on the origin, for the CSG AABB fallback (spec §7.4). The
/// caller positions it via the `Part` transform.
pub fn aabb_box_mesh(size: [f32; 3]) -> CsgMesh {
    let (hx, hy, hz) = (size[0] * 0.5, size[1] * 0.5, size[2] * 0.5);
    // 8 corners.
    let p = [
        [-hx, -hy, -hz],
        [hx, -hy, -hz],
        [hx, hy, -hz],
        [-hx, hy, -hz],
        [-hx, -hy, hz],
        [hx, -hy, hz],
        [hx, hy, hz],
        [-hx, hy, hz],
    ];
    // 12 triangles (CCW outward).
    let faces: [[usize; 3]; 12] = [
        [0, 2, 1],
        [0, 3, 2], // -Z
        [4, 5, 6],
        [4, 6, 7], // +Z
        [0, 1, 5],
        [0, 5, 4], // -Y
        [3, 7, 6],
        [3, 6, 2], // +Y
        [0, 4, 7],
        [0, 7, 3], // -X
        [1, 2, 6],
        [1, 6, 5], // +X
    ];
    let mut mesh = CsgMesh::default();
    for f in faces {
        for &vi in &f {
            let idx = mesh.positions.len() as u32;
            mesh.positions.push(p[vi]);
            mesh.indices.push(idx);
        }
    }
    mesh
}

// ---------------------------------------------------------------------------
// import_csg — public entry point
// ---------------------------------------------------------------------------

/// What [`import_csg`] decided to do with one CSG instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsgOutcome {
    /// The baked `MeshData` decoded successfully and `csg.glb` was
    /// written. The materializer should point the `Part` at it.
    Baked {
        /// Mesh filename written inside the CSG folder (always `csg.glb`).
        mesh_file: String,
        /// Triangle count (for reporting).
        triangles: usize,
    },
    /// No usable baked mesh; an AABB block `csg.glb` was written instead.
    Aabb {
        /// Mesh filename written (always `csg.glb`).
        mesh_file: String,
        /// Why we fell back (logged as an approximation).
        reason: String,
    },
}

/// Decode + extract one CSG instance's baked mesh into `csg_dir`
/// (`<space>/<service>/<csg-folder>/`).
///
/// - If `mesh_data` decodes, writes `csg.glb` and returns
///   [`CsgOutcome::Baked`].
/// - Otherwise writes an AABB block `csg.glb` sized from `aabb_size`
///   (the source `Part.Size`) and returns [`CsgOutcome::Aabb`] with the
///   reason.
///
/// Pure file I/O — no Bevy. Never panics. Disk write errors propagate
/// (spec §7.4: a `.glb` write failure is a hard error).
pub fn import_csg(
    csg_dir: &Path,
    mesh_data: Option<&[u8]>,
    aabb_size: [f32; 3],
) -> std::io::Result<CsgOutcome> {
    let glb_path = csg_dir.join("csg.glb");

    // Try the baked-mesh path.
    if let Some(blob) = mesh_data {
        match decode_mesh_data(blob) {
            Ok(mesh) if !mesh.is_empty() => match encode_glb(&mesh) {
                Ok(bytes) => {
                    std::fs::write(&glb_path, bytes)?;
                    return Ok(CsgOutcome::Baked {
                        mesh_file: "csg.glb".to_string(),
                        triangles: mesh.indices.len() / 3,
                    });
                }
                Err(e) => {
                    return write_aabb_fallback(
                        &glb_path,
                        aabb_size,
                        format!("CSG mesh decoded but glb encode failed: {e}"),
                    );
                }
            },
            Ok(_) => {
                return write_aabb_fallback(
                    &glb_path,
                    aabb_size,
                    "CSG MeshData decoded to an empty mesh".to_string(),
                );
            }
            Err(CsgError::NoMesh) => {
                return write_aabb_fallback(
                    &glb_path,
                    aabb_size,
                    "CSG carries a CSGK marker (unbaked) — no mesh data".to_string(),
                );
            }
            Err(e) => {
                return write_aabb_fallback(
                    &glb_path,
                    aabb_size,
                    format!("CSG MeshData decode failed: {e}"),
                );
            }
        }
    }

    write_aabb_fallback(
        &glb_path,
        aabb_size,
        "CSG instance has no MeshData property".to_string(),
    )
}

fn write_aabb_fallback(
    glb_path: &Path,
    aabb_size: [f32; 3],
    reason: String,
) -> std::io::Result<CsgOutcome> {
    // Guard against a degenerate size — fall back to a 4×4×4 stud block.
    let size = [
        if aabb_size[0].abs() < 1e-3 { 4.0 } else { aabb_size[0] },
        if aabb_size[1].abs() < 1e-3 { 4.0 } else { aabb_size[1] },
        if aabb_size[2].abs() < 1e-3 { 4.0 } else { aabb_size[2] },
    ];
    let mesh = aabb_box_mesh(size);
    let bytes = encode_glb(&mesh)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    std::fs::write(glb_path, bytes)?;
    Ok(CsgOutcome::Aabb {
        mesh_file: "csg.glb".to_string(),
        reason,
    })
}

// ---------------------------------------------------------------------------
// Test fixtures shared with the materializer integration tests
// ---------------------------------------------------------------------------

/// Build a tiny valid CSGMDL2 blob: a single triangle (3 vertices).
/// Exposed `pub(crate)` under `cfg(test)` so the materializer's
/// end-to-end test can craft a CSG instance without duplicating the
/// fixture.
#[cfg(test)]
pub(crate) fn make_csgmdl2_triangle_fixture() -> Vec<u8> {
    let mut buf = csgmdl_magic(2).to_vec();
    // hash: 32 bytes
    buf.extend_from_slice(&[0u8; 32]);
    // vertex_count
    buf.extend_from_slice(&3u32.to_le_bytes());
    // stride magic
    buf.extend_from_slice(&84u32.to_le_bytes());
    let verts = [
        ([0.0f32, 0.0, 0.0], [0.0f32, 0.0, 1.0], [0.0f32, 0.0]),
        ([1.0f32, 0.0, 0.0], [0.0f32, 0.0, 1.0], [1.0f32, 0.0]),
        ([0.0f32, 1.0, 0.0], [0.0f32, 0.0, 1.0], [0.0f32, 1.0]),
    ];
    for (pos, norm, uv) in verts {
        for c in pos {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        for c in norm {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        buf.extend_from_slice(&[200, 150, 100, 255]); // color
        buf.extend_from_slice(&1u32.to_le_bytes()); // normal_id
        for c in uv {
            buf.extend_from_slice(&c.to_le_bytes());
        }
        buf.extend_from_slice(&[0u8; 16]); // magic0 u128
        for c in [0.0f32, 0.0, 0.0] {
            buf.extend_from_slice(&c.to_le_bytes()); // tangent
        }
        buf.extend_from_slice(&[0u8; 16]); // magic1 u128
    }
    // face index count = 3
    buf.extend_from_slice(&3u32.to_le_bytes());
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

    /// Build a tiny valid CSGMDL2 blob: a single triangle (3 vertices).
    fn make_csgmdl2_triangle() -> Vec<u8> {
        super::make_csgmdl2_triangle_fixture()
    }

    #[test]
    fn obfuscation_is_self_inverse() {
        let mut data = vec![1, 2, 3, 4, 5, 200, 201, 202, 255, 0, 17, 99];
        let original = data.clone();
        deobfuscate(7, &mut data);
        assert_ne!(data, original);
        deobfuscate(7, &mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn csgmdl_magic_matches_known_constant() {
        // From rbx_mesh: CSGMDL2 magic = these obfuscated bytes.
        assert_eq!(
            csgmdl_magic(2),
            [0x15, 0x7d, 0x29, 0x15, 0x75, 0x6c, 0x32, 0x04, 0x34, 0x69]
        );
        assert_eq!(
            csgmdl_magic(4),
            [0x15, 0x7d, 0x29, 0x15, 0x75, 0x6c, 0x34, 0x04, 0x34, 0x69]
        );
        assert_eq!(
            csgmdl_magic(5),
            [0x15, 0x7d, 0x29, 0x15, 0x75, 0x6c, 0x35, 0x04, 0x34, 0x69]
        );
    }

    #[test]
    fn decode_csgmdl2_triangle() {
        let blob = make_csgmdl2_triangle();
        let mesh = decode_mesh_data(&blob).expect("decode");
        assert_eq!(mesh.positions.len(), 3);
        assert_eq!(mesh.normals.len(), 3);
        assert_eq!(mesh.uvs.len(), 3);
        assert_eq!(mesh.colors.len(), 3);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
        assert_eq!(mesh.positions[1], [1.0, 0.0, 0.0]);
        assert_eq!(mesh.uvs[2], [0.0, 1.0]);
    }

    #[test]
    fn csgk_is_detected_as_no_mesh() {
        let mut blob = b"CSGK".to_vec();
        blob.extend_from_slice(&[b'a'; 32]);
        match decode_mesh_data(&blob) {
            Err(CsgError::NoMesh) => {}
            other => panic!("expected NoMesh, got {other:?}"),
        }
    }

    #[test]
    fn unknown_magic_errors_gracefully() {
        let blob = vec![0xFF; 64];
        match decode_mesh_data(&blob) {
            Err(CsgError::UnknownMagic(_)) => {}
            other => panic!("expected UnknownMagic, got {other:?}"),
        }
    }

    #[test]
    fn truncated_blob_does_not_panic() {
        // Valid magic, then cut off mid-header.
        let mut blob = csgmdl_magic(2).to_vec();
        blob.extend_from_slice(&[0u8; 10]); // not even a full hash
        let res = decode_mesh_data(&blob);
        assert!(matches!(res, Err(CsgError::Truncated { .. })));
    }

    #[test]
    fn glb_encodes_valid_container() {
        let blob = make_csgmdl2_triangle();
        let mesh = decode_mesh_data(&blob).unwrap();
        let glb = encode_glb(&mesh).expect("encode glb");
        // GLB magic + version.
        assert_eq!(&glb[..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2);
        // total length matches.
        let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
        assert_eq!(total, glb.len());
        // First chunk is JSON.
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        assert_eq!(&glb[16..20], b"JSON");
        let json_str = std::str::from_utf8(&glb[20..20 + json_len]).unwrap();
        assert!(json_str.contains("\"POSITION\""));
        assert!(json_str.contains("\"version\":\"2.0\""));
        // Second chunk is BIN.
        let bin_hdr = 20 + json_len;
        assert_eq!(&glb[bin_hdr + 4..bin_hdr + 8], b"BIN\0");
    }

    #[test]
    fn import_csg_writes_baked_glb() {
        let dir = std::env::temp_dir().join(format!(
            "rbx_csg_baked_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let blob = make_csgmdl2_triangle();
        let outcome = import_csg(&dir, Some(&blob), [2.0, 2.0, 2.0]).expect("import_csg");
        match outcome {
            CsgOutcome::Baked { mesh_file, triangles } => {
                assert_eq!(mesh_file, "csg.glb");
                assert_eq!(triangles, 1);
            }
            other => panic!("expected Baked, got {other:?}"),
        }
        assert!(dir.join("csg.glb").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_csg_falls_back_to_aabb_when_no_mesh() {
        let dir = std::env::temp_dir().join(format!(
            "rbx_csg_aabb_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let outcome = import_csg(&dir, None, [3.0, 5.0, 7.0]).expect("import_csg");
        match outcome {
            CsgOutcome::Aabb { mesh_file, reason } => {
                assert_eq!(mesh_file, "csg.glb");
                assert!(reason.contains("no MeshData"));
            }
            other => panic!("expected Aabb, got {other:?}"),
        }
        // The AABB glb should be valid + non-trivial.
        let glb = std::fs::read(dir.join("csg.glb")).unwrap();
        assert_eq!(&glb[..4], b"glTF");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_csg_aabb_on_csgk_marker() {
        let dir = std::env::temp_dir().join(format!(
            "rbx_csg_csgk_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut blob = b"CSGK".to_vec();
        blob.extend_from_slice(&[b'0'; 32]);
        let outcome = import_csg(&dir, Some(&blob), [2.0, 2.0, 2.0]).expect("import_csg");
        assert!(matches!(outcome, CsgOutcome::Aabb { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn aabb_box_has_12_triangles() {
        let mesh = aabb_box_mesh([2.0, 4.0, 6.0]);
        assert_eq!(mesh.indices.len(), 36); // 12 tris × 3
        assert_eq!(mesh.positions.len(), 36);
        // Extents.
        let max_x = mesh.positions.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
        assert!((max_x - 1.0).abs() < 1e-6);
        let max_y = mesh.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((max_y - 2.0).abs() < 1e-6);
    }
}

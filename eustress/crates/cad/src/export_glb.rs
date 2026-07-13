//! Minimal glTF 2.0 binary (`.glb`) writer for [`EvalMesh`].
//!
//! No external `gltf` crate — pure std + serde_json so the CAD crate
//! stays FFI-free and lightweight. Embeds optional extras JSON (param
//! schema, semantic tags) on the mesh node for agent self-description.

use crate::eval::EvalMesh;
use crate::{CadError, CadResult};

/// Write `mesh` to `path` as a binary glTF 2.0 (`.glb`) file.
///
/// `extras` is optional JSON embedded on the root node (param schema,
/// feature-tree fingerprint, etc.). Positions are meters, Y-up,
/// right-handed — matching Eustress / glTF convention.
pub fn write_glb(
    path: &std::path::Path,
    mesh: &EvalMesh,
    extras: Option<serde_json::Value>,
) -> CadResult<()> {
    let bytes = encode_glb(mesh, extras)?;
    std::fs::write(path, bytes).map_err(|e| CadError::Io(format!("write {:?}: {e}", path)))
}

/// Encode to GLB bytes without touching the filesystem.
pub fn encode_glb(mesh: &EvalMesh, extras: Option<serde_json::Value>) -> CadResult<Vec<u8>> {
    if mesh.positions.is_empty() || mesh.indices.is_empty() {
        return Err(CadError::EvalFailed {
            feature: "export_glb".into(),
            reason: "empty mesh".into(),
        });
    }
    if mesh.indices.len() % 3 != 0 {
        return Err(CadError::EvalFailed {
            feature: "export_glb".into(),
            reason: "index count not divisible by 3".into(),
        });
    }

    let n_verts = mesh.positions.len();
    let has_normals = mesh.normals.len() == n_verts;
    let has_uvs = mesh.uvs.len() == n_verts;

    // Build tightly packed binary buffer: positions, normals?, uvs?, indices
    let mut bin: Vec<u8> = Vec::new();

    // Positions — f32 LE, 3 components
    let pos_offset = 0usize;
    for p in &mesh.positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_len = n_verts * 12;
    pad4(&mut bin);

    let mut normals_offset = 0usize;
    let mut normals_len = 0usize;
    if has_normals {
        normals_offset = bin.len();
        for n in &mesh.normals {
            for c in n {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        normals_len = n_verts * 12;
        pad4(&mut bin);
    }

    let mut uvs_offset = 0usize;
    let mut uvs_len = 0usize;
    if has_uvs {
        uvs_offset = bin.len();
        for uv in &mesh.uvs {
            for c in uv {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        uvs_len = n_verts * 8;
        pad4(&mut bin);
    }

    // Indices — prefer u16 if all fit, else u32
    let max_index = mesh.indices.iter().copied().max().unwrap_or(0);
    let use_u16 = max_index <= u16::MAX as u32;
    let indices_offset = bin.len();
    if use_u16 {
        for &i in &mesh.indices {
            bin.extend_from_slice(&(i as u16).to_le_bytes());
        }
    } else {
        for &i in &mesh.indices {
            bin.extend_from_slice(&i.to_le_bytes());
        }
    }
    let indices_len = bin.len() - indices_offset;
    pad4(&mut bin);

    // Bounds
    let (min, max) = position_bounds(&mesh.positions);

    // Accessors / bufferViews
    let mut buffer_views = vec![
        serde_json::json!({
            "buffer": 0,
            "byteOffset": pos_offset,
            "byteLength": pos_len,
            "target": 34962  // ARRAY_BUFFER
        }),
    ];
    let mut accessors = vec![
        serde_json::json!({
            "bufferView": 0,
            "componentType": 5126, // FLOAT
            "count": n_verts,
            "type": "VEC3",
            "min": min,
            "max": max
        }),
    ];

    let mut attributes = serde_json::json!({ "POSITION": 0 });
    let mut next_view = 1usize;
    let mut next_acc = 1usize;

    if has_normals {
        buffer_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": normals_offset,
            "byteLength": normals_len,
            "target": 34962
        }));
        accessors.push(serde_json::json!({
            "bufferView": next_view,
            "componentType": 5126,
            "count": n_verts,
            "type": "VEC3"
        }));
        attributes["NORMAL"] = serde_json::json!(next_acc);
        next_view += 1;
        next_acc += 1;
    }

    if has_uvs {
        buffer_views.push(serde_json::json!({
            "buffer": 0,
            "byteOffset": uvs_offset,
            "byteLength": uvs_len,
            "target": 34962
        }));
        accessors.push(serde_json::json!({
            "bufferView": next_view,
            "componentType": 5126,
            "count": n_verts,
            "type": "VEC2"
        }));
        attributes["TEXCOORD_0"] = serde_json::json!(next_acc);
        next_view += 1;
        next_acc += 1;
    }

    buffer_views.push(serde_json::json!({
        "buffer": 0,
        "byteOffset": indices_offset,
        "byteLength": indices_len,
        "target": 34963  // ELEMENT_ARRAY_BUFFER
    }));
    accessors.push(serde_json::json!({
        "bufferView": next_view,
        "componentType": if use_u16 { 5123 } else { 5125 }, // UNSIGNED_SHORT / UNSIGNED_INT
        "count": mesh.indices.len(),
        "type": "SCALAR"
    }));
    let indices_acc = next_acc;

    let mut node = serde_json::json!({
        "mesh": 0,
        "name": "CadPart"
    });
    if let Some(ex) = extras {
        node["extras"] = ex;
    }

    let gltf = serde_json::json!({
        "asset": { "version": "2.0", "generator": "eustress-cad" },
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": buffer_views,
        "accessors": accessors,
        "meshes": [{
            "primitives": [{
                "attributes": attributes,
                "indices": indices_acc,
                "mode": 4  // TRIANGLES
            }]
        }],
        "nodes": [node],
        "scenes": [{ "nodes": [0] }],
        "scene": 0
    });

    let json_str = serde_json::to_string(&gltf)
        .map_err(|e| CadError::Serialize(format!("gltf json: {e}")))?;
    let mut json_bytes = json_str.into_bytes();
    // Pad JSON to 4-byte boundary with spaces
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }

    // GLB layout
    let json_chunk_len = json_bytes.len() as u32;
    let bin_chunk_len = bin.len() as u32;
    let total_len = 12 + 8 + json_chunk_len + 8 + bin_chunk_len;

    let mut out = Vec::with_capacity(total_len as usize);
    // Header
    out.extend_from_slice(&0x4654_6C67u32.to_le_bytes()); // magic "glTF"
    out.extend_from_slice(&2u32.to_le_bytes()); // version
    out.extend_from_slice(&total_len.to_le_bytes());
    // JSON chunk
    out.extend_from_slice(&json_chunk_len.to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes()); // "JSON"
    out.extend_from_slice(&json_bytes);
    // BIN chunk
    out.extend_from_slice(&bin_chunk_len.to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes()); // "BIN\0"
    out.extend_from_slice(&bin);

    Ok(out)
}

fn pad4(buf: &mut Vec<u8>) {
    while buf.len() % 4 != 0 {
        buf.push(0);
    }
}

fn position_bounds(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for a in 0..3 {
            min[a] = min[a].min(p[a]);
            max[a] = max[a].max(p[a]);
        }
    }
    (min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_minimal_triangle() {
        let mesh = EvalMesh {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            indices: vec![0, 1, 2],
        };
        let bytes = encode_glb(&mesh, Some(serde_json::json!({"cad": true}))).unwrap();
        assert!(bytes.len() > 100);
        assert_eq!(&bytes[0..4], b"glTF");
    }
}

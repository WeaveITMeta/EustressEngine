#!/usr/bin/env rust-script
//! Generate Welcome Cube .glb file
//! 
//! Run with: cargo run --bin generate_welcome_cube

use std::fs::File;
use std::io::Write;
use std::path::Path;

fn main() {
    let output_path = Path::new("C:/Users/miksu/Documents/Eustress/Space1/src/Workspace/WelcomeCube.glb");
    
    // Create parent directory if it doesn't exist
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create directory");
    }
    
    // Generate minimal valid .glb file with cube geometry
    let glb_data = generate_cube_glb();
    
    let mut file = File::create(output_path).expect("Failed to create file");
    file.write_all(&glb_data).expect("Failed to write file");
    
    println!("✅ Created Welcome Cube at: {:?}", output_path);
}

fn generate_cube_glb() -> Vec<u8> {
    // GLB file structure:
    // - 12-byte header
    // - JSON chunk (glTF data)
    // - BIN chunk (binary buffer data)
    
    // Cube vertices (1.96133m = Space Grade Ready size)
    let size = 1.96133f32 / 2.0; // Half-size for centered cube
    
    // 24 vertices (4 per face, 6 faces) with normals
    let positions: Vec<f32> = vec![
        // Front face (+Z)
        -size, -size,  size,  size, -size,  size,  size,  size,  size, -size,  size,  size,
        // Back face (-Z)
        -size, -size, -size, -size,  size, -size,  size,  size, -size,  size, -size, -size,
        // Top face (+Y)
        -size,  size, -size, -size,  size,  size,  size,  size,  size,  size,  size, -size,
        // Bottom face (-Y)
        -size, -size, -size,  size, -size, -size,  size, -size,  size, -size, -size,  size,
        // Right face (+X)
         size, -size, -size,  size,  size, -size,  size,  size,  size,  size, -size,  size,
        // Left face (-X)
        -size, -size, -size, -size, -size,  size, -size,  size,  size, -size,  size, -size,
    ];
    
    let normals: Vec<f32> = vec![
        // Front (+Z)
        0.0, 0.0, 1.0,  0.0, 0.0, 1.0,  0.0, 0.0, 1.0,  0.0, 0.0, 1.0,
        // Back (-Z)
        0.0, 0.0, -1.0,  0.0, 0.0, -1.0,  0.0, 0.0, -1.0,  0.0, 0.0, -1.0,
        // Top (+Y)
        0.0, 1.0, 0.0,  0.0, 1.0, 0.0,  0.0, 1.0, 0.0,  0.0, 1.0, 0.0,
        // Bottom (-Y)
        0.0, -1.0, 0.0,  0.0, -1.0, 0.0,  0.0, -1.0, 0.0,  0.0, -1.0, 0.0,
        // Right (+X)
        1.0, 0.0, 0.0,  1.0, 0.0, 0.0,  1.0, 0.0, 0.0,  1.0, 0.0, 0.0,
        // Left (-X)
        -1.0, 0.0, 0.0,  -1.0, 0.0, 0.0,  -1.0, 0.0, 0.0,  -1.0, 0.0, 0.0,
    ];
    
    // Indices (2 triangles per face, 6 faces = 36 indices)
    let indices: Vec<u16> = vec![
        0,1,2, 0,2,3,       // Front
        4,5,6, 4,6,7,       // Back
        8,9,10, 8,10,11,    // Top
        12,13,14, 12,14,15, // Bottom
        16,17,18, 16,18,19, // Right
        20,21,22, 20,22,23, // Left
    ];
    
    // Convert to bytes
    let mut buffer = Vec::new();
    for &v in &positions {
        buffer.extend_from_slice(&v.to_le_bytes());
    }
    for &n in &normals {
        buffer.extend_from_slice(&n.to_le_bytes());
    }
    for &i in &indices {
        buffer.extend_from_slice(&i.to_le_bytes());
    }
    
    let positions_byte_length = positions.len() * 4;
    let normals_byte_length = normals.len() * 4;
    let indices_byte_length = indices.len() * 2;
    
    // glTF JSON
    let gltf_json = format!(r#"{{
  "asset": {{"version": "2.0", "generator": "Eustress Engine"}},
  "scene": 0,
  "scenes": [{{"name": "WelcomeCube", "nodes": [0]}}],
  "nodes": [{{"name": "Welcome Cube", "mesh": 0}}],
  "meshes": [{{
    "name": "Cube",
    "primitives": [{{
      "attributes": {{"POSITION": 0, "NORMAL": 1}},
      "indices": 2,
      "material": 0
    }}]
  }}],
  "materials": [{{
    "name": "Green Plastic",
    "pbrMetallicRoughness": {{
      "baseColorFactor": [0.4, 0.9, 0.6, 1.0],
      "metallicFactor": 0.0,
      "roughnessFactor": 0.8
    }}
  }}],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 24, "type": "VEC3", "max": [{size},{size},{size}], "min": [{},{},{}]}},
    {{"bufferView": 1, "componentType": 5126, "count": 24, "type": "VEC3"}},
    {{"bufferView": 2, "componentType": 5123, "count": 36, "type": "SCALAR"}}
  ],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": {positions_byte_length}}},
    {{"buffer": 0, "byteOffset": {positions_byte_length}, "byteLength": {normals_byte_length}}},
    {{"buffer": 0, "byteOffset": {}, "byteLength": {indices_byte_length}}}
  ],
  "buffers": [{{"byteLength": {}}}]
}}"#, -size, -size, -size, positions_byte_length + normals_byte_length, buffer.len());
    
    let json_bytes = gltf_json.as_bytes();
    let json_length = json_bytes.len();
    let json_padding = (4 - (json_length % 4)) % 4;
    let json_chunk_length = json_length + json_padding;
    
    let bin_length = buffer.len();
    let bin_padding = (4 - (bin_length % 4)) % 4;
    let bin_chunk_length = bin_length + bin_padding;
    
    let total_length = 12 + 8 + json_chunk_length + 8 + bin_chunk_length;
    
    let mut glb = Vec::new();
    
    // GLB header
    glb.extend_from_slice(b"glTF");                    // magic
    glb.extend_from_slice(&2u32.to_le_bytes());        // version
    glb.extend_from_slice(&(total_length as u32).to_le_bytes()); // length
    
    // JSON chunk
    glb.extend_from_slice(&(json_chunk_length as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(json_bytes);
    glb.extend_from_slice(&vec![0x20; json_padding]); // Space padding
    
    // BIN chunk
    glb.extend_from_slice(&(bin_chunk_length as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&buffer);
    glb.extend_from_slice(&vec![0; bin_padding]);
    
    glb
}

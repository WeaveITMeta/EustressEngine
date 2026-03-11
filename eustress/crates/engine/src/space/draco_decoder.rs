/// Draco mesh compression detection for KHR_draco_mesh_compression GLTF extension
/// 
/// This module detects Draco-compressed GLB files and provides guidance for decompression.
/// Bevy 0.18 does not support Draco compression by default, so users need to decompress
/// their GLB files before loading.

use std::path::Path;
use bevy::prelude::*;

/// Check if a GLB file uses Draco compression by examining its JSON chunk
pub fn is_draco_compressed(glb_path: &Path) -> bool {
    let data = match std::fs::read(glb_path) {
        Ok(d) => d,
        Err(_) => return false,
    };
    
    // GLB format: 12-byte header + chunks
    if data.len() < 20 || &data[0..4] != b"glTF" {
        return false;
    }
    
    // First chunk should be JSON
    let json_length = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    let json_type = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    
    // JSON chunk type is 0x4E4F534A ("JSON" in little-endian)
    if json_type != 0x4E4F534A || data.len() < 20 + json_length {
        return false;
    }
    
    // Check if the JSON mentions Draco compression
    if let Ok(json_str) = std::str::from_utf8(&data[20..20 + json_length]) {
        return json_str.contains("KHR_draco_mesh_compression");
    }
    
    false
}

/// Log a warning about Draco-compressed files that need decompression
pub fn warn_draco_file(path: &Path) {
    error!(
        "❌ GLB file uses Draco compression which Bevy doesn't support: {:?}\n\
         \n\
         To fix this, decompress the file using gltf-transform CLI:\n\
         \n\
         1. Install: npm install -g @gltf-transform/cli\n\
         2. Decompress: gltf-transform decompile \"{}\" \"{}\"",
        path,
        path.display(),
        path.display()
    );
}

/// Check all GLB files in a directory for Draco compression
pub fn check_directory_for_draco(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut draco_files = Vec::new();
    
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "glb" || ext == "gltf" {
                        if is_draco_compressed(&path) {
                            draco_files.push(path);
                        }
                    }
                }
            } else if path.is_dir() {
                draco_files.extend(check_directory_for_draco(&path));
            }
        }
    }
    
    draco_files
}

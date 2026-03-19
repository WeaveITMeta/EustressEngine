//! Automatic .txt to .toml Converter
//!
//! Watches for .txt files in the project directory and automatically converts them to .toml files.
//! This is a workaround for file systems that don't allow direct .toml file creation.

use bevy::prelude::*;
use notify::{Watcher, RecursiveMode, Event, EventKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Plugin for automatic .txt to .toml conversion
pub struct TxtToTomlWatcherPlugin;

impl Plugin for TxtToTomlWatcherPlugin {
    fn build(&self, app: &mut App) {
        // Initialize the watcher resource manually to handle potential failures
        if let Some(watcher) = TxtToTomlWatcher::new() {
            app.insert_resource(watcher)
                .add_systems(Update, process_txt_conversions);
        } else {
            warn!("TxtToTomlWatcher: Failed to initialize file watcher");
        }
    }
}

/// Resource that manages the file watcher
/// Uses Arc<Mutex<>> for thread-safe access to the receiver
#[derive(Resource)]
pub struct TxtToTomlWatcher {
    /// Path to the project root
    project_root: PathBuf,
    /// Receiver for file system events (wrapped for thread safety)
    receiver: Arc<Mutex<std::sync::mpsc::Receiver<notify::Result<Event>>>>,
    /// The watcher itself (must be kept alive, wrapped for Sync)
    _watcher: Arc<Mutex<Box<dyn Watcher + Send>>>,
}

impl TxtToTomlWatcher {
    fn new() -> Option<Self> {
        let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let (tx, rx) = std::sync::mpsc::channel();
        
        // Create the watcher
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => {
                warn!("Failed to create file watcher: {}", e);
                return None;
            }
        };
        
        // Watch the project root recursively
        if let Err(e) = watcher.watch(&project_root, RecursiveMode::Recursive) {
            warn!("Failed to watch project directory: {}", e);
            return None;
        }
        
        info!("📁 TxtToTomlWatcher: Monitoring {} for .txt files", project_root.display());
        
        Some(Self {
            project_root,
            receiver: Arc::new(Mutex::new(rx)),
            _watcher: Arc::new(Mutex::new(Box::new(watcher))),
        })
    }
}

/// System that processes file events and converts .txt to .toml
fn process_txt_conversions(watcher: Res<TxtToTomlWatcher>) {
    // Try to lock the receiver (non-blocking)
    let Ok(receiver) = watcher.receiver.try_lock() else { return };
    
    // Process all pending events (non-blocking)
    while let Ok(event_result) = receiver.try_recv() {
        match event_result {
            Ok(event) => {
                // Only process Create and Modify events
                if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    for path in event.paths {
                        // Only convert .txt files that have .toml in the filename
                        // Example: Baseplate.part.toml.txt → Baseplate.part.toml
                        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                            if file_name.ends_with(".toml.txt") {
                                if let Err(e) = convert_txt_to_toml(&path) {
                                    error!("Failed to convert {}: {}", path.display(), e);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!("File watcher error: {}", e);
            }
        }
    }
}

/// Convert a .toml.txt file to .toml by removing the .txt extension
/// Example: Baseplate.part.toml.txt → Baseplate.part.toml
fn convert_txt_to_toml(txt_path: &Path) -> std::io::Result<()> {
    // Check if file still exists (might have been deleted already)
    if !txt_path.exists() {
        return Ok(());
    }
    
    // Remove the .txt extension to get .toml
    // Example: "Baseplate.part.toml.txt" → "Baseplate.part.toml"
    let toml_path = txt_path.with_extension("");
    
    // Check if .toml already exists (avoid overwriting)
    if toml_path.exists() {
        warn!("Skipping conversion: {} already exists", toml_path.display());
        return Ok(());
    }
    
    // Read the content from .txt
    let content = std::fs::read_to_string(txt_path)?;
    
    // Write to .toml
    std::fs::write(&toml_path, content)?;
    
    // Delete the original .txt file
    std::fs::remove_file(txt_path)?;
    
    info!("✅ Auto-converted: {} → {}", 
        txt_path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
        toml_path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    );
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    
    #[test]
    fn test_txt_to_toml_conversion() {
        let temp_dir = std::env::temp_dir();
        let txt_file = temp_dir.join("test_baseplate.part.txt");
        let toml_file = temp_dir.join("test_baseplate.part.toml");
        
        // Clean up any existing files
        let _ = fs::remove_file(&txt_file);
        let _ = fs::remove_file(&toml_file);
        
        // Create a test .txt file
        let test_content = r#"[instance]
name = "Baseplate"
class_name = "Part"
"#;
        fs::write(&txt_file, test_content).unwrap();
        
        // Convert it
        convert_txt_to_toml(&txt_file).unwrap();
        
        // Verify .toml exists and .txt is gone
        assert!(toml_file.exists(), ".toml file should exist");
        assert!(!txt_file.exists(), ".txt file should be deleted");
        
        // Verify content
        let toml_content = fs::read_to_string(&toml_file).unwrap();
        assert_eq!(toml_content, test_content);
        
        // Clean up
        let _ = fs::remove_file(&toml_file);
    }
}

//! # Soul Hot Reload
//!
//! File watching and hot reload for Soul scripts.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// ============================================================================
// Hot Reload Watcher
// ============================================================================

/// Watches for script file changes
#[derive(Resource)]
pub struct HotReloadWatcher {
    /// Watched directories
    watch_dirs: Vec<PathBuf>,
    /// File modification times
    file_times: HashMap<PathBuf, std::time::SystemTime>,
    /// Changed files pending reload
    changed: HashSet<String>,
    /// Last poll time
    last_poll: Instant,
    /// Poll interval
    poll_interval: Duration,
    /// Is watching enabled?
    enabled: bool,
}

impl Default for HotReloadWatcher {
    fn default() -> Self {
        Self {
            watch_dirs: vec![PathBuf::from("./scripts")],
            file_times: HashMap::new(),
            changed: HashSet::new(),
            last_poll: Instant::now(),
            poll_interval: Duration::from_millis(500),
            enabled: true,
        }
    }
}

impl HotReloadWatcher {
    /// Create with custom directories
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self {
            watch_dirs: dirs,
            ..Default::default()
        }
    }
    
    /// Add a directory to watch
    pub fn watch_dir(&mut self, dir: PathBuf) {
        if !self.watch_dirs.contains(&dir) {
            self.watch_dirs.push(dir);
        }
    }
    
    /// Remove a directory from watch
    pub fn unwatch_dir(&mut self, dir: &Path) {
        self.watch_dirs.retain(|d| d != dir);
    }
    
    /// Poll for changes
    pub fn poll_changes(&mut self) -> Vec<String> {
        if !self.enabled {
            return Vec::new();
        }
        
        // Check if enough time has passed
        if self.last_poll.elapsed() < self.poll_interval {
            return Vec::new();
        }
        self.last_poll = Instant::now();
        
        let mut changes = Vec::new();
        
        // Scan all watch directories
        for dir in &self.watch_dirs.clone() {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    
                    // Only watch .md files
                    if path.extension().map(|e| e == "md").unwrap_or(false) {
                        if let Ok(metadata) = std::fs::metadata(&path) {
                            if let Ok(modified) = metadata.modified() {
                                // Check if file is new or modified
                                let is_changed = self.file_times
                                    .get(&path)
                                    .map(|&prev| prev != modified)
                                    .unwrap_or(true);
                                
                                if is_changed {
                                    self.file_times.insert(path.clone(), modified);
                                    
                                    // Convert to script ID
                                    let script_id = path.file_stem()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    
                                    changes.push(script_id.clone());
                                    self.changed.insert(script_id);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        changes
    }
    
    /// Get all pending changes
    pub fn pending_changes(&self) -> Vec<String> {
        self.changed.iter().cloned().collect()
    }
    
    /// Clear pending changes
    pub fn clear_pending(&mut self) {
        self.changed.clear();
    }
    
    /// Mark a change as processed
    pub fn mark_processed(&mut self, script_id: &str) {
        self.changed.remove(script_id);
    }
    
    /// Enable/disable watching
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    /// Is watching enabled?
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    
    /// Set poll interval
    pub fn set_poll_interval(&mut self, interval: Duration) {
        self.poll_interval = interval;
    }
    
    /// Get watched directories
    pub fn watched_dirs(&self) -> &[PathBuf] {
        &self.watch_dirs
    }
    
    /// Get number of tracked files
    pub fn tracked_file_count(&self) -> usize {
        self.file_times.len()
    }
    
    /// Force rescan all files
    pub fn force_rescan(&mut self) {
        self.file_times.clear();
        self.last_poll = Instant::now() - self.poll_interval;
    }
}

// ============================================================================
// Hot Reload Manager
// ============================================================================

/// Manages the hot reload process
pub struct HotReloadManager {
    /// Watcher
    watcher: HotReloadWatcher,
    /// Reload callbacks
    callbacks: Vec<Box<dyn Fn(&str) + Send + Sync>>,
    /// Reload history
    history: Vec<ReloadEntry>,
    /// Max history entries
    max_history: usize,
}

/// A reload history entry
#[derive(Debug, Clone)]
pub struct ReloadEntry {
    /// Script ID
    pub script_id: String,
    /// Timestamp
    pub timestamp: Instant,
    /// Was successful?
    pub success: bool,
    /// Duration (ms)
    pub duration_ms: u64,
    /// Error message (if failed)
    pub error: Option<String>,
}

impl Default for HotReloadManager {
    fn default() -> Self {
        Self {
            watcher: HotReloadWatcher::default(),
            callbacks: Vec::new(),
            history: Vec::new(),
            max_history: 100,
        }
    }
}

impl HotReloadManager {
    /// Add a reload callback
    pub fn on_reload<F>(&mut self, callback: F)
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.callbacks.push(Box::new(callback));
    }
    
    /// Process pending reloads
    pub fn process(&mut self) -> Vec<String> {
        let changes = self.watcher.poll_changes();
        
        for script_id in &changes {
            let start = Instant::now();
            
            // Call all callbacks
            for callback in &self.callbacks {
                callback(script_id);
            }
            
            // Record in history
            self.history.push(ReloadEntry {
                script_id: script_id.clone(),
                timestamp: Instant::now(),
                success: true,
                duration_ms: start.elapsed().as_millis() as u64,
                error: None,
            });
            
            // Trim history
            while self.history.len() > self.max_history {
                self.history.remove(0);
            }
        }
        
        changes
    }
    
    /// Get reload history
    pub fn history(&self) -> &[ReloadEntry] {
        &self.history
    }
    
    /// Get watcher
    pub fn watcher(&self) -> &HotReloadWatcher {
        &self.watcher
    }
    
    /// Get mutable watcher
    pub fn watcher_mut(&mut self) -> &mut HotReloadWatcher {
        &mut self.watcher
    }
}

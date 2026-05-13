//! Canonical cross-crate broadcast for filesystem changes.
//!
//! ## Why this exists
//!
//! For a long time the workspace ran two parallel `notify`-based file
//! watchers — one in `engine::space::file_watcher` (drives ECS
//! hot-reload) and one in `common::streaming::toml_watcher` (drives
//! the `SpatialChunkGrid` "Hot RAM tier"). Both watched the same
//! `Workspace/` tree, both opened the same files. Under Windows
//! share-mode rules they raced on every write: one watcher's read
//! would lock a TOML the engine was mid-write to, the save would fail
//! with `os error 32`, and the user's edit silently vanished.
//!
//! Every subsystem-that-cares-about-disk now reads
//! [`FileChanged`] messages emitted by a SINGLE watcher (the engine's
//! `SpaceFileWatcher`). Adding a new subscriber is a Bevy
//! `MessageReader<FileChanged>` parameter on its system, not a new
//! `notify::Watcher` instance. No more concurrent reads, no more
//! save-eating file-lock races.
//!
//! ## Payload shape
//!
//! The event carries just `path` + `kind`. Concrete-typed details
//! (`FileType`, `service`, `InstanceId`, …) live in the consumer
//! crates; making this payload "minimum-viable" keeps the message
//! type free of cross-crate type dependencies — `engine::FileType`
//! can stay in engine, `common::InstanceId` can stay in common, and
//! each side parses what it needs from the path itself.
//!
//! ## Ordering
//!
//! The engine's watcher fires `FileChanged` AFTER its own internal
//! hot-reload pass for an event, so external subscribers see the
//! event with the engine's ECS already updated. Subsystems that need
//! to read the file's current content (e.g. streaming grid loaders)
//! can safely `std::fs::read_to_string` the path on the same frame
//! they receive the event — the engine's processing is complete.

use bevy::ecs::message::Message;
use std::path::PathBuf;

/// "Something on disk changed" broadcast. Emitted by the engine's
/// single notify watcher; read by ECS hot-reload, the streaming
/// spatial grid, plugin hosts, and any future subscriber that needs
/// to react to filesystem changes.
///
/// See the module-level docs for the design rationale.
#[derive(Message, Debug, Clone)]
pub struct FileChanged {
    /// Absolute path of the file that changed.
    pub path: PathBuf,
    /// Kind of change — `notify`'s `Created`, `Modified`, `Removed`
    /// debounced and normalised to these three kinds. Subscribers
    /// that need the finer-grained `notify::EventKind` are out of
    /// luck for now; the union has been sufficient for every
    /// in-tree consumer so far.
    pub kind: FileChangeKind,
}

/// Coarse-grained classification of a file change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    /// File appeared on disk (never registered before, OR re-appeared
    /// after a delete — consumers that care about the distinction
    /// must consult their own state, not the event).
    Created,
    /// File contents or metadata changed in place.
    Modified,
    /// File no longer exists.
    Removed,
}

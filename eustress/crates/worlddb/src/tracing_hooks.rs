//! `tracing` → `inferno` bridge — Tier 1 #3 of the Fjall fork value zones.
//!
//! Every DB operation in [`crate::fjall_backend::FjallWorldDb`] is
//! already wrapped in `tracing::info_span!` / `tracing::trace_span!`.
//! This module sets up a `tracing_subscriber` layer that writes the
//! spans into the **folded-stack** format `inferno-flamegraph` consumes.
//!
//! The output file is what the engine's diagnostics overlay reads to
//! render an in-engine flamegraph alongside `frame_diagnostics.rs`.
//!
//! ## Usage
//!
//! ```ignore
//! let _guard = eustress_worlddb::tracing_hooks::FoldedStackWriter::install(
//!     "target/worlddb-spans.folded",
//! )?;
//! // ... run engine ...
//! drop(_guard); // flushes the folded-stack file
//! // Then: inferno-flamegraph < target/worlddb-spans.folded > flame.svg
//! ```
//!
//! ## Why folded-stack, not flamegraph-svg directly
//!
//! `inferno` is two crates: a folded-stack reader and an SVG renderer.
//! We capture the folded-stack format here (lightweight, append-only,
//! cheap per-event) and let the SVG render run as a separate, opt-in
//! step that doesn't compete with the frame budget.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;

use crate::error::Result;

/// RAII handle returned by [`FoldedStackWriter::install`]. Dropping
/// flushes the file. Hand to a long-lived owner (Bevy resource) to
/// keep capture running for the engine session.
pub struct FoldedStackWriter {
    _inner: std::sync::Arc<Mutex<BufWriter<File>>>,
}

impl FoldedStackWriter {
    /// Open `path` and install a folded-stack writer that mirrors the
    /// `eustress_worlddb::*` spans into it. The caller's existing
    /// `tracing_subscriber` (set by the engine main) keeps receiving
    /// everything — this is additive.
    ///
    /// **Phase 4 today — handle is a no-op shell.** The real layer
    /// wiring depends on `tracing_subscriber`'s `Layer` trait surface
    /// which we don't link into the crate (would pull a tree of deps
    /// the engine already brings in). Phase 4 will switch to using
    /// the engine's `tracing_subscriber` instance directly so the
    /// folded-stack lines hang off the same dispatcher.
    pub fn install(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::create(path.as_ref())?;
        let writer = Mutex::new(BufWriter::new(file));
        tracing::info!(
            target: "eustress_worlddb::tracing_hooks",
            "FoldedStackWriter installed — Phase 4 stub, actual span capture pending engine wiring"
        );
        Ok(Self {
            _inner: std::sync::Arc::new(writer),
        })
    }

    /// Manual append — used by the engine plugin's tick system to
    /// emit per-frame summary entries (`frame_n;world_db;commit 123`
    /// → flamegraph col). Returns the number of bytes written.
    pub fn append_folded(&self, line: &str) -> Result<usize> {
        let mut w = self._inner.lock().unwrap();
        let bytes = format!("{line}\n");
        w.write_all(bytes.as_bytes())?;
        Ok(bytes.len())
    }
}

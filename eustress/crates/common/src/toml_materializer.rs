//! # TOML Materializer — EustressStream subscriber
//!
//! Subscribes to the `scene_deltas` EustressStream topic, applies each
//! `SceneDelta` to an in-memory `SceneMirror`, and debounces async TOML
//! writes to disk.
//!
//! ## Architecture
//!
//! ```text
//! EustressStream "scene_deltas" topic
//!     → flume channel (subscribe_channel)
//!         → background tokio task (run_materializer_loop)
//!             → SceneMirror::apply(delta)
//!                 → every 200ms (debounce): to_toml_string()
//!                     → tokio::fs::write(.eustress/current.toml)
//!                     → tokio::fs::write(scenes/main.toml)
//! ```
//!
//! ## Feature Gate
//! Compiled only when the `iggy-streaming` feature is enabled.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use eustress_stream::{EustressStream, OwnedMessage};

use crate::iggy_delta::{
    DeltaKind, NamePayload, PartPayload, SceneDelta, TransformPayload,
    IGGY_DEFAULT_URL, IGGY_TOPIC_SCENE_DELTAS,
};

// ─────────────────────────────────────────────────────────────────────────────
// MaterializerConfig
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MaterializerConfig {
    /// Legacy field — retained for API compatibility, not used.
    pub iggy_url: String,
    pub consumer_group: String,
    pub debounce: Duration,
    pub hot_output_path: PathBuf,
    pub canonical_output_path: PathBuf,
    pub poll_batch: u32,
}

impl Default for MaterializerConfig {
    fn default() -> Self {
        Self {
            iggy_url: IGGY_DEFAULT_URL.to_string(),
            consumer_group: "toml-materializer".to_string(),
            debounce: Duration::from_millis(200),
            hot_output_path: PathBuf::from(".eustress/current.toml"),
            canonical_output_path: PathBuf::from("scenes/main.toml"),
            poll_batch: 512,
        }
    }
}

impl bevy::prelude::Resource for MaterializerConfig {}

// ─────────────────────────────────────────────────────────────────────────────
// MirrorEntity / SceneMirror
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MirrorEntity {
    pub entity: u64,
    pub name: String,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub color: [f32; 4],
    pub material: u16,
    pub size: [f32; 3],
    pub anchored: bool,
    pub can_collide: bool,
    pub transparency: f32,
    pub reflectance: f32,
    pub parent: Option<u64>,
    pub last_seq: u64,
}

impl MirrorEntity {
    fn apply_transform(&mut self, t: &TransformPayload, seq: u64) {
        self.position = t.position;
        self.rotation = t.rotation;
        self.scale = t.scale;
        self.last_seq = seq;
    }

    fn apply_part(&mut self, p: &PartPayload, seq: u64) {
        if let Some(c) = p.color { self.color = c; }
        if let Some(m) = p.material { self.material = m; }
        if let Some(s) = p.size { self.size = s; }
        if let Some(a) = p.anchored { self.anchored = a; }
        if let Some(cc) = p.can_collide { self.can_collide = cc; }
        if let Some(tr) = p.transparency { self.transparency = tr; }
        if let Some(r) = p.reflectance { self.reflectance = r; }
        self.last_seq = seq;
    }

    fn apply_name(&mut self, n: &NamePayload, seq: u64) {
        self.name = n.name.clone();
        self.last_seq = seq;
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SceneMirror {
    pub version: u32,
    pub session_id: String,
    pub max_seq: u64,
    pub entities: HashMap<u64, MirrorEntity>,
}

impl SceneMirror {
    pub fn new(session_id: String) -> Self {
        Self { version: 1, session_id, max_seq: 0, entities: HashMap::new() }
    }

    /// Apply a single `SceneDelta`. Returns `true` if the mirror changed.
    pub fn apply(&mut self, delta: &SceneDelta) -> bool {
        self.max_seq = self.max_seq.max(delta.seq);

        let default_entity = || MirrorEntity {
            entity: delta.entity,
            scale: [1.0, 1.0, 1.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            color: [0.639, 0.635, 0.647, 1.0],
            ..Default::default()
        };

        match delta.kind {
            DeltaKind::PartAdded => {
                self.entities.entry(delta.entity).or_insert_with(default_entity);
            }
            DeltaKind::PartRemoved => {
                self.entities.remove(&delta.entity);
            }
            DeltaKind::TransformChanged => {
                if let Some(t) = &delta.transform {
                    self.entities
                        .entry(delta.entity)
                        .or_insert_with(default_entity)
                        .apply_transform(t, delta.seq);
                }
            }
            DeltaKind::PartPropertiesChanged => {
                if let Some(p) = &delta.part {
                    self.entities
                        .entry(delta.entity)
                        .or_insert_with(default_entity)
                        .apply_part(p, delta.seq);
                }
            }
            DeltaKind::Renamed => {
                if let Some(n) = &delta.name {
                    if let Some(e) = self.entities.get_mut(&delta.entity) {
                        e.apply_name(n, delta.seq);
                    }
                }
            }
            DeltaKind::Reparented => {
                if let Some(e) = self.entities.get_mut(&delta.entity) {
                    e.parent = delta.new_parent;
                    e.last_seq = delta.seq;
                }
            }
            _ => return false,
        }
        true
    }

    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// spawn_toml_materializer
// ─────────────────────────────────────────────────────────────────────────────

/// Spawn the background TOML materializer. Returns immediately.
///
/// The `stream` parameter must be the same `EustressStream` that the engine
/// uses for scene deltas (clone from `IggyChangeQueue.stream`).
pub async fn spawn_toml_materializer(
    config: MaterializerConfig,
    session_id: String,
    stream: EustressStream,
) {
    let (tx, rx) = eustress_stream::flume::unbounded::<OwnedMessage>();

    if let Err(e) = stream.subscribe_channel(IGGY_TOPIC_SCENE_DELTAS, tx) {
        warn!("TomlMaterializer: failed to subscribe to scene_deltas: {e}");
        return;
    }

    info!("TomlMaterializer: subscribed to scene_deltas, starting materializer loop.");

    let mirror = Arc::new(Mutex::new(SceneMirror::new(session_id)));
    let mirror_shutdown = Arc::clone(&mirror);
    let config_shutdown = config.clone();

    // Shutdown flush hook — uses a long sleep as a simple keepalive since
    // tokio::signal requires the "signal" feature which common doesn't gate on.
    tokio::spawn(async move {
        // Park the task; the main loop handles the actual materializer lifecycle.
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
        #[allow(unreachable_code)]
        {
            info!("TomlMaterializer: shutdown — final flush.");
            let m = mirror_shutdown.lock().await;
            let _ = write_mirror(&m, &config_shutdown).await;
        }
    });

    run_materializer_loop(rx, config, mirror).await;
}

async fn run_materializer_loop(
    rx: eustress_stream::flume::Receiver<OwnedMessage>,
    config: MaterializerConfig,
    mirror: Arc<Mutex<SceneMirror>>,
) {
    let mut last_write = Instant::now();
    let mut dirty = false;

    loop {
        // Drain all pending messages without blocking.
        let mut received = false;
        while let Ok(msg) = rx.try_recv() {
            received = true;
            match SceneDelta::from_bytes(msg.data.as_ref()) {
                Ok(delta) => {
                    let mut m = mirror.lock().await;
                    if m.apply(&delta) {
                        dirty = true;
                    }
                }
                Err(e) => warn!("TomlMaterializer: delta deserialize: {e}"),
            }
        }

        if dirty && last_write.elapsed() >= config.debounce {
            let m = mirror.lock().await;
            match write_mirror(&m, &config).await {
                Ok((bytes, _)) => {
                    info!("TomlMaterializer: wrote {} entities ({bytes}B)", m.entities.len());
                }
                Err(e) => error!("TomlMaterializer: write error: {e}"),
            }
            dirty = false;
            last_write = Instant::now();
        }

        if !received {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

async fn write_mirror(
    mirror: &SceneMirror,
    config: &MaterializerConfig,
) -> Result<(usize, usize), String> {
    let toml_str = mirror
        .to_toml_string()
        .map_err(|e| format!("TOML serialize: {e}"))?;
    let bytes = toml_str.as_bytes();

    if let Some(p) = config.hot_output_path.parent() {
        let _ = tokio::fs::create_dir_all(p).await;
    }
    if let Some(p) = config.canonical_output_path.parent() {
        let _ = tokio::fs::create_dir_all(p).await;
    }

    tokio::fs::write(&config.hot_output_path, bytes)
        .await
        .map_err(|e| format!("hot write {:?}: {e}", config.hot_output_path))?;

    tokio::fs::write(&config.canonical_output_path, bytes)
        .await
        .map_err(|e| format!("canonical write {:?}: {e}", config.canonical_output_path))?;

    Ok((bytes.len(), bytes.len()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Bevy Startup system
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy Startup system: spawns the TOML materializer on a background thread.
///
/// Requires `IggyChangeQueue` to be present (added by `IggyPlugin`).
pub fn start_toml_materializer_system(
    config: Option<bevy::prelude::Res<MaterializerConfig>>,
    queue: Option<bevy::prelude::Res<crate::iggy_queue::IggyChangeQueue>>,
) {
    let cfg = config.map(|c| c.clone()).unwrap_or_default();
    let session_id = format!("session-{}", uuid::Uuid::new_v4());

    let Some(queue) = queue else {
        warn!("TomlMaterializer: IggyChangeQueue not available — skipping materializer.");
        return;
    };

    let stream = queue.stream.clone();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("materializer rt");
        rt.block_on(spawn_toml_materializer(cfg, session_id, stream));
    });

    tracing::info!("TomlMaterializer: background task started.");
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iggy_delta::{DeltaKind, TransformPayload};

    fn make_transform(entity: u64, seq: u64, x: f32) -> SceneDelta {
        SceneDelta::transform(
            entity, seq, seq * 16,
            TransformPayload {
                position: [x, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
            },
        )
    }

    #[test]
    fn mirror_add_move_remove() {
        let mut m = SceneMirror::new("test".to_string());
        assert!(m.apply(&SceneDelta::lifecycle(1, DeltaKind::PartAdded, 0, 0)));
        assert!(m.entities.contains_key(&1));
        assert!(m.apply(&make_transform(1, 1, 5.0)));
        assert_eq!(m.entities[&1].position, [5.0, 0.0, 0.0]);
        assert!(m.apply(&SceneDelta::lifecycle(1, DeltaKind::PartRemoved, 2, 32)));
        assert!(!m.entities.contains_key(&1));
    }

    #[test]
    fn mirror_toml_roundtrip() {
        let mut m = SceneMirror::new("rt".to_string());
        m.apply(&SceneDelta::lifecycle(42, DeltaKind::PartAdded, 0, 0));
        m.apply(&make_transform(42, 1, 10.0));
        let s = m.to_toml_string().unwrap();
        let r: SceneMirror = toml::from_str(&s).unwrap();
        assert_eq!(r.entities[&42].position, [10.0, 0.0, 0.0]);
    }
}

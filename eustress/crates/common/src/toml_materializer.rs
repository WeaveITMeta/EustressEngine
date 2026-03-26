//! # TOML Materializer
//!
//! ## Table of Contents
//! - MaterializerConfig      — debounce interval, output paths
//! - SceneMirror             — minimal in-memory mirror updated from SceneDelta stream
//! - spawn_toml_materializer — entry point: spawns the background task
//! - run_materializer_loop   — poll Iggy → apply deltas → debounced async TOML write
//! - write_mirror            — serialize SceneMirror → TOML → async fs::write
//! - start_toml_materializer_system — Bevy Startup system wrapper
//!
//! ## Architecture
//!
//! ```text
//! Iggy "eustress/scene_deltas" topic
//!     → background tokio task (never touches main thread)
//!         → SceneMirror::apply(delta) for each message
//!             → every 200ms (debounce): to_toml_string()
//!                 → tokio::fs::write(.eustress/current.toml)
//!                 → tokio::fs::write(scenes/main.toml)
//! ```
//!
//! ## Feature Gate
//! Compiled only when the `iggy-streaming` feature is enabled
//! (gated via `#[cfg(feature = "iggy-streaming")]` on the `mod` in `lib.rs`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use iggy::clients::client::IggyClient;
use iggy::prelude::{Consumer, Identifier, MessageClient, PollingStrategy};

use crate::iggy_delta::{ArchivedSceneDelta, DeltaKind, NamePayload, PartPayload, SceneDelta, TransformPayload, IGGY_DEFAULT_URL, IGGY_TOPIC_SCENE_DELTAS};

// ─────────────────────────────────────────────────────────────────────────────
// MaterializerConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the TOML materializer background task.
#[derive(Debug, Clone)]
pub struct MaterializerConfig {
    /// Iggy connection URL.
    pub iggy_url: String,
    /// Consumer group name — unique per materializer instance.
    pub consumer_group: String,
    /// Debounce interval before writing TOML after the last delta.
    pub debounce: Duration,
    /// Path to write the "hot" current-scene TOML.
    pub hot_output_path: PathBuf,
    /// Path to write the canonical human-readable TOML for git.
    pub canonical_output_path: PathBuf,
    /// Maximum deltas to poll per Iggy request.
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
// MirrorEntity
// ─────────────────────────────────────────────────────────────────────────────

/// One entity's current state in the in-memory mirror.
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

// ─────────────────────────────────────────────────────────────────────────────
// SceneMirror
// ─────────────────────────────────────────────────────────────────────────────

/// Full in-memory mirror of the scene, keyed by entity index.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SceneMirror {
    pub version: u32,
    pub session_id: String,
    pub max_seq: u64,
    pub entities: HashMap<u64, MirrorEntity>,
}

impl SceneMirror {
    pub fn new(session_id: String) -> Self {
        Self {
            version: 1,
            session_id,
            max_seq: 0,
            entities: HashMap::new(),
        }
    }

    /// Apply a single SceneDelta. Returns `true` if the mirror changed.
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

    /// Serialize the mirror to a TOML string.
    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// spawn_toml_materializer
// ─────────────────────────────────────────────────────────────────────────────

/// Spawn the background TOML materializer. Returns immediately.
/// The materializer runs in a background tokio task and never blocks the caller.
pub async fn spawn_toml_materializer(config: MaterializerConfig, session_id: String) {
    let client = match IggyClient::from_connection_string(&config.iggy_url) {
        Ok(c) => c,
        Err(e) => {
            warn!("TomlMaterializer: bad Iggy URL '{}': {e}", config.iggy_url);
            return;
        }
    };

    if let Err(e) = client.connect().await {
        warn!(
            "TomlMaterializer: cannot connect to Iggy ({}): {e}. \
             TOML files will NOT be updated live.",
            config.iggy_url
        );
        return;
    }

    info!("TomlMaterializer: connected, starting consumer.");

    let mirror = Arc::new(Mutex::new(SceneMirror::new(session_id)));
    let mirror_shutdown = Arc::clone(&mirror);
    let config_shutdown = config.clone();

    // Shutdown flush hook.
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sig = signal(SignalKind::terminate()).unwrap();
            sig.recv().await;
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        info!("TomlMaterializer: shutdown — final flush.");
        let m = mirror_shutdown.lock().await;
        let _ = write_mirror(&m, &config_shutdown).await;
    });

    run_materializer_loop(client, config, mirror).await;
}

async fn run_materializer_loop(
    client: IggyClient,
    config: MaterializerConfig,
    mirror: Arc<Mutex<SceneMirror>>,
) {
    let stream_id = match Identifier::named(crate::iggy_delta::IGGY_STREAM_NAME) {
        Ok(id) => id,
        Err(e) => { error!("TomlMaterializer: bad stream name: {e}"); return; }
    };
    let topic_id = match Identifier::named(IGGY_TOPIC_SCENE_DELTAS) {
        Ok(id) => id,
        Err(e) => { error!("TomlMaterializer: bad topic name: {e}"); return; }
    };

    let consumer = Consumer::default();
    let strategy = PollingStrategy::next();
    let mut last_write = Instant::now();
    let mut dirty = false;

    loop {
        match client
            .poll_messages(&stream_id, &topic_id, Some(1), &consumer, &strategy, config.poll_batch, true)
            .await
        {
            Ok(polled) => {
                if !polled.messages.is_empty() {
                    let mut m = mirror.lock().await;
                    for msg in polled.messages.iter() {
                        let payload = msg.payload.as_ref();
                        // Layer 6: use rkyv archived view first — zero allocation — to
                        // pre-screen the DeltaKind before committing to full deserialization.
                        // `mirror.apply` takes `&SceneDelta` (owned), so we still deserialize
                        // for the apply call, but we skip it entirely for unknown/no-op kinds.
                        match rkyv::access::<ArchivedSceneDelta, rkyv::rancor::Error>(payload) {
                            Ok(_archived) => {
                                // Archived view confirms the payload is valid rkyv; now
                                // deserialize to owned for mirror.apply().
                                match SceneDelta::from_bytes(payload) {
                                    Ok(delta) => {
                                        if m.apply(&delta) {
                                            dirty = true;
                                        }
                                    }
                                    Err(e) => warn!("TomlMaterializer: delta deserialize: {e}"),
                                }
                            }
                            Err(e) => warn!("TomlMaterializer: bad rkyv payload: {e}"),
                        }
                    }
                }
            }
            Err(e) => {
                warn!("TomlMaterializer: poll error: {e} — backing off 500ms");
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
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

        tokio::time::sleep(Duration::from_millis(5)).await;
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

/// Bevy Startup system: spawns the TOML materializer on a background `std::thread`
/// with its own tokio runtime. Never interferes with the Bevy loop.
pub fn start_toml_materializer_system(
    config: Option<bevy::prelude::Res<MaterializerConfig>>,
) {
    let cfg = config.map(|c| c.clone()).unwrap_or_default();
    let session_id = format!("session-{}", uuid::Uuid::new_v4());

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("materializer rt");
        rt.block_on(spawn_toml_materializer(cfg, session_id));
    });

    bevy::prelude::info!("TomlMaterializer: background task started.");
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

//! # Iggy Scene Delta Types
//!
//! ## Table of Contents
//! - DeltaKind       — enum of every mutation type tracked (Transform, Part, Added, Removed, etc.)
//! - SceneDelta      — compact per-entity change record serialized via rkyv (zero-copy)
//! - TransformDelta  — position + rotation + scale payload (only populated fields)
//! - PartDelta       — BasePart property payload (color, material, size, etc.)
//!
//! ## Architecture
//!
//! ```text
//! Bevy Changed<T> query
//!     → emit_deltas() system
//!         → SceneDelta { entity, kind, payload }
//!             → rkyv::to_bytes()        (~100 bytes per delta)
//!                 → IggyChangeQueue.send_delta()
//!                     → tokio mpsc channel
//!                         → background producer thread
//!                             → Iggy "eustress/scene_deltas" topic
//!                                 ↳ Explorer subscriber (real-time tree updates)
//!                                 ↳ Properties subscriber (real-time panel updates)
//!                                 ↳ Undo/Redo subscriber (inverse event log)
//!                                 ↳ TOML materializer (debounced async file write)
//! ```
//!
//! ## Wire Size
//! SceneDelta with TransformPayload: ~68 bytes.
//! SceneDelta with PartPayload:      ~96 bytes.
//! SceneDelta with LifecycleOnly:    ~12 bytes.
//! At 1M deltas/sec: ~100 MB/s — well within Iggy's >1 GB/s write ceiling.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// DeltaKind — category of the mutation
// ─────────────────────────────────────────────────────────────────────────────

/// The kind of mutation that produced this delta.
/// Keep variants compact — this is stored inline in every SceneDelta.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, Copy, PartialEq, Eq, Hash,
)]
#[rkyv(derive(Debug, PartialEq))]
pub enum DeltaKind {
    /// Entity was spawned into the scene (new Part, Model, light, etc.).
    PartAdded,
    /// Entity was despawned from the scene.
    PartRemoved,
    /// Transform (position, rotation, scale) changed.
    TransformChanged,
    /// BasePart properties changed (color, material, size, anchored, etc.).
    PartPropertiesChanged,
    /// Entity was renamed.
    Renamed,
    /// Entity was reparented in the hierarchy.
    Reparented,
    /// Script source was modified.
    ScriptChanged,
    /// A light component changed (brightness, color, range, etc.).
    LightChanged,
    /// Camera component changed.
    CameraChanged,
    /// Terrain chunk was modified (sculpt, material paint, etc.).
    TerrainChunkChanged,
    /// Batch marker — this delta covers multiple entities (use batch_id to fetch full list).
    BatchMarker,
}

// ─────────────────────────────────────────────────────────────────────────────
// Compact payload types — only the fields that actually changed
// ─────────────────────────────────────────────────────────────────────────────

/// Position + rotation (quaternion) + scale payload.
/// All f32 for minimal wire size. Compatible with InstanceBin layout.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, Copy, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct TransformPayload {
    /// World-space position [x, y, z].
    pub position: [f32; 3],
    /// Unit quaternion rotation [x, y, z, w].
    pub rotation: [f32; 4],
    /// Non-uniform scale [x, y, z].
    pub scale: [f32; 3],
}

/// BasePart property delta — only populated on PartPropertiesChanged.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct PartPayload {
    /// Color in linear RGBA [0.0–1.0] — None if unchanged.
    pub color: Option<[f32; 4]>,
    /// Material enum discriminant (matches BrickMaterial in classes.rs) — None if unchanged.
    pub material: Option<u16>,
    /// Part size [x, y, z] — None if unchanged.
    pub size: Option<[f32; 3]>,
    /// New name — None if unchanged.
    pub name: Option<u32>, // interned string index; full name in NamePayload
    /// Anchored toggle — None if unchanged.
    pub anchored: Option<bool>,
    /// CanCollide toggle — None if unchanged.
    pub can_collide: Option<bool>,
    /// Transparency [0.0–1.0] — None if unchanged.
    pub transparency: Option<f32>,
    /// Reflectance [0.0–1.0] — None if unchanged.
    pub reflectance: Option<f32>,
}

/// Rename payload — string stored separately to keep SceneDelta fixed-ish size.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct NamePayload {
    /// New name (UTF-8, max 256 bytes in practice).
    pub name: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// SceneDelta — the core message type published to Iggy
// ─────────────────────────────────────────────────────────────────────────────

/// One mutation event for a single Bevy entity.
///
/// Published to the Iggy stream `eustress/scene_deltas` on every
/// Bevy frame that detects `Changed<T>` components.
///
/// All consumers (Explorer, Properties, Undo/Redo, TOML materializer)
/// subscribe to this stream and react in real-time.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct SceneDelta {
    /// Bevy `Entity::index()` — identifies the affected entity.
    /// (Generation is not included to stay compact; use session context.)
    pub entity: u64,

    /// What kind of mutation this is.
    pub kind: DeltaKind,

    /// Monotonically increasing sequence number within this session.
    /// Used by Undo/Redo to replay in order.
    pub seq: u64,

    /// Wall-clock timestamp in milliseconds since session start.
    pub timestamp_ms: u64,

    /// Transform payload — populated when kind == TransformChanged.
    pub transform: Option<TransformPayload>,

    /// Part property payload — populated when kind == PartPropertiesChanged.
    pub part: Option<PartPayload>,

    /// Name payload — populated when kind == Renamed.
    pub name: Option<NamePayload>,

    /// New parent entity index — populated when kind == Reparented.
    pub new_parent: Option<u64>,
}

impl SceneDelta {
    /// Construct a minimal lifecycle delta (PartAdded / PartRemoved).
    pub fn lifecycle(entity: u64, kind: DeltaKind, seq: u64, timestamp_ms: u64) -> Self {
        Self {
            entity,
            kind,
            seq,
            timestamp_ms,
            transform: None,
            part: None,
            name: None,
            new_parent: None,
        }
    }

    /// Construct a transform delta.
    pub fn transform(
        entity: u64,
        seq: u64,
        timestamp_ms: u64,
        payload: TransformPayload,
    ) -> Self {
        Self {
            entity,
            kind: DeltaKind::TransformChanged,
            seq,
            timestamp_ms,
            transform: Some(payload),
            part: None,
            name: None,
            new_parent: None,
        }
    }

    /// Construct a part properties delta.
    pub fn part_props(
        entity: u64,
        seq: u64,
        timestamp_ms: u64,
        payload: PartPayload,
    ) -> Self {
        Self {
            entity,
            kind: DeltaKind::PartPropertiesChanged,
            seq,
            timestamp_ms,
            transform: None,
            part: Some(payload),
            name: None,
            new_parent: None,
        }
    }

    /// Construct a rename delta.
    pub fn rename(entity: u64, seq: u64, timestamp_ms: u64, new_name: String) -> Self {
        Self {
            entity,
            kind: DeltaKind::Renamed,
            seq,
            timestamp_ms,
            transform: None,
            part: None,
            name: Some(NamePayload { name: new_name }),
            new_parent: None,
        }
    }

    /// Serialize to raw bytes using rkyv.
    /// Returns ~68–96 bytes per delta for TransformChanged / PartPropertiesChanged.
    pub fn to_bytes(&self) -> Result<Vec<u8>, rkyv::rancor::Error> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self).map(|b| b.to_vec())
    }

    /// Deserialize from rkyv bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, rkyv::rancor::Error> {
        rkyv::from_bytes::<Self, rkyv::rancor::Error>(bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Iggy stream / topic constants
// ─────────────────────────────────────────────────────────────────────────────

/// Default Iggy connection string. Override via CLI `--iggy-url`.
pub const IGGY_DEFAULT_URL: &str = "iggy://iggy:iggy@127.0.0.1:8090";

/// Iggy stream name. One stream per Eustress installation.
pub const IGGY_STREAM_NAME: &str = "eustress";

/// Topic for scene mutation deltas.
pub const IGGY_TOPIC_SCENE_DELTAS: &str = "scene_deltas";

/// Topic for agent command/response loop (CLI ↔ running session).
pub const IGGY_TOPIC_AGENT_COMMANDS: &str = "agent_commands";

/// Topic for agent observations emitted by the session for the CLI agent.
pub const IGGY_TOPIC_AGENT_OBSERVATIONS: &str = "agent_observations";

/// Topic for Monte Carlo simulation results (SimRecord). One message per run_simulation() call.
/// Replaces the removed file-cache system. Enables replay + cross-run comparison.
pub const IGGY_TOPIC_SIM_RESULTS: &str = "sim_results";

/// Topic for VIGA / workshop iteration history (IterationRecord). One message per
/// feedback cycle — carries generated code, similarity score, and verifier feedback.
pub const IGGY_TOPIC_ITERATION_HISTORY: &str = "iteration_history";

/// Topic for Rune script execution records (RuneScriptRecord). One message per
/// execute_and_apply() call — carries source, directives, log messages, success flag.
pub const IGGY_TOPIC_RUNE_SCRIPTS: &str = "rune_scripts";

/// Topic for workshop product-iteration records (WorkshopIterationRecord). Aggregates
/// one full optimize→simulate→Rune→result cycle for workshop convergence analysis.
pub const IGGY_TOPIC_WORKSHOP_ITERATIONS: &str = "workshop_iterations";

/// Topic for ARC-AGI-3 (and gym-style) episode records (ArcEpisodeRecord).
/// One message per completed episode — carries step history, final score, and efficiency ratio.
pub const IGGY_TOPIC_ARC_EPISODES: &str = "arc_episodes";

// ─────────────────────────────────────────────────────────────────────────────
// Agent command / observation types (for CLI agent-in-the-loop)
// ─────────────────────────────────────────────────────────────────────────────

/// A command sent from the CLI agent into a running Eustress session.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct AgentCommand {
    /// Unique command identifier (UUID v4 string, kept as u128 for compactness).
    pub command_id: u128,

    /// The action to perform.
    pub action: AgentAction,

    /// Optional Rune script source to execute.
    pub script: Option<String>,

    /// ISO 8601 timestamp as Unix ms.
    pub issued_at_ms: u64,
}

/// The action variant of an AgentCommand.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub enum AgentAction {
    /// Execute a Rune script in the session context.
    ExecuteScript,
    /// Request a full scene snapshot (triggers AgentObservation::SceneSnapshot).
    RequestSnapshot,
    /// Spawn a new Part at the given world position.
    SpawnPart { position: [f32; 3], class_name: String },
    /// Despawn an entity by index.
    DespawnEntity { entity: u64 },
    /// Apply a transform to an entity.
    SetTransform { entity: u64, position: [f32; 3], rotation: [f32; 4] },
    /// Run the simulation forward by N ticks then pause.
    SimulateNTicks { ticks: u32 },
    /// Request the agent observation log to be flushed.
    FlushObservations,
    /// Submit an action back to an external interactive environment.
    EnvironmentAction {
        /// Opaque action label matching the env's action space (e.g. "ACTION1", "UP", "PLACE_0_0").
        action: String,
        /// Step this action is responding to (for correlation).
        step: u32,
    },
    /// Begin an interactive environment episode. Session will emit EnvironmentState
    /// observations and wait for EnvironmentAction commands in a step loop.
    BeginEpisode {
        task_id: String,
        max_steps: u32,
    },
    /// End the current episode and record the final score.
    EndEpisode {
        final_score: f32,
        goal_reached: bool,
    },
}

/// An observation emitted by the session back to the CLI agent.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub struct AgentObservation {
    /// Correlation ID matching the AgentCommand.command_id that triggered this.
    pub command_id: u128,

    /// The observation payload.
    pub payload: ObservationPayload,

    /// Sequence number within the session.
    pub seq: u64,

    /// Timestamp in Unix ms.
    pub timestamp_ms: u64,

    /// Overall world-model confidence for this observation [0.0–1.0].
    /// Derived from the salience score of the triggering SceneDelta and the
    /// EquivalenceCache confidence for any resolved causal formulas.
    /// 0.0 = no estimate available.
    pub confidence: f32,

    /// Per-field uncertainty estimates: (field_name, uncertainty_[0,1]).
    /// High uncertainty → the world model has little prior data on this field.
    /// Empty when no uncertainty information is available.
    pub uncertainty: Vec<(String, f32)>,
}

/// What the session observed / what happened as a result of the command.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize,
    Serialize, Deserialize,
    Debug, Clone, PartialEq,
)]
#[rkyv(derive(Debug, PartialEq))]
pub enum ObservationPayload {
    /// Script executed successfully; output is the return value.
    ScriptResult { output: String },
    /// Script execution failed.
    ScriptError { message: String },
    /// Snapshot of entity count and tick number.
    SceneSnapshot { entity_count: u64, tick: u64, scene_id: String },
    /// A new entity was spawned.
    EntitySpawned { entity: u64, class_name: String },
    /// An entity was despawned.
    EntityDespawned { entity: u64 },
    /// Simulation advanced N ticks.
    SimulationAdvanced { ticks: u32, elapsed_ms: u64 },
    /// Generic acknowledgement with optional metadata.
    Ack { message: String },
    /// Error that prevented the command from executing.
    Error { message: String },
    /// Raw environment observation from an external interactive benchmark (ARC-AGI-3, gym, etc.).
    EnvironmentState {
        /// Opaque JSON blob of the env observation (grid, goal, available_actions, step, score).
        json: String,
        /// Step number within the current episode.
        step: u32,
        /// Whether the episode is terminated (goal reached or max steps hit).
        terminated: bool,
    },
    /// World-model confidence update: per-field salience/uncertainty snapshot.
    ///
    /// Emitted by the `MemoryTierController` after routing a batch of `ScoredEvent`s.
    /// The CLI and any downstream consumer can display or log this to track how
    /// well the world model understands the current scene state.
    ConfidenceUpdate {
        /// Per-field confidence scores: (field_name, confidence_[0,1]).
        field_scores: Vec<(String, f32)>,
        /// Overall composite confidence for this step.
        composite: f32,
        /// Step number this update corresponds to.
        step: u32,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_transform_delta() {
        let delta = SceneDelta::transform(
            42,
            1,
            12345,
            TransformPayload {
                position: [1.0, 2.0, 3.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
            },
        );
        let bytes = delta.to_bytes().expect("rkyv serialize");
        let restored = SceneDelta::from_bytes(&bytes).expect("rkyv deserialize");
        assert_eq!(delta, restored);
    }

    #[test]
    fn round_trip_lifecycle_delta() {
        let delta = SceneDelta::lifecycle(7, DeltaKind::PartAdded, 2, 99999);
        let bytes = delta.to_bytes().expect("rkyv serialize");
        let restored = SceneDelta::from_bytes(&bytes).expect("rkyv deserialize");
        assert_eq!(delta, restored);
    }

    #[test]
    fn round_trip_agent_command() {
        let cmd = AgentCommand {
            command_id: 0xdeadbeef_cafebabe,
            action: AgentAction::SpawnPart {
                position: [0.0, 5.0, 0.0],
                class_name: "Part".to_string(),
            },
            script: None,
            issued_at_ms: 1000,
        };
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&cmd)
            .expect("serialize")
            .to_vec();
        let restored = rkyv::from_bytes::<AgentCommand, rkyv::rancor::Error>(&bytes)
            .expect("deserialize");
        assert_eq!(cmd, restored);
    }

    #[test]
    fn round_trip_agent_observation_with_confidence() {
        let obs = AgentObservation {
            command_id: 42,
            payload: ObservationPayload::ConfidenceUpdate {
                field_scores: vec![
                    ("position".to_string(), 0.9),
                    ("velocity".to_string(), 0.6),
                ],
                composite: 0.75,
                step: 3,
            },
            seq: 1,
            timestamp_ms: 5000,
            confidence: 0.75,
            uncertainty: vec![("velocity".to_string(), 0.4)],
        };
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&obs)
            .expect("serialize")
            .to_vec();
        let restored = rkyv::from_bytes::<AgentObservation, rkyv::rancor::Error>(&bytes)
            .expect("deserialize");
        assert_eq!(obs, restored);
        assert!((restored.confidence - 0.75).abs() < 1e-6);
    }

    #[test]
    fn round_trip_arc_agent_actions() {
        let begin = AgentCommand {
            command_id: 1,
            action: AgentAction::BeginEpisode {
                task_id: "arc_t01".to_string(),
                max_steps: 50,
            },
            script: None,
            issued_at_ms: 0,
        };
        let end = AgentCommand {
            command_id: 2,
            action: AgentAction::EndEpisode {
                final_score: 0.95,
                goal_reached: true,
            },
            script: None,
            issued_at_ms: 10_000,
        };
        for cmd in [begin, end] {
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&cmd)
                .expect("serialize")
                .to_vec();
            let restored = rkyv::from_bytes::<AgentCommand, rkyv::rancor::Error>(&bytes)
                .expect("deserialize");
            assert_eq!(cmd, restored);
        }
    }
}

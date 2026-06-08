//! # EustressStream Change Queue
//!
//! Replaces the former Apache Iggy integration with the embedded
//! `eustress-stream` crate. The public API is unchanged so all call sites
//! (engine, CLI, scenarios) continue to compile without modification.
//!
//! ## Design
//!
//! The hot path is **sub-microsecond**:
//!   1. ECS system calls `queue.send_delta(delta)` — serialises the delta and
//!      calls `Producer::send_bytes()` directly (no channel, no network hop).
//!   2. EustressStream dispatches synchronously to all in-process subscribers
//!      (TOML materializer, property panel, Explorer tree, …).
//!   3. No external server process is required; everything lives in the same
//!      address space as the Bevy app.
//!
//! ## Feature Gate
//! Compiled only when the `streaming` feature is enabled.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bytes::Bytes;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tracing::{info, warn};

use eustress_stream::{EustressStream, OwnedMessage, Producer, StreamConfig};

use crate::scene_delta::{
    AgentCommand, AgentObservation, DeltaKind, PartPayload, SceneDelta, TransformPayload,
    TOPIC_AGENT_COMMANDS, TOPIC_AGENT_OBSERVATIONS, TOPIC_SCENE_DELTAS,
};
use crate::classes::{BasePart, Instance};

// ─────────────────────────────────────────────────────────────────────────────
// ChangeQueueConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Connection and streaming configuration.
///
/// `url` and `stream_name` are retained for backward compatibility but are
/// ignored by EustressStream (no external server is required).
#[derive(Debug, Clone, Resource)]
pub struct ChangeQueueConfig {
    /// Legacy URL — retained for API compatibility, not used.
    pub url: String,
    /// Legacy stream name — retained for API compatibility, not used.
    pub stream_name: String,
    pub topic_scene_deltas: String,
    pub topic_agent_commands: String,
    pub topic_agent_observations: String,
    pub batch_size: usize,
    pub delta_linger_ms: u64,
    pub sim_linger_ms: u64,
    pub agent_poll_ms: u64,
    /// Ring-buffer capacity (number of messages retained in memory).
    pub channel_capacity: usize,
    /// When true, delta serialize errors are silently swallowed.
    pub drop_on_full: bool,
    pub scene_delta_partitions: u32,
    pub sim_result_partitions: u32,
}

impl Default for ChangeQueueConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            stream_name: "eustress".to_string(),
            topic_scene_deltas: TOPIC_SCENE_DELTAS.to_string(),
            topic_agent_commands: TOPIC_AGENT_COMMANDS.to_string(),
            topic_agent_observations: TOPIC_AGENT_OBSERVATIONS.to_string(),
            batch_size: 512,
            delta_linger_ms: 1,
            sim_linger_ms: 0,
            agent_poll_ms: 10,
            channel_capacity: 65_536,
            drop_on_full: true,
            scene_delta_partitions: 8,
            sim_result_partitions: 4,
        }
    }
}

impl ChangeQueueConfig {
    pub fn scene_partitions(&self) -> u64 {
        self.scene_delta_partitions.max(1) as u64
    }
    pub fn sim_partitions(&self) -> u32 {
        self.sim_result_partitions.max(1)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ChangeQueue
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy Resource — fire-and-forget sink for ECS mutation deltas.
///
/// ECS systems call `queue.send_delta(delta)` with <1 µs cost.
/// Dispatch is **synchronous and in-process** — no background thread, no TCP.
#[derive(Resource)]
pub struct ChangeQueue {
    /// Shared EustressStream — clone it to subscribe to any topic.
    pub stream: EustressStream,
    delta_producer: Producer,
    observation_producer: Producer,
    /// Receives decoded `AgentCommand`s from the `agent_commands` topic.
    pub command_rx: Arc<tokio::sync::Mutex<UnboundedReceiver<AgentCommand>>>,
    seq: Arc<AtomicU64>,
    session_start: Instant,
    drop_on_full: bool,
}

impl ChangeQueue {
    /// Send a scene delta. Non-blocking (<1 µs).
    pub fn send_delta(&self, delta: SceneDelta) {
        match delta.to_bytes() {
            Ok(b) => {
                self.delta_producer.send_bytes(Bytes::from(b));
            }
            Err(e) => {
                if !self.drop_on_full {
                    warn!("ChangeQueue: delta serialize failed: {e}");
                }
            }
        }
    }

    /// Send an agent observation to in-process subscribers.
    pub fn send_observation(&self, obs: AgentObservation) {
        match rkyv::to_bytes::<rkyv::rancor::Error>(&obs) {
            Ok(b) => {
                self.observation_producer.send_bytes(Bytes::from(b.to_vec()));
            }
            Err(e) => warn!("ChangeQueue: observation serialize failed: {e}"),
        }
    }

    /// Next monotonically increasing sequence number.
    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    /// Milliseconds elapsed since the session started.
    pub fn now_ms(&self) -> u64 {
        self.session_start.elapsed().as_millis() as u64
    }

    /// Wall-clock Unix timestamp in ms.
    pub fn unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// init_change_queue — bootstrap (no external server required)
// ─────────────────────────────────────────────────────────────────────────────

/// Initialise the EustressStream and return a ready `ChangeQueue`.
///
/// This is now infallible in practice (no network call), but returns `Result`
/// for API compatibility with existing call sites.
pub async fn init_change_queue(config: &ChangeQueueConfig) -> Result<ChangeQueue, String> {
    let ring_cap = config.channel_capacity.next_power_of_two();
    let stream = EustressStream::new(
        StreamConfig::default()
            .with_ring_capacity(ring_cap)
            .in_memory(),
    );

    let delta_producer       = stream.producer(TOPIC_SCENE_DELTAS);
    let observation_producer = stream.producer(TOPIC_AGENT_OBSERVATIONS);

    // Subscribe to incoming agent commands and forward to a tokio mpsc channel
    // so the existing `poll_agent_commands` Bevy system works unchanged.
    let (cmd_tx, cmd_rx) = unbounded_channel::<AgentCommand>();
    stream
        .subscribe_owned(TOPIC_AGENT_COMMANDS, move |msg: OwnedMessage| {
            match rkyv::from_bytes::<AgentCommand, rkyv::rancor::Error>(msg.data.as_ref()) {
                Ok(cmd) => {
                    let _ = cmd_tx.send(cmd);
                }
                Err(e) => warn!("EustressStream: command deserialize: {e}"),
            }
        })
        .map_err(|e| format!("subscribe agent_commands: {e}"))?;

    info!("EustressStream: streaming ready (in-process, no external server).");

    Ok(ChangeQueue {
        stream,
        delta_producer,
        observation_producer,
        command_rx: Arc::new(tokio::sync::Mutex::new(cmd_rx)),
        seq: Arc::new(AtomicU64::new(0)),
        session_start: Instant::now(),
        drop_on_full: config.drop_on_full,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// StreamingPlugin
// ─────────────────────────────────────────────────────────────────────────────

/// Bevy plugin: initialises EustressStream and registers ECS systems.
pub struct StreamingPlugin;

impl Plugin for StreamingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanelDirtyFlags>()
           .init_resource::<EvictedRecently>()
           .add_systems(Startup, setup_change_queue)
           // P2 two-tier: shift the eviction double-buffer once per frame, after
           // every Update reader (emit_scene_change_deltas) has had its pass.
           .add_systems(Last, rotate_evicted_recently)
           .add_systems(Update, (
               poll_agent_commands,
               // PERF (Vehicle Simulator, ~110K live, DEBUG): the five former
               // change-delta emitters were each `Changed<>`/`Added<>`-gated and
               // genuinely EMPTY at idle, but each still walked its full matched
               // table set every frame to run table-level change-tick checks. On a
               // Roblox import the archetype graph is heavily fragmented, so that
               // walk (×5 systems, ×5 schedule slots, un-inlined in debug) cost
               // ~50 ms/frame at idle. They are now one system that does a SINGLE
               // combined driver walk and early-returns when nothing changed.
               emit_scene_change_deltas,
               emit_transform_deltas,
           ));
    }
}

fn setup_change_queue(mut commands: Commands, config: Option<Res<ChangeQueueConfig>>) {
    let config = config.map(|c| c.clone()).unwrap_or_default();

    // Run init_change_queue on a background thread so we don't need an existing tokio
    // runtime on the main thread during Bevy startup.
    let result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio rt");
        rt.block_on(init_change_queue(&config))
    })
    .join()
    .unwrap_or_else(|_| Err("EustressStream init thread panicked".to_string()));

    match result {
        Ok(queue) => {
            info!("StreamingPlugin: ChangeQueue ready.");
            commands.insert_resource(queue);
        }
        Err(e) => {
            warn!("StreamingPlugin: stream init failed ({e}).");
        }
    }
}

fn poll_agent_commands(
    queue: Option<Res<ChangeQueue>>,
    mut commands: Commands,
) {
    let Some(queue) = queue else { return };
    let Ok(mut rx) = queue.command_rx.try_lock() else { return };
    while let Ok(cmd) = rx.try_recv() {
        commands.trigger(IncomingAgentCommand(cmd));
    }
}

/// Bevy observer event — fired once per incoming `AgentCommand`.
#[derive(Event)]
pub struct IncomingAgentCommand(pub AgentCommand);

// ─────────────────────────────────────────────────────────────────────────────
// Lifecycle delta emission
// ─────────────────────────────────────────────────────────────────────────────

/// Combined change filter for the idle short-circuit driver. A table is
/// visited ONCE and all of these tick-columns are checked in that single
/// pass, replacing the five separate full-table walks the former systems
/// each performed every frame. Removals are detected separately via
/// `RemovedComponents` (change-tick filters cannot see a despawn/remove).
type SceneChangeFilter = bevy::prelude::Or<(
    bevy::prelude::Changed<BasePart>,
    bevy::prelude::Changed<crate::attributes::Tags>,
    bevy::prelude::Changed<crate::attributes::Attributes>,
    bevy::prelude::Changed<bevy::prelude::Name>,
    bevy::prelude::Changed<bevy::prelude::ChildOf>,
    bevy::prelude::Added<Instance>,
)>;

/// Single merged change-delta emitter (replaces `emit_lifecycle_deltas`,
/// `emit_part_property_deltas`, `emit_tag_attr_dirty`, `emit_name_deltas`,
/// and `emit_parent_deltas`).
///
/// ## Idle fast-path
/// The first thing it does is a SINGLE combined driver scan
/// (`driver` + `removed.is_empty()`). At steady-state nothing mutates any of
/// BasePart/Tags/Attributes/Name/ChildOf/Instance (the Avian same-value
/// transform storm only trips `Changed<Transform>`, which `emit_transform_deltas`
/// owns), so the driver is empty and the system returns immediately — one
/// table-walk instead of five, and zero allocations.
///
/// ## Slow path (an edit/spawn/despawn happened this frame)
/// Falls through to the five per-type detail queries, emitting byte-identical
/// deltas and setting the exact same `PanelDirtyFlags` as the former systems.
#[allow(clippy::too_many_arguments)]
fn emit_scene_change_deltas(
    queue: Option<Res<ChangeQueue>>,
    // ── Idle driver: one combined walk over the union of changed tables. ──
    //
    // PERF (P2 two-tier — Vehicle Simulator, 120K residency-streamed COLD
    // parts): `Without<ColdStreamed>` excludes the streamed cold parts from
    // EVERY query here. Cold parts are never user-edited via the change
    // queue (the user can only edit a SELECTED part, and selection promotes
    // it by removing `ColdStreamed` — see `sync_selection_components`), and
    // the Explorer surfaces streamed parts from its own `instances` query +
    // the live-ECS tree rebuild, NOT from these deltas. So skipping them is
    // safe AND removes the ~36 ms/frame archetype-visit Bevy's change-tick
    // walk paid over all 120K, AND stops residency spawn/evict churn from
    // spamming Explorer rebuilds. A promoted (selected) part has had
    // `ColdStreamed` removed, so its edits emit deltas normally.
    driver: Query<(), (SceneChangeFilter, Without<crate::classes::ColdStreamed>)>,
    // ── Per-type detail queries (only iterated on the slow path). ──
    added_names: Query<Entity, (Added<bevy::prelude::Name>, Without<crate::classes::ColdStreamed>)>,
    added_instances: Query<(), (Added<Instance>, Without<crate::classes::ColdStreamed>)>,
    mut removed: RemovedComponents<bevy::prelude::Name>,
    // P2: a residency EVICTION despawns a streamed part, removing its `Name`
    // (so it shows up in `removed` above) — but an evict is NOT a real delete:
    // the part still lives in the Fjall DB and re-streams on demand, so it must
    // emit NO `PartRemoved` delta. An evict-despawn is indistinguishable from a
    // genuine delete-despawn by component-removal alone (both remove `Name`), so
    // `sys_residency_evict` records every entity it unloads in `EvictedRecently`
    // and we subtract that set from the Name-removal set below. This is robust
    // where the former `RemovedComponents<ColdStreamed>` correlation was not:
    //   (a) a genuine MCP/user delete of a still-COLD part is NOT in the
    //       eviction set, so its delete delta is emitted correctly; and
    //   (b) the double-buffered set spans exactly the 2-frame window over which
    //       a `Name`-removal is readable, so a promotion-then-delete cannot leak
    //       a stale suppression across frames.
    evicted: Res<EvictedRecently>,
    changed_parts: Query<(Entity, &BasePart), (bevy::prelude::Changed<BasePart>, Without<crate::classes::ColdStreamed>)>,
    changed_names: Query<(Entity, &bevy::prelude::Name), (bevy::prelude::Changed<bevy::prelude::Name>, Without<crate::classes::ColdStreamed>)>,
    changed_parents: Query<(Entity, &bevy::prelude::ChildOf), (bevy::prelude::Changed<bevy::prelude::ChildOf>, Without<crate::classes::ColdStreamed>)>,
    changed_tags: Query<(), (bevy::prelude::Changed<crate::attributes::Tags>, Without<crate::classes::ColdStreamed>)>,
    changed_attrs: Query<(), (bevy::prelude::Changed<crate::attributes::Attributes>, Without<crate::classes::ColdStreamed>)>,
    mut dirty: ResMut<PanelDirtyFlags>,
) {
    // ── Idle short-circuit. `removed.is_empty()` peeks the unread-events
    //    cursor without consuming, so skipping it here cannot leak events:
    //    if it is empty there is nothing to drain. The driver walks the
    //    matched tables ONCE; on a settled scene it is empty and we are done.
    //    `evicted` is a plain `Res` (no per-reader cursor to leak), so it
    //    plays no part in this gate — it is only consulted on the slow path
    //    below, which runs whenever `removed` is non-empty. ──
    if driver.is_empty() && removed.is_empty() {
        return;
    }

    // ── Slow path: at least one of the watched components changed, an
    //    Instance was added, or a Name was removed this frame. ──

    // (1) emit_lifecycle_deltas: Instance spawn flags Explorer rebuild even
    //     when no Name was added/removed (bulk in-memory imports insert
    //     `Instance` without tripping Changed<Name>/Changed<ChildOf>).
    if !added_instances.is_empty() {
        dirty.explorer = true;
    }

    let added_list: Vec<Entity> = added_names.iter().collect();
    // Real Name-removals MINUS residency evictions: an evict despawn removed
    // `Name` (so the entity is in `removed`) but is not a real delete, so drop
    // it here — streaming churn emits no lifecycle delta and no Explorer
    // rebuild. `EvictedRecently::contains` covers entities residency unloaded
    // THIS frame or LAST frame (double-buffered to bridge the despawn-flush
    // latency: residency records the entity the frame it despawns, but the
    // `Name`-removal only becomes readable here after the end-of-`Update`
    // flush, i.e. the next frame). A genuine delete (MCP/user) is never in this
    // set, so its `PartRemoved` delta is emitted normally.
    let removed_list: Vec<Entity> = removed
        .read()
        .filter(|e| !evicted.contains(*e))
        .collect();
    if !added_list.is_empty() || !removed_list.is_empty() {
        dirty.explorer = true;
        if let Some(ref queue) = queue {
            for entity in added_list {
                let seq = queue.next_seq();
                let ts  = ChangeQueue::unix_ms();
                queue.send_delta(SceneDelta::lifecycle(entity.to_bits(), DeltaKind::PartAdded, seq, ts));
            }
            for entity in removed_list {
                let seq = queue.next_seq();
                let ts  = ChangeQueue::unix_ms();
                queue.send_delta(SceneDelta::lifecycle(entity.to_bits(), DeltaKind::PartRemoved, seq, ts));
            }
        }
    }

    // (2) emit_tag_attr_dirty: any Tags/Attributes change refreshes the
    //     Properties panel (single selected entity rebuild — cheap).
    if !changed_tags.is_empty() || !changed_attrs.is_empty() {
        dirty.properties = true;
    }

    // (3) emit_part_property_deltas.
    if !changed_parts.is_empty() {
        dirty.properties = true;
        if let Some(ref queue) = queue {
            for (entity, bp) in changed_parts.iter() {
                let seq = queue.next_seq();
                let ts  = ChangeQueue::unix_ms();
                queue.send_delta(SceneDelta::part_props(
                    entity.to_bits(), seq, ts,
                    PartPayload {
                        color:        Some(bp.color.to_linear().to_f32_array()),
                        material:     Some(bp.material as u16),
                        size:         Some(bp.size.to_array()),
                        name:         None,
                        anchored:     Some(bp.anchored),
                        can_collide:  Some(bp.can_collide),
                        transparency: Some(bp.transparency),
                        reflectance:  Some(bp.reflectance),
                    },
                ));
            }
        }
    }

    // (4) emit_name_deltas.
    if !changed_names.is_empty() {
        dirty.explorer = true;
        if let Some(ref queue) = queue {
            for (entity, name) in changed_names.iter() {
                let seq = queue.next_seq();
                let ts  = ChangeQueue::unix_ms();
                queue.send_delta(SceneDelta::rename(entity.to_bits(), seq, ts, name.to_string()));
            }
        }
    }

    // (5) emit_parent_deltas.
    if !changed_parents.is_empty() {
        dirty.explorer = true;
        if let Some(ref queue) = queue {
            for (entity, child_of) in changed_parents.iter() {
                let seq = queue.next_seq();
                let ts  = ChangeQueue::unix_ms();
                queue.send_delta(SceneDelta {
                    entity:       entity.to_bits(),
                    kind:         DeltaKind::Reparented,
                    seq,
                    timestamp_ms: ts,
                    transform:    None,
                    part:         None,
                    name:         None,
                    new_parent:   Some(child_of.0.to_bits()),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// EvictedRecently — distinguishes a residency *evict* from a genuine *delete*
// ─────────────────────────────────────────────────────────────────────────────

/// Entities that `sys_residency_evict` unloaded recently (this frame + last
/// frame). A residency eviction despawns a streamed part — firing
/// `RemovedComponents<Name>` exactly like a real delete — but it is NOT a
/// delete: the part still lives in the Fjall DB and re-streams on demand, so it
/// must emit no `PartRemoved` delta. The two are indistinguishable by
/// component-removal alone, so residency records each evicted entity here and
/// `emit_scene_change_deltas` subtracts this set from the `Name`-removal set.
///
/// Double-buffered to match Bevy's own 2-frame `RemovedComponents` visibility
/// window. Residency adds to `current` the frame it despawns, but that despawn's
/// `Name`-removal only becomes readable to `emit_*` after the end-of-`Update`
/// command flush (i.e. the NEXT frame). `rotate_evicted_recently` (in `Last`)
/// shifts `current → previous` once per frame, so an entity evicted in frame N
/// stays in `current ∪ previous` across frames N and N+1 — exactly the span over
/// which its `Name`-removal is readable. A genuine delete (MCP/user) is never
/// recorded here, so its `PartRemoved` delta is always emitted.
#[derive(Resource, Default)]
pub struct EvictedRecently {
    current: std::collections::HashSet<bevy::prelude::Entity>,
    previous: std::collections::HashSet<bevy::prelude::Entity>,
}

impl EvictedRecently {
    /// Record an entity that residency unloaded this frame.
    #[inline]
    pub fn record(&mut self, entity: bevy::prelude::Entity) {
        self.current.insert(entity);
    }

    /// True if `entity` was evicted this frame or last frame.
    #[inline]
    pub fn contains(&self, entity: bevy::prelude::Entity) -> bool {
        self.current.contains(&entity) || self.previous.contains(&entity)
    }
}

/// Shift the eviction double-buffer once per frame (`Last`, after every
/// `Update` reader has had this frame's pass). Cheap no-op when idle.
fn rotate_evicted_recently(mut evicted: ResMut<EvictedRecently>) {
    if evicted.current.is_empty() && evicted.previous.is_empty() {
        return;
    }
    let current = std::mem::take(&mut evicted.current);
    evicted.previous = current;
}

// ─────────────────────────────────────────────────────────────────────────────
// PanelDirtyFlags — bridges change detection → UI sync systems
// ─────────────────────────────────────────────────────────────────────────────

/// Set by ECS change-detection systems; cleared by UI sync systems.
///
/// - `explorer`: tree needs immediate rebuild (entity added/removed/reparented/renamed)
/// - `properties`: panel needs immediate refresh (selected entity's Transform or BasePart changed)
///
/// The engine systems `sync_unified_explorer_to_slint` and `sync_properties_to_slint`
/// read these flags to bypass their normal throttling and fire immediately.
#[derive(Resource, Default)]
pub struct PanelDirtyFlags {
    pub explorer: bool,
    pub properties: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Transform / BasePart / Name / Hierarchy change-detection producers
// ─────────────────────────────────────────────────────────────────────────────

fn emit_transform_deltas(
    queue: Option<Res<ChangeQueue>>,
    // PERF (P2 two-tier — Vehicle Simulator, 120K residency-streamed COLD
    // parts): `Without<ColdStreamed>` excludes the streamed cold parts from
    // this `Changed<Transform>` driver. Bevy O(N)-visits every matching
    // archetype each frame to read change-ticks, and the Avian same-value
    // transform sync trips `Changed<Transform>` on every anchored body, so
    // over 120K cold parts this was a per-frame O(N) visit (+ a delta-emit
    // storm). A cold streamed part is static scenery that is NEVER user-moved
    // (editing requires SELECTION, which promotes the part by removing
    // `ColdStreamed` — see `selection_sync::sync_selection_components`), so it
    // can never have a `Changed<Transform>` that matters for the delta stream.
    // A promoted (selected) part has had `ColdStreamed` removed, so its moves
    // still emit deltas normally. Mirrors the `Without<ColdStreamed>` filter
    // already on `emit_scene_change_deltas`' queries above.
    changed: Query<(Entity, &bevy::prelude::Transform), (
        bevy::prelude::Changed<bevy::prelude::Transform>,
        bevy::prelude::With<BasePart>,
        bevy::prelude::Without<crate::classes::ColdStreamed>,
    )>,
    mut dirty: ResMut<PanelDirtyFlags>,
) {
    if changed.is_empty() { return; }
    dirty.properties = true;
    let Some(queue) = queue else { return };
    for (entity, t) in changed.iter() {
        let seq = queue.next_seq();
        let ts  = ChangeQueue::unix_ms();
        queue.send_delta(SceneDelta::transform(
            entity.to_bits(), seq, ts,
            TransformPayload {
                position: t.translation.to_array(),
                rotation: t.rotation.to_array(),
                scale:    t.scale.to_array(),
            },
        ));
    }
}

// NOTE: `emit_lifecycle_deltas`, `emit_tag_attr_dirty`,
// `emit_part_property_deltas`, `emit_name_deltas`, and `emit_parent_deltas`
// were merged into the single `emit_scene_change_deltas` system above (one
// combined Or<> driver walk + an idle short-circuit). `emit_transform_deltas`
// is kept separate because its Avian same-value transform value-gate is
// load-bearing.

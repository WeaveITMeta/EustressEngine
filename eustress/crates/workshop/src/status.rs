//! # status
//!
//! Live IoT telemetry and kinetic GPS state machine for workshop tools.
//!
//! ## Kinetic GPS — No Batteries Required
//!
//! Kinetic (energy-harvesting) GPS chips only power on and transmit when the
//! tool is physically moved. They harvest energy from the movement itself.
//! This means the engine NEVER gets a continuous stream — it only receives
//! a burst when the tool is picked up and when it is set down.
//!
//! The state machine handles this correctly:
//!
//! ```text
//!  [AtRest — known container]
//!         │
//!         │  chip fires: tool picked up, position changes
//!         ▼
//!  [InTransit — last-known position held, state = InUse]
//!    ← tool file moved to active-use/ folder
//!         │
//!         │  chip fires again: tool set down, new position received
//!         ▼
//!  [AtRest — new container resolved from position]
//!    ← tool file moved to new container folder
//! ```
//!
//! Between the two events the engine knows:
//! - The tool IS in use (someone picked it up)
//! - The last position it was seen at (before pickup)
//! - How long it has been in transit
//!
//! It does NOT know exactly where the tool is mid-transit — and that is fine.
//! The state machine does not pretend otherwise.
//!
//! ## Table of Contents
//!
//! | Section              | Purpose                                                       |
//! |----------------------|---------------------------------------------------------------|
//! | `ToolLocation`       | 3D position (zone label + optional GPS coordinates)           |
//! | `OperationalState`   | Available / InTransit / AtRest / CheckedOut / Missing         |
//! | `KineticChipState`   | Per-chip state machine tracking movement event pairs          |
//! | `IoTTelemetry`       | Full telemetry payload broadcast by a GPS chip on movement    |
//! | `LiveStatusStore`    | In-memory store with last-known position and state inference  |

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// 1. Location
// ============================================================================

/// The physical location of a tool at a point in time.
/// Supports both human-readable workshop zones and raw GPS coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolLocation {
    /// Human-readable zone label (e.g. "Bench 3, right shelf", "CNC bay", "Storage room B")
    pub zone_label: String,
    /// Raw GPS latitude (degrees), present when the chip has GPS capability
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    /// Raw GPS longitude (degrees)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
    /// Altitude in metres above floor level (useful for multi-storey workshops)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_m: Option<f32>,
    /// Position in the digital twin Space's local coordinate system [x, y, z] in metres
    /// Derived from GPS or manually set for non-GPS chips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_position: Option<[f32; 3]>,
    /// Accuracy radius in metres (GPS dilution of precision)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accuracy_m: Option<f32>,
}

impl Default for ToolLocation {
    fn default() -> Self {
        Self {
            zone_label: "Unknown".into(),
            latitude: None,
            longitude: None,
            altitude_m: None,
            space_position: None,
            accuracy_m: None,
        }
    }
}

// ============================================================================
// 2. Operational State
// ============================================================================

/// The current operational state of a workshop tool.
///
/// For kinetic GPS chips (no battery), states are inferred from the movement
/// event pair — not from a continuous stream. See `KineticChipState` for how
/// the state machine transitions between these values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationalState {
    /// Tool is sitting in a known container, ready to be used.
    /// Kinetic chip: last event was a set-down at a resolved container.
    Available,
    /// Tool has been picked up and is currently being moved or used.
    /// Kinetic chip: chip fired because it was lifted — second event not yet received.
    /// The tool's `.tool.toml` is in the `active-use/` folder during this state.
    InTransit,
    /// Tool has been set down in a known container after being in transit.
    /// Kinetic chip: second event received, new container resolved from position.
    /// This is a transient state — transitions to `Available` immediately after
    /// `StorageManager` moves the file to the new container folder.
    AtRest { container_id: uuid::Uuid },
    /// Tool has been explicitly checked out to a build guide step.
    /// Set by the build guide system when a step is started, not by GPS.
    CheckedOut { step_id: String },
    /// Tool has not sent a kinetic event for longer than `stale_threshold_hours`.
    /// This is NORMAL for kinetic chips — it means the tool has not moved.
    /// Not an error condition. Use `KineticChipState::hours_since_last_event()` to
    /// distinguish between "not moved recently" and "genuinely missing".
    Stationary,
    /// Tool is confirmed missing — not seen for `missing_threshold_hours` AND
    /// its last-known position does not match any registered container.
    Missing,
    /// Tool's IoT chip has not responded to any gateway — network or hardware failure.
    /// Different from Stationary (which means "not moved") — Unreachable means
    /// the chip should be reporting but is not.
    Unreachable,
}

impl OperationalState {
    /// Returns the display label used in the Properties Panel and build guides
    pub fn display_label(&self) -> &str {
        match self {
            OperationalState::Available => "Available",
            OperationalState::InTransit => "In Transit",
            OperationalState::AtRest { .. } => "At Rest",
            OperationalState::CheckedOut { .. } => "Checked Out",
            OperationalState::Stationary => "Stationary",
            OperationalState::Missing => "Missing",
            OperationalState::Unreachable => "Unreachable",
        }
    }

    /// Returns true if this tool can be assigned to a new build step right now
    pub fn is_assignable(&self) -> bool {
        matches!(self, OperationalState::Available | OperationalState::Stationary)
    }

    /// Returns true if this tool is currently in motion (kinetic chip is active)
    pub fn is_in_motion(&self) -> bool {
        matches!(self, OperationalState::InTransit)
    }
}

// ============================================================================
// 3. KineticChipState — per-chip state machine
// ============================================================================

/// The phase of the kinetic chip's two-event movement cycle.
/// A kinetic chip fires twice per movement:
/// 1. **Departure** — tool is picked up, chip harvests energy from the lift
/// 2. **Arrival** — tool is set down, chip harvests energy from the impact
///
/// Between these two events the engine infers the tool is in active use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KineticPhase {
    /// No movement recorded yet — initial state after registration
    Uninitialized,
    /// Departure event received — tool has been picked up.
    /// Waiting for the arrival event to resolve the new container.
    Departed {
        /// Position recorded at departure (last-known resting position)
        departed_from: [f32; 3],
        /// When the departure event was received
        departed_at: DateTime<Utc>,
    },
    /// Arrival event received — tool has been set down at a new position.
    /// `StorageManager` resolves this position to a container and moves the file.
    Arrived {
        /// Position recorded at arrival (new resting position)
        arrived_at_pos: [f32; 3],
        /// When the arrival event was received
        arrived_at: DateTime<Utc>,
    },
}

/// Per-chip state machine for a kinetic (energy-harvesting) GPS chip.
/// Tracks the two-event movement cycle and maintains last-known position
/// so the engine always has a confident answer to "where was this tool last?"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KineticChipState {
    /// The chip's hardware identifier
    pub chip_id: String,
    /// The tool this chip is attached to
    pub tool_id: Uuid,
    /// Current phase of the two-event movement cycle
    pub phase: KineticPhase,
    /// The last confirmed resting position (updated on each Arrival event)
    pub last_known_position: Option<[f32; 3]>,
    /// Human-readable label of the last confirmed container
    pub last_known_container: Option<String>,
    /// UUID of the last confirmed container
    pub last_known_container_id: Option<Uuid>,
    /// UTC timestamp of the most recent event (departure or arrival)
    pub last_event_at: Option<DateTime<Utc>>,
    /// Total number of movement events received from this chip
    pub total_events: u64,
}

impl KineticChipState {
    /// Create a new chip state in the uninitialized phase
    pub fn new(chip_id: impl Into<String>, tool_id: Uuid) -> Self {
        Self {
            chip_id: chip_id.into(),
            tool_id,
            phase: KineticPhase::Uninitialized,
            last_known_position: None,
            last_known_container: None,
            last_known_container_id: None,
            last_event_at: None,
            total_events: 0,
        }
    }

    /// Process an incoming telemetry event from this chip.
    /// Returns the new `OperationalState` inferred from the phase transition.
    ///
    /// # Kinetic two-event logic
    /// - If currently `Arrived` or `Uninitialized` → this is a **Departure** event.
    ///   The tool has been picked up. Transition to `InTransit`.
    /// - If currently `Departed` → this is an **Arrival** event.
    ///   The tool has been set down. Transition to `AtRest` with the new position.
    pub fn process_event(&mut self, position: [f32; 3], received_at: DateTime<Utc>) -> OperationalState {
        self.total_events += 1;
        self.last_event_at = Some(received_at);

        match &self.phase {
            // Uninitialized or just arrived → this event is a departure (tool picked up)
            KineticPhase::Uninitialized | KineticPhase::Arrived { .. } => {
                // Record the current resting position before departure
                if let Some(pos) = self.last_known_position {
                    let _ = pos; // retained — last_known_position stays as "where it was"
                }
                self.phase = KineticPhase::Departed {
                    departed_from: position,
                    departed_at: received_at,
                };
                OperationalState::InTransit
            }
            // Already departed → this event is an arrival (tool set down)
            KineticPhase::Departed { .. } => {
                self.last_known_position = Some(position);
                self.phase = KineticPhase::Arrived {
                    arrived_at_pos: position,
                    arrived_at: received_at,
                };
                // Container ID will be resolved by StorageManager after this returns
                OperationalState::AtRest { container_id: Uuid::nil() }
            }
        }
    }

    /// Update the last-known container after `StorageManager` resolves the arrival position
    pub fn confirm_container(&mut self, container_id: Uuid, container_label: impl Into<String>) {
        self.last_known_container_id = Some(container_id);
        self.last_known_container = Some(container_label.into());
        // Transition AtRest → Available now that the container is confirmed
    }

    /// Returns how many hours have elapsed since the last kinetic event.
    /// For kinetic chips, long silence means the tool has not moved — not that it is lost.
    pub fn hours_since_last_event(&self) -> Option<f64> {
        self.last_event_at.map(|t| {
            Utc::now()
                .signed_duration_since(t)
                .num_minutes() as f64
                / 60.0
        })
    }

    /// Returns the inferred `OperationalState` based on current phase and elapsed time.
    /// Called each frame by the Bevy sync system to keep entity state current.
    ///
    /// # Thresholds
    /// - `stale_hours`: hours of silence before transitioning from Available → Stationary
    ///   (default 8h — a full work shift without picking up the tool)
    /// - `missing_hours`: hours of silence before flagging as potentially Missing
    ///   (default 72h — 3 days without any movement)
    pub fn infer_state(&self, stale_hours: f64, missing_hours: f64) -> OperationalState {
        match &self.phase {
            KineticPhase::Uninitialized => OperationalState::Stationary,
            KineticPhase::Departed { .. } => OperationalState::InTransit,
            KineticPhase::Arrived { .. } => {
                match self.hours_since_last_event() {
                    None => OperationalState::Available,
                    Some(h) if h < stale_hours => OperationalState::Available,
                    Some(h) if h < missing_hours => OperationalState::Stationary,
                    _ => OperationalState::Missing,
                }
            }
        }
    }
}

// ============================================================================
// 3. CubePacket — merged dual-mode BLE wire format
// ============================================================================

/// Compressed binary packet broadcast by The Cube (merged V1/V2) over BLE advertisement.
///
/// ## Dual-mode design
///
/// The Cube uses a **Nordic nRF9161 SiP** — one chip with BLE 5.3, LTE-M, and GNSS.
/// A **10 mF ceramic supercapacitor bank** (10 × 1 mF in parallel) accumulates
/// charge across motion events.
///
/// Every event: BLE advertisement fires (~65 μJ — always within single-event harvest budget).
/// Every ~130 events: when bank reaches 35 mJ threshold, GPS + LTE-M fires for precise fix.
///
/// The `gps_fired` flag in this packet tells the gateway whether this event
/// also carries a GPS fix (transmitted separately over LTE-M at the same wake window).
/// The gateway correlates the BLE packet with the LTE-M GPS payload by `chip_id_short`
/// and `event_seq` within a 5-second correlation window.
///
/// ## Wire layout (big-endian, 13 bytes)
///
/// ```text
///  Byte  Width  Field           Encoding
///  ────  ─────  ──────────────  ──────────────────────────────────────────
///   0      1    version         0x02 (merged V1+V2 format)
///   1      1    flags           bit0 = event (0=depart, 1=arrive)
///                               bit1 = has_rssi
///                               bit2 = has_temp
///                               bit3 = gps_fired (GPS+LTE-M also firing this wake)
///                               bit4–7 = reserved (0)
///   2–4    3    chip_id_short   Lower 24 bits of chip UUID
///   5–6    2    bank_mv         10 mF bank voltage in mV, u16 big-endian (0–3300)
///   7–8    2    event_seq       u16 big-endian event counter — wraps at 65535
///                               Used to correlate BLE packet with LTE-M GPS payload
///   9      1    harvested_uj    μJ / 4 as u8 (0–1020 μJ range; ×4 on decode)
///  10      1    rssi_encoded    (rssi_dbm + 200) as u8; only if flags bit1 set
///  11      1    temp_encoded    (temp_c + 40) as u8; only if flags bit2 set
///  12      1    reserved        0x00
///
///  Total: 13 bytes
/// ```
///
/// ## GPS correlation (when gps_fired = true)
///
/// The nRF9161 transmits GPS coordinates via LTE-M as a separate 20-byte binary
/// payload to `workshop/tools/{chip_id}/gps`. The gateway correlates it with
/// the BLE packet via `chip_id_short` + `event_seq` within a 10-second window.
/// If no LTE-M payload arrives (no cellular coverage), the BLE position estimate
/// from gateway RSSI is used as a fallback and `gps_fix_pending` is set in the
/// stored `IoTTelemetry`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CubePacket {
    /// Packet format version — 0x02 for merged dual-mode design
    pub version: u8,
    /// True = arrival event (tool set down); False = departure (tool picked up)
    pub is_arrival: bool,
    /// Whether `rssi_dbm` field is valid
    pub has_rssi: bool,
    /// Whether `temp_c` field is valid
    pub has_temp: bool,
    /// True = GPS + LTE-M outdoor fallback is firing this wake window.
    /// **Only set on arrival events (tool set down and at rest).**
    /// GPS fires only when the tool has left the workshop — no BLE gateways present.
    /// Indoor positioning is handled by BLE gateway RSSI/AoA triangulation, not GPS.
    /// GPS requires open sky and is useless indoors.
    /// The firmware waits `GPS_WAIT_FOR_REST_MS` (500ms) of stillness before firing.
    /// When set, the gateway will expect a correlated LTE-M GPS payload within 10 seconds.
    pub gps_fired: bool,
    /// Lower 24 bits of the chip UUID — gateway resolves to full UUID
    pub chip_id_short: u32,
    /// 10 mF supercap bank voltage in millivolts
    pub bank_mv: u16,
    /// Monotonically increasing event counter — wraps at 65535
    /// Used to correlate this BLE packet with the LTE-M GPS payload
    pub event_seq: u16,
    /// Harvested energy this event in μJ
    pub harvested_uj: u16,
    /// RSSI in dBm; only valid if `has_rssi`
    pub rssi_dbm: i16,
    /// Module temperature in °C; only valid if `has_temp`
    pub temp_c: i16,
}

impl CubePacket {
    /// Packet size in bytes — fixed at 13 for merged dual-mode format
    pub const SIZE: usize = 13;
    /// Version byte for the merged dual-mode format
    pub const VERSION: u8 = 0x02;
    /// 10 mF bank voltage threshold for GPS + LTE-M fire (mV)
    /// Corresponds to stored energy of ~35 mJ: V = √(2 × 35e-3 / 10e-3) = 2650 mV
    pub const GPS_FIRE_THRESHOLD_MV: u16 = 2650;
    /// Milliseconds of stillness after motion stops before the firmware
    /// confirms the tool is at rest and fires the GPS fix.
    /// Prevents spurious GPS events from brief set-downs during active use.
    pub const GPS_WAIT_FOR_REST_MS: u32 = 500;

    /// Decode a 13-byte BLE manufacturer-specific data field.
    /// Returns `None` if too short or version byte is unrecognised.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        if bytes[0] != Self::VERSION {
            return None;
        }
        let flags      = bytes[1];
        let is_arrival = (flags & 0b0000_0001) != 0;
        let has_rssi   = (flags & 0b0000_0010) != 0;
        let has_temp   = (flags & 0b0000_0100) != 0;
        let gps_fired  = (flags & 0b0000_1000) != 0;

        let chip_id_short = (bytes[2] as u32) << 16
            | (bytes[3] as u32) << 8
            | (bytes[4] as u32);

        let bank_mv    = u16::from_be_bytes([bytes[5], bytes[6]]);
        let event_seq  = u16::from_be_bytes([bytes[7], bytes[8]]);
        let harvested_uj = (bytes[9] as u16) * 4;
        let rssi_dbm   = if has_rssi { bytes[10] as i16 - 200 } else { -128 };
        let temp_c     = if has_temp { bytes[11] as i16 - 40  } else { 0 };

        Some(Self {
            version: bytes[0],
            is_arrival,
            has_rssi,
            has_temp,
            gps_fired,
            chip_id_short,
            bank_mv,
            event_seq,
            harvested_uj,
            rssi_dbm,
            temp_c,
        })
    }

    /// Encode this packet into a 13-byte array.
    /// Used by firmware simulation and test harnesses.
    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut flags: u8 = 0;
        if self.is_arrival  { flags |= 0b0000_0001; }
        if self.has_rssi    { flags |= 0b0000_0010; }
        if self.has_temp    { flags |= 0b0000_0100; }
        if self.gps_fired   { flags |= 0b0000_1000; }

        let bank_bytes     = self.bank_mv.to_be_bytes();
        let seq_bytes      = self.event_seq.to_be_bytes();
        let harvested_raw  = (self.harvested_uj / 4).min(255) as u8;
        let rssi_raw       = if self.has_rssi { (self.rssi_dbm + 200).clamp(0, 255) as u8 } else { 0 };
        let temp_raw       = if self.has_temp { (self.temp_c   +  40).clamp(0, 255) as u8 } else { 0 };
        let id_b0 = ((self.chip_id_short >> 16) & 0xFF) as u8;
        let id_b1 = ((self.chip_id_short >>  8) & 0xFF) as u8;
        let id_b2 = ( self.chip_id_short        & 0xFF) as u8;

        [
            self.version,
            flags,
            id_b0, id_b1, id_b2,
            bank_bytes[0], bank_bytes[1],
            seq_bytes[0],  seq_bytes[1],
            harvested_raw,
            rssi_raw,
            temp_raw,
            0x00, // reserved
        ]
    }

    /// Returns the event type as a human-readable string
    pub fn event_label(&self) -> &'static str {
        if self.is_arrival { "arrival" } else { "departure" }
    }

    /// Returns true if this packet carries a GPS fix.
    /// GPS is **only valid on arrival events** — the tool must be at rest.
    /// A departure packet with `gps_fired = true` is malformed and should be rejected.
    pub fn is_gps_event(&self) -> bool {
        self.is_arrival && (self.gps_fired || self.bank_mv >= Self::GPS_FIRE_THRESHOLD_MV)
    }

    /// Returns true if this packet should be rejected as malformed.
    /// The only currently defined malformed state: GPS fired on a departure event.
    pub fn is_malformed(&self) -> bool {
        self.gps_fired && !self.is_arrival
    }

    /// Estimated stored energy in the 10 mF bank in millijoules
    /// E = ½ × C × V²
    pub fn bank_energy_mj(&self) -> f32 {
        let v = self.bank_mv as f32 / 1000.0;
        0.5 * 10e-3 * v * v * 1000.0  // result in mJ
    }
}

// ============================================================================
// 4. IoT Telemetry Payload
// ============================================================================

/// A single telemetry message broadcast by a GPS-chipped tool.
/// For Cube V1 devices, this is produced by the workshop gateway after
/// decoding a `CubePacket` and enriching it with RSSI-based position.
/// Stored in `LiveStatusStore` keyed by tool UUID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoTTelemetry {
    /// The tool this telemetry belongs to
    pub tool_id: Uuid,
    /// Hardware chip identifier — used to map incoming MQTT messages to a tool
    pub chip_id: String,
    /// Current physical location
    pub location: ToolLocation,
    /// Current operational state as reported by the chip's sensors
    pub state: OperationalState,
    /// Battery level as a percentage (0–100), if the chip reports it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_pct: Option<u8>,
    /// Internal chip temperature in Celsius (for diagnostics)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chip_temp_celsius: Option<f32>,
    /// Signal strength in dBm (RSSI) — lower magnitude = better signal
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rssi_dbm: Option<i16>,
    /// Raw sensor readings — arbitrary key-value pairs from the chip firmware
    /// Rendered as read-only rows in the Properties Panel under "Live Telemetry"
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub sensor_readings: HashMap<String, f32>,
    /// UTC timestamp when this telemetry was captured by the chip
    pub captured_at: DateTime<Utc>,
    /// UTC timestamp when this telemetry was received by the engine
    pub received_at: DateTime<Utc>,
}

impl IoTTelemetry {
    /// Build a minimal offline/unreachable telemetry record for a chip that has stopped reporting
    pub fn unreachable(tool_id: Uuid, chip_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            tool_id,
            chip_id: chip_id.into(),
            location: ToolLocation::default(),
            state: OperationalState::Unreachable,
            battery_pct: None,
            chip_temp_celsius: None,
            rssi_dbm: None,
            sensor_readings: HashMap::new(),
            captured_at: now,
            received_at: now,
        }
    }

    /// Returns a short human-readable status line for the build guide step display
    pub fn status_line(&self) -> String {
        format!(
            "{} — {} (last seen {})",
            self.state.display_label(),
            self.location.zone_label,
            self.captured_at.format("%H:%M:%S"),
        )
    }
}

// ============================================================================
// 5. Live Status Store
// ============================================================================

/// Configuration thresholds for kinetic chip state inference.
/// Loaded from `.workshop/workshop.toml` or uses defaults.
#[derive(Debug, Clone)]
pub struct KineticThresholds {
    /// Hours of no movement before `Available` → `Stationary` (default: 8h = one work shift)
    pub stale_hours: f64,
    /// Hours of no movement before `Stationary` → `Missing` (default: 72h = 3 days)
    pub missing_hours: f64,
}

impl Default for KineticThresholds {
    fn default() -> Self {
        Self {
            stale_hours: 8.0,
            missing_hours: 72.0,
        }
    }
}

/// In-memory store of tool telemetry and kinetic chip state machines.
///
/// # Kinetic GPS design
/// Each GPS-chipped tool has a `KineticChipState` entry in `chip_states`.
/// The store processes incoming telemetry events through the state machine and
/// infers the tool's `OperationalState` without requiring a continuous stream.
///
/// The store answers three questions correctly at all times:
/// 1. **Is this tool in motion right now?** — `state_of()` returns `InTransit`
/// 2. **Where was this tool last seen?** — `last_known_location_of()`
/// 3. **When was it last heard from?** — `hours_since_last_event()`
#[derive(Debug, Default)]
pub struct LiveStatusStore {
    /// Latest raw telemetry payload per tool UUID (most recent event only)
    latest: HashMap<Uuid, IoTTelemetry>,
    /// Kinetic chip state machines — one per chip_id
    chip_states: HashMap<String, KineticChipState>,
    /// Secondary index: chip_id → tool UUID (for routing incoming MQTT events)
    chip_to_tool: HashMap<String, Uuid>,
    /// Thresholds for inferring Stationary / Missing from silence duration
    pub thresholds: KineticThresholds,
}

impl LiveStatusStore {
    /// Create a store with custom thresholds
    pub fn with_thresholds(thresholds: KineticThresholds) -> Self {
        Self {
            thresholds,
            ..Self::default()
        }
    }

    /// Register a chip with the store so incoming events can be routed to a tool.
    /// Must be called for each `ToolIotConfig` at startup before any events arrive.
    pub fn register_chip(&mut self, chip_id: impl Into<String>, tool_id: Uuid) {
        let chip_id = chip_id.into();
        self.chip_to_tool.insert(chip_id.clone(), tool_id);
        self.chip_states
            .entry(chip_id.clone())
            .or_insert_with(|| KineticChipState::new(chip_id, tool_id));
    }

    /// Process an incoming kinetic telemetry event.
    /// Routes through the `KineticChipState` state machine and stores the raw payload.
    /// Returns the new `OperationalState` so the caller can trigger file moves.
    pub fn process_kinetic_event(&mut self, telemetry: IoTTelemetry) -> OperationalState {
        let tool_id = telemetry.tool_id;
        let chip_id = telemetry.chip_id.clone();
        let received_at = telemetry.received_at;

        // Ensure chip is registered
        self.chip_to_tool.insert(chip_id.clone(), tool_id);

        // Extract position from telemetry
        let position = telemetry
            .location
            .space_position
            .unwrap_or([0.0, 0.0, 0.0]);

        // Advance the kinetic state machine
        let new_state = self
            .chip_states
            .entry(chip_id.clone())
            .or_insert_with(|| KineticChipState::new(chip_id, tool_id))
            .process_event(position, received_at);

        // Store the raw telemetry with the inferred state
        let mut enriched = telemetry;
        enriched.state = new_state.clone();
        self.latest.insert(tool_id, enriched);

        new_state
    }

    /// Legacy update path for non-kinetic chips (battery-powered with continuous stream).
    /// Simply stores the telemetry without running through the kinetic state machine.
    pub fn update(&mut self, telemetry: IoTTelemetry) {
        let tool_id = telemetry.tool_id;
        self.chip_to_tool
            .insert(telemetry.chip_id.clone(), tool_id);
        self.latest.insert(tool_id, telemetry);
    }

    /// Confirm the container a tool arrived in after an Arrival event.
    /// Called by `StorageManager` after it resolves the GPS position to a container.
    pub fn confirm_arrival_container(
        &mut self,
        chip_id: &str,
        container_id: Uuid,
        container_label: impl Into<String>,
    ) {
        if let Some(chip_state) = self.chip_states.get_mut(chip_id) {
            chip_state.confirm_container(container_id, container_label);
            // Update the stored telemetry state to Available now container is confirmed
            if let Some(tool_id) = self.chip_to_tool.get(chip_id).copied() {
                if let Some(tel) = self.latest.get_mut(&tool_id) {
                    tel.state = OperationalState::Available;
                }
            }
        }
    }

    /// Get the latest raw telemetry for a tool (may be stale for kinetic chips)
    pub fn get(&self, tool_id: &Uuid) -> Option<&IoTTelemetry> {
        self.latest.get(tool_id)
    }

    /// Get the kinetic chip state for a given chip ID
    pub fn chip_state(&self, chip_id: &str) -> Option<&KineticChipState> {
        self.chip_states.get(chip_id)
    }

    /// Resolve a chip ID to its tool UUID
    pub fn tool_id_for_chip(&self, chip_id: &str) -> Option<Uuid> {
        self.chip_to_tool.get(chip_id).copied()
    }

    /// Returns the current inferred `OperationalState` for a tool.
    /// For kinetic chips, this is inferred from the state machine + elapsed time.
    /// For tools with no chip registered, returns `Available` (assume present).
    pub fn state_of(&self, tool_id: &Uuid) -> OperationalState {
        // Find the chip for this tool
        let chip_id = self
            .chip_to_tool
            .iter()
            .find(|(_, tid)| *tid == tool_id)
            .map(|(cid, _)| cid.as_str());

        match chip_id {
            Some(cid) => {
                match self.chip_states.get(cid) {
                    Some(state) => state.infer_state(
                        self.thresholds.stale_hours,
                        self.thresholds.missing_hours,
                    ),
                    None => OperationalState::Stationary,
                }
            }
            None => {
                // No chip — fall back to stored telemetry state, or Available
                self.latest
                    .get(tool_id)
                    .map(|t| t.state.clone())
                    .unwrap_or(OperationalState::Available)
            }
        }
    }

    /// Returns the last-known location label for a tool.
    /// For kinetic chips this is the container at last confirmed Arrival,
    /// not the current (unknown mid-transit) position.
    pub fn last_known_location_of(&self, tool_id: &Uuid) -> String {
        // Try chip state first — it has the confirmed container label
        let chip_id = self
            .chip_to_tool
            .iter()
            .find(|(_, tid)| *tid == tool_id)
            .map(|(cid, _)| cid.as_str());

        if let Some(cid) = chip_id {
            if let Some(state) = self.chip_states.get(cid) {
                if let Some(ref label) = state.last_known_container {
                    return label.clone();
                }
            }
        }

        // Fall back to raw telemetry zone label
        self.latest
            .get(tool_id)
            .map(|t| t.location.zone_label.clone())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    /// Backwards-compatible alias — returns last-known location for display
    pub fn location_of(&self, tool_id: &Uuid) -> String {
        self.last_known_location_of(tool_id)
    }

    /// Returns how many hours have passed since this tool's chip last fired.
    /// Returns `None` if the chip has never been heard from.
    /// For kinetic chips, long silence is normal — do not confuse with missing.
    pub fn hours_since_last_event(&self, tool_id: &Uuid) -> Option<f64> {
        let chip_id = self
            .chip_to_tool
            .iter()
            .find(|(_, tid)| *tid == tool_id)
            .map(|(cid, _)| cid.as_str())?;
        self.chip_states
            .get(chip_id)
            .and_then(|s| s.hours_since_last_event())
    }

    /// Iterate all currently tracked tools and their latest raw telemetry
    pub fn all(&self) -> impl Iterator<Item = (&Uuid, &IoTTelemetry)> {
        self.latest.iter()
    }

    /// Iterate all registered kinetic chip states
    pub fn all_chip_states(&self) -> impl Iterator<Item = &KineticChipState> {
        self.chip_states.values()
    }

    /// Returns the last confirmed 3D space position for a tool.
    /// For kinetic chips in transit, returns the departure position (last resting spot).
    pub fn last_known_position_of(&self, tool_id: &Uuid) -> Option<[f32; 3]> {
        let chip_id = self
            .chip_to_tool
            .iter()
            .find(|(_, tid)| *tid == tool_id)
            .map(|(cid, _)| cid.as_str())?;
        self.chip_states
            .get(chip_id)
            .and_then(|s| s.last_known_position)
    }
}

// ============================================================================
// 5. ToolStatus — combined view for the Properties Panel and build guides
// ============================================================================

/// A combined snapshot of a tool's registry definition + live telemetry.
/// This is what the Properties Panel and build guide resolver work with.
#[derive(Debug, Clone)]
pub struct ToolStatus {
    /// Stable tool UUID
    pub tool_id: Uuid,
    /// Human-readable name
    pub name: String,
    /// Home location label from the .tool.toml
    pub home_location: String,
    /// Current live state (from IoT telemetry, or Available if no chip registered)
    pub state: OperationalState,
    /// Current live location (from IoT telemetry, or home_location if no chip)
    pub current_location: String,
    /// Battery level if available
    pub battery_pct: Option<u8>,
    /// Timestamp of last telemetry update
    pub last_seen: Option<DateTime<Utc>>,
    /// Whether this tool has an IoT chip registered
    pub is_iot_tracked: bool,
}

impl ToolStatus {
    /// Build from a registered tool definition plus optional live telemetry
    pub fn from_tool_and_telemetry(
        tool_id: Uuid,
        name: String,
        home_location: String,
        has_iot: bool,
        telemetry: Option<&IoTTelemetry>,
    ) -> Self {
        match telemetry {
            Some(t) => Self {
                tool_id,
                name,
                home_location: home_location.clone(),
                state: t.state.clone(),
                current_location: t.location.zone_label.clone(),
                battery_pct: t.battery_pct,
                last_seen: Some(t.received_at),
                is_iot_tracked: has_iot,
            },
            None => Self {
                tool_id,
                name,
                home_location: home_location.clone(),
                state: if has_iot {
                    OperationalState::Unreachable
                } else {
                    OperationalState::Available
                },
                current_location: home_location,
                battery_pct: None,
                last_seen: None,
                is_iot_tracked: has_iot,
            },
        }
    }
}

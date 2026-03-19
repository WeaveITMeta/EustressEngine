# EustressEngine Requirements — The Cube

Maps The Cube hardware to the Eustress ECS simulation and workshop crate types.

---

## 1. Required Crate Features

| Feature flag | Crate | Purpose |
|---|---|---|
| `iot-mqtt` | `eustress-workshop` | MQTT subscriber for BLE gateway → broker → engine |
| `bevy-twin` | `eustress-workshop` | Spawn Cube mesh entity + live transform sync |
| `procurement` | `eustress-workshop` | Purchase list generation for unregistered tools |
| `realism/kinetic` | `eustress-common` | KineticEnergyState simulation component |

---

## 2. MaterialProperties — Per Component

### Housing — Aluminium 6061-T6 Hard Anodised

```toml
[material]
name = "Al6061-T6-HardAnodised"
role = "structural_housing"
youngs_modulus_gpa = 68.9
poissons_ratio = 0.33
yield_strength_mpa = 276.0
ultimate_strength_mpa = 310.0
fracture_toughness_mpa_sqrt_m = 29.0
hardness_vickers = 150.0        # Type III hard anodise surface
thermal_conductivity_w_mk = 167.0
specific_heat_j_kgk = 896.0
thermal_expansion_ppm_k = 23.6
melting_point_c = 582.0
density_kg_m3 = 2700.0
friction_static = 0.10
friction_kinetic = 0.08
restitution = 0.40

[material.custom]
anodise_type = "Type III hard anodise MIL-A-8625"
ip_rating = "IP68"
corrosion_resistance = "indefinite_anodised"
emf_shielding_db = 40.0
```

### Piezoelectric Disc Array — PZT-5H

```toml
[material]
name = "PZT-5H"
role = "kinetic_energy_harvester_primary"
youngs_modulus_gpa = 61.0
poissons_ratio = 0.31
yield_strength_mpa = 55.0
ultimate_strength_mpa = 55.0
fracture_toughness_mpa_sqrt_m = 0.9
hardness_vickers = 560.0
thermal_conductivity_w_mk = 1.5
specific_heat_j_kgk = 420.0
thermal_expansion_ppm_k = 4.5
melting_point_c = 193.0         # Curie temperature (depolarisation limit)
density_kg_m3 = 7800.0
friction_static = 0.20
friction_kinetic = 0.18
restitution = 0.15

[material.custom]
piezo_d33_pc_n = 593.0
piezo_d31_pc_n = -274.0
coupling_factor_k31 = 0.38
dielectric_constant = 3800.0
dissipation_factor = 0.020
fatigue_life_cycles = 1.0e9
disc_count = 3
disc_diameter_mm = 12.0
disc_thickness_mm = 0.5
configuration = "unimorph_parallel"
```

### EM Proof Mass — NdFeB N52

```toml
[material]
name = "NdFeB-N52"
role = "kinetic_energy_harvester_secondary_proof_mass"
youngs_modulus_gpa = 160.0
poissons_ratio = 0.24
yield_strength_mpa = 80.0
ultimate_strength_mpa = 80.0
fracture_toughness_mpa_sqrt_m = 1.0
hardness_vickers = 610.0
thermal_conductivity_w_mk = 9.0
specific_heat_j_kgk = 460.0
thermal_expansion_ppm_k = 5.2
melting_point_c = 310.0         # Curie temperature
density_kg_m3 = 7500.0
friction_static = 0.0           # Magnetically levitated — zero contact
friction_kinetic = 0.0
restitution = 0.50

[material.custom]
remanence_tesla = 1.44
coercivity_ka_m = 876.0
max_energy_product_kj_m3 = 398.0
max_operating_temp_c = 80.0
levitation_type = "bistable_magnetic_spring"
proof_mass_diameter_mm = 8.0
proof_mass_mass_g = 2.3
oscillation_travel_mm = 0.3
resonant_frequency_hz = 12.0
```

### PCB Substrate — FR4

```toml
[material]
name = "FR4-ENIG"
role = "pcb_substrate"
youngs_modulus_gpa = 22.0
poissons_ratio = 0.28
yield_strength_mpa = 240.0
ultimate_strength_mpa = 415.0
fracture_toughness_mpa_sqrt_m = 1.2
hardness_vickers = 30.0
thermal_conductivity_w_mk = 0.29
specific_heat_j_kgk = 1150.0
thermal_expansion_ppm_k = 14.0   # In-plane; Z-axis: 70 ppm/K
melting_point_c = 170.0          # Tg (glass transition)
density_kg_m3 = 1850.0
friction_static = 0.35
friction_kinetic = 0.30
restitution = 0.10

[material.custom]
layer_count = 4
copper_weight_oz = 2
surface_finish = "ENIG"
thickness_mm = 1.2
tg_celsius = 170.0
```

---

## 3. Instance File Structure

### File to Entity Mapping

| File | ECS Entity | Key Components |
|---|---|---|
| `cube.glb.toml` | Cube root entity | `Name`, `Transform`, `Visibility` |
| `cube.glb#Housing` | Housing mesh | `Mesh`, `MeshMaterial`, material=Al6061 |
| `cube.glb#PZTArray` | Piezo stack | `Mesh`, `KineticHarvesterComponent` |
| `cube.glb#EMCavity` | EM harvester | `Mesh`, `EMProofMassComponent` |
| `cube.glb#PCBA` | PCB + chips | `Mesh`, `ElectronicStateComponent` |

### Standard `.glb.toml` Section Template

```toml
[instance]
id = "voltec-cube-v1"
mesh = "eustress/crates/workshop/cube/V1/meshes/cube.glb"
name = "Voltec Cube"

[instance.transform]
translation = [0.0, 0.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]   # identity quaternion [x,y,z,w]
scale = [0.001, 0.001, 0.001]      # mm → metres

[instance.components.kinetic_energy]
phase                 = "uninitialized"
harvested_uj          = 0.0
bank_mv               = 0.0        # 10 mF ceramic supercap bank voltage
bank_capacity_mf      = 10.0      # 10 × 1 mF ceramics in parallel
bank_max_mv           = 3300.0
ble_fire_threshold_mv = 3000.0    # BLE fires here — always within single-event budget
gps_fire_threshold_mv = 2650.0    # GPS + LTE-M fires here — ~35 mJ stored
ble_active            = false
gps_active            = false
active_elapsed_ms     = 0.0
ble_window_ms         = 200.0
gps_window_ms         = 2000.0
event_seq             = 0
total_events          = 0
gps_events            = 0
temp_c                = 22.0

[instance.components.electronic_state]
chip_id          = ""             # filled at registration time
firmware_version = "0.1.0"
rssi_dbm         = -80
temp_c           = 22.0
active           = false
kinetic_powered  = true
chip_model       = "nRF9161-SICA" # BLE 5.3 + LTE-M + GNSS — single SiP
has_gps          = true
has_lte_m        = true
has_ble          = true
```

### Transform Layout

| Entity | Translation (mm) | Scale | Notes |
|---|---|---|---|
| Root | `[0, 0, 0]` | `[0.001, 0.001, 0.001]` | mm→m at instance level |
| Housing (bottom) | `[0, 0, 0]` | `[1, 1, 1]` | 1mm wall, 18×18×4mm |
| Housing (top lid) | `[0, 0, 7]` | `[1, 1, 1]` | 1mm lid (8mm total height) |
| PCB | `[0, 0, 1]` | `[1, 1, 1]` | 1.2mm thick |
| PZT array | `[0, 0, 2.5]` | `[1, 1, 1]` | 1.5mm stack |
| EM cavity + coil | `[0, 0, 4.5]` | `[1, 1, 1]` | 2.5mm cavity |
| nRF9161 SiP | `[3, 2, 2.2]` | `[1, 1, 1]` | 10×16mm footprint |

---

## 4. KineticEnergyState — Domain-Specific State

Tracks the dual-mode energy harvesting and discharge cycle in the simulation.
The 10 mF bank accumulates across events; BLE fires cheaply every event;
GPS fires autonomously when the bank crosses the 35 mJ threshold.

### Fields

| Field | Type | Unit | Initial | Description |
|---|---|---|---|---|
| `phase` | `KineticPhase` | — | `Uninitialized` | Departure/Arrival state machine phase |
| `harvested_uj` | `f32` | μJ | `0.0` | Energy harvested this event |
| `bank_mv` | `f32` | mV | `0.0` | 10 mF supercap bank voltage |
| `bank_capacity_mf` | `f32` | mF | `10.0` | Bank capacitance (10 × 1 mF in parallel) |
| `bank_max_mv` | `f32` | mV | `3300.0` | Bank max rated voltage |
| `ble_fire_threshold_mv` | `f32` | mV | `3000.0` | BLE fires at this voltage (~45 μJ, always reachable) |
| `gps_fire_threshold_mv` | `f32` | mV | `2650.0` | GPS + LTE-M fires at this voltage (~35 mJ stored) |
| `ble_active` | `bool` | — | `false` | BLE advertisement window active |
| `gps_active` | `bool` | — | `false` | GPS + LTE-M window active |
| `rest_confirmed` | `bool` | — | `false` | True when tool still for `gps_wait_for_rest_ms` |
| `rest_elapsed_ms` | `f32` | ms | `0.0` | Continuous stillness duration since last motion |
| `gps_wait_for_rest_ms` | `f32` | ms | `500.0` | Stillness required before GPS fires |
| `active_elapsed_ms` | `f32` | ms | `0.0` | Time into current active window |
| `ble_window_ms` | `f32` | ms | `200.0` | BLE active window duration |
| `gps_window_ms` | `f32` | ms | `2000.0` | GPS + LTE-M active window duration |
| `event_seq` | `u16` | count | `0` | Per-event sequence counter (wraps at 65535) |
| `total_events` | `u64` | count | `0` | Lifetime motion event count |
| `gps_events` | `u64` | count | `0` | Lifetime GPS fire count |
| `temp_c` | `f32` | °C | `22.0` | Module temperature |

### Runtime Update Flow

```
Each Bevy Update tick:
  if ble_active:
    active_elapsed_ms += delta_ms
    bank_mv -= ble_discharge_rate_mv_per_ms() * delta_ms
    if active_elapsed_ms >= ble_window_ms:
      ble_active = false
      active_elapsed_ms = 0.0

  if gps_active:
    active_elapsed_ms += delta_ms
    bank_mv -= gps_discharge_rate_mv_per_ms() * delta_ms
    if active_elapsed_ms >= gps_window_ms:
      gps_active = false
      active_elapsed_ms = 0.0
      gps_events += 1

  # Rest timer: accumulate stillness between events
  if not ble_active and not gps_active and phase == AtRest:
    rest_elapsed_ms += delta_ms
    if rest_elapsed_ms >= gps_wait_for_rest_ms:
      rest_confirmed = true

On motion event (MQTT received):
  harvested_uj = simulate_harvest(acceleration_g, duration_ms)
  bank_mv = min(bank_mv + charge_delta_mv(harvested_uj), bank_max_mv)
  event_seq = (event_seq + 1) % 65536
  total_events += 1

  # Reset rest state — tool is moving again
  rest_confirmed = false
  rest_elapsed_ms = 0.0

  phase = advance_kinetic_phase(phase, current_position)

  # BLE fires on every event (departure AND arrival)
  if bank_mv >= ble_fire_threshold_mv:
    ble_active = true
    active_elapsed_ms = 0.0

  # GPS fires ONLY on arrival after rest is confirmed
  # The packet's is_arrival flag is the gate — firmware enforces this
  if is_arrival and rest_confirmed and bank_mv >= gps_fire_threshold_mv:
    gps_active = true
    active_elapsed_ms = 0.0
    rest_confirmed = false   # consume the rest confirmation
```

**Why wait for rest before GPS?**  
A GPS fix takes ~800ms. If fired on departure, the tool is mid-carry and the
coordinates reflect an in-transit position — not where the tool lives.
Firing on arrival after 500ms of stillness guarantees the recorded position is
the tool’s actual resting place: the bench, the drawer, the job box. Every GPS
fix in the movement log is a verified storage position, not noise.

### Bank Voltage Helper Functions

```rust
// mV rise from harvested energy: ΔV = ΔE / (C × V_current) — approximate linear for small ΔV
fn charge_delta_mv(harvested_uj: f32, bank_mv: f32, bank_capacity_mf: f32, pmic_efficiency: f32) -> f32 {
    let delta_e_j = (harvested_uj * 1e-6) * pmic_efficiency;
    let capacitance_f = bank_capacity_mf * 1e-3;
    // ΔV from ΔE: V_new = √(V_old² + 2ΔE/C)
    let v_old = bank_mv / 1000.0;
    let v_new = (v_old * v_old + 2.0 * delta_e_j / capacitance_f).sqrt();
    (v_new - v_old) * 1000.0  // back to mV
}

// Energy stored in bank at given voltage: E = ½CV²
fn bank_energy_mj(bank_mv: f32, bank_capacity_mf: f32) -> f32 {
    let v = bank_mv / 1000.0;
    let c = bank_capacity_mf / 1000.0;
    0.5 * c * v * v * 1000.0  // mJ
}

// BLE discharge: ~65 μJ over 200ms window → 0.325 μJ/ms → 0.108 mV/ms at 3.0V, 10mF
fn ble_discharge_rate_mv_per_ms() -> f32 { 0.11 }

// GPS discharge: ~35 mJ over 2000ms window → 17.5 μJ/ms → 5.83 mV/ms at 2.65V, 10mF
fn gps_discharge_rate_mv_per_ms() -> f32 { 5.83 }
```

### Energy Simulation Functions

```rust
// Energy stored in supercap: E = ½CV²
fn supercap_energy_uj(cap_uf: f32, voltage_mv: f32) -> f32 {
    0.5 * cap_uf * 1e-6 * (voltage_mv * 1e-3).powi(2) * 1e6
}

// Charge from harvest event
fn charge_from_harvest(harvested_uj: f32, pmic_efficiency: f32) -> f32 {
    harvested_uj * pmic_efficiency  // pmic_efficiency ≈ 0.85
}

// Discharge rate during active window (nRF52840 BLE burst)
fn discharge_rate_uj_per_ms(active_elapsed_ms: f32) -> f32 {
    if active_elapsed_ms < 800.0 {
        0.66  // BLE scan + advertisement phase: 0.22mA × 3V = 0.66 mW = 0.66 μJ/ms
    } else {
        0.21  // Idle + processing: 0.07mA × 3V = 0.21 μJ/ms
    }
}
```

---

## 5. ThermodynamicState

| Field | Type | Unit | Initial |
|---|---|---|---|
| `housing_temp_c` | `f32` | °C | `22.0` |
| `pcb_temp_c` | `f32` | °C | `22.0` |
| `magnet_temp_c` | `f32` | °C | `22.0` |
| `ambient_temp_c` | `f32` | °C | `22.0` |
| `active_power_mw` | `f32` | mW | `0.0` |
| `thermal_resistance_k_w` | `f32` | K/W | `15.0` |

### Operating Envelope

| Condition | Min | Max | Critical component |
|---|---|---|---|
| Ambient temperature | −40°C | +85°C | nRF52840 |
| Housing temperature | −40°C | +85°C | nRF52840 junction |
| Magnet temperature | −40°C | +80°C | NdFeB N52 Curie limit |
| PCB solder temperature | — | +260°C | SAC305 reflow only |

---

## 6. Domain Laws

### Kinetic Harvesting Laws

| Function | Description | Calibrated constants |
|---|---|---|
| `piezo_harvest(accel_g, duration_ms)` | PZT-5H unimorph output model | d₃₁=−274 pC/N, E=61 GPa, η=0.60 |
| `em_harvest(vel_m_s, freq_hz)` | EM coil induction output | N=200 turns, B_r=1.44T, A=1.54×10⁻⁴m² |
| `pmic_rectify(v_ac_peak)` | AEM10941 MPPT efficiency | η=0.85 at >400mV input |
| `supercap_charge(energy_uj)` | Capacitor voltage rise | C=100μF, V_max=3.3V |
| `nrf_ble_burst_energy(n_packets)` | nRF52840 BLE advertisement | I=0.22mA, t=1.28ms/packet |

### Piezoelectric Energy Equation

```
E_piezo = (d₃₁² × E₁¹ × A_disc × n_discs) / (2 × ε₃₃) × ε_strain² × t_event

where:
  d₃₁   = −274 × 10⁻¹² C/N
  E₁¹   = 61 × 10⁹ Pa
  A_disc = π × (0.006)² = 1.13 × 10⁻⁴ m²
  n_discs = 3
  ε₃₃   = 3800 × 8.85 × 10⁻¹² = 3.36 × 10⁻⁸ F/m
  ε_strain = acceleration × t_event² / (2 × thickness) (bending strain model)
  t_event = shock duration (s)
```

### EM Induction Energy Equation

```
EMF = N × B_r × A_coil × (dx/dt) / l_coil
E_em = ∫ (EMF² / 4R_coil) dt  over carry swing duration

where:
  N      = 200 turns
  B_r    = 1.44 T
  A_coil = π × (0.007)² = 1.54 × 10⁻⁴ m²
  R_coil = 28 AWG, 200 turns, 14mm mean diameter ≈ 18 Ω
  dx/dt  = proof mass velocity (from oscillation model)
```

---

## 7. Realism Config

```toml
[realism]
enabled = true
kinetic_harvesting = true
thermal_simulation = true
ble_signal_propagation = false   # V1: simplified — gateway coverage zones only
gps_simulation = false           # V1: BLE-only; enable for V2 GPS edition

[realism.kinetic]
piezo_efficiency = 0.60
em_efficiency = 0.55
pmic_efficiency = 0.85
supercap_capacity_uf = 100.0
fire_threshold_mv = 3000.0
min_acceleration_g = 0.5
min_duration_ms = 80.0

[realism.thermal]
thermal_resistance_k_w = 15.0
ambient_temp_c = 22.0
active_power_mw = 150.0

[realism.lifetime]
piezo_fatigue_limit_cycles = 1.0e9
supercap_cycle_limit = 500000
magnet_max_temp_c = 80.0
simulate_wear = false            # No wear surfaces — skip wear simulation
```

---

## 8. Structural Bundle Requirements

### Cube Entity Bundle

```rust
#[derive(Bundle)]
pub struct CubeBundle {
    pub name: Name,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
    pub visibility: Visibility,
    pub mesh: Mesh3d,
    pub material: MeshMaterial3d<StandardMaterial>,

    // Workshop integration
    pub tool_component: ToolComponent,      // links to .tool.toml registry entry

    // Cube-specific simulation state
    pub kinetic_energy: KineticEnergyState,
    pub electronic_state: CubeElectronicState,
    pub thermal_state: CubeThermalState,
}
```

### KineticEnergyState Component

```rust
#[derive(Component, Reflect, Clone, Debug, Default)]
#[reflect(Component)]
pub struct KineticEnergyState {
    pub phase: String,                  // mirrors KineticPhase display label
    pub harvested_uj: f32,
    pub supercap_mv: f32,
    pub supercap_capacity_uf: f32,
    pub fire_threshold_mv: f32,
    pub active: bool,
    pub active_elapsed_ms: f32,
    pub active_window_ms: f32,
    pub total_events: u64,
    pub temp_c: f32,
}
```

### CubeElectronicState Component

```rust
#[derive(Component, Reflect, Clone, Debug)]
#[reflect(Component)]
pub struct CubeElectronicState {
    pub chip_id: String,
    pub firmware_version: String,
    pub rssi_dbm: i16,
    pub temp_c: f32,
    pub active: bool,
    pub kinetic_powered: bool,
}
```

---

## 9. Gateway Node ECS Representation

Gateway nodes (Voltec G1 fixed hardware and stationary phone nodes) are represented
as Bevy resources and entities alongside the tool twins. The position fusion system
reads all `GatewayNode` resources when resolving a tool's indoor position.

### GatewayNode Resource

```rust
#[derive(Resource, Reflect, Clone, Debug)]
pub struct GatewayNode {
    /// Unique gateway identifier — matches MQTT gateway_id field
    pub id: String,
    /// Physical position in workshop space (metres, Y-up)
    pub position: Vec3,
    /// True if this node has been calibrated in the current workshop session
    pub calibrated: bool,
    /// Gateway hardware type — determines positioning method available
    pub hardware_type: GatewayHardwareType,
    /// True if the node is currently active and contributing to position fusion
    pub online: bool,
    /// Reason the node went offline (moved, disconnected, etc.)
    pub offline_reason: Option<String>,
    /// Timestamp of last received scan packet
    pub last_seen_ms: f64,
}

#[derive(Reflect, Clone, Debug, PartialEq, Eq)]
pub enum GatewayHardwareType {
    /// Voltec G1 — nRF21540 + 4-element antenna array, AoA-capable (±0.3–1m)
    VoltecG1,
    /// Generic fixed node — RSSI only (±2–5m)
    GenericFixed,
    /// Phone in stationary mode — RSSI only (±2–5m), removed on movement
    PhoneStationary,
}
```

### Phone Gateway Lifecycle

The phone gateway follows a strict lifecycle enforced by the accelerometer:

```
PhoneStationary state machine:

  UNCALIBRATED
      │ user taps position on floor plan
      ▼
  CALIBRATING
      │ user confirms — phone placed face-up, still
      ▼
  ONLINE  ←──────────────────────────────────────┐
      │ (contributing to position fusion)         │
      │ accelerometer detects movement             │
      ▼                                           │
  OFFLINE ("moved")                               │
      │ (excluded from fusion)                    │
      │ user re-calibrates                        │
      └───────────────────────────────────────────┘
```

When `hardware_type == PhoneStationary` and `online == false`, the node's RSSI
readings are **excluded** from `ContainerIndex::fuse_rssi()`. The server publishes
a `gateway_offline` event to notify all subscribers.

### Position Fusion Priority

When resolving a tool's indoor position, the fusion system uses the best available
method from available online gateways:

| Method | Minimum nodes | Accuracy | Selected when |
|---|---|---|---|
| `aoa_trilateration` | 2× VoltecG1 online | ±0.3–1m | ≥2 AoA nodes see the Cube |
| `rssi_trilateration` | 3× any online | ±2–5m | ≥3 RSSI nodes, no AoA |
| `rssi_bilateration` | 2× any online | ±1–3m (ambiguous) | Only 2 nodes visible |
| `zone_only` | 1× any online | Zone label | Only 1 node visible |
| `gps_outdoor` | — | 1.8m CEP | No gateways, GPS fired |

The `position_method` field in the fused telemetry payload records which method
was used so the Eustress UI can show a confidence indicator.

### Calibration ECS Flow

```
On CalibrationWand event received:
  For each ground-truth point (known Vec3 + observed RSSI/AoA per gateway):
    PathLossModel::update(gateway_id, observed_rssi, known_distance)
    AoAOffsetModel::update(gateway_id, observed_angle, known_angle)

  workshop.toml written with fitted models
  All GatewayNode resources: calibrated = true
```

---

## 10. Blender Mesh Requirements (V1)

Script location: `cube/V1/meshes/scripts/generate_cube.py`

| Mesh object | Poly budget | Material slot | Notes |
|---|---|---|---|
| `Housing_Bottom` | 800 tris | `Al6061_Anodised` | Chamfered edges, M3 boss features |
| `Housing_Top` | 600 tris | `Al6061_Anodised` | Thin lid, chamfered |
| `PCB` | 400 tris | `FR4_ENIG` | Green substrate, gold pads visible |
| `nRF52840` | 200 tris | `IC_Black_Mold` | QFN-48 package shape |
| `AEM10941` | 100 tris | `IC_Black_Mold` | QFN-20 package shape |
| `Supercap` | 100 tris | `Capacitor_Silver` | Cylindrical 1206 footprint |
| `PZT_Array` | 300 tris | `PZT_Grey` | 3 disc stack, slight grey-white tint |
| `EM_Cavity` | 200 tris | `FR4_ENIG` | Cylindrical bore through PCB |
| `NdFeB_Sphere` | 200 tris | `NdFeB_Chrome` | Shiny chrome sphere, 8mm |
| `Coil_Winding` | 500 tris | `Copper_Coil` | Helical winding visible |
| `Oring_Gasket` | 200 tris | `Silicone_Black` | Recessed O-ring groove |
| **Total** | **3,600 tris** | | Well within mobile/realtime budget |

**Export settings:**
- Format: glTF 2.0 binary (`.glb`)
- Include: Normals, UVs, Tangents, Materials
- Draco compression: enabled (level 6)
- Y-up coordinate system (Bevy convention)
- Scale: 1 unit = 1 mm (rescaled at runtime by instance transform)

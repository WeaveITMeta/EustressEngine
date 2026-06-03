use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use super::constants::*;

// ── Marker ────────────────────────────────────────────────────────────────────

/// Marks an entity as the ARC-1 arc reactor core.
/// All nuclear simulation systems query for this component.
#[derive(Component, Reflect, Clone, Debug, Default, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ArcReactorCore;

// ── Point kinetics ────────────────────────────────────────────────────────────

/// One-group point kinetics state for the ARC-1 fission core.
///
/// Physics:  dn/dt = ((ρ − β) / Λ) · n + λ · C
///           dC/dt = (β / Λ) · n − λ · C
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct NuclearKineticsComponent {
    /// Normalised neutron population n (1.0 = steady-state critical).
    pub neutron_population: f32,
    /// Delayed-neutron precursor concentration C [arbitrary units].
    pub precursor_concentration: f32,
    /// Current reactivity ρ [Δk/k] — updated each frame from rod + Doppler.
    pub reactivity: f32,
    /// Effective delayed-neutron fraction β.
    pub beta: f32,
    /// Mean prompt-neutron generation time Λ [s].
    pub mean_generation_time: f32,
    /// Precursor decay constant λ [s⁻¹].
    pub precursor_decay_constant: f32,
    /// True when the reactor has been SCRAMmed.
    pub is_scrammed: bool,
    /// Seconds elapsed since shutdown (used for decay-heat calculation).
    pub shutdown_seconds: f32,
}

impl Default for NuclearKineticsComponent {
    fn default() -> Self {
        Self {
            neutron_population: 1.0,
            precursor_concentration: BETA / (MEAN_GENERATION_TIME * PRECURSOR_DECAY_CONSTANT),
            reactivity: 0.0,
            beta: BETA,
            mean_generation_time: MEAN_GENERATION_TIME,
            precursor_decay_constant: PRECURSOR_DECAY_CONSTANT,
            is_scrammed: false,
            shutdown_seconds: 0.0,
        }
    }
}

// ── Control rods ──────────────────────────────────────────────────────────────

/// Two-bank control-rod system.
///
/// `insertion_pct` ranges 0 (fully withdrawn) to 100 (fully inserted).
/// Rod worth is symmetric: ρ_rod = (0.5 − rod_insertion) · ROD_WORTH_FULL,
/// where rod_insertion = (bank_a + bank_b) / 200.
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ControlRodBankComponent {
    /// Bank A insertion percentage [0–100].
    pub bank_a_pct: f32,
    /// Bank B insertion percentage [0–100].
    pub bank_b_pct: f32,
}

impl Default for ControlRodBankComponent {
    fn default() -> Self {
        Self { bank_a_pct: 50.0, bank_b_pct: 50.0 }
    }
}

impl ControlRodBankComponent {
    /// Fractional insertion [0.0–1.0] averaged over both banks.
    #[inline]
    pub fn insertion_fraction(&self) -> f32 {
        (self.bank_a_pct + self.bank_b_pct) / 200.0
    }

    /// Reactivity contribution from rod position alone [Δk/k].
    #[inline]
    pub fn rod_reactivity(&self) -> f32 {
        (ROD_EQUILIBRIUM_INSERTION - self.insertion_fraction()) * ROD_WORTH_FULL
    }
}

// ── Thermal-hydraulics ────────────────────────────────────────────────────────

/// Thermal state of the ARC-1 core and its coolant loop.
///
/// dT_core/dt = (P_th − P_removed) · CORE_THERMAL_INERTIA
///            − (T_core − T_ambient) · PASSIVE_HEAT_LOSS_COEFF
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ThermalHydraulicsComponent {
    /// Core centre-line temperature [°C].
    pub core_temp_celsius: f32,
    /// Bulk coolant exit temperature [°C].
    pub coolant_temp_celsius: f32,
    /// Coolant mass-flow rate as % of rated design flow [0–100].
    pub coolant_flow_pct: f32,
    /// Instantaneous thermal power generated in the core [W].
    pub thermal_power_watts: f32,
    /// Rated full-power thermal output [W] — set once at spawn.
    pub rated_power_watts: f32,
    /// Auto-SCRAM threshold [°C].
    pub scram_temp_celsius: f32,
    /// Heat removed to the coolant this frame [W].
    pub heat_removed_watts: f32,
    /// Residual decay-heat power post-SCRAM [W].
    pub decay_heat_watts: f32,
}

impl Default for ThermalHydraulicsComponent {
    fn default() -> Self {
        Self {
            core_temp_celsius: NOMINAL_CORE_TEMP_CELSIUS,
            coolant_temp_celsius: NOMINAL_CORE_TEMP_CELSIUS * COOLANT_TEMP_RATIO * 0.7,
            coolant_flow_pct: 70.0,
            thermal_power_watts: 0.0,
            rated_power_watts: RATED_THERMAL_POWER_WATTS,
            scram_temp_celsius: SCRAM_TEMP_CELSIUS,
            heat_removed_watts: 0.0,
            decay_heat_watts: 0.0,
        }
    }
}

// ── Power conversion ──────────────────────────────────────────────────────────

/// Combined thermoelectric + Stirling power-conversion stage.
///
/// η_total = η_TE + η_St · (1 − η_TE)
/// P_elec  = P_th · η_total
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct PowerConversionComponent {
    /// Thermoelectric module efficiency [fraction 0–1].
    pub te_efficiency: f32,
    /// Stirling engine efficiency [fraction 0–1].
    pub stirling_efficiency: f32,
    /// Combined system efficiency [fraction 0–1] — computed each frame.
    pub total_efficiency: f32,
    /// Net electrical power delivered to the bus [W].
    pub electrical_output_watts: f32,
}

impl Default for PowerConversionComponent {
    fn default() -> Self {
        let te = DEFAULT_TE_EFF;
        let st = DEFAULT_STIRLING_EFF;
        Self {
            te_efficiency: te,
            stirling_efficiency: st,
            total_efficiency: te + st * (1.0 - te),
            electrical_output_watts: 0.0,
        }
    }
}

// ── V-Cell battery buffer ─────────────────────────────────────────────────────

/// Vanadium-flow / advanced chemistry battery buffer for peak-load shaving.
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct VCellBatteryComponent {
    /// State of charge [0–100 %].
    pub state_of_charge_pct: f32,
    /// Energy density [Wh/kg].
    pub capacity_wh_per_kg: f32,
    /// Maximum burst discharge power [W].
    pub peak_burst_watts: f32,
    /// Current electrical load demand [W].
    pub load_demand_watts: f32,
    /// Power surplus (+) or deficit (−) relative to load [W].
    pub power_balance_watts: f32,
}

impl Default for VCellBatteryComponent {
    fn default() -> Self {
        Self {
            state_of_charge_pct: 82.0,
            capacity_wh_per_kg: VCELL_CAPACITY_WH_PER_KG,
            peak_burst_watts: VCELL_PEAK_BURST_W,
            load_demand_watts: 280.0,
            power_balance_watts: 0.0,
        }
    }
}

// ── PID controller ────────────────────────────────────────────────────────────

/// Single PID loop state.  Three of these live inside `ArcReactorAIController`.
#[derive(Reflect, Clone, Debug, Serialize, Deserialize)]
pub struct PidState {
    pub kp: f32,
    pub ki: f32,
    pub kd: f32,
    pub setpoint: f32,
    pub integral: f32,
    pub prev_error: f32,
    pub output_min: f32,
    pub output_max: f32,
    pub enabled: bool,
}

impl PidState {
    pub fn new(kp: f32, ki: f32, kd: f32, setpoint: f32, out_min: f32, out_max: f32) -> Self {
        Self { kp, ki, kd, setpoint, integral: 0.0, prev_error: 0.0, output_min: out_min, output_max: out_max, enabled: true }
    }

    /// Compute one step of the PID.  Returns the clamped control output.
    pub fn update(&mut self, measured: f32, dt: f32) -> f32 {
        if !self.enabled { return 0.0; }
        let error = self.setpoint - measured;
        self.integral = (self.integral + error * dt).clamp(self.output_min / self.ki.max(1e-6), self.output_max / self.ki.max(1e-6));
        let derivative = if dt > 0.0 { (error - self.prev_error) / dt } else { 0.0 };
        self.prev_error = error;
        (self.kp * error + self.ki * self.integral + self.kd * derivative).clamp(self.output_min, self.output_max)
    }

    pub fn reset(&mut self) {
        self.integral = 0.0;
        self.prev_error = 0.0;
    }
}

/// Autonomous AI controller that wraps three PID loops to keep the ARC-1
/// stable under varying load, startup transients, and disturbances.
///
/// Operating modes:
/// - `Standby`  — all PIDs idle; manual rod/flow control only.
/// - `PowerFollow` — power-PID drives rod insertion to match load demand.
/// - `Regulation` — reactivity-PID + thermal-PID both active (primary mode).
/// - `EmergencyShutdown` — rods fully inserted, coolant at max, PIDs disabled.
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct ArcReactorAIController {
    /// Current operating mode.
    pub mode: ReactorControlMode,
    /// PID that drives rod insertion to keep neutron population near setpoint.
    pub reactivity_pid: PidState,
    /// PID that drives coolant flow to keep core temperature near target.
    pub thermal_pid: PidState,
    /// PID that blends rod + flow to track load demand.
    pub power_pid: PidState,
    /// Target neutron population (normalised; 1.0 = full power).
    pub neutron_setpoint: f32,
    /// Target core temperature [°C].
    pub temp_setpoint_celsius: f32,
    /// Target electrical output [W]; derived from load demand each frame.
    pub power_setpoint_watts: f32,
    /// Seconds since the last automatic SCRAM.
    pub time_since_scram: f32,
    /// Whether the AI controller is allowed to override manual inputs.
    pub ai_override_enabled: bool,
}

#[derive(Reflect, Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum ReactorControlMode {
    #[default]
    Standby,
    PowerFollow,
    Regulation,
    EmergencyShutdown,
}

impl Default for ArcReactorAIController {
    fn default() -> Self {
        Self {
            mode: ReactorControlMode::Regulation,
            reactivity_pid: PidState::new(PID_REACTIVITY_KP, PID_REACTIVITY_KI, PID_REACTIVITY_KD, 1.0, -30.0, 30.0),
            thermal_pid:    PidState::new(PID_THERMAL_KP,    PID_THERMAL_KI,    PID_THERMAL_KD,    NOMINAL_CORE_TEMP_CELSIUS, -40.0, 40.0),
            power_pid:      PidState::new(PID_POWER_KP,      PID_POWER_KI,      PID_POWER_KD,      420.0, -20.0, 20.0),
            neutron_setpoint: 1.0,
            temp_setpoint_celsius: NOMINAL_CORE_TEMP_CELSIUS,
            power_setpoint_watts: 420.0,
            time_since_scram: 0.0,
            ai_override_enabled: true,
        }
    }
}

// ── Safety message ────────────────────────────────────────────────────────────

/// Fired when a safety limit is breached and an automatic SCRAM is initiated.
/// Uses Bevy 0.18 Message derive.
#[derive(Event, Message, Clone, Debug)]
pub struct ReactorScramMessage {
    pub entity: Entity,
    pub reason: ScramReason,
}

#[derive(Clone, Debug)]
pub enum ScramReason {
    TemperatureExceeded { celsius: f32 },
    NeutronExcursion    { population: f32 },
    BatteryCritical     { soc_pct: f32 },
    ManualScram,
}

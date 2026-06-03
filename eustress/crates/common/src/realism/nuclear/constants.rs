/// Nuclear physics constants for the ARC-1 fission core simulation.
///
/// Values match the ARC-1 dashboard physics model and are grounded in
/// realistic compact research-reactor parameters (TRIGA / micro-reactor class).

// ── Point kinetics ────────────────────────────────────────────────────────────

/// Effective delayed-neutron fraction (β) for U-235/Pu-239 mixed fuel.
pub const BETA: f32 = 0.0065;

/// Mean prompt-neutron generation time Λ [seconds].
pub const MEAN_GENERATION_TIME: f32 = 2.5e-5;

/// Effective decay constant for the one-group precursor model λ [s⁻¹].
pub const PRECURSOR_DECAY_CONSTANT: f32 = 0.08;

// ── Reactivity model ──────────────────────────────────────────────────────────

/// Differential rod worth for a fully inserted bank: Δk/k per unit insertion.
/// Both banks at 100 % → rod_insertion = 1.0 → contributes −0.008 Δk/k each.
pub const ROD_WORTH_FULL: f32 = 0.008;

/// Baseline reactivity when rods are at 50 % insertion (equilibrium).
/// 0.5 − rod_insertion at 50 % = 0.0 → ρ_rod = 0.
pub const ROD_EQUILIBRIUM_INSERTION: f32 = 0.5;

/// Doppler (temperature) feedback coefficient [Δk/k per °C above 700 °C].
/// Negative value gives inherent self-shutdown on temperature rise.
pub const TEMP_FEEDBACK_COEFFICIENT: f32 = -3.0e-6;

/// Reference temperature for the Doppler feedback baseline [°C].
pub const TEMP_FEEDBACK_REFERENCE: f32 = 700.0;

// ── Thermal-hydraulics ────────────────────────────────────────────────────────

/// Nominal rated thermal power of the ARC-1 core [W].
pub const RATED_THERMAL_POWER_WATTS: f32 = 3_200.0;

/// Heat-transfer efficiency at 100 % coolant flow (fraction of Pth removed).
pub const COOLANT_HEAT_TRANSFER_EFF: f32 = 0.9;

/// Core thermal inertia coefficient — governs how fast core temp responds.
/// Higher → slower response.  Units: (°C / W) · frame-factor.
pub const CORE_THERMAL_INERTIA: f32 = 0.01;

/// Passive heat-loss coefficient to environment [(°C − 300 °C) → W·s⁻¹ per °C].
pub const PASSIVE_HEAT_LOSS_COEFF: f32 = 0.005;

/// Coolant-to-core temperature ratio at 100 % flow.
pub const COOLANT_TEMP_RATIO: f32 = 0.55;

/// Ambient / heat-sink temperature [°C].
pub const AMBIENT_TEMP_CELSIUS: f32 = 300.0;

/// Nominal steady-state core temperature [°C].
pub const NOMINAL_CORE_TEMP_CELSIUS: f32 = 847.0;

/// Automatic SCRAM temperature threshold [°C].
pub const SCRAM_TEMP_CELSIUS: f32 = 1_600.0;

// ── Decay heat (Way-Wigner correlation) ──────────────────────────────────────

/// Way-Wigner decay-heat coefficient: Q_d(t) = K · P₀ · t^(−α).
pub const DECAY_HEAT_COEFF: f32 = 0.066;

/// Way-Wigner time exponent.
pub const DECAY_HEAT_EXPONENT: f32 = -0.2;

// ── Power conversion ──────────────────────────────────────────────────────────

/// Default thermoelectric module efficiency η_TE [fraction].
pub const DEFAULT_TE_EFF: f32 = 0.14;

/// Default Stirling cycle efficiency η_St [fraction].
pub const DEFAULT_STIRLING_EFF: f32 = 0.28;

// ── V-Cell battery buffer ─────────────────────────────────────────────────────

/// Nominal energy density [Wh/kg] — vanadium-flow / advanced chemistry.
pub const VCELL_CAPACITY_WH_PER_KG: f32 = 1_000.0;

/// Peak burst discharge power [W].
pub const VCELL_PEAK_BURST_W: f32 = 5_000.0;

/// Charge rate coefficient [SoC % / W·s⁻¹] — how fast surplus power tops up.
pub const VCELL_CHARGE_RATE: f32 = 0.002;

/// Discharge rate coefficient [SoC % / W·s⁻¹] — how fast deficit drains it.
pub const VCELL_DISCHARGE_RATE: f32 = 0.005;

/// Low-battery warning threshold [%].
pub const VCELL_LOW_SOC_THRESHOLD: f32 = 10.0;

// ── Default PID tuning ────────────────────────────────────────────────────────

/// Reactivity PID — keeps neutron population n near 1.0 by nudging rod banks.
pub const PID_REACTIVITY_KP: f32 = 0.08;
pub const PID_REACTIVITY_KI: f32 = 0.012;
pub const PID_REACTIVITY_KD: f32 = 0.04;

/// Thermal PID — keeps core temperature near target by adjusting coolant flow.
pub const PID_THERMAL_KP: f32 = 0.15;
pub const PID_THERMAL_KI: f32 = 0.008;
pub const PID_THERMAL_KD: f32 = 0.06;

/// Power PID — tracks load demand by modulating rod insertion + coolant flow.
pub const PID_POWER_KP: f32 = 0.05;
pub const PID_POWER_KI: f32 = 0.004;
pub const PID_POWER_KD: f32 = 0.02;

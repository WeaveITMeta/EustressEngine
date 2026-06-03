//! Electrical power components and systems — DC/AC motors and DC-DC converters.
//!
//! All units are SI: watts, volts, amperes, ohms, henries, radians/sec, newton-metres.

use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Buck converter (step-down)
// ---------------------------------------------------------------------------

/// Step-down (buck) DC-DC converter.
///
/// `v_out = D * v_in * η`
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct BuckConverter {
    /// Duty cycle — fraction in `[0, 1]`.
    pub duty_cycle: f32,
    /// Input voltage (V).
    pub v_in: f32,
    /// Conversion efficiency in `(0, 1]`.
    pub efficiency: f32,
    /// Output voltage (V) — written by `update_buck_converter_system`.
    pub v_out: f32,
}

impl BuckConverter {
    pub fn new(duty_cycle: f32, v_in: f32, efficiency: f32) -> Self {
        Self {
            duty_cycle: duty_cycle.clamp(0.0, 1.0),
            v_in,
            efficiency: efficiency.clamp(f32::EPSILON, 1.0),
            v_out: 0.0,
        }
    }
}

impl Default for BuckConverter {
    fn default() -> Self {
        Self::new(0.5, 48.0, 0.95)
    }
}

// ---------------------------------------------------------------------------
// Boost converter (step-up)
// ---------------------------------------------------------------------------

/// Step-up (boost) DC-DC converter.
///
/// `v_out = v_in / (1 - D) * η`  — `D` clamped to `[0, 0.99]`.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct BoostConverter {
    /// Duty cycle — fraction in `[0, 0.99]`.
    pub duty_cycle: f32,
    /// Input voltage (V).
    pub v_in: f32,
    /// Conversion efficiency in `(0, 1]`.
    pub efficiency: f32,
    /// Output voltage (V) — written by `update_boost_converter_system`.
    pub v_out: f32,
}

impl BoostConverter {
    pub fn new(duty_cycle: f32, v_in: f32, efficiency: f32) -> Self {
        Self {
            duty_cycle: duty_cycle.clamp(0.0, 0.99),
            v_in,
            efficiency: efficiency.clamp(f32::EPSILON, 1.0),
            v_out: 0.0,
        }
    }
}

impl Default for BoostConverter {
    fn default() -> Self {
        Self::new(0.5, 24.0, 0.92)
    }
}

// ---------------------------------------------------------------------------
// DC motor — back-EMF model
// ---------------------------------------------------------------------------

/// Permanent-magnet DC motor modelled with back-EMF and armature inductance.
///
/// State variables: `current` (A), `omega` (rad/s).
/// Derived: `torque`, `power_in`, `power_out`, `efficiency`.
///
/// Equations:
/// ```text
/// dI/dt  = (V_a - I·Ra - Ke·ω) / La
/// dω/dt  = (Kt·I - τ_load) / J
/// torque  = Kt · I
/// power_in  = V_a · I
/// power_out = torque · ω
/// η     = power_out / power_in   (clamped to [0,1])
/// ```
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct DcMotor {
    // --- Parameters ---
    /// Back-EMF / torque constant `Ke = Kt` in SI (V·s/rad == N·m/A).
    pub ke: f32,
    /// Armature resistance (Ω).
    pub ra: f32,
    /// Armature inductance (H).
    pub la: f32,
    /// Rotor moment of inertia (kg·m²).
    pub inertia: f32,

    // --- Inputs (set each frame before the system runs) ---
    /// Applied armature voltage (V).
    pub voltage: f32,
    /// External load torque opposing rotation (N·m).
    pub load_torque: f32,

    // --- State ---
    /// Armature current (A).
    pub current: f32,
    /// Angular velocity (rad/s) — clamped >= 0.
    pub omega: f32,

    // --- Derived outputs ---
    /// Output torque (N·m).
    pub torque: f32,
    /// Electrical input power (W).
    pub power_in: f32,
    /// Mechanical output power (W).
    pub power_out: f32,
    /// Electromechanical efficiency `[0, 1]`.
    pub efficiency: f32,
}

impl DcMotor {
    /// Construct from motor constants.
    ///
    /// `kt = ke` by the SI identity for brush-type PM motors.
    pub fn new(ke: f32, ra: f32, la: f32, inertia: f32) -> Self {
        Self {
            ke,
            ra,
            la,
            inertia,
            voltage: 0.0,
            load_torque: 0.0,
            current: 0.0,
            omega: 0.0,
            torque: 0.0,
            power_in: 0.0,
            power_out: 0.0,
            efficiency: 0.0,
        }
    }

    /// Derive motor constants from nameplate data.
    ///
    /// Assumptions:
    /// - Back-EMF `Ke` obtained from rated speed at 90% of rated voltage.
    /// - Armature resistance accounts for 10% resistive drop at rated current.
    /// - Inductance set via 2 ms L/R time constant.
    /// - Inertia estimated as `0.01 * sqrt(P_rated)` kg·m².
    pub fn from_specs(rated_power_w: f32, rated_rpm: f32, rated_voltage: f32) -> Self {
        let omega_rated = rated_rpm * std::f32::consts::PI / 30.0;
        let ke = (rated_voltage * 0.9) / omega_rated.max(f32::EPSILON);
        let rated_current = rated_power_w / rated_voltage.max(f32::EPSILON);
        let ra = (rated_voltage * 0.1) / rated_current.max(f32::EPSILON);
        let la = 2e-3 * ra; // tau = L/R = 2 ms
        let inertia = 0.01 * rated_power_w.max(0.0).sqrt();
        Self::new(ke, ra, la, inertia)
    }

    /// Theoretical no-load speed (rad/s) at the current `voltage`.
    #[inline]
    pub fn no_load_speed(&self) -> f32 {
        self.voltage / self.ke.max(f32::EPSILON)
    }

    /// Stall current (A) — armature current when `omega = 0`.
    #[inline]
    pub fn stall_current(&self) -> f32 {
        self.voltage / self.ra.max(f32::EPSILON)
    }

    /// Stall torque (N·m) — maximum torque at zero speed.
    #[inline]
    pub fn stall_torque(&self) -> f32 {
        self.ke * self.stall_current() // Kt == Ke
    }
}

impl Default for DcMotor {
    fn default() -> Self {
        // 1 kW, 3000 RPM, 48 V brushed PM motor
        Self::from_specs(1000.0, 3000.0, 48.0)
    }
}

// ---------------------------------------------------------------------------
// AC motor — per-phase equivalent circuit
// ---------------------------------------------------------------------------

/// Induction (AC) motor modelled with a per-phase equivalent circuit.
///
/// State: `omega` (rad/s). Derived: `slip`, `torque`.
///
/// Torque approximated from the classical slip-torque relationship:
/// ```text
/// tau = (3 * V_ph^2 * R2/s) / (omega_s * ((R1 + R2/s)^2 + (X1+X2)^2))
/// dw/dt = (tau - tau_load) / J
/// slip  = (omega_s - omega) / omega_s   clamped to [1e-3, 1.0]
/// ```
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct AcMotor {
    // --- Nameplate / parameters ---
    /// Number of pole pairs.
    pub pole_pairs: u32,
    /// Supply frequency (Hz).
    pub frequency_hz: f32,
    /// Per-phase RMS supply voltage (V).
    pub v_phase: f32,
    /// Stator resistance per phase (Ohm).
    pub r1: f32,
    /// Rotor resistance per phase referred to stator (Ohm).
    pub r2: f32,
    /// Stator leakage reactance per phase (Ohm) at rated frequency.
    pub x1: f32,
    /// Rotor leakage reactance per phase referred to stator (Ohm).
    pub x2: f32,
    /// Rotor + load moment of inertia (kg·m²).
    pub inertia: f32,
    /// Rated output power (W).
    pub rated_power_w: f32,

    // --- Input ---
    /// External load torque (N·m).
    pub load_torque: f32,

    // --- State ---
    /// Angular velocity (rad/s) — clamped to `[0, omega_s]`.
    pub omega: f32,

    // --- Derived outputs ---
    /// Electromagnetic torque (N·m).
    pub torque: f32,
    /// Per-unit slip — `(omega_s - omega) / omega_s`.
    pub slip: f32,
}

impl AcMotor {
    /// Synchronous speed in rad/s: `4*pi*f / p`.
    #[inline]
    pub fn synchronous_speed_rad(&self) -> f32 {
        4.0 * std::f32::consts::PI * self.frequency_hz / self.pole_pairs.max(1) as f32
    }

    /// Full-load slip — `(omega_s - omega) / omega_s`.
    pub fn full_load_slip(&self) -> f32 {
        let omega_s = self.synchronous_speed_rad();
        if omega_s < f32::EPSILON {
            return 1.0;
        }
        ((omega_s - self.omega) / omega_s).clamp(1e-3, 1.0)
    }
}

impl Default for AcMotor {
    fn default() -> Self {
        // 4-pole, 50 Hz, 400 V line (231 V phase), 1.5 kW induction motor
        Self {
            pole_pairs: 2,
            frequency_hz: 50.0,
            v_phase: 231.0,
            r1: 2.5,
            r2: 3.0,
            x1: 4.0,
            x2: 4.0,
            inertia: 0.05,
            rated_power_w: 1500.0,
            load_torque: 0.0,
            omega: 0.0,
            torque: 0.0,
            slip: 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Stateless update for buck converters: `v_out = D * v_in * efficiency`.
pub fn update_buck_converter_system(mut query: Query<&mut BuckConverter>) {
    for mut conv in query.iter_mut() {
        let d = conv.duty_cycle.clamp(0.0, 1.0);
        conv.v_out = d * conv.v_in * conv.efficiency;
    }
}

/// Stateless update for boost converters: `v_out = v_in / (1-D) * efficiency`.
/// Duty cycle clamped to `[0, 0.99]` to prevent division by zero.
pub fn update_boost_converter_system(mut query: Query<&mut BoostConverter>) {
    for mut conv in query.iter_mut() {
        let d = conv.duty_cycle.clamp(0.0, 0.99);
        conv.v_out = conv.v_in / (1.0 - d) * conv.efficiency;
    }
}

/// Euler integration of the DC motor back-EMF state equations.
pub fn update_dc_motor_system(time: Res<Time>, mut query: Query<&mut DcMotor>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for mut motor in query.iter_mut() {
        let ke = motor.ke.max(f32::EPSILON);
        let ra = motor.ra.max(f32::EPSILON);
        let la = motor.la.max(f32::EPSILON);
        let j = motor.inertia.max(f32::EPSILON);

        // dI/dt = (V_a - I*Ra - Ke*omega) / La
        let back_emf = ke * motor.omega;
        let di_dt = (motor.voltage - motor.current * ra - back_emf) / la;
        motor.current += di_dt * dt;

        // dw/dt = (Kt*I - tau_load) / J   (Kt == Ke)
        let em_torque = ke * motor.current;
        let dw_dt = (em_torque - motor.load_torque) / j;
        motor.omega = (motor.omega + dw_dt * dt).max(0.0);

        // Derived quantities
        motor.torque = ke * motor.current;
        motor.power_in = motor.voltage * motor.current;
        motor.power_out = motor.torque * motor.omega;
        motor.efficiency = if motor.power_in.abs() > f32::EPSILON {
            (motor.power_out / motor.power_in).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
}

/// Euler integration of the AC induction motor slip-torque dynamics.
pub fn update_ac_motor_system(time: Res<Time>, mut query: Query<&mut AcMotor>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    for mut motor in query.iter_mut() {
        let omega_s = motor.synchronous_speed_rad();
        let j = motor.inertia.max(f32::EPSILON);

        // Slip clamped to avoid singularity at s=0
        let s = ((omega_s - motor.omega) / omega_s.max(f32::EPSILON)).clamp(1e-3, 1.0);
        motor.slip = s;

        // Classical per-phase slip-torque:
        // tau = 3 * V^2 * (R2/s) / (omega_s * ((R1 + R2/s)^2 + (X1+X2)^2))
        let r2_s = motor.r2 / s;
        let denom_r = motor.r1 + r2_s;
        let denom_x = motor.x1 + motor.x2;
        let denom = denom_r * denom_r + denom_x * denom_x;
        let em_torque = if denom > f32::EPSILON && omega_s > f32::EPSILON {
            3.0 * motor.v_phase * motor.v_phase * r2_s / (omega_s * denom)
        } else {
            0.0
        };
        motor.torque = em_torque;

        // dw/dt = (tau_em - tau_load) / J
        let dw_dt = (em_torque - motor.load_torque) / j;
        motor.omega = (motor.omega + dw_dt * dt).clamp(0.0, omega_s);
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Registers electrical power components and wires update systems into
/// `Update`. Add this plugin to your `App` to enable motor and converter
/// simulation.
pub struct ElectricalPowerPlugin;

impl Plugin for ElectricalPowerPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<BuckConverter>()
            .register_type::<BoostConverter>()
            .register_type::<DcMotor>()
            .register_type::<AcMotor>()
            .add_systems(
                Update,
                (
                    update_buck_converter_system,
                    update_boost_converter_system,
                    update_dc_motor_system,
                    update_ac_motor_system,
                ),
            );
    }
}
//! Measured quantities the solver publishes after every `advance`.

use bevy_math::Vec3;

/// Per-species measurements.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpeciesStats {
    pub name: String,
    /// Live simulated particles.
    pub count: u32,
    /// Real particles those stand for.
    pub real_count: f64,
    /// Kinetic energy of the real particles represented (J).
    pub kinetic_energy: f64,
    /// Mean velocity (m/s).
    pub drift_velocity: Vec3,
    /// Kinetic temperature of the motion about the drift (K); 0 for fluids.
    pub temperature: f32,
    /// Drude conductivity n q^2 tau / m of this species (S/m).
    pub drude_conductivity: f64,
    /// Plasma angular frequency (rad/s).
    pub plasma_frequency: f64,
}

/// Whole-simulation measurements.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SimStats {
    pub particle_count: u32,
    /// Simulated time since reset (s).
    pub sim_time: f64,
    /// Substep used last frame (s).
    pub timestep: f32,
    pub substeps: u32,
    /// Share of the requested simulated time the solver kept up with,
    /// smoothed over recent frames (1 = real time, below 1 = slow motion).
    pub realtime_ratio: f32,
    /// Wall time the solver used last frame (ms).
    pub solver_ms: f64,
    /// DFSPH pressure iterations in the last substep (0 without fluids).
    pub pressure_iterations: u32,
    pub kinetic_energy: f64,
    /// Count-weighted mean kinetic temperature over point species (K).
    pub temperature: f32,
    pub max_speed: f32,
    /// Time-averaged current density (A/m^2).
    pub current_density: Vec3,
    /// Time-averaged current through the mid-plane normal to the voltage
    /// axis (A), from particle crossings.
    pub current: f64,
    /// Uniform driving field (V/m).
    pub applied_field: Vec3,
    /// Measured conductivity J.E / |E|^2 (S/m); 0 without a driving field.
    pub conductivity: f64,
    /// Analytic Drude conductivity summed over species (S/m).
    pub drude_conductivity: f64,
    /// 1 / conductivity (ohm m); 0 when undefined.
    pub resistivity: f64,
    /// Joule heating power density J.E (W/m^3).
    pub joule_power_density: f64,
    pub lattice_temperature: f32,
    /// Energy handed to the lattice by scattering since reset (J).
    pub lattice_energy: f64,
    /// Plasma angular frequency of all mobile charges (rad/s).
    pub plasma_frequency: f64,
    /// Debye length of all mobile charges (m).
    pub debye_length: f64,
    /// Mean fluid density (kg/m^3); 0 without fluid.
    pub mean_density: f32,
    /// Mean compression above rest density over fluid particles (fraction).
    pub density_error: f32,
    /// Charge removed by absorbing walls since reset (C).
    pub absorbed_charge: f64,
    pub poisson_iterations: u32,
    pub species: Vec<SpeciesStats>,
}

/// A read-only measurement: the Properties panel's Runtime rows and the
/// `psim.<Name>.<Stat>` sim values are both generated from these tables.
#[derive(Clone, Copy)]
pub struct StatSpec<T: 'static> {
    pub name: &'static str,
    pub unit: &'static str,
    pub description: &'static str,
    pub get: fn(&T) -> f64,
}

macro_rules! stat {
    ($name:literal, $unit:literal, $desc:literal, $get:expr) => {
        StatSpec { name: $name, unit: $unit, description: $desc, get: $get }
    };
}

/// Whole-simulation measurements, in panel order.
pub const SIMULATION_STATS: &[StatSpec<SimStats>] = &[
    stat!("ParticleCount", "", "Live simulated particles.", |s| s.particle_count as f64),
    stat!("SimulatedTime", "s", "Simulated time since the last reset.", |s| s.sim_time),
    stat!("Substep", "s", "Substep used last frame.", |s| s.timestep as f64),
    stat!("SubstepsPerFrame", "", "Substeps taken last frame.", |s| s.substeps as f64),
    stat!("RealtimeRatio", "",
        "Share of TimeScale the run keeps up with; below 1 it plays in slow motion (FrameBudget or MaxSubsteps reached).",
        |s| s.realtime_ratio as f64),
    stat!("SolverTime", "ms", "Wall time the solver used last frame.", |s| s.solver_ms),
    stat!("KineticEnergy", "J", "Kinetic energy of all the real particles represented.", |s| s.kinetic_energy),
    stat!("KineticTemperature", "K", "Mean kinetic temperature of the mobile point species.", |s| s.temperature as f64),
    stat!("MaxSpeed", "m/s", "Fastest particle.", |s| s.max_speed as f64),
    stat!("Current", "A", "Charge crossing the mid-plane normal to VoltageAxis per second (time-averaged).", |s| s.current),
    stat!("CurrentDensity", "A/m^2", "Magnitude of the time-averaged current density.", |s| s.current_density.length() as f64),
    stat!("Conductivity", "S/m", "Measured J.E / |E|^2 under the applied field.", |s| s.conductivity),
    stat!("DrudeConductivity", "S/m", "Analytic n q^2 tau / m summed over species, for comparison.", |s| s.drude_conductivity),
    stat!("Resistivity", "ohm m", "1 / Conductivity.", |s| s.resistivity),
    stat!("JoulePowerDensity", "W/m^3", "Power the field delivers per volume, J.E.", |s| s.joule_power_density),
    stat!("LatticeTemperatureNow", "K", "Lattice temperature now (rises with JouleHeating on).", |s| s.lattice_temperature as f64),
    stat!("LatticeEnergy", "J", "Energy scattering has handed the lattice since reset.", |s| s.lattice_energy),
    stat!("PlasmaFrequency", "rad/s", "sqrt(sum n q^2 / (eps0 m)) of the mobile charges.", |s| s.plasma_frequency),
    stat!("DebyeLength", "m", "Screening length of the mobile charges.", |s| s.debye_length),
    stat!("MeanDensity", "kg/m^3", "Mean SPH density of the fluid particles.", |s| s.mean_density as f64),
    stat!("Compression", "%", "Mean compression of the fluid above rest density.", |s| s.density_error as f64 * 100.0),
    stat!("AbsorbedCharge", "C", "Charge collected by absorbing walls since reset.", |s| s.absorbed_charge),
    stat!("PoissonIterations", "", "Conjugate-gradient iterations of the last mesh solve.", |s| s.poisson_iterations as f64),
    stat!("PressureIterations", "", "DFSPH pressure iterations in the last substep.", |s| s.pressure_iterations as f64),
];

/// Per-species measurements, in panel order.
pub const SPECIES_STATS: &[StatSpec<SpeciesStats>] = &[
    stat!("LiveCount", "", "Simulated particles of this species alive now.", |s| s.count as f64),
    stat!("RealParticles", "", "Real particles those stand for.", |s| s.real_count),
    stat!("KineticTemperature", "K", "Kinetic temperature of the motion about the drift.", |s| s.temperature as f64),
    stat!("DriftSpeed", "m/s", "Speed of the species' mean velocity.", |s| s.drift_velocity.length() as f64),
    stat!("KineticEnergy", "J", "Kinetic energy of this species.", |s| s.kinetic_energy),
    stat!("SpeciesConductivity", "S/m", "Drude conductivity n q^2 tau / m of this species.", |s| s.drude_conductivity),
    stat!("PlasmaFrequency", "rad/s", "Plasma frequency of this species.", |s| s.plasma_frequency),
];

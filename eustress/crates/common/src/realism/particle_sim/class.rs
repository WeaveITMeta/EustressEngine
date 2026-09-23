//! The `ParticleSimulation` and `ParticleSpecies` classes: their components,
//! and one field table per class that drives everything else.
//!
//! A field table row is the property's PascalCase name (Properties label,
//! script and MCP key), its snake_case TOML key, its kind, category, unit
//! and description, and whether changing it restarts the run. Reading and
//! writing the TOML section, the Properties rows, parsing panel edits and
//! the MCP tool all walk these tables, so a property exists in exactly one
//! place.
//!
//! Floats are stored as `f64`: these classes span nanometre conductors and
//! metre tanks, and an f32 round trip would print 8.47e28 back to disk as
//! 8.4700001e28.

use bevy::prelude::*;

use super::colormap::ColorMode;
use super::params::{
    Arrangement, Boundary, Electrostatics, FluidSolver, RegionShape, SimParams, SpeciesKind, SpeciesParams,
    WallPotential,
};
use super::presets::{self, ParticlePreset, ATOMIC_MASS_UNIT};
use crate::realism::constants;

/// TOML section of a `ParticleSimulation` instance.
pub const SIMULATION_SECTION: &str = "particle_simulation";
/// TOML section of a `ParticleSpecies` instance.
pub const SPECIES_SECTION: &str = "particle_species";

// ============================================================================
// Field tables
// ============================================================================

/// How a property is typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Bool,
    Int,
    Float,
    /// Three floats, shown as "x, y, z".
    Vector3,
    /// RGB, 0-255 integers on disk and in the panel (0-1 floats accepted).
    Color3,
    /// One of a fixed set of names.
    Choice(&'static [&'static str]),
    /// Free text, such as an asset path. Surrounding spaces are trimmed.
    Text,
}

/// One property of a class.
#[derive(Clone, Copy, Debug)]
pub struct FieldSpec {
    /// PascalCase name: the Properties label and the script / MCP key.
    pub name: &'static str,
    /// snake_case key inside the class's TOML section.
    pub key: &'static str,
    pub kind: FieldKind,
    pub category: &'static str,
    /// SI unit, empty when dimensionless.
    pub unit: &'static str,
    pub description: &'static str,
    /// Changing it restarts the run (it is an initial condition).
    pub restarts: bool,
}

/// A property value.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Vec3([f64; 3]),
    Color([f32; 3]),
    Choice(String),
    Text(String),
}

const BOUNDARIES: &[&str] = &["Reflect", "Periodic", "Absorb"];
const AXES: &[&str] = &["X", "Y", "Z"];
const ELECTROSTATICS: &[&str] = &["Off", "Direct", "Mesh"];
const FLUID_SOLVERS: &[&str] = &["DFSPH", "WCSPH"];
const WALL_POTENTIALS: &[&str] = &["Grounded", "Insulating"];
const COLOR_MODES: &[&str] = &["Species", "Speed", "Temperature", "Charge", "Density", "Pressure", "Field"];
const PARTICLES: &[&str] = &[
    "Electron", "Positron", "Proton", "Ion", "Argon", "Water", "Oil", "Mercury", "CustomFluid", "Custom",
];
const CONDUCTORS: &[&str] = &["Custom", "Copper", "Silver", "Gold", "Aluminum", "Iron", "Sodium"];
const ARRANGEMENTS: &[&str] = &["Random", "Lattice"];
const REGION_SHAPES: &[&str] = &["Box", "Sphere"];

macro_rules! field {
    ($name:literal, $key:literal, $kind:expr, $cat:literal, $unit:literal, $desc:literal) => {
        FieldSpec { name: $name, key: $key, kind: $kind, category: $cat, unit: $unit, description: $desc, restarts: false }
    };
    ($name:literal, $key:literal, $kind:expr, $cat:literal, $unit:literal, $desc:literal, restarts) => {
        FieldSpec { name: $name, key: $key, kind: $kind, category: $cat, unit: $unit, description: $desc, restarts: true }
    };
}

use FieldKind::*;

/// Every property of `ParticleSimulation`, in panel order.
pub const SIMULATION_FIELDS: &[FieldSpec] = &[
    field!("Enabled", "enabled", Bool, "Simulation", "", "Simulate at all. Off clears every particle."),
    field!("Running", "running", Bool, "Simulation", "",
        "Advance time while the world runs (Play, a runtime, a scripted step). In Edit the run shows its initial state. Off pauses it where it is."),
    field!("TimeScale", "time_scale", Float, "Simulation", "",
        "Simulated seconds per real second. 1 is real time; about 1e-13 makes electrons in a metal watchable."),
    field!("Timestep", "timestep", Float, "Simulation", "s",
        "Fixed substep. 0 derives it every frame from the stability limits (CFL, plasma frequency, collisions)."),
    field!("MaxSubsteps", "max_substeps", Int, "Simulation", "",
        "Most substeps per frame. When the stable step needs more, the run slows below TimeScale instead of the frame rate dropping."),
    field!("FrameBudget", "frame_budget", Float, "Simulation", "ms",
        "Wall time per frame the solver may use. A run that needs more plays in slow motion instead of lowering the frame rate; 0 removes the limit."),
    field!("MaxParticles", "max_particles", Int, "Simulation", "", "Hard cap on live particles; emission stops there.", restarts),
    field!("Seed", "seed", Int, "Simulation", "",
        "Random seed (added to the Space's global seed). The same seed and properties replay the same run.", restarts),
    field!("DomainSize", "domain_size", Vector3, "Domain", "m",
        "Physical size of the simulated box. Anything from nanometres to kilometres.", restarts),
    field!("DisplayScale", "display_scale", Float, "Domain", "",
        "World metres per simulated metre. 1 is true scale; 1e8 shows a 20 nm conductor as a 2 m box."),
    field!("BoundaryX", "boundary_x", Choice(BOUNDARIES), "Domain", "", "What particles do at the X faces."),
    field!("BoundaryY", "boundary_y", Choice(BOUNDARIES), "Domain", "", "What particles do at the Y faces."),
    field!("BoundaryZ", "boundary_z", Choice(BOUNDARIES), "Domain", "", "What particles do at the Z faces."),
    field!("Restitution", "restitution", Float, "Domain", "",
        "Normal velocity kept after hitting a wall or part (0 sticks, 1 is elastic)."),
    field!("WallFriction", "wall_friction", Float, "Domain", "", "Tangential velocity removed on contact (0 to 1)."),
    field!("CollideWithParts", "collide_with_parts", Bool, "Domain", "", "Collidable parts inside the domain are solid obstacles."),
    field!("PushParts", "push_parts", Bool, "Domain", "",
        "Unanchored parts receive the force and torque the particles exert on them."),
    field!("Gravity", "gravity", Vector3, "Forces", "m/s^2", "Uniform gravitational acceleration."),
    field!("ElectricField", "electric_field", Vector3, "Forces", "V/m", "Uniform applied electric field."),
    field!("MagneticField", "magnetic_field", Vector3, "Forces", "T", "Uniform applied magnetic field (Lorentz force, Boris push)."),
    field!("AppliedVoltage", "applied_voltage", Float, "Forces", "V",
        "Potential difference across the domain; the +V electrode is the max face. Periodic along that axis it drives the loop as an EMF."),
    field!("VoltageAxis", "voltage_axis", Choice(AXES), "Forces", "", "Axis of the applied voltage, and of the current measurement."),
    field!("Electrostatics", "electrostatics", Choice(ELECTROSTATICS), "Electrostatics", "",
        "How charges feel each other: Off (screened, the Drude picture), Direct pairwise Coulomb, or Mesh particle-in-cell."),
    field!("GridResolution", "grid_resolution", Int, "Electrostatics", "", "Mesh cells along the longest domain axis."),
    field!("Softening", "softening", Float, "Electrostatics", "", "Coulomb softening as a fraction of the particle spacing."),
    field!("NeutralizingBackground", "neutralizing_background", Bool, "Electrostatics", "",
        "A uniform charge that cancels the particles' net charge (a metal's ion lattice)."),
    field!("WallPotential", "wall_potential", Choice(WALL_POTENTIALS), "Electrostatics", "",
        "Non-electrode walls: Grounded conductors at 0 V, or Insulating (charge piles up on them, e.g. a Hall voltage)."),
    field!("FluidSolver", "fluid_solver", Choice(FLUID_SOLVERS), "Fluid", "",
        "Pressure solver. DFSPH keeps the fluid incompressible by iteration, with substeps set by the flow speed (interactive rates). WCSPH is explicit and weakly compressible, with substeps set by an artificial speed of sound (sound waves, about 1% compression)."),
    field!("KernelRadius", "kernel_radius", Float, "Fluid", "", "SPH support radius in particle spacings."),
    field!("SpeedOfSound", "speed_of_sound", Float, "Fluid", "m/s",
        "Artificial speed of sound (WCSPH), also the velocity scale of the artificial viscosity. 0 uses ten times the expected flow speed (about 1% compression)."),
    field!("ArtificialViscosity", "artificial_viscosity", Float, "Fluid", "", "Monaghan viscosity coefficient (stability; 0.01 to 0.1)."),
    field!("SurfaceTension", "surface_tension", Float, "Fluid", "N/m", "Surface tension coefficient (continuum surface force)."),
    field!("LatticeTemperature", "lattice_temperature", Float, "Thermal", "K", "Temperature scattering thermalises particles to."),
    field!("JouleHeating", "joule_heating", Bool, "Thermal", "",
        "Energy lost in collisions heats the lattice. Off holds the lattice at its temperature (an ideal heat sink)."),
    field!("LatticeHeatCapacity", "lattice_heat_capacity", Float, "Thermal", "J/(m^3 K)", "Volumetric heat capacity of the lattice."),
    field!("ColorMode", "color_mode", Choice(COLOR_MODES), "Display", "", "What the particle colours show."),
    field!("ColorRangeMin", "color_range_min", Float, "Display", "", "Low end of the colour scale. Min = Max = 0 picks the range automatically."),
    field!("ColorRangeMax", "color_range_max", Float, "Display", "", "High end of the colour scale."),
    field!("ParticleScale", "particle_scale", Float, "Display", "", "Multiplier on every particle's display radius."),
    field!("ShowDomain", "show_domain", Bool, "Display", "", "Draw the domain box outline."),
];

/// Every property of `ParticleSpecies`, in panel order.
pub const SPECIES_FIELDS: &[FieldSpec] = &[
    field!("Enabled", "enabled", Bool, "Species", "", "Include this species in the simulation.", restarts),
    field!("Particle", "particle", Choice(PARTICLES), "Species", "",
        "What one particle is. Ion and Custom take ChargeNumber and MassAmu; CustomFluid takes RestDensity and Viscosity.", restarts),
    field!("Count", "count", Int, "Species", "", "Simulated particles placed at the start.", restarts),
    field!("ChargeNumber", "charge_number", Float, "Species", "e", "Charge in elementary charges (Ion and Custom)."),
    field!("MassAmu", "mass_amu", Float, "Species", "u", "Mass in atomic mass units (Ion and Custom)."),
    field!("RestDensity", "rest_density", Float, "Species", "kg/m^3", "Fluid density at rest (CustomFluid).", restarts),
    field!("Viscosity", "viscosity", Float, "Species", "Pa s", "Dynamic viscosity (CustomFluid)."),
    field!("Conductor", "conductor", Choice(CONDUCTORS), "Species", "",
        "Free-electron metal: sets NumberDensity and CollisionTime from measured values.", restarts),
    field!("NumberDensity", "number_density", Float, "Species", "1/m^3",
        "Real particles per cubic metre; each simulated particle stands for many. 0 means one to one.", restarts),
    field!("CollisionTime", "collision_time", Float, "Species", "s",
        "Mean time between lattice collisions (the Drude relaxation time). 0 means none."),
    field!("Mobile", "mobile", Bool, "Species", "", "Moves. Fixed particles (an ion lattice) still act as charges.", restarts),
    field!("Temperature", "temperature", Float, "Initial State", "K", "Initial Maxwell-Boltzmann temperature.", restarts),
    field!("DriftVelocity", "drift_velocity", Vector3, "Initial State", "m/s", "Initial bulk velocity (and of emitted particles).", restarts),
    field!("Arrangement", "arrangement", Choice(ARRANGEMENTS), "Initial State", "", "Random placement or a cubic lattice.", restarts),
    field!("RegionShape", "region_shape", Choice(REGION_SHAPES), "Initial State", "", "Shape of the placement region.", restarts),
    field!("RegionMin", "region_min", Vector3, "Initial State", "",
        "Region corner in domain fractions (0 to 1 per axis).", restarts),
    field!("RegionMax", "region_max", Vector3, "Initial State", "", "Opposite region corner in domain fractions.", restarts),
    field!("EmissionRate", "emission_rate", Float, "Initial State", "1/s",
        "New particles per simulated second, emitted into the region at DriftVelocity."),
    field!("Color", "color", Color3, "Display", "", "Colour in Species colour mode."),
    field!("DisplayRadius", "display_radius", Float, "Display", "m", "Radius drawn in world metres. 0 derives it from the spacing."),
];

pub fn field(specs: &'static [FieldSpec], name: &str) -> Option<&'static FieldSpec> {
    specs.iter().find(|f| f.name.eq_ignore_ascii_case(name) || f.key.eq_ignore_ascii_case(name))
}

// ============================================================================
// Components
// ============================================================================

/// A particle simulation domain (class `ParticleSimulation`). Its
/// `ParticleSpecies` children are the particle populations.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct ParticleSimulation {
    pub enabled: bool,
    pub running: bool,
    pub time_scale: f64,
    pub timestep: f64,
    pub max_substeps: i64,
    /// Solver wall time allowed per frame (ms); 0 is unlimited.
    pub frame_budget: f64,
    pub max_particles: i64,
    pub seed: i64,
    pub domain_size: [f64; 3],
    pub display_scale: f64,
    pub boundary: [Boundary; 3],
    pub restitution: f64,
    pub wall_friction: f64,
    pub collide_with_parts: bool,
    pub push_parts: bool,
    pub gravity: [f64; 3],
    pub electric_field: [f64; 3],
    pub magnetic_field: [f64; 3],
    pub applied_voltage: f64,
    pub voltage_axis: usize,
    pub electrostatics: Electrostatics,
    pub grid_resolution: i64,
    pub softening: f64,
    pub neutralizing_background: bool,
    pub wall_potential: WallPotential,
    pub fluid_solver: FluidSolver,
    pub kernel_radius: f64,
    pub speed_of_sound: f64,
    pub artificial_viscosity: f64,
    pub surface_tension: f64,
    pub lattice_temperature: f64,
    pub joule_heating: bool,
    pub lattice_heat_capacity: f64,
    pub color_mode: ColorMode,
    pub color_range_min: f64,
    pub color_range_max: f64,
    pub particle_scale: f64,
    pub show_domain: bool,
}

/// An f32 constant as the f64 a person would type (998.2, not
/// 998.2000122070312), so defaults read cleanly in the TOML.
fn clean(x: f32) -> f64 {
    format!("{x}").parse().unwrap_or(x as f64)
}

impl Default for ParticleSimulation {
    /// A one-metre tank at true scale, with the solver's defaults.
    fn default() -> Self {
        Self {
            enabled: true,
            running: true,
            time_scale: 1.0,
            timestep: 0.0,
            max_substeps: 64,
            frame_budget: 6.0,
            max_particles: 200_000,
            seed: 1,
            domain_size: [1.0, 1.0, 1.0],
            display_scale: 1.0,
            boundary: [Boundary::Reflect; 3],
            restitution: 0.3,
            wall_friction: 0.05,
            collide_with_parts: true,
            push_parts: true,
            gravity: [0.0, -9.80665, 0.0],
            electric_field: [0.0; 3],
            magnetic_field: [0.0; 3],
            applied_voltage: 0.0,
            voltage_axis: 0,
            electrostatics: Electrostatics::Off,
            grid_resolution: 32,
            softening: 0.3,
            neutralizing_background: true,
            wall_potential: WallPotential::Grounded,
            fluid_solver: FluidSolver::Dfsph,
            kernel_radius: 2.4,
            speed_of_sound: 0.0,
            artificial_viscosity: 0.05,
            surface_tension: 0.0,
            lattice_temperature: 293.15,
            joule_heating: false,
            lattice_heat_capacity: 3.45e6,
            color_mode: ColorMode::Species,
            color_range_min: 0.0,
            color_range_max: 0.0,
            particle_scale: 1.0,
            show_domain: true,
        }
    }
}

/// One particle population inside a `ParticleSimulation`.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct ParticleSpecies {
    pub enabled: bool,
    pub particle: ParticlePreset,
    pub count: i64,
    pub charge_number: f64,
    pub mass_amu: f64,
    pub rest_density: f64,
    pub viscosity: f64,
    /// Index into [`presets::CONDUCTORS`], or `None` for Custom.
    pub conductor: Option<usize>,
    pub number_density: f64,
    pub collision_time: f64,
    pub mobile: bool,
    pub temperature: f64,
    pub drift_velocity: [f64; 3],
    pub arrangement: Arrangement,
    pub region_shape: RegionShape,
    pub region_min: [f64; 3],
    pub region_max: [f64; 3],
    pub emission_rate: f64,
    pub color: [f32; 3],
    pub display_radius: f64,
}

impl Default for ParticleSpecies {
    /// A block of water filling the bottom 40% of the domain.
    fn default() -> Self {
        let (rho, mu) = ParticlePreset::Water.fluid_properties().unwrap_or((998.2, 1.0e-3));
        let c = ParticlePreset::Water.color();
        Self {
            enabled: true,
            particle: ParticlePreset::Water,
            count: 2000,
            charge_number: 1.0,
            mass_amu: 1.0,
            rest_density: clean(rho),
            viscosity: clean(mu),
            conductor: None,
            number_density: 0.0,
            collision_time: 0.0,
            mobile: true,
            temperature: 293.15,
            drift_velocity: [0.0; 3],
            arrangement: Arrangement::Lattice,
            region_shape: RegionShape::Box,
            region_min: [0.0, 0.0, 0.0],
            region_max: [1.0, 0.4, 1.0],
            emission_rate: 0.0,
            color: [c[0], c[1], c[2]],
            display_radius: 0.0,
        }
    }
}

// ============================================================================
// Choice <-> enum
// ============================================================================

fn boundary_name(b: Boundary) -> &'static str {
    match b {
        Boundary::Reflect => "Reflect",
        Boundary::Periodic => "Periodic",
        Boundary::Absorb => "Absorb",
    }
}

fn parse_boundary(s: &str) -> Option<Boundary> {
    match s.to_ascii_lowercase().as_str() {
        "reflect" => Some(Boundary::Reflect),
        "periodic" => Some(Boundary::Periodic),
        "absorb" => Some(Boundary::Absorb),
        _ => None,
    }
}

fn electrostatics_name(e: Electrostatics) -> &'static str {
    match e {
        Electrostatics::Off => "Off",
        Electrostatics::Direct => "Direct",
        Electrostatics::Mesh => "Mesh",
    }
}

fn parse_electrostatics(s: &str) -> Option<Electrostatics> {
    match s.to_ascii_lowercase().as_str() {
        "off" => Some(Electrostatics::Off),
        "direct" => Some(Electrostatics::Direct),
        "mesh" => Some(Electrostatics::Mesh),
        _ => None,
    }
}

fn fluid_solver_name(s: FluidSolver) -> &'static str {
    match s {
        FluidSolver::Dfsph => "DFSPH",
        FluidSolver::Wcsph => "WCSPH",
    }
}

fn parse_fluid_solver(s: &str) -> Option<FluidSolver> {
    match s.to_ascii_lowercase().as_str() {
        "dfsph" => Some(FluidSolver::Dfsph),
        "wcsph" => Some(FluidSolver::Wcsph),
        _ => None,
    }
}

fn wall_potential_name(w: WallPotential) -> &'static str {
    match w {
        WallPotential::Grounded => "Grounded",
        WallPotential::Insulating => "Insulating",
    }
}

fn parse_wall_potential(s: &str) -> Option<WallPotential> {
    match s.to_ascii_lowercase().as_str() {
        "grounded" => Some(WallPotential::Grounded),
        "insulating" => Some(WallPotential::Insulating),
        _ => None,
    }
}

fn arrangement_name(a: Arrangement) -> &'static str {
    match a {
        Arrangement::Random => "Random",
        Arrangement::Lattice => "Lattice",
    }
}

fn region_shape_name(r: RegionShape) -> &'static str {
    match r {
        RegionShape::Box => "Box",
        RegionShape::Sphere => "Sphere",
    }
}

fn conductor_name(c: Option<usize>) -> &'static str {
    c.and_then(|i| presets::CONDUCTORS.get(i)).map(|c| c.name).unwrap_or("Custom")
}

// ============================================================================
// Get / set
// ============================================================================

fn as_bool(v: &FieldValue) -> Result<bool, String> {
    match v {
        FieldValue::Bool(b) => Ok(*b),
        FieldValue::Int(i) => Ok(*i != 0),
        other => Err(format!("expected a boolean, got {other:?}")),
    }
}

fn as_f64(v: &FieldValue) -> Result<f64, String> {
    let f = match v {
        FieldValue::Float(f) => *f,
        FieldValue::Int(i) => *i as f64,
        other => return Err(format!("expected a number, got {other:?}")),
    };
    if f.is_finite() { Ok(f) } else { Err("value is not finite".into()) }
}

fn as_i64(v: &FieldValue) -> Result<i64, String> {
    match v {
        FieldValue::Int(i) => Ok(*i),
        FieldValue::Float(f) if f.is_finite() && f.fract() == 0.0 => Ok(*f as i64),
        other => Err(format!("expected an integer, got {other:?}")),
    }
}

fn as_vec3(v: &FieldValue) -> Result<[f64; 3], String> {
    match v {
        FieldValue::Vec3(a) if a.iter().all(|x| x.is_finite()) => Ok(*a),
        other => Err(format!("expected three numbers, got {other:?}")),
    }
}

fn as_choice<'a>(v: &'a FieldValue) -> Result<&'a str, String> {
    match v {
        FieldValue::Choice(s) => Ok(s),
        other => Err(format!("expected a name, got {other:?}")),
    }
}

impl ParticleSimulation {
    pub fn get(&self, name: &str) -> Option<FieldValue> {
        let f = field(SIMULATION_FIELDS, name)?;
        Some(match f.key {
            "enabled" => FieldValue::Bool(self.enabled),
            "running" => FieldValue::Bool(self.running),
            "time_scale" => FieldValue::Float(self.time_scale),
            "timestep" => FieldValue::Float(self.timestep),
            "max_substeps" => FieldValue::Int(self.max_substeps),
            "frame_budget" => FieldValue::Float(self.frame_budget),
            "max_particles" => FieldValue::Int(self.max_particles),
            "seed" => FieldValue::Int(self.seed),
            "domain_size" => FieldValue::Vec3(self.domain_size),
            "display_scale" => FieldValue::Float(self.display_scale),
            "boundary_x" => FieldValue::Choice(boundary_name(self.boundary[0]).into()),
            "boundary_y" => FieldValue::Choice(boundary_name(self.boundary[1]).into()),
            "boundary_z" => FieldValue::Choice(boundary_name(self.boundary[2]).into()),
            "restitution" => FieldValue::Float(self.restitution),
            "wall_friction" => FieldValue::Float(self.wall_friction),
            "collide_with_parts" => FieldValue::Bool(self.collide_with_parts),
            "push_parts" => FieldValue::Bool(self.push_parts),
            "gravity" => FieldValue::Vec3(self.gravity),
            "electric_field" => FieldValue::Vec3(self.electric_field),
            "magnetic_field" => FieldValue::Vec3(self.magnetic_field),
            "applied_voltage" => FieldValue::Float(self.applied_voltage),
            "voltage_axis" => FieldValue::Choice(AXES[self.voltage_axis.min(2)].into()),
            "electrostatics" => FieldValue::Choice(electrostatics_name(self.electrostatics).into()),
            "grid_resolution" => FieldValue::Int(self.grid_resolution),
            "softening" => FieldValue::Float(self.softening),
            "neutralizing_background" => FieldValue::Bool(self.neutralizing_background),
            "wall_potential" => FieldValue::Choice(wall_potential_name(self.wall_potential).into()),
            "fluid_solver" => FieldValue::Choice(fluid_solver_name(self.fluid_solver).into()),
            "kernel_radius" => FieldValue::Float(self.kernel_radius),
            "speed_of_sound" => FieldValue::Float(self.speed_of_sound),
            "artificial_viscosity" => FieldValue::Float(self.artificial_viscosity),
            "surface_tension" => FieldValue::Float(self.surface_tension),
            "lattice_temperature" => FieldValue::Float(self.lattice_temperature),
            "joule_heating" => FieldValue::Bool(self.joule_heating),
            "lattice_heat_capacity" => FieldValue::Float(self.lattice_heat_capacity),
            "color_mode" => FieldValue::Choice(self.color_mode.as_str().into()),
            "color_range_min" => FieldValue::Float(self.color_range_min),
            "color_range_max" => FieldValue::Float(self.color_range_max),
            "particle_scale" => FieldValue::Float(self.particle_scale),
            "show_domain" => FieldValue::Bool(self.show_domain),
            _ => return None,
        })
    }

    /// Set one property, validating and clamping it to a usable range.
    pub fn set(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        let f = field(SIMULATION_FIELDS, name).ok_or_else(|| format!("ParticleSimulation has no property {name}"))?;
        let positive = |x: f64, what: &str| if x > 0.0 { Ok(x) } else { Err(format!("{what} must be positive")) };
        match f.key {
            "enabled" => self.enabled = as_bool(&v)?,
            "running" => self.running = as_bool(&v)?,
            "time_scale" => self.time_scale = positive(as_f64(&v)?, "TimeScale")?,
            "timestep" => self.timestep = as_f64(&v)?.max(0.0),
            "max_substeps" => self.max_substeps = as_i64(&v)?.clamp(1, 1_000_000),
            "frame_budget" => self.frame_budget = as_f64(&v)?.clamp(0.0, 1000.0),
            "max_particles" => self.max_particles = as_i64(&v)?.clamp(0, 5_000_000),
            "seed" => self.seed = as_i64(&v)?,
            "domain_size" => {
                let d = as_vec3(&v)?;
                if d.iter().any(|x| *x <= 0.0) {
                    return Err("DomainSize must be positive on every axis".into());
                }
                self.domain_size = d;
            }
            "display_scale" => self.display_scale = positive(as_f64(&v)?, "DisplayScale")?,
            "boundary_x" | "boundary_y" | "boundary_z" => {
                let axis = match f.key { "boundary_x" => 0, "boundary_y" => 1, _ => 2 };
                let s = as_choice(&v)?;
                self.boundary[axis] = parse_boundary(s).ok_or_else(|| format!("unknown boundary {s}"))?;
            }
            "restitution" => self.restitution = as_f64(&v)?.clamp(0.0, 1.0),
            "wall_friction" => self.wall_friction = as_f64(&v)?.clamp(0.0, 1.0),
            "collide_with_parts" => self.collide_with_parts = as_bool(&v)?,
            "push_parts" => self.push_parts = as_bool(&v)?,
            "gravity" => self.gravity = as_vec3(&v)?,
            "electric_field" => self.electric_field = as_vec3(&v)?,
            "magnetic_field" => self.magnetic_field = as_vec3(&v)?,
            "applied_voltage" => self.applied_voltage = as_f64(&v)?,
            "voltage_axis" => {
                let s = as_choice(&v)?;
                self.voltage_axis = AXES
                    .iter()
                    .position(|a| a.eq_ignore_ascii_case(s))
                    .ok_or_else(|| format!("unknown axis {s}"))?;
            }
            "electrostatics" => {
                let s = as_choice(&v)?;
                self.electrostatics = parse_electrostatics(s).ok_or_else(|| format!("unknown electrostatics {s}"))?;
            }
            "grid_resolution" => self.grid_resolution = as_i64(&v)?.clamp(4, 256),
            "softening" => self.softening = as_f64(&v)?.max(0.0),
            "neutralizing_background" => self.neutralizing_background = as_bool(&v)?,
            "wall_potential" => {
                let s = as_choice(&v)?;
                self.wall_potential = parse_wall_potential(s).ok_or_else(|| format!("unknown wall potential {s}"))?;
            }
            "fluid_solver" => {
                let s = as_choice(&v)?;
                self.fluid_solver = parse_fluid_solver(s).ok_or_else(|| format!("unknown fluid solver {s}"))?;
            }
            "kernel_radius" => self.kernel_radius = as_f64(&v)?.clamp(1.2, 4.0),
            "speed_of_sound" => self.speed_of_sound = as_f64(&v)?.max(0.0),
            "artificial_viscosity" => self.artificial_viscosity = as_f64(&v)?.max(0.0),
            "surface_tension" => self.surface_tension = as_f64(&v)?.max(0.0),
            "lattice_temperature" => self.lattice_temperature = as_f64(&v)?.max(0.0),
            "joule_heating" => self.joule_heating = as_bool(&v)?,
            "lattice_heat_capacity" => self.lattice_heat_capacity = positive(as_f64(&v)?, "LatticeHeatCapacity")?,
            "color_mode" => {
                let s = as_choice(&v)?;
                self.color_mode = ColorMode::parse(s).ok_or_else(|| format!("unknown colour mode {s}"))?;
            }
            "color_range_min" => self.color_range_min = as_f64(&v)?,
            "color_range_max" => self.color_range_max = as_f64(&v)?,
            "particle_scale" => self.particle_scale = positive(as_f64(&v)?, "ParticleScale")?,
            "show_domain" => self.show_domain = as_bool(&v)?,
            other => return Err(format!("unhandled field {other}")),
        }
        Ok(())
    }

    /// Solver parameters for this domain. `global_seed` is the Space's
    /// `GlobalRngSeed`, so a run is reproducible from the Space alone.
    pub fn sim_params(&self, global_seed: u64) -> SimParams {
        let v = |a: [f64; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
        SimParams {
            domain_size: v(self.domain_size).max(Vec3::splat(f32::MIN_POSITIVE)),
            boundary: self.boundary,
            restitution: self.restitution as f32,
            wall_friction: self.wall_friction as f32,
            gravity: v(self.gravity),
            electric_field: v(self.electric_field),
            magnetic_field: v(self.magnetic_field),
            applied_voltage: self.applied_voltage as f32,
            voltage_axis: self.voltage_axis.min(2),
            electrostatics: self.electrostatics,
            grid_resolution: self.grid_resolution.clamp(4, 256) as u32,
            softening: self.softening as f32,
            neutralizing_background: self.neutralizing_background,
            wall_potential: self.wall_potential,
            kernel_radius_ratio: self.kernel_radius as f32,
            fluid_solver: self.fluid_solver,
            speed_of_sound: self.speed_of_sound as f32,
            artificial_viscosity: self.artificial_viscosity as f32,
            surface_tension: self.surface_tension as f32,
            lattice_temperature: self.lattice_temperature as f32,
            joule_heating: self.joule_heating,
            lattice_heat_capacity: self.lattice_heat_capacity as f32,
            timestep: self.timestep as f32,
            max_substeps: self.max_substeps.clamp(1, 1_000_000) as u32,
            max_particles: self.max_particles.clamp(0, 5_000_000) as u32,
            seed: global_seed.wrapping_add(self.seed as u64),
        }
    }

    /// Colour range override, or `None` for automatic.
    pub fn color_range(&self) -> Option<(f32, f32)> {
        (self.color_range_max > self.color_range_min)
            .then(|| (self.color_range_min as f32, self.color_range_max as f32))
    }
}

impl ParticleSpecies {
    pub fn get(&self, name: &str) -> Option<FieldValue> {
        let f = field(SPECIES_FIELDS, name)?;
        Some(match f.key {
            "enabled" => FieldValue::Bool(self.enabled),
            "particle" => FieldValue::Choice(self.particle.as_str().into()),
            "count" => FieldValue::Int(self.count),
            "charge_number" => FieldValue::Float(self.charge_number),
            "mass_amu" => FieldValue::Float(self.mass_amu),
            "rest_density" => FieldValue::Float(self.rest_density),
            "viscosity" => FieldValue::Float(self.viscosity),
            "conductor" => FieldValue::Choice(conductor_name(self.conductor).into()),
            "number_density" => FieldValue::Float(self.number_density),
            "collision_time" => FieldValue::Float(self.collision_time),
            "mobile" => FieldValue::Bool(self.mobile),
            "temperature" => FieldValue::Float(self.temperature),
            "drift_velocity" => FieldValue::Vec3(self.drift_velocity),
            "arrangement" => FieldValue::Choice(arrangement_name(self.arrangement).into()),
            "region_shape" => FieldValue::Choice(region_shape_name(self.region_shape).into()),
            "region_min" => FieldValue::Vec3(self.region_min),
            "region_max" => FieldValue::Vec3(self.region_max),
            "emission_rate" => FieldValue::Float(self.emission_rate),
            "color" => FieldValue::Color(self.color),
            "display_radius" => FieldValue::Float(self.display_radius),
            _ => return None,
        })
    }

    /// Set one property. Choosing a `Particle` also takes its colour, and
    /// choosing a `Conductor` fills in its electron density and relaxation
    /// time; editing either of those by hand marks the conductor Custom.
    pub fn set(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        let f = field(SPECIES_FIELDS, name).ok_or_else(|| format!("ParticleSpecies has no property {name}"))?;
        let unit_range = |a: [f64; 3]| a.map(|x| x.clamp(0.0, 1.0));
        match f.key {
            "enabled" => self.enabled = as_bool(&v)?,
            "particle" => {
                let s = as_choice(&v)?;
                let p = ParticlePreset::parse(s).ok_or_else(|| format!("unknown particle {s}"))?;
                if p != self.particle {
                    let c = p.color();
                    self.color = [c[0], c[1], c[2]];
                    if let Some((rho, mu)) = p.fluid_properties() {
                        self.rest_density = clean(rho);
                        self.viscosity = clean(mu);
                    }
                }
                self.particle = p;
            }
            "count" => self.count = as_i64(&v)?.clamp(0, 5_000_000),
            "charge_number" => self.charge_number = as_f64(&v)?,
            "mass_amu" => {
                let m = as_f64(&v)?;
                if m <= 0.0 {
                    return Err("MassAmu must be positive".into());
                }
                self.mass_amu = m;
            }
            "rest_density" => {
                let r = as_f64(&v)?;
                if r <= 0.0 {
                    return Err("RestDensity must be positive".into());
                }
                self.rest_density = r;
            }
            "viscosity" => self.viscosity = as_f64(&v)?.max(0.0),
            "conductor" => {
                let s = as_choice(&v)?;
                if s.eq_ignore_ascii_case("custom") {
                    self.conductor = None;
                } else {
                    let i = presets::CONDUCTORS
                        .iter()
                        .position(|c| c.name.eq_ignore_ascii_case(s))
                        .ok_or_else(|| format!("unknown conductor {s}"))?;
                    let c = presets::CONDUCTORS[i];
                    self.conductor = Some(i);
                    self.number_density = c.electron_density;
                    self.collision_time = c.relaxation_time;
                }
            }
            // A value that no longer matches the chosen metal makes the
            // conductor Custom; re-stating the metal's own value (as loading
            // a saved file does) keeps it.
            "number_density" => {
                self.number_density = as_f64(&v)?.max(0.0);
                if self.conductor_value(|c| c.electron_density) != Some(self.number_density) {
                    self.conductor = None;
                }
            }
            "collision_time" => {
                self.collision_time = as_f64(&v)?.max(0.0);
                if self.conductor_value(|c| c.relaxation_time) != Some(self.collision_time) {
                    self.conductor = None;
                }
            }
            "mobile" => self.mobile = as_bool(&v)?,
            "temperature" => self.temperature = as_f64(&v)?.max(0.0),
            "drift_velocity" => self.drift_velocity = as_vec3(&v)?,
            "arrangement" => {
                let s = as_choice(&v)?;
                self.arrangement = match s.to_ascii_lowercase().as_str() {
                    "random" => Arrangement::Random,
                    "lattice" => Arrangement::Lattice,
                    _ => return Err(format!("unknown arrangement {s}")),
                };
            }
            "region_shape" => {
                let s = as_choice(&v)?;
                self.region_shape = match s.to_ascii_lowercase().as_str() {
                    "box" => RegionShape::Box,
                    "sphere" => RegionShape::Sphere,
                    _ => return Err(format!("unknown region shape {s}")),
                };
            }
            "region_min" => self.region_min = unit_range(as_vec3(&v)?),
            "region_max" => self.region_max = unit_range(as_vec3(&v)?),
            "emission_rate" => self.emission_rate = as_f64(&v)?.max(0.0),
            "color" => match v {
                FieldValue::Color(c) => self.color = c.map(|x| x.clamp(0.0, 1.0)),
                other => return Err(format!("expected a colour, got {other:?}")),
            },
            "display_radius" => self.display_radius = as_f64(&v)?.max(0.0),
            other => return Err(format!("unhandled field {other}")),
        }
        Ok(())
    }

    fn conductor_value(&self, pick: impl Fn(&presets::ConductorMaterial) -> f64) -> Option<f64> {
        self.conductor.and_then(|i| presets::CONDUCTORS.get(i)).map(pick)
    }

    /// What one real particle is: (kind, charge C, mass kg).
    pub fn resolved(&self) -> (SpeciesKind, f64, f64) {
        let e = constants::ELEMENTARY_CHARGE;
        match self.particle {
            p if p.is_fluid() => (SpeciesKind::Fluid, 0.0, 0.0),
            ParticlePreset::Ion | ParticlePreset::Custom => {
                (SpeciesKind::Point, self.charge_number * e, self.mass_amu.max(1e-12) * ATOMIC_MASS_UNIT)
            }
            p => {
                let (q, m) = p.charge_mass().unwrap_or((0.0, ATOMIC_MASS_UNIT));
                (SpeciesKind::Point, q, m)
            }
        }
    }

    /// Solver parameters for this population.
    pub fn species_params(&self, name: &str) -> SpeciesParams {
        let (kind, charge, mass) = self.resolved();
        let v = |a: [f64; 3]| Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
        let (rest_density, viscosity) = match self.particle {
            ParticlePreset::CustomFluid => (self.rest_density as f32, self.viscosity as f32),
            p => p.fluid_properties().unwrap_or((self.rest_density as f32, self.viscosity as f32)),
        };
        SpeciesParams {
            name: name.to_string(),
            kind,
            charge,
            mass: if mass > 0.0 { mass } else { constants::PROTON_MASS },
            count: self.count.clamp(0, 5_000_000) as u32,
            number_density: self.number_density,
            rest_density,
            viscosity,
            temperature: self.temperature as f32,
            drift_velocity: v(self.drift_velocity),
            region_shape: self.region_shape,
            region_min: v(self.region_min),
            region_max: v(self.region_max),
            arrangement: self.arrangement,
            mobile: self.mobile,
            collision_time: self.collision_time as f32,
            emission_rate: self.emission_rate as f32,
            // Authored in sRGB like every Eustress colour; rendered linear.
            color: {
                let l = Color::srgb(self.color[0], self.color[1], self.color[2]).to_linear();
                [l.red, l.green, l.blue, 1.0]
            },
            display_radius: self.display_radius as f32,
            enabled: self.enabled,
        }
    }
}

// ============================================================================
// Text: panel display, panel edits, TOML
// ============================================================================

/// Seven significant digits: plain between 1e-3 and 1e6, scientific
/// outside, trailing zeros trimmed (8.47e28, 2.7e-14, 9.80665, 0.5).
pub fn format_number(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return if x == 0.0 { "0".into() } else { x.to_string() };
    }
    let a = x.abs();
    let trim = |s: &str| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() }
    };
    if (1e-3..1e6).contains(&a) {
        let decimals = (6 - a.log10().floor() as i32).clamp(0, 12) as usize;
        trim(&format!("{x:.decimals$}"))
    } else {
        let s = format!("{x:.6e}");
        match s.split_once('e') {
            Some((m, e)) => format!("{}e{e}", trim(m)),
            None => s,
        }
    }
}

/// Display text for a value, as the Properties panel shows it.
pub fn format_value(v: &FieldValue) -> String {
    match v {
        FieldValue::Bool(b) => b.to_string(),
        FieldValue::Int(i) => i.to_string(),
        FieldValue::Float(f) => format_number(*f),
        FieldValue::Vec3(a) => format!("{}, {}, {}", format_number(a[0]), format_number(a[1]), format_number(a[2])),
        FieldValue::Color(c) => {
            let b = c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
            format!("{}, {}, {}", b[0], b[1], b[2])
        }
        FieldValue::Choice(s) | FieldValue::Text(s) => s.clone(),
    }
}

/// Parse panel / script / MCP text into a value of the field's kind.
pub fn parse_value(kind: FieldKind, text: &str) -> Result<FieldValue, String> {
    let t = text.trim();
    let num = |s: &str| -> Result<f64, String> {
        s.trim().parse::<f64>().map_err(|_| format!("'{}' is not a number", s.trim())).and_then(|v| {
            if v.is_finite() { Ok(v) } else { Err("value is not finite".into()) }
        })
    };
    match kind {
        FieldKind::Bool => match t.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Ok(FieldValue::Bool(true)),
            "false" | "0" | "no" | "off" => Ok(FieldValue::Bool(false)),
            _ => Err(format!("'{t}' is not true or false")),
        },
        FieldKind::Int => {
            let v = num(t)?;
            if v.fract() != 0.0 {
                return Err(format!("'{t}' is not a whole number"));
            }
            Ok(FieldValue::Int(v as i64))
        }
        FieldKind::Float => Ok(FieldValue::Float(num(t)?)),
        FieldKind::Vector3 => {
            let parts: Vec<&str> = t.trim_matches(|c| c == '[' || c == ']' || c == '(' || c == ')').split(',').collect();
            if parts.len() != 3 {
                return Err(format!("'{t}' is not three numbers"));
            }
            Ok(FieldValue::Vec3([num(parts[0])?, num(parts[1])?, num(parts[2])?]))
        }
        FieldKind::Color3 => {
            let t = t.trim_start_matches('#');
            if t.len() == 6 && t.chars().all(|c| c.is_ascii_hexdigit()) {
                let b = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
                return Ok(FieldValue::Color([b(0), b(2), b(4)]));
            }
            let parts: Vec<f64> = t
                .trim_matches(|c| c == '[' || c == ']' || c == '(' || c == ')')
                .split(',')
                .map(num)
                .collect::<Result<_, _>>()?;
            if parts.len() != 3 {
                return Err(format!("'{t}' is not an r, g, b colour"));
            }
            // 0-255 integers, or 0-1 floats when every channel is <= 1.
            let scale = if parts.iter().any(|&c| c > 1.0) { 255.0 } else { 1.0 };
            Ok(FieldValue::Color([
                (parts[0] / scale) as f32,
                (parts[1] / scale) as f32,
                (parts[2] / scale) as f32,
            ]))
        }
        FieldKind::Choice(options) => options
            .iter()
            .find(|o| o.eq_ignore_ascii_case(t))
            .map(|o| FieldValue::Choice((*o).to_string()))
            .ok_or_else(|| format!("'{t}' is not one of {}", options.join(", "))),
        FieldKind::Text => Ok(FieldValue::Text(t.to_string())),
    }
}

/// A value as TOML.
pub fn value_to_toml(v: &FieldValue) -> toml::Value {
    match v {
        FieldValue::Bool(b) => toml::Value::Boolean(*b),
        FieldValue::Int(i) => toml::Value::Integer(*i),
        FieldValue::Float(f) => toml::Value::Float(*f),
        FieldValue::Vec3(a) => toml::Value::Array(a.iter().map(|x| toml::Value::Float(*x)).collect()),
        // Eustress convention: colours on disk are 0-255 integers.
        FieldValue::Color(c) => toml::Value::Array(
            c.iter().map(|x| toml::Value::Integer((x.clamp(0.0, 1.0) * 255.0).round() as i64)).collect(),
        ),
        FieldValue::Choice(s) | FieldValue::Text(s) => toml::Value::String(s.clone()),
    }
}

/// A TOML value read as the field's kind (ints and floats interchangeable).
pub fn value_from_toml(kind: FieldKind, v: &toml::Value) -> Option<FieldValue> {
    let num = |v: &toml::Value| v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
    match kind {
        FieldKind::Bool => v.as_bool().map(FieldValue::Bool),
        FieldKind::Int => v.as_integer().or_else(|| v.as_float().map(|f| f as i64)).map(FieldValue::Int),
        FieldKind::Float => num(v).map(FieldValue::Float),
        FieldKind::Vector3 => {
            let a = v.as_array()?;
            if a.len() != 3 {
                return None;
            }
            Some(FieldValue::Vec3([num(&a[0])?, num(&a[1])?, num(&a[2])?]))
        }
        FieldKind::Color3 => {
            let a = v.as_array()?;
            if a.len() < 3 {
                return None;
            }
            // Integer arrays are 0-255, float arrays 0-1 (Eustress convention).
            let ints = a.iter().take(3).all(|c| c.is_integer());
            let ch = |c: &toml::Value| -> Option<f32> {
                let f = num(c)? as f32;
                Some(if ints { f / 255.0 } else { f })
            };
            Some(FieldValue::Color([ch(&a[0])?, ch(&a[1])?, ch(&a[2])?]))
        }
        FieldKind::Choice(_) => v.as_str().and_then(|s| parse_value(kind, s).ok()),
        FieldKind::Text => v.as_str().map(|s| FieldValue::Text(s.trim().to_string())),
    }
}

/// Anything with a field table.
pub trait FieldTable {
    const FIELDS: &'static [FieldSpec];
    const SECTION: &'static str;
    fn get_field(&self, name: &str) -> Option<FieldValue>;
    fn set_field(&mut self, name: &str, v: FieldValue) -> Result<(), String>;

    /// Parse text for `name` and set it.
    fn set_text(&mut self, name: &str, text: &str) -> Result<&'static FieldSpec, String> {
        let spec = field(Self::FIELDS, name).ok_or_else(|| format!("no property {name}"))?;
        let v = parse_value(spec.kind, text)?;
        self.set_field(spec.name, v)?;
        Ok(spec)
    }

    /// Display text of `name`.
    fn text(&self, name: &str) -> Option<String> {
        self.get_field(name).map(|v| format_value(&v))
    }

    /// The whole section as a TOML table, in field order.
    fn to_toml_table(&self) -> toml::value::Table {
        let mut t = toml::value::Table::new();
        for f in Self::FIELDS {
            if let Some(v) = self.get_field(f.name) {
                t.insert(f.key.to_string(), value_to_toml(&v));
            }
        }
        t
    }

    /// Apply every recognised key of a TOML section. Unknown keys are left
    /// alone (a newer file stays loadable); bad values keep the default and
    /// are reported.
    fn apply_toml_table(&mut self, table: &toml::value::Table) -> Vec<String> {
        let mut problems = Vec::new();
        for f in Self::FIELDS {
            let Some(raw) = table.get(f.key) else { continue };
            match value_from_toml(f.kind, raw) {
                Some(v) => {
                    if let Err(e) = self.set_field(f.name, v) {
                        problems.push(format!("{}: {e}", f.key));
                    }
                }
                None => problems.push(format!("{}: unreadable value {raw}", f.key)),
            }
        }
        problems
    }
}

impl FieldTable for ParticleSimulation {
    const FIELDS: &'static [FieldSpec] = SIMULATION_FIELDS;
    const SECTION: &'static str = SIMULATION_SECTION;
    fn get_field(&self, name: &str) -> Option<FieldValue> {
        self.get(name)
    }
    fn set_field(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        self.set(name, v)
    }
}

impl FieldTable for ParticleSpecies {
    const FIELDS: &'static [FieldSpec] = SPECIES_FIELDS;
    const SECTION: &'static str = SPECIES_SECTION;
    fn get_field(&self, name: &str) -> Option<FieldValue> {
        self.get(name)
    }
    fn set_field(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        self.set(name, v)
    }
}

/// Read a class component from its instance TOML (the whole document or
/// the flattened `extra` map both work: pass the section table).
pub fn from_section<T: FieldTable + Default>(section: Option<&toml::value::Table>) -> (T, Vec<String>) {
    let mut out = T::default();
    let problems = section.map(|t| out.apply_toml_table(t)).unwrap_or_default();
    (out, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_round_trips_through_get_set_and_toml() {
        let sim = ParticleSimulation::default();
        for f in SIMULATION_FIELDS {
            let v = sim.get(f.name).unwrap_or_else(|| panic!("get {}", f.name));
            let mut s2 = ParticleSimulation::default();
            s2.set(f.name, v.clone()).unwrap_or_else(|e| panic!("set {}: {e}", f.name));
            let back = value_from_toml(f.kind, &value_to_toml(&v)).unwrap();
            assert_eq!(back, v, "{}", f.name);
            let parsed = parse_value(f.kind, &format_value(&v)).unwrap_or_else(|e| panic!("parse {}: {e}", f.name));
            assert_eq!(parsed, v, "{} text round trip", f.name);
        }
        let sp = ParticleSpecies::default();
        for f in SPECIES_FIELDS {
            let v = sp.get(f.name).unwrap_or_else(|| panic!("get {}", f.name));
            let mut s2 = ParticleSpecies::default();
            s2.set(f.name, v.clone()).unwrap_or_else(|e| panic!("set {}: {e}", f.name));
            let back = value_from_toml(f.kind, &value_to_toml(&v)).unwrap();
            assert_eq!(back, v, "{}", f.name);
        }
        // Names and keys are unique.
        for table in [SIMULATION_FIELDS, SPECIES_FIELDS] {
            let mut names: Vec<_> = table.iter().map(|f| f.name).collect();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), table.len());
        }
    }

    #[test]
    fn toml_section_round_trip_keeps_big_and_small_numbers() {
        let mut sp = ParticleSpecies::default();
        sp.set_text("Particle", "Electron").unwrap();
        sp.set_text("Conductor", "copper").unwrap();
        assert_eq!(sp.number_density, 8.47e28);
        assert_eq!(sp.collision_time, 2.7e-14);
        let table = sp.to_toml_table();
        let text = toml::to_string(&table).unwrap();
        let parsed: toml::value::Table = toml::from_str(&text).unwrap();
        let (back, problems): (ParticleSpecies, _) = from_section(Some(&parsed));
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back, sp);
        // Hand-editing the density marks the conductor custom.
        sp.set_text("NumberDensity", "1e28").unwrap();
        assert_eq!(sp.conductor, None);
    }

    #[test]
    fn panel_text_formats() {
        assert_eq!(format_number(8.47e28), "8.47e28");
        assert_eq!(format_number(2.7e-14), "2.7e-14");
        assert_eq!(format_number(9.80665), "9.80665");
        assert_eq!(format_number(0.5), "0.5");
        assert_eq!(format_number(-12.0), "-12");
        assert!(parse_value(FieldKind::Float, "abc").is_err());
        assert!(parse_value(FieldKind::Vector3, "1, 2").is_err());
        assert_eq!(
            parse_value(FieldKind::Color3, "255, 128, 0").unwrap(),
            FieldValue::Color([1.0, 128.0 / 255.0, 0.0])
        );
        assert_eq!(parse_value(FieldKind::Choice(BOUNDARIES), "periodic").unwrap(), FieldValue::Choice("Periodic".into()));
    }

    #[test]
    fn species_resolution() {
        let mut sp = ParticleSpecies::default();
        assert_eq!(sp.resolved().0, SpeciesKind::Fluid);
        sp.set_text("Particle", "Ion").unwrap();
        sp.set_text("ChargeNumber", "2").unwrap();
        sp.set_text("MassAmu", "63.546").unwrap();
        let (k, q, m) = sp.resolved();
        assert_eq!(k, SpeciesKind::Point);
        assert!((q - 2.0 * constants::ELEMENTARY_CHARGE).abs() < 1e-30);
        assert!((m - 63.546 * ATOMIC_MASS_UNIT).abs() / m < 1e-12);
        let p = sp.species_params("Cu2+");
        assert_eq!(p.kind, SpeciesKind::Point);
    }
}

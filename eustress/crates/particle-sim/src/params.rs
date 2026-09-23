//! Solver inputs: simulation-wide parameters, per-species parameters and the
//! obstacles the host passes in each frame. Everything is SI: metres,
//! seconds, kilograms, coulombs, kelvin, volts, tesla.

use bevy_math::{Quat, Vec3};

use crate::constants;

/// What happens to a particle that reaches a face of the domain box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Boundary {
    /// Bounces back with the wall restitution and friction.
    #[default]
    Reflect,
    /// Re-enters from the opposite face (an infinite repeating medium).
    Periodic,
    /// Is removed; its charge is tallied as collected current.
    Absorb,
}

/// How charged particles feel each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Electrostatics {
    /// No particle-particle fields (a screened, free electron gas: Drude).
    #[default]
    Off,
    /// Pairwise softened Coulomb, O(N^2). Exact for small systems.
    Direct,
    /// Particle-mesh: charge deposited on a grid, Poisson solved, field
    /// gathered back (particle-in-cell). Scales to large N.
    Mesh,
}

/// How fluid pressure is found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FluidSolver {
    /// Divergence-free SPH (Bender and Koschier): pressure solved by
    /// iteration so the fluid stays incompressible, with substeps limited by
    /// the flow speed. Fast enough for interactive scenes.
    #[default]
    Dfsph,
    /// Weakly compressible SPH: explicit Tait pressure from an artificial
    /// speed of sound, with substeps limited by that speed. Carries sound
    /// waves; compresses about 1%.
    Wcsph,
}

/// Electrostatic boundary condition on non-periodic faces that are not
/// electrodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum WallPotential {
    /// Conducting walls held at 0 V.
    #[default]
    Grounded,
    /// Insulating walls (zero normal field); charge can pile up on them,
    /// which is what produces a Hall voltage.
    Insulating,
}

/// Point particles move under fields and collisions; fluid particles are
/// SPH parcels that also feel pressure and viscosity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SpeciesKind {
    #[default]
    Point,
    Fluid,
}

/// Initial placement inside a species region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Arrangement {
    #[default]
    Random,
    /// A cubic lattice at the mean spacing (crystals, fluid blocks).
    Lattice,
}

/// Region shape, given in domain fractions (0..1 on each axis).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RegionShape {
    #[default]
    Box,
    /// Ellipsoid inscribed in the region box.
    Sphere,
}

/// Simulation-wide parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct SimParams {
    /// Physical size of the domain box (m). The domain is centred on the origin.
    pub domain_size: Vec3,
    /// Particle boundary per axis (X, Y, Z).
    pub boundary: [Boundary; 3],
    /// Normal restitution on walls and obstacles (0 = sticks, 1 = elastic).
    pub restitution: f32,
    /// Tangential velocity removed on contact (0 = frictionless, 1 = stops).
    pub wall_friction: f32,
    /// Uniform gravitational acceleration (m/s^2).
    pub gravity: Vec3,
    /// Uniform applied electric field (V/m).
    pub electric_field: Vec3,
    /// Uniform applied magnetic field (T).
    pub magnetic_field: Vec3,
    /// Potential difference across the domain along `voltage_axis` (V).
    /// The +V electrode is the max face. Periodic along that axis it acts as
    /// an EMF around the loop (a uniform field of V/L).
    pub applied_voltage: f32,
    /// 0 = X, 1 = Y, 2 = Z. Also the axis the current is measured along.
    pub voltage_axis: usize,
    pub electrostatics: Electrostatics,
    /// Mesh cells along the longest domain axis (Mesh electrostatics).
    pub grid_resolution: u32,
    /// Coulomb softening as a fraction of the mean particle spacing.
    pub softening: f32,
    /// Add a uniform background charge that cancels the particles' net
    /// charge (the ion lattice of a metal, a quasi-neutral plasma).
    pub neutralizing_background: bool,
    pub wall_potential: WallPotential,
    /// SPH support radius as a multiple of the fluid particle spacing.
    pub kernel_radius_ratio: f32,
    pub fluid_solver: FluidSolver,
    /// Artificial speed of sound (m/s, WCSPH); 0 picks 10x the expected
    /// flow speed. Also the velocity scale of the artificial viscosity.
    pub speed_of_sound: f32,
    /// Monaghan artificial viscosity coefficient (stabiliser, ~0.01..0.1).
    pub artificial_viscosity: f32,
    /// Surface tension coefficient (N/m) for the cohesion force.
    pub surface_tension: f32,
    /// Lattice / background temperature (K): what scattering thermalises to.
    pub lattice_temperature: f32,
    /// Energy lost to scattering heats the lattice (else the lattice is an
    /// ideal heat sink held at `lattice_temperature`).
    pub joule_heating: bool,
    /// Volumetric heat capacity of the lattice (J/(m^3 K)).
    pub lattice_heat_capacity: f32,
    /// Fixed substep (s); 0 derives it every frame from the stability limits.
    pub timestep: f32,
    /// Upper bound on substeps per `advance` call (protects the frame rate).
    pub max_substeps: u32,
    /// Hard cap on live particles (emission stops there).
    pub max_particles: u32,
    /// Seed for every random draw (placement, thermal velocities, scattering).
    pub seed: u64,
}

impl Default for SimParams {
    fn default() -> Self {
        Self {
            domain_size: Vec3::splat(1.0),
            boundary: [Boundary::Reflect; 3],
            restitution: 0.3,
            wall_friction: 0.05,
            gravity: Vec3::new(0.0, -9.80665, 0.0),
            electric_field: Vec3::ZERO,
            magnetic_field: Vec3::ZERO,
            applied_voltage: 0.0,
            voltage_axis: 0,
            electrostatics: Electrostatics::Off,
            grid_resolution: 32,
            softening: 0.3,
            neutralizing_background: true,
            wall_potential: WallPotential::Grounded,
            kernel_radius_ratio: 2.4,
            fluid_solver: FluidSolver::Dfsph,
            speed_of_sound: 0.0,
            artificial_viscosity: 0.05,
            surface_tension: 0.0,
            lattice_temperature: 293.15,
            joule_heating: false,
            lattice_heat_capacity: 3.45e6,
            timestep: 0.0,
            max_substeps: 64,
            max_particles: 200_000,
            seed: 1,
        }
    }
}

impl SimParams {
    pub fn volume(&self) -> f64 {
        self.domain_size.x as f64 * self.domain_size.y as f64 * self.domain_size.z as f64
    }

    pub fn half(&self) -> Vec3 {
        self.domain_size * 0.5
    }

    /// The uniform field the particles are driven by: the applied field plus
    /// the V/L of the applied voltage (E = -grad phi, so +V on the max face
    /// points the field toward the min face).
    pub fn drive_field(&self) -> Vec3 {
        let mut e = self.electric_field;
        let axis = self.voltage_axis.min(2);
        let len = self.domain_size[axis];
        if self.applied_voltage != 0.0 && len > 0.0 {
            e[axis] -= self.applied_voltage / len;
        }
        e
    }
}

/// One population of identical particles.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeciesParams {
    pub name: String,
    pub kind: SpeciesKind,
    /// Charge of one real particle (C).
    pub charge: f64,
    /// Mass of one real particle (kg). Ignored for fluids (derived from
    /// `rest_density` and the particle spacing).
    pub mass: f64,
    /// Simulated particles placed at reset.
    pub count: u32,
    /// Real particles per m^3 inside the region; each simulated particle
    /// then stands for `number_density * region_volume / count` real ones.
    /// 0 means one simulated particle is one real particle.
    pub number_density: f64,
    /// Fluid rest density (kg/m^3).
    pub rest_density: f32,
    /// Fluid dynamic viscosity (Pa s).
    pub viscosity: f32,
    /// Initial temperature (K): Maxwell-Boltzmann thermal velocities.
    pub temperature: f32,
    /// Initial bulk velocity (m/s).
    pub drift_velocity: Vec3,
    pub region_shape: RegionShape,
    /// Region corners in domain fractions (0..1).
    pub region_min: Vec3,
    pub region_max: Vec3,
    pub arrangement: Arrangement,
    /// Immobile particles (a fixed ion lattice) still act as field sources.
    pub mobile: bool,
    /// Mean free time between lattice collisions (s); 0 = no scattering.
    /// This is the Drude relaxation time.
    pub collision_time: f32,
    /// New particles per second of simulated time, emitted into the region.
    pub emission_rate: f32,
    /// Display colour (linear RGBA).
    pub color: [f32; 4],
    /// Display radius in world metres; 0 derives it from the spacing.
    pub display_radius: f32,
    pub enabled: bool,
}

impl Default for SpeciesParams {
    fn default() -> Self {
        Self {
            name: "Species".into(),
            kind: SpeciesKind::Point,
            charge: 0.0,
            mass: constants::PROTON_MASS,
            count: 1000,
            number_density: 0.0,
            rest_density: constants::WATER_DENSITY,
            viscosity: constants::WATER_VISCOSITY,
            temperature: 293.15,
            drift_velocity: Vec3::ZERO,
            region_shape: RegionShape::Box,
            region_min: Vec3::ZERO,
            region_max: Vec3::ONE,
            arrangement: Arrangement::Random,
            mobile: true,
            collision_time: 0.0,
            emission_rate: 0.0,
            color: [1.0, 1.0, 1.0, 1.0],
            display_radius: 0.0,
            enabled: true,
        }
    }
}

impl SpeciesParams {
    /// Region box clamped to the domain, in fractions.
    pub fn region_fractions(&self) -> (Vec3, Vec3) {
        let a = self.region_min.clamp(Vec3::ZERO, Vec3::ONE);
        let b = self.region_max.clamp(Vec3::ZERO, Vec3::ONE);
        (a.min(b), a.max(b))
    }

    /// Physical volume of the region (m^3).
    pub fn region_volume(&self, sim: &SimParams) -> f64 {
        let (a, b) = self.region_fractions();
        let ext = (b - a) * sim.domain_size;
        let boxv = ext.x as f64 * ext.y as f64 * ext.z as f64;
        match self.region_shape {
            RegionShape::Box => boxv,
            RegionShape::Sphere => boxv * std::f64::consts::PI / 6.0,
        }
    }
}

/// Static or moving body the particles collide with, in domain coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    pub shape: ObstacleShape,
    pub center: Vec3,
    pub rotation: Quat,
    /// Box: half extents. Sphere: radius in `x`. Cylinder: radius in `x`,
    /// half height in `y` (axis along local Y).
    pub half_extents: Vec3,
    /// Surface velocity (m/s) for moving bodies.
    pub velocity: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObstacleShape {
    Box,
    Sphere,
    Cylinder,
}

impl Obstacle {
    /// Signed distance from `p` to the surface (negative inside).
    #[inline]
    pub fn sdf(&self, p: Vec3) -> f32 {
        let q = self.rotation.inverse() * (p - self.center);
        match self.shape {
            ObstacleShape::Box => {
                let d = q.abs() - self.half_extents;
                d.max(Vec3::ZERO).length() + d.x.max(d.y).max(d.z).min(0.0)
            }
            ObstacleShape::Sphere => q.length() - self.half_extents.x,
            ObstacleShape::Cylinder => {
                let dx = (q.x * q.x + q.z * q.z).sqrt() - self.half_extents.x;
                let dy = q.y.abs() - self.half_extents.y;
                dx.max(dy).min(0.0) + (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt()
            }
        }
    }

    /// Outward surface normal at `p` (central differences of the SDF).
    #[inline]
    pub fn normal(&self, p: Vec3, eps: f32) -> Vec3 {
        let ex = Vec3::new(eps, 0.0, 0.0);
        let ey = Vec3::new(0.0, eps, 0.0);
        let ez = Vec3::new(0.0, 0.0, eps);
        Vec3::new(
            self.sdf(p + ex) - self.sdf(p - ex),
            self.sdf(p + ey) - self.sdf(p - ey),
            self.sdf(p + ez) - self.sdf(p - ez),
        )
        .normalize_or_zero()
    }

    /// Radius of a sphere that bounds the obstacle.
    pub fn bounding_radius(&self) -> f32 {
        match self.shape {
            ObstacleShape::Box => self.half_extents.length(),
            ObstacleShape::Sphere => self.half_extents.x,
            ObstacleShape::Cylinder => {
                (self.half_extents.x * self.half_extents.x + self.half_extents.y * self.half_extents.y)
                    .sqrt()
            }
        }
    }
}

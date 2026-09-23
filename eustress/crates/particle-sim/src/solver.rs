//! The stepper: state, reset, substepping and every force.
//!
//! Per substep:
//! 1. fluids: bin into the cell grid (arrays permuted into cell order),
//!    SPH density and neighbour lists, with walls and obstacles present as
//!    boundary particles; DFSPH makes the velocity field divergence-free;
//! 2. electrostatics: direct softened Coulomb or particle-mesh;
//! 3. forces and push: artificial and laminar viscosity, CSF surface
//!    tension, gravity, electric field, Boris rotation in B; fluid pressure
//!    either explicit (WCSPH, Tait) or solved so the predicted density is
//!    rest density (DFSPH) before anything moves;
//! 4. Drude scattering against the lattice (energy to the lattice);
//! 5. walls and obstacles (restitution, friction, impulse back to bodies);
//! 6. emission.
//!
//! Fresh fluid is relaxed to rest density before the first substep.

use bevy_math::Vec3;
use rayon::prelude::*;

use super::boundary::{BoundarySet, WALL};
use super::colormap::{coolwarm, viridis, ColorMode};
use super::diagnostics::{SimStats, SpeciesStats};
use super::grid::CellGrid;
use super::params::*;
use super::poisson::{AxisBc, MeshField};
use super::presets;
use super::rng::{counter_normal, counter_uniform, SimRng};
use crate::constants;
use crate::kernel::CubicSpline;

/// Particles per reduction chunk. Reductions (obstacle reactions, absorbed
/// charge, heat, solver errors) sum within a chunk and then over chunks in
/// index order, so results do not depend on the thread count.
const CHUNK: usize = 64;

/// Fewest particles one parallel task takes in the light passes (listed
/// neighbours, integration): below this, waking another thread costs more
/// than the work it would share. A multiple of `CHUNK`.
const PAR_MIN_PARTICLES: usize = 256;

/// The same for grid-scan passes, which do enough work per particle (a few
/// hundred candidates) to split finely.
const PAR_MIN_SCAN: usize = 16;

/// Marks a particle whose neighbours did not fit its list.
const NB_OVERFLOW: u16 = u16::MAX;

/// List slots per particle: about 1.6 times the neighbours a uniform fluid
/// has inside the kernel support (4/3 pi r^3, r the kernel radius in
/// spacings), so compression rarely overflows; bounded so memory stays
/// linear in the particle count.
fn neighbour_capacity(params: &SimParams) -> usize {
    // Room for the boundary particles DFSPH lists after the fluid ones.
    ((2.2 * expected_neighbours(params)).ceil() as usize).clamp(32, 192)
}

/// Neighbours a particle inside a uniform fluid has.
fn expected_neighbours(params: &SimParams) -> f32 {
    let r = params.kernel_radius_ratio.max(1.0);
    4.0 / 3.0 * std::f32::consts::PI * r * r * r
}

/// Fewer neighbours than this (about a third of the interior count) marks a
/// particle at a free surface.
fn deficient_neighbours(params: &SimParams) -> u32 {
    (0.35 * expected_neighbours(params)) as u32
}

/// Read view of the neighbour lists the density pass built: per particle,
/// its fluid neighbours, then (DFSPH) its boundary neighbours.
#[derive(Clone, Copy)]
struct Neighbours<'a> {
    idx: &'a [u32],
    len: &'a [u16],
    fluid_len: &'a [u16],
    cap: usize,
}

impl Neighbours<'_> {
    /// The listed fluid neighbours of `i`, or None when its list overflowed.
    #[inline]
    fn of(&self, i: usize) -> Option<&[u32]> {
        let n = *self.len.get(i)?;
        let f = *self.fluid_len.get(i)? as usize;
        (n != NB_OVERFLOW).then(|| &self.idx[i * self.cap..i * self.cap + f])
    }

    /// Slot range of the listed boundary neighbours of `i`.
    #[inline]
    fn boundary_slots(&self, i: usize) -> Option<std::ops::Range<usize>> {
        let n = *self.len.get(i)?;
        let f = *self.fluid_len.get(i)? as usize;
        (n != NB_OVERFLOW).then(|| i * self.cap + f..i * self.cap + n as usize)
    }
}

/// DFSPH pressure solve: the mean density error it accepts (0.05%).
const DFSPH_DENSITY_TOLERANCE: f32 = 5e-4;
/// DFSPH divergence solve: the mean density change per substep it accepts
/// (0.1%).
const DFSPH_DIVERGENCE_TOLERANCE: f32 = 1e-3;
/// Iteration cap of either DFSPH solve.
const DFSPH_MAX_ITERATIONS: u32 = 100;
/// Share of the last substep's pressure a pressure solve starts from.
const DFSPH_WARM_START: f32 = 0.5;
/// Longest DFSPH substep (s). The flow-speed limit alone would allow long
/// steps in still water; wall contact and gravity want a ceiling.
const DFSPH_MAX_SUBSTEP: f32 = 5e-3;

/// Fluid neighbours of a particle with V_j grad W_ij, from the lists and
/// gradients the density pass cached, or a grid scan where a list
/// overflowed.
#[derive(Clone, Copy)]
struct FluidPairs<'a> {
    nb: Neighbours<'a>,
    gv: &'a [Vec3],
    grid: &'a CellGrid,
    pos: &'a [Vec3],
    sp: &'a [u16],
    species: &'a [SpeciesRuntime],
    kernel: CubicSpline,
}

impl FluidPairs<'_> {
    #[inline]
    fn for_each(&self, i: usize, mut f: impl FnMut(usize, Vec3)) {
        if let Some(list) = self.nb.of(i) {
            let base = i * self.nb.cap;
            for (slot, &j) in list.iter().enumerate() {
                f(j as usize, self.gv[base + slot]);
            }
            return;
        }
        let xi = self.pos[i];
        let h2 = self.kernel.h * self.kernel.h;
        self.grid.for_each_candidate(xi, |j| {
            if j == i {
                return;
            }
            let sj = &self.species[self.sp[j] as usize];
            if !sj.fluid {
                return;
            }
            let rv = self.grid.delta(xi, self.pos[j]);
            let r2 = rv.length_squared();
            if r2 >= h2 || r2 <= 1e-12 * h2 {
                return;
            }
            let r = r2.sqrt();
            f(j, rv * (sj.m / sj.params.rest_density * self.kernel.dw(r) / r));
        });
    }
}

/// Boundary particles inside the kernel support of a fluid particle, with
/// V_b grad W: from its cached list, or a boundary-grid scan where the list
/// overflowed.
#[derive(Clone, Copy)]
struct BoundaryPairs<'a> {
    nb: Neighbours<'a>,
    gv: &'a [Vec3],
    pos: &'a [Vec3],
    bnd: &'a BoundarySet,
    kernel: CubicSpline,
}

impl BoundaryPairs<'_> {
    #[inline]
    fn for_each(&self, i: usize, mut f: impl FnMut(usize, Vec3)) {
        if self.bnd.is_empty() {
            return;
        }
        if let Some(slots) = self.nb.boundary_slots(i) {
            for slot in slots {
                f(self.nb.idx[slot] as usize, self.gv[slot]);
            }
            return;
        }
        let x = self.pos[i];
        let h2 = self.kernel.h * self.kernel.h;
        self.bnd.grid.for_each_candidate(x, |b| {
            let rv = self.bnd.grid.delta(x, self.bnd.pos[b]);
            let r2 = rv.length_squared();
            if r2 >= h2 || r2 <= 1e-12 * h2 {
                return;
            }
            let r = r2.sqrt();
            f(b, rv * (self.bnd.vol[b] * self.kernel.dw(r) / r));
        });
    }
}

/// Kick with acceleration `a` over `dt`, rotating in `b` for charges (Boris).
#[inline]
fn kick(v: Vec3, a: Vec3, s: &SpeciesRuntime, b: Vec3, dt: f32) -> Vec3 {
    let half_dt = 0.5 * dt;
    let v_minus = v + a * half_dt;
    let v_plus = if s.charged && b != Vec3::ZERO {
        boris_rotate(v_minus, b * (s.q_over_m * half_dt))
    } else {
        v_minus
    };
    v_plus + a * half_dt
}
use constants::COULOMB_K;

/// Per-species constants derived from the parameters.
#[derive(Clone, Debug)]
pub struct SpeciesRuntime {
    pub params: SpeciesParams,
    /// Real particles per simulated particle.
    pub weight: f64,
    /// Charge of one simulated particle (C).
    pub q: f32,
    /// Mass of one simulated particle (kg).
    pub m: f32,
    pub q_over_m: f32,
    /// Mass of one real particle (kg); for fluids, the parcel mass.
    pub real_mass: f64,
    /// Mean initial spacing of simulated particles (m).
    pub spacing: f32,
    pub fluid: bool,
    pub charged: bool,
    /// Speed of sound and Tait stiffness (fluids).
    pub c0: f32,
    pub tait_b: f32,
    /// Where particles are placed and emitted (domain coordinates).
    pub region: (Vec3, Vec3),
}

/// Placement box of a species. Fluids keep one particle spacing off
/// reflecting walls: the boundary particles already supply the wall's share
/// of the density, so fluid placed closer starts compressed and bursts.
fn placement_region(sim: &SimParams, p: &SpeciesParams, inset: f32) -> (Vec3, Vec3) {
    let half = sim.half();
    let (fa, fb) = p.region_fractions();
    let mut lo = -half + fa * sim.domain_size;
    let mut hi = -half + fb * sim.domain_size;
    if inset > 0.0 {
        for a in 0..3 {
            if sim.boundary[a] != Boundary::Reflect {
                continue;
            }
            lo[a] = lo[a].max(-half[a] + inset);
            hi[a] = hi[a].min(half[a] - inset);
            if hi[a] < lo[a] {
                let mid = 0.5 * (lo[a] + hi[a]);
                lo[a] = mid;
                hi[a] = mid;
            }
        }
    }
    (lo, hi)
}

fn region_volume(lo: Vec3, hi: Vec3, shape: RegionShape) -> f64 {
    let e = (hi - lo).max(Vec3::ZERO);
    let v = e.x as f64 * e.y as f64 * e.z as f64;
    match shape {
        RegionShape::Box => v,
        RegionShape::Sphere => v * std::f64::consts::PI / 6.0,
    }
}

impl SpeciesRuntime {
    fn derive(sim: &SimParams, p: &SpeciesParams) -> Self {
        let count = p.count.max(1) as f64;
        let region_v = p.region_volume(sim).max(1e-300);
        let fluid = p.kind == SpeciesKind::Fluid;
        if fluid {
            let s0 = (region_v / count).cbrt() as f32;
            let region = placement_region(sim, p, 0.5 * s0);
            let spacing = (region_volume(region.0, region.1, p.region_shape).max(1e-300) / count).cbrt() as f32;
            // Mass such that the initial cubic lattice sums to exactly the
            // rest density (the kernel sum over a lattice is not exactly
            // 1/s^3); otherwise the block starts off rest density and rings.
            let kernel = CubicSpline::new(sim.kernel_radius_ratio.max(1.2) * spacing);
            let m = p.rest_density / lattice_kernel_sum(kernel, spacing);
            let (a, b) = p.region_fractions();
            let height = ((b.y - a.y) * sim.domain_size.y).max(spacing);
            let g = sim.gravity.length();
            let v_expected = (2.0 * g * height).sqrt().max(p.drift_velocity.length());
            let c0 = if sim.speed_of_sound > 0.0 {
                sim.speed_of_sound
            } else {
                // Weakly compressible: 10x the flow speed keeps density
                // variation near 1%.
                (10.0 * v_expected).max(10.0 * spacing / 1e-3_f32.max(spacing))
            };
            let tait_b = p.rest_density * c0 * c0 / 7.0;
            Self {
                params: p.clone(),
                weight: 1.0,
                q: p.charge as f32,
                m,
                q_over_m: if m > 0.0 { p.charge as f32 / m } else { 0.0 },
                real_mass: m as f64,
                spacing,
                fluid,
                charged: p.charge != 0.0,
                c0,
                tait_b,
                region,
            }
        } else {
            let spacing = (region_v / count).cbrt() as f32;
            let weight = if p.number_density > 0.0 { p.number_density * region_v / count } else { 1.0 };
            let mass = p.mass.max(1e-40);
            Self {
                params: p.clone(),
                weight,
                q: (p.charge * weight) as f32,
                m: (mass * weight) as f32,
                q_over_m: (p.charge / mass) as f32,
                real_mass: mass,
                spacing,
                fluid,
                charged: p.charge != 0.0,
                c0: 0.0,
                tait_b: 0.0,
                region: placement_region(sim, p, 0.0),
            }
        }
    }

    /// A fluid placed at `spacing` instead of its nominal one: the parcel
    /// mass that makes that cubic lattice exactly rest density under the
    /// simulation's kernel.
    fn set_fluid_spacing(&mut self, sim: &SimParams, spacing: f32) {
        if !self.fluid || spacing <= 0.0 {
            return;
        }
        let kernel = CubicSpline::new(sim.kernel_radius_ratio.max(1.2) * spacing);
        self.spacing = spacing;
        self.m = self.params.rest_density / lattice_kernel_sum(kernel, spacing);
        self.real_mass = self.m as f64;
        self.q_over_m = if self.m > 0.0 { self.q / self.m } else { 0.0 };
    }
}

/// Rendering record for one particle: position in domain coordinates (m),
/// display radius (world m) and linear RGBA.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ParticleInstance {
    pub position: [f32; 3],
    pub radius: f32,
    pub color: [f32; 4],
}

/// Momentum the particles handed to one obstacle, in domain coordinates:
/// linear (N s) and angular about the obstacle's centre (N m s).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyImpulse {
    pub linear: Vec3,
    pub angular: Vec3,
}

impl BodyImpulse {
    #[inline]
    fn add(&mut self, impulse: Vec3, at: Vec3, center: Vec3) {
        self.linear += impulse;
        self.angular += (at - center).cross(impulse);
    }
}

impl std::ops::AddAssign for BodyImpulse {
    fn add_assign(&mut self, o: Self) {
        self.linear += o.linear;
        self.angular += o.angular;
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct MotionExtremes {
    fluid_v: f32,
    fluid_a: f32,
    point_v: f32,
    point_a: f32,
}

/// A running particle simulation.
#[derive(Clone, Debug)]
pub struct ParticleSim {
    pub params: SimParams,
    pub species: Vec<SpeciesRuntime>,
    pub pos: Vec<Vec3>,
    pub vel: Vec<Vec3>,
    /// Species index per particle.
    pub sp: Vec<u16>,
    pub density: Vec<f32>,
    pub pressure: Vec<f32>,
    /// Self-consistent electric field at each particle (V/m), excluding the
    /// uniform applied field.
    pub efield: Vec<Vec3>,
    acc: Vec<Vec3>,
    grid: CellGrid,
    /// Fluid neighbours of each fluid particle inside the kernel support,
    /// found by the density pass and reused by the force pass so the cell
    /// grid is scanned once per substep: `nb_cap` slots per particle,
    /// `nb_len[i]` of them used, `NB_OVERFLOW` when there were more (that
    /// particle scans the grid again instead).
    nb: Vec<u32>,
    nb_len: Vec<u16>,
    nb_fluid_len: Vec<u16>,
    nb_cap: usize,
    /// V_j grad W_ij of each listed neighbour (DFSPH).
    nb_gv: Vec<Vec3>,
    /// Neighbours below which a particle is at a free surface and skips the
    /// divergence solve.
    nb_deficient: u32,
    /// DFSPH per particle: stiffness factor, predicted density ratio (or
    /// density change rate), the pressure-like field of the current
    /// iteration, and its sum over a substep's pressure iterations.
    alpha: Vec<f32>,
    rho_adv: Vec<f32>,
    kfac: Vec<f32>,
    kappa_sum: Vec<f32>,
    /// Each particle's summed stiffness times dt^2 at the end of the last
    /// substep: the warm start of the next pressure solve (it follows the
    /// particles through sorting, emission and removal).
    warm_kappa: Vec<f32>,
    /// Pressure iterations in the last substep.
    dfsph_iterations: u32,
    fluid_kernel: Option<CubicSpline>,
    mesh: Option<MeshField>,
    /// Walls and obstacle surfaces as fluid boundary particles.
    boundary: BoundarySet,
    boundary_dirty: bool,
    obstacles: Vec<Obstacle>,
    obstacle_impulse: Vec<BodyImpulse>,
    pub time: f64,
    pub steps: u64,
    pub lattice_temperature: f32,
    pub lattice_energy: f64,
    emission_accum: Vec<f64>,
    plane_charge: f64,
    plane_time: f64,
    current_ema: f64,
    j_ema: [f64; 3],
    absorbed_charge: f64,
    extremes: MotionExtremes,
    rng: SimRng,
    /// Simulated time owed to the clock and not yet stepped (s).
    owed: f64,
    pub stats: SimStats,
}

impl ParticleSim {
    pub fn new(params: SimParams, species: Vec<SpeciesParams>) -> Self {
        let species: Vec<SpeciesParams> = species.into_iter().filter(|s| s.enabled).collect();
        let runtime: Vec<SpeciesRuntime> = species.iter().map(|s| SpeciesRuntime::derive(&params, s)).collect();
        let fluid_kernel = fluid_kernel(&params, &runtime);
        let nb_cap = neighbour_capacity(&params);
        let nb_deficient = deficient_neighbours(&params);
        let mut sim = Self {
            rng: SimRng::new(params.seed, 0x5157),
            lattice_temperature: params.lattice_temperature,
            emission_accum: vec![0.0; runtime.len()],
            params,
            species: runtime,
            pos: Vec::new(),
            vel: Vec::new(),
            sp: Vec::new(),
            density: Vec::new(),
            pressure: Vec::new(),
            efield: Vec::new(),
            acc: Vec::new(),
            grid: CellGrid::default(),
            nb: Vec::new(),
            nb_len: Vec::new(),
            nb_fluid_len: Vec::new(),
            nb_cap,
            nb_gv: Vec::new(),
            nb_deficient,
            alpha: Vec::new(),
            rho_adv: Vec::new(),
            kfac: Vec::new(),
            kappa_sum: Vec::new(),
            warm_kappa: Vec::new(),
            dfsph_iterations: 0,
            fluid_kernel,
            mesh: None,
            boundary: BoundarySet::default(),
            boundary_dirty: true,
            obstacles: Vec::new(),
            obstacle_impulse: Vec::new(),
            time: 0.0,
            steps: 0,
            lattice_energy: 0.0,
            plane_charge: 0.0,
            plane_time: 0.0,
            current_ema: 0.0,
            j_ema: [0.0; 3],
            absorbed_charge: 0.0,
            extremes: MotionExtremes::default(),
            owed: 0.0,
            stats: SimStats::default(),
        };
        sim.populate();
        sim.compute_stats(0.0, 0.0, 0);
        sim
    }

    /// Back to the initial state described by the current parameters.
    pub fn reset(&mut self) {
        let species = self.species.iter().map(|s| s.params.clone()).collect();
        let obstacles = std::mem::take(&mut self.obstacles);
        *self = Self::new(self.params.clone(), species);
        self.set_obstacles(obstacles);
    }

    /// Apply new parameters. Changes to initial conditions (domain, seed,
    /// counts, regions, arrangement, densities, temperatures) reset the run;
    /// everything else (fields, gravity, viscosity, scattering, boundaries,
    /// timestep, colours) applies live without disturbing the particles.
    /// Returns true when it reset.
    pub fn reconfigure(&mut self, params: SimParams, species: Vec<SpeciesParams>) -> bool {
        let species: Vec<SpeciesParams> = species.into_iter().filter(|s| s.enabled).collect();
        let structural = params.domain_size != self.params.domain_size
            || params.seed != self.params.seed
            || params.max_particles != self.params.max_particles
            || species.len() != self.species.len()
            || species.iter().zip(&self.species).any(|(n, o)| initial_conditions_differ(n, &o.params));
        if structural {
            let obstacles = std::mem::take(&mut self.obstacles);
            *self = Self::new(params, species);
            self.set_obstacles(obstacles);
            return true;
        }
        let mesh_changed = params.boundary != self.params.boundary
            || params.grid_resolution != self.params.grid_resolution
            || params.wall_potential != self.params.wall_potential
            || params.voltage_axis != self.params.voltage_axis
            || (params.applied_voltage != self.params.applied_voltage)
            || params.electrostatics != self.params.electrostatics;
        if params.lattice_temperature != self.params.lattice_temperature {
            self.lattice_temperature = params.lattice_temperature;
        }
        self.params = params;
        let placed: Vec<f32> = self.species.iter().map(|s| s.spacing).collect();
        self.species = species.iter().map(|s| SpeciesRuntime::derive(&self.params, s)).collect();
        // The particles stay where they were placed: keep that spacing, with
        // the mass that matches it under the (possibly new) kernel.
        for (s, &spacing) in self.species.iter_mut().zip(&placed) {
            if s.fluid && spacing != s.spacing {
                s.set_fluid_spacing(&self.params, spacing);
            }
        }
        self.fluid_kernel = fluid_kernel(&self.params, &self.species);
        self.nb_cap = neighbour_capacity(&self.params);
        self.nb_deficient = deficient_neighbours(&self.params);
        if mesh_changed {
            self.mesh = None;
        }
        self.boundary_dirty = true;
        false
    }

    pub fn particle_count(&self) -> usize {
        self.pos.len()
    }

    /// Obstacles for the coming frame (domain coordinates). Fluids resample
    /// their boundary only when the set actually changed.
    pub fn set_obstacles(&mut self, obstacles: Vec<Obstacle>) {
        if obstacles.len() != self.obstacle_impulse.len() {
            self.obstacle_impulse = vec![BodyImpulse::default(); obstacles.len()];
        }
        if obstacles != self.obstacles {
            self.boundary_dirty = true;
        }
        self.obstacles = obstacles;
    }

    pub fn obstacles(&self) -> &[Obstacle] {
        &self.obstacles
    }

    /// Smallest fluid particle spacing (the boundary sampling distance).
    fn fluid_spacing(&self) -> Option<f32> {
        self.species
            .iter()
            .filter(|s| s.fluid)
            .map(|s| s.spacing)
            .reduce(f32::min)
    }

    /// Impulse (N s) the particles delivered to each obstacle since the last
    /// call, in the order passed to `set_obstacles`.
    pub fn take_obstacle_impulses(&mut self) -> Vec<BodyImpulse> {
        let out = self.obstacle_impulse.clone();
        for v in &mut self.obstacle_impulse {
            *v = BodyImpulse::default();
        }
        out
    }

    // ------------------------------------------------------------------
    // Initial state
    // ------------------------------------------------------------------

    fn populate(&mut self) {
        let cap = self.params.max_particles as usize;
        for si in 0..self.species.len() {
            let s = self.species[si].clone();
            let want = (s.params.count as usize).min(cap.saturating_sub(self.pos.len()));
            if want == 0 {
                continue;
            }
            let mut rng = SimRng::new(self.params.seed, 1000 + si as u64);
            let (lo, hi) = s.region;
            let points = match s.params.arrangement {
                Arrangement::Lattice => {
                    let (points, used) = lattice_points(lo, hi, s.spacing, want, s.params.region_shape);
                    if s.fluid && used < s.spacing {
                        // Particles sit closer than the nominal spacing: give
                        // them the mass that makes that lattice rest density,
                        // or the fluid starts compressed and bursts.
                        self.species[si].set_fluid_spacing(&self.params, used);
                    }
                    points
                }
                Arrangement::Random => (0..want)
                    .map(|_| sample_region(&mut rng, lo, hi, s.params.region_shape))
                    .collect(),
            };
            let sigma_v = thermal_sigma(s.params.temperature, s.real_mass);
            for p in points {
                let v = if s.fluid || !s.params.mobile {
                    if s.params.mobile { s.params.drift_velocity } else { Vec3::ZERO }
                } else {
                    s.params.drift_velocity + rng.normal3() * sigma_v
                };
                self.push_particle(si as u16, p, v);
            }
        }
        // Spacings may have tightened to fit the counts.
        self.fluid_kernel = fluid_kernel(&self.params, &self.species);
        self.relax_fluid();
        self.extremes = self.measure_extremes();
    }

    /// Nudge freshly placed fluid until it is at rest density everywhere,
    /// walls included. A lattice cut off by a wall starts a few percent
    /// dense against it (the boundary particles and the first fluid layer
    /// overlap), which an incompressible solver would fix in one violent
    /// first substep. Pressure-only moves, with no gravity and no inertia,
    /// until the densest particle is within 0.5% of rest density; then the
    /// fluid gets back its initial velocity. Deterministic.
    fn relax_fluid(&mut self) {
        if self.fluid_kernel.is_none() || !self.species.iter().any(|s| s.fluid) || self.pos.is_empty() {
            return;
        }
        const MAX_ROUNDS: usize = 12;
        const DENSEST: f32 = 0.005;
        for _ in 0..MAX_ROUNDS {
            if self.boundary_dirty {
                if let (Some(k), Some(s)) = (self.fluid_kernel, self.fluid_spacing()) {
                    self.boundary.rebuild(&self.params, &self.obstacles, s, k);
                }
                self.boundary_dirty = false;
            }
            self.bin_and_sort();
            self.compute_density(true);
            let worst = self
                .density
                .iter()
                .zip(&self.sp)
                .filter(|(_, &s)| self.species[s as usize].fluid)
                .map(|(d, &s)| d / self.species[s as usize].params.rest_density - 1.0)
                .fold(0.0f32, f32::max);
            if worst < DENSEST {
                break;
            }
            let (sp, species) = (&self.sp, &self.species);
            self.vel.par_iter_mut().enumerate().for_each(|(i, v)| {
                if species[sp[i] as usize].fluid {
                    *v = Vec3::ZERO;
                }
            });
            // With a unit step the solve's velocities are the displacements
            // that bring the predicted density to rest density (each round
            // from scratch: no warm start).
            self.warm_kappa.iter_mut().for_each(|w| *w = 0.0);
            self.pressure_solve(1.0);
            let (vel, sp, species) = (&self.vel, &self.sp, &self.species);
            self.pos.par_iter_mut().enumerate().for_each(|(i, p)| {
                if species[sp[i] as usize].fluid {
                    *p += vel[i];
                }
            });
            self.apply_boundaries();
        }
        let (sp, species) = (&self.sp, &self.species);
        self.vel.par_iter_mut().enumerate().for_each(|(i, v)| {
            let s = &species[sp[i] as usize];
            if s.fluid {
                *v = if s.params.mobile { s.params.drift_velocity } else { Vec3::ZERO };
            }
        });
        self.pressure.iter_mut().for_each(|p| *p = 0.0);
        self.warm_kappa.iter_mut().for_each(|w| *w = 0.0);
        for imp in &mut self.obstacle_impulse {
            *imp = BodyImpulse::default();
        }
    }

    fn push_particle(&mut self, species: u16, p: Vec3, v: Vec3) {
        self.pos.push(p);
        self.vel.push(v);
        self.sp.push(species);
        self.density.push(0.0);
        self.pressure.push(0.0);
        self.efield.push(Vec3::ZERO);
        self.acc.push(Vec3::ZERO);
        self.warm_kappa.push(0.0);
    }

    fn measure_extremes(&self) -> MotionExtremes {
        let mut m = MotionExtremes::default();
        for (i, v) in self.vel.iter().enumerate() {
            let s = &self.species[self.sp[i] as usize];
            if s.fluid {
                m.fluid_v = m.fluid_v.max(v.length());
            } else {
                m.point_v = m.point_v.max(v.length());
            }
        }
        m
    }

    // ------------------------------------------------------------------
    // Time stepping
    // ------------------------------------------------------------------

    /// The largest substep the current state allows.
    pub fn stable_timestep(&self) -> f32 {
        let p = &self.params;
        let mut dt = f32::INFINITY;
        let ex = self.extremes;
        if let Some(k) = self.fluid_kernel {
            let h = k.h;
            let dfsph = p.fluid_solver == FluidSolver::Dfsph;
            for s in self.species.iter().filter(|s| s.fluid) {
                if dfsph {
                    // Flow-speed CFL: nothing crosses 0.4 of a spacing per
                    // substep (the speed a substep can add included).
                    let v = ex.fluid_v + ex.fluid_a * DFSPH_MAX_SUBSTEP;
                    if v > 0.0 {
                        dt = dt.min(0.4 * s.spacing / v);
                    }
                    // Explicit artificial viscosity (nu ~ alpha h c0 / 8)
                    // stays stable.
                    if p.artificial_viscosity > 0.0 && s.c0 > 0.0 {
                        dt = dt.min(h / (p.artificial_viscosity * s.c0));
                    }
                } else {
                    dt = dt.min(0.25 * h / (s.c0 + ex.fluid_v));
                }
                if s.params.viscosity > 0.0 {
                    dt = dt.min(0.125 * h * h * s.params.rest_density / s.params.viscosity);
                }
            }
            if ex.fluid_a > 0.0 {
                dt = dt.min(0.25 * (h / ex.fluid_a).sqrt());
            }
            if dfsph {
                dt = dt.min(DFSPH_MAX_SUBSTEP);
            }
        }
        let volume = p.volume();
        let b = p.magnetic_field.length();
        let e_drive = p.drive_field().length();
        for s in self.species.iter().filter(|s| !s.fluid && s.params.mobile) {
            let n_real = s.weight * s.params.count as f64 / volume;
            if s.charged && p.electrostatics != Electrostatics::Off {
                let wp = presets::plasma_frequency(n_real, s.params.charge, s.real_mass);
                if wp > 0.0 {
                    dt = dt.min((0.2 / wp) as f32);
                }
            }
            if s.charged && b > 0.0 {
                let wc = s.q_over_m.abs() * b;
                if wc > 0.0 {
                    dt = dt.min(0.2 / wc);
                }
            }
            if s.params.collision_time > 0.0 {
                dt = dt.min(0.2 * s.params.collision_time);
            }
            // Resolve motion: no particle crosses more than half its spacing
            // (or half a mesh cell) per substep.
            let mut resolve = 0.5 * s.spacing;
            if let Some(m) = &self.mesh {
                resolve = resolve.min(0.5 * m.d.min_element());
            }
            let v = ex.point_v.max(thermal_sigma(s.params.temperature, s.real_mass) * 3.0);
            if v > 0.0 {
                dt = dt.min(resolve / v);
            }
            let a = if s.charged { s.q_over_m.abs() * e_drive } else { 0.0 }.max(ex.point_a);
            if a > 0.0 {
                dt = dt.min((resolve / a).sqrt());
            }
        }
        if !dt.is_finite() {
            dt = 1.0 / 240.0;
        }
        dt.max(f32::MIN_POSITIVE)
    }

    /// Advance by `seconds` of simulated time in stable substeps, at most
    /// `max_substeps` of them (then the run falls behind: see
    /// `stats.realtime_ratio`). Returns the substeps taken.
    pub fn advance(&mut self, seconds: f64) -> u32 {
        if seconds <= 0.0 {
            return 0;
        }
        let limit = if self.params.timestep > 0.0 { self.params.timestep } else { self.stable_timestep() };
        let needed = (seconds / limit as f64).ceil().max(1.0);
        let cap = self.params.max_substeps.max(1) as f64;
        let (n, dt) = if needed <= cap {
            (needed as u32, (seconds / needed) as f32)
        } else {
            (cap as u32, limit)
        };
        for _ in 0..n {
            self.step(dt);
        }
        self.compute_stats(seconds, n as f64 * dt as f64, n);
        n
    }

    /// The substep the next step takes: `Timestep` when set, otherwise the
    /// stability limit of the current state.
    pub fn substep_size(&self) -> f32 {
        if self.params.timestep > 0.0 { self.params.timestep } else { self.stable_timestep() }
    }

    /// Real-time stepping. `seconds` more simulated time is owed to the
    /// clock and paid in whole stable substeps until less than one substep
    /// is owed, `max_substeps` were taken, or `budget` of wall time is
    /// spent. With `must_progress` the budget is checked only after the
    /// first substep, so a run whose one substep outcosts its budget still
    /// moves (hosts pass it on the first call of a frame). What a call
    /// cannot pay is forgiven, so a run too heavy for real time plays in
    /// slow motion (`stats.realtime_ratio` below 1) instead of every later
    /// frame getting slower. Substep sizes never depend on frame timing, so
    /// a run replays the same trajectory at any frame rate. `budget` None
    /// pays the whole debt. Returns the substeps taken.
    pub fn advance_realtime(
        &mut self,
        seconds: f64,
        budget: Option<std::time::Duration>,
        must_progress: bool,
    ) -> u32 {
        let start = std::time::Instant::now();
        let requested = seconds.max(0.0);
        self.owed += requested;
        let cap = self.params.max_substeps.max(1);
        let mut n = 0u32;
        let mut achieved = 0.0f64;
        let mut forgiven = 0.0f64;
        loop {
            let dt = self.substep_size();
            if self.owed < dt as f64 {
                break;
            }
            let over_budget = budget.is_some_and(|b| start.elapsed() >= b);
            if n >= cap || (over_budget && (n > 0 || !must_progress)) {
                forgiven = self.owed;
                self.owed = 0.0;
                break;
            }
            self.step(dt);
            self.owed -= dt as f64;
            achieved += dt as f64;
            n += 1;
        }
        let previous = self.stats.realtime_ratio;
        if n > 0 {
            self.compute_stats(requested, achieved, n);
        } else {
            self.stats.substeps = 0;
        }
        if requested > 0.0 {
            let kept = ((requested - forgiven) / requested).clamp(0.0, 1.0) as f32;
            self.stats.realtime_ratio = previous + 0.1 * (kept - previous);
        } else {
            self.stats.realtime_ratio = previous;
        }
        n
    }

    /// One substep of `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        self.dfsph_iterations = 0;
        if !self.pos.is_empty() {
            let has_fluid = self.fluid_kernel.is_some() && self.species.iter().any(|s| s.fluid);
            let dfsph = has_fluid && self.params.fluid_solver == FluidSolver::Dfsph;
            if has_fluid {
                if self.boundary_dirty {
                    if let (Some(k), Some(s)) = (self.fluid_kernel, self.fluid_spacing()) {
                        self.boundary.rebuild(&self.params, &self.obstacles, s, k);
                    }
                    self.boundary_dirty = false;
                }
                self.bin_and_sort();
                self.compute_density(dfsph);
                if dfsph {
                    // DFSPH (Bender and Koschier 2017, algorithm 1): make
                    // the velocity field divergence-free before forces act.
                    self.divergence_solve(dt);
                }
            }
            match self.params.electrostatics {
                Electrostatics::Off => self.efield.iter_mut().for_each(|e| *e = Vec3::ZERO),
                Electrostatics::Direct => self.direct_field(),
                Electrostatics::Mesh => self.mesh_field(),
            }
            if has_fluid {
                self.compute_fluid_acceleration(dt, !dfsph);
            } else {
                self.acc.iter_mut().for_each(|a| *a = Vec3::ZERO);
            }
            if dfsph {
                // Non-pressure forces first, then the pressure that keeps
                // the predicted density at rest density; only then move.
                self.kick_fluids(dt);
                self.dfsph_iterations = self.pressure_solve(dt);
            }
            self.push(dt, dfsph);
            self.scatter(dt);
            self.apply_boundaries();
        }
        self.emit(dt);
        self.time += dt as f64;
        self.steps += 1;
    }

    fn periodic(&self) -> [bool; 3] {
        [
            self.params.boundary[0] == Boundary::Periodic,
            self.params.boundary[1] == Boundary::Periodic,
            self.params.boundary[2] == Boundary::Periodic,
        ]
    }

    fn bin_and_sort(&mut self) {
        let h = self.fluid_kernel.map(|k| k.h).unwrap_or(1.0);
        let origin = -self.params.half();
        let size = self.params.domain_size;
        let periodic = self.periodic();
        let order = self.grid.build(&self.pos, origin, size, h, periodic);
        let identity = order.iter().enumerate().all(|(i, &o)| i as u32 == o);
        if !identity {
            permute(&mut self.pos, &order);
            permute(&mut self.vel, &order);
            permute(&mut self.sp, &order);
            permute(&mut self.density, &order);
            permute(&mut self.pressure, &order);
            permute(&mut self.efield, &order);
            permute(&mut self.acc, &order);
            permute(&mut self.warm_kappa, &order);
        }
    }

    /// SPH density of every fluid particle (fluid and boundary neighbours)
    /// and its neighbour list. DFSPH also caches V_j grad W_ij per listed
    /// neighbour for its iterations; WCSPH also sets the Tait pressure.
    fn compute_density(&mut self, dfsph: bool) {
        let Some(k) = self.fluid_kernel else { return };
        let h = k.h;
        let h2 = h * h;
        let cap = self.nb_cap;
        let n = self.pos.len();
        self.nb.resize(n * cap, 0);
        self.nb_len.resize(n, 0);
        let (grid, pos, sp, species) = (&self.grid, &self.pos, &self.sp, &self.species);
        let bnd = &self.boundary;
        // `dfsph_out`: the neighbour gradients V_j grad W_ij and the
        // stiffness factor alpha_i = 1 / (|sum V grad W|^2 + sum_j |V_j grad W_ij|^2)
        // with boundary neighbours in the first sum (DFSPH eq. 9), found in
        // the same scan.
        let body = |i: usize,
                    rho: &mut f32,
                    pr: &mut f32,
                    slots: &mut [u32],
                    len: &mut u16,
                    fluid_len: &mut u16,
                    mut dfsph_out: Option<(&mut [Vec3], &mut f32)>| {
            let si = &species[sp[i] as usize];
            if !si.fluid {
                *rho = 0.0;
                *pr = 0.0;
                *len = 0;
                *fluid_len = 0;
                if let Some((_, alpha)) = dfsph_out {
                    *alpha = 0.0;
                }
                return;
            }
            let xi = pos[i];
            let mut d = 0.0f32;
            let mut count = 0usize;
            let mut overflow = false;
            let mut grad_sum = Vec3::ZERO;
            let mut sq_sum = 0.0f32;
            grid.for_each_candidate(xi, |j| {
                let sj = &species[sp[j] as usize];
                if !sj.fluid {
                    return;
                }
                let rv = grid.delta(xi, pos[j]);
                let r2 = rv.length_squared();
                if r2 >= h2 {
                    return;
                }
                let r = r2.sqrt();
                d += sj.m * k.w(r);
                if j == i {
                    return;
                }
                let gv = if dfsph && r2 > 1e-12 * h2 {
                    rv * (sj.m / sj.params.rest_density * k.dw(r) / r)
                } else {
                    Vec3::ZERO
                };
                grad_sum += gv;
                sq_sum += gv.length_squared();
                if count == slots.len() {
                    overflow = true;
                    return;
                }
                slots[count] = j as u32;
                if let Some((gvs, _)) = dfsph_out.as_mut() {
                    gvs[count] = gv;
                }
                count += 1;
            });
            *fluid_len = count as u16;
            let rho0 = si.params.rest_density;
            if !bnd.is_empty() {
                let mut wsum = 0.0f32;
                bnd.grid.for_each_candidate(xi, |b| {
                    let rv = bnd.grid.delta(xi, bnd.pos[b]);
                    let r2 = rv.length_squared();
                    if r2 >= h2 {
                        return;
                    }
                    let r = r2.sqrt();
                    wsum += bnd.vol[b] * k.w(r);
                    if !dfsph || r2 <= 1e-12 * h2 {
                        return;
                    }
                    let gv = rv * (bnd.vol[b] * k.dw(r) / r);
                    grad_sum += gv;
                    // Listed after the fluid neighbours for the solver passes.
                    if count == slots.len() {
                        overflow = true;
                        return;
                    }
                    slots[count] = b as u32;
                    if let Some((gvs, _)) = dfsph_out.as_mut() {
                        gvs[count] = gv;
                    }
                    count += 1;
                });
                d += rho0 * wsum;
            }
            *len = if overflow { NB_OVERFLOW } else { count as u16 };
            *rho = d.max(1e-3 * rho0);
            if let Some((_, alpha)) = dfsph_out {
                let denom = grad_sum.length_squared() + sq_sum;
                *alpha = if denom > 0.0 { 1.0 / denom } else { 0.0 };
            }
            *pr = if dfsph {
                // Set by the pressure solve.
                0.0
            } else {
                let ratio = *rho / rho0;
                let r7 = ratio * ratio * ratio * ratio * ratio * ratio * ratio;
                (si.tait_b * (r7 - 1.0)).max(0.0)
            };
        };
        self.nb_fluid_len.resize(n, 0);
        if dfsph {
            self.nb_gv.resize(n * cap, Vec3::ZERO);
            self.alpha.resize(n, 0.0);
            self.density
                .par_iter_mut()
                .zip(self.pressure.par_iter_mut())
                .zip(self.nb.par_chunks_mut(cap))
                .zip(self.nb_len.par_iter_mut())
                .zip(self.nb_fluid_len.par_iter_mut())
                .zip(self.nb_gv.par_chunks_mut(cap))
                .zip(self.alpha.par_iter_mut())
                .enumerate()
                .with_min_len(PAR_MIN_SCAN)
                .for_each(|(i, ((((((rho, pr), slots), len), fluid_len), gv), alpha))| {
                    body(i, rho, pr, slots, len, fluid_len, Some((gv, alpha)))
                });
        } else {
            self.nb_gv = Vec::new();
            self.density
                .par_iter_mut()
                .zip(self.pressure.par_iter_mut())
                .zip(self.nb.par_chunks_mut(cap))
                .zip(self.nb_len.par_iter_mut())
                .zip(self.nb_fluid_len.par_iter_mut())
                .enumerate()
                .with_min_len(PAR_MIN_SCAN)
                .for_each(|(i, ((((rho, pr), slots), len), fluid_len))| {
                    body(i, rho, pr, slots, len, fluid_len, None)
                });
        }
    }

    fn fluid_pairs(&self) -> Option<FluidPairs<'_>> {
        Some(FluidPairs {
            nb: Neighbours { idx: &self.nb, len: &self.nb_len, fluid_len: &self.nb_fluid_len, cap: self.nb_cap },
            gv: &self.nb_gv,
            grid: &self.grid,
            pos: &self.pos,
            sp: &self.sp,
            species: &self.species,
            kernel: self.fluid_kernel?,
        })
    }

    // ------------------------------------------------------------------
    // DFSPH (Bender and Koschier, "Divergence-Free SPH for Incompressible
    // and Viscous Fluids", 2017). Densities and quantities are divided by
    // each particle's rest density; V_j = m_j / rho0_j.
    // ------------------------------------------------------------------

    /// Per fluid particle, the density change the velocities imply:
    /// `dt` None gives the rate (1/s) for the divergence solve, clamped at
    /// 0 and zeroed at free surfaces; `dt` Some gives the density ratio
    /// predicted after `dt` for the pressure solve, clamped at 1 (fluid
    /// never pulls itself together). Also sets each particle's stiffness
    /// k_i = (b_i - offset) alpha_i scale for the next velocity pass.
    /// Returns the mean error.
    fn density_change_pass(&mut self, dt: Option<f32>, scale: f32, offset: f32) -> f32 {
        let Some(k) = self.fluid_kernel else { return 0.0 };
        let n = self.pos.len();
        let mut rho_adv = std::mem::take(&mut self.rho_adv);
        let mut kfac = std::mem::take(&mut self.kfac);
        rho_adv.resize(n, 0.0);
        kfac.resize(n, 0.0);
        let (sum, count) = match self.fluid_pairs() {
            None => (0.0, 0),
            Some(pairs) => {
                let bpairs = BoundaryPairs { nb: pairs.nb, gv: &self.nb_gv, pos: &self.pos, bnd: &self.boundary, kernel: k };
                let (vel, density, alpha, sp, species, bnd) =
                    (&self.vel, &self.density, &self.alpha, &self.sp, &self.species, &self.boundary);
                let deficient = self.nb_deficient;
                let parts: Vec<(f64, u32)> = rho_adv
                    .par_chunks_mut(CHUNK)
                    .zip(kfac.par_chunks_mut(CHUNK))
                    .enumerate()
                    .with_min_len(PAR_MIN_PARTICLES / CHUNK)
                    .map(|(c, (chunk, kchunk))| {
                        let mut sum = 0.0f64;
                        let mut n = 0u32;
                        for (kk, (out, kf)) in chunk.iter_mut().zip(kchunk.iter_mut()).enumerate() {
                            let i = c * CHUNK + kk;
                            let si = &species[sp[i] as usize];
                            if !si.fluid {
                                *out = 0.0;
                                *kf = 0.0;
                                continue;
                            }
                            let vi = vel[i];
                            let mut div = 0.0f32;
                            let mut neighbours = 0u32;
                            pairs.for_each(i, |j, gv| {
                                div += (vi - vel[j]).dot(gv);
                                neighbours += 1;
                            });
                            bpairs.for_each(i, |b, gv| {
                                div += (vi - bnd.vel[b]).dot(gv);
                                neighbours += 1;
                            });
                            let (value, err) = match dt {
                                Some(dt) => {
                                    let v = (density[i] / si.params.rest_density + dt * div).max(1.0);
                                    (v, v - 1.0)
                                }
                                None => {
                                    let v = if neighbours < deficient { 0.0 } else { div.max(0.0) };
                                    (v, v)
                                }
                            };
                            *out = value;
                            *kf = (value - offset) * alpha[i] * scale;
                            sum += err as f64;
                            n += 1;
                        }
                        (sum, n)
                    })
                    .collect();
                parts.iter().fold((0.0f64, 0u32), |(s, n), (a, b)| (s + a, n + b))
            }
        };
        self.rho_adv = rho_adv;
        self.kfac = kfac;
        if count == 0 { 0.0 } else { (sum / count as f64) as f32 }
    }

    /// One Jacobi update of a DFSPH solve with the stiffness k the last
    /// density pass set:
    /// v_i -= dt [sum_j (k_i + rho0_j / rho0_i k_j) V_j grad W_ij + k_i sum_b V_b grad W_ib]
    /// (eqs. 12 and 14). Boundary terms hand the equal and opposite impulse
    /// to obstacles; `accumulate` sums the applied k into the pressure field.
    fn pressure_velocity_pass(&mut self, dt: f32, accumulate: bool) {
        let Some(k) = self.fluid_kernel else { return };
        let n = self.pos.len();
        let mut vel = std::mem::take(&mut self.vel);
        let mut kappa_sum = std::mem::take(&mut self.kappa_sum);
        kappa_sum.resize(n, 0.0);
        let reactions: Vec<Vec<BodyImpulse>> = match self.fluid_pairs() {
            None => Vec::new(),
            Some(pairs) => {
                let bpairs = BoundaryPairs { nb: pairs.nb, gv: &self.nb_gv, pos: &self.pos, bnd: &self.boundary, kernel: k };
                let (sp, species, bnd, obstacles, kfac) =
                    (&self.sp, &self.species, &self.boundary, &self.obstacles, &self.kfac);
                vel.par_chunks_mut(CHUNK)
                    .zip(kappa_sum.par_chunks_mut(CHUNK))
                    .enumerate()
                    .with_min_len(PAR_MIN_PARTICLES / CHUNK)
                    .map(|(c, (chunk, kappa_chunk))| {
                        let mut reaction = vec![BodyImpulse::default(); obstacles.len()];
                        for (kk, (v, kappa)) in chunk.iter_mut().zip(kappa_chunk.iter_mut()).enumerate() {
                            let i = c * CHUNK + kk;
                            let si = &species[sp[i] as usize];
                            if !si.fluid || !si.params.mobile {
                                continue;
                            }
                            let ki = kfac[i];
                            if accumulate {
                                *kappa += ki;
                            }
                            let rho0_i = si.params.rest_density;
                            let mut dv = Vec3::ZERO;
                            pairs.for_each(i, |j, gv| {
                                let sj = &species[sp[j] as usize];
                                dv -= gv * (ki + sj.params.rest_density / rho0_i * kfac[j]);
                            });
                            if ki != 0.0 {
                                bpairs.for_each(i, |b, gv| {
                                    let db = gv * -ki;
                                    dv += db;
                                    let owner = bnd.owner[b];
                                    if owner != WALL {
                                        if let (Some(f), Some(o)) =
                                            (reaction.get_mut(owner as usize), obstacles.get(owner as usize))
                                        {
                                            // The body gains the momentum the particle lost.
                                            f.add(-db * (dt * si.m), bnd.pos[b], o.center);
                                        }
                                    }
                                });
                            }
                            *v += dv * dt;
                        }
                        reaction
                    })
                    .collect()
            }
        };
        self.vel = vel;
        self.kappa_sum = kappa_sum;
        for reaction in reactions {
            for (imp, f) in self.obstacle_impulse.iter_mut().zip(reaction) {
                *imp += f;
            }
        }
    }

    /// Correct velocities until the density stops changing: the mean rate
    /// within `DFSPH_DIVERGENCE_TOLERANCE` per substep. Returns iterations.
    fn divergence_solve(&mut self, dt: f32) -> u32 {
        let eta = DFSPH_DIVERGENCE_TOLERANCE / dt;
        let mut err = self.density_change_pass(None, 1.0 / dt, 0.0);
        let mut iterations = 0;
        while iterations < DFSPH_MAX_ITERATIONS && err > eta {
            self.pressure_velocity_pass(dt, false);
            err = self.density_change_pass(None, 1.0 / dt, 0.0);
            iterations += 1;
        }
        iterations
    }

    /// Constant-density solve on the predicted velocities: iterate (at
    /// least twice) until the mean predicted compression is within
    /// `DFSPH_DENSITY_TOLERANCE`. Rest density times the summed stiffness
    /// is the pressure field. Returns iterations.
    fn pressure_solve(&mut self, dt: f32) -> u32 {
        let n = self.pos.len();
        let scale = 1.0 / (dt * dt);
        // Warm start: the pressure the last substep ended with is most of
        // this one's. Half of it is applied first, so the start can only
        // fall short (iterations add the rest) and never overshoot into an
        // expansion the one-sided solve could not take back.
        self.warm_kappa.resize(n, 0.0);
        self.kappa_sum.clear();
        self.kappa_sum.extend(self.warm_kappa.iter().map(|w| w * scale * DFSPH_WARM_START));
        self.kfac.clear();
        self.kfac.extend_from_slice(&self.kappa_sum);
        if self.kfac.iter().any(|&k| k != 0.0) {
            self.pressure_velocity_pass(dt, false);
        }
        let mut err = self.density_change_pass(Some(dt), scale, 1.0);
        let mut iterations = 0;
        while iterations < DFSPH_MAX_ITERATIONS && (iterations < 2 || err > DFSPH_DENSITY_TOLERANCE) {
            self.pressure_velocity_pass(dt, true);
            err = self.density_change_pass(Some(dt), scale, 1.0);
            iterations += 1;
        }
        let (kappa_sum, sp, species) = (&self.kappa_sum, &self.sp, &self.species);
        let dt2 = dt * dt;
        self.pressure
            .par_iter_mut()
            .zip(self.warm_kappa.par_iter_mut())
            .enumerate()
            .with_min_len(PAR_MIN_PARTICLES)
            .for_each(|(i, (p, warm))| {
                let si = &species[sp[i] as usize];
                *p = if si.fluid { (si.params.rest_density * kappa_sum[i]).max(0.0) } else { 0.0 };
                *warm = if si.fluid { kappa_sum[i] * dt2 } else { 0.0 };
            });
        iterations
    }

    /// v* = v + dt a for fluid particles (non-pressure forces, gravity,
    /// fields); the pressure solve corrects v* before anything moves.
    fn kick_fluids(&mut self, dt: f32) {
        let g = self.params.gravity;
        let b = self.params.magnetic_field;
        let e_uniform = self.uniform_field();
        let (acc, efield, sp, species) = (&self.acc, &self.efield, &self.sp, &self.species);
        self.vel.par_iter_mut().enumerate().with_min_len(PAR_MIN_PARTICLES).for_each(|(i, v)| {
            let s = &species[sp[i] as usize];
            if !s.fluid || !s.params.mobile {
                return;
            }
            let mut a = acc[i] + g;
            if s.charged {
                a += (e_uniform + efield[i]) * s.q_over_m;
            }
            *v = kick(*v, a, s, b, dt);
        });
    }

    /// SPH forces on fluid particles: pressure (WCSPH only; DFSPH solves it
    /// separately), laminar and artificial viscosity, surface tension, and
    /// wall and obstacle contact, whose reactions go to the obstacles.
    fn compute_fluid_acceleration(&mut self, dt: f32, with_pressure: bool) {
        let Some(k) = self.fluid_kernel else { return };
        let h = k.h;
        let h2 = h * h;
        let eta2 = 0.01 * h2;
        let alpha = self.params.artificial_viscosity;
        let sigma_st = self.params.surface_tension;
        let obstacles = &self.obstacles;
        let (grid, pos, vel, sp, species, density, pressure) =
            (&self.grid, &self.pos, &self.vel, &self.sp, &self.species, &self.density, &self.pressure);
        let nb = Neighbours { idx: &self.nb, len: &self.nb_len, fluid_len: &self.nb_fluid_len, cap: self.nb_cap };
        let bnd = &self.boundary;
        // Per chunk: the force and torque the fluid puts on each obstacle,
        // summed afterwards in chunk order (deterministic).
        let reactions: Vec<Vec<BodyImpulse>> = self
            .acc
            .par_chunks_mut(CHUNK)
            .enumerate()
            .map(|(c, chunk)| {
                let mut reaction = vec![BodyImpulse::default(); obstacles.len()];
                for (k_local, acc) in chunk.iter_mut().enumerate() {
                    let i = c * CHUNK + k_local;
                    *acc = fluid_acceleration(
                        i, k, h2, eta2, alpha, sigma_st, with_pressure, grid, nb, pos, vel, sp, species, density,
                        pressure, bnd, obstacles, &mut reaction,
                    );
                }
                reaction
            })
            .collect();
        for reaction in reactions {
            for (imp, f) in self.obstacle_impulse.iter_mut().zip(reaction) {
                imp.linear += f.linear * dt;
                imp.angular += f.angular * dt;
            }
        }
    }
}

/// SPH acceleration of fluid particle `i` from fluid neighbours and boundary
/// particles; adds the equal and opposite force on obstacles to `reaction`.
#[allow(clippy::too_many_arguments)]
#[inline]
fn fluid_acceleration(
    i: usize,
    k: CubicSpline,
    h2: f32,
    eta2: f32,
    alpha: f32,
    sigma_st: f32,
    with_pressure: bool,
    grid: &CellGrid,
    nb: Neighbours,
    pos: &[Vec3],
    vel: &[Vec3],
    sp: &[u16],
    species: &[SpeciesRuntime],
    density: &[f32],
    pressure: &[f32],
    bnd: &BoundarySet,
    obstacles: &[Obstacle],
    reaction: &mut [BodyImpulse],
) -> Vec3 {
    let h = k.h;
    let si = &species[sp[i] as usize];
    if !si.fluid {
        return Vec3::ZERO;
    }
    let xi = pos[i];
    let vi = vel[i];
    let rho_i = density[i];
    let pi_term = if with_pressure { pressure[i] / (rho_i * rho_i) } else { 0.0 };
    let mu_i = si.params.viscosity;
    let mut a = Vec3::ZERO;
    let mut normal = Vec3::ZERO;
    let mut lap = 0.0f32;
    let mut visit = |j: usize| {
        if j == i {
            return;
        }
        let sj = &species[sp[j] as usize];
        if !sj.fluid {
            return;
        }
        let rv = grid.delta(xi, pos[j]);
        let r2 = rv.length_squared();
        if r2 >= h2 || r2 <= 1e-12 * h2 {
            return;
        }
        let r = r2.sqrt();
        let grad = rv * (k.dw(r) / r);
        let rho_j = density[j];
        let mj = sj.m;
        if with_pressure {
            a -= grad * (mj * (pi_term + pressure[j] / (rho_j * rho_j)));
        }
        let vij = vi - vel[j];
        let vr = vij.dot(rv);
        if alpha > 0.0 && vr < 0.0 {
            let mu = h * vr / (r2 + eta2);
            let c = 0.5 * (si.c0 + sj.c0);
            let visc = -alpha * c * mu / (0.5 * (rho_i + rho_j));
            a -= grad * (mj * visc);
        }
        let mu_sum = mu_i + sj.params.viscosity;
        if mu_sum > 0.0 {
            a += vij * (mj * mu_sum / (rho_i * rho_j) * rv.dot(grad) / (r2 + eta2));
        }
        if sigma_st > 0.0 {
            let vol = mj / rho_j;
            normal += grad * vol;
            lap += vol * k.laplacian(r);
        }
    };
    match nb.of(i) {
        Some(list) => list.iter().for_each(|&j| visit(j as usize)),
        None => grid.for_each_candidate(xi, &mut visit),
    }
    if sigma_st > 0.0 {
        let n = normal.length();
        if n > 0.1 / h {
            a -= normal * (sigma_st * lap / (rho_i * n));
        }
    }
    // Boundary particles: pressure mirrored from this particle, plus
    // artificial viscosity against the surface velocity (friction).
    if !bnd.is_empty() {
        let rho0 = si.params.rest_density;
        bnd.grid.for_each_candidate(xi, |b| {
            let rv = bnd.grid.delta(xi, bnd.pos[b]);
            let r2 = rv.length_squared();
            if r2 >= h2 || r2 <= 1e-12 * h2 {
                return;
            }
            let r = r2.sqrt();
            let grad = rv * (k.dw(r) / r);
            let psi = rho0 * bnd.vol[b];
            // Akinci et al. 2012, eq. 10: the particle's own p/rho^2 once.
            // Mirroring the pressure (2x) over-repels where the free
            // surface meets a wall and keeps a resting tank jittering.
            let mut ab = -grad * (psi * pi_term);
            let vr = (vi - bnd.vel[b]).dot(rv);
            if alpha > 0.0 && vr < 0.0 {
                let mu = h * vr / (r2 + eta2);
                let visc = -alpha * si.c0 * mu / rho_i;
                ab -= grad * (psi * visc);
            }
            a += ab;
            let owner = bnd.owner[b];
            if owner != WALL {
                if let (Some(f), Some(o)) = (reaction.get_mut(owner as usize), obstacles.get(owner as usize)) {
                    // Equal and opposite, applied where the boundary particle sits.
                    f.add(-ab * si.m, bnd.pos[b], o.center);
                }
            }
        });
    }
    a
}

impl ParticleSim {
    fn direct_field(&mut self) {
        let periodic = self.periodic();
        let size = self.params.domain_size;
        let min_spacing = self
            .species
            .iter()
            .filter(|s| s.charged)
            .map(|s| s.spacing)
            .fold(f32::INFINITY, f32::min);
        let eps = if min_spacing.is_finite() { self.params.softening * min_spacing } else { 0.0 };
        let eps2 = eps * eps;
        let sources: Vec<(Vec3, f32)> = self
            .pos
            .iter()
            .zip(&self.sp)
            .filter_map(|(p, &s)| {
                let s = &self.species[s as usize];
                s.charged.then_some((*p, s.q))
            })
            .collect();
        let (pos, sp, species) = (&self.pos, &self.sp, &self.species);
        self.efield.par_iter_mut().enumerate().for_each(|(i, e)| {
            let s = &species[sp[i] as usize];
            if !s.charged || !s.params.mobile {
                *e = Vec3::ZERO;
                return;
            }
            let xi = pos[i];
            let mut acc = Vec3::ZERO;
            for &(xj, qj) in &sources {
                let d = min_image(xi - xj, size, periodic);
                let r2 = d.length_squared();
                if r2 == 0.0 {
                    continue;
                }
                let inv = 1.0 / (r2 + eps2);
                acc += d * (qj * inv * inv.sqrt());
            }
            *e = acc * COULOMB_K;
        });
    }

    fn mesh_bc(&self) -> [AxisBc; 3] {
        let p = &self.params;
        let mut bc = [AxisBc::Neumann; 3];
        for a in 0..3 {
            bc[a] = if p.boundary[a] == Boundary::Periodic {
                AxisBc::Periodic
            } else if a == p.voltage_axis.min(2) && p.applied_voltage != 0.0 {
                AxisBc::Dirichlet { lo: 0.0, hi: p.applied_voltage }
            } else {
                match p.wall_potential {
                    WallPotential::Grounded => AxisBc::Dirichlet { lo: 0.0, hi: 0.0 },
                    WallPotential::Insulating => AxisBc::Neumann,
                }
            };
        }
        bc
    }

    fn mesh_field(&mut self) {
        let bc = self.mesh_bc();
        let rebuild = match &self.mesh {
            Some(m) => m.bc != bc,
            None => true,
        };
        if rebuild {
            self.mesh = Some(MeshField::new(
                -self.params.half(),
                self.params.domain_size,
                self.params.grid_resolution,
                bc,
            ));
        }
        let charges: Vec<f32> = self.sp.iter().map(|&s| self.species[s as usize].q).collect();
        let background = if self.params.neutralizing_background {
            let total: f64 = charges.iter().map(|&q| q as f64).sum();
            (-total / self.params.volume()) as f32
        } else {
            0.0
        };
        let mesh = self.mesh.as_mut().expect("mesh built above");
        mesh.deposit(&self.pos, &charges, background);
        let max_it = (8 * mesh.cells.iter().copied().max().unwrap_or(16)) as u32 + 50;
        mesh.solve(1e-4, max_it);
        let mesh = &*mesh;
        let (pos, sp, species) = (&self.pos, &self.sp, &self.species);
        self.efield.par_iter_mut().enumerate().for_each(|(i, e)| {
            let s = &species[sp[i] as usize];
            *e = if s.charged && s.params.mobile { mesh.field_at(pos[i]) } else { Vec3::ZERO };
        });
    }

    /// The uniform field applied on top of the self-consistent one. With
    /// mesh electrostatics and electrodes on a non-periodic voltage axis the
    /// Poisson solve already carries the voltage, so only the applied
    /// `electric_field` remains.
    pub fn uniform_field(&self) -> Vec3 {
        let p = &self.params;
        let axis = p.voltage_axis.min(2);
        let electrodes = p.electrostatics == Electrostatics::Mesh && p.boundary[axis] != Boundary::Periodic;
        if electrodes {
            p.electric_field
        } else {
            p.drive_field()
        }
    }

    /// Kick and drift (Boris in a magnetic field). With `fluids_kicked`
    /// (DFSPH), fluid particles already carry their pressure-corrected
    /// velocity and only drift.
    fn push(&mut self, dt: f32, fluids_kicked: bool) {
        let g = self.params.gravity;
        let b = self.params.magnetic_field;
        let e_uniform = self.uniform_field();
        let axis = self.params.voltage_axis.min(2);
        let (acc, efield, sp, species) = (&self.acc, &self.efield, &self.sp, &self.species);
        let parts: Vec<(f64, MotionExtremes)> = self
            .pos
            .par_chunks_mut(CHUNK)
            .zip(self.vel.par_chunks_mut(CHUNK))
            .enumerate()
            .with_min_len(PAR_MIN_PARTICLES / CHUNK)
            .map(|(c, (pc, vc))| {
                let base = c * CHUNK;
                let mut cross = 0.0f64;
                let mut ex = MotionExtremes::default();
                for k in 0..pc.len() {
                    let i = base + k;
                    let s = &species[sp[i] as usize];
                    if !s.params.mobile {
                        continue;
                    }
                    let mut a = acc[i] + g;
                    if s.charged {
                        a += (e_uniform + efield[i]) * s.q_over_m;
                    }
                    let v = if s.fluid && fluids_kicked { vc[k] } else { kick(vc[k], a, s, b, dt) };
                    let x0 = pc[k];
                    let x1 = x0 + v * dt;
                    if s.charged {
                        if x0[axis] < 0.0 && x1[axis] >= 0.0 {
                            cross += s.q as f64;
                        } else if x0[axis] >= 0.0 && x1[axis] < 0.0 {
                            cross -= s.q as f64;
                        }
                    }
                    vc[k] = v;
                    pc[k] = x1;
                    let (sv, sa) = (v.length(), a.length());
                    if s.fluid {
                        ex.fluid_v = ex.fluid_v.max(sv);
                        ex.fluid_a = ex.fluid_a.max(sa);
                    } else {
                        ex.point_v = ex.point_v.max(sv);
                        ex.point_a = ex.point_a.max((a - g).length());
                    }
                }
                (cross, ex)
            })
            .collect();
        let mut ex = MotionExtremes::default();
        for (cross, e) in parts {
            self.plane_charge += cross;
            ex.fluid_v = ex.fluid_v.max(e.fluid_v);
            ex.fluid_a = ex.fluid_a.max(e.fluid_a);
            ex.point_v = ex.point_v.max(e.point_v);
            ex.point_a = ex.point_a.max(e.point_a);
        }
        self.plane_time += dt as f64;
        self.extremes = ex;
    }

    fn scatter(&mut self, dt: f32) {
        let t_lat = self.lattice_temperature.max(0.0);
        let per_species: Vec<(f32, f32)> = self
            .species
            .iter()
            .map(|s| {
                if s.params.collision_time > 0.0 && s.params.mobile {
                    // p = dt/(tau+dt) makes the discrete steady state of
                    // "kick, then maybe scatter" drift at exactly (q/m) E tau,
                    // the continuous Drude value, at any dt/tau.
                    (dt / (s.params.collision_time + dt), thermal_sigma(t_lat, s.real_mass))
                } else {
                    (0.0, 0.0)
                }
            })
            .collect();
        if per_species.iter().all(|(p, _)| *p == 0.0) {
            return;
        }
        let seed = self.params.seed;
        let step = self.steps;
        let (sp, species) = (&self.sp, &self.species);
        let dumped: f64 = self
            .vel
            .par_chunks_mut(CHUNK)
            .enumerate()
            .with_min_len(PAR_MIN_PARTICLES / CHUNK)
            .map(|(c, vc)| {
                let base = c * CHUNK;
                let mut de = 0.0f64;
                for k in 0..vc.len() {
                    let i = base + k;
                    let si = sp[i] as usize;
                    let (p, sigma) = per_species[si];
                    if p == 0.0 || counter_uniform(seed, step, i as u64, 0) >= p {
                        continue;
                    }
                    let old = vc[k];
                    let new = Vec3::new(
                        counter_normal(seed, step, i as u64, 1),
                        counter_normal(seed, step, i as u64, 2),
                        counter_normal(seed, step, i as u64, 3),
                    ) * sigma;
                    de += 0.5 * species[si].m as f64 * (old.length_squared() - new.length_squared()) as f64;
                    vc[k] = new;
                }
                de
            })
            .collect::<Vec<f64>>()
            .iter()
            .sum();
        self.lattice_energy += dumped;
        if self.params.joule_heating && self.params.lattice_heat_capacity > 0.0 {
            let dt_lat = dumped / (self.params.lattice_heat_capacity as f64 * self.params.volume());
            self.lattice_temperature = (self.lattice_temperature as f64 + dt_lat).max(0.0) as f32;
        }
    }

    fn apply_boundaries(&mut self) {
        let p = &self.params;
        let half = p.half();
        let size = p.domain_size;
        let bnd = p.boundary;
        let e = p.restitution.clamp(0.0, 1.0);
        let fr = p.wall_friction.clamp(0.0, 1.0);
        let obstacles = &self.obstacles;
        let nobs = obstacles.len();
        let (sp, species) = (&self.sp, &self.species);
        let parts: Vec<(Vec<u32>, f64, Vec<BodyImpulse>)> = self
            .pos
            .par_chunks_mut(CHUNK)
            .zip(self.vel.par_chunks_mut(CHUNK))
            .enumerate()
            .with_min_len(PAR_MIN_PARTICLES / CHUNK)
            .map(|(c, (pc, vc))| {
                let base = c * CHUNK;
                let mut dead = Vec::new();
                let mut absorbed = 0.0f64;
                let mut impulses = vec![BodyImpulse::default(); nobs];
                for k in 0..pc.len() {
                    let i = base + k;
                    let s = &species[sp[i] as usize];
                    let mut x = pc[k];
                    let mut v = vc[k];
                    let mut gone = false;
                    for a in 0..3 {
                        let (lo, hi) = (-half[a], half[a]);
                        if x[a] >= lo && x[a] <= hi {
                            continue;
                        }
                        match bnd[a] {
                            Boundary::Periodic => {
                                x[a] = lo + (x[a] - lo).rem_euclid(size[a]);
                                if x[a] >= hi {
                                    x[a] = lo;
                                }
                            }
                            Boundary::Reflect => {
                                if x[a] < lo {
                                    x[a] = (lo + (lo - x[a])).min(hi);
                                    if v[a] < 0.0 {
                                        v[a] = -e * v[a];
                                    }
                                } else {
                                    x[a] = (hi - (x[a] - hi)).max(lo);
                                    if v[a] > 0.0 {
                                        v[a] = -e * v[a];
                                    }
                                }
                                for b in 0..3 {
                                    if b != a {
                                        v[b] *= 1.0 - fr;
                                    }
                                }
                            }
                            Boundary::Absorb => {
                                gone = true;
                            }
                        }
                    }
                    if gone {
                        dead.push(i as u32);
                        absorbed += s.q as f64;
                        continue;
                    }
                    if nobs > 0 && s.params.mobile {
                        // Fluids feel obstacles through boundary particles; this
                        // contact is only the no-penetration backstop.
                        let margin = 0.0f32;
                        for (oi, o) in obstacles.iter().enumerate() {
                            if (x - o.center).length() > o.bounding_radius() + margin {
                                continue;
                            }
                            let d = o.sdf(x);
                            if d >= margin {
                                continue;
                            }
                            let n = o.normal(x, 1e-3 * o.bounding_radius().max(1e-12));
                            if n == Vec3::ZERO {
                                continue;
                            }
                            let before = v;
                            x += n * (margin - d);
                            let mut rel = v - o.velocity;
                            let vn = rel.dot(n);
                            if vn < 0.0 {
                                rel -= n * ((1.0 + e) * vn);
                                let vt = rel - n * rel.dot(n);
                                rel -= vt * fr;
                            }
                            v = rel + o.velocity;
                            impulses[oi].add(-(v - before) * s.m, x, o.center);
                        }
                    }
                    pc[k] = x;
                    vc[k] = v;
                }
                (dead, absorbed, impulses)
            })
            .collect();
        let mut dead_all = Vec::new();
        for (dead, absorbed, impulses) in parts {
            dead_all.extend(dead);
            self.absorbed_charge += absorbed;
            for (acc, imp) in self.obstacle_impulse.iter_mut().zip(impulses) {
                *acc += imp;
            }
        }
        if !dead_all.is_empty() {
            self.remove(&dead_all);
        }
    }

    /// Remove particles by index (ascending), keeping the others in order.
    fn remove(&mut self, dead: &[u32]) {
        let mut keep = vec![true; self.pos.len()];
        for &d in dead {
            keep[d as usize] = false;
        }
        retain_by(&mut self.pos, &keep);
        retain_by(&mut self.vel, &keep);
        retain_by(&mut self.sp, &keep);
        retain_by(&mut self.density, &keep);
        retain_by(&mut self.pressure, &keep);
        retain_by(&mut self.efield, &keep);
        retain_by(&mut self.acc, &keep);
        retain_by(&mut self.warm_kappa, &keep);
    }

    fn emit(&mut self, dt: f32) {
        let cap = self.params.max_particles as usize;
        for si in 0..self.species.len() {
            let s = &self.species[si];
            let rate = s.params.emission_rate;
            if rate <= 0.0 {
                continue;
            }
            self.emission_accum[si] += rate as f64 * dt as f64;
            let n = self.emission_accum[si].floor();
            self.emission_accum[si] -= n;
            let (lo, hi) = s.region;
            let shape = s.params.region_shape;
            let drift = s.params.drift_velocity;
            let sigma = if s.fluid { 0.0 } else { thermal_sigma(s.params.temperature, s.real_mass) };
            for _ in 0..n as usize {
                if self.pos.len() >= cap {
                    break;
                }
                let p = sample_region(&mut self.rng, lo, hi, shape);
                let v = drift + self.rng.normal3() * sigma;
                self.push_particle(si as u16, p, v);
            }
        }
    }

    // ------------------------------------------------------------------
    // Measurement
    // ------------------------------------------------------------------

    fn compute_stats(&mut self, requested: f64, achieved: f64, substeps: u32) {
        let volume = self.params.volume();
        let mut st = SimStats {
            particle_count: self.pos.len() as u32,
            sim_time: self.time,
            timestep: if substeps > 0 { (achieved / substeps as f64) as f32 } else { self.stats.timestep },
            substeps,
            realtime_ratio: if requested > 0.0 { (achieved / requested) as f32 } else { 1.0 },
            // Written by the host once per frame.
            solver_ms: self.stats.solver_ms,
            lattice_temperature: self.lattice_temperature,
            lattice_energy: self.lattice_energy,
            absorbed_charge: self.absorbed_charge,
            poisson_iterations: self.mesh.as_ref().map(|m| m.iterations).unwrap_or(0),
            pressure_iterations: self.dfsph_iterations,
            applied_field: self.params.drive_field(),
            ..Default::default()
        };
        let ns = self.species.len();
        let mut count = vec![0u32; ns];
        let mut vsum = vec![[0.0f64; 3]; ns];
        let mut v2sum = vec![0.0f64; ns];
        let mut j = [0.0f64; 3];
        let mut max_speed = 0.0f32;
        let mut rho_sum = 0.0f64;
        let mut rho_n = 0u32;
        let mut compress = 0.0f64;
        for i in 0..self.pos.len() {
            let si = self.sp[i] as usize;
            let s = &self.species[si];
            let v = self.vel[i];
            count[si] += 1;
            for a in 0..3 {
                vsum[si][a] += v[a] as f64;
            }
            v2sum[si] += v.length_squared() as f64;
            max_speed = max_speed.max(v.length());
            if s.charged {
                for a in 0..3 {
                    j[a] += s.q as f64 * v[a] as f64;
                }
            }
            if s.fluid {
                rho_sum += self.density[i] as f64;
                rho_n += 1;
                compress += ((self.density[i] / s.params.rest_density) - 1.0).max(0.0) as f64;
            }
        }
        st.max_speed = max_speed;
        let mut t_weighted = 0.0f64;
        let mut t_count = 0u64;
        let mut wp2 = 0.0f64;
        let mut inv_debye2 = 0.0f64;
        for si in 0..ns {
            let s = &self.species[si];
            let n = count[si];
            let mut ss = SpeciesStats {
                name: s.params.name.clone(),
                count: n,
                real_count: n as f64 * s.weight,
                ..Default::default()
            };
            if n > 0 {
                let mean = [vsum[si][0] / n as f64, vsum[si][1] / n as f64, vsum[si][2] / n as f64];
                ss.drift_velocity = Vec3::new(mean[0] as f32, mean[1] as f32, mean[2] as f32);
                ss.kinetic_energy = 0.5 * s.m as f64 * v2sum[si];
                if !s.fluid {
                    let mean2 = mean[0] * mean[0] + mean[1] * mean[1] + mean[2] * mean[2];
                    let thermal = (v2sum[si] / n as f64 - mean2).max(0.0);
                    let t = s.real_mass * thermal / (3.0 * constants::K_B);
                    ss.temperature = t as f32;
                    if s.params.mobile {
                        t_weighted += t * n as f64;
                        t_count += n as u64;
                    }
                }
                let n_real = ss.real_count / volume;
                if s.charged && s.params.mobile {
                    ss.plasma_frequency = presets::plasma_frequency(n_real, s.params.charge, s.real_mass);
                    wp2 += ss.plasma_frequency * ss.plasma_frequency;
                    let t = (ss.temperature as f64).max(1e-3);
                    inv_debye2 += n_real * s.params.charge * s.params.charge / (constants::EPSILON_0 * constants::K_B * t);
                    if s.params.collision_time > 0.0 {
                        ss.drude_conductivity = presets::drude_conductivity(
                            n_real,
                            s.params.charge,
                            s.params.collision_time as f64,
                            s.real_mass,
                        );
                    }
                }
            }
            st.kinetic_energy += ss.kinetic_energy;
            st.drude_conductivity += ss.drude_conductivity;
            st.species.push(ss);
        }
        st.temperature = if t_count > 0 { (t_weighted / t_count as f64) as f32 } else { 0.0 };
        st.plasma_frequency = wp2.sqrt();
        st.debye_length = if inv_debye2 > 0.0 { 1.0 / inv_debye2.sqrt() } else { 0.0 };
        if rho_n > 0 {
            st.mean_density = (rho_sum / rho_n as f64) as f32;
            st.density_error = (compress / rho_n as f64) as f32;
        }

        // Time-averaged current density: an exponential average over about
        // 30 frames, reset-free because it starts from zero with the run.
        let j_now = [j[0] / volume, j[1] / volume, j[2] / volume];
        let alpha = if substeps == 0 { 0.0 } else { 1.0 / 30.0 };
        for a in 0..3 {
            self.j_ema[a] += alpha * (j_now[a] - self.j_ema[a]);
        }
        st.current_density = Vec3::new(self.j_ema[0] as f32, self.j_ema[1] as f32, self.j_ema[2] as f32);
        if self.plane_time > 0.0 {
            let i_now = self.plane_charge / self.plane_time;
            self.current_ema += alpha * (i_now - self.current_ema);
            self.plane_charge = 0.0;
            self.plane_time = 0.0;
        }
        st.current = self.current_ema;
        let e = self.params.drive_field();
        let e2 = e.length_squared() as f64;
        let je = self.j_ema[0] * e.x as f64 + self.j_ema[1] * e.y as f64 + self.j_ema[2] * e.z as f64;
        if e2 > 0.0 {
            st.conductivity = je / e2;
            st.resistivity = if st.conductivity.abs() > 0.0 { 1.0 / st.conductivity } else { 0.0 };
        }
        st.joule_power_density = je;
        self.stats = st;
    }

    /// Mean velocity of a species from the last stats (for temperature colouring).
    fn species_drift(&self, si: usize) -> Vec3 {
        self.stats.species.get(si).map(|s| s.drift_velocity).unwrap_or(Vec3::ZERO)
    }

    /// Scalar a colour mode maps (None for "not applicable to this particle").
    fn scalar(&self, mode: ColorMode, i: usize, e_uniform: Vec3) -> Option<f32> {
        let si = self.sp[i] as usize;
        let s = &self.species[si];
        match mode {
            ColorMode::Species => None,
            ColorMode::Speed => Some(self.vel[i].length()),
            ColorMode::Temperature => {
                if s.fluid {
                    None
                } else {
                    let dv = self.vel[i] - self.species_drift(si);
                    Some((s.real_mass * dv.length_squared() as f64 / (3.0 * constants::K_B)) as f32)
                }
            }
            ColorMode::Charge => Some(s.params.charge as f32),
            ColorMode::Density => s.fluid.then(|| self.density[i]),
            ColorMode::Pressure => s.fluid.then(|| self.pressure[i]),
            ColorMode::Field => s.charged.then(|| (self.efield[i] + e_uniform).length()),
        }
    }

    /// Fill `out` with one instance per particle. `radii` is the display
    /// radius per species (world m). `range` fixes the colour range; `None`
    /// picks the 2nd..98th percentile. Returns the range used.
    pub fn write_instances(
        &self,
        mode: ColorMode,
        range: Option<(f32, f32)>,
        radii: &[f32],
        out: &mut Vec<ParticleInstance>,
    ) -> (f32, f32) {
        let n = self.pos.len();
        out.clear();
        out.reserve(n);
        let e_uniform = self.uniform_field();
        let range = match (mode, range) {
            (ColorMode::Species, _) => (0.0, 1.0),
            (ColorMode::Charge, _) => {
                let m = self
                    .species
                    .iter()
                    .map(|s| s.params.charge.abs() as f32)
                    .fold(0.0f32, f32::max)
                    .max(f32::MIN_POSITIVE);
                (-m, m)
            }
            (_, Some(r)) if r.1 > r.0 => r,
            _ => self.auto_range(mode, e_uniform),
        };
        let span = (range.1 - range.0).max(f32::MIN_POSITIVE);
        let neutral = [0.35, 0.35, 0.38, 1.0];
        out.par_extend((0..n).into_par_iter().map(|i| {
            let si = self.sp[i] as usize;
            let s = &self.species[si];
            let color = match mode {
                ColorMode::Species => s.params.color,
                ColorMode::Charge => {
                    if s.params.charge == 0.0 {
                        neutral
                    } else {
                        coolwarm(s.params.charge as f32 / range.1)
                    }
                }
                _ => match self.scalar(mode, i, e_uniform) {
                    Some(v) => viridis((v - range.0) / span),
                    None => neutral,
                },
            };
            let p = self.pos[i];
            ParticleInstance {
                position: [p.x, p.y, p.z],
                radius: radii.get(si).copied().unwrap_or(0.01),
                color,
            }
        }));
        range
    }

    fn auto_range(&self, mode: ColorMode, e_uniform: Vec3) -> (f32, f32) {
        let n = self.pos.len();
        let stride = (n / 20_000).max(1);
        let mut vals: Vec<f32> = (0..n)
            .step_by(stride)
            .filter_map(|i| self.scalar(mode, i, e_uniform))
            .filter(|v| v.is_finite())
            .collect();
        if vals.is_empty() {
            return (0.0, 1.0);
        }
        let lo_i = (vals.len() as f32 * 0.02) as usize;
        let hi_i = ((vals.len() as f32 * 0.98) as usize).min(vals.len() - 1);
        let cmp = |a: &f32, b: &f32| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal);
        let lo = *vals.select_nth_unstable_by(lo_i, cmp).1;
        let hi = *vals.select_nth_unstable_by(hi_i, cmp).1;
        if hi > lo {
            (lo, hi)
        } else {
            (lo, lo + lo.abs().max(1e-6))
        }
    }

    /// Up to `max` particles, evenly strided: (species, position, velocity).
    pub fn sample(&self, max: usize) -> Vec<(u16, Vec3, Vec3)> {
        let n = self.pos.len();
        if n == 0 || max == 0 {
            return Vec::new();
        }
        let stride = n.div_ceil(max).max(1);
        (0..n).step_by(stride).map(|i| (self.sp[i], self.pos[i], self.vel[i])).collect()
    }

    /// Electric potential at a domain point (Mesh electrostatics only).
    pub fn potential_at(&self, p: Vec3) -> Option<f32> {
        self.mesh.as_ref().map(|m| m.potential_at(p))
    }
}

/// Kernel sum at a site of an infinite cubic lattice of spacing `s`.
fn lattice_kernel_sum(k: CubicSpline, s: f32) -> f32 {
    let reach = (k.h / s).ceil() as i32;
    let mut sum = 0.0f32;
    for i in -reach..=reach {
        for j in -reach..=reach {
            for l in -reach..=reach {
                let r = s * ((i * i + j * j + l * l) as f32).sqrt();
                sum += k.w(r);
            }
        }
    }
    sum.max(f32::MIN_POSITIVE)
}

/// One support radius for all fluids: the widest the species ask for.
fn fluid_kernel(sim: &SimParams, species: &[SpeciesRuntime]) -> Option<CubicSpline> {
    let h = species
        .iter()
        .filter(|s| s.fluid)
        .map(|s| sim.kernel_radius_ratio.max(1.2) * s.spacing)
        .fold(0.0f32, f32::max);
    (h > 0.0).then(|| CubicSpline::new(h))
}

fn initial_conditions_differ(a: &SpeciesParams, b: &SpeciesParams) -> bool {
    a.name != b.name
        || a.kind != b.kind
        || a.count != b.count
        || a.number_density != b.number_density
        || a.temperature != b.temperature
        || a.drift_velocity != b.drift_velocity
        || a.region_shape != b.region_shape
        || a.region_min != b.region_min
        || a.region_max != b.region_max
        || a.arrangement != b.arrangement
        || a.mobile != b.mobile
        || (a.kind == SpeciesKind::Fluid && a.rest_density != b.rest_density)
}

/// Standard deviation of one velocity component at temperature T for mass m.
#[inline]
fn thermal_sigma(temperature: f32, mass: f64) -> f32 {
    if temperature <= 0.0 || mass <= 0.0 {
        return 0.0;
    }
    (constants::K_B * temperature as f64 / mass).sqrt() as f32
}

/// Rotate v about B by the Boris scheme; `t` = (q/m) B dt/2.
#[inline]
fn boris_rotate(v: Vec3, t: Vec3) -> Vec3 {
    let s = t * (2.0 / (1.0 + t.length_squared()));
    let v_prime = v + v.cross(t);
    v + v_prime.cross(s)
}

#[inline]
fn min_image(mut d: Vec3, size: Vec3, periodic: [bool; 3]) -> Vec3 {
    for a in 0..3 {
        if periodic[a] {
            let l = size[a];
            if d[a] > 0.5 * l {
                d[a] -= l;
            } else if d[a] < -0.5 * l {
                d[a] += l;
            }
        }
    }
    d
}

fn permute<T: Copy>(v: &mut Vec<T>, order: &[u32]) {
    let sorted: Vec<T> = order.iter().map(|&i| v[i as usize]).collect();
    *v = sorted;
}

fn retain_by<T: Copy>(v: &mut Vec<T>, keep: &[bool]) {
    let mut w = 0;
    for r in 0..v.len() {
        if keep[r] {
            v[w] = v[r];
            w += 1;
        }
    }
    v.truncate(w);
}

fn sample_region(rng: &mut SimRng, lo: Vec3, hi: Vec3, shape: RegionShape) -> Vec3 {
    let ext = hi - lo;
    match shape {
        RegionShape::Box => lo + Vec3::new(rng.uniform(), rng.uniform(), rng.uniform()) * ext,
        RegionShape::Sphere => loop {
            let u = Vec3::new(rng.uniform(), rng.uniform(), rng.uniform()) * 2.0 - Vec3::ONE;
            if u.length_squared() <= 1.0 {
                break lo + (u * 0.5 + Vec3::splat(0.5)) * ext;
            }
        },
    }
}

/// Up to `want` lattice points at `spacing` inside the region, bottom layer
/// first (so a partial fill leaves a flat free surface on top). When that
/// many do not fit at `spacing`, the lattice tightens until they do; the
/// spacing actually used is returned with the points.
fn lattice_points(lo: Vec3, hi: Vec3, spacing: f32, want: usize, shape: RegionShape) -> (Vec<Vec3>, f32) {
    let ext = (hi - lo).max(Vec3::splat(f32::MIN_POSITIVE));
    let fill = |s: f32| -> Vec<Vec3> {
        let n = [
            ((ext.x / s + 1e-4).floor() as usize).max(1),
            ((ext.y / s + 1e-4).floor() as usize).max(1),
            ((ext.z / s + 1e-4).floor() as usize).max(1),
        ];
        let mut pts = Vec::with_capacity(want);
        let offset = (ext - Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32) * s) * 0.5 + Vec3::splat(0.5 * s);
        'outer: for y in 0..n[1] {
            for z in 0..n[2] {
                for x in 0..n[0] {
                    let p = lo + offset + Vec3::new(x as f32, y as f32, z as f32) * s;
                    if shape == RegionShape::Sphere {
                        let u = (p - lo) / ext * 2.0 - Vec3::ONE;
                        if u.length_squared() > 1.0 {
                            continue;
                        }
                    }
                    pts.push(p);
                    if pts.len() == want {
                        break 'outer;
                    }
                }
            }
        }
        pts
    };
    // Largest spacing (at most the nominal one) whose lattice holds `want`
    // points: bisection, since thin regions lose whole rows to rounding.
    let nominal = spacing.max(f32::MIN_POSITIVE);
    let first = fill(nominal);
    if first.len() == want {
        return (first, nominal);
    }
    let (mut lo_s, mut hi_s) = (nominal * 1e-3, nominal);
    for _ in 0..40 {
        let mid = 0.5 * (lo_s + hi_s);
        if fill(mid).len() == want {
            lo_s = mid;
        } else {
            hi_s = mid;
        }
    }
    (fill(lo_s), lo_s)
}

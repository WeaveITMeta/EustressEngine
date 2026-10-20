# Particle Simulation

The `ParticleSimulation` class is a physical particle simulation you place in a
Space like any other instance. Its `ParticleSpecies` children are the particle
populations: water, oil or mercury as SPH fluids; electrons, positrons, protons
and ions as charged point particles; argon as a neutral gas. One domain can mix
them.

The same solver covers four regimes:

| Regime | Model | What emerges |
|---|---|---|
| Fluids | Divergence-free SPH (DFSPH) or weakly compressible SPH (Tait equation of state), artificial and laminar viscosity, CSF surface tension, Akinci boundary particles | Hydrostatic pressure, splashing, flow around parts, buoyant push on bodies |
| Charged particles | Boris push in uniform E and B fields | Gyration, drift, beams |
| Electrostatics | Direct softened Coulomb, or particle-in-cell (cloud-in-cell deposit, conjugate-gradient Poisson, electrodes) | Plasma oscillation, Debye screening, space charge, Hall voltage |
| Conduction | Drude scattering against a lattice at a set temperature | Ohm's law, sigma = n e^2 tau / m, Joule heating, hot electrons |

Everything is SI: metres, seconds, kilograms, coulombs, kelvin, volts, tesla.

## Using it

**Insert.** Model tab, Effects group, **Particle Sim**; or Insert Object,
category Simulation. A new simulation arrives with one species (a block of
water) at rest in its initial state; press Play to run it (see Play and Stop).
Insert a `ParticleSpecies` with the simulation selected to add another
population.

**Explorer.** The simulation is a folder instance (`_instance.toml` with a
`[particle_simulation]` section); each species is a folder inside it with a
`[particle_species]` section. Both are file-natured: they never collapse into
binary cores, and they survive reload, copy and paste like any folder.

**Properties.** Every field shows in its category, with units in the tooltip.
Choice fields (boundaries, particle type, colour mode, conductor) are
dropdowns. A **Runtime** section at the bottom shows live measurements,
refreshed about four times a second. Edits are undoable. A field marked as an
initial condition (domain size, counts, regions, temperature, seed) restarts
the run; everything else applies live.

**Scale.** `DomainSize` is the physical box. `DisplayScale` maps it into the
world: a 20 nm copper conductor (`DomainSize = 2e-8, 2e-8, 2e-8`) shows as a
2 m box with `DisplayScale = 1e8`. `TimeScale` is simulated seconds per real
second: 1 for a water tank, about 1e-13 for electrons in a metal.

**Performance.** The solver picks each substep from the stability limits
(fluid CFL and force limits, viscous limit, plasma and cyclotron frequencies,
collision time, particle travel per step) and pays each frame's simulated time
in whole substeps. `FrameBudget` (6 ms by default) caps the solver's wall time
per frame and `MaxSubsteps` its substeps; what a frame cannot pay is dropped,
so a heavy run plays in slow motion instead of the frame rate falling. Runtime
shows `RealtimeRatio` (the share of `TimeScale` the run keeps up with) and
`SolverTime` (milliseconds used last frame). Fluids are the costly case. The
default 1 m tank of 2000 water particles (DFSPH) takes about 3.3 substeps of
5 ms per frame and about 7 ms of wall time per frame on 8 cores; 4000
particles take about 12 ms. With `FluidSolver` WCSPH the same tank needs about
19 substeps of 0.9 ms per frame, roughly five times the work. More particles or
a smaller `FrameBudget` play in slow motion, as `RealtimeRatio` shows. The
solver is compiled optimised in every build, dev builds included: unoptimised
it runs about five times slower, and a single substep would outlast the budget.

### Recipes

| Goal | Simulation | Species |
|---|---|---|
| Water tank | defaults | defaults (Water) |
| Copper wire, Ohm's law | `DomainSize` 2e-8 on each axis, `DisplayScale` 1e8, `TimeScale` 1e-13, all boundaries Periodic, `Gravity` 0, `ElectricField` 1e7, 0, 0 | `Particle` Electron, `Conductor` Copper, `Count` 20000, `Arrangement` Random, `RegionMax` 1, 1, 1 |
| Joule heating | the copper wire plus `JouleHeating` on, `LatticeHeatCapacity` 3.45e6 | as above |
| Plasma oscillation | Periodic boundaries, `Electrostatics` Mesh, `Gravity` 0 | Electron, `NumberDensity` 1e18, `Temperature` 0, Lattice |
| Electron beam | `BoundaryX` Absorb, `Gravity` 0 | Electron, `Count` 0, `EmissionRate` > 0, `DriftVelocity` along X, a thin region at the X min face |
| Hall voltage | the copper wire with `MagneticField` 0, 0, 1, `BoundaryY` Reflect, `Electrostatics` Mesh, `WallPotential` Insulating | as above |

Read `Conductivity` against `DrudeConductivity` in Runtime: the first is
measured from the particles (J.E / E^2), the second is n q^2 tau / m.

### Runtime control

Every simulation publishes its measurements as sim values and accepts writes
to its properties, so Rune scripts, MCP and data bindings drive it with the
existing tools:

| Key | Meaning |
|---|---|
| `psim.<Simulation>.<Stat>` | a Runtime measurement, for example `psim.Tank.MaxSpeed` |
| `psim.<Simulation>.<Species>.<Stat>` | a species measurement, for example `psim.Wire.Electrons.DriftSpeed` |
| `psim.<Simulation>.<Property>` | write a bool, integer or float property live |
| `psim.<Simulation>.<Property>.x` (`.y`, `.z`) | write one component of a vector property |
| `psim.<Simulation>.Reset` | write 1 to restart the run |

From Rune: `set_sim_value("psim.Wire.AppliedVoltage", 0.2)`,
`get_sim_value("psim.Wire.Conductivity")`. From MCP: `set_sim_value` and
`get_sim_value` with the same keys.

The MCP tool `particle_simulation` authors simulations on disk:
`action = "describe"` lists every property with its type, unit and default,
plus presets; `create` makes a simulation and its species; `set` and `get`
edit and read properties by path (`"Tank"`, `"Tank/Water"`). The engine
hot-reloads each edit.

### Play and Stop

Simulations run on the world clock: they step while physics does (Play, a
runtime, a scripted `sim.step`) and hold in Edit, where each shows its initial
state, so a setup can be arranged and inspected before it runs. Changing an
initial condition (counts, regions, arrangement, domain) redraws that state at
once. Play starts every simulation from its authored state. Edits made during
Play (from Properties, scripts or MCP) apply live but are not saved; Stop
restores the authored properties and returns each run to its initial state.
Pause freezes every simulation where it is, and `Running` off holds one even
in Play.

### Parts in the domain

With `CollideWithParts` on, every collidable part overlapping the domain is an
obstacle (boxes, balls and cylinders exactly; other shapes by their box).
Fluids feel them through boundary particles; point particles bounce off their
surfaces. With `PushParts` on, unanchored parts receive the force and torque
the particles exert, so a floating box is pushed by the water and turns in a
current.

## Physics

### Fluids

Density is the SPH sum with the cubic spline kernel of support `KernelRadius`
particle spacings, plus the boundary term of Akinci et al. (2012): walls and
parts are sampled into boundary particles with volume V_b = 1 / sum_k W_bk,
contributing rho0 V_b W_ib. Non-pressure accelerations are Monaghan
artificial viscosity, the laminar viscosity of Morris et al. (1997) with the
species' dynamic viscosity, continuum surface force tension and gravity.

`FluidSolver` picks how pressure is found:

- **DFSPH** (default; Bender and Koschier 2017). Every substep first makes the
  velocity field divergence-free, then applies the other forces, then solves
  for the pressure that brings the predicted density back to rest density
  (mean error under 0.05%, at least two Jacobi iterations, the solve starting
  from half the previous substep's pressure). Pressure never pulls, so a free
  surface is left alone. Substeps are set by the flow: nothing crosses 0.4 of
  a particle spacing per substep, capped at 5 ms.
- **WCSPH**. Pressure follows the Tait equation of state with gamma = 7,
  clamped at zero, and a speed of sound ten times the expected flow speed
  (about 1% compression); substeps are set by that speed.

Boundary pressure acts on the particle alone (Akinci's term), and the equal
and opposite impulse of each boundary term is the push on that part.

Particle mass is set so that the lattice the particles are actually placed on
sums to exactly the rest density (a count that does not fit the region at the
nominal spacing tightens the lattice, and the mass follows), and fluid is
placed one spacing off reflecting walls. Before the first substep the fluid is
relaxed: pressure-only moves, with no gravity and no inertia, until the
densest particle is within 0.5% of rest density, because a lattice cut off by
a wall starts a few percent dense against it. A block therefore starts at rest
instead of popping.

### Charged particles and fields

Point particles are pushed with the Boris scheme (half kick, rotation in B,
half kick), which conserves speed in a pure magnetic field. The electric field
at each particle is the uniform applied field plus the self-consistent field:

- **Direct**: softened Coulomb sum over all charges (minimum image across
  periodic faces). Exact; O(N^2), for a few thousand particles.
- **Mesh**: charges deposited on a node grid by cloud-in-cell weights, the
  Poisson equation solved by conjugate gradients (finite volume, periodic,
  Dirichlet or Neumann faces, warm-started), and the field gathered back with
  the same weights. With electrodes, the voltage is a Dirichlet condition on
  the axis faces.

`AppliedVoltage` puts +V on the max face of `VoltageAxis`. Periodic along that
axis, it acts as an EMF around the loop (a uniform V / L).

### Conduction

Each species with a `CollisionTime` scatters against a lattice at
`LatticeTemperature`: every substep a particle collides with probability
dt / (tau + dt), which makes the discrete steady-state drift exactly
(q / m) E tau at any timestep, and takes a fresh Maxwell-Boltzmann velocity.
The energy each collision removes goes to the lattice; with `JouleHeating` on
it raises the lattice temperature through `LatticeHeatCapacity`, otherwise the
lattice is an ideal heat sink.

The conductor presets use conduction-electron densities and 273 K relaxation
times from Ashcroft and Mermin, *Solid State Physics*, Tables 1.1 and 1.3,
which reproduce the measured conductivities of copper, silver, gold,
aluminium, iron and sodium within 6%.

### Macro-particles

A species' `NumberDensity` is the real particle density. Each simulated
particle stands for `NumberDensity x region volume / Count` real ones and
carries their total charge and mass, so q / m, densities, currents and
conductivities stay physical whatever `Count` is.

### Determinism

Every random draw (placement, thermal velocities, scattering, emission) derives
from the Space's `GlobalRngSeed` plus the simulation's `Seed`, through
counter-based generators in the parallel loops. Reductions run in a fixed
chunk order. The same Space and properties replay the same run on any number
of threads. Substep sizes depend only on the state, never on the frame rate,
so a slow frame or a spent `FrameBudget` changes how fast a run plays, not its
trajectory; scripted steps (`sim.step`) ignore the budget, so each tick pays
its simulated time in full.

## Verified behaviour

The solver ships with acceptance tests that compare emergent quantities with
their analytic values (`particle-sim/src/physics_tests.rs`; run them with
`cargo test -p eustress-particle-sim --release`):

| Test | Checks | Tolerance |
|---|---|---|
| `drude_conductivity_emerges_for_copper` | measured conductivity vs n e^2 tau / m (6.44e7 S/m); electron temperature vs T_L + m v_d^2 / 3 k_B | 3%, 5% |
| `reported_conductivity_tracks_drude` | the Runtime `Conductivity` stat | 5% |
| `joule_heating_conserves_energy` | field work vs lattice heat plus electron kinetic energy; lattice temperature rise | 3%, 0.1% |
| `plasma_oscillates_at_plasma_frequency` | mesh electrostatics rings at sqrt(n e^2 / eps0 m) | 3% |
| `electron_gyrates_in_magnetic_field` | orbit diameter, closure after one period, speed | 1%, 0.01% |
| `coulomb_pair_conserves_energy` | kinetic plus Coulomb energy of two electrons | 0.1% |
| `neutral_gas_temperature_and_energy` | Maxwell-Boltzmann initial temperature; energy in an elastic box | 2%, 0.01% |
| `absorbed_beam_current_matches_emission` | mid-plane current of an absorbed beam vs rate x q | 5% |
| `water_column_settles_hydrostatically` | DFSPH: energy decays, compression, interior dp/dy vs rho g | 0.5%, 20% |
| `wcsph_water_column_settles_hydrostatically` | the same with WCSPH | 2%, 20% |
| `dfsph_dam_break_stays_incompressible` | a collapsing column never gains energy, stays incompressible while flowing, and settles at the depth its volume gives | 1%, 35% |
| `dfsph_substeps_follow_the_flow` | DFSPH substeps are several times WCSPH's for the same tank | more than 4x |
| `obstacle_supports_fluid_and_reports_impulse` | force on a box under water vs the water's weight; no spurious torque | 25% |
| `deterministic_across_thread_counts` | identical trajectories on 1 and 4 threads | bit for bit |
| `slow_motion_replays_the_same_trajectory` | a run held to one substep per frame walks through the same states as an unhurried one, and reports falling behind | bit for bit |
| `realtime_run_keeps_up` | with room to spare, the simulated clock tracks the requested time | one substep |

The Poisson solver is checked against a parallel-plate capacitor and a
periodic sinusoidal charge, and the kernel against its analytic integral and
derivative. In `eustress-common`, the class tables round-trip every property
through TOML and panel text; the Bevy integration is tested for spawning, live
edits, restarts, pausing, stepping, resetting, holding the initial state while
the host's clock is stopped, publishing solver time, and holding a spent frame
budget to one substep per frame; and the solver's physical constants are
checked against the engine's.

## Fidelity notes

- The electron gas is classical. Its heat capacity is 3/2 n k_B, about 100
  times the real (Sommerfeld) value, so in a small domain the electrons absorb
  a visible share of Joule heat. Conductivity is unaffected.
- With electrostatics Off, electrons do not repel: this is the Drude picture,
  where the ion lattice screens them. Mesh electrostatics at metal density
  must resolve the plasma frequency (about 1.6e16 rad/s for copper), which
  forces substeps near 1e-17 s.
- SPH pressure in the layer touching a wall or part reads up to twice the
  hydrostatic value: with Akinci's one-sided wall term that layer carries the
  column's weight alone. Interior pressure follows rho g h.
- DFSPH holds mean compression near 0.05% but carries no sound: pressure
  reaches the whole fluid within a substep. WCSPH keeps sound waves (at the
  artificial speed of sound, not water's 1480 m/s) and trades about 1%
  compression for explicit time stepping.
- The starting relaxation moves fluid particles by a fraction of a spacing,
  so a block settles against its walls before the run instead of during it.
- Fluid with fewer neighbours than about a third of an interior particle (a
  thin sheet or spray) is treated as free surface by the divergence solve.
- Obstacles other than boxes, balls and cylinders use their bounding box.
  With `DisplayScale` other than 1, momentum handed to parts is scaled by
  length; pushing parts is meaningful at true scale.
- The simulation runs on the CPU (rayon). `realism::gpu` is not used.

## Architecture

| Piece | Where |
|---|---|
| Solver (SoA state, stepping, SPH, Boris, Drude, boundaries, emission, stats) | `particle-sim/src/solver.rs` (crate `eustress-particle-sim`) |
| Boundary particles | `particle-sim/src/boundary.rs` |
| Particle-mesh Poisson | `particle-sim/src/poisson.rs` |
| Neighbour grid, deterministic RNG, colour maps | `particle-sim/src/{grid,rng,colormap}.rs` |
| SPH kernel (`CubicSpline`) | `particle-sim/src/kernel.rs` |
| Particle and conductor presets, physical constants | `particle-sim/src/{presets,constants}.rs` |
| Runtime stats tables | `particle-sim/src/diagnostics.rs` |
| Class components and field tables (TOML, panel text, validation) | `common/src/realism/particle_sim/class.rs` |
| Bevy runtime (FixedUpdate stepping within the frame budget, commands, render buffer) | `common/src/realism/particle_sim/plugin.rs`, registered by `RealismPlugin` |
| Templates | `common/assets/class_schema/{ParticleSimulation,ParticleSpecies}/_instance.toml` |
| Engine bridge (spawn, save, hot reload, obstacles, coupling, world clock, Play/Stop, sim values) | `engine/src/particles/bridge.rs`, registered with the core plugins (editor and headless) |
| Renderer (instanced sphere impostors) | `engine/src/particles/render.rs`, `engine/assets/shaders/particle_cloud.wgsl` |
| Properties rows, edits, live Runtime rows | `engine/src/ui/particle_sim_panel.rs` |
| MCP tool | `tools/src/particle_sim_tools.rs` |

The solver is a crate of its own, depending only on `bevy_math`, rayon and
bytemuck, and `eustress_common::realism::particle_sim` re-exports its modules,
so `particle_sim::solver::ParticleSim` and the other paths resolve there. Being
separate lets `.cargo/config.toml` compile it optimised in dev builds (the rest
of the workspace compiles at opt-level 0 there), and lets its physics tests run
without building the engine.

## What Eustress requires of a simulation class

These are the contracts the rest of the engine assumes; the class meets each.

1. **File-system first.** The instance is its `_instance.toml`; every property
   round-trips through its section, readable and diffable. Writes go to the
   WorldDb when one is active and to the disk mirror.
2. **One creation path.** Insert, the ribbon, the Insert Object dialog and MCP
   all go through `instance_create::create_instance` and the class template.
3. **One inspector.** Properties is the only editor: typed rows, dropdowns,
   units, undo, validation with a message instead of a silent drop.
4. **Metres and SI.** Engine units are metres; the class stores SI throughout
   and maps scale explicitly (`DisplayScale`, `TimeScale`).
5. **The world clock decides.** A simulation advances only while the world
   does (Play, a runtime, a scripted step) and shows its initial state in
   Edit. Play-time changes never persist; Stop returns the authored state and
   the runs restart.
6. **Determinism.** Randomness derives from `GlobalRngSeed`; results do not
   depend on thread count.
7. **Observable.** Measurements reach sim values, so watchpoints, recordings,
   telemetry, data bindings, scripts and MCP all see them.
8. **Frame budget.** Work per frame is bounded (`FrameBudget` wall time,
   `MaxSubsteps`, `MaxParticles`); a run that cannot keep up slows itself,
   never the editor, and reports it (`RealtimeRatio`, `SolverTime`).
9. **Avian is the physics engine.** Parts are obstacles through Avian's spatial
   queries, and momentum goes back through Avian's forces.
10. **Headless parity.** The simulation and its bridge run in the headless
    runner with the same results as the editor.
11. **Honest fidelity.** Every model states what it leaves out (see Fidelity
    notes) and is tested against an analytic result.

# Eustress Engine — Universal STEM Stack
# Gap Analysis, Architecture, and Phased Build Plan

**Purpose:** Make Eustress Engine capable of simulating any phenomenon in science
and engineering through built-in kernel laws — so nothing ever needs to be
"approximated away" or left as magic numbers.

**Guiding principle:** Every law in this stack is **dimensionally correct**,
**derivable from first principles**, and **composable** with every other law
in the same codebase.  The V-Cell battery, the ARC-1 reactor, a rocket engine,
a living cell, and a structural beam can all run in the same simulation, exchange
heat, exchange charge, exchange force — and never contradict each other.

---

## 1 — What Exists Today

### ✅ Implemented (93 files, ~1 900 exported symbols) — last updated 2026-06-03

**Phase A–D shipped** (commit `3df9bf58`, on `main`). 36 new files, 10 197 lines added.

| Domain | Module(s) | Coverage |
|--------|-----------|----------|
| Physical constants | `constants.rs` | Universal (G, c, h, k_B, R, N_A), EM (ε₀, μ₀, e, COULOMB_K), Atmospheric, Water, Na-S battery, Sc-NASICON, V-Cell materials, ΔH_f / ΔG_f tables (12 compounds), Ka values (6 acids), WIEN_CONSTANT, BOHR_RADIUS, SOLAR_CONSTANT |
| SI units + conversions | `units.rs` | 30 unit newtypes, 60+ conversions — adds Tesla, Henries, Farads, Ohms, Volts, Coulombs, Siemens, Gray (+rad), Sievert (+rem), Becquerel (+Ci), Lumen, Lux (+fc) |
| Classical mechanics | `laws/mechanics.rs` | F=ma, kinematics, energy, momentum, rotation, moment-of-inertia, gravity, Kepler, friction, springs |
| Thermodynamics | `laws/thermodynamics.rs` | Ideal + van der Waals gas, 1st/2nd/3rd law, heat transfer (Fourier/Newton/Stefan-Boltzmann), Carnot, phase transitions, Gibbs/Helmholtz |
| Conservation | `laws/conservation.rs` | Mass, energy, linear/angular momentum; Bernoulli; ConservationTracker |
| Electrochemistry | `laws/electrochemistry.rs` | Nernst, Butler-Volmer, Tafel, Ohmic, ionic transport (Arrhenius/Nernst-Einstein/Nernst-Planck), heat generation, dendrite risk, Peukert |
| **Electromagnetism** ✨ | `laws/electromagnetism/` | Coulomb, Gauss, electric field/potential, Biot-Savart, Lorentz force, energy density, Faraday EMF, solenoid/toroid/mutual inductance, transformer V/I/Z/η, skin depth, Poynting, Fresnel, Friis, dBm↔W |
| **Circuit laws** ✨ | `laws/electromagnetism/circuits.rs` | Ohm, series/parallel R/L/C, RC/RL/RLC time constants + Q, transient equations, AC reactance/impedance/phase, resonant frequency, real/reactive/apparent power, KCL/KVL, voltage/current dividers |
| **Circuit ECS** ✨ | `electrical/` | Resistor, Capacitor, Inductor, VoltageSource, CurrentSource, Diode (Si/Schottky/LED/Zener), CircuitBranch, PowerBus — Euler integration systems; DcMotor (back-EMF), AcMotor (slip-torque), BuckConverter, BoostConverter |
| **Chemical kinetics** ✨ | `laws/kinetics/chemical.rs` | Arrhenius k(T), rate laws, 1st/2nd order, half-lives, Keq↔ΔG°, Van't Hoff, reaction quotient, pH/pOH, Henderson-Hasselbalch, weak acid, enthalpy of reaction, Kirchhoff |
| **Catalysis** ✨ | `laws/kinetics/catalysis.rs` | Michaelis-Menten, Hill, competitive/non-competitive/uncompetitive inhibition, Langmuir θ, BET, Langmuir-Hinshelwood, Eley-Rideal, TOF |
| **Combustion** ✨ | `chemistry/combustion.rs` | Stoichiometric AFR, equivalence ratio, Dulong LHV, LHV→HHV, 8 fuel constants, adiabatic flame temperature (iterative), CO₂/H₂O emission factors, Wobbe index |
| **Chemical equilibrium** ✨ | `chemistry/equilibrium.rs` | Weak acid/base, buffer pH + capacity, Ksp, common-ion effect, Clausius-Clapeyron, boiling-point elevation, freezing-point depression, osmotic pressure, Henry's law, Raoult, Le Chatelier |
| **Chemistry ECS** ✨ | `chemistry/` | ChemicalSpecies (10 presets), ChemicalMixture, ChemicalReaction (Arrhenius, net_rate, heat_release_w), CstrReactor, BatchReactor — CSTR + Batch + temperature update systems |
| **ODE solvers** ✨ | `numerics/ode/` | Forward Euler, Backward Euler, RK4, RK45 Dormand-Prince (adaptive), Velocity Verlet (symplectic), BDF-1 (Newton + fixed-point), BDF-2 |
| **Interpolation** ✨ | `numerics/interpolation.rs` | lerp, inv_lerp, bilinear, 1D/2D table lookup, cubic Hermite, smoothstep/smootherstep, natural cubic spline (Thomas algorithm) |
| **Statistics** ✨ | `numerics/statistics/` | Gaussian PDF/CDF/quantile, Box-Muller, uniform, exponential, Poisson, Weibull, erf/erfc, LCG PRNG; linear + polynomial regression, mean/variance/stddev/Pearson-r, RMSE, MAE |
| **PID controller** ✨ | `control/pid.rs` | PidController with anti-windup, derivative-on-measurement, gain scheduling (interpolated), cascade PID, bumpless transfer |
| **State-space** ✨ | `control/state_space.rs` | StateSpaceModel (8-state fixed arrays), RK4/Euler integration, TF→canonical form, Routh-Hurwitz stability (6th order), 1st/2nd order helpers |
| **Frequency analysis** ✨ | `control/frequency.rs` | Bode magnitude/phase, log-spaced bode_plot, gain crossover, phase margin, gain margin, step response, damping from overshoot, settling time |
| **Digital control** ✨ | `control/discrete.rs` | Bilinear transform (Tustin), digital velocity-form PID, IIR Direct Form II, LPF1/HPF1, Butterworth biquad, notch filter, group delay |
| Particle simulation | `particles/` | ThermodynamicState + KineticState ECS; spatial hash; particle types (Gas/Liquid/Solid/Plasma/Dust/Smoke/Fire) |
| Material properties | `materials/` | MaterialProperties (Young, yield, fracture toughness, thermal conductivity…); presets: steel/Al/concrete/glass/rubber/wood |
| Fluid dynamics | `fluids/` | SPH, aerodynamics (Cd/Cl presets), buoyancy (Archimedes), Bernoulli |
| Deformation | `deformation/` | Vertex-level stress/thermal/impact deformation; fracture mesh; GPU deform |
| Thermal conduction | `thermal_conduction.rs` | Fourier's law between ECS entity pairs; auto proximity detection |
| Quantum statistics | `quantum/` | Bose-Einstein, Fermi-Dirac distributions; condensates; partition functions |
| Nuclear kinetics | `nuclear/` | Point kinetics (dn/dt, dC/dt), Doppler feedback, decay heat, 3-loop PID, deterministic control law (feedforward + P-trim) |
| Simulation infra | `simulation/` | Clock (10⁹× compression), WatchPoints, Breakpoints, Recorder, LOD |
| Visualizers | `visualizers/` | Property overlays, vector fields, heat maps, stress indicators |

### Strength assessment — Phases A–L SHIPPED (commits 3df9bf58 + 98fe9ff4)

The four biggest gaps closed by A–D, plus the full breadth from E–L:
- **Electromagnetism / circuits / control** — solid (A–D)
- **Chemical kinetics / combustion / equilibrium** — solid (A–D)
- **Numerical integration** — RK4/RK45/Verlet/BDF available everywhere (A)
- **Structures, thermodynamic cycles, propulsion** — solid (E–G)
- **Optics, acoustics** — solid (H–I)
- **Nuclear decay/shielding/criticality** — solid (J)
- **Biology** (population, enzyme, membrane) — solid (K)
- **Plasma / fusion** — solid (L)

Verified: `eustress-common` compiles clean; 425 realism unit tests pass.
Remaining for a future pass: Navier-Stokes fluid solver, Schrödinger quantum,
FFT, Monte Carlo, optimisation, graph/network, special relativity, full phase diagrams.

---

## 2 — Gap Map: Every STEM Domain

Legend: ✅ SOLID  ⚠️ PARTIAL  ❌ MISSING  🆕 A–D  ✨ E–L

```
PHYSICS
  Classical mechanics     ████████████████ ✅ SOLID
  Thermodynamics          ████████████████ ✅ SOLID
  Electromagnetism        ████████████████ ✅ SOLID 🆕 (fields, circuits, induction, waves, ECS motors)
  Optics                  ████████████████ ✅ SOLID ✨ (Snell, lenses, Fresnel, Planck, Beer-Lambert)
  Acoustics / waves       ████████████████ ✅ SOLID ✨ (speed/impedance, Doppler, Sabine RT60, modes)
  Fluid dynamics          ████████░░░░░░░░ ⚠️  PARTIAL (SPH + drag, no NS solver)
  Statistical mechanics   ████████░░░░░░░░ ⚠️  PARTIAL (particles + quantum stats)
  Quantum mechanics       ████░░░░░░░░░░░░ ⚠️  PARTIAL (statistics only, no Schrödinger)
  Plasma physics          ████████████████ ✅ SOLID ✨ (Debye, MHD, Lawson, fusion gain, ECS state)
  Nuclear physics         ████████████████ ✅ SOLID ✨ (fission + PID + decay chains + shielding + criticality)
  Special relativity      ░░░░░░░░░░░░░░░░ ❌ MISSING
  Condensed matter        ████░░░░░░░░░░░░ ⚠️  PARTIAL (material properties, no band theory)

CHEMISTRY
  Electrochemistry        ████████████████ ✅ SOLID
  Chemical kinetics       ████████████████ ✅ SOLID 🆕 (Arrhenius, rate laws, equilibrium, pH, catalysis)
  Thermochemistry         ████████████████ ✅ SOLID 🆕 (ΔH_f tables, reaction enthalpy, combustion)
  Stoichiometry           ████████████░░░░ ✅ SOLID 🆕 (AFR, emission factors, species balance)
  Acid-base / pH          ████████████████ ✅ SOLID 🆕 (pH, Henderson-Hasselbalch, buffers, Ka values)
  Phase equilibrium       ████████████░░░░ ✅ SOLID ✨ (Clausius-Clapeyron, colligative, Henry, Raoult)
  Materials chemistry     ████████░░░░░░░░ ⚠️  PARTIAL (properties; no Pilling-Bedworth, no corrosion kinetics)

ENGINEERING
  Electrical circuits     ████████████████ ✅ SOLID 🆕 (Kirchhoff, Ohm, R/L/C, AC power, ECS components)
  Control systems         ████████████████ ✅ SOLID 🆕 (PID + anti-windup, state-space, Bode, digital filters)
  Structural / FEA        ████████████████ ✅ SOLID ✨ (beams, columns/buckling, fatigue, composites)
  Heat exchangers / HVAC  ████████████████ ✅ SOLID ✨ (LMTD, NTU-effectiveness)
  Thermodynamic cycles    ████████████████ ✅ SOLID ✨ (Rankine, Brayton, Otto/Diesel, refrigeration)
  Rocket propulsion       ████████████████ ✅ SOLID ✨ (Tsiolkovsky, de Laval, Isp, multistage, electric)
  Compressible flow       ████░░░░░░░░░░░░ ⚠️  PARTIAL (nozzle/Mach in propulsion; no shock tables)
  Power systems           ████████░░░░░░░░ ⚠️  PARTIAL (DC/AC motors, converters — no grid)

BIOLOGY / LIFE SCIENCE
  Population dynamics     ████████████████ ✅ SOLID ✨ (logistic, Lotka-Volterra, SIR, R0, herd immunity)
  Enzyme kinetics         ████████████████ ✅ SOLID 🆕✨ (Michaelis-Menten, Hill, Monod, inhibition)
  Membrane biophysics     ████████████████ ✅ SOLID ✨ (Nernst, Goldman, Hodgkin-Huxley, cable)
  Ecology                 ████░░░░░░░░░░░░ ⚠️  PARTIAL (trophic via population dynamics; no food-web graph)

APPLIED MATHEMATICS
  Numerical ODE solvers   ████████████████ ✅ SOLID 🆕 (RK4, RK45 adaptive, Verlet, BDF-1/2)
  Signal processing       ████████░░░░░░░░ ⚠️  PARTIAL 🆕 (IIR filters, Bode — no FFT yet)
  Statistical analysis    ████████████░░░░ ✅ SOLID 🆕 (distributions, regression, RMSE — no MC yet)
  Optimization            ░░░░░░░░░░░░░░░░ ❌ MISSING (gradient descent, Newton, LP)
  Graph / network         ░░░░░░░░░░░░░░░░ ❌ MISSING
```

---

## 3 — Architecture: Target Module Tree

✅ = shipped   🚧 = next   ❌ = planned

```
eustress/crates/common/src/realism/
│
├── constants.rs           ✅ DONE (Phase A ext) — ΔH_f tables, Ka, COULOMB_K, WIEN, BOHR…
├── units.rs               ✅ DONE (Phase A ext) — Tesla, Henry, Farad, Gray, Sievert, Becquerel…
│
├── laws/
│   ├── mod.rs             ✅ DONE
│   ├── thermodynamics.rs  ✅ DONE
│   ├── mechanics.rs       ✅ DONE
│   ├── conservation.rs    ✅ DONE
│   ├── electrochemistry.rs ✅ DONE
│   │
│   ├── electromagnetism/  ✅ DONE (Phase B) — fields, circuits, induction, waves
│   │
│   ├── kinetics/          ✅ DONE (Phase C) — chemical, catalysis
│   │
│   ├── optics/            ❌ Phase H
│   │   ├── geometric.rs   Snell, lens equation, mirrors, thin-lens
│   │   ├── wave.rs        Interference, diffraction, Huygens
│   │   └── photons.rs     Photoelectric, blackbody, Beer-Lambert
│   │
│   ├── acoustics/         ❌ Phase I
│   │   ├── waves.rs       Wave equation, SHM, standing waves
│   │   ├── propagation.rs Intensity, attenuation, Doppler, Mach cone
│   │   └── rooms.rs       Reverberation time, absorption coefficients
│   │
│   ├── relativity/        ❌ Tier 3
│   │   ├── special.rs     Lorentz transforms, time dilation, mass-energy
│   │   └── corrections.rs GPS correction, relativistic kinetic energy
│   │
│   └── biology/           ❌ NEW — Tier 3
│       ├── population.rs  Lotka-Volterra, SIR, logistic growth
│       ├── enzyme.rs      Michaelis-Menten, Hill equation
│       └── membrane.rs    Hodgkin-Huxley, Goldman, Nernst potential
│
├── electrical/            ✅ DONE (Phase B)
│   ├── mod.rs             ✅ ElectricalPlugin
│   ├── components.rs      ✅ Resistor, Capacitor, Inductor, VoltageSource, Diode, CircuitBranch, PowerBus
│   ├── circuit.rs         ✅ Euler integration for C/L, resistor I/P, diode conduction
│   └── power.rs           ✅ BuckConverter, BoostConverter, DcMotor (back-EMF), AcMotor (slip-torque)
│
├── control/               ✅ DONE (Phase D)
│   ├── mod.rs             ✅ ControlPlugin
│   ├── pid.rs             ✅ PidController (anti-windup, gain scheduling, cascade, bumpless)
│   ├── state_space.rs     ✅ StateSpaceModel, TF→SS, Routh-Hurwitz stability
│   ├── frequency.rs       ✅ Bode plot, gain/phase margins, step response
│   └── discrete.rs        ✅ Bilinear transform, IIR filters, digital PID, Butterworth, notch
│
├── chemistry/             ✅ DONE (Phase C)
│   ├── mod.rs             ✅ ChemistryPlugin
│   ├── components.rs      ✅ ChemicalSpecies (10 presets), ChemicalMixture, ChemicalReaction, CstrReactor, BatchReactor
│   ├── reactor.rs         ✅ CSTR + Batch Euler integration, temperature energy balance
│   ├── equilibrium.rs     ✅ Ksp, buffers, Clausius-Clapeyron, boiling/freezing-point, osmotic, Henry, Raoult
│   └── combustion.rs      ✅ AFR, LHV/HHV, Dulong, adiabatic flame temp, emission factors, Wobbe
│
├── structures/            ❌ Phase E
│   ├── beams.rs           Euler-Bernoulli, shear/moment diagrams
│   ├── trusses.rs         Method of joints, zero-force members
│   ├── columns.rs         Euler buckling, slenderness ratio
│   ├── fatigue.rs         S-N curve, Miner's rule, Paris crack growth
│   └── composites.rs      Rule of mixtures, laminate theory (CLT)
│
├── thermocycles/          ❌ Phase F
│   ├── rankine.rs         Steam power cycle (turbine, condenser, pump, boiler)
│   ├── brayton.rs         Gas turbine cycle (compressor, combustor, turbine)
│   ├── otto.rs            Spark-ignition (4-stroke, compression ratio)
│   ├── diesel.rs          Compression-ignition (cutoff ratio, Diesel efficiency)
│   ├── refrigeration.rs   Vapor-compression cycle (COP, superheat, subcooling)
│   └── heat_exchangers.rs LMTD, NTU-effectiveness, shell-and-tube, plate
│
├── propulsion/            ❌ Phase G
│   ├── rockets.rs         Tsiolkovsky, specific impulse, nozzle (de Laval)
│   ├── jets.rs            Turbojet/turbofan thrust, bypass ratio, TSFC
│   ├── propellers.rs      Blade element theory, thrust/torque coefficients
│   └── electric.rs        Hall thruster, ion thruster, electrostatic propulsion
│
├── numerics/              ✅ DONE (Phase A)
│   ├── mod.rs             ✅ NumericsPlugin
│   ├── ode/
│   │   ├── euler.rs       ✅ Forward + Backward Euler (scalar + slice)
│   │   ├── runge_kutta.rs ✅ RK4 + RK45 Dormand-Prince (adaptive)
│   │   ├── implicit.rs    ✅ BDF-1 (Newton + fixed-point), BDF-2
│   │   └── verlet.rs      ✅ Velocity Verlet (symplectic, 3D)
│   ├── transforms/
│   │   ├── fft.rs         ❌ DFT, FFT (Cooley-Tukey), inverse FFT
│   │   └── laplace.rs     ❌ Numerical Laplace, s-domain utilities
│   ├── interpolation.rs   ✅ lerp, bilinear, table_lookup_1D/2D, cubic Hermite, natural spline
│   ├── statistics/
│   │   ├── distributions.rs  ✅ Gaussian, Poisson, exponential, uniform, Weibull, erf, LCG
│   │   ├── regression.rs     ✅ Linear + polynomial regression, R², RMSE, MAE
│   │   └── monte_carlo.rs    ❌ Importance sampling, Markov chain MC
│   └── optimization/
│       ├── gradient.rs    ❌ Gradient descent, Adam, BFGS
│       ├── root.rs        ❌ Newton-Raphson, bisection, secant method
│       └── linear.rs      ❌ Simplex method, LP utilities
│
├── plasma/                ❌ NEW — Tier 2
│   ├── mod.rs             PlasmaPlugin
│   ├── components.rs      PlasmaState (ionization degree, Te, Ti, ne)
│   ├── mhd.rs             Magnetohydrodynamics (ideal + resistive)
│   ├── debye.rs           Debye screening, plasma frequency, Larmor radius
│   └── fusion.rs          D-T reaction rate, Lawson criterion, confinement
│
├── nuclear/               ✅ (extend — Tier 1 already done)
│   ├── constants.rs       ✅ (+cross-sections, Q-values, decay constants)
│   ├── components.rs      ✅
│   ├── systems.rs         ✅
│   ├── control_law.rs     ✅
│   ├── decay.rs           ❌ NEW — radioactive decay chains (Bateman equations)
│   ├── shielding.rs       ❌ NEW — gamma attenuation, dose calculations
│   └── criticality.rs     ❌ NEW — four-factor formula, migration area
│
├── optics_render/         ❌ NEW — Tier 3 (GPU-accelerated ray physics)
│   ├── dispersion.rs      Cauchy equation, Sellmeier, Abbe number
│   ├── polarization.rs    Stokes parameters, Jones calculus
│   └── nonlinear.rs       SHG, Kerr effect (for laser simulation)
│
├── geoscience/            ❌ NEW — Tier 4
│   ├── atmosphere.rs      Standard atmosphere, CAPE, lapse rate
│   ├── seismology.rs      P/S waves, Richter/moment magnitude
│   └── climate.rs         Simple energy balance model, Milankovitch
│
├── biology/               ❌ NEW — Tier 3 (ECS-level life simulation)
│   ├── components.rs      Cell, Organism, Population ECS components
│   ├── metabolism.rs      ATP synthesis (chemiosmosis), basal metabolic rate
│   ├── genetics.rs        Mendelian inheritance, Hardy-Weinberg, mutation rate
│   └── ecology.rs         Food web energetics, trophic efficiency
│
│   — existing modules remain unchanged —
├── particles/             ✅
├── materials/             ✅  (+fatigue, creep, phase diagrams → materials v2)
├── fluids/                ✅  (+compressible flow, turbulence models → fluids v2)
├── deformation/           ✅
├── thermal_conduction.rs  ✅
├── quantum/               ✅  (+Schrödinger solver → quantum v2)
├── visualizers/           ✅  (+circuit diagrams, Bode plots, phase diagrams)
└── lod.rs                 ✅
```

---

## 4 — Inter-Module Dependency Graph

```
                        numerics/
                       (ODE solvers)
                     ╔══════════════╗
                     ║ Used by ALL  ║
                     ╚══════╤═══════╝
                            │
          ┌─────────────────┼──────────────────┐
          ▼                 ▼                  ▼
   laws/electromagnetism  chemistry/         control/
          │                 │                  │
          ├──► electrical/  ├──► thermocycles/ ├──► nuclear/
          │       │         │                  │
          │       └─────────┼──► propulsion/   └──► structures/
          │                 │
          └──► plasma/      └──► biology/
                 │
                 └──► nuclear/ (fusion path)

   constants.rs + units.rs underpin everything
   particles/ + materials/ are used by fluids/ + deformation/ + plasma/
   simulation/ (Clock/WatchPoints) is used by every plugin for telemetry
```

**Key principle:** `numerics/ode` replaces hand-coded Euler integrators. Every
existing system (nuclear kinetics, thermal, battery) can opt into RK4 or BDF
by swapping one line. Already available — just needs call-site migration.

---

## 5 — Phased Delivery Plan

### ✅ Phase A — Foundations — SHIPPED (commit 3df9bf58)

`numerics/` — RK4, RK45 adaptive, Velocity Verlet, BDF-1/2, cubic spline,
bilinear table lookup, Gaussian/Poisson/Weibull distributions, linear +
polynomial regression. `constants.rs` + `units.rs` extended with EM/radiation/
chemical constants and 12 new unit types.

---

### ✅ Phase B — Electromagnetism + Circuits — SHIPPED (commit 3df9bf58)

`laws/electromagnetism/` (fields, circuits, induction, waves) +
`electrical/` ECS (Resistor, Capacitor, Inductor, Diode, DcMotor, AcMotor,
BuckConverter, BoostConverter, CircuitBranch, PowerBus).

---

### ✅ Phase C — Chemical Kinetics + Combustion — SHIPPED (commit 3df9bf58)

`laws/kinetics/` (Arrhenius, rate laws, equilibrium, pH, Michaelis-Menten,
Langmuir, BET) + `chemistry/` ECS (ChemicalSpecies, ChemicalMixture,
ChemicalReaction, CstrReactor, BatchReactor) + combustion + equilibrium.

---

### ✅ Phase D — Control Systems — SHIPPED (commit 3df9bf58)

`control/` — PidController (anti-windup, gain scheduling, cascade, bumpless),
StateSpaceModel (8-state, RK4, Routh-Hurwitz), Bode plots, gain/phase margins,
bilinear transform, IIR filters, Butterworth, notch filter.

---

### Phase E — Structures + FEA Kernel ★★★★ Next

**Target: `structures/`**

E.1 — `structures/beams.rs`
: *Needed by: electrical, nuclear/shielding, optics*

---

### Phase B — Electromagnetism + Circuits ★★★★★ Priority: CRITICAL

**Target: `laws/electromagnetism/`, `electrical/`**

B.1 — `laws/electromagnetism/fields.rs`
: `coulomb_force(q1, q2, r) -> Vec3` — Coulomb's law
: `electric_field_point_charge(q, r) -> Vec3` — E = kq/r²
: `magnetic_force_lorentz(q, v, B) -> Vec3` — F = q(v×B)
: `biot_savart(I, dl, r) -> Vec3` — dB from current element
: `gauss_electric(charge, epsilon) -> f32` — Gauss's law flux
: `faraday_emf(d_flux, dt) -> f32` — ε = −dΦ/dt
: `energy_electric_field(E, epsilon, volume) -> f32` — u = ½ε₀E²
: `energy_magnetic_field(B, mu, volume) -> f32` — u = B²/(2μ₀)

B.2 — `laws/electromagnetism/circuits.rs`
: `ohm_law(V, R) -> f32` — I = V/R
: `kirchhoff_current(currents: &[f32]) -> bool` — KCL: ΣI = 0
: `kirchhoff_voltage(voltages: &[f32]) -> bool` — KVL: ΣV = 0
: `series_resistance(Rs: &[f32]) -> f32`, `parallel_resistance(Rs: &[f32]) -> f32`
: `capacitor_charge(C, V) -> f32` — Q = CV
: `capacitor_energy(C, V) -> f32` — E = ½CV²
: `inductor_energy(L, I) -> f32` — E = ½LI²
: `capacitor_impedance(C, omega) -> f32` — Z_C = 1/(jωC) magnitude
: `inductor_impedance(L, omega) -> f32` — Z_L = jωL magnitude
: `resonant_frequency(L, C) -> f32` — ω₀ = 1/√(LC)
: `rc_time_constant(R, C) -> f32` — τ = RC
: `rl_time_constant(R, L) -> f32` — τ = L/R
: `rc_voltage_charge(V0, t, tau) -> f32` — V(t) = V₀·(1−e^(−t/τ))
: `rc_voltage_discharge(V0, t, tau) -> f32` — V(t) = V₀·e^(−t/τ)
: `power_dc(V, I) -> f32` — P = VI
: `power_resistive(I, R) -> f32` — P = I²R
: `power_ac_real(V_rms, I_rms, cos_phi) -> f32` — P = V·I·cos(φ)
: `power_factor(P, S) -> f32` — pf = P/S

B.3 — `electrical/components.rs`
: `ElectricalNode { voltage: f32, current: f32, charge: f32 }`
: `Resistor { resistance_ohms: f32 }` — Component
: `Capacitor { capacitance_farads: f32, charge: f32 }` — Component
: `Inductor { inductance_henries: f32, flux_linkage: f32 }` — Component
: `VoltageSource { voltage: f32, internal_resistance: f32 }` — Component
: `CurrentSource { current: f32 }` — Component
: `CircuitConnection { from: Entity, to: Entity, element: CircuitElement }` — Component

B.4 — `electrical/circuit.rs` system
: Node-voltage method for small circuits (<64 nodes)
: Per-frame integration of capacitor voltage, inductor current
: Compatible with battery (VCellBatteryComponent) as a source

*Why critical: motors, electromagnets, sensors, PCBs, power grids, plasma heating*

---

### Phase C — Chemical Kinetics + Combustion ★★★★★ Priority: CRITICAL

**Target: `laws/kinetics/`, `chemistry/`**

C.1 — `laws/kinetics/chemical.rs`
: `arrhenius_rate_constant(A, E_a, T) -> f32` — k(T) = A·exp(−E_a/RT)
: `reaction_rate_elementary(k, concentrations: &[f32], orders: &[f32]) -> f32` — r = k·∏cᵢⁿⁱ
: `equilibrium_constant_from_gibbs(delta_G, T) -> f32` — K = exp(−ΔG/RT)
: `equilibrium_constant_temperature(K_ref, delta_H, T_ref, T) -> f32` — Van't Hoff
: `concentration_1st_order(c0, k, t) -> f32` — c(t) = c₀·e^(−kt)
: `half_life_1st_order(k) -> f32` — t½ = ln(2)/k
: `activation_energy_from_rates(k1, T1, k2, T2) -> f32` — E_a from two measurements
: `ph_from_concentration(H_concentration) -> f32` — pH = −log₁₀([H⁺])
: `henderson_hasselbalch(pKa, c_acid, c_base) -> f32` — buffer pH
: `solubility_product(concentrations: &[f32], stoich: &[f32]) -> f32` — Ksp

C.2 — `laws/kinetics/catalysis.rs`
: `michaelis_menten_rate(Vmax, Km, substrate) -> f32` — v = Vmax·[S]/(Km + [S])
: `enzyme_inhibition_competitive(Vmax, Km, I, Ki, S) -> f32`
: `langmuir_adsorption(theta_max, K, c) -> f32` — surface coverage θ = K·c/(1 + K·c)

C.3 — `chemistry/combustion.rs`
: `stoichiometric_afr(fuel_formula) -> f32` — Air-fuel ratio for complete combustion
: `adiabatic_flame_temperature(fuel, oxidizer, T_initial) -> f32`
: `heating_value_lower(fuel) -> f32` — LHV from formation enthalpies
: `heating_value_higher(fuel) -> f32` — HHV (includes water condensation)
: `equivalence_ratio(actual_afr, stoich_afr) -> f32` — λ = AFR_actual/AFR_stoich

C.4 — `chemistry/components.rs`
: `ChemicalSpecies { name: String, molar_mass: f32, concentration: f32 }`
: `Mixture { species: Vec<ChemicalSpecies>, temperature: f32, pressure: f32 }`
: `ChemicalReaction { reactants, products, delta_H, activation_energy, rate_constant }`

*Why critical: combustion engines, fuel cells, chemical plant simulation, corrosion, explosives, atmospheric chemistry*

---

### Phase D — Control Systems (generalized) ★★★★ Priority: HIGH

**Target: `control/`**

Generalizes the nuclear PID into a universal control toolkit.

D.1 — `control/pid.rs`
: `PidController { kp, ki, kd, setpoint, integral, prev_error, output_min, output_max, anti_windup_limit }`
: `pid_step(controller, measured, dt) -> f32`
: `pid_with_feedforward(controller, measured, feedforward, dt) -> f32`
: `gain_schedule(pid, operating_point, schedule: &[(f32, f32, f32, f32)]) -> PidController`
: *Note: promote and generalize existing nuclear PidState*

D.2 — `control/state_space.rs`
: `StateSpaceModel { A, B, C, D: MatN }` — continuous-time
: `stability_eigenvalues(A) -> Vec<Complex<f32>>` — Routh-Hurwitz check
: `controllability_matrix(A, B) -> MatN`
: `observability_matrix(A, C) -> MatN`
: `lqr_gains(A, B, Q, R) -> MatN` — Linear Quadratic Regulator (if Symbolica available)

D.3 — `control/frequency.rs`
: `bode_magnitude(tf_num, tf_den, omega) -> f32` — Transfer function |H(jω)|
: `bode_phase(tf_num, tf_den, omega) -> f32` — ∠H(jω)
: `gain_margin(tf_num, tf_den) -> f32`
: `phase_margin(tf_num, tf_den) -> f32`

D.4 — `control/discrete.rs`
: `bilinear_transform(tf_s, T) -> (Vec<f32>, Vec<f32>)` — Tustin approximation
: `iir_filter_step(b, a, x, state) -> f32` — Direct Form II
: `pid_discrete(kp, ki, kd, T) -> (Vec<f32>, Vec<f32>)` — Digital PID coefficients

---

### Phase E — Structures + FEA Kernel ★★★★ Priority: HIGH

**Target: `structures/`**

E.1 — `structures/beams.rs`
: `bending_stress(M, y, I) -> f32` — σ = M·y/I (Euler-Bernoulli)
: `shear_stress(V, Q, I, b) -> f32` — τ = VQ/(Ib)
: `beam_deflection_simply_supported(P, a, b, L, E, I, x) -> f32`
: `beam_deflection_cantilever(P, L, E, I, x) -> f32`
: `natural_frequency_beam(E, I, rho, A, L) -> f32` — First bending mode
: `moment_of_area_rectangle(b, h) -> f32` — I = bh³/12
: `moment_of_area_circle(r) -> f32` — I = πr⁴/4
: `moment_of_area_hollow_circle(r_outer, r_inner) -> f32`
: `section_modulus(I, y_max) -> f32` — Z = I/c

E.2 — `structures/columns.rs`
: `euler_critical_load(E, I, L, end_condition) -> f32` — P_cr = π²EI/(KL)²
: `slenderness_ratio(K, L, r) -> f32` — λ = KL/r
: `johnson_parabola(Fy, E, slenderness) -> f32` — Column strength for short columns
: `buckling_safety_factor(P_cr, P_applied) -> f32`

E.3 — `structures/fatigue.rs`
: `goodman_diagram(sigma_a, sigma_m, Sut, Se) -> f32` — Modified Goodman factor
: `miners_rule(cycles: &[f32], life: &[f32]) -> f32` — D = Σ(n/N)
: `paris_crack_growth(C, m, delta_K) -> f32` — da/dN = C·(ΔK)^m
: `stress_intensity_factor(sigma, a, geometry_factor) -> f32` — K = Yσ√(πa)
: `fracture_condition(K, K_IC) -> bool` — K ≥ K_IC → fracture

E.4 — `structures/composites.rs`
: `rule_of_mixtures_E(E_f, E_m, Vf) -> f32` — Longitudinal: E_l = E_f·Vf + E_m·(1-Vf)
: `rule_of_mixtures_transverse(E_f, E_m, Vf) -> f32` — Transverse Halpin-Tsai
: `tsai_hill_criterion(sigma, X, Y, S) -> f32` — Failure index for ortho lamina
: `classical_laminate_theory(plies: &[Ply]) -> ABD_Matrix` — Laminate stiffness

---

### Phase F — Thermodynamic Cycles + Heat Exchangers ★★★★ Priority: HIGH

**Target: `thermocycles/`**

F.1 — `thermocycles/rankine.rs`
: `rankine_thermal_efficiency(T_high, T_low) -> f32` — Ideal η
: `rankine_back_work_ratio(w_pump, w_turbine) -> f32`
: `reheat_rankine_efficiency(T_high, T_reheat, T_low) -> f32`
: `regenerative_rankine(T_high, T_low, extraction_fraction) -> f32`
: State points (h, s, x) at each cycle point via steam tables lookup

F.2 — `thermocycles/brayton.rs`
: `brayton_thermal_efficiency(pressure_ratio, gamma) -> f32` — η = 1 - r_p^((γ-1)/γ)
: `compressor_work(h2, h1) -> f32`
: `turbine_work(h3, h4) -> f32`
: `specific_work_output(turbine_work, compressor_work) -> f32`
: `turbofan_thrust(mdot_core, mdot_fan, V_jet, V_inf, BPR) -> f32`

F.3 — `thermocycles/otto.rs`
: `otto_thermal_efficiency(compression_ratio, gamma) -> f32` — η = 1 - r^(1-γ)
: `mean_effective_pressure(W_net, V_displacement) -> f32` — BMEP
: `engine_power(BMEP, displacement, RPM, n_cyl, n_stroke) -> f32`

F.4 — `thermocycles/heat_exchangers.rs`
: `lmtd(delta_T1, delta_T2) -> f32` — Log mean temperature difference
: `lmtd_correction_factor(R, P, flow_config) -> f32` — LMTD-F correction
: `ntu_effectiveness_parallel(NTU, C_min_ratio) -> f32`
: `ntu_effectiveness_counter(NTU, C_min_ratio) -> f32`
: `ntu_from_ua(UA, C_min) -> f32` — NTU = UA/C_min
: `required_area(Q, U, lmtd) -> f32` — A = Q/(U·ΔT_lm)
: `effectiveness_to_ntu(effectiveness, C_ratio, config) -> f32`

---

### Phase G — Propulsion ★★★ Priority: MEDIUM

**Target: `propulsion/`**

G.1 — `propulsion/rockets.rs`
: `tsiolkovsky_delta_v(v_e, m_initial, m_final) -> f32` — Δv = v_e·ln(m_i/m_f)
: `specific_impulse(thrust, mass_flow_rate) -> f32` — Isp = F/ṁg
: `nozzle_exit_velocity(T0, P0, Pe, gamma, R_gas) -> f32` — de Laval exit velocity
: `nozzle_thrust_coefficient(gamma, P0, Pe, Pa, A_e, A_t) -> f32` — C_F
: `chamber_pressure_from_cstar(c_star, A_t, mdot) -> f32`
: `rocket_staging_delta_v(stages: &[(f32, f32, f32)]) -> f32` — Multistage Δv

G.2 — `propulsion/jets.rs`
: `turbojet_thrust(mdot, V_jet, V_inf) -> f32` — F = ṁ(V_jet - V_inf)
: `specific_fuel_consumption(thrust, fuel_flow) -> f32` — TSFC
: `bypass_ratio_thrust(mdot_core, mdot_fan, V_core, V_fan, V_inf) -> f32`

---

### Phase H — Optics ★★★ Priority: MEDIUM

**Target: `laws/optics/`, `optics_render/`**

H.1 — `laws/optics/geometric.rs`
: `snell_refraction_angle(n1, theta1, n2) -> f32` — Snell's law
: `critical_angle(n1, n2) -> f32` — θ_c = arcsin(n2/n1)
: `thin_lens_image_distance(f, do) -> f32` — 1/f = 1/do + 1/di
: `magnification(di, do) -> f32` — m = −di/do
: `mirror_image_distance(f, do) -> f32` — 1/f = 1/do + 1/di (same form)
: `reflectance_fresnel_normal(n1, n2) -> f32` — R = ((n1-n2)/(n1+n2))²
: `brewster_angle(n1, n2) -> f32` — θ_B = arctan(n2/n1)

H.2 — `laws/optics/wave.rs`
: `double_slit_fringe_spacing(lambda, L, d) -> f32` — y = λL/d
: `single_slit_diffraction_minima(m, lambda, a) -> f32` — sin(θ) = mλ/a
: `thin_film_condition(n, thickness, order) -> (f32, f32)` — constructive/destructive λ
: `rayleigh_criterion(lambda, D) -> f32` — θ_min = 1.22λ/D (resolving power)

H.3 — `laws/optics/photons.rs`
: `photon_energy(frequency) -> f32` — E = hf
: `photon_wavelength(energy) -> f32` — λ = hc/E
: `photoelectric_cutoff(work_function) -> f32` — ν_min = φ/h
: `compton_shift(theta) -> f32` — Δλ = (h/m_e·c)·(1−cos θ)
: `blackbody_peak_wavelength(T) -> f32` — Wien: λ_max = b/T
: `blackbody_spectral_radiance(lambda, T) -> f32` — Planck distribution
: `beer_lambert(I0, alpha, path_length) -> f32` — I = I₀·e^(−αx)

---

### Phase I — Acoustics ★★★ Priority: MEDIUM

**Target: `laws/acoustics/`**

I.1 — `laws/acoustics/waves.rs`
: `sound_speed(bulk_modulus, density) -> f32` — c = √(K/ρ)
: `acoustic_impedance(density, speed) -> f32` — Z = ρc
: `wave_intensity(pressure_amplitude, impedance) -> f32` — I = p²/(2Z)
: `decibels_spl(p, p_ref) -> f32` — L_p = 20·log₁₀(p/p_ref)
: `decibels_intensity(I, I_ref) -> f32` — L = 10·log₁₀(I/I_ref)

I.2 — `laws/acoustics/propagation.rs`
: `doppler_frequency(f_source, v_source, v_observer, v_sound) -> f32`
: `mach_cone_angle(v_object, v_sound) -> f32` — sin(μ) = v_s/v
: `spherical_spreading(I0, r0, r) -> f32` — I = I₀·(r₀/r)²
: `atmospheric_absorption(f, humidity, T) -> f32` — ISO 9613-1

I.3 — `laws/acoustics/rooms.rs`
: `sabine_reverberation_time(volume, total_absorption) -> f32` — T60 = 0.161V/A
: `eyring_reverberation_time(volume, mean_absorption, S) -> f32` — Eyring correction
: `schroeder_frequency(T60, V) -> f32` — f_S = 2000√(T60/V)
: `modal_density(f, V, c) -> f32` — Modes per Hz

---

### Phase J — Nuclear Extensions ★★★ Priority: MEDIUM

**Target: `nuclear/decay.rs`, `nuclear/shielding.rs`, `nuclear/criticality.rs`**

J.1 — `nuclear/decay.rs`
: `radioactive_decay(N0, lambda, t) -> f32` — N(t) = N₀·e^(−λt)
: `bateman_equations(N0: &[f32], lambdas: &[f32], t) -> Vec<f32>` — Decay chain
: `activity(N, lambda) -> f32` — A = λN [Becquerel]
: `decay_constant_from_half_life(t_half) -> f32` — λ = ln(2)/t½
: `specific_activity(lambda, molar_mass) -> f32`
: Prebuilt chains: U-238→Pb-206, Th-232→Pb-208, fission product mix

J.2 — `nuclear/shielding.rs`
: `gamma_attenuation(I0, mu, x) -> f32` — I = I₀·e^(−μx)
: `half_value_layer(mu) -> f32` — HVL = ln(2)/μ
: `dose_rate_point_source(activity, energy, distance, buildup) -> f32`
: `dose_equivalent(absorbed_dose, quality_factor) -> f32` — H = QD [Sievert]

J.3 — `nuclear/criticality.rs`
: `four_factor_formula(eta, epsilon, p, f) -> f32` — k_inf = ηεpf
: `migration_area(diffusion_length, slowing_down_length) -> f32`
: `critical_radius_sphere(M_squared, k_inf) -> f32` — Geometric buckling

---

### Phase K — Biology / Life Science ★★ Priority: LOWER

**Target: `laws/biology/`, `biology/`**

K.1 — Population dynamics
: `logistic_growth(r, K, N, dt) -> f32` — dN/dt = rN(1 - N/K)
: `lotka_volterra(alpha, beta, gamma, delta, prey, pred, dt) -> (f32, f32)` — predator-prey
: `sir_model(S, I, R, beta, gamma, dt) -> (f32, f32, f32)` — SIR epidemic

K.2 — Enzyme kinetics
: `michaelis_menten_rate(Vmax, Km, S) -> f32` — already in C.2
: `hill_cooperativity(S, K, n) -> f32` — v = Vmax·Sⁿ/(Kⁿ + Sⁿ)

K.3 — Membrane biophysics
: `nernst_membrane_potential(z, T, c_out, c_in) -> f32` — E = (RT/zF)·ln(c_out/c_in)
: `goldman_potential(PK, PNa, PCl, T, K_in, K_out, Na_in, Na_out, Cl_in, Cl_out) -> f32`
: `hodgkin_huxley_step(V, m, h, n, I_ext, dt) -> (f32, f32, f32, f32)` — Action potential integrator

---

### Phase L — Plasma + Fusion ★★ Priority: LOWER (post-V-Cell maturity)

**Target: `plasma/`**

L.1 — `plasma/debye.rs`
: `debye_length(n_e, T_e, epsilon_0) -> f32` — λ_D = √(ε₀k_BT_e/(n_e·e²))
: `plasma_frequency(n_e) -> f32` — ω_p = √(n_e·e²/(ε₀·m_e))
: `larmor_radius(m, v_perp, q, B) -> f32` — r_L = mv_⊥/(qB)
: `coulomb_logarithm(n_e, T_e) -> f32` — ln Λ ≈ 23 - ln(n_e^½ T_e^(-3/2))

L.2 — `plasma/mhd.rs`
: Ideal MHD equations (Alfvén wave speed, magnetic pressure, plasma beta)
: Frozen-in flux theorem condition check

L.3 — `plasma/fusion.rs`
: `lawson_criterion(n_tau, T) -> bool` — n·τ > n·τ_Lawson(T)
: `dt_reaction_rate(n_D, n_T, sigma_v) -> f32` — Fusion power density
: `fusion_gain_Q(P_fusion, P_heating) -> f32` — Q = P_fusion/P_heating
: Lawson criterion curve for D-T plasma (tabulated σv vs T)

---

## 6 — Implementation Priority Matrix

```
                   IMPACT (breadth of things it enables)
                   LOW          MEDIUM        HIGH
EFFORT  LOW    │ acoustics   │ optics      │ numerics/ode  │
               │ bio/ecology │ propulsion  │ chem kinetics │
               │             │ nuclear ext │               │
        MEDIUM │ relativity  │ structures  │ electromag    │
               │ plasma      │ thermo cycle│ circuits      │
               │             │ control sys │               │
        HIGH   │ biology full│ geoscience  │ (none — avoid)│
               │ fusion      │ FEA solver  │               │
```

**Sequence recommendation:**
1. `numerics/` — foundational, low effort, unlocks everything
2. `laws/electromagnetism/` + `electrical/` — highest new territory
3. `laws/kinetics/` + `chemistry/` — combustion, fuel cells, reactors
4. `control/` (generalized) — robotics, automation, any feedback loop
5. `structures/` — structural engineering, mechanical design
6. `thermocycles/` — power plant design, engine simulation
7. `propulsion/` — rockets, turbines
8. `laws/optics/` — cameras, lasers, displays
9. `laws/acoustics/` — sound, sonar, seismology
10. `nuclear/` extensions — decay chains, shielding
11. `laws/biology/` — life simulation
12. `plasma/` — fusion path (after electrical + nuclear mature)

---

## 7 — Design Invariants for Every New Module

Every module added to the STEM stack must follow these rules:

1. **Pure functions in `laws/`** — No Bevy imports, no ECS.  Input = scalars/vecs.
   Output = scalars/vecs.  Testable in isolation.  These are the kernel laws.

2. **ECS components in `<domain>/components.rs`** — `#[derive(Component, Reflect)]`.
   Hold state between frames.  Named `<Domain>State` or `<Domain>Properties`.

3. **Systems in `<domain>/systems.rs`** — Read components, call `laws/` functions,
   write components.  Use `numerics/ode` integrators.  Publish to WatchPoints.

4. **Constants in `constants.rs`** — All physical constants go in the existing file,
   under a sub-module if domain-specific.  Never hardcode literals in law functions.

5. **Units** — All law functions take and return SI base units (Pa, K, m, kg, A, mol).
   Conversion to/from other units happens at the boundary (UI, TOML loading, Rune).

6. **LOD-aware** — Every system checks `SimLodTier` and skips when `Culled`.
   Expensive systems (FEA, SPH) gate on `High` tier only.

7. **Rune-accessible** — Every component's key scalar outputs are published as
   `<domain>.<entity_name>.<quantity>` watchpoints so Rune scripts and the
   Workshop AI can read and write them.

8. **Composable with everything** — Thermal, electrical, mechanical, chemical
   effects must be able to share entities.  A battery is simultaneously a
   `ThermodynamicState` + `ElectrochemicalState` + `MaterialProperties` entity.

---

## 8 — What This Enables (Examples)

| Scenario | Modules required |
|----------|-----------------|
| Electric motor driving a pump | `electrical/` + `mechanics/` + `fluids/` |
| Solid rocket booster trajectory | `propulsion/rockets` + `mechanics/` + `fluids/aerodynamics` |
| Nuclear-powered submarine | `nuclear/` + `thermocycles/rankine` + `electrical/` + `structures/` |
| Combustion engine | `chemistry/combustion` + `thermocycles/otto` + `mechanics/` + `structures/fatigue` |
| Bridge under load | `structures/beams` + `structures/fatigue` + `materials/` |
| Hospital MRI | `laws/electromagnetism/` + `plasma/debye` + `biology/membrane` |
| Ecological impact of temperature rise | `thermodynamics/` + `biology/population` + `chemistry/` |
| Laser cutting | `laws/optics/` + `thermodynamics/` + `materials/` + `deformation/` |
| Satellite in orbit | `mechanics/` + `propulsion/rockets` + `electrical/` + `thermocycles/` |
| Drug metabolism | `laws/kinetics/catalysis` + `biology/membrane` + `chemistry/equilibrium` |
| Fusion reactor | `nuclear/` + `plasma/` + `laws/electromagnetism/` + `thermocycles/rankine` |
| Smart grid under load | `electrical/` + `control/` + `thermocycles/` + `numerics/statistics` |

COMPLETE INVENTORY OF EUSTRESS ENGINE REALISM SYSTEM
1. CONSTANTS.RS
Physical Constants (SI units)

Universal Constants:
G (f64 6.674e-11 m³/(kg·s²)) - Gravitational constant
G_F32 (f32 version)
C (f64 299_792_458 m/s) - Speed of light
C_F32 (f32 version)
H (f64 6.626e-34 J·s) - Planck constant
H_BAR (f64 1.055e-34 J·s) - Reduced Planck constant
STEFAN_BOLTZMANN (f64 5.670e-8 W/(m²·K⁴))
STEFAN_BOLTZMANN_F32 (f32 version)
Thermodynamic Constants:
R (f64 8.314 J/(mol·K)) - Universal gas constant + f32 version
K_B (f64 1.381e-23 J/K) - Boltzmann constant + f32 version
N_A (f64 6.022e23 1/mol) - Avogadro constant
STANDARD_TEMPERATURE (f32 298.15 K - 25°C)
STANDARD_PRESSURE (f32 101_325 Pa)
WATER_TRIPLE_POINT (f32 273.16 K)
WATER_BOILING_POINT (f32 373.15 K)
Electromagnetic Constants:
EPSILON_0 (f64 8.854e-12 F/m) - Vacuum permittivity
MU_0 (f64 1.257e-6 H/m) - Vacuum permeability
ELEMENTARY_CHARGE (f64 1.602e-19 C)
ELECTRON_MASS (f64 9.109e-31 kg)
PROTON_MASS (f64 1.673e-27 kg)
Atmospheric Constants:
AIR_DENSITY_SEA_LEVEL (f32 1.225 kg/m³)
AIR_VISCOSITY (f32 1.81e-5 Pa·s)
AIR_KINEMATIC_VISCOSITY (f32 1.48e-5 m²/s)
AIR_SPECIFIC_GAS_CONSTANT (f32 287.05 J/(kg·K))
AIR_GAMMA (f32 1.4) - Ratio of specific heats (Cp/Cv)
SPEED_OF_SOUND_AIR (f32 343 m/s)
Water Constants:
WATER_DENSITY (f32 1000 kg/m³)
WATER_VISCOSITY (f32 1.002e-3 Pa·s)
WATER_SURFACE_TENSION (f32 0.0728 N/m)
WATER_SPECIFIC_HEAT (f32 4186 J/(kg·K))
WATER_THERMAL_CONDUCTIVITY (f32 0.606 W/(m·K))
WATER_LATENT_HEAT_VAPORIZATION (f32 2.26e6 J/kg)
WATER_LATENT_HEAT_FUSION (f32 3.34e5 J/kg)
Material Preset Constants (nested modules):
materials::steel::* (YOUNG_MODULUS, POISSON_RATIO, YIELD_STRENGTH, ULTIMATE_STRENGTH, DENSITY, THERMAL_CONDUCTIVITY, SPECIFIC_HEAT)
materials::aluminum::* (same properties)
materials::concrete::*
materials::glass::*
materials::rubber::*
materials::wood::*
Electrochemistry Constants:
FARADAY (f64 96_485.33 C/mol) + f32 version
SHE_REFERENCE (f32 0.0 V) - Standard hydrogen electrode
ELECTRON_CHARGE (f64 1.602e-19 C)
THERMAL_VOLTAGE_25C (f32 0.025693 V)
R_ELECTRO (f32 8.314 J/(mol·K))
Na-S Battery Constants (na_s::*):
STANDARD_POTENTIAL (f32 2.23 V), ANODE_POTENTIAL (-2.714 V), CATHODE_POTENTIAL (-0.480 V)
ELECTRONS (f32 2.0), SULFUR_CAPACITY_MAH_G (f32 1_672), SODIUM_CAPACITY_MAH_G (f32 1_166)
THEORETICAL_ENERGY_DENSITY (f32 5_517 Wh/kg)
SULFUR_VOLUME_EXPANSION (f32 0.80)
ENTROPY_COEFFICIENT (f32 -0.00015 V/K)
Molar masses: SULFUR_MOLAR_MASS (f32 32.06), SODIUM_MOLAR_MASS (f32 22.990), NA2S_MOLAR_MASS (f32 78.04), S8_MOLAR_MASS (f32 256.48)
Sc-NASICON Constants (sc_nasicon::*):
IONIC_CONDUCTIVITY_TARGET (f32 1.0e-2 S/cm)
IONIC_CONDUCTIVITY_DEMONSTRATED (f32 1.0e-3 S/cm)
ACTIVATION_ENERGY_EV (f32 0.22 eV), ACTIVATION_ENERGY_J_MOL (f32 21_224 J/mol)
ARRHENIUS_PREFACTOR (f32 1_500 S/cm)
ELECTRONIC_CONDUCTIVITY (f32 1.0e-10 S/cm)
VACANCY_FRACTION (f32 0.067)
WINDOW_MIN/MAX (0.0 to 5.0 V vs Na/Na⁺)
DENSITY (f32 3_200 kg/m³)
V-Cell Material Constants (vcell_materials::*):
sodium::* - Young modulus 10e9 Pa, density 971 kg/m³, thermal conductivity 142 W/(m·K), etc.
sc_nasicon::* - Young modulus 80e9 Pa, density 3_200 kg/m³, thermal conductivity 1.5 W/(m·K), etc.
sulfur_vacnt::* - Young modulus 50e9 Pa, density 1_075 kg/m³, thermal conductivity 15 W/(m·K), etc.
al_hex_lattice::* - Young modulus 4.2e9 Pa (porous), porosity 0.92, hex edge 50µm, wall thickness 5µm
aluminum_nitride::* - Young modulus 310e9 Pa, thermal conductivity 170 W/(m·K)
al_6061_t6::* - Young modulus 68.9e9 Pa, yield 276e6 Pa, density 2_700 kg/m³
Conversion Helper Functions:
celsius_to_kelvin(f32) -> f32
kelvin_to_celsius(f32) -> f32
atm_to_pascals(f32) -> f32
pascals_to_atm(f32) -> f32
bar_to_pascals(f32) -> f32
pascals_to_bar(f32) -> f32
2. UNITS.RS
SI Unit System with Type-Safe Conversions

Base SI Unit Newtypes:
Meters(f32) - Length in meters
Kilograms(f32) - Mass in kilograms
Seconds(f32) - Time in seconds
Kelvin(f32) - Temperature
Moles(f32) - Amount of substance
Amperes(f32) - Electric current
Candela(f32) - Luminous intensity
Derived SI Unit Newtypes:
Newtons(f32) - Force
Pascals(f32) - Pressure
Joules(f32) - Energy
Watts(f32) - Power
MetersPerSecond(f32) - Velocity
MetersPerSecondSquared(f32) - Acceleration
KilogramsPerCubicMeter(f32) - Density
PascalSeconds(f32) - Dynamic viscosity
JoulesPerKelvin(f32) - Entropy
JoulesPerKilogramKelvin(f32) - Specific heat capacity
WattsPerMeterKelvin(f32) - Thermal conductivity
CubicMeters(f32) - Volume
SquareMeters(f32) - Area
Operator Implementations: (via impl_unit_ops! macro)
Add, Sub, Mul (f32), Div (f32), Neg for all unit types
From<f32> and Into<f32> conversions
Meters Conversion Methods:
from_feet(f32) -> Meters, to_feet() -> f32
from_inches(f32) -> Meters, to_inches() -> f32
from_cm(f32) -> Meters, to_cm() -> f32
from_mm(f32) -> Meters, to_mm() -> f32
from_studs(f32) -> Meters (1 stud = 0.28m), to_studs() -> f32
Kilograms Conversion Methods:
from_pounds(f32) -> Kilograms, to_pounds() -> f32
from_grams(f32) -> Kilograms, to_grams() -> f32
Kelvin Conversion Methods:
from_celsius(f32) -> Kelvin, to_celsius() -> f32
from_fahrenheit(f32) -> Kelvin, to_fahrenheit() -> f32
Pascals Conversion Methods:
from_atm(f32) -> Pascals, to_atm() -> f32
from_bar(f32) -> Pascals, to_bar() -> f32
from_psi(f32) -> Pascals, to_psi() -> f32
from_mpa(f32) -> Pascals, to_mpa() -> f32
from_gpa(f32) -> Pascals, to_gpa() -> f32
Joules Conversion Methods:
from_calories(f32) -> Joules, to_calories() -> f32
from_kwh(f32) -> Joules, to_kwh() -> f32
from_ev(f32) -> Joules, to_ev() -> f32
MetersPerSecond Conversion Methods:
from_kmh(f32), to_kmh()
from_mph(f32), to_mph()
from_knots(f32), to_knots()
from_mach(f32), to_mach()
CubicMeters Conversion Methods:
from_liters(f32), to_liters()
from_gallons(f32), to_gallons()
Dimensional Analysis Helper Functions:
force(Kilograms, MetersPerSecondSquared) -> Newtons
pressure(Newtons, SquareMeters) -> Pascals
work(Newtons, Meters) -> Joules
power(Joules, Seconds) -> Watts
kinetic_energy(Kilograms, MetersPerSecond) -> Joules
density(Kilograms, CubicMeters) -> KilogramsPerCubicMeter
3. LAWS/MOD.RS
Module Exports:

pub mod thermodynamics;
pub mod mechanics;
pub mod conservation;
pub mod electrochemistry;
pub mod prelude { /* exports from all 4 modules */ }
4. LAWS/THERMODYNAMICS.RS
Gas Laws & Heat Transfer

Ideal Gas Law Functions:
ideal_gas_pressure(n: f32, t: f32, v: f32) -> f32 - P = nRT/V
ideal_gas_volume(n: f32, t: f32, p: f32) -> f32 - V = nRT/P
ideal_gas_temperature(p: f32, v: f32, n: f32) -> f32 - T = PV/(nR)
ideal_gas_moles(p: f32, v: f32, t: f32) -> f32 - n = PV/(RT)
Van der Waals Equation:
van_der_waals_pressure(n, t, v, a, b) -> f32 - Real gas pressure
Van der Waals constants for: HELIUM, HYDROGEN, NITROGEN, OXYGEN, CO2, WATER
First Law of Thermodynamics:
internal_energy_change(heat_in, work_out) -> f32 - ΔU = Q - W
work_isobaric(pressure, delta_volume) -> f32 - W = PΔV
work_isothermal(n, t, v1, v2) -> f32 - W = nRT·ln(V₂/V₁)
work_adiabatic(p1, v1, p2, v2, gamma) -> f32 - Adiabatic work
Heat Capacity Functions:
heat_capacity_monatomic_cv(n) -> f32 - Cv = (3/2)nR
heat_capacity_monatomic_cp(n) -> f32 - Cp = (5/2)nR
heat_capacity_diatomic_cv(n) -> f32 - Cv = (5/2)nR
heat_capacity_diatomic_cp(n) -> f32 - Cp = (7/2)nR
heat_required(mass, specific_heat, delta_temp) -> f32 - Q = mcΔT
temperature_change(heat, mass, specific_heat) -> f32 - ΔT = Q/(mc)
Entropy & Second Law:
entropy_change_reversible(heat, temperature) -> f32 - ΔS = Q/T
entropy_change_isothermal(n, v1, v2) -> f32 - ΔS = nR·ln(V₂/V₁)
entropy_change_general(n, cv, t1, t2, v1, v2) -> f32 - General ideal gas entropy
carnot_efficiency(t_cold, t_hot) -> f32 - η = 1 - T_cold/T_hot
cop_refrigerator(t_cold, t_hot) -> f32 - COP = T_cold/(T_hot - T_cold)
cop_heat_pump(t_cold, t_hot) -> f32 - COP = T_hot/(T_hot - T_cold)
Heat Transfer:
heat_conduction_rate(k, area, delta_temp, thickness) -> f32 - Fourier's law: Q = kA·ΔT/L
heat_convection_rate(h, area, t_surface, t_fluid) -> f32 - Newton's cooling: Q = hA·ΔT
heat_radiation_rate(emissivity, area, t_surface, t_environment) -> f32 - Stefan-Boltzmann: Q = εσA(T⁴ - T_env⁴)
Phase Transitions:
heat_phase_change(mass, latent_heat) -> f32 - Q = mL
enum WaterPhase { Solid, Liquid, Gas, Supercritical }
water_phase(temperature, pressure) -> WaterPhase - Simplified phase diagram
Free Energies:
enthalpy(internal_energy, pressure, volume) -> f32 - H = U + PV
enthalpy_change_isobaric(heat_at_constant_pressure) -> f32 - ΔH = Q_p
gibbs_free_energy(enthalpy, temperature, entropy) -> f32 - G = H - TS
helmholtz_free_energy(internal_energy, temperature, entropy) -> f32 - F = U - TS
ThermodynamicStateData Struct:
Fields: temperature, pressure, volume, internal_energy, entropy, enthalpy, gibbs, moles
ideal_gas(moles, temperature, volume) -> Self - Create ideal gas state
add_heat_isochoric(&mut self, heat) - Constant volume heating
add_heat_isobaric(&mut self, heat) - Constant pressure heating
5. LAWS/MECHANICS.RS
Newtonian Mechanics & Dynamics

Newton's Laws:
force_from_acceleration(mass, acceleration) -> Vec3 - F = ma
acceleration_from_force(force, mass) -> Vec3 - a = F/m
net_force(forces: &[Vec3]) -> Vec3 - Sum of forces
Kinematics:
position_constant_velocity(initial_position, velocity, time) -> Vec3 - x = x₀ + vt
position_constant_acceleration(initial_position, initial_velocity, acceleration, time) -> Vec3 - x = x₀ + v₀t + ½at²
velocity_constant_acceleration(initial_velocity, acceleration, time) -> Vec3 - v = v₀ + at
velocity_from_displacement(initial_velocity, acceleration, displacement) -> f32 - v² = v₀² + 2a·Δx
Energy:
kinetic_energy(mass, velocity) -> f32 - KE = ½m·v²
kinetic_energy_scalar(mass, speed) -> f32 - KE = ½m·v²
gravitational_potential_energy(mass, gravity, height) -> f32 - PE = mgh
gravitational_potential_universal(m1, m2, distance) -> f32 - U = -GMm/r
elastic_potential_energy(spring_constant, displacement) -> f32 - PE = ½kx²
work(force, displacement) -> f32 - W = F·d
work_at_angle(force_magnitude, displacement, angle_radians) -> f32 - W = Fd·cos(θ)
power_from_work(work, time) -> f32 - P = W/t
power_from_force_velocity(force, velocity) -> f32 - P = F·v
Momentum & Collisions:
momentum(mass, velocity) -> Vec3 - p = mv
impulse(force, delta_time) -> Vec3 - J = F·Δt
velocity_change_from_impulse(impulse, mass) -> Vec3 - Δv = J/m
elastic_collision_1d(m1, v1, m2, v2) -> (f32, f32) - 1D elastic collision
elastic_collision_3d(m1, v1, pos1, m2, v2, pos2) -> (Vec3, Vec3) - 3D elastic collision
inelastic_collision(m1, v1, m2, v2) -> Vec3 - Perfectly inelastic (stick together)
collision_with_restitution(m1, v1, m2, v2, restitution) -> (f32, f32) - Collision with coefficient of restitution e
Rotational Dynamics:
angular_momentum(moment_of_inertia, angular_velocity) -> Vec3 - L = Iω
torque(position, force) -> Vec3 - τ = r × F
torque_from_angular_acceleration(moment_of_inertia, angular_acceleration) -> Vec3 - τ = Iα
angular_acceleration_from_torque(torque, moment_of_inertia) -> Vec3 - α = τ/I
rotational_kinetic_energy(moment_of_inertia, angular_velocity) -> f32 - KE_rot = ½Iω²
Moment of Inertia Submodule:
solid_sphere(mass, radius) -> f32 - I = (2/5)mr²
hollow_sphere(mass, radius) -> f32 - I = (2/3)mr²
solid_cylinder(mass, radius) -> f32 - I = (1/2)mr²
hollow_cylinder(mass, radius) -> f32 - I = mr²
solid_rod_center(mass, length) -> f32 - I = (1/12)mL²
solid_rod_end(mass, length) -> f32 - I = (1/3)mL²
rectangular_plate(mass, width, height) -> f32 - I = (1/12)m(a² + b²)
Gravity & Orbits:
gravitational_force(m1, m2, distance) -> f32 - F = GMm/r²
gravitational_force_vector(m1, pos1, m2, pos2) -> Vec3 - Gravitational force vector (attractive)
surface_gravity(mass, radius) -> f32 - g = GM/r²
escape_velocity(mass, radius) -> f32 - v_esc = √(2GM/r)
orbital_velocity(central_mass, orbital_radius) -> f32 - v_orbit = √(GM/r)
orbital_period(central_mass, semi_major_axis) -> f32 - T = 2π√(r³/GM) [Kepler's 3rd law]
Friction:
static_friction_max(coefficient, normal_force) -> f32 - f_s = μ_s·N
kinetic_friction(coefficient, normal_force) -> f32 - f_k = μ_k·N
friction_force_vector(coefficient, normal_force, velocity) -> Vec3 - Friction opposing motion
Spring Forces:
spring_force(spring_constant, displacement) -> f32 - F = -kx [Hooke's Law]
spring_force_vector(spring_constant, anchor, position, rest_length) -> Vec3 - 3D spring force
damped_spring_force(spring_constant, damping, displacement, velocity) -> f32 - F = -kx - cv
spring_damper_force_vector(spring_constant, damping, anchor, position, velocity, rest_length) -> Vec3 - 3D damped spring
6. LAWS/CONSERVATION.RS
Conservation Laws

Mass Conservation:
mass_conservation_check(initial_mass, current_masses) -> f32 - Returns error
mass_flow_rate(density, area, velocity) -> f32 - ṁ = ρAv [Continuity equation]
volume_flow_rate(area, velocity) -> f32 - Q = Av
velocity_from_continuity(area1, velocity1, area2) -> f32 - v₂ = (A₁/A₂)v₁
Energy Conservation:
total_mechanical_energy(kinetic, potential) -> f32 - E = KE + PE
energy_conservation_check(initial_energy, current_energy) -> f32 - Returns error
energy_dissipated(initial_energy, final_energy) -> f32
bernoulli_constant(pressure, density, velocity, height, gravity) -> f32 - P + ½ρv² + ρgh = const
bernoulli_pressure(p1, v1, h1, v2, h2, density, gravity) -> f32 - Pressure from Bernoulli
torricelli_velocity(gravity, height) -> f32 - v = √(2gh) [Free drainage]
Momentum Conservation:
total_momentum(masses, velocities) -> Vec3 - Sum of all momenta
momentum_conservation_check(initial_momentum, current_momentum) -> Vec3
center_of_mass(masses, positions) -> Vec3 - r_cm = Σ(m_i·r_i)/Σm_i
center_of_mass_velocity(masses, velocities) -> Vec3
Angular Momentum Conservation:
angular_momentum_point(position, mass, velocity) -> Vec3 - L = r × p
total_angular_momentum(positions, masses, velocities) -> Vec3
angular_momentum_conservation_check(initial, current) -> Vec3
ConservationTracker Struct:
Fields: initial_mass, initial_energy, initial_momentum, initial_angular_momentum, tolerance
initialize(&mut self, masses, positions, velocities, potential_energies) - Set initial state
check(&self, masses, positions, velocities, potential_energies) -> ConservationResult
ConservationResult Struct:
Bools: mass_conserved, energy_conserved, momentum_conserved, angular_momentum_conserved
f32 errors: mass_error, energy_error, momentum_error, angular_momentum_error
all_conserved(&self) -> bool
7. LAWS/ELECTROCHEMISTRY.RS
Electrochemical Equations - Detailed Inventory

Nernst Equation:
nernst_potential(e_standard, n, temperature, activity_ratio) -> f32 - E = E° - (RT/nF)·ln(Q)
thermal_voltage(temperature) -> f32 - V_T = RT/F (≈ 25.7 mV @ 298K)
Butler-Volmer Kinetics:
butler_volmer_current(j0, eta, alpha_a, alpha_c, temperature) -> f32 - Full BV equation
butler_volmer_symmetric(j0, eta, temperature) -> f32 - j = 2j₀·sinh(Fη/2RT)
tafel_overpotential(j, j0, alpha, temperature) -> f32 - η = (RT/αF)·ln(j/j₀)
exchange_current_density(k0, c_ox, c_red, alpha) -> f32 - j₀ = Fk₀·c_ox^α·c_red^(1-α)
Ohmic Losses:
ohmic_overpotential(current, resistance) -> f32 - η_ohm = IR
electrolyte_asr(thickness, ionic_conductivity) -> f32 - ASR = thickness/σ
cell_resistance_from_asr(asr, electrode_area) -> f32 - R = ASR/A
terminal_voltage(ocv, eta_ohmic, eta_ct, eta_diff, is_discharge) -> f32 - V = OCV ± losses
round_trip_efficiency(v_discharge, v_charge) -> f32 - η_rt = V_discharge/V_charge
Ionic Transport:
arrhenius_conductivity(sigma0, e_act, temperature) -> f32 - σ(T) = σ₀·exp(-E_a/RT)
nernst_einstein_diffusivity(conductivity, concentration, z, temperature) -> f32 - D = σRT/(z²F²c)
nernst_planck_flux(diffusivity, concentration, conc_gradient, potential_gradient, z, temperature) -> f32 - J = -D(dc/dx) - (zFD/RT)c(dφ/dx)
Heat Generation:
ohmic_heat(current, resistance) -> f32 - Q = I²R
reaction_heat(current, eta_ct) -> f32 - Q = I·|η_ct|
entropic_heat(temperature, current, de_dt) -> f32 - Q = -T·I·(dE/dT)
total_heat_generation(current, resistance, eta_ct, temperature, de_dt) -> f32 - Q_total = Q_ohm + Q_rxn + Q_entropy
steady_state_temp_rise(heat_rate, r_thermal) -> f32 - ΔT = Q·R_thermal
Cycle Degradation:
capacity_retention_power_law(cycle_count, alpha, beta) -> f32 - Q(N)/Q₀ = 1 - α·N^β
cycles_to_retention(target_retention, alpha, beta) -> f32 - N = ((1-target)/α)^(1/β)
State Functions:
state_of_charge(soc_initial, charge_out_ah, nominal_capacity) -> f32 - SOC = SOC₀ - Q_out/Q_nom [Coulomb counting]
depth_of_discharge(soc) -> f32 - DOD = 1 - SOC
power_output(v_terminal, current) -> f32 - P = V·I
specific_power(v_terminal, current, mass_kg) -> f32 - P_specific = P/m
c_rate(current_a, capacity_ah) -> f32 - C = I/Q_nom
current_from_c_rate(c_rate_val, capacity_ah) -> f32 - I = C·Q_nom
ragone_energy_density(energy_1c, c_rate_val, peukert_exp) -> f32 - E(C) = E_1C / C^(n-1) [Peukert]
ionic_limiting_current(conductivity_s_m, temperature, thickness, tortuosity) -> f32 - j_lim = σV_T/(L·τ)
Dendrite Risk:
sands_time(diffusivity, concentration, current_density) -> f32 - t = π·D·(Fc₀)²/j²
monroe_newman_critical_current(shear_modulus, interlayer_thickness, molar_volume) -> f32 - j_crit = 2G_e·δ/(F·V_m)
dendrite_risk(current_density, critical_current) -> f32 - Risk = j/j_crit (≥1.0 is danger)
8. PARTICLES/COMPONENTS.RS
Particle ECS Components

ParticleType Enum:
Gas - Ideal gas behavior
Liquid - SPH fluid
Solid - Rigid body
Plasma - Charged gas
Dust - Air resistance
Smoke - Buoyant, dissipates
Fire - Emits heat, rises
Particle Component:
mass: f32 - Kilograms
radius: f32 - Meters (collision/visualization)
particle_type: ParticleType
lifetime: Option<f32> - Seconds remaining
active: bool
Constructor methods: new(), gas(), liquid(), solid(), with_lifetime()
ThermodynamicState Component:
temperature: f32 - Kelvin
pressure: f32 - Pascals
volume: f32 - m³
internal_energy: f32 - Joules
entropy: f32 - J/K
enthalpy: f32 - Joules
moles: f32 - Amount of substance
Constructor methods: standard_conditions(), ideal_gas(), at_conditions()
Update methods: update_pressure(), update_internal_energy(), update_enthalpy(), add_heat_isochoric(), add_heat_isobaric()
KineticState Component: (partial read)
Velocity, momentum, angular motion tracking
9. PARTICLES/SYSTEMS.RS
Particle Physics Systems

update_spatial_hash() - Rebuild spatial hash for neighbor queries
update_thermodynamics() - Update particle temperature, pressure, entropy per frame
Heat transfer calculations with neighbors
Pressure updates from ideal gas law
Simplified thermal conduction between particles
update_kinematics() - Update velocity and position from forces
Acceleration from F = ma
Parallel iteration support
Velocity and position updates
apply_forces() - Apply accumulated forces system
10. MATERIALS/PROPERTIES.RS
Material Property Component & Presets

MaterialProperties Component:
Mechanical: young_modulus, poisson_ratio, yield_strength, ultimate_strength, fracture_toughness, hardness
Thermal: thermal_conductivity, specific_heat, thermal_expansion, melting_point
Physical: density
Surface: friction_static, friction_kinetic, restitution
Custom: custom_properties: HashMap<String, f64> (extensible)
Material Presets (constructor methods):
steel(), aluminum(), concrete(), glass(), rubber(), wood()
Each preset has full 15+ property specification
Drawn from constants::materials::* or literature values
11. FLUIDS/MOD.RS
Fluid Dynamics Module Structure

Submodules: sph, water, aerodynamics, buoyancy
FluidsPlugin registers:
SphConfig resource
AerodynamicBody type
BuoyancyBody type
Systems: update_sph_density, update_sph_forces, apply_aerodynamic_forces, apply_buoyancy_forces
12. FLUIDS/AERODYNAMICS.RS
Aerodynamic Forces & Properties

AerodynamicBody Component:
drag_coefficient (C_d)
lift_coefficient (C_l)
drag_area, lift_area (m²)
center_of_pressure (Vec3 offset)
lift_direction (Vec3, typically Y)
enabled (bool)
Preset Constructors:
sphere(radius) - C_d = 0.47
cube(side) - C_d = 1.05
cylinder(radius, length) - C_d = 0.82
flat_plate(width, height) - C_d = 1.28
streamlined(frontal_area) - C_d = 0.04 (teardrop)
airfoil(chord, span, angle_of_attack) - Thin airfoil theory: C_l ≈ 2π·α, C_d from induced + parasitic
car(frontal_area) - C_d = 0.3 (modern car)
human_standing() - C_d = 1.0, drag_area = 0.7 m²
13. FLUIDS/BUOYANCY.RS
Buoyancy & Archimedes' Principle

BuoyancyBody Component:
volume: f32 (m³)
mass: f32 (kg)
center_of_buoyancy (Vec3 offset)
fluid_drag: f32
water_level: f32 (Y coordinate)
fluid_density: f32 (kg/m³, default WATER_DENSITY = 1000)
enabled: bool
Preset Constructors:
from_box(width, height, depth, mass) - Volume = w·h·d
from_sphere(radius, mass) - Volume = (4/3)πr³
from_cylinder(radius, height, mass) - Volume = πr²h
Helper Methods:
density() -> f32 - Returns m/V
will_float() -> bool - Density < fluid_density
submerged_fraction() -> f32 - Equilibrium submersion
14. DEFORMATION/MOD.RS
Mesh Deformation System

Submodules: components, systems, vertex, fracture_mesh, gpu_deform
DeformationPlugin registers:
DeformationConfig resource
DeformableMesh component
DeformationState component
Systems: init_deformable_meshes, update_stress_deformation, update_thermal_deformation, apply_impact_deformation, update_mesh_vertices, handle_fracture_mesh
Enables vertex-level deformation from stress, temperature, and impacts
Fracture propagation support for mesh splitting
15. THERMAL_CONDUCTION.RS
Thermal Conduction System

ThermalContact Component:
entity_a, entity_b (Entity pair)
contact_area: f32 (m²) - Heat flow cross-section
contact_thickness: f32 (m) - Conduction path length
ThermalConductionConfig Resource:
enabled: bool
max_delta_t_per_frame: f32 (K) - Stability limit
min_delta_t_threshold: f32 (K) - Minimum to trigger conduction
auto_contact_radius: f32 (m) - Proximity detection
auto_detect_contacts: bool
thermal_conduction_system() - Per-frame heat transfer using Fourier's law
Computes harmonic mean of both materials' conductivities
Updates temperatures based on contact geometry
Clamps temperature changes for stability
auto_thermal_contacts_system() - Auto-detect nearby thermal entities
Proximity-based contact creation
Deduplication of contact pairs
O(n²) check at low frequency
16. VISUALIZERS/MOD.RS
Real-Time Property Display

Submodules: property_overlay, vector_field, heat_map, stress_viz
VisualizersPlugin registers:
OverlaySettings, VectorFieldSettings, HeatMapSettings resources
PropertyOverlay component
Systems: update_property_overlays, draw_vector_field, draw_stress_indicators
Displays: T, P, V, U, S overlays + force/velocity fields + temperature gradients + stress tensors
17. QUANTUM/MOD.RS
Quantum Statistical Mechanics

Submodules: statistics, condensates
QuantumPlugin registers:
QuantumState component
update_quantum_statistics system
Integrates with Symbolica for exact arithmetic
Supports:
Bose-Einstein distribution (bosons)
Fermi-Dirac distribution (fermions)
Bose-Einstein condensates
Quantum tunneling effects
Partition functions with rational coefficients
18. LOD.RS
Simulation Level-of-Detail

SimLodTier Enum:
High - Within high_radius, updated every frame (~60 Hz)
Mid - Within mid_radius, updated every 6 frames (~10 Hz)
Low - Within low_radius, updated every 30 frames (~2 Hz)
Culled - Beyond low_radius, suspended (0 Hz)
SimLodTier Methods:
should_update(frame: u32) -> bool - Check if should run on this frame
hz_at_60fps() -> f32 - Effective simulation frequency
label() -> &'static str - Debug label
SimLodConfig Resource:
Distance thresholds: high_radius, mid_radius, low_radius (in meters)
Controls update frequency per distance tier
Zero stream events for culled entities
19. SIMULATION/CLOCK.RS
Simulation Clock (Time Acceleration)

SimulationClock Resource:
simulation_time_s: f64 - Current simulation time
wall_time_s: f64 - Wall clock elapsed
time_scale: f64 - Compression factor (1.0 = real-time, 1e6 = 1 million x faster)
fixed_timestep_s: f64 - Physics fixed timestep
accumulator_s: f64 - Accumulated time for fixed stepping
tick_count: u64 - Total ticks executed
tick_rate_hz: f64 - Target ticks per second
max_ticks_per_frame: u32 - Spiral-of-death prevention
Constructor Methods:
new(time_scale) -> Self
accelerated(time_scale, tick_rate_hz) -> Self
Methods:
advance(wall_delta_s) -> u32 - Returns ticks to execute
simulation_seconds/minutes/hours/days() -> f64 - Time accessors
Plus accumulator management for fixed timestep
20. SIMULATION/STATE.RS
Simulation Execution Modes

SimulationMode Enum:
Running - Normal execution
Paused - Paused
StepOnce - Execute one tick then pause
StepN(u32) - Execute N ticks then pause
RunUntil - Run until breakpoint or target reached
SimulationState Resource:
mode: SimulationMode
run_until_time_s: Option<f64> - Target time
run_until_tick: Option<u64> - Target tick count
completed: bool - Completion flag
completion_reason: Option<String>
breakpoints_hit: u32
last_breakpoint: Option<String>
Control Methods:
should_tick() -> bool - Check if should execute
pause(), resume(), step(), step_n(u32)
21. SIMULATION/WATCHPOINT.RS
Observable Variable Tracking

DataPoint Struct:
time_s: f64 - Simulation time when recorded
tick: u64 - Tick count
value: f64 - Recorded value
WatchPoint Struct:
name: String - Unique identifier
label: String - UI display name
unit: String - Unit string (V, A, °C, %, etc.)
history: VecDeque<DataPoint> - Time-series data
max_history: usize - Rolling buffer size
current: f64 - Current value
min/max: f64 - Min/max seen
average: f64 - Running average
record_interval: u32 - Sampling interval in ticks
enabled: bool
color: [u8; 4] - RGBA for graphing
Constructor Methods:
new(name, label, unit) -> Self
with_history_size(size) -> Self
with_interval(interval) -> Self
22. SIMULATION/MOD.RS
Simulation Module Exports

Public modules: clock, state, watchpoint, breakpoint, recorder, config
Re-exports all symbols from submodules
ADDITIONAL SCIENCE-ADJACENT MODULES IN COMMON/SRC:
From the file listing in common/src/, the following non-realism but related science modules exist:

classes.rs (409 KB) - Part/Instance class definitions
properties.rs (154 KB) - Entity property system
instance_create.rs (35 KB) - Instance spawning
parameters.rs (31 KB) - Configuration parameters
sim_record.rs (23 KB) - Simulation recording
sim_stream.rs (15 KB) - Real-time streaming
toml_materializer.rs (14 KB) - TOML persistence
scene.rs (49 KB) - Scene management
eustress_format.rs (17 KB) - File format
change_queue.rs (18 KB) - Delta tracking
SUMMARY STATISTICS
Module Category	File Count	Key Structs/Functions
Constants	1	100+ constants (physics, materials, electrochemistry)
Units	1	20 unit newtypes + 40+ conversion methods
Laws	4	80+ fundamental physics functions
Particles	4	3 major ECS components + systems
Materials	5	MaterialProperties + 6 presets
Fluids	4	Aerodynamics/Buoyancy/SPH/Water
Deformation	5	Vertex deformation + fracture + GPU
Thermal	1	ThermalContact + conduction system
Quantum	2	Bose-Einstein/Fermi-Dirac + condensates
Visualizers	4	Overlays, fields, heat maps, stress
LOD	1	SimLodTier (4 tiers, proximity-based)
Simulation	6	Clock, State, WatchPoint, BP, Recorder, Config
Total Realism	57 files	500+ exported symbols
The Eustress Engine realism system is a comprehensive physics simulation framework spanning classical mechanics, thermodynamics, electrochemistry, fluid dynamics, material science, quantum effects, and real-time visualization — all grounded in SI units and fundamental physical laws with support for time-acceleration up to 10⁹x and proximity-based LOD.
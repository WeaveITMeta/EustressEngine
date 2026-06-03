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

### ✅ Implemented (57 files, ~500 exported symbols)

| Domain | Module(s) | Coverage |
|--------|-----------|----------|
| Physical constants | `constants.rs` | Universal (G, c, h, k_B, R, N_A), EM (ε₀, μ₀, e), Atmospheric, Water, Na-S battery, Sc-NASICON, V-Cell materials |
| SI units + conversions | `units.rs` | 20 unit newtypes, 40+ conversions (ft, in, lb, °F, psi, mph, Mach…) |
| Classical mechanics | `laws/mechanics.rs` | F=ma, kinematics, energy, momentum, rotation, moment-of-inertia, gravity, Kepler, friction, springs |
| Thermodynamics | `laws/thermodynamics.rs` | Ideal + van der Waals gas, 1st/2nd/3rd law, heat transfer (Fourier/Newton/Stefan-Boltzmann), Carnot, phase transitions, Gibbs/Helmholtz |
| Conservation | `laws/conservation.rs` | Mass, energy, linear/angular momentum; Bernoulli; ConservationTracker |
| Electrochemistry | `laws/electrochemistry.rs` | Nernst, Butler-Volmer, Tafel, Ohmic, ionic transport (Arrhenius/Nernst-Einstein/Nernst-Planck), heat generation, dendrite risk, Peukert |
| Particle simulation | `particles/` | ThermodynamicState + KineticState ECS components; spatial hash; particle types (Gas/Liquid/Solid/Plasma/Dust/Smoke/Fire) |
| Material properties | `materials/` | MaterialProperties (Young, yield, fracture toughness, thermal conductivity…); presets: steel/Al/concrete/glass/rubber/wood |
| Fluid dynamics | `fluids/` | SPH, aerodynamics (Cd/Cl presets), buoyancy (Archimedes), Bernoulli |
| Deformation | `deformation/` | Vertex-level stress/thermal/impact deformation; fracture mesh splitting; GPU deform |
| Thermal conduction | `thermal_conduction.rs` | Fourier's law between ECS entity pairs; auto proximity detection |
| Quantum statistics | `quantum/` | Bose-Einstein, Fermi-Dirac distributions; condensates; partition functions |
| Nuclear kinetics | `nuclear/` | Point kinetics (dn/dt, dC/dt), Doppler feedback, decay heat, 3-loop PID, deterministic control law |
| Simulation infra | `simulation/` | Clock (10⁹× compression), WatchPoints, Breakpoints, Recorder, LOD |
| Visualizers | `visualizers/` | Property overlays, vector fields, heat maps, stress indicators |

### Strength assessment

The existing stack is **deep in two domains** — thermodynamics and electrochemistry
(V-Cell battery physics is graduate-level) — and **broad but shallow** everywhere
else.  Classical mechanics is complete.  Fluids have SPH particles and drag presets
but no Navier-Stokes solver.  Electromagnetism has constants but no field equations.
Chemistry has electrochemistry but no general reaction kinetics.

---

## 2 — Gap Map: Every STEM Domain

Red = absent.  Yellow = partial/stub.  Green = solid.

```
PHYSICS
  Classical mechanics     ████████████████ ✅ SOLID
  Thermodynamics          ████████████████ ✅ SOLID
  Electromagnetism        ░░░░░░░░░░░░░░░░ ❌ MISSING (constants only)
  Optics                  ░░░░░░░░░░░░░░░░ ❌ MISSING
  Acoustics / waves       ░░░░░░░░░░░░░░░░ ❌ MISSING
  Fluid dynamics          ████████░░░░░░░░ ⚠️  PARTIAL (SPH + drag, no NS solver)
  Statistical mechanics   ████████░░░░░░░░ ⚠️  PARTIAL (particles + quantum stats)
  Quantum mechanics       ████░░░░░░░░░░░░ ⚠️  PARTIAL (statistics only, no Schrödinger)
  Plasma physics          ░░░░░░░░░░░░░░░░ ❌ MISSING (Plasma particle type exists but no MHD)
  Nuclear physics         ████████████░░░░ ✅ SOLID (fission kinetics; decay chains missing)
  Special relativity      ░░░░░░░░░░░░░░░░ ❌ MISSING
  Condensed matter        ████░░░░░░░░░░░░ ⚠️  PARTIAL (materials properties, no band theory)

CHEMISTRY
  Electrochemistry        ████████████████ ✅ SOLID
  Chemical kinetics       ░░░░░░░░░░░░░░░░ ❌ MISSING (Arrhenius rate, equilibrium, catalysis)
  Thermochemistry         ████████░░░░░░░░ ⚠️  PARTIAL (ΔG, ΔH; no ΔH_f tables, no reaction enthalpy)
  Stoichiometry           ░░░░░░░░░░░░░░░░ ❌ MISSING
  Acid-base / pH          ░░░░░░░░░░░░░░░░ ❌ MISSING
  Phase equilibrium       ████░░░░░░░░░░░░ ⚠️  PARTIAL (water phases only)
  Materials chemistry     ████████░░░░░░░░ ⚠️  PARTIAL (properties; no Pilling-Bedworth, no corrosion)

ENGINEERING
  Electrical circuits     ░░░░░░░░░░░░░░░░ ❌ MISSING (Kirchhoff, Ohm, R/L/C, power)
  Control systems         ████░░░░░░░░░░░░ ⚠️  PARTIAL (nuclear PID; no general state-space/Bode)
  Structural / FEA        ████░░░░░░░░░░░░ ⚠️  PARTIAL (stress/strain; no beams, trusses, buckling)
  Heat exchangers / HVAC  ░░░░░░░░░░░░░░░░ ❌ MISSING (LMTD, NTU, effectiveness)
  Thermodynamic cycles    ░░░░░░░░░░░░░░░░ ❌ MISSING (Rankine, Brayton, Otto, refrigeration)
  Rocket propulsion       ░░░░░░░░░░░░░░░░ ❌ MISSING (Tsiolkovsky, nozzle, Isp)
  Compressible flow       ░░░░░░░░░░░░░░░░ ❌ MISSING (Mach, normal shock, isentropic)
  Geotechnical            ░░░░░░░░░░░░░░░░ ❌ MISSING
  Power systems           ░░░░░░░░░░░░░░░░ ❌ MISSING (grid, transformers, transmission)

BIOLOGY / LIFE SCIENCE
  Population dynamics     ░░░░░░░░░░░░░░░░ ❌ MISSING (Lotka-Volterra, SIR, logistic)
  Enzyme kinetics         ░░░░░░░░░░░░░░░░ ❌ MISSING (Michaelis-Menten)
  Membrane biophysics     ░░░░░░░░░░░░░░░░ ❌ MISSING (Hodgkin-Huxley, Goldman equation)
  Ecology                 ░░░░░░░░░░░░░░░░ ❌ MISSING

APPLIED MATHEMATICS
  Numerical ODE solvers   ░░░░░░░░░░░░░░░░ ❌ MISSING (RK4, BDF, implicit Euler; only explicit Euler exists)
  Signal processing       ░░░░░░░░░░░░░░░░ ❌ MISSING (DFT, FFT, filtering)
  Statistical analysis    ░░░░░░░░░░░░░░░░ ❌ MISSING (distributions, regression, Monte Carlo)
  Optimization            ░░░░░░░░░░░░░░░░ ❌ MISSING (gradient descent, Newton, linear programming)
  Graph / network         ░░░░░░░░░░░░░░░░ ❌ MISSING (flow, shortest path, topology)
```

---

## 3 — Architecture: Target Module Tree

```
eustress/crates/common/src/realism/
│
├── constants.rs           ✅  (+radiation, spectral, nuclear cross-sections)
├── units.rs               ✅  (+rad, gray, sievert, lumen, tesla, henry, farad)
│
├── laws/
│   ├── mod.rs             ✅  (extend)
│   ├── thermodynamics.rs  ✅
│   ├── mechanics.rs       ✅
│   ├── conservation.rs    ✅
│   ├── electrochemistry.rs ✅
│   │
│   ├── electromagnetism/  ❌ NEW — Tier 1
│   │   ├── fields.rs      Maxwell's equations, Coulomb, Biot-Savart
│   │   ├── circuits.rs    Kirchhoff's laws, R/L/C, AC impedance
│   │   ├── waves.rs       EM wave propagation, skin effect
│   │   └── induction.rs   Faraday's law, mutual/self inductance
│   │
│   ├── kinetics/          ❌ NEW — Tier 1
│   │   ├── chemical.rs    Arrhenius, rate laws, equilibrium constants
│   │   ├── reaction.rs    Stoichiometry, enthalpy of formation
│   │   └── catalysis.rs   Langmuir-Hinshelwood, Michaelis-Menten
│   │
│   ├── optics/            ❌ NEW — Tier 2
│   │   ├── geometric.rs   Snell, lens equation, mirrors, thin-lens
│   │   ├── wave.rs        Interference, diffraction, Huygens
│   │   └── photons.rs     Photoelectric, blackbody, Beer-Lambert
│   │
│   ├── acoustics/         ❌ NEW — Tier 2
│   │   ├── waves.rs       Wave equation, SHM, standing waves
│   │   ├── propagation.rs Intensity, attenuation, Doppler, Mach cone
│   │   └── rooms.rs       Reverberation time, absorption coefficients
│   │
│   ├── relativity/        ❌ NEW — Tier 3
│   │   ├── special.rs     Lorentz transforms, time dilation, mass-energy
│   │   └── corrections.rs GPS correction, relativistic kinetic energy
│   │
│   └── biology/           ❌ NEW — Tier 3
│       ├── population.rs  Lotka-Volterra, SIR, logistic growth
│       ├── enzyme.rs      Michaelis-Menten, Hill equation
│       └── membrane.rs    Hodgkin-Huxley, Goldman, Nernst potential
│
├── electrical/            ❌ NEW — Tier 1
│   ├── mod.rs             ElectricalPlugin
│   ├── components.rs      Circuit node components (voltage, current, charge)
│   ├── circuit.rs         Node-voltage method, mesh analysis
│   ├── devices.rs         Resistor, Capacitor, Inductor, Diode, Transistor ECS
│   ├── power.rs           Real/reactive/apparent power, power factor
│   └── motor.rs           DC/AC motor torque-speed curves
│
├── control/               ❌ NEW — Tier 1
│   ├── mod.rs             ControlPlugin
│   ├── pid.rs             Generic PID + anti-windup + gain scheduling
│   ├── state_space.rs     A/B/C/D matrices, eigenvalue stability
│   ├── frequency.rs       Bode plot, gain/phase margin, Nyquist
│   └── discrete.rs        Z-transform, sampled-data controllers
│
├── chemistry/             ❌ NEW — Tier 1
│   ├── mod.rs             ChemistryPlugin
│   ├── components.rs      ChemicalSpecies, Reaction, Mixture ECS
│   ├── reactor.rs         CSTR, PFR, batch reactor ODEs
│   ├── equilibrium.rs     Le Chatelier, Ka/Kb, Ksp, Henderson-Hasselbalch
│   └── combustion.rs      Stoichiometric combustion, adiabatic flame temp
│
├── structures/            ❌ NEW — Tier 2
│   ├── mod.rs             StructuresPlugin
│   ├── beams.rs           Euler-Bernoulli, shear/moment diagrams
│   ├── trusses.rs         Method of joints, zero-force members
│   ├── columns.rs         Euler buckling, slenderness ratio
│   ├── fatigue.rs         S-N curve, Miner's rule, Paris crack growth
│   └── composites.rs      Rule of mixtures, laminate theory (CLT)
│
├── thermocycles/          ❌ NEW — Tier 2
│   ├── mod.rs             ThermoCyclesPlugin
│   ├── rankine.rs         Steam power cycle (turbine, condenser, pump, boiler)
│   ├── brayton.rs         Gas turbine cycle (compressor, combustor, turbine)
│   ├── otto.rs            Spark-ignition (4-stroke, compression ratio)
│   ├── diesel.rs          Compression-ignition (cutoff ratio, Diesel efficiency)
│   ├── refrigeration.rs   Vapor-compression cycle (COP, superheat, subcooling)
│   └── heat_exchangers.rs LMTD, NTU-effectiveness, shell-and-tube, plate
│
├── propulsion/            ❌ NEW — Tier 2
│   ├── mod.rs             PropulsionPlugin
│   ├── rockets.rs         Tsiolkovsky, specific impulse, nozzle (de Laval)
│   ├── jets.rs            Turbojet/turbofan thrust, bypass ratio, TSFC
│   ├── propellers.rs      Blade element theory, thrust/torque coefficients
│   └── electric.rs        Hall thruster, ion thruster, electrostatic propulsion
│
├── numerics/              ❌ NEW — Tier 1 (foundational — used by all other modules)
│   ├── mod.rs             NumericsPlugin
│   ├── ode/
│   │   ├── euler.rs       Forward/backward Euler (already used implicitly)
│   │   ├── runge_kutta.rs RK4, RK45 (Dormand-Prince), adaptive step
│   │   ├── implicit.rs    Backward Euler (BDF1), BDF2 for stiff systems
│   │   └── verlet.rs      Velocity Verlet, Störmer-Verlet (symplectic)
│   ├── transforms/
│   │   ├── fft.rs         DFT, FFT (Cooley-Tukey), inverse FFT
│   │   └── laplace.rs     Numerical Laplace, s-domain utilities
│   ├── interpolation.rs   Linear, cubic spline, bilinear, trilinear
│   ├── statistics/
│   │   ├── distributions.rs  Gaussian, Poisson, exponential, uniform, Weibull
│   │   ├── regression.rs     Linear least-squares, polynomial fit
│   │   └── monte_carlo.rs    Importance sampling, Markov chain MC
│   └── optimization/
│       ├── gradient.rs    Gradient descent, Adam, BFGS
│       ├── root.rs        Newton-Raphson, bisection, secant method
│       └── linear.rs      Simplex method, LP utilities
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

**Key principle:** `numerics/ode` replaces all the hand-coded `euler` integrators
scattered through the existing modules.  When we add RK4 and BDF solvers, every
existing system (nuclear kinetics, thermal, battery) can opt into higher accuracy
by swapping one line.

---

## 5 — Phased Delivery Plan

### Phase A — Foundations (builds on everything) ★★★★★ Priority: CRITICAL

**Target: `numerics/`, extend `constants.rs` + `units.rs`**

A.1 — `numerics/ode/`
: Forward Euler (document existing), Velocity Verlet, RK4, RK45 (Dormand-Prince
  adaptive), BDF-1 (implicit Euler for stiff systems like nuclear kinetics)
: *Unlocks: every existing physics system can upgrade accuracy; ARC-1 PID
  integrators become 4× more accurate for free*

A.2 — `numerics/interpolation.rs`
: Linear, cubic spline, bilinear table lookups
: *Unlocks: feedforward coefficient tables from Workshop data, material property
  curves as a function of temperature*

A.3 — `numerics/statistics/distributions.rs`
: Gaussian, Poisson, uniform, Weibull
: *Unlocks: Monte Carlo uncertainty quantification, sensor noise models*

A.4 — `constants.rs` extensions
: Radiation constants (dose units, Q-values), spectral constants (hc, eV/nm),
  chemical standard enthalpies of formation (25 common compounds)
: *Needed by: kinetics, nuclear/decay, optics/photons*

A.5 — `units.rs` extensions
: Tesla (T), Henry (H), Farad (F), Siemens (S), Gray (Gy), Sievert (Sv),
  Becquerel (Bq), Lumen (lm), Candela/sr
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

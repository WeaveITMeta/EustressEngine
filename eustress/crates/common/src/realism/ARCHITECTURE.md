# Realism Physics System Architecture

## Overview

The Realism Physics System extends EustressEngine with physically accurate simulations grounded in fundamental laws of physics. It provides:

1. **Fundamental Laws** - Thermodynamics, Newtonian mechanics, conservation laws
2. **Particle ECS** - High-performance particle systems with physical properties
3. **Symbolic Math** - Real-time equation solving via Symbolica 1.0+
4. **Rune Scripting** - Dynamic, hot-reloadable physics scripting (0.14+ with WASM)
5. **Materials Science** - Stress, strain, fracture mechanics
6. **Fluid Dynamics** - Water, hydrodynamics, aerodynamics
7. **GPU Compute** - WGPU compute shaders for SPH (Bevy 0.17+ integration)
8. **Quantum Effects** - Bose-Einstein/Fermi-Dirac statistics

## Table of Contents

1. [Core Architecture](#core-architecture)
2. [ECS Components](#ecs-components)
3. [Fundamental Laws Module](#fundamental-laws-module)
4. [Symbolica Integration](#symbolica-integration)
5. [Rune API](#rune-api)
6. [Materials Science](#materials-science)
7. [Fluid Dynamics](#fluid-dynamics)
8. [Visualizers](#visualizers)
9. [Implementation Phases](#implementation-phases)

---

## Core Architecture

```
eustress-common/src/realism/
├── mod.rs                    # Module exports, RealismPlugin
├── constants.rs              # Physical constants (R, k_B, G, etc.)
├── units.rs                  # SI unit system with conversions
│
├── laws/                     # Fundamental Physics Laws
│   ├── mod.rs
│   ├── thermodynamics.rs     # PV=nRT, entropy, energy
│   ├── mechanics.rs          # F=ma, momentum, work-energy
│   ├── conservation.rs       # Mass, energy, momentum conservation
│   └── electromagnetism.rs   # (Future) Maxwell's equations
│
├── particles/                # Particle ECS System
│   ├── mod.rs
│   ├── components.rs         # Particle, Temperature, Pressure, etc.
│   ├── systems.rs            # Update systems (parallel via Rayon)
│   ├── spawner.rs            # Particle emission
│   └── spatial.rs            # Spatial hashing for neighbor queries
│
├── symbolic/                 # Symbolica Integration
│   ├── mod.rs
│   ├── solver.rs             # Real-time equation solver
│   ├── expressions.rs        # Pre-compiled physics expressions
│   └── codegen.rs            # Runtime code generation
│
├── scripting/                # Rune API Layer
│   ├── mod.rs
│   ├── api.rs                # Exposed functions/types
│   ├── bindings.rs           # ECS <-> Rune bindings
│   └── hot_reload.rs         # Script hot-reloading
│
├── materials/                # Materials Science
│   ├── mod.rs
│   ├── properties.rs         # MaterialProperties component
│   ├── stress_strain.rs      # Stress/strain calculations
│   ├── fracture.rs           # Fracture mechanics
│   └── deformation.rs        # Elastic/plastic deformation
│
├── fluids/                   # Fluid Dynamics
│   ├── mod.rs
│   ├── sph.rs                # Smoothed Particle Hydrodynamics
│   ├── navier_stokes.rs      # Navier-Stokes solver
│   ├── water.rs              # Water simulation
│   ├── aerodynamics.rs       # Lift, drag, turbulence
│   └── buoyancy.rs           # Buoyancy forces
│
├── visualizers/              # Real-time Property Display
│   ├── mod.rs
│   ├── property_overlay.rs   # T, P, V, U, S overlays
│   ├── vector_field.rs       # Force/velocity field viz
│   ├── heat_map.rs           # Temperature gradients
│   └── stress_viz.rs         # Stress tensor visualization
│
├── gpu/                      # GPU Compute (WGPU)
│   ├── mod.rs
│   ├── pipeline.rs           # Compute pipeline setup
│   ├── buffers.rs            # GPU buffer management
│   └── shaders.rs            # WGSL compute shaders
│
└── quantum/                  # Quantum Effects
    ├── mod.rs
    ├── statistics.rs         # BE/FD distributions
    └── condensates.rs        # Bose-Einstein condensates
```

---

## Bevy 0.17+ / Avian 0.4 Integration

### Observer Pattern (Bevy 0.17+)

Bevy 0.17 overhauls the observer system. Use observers for physics events:

```rust
// Register physics event observers
app.add_observer(on_collision_start)
   .add_observer(on_fracture_event)
   .add_observer(on_phase_transition);

fn on_collision_start(trigger: Trigger<CollisionStarted>, query: Query<&MaterialProperties>) {
    // Handle collision with material properties
}

fn on_fracture_event(trigger: Trigger<FractureEvent>, mut commands: Commands) {
    // Spawn fragments, play effects
}
```

### Avian 0.4 SIMD Improvements

Avian 0.4 adds SIMD optimizations. Integrate realism forces via `ExternalForce`:

```rust
fn sync_realism_to_avian(
    mut avian_query: Query<(&mut ExternalForce, &Transform), With<RigidBody>>,
    realism_query: Query<(Entity, &KineticState, &AerodynamicBody)>,
) {
    for (entity, kinetic, aero) in realism_query.iter() {
        if let Ok((mut ext_force, transform)) = avian_query.get_mut(entity) {
            // Apply aerodynamic drag
            let drag = drag_force(1.225, kinetic.velocity, aero.drag_coefficient, aero.drag_area);
            ext_force.apply_force(drag);
            
            // Apply buoyancy if in water
            // ...
        }
    }
}
```

### Contact Events (Peck-style)

For detailed contact handling similar to Peck:

```rust
#[derive(Event)]
pub struct DetailedContact {
    pub entity_a: Entity,
    pub entity_b: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    pub impulse: f32,
    pub penetration: f32,
}

fn process_contacts(
    mut contacts: EventReader<DetailedContact>,
    mut stress_query: Query<&mut StressTensor>,
) {
    for contact in contacts.read() {
        // Apply contact stress to materials
        if let Ok(mut stress) = stress_query.get_mut(contact.entity_a) {
            let contact_stress = contact.impulse / (contact.penetration.max(0.001));
            stress.set(0, 0, stress.normal(0) + contact_stress);
            stress.update_invariants();
        }
    }
}
```

---

## ECS Components

### Core Particle Components

```rust
/// Physical particle with thermodynamic properties
#[derive(Component, Reflect, Clone, Debug)]
pub struct Particle {
    pub mass: f32,           // kg
    pub radius: f32,         // m
    pub particle_type: ParticleType,
}

/// Thermodynamic state
#[derive(Component, Reflect, Clone, Debug)]
pub struct ThermodynamicState {
    pub temperature: f32,    // K (Kelvin)
    pub pressure: f32,       // Pa (Pascals)
    pub volume: f32,         // m³
    pub internal_energy: f32, // J (Joules)
    pub entropy: f32,        // J/K
    pub enthalpy: f32,       // J
}

/// Kinetic state (velocity, momentum)
#[derive(Component, Reflect, Clone, Debug)]
pub struct KineticState {
    pub velocity: Vec3,      // m/s
    pub momentum: Vec3,      // kg·m/s
    pub angular_velocity: Vec3, // rad/s
    pub angular_momentum: Vec3, // kg·m²/s
}

/// Material properties for stress/strain
#[derive(Component, Reflect, Clone, Debug)]
pub struct MaterialProperties {
    pub young_modulus: f32,      // Pa (elastic modulus)
    pub poisson_ratio: f32,      // dimensionless (0-0.5)
    pub yield_strength: f32,     // Pa
    pub ultimate_strength: f32,  // Pa
    pub fracture_toughness: f32, // Pa·√m
    pub thermal_conductivity: f32, // W/(m·K)
    pub specific_heat: f32,      // J/(kg·K)
    pub density: f32,            // kg/m³
}

/// Stress tensor (3x3 symmetric)
#[derive(Component, Reflect, Clone, Debug)]
pub struct StressTensor {
    pub components: [[f32; 3]; 3], // σ_ij
    pub von_mises: f32,            // Equivalent stress
    pub principal: [f32; 3],       // Principal stresses
}

/// Strain tensor (3x3 symmetric)
#[derive(Component, Reflect, Clone, Debug)]
pub struct StrainTensor {
    pub components: [[f32; 3]; 3], // ε_ij
    pub volumetric: f32,           // Volumetric strain
    pub deviatoric: f32,           // Deviatoric strain
}

/// Fluid particle (SPH)
#[derive(Component, Reflect, Clone, Debug)]
pub struct FluidParticle {
    pub density: f32,        // kg/m³
    pub viscosity: f32,      // Pa·s
    pub surface_tension: f32, // N/m
    pub phase: FluidPhase,   // Liquid, Gas, Solid
}

/// Aerodynamic properties
#[derive(Component, Reflect, Clone, Debug)]
pub struct AerodynamicBody {
    pub drag_coefficient: f32,    // C_d
    pub lift_coefficient: f32,    // C_l
    pub reference_area: f32,      // m²
    pub center_of_pressure: Vec3, // Local offset
}
```

### Bundles for Common Use Cases

```rust
/// Complete thermodynamic particle bundle
#[derive(Bundle)]
pub struct ThermodynamicParticleBundle {
    pub particle: Particle,
    pub thermo: ThermodynamicState,
    pub kinetic: KineticState,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
}

/// Structural element with material properties
#[derive(Bundle)]
pub struct StructuralElementBundle {
    pub material: MaterialProperties,
    pub stress: StressTensor,
    pub strain: StrainTensor,
    pub transform: Transform,
}

/// Fluid particle bundle (SPH)
#[derive(Bundle)]
pub struct FluidParticleBundle {
    pub particle: Particle,
    pub fluid: FluidParticle,
    pub kinetic: KineticState,
    pub thermo: ThermodynamicState,
    pub transform: Transform,
}
```

---

## Fundamental Laws Module

### Thermodynamics

```rust
/// Ideal Gas Law: PV = nRT
pub fn ideal_gas_pressure(n: f32, t: f32, v: f32) -> f32 {
    (n * constants::R * t) / v
}

/// First Law: ΔU = Q - W
pub fn internal_energy_change(heat_in: f32, work_out: f32) -> f32 {
    heat_in - work_out
}

/// Entropy change: ΔS = Q/T (reversible)
pub fn entropy_change_reversible(heat: f32, temperature: f32) -> f32 {
    heat / temperature
}

/// Carnot efficiency: η = 1 - T_cold/T_hot
pub fn carnot_efficiency(t_cold: f32, t_hot: f32) -> f32 {
    1.0 - (t_cold / t_hot)
}

/// Heat capacity at constant volume: C_v = (∂U/∂T)_V
/// For ideal monatomic gas: C_v = (3/2)nR
pub fn heat_capacity_monatomic(n: f32) -> f32 {
    1.5 * n * constants::R
}
```

### Newtonian Mechanics

```rust
/// Newton's Second Law: F = ma
pub fn force_from_acceleration(mass: f32, acceleration: Vec3) -> Vec3 {
    mass * acceleration
}

/// Gravitational force: F = Gm₁m₂/r²
pub fn gravitational_force(m1: f32, m2: f32, r: f32) -> f32 {
    (constants::G * m1 * m2) / (r * r)
}

/// Kinetic energy: KE = ½mv²
pub fn kinetic_energy(mass: f32, velocity: Vec3) -> f32 {
    0.5 * mass * velocity.length_squared()
}

/// Momentum: p = mv
pub fn momentum(mass: f32, velocity: Vec3) -> Vec3 {
    mass * velocity
}

/// Work: W = F·d
pub fn work(force: Vec3, displacement: Vec3) -> f32 {
    force.dot(displacement)
}

/// Impulse: J = FΔt = Δp
pub fn impulse(force: Vec3, dt: f32) -> Vec3 {
    force * dt
}
```

### Conservation Laws

```rust
/// Conservation of momentum (elastic collision)
pub fn elastic_collision_1d(
    m1: f32, v1: f32,
    m2: f32, v2: f32,
) -> (f32, f32) {
    let v1_final = ((m1 - m2) * v1 + 2.0 * m2 * v2) / (m1 + m2);
    let v2_final = ((m2 - m1) * v2 + 2.0 * m1 * v1) / (m1 + m2);
    (v1_final, v2_final)
}

/// Conservation of energy check
pub fn total_mechanical_energy(
    kinetic: f32,
    potential: f32,
) -> f32 {
    kinetic + potential
}
```

---

## Symbolica Integration

### Purpose

Symbolica enables:
1. **Symbolic derivation** of physics equations at compile-time
2. **Real-time solving** of constraint systems
3. **Code generation** for optimized numerical evaluation
4. **Exact arithmetic** avoiding floating-point drift

### Architecture

```rust
use symbolica::atom::Atom;
use symbolica::evaluate::{CompileOptions, CompiledEvaluator};

/// Pre-compiled physics expressions
pub struct PhysicsExpressions {
    /// Ideal gas: P = nRT/V
    pub ideal_gas: CompiledEvaluator,
    /// Kinetic energy: KE = 0.5*m*v²
    pub kinetic_energy: CompiledEvaluator,
    /// Gravitational potential: U = -GMm/r
    pub gravitational_potential: CompiledEvaluator,
    /// Stress-strain: σ = Eε
    pub hookes_law: CompiledEvaluator,
    /// Navier-Stokes momentum (simplified)
    pub ns_momentum: CompiledEvaluator,
}

impl PhysicsExpressions {
    pub fn compile() -> Self {
        // Compile at startup for fast runtime evaluation
        let ideal_gas = Atom::parse("n*R*T/V").unwrap();
        let ideal_gas_compiled = ideal_gas.compile(
            &["n", "R", "T", "V"],
            CompileOptions::default(),
        ).unwrap();
        
        // ... compile other expressions
        
        Self {
            ideal_gas: ideal_gas_compiled,
            // ...
        }
    }
    
    /// Evaluate ideal gas pressure
    pub fn eval_pressure(&self, n: f64, t: f64, v: f64) -> f64 {
        self.ideal_gas.evaluate(&[n, constants::R_F64, t, v])
    }
}

/// Real-time equation solver for constraints
pub struct ConstraintSolver {
    /// Symbolic system of equations
    system: Vec<Atom>,
    /// Variables to solve for
    unknowns: Vec<String>,
}

impl ConstraintSolver {
    /// Solve system for unknowns given known values
    pub fn solve(&self, knowns: &HashMap<String, f64>) -> Result<HashMap<String, f64>, SolveError> {
        // Use Symbolica's linear/nonlinear solvers
        // ...
    }
}
```

### Usage in ECS Systems

```rust
fn update_thermodynamics(
    expressions: Res<PhysicsExpressions>,
    mut query: Query<&mut ThermodynamicState>,
) {
    query.par_iter_mut().for_each(|mut state| {
        // Real-time symbolic evaluation
        state.pressure = expressions.eval_pressure(
            state.moles(),
            state.temperature,
            state.volume,
        ) as f32;
    });
}
```

---

## Rune API

### Purpose

Rune provides:
1. **Hot-reloadable** physics scripts
2. **DSL** for defining custom laws and behaviors
3. **Safe sandboxing** for user-generated content
4. **Real-time interactivity** without recompilation

### Module Structure

```rust
use rune::{Context, Module, Vm};
use rune::runtime::{Function, VmError};

/// Rune module exposing physics API
pub fn physics_module() -> Result<Module, rune::ContextError> {
    let mut module = Module::with_crate("physics")?;
    
    // Constants
    module.constant("R", constants::R)?;
    module.constant("G", constants::G)?;
    module.constant("K_B", constants::K_B)?;
    
    // Thermodynamics
    module.function("ideal_gas_pressure", laws::thermodynamics::ideal_gas_pressure)?;
    module.function("entropy_change", laws::thermodynamics::entropy_change_reversible)?;
    module.function("carnot_efficiency", laws::thermodynamics::carnot_efficiency)?;
    
    // Mechanics
    module.function("kinetic_energy", |mass: f32, vx: f32, vy: f32, vz: f32| {
        laws::mechanics::kinetic_energy(mass, Vec3::new(vx, vy, vz))
    })?;
    module.function("gravitational_force", laws::mechanics::gravitational_force)?;
    
    // Materials
    module.function("von_mises_stress", materials::stress_strain::von_mises)?;
    module.function("yield_check", materials::fracture::check_yield)?;
    
    // Fluids
    module.function("drag_force", fluids::aerodynamics::drag_force)?;
    module.function("buoyancy", fluids::buoyancy::archimedes_force)?;
    
    Ok(module)
}

/// Rune module for entity queries
pub fn entity_module() -> Result<Module, rune::ContextError> {
    let mut module = Module::with_crate("entity")?;
    
    // Query functions (bound at runtime to ECS world)
    module.function("get_temperature", |entity_id: u64| -> f32 { 0.0 })?; // Placeholder
    module.function("set_temperature", |entity_id: u64, temp: f32| {})?;
    module.function("get_pressure", |entity_id: u64| -> f32 { 0.0 })?;
    module.function("apply_force", |entity_id: u64, fx: f32, fy: f32, fz: f32| {})?;
    
    Ok(module)
}
```

### Example Rune Script

```rune
// custom_physics.rn - Hot-reloadable physics behavior

use physics::{ideal_gas_pressure, kinetic_energy, R};
use entity::{get_temperature, set_temperature, apply_force};

/// Custom thermal expansion behavior
pub fn thermal_expansion(entity_id, heat_input) {
    let current_temp = entity::get_temperature(entity_id);
    let new_temp = current_temp + heat_input / 1000.0;
    entity::set_temperature(entity_id, new_temp);
    
    // Calculate pressure change
    let pressure = physics::ideal_gas_pressure(1.0, new_temp, 1.0);
    
    // Apply expansion force if pressure exceeds threshold
    if pressure > 101325.0 * 1.5 {
        entity::apply_force(entity_id, 0.0, pressure * 0.001, 0.0);
    }
}

/// Custom aerodynamic behavior
pub fn wind_effect(entity_id, wind_velocity) {
    let drag = physics::drag_force(1.225, wind_velocity, 0.47, 1.0);
    entity::apply_force(entity_id, drag, 0.0, 0.0);
}
```

### Hot-Reload System

```rust
/// Rune script hot-reload resource
#[derive(Resource)]
pub struct RuneScriptManager {
    context: Context,
    scripts: HashMap<String, CompiledScript>,
    watcher: Option<notify::RecommendedWatcher>,
}

impl RuneScriptManager {
    /// Reload a script from disk
    pub fn reload(&mut self, path: &Path) -> Result<(), RuneError> {
        let source = std::fs::read_to_string(path)?;
        let unit = rune::prepare(&mut self.context)
            .with_source(Source::new(path.to_str().unwrap(), &source)?)
            .build()?;
        
        self.scripts.insert(
            path.to_string_lossy().to_string(),
            CompiledScript { unit, last_modified: SystemTime::now() },
        );
        
        info!("Hot-reloaded physics script: {}", path.display());
        Ok(())
    }
    
    /// Execute a function from a script
    pub fn call<T>(&self, script: &str, function: &str, args: impl Args) -> Result<T, VmError>
    where
        T: FromValue,
    {
        let script = self.scripts.get(script).ok_or(RuneError::NotFound)?;
        let mut vm = Vm::new(self.context.runtime()?, Arc::clone(&script.unit));
        vm.call([function], args)?.into_result()
    }
}
```

---

## Materials Science

### Stress-Strain Calculations

```rust
/// Calculate stress from strain using Hooke's Law (linear elastic)
pub fn hookes_law_3d(
    strain: &StrainTensor,
    young_modulus: f32,
    poisson_ratio: f32,
) -> StressTensor {
    let lambda = (young_modulus * poisson_ratio) 
        / ((1.0 + poisson_ratio) * (1.0 - 2.0 * poisson_ratio));
    let mu = young_modulus / (2.0 * (1.0 + poisson_ratio));
    
    let trace = strain.volumetric;
    let mut stress = [[0.0f32; 3]; 3];
    
    for i in 0..3 {
        for j in 0..3 {
            stress[i][j] = 2.0 * mu * strain.components[i][j];
            if i == j {
                stress[i][j] += lambda * trace;
            }
        }
    }
    
    StressTensor::from_components(stress)
}

/// Von Mises equivalent stress
pub fn von_mises(stress: &StressTensor) -> f32 {
    let s = &stress.components;
    let term1 = (s[0][0] - s[1][1]).powi(2) 
              + (s[1][1] - s[2][2]).powi(2) 
              + (s[2][2] - s[0][0]).powi(2);
    let term2 = 6.0 * (s[0][1].powi(2) + s[1][2].powi(2) + s[2][0].powi(2));
    
    ((term1 + term2) / 2.0).sqrt()
}
```

### Fracture Mechanics

```rust
/// Check if material has yielded (plastic deformation begins)
pub fn check_yield(von_mises_stress: f32, yield_strength: f32) -> bool {
    von_mises_stress >= yield_strength
}

/// Check for fracture using stress intensity factor
pub fn check_fracture(
    stress: f32,
    crack_length: f32,
    fracture_toughness: f32, // K_IC
) -> bool {
    let k_i = stress * (std::f32::consts::PI * crack_length).sqrt();
    k_i >= fracture_toughness
}

/// Griffith criterion for brittle fracture
pub fn griffith_critical_stress(
    surface_energy: f32,
    young_modulus: f32,
    crack_length: f32,
) -> f32 {
    (2.0 * surface_energy * young_modulus / (std::f32::consts::PI * crack_length)).sqrt()
}

/// Fracture event component
#[derive(Component)]
pub struct FractureState {
    pub cracks: Vec<Crack>,
    pub is_fractured: bool,
    pub fracture_time: Option<f32>,
}

pub struct Crack {
    pub position: Vec3,
    pub direction: Vec3,
    pub length: f32,
    pub growth_rate: f32,
}
```

---

## Fluid Dynamics

### Smoothed Particle Hydrodynamics (SPH)

```rust
/// SPH kernel function (cubic spline)
pub fn cubic_spline_kernel(r: f32, h: f32) -> f32 {
    let q = r / h;
    let sigma = 8.0 / (std::f32::consts::PI * h.powi(3));
    
    if q <= 0.5 {
        sigma * (6.0 * (q.powi(3) - q.powi(2)) + 1.0)
    } else if q <= 1.0 {
        sigma * 2.0 * (1.0 - q).powi(3)
    } else {
        0.0
    }
}

/// SPH density estimation
pub fn sph_density(
    position: Vec3,
    neighbors: &[(Vec3, f32)], // (position, mass)
    smoothing_length: f32,
) -> f32 {
    neighbors.iter()
        .map(|(pos, mass)| {
            let r = (position - *pos).length();
            mass * cubic_spline_kernel(r, smoothing_length)
        })
        .sum()
}

/// SPH pressure force
pub fn sph_pressure_force(
    particle: &FluidParticle,
    position: Vec3,
    neighbors: &[(Vec3, f32, f32)], // (pos, mass, pressure)
    smoothing_length: f32,
) -> Vec3 {
    let mut force = Vec3::ZERO;
    
    for (pos, mass, pressure) in neighbors {
        let r_vec = position - *pos;
        let r = r_vec.length();
        if r > 0.0 && r < smoothing_length {
            let grad_w = cubic_spline_gradient(r_vec, smoothing_length);
            let pressure_term = (particle.pressure() + pressure) / (2.0 * particle.density);
            force -= mass * pressure_term * grad_w;
        }
    }
    
    force
}
```

### Navier-Stokes (Simplified Grid-Based)

```rust
/// Navier-Stokes momentum equation (incompressible)
/// ∂u/∂t + (u·∇)u = -∇p/ρ + ν∇²u + f
pub struct NavierStokesSolver {
    pub grid_size: UVec3,
    pub cell_size: f32,
    pub velocity: Vec<Vec3>,
    pub pressure: Vec<f32>,
    pub density: f32,
    pub viscosity: f32,
}

impl NavierStokesSolver {
    /// Advection step: (u·∇)u
    pub fn advect(&mut self, dt: f32) {
        // Semi-Lagrangian advection
        // ...
    }
    
    /// Diffusion step: ν∇²u
    pub fn diffuse(&mut self, dt: f32) {
        // Jacobi iteration for diffusion
        // ...
    }
    
    /// Pressure projection: enforce ∇·u = 0
    pub fn project(&mut self) {
        // Solve Poisson equation for pressure
        // Apply pressure gradient to velocity
        // ...
    }
    
    /// Full timestep
    pub fn step(&mut self, dt: f32, external_forces: &[Vec3]) {
        self.advect(dt);
        self.diffuse(dt);
        self.apply_forces(external_forces, dt);
        self.project();
    }
}
```

### Aerodynamics

```rust
/// Drag force: F_d = ½ρv²C_dA
pub fn drag_force(
    density: f32,      // Air density (kg/m³)
    velocity: Vec3,    // Relative velocity
    drag_coeff: f32,   // Drag coefficient
    area: f32,         // Reference area (m²)
) -> Vec3 {
    let speed = velocity.length();
    if speed < 0.001 {
        return Vec3::ZERO;
    }
    
    let magnitude = 0.5 * density * speed * speed * drag_coeff * area;
    -velocity.normalize() * magnitude
}

/// Lift force: F_l = ½ρv²C_lA
pub fn lift_force(
    density: f32,
    velocity: Vec3,
    lift_coeff: f32,
    area: f32,
    up_direction: Vec3,
) -> Vec3 {
    let speed = velocity.length();
    if speed < 0.001 {
        return Vec3::ZERO;
    }
    
    let magnitude = 0.5 * density * speed * speed * lift_coeff * area;
    up_direction.normalize() * magnitude
}

/// Reynolds number: Re = ρvL/μ
pub fn reynolds_number(
    density: f32,
    velocity: f32,
    characteristic_length: f32,
    dynamic_viscosity: f32,
) -> f32 {
    density * velocity * characteristic_length / dynamic_viscosity
}
```

---

## Visualizers

### Property Overlay System

```rust
/// Real-time property display overlay
#[derive(Component)]
pub struct PropertyOverlay {
    pub show_temperature: bool,
    pub show_pressure: bool,
    pub show_velocity: bool,
    pub show_stress: bool,
    pub show_entropy: bool,
}

/// System to render property overlays in egui
pub fn render_property_overlays(
    mut egui_ctx: EguiContexts,
    query: Query<(Entity, &ThermodynamicState, &Transform, Option<&StressTensor>)>,
    camera: Query<(&Camera, &GlobalTransform)>,
    overlay_settings: Res<OverlaySettings>,
) {
    let (camera, camera_transform) = camera.single();
    
    for (entity, thermo, transform, stress) in query.iter() {
        // Project 3D position to screen
        if let Some(screen_pos) = camera.world_to_viewport(camera_transform, transform.translation) {
            egui::Area::new(format!("props_{:?}", entity))
                .fixed_pos([screen_pos.x, screen_pos.y])
                .show(egui_ctx.ctx_mut(), |ui| {
                    ui.vertical(|ui| {
                        if overlay_settings.show_temperature {
                            ui.label(format!("T: {:.1} K", thermo.temperature));
                        }
                        if overlay_settings.show_pressure {
                            ui.label(format!("P: {:.0} Pa", thermo.pressure));
                        }
                        if overlay_settings.show_entropy {
                            ui.label(format!("S: {:.2} J/K", thermo.entropy));
                        }
                        if let Some(s) = stress {
                            if overlay_settings.show_stress {
                                ui.label(format!("σ_vm: {:.0} MPa", s.von_mises / 1e6));
                            }
                        }
                    });
                });
        }
    }
}
```

### Vector Field Visualization

```rust
/// Velocity/force field visualization
pub fn render_vector_field(
    mut gizmos: Gizmos,
    query: Query<(&Transform, &KineticState)>,
    settings: Res<VectorFieldSettings>,
) {
    for (transform, kinetic) in query.iter() {
        let start = transform.translation;
        let end = start + kinetic.velocity * settings.scale;
        
        // Color by magnitude
        let speed = kinetic.velocity.length();
        let color = velocity_to_color(speed, settings.max_speed);
        
        gizmos.arrow(start, end, color);
    }
}

fn velocity_to_color(speed: f32, max_speed: f32) -> Color {
    let t = (speed / max_speed).clamp(0.0, 1.0);
    // Blue (slow) -> Green -> Yellow -> Red (fast)
    Color::hsl(240.0 - t * 240.0, 1.0, 0.5)
}
```

---

## Implementation Phases

### Phase 1: Foundation (Week 1-2)
- [ ] Create `realism` module structure
- [ ] Implement `constants.rs` and `units.rs`
- [ ] Define core ECS components
- [ ] Implement fundamental laws (thermodynamics, mechanics)
- [ ] Add to workspace Cargo.toml

### Phase 2: Symbolica Integration (Week 3)
- [ ] Add Symbolica dependency
- [ ] Create pre-compiled physics expressions
- [ ] Implement constraint solver
- [ ] Benchmark symbolic vs direct computation

### Phase 3: Rune Scripting (Week 4)
- [ ] Add Rune dependency
- [ ] Create physics module bindings
- [ ] Implement entity query bindings
- [ ] Build hot-reload system
- [ ] Create example scripts

### Phase 4: Materials Science (Week 5)
- [ ] Implement stress-strain calculations
- [ ] Add fracture mechanics
- [ ] Create deformation system
- [ ] Integrate with Avian3D constraints

### Phase 5: Fluid Dynamics (Week 6-7)
- [ ] Implement SPH core
- [ ] Add water simulation
- [ ] Implement aerodynamics
- [ ] Create Navier-Stokes solver (grid-based)

### Phase 6: Visualizers (Week 8)
- [ ] Property overlay system
- [ ] Vector field visualization
- [ ] Heat map rendering
- [ ] Stress tensor visualization
- [ ] Studio Engine integration

---

## Dependencies

Add to `eustress/Cargo.toml`:

```toml
[workspace.dependencies]
# Symbolic Mathematics
symbolica = "0.14"

# Rune Scripting
rune = "0.14"
rune-modules = "0.14"

# Spatial data structures
kiddo = "4.2"  # KD-tree for neighbor queries

# Linear algebra (already have nalgebra via Avian)
nalgebra = "0.33"
```

Add to `eustress-common/Cargo.toml`:

```toml
[dependencies]
symbolica = { workspace = true, optional = true }
rune = { workspace = true, optional = true }
rune-modules = { workspace = true, optional = true }
kiddo = { workspace = true, optional = true }

[features]
default = ["physics"]
realism = ["symbolica", "rune", "rune-modules", "kiddo"]
```

---

## Integration with Avian3D

The realism system extends Avian3D rather than replacing it:

```rust
/// Bridge between realism and Avian3D
pub fn sync_avian_forces(
    mut avian_query: Query<&mut ExternalForce, With<RigidBody>>,
    realism_query: Query<(Entity, &KineticState, &AerodynamicBody)>,
) {
    for (entity, kinetic, aero) in realism_query.iter() {
        if let Ok(mut ext_force) = avian_query.get_mut(entity) {
            // Apply aerodynamic forces to Avian
            let drag = drag_force(1.225, kinetic.velocity, aero.drag_coefficient, aero.reference_area);
            let lift = lift_force(1.225, kinetic.velocity, aero.lift_coefficient, aero.reference_area, Vec3::Y);
            
            ext_force.apply_force(drag + lift);
        }
    }
}
```

---

## Performance Considerations

1. **Parallel Processing**: Use Rayon for particle systems via `par_iter_mut()`
2. **Spatial Hashing**: Use `kiddo` KD-tree for O(log n) neighbor queries
3. **Symbolica Codegen**: Pre-compile expressions at startup, not runtime
4. **LOD for Fluids**: Reduce particle count at distance
5. **GPU Compute**: Future work - compute shaders for SPH

---

## API Summary

### Rust API
```rust
use eustress_common::realism::prelude::*;

// Spawn thermodynamic particle
commands.spawn(ThermodynamicParticleBundle {
    particle: Particle { mass: 1.0, radius: 0.1, particle_type: ParticleType::Gas },
    thermo: ThermodynamicState::ideal_gas(1.0, 300.0, 0.001),
    kinetic: KineticState::default(),
    ..default()
});

// Query and update
for mut state in thermo_query.iter_mut() {
    state.pressure = ideal_gas_pressure(state.moles(), state.temperature, state.volume);
}
```

### Rune API
```rune
use physics::{ideal_gas_pressure, drag_force};
use entity::{get_temperature, apply_force};

pub fn update_particle(id) {
    let temp = entity::get_temperature(id);
    let pressure = physics::ideal_gas_pressure(1.0, temp, 0.001);
    
    if pressure > 200000.0 {
        entity::apply_force(id, 0.0, 100.0, 0.0);
    }
}
```

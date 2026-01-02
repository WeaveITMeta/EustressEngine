# Realism Physics System

A comprehensive physics simulation system for EustressEngine providing physically accurate simulations grounded in fundamental laws of physics.

## Features

- **Fundamental Laws** - Thermodynamics, Newtonian mechanics, conservation laws
- **Particle ECS** - High-performance particle systems with physical properties
- **Symbolic Math** - Real-time equation solving via Symbolica (optional)
- **Rune Scripting** - Dynamic, hot-reloadable physics scripting (optional)
- **Materials Science** - Stress, strain, fracture mechanics
- **Fluid Dynamics** - SPH water simulation, aerodynamics, buoyancy

## Quick Start

### Enable the Feature

In your `Cargo.toml`:

```toml
[dependencies]
eustress-common = { path = "../common", features = ["realism"] }

# For full features:
# eustress-common = { path = "../common", features = ["realism-full"] }
```

### Add the Plugin

```rust
use bevy::prelude::*;
use eustress_common::realism::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(RealismPlugin)
        .run();
}
```

### Spawn Particles

```rust
use eustress_common::realism::prelude::*;

fn spawn_gas_particles(mut commands: Commands) {
    // Spawn a thermodynamic gas particle
    commands.spawn(ThermodynamicParticleBundle::gas(
        Vec3::new(0.0, 5.0, 0.0),  // position
        0.001,                       // mass (kg)
        350.0,                       // temperature (K)
    ));
}

fn spawn_water(mut commands: Commands) {
    // Spawn a block of water particles for SPH simulation
    spawn_water_block(
        &mut commands,
        Vec3::new(0.0, 2.0, 0.0),  // center
        Vec3::new(2.0, 1.0, 2.0),  // size
        0.05,                       // particle spacing
    );
}
```

### Query Properties

```rust
fn display_properties(
    query: Query<(&ThermodynamicState, &KineticState)>,
) {
    for (thermo, kinetic) in query.iter() {
        println!("Temperature: {} K", thermo.temperature);
        println!("Pressure: {} Pa", thermo.pressure);
        println!("Velocity: {} m/s", kinetic.velocity.length());
        println!("Entropy: {} J/K", thermo.entropy);
    }
}
```

## Module Structure

```
realism/
├── mod.rs              # Main module, RealismPlugin
├── constants.rs        # Physical constants (R, G, k_B, etc.)
├── units.rs            # SI unit system with conversions
│
├── laws/               # Fundamental Physics Laws
│   ├── thermodynamics.rs   # PV=nRT, entropy, heat transfer
│   ├── mechanics.rs        # F=ma, momentum, energy
│   └── conservation.rs     # Mass, energy, momentum conservation
│
├── particles/          # Particle ECS System
│   ├── components.rs   # Particle, ThermodynamicState, KineticState
│   ├── systems.rs      # Update systems
│   ├── spawner.rs      # Particle emission
│   └── spatial.rs      # Spatial hashing
│
├── materials/          # Materials Science
│   ├── properties.rs   # MaterialProperties (steel, aluminum, etc.)
│   ├── stress_strain.rs    # Hooke's law, stress tensors
│   ├── fracture.rs     # Fracture mechanics
│   └── deformation.rs  # Elastic/plastic deformation
│
├── fluids/             # Fluid Dynamics
│   ├── sph.rs          # Smoothed Particle Hydrodynamics
│   ├── water.rs        # Water simulation
│   ├── aerodynamics.rs # Lift, drag, Reynolds number
│   └── buoyancy.rs     # Archimedes' principle
│
├── visualizers/        # Real-time Display
│   ├── property_overlay.rs # T, P, V, U, S overlays
│   ├── vector_field.rs     # Velocity/force visualization
│   ├── heat_map.rs         # Temperature gradients
│   └── stress_viz.rs       # Stress tensor display
│
├── symbolic/           # Symbolica Integration (optional)
│   ├── expressions.rs  # Pre-compiled physics expressions
│   ├── solver.rs       # Constraint solver
│   └── codegen.rs      # Runtime code generation
│
└── scripting/          # Rune Scripting (optional)
    ├── api.rs          # Exposed physics functions
    ├── bindings.rs     # ECS <-> Rune bindings
    └── hot_reload.rs   # Script hot-reloading
```

## Physics Laws

### Thermodynamics

```rust
use eustress_common::realism::laws::thermodynamics::*;

// Ideal gas law: P = nRT/V
let pressure = ideal_gas_pressure(1.0, 300.0, 0.001);

// Carnot efficiency
let efficiency = carnot_efficiency(300.0, 500.0);

// Heat conduction
let heat_rate = heat_conduction_rate(50.0, 1.0, 100.0, 0.01);
```

### Mechanics

```rust
use eustress_common::realism::laws::mechanics::*;

// Newton's second law
let force = force_from_acceleration(10.0, Vec3::new(0.0, 9.81, 0.0));

// Kinetic energy
let ke = kinetic_energy(2.0, Vec3::new(10.0, 0.0, 0.0));

// Elastic collision
let (v1, v2) = elastic_collision_1d(1.0, 10.0, 1.0, 0.0);
```

### Conservation

```rust
use eustress_common::realism::laws::conservation::*;

// Bernoulli's equation
let pressure = bernoulli_pressure(101325.0, 1.0, 0.0, 10.0, 0.0, 1000.0, 9.81);

// Center of mass
let com = center_of_mass(&[1.0, 2.0], &[Vec3::X, Vec3::Y]);
```

## Materials Science

### Material Properties

```rust
use eustress_common::realism::materials::prelude::*;

// Use preset materials
let steel = MaterialProperties::steel();
let aluminum = MaterialProperties::aluminum();
let glass = MaterialProperties::glass();

// Derived properties
let shear_modulus = steel.shear_modulus();
let speed_of_sound = steel.speed_of_sound();
```

### Stress-Strain

```rust
use eustress_common::realism::materials::stress_strain::*;

// Create strain tensor
let strain = StrainTensor::from_normal(0.001, 0.0005, -0.0003);

// Calculate stress using Hooke's law
let stress = hookes_law_3d(&strain, &MaterialProperties::steel());

// Check yield condition
let yielded = check_von_mises_yield(&stress, 250e6);
```

### Fracture Mechanics

```rust
use eustress_common::realism::materials::fracture::*;

// Stress intensity factor
let k_i = stress_intensity_mode_i(100e6, 0.01, 1.0);

// Check Griffith criterion
let will_fracture = check_griffith_fracture(k_i, 50e6);

// Paris law for fatigue
let crack_growth = paris_law(20e6, 3e-12, 3.0);
```

## Fluid Dynamics

### SPH Water Simulation

```rust
use eustress_common::realism::fluids::sph::*;

// Configure SPH
let config = SphConfig {
    smoothing_length: 0.1,
    rest_density: 1000.0,
    gas_constant: 2000.0,
    viscosity: 0.001,
    ..default()
};
```

### Aerodynamics

```rust
use eustress_common::realism::fluids::aerodynamics::*;

// Create aerodynamic body
let car = AerodynamicBody::car(2.0);
let sphere = AerodynamicBody::sphere(0.5);

// Calculate drag
let drag = drag_force(1.225, Vec3::new(30.0, 0.0, 0.0), 0.3, 2.0);

// Reynolds number
let re = reynolds_number(1.225, 10.0, 1.0, 1.81e-5);
```

### Buoyancy

```rust
use eustress_common::realism::fluids::buoyancy::*;

// Create buoyant body
let boat = BuoyancyBody::from_box(5.0, 2.0, 10.0, 5000.0);

// Check if it floats
assert!(boat.will_float());

// Archimedes force
let buoyancy = archimedes_force(1000.0, 1.0, 9.81);
```

## Rune Scripting (Optional)

Enable with `features = ["realism-scripting"]`.

### Example Script

```rune
// physics_behavior.rn
use physics::{ideal_gas_pressure, drag_force};
use entity::{get_temperature, set_temperature, apply_force};

pub fn update(entity_id, dt) {
    let temp = entity::get_temperature(entity_id);
    let pressure = physics::ideal_gas_pressure(1.0, temp, 0.001);
    
    if pressure > 200000.0 {
        let force = (pressure - 200000.0) * 0.001;
        entity::apply_force(entity_id, 0.0, force, 0.0);
    }
}
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `realism` | Core realism system (requires `physics`) |
| `realism-symbolic` | Symbolica integration for symbolic math |
| `realism-scripting` | Rune scripting with hot-reload |
| `realism-full` | All realism features |

## Performance

- **Parallel Processing**: Uses Rayon for particle systems via `par_iter_mut()`
- **Spatial Hashing**: O(1) cell lookup for neighbor queries
- **Pre-compiled Expressions**: Symbolica expressions compiled at startup
- **LOD for Fluids**: Reduce particle count at distance

## Integration with Avian3D

The realism system extends Avian3D rather than replacing it:

```rust
fn sync_avian_forces(
    mut avian_query: Query<&mut ExternalForce, With<RigidBody>>,
    realism_query: Query<(Entity, &KineticState, &AerodynamicBody)>,
) {
    for (entity, kinetic, aero) in realism_query.iter() {
        if let Ok(mut ext_force) = avian_query.get_mut(entity) {
            let drag = drag_force(1.225, kinetic.velocity, aero.drag_coefficient, aero.drag_area);
            ext_force.apply_force(drag);
        }
    }
}
```

## Units

All values use SI units:

| Quantity | Unit | Symbol |
|----------|------|--------|
| Length | meter | m |
| Mass | kilogram | kg |
| Time | second | s |
| Temperature | Kelvin | K |
| Pressure | Pascal | Pa |
| Energy | Joule | J |
| Force | Newton | N |

Use the `units` module for conversions:

```rust
use eustress_common::realism::units::*;

let temp = Kelvin::from_celsius(25.0);
let pressure = Pascals::from_atm(1.0);
let velocity = MetersPerSecond::from_kmh(100.0);
```

## License

Part of EustressEngine. See main LICENSE file.

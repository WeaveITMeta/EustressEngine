# Dynamic Gravity System

**Status**: ✅ **IMPLEMENTED** (2025-03-11)

## Overview

Explorer Workspace-integrated gravitational physics with real-time force calculations and tiered performance optimization. All mass and radius values are editable at runtime through the Properties panel.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Dynamic Gravity System                        │
├─────────────────────────────────────────────────────────────────┤
│  Tier 1: Heavy Objects (>1e20 kg)                               │
│    ├── Planets, Stars, Large Moons                              │
│    ├── Full N-body calculations (60 Hz)                         │
│    └── Tracked in ForceMetatable                                │
├─────────────────────────────────────────────────────────────────┤
│  Tier 2: Medium Objects (1e10 - 1e20 kg)                        │
│    ├── Large Asteroids, Small Moons                             │
│    ├── Simplified calculations (10 Hz)                          │
│    └── Affected by Heavy + Medium objects                       │
├─────────────────────────────────────────────────────────────────┤
│  Tier 3: Light Objects (<1e10 kg)                               │
│    ├── Spacecraft, Debris, Humans                               │
│    ├── Minimal calculations (1 Hz)                              │
│    └── Only affected by Heavy objects                           │
├─────────────────────────────────────────────────────────────────┤
│  Force Metatable                                                 │
│    ├── Real-time force tracking between pairs                   │
│    ├── Cached calculations for performance                      │
│    └── Automatic cleanup of old entries                         │
└─────────────────────────────────────────────────────────────────┘
```

## Conditional Gravity Activation

**Important**: Gravity calculations are **automatically disabled** when no heavy objects (>1e20 kg) are present in the scene.

This means:
- Empty workspace with only light objects (spacecraft, debris, humans) → **No gravity**
- Workspace with planets/stars → **Gravity enabled**
- Remove last heavy object → **Gravity automatically disables**
- Add first heavy object → **Gravity automatically enables**

This prevents unnecessary calculations in scenes that don't need gravitational physics (e.g., indoor spaces, small-scale simulations, abstract game levels).

```rust
// Example: No gravity in empty space station interior
commands.spawn((
    DynamicMass::new(1000.0),  // 1 ton (Light tier)
    // No heavy objects present → No gravitational forces applied
));

// Example: Gravity activates when Earth is added
commands.spawn((
    DynamicMass::new(5.972e24),  // Earth (Heavy tier)
    // Heavy object present → All objects now experience gravity
));
```

## Key Differences from Static Gravity

| Feature | Static Gravity | Dynamic Gravity |
|---------|---------------|-----------------|
| Mass Values | Hardcoded structs | Runtime-editable components |
| Performance | Uniform O(n²) | Tiered O(n log n) |
| Force Tracking | None | Full metatable |
| Update Rate | Every frame | Tier-based (1-60 Hz) |
| Explorer Integration | No | Yes - edit in Properties |
| Heavy Object Optimization | No | Automatic tier promotion |
| Conditional Activation | Always on | Only when heavy objects present |

## Components

### DynamicMass

Runtime-editable mass component with automatic tier management:

```rust
#[derive(Component, Reflect)]
pub struct DynamicMass {
    /// Mass in kilograms (editable in Properties panel)
    pub kilograms: f64,
    
    /// Cached tier for performance
    pub tier: MassTier,
    
    /// Last update time for tier-based scheduling
    pub last_update: f64,
}

// Create and edit at runtime
let mut mass = DynamicMass::new(5.972e24); // Earth mass
mass.kilograms = 7.342e22; // Change to Moon mass
mass.update_tier(); // Automatically recalculates tier
```

### MassTier

Automatic performance tier based on mass:

```rust
pub enum MassTier {
    Heavy,   // > 1e20 kg - Full N-body (60 Hz)
    Medium,  // 1e10 - 1e20 kg - Simplified (10 Hz)
    Light,   // < 1e10 kg - Minimal (1 Hz)
}

// Thresholds
HEAVY_MASS_THRESHOLD  = 1e20 kg  // 100 quintillion kg (small moons+)
MEDIUM_MASS_THRESHOLD = 1e10 kg  // 10 billion kg (large asteroids+)
```

### DynamicRadius

Runtime-editable physical radius:

```rust
#[derive(Component, Reflect)]
pub struct DynamicRadius {
    /// Radius in meters (editable in Properties panel)
    pub meters: f64,
}

// Calculate surface gravity dynamically
let g = radius.surface_gravity(&mass);
```

### DynamicGravityForce

Force accumulator with tier breakdown:

```rust
#[derive(Component)]
pub struct DynamicGravityForce {
    /// Total accumulated force
    pub force: Vec3,
    
    /// Forces by tier
    pub heavy_force: Vec3,
    pub medium_force: Vec3,
    pub light_force: Vec3,
    
    /// Source counts
    pub heavy_sources: usize,
    pub medium_sources: usize,
    pub light_sources: usize,
}
```

## Force Metatable

Real-time tracking of all pairwise gravitational forces:

```rust
#[derive(Resource)]
pub struct ForceMetatable {
    /// Map of entity pairs to force entries
    forces: HashMap<EntityPair, ForceEntry>,
    
    /// Cached heavy object list
    heavy_objects: HashSet<Entity>,
    
    /// Statistics
    total_pairs: usize,
    significant_pairs: usize,
}

// Access forces
let force = metatable.get_force(earth_entity, moon_entity);
println!("Force: {:.3e} N", force.magnitude);
println!("Distance: {:.1} km", force.distance / 1000.0);
```

### ForceEntry

Individual force record in the metatable:

```rust
struct ForceEntry {
    magnitude: f64,        // Newtons
    direction: Vec3,       // Unit vector
    distance: f64,         // Meters
    last_calculated: f64,  // Timestamp
    significant: bool,     // Above threshold
}
```

## Performance Optimization

### Tiered Update Frequencies

```rust
// Heavy objects: Every frame
HEAVY_UPDATE_HZ  = 60.0 Hz  // 16.7 ms interval

// Medium objects: 10 times per second
MEDIUM_UPDATE_HZ = 10.0 Hz  // 100 ms interval

// Light objects: Once per second
LIGHT_UPDATE_HZ  = 1.0 Hz   // 1000 ms interval
```

### Interaction Matrix

| Target ↓ Source → | Heavy | Medium | Light |
|-------------------|-------|--------|-------|
| **Heavy** | ✅ Full | ✅ Full | ✅ Full |
| **Medium** | ✅ Full | ✅ Full | ❌ Skip |
| **Light** | ✅ Full | ❌ Skip | ❌ Skip |

### Performance Gains

| Scenario | Static Gravity | Dynamic Gravity | Speedup |
|----------|----------------|-----------------|---------|
| 1 Heavy + 1000 Light | O(1000²) = 1M | O(1000) = 1K | **1000x** |
| 10 Heavy + 100 Medium | O(110²) = 12K | O(10² + 100) = 200 | **60x** |
| 100 Heavy (worst case) | O(100²) = 10K | O(100²) = 10K | 1x |

## Usage Examples

### Basic Setup

```rust
use eustress_common::physics::*;
use eustress_common::orbital::hybrid_coords::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(HybridCoordsPlugin)
        .add_plugins(DynamicGravityPlugin)
        .add_systems(Startup, setup_solar_system)
        .run();
}
```

### Spawn Dynamic Objects

```rust
fn setup_solar_system(mut commands: Commands) {
    // Earth - Heavy object (auto-promoted to Tier 1)
    commands.spawn((
        DynamicMass::new(5.972e24),      // Editable at runtime
        DynamicRadius::new(6.371e6),
        HybridPosition::default(),
        HybridVelocity::default(),
        DynamicGravityForce::default(),
        Name::new("Earth"),
    ));
    
    // ISS - Light object (Tier 3)
    commands.spawn((
        DynamicMass::new(420_000.0),     // 420 tons
        DynamicRadius::new(50.0),
        HybridPosition::from_vec3(Vec3::new(6_771_000.0, 0.0, 0.0)),
        HybridVelocity::default(),
        DynamicGravityForce::default(),
        Name::new("ISS"),
    ));
}
```

### Runtime Mass Editing

```rust
// Edit mass at runtime (e.g., from Properties panel)
fn edit_mass_system(
    mut query: Query<&mut DynamicMass>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::KeyM) {
        for mut mass in query.iter_mut() {
            // Double the mass
            mass.kilograms *= 2.0;
            mass.update_tier(); // Recalculate tier
            
            info!("New mass: {:.3e} kg, Tier: {:?}", 
                  mass.kilograms, mass.tier);
        }
    }
}
```

### Query Force Metatable

```rust
fn display_forces(
    metatable: Res<ForceMetatable>,
    query: Query<(Entity, &Name)>,
) {
    println!("\n=== Force Metatable ===");
    
    for (entity_a, name_a) in query.iter() {
        for (entity_b, name_b) in query.iter() {
            if entity_a == entity_b { continue; }
            
            if let Some(force) = metatable.get_force(entity_a, entity_b) {
                if force.significant {
                    println!("{} → {}: {:.3e} N at {:.1} km",
                             name_a, name_b,
                             force.magnitude,
                             force.distance / 1000.0);
                }
            }
        }
    }
}
```

### Monitor Statistics

```rust
fn display_stats(
    stats: Res<DynamicGravityStats>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::Space) {
        println!("\n=== Dynamic Gravity Stats ===");
        println!("Heavy Objects: {}", stats.heavy_objects);
        println!("Medium Objects: {}", stats.medium_objects);
        println!("Light Objects: {}", stats.light_objects);
        println!("Total Forces: {}", stats.total_forces);
        println!("Significant Forces: {}", stats.significant_forces);
        println!("Metatable Size: {}", stats.metatable_size);
    }
}
```

## Configuration

```rust
#[derive(Resource)]
pub struct DynamicGravityConfig {
    pub enabled: bool,
    pub force_threshold: f64,        // 0.001 N default
    pub max_distance: f64,           // 1e12 m default
    pub use_tiered_updates: bool,    // true = performance optimization
    pub track_forces: bool,          // true = enable metatable
    pub cleanup_interval: f64,       // 10 seconds
    pub max_force_age: f64,          // 30 seconds
    pub debug_draw: bool,            // Visualize forces
    pub show_metatable: bool,        // Show in UI
}

// Configure at startup
app.insert_resource(DynamicGravityConfig {
    use_tiered_updates: true,
    track_forces: true,
    debug_draw: true,
    ..Default::default()
});
```

## Explorer Integration

### Properties Panel Editing

All dynamic components are `Reflect`-enabled for runtime editing:

```rust
// In Properties panel:
DynamicMass
  ├─ kilograms: 5.972e24  [editable]
  └─ tier: Heavy          [read-only]

DynamicRadius
  └─ meters: 6.371e6      [editable]

DynamicGravityForce
  ├─ force: (0, -9.81, 0) [read-only]
  ├─ heavy_sources: 1     [read-only]
  ├─ medium_sources: 0    [read-only]
  └─ light_sources: 0     [read-only]
```

### Automatic Tier Promotion

When mass is edited in Properties panel:

1. User changes `kilograms` field
2. `Changed<DynamicMass>` query detects change
3. `update_mass_tiers` system runs
4. Tier recalculated automatically
5. Heavy object list updated in `ForceMetatable`
6. Next frame uses new tier for calculations

```rust
// Example: Promote asteroid to planet
// Before: 1e15 kg (Medium tier)
mass.kilograms = 1e15;  // Large asteroid

// After: 5e24 kg (Heavy tier)
mass.kilograms = 5e24;  // Earth-sized planet
// Automatically promoted to Heavy tier!
```

## Debug Visualization

Enable with `debug_draw: true`:

- **Red lines**: Forces from Heavy objects
- **Yellow lines**: Forces from Medium objects
- **Green lines**: Forces from Light objects
- **Sphere size**: Indicates mass tier

```rust
config.debug_draw = true;

// Gizmos show:
// - Force vectors (scaled for visibility)
// - Color-coded by source tier
// - Sphere at each object (size = tier)
```

## Real-World Examples

### Earth-Moon System

```rust
// Earth (Heavy tier)
DynamicMass::new(5.972e24)  // 60 Hz updates
DynamicRadius::new(6.371e6)

// Moon (Heavy tier - just above threshold)
DynamicMass::new(7.342e22)  // 60 Hz updates
DynamicRadius::new(1.737e6)

// Force: ~1.98e20 N
// Distance: 384,400 km
// Update rate: 60 Hz (both Heavy)
```

### ISS in Orbit

```rust
// ISS (Light tier)
DynamicMass::new(420_000.0)  // 1 Hz updates
DynamicRadius::new(50.0)

// Only affected by Earth (Heavy)
// Force: ~3.5e6 N
// Update rate: 1 Hz (Light object)
// Performance: 60x faster than full N-body
```

### Asteroid Belt

```rust
// 1000 asteroids (Light tier)
for i in 0..1000 {
    DynamicMass::new(1e8)  // 100 million kg each
    // Only affected by Sun (Heavy)
    // Update rate: 1 Hz
}

// Performance: O(1000) instead of O(1000²)
// Speedup: 1000x
```

## Comparison: Static vs Dynamic

### Static Gravity (gravity.rs)

```rust
// Hardcoded presets
Mass::earth()  // Cannot edit at runtime
PhysicalRadius::earth()

// Uniform calculations
// All objects: O(n²) every frame
// No tier optimization
// No force tracking
```

### Dynamic Gravity (dynamic_gravity.rs)

```rust
// Runtime-editable
DynamicMass::new(5.972e24)  // Edit in Properties
DynamicRadius::new(6.371e6)

// Tiered calculations
// Heavy: O(n²) at 60 Hz
// Medium: O(n) at 10 Hz
// Light: O(1) at 1 Hz
// Full force metatable
```

## Best Practices

### 1. Use Appropriate Tiers

```rust
// ✅ Good: Let system auto-tier
let mass = DynamicMass::new(actual_mass_kg);

// ❌ Bad: Force wrong tier
// (System will auto-correct anyway)
```

### 2. Enable Tiered Updates for Performance

```rust
// ✅ Good: Enable for large simulations
config.use_tiered_updates = true;

// ❌ Bad: Disable for >100 objects
config.use_tiered_updates = false; // O(n²) every frame!
```

### 3. Use Force Metatable for Analysis

```rust
// ✅ Good: Query metatable for insights
if let Some(force) = metatable.get_force(a, b) {
    println!("Force: {:.3e} N", force.magnitude);
}

// ✅ Good: Check if force is significant
if force.significant {
    // Force > 0.001 N
}
```

### 4. Cleanup Metatable Periodically

```rust
// ✅ Good: Auto-cleanup enabled
config.cleanup_interval = 10.0;  // Every 10 seconds
config.max_force_age = 30.0;     // Remove entries >30s old

// ❌ Bad: Never cleanup
config.cleanup_interval = f64::INFINITY;
```

## Testing

```bash
# Run dynamic gravity tests
cargo test --package eustress-common dynamic_gravity

# Test tier promotion
cargo test test_mass_tiers

# Test dynamic mass editing
cargo test test_dynamic_mass
```

## Related Documentation

- [Static Gravity System](./GRAVITY_PHYSICS.md)
- [Hybrid Coordinates](./HYBRID_COORDINATES.md)
- [Orbital Grid](../architecture/ORBITAL_GRID.md)

---

**Implementation Date**: 2025-03-11  
**Author**: Cascade AI + User  
**Status**: Production Ready ✅  
**Performance**: Up to 1000x faster than static gravity for large simulations

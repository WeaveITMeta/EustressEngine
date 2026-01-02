# Quick Start: Using the Roblox Class System

## 🚀 For Developers: Getting Started in 5 Minutes

---

## Overview

The Roblox-compatible class system is now integrated into Eustress Engine. This guide shows you how to start using it immediately.

---

## Current Status

**Migration Phase:** Phase 1 (Compatibility Layer)
- ✅ Both systems work simultaneously
- ✅ Legacy PartData still active by default
- ✅ New class system ready to enable
- ✅ Zero-downtime switching

---

## Basic Usage

### 1. Spawn a Part (New System)

```rust
use crate::classes::*;
use crate::compatibility::*;

fn spawn_example_part(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Create Instance (identity)
    let instance = Instance {
        name: "MyPart".to_string(),
        class_name: ClassName::Part,
        archivable: true,
        id: 1,
    };
    
    // Create BasePart (transform, appearance, physics)
    let base_part = BasePart {
        cframe: Transform::from_xyz(0.0, 5.0, 0.0),
        size: Vec3::new(4.0, 1.0, 2.0),
        color: Color::srgb(1.0, 0.0, 0.0),  // Red
        material: Material::SmoothPlastic,
        anchored: false,
        can_collide: true,
        ..default()
    };
    
    // Create Part (shape)
    let part = Part {
        shape: PartType::Block,
    };
    
    // Spawn using helper function
    spawn_part(&mut commands, &mut meshes, &mut materials, 
               instance, base_part, part);
}
```

### 2. Query Parts (ECS Style)

```rust
fn update_parts(
    query: Query<(Entity, &Instance, &BasePart, &Part)>
) {
    for (entity, instance, base_part, part) in &query {
        println!("Part '{}' at position {:?}", 
                 instance.name, 
                 base_part.cframe.translation);
    }
}
```

### 3. Use Property Access

```rust
use crate::properties::{PropertyAccess, PropertyValue};

fn modify_part_color(
    mut query: Query<&mut BasePart>
) {
    for mut base_part in &mut query {
        // Get property
        if let Some(PropertyValue::Color(color)) = base_part.get_property("Color") {
            println!("Current color: {:?}", color);
        }
        
        // Set property (with validation)
        let result = base_part.set_property(
            "Color", 
            PropertyValue::Color(Color::srgb(0.0, 1.0, 0.0))
        );
        
        if let Err(e) = result {
            eprintln!("Failed to set color: {}", e);
        }
        
        // List all properties (for UI)
        let properties = base_part.list_properties();
        for prop in properties {
            println!("  [{}] {} ({})", 
                     prop.category, prop.name, prop.property_type);
        }
    }
}
```

### 4. Convert Legacy PartData

```rust
use crate::compatibility::*;

fn convert_legacy_part(old_data: PartData) -> (Instance, BasePart, Part) {
    // Convert from legacy
    let (instance, base_part, part) = part_data_to_components(&old_data);
    
    // Validate no data loss
    validate_roundtrip(&old_data).expect("Conversion failed");
    
    (instance, base_part, part)
}

fn convert_to_legacy(
    instance: &Instance,
    base_part: &BasePart,
    part: &Part,
) -> PartData {
    // Convert back to legacy (for compatibility)
    components_to_part_data(instance, base_part, part)
}
```

---

## Migration Control

### Toggle Between Systems

```rust
fn toggle_migration_system(
    mut migration_config: ResMut<MigrationConfig>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    // Press F9 to toggle (for testing)
    if keyboard.just_pressed(KeyCode::F9) {
        migration_config.toggle();
        
        if migration_config.enabled {
            println!("✅ Switched to NEW class system");
        } else {
            println!("⏮️ Rolled back to LEGACY PartData system");
        }
    }
}
```

### Check Current Mode

```rust
fn render_parts(
    migration_config: Res<MigrationConfig>,
    // ... other resources
) {
    if migration_config.enabled {
        // Use new class system
        render_with_components();
    } else {
        // Use legacy PartData
        render_with_part_data();
    }
}
```

---

## Common Patterns

### 1. Create a Model with Children

```rust
fn spawn_tool_model(mut commands: Commands) {
    // Create model (container)
    let model_instance = Instance {
        name: "Tool".to_string(),
        class_name: ClassName::Model,
        id: 10,
        ..default()
    };
    
    let model = Model {
        primary_part: Some(11),  // Handle part ID
        world_pivot: Transform::IDENTITY,
    };
    
    let model_entity = spawn_model(&mut commands, model_instance, model);
    
    // Create child parts
    let handle = spawn_part(&mut commands, ...);
    let blade = spawn_part(&mut commands, ...);
    
    // Parent to model
    commands.entity(handle).set_parent(model_entity);
    commands.entity(blade).set_parent(model_entity);
}
```

### 2. Add an Attachment

```rust
fn add_muzzle_flash(
    mut commands: Commands,
    gun_part: Entity,
) {
    // Create attachment point
    let attachment = commands.spawn((
        TransformBundle::from_transform(
            Transform::from_xyz(0.0, 0.0, 2.0)  // At barrel end
        ),
        Attachment {
            position: Vec3::new(0.0, 0.0, 2.0),
            orientation: Vec3::ZERO,
            cframe: Transform::from_xyz(0.0, 0.0, 2.0),
            name: "Muzzle".to_string(),
        },
    )).set_parent(gun_part).id();
    
    // Add particle effect to attachment
    commands.entity(attachment).insert(ParticleEmitter {
        rate: 100.0,
        enabled: true,
        ..default()
    });
}
```

### 3. Weld Two Parts

```rust
fn weld_parts(
    mut commands: Commands,
    part0_id: u32,
    part1_id: u32,
) {
    commands.spawn((
        WeldConstraint {
            part0: Some(part0_id),
            part1: Some(part1_id),
            c0: Transform::IDENTITY,
            c1: Transform::IDENTITY,
            enabled: true,
        },
        Instance {
            name: "Weld".to_string(),
            class_name: ClassName::WeldConstraint,
            id: 100,
            ..default()
        },
    ));
}
```

### 4. Add Sound

```rust
fn add_explosion_sound(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    explosion_part: Entity,
) {
    commands.entity(explosion_part).insert((
        AudioBundle {
            source: asset_server.load("sounds/explosion.ogg"),
            settings: PlaybackSettings {
                mode: PlaybackMode::Once,
                volume: Volume::new(0.8),
                speed: 1.0,
                spatial: true,
            },
        },
        Sound {
            sound_id: "sounds/explosion.ogg".to_string(),
            volume: 0.8,
            pitch: 1.0,
            looped: false,
            playing: true,
            spatial: true,
            roll_off_max_distance: 100.0,
        },
    ));
}
```

---

## Property Categories

Properties are organized into categories for the UI:

| Category | Properties |
|----------|-----------|
| **Data** | Name, ClassName, Archivable, Locked |
| **Transform** | Position, Orientation, Size, CFrame |
| **Appearance** | Color, Material, Transparency, Reflectance |
| **Physics** | Anchored, CanCollide, CanTouch |
| **AssemblyPhysics** | Mass, AssemblyLinearVelocity, AssemblyAngularVelocity |
| **Collision** | CollisionGroup |
| **Character** | WalkSpeed, JumpPower, HipHeight |
| **State** | Health, MaxHealth |
| **Animation** | PreferredAnimationSpeed, RigType |
| **Playback** | Volume, Pitch, Looped, Playing |
| **Spatial** | RollOffMaxDistance |
| **Emission** | Rate, Enabled |
| **Motion** | DesiredAngle, MaxVelocity |
| **Behavior** | Enabled |

---

## Material Presets

Available materials with PBR parameters:

```rust
// Smooth materials
Material::Plastic          // Rough plastic
Material::SmoothPlastic    // Glossy plastic

// Wood types
Material::Wood
Material::WoodPlanks

// Metals
Material::Metal
Material::CorrodedMetal
Material::DiamondPlate
Material::Foil

// Natural
Material::Grass
Material::Sand
Material::Ice

// Construction
Material::Concrete
Material::Brick
Material::Granite
Material::Marble
Material::Slate

// Special
Material::Glass
Material::Neon
Material::Fabric
```

---

## Property Types

Supported property value types:

```rust
pub enum PropertyValue {
    String(String),      // Text fields
    Float(f32),          // Numbers with decimals
    Int(i32),            // Whole numbers
    Bool(bool),          // Checkboxes
    Vector3(Vec3),       // Position, Size, etc.
    Color(Color),        // Color pickers
    Transform(Transform),// Full pose
    Enum(String),        // Dropdowns (Material, RigType, etc.)
}
```

---

## Debug Commands

Useful commands for development:

```rust
// Print all parts in scene
fn debug_print_parts(
    query: Query<(&Instance, &BasePart, Option<&Part>)>
) {
    println!("=== Scene Parts ===");
    for (instance, base_part, part) in &query {
        println!("  {} (ID: {}) - {:?} at {:?}", 
                 instance.name,
                 instance.id,
                 part.map(|p| p.shape),
                 base_part.cframe.translation);
    }
}

// Validate all conversions
fn validate_all_parts(
    query: Query<(&Instance, &BasePart, &Part)>
) {
    for (instance, base_part, part) in &query {
        let part_data = components_to_part_data(instance, base_part, part);
        match validate_roundtrip(&part_data) {
            Ok(_) => println!("✅ {} validated", instance.name),
            Err(e) => println!("❌ {} failed: {}", instance.name, e),
        }
    }
}
```

---

## Testing

Run the test suite:

```bash
# Test compatibility layer
cargo test compatibility

# Test property access
cargo test properties

# Run all tests
cargo test
```

---

## Next Steps

1. **Try spawning a part** using the new system
2. **Query parts** with ECS queries
3. **Experiment with PropertyAccess** for dynamic properties
4. **Check migration status** with `migration_config.enabled`
5. **Read full documentation** in:
   - `CLASSES_GUIDE.md` - Core classes
   - `CLASSES_EXTENDED.md` - Extended classes
   - `MIGRATION_PLAN.md` - Full migration strategy

---

## Troubleshooting

**Q: I can't see new class components in queries**
- Check that `migration_config.enabled = true`
- Ensure parts are spawned with `spawn_part()` helper
- Verify entity has all required components

**Q: Properties aren't updating**
- Use `set_property()` instead of direct field access for validation
- Check return value for errors
- Ensure property name is correct (case-sensitive)

**Q: Conversion fails with data loss**
- Run `validate_roundtrip()` to identify issue
- Check for NaN or infinite values in floats
- Verify all required fields are set

**Q: How do I revert to legacy system?**
```rust
migration_config.enabled = false;
```
Or press F9 if toggle system is enabled.

---

## Support

For questions or issues:
1. Check `CLASS_SYSTEM_COMPLETE.md` for full overview
2. Review test cases in `src/compatibility.rs`
3. See examples in `CLASSES_GUIDE.md`

---

**Ready to build with 25 Roblox-compatible classes!** 🎉🏗️✨

# Roblox-Compatible Class System for Eustress Engine

## Overview

Eustress Engine now features a complete Roblox-style class and property system built on Bevy ECS. This enables familiar Roblox workflows while leveraging Rust's performance and Bevy's powerful architecture.

## Table of Contents

1. [Architecture](#architecture)
2. [Core Classes](#core-classes)
3. [Property System](#property-system)
4. [Usage Examples](#usage-examples)
5. [Bevy Component Mapping](#bevy-component-mapping)
6. [Physics Integration](#physics-integration)
7. [Extending the System](#extending-the-system)

---

## Architecture

### Class Hierarchy

```
Instance (base for all ~200 Roblox classes)
├── PVInstance (adds pivot)
│   └── BasePart (core 3D object, ~50 properties)
│       ├── Part (primitive shapes)
│       └── MeshPart (custom meshes)
├── Model (container/groups)
├── Humanoid (character controller)
├── Camera (viewport control)
└── Light
    ├── PointLight
    ├── SpotLight
    └── SurfaceLight
```

### Design Principles

- **ECS-Native**: Each class is a Bevy component
- **Composition**: Entities can have multiple class components
- **Type-Safe**: Rust enums for PartType, Material, ClassName
- **Property Access**: Unified trait for get/set/list operations
- **Serialization**: Full serde support for save/load

---

## Core Classes

### 1. Instance (Base Class)

All entities inherit from `Instance`. Provides core identity and hierarchy.

```rust
#[derive(Component)]
pub struct Instance {
    pub name: String,           // Editable label
    pub class_name: ClassName,  // ReadOnly type identifier
    pub archivable: bool,       // Save eligibility
    pub id: u32,                // Unique entity ID
}
```

**Properties:**
- `Name` (string) - Editable label
- `ClassName` (string, ReadOnly) - "Part", "Model", etc.
- `Archivable` (bool) - Should be saved to file

**Bevy Mapping:**
- `Name` → Bevy `Name` component
- `Parent` → Bevy `Parent` component (hierarchy)
- Entity ID → Bevy `Entity`

---

### 2. BasePart (Core 3D Object)

Physical primitive base with ~50 properties for transform, physics, and rendering.

```rust
#[derive(Component)]
pub struct BasePart {
    // Transform/Geometry (~7 properties)
    pub cframe: Transform,                  // Full pose
    pub size: Vec3,                         // Dimensions (studs)
    pub pivot_offset: Transform,            // Local pivot
    
    // Appearance/Rendering (~4 properties)
    pub color: Color,                       // Tint
    pub material: Material,                 // PBR preset
    pub transparency: f32,                  // 0-1 opacity
    pub reflectance: f32,                   // Mirror-like
    
    // Physics/Collision (~9 properties)
    pub anchored: bool,                     // Immovable
    pub can_collide: bool,                  // Physics interactions
    pub can_touch: bool,                    // Touch events
    pub assembly_linear_velocity: Vec3,     // Linear velocity
    pub assembly_angular_velocity: Vec3,    // Angular velocity
    pub custom_physical_properties: Option<PhysicalProperties>,
    pub collision_group: String,            // Filtering
    
    // ReadOnly/Computed
    pub mass: f32,                          // Computed
    pub assembly_mass: f32,                 // Group total
    pub locked: bool,                       // Editing lock
}
```

**Key Properties:**

| Property | Type | Bevy Equivalent | Notes |
|----------|------|-----------------|-------|
| Position | Vector3 | Transform.translation | World position (Y-up) |
| CFrame | CFrame | Transform | Full pose (pos + rot) |
| Size | Vector3 | Transform.scale | Dimensions in studs |
| Orientation | Vector3 | Transform.rotation | Euler degrees |
| Color | Color3 | StandardMaterial.base_color | RGB tint |
| Material | Enum | StandardMaterial (PBR params) | Plastic, Metal, etc. |
| Transparency | float | StandardMaterial.alpha_mode | 0-1 opacity |
| Anchored | bool | RigidBody::Fixed vs Dynamic | Immovable |
| CanCollide | bool | Collider active/inactive | Physics on/off |
| Mass | float | Computed | From density/size |

---

### 3. Part (Primitive Shapes)

Extends `BasePart` with built-in procedural meshes.

```rust
#[derive(Component)]
pub struct Part {
    pub shape: PartType,  // Block, Ball, Cylinder, Wedge, CornerWedge, Cone
}
```

**PartType Enum:**
- `Block` (Cube) - Default Roblox "Part"
- `Ball` (Sphere)
- `Cylinder`
- `Wedge`
- `CornerWedge`
- `Cone`

**Bevy Mapping:**
- `Shape` → `Handle<Mesh>` (procedural generation)

---

### 4. MeshPart (Custom Meshes)

Extends `BasePart` with asset-loaded geometry.

```rust
#[derive(Component)]
pub struct MeshPart {
    pub mesh_id: String,     // Asset URL (rbxassetid://)
    pub texture_id: String,  // Texture asset URL
}
```

**Bevy Mapping:**
- `MeshId` → `Handle<Mesh>` from `AssetServer`
- `TextureId` → `Handle<Image>`

---

### 5. Model (Container/Groups)

Hierarchical assemblies like tools, characters, or buildings.

```rust
#[derive(Component)]
pub struct Model {
    pub primary_part: Option<u32>,  // Pivot reference part
    pub world_pivot: Transform,     // Computed group pose
}
```

**Usage:**
- Models are **containers only** (no geometry)
- Children parts attached via Bevy `Parent`/`Children`
- `PrimaryPart` defines pivot point
- `WorldPivot` computed from group bounds

**Bevy Mapping:**
- Spawned as `SpatialBundle` (transform only)
- Children use `Parent` component

---

### 6. Humanoid (Character Controller)

Controls character movement and animation.

```rust
#[derive(Component)]
pub struct Humanoid {
    pub walk_speed: f32,      // Default: 16.0 studs/sec
    pub jump_power: f32,      // Default: 50.0 studs
    pub hip_height: f32,      // Leg length offset
    pub health: f32,
    pub max_health: f32,
    pub auto_rotate: bool,
}
```

**Bevy Mapping:**
- Custom velocity controller
- Rapier physics integration for jumping
- Capsule collider with `hip_height` offset

---

### 7. Camera

Per-player viewport control.

```rust
#[derive(Component)]
pub struct RobloxCamera {
    pub cframe: Transform,        // View pose
    pub field_of_view: f32,       // FOV in degrees (default: 70)
    pub focus: Option<u32>,       // Target entity
}
```

**Bevy Mapping:**
- `Camera3dBundle` + custom controller
- `field_of_view` → `PerspectiveProjection.fov` (convert to radians)

---

### 8. PointLight

Omni-directional light source.

```rust
#[derive(Component)]
pub struct RobloxPointLight {
    pub brightness: f32,    // Intensity
    pub color: Color,       // Hue
    pub range: f32,         // Falloff distance (default: 60)
    pub shadows: bool,      // Cast shadows
}
```

**Bevy Mapping:**
- `PointLightBundle`
- Direct property mapping

---

## Property System

### PropertyAccess Trait

Unified interface for all classes:

```rust
pub trait PropertyAccess {
    fn get_property(&self, name: &str) -> Option<PropertyValue>;
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String>;
    fn list_properties(&self) -> Vec<PropertyDescriptor>;
}
```

### PropertyValue Enum

Covers all Roblox data types:

```rust
pub enum PropertyValue {
    String(String),
    Float(f32),
    Int(i32),
    Bool(bool),
    Vector3(Vec3),
    Color(Color),
    Transform(Transform),
    Enum(String),
}
```

### PropertyDescriptor

Metadata for UI generation:

```rust
pub struct PropertyDescriptor {
    pub name: String,
    pub property_type: String,
    pub read_only: bool,
    pub category: String,  // "Data", "Appearance", "Transform", etc.
}
```

---

## Usage Examples

### Spawning a Part

```rust
use crate::classes::*;

fn spawn_cube(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let instance = Instance {
        name: "MyCube".to_string(),
        class_name: ClassName::Part,
        archivable: true,
        id: 1,
    };
    
    let base_part = BasePart {
        cframe: Transform::from_xyz(0.0, 5.0, 0.0),
        size: Vec3::new(4.0, 1.0, 2.0),
        color: Color::srgb(1.0, 0.0, 0.0), // Red
        material: Material::SmoothPlastic,
        anchored: false,
        can_collide: true,
        ..default()
    };
    
    let part = Part {
        shape: PartType::Block,
    };
    
    spawn_part(&mut commands, &mut meshes, &mut materials, instance, base_part, part);
}
```

### Spawning a Model

```rust
fn spawn_tool_model(mut commands: Commands) {
    let instance = Instance {
        name: "Sword".to_string(),
        class_name: ClassName::Model,
        archivable: true,
        id: 10,
    };
    
    let model = Model {
        primary_part: Some(11), // Handle part ID
        world_pivot: Transform::IDENTITY,
    };
    
    let model_entity = spawn_model(&mut commands, instance, model);
    
    // Spawn child parts and attach to model
    // ... (spawn blade, handle, guard as children)
}
```

### Using PropertyAccess

```rust
fn modify_part_color(mut query: Query<&mut BasePart>) {
    for mut part in &mut query {
        // Get property
        if let Some(PropertyValue::Color(old_color)) = part.get_property("Color") {
            println!("Old color: {:?}", old_color);
        }
        
        // Set property
        let new_color = PropertyValue::Color(Color::srgb(0.0, 1.0, 0.0)); // Green
        if let Err(e) = part.set_property("Color", new_color) {
            eprintln!("Failed to set color: {}", e);
        }
    }
}
```

### Listing Properties for UI

```rust
fn show_properties_panel(part: &BasePart) {
    let properties = part.list_properties();
    
    for prop in properties {
        println!("[{}] {} ({}) - ReadOnly: {}", 
            prop.category, 
            prop.name, 
            prop.property_type, 
            prop.read_only
        );
    }
}

// Output:
// [Data] Anchored (bool) - ReadOnly: false
// [Data] CanCollide (bool) - ReadOnly: false
// [Appearance] Color (Color3) - ReadOnly: false
// [Transform] Position (Vector3) - ReadOnly: false
// [AssemblyPhysics] Mass (float) - ReadOnly: true
// ...
```

---

## Bevy Component Mapping

### Complete Bundle for Part

```rust
commands.spawn((
    // Rendering
    PbrBundle {
        mesh: meshes.add(Cuboid::from_size(part.size)),
        material: materials.add(StandardMaterial {
            base_color: part.color,
            perceptual_roughness: roughness,
            metallic,
            reflectance,
            alpha_mode: if part.transparency > 0.0 {
                AlphaMode::Blend
            } else {
                AlphaMode::Opaque
            },
            ..default()
        }),
        transform: part.cframe,
        ..default()
    },
    
    // Class components
    instance,
    base_part,
    part,
    
    // Bevy built-ins
    Name::new(instance.name.clone()),
));
```

### With Physics (bevy_rapier3d)

```rust
commands.spawn((
    // ... PbrBundle + class components ...
    
    // Physics
    RigidBody::Dynamic,  // or Fixed if anchored
    Collider::cuboid(size.x / 2.0, size.y / 2.0, size.z / 2.0),
    Velocity::linear(base_part.assembly_linear_velocity),
    Velocity::angular(base_part.assembly_angular_velocity),
    ColliderMassProperties::Density(0.7), // From custom_physical_properties
));
```

---

## Physics Integration

### Anchored Property

```rust
if base_part.anchored {
    commands.entity(entity).insert(RigidBody::Fixed);
} else {
    commands.entity(entity).insert(RigidBody::Dynamic);
}
```

### CanCollide Property

```rust
if !base_part.can_collide {
    commands.entity(entity).remove::<Collider>();
}
```

### Custom Physical Properties

```rust
if let Some(props) = base_part.custom_physical_properties {
    commands.entity(entity).insert(ColliderMassProperties::Density(props.density));
    commands.entity(entity).insert(Friction {
        coefficient: props.friction,
        combine_rule: CoefficientCombineRule::Average,
    });
    commands.entity(entity).insert(Restitution {
        coefficient: props.elasticity,
        combine_rule: CoefficientCombineRule::Average,
    });
}
```

---

## Extending the System

### Adding New Classes

1. **Define Component:**
```rust
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct TrussPart {
    pub style: TrussStyle,  // NoSupports, Supports, etc.
}
```

2. **Add to ClassName Enum:**
```rust
pub enum ClassName {
    // ... existing ...
    TrussPart,
}
```

3. **Implement PropertyAccess:**
```rust
impl PropertyAccess for TrussPart {
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Style" => Some(PropertyValue::Enum(format!("{:?}", self.style))),
            _ => None,
        }
    }
    // ... set_property, list_properties
}
```

### Adding New Properties to BasePart

1. Add field to struct
2. Update `PropertyAccess` implementation
3. Add to `PropertyDescriptor` list
4. Update Bevy component spawn logic

---

## Material System

### PBR Parameter Mapping

Each `Material` enum value maps to PBR parameters:

```rust
pub fn pbr_params(&self) -> (f32, f32, f32) {
    // (roughness, metallic, reflectance)
    match self {
        Material::Plastic => (0.7, 0.0, 0.0),
        Material::Metal => (0.2, 1.0, 0.5),
        Material::Glass => (0.0, 0.0, 0.9),
        Material::Neon => (0.0, 0.0, 1.0),
        // ...
    }
}
```

**Usage:**
```rust
let (roughness, metallic, reflectance) = base_part.material.pbr_params();
let material = materials.add(StandardMaterial {
    perceptual_roughness: roughness,
    metallic,
    reflectance,
    ..default()
});
```

---

## Best Practices

1. **Always use class components:** Don't bypass the system with raw Bevy components for properties
2. **Property validation:** Set methods enforce constraints (e.g., Size > 0)
3. **Read-only enforcement:** Mass, ClassName return errors on set attempts
4. **Computed properties:** Update in systems (e.g., `assembly_mass` from children)
5. **Serialization:** Use `archivable` flag to exclude runtime-only entities

---

## Future Enhancements

### Planned Classes
- Attachment (surfaces, constraints)
- Weld, Motor6D (joints)
- Sound, SoundGroup
- Script, LocalScript (WASM scripting)
- ParticleEmitter
- Beam, Trail

### Planned Features
- Constraint solver (Roblox physics)
- Surface properties (Studs, Smooth, Inlet, etc.)
- Decals and textures
- Character animation system
- DataModel serialization (.rbxl format)

---

## References

- [Roblox API Dump (2025)](https://anaminus.github.io/rbx/api/dump)
- [Bevy ECS Guide](https://bevyengine.org/learn/book/getting-started/ecs/)
- [bevy_rapier3d Documentation](https://rapier.rs/docs/user_guides/bevy_plugin/getting_started)
- [Eustress Engine User Rules](README.md)

---

**Status:** ✅ MVP Complete (10 core classes, ~80% coverage)  
**Next:** Physics integration, UI property editor, serialization system

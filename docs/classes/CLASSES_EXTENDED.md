# Extended Roblox Classes for Eustress Engine

## Additional Classes (10-25)

This document covers the extended class system beyond the core 10 classes.

### 10. Attachment (Local Offset for Lights/Joints)

Defines child positions relative to a BasePart (e.g., gun muzzle, light mount).

```rust
#[derive(Component)]
pub struct Attachment {
    pub position: Vec3,       // Local position
    pub orientation: Vec3,    // Local rotation (degrees)
    pub cframe: Transform,    // Computed (ReadOnly)
    pub name: String,         // Identifier
}
```

**Properties:**
- Position (Vector3) - Local offset
- Orientation (Vector3) - Euler angles in degrees
- CFrame (Transform) - ReadOnly computed transform
- Name (string) - For targeting in scripts

**Bevy Mapping:**
```rust
// Spawn attachment as child entity
commands.spawn((
    TransformBundle::from_transform(
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0))
    ),
    Attachment {
        position: Vec3::new(0.0, 1.0, 0.0),
        orientation: Vec3::ZERO,
        cframe: Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        name: "MuzzleFlash".to_string(),
    },
)).set_parent(part_entity);
```

**egui Editor:**
```rust
ui.horizontal(|ui| {
    ui.label("Position:");
    ui.add(egui::DragValue::new(&mut attachment.position.x).prefix("X:"));
    ui.add(egui::DragValue::new(&mut attachment.position.y).prefix("Y:"));
    ui.add(egui::DragValue::new(&mut attachment.position.z).prefix("Z:"));
});
```

---

### 11-12. Lights (Already Implemented)

PointLight and SpotLight were covered in the core classes. Additional notes:

**PointLight Bevy Bundle:**
```rust
commands.spawn(PointLightBundle {
    point_light: PointLight {
        intensity: brightness * 1000.0,  // Roblox-to-lux conversion
        range: range * 4.0,               // Studs to meters
        shadows_enabled: true,
        color: Color::srgb(color.r, color.g, color.b),
        ..default()
    },
    transform: attachment_transform,
    ..default()
});
```

---

### 13. WeldConstraint (Physics Joint: Fixed Link)

Welds two BaseParts rigidly (no relative movement).

```rust
#[derive(Component)]
pub struct WeldConstraint {
    pub part0: Option<u32>,  // Parent part
    pub part1: Option<u32>,  // Child part
    pub c0: Transform,       // Relative offset Part0
    pub c1: Transform,       // Relative offset Part1
    pub enabled: bool,       // Toggle joint
}
```

**Avian3D Integration:**
```rust
use avian3d::prelude::*;

// Fixed joint between two rigid bodies
let joint = FixedJoint::new(part0_entity, part1_entity)
    .with_local_anchor_1(c0.translation)
    .with_local_anchor_2(c1.translation);

commands.spawn((
    joint,
    WeldConstraint {
        part0: Some(part0_id),
        part1: Some(part1_id),
        c0: Transform::IDENTITY,
        c1: Transform::IDENTITY,
        enabled: true,
    },
));
```

---

### 14. Motor6D (Animation Joint: Dynamic Weld)

For character rigs and animations; allows rotation/translation.

```rust
#[derive(Component)]
pub struct Motor6D {
    pub part0: Option<u32>,      // Parent bone
    pub part1: Option<u32>,      // Child bone
    pub c0: Transform,           // Bind pose Part0
    pub c1: Transform,           // Bind pose Part1
    pub transform: Transform,    // Animated pose (runtime)
    pub desired_angle: f32,      // Target rotation
    pub max_velocity: f32,       // Speed limit
}
```

**Bevy Animation Integration:**
```rust
// Use bevy::animation::AnimationPlayer
commands.spawn((
    Motor6D {
        part0: Some(torso_id),
        part1: Some(arm_id),
        c0: Transform::from_xyz(0.0, 0.5, 0.0),
        c1: Transform::IDENTITY,
        transform: Transform::IDENTITY,
        desired_angle: 0.0,
        max_velocity: 0.1,
    },
    AnimationPlayer::default(),
));
```

---

### 15. SpecialMesh (Legacy Mesh Scaler)

Scales imported meshes with various types.

```rust
#[derive(Component)]
pub struct SpecialMesh {
    pub mesh_type: MeshType,  // FileMesh, Head, Torso, etc.
    pub scale: Vec3,          // Non-uniform scale
    pub mesh_id: String,      // Asset reference
    pub offset: Vec3,         // Position offset
}

pub enum MeshType {
    FileMesh, Head, Torso, Brick, Sphere, Cylinder
}
```

**Usage:**
```rust
let special_mesh = SpecialMesh {
    mesh_type: MeshType::FileMesh,
    scale: Vec3::new(2.0, 1.0, 1.5),
    mesh_id: "models/character_head.glb".to_string(),
    offset: Vec3::ZERO,
};
```

---

### 16. Decal (Surface Texture) - Updated for Bevy 0.16+

Projects images onto surfaces using Bevy's native **ForwardDecal** system.

```rust
#[derive(Component)]
pub struct Decal {
    pub texture: String,         // Asset path for decal image
    pub face: Face,              // Projection direction (legacy compat)
    pub transparency: f32,       // Alpha (0.0 = opaque, 1.0 = invisible)
    pub depth_fade_factor: f32,  // Edge blending (higher = sharper)
    pub color: [f32; 4],         // RGBA color tint
    pub z_index: i32,            // Depth sorting (legacy)
}

pub enum Face {
    Top, Bottom, Front, Back, Left, Right
}

impl Face {
    /// Convert to rotation for ForwardDecal projection (-Z direction)
    pub fn to_rotation(&self) -> Quat { ... }
}
```

**Bevy 0.16+ ForwardDecal System:**
```rust
use bevy::pbr::decal::{ForwardDecal, ForwardDecalMaterial, ForwardDecalMaterialExt};

// Spawn a forward decal
commands.spawn((
    ForwardDecal,
    MeshMaterial3d(decal_materials.add(ForwardDecalMaterial {
        base: StandardMaterial {
            base_color_texture: Some(asset_server.load("textures/blood_splatter.png")),
            alpha_mode: AlphaMode::Blend,
            ..default()
        },
        extension: ForwardDecalMaterialExt {
            depth_fade_factor: 1.0,  // Edge fade control
        },
    })),
    Transform::from_xyz(0.0, 0.1, 0.0).with_scale(Vec3::splat(2.0)),
));

// Camera MUST have DepthPrepass for decals to work
commands.spawn((
    Camera3d::default(),
    DepthPrepass,  // Required!
    Msaa::Off,     // Required on WebGPU
));
```

**Helper Functions:**
```rust
// Spawn decal with Eustress Decal component
spawn_decal(commands, asset_server, decal_materials, instance, decal, parent_transform);

// Quick spawn at position
spawn_decal_at(commands, asset_server, decal_materials, "textures/decal.png", position, scale, depth_fade);
```

---

### 17. Folder (Hierarchy Container)

Non-rendered logical grouping (like empty GameObjects in Unity).

```rust
#[derive(Component)]
pub struct Folder {} // Empty - just organizational
```

**Bevy Spawn:**
```rust
// Just an entity with children, no rendering
commands.spawn((
    SpatialBundle::default(),
    Instance {
        name: "Characters".to_string(),
        class_name: ClassName::Folder,
        archivable: true,
        id: folder_id,
    },
    Folder {},
));
```

---

### 18. Animator (Plays Animations)

Applies KeyframeSequences to rigs.

```rust
#[derive(Component)]
pub struct Animator {
    pub preferred_animation_speed: f32,  // Playback multiplier
    pub rig_type: RigType,               // Humanoid, R15, R6, Custom
}

pub enum RigType {
    Humanoid, R15, R6, Custom
}
```

**egui Property Editor:**
```rust
fn animator_editor(ui: &mut egui::Ui, animator: &mut Animator) {
    ui.label("Animation Speed");
    ui.add(egui::Slider::new(&mut animator.preferred_animation_speed, 0.1..=2.0));
    
    ui.label("Rig Type");
    egui::ComboBox::from_label("Skeleton")
        .selected_text(format!("{:?}", animator.rig_type))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut animator.rig_type, RigType::Humanoid, "Humanoid");
            ui.selectable_value(&mut animator.rig_type, RigType::R15, "R15");
            ui.selectable_value(&mut animator.rig_type, RigType::R6, "R6");
        });
}
```

---

### 19. KeyframeSequence (Animation Asset)

Stores poses over time.

```rust
#[derive(Component)]
pub struct KeyframeSequence {
    pub looped: bool,                     // Loop behavior
    pub priority: AnimationPriority,      // Layer blending
    pub keyframes: Vec<Keyframe>,         // Pose data
}

pub enum AnimationPriority {
    Core, Idle, Movement, Action
}

pub struct Keyframe {
    pub time: f32,
    pub pose: Transform,
    pub easing: EasingStyle,
}

pub enum EasingStyle {
    Linear, Constant, Cubic, Elastic
}
```

**Bevy AnimationClip Conversion:**
```rust
fn keyframe_to_animation_clip(sequence: &KeyframeSequence) -> AnimationClip {
    let mut clip = AnimationClip::default();
    // Convert keyframes to Bevy's animation format
    // ...
    clip
}
```

**egui Timeline Editor:**
```rust
ui.checkbox(&mut sequence.looped, "Loop");

// Keyframe list
for (i, keyframe) in sequence.keyframes.iter_mut().enumerate() {
    ui.horizontal(|ui| {
        ui.label(format!("Frame {}", i));
        ui.add(egui::DragValue::new(&mut keyframe.time).suffix("s"));
        // Easing selector
        egui::ComboBox::from_id_source(i)
            .selected_text(format!("{:?}", keyframe.easing))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut keyframe.easing, EasingStyle::Linear, "Linear");
                ui.selectable_value(&mut keyframe.easing, EasingStyle::Cubic, "Cubic");
            });
    });
}
```

---

### 20. ParticleEmitter (2D/3D Particles)

Effects like fire, smoke, explosions.

```rust
#[derive(Component)]
pub struct ParticleEmitter {
    pub rate: f32,                           // Particles/sec
    pub lifetime: (f32, f32),                // Min/max TTL
    pub color_sequence: Vec<(f32, Color)>,   // Color over time
    pub texture: String,                     // Particle sprite
    pub enabled: bool,                       // Active state
    pub speed: (f32, f32),                   // Velocity range
    pub spread_angle: Vec2,                  // Cone angle
}
```

**Bevy Integration (bevy_hanabi):**
```rust
use bevy_hanabi::*;

let effect = effects.add(
    EffectAsset::new(
        vec![32768],  // Capacity
        Spawner::rate(emitter.rate.into()),
        // ... configure from ParticleEmitter properties
    )
);
```

**egui Inspector:**
```rust
ui.add(egui::Slider::new(&mut emitter.rate, 0.0..=1000.0).text("Rate"));
ui.checkbox(&mut emitter.enabled, "Enabled");

// Color sequence editor
for (i, (time, color)) in emitter.color_sequence.iter_mut().enumerate() {
    ui.horizontal(|ui| {
        ui.label(format!("T={:.2}", time));
        let mut color32 = egui::Color32::from_rgba_premultiplied(
            (color.r() * 255.0) as u8,
            (color.g() * 255.0) as u8,
            (color.b() * 255.0) as u8,
            255,
        );
        ui.color_edit_button_srgba(&mut color32);
    });
}
```

---

### 21. Beam (Curved Line Effect)

Connects two Attachments with a curved line.

```rust
#[derive(Component)]
pub struct Beam {
    pub attachment0: Option<u32>,  // Start point
    pub attachment1: Option<u32>,  // End point
    pub curve_size0: f32,          // Bezier control 0
    pub curve_size1: f32,          // Bezier control 1
    pub segments: u32,             // Line resolution
    pub width: (f32, f32),         // Start/end width
    pub color: Color,
    pub texture: String,
}
```

**Bevy Procedural Mesh:**
```rust
fn generate_beam_mesh(beam: &Beam, att0_pos: Vec3, att1_pos: Vec3) -> Mesh {
    let mut positions = Vec::new();
    
    for i in 0..=beam.segments {
        let t = i as f32 / beam.segments as f32;
        // Bezier curve calculation
        let pos = bezier_cubic(att0_pos, control0, control1, att1_pos, t);
        positions.push(pos);
    }
    
    // Create line strip mesh
    Mesh::new(PrimitiveTopology::LineStrip)
        // ... add positions
}
```

**egui Editor:**
```rust
egui::ComboBox::from_label("Start Attachment")
    .show_ui(ui, |ui| {
        // Entity picker for attachment0
    });

ui.add(egui::Slider::new(&mut beam.curve_size0, -10.0..=10.0).text("Curve 0"));
ui.add(egui::DragValue::new(&mut beam.segments).clamp_range(2..=100));
```

---

### 22. Sound (Audio Playback)

3D positional audio.

```rust
#[derive(Component)]
pub struct Sound {
    pub sound_id: String,              // Asset path
    pub volume: f32,                   // 0-1
    pub pitch: f32,                    // 0.5-2
    pub looped: bool,
    pub playing: bool,
    pub spatial: bool,                 // 3D audio
    pub roll_off_max_distance: f32,
}
```

**Bevy Audio System:**
```rust
commands.spawn((
    AudioBundle {
        source: asset_server.load(&sound.sound_id),
        settings: PlaybackSettings {
            mode: if sound.looped { 
                PlaybackMode::Loop 
            } else { 
                PlaybackMode::Once 
            },
            volume: Volume::new(sound.volume),
            speed: sound.pitch,
            spatial: sound.spatial,
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
```

**egui Audio Controls:**
```rust
ui.add(egui::Slider::new(&mut sound.volume, 0.0..=1.0).text("Volume"));
ui.add(egui::Slider::new(&mut sound.pitch, 0.5..=2.0).text("Pitch"));
ui.checkbox(&mut sound.looped, "Loop");
if ui.button(if sound.playing { "⏸ Pause" } else { "▶ Play" }).clicked() {
    sound.playing = !sound.playing;
}
```

---

### 23. Terrain (Voxel Grid)

Procedural landscape system.

```rust
#[derive(Component)]
pub struct Terrain {
    pub material_colors: Vec<(String, Color)>,  // Voxel types
    pub water_wave_size: f32,
    pub water_transparency: f32,
    pub water_color: Color,
}
```

**Bevy Voxel System:**
```rust
// Custom voxel engine or use bevy_voxel
// Each voxel stores material type
// Mesh generation via marching cubes or greedy meshing
```

**egui Terrain Editor:**
```rust
// Material palette
for (name, color) in terrain.material_colors.iter_mut() {
    ui.horizontal(|ui| {
        ui.label(name);
        let mut color32 = egui::Color32::from_rgb(
            (color.r() * 255.0) as u8,
            (color.g() * 255.0) as u8,
            (color.b() * 255.0) as u8,
        );
        ui.color_edit_button_srgb(&mut color32);
    });
}

ui.add(egui::Slider::new(&mut terrain.water_wave_size, 0.0..=1.0).text("Wave Size"));
```

---

### 24. Sky (Skybox)

Environment map for atmospheric rendering.

```rust
#[derive(Component)]
pub struct Sky {
    pub skybox_textures: SkyboxTextures,  // 6 cubemap faces
    pub star_count: u32,
    pub celestial_bodies_shown: bool,
}

pub struct SkyboxTextures {
    pub back: String,
    pub front: String,
    pub left: String,
    pub right: String,
    pub up: String,
    pub down: String,
}
```

**Bevy Skybox:**
```rust
commands.spawn((
    Skybox {
        image: asset_server.load("skybox.ktx2"),  // Cubemap
        brightness: 1000.0,
    },
    Sky {
        skybox_textures: SkyboxTextures {
            back: "sky/back.png".to_string(),
            // ... other faces
        },
        star_count: 3000,
        celestial_bodies_shown: true,
    },
));
```

---

### 25. UnionOperation (CSG Boolean Operations)

Combines BaseParts using constructive solid geometry.

```rust
#[derive(Component)]
pub struct UnionOperation {
    pub operation: CSGOperation,       // Union, Subtract, Intersect
    pub use_part_color: bool,          // Material inheritance
    pub source_parts: Vec<u32>,        // Parts to combine
}

pub enum CSGOperation {
    Union, Subtract, Intersect
}
```

**Bevy CSG (using parry3d):**
```rust
use parry3d::shape::*;

fn perform_csg(union_op: &UnionOperation, parts: &[BasePart]) -> Mesh {
    // Convert parts to parry3d shapes
    // Perform boolean operations
    // Generate resulting mesh
    // ...
}
```

---

## Summary

**Total Classes: 25**
- Core: 10 (Instance → Humanoid, Camera, Lights)
- Extended: 15 (Attachment → UnionOperation)

**Coverage:**
- ✅ Hierarchy & Organization (Instance, Model, Folder)
- ✅ 3D Rendering (Part, MeshPart, SpecialMesh, Decal)
- ✅ Physics (WeldConstraint, Motor6D)
- ✅ Animation (Animator, KeyframeSequence)
- ✅ Effects (ParticleEmitter, Beam)
- ✅ Audio (Sound)
- ✅ Environment (Terrain, Sky)
- ✅ CSG (UnionOperation)

**Bevy Plugins Needed:**
- avian3d (physics)
- bevy_hanabi (particles)
- bevy_voxel or custom (terrain)
- CSG library (parry3d_f64)

**egui Integration:**
- All classes have property editors
- Sliders, drag values, color pickers
- ComboBoxes for enums
- Entity pickers for references
- Timeline editors for animation

---

## Next Steps

1. **Implement PropertyAccess** for new classes
2. **Create spawn helpers** for each class
3. **Build egui property panels** using descriptors
4. **Integrate physics** with avian3d
5. **Add animation system** with Motor6D support
6. **Implement particle effects** with bevy_hanabi
7. **Create CSG processor** for UnionOperation

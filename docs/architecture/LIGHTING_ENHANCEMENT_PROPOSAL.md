# Lighting Enhancement Proposal for Eustress Engine

## Problem Statement
Instances like the baseplate appear flat because they lack proper light interaction, despite having:
- ✅ Two directional lights (Sun + Fill)
- ✅ Ambient lighting
- ✅ PBR materials with roughness/metallic parameters

The flatness likely comes from:
1. **Missing normal maps** - No surface detail for light scattering
2. **Insufficient material variety** - All parts use basic colors without texture
3. **No environment lighting** - Missing image-based lighting (IBL) for realistic reflections
4. **Shadow quality** - Could be improved for depth perception

---

## Proposed Solutions (Priority Order)

### **Phase 1: Immediate Improvements (Easy Wins)**

#### 1. **Enable Environment Map Lighting (IBL)**
**Impact: HIGH** | **Effort: LOW**

Add image-based lighting using the skybox as an environment map:

```rust
// In default_scene.rs - after skybox creation
commands.insert_resource(EnvironmentMapLight {
    diffuse_map: skybox_handle.clone(),
    specular_map: skybox_handle.clone(),
    intensity: 400.0,
});
```

**Benefits:**
- Realistic ambient occlusion
- Proper reflections on materials
- Better light scattering on surfaces
- More depth and dimension

---

#### 2. **Improve Shadow Quality**
**Impact: MEDIUM** | **Effort: LOW**

Update shadow cascade settings:

```rust
// In default_scene.rs - DirectionalLight
shadow_depth_bias: 0.01,  // Reduce self-shadowing
shadow_normal_bias: 0.3,  // Better shadow edges
// Add cascade configuration
cascade_shadow_config: CascadeShadowConfigBuilder {
    num_cascades: 4,
    first_cascade_far_bound: 8.0,
    maximum_distance: 200.0,
    ..default()
}.build(),
```

**Benefits:**
- Sharper shadows on nearby objects
- Better depth perception
- Reduced shadow artifacts

---

#### 3. **Enhanced Material Presets**
**Impact: MEDIUM** | **Effort: MEDIUM**

Update `Material::pbr_params()` to use better PBR values:

```rust
// In classes.rs - Material enum
impl Material {
    pub fn pbr_params(&self) -> (f32, f32, f32) {
        match self {
            // (roughness, metallic, reflectance)
            Material::Plastic => (0.6, 0.0, 0.4),        // More reflective
            Material::SmoothPlastic => (0.2, 0.0, 0.5),  // Very smooth
            Material::Metal => (0.3, 1.0, 0.6),          // Shiny metal
            Material::Concrete => (0.95, 0.0, 0.2),      // Very rough
            Material::Grass => (0.9, 0.0, 0.1),          // Matte
            Material::Glass => (0.05, 0.0, 0.9),         // Very reflective
            Material::Neon => (0.1, 0.0, 0.0),           // Smooth + emissive
            // ... other materials
        }
    }
    
    /// New: Get emissive properties for self-lit materials
    pub fn emissive_params(&self) -> Option<(Color, f32)> {
        match self {
            Material::Neon => Some((Color::srgb(1.0, 1.0, 1.0), 10.0)),
            _ => None,
        }
    }
}
```

Then in `spawn_part()`:

```rust
let material = materials.add(StandardMaterial {
    base_color: base_part.color,
    perceptual_roughness: roughness,
    metallic,
    reflectance,
    // NEW: Add emissive for materials like Neon
    emissive: if let Some((color, strength)) = base_part.material.emissive_params() {
        base_part.color * color * strength
    } else {
        Color::BLACK
    },
    alpha_mode: if base_part.transparency > 0.0 {
        AlphaMode::Blend
    } else {
        AlphaMode::Opaque
    },
    ..default()
});
```

**Benefits:**
- Materials look more realistic
- Better visual distinction between material types
- Neon actually glows!

---

### **Phase 2: Advanced Enhancements (Polish)**

#### 4. **Add Procedural Normal Maps**
**Impact: HIGH** | **Effort: HIGH**

Generate normal maps procedurally for each material:

```rust
/// Generate a procedural normal map for a material
fn create_material_normal_map(
    material: Material,
    images: &mut Assets<Image>,
) -> Handle<Image> {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    
    const SIZE: u32 = 512;
    let mut normal_data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    
    for y in 0..SIZE {
        for x in 0..SIZE {
            // Generate noise-based normals for different materials
            let (nx, ny, nz) = match material {
                Material::Concrete | Material::Brick => {
                    // Rough, bumpy surface
                    let noise = perlin_noise(x as f32 / 10.0, y as f32 / 10.0);
                    (noise * 0.3, noise * 0.3, 0.8)
                },
                Material::WoodPlanks | Material::Wood => {
                    // Linear grain pattern
                    let grain = ((y as f32 / 5.0).sin() * 0.2).clamp(-0.3, 0.3);
                    (grain, 0.0, 0.9)
                },
                Material::Metal | Material::DiamondPlate => {
                    // Small scratches
                    let noise = perlin_noise(x as f32 / 50.0, y as f32 / 50.0);
                    (noise * 0.1, noise * 0.1, 0.95)
                },
                _ => {
                    // Flat normal (pointing up)
                    (0.0, 0.0, 1.0)
                }
            };
            
            // Convert normal to RGB (0-255 range)
            normal_data.push(((nx + 1.0) * 0.5 * 255.0) as u8);
            normal_data.push(((ny + 1.0) * 0.5 * 255.0) as u8);
            normal_data.push(((nz + 1.0) * 0.5 * 255.0) as u8);
            normal_data.push(255); // Alpha
        }
    }
    
    // Create normal map image
    images.add(Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        normal_data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

// Then in spawn_part(), add:
let normal_map = create_material_normal_map(base_part.material, images);

let material = materials.add(StandardMaterial {
    base_color: base_part.color,
    perceptual_roughness: roughness,
    metallic,
    reflectance,
    normal_map_texture: Some(normal_map),  // NEW!
    // ... rest of material
});
```

**Benefits:**
- Dramatic visual improvement
- Surface detail without geometry
- Better light scattering and depth

---

#### 5. **Screen Space Ambient Occlusion (SSAO)**
**Impact: MEDIUM** | **Effort: MEDIUM**

Enable SSAO in rendering pipeline:

```rust
// In main.rs, add to camera
use bevy::core_pipeline::experimental::taa::{TemporalAntiAliasBundle, TemporalAntiAliasPlugin};
use bevy::pbr::{ScreenSpaceAmbientOcclusionBundle, ScreenSpaceAmbientOcclusionSettings};

// Add plugin
app.add_plugins(TemporalAntiAliasPlugin);

// In camera setup
commands.spawn((
    Camera3dBundle { /* ... */ },
    ScreenSpaceAmbientOcclusionBundle::default(),
    TemporalAntiAliasBundle::default(),
));
```

**Benefits:**
- Contact shadows in crevices
- Better depth perception
- More realistic lighting

---

#### 6. **Bloom Effect for Bright Materials**
**Impact: LOW** | **Effort: LOW**

Add bloom for glowing materials:

```rust
// In camera setup
use bevy::core_pipeline::bloom::BloomSettings;

commands.spawn((
    Camera3dBundle { /* ... */ },
    BloomSettings {
        intensity: 0.3,
        low_frequency_boost: 0.7,
        low_frequency_boost_curvature: 0.95,
        high_pass_frequency: 1.0,
        prefilter_settings: BloomPrefilterSettings {
            threshold: 0.8,
            threshold_softness: 0.5,
        },
        composite_mode: BloomCompositeMode::Additive,
    },
));
```

**Benefits:**
- Neon materials glow realistically
- Better highlight rendering
- More cinematic look

---

### **Phase 3: User-Configurable Options (Future)**

#### 7. **Lighting Settings Panel**
**Impact: LOW** | **Effort: MEDIUM**

Add UI panel for real-time lighting adjustments:

```rust
#[derive(Resource)]
pub struct LightingSettings {
    pub sun_intensity: f32,
    pub sun_color: Color,
    pub sun_angle: f32,
    pub ambient_intensity: f32,
    pub shadow_quality: ShadowQuality,
    pub enable_ssao: bool,
    pub enable_bloom: bool,
    pub environment_intensity: f32,
}

enum ShadowQuality {
    Low,    // 1 cascade, 1024x1024
    Medium, // 2 cascades, 2048x2048
    High,   // 4 cascades, 4096x4096
    Ultra,  // 4 cascades, 8192x8192
}
```

**Benefits:**
- Users can customize lighting per scene
- Performance vs quality tradeoffs
- Match different art styles

---

## Implementation Roadmap

### **Week 1: Core Lighting**
1. ✅ Add EnvironmentMapLight
2. ✅ Improve shadow cascades
3. ✅ Update material PBR values

**Result:** Immediate visual improvement with minimal effort

### **Week 2: Advanced Effects**
1. ⚙️ Add procedural normal maps
2. ⚙️ Enable SSAO
3. ⚙️ Add bloom for emissive materials

**Result:** Professional-quality rendering

### **Week 3: Polish & UI**
1. 📋 Lighting settings panel
2. 📋 Per-scene lighting presets (Day, Night, Indoor, etc.)
3. 📋 Performance profiling and optimization

**Result:** User control and optimization

---

## Quick Fix for Baseplate

**Immediate action** (add to `default_scene.rs`):

```rust
// After spawning baseplate, update its material for better lighting
commands.spawn((
    PbrBundle {
        mesh: meshes.add(Cuboid::from_size(Vec3::new(100.0, 1.0, 100.0))),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.45, 0.5),
            perceptual_roughness: 0.7,  // Slightly rough for light scattering
            metallic: 0.0,
            reflectance: 0.3,           // Some reflection
            // NEW: Add slight normal variation for visual interest
            // (Or generate a proper normal map in Phase 2)
            ..default()
        }),
        transform: Transform::from_xyz(0.0, -0.5, 0.0),
        ..default()
    },
    // ... rest of baseplate components
));
```

---

## Performance Considerations

| Feature | Performance Impact | Quality Gain |
|---------|-------------------|--------------|
| Environment Map | Low (1-2ms) | High ⭐⭐⭐ |
| Better Shadows | Medium (2-5ms) | Medium ⭐⭐ |
| Material Updates | None | Medium ⭐⭐ |
| Normal Maps | Medium (3-6ms) | Very High ⭐⭐⭐⭐ |
| SSAO | High (5-10ms) | High ⭐⭐⭐ |
| Bloom | Low (1-3ms) | Low ⭐ |

**Recommended:** Start with **Phase 1** for best quality/performance ratio.

---

## Summary

**Root Cause:** Flat appearance due to:
- Missing environment-based lighting (IBL)
- Basic material properties
- No surface detail (normal maps)

**Solution Priority:**
1. ✅ **Environment Map** (biggest impact, easiest)
2. ✅ **Material Tweaks** (free improvement)
3. ✅ **Better Shadows** (small effort, good result)
4. ⚙️ **Normal Maps** (major visual upgrade)
5. 📋 **SSAO/Bloom** (polish)

**Estimated Timeline:** 
- Phase 1 (Week 1): **Immediate visible improvement**
- Phase 2 (Week 2): **Professional quality**
- Phase 3 (Week 3): **User customization**

This approach balances quick wins with long-term quality improvements while maintaining good performance! 🎨

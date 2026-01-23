# 06 - SAM 3D Integration

> Telescope imagery processing, 3D reconstruction, and model caching for orbital objects

## Overview

SAM 3D (Segment Anything Model in 3D) enables the reconstruction of 3D models from telescope imagery. This document covers the complete pipeline from raw telescope frames to cached 3D models integrated into the navigation system.

## Pipeline Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      SAM 3D PROCESSING PIPELINE                         │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐              │
│  │  Telescope   │───▶│    Lens      │───▶│   Frame      │              │
│  │   Sensor     │    │  Correction  │    │   Buffer     │              │
│  └──────────────┘    └──────────────┘    └──────────────┘              │
│                                                 │                        │
│                                                 ▼                        │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                    SEGMENTATION STAGE                            │  │
│  │  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐       │  │
│  │  │    SAM       │───▶│   Object     │───▶│    Star      │       │  │
│  │  │  Inference   │    │   Masks      │    │  Matching    │       │  │
│  │  └──────────────┘    └──────────────┘    └──────────────┘       │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│                                                 │                        │
│                                                 ▼                        │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                    3D RECONSTRUCTION STAGE                       │  │
│  │  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐       │  │
│  │  │    Depth     │───▶│    Mesh      │───▶│   Texture    │       │  │
│  │  │  Estimation  │    │  Generation  │    │   Mapping    │       │  │
│  │  └──────────────┘    └──────────────┘    └──────────────┘       │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│                                                 │                        │
│                                                 ▼                        │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                    INTEGRATION STAGE                             │  │
│  │  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐       │  │
│  │  │   Position   │───▶│    Model     │───▶│    Bevy      │       │  │
│  │  │  Calibration │    │    Cache     │    │   Entity     │       │  │
│  │  └──────────────┘    └──────────────┘    └──────────────┘       │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

## Telescope Input Processing

### Lens Geometry Model

```rust
/// Telescope lens parameters for projection correction
#[derive(Clone, Debug)]
pub struct TelescopeLens {
    /// Focal length in millimeters
    pub focal_length_mm: f64,
    /// Sensor width in millimeters
    pub sensor_width_mm: f64,
    /// Sensor height in millimeters
    pub sensor_height_mm: f64,
    /// Image width in pixels
    pub image_width_px: u32,
    /// Image height in pixels
    pub image_height_px: u32,
    /// Radial distortion coefficients (k1, k2, k3)
    pub radial_distortion: [f64; 3],
    /// Tangential distortion coefficients (p1, p2)
    pub tangential_distortion: [f64; 2],
    /// Principal point offset (cx, cy) in pixels
    pub principal_point: [f64; 2],
}

impl TelescopeLens {
    /// Convert pixel coordinates to normalized camera coordinates
    pub fn pixel_to_normalized(&self, pixel: [f64; 2]) -> [f64; 2] {
        let cx = self.principal_point[0];
        let cy = self.principal_point[1];
        
        // Pixel to sensor coordinates (mm)
        let x_mm = (pixel[0] - cx) * self.sensor_width_mm / self.image_width_px as f64;
        let y_mm = (pixel[1] - cy) * self.sensor_height_mm / self.image_height_px as f64;
        
        // Normalize by focal length
        [x_mm / self.focal_length_mm, y_mm / self.focal_length_mm]
    }
    
    /// Apply distortion correction
    pub fn undistort(&self, normalized: [f64; 2]) -> [f64; 2] {
        let x = normalized[0];
        let y = normalized[1];
        let r2 = x * x + y * y;
        let r4 = r2 * r2;
        let r6 = r4 * r2;
        
        let [k1, k2, k3] = self.radial_distortion;
        let [p1, p2] = self.tangential_distortion;
        
        // Radial distortion
        let radial = 1.0 + k1 * r2 + k2 * r4 + k3 * r6;
        
        // Tangential distortion
        let x_tang = 2.0 * p1 * x * y + p2 * (r2 + 2.0 * x * x);
        let y_tang = p1 * (r2 + 2.0 * y * y) + 2.0 * p2 * x * y;
        
        [
            x * radial + x_tang,
            y * radial + y_tang,
        ]
    }
    
    /// Convert normalized coordinates to 3D ray direction
    pub fn normalized_to_ray(&self, normalized: [f64; 2]) -> DVec3 {
        let undistorted = self.undistort(normalized);
        DVec3::new(undistorted[0], undistorted[1], 1.0).normalize()
    }
    
    /// Full pipeline: pixel to ray
    pub fn pixel_to_ray(&self, pixel: [f64; 2]) -> DVec3 {
        let normalized = self.pixel_to_normalized(pixel);
        self.normalized_to_ray(normalized)
    }
}
```

### Frame Buffer

```rust
/// Buffer for telescope frames awaiting processing
#[derive(Resource)]
pub struct TelescopeFrameBuffer {
    /// Pending frames
    pub frames: Vec<TelescopeFrame>,
    /// Maximum buffer size
    pub max_frames: usize,
    /// Processing state
    pub processing: Option<usize>,
}

#[derive(Clone)]
pub struct TelescopeFrame {
    /// Unique frame identifier
    pub id: u64,
    /// Capture timestamp (Julian Date)
    pub timestamp: f64,
    /// Raw image data (grayscale or RGB)
    pub image: Vec<u8>,
    /// Image dimensions
    pub width: u32,
    pub height: u32,
    /// Telescope pointing direction (ICRS)
    pub pointing_ra: f64,  // Right ascension (radians)
    pub pointing_dec: f64, // Declination (radians)
    /// Field of view (radians)
    pub fov: f64,
    /// Exposure time (seconds)
    pub exposure: f64,
}

impl TelescopeFrameBuffer {
    pub fn push(&mut self, frame: TelescopeFrame) {
        if self.frames.len() >= self.max_frames {
            self.frames.remove(0);
        }
        self.frames.push(frame);
    }
    
    pub fn pop_for_processing(&mut self) -> Option<TelescopeFrame> {
        if self.processing.is_some() {
            return None;
        }
        
        if let Some(frame) = self.frames.pop() {
            self.processing = Some(frame.id as usize);
            Some(frame)
        } else {
            None
        }
    }
}
```

## SAM 3D Inference

### ONNX Runtime Integration

```rust
use ort::{Session, SessionBuilder, Value};

/// SAM 3D inference engine
pub struct Sam3dEngine {
    /// ONNX session for SAM encoder
    encoder: Session,
    /// ONNX session for SAM decoder
    decoder: Session,
    /// ONNX session for depth estimation
    depth_estimator: Session,
    /// ONNX session for mesh generation
    mesh_generator: Session,
}

impl Sam3dEngine {
    pub fn new(model_path: &str) -> Result<Self, ort::Error> {
        let encoder = SessionBuilder::new()?
            .with_model_from_file(format!("{}/sam_encoder.onnx", model_path))?;
        
        let decoder = SessionBuilder::new()?
            .with_model_from_file(format!("{}/sam_decoder.onnx", model_path))?;
        
        let depth_estimator = SessionBuilder::new()?
            .with_model_from_file(format!("{}/depth_estimator.onnx", model_path))?;
        
        let mesh_generator = SessionBuilder::new()?
            .with_model_from_file(format!("{}/mesh_generator.onnx", model_path))?;
        
        Ok(Self {
            encoder,
            decoder,
            depth_estimator,
            mesh_generator,
        })
    }
    
    /// Run full SAM 3D pipeline on a frame
    pub fn process_frame(
        &self,
        frame: &TelescopeFrame,
        prompts: &[SegmentationPrompt],
    ) -> Result<Vec<Sam3dResult>, Sam3dError> {
        // Encode image
        let embeddings = self.encode_image(&frame.image, frame.width, frame.height)?;
        
        let mut results = Vec::new();
        
        for prompt in prompts {
            // Decode with prompt
            let mask = self.decode_mask(&embeddings, prompt)?;
            
            // Estimate depth
            let depth_map = self.estimate_depth(&frame.image, &mask, frame.width, frame.height)?;
            
            // Generate mesh
            let mesh = self.generate_mesh(&mask, &depth_map)?;
            
            results.push(Sam3dResult {
                mask,
                depth_map,
                mesh,
                prompt: prompt.clone(),
            });
        }
        
        Ok(results)
    }
    
    fn encode_image(
        &self,
        image: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<f32>, Sam3dError> {
        // Preprocess image to model input format
        let input = preprocess_image(image, width, height);
        
        // Run encoder
        let outputs = self.encoder.run(ort::inputs!["image" => input]?)?;
        
        let embeddings = outputs["embeddings"]
            .try_extract_tensor::<f32>()?
            .view()
            .to_owned()
            .into_raw_vec();
        
        Ok(embeddings)
    }
    
    fn decode_mask(
        &self,
        embeddings: &[f32],
        prompt: &SegmentationPrompt,
    ) -> Result<SegmentationMask, Sam3dError> {
        let prompt_input = prompt.to_tensor();
        
        let outputs = self.decoder.run(ort::inputs![
            "embeddings" => embeddings,
            "prompt" => prompt_input,
        ]?)?;
        
        let mask_data = outputs["mask"]
            .try_extract_tensor::<f32>()?
            .view()
            .to_owned()
            .into_raw_vec();
        
        Ok(SegmentationMask {
            data: mask_data,
            width: prompt.image_width,
            height: prompt.image_height,
            threshold: 0.5,
        })
    }
    
    fn estimate_depth(
        &self,
        image: &[u8],
        mask: &SegmentationMask,
        width: u32,
        height: u32,
    ) -> Result<DepthMap, Sam3dError> {
        let masked_image = apply_mask(image, mask);
        
        let outputs = self.depth_estimator.run(ort::inputs![
            "image" => masked_image,
        ]?)?;
        
        let depth_data = outputs["depth"]
            .try_extract_tensor::<f32>()?
            .view()
            .to_owned()
            .into_raw_vec();
        
        Ok(DepthMap {
            data: depth_data,
            width,
            height,
            min_depth: 0.0,
            max_depth: 1.0, // Normalized, will be scaled later
        })
    }
    
    fn generate_mesh(
        &self,
        mask: &SegmentationMask,
        depth_map: &DepthMap,
    ) -> Result<GeneratedMesh, Sam3dError> {
        let outputs = self.mesh_generator.run(ort::inputs![
            "mask" => &mask.data,
            "depth" => &depth_map.data,
        ]?)?;
        
        let vertices = outputs["vertices"]
            .try_extract_tensor::<f32>()?
            .view()
            .to_owned()
            .into_raw_vec();
        
        let indices = outputs["indices"]
            .try_extract_tensor::<u32>()?
            .view()
            .to_owned()
            .into_raw_vec();
        
        let normals = outputs["normals"]
            .try_extract_tensor::<f32>()?
            .view()
            .to_owned()
            .into_raw_vec();
        
        Ok(GeneratedMesh {
            vertices,
            indices,
            normals,
            uvs: Vec::new(), // Optional texture coordinates
        })
    }
}

#[derive(Clone, Debug)]
pub struct SegmentationPrompt {
    /// Point prompts (x, y, is_foreground)
    pub points: Vec<(f32, f32, bool)>,
    /// Box prompts (x1, y1, x2, y2)
    pub boxes: Vec<(f32, f32, f32, f32)>,
    /// Image dimensions
    pub image_width: u32,
    pub image_height: u32,
}

#[derive(Clone)]
pub struct Sam3dResult {
    pub mask: SegmentationMask,
    pub depth_map: DepthMap,
    pub mesh: GeneratedMesh,
    pub prompt: SegmentationPrompt,
}

#[derive(Clone)]
pub struct SegmentationMask {
    pub data: Vec<f32>,
    pub width: u32,
    pub height: u32,
    pub threshold: f32,
}

#[derive(Clone)]
pub struct DepthMap {
    pub data: Vec<f32>,
    pub width: u32,
    pub height: u32,
    pub min_depth: f32,
    pub max_depth: f32,
}

#[derive(Clone)]
pub struct GeneratedMesh {
    pub vertices: Vec<f32>,  // [x, y, z, x, y, z, ...]
    pub indices: Vec<u32>,
    pub normals: Vec<f32>,
    pub uvs: Vec<f32>,
}
```

## Star Matching and Astrometric Calibration

### Star Catalog Integration

```rust
/// Star catalog entry
#[derive(Clone, Debug)]
pub struct CatalogStar {
    /// Hipparcos/Gaia ID
    pub id: u64,
    /// Right ascension (radians, ICRS)
    pub ra: f64,
    /// Declination (radians, ICRS)
    pub dec: f64,
    /// Visual magnitude
    pub magnitude: f32,
    /// Proper motion RA (mas/year)
    pub pm_ra: f32,
    /// Proper motion Dec (mas/year)
    pub pm_dec: f32,
}

/// Star catalog for astrometric calibration
#[derive(Resource)]
pub struct StarCatalog {
    /// All catalog stars (sorted by RA for efficient queries)
    pub stars: Vec<CatalogStar>,
    /// Spatial index for fast queries
    index: rstar::RTree<StarEntry>,
}

struct StarEntry {
    idx: usize,
    ra: f64,
    dec: f64,
}

impl rstar::RTreeObject for StarEntry {
    type Envelope = rstar::AABB<[f64; 2]>;
    
    fn envelope(&self) -> Self::Envelope {
        rstar::AABB::from_point([self.ra, self.dec])
    }
}

impl StarCatalog {
    /// Query stars within a field of view
    pub fn query_fov(
        &self,
        center_ra: f64,
        center_dec: f64,
        fov_radius: f64,
        max_magnitude: f32,
    ) -> Vec<&CatalogStar> {
        let min_ra = center_ra - fov_radius;
        let max_ra = center_ra + fov_radius;
        let min_dec = (center_dec - fov_radius).max(-std::f64::consts::FRAC_PI_2);
        let max_dec = (center_dec + fov_radius).min(std::f64::consts::FRAC_PI_2);
        
        let envelope = rstar::AABB::from_corners([min_ra, min_dec], [max_ra, max_dec]);
        
        self.index
            .locate_in_envelope(&envelope)
            .filter_map(|entry| {
                let star = &self.stars[entry.idx];
                if star.magnitude <= max_magnitude {
                    Some(star)
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Detected star in telescope image
#[derive(Clone, Debug)]
pub struct DetectedStar {
    /// Pixel position
    pub pixel_x: f32,
    pub pixel_y: f32,
    /// Brightness (ADU or normalized)
    pub brightness: f32,
    /// Matched catalog star (if any)
    pub catalog_match: Option<u64>,
}

/// Match detected stars to catalog
pub fn match_stars(
    detected: &[DetectedStar],
    catalog: &StarCatalog,
    lens: &TelescopeLens,
    initial_pointing: (f64, f64), // (RA, Dec)
    fov: f64,
) -> AstrometricSolution {
    // Get candidate catalog stars
    let candidates = catalog.query_fov(
        initial_pointing.0,
        initial_pointing.1,
        fov * 1.5, // Slightly larger to account for pointing error
        6.0, // Magnitude limit
    );
    
    // Build pattern of detected stars
    let detected_pattern = build_star_pattern(detected);
    
    // Build pattern of catalog stars
    let catalog_pattern = build_catalog_pattern(&candidates, lens, initial_pointing);
    
    // Match patterns (triangle matching algorithm)
    let matches = match_patterns(&detected_pattern, &catalog_pattern);
    
    // Compute plate solution
    compute_plate_solution(detected, &candidates, &matches, lens)
}

#[derive(Clone, Debug)]
pub struct AstrometricSolution {
    /// Corrected pointing (RA, Dec)
    pub pointing: (f64, f64),
    /// Rotation angle (radians)
    pub rotation: f64,
    /// Plate scale (arcsec/pixel)
    pub plate_scale: f64,
    /// RMS residual (arcsec)
    pub rms_residual: f64,
    /// Number of matched stars
    pub num_matches: usize,
    /// Matched star pairs (detected_idx, catalog_id)
    pub matches: Vec<(usize, u64)>,
}
```

## Depth Estimation and 3D Positioning

### Depth Scaling

```rust
/// Scale normalized depth to physical distance
pub fn scale_depth(
    normalized_depth: f32,
    object_type: &ObjectType,
    angular_size: f64, // radians
) -> f64 {
    match object_type {
        ObjectType::Satellite => {
            // Use known orbital altitudes
            // GEO: ~35,786 km, LEO: 160-2000 km
            // Estimate from angular size if known satellite size
            let typical_size_m = 10.0; // Assume 10m satellite
            typical_size_m / angular_size
        }
        ObjectType::Debris => {
            // Debris is typically smaller
            let typical_size_m = 0.1;
            typical_size_m / angular_size
        }
        ObjectType::Planet => {
            // Use ephemeris data for planets
            // This is a placeholder - actual implementation uses pracstro
            1e9 // 1 million km placeholder
        }
        ObjectType::Star => {
            // Stars are effectively at infinity
            // Place at maximum render distance
            1e12 // 1 billion km (still within solar system scale)
        }
        _ => {
            // Unknown - use normalized depth scaled to reasonable range
            normalized_depth as f64 * 1e6 // 0-1 million km
        }
    }
}

/// Convert 2D detection + depth to 3D ECEF position
pub fn detection_to_ecef(
    pixel: [f64; 2],
    depth_m: f64,
    lens: &TelescopeLens,
    solution: &AstrometricSolution,
    telescope_ecef: DVec3,
    julian_date: f64,
) -> DVec3 {
    // Pixel to ray in camera frame
    let ray_camera = lens.pixel_to_ray(pixel);
    
    // Apply plate solution rotation
    let rotation = nalgebra::Rotation3::from_euler_angles(
        0.0,
        solution.pointing.1, // Dec
        solution.pointing.0, // RA
    );
    let ray_icrs = rotation * nalgebra::Vector3::new(ray_camera.x, ray_camera.y, ray_camera.z);
    
    // ICRS to ECEF (apply Earth rotation)
    let ray_ecef = icrs_to_ecef(
        DVec3::new(ray_icrs.x, ray_icrs.y, ray_icrs.z),
        julian_date,
    );
    
    // Position = telescope + ray * depth
    telescope_ecef + ray_ecef * depth_m
}
```

## Model Cache System

### Cache Structure

```rust
use bevy::prelude::*;
use std::collections::HashMap;

/// Cached SAM 3D model
#[derive(Clone)]
pub struct CachedSam3dModel {
    /// Unique identifier (hash of observation parameters)
    pub id: u64,
    /// Bevy mesh handle
    pub mesh: Handle<Mesh>,
    /// Optional texture
    pub texture: Option<Handle<Image>>,
    /// Observation timestamp
    pub timestamp: f64,
    /// Celestial coordinates at observation (RA, Dec)
    pub celestial_coords: (f64, f64),
    /// Estimated ECEF position
    pub estimated_ecef: DVec3,
    /// Confidence score (0-1)
    pub confidence: f32,
    /// Object classification
    pub classification: ObjectType,
    /// Reference name if identified
    pub reference_name: Option<String>,
}

/// Model cache resource
#[derive(Resource)]
pub struct Sam3dModelCache {
    /// Models indexed by ID
    models: HashMap<u64, CachedSam3dModel>,
    /// Spatial index for position queries
    spatial_index: rstar::RTree<ModelSpatialEntry>,
    /// LRU order for cache eviction
    lru_order: Vec<u64>,
    /// Maximum cache size
    max_models: usize,
    /// Maximum age before eviction (seconds)
    max_age: f64,
}

struct ModelSpatialEntry {
    id: u64,
    position: [f64; 3],
}

impl rstar::RTreeObject for ModelSpatialEntry {
    type Envelope = rstar::AABB<[f64; 3]>;
    
    fn envelope(&self) -> Self::Envelope {
        rstar::AABB::from_point(self.position)
    }
}

impl Sam3dModelCache {
    pub fn new(max_models: usize, max_age: f64) -> Self {
        Self {
            models: HashMap::new(),
            spatial_index: rstar::RTree::new(),
            lru_order: Vec::new(),
            max_models,
            max_age,
        }
    }
    
    /// Insert or update a model
    pub fn insert(&mut self, model: CachedSam3dModel) {
        let id = model.id;
        let position = [
            model.estimated_ecef.x,
            model.estimated_ecef.y,
            model.estimated_ecef.z,
        ];
        
        // Remove old entry if exists
        if self.models.contains_key(&id) {
            self.remove(id);
        }
        
        // Evict if at capacity
        while self.models.len() >= self.max_models {
            if let Some(oldest_id) = self.lru_order.first().copied() {
                self.remove(oldest_id);
            } else {
                break;
            }
        }
        
        // Insert new model
        self.models.insert(id, model);
        self.spatial_index.insert(ModelSpatialEntry { id, position });
        self.lru_order.push(id);
    }
    
    /// Get model by ID (updates LRU)
    pub fn get(&mut self, id: u64) -> Option<&CachedSam3dModel> {
        if self.models.contains_key(&id) {
            // Update LRU order
            self.lru_order.retain(|&x| x != id);
            self.lru_order.push(id);
            self.models.get(&id)
        } else {
            None
        }
    }
    
    /// Query models near a position
    pub fn query_nearby(&self, ecef: DVec3, radius: f64) -> Vec<&CachedSam3dModel> {
        let min = [ecef.x - radius, ecef.y - radius, ecef.z - radius];
        let max = [ecef.x + radius, ecef.y + radius, ecef.z + radius];
        let envelope = rstar::AABB::from_corners(min, max);
        
        self.spatial_index
            .locate_in_envelope(&envelope)
            .filter_map(|entry| self.models.get(&entry.id))
            .collect()
    }
    
    /// Remove stale models
    pub fn evict_stale(&mut self, current_time: f64) {
        let stale_ids: Vec<u64> = self.models
            .iter()
            .filter(|(_, model)| current_time - model.timestamp > self.max_age)
            .map(|(&id, _)| id)
            .collect();
        
        for id in stale_ids {
            self.remove(id);
        }
    }
    
    fn remove(&mut self, id: u64) {
        self.models.remove(&id);
        self.lru_order.retain(|&x| x != id);
        // Note: rstar doesn't support removal by value, would need rebuild or custom index
    }
}
```

## Bevy Integration

### SAM 3D Component

```rust
/// Component for entities with SAM 3D models
#[derive(Component)]
pub struct Sam3dModel {
    /// Cache ID
    pub cache_id: u64,
    /// Observation quality
    pub quality: ModelQuality,
    /// Last update timestamp
    pub last_updated: f64,
    /// Interpolation with reference model (0 = SAM only, 1 = reference only)
    pub reference_blend: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelQuality {
    /// Low quality, high uncertainty
    Rough,
    /// Medium quality
    Standard,
    /// High quality, multiple observations
    Refined,
    /// Matched to known reference model
    Reference,
}

/// System to spawn SAM 3D models as entities
fn spawn_sam3d_entities(
    mut commands: Commands,
    cache: Res<Sam3dModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    nav_state: Res<NavigationState>,
    existing: Query<(Entity, &Sam3dModel)>,
) {
    // Get models near current position
    let nearby = cache.query_nearby(nav_state.origin_ecef, 1e8); // 100,000 km
    
    // Track existing model IDs
    let existing_ids: std::collections::HashSet<u64> = existing
        .iter()
        .map(|(_, sam)| sam.cache_id)
        .collect();
    
    for model in nearby {
        if existing_ids.contains(&model.id) {
            continue;
        }
        
        // Calculate relative position
        let relative_pos = (model.estimated_ecef - nav_state.origin_ecef).as_vec3();
        
        // Spawn entity
        commands.spawn((
            Mesh3d(model.mesh.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.7, 0.7, 0.8),
                metallic: 0.5,
                ..default()
            })),
            Transform::from_translation(relative_pos),
            OrbitalCoords::from_ecef(model.estimated_ecef),
            Sam3dModel {
                cache_id: model.id,
                quality: ModelQuality::Standard,
                last_updated: model.timestamp,
                reference_blend: 0.0,
            },
            OrbitalObject {
                tle: None,
                body: None,
                object_type: model.classification,
            },
            Name::new(model.reference_name.clone().unwrap_or_else(|| format!("SAM3D_{}", model.id))),
        ));
    }
}
```

### Targeting System

```rust
/// Marker for currently targeted SAM 3D model
#[derive(Component)]
pub struct TargetedSam3d;

/// System to handle SAM 3D model targeting
fn handle_sam3d_targeting(
    mut commands: Commands,
    input: Res<ButtonInput<MouseButton>>,
    camera: Query<(&Camera, &GlobalTransform)>,
    windows: Query<&Window>,
    sam_models: Query<(Entity, &Transform, &Sam3dModel)>,
    targeted: Query<Entity, With<TargetedSam3d>>,
    cache: Res<Sam3dModelCache>,
) {
    if !input.just_pressed(MouseButton::Left) {
        return;
    }
    
    let (camera, camera_transform) = camera.single();
    let window = windows.single();
    
    let Some(cursor_pos) = window.cursor_position() else { return };
    let Some(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else { return };
    
    // Find closest SAM model along ray
    let mut closest: Option<(Entity, f32)> = None;
    
    for (entity, transform, _) in &sam_models {
        let to_model = transform.translation - ray.origin;
        let along_ray = to_model.dot(*ray.direction);
        
        if along_ray < 0.0 {
            continue;
        }
        
        let closest_point = ray.origin + *ray.direction * along_ray;
        let distance_to_ray = (transform.translation - closest_point).length();
        
        // Hit threshold based on model size (simplified)
        let hit_threshold = 100.0; // meters
        
        if distance_to_ray < hit_threshold {
            if closest.map(|(_, d)| along_ray < d).unwrap_or(true) {
                closest = Some((entity, along_ray));
            }
        }
    }
    
    // Clear previous target
    for entity in &targeted {
        commands.entity(entity).remove::<TargetedSam3d>();
    }
    
    // Set new target
    if let Some((entity, _)) = closest {
        commands.entity(entity).insert(TargetedSam3d);
    }
}

/// System to display targeted model details
fn display_targeted_info(
    targeted: Query<(&Sam3dModel, &OrbitalCoords, &Name), With<TargetedSam3d>>,
    cache: Res<Sam3dModelCache>,
    nav_state: Res<NavigationState>,
) {
    for (sam, coords, name) in &targeted {
        let distance = coords.global_ecef.distance(nav_state.origin_ecef);
        
        // Log or display info (would connect to UI system)
        println!("Targeted: {}", name);
        println!("  Distance: {:.1} km", distance / 1000.0);
        println!("  Quality: {:?}", sam.quality);
        println!("  Last Updated: {:.1}s ago", nav_state.last_update - sam.last_updated);
    }
}
```

## Plugin

```rust
pub struct Sam3dIntegrationPlugin;

impl Plugin for Sam3dIntegrationPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_resource(TelescopeFrameBuffer {
                frames: Vec::new(),
                max_frames: 10,
                processing: None,
            })
            .insert_resource(Sam3dModelCache::new(1000, 3600.0))
            .add_systems(Update, (
                spawn_sam3d_entities,
                handle_sam3d_targeting,
                display_targeted_info,
            ));
    }
}
```

## Next Steps

- [07_DYNAMIC_OBJECTS.md](./07_DYNAMIC_OBJECTS.md) - Satellite and debris tracking
- [08_PHYSICS_FOUNDATIONS.md](./08_PHYSICS_FOUNDATIONS.md) - Orbital mechanics details

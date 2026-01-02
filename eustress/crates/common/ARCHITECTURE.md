# Eustress Common Crate Architecture

## Overview

The `eustress-common` crate provides shared types, asset management, and utilities used across all Eustress components (Engine Studio, Client, Server).

## Module Structure

```
eustress-common/src/
├── lib.rs                    # Main entry point
├── classes.rs                # ECS components (Instance, Part, Model, etc.)
├── properties.rs             # Property system for components
├── types.rs                  # Common type definitions
├── utils.rs                  # Utility functions
├── scene.rs                  # Scene serialization (RON format)
├── eustress_format.rs        # File format definitions
├── default_scene.rs          # Default scene spawning
├── xr.rs                     # XR/VR support
│
├── assets/                   # Asset Pipeline
│   ├── mod.rs               # Asset plugin and systems
│   ├── asset_id.rs          # ContentHash (SHA256)
│   ├── source.rs            # AssetSource (Local/IPFS/S3/P2P)
│   ├── resolver.rs          # Async asset resolution
│   ├── service.rs           # AssetService resource
│   ├── bundle.rs            # Asset bundles
│   ├── progressive.rs       # LOD-based loading
│   ├── config.rs            # Configuration
│   ├── categories.rs        # Asset categories (Mesh, Terrain, etc.) ← NEW
│   ├── s3/                  # S3/MinIO support
│   └── p2p/                 # Peer-to-peer sharing
│
├── terrain/                  # Terrain System
│   ├── mod.rs               # TerrainPlugin
│   ├── config.rs            # TerrainConfig, TerrainData
│   ├── chunk.rs             # Chunk management
│   ├── mesh.rs              # Mesh generation
│   ├── lod.rs               # LOD system
│   ├── editor.rs            # Terrain editing
│   ├── material.rs          # Terrain materials
│   ├── history.rs           # Undo/redo
│   ├── brushes.rs           # Paint brushes
│   └── compute.rs           # GPU compute
│
├── pointcloud/               # Point Cloud Processing
│   ├── mod.rs               # Module exports
│   ├── core.rs              # Point data structures
│   ├── processing.rs        # Import pipeline
│   ├── mesh_optimization.rs # Surface reconstruction
│   ├── formats.rs           # Format support (PLY, LAS, FBX, etc.)
│   └── elevation_import.rs  # DEM/GeoTIFF → Terrain ← NEW
│
├── plugins/                  # Shared Bevy Plugins
│   └── ...
│
├── services/                 # Runtime Services
│   └── ...
│
└── soul/                     # Soul Scripting
    └── ...
```

## Asset Pipeline Integration

### How Formats Connect to Assets

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           FILE IMPORT                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  MESH FORMATS              POINT CLOUD           ELEVATION                   │
│  ├── OBJ                   ├── PLY               ├── DEM                     │
│  ├── GLTF/GLB              ├── LAS/LAZ           ├── GeoTIFF                 │
│  ├── FBX                   ├── PTS               ├── HGT (SRTM)              │
│  ├── STL                   ├── XYZ               └── ASC                     │
│  ├── DAE                   ├── DXF                                           │
│  └── USDZ                  └── E57                                           │
│                                                                              │
└──────────────────────────────────┬──────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         PROCESSING PIPELINE                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  pointcloud/formats.rs     pointcloud/processing.rs   elevation_import.rs   │
│  ├── import_mesh()         ├── PointCloudProcessor    ├── elevation_to_     │
│  ├── import_gltf()         ├── clean_points()             terrain()         │
│  ├── import_fbx()          ├── decimate()             ├── split_into_       │
│  └── import_stl()          └── build_octree()             chunks()          │
│                                                                              │
│  mesh_optimization.rs                                                        │
│  ├── reconstruct_surface()                                                   │
│  ├── simplify_mesh()                                                         │
│  └── generate_mesh_lods()                                                    │
│                                                                              │
└──────────────────────────────────┬──────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          ASSET REGISTRATION                                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  assets/categories.rs                                                        │
│  ├── AssetCategory::Mesh      → MeshAsset { hash, lods, bounds }            │
│  ├── AssetCategory::Terrain   → TerrainRegion { hash, heightmap }           │
│  ├── AssetCategory::PointCloud → PointCloudAsset { hash, octree }           │
│  └── AssetCategory::Image     → ImageAsset { hash, dimensions }             │
│                                                                              │
│  assets/service.rs                                                           │
│  ├── AssetService::upload()   → ContentHash (SHA256)                        │
│  ├── AssetService::download() → Cached file                                 │
│  └── AssetRegistry::register() → Indexed by category                        │
│                                                                              │
└──────────────────────────────────┬──────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           STORAGE BACKENDS                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  AssetSource::Local      → ./assets/{hash}                                  │
│  AssetSource::S3         → s3://bucket/{hash}                               │
│  AssetSource::IPFS       → ipfs://{hash}                                    │
│  AssetSource::P2P        → peer://{peer_id}/{hash}                          │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Mesh Upload Flow (Like Roblox)

```rust
use eustress_common::assets::{AssetService, AssetCategory, MeshAsset};
use eustress_common::pointcloud::{import_mesh, generate_mesh_lods};

// 1. Import mesh from file
let mesh_result = import_mesh(Path::new("character.fbx"))?;

// 2. Generate LODs
let lod_set = generate_mesh_lods(&mesh_result.meshes[0], 4)?;

// 3. Upload to asset service
let asset_service = world.resource::<AssetService>();
let mesh_asset = asset_service.upload_mesh(&lod_set)?;

// 4. Use in MeshPart
commands.spawn((
    Instance::new("Character"),
    MeshPart {
        mesh_id: mesh_asset.hash.clone(),  // Like Roblox MeshId
        ..default()
    },
));
```

## Terrain Region Flow

```rust
use eustress_common::pointcloud::{
    import_geotiff_to_terrain, ElevationImportConfig, split_into_chunks
};
use eustress_common::assets::{AssetService, TerrainRegion};

// 1. Import elevation data
let config = ElevationImportConfig::game_terrain();
let result = import_geotiff_to_terrain(Path::new("terrain.tif"), &config)?;

// 2. Split into regions
let chunks = split_into_chunks(&elevation, &config);

// 3. Upload each region
let asset_service = world.resource::<AssetService>();
for chunk in chunks {
    let region = asset_service.upload_terrain_region(&chunk)?;
    // region.hash can be saved/shared
}

// 4. Load region into terrain
let terrain_entity = commands.spawn((
    TerrainConfig::from_import(&result.config),
    TerrainData::from_import(&result.data),
));
```

## Asset Categories (Like Roblox Toolbox)

| Category | Extensions | Asset Type | Usage |
|----------|------------|------------|-------|
| **Mesh** | gltf, fbx, obj | `MeshAsset` | `MeshPart.mesh_id` |
| **Image** | png, jpg, hdr | `ImageAsset` | Textures, Decals |
| **Audio** | wav, ogg, mp3 | `AudioAsset` | `Sound.sound_id` |
| **Terrain** | dem, tif, hgt | `TerrainRegion` | Terrain chunks |
| **PointCloud** | ply, las, pts | `PointCloudAsset` | Visualization |
| **Animation** | anim, bvh | `AnimationAsset` | `Animator` |
| **Script** | soul | `ScriptAsset` | Soul scripts |
| **Package** | eustresspack | `PackageAsset` | Bundled models |

## Proposed Restructure

To better organize the common crate, consider this structure:

```
eustress-common/src/
├── lib.rs
│
├── core/                     # Core types (moved from root)
│   ├── mod.rs
│   ├── classes.rs           # ECS components
│   ├── properties.rs        # Property system
│   ├── types.rs             # Common types
│   └── utils.rs             # Utilities
│
├── assets/                   # Asset pipeline (unchanged)
│   └── ...
│
├── formats/                  # File format support (NEW - consolidated)
│   ├── mod.rs
│   ├── mesh/                # Mesh formats
│   │   ├── gltf.rs
│   │   ├── fbx.rs
│   │   ├── obj.rs
│   │   └── stl.rs
│   ├── pointcloud/          # Point cloud formats
│   │   ├── ply.rs
│   │   ├── las.rs
│   │   └── pts.rs
│   ├── elevation/           # Elevation formats
│   │   ├── geotiff.rs
│   │   ├── dem.rs
│   │   └── hgt.rs
│   └── scene/               # Scene formats
│       ├── eustress.rs
│       └── binary.rs
│
├── terrain/                  # Terrain system (unchanged)
│   └── ...
│
├── processing/               # Processing pipelines (NEW - consolidated)
│   ├── mod.rs
│   ├── mesh_optimization.rs
│   ├── pointcloud_processing.rs
│   └── elevation_import.rs
│
├── plugins/                  # Bevy plugins
│   └── ...
│
├── services/                 # Runtime services
│   └── ...
│
└── soul/                     # Scripting
    └── ...
```

## Key Integration Points

### 1. MeshPart with Uploaded Mesh
```rust
// In classes.rs
pub struct MeshPart {
    pub mesh_id: String,        // ContentHash of uploaded mesh
    pub texture_id: String,     // ContentHash of texture
    pub lod_mode: LODMode,      // Auto, Manual, Distance
    // ...
}
```

### 2. Terrain with Regions
```rust
// In terrain/config.rs
pub struct TerrainData {
    pub regions: Vec<String>,   // ContentHashes of TerrainRegions
    pub height_cache: Vec<f32>, // Merged from regions
    // ...
}
```

### 3. Asset Browser UI
```rust
// In engine UI
fn asset_browser(registry: &AssetRegistry) {
    // Categories sidebar
    for category in AssetCategory::iter() {
        if ui.button(category.display_name()) {
            let assets = registry.get_by_category(category);
            // Show grid of assets with thumbnails
        }
    }
}
```

## Next Steps

1. **Mesh Upload UI** - Add "Upload Mesh" button in Studio
2. **Terrain Region Save/Load** - Save terrain edits as regions
3. **Asset Browser** - Roblox-style toolbox with categories
4. **Thumbnail Generation** - Auto-generate previews for assets
5. **Search & Tags** - Full-text search across assets

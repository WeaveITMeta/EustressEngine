# Local-First Geospatial for Eustress Engine

> **Replace PostGIS with file-system-first geospatial data for 3D map visualization**
>
> No database server. No Docker. No connection strings. Just files.

---

## Table of Contents

1. [Problem Statement](#1-problem-statement)
2. [What PostGIS Actually Provides](#2-what-postgis-actually-provides)
3. [Why PostGIS Violates File-System-First](#3-why-postgis-violates-file-system-first)
4. [File Format Evaluation](#4-file-format-evaluation)
5. [Chosen Stack](#5-chosen-stack)
6. [Architecture](#6-architecture)
7. [Coordinate Systems](#7-coordinate-systems)
8. [Data Pipeline: Raw → Eustress Scene](#8-data-pipeline-raw--eustress-scene)
9. [3D Rendering in Bevy](#9-3d-rendering-in-bevy)
10. [WATER Project Use Case](#10-water-project-use-case)
11. [Rust Crate Ecosystem](#11-rust-crate-ecosystem)
12. [Project Structure](#12-project-structure)
13. [Migration Path](#13-migration-path)
14. [Performance Considerations](#14-performance-considerations)

---

## 1. Problem Statement

We need to visualize 3D maps in Eustress — terrain elevation, pipeline routes, aquifer boundaries, well locations, political boundaries, satellite imagery — for projects like the Indo-Gangetic Basin Water Project (IGBWP).

PostGIS is the standard tool for geospatial queries (spatial joins, buffer analysis, coordinate transforms). But PostGIS is a **server process** — it requires PostgreSQL running, connection management, SQL queries, and a daemon. This fundamentally conflicts with Eustress's file-system-first architecture where opening a folder = opening a project.

**Goal:** Identify which PostGIS capabilities we actually need, then replace each with a local file-based equivalent that lives in the project directory, is git-diffable, and loads directly into Bevy's ECS.

---

## 2. What PostGIS Actually Provides

PostGIS gives you six categories of capability. Not all are needed for 3D visualization:

| Capability | PostGIS Feature | Do We Need It? | Local Alternative |
|-----------|----------------|----------------|-------------------|
| **Geometry storage** | `geometry`/`geography` columns | Yes | GeoPackage, FlatGeobuf, GeoJSON files |
| **Spatial indexing** | R-tree (GiST) | Yes, for large datasets | GeoPackage has built-in R-tree; FlatGeobuf has spatial index |
| **Spatial queries** | `ST_Contains`, `ST_Intersects`, `ST_Buffer` | Partially — for data prep, not runtime | `geo` crate (Rust) for runtime; GDAL/OGR CLI for batch |
| **Coordinate transforms** | `ST_Transform`, SRID system | Yes | `proj` crate (Rust bindings to PROJ library) |
| **Raster analysis** | `ST_Value`, `ST_MapAlgebra` | For elevation/DEM | GeoTIFF files + `gdal` crate or direct heightmap parsing |
| **Topology** | `ST_Split`, `ST_Polygonize` | Rarely | `geo` crate or pre-process with QGIS |

**Key insight:** For a 3D visualization engine, we need **read + render**, not **query + mutate**. PostGIS is optimized for the latter. We can replace 90% of what we need with smart file formats and a few Rust crates.

---

## 3. Why PostGIS Violates File-System-First

| Principle | PostGIS | File-System-First |
|-----------|---------|-------------------|
| Open folder = open project | Need `pg_ctl start`, connection string | `assets/geo/` directory just works |
| Git-diffable | Binary WAL, opaque storage | GeoJSON is JSON, GeoPackage is SQLite |
| No daemon | PostgreSQL server process | Zero processes |
| Portable | OS-specific install, extensions | Files copy anywhere |
| Offline | Needs running DB | Always offline |
| Reproducible | Schema migrations, dump/restore | Files are the source of truth |

---

## 4. File Format Evaluation

### Vector Formats (points, lines, polygons)

| Format | Extension | Spatial Index | Streaming | Git-Diffable | Size | Best For |
|--------|-----------|--------------|-----------|--------------|------|----------|
| **GeoJSON** | `.geojson` | No | No | **Yes** | Large (text) | Small datasets (<10k features), human-readable |
| **GeoPackage** | `.gpkg` | **R-tree built-in** | No | No (SQLite binary) | Medium | Medium datasets, multiple layers, the "SQLite of GIS" |
| **FlatGeobuf** | `.fgb` | **Packed Hilbert R-tree** | **Yes** | No (binary) | Small | Large datasets, HTTP range requests, fast spatial queries |
| **Shapefile** | `.shp/.dbf/.shx` | `.shx` index | No | No | Medium | Legacy compatibility only — **avoid** |
| **GeoParquet** | `.parquet` | Column stats | **Yes** | No (columnar binary) | **Smallest** | Analytics, millions of features, DuckDB integration |
| **PMTiles** | `.pmtiles` | **Built-in tile index** | **Yes** | No (binary) | Small | Pre-tiled vector/raster for web maps |

### Raster/Elevation Formats

| Format | Extension | Tiled | Compression | Best For |
|--------|-----------|-------|-------------|----------|
| **GeoTIFF** | `.tif` | COG variant | LZW/Deflate/ZSTD | DEM, satellite imagery, elevation |
| **Cloud Optimized GeoTIFF (COG)** | `.tif` | **Yes** | ZSTD | Large rasters, range-request streaming |
| **HGT** | `.hgt` | No | None | SRTM elevation tiles (fixed 1°×1° grid) |
| **ASC** | `.asc` | No | None | Simple ASCII grid — human readable |
| **PNG heightmap** | `.png` | No | PNG | Game-engine-native, 16-bit grayscale |

### Recommendation

**Use a tiered approach based on dataset size:**

| Dataset Size | Vector Format | Raster Format |
|-------------|---------------|---------------|
| < 10k features | **GeoJSON** (git-diffable, human-editable) | **PNG heightmap** (Bevy-native) |
| 10k–1M features | **GeoPackage** (spatial index, multi-layer) | **GeoTIFF** (georeferenced) |
| > 1M features | **FlatGeobuf** or **GeoParquet** | **COG** (tiled, streamable) |

---

## 5. Chosen Stack

### Runtime (Rust, in-engine)

| Need | Solution | Crate |
|------|----------|-------|
| Parse GeoJSON | `geojson` crate | `geojson = "0.24"` |
| Geometry operations | `geo` crate (contains, intersects, buffer, distance, area) | `geo = "0.28"` |
| Coordinate transforms | `proj` crate (Rust bindings to PROJ) | `proj = "0.27"` |
| Read GeoPackage | `sqlite` + manual parsing, or `gdal` crate | `rusqlite = "0.31"` |
| Read FlatGeobuf | `flatgeobuf` crate | `flatgeobuf = "4.1"` |
| Read GeoTIFF | `tiff` crate + georef parsing | `tiff = "0.9"` |
| Read Shapefiles | `shapefile` crate | `shapefile = "0.6"` |
| Spatial indexing | `rstar` crate (R-tree) | `rstar = "0.12"` |
| S2 cell indexing | `s2` crate (Google S2 geometry) | `s2 = "0.0.12"` |

### Offline Processing (CLI tools, not runtime)

| Need | Tool |
|------|------|
| Batch coordinate transforms | `ogr2ogr` (GDAL CLI) |
| Raster reprojection | `gdalwarp` |
| Tile generation | `tippecanoe` (vector tiles), `gdal2tiles` (raster) |
| Visual inspection | QGIS (free, opens all formats) |
| Elevation → heightmap | `gdal_translate -of PNG` |

---

## 6. Architecture

```
my-game/
├── assets/
│   └── geo/                          ← Geospatial data directory
│       ├── terrain/
│       │   ├── srtm_n26e080.hgt      ← Raw SRTM elevation tile
│       │   ├── dem_kanpur.tif         ← GeoTIFF DEM
│       │   └── heightmap_kanpur.png   ← Derived PNG heightmap (cached)
│       ├── vectors/
│       │   ├── pipeline_route.geojson ← Pipeline centerline (git-diffable)
│       │   ├── aquifer_boundary.gpkg  ← Aquifer polygons (spatial indexed)
│       │   ├── well_locations.geojson ← Well points with attributes
│       │   └── admin_boundaries.fgb   ← State/district boundaries (large)
│       ├── imagery/
│       │   ├── sentinel_kanpur.tif    ← Satellite imagery (COG)
│       │   └── landuse.pmtiles        ← Pre-tiled land use
│       └── geo.toml                   ← Geospatial project config
├── .eustress/
│   └── cache/
│       └── geo/                       ← Derived geospatial cache
│           ├── terrain_meshes/        ← Generated .glb terrain chunks
│           ├── vector_meshes/         ← Extruded pipeline/boundary meshes
│           └── tile_index.bin         ← Spatial index cache
└── scenes/
    └── water_project.gltf            ← Scene referencing geo assets
```

### `geo.toml` — Geospatial Project Configuration

```toml
[project]
name = "IGBWP Visualization"
# Origin point for local coordinate system (all geo data projected relative to this)
origin_lat = 25.43
origin_lon = 81.88
origin_name = "Prayagraj Terminus"
# Target CRS for all data (UTM Zone 44N covers most of UP)
target_crs = "EPSG:32644"

[terrain]
# Elevation data sources (processed in order, later overrides earlier)
sources = [
    { path = "terrain/srtm_n26e080.hgt", format = "hgt" },
    { path = "terrain/dem_kanpur.tif", format = "geotiff" },
]
height_scale = 1.0          # 1:1 real-world meters
vertical_exaggeration = 3.0 # Exaggerate for visibility at map scale
chunk_size = 1000.0         # 1km chunks in world units
chunk_resolution = 128      # Vertices per chunk side

[layers]
# Each layer becomes a Bevy entity group
[[layers.vector]]
name = "Pipeline Route"
path = "vectors/pipeline_route.geojson"
style = { color = [0.2, 0.5, 1.0], width = 20.0, extrude = true, height = 5.0 }

[[layers.vector]]
name = "Aquifer Boundary"
path = "vectors/aquifer_boundary.gpkg"
layer = "shallow_alluvial"  # GeoPackage layer name
style = { color = [0.3, 0.8, 0.4, 0.3], extrude = false }

[[layers.vector]]
name = "Well Locations"
path = "vectors/well_locations.geojson"
style = { marker = "cylinder", radius = 50.0, color = [1.0, 0.3, 0.3] }

[[layers.vector]]
name = "Admin Boundaries"
path = "vectors/admin_boundaries.fgb"
style = { color = [0.5, 0.5, 0.5, 0.5], width = 2.0 }

[[layers.raster]]
name = "Satellite Imagery"
path = "imagery/sentinel_kanpur.tif"
drape = true  # Drape onto terrain surface
```

---

## 7. Coordinate Systems

### The Core Problem

Geospatial data uses geographic coordinates (lat/lon in degrees on an ellipsoid). Bevy uses a local Cartesian coordinate system (meters, Y-up). We need a projection pipeline.

### Solution: Project-Local Origin

Every Eustress geospatial project defines an **origin point** in `geo.toml`. All coordinates are transformed to meters relative to this origin:

```
Geographic (WGS84)  →  Projected (UTM)  →  Local (Bevy meters, Y-up)
  lat/lon degrees       easting/northing      x/z meters from origin
```

**Pipeline:**
1. Raw data arrives in any CRS (WGS84, UTM, state plane, etc.)
2. `proj` crate transforms to the project's target CRS (e.g., UTM 44N)
3. Subtract origin easting/northing → local meters
4. Swap axes: GIS (X=East, Y=North, Z=Up) → Bevy (X=East, Y=Up, Z=South)

```rust
/// Transform geographic coordinates to Bevy world position
pub fn geo_to_world(lat: f64, lon: f64, config: &GeoConfig) -> Vec3 {
    // Step 1: WGS84 → UTM via proj
    let (easting, northing) = proj_transform(lat, lon, &config.target_crs);
    
    // Step 2: UTM → local meters (subtract origin)
    let x = (easting - config.origin_easting) as f32;
    let z = -(northing - config.origin_northing) as f32; // Negate: GIS North → Bevy -Z
    
    // Step 3: Sample terrain elevation at this point
    let y = sample_terrain_height(x, z);
    
    Vec3::new(x, y, z)
}
```

### Existing Infrastructure

Eustress already has coordinate transforms in `elevation_import.rs`:
- `CoordinateSystem` enum (Local, Geographic, UTM)
- `geographic_to_local()` — equirectangular approximation
- `utm_to_local()` — simple offset subtraction

**Upgrade path:** Replace the equirectangular approximation with `proj` crate for accurate transforms at any scale.

---

## 8. Data Pipeline: Raw → Eustress Scene

### Phase 1: Import (offline or on first load)

```
Raw GeoTIFF/HGT  →  Parse elevation grid  →  Generate heightmap PNG
                                            →  Generate terrain mesh chunks (.glb)
                                            →  Cache in .eustress/cache/geo/

Raw GeoJSON/GPKG  →  Parse geometries      →  Project to local coords
                                            →  Build R-tree spatial index
                                            →  Generate 3D meshes (.glb)
                                            →  Cache in .eustress/cache/geo/
```

### Phase 2: Runtime (Bevy ECS)

```
GeoLayerPlugin
  ├── startup: read geo.toml, load spatial indices
  ├── system: spawn_terrain_chunks (LOD-based, camera distance)
  ├── system: spawn_vector_features (frustum-culled, LOD)
  ├── system: drape_imagery (project raster onto terrain mesh UVs)
  └── system: update_geo_labels (billboarded text for cities/wells)
```

### Terrain Mesh Generation

For each terrain chunk:
1. Sample elevation grid at chunk resolution
2. Generate vertex positions (x, elevation, z)
3. Generate triangle indices (regular grid → triangle strip)
4. Compute normals from adjacent heights
5. Export as `.glb` (file-system-first: mesh is a file)
6. At runtime: `AssetServer::load("cache/geo/terrain_meshes/chunk_3_7.glb")`

### Vector Feature Rendering

| Geometry Type | 3D Representation |
|--------------|-------------------|
| **Point** | Billboard sprite, 3D marker (cylinder/sphere), or instanced mesh |
| **LineString** | Extruded tube/ribbon mesh draped on terrain |
| **Polygon** | Extruded prism, or flat polygon draped on terrain with transparency |
| **MultiPolygon** | Same as Polygon, batched |

---

## 9. 3D Rendering in Bevy

### Components

```rust
/// Marks an entity as a geospatial feature
#[derive(Component, Reflect)]
pub struct GeoFeature {
    /// Original coordinates (for inspector display)
    pub lat: f64,
    pub lon: f64,
    /// Source file path (file-system-first)
    pub source: String,
    /// Feature ID within source
    pub feature_id: Option<u64>,
}

/// Geospatial layer grouping
#[derive(Component, Reflect)]
pub struct GeoLayer {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
}

/// Terrain chunk with LOD
#[derive(Component, Reflect)]
pub struct GeoTerrainChunk {
    pub grid_x: i32,
    pub grid_z: i32,
    pub lod: u32,
    /// Path to .glb mesh (file-system-first)
    pub mesh_path: String,
}
```

### Integration with Existing Terrain System

Eustress already has a full terrain system (`crates/common/src/terrain/`) with:
- Chunk-based LOD
- Heightmap support
- Splatmap texture blending
- Physics colliders
- Procedural generation

**The geospatial layer extends this** by adding:
- Real-world coordinate mapping (origin + projection)
- GeoTIFF/HGT import → heightmap conversion
- Vector overlay rendering (pipelines, boundaries)
- Satellite imagery draping

We don't replace the terrain system — we feed it real-world data.

---

## 10. WATER Project Use Case

For the IGBWP visualization, the concrete data layers are:

| Layer | Format | Source | Rendering |
|-------|--------|--------|-----------|
| **Terrain elevation** | SRTM HGT tiles | NASA SRTM 30m | 3D terrain mesh, 3× vertical exaggeration |
| **Pipeline route** | GeoJSON LineString | Hand-authored from IGBWP.md coordinates | Blue extruded tube on terrain |
| **Desalination plants** | GeoJSON Points | Paradip, Dhamra, Haldia coordinates | 3D building models or markers |
| **Pump stations** | GeoJSON Points | Raipur, Sambalpur, Mirzapur | Cylinder markers |
| **Aquifer boundary** | GeoJSON/GPKG Polygon | CGWB data | Semi-transparent green overlay |
| **Recharge zones** | GeoJSON Polygons | Prayagraj terminus area | Animated blue overlay |
| **State boundaries** | FlatGeobuf | Natural Earth / GADM | Thin white lines |
| **River centerlines** | GeoJSON LineStrings | OpenStreetMap / HydroSHEDS | Blue lines on terrain |
| **Well locations** | GeoJSON Points | CGWB monitoring wells | Red cylinders, height = depth |
| **Elevation profile** | Derived from terrain | Sample along pipeline route | 2D chart overlay or 3D ribbon |
| **Satellite imagery** | COG GeoTIFF | Sentinel-2 / Landsat | Draped on terrain |

### Example: Pipeline Route GeoJSON

```json
{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "properties": {
        "name": "Paradip → Prayagraj Pipeline",
        "node": "Node 1",
        "capacity_m3s": 200,
        "diameter_m": 4.5,
        "status": "proposed"
      },
      "geometry": {
        "type": "LineString",
        "coordinates": [
          [86.61, 20.32, 0],
          [83.97, 21.47, 170],
          [81.63, 21.25, 300],
          [82.57, 25.15, 80],
          [81.88, 25.43, 98]
        ]
      }
    }
  ]
}
```

This file lives at `assets/geo/vectors/pipeline_route.geojson` — editable in any text editor, diffable in git, renderable in QGIS and Eustress.

---

## 11. Rust Crate Ecosystem

### Core Geospatial

| Crate | Purpose | Maturity |
|-------|---------|----------|
| [`geo`](https://crates.io/crates/geo) | Geometry types + algorithms (area, distance, contains, buffer, simplify) | Stable, widely used |
| [`geojson`](https://crates.io/crates/geojson) | GeoJSON parsing/serialization | Stable |
| [`proj`](https://crates.io/crates/proj) | Coordinate transforms (Rust bindings to PROJ) | Stable |
| [`rstar`](https://crates.io/crates/rstar) | R-tree spatial index | Stable |
| [`flatgeobuf`](https://crates.io/crates/flatgeobuf) | FlatGeobuf reader (spatial-indexed binary vector) | Stable |
| [`shapefile`](https://crates.io/crates/shapefile) | Shapefile reader | Stable |
| [`tiff`](https://crates.io/crates/tiff) | TIFF/GeoTIFF reader | Stable |
| [`gdal`](https://crates.io/crates/gdal) | Full GDAL bindings (heavy, C dependency) | Stable but heavy |

### Supporting

| Crate | Purpose |
|-------|---------|
| [`rusqlite`](https://crates.io/crates/rusqlite) | SQLite (for reading GeoPackage) |
| [`wkt`](https://crates.io/crates/wkt) | Well-Known Text geometry parsing |
| [`geozero`](https://crates.io/crates/geozero) | Zero-copy geometry processing, format conversion |
| [`h3o`](https://crates.io/crates/h3o) | Uber H3 hexagonal spatial index |
| [`s2`](https://crates.io/crates/s2) | Google S2 spherical geometry |

### Recommendation: Start Lean

**Phase 1 (minimum viable):**
- `geo` + `geojson` + `proj` + `rstar` + `tiff`
- Covers: parse GeoJSON vectors, transform coordinates, build spatial index, read elevation rasters

**Phase 2 (scale up):**
- Add `flatgeobuf` for large vector datasets
- Add `rusqlite` for GeoPackage support
- Add `geozero` for format-agnostic geometry processing

**Phase 3 (full stack):**
- Add `gdal` bindings for anything exotic
- Add `h3o` for hexagonal binning / analytics

**Avoid `gdal` in Phase 1** — it's a heavy C dependency that complicates cross-compilation. The pure-Rust crates cover 90% of needs.

---

## 12. Project Structure

### New Crate: `eustress-geo`

```
eustress/crates/geo/
├── Cargo.toml
└── src/
    ├── lib.rs              ← Plugin registration
    ├── config.rs           ← GeoConfig from geo.toml
    ├── coords.rs           ← Coordinate transforms (proj wrapper)
    ├── layers.rs           ← GeoLayer, GeoFeature components
    ├── terrain_import.rs   ← GeoTIFF/HGT → terrain mesh pipeline
    ├── vector_import.rs    ← GeoJSON/FGB → 3D mesh pipeline
    ├── vector_render.rs    ← LineString → tube mesh, Polygon → extruded mesh
    ├── spatial_index.rs    ← R-tree wrapper for runtime queries
    ├── imagery.rs          ← Satellite imagery draping
    └── inspector.rs        ← Slint UI: layer toggle, feature properties
```

### Cargo.toml

```toml
[package]
name = "eustress-geo"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy = { workspace = true }
geo = "0.28"
geojson = "0.24"
proj = "0.27"
rstar = "0.12"
tiff = "0.9"
toml = { workspace = true }
serde = { workspace = true }

[features]
default = []
flatgeobuf = ["dep:flatgeobuf"]
geopackage = ["dep:rusqlite"]
gdal = ["dep:gdal"]

[dependencies.flatgeobuf]
version = "4.1"
optional = true

[dependencies.rusqlite]
version = "0.31"
optional = true
features = ["bundled"]

[dependencies.gdal]
version = "0.17"
optional = true
```

---

## 13. Migration Path

### From PostGIS Workflow

| PostGIS Workflow | File-System-First Equivalent |
|-----------------|------------------------------|
| `psql -c "CREATE TABLE wells ..."` | Create `wells.geojson` in text editor |
| `shp2pgsql -s 4326 boundary.shp` | Copy `boundary.fgb` to `assets/geo/vectors/` |
| `SELECT ST_Transform(geom, 32644)` | `proj` crate at import time, or `ogr2ogr` CLI |
| `SELECT * FROM wells WHERE ST_DWithin(...)` | `rstar` R-tree query at runtime |
| `SELECT ST_Buffer(pipeline, 1000)` | `geo::algorithm::Buffer` at runtime |
| `raster2pgsql -s 4326 dem.tif` | Copy `dem.tif` to `assets/geo/terrain/` |
| `SELECT ST_Value(raster, point)` | Sample heightmap array at runtime |
| `pg_dump > backup.sql` | `git commit` (files are the backup) |

### From Existing Eustress Terrain

The existing `TerrainPlugin` and `elevation_import.rs` already handle:
- Heightmap → mesh generation
- Chunk-based LOD
- Coordinate transforms (basic)

**What `eustress-geo` adds:**
- `geo.toml` config (declarative layer definitions)
- Accurate projection via `proj` (replaces equirectangular approximation)
- Vector feature rendering (pipelines, boundaries, points)
- Satellite imagery draping
- Spatial indexing for large datasets
- Layer management UI in Slint explorer

---

## 14. Performance Considerations

### Terrain

| Scale | Approach |
|-------|----------|
| City (10 km²) | Single GeoTIFF → one terrain entity, no chunking needed |
| Region (10,000 km²) | Chunked terrain, LOD, frustum culling (existing system) |
| Country (1M km²) | Tiled COG + on-demand chunk loading, aggressive LOD |
| Planet | PMTiles + LOD pyramid, only load visible tiles |

### Vectors

| Feature Count | Approach |
|--------------|----------|
| < 1,000 | Load all, render as individual meshes |
| 1,000–100,000 | R-tree spatial index, frustum-cull, instance rendering |
| > 100,000 | FlatGeobuf spatial filter, stream visible features only |

### Memory Budget

For the IGBWP visualization (990 km pipeline, ~500,000 km² terrain):
- **Terrain:** ~100 SRTM tiles × 2.8 MB = ~280 MB raw, chunked/LOD'd to ~50 MB GPU
- **Vectors:** Pipeline + boundaries + wells < 10 MB GeoJSON
- **Imagery:** Sentinel-2 COG, stream visible tiles only, ~100 MB GPU budget
- **Total:** ~160 MB GPU memory — well within budget

### Key Optimization: Derived Cache

All heavy processing (mesh generation, coordinate transforms, spatial indexing) happens once and caches to `.eustress/cache/geo/`. Runtime just loads `.glb` meshes and pre-built indices.

```
First load:  GeoTIFF → parse → project → mesh → .glb cache    (slow, seconds)
Subsequent:  .glb cache → AssetServer::load                     (fast, milliseconds)
```

Cache invalidation: mtime check on source file → content hash if changed → regenerate.

---

## Summary

**PostGIS is a database. We need a file system.**

The local-first geospatial stack for Eustress:
1. **Store** geospatial data as files: GeoJSON (small/diffable), GeoPackage (medium), FlatGeobuf (large), GeoTIFF (raster)
2. **Transform** coordinates with `proj` crate (accurate, no server)
3. **Index** spatially with `rstar` R-tree (in-memory, fast)
4. **Render** as Bevy meshes: terrain chunks from heightmaps, extruded tubes for pipelines, markers for points
5. **Cache** derived meshes as `.glb` files in `.eustress/cache/geo/` (file-system-first)
6. **Configure** declaratively via `geo.toml` (git-diffable, human-readable)

No PostgreSQL. No Docker. No connection strings. Open the folder, see the map.

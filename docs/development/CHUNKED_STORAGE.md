# Chunked Storage System

This document defines the chunked binary storage system for Eustress Engine, enabling scalability to **10M+ instances** while maintaining the file-system-first philosophy for human-editable content.

---

## Table of Contents

1. [Overview](#overview)
2. [Scalability Tiers](#scalability-tiers)
3. [Hybrid Architecture](#hybrid-architecture)
4. [Chunk Format](#chunk-format)
5. [Manifest Format](#manifest-format)
6. [Streaming System](#streaming-system)
7. [Integration with EEP](#integration-with-eep)
8. [Studio Integration](#studio-integration)
9. [Performance Targets](#performance-targets)
10. [Implementation Checklist](#implementation-checklist)

---

## Overview

### The Problem

The TOML file-per-instance model works excellently for:
- Hand-crafted levels (< 10K instances)
- Human-readable, git-friendly content
- Hot-reload during development

But it breaks down at scale:

| Instance Count | Files | Directory Listing | Git Status | Cold Load |
|----------------|-------|-------------------|------------|-----------|
| 1,000 | 1K | Instant | Instant | < 1s |
| 10,000 | 10K | Fast | Slow | ~5s |
| 100,000 | 100K | Sluggish | Very slow | ~30s |
| 1,000,000 | 1M | Minutes | Unusable | Minutes |
| 10,000,000 | 10M | **Fails** | **Fails** | **Fails** |

### The Solution

**Chunked binary storage** for large-scale content:
- Spatial partitioning into fixed-size chunks
- Binary format for compact storage and fast parsing
- Streaming load/unload based on camera position
- TOML manifest for chunk metadata

**TOML remains authoritative** for:
- Player models, characters, vehicles
- UI (ScreenGui, Frame, etc.)
- Scripts (SoulScript)
- Hand-placed props and decorations
- Anything a human might edit

---

## Scalability Tiers

| Tier | Instance Count | Storage Strategy | Use Case |
|------|----------------|------------------|----------|
| **Tiny** | < 1K | TOML only | Prototypes, demos |
| **Small** | 1K - 10K | TOML only | Hand-crafted levels |
| **Medium** | 10K - 100K | TOML + lazy loading | Large maps |
| **Large** | 100K - 1M | Chunked binary | Open worlds |
| **Massive** | 1M - 10M | Chunked + LOD streaming | MMO zones |
| **Planetary** | 10M+ | Chunked + procedural + LOD | Procedural planets |

The engine automatically selects the appropriate strategy based on content.

---

## Hybrid Architecture

### Directory Structure

```
MyGame/
├── Workspace/
│   ├── _service.toml
│   │
│   │   ══════════════════════════════════════════════════════
│   │   TOML ZONE: Human-editable, git-friendly, < 10K instances
│   │   ══════════════════════════════════════════════════════
│   │
│   ├── Player/                        ← Model (TOML)
│   │   ├── _instance.toml
│   │   ├── HumanoidRootPart.part.toml
│   │   └── ...
│   │
│   ├── Vehicles/                      ← Folder (TOML)
│   │   ├── _instance.toml
│   │   └── Car.part.toml
│   │
│   ├── Props/                         ← Folder (TOML)
│   │   ├── _instance.toml
│   │   ├── Bench.part.toml
│   │   └── Lamp.part.toml
│   │
│   │   ══════════════════════════════════════════════════════
│   │   CHUNKED ZONE: Binary, streaming, 10K - 10M+ instances
│   │   ══════════════════════════════════════════════════════
│   │
│   └── World/                         ← ChunkedWorld container
│       ├── _instance.toml             ← class_name = "ChunkedWorld"
│       ├── manifest.toml              ← Chunk index and metadata
│       ├── chunks/
│       │   ├── 0_0_0.echk             ← Binary chunk file
│       │   ├── 0_0_1.echk
│       │   ├── 0_1_0.echk
│       │   ├── 1_0_0.echk
│       │   └── ...
│       └── lod/                       ← Optional LOD data
│           ├── lod1/
│           │   └── ...
│           └── lod2/
│               └── ...
│
├── .eustress/
│   └── cache/
│       └── chunks/
│           ├── 0_0_0.physics          ← Cached physics data
│           ├── 0_0_0.navmesh          ← Cached navigation
│           └── ...
```

### ChunkedWorld Instance

```toml
# Workspace/World/_instance.toml
[instance]
name = "World"
class_name = "ChunkedWorld"
archivable = true
ai = false

[chunked_world]
# Chunk dimensions in meters
chunk_size = [256.0, 256.0, 256.0]

# World bounds (chunks outside this are invalid)
min_chunk = [-64, -4, -64]
max_chunk = [63, 3, 63]

# Streaming configuration
load_radius = 3              # Chunks to load around camera
unload_radius = 5            # Chunks to keep loaded (hysteresis)
lod_distances = [256.0, 512.0, 1024.0]  # LOD transition distances

# Compression
compression = "lz4"          # none | lz4 | zstd

[metadata]
id = "world-001"
created = "2026-03-02T10:00:00Z"
last_modified = "2026-03-02T10:00:00Z"
```

---

## Chunk Format

### File Extension

`.echk` — Eustress Chunk

### Binary Layout

```
┌─────────────────────────────────────────────────────────────┐
│                      CHUNK FILE (.echk)                      │
├─────────────────────────────────────────────────────────────┤
│  Header (64 bytes)                                           │
│  ├── Magic: "ECHK" (4 bytes)                                │
│  ├── Version: u32 (4 bytes)                                 │
│  ├── Flags: u32 (4 bytes)                                   │
│  ├── Instance Count: u32 (4 bytes)                          │
│  ├── Chunk Coords: [i32; 3] (12 bytes)                      │
│  ├── Bounds Min: [f32; 3] (12 bytes)                        │
│  ├── Bounds Max: [f32; 3] (12 bytes)                        │
│  ├── Data Offset: u64 (8 bytes)                             │
│  └── Reserved (4 bytes)                                     │
├─────────────────────────────────────────────────────────────┤
│  Class Table (variable)                                      │
│  ├── Class Count: u16                                       │
│  └── Classes: [ClassEntry; count]                           │
│      ├── Class ID: u16                                      │
│      ├── Name Length: u8                                    │
│      └── Name: [u8; length]                                 │
├─────────────────────────────────────────────────────────────┤
│  Material Table (variable)                                   │
│  ├── Material Count: u16                                    │
│  └── Materials: [MaterialEntry; count]                      │
│      ├── Material ID: u8                                    │
│      ├── Name Length: u8                                    │
│      └── Name: [u8; length]                                 │
├─────────────────────────────────────────────────────────────┤
│  Mesh Reference Table (variable)                             │
│  ├── Mesh Count: u16                                        │
│  └── Meshes: [MeshEntry; count]                             │
│      ├── Mesh ID: u16                                       │
│      ├── Path Length: u16                                   │
│      └── Path: [u8; length]  (e.g., "assets/meshes/tree.glb")│
├─────────────────────────────────────────────────────────────┤
│  Instance Data (variable, bulk of file)                      │
│  └── Instances: [PackedInstance; instance_count]            │
└─────────────────────────────────────────────────────────────┘
```

### PackedInstance Structure

```rust
/// Compact instance representation (48 bytes typical)
#[repr(C, packed)]
struct PackedInstance {
    // Identity (4 bytes)
    class_id: u16,           // Index into class table
    flags: u16,              // Bit flags (see below)
    
    // Transform (40 bytes)
    position: [f32; 3],      // 12 bytes - world position
    rotation: [f32; 4],      // 16 bytes - quaternion
    scale: [f32; 3],         // 12 bytes - non-uniform scale
    
    // Appearance (8 bytes)
    color: u32,              // RGBA packed
    material_id: u8,         // Index into material table
    transparency: u8,        // 0-255 (0 = opaque, 255 = invisible)
    reflectance: u8,         // 0-255
    _pad: u8,
    
    // Mesh reference (2 bytes, optional)
    mesh_id: u16,            // Index into mesh table (0 = primitive)
    
    // Primitive shape (2 bytes, if mesh_id == 0)
    shape: u8,               // 0=Block, 1=Ball, 2=Cylinder, 3=Wedge, 4=Cone
    _shape_pad: u8,
}

// Total: 56 bytes per instance
// 10M instances = 560 MB uncompressed
// With LZ4: ~100-200 MB typical
```

### Flag Bits

```rust
const FLAG_ANCHORED: u16      = 0x0001;
const FLAG_CAN_COLLIDE: u16   = 0x0002;
const FLAG_CAN_TOUCH: u16     = 0x0004;
const FLAG_CAN_QUERY: u16     = 0x0008;
const FLAG_CAST_SHADOW: u16   = 0x0010;
const FLAG_LOCKED: u16        = 0x0020;
const FLAG_ARCHIVABLE: u16    = 0x0040;
const FLAG_HAS_MESH: u16      = 0x0080;  // mesh_id is valid
const FLAG_HAS_CUSTOM_PHYSICS: u16 = 0x0100;  // Extended data follows
```

### Extended Data (Optional)

For instances with custom physics or attributes, an extended data section follows:

```rust
struct ExtendedData {
    instance_index: u32,     // Which instance this extends
    data_type: u8,           // 0=Physics, 1=Attributes, 2=Tags
    data_length: u16,
    data: [u8; data_length],
}
```

---

## Manifest Format

### manifest.toml

```toml
# Workspace/World/manifest.toml

[manifest]
version = 1
total_instances = 8_547_231
total_chunks = 2048
last_rebuild = "2026-03-02T10:00:00Z"

# Statistics
bounds_min = [-16384.0, -1024.0, -16384.0]
bounds_max = [16384.0, 1024.0, 16384.0]

# Chunk grid info
chunk_size = [256.0, 256.0, 256.0]

[chunks]
# Sparse listing: only non-empty chunks
# Format: "x_y_z" = { instances, size_bytes, modified, hash }

"0_0_0" = { instances = 12847, size = 719432, modified = "2026-03-02T10:00:00Z", hash = "a1b2c3d4" }
"0_0_1" = { instances = 8234, size = 461104, modified = "2026-03-02T10:00:00Z", hash = "e5f6g7h8" }
"0_1_0" = { instances = 15632, size = 875392, modified = "2026-03-02T10:00:00Z", hash = "i9j0k1l2" }
"1_0_0" = { instances = 9821, size = 549976, modified = "2026-03-02T10:00:00Z", hash = "m3n4o5p6" }
# ... thousands more entries

[lod]
# LOD chunk info (optional)
lod1_chunks = 512
lod2_chunks = 128
lod3_chunks = 32
```

### Chunk Coordinate System

```
        +Y (up)
         │
         │    +Z (north)
         │   /
         │  /
         │ /
         │/_________ +X (east)
        O

Chunk (0, 0, 0) contains world positions:
  X: [0, 256)
  Y: [0, 256)
  Z: [0, 256)

Chunk (-1, 0, 0) contains:
  X: [-256, 0)
  Y: [0, 256)
  Z: [0, 256)
```

---

## Streaming System

### Load/Unload Algorithm

```rust
fn update_chunk_streaming(
    camera: &Transform,
    world: &ChunkedWorld,
    loaded_chunks: &mut HashSet<ChunkCoord>,
    chunk_entities: &mut HashMap<ChunkCoord, Vec<Entity>>,
) {
    let camera_chunk = world_to_chunk(camera.translation, world.chunk_size);
    
    // Determine which chunks should be loaded
    let mut should_load = HashSet::new();
    for dx in -world.load_radius..=world.load_radius {
        for dy in -world.load_radius..=world.load_radius {
            for dz in -world.load_radius..=world.load_radius {
                let coord = ChunkCoord {
                    x: camera_chunk.x + dx,
                    y: camera_chunk.y + dy,
                    z: camera_chunk.z + dz,
                };
                if world.chunk_exists(&coord) {
                    should_load.insert(coord);
                }
            }
        }
    }
    
    // Unload chunks outside unload_radius
    let to_unload: Vec<_> = loaded_chunks
        .iter()
        .filter(|c| c.distance_to(&camera_chunk) > world.unload_radius)
        .cloned()
        .collect();
    
    for coord in to_unload {
        unload_chunk(&coord, chunk_entities);
        loaded_chunks.remove(&coord);
    }
    
    // Load new chunks (async, prioritized by distance)
    let to_load: Vec<_> = should_load
        .difference(&loaded_chunks)
        .cloned()
        .collect();
    
    // Sort by distance (closest first)
    let mut to_load = to_load;
    to_load.sort_by_key(|c| c.distance_squared_to(&camera_chunk));
    
    // Queue async loads
    for coord in to_load.into_iter().take(MAX_LOADS_PER_FRAME) {
        spawn_chunk_load_task(coord, world);
    }
}
```

### Async Chunk Loading

```rust
async fn load_chunk(
    coord: ChunkCoord,
    world: &ChunkedWorld,
) -> Result<ChunkData, ChunkError> {
    let path = world.chunk_path(&coord);
    
    // Read file (async I/O)
    let bytes = tokio::fs::read(&path).await?;
    
    // Decompress if needed
    let bytes = match world.compression {
        Compression::None => bytes,
        Compression::Lz4 => lz4_flex::decompress_size_prepended(&bytes)?,
        Compression::Zstd => zstd::decode_all(&bytes[..])?,
    };
    
    // Parse header
    let header = ChunkHeader::from_bytes(&bytes[0..64])?;
    
    // Parse tables
    let (class_table, offset) = parse_class_table(&bytes[64..])?;
    let (material_table, offset) = parse_material_table(&bytes[offset..])?;
    let (mesh_table, offset) = parse_mesh_table(&bytes[offset..])?;
    
    // Parse instances (zero-copy where possible)
    let instances = parse_instances(&bytes[offset..], header.instance_count)?;
    
    Ok(ChunkData {
        header,
        class_table,
        material_table,
        mesh_table,
        instances,
    })
}
```

### Spawning Entities from Chunk

```rust
fn spawn_chunk_entities(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    chunk: &ChunkData,
) -> Vec<Entity> {
    let mut entities = Vec::with_capacity(chunk.instances.len());
    
    for instance in &chunk.instances {
        let class_name = &chunk.class_table[instance.class_id as usize];
        
        let entity = commands.spawn((
            // Transform
            Transform {
                translation: Vec3::from(instance.position),
                rotation: Quat::from_array(instance.rotation),
                scale: Vec3::from(instance.scale),
            },
            
            // Visibility
            Visibility::default(),
            
            // Mesh
            if instance.flags & FLAG_HAS_MESH != 0 {
                let mesh_path = &chunk.mesh_table[instance.mesh_id as usize];
                // Load mesh asset (cached)
                MeshHandle::from_path(mesh_path)
            } else {
                // Primitive shape
                match instance.shape {
                    0 => MeshHandle::cube(),
                    1 => MeshHandle::sphere(),
                    2 => MeshHandle::cylinder(),
                    3 => MeshHandle::wedge(),
                    _ => MeshHandle::cube(),
                }
            },
            
            // Material
            StandardMaterial {
                base_color: Color::rgba_u8(
                    (instance.color >> 24) as u8,
                    (instance.color >> 16) as u8,
                    (instance.color >> 8) as u8,
                    instance.color as u8,
                ),
                ..default()
            },
            
            // Physics (if collidable)
            if instance.flags & FLAG_CAN_COLLIDE != 0 {
                Some(Collider::from_shape(instance.shape, instance.scale))
            } else {
                None
            },
            
            // Marker
            ChunkedInstance {
                chunk_coord: chunk.header.coords,
                local_index: entities.len() as u32,
            },
        )).id();
        
        entities.push(entity);
    }
    
    entities
}
```

---

## Integration with EEP

### EEP Spec Additions

The EEP specification should reference this document for chunked storage. Add to the File Naming Conventions:

```markdown
| ChunkedWorld | `_instance.toml` + `manifest.toml` + `chunks/*.echk` | Large-scale terrain |
```

### TOML ↔ Chunk Conversion

Instances can be converted between TOML and chunked storage:

```rust
/// Convert TOML instances to chunked storage
fn toml_to_chunk(
    toml_folder: &Path,
    output_chunk: &Path,
    chunk_coord: ChunkCoord,
) -> Result<()> {
    let mut instances = Vec::new();
    
    for entry in fs::read_dir(toml_folder)? {
        let path = entry?.path();
        if path.extension() == Some("toml".as_ref()) {
            let toml = fs::read_to_string(&path)?;
            let data: PartData = toml::from_str(&toml)?;
            instances.push(PackedInstance::from_toml(&data));
        }
    }
    
    write_chunk(output_chunk, chunk_coord, &instances)?;
    Ok(())
}

/// Extract a single instance from chunk to TOML (for editing)
fn chunk_to_toml(
    chunk_path: &Path,
    instance_index: u32,
    output_toml: &Path,
) -> Result<()> {
    let chunk = load_chunk_sync(chunk_path)?;
    let instance = &chunk.instances[instance_index as usize];
    let toml_data = instance.to_toml(&chunk.class_table, &chunk.material_table);
    fs::write(output_toml, toml::to_string_pretty(&toml_data)?)?;
    Ok(())
}
```

### When to Use Chunked vs TOML

| Content Type | Storage | Reason |
|--------------|---------|--------|
| Player character | TOML | Human-editable, few instances |
| NPC templates | TOML | Human-editable, reused |
| Hand-placed props | TOML | Designer-placed, < 1000 |
| Terrain geometry | Chunked | Millions of instances |
| Procedural forests | Chunked | Generated, not hand-edited |
| Building interiors | TOML or Chunked | Depends on scale |
| UI (ScreenGui) | TOML | Always human-editable |
| Scripts | TOML | Always human-editable |

---

## Studio Integration

### Chunk Visualization

Studio shows chunk boundaries in the viewport:

```
┌─────────────────────────────────────────────────────────────┐
│                        3D Viewport                           │
│                                                              │
│    ┌─────────┬─────────┬─────────┐                          │
│    │ Chunk   │ Chunk   │ Chunk   │                          │
│    │ -1,0,0  │  0,0,0  │  1,0,0  │                          │
│    │ (gray)  │ (green) │ (gray)  │                          │
│    └─────────┴─────────┴─────────┘                          │
│                    ▲                                         │
│                 Camera                                       │
│                                                              │
│    Legend:                                                   │
│    ■ Green = Loaded    ■ Gray = Unloaded    ■ Red = Loading │
└─────────────────────────────────────────────────────────────┘
```

### Chunk Editor Panel

```slint
// chunk_editor.slint
export component ChunkEditorPanel inherits Rectangle {
    in property <ChunkCoord> selected-chunk;
    in property <int> instance-count;
    in property <string> chunk-status;  // "loaded" | "unloaded" | "modified"
    
    callback on-rebuild-chunk(ChunkCoord);
    callback on-extract-to-toml(ChunkCoord, int);  // Extract instance to TOML
    callback on-merge-toml-to-chunk(string);       // Merge TOML folder into chunk
    
    // ... UI layout
}
```

### Editing Chunked Instances

1. **Select instance in viewport** → Shows properties in Properties panel
2. **Edit property** → Instance marked as "modified"
3. **Save** → Modified instances written back to chunk file
4. **Extract to TOML** → Converts instance to `.part.toml` for detailed editing
5. **Merge back** → Converts TOML back to chunk

### Chunk Rebuild Tool

For bulk operations (e.g., changing all tree colors):

```rust
/// Rebuild all chunks with a transformation
fn rebuild_chunks_with_transform<F>(
    world: &ChunkedWorld,
    transform: F,
) -> Result<()>
where
    F: Fn(&mut PackedInstance) -> bool,  // Returns true if modified
{
    for chunk_coord in world.all_chunk_coords() {
        let mut chunk = load_chunk_sync(&world.chunk_path(&chunk_coord))?;
        let mut modified = false;
        
        for instance in &mut chunk.instances {
            if transform(instance) {
                modified = true;
            }
        }
        
        if modified {
            write_chunk(&world.chunk_path(&chunk_coord), chunk_coord, &chunk.instances)?;
        }
    }
    
    Ok(())
}

// Example: Change all "Grass" material to "DriedGrass"
rebuild_chunks_with_transform(&world, |instance| {
    if instance.material_id == GRASS_ID {
        instance.material_id = DRIED_GRASS_ID;
        true
    } else {
        false
    }
});
```

---

## Performance Targets

### Load Times

| Metric | Target | Notes |
|--------|--------|-------|
| Chunk parse (10K instances) | < 5ms | Binary parsing, no allocation |
| Chunk decompress (LZ4) | < 2ms | LZ4 is extremely fast |
| Entity spawn (10K instances) | < 50ms | Batched spawning |
| Initial world load (100 chunks) | < 2s | Async parallel loading |

### Memory

| Metric | Target | Notes |
|--------|--------|-------|
| Loaded chunk overhead | ~1 KB/chunk | Metadata only |
| Instance memory | ~200 bytes/instance | ECS components |
| 1M loaded instances | ~200 MB | Acceptable for modern systems |
| 10M loaded instances | ~2 GB | Requires LOD/streaming |

### Disk

| Metric | Value | Notes |
|--------|-------|-------|
| Instance size (packed) | 56 bytes | Uncompressed |
| Instance size (LZ4) | ~15-25 bytes | Typical compression |
| 10M instances | ~150-250 MB | Compressed on disk |
| Chunk file size | ~500 KB - 2 MB | 10K-50K instances per chunk |

---

## Implementation Checklist

### Phase 1: Core Format

- [ ] Define `PackedInstance` struct in Rust
- [ ] Implement chunk header parsing
- [ ] Implement chunk writing
- [ ] Add LZ4 compression support
- [ ] Add Zstd compression support
- [ ] Unit tests for round-trip (write → read)

### Phase 2: Manifest & ChunkedWorld

- [ ] Define `ChunkedWorld` class in `classes.rs`
- [ ] Implement `manifest.toml` parsing
- [ ] Implement chunk coordinate system
- [ ] Add `_instance.toml` schema for ChunkedWorld

### Phase 3: Streaming

- [ ] Implement camera-based chunk loading
- [ ] Implement async chunk loading with Tokio
- [ ] Implement chunk unloading
- [ ] Add load priority queue (distance-based)
- [ ] Add hysteresis to prevent thrashing

### Phase 4: Entity Spawning

- [ ] Implement `spawn_chunk_entities()`
- [ ] Batch entity spawning for performance
- [ ] Handle mesh asset loading
- [ ] Handle material creation
- [ ] Add `ChunkedInstance` marker component

### Phase 5: Studio Integration

- [ ] Add chunk visualization overlay
- [ ] Add Chunk Editor panel
- [ ] Implement "Extract to TOML" command
- [ ] Implement "Merge TOML to Chunk" command
- [ ] Add chunk rebuild tool

### Phase 6: Conversion Tools

- [ ] `toml_to_chunk` CLI command
- [ ] `chunk_to_toml` CLI command
- [ ] Batch conversion for existing projects
- [ ] Validation and error reporting

### Phase 7: LOD System (Future)

- [ ] Define LOD chunk format
- [ ] Implement LOD generation
- [ ] Implement LOD streaming
- [ ] Integrate with rendering LOD system

---

## Changelog

### v1.0 (2026-03-02)

- Initial specification
- Defined hybrid TOML + chunked architecture
- Binary chunk format (.echk)
- Manifest format
- Streaming system design
- Studio integration design
- Performance targets

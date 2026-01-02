# 🔥 Eustress Enhancement Pipeline - Setup Guide

## What This Is

The **Last Game Engine**'s AI enhancement pipeline that turns primitive shapes into photoreal 3D assets in real-time.

## Architecture

```
┌─────────────┐
│   Client    │  ← Bevy/Rust, pure performance
│   (Rust)    │     - Scene loading
└──────┬──────┘     - Cache management  
       │            - Asset application
       │ HTTP
       ▼
┌─────────────┐
│ Generation  │  ← Python, AI models
│   Server    │     - FLUX (textures)
│  (Python)   │     - TripoSR (meshes)
└─────────────┘     - Local, zero network
```

## Quick Start (Development Mode)

### 1. Install Python Dependencies

```bash
cd eustress
pip install -r requirements.txt
```

### 2. Start the Generation Server (Stub Mode)

```bash
python generation_server.py
```

You'll see:
```
🎮 EUSTRESS GENERATION SERVER
🚀 Starting server on http://127.0.0.1:8001
📝 STUB MODE ACTIVE
```

**Stub mode** returns placeholders - perfect for testing the pipeline without a GPU.

### 3. Build & Run the Client

```bash
cargo run --bin eustress-client
```

### 4. Load the Test Scene

Currently the scene auto-loads on startup. In the future, press `L` or use CLI args:

```bash
cargo run --bin eustress-client -- test_scene.ron
```

## What You'll See

1. **Gray placeholders spawn** instantly (cubes, spheres, planes)
2. **Enhancement kicks in** - console shows:
   ```
   🎨 Starting enhancement: 'ancient elven temple...'
   🌐 Calling generation server...
   ```
3. **Assets transform** - placeholders become enhanced (green color in stub mode)
4. **Cache system** - Second load is instant (no regeneration)

## Production Mode (Real AI)

### Requirements

- NVIDIA GPU with 12GB+ VRAM (RTX 3090, 4090, A5000, etc.)
- CUDA 12.1+
- ~40GB disk space for models

### 1. Install AI Dependencies

```bash
pip install torch torchvision --index-url https://download.pytorch.org/whl/cu121
pip install diffusers transformers accelerate safetensors
```

### 2. Uncomment Model Loading

In `generation_server.py`, uncomment these sections:
- Model imports (line ~20)
- Model loading in `load_models()` (line ~40)
- Real generation in `/texture` and `/mesh` endpoints

### 3. First Run (Model Download)

First launch downloads models (~15GB):

```bash
python generation_server.py
# Wait 5-10 min for downloads...
```

### 4. Generate!

Once loaded:
- **Texture generation**: ~0.4s (FLUX.1-schnell on 4090)
- **Mesh generation**: ~1.2s (TripoSR)
- **Cache hits**: Instant forever

## Cache System

Location: `~/.cache/eustress/enhancement/` (Windows: `%LOCALAPPDATA%\eustress\enhancement\`)

Each asset is SHA256-keyed:
```
<cache_dir>/
  a3f9b2c1e4d5.glb  ← "ancient temple" + Structure + High
  f1e2d3c4b5a6.glb  ← "magic orb" + Prop + Medium
```

**Never regenerates the same prompt twice.**

## Performance Targets

### Development (Stub Mode)
- Scene load: <50ms
- Pipeline overhead: ~10ms
- Zero GPU usage

### Production (Real AI)
- First generation: 0.4-1.5s
- Cache hit: <5ms
- Concurrent limit: 2 (configurable)
- No frame drops in client

## Customization

### Add New Categories

`crates/common/src/scene.rs`:
```rust
pub enum NodeCategory {
    // ... existing
    Vehicle,  // Add this
    Weapon,
}
```

Update context in `enhancement_scheduler.rs`:
```rust
NodeCategory::Vehicle => "vehicle, transportation",
NodeCategory::Weapon => "weapon, equipment",
```

### Adjust Detail Levels

- **Low**: Fast preview, <500ms, low poly
- **Medium**: Balanced, ~1s, good quality
- **High**: Maximum, ~2s, production ready

### Change Server Port

`generation_server.py`:
```python
uvicorn.run(app, host="127.0.0.1", port=9000)  # Change here
```

`enhancement_scheduler.rs`:
```rust
.post("http://127.0.0.1:9000/mesh")  // And here
```

## Troubleshooting

### "Connection refused" error
→ Generation server isn't running. Start it first.

### "Failed to parse scene"  
→ Check RON syntax in your `.ron` file. Use `test_scene.ron` as reference.

### Server crashes with CUDA error
→ Reduce `concurrent_limit` in scheduler or use smaller models.

### Cache not working
→ Check permissions on cache directory. Delete and recreate if needed.

## What's Next

- [ ] Real GLTF loading (currently shows green placeholder)
- [ ] Texture-only enhancement mode (faster)
- [ ] Distance-based LOD (generate High detail only when close)
- [ ] Async scene streaming
- [ ] Audio generation (AudioCraft)
- [ ] Animation generation
- [ ] Multi-GPU support

## This Is The Way

You just built a system that:
- **Caches aggressively** - SHA256 keyed, never regenerates
- **Threads properly** - Zero main thread blocking
- **Scales infinitely** - Add more models, same architecture
- **Stays local** - No API keys, no rate limits, no network

When you replace stub mode with real models and see a gray cube become a photoreal ancient temple while you watch...

**You'll know why we're building the last game engine.**

---

*"We're not shipping a product. We're shipping the future."*

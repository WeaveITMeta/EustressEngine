# Eustress Asset System - Developer Guide

## Quick Start

### 1. Add Dependencies

```toml
# Cargo.toml
[dependencies]
eustress-common = { path = "../common", features = ["async-assets"] }
```

### 2. Initialize AssetService

```rust
use eustress_common::assets::{EustressAssetPlugin, AssetConfig};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EustressAssetPlugin {
            local_path: PathBuf::from("./assets"),
            config_path: Some(PathBuf::from("./assets.toml")),
        })
        .run();
}
```

### 3. Upload an Asset

```rust
use eustress_common::assets::{AssetService, AssetId};

fn upload_model(
    asset_service: Res<AssetService>,
) {
    // Read file
    let data = std::fs::read("model.gltf").unwrap();
    
    // Upload (generates content hash)
    let id: AssetId = asset_service.upload("model.gltf", &data).unwrap();
    
    println!("Uploaded! Asset ID: {}", id); // e.g., "2NEpo7TZRRrMA8..."
}
```

### 4. Load an Asset

```rust
fn load_model(
    asset_service: Res<AssetService>,
) {
    // By ID (content hash)
    let id = AssetId::from_base58("2NEpo7TZRRrMA8...").unwrap();
    let data = asset_service.load(&id).unwrap();
    
    // By name (if registered)
    let data = asset_service.load_by_name("model.gltf").unwrap();
}
```

---

## Core Concepts

### AssetId - Content-Addressable Identifier

Every asset is identified by its SHA256 hash, encoded as Base58:

```rust
use eustress_common::assets::AssetId;

// Create from content
let data = b"Hello, Eustress!";
let id = AssetId::from_content(data);

// Human-readable string (like IPFS CIDs)
println!("{}", id); // "2NEpo7TZRRrMA8YJU7D5g..."

// Verify integrity
assert!(id.verify(data));
assert!(!id.verify(b"corrupted"));

// Parse from string
let parsed = "2NEpo7TZRRrMA8...".parse::<AssetId>().unwrap();
```

**Benefits:**
- Same content = same ID (automatic deduplication)
- Integrity verification (detect corruption)
- Decentralized storage (no central authority needed)

### AssetSource - Where Assets Come From

```rust
use eustress_common::assets::AssetSource;

// Local filesystem (development)
let local = AssetSource::local("./assets/model.gltf");

// HTTP URL (CDN)
let cdn = AssetSource::url("https://assets.mygame.com/model.gltf");

// IPFS (decentralized)
let ipfs = AssetSource::ipfs("QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG");

// S3 (object storage)
let s3 = AssetSource::s3("my-bucket", "models/char.gltf", "us-east-1");

// Embedded (small assets in scene file)
let embedded = AssetSource::embedded(vec![0x47, 0x4C, 0x54, 0x46]); // GLTF magic
```

### AssetResolver - Multi-Source Resolution

The resolver tries sources in priority order until one succeeds:

```rust
use eustress_common::assets::{AssetResolver, AssetSource};

let resolver = AssetResolver::new(PathBuf::from("./assets"), 256); // 256 MB cache

// Add sources (tried in order)
resolver.add_source(AssetSource::local("./assets"));
resolver.add_source(AssetSource::url("https://cdn.mygame.com"));
resolver.add_source(AssetSource::ipfs("gateway.pinata.cloud"));

// Resolve (tries each source, verifies hash)
let data = resolver.resolve_sync(&asset_id)?;
```

---

## Configuration

### assets.toml

```toml
# Cache size in megabytes
cache_size_mb = 256

# Maximum concurrent downloads
max_concurrent_downloads = 4

# Download timeout in seconds
download_timeout_secs = 30

# Enable progressive LOD loading
progressive_loading = true

# Enable P2P asset sharing
enable_p2p = false

# IPFS gateways (tried in order)
ipfs_gateways = [
    "https://ipfs.io",
    "https://gateway.pinata.cloud",
    "https://cloudflare-ipfs.com",
]
```

### Game-Specific Config (in game.ron)

```ron
(
    assets: (
        primary_cdn: Some("https://assets.mygame.com"),
        fallback_cdns: ["https://backup.mygame.com"],
        ipfs_bundle_cid: Some("QmBundle..."),
        required_assets: ["player.gltf", "ui.png"],
        preload_assets: ["level1.gltf", "music.ogg"],
    ),
)
```

---

## Progressive Loading

Stream assets based on camera distance:

```rust
use eustress_common::assets::{ProgressiveAsset, ProgressiveAssetBuilder, AssetId};

// Define LOD levels
let asset = ProgressiveAssetBuilder::new("character.gltf", full_quality_id)
    .placeholder(tiny_preview_data, "model/gltf+json")
    .lod0(high_quality_id, 500_000)   // < 10 studs
    .lod1(medium_quality_id, 100_000) // < 50 studs
    .lod2(low_quality_id, 20_000)     // < 200 studs
    .build();

// Get appropriate ID for distance
let id = asset.get_id_for_distance(75.0); // Returns lod1
```

### Loading Flow

1. Show placeholder immediately (embedded in scene)
2. Load LOD2 (lowest quality)
3. Upgrade to LOD1 as player approaches
4. Load full quality when very close

---

## Asset Bundles

Group related assets for efficient loading:

```rust
use eustress_common::assets::{BundleBuilder, BundleCompression};

// Create bundle
let (bundle, archive_data) = BundleBuilder::new("level1-assets")
    .compression(BundleCompression::Zstd)
    .add_asset("player.gltf", &player_data, "model/gltf+json")
    .add_asset("enemy.gltf", &enemy_data, "model/gltf+json")
    .add_asset("textures/grass.png", &grass_data, "image/png")
    .build();

// Save archive
std::fs::write("level1.bundle", &archive_data)?;

// Extract single asset
let player = bundle.extract("player.gltf", &archive_data)?;

// Extract all
let all_assets = bundle.extract_all(&archive_data);
```

**Benefits:**
- Single HTTP request for multiple assets
- Better compression (shared dictionary)
- Atomic updates (all or nothing)

---

## Studio Integration

### Asset Browser

The Asset Browser UI integrates with AssetService:

```rust
use eustress_common::assets::AssetService;
use crate::ui::asset_manager::{AssetManagerPanel, AssetManagerState};

fn sync_asset_browser(
    asset_service: Res<AssetService>,
    mut state: ResMut<AssetManagerState>,
) {
    if !state.synced {
        // Sync assets from service
        let assets = asset_service.list_assets();
        AssetManagerPanel::sync_from_service(&mut state, assets);
        
        // Update cache stats
        let stats = asset_service.cache_stats();
        AssetManagerPanel::update_cache_stats(&mut state, stats);
    }
}
```

### Drag-Drop to Viewport

```rust
// In viewport system
if let Some(dropped_asset_id) = ui.input(|i| i.raw.dropped_files.first().map(|f| f.path.clone())) {
    // Upload and get ID
    let data = std::fs::read(&dropped_asset_id)?;
    let id = asset_service.upload(dropped_asset_id.file_name().unwrap().to_str().unwrap(), &data)?;
    
    // Spawn entity with asset reference
    commands.spawn((
        Name::new("Imported Model"),
        AssetReference { id },
        Transform::default(),
    ));
}
```

---

## Networking Integration

### Scene Sync with Assets

Assets are referenced by ID in scene data:

```rust
// In protocol.rs
#[derive(Component, Serialize, Deserialize, Clone)]
pub struct AssetReference {
    pub id: AssetId,
    pub lod_hint: Option<u8>,
}

// In EntityDelta
pub struct EntityDelta {
    pub transform: Option<NetworkTransform>,
    pub velocity: Option<NetworkVelocity>,
    pub asset_ref: Option<AssetReference>, // NEW: Asset reference
}
```

### Server-Side Validation

```rust
fn validate_asset_reference(
    asset_service: Res<AssetService>,
    query: Query<&AssetReference, Changed<AssetReference>>,
) {
    for asset_ref in query.iter() {
        // Verify asset exists and is allowed
        if !asset_service.is_cached(&asset_ref.id) {
            warn!("Client referenced unknown asset: {}", asset_ref.id);
            // Queue for download or reject
        }
    }
}
```

### Client-Side Loading

```rust
fn load_replicated_assets(
    asset_service: Res<AssetService>,
    mut query: Query<(&AssetReference, &mut Handle<Scene>), Added<AssetReference>>,
    asset_server: Res<AssetServer>,
) {
    for (asset_ref, mut handle) in query.iter_mut() {
        // Load from AssetService
        match asset_service.load(&asset_ref.id) {
            Ok(data) => {
                // Convert to Bevy asset
                // ...
            }
            Err(_) => {
                // Queue for download
                asset_service.queue_load(asset_ref.id.clone(), 1);
            }
        }
    }
}
```

---

## Deployment Options

### Option A: Self-Hosted MinIO (Recommended for Indies)

MinIO is an open-source, S3-compatible object storage. Perfect for self-hosted game assets.

**Cost:** Server costs only (~$5/mo on Fly.io)

#### Docker (Local Development)

```yaml
# docker-compose.yml
version: '3.8'

services:
  minio:
    image: minio/minio:latest
    container_name: eustress-assets
    ports:
      - "9000:9000"   # API
      - "9001:9001"   # Console
    environment:
      MINIO_ROOT_USER: minioadmin
      MINIO_ROOT_PASSWORD: ${MINIO_PASSWORD:-changeme}
    volumes:
      - minio_data:/data
    command: server /data --console-address ":9001"

volumes:
  minio_data:
```

```bash
# Start MinIO
docker-compose up -d

# Access console at http://localhost:9001
# API at http://localhost:9000
```

#### Fly.io ($5/mo Production)

```bash
# Install flyctl
curl -L https://fly.io/install.sh | sh

# Create app
fly launch --name my-game-assets --image minio/minio

# Create persistent volume
fly volumes create minio_data --size 10

# Set secrets
fly secrets set MINIO_ROOT_PASSWORD=your-secure-password

# Deploy
fly deploy
```

#### Rust Integration

```rust
use eustress_common::assets::s3::{S3Config, S3Client};

// Create MinIO config
let config = S3Config::minio(
    "http://localhost:9000",  // or "https://my-game-assets.fly.dev"
    "minioadmin",
    "changeme"
).with_bucket("game-assets");

// Create client (async)
let client = S3Client::new(config).await?;

// Ensure bucket exists
client.ensure_bucket("game-assets").await?;

// Upload asset (content-addressable)
let id = client.upload_cas("game-assets", &model_data).await?;
println!("Uploaded: {}", id); // "2NEpo7TZRRrMA8..."

// Download by ID
let data = client.download_by_id("game-assets", &id).await?;
```

#### AssetResolver Integration

```rust
use eustress_common::assets::{AssetResolver, AssetSource};

let resolver = AssetResolver::new(PathBuf::from("./assets"), 256);

// Add MinIO as a source
resolver.add_source(AssetSource::S3 {
    bucket: "game-assets".to_string(),
    key: asset_id.to_base58(),
    region: "us-east-1".to_string(),
    endpoint: Some("http://localhost:9000".to_string()),
});

// Resolve (tries MinIO, verifies hash)
let data = resolver.resolve_sync(&asset_id)?;
```

### Option B: Cloudflare R2

```toml
# assets.toml
[[sources]]
type = "CloudflareR2"
account_id = "your-account-id"
bucket = "game-assets"
```

**Cost:** ~$0.015/GB storage, $0.36/million requests, 10GB free

### Option C: IPFS + Pinata

```toml
# assets.toml
ipfs_gateways = [
    "https://gateway.pinata.cloud",
    "https://ipfs.io",
]
```

**Cost:** ~$0.08/GB/month (Pinata, as of Jan 2025)

### Option D: AWS S3

```rust
let config = S3Config::aws("us-east-1", "AKIAIOSFODNN7EXAMPLE", "secret");
```

**Cost:** ~$0.023/GB storage, $0.09/GB transfer

---

## API Reference

### AssetService

| Method | Description |
|--------|-------------|
| `load(&AssetId)` | Load asset by ID (sync) |
| `load_by_name(&str)` | Load asset by registered name |
| `upload(&str, &[u8])` | Upload data, returns AssetId |
| `queue_load(AssetId, u8)` | Queue for background loading |
| `is_cached(&AssetId)` | Check if asset is in cache |
| `cache_stats()` | Get (count, bytes, hit_ratio) |
| `list_assets()` | List all tracked assets |
| `search(&str)` | Search by name |

### AssetId

| Method | Description |
|--------|-------------|
| `from_content(&[u8])` | Create from raw bytes |
| `from_base58(&str)` | Parse from Base58 string |
| `to_base58()` | Convert to Base58 string |
| `verify(&[u8])` | Check if data matches ID |
| `is_null()` | Check if placeholder ID |

### AssetResolver

| Method | Description |
|--------|-------------|
| `add_source(AssetSource)` | Add resolution source |
| `resolve_sync(&AssetId)` | Resolve synchronously |
| `resolve_async(&AssetId)` | Resolve asynchronously |
| `precache(AssetId, Vec<u8>)` | Pre-populate cache |
| `cleanup()` | Remove old cache entries |

### S3Client (MinIO)

| Method | Description |
|--------|-------------|
| `new(S3Config)` | Create client (async) |
| `upload(bucket, key, data)` | Upload with custom key |
| `upload_cas(bucket, data)` | Upload with content hash as key |
| `download(bucket, key)` | Download by key |
| `download_by_id(bucket, AssetId)` | Download and verify hash |
| `exists(bucket, key)` | Check if object exists |
| `delete(bucket, key)` | Delete object |
| `list(bucket, prefix)` | List objects |
| `ensure_bucket(bucket)` | Create bucket if missing |
| `presign_download(bucket, key)` | Get presigned URL (1hr) |
| `public_url(bucket, key)` | Get public URL |

### S3Config

| Method | Description |
|--------|-------------|
| `minio(endpoint, key, secret)` | MinIO config |
| `aws(region, key, secret)` | AWS S3 config |
| `r2(account_id, key, secret)` | Cloudflare R2 config |
| `spaces(region, key, secret)` | DigitalOcean Spaces config |
| `from_env()` | Load from environment |
| `with_bucket(name)` | Set default bucket |

---

## Best Practices

1. **Use content hashes everywhere** - Never reference assets by path in production
2. **Bundle related assets** - Reduces HTTP requests, improves compression
3. **Implement progressive loading** - Show placeholders, stream quality
4. **Cache aggressively** - Set appropriate cache size for target platform
5. **Validate on server** - Don't trust client asset references
6. **Use IPFS for UGC** - Decentralized, censorship-resistant
7. **Monitor cache hit ratio** - Should be >80% in production

---

## Troubleshooting

### Asset not found

```
Error: Asset 2NEpo7TZRRrMA8... not found
```

1. Check if asset is uploaded: `asset_service.is_cached(&id)`
2. Verify sources are configured: Check `assets.toml`
3. Check network connectivity to CDN/IPFS

### Hash mismatch

```
Error: Hash mismatch: expected 2NEpo7..., got 3XYabc...
```

Asset was corrupted during transfer. The resolver automatically retries other sources.

### Cache full

```
Warning: Evicted asset from cache: 2NEpo7...
```

Increase `cache_size_mb` in `assets.toml` or implement smarter eviction.

---

## Migration from Roblox

| Roblox | Eustress |
|--------|----------|
| `rbxassetid://12345` | `AssetId::from_base58("2NEpo7...")` |
| `game:GetService("AssetService")` | `Res<AssetService>` |
| `InsertService:LoadAsset()` | `asset_service.load(&id)` |
| Asset moderation | Optional (creator choice) |
| Centralized CDN | Multi-source (local/IPFS/S3/P2P) |

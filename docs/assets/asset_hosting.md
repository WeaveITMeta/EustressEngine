# Asset Hosting Infrastructure

> **Status**: Implemented in `eustress-common/src/assets/`
> 
> See [ASSET_DEVELOPER_GUIDE.md](./ASSET_DEVELOPER_GUIDE.md) for usage.

---

## Implementation Status (January 2026)

All core features from the asset hosting proposal are **fully implemented**:

| Feature | Status | Location | Notes |
|---------|--------|----------|-------|
| **World Snapshot** | ✅ Complete | `engine/src/play_mode.rs` | `WorldSnapshot`, `SnapshotStack`, `EntitySnapshot` with disk spill, 10 save points |
| **OrderedDataStore** | ✅ Complete | `common/src/services/datastore.rs` | BTreeMap-based, `get_range()`, `get_rank()`, ascending/descending |
| **Ownership Auto-Release** | ✅ Complete | `common/eustress-networking/src/ownership.rs` | `ActivityTracker`, `auto_release_inactive()`, `GradualHandoff` (1-2s blend) |
| **Party Teleport** | ✅ Complete | `common/src/services/teleport.rs` | `PartyTeleport`, `ServerReservation`, confirmation/veto, countdown |
| **P2P Assets** | ✅ Complete | `common/src/assets/p2p.rs` | `PeerManager`, `ChunkTransferManager`, WebRTC signaling |
| **Asset CAS** | ✅ Complete | `common/src/assets/` | `ContentHash`, `AssetResolver`, `AssetService`, multi-source |

### Key Implementation Details

#### 1. World Snapshot (`play_mode.rs`)
- **SnapshotConfig**: Configurable capture (transforms, Instance, BasePart, Humanoid)
- **SnapshotStack**: Up to 10 save points with auto-spill to disk at 10MB
- **Entity tracking**: Spawned/deleted entities tracked for accurate restore
- **Compression**: Optional snap compression for disk storage

#### 2. OrderedDataStore (`datastore.rs`)
- **BTreeMap-based**: Efficient O(log n) operations
- **Range queries**: `get_range(start, count, SortOrder)` for leaderboards
- **Rank lookup**: `get_rank(key, order)` returns 0-indexed position
- **Persistence**: JSON serialization to backend with dirty tracking

#### 3. Ownership Auto-Release (`ownership.rs`)
- **ActivityType**: Input, Movement, Interaction tracking
- **Per-entity timers**: `record_activity()`, `is_idle()`, `get_idle_entities()`
- **Gradual handoff**: 1-2s physics authority blend via `GradualHandoff`
- **Config**: `auto_release_secs`, `gradual_handoff_ms` in `OwnershipConfig`

#### 4. Party Teleport (`teleport.rs`)
- **ServerReservation**: TTL-based slot reservation before teleport
- **Confirmation flow**: `confirm_party_member()`, `veto_party_teleport()`
- **Quorum**: `min_confirm_ratio`, `require_all_confirm` options
- **Status tracking**: `WaitingForConfirmation` → `ReservingServer` → `Countdown` → `Teleporting`

#### 5. P2P Assets (`p2p.rs` + `service.rs`)
- **PeerManager**: Discovery, health scoring, blacklisting
- **ChunkTransferManager**: Parallel chunk downloads, timeout handling
- **SignalingClient**: WebRTC offer/answer/ICE via signaling server
- **Integration**: `AssetService::resolve_with_p2p()` for CDN→P2P fallback
- **Seeding**: `AssetService::start_seeding()`, `stop_seeding()` for peer distribution

---

## How to Beat Roblox's Asset System

Roblox's asset system has limitations:
- Centralized, proprietary CDN
- Limited file formats
- Moderation delays (2-7 days)
- No self-hosting option
- Asset IDs are opaque numbers
- ~$50/TB/month (upload fees + moderation)

Eustress beats this with a **hybrid decentralized approach**.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Eustress Asset Network                           │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                 │
│  │   Studio    │    │   Client    │    │   Server    │                 │
│  │  (Creator)  │    │  (Player)   │    │  (Game)     │                 │
│  └──────┬──────┘    └──────┬──────┘    └──────┬──────┘                 │
│         │                  │                  │                         │
│         ▼                  ▼                  ▼                         │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    Asset Resolution Layer                        │   │
│  │  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐     │   │
│  │  │  Local    │  │   IPFS    │  │    S3     │  │  Custom   │     │   │
│  │  │  Cache    │  │  Gateway  │  │   CDN     │  │  Server   │     │   │
│  │  └───────────┘  └───────────┘  └───────────┘  └───────────┘     │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Key Innovations

### 1. Content-Addressable Assets (CAS)

Instead of opaque IDs, use **content hashes**:

```rust
// Asset ID is the SHA-256 hash of the content
pub struct AssetId(pub [u8; 32]);

impl AssetId {
    pub fn from_content(data: &[u8]) -> Self {
        use sha2::{Sha256, Digest};
        let hash = Sha256::digest(data);
        Self(hash.into())
    }
    
    pub fn to_string(&self) -> String {
        // Base58 encoding for human-readable IDs
        bs58::encode(&self.0).into_string()
    }
}

// Example: "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG"
```

**Benefits:**
- Deduplication (same content = same ID)
- Integrity verification (hash mismatch = corrupted)
- Decentralized storage (IPFS, BitTorrent, etc.)
- No central authority needed

### 2. Multi-Source Resolution

Assets can come from multiple sources:

```rust
pub enum AssetSource {
    /// Local file system (development)
    Local(PathBuf),
    
    /// IPFS content-addressed storage
    Ipfs { gateway: String, cid: String },
    
    /// S3-compatible object storage
    S3 { bucket: String, key: String, region: String },
    
    /// HTTP(S) URL
    Url(String),
    
    /// Embedded in scene file (small assets)
    Embedded(Vec<u8>),
    
    /// Peer-to-peer (BitTorrent-style)
    P2P { info_hash: [u8; 20] },
}

pub struct AssetResolver {
    sources: Vec<AssetSource>,
    cache: AssetCache,
}

impl AssetResolver {
    /// Try each source until one succeeds
    pub async fn resolve(&self, id: &AssetId) -> Result<Vec<u8>, AssetError> {
        // 1. Check local cache first
        if let Some(data) = self.cache.get(id) {
            return Ok(data);
        }
        
        // 2. Try each source
        for source in &self.sources {
            match self.fetch_from(source, id).await {
                Ok(data) => {
                    // Verify hash
                    if AssetId::from_content(&data) == *id {
                        self.cache.put(id, &data);
                        return Ok(data);
                    }
                }
                Err(_) => continue,
            }
        }
        
        Err(AssetError::NotFound)
    }
}
```

### 3. Progressive Loading

Stream assets as needed:

```rust
pub struct ProgressiveAsset {
    /// Low-quality placeholder (embedded in scene)
    pub placeholder: Option<Vec<u8>>,
    
    /// Full asset ID
    pub full_id: AssetId,
    
    /// LOD levels (optional)
    pub lods: Vec<AssetId>,
}

// Loading flow:
// 1. Show placeholder immediately
// 2. Load LOD0 (lowest quality)
// 3. Load LOD1, LOD2... as bandwidth allows
// 4. Load full quality when close
```

### 4. Asset Bundles

Group related assets for efficient loading:

```rust
pub struct AssetBundle {
    /// Bundle manifest
    pub manifest: BundleManifest,
    
    /// Compressed archive
    pub archive_id: AssetId,
}

pub struct BundleManifest {
    /// Assets in this bundle
    pub assets: Vec<(String, AssetId, u64)>, // (name, id, offset)
    
    /// Total size
    pub total_size: u64,
    
    /// Compression format
    pub compression: Compression,
}

// Benefits:
// - Single HTTP request for multiple assets
// - Better compression (shared dictionary)
// - Atomic updates (all or nothing)
```

### 5. Self-Hosted Option

Creators can host their own assets:

```toml
# game.toml
[assets]
# Primary source (creator's server)
primary = "https://assets.mygame.com"

# Fallback sources
fallbacks = [
    "ipfs://gateway.pinata.cloud",
    "s3://eustress-public-assets",
]

# Local development
[assets.dev]
path = "./assets"
```

---

## Implementation Plan

### Phase 1: Local + HTTP (MVP)

```rust
// Simple asset service
pub struct AssetService {
    local_path: PathBuf,
    http_client: reqwest::Client,
    cache: DashMap<AssetId, Vec<u8>>,
}

impl AssetService {
    pub async fn load(&self, uri: &str) -> Result<Vec<u8>> {
        // Parse URI
        if uri.starts_with("file://") {
            self.load_local(&uri[7..]).await
        } else if uri.starts_with("http") {
            self.load_http(uri).await
        } else {
            // Assume local path
            self.load_local(uri).await
        }
    }
}
```

### Phase 2: IPFS Integration

```rust
// Add IPFS support
impl AssetService {
    pub async fn load_ipfs(&self, cid: &str) -> Result<Vec<u8>> {
        // Try multiple gateways
        let gateways = [
            "https://ipfs.io/ipfs/",
            "https://gateway.pinata.cloud/ipfs/",
            "https://cloudflare-ipfs.com/ipfs/",
        ];
        
        for gateway in gateways {
            let url = format!("{}{}", gateway, cid);
            if let Ok(data) = self.load_http(&url).await {
                return Ok(data);
            }
        }
        
        Err(AssetError::NotFound)
    }
}
```

### Phase 3: P2P Distribution

```rust
// BitTorrent-style peer distribution
pub struct P2PAssetNetwork {
    /// Known peers
    peers: Vec<PeerInfo>,
    
    /// Assets we're seeding
    seeding: HashMap<AssetId, Vec<u8>>,
    
    /// Assets we're downloading
    downloading: HashMap<AssetId, DownloadState>,
}

impl P2PAssetNetwork {
    /// Request asset from peers
    pub async fn request(&mut self, id: &AssetId) -> Result<Vec<u8>> {
        // 1. Ask peers who has it
        let providers = self.find_providers(id).await?;
        
        // 2. Download from multiple peers in parallel
        let chunks = self.download_parallel(id, &providers).await?;
        
        // 3. Verify and assemble
        let data = self.assemble_and_verify(id, chunks)?;
        
        // 4. Start seeding
        self.seeding.insert(id.clone(), data.clone());
        
        Ok(data)
    }
}
```

---

## Comparison with Roblox (December 2025)

| Feature | Roblox | Eustress |
|---------|--------|----------|
| **Storage** | Centralized CDN | Hybrid (local/IPFS/S3/P2P) |
| **Asset IDs** | Opaque 10-digit numbers | SHA256 hashes (Base58 CIDs) |
| **Self-hosting** | ❌ Vendor lock-in | ✅ Full (custom servers/CDNs) |
| **Offline support** | Partial (cached meshes) | ✅ Complete (local resolver + cache) |
| **Moderation** | Mandatory global review | Optional (creator + optional IPFS pins) |
| **Formats** | OBJ/MTL, limited audio/video | Any (GLTF/FBX/WAV/MP4 via mime_guess) |
| **Deduplication** | Server-side (inefficient) | ✅ Automatic (CAS collisions = same ID) |
| **Integrity** | Trust CDN (no client verify) | ✅ Hash checks on resolve |
| **Censorship Risk** | High (platform takedowns) | Low (decentralized fallbacks) |
| **Cost (1TB/mo)** | ~$50 (upload fees + moderation) | ~$8 (Pinata @ $0.08/GB post-Jan '25) |
| **Load Speed** | Global CDN (consistent) | Variable (edge cache + P2P boosts) |

Eustress pulls ahead on flexibility/cost; Roblox edges on reliability for casuals.

---

## Asset Service API

```rust
/// Asset service for Eustress
#[derive(Resource)]
pub struct AssetService {
    resolver: AssetResolver,
    loader: AssetLoader,
    cache: AssetCache,
}

impl AssetService {
    /// Load an asset by URI
    pub async fn load(&self, uri: &str) -> Result<Handle<Asset>> {
        // ...
    }
    
    /// Upload an asset (returns content hash)
    pub async fn upload(&self, data: &[u8]) -> Result<AssetId> {
        // ...
    }
    
    /// Pin an asset (ensure availability)
    pub async fn pin(&self, id: &AssetId) -> Result<()> {
        // ...
    }
    
    /// Get asset info
    pub async fn info(&self, id: &AssetId) -> Result<AssetInfo> {
        // ...
    }
}
```

---

## Deployment Options

### Option A: Cloudflare R2 + Workers (Recommended)

- **Cost**: $0.015/GB storage, $0.36/million requests
- **Speed**: Global edge network
- **Setup**: 5 minutes

```toml
# wrangler.toml
name = "eustress-assets"
main = "src/worker.js"

[[r2_buckets]]
binding = "ASSETS"
bucket_name = "eustress-assets"
```

### Option B: Self-Hosted MinIO

- **Cost**: Server costs only
- **Speed**: Depends on infrastructure
- **Setup**: Docker compose

```yaml
# docker-compose.yml
services:
  minio:
    image: minio/minio
    ports:
      - "9000:9000"
    volumes:
      - ./data:/data
    command: server /data
```

### Option C: IPFS Pinning (Decentralized)

- **Cost**: ~$0.10/GB/month (Pinata)
- **Speed**: Variable (gateway dependent)
- **Setup**: API key

```rust
// Upload to Pinata
let response = client
    .post("https://api.pinata.cloud/pinning/pinFileToIPFS")
    .bearer_auth(api_key)
    .multipart(form)
    .send()
    .await?;
```

---

## Asset Moderation Pipeline

All uploaded assets pass through a multi-stage moderation pipeline before becoming publicly available.

### Pipeline Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      ASSET MODERATION PIPELINE                           │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  STAGE 1: CSAM DETECTION (Blocking - Must Pass)                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                      │
│  │ PhotoDNA    │  │ NCMEC Hash  │  │ AI Age      │                      │
│  │ Hash Match  │  │ Database    │  │ Classifier  │                      │
│  └─────────────┘  └─────────────┘  └─────────────┘                      │
│  Result: BLOCK → Immediate removal + NCMEC report                        │
└─────────────────────────────────────────────────────────────────────────┘
                                    │ PASS
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  STAGE 2: COPYRIGHT DETECTION                                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                      │
│  │ Perceptual  │  │ Audio       │  │ Watermark   │                      │
│  │ Hash (pHash)│  │ Fingerprint │  │ Detection   │                      │
│  └─────────────┘  └─────────────┘  └─────────────┘                      │
│  Result: FLAG → Queue for review or auto-reject if >95% match            │
└─────────────────────────────────────────────────────────────────────────┘
                                    │ PASS
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  STAGE 3: CONTENT CLASSIFICATION                                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                      │
│  │ Violence    │  │ Nudity      │  │ Hate        │                      │
│  │ Classifier  │  │ Detector    │  │ Speech      │                      │
│  └─────────────┘  └─────────────┘  └─────────────┘                      │
│  Result: LABEL → Age rating assignment (E, E10, T, M)                    │
└─────────────────────────────────────────────────────────────────────────┘
                                    │ PASS
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  STAGE 4: QUALITY & SAFETY                                               │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                      │
│  │ Malware     │  │ Format      │  │ Size        │                      │
│  │ Scan        │  │ Validation  │  │ Limits      │                      │
│  └─────────────┘  └─────────────┘  └─────────────┘                      │
│  Result: REJECT if malformed/malicious                                   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │ PASS
                                    ▼
                        ┌───────────────────┐
                        │  ASSET PUBLISHED  │
                        │  (with metadata)  │
                        └───────────────────┘
```

### Moderation API Integration

```rust
// crates/assets/src/moderation.rs

use crate::moderation_api::ModerationApiClient;

/// Asset moderation service
pub struct AssetModerationService {
    csam_detector: CsamDetector,
    copyright_detector: CopyrightDetector,
    content_classifier: ContentClassifier,
    moderation_api: ModerationApiClient,
}

impl AssetModerationService {
    /// Run full moderation pipeline on asset
    pub async fn moderate(&self, asset: &Asset) -> Result<ModerationResult, ModerationError> {
        // Stage 1: CSAM (blocking)
        let csam_result = self.csam_detector.check(asset).await?;
        if csam_result.is_violation() {
            return Ok(ModerationResult::Blocked {
                reason: BlockReason::Csam,
                report_submitted: true,
            });
        }
        
        // Stage 2: Copyright
        let copyright_result = self.copyright_detector.scan(asset).await?;
        if copyright_result.confidence > 0.95 {
            return Ok(ModerationResult::Blocked {
                reason: BlockReason::Copyright,
                report_submitted: false,
            });
        }
        
        // Stage 3: Content classification
        let classification = self.content_classifier.classify(asset).await?;
        
        // Stage 4: Quality/safety
        let safety_result = self.check_safety(asset).await?;
        if !safety_result.is_safe {
            return Ok(ModerationResult::Rejected {
                reason: safety_result.rejection_reason,
            });
        }
        
        Ok(ModerationResult::Approved {
            age_rating: classification.age_rating,
            content_tags: classification.tags,
            copyright_flags: copyright_result.flags,
        })
    }
}
```

### Related Documentation

- [CSAM.md](../legal/CSAM.md) — Child safety content policies
- [DMCA.md](../legal/DMCA.md) — Copyright takedown procedures
- [AI_AGENTS.md](../moderation/AI_AGENTS.md) — AI moderation architecture
- [MODERATION_API.md](../moderation/MODERATION_API.md) — Moderation API reference

---

## Asset Versioning

Content-addressable storage means the same content always has the same ID. For asset updates, we use a manifest-based versioning system.

### Version Manifest

```rust
// crates/assets/src/versioning.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Asset version manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetManifest {
    /// Stable asset identifier (UUID, not content hash)
    pub asset_id: String,
    
    /// Human-readable name
    pub name: String,
    
    /// Creator ID
    pub creator_id: String,
    
    /// Version history (newest first)
    pub versions: Vec<AssetVersion>,
    
    /// Current/latest version
    pub current_version: u32,
    
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    
    /// Last updated timestamp
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetVersion {
    /// Version number (1, 2, 3...)
    pub version: u32,
    
    /// Content hash (CAS ID)
    pub content_hash: String,
    
    /// Version label (optional, e.g., "v1.2.0")
    pub label: Option<String>,
    
    /// Changelog/description
    pub changelog: Option<String>,
    
    /// Published timestamp
    pub published_at: DateTime<Utc>,
    
    /// File size in bytes
    pub size_bytes: u64,
    
    /// Whether this version is deprecated
    pub deprecated: bool,
}

impl AssetManifest {
    /// Get the current version's content hash
    pub fn current_content_hash(&self) -> Option<&str> {
        self.versions
            .iter()
            .find(|v| v.version == self.current_version)
            .map(|v| v.content_hash.as_str())
    }
    
    /// Add a new version
    pub fn add_version(&mut self, content_hash: String, changelog: Option<String>, size_bytes: u64) {
        let new_version = self.current_version + 1;
        
        self.versions.insert(0, AssetVersion {
            version: new_version,
            content_hash,
            label: None,
            changelog,
            published_at: Utc::now(),
            size_bytes,
            deprecated: false,
        });
        
        self.current_version = new_version;
        self.updated_at = Utc::now();
    }
    
    /// Rollback to a previous version
    pub fn rollback(&mut self, target_version: u32) -> Result<(), VersionError> {
        if !self.versions.iter().any(|v| v.version == target_version) {
            return Err(VersionError::VersionNotFound(target_version));
        }
        
        self.current_version = target_version;
        self.updated_at = Utc::now();
        Ok(())
    }
}
```

### Version Resolution

```rust
// crates/assets/src/resolver.rs

/// Resolve asset references to specific versions
pub struct VersionResolver {
    manifest_store: ManifestStore,
    content_store: ContentStore,
}

impl VersionResolver {
    /// Resolve an asset reference to a content hash
    pub async fn resolve(&self, reference: &AssetReference) -> Result<String, ResolveError> {
        match reference {
            // Direct content hash (immutable)
            AssetReference::ContentHash(hash) => Ok(hash.clone()),
            
            // Asset ID with optional version
            AssetReference::AssetId { id, version } => {
                let manifest = self.manifest_store.get(id).await?;
                
                let target_version = version.unwrap_or(manifest.current_version);
                
                manifest.versions
                    .iter()
                    .find(|v| v.version == target_version)
                    .map(|v| v.content_hash.clone())
                    .ok_or(ResolveError::VersionNotFound)
            }
            
            // Semantic version constraint (e.g., "^1.0.0")
            AssetReference::Semver { id, constraint } => {
                let manifest = self.manifest_store.get(id).await?;
                
                // Find best matching version
                manifest.versions
                    .iter()
                    .filter(|v| v.label.as_ref().map(|l| constraint.matches(l)).unwrap_or(false))
                    .max_by_key(|v| v.version)
                    .map(|v| v.content_hash.clone())
                    .ok_or(ResolveError::NoMatchingVersion)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum AssetReference {
    /// Direct content hash (immutable, always same content)
    ContentHash(String),
    
    /// Asset ID with optional version number
    AssetId { id: String, version: Option<u32> },
    
    /// Semantic version constraint
    Semver { id: String, constraint: SemverConstraint },
}
```

### Scene Asset Locking

```rust
// crates/assets/src/locking.rs

/// Lock file for scene asset versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetLockFile {
    /// Lock file version
    pub version: u32,
    
    /// Locked asset versions
    pub assets: HashMap<String, LockedAsset>,
    
    /// Generated timestamp
    pub generated_at: DateTime<Utc>,
    
    /// Checksum of all locked hashes
    pub integrity_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedAsset {
    /// Asset ID
    pub asset_id: String,
    
    /// Locked version number
    pub version: u32,
    
    /// Content hash at lock time
    pub content_hash: String,
    
    /// Integrity verified
    pub verified: bool,
}

impl AssetLockFile {
    /// Generate lock file from scene
    pub async fn from_scene(scene: &Scene, resolver: &VersionResolver) -> Result<Self, LockError> {
        let mut assets = HashMap::new();
        
        for asset_ref in scene.asset_references() {
            let content_hash = resolver.resolve(&asset_ref).await?;
            let manifest = resolver.get_manifest(&asset_ref.asset_id()).await?;
            
            assets.insert(asset_ref.asset_id().to_string(), LockedAsset {
                asset_id: asset_ref.asset_id().to_string(),
                version: manifest.current_version,
                content_hash,
                verified: true,
            });
        }
        
        let integrity_hash = Self::compute_integrity(&assets);
        
        Ok(Self {
            version: 1,
            assets,
            generated_at: Utc::now(),
            integrity_hash,
        })
    }
}
```

---

## CDN Caching Strategy

### Edge Cache Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        CDN CACHING ARCHITECTURE                          │
└─────────────────────────────────────────────────────────────────────────┘

                              ┌─────────────┐
                              │   Client    │
                              └──────┬──────┘
                                     │
                                     ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  LAYER 1: CLIENT CACHE                                                   │
│  - Browser/game cache                                                    │
│  - Indexed by content hash (immutable)                                   │
│  - TTL: Infinite (content-addressed = never stale)                       │
└─────────────────────────────────────────────────────────────────────────┘
                                     │ MISS
                                     ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  LAYER 2: EDGE CACHE (Cloudflare/Fastly)                                 │
│  - Global edge network                                                   │
│  - Content hash → immutable, cache forever                               │
│  - Manifests → short TTL (5 min) for version updates                     │
└─────────────────────────────────────────────────────────────────────────┘
                                     │ MISS
                                     ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  LAYER 3: REGIONAL CACHE                                                 │
│  - Regional data centers                                                 │
│  - Reduces origin load                                                   │
│  - Handles burst traffic                                                 │
└─────────────────────────────────────────────────────────────────────────┘
                                     │ MISS
                                     ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  LAYER 4: ORIGIN (R2/S3/MinIO)                                           │
│  - Primary storage                                                       │
│  - Serves cache misses                                                   │
│  - Handles uploads                                                       │
└─────────────────────────────────────────────────────────────────────────┘
```

### Cache Headers Strategy

```rust
// crates/assets/src/cdn.rs

/// CDN cache control headers
pub struct CacheControl {
    /// Cache-Control header value
    pub cache_control: String,
    
    /// ETag (content hash)
    pub etag: String,
    
    /// Surrogate-Control (CDN-specific)
    pub surrogate_control: Option<String>,
}

impl CacheControl {
    /// Headers for content-addressed assets (immutable)
    pub fn for_content_hash(hash: &str) -> Self {
        Self {
            // Immutable = cache forever, never revalidate
            cache_control: "public, max-age=31536000, immutable".to_string(),
            etag: format!("\"{}\"", hash),
            surrogate_control: Some("max-age=31536000".to_string()),
        }
    }
    
    /// Headers for asset manifests (mutable)
    pub fn for_manifest() -> Self {
        Self {
            // Short cache, must revalidate
            cache_control: "public, max-age=300, must-revalidate".to_string(),
            etag: String::new(), // Set dynamically
            surrogate_control: Some("max-age=60".to_string()),
        }
    }
    
    /// Headers for presigned URLs (private, time-limited)
    pub fn for_presigned() -> Self {
        Self {
            cache_control: "private, no-store".to_string(),
            etag: String::new(),
            surrogate_control: None,
        }
    }
}
```

### Cache Invalidation

```rust
// crates/assets/src/invalidation.rs

/// CDN cache invalidation service
pub struct CacheInvalidationService {
    cloudflare_client: CloudflareClient,
    fastly_client: Option<FastlyClient>,
}

impl CacheInvalidationService {
    /// Invalidate manifest cache when asset is updated
    pub async fn invalidate_manifest(&self, asset_id: &str) -> Result<(), InvalidationError> {
        // Manifest URLs need invalidation on version update
        let urls = vec![
            format!("https://assets.eustress.io/manifests/{}.json", asset_id),
            format!("https://assets.eustress.io/v1/assets/{}/manifest", asset_id),
        ];
        
        self.cloudflare_client.purge_urls(&urls).await?;
        
        if let Some(ref fastly) = self.fastly_client {
            fastly.purge_urls(&urls).await?;
        }
        
        Ok(())
    }
    
    /// Purge content (for DMCA/CSAM removal)
    pub async fn purge_content(&self, content_hash: &str) -> Result<(), InvalidationError> {
        // Content hash URLs - purge from all edges
        let urls = vec![
            format!("https://assets.eustress.io/content/{}", content_hash),
            format!("https://cdn.eustress.io/{}", content_hash),
        ];
        
        // Also purge by cache tag if supported
        self.cloudflare_client.purge_by_tag(&format!("content:{}", content_hash)).await?;
        
        self.cloudflare_client.purge_urls(&urls).await?;
        
        Ok(())
    }
}
```

### Cloudflare R2 + Workers Setup

```javascript
// workers/asset-cdn/src/index.js

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const path = url.pathname;
    
    // Content-addressed assets: /content/{hash}
    if (path.startsWith('/content/')) {
      const hash = path.split('/')[2];
      
      // Check cache first
      const cache = caches.default;
      let response = await cache.match(request);
      
      if (!response) {
        // Fetch from R2
        const object = await env.ASSETS.get(`content/${hash}`);
        
        if (!object) {
          return new Response('Not Found', { status: 404 });
        }
        
        response = new Response(object.body, {
          headers: {
            'Content-Type': object.httpMetadata?.contentType || 'application/octet-stream',
            'Cache-Control': 'public, max-age=31536000, immutable',
            'ETag': `"${hash}"`,
            'X-Content-Hash': hash,
          },
        });
        
        // Cache forever (immutable)
        await cache.put(request, response.clone());
      }
      
      return response;
    }
    
    // Manifests: /manifests/{asset_id}.json
    if (path.startsWith('/manifests/')) {
      const assetId = path.split('/')[2].replace('.json', '');
      
      const object = await env.ASSETS.get(`manifests/${assetId}.json`);
      
      if (!object) {
        return new Response('Not Found', { status: 404 });
      }
      
      return new Response(object.body, {
        headers: {
          'Content-Type': 'application/json',
          'Cache-Control': 'public, max-age=300, must-revalidate',
          'ETag': object.httpEtag,
        },
      });
    }
    
    return new Response('Not Found', { status: 404 });
  },
};
```

---

## Bevy AssetServer Integration

### Custom AssetLoader for Eustress Assets

```rust
// crates/assets/src/bevy_integration.rs

use bevy::prelude::*;
use bevy::asset::{AssetLoader, LoadContext, LoadedAsset};
use bevy::asset::io::{AssetReader, AssetReaderError, Reader};
use std::path::Path;

/// Eustress asset loader plugin
pub struct EustressAssetPlugin {
    pub config: AssetConfig,
}

impl Plugin for EustressAssetPlugin {
    fn build(&self, app: &mut App) {
        // Register custom asset source
        app.register_asset_source(
            "eustress",
            AssetSource::build()
                .with_reader(move || Box::new(EustressAssetReader::new(self.config.clone())))
        );
        
        // Register custom loaders
        app.init_asset_loader::<EustressModelLoader>()
           .init_asset_loader::<EustressTextureLoader>()
           .init_asset_loader::<EustressAudioLoader>();
        
        // Add asset resolution system
        app.insert_resource(AssetResolver::new(self.config.clone()))
           .add_systems(Update, resolve_pending_assets);
    }
}

/// Custom asset reader for eustress:// URIs
pub struct EustressAssetReader {
    config: AssetConfig,
    http_client: reqwest::Client,
    cache: AssetCache,
}

impl EustressAssetReader {
    pub fn new(config: AssetConfig) -> Self {
        Self {
            config,
            http_client: reqwest::Client::new(),
            cache: AssetCache::new(),
        }
    }
}

impl AssetReader for EustressAssetReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<Box<Reader<'a>>, AssetReaderError> {
        let path_str = path.to_string_lossy();
        
        // Parse eustress:// URI
        // Format: eustress://asset_id[@version][/subpath]
        let (asset_id, version, subpath) = parse_eustress_uri(&path_str)?;
        
        // Check local cache first
        if let Some(data) = self.cache.get(&asset_id, version).await {
            return Ok(Box::new(std::io::Cursor::new(data)));
        }
        
        // Resolve to content hash
        let content_hash = self.resolve_content_hash(&asset_id, version).await?;
        
        // Fetch from CDN
        let data = self.fetch_content(&content_hash).await?;
        
        // Verify integrity
        let computed_hash = compute_hash(&data);
        if computed_hash != content_hash {
            return Err(AssetReaderError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Content hash mismatch",
            )));
        }
        
        // Cache locally
        self.cache.put(&asset_id, version, &data).await;
        
        Ok(Box::new(std::io::Cursor::new(data)))
    }
    
    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<Box<Reader<'a>>, AssetReaderError> {
        // Return empty meta for now
        Ok(Box::new(std::io::Cursor::new(Vec::new())))
    }
    
    async fn is_directory<'a>(&'a self, _path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
    
    async fn read_directory<'a>(
        &'a self,
        _path: &'a Path,
    ) -> Result<Box<bevy::asset::io::PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(PathBuf::new()))
    }
}

impl EustressAssetReader {
    async fn resolve_content_hash(&self, asset_id: &str, version: Option<u32>) -> Result<String, AssetReaderError> {
        // Fetch manifest
        let manifest_url = format!("{}/manifests/{}.json", self.config.cdn_url, asset_id);
        
        let response = self.http_client.get(&manifest_url)
            .send()
            .await
            .map_err(|e| AssetReaderError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        
        let manifest: AssetManifest = response.json()
            .await
            .map_err(|e| AssetReaderError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
        
        let target_version = version.unwrap_or(manifest.current_version);
        
        manifest.versions
            .iter()
            .find(|v| v.version == target_version)
            .map(|v| v.content_hash.clone())
            .ok_or_else(|| AssetReaderError::NotFound(PathBuf::from(asset_id)))
    }
    
    async fn fetch_content(&self, content_hash: &str) -> Result<Vec<u8>, AssetReaderError> {
        let content_url = format!("{}/content/{}", self.config.cdn_url, content_hash);
        
        let response = self.http_client.get(&content_url)
            .send()
            .await
            .map_err(|e| AssetReaderError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        
        response.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| AssetReaderError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))
    }
}
```

### Custom Asset Loaders

```rust
// crates/assets/src/loaders.rs

use bevy::prelude::*;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::render::texture::{Image, ImageType};

/// Loader for Eustress 3D models (.gltf, .glb)
#[derive(Default)]
pub struct EustressModelLoader;

impl AssetLoader for EustressModelLoader {
    type Asset = Scene;
    type Settings = ();
    type Error = ModelLoadError;
    
    fn load<'a>(
        &'a self,
        reader: &'a mut Reader,
        _settings: &'a Self::Settings,
        load_context: &'a mut LoadContext,
    ) -> BoxedFuture<'a, Result<Self::Asset, Self::Error>> {
        Box::pin(async move {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            
            // Detect format from magic bytes
            let format = detect_model_format(&bytes)?;
            
            match format {
                ModelFormat::Gltf => load_gltf(&bytes, load_context).await,
                ModelFormat::Glb => load_glb(&bytes, load_context).await,
                ModelFormat::Obj => load_obj(&bytes, load_context).await,
                ModelFormat::Fbx => load_fbx(&bytes, load_context).await,
            }
        })
    }
    
    fn extensions(&self) -> &[&str] {
        &["gltf", "glb", "obj", "fbx"]
    }
}

/// Loader for Eustress textures with format detection
#[derive(Default)]
pub struct EustressTextureLoader;

impl AssetLoader for EustressTextureLoader {
    type Asset = Image;
    type Settings = TextureSettings;
    type Error = TextureLoadError;
    
    fn load<'a>(
        &'a self,
        reader: &'a mut Reader,
        settings: &'a Self::Settings,
        _load_context: &'a mut LoadContext,
    ) -> BoxedFuture<'a, Result<Self::Asset, Self::Error>> {
        Box::pin(async move {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            
            // Detect format from magic bytes
            let format = detect_image_format(&bytes)?;
            
            let image = Image::from_buffer(
                &bytes,
                format.into(),
                settings.sampler.clone(),
                settings.is_srgb,
            )?;
            
            Ok(image)
        })
    }
    
    fn extensions(&self) -> &[&str] {
        &["png", "jpg", "jpeg", "webp", "ktx2", "dds", "basis"]
    }
}

#[derive(Default, Clone)]
pub struct TextureSettings {
    pub sampler: bevy::render::texture::ImageSampler,
    pub is_srgb: bool,
}
```

### Usage in Bevy

```rust
// Example usage in a Bevy app

use bevy::prelude::*;
use eustress_assets::EustressAssetPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(EustressAssetPlugin {
            config: AssetConfig {
                cdn_url: "https://assets.eustress.io".to_string(),
                cache_dir: dirs::cache_dir().unwrap().join("eustress/assets"),
                max_cache_size_mb: 1024,
            },
        })
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Load from Eustress CDN using asset ID
    let model: Handle<Scene> = asset_server.load("eustress://abc123-def456");
    
    // Load specific version
    let model_v2: Handle<Scene> = asset_server.load("eustress://abc123-def456@2");
    
    // Load by content hash (immutable)
    let texture: Handle<Image> = asset_server.load("eustress://content/QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG");
    
    // Spawn with loaded assets
    commands.spawn(SceneBundle {
        scene: model,
        ..default()
    });
}
```

### Asset Preloading

```rust
// crates/assets/src/preload.rs

/// Preload assets for a scene
pub struct AssetPreloader {
    asset_server: AssetServer,
    resolver: AssetResolver,
}

impl AssetPreloader {
    /// Preload all assets referenced in a scene
    pub async fn preload_scene(&self, scene_path: &str) -> Result<PreloadResult, PreloadError> {
        // Load scene manifest
        let manifest = self.load_scene_manifest(scene_path).await?;
        
        let mut handles = Vec::new();
        let mut total_size = 0u64;
        
        for asset_ref in &manifest.assets {
            // Resolve to content hash
            let content_hash = self.resolver.resolve(asset_ref).await?;
            
            // Start loading
            let handle = self.asset_server.load(format!("eustress://content/{}", content_hash));
            handles.push(handle);
            
            total_size += asset_ref.size_bytes;
        }
        
        Ok(PreloadResult {
            handles,
            total_assets: manifest.assets.len(),
            total_size_bytes: total_size,
        })
    }
    
    /// Check preload progress
    pub fn check_progress(&self, result: &PreloadResult) -> PreloadProgress {
        let loaded = result.handles.iter()
            .filter(|h| self.asset_server.is_loaded_with_dependencies(*h))
            .count();
        
        PreloadProgress {
            loaded,
            total: result.total_assets,
            percentage: (loaded as f32 / result.total_assets as f32) * 100.0,
        }
    }
}
```

---

## Summary

Eustress beats Roblox's asset system by:

1. **Decentralization**: No single point of failure
2. **Content-addressing**: Automatic deduplication and verification
3. **Self-hosting**: Creators control their assets
4. **Multi-source**: Fallback to multiple providers
5. **Open formats**: Support any file type
6. **Offline-first**: Full local caching
7. **P2P distribution**: Reduce server costs
8. **AI Moderation**: Automated CSAM/copyright detection
9. **Asset Versioning**: Manifest-based updates with rollback
10. **CDN Caching**: Immutable content cached forever at edge
11. **Bevy Integration**: Native AssetServer support

This creates a more resilient, flexible, and creator-friendly asset ecosystem.

---

## Related Documentation

- [ASSET_DEVELOPER_GUIDE.md](./ASSET_DEVELOPER_GUIDE.md) — Developer usage guide
- [s3.rs](../../eustress/crates/common/src/assets/s3.rs) — S3 client implementation
- [CSAM.md](../legal/CSAM.md) — Child safety policies
- [DMCA.md](../legal/DMCA.md) — Copyright procedures
- [AI_AGENTS.md](../moderation/AI_AGENTS.md) — AI moderation
- [MODERATION_API.md](../moderation/MODERATION_API.md) — Moderation API

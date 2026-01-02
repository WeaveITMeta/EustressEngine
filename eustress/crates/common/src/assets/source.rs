//! # AssetSource - Multi-Source Asset Resolution
//!
//! Defines where assets can be loaded from. Supports:
//! - Local filesystem (development)
//! - HTTP/HTTPS URLs
//! - IPFS (decentralized)
//! - S3-compatible storage
//! - Embedded in scene files
//! - P2P (BitTorrent-style)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Source from which an asset can be loaded
/// 
/// Assets can come from multiple sources, tried in order until one succeeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssetSource {
    /// Local filesystem path (for development)
    /// 
    /// Example: `Local("./assets/models/character.gltf")`
    Local(PathBuf),
    
    /// HTTP/HTTPS URL
    /// 
    /// Example: `Url("https://assets.mygame.com/models/character.gltf")`
    Url(String),
    
    /// IPFS content-addressed storage
    /// 
    /// Uses public gateways or local IPFS node.
    /// Example: `Ipfs { gateway: "https://ipfs.io", cid: "Qm..." }`
    Ipfs {
        /// IPFS gateway URL (e.g., "https://ipfs.io", "https://gateway.pinata.cloud")
        gateway: String,
        /// Content ID (CID) - the IPFS hash
        cid: String,
    },
    
    /// S3-compatible object storage (AWS S3, MinIO, R2, etc.)
    /// 
    /// Example: `S3 { bucket: "my-assets", key: "models/char.gltf", region: "us-east-1" }`
    S3 {
        bucket: String,
        key: String,
        region: String,
        /// Optional endpoint for S3-compatible services (MinIO, R2)
        endpoint: Option<String>,
    },
    
    /// Asset embedded directly in scene file (for small assets)
    /// 
    /// Base64-encoded data stored inline.
    Embedded(Vec<u8>),
    
    /// Peer-to-peer distribution (BitTorrent-style)
    /// 
    /// Uses info_hash to identify the asset across peers.
    P2P {
        /// BitTorrent info hash (20 bytes)
        info_hash: [u8; 20],
        /// Optional tracker URLs
        trackers: Vec<String>,
    },
    
    /// Cloudflare R2 (S3-compatible but with different auth)
    CloudflareR2 {
        account_id: String,
        bucket: String,
        key: String,
    },
}

impl AssetSource {
    /// Create a local source
    pub fn local<P: Into<PathBuf>>(path: P) -> Self {
        Self::Local(path.into())
    }
    
    /// Create a URL source
    pub fn url<S: Into<String>>(url: S) -> Self {
        Self::Url(url.into())
    }
    
    /// Create an IPFS source with default gateway
    pub fn ipfs<S: Into<String>>(cid: S) -> Self {
        Self::Ipfs {
            gateway: "https://ipfs.io".to_string(),
            cid: cid.into(),
        }
    }
    
    /// Create an IPFS source with custom gateway
    pub fn ipfs_with_gateway<S: Into<String>>(gateway: S, cid: S) -> Self {
        Self::Ipfs {
            gateway: gateway.into(),
            cid: cid.into(),
        }
    }
    
    /// Create an S3 source
    pub fn s3<S: Into<String>>(bucket: S, key: S, region: S) -> Self {
        Self::S3 {
            bucket: bucket.into(),
            key: key.into(),
            region: region.into(),
            endpoint: None,
        }
    }
    
    /// Create an embedded source from raw bytes
    pub fn embedded(data: Vec<u8>) -> Self {
        Self::Embedded(data)
    }
    
    /// Get the URL to fetch this asset from
    /// 
    /// Returns None for sources that require special handling (P2P, Embedded)
    pub fn to_url(&self) -> Option<String> {
        match self {
            Self::Local(path) => Some(format!("file://{}", path.display())),
            Self::Url(url) => Some(url.clone()),
            Self::Ipfs { gateway, cid } => Some(format!("{}/ipfs/{}", gateway, cid)),
            Self::S3 { bucket, key, region, endpoint } => {
                if let Some(ep) = endpoint {
                    Some(format!("{}/{}/{}", ep, bucket, key))
                } else {
                    Some(format!("https://{}.s3.{}.amazonaws.com/{}", bucket, region, key))
                }
            }
            Self::CloudflareR2 { account_id, bucket, key } => {
                Some(format!("https://{}.r2.cloudflarestorage.com/{}/{}", account_id, bucket, key))
            }
            Self::Embedded(_) | Self::P2P { .. } => None,
        }
    }
    
    /// Check if this source is available offline
    pub fn is_offline_available(&self) -> bool {
        matches!(self, Self::Local(_) | Self::Embedded(_))
    }
    
    /// Get a human-readable description
    pub fn description(&self) -> String {
        match self {
            Self::Local(path) => format!("Local: {}", path.display()),
            Self::Url(url) => format!("URL: {}", url),
            Self::Ipfs { cid, .. } => format!("IPFS: {}", &cid[..12.min(cid.len())]),
            Self::S3 { bucket, key, .. } => format!("S3: {}/{}", bucket, key),
            Self::CloudflareR2 { bucket, key, .. } => format!("R2: {}/{}", bucket, key),
            Self::Embedded(data) => format!("Embedded: {} bytes", data.len()),
            Self::P2P { info_hash, .. } => format!("P2P: {:02x}{:02x}...", info_hash[0], info_hash[1]),
        }
    }
}

/// Priority order for trying sources
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourcePriority {
    /// Embedded data (instant, no network)
    Embedded = 0,
    /// Local filesystem (fast, no network)
    Local = 1,
    /// CDN/URL (fast, requires network)
    Cdn = 2,
    /// S3/R2 (medium, requires network)
    ObjectStorage = 3,
    /// IPFS (variable, decentralized)
    Ipfs = 4,
    /// P2P (slow start, scales well)
    P2P = 5,
}

impl AssetSource {
    /// Get the priority of this source type
    pub fn priority(&self) -> SourcePriority {
        match self {
            Self::Embedded(_) => SourcePriority::Embedded,
            Self::Local(_) => SourcePriority::Local,
            Self::Url(_) => SourcePriority::Cdn,
            Self::S3 { .. } | Self::CloudflareR2 { .. } => SourcePriority::ObjectStorage,
            Self::Ipfs { .. } => SourcePriority::Ipfs,
            Self::P2P { .. } => SourcePriority::P2P,
        }
    }
}

/// Well-known IPFS gateways for fallback
pub const IPFS_GATEWAYS: &[&str] = &[
    "https://ipfs.io",
    "https://gateway.pinata.cloud",
    "https://cloudflare-ipfs.com",
    "https://dweb.link",
    "https://w3s.link",
];

/// Create IPFS sources for all known gateways
pub fn ipfs_all_gateways(cid: &str) -> Vec<AssetSource> {
    IPFS_GATEWAYS
        .iter()
        .map(|gateway| AssetSource::ipfs_with_gateway(*gateway, cid))
        .collect()
}

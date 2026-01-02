//! # S3/MinIO Asset Storage
//!
//! S3-compatible object storage support for self-hosted asset hosting.
//!
//! ## MinIO Setup
//!
//! MinIO is an open-source, S3-compatible object storage server.
//! Perfect for self-hosted game assets.
//!
//! ### Docker (Local Development)
//!
//! ```bash
//! docker run -p 9000:9000 -p 9001:9001 \
//!   -e MINIO_ROOT_USER=minioadmin \
//!   -e MINIO_ROOT_PASSWORD=minioadmin \
//!   -v ./data:/data \
//!   minio/minio server /data --console-address ":9001"
//! ```
//!
//! ### Fly.io ($5/mo)
//!
//! ```toml
//! # fly.toml
//! app = "my-game-assets"
//! primary_region = "ord"
//!
//! [build]
//!   image = "minio/minio"
//!
//! [env]
//!   MINIO_ROOT_USER = "minioadmin"
//!
//! [mounts]
//!   source = "minio_data"
//!   destination = "/data"
//!
//! [[services]]
//!   internal_port = 9000
//!   protocol = "tcp"
//!   [[services.ports]]
//!     port = 443
//!     handlers = ["tls", "http"]
//! ```
//!
//! ## Usage
//!
//! ```rust
//! use eustress_common::assets::s3::{S3Config, S3Client};
//!
//! // MinIO local
//! let config = S3Config::minio("http://localhost:9000", "minioadmin", "minioadmin");
//! let client = S3Client::new(config).await?;
//!
//! // Upload
//! let key = client.upload("game-assets", "models/player.gltf", &data).await?;
//!
//! // Download
//! let data = client.download("game-assets", "models/player.gltf").await?;
//! ```

use super::ContentHash;
use serde::{Deserialize, Serialize};

/// S3/MinIO configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    /// Endpoint URL (e.g., "http://localhost:9000" for MinIO)
    pub endpoint: String,
    
    /// AWS region (use "us-east-1" for MinIO)
    pub region: String,
    
    /// Access key ID
    pub access_key_id: String,
    
    /// Secret access key
    pub secret_access_key: String,
    
    /// Force path-style URLs (required for MinIO)
    pub force_path_style: bool,
    
    /// Default bucket name
    pub default_bucket: Option<String>,
}

impl S3Config {
    /// Create config for MinIO
    pub fn minio(endpoint: &str, access_key: &str, secret_key: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            region: "us-east-1".to_string(), // MinIO ignores this but SDK requires it
            access_key_id: access_key.to_string(),
            secret_access_key: secret_key.to_string(),
            force_path_style: true, // Required for MinIO
            default_bucket: None,
        }
    }
    
    /// Create config for AWS S3
    pub fn aws(region: &str, access_key: &str, secret_key: &str) -> Self {
        Self {
            endpoint: format!("https://s3.{}.amazonaws.com", region),
            region: region.to_string(),
            access_key_id: access_key.to_string(),
            secret_access_key: secret_key.to_string(),
            force_path_style: false,
            default_bucket: None,
        }
    }
    
    /// Create config for Cloudflare R2
    pub fn r2(account_id: &str, access_key: &str, secret_key: &str) -> Self {
        Self {
            endpoint: format!("https://{}.r2.cloudflarestorage.com", account_id),
            region: "auto".to_string(),
            access_key_id: access_key.to_string(),
            secret_access_key: secret_key.to_string(),
            force_path_style: true,
            default_bucket: None,
        }
    }
    
    /// Create config for DigitalOcean Spaces
    pub fn spaces(region: &str, access_key: &str, secret_key: &str) -> Self {
        Self {
            endpoint: format!("https://{}.digitaloceanspaces.com", region),
            region: region.to_string(),
            access_key_id: access_key.to_string(),
            secret_access_key: secret_key.to_string(),
            force_path_style: false,
            default_bucket: None,
        }
    }
    
    /// Set default bucket
    pub fn with_bucket(mut self, bucket: &str) -> Self {
        self.default_bucket = Some(bucket.to_string());
        self
    }
    
    /// Load from environment variables
    pub fn from_env() -> Option<Self> {
        Some(Self {
            endpoint: std::env::var("S3_ENDPOINT").ok()?,
            region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            access_key_id: std::env::var("AWS_ACCESS_KEY_ID").ok()?,
            secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").ok()?,
            force_path_style: std::env::var("S3_FORCE_PATH_STYLE")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            default_bucket: std::env::var("S3_BUCKET").ok(),
        })
    }
}

/// S3/MinIO client wrapper
#[cfg(feature = "s3")]
pub struct S3Client {
    client: aws_sdk_s3::Client,
    config: S3Config,
}

#[cfg(feature = "s3")]
impl S3Client {
    /// Create a new S3 client
    pub async fn new(config: S3Config) -> Result<Self, S3Error> {
        use aws_sdk_s3::config::{Credentials, Region};
        
        let credentials = Credentials::new(
            &config.access_key_id,
            &config.secret_access_key,
            None, // session token
            None, // expiry
            "eustress-assets",
        );
        
        let s3_config = aws_sdk_s3::Config::builder()
            .endpoint_url(&config.endpoint)
            .region(Region::new(config.region.clone()))
            .credentials_provider(credentials)
            .force_path_style(config.force_path_style)
            .build();
        
        let client = aws_sdk_s3::Client::from_conf(s3_config);
        
        Ok(Self { client, config })
    }
    
    /// Upload data to S3/MinIO
    /// 
    /// Returns the object key.
    pub async fn upload(&self, bucket: &str, key: &str, data: &[u8]) -> Result<String, S3Error> {
        use aws_sdk_s3::primitives::ByteStream;
        
        let body = ByteStream::from(data.to_vec());
        
        self.client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(body)
            .write()
            .await
            .map_err(|e| S3Error::Upload(e.to_string()))?;
        
        Ok(key.to_string())
    }
    
    /// Upload with content-addressable key (ContentHash)
    /// 
    /// Stores the asset using its hash as the key.
    pub async fn upload_cas(&self, bucket: &str, data: &[u8]) -> Result<ContentHash, S3Error> {
        let id = ContentHash::from_content(data);
        let key = id.to_base58();
        
        self.upload(bucket, &key, data).await?;
        
        Ok(id)
    }
    
    /// Download data from S3/MinIO
    pub async fn download(&self, bucket: &str, key: &str) -> Result<Vec<u8>, S3Error> {
        let response = self.client
            .get_object()
            .bucket(bucket)
            .key(key)
            .write()
            .await
            .map_err(|e| S3Error::Download(e.to_string()))?;
        
        let data = response.body
            .collect()
            .await
            .map_err(|e| S3Error::Download(e.to_string()))?
            .into_bytes()
            .to_vec();
        
        Ok(data)
    }
    
    /// Download by ContentHash
    pub async fn download_by_id(&self, bucket: &str, id: &ContentHash) -> Result<Vec<u8>, S3Error> {
        let key = id.to_base58();
        let data = self.download(bucket, &key).await?;
        
        // Verify hash
        if !id.verify(&data) {
            return Err(S3Error::HashMismatch);
        }
        
        Ok(data)
    }
    
    /// Check if object exists
    pub async fn exists(&self, bucket: &str, key: &str) -> bool {
        self.client
            .head_object()
            .bucket(bucket)
            .key(key)
            .write()
            .await
            .is_ok()
    }
    
    /// Delete object
    pub async fn delete(&self, bucket: &str, key: &str) -> Result<(), S3Error> {
        self.client
            .delete_object()
            .bucket(bucket)
            .key(key)
            .write()
            .await
            .map_err(|e| S3Error::Delete(e.to_string()))?;
        
        Ok(())
    }
    
    /// List objects in bucket
    pub async fn list(&self, bucket: &str, prefix: Option<&str>) -> Result<Vec<String>, S3Error> {
        let mut request = self.client.list_objects_v2().bucket(bucket);
        
        if let Some(p) = prefix {
            request = request.prefix(p);
        }
        
        let response = request
            .write()
            .await
            .map_err(|e| S3Error::List(e.to_string()))?;
        
        let keys = response.contents()
            .iter()
            .filter_map(|obj| obj.key().map(|k| k.to_string()))
            .collect();
        
        Ok(keys)
    }
    
    /// Create bucket if it doesn't exist
    pub async fn ensure_bucket(&self, bucket: &str) -> Result<(), S3Error> {
        // Check if bucket exists
        let exists = self.client
            .head_bucket()
            .bucket(bucket)
            .write()
            .await
            .is_ok();
        
        if !exists {
            self.client
                .create_bucket()
                .bucket(bucket)
                .write()
                .await
                .map_err(|e| S3Error::CreateBucket(e.to_string()))?;
        }
        
        Ok(())
    }
    
    /// Get presigned URL for direct download (valid for 1 hour)
    pub async fn presign_download(&self, bucket: &str, key: &str) -> Result<String, S3Error> {
        use aws_sdk_s3::presigning::PresigningConfig;
        use std::time::Duration;
        
        let presign_config = PresigningConfig::expires_in(Duration::from_secs(3600))
            .map_err(|e| S3Error::Presign(e.to_string()))?;
        
        let presigned = self.client
            .get_object()
            .bucket(bucket)
            .key(key)
            .presigned(presign_config)
            .await
            .map_err(|e| S3Error::Presign(e.to_string()))?;
        
        Ok(presigned.uri().to_string())
    }
    
    /// Get public URL (for public buckets)
    pub fn public_url(&self, bucket: &str, key: &str) -> String {
        if self.config.force_path_style {
            format!("{}/{}/{}", self.config.endpoint, bucket, key)
        } else {
            format!("https://{}.s3.{}.amazonaws.com/{}", bucket, self.config.region, key)
        }
    }
    
    /// Get default bucket
    pub fn default_bucket(&self) -> Option<&str> {
        self.config.default_bucket.as_deref()
    }
}

/// S3 errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum S3Error {
    #[error("Upload failed: {0}")]
    Upload(String),
    
    #[error("Download failed: {0}")]
    Download(String),
    
    #[error("Delete failed: {0}")]
    Delete(String),
    
    #[error("List failed: {0}")]
    List(String),
    
    #[error("Create bucket failed: {0}")]
    CreateBucket(String),
    
    #[error("Presign failed: {0}")]
    Presign(String),
    
    #[error("Hash mismatch - asset corrupted")]
    HashMismatch,
    
    #[error("Configuration error: {0}")]
    Config(String),
}

/// Stub client when S3 feature is disabled
#[cfg(not(feature = "s3"))]
pub struct S3Client;

#[cfg(not(feature = "s3"))]
impl S3Client {
    pub async fn new(_config: S3Config) -> Result<Self, S3Error> {
        Err(S3Error::Config("S3 feature not enabled. Add `features = [\"s3\"]` to Cargo.toml".to_string()))
    }
}

// ============================================================================
// MinIO-specific helpers
// ============================================================================

/// MinIO deployment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinioDeployment {
    /// Deployment type
    pub deployment: MinioDeploymentType,
    /// Endpoint URL
    pub endpoint: String,
    /// Access credentials
    pub credentials: MinioCredentials,
    /// Bucket configuration
    pub buckets: Vec<MinioBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MinioDeploymentType {
    /// Local Docker container
    Docker,
    /// Fly.io deployment
    FlyIo,
    /// Railway deployment
    Railway,
    /// Self-hosted VPS
    VPS,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinioCredentials {
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinioBucket {
    pub name: String,
    pub public: bool,
}

impl MinioDeployment {
    /// Docker Compose configuration
    pub fn docker_compose() -> String {
        r#"version: '3.8'

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
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9000/minio/health/live"]
      interval: 30s
      timeout: 20s
      retries: 3

volumes:
  minio_data:
"#.to_string()
    }
    
    /// Fly.io configuration
    pub fn fly_toml(app_name: &str) -> String {
        format!(r#"app = "{}"
primary_region = "ord"

[build]
  image = "minio/minio:latest"

[env]
  MINIO_ROOT_USER = "minioadmin"
  # Set MINIO_ROOT_PASSWORD via `fly secrets set`

[mounts]
  source = "minio_data"
  destination = "/data"

[processes]
  app = "server /data --console-address :9001"

[[services]]
  internal_port = 9000
  protocol = "tcp"
  auto_stop_machines = false
  auto_start_machines = true

  [[services.ports]]
    port = 443
    handlers = ["tls", "http"]

[[services]]
  internal_port = 9001
  protocol = "tcp"

  [[services.ports]]
    port = 9001
    handlers = ["tls", "http"]
"#, app_name)
    }
    
    /// Environment variables template
    pub fn env_template() -> String {
        r#"# S3/MinIO Configuration
S3_ENDPOINT=http://localhost:9000
S3_REGION=us-east-1
S3_BUCKET=game-assets
S3_FORCE_PATH_STYLE=true

# Credentials (use secrets in production!)
AWS_ACCESS_KEY_ID=minioadmin
AWS_SECRET_ACCESS_KEY=changeme
"#.to_string()
    }
}

// ============================================================================
// Integration with AssetResolver
// ============================================================================

/// Create an AssetSource for S3/MinIO
pub fn s3_source(config: &S3Config, bucket: &str, key: &str) -> super::AssetSource {
    super::AssetSource::S3 {
        bucket: bucket.to_string(),
        key: key.to_string(),
        region: config.region.clone(),
        endpoint: Some(config.endpoint.clone()),
    }
}

/// Create an AssetSource for MinIO with content-addressable key
pub fn minio_cas_source(endpoint: &str, bucket: &str, asset_id: &ContentHash) -> super::AssetSource {
    super::AssetSource::S3 {
        bucket: bucket.to_string(),
        key: asset_id.to_base58(),
        region: "us-east-1".to_string(),
        endpoint: Some(endpoint.to_string()),
    }
}

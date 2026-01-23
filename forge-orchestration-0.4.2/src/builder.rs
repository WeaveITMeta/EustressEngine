//! ForgeBuilder for configuring and constructing Forge instances
//!
//! ## Table of Contents
//! - **ForgeBuilder**: Builder pattern for Forge configuration
//! - **ForgeConfig**: Complete configuration struct

use crate::autoscaler::{Autoscaler, AutoscalerConfig};
use crate::error::Result;
use crate::metrics::ForgeMetrics;
use crate::moe::{BoxedMoERouter, DefaultMoERouter, MoERouter};
#[cfg(feature = "quic")]
use crate::networking::QuicConfig;
use crate::networking::HttpServerConfig;
use crate::nomad::NomadClient;
use crate::runtime::Forge;
use crate::storage::{BoxedStateStore, FileStore, MemoryStore};
use crate::types::Region;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

/// Complete Forge configuration
#[derive(Debug, Clone)]
pub struct ForgeConfig {
    /// Nomad API endpoint
    pub nomad_api: Option<String>,
    /// Nomad ACL token
    pub nomad_token: Option<String>,
    /// etcd endpoints
    pub etcd_endpoints: Vec<String>,
    /// Store path for file-based persistence
    pub store_path: Option<PathBuf>,
    /// HTTP server config
    pub http_config: HttpServerConfig,
    /// QUIC config (requires `quic` feature)
    #[cfg(feature = "quic")]
    pub quic_config: QuicConfig,
    /// Autoscaler config
    pub autoscaler_config: AutoscalerConfig,
    /// Federation regions
    pub federation_regions: Vec<Region>,
    /// Enable metrics
    pub metrics_enabled: bool,
    /// Node name
    pub node_name: String,
    /// Datacenter
    pub datacenter: String,
}

impl Default for ForgeConfig {
    fn default() -> Self {
        Self {
            nomad_api: None,
            nomad_token: None,
            etcd_endpoints: Vec::new(),
            store_path: None,
            http_config: HttpServerConfig::default(),
            #[cfg(feature = "quic")]
            quic_config: QuicConfig::default(),
            autoscaler_config: AutoscalerConfig::default(),
            federation_regions: Vec::new(),
            metrics_enabled: true,
            node_name: hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "forge-node".to_string()),
            datacenter: "dc1".to_string(),
        }
    }
}

/// Builder for constructing Forge instances
pub struct ForgeBuilder {
    config: ForgeConfig,
    router: Option<BoxedMoERouter>,
    store: Option<BoxedStateStore>,
}

impl ForgeBuilder {
    /// Create a new ForgeBuilder with default configuration
    pub fn new() -> Self {
        Self {
            config: ForgeConfig::default(),
            router: None,
            store: None,
        }
    }

    /// Set the Nomad API endpoint
    pub fn with_nomad_api(mut self, url: impl Into<String>) -> Self {
        self.config.nomad_api = Some(url.into());
        self
    }

    /// Set the Nomad ACL token
    pub fn with_nomad_token(mut self, token: impl Into<String>) -> Self {
        self.config.nomad_token = Some(token.into());
        self
    }

    /// Set etcd endpoints
    pub fn with_etcd_endpoints(mut self, endpoints: Vec<impl Into<String>>) -> Self {
        self.config.etcd_endpoints = endpoints.into_iter().map(|e| e.into()).collect();
        self
    }

    /// Set file store path for local storage
    pub fn with_store_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.store_path = Some(path.into());
        self
    }

    /// Set the MoE router
    pub fn with_moe_router<R: MoERouter + 'static>(mut self, router: R) -> Self {
        self.router = Some(Arc::new(router));
        self
    }

    /// Set a custom state store
    pub fn with_store(mut self, store: BoxedStateStore) -> Self {
        self.store = Some(store);
        self
    }

    /// Set autoscaler configuration
    pub fn with_autoscaler(mut self, config: AutoscalerConfig) -> Self {
        self.config.autoscaler_config = config;
        self
    }

    /// Set HTTP server configuration
    pub fn with_http_config(mut self, config: HttpServerConfig) -> Self {
        self.config.http_config = config;
        self
    }

    /// Set HTTP bind address
    pub fn with_http_addr(mut self, addr: &str) -> Result<Self> {
        self.config.http_config = self.config.http_config.with_addr_str(addr)?;
        Ok(self)
    }

    /// Set QUIC configuration (requires `quic` feature)
    #[cfg(feature = "quic")]
    pub fn with_quic_config(mut self, config: QuicConfig) -> Self {
        self.config.quic_config = config;
        self
    }

    /// Enable multi-region federation
    pub fn with_federation(mut self, regions: Vec<impl Into<Region>>) -> Self {
        self.config.federation_regions = regions.into_iter().map(|r| r.into()).collect();
        self
    }

    /// Set node name
    pub fn with_node_name(mut self, name: impl Into<String>) -> Self {
        self.config.node_name = name.into();
        self
    }

    /// Set datacenter
    pub fn with_datacenter(mut self, dc: impl Into<String>) -> Self {
        self.config.datacenter = dc.into();
        self
    }

    /// Enable or disable metrics
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.config.metrics_enabled = enabled;
        self
    }

    /// Build the Forge instance
    pub fn build(self) -> Result<Forge> {
        info!(
            node = %self.config.node_name,
            dc = %self.config.datacenter,
            "Building Forge instance"
        );

        // Create Nomad client if configured
        let nomad = match &self.config.nomad_api {
            Some(url) => {
                let mut client = NomadClient::new(url)?;
                if let Some(token) = &self.config.nomad_token {
                    client = client.with_token(token);
                }
                Some(client)
            }
            None => None,
        };

        // Create or use provided state store
        let store: BoxedStateStore = match self.store {
            Some(s) => s,
            None => {
                if let Some(path) = &self.config.store_path {
                    Arc::new(FileStore::open(path)?) as BoxedStateStore
                } else {
                    Arc::new(MemoryStore::new()) as BoxedStateStore
                }
            }
        };

        // Create MoE router
        let router = self.router.unwrap_or_else(|| Arc::new(DefaultMoERouter::new()));

        // Create autoscaler
        let autoscaler = Autoscaler::new(self.config.autoscaler_config.clone())?;

        // Create metrics
        let metrics = if self.config.metrics_enabled {
            Some(Arc::new(ForgeMetrics::new()?))
        } else {
            None
        };

        Ok(Forge::new(
            self.config,
            nomad,
            store,
            router,
            autoscaler,
            metrics,
        ))
    }
}

impl Default for ForgeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_default() {
        let forge = ForgeBuilder::new().build();
        assert!(forge.is_ok());
    }

    #[test]
    fn test_builder_with_nomad() {
        let forge = ForgeBuilder::new()
            .with_nomad_api("http://localhost:4646")
            .with_nomad_token("secret-token")
            .build();
        assert!(forge.is_ok());
    }

    #[test]
    fn test_builder_with_autoscaler() {
        let config = AutoscalerConfig::default()
            .upscale_threshold(0.9)
            .downscale_threshold(0.2);

        let forge = ForgeBuilder::new().with_autoscaler(config).build();
        assert!(forge.is_ok());
    }

    #[test]
    fn test_builder_with_custom_router() {
        use crate::moe::RoundRobinMoERouter;

        let forge = ForgeBuilder::new()
            .with_moe_router(RoundRobinMoERouter::new())
            .build();
        assert!(forge.is_ok());
    }
}

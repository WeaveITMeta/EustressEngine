//! Multi-Region Federation for Forge Orchestration
//!
//! Implements cross-region orchestration with:
//! - Region discovery and health monitoring
//! - Cross-region workload placement
//! - Geo-aware routing and failover
//! - Data locality optimization
//! - Latency-based routing

pub mod routing;
pub mod replication;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub use routing::{GeoRouter, RoutingPolicy, RegionRoute};
pub use replication::{ReplicationPolicy, ReplicationController};

/// Region configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionConfig {
    /// Region name (e.g., "us-east-1", "eu-west-1")
    pub name: String,
    /// Region display name
    pub display_name: String,
    /// API endpoint for this region
    pub endpoint: String,
    /// Geographic location
    pub location: GeoLocation,
    /// Region capacity
    pub capacity: RegionCapacity,
    /// Region labels
    pub labels: HashMap<String, String>,
    /// Is this the local region
    pub is_local: bool,
}

impl RegionConfig {
    /// Create new region config
    pub fn new(name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            display_name: String::new(),
            endpoint: endpoint.into(),
            location: GeoLocation::default(),
            capacity: RegionCapacity::default(),
            labels: HashMap::new(),
            is_local: false,
        }
    }

    /// Set display name
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = name.into();
        self
    }

    /// Set location
    pub fn with_location(mut self, lat: f64, lon: f64) -> Self {
        self.location = GeoLocation { latitude: lat, longitude: lon };
        self
    }

    /// Mark as local region
    pub fn as_local(mut self) -> Self {
        self.is_local = true;
        self
    }

    /// Add label
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }
}

/// Geographic location
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct GeoLocation {
    /// Latitude
    pub latitude: f64,
    /// Longitude
    pub longitude: f64,
}

impl GeoLocation {
    /// Calculate distance to another location in kilometers
    pub fn distance_to(&self, other: &GeoLocation) -> f64 {
        // Haversine formula
        let r = 6371.0; // Earth's radius in km
        
        let lat1 = self.latitude.to_radians();
        let lat2 = other.latitude.to_radians();
        let dlat = (other.latitude - self.latitude).to_radians();
        let dlon = (other.longitude - self.longitude).to_radians();

        let a = (dlat / 2.0).sin().powi(2) 
            + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();

        r * c
    }
}

/// Region capacity
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegionCapacity {
    /// Total CPU capacity (millicores)
    pub cpu_total: u64,
    /// Available CPU
    pub cpu_available: u64,
    /// Total memory (MB)
    pub memory_total: u64,
    /// Available memory
    pub memory_available: u64,
    /// Total GPU count
    pub gpu_total: u32,
    /// Available GPUs
    pub gpu_available: u32,
    /// Node count
    pub node_count: u32,
}

/// Region health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionHealth {
    /// Region name
    pub region: String,
    /// Is region healthy
    pub healthy: bool,
    /// Last health check time
    pub last_check: chrono::DateTime<chrono::Utc>,
    /// Round-trip latency in ms
    pub latency_ms: u32,
    /// Error message if unhealthy
    pub error: Option<String>,
    /// Consecutive failures
    pub consecutive_failures: u32,
}

impl RegionHealth {
    /// Create healthy status
    pub fn healthy(region: impl Into<String>, latency_ms: u32) -> Self {
        Self {
            region: region.into(),
            healthy: true,
            last_check: chrono::Utc::now(),
            latency_ms,
            error: None,
            consecutive_failures: 0,
        }
    }

    /// Create unhealthy status
    pub fn unhealthy(region: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            region: region.into(),
            healthy: false,
            last_check: chrono::Utc::now(),
            latency_ms: 0,
            error: Some(error.into()),
            consecutive_failures: 1,
        }
    }
}

/// Federation manager for multi-region coordination
pub struct FederationManager {
    /// Local region name
    local_region: String,
    /// Known regions
    regions: Arc<RwLock<HashMap<String, RegionConfig>>>,
    /// Region health status
    health: Arc<RwLock<HashMap<String, RegionHealth>>>,
    /// Geo router
    router: Arc<GeoRouter>,
    /// Health check interval
    health_check_interval: Duration,
}

impl FederationManager {
    /// Create new federation manager
    pub fn new(local_region: impl Into<String>) -> Self {
        Self {
            local_region: local_region.into(),
            regions: Arc::new(RwLock::new(HashMap::new())),
            health: Arc::new(RwLock::new(HashMap::new())),
            router: Arc::new(GeoRouter::new()),
            health_check_interval: Duration::from_secs(30),
        }
    }

    /// Register a region
    pub fn register_region(&self, config: RegionConfig) {
        info!(region = %config.name, endpoint = %config.endpoint, "Registering region");
        
        let name = config.name.clone();
        self.regions.write().insert(name.clone(), config);
        
        // Initialize health as unknown
        self.health.write().insert(name.clone(), RegionHealth {
            region: name,
            healthy: false,
            last_check: chrono::Utc::now(),
            latency_ms: 0,
            error: Some("Not yet checked".to_string()),
            consecutive_failures: 0,
        });
    }

    /// Unregister a region
    pub fn unregister_region(&self, name: &str) {
        info!(region = name, "Unregistering region");
        self.regions.write().remove(name);
        self.health.write().remove(name);
    }

    /// Get all regions
    pub fn regions(&self) -> Vec<RegionConfig> {
        self.regions.read().values().cloned().collect()
    }

    /// Get healthy regions
    pub fn healthy_regions(&self) -> Vec<RegionConfig> {
        let health = self.health.read();
        self.regions.read()
            .values()
            .filter(|r| health.get(&r.name).map(|h| h.healthy).unwrap_or(false))
            .cloned()
            .collect()
    }

    /// Get region by name
    pub fn get_region(&self, name: &str) -> Option<RegionConfig> {
        self.regions.read().get(name).cloned()
    }

    /// Get region health
    pub fn get_health(&self, name: &str) -> Option<RegionHealth> {
        self.health.read().get(name).cloned()
    }

    /// Update region health
    pub fn update_health(&self, health: RegionHealth) {
        let region = health.region.clone();
        let was_healthy = self.health.read()
            .get(&region)
            .map(|h| h.healthy)
            .unwrap_or(false);

        if was_healthy && !health.healthy {
            warn!(region = %region, error = ?health.error, "Region became unhealthy");
        } else if !was_healthy && health.healthy {
            info!(region = %region, latency_ms = health.latency_ms, "Region became healthy");
        }

        self.health.write().insert(region, health);
    }

    /// Find best region for a workload
    pub fn find_best_region(&self, requirements: &PlacementRequirements) -> Option<String> {
        let regions = self.healthy_regions();
        
        if regions.is_empty() {
            return None;
        }

        // Filter by capacity
        let candidates: Vec<_> = regions.iter()
            .filter(|r| {
                r.capacity.cpu_available >= requirements.cpu_millis
                    && r.capacity.memory_available >= requirements.memory_mb
                    && r.capacity.gpu_available >= requirements.gpu_count
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // Score candidates
        let health = self.health.read();
        let mut best: Option<(&RegionConfig, f64)> = None;

        for region in candidates {
            let mut score = 0.0;

            // Prefer local region
            if region.is_local {
                score += 100.0;
            }

            // Prefer low latency
            if let Some(h) = health.get(&region.name) {
                score += 50.0 / (1.0 + h.latency_ms as f64 / 100.0);
            }

            // Prefer regions with more capacity
            let cpu_ratio = region.capacity.cpu_available as f64 / region.capacity.cpu_total.max(1) as f64;
            score += cpu_ratio * 30.0;

            // Check affinity
            if let Some(preferred) = &requirements.preferred_region {
                if &region.name == preferred {
                    score += 200.0;
                }
            }

            // Check anti-affinity
            if let Some(avoid) = &requirements.avoid_region {
                if &region.name == avoid {
                    score -= 500.0;
                }
            }

            if best.is_none() || score > best.unwrap().1 {
                best = Some((region, score));
            }
        }

        best.map(|(r, _)| r.name.clone())
    }

    /// Get the geo router
    pub fn router(&self) -> Arc<GeoRouter> {
        self.router.clone()
    }

    /// Start health check loop
    pub async fn start_health_checks(self: Arc<Self>) {
        let manager = self.clone();
        
        tokio::spawn(async move {
            loop {
                manager.check_all_regions().await;
                tokio::time::sleep(manager.health_check_interval).await;
            }
        });
    }

    /// Check health of all regions
    async fn check_all_regions(&self) {
        let regions: Vec<_> = self.regions.read().values().cloned().collect();
        
        for region in regions {
            if region.is_local {
                // Local region is always healthy
                self.update_health(RegionHealth::healthy(&region.name, 0));
                continue;
            }

            let health = self.check_region_health(&region).await;
            self.update_health(health);
        }
    }

    /// Check health of a single region
    async fn check_region_health(&self, region: &RegionConfig) -> RegionHealth {
        let start = Instant::now();
        
        // Try to connect to region endpoint
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        let health_url = format!("{}/healthz", region.endpoint);
        
        match client.get(&health_url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    RegionHealth::healthy(&region.name, start.elapsed().as_millis() as u32)
                } else {
                    RegionHealth::unhealthy(&region.name, format!("HTTP {}", response.status()))
                }
            }
            Err(e) => {
                RegionHealth::unhealthy(&region.name, e.to_string())
            }
        }
    }
}

/// Placement requirements for cross-region scheduling
#[derive(Debug, Clone, Default)]
pub struct PlacementRequirements {
    /// CPU requirement (millicores)
    pub cpu_millis: u64,
    /// Memory requirement (MB)
    pub memory_mb: u64,
    /// GPU count requirement
    pub gpu_count: u32,
    /// Preferred region
    pub preferred_region: Option<String>,
    /// Region to avoid
    pub avoid_region: Option<String>,
    /// Required labels
    pub required_labels: HashMap<String, String>,
    /// Data locality (region where data resides)
    pub data_locality: Option<String>,
}

impl PlacementRequirements {
    /// Create new requirements
    pub fn new() -> Self {
        Self::default()
    }

    /// Set CPU requirement
    pub fn cpu(mut self, millis: u64) -> Self {
        self.cpu_millis = millis;
        self
    }

    /// Set memory requirement
    pub fn memory(mut self, mb: u64) -> Self {
        self.memory_mb = mb;
        self
    }

    /// Set GPU requirement
    pub fn gpu(mut self, count: u32) -> Self {
        self.gpu_count = count;
        self
    }

    /// Set preferred region
    pub fn prefer_region(mut self, region: impl Into<String>) -> Self {
        self.preferred_region = Some(region.into());
        self
    }

    /// Set region to avoid
    pub fn avoid_region(mut self, region: impl Into<String>) -> Self {
        self.avoid_region = Some(region.into());
        self
    }

    /// Set data locality
    pub fn with_data_locality(mut self, region: impl Into<String>) -> Self {
        self.data_locality = Some(region.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geo_distance() {
        // New York to London
        let ny = GeoLocation { latitude: 40.7128, longitude: -74.0060 };
        let london = GeoLocation { latitude: 51.5074, longitude: -0.1278 };
        
        let distance = ny.distance_to(&london);
        // Should be approximately 5570 km
        assert!(distance > 5500.0 && distance < 5700.0);
    }

    #[test]
    fn test_federation_manager() {
        let manager = FederationManager::new("us-east-1");
        
        manager.register_region(
            RegionConfig::new("us-east-1", "http://localhost:6443")
                .with_location(39.0, -77.0)
                .as_local()
        );
        
        manager.register_region(
            RegionConfig::new("eu-west-1", "http://eu.example.com:6443")
                .with_location(53.0, -8.0)
        );

        assert_eq!(manager.regions().len(), 2);
    }

    #[test]
    fn test_find_best_region() {
        let manager = FederationManager::new("us-east-1");
        
        let mut us_config = RegionConfig::new("us-east-1", "http://localhost:6443").as_local();
        us_config.capacity = RegionCapacity {
            cpu_total: 10000,
            cpu_available: 5000,
            memory_total: 32000,
            memory_available: 16000,
            gpu_total: 4,
            gpu_available: 2,
            node_count: 5,
        };
        manager.register_region(us_config);
        
        // Mark as healthy
        manager.update_health(RegionHealth::healthy("us-east-1", 5));

        let requirements = PlacementRequirements::new()
            .cpu(1000)
            .memory(2048);

        let best = manager.find_best_region(&requirements);
        assert_eq!(best, Some("us-east-1".to_string()));
    }
}

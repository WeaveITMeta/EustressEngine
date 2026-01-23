//! Geo-aware routing for multi-region federation
//!
//! Implements intelligent routing based on:
//! - Geographic proximity
//! - Latency measurements
//! - Region health
//! - Data locality

use std::collections::HashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::GeoLocation;

/// Routing policy for cross-region requests
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutingPolicy {
    /// Route to nearest healthy region
    Nearest,
    /// Route to lowest latency region
    LowestLatency,
    /// Route to region with most capacity
    MostCapacity,
    /// Route to specific region (with fallback)
    Pinned,
    /// Round-robin across healthy regions
    RoundRobin,
    /// Weighted random based on capacity
    WeightedRandom,
}

impl Default for RoutingPolicy {
    fn default() -> Self {
        Self::LowestLatency
    }
}

/// Route to a specific region
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionRoute {
    /// Target region
    pub region: String,
    /// Route weight (for weighted routing)
    pub weight: u32,
    /// Is this a fallback route
    pub is_fallback: bool,
    /// Route priority (lower = higher priority)
    pub priority: u32,
}

impl RegionRoute {
    /// Create new route
    pub fn new(region: impl Into<String>) -> Self {
        Self {
            region: region.into(),
            weight: 100,
            is_fallback: false,
            priority: 0,
        }
    }

    /// Set as fallback
    pub fn as_fallback(mut self) -> Self {
        self.is_fallback = true;
        self.priority = 100;
        self
    }

    /// Set weight
    pub fn with_weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }
}

/// Geo-aware router for multi-region routing
pub struct GeoRouter {
    /// Region locations
    locations: RwLock<HashMap<String, GeoLocation>>,
    /// Region latencies (measured RTT in ms)
    latencies: RwLock<HashMap<String, u32>>,
    /// Region health
    health: RwLock<HashMap<String, bool>>,
    /// Default routing policy
    default_policy: RwLock<RoutingPolicy>,
    /// Round-robin counter
    rr_counter: RwLock<usize>,
    /// Static routes
    static_routes: RwLock<HashMap<String, Vec<RegionRoute>>>,
}

impl GeoRouter {
    /// Create new geo router
    pub fn new() -> Self {
        Self {
            locations: RwLock::new(HashMap::new()),
            latencies: RwLock::new(HashMap::new()),
            health: RwLock::new(HashMap::new()),
            default_policy: RwLock::new(RoutingPolicy::LowestLatency),
            rr_counter: RwLock::new(0),
            static_routes: RwLock::new(HashMap::new()),
        }
    }

    /// Set default routing policy
    pub fn set_default_policy(&self, policy: RoutingPolicy) {
        *self.default_policy.write() = policy;
    }

    /// Register region location
    pub fn register_location(&self, region: impl Into<String>, location: GeoLocation) {
        self.locations.write().insert(region.into(), location);
    }

    /// Update region latency
    pub fn update_latency(&self, region: impl Into<String>, latency_ms: u32) {
        self.latencies.write().insert(region.into(), latency_ms);
    }

    /// Update region health
    pub fn update_health(&self, region: impl Into<String>, healthy: bool) {
        self.health.write().insert(region.into(), healthy);
    }

    /// Add static route for a service
    pub fn add_static_route(&self, service: impl Into<String>, route: RegionRoute) {
        let mut routes = self.static_routes.write();
        routes.entry(service.into())
            .or_insert_with(Vec::new)
            .push(route);
    }

    /// Route request to best region
    pub fn route(&self, client_location: Option<&GeoLocation>, policy: Option<RoutingPolicy>) -> Option<String> {
        let policy = policy.unwrap_or_else(|| *self.default_policy.read());
        let health = self.health.read();
        
        // Get healthy regions
        let healthy_regions: Vec<_> = health.iter()
            .filter(|(_, &healthy)| healthy)
            .map(|(r, _)| r.clone())
            .collect();

        if healthy_regions.is_empty() {
            return None;
        }

        match policy {
            RoutingPolicy::Nearest => {
                self.route_nearest(client_location, &healthy_regions)
            }
            RoutingPolicy::LowestLatency => {
                self.route_lowest_latency(&healthy_regions)
            }
            RoutingPolicy::MostCapacity => {
                // For now, just return first healthy region
                // Real implementation would check capacity
                healthy_regions.first().cloned()
            }
            RoutingPolicy::RoundRobin => {
                self.route_round_robin(&healthy_regions)
            }
            RoutingPolicy::WeightedRandom => {
                self.route_weighted_random(&healthy_regions)
            }
            RoutingPolicy::Pinned => {
                // Return first healthy region as pinned
                healthy_regions.first().cloned()
            }
        }
    }

    /// Route for a specific service
    pub fn route_service(&self, service: &str, client_location: Option<&GeoLocation>) -> Option<String> {
        let routes = self.static_routes.read();
        
        if let Some(service_routes) = routes.get(service) {
            let health = self.health.read();
            
            // Sort by priority and find first healthy route
            let mut sorted_routes = service_routes.clone();
            sorted_routes.sort_by_key(|r| r.priority);

            for route in sorted_routes {
                if health.get(&route.region).copied().unwrap_or(false) {
                    return Some(route.region);
                }
            }
        }

        // Fall back to default routing
        self.route(client_location, None)
    }

    /// Route to nearest region by geographic distance
    fn route_nearest(&self, client_location: Option<&GeoLocation>, healthy_regions: &[String]) -> Option<String> {
        let client_loc = client_location?;
        let locations = self.locations.read();

        healthy_regions.iter()
            .filter_map(|r| {
                locations.get(r).map(|loc| (r, client_loc.distance_to(loc)))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(r, _)| r.clone())
    }

    /// Route to lowest latency region
    fn route_lowest_latency(&self, healthy_regions: &[String]) -> Option<String> {
        let latencies = self.latencies.read();

        healthy_regions.iter()
            .filter_map(|r| latencies.get(r).map(|&lat| (r, lat)))
            .min_by_key(|(_, lat)| *lat)
            .map(|(r, _)| r.clone())
            .or_else(|| healthy_regions.first().cloned())
    }

    /// Round-robin routing
    fn route_round_robin(&self, healthy_regions: &[String]) -> Option<String> {
        if healthy_regions.is_empty() {
            return None;
        }

        let mut counter = self.rr_counter.write();
        let index = *counter % healthy_regions.len();
        *counter = counter.wrapping_add(1);

        healthy_regions.get(index).cloned()
    }

    /// Weighted random routing based on inverse latency
    fn route_weighted_random(&self, healthy_regions: &[String]) -> Option<String> {
        if healthy_regions.is_empty() {
            return None;
        }

        let latencies = self.latencies.read();
        
        // Calculate weights (inverse of latency)
        let weights: Vec<_> = healthy_regions.iter()
            .map(|r| {
                let latency = latencies.get(r).copied().unwrap_or(100);
                let weight = 10000 / (latency.max(1) as u64);
                (r, weight)
            })
            .collect();

        let total_weight: u64 = weights.iter().map(|(_, w)| w).sum();
        
        if total_weight == 0 {
            return healthy_regions.first().cloned();
        }

        // Simple pseudo-random selection
        let rand = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0);
        
        let target = rand % total_weight;
        let mut cumulative = 0u64;

        for (region, weight) in weights {
            cumulative += weight;
            if cumulative > target {
                return Some(region.clone());
            }
        }

        healthy_regions.first().cloned()
    }

    /// Get all healthy regions sorted by preference
    pub fn get_preferred_regions(&self, client_location: Option<&GeoLocation>) -> Vec<String> {
        let health = self.health.read();
        let latencies = self.latencies.read();
        let locations = self.locations.read();

        let mut regions: Vec<_> = health.iter()
            .filter(|(_, &healthy)| healthy)
            .map(|(r, _)| {
                let latency = latencies.get(r).copied().unwrap_or(u32::MAX);
                let distance = client_location
                    .and_then(|cl| locations.get(r).map(|rl| cl.distance_to(rl)))
                    .unwrap_or(f64::MAX);
                (r.clone(), latency, distance)
            })
            .collect();

        // Sort by latency, then distance
        regions.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
        });

        regions.into_iter().map(|(r, _, _)| r).collect()
    }
}

impl Default for GeoRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routing_policy() {
        let router = GeoRouter::new();
        
        router.update_health("us-east-1", true);
        router.update_health("eu-west-1", true);
        router.update_latency("us-east-1", 10);
        router.update_latency("eu-west-1", 100);

        // Should route to lowest latency
        let result = router.route(None, Some(RoutingPolicy::LowestLatency));
        assert_eq!(result, Some("us-east-1".to_string()));
    }

    #[test]
    fn test_nearest_routing() {
        let router = GeoRouter::new();
        
        // New York
        let ny = GeoLocation { latitude: 40.7128, longitude: -74.0060 };
        // Virginia (us-east-1)
        let virginia = GeoLocation { latitude: 39.0, longitude: -77.0 };
        // Ireland (eu-west-1)
        let ireland = GeoLocation { latitude: 53.0, longitude: -8.0 };

        router.register_location("us-east-1", virginia);
        router.register_location("eu-west-1", ireland);
        router.update_health("us-east-1", true);
        router.update_health("eu-west-1", true);

        let result = router.route(Some(&ny), Some(RoutingPolicy::Nearest));
        assert_eq!(result, Some("us-east-1".to_string()));
    }

    #[test]
    fn test_round_robin() {
        let router = GeoRouter::new();
        
        router.update_health("region-1", true);
        router.update_health("region-2", true);
        router.update_health("region-3", true);

        let r1 = router.route(None, Some(RoutingPolicy::RoundRobin));
        let r2 = router.route(None, Some(RoutingPolicy::RoundRobin));
        let r3 = router.route(None, Some(RoutingPolicy::RoundRobin));
        let r4 = router.route(None, Some(RoutingPolicy::RoundRobin));

        // Should cycle through regions
        assert_ne!(r1, r2);
        assert_ne!(r2, r3);
        assert_eq!(r1, r4); // Back to first
    }

    #[test]
    fn test_static_routes() {
        let router = GeoRouter::new();
        
        router.update_health("us-east-1", true);
        router.update_health("eu-west-1", true);
        
        router.add_static_route("my-service", RegionRoute::new("eu-west-1").with_priority(0));
        router.add_static_route("my-service", RegionRoute::new("us-east-1").as_fallback());

        let result = router.route_service("my-service", None);
        assert_eq!(result, Some("eu-west-1".to_string()));
    }
}

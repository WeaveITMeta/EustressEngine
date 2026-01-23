//! Experience routing for Eustress game servers.
//!
//! Routes players to appropriate game server instances based on experience ID,
//! region, and server capacity.

use crate::config::Region;
use async_trait::async_trait;
use forge_orchestration::moe::{MoERouter, RouteResult};
use forge_orchestration::types::Expert;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

/// Experience metadata for routing decisions.
#[derive(Debug, Clone)]
pub struct ExperienceInfo {
    /// Experience ID
    pub id: String,
    /// Display name
    pub name: String,
    /// Preferred region
    pub region: Region,
    /// Maximum players per server
    pub max_players: u32,
    /// Current player count across all servers
    pub total_players: u32,
    /// Number of active servers
    pub server_count: u32,
}

/// Server instance info for routing.
#[derive(Debug, Clone)]
pub struct ServerInstance {
    /// Server ID (job allocation ID)
    pub server_id: String,
    /// Experience ID this server is running
    pub experience_id: String,
    /// Region
    pub region: Region,
    /// Current player count
    pub player_count: u32,
    /// Maximum players
    pub max_players: u32,
    /// Server load (0.0 - 1.0)
    pub load: f64,
    /// Is server accepting new players
    pub accepting: bool,
}

impl ServerInstance {
    /// Check if server has capacity for more players.
    pub fn has_capacity(&self) -> bool {
        self.accepting && self.player_count < self.max_players
    }

    /// Get available slots.
    pub fn available_slots(&self) -> u32 {
        if self.accepting {
            self.max_players.saturating_sub(self.player_count)
        } else {
            0
        }
    }

    /// Get fill percentage.
    pub fn fill_percent(&self) -> f64 {
        if self.max_players == 0 {
            1.0
        } else {
            self.player_count as f64 / self.max_players as f64
        }
    }
}

/// Experience-aware router for game servers.
///
/// Routes players to the best available server for their requested experience,
/// considering region, capacity, and load.
#[derive(Clone)]
pub struct ExperienceRouter {
    /// Experience ID -> list of server instances
    servers: Arc<RwLock<HashMap<String, Vec<ServerInstance>>>>,
    /// Prefer servers in same region
    prefer_same_region: bool,
    /// Target fill percentage before spawning new server
    target_fill: f64,
}

impl ExperienceRouter {
    /// Create a new experience router.
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            prefer_same_region: true,
            target_fill: 0.8,
        }
    }

    /// Set region preference.
    pub fn with_region_preference(mut self, prefer: bool) -> Self {
        self.prefer_same_region = prefer;
        self
    }

    /// Set target fill percentage.
    pub fn with_target_fill(mut self, fill: f64) -> Self {
        self.target_fill = fill.clamp(0.1, 1.0);
        self
    }

    /// Register a server instance.
    pub fn register_server(&self, server: ServerInstance) {
        let mut servers = self.servers.write();
        servers
            .entry(server.experience_id.clone())
            .or_insert_with(Vec::new)
            .push(server);
    }

    /// Update server info.
    pub fn update_server(&self, server_id: &str, player_count: u32, load: f64, accepting: bool) {
        let mut servers = self.servers.write();
        for instances in servers.values_mut() {
            if let Some(server) = instances.iter_mut().find(|s| s.server_id == server_id) {
                server.player_count = player_count;
                server.load = load;
                server.accepting = accepting;
                return;
            }
        }
    }

    /// Remove a server instance.
    pub fn remove_server(&self, server_id: &str) {
        let mut servers = self.servers.write();
        for instances in servers.values_mut() {
            instances.retain(|s| s.server_id != server_id);
        }
    }

    /// Get servers for an experience.
    pub fn get_servers(&self, experience_id: &str) -> Vec<ServerInstance> {
        self.servers
            .read()
            .get(experience_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Find best server for a player.
    ///
    /// Returns `None` if no suitable server exists (need to spawn new one).
    pub fn find_server(&self, experience_id: &str, preferred_region: Option<Region>) -> Option<ServerInstance> {
        let servers = self.servers.read();
        let instances = servers.get(experience_id)?;

        // Filter to servers with capacity
        let mut candidates: Vec<_> = instances
            .iter()
            .filter(|s| s.has_capacity())
            .cloned()
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // Sort by preference
        candidates.sort_by(|a, b| {
            // 1. Prefer same region
            if self.prefer_same_region {
                if let Some(region) = preferred_region {
                    let a_same = a.region == region;
                    let b_same = b.region == region;
                    if a_same != b_same {
                        return b_same.cmp(&a_same);
                    }
                }
            }

            // 2. Prefer servers with more players (for social experience)
            //    but not too full (below target fill)
            let a_fill = a.fill_percent();
            let b_fill = b.fill_percent();
            
            let a_good = a_fill < self.target_fill;
            let b_good = b_fill < self.target_fill;
            
            if a_good && b_good {
                // Both under target, prefer more full
                b_fill.partial_cmp(&a_fill).unwrap_or(std::cmp::Ordering::Equal)
            } else if a_good {
                std::cmp::Ordering::Less
            } else if b_good {
                std::cmp::Ordering::Greater
            } else {
                // Both over target, prefer less full
                a_fill.partial_cmp(&b_fill).unwrap_or(std::cmp::Ordering::Equal)
            }
        });

        candidates.into_iter().next()
    }

    /// Check if a new server should be spawned for an experience.
    pub fn should_spawn_server(&self, experience_id: &str) -> bool {
        let servers = self.servers.read();
        
        match servers.get(experience_id) {
            None => true, // No servers at all
            Some(instances) => {
                // Check if all servers are above target fill
                let accepting_servers: Vec<_> = instances.iter().filter(|s| s.accepting).collect();
                
                if accepting_servers.is_empty() {
                    return true;
                }

                // Spawn if average fill is above target
                let avg_fill: f64 = accepting_servers.iter().map(|s| s.fill_percent()).sum::<f64>()
                    / accepting_servers.len() as f64;
                
                avg_fill >= self.target_fill
            }
        }
    }

    /// Get experience statistics.
    pub fn experience_stats(&self, experience_id: &str) -> Option<ExperienceInfo> {
        let servers = self.servers.read();
        let instances = servers.get(experience_id)?;

        if instances.is_empty() {
            return None;
        }

        let total_players: u32 = instances.iter().map(|s| s.player_count).sum();
        let server_count = instances.len() as u32;
        
        // Use first server's info for name/region (could be improved)
        let first = &instances[0];

        Some(ExperienceInfo {
            id: experience_id.to_string(),
            name: experience_id.to_string(), // Would come from metadata
            region: first.region,
            max_players: first.max_players,
            total_players,
            server_count,
        })
    }
}

impl Default for ExperienceRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MoERouter for ExperienceRouter {
    async fn route(&self, input: &str, num_experts: usize) -> RouteResult {
        // Input format: "experience_id:region" or just "experience_id"
        let parts: Vec<&str> = input.split(':').collect();
        let experience_id = parts.first().unwrap_or(&"");
        let region = parts.get(1).and_then(|r| match *r {
            "us-east" => Some(Region::UsEast),
            "us-west" => Some(Region::UsWest),
            "eu-west" => Some(Region::EuWest),
            "eu-central" => Some(Region::EuCentral),
            "asia-pacific" => Some(Region::AsiaPacific),
            "south-america" => Some(Region::SouthAmerica),
            _ => None,
        });

        if let Some(server) = self.find_server(experience_id, region) {
            // Hash server_id to expert index
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            server.server_id.hash(&mut hasher);
            let index = (hasher.finish() % num_experts as u64) as usize;

            RouteResult::new(index)
                .with_confidence(1.0 - server.fill_percent())
        } else {
            // No server found, return index 0 with low confidence
            RouteResult::new(0).with_confidence(0.0)
        }
    }

    async fn route_with_experts(&self, input: &str, experts: &[Expert]) -> RouteResult {
        // For experience routing, we don't use the expert list directly
        // Instead we route based on our server registry
        self.route(input, experts.len()).await
    }

    fn name(&self) -> &str {
        "experience-router"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_capacity() {
        let server = ServerInstance {
            server_id: "test-1".into(),
            experience_id: "exp-1".into(),
            region: Region::UsEast,
            player_count: 50,
            max_players: 100,
            load: 0.5,
            accepting: true,
        };

        assert!(server.has_capacity());
        assert_eq!(server.available_slots(), 50);
        assert!((server.fill_percent() - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_experience_router() {
        let router = ExperienceRouter::new();

        router.register_server(ServerInstance {
            server_id: "server-1".into(),
            experience_id: "my-game".into(),
            region: Region::UsEast,
            player_count: 30,
            max_players: 100,
            load: 0.3,
            accepting: true,
        });

        router.register_server(ServerInstance {
            server_id: "server-2".into(),
            experience_id: "my-game".into(),
            region: Region::UsEast,
            player_count: 70,
            max_players: 100,
            load: 0.7,
            accepting: true,
        });

        // Should find server-2 (more full but under target)
        let found = router.find_server("my-game", Some(Region::UsEast));
        assert!(found.is_some());
        
        let servers = router.get_servers("my-game");
        assert_eq!(servers.len(), 2);
    }

    #[test]
    fn test_should_spawn() {
        let router = ExperienceRouter::new().with_target_fill(0.8);

        // No servers - should spawn
        assert!(router.should_spawn_server("new-game"));

        // Add a server at 50% - should not spawn
        router.register_server(ServerInstance {
            server_id: "server-1".into(),
            experience_id: "test-game".into(),
            region: Region::UsEast,
            player_count: 50,
            max_players: 100,
            load: 0.5,
            accepting: true,
        });
        assert!(!router.should_spawn_server("test-game"));

        // Update to 90% - should spawn
        router.update_server("server-1", 90, 0.9, true);
        assert!(router.should_spawn_server("test-game"));
    }
}

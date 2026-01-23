//! Game server job definitions for Eustress Forge.
//!
//! Extends `forge_orchestration::Job` with game-server-specific configuration.
//! Uses forge-orchestration 0.4.0's UDP/game port allocation for QUIC networking.

use crate::config::Region;
use forge_orchestration::{Driver, Job, Task, TaskGroup};
use forge_orchestration::job::{HealthCheck, NetworkConfig, Resources};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Game server job builder.
///
/// Creates Nomad jobs specifically configured for Eustress game servers
/// with QUIC networking, health checks, and proper resource allocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameServerJob {
    /// Experience ID
    pub experience_id: String,
    /// Target region
    pub region: Region,
    /// Maximum players
    pub max_players: u32,
    /// Server version
    pub version: Option<String>,
    /// CPU in MHz
    pub cpu: u32,
    /// Memory in MB
    pub memory: u32,
    /// QUIC port
    pub quic_port: u16,
    /// HTTP health port
    pub health_port: u16,
    /// Artifact URL for game server binary
    pub artifact_url: Option<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
}

impl GameServerJob {
    /// Create a new game server job.
    pub fn new(experience_id: impl Into<String>, region: Region, max_players: u32) -> Self {
        Self {
            experience_id: experience_id.into(),
            region,
            max_players,
            version: None,
            cpu: 2000,      // 2 CPU cores
            memory: 4096,   // 4 GB RAM
            quic_port: 4433,
            health_port: 8080,
            artifact_url: None,
            env: HashMap::new(),
        }
    }

    /// Set server version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set CPU allocation in MHz.
    pub fn with_cpu(mut self, cpu: u32) -> Self {
        self.cpu = cpu;
        self
    }

    /// Set memory allocation in MB.
    pub fn with_memory(mut self, memory: u32) -> Self {
        self.memory = memory;
        self
    }

    /// Set QUIC port.
    pub fn with_quic_port(mut self, port: u16) -> Self {
        self.quic_port = port;
        self
    }

    /// Set artifact URL.
    pub fn with_artifact(mut self, url: impl Into<String>) -> Self {
        self.artifact_url = Some(url.into());
        self
    }

    /// Add environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Convert to a forge-orchestration Job.
    pub fn into_job(self) -> Job {
        let job_name = format!("eustress-gameserver-{}", &self.experience_id);
        
        // Build the game server task
        let mut task = Task::new("gameserver")
            .driver(Driver::Exec)
            .command("eustress-server")
            .args(vec![
                "--experience-id".to_string(),
                self.experience_id.clone(),
                "--max-players".to_string(),
                self.max_players.to_string(),
                "--quic-port".to_string(),
                self.quic_port.to_string(),
            ])
            .resources(self.cpu, self.memory)
            .health_check(HealthCheck::http("/health", self.health_port).interval(10).timeout(3));

        // Add artifact if specified
        if let Some(url) = &self.artifact_url {
            task = task.artifact(url);
        }

        // Add environment variables
        task = task
            .env("EXPERIENCE_ID", &self.experience_id)
            .env("MAX_PLAYERS", self.max_players.to_string())
            .env("REGION", self.region.datacenter())
            .env("QUIC_PORT", self.quic_port.to_string());

        if let Some(version) = &self.version {
            task = task.env("SERVER_VERSION", version);
        }

        for (key, value) in &self.env {
            task = task.env(key, value);
        }

        // Build task group with network config
        let group = TaskGroup::new("primary")
            .task(task)
            .scaling(1, 1)
            .network(
                NetworkConfig::host()
                    .port("quic", self.quic_port)
                    .port("health", self.health_port)
            );

        // Build job
        Job::new(job_name)
            .job_type(forge_orchestration::job::JobType::Service)
            .datacenters(vec![self.region.datacenter()])
            .group(group)
            .metadata("experience_id", &self.experience_id)
            .metadata("max_players", self.max_players.to_string())
            .metadata("region", self.region.datacenter())
    }
}

/// Physics server job builder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsServerJob {
    /// Server ID
    pub server_id: String,
    /// Target region
    pub region: Region,
    /// CPU in MHz
    pub cpu: u32,
    /// Memory in MB
    pub memory: u32,
    /// Tick rate (Hz)
    pub tick_rate: u32,
}

impl PhysicsServerJob {
    /// Create a new physics server job.
    pub fn new(server_id: impl Into<String>, region: Region) -> Self {
        Self {
            server_id: server_id.into(),
            region,
            cpu: 4000,      // 4 CPU cores
            memory: 8192,   // 8 GB RAM
            tick_rate: 120, // 120 Hz
        }
    }

    /// Convert to a forge-orchestration Job.
    pub fn into_job(self) -> Job {
        let job_name = format!("eustress-physics-{}", &self.server_id);

        let task = Task::new("physics")
            .driver(Driver::Exec)
            .command("eustress-physics-server")
            .args(vec![
                "--tick-rate".to_string(),
                self.tick_rate.to_string(),
            ])
            .resources(self.cpu, self.memory)
            .env("SERVER_ID", &self.server_id)
            .env("TICK_RATE", self.tick_rate.to_string());

        let group = TaskGroup::new("physics")
            .task(task)
            .scaling(1, 1);

        Job::new(job_name)
            .job_type(forge_orchestration::job::JobType::Service)
            .datacenters(vec![self.region.datacenter()])
            .group(group)
    }
}

/// AI inference server job builder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIServerJob {
    /// Server ID
    pub server_id: String,
    /// Target region
    pub region: Region,
    /// CPU in MHz
    pub cpu: u32,
    /// Memory in MB
    pub memory: u32,
    /// GPU count
    pub gpu: Option<u32>,
    /// Model name
    pub model: String,
}

impl AIServerJob {
    /// Create a new AI server job.
    pub fn new(server_id: impl Into<String>, region: Region, model: impl Into<String>) -> Self {
        Self {
            server_id: server_id.into(),
            region,
            cpu: 2000,
            memory: 16384,  // 16 GB RAM
            gpu: Some(1),
            model: model.into(),
        }
    }

    /// Convert to a forge-orchestration Job.
    pub fn into_job(self) -> Job {
        let job_name = format!("eustress-ai-{}", &self.server_id);

        let mut resources = Resources::new(self.cpu, self.memory);
        if let Some(gpu) = self.gpu {
            resources = resources.with_gpu(gpu);
        }

        let task = Task::new("ai-inference")
            .driver(Driver::Exec)
            .command("eustress-ai-server")
            .with_resources(resources)
            .env("SERVER_ID", &self.server_id)
            .env("MODEL", &self.model);

        let group = TaskGroup::new("ai")
            .task(task)
            .scaling(1, 1);

        Job::new(job_name)
            .job_type(forge_orchestration::job::JobType::Service)
            .datacenters(vec![self.region.datacenter()])
            .group(group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_game_server_job() {
        let job = GameServerJob::new("test-experience", Region::UsEast, 100)
            .with_version("1.0.0")
            .with_cpu(4000)
            .with_memory(8192)
            .into_job();

        assert!(job.name.contains("test-experience"));
        assert_eq!(job.datacenters[0], "us-east-1");
    }

    #[test]
    fn test_physics_server_job() {
        let job = PhysicsServerJob::new("physics-001", Region::EuWest)
            .into_job();

        assert!(job.name.contains("physics-001"));
    }

    #[test]
    fn test_ai_server_job() {
        let job = AIServerJob::new("ai-001", Region::UsWest, "npc-behavior-v1")
            .into_job();

        assert!(job.name.contains("ai-001"));
    }
}

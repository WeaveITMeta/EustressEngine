//! Job and Task definitions for Forge orchestration
//!
//! ## Table of Contents
//! - **Job**: Top-level workload definition (similar to K8s Deployment)
//! - **TaskGroup**: Group of related tasks with shared lifecycle
//! - **Task**: Individual executable unit
//! - **Driver**: Execution driver (Exec, Docker, etc.)
//! - **Resources**: CPU/Memory resource requirements

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Execution driver for tasks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Driver {
    /// Execute a binary directly
    Exec,
    /// Run in a Docker container
    Docker,
    /// Run in a Podman container
    Podman,
    /// Run as a raw fork/exec
    RawExec,
    /// Java application
    Java,
    /// QEMU virtual machine
    Qemu,
}

impl Default for Driver {
    fn default() -> Self {
        Self::Exec
    }
}

/// Resource requirements for a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resources {
    /// CPU in MHz
    pub cpu: u32,
    /// Memory in MB
    pub memory: u32,
    /// Disk in MB (optional)
    pub disk: Option<u32>,
    /// Network bandwidth in Mbps (optional)
    pub network: Option<u32>,
    /// GPU count (optional)
    pub gpu: Option<u32>,
}

impl Resources {
    /// Create new resource requirements
    pub fn new(cpu: u32, memory: u32) -> Self {
        Self {
            cpu,
            memory,
            disk: None,
            network: None,
            gpu: None,
        }
    }

    /// Set disk requirement
    pub fn with_disk(mut self, disk: u32) -> Self {
        self.disk = Some(disk);
        self
    }

    /// Set network requirement
    pub fn with_network(mut self, network: u32) -> Self {
        self.network = Some(network);
        self
    }

    /// Set GPU requirement
    pub fn with_gpu(mut self, gpu: u32) -> Self {
        self.gpu = Some(gpu);
        self
    }
}

impl Default for Resources {
    fn default() -> Self {
        Self::new(100, 128)
    }
}

/// Scaling configuration for a task group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingConfig {
    /// Minimum instance count
    pub min: u32,
    /// Maximum instance count
    pub max: u32,
    /// Desired instance count
    pub desired: u32,
}

impl ScalingConfig {
    /// Create new scaling config
    pub fn new(min: u32, max: u32) -> Self {
        Self {
            min,
            max,
            desired: min,
        }
    }

    /// Set desired count
    pub fn with_desired(mut self, desired: u32) -> Self {
        self.desired = desired.clamp(self.min, self.max);
        self
    }
}

impl Default for ScalingConfig {
    fn default() -> Self {
        Self::new(1, 1)
    }
}

/// Individual task within a task group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Task name
    pub name: String,
    /// Execution driver
    pub driver: Driver,
    /// Command to execute
    pub command: Option<String>,
    /// Command arguments
    pub args: Vec<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Resource requirements
    pub resources: Resources,
    /// Artifact URLs to download
    pub artifacts: Vec<String>,
    /// Health check configuration
    pub health_check: Option<HealthCheck>,
    /// Task metadata
    pub metadata: HashMap<String, String>,
}

impl Task {
    /// Create a new task with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            driver: Driver::default(),
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            resources: Resources::default(),
            artifacts: Vec::new(),
            health_check: None,
            metadata: HashMap::new(),
        }
    }

    /// Set the execution driver
    pub fn driver(mut self, driver: Driver) -> Self {
        self.driver = driver;
        self
    }

    /// Set the command to execute
    pub fn command(mut self, cmd: impl Into<String>) -> Self {
        self.command = Some(cmd.into());
        self
    }

    /// Set command arguments
    pub fn args(mut self, args: Vec<impl Into<String>>) -> Self {
        self.args = args.into_iter().map(|a| a.into()).collect();
        self
    }

    /// Add an environment variable
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set resource requirements (CPU MHz, Memory MB)
    pub fn resources(mut self, cpu: u32, memory: u32) -> Self {
        self.resources = Resources::new(cpu, memory);
        self
    }

    /// Set full resource configuration
    pub fn with_resources(mut self, resources: Resources) -> Self {
        self.resources = resources;
        self
    }

    /// Add an artifact URL
    pub fn artifact(mut self, url: impl Into<String>) -> Self {
        self.artifacts.push(url.into());
        self
    }

    /// Set health check
    pub fn health_check(mut self, check: HealthCheck) -> Self {
        self.health_check = Some(check);
        self
    }

    /// Add metadata
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Check type
    pub check_type: HealthCheckType,
    /// Check interval in seconds
    pub interval_secs: u32,
    /// Timeout in seconds
    pub timeout_secs: u32,
    /// Path for HTTP checks
    pub path: Option<String>,
    /// Port for network checks
    pub port: Option<u16>,
}

impl HealthCheck {
    /// Create an HTTP health check
    pub fn http(path: impl Into<String>, port: u16) -> Self {
        Self {
            check_type: HealthCheckType::Http,
            interval_secs: 10,
            timeout_secs: 2,
            path: Some(path.into()),
            port: Some(port),
        }
    }

    /// Create a TCP health check
    pub fn tcp(port: u16) -> Self {
        Self {
            check_type: HealthCheckType::Tcp,
            interval_secs: 10,
            timeout_secs: 2,
            path: None,
            port: Some(port),
        }
    }

    /// Create a script health check
    pub fn script() -> Self {
        Self {
            check_type: HealthCheckType::Script,
            interval_secs: 30,
            timeout_secs: 5,
            path: None,
            port: None,
        }
    }

    /// Set check interval
    pub fn interval(mut self, secs: u32) -> Self {
        self.interval_secs = secs;
        self
    }

    /// Set timeout
    pub fn timeout(mut self, secs: u32) -> Self {
        self.timeout_secs = secs;
        self
    }
}

/// Health check type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthCheckType {
    /// HTTP GET check
    Http,
    /// TCP connection check
    Tcp,
    /// Script execution check
    Script,
    /// gRPC health check
    Grpc,
}

/// Group of related tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGroup {
    /// Group name
    pub name: String,
    /// Tasks in this group
    pub tasks: Vec<Task>,
    /// Scaling configuration
    pub scaling: ScalingConfig,
    /// Restart policy
    pub restart_policy: RestartPolicy,
    /// Network configuration
    pub network: Option<NetworkConfig>,
    /// Group metadata
    pub metadata: HashMap<String, String>,
}

impl TaskGroup {
    /// Create a new task group
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tasks: Vec::new(),
            scaling: ScalingConfig::default(),
            restart_policy: RestartPolicy::default(),
            network: None,
            metadata: HashMap::new(),
        }
    }

    /// Add a task to the group
    pub fn task(mut self, task: Task) -> Self {
        self.tasks.push(task);
        self
    }

    /// Set scaling configuration
    pub fn scaling(mut self, min: u32, max: u32) -> Self {
        self.scaling = ScalingConfig::new(min, max);
        self
    }

    /// Set restart policy
    pub fn restart_policy(mut self, policy: RestartPolicy) -> Self {
        self.restart_policy = policy;
        self
    }

    /// Set network configuration
    pub fn network(mut self, config: NetworkConfig) -> Self {
        self.network = Some(config);
        self
    }
}

/// Restart policy for task groups
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartPolicy {
    /// Number of restart attempts
    pub attempts: u32,
    /// Delay between restarts in seconds
    pub delay_secs: u32,
    /// Restart mode
    pub mode: RestartMode,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            attempts: 3,
            delay_secs: 15,
            mode: RestartMode::Fail,
        }
    }
}

/// Restart mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestartMode {
    /// Fail after max attempts
    Fail,
    /// Keep retrying indefinitely
    Delay,
}

/// Network configuration for task groups
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Network mode
    pub mode: NetworkMode,
    /// Port mappings
    pub ports: Vec<PortMapping>,
}

impl NetworkConfig {
    /// Create bridge network config
    pub fn bridge() -> Self {
        Self {
            mode: NetworkMode::Bridge,
            ports: Vec::new(),
        }
    }

    /// Create host network config
    pub fn host() -> Self {
        Self {
            mode: NetworkMode::Host,
            ports: Vec::new(),
        }
    }

    /// Add a port mapping
    pub fn port(mut self, label: impl Into<String>, to: u16) -> Self {
        self.ports.push(PortMapping {
            label: label.into(),
            to,
            static_port: None,
        });
        self
    }

    /// Add a static port mapping
    pub fn static_port(mut self, label: impl Into<String>, port: u16) -> Self {
        self.ports.push(PortMapping {
            label: label.into(),
            to: port,
            static_port: Some(port),
        });
        self
    }
}

/// Network mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkMode {
    /// Bridge networking
    Bridge,
    /// Host networking
    Host,
    /// No networking
    None,
}

/// Port mapping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    /// Port label
    pub label: String,
    /// Container port
    pub to: u16,
    /// Static host port (if any)
    pub static_port: Option<u16>,
}

/// Job state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    /// Job is pending submission
    Pending,
    /// Job is running
    Running,
    /// Job completed successfully
    Complete,
    /// Job failed
    Failed,
    /// Job was stopped
    Stopped,
}

/// Top-level job definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique job ID
    pub id: String,
    /// Job name
    pub name: String,
    /// Job type
    pub job_type: JobType,
    /// Task groups
    pub groups: Vec<TaskGroup>,
    /// Job priority (0-100)
    pub priority: u8,
    /// Datacenter constraints
    pub datacenters: Vec<String>,
    /// Job state
    pub state: JobState,
    /// Job metadata
    pub metadata: HashMap<String, String>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Job {
    /// Create a new job with the given name
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id: format!("{}-{}", &name, &Uuid::new_v4().to_string()[..8]),
            name,
            job_type: JobType::Service,
            groups: Vec::new(),
            priority: 50,
            datacenters: vec!["dc1".to_string()],
            state: JobState::Pending,
            metadata: HashMap::new(),
            created_at: chrono::Utc::now(),
        }
    }

    /// Set job type
    pub fn job_type(mut self, job_type: JobType) -> Self {
        self.job_type = job_type;
        self
    }

    /// Add a task group with a single task
    pub fn with_group(mut self, group_name: impl Into<String>, task: Task) -> Self {
        let group = TaskGroup::new(group_name).task(task);
        self.groups.push(group);
        self
    }

    /// Add a full task group
    pub fn group(mut self, group: TaskGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// Set job priority
    pub fn priority(mut self, priority: u8) -> Self {
        self.priority = priority.min(100);
        self
    }

    /// Set datacenters
    pub fn datacenters(mut self, dcs: Vec<impl Into<String>>) -> Self {
        self.datacenters = dcs.into_iter().map(|d| d.into()).collect();
        self
    }

    /// Add metadata
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get total task count across all groups
    pub fn task_count(&self) -> usize {
        self.groups.iter().map(|g| g.tasks.len()).sum()
    }

    /// Get total desired instance count
    pub fn desired_count(&self) -> u32 {
        self.groups.iter().map(|g| g.scaling.desired).sum()
    }
}

/// Job type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobType {
    /// Long-running service
    Service,
    /// Batch job (runs to completion)
    Batch,
    /// System job (runs on all nodes)
    System,
    /// Parameterized job (dispatch-based)
    Parameterized,
}

impl Default for JobType {
    fn default() -> Self {
        Self::Service
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_builder() {
        let job = Job::new("my-service")
            .job_type(JobType::Service)
            .priority(75)
            .with_group(
                "api",
                Task::new("server")
                    .driver(Driver::Exec)
                    .command("/usr/bin/server")
                    .args(vec!["--port", "8080"])
                    .resources(500, 256),
            );

        assert_eq!(job.name, "my-service");
        assert_eq!(job.priority, 75);
        assert_eq!(job.groups.len(), 1);
        assert_eq!(job.groups[0].tasks[0].name, "server");
    }

    #[test]
    fn test_task_group_scaling() {
        let group = TaskGroup::new("workers")
            .task(Task::new("worker"))
            .scaling(2, 10);

        assert_eq!(group.scaling.min, 2);
        assert_eq!(group.scaling.max, 10);
    }

    #[test]
    fn test_resources_builder() {
        let res = Resources::new(1000, 512).with_gpu(2).with_disk(10000);

        assert_eq!(res.cpu, 1000);
        assert_eq!(res.memory, 512);
        assert_eq!(res.gpu, Some(2));
        assert_eq!(res.disk, Some(10000));
    }
}

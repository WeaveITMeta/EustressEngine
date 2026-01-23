//! Nomad client integration for Forge
//!
//! ## Table of Contents
//! - **NomadClient**: HTTP client for Nomad API
//! - **NomadJob**: Nomad-specific job representation
//! - **Allocation**: Nomad allocation info
//! - **Node**: Nomad node info

use crate::error::{ForgeError, Result};
use crate::job::{Driver, Job, JobType, Task, TaskGroup};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::info;

/// Nomad API client
#[derive(Clone)]
pub struct NomadClient {
    client: Client,
    base_url: String,
    token: Option<String>,
    namespace: String,
    region: String,
}

impl NomadClient {
    /// Create a new Nomad client
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| ForgeError::nomad(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: None,
            namespace: "default".to_string(),
            region: "global".to_string(),
        })
    }

    /// Set ACL token
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Set namespace
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = namespace.into();
        self
    }

    /// Set region
    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = region.into();
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}/v1{}", self.base_url, path)
    }

    fn add_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => req.header("X-Nomad-Token", token),
            None => req,
        }
    }

    /// Check Nomad connectivity
    pub async fn health(&self) -> Result<bool> {
        let resp = self
            .add_auth(self.client.get(self.url("/status/leader")))
            .send()
            .await?;

        Ok(resp.status().is_success())
    }

    /// Get cluster leader
    pub async fn leader(&self) -> Result<String> {
        let resp = self
            .add_auth(self.client.get(self.url("/status/leader")))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        resp.text()
            .await
            .map_err(|e| ForgeError::nomad(e.to_string()))
    }

    /// List all jobs
    pub async fn list_jobs(&self) -> Result<Vec<JobListStub>> {
        let url = format!("{}?namespace={}", self.url("/jobs"), self.namespace);
        let resp = self
            .add_auth(self.client.get(&url))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        resp.json()
            .await
            .map_err(|e| ForgeError::nomad(e.to_string()))
    }

    /// Get job details
    pub async fn get_job(&self, job_id: &str) -> Result<NomadJob> {
        let url = format!(
            "{}?namespace={}",
            self.url(&format!("/job/{}", job_id)),
            self.namespace
        );
        let resp = self
            .add_auth(self.client.get(&url))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        resp.json()
            .await
            .map_err(|e| ForgeError::nomad(e.to_string()))
    }

    /// Submit a job
    pub async fn submit_job(&self, job: &Job) -> Result<JobSubmitResponse> {
        let nomad_job = NomadJob::from_forge_job(job);
        let payload = JobSubmitRequest {
            job: nomad_job,
            enforce_index: false,
            job_modify_index: None,
            policy_override: false,
        };

        let url = format!("{}?namespace={}", self.url("/jobs"), self.namespace);
        let resp = self
            .add_auth(self.client.post(&url))
            .json(&payload)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        let result: JobSubmitResponse = resp
            .json()
            .await
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        info!(job_id = %job.id, eval_id = %result.eval_id, "Job submitted to Nomad");
        Ok(result)
    }

    /// Stop a job
    pub async fn stop_job(&self, job_id: &str, purge: bool) -> Result<JobSubmitResponse> {
        let url = format!(
            "{}?namespace={}&purge={}",
            self.url(&format!("/job/{}", job_id)),
            self.namespace,
            purge
        );
        let resp = self
            .add_auth(self.client.delete(&url))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        let result: JobSubmitResponse = resp
            .json()
            .await
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        info!(job_id = %job_id, "Job stopped");
        Ok(result)
    }

    /// Scale a job's task group
    pub async fn scale_job(
        &self,
        job_id: &str,
        group: &str,
        count: u32,
        reason: Option<&str>,
    ) -> Result<ScaleResponse> {
        let payload = ScaleRequest {
            count: Some(count as i64),
            target: HashMap::from([("Group".to_string(), group.to_string())]),
            message: reason.map(|s| s.to_string()),
            policy_override: false,
            error: false,
            meta: None,
        };

        let url = format!(
            "{}?namespace={}",
            self.url(&format!("/job/{}/scale", job_id)),
            self.namespace
        );
        let resp = self
            .add_auth(self.client.post(&url))
            .json(&payload)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        let result: ScaleResponse = resp
            .json()
            .await
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        info!(job_id = %job_id, group = %group, count = count, "Job scaled");
        Ok(result)
    }

    /// Get job allocations
    pub async fn get_allocations(&self, job_id: &str) -> Result<Vec<AllocationListStub>> {
        let url = format!(
            "{}?namespace={}",
            self.url(&format!("/job/{}/allocations", job_id)),
            self.namespace
        );
        let resp = self
            .add_auth(self.client.get(&url))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        resp.json()
            .await
            .map_err(|e| ForgeError::nomad(e.to_string()))
    }

    /// List nodes
    pub async fn list_nodes(&self) -> Result<Vec<NodeListStub>> {
        let resp = self
            .add_auth(self.client.get(self.url("/nodes")))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        resp.json()
            .await
            .map_err(|e| ForgeError::nomad(e.to_string()))
    }

    /// Get node details
    pub async fn get_node(&self, node_id: &str) -> Result<Node> {
        let resp = self
            .add_auth(self.client.get(self.url(&format!("/node/{}", node_id))))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| ForgeError::nomad(e.to_string()))?;

        resp.json()
            .await
            .map_err(|e| ForgeError::nomad(e.to_string()))
    }
}

// Nomad API types

/// Job list stub from Nomad API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JobListStub {
    #[serde(rename = "ID")]
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub job_type: String,
    pub status: String,
    pub status_description: Option<String>,
    pub priority: i32,
}

/// Nomad job representation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NomadJob {
    #[serde(rename = "ID")]
    pub id: String,
    pub name: String,
    #[serde(rename = "Type")]
    pub job_type: String,
    pub priority: i32,
    pub datacenters: Vec<String>,
    pub task_groups: Vec<NomadTaskGroup>,
    pub namespace: Option<String>,
    pub region: Option<String>,
    pub meta: Option<HashMap<String, String>>,
}

impl NomadJob {
    /// Convert from Forge Job
    pub fn from_forge_job(job: &Job) -> Self {
        Self {
            id: job.id.clone(),
            name: job.name.clone(),
            job_type: match job.job_type {
                JobType::Service => "service".to_string(),
                JobType::Batch => "batch".to_string(),
                JobType::System => "system".to_string(),
                JobType::Parameterized => "parameterized".to_string(),
            },
            priority: job.priority as i32,
            datacenters: job.datacenters.clone(),
            task_groups: job.groups.iter().map(NomadTaskGroup::from_forge).collect(),
            namespace: None,
            region: None,
            meta: if job.metadata.is_empty() {
                None
            } else {
                Some(job.metadata.clone())
            },
        }
    }
}

/// Nomad task group
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NomadTaskGroup {
    pub name: String,
    pub count: i32,
    pub tasks: Vec<NomadTask>,
    pub scaling: Option<NomadScaling>,
    pub restart_policy: Option<NomadRestartPolicy>,
    pub meta: Option<HashMap<String, String>>,
}

impl NomadTaskGroup {
    fn from_forge(group: &TaskGroup) -> Self {
        Self {
            name: group.name.clone(),
            count: group.scaling.desired as i32,
            tasks: group.tasks.iter().map(NomadTask::from_forge).collect(),
            scaling: Some(NomadScaling {
                min: group.scaling.min as i64,
                max: group.scaling.max as i64,
                enabled: true,
                policy: None,
            }),
            restart_policy: Some(NomadRestartPolicy {
                attempts: group.restart_policy.attempts as i32,
                delay: group.restart_policy.delay_secs as i64 * 1_000_000_000,
                mode: match group.restart_policy.mode {
                    crate::job::RestartMode::Fail => "fail".to_string(),
                    crate::job::RestartMode::Delay => "delay".to_string(),
                },
                interval: 1800_000_000_000, // 30 minutes in nanoseconds
            }),
            meta: if group.metadata.is_empty() {
                None
            } else {
                Some(group.metadata.clone())
            },
        }
    }
}

/// Nomad task
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NomadTask {
    pub name: String,
    pub driver: String,
    pub config: HashMap<String, serde_json::Value>,
    pub resources: NomadResources,
    pub env: Option<HashMap<String, String>>,
    pub meta: Option<HashMap<String, String>>,
}

impl NomadTask {
    fn from_forge(task: &Task) -> Self {
        let mut config = HashMap::new();

        if let Some(cmd) = &task.command {
            config.insert("command".to_string(), serde_json::json!(cmd));
        }
        if !task.args.is_empty() {
            config.insert("args".to_string(), serde_json::json!(task.args));
        }

        Self {
            name: task.name.clone(),
            driver: match task.driver {
                Driver::Exec => "exec".to_string(),
                Driver::Docker => "docker".to_string(),
                Driver::Podman => "podman".to_string(),
                Driver::RawExec => "raw_exec".to_string(),
                Driver::Java => "java".to_string(),
                Driver::Qemu => "qemu".to_string(),
            },
            config,
            resources: NomadResources {
                cpu: task.resources.cpu as i32,
                memory_mb: task.resources.memory as i32,
                disk_mb: task.resources.disk.map(|d| d as i32),
            },
            env: if task.env.is_empty() {
                None
            } else {
                Some(task.env.clone())
            },
            meta: if task.metadata.is_empty() {
                None
            } else {
                Some(task.metadata.clone())
            },
        }
    }
}

/// Nomad resources
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NomadResources {
    #[serde(rename = "CPU")]
    pub cpu: i32,
    #[serde(rename = "MemoryMB")]
    pub memory_mb: i32,
    #[serde(rename = "DiskMB")]
    pub disk_mb: Option<i32>,
}

/// Nomad scaling config
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NomadScaling {
    pub min: i64,
    pub max: i64,
    pub enabled: bool,
    pub policy: Option<HashMap<String, serde_json::Value>>,
}

/// Nomad restart policy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NomadRestartPolicy {
    pub attempts: i32,
    pub delay: i64,
    pub mode: String,
    pub interval: i64,
}

/// Job submit request
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct JobSubmitRequest {
    job: NomadJob,
    enforce_index: bool,
    job_modify_index: Option<u64>,
    policy_override: bool,
}

/// Job submit response
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct JobSubmitResponse {
    #[serde(rename = "EvalID")]
    pub eval_id: String,
    pub eval_create_index: Option<u64>,
    pub job_modify_index: Option<u64>,
    pub warnings: Option<String>,
}

/// Scale request
#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ScaleRequest {
    count: Option<i64>,
    target: HashMap<String, String>,
    message: Option<String>,
    policy_override: bool,
    error: bool,
    meta: Option<HashMap<String, String>>,
}

/// Scale response
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ScaleResponse {
    #[serde(rename = "EvalID")]
    pub eval_id: Option<String>,
    pub eval_create_index: Option<u64>,
}

/// Allocation list stub
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AllocationListStub {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "JobID")]
    pub job_id: String,
    #[serde(rename = "NodeID")]
    pub node_id: String,
    pub task_group: String,
    pub client_status: String,
    pub desired_status: String,
}

/// Node list stub
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NodeListStub {
    #[serde(rename = "ID")]
    pub id: String,
    pub name: String,
    pub status: String,
    pub status_description: Option<String>,
    pub datacenter: String,
    pub node_class: Option<String>,
    pub drain: bool,
    pub schedulability_eligibility: Option<String>,
}

/// Node details
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Node {
    #[serde(rename = "ID")]
    pub id: String,
    pub name: String,
    pub datacenter: String,
    pub status: String,
    pub drain: bool,
    pub attributes: Option<HashMap<String, String>>,
    pub resources: Option<NodeResources>,
    pub reserved: Option<NodeResources>,
}

/// Node resources
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NodeResources {
    #[serde(rename = "CPU")]
    pub cpu: Option<i32>,
    #[serde(rename = "MemoryMB")]
    pub memory_mb: Option<i32>,
    #[serde(rename = "DiskMB")]
    pub disk_mb: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nomad_job_conversion() {
        let job = Job::new("test-job")
            .job_type(JobType::Service)
            .with_group(
                "api",
                Task::new("server")
                    .driver(Driver::Exec)
                    .command("/bin/server")
                    .resources(500, 256),
            );

        let nomad_job = NomadJob::from_forge_job(&job);

        assert_eq!(nomad_job.name, "test-job");
        assert_eq!(nomad_job.job_type, "service");
        assert_eq!(nomad_job.task_groups.len(), 1);
        assert_eq!(nomad_job.task_groups[0].name, "api");
        assert_eq!(nomad_job.task_groups[0].tasks[0].driver, "exec");
    }
}

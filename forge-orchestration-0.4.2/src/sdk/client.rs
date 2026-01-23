//! Forge API client for SDK

use super::{forge_api_url, SdkError, SdkResult, FORGE_API_ENV};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info};

/// Job information from Forge API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    /// Job ID
    pub id: String,
    /// Job name
    pub name: String,
    /// Job status
    pub status: String,
    /// Task groups
    pub groups: Vec<GroupInfo>,
}

/// Task group information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupInfo {
    /// Group name
    pub name: String,
    /// Current instance count
    pub count: u32,
    /// Desired instance count
    pub desired: u32,
}

/// Allocation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocationInfo {
    /// Allocation ID
    pub id: String,
    /// Job ID
    pub job_id: String,
    /// Task group
    pub task_group: String,
    /// Node ID
    pub node_id: String,
    /// Status
    pub status: String,
}

/// Forge API client
#[derive(Clone)]
pub struct ForgeClient {
    client: Client,
    base_url: String,
}

impl ForgeClient {
    /// Create a new client from environment
    pub fn from_env() -> SdkResult<Self> {
        let base_url = forge_api_url().ok_or(SdkError::NotConfigured(FORGE_API_ENV))?;
        Self::new(base_url)
    }

    /// Create a new client with explicit URL
    pub fn new(base_url: impl Into<String>) -> SdkResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| SdkError::api(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Check API health
    pub async fn health(&self) -> SdkResult<bool> {
        let resp = self.client.get(self.url("/health")).send().await?;
        Ok(resp.status().is_success())
    }

    /// List all jobs
    pub async fn list_jobs(&self) -> SdkResult<Vec<JobInfo>> {
        let resp = self
            .client
            .get(self.url("/api/v1/jobs"))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| SdkError::api(e.to_string()))?;

        resp.json()
            .await
            .map_err(|e| SdkError::api(e.to_string()))
    }

    /// Get job by ID
    pub async fn get_job(&self, job_id: &str) -> SdkResult<JobInfo> {
        let resp = self
            .client
            .get(self.url(&format!("/api/v1/jobs/{}", job_id)))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| SdkError::api(e.to_string()))?;

        resp.json()
            .await
            .map_err(|e| SdkError::api(e.to_string()))
    }

    /// Get allocations for a job
    pub async fn get_allocations(&self, job_id: &str) -> SdkResult<Vec<AllocationInfo>> {
        let resp = self
            .client
            .get(self.url(&format!("/api/v1/jobs/{}/allocations", job_id)))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| SdkError::api(e.to_string()))?;

        resp.json()
            .await
            .map_err(|e| SdkError::api(e.to_string()))
    }

    /// Report metrics to Forge
    pub async fn report_metrics(&self, metrics: &MetricsReport) -> SdkResult<()> {
        self.client
            .post(self.url("/api/v1/metrics"))
            .json(metrics)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| SdkError::api(e.to_string()))?;

        debug!("Metrics reported");
        Ok(())
    }

    /// Send a heartbeat
    pub async fn heartbeat(&self, alloc_id: &str, task: &str) -> SdkResult<()> {
        let payload = serde_json::json!({
            "alloc_id": alloc_id,
            "task": task,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        self.client
            .post(self.url("/api/v1/heartbeat"))
            .json(&payload)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| SdkError::api(e.to_string()))?;

        debug!(alloc_id = %alloc_id, "Heartbeat sent");
        Ok(())
    }
}

/// Metrics report payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsReport {
    /// Allocation ID
    pub alloc_id: String,
    /// Task name
    pub task: String,
    /// CPU utilization (0.0 - 1.0)
    pub cpu: f64,
    /// Memory utilization (0.0 - 1.0)
    pub memory: f64,
    /// Custom metrics
    #[serde(default)]
    pub custom: HashMap<String, f64>,
}

impl MetricsReport {
    /// Create a new metrics report
    pub fn new(alloc_id: impl Into<String>, task: impl Into<String>) -> Self {
        Self {
            alloc_id: alloc_id.into(),
            task: task.into(),
            cpu: 0.0,
            memory: 0.0,
            custom: HashMap::new(),
        }
    }

    /// Set CPU utilization
    pub fn cpu(mut self, cpu: f64) -> Self {
        self.cpu = cpu.clamp(0.0, 1.0);
        self
    }

    /// Set memory utilization
    pub fn memory(mut self, memory: f64) -> Self {
        self.memory = memory.clamp(0.0, 1.0);
        self
    }

    /// Add a custom metric
    pub fn metric(mut self, name: impl Into<String>, value: f64) -> Self {
        self.custom.insert(name.into(), value);
        self
    }
}

/// Start a background heartbeat task
pub fn start_heartbeat(client: ForgeClient, alloc_id: String, task: String, interval_secs: u64) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

        loop {
            interval.tick().await;

            if let Err(e) = client.heartbeat(&alloc_id, &task).await {
                tracing::warn!(error = %e, "Heartbeat failed");
            }
        }
    });

    info!(interval = interval_secs, "Heartbeat task started");
}

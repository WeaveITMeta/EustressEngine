//! Forge runtime and control plane
//!
//! ## Table of Contents
//! - **Forge**: Main runtime struct
//! - **ForgeHandle**: Handle for interacting with running Forge

use crate::autoscaler::{Autoscaler, MetricsSnapshot, ScalingDecision};
use crate::builder::ForgeConfig;
use crate::error::Result;
use crate::job::Job;
use crate::metrics::ForgeMetrics;
use crate::moe::{BoxedMoERouter, RouteResult};
use crate::networking::{HttpServer, HttpState};
use crate::nomad::NomadClient;
use crate::storage::{keys, store_get_json, store_set_json, BoxedStateStore};
use crate::types::{Expert, NodeId, Shard, ShardId};
use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info, warn};

/// Runtime state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    /// Not started
    Stopped,
    /// Starting up
    Starting,
    /// Running normally
    Running,
    /// Shutting down
    ShuttingDown,
}

/// Main Forge runtime
pub struct Forge {
    config: ForgeConfig,
    state: Arc<RwLock<RuntimeState>>,
    node_id: NodeId,
    start_time: Option<Instant>,

    // Core components
    nomad: Option<NomadClient>,
    store: BoxedStateStore,
    router: BoxedMoERouter,
    autoscaler: Autoscaler,
    metrics: Option<Arc<ForgeMetrics>>,

    // Runtime state
    jobs: DashMap<String, Job>,
    experts: DashMap<usize, Expert>,
    shards: DashMap<ShardId, Shard>,

    // Shutdown signal
    shutdown_tx: broadcast::Sender<()>,
}

impl Forge {
    /// Create a new Forge instance (use ForgeBuilder instead)
    pub(crate) fn new(
        config: ForgeConfig,
        nomad: Option<NomadClient>,
        store: BoxedStateStore,
        router: BoxedMoERouter,
        autoscaler: Autoscaler,
        metrics: Option<Arc<ForgeMetrics>>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            config,
            state: Arc::new(RwLock::new(RuntimeState::Stopped)),
            node_id: NodeId::new(),
            start_time: None,
            nomad,
            store,
            router,
            autoscaler,
            metrics,
            jobs: DashMap::new(),
            experts: DashMap::new(),
            shards: DashMap::new(),
            shutdown_tx,
        }
    }

    /// Get the node ID
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Get current runtime state
    pub async fn state(&self) -> RuntimeState {
        *self.state.read().await
    }

    /// Get uptime in seconds
    pub fn uptime_secs(&self) -> u64 {
        self.start_time
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0)
    }

    /// Get metrics instance
    pub fn metrics(&self) -> Option<&Arc<ForgeMetrics>> {
        self.metrics.as_ref()
    }

    /// Get the state store
    pub fn store(&self) -> &BoxedStateStore {
        &self.store
    }

    /// Get the MoE router
    pub fn router(&self) -> &BoxedMoERouter {
        &self.router
    }

    /// Check if Nomad is configured
    pub fn has_nomad(&self) -> bool {
        self.nomad.is_some()
    }

    /// Run the Forge control plane
    pub async fn run(mut self) -> Result<()> {
        {
            let mut state = self.state.write().await;
            *state = RuntimeState::Starting;
        }

        info!(
            node_id = %self.node_id,
            node_name = %self.config.node_name,
            "Starting Forge control plane"
        );

        self.start_time = Some(Instant::now());

        // Verify Nomad connectivity if configured
        if let Some(nomad) = &self.nomad {
            match nomad.health().await {
                Ok(true) => info!("Nomad connection verified"),
                Ok(false) => warn!("Nomad returned unhealthy status"),
                Err(e) => warn!(error = %e, "Failed to connect to Nomad"),
            }
        }

        // Load existing state from store
        self.load_state().await?;

        {
            let mut state = self.state.write().await;
            *state = RuntimeState::Running;
        }

        info!("Forge control plane running");

        // Create shared state for HTTP handlers
        let forge_state = Arc::new(RwLock::new(ForgeHttpState {
            jobs: self.jobs.clone(),
            metrics: self.metrics.clone(),
        }));

        // Build HTTP router
        let http_router = self.build_http_router(forge_state);

        // Start HTTP server
        let http_server = HttpServer::new(self.config.http_config.clone())
            .with_router(http_router);

        // Run until shutdown
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::select! {
            result = http_server.serve() => {
                if let Err(e) = result {
                    error!(error = %e, "HTTP server error");
                }
            }
            _ = shutdown_rx.recv() => {
                info!("Shutdown signal received");
            }
        }

        self.shutdown().await?;

        Ok(())
    }

    /// Shutdown the control plane
    pub async fn shutdown(&self) -> Result<()> {
        {
            let mut state = self.state.write().await;
            if *state == RuntimeState::Stopped {
                return Ok(());
            }
            *state = RuntimeState::ShuttingDown;
        }

        info!("Shutting down Forge control plane");

        // Save state
        self.save_state().await?;

        // Send shutdown signal
        let _ = self.shutdown_tx.send(());

        {
            let mut state = self.state.write().await;
            *state = RuntimeState::Stopped;
        }

        info!("Forge control plane stopped");
        Ok(())
    }

    /// Signal shutdown
    pub fn signal_shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Subscribe to shutdown signal
    pub fn shutdown_receiver(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    // State management

    async fn load_state(&self) -> Result<()> {
        // Load jobs from store
        let job_keys = self.store.list_prefix(keys::JOBS).await?;
        for key in job_keys {
            if let Some(job) = store_get_json::<Job>(self.store.as_ref(), &key).await? {
                self.jobs.insert(job.id.clone(), job);
            }
        }

        info!(jobs = self.jobs.len(), "Loaded state from store");
        Ok(())
    }

    async fn save_state(&self) -> Result<()> {
        // Save all jobs
        for entry in self.jobs.iter() {
            let key = keys::job(&entry.key());
            store_set_json(self.store.as_ref(), &key, entry.value()).await?;
        }

        info!(jobs = self.jobs.len(), "Saved state to store");
        Ok(())
    }

    // Job management

    /// Submit a job
    pub async fn submit_job(&self, job: Job) -> Result<String> {
        let job_id = job.id.clone();

        // Submit to Nomad if configured
        if let Some(nomad) = &self.nomad {
            nomad.submit_job(&job).await?;
        }

        // Store locally
        let key = keys::job(&job_id);
        store_set_json(self.store.as_ref(), &key, &job).await?;
        self.jobs.insert(job_id.clone(), job);

        if let Some(metrics) = &self.metrics {
            metrics.record_job_submitted();
        }

        info!(job_id = %job_id, "Job submitted");
        Ok(job_id)
    }

    /// Get a job by ID
    pub fn get_job(&self, job_id: &str) -> Option<Job> {
        self.jobs.get(job_id).map(|e| e.value().clone())
    }

    /// List all jobs
    pub fn list_jobs(&self) -> Vec<Job> {
        self.jobs.iter().map(|e| e.value().clone()).collect()
    }

    /// Stop a job
    pub async fn stop_job(&self, job_id: &str, purge: bool) -> Result<()> {
        // Stop in Nomad if configured
        if let Some(nomad) = &self.nomad {
            nomad.stop_job(job_id, purge).await?;
        }

        // Remove locally
        if purge {
            let key = keys::job(job_id);
            self.store.delete(&key).await?;
            self.jobs.remove(job_id);
        }

        if let Some(metrics) = &self.metrics {
            metrics.record_job_completed(true);
        }

        info!(job_id = %job_id, purge = purge, "Job stopped");
        Ok(())
    }

    /// Scale a job's task group
    pub async fn scale_job(&self, job_id: &str, group: &str, count: u32) -> Result<()> {
        // Scale in Nomad if configured
        if let Some(nomad) = &self.nomad {
            nomad
                .scale_job(job_id, group, count, Some("Manual scale"))
                .await?;
        }

        // Update local state
        if let Some(mut job) = self.jobs.get_mut(job_id) {
            for g in &mut job.groups {
                if g.name == group {
                    g.scaling.desired = count;
                    break;
                }
            }
        }

        if let Some(metrics) = &self.metrics {
            let direction = "manual";
            metrics.record_scale_event(job_id, direction);
            metrics.set_instances(job_id, group, count as f64);
        }

        info!(job_id = %job_id, group = %group, count = count, "Job scaled");
        Ok(())
    }

    // MoE routing

    /// Route an input to an expert
    pub async fn route(&self, input: &str) -> RouteResult {
        let experts: Vec<Expert> = self.experts.iter().map(|e| e.value().clone()).collect();

        let result = if experts.is_empty() {
            self.router.route(input, 8).await
        } else {
            self.router.route_with_experts(input, &experts).await
        };

        if let Some(metrics) = &self.metrics {
            metrics.record_route(self.router.name(), result.expert_index, 0.001);
        }

        result
    }

    /// Register an expert
    pub fn register_expert(&self, expert: Expert) {
        info!(index = expert.index, node = %expert.node, "Expert registered");
        self.experts.insert(expert.index, expert);
    }

    /// Update expert load
    pub fn update_expert_load(&self, index: usize, load: f64) {
        if let Some(mut expert) = self.experts.get_mut(&index) {
            expert.update_load(load);
        }
    }

    // Autoscaling

    /// Evaluate autoscaling for a job
    pub async fn evaluate_scaling(
        &self,
        job_id: &str,
        cpu: f64,
        memory: f64,
        instances: u32,
    ) -> ScalingDecision {
        let metrics = MetricsSnapshot::new(cpu, memory, instances);
        let decision = self.autoscaler.evaluate(job_id, metrics).await;

        if decision.is_scaling() {
            if let Some(m) = &self.metrics {
                let direction = match &decision {
                    ScalingDecision::ScaleUp(_) => "up",
                    ScalingDecision::ScaleDown(_) => "down",
                    _ => "none",
                };
                m.record_scale_event(job_id, direction);
            }
        }

        decision
    }

    // HTTP router

    fn build_http_router(&self, state: Arc<RwLock<ForgeHttpState>>) -> Router {
        let state = HttpState { app: state };

        Router::new()
            .route("/health", get(health_handler))
            .route("/ready", get(ready_handler))
            .route("/api/v1/jobs", get(list_jobs_handler))
            .route("/api/v1/jobs", post(submit_job_handler))
            .route("/api/v1/jobs/:id", get(get_job_handler))
            .route("/api/v1/jobs/:id", delete(stop_job_handler))
            .route("/metrics", get(metrics_handler))
            .with_state(state)
    }
}

// HTTP state for handlers
struct ForgeHttpState {
    jobs: DashMap<String, Job>,
    metrics: Option<Arc<ForgeMetrics>>,
}

// HTTP handlers

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn ready_handler() -> axum::http::StatusCode {
    axum::http::StatusCode::OK
}

async fn list_jobs_handler(
    State(state): State<HttpState<ForgeHttpState>>,
) -> Json<Vec<Job>> {
    let app = state.app.read().await;
    let jobs: Vec<Job> = app.jobs.iter().map(|e| e.value().clone()).collect();
    Json(jobs)
}

async fn get_job_handler(
    State(state): State<HttpState<ForgeHttpState>>,
    Path(id): Path<String>,
) -> std::result::Result<Json<Job>, axum::http::StatusCode> {
    let app = state.app.read().await;
    app.jobs
        .get(&id)
        .map(|e| Json(e.value().clone()))
        .ok_or(axum::http::StatusCode::NOT_FOUND)
}

async fn submit_job_handler(
    State(state): State<HttpState<ForgeHttpState>>,
    Json(job): Json<Job>,
) -> std::result::Result<Json<serde_json::Value>, axum::http::StatusCode> {
    let app = state.app.read().await;
    let job_id = job.id.clone();
    app.jobs.insert(job_id.clone(), job);
    Ok(Json(serde_json::json!({ "job_id": job_id })))
}

async fn stop_job_handler(
    State(state): State<HttpState<ForgeHttpState>>,
    Path(id): Path<String>,
) -> axum::http::StatusCode {
    let app = state.app.read().await;
    if app.jobs.remove(&id).is_some() {
        axum::http::StatusCode::NO_CONTENT
    } else {
        axum::http::StatusCode::NOT_FOUND
    }
}

async fn metrics_handler(
    State(state): State<HttpState<ForgeHttpState>>,
) -> std::result::Result<String, axum::http::StatusCode> {
    let app = state.app.read().await;
    match &app.metrics {
        Some(m) => m
            .gather_text()
            .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR),
        None => Err(axum::http::StatusCode::NOT_FOUND),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::ForgeBuilder;
    use crate::job::{Driver, Task};

    #[tokio::test]
    async fn test_forge_creation() {
        let forge = ForgeBuilder::new().build().unwrap();
        assert_eq!(forge.state().await, RuntimeState::Stopped);
    }

    #[tokio::test]
    async fn test_job_management() {
        let forge = ForgeBuilder::new().build().unwrap();

        let job = Job::new("test-job").with_group(
            "api",
            Task::new("server")
                .driver(Driver::Exec)
                .command("/bin/server"),
        );

        let job_id = forge.submit_job(job).await.unwrap();
        assert!(forge.get_job(&job_id).is_some());

        let jobs = forge.list_jobs();
        assert_eq!(jobs.len(), 1);

        forge.stop_job(&job_id, true).await.unwrap();
        assert!(forge.get_job(&job_id).is_none());
    }

    #[tokio::test]
    async fn test_routing() {
        let forge = ForgeBuilder::new().build().unwrap();

        let result = forge.route("test-input").await;
        assert!(result.expert_index < 8);
    }

    #[tokio::test]
    async fn test_expert_registration() {
        let forge = ForgeBuilder::new().build().unwrap();

        let expert = Expert::new(0, NodeId::new());
        forge.register_expert(expert);

        forge.update_expert_load(0, 0.5);
    }
}

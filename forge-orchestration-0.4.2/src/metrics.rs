//! Metrics and monitoring for Forge
//!
//! ## Table of Contents
//! - **MetricsExporter**: Prometheus metrics exporter (requires `metrics` feature)
//! - **MetricsHook**: Custom metrics callback trait
//! - **MetricsRegistry**: Central metrics registry

use crate::error::{ForgeError, Result};
use prometheus::{
    Counter, CounterVec, Gauge, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry,
};
use std::sync::Arc;
use tracing::info;

/// Core metrics for Forge
pub struct ForgeMetrics {
    registry: Registry,

    // Job metrics
    pub jobs_submitted: Counter,
    pub jobs_running: Gauge,
    pub jobs_completed: CounterVec,

    // Scaling metrics
    pub scale_events: CounterVec,
    pub current_instances: GaugeVec,

    // Routing metrics
    pub route_requests: CounterVec,
    pub route_latency: HistogramVec,

    // Resource metrics
    pub cpu_utilization: GaugeVec,
    pub memory_utilization: GaugeVec,

    // Network metrics
    pub requests_total: CounterVec,
    pub request_duration: HistogramVec,
    pub active_connections: Gauge,
}

impl ForgeMetrics {
    /// Create a new metrics instance
    pub fn new() -> Result<Self> {
        let registry = Registry::new();

        // Job metrics
        let jobs_submitted = Counter::new("forge_jobs_submitted_total", "Total jobs submitted")?;
        let jobs_running = Gauge::new("forge_jobs_running", "Currently running jobs")?;
        let jobs_completed = CounterVec::new(
            Opts::new("forge_jobs_completed_total", "Total jobs completed"),
            &["status"],
        )?;

        // Scaling metrics
        let scale_events = CounterVec::new(
            Opts::new("forge_scale_events_total", "Total scaling events"),
            &["job", "direction"],
        )?;
        let current_instances = GaugeVec::new(
            Opts::new("forge_instances", "Current instance count"),
            &["job", "group"],
        )?;

        // Routing metrics
        let route_requests = CounterVec::new(
            Opts::new("forge_route_requests_total", "Total routing requests"),
            &["router", "expert"],
        )?;
        let route_latency = HistogramVec::new(
            HistogramOpts::new("forge_route_latency_seconds", "Routing latency")
                .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]),
            &["router"],
        )?;

        // Resource metrics
        let cpu_utilization = GaugeVec::new(
            Opts::new("forge_cpu_utilization", "CPU utilization (0-1)"),
            &["job", "group"],
        )?;
        let memory_utilization = GaugeVec::new(
            Opts::new("forge_memory_utilization", "Memory utilization (0-1)"),
            &["job", "group"],
        )?;

        // Network metrics
        let requests_total = CounterVec::new(
            Opts::new("forge_http_requests_total", "Total HTTP requests"),
            &["method", "path", "status"],
        )?;
        let request_duration = HistogramVec::new(
            HistogramOpts::new("forge_http_request_duration_seconds", "HTTP request duration")
                .buckets(vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
            &["method", "path"],
        )?;
        let active_connections = Gauge::new("forge_active_connections", "Active connections")?;

        // Register all metrics
        registry.register(Box::new(jobs_submitted.clone()))?;
        registry.register(Box::new(jobs_running.clone()))?;
        registry.register(Box::new(jobs_completed.clone()))?;
        registry.register(Box::new(scale_events.clone()))?;
        registry.register(Box::new(current_instances.clone()))?;
        registry.register(Box::new(route_requests.clone()))?;
        registry.register(Box::new(route_latency.clone()))?;
        registry.register(Box::new(cpu_utilization.clone()))?;
        registry.register(Box::new(memory_utilization.clone()))?;
        registry.register(Box::new(requests_total.clone()))?;
        registry.register(Box::new(request_duration.clone()))?;
        registry.register(Box::new(active_connections.clone()))?;

        Ok(Self {
            registry,
            jobs_submitted,
            jobs_running,
            jobs_completed,
            scale_events,
            current_instances,
            route_requests,
            route_latency,
            cpu_utilization,
            memory_utilization,
            requests_total,
            request_duration,
            active_connections,
        })
    }

    /// Get the Prometheus registry
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Record a job submission
    pub fn record_job_submitted(&self) {
        self.jobs_submitted.inc();
        self.jobs_running.inc();
    }

    /// Record a job completion
    pub fn record_job_completed(&self, success: bool) {
        self.jobs_running.dec();
        let status = if success { "success" } else { "failed" };
        self.jobs_completed.with_label_values(&[status]).inc();
    }

    /// Record a scaling event
    pub fn record_scale_event(&self, job: &str, direction: &str) {
        self.scale_events.with_label_values(&[job, direction]).inc();
    }

    /// Update instance count
    pub fn set_instances(&self, job: &str, group: &str, count: f64) {
        self.current_instances
            .with_label_values(&[job, group])
            .set(count);
    }

    /// Record a routing request
    pub fn record_route(&self, router: &str, expert: usize, latency_secs: f64) {
        let expert_str = expert.to_string();
        self.route_requests
            .with_label_values(&[router, &expert_str])
            .inc();
        self.route_latency
            .with_label_values(&[router])
            .observe(latency_secs);
    }

    /// Update resource utilization
    pub fn set_utilization(&self, job: &str, group: &str, cpu: f64, memory: f64) {
        self.cpu_utilization
            .with_label_values(&[job, group])
            .set(cpu);
        self.memory_utilization
            .with_label_values(&[job, group])
            .set(memory);
    }

    /// Record an HTTP request
    pub fn record_http_request(&self, method: &str, path: &str, status: u16, duration_secs: f64) {
        let status_str = status.to_string();
        self.requests_total
            .with_label_values(&[method, path, &status_str])
            .inc();
        self.request_duration
            .with_label_values(&[method, path])
            .observe(duration_secs);
    }

    /// Gather all metrics as text
    pub fn gather_text(&self) -> Result<String> {
        use prometheus::Encoder;
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .map_err(|e| ForgeError::metrics(format!("Encode error: {}", e)))?;
        String::from_utf8(buffer).map_err(|e| ForgeError::metrics(format!("UTF8 error: {}", e)))
    }
}

impl Default for ForgeMetrics {
    fn default() -> Self {
        match Self::new() {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(error = %e, "Failed to create metrics, using stub");
                panic!("ForgeMetrics::default() failed: {}", e);
            }
        }
    }
}

/// Trait for custom metrics hooks
pub trait MetricsHook: Send + Sync {
    /// Called periodically to collect custom metrics
    fn collect(&self, metrics: &ForgeMetrics);

    /// Hook name for identification
    fn name(&self) -> &str;
}

/// Metrics exporter for Prometheus scraping
pub struct MetricsExporter {
    metrics: Arc<ForgeMetrics>,
    hooks: Vec<Box<dyn MetricsHook>>,
}

impl MetricsExporter {
    /// Create a new exporter
    pub fn new(metrics: Arc<ForgeMetrics>) -> Self {
        Self {
            metrics,
            hooks: Vec::new(),
        }
    }

    /// Register a custom metrics hook
    pub fn register_hook(&mut self, hook: Box<dyn MetricsHook>) {
        info!(hook = %hook.name(), "Registered metrics hook");
        self.hooks.push(hook);
    }

    /// Collect all metrics
    pub fn collect(&self) {
        for hook in &self.hooks {
            hook.collect(&self.metrics);
        }
    }

    /// Get metrics as Prometheus text format
    pub fn export(&self) -> Result<String> {
        self.collect();
        self.metrics.gather_text()
    }

    /// Create an axum handler for /metrics endpoint
    pub fn handler(
        metrics: Arc<ForgeMetrics>,
    ) -> impl Fn() -> std::pin::Pin<
        Box<dyn std::future::Future<Output = axum::response::Response> + Send>,
    > + Clone {
        move || {
            let metrics = Arc::clone(&metrics);
            Box::pin(async move {
                match metrics.gather_text() {
                    Ok(text) => axum::response::Response::builder()
                        .header("Content-Type", "text/plain; charset=utf-8")
                        .body(axum::body::Body::from(text))
                        .unwrap_or_else(|_| {
                            axum::response::Response::new(axum::body::Body::from("Internal error"))
                        }),
                    Err(e) => axum::response::Response::builder()
                        .status(500)
                        .body(axum::body::Body::from(format!("Error: {}", e)))
                        .unwrap_or_else(|_| {
                            axum::response::Response::new(axum::body::Body::from("Internal error"))
                        }),
                }
            })
        }
    }
}

/// Timer for measuring operation duration
pub struct Timer {
    start: std::time::Instant,
}

impl Timer {
    /// Start a new timer
    pub fn start() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }

    /// Get elapsed time in seconds
    pub fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// Stop and return elapsed seconds
    pub fn stop(self) -> f64 {
        self.elapsed_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = ForgeMetrics::new().unwrap();
        assert!(metrics.gather_text().is_ok());
    }

    #[test]
    fn test_job_metrics() {
        let metrics = ForgeMetrics::new().unwrap();

        metrics.record_job_submitted();
        metrics.record_job_submitted();
        metrics.record_job_completed(true);

        let text = metrics.gather_text().unwrap();
        assert!(text.contains("forge_jobs_submitted_total 2"));
        assert!(text.contains("forge_jobs_running 1"));
    }

    #[test]
    fn test_scale_metrics() {
        let metrics = ForgeMetrics::new().unwrap();

        metrics.record_scale_event("my-job", "up");
        metrics.record_scale_event("my-job", "up");
        metrics.record_scale_event("my-job", "down");

        let text = metrics.gather_text().unwrap();
        assert!(text.contains("forge_scale_events_total"));
    }

    #[test]
    fn test_timer() {
        let timer = Timer::start();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let elapsed = timer.stop();
        assert!(elapsed >= 0.01);
    }
}

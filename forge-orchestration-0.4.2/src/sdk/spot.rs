//! Spot instance interruption handling for cloud workloads
//!
//! Provides hooks for AWS, GCP, and Azure spot/preemptible instance termination notices.

use super::SdkResult;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::info;

/// Spot interruption notice with metadata
#[derive(Debug, Clone)]
pub struct SpotInterruption {
    /// Cloud provider
    pub provider: CloudProvider,
    /// Time until termination (if known)
    pub time_remaining: Option<Duration>,
    /// Instance ID (if available)
    pub instance_id: Option<String>,
    /// Action to take (terminate, stop, hibernate)
    pub action: SpotAction,
}

/// Cloud provider for spot instances
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudProvider {
    /// Amazon Web Services
    Aws,
    /// Google Cloud Platform
    Gcp,
    /// Microsoft Azure
    Azure,
    /// Generic/Unknown
    Generic,
}

/// Action the cloud provider will take
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotAction {
    /// Instance will be terminated
    Terminate,
    /// Instance will be stopped
    Stop,
    /// Instance will be hibernated
    Hibernate,
    /// Unknown action
    Unknown,
}

/// Spot interruption handler
pub struct SpotHandler {
    interrupted: Arc<AtomicBool>,
    tx: broadcast::Sender<SpotInterruption>,
    _rx: broadcast::Receiver<SpotInterruption>,
}

impl SpotHandler {
    /// Create a new spot handler
    pub fn new() -> Self {
        let (tx, rx) = broadcast::channel(16);
        Self {
            interrupted: Arc::new(AtomicBool::new(false)),
            tx,
            _rx: rx,
        }
    }

    /// Check if spot interruption has been received
    pub fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }

    /// Subscribe to interruption notifications
    pub fn subscribe(&self) -> broadcast::Receiver<SpotInterruption> {
        self.tx.subscribe()
    }

    /// Manually trigger an interruption (for testing or custom sources)
    pub fn trigger(&self, interruption: SpotInterruption) {
        self.interrupted.store(true, Ordering::SeqCst);
        let _ = self.tx.send(interruption);
    }

    /// Start monitoring AWS spot instance metadata
    pub async fn start_aws_monitor(&self) -> SdkResult<()> {
        self.start_aws_monitor_internal().await
    }

    /// Start monitoring AWS spot instance metadata (internal)
    async fn start_aws_monitor_internal(&self) -> SdkResult<()> {
        let interrupted = self.interrupted.clone();
        let tx = self.tx.clone();

        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap_or_default();

            // AWS IMDS v2 token endpoint
            let token_url = "http://169.254.169.254/latest/api/token";
            // Spot interruption endpoint
            let spot_url = "http://169.254.169.254/latest/meta-data/spot/instance-action";

            loop {
                // Get IMDS token
                let token = match client
                    .put(token_url)
                    .header("X-aws-ec2-metadata-token-ttl-seconds", "21600")
                    .send()
                    .await
                {
                    Ok(resp) => resp.text().await.unwrap_or_default(),
                    Err(_) => {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                };

                // Check for spot interruption
                match client
                    .get(spot_url)
                    .header("X-aws-ec2-metadata-token", &token)
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(body) = resp.text().await {
                            info!(body = %body, "AWS spot interruption notice received");
                            interrupted.store(true, Ordering::SeqCst);

                            let interruption = SpotInterruption {
                                provider: CloudProvider::Aws,
                                time_remaining: Some(Duration::from_secs(120)), // AWS gives 2 min
                                instance_id: None,
                                action: SpotAction::Terminate,
                            };

                            let _ = tx.send(interruption);
                            return; // Stop monitoring after interruption
                        }
                    }
                    _ => {}
                }

                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        Ok(())
    }

    /// Start monitoring GCP preemptible instance metadata
    async fn start_gcp_monitor_internal(&self) -> SdkResult<()> {
        let interrupted = self.interrupted.clone();
        let tx = self.tx.clone();

        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap_or_default();

            // GCP preemption endpoint
            let preempt_url = "http://metadata.google.internal/computeMetadata/v1/instance/preempted";

            loop {
                match client
                    .get(preempt_url)
                    .header("Metadata-Flavor", "Google")
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(body) = resp.text().await {
                            if body.trim() == "TRUE" {
                                info!("GCP preemption notice received");
                                interrupted.store(true, Ordering::SeqCst);

                                let interruption = SpotInterruption {
                                    provider: CloudProvider::Gcp,
                                    time_remaining: Some(Duration::from_secs(30)), // GCP gives 30 sec
                                    instance_id: None,
                                    action: SpotAction::Terminate,
                                };

                                let _ = tx.send(interruption);
                                return;
                            }
                        }
                    }
                    _ => {}
                }

                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        Ok(())
    }

    /// Start monitoring Azure spot instance metadata
    async fn start_azure_monitor_internal(&self) -> SdkResult<()> {
        let interrupted = self.interrupted.clone();
        let tx = self.tx.clone();

        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap_or_default();

            // Azure scheduled events endpoint
            let events_url = "http://169.254.169.254/metadata/scheduledevents?api-version=2020-07-01";

            loop {
                match client
                    .get(events_url)
                    .header("Metadata", "true")
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(body) = resp.text().await {
                            // Check for Preempt event type
                            if body.contains("\"EventType\":\"Preempt\"") {
                                info!(body = %body, "Azure spot preemption notice received");
                                interrupted.store(true, Ordering::SeqCst);

                                let interruption = SpotInterruption {
                                    provider: CloudProvider::Azure,
                                    time_remaining: Some(Duration::from_secs(30)),
                                    instance_id: None,
                                    action: SpotAction::Terminate,
                                };

                                let _ = tx.send(interruption);
                                return;
                            }
                        }
                    }
                    _ => {}
                }

                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        Ok(())
    }
}

impl Default for SpotHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Start monitoring for spot interruptions on all supported cloud providers
pub async fn start_spot_monitor() -> SdkResult<SpotHandler> {
    let handler = SpotHandler::new();

    // Try to start monitors for each provider (they'll fail gracefully if not on that cloud)
    let _ = handler.start_aws_monitor_internal().await;
    let _ = handler.start_gcp_monitor_internal().await;
    let _ = handler.start_azure_monitor_internal().await;

    info!("Spot interruption monitors started");
    Ok(handler)
}

/// Wait for a spot interruption signal
pub async fn wait_for_spot_interruption(handler: &SpotHandler) -> SpotInterruption {
    let mut rx = handler.subscribe();
    rx.recv().await.unwrap_or(SpotInterruption {
        provider: CloudProvider::Generic,
        time_remaining: None,
        instance_id: None,
        action: SpotAction::Unknown,
    })
}

/// Check if running on a spot/preemptible instance
pub async fn is_spot_instance() -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    // Try AWS
    if client
        .get("http://169.254.169.254/latest/meta-data/spot/instance-action")
        .send()
        .await
        .map(|r| r.status().as_u16() != 404)
        .unwrap_or(false)
    {
        return true;
    }

    // Try GCP
    if client
        .get("http://metadata.google.internal/computeMetadata/v1/instance/scheduling/preemptible")
        .header("Metadata-Flavor", "Google")
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spot_handler_creation() {
        let handler = SpotHandler::new();
        assert!(!handler.is_interrupted());
    }

    #[test]
    fn test_manual_trigger() {
        let handler = SpotHandler::new();
        
        handler.trigger(SpotInterruption {
            provider: CloudProvider::Aws,
            time_remaining: Some(Duration::from_secs(120)),
            instance_id: Some("i-1234567890abcdef0".to_string()),
            action: SpotAction::Terminate,
        });

        assert!(handler.is_interrupted());
    }
}

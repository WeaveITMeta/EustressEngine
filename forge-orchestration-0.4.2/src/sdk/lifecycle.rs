//! Lifecycle management for Forge workloads

use super::{forge_api_url, SdkError, SdkResult};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;
use tracing::{debug, info, warn};

static READY_SIGNALED: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_HANDLER_INSTALLED: Once = Once::new();
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Signal readiness to the Forge orchestrator
///
/// Call this once your workload is ready to receive traffic.
pub fn ready() -> SdkResult<()> {
    if READY_SIGNALED.swap(true, Ordering::SeqCst) {
        debug!("Ready already signaled, ignoring duplicate call");
        return Ok(());
    }

    if let Some(api_url) = forge_api_url() {
        let alloc_id = super::alloc_id().unwrap_or_else(|| "unknown".to_string());
        let task = super::task_name().unwrap_or_else(|| "unknown".to_string());

        let url = format!("{}/api/v1/ready", api_url);
        tokio::spawn(async move {
            let client = reqwest::Client::new();
            let payload = serde_json::json!({
                "alloc_id": alloc_id,
                "task": task,
                "status": "ready"
            });

            match client.post(&url).json(&payload).send().await {
                Ok(resp) if resp.status().is_success() => {
                    info!("Ready signal sent to Forge");
                }
                Ok(resp) => {
                    warn!(status = %resp.status(), "Ready signal failed");
                }
                Err(e) => {
                    warn!(error = %e, "Failed to send ready signal");
                }
            }
        });
    } else {
        debug!("Forge API not configured, ready signal is local only");
    }

    info!("Workload ready");
    Ok(())
}

/// Check if ready has been signaled
pub fn is_ready() -> bool {
    READY_SIGNALED.load(Ordering::SeqCst)
}

/// Handle graceful shutdown
///
/// Installs signal handlers for SIGTERM/SIGINT (Unix) or Ctrl+C (Windows).
pub fn graceful_shutdown() {
    SHUTDOWN_HANDLER_INSTALLED.call_once(|| {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};

            tokio::spawn(async {
                let mut sigterm = match signal(SignalKind::terminate()) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = %e, "Failed to install SIGTERM handler");
                        return;
                    }
                };
                let mut sigint = match signal(SignalKind::interrupt()) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = %e, "Failed to install SIGINT handler");
                        return;
                    }
                };

                tokio::select! {
                    _ = sigterm.recv() => {
                        info!("Received SIGTERM");
                    }
                    _ = sigint.recv() => {
                        info!("Received SIGINT");
                    }
                }

                SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
            });
        }

        #[cfg(windows)]
        {
            tokio::spawn(async {
                match tokio::signal::ctrl_c().await {
                    Ok(()) => {
                        info!("Received Ctrl+C");
                        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to listen for Ctrl+C");
                    }
                }
            });
        }

        info!("Graceful shutdown handlers installed");
    });
}

/// Check if shutdown has been requested
pub fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

/// Wait for shutdown signal (async)
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate()).unwrap_or_else(|e| {
            panic!("Failed to install SIGTERM handler: {}", e);
        });
        let mut sigint = signal(SignalKind::interrupt()).unwrap_or_else(|e| {
            panic!("Failed to install SIGINT handler: {}", e);
        });

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT");
            }
        }
    }

    #[cfg(windows)]
    {
        if let Err(e) = tokio::signal::ctrl_c().await {
            warn!(error = %e, "Failed to listen for Ctrl+C");
        } else {
            info!("Received Ctrl+C");
        }
    }

    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

/// Notify Forge that the task is shutting down
pub async fn notify_shutdown() -> SdkResult<()> {
    if let Some(api_url) = forge_api_url() {
        let alloc_id = super::alloc_id().unwrap_or_else(|| "unknown".to_string());
        let task = super::task_name().unwrap_or_else(|| "unknown".to_string());

        let client = reqwest::Client::new();
        let url = format!("{}/api/v1/shutdown", api_url);
        let payload = serde_json::json!({
            "alloc_id": alloc_id,
            "task": task,
            "status": "shutting_down"
        });

        client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| SdkError::api(e.to_string()))?;

        info!("Shutdown notification sent to Forge");
    }

    Ok(())
}

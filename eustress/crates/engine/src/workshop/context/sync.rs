//! Cloudflare KV sync for Workshop context (memories + global rules).
//!
//! Syncs when:
//! - User signs in (download context from Cloudflare)
//! - Memory is added/updated (upload to Cloudflare in background)
//! - Session ends (final upload)
//!
//! Requires authentication — if not signed in, sync is skipped
//! and a hint is shown in the Workshop panel.

use std::sync::{Arc, Mutex};

const API_URL: &str = "https://api.eustress.dev";

/// Sync state for background operations.
pub enum SyncState {
    Idle,
    Downloading,
    Uploading,
    Error(String),
}

/// Background sync task result (polled each frame).
pub struct SyncTask {
    pub result: Arc<Mutex<Option<Result<serde_json::Value, String>>>>,
    pub direction: SyncDirection,
}

pub enum SyncDirection {
    Download,
    Upload,
}

/// Download context from Cloudflare KV (background thread).
/// Returns the context document JSON on success.
pub fn download_context(auth_token: &str, project_id: &str) -> SyncTask {
    let url = format!("{}/api/workshop/context?project_id={}", API_URL, project_id);
    let token = auth_token.to_string();
    let result = Arc::new(Mutex::new(None));
    let result_clone = result.clone();

    std::thread::spawn(move || {
        let resp = ureq::get(&url)
            .set("Authorization", &format!("Bearer {}", token))
            .call();

        let outcome = match resp {
            Ok(r) => {
                let body: serde_json::Value = r.into_json().unwrap_or_default();
                Ok(body)
            }
            Err(ureq::Error::Status(401, _)) => {
                Err("Sign in to sync Workshop context across devices".to_string())
            }
            Err(e) => Err(format!("Sync download failed: {}", e)),
        };

        if let Ok(mut lock) = result_clone.lock() {
            *lock = Some(outcome);
        }
    });

    SyncTask { result, direction: SyncDirection::Download }
}

/// Upload context to Cloudflare KV (background thread).
pub fn upload_context(
    auth_token: &str,
    project_id: &str,
    memories: &[super::MemoryEntry],
) -> SyncTask {
    let url = format!("{}/api/workshop/context", API_URL);
    let token = auth_token.to_string();
    let body = serde_json::json!({
        "project_id": project_id,
        "memories": memories,
    });
    let result = Arc::new(Mutex::new(None));
    let result_clone = result.clone();

    std::thread::spawn(move || {
        let resp = ureq::put(&url)
            .set("Authorization", &format!("Bearer {}", token))
            .set("Content-Type", "application/json")
            .send_json(&body);

        let outcome = match resp {
            Ok(_) => Ok(serde_json::json!({ "ok": true })),
            Err(ureq::Error::Status(401, _)) => {
                Err("Sign in to sync Workshop context".to_string())
            }
            Err(e) => Err(format!("Sync upload failed: {}", e)),
        };

        if let Ok(mut lock) = result_clone.lock() {
            *lock = Some(outcome);
        }
    });

    SyncTask { result, direction: SyncDirection::Upload }
}

/// Upload a single memory entry to Cloudflare KV (fire-and-forget).
pub fn upload_single_memory(
    auth_token: &str,
    project_id: &str,
    key: &str,
    value: &str,
    category: &str,
) {
    let url = format!("{}/api/workshop/context/memory", API_URL);
    let token = auth_token.to_string();
    let body = serde_json::json!({
        "project_id": project_id,
        "key": key,
        "value": value,
        "category": category,
        "source": "user",
    });

    std::thread::spawn(move || {
        let _ = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", token))
            .set("Content-Type", "application/json")
            .send_json(&body);
    });
}

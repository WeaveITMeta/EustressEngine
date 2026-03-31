//! MCP Router — routes entity changes to export targets via EustressStream.
//!
//! ## Architecture
//!
//! ```text
//! "mcp.entity.create" topic
//!     ↳ McpRouter subscriber (AI consent check → build EEP record → publish to "mcp.exports")
//!
//! "mcp.exports" topic
//!     ↳ WebhookExportTarget subscriber  (async HTTP POST, spawns tokio task)
//!     ↳ ConsoleExportTarget subscriber  (synchronous tracing::info, instant)
//!     ↳ FileExportTarget subscriber     (async file write, spawns tokio task)
//!     ↳ Any future subscriber           (zero-copy access to the record bytes)
//! ```
//!
//! Each export target subscribes independently. A slow webhook does **not**
//! block the console logger or file writer.

use std::sync::Arc;

use bytes::Bytes;
use eustress_stream::EustressStream;

use crate::{
    protocol::*,
    server::topics,
    types::*,
};

// ─────────────────────────────────────────────────────────────────────────────
// McpRouter — stream subscriber that enforces AI consent and produces exports
// ─────────────────────────────────────────────────────────────────────────────

/// Routes entity operations to the `"mcp.exports"` topic after checking AI
/// consent. Registered as an EustressStream subscriber — no background task,
/// no polling, no channel.
pub struct McpRouter;

impl McpRouter {
    /// Register the router's subscriptions on the given stream.
    ///
    /// Subscribes to `ENTITY_CREATE`, `ENTITY_UPDATE`, and `ENTITY_DELETE`
    /// topics. On entity create, if the entity has `ai = true`, an
    /// `EepExportRecord` is built and published to the `"mcp.exports"` topic.
    pub fn register(stream: &EustressStream) {
        // Clone the stream for the subscriber closures — cheaply cloneable Arc.
        let export_stream = stream.clone();

        // ── Create subscriber ────────────────────────────────────────────────
        let _ = stream.subscribe_owned(topics::ENTITY_CREATE, move |msg| {
            let Ok(entity) = serde_json::from_slice::<EntityData>(&msg.data) else {
                tracing::warn!("McpRouter: failed to deserialize EntityData from create topic");
                return;
            };

            tracing::debug!(entity_id = %entity.id, ai = %entity.ai, "McpRouter: create");

            // Only export if AI flag is set (consent model)
            if entity.ai {
                let record = build_export_record(&entity, ChangeType::Created);
                publish_export(&export_stream, &record);
            }
        });

        // ── Update subscriber ────────────────────────────────────────────────
        let _ = stream.subscribe_owned(topics::ENTITY_UPDATE, move |msg| {
            let Ok(request) = serde_json::from_slice::<UpdateEntityRequest>(&msg.data) else {
                tracing::warn!("McpRouter: failed to deserialize UpdateEntityRequest");
                return;
            };

            tracing::debug!(entity_id = %request.entity_id, "McpRouter: update");

            // Log when AI training is being enabled
            if request.ai == Some(true) {
                tracing::info!(
                    entity_id = %request.entity_id,
                    "AI training enabled for entity via MCP update"
                );
            }
        });

        // ── Delete subscriber ────────────────────────────────────────────────
        let _ = stream.subscribe_owned(topics::ENTITY_DELETE, move |msg| {
            let Ok(request) = serde_json::from_slice::<DeleteEntityRequest>(&msg.data) else {
                tracing::warn!("McpRouter: failed to deserialize DeleteEntityRequest");
                return;
            };

            tracing::debug!(entity_id = %request.entity_id, "McpRouter: delete");
        });

        tracing::info!("McpRouter: registered on EustressStream topics");
    }
}

/// Build an EEP export record from entity data.
fn build_export_record(entity: &EntityData, _change_type: ChangeType) -> EepExportRecord {
    EepExportRecord {
        protocol_version: "eep_v1".to_string(),
        export_id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        space: EepSpaceInfo {
            id: "default".to_string(),
            name: "Default Space".to_string(),
            settings: serde_json::json!({}),
        },
        entity: EepEntityData {
            id: entity.id.clone(),
            name: entity.name.clone(),
            class: entity.class.clone(),
            transform: entity.transform.clone(),
            properties: entity.properties.clone(),
            tags: entity.tags.clone(),
            attributes: entity.attributes.iter()
                .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or_default()))
                .collect(),
            parameters: entity.parameters.clone(),
            child_count: entity.children.len() as u32,
        },
        hierarchy: vec![EepHierarchyNode {
            id: entity.id.clone(),
            name: entity.name.clone(),
            class: entity.class.clone(),
            depth: 0,
        }],
        creator: ChangeSource {
            source_type: SourceType::AiModel,
            id: "mcp_server".to_string(),
            name: "MCP Server".to_string(),
        },
        consent: EepConsent {
            ai_training: entity.ai,
            consented_at: chrono::Utc::now(),
            consented_by: "system".to_string(),
        },
    }
}

/// Publish an export record to the `"mcp.exports"` topic.
fn publish_export(stream: &EustressStream, record: &EepExportRecord) {
    match serde_json::to_vec(record) {
        Ok(json_bytes) => {
            stream.producer(topics::EXPORTS)
                .send_bytes(Bytes::from(json_bytes));
            tracing::debug!(export_id = %record.export_id, "Published EEP record to mcp.exports");
        }
        Err(e) => {
            tracing::error!("McpRouter: failed to serialize EEP record: {e}");
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Export Targets — independent stream subscribers on "mcp.exports"
// ─────────────────────────────────────────────────────────────────────────────
//
// Each target subscribes to the "mcp.exports" topic. Dispatch is synchronous
// and parallel — a slow webhook spawns an async task and returns immediately,
// so it never blocks the console logger or file writer.

/// Webhook export target — sends EEP records to HTTP endpoints.
///
/// Async HTTP POST is spawned on a tokio task so the synchronous stream
/// callback returns immediately.
pub struct WebhookExportTarget {
    name: String,
    endpoint: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl WebhookExportTarget {
    pub fn new(name: String, endpoint: String, api_key: Option<String>) -> Self {
        Self {
            name,
            endpoint,
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

/// Register a `WebhookExportTarget` as a subscriber on `"mcp.exports"`.
pub fn register_webhook_subscriber(stream: &EustressStream, target: WebhookExportTarget) {
    let target = Arc::new(target);
    let _ = stream.subscribe_owned(topics::EXPORTS, move |msg| {
        let Ok(record) = serde_json::from_slice::<EepExportRecord>(&msg.data) else {
            tracing::warn!("WebhookExportTarget: failed to deserialize EepExportRecord");
            return;
        };

        // Spawn async HTTP POST — does not block the synchronous callback
        let target = Arc::clone(&target);
        tokio::spawn(async move {
            let mut request = target.client
                .post(&target.endpoint)
                .json(&record);

            if let Some(key) = &target.api_key {
                request = request.header("Authorization", format!("Bearer {}", key));
            }

            match request.send().await {
                Ok(_) => tracing::debug!(
                    target = %target.name,
                    export_id = %record.export_id,
                    "Webhook export succeeded"
                ),
                Err(e) => tracing::error!(
                    target = %target.name,
                    error = %e,
                    "Webhook export failed"
                ),
            }
        });
    });
    tracing::info!("WebhookExportTarget registered on mcp.exports");
}

/// Console export target — logs EEP records (for debugging).
/// Entirely synchronous — no async, no spawned task.
pub struct ConsoleExportTarget {
    pub name: String,
}

impl ConsoleExportTarget {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

/// Register a console export target as a subscriber on `"mcp.exports"`.
pub fn register_console_subscriber(stream: &EustressStream, name: String) {
    let _ = stream.subscribe_owned(topics::EXPORTS, move |msg| {
        let Ok(record) = serde_json::from_slice::<EepExportRecord>(&msg.data) else {
            tracing::warn!("ConsoleExportTarget: failed to deserialize EepExportRecord");
            return;
        };

        tracing::info!(
            target = %name,
            export_id = %record.export_id,
            entity_id = %record.entity.id,
            entity_class = %record.entity.class,
            ai_consent = %record.consent.ai_training,
            "EEP Export Record"
        );
    });
    tracing::info!("ConsoleExportTarget registered on mcp.exports");
}

/// File export target — writes EEP records to JSON files.
///
/// Async file write is spawned on a tokio task so the synchronous stream
/// callback returns immediately.
pub struct FileExportTarget {
    pub name: String,
    pub output_dir: std::path::PathBuf,
}

impl FileExportTarget {
    pub fn new(name: String, output_dir: std::path::PathBuf) -> Self {
        Self { name, output_dir }
    }
}

/// Register a `FileExportTarget` as a subscriber on `"mcp.exports"`.
pub fn register_file_subscriber(stream: &EustressStream, target: FileExportTarget) {
    let target = Arc::new(target);
    let _ = stream.subscribe_owned(topics::EXPORTS, move |msg| {
        let Ok(record) = serde_json::from_slice::<EepExportRecord>(&msg.data) else {
            tracing::warn!("FileExportTarget: failed to deserialize EepExportRecord");
            return;
        };

        // Spawn async file write — does not block the synchronous callback
        let target = Arc::clone(&target);
        tokio::spawn(async move {
            let filename = format!("{}.json", record.export_id);
            let path = target.output_dir.join(filename);

            match serde_json::to_string_pretty(&record) {
                Ok(json) => {
                    if let Err(e) = tokio::fs::write(&path, json).await {
                        tracing::error!(
                            target = %target.name,
                            path = %path.display(),
                            error = %e,
                            "File export failed"
                        );
                    } else {
                        tracing::debug!(
                            target = %target.name,
                            path = %path.display(),
                            "Exported record to file"
                        );
                    }
                }
                Err(e) => tracing::error!(
                    target = %target.name,
                    error = %e,
                    "Failed to serialize EEP record"
                ),
            }
        });
    });
    tracing::info!("FileExportTarget registered on mcp.exports");
}

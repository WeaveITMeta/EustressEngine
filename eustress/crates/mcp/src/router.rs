//! MCP Router - Routes entity changes to export targets.

use std::sync::Arc;
use tokio::sync::mpsc;

use crate::{
    error::McpResult,
    handlers::EntityOperation,
    protocol::*,
    types::*,
};

/// Parameter router for processing entity changes and routing to export targets
pub struct McpRouter {
    /// Receiver for entity operations
    rx: mpsc::Receiver<EntityOperation>,
    /// Export targets
    export_targets: Vec<Arc<dyn ExportTarget>>,
}

impl McpRouter {
    /// Create a new router
    pub fn new(rx: mpsc::Receiver<EntityOperation>) -> Self {
        Self {
            rx,
            export_targets: Vec::new(),
        }
    }

    /// Add an export target
    pub fn add_target(&mut self, target: Arc<dyn ExportTarget>) {
        self.export_targets.push(target);
    }

    /// Run the router processing loop
    pub async fn run(mut self) {
        tracing::info!("MCP Router started");

        while let Some(op) = self.rx.recv().await {
            if let Err(e) = self.process_operation(op).await {
                tracing::error!("Failed to process operation: {}", e);
            }
        }

        tracing::info!("MCP Router stopped");
    }

    /// Process a single entity operation
    async fn process_operation(&self, op: EntityOperation) -> McpResult<()> {
        match op {
            EntityOperation::Create(entity) => {
                tracing::debug!(entity_id = %entity.id, "Processing create");
                
                // Only export if AI flag is set
                if entity.ai {
                    let record = self.create_export_record(&entity, ChangeType::Created);
                    self.export_to_targets(&record).await?;
                }
            }
            EntityOperation::Update(request) => {
                tracing::debug!(entity_id = %request.entity_id, "Processing update");
                
                // Check if AI flag is being enabled
                if request.ai == Some(true) {
                    // Would need to fetch full entity data here
                    tracing::info!(
                        entity_id = %request.entity_id,
                        "AI training enabled for entity"
                    );
                }
            }
            EntityOperation::Delete(request) => {
                tracing::debug!(entity_id = %request.entity_id, "Processing delete");
                // Optionally notify targets of deletion
            }
        }

        Ok(())
    }

    /// Create an EEP export record from entity data
    fn create_export_record(&self, entity: &EntityData, change_type: ChangeType) -> EepExportRecord {
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
            hierarchy: self.build_hierarchy(entity),
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

    /// Build hierarchy path for entity
    fn build_hierarchy(&self, entity: &EntityData) -> Vec<EepHierarchyNode> {
        // For now, just include the entity itself
        // Full implementation would traverse parent chain
        vec![EepHierarchyNode {
            id: entity.id.clone(),
            name: entity.name.clone(),
            class: entity.class.clone(),
            depth: 0,
        }]
    }

    /// Export record to all configured targets
    async fn export_to_targets(&self, record: &EepExportRecord) -> McpResult<()> {
        for target in &self.export_targets {
            if let Err(e) = target.export(record).await {
                tracing::error!(
                    target = %target.name(),
                    error = %e,
                    "Failed to export to target"
                );
            }
        }
        Ok(())
    }
}

// ============================================================================
// Export Target Trait
// ============================================================================

/// Trait for export targets (MCP servers, databases, files, etc.)
#[async_trait::async_trait]
pub trait ExportTarget: Send + Sync {
    /// Target name for logging
    fn name(&self) -> &str;

    /// Export a record to this target
    async fn export(&self, record: &EepExportRecord) -> McpResult<()>;

    /// Check if target is healthy
    async fn health_check(&self) -> bool {
        true
    }
}

// ============================================================================
// Built-in Export Targets
// ============================================================================

/// Webhook export target - sends EEP records to HTTP endpoints
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

#[async_trait::async_trait]
impl ExportTarget for WebhookExportTarget {
    fn name(&self) -> &str {
        &self.name
    }

    async fn export(&self, record: &EepExportRecord) -> McpResult<()> {
        let mut request = self.client
            .post(&self.endpoint)
            .json(record);

        if let Some(key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {}", key));
        }

        request.send().await
            .map_err(|e| crate::error::McpError::Internal(e.to_string()))?;

        Ok(())
    }
}

/// Console export target - logs EEP records (for debugging)
pub struct ConsoleExportTarget {
    name: String,
}

impl ConsoleExportTarget {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

#[async_trait::async_trait]
impl ExportTarget for ConsoleExportTarget {
    fn name(&self) -> &str {
        &self.name
    }

    async fn export(&self, record: &EepExportRecord) -> McpResult<()> {
        tracing::info!(
            export_id = %record.export_id,
            entity_id = %record.entity.id,
            entity_class = %record.entity.class,
            ai_consent = %record.consent.ai_training,
            "EEP Export Record"
        );
        Ok(())
    }
}

/// File export target - writes EEP records to JSON files
pub struct FileExportTarget {
    name: String,
    output_dir: std::path::PathBuf,
}

impl FileExportTarget {
    pub fn new(name: String, output_dir: std::path::PathBuf) -> Self {
        Self { name, output_dir }
    }
}

#[async_trait::async_trait]
impl ExportTarget for FileExportTarget {
    fn name(&self) -> &str {
        &self.name
    }

    async fn export(&self, record: &EepExportRecord) -> McpResult<()> {
        let filename = format!("{}.json", record.export_id);
        let path = self.output_dir.join(filename);

        let json = serde_json::to_string_pretty(record)
            .map_err(|e| crate::error::McpError::Serialization(e))?;

        tokio::fs::write(&path, json).await
            .map_err(|e| crate::error::McpError::Io(e))?;

        tracing::debug!(path = %path.display(), "Exported record to file");
        Ok(())
    }
}

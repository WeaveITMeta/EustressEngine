//! # Eustress Scenarios — Data Agglomeration Adapters
//!
//! Table of Contents:
//! 1. DataAdapter trait — Unified interface for all data sources
//! 2. AdapterError — Error types for adapter operations
//! 3. LocalFileAdapter — JSON/CSV/RON file ingestion
//! 4. RestApiAdapter — REST API data fetching (reqwest)
//! 5. LiveFeedAdapter — Eustress Parameters live stream (tokio async)
//! 6. AdapterRegistry — Registry of all available adapters

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{
    DataSourceRef, Evidence, EvidenceType, FileFormat, ParameterValue,
    ScenarioEntity, ScenarioParameter,
};

// ─────────────────────────────────────────────
// 1. DataAdapter trait
// ─────────────────────────────────────────────

/// Unified interface for all data source adapters.
/// Each adapter knows how to fetch data from its source type
/// and convert it into scenario objects (parameters, entities, evidence).
pub trait DataAdapter: Send + Sync {
    /// Human-readable name of this adapter.
    fn name(&self) -> &str;

    /// Fetch parameters from this data source.
    fn fetch_parameters(&self) -> Result<Vec<ScenarioParameter>, AdapterError>;

    /// Fetch entities from this data source.
    fn fetch_entities(&self) -> Result<Vec<ScenarioEntity>, AdapterError>;

    /// Fetch evidence from this data source.
    fn fetch_evidence(&self) -> Result<Vec<Evidence>, AdapterError>;

    /// Check if the data source is available/reachable.
    fn health_check(&self) -> Result<bool, AdapterError>;

    /// Get the data source reference for provenance tracking.
    fn source_ref(&self) -> DataSourceRef;
}

// ─────────────────────────────────────────────
// 2. AdapterError
// ─────────────────────────────────────────────

/// Errors that can occur during adapter operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdapterError {
    /// File not found or inaccessible
    FileNotFound(String),
    /// File parse error
    ParseError { path: String, detail: String },
    /// Network/HTTP error
    NetworkError { url: String, detail: String },
    /// API returned an error status
    ApiError { status: u16, body: String },
    /// Live feed connection error
    FeedError { stream_id: String, detail: String },
    /// Data format/schema mismatch
    SchemaError(String),
    /// Generic error
    Other(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileNotFound(p) => write!(f, "File not found: {p}"),
            Self::ParseError { path, detail } => write!(f, "Parse error in {path}: {detail}"),
            Self::NetworkError { url, detail } => write!(f, "Network error for {url}: {detail}"),
            Self::ApiError { status, body } => write!(f, "API error {status}: {body}"),
            Self::FeedError { stream_id, detail } => write!(f, "Feed error [{stream_id}]: {detail}"),
            Self::SchemaError(d) => write!(f, "Schema error: {d}"),
            Self::Other(d) => write!(f, "Adapter error: {d}"),
        }
    }
}

impl std::error::Error for AdapterError {}

// ─────────────────────────────────────────────
// 3. LocalFileAdapter — JSON/CSV/RON
// ─────────────────────────────────────────────

/// Adapter for ingesting data from local files (JSON, CSV, RON).
pub struct LocalFileAdapter {
    /// Path to the file
    pub path: PathBuf,
    /// Detected or specified format
    pub format: FileFormat,
}

impl LocalFileAdapter {
    /// Create a new adapter for a local file.
    /// Format is auto-detected from extension if not specified.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, AdapterError> {
        let path = path.into();
        let format = Self::detect_format(&path)?;
        Ok(Self { path, format })
    }

    /// Create with explicit format.
    pub fn with_format(path: impl Into<PathBuf>, format: FileFormat) -> Self {
        Self {
            path: path.into(),
            format,
        }
    }

    /// Detect file format from extension.
    fn detect_format(path: &Path) -> Result<FileFormat, AdapterError> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("json") => Ok(FileFormat::Json),
            Some("csv") => Ok(FileFormat::Csv),
            Some("ron") => Ok(FileFormat::Ron),
            Some(ext) => Err(AdapterError::SchemaError(format!(
                "Unsupported file extension: .{ext}"
            ))),
            None => Err(AdapterError::SchemaError(
                "No file extension to detect format".into(),
            )),
        }
    }

    /// Read the file contents as a string.
    fn read_contents(&self) -> Result<String, AdapterError> {
        std::fs::read_to_string(&self.path).map_err(|e| {
            AdapterError::FileNotFound(format!("{}: {e}", self.path.display()))
        })
    }

    /// Parse JSON content into a generic serde_json::Value.
    fn parse_json(&self, contents: &str) -> Result<serde_json::Value, AdapterError> {
        serde_json::from_str(contents).map_err(|e| AdapterError::ParseError {
            path: self.path.display().to_string(),
            detail: e.to_string(),
        })
    }

    /// Parse RON content into a generic ron::Value.
    fn parse_ron(&self, contents: &str) -> Result<ron::Value, AdapterError> {
        ron::from_str(contents).map_err(|e| AdapterError::ParseError {
            path: self.path.display().to_string(),
            detail: e.to_string(),
        })
    }
}

impl DataAdapter for LocalFileAdapter {
    fn name(&self) -> &str {
        "LocalFile"
    }

    fn fetch_parameters(&self) -> Result<Vec<ScenarioParameter>, AdapterError> {
        let contents = self.read_contents()?;
        match self.format {
            FileFormat::Json => {
                let value = self.parse_json(&contents)?;
                // Attempt to deserialize as Vec<ScenarioParameter> directly
                serde_json::from_value(value).map_err(|e| AdapterError::ParseError {
                    path: self.path.display().to_string(),
                    detail: format!("Failed to parse as parameters: {e}"),
                })
            }
            FileFormat::Ron => {
                // RON deserialization via serde
                ron::from_str(&contents).map_err(|e| AdapterError::ParseError {
                    path: self.path.display().to_string(),
                    detail: format!("Failed to parse RON as parameters: {e}"),
                })
            }
            FileFormat::Csv => {
                // CSV: each row becomes a parameter with Text value
                let mut reader = csv::ReaderBuilder::new()
                    .has_headers(true)
                    .from_reader(contents.as_bytes());
                let mut params = Vec::new();
                for result in reader.records() {
                    let record = result.map_err(|e| AdapterError::ParseError {
                        path: self.path.display().to_string(),
                        detail: e.to_string(),
                    })?;
                    if let (Some(name), Some(value)) = (record.get(0), record.get(1)) {
                        let param_value = Self::infer_parameter_value(value);
                        let mut param = ScenarioParameter::new(name, param_value);
                        param.source = Some(self.source_ref());
                        params.push(param);
                    }
                }
                Ok(params)
            }
        }
    }

    fn fetch_entities(&self) -> Result<Vec<ScenarioEntity>, AdapterError> {
        let contents = self.read_contents()?;
        match self.format {
            FileFormat::Json => {
                serde_json::from_str(&contents).map_err(|e| AdapterError::ParseError {
                    path: self.path.display().to_string(),
                    detail: format!("Failed to parse as entities: {e}"),
                })
            }
            FileFormat::Ron => {
                ron::from_str(&contents).map_err(|e| AdapterError::ParseError {
                    path: self.path.display().to_string(),
                    detail: format!("Failed to parse RON as entities: {e}"),
                })
            }
            FileFormat::Csv => {
                // CSV entity import not yet supported — return empty
                Ok(Vec::new())
            }
        }
    }

    fn fetch_evidence(&self) -> Result<Vec<Evidence>, AdapterError> {
        let contents = self.read_contents()?;
        match self.format {
            FileFormat::Json => {
                serde_json::from_str(&contents).map_err(|e| AdapterError::ParseError {
                    path: self.path.display().to_string(),
                    detail: format!("Failed to parse as evidence: {e}"),
                })
            }
            FileFormat::Ron => {
                ron::from_str(&contents).map_err(|e| AdapterError::ParseError {
                    path: self.path.display().to_string(),
                    detail: format!("Failed to parse RON as evidence: {e}"),
                })
            }
            FileFormat::Csv => Ok(Vec::new()),
        }
    }

    fn health_check(&self) -> Result<bool, AdapterError> {
        Ok(self.path.exists())
    }

    fn source_ref(&self) -> DataSourceRef {
        DataSourceRef::LocalFile {
            path: self.path.display().to_string(),
            format: self.format,
        }
    }
}

impl LocalFileAdapter {
    /// Attempt to infer a typed ParameterValue from a string.
    fn infer_parameter_value(s: &str) -> ParameterValue {
        // Try bool
        if let Ok(b) = s.parse::<bool>() {
            return ParameterValue::Bool(b);
        }
        // Try integer
        if let Ok(i) = s.parse::<i64>() {
            return ParameterValue::Integer(i);
        }
        // Try float
        if let Ok(f) = s.parse::<f64>() {
            return ParameterValue::Float(f);
        }
        // Try timestamp
        if let Ok(dt) = s.parse::<DateTime<Utc>>() {
            return ParameterValue::Timestamp(dt);
        }
        // Fallback to text
        ParameterValue::Text(s.to_string())
    }
}

// ─────────────────────────────────────────────
// 4. RestApiAdapter — REST API data fetching
// ─────────────────────────────────────────────

/// Adapter for fetching data from REST API endpoints.
pub struct RestApiAdapter {
    /// Base URL of the API
    pub base_url: String,
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// Custom headers
    pub headers: HashMap<String, String>,
    /// Optional query parameters
    pub query_params: HashMap<String, String>,
    /// HTTP client (blocking for simplicity in Bevy systems)
    client: reqwest::blocking::Client,
}

impl RestApiAdapter {
    /// Create a new REST API adapter.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            method: "GET".into(),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            client: reqwest::blocking::Client::new(),
        }
    }

    /// Set the HTTP method.
    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = method.into();
        self
    }

    /// Add a header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Add a query parameter.
    pub fn with_query(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.query_params.insert(key.into(), value.into());
        self
    }

    /// Execute the HTTP request and return the response body as JSON.
    fn fetch_json(&self, endpoint: &str) -> Result<serde_json::Value, AdapterError> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), endpoint);
        let mut request = match self.method.to_uppercase().as_str() {
            "POST" => self.client.post(&url),
            "PUT" => self.client.put(&url),
            _ => self.client.get(&url),
        };

        for (k, v) in &self.headers {
            request = request.header(k.as_str(), v.as_str());
        }

        for (k, v) in &self.query_params {
            request = request.query(&[(k.as_str(), v.as_str())]);
        }

        let response = request.send().map_err(|e| AdapterError::NetworkError {
            url: url.clone(),
            detail: e.to_string(),
        })?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().unwrap_or_default();
            return Err(AdapterError::ApiError { status, body });
        }

        response.json().map_err(|e| AdapterError::ParseError {
            path: url,
            detail: e.to_string(),
        })
    }
}

impl DataAdapter for RestApiAdapter {
    fn name(&self) -> &str {
        "RestApi"
    }

    fn fetch_parameters(&self) -> Result<Vec<ScenarioParameter>, AdapterError> {
        let json = self.fetch_json("parameters")?;
        serde_json::from_value(json).map_err(|e| AdapterError::SchemaError(e.to_string()))
    }

    fn fetch_entities(&self) -> Result<Vec<ScenarioEntity>, AdapterError> {
        let json = self.fetch_json("entities")?;
        serde_json::from_value(json).map_err(|e| AdapterError::SchemaError(e.to_string()))
    }

    fn fetch_evidence(&self) -> Result<Vec<Evidence>, AdapterError> {
        let json = self.fetch_json("evidence")?;
        serde_json::from_value(json).map_err(|e| AdapterError::SchemaError(e.to_string()))
    }

    fn health_check(&self) -> Result<bool, AdapterError> {
        let url = format!("{}/health", self.base_url.trim_end_matches('/'));
        match self.client.get(&url).send() {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    fn source_ref(&self) -> DataSourceRef {
        DataSourceRef::RestApi {
            url: self.base_url.clone(),
            method: self.method.clone(),
            headers: self.headers.clone(),
        }
    }
}

// ─────────────────────────────────────────────
// 5. LiveFeedAdapter — Eustress Parameters async stream
// ─────────────────────────────────────────────

/// Adapter for receiving live data from Eustress Parameters via tokio channels.
/// This adapter bridges async tokio streams into the synchronous Bevy world
/// via crossbeam channels.
pub struct LiveFeedAdapter {
    /// Parameter name this feed is bound to
    pub parameter_name: String,
    /// Unique stream identifier
    pub stream_id: String,
    /// Receiver end of the crossbeam channel (sync side, read by Bevy systems)
    pub receiver: crossbeam_channel::Receiver<LiveFeedMessage>,
    /// Whether the feed is currently connected
    pub connected: bool,
}

/// A message received from a live feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveFeedMessage {
    /// Timestamp of the message
    pub timestamp: DateTime<Utc>,
    /// The parameter value update
    pub value: ParameterValue,
    /// Optional metadata
    pub metadata: HashMap<String, String>,
}

/// Handle for the async side of a live feed (held by the tokio task).
pub struct LiveFeedSender {
    /// Sender end of the crossbeam channel (async side, written by tokio task)
    pub sender: crossbeam_channel::Sender<LiveFeedMessage>,
    /// Stream identifier
    pub stream_id: String,
}

impl LiveFeedAdapter {
    /// Create a new live feed adapter with a bounded channel.
    /// Returns (adapter, sender) — the sender is given to the tokio task.
    pub fn new(
        parameter_name: impl Into<String>,
        buffer_size: usize,
    ) -> (Self, LiveFeedSender) {
        let stream_id = Uuid::new_v4().to_string();
        let (sender, receiver) = crossbeam_channel::bounded(buffer_size);

        let adapter = Self {
            parameter_name: parameter_name.into(),
            stream_id: stream_id.clone(),
            receiver,
            connected: false,
        };

        let feed_sender = LiveFeedSender {
            sender,
            stream_id,
        };

        (adapter, feed_sender)
    }

    /// Drain all pending messages from the channel (non-blocking).
    pub fn drain(&self) -> Vec<LiveFeedMessage> {
        let mut messages = Vec::new();
        while let Ok(msg) = self.receiver.try_recv() {
            messages.push(msg);
        }
        messages
    }
}

impl DataAdapter for LiveFeedAdapter {
    fn name(&self) -> &str {
        "LiveFeed"
    }

    fn fetch_parameters(&self) -> Result<Vec<ScenarioParameter>, AdapterError> {
        // Convert pending messages into parameter updates
        let messages = self.drain();
        let params = messages
            .into_iter()
            .map(|msg| {
                let mut param = ScenarioParameter::new(&self.parameter_name, msg.value);
                param.source = Some(self.source_ref());
                param.updated_at = msg.timestamp;
                param.live = true;
                param
            })
            .collect();
        Ok(params)
    }

    fn fetch_entities(&self) -> Result<Vec<ScenarioEntity>, AdapterError> {
        // Live feeds primarily produce parameter updates, not entities
        Ok(Vec::new())
    }

    fn fetch_evidence(&self) -> Result<Vec<Evidence>, AdapterError> {
        // Live feeds primarily produce parameter updates, not evidence
        Ok(Vec::new())
    }

    fn health_check(&self) -> Result<bool, AdapterError> {
        Ok(self.connected && !self.receiver.is_empty())
    }

    fn source_ref(&self) -> DataSourceRef {
        DataSourceRef::LiveFeed {
            parameter_name: self.parameter_name.clone(),
            stream_id: self.stream_id.clone(),
        }
    }
}

// ─────────────────────────────────────────────
// 6. AdapterRegistry
// ─────────────────────────────────────────────

/// Registry of all available data adapters for a scenario.
/// Manages adapter lifecycle and provides unified data fetching.
pub struct AdapterRegistry {
    /// Registered adapters indexed by name
    adapters: Vec<Box<dyn DataAdapter>>,
}

impl AdapterRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            adapters: Vec::new(),
        }
    }

    /// Register a new adapter.
    pub fn register(&mut self, adapter: Box<dyn DataAdapter>) {
        self.adapters.push(adapter);
    }

    /// Fetch parameters from all adapters, merging results.
    pub fn fetch_all_parameters(&self) -> Vec<(String, Result<Vec<ScenarioParameter>, AdapterError>)> {
        self.adapters
            .iter()
            .map(|a| (a.name().to_string(), a.fetch_parameters()))
            .collect()
    }

    /// Fetch entities from all adapters, merging results.
    pub fn fetch_all_entities(&self) -> Vec<(String, Result<Vec<ScenarioEntity>, AdapterError>)> {
        self.adapters
            .iter()
            .map(|a| (a.name().to_string(), a.fetch_entities()))
            .collect()
    }

    /// Fetch evidence from all adapters, merging results.
    pub fn fetch_all_evidence(&self) -> Vec<(String, Result<Vec<Evidence>, AdapterError>)> {
        self.adapters
            .iter()
            .map(|a| (a.name().to_string(), a.fetch_evidence()))
            .collect()
    }

    /// Health check all adapters.
    pub fn health_check_all(&self) -> Vec<(String, bool)> {
        self.adapters
            .iter()
            .map(|a| (a.name().to_string(), a.health_check().unwrap_or(false)))
            .collect()
    }

    /// Number of registered adapters.
    pub fn count(&self) -> usize {
        self.adapters.len()
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_parameter_value() {
        assert_eq!(
            LocalFileAdapter::infer_parameter_value("true"),
            ParameterValue::Bool(true)
        );
        assert_eq!(
            LocalFileAdapter::infer_parameter_value("42"),
            ParameterValue::Integer(42)
        );
        assert_eq!(
            LocalFileAdapter::infer_parameter_value("3.14"),
            ParameterValue::Float(3.14)
        );
        assert_eq!(
            LocalFileAdapter::infer_parameter_value("hello"),
            ParameterValue::Text("hello".into())
        );
    }

    #[test]
    fn test_live_feed_channel() {
        let (adapter, sender) = LiveFeedAdapter::new("temperature", 10);

        // Send a message
        sender.sender.send(LiveFeedMessage {
            timestamp: Utc::now(),
            value: ParameterValue::Float(72.5),
            metadata: HashMap::new(),
        }).unwrap();

        // Drain and check
        let messages = adapter.drain();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].value, ParameterValue::Float(72.5));
    }

    #[test]
    fn test_adapter_registry() {
        let registry = AdapterRegistry::new();
        assert_eq!(registry.count(), 0);
    }
}

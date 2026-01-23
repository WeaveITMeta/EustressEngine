//! Error types for spatial-llm
//!
//! ## Table of Contents
//! 1. SpatialLlmError - Main error enum

use thiserror::Error;

/// Errors that can occur in spatial LLM operations
#[derive(Error, Debug)]
pub enum SpatialLlmError {
    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Network/API error
    #[error("API error: {0}")]
    Api(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Context building error
    #[error("Context error: {0}")]
    Context(String),

    /// Generation error
    #[error("Generation error: {0}")]
    Generation(String),

    /// Query error
    #[error("Query error: {0}")]
    Query(String),

    /// Index error
    #[error("Index error: {0}")]
    Index(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<serde_json::Error> for SpatialLlmError {
    fn from(err: serde_json::Error) -> Self {
        SpatialLlmError::Serialization(err.to_string())
    }
}

impl From<reqwest::Error> for SpatialLlmError {
    fn from(err: reqwest::Error) -> Self {
        SpatialLlmError::Api(err.to_string())
    }
}

/// Result type for spatial LLM operations
pub type Result<T> = std::result::Result<T, SpatialLlmError>;

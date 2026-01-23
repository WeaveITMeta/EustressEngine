//! Error types for eustress-embedvec
//!
//! ## Table of Contents
//! 1. EmbedvecError - Main error enum
//! 2. Result type alias

use thiserror::Error;

/// Result type alias for embedvec operations
pub type Result<T> = std::result::Result<T, EmbedvecError>;

/// Errors that can occur in embedvec operations
#[derive(Error, Debug)]
pub enum EmbedvecError {
    /// Index not initialized
    #[error("Embedvec index not initialized")]
    NotInitialized,

    /// Entity not found in index
    #[error("Entity {0} not found in index")]
    EntityNotFound(bevy::prelude::Entity),

    /// Embedding dimension mismatch
    #[error("Embedding dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Persistence error
    #[error("Persistence error: {0}")]
    Persistence(String),

    /// Embedder error
    #[error("Embedder error: {0}")]
    Embedder(String),

    /// Index operation error
    #[error("Index error: {0}")]
    Index(String),

    /// Reflection error
    #[error("Reflection error: {0}")]
    Reflection(String),
}

impl From<serde_json::Error> for EmbedvecError {
    fn from(err: serde_json::Error) -> Self {
        EmbedvecError::Serialization(err.to_string())
    }
}

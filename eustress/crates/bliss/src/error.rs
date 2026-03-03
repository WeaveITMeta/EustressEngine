// =============================================================================
// Bliss Cryptocurrency - Error Types
// =============================================================================
// Table of Contents:
// 1. BlissError enum - all error variants for the Bliss system
// =============================================================================

use thiserror::Error;

/// All error types for the Bliss cryptocurrency system.
#[derive(Debug, Error)]
pub enum BlissError {
    /// Configuration error (missing or invalid config).
    #[error("Configuration error: {0}")]
    Config(String),
    
    /// Wallet error (creation, loading, signing).
    #[error("Wallet error: {0}")]
    Wallet(String),
    
    /// Wallet not found by address.
    #[error("Wallet not found: {0}")]
    WalletNotFound(String),
    
    /// Insufficient balance for a transaction.
    #[error("Insufficient balance: have {have}, need {need}")]
    InsufficientBalance { have: u64, need: u64 },
    
    /// Cryptographic operation failed.
    #[error("Crypto error: {0}")]
    Crypto(String),
    
    /// Invalid signature.
    #[error("Invalid signature")]
    InvalidSignature,
    
    /// Blockchain communication error.
    #[error("Blockchain error: {0}")]
    Blockchain(String),
    
    /// Transaction failed or was rejected.
    #[error("Transaction error: {0}")]
    Transaction(String),
    
    /// Contribution validation error.
    #[error("Contribution error: {0}")]
    Contribution(String),
    
    /// Duplicate contribution detected.
    #[error("Duplicate contribution: {0}")]
    DuplicateContribution(String),
    
    /// Network/HTTP error.
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    
    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    /// Generic internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

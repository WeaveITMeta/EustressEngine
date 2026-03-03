// =============================================================================
// Bliss Cryptocurrency - Blockchain Interface
// =============================================================================
// Table of Contents:
// 1. BlockchainConfig - connection and chain configuration
// 2. Transaction - on-chain transaction record
// 3. Blockchain - high-level blockchain client for Bliss operations
// =============================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::crypto::BlissCrypto;
use crate::error::BlissError;
use crate::wallet::WalletAddress;

// =============================================================================
// 1. BlockchainConfig
// =============================================================================

/// Configuration for connecting to the Bliss blockchain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainConfig {
    /// RPC endpoint URL (e.g., "https://rpc.bliss.eustress.dev").
    pub rpc_url: String,
    /// Chain ID for transaction signing.
    pub chain_id: u64,
    /// Whether to use a local mock chain for development.
    pub use_mock: bool,
    /// Block confirmation count before considering a transaction final.
    pub confirmations: u32,
}

impl BlockchainConfig {
    /// Load config from environment variables.
    pub fn from_env() -> Result<Self, BlissError> {
        Ok(Self {
            rpc_url: std::env::var("BLISS_RPC_URL")
                .unwrap_or_else(|_| "http://localhost:8545".into()),
            chain_id: std::env::var("BLISS_CHAIN_ID")
                .unwrap_or_else(|_| "1337".into())
                .parse()
                .unwrap_or(1337),
            use_mock: std::env::var("BLISS_USE_MOCK")
                .unwrap_or_else(|_| "true".into())
                .parse()
                .unwrap_or(true),
            confirmations: std::env::var("BLISS_CONFIRMATIONS")
                .unwrap_or_else(|_| "1".into())
                .parse()
                .unwrap_or(1),
        })
    }
}

impl Default for BlockchainConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8545".into(),
            chain_id: 1337,
            use_mock: true,
            confirmations: 1,
        }
    }
}

// =============================================================================
// 2. Transaction
// =============================================================================

/// A Bliss blockchain transaction record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Unique transaction hash.
    pub hash: String,
    /// Sender wallet address.
    pub from: String,
    /// Recipient wallet address.
    pub to: String,
    /// Amount in micro-BLS (1 BLS = 1_000_000 micro-BLS).
    pub amount: u64,
    /// Optional memo/description.
    pub memo: Option<String>,
    /// Transaction timestamp.
    pub timestamp: DateTime<Utc>,
    /// Whether the transaction is confirmed.
    pub confirmed: bool,
}

impl Transaction {
    /// Generate a transaction hash from its contents.
    pub fn compute_hash(from: &str, to: &str, amount: u64, timestamp: &DateTime<Utc>) -> String {
        let data = format!("{}:{}:{}:{}", from, to, amount, timestamp.timestamp_millis());
        let hash = BlissCrypto::hash(data.as_bytes());
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

// =============================================================================
// 3. Blockchain
// =============================================================================

/// High-level blockchain client for Bliss operations.
/// Uses an in-memory mock ledger when `config.use_mock` is true.
pub struct Blockchain {
    /// Connection configuration.
    config: BlockchainConfig,
    /// In-memory balances (mock mode only).
    balances: HashMap<String, u64>,
    /// In-memory transaction history (mock mode only).
    transactions: Vec<Transaction>,
}

impl Blockchain {
    /// Create a new blockchain client.
    pub async fn new(config: BlockchainConfig) -> Result<Self, BlissError> {
        if !config.use_mock {
            tracing::warn!("Live blockchain not yet implemented; falling back to mock mode");
        }
        
        Ok(Self {
            config,
            balances: HashMap::new(),
            transactions: Vec::new(),
        })
    }
    
    /// Get the balance for a wallet address.
    pub async fn get_balance(&self, address: &WalletAddress) -> Result<u64, BlissError> {
        Ok(*self.balances.get(address.as_str()).unwrap_or(&0))
    }
    
    /// Credit tokens to a wallet (used by contribution rewards).
    pub async fn credit(&mut self, address: &WalletAddress, amount: u64, memo: Option<String>) -> Result<Transaction, BlissError> {
        let balance = self.balances.entry(address.to_string()).or_insert(0);
        *balance += amount;
        
        let now = Utc::now();
        let transaction = Transaction {
            hash: Transaction::compute_hash("system", address.as_str(), amount, &now),
            from: "system".into(),
            to: address.to_string(),
            amount,
            memo,
            timestamp: now,
            confirmed: true,
        };
        
        self.transactions.push(transaction.clone());
        Ok(transaction)
    }
    
    /// Transfer tokens between wallets.
    pub async fn transfer(
        &mut self,
        from: &WalletAddress,
        to: &WalletAddress,
        amount: u64,
        memo: Option<String>,
    ) -> Result<Transaction, BlissError> {
        // Check sender balance
        let sender_balance = *self.balances.get(from.as_str()).unwrap_or(&0);
        if sender_balance < amount {
            return Err(BlissError::InsufficientBalance {
                have: sender_balance,
                need: amount,
            });
        }
        
        // Debit sender
        *self.balances.entry(from.to_string()).or_insert(0) -= amount;
        // Credit recipient
        *self.balances.entry(to.to_string()).or_insert(0) += amount;
        
        let now = Utc::now();
        let transaction = Transaction {
            hash: Transaction::compute_hash(from.as_str(), to.as_str(), amount, &now),
            from: from.to_string(),
            to: to.to_string(),
            amount,
            memo,
            timestamp: now,
            confirmed: true,
        };
        
        self.transactions.push(transaction.clone());
        Ok(transaction)
    }
    
    /// Get transaction history for an address (sent or received).
    pub async fn get_transactions(&self, address: &WalletAddress) -> Result<Vec<Transaction>, BlissError> {
        let addr = address.as_str();
        let matching: Vec<Transaction> = self.transactions.iter()
            .filter(|transaction| transaction.from == addr || transaction.to == addr)
            .cloned()
            .collect();
        Ok(matching)
    }
    
    /// Get a transaction by hash.
    pub async fn get_transaction(&self, hash: &str) -> Result<Option<Transaction>, BlissError> {
        Ok(self.transactions.iter().find(|transaction| transaction.hash == hash).cloned())
    }
    
    /// Get the current blockchain configuration.
    pub fn config(&self) -> &BlockchainConfig {
        &self.config
    }
}

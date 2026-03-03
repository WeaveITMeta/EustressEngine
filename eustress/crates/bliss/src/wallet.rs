// =============================================================================
// Bliss Cryptocurrency - Wallet Management
// =============================================================================
// Table of Contents:
// 1. WalletAddress - unique wallet identifier derived from public key
// 2. Wallet - single wallet with keypair and balance
// 3. WalletManager - create, load, and manage multiple wallets
// =============================================================================

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::crypto::{BlissCrypto, PrivateKey, PublicKey, Signature};
use crate::error::BlissError;

// =============================================================================
// 1. WalletAddress
// =============================================================================

/// Unique wallet address derived from the public key hash.
/// Format: "bls_" + first 20 bytes of BLAKE3(public_key) as hex.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WalletAddress(String);

impl WalletAddress {
    /// Derive a wallet address from a public key.
    pub fn from_public_key(public_key: &PublicKey) -> Self {
        let hash = BlissCrypto::hash(public_key.as_bytes());
        let hex: String = hash[..20].iter().map(|b| format!("{:02x}", b)).collect();
        Self(format!("bls_{}", hex))
    }
    
    /// Get the address string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WalletAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// =============================================================================
// 2. Wallet
// =============================================================================

/// A single Bliss wallet containing a keypair and cached balance.
#[derive(Debug)]
pub struct Wallet {
    /// Wallet address (derived from public key).
    address: WalletAddress,
    /// Public key for verification.
    public_key: PublicKey,
    /// Private key for signing (kept in memory only).
    private_key: PrivateKey,
    /// Cached BLS balance (in smallest unit, 1 BLS = 1_000_000 micro-BLS).
    balance: u64,
}

impl Wallet {
    /// Create a new wallet with a fresh keypair.
    pub fn new() -> Self {
        let (private_key, public_key) = BlissCrypto::generate_keypair();
        let address = WalletAddress::from_public_key(&public_key);
        Self {
            address,
            public_key,
            private_key,
            balance: 0,
        }
    }
    
    /// Restore a wallet from an existing private key.
    pub fn from_private_key(private_key: PrivateKey) -> Self {
        let public_key = private_key.public_key();
        let address = WalletAddress::from_public_key(&public_key);
        Self {
            address,
            public_key,
            private_key,
            balance: 0,
        }
    }
    
    /// Get the wallet address.
    pub fn address(&self) -> &WalletAddress {
        &self.address
    }
    
    /// Get the public key.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }
    
    /// Get the cached balance.
    pub fn balance(&self) -> u64 {
        self.balance
    }
    
    /// Update the cached balance.
    pub fn set_balance(&mut self, balance: u64) {
        self.balance = balance;
    }
    
    /// Sign a message with this wallet's private key.
    pub fn sign(&self, message: &[u8]) -> Signature {
        BlissCrypto::sign(message, &self.private_key)
    }
}

impl Default for Wallet {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// 3. WalletManager
// =============================================================================

/// Manages multiple wallets in memory.
pub struct WalletManager {
    /// All managed wallets, keyed by address.
    wallets: HashMap<String, Wallet>,
}

impl WalletManager {
    /// Create an empty wallet manager.
    pub fn new() -> Self {
        Self {
            wallets: HashMap::new(),
        }
    }
    
    /// Create a new wallet and add it to the manager.
    pub fn create_wallet(&mut self) -> &Wallet {
        let wallet = Wallet::new();
        let address = wallet.address().to_string();
        self.wallets.insert(address.clone(), wallet);
        self.wallets.get(&address).unwrap()
    }
    
    /// Import a wallet from a private key.
    pub fn import_wallet(&mut self, private_key: PrivateKey) -> &Wallet {
        let wallet = Wallet::from_private_key(private_key);
        let address = wallet.address().to_string();
        self.wallets.insert(address.clone(), wallet);
        self.wallets.get(&address).unwrap()
    }
    
    /// Get a wallet by address.
    pub fn get_wallet(&self, address: &str) -> Option<&Wallet> {
        self.wallets.get(address)
    }
    
    /// Get a mutable wallet by address.
    pub fn get_wallet_mut(&mut self, address: &str) -> Option<&mut Wallet> {
        self.wallets.get_mut(address)
    }
    
    /// List all wallet addresses.
    pub fn list_addresses(&self) -> Vec<&str> {
        self.wallets.keys().map(|s| s.as_str()).collect()
    }
    
    /// Number of managed wallets.
    pub fn count(&self) -> usize {
        self.wallets.len()
    }
}

impl Default for WalletManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Eustress Web - Wallet Module
// =============================================================================
// Table of Contents:
// 1. Types (WalletStatus, WalletInfo)
// 2. WalletHandle (reactive wallet state)
// 3. use_wallet hook
// =============================================================================

use leptos::prelude::*;

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// Wallet connection info displayed when connected.
#[derive(Debug, Clone, PartialEq)]
pub struct WalletInfo {
    /// Wallet display name
    pub name: String,
    /// Truncated wallet address
    pub address: String,
}

/// Wallet connection status.
#[derive(Debug, Clone, PartialEq)]
pub enum WalletStatus {
    /// Wallet is disconnected
    Disconnected,
    /// Wallet is connecting
    Connecting,
    /// Wallet is connected with info
    Connected(WalletInfo),
    /// Wallet encountered an error
    Error(String),
}

// -----------------------------------------------------------------------------
// 2. WalletHandle
// -----------------------------------------------------------------------------

/// Reactive wallet handle providing signals and methods for wallet interaction.
#[derive(Clone)]
pub struct WalletHandle {
    /// Current wallet status
    pub status: RwSignal<WalletStatus>,
    /// BLS balance
    pub balance: RwSignal<f64>,
    /// Pending rewards
    pub pending: RwSignal<f64>,
    /// Contribution score
    pub contribution_score: RwSignal<f64>,
}

impl WalletHandle {
    /// Create a new disconnected wallet handle.
    pub fn new() -> Self {
        Self {
            status: RwSignal::new(WalletStatus::Disconnected),
            balance: RwSignal::new(0.0),
            pending: RwSignal::new(0.0),
            contribution_score: RwSignal::new(0.0),
        }
    }

    /// Create a new wallet with the given name.
    pub fn create_wallet(&self, name: &str) -> Result<(), String> {
        self.status.set(WalletStatus::Connecting);

        // Generate a placeholder address
        let address = format!("BLS:{:x}{:x}", name.len(), name.as_bytes().iter().sum::<u8>());
        let truncated = if address.len() > 12 {
            format!("{}...{}", &address[..6], &address[address.len()-4..])
        } else {
            address
        };

        self.status.set(WalletStatus::Connected(WalletInfo {
            name: name.to_string(),
            address: truncated,
        }));
        self.balance.set(0.0);
        self.pending.set(0.0);
        self.contribution_score.set(0.0);
        Ok(())
    }

    /// Import an existing wallet from a private key.
    pub fn import_wallet(&self, name: &str, private_key: &str) -> Result<(), String> {
        if private_key.len() < 8 {
            return Err("Invalid private key".to_string());
        }

        self.status.set(WalletStatus::Connecting);

        let truncated = if private_key.len() > 12 {
            format!("{}...{}", &private_key[..6], &private_key[private_key.len()-4..])
        } else {
            private_key.to_string()
        };

        self.status.set(WalletStatus::Connected(WalletInfo {
            name: name.to_string(),
            address: truncated,
        }));
        Ok(())
    }

    /// Disconnect the wallet.
    pub fn disconnect(&self) {
        self.status.set(WalletStatus::Disconnected);
        self.balance.set(0.0);
        self.pending.set(0.0);
        self.contribution_score.set(0.0);
    }

    /// Format the BLS balance for display.
    pub fn formatted_balance(&self) -> String {
        format!("{:.4} BLS", self.balance.get())
    }

    /// Format the pending rewards for display.
    pub fn formatted_pending(&self) -> String {
        format!("{:.4} BLS", self.pending.get())
    }
}

// -----------------------------------------------------------------------------
// 3. use_wallet hook
// -----------------------------------------------------------------------------

/// Leptos hook to get a reactive wallet handle.
pub fn use_wallet() -> WalletHandle {
    WalletHandle::new()
}

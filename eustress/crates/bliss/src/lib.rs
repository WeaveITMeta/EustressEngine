//! # Bliss Cryptocurrency
//!
//! Bliss (BLS) - Proof-of-Contribution cryptocurrency for the Eustress ecosystem.
//! Earn through contributions to the platform or purchase on exchanges.
//!
//! ## Features
//!
//! - **Contribution Tracking**: Automatically track user contributions
//! - **Proof-of-Contribution**: Cryptographic proof of meaningful contributions
//! - **Token Distribution**: Fair distribution based on contribution weight
//! - **Exchange Integration**: Trade on decentralized exchanges
//! - **Wallet Management**: Built-in wallet for BLS tokens
//!
//! ## Contribution Types
//!
//! | Type | Weight | Description |
//! |------|--------|-------------|
//! | Building | 2.5x | Create 3D models, places, and assets |
//! | Scripting | 3.0x | Write Soul scripts and game logic |
//! | Design | 2.0x | UI/UX design, texturing, visual work |
//! | Collaboration | 2.0x | Team work, communication, helping others |
//! | Teaching | 2.2x | Tutorials, mentoring, documentation |
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use eustress_bliss::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), BlissError> {
//!     let bliss = Bliss::new(BlissConfig::from_env()?).await?;
//!     
//!     // Create a wallet
//!     let wallet = bliss.create_wallet().await?;
//!     println!("Wallet address: {}", wallet.address());
//!     
//!     // Record a contribution
//!     let contribution = bliss.record_contribution(Contribution {
//!         user_id: "user123".into(),
//!         contribution_type: ContributionType::Building,
//!         weight: 2.5,
//!         description: "Created medieval castle model".into(),
//!         evidence: vec!["asset_id_123".into()],
//!     }).await?;
//!     
//!     // Check balance
//!     let balance = bliss.get_balance(wallet.address()).await?;
//!     println!("BLS Balance: {}", balance);
//!     
//!     Ok(())
//! }
//! ```

pub mod blockchain;
pub mod contribution;
pub mod crypto;
pub mod error;
pub mod wallet;

#[cfg(feature = "database")]
pub mod database;

#[cfg(feature = "mock")]
pub mod mock;

pub use blockchain::{Blockchain, BlockchainConfig};
pub use contribution::{Contribution, ContributionTracker, ContributionType};
pub use crypto::{BlissCrypto, Signature};
pub use error::BlissError;
pub use wallet::{Wallet, WalletManager};

// ============================================================================
// Prelude
// ============================================================================

/// Convenient re-exports for common Bliss types.
pub mod prelude {
    pub use super::blockchain::{Blockchain, BlockchainConfig, Transaction};
    pub use super::contribution::{Contribution, ContributionTracker, ContributionType, ContributionWeight};
    pub use super::crypto::{BlissCrypto, Signature, PublicKey, PrivateKey};
    pub use super::error::BlissError;
    pub use super::wallet::{Wallet, WalletManager, WalletAddress};
}
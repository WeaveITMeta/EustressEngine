//! # Eustress Bliss Integration
//!
//! Connects EustressEngine to the Bliss (BLS) proof-of-contribution network.
//! Every engine instance runs a **Light Node** by default. Users can opt in
//! to a **Full Node** for +10% BLS bonus and block production.
//!
//! ## What launches with the engine
//!
//! ```text
//! EustressEngine
//!   └── eustress-bliss (this crate)
//!         ├── node     — Light/Full mode, contribution tracking
//!         ├── api      — Axum HTTP server (auth, cosign, health)
//!         ├── store    — SQLite identity database (embedded)
//!         ├── cosign   — Witness co-signing client
//!         └── bliss-*  — Official crates from crates.io
//! ```
//!
//! One engine launched = one working auth server. No external DB needed.

pub mod api;
pub mod cosign;
pub mod error;
pub mod node;

// Re-export official Bliss crates for engine-wide access
pub use bliss_core as core;
pub use bliss_crypto as crypto;
pub use bliss_embedded as embedded;
pub use bliss_events as events;
pub use bliss_wallet as wallet;

pub use api::start_server;
pub use cosign::CosignClient;
pub use error::BlissError;
pub use node::{BlissNode, NodeConfig, NodeMode};

/// Prelude for convenient imports.
pub mod prelude {
    pub use super::api::start_server;
    pub use super::cosign::CosignClient;
    pub use super::error::BlissError;
    pub use super::node::{BlissNode, NodeConfig, NodeMode};

    pub use bliss_core::amount::Amount;
    pub use bliss_crypto::{KeyPair, PrivateKey, PublicKey, Signature};
}

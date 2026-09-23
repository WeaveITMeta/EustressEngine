// =============================================================================
// Eustress Web - Purchases API
// =============================================================================
// The signed-in account's own purchases: every Tickets and Bliss spend, with
// the totals grouped by creator and by simulation. The Worker answers only for
// the bearer's own account; there is no call here that takes an account id.
//
// Table of Contents:
// 1. Types
// 2. Purchases API Functions
// =============================================================================

use serde::Deserialize;
use super::{ApiClient, ApiError};

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// Everything spent, per currency. Tickets and Bliss are never summed: they
/// have no fixed rate between them.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct PurchaseTotals {
    #[serde(default)]
    pub tickets: u64,
    /// Whole BLS, exact to two decimals (the ledger stores integer cents).
    #[serde(default)]
    pub bliss: f64,
    #[serde(default)]
    pub purchases: u64,
    #[serde(default)]
    pub creators: u64,
    #[serde(default)]
    pub simulations: u64,
}

/// One purchase, as the buyer's receipt recorded it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Purchase {
    pub id: String,
    /// RFC 3339 time of the spend.
    pub ts: String,
    /// "TKT" or "BLS".
    pub currency: String,
    /// Whole Tickets, or BLS to two decimals.
    pub amount: f64,
    #[serde(default)]
    pub title: String,
    /// An Eustress-hosted image, or empty when the purchase named none.
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub product_id: String,
    #[serde(default)]
    pub simulation_id: Option<String>,
    #[serde(default)]
    pub simulation_name: String,
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub creator_name: String,
}

impl Purchase {
    pub fn is_bliss(&self) -> bool {
        self.currency == "BLS"
    }
}

/// Spend with one creator. `creator_id` is `None` for purchases no creator
/// was credited with.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CreatorSpend {
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub creator_name: String,
    #[serde(default)]
    pub tickets: u64,
    #[serde(default)]
    pub bliss: f64,
    #[serde(default)]
    pub purchases: u64,
    #[serde(default)]
    pub last_at: String,
}

/// Spend in one simulation. `simulation_id` is `None` for everything bought
/// outside a simulation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SimulationSpend {
    #[serde(default)]
    pub simulation_id: Option<String>,
    #[serde(default)]
    pub simulation_name: String,
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub creator_name: String,
    #[serde(default)]
    pub tickets: u64,
    #[serde(default)]
    pub bliss: f64,
    #[serde(default)]
    pub purchases: u64,
    #[serde(default)]
    pub last_at: String,
}

/// `GET /api/purchases`
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct PurchaseHistory {
    #[serde(default)]
    pub totals: PurchaseTotals,
    /// Newest first.
    #[serde(default)]
    pub purchases: Vec<Purchase>,
    #[serde(default)]
    pub by_creator: Vec<CreatorSpend>,
    #[serde(default)]
    pub by_simulation: Vec<SimulationSpend>,
    /// Set when the account has more purchases than one read returns (50,000).
    #[serde(default)]
    pub truncated: bool,
}

/// `GET /api/purchases/summary`
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct PurchaseSummary {
    #[serde(default)]
    pub totals: PurchaseTotals,
    #[serde(default)]
    pub truncated: bool,
}

// -----------------------------------------------------------------------------
// 2. Purchases API Functions
// -----------------------------------------------------------------------------

/// Every purchase the signed-in account has made, with both groupings.
pub async fn get_purchases(client: &ApiClient) -> Result<PurchaseHistory, ApiError> {
    client.get("/api/purchases").await
}

/// Totals only, for the profile.
pub async fn get_purchase_summary(client: &ApiClient) -> Result<PurchaseSummary, ApiError> {
    client.get("/api/purchases/summary").await
}

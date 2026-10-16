// =============================================================================
// Eustress Web - Delta API
// =============================================================================
// Delta is a usage currency for premium Studio tools: new verified accounts get
// a trial amount, subscriptions add more each month, and more can be bought.
// It never converts to Bliss or Tickets and cannot be cashed out. The balance
// is read from the Worker; this page-side code never computes one.
//
// Table of Contents:
// 1. Types
// 2. Delta API Functions
// =============================================================================

use serde::Deserialize;
use super::{ApiClient, ApiError};

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// `GET /api/delta/balance`
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct DeltaBalance {
    /// Delta available to spend. Read as a number so a fractional balance
    /// still parses; the chip shows the whole part.
    #[serde(default)]
    pub delta: f64,
    /// Delta held for work in progress.
    #[serde(default)]
    pub reserved: f64,
    /// The account's plan: `free`, `player_plus`, `creator`, `professional` or `bundle`.
    #[serde(default)]
    pub plan: Option<String>,
}

impl DeltaBalance {
    /// Whole Delta, for display. Never negative.
    pub fn whole(&self) -> u64 {
        if self.delta.is_finite() && self.delta > 0.0 {
            self.delta.floor() as u64
        } else {
            0
        }
    }
}

// -----------------------------------------------------------------------------
// 2. Delta API Functions
// -----------------------------------------------------------------------------

/// The signed-in account's Delta balance, or `None` when the Worker has no
/// Delta route yet (a 404), so the top bar simply shows no chip.
pub async fn get_delta_balance(client: &ApiClient) -> Result<Option<DeltaBalance>, ApiError> {
    match client.get::<DeltaBalance>("/api/delta/balance").await {
        Ok(balance) => Ok(Some(balance)),
        Err(ApiError::NotFound) => Ok(None),
        Err(e) => Err(e),
    }
}

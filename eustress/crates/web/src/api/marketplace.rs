// =============================================================================
// Eustress Web - Marketplace API
// =============================================================================
// Table of Contents:
// 1. Types
// 2. Response Types
// 3. Marketplace API Functions
// =============================================================================

use serde::{Deserialize, Serialize};
use super::{ApiClient, ApiError};

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// Marketplace item listing (API shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub thumbnail_url: Option<String>,
    pub price_bliss: f64,
    pub currency: String,
    pub creator_name: String,
    pub creator_id: String,
    pub category: String,
    pub sales_count: u64,
    pub rating: f32,
    pub is_verified: bool,
    pub is_free: bool,
    pub equity_available: Option<f32>,
    pub equity_price_per_percent: Option<f64>,
    pub created_at: String,
}

/// Marketplace query parameters.
#[derive(Debug, Default, Serialize)]
pub struct MarketplaceQuery {
    pub category: Option<String>,
    pub tab: Option<String>,
    pub sort: Option<String>,
    pub min_price: Option<f64>,
    pub max_price: Option<f64>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub limit: Option<u32>,
}

// -----------------------------------------------------------------------------
// 2. Response Types
// -----------------------------------------------------------------------------

/// Response from get_marketplace endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct MarketplaceResponse {
    pub items: Vec<MarketplaceItem>,
}

// -----------------------------------------------------------------------------
// 3. Marketplace API Functions
// -----------------------------------------------------------------------------

/// Get marketplace items (paginated).
pub async fn get_marketplace(
    client: &ApiClient,
    query: &MarketplaceQuery,
) -> Result<MarketplaceResponse, ApiError> {
    let mut endpoint = "/api/marketplace?".to_string();
    if let Some(c) = &query.category {
        endpoint.push_str(&format!("category={}&", c));
    }
    if let Some(t) = &query.tab {
        endpoint.push_str(&format!("tab={}&", t));
    }
    if let Some(s) = &query.sort {
        endpoint.push_str(&format!("sort={}&", s));
    }
    if let Some(p) = query.page {
        endpoint.push_str(&format!("page={}&", p));
    }
    if let Some(l) = query.limit {
        endpoint.push_str(&format!("limit={}&", l));
    }
    client.get(&endpoint).await
}

/// Get featured marketplace items.
pub async fn get_featured_items(client: &ApiClient) -> Result<Vec<MarketplaceItem>, ApiError> {
    client.get("/api/marketplace/featured").await
}

/// Get a specific marketplace item by ID.
pub async fn get_marketplace_item(
    client: &ApiClient,
    item_id: &str,
) -> Result<MarketplaceItem, ApiError> {
    let endpoint = format!("/api/marketplace/{}", item_id);
    client.get(&endpoint).await
}

/// Purchase a marketplace item.
pub async fn purchase_item(
    client: &ApiClient,
    item_id: &str,
) -> Result<serde_json::Value, ApiError> {
    let endpoint = format!("/api/marketplace/{}/purchase", item_id);
    client.post(&endpoint, &()).await
}

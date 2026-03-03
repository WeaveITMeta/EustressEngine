// =============================================================================
// Eustress Backend - Marketplace API
// =============================================================================
// Table of Contents:
// 1. Imports
// 2. Request/Response Types
// 3. Handlers - get_marketplace, get_featured_items, get_purchased_items,
//               purchase_item, get_marketplace_item
// =============================================================================

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::{extract_token, validate_token};
use crate::error::{ApiError, AppError};
use crate::AppState;

// =============================================================================
// 2. Request/Response Types
// =============================================================================

/// Query parameters for marketplace listing.
#[derive(Debug, Deserialize)]
pub struct MarketplaceQuery {
    /// Filter by category (e.g., "assets", "plugins", "scripts", "models").
    pub category: Option<String>,
    /// Search by item name.
    pub search: Option<String>,
    /// Maximum results to return (default 20, max 100).
    pub limit: Option<i64>,
    /// Offset for pagination (default 0).
    pub offset: Option<i64>,
}

/// A marketplace item response.
#[derive(Debug, Serialize)]
pub struct MarketplaceItemResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: String,
    pub price: i64,
    pub author_id: String,
    pub purchase_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response for paginated marketplace listings.
#[derive(Debug, Serialize)]
pub struct MarketplaceResponse {
    pub items: Vec<MarketplaceItemResponse>,
    pub count: usize,
}

/// Request body for purchasing an item.
#[derive(Debug, Deserialize)]
pub struct PurchaseRequest {
    pub item_id: String,
}

/// Response after a successful purchase.
#[derive(Debug, Serialize)]
pub struct PurchaseResponse {
    pub success: bool,
    pub item_id: String,
    pub price_paid: i64,
    pub remaining_balance: i64,
}

/// A purchased item summary.
#[derive(Debug, Serialize)]
pub struct PurchasedItemResponse {
    pub item_id: String,
    pub item_name: Option<String>,
    pub price_paid: i64,
    pub purchased_at: DateTime<Utc>,
}

// =============================================================================
// 3. Handlers
// =============================================================================

/// Get paginated marketplace items (public).
/// GET /api/marketplace?category=assets&search=tree&limit=20&offset=0
pub async fn get_marketplace(
    State(state): State<AppState>,
    Query(query): Query<MarketplaceQuery>,
) -> Result<Json<MarketplaceResponse>, ApiError> {
    let limit = query.limit.unwrap_or(20).min(100).max(1);
    let offset = query.offset.unwrap_or(0).max(0);
    
    let items = state.db.get_marketplace_items(
        query.category.as_deref(),
        query.search.as_deref(),
        limit,
        offset,
    )
    .await
    .map_err(|e| ApiError::Database(e.to_string()))?;
    
    let responses: Vec<MarketplaceItemResponse> = items.into_iter().map(|item| {
        MarketplaceItemResponse {
            id: item.id,
            name: item.name,
            description: item.description,
            category: item.category,
            price: item.price,
            author_id: item.author_id,
            purchase_count: item.purchase_count,
            created_at: item.created_at,
            updated_at: item.updated_at,
        }
    }).collect();
    
    let count = responses.len();
    Ok(Json(MarketplaceResponse { items: responses, count }))
}

/// Get featured marketplace items (public, ranked by purchase count).
/// GET /api/marketplace/featured
pub async fn get_featured_items(
    State(state): State<AppState>,
) -> Result<Json<MarketplaceResponse>, ApiError> {
    let items = state.db.get_featured_marketplace_items(12)
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?;
    
    let responses: Vec<MarketplaceItemResponse> = items.into_iter().map(|item| {
        MarketplaceItemResponse {
            id: item.id,
            name: item.name,
            description: item.description,
            category: item.category,
            price: item.price,
            author_id: item.author_id,
            purchase_count: item.purchase_count,
            created_at: item.created_at,
            updated_at: item.updated_at,
        }
    }).collect();
    
    let count = responses.len();
    Ok(Json(MarketplaceResponse { items: responses, count }))
}

/// Get items purchased by the authenticated user.
/// GET /api/marketplace/purchased
pub async fn get_purchased_items(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    let purchases = state.db.get_user_purchases(&claims.sub).await?;
    
    // Enrich with item names
    let mut items = Vec::with_capacity(purchases.len());
    for purchase in purchases {
        let item_name = state.db.find_marketplace_item_by_id(&purchase.item_id)
            .await
            .ok()
            .flatten()
            .map(|i| i.name);
        
        items.push(PurchasedItemResponse {
            item_id: purchase.item_id,
            item_name,
            price_paid: purchase.price_paid,
            purchased_at: purchase.purchased_at,
        });
    }
    
    Ok(Json(serde_json::json!({ "purchased": items })))
}

/// Purchase a marketplace item (authenticated, deducts bliss balance).
/// POST /api/marketplace/purchase
pub async fn purchase_item(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PurchaseRequest>,
) -> Result<Json<PurchaseResponse>, AppError> {
    let token = extract_token(&headers).ok_or(AppError::InvalidToken)?;
    let claims = validate_token(&token, &state.config.jwt_secret)?;
    
    // Find the item
    let item = state.db.find_marketplace_item_by_id(&req.item_id)
        .await?
        .ok_or(AppError::NotFound)?;
    
    // Check if already purchased
    let already_purchased = state.db.has_purchased(&claims.sub, &req.item_id)
        .await
        .unwrap_or(false);
    if already_purchased {
        return Err(AppError::Auth("Item already purchased".into()));
    }
    
    // Check balance
    let balance = state.db.get_user_balance(&claims.sub).await?;
    if balance < item.price {
        return Err(AppError::Auth("Insufficient bliss balance".into()));
    }
    
    // Execute purchase
    state.db.purchase_item(&claims.sub, &req.item_id, item.price).await?;
    
    let remaining = state.db.get_user_balance(&claims.sub).await?;
    
    Ok(Json(PurchaseResponse {
        success: true,
        item_id: req.item_id,
        price_paid: item.price,
        remaining_balance: remaining,
    }))
}

/// Get a single marketplace item by ID (public).
/// GET /api/marketplace/:id
pub async fn get_marketplace_item(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<MarketplaceItemResponse>, ApiError> {
    let item = state.db.find_marketplace_item_by_id(&id)
        .await
        .map_err(|e| ApiError::Database(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("Marketplace item '{}' not found", id)))?;
    
    Ok(Json(MarketplaceItemResponse {
        id: item.id,
        name: item.name,
        description: item.description,
        category: item.category,
        price: item.price,
        author_id: item.author_id,
        purchase_count: item.purchase_count,
        created_at: item.created_at,
        updated_at: item.updated_at,
    }))
}

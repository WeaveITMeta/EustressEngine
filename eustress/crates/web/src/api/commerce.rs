// =============================================================================
// Eustress Web - Commerce API
// =============================================================================
// Products a creator sells inside a published simulation, the sales they
// make, and the buyer's side of the same API (the pass store on a listing).
//
// Every signed-in commerce request names its mode with `Eustress-Mode`, as
// the CLI does: test mode shapes drafts and never moves Tickets; live mode
// puts products on sale, sells for real and refunds. A request that left the
// header out would be test mode, so none here does.
//
// Table of Contents:
// 1. Types
// 2. Requests
// 3. Creator: account, products, sales, events, webhooks
// 4. Buyer: catalog, passes, balance, buying
// =============================================================================

use gloo_net::http::Request;
use gloo_storage::Storage;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use super::ApiError;

// -----------------------------------------------------------------------------
// 1. Types
// -----------------------------------------------------------------------------

/// Which data a request acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommerceMode {
    Test,
    Live,
}

impl CommerceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Live => "live",
        }
    }
}

/// `{ object: "list", data, has_more }`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CommerceList<T> {
    #[serde(default = "Vec::new")]
    pub data: Vec<T>,
    #[serde(default)]
    pub has_more: bool,
}

/// `GET /api/commerce/account`: the caller and the simulations it can sell in.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CommerceAccount {
    pub id: String,
    #[serde(default)]
    pub livemode: bool,
    #[serde(default)]
    pub simulations: Vec<SellerSimulation>,
}

/// One of the creator's published simulations.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SellerSimulation {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// The published Spaces a product can be sold in.
    #[serde(default)]
    pub spaces: Vec<String>,
    /// Listed in the gallery: only a listed simulation sells to players.
    #[serde(default)]
    pub listed: bool,
    #[serde(default)]
    pub can_sell: bool,
    /// Why it cannot sell, when it cannot.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub products: Option<u64>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// A product a simulation sells.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CommerceProduct {
    pub id: String,
    /// What scripts pass to `PromptProductPurchase`.
    pub number: u64,
    #[serde(default)]
    pub sim_id: String,
    #[serde(default)]
    pub space: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// "consumable" or "pass".
    #[serde(rename = "type", default)]
    pub kind: String,
    /// Whole Tickets.
    #[serde(default)]
    pub price: u64,
    #[serde(default)]
    pub icon: Option<String>,
    /// On sale to players; a draft otherwise.
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub updated: i64,
}

impl CommerceProduct {
    pub fn is_pass(&self) -> bool {
        self.kind == "pass"
    }
}

/// The product as it was when a purchase was made.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ProductAtPurchase {
    #[serde(default)]
    pub number: u64,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: String,
}

/// A sale, as its creator sees it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CommerceSale {
    pub id: String,
    #[serde(default)]
    pub livemode: bool,
    /// "succeeded" or "refunded".
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub sim_id: String,
    #[serde(default)]
    pub space: Option<String>,
    #[serde(default)]
    pub buyer_id: String,
    #[serde(default)]
    pub product: ProductAtPurchase,
    /// Tickets the buyer paid.
    #[serde(default)]
    pub amount: u64,
    /// The creator's share.
    #[serde(default)]
    pub creator_amount: u64,
    /// The simulation granted it (`ProcessReceipt` answered).
    #[serde(default)]
    pub fulfilled: bool,
    #[serde(default)]
    pub created: i64,
    /// A test helper's made-up sale.
    #[serde(default)]
    pub synthetic: bool,
}

/// Something that happened to a product or a purchase.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CommerceEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub sim_id: Option<String>,
    #[serde(default)]
    pub data: Value,
}

/// Where the Worker posts events.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WebhookEndpoint {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub enabled_events: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: String,
    /// Only in the reply that created it: shown once.
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub last_delivery: Option<Value>,
}

/// A pass the signed-in player owns in a simulation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Entitlement {
    #[serde(default)]
    pub number: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Balance {
    #[serde(default)]
    tickets: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct PurchaseReply {
    purchase: CommerceSale,
}

// -----------------------------------------------------------------------------
// 2. Requests
// -----------------------------------------------------------------------------

fn token() -> Option<String> {
    gloo_storage::LocalStorage::get::<String>("auth_token").ok()
}

/// One request. `mode` is sent on every signed-in commerce call; the public
/// catalog read takes none.
async fn call<T: DeserializeOwned>(
    api_url: &str,
    method: &str,
    path: &str,
    mode: Option<CommerceMode>,
    body: Option<Value>,
) -> Result<T, ApiError> {
    let url = format!("{api_url}{path}");
    let mut request = match method {
        "POST" => Request::post(&url),
        "DELETE" => Request::delete(&url),
        _ => Request::get(&url),
    };
    if let Some(token) = token() {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    if let Some(mode) = mode {
        request = request.header("Eustress-Mode", mode.as_str());
    }
    let sent = match body {
        Some(body) => request.json(&body).map_err(|e| ApiError::Deserialize(e.to_string()))?.send().await,
        None => request.send().await,
    };
    let response = sent.map_err(|e| ApiError::Network(e.to_string()))?;
    let status = response.status();
    if (200..300).contains(&status) {
        return response.json::<T>().await.map_err(|e| ApiError::Deserialize(e.to_string()));
    }
    // The Worker explains a refusal in `error`; show that, not the JSON.
    let text = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .unwrap_or(text);
    match status {
        401 => Err(ApiError::Unauthorized),
        404 if message.is_empty() => Err(ApiError::NotFound),
        _ => Err(ApiError::Server { status, message }),
    }
}

/// What to show for a failed request: the Worker's own sentence when it gave
/// one.
pub fn commerce_error_text(error: &ApiError) -> String {
    match error {
        ApiError::Server { message, .. } if !message.is_empty() => message.clone(),
        ApiError::Unauthorized => "Your session has ended. Sign in again.".to_string(),
        other => other.to_string(),
    }
}

fn idempotency_key(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

// -----------------------------------------------------------------------------
// 3. Creator: account, products, sales, events, webhooks
// -----------------------------------------------------------------------------

pub async fn get_commerce_account(api_url: &str, mode: CommerceMode) -> Result<CommerceAccount, ApiError> {
    call(api_url, "GET", "/api/commerce/account", Some(mode), None).await
}

/// Every product of `sim_id`, drafts included.
pub async fn list_products(api_url: &str, mode: CommerceMode, sim_id: &str) -> Result<Vec<CommerceProduct>, ApiError> {
    let path = format!("/api/commerce/products?sim_id={}&limit=100", urlencoding::encode(sim_id));
    let list: CommerceList<CommerceProduct> = call(api_url, "GET", &path, Some(mode), None).await?;
    Ok(list.data)
}

/// A new product. In test mode it is a draft; putting it on sale is a live
/// change.
#[allow(clippy::too_many_arguments)]
pub async fn create_product(
    api_url: &str,
    mode: CommerceMode,
    sim_id: &str,
    space: &str,
    name: &str,
    kind: &str,
    price: u64,
    description: &str,
) -> Result<CommerceProduct, ApiError> {
    let body = json!({
        "sim_id": sim_id, "space": space, "name": name, "type": kind, "price": price,
        "description": description,
    });
    call(api_url, "POST", "/api/commerce/products", Some(mode), Some(body)).await
}

/// Change a product: any of `name`, `description`, `price`, `active`.
pub async fn update_product(api_url: &str, mode: CommerceMode, id: &str, patch: Value) -> Result<CommerceProduct, ApiError> {
    call(api_url, "POST", &format!("/api/commerce/products/{id}"), Some(mode), Some(patch)).await
}

/// The sales of `sim_id` in `mode`, newest first.
pub async fn list_sales(api_url: &str, mode: CommerceMode, sim_id: &str) -> Result<CommerceList<CommerceSale>, ApiError> {
    let path = format!("/api/commerce/purchases?sim_id={}&limit=100", urlencoding::encode(sim_id));
    call(api_url, "GET", &path, Some(mode), None).await
}

/// Give the buyer its Tickets back and take back the creator's share.
pub async fn refund_sale(api_url: &str, mode: CommerceMode, id: &str) -> Result<Value, ApiError> {
    call(api_url, "POST", &format!("/api/commerce/purchases/{id}/refund"), Some(mode), Some(json!({}))).await
}

/// The account's newest events in `mode`.
pub async fn list_events(api_url: &str, mode: CommerceMode) -> Result<Vec<CommerceEvent>, ApiError> {
    let list: CommerceList<CommerceEvent> = call(api_url, "GET", "/api/commerce/events?limit=100", Some(mode), None).await?;
    Ok(list.data)
}

pub async fn list_webhooks(api_url: &str, mode: CommerceMode) -> Result<Vec<WebhookEndpoint>, ApiError> {
    let list: CommerceList<WebhookEndpoint> = call(api_url, "GET", "/api/commerce/webhook_endpoints", Some(mode), None).await?;
    Ok(list.data)
}

/// A new endpoint for every event type. Its signing secret is in the reply,
/// and only there.
pub async fn create_webhook(api_url: &str, mode: CommerceMode, url: &str, description: &str) -> Result<WebhookEndpoint, ApiError> {
    let body = json!({ "url": url, "enabled_events": ["*"], "description": description });
    call(api_url, "POST", "/api/commerce/webhook_endpoints", Some(mode), Some(body)).await
}

pub async fn delete_webhook(api_url: &str, mode: CommerceMode, id: &str) -> Result<Value, ApiError> {
    call(api_url, "DELETE", &format!("/api/commerce/webhook_endpoints/{id}"), Some(mode), None).await
}

// -----------------------------------------------------------------------------
// 4. Buyer: catalog, passes, balance, buying
// -----------------------------------------------------------------------------

/// What a listed simulation has on sale. Public: no sign-in needed.
pub async fn get_catalog(api_url: &str, sim_id: &str) -> Result<Vec<CommerceProduct>, ApiError> {
    let list: CommerceList<CommerceProduct> = call(api_url, "GET", &format!("/api/commerce/catalog/{sim_id}"), None, None).await?;
    Ok(list.data)
}

/// The pass numbers the signed-in player owns in `sim_id`.
pub async fn get_owned_passes(api_url: &str, sim_id: &str) -> Result<Vec<u64>, ApiError> {
    let path = format!("/api/commerce/me/entitlements?sim_id={}", urlencoding::encode(sim_id));
    let list: CommerceList<Entitlement> = call(api_url, "GET", &path, Some(CommerceMode::Live), None).await?;
    Ok(list.data.into_iter().map(|e| e.number).collect())
}

/// The signed-in player's Tickets.
pub async fn get_ticket_balance(api_url: &str) -> Result<u64, ApiError> {
    let balance: Balance = call(api_url, "GET", "/api/commerce/me/balance", Some(CommerceMode::Live), None).await?;
    Ok(balance.tickets)
}

/// Buy `product` for the signed-in player, live, at the price the player was
/// shown: a changed price refuses the purchase rather than charge another.
pub async fn buy_product(api_url: &str, sim_id: &str, product: &CommerceProduct) -> Result<CommerceSale, ApiError> {
    let body = json!({
        "sim_id": sim_id,
        "product": product.number,
        "expected_price": product.price,
        "idempotency_key": idempotency_key("web"),
    });
    let reply: PurchaseReply = call(api_url, "POST", "/api/commerce/purchases", Some(CommerceMode::Live), Some(body)).await?;
    Ok(reply.purchase)
}

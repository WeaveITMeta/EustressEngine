//! # procurement
//!
//! Missing item detection and purchase list generation.
//! When a `BuildGuide` is resolved against the workshop registry and live status,
//! any tool or material that cannot be fulfilled becomes a `MissingItem`.
//! These are aggregated into a `PurchaseList` which can be exported to:
//!   - A human-readable Markdown shopping list
//!   - Amazon Product Advertising API v5 search links
//!   - An Alexa list (via Alexa Lists REST API) for voice-guided shopping
//!
//! ## Table of Contents
//!
//! | Section           | Purpose                                                         |
//! |-------------------|-----------------------------------------------------------------|
//! | `MissingItem`     | A single tool or material that needs to be acquired             |
//! | `PurchaseList`    | Aggregated shopping list from one or more guide resolutions     |
//! | `AmazonSearchLink`| Pre-signed Amazon PA-API v5 search URL for a missing item       |
//! | `AlexaListEntry`  | Payload for adding an item to an Alexa shopping list            |
//! | `ProcurementConfig`| API credentials and settings (loaded from .workshop/workshop.toml)|

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::guide::MissingReason;

// ============================================================================
// 1. MissingItem
// ============================================================================

/// A tool or material that is required but cannot be fulfilled from the current workshop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingItem {
    /// Unique identifier for this missing item entry
    pub id: Uuid,
    /// Human-readable name of the missing item
    pub name: String,
    /// The step number where this item is needed (1-based)
    pub step_index: u32,
    /// The title of the step that needs this item
    pub step_title: String,
    /// Why this item is missing
    pub reason: MissingReason,
    /// Quantity needed (applicable to materials; 1 for tools)
    pub quantity: f32,
    /// Unit of measure ("pcs", "kg", "m", etc.)
    pub unit: String,
    /// Search terms for finding this item to purchase
    pub search_terms: Vec<String>,
    /// Known Amazon ASIN if available from the tool's spec
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amazon_asin: Option<String>,
    /// Estimated cost in USD (if known from the tool's spec)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f32>,
    /// Whether the user has already marked this as handled (ordered / found)
    #[serde(default)]
    pub handled: bool,
}

impl MissingItem {
    /// Build a missing item from a `MissingRequirement` and optional spec data
    pub fn from_missing_requirement(
        step_index: u32,
        step_title: impl Into<String>,
        name: impl Into<String>,
        reason: MissingReason,
        search_terms: Vec<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            step_index,
            step_title: step_title.into(),
            reason,
            quantity: 1.0,
            unit: "pcs".into(),
            search_terms,
            amazon_asin: None,
            estimated_cost_usd: None,
            handled: false,
        }
    }

    /// Generate a primary Amazon search URL for this item
    pub fn amazon_search_url(&self) -> String {
        let query = if let Some(asin) = &self.amazon_asin {
            return format!("https://www.amazon.com/dp/{}", asin);
        } else {
            self.search_terms
                .first()
                .cloned()
                .unwrap_or_else(|| self.name.clone())
        };

        let encoded = query
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("+");
        format!("https://www.amazon.com/s?k={}&tag=eustress-20", encoded)
    }

    /// Format as a single line for a plain-text shopping list
    pub fn shopping_line(&self) -> String {
        if self.quantity > 1.0 {
            format!("[ ] {} × {} {} — {}", self.quantity, self.unit, self.name, self.amazon_search_url())
        } else {
            format!("[ ] {} — {}", self.name, self.amazon_search_url())
        }
    }
}

// ============================================================================
// 2. PurchaseList
// ============================================================================

/// Aggregated shopping list from one or more resolved build guides.
/// Deduplicates items by name so the same tool missing in multiple steps
/// appears only once in the purchase list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseList {
    /// Title of this purchase list (usually the guide title)
    pub title: String,
    /// All missing items — deduplicated by name
    pub items: Vec<MissingItem>,
    /// Total estimated cost (sum of items with known prices)
    pub estimated_total_usd: f32,
}

impl PurchaseList {
    /// Build a purchase list from a slice of missing items, deduplicating by name
    pub fn from_missing_items(title: impl Into<String>, items: Vec<MissingItem>) -> Self {
        // Deduplicate: keep first occurrence of each item name
        let mut seen = std::collections::HashSet::new();
        let deduplicated: Vec<MissingItem> = items
            .into_iter()
            .filter(|item| seen.insert(item.name.to_lowercase()))
            .collect();

        let estimated_total_usd = deduplicated
            .iter()
            .filter_map(|i| i.estimated_cost_usd)
            .sum();

        Self {
            title: title.into(),
            items: deduplicated,
            estimated_total_usd,
        }
    }

    /// Returns only items that have not been marked as handled
    pub fn unhandled_items(&self) -> Vec<&MissingItem> {
        self.items.iter().filter(|i| !i.handled).collect()
    }

    /// Mark an item as handled by its UUID
    pub fn mark_handled(&mut self, id: &Uuid) {
        if let Some(item) = self.items.iter_mut().find(|i| &i.id == id) {
            item.handled = true;
        }
    }

    /// Returns true if every item has been handled
    pub fn is_complete(&self) -> bool {
        self.items.iter().all(|i| i.handled)
    }

    /// Render as a Markdown shopping list document
    pub fn render_markdown(&self) -> String {
        let unhandled: Vec<&MissingItem> = self.unhandled_items();

        if unhandled.is_empty() {
            return format!("# {} — All items acquired ✓", self.title);
        }

        let mut lines = vec![
            format!("# {} — Shopping List", self.title),
            String::new(),
            format!(
                "*{} item{} needed{}*",
                unhandled.len(),
                if unhandled.len() == 1 { "" } else { "s" },
                if self.estimated_total_usd > 0.0 {
                    format!(" — estimated total ${:.2}", self.estimated_total_usd)
                } else {
                    String::new()
                }
            ),
            String::new(),
        ];

        // Group by step
        let mut by_step: std::collections::BTreeMap<u32, Vec<&MissingItem>> =
            std::collections::BTreeMap::new();
        for item in &unhandled {
            by_step.entry(item.step_index).or_default().push(item);
        }

        for (step_index, items) in by_step {
            let step_title = items[0].step_title.clone();
            lines.push(format!("## Step {}: {}", step_index, step_title));
            lines.push(String::new());
            for item in items {
                lines.push(item.shopping_line());
            }
            lines.push(String::new());
        }

        if self.estimated_total_usd > 0.0 {
            lines.push(format!(
                "---\n**Estimated total: ${:.2}**",
                self.estimated_total_usd
            ));
        }

        lines.join("\n")
    }

    /// Render as a JSON payload suitable for the Alexa Lists REST API.
    /// Sends unhandled items to the user's Alexa shopping list via the
    /// Alexa List Management API endpoint: POST /v2/householdlists/{listId}/items
    pub fn render_alexa_payload(&self) -> String {
        let items: Vec<serde_json::Value> = self
            .unhandled_items()
            .iter()
            .map(|item| {
                let value = if item.quantity > 1.0 {
                    format!("{} × {} {}", item.quantity, item.unit, item.name)
                } else {
                    item.name.clone()
                };
                serde_json::json!({
                    "value": value,
                    "status": "active",
                    "href": item.amazon_search_url(),
                })
            })
            .collect();

        serde_json::to_string_pretty(&serde_json::json!({ "items": items }))
            .unwrap_or_else(|_| "{}".into())
    }

    /// Returns a Vec of Amazon search URLs for all unhandled items.
    /// Used for bulk-opening browser tabs or sending to the Alexa app.
    pub fn amazon_links(&self) -> Vec<AmazonSearchLink> {
        self.unhandled_items()
            .iter()
            .map(|item| AmazonSearchLink {
                item_name: item.name.clone(),
                url: item.amazon_search_url(),
                asin: item.amazon_asin.clone(),
                estimated_cost_usd: item.estimated_cost_usd,
            })
            .collect()
    }
}

// ============================================================================
// 3. AmazonSearchLink
// ============================================================================

/// A pre-built Amazon search or product link for a missing item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmazonSearchLink {
    /// Human-readable item name
    pub item_name: String,
    /// Full Amazon URL (search results or direct ASIN product page)
    pub url: String,
    /// Direct ASIN if known — links straight to the product page
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asin: Option<String>,
    /// Estimated price in USD from the tool's spec (informational only)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f32>,
}

// ============================================================================
// 4. ProcurementConfig — loaded from .workshop/workshop.toml
// ============================================================================

/// API credentials and settings for procurement integrations.
/// Loaded from `.workshop/workshop.toml` in the workspace root.
/// Credentials are NEVER hardcoded — read from the TOML at runtime only.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcurementConfig {
    /// Amazon Product Advertising API v5 — Access Key ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amazon_access_key: Option<String>,
    /// Amazon PA-API v5 — Secret Access Key
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amazon_secret_key: Option<String>,
    /// Amazon PA-API v5 — Partner Tag (affiliate tag)
    #[serde(default)]
    pub amazon_partner_tag: String,
    /// Amazon PA-API v5 — Marketplace (default: "www.amazon.com")
    #[serde(default = "default_amazon_marketplace")]
    pub amazon_marketplace: String,
    /// Alexa List Management API — OAuth access token
    /// Obtained via Login with Amazon (LWA) OAuth 2.0 flow
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alexa_access_token: Option<String>,
    /// Alexa shopping list household list ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alexa_shopping_list_id: Option<String>,
    /// Whether to automatically send purchase lists to Alexa on guide resolution
    #[serde(default)]
    pub auto_send_to_alexa: bool,
    /// Whether to open Amazon links automatically in the browser on guide resolution
    #[serde(default)]
    pub auto_open_amazon_links: bool,
}

fn default_amazon_marketplace() -> String {
    "www.amazon.com".into()
}

impl ProcurementConfig {
    /// Returns true if Amazon PA-API credentials are configured
    pub fn has_amazon_credentials(&self) -> bool {
        self.amazon_access_key.is_some() && self.amazon_secret_key.is_some()
    }

    /// Returns true if Alexa integration is configured
    pub fn has_alexa_credentials(&self) -> bool {
        self.alexa_access_token.is_some() && self.alexa_shopping_list_id.is_some()
    }
}

// ============================================================================
// 5. AlexaListEntry — single item payload for the Alexa Lists REST API
// ============================================================================

/// A single item to be added to an Alexa household shopping list via
/// the Alexa List Management REST API
/// POST /v2/householdlists/{listId}/items
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlexaListEntry {
    /// Display value shown in the Alexa app and read by voice
    pub value: String,
    /// Status: always "active" for new items
    pub status: String,
}

impl AlexaListEntry {
    /// Build an Alexa list entry from a `MissingItem`
    pub fn from_missing_item(item: &MissingItem) -> Self {
        let value = if item.quantity > 1.0 {
            format!("{} {} - {}", item.quantity, item.unit, item.name)
        } else {
            item.name.clone()
        };
        Self {
            value,
            status: "active".into(),
        }
    }
}

// ============================================================================
// 6. HTTP client helpers (behind procurement feature flag)
// ============================================================================

#[cfg(feature = "procurement")]
pub mod client {
    //! HTTP client helpers for Amazon PA-API v5 and Alexa Lists REST API.
    //! All network calls are behind the `procurement` feature flag.

    use anyhow::{Context, Result};
    use super::{AlexaListEntry, ProcurementConfig, PurchaseList};

    /// Send all unhandled items from a purchase list to the user's Alexa shopping list.
    /// Requires `alexa_access_token` and `alexa_shopping_list_id` in `ProcurementConfig`.
    ///
    /// # Alexa Lists REST API
    /// Endpoint: POST https://api.amazonalexa.com/v2/householdlists/{listId}/items
    /// Auth: Bearer token (LWA OAuth 2.0)
    pub async fn send_to_alexa_list(
        config: &ProcurementConfig,
        list: &PurchaseList,
    ) -> Result<usize> {
        let token = config
            .alexa_access_token
            .as_deref()
            .context("Alexa access token not configured in workshop.toml")?;
        let list_id = config
            .alexa_shopping_list_id
            .as_deref()
            .context("Alexa shopping list ID not configured in workshop.toml")?;

        let client = reqwest::Client::new();
        let url = format!(
            "https://api.amazonalexa.com/v2/householdlists/{}/items",
            list_id
        );

        let mut sent = 0usize;
        for item in list.unhandled_items() {
            let entry = AlexaListEntry::from_missing_item(item);
            let response = client
                .post(&url)
                .bearer_auth(token)
                .json(&entry)
                .send()
                .await
                .with_context(|| format!("Failed to send '{}' to Alexa list", item.name))?;

            if response.status().is_success() {
                sent += 1;
                tracing::info!("Sent to Alexa list: {}", item.name);
            } else {
                tracing::warn!(
                    "Alexa list rejected '{}': HTTP {}",
                    item.name,
                    response.status()
                );
            }
        }
        Ok(sent)
    }

    /// Search Amazon for a single item using PA-API v5 search URL (no credentials needed).
    /// Returns the search URL without making any API call.
    /// For full PA-API v5 signed requests, credentials + HMAC-SHA256 SigV4 are required.
    pub fn amazon_search_url_simple(query: &str, partner_tag: &str) -> String {
        let encoded = query
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("+");
        if partner_tag.is_empty() {
            format!("https://www.amazon.com/s?k={}", encoded)
        } else {
            format!("https://www.amazon.com/s?k={}&tag={}", encoded, partner_tag)
        }
    }
}

// ============================================================================
// 7. Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_missing_item(name: &str) -> MissingItem {
        MissingItem::from_missing_requirement(
            1,
            "Drill pilot holes",
            name,
            MissingReason::ToolNotRegistered,
            vec![name.to_string()],
        )
    }

    #[test]
    fn purchase_list_deduplicates_by_name() {
        let items = vec![
            sample_missing_item("Milwaukee M18 Drill"),
            sample_missing_item("Milwaukee M18 Drill"), // duplicate
            sample_missing_item("Torque Wrench"),
        ];
        let list = PurchaseList::from_missing_items("Test Guide", items);
        assert_eq!(list.items.len(), 2);
    }

    #[test]
    fn amazon_search_url_uses_asin_when_present() {
        let mut item = sample_missing_item("Some Tool");
        item.amazon_asin = Some("B08XYZ123".into());
        assert_eq!(item.amazon_search_url(), "https://www.amazon.com/dp/B08XYZ123");
    }

    #[test]
    fn amazon_search_url_falls_back_to_search() {
        let item = sample_missing_item("Torque Wrench 3/8");
        let url = item.amazon_search_url();
        assert!(url.contains("amazon.com/s?k="));
        assert!(url.contains("Torque"));
    }

    #[test]
    fn purchase_list_markdown_contains_item_names() {
        let items = vec![sample_missing_item("Drill"), sample_missing_item("Wrench")];
        let list = PurchaseList::from_missing_items("Test", items);
        let md = list.render_markdown();
        assert!(md.contains("Drill"));
        assert!(md.contains("Wrench"));
    }

    #[test]
    fn alexa_payload_is_valid_json() {
        let items = vec![sample_missing_item("Drill")];
        let list = PurchaseList::from_missing_items("Test", items);
        let payload = list.render_alexa_payload();
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert!(parsed["items"].is_array());
    }
}

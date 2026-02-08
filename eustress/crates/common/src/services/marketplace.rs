//! # Marketplace Service
//!
//! In-app purchases, game passes, developer products, and virtual economy.
//! Inspired by Roblox MarketplaceService but with modern improvements.
//!
//! ## Features
//!
//! - **Game Passes**: One-time purchases for permanent perks
//! - **Developer Products**: Consumable purchases (coins, boosts)
//! - **Subscriptions**: Recurring payments for premium features
//! - **Virtual Currency**: In-game currency management
//! - **Receipts**: Secure purchase verification
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Check if player owns a game pass
//! if marketplace.owns_game_pass(player, GAME_PASS_VIP) {
//!     // Grant VIP perks
//! }
//!
//! // Prompt purchase
//! marketplace.prompt_purchase(player, ProductType::GamePass(GAME_PASS_VIP));
//!
//! // Handle developer product purchase
//! marketplace.on_product_purchased(|receipt| {
//!     match receipt.product_id {
//!         PRODUCT_100_COINS => player.coins += 100,
//!         _ => {}
//!     }
//!     PurchaseDecision::Grant
//! });
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tracing::info;

// ============================================================================
// Product Types
// ============================================================================

/// Unique identifier for products
pub type ProductId = u64;

/// Unique identifier for game passes
pub type GamePassId = u64;

/// Unique identifier for subscriptions
pub type SubscriptionId = u64;

/// Types of purchasable products
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ProductType {
    /// One-time purchase, permanent ownership
    GamePass(GamePassId),
    /// Consumable, can be purchased multiple times
    DeveloperProduct(ProductId),
    /// Recurring subscription
    Subscription(SubscriptionId),
    /// Virtual currency bundle
    CurrencyBundle { currency: String, amount: u64 },
}

/// Product information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductInfo {
    /// Product ID
    pub id: ProductId,
    /// Product type
    pub product_type: ProductType,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Price in platform currency (e.g., Robux equivalent)
    pub price: u64,
    /// Icon asset ID
    pub icon: Option<String>,
    /// Is currently for sale
    pub is_for_sale: bool,
    /// Creation timestamp
    pub created_at: u64,
    /// Last updated timestamp
    pub updated_at: u64,
}

/// Game pass information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamePassInfo {
    /// Game pass ID
    pub id: GamePassId,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Price
    pub price: u64,
    /// Icon asset ID
    pub icon: Option<String>,
    /// Is currently for sale
    pub is_for_sale: bool,
}

/// Subscription tier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionInfo {
    /// Subscription ID
    pub id: SubscriptionId,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Price per period
    pub price: u64,
    /// Billing period in days
    pub period_days: u32,
    /// Benefits/perks
    pub benefits: Vec<String>,
}

// ============================================================================
// Purchase Flow
// ============================================================================

/// Purchase receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseReceipt {
    /// Unique receipt ID
    pub receipt_id: String,
    /// Player who made the purchase
    pub player_id: u64,
    /// Product purchased
    pub product_type: ProductType,
    /// Product ID
    pub product_id: ProductId,
    /// Price paid
    pub price_paid: u64,
    /// Currency used
    pub currency: String,
    /// Purchase timestamp
    pub purchased_at: u64,
    /// Place ID where purchase was made
    pub place_id: u64,
    /// Server ID where purchase was made
    pub server_id: String,
}

/// Decision for handling a purchase
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurchaseDecision {
    /// Grant the purchase to the player
    Grant,
    /// Deny the purchase (refund)
    Deny,
    /// Defer decision (process later)
    Defer,
}

/// Purchase prompt result
#[derive(Debug, Clone)]
pub enum PromptResult {
    /// Purchase completed successfully
    Purchased(PurchaseReceipt),
    /// User cancelled
    Cancelled,
    /// Purchase failed
    Failed(String),
    /// User doesn't have enough currency
    InsufficientFunds,
    /// Product not available
    NotAvailable,
}

// ============================================================================
// Events
// ============================================================================

/// Event: Purchase prompt requested
#[derive(Message, Debug, Clone)]
pub struct PromptPurchaseEvent {
    /// Player entity
    pub player: Entity,
    /// Product to purchase
    pub product_type: ProductType,
}

/// Event: Purchase completed
#[derive(Message, Debug, Clone)]
pub struct PurchaseCompletedEvent {
    /// Player entity
    pub player: Entity,
    /// Purchase receipt
    pub receipt: PurchaseReceipt,
}

/// Event: Purchase prompt closed
#[derive(Message, Debug, Clone)]
pub struct PromptClosedEvent {
    /// Player entity
    pub player: Entity,
    /// Result
    pub result: PromptResult,
}

/// Event: Subscription status changed
#[derive(Message, Debug, Clone)]
pub struct SubscriptionChangedEvent {
    /// Player entity
    pub player: Entity,
    /// Subscription ID
    pub subscription_id: SubscriptionId,
    /// Is now active
    pub is_active: bool,
}

// ============================================================================
// Marketplace Service Resource
// ============================================================================

/// MarketplaceService - manages in-app purchases and virtual economy
#[derive(Resource)]
pub struct MarketplaceService {
    /// Registered products
    products: HashMap<ProductId, ProductInfo>,
    /// Registered game passes
    game_passes: HashMap<GamePassId, GamePassInfo>,
    /// Registered subscriptions
    subscriptions: HashMap<SubscriptionId, SubscriptionInfo>,
    /// Player game pass ownership (player_id -> set of owned passes)
    player_passes: HashMap<u64, HashSet<GamePassId>>,
    /// Player subscription status (player_id -> subscription_id -> expiry)
    player_subscriptions: HashMap<u64, HashMap<SubscriptionId, u64>>,
    /// Pending receipts awaiting processing
    pending_receipts: Vec<PurchaseReceipt>,
    /// Receipt handler callback
    receipt_handler: Option<Box<dyn Fn(&PurchaseReceipt) -> PurchaseDecision + Send + Sync>>,
}

impl Default for MarketplaceService {
    fn default() -> Self {
        Self {
            products: HashMap::new(),
            game_passes: HashMap::new(),
            subscriptions: HashMap::new(),
            player_passes: HashMap::new(),
            player_subscriptions: HashMap::new(),
            pending_receipts: Vec::new(),
            receipt_handler: None,
        }
    }
}

impl MarketplaceService {
    // ========================================================================
    // Product Registration
    // ========================================================================
    
    /// Register a developer product
    pub fn register_product(&mut self, info: ProductInfo) {
        self.products.insert(info.id, info);
    }
    
    /// Register a game pass
    pub fn register_game_pass(&mut self, info: GamePassInfo) {
        self.game_passes.insert(info.id, info);
    }
    
    /// Register a subscription
    pub fn register_subscription(&mut self, info: SubscriptionInfo) {
        self.subscriptions.insert(info.id, info);
    }
    
    /// Get product info
    pub fn get_product_info(&self, id: ProductId) -> Option<&ProductInfo> {
        self.products.get(&id)
    }
    
    /// Get game pass info
    pub fn get_game_pass_info(&self, id: GamePassId) -> Option<&GamePassInfo> {
        self.game_passes.get(&id)
    }
    
    /// Get subscription info
    pub fn get_subscription_info(&self, id: SubscriptionId) -> Option<&SubscriptionInfo> {
        self.subscriptions.get(&id)
    }
    
    // ========================================================================
    // Ownership Checks
    // ========================================================================
    
    /// Check if player owns a game pass
    pub fn owns_game_pass(&self, player_id: u64, pass_id: GamePassId) -> bool {
        self.player_passes
            .get(&player_id)
            .map(|passes| passes.contains(&pass_id))
            .unwrap_or(false)
    }
    
    /// Check if player has active subscription
    pub fn has_subscription(&self, player_id: u64, sub_id: SubscriptionId) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        self.player_subscriptions
            .get(&player_id)
            .and_then(|subs| subs.get(&sub_id))
            .map(|&expiry| expiry > now)
            .unwrap_or(false)
    }
    
    /// Get all game passes owned by player
    pub fn get_owned_passes(&self, player_id: u64) -> Vec<GamePassId> {
        self.player_passes
            .get(&player_id)
            .map(|passes| passes.iter().copied().collect())
            .unwrap_or_default()
    }
    
    /// Get all active subscriptions for player
    pub fn get_active_subscriptions(&self, player_id: u64) -> Vec<SubscriptionId> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        self.player_subscriptions
            .get(&player_id)
            .map(|subs| {
                subs.iter()
                    .filter(|(_, &expiry)| expiry > now)
                    .map(|(&id, _)| id)
                    .collect()
            })
            .unwrap_or_default()
    }
    
    // ========================================================================
    // Purchase Flow
    // ========================================================================
    
    /// Grant a game pass to a player (server-side)
    pub fn grant_game_pass(&mut self, player_id: u64, pass_id: GamePassId) {
        self.player_passes
            .entry(player_id)
            .or_default()
            .insert(pass_id);
    }
    
    /// Grant subscription to a player (server-side)
    pub fn grant_subscription(&mut self, player_id: u64, sub_id: SubscriptionId, duration_days: u32) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let expiry = now + (duration_days as u64 * 24 * 60 * 60);
        
        self.player_subscriptions
            .entry(player_id)
            .or_default()
            .insert(sub_id, expiry);
    }
    
    /// Set receipt handler callback
    pub fn set_receipt_handler<F>(&mut self, handler: F)
    where
        F: Fn(&PurchaseReceipt) -> PurchaseDecision + Send + Sync + 'static,
    {
        self.receipt_handler = Some(Box::new(handler));
    }
    
    /// Process a purchase receipt
    pub fn process_receipt(&mut self, receipt: PurchaseReceipt) -> PurchaseDecision {
        if let Some(handler) = &self.receipt_handler {
            let decision = handler(&receipt);
            
            if decision == PurchaseDecision::Grant {
                // Auto-grant game passes
                if let ProductType::GamePass(pass_id) = receipt.product_type {
                    self.grant_game_pass(receipt.player_id, pass_id);
                }
            }
            
            decision
        } else {
            // No handler, defer
            self.pending_receipts.push(receipt);
            PurchaseDecision::Defer
        }
    }
    
    /// Get pending receipts
    pub fn get_pending_receipts(&mut self) -> Vec<PurchaseReceipt> {
        std::mem::take(&mut self.pending_receipts)
    }
    
    // ========================================================================
    // Price Calculation
    // ========================================================================
    
    /// Calculate price with any discounts
    pub fn calculate_price(&self, product_type: &ProductType) -> Option<u64> {
        match product_type {
            ProductType::GamePass(id) => self.game_passes.get(id).map(|p| p.price),
            ProductType::DeveloperProduct(id) => self.products.get(id).map(|p| p.price),
            ProductType::Subscription(id) => self.subscriptions.get(id).map(|s| s.price),
            ProductType::CurrencyBundle { amount, .. } => Some(*amount / 10), // Example: 10 currency per 1 price unit
        }
    }
}

// ============================================================================
// Virtual Currency
// ============================================================================

/// Virtual currency balance for a player
#[derive(Component, Debug, Clone, Default, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
pub struct CurrencyBalance {
    /// Balances by currency type
    pub balances: HashMap<String, u64>,
}

impl CurrencyBalance {
    /// Get balance for a currency
    pub fn get(&self, currency: &str) -> u64 {
        self.balances.get(currency).copied().unwrap_or(0)
    }
    
    /// Add to balance
    pub fn add(&mut self, currency: &str, amount: u64) {
        *self.balances.entry(currency.to_string()).or_default() += amount;
    }
    
    /// Subtract from balance (returns false if insufficient)
    pub fn subtract(&mut self, currency: &str, amount: u64) -> bool {
        if let Some(balance) = self.balances.get_mut(currency) {
            if *balance >= amount {
                *balance -= amount;
                return true;
            }
        }
        false
    }
    
    /// Check if has enough
    pub fn has(&self, currency: &str, amount: u64) -> bool {
        self.get(currency) >= amount
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// Marketplace plugin for Bevy
pub struct MarketplacePlugin;

impl Plugin for MarketplacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MarketplaceService>()
            .register_type::<CurrencyBalance>()
            .add_message::<PromptPurchaseEvent>()
            .add_message::<PurchaseCompletedEvent>()
            .add_message::<PromptClosedEvent>()
            .add_message::<SubscriptionChangedEvent>();
        
        info!("MarketplaceService initialized");
    }
}

// ============================================================================
// Common Product IDs (Example)
// ============================================================================

/// Example game pass IDs
pub mod game_passes {
    use super::GamePassId;
    
    pub const VIP: GamePassId = 1;
    pub const DOUBLE_COINS: GamePassId = 2;
    pub const RADIO: GamePassId = 3;
    pub const EXTRA_INVENTORY: GamePassId = 4;
}

/// Example developer product IDs
pub mod products {
    use super::ProductId;
    
    pub const COINS_100: ProductId = 101;
    pub const COINS_500: ProductId = 102;
    pub const COINS_1000: ProductId = 103;
    pub const SPEED_BOOST: ProductId = 201;
    pub const REVIVE: ProductId = 202;
}

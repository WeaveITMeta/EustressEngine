# Bliss Currency System

**Platform Currency for Eustress Engine**

> *Arcade-style tokens for cosmetics, creator support, and marketplace purchases*

**Last Updated:** December 03, 2025  
**Status:** Pre-Release Design  
**Steam Compliance:** Required

---

## Table of Contents

1. [Overview](#overview)
2. [Currency Packages](#currency-packages)
3. [Steam Integration](#steam-integration)
4. [DataStore Architecture](#datastore-architecture)
5. [Spending Mechanics](#spending-mechanics)
6. [Child Safety](#child-safety)

---

## Overview

### What is Bliss?

Bliss is Eustress Engine's platform currency—arcade-style tokens purchased via Steam Wallet and spent on cosmetics, creator assets, and profile customization.

**Core Principles:**
- **No pay-to-win** — Bliss purchases cosmetics only, never gameplay advantages
- **Steam-native** — All purchases through Steam Wallet (30% platform fee)
- **Creator economy** — Players can tip creators and buy marketplace assets
- **Child-safe** — Purchases blocked for children, parental approval for teens

---

## Currency Packages

### One-Time Purchases (Steam Wallet)

| Package | Bliss | USD | Bonus | Bliss/$ |
|---------|-------|-----|-------|---------|
| **Starter** | 100 | $0.99 | — | 101 |
| **Standard** | 500 | $4.99 | — | 100 |
| **Plus** | 1,100 | $9.99 | +10% | 110 |
| **Premium** | 2,400 | $19.99 | +20% | 120 |
| **Ultimate** | 6,500 | $49.99 | +30% | 130 |

### Revenue Breakdown (per package sold)

| Package | Gross | Steam Cut (30%) | Net Revenue |
|---------|-------|-----------------|-------------|
| Starter | $0.99 | $0.30 | $0.69 |
| Standard | $4.99 | $1.50 | $3.49 |
| Plus | $9.99 | $3.00 | $6.99 |
| Premium | $19.99 | $6.00 | $13.99 |
| Ultimate | $49.99 | $15.00 | $34.99 |

---

## Steam Integration

### Steamworks API Integration

```rust
// crates/services/src/monetization/steam_iap.rs
use steamworks::{Client, SteamId};

/// Steam In-App Purchase handler
pub struct SteamIapService {
    client: Client,
    db: sqlx::PgPool,
}

/// Bliss package definitions (must match Steam inventory)
#[derive(Debug, Clone, Copy)]
pub enum BlissPackage {
    Starter,    // 100 Bliss - $0.99
    Standard,   // 500 Bliss - $4.99
    Plus,       // 1,100 Bliss - $9.99
    Premium,    // 2,400 Bliss - $19.99
    Ultimate,   // 6,500 Bliss - $49.99
}

impl BlissPackage {
    pub fn bliss_amount(&self) -> u64 {
        match self {
            BlissPackage::Starter => 100,
            BlissPackage::Standard => 500,
            BlissPackage::Plus => 1_100,
            BlissPackage::Premium => 2_400,
            BlissPackage::Ultimate => 6_500,
        }
    }
    
    pub fn price_cents(&self) -> u32 {
        match self {
            BlissPackage::Starter => 99,
            BlissPackage::Standard => 499,
            BlissPackage::Plus => 999,
            BlissPackage::Premium => 1999,
            BlissPackage::Ultimate => 4999,
        }
    }
    
    pub fn steam_item_def_id(&self) -> u32 {
        match self {
            BlissPackage::Starter => 1001,
            BlissPackage::Standard => 1002,
            BlissPackage::Plus => 1003,
            BlissPackage::Premium => 1004,
            BlissPackage::Ultimate => 1005,
        }
    }
}

impl SteamIapService {
    /// Initiate purchase via Steam Overlay
    pub async fn initiate_purchase(
        &self,
        steam_id: SteamId,
        package: BlissPackage,
    ) -> Result<PurchaseSession, IapError> {
        // Verify user is not a child (COPPA compliance)
        let user = self.get_user(steam_id).await?;
        if user.is_child {
            return Err(IapError::ChildPurchaseBlocked);
        }
        if user.is_teen && !user.parental_purchase_approval {
            return Err(IapError::ParentalApprovalRequired);
        }
        
        // Create pending transaction
        let session = PurchaseSession {
            session_id: uuid::Uuid::new_v4().to_string(),
            steam_id: steam_id.raw(),
            package,
            status: PurchaseStatus::Pending,
            created_at: chrono::Utc::now(),
        };
        
        // Store pending transaction
        self.store_pending_purchase(&session).await?;
        
        // Steam overlay will handle the actual purchase
        // We receive callback via Steam API
        Ok(session)
    }
    
    /// Callback from Steam when purchase completes
    pub async fn on_purchase_complete(
        &self,
        steam_id: SteamId,
        order_id: u64,
        item_def_id: u32,
    ) -> Result<BlissGrant, IapError> {
        // Verify with Steam servers
        let verified = self.verify_purchase_with_steam(order_id).await?;
        if !verified {
            return Err(IapError::VerificationFailed);
        }
        
        // Determine package from item def
        let package = BlissPackage::from_item_def(item_def_id)?;
        
        // Grant Bliss to user
        let grant = self.grant_bliss(steam_id, package.bliss_amount(), order_id).await?;
        
        // Record transaction
        self.record_transaction(steam_id, package, order_id).await?;
        
        Ok(grant)
    }
}

#[derive(Debug, Clone)]
pub struct BlissGrant {
    pub user_id: String,
    pub amount: u64,
    pub new_balance: u64,
    pub transaction_id: String,
}

#[derive(Debug, Clone)]
pub enum PurchaseStatus {
    Pending,
    Completed,
    Failed,
    Refunded,
}

#[derive(Debug)]
pub enum IapError {
    ChildPurchaseBlocked,
    ParentalApprovalRequired,
    VerificationFailed,
    SteamApiError(String),
    DatabaseError(String),
}
```

### Steam Inventory Service Setup

```json
// steamworks/item_definitions.json
{
  "appid": 123456,
  "items": [
    {
      "itemdefid": 1001,
      "type": "item",
      "name": "Bliss - Starter Pack",
      "description": "100 Bliss tokens",
      "price": "99;USD",
      "tradable": false,
      "marketable": false
    },
    {
      "itemdefid": 1002,
      "type": "item", 
      "name": "Bliss - Standard Pack",
      "description": "500 Bliss tokens",
      "price": "499;USD",
      "tradable": false,
      "marketable": false
    },
    {
      "itemdefid": 1003,
      "type": "item",
      "name": "Bliss - Plus Pack",
      "description": "1,100 Bliss tokens (+10% bonus)",
      "price": "999;USD",
      "tradable": false,
      "marketable": false
    },
    {
      "itemdefid": 1004,
      "type": "item",
      "name": "Bliss - Premium Pack",
      "description": "2,400 Bliss tokens (+20% bonus)",
      "price": "1999;USD",
      "tradable": false,
      "marketable": false
    },
    {
      "itemdefid": 1005,
      "type": "item",
      "name": "Bliss - Ultimate Pack",
      "description": "6,500 Bliss tokens (+30% bonus)",
      "price": "4999;USD",
      "tradable": false,
      "marketable": false
    }
  ]
}
```

---

## DataStore Architecture

### Bliss Balance Storage

```rust
// crates/services/src/datastore/bliss.rs

/// Bliss balance for a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlissBalance {
    pub user_id: String,
    pub balance: u64,
    pub lifetime_purchased: u64,
    pub lifetime_spent: u64,
    pub lifetime_earned: u64,  // From creator revenue
    pub last_updated: DateTime<Utc>,
}

/// Bliss DataStore service
pub struct BlissDataStore {
    db: sqlx::PgPool,
    cache: redis::aio::ConnectionManager,
}

impl BlissDataStore {
    /// Get user's Bliss balance
    pub async fn get_balance(&self, user_id: &str) -> Result<u64, DataStoreError> {
        // Check cache first
        if let Some(cached) = self.cache.get(&format!("bliss:{}", user_id)).await? {
            return Ok(cached);
        }
        
        // Fetch from database
        let balance = sqlx::query_scalar!(
            "SELECT balance FROM bliss_balances WHERE user_id = $1",
            user_id
        )
        .fetch_optional(&self.db)
        .await?
        .unwrap_or(0);
        
        // Cache for 5 minutes
        self.cache.set_ex(&format!("bliss:{}", user_id), balance, 300).await?;
        
        Ok(balance as u64)
    }
    
    /// Add Bliss to user's balance (purchase or earning)
    pub async fn credit(
        &self,
        user_id: &str,
        amount: u64,
        reason: CreditReason,
        reference_id: &str,
    ) -> Result<u64, DataStoreError> {
        let mut tx = self.db.begin().await?;
        
        // Upsert balance
        let new_balance = sqlx::query_scalar!(
            r#"
            INSERT INTO bliss_balances (user_id, balance, lifetime_purchased, lifetime_earned)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (user_id) DO UPDATE SET
                balance = bliss_balances.balance + $2,
                lifetime_purchased = bliss_balances.lifetime_purchased + $3,
                lifetime_earned = bliss_balances.lifetime_earned + $4,
                last_updated = NOW()
            RETURNING balance
            "#,
            user_id,
            amount as i64,
            if matches!(reason, CreditReason::Purchase { .. }) { amount as i64 } else { 0 },
            if matches!(reason, CreditReason::CreatorRevenue { .. }) { amount as i64 } else { 0 },
        )
        .fetch_one(&mut *tx)
        .await?;
        
        // Record transaction
        sqlx::query!(
            r#"
            INSERT INTO bliss_transactions (user_id, amount, transaction_type, reason, reference_id)
            VALUES ($1, $2, 'credit', $3, $4)
            "#,
            user_id,
            amount as i64,
            serde_json::to_string(&reason)?,
            reference_id,
        )
        .execute(&mut *tx)
        .await?;
        
        tx.commit().await?;
        
        // Invalidate cache
        self.cache.del(&format!("bliss:{}", user_id)).await?;
        
        Ok(new_balance as u64)
    }
    
    /// Deduct Bliss from user's balance (spending)
    pub async fn debit(
        &self,
        user_id: &str,
        amount: u64,
        reason: DebitReason,
        reference_id: &str,
    ) -> Result<u64, DataStoreError> {
        let mut tx = self.db.begin().await?;
        
        // Check sufficient balance
        let current = sqlx::query_scalar!(
            "SELECT balance FROM bliss_balances WHERE user_id = $1 FOR UPDATE",
            user_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .unwrap_or(0);
        
        if (current as u64) < amount {
            return Err(DataStoreError::InsufficientBalance {
                required: amount,
                available: current as u64,
            });
        }
        
        // Deduct balance
        let new_balance = sqlx::query_scalar!(
            r#"
            UPDATE bliss_balances 
            SET balance = balance - $2,
                lifetime_spent = lifetime_spent + $2,
                last_updated = NOW()
            WHERE user_id = $1
            RETURNING balance
            "#,
            user_id,
            amount as i64,
        )
        .fetch_one(&mut *tx)
        .await?;
        
        // Record transaction
        sqlx::query!(
            r#"
            INSERT INTO bliss_transactions (user_id, amount, transaction_type, reason, reference_id)
            VALUES ($1, $2, 'debit', $3, $4)
            "#,
            user_id,
            amount as i64,
            serde_json::to_string(&reason)?,
            reference_id,
        )
        .execute(&mut *tx)
        .await?;
        
        tx.commit().await?;
        
        // Invalidate cache
        self.cache.del(&format!("bliss:{}", user_id)).await?;
        
        Ok(new_balance as u64)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CreditReason {
    Purchase { order_id: String, package: String },
    CreatorRevenue { asset_id: String, buyer_id: String },
    SubscriptionAllowance { month: String },
    Refund { original_transaction: String },
    Promotion { campaign: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DebitReason {
    MarketplacePurchase { asset_id: String, seller_id: String },
    CosmeticPurchase { item_id: String },
    CreatorTip { creator_id: String },
    ProfileCustomization { item_id: String },
}
```

### Database Schema

```sql
-- migrations/20241203_bliss_currency.sql

CREATE TABLE bliss_balances (
    user_id VARCHAR(64) PRIMARY KEY,
    balance BIGINT NOT NULL DEFAULT 0,
    lifetime_purchased BIGINT NOT NULL DEFAULT 0,
    lifetime_spent BIGINT NOT NULL DEFAULT 0,
    lifetime_earned BIGINT NOT NULL DEFAULT 0,
    last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    CONSTRAINT positive_balance CHECK (balance >= 0)
);

CREATE TABLE bliss_transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id VARCHAR(64) NOT NULL REFERENCES bliss_balances(user_id),
    amount BIGINT NOT NULL,
    transaction_type VARCHAR(16) NOT NULL, -- 'credit' or 'debit'
    reason JSONB NOT NULL,
    reference_id VARCHAR(128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    CONSTRAINT valid_type CHECK (transaction_type IN ('credit', 'debit'))
);

CREATE INDEX idx_bliss_transactions_user ON bliss_transactions(user_id);
CREATE INDEX idx_bliss_transactions_created ON bliss_transactions(created_at);
CREATE INDEX idx_bliss_transactions_reference ON bliss_transactions(reference_id);
```

---

## Spending Mechanics

### What Bliss Buys

| Category | Example Items | Price Range |
|----------|---------------|-------------|
| **Cosmetics** | Skins, effects, emotes | 50-500 Bliss |
| **Profile** | Banners, frames, badges | 25-200 Bliss |
| **Marketplace** | Creator assets, models, scripts | 10-10,000 Bliss |
| **Creator Tips** | Support favorite creators | 10+ Bliss |

### Marketplace Revenue Split

When a user buys a creator's asset:

| Recipient | Default | Creator Pro |
|-----------|---------|-------------|
| Creator | 25% | 40% |
| Platform | 75% | 60% |

```rust
/// Calculate revenue split for marketplace sale
pub fn calculate_revenue_split(
    sale_amount: u64,
    seller_is_pro: bool,
) -> RevenueSplit {
    let creator_share = if seller_is_pro { 0.40 } else { 0.25 };
    let platform_share = 1.0 - creator_share;
    
    RevenueSplit {
        creator_amount: (sale_amount as f64 * creator_share).floor() as u64,
        platform_amount: (sale_amount as f64 * platform_share).ceil() as u64,
    }
}
```

---

## Child Safety

### Purchase Restrictions

```rust
/// Validate user can make purchases
pub async fn validate_purchase_eligibility(
    user: &User,
    db: &PgPool,
) -> Result<(), PurchaseError> {
    // Children cannot purchase
    if user.is_child {
        return Err(PurchaseError::ChildPurchaseBlocked);
    }
    
    // Teens need parental approval
    if user.is_teen {
        let parental_settings = get_parental_settings(db, &user.id).await?;
        if !parental_settings.allow_purchases {
            return Err(PurchaseError::ParentalApprovalRequired);
        }
        
        // Check spending limits
        let monthly_spent = get_monthly_spending(db, &user.id).await?;
        if monthly_spent + amount > parental_settings.monthly_limit {
            return Err(PurchaseError::SpendingLimitExceeded {
                limit: parental_settings.monthly_limit,
                spent: monthly_spent,
            });
        }
    }
    
    Ok(())
}
```

---

## Contact

**Monetization Questions:** monetization@simbuilder.com  
**Steam Integration:** steam-support@simbuilder.com

# Subscription Plans

**Premium Subscriptions for Eustress Engine**

> *Player Plus, Creator Pro, and Bundle subscriptions via Steam*

**Last Updated:** December 03, 2025  
**Status:** Pre-Release Design  
**Steam Compliance:** Required

---

## Table of Contents

1. [Overview](#overview)
2. [Subscription Tiers](#subscription-tiers)
3. [Player Plus](#player-plus)
4. [Creator Pro](#creator-pro)
5. [Bundle](#bundle)
6. [Steam Integration](#steam-integration)
7. [Cost Analysis](#cost-analysis)
8. [Profitability Model](#profitability-model)

---

## Overview

### Subscription Philosophy

- **Value-first** — Subscriptions enhance experience, never gate core features
- **No pay-to-win** — Zero gameplay advantages
- **Creator-focused** — Better revenue share for serious creators
- **Sustainable** — Pricing covers infrastructure costs with healthy margin

---

## Subscription Tiers

| Plan | Monthly | Annual | Savings |
|------|---------|--------|---------|
| **Player Plus** | $4.99 | $49.99 | 17% |
| **Creator Pro** | $9.99 | $99.99 | 17% |
| **Bundle** | $12.99 | $129.99 | 17% |

### Revenue After Steam Cut (30%)

| Plan | Monthly Net | Annual Net |
|------|-------------|------------|
| Player Plus | $3.49 | $34.99 |
| Creator Pro | $6.99 | $69.99 |
| Bundle | $9.09 | $90.99 |

---

## Player Plus

### Benefits

| Feature | Free | Player Plus |
|---------|------|-------------|
| **Monthly Bliss** | 0 | 500 Bliss/month |
| **Profile Badge** | — | ✅ Exclusive badge |
| **Profile Flair** | Basic | Premium options |
| **Subscriber Cosmetics** | — | ✅ Exclusive items |
| **Priority Queue** | Standard | Priority matchmaking |
| **Subscriber Events** | — | ✅ Exclusive events |
| **Cloud Saves** | 1 GB | 10 GB |

### Implementation

```rust
// crates/services/src/subscriptions/player_plus.rs

#[derive(Debug, Clone)]
pub struct PlayerPlusBenefits {
    /// Monthly Bliss allowance
    pub monthly_bliss: u64,
    
    /// Cloud save storage limit (bytes)
    pub cloud_storage_bytes: u64,
    
    /// Access to subscriber cosmetics
    pub subscriber_cosmetics: bool,
    
    /// Priority matchmaking
    pub priority_queue: bool,
    
    /// Subscriber-only events
    pub subscriber_events: bool,
    
    /// Premium profile options
    pub premium_profile: bool,
}

impl Default for PlayerPlusBenefits {
    fn default() -> Self {
        Self {
            monthly_bliss: 500,
            cloud_storage_bytes: 10 * 1024 * 1024 * 1024, // 10 GB
            subscriber_cosmetics: true,
            priority_queue: true,
            subscriber_events: true,
            premium_profile: true,
        }
    }
}

/// Grant monthly Bliss allowance to Player Plus subscribers
pub async fn grant_monthly_bliss(
    db: &PgPool,
    bliss_store: &BlissDataStore,
) -> Result<u64, SubscriptionError> {
    // Find all active Player Plus subscribers
    let subscribers = sqlx::query!(
        r#"
        SELECT user_id FROM subscriptions 
        WHERE plan IN ('player_plus', 'bundle')
          AND status = 'active'
          AND NOT EXISTS (
              SELECT 1 FROM bliss_transactions 
              WHERE user_id = subscriptions.user_id
                AND reason->>'type' = 'subscription_allowance'
                AND DATE_TRUNC('month', created_at) = DATE_TRUNC('month', NOW())
          )
        "#
    )
    .fetch_all(db)
    .await?;
    
    let mut granted = 0;
    for sub in subscribers {
        bliss_store.credit(
            &sub.user_id,
            500,
            CreditReason::SubscriptionAllowance {
                month: chrono::Utc::now().format("%Y-%m").to_string(),
            },
            &format!("sub_allowance_{}", chrono::Utc::now().format("%Y%m")),
        ).await?;
        granted += 1;
    }
    
    Ok(granted)
}
```

---

## Creator Pro

### Benefits

| Feature | Free | Creator Pro |
|---------|------|-------------|
| **Asset Storage** | 10 GB | 1TB* |
| **Revenue Share** | 25% | 40% |
| **Publishing Queue** | Standard | Priority review |
| **Analytics** | Basic | Advanced dashboard |
| **Priority Support** | — | ✅ 24hr response |
| **Monthly Bliss** | 0 | 500 Bliss/month |

### Revenue Share Comparison

| Sale Price | Free Creator (25%) | Creator Pro (40%) | Difference |
|------------|-------------------|-------------------|------------|
| 100 Bliss | 25 Bliss | 40 Bliss | +15 |
| 500 Bliss | 125 Bliss | 200 Bliss | +75 |
| 1,000 Bliss | 250 Bliss | 400 Bliss | +150 |
| 5,000 Bliss | 1,250 Bliss | 2,000 Bliss | +750 |

### Break-Even Analysis

Creator Pro costs $9.99/month ($6.99 net after Steam).

To break even on the 15% revenue increase:
- Need to earn 466 Bliss/month in additional revenue
- At 1,000 Bliss average sale: ~3 sales/month to break even
- At 500 Bliss average sale: ~6 sales/month to break even

### Implementation

```rust
// crates/services/src/subscriptions/creator_pro.rs

#[derive(Debug, Clone)]
pub struct CreatorProBenefits {
    /// Revenue share percentage (0.0 to 1.0)
    pub revenue_share: f64,
    
    /// Asset storage limit (bytes), None = unlimited
    pub storage_limit_bytes: Option<u64>,
    
    /// Priority publishing queue
    pub priority_publishing: bool,
    
    /// Advanced analytics access
    pub advanced_analytics: bool,
    
    /// Early access to beta features
    pub early_access: bool,
    
    /// Priority support (24hr response)
    pub priority_support: bool,
    
    /// Monthly Bliss allowance
    pub monthly_bliss: u64,
}

impl Default for CreatorProBenefits {
    fn default() -> Self {
        Self {
            revenue_share: 0.40,  // 40% to creator
            storage_limit_bytes: Some(1024 * 1024 * 1024 * 1024), // 1 TB soft cap
            priority_publishing: true,
            advanced_analytics: true,
            early_access: true,
            priority_support: true,
            monthly_bliss: 500,
        }
    }
}

/// Free tier benefits for comparison
pub struct FreeTierBenefits {
    pub revenue_share: f64,
    pub storage_limit_bytes: u64,
}

impl Default for FreeTierBenefits {
    fn default() -> Self {
        Self {
            revenue_share: 0.25,  // 25% to creator
            storage_limit_bytes: 10 * 1024 * 1024 * 1024, // 10 GB
        }
    }
}

/// Calculate creator revenue for a sale
pub fn calculate_creator_revenue(
    sale_bliss: u64,
    is_pro: bool,
) -> CreatorRevenue {
    let share = if is_pro { 0.40 } else { 0.25 };
    let creator_amount = (sale_bliss as f64 * share).floor() as u64;
    let platform_amount = sale_bliss - creator_amount;
    
    CreatorRevenue {
        sale_amount: sale_bliss,
        creator_share: share,
        creator_amount,
        platform_amount,
    }
}

#[derive(Debug, Clone)]
pub struct CreatorRevenue {
    pub sale_amount: u64,
    pub creator_share: f64,
    pub creator_amount: u64,
    pub platform_amount: u64,
}
```

---

## Bundle

### Benefits

Combines **Player Plus** + **Creator Pro** at a discount.

| Feature | Bundle |
|---------|--------|
| Monthly Bliss | 1,000 (combined) |
| Cloud Storage | 10 GB (player) |
| Asset Storage | Unlimited (creator) |
| Revenue Share | 40% |
| All Player Plus perks | ✅ |
| All Creator Pro perks | ✅ |

### Pricing Value

| Separate | Bundle | Savings |
|----------|--------|---------|
| $14.98/mo | $12.99/mo | $1.99/mo (13%) |
| $149.98/yr | $129.99/yr | $19.99/yr (13%) |

---

## Steam Integration

### Subscription Item Definitions

```json
// steamworks/subscription_definitions.json
{
  "appid": 123456,
  "subscriptions": [
    {
      "itemdefid": 2001,
      "type": "subscription",
      "name": "Player Plus - Monthly",
      "description": "500 Bliss/month, priority queue, exclusive cosmetics",
      "price": "499;USD",
      "billing_period": "monthly"
    },
    {
      "itemdefid": 2002,
      "type": "subscription",
      "name": "Player Plus - Annual",
      "description": "500 Bliss/month, priority queue, exclusive cosmetics (17% savings)",
      "price": "4999;USD",
      "billing_period": "yearly"
    },
    {
      "itemdefid": 2003,
      "type": "subscription",
      "name": "Creator Pro - Monthly",
      "description": "40% revenue share, unlimited storage, priority publishing",
      "price": "999;USD",
      "billing_period": "monthly"
    },
    {
      "itemdefid": 2004,
      "type": "subscription",
      "name": "Creator Pro - Annual",
      "description": "40% revenue share, unlimited storage, priority publishing (17% savings)",
      "price": "9999;USD",
      "billing_period": "yearly"
    },
    {
      "itemdefid": 2005,
      "type": "subscription",
      "name": "Bundle - Monthly",
      "description": "Player Plus + Creator Pro combined",
      "price": "1299;USD",
      "billing_period": "monthly"
    },
    {
      "itemdefid": 2006,
      "type": "subscription",
      "name": "Bundle - Annual",
      "description": "Player Plus + Creator Pro combined (17% savings)",
      "price": "12999;USD",
      "billing_period": "yearly"
    }
  ]
}
```

### Subscription Service

```rust
// crates/services/src/subscriptions/service.rs

pub struct SubscriptionService {
    db: sqlx::PgPool,
    steam: SteamClient,
    bliss_store: BlissDataStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionPlan {
    PlayerPlus,
    CreatorPro,
    Bundle,
}

#[derive(Debug, Clone)]
pub struct Subscription {
    pub user_id: String,
    pub plan: SubscriptionPlan,
    pub billing_period: BillingPeriod,
    pub status: SubscriptionStatus,
    pub started_at: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub steam_subscription_id: String,
}

#[derive(Debug, Clone, Copy)]
pub enum BillingPeriod {
    Monthly,
    Annual,
}

#[derive(Debug, Clone, Copy)]
pub enum SubscriptionStatus {
    Active,
    PastDue,
    Canceled,
    Expired,
}

impl SubscriptionService {
    /// Check if user has specific subscription benefit
    pub async fn has_benefit(
        &self,
        user_id: &str,
        benefit: SubscriptionBenefit,
    ) -> Result<bool, SubscriptionError> {
        let sub = self.get_active_subscription(user_id).await?;
        
        match sub {
            None => Ok(false),
            Some(s) => Ok(benefit.included_in(s.plan)),
        }
    }
    
    /// Get user's current subscription
    pub async fn get_active_subscription(
        &self,
        user_id: &str,
    ) -> Result<Option<Subscription>, SubscriptionError> {
        sqlx::query_as!(
            Subscription,
            r#"
            SELECT user_id, plan as "plan: _", billing_period as "billing_period: _",
                   status as "status: _", started_at, current_period_end, steam_subscription_id
            FROM subscriptions
            WHERE user_id = $1 AND status = 'active'
            "#,
            user_id
        )
        .fetch_optional(&self.db)
        .await
        .map_err(|e| SubscriptionError::Database(e.to_string()))
    }
    
    /// Get revenue share for user
    pub async fn get_revenue_share(&self, user_id: &str) -> Result<f64, SubscriptionError> {
        let has_pro = self.has_benefit(user_id, SubscriptionBenefit::EnhancedRevenueShare).await?;
        Ok(if has_pro { 0.40 } else { 0.25 })
    }
    
    /// Get storage limit for user (bytes)
    pub async fn get_storage_limit(&self, user_id: &str) -> Result<u64, SubscriptionError> {
        let has_pro = self.has_benefit(user_id, SubscriptionBenefit::UnlimitedStorage).await?;
        Ok(if has_pro {
            1024 * 1024 * 1024 * 1024  // 1 TB
        } else {
            10 * 1024 * 1024 * 1024    // 10 GB
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SubscriptionBenefit {
    MonthlyBliss,
    PriorityQueue,
    SubscriberCosmetics,
    SubscriberEvents,
    PremiumProfile,
    EnhancedRevenueShare,
    UnlimitedStorage,
    PriorityPublishing,
    AdvancedAnalytics,
    EarlyAccess,
    PrioritySupport,
}

impl SubscriptionBenefit {
    pub fn included_in(&self, plan: SubscriptionPlan) -> bool {
        match self {
            // Player Plus benefits
            SubscriptionBenefit::MonthlyBliss => true, // All plans
            SubscriptionBenefit::PriorityQueue => matches!(plan, SubscriptionPlan::PlayerPlus | SubscriptionPlan::Bundle),
            SubscriptionBenefit::SubscriberCosmetics => matches!(plan, SubscriptionPlan::PlayerPlus | SubscriptionPlan::Bundle),
            SubscriptionBenefit::SubscriberEvents => matches!(plan, SubscriptionPlan::PlayerPlus | SubscriptionPlan::Bundle),
            SubscriptionBenefit::PremiumProfile => matches!(plan, SubscriptionPlan::PlayerPlus | SubscriptionPlan::Bundle),
            
            // Creator Pro benefits
            SubscriptionBenefit::EnhancedRevenueShare => matches!(plan, SubscriptionPlan::CreatorPro | SubscriptionPlan::Bundle),
            SubscriptionBenefit::UnlimitedStorage => matches!(plan, SubscriptionPlan::CreatorPro | SubscriptionPlan::Bundle),
            SubscriptionBenefit::PriorityPublishing => matches!(plan, SubscriptionPlan::CreatorPro | SubscriptionPlan::Bundle),
            SubscriptionBenefit::AdvancedAnalytics => matches!(plan, SubscriptionPlan::CreatorPro | SubscriptionPlan::Bundle),
            SubscriptionBenefit::EarlyAccess => matches!(plan, SubscriptionPlan::CreatorPro | SubscriptionPlan::Bundle),
            SubscriptionBenefit::PrioritySupport => matches!(plan, SubscriptionPlan::CreatorPro | SubscriptionPlan::Bundle),
        }
    }
}
```

---

## Cost Analysis

### Infrastructure Costs

#### Cloud Storage (S3-Compatible)

Based on `eustress/crates/common/src/assets/s3.rs`, we support multiple providers:

| Provider | Storage | Egress | Best For |
|----------|---------|--------|----------|
| **MinIO (Fly.io)** | $5/mo flat (3GB vol) | Free* | Dev/Small scale |
| **MinIO (Hetzner VPS)** | $4/mo (40GB SSD) | 20TB free | Self-hosted |
| **Cloudflare R2** | $0.015/GB/mo | Free | Production (recommended) |
| **AWS S3** | $0.023/GB/mo | $0.09/GB | Enterprise |
| **DO Spaces** | $5/mo (250GB) | $0.01/GB | Mid-tier |

*Fly.io egress: 100GB/mo free, then $0.02/GB

**Recommended Strategy:**
- **0-1,000 users:** MinIO on Fly.io ($5/mo flat)
- **1,000-10,000 users:** Cloudflare R2 (scales with usage)
- **10,000+ users:** R2 + CDN caching

#### Storage Cost by Provider & Scale

| Users | Avg Storage | MinIO (Fly) | R2 | AWS S3 |
|-------|-------------|-------------|-----|--------|
| 100 | 500 GB | $5/mo* | $7.50 | $11.50 |
| 1,000 | 5 TB | $15/mo* | $75 | $115 |
| 10,000 | 50 TB | N/A | $750 | $1,150 |
| 100,000 | 500 TB | N/A | $7,500 | $11,500 |

*MinIO requires volume upgrades at scale ($0.15/GB on Fly.io)

#### Per-User Storage Estimates

| User Type | Avg Storage | R2 Cost/User/Mo |
|-----------|-------------|-----------------|
| Free Player | 100 MB | $0.0015 |
| Active Player | 500 MB | $0.0075 |
| Free Creator | 2 GB | $0.03 |
| Creator Pro | 50 GB avg | $0.75 |
| Heavy Creator | 200 GB | $3.00 |

**Key Insight:** Storage is cheap. At R2 rates:
- 10 GB free tier = $0.15/user/month
- 1 TB Creator Pro cap = $15/user/month (but avg usage ~50GB = $0.75)

#### Dedicated Servers (Game Hosting)

| Provider | Cost | Specs |
|----------|------|-------|
| Hetzner | $0.007/hr | 4 vCPU, 8GB RAM |
| Vultr | $0.012/hr | 4 vCPU, 8GB RAM |
| AWS EC2 | $0.034/hr | t3.xlarge |

**Assumed:** Hetzner @ $0.007/hr = $5.04/month per server

| Players/Server | Cost/Player/Month |
|----------------|-------------------|
| 50 concurrent | $0.10 |
| 100 concurrent | $0.05 |
| 200 concurrent | $0.025 |

#### Database & Cache

| Service | Cost/Month | Capacity |
|---------|------------|----------|
| PostgreSQL (managed) | $15 | 10GB, shared |
| PostgreSQL (dedicated) | $50 | 100GB, dedicated |
| Redis (managed) | $10 | 1GB cache |

#### CDN & Bandwidth

| Provider | Cost | Notes |
|----------|------|-------|
| Cloudflare | Free-$20 | Most traffic free |
| AWS CloudFront | $0.085/GB | First 10TB |

**Assumed:** Cloudflare Pro @ $20/month (covers most needs)

### Cost Per User Estimates

#### Free User (Player)

| Cost Category | Monthly | Notes |
|---------------|---------|-------|
| Storage (100 MB avg) | $0.002 | Cloud saves only |
| Server share | $0.03 | Shared game servers |
| Database share | $0.005 | User record |
| CDN share | $0.01 | Asset delivery |
| **Total** | **$0.05** | Very cheap |

#### Free User (Creator)

| Cost Category | Monthly | Notes |
|---------------|---------|-------|
| Storage (2 GB avg) | $0.03 | Assets + projects |
| Server share | $0.03 | Shared |
| Database share | $0.01 | More records |
| CDN share | $0.02 | Asset delivery |
| **Total** | **$0.09** | Still cheap |

#### Player Plus Subscriber

| Cost Category | Monthly | Notes |
|---------------|---------|-------|
| Storage (500 MB avg) | $0.008 | More cloud saves |
| Server share (priority) | $0.05 | Better allocation |
| Database share | $0.01 | |
| CDN share | $0.02 | |
| Bliss grant (500) | $0.35* | 70% margin on Bliss |
| **Total** | **$0.44** | |

*500 Bliss costs us ~$0.35 (users don't spend 100%)

#### Creator Pro Subscriber

| Cost Category | Monthly | Notes |
|---------------|---------|-------|
| Storage (50 GB avg) | $0.75 | R2 @ $0.015/GB |
| Server share | $0.03 | |
| Database share | $0.02 | More analytics data |
| CDN share | $0.10 | Asset delivery |
| Analytics compute | $0.15 | Dashboard queries |
| Bliss grant (500) | $0.35 | |
| **Total** | **$1.40** | |

#### Bundle Subscriber

| Cost Category | Monthly | Notes |
|---------------|---------|-------|
| Storage (50 GB avg) | $0.75 | Creator storage |
| Server share (priority) | $0.05 | |
| Database share | $0.02 | |
| CDN share | $0.10 | |
| Analytics compute | $0.15 | |
| Bliss grant (1,000) | $0.70 | Double allowance |
| **Total** | **$1.77** | |

---

## Profitability Model

### User Acquisition Cost (UAC)

| Channel | Cost/Install | Conversion | Cost/Paying User |
|---------|--------------|------------|------------------|
| Steam organic | $0 | 2% | $0 |
| Steam featuring | $0 | 5% | $0 |
| YouTube ads | $2.00 | 1% | $200 |
| Twitch sponsorship | $5.00 | 3% | $167 |
| Reddit/Discord | $0.50 | 2% | $25 |
| Word of mouth | $0 | 3% | $0 |

**Blended UAC estimate:** $15-30 per paying user (heavy organic focus)

### Revenue Per User Per Month (RPUPM)

| User Type | % of Users | RPUPM | Cost | Profit | Weighted Rev |
|-----------|------------|-------|------|--------|--------------|
| Free Player | 60% | $0 | $0.05 | -$0.05 | $0 |
| Free Creator | 10% | $0 | $0.09 | -$0.09 | $0 |
| Free (Bliss buyer) | 15% | $2.00 | $0.07 | $1.93 | $0.30 |
| Player Plus | 8% | $3.49 | $0.44 | $3.05 | $0.28 |
| Creator Pro | 5% | $6.99 | $1.40 | $5.59 | $0.35 |
| Bundle | 2% | $9.09 | $1.77 | $7.32 | $0.18 |
| **Average** | 100% | — | $0.15 | — | **$1.11** |

### Profitability by Scale

#### 0 Users (Pre-Launch)

| Category | Monthly |
|----------|---------|
| Revenue | $0 |
| MinIO (Fly.io) | -$5 |
| Database (Supabase free) | $0 |
| Domain/SSL | -$2 |
| **Net** | **-$7** |

#### 100 Users

| Category | Monthly |
|----------|---------|
| Revenue | $111 |
| User costs (100 × $0.15 avg) | -$15 |
| MinIO (Fly.io) | -$5 |
| Database | -$10 |
| **Net** | **$81** |
| **Margin** | **73%** |

#### 1,000 Users

| Category | Monthly |
|----------|---------|
| Revenue | $1,110 |
| User costs (1,000 × $0.15) | -$150 |
| Storage (R2, 5TB) | -$75 |
| Database (managed) | -$25 |
| Servers (2× Hetzner) | -$10 |
| **Net** | **$850** |
| **Margin** | **77%** |

#### 10,000 Users

| Category | Monthly |
|----------|---------|
| Revenue | $11,100 |
| User costs (10,000 × $0.15) | -$1,500 |
| Storage (R2, 50TB) | -$750 |
| Database (dedicated) | -$100 |
| K8s Cluster (MoE, 8 nodes) | -$74 |
| Support (1 PT) | -$2,000 |
| **Net** | **$6,676** |
| **Margin** | **60%** |

#### 100,000 Users

| Category | Monthly |
|----------|---------|
| Revenue | $111,000 |
| User costs (100K × $0.12*) | -$12,000 |
| Storage (R2, 500TB) | -$7,500 |
| Database cluster | -$500 |
| K8s Cluster (MoE, 24 nodes) | -$284 |
| Support (3 FT) | -$15,000 |
| Engineering (2 FT) | -$25,000 |
| **Net** | **$50,716** |
| **Margin** | **46%** |

*Economies of scale reduce per-user cost

#### 1,000,000 Users

| Category | Monthly |
|----------|---------|
| Revenue | $1,110,000 |
| User costs (1M × $0.10*) | -$100,000 |
| Storage (R2, 5PB) | -$75,000 |
| Database cluster | -$2,000 |
| K8s Cluster (MoE, 160 nodes) | -$1,600 |
| Support (15 FT) | -$75,000 |
| Engineering (10 FT) | -$150,000 |
| Operations (5 FT) | -$50,000 |
| Marketing | -$100,000 |
| Legal/Compliance | -$20,000 |
| **Net** | **$536,400** |
| **Margin** | **48%** |

*Heavy economies of scale + MoE + CDN caching reduces costs

### Break-Even Analysis

| Metric | Value |
|--------|-------|
| Fixed costs (early) | ~$7-50/month |
| Variable cost/user | ~$0.10-0.15 |
| Revenue/user | ~$1.11 |
| Contribution margin | ~$0.96/user |
| **Break-even users** | **~10-50** |

### Storage Provider Transition Plan

| Users | Provider | Monthly Cost | Notes |
|-------|----------|--------------|-------|
| 0-500 | MinIO (Fly.io) | $5 flat | 3GB volume included |
| 500-2,000 | MinIO (Fly.io + vol) | $5 + $15 | 100GB volume |
| 2,000-10,000 | Cloudflare R2 | $75-750 | Migrate to R2 |
| 10,000+ | R2 + CDN | $750+ | Add caching layer |

---

## Server Infrastructure

For detailed information on our Mixture of Experts (MoE) architecture, Kubernetes integration, and Roblox comparison, see:

📄 **[Infrastructure Architecture](../architecture/INFRASTRUCTURE.md)**

### Summary

- **Architecture:** MoE on Kubernetes (k3s) hosted on Hetzner Cloud
- **Experts:** GameLogic, Physics (Avian), AINPC, AssetServing, VoiceChat, Matchmaking, Analytics, Moderation
- **Cost Savings:** 60-77% vs homogeneous servers
- **Utilization:** 80%+ (vs ~50% monolithic)

### Server Cost Summary

| Scale | MoE + K8s Cost | Notes |
|-------|----------------|-------|
| 1,000 users | $19.50/mo | 2× CPX31 + 1× CPX21 |
| 10,000 users | $74/mo | 4× CPX31 + 2× CPX21 + 1× CCX33 |
| 100,000 users | $284/mo | 16× CPX31 + 4× CPX21 + 4× CCX33 |
| 1,000,000 users | $1,136/mo | 64× CPX31 + 16× CPX21 + 16× CCX33 |

### Key Metrics to Track

```rust
/// Business metrics for subscription health
#[derive(Debug, Clone)]
pub struct SubscriptionMetrics {
    /// Monthly Recurring Revenue
    pub mrr: f64,
    
    /// Annual Recurring Revenue
    pub arr: f64,
    
    /// Average Revenue Per User
    pub arpu: f64,
    
    /// Customer Acquisition Cost
    pub cac: f64,
    
    /// Lifetime Value
    pub ltv: f64,
    
    /// LTV:CAC ratio (target: >3:1)
    pub ltv_cac_ratio: f64,
    
    /// Monthly churn rate
    pub churn_rate: f64,
    
    /// Net Revenue Retention
    pub nrr: f64,
}

impl SubscriptionMetrics {
    pub fn calculate(db: &PgPool) -> Self {
        // Implementation...
        todo!()
    }
    
    pub fn is_healthy(&self) -> bool {
        self.ltv_cac_ratio > 3.0 && self.churn_rate < 0.05
    }
}
```

---

## Summary

### Per-Subscription Profitability

| Plan | Price | Net (after Steam) | Cost | Profit | Margin |
|------|-------|-------------------|------|--------|--------|
| Player Plus | $4.99 | $3.49 | $0.44 | $3.05 | 87% |
| Creator Pro | $9.99 | $6.99 | $1.40 | $5.59 | 80% |
| Bundle | $12.99 | $9.09 | $1.77 | $7.32 | 81% |

### Platform Profitability by Scale

| Users | Revenue | Costs | Net Profit | Margin |
|-------|---------|-------|------------|--------|
| 100 | $111 | $30 | $81 | 73% |
| 1,000 | $1,110 | $260 | $850 | 77% |
| 10,000 | $11,100 | $4,400 | $6,700 | 60% |
| 100,000 | $111,000 | $60,250 | $50,750 | 46% |
| 1,000,000 | $1,110,000 | $573,000 | $537,000 | 48% |

### Key Targets

| Metric | Target |
|--------|--------|
| Subscription conversion | 15% |
| LTV:CAC ratio | >3:1 |
| Monthly churn | <5% |
| Break-even | ~10-50 users |

### Infrastructure Strategy

| Scale | Storage | Servers | Database |
|-------|---------|---------|----------|
| 0-500 | MinIO (Fly.io) | Fly.io | Supabase free |
| 500-10K | Cloudflare R2 | Hetzner | Supabase Pro |
| 10K+ | R2 + CDN | Hetzner cluster | Dedicated PG |

---

## Contact

**Subscription Support:** 
**Creator Program:** 
**Business Inquiries:** 

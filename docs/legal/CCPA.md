# CCPA Compliance Documentation

**California Consumer Privacy Act (CCPA) / CPRA Implementation for Eustress Engine**

> *Best Match Dynamic: Consent → Unified verifiable flows with granular opt-ins*

**Last Updated:** December 03, 2025  
**Status:** Pre-Release Compliance Framework  
**Applies To:** California residents using Eustress Engine or Simbuilder products

---

## Table of Contents

1. [Overview](#overview)
2. [Consumer Rights Implementation](#consumer-rights-implementation)
3. [Rust Implementation](#rust-implementation)
4. [API Endpoints](#api-endpoints)
5. [Data Categories](#data-categories)
6. [Opt-Out Mechanisms](#opt-out-mechanisms)
7. [Monorepo Integration](#monorepo-integration)
8. [Testing & Verification](#testing--verification)

---

## Overview

### Regulatory Context

The California Consumer Privacy Act (CCPA), as amended by the California Privacy Rights Act (CPRA), grants California residents:

- **Right to Know**: What personal information is collected
- **Right to Delete**: Request erasure of personal data
- **Right to Opt-Out**: Refuse sale/sharing of personal information
- **Right to Correct**: Amend inaccurate personal data
- **Right to Limit**: Restrict use of sensitive personal information

### Eustress Engine Compliance Strategy

```
Dynamic: Rust + Moderation → Filter
Implication: Tokio async for real-time scans, no-downtime updates
Savings: Avoid $2,500/intentional + $7,500/unintentional violation fines
```

**Core Principle:** Privacy-by-design via Rust's memory safety guarantees and `data_privacy` crate annotations.

---

## Consumer Rights Implementation

### Right to Know (§1798.100)

| Data Category | Collection Point | Retention | Access Method |
|---------------|------------------|-----------|---------------|
| Identifiers | Account creation | 3 years post-deletion | `/api/v1/ccpa/access` |
| Device Info | Engine telemetry | Session only (ephemeral) | Dashboard export |
| Usage Data | Gameplay analytics | 30 days rolling | Self-service portal |
| Geolocation | IP-derived (coarse) | Not stored | N/A |

### Right to Delete (§1798.105)

```rust
// crates/api/src/ccpa/delete.rs
use sqlx::PgPool;
use data_privacy::Redactor;

pub async fn process_deletion_request(
    pool: &PgPool,
    user_id: &str,
    redactor: &Redactor,
) -> Result<DeletionReceipt, CcpaError> {
    let hashed_id = redactor.hash(user_id);
    
    // Cascade delete across all tables
    sqlx::query!("CALL ccpa_cascade_delete($1)", hashed_id)
        .execute(pool)
        .await?;
    
    // Log for audit (anonymized)
    audit_log::record(AuditEvent::CcpaDeletion {
        hash: hashed_id,
        timestamp: chrono::Utc::now(),
        tables_affected: vec!["users", "sessions", "preferences"],
    });
    
    Ok(DeletionReceipt {
        confirmation_id: uuid::Uuid::new_v4(),
        completed_at: chrono::Utc::now(),
        retention_exemptions: vec![], // List any legal holds
    })
}
```

**SLA:** Deletion completed within 45 days (15-day extension if notified).

### Right to Opt-Out (§1798.120)

```rust
// crates/shared/src/consent.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CcpaConsent {
    pub do_not_sell: bool,           // "Do Not Sell My Personal Information"
    pub do_not_share: bool,          // CPRA addition
    pub limit_sensitive_use: bool,   // Sensitive PI restrictions
    pub opt_out_profiling: bool,     // Automated decision-making
}

impl Default for CcpaConsent {
    fn default() -> Self {
        Self {
            do_not_sell: true,       // Default: privacy-protective
            do_not_share: true,
            limit_sensitive_use: true,
            opt_out_profiling: true,
        }
    }
}
```

---

## Rust Implementation

### Crate Dependencies

```toml
# crates/api/Cargo.toml
[dependencies]
data_privacy = "0.1"      # PII annotation/redaction
secrecy = "0.8"           # Secret handling (no Debug leaks)
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio"] }
axum = "0.7"
tower = "0.4"             # Rate limiting middleware
```

### Data Annotation Pattern

```rust
// crates/shared/src/pii.rs
use data_privacy::{Pii, Sensitive};
use secrecy::SecretString;

/// User data with CCPA-compliant annotations
#[derive(Debug, Pii)]
pub struct UserProfile {
    #[pii(category = "identifier")]
    pub email: SecretString,
    
    #[pii(category = "identifier", retention = "3y")]
    pub username: String,
    
    #[pii(category = "sensitive", purpose = "age_verification")]
    pub birth_year: Option<u16>,
    
    #[pii(redact)]  // Automatic redaction in logs
    pub ip_address: Option<std::net::IpAddr>,
}
```

### Minimization Enforcement

```rust
// crates/shared/src/config.rs
pub struct EustressConfig {
    /// CCPA: Collect only necessary data
    pub ccpa_minimization: bool,
    
    /// Ephemeral session data (no persistence)
    pub ephemeral_telemetry: bool,
    
    /// Redis TTL for session data
    pub session_ttl_seconds: u64,
}

impl Default for EustressConfig {
    fn default() -> Self {
        Self {
            ccpa_minimization: true,      // Always on
            ephemeral_telemetry: true,    // Default: no persistence
            session_ttl_seconds: 3600,    // 1 hour
        }
    }
}
```

---

## API Endpoints

### CCPA Consumer Rights API

| Endpoint | Method | Purpose | Auth Required |
|----------|--------|---------|---------------|
| `/api/v1/ccpa/access` | GET | Download personal data | JWT |
| `/api/v1/ccpa/delete` | POST | Request deletion | JWT + Verification |
| `/api/v1/ccpa/optout` | POST | Opt-out of sale/sharing | Session |
| `/api/v1/ccpa/correct` | PATCH | Correct inaccurate data | JWT |
| `/api/v1/ccpa/categories` | GET | List data categories | None |

### Implementation

```rust
// crates/api/src/routes/ccpa.rs
use axum::{
    routing::{get, post, patch},
    Router, Json, extract::State,
};

pub fn ccpa_router() -> Router<AppState> {
    Router::new()
        .route("/access", get(handle_access_request))
        .route("/delete", post(handle_deletion_request))
        .route("/optout", post(handle_optout))
        .route("/correct", patch(handle_correction))
        .route("/categories", get(list_categories))
        .layer(tower::limit::RateLimitLayer::new(10, std::time::Duration::from_secs(60)))
}

async fn handle_optout(
    State(state): State<AppState>,
    Json(payload): Json<OptOutRequest>,
) -> Result<Json<OptOutResponse>, CcpaError> {
    // Set GPC (Global Privacy Control) respecting flags
    if payload.gpc_signal {
        state.consent_store.set_do_not_sell(&payload.user_id, true).await?;
    }
    
    // Propagate to third parties within 15 days
    state.third_party_sync.queue_optout(&payload.user_id).await?;
    
    Ok(Json(OptOutResponse {
        status: "opted_out",
        effective_date: chrono::Utc::now(),
    }))
}
```

---

## Data Categories

### Categories Collected (§1798.110)

```rust
/// CCPA-defined data categories
pub enum CcpaDataCategory {
    /// Real name, alias, postal address, email, account name
    Identifiers,
    
    /// Age, gender (NOT collected for minors)
    Demographics,
    
    /// Purchasing history, game purchases
    CommercialInfo,
    
    /// Gameplay patterns, session duration
    ActivityInfo,
    
    /// Coarse geolocation (country/region only)
    Geolocation,
    
    /// Device type, OS version (no fingerprinting)
    DeviceInfo,
    
    /// Preferences, settings, accessibility options
    Inferences,
}

impl CcpaDataCategory {
    /// Business purpose for collection
    pub fn purpose(&self) -> &'static str {
        match self {
            Self::Identifiers => "Account management and authentication",
            Self::Demographics => "Age-appropriate content filtering",
            Self::CommercialInfo => "Transaction processing and refunds",
            Self::ActivityInfo => "Service improvement and bug fixing",
            Self::Geolocation => "Regulatory compliance (GDPR/CCPA regions)",
            Self::DeviceInfo => "Compatibility and performance optimization",
            Self::Inferences => "Personalized experience (with consent)",
        }
    }
}
```

### Sensitive Personal Information

**NOT Collected:**
- Social Security numbers
- Financial account credentials
- Precise geolocation
- Racial/ethnic origin
- Religious beliefs
- Biometric data
- Health information
- Sexual orientation

---

## Opt-Out Mechanisms

### 1. Global Privacy Control (GPC)

```rust
// crates/api/src/middleware/gpc.rs
use axum::http::HeaderMap;

pub fn detect_gpc_signal(headers: &HeaderMap) -> bool {
    headers
        .get("Sec-GPC")
        .map(|v| v == "1")
        .unwrap_or(false)
}

pub async fn gpc_middleware<B>(
    headers: HeaderMap,
    request: axum::http::Request<B>,
    next: axum::middleware::Next<B>,
) -> axum::response::Response {
    if detect_gpc_signal(&headers) {
        // Automatically honor GPC as opt-out
        // Set context flag for downstream handlers
        request.extensions_mut().insert(GpcOptOut(true));
    }
    next.run(request).await
}
```

### 2. "Do Not Sell" Link

Required on homepage footer. Links to `/privacy/do-not-sell`:

```rust
// In Eustress Engine UI (Bevy/egui)
fn render_privacy_footer(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.link("Do Not Sell or Share My Personal Information").clicked() {
            // Open opt-out flow
            open_ccpa_optout_dialog();
        }
        ui.separator();
        if ui.link("Privacy Policy").clicked() {
            open_url("https://simbuilder.com/privacy");
        }
    });
}
```

### 3. Authorized Agent Support

```rust
#[derive(Deserialize)]
pub struct AuthorizedAgentRequest {
    pub consumer_id: String,
    pub agent_authorization: AgentProof,  // Power of attorney or signed permission
    pub request_type: CcpaRequestType,
}

pub enum AgentProof {
    PowerOfAttorney { document_hash: String },
    SignedPermission { consumer_signature: String, date: chrono::NaiveDate },
}
```

---

## Monorepo Integration

### Directory Structure

```
crates/
├── api/
│   └── src/
│       ├── routes/
│       │   └── ccpa.rs      # CCPA endpoints
│       └── middleware/
│           └── gpc.rs       # GPC detection
├── shared/
│   └── src/
│       ├── consent.rs       # CcpaConsent struct
│       ├── pii.rs           # PII annotations
│       └── config.rs        # Minimization config
└── ml-core/
    └── src/
        └── anonymize.rs     # Training data anonymization
```

### CI/CD Compliance Checks

```yaml
# .github/workflows/ccpa-audit.yml
name: CCPA Compliance Audit
on: [push, pull_request]

jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Check PII Annotations
        run: cargo run --bin pii-linter -- --strict
        
      - name: Verify Deletion Cascade
        run: cargo test --package api -- ccpa::deletion
        
      - name: Audit Dependencies
        run: cargo audit --deny warnings
        
      - name: Check Retention Policies
        run: cargo run --bin retention-checker
```

---

## Testing & Verification

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_deletion_cascades_all_tables() {
        let pool = test_db_pool().await;
        let user_id = create_test_user(&pool).await;
        
        // Create data across tables
        create_user_sessions(&pool, &user_id).await;
        create_user_preferences(&pool, &user_id).await;
        
        // Process deletion
        let receipt = process_deletion_request(&pool, &user_id, &Redactor::new())
            .await
            .unwrap();
        
        // Verify complete erasure
        assert!(find_user(&pool, &user_id).await.is_none());
        assert!(find_sessions(&pool, &user_id).await.is_empty());
        assert!(find_preferences(&pool, &user_id).await.is_none());
    }
    
    #[tokio::test]
    async fn test_optout_propagates_to_third_parties() {
        // Verify opt-out reaches all data processors within 15 days
    }
    
    #[test]
    fn test_gpc_signal_detection() {
        let mut headers = HeaderMap::new();
        headers.insert("Sec-GPC", "1".parse().unwrap());
        assert!(detect_gpc_signal(&headers));
    }
}
```

### Compliance Checklist

- [ ] "Do Not Sell" link in footer (12-month cookie)
- [ ] GPC signal honored automatically
- [ ] Deletion within 45 days (15-day extension documented)
- [ ] Access request delivers machine-readable format
- [ ] Third-party processor agreements updated
- [ ] Privacy policy lists all categories
- [ ] Minors (<16) require opt-in (see [COPPA.md](./COPPA.md))
- [ ] Financial incentive disclosures (if applicable)
- [ ] Audit logs retained for 24 months

---

## Related Documentation

- [COPPA.md](./COPPA.md) - Children's Privacy (under-16 CCPA provisions)
- [GDPR.md](./GDPR.md) - EU Data Protection (overlapping requirements)
- [MODERATION_API.md](../moderation/MODERATION_API.md) - API implementation details

---

**Compliance Contact:** legal@simbuilder.com  
**Data Protection:** privacy@simbuilder.com

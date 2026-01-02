# Moderation API Documentation

**REST/gRPC API for Eustress Engine Content Moderation System**

> *Best Match Dynamic: Classifier → Axum endpoints with Candle inference, confidence thresholds (>0.9 safe)*

**Last Updated:** December 03, 2025  
**Status:** Pre-Release API Specification  
**Base URL:** `https://api.eustress.io/v1/moderation`

---

## Table of Contents

1. [Overview](#overview)
2. [Authentication](#authentication)
3. [Endpoints](#endpoints)
4. [Classification API](#classification-api)
5. [Action API](#action-api)
6. [Reporting API](#reporting-api)
7. [Appeal API](#appeal-api)
8. [Webhook Integration](#webhook-integration)
9. [Rate Limiting](#rate-limiting)
10. [Rust Implementation](#rust-implementation)

---

## Overview

### API Design Principles

```
Dynamic: API + Supervised ML → Classifier
Implication: Axum endpoints with Candle inference, >0.9 confidence = safe
Benefit: 48hr Take It Down SLAs, minimizes false positives
```

**Architecture:**

```
┌──────────────────────────────────────────────────────────────────────┐
│                         API GATEWAY (Axum)                           │
│                    Rate Limiting + Auth + Routing                    │
└──────────────────────────────────────────────────────────────────────┘
                                    │
         ┌──────────────────────────┼──────────────────────────┐
         │                          │                          │
         ▼                          ▼                          ▼
┌─────────────────┐      ┌─────────────────┐      ┌─────────────────┐
│  /classify      │      │  /action        │      │  /report        │
│  ML Inference   │      │  Apply Decision │      │  User Reports   │
└─────────────────┘      └─────────────────┘      └─────────────────┘
         │                          │                          │
         └──────────────────────────┼──────────────────────────┘
                                    │
                                    ▼
                        ┌─────────────────────┐
                        │  ML-Core (Candle)   │
                        │  + Agent Orchestrator│
                        └─────────────────────┘
```

### Response Format

All responses follow a consistent structure:

```json
{
  "success": true,
  "data": { ... },
  "meta": {
    "request_id": "uuid",
    "timestamp": "ISO8601",
    "processing_time_ms": 42
  },
  "errors": []
}
```

---

## Authentication

### API Key Authentication (External Clients)

```http
Authorization: Bearer <API_KEY>
X-Client-ID: <CLIENT_ID>
```

### Steam OpenID Authentication (Players)

Eustress Engine uses **Steam OpenID 2.0** for player authentication. JWT tokens are issued by our auth service after Steam verification, NOT sourced directly from Steam.

```rust
// crates/api/src/auth/steam.rs
use reqwest::Client;

/// Steam OpenID verification
pub struct SteamAuth {
    client: Client,
    api_key: String,  // Steam Web API key
}

impl SteamAuth {
    /// Verify Steam OpenID response and issue our JWT
    pub async fn verify_and_issue_token(
        &self,
        openid_response: &SteamOpenIdResponse,
    ) -> Result<AuthToken, AuthError> {
        // Verify OpenID signature with Steam
        let steam_id = self.verify_openid(openid_response).await?;
        
        // Get player info from Steam Web API
        let player_info = self.get_player_info(&steam_id).await?;
        
        // Load or create user record
        let user = self.get_or_create_user(&steam_id, &player_info).await?;
        
        // Issue OUR JWT (not Steam's)
        let token = self.issue_jwt(JwtClaims {
            sub: user.id.clone(),
            steam_id: steam_id.clone(),
            display_name: player_info.persona_name,
            reputation: user.reputation,  // Our reputation system
            exp: chrono::Utc::now().timestamp() as usize + 86400,
        })?;
        
        Ok(AuthToken {
            access_token: token,
            token_type: "Bearer".into(),
            expires_in: 86400,
            steam_id,
            user_id: user.id,
        })
    }
    
    async fn verify_openid(&self, response: &SteamOpenIdResponse) -> Result<String, AuthError> {
        // Steam OpenID verification endpoint
        let verify_url = "https://steamcommunity.com/openid/login";
        
        let mut params = response.to_params();
        params.insert("openid.mode", "check_authentication");
        
        let result = self.client
            .post(verify_url)
            .form(&params)
            .send()
            .await?
            .text()
            .await?;
        
        if result.contains("is_valid:true") {
            // Extract Steam ID from claimed_id
            let steam_id = response.claimed_id
                .split('/')
                .last()
                .ok_or(AuthError::InvalidSteamId)?;
            Ok(steam_id.to_string())
        } else {
            Err(AuthError::SteamVerificationFailed)
        }
    }
}

/// Steam Web API player info
#[derive(Deserialize)]
pub struct SteamPlayerInfo {
    pub steamid: String,
    pub persona_name: String,
    pub profile_url: String,
    pub avatar: String,
    pub avatar_medium: String,
    pub avatar_full: String,
    pub community_visibility_state: u8,
    pub profile_state: Option<u8>,
}
```

### JWT for Internal Services (Service-to-Service)

```rust
// crates/api/src/auth/jwt.rs
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,           // Service ID
    pub aud: String,           // Audience (moderation-api)
    pub exp: usize,            // Expiration
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Permission {
    Classify,
    ActionApply,
    ActionOverride,
    ReportRead,
    ReportWrite,
    AppealReview,
    Admin,
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
    next: Next<Body>,
) -> Response {
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(AuthError::MissingToken)?;
    
    let claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_ref()),
        &Validation::default(),
    )?;
    
    // Inject claims into request extensions
    request.extensions_mut().insert(claims.claims);
    
    next.run(request).await
}
```

---

## Endpoints

### Endpoint Summary

| Method | Path | Description | Auth |
|--------|------|-------------|------|
| POST | `/classify` | Classify content for violations | API Key |
| POST | `/classify/batch` | Batch classification | API Key |
| POST | `/action` | Apply moderation action | JWT |
| GET | `/action/:id` | Get action details | JWT |
| POST | `/report` | Submit user report | Session |
| GET | `/report/:id` | Get report status | Session |
| POST | `/appeal` | Submit appeal | Session |
| GET | `/appeal/:id` | Get appeal status | Session |
| GET | `/health` | Health check | None |
| GET | `/metrics` | Prometheus metrics | Internal |

---

## Classification API

### POST /classify

Classify a single piece of content.

**Request:**

```json
{
  "content_type": "text",
  "content": "Hello, this is a test message",
  "context": {
    "channel": "public_chat",
    "user_age": 16,
    "region": "EU"
  },
  "options": {
    "return_explanation": true,
    "confidence_threshold": 0.9
  }
}
```

**Response:**

```json
{
  "success": true,
  "data": {
    "classification_id": "cls_abc123",
    "action": "allow",
    "confidence": 0.97,
    "categories": [],
    "flags": [],
    "explanation": {
      "summary": "Content appears safe",
      "factors": [
        {"name": "toxicity", "score": 0.02, "weight": 0.4},
        {"name": "spam", "score": 0.01, "weight": 0.2}
      ]
    },
    "processing_time_ms": 12
  },
  "meta": {
    "request_id": "req_xyz789",
    "model_version": "text-v1.2.0"
  }
}
```

**Rust Implementation:**

```rust
// crates/api/src/routes/classify.rs
use axum::{extract::State, Json};

#[derive(Deserialize)]
pub struct ClassifyRequest {
    pub content_type: ContentType,
    pub content: String,
    pub context: Option<ClassificationContext>,
    pub options: Option<ClassificationOptions>,
}

#[derive(Serialize)]
pub struct ClassifyResponse {
    pub classification_id: String,
    pub action: String,
    pub confidence: f32,
    pub categories: Vec<String>,
    pub flags: Vec<String>,
    pub explanation: Option<Explanation>,
    pub processing_time_ms: u64,
}

pub async fn classify(
    State(state): State<AppState>,
    Json(request): Json<ClassifyRequest>,
) -> Result<Json<ApiResponse<ClassifyResponse>>, ApiError> {
    let start = std::time::Instant::now();
    
    // Build content object
    let content = Content {
        id: uuid::Uuid::new_v4().to_string(),
        content_type: request.content_type,
        data: ContentData::from_string(&request.content, &request.content_type),
        context: request.context.unwrap_or_default(),
    };
    
    // Run classification through orchestrator
    let decision = state.orchestrator.moderate(content).await?;
    
    // Apply confidence threshold
    let threshold = request.options
        .as_ref()
        .map(|o| o.confidence_threshold)
        .unwrap_or(0.9);
    
    let action = if decision.confidence >= threshold {
        decision.action.to_string()
    } else {
        "review".to_string()  // Below threshold = needs review
    };
    
    // Generate explanation if requested
    let explanation = if request.options.map(|o| o.return_explanation).unwrap_or(false) {
        Some(state.orchestrator.explain(&decision).await?)
    } else {
        None
    };
    
    Ok(Json(ApiResponse::success(ClassifyResponse {
        classification_id: decision.content_id,
        action,
        confidence: decision.confidence,
        categories: decision.categories.iter().map(|c| c.to_string()).collect(),
        flags: vec![],
        explanation,
        processing_time_ms: start.elapsed().as_millis() as u64,
    })))
}
```

### POST /classify/batch

Batch classification for multiple items.

**Request:**

```json
{
  "items": [
    { "id": "msg_1", "content_type": "text", "content": "Hello" },
    { "id": "msg_2", "content_type": "text", "content": "World" }
  ],
  "options": {
    "parallel": true,
    "max_concurrency": 10
  }
}
```

**Response:**

```json
{
  "success": true,
  "data": {
    "batch_id": "batch_123",
    "results": [
      { "id": "msg_1", "action": "allow", "confidence": 0.99 },
      { "id": "msg_2", "action": "allow", "confidence": 0.98 }
    ],
    "summary": {
      "total": 2,
      "allowed": 2,
      "flagged": 0,
      "removed": 0
    }
  }
}
```

**Rust Implementation:**

```rust
pub async fn classify_batch(
    State(state): State<AppState>,
    Json(request): Json<BatchClassifyRequest>,
) -> Result<Json<ApiResponse<BatchClassifyResponse>>, ApiError> {
    let concurrency = request.options
        .as_ref()
        .map(|o| o.max_concurrency)
        .unwrap_or(10);
    
    // Process in parallel with bounded concurrency
    let results: Vec<ClassifyResult> = futures::stream::iter(request.items)
        .map(|item| {
            let orchestrator = state.orchestrator.clone();
            async move {
                let content = Content::from_request_item(&item);
                let decision = orchestrator.moderate(content).await?;
                Ok::<_, ApiError>(ClassifyResult {
                    id: item.id,
                    action: decision.action.to_string(),
                    confidence: decision.confidence,
                })
            }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;
    
    // Aggregate results
    let summary = BatchSummary::from_results(&results);
    
    Ok(Json(ApiResponse::success(BatchClassifyResponse {
        batch_id: uuid::Uuid::new_v4().to_string(),
        results,
        summary,
    })))
}
```

---

## Action API

### POST /action

Apply a moderation action to content.

**Request:**

```json
{
  "content_id": "content_abc123",
  "action": "remove",
  "reason": "Violates community guidelines",
  "classification_id": "cls_abc123",
  "notify_user": true
}
```

**Response:**

```json
{
  "success": true,
  "data": {
    "action_id": "act_xyz789",
    "status": "applied",
    "content_id": "content_abc123",
    "action": "remove",
    "applied_at": "2025-12-03T20:00:00Z",
    "reversible": true,
    "appeal_available": true
  }
}
```

**Rust Implementation:**

```rust
// crates/api/src/routes/action.rs

#[derive(Deserialize)]
pub struct ApplyActionRequest {
    pub content_id: String,
    pub action: ActionType,
    pub reason: String,
    pub classification_id: Option<String>,
    pub notify_user: bool,
}

#[derive(Deserialize)]
pub enum ActionType {
    #[serde(rename = "warn")]
    Warn,
    #[serde(rename = "hide")]
    Hide,
    #[serde(rename = "remove")]
    Remove,
    #[serde(rename = "ban")]
    Ban { duration_hours: Option<u64> },
}

pub async fn apply_action(
    State(state): State<AppState>,
    claims: Claims,
    Json(request): Json<ApplyActionRequest>,
) -> Result<Json<ApiResponse<ActionResponse>>, ApiError> {
    // Verify permission
    if !claims.permissions.contains(&Permission::ActionApply) {
        return Err(ApiError::Forbidden);
    }
    
    // Create action record
    let action = ModerationActionRecord {
        id: uuid::Uuid::new_v4().to_string(),
        content_id: request.content_id.clone(),
        action_type: request.action.clone(),
        reason: request.reason,
        applied_by: claims.sub,
        classification_id: request.classification_id,
        applied_at: chrono::Utc::now(),
    };
    
    // Store action
    sqlx::query!(
        "INSERT INTO moderation_actions (id, content_id, action_type, reason, applied_by, applied_at) 
         VALUES ($1, $2, $3, $4, $5, $6)",
        action.id, action.content_id, 
        serde_json::to_string(&action.action_type)?,
        action.reason, action.applied_by, action.applied_at
    )
    .execute(&state.db)
    .await?;
    
    // Execute action
    match &request.action {
        ActionType::Remove => {
            state.content_service.remove(&request.content_id).await?;
        }
        ActionType::Hide => {
            state.content_service.hide(&request.content_id).await?;
        }
        ActionType::Ban { duration_hours } => {
            let user_id = state.content_service.get_author(&request.content_id).await?;
            state.user_service.ban(&user_id, *duration_hours).await?;
        }
        ActionType::Warn => {
            // Just record, no action needed
        }
    }
    
    // Notify user if requested
    if request.notify_user {
        let user_id = state.content_service.get_author(&request.content_id).await?;
        state.notification_service.notify_moderation_action(&user_id, &action).await?;
    }
    
    Ok(Json(ApiResponse::success(ActionResponse {
        action_id: action.id,
        status: "applied".into(),
        content_id: request.content_id,
        action: request.action.to_string(),
        applied_at: action.applied_at,
        reversible: true,
        appeal_available: true,
    })))
}
```

---

## Reporting API

### POST /report

Submit a user report.

**Request:**

```json
{
  "content_id": "content_abc123",
  "reason": "harassment",
  "details": "This user is sending threatening messages",
  "evidence": [
    { "type": "screenshot", "url": "https://..." }
  ]
}
```

**Response:**

```json
{
  "success": true,
  "data": {
    "report_id": "rpt_abc123",
    "status": "received",
    "estimated_review_time": "24-48 hours",
    "reference_number": "RPT-2025-12345"
  }
}
```

**Rust Implementation:**

```rust
// crates/api/src/routes/report.rs

#[derive(Deserialize)]
pub struct ReportRequest {
    pub content_id: String,
    pub reason: ReportReason,
    pub details: Option<String>,
    pub evidence: Option<Vec<Evidence>>,
}

#[derive(Deserialize)]
pub enum ReportReason {
    Harassment,
    Spam,
    Violence,
    Hate,
    SexualContent,
    ChildSafety,
    Ncii,  // Non-consensual intimate imagery
    Scam,
    Impersonation,
    Other,
}

pub async fn submit_report(
    State(state): State<AppState>,
    session: Session,
    Json(request): Json<ReportRequest>,
) -> Result<Json<ApiResponse<ReportResponse>>, ApiError> {
    // Create report
    let report = UserReport {
        id: uuid::Uuid::new_v4().to_string(),
        content_id: request.content_id.clone(),
        reporter_id: session.user_id,
        reason: request.reason.clone(),
        details: request.details,
        evidence: request.evidence,
        status: ReportStatus::Received,
        created_at: chrono::Utc::now(),
    };
    
    // Store report
    sqlx::query!(
        "INSERT INTO user_reports (id, content_id, reporter_id, reason, details, status, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
        report.id, report.content_id, report.reporter_id,
        format!("{:?}", report.reason),
        report.details, format!("{:?}", report.status),
        report.created_at
    )
    .execute(&state.db)
    .await?;
    
    // Priority routing for sensitive reports
    let priority = match request.reason {
        ReportReason::ChildSafety | ReportReason::Ncii => Priority::Critical,
        ReportReason::Violence | ReportReason::Harassment => Priority::High,
        _ => Priority::Normal,
    };
    
    // Queue for review
    state.review_queue.enqueue(ReviewItem {
        report_id: report.id.clone(),
        priority,
        auto_classified: false,
    }).await?;
    
    // For NCII, also trigger Take It Down pipeline
    if matches!(request.reason, ReportReason::Ncii) {
        state.tida_pipeline.process_from_report(&report).await?;
    }
    
    Ok(Json(ApiResponse::success(ReportResponse {
        report_id: report.id,
        status: "received".into(),
        estimated_review_time: priority.estimated_time(),
        reference_number: format!("RPT-{}", chrono::Utc::now().format("%Y-%H%M%S")),
    })))
}
```

---

## Appeal API

### POST /appeal

Submit an appeal for a moderation decision.

**Request:**

```json
{
  "action_id": "act_xyz789",
  "reason": "false_positive",
  "explanation": "This content was taken out of context...",
  "evidence": []
}
```

**Response:**

```json
{
  "success": true,
  "data": {
    "appeal_id": "apl_abc123",
    "status": "submitted",
    "review_timeline": "3-5 business days",
    "reference_number": "APL-2025-67890"
  }
}
```

**Rust Implementation:**

```rust
// crates/api/src/routes/appeal.rs

#[derive(Deserialize)]
pub struct AppealRequest {
    pub action_id: String,
    pub reason: AppealReason,
    pub explanation: String,
    pub evidence: Option<Vec<Evidence>>,
}

#[derive(Deserialize, Clone, Debug)]
pub enum AppealReason {
    // Content Misclassification
    FalsePositive,
    OutOfContext,
    MisunderstoodIntent,
    
    // Protected Expression (1st Amendment)
    SatireOrParody,
    ArtisticExpression,
    PoliticalCommentary,
    ReligiousExpression,
    EducationalContent,
    NewsOrDocumentary,
    
    // Technical Issues
    AiMisclassification,
    WrongContentFlagged,
    DuplicateReport,
    
    // Contextual Defense
    PrivateConversation,
    ConsentObtained,
    SelfDeprecating,
    FriendlyBanter,
    RoleplayContext,
    
    // Procedural Issues
    NoWarningGiven,
    DisproportionatePunishment,
    InconsistentEnforcement,
    
    // Identity/Ownership
    ContentOwnership,
    Impersonation,
    HackedAccount,
    
    Other { explanation: String },
}

pub async fn submit_appeal(
    State(state): State<AppState>,
    session: Session,
    Json(request): Json<AppealRequest>,
) -> Result<Json<ApiResponse<AppealResponse>>, ApiError> {
    // Verify action exists and user can appeal
    let action = state.db.get_action(&request.action_id).await?
        .ok_or(ApiError::NotFound)?;
    
    let content = state.content_service.get(&action.content_id).await?;
    if content.author_id != session.user_id {
        return Err(ApiError::Forbidden);
    }
    
    // Check appeal window (7 days)
    let appeal_deadline = action.applied_at + chrono::Duration::days(7);
    if chrono::Utc::now() > appeal_deadline {
        return Err(ApiError::AppealWindowClosed);
    }
    
    // Create appeal
    let appeal = Appeal {
        id: uuid::Uuid::new_v4().to_string(),
        action_id: request.action_id,
        appellant_id: session.user_id,
        reason: request.reason,
        explanation: request.explanation,
        evidence: request.evidence,
        status: AppealStatus::Submitted,
        created_at: chrono::Utc::now(),
    };
    
    // Store appeal
    sqlx::query!(
        "INSERT INTO appeals (id, action_id, appellant_id, reason, explanation, status, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
        appeal.id, appeal.action_id, appeal.appellant_id,
        format!("{:?}", appeal.reason),
        appeal.explanation, format!("{:?}", appeal.status),
        appeal.created_at
    )
    .execute(&state.db)
    .await?;
    
    // Queue for human review
    state.appeal_queue.enqueue(appeal.id.clone()).await?;
    
    Ok(Json(ApiResponse::success(AppealResponse {
        appeal_id: appeal.id,
        status: "submitted".into(),
        review_timeline: "3-5 business days".into(),
        reference_number: format!("APL-{}", chrono::Utc::now().format("%Y-%H%M%S")),
    })))
}
```

---

## Webhook Integration

### Webhook Events

| Event | Trigger | Payload |
|-------|---------|---------|
| `moderation.action.applied` | Action applied to content | Action details |
| `moderation.report.received` | New report submitted | Report summary |
| `moderation.report.resolved` | Report resolved | Resolution details |
| `moderation.appeal.submitted` | New appeal submitted | Appeal summary |
| `moderation.appeal.decided` | Appeal decision made | Decision details |

### Webhook Configuration

```json
{
  "url": "https://your-service.com/webhooks/moderation",
  "events": ["moderation.action.applied", "moderation.report.resolved"],
  "secret": "whsec_...",
  "enabled": true
}
```

### Webhook Delivery

```rust
// crates/api/src/webhooks.rs

pub struct WebhookDelivery {
    client: reqwest::Client,
    retry_policy: RetryPolicy,
}

impl WebhookDelivery {
    pub async fn deliver(&self, webhook: &Webhook, event: WebhookEvent) -> Result<(), WebhookError> {
        let payload = WebhookPayload {
            id: uuid::Uuid::new_v4().to_string(),
            event_type: event.event_type.clone(),
            created_at: chrono::Utc::now(),
            data: event.data,
        };
        
        // Sign payload
        let signature = self.sign(&webhook.secret, &payload)?;
        
        // Deliver with retries
        let mut attempts = 0;
        loop {
            let result = self.client
                .post(&webhook.url)
                .header("Content-Type", "application/json")
                .header("X-Webhook-Signature", &signature)
                .header("X-Webhook-ID", &payload.id)
                .json(&payload)
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await;
            
            match result {
                Ok(response) if response.status().is_success() => {
                    return Ok(());
                }
                Ok(response) => {
                    attempts += 1;
                    if attempts >= self.retry_policy.max_attempts {
                        return Err(WebhookError::MaxRetriesExceeded);
                    }
                    tokio::time::sleep(self.retry_policy.delay(attempts)).await;
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= self.retry_policy.max_attempts {
                        return Err(WebhookError::DeliveryFailed(e.to_string()));
                    }
                    tokio::time::sleep(self.retry_policy.delay(attempts)).await;
                }
            }
        }
    }
    
    fn sign(&self, secret: &str, payload: &WebhookPayload) -> Result<String, WebhookError> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
        mac.update(serde_json::to_string(payload)?.as_bytes());
        
        Ok(hex::encode(mac.finalize().into_bytes()))
    }
}
```

---

## Rate Limiting

### Rate Limit Tiers

| Tier | Classify | Batch | Actions | Reports |
|------|----------|-------|---------|---------|
| Free | 100/min | 10/min | 10/min | 50/day |
| Pro | 1000/min | 100/min | 100/min | Unlimited |
| Enterprise | 10000/min | 1000/min | 1000/min | Unlimited |

### Implementation

```rust
// crates/api/src/middleware/rate_limit.rs
use tower::limit::RateLimitLayer;
use redis::AsyncCommands;

pub struct RateLimiter {
    redis: redis::aio::ConnectionManager,
}

impl RateLimiter {
    pub async fn check(&self, client_id: &str, endpoint: &str) -> Result<RateLimitResult, RateLimitError> {
        let key = format!("ratelimit:{}:{}", client_id, endpoint);
        let window_secs = 60;
        
        // Get current count
        let count: u64 = self.redis.incr(&key, 1).await?;
        
        // Set expiry on first request
        if count == 1 {
            self.redis.expire(&key, window_secs).await?;
        }
        
        // Get limit for client tier
        let limit = self.get_limit(client_id, endpoint).await?;
        
        if count > limit {
            return Ok(RateLimitResult::Exceeded {
                limit,
                reset_at: chrono::Utc::now() + chrono::Duration::seconds(window_secs as i64),
            });
        }
        
        Ok(RateLimitResult::Allowed {
            remaining: limit - count,
            limit,
        })
    }
}

pub fn rate_limit_layer() -> axum::middleware::from_fn<RateLimitMiddleware> {
    axum::middleware::from_fn(rate_limit_middleware)
}

async fn rate_limit_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<Body>,
    next: Next<Body>,
) -> Response {
    let client_id = headers
        .get("X-Client-ID")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous");
    
    let endpoint = request.uri().path();
    
    match state.rate_limiter.check(client_id, endpoint).await {
        Ok(RateLimitResult::Allowed { remaining, limit }) => {
            let mut response = next.run(request).await;
            response.headers_mut().insert("X-RateLimit-Remaining", remaining.into());
            response.headers_mut().insert("X-RateLimit-Limit", limit.into());
            response
        }
        Ok(RateLimitResult::Exceeded { limit, reset_at }) => {
            Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header("X-RateLimit-Limit", limit)
                .header("X-RateLimit-Reset", reset_at.to_rfc3339())
                .header("Retry-After", "60")
                .body(Body::from("Rate limit exceeded"))
                .unwrap()
        }
        Err(_) => {
            // On error, allow request (fail open for availability)
            next.run(request).await
        }
    }
}
```

---

## Rust Implementation

### Router Setup

```rust
// crates/api/src/main.rs
use axum::{
    routing::{get, post},
    Router,
};

#[tokio::main]
async fn main() {
    // Initialize state
    let state = AppState::new().await;
    
    // Build router
    let app = Router::new()
        // Classification endpoints
        .route("/v1/moderation/classify", post(routes::classify::classify))
        .route("/v1/moderation/classify/batch", post(routes::classify::classify_batch))
        
        // Action endpoints
        .route("/v1/moderation/action", post(routes::action::apply_action))
        .route("/v1/moderation/action/:id", get(routes::action::get_action))
        
        // Report endpoints
        .route("/v1/moderation/report", post(routes::report::submit_report))
        .route("/v1/moderation/report/:id", get(routes::report::get_report))
        
        // Appeal endpoints
        .route("/v1/moderation/appeal", post(routes::appeal::submit_appeal))
        .route("/v1/moderation/appeal/:id", get(routes::appeal::get_appeal))
        
        // Health and metrics
        .route("/health", get(routes::health::health_check))
        .route("/metrics", get(routes::metrics::prometheus_metrics))
        
        // Middleware
        .layer(rate_limit_layer())
        .layer(auth_layer())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);
    
    // Start server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    tracing::info!("Moderation API listening on :8080");
    axum::serve(listener, app).await.unwrap();
}
```

### Crate Dependencies

```toml
# crates/api/Cargo.toml
[dependencies]
axum = "0.7"
tokio = { version = "1", features = ["full"] }
tower = "0.4"
tower-http = { version = "0.5", features = ["trace", "cors"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio"] }
redis = { version = "0.24", features = ["tokio-comp"] }
jsonwebtoken = "9"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = "0.3"
reqwest = { version = "0.11", features = ["json"] }
hmac = "0.12"
sha2 = "0.10"
hex = "0.4"
futures = "0.3"
```

---

## Error Responses

### Error Format

```json
{
  "success": false,
  "data": null,
  "errors": [
    {
      "code": "RATE_LIMIT_EXCEEDED",
      "message": "Too many requests",
      "details": {
        "limit": 100,
        "reset_at": "2025-12-03T20:05:00Z"
      }
    }
  ]
}
```

### Error Codes

| Code | HTTP Status | Description |
|------|-------------|-------------|
| `UNAUTHORIZED` | 401 | Invalid or missing authentication |
| `FORBIDDEN` | 403 | Insufficient permissions |
| `NOT_FOUND` | 404 | Resource not found |
| `RATE_LIMIT_EXCEEDED` | 429 | Rate limit exceeded |
| `VALIDATION_ERROR` | 400 | Invalid request parameters |
| `CONTENT_NOT_FOUND` | 404 | Referenced content doesn't exist |
| `APPEAL_WINDOW_CLOSED` | 400 | Appeal deadline passed |
| `INTERNAL_ERROR` | 500 | Unexpected server error |

---

## Real-Time Chat Filtering

### Live Text Classification

Chat messages are filtered in real-time before display. Profanity is replaced with symbols rather than blocked.

```rust
// crates/api/src/chat/filter.rs
use regex::Regex;
use lazy_static::lazy_static;

/// Real-time chat filter with symbol replacement
pub struct ChatFilter {
    profanity_patterns: Vec<CompiledPattern>,
    pii_detector: PiiDetector,
    platform_detector: PlatformMentionDetector,
}

lazy_static! {
    /// Symbol set for replacement: #$&*!@%
    static ref REPLACEMENT_CHARS: Vec<char> = vec!['#', '$', '&', '*', '!', '@', '%'];
}

impl ChatFilter {
    /// Filter message in real-time (<5ms target)
    pub fn filter(&self, message: &str) -> FilteredMessage {
        let mut result = message.to_string();
        let mut redactions = vec![];
        
        // 1. Replace profanity with symbols
        for pattern in &self.profanity_patterns {
            result = pattern.regex.replace_all(&result, |caps: &regex::Captures| {
                let matched = caps.get(0).unwrap().as_str();
                self.generate_symbols(matched.len())
            }).to_string();
        }
        
        // 2. Redact PII (including misspellings)
        let pii_findings = self.pii_detector.detect_fuzzy(&result);
        for finding in &pii_findings {
            result = result.replace(&finding.text, "[REDACTED]");
            redactions.push(Redaction::Pii(finding.clone()));
        }
        
        // 3. Redact platform mentions (Discord, Twitter, etc.)
        let platform_mentions = self.platform_detector.detect(&result);
        for mention in &platform_mentions {
            result = result.replace(&mention.text, "[LINK REMOVED]");
            redactions.push(Redaction::Platform(mention.clone()));
        }
        
        FilteredMessage {
            original_hash: hash_message(message),  // For audit
            filtered: result,
            redactions,
            was_modified: message != result,
        }
    }
    
    /// Generate random symbols to replace profanity
    fn generate_symbols(&self, len: usize) -> String {
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        
        (0..len)
            .map(|_| *REPLACEMENT_CHARS.choose(&mut rng).unwrap())
            .collect()
    }
}

/// PII detector with fuzzy matching for misspellings
pub struct PiiDetector {
    /// Common PII patterns (SSN, phone, email, etc.)
    patterns: Vec<Regex>,
    
    /// Fuzzy name matcher
    name_matcher: FuzzyMatcher,
}

impl PiiDetector {
    /// Detect PII including common misspellings/obfuscations
    pub fn detect_fuzzy(&self, text: &str) -> Vec<PiiFindings> {
        let mut findings = vec![];
        
        // Standard pattern matching
        for pattern in &self.patterns {
            for m in pattern.find_iter(text) {
                findings.push(PiiFindings {
                    text: m.as_str().to_string(),
                    pii_type: PiiType::from_pattern(pattern),
                    confidence: 1.0,
                });
            }
        }
        
        // Fuzzy matching for obfuscated PII
        // e.g., "5 5 5 - 1 2 - 3 4 5 6" -> SSN
        // e.g., "my email is john dot doe at geemail" -> email
        let normalized = self.normalize_obfuscation(text);
        for pattern in &self.patterns {
            for m in pattern.find_iter(&normalized) {
                // Map back to original text position
                findings.push(PiiFindings {
                    text: self.map_to_original(text, &normalized, m.start(), m.end()),
                    pii_type: PiiType::from_pattern(pattern),
                    confidence: 0.85,
                });
            }
        }
        
        findings
    }
    
    fn normalize_obfuscation(&self, text: &str) -> String {
        text
            .replace(" dot ", ".")
            .replace(" at ", "@")
            .replace("(at)", "@")
            .replace("[at]", "@")
            .replace(" dash ", "-")
            .chars()
            .filter(|c| !c.is_whitespace() || *c == ' ')
            .collect()
    }
}

/// Platform mention detector (Discord, Twitter, etc.)
pub struct PlatformMentionDetector {
    patterns: Vec<PlatformPattern>,
}

#[derive(Clone)]
pub struct PlatformPattern {
    pub platform: String,
    pub patterns: Vec<Regex>,  // Includes misspellings
}

impl PlatformMentionDetector {
    pub fn new() -> Self {
        Self {
            patterns: vec![
                PlatformPattern {
                    platform: "Discord".into(),
                    patterns: vec![
                        Regex::new(r"(?i)discord\.gg/\w+").unwrap(),
                        Regex::new(r"(?i)disc[o0]rd").unwrap(),  // d1scord, disc0rd
                        Regex::new(r"(?i)my\s+dc\s*:\s*\w+#\d+").unwrap(),
                    ],
                },
                PlatformPattern {
                    platform: "Twitter/X".into(),
                    patterns: vec![
                        Regex::new(r"(?i)twitter\.com/\w+").unwrap(),
                        Regex::new(r"(?i)x\.com/\w+").unwrap(),
                        Regex::new(r"(?i)@\w+\s+on\s+(twitter|x)").unwrap(),
                    ],
                },
                PlatformPattern {
                    platform: "Instagram".into(),
                    patterns: vec![
                        Regex::new(r"(?i)instagram\.com/\w+").unwrap(),
                        Regex::new(r"(?i)insta(gram)?\s*:\s*@?\w+").unwrap(),
                    ],
                },
                // Add more platforms...
            ],
        }
    }
}
```

---

## Community Consensus System

### User Ratings for Content Moderation

Content receives thumbs up/down ratings. Low-rated content is flagged for AI review.

```rust
// crates/api/src/routes/consensus.rs

/// Content rating for community consensus
#[derive(Deserialize)]
pub struct RatingRequest {
    pub content_id: String,
    pub rating: Rating,
    pub reason: Option<String>,
}

#[derive(Deserialize, Clone, Copy)]
pub enum Rating {
    ThumbsUp,
    ThumbsDown,
}

/// POST /v1/content/:id/rate
pub async fn rate_content(
    State(state): State<AppState>,
    session: Session,
    Path(content_id): Path<String>,
    Json(request): Json<RatingRequest>,
) -> Result<Json<ApiResponse<RatingResponse>>, ApiError> {
    // Rate limit: 1 rating per content per user
    if state.has_rated(&session.user_id, &content_id).await? {
        return Err(ApiError::AlreadyRated);
    }
    
    // Store rating
    sqlx::query!(
        "INSERT INTO content_ratings (content_id, user_id, rating, reason, created_at)
         VALUES ($1, $2, $3, $4, NOW())",
        content_id, session.user_id,
        matches!(request.rating, Rating::ThumbsUp),
        request.reason
    )
    .execute(&state.db)
    .await?;
    
    // Calculate new consensus
    let consensus = calculate_consensus(&state.db, &content_id).await?;
    
    // Check if flagging threshold reached
    if consensus.should_flag() {
        state.ai_review_queue.enqueue(AiReviewItem {
            content_id: content_id.clone(),
            trigger: ReviewTrigger::CommunityConsensus,
            consensus_score: consensus.score,
            down_vote_percentage: consensus.down_percentage,
        }).await?;
    }
    
    Ok(Json(ApiResponse::success(RatingResponse {
        content_id,
        new_score: consensus.score,
        total_ratings: consensus.total,
    })))
}

/// Consensus calculation with weighted voting
#[derive(Debug)]
pub struct ContentConsensus {
    pub content_id: String,
    pub up_votes: u64,
    pub down_votes: u64,
    pub total: u64,
    pub score: f32,           // -1.0 to 1.0
    pub down_percentage: f32,
}

impl ContentConsensus {
    /// Flag for AI review if:
    /// - >40% downvotes AND >10 total votes
    /// - OR >5 downvotes from high-reputation users
    pub fn should_flag(&self) -> bool {
        (self.down_percentage > 0.4 && self.total >= 10) ||
        self.high_rep_down_votes > 5
    }
}

async fn calculate_consensus(db: &PgPool, content_id: &str) -> Result<ContentConsensus, DbError> {
    let stats = sqlx::query!(
        r#"
        SELECT 
            COUNT(*) FILTER (WHERE rating = true) as up_votes,
            COUNT(*) FILTER (WHERE rating = false) as down_votes,
            COUNT(*) as total
        FROM content_ratings
        WHERE content_id = $1
        "#,
        content_id
    )
    .fetch_one(db)
    .await?;
    
    let up = stats.up_votes.unwrap_or(0) as f32;
    let down = stats.down_votes.unwrap_or(0) as f32;
    let total = stats.total.unwrap_or(0) as f32;
    
    Ok(ContentConsensus {
        content_id: content_id.to_string(),
        up_votes: up as u64,
        down_votes: down as u64,
        total: total as u64,
        score: if total > 0.0 { (up - down) / total } else { 0.0 },
        down_percentage: if total > 0.0 { down / total } else { 0.0 },
    })
}
```

---

## Report Abuse System

### Automated Report Processing with Creator Consideration

```rust
// crates/api/src/routes/report_abuse.rs

/// Enhanced report with user history tracking
pub struct ReportAbuseSystem {
    db: PgPool,
    ai_classifier: ModerationOrchestrator,
    reputation_service: ReputationService,
}

impl ReportAbuseSystem {
    /// Process abuse report with reputation weighting
    pub async fn process_report(&self, report: UserReport) -> Result<ReportOutcome, ReportError> {
        // 1. Get reported user's reputation
        let reported_user = self.reputation_service
            .get_user(&report.reported_user_id)
            .await?;
        
        // 2. AI classification
        let ai_decision = self.ai_classifier
            .moderate(&report.content)
            .await?;
        
        // 3. Determine severity
        let severity = self.assess_severity(&ai_decision, &report);
        
        // 4. Add to user's report history
        self.add_to_history(&report.reported_user_id, &report).await?;
        
        // 5. Decision matrix based on severity + reputation
        let outcome = match severity {
            Severity::Critical => {
                // Immediate action regardless of reputation
                // CSAM, imminent threats, etc.
                ReportOutcome::ImmediateTakedown {
                    reason: ai_decision.categories.first().map(|c| c.to_string()),
                    notify_law_enforcement: severity.requires_law_enforcement(),
                }
            }
            Severity::High => {
                // High-rep users get expedited human review
                // Low-rep users get automated action
                if reported_user.reputation.score > 0.8 {
                    ReportOutcome::ExpeditedHumanReview {
                        priority: Priority::High,
                        reputation_context: reported_user.reputation.summary(),
                    }
                } else {
                    ReportOutcome::AutomatedAction {
                        action: ModerationAction::Hide { reason: "Under review".into() },
                    }
                }
            }
            Severity::Medium => {
                // All get human review, priority based on reputation
                ReportOutcome::QueuedForReview {
                    priority: if reported_user.reputation.score > 0.7 {
                        Priority::Low  // Trusted creators reviewed carefully
                    } else {
                        Priority::Normal
                    },
                }
            }
            Severity::Low | Severity::Mild => {
                // Can be declined if reputation is high
                if reported_user.reputation.score > 0.9 && 
                   ai_decision.confidence < 0.7 {
                    ReportOutcome::Declined {
                        reason: "Insufficient evidence against trusted creator".into(),
                        reporter_notified: true,
                    }
                } else {
                    ReportOutcome::QueuedForReview {
                        priority: Priority::Low,
                    }
                }
            }
        };
        
        // 6. Update reporter's accuracy history (affects their report weight)
        self.track_reporter_accuracy(&report.reporter_id).await?;
        
        Ok(outcome)
    }
    
    /// Add report to user's permanent history
    async fn add_to_history(&self, user_id: &str, report: &UserReport) -> Result<(), DbError> {
        sqlx::query!(
            "INSERT INTO user_report_history 
             (user_id, report_id, report_type, severity, created_at)
             VALUES ($1, $2, $3, $4, NOW())",
            user_id, report.id,
            format!("{:?}", report.reason),
            format!("{:?}", report.assessed_severity)
        )
        .execute(&self.db)
        .await?;
        
        // Recalculate reputation impact
        self.reputation_service.recalculate(user_id).await?;
        
        Ok(())
    }
}

#[derive(Debug)]
pub enum ReportOutcome {
    /// Immediate takedown (critical violations)
    ImmediateTakedown {
        reason: Option<String>,
        notify_law_enforcement: bool,
    },
    
    /// Fast-track human review for edge cases
    ExpeditedHumanReview {
        priority: Priority,
        reputation_context: String,
    },
    
    /// Automated action applied
    AutomatedAction {
        action: ModerationAction,
    },
    
    /// Queued for normal review
    QueuedForReview {
        priority: Priority,
    },
    
    /// Report declined (insufficient evidence)
    Declined {
        reason: String,
        reporter_notified: bool,
    },
}
```

---

## SDK Examples

### Rust Client

```rust
use moderation_client::ModerationClient;

#[tokio::main]
async fn main() {
    let client = ModerationClient::new("your_api_key");
    
    let result = client
        .classify()
        .text("Hello, world!")
        .context("public_chat")
        .send()
        .await
        .unwrap();
    
    println!("Action: {}, Confidence: {}", result.action, result.confidence);
}
```

### TypeScript Client

```typescript
import { ModerationClient } from '@eustress/moderation-sdk';

const client = new ModerationClient({ apiKey: 'your_api_key' });

const result = await client.classify({
  contentType: 'text',
  content: 'Hello, world!',
  context: { channel: 'public_chat' }
});

console.log(`Action: ${result.action}, Confidence: ${result.confidence}`);
```

---

## Related Documentation

- [AI_AGENTS.md](./AI_AGENTS.md) - AI agent architecture
- [TIDA.md](../legal/TIDA.md) - Take It Down Act integration
- [COPPA.md](../legal/COPPA.md) - Child safety filters
- [GDPR.md](../legal/GDPR.md) - Data protection compliance

---

**API Support:** api-support@simbuilder.com  
**Status Page:** status.eustress.io

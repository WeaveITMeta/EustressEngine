# DMCA Compliance Documentation

**Digital Millennium Copyright Act Implementation for Eustress Engine**

> *Best Match Dynamic: Takedown → AI pre-scan + DMCA agent, 24hr response SLA*

**Last Updated:** December 04, 2025  
**Status:** Pre-Release Compliance Framework  
**Applies To:** All user-generated content, assets, and creator uploads

---

## Table of Contents

1. [Overview](#overview)
2. [DMCA Safe Harbor Requirements](#dmca-safe-harbor-requirements)
3. [Designated Agent Registration](#designated-agent-registration)
4. [Takedown Process](#takedown-process)
5. [Counter-Notification Process](#counter-notification-process)
6. [AI-Assisted Detection](#ai-assisted-detection)
7. [Repeat Infringer Policy](#repeat-infringer-policy)
8. [Rust Implementation](#rust-implementation)
9. [Integration with Moderation API](#integration-with-moderation-api)

---

## Overview

### Regulatory Context

The **Digital Millennium Copyright Act (1998)** provides safe harbor for online service providers who:

| Requirement | Implementation |
|-------------|----------------|
| Designate DMCA agent | Registered with Copyright Office |
| Expeditious takedown | 24-hour response SLA |
| Counter-notification process | 10-14 business day restoration |
| Repeat infringer policy | 3-strike termination |
| No actual knowledge | AI pre-scan + good faith removal |

### Eustress Engine Compliance Strategy

```
Dynamic: Copyright + UGC Platform → Takedown
Implication: AI pre-scan for known hashes, DMCA agent workflow, 24hr SLA
Benefit: Safe harbor protection, creator trust, legal defensibility
```

**Mantra:** "Respect Creators" — Every copyright holder deserves swift, fair resolution.

---

## DMCA Safe Harbor Requirements

### §512(c) Safe Harbor Checklist

```rust
// crates/legal/src/dmca/safe_harbor.rs

/// DMCA Safe Harbor compliance checklist
#[derive(Debug, Clone)]
pub struct SafeHarborCompliance {
    /// 1. Designated agent registered with Copyright Office
    pub agent_registered: bool,
    
    /// 2. Agent contact info on website
    pub agent_info_published: bool,
    
    /// 3. Takedown procedure implemented
    pub takedown_procedure: bool,
    
    /// 4. Counter-notification procedure implemented
    pub counter_notification_procedure: bool,
    
    /// 5. Repeat infringer policy
    pub repeat_infringer_policy: bool,
    
    /// 6. No interference with standard technical measures
    pub respects_technical_measures: bool,
    
    /// 7. No actual knowledge of infringement
    pub no_actual_knowledge: bool,
    
    /// 8. No financial benefit from infringement
    pub no_financial_benefit: bool,
}

impl SafeHarborCompliance {
    pub fn is_compliant(&self) -> bool {
        self.agent_registered
            && self.agent_info_published
            && self.takedown_procedure
            && self.counter_notification_procedure
            && self.repeat_infringer_policy
            && self.respects_technical_measures
            && self.no_actual_knowledge
            && self.no_financial_benefit
    }
}
```

---

## Designated Agent Registration

### Agent Information

```toml
# config/dmca_agent.toml
[agent]
name = "Eustress Engine DMCA Agent"
organization = "Eustress Engine, Inc."
address = "TBD - Register before launch"
email = "dmca@eustress.io"
phone = "TBD"

[registration]
copyright_office_id = "TBD"
registration_date = "TBD"
renewal_date = "TBD"

[website_notice]
url = "https://eustress.io/legal/dmca"
```

### Website Notice Requirements

The following must be publicly accessible at `/legal/dmca`:

1. **Agent Name and Contact Information**
2. **Takedown Request Form**
3. **Counter-Notification Form**
4. **Repeat Infringer Policy Summary**
5. **Response Time Commitments**

---

## Takedown Process

### Takedown Request Requirements (§512(c)(3))

```rust
// crates/legal/src/dmca/takedown.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// DMCA Takedown Notice (§512(c)(3) compliant)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakedownNotice {
    /// Unique notice ID
    pub notice_id: String,
    
    /// Timestamp received
    pub received_at: DateTime<Utc>,
    
    /// Complainant information
    pub complainant: Complainant,
    
    /// Copyrighted work identification
    pub copyrighted_work: CopyrightedWork,
    
    /// Infringing material identification
    pub infringing_material: InfringingMaterial,
    
    /// Good faith statement
    pub good_faith_statement: bool,
    
    /// Accuracy statement under penalty of perjury
    pub accuracy_statement: bool,
    
    /// Authorization statement
    pub authorization_statement: bool,
    
    /// Physical or electronic signature
    pub signature: Signature,
    
    /// Processing status
    pub status: TakedownStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Complainant {
    pub name: String,
    pub organization: Option<String>,
    pub email: String,
    pub phone: Option<String>,
    pub address: String,
    pub is_owner: bool,
    pub authorized_agent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyrightedWork {
    /// Description of the copyrighted work
    pub description: String,
    
    /// URL or location of original work
    pub original_location: Option<String>,
    
    /// Registration number (if registered)
    pub registration_number: Option<String>,
    
    /// Type of work
    pub work_type: WorkType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkType {
    Image,
    Audio,
    Video,
    Model3D,
    Code,
    Text,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfringingMaterial {
    /// Asset IDs on our platform
    pub asset_ids: Vec<String>,
    
    /// URLs where infringing content is located
    pub urls: Vec<String>,
    
    /// Description of how it infringes
    pub infringement_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Signature {
    Electronic { name: String, ip_address: String, timestamp: DateTime<Utc> },
    Physical { scanned_document_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TakedownStatus {
    /// Notice received, pending review
    Received,
    
    /// Notice validated, content removed
    Processed { removed_at: DateTime<Utc> },
    
    /// Notice rejected (incomplete or invalid)
    Rejected { reason: String },
    
    /// Counter-notification received, waiting period
    CounterNotified { counter_notice_id: String, restore_date: DateTime<Utc> },
    
    /// Content restored after counter-notification
    Restored { restored_at: DateTime<Utc> },
    
    /// Escalated to legal
    Escalated { reason: String },
}
```

### Takedown Workflow

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        DMCA TAKEDOWN WORKFLOW                            │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
                        ┌───────────────────┐
                        │  Notice Received  │
                        │   (via form/email)│
                        └─────────┬─────────┘
                                  │
                                  ▼
                        ┌───────────────────┐
                        │  Validate Notice  │◄──────────────────┐
                        │  (§512(c)(3))     │                   │
                        └─────────┬─────────┘                   │
                                  │                             │
                    ┌─────────────┴─────────────┐               │
                    │                           │               │
                    ▼                           ▼               │
            ┌───────────┐               ┌───────────┐           │
            │  Valid    │               │  Invalid  │───────────┘
            └─────┬─────┘               └───────────┘  Request
                  │                                    more info
                  ▼
        ┌───────────────────┐
        │  Remove Content   │ ◄─── Within 24 hours
        │  (Expeditious)    │
        └─────────┬─────────┘
                  │
                  ▼
        ┌───────────────────┐
        │  Notify Uploader  │
        │  (Counter-notice  │
        │   rights)         │
        └─────────┬─────────┘
                  │
        ┌─────────┴─────────┐
        │                   │
        ▼                   ▼
┌───────────────┐   ┌───────────────┐
│ No Counter    │   │ Counter-Notice│
│ (Stays down)  │   │ Received      │
└───────────────┘   └───────┬───────┘
                            │
                            ▼
                    ┌───────────────┐
                    │ 10-14 Day     │
                    │ Waiting Period│
                    └───────┬───────┘
                            │
              ┌─────────────┴─────────────┐
              │                           │
              ▼                           ▼
      ┌───────────────┐           ┌───────────────┐
      │ No Court      │           │ Court Action  │
      │ Action Filed  │           │ Filed         │
      └───────┬───────┘           └───────┬───────┘
              │                           │
              ▼                           ▼
      ┌───────────────┐           ┌───────────────┐
      │ Restore       │           │ Keep Down     │
      │ Content       │           │ Pending Court │
      └───────────────┘           └───────────────┘
```

### Takedown Service Implementation

```rust
// crates/legal/src/dmca/service.rs

use sqlx::PgPool;
use chrono::{Duration, Utc};

pub struct DmcaService {
    db: PgPool,
    notification_service: NotificationService,
    asset_service: AssetService,
    moderation_api: ModerationApiClient,
}

impl DmcaService {
    /// Process incoming takedown notice
    pub async fn process_takedown(&self, notice: TakedownNotice) -> Result<TakedownResult, DmcaError> {
        // 1. Validate notice completeness
        self.validate_notice(&notice)?;
        
        // 2. Store notice in database
        let notice_id = self.store_notice(&notice).await?;
        
        // 3. Remove content expeditiously (within 24 hours)
        for asset_id in &notice.infringing_material.asset_ids {
            self.asset_service.remove_asset(asset_id, RemovalReason::DmcaTakedown {
                notice_id: notice_id.clone(),
            }).await?;
        }
        
        // 4. Notify uploader of their counter-notification rights
        self.notify_uploader(&notice, &notice_id).await?;
        
        // 5. Log for compliance records
        self.log_takedown(&notice_id, &notice).await?;
        
        Ok(TakedownResult {
            notice_id,
            assets_removed: notice.infringing_material.asset_ids.len(),
            processed_at: Utc::now(),
        })
    }
    
    /// Validate takedown notice per §512(c)(3)
    fn validate_notice(&self, notice: &TakedownNotice) -> Result<(), DmcaError> {
        // Must have complainant contact info
        if notice.complainant.email.is_empty() {
            return Err(DmcaError::IncompleteNotice("Missing complainant email".into()));
        }
        
        // Must identify copyrighted work
        if notice.copyrighted_work.description.is_empty() {
            return Err(DmcaError::IncompleteNotice("Missing copyrighted work description".into()));
        }
        
        // Must identify infringing material
        if notice.infringing_material.asset_ids.is_empty() && notice.infringing_material.urls.is_empty() {
            return Err(DmcaError::IncompleteNotice("Missing infringing material identification".into()));
        }
        
        // Must have required statements
        if !notice.good_faith_statement {
            return Err(DmcaError::IncompleteNotice("Missing good faith statement".into()));
        }
        
        if !notice.accuracy_statement {
            return Err(DmcaError::IncompleteNotice("Missing accuracy statement under penalty of perjury".into()));
        }
        
        if !notice.authorization_statement {
            return Err(DmcaError::IncompleteNotice("Missing authorization statement".into()));
        }
        
        Ok(())
    }
    
    /// Process counter-notification
    pub async fn process_counter_notification(
        &self,
        counter: CounterNotification,
    ) -> Result<CounterResult, DmcaError> {
        // 1. Validate counter-notification
        self.validate_counter(&counter)?;
        
        // 2. Store counter-notification
        let counter_id = self.store_counter(&counter).await?;
        
        // 3. Calculate restore date (10-14 business days)
        let restore_date = self.calculate_restore_date();
        
        // 4. Notify original complainant
        self.notify_complainant(&counter, &restore_date).await?;
        
        // 5. Update takedown status
        self.update_takedown_status(
            &counter.original_notice_id,
            TakedownStatus::CounterNotified {
                counter_notice_id: counter_id.clone(),
                restore_date,
            },
        ).await?;
        
        Ok(CounterResult {
            counter_id,
            restore_date,
            status: CounterStatus::Pending,
        })
    }
    
    /// Restore content after waiting period (if no court action)
    pub async fn check_and_restore(&self) -> Result<Vec<String>, DmcaError> {
        let pending_restores = sqlx::query!(
            r#"
            SELECT notice_id, counter_notice_id, restore_date
            FROM dmca_takedowns
            WHERE status = 'counter_notified'
            AND restore_date <= NOW()
            AND court_action_filed = false
            "#
        )
        .fetch_all(&self.db)
        .await?;
        
        let mut restored = vec![];
        
        for record in pending_restores {
            // Restore the content
            self.restore_content(&record.notice_id).await?;
            restored.push(record.notice_id);
        }
        
        Ok(restored)
    }
    
    fn calculate_restore_date(&self) -> DateTime<Utc> {
        // 10-14 business days from now
        // Using 14 calendar days as safe default
        Utc::now() + Duration::days(14)
    }
}
```

---

## Counter-Notification Process

### Counter-Notification Requirements (§512(g)(3))

```rust
// crates/legal/src/dmca/counter.rs

/// DMCA Counter-Notification (§512(g)(3) compliant)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterNotification {
    /// Counter-notification ID
    pub counter_id: String,
    
    /// Original takedown notice ID
    pub original_notice_id: String,
    
    /// Subscriber/uploader information
    pub subscriber: Subscriber,
    
    /// Identification of removed material
    pub removed_material: RemovedMaterial,
    
    /// Statement under penalty of perjury
    pub perjury_statement: bool,
    
    /// Good faith belief statement
    pub good_faith_belief: String,
    
    /// Consent to jurisdiction
    pub jurisdiction_consent: JurisdictionConsent,
    
    /// Physical or electronic signature
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscriber {
    pub name: String,
    pub email: String,
    pub address: String,
    pub phone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemovedMaterial {
    /// Asset IDs that were removed
    pub asset_ids: Vec<String>,
    
    /// URLs where content was located
    pub original_urls: Vec<String>,
    
    /// Description of why removal was erroneous
    pub removal_error_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JurisdictionConsent {
    /// Consent to federal district court jurisdiction
    pub federal_court_consent: bool,
    
    /// Judicial district (subscriber's address or our registered agent)
    pub judicial_district: String,
    
    /// Accept service of process from complainant
    pub accept_service: bool,
}
```

---

## AI-Assisted Detection

### Proactive Copyright Scanning

```rust
// crates/legal/src/dmca/ai_detection.rs

use crate::moderation::ModerationApiClient;

/// AI-assisted copyright detection for uploaded assets
pub struct CopyrightDetector {
    /// Perceptual hash database for known copyrighted works
    hash_db: HashDatabase,
    
    /// Audio fingerprint database (for music)
    audio_fingerprints: AudioFingerprintDb,
    
    /// Moderation API for image/model analysis
    moderation_api: ModerationApiClient,
}

impl CopyrightDetector {
    /// Scan asset before publishing
    pub async fn scan_asset(&self, asset: &Asset) -> Result<CopyrightScanResult, ScanError> {
        let mut flags = vec![];
        
        match asset.asset_type {
            AssetType::Image | AssetType::Texture => {
                // Perceptual hash comparison
                let phash = self.compute_phash(&asset.data)?;
                if let Some(match_info) = self.hash_db.find_similar(phash, 0.95).await? {
                    flags.push(CopyrightFlag::PerceptualHashMatch {
                        similarity: match_info.similarity,
                        known_work: match_info.work_id,
                    });
                }
                
                // AI watermark detection
                let watermark_result = self.moderation_api.detect_watermarks(&asset.data).await?;
                if watermark_result.has_watermark {
                    flags.push(CopyrightFlag::WatermarkDetected {
                        confidence: watermark_result.confidence,
                        watermark_type: watermark_result.watermark_type,
                    });
                }
            }
            
            AssetType::Audio => {
                // Audio fingerprinting (like Shazam)
                let fingerprint = self.compute_audio_fingerprint(&asset.data)?;
                if let Some(match_info) = self.audio_fingerprints.find_match(fingerprint).await? {
                    flags.push(CopyrightFlag::AudioFingerprintMatch {
                        confidence: match_info.confidence,
                        known_work: match_info.work_id,
                        matched_duration: match_info.duration,
                    });
                }
            }
            
            AssetType::Model3D => {
                // 3D model similarity (mesh hash + texture analysis)
                let mesh_hash = self.compute_mesh_hash(&asset.data)?;
                if let Some(match_info) = self.hash_db.find_similar_mesh(mesh_hash, 0.90).await? {
                    flags.push(CopyrightFlag::MeshSimilarityMatch {
                        similarity: match_info.similarity,
                        known_work: match_info.work_id,
                    });
                }
            }
            
            _ => {}
        }
        
        Ok(CopyrightScanResult {
            asset_id: asset.id.clone(),
            flags,
            recommendation: self.calculate_recommendation(&flags),
            scanned_at: Utc::now(),
        })
    }
    
    fn calculate_recommendation(&self, flags: &[CopyrightFlag]) -> ScanRecommendation {
        if flags.is_empty() {
            return ScanRecommendation::Allow;
        }
        
        // High confidence match = block
        let max_confidence = flags.iter()
            .map(|f| f.confidence())
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0);
        
        if max_confidence > 0.95 {
            ScanRecommendation::Block { reason: "High-confidence copyright match".into() }
        } else if max_confidence > 0.80 {
            ScanRecommendation::HumanReview { priority: Priority::High }
        } else if max_confidence > 0.60 {
            ScanRecommendation::HumanReview { priority: Priority::Normal }
        } else {
            ScanRecommendation::AllowWithFlag
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CopyrightFlag {
    PerceptualHashMatch { similarity: f32, known_work: String },
    WatermarkDetected { confidence: f32, watermark_type: String },
    AudioFingerprintMatch { confidence: f32, known_work: String, matched_duration: f32 },
    MeshSimilarityMatch { similarity: f32, known_work: String },
}

impl CopyrightFlag {
    pub fn confidence(&self) -> f32 {
        match self {
            Self::PerceptualHashMatch { similarity, .. } => *similarity,
            Self::WatermarkDetected { confidence, .. } => *confidence,
            Self::AudioFingerprintMatch { confidence, .. } => *confidence,
            Self::MeshSimilarityMatch { similarity, .. } => *similarity,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScanRecommendation {
    Allow,
    AllowWithFlag,
    HumanReview { priority: Priority },
    Block { reason: String },
}
```

### Known Works Database

```rust
// crates/legal/src/dmca/known_works.rs

/// Database of known copyrighted works for proactive detection
pub struct KnownWorksDatabase {
    /// Perceptual hashes of known images/textures
    image_hashes: sled::Db,
    
    /// Audio fingerprints of known music
    audio_fingerprints: sled::Db,
    
    /// Mesh signatures of known 3D models
    mesh_signatures: sled::Db,
    
    /// Metadata about known works
    metadata: sqlx::PgPool,
}

impl KnownWorksDatabase {
    /// Add a known copyrighted work (from DMCA notice or rights holder submission)
    pub async fn add_known_work(&self, work: KnownWork) -> Result<String, DbError> {
        let work_id = uuid::Uuid::new_v4().to_string();
        
        // Store metadata
        sqlx::query!(
            r#"
            INSERT INTO known_works (id, title, rights_holder, work_type, added_at, source)
            VALUES ($1, $2, $3, $4, NOW(), $5)
            "#,
            work_id,
            work.title,
            work.rights_holder,
            work.work_type.to_string(),
            work.source.to_string(),
        )
        .execute(&self.metadata)
        .await?;
        
        // Store hashes/fingerprints
        match work.signature {
            WorkSignature::ImageHash(hash) => {
                self.image_hashes.insert(hash.as_bytes(), work_id.as_bytes())?;
            }
            WorkSignature::AudioFingerprint(fp) => {
                self.audio_fingerprints.insert(&fp, work_id.as_bytes())?;
            }
            WorkSignature::MeshSignature(sig) => {
                self.mesh_signatures.insert(&sig, work_id.as_bytes())?;
            }
        }
        
        Ok(work_id)
    }
}

#[derive(Debug, Clone)]
pub struct KnownWork {
    pub title: String,
    pub rights_holder: String,
    pub work_type: WorkType,
    pub signature: WorkSignature,
    pub source: WorkSource,
}

#[derive(Debug, Clone)]
pub enum WorkSignature {
    ImageHash(String),
    AudioFingerprint(Vec<u8>),
    MeshSignature(Vec<u8>),
}

#[derive(Debug, Clone)]
pub enum WorkSource {
    DmcaNotice { notice_id: String },
    RightsHolderSubmission { submission_id: String },
    PublicDatabase { database_name: String },
}
```

---

## Repeat Infringer Policy

### Three-Strike System

```rust
// crates/legal/src/dmca/repeat_infringer.rs

/// Repeat infringer tracking and enforcement
pub struct RepeatInfringerPolicy {
    db: PgPool,
    notification_service: NotificationService,
}

impl RepeatInfringerPolicy {
    /// Record a strike against a user
    pub async fn record_strike(&self, user_id: &str, notice_id: &str) -> Result<StrikeResult, PolicyError> {
        // Get current strike count
        let current_strikes = self.get_strike_count(user_id).await?;
        
        // Record new strike
        sqlx::query!(
            r#"
            INSERT INTO dmca_strikes (user_id, notice_id, strike_number, created_at)
            VALUES ($1, $2, $3, NOW())
            "#,
            user_id,
            notice_id,
            current_strikes + 1,
        )
        .execute(&self.db)
        .await?;
        
        let new_count = current_strikes + 1;
        
        // Determine action based on strike count
        let action = match new_count {
            1 => StrikeAction::Warning {
                message: "First DMCA strike. Please review our copyright policy.".into(),
            },
            2 => StrikeAction::Restriction {
                message: "Second DMCA strike. Upload privileges restricted for 30 days.".into(),
                restriction: Restriction::UploadCooldown { days: 30 },
            },
            3 => StrikeAction::Termination {
                message: "Third DMCA strike. Account terminated per repeat infringer policy.".into(),
                appeal_available: true,
            },
            _ => StrikeAction::Terminated,
        };
        
        // Apply action
        self.apply_action(user_id, &action).await?;
        
        // Notify user
        self.notification_service.send_strike_notification(user_id, new_count, &action).await?;
        
        Ok(StrikeResult {
            user_id: user_id.to_string(),
            strike_count: new_count,
            action,
        })
    }
    
    /// Check if counter-notification was successful (removes strike)
    pub async fn remove_strike(&self, user_id: &str, notice_id: &str) -> Result<(), PolicyError> {
        sqlx::query!(
            "DELETE FROM dmca_strikes WHERE user_id = $1 AND notice_id = $2",
            user_id,
            notice_id,
        )
        .execute(&self.db)
        .await?;
        
        // Recalculate restrictions
        self.recalculate_restrictions(user_id).await?;
        
        Ok(())
    }
    
    /// Strike decay (strikes older than 12 months don't count toward termination)
    pub async fn apply_strike_decay(&self) -> Result<u64, PolicyError> {
        let result = sqlx::query!(
            r#"
            UPDATE dmca_strikes
            SET decayed = true
            WHERE created_at < NOW() - INTERVAL '12 months'
            AND decayed = false
            "#
        )
        .execute(&self.db)
        .await?;
        
        Ok(result.rows_affected())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StrikeAction {
    Warning { message: String },
    Restriction { message: String, restriction: Restriction },
    Termination { message: String, appeal_available: bool },
    Terminated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Restriction {
    UploadCooldown { days: u32 },
    ManualReviewRequired,
    MonetizationDisabled,
}
```

---

## Integration with Moderation API

### DMCA Endpoints

```rust
// crates/api/src/routes/dmca.rs

use axum::{extract::State, Json, Router};
use axum::routing::{get, post};

pub fn dmca_routes() -> Router<AppState> {
    Router::new()
        .route("/takedown", post(submit_takedown))
        .route("/takedown/:id", get(get_takedown_status))
        .route("/counter", post(submit_counter_notification))
        .route("/counter/:id", get(get_counter_status))
        .route("/scan", post(scan_asset))
}

/// POST /dmca/takedown - Submit DMCA takedown notice
pub async fn submit_takedown(
    State(state): State<AppState>,
    Json(request): Json<TakedownRequest>,
) -> Result<Json<ApiResponse<TakedownResult>>, ApiError> {
    // Validate and process takedown
    let notice = TakedownNotice::from_request(request)?;
    let result = state.dmca_service.process_takedown(notice).await?;
    
    Ok(Json(ApiResponse::success(result)))
}

/// POST /dmca/counter - Submit counter-notification
pub async fn submit_counter_notification(
    State(state): State<AppState>,
    claims: Claims,
    Json(request): Json<CounterRequest>,
) -> Result<Json<ApiResponse<CounterResult>>, ApiError> {
    // Verify user owns the removed content
    let counter = CounterNotification::from_request(request, &claims.sub)?;
    let result = state.dmca_service.process_counter_notification(counter).await?;
    
    Ok(Json(ApiResponse::success(result)))
}

/// POST /dmca/scan - Scan asset for copyright issues (internal)
pub async fn scan_asset(
    State(state): State<AppState>,
    Json(request): Json<ScanRequest>,
) -> Result<Json<ApiResponse<CopyrightScanResult>>, ApiError> {
    let asset = state.asset_service.get_asset(&request.asset_id).await?;
    let result = state.copyright_detector.scan_asset(&asset).await?;
    
    Ok(Json(ApiResponse::success(result)))
}
```

---

## Summary

### DMCA Compliance Checklist

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| Designated Agent | ⏳ Pending | Register before launch |
| Website Notice | ✅ Designed | `/legal/dmca` page |
| Takedown Process | ✅ Implemented | `DmcaService::process_takedown` |
| Counter-Notification | ✅ Implemented | `DmcaService::process_counter_notification` |
| Repeat Infringer Policy | ✅ Implemented | 3-strike system with decay |
| AI Pre-Scanning | ✅ Implemented | `CopyrightDetector::scan_asset` |
| 24-Hour SLA | ✅ Designed | Automated processing |

### Key Metrics

| Metric | Target |
|--------|--------|
| Takedown Response Time | < 24 hours |
| Counter-Notification Processing | < 48 hours |
| AI Scan Accuracy | > 90% |
| False Positive Rate | < 5% |

---

## Related Documentation

- [CSAM.md](./CSAM.md) — Child safety content policies
- [AI_AGENTS.md](../moderation/AI_AGENTS.md) — AI moderation architecture
- [MODERATION_API.md](../moderation/MODERATION_API.md) — Moderation API reference
- [asset_hosting.md](../assets/asset_hosting.md) — Asset hosting infrastructure

# CSAM Prevention & Reporting Documentation

**Child Sexual Abuse Material Prevention for Eustress Engine**

> *Best Match Dynamic: Zero Tolerance → PhotoDNA + NCMEC reporting, instant removal, law enforcement cooperation*

**Last Updated:** December 04, 2025  
**Status:** Pre-Release Compliance Framework  
**Applies To:** All user-generated content, assets, avatars, and communications

---

## Table of Contents

1. [Overview](#overview)
2. [Legal Requirements](#legal-requirements)
3. [Detection Systems](#detection-systems)
4. [Reporting Obligations](#reporting-obligations)
5. [Content Removal](#content-removal)
6. [Evidence Preservation](#evidence-preservation)
7. [Staff Training](#staff-training)
8. [Rust Implementation](#rust-implementation)
9. [Integration with Moderation Pipeline](#integration-with-moderation-pipeline)

---

## Overview

### Zero Tolerance Policy

```
Dynamic: Child Safety + UGC Platform → Zero Tolerance
Implication: PhotoDNA hashing, NCMEC CyberTipline, instant removal, no appeals
Benefit: Legal compliance, child protection, platform integrity
```

**Mantra:** "Protect Every Child" — No exceptions, no delays, no tolerance.

### Legal Framework

| Law | Jurisdiction | Key Requirements |
|-----|--------------|------------------|
| **18 U.S.C. § 2258A** | USA | Mandatory NCMEC reporting within 24 hours |
| **PROTECT Act** | USA | Virtual/AI-generated CSAM is illegal |
| **FOSTA-SESTA** | USA | Platform liability for facilitation |
| **EU Directive 2011/93** | EU | Detection, reporting, removal obligations |
| **UK Online Safety Act** | UK | Proactive detection requirements |
| **Australia Online Safety Act** | AU | eSafety Commissioner reporting |

### Scope of Prohibited Content

This policy covers ALL forms of child sexual abuse material:

1. **Photographic CSAM** — Real images/videos of abuse
2. **Virtual CSAM** — AI-generated, CGI, or drawn depictions
3. **Grooming Content** — Material designed to normalize abuse
4. **Sexualized Avatars** — Child-like avatars in sexual contexts
5. **Exploitative Text** — Descriptions of child abuse
6. **Enticement** — Communications soliciting minors

---

## Legal Requirements

### 18 U.S.C. § 2258A - Mandatory Reporting

```rust
// crates/legal/src/csam/reporting.rs

/// NCMEC CyberTipline reporting requirements
/// Per 18 U.S.C. § 2258A, electronic service providers MUST report
/// apparent violations within specific timeframes.

#[derive(Debug, Clone)]
pub struct ReportingRequirements {
    /// Must report to NCMEC within 24 hours of obtaining actual knowledge
    pub reporting_deadline_hours: u32,  // 24
    
    /// Must preserve evidence for 90 days (or longer if requested)
    pub preservation_period_days: u32,  // 90
    
    /// Must not notify the user (could compromise investigation)
    pub user_notification_prohibited: bool,  // true
    
    /// Must not destroy evidence
    pub evidence_destruction_prohibited: bool,  // true
    
    /// Failure to report: Up to $150,000 per violation (first offense)
    /// Up to $300,000 per violation (subsequent offenses)
    pub penalty_first_offense: u32,  // 150_000
    pub penalty_subsequent: u32,     // 300_000
}

impl Default for ReportingRequirements {
    fn default() -> Self {
        Self {
            reporting_deadline_hours: 24,
            preservation_period_days: 90,
            user_notification_prohibited: true,
            evidence_destruction_prohibited: true,
            penalty_first_offense: 150_000,
            penalty_subsequent: 300_000,
        }
    }
}
```

### NCMEC CyberTipline Integration

```rust
// crates/legal/src/csam/ncmec.rs

use reqwest::Client;
use serde::{Deserialize, Serialize};

/// NCMEC CyberTipline API client
/// API Documentation: https://report.cybertip.org/ispws/documentation
pub struct NcmecClient {
    client: Client,
    api_endpoint: String,
    username: String,
    password: String,
    esp_id: String,  // Electronic Service Provider ID
}

impl NcmecClient {
    pub fn new(config: NcmecConfig) -> Self {
        Self {
            client: Client::new(),
            api_endpoint: "https://report.cybertip.org/ispws".to_string(),
            username: config.username,
            password: config.password,
            esp_id: config.esp_id,
        }
    }
    
    /// Submit a CyberTipline report
    pub async fn submit_report(&self, report: CyberTipReport) -> Result<ReportResponse, NcmecError> {
        // Build XML report per NCMEC schema
        let xml = self.build_report_xml(&report)?;
        
        let response = self.client
            .post(&format!("{}/submit", self.api_endpoint))
            .basic_auth(&self.username, Some(&self.password))
            .header("Content-Type", "application/xml")
            .body(xml)
            .send()
            .await?;
        
        if response.status().is_success() {
            let report_id = response.text().await?;
            Ok(ReportResponse {
                report_id,
                submitted_at: chrono::Utc::now(),
                status: ReportStatus::Submitted,
            })
        } else {
            Err(NcmecError::SubmissionFailed(response.status().to_string()))
        }
    }
    
    /// Upload file evidence to an existing report
    pub async fn upload_file(
        &self,
        report_id: &str,
        file_data: &[u8],
        file_info: FileInfo,
    ) -> Result<FileUploadResponse, NcmecError> {
        let form = reqwest::multipart::Form::new()
            .text("reportId", report_id.to_string())
            .text("fileName", file_info.filename)
            .text("fileType", file_info.mime_type)
            .part("file", reqwest::multipart::Part::bytes(file_data.to_vec()));
        
        let response = self.client
            .post(&format!("{}/upload", self.api_endpoint))
            .basic_auth(&self.username, Some(&self.password))
            .multipart(form)
            .send()
            .await?;
        
        // Parse response...
        Ok(FileUploadResponse {
            file_id: response.text().await?,
            uploaded_at: chrono::Utc::now(),
        })
    }
    
    /// Finish and submit the report
    pub async fn finish_report(&self, report_id: &str) -> Result<(), NcmecError> {
        self.client
            .post(&format!("{}/finish", self.api_endpoint))
            .basic_auth(&self.username, Some(&self.password))
            .query(&[("id", report_id)])
            .send()
            .await?;
        
        Ok(())
    }
}

/// CyberTipline report structure
#[derive(Debug, Clone, Serialize)]
pub struct CyberTipReport {
    /// Incident information
    pub incident: IncidentInfo,
    
    /// Reported user information
    pub reported_user: ReportedUser,
    
    /// Uploaded content information
    pub uploaded_content: Vec<UploadedContent>,
    
    /// Reporter information (if user-reported)
    pub reporter: Option<ReporterInfo>,
    
    /// Additional information
    pub additional_info: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IncidentInfo {
    /// Type of incident
    pub incident_type: IncidentType,
    
    /// Date/time incident was discovered
    pub incident_datetime: chrono::DateTime<chrono::Utc>,
    
    /// How the incident was discovered
    pub discovery_method: DiscoveryMethod,
    
    /// URL where content was found
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub enum IncidentType {
    ChildPornography,
    ChildSexTrafficking,
    ChildSexTourism,
    ChildSexMolestation,
    UnsolicitorObsceneMaterial,
    MisleadingDomainName,
    MisleadingWords,
    OnlineEnticement,
}

#[derive(Debug, Clone, Serialize)]
pub enum DiscoveryMethod {
    /// Detected by automated hash matching
    AutomatedHashMatch,
    
    /// Detected by AI classifier
    AiClassifier,
    
    /// Reported by user
    UserReport,
    
    /// Discovered by human moderator
    HumanModerator,
    
    /// Proactive scanning
    ProactiveScan,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportedUser {
    /// Our internal user ID
    pub internal_user_id: String,
    
    /// Username/display name
    pub username: Option<String>,
    
    /// Email address (if known)
    pub email: Option<String>,
    
    /// IP addresses associated with the activity
    pub ip_addresses: Vec<IpAddressInfo>,
    
    /// Account creation date
    pub account_created: Option<chrono::DateTime<chrono::Utc>>,
    
    /// Steam ID (if applicable)
    pub steam_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IpAddressInfo {
    pub ip_address: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub activity_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UploadedContent {
    /// Our internal asset/content ID
    pub content_id: String,
    
    /// Content hash (for deduplication)
    pub content_hash: String,
    
    /// PhotoDNA hash (if image)
    pub photodna_hash: Option<String>,
    
    /// File type
    pub file_type: String,
    
    /// Upload timestamp
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
    
    /// File size in bytes
    pub file_size: u64,
}
```

---

## Detection Systems

### Multi-Layer Detection Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                      CSAM DETECTION PIPELINE                             │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  LAYER 1: HASH MATCHING (Instant)                                        │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                      │
│  │ PhotoDNA    │  │ MD5/SHA256  │  │ pHash       │                      │
│  │ (Microsoft) │  │ (NCMEC DB)  │  │ (Perceptual)│                      │
│  └─────────────┘  └─────────────┘  └─────────────┘                      │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  LAYER 2: AI CLASSIFICATION (< 100ms)                                    │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                      │
│  │ Image       │  │ Video       │  │ Text        │                      │
│  │ Classifier  │  │ Analyzer    │  │ Classifier  │                      │
│  │ (Candle)    │  │ (Frames)    │  │ (NLP)       │                      │
│  └─────────────┘  └─────────────┘  └─────────────┘                      │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  LAYER 3: BEHAVIORAL ANALYSIS                                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                      │
│  │ Grooming    │  │ Age Gap     │  │ Pattern     │                      │
│  │ Detection   │  │ Analysis    │  │ Recognition │                      │
│  └─────────────┘  └─────────────┘  └─────────────┘                      │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  LAYER 4: HUMAN REVIEW (Edge Cases Only)                                 │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  Trained specialists with law enforcement background             │    │
│  │  Never view actual CSAM - only metadata and AI classifications   │    │
│  └─────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
```

### PhotoDNA Integration

```rust
// crates/legal/src/csam/photodna.rs

/// PhotoDNA hash matching for known CSAM
/// Microsoft PhotoDNA creates robust hashes that survive:
/// - Resizing
/// - Color changes
/// - Minor cropping
/// - Format conversion
pub struct PhotoDnaService {
    /// PhotoDNA Cloud API client
    api_client: PhotoDnaApiClient,
    
    /// Local hash cache for faster matching
    hash_cache: HashCache,
    
    /// NCMEC hash list (updated regularly)
    ncmec_hashes: NcmecHashList,
}

impl PhotoDnaService {
    /// Compute PhotoDNA hash for an image
    pub async fn compute_hash(&self, image_data: &[u8]) -> Result<PhotoDnaHash, HashError> {
        // Use PhotoDNA Cloud API
        let response = self.api_client.compute_hash(image_data).await?;
        Ok(PhotoDnaHash(response.hash))
    }
    
    /// Check image against known CSAM database
    pub async fn check_image(&self, image_data: &[u8]) -> Result<PhotoDnaResult, HashError> {
        // 1. Compute hash
        let hash = self.compute_hash(image_data).await?;
        
        // 2. Check local cache first (faster)
        if let Some(match_info) = self.hash_cache.check(&hash).await? {
            return Ok(PhotoDnaResult::Match(match_info));
        }
        
        // 3. Check NCMEC hash list
        if let Some(match_info) = self.ncmec_hashes.check(&hash).await? {
            // Cache the match for future
            self.hash_cache.add(&hash, &match_info).await?;
            return Ok(PhotoDnaResult::Match(match_info));
        }
        
        // 4. Check PhotoDNA Cloud (most comprehensive)
        let cloud_result = self.api_client.match_hash(&hash).await?;
        
        if cloud_result.is_match {
            Ok(PhotoDnaResult::Match(MatchInfo {
                hash: hash.clone(),
                confidence: cloud_result.confidence,
                source: MatchSource::PhotoDnaCloud,
            }))
        } else {
            Ok(PhotoDnaResult::NoMatch)
        }
    }
}

#[derive(Debug, Clone)]
pub struct PhotoDnaHash(pub Vec<u8>);

#[derive(Debug, Clone)]
pub enum PhotoDnaResult {
    Match(MatchInfo),
    NoMatch,
}

#[derive(Debug, Clone)]
pub struct MatchInfo {
    pub hash: PhotoDnaHash,
    pub confidence: f32,
    pub source: MatchSource,
}

#[derive(Debug, Clone)]
pub enum MatchSource {
    LocalCache,
    NcmecHashList,
    PhotoDnaCloud,
}
```

### AI Classifier for Unknown Content

```rust
// crates/legal/src/csam/classifier.rs

use crate::moderation::ModerationApiClient;

/// AI-based CSAM classifier for content not in hash databases
/// Uses multiple signals to detect potential CSAM
pub struct CsamClassifier {
    /// Image classification model (Candle)
    image_model: ImageClassificationModel,
    
    /// Age estimation model
    age_estimator: AgeEstimationModel,
    
    /// Nudity detection model
    nudity_detector: NudityDetectionModel,
    
    /// Context analyzer
    context_analyzer: ContextAnalyzer,
}

impl CsamClassifier {
    /// Classify an image for potential CSAM
    /// Returns classification with confidence score
    pub async fn classify_image(&self, image_data: &[u8]) -> Result<CsamClassification, ClassifierError> {
        // 1. Detect if image contains a person
        let person_detection = self.image_model.detect_persons(image_data).await?;
        
        if person_detection.persons.is_empty() {
            return Ok(CsamClassification::Safe { confidence: 0.99 });
        }
        
        // 2. Estimate age of detected persons
        let age_estimates = self.age_estimator.estimate_ages(&person_detection.persons).await?;
        
        let has_minor = age_estimates.iter().any(|e| e.estimated_age < 18 && e.confidence > 0.7);
        
        if !has_minor {
            return Ok(CsamClassification::Safe { confidence: 0.95 });
        }
        
        // 3. Check for nudity/sexual content
        let nudity_result = self.nudity_detector.detect(image_data).await?;
        
        if !nudity_result.has_nudity && !nudity_result.has_sexual_content {
            return Ok(CsamClassification::Safe { confidence: 0.90 });
        }
        
        // 4. If minor + nudity/sexual content = CSAM
        if has_minor && (nudity_result.has_nudity || nudity_result.has_sexual_content) {
            let confidence = self.calculate_confidence(&age_estimates, &nudity_result);
            
            return Ok(CsamClassification::PotentialCsam {
                confidence,
                age_estimates,
                nudity_score: nudity_result.score,
                requires_immediate_action: confidence > 0.8,
            });
        }
        
        // 5. Edge case - needs human review
        Ok(CsamClassification::RequiresReview {
            reason: "Ambiguous age/content combination".into(),
            priority: ReviewPriority::High,
        })
    }
    
    /// Classify text for grooming/enticement patterns
    pub async fn classify_text(&self, text: &str, context: &TextContext) -> Result<TextClassification, ClassifierError> {
        // Check for grooming patterns
        let grooming_score = self.context_analyzer.detect_grooming(text, context).await?;
        
        // Check for explicit content involving minors
        let explicit_score = self.context_analyzer.detect_explicit_minor_content(text).await?;
        
        // Check for enticement patterns
        let enticement_score = self.context_analyzer.detect_enticement(text, context).await?;
        
        let max_score = grooming_score.max(explicit_score).max(enticement_score);
        
        if max_score > 0.9 {
            Ok(TextClassification::Violation {
                violation_type: self.determine_violation_type(grooming_score, explicit_score, enticement_score),
                confidence: max_score,
                requires_immediate_action: true,
            })
        } else if max_score > 0.6 {
            Ok(TextClassification::RequiresReview {
                scores: TextScores { grooming_score, explicit_score, enticement_score },
                priority: ReviewPriority::High,
            })
        } else {
            Ok(TextClassification::Safe { confidence: 1.0 - max_score })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CsamClassification {
    Safe { confidence: f32 },
    PotentialCsam {
        confidence: f32,
        age_estimates: Vec<AgeEstimate>,
        nudity_score: f32,
        requires_immediate_action: bool,
    },
    RequiresReview {
        reason: String,
        priority: ReviewPriority,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgeEstimate {
    pub person_id: u32,
    pub estimated_age: u8,
    pub confidence: f32,
    pub age_range: (u8, u8),  // (min, max)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ReviewPriority {
    Critical,  // < 1 hour
    High,      // < 4 hours
    Normal,    // < 24 hours
}
```

### Avatar/3D Model Analysis

```rust
// crates/legal/src/csam/avatar_analysis.rs

/// Analyze 3D avatars for child-like characteristics in sexual contexts
pub struct AvatarAnalyzer {
    /// Body proportion analyzer
    proportion_analyzer: ProportionAnalyzer,
    
    /// Clothing/nudity detector
    clothing_detector: ClothingDetector,
    
    /// Pose analyzer
    pose_analyzer: PoseAnalyzer,
}

impl AvatarAnalyzer {
    /// Analyze avatar for potential policy violations
    pub async fn analyze_avatar(&self, avatar: &AvatarData) -> Result<AvatarAnalysis, AnalysisError> {
        // 1. Analyze body proportions (child-like vs adult)
        let proportions = self.proportion_analyzer.analyze(&avatar.mesh).await?;
        
        let is_child_like = proportions.head_to_body_ratio > 0.25  // Larger head ratio = child-like
            || proportions.limb_proportions < 0.7                   // Shorter limbs = child-like
            || proportions.estimated_height_percentile < 0.3;       // Short stature
        
        // 2. Check clothing/nudity
        let clothing = self.clothing_detector.analyze(&avatar.mesh, &avatar.textures).await?;
        
        let is_sexualized = clothing.nudity_level > NudityLevel::Partial
            || clothing.sexual_clothing;
        
        // 3. Check pose
        let pose = self.pose_analyzer.analyze(&avatar.skeleton).await?;
        
        let sexual_pose = pose.is_sexual_pose;
        
        // 4. Combine signals
        if is_child_like && (is_sexualized || sexual_pose) {
            return Ok(AvatarAnalysis::Violation {
                violation_type: AvatarViolationType::SexualizedMinorAvatar,
                confidence: self.calculate_confidence(&proportions, &clothing, &pose),
                details: AvatarViolationDetails {
                    child_like_score: proportions.child_like_score,
                    sexualization_score: clothing.sexualization_score,
                    pose_score: pose.sexual_score,
                },
            });
        }
        
        if is_child_like && clothing.nudity_level > NudityLevel::None {
            return Ok(AvatarAnalysis::RequiresReview {
                reason: "Child-like avatar with partial nudity".into(),
                priority: ReviewPriority::High,
            });
        }
        
        Ok(AvatarAnalysis::Safe)
    }
}

#[derive(Debug, Clone)]
pub enum AvatarAnalysis {
    Safe,
    RequiresReview { reason: String, priority: ReviewPriority },
    Violation {
        violation_type: AvatarViolationType,
        confidence: f32,
        details: AvatarViolationDetails,
    },
}

#[derive(Debug, Clone)]
pub enum AvatarViolationType {
    SexualizedMinorAvatar,
    ExplicitMinorAvatar,
    GroomingRelatedAvatar,
}
```

---

## Content Removal

### Immediate Removal Process

```rust
// crates/legal/src/csam/removal.rs

/// CSAM content removal service
/// Content is removed IMMEDIATELY upon detection - no appeals
pub struct CsamRemovalService {
    asset_service: AssetService,
    user_service: UserService,
    evidence_service: EvidencePreservationService,
    ncmec_client: NcmecClient,
    notification_service: InternalNotificationService,
}

impl CsamRemovalService {
    /// Handle detected CSAM - IMMEDIATE action required
    pub async fn handle_csam_detection(
        &self,
        detection: CsamDetection,
    ) -> Result<CsamResponse, CsamError> {
        let start = std::time::Instant::now();
        
        // 1. IMMEDIATELY remove content from all locations
        self.remove_content_immediately(&detection).await?;
        
        // 2. Preserve evidence (required by law)
        let evidence_id = self.evidence_service.preserve(&detection).await?;
        
        // 3. Suspend user account immediately
        self.user_service.suspend_account(
            &detection.user_id,
            SuspensionReason::CsamViolation,
            SuspensionDuration::Permanent,
        ).await?;
        
        // 4. Block user's IP addresses
        self.user_service.block_ip_addresses(&detection.user_id).await?;
        
        // 5. Submit NCMEC report (within 24 hours - we do it immediately)
        let ncmec_report = self.build_ncmec_report(&detection, &evidence_id).await?;
        let report_response = self.ncmec_client.submit_report(ncmec_report).await?;
        
        // 6. Upload evidence files to NCMEC
        for content in &detection.content {
            let file_data = self.evidence_service.get_preserved_content(&evidence_id, &content.id).await?;
            self.ncmec_client.upload_file(
                &report_response.report_id,
                &file_data,
                FileInfo {
                    filename: content.filename.clone(),
                    mime_type: content.mime_type.clone(),
                },
            ).await?;
        }
        
        // 7. Finalize NCMEC report
        self.ncmec_client.finish_report(&report_response.report_id).await?;
        
        // 8. Notify internal security team (NOT the user)
        self.notification_service.notify_security_team(SecurityAlert {
            alert_type: AlertType::CsamDetected,
            detection_id: detection.id.clone(),
            ncmec_report_id: report_response.report_id.clone(),
            processing_time_ms: start.elapsed().as_millis() as u64,
        }).await?;
        
        // 9. Log for compliance (encrypted, access-controlled)
        self.log_csam_action(&detection, &report_response).await?;
        
        Ok(CsamResponse {
            detection_id: detection.id,
            ncmec_report_id: report_response.report_id,
            content_removed: true,
            user_suspended: true,
            evidence_preserved: true,
            processing_time_ms: start.elapsed().as_millis() as u64,
        })
    }
    
    /// Remove content from ALL locations immediately
    async fn remove_content_immediately(&self, detection: &CsamDetection) -> Result<(), CsamError> {
        for content in &detection.content {
            // Remove from primary storage
            self.asset_service.hard_delete(&content.id).await?;
            
            // Remove from all CDN edge caches
            self.asset_service.purge_cdn_cache(&content.id).await?;
            
            // Remove from search indexes
            self.asset_service.remove_from_search(&content.id).await?;
            
            // Remove any thumbnails/previews
            self.asset_service.remove_derivatives(&content.id).await?;
            
            // Remove from any user collections/favorites
            self.asset_service.remove_from_collections(&content.id).await?;
        }
        
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CsamDetection {
    pub id: String,
    pub detection_method: DetectionMethod,
    pub user_id: String,
    pub content: Vec<DetectedContent>,
    pub detected_at: chrono::DateTime<chrono::Utc>,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub enum DetectionMethod {
    PhotoDnaMatch,
    NcmecHashMatch,
    AiClassifier,
    UserReport,
    ProactiveScan,
}

#[derive(Debug, Clone)]
pub struct DetectedContent {
    pub id: String,
    pub content_type: ContentType,
    pub filename: String,
    pub mime_type: String,
    pub hash: String,
    pub photodna_hash: Option<String>,
    pub size_bytes: u64,
}
```

---

## Evidence Preservation

### 90-Day Preservation Requirement

```rust
// crates/legal/src/csam/evidence.rs

use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::Aead;

/// Evidence preservation service
/// Per 18 U.S.C. § 2258A, must preserve for at least 90 days
pub struct EvidencePreservationService {
    /// Encrypted evidence storage (separate from main storage)
    evidence_storage: EncryptedStorage,
    
    /// Database for evidence metadata
    db: sqlx::PgPool,
    
    /// Encryption key (HSM-backed in production)
    encryption_key: Key<Aes256Gcm>,
}

impl EvidencePreservationService {
    /// Preserve evidence for NCMEC reporting
    pub async fn preserve(&self, detection: &CsamDetection) -> Result<String, EvidenceError> {
        let evidence_id = uuid::Uuid::new_v4().to_string();
        
        // Calculate preservation expiry (90 days minimum)
        let expires_at = chrono::Utc::now() + chrono::Duration::days(90);
        
        // Create evidence record
        sqlx::query!(
            r#"
            INSERT INTO csam_evidence (
                id, detection_id, user_id, detected_at, 
                preserved_at, expires_at, ncmec_report_id, status
            )
            VALUES ($1, $2, $3, $4, NOW(), $5, NULL, 'preserved')
            "#,
            evidence_id,
            detection.id,
            detection.user_id,
            detection.detected_at,
            expires_at,
        )
        .execute(&self.db)
        .await?;
        
        // Preserve each piece of content
        for content in &detection.content {
            self.preserve_content(&evidence_id, content).await?;
        }
        
        // Preserve user information
        self.preserve_user_info(&evidence_id, &detection.user_id).await?;
        
        // Preserve IP addresses and activity logs
        self.preserve_activity_logs(&evidence_id, &detection.user_id).await?;
        
        Ok(evidence_id)
    }
    
    /// Preserve content with encryption
    async fn preserve_content(
        &self,
        evidence_id: &str,
        content: &DetectedContent,
    ) -> Result<(), EvidenceError> {
        // Get original content data
        let data = self.get_original_content(&content.id).await?;
        
        // Encrypt content
        let encrypted = self.encrypt(&data)?;
        
        // Store in evidence storage
        let storage_key = format!("{}/{}", evidence_id, content.id);
        self.evidence_storage.store(&storage_key, &encrypted).await?;
        
        // Record metadata
        sqlx::query!(
            r#"
            INSERT INTO csam_evidence_files (
                evidence_id, content_id, original_hash, 
                photodna_hash, file_type, size_bytes, storage_key
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            evidence_id,
            content.id,
            content.hash,
            content.photodna_hash,
            content.mime_type,
            content.size_bytes as i64,
            storage_key,
        )
        .execute(&self.db)
        .await?;
        
        Ok(())
    }
    
    /// Preserve user information at time of detection
    async fn preserve_user_info(&self, evidence_id: &str, user_id: &str) -> Result<(), EvidenceError> {
        // Get user record
        let user = sqlx::query!(
            "SELECT * FROM users WHERE id = $1",
            user_id
        )
        .fetch_one(&self.db)
        .await?;
        
        // Get associated IP addresses
        let ips = sqlx::query!(
            "SELECT ip_address, first_seen, last_seen FROM user_ip_addresses WHERE user_id = $1",
            user_id
        )
        .fetch_all(&self.db)
        .await?;
        
        // Serialize and encrypt
        let user_info = UserInfoSnapshot {
            user_id: user_id.to_string(),
            username: user.username,
            email: user.email,
            steam_id: user.steam_id,
            created_at: user.created_at,
            ip_addresses: ips.into_iter().map(|ip| IpSnapshot {
                ip_address: ip.ip_address,
                first_seen: ip.first_seen,
                last_seen: ip.last_seen,
            }).collect(),
        };
        
        let serialized = serde_json::to_vec(&user_info)?;
        let encrypted = self.encrypt(&serialized)?;
        
        let storage_key = format!("{}/user_info", evidence_id);
        self.evidence_storage.store(&storage_key, &encrypted).await?;
        
        Ok(())
    }
    
    /// Extend preservation if requested by law enforcement
    pub async fn extend_preservation(
        &self,
        evidence_id: &str,
        new_expiry: chrono::DateTime<chrono::Utc>,
        request_reference: &str,
    ) -> Result<(), EvidenceError> {
        sqlx::query!(
            r#"
            UPDATE csam_evidence 
            SET expires_at = $1, 
                law_enforcement_hold = true,
                hold_reference = $2
            WHERE id = $3
            "#,
            new_expiry,
            request_reference,
            evidence_id,
        )
        .execute(&self.db)
        .await?;
        
        Ok(())
    }
    
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>, EvidenceError> {
        let cipher = Aes256Gcm::new(&self.encryption_key);
        let nonce = Nonce::from_slice(b"unique nonce"); // Use random nonce in production
        
        cipher.encrypt(nonce, data)
            .map_err(|e| EvidenceError::EncryptionFailed(e.to_string()))
    }
}
```

---

## Staff Training

### Required Training for Moderators

```rust
// crates/legal/src/csam/training.rs

/// Staff training requirements for CSAM handling
#[derive(Debug, Clone)]
pub struct TrainingRequirements {
    /// Initial training before handling any content
    pub initial_training: TrainingModule,
    
    /// Quarterly refresher training
    pub refresher_training: TrainingModule,
    
    /// Psychological support resources
    pub wellness_resources: WellnessResources,
}

#[derive(Debug, Clone)]
pub struct TrainingModule {
    pub name: String,
    pub duration_hours: f32,
    pub topics: Vec<TrainingTopic>,
    pub certification_required: bool,
    pub renewal_period_months: u32,
}

#[derive(Debug, Clone)]
pub enum TrainingTopic {
    /// Legal requirements and reporting obligations
    LegalRequirements,
    
    /// How to identify CSAM without viewing
    IdentificationTechniques,
    
    /// Using AI tools to minimize exposure
    AiToolUsage,
    
    /// Evidence preservation procedures
    EvidencePreservation,
    
    /// NCMEC reporting procedures
    NcmecReporting,
    
    /// Psychological impact and self-care
    WellnessAndSelfCare,
    
    /// Escalation procedures
    EscalationProcedures,
}

#[derive(Debug, Clone)]
pub struct WellnessResources {
    /// Access to counseling services
    pub counseling_available: bool,
    
    /// Maximum exposure time per day
    pub max_exposure_hours: f32,
    
    /// Mandatory breaks
    pub break_frequency_minutes: u32,
    
    /// Rotation policy
    pub rotation_policy: RotationPolicy,
}

#[derive(Debug, Clone)]
pub enum RotationPolicy {
    /// Rotate off CSAM review after N months
    TimeBasedRotation { months: u32 },
    
    /// Rotate based on exposure count
    ExposureBasedRotation { max_reviews: u32 },
    
    /// Voluntary rotation
    Voluntary,
}
```

### Minimizing Human Exposure

```rust
// crates/legal/src/csam/human_review.rs

/// Human review system designed to minimize exposure to actual CSAM
pub struct HumanReviewSystem {
    /// AI pre-classification results
    ai_classifications: AiClassificationService,
    
    /// Metadata-only review interface
    review_interface: MetadataReviewInterface,
}

impl HumanReviewSystem {
    /// Create review task that minimizes exposure
    pub async fn create_review_task(&self, detection: &CsamDetection) -> Result<ReviewTask, ReviewError> {
        // Human reviewers NEVER see actual CSAM content
        // They only see:
        // 1. AI classification results
        // 2. Hash match information
        // 3. Metadata (file type, size, upload time)
        // 4. Blurred/redacted thumbnails (optional, for edge cases)
        
        let ai_result = self.ai_classifications.get(&detection.id).await?;
        
        Ok(ReviewTask {
            task_id: uuid::Uuid::new_v4().to_string(),
            detection_id: detection.id.clone(),
            
            // AI classification summary
            ai_classification: ai_result.classification,
            ai_confidence: ai_result.confidence,
            
            // Hash match info (if any)
            hash_match: detection.hash_match_info.clone(),
            
            // Metadata only
            metadata: ContentMetadata {
                file_type: detection.content[0].mime_type.clone(),
                file_size: detection.content[0].size_bytes,
                upload_time: detection.detected_at,
                user_account_age: detection.user_account_age,
            },
            
            // NO actual content
            content_preview: None,
            
            // Decision options
            options: vec![
                ReviewOption::ConfirmCsam,
                ReviewOption::NotCsam { reason: String::new() },
                ReviewOption::Escalate { reason: String::new() },
            ],
        })
    }
}
```

---

## Integration with Moderation Pipeline

### CSAM Check in Asset Upload Flow

```rust
// crates/api/src/routes/assets.rs

/// Asset upload with CSAM scanning
pub async fn upload_asset(
    State(state): State<AppState>,
    claims: Claims,
    mut multipart: Multipart,
) -> Result<Json<ApiResponse<UploadResponse>>, ApiError> {
    while let Some(field) = multipart.next_field().await? {
        let data = field.bytes().await?;
        
        // CRITICAL: CSAM check BEFORE any other processing
        let csam_result = state.csam_service.check_content(&data).await?;
        
        match csam_result {
            CsamCheckResult::Clean => {
                // Continue with normal upload flow
            }
            CsamCheckResult::Detected(detection) => {
                // IMMEDIATE action - no further processing
                state.csam_removal_service.handle_csam_detection(detection).await?;
                
                // Return generic error (don't reveal detection)
                return Err(ApiError::UploadFailed("Upload failed".into()));
            }
            CsamCheckResult::RequiresReview(review_info) => {
                // Queue for human review, don't publish yet
                state.review_queue.add(review_info).await?;
                
                return Ok(Json(ApiResponse::success(UploadResponse {
                    status: UploadStatus::PendingReview,
                    asset_id: None,
                    message: "Upload is being reviewed".into(),
                })));
            }
        }
        
        // Normal upload continues...
    }
    
    // ...
}
```

### Integration with AI Agents

```rust
// crates/agents/src/csam_agent.rs

use crate::core::{ModerationAgent, AgentCard, ModerationDecision};

/// Specialized CSAM detection agent
pub struct CsamAgent {
    photodna_service: PhotoDnaService,
    classifier: CsamClassifier,
    avatar_analyzer: AvatarAnalyzer,
    ncmec_client: NcmecClient,
}

#[async_trait]
impl ModerationAgent for CsamAgent {
    fn agent_card(&self) -> AgentCard {
        AgentCard {
            id: "csam-agent-v1".into(),
            name: "CSAM Detection Agent".into(),
            version: "1.0.0".into(),
            capabilities: vec![
                Capability::ImageAnalysis,
                Capability::VideoAnalysis,
                Capability::AvatarAnalysis,
                Capability::TextAnalysis,
            ],
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "content_type": { "type": "string" },
                    "content_data": { "type": "string", "format": "base64" }
                }
            }),
            output_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "is_csam": { "type": "boolean" },
                    "confidence": { "type": "number" },
                    "detection_method": { "type": "string" }
                }
            }),
            performance: PerformanceMetrics {
                accuracy: 0.999,  // Must be extremely high
                latency_p50_ms: 50,
                latency_p99_ms: 200,
                throughput_rps: 1000,
            },
        }
    }
    
    async fn moderate(&self, content: &Content) -> Result<ModerationDecision, AgentError> {
        match content.content_type {
            ContentType::Image => {
                // PhotoDNA first (fastest, most reliable)
                let photodna_result = self.photodna_service.check_image(&content.data).await?;
                
                if let PhotoDnaResult::Match(info) = photodna_result {
                    return Ok(ModerationDecision {
                        content_id: content.id.clone(),
                        action: ModerationAction::Remove {
                            reason: "CSAM detected (hash match)".into(),
                            severity: Severity::Critical,
                        },
                        confidence: 1.0,
                        categories: vec![ViolationCategory::Csam],
                        timestamp: chrono::Utc::now(),
                        agent_id: self.agent_card().id,
                        requires_human_review: false,  // No review needed for hash match
                    });
                }
                
                // AI classifier for unknown content
                let classification = self.classifier.classify_image(&content.data).await?;
                
                match classification {
                    CsamClassification::PotentialCsam { confidence, .. } if confidence > 0.8 => {
                        Ok(ModerationDecision {
                            content_id: content.id.clone(),
                            action: ModerationAction::Remove {
                                reason: "Potential CSAM detected".into(),
                                severity: Severity::Critical,
                            },
                            confidence,
                            categories: vec![ViolationCategory::Csam],
                            timestamp: chrono::Utc::now(),
                            agent_id: self.agent_card().id,
                            requires_human_review: false,
                        })
                    }
                    CsamClassification::RequiresReview { priority, .. } => {
                        Ok(ModerationDecision {
                            content_id: content.id.clone(),
                            action: ModerationAction::Hide {
                                reason: "Requires review".into(),
                            },
                            confidence: 0.5,
                            categories: vec![ViolationCategory::PotentialCsam],
                            timestamp: chrono::Utc::now(),
                            agent_id: self.agent_card().id,
                            requires_human_review: true,
                        })
                    }
                    _ => {
                        Ok(ModerationDecision {
                            content_id: content.id.clone(),
                            action: ModerationAction::Allow,
                            confidence: 0.99,
                            categories: vec![],
                            timestamp: chrono::Utc::now(),
                            agent_id: self.agent_card().id,
                            requires_human_review: false,
                        })
                    }
                }
            }
            
            ContentType::Avatar | ContentType::Model3D => {
                let analysis = self.avatar_analyzer.analyze_avatar(&content.as_avatar()?).await?;
                
                match analysis {
                    AvatarAnalysis::Violation { confidence, .. } => {
                        Ok(ModerationDecision {
                            content_id: content.id.clone(),
                            action: ModerationAction::Remove {
                                reason: "Sexualized minor avatar".into(),
                                severity: Severity::Critical,
                            },
                            confidence,
                            categories: vec![ViolationCategory::Csam],
                            timestamp: chrono::Utc::now(),
                            agent_id: self.agent_card().id,
                            requires_human_review: false,
                        })
                    }
                    _ => {
                        Ok(ModerationDecision {
                            content_id: content.id.clone(),
                            action: ModerationAction::Allow,
                            confidence: 0.95,
                            categories: vec![],
                            timestamp: chrono::Utc::now(),
                            agent_id: self.agent_card().id,
                            requires_human_review: false,
                        })
                    }
                }
            }
            
            _ => {
                // Other content types
                Ok(ModerationDecision {
                    content_id: content.id.clone(),
                    action: ModerationAction::Allow,
                    confidence: 0.99,
                    categories: vec![],
                    timestamp: chrono::Utc::now(),
                    agent_id: self.agent_card().id,
                    requires_human_review: false,
                })
            }
        }
    }
    
    async fn explain(&self, decision: &ModerationDecision) -> Result<Explanation, AgentError> {
        // CSAM decisions have limited explanations for legal reasons
        Ok(Explanation {
            summary: "Content violated child safety policies".into(),
            factors: vec![],  // Don't reveal detection methods
            confidence_breakdown: None,
        })
    }
    
    async fn learn(&mut self, _feedback: &Feedback) -> Result<(), AgentError> {
        // CSAM agent doesn't learn from feedback
        // Model updates are done through controlled retraining
        Ok(())
    }
    
    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "photodna_check".into(),
                description: "Check image against PhotoDNA database".into(),
                parameters: serde_json::json!({}),
            },
            ToolDefinition {
                name: "classify_image".into(),
                description: "AI classification for unknown images".into(),
                parameters: serde_json::json!({}),
            },
            ToolDefinition {
                name: "analyze_avatar".into(),
                description: "Analyze 3D avatar for policy violations".into(),
                parameters: serde_json::json!({}),
            },
        ]
    }
}
```

---

## Summary

### CSAM Prevention Checklist

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| PhotoDNA Integration | ✅ Designed | `PhotoDnaService` |
| NCMEC Reporting | ✅ Designed | `NcmecClient` |
| 24-Hour Reporting SLA | ✅ Designed | Immediate submission |
| Evidence Preservation | ✅ Designed | 90-day encrypted storage |
| AI Classification | ✅ Designed | `CsamClassifier` |
| Avatar Analysis | ✅ Designed | `AvatarAnalyzer` |
| Immediate Removal | ✅ Designed | `CsamRemovalService` |
| Staff Training | ✅ Designed | Training requirements |

### Key Metrics

| Metric | Target |
|--------|--------|
| Detection Rate | > 99.9% |
| False Positive Rate | < 0.1% |
| NCMEC Report Time | < 1 hour |
| Content Removal Time | < 1 minute |
| Evidence Preservation | 100% |

### Zero Tolerance Principles

1. **No Appeals** — CSAM violations are permanent
2. **Immediate Action** — Content removed within seconds
3. **Full Cooperation** — Law enforcement gets everything they need
4. **Evidence Preservation** — 90+ days, encrypted, access-controlled
5. **Staff Protection** — Minimize human exposure to actual content

---

## Related Documentation

- [DMCA.md](./DMCA.md) — Copyright takedown procedures
- [COPPA.md](./COPPA.md) — Child privacy protections
- [AI_AGENTS.md](../moderation/AI_AGENTS.md) — AI moderation architecture
- [MODERATION_API.md](../moderation/MODERATION_API.md) — Moderation API reference

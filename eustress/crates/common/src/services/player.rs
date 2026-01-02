//! # Player Service Types
//! 
//! Shared data types for player management across Engine and Client.
//! 
//! ## Classes (Eustress-style)
//! 
//! - `Player`: Represents a player (local or remote)
//! - `PlayerProfile`: Player identity and biological data
//! - `Character`: Physical representation (Humanoid equivalent)
//! - `PlayerService`: Service resource managing all players

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// User Verification
// ============================================================================

/// Email verification status
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct EmailVerification {
    /// Whether email is verified
    pub verified: bool,
    /// Masked email (e.g., "j***@gmail.com")
    pub masked_email: Option<String>,
    /// When verified (Unix timestamp)
    pub verified_at: u64,
    /// Verification method used
    pub method: VerificationMethod,
}

/// Phone verification status
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct PhoneVerification {
    /// Whether phone is verified
    pub verified: bool,
    /// Masked phone (e.g., "+1 ***-***-1234")
    pub masked_phone: Option<String>,
    /// Country code (e.g., "US", "GB")
    pub country_code: Option<String>,
    /// When verified (Unix timestamp)
    pub verified_at: u64,
    /// Verification method used
    pub method: VerificationMethod,
}

/// Two-factor authentication status
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct TwoFactorAuth {
    /// Whether 2FA is enabled
    pub enabled: bool,
    /// 2FA method
    pub method: TwoFactorMethod,
    /// When enabled (Unix timestamp)
    pub enabled_at: u64,
    /// Backup codes remaining
    pub backup_codes_remaining: u8,
    /// Recovery email (separate from primary, for 2FA recovery)
    pub recovery_email: Option<String>,
    /// Whether recovery email is verified
    pub recovery_email_verified: bool,
    /// Last recovery attempt timestamp
    pub last_recovery_attempt: u64,
    /// Failed recovery attempts count
    pub failed_recovery_attempts: u8,
    /// Account locked until (0 = not locked)
    pub locked_until: u64,
}

/// Verification method
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum VerificationMethod {
    #[default]
    None,
    /// Code sent via email/SMS
    Code,
    /// Link clicked
    Link,
    /// OAuth provider verified
    OAuth,
    /// Manual admin verification
    Manual,
}

/// Two-factor authentication method
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum TwoFactorMethod {
    #[default]
    None,
    /// Authenticator app (TOTP)
    Authenticator,
    /// SMS codes
    SMS,
    /// Email codes
    Email,
    /// Hardware key (WebAuthn)
    HardwareKey,
}

/// Complete user verification status
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Reflect)]
pub struct UserVerification {
    /// Email verification
    pub email: EmailVerification,
    /// Phone verification
    pub phone: PhoneVerification,
    /// Two-factor authentication
    pub two_factor: TwoFactorAuth,
    /// Identity verification (government ID)
    pub identity_verified: bool,
    /// Age verification (18+)
    pub age_verified: bool,
    /// Parental consent (for minors)
    pub parental_consent: bool,
    /// Account standing
    pub standing: AccountStanding,
    /// Trust score (0-100, based on verification level)
    pub trust_score: u8,
    /// Bliss crypto balance (for trust score calculation)
    pub bliss_balance: u64,
    /// Identity document hash (for phone number override verification)
    pub identity_document_hash: Option<String>,
    /// Pending 2FA recovery request
    pub pending_recovery: Option<TwoFactorRecovery>,
}

impl UserVerification {
    /// Check if email is verified
    pub fn has_verified_email(&self) -> bool {
        self.email.verified
    }
    
    /// Check if phone is verified
    pub fn has_verified_phone(&self) -> bool {
        self.phone.verified
    }
    
    /// Check if 2FA is enabled
    pub fn has_2fa(&self) -> bool {
        self.two_factor.enabled
    }
    
    /// Check if fully verified (email + phone)
    pub fn is_fully_verified(&self) -> bool {
        self.email.verified && self.phone.verified
    }
    
    /// Calculate trust score based on verifications and Bliss balance
    pub fn calculate_trust_score(&self) -> u8 {
        let mut score = 0u8;
        
        // Verification bonuses
        if self.email.verified { score += 15; }
        if self.phone.verified { score += 20; }
        if self.two_factor.enabled { score += 10; }
        if self.identity_verified { score += 20; }
        if self.age_verified { score += 5; }
        
        // Bliss crypto balance bonus (up to 20 points)
        // Tiers: 100 Bliss = 5pts, 1000 = 10pts, 10000 = 15pts, 100000+ = 20pts
        let bliss_bonus = if self.bliss_balance >= 100_000 {
            20
        } else if self.bliss_balance >= 10_000 {
            15
        } else if self.bliss_balance >= 1_000 {
            10
        } else if self.bliss_balance >= 100 {
            5
        } else {
            0
        };
        score += bliss_bonus;
        
        // Account standing modifier
        match self.standing {
            AccountStanding::Good => score += 10,
            AccountStanding::Warning => {},
            AccountStanding::Restricted => score = score.saturating_sub(30),
            AccountStanding::Suspended => score = 0,
        }
        
        score.min(100)
    }
    
    /// Update trust score
    pub fn update_trust_score(&mut self) {
        self.trust_score = self.calculate_trust_score();
    }
    
    /// Update Bliss balance and recalculate trust score
    pub fn set_bliss_balance(&mut self, balance: u64) {
        self.bliss_balance = balance;
        self.update_trust_score();
    }
}

// ============================================================================
// 2FA Recovery System
// ============================================================================

/// Status of a 2FA recovery request
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum RecoveryStatus {
    #[default]
    Pending,
    /// Email verification sent
    EmailSent,
    /// Email verified, awaiting identity check
    EmailVerified,
    /// Identity verification in progress
    IdentityPending,
    /// Identity verified, ready to reset
    IdentityVerified,
    /// Recovery approved
    Approved,
    /// Recovery denied
    Denied,
    /// Recovery expired
    Expired,
    /// Recovery completed
    Completed,
}

/// Type of 2FA recovery
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum RecoveryType {
    #[default]
    /// Standard email-based recovery
    EmailRecovery,
    /// Recovery with backup codes
    BackupCode,
    /// Recovery with identity verification + new phone
    IdentityPhoneOverride,
    /// Admin-assisted recovery
    AdminAssisted,
}

/// A pending 2FA recovery request
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, Reflect)]
pub struct TwoFactorRecovery {
    /// Recovery request ID
    pub id: String,
    /// User ID
    pub user_id: u64,
    /// Recovery type
    pub recovery_type: RecoveryType,
    /// Current status
    pub status: RecoveryStatus,
    /// When initiated
    pub initiated_at: u64,
    /// Expires at
    pub expires_at: u64,
    /// Email verification code (hashed)
    pub email_code_hash: Option<String>,
    /// Email verified at
    pub email_verified_at: u64,
    /// New phone number (for phone override)
    pub new_phone_number: Option<String>,
    /// New phone country code
    pub new_phone_country: Option<String>,
    /// Identity document submitted
    pub identity_submitted: bool,
    /// Identity verification result
    pub identity_match_score: f32,
    /// IP address of request (hashed)
    pub ip_hash: Option<String>,
    /// Device fingerprint
    pub device_fingerprint: Option<String>,
    /// Risk score (0-100, higher = more suspicious)
    pub risk_score: u8,
    /// Admin reviewer ID (if admin-assisted)
    pub admin_reviewer: Option<u64>,
    /// Admin notes
    pub admin_notes: Option<String>,
}

impl TwoFactorRecovery {
    /// Check if recovery has expired
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now > self.expires_at
    }
    
    /// Check if email verification is complete
    pub fn is_email_verified(&self) -> bool {
        self.email_verified_at > 0
    }
    
    /// Check if identity is verified with sufficient confidence
    pub fn is_identity_verified(&self, min_score: f32) -> bool {
        self.identity_submitted && self.identity_match_score >= min_score
    }
    
    /// Check if recovery can proceed (email verified, not compromised)
    pub fn can_proceed(&self) -> bool {
        !self.is_expired() 
            && self.is_email_verified() 
            && self.risk_score < 70 // Not high risk
            && self.status != RecoveryStatus::Denied
    }
}

/// Request to initiate 2FA recovery
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitiateRecoveryRequest {
    pub user_id: u64,
    pub recovery_type: RecoveryType,
    pub email: String,
    pub ip_address: Option<String>,
    pub device_fingerprint: Option<String>,
}

/// Response for recovery initiation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitiateRecoveryResponse {
    pub success: bool,
    pub recovery_id: Option<String>,
    pub message: String,
    /// Masked email where code was sent
    pub email_sent_to: Option<String>,
    /// Seconds until code expires
    pub expires_in_seconds: u64,
}

/// Request to verify email code for recovery
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifyRecoveryEmailRequest {
    pub recovery_id: String,
    pub code: String,
}

/// Request for identity-based phone override
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityPhoneOverrideRequest {
    pub recovery_id: String,
    /// New phone number to register
    pub new_phone_number: String,
    pub new_phone_country: String,
    /// Identity document image (base64)
    pub identity_document: String,
    /// Selfie for liveness check (base64)
    pub selfie: Option<String>,
}

/// Response for identity verification
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityVerificationResponse {
    pub success: bool,
    pub match_score: f32,
    pub message: String,
    /// Whether phone can be overridden
    pub phone_override_approved: bool,
    /// If approved, verification code sent to new phone
    pub verification_sent: bool,
}

/// Request to complete 2FA recovery
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompleteRecoveryRequest {
    pub recovery_id: String,
    /// New phone verification code (if phone override)
    pub phone_code: Option<String>,
    /// New 2FA method to set up
    pub new_2fa_method: Option<TwoFactorMethod>,
    /// Disable 2FA entirely (requires high trust score)
    pub disable_2fa: bool,
}

/// Response for recovery completion
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompleteRecoveryResponse {
    pub success: bool,
    pub message: String,
    /// New backup codes (if 2FA re-enabled)
    pub new_backup_codes: Vec<String>,
    /// QR code for authenticator (if applicable)
    pub authenticator_qr: Option<String>,
}

/// Configuration for 2FA recovery system
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Email code expiration in seconds
    pub email_code_expires_seconds: u64,
    /// Recovery request expiration in hours
    pub recovery_expires_hours: u64,
    /// Max failed attempts before lockout
    pub max_failed_attempts: u8,
    /// Lockout duration in hours
    pub lockout_hours: u64,
    /// Minimum identity match score for phone override (0.0-1.0)
    pub min_identity_match_score: f32,
    /// Minimum trust score to disable 2FA entirely
    pub min_trust_score_disable_2fa: u8,
    /// Risk score threshold for auto-deny
    pub risk_score_auto_deny: u8,
    /// Require admin review for high-risk recoveries
    pub require_admin_review_risk_threshold: u8,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            email_code_expires_seconds: 600, // 10 minutes
            recovery_expires_hours: 24,
            max_failed_attempts: 3,
            lockout_hours: 24,
            min_identity_match_score: 0.85, // 85% confidence
            min_trust_score_disable_2fa: 80,
            risk_score_auto_deny: 90,
            require_admin_review_risk_threshold: 70,
        }
    }
}

/// Account standing status
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum AccountStanding {
    #[default]
    Good,
    /// Has warnings but not restricted
    Warning,
    /// Limited functionality
    Restricted,
    /// Account suspended
    Suspended,
}

// ============================================================================
// Service Status (Military, Disability, First Responder)
// ============================================================================

/// User's service/employment status
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct UserServiceStatus {
    /// Military status
    pub military: MilitaryServiceInfo,
    /// Disability status
    pub disability: DisabilityInfo,
    /// First responder status
    pub first_responder: FirstResponderInfo,
    /// Government employee status
    pub government_employee: GovernmentEmployeeInfo,
}

/// Military service information
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct MilitaryServiceInfo {
    /// Current military status
    pub status: UserMilitaryStatus,
    /// Branch of service
    pub branch: MilitaryBranch,
    /// Verified by official records
    pub verified: bool,
    /// Verification method
    pub verification_method: ServiceVerificationMethod,
    /// When verified
    pub verified_at: u64,
    /// Service start date (Unix timestamp)
    pub service_start: u64,
    /// Service end date (0 = still serving)
    pub service_end: u64,
    /// Discharge type (for veterans)
    pub discharge_type: DischargeType,
    /// Rank at separation
    pub rank: Option<String>,
    /// MOS/Rating/AFSC
    pub specialty_code: Option<String>,
}

/// Military status for users
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum UserMilitaryStatus {
    #[default]
    None,
    ActiveDuty,
    Reserve,
    NationalGuard,
    Veteran,
    Retired,
    Dependent,
}

/// Military branch
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum MilitaryBranch {
    #[default]
    None,
    Army,
    Navy,
    AirForce,
    Marines,
    CoastGuard,
    SpaceForce,
    NationalGuard,
    /// Foreign military (allied nations)
    Foreign,
}

/// Discharge type
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum DischargeType {
    #[default]
    NotApplicable,
    Honorable,
    GeneralUnderHonorable,
    OtherThanHonorable,
    BadConduct,
    Dishonorable,
    /// Still serving
    StillServing,
}

/// Service verification method
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum ServiceVerificationMethod {
    #[default]
    None,
    /// DD-214 or equivalent
    DischargeDocument,
    /// Military ID card
    MilitaryID,
    /// VA verification
    VAVerification,
    /// SCRA database
    SCRADatabase,
    /// ID.me or similar
    ThirdPartyService,
    /// Manual admin verification
    AdminVerified,
}

/// Disability information
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct DisabilityInfo {
    /// Has verified disability
    pub has_disability: bool,
    /// Disability type
    pub disability_type: UserDisabilityType,
    /// Service-connected (VA disability)
    pub service_connected: bool,
    /// VA disability rating (0-100%)
    pub va_rating: u8,
    /// Verified
    pub verified: bool,
    /// When verified
    pub verified_at: u64,
}

/// Disability type for users
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum UserDisabilityType {
    #[default]
    None,
    ServiceConnected,
    Civilian,
    Temporary,
    Permanent,
}

/// First responder information
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct FirstResponderInfo {
    /// Is a first responder
    pub is_first_responder: bool,
    /// Type of first responder
    pub responder_type: FirstResponderType,
    /// Currently active
    pub active: bool,
    /// Verified
    pub verified: bool,
    /// When verified
    pub verified_at: u64,
    /// Department/agency
    pub department: Option<String>,
}

/// First responder type
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum FirstResponderType {
    #[default]
    None,
    Police,
    Fire,
    EMS,
    Paramedic,
    Dispatcher,
    /// Search and rescue
    SAR,
    /// Emergency management
    EmergencyManagement,
}

/// Government employee information
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct GovernmentEmployeeInfo {
    /// Is a government employee
    pub is_government_employee: bool,
    /// Level of government
    pub level: GovernmentLevel,
    /// Has security clearance
    pub has_clearance: bool,
    /// Clearance level
    pub clearance_level: UserSecurityClearance,
    /// Clearance verified
    pub clearance_verified: bool,
    /// Agency/department
    pub agency: Option<String>,
    /// Verified
    pub verified: bool,
    /// When verified
    pub verified_at: u64,
}

/// Government level
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum GovernmentLevel {
    #[default]
    None,
    Federal,
    State,
    County,
    Municipal,
    Tribal,
}

/// Security clearance for users
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum UserSecurityClearance {
    #[default]
    None,
    PublicTrust,
    Confidential,
    Secret,
    TopSecret,
    TopSecretSCI,
}

// ============================================================================
// Background Check
// ============================================================================

/// User's background check status
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct UserBackgroundCheck {
    /// Background check completed
    pub completed: bool,
    /// When completed
    pub completed_at: u64,
    /// Check provider
    pub provider: Option<String>,
    /// Check passed (clean record)
    pub passed: bool,
    /// Has felony convictions
    pub has_felony: bool,
    /// Felony details (if disclosed)
    pub felony_info: Option<FelonyInfo>,
    /// Is registered sex offender
    pub is_sex_offender: bool,
    /// Check expiration (background checks may need renewal)
    pub expires_at: u64,
}

/// Felony information (for transparency/rehabilitation programs)
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct FelonyInfo {
    /// Number of felony convictions
    pub conviction_count: u8,
    /// Most recent conviction year
    pub most_recent_year: u16,
    /// Convictions are expunged/sealed
    pub expunged: bool,
    /// User has completed rehabilitation program
    pub rehabilitation_completed: bool,
    /// Categories of offenses (non-specific for privacy)
    pub offense_categories: Vec<OffenseCategory>,
}

/// Offense category (general, not specific charges)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum OffenseCategory {
    /// Property crimes (theft, burglary)
    Property,
    /// Drug-related
    Drug,
    /// Financial crimes (fraud, embezzlement)
    Financial,
    /// Violent crimes
    Violent,
    /// Sex offenses
    Sexual,
    /// Weapons-related
    Weapons,
    /// DUI/DWI
    DUI,
    /// Other
    Other,
}

/// User's verified age information
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct UserAgeInfo {
    /// Date of birth verified
    pub dob_verified: bool,
    /// Age in years (calculated from DOB)
    pub age: u8,
    /// Birth year (for privacy, not full DOB)
    pub birth_year: u16,
    /// Verification method
    pub verification_method: AgeVerificationMethod,
    /// When verified
    pub verified_at: u64,
}

/// Age verification method
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum AgeVerificationMethod {
    #[default]
    None,
    /// Government ID scan
    GovernmentID,
    /// Credit card (implies 18+)
    CreditCard,
    /// Third-party service (ID.me, etc.)
    ThirdParty,
    /// Parental consent form
    ParentalConsent,
    /// Self-reported (not verified)
    SelfReported,
}

// ============================================================================
// Family Linking System
// ============================================================================

/// Family account linking for parent-child relationships
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct FamilyLink {
    /// Family group ID
    pub family_id: u64,
    /// User's role in the family
    pub role: FamilyRole,
    /// Linked family members (user_id -> relationship)
    pub members: Vec<FamilyMember>,
    /// When family link was established
    pub linked_at: u64,
    /// Family link verified
    pub verified: bool,
}

/// Role in a family group
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum FamilyRole {
    #[default]
    None,
    /// Parent/guardian (can manage children's accounts)
    Parent,
    /// Child (under 13, requires parental consent)
    Child,
    /// Teen (13-17, some parental oversight)
    Teen,
    /// Adult child (18+, independent but linked)
    AdultChild,
    /// Grandparent
    Grandparent,
    /// Other guardian (legal guardian, foster parent)
    Guardian,
    /// Spouse/partner
    Spouse,
}

/// A family member link
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub struct FamilyMember {
    /// User ID of family member
    pub user_id: u64,
    /// Display name
    pub display_name: String,
    /// Relationship to this user
    pub relationship: FamilyRelationship,
    /// Whether this link is verified
    pub verified: bool,
    /// When linked
    pub linked_at: u64,
}

/// Relationship types in a family
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum FamilyRelationship {
    #[default]
    None,
    Parent,
    Child,
    Sibling,
    Grandparent,
    Grandchild,
    Spouse,
    Guardian,
    Ward,
}

impl FamilyLink {
    /// Check if user is a parent/guardian
    pub fn is_parent_or_guardian(&self) -> bool {
        matches!(self.role, FamilyRole::Parent | FamilyRole::Guardian | FamilyRole::Grandparent)
    }
    
    /// Check if user is a minor (child or teen)
    pub fn is_minor(&self) -> bool {
        matches!(self.role, FamilyRole::Child | FamilyRole::Teen)
    }
    
    /// Check if user has a linked parent/guardian
    pub fn has_guardian(&self) -> bool {
        self.members.iter().any(|m| 
            matches!(m.relationship, FamilyRelationship::Parent | FamilyRelationship::Guardian)
        )
    }
    
    /// Get parent/guardian user IDs
    pub fn get_guardians(&self) -> Vec<u64> {
        self.members.iter()
            .filter(|m| matches!(m.relationship, FamilyRelationship::Parent | FamilyRelationship::Guardian))
            .map(|m| m.user_id)
            .collect()
    }
    
    /// Get children user IDs
    pub fn get_children(&self) -> Vec<u64> {
        self.members.iter()
            .filter(|m| matches!(m.relationship, FamilyRelationship::Child | FamilyRelationship::Ward))
            .map(|m| m.user_id)
            .collect()
    }
}

/// Request to create a family link
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateFamilyLinkRequest {
    /// User ID of the person being linked
    pub target_user_id: u64,
    /// Relationship to target
    pub relationship: FamilyRelationship,
    /// Verification method
    pub verification_method: FamilyVerificationMethod,
}

/// Family verification method
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum FamilyVerificationMethod {
    #[default]
    None,
    /// Email confirmation from both parties
    EmailConfirmation,
    /// Phone verification
    PhoneVerification,
    /// Identity document showing relationship
    IdentityDocument,
    /// Court/legal document (adoption, guardianship)
    LegalDocument,
    /// Same household verification (address match)
    AddressMatch,
    /// Admin verification
    AdminVerified,
}

/// Response for family link request
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FamilyLinkResponse {
    pub success: bool,
    pub family_id: Option<u64>,
    pub message: String,
    /// Pending verification required
    pub verification_pending: bool,
}

// ============================================================================
// Verification Web API Types
// ============================================================================

/// Request to start email verification
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StartEmailVerificationRequest {
    pub email: String,
}

/// Request to complete email verification
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompleteEmailVerificationRequest {
    pub email: String,
    pub code: String,
}

/// Request to start phone verification
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StartPhoneVerificationRequest {
    pub phone_number: String,
    pub country_code: String,
}

/// Request to complete phone verification
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletePhoneVerificationRequest {
    pub phone_number: String,
    pub code: String,
}

/// Request to enable 2FA
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Enable2FARequest {
    pub method: TwoFactorMethod,
    /// For authenticator: the TOTP secret
    pub secret: Option<String>,
    /// Verification code to confirm setup
    pub verification_code: String,
}

/// Response for 2FA setup
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Enable2FAResponse {
    pub success: bool,
    /// QR code data URL for authenticator apps
    pub qr_code: Option<String>,
    /// Backup codes (only shown once)
    pub backup_codes: Vec<String>,
    pub error: Option<String>,
}

/// Verification status response for profile settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationStatusResponse {
    pub email_verified: bool,
    pub email_masked: Option<String>,
    pub phone_verified: bool,
    pub phone_masked: Option<String>,
    pub two_factor_enabled: bool,
    pub two_factor_method: TwoFactorMethod,
    pub identity_verified: bool,
    pub age_verified: bool,
    pub trust_score: u8,
    pub standing: AccountStanding,
}

impl From<&UserVerification> for VerificationStatusResponse {
    fn from(v: &UserVerification) -> Self {
        Self {
            email_verified: v.email.verified,
            email_masked: v.email.masked_email.clone(),
            phone_verified: v.phone.verified,
            phone_masked: v.phone.masked_phone.clone(),
            two_factor_enabled: v.two_factor.enabled,
            two_factor_method: v.two_factor.method,
            identity_verified: v.identity_verified,
            age_verified: v.age_verified,
            trust_score: v.trust_score,
            standing: v.standing,
        }
    }
}

// ============================================================================
// Biological Sex / Chromosome
// ============================================================================

/// Biological sex based on chromosomes (XX/XY)
/// Used for character model selection (X-Bot vs Y-Bot)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Reflect, Serialize, Deserialize)]
pub enum BiologicalSex {
    /// XX chromosome - Female (uses Y-Bot model)
    #[default]
    Female,
    /// XY chromosome - Male (uses X-Bot model)
    Male,
}

impl BiologicalSex {
    /// Get the character model for this biological sex
    /// X-Bot = Female, Y-Bot = Male (Mixamo naming convention)
    pub fn character_model(&self) -> crate::plugins::skinned_character::CharacterModel {
        match self {
            BiologicalSex::Female => crate::plugins::skinned_character::CharacterModel::XBot,
            BiologicalSex::Male => crate::plugins::skinned_character::CharacterModel::YBot,
        }
    }
    
    /// Get the character gender for animation selection
    pub fn character_gender(&self) -> crate::plugins::skinned_character::CharacterGender {
        match self {
            BiologicalSex::Female => crate::plugins::skinned_character::CharacterGender::Female,
            BiologicalSex::Male => crate::plugins::skinned_character::CharacterGender::Male,
        }
    }
    
    /// Create from string (for API/database)
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "male" | "m" | "xy" => BiologicalSex::Male,
            "female" | "f" | "xx" => BiologicalSex::Female,
            _ => BiologicalSex::Female, // Default
        }
    }
}

// ============================================================================
// Player Profile
// ============================================================================

/// Player profile data - identity and biological information
/// This is fetched from the user's account/database
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct PlayerProfile {
    /// Unique user ID (primary key)
    pub user_id: u64,
    /// Display name
    pub display_name: String,
    /// Biological sex (XX/XY chromosome) - determines character model
    pub biological_sex: BiologicalSex,
    /// Avatar customization data (JSON or struct)
    pub avatar_data: Option<String>,
    /// Account creation timestamp (Unix epoch)
    pub created_at: u64,
    /// Premium membership status
    pub is_premium: bool,
    /// Verified account (legacy - use verification field)
    pub is_verified: bool,
    /// Detailed verification status
    #[serde(default)]
    pub verification: UserVerification,
}

impl Default for PlayerProfile {
    fn default() -> Self {
        Self {
            user_id: 0,
            display_name: "Guest".to_string(),
            biological_sex: BiologicalSex::Female,
            avatar_data: None,
            created_at: 0,
            is_premium: false,
            is_verified: false,
            verification: UserVerification::default(),
        }
    }
}

impl PlayerProfile {
    /// Create a new profile with user ID and name
    pub fn new(user_id: u64, display_name: impl Into<String>) -> Self {
        Self {
            user_id,
            display_name: display_name.into(),
            ..default()
        }
    }
    
    /// Set biological sex
    pub fn with_biological_sex(mut self, sex: BiologicalSex) -> Self {
        self.biological_sex = sex;
        self
    }
    
    /// Get the appropriate character model based on biological sex
    pub fn character_model(&self) -> crate::plugins::skinned_character::CharacterModel {
        self.biological_sex.character_model()
    }
    
    /// Get the appropriate character gender for animations
    pub fn character_gender(&self) -> crate::plugins::skinned_character::CharacterGender {
        self.biological_sex.character_gender()
    }
}

// ============================================================================
// Player Class
// ============================================================================

/// Player class - represents a player in the game (like Eustress's Player)
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Player {
    /// Player's display name
    pub name: String,
    /// Player's unique user ID
    pub user_id: u64,
    /// Team color (optional)
    pub team_color: Option<[f32; 4]>,
    /// Is this the local player?
    pub is_local: bool,
    /// Character entity (if spawned)
    #[serde(skip)]
    pub character: Option<Entity>,
    /// Player profile (biological data, avatar, etc.)
    #[serde(skip)]
    #[reflect(ignore)]
    pub profile: Option<PlayerProfile>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            name: "Player".to_string(),
            user_id: 0,
            team_color: None,
            is_local: true,
            character: None,
            profile: None,
        }
    }
}

impl Player {
    pub fn new(name: impl Into<String>, user_id: u64) -> Self {
        Self {
            name: name.into(),
            user_id,
            ..default()
        }
    }
    
    pub fn local(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_local: true,
            ..default()
        }
    }
    
    /// Create player with profile
    pub fn with_profile(mut self, profile: PlayerProfile) -> Self {
        self.name = profile.display_name.clone();
        self.user_id = profile.user_id;
        self.profile = Some(profile);
        self
    }
    
    /// Get biological sex from profile (defaults to Female if no profile)
    pub fn biological_sex(&self) -> BiologicalSex {
        self.profile.as_ref()
            .map(|p| p.biological_sex)
            .unwrap_or(BiologicalSex::Female)
    }
    
    /// Get character model based on biological sex
    pub fn character_model(&self) -> crate::plugins::skinned_character::CharacterModel {
        self.biological_sex().character_model()
    }
    
    /// Get character gender for animations
    pub fn character_gender(&self) -> crate::plugins::skinned_character::CharacterGender {
        self.biological_sex().character_gender()
    }
}

// ============================================================================
// Character Class (Humanoid equivalent)
// ============================================================================

/// Character component - the physical representation of a player
/// Similar to Eustress's Humanoid
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct Character {
    /// Walk speed (studs/sec)
    pub walk_speed: f32,
    /// Run/sprint speed multiplier
    pub sprint_multiplier: f32,
    /// Jump power (impulse)
    pub jump_power: f32,
    /// Maximum health
    pub max_health: f32,
    /// Current health
    pub health: f32,
    /// Is character on ground
    #[serde(skip)]
    pub grounded: bool,
    /// Hip height (distance from ground to root)
    pub hip_height: f32,
    /// Can the character jump?
    pub can_jump: bool,
    /// Can the character move?
    pub can_move: bool,
}

impl Default for Character {
    fn default() -> Self {
        Self {
            // Movement speeds matched to animation playback (1 unit = 1 meter)
            // Walk animation plays naturally at ~1.6 m/s
            // Run animation plays naturally at ~4.0 m/s
            walk_speed: 1.8,   // Natural walk: 1.8 m/s (~4 mph) - animation at 1.125x
            sprint_multiplier: 3.0,  // Sprint: 5.4 m/s (~12 mph) - animation at 1.35x
            jump_power: 5.5,   // Good jump height for gameplay
            max_health: 100.0,
            health: 100.0,
            grounded: false,
            hip_height: 0.95,  // Realistic hip height for 1.75m human
            can_jump: true,
            can_move: true,
        }
    }
}

impl Character {
    /// Take damage, returns true if character died
    pub fn take_damage(&mut self, amount: f32) -> bool {
        self.health = (self.health - amount).max(0.0);
        self.health <= 0.0
    }
    
    /// Heal the character
    pub fn heal(&mut self, amount: f32) {
        self.health = (self.health + amount).min(self.max_health);
    }
    
    /// Is the character alive?
    pub fn is_alive(&self) -> bool {
        self.health > 0.0
    }
}

// ============================================================================
// Character Part Markers
// ============================================================================

/// Marks the character's root part (HumanoidRootPart equivalent)
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct CharacterRoot;

/// Marks the character's head
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct CharacterHead;

/// Marks the character's torso
#[derive(Component, Reflect, Default)]
#[reflect(Component)]
pub struct CharacterTorso;

// ============================================================================
// PlayerService Resource
// ============================================================================

/// PlayerService - manages all players (like Eustress's Players service)
/// 
/// # Network Integration
/// 
/// Default character properties are applied when spawning new Humanoids.
/// Per-player overrides allow server-side speed boosts/nerfs.
/// 
/// # Example
/// ```rust,ignore
/// // Apply speed boost to a player
/// player_service.set_speed_multiplier(client_id, 1.5);
/// ```
#[derive(Resource, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Resource)]
pub struct PlayerService {
    // === Character Defaults (applied to new Humanoids) ===
    
    /// Default walk speed for new characters (studs/s)
    pub default_walk_speed: f32,
    
    /// Default run speed for new characters (studs/s)
    pub default_run_speed: f32,
    
    /// Default jump power for new characters (studs/s)
    pub default_jump_power: f32,
    
    /// Default max health for new characters
    pub default_max_health: f32,
    
    /// Enable auto-jump when walking into obstacles
    pub auto_jump_enabled: bool,
    
    // === Spawn Configuration ===
    
    /// Default spawn position for new players
    pub spawn_position: Vec3,
    
    /// Character respawn time in seconds
    pub respawn_time: f32,
    
    /// Maximum players allowed
    pub max_players: u32,
    
    // === Per-Player Overrides (runtime, not serialized) ===
    
    /// Speed multipliers per client (for boosts/debuffs)
    /// Key: client_id, Value: multiplier (1.0 = normal)
    #[serde(skip)]
    #[reflect(ignore)]
    pub speed_multipliers: std::collections::HashMap<u64, f32>,
    
    // === Runtime State (not serialized) ===
    
    /// The local player entity
    #[serde(skip)]
    pub local_player: Option<Entity>,
    
    /// Is cursor locked for gameplay (client-specific)
    #[serde(skip)]
    pub cursor_locked: bool,
}

impl Default for PlayerService {
    fn default() -> Self {
        Self {
            // Character defaults (Roblox-compatible)
            default_walk_speed: 16.0,   // studs/s
            default_run_speed: 32.0,    // studs/s (2x walk)
            default_jump_power: 50.0,   // studs/s
            default_max_health: 100.0,
            auto_jump_enabled: true,
            
            // Spawn config
            spawn_position: Vec3::new(0.0, 5.0, 0.0),
            respawn_time: 5.0,
            max_players: 100,
            
            // Runtime
            speed_multipliers: std::collections::HashMap::new(),
            local_player: None,
            cursor_locked: false,
        }
    }
}

impl PlayerService {
    /// Set the spawn position
    pub fn with_spawn_position(mut self, pos: Vec3) -> Self {
        self.spawn_position = pos;
        self
    }
    
    /// Set default character speeds
    pub fn with_speeds(mut self, walk: f32, run: f32) -> Self {
        self.default_walk_speed = walk;
        self.default_run_speed = run;
        self
    }
    
    /// Set speed multiplier for a specific client
    pub fn set_speed_multiplier(&mut self, client_id: u64, multiplier: f32) {
        self.speed_multipliers.insert(client_id, multiplier);
    }
    
    /// Get speed multiplier for a client (1.0 if not set)
    pub fn get_speed_multiplier(&self, client_id: u64) -> f32 {
        self.speed_multipliers.get(&client_id).copied().unwrap_or(1.0)
    }
    
    /// Remove speed multiplier for a client
    pub fn clear_speed_multiplier(&mut self, client_id: u64) {
        self.speed_multipliers.remove(&client_id);
    }
    
    /// Apply default properties to a Humanoid
    pub fn apply_defaults(&self, humanoid: &mut crate::classes::Humanoid) {
        humanoid.walk_speed = self.default_walk_speed;
        humanoid.run_speed = self.default_run_speed;
        humanoid.jump_power = self.default_jump_power;
        humanoid.max_health = self.default_max_health;
        humanoid.health = self.default_max_health;
    }
    
    /// Get effective walk speed for a client (with multiplier)
    pub fn effective_walk_speed(&self, client_id: u64) -> f32 {
        self.default_walk_speed * self.get_speed_multiplier(client_id)
    }
    
    /// Get effective run speed for a client (with multiplier)
    pub fn effective_run_speed(&self, client_id: u64) -> f32 {
        self.default_run_speed * self.get_speed_multiplier(client_id)
    }
}

// ============================================================================
// Spawn Location Utilities
// ============================================================================

/// Find the best spawn position from SpawnLocation entities using team ID
/// Returns the position and spawn protection duration
/// 
/// # Arguments
/// * `spawn_locations` - Iterator of (Transform, SpawnLocation) tuples
/// * `player_team_id` - Optional team ID for the player (0 or None = no team)
/// 
/// # Returns
/// * `Some((position, protection_duration))` if a valid spawn was found
/// * `None` if no valid spawns exist
pub fn find_spawn_position_by_team_id<'a>(
    spawn_locations: impl Iterator<Item = (&'a Transform, &'a crate::classes::SpawnLocation)>,
    player_team_id: Option<u32>,
) -> Option<(Vec3, f32)> {
    let mut valid_spawns: Vec<_> = spawn_locations
        .filter(|(_, spawn)| spawn.can_spawn_team_id(player_team_id))
        .collect();
    
    if valid_spawns.is_empty() {
        return None;
    }
    
    // Sort by priority (highest first)
    valid_spawns.sort_by(|a, b| b.1.priority.cmp(&a.1.priority));
    
    // Get highest priority spawns
    let max_priority = valid_spawns[0].1.priority;
    let top_spawns: Vec<_> = valid_spawns
        .into_iter()
        .filter(|(_, spawn)| spawn.priority == max_priority)
        .collect();
    
    // Pick a random spawn from top priority (or first if only one)
    // For now, just pick the first one - could add randomization later
    let (transform, spawn) = top_spawns.first()?;
    
    // Spawn slightly above the spawn location to avoid clipping
    let spawn_pos = transform.translation + Vec3::Y * 2.0;
    
    Some((spawn_pos, spawn.spawn_protection_duration))
}

/// Find the best spawn position from SpawnLocation entities (legacy string-based)
/// Returns the position and spawn protection duration
/// 
/// # Arguments
/// * `spawn_locations` - Iterator of (Transform, SpawnLocation) tuples
/// * `player_team` - Optional team name for the player
/// 
/// # Returns
/// * `Some((position, protection_duration))` if a valid spawn was found
/// * `None` if no valid spawns exist
pub fn find_spawn_position<'a>(
    spawn_locations: impl Iterator<Item = (&'a Transform, &'a crate::classes::SpawnLocation)>,
    player_team: Option<&str>,
) -> Option<(Vec3, f32)> {
    let mut valid_spawns: Vec<_> = spawn_locations
        .filter(|(_, spawn)| spawn.can_spawn(player_team))
        .collect();
    
    if valid_spawns.is_empty() {
        return None;
    }
    
    // Sort by priority (highest first)
    valid_spawns.sort_by(|a, b| b.1.priority.cmp(&a.1.priority));
    
    // Get highest priority spawns
    let max_priority = valid_spawns[0].1.priority;
    let top_spawns: Vec<_> = valid_spawns
        .into_iter()
        .filter(|(_, spawn)| spawn.priority == max_priority)
        .collect();
    
    // Pick a random spawn from top priority (or first if only one)
    // For now, just pick the first one - could add randomization later
    let (transform, spawn) = top_spawns.first()?;
    
    // Spawn slightly above the spawn location to avoid clipping
    let spawn_pos = transform.translation + Vec3::Y * 2.0;
    
    Some((spawn_pos, spawn.spawn_protection_duration))
}

/// Get spawn position by team ID, falling back to default if no SpawnLocations exist
pub fn get_spawn_position_by_team_id_or_default<'a>(
    spawn_locations: impl Iterator<Item = (&'a Transform, &'a crate::classes::SpawnLocation)>,
    player_team_id: Option<u32>,
    default_position: Vec3,
) -> (Vec3, f32) {
    find_spawn_position_by_team_id(spawn_locations, player_team_id)
        .unwrap_or((default_position + Vec3::Y * 2.0, 0.0))
}

/// Get spawn position, falling back to default if no SpawnLocations exist (legacy)
pub fn get_spawn_position_or_default<'a>(
    spawn_locations: impl Iterator<Item = (&'a Transform, &'a crate::classes::SpawnLocation)>,
    player_team: Option<&str>,
    default_position: Vec3,
) -> (Vec3, f32) {
    find_spawn_position(spawn_locations, player_team)
        .unwrap_or((default_position + Vec3::Y * 2.0, 0.0))
}

// ============================================================================
// Camera Types
// ============================================================================

/// Player camera configuration
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
#[reflect(Component)]
pub struct PlayerCamera {
    /// Distance from character (third person)
    pub distance: f32,
    /// Minimum zoom distance (first person threshold)
    pub min_distance: f32,
    /// Maximum zoom distance
    pub max_distance: f32,
    /// Zoom speed (scroll sensitivity)
    pub zoom_speed: f32,
    /// Pitch angle (up/down) in radians
    pub pitch: f32,
    /// Yaw angle (left/right) in radians  
    pub yaw: f32,
    /// Mouse sensitivity
    pub sensitivity: f32,
    /// Minimum pitch (looking up limit)
    pub pitch_min: f32,
    /// Maximum pitch (looking down limit)
    pub pitch_max: f32,
    /// Camera mode
    pub mode: CameraMode,
    /// Target entity to follow
    #[serde(skip)]
    pub target: Option<Entity>,
    /// Is in first person mode (head hidden locally)
    pub is_first_person: bool,
}

impl Default for PlayerCamera {
    fn default() -> Self {
        Self {
            distance: 5.0,
            min_distance: 0.5,   // Below this = first person
            max_distance: 20.0,
            zoom_speed: 1.0,
            pitch: -15.0_f32.to_radians(),
            yaw: 0.0,
            sensitivity: 0.003,
            pitch_min: -80.0_f32.to_radians(),
            pitch_max: 80.0_f32.to_radians(),
            mode: CameraMode::ThirdPerson,
            target: None,
            is_first_person: false,
        }
    }
}

/// Camera mode
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum CameraMode {
    #[default]
    ThirdPerson,
    FirstPerson,
    Freecam,
    Fixed,
}

// ============================================================================
// Profile Cache Resource
// ============================================================================

/// Cache of player profiles by user ID
/// Used to look up biological sex and other data when spawning characters
#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct PlayerProfileCache {
    /// Cached profiles by user ID
    #[reflect(ignore)]
    pub profiles: HashMap<u64, PlayerProfile>,
}

impl PlayerProfileCache {
    /// Insert or update a profile
    pub fn insert(&mut self, profile: PlayerProfile) {
        self.profiles.insert(profile.user_id, profile);
    }
    
    /// Get a profile by user ID
    pub fn get(&self, user_id: u64) -> Option<&PlayerProfile> {
        self.profiles.get(&user_id)
    }
    
    /// Get biological sex for a user ID (defaults to Female if not found)
    pub fn get_biological_sex(&self, user_id: u64) -> BiologicalSex {
        self.profiles.get(&user_id)
            .map(|p| p.biological_sex)
            .unwrap_or(BiologicalSex::Female)
    }
    
    /// Get character model for a user ID
    pub fn get_character_model(&self, user_id: u64) -> crate::plugins::skinned_character::CharacterModel {
        self.get_biological_sex(user_id).character_model()
    }
    
    /// Get character gender for a user ID
    pub fn get_character_gender(&self, user_id: u64) -> crate::plugins::skinned_character::CharacterGender {
        self.get_biological_sex(user_id).character_gender()
    }
    
    /// Remove a profile
    pub fn remove(&mut self, user_id: u64) -> Option<PlayerProfile> {
        self.profiles.remove(&user_id)
    }
    
    /// Clear all profiles
    pub fn clear(&mut self) {
        self.profiles.clear();
    }
}

// ============================================================================
// Character Spawning Helpers
// ============================================================================

/// Spawn a skinned character for a player using their profile data
pub fn spawn_character_for_player(
    commands: &mut Commands,
    asset_server: &AssetServer,
    player: &Player,
    spawn_pos: Vec3,
) -> Entity {
    let model = player.character_model();
    let gender = player.character_gender();
    
    crate::plugins::skinned_character::spawn_skinned_character(
        commands,
        asset_server,
        spawn_pos,
        model,
        gender,
    )
}

/// Spawn a skinned character using profile cache lookup by user ID
pub fn spawn_character_for_user_id(
    commands: &mut Commands,
    asset_server: &AssetServer,
    profile_cache: &PlayerProfileCache,
    user_id: u64,
    spawn_pos: Vec3,
) -> Entity {
    let model = profile_cache.get_character_model(user_id);
    let gender = profile_cache.get_character_gender(user_id);
    
    crate::plugins::skinned_character::spawn_skinned_character(
        commands,
        asset_server,
        spawn_pos,
        model,
        gender,
    )
}

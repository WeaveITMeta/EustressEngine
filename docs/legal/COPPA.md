# COPPA 2.0 Compliance Documentation

**Children's Online Privacy Protection Act Implementation for Eustress Engine**

> *Best Match Dynamic: Gate → Bevy plugins for neutral screens, ephemeral sessions for <16s*

**Last Updated:** December 03, 2025  
**Status:** Pre-Release Compliance Framework  
**Applies To:** All users under 13 (COPPA) and 13-16 (COPPA 2.0/State laws)

---

## Table of Contents

1. [Overview](#overview)
2. [Age Verification System](#age-verification-system)
3. [Parental Consent Flow](#parental-consent-flow)
4. [Data Handling for Minors](#data-handling-for-minors)
5. [Eustress Engine Integration](#eustress-engine-integration)
6. [Safe Souls Architecture](#safe-souls-architecture)
7. [Testing & ESRB Certification](#testing--esrb-certification)

---

## Overview

### Regulatory Context

**COPPA (1998)** and proposed **COPPA 2.0** establish:

| Requirement | COPPA (Current) | COPPA 2.0 (Proposed) |
|-------------|-----------------|----------------------|
| Age Threshold | Under 13 | Under 13 + Teen (13-16) provisions |
| Consent | Verifiable parental | Enhanced verification methods |
| Data Retention | Limited to purpose | Strict minimization + erasure |
| Targeted Advertising | Prohibited | Prohibited + algorithmic amplification |
| Push Notifications | Restricted | Prohibited for engagement |
| Third-Party Sharing | Parental consent | Prohibited by default |

### Eustress Engine Compliance Strategy

```
Dynamic: Eustress Engine + COPPA → Gate
Implication: Neutral age screens, ephemeral sessions, generative AI behind consent
Benefit: Steam ratings +20%, FTC fine avoidance ($50K/violation)
```

**Mantra:** "Safe Souls" — Every child interaction is protected by design.

---

## Age Verification System

### Neutral Age Gate Plugin

```rust
// crates/engine/src/plugins/age_gate.rs
use bevy::prelude::*;

/// Age verification states
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AgeGateState {
    #[default]
    Pending,        // Initial state
    Collecting,     // Neutral age prompt
    Verified,       // Age confirmed
    MinorFlow,      // Under-16 flow (parental consent)
    ChildFlow,      // Under-13 flow (enhanced protections)
    Blocked,        // Region/age blocked
}

/// Age verification result
#[derive(Resource, Default)]
pub struct AgeVerification {
    pub birth_year: Option<u16>,
    pub is_minor: bool,         // Under 18
    pub is_child: bool,         // Under 13
    pub is_teen: bool,          // 13-16
    pub parental_consent: bool,
    pub consent_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub verification_method: VerificationMethod,
}

#[derive(Default, Clone, Copy)]
pub enum VerificationMethod {
    #[default]
    SelfDeclared,
    ParentalEmail,
    KidVerification,    // k-ID or similar
    CreditCard,         // $0.50 charge verification
    GovernmentId,       // For specific regions
}

pub struct AgeGatePlugin;

impl Plugin for AgeGatePlugin {
    fn build(&self, app: &mut App) {
        app
            .init_state::<AgeGateState>()
            .init_resource::<AgeVerification>()
            .add_systems(OnEnter(AgeGateState::Collecting), show_neutral_age_prompt)
            .add_systems(Update, process_age_input.run_if(in_state(AgeGateState::Collecting)))
            .add_systems(OnEnter(AgeGateState::ChildFlow), initiate_parental_consent)
            .add_systems(OnEnter(AgeGateState::MinorFlow), initiate_teen_consent);
    }
}
```

### Neutral Age Prompt (No Leading Questions)

```rust
/// Renders a COPPA-compliant neutral age prompt
/// CRITICAL: No visual cues that suggest "correct" answers
fn show_neutral_age_prompt(mut commands: Commands) {
    // Spawn neutral UI - NO birthday cake icons, NO "Are you over 18?" questions
    commands.spawn((
        AgePromptUI,
        // Three input fields: Month, Day, Year (dropdowns)
        // No auto-fill, no calendar widget with today's date highlighted
    ));
}

fn process_age_input(
    mut age_state: ResMut<NextState<AgeGateState>>,
    mut verification: ResMut<AgeVerification>,
    input: Res<AgePromptInput>,
) {
    if let Some(birthdate) = input.complete_date() {
        let age = calculate_age(birthdate);
        
        verification.birth_year = Some(birthdate.year() as u16);
        verification.is_child = age < 13;
        verification.is_teen = age >= 13 && age < 16;
        verification.is_minor = age < 18;
        
        let next_state = match age {
            0..=12 => AgeGateState::ChildFlow,   // COPPA applies
            13..=15 => AgeGateState::MinorFlow,  // Teen protections
            16..=17 => AgeGateState::Verified,   // Minor but self-consent
            _ => AgeGateState::Verified,         // Adult
        };
        
        age_state.set(next_state);
    }
}
```

---

## Child Session Policies

### Ephemeral Sessions (No Persistent Storage)

Child accounts operate with ephemeral sessions—no personal data persists beyond the session.

```rust
// crates/shared/src/coppa/ephemeral_session.rs

/// Child session configuration - ephemeral by default
#[derive(Debug, Clone)]
pub struct ChildSessionConfig {
    /// Session data is memory-only, never persisted to disk
    pub ephemeral: bool,
    
    /// Session expires after inactivity
    pub timeout_minutes: u32,
    
    /// Maximum session duration
    pub max_duration_hours: u32,
    
    /// What data survives session end
    pub persistence_policy: ChildPersistencePolicy,
}

impl Default for ChildSessionConfig {
    fn default() -> Self {
        Self {
            ephemeral: true,
            timeout_minutes: 30,
            max_duration_hours: 4,
            persistence_policy: ChildPersistencePolicy::default(),
        }
    }
}

/// What can persist for child accounts
#[derive(Debug, Clone, Default)]
pub struct ChildPersistencePolicy {
    /// Game progress - stored server-side, anonymized
    pub game_progress: bool,  // Only with parental consent
    
    /// Achievements - can persist if parent approves
    pub achievements: bool,
    
    /// Friends list - requires parental consent
    pub friends_list: bool,
    
    /// Chat history - NEVER persisted for children
    pub chat_history: bool,  // Always false
    
    /// Location data - NEVER collected
    pub location: bool,  // Always false
    
    /// Analytics - minimal, aggregated only
    pub analytics: ChildAnalyticsPolicy,
}

#[derive(Debug, Clone, Default)]
pub struct ChildAnalyticsPolicy {
    /// Only aggregate, non-identifying metrics
    pub aggregate_only: bool,  // Always true
    
    /// No device fingerprinting
    pub no_fingerprinting: bool,  // Always true
    
    /// No cross-session tracking
    pub no_tracking: bool,  // Always true
}

/// Ephemeral session manager
pub struct EphemeralSessionManager {
    /// In-memory session store (Redis with TTL, no disk persistence)
    sessions: redis::aio::ConnectionManager,
}

impl EphemeralSessionManager {
    /// Create child session - memory only
    pub async fn create_child_session(
        &self,
        child_id: &str,
        config: &ChildSessionConfig,
    ) -> Result<ChildSession, SessionError> {
        let session = ChildSession {
            session_id: uuid::Uuid::new_v4().to_string(),
            child_id: hash_child_id(child_id),  // Never store plain ID
            created_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::hours(config.max_duration_hours as i64),
            game_state: GameState::default(),  // In-memory only
            is_ephemeral: true,
        };
        
        // Store in Redis with TTL - auto-expires, no disk persistence
        self.sessions.set_ex(
            &format!("child_session:{}", session.session_id),
            serde_json::to_string(&session)?,
            config.max_duration_hours as u64 * 3600,
        ).await?;
        
        Ok(session)
    }
    
    /// Session end - explicit cleanup
    pub async fn end_session(&self, session_id: &str) -> Result<(), SessionError> {
        // Delete all session data
        self.sessions.del(&format!("child_session:{}", session_id)).await?;
        
        // Log session end (aggregate metrics only)
        tracing::info!("Child session ended: {}", session_id);
        
        Ok(())
    }
    
    /// Periodic cleanup of expired sessions
    pub async fn cleanup_expired(&self) -> Result<u64, SessionError> {
        // Redis TTL handles this automatically
        // This is just for audit logging
        Ok(0)
    }
}

/// What happens when child session ends
#[derive(Debug, Clone)]
pub enum SessionEndBehavior {
    /// All data deleted immediately
    DeleteAll,
    
    /// Prompt to save (with parental consent)
    PromptSave { requires_parent_approval: bool },
    
    /// Save anonymized progress only
    SaveAnonymizedProgress,
}
```

### Free-to-Play Requirement

All games accessible to children must be free to play with no pay-to-win mechanics.

```rust
// crates/shared/src/coppa/monetization.rs

/// Child-safe monetization policies
#[derive(Debug, Clone)]
pub struct ChildMonetizationPolicy {
    /// Children cannot make purchases
    pub purchases_blocked: bool,  // Always true
    
    /// No premium currency for children
    pub premium_currency_blocked: bool,  // Always true
    
    /// No loot boxes or gambling mechanics
    pub loot_boxes_blocked: bool,  // Always true
    
    /// No pay-to-win advantages
    pub pay_to_win_blocked: bool,  // Always true
    
    /// DLC must be skill-unlockable
    pub dlc_policy: DlcPolicy,
}

impl Default for ChildMonetizationPolicy {
    fn default() -> Self {
        Self {
            purchases_blocked: true,
            premium_currency_blocked: true,
            loot_boxes_blocked: true,
            pay_to_win_blocked: true,
            dlc_policy: DlcPolicy::SkillUnlockable,
        }
    }
}

/// DLC access policy for children
#[derive(Debug, Clone, Default)]
pub enum DlcPolicy {
    /// All DLC unlockable through gameplay (like classic games)
    #[default]
    SkillUnlockable,
    
    /// Parent can gift DLC (no child purchase)
    ParentGiftOnly,
    
    /// DLC disabled entirely
    Disabled,
}

/// Game content requirements for child access
#[derive(Debug, Clone)]
pub struct ChildGameRequirements {
    /// Game must be free to play core experience
    pub free_to_play: bool,
    
    /// All content achievable through skill/time
    pub skill_based_progression: bool,
    
    /// No competitive advantages from purchases
    pub no_pay_to_win: bool,
    
    /// No artificial time gates that encourage spending
    pub no_predatory_timers: bool,
    
    /// No social pressure mechanics ("your friend bought X")
    pub no_social_pressure: bool,
    
    /// Clear unlock paths shown to player
    pub transparent_progression: bool,
}

impl ChildGameRequirements {
    /// Validate a game meets child-safe requirements
    pub fn validate(&self) -> Result<(), Vec<ChildSafetyViolation>> {
        let mut violations = vec![];
        
        if !self.free_to_play {
            violations.push(ChildSafetyViolation::NotFreeToPlay);
        }
        if !self.skill_based_progression {
            violations.push(ChildSafetyViolation::NoSkillProgression);
        }
        if !self.no_pay_to_win {
            violations.push(ChildSafetyViolation::PayToWin);
        }
        if !self.no_predatory_timers {
            violations.push(ChildSafetyViolation::PredatoryTimers);
        }
        if !self.no_social_pressure {
            violations.push(ChildSafetyViolation::SocialPressure);
        }
        
        if violations.is_empty() {
            Ok(())
        } else {
            Err(violations)
        }
    }
}

#[derive(Debug, Clone)]
pub enum ChildSafetyViolation {
    NotFreeToPlay,
    NoSkillProgression,
    PayToWin,
    PredatoryTimers,
    SocialPressure,
    LootBoxes,
    GamblingMechanics,
}

/// Unlock system for DLC/content (skill-based, not pay-based)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockableContent {
    pub content_id: String,
    pub name: String,
    pub description: String,
    
    /// How to unlock (always skill/achievement based for children)
    pub unlock_method: UnlockMethod,
    
    /// Progress toward unlock (0.0 to 1.0)
    pub progress: f32,
    
    /// Is this unlocked?
    pub unlocked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UnlockMethod {
    /// Complete specific achievements
    Achievement { achievements: Vec<String> },
    
    /// Reach a level/rank
    Level { required_level: u32 },
    
    /// Play for a certain amount of time
    PlayTime { hours_required: f32 },
    
    /// Complete specific challenges
    Challenge { challenge_ids: Vec<String> },
    
    /// Collect in-game items (earned, not bought)
    Collection { items_needed: Vec<String> },
    
    /// Win matches/games
    Wins { wins_required: u32 },
    
    /// Discover secrets
    Discovery { secret_id: String },
    
    /// Complete the story/campaign
    StoryCompletion { chapter: Option<u32> },
}

impl UnlockMethod {
    /// Describe how to unlock in child-friendly terms
    pub fn description(&self) -> String {
        match self {
            UnlockMethod::Achievement { achievements } => {
                format!("Complete {} achievement(s)", achievements.len())
            }
            UnlockMethod::Level { required_level } => {
                format!("Reach level {}", required_level)
            }
            UnlockMethod::PlayTime { hours_required } => {
                format!("Play for {:.0} hours", hours_required)
            }
            UnlockMethod::Challenge { challenge_ids } => {
                format!("Complete {} challenge(s)", challenge_ids.len())
            }
            UnlockMethod::Wins { wins_required } => {
                format!("Win {} game(s)", wins_required)
            }
            UnlockMethod::Discovery { .. } => {
                "Find a hidden secret!".into()
            }
            UnlockMethod::StoryCompletion { chapter } => {
                match chapter {
                    Some(c) => format!("Complete Chapter {}", c),
                    None => "Complete the story".into(),
                }
            }
            UnlockMethod::Collection { items_needed } => {
                format!("Collect {} item(s)", items_needed.len())
            }
        }
    }
}
```

### Game Certification for Child Access

```rust
/// Certification process for games to be child-accessible
pub struct ChildAccessCertification {
    pub game_id: String,
    pub certified: bool,
    pub certification_date: Option<DateTime<Utc>>,
    pub violations: Vec<ChildSafetyViolation>,
    pub requirements_met: ChildGameRequirements,
}

impl ChildAccessCertification {
    /// Certify a game for child access
    pub async fn certify(game_id: &str, db: &PgPool) -> Result<Self, CertificationError> {
        // Fetch game metadata
        let game = fetch_game_metadata(db, game_id).await?;
        
        // Check monetization
        let has_iap = game.has_in_app_purchases;
        let has_lootboxes = game.has_loot_boxes;
        let has_premium_currency = game.has_premium_currency;
        
        // Check progression
        let all_content_unlockable = game.all_content_skill_unlockable;
        let no_pay_advantages = !game.has_paid_advantages;
        
        let requirements = ChildGameRequirements {
            free_to_play: !has_iap || game.iap_cosmetic_only,
            skill_based_progression: all_content_unlockable,
            no_pay_to_win: no_pay_advantages,
            no_predatory_timers: !game.has_energy_systems,
            no_social_pressure: !game.has_social_purchase_prompts,
            transparent_progression: game.shows_unlock_paths,
        };
        
        let validation = requirements.validate();
        
        let cert = Self {
            game_id: game_id.to_string(),
            certified: validation.is_ok(),
            certification_date: if validation.is_ok() { Some(Utc::now()) } else { None },
            violations: validation.err().unwrap_or_default(),
            requirements_met: requirements,
        };
        
        // Store certification
        sqlx::query!(
            "INSERT INTO game_child_certifications (game_id, certified, certified_at, violations) 
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (game_id) DO UPDATE SET certified = $2, certified_at = $3, violations = $4",
            game_id,
            cert.certified,
            cert.certification_date,
            serde_json::to_value(&cert.violations)?
        )
        .execute(db)
        .await?;
        
        Ok(cert)
    }
}

/// Filter games for child access
pub async fn get_child_accessible_games(
    db: &PgPool,
    limit: u32,
    offset: u32,
) -> Result<Vec<GameListing>, DbError> {
    sqlx::query_as!(
        GameListing,
        r#"
        SELECT g.id, g.name, g.description, g.thumbnail, g.genre
        FROM games g
        JOIN game_child_certifications c ON g.id = c.game_id
        WHERE c.certified = true
          AND g.age_rating <= 'E10'
          AND g.published = true
        ORDER BY g.popularity DESC
        LIMIT $1 OFFSET $2
        "#,
        limit as i64,
        offset as i64
    )
    .fetch_all(db)
    .await
}
```

### Age-Appropriate Content Standards

Content accessible to children must meet strict age-appropriateness standards.

```rust
// crates/shared/src/coppa/content_standards.rs

/// Content standards for child-accessible games
#[derive(Debug, Clone)]
pub struct ChildContentStandards {
    /// Violence level (none, cartoon, fantasy)
    pub max_violence: ViolenceLevel,
    
    /// Language restrictions
    pub language_filter: LanguageFilter,
    
    /// Theme restrictions
    pub restricted_themes: Vec<RestrictedTheme>,
    
    /// Required positive themes
    pub required_themes: Vec<PositiveTheme>,
}

impl Default for ChildContentStandards {
    fn default() -> Self {
        Self {
            max_violence: ViolenceLevel::CartoonFantasy,
            language_filter: LanguageFilter::Strict,
            restricted_themes: vec![
                RestrictedTheme::AdultContent,
                RestrictedTheme::ExplicitSexuality,
                RestrictedTheme::GraphicViolence,
                RestrictedTheme::DrugUse,
                RestrictedTheme::Gambling,
                RestrictedTheme::Horror,
                RestrictedTheme::PoliticalActivism,
                RestrictedTheme::ReligiousProselytizing,
                RestrictedTheme::LgbtqContent,
                RestrictedTheme::GenderIdeology,
                RestrictedTheme::ControversialSocialTopics,
            ],
            required_themes: vec![
                PositiveTheme::Creativity,
                PositiveTheme::Teamwork,
                PositiveTheme::ProblemSolving,
                PositiveTheme::FairPlay,
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViolenceLevel {
    None,
    CartoonFantasy,  // Looney Tunes style
    MildConflict,    // Mario stomping goombas
}

#[derive(Debug, Clone)]
pub enum LanguageFilter {
    /// No profanity, slurs, or adult language
    Strict,
    /// Mild expressions allowed (darn, heck)
    Mild,
}

/// Themes restricted from child content
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestrictedTheme {
    /// Explicit sexual content
    AdultContent,
    
    /// Sexual themes of any kind
    ExplicitSexuality,
    
    /// Realistic violence, gore, death
    GraphicViolence,
    
    /// Drug/alcohol use or references
    DrugUse,
    
    /// Gambling or gambling-like mechanics
    Gambling,
    
    /// Horror, jump scares, disturbing imagery
    Horror,
    
    /// Political messaging or activism
    PoliticalActivism,
    
    /// Religious proselytizing or indoctrination
    ReligiousProselytizing,
    
    /// LGBTQ+ content, gender ideology, sexuality discussions
    /// (Not age-appropriate - children should not be exposed to adult sexuality topics)
    LgbtqContent,
    
    /// Gender identity ideology or promotion
    GenderIdeology,
    
    /// Controversial social/cultural topics
    /// (Not age-appropriate for children to process)
    ControversialSocialTopics,
    
    /// Mature relationship themes
    MatureRelationships,
    
    /// Real-world tragedy references
    RealWorldTragedy,
}

/// Positive themes encouraged in child content
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PositiveTheme {
    Creativity,
    Teamwork,
    ProblemSolving,
    FairPlay,
    Friendship,
    Kindness,
    Perseverance,
    Learning,
    Adventure,
    Imagination,
}

/// Content review for child accessibility
pub struct ContentReviewer {
    theme_detector: ThemeDetectionModel,
    language_filter: LanguageFilterModel,
}

impl ContentReviewer {
    /// Review game/content for child accessibility
    pub async fn review(&self, content: &GameContent) -> ContentReviewResult {
        let mut violations = vec![];
        
        // Check for restricted themes
        let detected_themes = self.theme_detector.detect(&content).await;
        for theme in &detected_themes {
            if CHILD_RESTRICTED_THEMES.contains(theme) {
                violations.push(ContentViolation::RestrictedTheme(theme.clone()));
            }
        }
        
        // Check language
        let language_issues = self.language_filter.scan(&content.text_content).await;
        if !language_issues.is_empty() {
            violations.push(ContentViolation::LanguageIssues(language_issues));
        }
        
        // Check violence level
        if content.violence_level > ViolenceLevel::CartoonFantasy {
            violations.push(ContentViolation::ExcessiveViolence);
        }
        
        ContentReviewResult {
            approved: violations.is_empty(),
            violations,
            recommended_rating: self.calculate_rating(&detected_themes),
        }
    }
    
    fn calculate_rating(&self, themes: &[DetectedTheme]) -> AgeRating {
        // ESRB-style rating based on content
        if themes.iter().any(|t| t.is_mature()) {
            AgeRating::Mature
        } else if themes.iter().any(|t| t.is_teen()) {
            AgeRating::Teen
        } else if themes.iter().any(|t| t.is_e10()) {
            AgeRating::E10
        } else {
            AgeRating::Everyone
        }
    }
}

/// Age ratings (ESRB-aligned)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgeRating {
    Everyone,      // E - All ages
    E10,           // E10+ - 10 and older
    Teen,          // T - 13 and older
    Mature,        // M - 17 and older
    AdultsOnly,    // AO - 18+ only
}

impl AgeRating {
    /// Can a child of this age access this rating?
    pub fn accessible_to_age(&self, age: u8) -> bool {
        match self {
            AgeRating::Everyone => true,
            AgeRating::E10 => age >= 10,
            AgeRating::Teen => age >= 13,
            AgeRating::Mature => age >= 17,
            AgeRating::AdultsOnly => age >= 18,
        }
    }
}
```

### Content Moderation for UGC

```rust
/// User-generated content moderation for child safety
pub struct ChildSafeUgcFilter {
    standards: ChildContentStandards,
    reviewer: ContentReviewer,
}

impl ChildSafeUgcFilter {
    /// Filter UGC for child-accessible areas
    pub async fn filter(&self, ugc: &UserGeneratedContent) -> UgcFilterResult {
        // Review content
        let review = self.reviewer.review(&ugc.as_game_content()).await;
        
        if !review.approved {
            return UgcFilterResult::Blocked {
                reason: "Content not appropriate for all ages".into(),
                violations: review.violations,
            };
        }
        
        // Additional UGC-specific checks
        if ugc.contains_external_links() {
            return UgcFilterResult::Blocked {
                reason: "External links not allowed in child areas".into(),
                violations: vec![],
            };
        }
        
        if ugc.contains_contact_info() {
            return UgcFilterResult::Blocked {
                reason: "Contact information not allowed".into(),
                violations: vec![],
            };
        }
        
        UgcFilterResult::Approved {
            rating: review.recommended_rating,
        }
    }
}
```

---

### Classic Unlock Examples

```rust
/// Examples of skill-based unlocks (like games used to be)
pub fn example_unlock_paths() -> Vec<UnlockableContent> {
    vec![
        // Character unlock - beat the game
        UnlockableContent {
            content_id: "char_secret_ninja".into(),
            name: "Shadow Ninja".into(),
            description: "A mysterious warrior from the shadows".into(),
            unlock_method: UnlockMethod::StoryCompletion { chapter: None },
            progress: 0.0,
            unlocked: false,
        },
        
        // Costume unlock - collect items
        UnlockableContent {
            content_id: "costume_gold_armor".into(),
            name: "Golden Armor Set".into(),
            description: "Legendary armor that shines like the sun".into(),
            unlock_method: UnlockMethod::Collection {
                items_needed: vec!["gold_helm", "gold_chest", "gold_boots", "gold_gloves"]
                    .into_iter().map(String::from).collect(),
            },
            progress: 0.0,
            unlocked: false,
        },
        
        // Level unlock - win games
        UnlockableContent {
            content_id: "map_champions_arena".into(),
            name: "Champion's Arena".into(),
            description: "Only the greatest warriors may enter".into(),
            unlock_method: UnlockMethod::Wins { wins_required: 50 },
            progress: 0.0,
            unlocked: false,
        },
        
        // Secret unlock - discovery
        UnlockableContent {
            content_id: "easter_egg_dev_room".into(),
            name: "Developer's Secret Room".into(),
            description: "???".into(),
            unlock_method: UnlockMethod::Discovery {
                secret_id: "hidden_door_level_3".into(),
            },
            progress: 0.0,
            unlocked: false,
        },
        
        // Skill unlock - reach level
        UnlockableContent {
            content_id: "ability_super_jump".into(),
            name: "Super Jump".into(),
            description: "Jump twice as high!".into(),
            unlock_method: UnlockMethod::Level { required_level: 25 },
            progress: 0.0,
            unlocked: false,
        },
    ]
}
```

---

## Parental Consent Flow

### Verifiable Parental Consent (VPC)

FTC-approved methods for obtaining consent:

```rust
// crates/api/src/consent/parental.rs
use secrecy::SecretString;

#[derive(Debug)]
pub enum ConsentMethod {
    /// Email with confirmation link (for limited data collection)
    EmailPlusConfirmation { parent_email: String },
    
    /// Credit card verification ($0.50 charge, immediately refunded)
    CreditCardVerification { 
        last_four: SecretString,
        transaction_id: String,
    },
    
    /// Government ID verification (for full data access)
    GovernmentId { 
        verification_provider: String, // e.g., "id.me"
        verified: bool,
    },
    
    /// Video call verification (for sensitive features)
    VideoCall {
        call_id: String,
        agent_confirmed: bool,
    },
    
    /// Third-party verification service (k-ID, Privo, etc.)
    ThirdPartyService {
        provider: String,
        token: SecretString,
    },
}

#[derive(Debug)]
pub struct ParentalConsentRequest {
    pub child_id: String,           // Hashed, never plain
    pub parent_email: String,
    pub method: ConsentMethod,
    pub permissions_requested: Vec<Permission>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub enum Permission {
    BasicGameplay,          // Always granted
    TextChat,               // Requires consent
    VoiceChat,              // Requires enhanced consent
    UserGeneratedContent,   // Requires consent + moderation
    SocialFeatures,         // Friend lists, etc.
    Analytics,              // Optional
    ThirdPartySharing,      // Prohibited for children
}
```

---

## Age Progression & Automatic Updates

### Age Tracking System

COPPA restrictions automatically update as users age. The system runs checks at multiple trigger points.

```rust
// crates/shared/src/coppa/age_progression.rs
use chrono::{DateTime, Utc, Datelike};
use tokio_cron_scheduler::{Job, JobScheduler};

/// Age progression service - updates COPPA status as children age
pub struct AgeProgressionService {
    db: sqlx::PgPool,
    notification_service: NotificationService,
    permission_service: PermissionService,
}

/// When do we check/update age status?
#[derive(Debug, Clone)]
pub enum AgeCheckTrigger {
    /// Daily batch job at 00:00 UTC
    DailyBatch,
    
    /// On every login
    LoginEvent,
    
    /// When accessing age-restricted features
    FeatureAccess { feature: String },
    
    /// Periodic session check (every 4 hours during active session)
    SessionHeartbeat,
    
    /// Parent/guardian updates profile
    ProfileUpdate,
    
    /// Annual re-verification requirement
    AnnualReVerification,
}

/// Age milestones that change permissions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeMilestone {
    /// Under 13: Full COPPA restrictions
    Under13,
    
    /// 13-15: Reduced restrictions, some features unlocked
    Teen13To15,
    
    /// 16-17: Near-adult, most features available
    Teen16To17,
    
    /// 18+: Full adult access
    Adult,
}

impl AgeProgressionService {
    /// Initialize scheduled jobs for age checking
    pub async fn init_schedulers(&self) -> Result<(), SchedulerError> {
        let scheduler = JobScheduler::new().await?;
        
        // Daily batch job at midnight UTC
        let db = self.db.clone();
        scheduler.add(Job::new_async("0 0 0 * * *", move |_, _| {
            let db = db.clone();
            Box::pin(async move {
                Self::run_daily_age_check(&db).await;
            })
        })?).await?;
        
        scheduler.start().await?;
        Ok(())
    }
    
    /// Daily batch: Check all users with birthdays today
    async fn run_daily_age_check(db: &sqlx::PgPool) -> Result<u64, AgeError> {
        let today = Utc::now().date_naive();
        
        // Find users whose birthday is today
        let birthday_users = sqlx::query!(
            r#"
            SELECT user_id, date_of_birth, current_age_bracket
            FROM user_profiles
            WHERE EXTRACT(MONTH FROM date_of_birth) = $1
              AND EXTRACT(DAY FROM date_of_birth) = $2
            "#,
            today.month() as i32,
            today.day() as i32
        )
        .fetch_all(db)
        .await?;
        
        let mut updated = 0;
        for user in birthday_users {
            let new_age = Self::calculate_age(&user.date_of_birth, &today);
            let new_bracket = AgeMilestone::from_age(new_age);
            let old_bracket = AgeMilestone::from_str(&user.current_age_bracket)?;
            
            if new_bracket != old_bracket {
                Self::transition_age_bracket(db, &user.user_id, old_bracket, new_bracket).await?;
                updated += 1;
            }
        }
        
        tracing::info!("Daily age check: {} users transitioned brackets", updated);
        Ok(updated)
    }
    
    /// Check age on login - fast path
    pub async fn check_on_login(&self, user_id: &str) -> Result<AgeStatus, AgeError> {
        let profile = self.db.get_user_profile(user_id).await?;
        
        let current_age = Self::calculate_age(&profile.date_of_birth, &Utc::now().date_naive());
        let expected_bracket = AgeMilestone::from_age(current_age);
        
        if expected_bracket != profile.current_age_bracket {
            // Age bracket changed since last check
            self.transition_age_bracket(
                &self.db, 
                user_id, 
                profile.current_age_bracket, 
                expected_bracket
            ).await?;
        }
        
        Ok(AgeStatus {
            user_id: user_id.to_string(),
            age: current_age,
            bracket: expected_bracket,
            permissions: self.permission_service.get_for_bracket(expected_bracket).await?,
            next_milestone: expected_bracket.next_milestone_date(&profile.date_of_birth),
        })
    }
    
    /// Handle age bracket transition
    async fn transition_age_bracket(
        db: &sqlx::PgPool,
        user_id: &str,
        from: AgeMilestone,
        to: AgeMilestone,
    ) -> Result<(), AgeError> {
        tracing::info!(
            "User {} transitioning from {:?} to {:?}",
            user_id, from, to
        );
        
        // Update database
        sqlx::query!(
            "UPDATE user_profiles SET current_age_bracket = $1, bracket_updated_at = NOW() WHERE user_id = $2",
            to.to_string(), user_id
        )
        .execute(db)
        .await?;
        
        // Handle specific transitions
        match (from, to) {
            (AgeMilestone::Under13, AgeMilestone::Teen13To15) => {
                // Child turned 13 - unlock teen features
                Self::unlock_teen_features(db, user_id).await?;
                Self::notify_turned_13(user_id).await?;
            }
            (AgeMilestone::Teen16To17, AgeMilestone::Adult) => {
                // User turned 18 - full access, remove parental controls
                Self::remove_parental_restrictions(db, user_id).await?;
                Self::notify_turned_18(user_id).await?;
            }
            _ => {}
        }
        
        // Audit log
        audit_log::record(AuditEvent::AgeTransition {
            user_id: user_id.to_string(),
            from_bracket: from,
            to_bracket: to,
            timestamp: Utc::now(),
        });
        
        Ok(())
    }
    
    /// Unlock features when child turns 13
    async fn unlock_teen_features(db: &sqlx::PgPool, user_id: &str) -> Result<(), AgeError> {
        // Enable features that were locked for under-13
        sqlx::query!(
            r#"
            UPDATE user_permissions SET
                text_chat_enabled = true,
                voice_chat_enabled = COALESCE(parental_voice_consent, false),
                ugc_enabled = COALESCE(parental_ugc_consent, false),
                social_features_enabled = true,
                -- Keep parental consent flags but they're now optional
                coppa_restricted = false
            WHERE user_id = $1
            "#,
            user_id
        )
        .execute(db)
        .await?;
        
        Ok(())
    }
    
    /// Remove all parental restrictions at 18
    async fn remove_parental_restrictions(db: &sqlx::PgPool, user_id: &str) -> Result<(), AgeError> {
        sqlx::query!(
            r#"
            UPDATE user_permissions SET
                parental_controls_active = false,
                parental_email = NULL,
                all_features_unlocked = true,
                age_verified_adult = true
            WHERE user_id = $1
            "#,
            user_id
        )
        .execute(db)
        .await?;
        
        // Optionally purge parental consent records (data minimization)
        sqlx::query!(
            "DELETE FROM parental_consents WHERE child_id = $1",
            user_id
        )
        .execute(db)
        .await?;
        
        Ok(())
    }
}

impl AgeMilestone {
    pub fn from_age(age: u8) -> Self {
        match age {
            0..=12 => AgeMilestone::Under13,
            13..=15 => AgeMilestone::Teen13To15,
            16..=17 => AgeMilestone::Teen16To17,
            _ => AgeMilestone::Adult,
        }
    }
    
    /// When will user reach next milestone?
    pub fn next_milestone_date(&self, dob: &chrono::NaiveDate) -> Option<chrono::NaiveDate> {
        match self {
            AgeMilestone::Under13 => Some(dob.with_year(dob.year() + 13)?),
            AgeMilestone::Teen13To15 => Some(dob.with_year(dob.year() + 16)?),
            AgeMilestone::Teen16To17 => Some(dob.with_year(dob.year() + 18)?),
            AgeMilestone::Adult => None,  // No more milestones
        }
    }
}
```

### Session Heartbeat Check

```rust
/// Periodic age check during active sessions
pub async fn session_heartbeat_check(
    session: &Session,
    age_service: &AgeProgressionService,
) -> Result<(), SessionError> {
    // Only check every 4 hours
    let last_check = session.last_age_check;
    if Utc::now() - last_check < chrono::Duration::hours(4) {
        return Ok(());
    }
    
    // Re-verify age status
    let status = age_service.check_on_login(&session.user_id).await?;
    
    // Update session with current permissions
    session.update_permissions(status.permissions).await?;
    session.last_age_check = Utc::now();
    
    Ok(())
}
```

---

## Criminal Background Verification

### Sex Offender Registry Integration

To protect children, we integrate with sex offender registries during ID verification.

```rust
// crates/shared/src/coppa/background_check.rs

/// Background check service for child safety
pub struct BackgroundCheckService {
    /// National Sex Offender Public Website API
    nsopw_client: NsopwClient,
    
    /// State-level registry APIs
    state_registries: HashMap<String, StateRegistryClient>,
    
    /// International registries (INTERPOL, etc.)
    international_client: Option<InternationalRegistryClient>,
    
    /// Cache for recent checks
    cache: redis::aio::ConnectionManager,
}

/// Background check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundCheckResult {
    pub user_id: String,
    pub check_id: String,
    pub status: BackgroundStatus,
    pub registries_checked: Vec<String>,
    pub matches_found: Vec<RegistryMatch>,
    pub risk_level: RiskLevel,
    pub checked_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackgroundStatus {
    /// No matches found - cleared
    Clear,
    
    /// Potential match - requires manual review
    PotentialMatch,
    
    /// Confirmed match - deny access
    ConfirmedMatch,
    
    /// Check failed - retry required
    CheckFailed { reason: String },
    
    /// Pending verification
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryMatch {
    pub registry: String,
    pub match_type: MatchType,
    pub confidence: f32,
    pub offense_category: Option<OffenseCategory>,
    pub requires_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OffenseCategory {
    /// Crimes against children - BLOCK
    ChildAbuse,
    SexualOffenseMinor,
    ChildPornography,
    Kidnapping,
    
    /// Other sexual offenses - BLOCK from child-accessible areas
    SexualOffenseAdult,
    
    /// Violent crimes - ENHANCED MONITORING
    ViolentCrime,
    
    /// Other - REVIEW
    Other,
}

impl BackgroundCheckService {
    /// Run background check during ID verification
    pub async fn check_user(
        &self,
        user_id: &str,
        id_data: &VerifiedIdData,
    ) -> Result<BackgroundCheckResult, BackgroundError> {
        let check_id = uuid::Uuid::new_v4().to_string();
        
        // Extract search parameters from verified ID
        let search_params = SearchParams {
            full_name: id_data.full_name.clone(),
            date_of_birth: id_data.date_of_birth,
            address: id_data.address.clone(),
            // SSN only if provided and consented
            ssn_last_four: id_data.ssn_last_four.clone(),
        };
        
        // Run parallel checks across registries
        let (nsopw_result, state_results, intl_result) = tokio::join!(
            self.check_nsopw(&search_params),
            self.check_state_registries(&search_params),
            self.check_international(&search_params),
        );
        
        // Aggregate results
        let mut all_matches = vec![];
        let mut registries_checked = vec!["NSOPW".to_string()];
        
        if let Ok(matches) = nsopw_result {
            all_matches.extend(matches);
        }
        
        for (state, result) in state_results {
            registries_checked.push(format!("State:{}", state));
            if let Ok(matches) = result {
                all_matches.extend(matches);
            }
        }
        
        if let Some(client) = &self.international_client {
            registries_checked.push("INTERPOL".to_string());
            if let Ok(matches) = intl_result {
                all_matches.extend(matches);
            }
        }
        
        // Determine status and risk level
        let (status, risk_level) = self.evaluate_matches(&all_matches);
        
        let result = BackgroundCheckResult {
            user_id: user_id.to_string(),
            check_id,
            status,
            registries_checked,
            matches_found: all_matches,
            risk_level,
            checked_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::days(365), // Re-check annually
        };
        
        // Store result
        self.store_result(&result).await?;
        
        // Take immediate action if needed
        if result.status == BackgroundStatus::ConfirmedMatch {
            self.block_user(user_id, &result).await?;
        }
        
        Ok(result)
    }
    
    /// Check National Sex Offender Public Website
    async fn check_nsopw(&self, params: &SearchParams) -> Result<Vec<RegistryMatch>, BackgroundError> {
        // NSOPW API integration
        // https://www.nsopw.gov/
        let response = self.nsopw_client
            .search(NsopwSearchRequest {
                first_name: params.full_name.first.clone(),
                last_name: params.full_name.last.clone(),
                city: params.address.as_ref().map(|a| a.city.clone()),
                state: params.address.as_ref().map(|a| a.state.clone()),
                zip: params.address.as_ref().map(|a| a.zip.clone()),
            })
            .await?;
        
        // Convert to our match format
        Ok(response.results.into_iter().map(|r| RegistryMatch {
            registry: "NSOPW".into(),
            match_type: MatchType::NameAndLocation,
            confidence: r.match_score,
            offense_category: Some(OffenseCategory::SexualOffenseMinor),
            requires_review: r.match_score < 0.95,
        }).collect())
    }
    
    /// Evaluate matches and determine action
    fn evaluate_matches(&self, matches: &[RegistryMatch]) -> (BackgroundStatus, RiskLevel) {
        if matches.is_empty() {
            return (BackgroundStatus::Clear, RiskLevel::Low);
        }
        
        // Any child-related offense = immediate block
        let child_offense = matches.iter().any(|m| {
            matches!(
                m.offense_category,
                Some(OffenseCategory::ChildAbuse) |
                Some(OffenseCategory::SexualOffenseMinor) |
                Some(OffenseCategory::ChildPornography) |
                Some(OffenseCategory::Kidnapping)
            )
        });
        
        if child_offense {
            let high_confidence = matches.iter().any(|m| m.confidence > 0.9);
            if high_confidence {
                return (BackgroundStatus::ConfirmedMatch, RiskLevel::Critical);
            } else {
                return (BackgroundStatus::PotentialMatch, RiskLevel::High);
            }
        }
        
        // Other matches = review
        (BackgroundStatus::PotentialMatch, RiskLevel::Medium)
    }
    
    /// Block user and notify authorities if needed
    async fn block_user(&self, user_id: &str, result: &BackgroundCheckResult) -> Result<(), BackgroundError> {
        // Immediate account suspension
        sqlx::query!(
            "UPDATE users SET status = 'blocked_background_check', blocked_at = NOW() WHERE id = $1",
            user_id
        )
        .execute(&self.db)
        .await?;
        
        // Alert security team
        self.alert_security_team(user_id, result).await?;
        
        // If attempting to access child areas, report to NCMEC
        if result.risk_level == RiskLevel::Critical {
            self.report_to_ncmec(user_id, result).await?;
        }
        
        Ok(())
    }
}
```

### Periodic Re-Verification

```rust
/// Annual background re-check for all users with child interaction privileges
pub async fn annual_recheck_job(service: &BackgroundCheckService, db: &PgPool) {
    let users_due = sqlx::query!(
        r#"
        SELECT user_id, last_background_check
        FROM user_verifications
        WHERE last_background_check < NOW() - INTERVAL '1 year'
          AND has_child_interaction_access = true
        "#
    )
    .fetch_all(db)
    .await
    .unwrap_or_default();
    
    for user in users_due {
        // Silently re-run background check
        if let Err(e) = service.recheck_user(&user.user_id).await {
            tracing::error!("Background recheck failed for {}: {}", user.user_id, e);
        }
    }
}
```

---

## Real-Time AI Video Verification

### Automated Liveness & Identity Verification

No human agents required. Real-time webcam stream with AI verification.

```rust
// crates/shared/src/coppa/video_verification.rs
use tokio::sync::mpsc;
use bytes::Bytes;

/// Real-time video verification service (no human agents)
pub struct VideoVerificationService {
    /// Kafka producer for video frames
    kafka_producer: rdkafka::producer::FutureProducer,
    
    /// ML models for verification
    face_detector: FaceDetectionModel,
    liveness_detector: LivenessModel,
    age_estimator: AgeEstimationModel,
    id_matcher: IdMatchingModel,
    deepfake_detector: DeepfakeDetectionModel,
    
    /// Redis for session state
    redis: redis::aio::ConnectionManager,
}

/// Verification session state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationSession {
    pub session_id: String,
    pub user_id: String,
    pub state: VerificationState,
    pub challenges_completed: Vec<Challenge>,
    pub challenges_remaining: Vec<Challenge>,
    pub id_document: Option<IdDocumentData>,
    pub started_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationState {
    /// Waiting for camera access
    WaitingForCamera,
    
    /// Checking liveness (anti-spoofing)
    LivenessCheck,
    
    /// Verifying face matches ID
    FaceMatching,
    
    /// Age estimation
    AgeEstimation,
    
    /// Random challenge (turn head, blink, etc.)
    Challenge { current: Challenge },
    
    /// Verification complete
    Complete { result: VerificationResult },
    
    /// Failed verification
    Failed { reason: String, can_retry: bool },
}

/// Anti-spoofing challenges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Challenge {
    /// Turn head left
    TurnLeft,
    /// Turn head right  
    TurnRight,
    /// Look up
    LookUp,
    /// Look down
    LookDown,
    /// Blink twice
    BlinkTwice,
    /// Smile
    Smile,
    /// Hold ID next to face
    HoldIdNextToFace,
    /// Read random numbers aloud (with audio verification)
    ReadNumbers { numbers: String },
    /// Show back of ID
    ShowIdBack,
}

impl VideoVerificationService {
    /// Start verification session
    pub async fn start_session(
        &self,
        user_id: &str,
        id_document: IdDocumentData,
    ) -> Result<VerificationSession, VerificationError> {
        let session = VerificationSession {
            session_id: uuid::Uuid::new_v4().to_string(),
            user_id: user_id.to_string(),
            state: VerificationState::WaitingForCamera,
            challenges_completed: vec![],
            challenges_remaining: self.generate_random_challenges(),
            id_document: Some(id_document),
            started_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::minutes(10),
        };
        
        // Store in Redis
        self.redis.set_ex(
            &format!("video_session:{}", session.session_id),
            serde_json::to_string(&session)?,
            600, // 10 minute TTL
        ).await?;
        
        Ok(session)
    }
    
    /// Process incoming video frame (called per frame from Kafka consumer)
    pub async fn process_frame(
        &self,
        session_id: &str,
        frame: VideoFrame,
    ) -> Result<FrameResult, VerificationError> {
        let mut session = self.get_session(session_id).await?;
        
        // Check session expiry
        if Utc::now() > session.expires_at {
            return Err(VerificationError::SessionExpired);
        }
        
        match &session.state {
            VerificationState::WaitingForCamera => {
                // Camera connected, start liveness check
                session.state = VerificationState::LivenessCheck;
                self.save_session(&session).await?;
                Ok(FrameResult::StateChange(session.state.clone()))
            }
            
            VerificationState::LivenessCheck => {
                // Run anti-spoofing checks
                let liveness = self.check_liveness(&frame).await?;
                
                if liveness.is_live {
                    // Passed liveness, move to face matching
                    session.state = VerificationState::FaceMatching;
                    self.save_session(&session).await?;
                    Ok(FrameResult::LivenessConfirmed)
                } else if liveness.is_spoof {
                    // Detected fake (photo, video playback, mask)
                    session.state = VerificationState::Failed {
                        reason: "Spoofing detected".into(),
                        can_retry: false,
                    };
                    self.flag_suspicious_user(&session.user_id, "spoof_attempt").await?;
                    Ok(FrameResult::SpoofDetected)
                } else {
                    Ok(FrameResult::Continue)
                }
            }
            
            VerificationState::FaceMatching => {
                // Compare face to ID photo
                let id_photo = session.id_document.as_ref()
                    .ok_or(VerificationError::MissingIdDocument)?
                    .photo.clone();
                
                let match_result = self.match_face_to_id(&frame, &id_photo).await?;
                
                if match_result.confidence > 0.92 {
                    // Face matches ID, move to age estimation
                    session.state = VerificationState::AgeEstimation;
                    self.save_session(&session).await?;
                    Ok(FrameResult::FaceMatched { confidence: match_result.confidence })
                } else if match_result.confidence < 0.5 {
                    // Clear mismatch
                    session.state = VerificationState::Failed {
                        reason: "Face does not match ID".into(),
                        can_retry: true,
                    };
                    Ok(FrameResult::FaceMismatch)
                } else {
                    // Uncertain - request better positioning
                    Ok(FrameResult::NeedsBetterFrame { suggestion: "Center your face in the frame" })
                }
            }
            
            VerificationState::AgeEstimation => {
                // Estimate age from face
                let age_result = self.estimate_age(&frame).await?;
                
                // Compare to claimed age on ID
                let claimed_age = session.id_document.as_ref()
                    .map(|id| id.calculated_age)
                    .unwrap_or(0);
                
                let age_difference = (age_result.estimated_age as i32 - claimed_age as i32).abs();
                
                if age_difference <= 5 {
                    // Age estimate matches ID (within tolerance)
                    // Move to random challenges
                    if let Some(challenge) = session.challenges_remaining.pop() {
                        session.state = VerificationState::Challenge { current: challenge };
                    } else {
                        session.state = VerificationState::Complete {
                            result: VerificationResult::Verified,
                        };
                    }
                    self.save_session(&session).await?;
                    Ok(FrameResult::AgeVerified { estimated: age_result.estimated_age })
                } else {
                    // Significant age discrepancy - flag for review
                    session.state = VerificationState::Failed {
                        reason: format!(
                            "Age discrepancy: ID shows {}, estimated {}",
                            claimed_age, age_result.estimated_age
                        ),
                        can_retry: false,
                    };
                    self.flag_suspicious_user(&session.user_id, "age_discrepancy").await?;
                    Ok(FrameResult::AgeDiscrepancy)
                }
            }
            
            VerificationState::Challenge { current } => {
                // Check if challenge is completed
                let challenge_result = self.check_challenge(&frame, current).await?;
                
                if challenge_result.completed {
                    session.challenges_completed.push(current.clone());
                    
                    if let Some(next_challenge) = session.challenges_remaining.pop() {
                        session.state = VerificationState::Challenge { current: next_challenge };
                    } else {
                        // All challenges complete!
                        session.state = VerificationState::Complete {
                            result: VerificationResult::Verified,
                        };
                        self.complete_verification(&session).await?;
                    }
                    self.save_session(&session).await?;
                    Ok(FrameResult::ChallengeCompleted)
                } else {
                    Ok(FrameResult::ChallengeInProgress { 
                        progress: challenge_result.progress 
                    })
                }
            }
            
            _ => Ok(FrameResult::Continue),
        }
    }
    
    /// Anti-spoofing liveness detection
    async fn check_liveness(&self, frame: &VideoFrame) -> Result<LivenessResult, VerificationError> {
        // Multi-layer spoof detection:
        
        // 1. Texture analysis (detect printed photos)
        let texture_score = self.liveness_detector.check_texture(frame).await?;
        
        // 2. Depth estimation (detect flat surfaces)
        let depth_score = self.liveness_detector.check_depth(frame).await?;
        
        // 3. Reflection analysis (detect screens)
        let reflection_score = self.liveness_detector.check_reflections(frame).await?;
        
        // 4. Micro-movement analysis (real faces have micro-movements)
        let movement_score = self.liveness_detector.check_micro_movements(frame).await?;
        
        // 5. Deepfake detection
        let deepfake_score = self.deepfake_detector.detect(frame).await?;
        
        let combined_score = (
            texture_score * 0.2 +
            depth_score * 0.2 +
            reflection_score * 0.2 +
            movement_score * 0.2 +
            (1.0 - deepfake_score) * 0.2
        );
        
        Ok(LivenessResult {
            is_live: combined_score > 0.8,
            is_spoof: combined_score < 0.3 || deepfake_score > 0.8,
            confidence: combined_score,
            spoof_type: if deepfake_score > 0.8 {
                Some(SpoofType::Deepfake)
            } else if texture_score < 0.3 {
                Some(SpoofType::PrintedPhoto)
            } else if reflection_score < 0.3 {
                Some(SpoofType::ScreenPlayback)
            } else {
                None
            },
        })
    }
    
    /// Generate random challenges for this session
    fn generate_random_challenges(&self) -> Vec<Challenge> {
        use rand::seq::SliceRandom;
        
        let all_challenges = vec![
            Challenge::TurnLeft,
            Challenge::TurnRight,
            Challenge::LookUp,
            Challenge::LookDown,
            Challenge::BlinkTwice,
            Challenge::Smile,
        ];
        
        let mut rng = rand::thread_rng();
        let mut selected: Vec<_> = all_challenges
            .choose_multiple(&mut rng, 3)
            .cloned()
            .collect();
        
        // Always include ID comparison
        selected.push(Challenge::HoldIdNextToFace);
        
        selected
    }
    
    /// Flag user for suspicious activity
    async fn flag_suspicious_user(&self, user_id: &str, reason: &str) -> Result<(), VerificationError> {
        sqlx::query!(
            "INSERT INTO suspicious_verification_attempts (user_id, reason, timestamp) VALUES ($1, $2, NOW())",
            user_id, reason
        )
        .execute(&self.db)
        .await?;
        
        // If multiple suspicious attempts, escalate
        let attempt_count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM suspicious_verification_attempts WHERE user_id = $1 AND timestamp > NOW() - INTERVAL '24 hours'",
            user_id
        )
        .fetch_one(&self.db)
        .await?
        .unwrap_or(0);
        
        if attempt_count >= 3 {
            // Block account and alert security
            self.block_for_verification_abuse(user_id).await?;
        }
        
        Ok(())
    }
}

/// Kafka stream handler for video frames
pub async fn video_frame_consumer(
    service: VideoVerificationService,
    mut consumer: rdkafka::consumer::StreamConsumer,
) {
    use rdkafka::Message;
    
    loop {
        match consumer.recv().await {
            Ok(msg) => {
                if let Some(payload) = msg.payload() {
                    let frame: VideoFrame = match bincode::deserialize(payload) {
                        Ok(f) => f,
                        Err(_) => continue,
                    };
                    
                    let session_id = msg.key()
                        .and_then(|k| std::str::from_utf8(k).ok())
                        .unwrap_or_default();
                    
                    // Process frame (non-blocking)
                    let result = service.process_frame(session_id, frame).await;
                    
                    // Send result back to client via WebSocket
                    if let Ok(result) = result {
                        service.send_result_to_client(session_id, result).await;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Kafka consumer error: {}", e);
            }
        }
    }
}
```

### WebSocket Client Interface

```rust
/// Client-side WebSocket handler for video verification
pub async fn video_verification_websocket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_video_socket(socket, state, session_id))
}

async fn handle_video_socket(
    mut socket: WebSocket,
    state: AppState,
    session_id: String,
) {
    // Receive video frames from client
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Binary(data) => {
                // Send frame to Kafka for processing
                state.kafka_producer
                    .send(
                        FutureRecord::to("video-verification-frames")
                            .key(&session_id)
                            .payload(&data),
                        Duration::from_secs(5),
                    )
                    .await
                    .ok();
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}
```

### Consent Email Template

```rust
fn generate_consent_email(request: &ParentalConsentRequest) -> Email {
    Email {
        subject: "Parental Consent Required - Eustress Engine",
        body: format!(r#"
Your child has requested to use Eustress Engine features that require parental consent.

Requested Permissions:
{}

To APPROVE, click: {}/consent/approve/{}
To DENY, click: {}/consent/deny/{}

This link expires in 48 hours.

You can manage your child's privacy settings at any time: {}/parental-dashboard

Questions? Contact privacy@simbuilder.com

This notice is required by the Children's Online Privacy Protection Act (COPPA).
        "#,
            format_permissions(&request.permissions_requested),
            BASE_URL, request.token,
            BASE_URL, request.token,
            BASE_URL
        ),
    }
}
```

---

## Parental Features

### Weekly Activity Digest

Every Sunday, parents receive an email summarizing their child's activity for the week.

```rust
// crates/api/src/parental/weekly_digest.rs
use chrono::{DateTime, Utc, Weekday, Datelike};
use tokio_cron_scheduler::{Job, JobScheduler};

/// Weekly parental digest service
pub struct WeeklyDigestService {
    db: sqlx::PgPool,
    email_service: EmailService,
    analytics: AnalyticsService,
}

/// Weekly activity summary for a child
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyDigest {
    pub child_id: String,
    pub child_display_name: String,
    pub week_start: DateTime<Utc>,
    pub week_end: DateTime<Utc>,
    
    // Time spent
    pub time_summary: TimeSummary,
    
    // Games played
    pub games_played: Vec<GameActivity>,
    
    // Studio activity (if applicable)
    pub studio_activity: Option<StudioActivity>,
    
    // Social interactions
    pub social_summary: SocialSummary,
    
    // Safety events (if any)
    pub safety_events: Vec<SafetyEvent>,
    
    // Achievements & milestones
    pub achievements: Vec<Achievement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSummary {
    /// Total time online this week
    pub total_hours: f32,
    
    /// Time playing games
    pub gaming_hours: f32,
    
    /// Time in studio/creator mode
    pub studio_hours: f32,
    
    /// Time in social/chat areas
    pub social_hours: f32,
    
    /// Comparison to last week
    pub vs_last_week: f32,  // percentage change
    
    /// Daily breakdown
    pub daily_breakdown: HashMap<Weekday, f32>,
    
    /// Peak activity time
    pub peak_hour: u8,
    
    /// Sessions count
    pub session_count: u32,
    
    /// Average session length
    pub avg_session_minutes: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameActivity {
    pub game_id: String,
    pub game_name: String,
    pub game_thumbnail: Option<String>,
    pub time_played_hours: f32,
    pub sessions: u32,
    pub age_rating: AgeRating,
    pub genre: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudioActivity {
    /// Time spent in studio
    pub hours: f32,
    
    /// Projects worked on
    pub projects: Vec<ProjectSummary>,
    
    /// Assets created
    pub assets_created: u32,
    
    /// Scripts written (line count)
    pub code_lines_written: u32,
    
    /// Published content
    pub published_items: Vec<PublishedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialSummary {
    /// New friends added
    pub new_friends: u32,
    
    /// Messages sent (if chat enabled)
    pub messages_sent: Option<u32>,
    
    /// Groups/parties joined
    pub groups_joined: u32,
    
    /// Multiplayer sessions
    pub multiplayer_sessions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyEvent {
    pub event_type: SafetyEventType,
    pub timestamp: DateTime<Utc>,
    pub description: String,
    pub action_taken: String,
    pub requires_attention: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SafetyEventType {
    /// Chat filter blocked content
    ChatFiltered,
    /// Reported by another user
    ReportReceived,
    /// Attempted to access restricted content
    RestrictedAccess,
    /// Suspicious contact attempt
    SuspiciousContact,
    /// Time limit exceeded
    TimeLimitExceeded,
}

impl WeeklyDigestService {
    /// Initialize Sunday digest job
    pub async fn init_scheduler(&self) -> Result<(), SchedulerError> {
        let scheduler = JobScheduler::new().await?;
        
        // Every Sunday at 9:00 AM UTC
        let db = self.db.clone();
        let email_service = self.email_service.clone();
        
        scheduler.add(Job::new_async("0 0 9 * * SUN", move |_, _| {
            let db = db.clone();
            let email_service = email_service.clone();
            Box::pin(async move {
                if let Err(e) = Self::send_all_digests(&db, &email_service).await {
                    tracing::error!("Weekly digest job failed: {}", e);
                }
            })
        })?).await?;
        
        scheduler.start().await?;
        Ok(())
    }
    
    /// Send digests to all parents
    async fn send_all_digests(
        db: &sqlx::PgPool,
        email_service: &EmailService,
    ) -> Result<u64, DigestError> {
        let week_end = Utc::now();
        let week_start = week_end - chrono::Duration::days(7);
        
        // Get all children with active parental oversight
        let children = sqlx::query!(
            r#"
            SELECT 
                c.user_id as child_id,
                c.display_name,
                p.parent_email,
                p.digest_preferences
            FROM user_profiles c
            JOIN parental_controls p ON c.user_id = p.child_id
            WHERE p.parental_controls_active = true
              AND p.weekly_digest_enabled = true
              AND p.parent_email IS NOT NULL
            "#
        )
        .fetch_all(db)
        .await?;
        
        let mut sent = 0;
        for child in children {
            // Build digest
            let digest = Self::build_digest(
                db,
                &child.child_id,
                &child.display_name,
                week_start,
                week_end,
            ).await?;
            
            // Generate email
            let email = Self::generate_digest_email(&digest, &child.digest_preferences);
            
            // Send
            if email_service.send(&child.parent_email, email).await.is_ok() {
                sent += 1;
            }
        }
        
        tracing::info!("Weekly digest: sent {} emails", sent);
        Ok(sent)
    }
    
    /// Build digest for a single child
    async fn build_digest(
        db: &sqlx::PgPool,
        child_id: &str,
        display_name: &str,
        week_start: DateTime<Utc>,
        week_end: DateTime<Utc>,
    ) -> Result<WeeklyDigest, DigestError> {
        // Parallel data fetching
        let (time_data, games, studio, social, safety, achievements) = tokio::join!(
            Self::fetch_time_summary(db, child_id, week_start, week_end),
            Self::fetch_games_played(db, child_id, week_start, week_end),
            Self::fetch_studio_activity(db, child_id, week_start, week_end),
            Self::fetch_social_summary(db, child_id, week_start, week_end),
            Self::fetch_safety_events(db, child_id, week_start, week_end),
            Self::fetch_achievements(db, child_id, week_start, week_end),
        );
        
        Ok(WeeklyDigest {
            child_id: child_id.to_string(),
            child_display_name: display_name.to_string(),
            week_start,
            week_end,
            time_summary: time_data?,
            games_played: games?,
            studio_activity: studio?,
            social_summary: social?,
            safety_events: safety?,
            achievements: achievements?,
        })
    }
    
    /// Generate HTML email from digest
    fn generate_digest_email(digest: &WeeklyDigest, prefs: &DigestPreferences) -> Email {
        let subject = format!(
            "📊 {}'s Weekly Activity Report - Eustress Engine",
            digest.child_display_name
        );
        
        let body = format!(r#"
<!DOCTYPE html>
<html>
<head>
    <style>
        body {{ font-family: Arial, sans-serif; max-width: 600px; margin: 0 auto; }}
        .header {{ background: linear-gradient(135deg, #667eea 0%, #764ba2 100%); color: white; padding: 20px; border-radius: 10px 10px 0 0; }}
        .section {{ padding: 20px; border-bottom: 1px solid #eee; }}
        .stat-box {{ display: inline-block; text-align: center; padding: 15px; background: #f8f9fa; border-radius: 8px; margin: 5px; }}
        .stat-number {{ font-size: 24px; font-weight: bold; color: #667eea; }}
        .game-item {{ display: flex; align-items: center; padding: 10px 0; }}
        .game-thumb {{ width: 50px; height: 50px; border-radius: 8px; margin-right: 15px; }}
        .alert {{ background: #fff3cd; border-left: 4px solid #ffc107; padding: 10px; margin: 10px 0; }}
        .achievement {{ background: #d4edda; padding: 10px; border-radius: 8px; margin: 5px 0; }}
    </style>
</head>
<body>
    <div class="header">
        <h1>📊 Weekly Activity Report</h1>
        <p>{}'s activity from {} to {}</p>
    </div>
    
    <div class="section">
        <h2>⏱️ Time Summary</h2>
        <div class="stat-box">
            <div class="stat-number">{:.1}h</div>
            <div>Total Time</div>
        </div>
        <div class="stat-box">
            <div class="stat-number">{:.1}h</div>
            <div>Gaming</div>
        </div>
        <div class="stat-box">
            <div class="stat-number">{:.1}h</div>
            <div>Creating</div>
        </div>
        <div class="stat-box">
            <div class="stat-number">{}</div>
            <div>Sessions</div>
        </div>
        <p>Average session: {:.0} minutes | Peak time: {}:00</p>
        <p>{}</p>
    </div>
    
    <div class="section">
        <h2>🎮 Games Played</h2>
        {}
    </div>
    
    {}
    
    {}
    
    {}
    
    <div class="section" style="text-align: center; background: #f8f9fa;">
        <p><a href="{}/parental-dashboard" style="color: #667eea;">Manage Settings</a> | 
           <a href="{}/parental-dashboard/time-limits" style="color: #667eea;">Set Time Limits</a> |
           <a href="{}/parental-dashboard/unsubscribe" style="color: #667eea;">Unsubscribe</a></p>
        <p style="font-size: 12px; color: #666;">
            This report is sent weekly as part of our COPPA-compliant parental controls.
        </p>
    </div>
</body>
</html>
        "#,
            digest.child_display_name,
            digest.week_start.format("%b %d"),
            digest.week_end.format("%b %d, %Y"),
            digest.time_summary.total_hours,
            digest.time_summary.gaming_hours,
            digest.time_summary.studio_hours,
            digest.time_summary.session_count,
            digest.time_summary.avg_session_minutes,
            digest.time_summary.peak_hour,
            Self::format_vs_last_week(digest.time_summary.vs_last_week),
            Self::format_games_list(&digest.games_played),
            Self::format_studio_section(&digest.studio_activity),
            Self::format_safety_section(&digest.safety_events),
            Self::format_achievements_section(&digest.achievements),
            BASE_URL, BASE_URL, BASE_URL
        );
        
        Email {
            subject,
            body,
            content_type: "text/html".into(),
        }
    }
    
    fn format_vs_last_week(change: f32) -> String {
        if change > 0.0 {
            format!("📈 {}% more than last week", change.abs() as i32)
        } else if change < 0.0 {
            format!("📉 {}% less than last week", change.abs() as i32)
        } else {
            "Same as last week".into()
        }
    }
    
    fn format_games_list(games: &[GameActivity]) -> String {
        if games.is_empty() {
            return "<p>No games played this week.</p>".into();
        }
        
        games.iter().map(|g| format!(
            r#"<div class="game-item">
                <img class="game-thumb" src="{}" alt="{}">
                <div>
                    <strong>{}</strong><br>
                    <span style="color: #666;">{:.1}h played • {} sessions • {}</span>
                </div>
            </div>"#,
            g.game_thumbnail.as_deref().unwrap_or("/default-game.png"),
            g.game_name,
            g.game_name,
            g.time_played_hours,
            g.sessions,
            g.genre
        )).collect::<Vec<_>>().join("\n")
    }
    
    fn format_studio_section(studio: &Option<StudioActivity>) -> String {
        match studio {
            Some(s) if s.hours > 0.0 => format!(
                r#"<div class="section">
                    <h2>🎨 Creator Studio</h2>
                    <p><strong>{:.1} hours</strong> creating</p>
                    <p>{} assets created • {} lines of code written</p>
                    {}
                </div>"#,
                s.hours,
                s.assets_created,
                s.code_lines_written,
                if !s.published_items.is_empty() {
                    format!("<p>🚀 Published {} new items!</p>", s.published_items.len())
                } else {
                    String::new()
                }
            ),
            _ => String::new(),
        }
    }
    
    fn format_safety_section(events: &[SafetyEvent]) -> String {
        if events.is_empty() {
            return r#"<div class="section">
                <h2>🛡️ Safety</h2>
                <p style="color: green;">✅ No safety concerns this week!</p>
            </div>"#.into();
        }
        
        let alerts: String = events.iter().map(|e| format!(
            r#"<div class="alert">
                <strong>{:?}</strong>: {}<br>
                <span style="font-size: 12px;">Action: {}</span>
            </div>"#,
            e.event_type,
            e.description,
            e.action_taken
        )).collect::<Vec<_>>().join("\n");
        
        format!(
            r#"<div class="section">
                <h2>🛡️ Safety Alerts</h2>
                <p>⚠️ {} event(s) this week:</p>
                {}
            </div>"#,
            events.len(),
            alerts
        )
    }
    
    fn format_achievements_section(achievements: &[Achievement]) -> String {
        if achievements.is_empty() {
            return String::new();
        }
        
        let items: String = achievements.iter().map(|a| format!(
            r#"<div class="achievement">🏆 {}</div>"#,
            a.name
        )).collect::<Vec<_>>().join("\n");
        
        format!(
            r#"<div class="section">
                <h2>🏆 Achievements Unlocked</h2>
                {}
            </div>"#,
            items
        )
    }
}
```

### Parental Dashboard API

```rust
// crates/api/src/parental/dashboard.rs

/// Parental control dashboard endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParentalDashboard {
    pub child_id: String,
    pub child_display_name: String,
    pub child_avatar: Option<String>,
    pub account_created: DateTime<Utc>,
    
    // Current settings
    pub settings: ParentalSettings,
    
    // Real-time status
    pub is_online: bool,
    pub current_activity: Option<CurrentActivity>,
    
    // Quick stats
    pub today_playtime: f32,
    pub week_playtime: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParentalSettings {
    /// Daily time limits
    pub time_limits: TimeLimits,
    
    /// Content restrictions
    pub content_restrictions: ContentRestrictions,
    
    /// Social restrictions
    pub social_restrictions: SocialRestrictions,
    
    /// Notification preferences
    pub notifications: NotificationPreferences,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeLimits {
    pub enabled: bool,
    /// Minutes per day, by day of week
    pub daily_limits: HashMap<Weekday, u32>,
    /// Bedtime - no play after this time
    pub bedtime: Option<chrono::NaiveTime>,
    /// Wake time - no play before this time
    pub wake_time: Option<chrono::NaiveTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPreferences {
    pub weekly_digest: bool,
    pub daily_summary: bool,
    pub safety_alerts_immediate: bool,
    pub friend_request_alerts: bool,
    pub spending_alerts: bool,
    pub time_limit_warnings: bool,
}

/// REST API endpoints
pub fn parental_routes() -> Router {
    Router::new()
        .route("/dashboard", get(get_dashboard))
        .route("/settings", get(get_settings).put(update_settings))
        .route("/time-limits", put(update_time_limits))
        .route("/content-restrictions", put(update_content_restrictions))
        .route("/activity/live", get(get_live_activity))
        .route("/activity/history", get(get_activity_history))
        .route("/reports/weekly", get(get_weekly_reports))
        .route("/reports/:week", get(get_specific_report))
        .route("/digest/preview", get(preview_digest))
        .route("/digest/unsubscribe", post(unsubscribe_digest))
        .layer(require_parent_auth())
}
```

### Real-Time Activity Monitoring

```rust
/// Real-time activity WebSocket for parental monitoring
pub async fn parent_activity_websocket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    claims: ParentClaims,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_parent_socket(socket, state, claims))
}

async fn handle_parent_socket(
    mut socket: WebSocket,
    state: AppState,
    claims: ParentClaims,
) {
    // Subscribe to child's activity events
    let mut rx = state.activity_bus.subscribe(&claims.child_id);
    
    while let Some(event) = rx.recv().await {
        let msg = match event {
            ActivityEvent::LoggedIn { game, timestamp } => {
                serde_json::json!({
                    "type": "online",
                    "game": game,
                    "timestamp": timestamp
                })
            }
            ActivityEvent::LoggedOut { session_duration } => {
                serde_json::json!({
                    "type": "offline",
                    "session_minutes": session_duration.num_minutes()
                })
            }
            ActivityEvent::GameChanged { from, to } => {
                serde_json::json!({
                    "type": "game_changed",
                    "from": from,
                    "to": to
                })
            }
            ActivityEvent::TimeLimitWarning { minutes_remaining } => {
                serde_json::json!({
                    "type": "time_warning",
                    "minutes_remaining": minutes_remaining
                })
            }
            ActivityEvent::SafetyAlert(event) => {
                serde_json::json!({
                    "type": "safety_alert",
                    "event": event
                })
            }
        };
        
        if socket.send(Message::Text(msg.to_string())).await.is_err() {
            break;
        }
    }
}
```

---

### k-ID Integration (Recommended)

```rust
// crates/api/src/consent/kid_integration.rs
use reqwest::Client;

pub struct KidVerificationService {
    client: Client,
    api_key: secrecy::SecretString,
}

impl KidVerificationService {
    pub async fn initiate_verification(
        &self,
        child_age: u8,
        permissions: &[Permission],
    ) -> Result<VerificationSession, KidError> {
        // k-ID handles age-appropriate consent flows
        let response = self.client
            .post("https://api.k-id.com/v1/verify")
            .header("Authorization", format!("Bearer {}", self.api_key.expose_secret()))
            .json(&KidRequest {
                age: child_age,
                permissions: permissions.iter().map(|p| p.to_kid_scope()).collect(),
                redirect_uri: format!("{}/consent/callback", BASE_URL),
            })
            .send()
            .await?;
        
        Ok(response.json().await?)
    }
}
```

---

## Data Handling for Minors

### Ephemeral Session Architecture

```rust
// crates/shared/src/child_session.rs
use redis::AsyncCommands;

/// Child sessions are ephemeral - no persistent storage
pub struct ChildSession {
    pub session_id: String,
    pub redis_key: String,
    pub ttl: std::time::Duration,  // Max 24 hours
}

impl ChildSession {
    pub fn new() -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            redis_key: format!("child_session:{}", uuid::Uuid::new_v4()),
            ttl: std::time::Duration::from_secs(86400), // 24 hours max
        }
    }
    
    pub async fn store(&self, redis: &mut redis::aio::Connection, data: &SessionData) -> Result<()> {
        let serialized = serde_json::to_string(data)?;
        redis.set_ex(&self.redis_key, serialized, self.ttl.as_secs() as usize).await?;
        Ok(())
    }
}

/// Data minimization for children
#[derive(Serialize, Deserialize)]
pub struct ChildSessionData {
    // ONLY essential data - no PII
    pub display_name: String,       // System-generated, not user-provided
    pub avatar_preset: u32,         // Preset avatar, no customization
    pub game_progress: GameProgress,
    // NO: email, real name, location, photos, voice recordings
}
```

### Prohibited Data Collection

```rust
/// Data that CANNOT be collected from children without VPC
pub fn is_prohibited_without_consent(data_type: &str) -> bool {
    matches!(data_type,
        "email" |
        "phone" |
        "address" |
        "real_name" |
        "photo" |
        "video" |
        "voice" |
        "geolocation" |
        "persistent_identifier" |
        "behavioral_tracking" |
        "biometric"
    )
}

/// Data collection guard
pub fn guard_child_data_collection<T>(
    data: T,
    child_session: &ChildSession,
    has_consent: bool,
) -> Result<T, CoppaError> {
    if child_session.is_child && !has_consent {
        if is_prohibited_without_consent(std::any::type_name::<T>()) {
            return Err(CoppaError::ConsentRequired);
        }
    }
    Ok(data)
}
```

---

## Eustress Engine Integration

### Safe Entity Spawning

```rust
// crates/engine/src/plugins/safe_spawns.rs
use bevy::prelude::*;

/// Component marking child-safe entities
#[derive(Component)]
pub struct ChildSafe;

/// Component for entities requiring age verification
#[derive(Component)]
pub struct AgeRestricted {
    pub minimum_age: u8,
    pub reason: &'static str,
}

pub struct SafeSpawnPlugin;

impl Plugin for SafeSpawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            filter_unsafe_entities,
            apply_child_safe_defaults,
            enforce_content_restrictions,
        ).chain());
    }
}

/// Filter entities based on user age verification
fn filter_unsafe_entities(
    mut commands: Commands,
    age_verification: Res<AgeVerification>,
    query: Query<(Entity, &AgeRestricted), Without<ChildSafe>>,
) {
    if age_verification.is_child {
        for (entity, restriction) in query.iter() {
            // Hide age-restricted content from children
            commands.entity(entity).insert(Visibility::Hidden);
            
            // Log for audit
            info!(
                "Filtered age-restricted content for child session: {} (min age: {})",
                restriction.reason, restriction.minimum_age
            );
        }
    }
}

/// Apply child-safe defaults to avatars and UGC
fn apply_child_safe_defaults(
    mut commands: Commands,
    age_verification: Res<AgeVerification>,
    query: Query<Entity, (With<Avatar>, Without<ChildSafe>)>,
) {
    if age_verification.is_child || (age_verification.is_teen && !age_verification.parental_consent) {
        for entity in query.iter() {
            commands.entity(entity).insert((
                ChildSafe,
                PresetAvatar::default(),  // No custom avatars
                ChatDisabled,             // No direct messaging
                FriendListDisabled,       // No social features without consent
            ));
        }
    }
}
```

### Generative AI Restrictions

```rust
// crates/engine/src/plugins/ai_safety.rs

/// AI features that require parental consent for minors
#[derive(Component)]
pub struct GenerativeAIFeature {
    pub feature_type: AIFeatureType,
    pub requires_consent: bool,
}

pub enum AIFeatureType {
    TextGeneration,     // Chat with AI NPCs
    ImageGeneration,    // Create custom textures
    VoiceSynthesis,     // AI voices
    BehaviorLearning,   // Adaptive difficulty
}

fn gate_ai_features(
    age_verification: Res<AgeVerification>,
    mut query: Query<&mut GenerativeAIFeature>,
) {
    for mut feature in query.iter_mut() {
        if age_verification.is_child {
            // All generative AI requires consent for <13
            feature.requires_consent = true;
        } else if age_verification.is_teen {
            // Certain AI features require consent for teens
            feature.requires_consent = matches!(
                feature.feature_type,
                AIFeatureType::TextGeneration | AIFeatureType::VoiceSynthesis
            );
        }
    }
}
```

---

## Safe Souls Architecture

### Core Principles

```
┌─────────────────────────────────────────────────────────────────┐
│                     SAFE SOULS FRAMEWORK                        │
├─────────────────────────────────────────────────────────────────┤
│  1. GATE: Age-appropriate access controls                       │
│  2. FILTER: Content moderation for all user inputs              │
│  3. EPHEMERAL: No persistent child data without consent         │
│  4. AUDIT: Every child interaction logged (anonymized)          │
│  5. PARENT: Dashboard for parental oversight                    │
└─────────────────────────────────────────────────────────────────┘
```

### Parental Dashboard

```rust
// crates/api/src/routes/parental.rs
use axum::{extract::State, Json, routing::get};

pub fn parental_router() -> Router<AppState> {
    Router::new()
        .route("/dashboard", get(get_dashboard))
        .route("/activity", get(get_activity_log))
        .route("/settings", get(get_settings).patch(update_settings))
        .route("/revoke", post(revoke_consent))
        .route("/delete", post(delete_child_data))
}

#[derive(Serialize)]
pub struct ParentalDashboard {
    pub child_display_name: String,
    pub active_permissions: Vec<Permission>,
    pub recent_sessions: Vec<SessionSummary>,
    pub content_filters: ContentFilterSettings,
    pub communication_settings: CommunicationSettings,
    pub data_collected: DataSummary,  // What data exists
    pub delete_all_button: bool,      // One-click erasure
}

#[derive(Serialize)]
pub struct ContentFilterSettings {
    pub block_ugc: bool,              // User-generated content
    pub block_chat: bool,             // All chat
    pub block_voice: bool,            // Voice features
    pub safe_search: bool,            // Filter search results
    pub friend_requests: FriendRequestPolicy,
}

pub enum FriendRequestPolicy {
    Disabled,
    FriendsOfFriends,
    ApprovalRequired,  // Parent must approve each request
}
```

---

## Testing & ESRB Certification

### Compliance Test Suite

```rust
#[cfg(test)]
mod coppa_tests {
    use super::*;
    
    #[test]
    fn test_age_gate_neutral() {
        // Verify no leading questions or visual cues
        let ui = render_age_prompt();
        assert!(!ui.contains("Are you over"));
        assert!(!ui.contains("🎂"));  // No birthday icons
        assert!(!ui.contains("adult"));
    }
    
    #[tokio::test]
    async fn test_child_session_ephemeral() {
        let session = ChildSession::new();
        assert!(session.ttl <= std::time::Duration::from_secs(86400));
        
        // Verify data doesn't persist beyond TTL
        let mut redis = test_redis().await;
        session.store(&mut redis, &test_data()).await.unwrap();
        
        tokio::time::sleep(session.ttl + std::time::Duration::from_secs(1)).await;
        
        let result: Option<String> = redis.get(&session.redis_key).await.unwrap();
        assert!(result.is_none());
    }
    
    #[test]
    fn test_prohibited_data_blocked() {
        let child_session = ChildSession::new();
        let result = guard_child_data_collection(
            Email("child@example.com".to_string()),
            &child_session,
            false, // No consent
        );
        assert!(matches!(result, Err(CoppaError::ConsentRequired)));
    }
    
    #[test]
    fn test_parental_consent_methods() {
        // All FTC-approved methods available
        assert!(ConsentMethod::EmailPlusConfirmation { parent_email: "".into() }.is_valid_for_limited_collection());
        assert!(ConsentMethod::CreditCardVerification { last_four: "".into(), transaction_id: "".into() }.is_valid_for_full_collection());
    }
}
```

### ESRB Privacy Certified Program

```yaml
# esrb-certification-checklist.yml
esrb_privacy_certified:
  tier: "Kids"  # For games targeting children
  
  requirements:
    - name: "Neutral Age Screen"
      status: implemented
      evidence: "age_gate.rs - no leading questions"
    
    - name: "Verifiable Parental Consent"
      status: implemented
      evidence: "parental.rs - FTC-approved methods"
    
    - name: "Data Minimization"
      status: implemented
      evidence: "child_session.rs - ephemeral only"
    
    - name: "Parental Controls"
      status: implemented
      evidence: "parental dashboard with revocation"
    
    - name: "No Behavioral Advertising"
      status: implemented
      evidence: "ad_free: true in child sessions"
    
    - name: "Safe Chat"
      status: implemented
      evidence: "filtered/disabled for children"
  
  annual_audit: 
    next_date: "2026-01-15"
    auditor: "ESRB"
```

### FTC Safe Harbor Compliance

```rust
/// Self-regulatory program compliance
pub struct SafeHarborCompliance {
    pub program: SafeHarborProgram,
    pub certification_date: chrono::NaiveDate,
    pub expiration_date: chrono::NaiveDate,
    pub audit_schedule: AuditSchedule,
}

pub enum SafeHarborProgram {
    ESRB,       // Entertainment Software Rating Board
    CARU,       // Children's Advertising Review Unit
    Privo,      // PRIVO Privacy Assurance
    TrustArc,   // TRUSTe/TrustArc
    KidSafe,    // kidSAFE Seal Program
}
```

---

## Fines & Risk Mitigation

### Violation Penalties

| Violation Type | Fine per Violation | Eustress Mitigation |
|----------------|-------------------|---------------------|
| Collection without consent | $50,120 | Age gate + VPC |
| Inadequate consent method | $50,120 | k-ID integration |
| Data retention violations | $50,120 | Ephemeral sessions |
| Missing privacy policy | $50,120 | Auto-generated policy |
| Push notification abuse | $50,120 | Disabled for children |

### Projected Savings

```
Without Compliance: $50K × potential violations = $5M-50M risk
With Safe Souls: $0 fines + ESRB certification + 20% Steam rating boost
```

---

## Related Documentation

- [CCPA.md](./CCPA.md) - California under-16 provisions
- [GDPR.md](./GDPR.md) - EU child data protection (Article 8)
- [AI_AGENTS.md](../moderation/AI_AGENTS.md) - AI safety for children
- [MODERATION_API.md](../moderation/MODERATION_API.md) - Content filtering

---

## Age Check Trigger Summary

| Trigger | Frequency | What It Checks |
|---------|-----------|----------------|
| Daily Batch | 00:00 UTC | All users with birthdays today |
| Login Event | Every login | Current age vs stored bracket |
| Feature Access | On restricted feature use | Permissions for age bracket |
| Session Heartbeat | Every 4 hours | Age status during long sessions |
| Profile Update | On parent/guardian changes | Re-verify consent status |
| Annual Re-Verification | Yearly | Re-confirm identity + background |

---

## Contact Information

**COPPA Compliance Officer:** coppa@simbuilder.com
**Child Safety Contact:** childsafety@simbuilder.com  
**COPPA Inquiries:** coppa@simbuilder.com

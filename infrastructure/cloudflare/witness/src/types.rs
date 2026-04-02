// =============================================================================
// Eustress Witness Worker — Request/Response Types
// =============================================================================

use serde::{Deserialize, Serialize};

// -----------------------------------------------------------------------------
// Generic
// -----------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// -----------------------------------------------------------------------------
// Well-known
// -----------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
pub struct ForkInfo {
    pub fork_id: String,
    pub public_key: String,
    pub chain_id: u32,
    pub bliss_version: String,
    pub identity_schema_version: String,
    pub contact: String,
}

#[derive(Serialize)]
pub struct IdentityInfo {
    pub public_key: String,
    pub fork_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct RevocationList {
    pub issued_by: String,
    pub list_version: u64,
    pub entries: Vec<RevocationEntry>,
    pub signature: String,
}

#[derive(Serialize, Deserialize)]
pub struct RevocationEntry {
    pub user_id: String,
    pub revoked_at: String,
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct RateReport {
    pub fork_id: String,
    pub total_issued: String,
    pub total_contribution_score: f64,
    pub rate: f64,
    pub active_users: u64,
    pub total_bridged_out: String,
    pub total_bridged_in: String,
    pub last_updated: String,
}

// -----------------------------------------------------------------------------
// Cosign
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CosignRequest {
    pub user_id: String,
    pub contribution_hash: String,
    pub timestamp: String,
}

#[derive(Serialize)]
pub struct CosignResponse {
    pub server_signature: String,
    pub co_signed_at: String,
}

// -----------------------------------------------------------------------------
// Register
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub user_id: String,
    pub public_key: String,
}

#[derive(Serialize)]
pub struct RegisterResponse {
    pub registered: bool,
    pub user_id: String,
}

// -----------------------------------------------------------------------------
// Fork Registration
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ForkRegisterRequest {
    pub fork_id: String,
    pub public_key: String,
    pub chain_id: u32,
    pub endpoint: String,
}

#[derive(Serialize)]
pub struct ForkRegisterResponse {
    pub registered: bool,
    pub fork_id: String,
}

// -----------------------------------------------------------------------------
// User record (KV stored)
// -----------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
pub struct UserRecord {
    pub public_key: String,
    pub registered_at: String,
    pub revoked: bool,
}

// -----------------------------------------------------------------------------
// Fork record (KV stored)
// -----------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
pub struct ForkRecord {
    pub fork_id: String,
    pub public_key: String,
    pub chain_id: u32,
    pub endpoint: String,
    pub registered_at: String,
    pub trusted: bool,
}

// -----------------------------------------------------------------------------
// Trust Registry
// -----------------------------------------------------------------------------

#[derive(Serialize)]
pub struct TrustRegistryResponse {
    pub fork_count: usize,
    pub median_rate: f64,
    pub last_updated: String,
    pub entries: Vec<TrustRegistryEntry>,
}

#[derive(Serialize)]
pub struct TrustRegistryEntry {
    #[serde(flatten)]
    pub fork: ForkRecord,
    pub rates: Option<RateReport>,
    pub deviation_pct: f64,
}

// -----------------------------------------------------------------------------
// Rate limiting
// -----------------------------------------------------------------------------

#[derive(Serialize)]
pub struct RateLimitError {
    pub error: String,
    pub limit: u64,
    pub reset_at: String,
}

// -----------------------------------------------------------------------------
// Health
// -----------------------------------------------------------------------------

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub fork_id: String,
}

// -----------------------------------------------------------------------------
// Forks list
// -----------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ForksListResponse {
    pub forks: Vec<ForkRecord>,
}

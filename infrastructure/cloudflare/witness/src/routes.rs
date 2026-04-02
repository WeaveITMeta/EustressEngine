// =============================================================================
// Eustress Witness Worker — Route Handlers
// =============================================================================

use worker::*;
use crate::types::*;
use crate::signing;

// =============================================================================
// Helpers
// =============================================================================

fn env_var(env: &Env, name: &str) -> Result<String> {
    env.var(name)
        .map(|v| v.to_string())
        .map_err(|_| Error::RustError(format!("Missing env var: {}", name)))
}

fn env_secret(env: &Env, name: &str) -> Result<String> {
    env.secret(name)
        .map(|v| v.to_string())
        .map_err(|_| Error::RustError(format!("Missing secret: {}", name)))
}

fn users_kv(env: &Env) -> Result<kv::KvStore> {
    env.kv("USERS")
}

fn rate_limits_kv(env: &Env) -> Result<kv::KvStore> {
    env.kv("RATE_LIMITS")
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn current_hour_key() -> String {
    let now = chrono::Utc::now();
    now.format("%Y-%m-%d-%H").to_string()
}

fn next_hour_iso() -> String {
    let now = chrono::Utc::now();
    let next = now + chrono::Duration::hours(1);
    // Round down to the start of the next hour
    next.format("%Y-%m-%dT%H:00:00Z").to_string()
}

// =============================================================================
// Well-known endpoints
// =============================================================================

pub fn well_known_fork(env: &Env) -> Result<Response> {
    let chain_id_str = env_var(env, "CHAIN_ID")?;
    let chain_id: u32 = chain_id_str.parse().unwrap_or(1);

    crate::json_ok(&ForkInfo {
        fork_id: env_var(env, "FORK_ID")?,
        public_key: env_secret(env, "SERVER_PUBLIC_KEY")?,
        chain_id,
        bliss_version: env_var(env, "BLISS_VERSION")?,
        identity_schema_version: env_var(env, "IDENTITY_SCHEMA_VERSION")?,
        contact: "admin@eustress.dev".to_string(),
    })
}

pub fn well_known_identity(env: &Env) -> Result<Response> {
    crate::json_ok(&IdentityInfo {
        public_key: env_secret(env, "SERVER_PUBLIC_KEY")?,
        fork_id: env_var(env, "FORK_ID")?,
    })
}

pub async fn well_known_revoked(env: &Env) -> Result<Response> {
    let kv = users_kv(env)?;

    let revoked: Option<RevocationList> = kv
        .get("__revocation_list")
        .json()
        .await?;

    let list = revoked.unwrap_or(RevocationList {
        issued_by: env_var(env, "FORK_ID")?,
        list_version: 0,
        entries: vec![],
        signature: String::new(),
    });

    crate::json_ok(&list)
}

pub async fn well_known_rates(env: &Env) -> Result<Response> {
    let kv = users_kv(env)?;

    let rates: Option<RateReport> = kv
        .get("__rate_report")
        .json()
        .await?;

    let report = rates.unwrap_or(RateReport {
        fork_id: env_var(env, "FORK_ID")?,
        total_issued: "0".to_string(),
        total_contribution_score: 0.0,
        rate: 0.0,
        active_users: 0,
        total_bridged_out: "0".to_string(),
        total_bridged_in: "0".to_string(),
        last_updated: now_iso(),
    });

    crate::json_ok(&report)
}

// =============================================================================
// POST /api/cosign
// =============================================================================

pub async fn cosign(mut req: Request, env: &Env) -> Result<Response> {
    let body: CosignRequest = req.json().await.map_err(|_| {
        Error::RustError("Invalid JSON body".to_string())
    })?;

    if body.user_id.is_empty() || body.contribution_hash.is_empty() || body.timestamp.is_empty() {
        return crate::json_error("missing_fields", 400);
    }

    // Look up user in KV
    let kv = users_kv(env)?;
    let user_key = format!("user:{}", body.user_id);

    let user: Option<UserRecord> = kv.get(&user_key).json().await?;
    let user = match user {
        Some(u) => u,
        None => return crate::json_error("unknown_user", 403),
    };

    if user.revoked {
        return crate::json_error("user_revoked", 403);
    }

    // Rate limiting
    let rate_kv = rate_limits_kv(env)?;
    let hour_key = format!("rate:{}:{}", body.user_id, current_hour_key());
    let current_count: u64 = rate_kv
        .get(&hour_key)
        .text()
        .await?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let limit_str = env_var(env, "RATE_LIMIT_PER_HOUR")?;
    let limit: u64 = limit_str.parse().unwrap_or(120);

    if current_count >= limit {
        return crate::json_response(
            &RateLimitError {
                error: "rate_limited".to_string(),
                limit,
                reset_at: next_hour_iso(),
            },
            429,
        );
    }

    // Increment rate counter (TTL: 2 hours)
    rate_kv
        .put(&hour_key, (current_count + 1).to_string())
        .map_err(|e| Error::RustError(e.to_string()))?
        .expiration_ttl(7200)
        .execute()
        .await?;

    // Build canonical payload and sign
    let payload = signing::build_cosign_payload(
        &body.user_id,
        &body.contribution_hash,
        &body.timestamp,
    );

    let signing_key = env_secret(env, "SIGNING_KEY")?;
    let signature = signing::sign_payload(&signing_key, &payload)
        .map_err(|e| Error::RustError(e))?;

    crate::json_ok(&CosignResponse {
        server_signature: signature,
        co_signed_at: now_iso(),
    })
}

// =============================================================================
// POST /api/register
// =============================================================================

pub async fn register(mut req: Request, env: &Env) -> Result<Response> {
    let body: RegisterRequest = req.json().await.map_err(|_| {
        Error::RustError("Invalid JSON body".to_string())
    })?;

    if body.user_id.is_empty() || body.public_key.is_empty() {
        return crate::json_error("missing_fields", 400);
    }

    let kv = users_kv(env)?;
    let user_key = format!("user:{}", body.user_id);

    // Check if user already exists
    let existing: Option<UserRecord> = kv.get(&user_key).json().await?;
    if existing.is_some() {
        return crate::json_error("user_exists", 409);
    }

    // Store user
    let record = UserRecord {
        public_key: body.public_key,
        registered_at: now_iso(),
        revoked: false,
    };

    let json_str = serde_json::to_string(&record)
        .map_err(|e| Error::RustError(e.to_string()))?;

    kv.put(&user_key, json_str)
        .map_err(|e| Error::RustError(e.to_string()))?
        .execute()
        .await?;

    crate::json_ok(&RegisterResponse {
        registered: true,
        user_id: body.user_id,
    })
}

// =============================================================================
// POST /api/fork/register
// =============================================================================

pub async fn fork_register(mut req: Request, env: &Env) -> Result<Response> {
    let body: ForkRegisterRequest = req.json().await.map_err(|_| {
        Error::RustError("Invalid JSON body".to_string())
    })?;

    if body.fork_id.is_empty()
        || body.public_key.is_empty()
        || body.endpoint.is_empty()
    {
        return crate::json_error("missing_fields", 400);
    }

    // Verify the fork's well-known endpoint is live
    let well_known_url = format!("{}/.well-known/eustress-fork", body.endpoint);
    let fetch_req = Request::new(&well_known_url, Method::Get)?;

    match Fetch::Request(fetch_req).send().await {
        Ok(mut resp) => {
            if resp.status_code() != 200 {
                return crate::json_error("endpoint_unreachable", 502);
            }
            // Verify the fork info matches
            if let Ok(fork_info) = resp.json::<ForkInfo>().await {
                if fork_info.fork_id != body.fork_id || fork_info.public_key != body.public_key {
                    return crate::json_error("endpoint_mismatch", 400);
                }
            } else {
                return crate::json_error("endpoint_invalid_response", 502);
            }
        }
        Err(_) => {
            return crate::json_error("endpoint_unreachable", 502);
        }
    }

    // Store fork in KV
    let kv = users_kv(env)?;
    let fork_key = format!("fork:{}", body.fork_id);

    let record = ForkRecord {
        fork_id: body.fork_id.clone(),
        public_key: body.public_key,
        chain_id: body.chain_id,
        endpoint: body.endpoint,
        registered_at: now_iso(),
        trusted: false,
    };

    let json_str = serde_json::to_string(&record)
        .map_err(|e| Error::RustError(e.to_string()))?;

    kv.put(&fork_key, json_str)
        .map_err(|e| Error::RustError(e.to_string()))?
        .execute()
        .await?;

    crate::json_ok(&ForkRegisterResponse {
        registered: true,
        fork_id: body.fork_id,
    })
}

// =============================================================================
// GET /api/forks
// =============================================================================

pub async fn list_forks(env: &Env) -> Result<Response> {
    let kv = users_kv(env)?;
    let list = kv.list().prefix("fork:".to_string()).execute().await?;

    let mut forks = Vec::new();
    for key in &list.keys {
        if let Some(fork) = kv.get(&key.name).json::<ForkRecord>().await? {
            forks.push(fork);
        }
    }

    crate::json_ok(&ForksListResponse { forks })
}

// =============================================================================
// GET /api/trust-registry
// =============================================================================

pub async fn trust_registry(env: &Env) -> Result<Response> {
    let kv = users_kv(env)?;
    let list = kv.list().prefix("fork:".to_string()).execute().await?;

    let mut forks_with_rates: Vec<(ForkRecord, Option<RateReport>)> = Vec::new();

    for key in &list.keys {
        let fork: Option<ForkRecord> = kv.get(&key.name).json().await?;
        let fork = match fork {
            Some(f) => f,
            None => continue,
        };

        // Try to fetch the fork's rate data
        let rates_url = format!("{}/.well-known/eustress-rates", fork.endpoint);
        let rates = match Request::new(&rates_url, Method::Get) {
            Ok(rates_req) => {
                match Fetch::Request(rates_req).send().await {
                    Ok(mut resp) if resp.status_code() == 200 => {
                        resp.json::<RateReport>().await.ok()
                    }
                    _ => None,
                }
            }
            Err(_) => None,
        };

        forks_with_rates.push((fork, rates));
    }

    // Compute median rate
    let mut rates_values: Vec<f64> = forks_with_rates
        .iter()
        .filter_map(|(_, rates)| rates.as_ref().map(|r| r.rate))
        .filter(|r| *r > 0.0)
        .collect();
    rates_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median_rate = if rates_values.is_empty() {
        0.0
    } else {
        rates_values[rates_values.len() / 2]
    };

    // Build entries with deviation
    let entries: Vec<TrustRegistryEntry> = forks_with_rates
        .into_iter()
        .map(|(fork, rates)| {
            let deviation_pct = if let Some(ref r) = rates {
                if median_rate > 0.0 {
                    ((r.rate - median_rate) / median_rate) * 100.0
                } else {
                    0.0
                }
            } else {
                0.0
            };

            TrustRegistryEntry {
                fork,
                rates,
                deviation_pct,
            }
        })
        .collect();

    crate::json_ok(&TrustRegistryResponse {
        fork_count: entries.len(),
        median_rate,
        last_updated: now_iso(),
        entries,
    })
}

// =============================================================================
// GET /health
// =============================================================================

pub fn health(env: &Env) -> Result<Response> {
    crate::json_ok(&HealthResponse {
        status: "healthy".to_string(),
        fork_id: env_var(env, "FORK_ID").unwrap_or_else(|_| "unknown".to_string()),
    })
}

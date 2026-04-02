// =============================================================================
// Eustress Witness Worker — Rust/WASM Cloudflare Worker
// =============================================================================
// Independent co-signing witness for local-first earning.
// The user never touches the server key. The Worker never touches user content.
//
// Endpoints:
//   GET  /.well-known/eustress-fork      — Fork metadata
//   GET  /.well-known/eustress-identity   — Server public key
//   GET  /.well-known/eustress-revoked    — Revocation list
//   GET  /.well-known/eustress-rates      — Rate reporting
//   POST /api/cosign                      — Co-sign a contribution hash
//   POST /api/register                    — Register a new user
//   POST /api/fork/register               — Register an external fork
//   GET  /api/forks                       — List registered forks
//   GET  /api/trust-registry              — Public Online Trust Registry
//   GET  /health                          — Health check
// =============================================================================

use worker::*;
use serde::Serialize;

mod types;
mod routes;
mod signing;

use types::*;

// =============================================================================
// JSON helpers
// =============================================================================

fn json_response(body: &impl Serialize, status: u16) -> Result<Response> {
    let json = serde_json::to_string(body).map_err(|e| Error::RustError(e.to_string()))?;
    let mut resp = Response::ok(json)?;
    resp = resp.with_status(status);
    let headers = resp.headers_mut();
    headers.set("Content-Type", "application/json")?;
    headers.set("Access-Control-Allow-Origin", "*")?;
    headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS")?;
    headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization")?;
    Ok(resp)
}

fn json_ok(body: &impl Serialize) -> Result<Response> {
    json_response(body, 200)
}

fn json_error(error: &str, status: u16) -> Result<Response> {
    json_response(&ErrorResponse { error: error.to_string() }, status)
}

// =============================================================================
// Router
// =============================================================================

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // CORS preflight
    if req.method() == Method::Options {
        let mut resp = Response::empty()?.with_status(204);
        let headers = resp.headers_mut();
        headers.set("Access-Control-Allow-Origin", "*")?;
        headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS")?;
        headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization")?;
        return Ok(resp);
    }

    let path = req.path();
    let method = req.method();

    let result = match (method, path.as_str()) {
        // Well-known endpoints (public, read-only)
        (Method::Get, "/.well-known/eustress-fork") => routes::well_known_fork(&env),
        (Method::Get, "/.well-known/eustress-identity") => routes::well_known_identity(&env),
        (Method::Get, "/.well-known/eustress-revoked") => routes::well_known_revoked(&env).await,
        (Method::Get, "/.well-known/eustress-rates") => routes::well_known_rates(&env).await,

        // API endpoints
        (Method::Post, "/api/cosign") => routes::cosign(req, &env).await,
        (Method::Post, "/api/register") => routes::register(req, &env).await,
        (Method::Post, "/api/fork/register") => routes::fork_register(req, &env).await,
        (Method::Get, "/api/forks") => routes::list_forks(&env).await,
        (Method::Get, "/api/trust-registry") => routes::trust_registry(&env).await,

        // Health
        (Method::Get, "/health") => routes::health(&env),

        _ => json_error("not_found", 404),
    };

    match result {
        Ok(resp) => Ok(resp),
        Err(e) => json_response(
            &ErrorResponse { error: format!("internal_error: {}", e) },
            500,
        ),
    }
}

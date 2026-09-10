//! # Model catalog fetch and cache
//!
//! Keeps [`super::workshop_model`]'s list current without shipping a build.
//! The worker at `api.eustress.dev` recompiles the catalog nightly (Grok 4.6
//! with live search, validated against a provider whitelist); this module is
//! the client half.
//!
//! ## Read at startup, refresh in the background
//!
//! [`install_and_refresh`] does two things in that order, and the order is the
//! design:
//!
//! 1. Install whatever is already in the on-disk cache, synchronously, before
//!    anything resolves a model.
//! 2. Spawn a thread that fetches a fresh copy and writes it to that cache for
//!    **the next launch**.
//!
//! So a refresh never changes the model list, or a price, under a running
//! session — a picker that reshuffles mid-turn is a bug, not freshness — and
//! startup never blocks on the network. The cost is that a brand new model is
//! one relaunch away rather than instant, which is the right trade for a list
//! that changes at most daily.
//!
//! Nothing here is load-bearing: no cache, no network, or a malformed
//! response all land on the compiled-in seed with a warning. Set
//! `EUSTRESS_MODEL_CATALOG_URL` to point at a local worker (`wrangler dev`)
//! instead of production.

use std::path::PathBuf;
use std::time::Duration;

use super::workshop_model::{install_catalog, Catalog};

/// Where the nightly catalog is published. Public and unauthenticated: it is
/// public model names at public list prices.
const CATALOG_URL: &str = "https://api.eustress.dev/api/models/catalog";

/// Short on purpose. This runs on every launch and its failure mode is
/// "yesterday's list", so waiting is never worth it.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Refuse a response that could not plausibly be a catalog before it is
/// parsed, so a captive-portal login page or an error blob never reaches the
/// cache file.
const MAX_CATALOG_BYTES: usize = 256 * 1024;

/// `~/.eustress_engine/model_catalog.json`, alongside `soul_settings.json`.
pub fn cache_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".eustress_engine").join("model_catalog.json"))
}

fn catalog_url() -> String {
    std::env::var("EUSTRESS_MODEL_CATALOG_URL").unwrap_or_else(|_| CATALOG_URL.to_string())
}

/// Install the cached catalog, then refresh it in the background for the next
/// launch. Call once, early in startup, before any model is resolved.
pub fn install_and_refresh() {
    if let Some(catalog) = load_cached() {
        // `install_catalog` runs its own usability check and keeps the seed if
        // this one cannot serve a picker, so a corrupt cache is inert.
        install_catalog(catalog);
    }
    spawn_refresh();
}

/// Read and parse the cache file. `None` for absent, unreadable or malformed.
fn load_cached() -> Option<Catalog> {
    let path = cache_path()?;
    if !path.exists() {
        return None;
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Catalog>(&text) {
            Ok(catalog) => Some(catalog),
            Err(e) => {
                // Left in place rather than deleted: the next successful fetch
                // overwrites it, and keeping it makes the failure diagnosable.
                tracing::warn!("Workshop: cached model catalog is malformed ({e}); using the built-in list");
                None
            }
        },
        Err(e) => {
            tracing::warn!("Workshop: could not read the cached model catalog ({e}); using the built-in list");
            None
        }
    }
}

/// Fetch the published catalog on a background thread and cache it.
///
/// Deliberately does NOT install what it fetches — see the module docs. Every
/// failure is a warning and nothing else; this is a nicety, and a machine with
/// no network must not pay for it in log noise or startup time.
fn spawn_refresh() {
    std::thread::spawn(|| {
        let url = catalog_url();
        let response = match ureq::get(&url).timeout(FETCH_TIMEOUT).call() {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("Workshop: model catalog refresh skipped ({e})");
                return;
            }
        };

        let body = match response.into_string() {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!("Workshop: model catalog response unreadable ({e})");
                return;
            }
        };
        if body.len() > MAX_CATALOG_BYTES {
            tracing::warn!(
                "Workshop: model catalog response was {} bytes, over the {MAX_CATALOG_BYTES} cap; ignoring",
                body.len()
            );
            return;
        }

        // Parse before writing. A cache file is only worth keeping if the next
        // launch can actually read it, and validating here means a bad
        // response is discarded now rather than discovered at startup.
        let catalog: Catalog = match serde_json::from_str(&body) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Workshop: published model catalog did not parse ({e}); keeping the cached copy");
                return;
            }
        };

        let Some(path) = cache_path() else { return };
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!("Workshop: could not create {}: {e}", parent.display());
                return;
            }
        }

        // Write-then-rename. A half-written catalog is exactly the corrupt
        // cache the parse check above exists to avoid, and an interrupted
        // write is the likeliest way to produce one.
        let tmp = path.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp, &body) {
            tracing::warn!("Workshop: could not write the model catalog cache ({e})");
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &path) {
            tracing::warn!("Workshop: could not replace the model catalog cache ({e})");
            let _ = std::fs::remove_file(&tmp);
            return;
        }

        tracing::info!(
            "Workshop: model catalog v{} cached ({} models); it applies at the next launch",
            catalog.version,
            catalog.models.len()
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_catalog_shape_round_trips() {
        // The exact document the worker publishes, pinned here so a change to
        // either side that the other cannot read fails a test rather than a
        // user's picker.
        let published = r#"{
            "schema": 1,
            "version": 7,
            "updated_at": "2026-09-07T00:00:00Z",
            "source": "grok-4.6",
            "default_model": "claude-sonnet-5",
            "advisor_model": "claude-fable-5-1",
            "pinned_kyc_model": "grok-4.6",
            "aliases": { "claude-fable-5": "claude-fable-5-1" },
            "models": [
                {
                    "id": "claude-sonnet-5",
                    "display_name": "Sonnet 5",
                    "provider": "anthropic",
                    "tagline": "Balanced speed and depth.",
                    "input_price_per_mtok": 3.0,
                    "output_price_per_mtok": 15.0,
                    "max_tokens": 16384,
                    "timeout_secs": 180,
                    "vision": true
                },
                {
                    "id": "gpt-6-astra",
                    "display_name": "GPT-6 Astra",
                    "provider": "openai",
                    "tagline": "OpenAI flagship.",
                    "input_price_per_mtok": 10.0,
                    "output_price_per_mtok": 50.0,
                    "max_tokens": 32000,
                    "timeout_secs": 300,
                    "vision": true
                }
            ]
        }"#;

        let catalog: Catalog = serde_json::from_str(published).expect("published shape must parse");
        assert_eq!(catalog.version, 7);
        assert_eq!(catalog.models.len(), 2);
        assert_eq!(catalog.advisor_model, "claude-fable-5-1");
        assert_eq!(
            catalog.aliases.get("claude-fable-5").map(String::as_str),
            Some("claude-fable-5-1")
        );
        // `pinned_kyc_model` is deliberately not a field on the engine side:
        // an unknown key must not break the parse, or adding one server-side
        // would brick every older engine.
        assert_eq!(
            catalog.models[1].provider,
            super::super::workshop_model::Provider::OpenAi
        );
    }

    #[test]
    fn unknown_provider_fails_the_parse_rather_than_defaulting() {
        // The whitelist is the guardrail. A vendor the engine cannot route to
        // must not silently deserialize into one it can.
        let hostile = r#"{
            "schema": 1, "version": 1, "updated_at": "", "source": "",
            "default_model": "x", "advisor_model": "x", "aliases": {},
            "models": [{
                "id": "x", "display_name": "X", "provider": "acme-labs",
                "input_price_per_mtok": 1.0, "output_price_per_mtok": 1.0,
                "max_tokens": 1024, "timeout_secs": 60
            }]
        }"#;
        assert!(
            serde_json::from_str::<Catalog>(hostile).is_err(),
            "an off-whitelist provider must fail the parse"
        );
    }
}

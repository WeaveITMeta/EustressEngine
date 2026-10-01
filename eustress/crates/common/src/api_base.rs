//! # Where the Eustress API is
//!
//! Every call Studio and the Player make to the Eustress API goes through
//! [`api_base`], so one setting points them all somewhere else:
//! `EUSTRESS_API_URL`, for a local `wrangler dev` or a staging Worker. Only
//! `https://`, or `http://` to this machine, is taken; anything else is
//! refused with a warning and production stays in use, so a typo never sends
//! a login in the clear.
//!
//! A login belongs to the API that issued it. [`credential_scope`] names the
//! API in use, so whatever keeps a token (Studio's saved login) keeps one per
//! API and never offers a production token to a test API.

use std::sync::OnceLock;

/// The production API.
pub const PRODUCTION_API: &str = "https://api.eustress.dev";
/// The variable that points every call somewhere else.
pub const API_URL_VAR: &str = "EUSTRESS_API_URL";

/// The API every call goes to: `EUSTRESS_API_URL` when it holds an accepted
/// base ([`validate_api_base`]), else production. Read once; a base other
/// than production is logged as a warning the first time.
pub fn api_base() -> &'static str {
    static BASE: OnceLock<String> = OnceLock::new();
    BASE.get_or_init(|| resolve(std::env::var(API_URL_VAR).ok().as_deref()))
}

/// `path` (starting with `/`) on the API in use.
pub fn api_url(path: &str) -> String {
    format!("{}{path}", api_base())
}

/// True when calls go to production.
pub fn is_production() -> bool {
    api_base() == PRODUCTION_API
}

/// `None` for production; for any other API, a short name of it (16 hex
/// digits of a hash of the base) to keep its credentials apart under.
pub fn credential_scope() -> Option<String> {
    (!is_production()).then(|| scope_of(api_base()))
}

fn scope_of(base: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in base.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Check a base: `https://host[:port]`, or `http://localhost[:port]` and
/// `http://127.0.0.1[:port]`, with no path, query or user after the host (a
/// trailing `/` is dropped). Returns the base as calls use it.
pub fn validate_api_base(raw: &str) -> Result<String, String> {
    let base = raw.trim().trim_end_matches('/');
    let Some((scheme, authority)) = base.split_once("://") else {
        return Err(format!("{base:?} has no scheme; use https://"));
    };
    if authority.is_empty() || authority.contains(['/', '?', '#', '@', '[', ']']) || authority.contains(char::is_whitespace) {
        return Err(format!("{base:?} must be a scheme and a host only, with no path, query or user"));
    }
    let host = match authority.split_once(':') {
        Some((host, port)) => {
            if !port.parse::<u16>().is_ok_and(|p| p > 0) {
                return Err(format!("{base:?} has an invalid port"));
            }
            host
        }
        None => authority,
    };
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') {
        return Err(format!("{base:?} has an invalid host"));
    }
    let host = host.to_ascii_lowercase();
    match scheme.to_ascii_lowercase().as_str() {
        "https" => Ok(base.to_string()),
        "http" if host == "localhost" || host == "127.0.0.1" => Ok(base.to_string()),
        "http" => Err(format!(
            "{base:?} is plain http to another machine; use https, or http to localhost or 127.0.0.1"
        )),
        other => Err(format!("{base:?} uses {other}; use https")),
    }
}

fn resolve(var: Option<&str>) -> String {
    let Some(raw) = var.map(str::trim).filter(|v| !v.is_empty()) else {
        return PRODUCTION_API.to_string();
    };
    match validate_api_base(raw) {
        Ok(base) if base == PRODUCTION_API => base,
        Ok(base) => {
            tracing::warn!(
                "Eustress API: {base} ({API_URL_VAR}), NOT production. Logins and saved tokens are kept apart from production's."
            );
            base
        }
        Err(e) => {
            tracing::warn!("{API_URL_VAR} ignored: {e}. Using {PRODUCTION_API}.");
            PRODUCTION_API.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only https, or plain http to this machine, is taken, and a refused
    /// value leaves production in use.
    #[test]
    fn only_https_or_http_to_this_machine_is_taken() {
        assert_eq!(validate_api_base("https://api.eustress.dev/").unwrap(), PRODUCTION_API);
        assert_eq!(validate_api_base(" http://localhost:8787 ").unwrap(), "http://localhost:8787");
        assert_eq!(validate_api_base("http://127.0.0.1:8787").unwrap(), "http://127.0.0.1:8787");
        assert_eq!(validate_api_base("HTTP://LocalHost:8787").unwrap(), "HTTP://LocalHost:8787");
        assert!(validate_api_base("https://staging.eustress.dev").is_ok());
        for bad in [
            "http://192.168.1.5:8787",
            "http://api.eustress.dev",
            "http://localhost.evil.example",
            "ftp://example.com",
            "api.eustress.dev",
            "https://",
            "https://host/path",
            "https://user@host",
            "https://host?x=1",
            "https://host#x",
            "https://host:99999",
            "https://host:0",
            "https://host:",
            "https://ho st",
            "https://[::1]",
        ] {
            assert!(validate_api_base(bad).is_err(), "{bad} was taken");
        }
        assert_eq!(resolve(None), PRODUCTION_API);
        assert_eq!(resolve(Some("  ")), PRODUCTION_API);
        assert_eq!(resolve(Some("http://10.0.0.1")), PRODUCTION_API, "a refused value leaves production in use");
        assert_eq!(resolve(Some("http://localhost:8787/")), "http://localhost:8787");
        assert_ne!(scope_of("http://localhost:8787"), scope_of("http://127.0.0.1:8787"));
        assert_eq!(scope_of("http://localhost:8787").len(), 16);
    }
}

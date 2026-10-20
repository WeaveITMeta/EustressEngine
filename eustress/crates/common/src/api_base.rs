//! # Where the Eustress API is
//!
//! Every call Studio and the Player make to the Eustress API goes through
//! [`api_base`], so one setting points them all somewhere else. A running
//! program uses `EUSTRESS_API_URL`. When that is unset, a build may carry
//! `EUSTRESS_DEFAULT_API_URL` from compile time (the staging Player). When
//! both are absent, production is used. Only `https://`, or `http://` to
//! this machine, is taken; anything else is refused with a warning and
//! production stays in use, so a typo never sends a login in the clear.
//!
//! A login belongs to the API that issued it. [`credential_scope`] names the
//! API in use, and [`login_file`] the saved login's file for it, so Studio
//! saves one login per API, the Player reads the one for its API, and neither
//! offers a production token to a test API.

use std::sync::OnceLock;

/// The production API.
pub const PRODUCTION_API: &str = "https://api.eustress.dev";
/// The staging API.
pub const STAGING_API: &str = "https://api-staging.eustress.dev";
/// The variable that points every call somewhere else.
pub const API_URL_VAR: &str = "EUSTRESS_API_URL";

/// The APIs a Player knows by name, for a link that names the API it belongs
/// to (`docs/networking/PLAYER_JOIN_TICKET.md`): `production` and `staging`,
/// and no other.
pub fn api_for_name(name: &str) -> Option<&'static str> {
    match name {
        "production" => Some(PRODUCTION_API),
        "staging" => Some(STAGING_API),
        _ => None,
    }
}

/// What a person calls the API at `base`: `production`, `staging`, `the
/// local server` or `the test server`.
pub fn server_name(base: &str) -> String {
    label_for(base).map_or_else(|| "production".to_string(), |(name, _)| name)
}

/// Compiled in when the build set `EUSTRESS_DEFAULT_API_URL`. A production
/// build leaves the variable unset, so this is `None` and [`api_base`] stays
/// on [`PRODUCTION_API`]. An empty value is treated as unset.
const COMPILED_DEFAULT_API: Option<&str> = option_env!("EUSTRESS_DEFAULT_API_URL");

/// The API every call goes to. [`API_URL_VAR`] when it holds an accepted
/// base ([`validate_api_base`]), else the base compiled in from
/// `EUSTRESS_DEFAULT_API_URL` when that holds one, else production. The
/// runtime variable wins, including when it is refused (production stays in
/// use). Read once; a base other than production is logged as a warning the
/// first time.
pub fn api_base() -> &'static str {
    static BASE: OnceLock<String> = OnceLock::new();
    BASE.get_or_init(|| {
        resolve(std::env::var(API_URL_VAR).ok().as_deref(), COMPILED_DEFAULT_API)
    })
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

/// The first line of the login message ([`login_message`]); the API's
/// `verify-challenge` takes it as `format`.
pub const LOGIN_FORMAT: &str = "eustress-login-v1";

/// The API in use as a web origin, exactly as the API itself computes it
/// (the WHATWG URL origin, through the same `url` rules): scheme and host in
/// lower case, the default port dropped. A login signature names it.
pub fn api_origin() -> String {
    origin_of(api_base())
}

/// `base`'s origin; a base that does not parse is its own text, lower case,
/// which no API's origin equals, so a login signed for it fails closed.
pub fn origin_of(base: &str) -> String {
    match url::Url::parse(base.trim()) {
        Ok(url) => url.origin().ascii_serialization(),
        Err(_) => base.trim().to_ascii_lowercase(),
    }
}

/// The production website.
pub const PRODUCTION_SITE: &str = "https://eustress.dev";
/// The variable that names the website going with a test API.
pub const SITE_URL_VAR: &str = "EUSTRESS_SITE_URL";

/// The website each known API serves: where its accounts and listings have
/// their pages.
const KNOWN_SITES: &[(&str, &str)] = &[
    (PRODUCTION_API, PRODUCTION_SITE),
    (STAGING_API, "https://staging.eustress-cbb.pages.dev"),
];

/// The website that goes with the API in use, for pages about the signed-in
/// account and its listings: `EUSTRESS_SITE_URL` when it holds a base the API
/// rules accept ([`validate_api_base`]), else the known site of the API in
/// use. `None` for an API with no known site, so Studio never opens a page
/// about a listing on a site that does not hold it.
pub fn site_base() -> Option<String> {
    site_for(std::env::var(SITE_URL_VAR).ok().as_deref(), &api_origin())
}

fn site_for(var: Option<&str>, api_origin: &str) -> Option<String> {
    if let Some(raw) = var.map(str::trim).filter(|v| !v.is_empty()) {
        match validate_api_base(raw) {
            Ok(site) => return Some(site),
            Err(e) => tracing::warn!("{SITE_URL_VAR} ignored: {e}"),
        }
    }
    KNOWN_SITES.iter().find(|(api, _)| *api == api_origin).map(|(_, site)| site.to_string())
}

/// The names people know the test APIs by.
const KNOWN_NAMES: &[(&str, &str)] = &[(STAGING_API, "staging")];

/// What a person calls the API in use, where Studio says which server it
/// talks to: `None` for production; for another, its name and host, as
/// `("staging", "api-staging.eustress.dev")`, `("the local server",
/// "localhost:8787")` or `("the test server", "<host>")`.
pub fn api_label() -> Option<(String, String)> {
    label_for(api_base())
}

fn label_for(base: &str) -> Option<(String, String)> {
    let origin = origin_of(base);
    if origin == origin_of(PRODUCTION_API) {
        return None;
    }
    let host = url::Url::parse(base.trim())
        .ok()
        .and_then(|url| url.host_str().map(|host| match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        }))
        .unwrap_or_else(|| base.trim().to_string());
    let local = host == "localhost" || host.starts_with("localhost:") || host.starts_with("127.0.0.1");
    let name = KNOWN_NAMES
        .iter()
        .find(|(api, _)| *api == origin)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| if local { "the local server" } else { "the test server" }.to_string());
    Some((name, host))
}

/// What an identity key signs to sign in to the API at `origin`
/// ([`api_origin`]): three lines, the format, the origin and the API's
/// challenge (`docs/architecture/STUDIO_IDENTITY_SIGNIN.md`). A signature for
/// one API never signs in to another.
pub fn login_message(origin: &str, challenge: &str) -> String {
    format!("{LOGIN_FORMAT}\n{origin}\n{challenge}")
}

/// The saved login's file name, under `<local data>/EustressEngine/`, for the
/// API in use: `auth_token` for production, `auth_token.<scope>` for any
/// other ([`credential_scope`]). Studio saves its login there and the Player
/// reads it, so a login only ever goes back to the API that issued it.
pub fn login_file() -> String {
    login_file_for(api_base())
}

/// [`login_file`] for a base chosen some other way (a command-line flag). A
/// base [`validate_api_base`] refuses still gets a file of its own.
pub fn login_file_for(base: &str) -> String {
    let base = validate_api_base(base).unwrap_or_else(|_| base.trim().trim_end_matches('/').to_string());
    if base == PRODUCTION_API {
        "auth_token".to_string()
    } else {
        format!("auth_token.{}", scope_of(&base))
    }
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

/// `runtime` is [`API_URL_VAR`]. `compiled` is `EUSTRESS_DEFAULT_API_URL`
/// from [`option_env!`]. A set runtime is taken even when it is refused,
/// which leaves production in use; the compiled base is used only when the
/// runtime variable is unset or blank.
fn resolve(runtime: Option<&str>, compiled: Option<&str>) -> String {
    if let Some(raw) = nonempty(runtime) {
        return accept(raw, API_URL_VAR);
    }
    if let Some(raw) = nonempty(compiled) {
        return accept(raw, "EUSTRESS_DEFAULT_API_URL");
    }
    PRODUCTION_API.to_string()
}

fn nonempty(var: Option<&str>) -> Option<&str> {
    var.map(str::trim).filter(|v| !v.is_empty())
}

fn accept(raw: &str, source: &str) -> String {
    match validate_api_base(raw) {
        Ok(base) if base == PRODUCTION_API => base,
        Ok(base) => {
            tracing::warn!(
                "Eustress API: {base} ({source}), NOT production. Logins and saved tokens are kept apart from production's."
            );
            base
        }
        Err(e) => {
            tracing::warn!("{source} ignored: {e}. Using {PRODUCTION_API}.");
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
        assert_eq!(resolve(None, None), PRODUCTION_API);
        assert_eq!(resolve(Some("  "), None), PRODUCTION_API);
        assert_eq!(
            resolve(Some("http://10.0.0.1"), None),
            PRODUCTION_API,
            "a refused value leaves production in use"
        );
        assert_eq!(resolve(Some("http://localhost:8787/"), None), "http://localhost:8787");
        assert_ne!(scope_of("http://localhost:8787"), scope_of("http://127.0.0.1:8787"));
        assert_eq!(scope_of("http://localhost:8787").len(), 16);
    }

    /// A staging Player is compiled with `EUSTRESS_DEFAULT_API_URL`. That
    /// base is used only when `EUSTRESS_API_URL` is unset. A blank or refused
    /// compiled value leaves production in use, as a production build does.
    #[test]
    fn a_compiled_default_applies_only_when_the_runtime_url_is_unset() {
        assert_eq!(resolve(None, Some(STAGING_API)), STAGING_API);
        assert_eq!(resolve(Some(""), Some(STAGING_API)), STAGING_API);
        assert_eq!(resolve(Some("  "), Some(STAGING_API)), STAGING_API);
        assert_eq!(
            resolve(Some("http://localhost:8787"), Some(STAGING_API)),
            "http://localhost:8787"
        );
        assert_eq!(
            resolve(Some("http://10.0.0.1"), Some(STAGING_API)),
            PRODUCTION_API,
            "a refused runtime value leaves production in use"
        );
        assert_eq!(resolve(None, Some("")), PRODUCTION_API);
        assert_eq!(resolve(None, Some("   ")), PRODUCTION_API);
        assert_eq!(resolve(None, Some("http://10.0.0.1")), PRODUCTION_API);
        assert_eq!(resolve(None, Some(PRODUCTION_API)), PRODUCTION_API);
        assert_eq!(resolve(None, None), PRODUCTION_API);
    }

    /// Production's login keeps its file; every other API gets its own, the
    /// same one however the base is written.
    #[test]
    fn each_api_keeps_its_own_login_file() {
        assert_eq!(login_file_for("https://api.eustress.dev"), "auth_token");
        assert_eq!(login_file_for(" https://api.eustress.dev/ "), "auth_token");
        let staging = login_file_for("https://api-staging.eustress.dev");
        assert_eq!(staging, format!("auth_token.{}", scope_of("https://api-staging.eustress.dev")));
        assert_eq!(login_file_for("https://api-staging.eustress.dev/"), staging);
        assert_ne!(login_file_for("http://127.0.0.1:8787"), staging);
        assert_ne!(login_file_for("http://10.0.0.1"), "auth_token", "a refused base never reads production's login");
    }

    /// The shared login-v1 vectors, read by the Worker's tests too.
    pub(crate) fn login_vectors() -> serde_json::Value {
        serde_json::from_str(include_str!("../../../../docs/architecture/login-v1-vectors.json")).unwrap()
    }

    /// Studio names the API as the API itself does (a browser's `URL`
    /// origin), so a signature made for it is checked against the same text.
    #[test]
    fn api_origin_is_normalised() {
        let vectors = login_vectors();
        for case in vectors["origins"].as_array().unwrap() {
            let (base, origin) = (case["base"].as_str().unwrap(), case["origin"].as_str().unwrap());
            assert_eq!(origin_of(base), origin, "{base}");
        }
        // No scheme, no web origin: "null", which Studio refuses to sign for.
        assert_eq!(origin_of("api.eustress.dev:443"), "null");
    }

    /// A listing's page opens on the site of the API that holds it, never on
    /// production's for a test API.
    #[test]
    fn each_api_opens_its_own_site() {
        assert_eq!(site_for(None, PRODUCTION_API).as_deref(), Some(PRODUCTION_SITE));
        assert_eq!(site_for(None, "https://api-staging.eustress.dev").as_deref(), Some("https://staging.eustress-cbb.pages.dev"));
        assert_eq!(site_for(None, "http://127.0.0.1:8787"), None, "a local API has no known site");
        assert_eq!(site_for(Some("http://localhost:8788/"), "http://127.0.0.1:8787").as_deref(), Some("http://localhost:8788"));
        assert_eq!(
            site_for(Some("http://evil.example"), "https://api-staging.eustress.dev").as_deref(),
            Some("https://staging.eustress-cbb.pages.dev"),
            "a refused site address is ignored"
        );
        for (api, _) in KNOWN_SITES {
            assert_eq!(origin_of(api), *api, "known APIs are listed by origin");
        }
    }

    /// The login message is the three lines both sides build, byte for byte.
    #[test]
    fn login_message_is_bound_to_the_api() {
        let vectors = login_vectors();
        let signatures = vectors["signatures"].as_array().unwrap();
        assert!(signatures.len() >= 2);
        for case in signatures {
            let (origin, challenge) = (case["origin"].as_str().unwrap(), case["challenge"].as_str().unwrap());
            assert!(
                challenge.len() == 64 && challenge.bytes().all(|b| b.is_ascii_hexdigit()),
                "a vector's challenge is a real one, 64 hex digits"
            );
            assert_eq!(origin_of(origin), origin, "a vector's origin is already an origin");
            assert_eq!(login_message(origin, challenge), case["message"].as_str().unwrap());
        }
        let first = &signatures[0];
        let challenge = first["challenge"].as_str().unwrap();
        assert_ne!(
            login_message(first["origin"].as_str().unwrap(), challenge),
            login_message(signatures[1]["origin"].as_str().unwrap(), challenge),
            "one challenge, two APIs, two messages"
        );
    }

    /// Studio names the server it signs in to when it is not production:
    /// staging by name, this machine as the local server, anything else as
    /// the test server, each with its host.
    #[test]
    fn a_server_other_than_production_is_named() {
        assert_eq!(label_for(PRODUCTION_API), None);
        assert_eq!(label_for("https://API.eustress.dev/"), None, "production however it is written");
        let named = |base: &str| label_for(base).unwrap();
        assert_eq!(named("https://api-staging.eustress.dev"), ("staging".to_string(), "api-staging.eustress.dev".to_string()));
        assert_eq!(named("http://localhost:8787"), ("the local server".to_string(), "localhost:8787".to_string()));
        assert_eq!(named("http://127.0.0.1:8787").0, "the local server");
        assert_eq!(named("https://my-worker.example.dev"), ("the test server".to_string(), "my-worker.example.dev".to_string()));
    }

    /// A link names its API as `production` or `staging`, and nothing else
    /// points a Player anywhere.
    #[test]
    fn a_link_names_only_the_two_known_apis() {
        assert_eq!(api_for_name("production"), Some(PRODUCTION_API));
        assert_eq!(api_for_name("staging"), Some(STAGING_API));
        for other in ["", "Staging", "local", "https://api.eustress.dev", "https://evil.example"] {
            assert_eq!(api_for_name(other), None, "{other:?}");
        }
        assert_eq!(validate_api_base(STAGING_API).as_deref(), Ok(STAGING_API), "staging passes the base rules");
        assert_eq!(server_name(PRODUCTION_API), "production");
        assert_eq!(server_name(STAGING_API), "staging");
        assert_eq!(server_name("http://localhost:8787"), "the local server");
    }
}

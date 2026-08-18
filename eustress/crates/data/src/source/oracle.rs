//! Oracle Cloud source — the Data menu's "Oracle" provider, reading rows over
//! Oracle REST Data Services (ORDS).
//!
//! Like [`super::azure`] and unlike [`super::postgres`], this provider really
//! reads. Oracle Autonomous Database and any ORDS-fronted schema expose their
//! tables and handlers as ordinary HTTPS resources returning
//! `{"items": [ … ], "hasMore": …}` — no OCI SDK, no Tokio runtime, no OCI
//! request-signing. A `GET`, an optional `Authorization: Bearer` header, and a
//! JSON decode is the whole protocol.
//!
//! The blocking HTTP client is injected through [`HttpTransport`], which lives
//! in [`super::azure`] (see its module docs for why); `eustress-data` therefore
//! links no HTTP dependency of its own, and the whole fetch path is exercised
//! in CI against a loopback socket with no credentials.
//!
//! ## What is deliberately not here
//!
//! - **`oci://` object storage** — a different service with its own signing
//!   scheme; a public object is an ordinary URL a REST source can read.
//! - **Ad-hoc SQL** — ORDS runs handlers that the *database* defines. A `query`
//!   option is rejected rather than ignored, so nobody believes SQL was sent.
//! - **HTTP Basic auth** — it needs a live credential base64-encoded into every
//!   request; ORDS's OAuth2 bearer flow is both supported and safer.
//!
//! ## Secrets
//!
//! [`SourceConfig::secret_ref`] names the environment variable holding the
//! OAuth2 access token; it is read at call time and never stored.

use std::fmt;
use std::sync::Arc;

use super::azure::{excerpt, is_loopback_authority, HttpMethod, HttpRequest, HttpTransport};
use super::{ConnectionStatus, DataSource, SourceConfig, SourceKind};
use crate::{DataError, Frame, Result};

/// The field ORDS wraps its rows in.
pub const DEFAULT_RECORDS_PATH: &str = "items";

/// Page size used by the liveness probe — enough to prove the handler answers,
/// small enough that a probe is never a download.
const PROBE_LIMIT: u64 = 1;

// ── Resolved target ──────────────────────────────────────────────────────────

/// How an ORDS request authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleAuth {
    /// A handler published without protection.
    Anonymous,
    /// An OAuth2 access token in `Authorization: Bearer …`.
    Bearer,
}

impl OracleAuth {
    /// Stable wire token, as written in the `auth` option.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anonymous => "anonymous",
            Self::Bearer => "bearer",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "anonymous" | "none" | "public" => Some(Self::Anonymous),
            "bearer" | "oauth2" | "oauth" => Some(Self::Bearer),
            _ => None,
        }
    }
}

/// A fully resolved ORDS resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrdsTarget {
    /// The resource URL with no query string — the endpoint plus the `table`
    /// option, when one was given.
    pub base: String,
    /// AutoREST object appended to the endpoint, if any.
    pub table: Option<String>,
    /// ORDS filter object (the `q` parameter), verbatim as the author wrote it.
    pub filter: Option<String>,
    /// Page size.
    pub limit: Option<u64>,
    /// Page offset.
    pub offset: Option<u64>,
    /// Dotted path to the array of rows in the response.
    pub records_path: String,
    /// How the request authenticates.
    pub auth: OracleAuth,
}

impl OrdsTarget {
    /// The URL this source would request.
    ///
    /// `limit_override` lets the liveness probe ask for a single row without
    /// disturbing the configured page size. Query parameters are emitted in a
    /// fixed order so the URL is stable and a test can assert on it.
    pub fn request_url(&self, limit_override: Option<u64>) -> String {
        let mut params: Vec<String> = Vec::new();
        if let Some(q) = &self.filter {
            params.push(format!("q={}", percent_encode(q)));
        }
        if let Some(n) = limit_override.or(self.limit) {
            params.push(format!("limit={n}"));
        }
        if let Some(n) = self.offset {
            params.push(format!("offset={n}"));
        }
        if params.is_empty() {
            self.base.clone()
        } else {
            format!("{}?{}", self.base, params.join("&"))
        }
    }
}

impl fmt::Display for OrdsTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.base)
    }
}

/// Percent-encode a query-parameter value (RFC 3986 unreserved set kept).
///
/// An ORDS filter is a JSON object, so it is full of characters that would
/// otherwise terminate or split the query string.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// ── Source ───────────────────────────────────────────────────────────────────

/// The Oracle Cloud (ORDS) provider.
///
/// Construction validates. A transport must be attached with
/// [`with_transport`](OracleSource::with_transport) before the source can reach
/// anything — without one it says so plainly rather than failing oddly.
#[derive(Clone)]
pub struct OracleSource {
    config: SourceConfig,
    target: OrdsTarget,
    transport: Option<Arc<dyn HttpTransport>>,
}

impl fmt::Debug for OracleSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OracleSource")
            .field("target", &self.target)
            .field("transport", &if self.transport.is_some() { "attached" } else { "none" })
            .finish()
    }
}

impl OracleSource {
    /// Validate `config` and resolve its target. No transport is attached yet.
    pub fn new(config: SourceConfig) -> Result<Self> {
        validate(&config)?;
        let target = resolve_target(&config)?;
        Ok(Self { config, target, transport: None })
    }

    /// Attach the blocking HTTP client this source will use.
    pub fn with_transport(mut self, transport: Arc<dyn HttpTransport>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// The config this source was built from.
    pub fn config(&self) -> &SourceConfig {
        &self.config
    }

    /// The resolved target.
    pub fn target(&self) -> &OrdsTarget {
        &self.target
    }

    /// Build the request this source would issue, resolving the token from the
    /// environment at call time.
    ///
    /// Public so the UI can show exactly what will be sent — [`HttpRequest`]'s
    /// [`fmt::Debug`] is redacted, so doing so cannot leak the token.
    pub fn request(&self, method: HttpMethod, limit_override: Option<u64>) -> Result<HttpRequest> {
        let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
        if self.target.auth == OracleAuth::Bearer {
            let name = self.config.secret_ref.as_deref().unwrap_or("<unset>");
            let token = self.config.resolve_secret().ok_or_else(|| {
                DataError::Schema(format!(
                    "Oracle source is configured for bearer auth, but the environment variable \
                     '{name}' named by secret_ref is not set"
                ))
            })?;
            headers.push(("Authorization".to_string(), format!("Bearer {token}")));
        }
        Ok(HttpRequest { method, url: self.target.request_url(limit_override), headers })
    }

    fn transport(&self) -> Result<&dyn HttpTransport> {
        self.transport.as_deref().ok_or_else(|| {
            DataError::Io(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!(
                    "Oracle source for {} has no HTTP transport — eustress-data links no HTTP \
                     client (leaf purity, invariant D2), so the caller must attach one with \
                     `OracleSource::with_transport`",
                    self.target
                ),
            ))
        })
    }
}

impl DataSource for OracleSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Oracle
    }

    fn test_connection(&self) -> Result<ConnectionStatus> {
        let transport = self.transport()?;
        // GET with limit=1: ORDS has no HEAD contract, and one row is the
        // cheapest honest proof that the handler exists and answers.
        let request = self.request(HttpMethod::Get, Some(PROBE_LIMIT))?;
        match transport.send(&request) {
            Err(e) => Ok(ConnectionStatus::failed(format!("{} unreachable: {e}", self.target))),
            Ok(r) if r.is_success() => Ok(ConnectionStatus::ok(format!(
                "HTTP {} from {} ({} bytes)",
                r.status,
                self.target,
                r.body.len()
            ))),
            Ok(r) => Ok(ConnectionStatus::failed(format!(
                "HTTP {} for {} ({})",
                r.status,
                self.target,
                status_hint(r.status)
            ))),
        }
    }

    fn fetch(&self) -> Result<Frame> {
        let transport = self.transport()?;
        let request = self.request(HttpMethod::Get, None)?;
        let response = transport.send(&request)?;
        if !response.is_success() {
            return Err(DataError::Io(std::io::Error::other(format!(
                "Oracle ORDS GET {} returned HTTP {} ({}): {}",
                self.target,
                response.status,
                status_hint(response.status),
                excerpt(&response.body)
            ))));
        }
        super::azure::frame_from_json_records(&response.body, &self.target.records_path)
    }
}

/// Plain-language reading of the status codes ORDS actually returns.
fn status_hint(status: u16) -> &'static str {
    match status {
        400 => "ORDS rejected the request — check the 'q' filter",
        401 => "not authenticated — the bearer token is missing or expired",
        403 => "authenticated but this role may not read the handler",
        404 => "no such ORDS module, template or table",
        405 => "the handler does not publish a GET",
        429 => "throttled by the service",
        s if (500..600).contains(&s) => "database or ORDS error",
        _ => "unexpected status",
    }
}

// ── Validation (pure, offline) ───────────────────────────────────────────────

/// Validate an Oracle config completely, without touching the network.
pub fn validate(config: &SourceConfig) -> Result<()> {
    if config.kind != SourceKind::Oracle {
        return Err(DataError::Schema(format!(
            "Oracle validation was handed a {} config",
            config.kind.as_str()
        )));
    }
    super::validate_config(config)?;
    resolve_target(config).map(|_| ())
}

fn schema_err(msg: impl Into<String>) -> DataError {
    DataError::Schema(msg.into())
}

fn non_empty(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|s| !s.is_empty())
}

fn resolve_target(config: &SourceConfig) -> Result<OrdsTarget> {
    let endpoint = config.endpoint.trim();

    reject_misleading_options(config)?;

    let (rest, secure) = match endpoint.strip_prefix("https://") {
        Some(r) => (r, true),
        None => (
            endpoint.strip_prefix("http://").ok_or_else(|| {
                schema_err(format!("Oracle endpoint '{endpoint}' must be an http(s) URL"))
            })?,
            false,
        ),
    };

    if endpoint.contains('?') || endpoint.contains('#') {
        return Err(schema_err(format!(
            "Oracle endpoint '{endpoint}' carries a query string — use the 'q', 'limit' and \
             'offset' options so the request is built one way only"
        )));
    }

    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, p),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return Err(schema_err(format!("Oracle endpoint '{endpoint}' has no host")));
    }
    if path.trim().trim_matches('/').is_empty() {
        return Err(schema_err(format!(
            "Oracle endpoint '{endpoint}' names no ORDS resource — expected \
             https://host/ords/<schema>/<module>/<template>"
        )));
    }

    if !secure && !is_loopback_authority(authority) && !allow_insecure(config)? {
        return Err(schema_err(format!(
            "Oracle endpoint '{endpoint}' is plaintext http:// to a non-loopback host — a bearer \
             token would travel in the clear; use https://, or set 'allow_insecure' to true if \
             this really is a trusted private network"
        )));
    }

    let table = match non_empty(config.option("table")) {
        Some(t) => {
            validate_object_name(t)?;
            Some(t.to_string())
        }
        None => None,
    };

    let base = {
        let trimmed = endpoint.trim_end_matches('/');
        match &table {
            Some(t) => format!("{trimmed}/{t}"),
            None => trimmed.to_string(),
        }
    };

    let filter = match non_empty(config.option("q")) {
        Some(q) => {
            if !(q.starts_with('{') && q.ends_with('}')) {
                return Err(schema_err(format!(
                    "Oracle 'q' option must be an ORDS filter object, e.g. \
                     {{\"id\":{{\"$gt\":100}}}} — got '{q}'"
                )));
            }
            Some(q.to_string())
        }
        None => None,
    };

    let limit = resolve_count(config, "limit", false)?;
    let offset = resolve_count(config, "offset", true)?;
    let records_path =
        non_empty(config.option("records_path")).unwrap_or(DEFAULT_RECORDS_PATH).to_string();
    let auth = resolve_auth(config)?;

    Ok(OrdsTarget { base, table, filter, limit, offset, records_path, auth })
}

/// Options that would silently do nothing, or something other than the author
/// expects, are refused rather than ignored.
fn reject_misleading_options(config: &SourceConfig) -> Result<()> {
    if non_empty(config.option("query")).is_some() {
        return Err(schema_err(
            "Oracle source sets 'query', but ORDS runs handlers the database publishes — no SQL \
             is sent over the wire. Point the endpoint at the handler, or use the 'q' option for \
             an ORDS filter",
        ));
    }
    if let Some(auth) = non_empty(config.option("auth")) {
        if auth.eq_ignore_ascii_case("basic") {
            return Err(schema_err(
                "Oracle 'basic' auth is not supported — it would base64-encode a live password \
                 into every request. Use ORDS OAuth2 and set 'auth' to bearer",
            ));
        }
    }
    Ok(())
}

fn validate_object_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let head_ok = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_');
    let tail_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '#');
    if !head_ok || !tail_ok {
        return Err(schema_err(format!(
            "Oracle 'table' option '{name}' is not a database object name — it becomes a URL path \
             segment, so it must be plain letters, digits and underscores"
        )));
    }
    Ok(())
}

fn resolve_count(config: &SourceConfig, name: &str, zero_ok: bool) -> Result<Option<u64>> {
    let Some(raw) = non_empty(config.option(name)) else {
        return Ok(None);
    };
    let n: u64 = raw.parse().map_err(|_| {
        schema_err(format!("Oracle '{name}' option '{raw}' is not a whole number"))
    })?;
    if n == 0 && !zero_ok {
        return Err(schema_err(format!(
            "Oracle '{name}' option must be greater than zero"
        )));
    }
    Ok(Some(n))
}

fn allow_insecure(config: &SourceConfig) -> Result<bool> {
    let Some(raw) = non_empty(config.option("allow_insecure")) else {
        return Ok(false);
    };
    match raw.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        _ => Err(schema_err(format!(
            "Oracle 'allow_insecure' option '{raw}' is not a boolean (true or false)"
        ))),
    }
}

fn resolve_auth(config: &SourceConfig) -> Result<OracleAuth> {
    let named = non_empty(config.option("auth"));
    let has_secret = config.secret_ref.is_some();

    let auth = match named {
        Some(a) => OracleAuth::parse(a).ok_or_else(|| {
            schema_err(format!(
                "Oracle 'auth' option '{a}' is not one of anonymous, bearer"
            ))
        })?,
        None if has_secret => OracleAuth::Bearer,
        None => OracleAuth::Anonymous,
    };

    if auth == OracleAuth::Bearer && !has_secret {
        return Err(schema_err(
            "Oracle 'bearer' auth requires secret_ref to name the environment variable holding \
             the OAuth2 access token",
        ));
    }
    if auth == OracleAuth::Anonymous && has_secret {
        return Err(schema_err(
            "Oracle 'anonymous' auth cannot use a secret_ref — set 'auth' to bearer, or drop the \
             secret_ref",
        ));
    }
    Ok(auth)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::azure::testing::{DeadTransport, RecordingTransport, StubServer, TcpTransport};
    use super::*;

    fn base() -> SourceConfig {
        SourceConfig::new(SourceKind::Oracle, "https://adb.us-phoenix-1.oraclecloud.com/ords/hr/employees/")
    }

    fn err_of(config: &SourceConfig) -> String {
        validate(config).expect_err("expected a validation error").to_string()
    }

    /// A config wired to a live loopback stub server.
    fn against(server: &StubServer) -> SourceConfig {
        SourceConfig::new(SourceKind::Oracle, format!("{}/ords/hr/employees", server.base_url()))
    }

    // ── Happy paths ──────────────────────────────────────────────────────────

    #[test]
    fn a_plain_handler_url_resolves_with_ords_defaults() {
        let src = OracleSource::new(base()).unwrap();
        let t = src.target();
        assert_eq!(t.base, "https://adb.us-phoenix-1.oraclecloud.com/ords/hr/employees");
        assert_eq!(t.records_path, DEFAULT_RECORDS_PATH);
        assert_eq!(t.auth, OracleAuth::Anonymous);
        assert_eq!(t.table, None);
        assert_eq!(t.request_url(None), t.base, "no options means no query string");
        assert_eq!(src.kind(), SourceKind::Oracle);
    }

    #[test]
    fn a_table_option_becomes_a_path_segment() {
        let c = SourceConfig::new(SourceKind::Oracle, "https://adb.example.com/ords/hr")
            .with_option("table", "EMPLOYEES");
        let t = OracleSource::new(c).unwrap().target().clone();
        assert_eq!(t.base, "https://adb.example.com/ords/hr/EMPLOYEES");
    }

    #[test]
    fn limit_and_offset_are_emitted_in_a_stable_order() {
        let c = base().with_option("limit", "500").with_option("offset", "1000");
        let t = OracleSource::new(c).unwrap().target().clone();
        assert!(t.request_url(None).ends_with("?limit=500&offset=1000"), "{}", t.request_url(None));
        assert!(
            t.request_url(Some(1)).ends_with("?limit=1&offset=1000"),
            "a probe overrides only the page size"
        );
    }

    #[test]
    fn an_offset_of_zero_is_meaningful_and_allowed() {
        let c = base().with_option("offset", "0");
        let t = OracleSource::new(c).unwrap().target().clone();
        assert_eq!(t.offset, Some(0));
    }

    #[test]
    fn an_ords_filter_is_percent_encoded_into_the_query() {
        let c = base().with_option("q", r#"{"id":{"$gt":100}}"#);
        let t = OracleSource::new(c).unwrap().target().clone();
        let url = t.request_url(None);
        assert!(url.contains("q=%7B%22id%22%3A%7B%22%24gt%22%3A100%7D%7D"), "{url}");
        assert!(!url.contains('{'), "an unencoded brace would break the query: {url}");
    }

    #[test]
    fn percent_encoding_keeps_the_unreserved_set_intact() {
        assert_eq!(percent_encode("aZ0-_.~"), "aZ0-_.~");
        assert_eq!(percent_encode("a b&c=d"), "a%20b%26c%3Dd");
    }

    #[test]
    fn every_auth_token_round_trips() {
        for a in [OracleAuth::Anonymous, OracleAuth::Bearer] {
            assert_eq!(OracleAuth::parse(a.as_str()), Some(a), "{a:?}");
        }
        assert_eq!(OracleAuth::parse("kerberos"), None);
    }

    #[test]
    fn a_secret_ref_alone_implies_bearer() {
        let c = base().with_secret_ref("ORACLE_TOKEN");
        assert_eq!(OracleSource::new(c).unwrap().target().auth, OracleAuth::Bearer);
    }

    #[test]
    fn a_loopback_http_endpoint_needs_no_opt_in() {
        for host in ["127.0.0.1:8080", "localhost:8080", "[::1]:8080"] {
            let c = SourceConfig::new(SourceKind::Oracle, format!("http://{host}/ords/hr/emp"));
            assert!(validate(&c).is_ok(), "{host} rejected");
        }
    }

    // ── Rejected shapes ──────────────────────────────────────────────────────

    #[test]
    fn a_non_oracle_config_is_refused_by_name() {
        let c = SourceConfig::new(SourceKind::Rest, "https://api.example.com/x");
        assert!(err_of(&c).contains("REST"));
    }

    #[test]
    fn a_non_http_endpoint_is_rejected() {
        let c = SourceConfig::new(SourceKind::Oracle, "oci://namespace/bucket/o/rows.json");
        assert!(validate(&c).is_err());
    }

    #[test]
    fn an_endpoint_with_no_resource_path_is_rejected() {
        for endpoint in ["https://adb.example.com", "https://adb.example.com/", "https://adb.example.com///"]
        {
            let c = SourceConfig::new(SourceKind::Oracle, endpoint);
            assert!(err_of(&c).contains("names no ORDS resource"), "{endpoint} accepted");
        }
    }

    #[test]
    fn an_endpoint_carrying_its_own_query_string_is_rejected() {
        let c = SourceConfig::new(SourceKind::Oracle, "https://adb.example.com/ords/hr/emp?limit=5");
        assert!(err_of(&c).contains("carries a query string"));
    }

    #[test]
    fn plaintext_http_to_a_remote_host_is_rejected_unless_opted_in() {
        let c = SourceConfig::new(SourceKind::Oracle, "http://adb.internal/ords/hr/emp");
        assert!(err_of(&c).contains("in the clear"));
        let opted_in = c.with_option("allow_insecure", "true");
        assert!(validate(&opted_in).is_ok());
    }

    #[test]
    fn a_non_boolean_allow_insecure_is_rejected() {
        let c = SourceConfig::new(SourceKind::Oracle, "http://adb.internal/ords/hr/emp")
            .with_option("allow_insecure", "sometimes");
        assert!(err_of(&c).contains("not a boolean"));
    }

    #[test]
    fn a_sql_query_option_is_refused_rather_than_ignored() {
        let c = base().with_option("query", "SELECT * FROM employees");
        let msg = err_of(&c);
        assert!(msg.contains("no SQL is sent"), "{msg}");
        assert!(msg.contains("'q' option"), "the real alternative must be named: {msg}");
    }

    #[test]
    fn basic_auth_is_refused_with_its_reason() {
        let c = base().with_option("auth", "basic").with_secret_ref("ORACLE_PASSWORD");
        assert!(err_of(&c).contains("base64-encode a live password"));
    }

    #[test]
    fn an_unknown_auth_mode_is_rejected() {
        let c = base().with_option("auth", "kerberos");
        assert!(err_of(&c).contains("anonymous, bearer"));
    }

    #[test]
    fn bearer_auth_without_a_secret_ref_is_rejected() {
        let c = base().with_option("auth", "bearer");
        assert!(err_of(&c).contains("secret_ref"));
    }

    #[test]
    fn anonymous_auth_with_a_secret_ref_is_rejected() {
        let c = base().with_option("auth", "anonymous").with_secret_ref("ORACLE_TOKEN");
        assert!(err_of(&c).contains("anonymous"));
    }

    #[test]
    fn a_table_that_is_not_an_object_name_is_rejected() {
        for table in ["employees/../secrets", "emp loyees", "1employees", "emp?x"] {
            let c = base().with_option("table", table);
            assert!(validate(&c).is_err(), "table '{table}' accepted");
        }
    }

    #[test]
    fn a_malformed_ords_filter_is_rejected() {
        for q in ["id > 100", "[{\"id\":1}]", "{\"id\":1"] {
            let c = base().with_option("q", q);
            assert!(validate(&c).is_err(), "q '{q}' accepted");
        }
    }

    #[test]
    fn a_bad_limit_or_offset_is_rejected() {
        for (name, value) in
            [("limit", "0"), ("limit", "-1"), ("limit", "many"), ("offset", "-1"), ("offset", "1.5")]
        {
            let c = base().with_option(name, value);
            assert!(validate(&c).is_err(), "{name} '{value}' accepted");
        }
    }

    #[test]
    fn the_shared_checks_still_apply() {
        let mut c = base();
        c.poll_seconds = Some(0);
        assert!(validate(&c).is_err());
    }

    // ── The request that would be sent ───────────────────────────────────────

    #[test]
    fn an_anonymous_request_carries_only_an_accept_header() {
        let src = OracleSource::new(base()).unwrap();
        let req = src.request(HttpMethod::Get, None).unwrap();
        assert_eq!(req.headers, vec![("Accept".to_string(), "application/json".to_string())]);
    }

    #[test]
    fn a_bearer_token_is_read_from_the_environment_at_call_time() {
        let key = "EUSTRESS_TEST_ORACLE_TOKEN";
        std::env::set_var(key, "STUBTOKEN");
        let src = OracleSource::new(base().with_secret_ref(key)).unwrap();
        let req = src.request(HttpMethod::Get, None).unwrap();
        assert!(req
            .headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer STUBTOKEN"));
        assert!(!format!("{req:?}").contains("STUBTOKEN"), "Debug must redact the token");

        std::env::remove_var(key);
        let msg = src.request(HttpMethod::Get, None).unwrap_err().to_string();
        assert!(msg.contains(key), "an unset variable is reported by name: {msg}");
    }

    // ── The fetch path, against a real socket ────────────────────────────────

    #[test]
    fn with_no_transport_the_source_says_so_plainly() {
        let src = OracleSource::new(base()).unwrap();
        let err = src.fetch().expect_err("no transport attached");
        let msg = err.to_string();
        assert!(msg.contains("Oracle"), "must name the provider: {msg}");
        assert!(msg.contains("with_transport"), "must name the remedy: {msg}");
        assert!(
            matches!(&err, DataError::Io(e) if e.kind() == std::io::ErrorKind::Unsupported),
            "a missing capability must not masquerade as a config error"
        );
        assert!(src.test_connection().is_err());
    }

    #[test]
    fn test_connection_reports_unreachable_rather_than_erroring() {
        let src = OracleSource::new(base()).unwrap().with_transport(Arc::new(DeadTransport));
        let status = src.test_connection().expect("asking is possible; the answer is 'no'");
        assert!(!status.reachable);
        assert!(status.detail.contains("unreachable"), "{}", status.detail);
    }

    #[test]
    fn the_probe_asks_for_one_row_only() {
        let server = StubServer::spawn(200, "application/json", br#"{"items":[]}"#.to_vec());
        let src = OracleSource::new(against(&server)).unwrap().with_transport(Arc::new(TcpTransport));

        let status = src.test_connection().unwrap();
        assert!(status.reachable, "{}", status.detail);

        let seen = server.requests();
        assert_eq!(seen.len(), 1);
        assert!(
            seen[0].starts_with("GET /ords/hr/employees?limit=1 HTTP/1.1"),
            "a probe must not download the page: {}",
            seen[0]
        );
        assert!(seen[0].contains("Accept: application/json"), "{}", seen[0]);
    }

    #[test]
    fn a_404_is_a_reachable_answer_with_a_plain_language_hint() {
        let server = StubServer::spawn(404, "application/json", b"{}".to_vec());
        let src = OracleSource::new(against(&server)).unwrap().with_transport(Arc::new(TcpTransport));
        let status = src.test_connection().unwrap();
        assert!(!status.reachable);
        assert!(status.detail.contains("no such ORDS module"), "{}", status.detail);
    }

    #[test]
    fn a_401_fetch_names_the_expired_token_case() {
        let server = StubServer::spawn(401, "application/json", br#"{"message":"nope"}"#.to_vec());
        let src = OracleSource::new(against(&server)).unwrap().with_transport(Arc::new(TcpTransport));
        let msg = src.fetch().unwrap_err().to_string();
        assert!(msg.contains("HTTP 401"), "{msg}");
        assert!(msg.contains("expired"), "{msg}");
        assert!(msg.contains("nope"), "the body helps diagnose: {msg}");
    }

    #[test]
    fn the_configured_page_size_reaches_the_wire() {
        let transport = Arc::new(RecordingTransport::new(200, r#"{"items":[]}"#));
        let src = OracleSource::new(base().with_option("limit", "250"))
            .unwrap()
            .with_transport(transport.clone());
        let _ = src.fetch();
        assert!(transport.last().url.ends_with("?limit=250"), "{}", transport.last().url);
    }

    #[cfg(not(feature = "import"))]
    #[test]
    fn without_the_import_feature_decoding_fails_honestly_after_a_good_response() {
        let server = StubServer::spawn(200, "application/json", br#"{"items":[{"id":1}]}"#.to_vec());
        let src = OracleSource::new(against(&server)).unwrap().with_transport(Arc::new(TcpTransport));
        let msg = src.fetch().unwrap_err().to_string();
        assert!(msg.contains("the response arrived"), "{msg}");
        assert!(msg.contains("'import' feature"), "{msg}");
    }

    #[cfg(feature = "import")]
    mod decoding {
        use super::*;
        use crate::ColumnDtype;

        #[test]
        fn an_ords_payload_becomes_a_typed_frame() {
            let body = br#"{"items":[{"id":1,"name":"a","psi":14.7},
                                     {"id":2,"name":"b","psi":15.2}],
                            "hasMore":false,"limit":25,"offset":0}"#
                .to_vec();
            let server = StubServer::spawn(200, "application/json", body);
            let src =
                OracleSource::new(against(&server)).unwrap().with_transport(Arc::new(TcpTransport));

            let frame = src.fetch().expect("an ORDS page must decode");
            assert_eq!(frame.n_rows(), 2);
            assert_eq!(frame.n_cols(), 3, "only the rows become columns, not the envelope");
            assert_eq!(frame.specs().find(|s| s.name == "id").unwrap().dtype, ColumnDtype::I64);
            assert_eq!(frame.specs().find(|s| s.name == "psi").unwrap().dtype, ColumnDtype::F64);
        }

        #[test]
        fn an_empty_page_is_an_empty_frame_not_an_error() {
            let server =
                StubServer::spawn(200, "application/json", br#"{"items":[]}"#.to_vec());
            let src =
                OracleSource::new(against(&server)).unwrap().with_transport(Arc::new(TcpTransport));
            assert_eq!(src.fetch().unwrap().n_rows(), 0);
        }

        #[test]
        fn a_custom_records_path_is_honoured() {
            let body = br#"{"data":{"rows":[{"id":1}]}}"#.to_vec();
            let server = StubServer::spawn(200, "application/json", body);
            let config = against(&server).with_option("records_path", "data.rows");
            let src = OracleSource::new(config).unwrap().with_transport(Arc::new(TcpTransport));
            assert_eq!(src.fetch().unwrap().n_rows(), 1);
        }

        #[test]
        fn a_payload_without_the_records_field_names_it() {
            let server =
                StubServer::spawn(200, "application/json", br#"{"rows":[{"id":1}]}"#.to_vec());
            let src =
                OracleSource::new(against(&server)).unwrap().with_transport(Arc::new(TcpTransport));
            let msg = src.fetch().unwrap_err().to_string();
            assert!(msg.contains("'items'"), "{msg}");
        }
    }
}

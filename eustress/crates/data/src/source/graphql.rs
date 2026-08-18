//! GraphQL — one POSTed query, the `data` envelope unwrapped, normalized into a
//! [`Frame`].
//!
//! Shares the HTTP seam with [`super::rest`]: the same [`HttpTransport`], the
//! same auth-header assembly, the same `json_path` navigation, and the same
//! JSON → `Frame` inference. What is GraphQL-specific is only the envelope —
//! the request body carries `query`/`variables`, and a response is a failure
//! when it carries `errors` even though the status said 200.
//!
//! ## Options
//!
//! | option | default | meaning |
//! |---|---|---|
//! | `query` | **required** | the GraphQL document to POST (already enforced by [`super::validate_config`]) |
//! | `variables` | none | a JSON **object** literal bound to the query's variables |
//! | `operation_name` | none | which operation to run in a multi-operation document |
//! | `json_path` | none | where the rows live *inside `data`*, e.g. `readings.edges` |
//! | `auth_header` | `Authorization` | header the resolved secret is sent in |
//! | `auth_scheme` | `Bearer` | prefix for the secret; set it empty to send the raw value |
//! | `header.<Name>` | none | any additional static request header |
//! | `timeout_seconds` | `30` | transport timeout |

#![cfg(feature = "import")]

use std::sync::Arc;

use serde_json::Value;

use super::rest::{HttpRequest, HttpResponse, HttpTransport};
use super::{validate_config, ConnectionStatus, DataSource, SourceConfig, SourceKind};
use crate::{DataError, Frame, Result};

/// The cheapest question every spec-compliant GraphQL server can answer. Used
/// by [`DataSource::test_connection`] so a liveness probe never runs the real
/// query or pulls a dataset.
const PROBE_QUERY: &str = "{__typename}";

/// A GraphQL endpoint.
pub struct GraphQlSource {
    config: SourceConfig,
    transport: Arc<dyn HttpTransport>,
}

impl std::fmt::Debug for GraphQlSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `config` holds only the NAME of the secret, never its value.
        f.debug_struct("GraphQlSource").field("config", &self.config).finish_non_exhaustive()
    }
}

impl GraphQlSource {
    /// Build a GraphQL source on the default transport, validating the config
    /// first. Never touches the network.
    pub fn new(config: SourceConfig) -> Result<Self> {
        Self::with_transport(config, super::rest::default_transport())
    }

    /// Build a GraphQL source on a caller-supplied transport — the constructor
    /// tests use to drive the whole provider with no network at all.
    pub fn with_transport(config: SourceConfig, transport: Arc<dyn HttpTransport>) -> Result<Self> {
        validate(&config)?;
        Ok(Self { config, transport })
    }

    /// Borrow the config this source was built from.
    pub fn config(&self) -> &SourceConfig {
        &self.config
    }

    /// Assemble the request [`DataSource::fetch`] would send. Exposed so a
    /// caller can show the user exactly what will go out before enabling a
    /// source.
    pub fn request(&self) -> Result<HttpRequest> {
        let query = self.config.option("query").unwrap_or_default();
        self.request_for(query)
    }

    /// The request for an arbitrary document — `fetch` passes the configured
    /// query, `test_connection` passes [`PROBE_QUERY`].
    fn request_for(&self, query: &str) -> Result<HttpRequest> {
        let mut envelope = serde_json::Map::new();
        envelope.insert("query".into(), Value::String(query.to_string()));
        if let Some(vars) = parse_variables(&self.config)? {
            envelope.insert("variables".into(), vars);
        }
        if let Some(name) = self.config.option("operation_name").map(str::trim).filter(|s| !s.is_empty())
        {
            envelope.insert("operationName".into(), Value::String(name.to_string()));
        }

        let mut headers = super::rest::auth_headers(&self.config);
        super::rest::set_header(&mut headers, "Content-Type", "application/json");

        Ok(HttpRequest {
            method: "POST".into(),
            url: self.config.endpoint.clone(),
            headers,
            body: Some(Value::Object(envelope).to_string()),
        })
    }

}

/// Parse the `variables` option. It must be a JSON object, because that is what
/// the GraphQL wire format accepts — anything else is rejected here rather than
/// by the server: offline, at author time.
///
/// A free function, so [`validate`] runs the exact parser [`GraphQlSource`]
/// will, without constructing a source.
fn parse_variables(config: &SourceConfig) -> Result<Option<Value>> {
    let Some(raw) = config.option("variables").map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let parsed: Value = serde_json::from_str(raw).map_err(|e| {
        DataError::Schema(format!("GraphQL source: 'variables' is not valid JSON: {e}"))
    })?;
    match parsed {
        Value::Object(_) => Ok(Some(parsed)),
        other => Err(DataError::Schema(format!(
            "GraphQL source: 'variables' must be a JSON object, got a {}",
            super::rest::json_type(&other)
        ))),
    }
}

/// Validate a GraphQL config without touching the network.
///
/// Runs the shared [`validate_config`] checks (which already require a
/// non-empty `query`), then the GraphQL-only ones. Pure, so the UI can reject a
/// bad config before anything is enabled.
pub fn validate(config: &SourceConfig) -> Result<()> {
    if config.kind != SourceKind::GraphQl {
        return Err(DataError::Schema(format!(
            "GraphQlSource cannot serve a {} config",
            config.kind.as_str()
        )));
    }
    validate_config(config)?;
    super::rest::check_json_path(config)?;
    super::rest::check_timeout(config)?;
    // The real parser, so a bad `variables` literal fails identically whether
    // it is caught by validation or by a fetch.
    parse_variables(config)?;
    Ok(())
}

impl DataSource for GraphQlSource {
    fn kind(&self) -> SourceKind {
        SourceKind::GraphQl
    }

    fn test_connection(&self) -> Result<ConnectionStatus> {
        let probe = self.request_for(PROBE_QUERY)?;
        let resp = match self.transport.send(&probe) {
            Ok(r) => r,
            Err(e) => {
                return Ok(ConnectionStatus::failed(format!("{} unreachable: {e}", probe.url)))
            }
        };
        if !(200..300).contains(&resp.status) {
            return Ok(ConnectionStatus::failed(format!(
                "HTTP {} from {}",
                resp.status, probe.url
            )));
        }
        // A GraphQL server answers `{__typename}` with the query root's type
        // name; anything else means the endpoint is not speaking GraphQL.
        match super::rest::parse_json(&resp.body).and_then(|v| unwrap_data(&v).map(Value::clone)) {
            Ok(Value::Object(m)) => {
                let root = m.get("__typename").and_then(Value::as_str).unwrap_or("(unnamed)");
                Ok(ConnectionStatus::ok(format!("GraphQL endpoint reachable — query root {root}")))
            }
            Ok(_) | Err(_) => Ok(ConnectionStatus::failed(format!(
                "{} answered HTTP {} but not with a GraphQL envelope",
                probe.url, resp.status
            ))),
        }
    }

    fn fetch(&self) -> Result<Frame> {
        let req = self.request()?;
        let resp = self.transport.send(&req)?;
        require_graphql_success(&req, &resp)?;
        let root = super::rest::parse_json(&resp.body)?;
        let data = unwrap_data(&root)?;
        super::rest::frame_from_json_value(data, self.config.option("json_path"))
    }
}

/// A GraphQL failure is not always an HTTP failure: many servers answer 200
/// with an `errors` array and a null `data`. Check the status first, then the
/// envelope, so neither shape is silently normalized into an empty frame.
fn require_graphql_success(req: &HttpRequest, resp: &HttpResponse) -> Result<()> {
    super::rest::require_success(SourceKind::GraphQl, req, resp)?;
    let root = super::rest::parse_json(&resp.body)?;
    if let Some(errors) = root.get("errors").filter(|v| !v.is_null()) {
        return Err(DataError::Schema(format!(
            "GraphQL endpoint {} returned errors: {}",
            req.url,
            summarize_errors(errors)
        )));
    }
    Ok(())
}

/// Flatten a GraphQL `errors` array into one readable line.
fn summarize_errors(errors: &Value) -> String {
    match errors {
        Value::Array(items) => {
            let msgs: Vec<String> = items
                .iter()
                .map(|e| {
                    e.get("message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| e.to_string())
                })
                .collect();
            msgs.join("; ")
        }
        other => other.to_string(),
    }
}

/// Unwrap the mandatory `data` member of a GraphQL response envelope.
fn unwrap_data(root: &Value) -> Result<&Value> {
    match root.get("data") {
        Some(Value::Null) | None => Err(DataError::Schema(
            "GraphQL response has no 'data' member — the endpoint did not return a GraphQL envelope"
                .into(),
        )),
        Some(data) => Ok(data),
    }
}

#[cfg(test)]
mod tests {
    use super::super::rest::testing::{CannedTransport, SocketTransport, StubServer};
    use super::*;
    use crate::{ColumnData, ColumnDtype};

    /// Serialize env mutation: `resolve_secret` reads process-wide state.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn gql(endpoint: &str) -> SourceConfig {
        SourceConfig::new(SourceKind::GraphQl, endpoint)
            .with_option("query", "{ readings { t n } }")
    }

    fn source(config: SourceConfig) -> GraphQlSource {
        GraphQlSource::with_transport(config, Arc::new(SocketTransport))
            .expect("valid graphql config")
    }

    fn canned(config: SourceConfig, status: u16, body: &str) -> GraphQlSource {
        GraphQlSource::with_transport(
            config,
            Arc::new(CannedTransport(HttpResponse { status, body: body.into() })),
        )
        .expect("valid graphql config")
    }

    // ── happy path ───────────────────────────────────────────────────────────

    #[test]
    fn fetches_a_data_envelope_and_posts_the_query() {
        let server = StubServer::json(
            r#"{"data":{"readings":[{"t":0.0,"n":1},{"t":0.5,"n":2},{"t":1.0,"n":3}]}}"#,
        );
        let cfg = gql(&format!("{}/graphql", server.base_url)).with_option("json_path", "readings");
        let frame = source(cfg).fetch().unwrap();

        assert_eq!(frame.n_rows(), 3);
        assert_eq!(frame.n_cols(), 2);
        assert_eq!(frame.specs().find(|s| s.name == "t").unwrap().dtype, ColumnDtype::F64);
        assert_eq!(frame.specs().find(|s| s.name == "n").unwrap().dtype, ColumnDtype::I64);
        match frame.column("n").unwrap() {
            ColumnData::I64(v) => assert_eq!(v, &[Some(1), Some(2), Some(3)]),
            other => panic!("n should be I64, got {other:?}"),
        }

        let sent = server.first_request();
        assert!(sent.starts_with("POST /graphql HTTP/1.1"), "GraphQL must POST: {sent}");
        assert!(sent.contains("Content-Type: application/json"), "{sent}");
        assert!(sent.contains(r#""query":"{ readings { t n } }""#), "query not sent: {sent}");
    }

    #[test]
    fn data_with_a_single_object_is_a_one_row_frame() {
        let body = r#"{"data":{"me":{"id":7,"name":"mck"}}}"#;
        let cfg = gql("https://api.example.com/graphql").with_option("json_path", "me");
        let frame = canned(cfg, 200, body).fetch().unwrap();
        assert_eq!(frame.n_rows(), 1);
        assert_eq!(frame.n_cols(), 2);
    }

    #[test]
    fn json_path_is_relative_to_data_and_walks_nested_connections() {
        let body = r#"{"data":{"readings":{"edges":[{"id":1},{"id":2}]}}}"#;
        let cfg =
            gql("https://api.example.com/graphql").with_option("json_path", "readings.edges");
        let frame = canned(cfg, 200, body).fetch().unwrap();
        assert_eq!(frame.n_rows(), 2);
        match frame.column("id").unwrap() {
            ColumnData::I64(v) => assert_eq!(v, &[Some(1), Some(2)]),
            other => panic!("id should be I64, got {other:?}"),
        }
    }

    #[test]
    fn variables_and_operation_name_ride_in_the_envelope() {
        let server = StubServer::json(r#"{"data":{"rows":[{"a":1}]}}"#);
        let cfg = gql(&format!("{}/graphql", server.base_url))
            .with_option("variables", r#"{"limit":10,"since":"2026-01-01"}"#)
            .with_option("operation_name", "Readings")
            .with_option("json_path", "rows");
        source(cfg).fetch().unwrap();

        let sent = server.first_request();
        assert!(sent.contains(r#""variables":{"limit":10,"since":"2026-01-01"}"#), "{sent}");
        assert!(sent.contains(r#""operationName":"Readings""#), "{sent}");
    }

    // ── failure modes ────────────────────────────────────────────────────────

    #[test]
    fn a_200_with_an_errors_array_is_a_failure_not_an_empty_frame() {
        let body = r#"{"errors":[{"message":"Field 'nope' doesn't exist"},{"message":"and another"}],"data":null}"#;
        let err = canned(gql("https://api.example.com/graphql"), 200, body).fetch().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Field 'nope' doesn't exist"), "server's reason missing: {msg}");
        assert!(msg.contains("and another"), "every error should be reported: {msg}");
    }

    #[test]
    fn an_http_error_status_fails_with_the_status() {
        let server = StubServer::spawn(401, "application/json", r#"{"message":"unauthorized"}"#);
        let err = source(gql(&format!("{}/graphql", server.base_url))).fetch().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("401"), "status missing from: {msg}");
        assert!(msg.contains("unauthorized"), "reason missing from: {msg}");
    }

    #[test]
    fn malformed_json_fails_as_a_parse_error_not_a_panic() {
        let server = StubServer::json("<html>gateway timeout</html>");
        let err = source(gql(&format!("{}/graphql", server.base_url))).fetch().unwrap_err();
        assert!(matches!(err, DataError::Schema(_)), "expected a schema error, got {err:?}");
        assert!(err.to_string().contains("json parse"), "{err}");
    }

    #[test]
    fn a_response_without_a_data_member_is_rejected() {
        let err = canned(gql("https://api.example.com/graphql"), 200, r#"{"result":[]}"#)
            .fetch()
            .unwrap_err();
        assert!(err.to_string().contains("'data'"), "{err}");
    }

    #[test]
    fn a_null_data_member_is_rejected() {
        let err = canned(gql("https://api.example.com/graphql"), 200, r#"{"data":null}"#)
            .fetch()
            .unwrap_err();
        assert!(err.to_string().contains("'data'"), "{err}");
    }

    #[test]
    fn an_unreachable_endpoint_reports_unreachable_rather_than_erroring() {
        let status = source(gql("http://127.0.0.1:1/graphql")).test_connection().unwrap();
        assert!(!status.reachable, "a refused connection must not read as reachable");
    }

    // ── test_connection ──────────────────────────────────────────────────────

    #[test]
    fn test_connection_probes_with_typename_not_the_real_query() {
        let server = StubServer::json(r#"{"data":{"__typename":"Query"}}"#);
        let s = source(gql(&format!("{}/graphql", server.base_url)));
        let status = s.test_connection().unwrap();
        assert!(status.reachable, "{}", status.detail);
        assert!(status.detail.contains("Query"), "root type should be reported: {}", status.detail);

        let sent = server.first_request();
        assert!(sent.contains("__typename"), "probe should ask for __typename: {sent}");
        assert!(
            !sent.contains("readings"),
            "a liveness probe must not run the configured query: {sent}"
        );
    }

    #[test]
    fn a_non_graphql_endpoint_answering_200_is_not_reachable() {
        let server = StubServer::spawn(200, "text/html", "<html>hello</html>");
        let status = source(gql(&format!("{}/graphql", server.base_url))).test_connection().unwrap();
        assert!(!status.reachable, "HTML at a GraphQL endpoint is not a live GraphQL server");
    }

    // ── auth ─────────────────────────────────────────────────────────────────

    #[test]
    fn the_auth_header_is_sent_when_the_secret_ref_resolves() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("EUSTRESS_TEST_GQL_TOKEN", "gql-s3cr3t");

        let server = StubServer::json(r#"{"data":{"rows":[{"a":1}]}}"#);
        let cfg = gql(&format!("{}/graphql", server.base_url))
            .with_secret_ref("EUSTRESS_TEST_GQL_TOKEN")
            .with_option("json_path", "rows");
        source(cfg).fetch().unwrap();

        let sent = server.first_request();
        std::env::remove_var("EUSTRESS_TEST_GQL_TOKEN");
        assert!(
            sent.contains("Authorization: Bearer gql-s3cr3t"),
            "auth header never reached the wire: {sent}"
        );
    }

    #[test]
    fn no_secret_ever_appears_in_debug_output() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("EUSTRESS_TEST_GQL_DEBUG", "TOP-SECRET-VALUE");

        let cfg =
            gql("https://api.example.com/graphql").with_secret_ref("EUSTRESS_TEST_GQL_DEBUG");
        let s = canned(cfg, 200, r#"{"data":[]}"#);
        let req = s.request().unwrap();

        assert!(
            req.headers.iter().any(|(_, v)| v.contains("TOP-SECRET-VALUE")),
            "sanity: the request should carry the credential"
        );
        let rendered = format!("{req:?} {s:?}");
        std::env::remove_var("EUSTRESS_TEST_GQL_DEBUG");
        assert!(
            !rendered.contains("TOP-SECRET-VALUE"),
            "a secret leaked into Debug output: {rendered}"
        );
        assert!(
            rendered.contains("EUSTRESS_TEST_GQL_DEBUG"),
            "the secret_ref NAME is not a secret: {rendered}"
        );
    }

    // ── validation (pure, no transport) ──────────────────────────────────────

    #[test]
    fn validate_rejects_a_config_from_another_provider() {
        let cfg = SourceConfig::new(SourceKind::Rest, "https://api.example.com/x");
        assert!(validate(&cfg).is_err(), "GraphQlSource must refuse a REST config");
    }

    #[test]
    fn validate_still_requires_a_query() {
        let bare = SourceConfig::new(SourceKind::GraphQl, "https://api.example.com/graphql");
        assert!(validate(&bare).is_err(), "the shared rule requiring 'query' must still apply");
    }

    #[test]
    fn validate_rejects_variables_that_are_not_a_json_object() {
        let base = gql("https://api.example.com/graphql");
        assert!(validate(&base.clone().with_option("variables", "{not json")).is_err());
        assert!(validate(&base.clone().with_option("variables", "[1,2]")).is_err());
        assert!(validate(&base.clone().with_option("variables", "42")).is_err());
        assert!(validate(&base.with_option("variables", r#"{"limit":5}"#)).is_ok());
    }

    #[test]
    fn validate_rejects_a_blank_json_path_and_a_non_positive_timeout() {
        let base = gql("https://api.example.com/graphql");
        assert!(validate(&base.clone().with_option("json_path", " ")).is_err());
        assert!(validate(&base.clone().with_option("timeout_seconds", "0")).is_err());
        assert!(validate(&base.with_option("timeout_seconds", "15")).is_ok());
    }

    #[test]
    fn construction_never_touches_the_network() {
        assert!(GraphQlSource::new(gql("http://127.0.0.1:1/graphql")).is_ok());
    }

    // ── the real transport ───────────────────────────────────────────────────
    // Still hermetic: `ureq` talks to the local stub server on loopback.

    #[cfg(feature = "http")]
    #[test]
    fn the_ureq_transport_posts_a_query_to_a_local_server() {
        let server = StubServer::json(r#"{"data":{"rows":[{"a":1},{"a":2}]}}"#);
        let cfg = gql(&format!("{}/graphql", server.base_url)).with_option("json_path", "rows");
        let s = GraphQlSource::with_transport(
            cfg,
            Arc::new(super::super::rest::UreqTransport::default()),
        )
        .unwrap();
        assert_eq!(s.fetch().unwrap().n_rows(), 2);
        assert!(server.first_request().starts_with("POST /graphql"), "GraphQL must POST");
    }

    #[cfg(not(feature = "http"))]
    #[test]
    fn without_the_http_feature_the_default_transport_says_exactly_that() {
        let err =
            GraphQlSource::new(gql("https://api.example.com/graphql")).unwrap().fetch().unwrap_err();
        assert!(err.to_string().contains("`http` feature"), "{err}");
    }
}

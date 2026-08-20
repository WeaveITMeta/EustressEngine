//! Supabase source — a table or view read through PostgREST.
//!
//! Supabase exposes every table under `{project}/rest/v1/{table}` as PostgREST,
//! which answers a plain `GET` with a JSON array of row objects:
//!
//! ```json
//! [ { "id": 1, "sensor": "bay-a-inlet", "pressure_psi": 14.7,
//!     "valve_open": true, "operator": null } ]
//! ```
//!
//! That is already the shape the platform's shared JSON → [`Frame`] inference
//! consumes, so this provider is deliberately thin: build the right URL, attach
//! the project key, and hand the body to [`super::rest::frame_from_json_body`]
//! — the same code path the REST provider and the JSONL file importer use. A
//! Supabase column and a CSV column of the same numbers therefore get the same
//! dtype, and there is no second inference rule to drift.
//!
//! What *is* Supabase-specific, and what the tests below pin down:
//!
//! - `apikey` **and** `Authorization` both carry the project key; PostgREST
//!   uses the first to select the project and the second to pick the role.
//! - Query composition: `select`, `limit`, `offset`, `order`, and a raw
//!   `filter` fragment.
//! - Error envelopes: `{"code": "42P01", "message": …, "hint": …}`, which names
//!   a missing table or a row-level-security denial far better than the status.
//!
//! ## Configuration
//!
//! | key | default | meaning |
//! |---|---|---|
//! | `endpoint` | — | project URL, e.g. `https://abcdefgh.supabase.co` |
//! | `table` | — | table or view name (`collection` is accepted as an alias) |
//! | `select` | `*` | PostgREST `select` expression |
//! | `limit` | 1000 | maximum rows |
//! | `offset` | none | rows to skip |
//! | `order` | none | PostgREST `order` expression, e.g. `recorded_at.desc` |
//! | `filter` | none | extra query fragment appended verbatim, e.g. `status=eq.active` |
//! | `json_path` | none | for the rare view wrapped in an envelope; PostgREST normally returns a bare array |
//! | `secret_ref` | none | env var holding the anon/service key |
//!
//! ## Transport
//!
//! HTTP goes through the same injectable seam as the REST provider
//! ([`super::rest::HttpTransport`]), so this provider gets real TLS from the
//! `http` feature's `ureq` client and tests get real sockets against a
//! throwaway loopback server — with no network, no credentials, and no live
//! Supabase.

// This provider parses JSON and shares the HTTP seam in `rest`, both of
// which live behind `import`. Without this gate the leaf fails to build
// with default features, which is the D2 purity contract in lib.rs.
#![cfg(feature = "import")]

use std::sync::Arc;

use super::firebase::{encode_path, encode_query};
use super::rest::{
    check_json_path, default_transport, frame_from_json_body, parse_json, HttpRequest, HttpResponse,
    HttpTransport,
};
use super::{validate_config, ConnectionStatus, DataSource, SourceConfig, SourceKind};
use crate::{DataError, Frame, Result};

/// Rows per request when `limit` is not configured. This only stops an
/// unbounded pull; the project's own PostgREST cap still applies.
const DEFAULT_LIMIT: usize = 1000;

/// A Supabase table or view, read through PostgREST.
pub struct SupabaseSource {
    config: SourceConfig,
    transport: Arc<dyn HttpTransport>,
}

impl std::fmt::Debug for SupabaseSource {
    /// Hand-written because the transport is a trait object. Renders only the
    /// config, which by construction holds the *name* of a credential and never
    /// its value.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SupabaseSource")
            .field("endpoint", &self.config.endpoint)
            .field("table", &self.table())
            .field("secret_ref", &self.config.secret_ref)
            .finish_non_exhaustive()
    }
}

impl SupabaseSource {
    /// Build a source over the workspace's default HTTP transport.
    ///
    /// Fails on any config the provider cannot honor, so a bad config is caught
    /// at construction rather than on the first fetch.
    pub fn new(config: SourceConfig) -> Result<Self> {
        Self::with_transport(config, default_transport())
    }

    /// Build a source over a caller-supplied transport — the seam tests use to
    /// drive the real provider against a loopback stub.
    pub fn with_transport(config: SourceConfig, transport: Arc<dyn HttpTransport>) -> Result<Self> {
        if config.kind != SourceKind::Supabase {
            return Err(DataError::Schema(format!(
                "SupabaseSource cannot serve a {} config",
                config.kind.as_str()
            )));
        }
        validate_config(&config)?;
        // A blank `json_path` silently means "the whole document", which is
        // never what an author meant to write.
        check_json_path(&config)?;
        Ok(Self { config, transport })
    }

    /// The config this source was built from.
    pub fn config(&self) -> &SourceConfig {
        &self.config
    }

    /// The table or view. `validate_config` guarantees one of these is set.
    fn table(&self) -> &str {
        self.config
            .option("table")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.config.option("collection"))
            .unwrap_or("")
            .trim()
    }

    fn trimmed_option(&self, key: &str) -> Option<&str> {
        self.config.option(key).map(str::trim).filter(|s| !s.is_empty())
    }

    fn limit(&self) -> usize {
        self.trimmed_option("limit")
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_LIMIT)
    }

    /// `{endpoint}/rest/v1/{table}?select=…&limit=…`.
    ///
    /// `/rest/v1` is appended when the caller has not already written it, so
    /// both the bare project URL and the full REST base work.
    fn request_url(&self, limit: usize) -> Result<String> {
        let base = self.config.endpoint.trim().trim_end_matches('/');
        let mut url = base.to_string();
        if !base.ends_with("/rest/v1") {
            url.push_str("/rest/v1");
        }
        url.push('/');
        url.push_str(&encode_path(self.table()));

        url.push_str(&format!("?select={}", encode_query(self.trimmed_option("select").unwrap_or("*"))));
        url.push_str(&format!("&limit={limit}"));
        if let Some(offset) = self.trimmed_option("offset") {
            url.push_str(&format!("&offset={}", encode_query(offset)));
        }
        if let Some(order) = self.trimmed_option("order") {
            url.push_str(&format!("&order={}", encode_query(order)));
        }
        // `filter` is an author-written query fragment (`status=eq.active&
        // psi=gt.10`), so it rides through as written apart from spaces, which
        // are legal inside a PostgREST value but would split the request line.
        if let Some(filter) = self.trimmed_option("filter") {
            if filter.chars().any(char::is_control) {
                return Err(DataError::Schema(
                    "Supabase 'filter' option contains control characters".into(),
                ));
            }
            url.push('&');
            url.push_str(&filter.trim_start_matches('&').replace(' ', "%20"));
        }
        Ok(url)
    }

    /// Build one request.
    ///
    /// PostgREST wants the project key twice: `apikey` selects the project,
    /// `Authorization` picks the role. A `secret_ref` naming an env var that is
    /// not set is an error, never a silently anonymous request against a table
    /// the caller believes is protected.
    fn request(&self, url: String) -> Result<HttpRequest> {
        let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
        if let Some(name) = &self.config.secret_ref {
            let key = self.config.resolve_secret().ok_or_else(|| {
                DataError::Schema(format!(
                    "Supabase source names secret_ref '{name}' but that environment variable is \
                     not set; export it, or clear secret_ref to query an anonymous endpoint"
                ))
            })?;
            headers.push(("apikey".to_string(), key.clone()));
            headers.push(("Authorization".to_string(), format!("Bearer {key}")));
        }
        Ok(HttpRequest { method: "GET".to_string(), url, headers, body: None })
    }

    /// Issue one GET, turning a non-2xx into an error that carries PostgREST's
    /// own message — which names the real problem (missing table, RLS denial,
    /// malformed filter) far better than the status alone.
    fn send(&self, url: String) -> Result<HttpResponse> {
        let response = self.transport.send(&self.request(url)?)?;
        if !is_success(&response) {
            return Err(DataError::Schema(format!(
                "Supabase returned HTTP {} for table '{}': {}",
                response.status,
                self.table(),
                error_detail(&response.body)
            )));
        }
        Ok(response)
    }
}

impl DataSource for SupabaseSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Supabase
    }

    fn test_connection(&self) -> Result<ConnectionStatus> {
        // One row proves the project, the key, the table, and row-level
        // security all line up, without pulling the dataset.
        let request = self.request(self.request_url(1)?)?;
        match self.transport.send(&request) {
            Ok(response) if is_success(&response) => {
                let rows = parse_json(&response.body)
                    .map(|payload| match payload {
                        serde_json::Value::Array(items) => items.len(),
                        _ => 1,
                    })
                    .unwrap_or(0);
                Ok(ConnectionStatus::ok(format!(
                    "Supabase table '{}' reachable ({rows} row{} in the probe page)",
                    self.table(),
                    if rows == 1 { "" } else { "s" }
                )))
            }
            Ok(response) => Ok(ConnectionStatus::failed(format!(
                "Supabase returned HTTP {}: {}",
                response.status,
                error_detail(&response.body)
            ))),
            Err(e) => Ok(ConnectionStatus::failed(e.to_string())),
        }
    }

    fn fetch(&self) -> Result<Frame> {
        let response = self.send(self.request_url(self.limit())?)?;
        // The shared JSON → Frame path: same dtype and null inference as the
        // REST provider and the JSONL importer.
        frame_from_json_body(&response.body, self.config.option("json_path"))
    }
}

/// Whether a transport response carries a 2xx status.
fn is_success(response: &HttpResponse) -> bool {
    (200..300).contains(&response.status)
}

/// Pull the human-readable message out of a PostgREST error envelope
/// (`{"code": …, "message": …, "details": …, "hint": …}`), falling back to the
/// raw body so nothing is ever swallowed.
fn error_detail(body: &str) -> String {
    let trimmed = body.trim();
    if let Ok(payload) = parse_json(trimmed) {
        let message = payload.get("message").and_then(serde_json::Value::as_str).unwrap_or("");
        if !message.is_empty() {
            let mut detail = match payload.get("code").and_then(serde_json::Value::as_str) {
                Some(code) if !code.is_empty() => format!("{code}: {message}"),
                _ => message.to_string(),
            };
            if let Some(hint) = payload.get("hint").and_then(serde_json::Value::as_str) {
                if !hint.is_empty() {
                    detail.push_str(&format!(" (hint: {hint})"));
                }
            }
            return detail;
        }
    }
    if trimmed.is_empty() {
        "(empty response body)".to_string()
    } else {
        trimmed.chars().take(400).collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — hermetic: no network, no credentials, no live Supabase.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::rest::testing::{SocketTransport, StubServer};
    use super::*;
    use crate::{ColumnData, ColumnDtype};
    use std::sync::Mutex;

    /// A transport that answers with one canned response and records what it
    /// was asked for. Socket-level behaviour is covered separately by the
    /// [`StubServer`] tests below.
    struct RecordingTransport {
        reply: HttpResponse,
        seen: Mutex<Vec<HttpRequest>>,
    }

    impl RecordingTransport {
        fn new(status: u16, body: &str) -> Arc<Self> {
            Arc::new(Self {
                reply: HttpResponse { status, body: body.to_string() },
                seen: Mutex::new(Vec::new()),
            })
        }

        fn url(&self) -> String {
            self.seen.lock().unwrap().first().map(|r| r.url.clone()).unwrap_or_default()
        }

        fn header(&self, name: &str) -> Option<String> {
            self.seen.lock().unwrap().first().and_then(|r| {
                r.headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(name))
                    .map(|(_, v)| v.clone())
            })
        }

        fn request_count(&self) -> usize {
            self.seen.lock().unwrap().len()
        }
    }

    impl HttpTransport for RecordingTransport {
        fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
            self.seen.lock().unwrap().push(req.clone());
            Ok(self.reply.clone())
        }
    }

    /// The PostgREST payload from the shared fixture, inlined so this unit test
    /// does not reach outside the crate source: every JSON scalar plus SQL
    /// nulls in both a numeric and a text column.
    const READINGS: &str = r#"[
      { "id": 1, "sensor": "bay-a-inlet", "pressure_psi": 14.7,
        "valve_open": true, "operator": "alvarez",
        "recorded_at": "2026-08-14T17:04:11.882+00:00" },
      { "id": 2, "sensor": "bay-a-inlet", "pressure_psi": 15.02,
        "valve_open": true, "operator": "alvarez",
        "recorded_at": "2026-08-14T17:04:12.382+00:00" },
      { "id": 3, "sensor": "bay-a-inlet", "pressure_psi": null,
        "valve_open": false, "operator": "alvarez",
        "recorded_at": "2026-08-14T17:04:12.884+00:00" },
      { "id": 4, "sensor": "bay-b-outlet", "pressure_psi": 15.21,
        "valve_open": true, "operator": null,
        "recorded_at": "2026-08-14T17:04:13.377+00:00" }
    ]"#;

    fn config(endpoint: &str) -> SourceConfig {
        SourceConfig::new(SourceKind::Supabase, endpoint).with_option("table", "readings")
    }

    /// A source whose transport replays one canned reply without a socket.
    fn canned(status: u16, body: &str) -> (SupabaseSource, Arc<RecordingTransport>) {
        let transport = RecordingTransport::new(status, body);
        let source =
            SupabaseSource::with_transport(config("http://supabase.invalid"), transport.clone())
                .expect("valid supabase config");
        (source, transport)
    }

    fn i64_col(frame: &Frame, name: &str) -> Vec<Option<i64>> {
        match frame.column(name).unwrap_or_else(|| panic!("no column `{name}`")) {
            ColumnData::I64(v) => v.clone(),
            other => panic!("column `{name}` is {:?}, expected I64", other.dtype()),
        }
    }

    fn f64_col(frame: &Frame, name: &str) -> Vec<Option<f64>> {
        match frame.column(name).unwrap_or_else(|| panic!("no column `{name}`")) {
            ColumnData::F64(v) => v.clone(),
            other => panic!("column `{name}` is {:?}, expected F64", other.dtype()),
        }
    }

    fn str_col(frame: &Frame, name: &str) -> Vec<Option<String>> {
        match frame.column(name).unwrap_or_else(|| panic!("no column `{name}`")) {
            ColumnData::Str(v) => v.clone(),
            other => panic!("column `{name}` is {:?}, expected Str", other.dtype()),
        }
    }

    // ── Typed columns ────────────────────────────────────────────────────────

    #[test]
    fn json_scalars_narrow_to_the_right_column_types() {
        let (source, _t) = canned(200, READINGS);
        let frame = source.fetch().expect("fetch");

        assert_eq!(frame.n_rows(), 4);
        let dtype = |name: &str| frame.column(name).expect(name).dtype();
        assert_eq!(dtype("id"), ColumnDtype::I64, "whole numbers must not become floats");
        assert_eq!(dtype("pressure_psi"), ColumnDtype::F64);
        assert_eq!(dtype("valve_open"), ColumnDtype::Bool);
        assert_eq!(dtype("sensor"), ColumnDtype::Str);
        assert_eq!(dtype("recorded_at"), ColumnDtype::Str);

        assert_eq!(i64_col(&frame, "id"), vec![Some(1), Some(2), Some(3), Some(4)]);
        assert_eq!(
            match frame.column("valve_open").unwrap() {
                ColumnData::Bool(v) => v.clone(),
                _ => unreachable!(),
            },
            vec![Some(true), Some(true), Some(false), Some(true)]
        );
    }

    #[test]
    fn a_sql_null_is_a_null_cell_and_does_not_demote_its_column() {
        let (source, _t) = canned(200, READINGS);
        let frame = source.fetch().expect("fetch");
        // A null in the middle of a numeric column must not make it text.
        assert_eq!(frame.column("pressure_psi").unwrap().dtype(), ColumnDtype::F64);
        assert_eq!(
            f64_col(&frame, "pressure_psi"),
            vec![Some(14.7), Some(15.02), None, Some(15.21)]
        );
        assert_eq!(str_col(&frame, "operator")[3], None);
    }

    #[test]
    fn integers_and_floats_in_one_column_widen_to_f64() {
        let (source, _t) = canned(200, r#"[{"v":3},{"v":2.5},{"v":null}]"#);
        let frame = source.fetch().expect("fetch");
        assert_eq!(frame.column("v").unwrap().dtype(), ColumnDtype::F64);
        assert_eq!(f64_col(&frame, "v"), vec![Some(3.0), Some(2.5), None]);
    }

    #[test]
    fn an_embedded_resource_is_kept_as_compact_json() {
        // `select=*,station(*)` embeds a related row; a Frame column is scalar,
        // so the structure is preserved as text rather than dropped.
        let body = r#"[{"id":1,"station":{"name":"alpha","zone":3}},{"id":2}]"#;
        let (source, _t) = canned(200, body);
        let frame = source.fetch().expect("fetch");
        let station = str_col(&frame, "station");
        assert_eq!(station[0].as_deref(), Some(r#"{"name":"alpha","zone":3}"#));
        assert_eq!(station[1], None, "the row without `station` must hold a null");
    }

    #[test]
    fn genuinely_mixed_types_fall_back_to_rendered_strings() {
        let (source, _t) = canned(200, r#"[{"v":1},{"v":"n/a"},{"v":true}]"#);
        let frame = source.fetch().expect("fetch");
        assert_eq!(
            str_col(&frame, "v"),
            vec![Some("1".into()), Some("n/a".into()), Some("true".into())]
        );
    }

    #[test]
    fn an_empty_table_is_an_empty_frame_not_an_error() {
        let (source, _t) = canned(200, "[]");
        let frame = source.fetch().expect("fetch");
        assert_eq!(frame.n_rows(), 0);
        assert_eq!(frame.n_cols(), 0);
    }

    #[test]
    fn a_single_object_response_is_read_as_one_row() {
        // What PostgREST returns for `Accept: application/vnd.pgrst.object+json`
        // or a `.single()` query.
        let (source, _t) = canned(200, r#"{"id":7,"sensor":"solo"}"#);
        let frame = source.fetch().expect("fetch");
        assert_eq!(frame.n_rows(), 1);
        assert_eq!(i64_col(&frame, "id"), vec![Some(7)]);
    }

    #[test]
    fn a_json_path_reaches_rows_wrapped_in_an_envelope() {
        let (_s, _t) = canned(200, "[]");
        let transport = RecordingTransport::new(200, r#"{"data":{"rows":[{"id":5}]}}"#);
        let config = config("http://supabase.invalid").with_option("json_path", "data.rows");
        let frame = SupabaseSource::with_transport(config, transport)
            .unwrap()
            .fetch()
            .expect("fetch");
        assert_eq!(i64_col(&frame, "id"), vec![Some(5)]);
    }

    // ── Request shape ────────────────────────────────────────────────────────

    #[test]
    fn the_request_targets_the_postgrest_path_with_select_and_limit() {
        let transport = RecordingTransport::new(200, "[]");
        let config = config("http://supabase.invalid")
            .with_option("select", "id,sensor,pressure_psi")
            .with_option("limit", "25")
            .with_option("offset", "50")
            .with_option("order", "recorded_at.desc");
        SupabaseSource::with_transport(config, transport.clone()).unwrap().fetch().expect("fetch");

        let url = transport.url();
        assert!(url.contains("/rest/v1/readings"), "{url}");
        assert!(url.contains("select=id,sensor,pressure_psi"), "{url}");
        assert!(url.contains("limit=25"), "{url}");
        assert!(url.contains("offset=50"), "{url}");
        assert!(url.contains("order=recorded_at.desc"), "{url}");
    }

    #[test]
    fn select_defaults_to_everything_and_limit_is_bounded() {
        let (source, transport) = canned(200, "[]");
        source.fetch().expect("fetch");
        let url = transport.url();
        assert!(url.contains("select=*"), "{url}");
        assert!(url.contains(&format!("limit={DEFAULT_LIMIT}")), "a fetch must never be unbounded: {url}");
    }

    #[test]
    fn a_filter_fragment_rides_through_to_postgrest() {
        let transport = RecordingTransport::new(200, "[]");
        let config =
            config("http://supabase.invalid").with_option("filter", "status=eq.active&psi=gt.10");
        SupabaseSource::with_transport(config, transport.clone()).unwrap().fetch().expect("fetch");
        let url = transport.url();
        assert!(url.contains("status=eq.active"), "{url}");
        assert!(url.contains("psi=gt.10"), "{url}");
    }

    #[test]
    fn a_filter_with_spaces_is_escaped_rather_than_splitting_the_request_line() {
        let transport = RecordingTransport::new(200, "[]");
        let config =
            config("http://supabase.invalid").with_option("filter", "sensor=eq.north inlet");
        SupabaseSource::with_transport(config, transport.clone()).unwrap().fetch().expect("fetch");
        let url = transport.url();
        assert!(url.contains("sensor=eq.north%20inlet"), "{url}");
        assert!(!url.contains(' '), "a raw space would split the HTTP request line: {url}");
    }

    #[test]
    fn an_endpoint_that_already_names_rest_v1_is_not_doubled() {
        let transport = RecordingTransport::new(200, "[]");
        let config = config("http://supabase.invalid/rest/v1/");
        SupabaseSource::with_transport(config, transport.clone()).unwrap().fetch().expect("fetch");
        let url = transport.url();
        assert!(!url.contains("rest/v1/rest/v1"), "path was doubled: {url}");
        assert!(url.contains("/rest/v1/readings"), "{url}");
    }

    #[test]
    fn collection_is_accepted_as_an_alias_for_table() {
        let transport = RecordingTransport::new(200, "[]");
        let config = SourceConfig::new(SourceKind::Supabase, "http://supabase.invalid")
            .with_option("collection", "readings");
        SupabaseSource::with_transport(config, transport.clone()).unwrap().fetch().expect("fetch");
        assert!(transport.url().contains("/rest/v1/readings"), "{}", transport.url());
    }

    #[test]
    fn test_connection_asks_for_a_single_row() {
        let (source, transport) = canned(200, r#"[{"id":1}]"#);
        let status = source.test_connection().expect("probe");
        assert!(status.reachable, "{}", status.detail);
        assert!(status.detail.contains("readings"), "{}", status.detail);
        assert!(
            transport.url().contains("limit=1"),
            "the liveness probe must not fetch the whole table: {}",
            transport.url()
        );
    }

    // ── Credentials ──────────────────────────────────────────────────────────

    #[test]
    fn the_project_key_is_sent_as_both_apikey_and_bearer() {
        const VAR: &str = "EUSTRESS_TEST_SUPABASE_KEY";
        std::env::set_var(VAR, "anon-key-xyz");
        let transport = RecordingTransport::new(200, "[]");
        let config = config("http://supabase.invalid").with_secret_ref(VAR);
        SupabaseSource::with_transport(config, transport.clone()).unwrap().fetch().expect("fetch");
        std::env::remove_var(VAR);

        assert_eq!(transport.header("apikey").as_deref(), Some("anon-key-xyz"));
        assert_eq!(transport.header("authorization").as_deref(), Some("Bearer anon-key-xyz"));
    }

    #[test]
    fn no_secret_ref_means_no_credential_headers() {
        let (source, transport) = canned(200, "[]");
        source.fetch().expect("fetch");
        assert_eq!(transport.header("apikey"), None);
        assert_eq!(transport.header("authorization"), None);
    }

    #[test]
    fn a_named_secret_that_is_not_in_the_environment_fails_loudly() {
        const VAR: &str = "EUSTRESS_TEST_SUPABASE_ABSENT";
        std::env::remove_var(VAR);
        let transport = RecordingTransport::new(200, "[]");
        let config = config("http://supabase.invalid").with_secret_ref(VAR);
        let err = SupabaseSource::with_transport(config, transport.clone())
            .unwrap()
            .fetch()
            .unwrap_err();
        assert!(err.to_string().contains(VAR), "the error must name the variable: {err}");
        assert_eq!(transport.request_count(), 0, "no request may be sent without the key");
    }

    // ── Failure modes ────────────────────────────────────────────────────────

    #[test]
    fn a_postgrest_error_envelope_surfaces_code_message_and_hint() {
        let body = r#"{"code":"42P01","details":null,"hint":"Check the table name","message":"relation \"public.readings\" does not exist"}"#;
        let (source, _t) = canned(404, body);
        let text = source.fetch().unwrap_err().to_string();
        assert!(text.contains("404"), "{text}");
        assert!(text.contains("42P01"), "{text}");
        assert!(text.contains("does not exist"), "{text}");
        assert!(text.contains("Check the table name"), "{text}");
    }

    #[test]
    fn a_row_level_security_denial_is_reported_verbatim_by_the_probe() {
        let body = r#"{"code":"42501","message":"permission denied for table readings"}"#;
        let (source, _t) = canned(401, body);
        let status = source.test_connection().expect("probe returns Ok");
        assert!(!status.reachable);
        assert!(
            status.detail.contains("permission denied for table readings"),
            "{}",
            status.detail
        );
    }

    #[test]
    fn an_error_body_that_is_not_an_envelope_is_still_shown() {
        let (source, _t) = canned(502, "<html>upstream exploded</html>");
        let text = source.fetch().unwrap_err().to_string();
        assert!(text.contains("upstream exploded"), "{text}");
    }

    #[test]
    fn a_scalar_payload_is_rejected_rather_than_silently_producing_nothing() {
        let (source, _t) = canned(200, "42");
        assert!(source.fetch().is_err(), "a bare number is not a PostgREST result");
    }

    #[test]
    fn malformed_json_is_rejected() {
        let (source, _t) = canned(200, r#"[{"id": }]"#);
        assert!(source.fetch().is_err(), "a truncated payload must not yield a frame");
    }

    // ── Over a real socket ───────────────────────────────────────────────────

    #[test]
    fn the_provider_works_end_to_end_over_a_loopback_socket() {
        // Everything above swaps the transport out; this proves the request the
        // provider builds is a well-formed HTTP request that a real server
        // answers, with both credential headers genuinely on the wire.
        const VAR: &str = "EUSTRESS_TEST_SUPABASE_SOCKET_KEY";
        std::env::set_var(VAR, "wire-key");
        let server = StubServer::json(READINGS);
        let config = config(&server.base_url)
            .with_secret_ref(VAR)
            .with_option("select", "id,pressure_psi")
            .with_option("order", "recorded_at.desc");
        let source = SupabaseSource::with_transport(config, Arc::new(SocketTransport)).unwrap();
        let frame = source.fetch().expect("fetch over a socket");
        std::env::remove_var(VAR);

        assert_eq!(frame.n_rows(), 4);
        assert_eq!(i64_col(&frame, "id"), vec![Some(1), Some(2), Some(3), Some(4)]);

        let request = server.first_request();
        assert!(
            request.starts_with("GET /rest/v1/readings?select=id,pressure_psi"),
            "unexpected request line: {request}"
        );
        assert!(request.contains("order=recorded_at.desc"), "{request}");
        assert!(request.contains("apikey: wire-key"), "the key must reach the wire: {request}");
        assert!(request.contains("Authorization: Bearer wire-key"), "{request}");
    }

    #[test]
    fn a_non_200_over_a_real_socket_becomes_an_error() {
        let server = StubServer::spawn(
            404,
            "application/json",
            r#"{"code":"42P01","message":"relation does not exist"}"#,
        );
        let source =
            SupabaseSource::with_transport(config(&server.base_url), Arc::new(SocketTransport))
                .unwrap();
        let text = source.fetch().unwrap_err().to_string();
        assert!(text.contains("404"), "{text}");
        assert!(text.contains("42P01"), "{text}");
    }

    // ── Config guards ────────────────────────────────────────────────────────

    #[test]
    fn a_config_for_another_provider_is_refused() {
        let config = SourceConfig::new(SourceKind::Firebase, "https://x.example.com")
            .with_option("collection", "readings");
        let err =
            SupabaseSource::with_transport(config, RecordingTransport::new(200, "[]")).unwrap_err();
        assert!(err.to_string().contains("Firebase"), "{err}");
    }

    #[test]
    fn a_config_without_a_table_is_refused_at_construction() {
        let config = SourceConfig::new(SourceKind::Supabase, "https://x.example.com");
        assert!(SupabaseSource::new(config).is_err());
    }

    #[test]
    fn a_blank_json_path_is_refused_at_construction() {
        let config = config("http://supabase.invalid").with_option("json_path", "   ");
        assert!(
            SupabaseSource::with_transport(config, RecordingTransport::new(200, "[]")).is_err(),
            "a blank json_path silently means 'the whole document', which is never intended"
        );
    }

    #[test]
    fn the_source_reports_its_kind() {
        let (source, _t) = canned(200, "[]");
        assert_eq!(source.kind(), SourceKind::Supabase);
        assert_eq!(source.config().kind, SourceKind::Supabase);
    }
}

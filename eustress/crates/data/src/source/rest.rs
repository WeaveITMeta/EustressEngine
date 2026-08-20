//! REST — a JSON document fetched over HTTP(S) and normalized into a [`Frame`].
//!
//! Requires the `import` feature: the payload is normalized through
//! [`crate::import::frame_from_jsonl`], so a REST row and a JSONL row infer
//! their dtypes by exactly the same rules and a REST `Frame` is
//! indistinguishable from an imported one downstream.
//!
//! ## Why the transport is a trait
//!
//! [`HttpTransport`] is the seam between "what to ask for" and "how to put it
//! on a socket". Everything interesting in this module — auth header assembly,
//! `json_path` navigation, status handling, JSON → `Frame` — lives above that
//! seam, so it is all exercised in CI with no network, no credentials, and no
//! TLS stack. The real blocking implementation ([`UreqTransport`]) sits behind
//! the `http` feature and is the only part of the file that needs a socket.
//!
//! ## Options
//!
//! | option | default | meaning |
//! |---|---|---|
//! | `method` | `GET` | `GET` or `POST` |
//! | `body` | none | request body, sent as `application/json` when `method = POST` |
//! | `json_path` | none | where the rows live in a nested payload, e.g. `data.results` or `payload.items[0].rows` |
//! | `auth_header` | `Authorization` | header the resolved secret is sent in |
//! | `auth_scheme` | `Bearer` | prefix for the secret; set it empty to send the raw value |
//! | `header.<Name>` | none | any additional static request header |
//! | `timeout_seconds` | `30` | transport timeout (honoured by [`UreqTransport`]) |
//!
//! The secret itself is never in the config — [`SourceConfig::secret_ref`]
//! names an environment variable, read via
//! [`SourceConfig::resolve_secret`](super::SourceConfig::resolve_secret) at
//! request time and never stored, cached, or rendered.

#![cfg(feature = "import")]

use std::io::{Error as IoError, ErrorKind};
use std::sync::Arc;

use serde_json::Value;

use super::{validate_config, ConnectionStatus, DataSource, SourceConfig, SourceKind};
use crate::{DataError, Frame, Result};

// ── The transport seam ───────────────────────────────────────────────────────
//
// Re-exported from `super::http`, which is the single seam every remote
// provider shares. Kept as a re-export so existing `super::rest::HttpRequest`
// paths keep resolving, and so there is no second definition that could drift
// from the one the security properties are proven against.
pub use super::http::{HttpMethod, HttpRequest, HttpResponse, HttpTransport};

/// The default transport for a source built with `new`.
pub(super) fn default_transport() -> Arc<dyn HttpTransport> {
    #[cfg(feature = "http")]
    {
        Arc::new(UreqTransport::default())
    }
    #[cfg(not(feature = "http"))]
    {
        Arc::new(super::http::UnavailableTransport)
    }
}

/// Real blocking transport, on `ureq` — the same client the rest of the
/// workspace already links. Requires the `http` feature.
#[cfg(feature = "http")]
#[derive(Debug, Clone)]
pub struct UreqTransport {
    /// Connect + read timeout.
    pub timeout: std::time::Duration,
}

#[cfg(feature = "http")]
impl Default for UreqTransport {
    fn default() -> Self {
        Self { timeout: std::time::Duration::from_secs(30) }
    }
}

#[cfg(feature = "http")]
impl HttpTransport for UreqTransport {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        use std::io::Read;
        let agent = ureq::AgentBuilder::new().timeout(self.timeout).build();
        let mut r = agent.request(req.method.as_str(), &req.url);
        for (k, v) in &req.headers {
            r = r.set(k, v);
        }
        let sent = match &req.body {
            Some(b) => r.send_bytes(b),
            None => r.call(),
        };
        // Bodies are read as BYTES: a blob store returns parquet and images, and
        // decoding those as text to fit a String would corrupt them silently.
        let read_body = |resp: ureq::Response| -> Vec<u8> {
            let mut buf = Vec::new();
            let _ = resp.into_reader().take(64 * 1024 * 1024).read_to_end(&mut buf);
            buf
        };
        match sent {
            Ok(resp) => {
                let status = resp.status();
                Ok(HttpResponse { status, body: read_body(resp) })
            }
            // ureq reports a non-2xx status as an error; the provider wants it
            // as a normal response so every status flows through one code path.
            Err(ureq::Error::Status(status, resp)) => {
                Ok(HttpResponse { status, body: read_body(resp) })
            }
            Err(ureq::Error::Transport(t)) => Err(DataError::Io(IoError::new(
                ErrorKind::Other,
                // The URL is config, never a secret; the auth header is not in
                // the transport error.
                format!("{} {} failed: {t}", req.method, req.url),
            ))),
        }
    }
}


// ── The provider ─────────────────────────────────────────────────────────────

/// A REST endpoint returning JSON.
pub struct RestSource {
    config: SourceConfig,
    transport: Arc<dyn HttpTransport>,
}

impl std::fmt::Debug for RestSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `config` holds only the NAME of the secret, never its value.
        f.debug_struct("RestSource").field("config", &self.config).finish_non_exhaustive()
    }
}

impl RestSource {
    /// Build a REST source on the default transport, validating the config
    /// first. Never touches the network.
    pub fn new(config: SourceConfig) -> Result<Self> {
        Self::with_transport(config, default_transport())
    }

    /// Build a REST source on a caller-supplied transport — the constructor
    /// tests use to drive the whole provider with no network at all.
    pub fn with_transport(config: SourceConfig, transport: Arc<dyn HttpTransport>) -> Result<Self> {
        validate(&config)?;
        Ok(Self { config, transport })
    }

    /// Borrow the config this source was built from.
    pub fn config(&self) -> &SourceConfig {
        &self.config
    }

    /// Assemble the request `fetch` would send. Exposed so a caller can show
    /// the user exactly what will go out before enabling a source.
    pub fn request(&self) -> Result<HttpRequest> {
        let method = rest_method(&self.config)?;
        let mut headers = auth_headers(&self.config);
        let body = self.config.option("body").map(str::to_string);
        if method == "POST" {
            set_header(&mut headers, "Content-Type", "application/json");
        }
        Ok(HttpRequest {
            // `rest_method` has already validated the verb; map it onto the
            // shared seam's typed method.
            method: match method {
                "POST" => HttpMethod::Post,
                "HEAD" => HttpMethod::Head,
                _ => HttpMethod::Get,
            },
            url: self.config.endpoint.clone(),
            headers,
            body: if method == "POST" { body.map(String::into_bytes) } else { None },
        })
    }
}

/// Validate a REST config without touching the network.
///
/// Runs the shared [`validate_config`] checks, then the REST-only ones. Pure,
/// so the UI can reject a bad config before anything is enabled.
pub fn validate(config: &SourceConfig) -> Result<()> {
    if config.kind != SourceKind::Rest {
        return Err(DataError::Schema(format!(
            "RestSource cannot serve a {} config",
            config.kind.as_str()
        )));
    }
    validate_config(config)?;
    rest_method(config)?;
    check_json_path(config)?;
    check_timeout(config)
}

impl DataSource for RestSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Rest
    }

    fn test_connection(&self) -> Result<ConnectionStatus> {
        let mut probe = self.request()?;
        probe.method = HttpMethod::Head;
        probe.body = None;
        match self.transport.send(&probe) {
            Err(e) => Ok(ConnectionStatus::failed(format!("{} unreachable: {e}", probe.url))),
            Ok(r) if is_success(r.status) => {
                Ok(ConnectionStatus::ok(format!("HTTP {} from {}", r.status, probe.url)))
            }
            // A host that answers "I do not do HEAD" has still answered, which
            // is the whole question a liveness probe asks.
            Ok(r) if matches!(r.status, 405 | 501) => Ok(ConnectionStatus::ok(format!(
                "reachable — {} declines HEAD (HTTP {})",
                probe.url, r.status
            ))),
            Ok(r) => Ok(ConnectionStatus::failed(format!("HTTP {} from {}", r.status, probe.url))),
        }
    }

    fn fetch(&self) -> Result<Frame> {
        let req = self.request()?;
        let resp = self.transport.send(&req)?;
        require_success(SourceKind::Rest, &req, &resp)?;
        frame_from_json_body(&resp.text(), self.config.option("json_path"))
    }
}

// ── Option handling, shared with the GraphQL provider ────────────────────────

/// Resolve the REST `method` option. `GET` unless told otherwise.
fn rest_method(config: &SourceConfig) -> Result<&'static str> {
    match config.option("method").unwrap_or("GET").trim().to_ascii_uppercase().as_str() {
        "GET" => Ok("GET"),
        "POST" => Ok("POST"),
        other => Err(DataError::Schema(format!(
            "REST source: unsupported method '{other}' (expected GET or POST)"
        ))),
    }
}

/// A `json_path` that is present must not be blank — a blank one silently means
/// "the whole document", which is never what the author meant to type.
pub(super) fn check_json_path(config: &SourceConfig) -> Result<()> {
    match config.options.get("json_path") {
        Some(p) if p.trim().is_empty() => Err(DataError::Schema(format!(
            "{} source: 'json_path' is present but blank (omit it to use the whole document)",
            config.kind.as_str()
        ))),
        _ => Ok(()),
    }
}

/// `timeout_seconds`, when present, must be a positive integer.
pub(super) fn check_timeout(config: &SourceConfig) -> Result<()> {
    match config.options.get("timeout_seconds") {
        None => Ok(()),
        Some(raw) => match raw.trim().parse::<u64>() {
            Ok(n) if n > 0 => Ok(()),
            _ => Err(DataError::Schema(format!(
                "{} source: 'timeout_seconds' must be a positive integer, got '{raw}'",
                config.kind.as_str()
            ))),
        },
    }
}

/// The headers every HTTP-family provider sends: `Accept`, any `header.<Name>`
/// options, and the auth header built from the resolved secret.
///
/// The secret is read from the environment here and nowhere else, and it is
/// dropped with the returned vector — never stored on the source.
pub(super) fn auth_headers(config: &SourceConfig) -> Vec<(String, String)> {
    let mut headers = vec![("Accept".to_string(), "application/json".to_string())];

    for (k, v) in &config.options {
        if let Some(name) = k.strip_prefix("header.") {
            if !name.trim().is_empty() {
                set_header(&mut headers, name.trim(), v);
            }
        }
    }

    if let Some(secret) = config.resolve_secret() {
        let name = config.option("auth_header").map(str::trim).filter(|s| !s.is_empty());
        let name = name.unwrap_or("Authorization");
        let scheme = config.option("auth_scheme").unwrap_or("Bearer").trim();
        let value = if scheme.is_empty() { secret } else { format!("{scheme} {secret}") };
        set_header(&mut headers, name, &value);
    }

    headers
}

/// Insert or replace a header, matching the name case-insensitively so a
/// `header.accept` option overrides the default `Accept` instead of duplicating
/// it.
pub(super) fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: &str) {
    match headers.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
        Some(slot) => slot.1 = value.to_string(),
        None => headers.push((name.to_string(), value.to_string())),
    }
}

// ── Status + payload normalization, shared with the GraphQL provider ─────────

fn is_success(status: u16) -> bool {
    (200..300).contains(&status)
}

/// Turn a non-2xx response into an error carrying the status and a bounded
/// slice of the body, which is where an API puts the reason it said no.
pub(super) fn require_success(
    kind: SourceKind,
    req: &HttpRequest,
    resp: &HttpResponse,
) -> Result<()> {
    if is_success(resp.status) {
        return Ok(());
    }
    let snippet: String = resp.text().trim().chars().take(200).collect();
    let because = if snippet.is_empty() { String::new() } else { format!(": {snippet}") };
    Err(DataError::Io(IoError::new(
        ErrorKind::Other,
        format!(
            "{} {} {} returned HTTP {}{because}",
            kind.as_str(),
            req.method,
            req.url,
            resp.status
        ),
    )))
}

/// Parse a response body as JSON, with the provider's phrasing on failure.
pub(super) fn parse_json(body: &str) -> Result<Value> {
    serde_json::from_str(body)
        .map_err(|e| DataError::Schema(format!("http json parse: {e}")))
}

/// Parse a JSON body, walk `json_path`, and normalize the rows into a [`Frame`].
pub(super) fn frame_from_json_body(body: &str, json_path: Option<&str>) -> Result<Frame> {
    let root = parse_json(body)?;
    frame_from_json_value(&root, json_path)
}

/// Walk `json_path` from `root` and normalize what it lands on into a [`Frame`].
///
/// The rows are handed to [`crate::import::frame_from_jsonl`] so dtype and null
/// inference are literally the same code the file importer runs.
pub(super) fn frame_from_json_value(root: &Value, json_path: Option<&str>) -> Result<Frame> {
    let located = match json_path {
        Some(p) => locate(root, p)?,
        None => root,
    };
    let whence = json_path.map(|p| format!("json_path '{p}'")).unwrap_or_else(|| "payload".into());

    let rows: Vec<Value> = match located {
        // An array of objects is the shape every list endpoint returns.
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Object(_) => item.clone(),
                // An array of bare scalars is still a column of data; give it
                // the name the rest of the platform can address it by.
                scalar => {
                    let mut m = serde_json::Map::new();
                    m.insert("value".to_string(), scalar.clone());
                    Value::Object(m)
                }
            })
            .collect(),
        // A single object is a one-row frame — the shape a "current reading"
        // endpoint returns.
        Value::Object(_) => vec![located.clone()],
        other => {
            return Err(DataError::Schema(format!(
                "{whence} is a {} — an HTTP JSON payload must resolve to an array or an object",
                json_type(other)
            )))
        }
    };

    // `frame_from_jsonl` is the single JSON → Frame inference path (P6 import).
    // Re-serializing per row keeps that path authoritative instead of growing a
    // second, subtly different one here.
    let mut jsonl = String::new();
    for row in &rows {
        jsonl.push_str(&row.to_string());
        jsonl.push('\n');
    }
    crate::import::frame_from_jsonl(jsonl.as_bytes())
}

/// Navigate a dotted path such as `data.results` or `payload.items[0].rows`.
/// A numeric segment indexes an array; `$` is accepted as the root and skipped.
fn locate<'a>(root: &'a Value, path: &str) -> Result<&'a Value> {
    let normalized = path.replace('[', ".").replace(']', "");
    let mut cur = root;
    let mut walked = String::new();

    for seg in normalized.split('.').map(str::trim).filter(|s| !s.is_empty() && *s != "$") {
        let here = if walked.is_empty() { "$" } else { walked.as_str() };
        cur = match cur {
            Value::Object(m) => m.get(seg).ok_or_else(|| {
                DataError::Schema(format!("json_path '{path}': no key '{seg}' at '{here}'"))
            })?,
            Value::Array(a) => {
                let i: usize = seg.parse().map_err(|_| {
                    DataError::Schema(format!(
                        "json_path '{path}': '{here}' is an array, so '{seg}' must be an index"
                    ))
                })?;
                a.get(i).ok_or_else(|| {
                    DataError::Schema(format!(
                        "json_path '{path}': index {i} is past the end of the array at '{here}' \
                         ({} items)",
                        a.len()
                    ))
                })?
            }
            other => {
                return Err(DataError::Schema(format!(
                    "json_path '{path}': '{here}' is a {} and has no member '{seg}'",
                    json_type(other)
                )))
            }
        };
        if !walked.is_empty() {
            walked.push('.');
        }
        walked.push_str(seg);
    }
    Ok(cur)
}

/// The JSON type name used in error messages.
pub(super) fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ── Test support ─────────────────────────────────────────────────────────────

/// A canned HTTP server and a std-only HTTP client, shared with the GraphQL
/// provider's tests.
///
/// Both halves are real sockets: the client serializes the same headers the
/// production transport would, and the server records the exact bytes it
/// received — which is how the auth-header test proves the credential reached
/// the wire rather than merely reaching a struct field.
#[cfg(test)]
pub(super) mod testing {
    use super::{HttpRequest, HttpResponse, HttpTransport};
    use crate::Result;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    /// A local HTTP/1.1 server on an ephemeral port that answers every request
    /// with one canned response and records the raw request text.
    pub(crate) struct StubServer {
        /// `http://127.0.0.1:<port>` — no path.
        pub base_url: String,
        requests: Arc<Mutex<Vec<String>>>,
    }

    impl StubServer {
        /// Serve `body` with HTTP 200 and `application/json`.
        pub(crate) fn json(body: &str) -> Self {
            Self::spawn(200, "application/json", body)
        }

        /// Serve an arbitrary status and body.
        pub(crate) fn spawn(status: u16, content_type: &str, body: &str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub server");
            let addr = listener.local_addr().expect("stub server addr");
            let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

            let log = Arc::clone(&requests);
            let response = format!(
                "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                reason(status),
                body.len()
            );
            // Detached: the listener lives as long as the test process. Tests
            // are short-lived, so there is nothing to join and nothing to leak
            // beyond process exit.
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    if let Some(text) = read_request(&mut stream) {
                        log.lock().unwrap().push(text);
                    }
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            });

            Self { base_url: format!("http://{addr}"), requests }
        }

        /// Every raw request the server has received, in order.
        pub(crate) fn received(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }

        /// The first raw request, panicking if none arrived.
        pub(crate) fn first_request(&self) -> String {
            self.received().first().cloned().expect("stub server received no request")
        }
    }

    fn reason(status: u16) -> &'static str {
        match status {
            200 => "OK",
            401 => "Unauthorized",
            404 => "Not Found",
            500 => "Internal Server Error",
            _ => "Status",
        }
    }

    /// Read one HTTP request (head plus `Content-Length` body) as text.
    fn read_request(stream: &mut TcpStream) -> Option<String> {
        let mut reader = BufReader::new(stream.try_clone().ok()?);
        let mut text = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).ok()? == 0 {
                return None;
            }
            let end = line == "\r\n" || line == "\n";
            text.push_str(&line);
            if end {
                break;
            }
        }
        let len = text
            .lines()
            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(str::trim).and_then(|v| v.parse::<usize>().ok()));
        if let Some(n) = len.filter(|n| *n > 0) {
            let mut buf = vec![0u8; n];
            reader.read_exact(&mut buf).ok()?;
            text.push_str(&String::from_utf8_lossy(&buf));
        }
        Some(text)
    }

    /// A std-only HTTP/1.1 client. Enough for `http://` against the stub
    /// server, which is all a hermetic test needs — the production transport is
    /// [`super::UreqTransport`].
    pub(crate) struct SocketTransport;

    impl HttpTransport for SocketTransport {
        fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
            let rest = req.url.strip_prefix("http://").ok_or_else(|| {
                crate::DataError::Schema(format!("test transport only speaks http://, got {}", req.url))
            })?;
            let (authority, path) = match rest.find('/') {
                Some(i) => (&rest[..i], &rest[i..]),
                None => (rest, "/"),
            };

            let body = req.body.clone().unwrap_or_default();
            let mut wire = format!(
                "{} {} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nContent-Length: {}\r\n",
                req.method,
                path,
                body.len()
            );
            for (k, v) in &req.headers {
                wire.push_str(&format!("{k}: {v}\r\n"));
            }
            wire.push_str("\r\n");
            wire.push_str(&String::from_utf8_lossy(&body));

            let mut stream = TcpStream::connect(authority)?;
            stream.write_all(wire.as_bytes())?;
            stream.flush()?;
            let mut raw = Vec::new();
            stream.read_to_end(&mut raw)?;

            let text = String::from_utf8_lossy(&raw).into_owned();
            let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
            let status = head
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(0);
            Ok(HttpResponse::new(status, body))
        }
    }

    /// A transport that answers from a canned response with no socket at all —
    /// for the cases where the point is the payload, not the wire.
    pub(crate) struct CannedTransport(pub HttpResponse);

    impl HttpTransport for CannedTransport {
        fn send(&self, _req: &HttpRequest) -> Result<HttpResponse> {
            Ok(self.0.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{CannedTransport, SocketTransport, StubServer};
    use super::*;
    use crate::{ColumnData, ColumnDtype};

    /// Serialize env mutation: `resolve_secret` reads process-wide state, and
    /// the test harness runs tests on many threads.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn rest(endpoint: &str) -> SourceConfig {
        SourceConfig::new(SourceKind::Rest, endpoint)
    }

    fn source(config: SourceConfig) -> RestSource {
        RestSource::with_transport(config, Arc::new(SocketTransport)).expect("valid rest config")
    }

    fn canned(config: SourceConfig, status: u16, body: &str) -> RestSource {
        RestSource::with_transport(
            config,
            Arc::new(CannedTransport(HttpResponse { status, body: body.into() })),
        )
        .expect("valid rest config")
    }

    // ── happy path ───────────────────────────────────────────────────────────

    #[test]
    fn fetches_a_top_level_array_and_types_every_column() {
        let server = StubServer::json(
            r#"[{"t":0.0,"n":1,"ok":true,"label":"a"},
                {"t":0.5,"n":2,"ok":false,"label":"b"},
                {"t":1.0,"n":3,"ok":true}]"#,
        );
        let frame = source(rest(&format!("{}/readings", server.base_url))).fetch().unwrap();

        assert_eq!(frame.n_rows(), 3);
        assert_eq!(frame.n_cols(), 4);
        assert_eq!(frame.specs().find(|s| s.name == "t").unwrap().dtype, ColumnDtype::F64);
        assert_eq!(frame.specs().find(|s| s.name == "n").unwrap().dtype, ColumnDtype::I64);
        assert_eq!(frame.specs().find(|s| s.name == "ok").unwrap().dtype, ColumnDtype::Bool);
        match frame.column("n").unwrap() {
            ColumnData::I64(v) => assert_eq!(v, &[Some(1), Some(2), Some(3)]),
            other => panic!("n should be I64, got {other:?}"),
        }
        match frame.column("label").unwrap() {
            ColumnData::Str(v) => assert_eq!(v[2], None, "a field absent from a row is a null"),
            other => panic!("label should be Str, got {other:?}"),
        }

        let req = server.first_request();
        assert!(req.starts_with("GET /readings HTTP/1.1"), "wrong request line: {req}");
    }

    #[test]
    fn a_single_object_payload_is_a_one_row_frame() {
        let server = StubServer::json(r#"{"psi":14.7,"sensor":"tank-a"}"#);
        let frame = source(rest(&format!("{}/current", server.base_url))).fetch().unwrap();
        assert_eq!(frame.n_rows(), 1);
        assert_eq!(frame.n_cols(), 2);
        match frame.column("psi").unwrap() {
            ColumnData::F64(v) => assert_eq!(v[0], Some(14.7)),
            other => panic!("psi should be F64, got {other:?}"),
        }
    }

    // ── nested json_path ─────────────────────────────────────────────────────

    #[test]
    fn json_path_locates_rows_in_a_nested_payload() {
        let server = StubServer::json(
            r#"{"meta":{"page":1},"data":{"results":[{"id":7,"name":"x"},{"id":8,"name":"y"}]}}"#,
        );
        let cfg =
            rest(&format!("{}/v1/items", server.base_url)).with_option("json_path", "data.results");
        let frame = source(cfg).fetch().unwrap();

        assert_eq!(frame.n_rows(), 2);
        assert_eq!(frame.n_cols(), 2);
        match frame.column("id").unwrap() {
            ColumnData::I64(v) => assert_eq!(v, &[Some(7), Some(8)]),
            other => panic!("id should be I64, got {other:?}"),
        }
    }

    #[test]
    fn json_path_indexes_arrays_with_bracket_or_dot_notation() {
        let body = r#"{"pages":[{"rows":[{"a":1}]},{"rows":[{"a":2},{"a":3}]}]}"#;
        for path in ["pages[1].rows", "pages.1.rows"] {
            let frame = canned(rest("https://api.example.com/x").with_option("json_path", path), 200, body)
                .fetch()
                .unwrap();
            assert_eq!(frame.n_rows(), 2, "path {path}");
        }
    }

    #[test]
    fn a_missing_json_path_names_the_key_and_where_it_looked() {
        let cfg = rest("https://api.example.com/x").with_option("json_path", "data.rows");
        let err = canned(cfg, 200, r#"{"data":{"items":[]}}"#).fetch().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no key 'rows'"), "unhelpful error: {msg}");
        assert!(msg.contains("data"), "error should say where it looked: {msg}");
    }

    #[test]
    fn a_scalar_payload_is_rejected_with_its_type() {
        let cfg = rest("https://api.example.com/x").with_option("json_path", "count");
        let err = canned(cfg, 200, r#"{"count":42}"#).fetch().unwrap_err();
        assert!(err.to_string().contains("number"), "error should name the type: {err}");
    }

    #[test]
    fn an_array_of_scalars_becomes_a_value_column() {
        let frame = canned(rest("https://api.example.com/x"), 200, "[1,2,3]").fetch().unwrap();
        assert_eq!(frame.n_cols(), 1);
        match frame.column("value").unwrap() {
            ColumnData::I64(v) => assert_eq!(v, &[Some(1), Some(2), Some(3)]),
            other => panic!("value should be I64, got {other:?}"),
        }
    }

    // ── failure modes ────────────────────────────────────────────────────────

    #[test]
    fn an_http_error_status_fails_with_the_status_and_the_reason() {
        let server = StubServer::spawn(500, "application/json", r#"{"error":"boom"}"#);
        let err = source(rest(&format!("{}/x", server.base_url))).fetch().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("500"), "status missing from: {msg}");
        assert!(msg.contains("boom"), "server's reason missing from: {msg}");
    }

    #[test]
    fn a_404_is_an_error_not_an_empty_frame() {
        let server = StubServer::spawn(404, "text/plain", "no such collection");
        let err = source(rest(&format!("{}/nope", server.base_url))).fetch().unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");
    }

    #[test]
    fn malformed_json_fails_as_a_parse_error_not_a_panic() {
        let server = StubServer::json("{ this is not json ");
        let err = source(rest(&format!("{}/x", server.base_url))).fetch().unwrap_err();
        assert!(matches!(err, DataError::Schema(_)), "expected a schema error, got {err:?}");
        assert!(err.to_string().contains("json parse"), "{err}");
    }

    #[test]
    fn an_unreachable_endpoint_reports_unreachable_rather_than_erroring() {
        // Port 1 on loopback refuses instantly; no network required.
        let s = source(rest("http://127.0.0.1:1/x"));
        let status = s.test_connection().unwrap();
        assert!(!status.reachable, "a refused connection must not read as reachable");
        assert!(status.detail.contains("unreachable"), "{}", status.detail);
    }

    #[test]
    fn test_connection_probes_with_head_and_never_fetches() {
        let server = StubServer::json(r#"[{"a":1}]"#);
        let s = source(rest(&format!("{}/x", server.base_url)));
        let status = s.test_connection().unwrap();
        assert!(status.reachable, "{}", status.detail);
        assert!(server.first_request().starts_with("HEAD "), "probe must be a HEAD");
    }

    #[test]
    fn an_endpoint_that_declines_head_still_reads_as_reachable() {
        let server = StubServer::spawn(405, "text/plain", "method not allowed");
        let status = source(rest(&format!("{}/x", server.base_url))).test_connection().unwrap();
        assert!(status.reachable, "405 means the host answered: {}", status.detail);
    }

    // ── auth ─────────────────────────────────────────────────────────────────

    #[test]
    fn the_auth_header_is_sent_when_the_secret_ref_resolves() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("EUSTRESS_TEST_REST_TOKEN", "s3cr3t-value");

        let server = StubServer::json(r#"[{"a":1}]"#);
        let cfg = rest(&format!("{}/x", server.base_url))
            .with_secret_ref("EUSTRESS_TEST_REST_TOKEN");
        source(cfg).fetch().unwrap();

        let sent = server.first_request();
        std::env::remove_var("EUSTRESS_TEST_REST_TOKEN");
        assert!(
            sent.contains("Authorization: Bearer s3cr3t-value"),
            "auth header never reached the wire: {sent}"
        );
    }

    #[test]
    fn no_auth_header_is_sent_when_the_env_var_is_unset() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("EUSTRESS_TEST_REST_ABSENT");

        let server = StubServer::json(r#"[{"a":1}]"#);
        let cfg =
            rest(&format!("{}/x", server.base_url)).with_secret_ref("EUSTRESS_TEST_REST_ABSENT");
        source(cfg).fetch().unwrap();
        assert!(
            !server.first_request().to_ascii_lowercase().contains("authorization:"),
            "an unresolvable secret_ref must not produce an empty credential"
        );
    }

    #[test]
    fn the_auth_header_name_and_scheme_are_configurable() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("EUSTRESS_TEST_REST_KEY", "abc123");

        let server = StubServer::json(r#"[{"a":1}]"#);
        let cfg = rest(&format!("{}/x", server.base_url))
            .with_secret_ref("EUSTRESS_TEST_REST_KEY")
            .with_option("auth_header", "X-Api-Key")
            .with_option("auth_scheme", "");
        source(cfg).fetch().unwrap();

        let sent = server.first_request();
        std::env::remove_var("EUSTRESS_TEST_REST_KEY");
        assert!(sent.contains("X-Api-Key: abc123"), "raw-value auth header missing: {sent}");
        assert!(!sent.contains("Bearer"), "auth_scheme='' must not add a scheme: {sent}");
    }

    #[test]
    fn extra_static_headers_ride_along_and_can_override_the_default_accept() {
        let server = StubServer::json(r#"[{"a":1}]"#);
        let cfg = rest(&format!("{}/x", server.base_url))
            .with_option("header.X-Trace", "abc")
            .with_option("header.Accept", "application/vnd.api+json");
        source(cfg).fetch().unwrap();

        let sent = server.first_request();
        assert!(sent.contains("X-Trace: abc"), "{sent}");
        assert!(sent.contains("Accept: application/vnd.api+json"), "{sent}");
        assert_eq!(
            sent.matches("Accept:").count(),
            1,
            "an override must replace the default Accept, not duplicate it: {sent}"
        );
    }

    #[test]
    fn no_secret_ever_appears_in_debug_output() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("EUSTRESS_TEST_REST_DEBUG", "TOP-SECRET-VALUE");

        let cfg = rest("https://api.example.com/x").with_secret_ref("EUSTRESS_TEST_REST_DEBUG");
        let s = canned(cfg, 200, "[]");
        let req = s.request().unwrap();

        // The header value must be built (it goes on the wire) …
        assert!(
            req.headers.iter().any(|(_, v)| v.contains("TOP-SECRET-VALUE")),
            "sanity: the request should carry the credential"
        );
        // … and must be unreachable through Debug.
        let rendered = format!("{req:?} {s:?}");
        std::env::remove_var("EUSTRESS_TEST_REST_DEBUG");
        assert!(
            !rendered.contains("TOP-SECRET-VALUE"),
            "a secret leaked into Debug output: {rendered}"
        );
        assert!(rendered.contains("Authorization"), "the header NAME is still useful: {rendered}");
        assert!(
            rendered.contains("EUSTRESS_TEST_REST_DEBUG"),
            "the secret_ref NAME is not a secret: {rendered}"
        );
    }

    // ── POST ─────────────────────────────────────────────────────────────────

    #[test]
    fn method_post_sends_the_body_with_a_json_content_type() {
        let server = StubServer::json(r#"{"rows":[{"a":1},{"a":2}]}"#);
        let cfg = rest(&format!("{}/search", server.base_url))
            .with_option("method", "post")
            .with_option("body", r#"{"filter":"all"}"#)
            .with_option("json_path", "rows");
        let frame = source(cfg).fetch().unwrap();
        assert_eq!(frame.n_rows(), 2);

        let sent = server.first_request();
        assert!(sent.starts_with("POST /search HTTP/1.1"), "{sent}");
        assert!(sent.contains("Content-Type: application/json"), "{sent}");
        assert!(sent.ends_with(r#"{"filter":"all"}"#), "body missing from: {sent}");
    }

    // ── validation (pure, no transport) ──────────────────────────────────────

    #[test]
    fn validate_rejects_a_config_from_another_provider() {
        let cfg = SourceConfig::new(SourceKind::GraphQl, "https://api.example.com/graphql")
            .with_option("query", "{ me { id } }");
        assert!(validate(&cfg).is_err(), "RestSource must refuse a GraphQL config");
    }

    #[test]
    fn validate_rejects_an_unsupported_method_and_a_blank_json_path() {
        assert!(validate(&rest("https://x.example.com/a").with_option("method", "DELETE")).is_err());
        assert!(validate(&rest("https://x.example.com/a").with_option("json_path", "  ")).is_err());
        assert!(validate(&rest("https://x.example.com/a").with_option("json_path", "data")).is_ok());
    }

    #[test]
    fn validate_rejects_a_non_positive_timeout() {
        for bad in ["0", "-1", "soon"] {
            assert!(
                validate(&rest("https://x.example.com/a").with_option("timeout_seconds", bad))
                    .is_err(),
                "timeout_seconds '{bad}' should be rejected"
            );
        }
        assert!(validate(&rest("https://x.example.com/a").with_option("timeout_seconds", "5")).is_ok());
    }

    #[test]
    fn validate_still_enforces_the_shared_endpoint_rules() {
        assert!(validate(&rest("not-a-url")).is_err(), "REST requires an http(s) endpoint");
        assert!(validate(&rest("   ")).is_err(), "REST requires a non-empty endpoint");
    }

    #[test]
    fn construction_never_touches_the_network() {
        // A config pointing at a port nothing listens on still constructs; only
        // fetch/test_connection may reach out.
        assert!(RestSource::new(rest("http://127.0.0.1:1/x")).is_ok());
    }

    // ── the real transport ───────────────────────────────────────────────────
    // Still hermetic: `ureq` talks to the local stub server on loopback.

    #[cfg(feature = "http")]
    #[test]
    fn the_ureq_transport_fetches_from_a_local_server() {
        let server = StubServer::json(r#"[{"a":1},{"a":2}]"#);
        let s = RestSource::with_transport(
            rest(&format!("{}/x", server.base_url)),
            Arc::new(UreqTransport::default()),
        )
        .unwrap();
        assert_eq!(s.fetch().unwrap().n_rows(), 2);
    }

    #[cfg(feature = "http")]
    #[test]
    fn the_ureq_transport_reports_an_error_status_rather_than_swallowing_it() {
        let server = StubServer::spawn(500, "application/json", r#"{"error":"boom"}"#);
        let s = RestSource::with_transport(
            rest(&format!("{}/x", server.base_url)),
            Arc::new(UreqTransport::default()),
        )
        .unwrap();
        let msg = s.fetch().unwrap_err().to_string();
        assert!(msg.contains("500"), "{msg}");
        assert!(msg.contains("boom"), "ureq must surface the body of an error response: {msg}");
    }

    #[cfg(not(feature = "http"))]
    #[test]
    fn without_the_http_feature_the_default_transport_says_exactly_that() {
        // The honest-failure contract: no transport is compiled in, so a fetch
        // must name the missing feature instead of returning an empty frame.
        let err = RestSource::new(rest("https://api.example.com/x")).unwrap().fetch().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("`http` feature"), "{msg}");
        assert!(msg.contains("with_transport"), "the error should name the way out: {msg}");
    }
}

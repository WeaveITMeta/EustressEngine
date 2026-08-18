//! Shared hermetic test harness for `eustress_data::source` providers.
//!
//! Every provider behind the Studio's Data menu is meant to be testable
//! *without credentials and without a network* (see the design rules in
//! `src/source/mod.rs`). This file is the machinery that makes that true: a
//! throwaway HTTP server bound to `127.0.0.1:0`, canned responses routed by
//! path, and a full recording of what the provider actually sent — so a test
//! can assert that an `Authorization` header was attached, that a query string
//! carried the right `select=`, or that a POST body held the GraphQL document.
//!
//! ## Why it is written this way
//!
//! - **Pure `std`.** No tokio, no HTTP crate, no C toolchain. The harness is
//!   ~400 lines of `TcpListener` and string handling, which costs less to
//!   compile than any dependency that would replace it and keeps the leaf's
//!   dev-dependency graph honest.
//! - **Ephemeral port.** Binding port 0 lets the OS pick, so parallel `cargo
//!   test` threads never collide and CI never needs a reserved port.
//! - **Never hangs.** The accept loop is non-blocking and polls a shutdown
//!   flag; every socket on both sides carries a read/write timeout; every
//!   response closes the connection. A wedged provider fails a test in seconds
//!   rather than stalling the run.
//! - **Recording, not mocking.** Requests are captured verbatim
//!   ([`RecordedRequest`]) rather than matched against expectations up front,
//!   so a test asserts on evidence after the fact and the failure message shows
//!   what really went over the wire.
//!
//! ## Using it from another test target
//!
//! This is an integration-test *target*, so other targets pull it in as a
//! module rather than as a crate:
//!
//! ```ignore
//! // tests/rest_source.rs
//! #[path = "source_harness.rs"]
//! mod harness;
//! use harness::{fixture, Response, StubServer, FIXTURE_REST_ARRAY};
//!
//! #[test]
//! fn rest_source_sends_its_bearer_token() {
//!     let server = StubServer::builder()
//!         .route("/v1/readings", Response::json(fixture(FIXTURE_REST_ARRAY)))
//!         .start();
//!     // ... drive the provider against server.url_for("/v1/readings") ...
//!     let req = server.last_request().expect("provider issued a request");
//!     assert_eq!(req.header("authorization"), Some("Bearer test-token"));
//! }
//! ```
//!
//! The `#[test]` functions at the bottom of this file are the harness's own
//! self-tests. They re-run in any target that includes the module; they are
//! hermetic and finish in well under a second, so that duplication is cheap.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

// ── Timeouts ─────────────────────────────────────────────────────────────────
// Generous enough that a slow CI box never flakes, short enough that a wedged
// provider fails the test instead of stalling the whole run.

/// How long a connection handler waits for a client to finish its request.
const SERVER_IO_TIMEOUT: Duration = Duration::from_secs(3);
/// How long the bundled client waits to connect, write, and read.
const CLIENT_IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Accept-loop poll interval; also the worst-case shutdown latency.
const ACCEPT_POLL: Duration = Duration::from_millis(2);

// ── Fixtures ─────────────────────────────────────────────────────────────────

/// A Firestore REST `documents.list` payload: typed `fields`, one `nullValue`,
/// one document missing a field entirely, plus a `nextPageToken`.
pub const FIXTURE_FIRESTORE_DOCUMENTS: &str = "firestore_documents.json";
/// A PostgREST result: a bare array of row objects with SQL `null`s.
pub const FIXTURE_POSTGREST_ROWS: &str = "postgrest_rows.json";
/// A plain REST array — the shape a provider must handle with no `json_path`.
pub const FIXTURE_REST_ARRAY: &str = "rest_array.json";
/// A payload whose rows are buried at `data.results.items`, so reaching them
/// requires a `json_path` option.
pub const FIXTURE_NESTED_PAYLOAD: &str = "nested_payload.json";
/// A GraphQL response with the rows under `data.readings`.
pub const FIXTURE_GRAPHQL_RESPONSE: &str = "graphql_response.json";
/// A small CSV with a unit-bearing header, a missing number, and a missing
/// string.
pub const FIXTURE_READINGS_CSV: &str = "readings.csv";

/// Every fixture this harness ships, for tests that want to sweep them.
pub const ALL_FIXTURES: [&str; 6] = [
    FIXTURE_FIRESTORE_DOCUMENTS,
    FIXTURE_POSTGREST_ROWS,
    FIXTURE_REST_ARRAY,
    FIXTURE_NESTED_PAYLOAD,
    FIXTURE_GRAPHQL_RESPONSE,
    FIXTURE_READINGS_CSV,
];

/// Directory holding the canned payloads.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// Absolute path of a fixture — what a `Csv` source's `endpoint` wants.
pub fn fixture_path(name: &str) -> PathBuf {
    fixture_dir().join(name)
}

/// Read a fixture as text. Panics with the full path when it is missing, since
/// a silently empty body would make a downstream assertion lie.
pub fn fixture(name: &str) -> String {
    let p = fixture_path(name);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("fixture {} is unreadable: {e}", p.display()))
}

/// Read a fixture as raw bytes.
pub fn fixture_bytes(name: &str) -> Vec<u8> {
    let p = fixture_path(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("fixture {} is unreadable: {e}", p.display()))
}

// ── Responses ────────────────────────────────────────────────────────────────

/// A canned reply. Build one with [`Response::json`] / [`Response::csv`] /
/// [`Response::text`] / [`Response::status`] and adjust it with the `with_*`
/// builders.
#[derive(Clone, Debug)]
pub struct Response {
    /// HTTP status code.
    pub status: u16,
    /// Reason phrase; defaults from the status code.
    pub reason: String,
    /// Response headers, sent in order. `Content-Length` and `Connection` are
    /// added by the server.
    pub headers: Vec<(String, String)>,
    /// Response body.
    pub body: Vec<u8>,
    /// Artificial latency before the reply is written — for exercising a
    /// provider's own timeout handling.
    pub delay: Duration,
}

impl Response {
    /// A `200 OK` with an explicit content type.
    pub fn new(content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            reason: reason_phrase(200).to_string(),
            headers: vec![("Content-Type".into(), content_type.into())],
            body: body.into(),
            delay: Duration::ZERO,
        }
    }

    /// `200 OK`, `application/json`.
    pub fn json(body: impl Into<Vec<u8>>) -> Self {
        Self::new("application/json; charset=utf-8", body)
    }

    /// `200 OK`, `text/csv`.
    pub fn csv(body: impl Into<Vec<u8>>) -> Self {
        Self::new("text/csv; charset=utf-8", body)
    }

    /// `200 OK`, `text/plain`.
    pub fn text(body: impl Into<Vec<u8>>) -> Self {
        Self::new("text/plain; charset=utf-8", body)
    }

    /// An empty reply with the given status — the terse way to spell "this
    /// endpoint is down".
    pub fn status(status: u16) -> Self {
        Self {
            status,
            reason: reason_phrase(status).to_string(),
            headers: Vec::new(),
            body: Vec::new(),
            delay: Duration::ZERO,
        }
    }

    /// Serve a fixture file, with the content type inferred from its extension.
    pub fn fixture(name: &str) -> Self {
        let body = fixture_bytes(name);
        match Path::new(name).extension().and_then(|e| e.to_str()) {
            Some("json") => Self::json(body),
            Some("csv") => Self::csv(body),
            _ => Self::text(body),
        }
    }

    /// Override the status code (the reason phrase follows unless
    /// [`Response::with_reason`] is used afterwards).
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self.reason = reason_phrase(status).to_string();
        self
    }

    /// Override the reason phrase.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = reason.into();
        self
    }

    /// Append a response header (e.g. a `Content-Range` for a paging test).
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Replace the body.
    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// Stall this long before replying.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

/// Reason phrases for the handful of statuses a source test actually uses.
fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Status",
    }
}

// ── Recorded requests ────────────────────────────────────────────────────────

/// Everything the stub saw for one request. The point of the harness: a test
/// asserts on this rather than trusting that a provider "probably" sent its
/// credentials.
#[derive(Clone, Debug)]
pub struct RecordedRequest {
    /// Uppercased method, e.g. `GET`.
    pub method: String,
    /// Raw request target, query string included.
    pub target: String,
    /// Path portion of the target.
    pub path: String,
    /// Query portion of the target, without the `?`.
    pub query: Option<String>,
    /// Protocol version token, e.g. `HTTP/1.1`.
    pub version: String,
    /// Request headers with **lowercased** names, so lookups are stable.
    pub headers: BTreeMap<String, String>,
    /// Request body (empty when no `Content-Length` was sent).
    pub body: Vec<u8>,
}

impl RecordedRequest {
    /// Look a header up case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }

    /// The body as UTF-8, lossily — bodies in these tests are always text.
    pub fn body_string(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Percent-decoded value of a query parameter.
    pub fn query_param(&self, key: &str) -> Option<String> {
        let q = self.query.as_deref()?;
        q.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(k) == key).then(|| percent_decode(v))
        })
    }
}

/// Minimal `application/x-www-form-urlencoded` decoding — enough for asserting
/// on query strings without pulling in a URL crate.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 3 <= b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b[i]);
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ── Routing ──────────────────────────────────────────────────────────────────

/// One route plus its hit counter. A route with several responses walks them in
/// order and then repeats the last one, which is how a retry/backoff test gets
/// "503 then 200" without any scheduling machinery.
struct RouteState {
    method: Option<String>,
    path: String,
    responses: Vec<Response>,
    hits: AtomicUsize,
}

impl RouteState {
    fn matches(&self, req: &RecordedRequest) -> bool {
        if self.path != req.path {
            return false;
        }
        match &self.method {
            Some(m) => m == &req.method,
            None => true,
        }
    }

    fn next_response(&self) -> Response {
        let i = self.hits.fetch_add(1, Ordering::SeqCst);
        let last = self.responses.len() - 1;
        self.responses[i.min(last)].clone()
    }
}

/// Builder for a [`StubServer`]. Nothing binds a socket until [`start`] is
/// called.
///
/// [`start`]: StubServerBuilder::start
pub struct StubServerBuilder {
    routes: Vec<RouteState>,
    fallback: Response,
}

impl StubServerBuilder {
    /// Serve `response` for any method on `path`.
    pub fn route(self, path: impl Into<String>, response: Response) -> Self {
        self.push_route(None, path, vec![response])
    }

    /// Serve `response` only when the method matches — the way to prove a
    /// provider used `POST` rather than `GET`.
    pub fn route_method(
        self,
        method: &str,
        path: impl Into<String>,
        response: Response,
    ) -> Self {
        self.push_route(Some(method.to_ascii_uppercase()), path, vec![response])
    }

    /// Serve `responses` in order on successive hits, repeating the last one
    /// once they are exhausted.
    pub fn route_sequence(self, path: impl Into<String>, responses: Vec<Response>) -> Self {
        assert!(
            !responses.is_empty(),
            "route_sequence needs at least one response; use `route` for a single reply"
        );
        self.push_route(None, path, responses)
    }

    /// Reply used for any path with no route. Defaults to `404 Not Found`.
    pub fn fallback(mut self, response: Response) -> Self {
        self.fallback = response;
        self
    }

    fn push_route(
        mut self,
        method: Option<String>,
        path: impl Into<String>,
        responses: Vec<Response>,
    ) -> Self {
        self.routes.push(RouteState {
            method,
            path: path.into(),
            responses,
            hits: AtomicUsize::new(0),
        });
        self
    }

    /// Bind an ephemeral port and start accepting. Panics if the loopback
    /// interface is unavailable, which is a broken machine, not a test failure.
    pub fn start(self) -> StubServer {
        let listener = TcpListener::bind("127.0.0.1:0")
            .expect("bind a stub server on an ephemeral loopback port");
        let addr = listener.local_addr().expect("read the stub server's bound address");
        listener
            .set_nonblocking(true)
            .expect("put the stub listener in non-blocking mode");

        let routes = Arc::new(self.routes);
        let fallback = Arc::new(self.fallback);
        let requests: Arc<Mutex<Vec<RecordedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let acceptor = {
            let routes = Arc::clone(&routes);
            let fallback = Arc::clone(&fallback);
            let requests = Arc::clone(&requests);
            let shutdown = Arc::clone(&shutdown);
            thread::spawn(move || {
                let mut conns: Vec<JoinHandle<()>> = Vec::new();
                while !shutdown.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _peer)) => {
                            // On Windows an accepted socket inherits the
                            // listener's non-blocking mode; the handler wants a
                            // blocking socket with timeouts instead.
                            let _ = stream.set_nonblocking(false);
                            let routes = Arc::clone(&routes);
                            let fallback = Arc::clone(&fallback);
                            let requests = Arc::clone(&requests);
                            conns.push(thread::spawn(move || {
                                serve_connection(stream, &routes, &fallback, &requests);
                            }));
                            // Reap finished handlers so a long-lived server does
                            // not accumulate them.
                            conns.retain(|h| !h.is_finished());
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(ACCEPT_POLL);
                        }
                        Err(_) => break,
                    }
                }
                // Drain in-flight handlers; each is bounded by SERVER_IO_TIMEOUT.
                for h in conns {
                    let _ = h.join();
                }
                drop(listener);
            })
        };

        StubServer { addr, shutdown, requests, acceptor: Some(acceptor) }
    }
}

// ── The server ───────────────────────────────────────────────────────────────

/// A local HTTP stub. Shuts down when dropped, so a test never has to remember
/// to clean up.
pub struct StubServer {
    addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    acceptor: Option<JoinHandle<()>>,
}

impl StubServer {
    /// Start describing a server.
    pub fn builder() -> StubServerBuilder {
        StubServerBuilder {
            routes: Vec::new(),
            fallback: Response::status(404).with_body("no stub route for this path"),
        }
    }

    /// The bound address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The OS-assigned port.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Base URL, no trailing slash — e.g. `http://127.0.0.1:52413`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Absolute URL for a path — what a `SourceConfig::endpoint` is set to.
    pub fn url_for(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{}", self.url(), path)
        } else {
            format!("{}/{}", self.url(), path)
        }
    }

    /// Every request received so far, in arrival order.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("stub request log is not poisoned").clone()
    }

    /// The most recent request.
    pub fn last_request(&self) -> Option<RecordedRequest> {
        self.requests().pop()
    }

    /// How many requests arrived.
    pub fn request_count(&self) -> usize {
        self.requests.lock().expect("stub request log is not poisoned").len()
    }

    /// Requests whose path matches exactly.
    pub fn requests_for(&self, path: &str) -> Vec<RecordedRequest> {
        self.requests().into_iter().filter(|r| r.path == path).collect()
    }

    /// Block until at least `n` requests have arrived. Returns `false` on
    /// timeout rather than panicking, so the caller can produce a better
    /// message. Bounded, so it can never hang the run.
    pub fn wait_for_requests(&self, n: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.request_count() >= n {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Forget the recorded requests, for a test that reuses one server across
    /// phases.
    pub fn clear_requests(&self) {
        self.requests.lock().expect("stub request log is not poisoned").clear();
    }

    /// Stop accepting and join the worker threads. Idempotent; [`Drop`] calls
    /// it, so an explicit call is only needed when a test wants to assert that
    /// the port really is closed afterwards.
    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(h) = self.acceptor.take() {
            let _ = h.join();
        }
    }
}

impl Drop for StubServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Read one request, record it, and write the matching canned reply.
fn serve_connection(
    mut stream: TcpStream,
    routes: &[RouteState],
    fallback: &Response,
    log: &Mutex<Vec<RecordedRequest>>,
) {
    let _ = stream.set_read_timeout(Some(SERVER_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(SERVER_IO_TIMEOUT));

    let req = match read_request(&stream) {
        Ok(r) => r,
        // A half-open probe (a bare connect, or a client that vanished) is not
        // a request; drop it silently rather than poisoning the log.
        Err(_) => return,
    };

    let response = routes
        .iter()
        .find(|r| r.matches(&req))
        .map(|r| r.next_response())
        .unwrap_or_else(|| fallback.clone());

    if let Ok(mut guard) = log.lock() {
        guard.push(req);
    }

    if !response.delay.is_zero() {
        thread::sleep(response.delay);
    }
    let _ = write_response(&mut stream, &response);
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

fn read_request(stream: &TcpStream) -> io::Result<RecordedRequest> {
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "connection closed before a request line arrived",
        ));
    }
    let mut parts = line.trim_end().split_whitespace();
    let method = parts.next().unwrap_or_default().to_ascii_uppercase();
    let target = parts.next().unwrap_or("/").to_string();
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();

    let mut headers = BTreeMap::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 {
            break;
        }
        let t = h.trim_end_matches(['\r', '\n']);
        if t.is_empty() {
            break;
        }
        if let Some((k, v)) = t.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let len = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut body)?;
    }

    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (target.clone(), None),
    };

    Ok(RecordedRequest { method, target, path, query, version, headers, body })
}

fn write_response(stream: &mut TcpStream, r: &Response) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", r.status, r.reason);
    for (k, v) in &r.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n", r.body.len()));
    // One request per connection: no keep-alive bookkeeping, and the bundled
    // client can simply read to EOF.
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&r.body)?;
    stream.flush()
}

// ── A minimal blocking client ────────────────────────────────────────────────
//
// Used by the self-tests below, and available to any test that needs to poke
// the stub directly (for instance to prove a route is wired before handing the
// URL to a provider). Providers under test use their own transport.

/// A reply parsed by the bundled client.
#[derive(Clone, Debug)]
pub struct ClientResponse {
    /// HTTP status code.
    pub status: u16,
    /// Reason phrase as sent.
    pub reason: String,
    /// Response headers with lowercased names.
    pub headers: BTreeMap<String, String>,
    /// Response body.
    pub body: Vec<u8>,
}

impl ClientResponse {
    /// The body as UTF-8, lossily.
    pub fn body_string(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Look a response header up case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }
}

/// `GET url` with no extra headers.
pub fn http_get(url: &str) -> io::Result<ClientResponse> {
    http_request("GET", url, &[], None)
}

/// Issue one request over a fresh connection. Plain `http://` only — the stub
/// speaks no TLS, deliberately, so no test can accidentally reach the internet.
pub fn http_request(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> io::Result<ClientResponse> {
    let (authority, target) = split_url(url)?;
    let addr = authority
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{authority}` did not resolve to an address"),
            )
        })?;

    let stream = TcpStream::connect_timeout(&addr, CLIENT_IO_TIMEOUT)?;
    stream.set_read_timeout(Some(CLIENT_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(CLIENT_IO_TIMEOUT))?;

    let mut writer = stream.try_clone()?;
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        method.to_ascii_uppercase(),
        target,
        authority
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some(b) = body {
        head.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    head.push_str("\r\n");
    writer.write_all(head.as_bytes())?;
    if let Some(b) = body {
        writer.write_all(b)?;
    }
    writer.flush()?;

    let mut raw = Vec::new();
    let mut reader = stream;
    reader.read_to_end(&mut raw)?;
    parse_client_response(&raw)
}

fn split_url(url: &str) -> io::Result<(String, String)> {
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the stub client speaks plain http:// only, got `{url}`"),
        )
    })?;
    Ok(match rest.find('/') {
        Some(i) => (rest[..i].to_string(), rest[i..].to_string()),
        None => (rest.to_string(), "/".to_string()),
    })
}

fn parse_client_response(raw: &[u8]) -> io::Result<ClientResponse> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "response had no header block"))?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let body = raw[split + 4..].to_vec();

    let mut lines = head.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "response had no status line"))?;
    let mut sp = status_line.splitn(3, ' ');
    let _version = sp.next().unwrap_or_default();
    let status = sp.next().unwrap_or("0").parse::<u16>().unwrap_or(0);
    let reason = sp.next().unwrap_or_default().to_string();

    let mut headers = BTreeMap::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    Ok(ClientResponse { status, reason, headers, body })
}

// ── Self-tests: the harness proving it does what the other suites assume ─────

#[cfg(test)]
mod harness_self_tests {
    use super::*;

    #[test]
    fn serves_a_canned_route_on_an_ephemeral_port() {
        let server = StubServer::builder()
            .route("/v1/readings", Response::json(fixture(FIXTURE_REST_ARRAY)))
            .start();

        assert_ne!(server.port(), 0, "the OS must hand back a real bound port");

        let res = http_get(&server.url_for("/v1/readings")).expect("stub answered");
        assert_eq!(res.status, 200);
        assert_eq!(res.reason, "OK");
        assert_eq!(
            res.header("content-type"),
            Some("application/json; charset=utf-8"),
            "content type must reach the client verbatim"
        );
        assert_eq!(
            res.body_string(),
            fixture(FIXTURE_REST_ARRAY),
            "the body must be the fixture byte-for-byte"
        );
    }

    #[test]
    fn two_servers_get_distinct_ports() {
        let a = StubServer::builder().route("/x", Response::text("a")).start();
        let b = StubServer::builder().route("/x", Response::text("b")).start();
        assert_ne!(a.port(), b.port(), "ephemeral ports must not collide");
        assert_eq!(http_get(&a.url_for("/x")).unwrap().body_string(), "a");
        assert_eq!(http_get(&b.url_for("/x")).unwrap().body_string(), "b");
    }

    #[test]
    fn records_method_path_query_headers_and_body() {
        let server = StubServer::builder()
            .route("/rest/v1/readings", Response::json(fixture(FIXTURE_POSTGREST_ROWS)))
            .start();

        let body = br#"{"query":"{ readings { sensor } }"}"#;
        let res = http_request(
            "post",
            &server.url_for("/rest/v1/readings?select=*&order=recorded_at.desc&limit=100"),
            &[
                ("Authorization", "Bearer test-token"),
                ("apikey", "anon-key"),
                ("Content-Type", "application/json"),
            ],
            Some(body),
        )
        .expect("stub answered");
        assert_eq!(res.status, 200);

        let req = server.last_request().expect("the request was recorded");
        assert_eq!(req.method, "POST", "the method is normalized to uppercase");
        assert_eq!(req.path, "/rest/v1/readings");
        assert_eq!(req.query.as_deref(), Some("select=*&order=recorded_at.desc&limit=100"));

        // The assertion every provider suite depends on: credentials really went
        // over the wire, and header lookup ignores case.
        assert_eq!(req.header("authorization"), Some("Bearer test-token"));
        assert_eq!(req.header("AUTHORIZATION"), Some("Bearer test-token"));
        assert_eq!(req.header("apikey"), Some("anon-key"));
        assert_eq!(req.header("x-not-sent"), None);

        assert_eq!(req.query_param("limit").as_deref(), Some("100"));
        assert_eq!(req.query_param("order").as_deref(), Some("recorded_at.desc"));
        assert_eq!(req.query_param("missing"), None);

        assert_eq!(req.body_string(), String::from_utf8_lossy(body));
    }

    #[test]
    fn percent_encoded_query_values_decode() {
        let server = StubServer::builder().route("/q", Response::text("ok")).start();
        http_get(&server.url_for("/q?filter=pressure%20%3E%3D%2015&name=bay+a")).unwrap();
        let req = server.last_request().unwrap();
        assert_eq!(req.query_param("filter").as_deref(), Some("pressure >= 15"));
        assert_eq!(req.query_param("name").as_deref(), Some("bay a"));
    }

    #[test]
    fn unrouted_paths_hit_the_fallback_and_are_still_recorded() {
        let server = StubServer::builder().route("/known", Response::text("hi")).start();

        let res = http_get(&server.url_for("/unknown")).expect("stub answered");
        assert_eq!(res.status, 404, "the default fallback is 404");
        assert_eq!(
            server.requests_for("/unknown").len(),
            1,
            "an unrouted request must still be recorded, so a test can see the typo"
        );

        let custom = StubServer::builder()
            .fallback(Response::status(503).with_body("maintenance"))
            .start();
        let res = http_get(&custom.url_for("/anything")).expect("stub answered");
        assert_eq!(res.status, 503);
        assert_eq!(res.body_string(), "maintenance");
    }

    #[test]
    fn error_statuses_and_method_scoped_routes_work() {
        let server = StubServer::builder()
            .route("/boom", Response::status(500).with_body("upstream exploded"))
            .route("/unauthorized", Response::status(401))
            .route_method("POST", "/graphql", Response::json(fixture(FIXTURE_GRAPHQL_RESPONSE)))
            .start();

        let boom = http_get(&server.url_for("/boom")).unwrap();
        assert_eq!(boom.status, 500);
        assert_eq!(boom.reason, "Internal Server Error");
        assert_eq!(boom.body_string(), "upstream exploded");

        assert_eq!(http_get(&server.url_for("/unauthorized")).unwrap().status, 401);

        // GET on a POST-only route falls through to the fallback — that is how a
        // test proves a provider used the right verb.
        assert_eq!(http_get(&server.url_for("/graphql")).unwrap().status, 404);
        let posted = http_request("POST", &server.url_for("/graphql"), &[], Some(b"{}")).unwrap();
        assert_eq!(posted.status, 200);
        assert!(posted.body_string().contains("\"readings\""));
    }

    #[test]
    fn a_route_sequence_walks_its_responses_then_repeats_the_last() {
        let server = StubServer::builder()
            .route_sequence(
                "/flaky",
                vec![
                    Response::status(503).with_body("try again"),
                    Response::status(429).with_body("slow down"),
                    Response::json("[]"),
                ],
            )
            .start();

        let codes: Vec<u16> = (0..5)
            .map(|_| http_get(&server.url_for("/flaky")).unwrap().status)
            .collect();
        assert_eq!(
            codes,
            vec![503, 429, 200, 200, 200],
            "a sequence must advance per hit and then hold the final response"
        );
        assert_eq!(server.request_count(), 5);
    }

    #[test]
    fn concurrent_requests_are_all_served_and_all_recorded() {
        const N: usize = 12;
        let server = StubServer::builder()
            .route("/rows", Response::json(fixture(FIXTURE_POSTGREST_ROWS)))
            .start();

        let url = server.url_for("/rows");
        let workers: Vec<_> = (0..N)
            .map(|i| {
                let url = url.clone();
                thread::spawn(move || {
                    let tag = format!("worker-{i}");
                    let res = http_request("GET", &url, &[("X-Worker", tag.as_str())], None)
                        .unwrap_or_else(|e| panic!("{tag} failed: {e}"));
                    assert_eq!(res.status, 200, "{tag} got a non-200");
                    tag
                })
            })
            .collect();
        let mut tags: Vec<String> = workers.into_iter().map(|h| h.join().unwrap()).collect();
        tags.sort();

        assert!(
            server.wait_for_requests(N, Duration::from_secs(5)),
            "only {} of {N} concurrent requests were recorded",
            server.request_count()
        );
        let mut seen: Vec<String> = server
            .requests()
            .iter()
            .filter_map(|r| r.header("x-worker").map(str::to_string))
            .collect();
        seen.sort();
        assert_eq!(seen, tags, "every concurrent request must appear in the log exactly once");
    }

    #[test]
    fn a_delayed_response_still_arrives_and_does_not_hang() {
        let server = StubServer::builder()
            .route("/slow", Response::text("eventually").with_delay(Duration::from_millis(120)))
            .start();

        let started = Instant::now();
        let res = http_get(&server.url_for("/slow")).expect("stub answered");
        let elapsed = started.elapsed();

        assert_eq!(res.body_string(), "eventually");
        assert!(elapsed >= Duration::from_millis(100), "the delay was not applied: {elapsed:?}");
        assert!(elapsed < CLIENT_IO_TIMEOUT, "a delayed reply must not reach the client timeout");
    }

    #[test]
    fn clear_requests_resets_the_log_without_stopping_the_server() {
        let server = StubServer::builder().route("/ping", Response::text("pong")).start();
        http_get(&server.url_for("/ping")).unwrap();
        assert_eq!(server.request_count(), 1);
        server.clear_requests();
        assert_eq!(server.request_count(), 0);
        http_get(&server.url_for("/ping")).unwrap();
        assert_eq!(server.request_count(), 1, "the server keeps serving after a log reset");
    }

    #[test]
    fn wait_for_requests_times_out_instead_of_blocking_forever() {
        let server = StubServer::builder().route("/never", Response::text("x")).start();
        let started = Instant::now();
        assert!(
            !server.wait_for_requests(1, Duration::from_millis(80)),
            "no request was made, so the wait must report failure"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the bounded wait must return promptly, not hang the run"
        );
    }

    #[test]
    fn stop_closes_the_port_and_is_idempotent() {
        let mut server = StubServer::builder().route("/ping", Response::text("pong")).start();
        let addr = server.addr();
        assert_eq!(http_get(&server.url_for("/ping")).unwrap().body_string(), "pong");

        server.stop();
        server.stop(); // idempotent: the second call must not panic or hang.

        // The listener is closed, so either the connect is refused or nothing
        // ever answers. Both are "shut down"; neither may hang.
        let started = Instant::now();
        let after = http_get(&format!("http://{addr}/ping"));
        assert!(after.is_err(), "the stub kept serving after stop(): {after:?}");
        assert!(
            started.elapsed() < CLIENT_IO_TIMEOUT + Duration::from_secs(1),
            "a post-shutdown request must fail fast"
        );
    }

    #[test]
    fn dropping_the_server_shuts_it_down() {
        let addr = {
            let server = StubServer::builder().route("/ping", Response::text("pong")).start();
            let addr = server.addr();
            assert_eq!(http_get(&server.url_for("/ping")).unwrap().status, 200);
            addr
        };
        assert!(
            http_get(&format!("http://{addr}/ping")).is_err(),
            "Drop must join the accept loop and release the port"
        );
    }

    #[test]
    fn the_client_refuses_anything_but_plain_http() {
        let err = http_get("https://api.example.com/x").expect_err("https must be rejected");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(
            err.to_string().contains("http://"),
            "the error must say why: {err}"
        );
    }

    #[test]
    fn every_fixture_is_present_and_has_the_shape_its_name_promises() {
        for name in ALL_FIXTURES {
            let path = fixture_path(name);
            assert!(path.is_file(), "missing fixture {}", path.display());
            assert!(!fixture(name).trim().is_empty(), "fixture {name} is empty");
        }

        // Firestore: typed field wrappers, a null, and a paging token.
        let fs = fixture(FIXTURE_FIRESTORE_DOCUMENTS);
        assert!(fs.contains("\"documents\""));
        assert!(fs.contains("\"stringValue\"") && fs.contains("\"doubleValue\""));
        assert!(fs.contains("\"integerValue\"") && fs.contains("\"booleanValue\""));
        assert!(fs.contains("\"nullValue\""), "a null cell must be exercised");
        assert!(fs.contains("\"nextPageToken\""));

        // PostgREST / plain REST: bare arrays of row objects, with nulls.
        for name in [FIXTURE_POSTGREST_ROWS, FIXTURE_REST_ARRAY] {
            let body = fixture(name);
            assert!(body.trim_start().starts_with('['), "{name} must be a bare array");
            assert!(body.contains("null"), "{name} must exercise a null cell");
        }

        // Nested: the rows are NOT at the root, so a json_path is mandatory.
        let nested = fixture(FIXTURE_NESTED_PAYLOAD);
        assert!(nested.trim_start().starts_with('{'));
        assert!(nested.contains("\"results\"") && nested.contains("\"items\""));
        assert!(
            !nested.trim_start().starts_with('['),
            "the nested fixture must not be reachable without a json_path"
        );

        // GraphQL: rows under `data`.
        assert!(fixture(FIXTURE_GRAPHQL_RESPONSE).contains("\"data\""));

        // CSV: header + 4 rows, a missing number and a missing string.
        let csv = fixture(FIXTURE_READINGS_CSV);
        let lines: Vec<&str> = csv.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 5, "header plus four rows");
        assert!(lines[0].contains("time (s)"), "the header carries unit symbols");
        assert!(lines[3].contains(",,"), "a missing numeric cell must be present");
        assert!(lines[4].ends_with(','), "a missing trailing string cell must be present");
    }

    #[test]
    fn fixtures_can_be_served_straight_from_disk() {
        let server = StubServer::builder()
            .route("/firestore", Response::fixture(FIXTURE_FIRESTORE_DOCUMENTS))
            .route("/csv", Response::fixture(FIXTURE_READINGS_CSV))
            .start();

        let json = http_get(&server.url_for("/firestore")).unwrap();
        assert_eq!(json.header("content-type"), Some("application/json; charset=utf-8"));
        assert_eq!(json.body_string(), fixture(FIXTURE_FIRESTORE_DOCUMENTS));

        let csv = http_get(&server.url_for("/csv")).unwrap();
        assert_eq!(csv.header("content-type"), Some("text/csv; charset=utf-8"));
        assert_eq!(csv.body_string(), fixture(FIXTURE_READINGS_CSV));
    }

    #[test]
    fn a_local_fixture_path_is_a_valid_csv_source_endpoint() {
        // The CSV provider takes a path, not a URL — the harness hands it one
        // that exists, and the crate's own validator agrees with it.
        use eustress_data::source::{validate_config, SourceConfig, SourceKind};

        let path = fixture_path(FIXTURE_READINGS_CSV);
        assert!(path.is_file());
        let cfg = SourceConfig::new(SourceKind::Csv, path.to_string_lossy().to_string());
        validate_config(&cfg).expect("a real fixture path must validate as a CSV endpoint");
    }

    #[test]
    fn a_stub_url_is_a_valid_rest_source_endpoint() {
        use eustress_data::source::{validate_config, SourceConfig, SourceKind};

        let server = StubServer::builder()
            .route("/v1/readings", Response::json(fixture(FIXTURE_REST_ARRAY)))
            .start();
        let cfg = SourceConfig::new(SourceKind::Rest, server.url_for("/v1/readings"));
        validate_config(&cfg).expect("a loopback stub URL must validate as a REST endpoint");
    }
}

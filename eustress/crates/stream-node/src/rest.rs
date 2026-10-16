//! REST + SSE HTTP API for EustressStream nodes.
//!
//! Endpoints:
//!   POST /topics/{name}/publish          — publish raw bytes, returns {"offset":N}
//!   GET  /topics/{name}/stream           — SSE live message feed (JSON-encoded)
//!   GET  /topics/{name}/replay/{from}    replay the ring buffer from offset {from} via SSE
//!   GET  /topics                         — list topics with stats
//!   GET  /health                         — node health and uptime
//!
//! The API is served over TLS 1.3 only. Every endpoint above requires
//! `Authorization: Bearer <token>`, and each party is held to its grants: a
//! publish needs a publish grant, a stream or a replay a subscribe grant (a
//! stream also counts against the party's subscription quota), and the topic
//! list shows only the topics the party holds a grant on. A bearer route
//! takes no query parameters at all: its allowlist is empty, and the query is
//! percent-decoded before the check, so a token in a query string, under any
//! spelling, is refused.
//!
//! A token is bound to its party and to the key the party logged in with. An
//! SSE stream ends at its token's `expires_at`, when its party is revoked or
//! enrolled under another key, and at the first reload that removes its grant
//! for the topic. Every write to a REST peer has the 10-second write deadline,
//! so a reader that stops reading holds no subscription or quota slot.
//!
//! Login routes (no token):
//!   POST /v1/login/challenge   returns {"nonce": "<base64, 32 bytes>", "node_id": "..."}
//!   POST /v1/login             takes {"party_id", "nonce", "token_class": "session" | "browser", "signature"}
//!                              returns {"token": "<base64, 32 bytes>", "expires_in": N}
//!
//! The login signature covers the REST transcript, which carries the SHA-256
//! of the node's own DER certificate, so only a native client holding the pin
//! can log in. Browsers never log in and never hold a party key: a browser
//! uses a `browser` token (15 minutes) that a native login minted, sent in the
//! `Authorization` header from `fetch` (not `EventSource`, which cannot set
//! headers).
//!
//! Reserved topics (`scene_deltas`, `agent_commands`) also need `admin = true`
//! to publish. The materializer that writes a Space to disk keeps its own
//! `space://` path checks on whatever a scene delta asks it to write.

use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::connect_info::Connected;
use axum::extract::{Path, State};
use axum::extract::{ConnectInfo, Request};
use axum::http::StatusCode;
use axum::http::{header, HeaderMap};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Json};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use axum::Extension;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::AbortHandle;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tracing::{debug, info, warn};

use eustress_stream::{EustressStream, OwnedMessage, SubscriberId};

use crate::auth::{
    self, truncate_for_log, Channel, ChallengeRefusal, NodeAuth, Party, TokenClass, TokenRefusal,
    NONCE_LEN, PROTOCOL_VERSION, REASON_AUTH_FAILED,
};
use crate::config::NodeConfig;
use crate::error::NodeError;
use crate::protocol::TopicStats;

// ─────────────────────────────────────────────────────────────────────────────
// State
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RestState {
    pub stream: EustressStream,
    pub start_time: Arc<Instant>,
    pub node_id: String,
    pub tcp_port: u16,
    pub rest_port: u16,
    /// The node's authentication state, shared with its TCP listener.
    pub auth: Arc<NodeAuth>,
}

impl RestState {
    pub fn new(stream: EustressStream, auth: Arc<NodeAuth>, tcp_port: u16, rest_port: u16) -> Self {
        RestState {
            stream,
            start_time: Arc::new(Instant::now()),
            node_id: auth.node_id().to_string(),
            tcp_port,
            rest_port,
            auth,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

/// The routes, with the bearer middleware on every route but the two login
/// routes. Crate-private: the router is only ever served over TLS, by
/// [`RestServer`].
pub(crate) fn build_router(state: RestState) -> Router {
    let protected = Router::new()
        .route("/topics/{name}/publish", post(publish_handler))
        .route("/topics/{name}/stream", get(stream_handler))
        .route("/topics/{name}/replay/{from}", get(replay_handler))
        .route("/topics", get(list_topics_handler))
        .route("/health", get(health_handler))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_bearer));

    Router::new()
        .route("/v1/login/challenge", post(challenge_handler))
        .route("/v1/login", post(login_handler))
        .merge(protected)
        .with_state(state)
}

/// CORS policy for the REST API.
///
/// Every route but the two login routes needs a bearer token, sent in the
/// `Authorization` header, and a browser sends that header across origins
/// only where this policy allows it. A wildcard `Access-Control-Allow-Origin`
/// would give any page the operator happens to visit the reach a dashboard
/// has. The policy is therefore restricted to the loopback origins a local
/// dashboard is actually served from, and to the methods and headers those
/// endpoints use, `authorization` among them.
fn cors_layer() -> tower_http::cors::CorsLayer {
    use axum::http::{header, HeaderValue, Method};

    // Loopback origins across the ports a local dev dashboard typically uses.
    // A non-browser client (curl, the CLI, an agent) sends no `Origin` header
    // and is unaffected by this list.
    let origins: Vec<HeaderValue> = [
        "http://localhost:1420",
        "http://127.0.0.1:1420",
        "http://localhost:3000",
        "http://127.0.0.1:3000",
        "http://localhost:8080",
        "http://127.0.0.1:8080",
    ]
    .iter()
    .filter_map(|o| HeaderValue::from_str(o).ok())
    .collect();

    tower_http::cors::CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE, header::ACCEPT, header::AUTHORIZATION])
}

// ─────────────────────────────────────────────────────────────────────────────
// TLS serving
// ─────────────────────────────────────────────────────────────────────────────

/// A byte stream the REST server can run TLS over.
pub trait AsyncIo: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> AsyncIo for T {}

type TlsIo = tokio_rustls::server::TlsStream<Box<dyn AsyncIo>>;

/// A REST connection whose every write must finish within the write
/// deadline. A write still waiting on the peer at the deadline fails with
/// `TimedOut`, which ends the connection, its streams, and the subscriptions
/// and quota slots they hold, so a reader that stops reading holds nothing.
pub struct DeadlineIo<T> {
    inner: T,
    /// Running while a write waits on the peer.
    stalled: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl<T> DeadlineIo<T> {
    fn new(inner: T) -> Self {
        DeadlineIo { inner, stalled: None }
    }

    /// Pass `result` through. A write that cannot go on now starts the
    /// deadline, unless it is already running; one that goes on stops it.
    fn within_deadline<R>(&mut self, result: Poll<io::Result<R>>, cx: &mut Context<'_>) -> Poll<io::Result<R>> {
        if result.is_ready() {
            self.stalled = None;
            return result;
        }
        let deadline = self.stalled.get_or_insert_with(|| Box::pin(tokio::time::sleep(auth::WRITE_DEADLINE)));
        match deadline.as_mut().poll(cx) {
            Poll::Ready(()) => {
                self.stalled = None;
                Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "a write to the REST peer did not finish within 10 seconds",
                )))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for DeadlineIo<T> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for DeadlineIo<T> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write(cx, buf);
        this.within_deadline(result, cx)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_flush(cx);
        this.within_deadline(result, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_shutdown(cx);
        this.within_deadline(result, cx)
    }
}

/// The source address of a REST connection.
#[derive(Clone, Copy, Debug)]
pub struct PeerAddr(pub SocketAddr);

/// Hands axum the connections whose TLS handshake has finished.
pub struct TlsListener {
    ready: mpsc::Receiver<(DeadlineIo<TlsIo>, PeerAddr)>,
    local: PeerAddr,
}

// VERIFY: axum 0.8 serve::Listener (type Io, type Addr, accept returning
// `impl Future + Send`, local_addr), copied from axum-0.8.9/src/serve/listener.rs,
// and the Connected<IncomingStream<'_, TlsListener>> impl below that
// into_make_service_with_connect_info::<PeerAddr>() needs.
impl axum::serve::Listener for TlsListener {
    type Io = DeadlineIo<TlsIo>;
    type Addr = PeerAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(connection) => connection,
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local)
    }
}

impl Connected<axum::serve::IncomingStream<'_, TlsListener>> for PeerAddr {
    fn connect_info(stream: axum::serve::IncomingStream<'_, TlsListener>) -> Self {
        *stream.remote_addr()
    }
}

/// Runs each accepted connection's TLS handshake under the 5-second deadline
/// and the per-source limits (128 in all, 16 per source IP), then passes it
/// to axum.
struct Admission {
    acceptor: tokio_rustls::TlsAcceptor,
    auth: Arc<NodeAuth>,
    ready: mpsc::Sender<(DeadlineIo<TlsIo>, PeerAddr)>,
}

impl Admission {
    fn admit(self: &Arc<Self>, io: Box<dyn AsyncIo>, peer: SocketAddr) -> bool {
        let accepted_at = tokio::time::Instant::now();
        let Some(permit) = self.auth.rest_handshakes.try_acquire(peer.ip()) else {
            self.auth.log_refused_at_accept(peer.ip(), Channel::Rest);
            return false;
        };
        let admission = Arc::clone(self);
        tokio::spawn(async move {
            let deadline = accepted_at + auth::LOGIN_DEADLINE;
            let handshake = tokio::time::timeout_at(deadline, admission.acceptor.accept(io)).await;
            drop(permit);
            match handshake {
                Ok(Ok(tls)) => {
                    let _ = admission.ready.send((DeadlineIo::new(tls), PeerAddr(peer))).await;
                }
                Ok(Err(e)) => debug!(source = %peer, "REST TLS handshake failed: {e}"),
                Err(_) => debug!(source = %peer, "REST TLS handshake did not finish within 5 seconds"),
            }
        });
        true
    }
}

/// A running REST server. TLS 1.3 only, with the node's certificate.
pub struct RestServer {
    local_addr: Option<SocketAddr>,
    admission: Arc<Admission>,
}

impl RestServer {
    /// Bind the REST port ([`NodeConfig::effective_rest_port`] on
    /// [`NodeConfig::bind_addr`]) and serve it.
    pub async fn bind(state: RestState, config: &NodeConfig) -> Result<Self, NodeError> {
        let addr = SocketAddr::new(config.bind_addr, config.effective_rest_port());
        let listener = TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        let server = Self::start(state, Some(local));
        let admission = Arc::clone(&server.admission);
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, peer)) => {
                        let _ = socket.set_nodelay(true);
                        admission.admit(Box::new(socket), peer);
                    }
                    Err(e) => warn!("REST accept error: {e}"),
                }
            }
        });
        info!("EustressStream REST API on https://{local} (TLS 1.3, bearer tokens)");
        Ok(server)
    }

    /// A server with no listener of its own; it serves the byte streams
    /// handed to [`RestServer::serve_io`].
    pub fn unbound(state: RestState) -> Self {
        Self::start(state, None)
    }

    fn start(state: RestState, local: Option<SocketAddr>) -> Self {
        let (ready_tx, ready_rx) = mpsc::channel(64);
        let admission = Arc::new(Admission {
            acceptor: state.auth.tls().rest_acceptor(),
            auth: Arc::clone(&state.auth),
            ready: ready_tx,
        });
        let listener = TlsListener {
            ready: ready_rx,
            local: PeerAddr(local.unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 0)))),
        };
        let app = build_router(state)
            .layer(cors_layer())
            // Spans carry the method and path only: never the query string,
            // which is where a misplaced token would be.
            .layer(tower_http::trace::TraceLayer::new_for_http().make_span_with(|request: &Request| {
                tracing::debug_span!("rest", method = %request.method(), path = %request.uri().path())
            }));
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app.into_make_service_with_connect_info::<PeerAddr>()).await {
                tracing::error!("REST server error: {e}");
            }
        });
        RestServer { local_addr: local, admission }
    }

    /// The bound address, when this server has a listener.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Serve one connection from `peer` over `io`: TLS 1.3, then HTTP.
    /// Returns `false` when the per-source limit refused it.
    pub fn serve_io<IO: AsyncIo>(&self, io: IO, peer: SocketAddr) -> bool {
        self.admission.admit(Box::new(io), peer)
    }
}

/// Start the REST server on the configured REST port. Runs until the process exits.
///
/// Binds [`NodeConfig::bind_addr`] (loopback by default), over TLS 1.3 with
/// the node's certificate. Every route but the login routes requires a
/// bearer token, and every party is held to its grants.
pub async fn serve_rest(state: RestState, config: &NodeConfig) -> Result<(), NodeError> {
    let _server = RestServer::bind(state, config).await?;
    std::future::pending::<()>().await;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Bearer middleware
// ─────────────────────────────────────────────────────────────────────────────

/// The party a request's bearer token belongs to, and when the token
/// expires, set by the bearer middleware for every protected route.
#[derive(Clone)]
pub struct AuthedParty(pub Arc<Party>, pub tokio::time::Instant);

/// Query parameters a bearer route accepts: none. Every bearer route's
/// explicit allowlist is empty, so a token misplaced into a URL, which ends
/// up in logs and browser history, fails loudly, whatever it is called.
const BEARER_QUERY_ALLOWLIST: [&str; 0] = [];

/// `true` when a bearer route must refuse this query string: once
/// percent-decoded, it names a parameter outside the allowlist, or it does
/// not decode at all.
fn refuses_query(query: Option<&str>) -> bool {
    let Some(query) = query else { return false };
    query.split(['&', ';']).filter(|pair| !pair.is_empty()).any(|pair| {
        let name = pair.split('=').next().unwrap_or("");
        match percent_decode(name) {
            Some(name) => !BEARER_QUERY_ALLOWLIST.contains(&name.as_str()),
            None => true,
        }
    })
}

/// Decode `%XX` escapes and `+` in a query component. `None` for a
/// malformed escape, or bytes that are not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3)?;
                if !hex.iter().all(u8::is_ascii_hexdigit) {
                    return None;
                }
                out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// The token from `Authorization: Bearer <token>`.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

async fn require_bearer(State(state): State<RestState>, mut request: Request, next: Next) -> Response {
    if refuses_query(request.uri().query()) {
        return refusal(
            StatusCode::BAD_REQUEST,
            "this route takes no query parameters; a token goes only in the Authorization header",
        )
        .into_response();
    }
    let Some(token) = bearer_token(request.headers()) else {
        return refusal(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    };
    let Some(grant) = state.auth.tokens.validate(token, tokio::time::Instant::now()) else {
        return refusal(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    };
    // The token's party must still be enabled, under the key it logged in with.
    let set = state.auth.party_set();
    let Some(party) = set
        .active(&grant.party_id)
        .filter(|_| set.holds_key(&grant.party_id, &grant.key_fingerprint))
        .cloned()
    else {
        return refusal(StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    };
    request.extensions_mut().insert(AuthedParty(party, grant.expires_at));
    next.run(request).await
}

/// An error reply.
fn refusal(status: StatusCode, error: &str) -> (StatusCode, Json<ErrorResponse>) {
    (status, Json(ErrorResponse { error: error.to_string() }))
}

/// A refusal by a node limit: 429 or 503, with `Retry-After: 1`. Native
/// clients retry it under their retry rule.
fn limit_refusal(status: StatusCode, error: &str) -> Response {
    (status, [(header::RETRY_AFTER, "1")], Json(ErrorResponse { error: error.to_string() })).into_response()
}

/// The one login refusal: 401 with `auth_failed`, whatever the cause, so a
/// reply never tells which check failed.
fn auth_failed() -> Response {
    refusal(StatusCode::UNAUTHORIZED, REASON_AUTH_FAILED).into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// Login
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ChallengeResponse {
    nonce: String,
    node_id: String,
}

/// `POST /v1/login/challenge`. At most 16 challenges may be outstanding from
/// one source IP (429 beyond) and 4096 in all (503 beyond), each with
/// `Retry-After: 1`.
async fn challenge_handler(State(state): State<RestState>, ConnectInfo(peer): ConnectInfo<PeerAddr>) -> Response {
    match state.auth.challenges.issue(peer.0.ip(), tokio::time::Instant::now()) {
        Ok(nonce) => (
            StatusCode::OK,
            Json(ChallengeResponse { nonce: STANDARD.encode(nonce), node_id: state.auth.node_id().to_string() }),
        )
            .into_response(),
        Err(ChallengeRefusal::TooManyFromSource) => {
            limit_refusal(StatusCode::TOO_MANY_REQUESTS, "too many outstanding challenges from this address")
        }
        Err(ChallengeRefusal::Full) => limit_refusal(StatusCode::SERVICE_UNAVAILABLE, "too many outstanding challenges"),
    }
}

#[derive(Deserialize)]
struct LoginRequest {
    #[serde(default)]
    party_id: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    token_class: Option<String>,
    #[serde(default)]
    signature: Option<String>,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    expires_in: u64,
}

/// `POST /v1/login`. The nonce names the challenge: the node finds it among
/// its live challenges, from any source address, and uses it up. An unknown,
/// expired or used nonce is refused exactly as a bad signature is, after one
/// dummy signature check so the timing matches. The node then rebuilds the
/// REST transcript with its own node id and the hash of its own certificate.
async fn login_handler(State(state): State<RestState>, ConnectInfo(peer): ConnectInfo<PeerAddr>, body: Bytes) -> Response {
    let source = peer.0.ip();
    let node = &state.auth;

    let Ok(request) = serde_json::from_slice::<LoginRequest>(&body) else {
        node.log_login_failure(source, Channel::Rest, None, REASON_AUTH_FAILED, "login body is not JSON");
        return refusal(StatusCode::BAD_REQUEST, "the login body is not a JSON object").into_response();
    };
    let Some(class) = request.token_class.as_deref().and_then(TokenClass::parse) else {
        node.log_login_failure(source, Channel::Rest, None, REASON_AUTH_FAILED, "unknown token_class");
        return refusal(StatusCode::BAD_REQUEST, "token_class must be \"session\" or \"browser\"").into_response();
    };
    let signature = request
        .signature
        .as_deref()
        .and_then(|s| STANDARD.decode(s.as_bytes()).ok())
        .filter(|s| s.len() == 64);
    let Some(signature) = signature else {
        node.log_login_failure(source, Channel::Rest, None, REASON_AUTH_FAILED, "no valid signature");
        return auth_failed();
    };
    let party_id = request.party_id.unwrap_or_default();
    if !auth::is_valid_party_id(&party_id) {
        // Refused before any lookup.
        node.log_login_failure(source, Channel::Rest, None, REASON_AUTH_FAILED, "malformed party id");
        return auth_failed();
    }

    // The nonce is used up by this attempt, whatever the outcome.
    let now = tokio::time::Instant::now();
    let nonce: Option<[u8; NONCE_LEN]> = request
        .nonce
        .as_deref()
        .and_then(|n| STANDARD.decode(n.as_bytes()).ok())
        .and_then(|raw| raw.try_into().ok());
    let live = nonce.is_some_and(|n| node.challenges.take(&n, now));
    let set = node.party_set();
    let enrolled = set.get(&party_id).map(|p| p.id.clone());
    let Some(nonce) = nonce.filter(|_| live) else {
        auth::burn_signature_check(&signature);
        node.log_login_failure(source, Channel::Rest, enrolled.as_deref(), REASON_AUTH_FAILED, "unknown, expired or used nonce");
        return auth_failed();
    };
    let Some(party) = set.active(&party_id).cloned() else {
        auth::burn_signature_check(&signature);
        let cause = if enrolled.is_some() { "party disabled" } else { "unknown party" };
        node.log_login_failure(source, Channel::Rest, enrolled.as_deref(), REASON_AUTH_FAILED, cause);
        return auth_failed();
    };

    // A client that saw another certificate signed another transcript.
    let cert_sha256 = node.tls().cert_sha256();
    let verified = auth::rest_transcript(node.node_id(), &nonce, &cert_sha256, &party.id, PROTOCOL_VERSION, class)
        .map(|message| party.verify(&message, &signature))
        .unwrap_or(false);
    if !verified {
        node.log_login_failure(source, Channel::Rest, Some(&party.id), REASON_AUTH_FAILED, "bad signature");
        return auth_failed();
    }

    match node.tokens.issue(&party, class, now) {
        Ok(issued) => {
            info!(party = %party.id, token_class = class.as_str(), "token issued");
            (StatusCode::OK, Json(LoginResponse { token: issued.token, expires_in: issued.expires_in.as_secs() }))
                .into_response()
        }
        Err(TokenRefusal::Full) => {
            warn!(party = %party.id, "login refused: the token table is full");
            limit_refusal(StatusCode::SERVICE_UNAVAILABLE, "the token table is full")
        }
        Err(TokenRefusal::PartyLimit) => {
            node.log_rest_refusal(&party.id, None, "login: the party holds the most live tokens it may");
            limit_refusal(StatusCode::TOO_MANY_REQUESTS, "this party holds the most live tokens it may")
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Subscription lifetime
// ─────────────────────────────────────────────────────────────────────────────

/// Returns a topic subscription when the SSE response body is dropped.
/// Its watcher stops, and the party's quota slot, which the watcher holds,
/// goes back.
///
/// A topic holds at most `StreamConfig::max_subscribers` callbacks (64 by
/// default) and `Topic::subscribe` refuses further ones once that is reached.
/// An SSE handler that registers a callback and forgets its `SubscriberId`
/// therefore burns a slot per connection permanently: after 64 connects and
/// disconnects the topic is full, every later subscriber is rejected, and that
/// includes the engine's own. Tying the id to a guard whose `Drop` unsubscribes
/// makes the slot follow the connection instead.
struct SubscriptionGuard {
    stream: EustressStream,
    topic: String,
    id: SubscriberId,
    /// Ends the stream when the party is revoked or enrolled under another
    /// key, or a reload removes its grant. It holds the quota slot.
    watcher: AbortHandle,
}

/// An SSE stream's messages, until its token expires or its watcher stops it
/// (revocation, a new key, a removed grant). Nothing outlives the token: a
/// message still waiting when the stream ends is not sent.
struct UntilEnd {
    messages: ReceiverStream<OwnedMessage>,
    /// Fires at the token's `expires_at`.
    expiry: Pin<Box<tokio::time::Sleep>>,
    /// Told by the watcher; dropped with it.
    stop: oneshot::Receiver<()>,
    done: bool,
}

impl tokio_stream::Stream for UntilEnd {
    type Item = OwnedMessage;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<OwnedMessage>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        if this.expiry.as_mut().poll(cx).is_ready() || Pin::new(&mut this.stop).poll(cx).is_ready() {
            this.done = true;
            return Poll::Ready(None);
        }
        tokio_stream::Stream::poll_next(Pin::new(&mut this.messages), cx)
    }
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        self.watcher.abort();
        self.stream.unsubscribe(&self.topic, self.id);
        debug!(
            "SSE subscriber {} released on topic '{}' ({} remaining)",
            self.id.0,
            self.topic,
            self.stream.subscriber_count(&self.topic),
        );
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct PublishResponse {
    offset: u64,
}

async fn publish_handler(
    State(state): State<RestState>,
    Extension(AuthedParty(party, _)): Extension<AuthedParty>,
    Path(name): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    if !party.may_publish(&name) {
        state.auth.log_rest_refusal(&party.id, Some(&name), "publish: no grant for this topic");
        return refusal(StatusCode::FORBIDDEN, "publish refused: no grant for this topic").into_response();
    }
    let offset = state.stream.producer(&name).send_bytes(body.into());
    (StatusCode::OK, Json(PublishResponse { offset })).into_response()
}

async fn stream_handler(
    State(state): State<RestState>,
    Extension(AuthedParty(party, expires_at)): Extension<AuthedParty>,
    Path(name): Path<String>,
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>,
    (StatusCode, Json<ErrorResponse>),
> {
    if !party.may_subscribe(&name) {
        state.auth.log_rest_refusal(&party.id, Some(&name), "subscribe: no grant for this topic");
        return Err(refusal(StatusCode::FORBIDDEN, "subscribe refused: no grant for this topic"));
    }
    let Some(slot) = state.auth.quotas.try_acquire(&party) else {
        state.auth.log_rest_refusal(&party.id, Some(&name), "subscribe: the party's subscription quota is used up");
        return Err(refusal(StatusCode::TOO_MANY_REQUESTS, "subscribe refused: subscription quota reached"));
    };
    // The stream is a session: registered under the key the party logged in
    // with, and the party checked again right after.
    let Some(mut session) = state.auth.register_session(&party) else {
        return Err(refusal(StatusCode::UNAUTHORIZED, "unauthorized"));
    };

    let (tx, rx) = mpsc::channel::<OwnedMessage>(4096);

    // subscribe_owned: forward each message to the tokio channel.
    //
    // A refusal here means the topic is at its subscriber limit. Report it as
    // 503 rather than returning 200 over a stream that can never carry a
    // message: a silent dead feed is indistinguishable from an idle topic, and
    // the caller has no way to tell it should retry.
    let id = state
        .stream
        .subscribe_owned(&name, move |msg: OwnedMessage| {
            let _ = tx.try_send(msg); // drop if channel full (back-pressure)
        })
        .map_err(|e| {
            warn!("SSE subscribe to '{name}' refused: {e}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse { error: e.to_string() }),
            )
        })?;

    // The watcher ends the stream when the party is revoked or enrolled under
    // another key, and at the first reload that removes its grant for the
    // topic: it stops the event stream at once and removes the subscription.
    // It holds the quota slot, which goes back when it ends.
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let watcher = {
        let stream = state.stream.clone();
        let topic = name.clone();
        let auth = Arc::clone(&state.auth);
        let party_id = party.id.clone();
        let fingerprint = party.key_fingerprint();
        let mut parties_changed = state.auth.parties_changed();
        tokio::spawn(async move {
            let _slot = slot;
            let mut watching_parties = true;
            let why = loop {
                tokio::select! {
                    _ = session.revoked() => break "the party was revoked or enrolled under another key",
                    changed = parties_changed.changed(), if watching_parties => {
                        if changed.is_err() {
                            watching_parties = false;
                            continue;
                        }
                        let set = auth.party_set();
                        let granted = set.holds_key(&party_id, &fingerprint)
                            && set.active(&party_id).is_some_and(|p| p.may_subscribe(&topic));
                        if !granted {
                            break "the party's grant for this topic was removed";
                        }
                    }
                }
            };
            debug!(party = %party_id, topic = ?truncate_for_log(&topic), "SSE stream ended: {why}");
            let _ = stop_tx.send(());
            stream.unsubscribe(&topic, id);
        })
        .abort_handle()
    };

    let guard = SubscriptionGuard {
        stream: state.stream.clone(),
        topic: name.clone(),
        id,
        watcher,
    };

    debug!(
        "SSE subscriber {} attached to topic '{name}' ({} total)",
        id.0,
        state.stream.subscriber_count(&name),
    );

    // The token's expiry is inside the stream itself: at `expires_at` it ends,
    // whatever is still waiting in it.
    let messages = UntilEnd {
        messages: ReceiverStream::new(rx),
        expiry: Box::pin(tokio::time::sleep_until(expires_at)),
        stop: stop_rx,
        done: false,
    };
    let stream = messages.map(move |msg: OwnedMessage| {
        // `guard` is moved into this closure and read nowhere else. That is the
        // point: axum drops the response stream when the client disconnects,
        // which drops this closure, which drops the guard and returns the
        // subscriber slot. Removing this line un-captures it and reinstates the
        // leak.
        let _guard = &guard;

        let data = serde_json::json!({
            "topic": msg.topic,
            "offset": msg.offset,
            "timestamp": msg.timestamp,
            "payload_b64": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD, &msg.data
            ),
        });
        Ok::<_, Infallible>(
            Event::default()
                .json_data(data)
                .unwrap_or_else(|_| Event::default().data("error"))
        )
    });

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"),
    ))
}

/// `GET /topics/{name}/replay/{from}`: the ring buffer from offset `from`,
/// as SSE. The offset is in the path because a bearer route takes no query
/// parameters; there is no replay route that reads it from the query string.
async fn replay_handler(
    State(state): State<RestState>,
    Extension(AuthedParty(party, _)): Extension<AuthedParty>,
    Path((name, from_offset)): Path<(String, u64)>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, (StatusCode, Json<ErrorResponse>)> {
    if !party.may_subscribe(&name) {
        state.auth.log_rest_refusal(&party.id, Some(&name), "replay: no grant for this topic");
        return Err(refusal(StatusCode::FORBIDDEN, "replay refused: no grant for this topic"));
    }
    let mut events: Vec<serde_json::Value> = Vec::new();

    state.stream.replay_ring(&name, from_offset, |view| {
        events.push(serde_json::json!({
            "topic": view.topic,
            "offset": view.offset,
            "timestamp": view.timestamp,
            "payload_b64": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD, view.data
            ),
        }));
    });

    let stream = tokio_stream::iter(events).map(|data| {
        Ok::<_, Infallible>(
            Event::default()
                .json_data(data)
                .unwrap_or_else(|_| Event::default().data("error"))
        )
    });

    Ok(Sse::new(stream))
}

#[derive(Serialize)]
struct TopicListResponse {
    topics: Vec<TopicStats>,
}

async fn list_topics_handler(
    State(state): State<RestState>,
    Extension(AuthedParty(party, _)): Extension<AuthedParty>,
) -> impl IntoResponse {
    let topics: Vec<TopicStats> = state.stream.topics()
        .into_iter()
        // Only the topics this party holds a grant on.
        .filter(|name| party.may_list(name))
        .map(|name| TopicStats {
            head: state.stream.head(&name),
            subscriber_count: state.stream.subscriber_count(&name),
            name,
        })
        .collect();
    Json(TopicListResponse { topics })
}

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
    node_id: String,
    tcp_port: u16,
    rest_port: u16,
    uptime_secs: u64,
    topic_count: usize,
}

async fn health_handler(State(state): State<RestState>) -> impl IntoResponse {
    Json(HealthResponse {
        ok: true,
        node_id: state.node_id.clone(),
        tcp_port: state.tcp_port,
        rest_port: state.rest_port,
        uptime_secs: state.start_time.elapsed().as_secs(),
        topic_count: state.stream.topics().len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_routes_refuse_every_query_parameter_after_decoding() {
        assert!(refuses_query(Some("token=abc")));
        assert!(refuses_query(Some("%74oken=abc")), "an encoded name is decoded first");
        assert!(refuses_query(Some("foo=")));
        assert!(refuses_query(Some("from=3")), "the allowlist is empty");
        assert!(refuses_query(Some("%zz=1")), "a malformed escape is refused");
        assert!(!refuses_query(None));
        assert!(!refuses_query(Some("")));
        assert_eq!(percent_decode("%74oken").as_deref(), Some("token"));
        assert_eq!(percent_decode("a+b%20c").as_deref(), Some("a b c"));
        assert_eq!(percent_decode("%4"), None);
    }

    #[test]
    fn bearer_header_is_parsed() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer abc=".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("abc="));
        headers.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert_eq!(bearer_token(&headers), None);
    }
}

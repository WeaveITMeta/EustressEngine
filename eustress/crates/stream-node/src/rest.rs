//! REST + SSE HTTP API for EustressStream nodes.
//!
//! Endpoints:
//!   POST /topics/{name}/publish          — publish raw bytes, returns {"offset":N}
//!   GET  /topics/{name}/stream           — SSE live message feed (JSON-encoded)
//!   GET  /topics/{name}/replay?from=N    — replay ring buffer from offset N via SSE
//!   GET  /topics                         — list topics with stats
//!   GET  /health                         — node health and uptime

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tracing::{debug, info, warn};

use eustress_stream::{EustressStream, OwnedMessage, SubscriberId};

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
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

pub fn build_router(state: RestState) -> Router {
    Router::new()
        .route("/topics/{name}/publish", post(publish_handler))
        .route("/topics/{name}/stream", get(stream_handler))
        .route("/topics/{name}/replay", get(replay_handler))
        .route("/topics", get(list_topics_handler))
        .route("/health", get(health_handler))
        .with_state(state)
}

/// CORS policy for the REST API.
///
/// `POST /topics/{name}/publish` writes into a topic ring with no
/// authentication, so a wildcard `Access-Control-Allow-Origin` would let any
/// page the operator happens to visit publish into this node from their
/// browser. The policy is therefore restricted to the loopback origins a local
/// dashboard is actually served from, and to the methods and headers those
/// endpoints use.
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
        .allow_headers([header::CONTENT_TYPE, header::ACCEPT])
}

/// Start the REST server on the configured REST port. Runs until the process exits.
///
/// Binds [`NodeConfig::bind_addr`] (loopback by default). The API is
/// unauthenticated: every caller may publish into any topic and subscribe to
/// any other, so a non-loopback bind must be a deliberate operator choice.
pub async fn serve_rest(state: RestState, config: &NodeConfig) -> Result<(), NodeError> {
    let port = config.effective_rest_port();
    let addr = SocketAddr::new(config.bind_addr, port);
    info!("EustressStream REST API on http://{addr}");
    if config.is_externally_bound() {
        warn!(
            "EustressStream REST API bound to {}. This API is UNAUTHENTICATED: anything that can route to this host may publish into and read every topic. Put a trusted network boundary in front of it.",
            config.bind_addr
        );
    }

    let router = build_router(state)
        .layer(cors_layer())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router)
        .await
        .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))
}

// ─────────────────────────────────────────────────────────────────────────────
// Subscription lifetime
// ─────────────────────────────────────────────────────────────────────────────

/// Returns a topic subscription when the SSE response body is dropped.
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
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
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
    Path(name): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let offset = state.stream.producer(&name).send_bytes(body.into());
    (StatusCode::OK, Json(PublishResponse { offset }))
}

#[derive(Deserialize)]
struct ReplayParams {
    from: Option<u64>,
}

async fn stream_handler(
    State(state): State<RestState>,
    Path(name): Path<String>,
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>,
    (StatusCode, Json<ErrorResponse>),
> {
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

    let guard = SubscriptionGuard {
        stream: state.stream.clone(),
        topic: name.clone(),
        id,
    };

    debug!(
        "SSE subscriber {} attached to topic '{name}' ({} total)",
        id.0,
        state.stream.subscriber_count(&name),
    );

    let stream = ReceiverStream::new(rx).map(move |msg: OwnedMessage| {
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

async fn replay_handler(
    State(state): State<RestState>,
    Path(name): Path<String>,
    Query(params): Query<ReplayParams>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let from_offset = params.from.unwrap_or(0);
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

    Sse::new(stream)
}

#[derive(Serialize)]
struct TopicListResponse {
    topics: Vec<TopicStats>,
}

async fn list_topics_handler(State(state): State<RestState>) -> impl IntoResponse {
    let topics: Vec<TopicStats> = state.stream.topics()
        .into_iter()
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

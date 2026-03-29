//! REST + SSE HTTP API for EustressStream nodes.
//!
//! Endpoints:
//!   POST /topics/{name}/publish          — publish raw bytes, returns {"offset":N}
//!   GET  /topics/{name}/stream           — SSE live message feed (JSON-encoded)
//!   GET  /topics/{name}/replay?from=N    — replay ring buffer from offset N via SSE
//!   GET  /topics                         — list topics with stats
//!   GET  /health                         — node health and uptime

use std::convert::Infallible;
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
use tracing::info;

use eustress_stream::{EustressStream, OwnedMessage};

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

/// Start the REST server on the configured REST port. Runs until the process exits.
pub async fn serve_rest(state: RestState, config: &NodeConfig) -> Result<(), NodeError> {
    let port = config.effective_rest_port();
    let addr = format!("0.0.0.0:{port}");
    info!("EustressStream REST API on http://{addr}");

    let router = build_router(state)
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router)
        .await
        .map_err(|e| NodeError::Io(std::io::Error::other(e.to_string())))
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
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<OwnedMessage>(4096);

    // subscribe_owned: forward each message to the tokio channel.
    let _ = state.stream.subscribe_owned(&name, move |msg: OwnedMessage| {
        let _ = tx.try_send(msg); // drop if channel full (back-pressure)
    });

    let stream = ReceiverStream::new(rx).map(|msg: OwnedMessage| {
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

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"),
    )
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

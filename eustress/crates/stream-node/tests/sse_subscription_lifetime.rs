//! The SSE endpoint must return its topic subscriber slot when a client goes
//! away.
//!
//! A topic accepts at most `StreamConfig::max_subscribers` callbacks and
//! refuses every registration past that. `GET /topics/{name}/stream` used to
//! register one and discard the `SubscriberId`, so a slot was consumed for the
//! lifetime of the process rather than the lifetime of the connection. After
//! `max_subscribers` connect/disconnect cycles the topic was full: later
//! clients received `200 OK` and keep-alives over a stream that could never
//! carry a message, and the engine's own subscribers were locked out too.
//!
//! These tests run the real REST server (TLS, bearer token) over in-memory
//! pipes and hang up by dropping the client, so they exercise the disconnect
//! path axum actually takes. Waiting for the server to notice is a bounded
//! count of scheduler turns, never a sleep.

mod common;

use std::net::SocketAddr;

use eustress_stream::{EustressStream, StreamConfig};
use eustress_stream_node::auth::TokenClass;
use eustress_stream_node::client::EventStream;
use eustress_stream_node::{NodeAuth, RestClient, RestServer, RestState, SpkiPin};

use common::{Fixture, TestParty};

/// Small cap so the tests cross the limit quickly.
const MAX_SUBSCRIBERS: usize = 4;
const TOPIC: &str = "scene_deltas";

struct Harness {
    stream: EustressStream,
    rest: RestServer,
    token: String,
    pin: SpkiPin,
    _fixture: Fixture,
}

fn harness() -> Harness {
    let fixture = Fixture::new();
    // The party's own quota is well above the topic's cap, so the topic's
    // subscriber limit is what these tests reach.
    let reader = TestParty::new("sse-reader").subscribe(&[TOPIC]).max_subscriptions(64);
    fixture.write_parties(&[&reader]);
    let auth = NodeAuth::from_config(&fixture.config()).expect("startup checks");
    let stream = EustressStream::new(StreamConfig {
        max_subscribers: MAX_SUBSCRIBERS,
        ..StreamConfig::default().in_memory()
    });
    let token = auth
        .tokens
        .issue(&common::enrolled(&auth, "sse-reader"), TokenClass::Session, tokio::time::Instant::now())
        .expect("token")
        .token;
    let rest = RestServer::unbound(RestState::new(stream.clone(), auth, 0, 0));
    Harness { stream, rest, token, pin: fixture.cert.pin, _fixture: fixture }
}

/// Open an SSE connection and read the response status. The returned client
/// and stream keep the subscription alive until they are dropped.
async fn open_sse(h: &Harness, topic: &str) -> (RestClient, EventStream) {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    assert!(h.rest.serve_io(server_io, SocketAddr::from(([127, 0, 0, 1], 50_000))), "admitted");
    let mut client = RestClient::handshake(client_io, h.pin).await.expect("TLS to the REST server");
    let events = client.open_stream(&format!("/topics/{topic}/stream"), &h.token).await.expect("response");
    (client, events)
}

/// Wait for `subscriber_count` to reach `want`, for a bounded number of
/// scheduler turns.
///
/// The server notices a hung-up client asynchronously, so this polls rather
/// than asserting immediately. It still fails on the leak: a leaked slot never
/// comes back no matter how long the wait.
async fn await_subscriber_count(stream: &EustressStream, topic: &str, want: usize) -> usize {
    for _ in 0..10_000 {
        let n = stream.subscriber_count(topic);
        if n == want {
            return n;
        }
        // Publishing makes the server write to the dead socket, which is how it
        // learns the peer is gone.
        stream.producer(topic).send_bytes(bytes::Bytes::from_static(b"poke"));
        tokio::task::yield_now().await;
    }
    stream.subscriber_count(topic)
}

/// Every disconnect returns its slot, so far more clients than
/// `max_subscribers` can be served over the life of the process.
#[tokio::test]
async fn sse_disconnect_releases_subscriber_slot() {
    let h = harness();

    // Three times the cap, one at a time. Pre-fix this saturates at
    // MAX_SUBSCRIBERS and never recovers.
    for i in 0..(MAX_SUBSCRIBERS * 3) {
        let (client, events) = open_sse(&h, TOPIC).await;
        assert_eq!(events.status, 200, "connection {i} was refused");
        drop(events);
        drop(client);

        let n = await_subscriber_count(&h.stream, TOPIC, 0).await;
        assert_eq!(
            n, 0,
            "connection {i} leaked its subscriber slot ({n} still registered)"
        );
    }
}

/// A client that arrives after `max_subscribers` earlier clients have come and
/// gone still receives published messages.
///
/// This is the user-visible symptom of the leak: the response was `200 OK` with
/// keep-alives and no data, indistinguishable from an idle topic.
#[tokio::test]
async fn sse_client_after_cap_churn_still_receives_messages() {
    let h = harness();

    // Burn through the cap and hang up each time.
    for _ in 0..MAX_SUBSCRIBERS {
        let (client, events) = open_sse(&h, TOPIC).await;
        drop(events);
        drop(client);
        await_subscriber_count(&h.stream, TOPIC, 0).await;
    }

    // The next client must be a working subscriber, not a silent one.
    let (_client, mut events) = open_sse(&h, TOPIC).await;
    assert_eq!(events.status, 200, "post-churn client refused");
    assert_eq!(await_subscriber_count(&h.stream, TOPIC, 1).await, 1);

    h.stream
        .producer(TOPIC)
        .send_bytes(bytes::Bytes::from_static(b"hello"));

    let frame = events
        .next_data()
        .await
        .expect("the stream is readable")
        .expect("connection closed before any data frame");
    assert!(
        frame.contains("payload_b64"),
        "expected a message payload, got {frame:?}"
    );
}

/// When the topic genuinely is full, the endpoint says so instead of handing
/// back a stream that can never deliver anything.
#[tokio::test]
async fn sse_reports_503_when_topic_is_full() {
    let h = harness();

    // Hold every slot open concurrently.
    let mut held = Vec::new();
    for i in 0..MAX_SUBSCRIBERS {
        let (client, events) = open_sse(&h, TOPIC).await;
        assert_eq!(events.status, 200, "connection {i} refused");
        held.push((client, events));
    }
    assert_eq!(h.stream.subscriber_count(TOPIC), MAX_SUBSCRIBERS);

    let (_client, events) = open_sse(&h, TOPIC).await;
    assert_eq!(events.status, 503, "expected 503 once the topic is full");

    // Freeing one slot lets the next client in.
    held.pop();
    await_subscriber_count(&h.stream, TOPIC, MAX_SUBSCRIBERS - 1).await;

    let (_client, events) = open_sse(&h, TOPIC).await;
    assert_eq!(events.status, 200, "expected the freed slot to be reusable");
}

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
//! These tests connect over real TCP and hang up, so they exercise the
//! disconnect path axum actually takes.

use std::time::Duration;

use eustress_stream::{EustressStream, StreamConfig};
use eustress_stream_node::{build_router, RestState};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Small cap so the tests cross the limit quickly.
const MAX_SUBSCRIBERS: usize = 4;
const TOPIC: &str = "scene_deltas";

fn test_stream() -> EustressStream {
    EustressStream::new(StreamConfig {
        max_subscribers: MAX_SUBSCRIBERS,
        ..StreamConfig::default().in_memory()
    })
}

/// Serve the REST router on an ephemeral loopback port. Returns the bound port.
async fn spawn_server(stream: EustressStream) -> u16 {
    let state = RestState {
        stream,
        start_time: std::sync::Arc::new(std::time::Instant::now()),
        node_id: "test-node".to_string(),
        tcp_port: 0,
        rest_port: 0,
    };

    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    let router = build_router(state);

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    port
}

/// Open an SSE connection and read the response status line. The returned
/// socket keeps the subscription alive until it is dropped.
async fn open_sse(port: u16, topic: &str) -> (TcpStream, String) {
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.expect("connect");
    let req = format!(
        "GET /topics/{topic}/stream HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: text/event-stream\r\n\r\n"
    );
    sock.write_all(req.as_bytes()).await.expect("write request");

    // Read just the status line; the body stays open on the socket.
    let mut byte = [0u8; 1];
    let mut line = Vec::new();
    while sock.read_exact(&mut byte).await.is_ok() {
        if byte[0] == b'\n' {
            break;
        }
        if byte[0] != b'\r' {
            line.push(byte[0]);
        }
    }
    (sock, String::from_utf8_lossy(&line).into_owned())
}

/// Wait for `subscriber_count` to reach `want`, up to a few seconds.
///
/// The server notices a hung-up client asynchronously, so this polls rather
/// than asserting immediately. It still fails on the leak: a leaked slot never
/// comes back no matter how long the wait.
async fn await_subscriber_count(stream: &EustressStream, topic: &str, want: usize) -> usize {
    for _ in 0..100 {
        let n = stream.subscriber_count(topic);
        if n == want {
            return n;
        }
        // Publishing makes the server write to the dead socket, which is how it
        // learns the peer is gone.
        stream.producer(topic).send_bytes(bytes::Bytes::from_static(b"poke"));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    stream.subscriber_count(topic)
}

/// Every disconnect returns its slot, so far more clients than
/// `max_subscribers` can be served over the life of the process.
#[tokio::test]
async fn sse_disconnect_releases_subscriber_slot() {
    let stream = test_stream();
    let port = spawn_server(stream.clone()).await;

    // Three times the cap, one at a time. Pre-fix this saturates at
    // MAX_SUBSCRIBERS and never recovers.
    for i in 0..(MAX_SUBSCRIBERS * 3) {
        let (sock, status) = open_sse(port, TOPIC).await;
        assert!(
            status.contains("200"),
            "connection {i} was refused: {status:?}"
        );
        drop(sock);

        let n = await_subscriber_count(&stream, TOPIC, 0).await;
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
    let stream = test_stream();
    let port = spawn_server(stream.clone()).await;

    // Burn through the cap and hang up each time.
    for _ in 0..MAX_SUBSCRIBERS {
        let (sock, _) = open_sse(port, TOPIC).await;
        drop(sock);
        await_subscriber_count(&stream, TOPIC, 0).await;
    }

    // The next client must be a working subscriber, not a silent one.
    let (sock, status) = open_sse(port, TOPIC).await;
    assert!(status.contains("200"), "post-churn client refused: {status:?}");
    await_subscriber_count(&stream, TOPIC, 1).await;

    let mut reader = BufReader::new(sock);
    stream
        .producer(TOPIC)
        .send_bytes(bytes::Bytes::from_static(b"hello"));

    // Read until a data frame arrives or the read times out.
    let read_data = tokio::time::timeout(Duration::from_secs(5), async {
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                return None;
            }
            if line.starts_with("data:") {
                return Some(line.clone());
            }
        }
    })
    .await;

    let frame = read_data
        .expect("timed out waiting for an SSE data frame")
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
    let stream = test_stream();
    let port = spawn_server(stream.clone()).await;

    // Hold every slot open concurrently.
    let mut held = Vec::new();
    for i in 0..MAX_SUBSCRIBERS {
        let (sock, status) = open_sse(port, TOPIC).await;
        assert!(status.contains("200"), "connection {i} refused: {status:?}");
        held.push(sock);
    }
    assert_eq!(stream.subscriber_count(TOPIC), MAX_SUBSCRIBERS);

    let (_sock, status) = open_sse(port, TOPIC).await;
    assert!(
        status.contains("503"),
        "expected 503 once the topic is full, got {status:?}"
    );

    // Freeing one slot lets the next client in.
    held.pop();
    await_subscriber_count(&stream, TOPIC, MAX_SUBSCRIBERS - 1).await;

    let (_sock, status) = open_sse(port, TOPIC).await;
    assert!(
        status.contains("200"),
        "expected the freed slot to be reusable, got {status:?}"
    );
}

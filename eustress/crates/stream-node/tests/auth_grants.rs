//! Authorization: default-deny grants per topic, the admin rule for reserved
//! topics, the per-party subscription quota, and revocation of a live
//! connection and a live SSE stream. Also the reply discipline: one reply
//! per reply-expecting request, in order, matched by the client's one queue.
//!
//! TCP and QUIC share one dispatcher (`handler::ConnectionHandler`), so these
//! run over TCP through the node's real connection task.

mod common;

use std::sync::Arc;
use std::time::Duration;

use eustress_stream_node::auth::{
    TokenClass, ERR_FORBIDDEN, ERR_QUOTA, MAX_NO_ACK_REFUSALS, NO_ACK_REFUSAL_WINDOW, RELOAD_INTERVAL,
};
use eustress_stream_node::protocol::{read_client_frame, write_server_frame, ClientFrame, ServerFrame};
use eustress_stream_node::{NodeError, StreamNodeClient};
use tokio::io::DuplexStream;
use tokio::time::Instant;

use common::{peer, DuplexNode, DuplexRest, Fixture, RawSession, TestParty};

async fn logged_in(node: &DuplexNode, fixture: &Fixture, party: &TestParty, from: u8) -> RawSession<DuplexStream> {
    let mut session = RawSession::open(node.connect(peer(from)), fixture.cert.pin).await.expect("TLS");
    match session.login(party).await.expect("reply") {
        ServerFrame::Welcome { .. } => session,
        other => panic!("{} must log in, got {other:?}", party.id),
    }
}

/// The next frame from the node. The timeout is on the paused clock: it only
/// turns a reply that never comes (a refusal a mutant swallowed) into a clean
/// failure instead of a hang.
async fn reply(session: &mut RawSession<DuplexStream>) -> ServerFrame {
    tokio::time::timeout(Duration::from_secs(5), session.recv())
        .await
        .expect("a reply within 5 s on the paused clock")
        .expect("a frame")
}

fn publish(topic: &str, payload: &[u8]) -> ClientFrame {
    ClientFrame::Publish { topic: topic.to_string(), payload: payload.to_vec() }
}

/// Publishing without a `publish` grant returns `Error`, and the topic's
/// offset does not advance.
#[tokio::test(start_paused = true)]
async fn publish_without_grant_returns_error_and_offset_does_not_advance() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["log/output"]).subscribe(&["telemetry"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut session = logged_in(&node, &fixture, &plant, 1).await;

    // Control: the granted topic is acked.
    session.send(&publish("log/output", b"ok")).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Ack { offset: 0 }));

    let before = node.stream.head("telemetry");
    session.send(&publish("telemetry", b"not mine")).await.unwrap();
    match reply(&mut session).await {
        ServerFrame::Error { code, .. } => assert_eq!(code, ERR_FORBIDDEN),
        other => panic!("expected Error, got {other:?}"),
    }
    assert_eq!(node.stream.head("telemetry"), before, "a refused publish must not advance the topic");

    // The batch form is refused the same way. A refused fire-and-forget
    // publish sends nothing back (tested below).
    session
        .send(&ClientFrame::PublishBatch {
            messages: vec![("log/output".into(), b"a".to_vec()), ("telemetry".into(), b"b".to_vec())],
        })
        .await
        .unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Error { code, .. } if code == ERR_FORBIDDEN));
    assert_eq!(node.stream.head("telemetry"), before);
    assert_eq!(node.stream.head("log/output"), 1, "a refused batch writes nothing, not even its granted part");
}

/// A `PublishBatch` with one refused topic publishes nothing: not even the
/// messages for the topics the party does hold a grant on.
#[tokio::test(start_paused = true)]
async fn publish_batch_with_one_refused_topic_publishes_nothing() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["log/output", "telemetry"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut session = logged_in(&node, &fixture, &plant, 1).await;

    let batch = |topics: &[&str]| ClientFrame::PublishBatch {
        messages: topics.iter().map(|t| (t.to_string(), b"m".to_vec())).collect(),
    };
    session.send(&batch(&["log/output", "telemetry", "scene_deltas", "log/output"])).await.unwrap();
    match reply(&mut session).await {
        ServerFrame::Error { code, .. } => assert_eq!(code, ERR_FORBIDDEN),
        other => panic!("expected Error, got {other:?}"),
    }
    for topic in ["log/output", "telemetry", "scene_deltas"] {
        assert_eq!(node.stream.head(topic), 0, "{topic}: a refused batch writes nothing");
    }

    // Control: the same batch without the refused topic is written in full.
    session.send(&batch(&["log/output", "telemetry", "log/output"])).await.unwrap();
    match reply(&mut session).await {
        ServerFrame::BatchAck { offsets } => assert_eq!(offsets, vec![0, 0, 1]),
        other => panic!("expected BatchAck, got {other:?}"),
    }
    assert_eq!(node.stream.head("log/output"), 2);
    assert_eq!(node.stream.head("telemetry"), 1);
}

/// A fire-and-forget publish to `telemetry`, which the test party holds no
/// grant for. Even numbers send `PublishNoAck`, odd ones `PublishBatchNoAck`:
/// both count toward the window.
fn refused_no_ack(i: u32) -> ClientFrame {
    if i % 2 == 0 {
        ClientFrame::PublishNoAck { topic: "telemetry".into(), payload: b"refused".to_vec() }
    } else {
        ClientFrame::PublishBatchNoAck { topic: "telemetry".into(), payloads: vec![b"refused".to_vec()] }
    }
}

/// The node's one warn line when it closes a connection for refused
/// fire-and-forget publishes.
const CLOSE_LINE: &str = "more than 100 refused fire-and-forget publishes";

/// This thread's log output. A paused-clock test runs on a current-thread
/// runtime, so the node's tasks log on this thread too.
#[derive(Clone, Default)]
struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    /// Send this thread's warn lines here until the guard drops.
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let writer = self.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .finish();
        tracing::subscriber::set_default(subscriber)
    }

    fn lines_containing(&self, needle: &str) -> Vec<String> {
        let text = String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned();
        text.lines().filter(|line| line.contains(needle)).map(str::to_string).collect()
    }
}

/// A refused `PublishNoAck` sends no frame back, and the next acknowledged
/// publish's `Ack` reaches its own caller: the next frame on the connection
/// is that `Ack`, not an `Error` the sender would take for it.
#[tokio::test(start_paused = true)]
async fn refused_no_ack_publish_sends_no_frame_and_the_next_ack_reaches_its_own_caller() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut session = logged_in(&node, &fixture, &plant, 1).await;

    session.send(&refused_no_ack(0)).await.unwrap();
    session.send(&refused_no_ack(1)).await.unwrap();
    session.send(&publish("log/output", b"acknowledged")).await.unwrap();
    match reply(&mut session).await {
        ServerFrame::Ack { offset } => assert_eq!(offset, 0, "the Ack is the acknowledged publish's own"),
        other => panic!("a refused fire-and-forget publish must send nothing; the next frame was {other:?}"),
    }
    assert_eq!(node.stream.head("telemetry"), 0, "the refused publishes wrote nothing");

    // Control: a granted fire-and-forget publish is written and sends nothing either.
    session.send(&ClientFrame::PublishNoAck { topic: "log/output".into(), payload: b"granted".to_vec() }).await.unwrap();
    session.send(&ClientFrame::Ping).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Pong));
    assert_eq!(node.stream.head("log/output"), 2);
}

/// 100 refused fire-and-forget publishes in one 60-second window keep the
/// connection open; the 101st closes it, with one warn line that names the
/// party. Acknowledged refusals answer `Error` and do not count.
#[tokio::test(start_paused = true)]
async fn the_101st_refused_no_ack_publish_in_one_window_closes_the_connection_with_one_warn_line() {
    let logs = Captured::default();
    let _logging = logs.install();
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut session = logged_in(&node, &fixture, &plant, 1).await;

    for _ in 0..5 {
        session.send(&publish("telemetry", b"acknowledged and refused")).await.unwrap();
        assert!(matches!(reply(&mut session).await, ServerFrame::Error { code, .. } if code == ERR_FORBIDDEN));
    }
    for i in 0..MAX_NO_ACK_REFUSALS {
        session.send(&refused_no_ack(i)).await.unwrap();
    }
    session.send(&ClientFrame::Ping).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Pong), "100 refusals in one window keep the connection open");
    assert!(logs.lines_containing(CLOSE_LINE).is_empty(), "no close line yet");

    session.send(&refused_no_ack(MAX_NO_ACK_REFUSALS)).await.unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(5), session.recv())
        .await
        .expect("the 101st refusal in one window must close the connection");
    assert!(matches!(closed, Err(NodeError::ConnectionClosed)), "expected the node to close, got {closed:?}");
    let lines = logs.lines_containing(CLOSE_LINE);
    assert_eq!(lines.len(), 1, "exactly one warn line for the close: {lines:?}");
    assert!(lines[0].contains("north-plant"), "the line names the party: {}", lines[0]);
    assert!(lines[0].contains("WARN"), "the line is a warn line: {}", lines[0]);
    assert_eq!(node.stream.head("telemetry"), 0);
}

/// The window is fixed and resets after 60 seconds: a 101st refusal that
/// arrives after the reset begins a new window and does not close the
/// connection.
#[tokio::test(start_paused = true)]
async fn a_101st_refusal_after_the_window_resets_does_not_close_the_connection() {
    let logs = Captured::default();
    let _logging = logs.install();
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut session = logged_in(&node, &fixture, &plant, 1).await;

    for i in 0..MAX_NO_ACK_REFUSALS {
        session.send(&refused_no_ack(i)).await.unwrap();
    }
    session.send(&ClientFrame::Ping).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Pong), "the first window holds 100 refusals");

    tokio::time::advance(NO_ACK_REFUSAL_WINDOW).await;
    session.send(&refused_no_ack(MAX_NO_ACK_REFUSALS)).await.unwrap();
    session.send(&ClientFrame::Ping).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Pong), "after the reset, the 101st refusal begins a new window");
    assert!(logs.lines_containing(CLOSE_LINE).is_empty(), "no close line: {:?}", logs.lines_containing(CLOSE_LINE));
}

/// Publishing to `scene_deltas` by a party without `admin = true` returns
/// `Error`, even when `scene_deltas` is in its `publish` list.
#[tokio::test(start_paused = true)]
async fn publish_to_scene_deltas_without_admin_returns_error_even_with_the_grant() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["scene_deltas", "agent_commands"]);
    let editor = TestParty::new("editor").publish(&["scene_deltas"]).admin();
    fixture.write_parties(&[&plant, &editor]);
    let node = DuplexNode::start(&fixture);

    let mut session = logged_in(&node, &fixture, &plant, 1).await;
    for topic in ["scene_deltas", "agent_commands"] {
        session.send(&publish(topic, b"delta")).await.unwrap();
        match reply(&mut session).await {
            ServerFrame::Error { code, .. } => assert_eq!(code, ERR_FORBIDDEN, "{topic}"),
            other => panic!("{topic}: expected Error, got {other:?}"),
        }
        assert_eq!(node.stream.head(topic), 0);
    }

    // Control: an admin with the grant may publish a scene delta.
    let mut admin = logged_in(&node, &fixture, &editor, 2).await;
    admin.send(&publish("scene_deltas", b"delta")).await.unwrap();
    assert!(matches!(reply(&mut admin).await, ServerFrame::Ack { offset: 0 }));
}

/// Subscribing to a topic missing from `subscribe` is refused.
#[tokio::test(start_paused = true)]
async fn subscribe_without_grant_is_refused() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["log/output"]).publish(&["scene_deltas"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut session = logged_in(&node, &fixture, &plant, 1).await;

    session.send(&ClientFrame::Subscribe { topic: "scene_deltas".into(), from_offset: None }).await.unwrap();
    match reply(&mut session).await {
        ServerFrame::Error { code, .. } => assert_eq!(code, ERR_FORBIDDEN),
        other => panic!("expected Error, got {other:?}"),
    }
    assert_eq!(node.stream.subscriber_count("scene_deltas"), 0);

    // Control: the granted topic subscribes and delivers.
    session.send(&ClientFrame::Subscribe { topic: "log/output".into(), from_offset: None }).await.unwrap();
    assert!(
        matches!(reply(&mut session).await, ServerFrame::Subscribed { head: 0 }),
        "a granted subscribe is answered Subscribed"
    );
    assert_eq!(node.stream.subscriber_count("log/output"), 1);
    node.stream.producer("log/output").send_bytes(bytes::Bytes::from_static(b"hello"));
    match reply(&mut session).await {
        ServerFrame::Message { topic, payload, .. } => {
            assert_eq!(topic, "log/output");
            assert_eq!(payload, b"hello");
        }
        other => panic!("expected the message, got {other:?}"),
    }
}

/// Subscribing beyond `max_subscriptions` returns `Error`, and the topic's
/// subscriber count is unchanged. The quota counts across connections.
#[tokio::test(start_paused = true)]
async fn subscribe_beyond_max_subscriptions_returns_error_and_subscriber_count_is_unchanged() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["a", "b", "c"]).max_subscriptions(2);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut first = logged_in(&node, &fixture, &plant, 1).await;
    let mut second = logged_in(&node, &fixture, &plant, 2).await;

    for topic in ["a", "b"] {
        first.send(&ClientFrame::Subscribe { topic: topic.into(), from_offset: None }).await.unwrap();
        assert!(matches!(reply(&mut first).await, ServerFrame::Subscribed { .. }), "two subscriptions fit the quota");
    }

    second.send(&ClientFrame::Subscribe { topic: "c".into(), from_offset: None }).await.unwrap();
    match reply(&mut second).await {
        ServerFrame::Error { code, .. } => assert_eq!(code, ERR_QUOTA),
        other => panic!("expected Error, got {other:?}"),
    }
    assert_eq!(node.stream.subscriber_count("c"), 0, "a refused subscribe takes no subscriber slot");
    assert_eq!(node.auth.quotas.in_use(&plant.id), 2);

    // Releasing one makes room again.
    first.send(&ClientFrame::Unsubscribe { topic: "a".into() }).await.unwrap();
    first.send(&ClientFrame::Ping).await.unwrap();
    assert!(matches!(reply(&mut first).await, ServerFrame::Pong));
    second.send(&ClientFrame::Subscribe { topic: "c".into(), from_offset: None }).await.unwrap();
    assert!(matches!(reply(&mut second).await, ServerFrame::Subscribed { .. }));
    assert_eq!(node.stream.subscriber_count("c"), 1);
}

/// `ListTopics` shows only the topics the party holds a grant on.
#[tokio::test(start_paused = true)]
async fn list_topics_shows_only_granted_topics() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    node.stream.producer("log/output").send_bytes(bytes::Bytes::from_static(b"x"));
    node.stream.producer("agent_commands").send_bytes(bytes::Bytes::from_static(b"y"));

    let mut session = logged_in(&node, &fixture, &plant, 1).await;
    session.send(&ClientFrame::ListTopics).await.unwrap();
    match reply(&mut session).await {
        ServerFrame::TopicList(topics) => {
            let names: Vec<_> = topics.iter().map(|t| t.name.as_str()).collect();
            assert_eq!(names, vec!["log/output"]);
        }
        other => panic!("expected TopicList, got {other:?}"),
    }
}

/// Disabling a party drops its live connection and its SSE stream within the
/// one-second check, measured on the paused clock. Another party's
/// connection stays up.
#[tokio::test(start_paused = true)]
async fn disabling_a_party_drops_its_live_connection_and_sse_stream_within_the_one_second_check() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["log/output", "telemetry"]);
    let other = TestParty::new("south-plant").subscribe(&["log/output"]);
    fixture.write_parties(&[&plant, &other]);
    let node = DuplexNode::start(&fixture);
    node.auth.spawn_revocation_task(None);

    let mut revoked = logged_in(&node, &fixture, &plant, 1).await;
    revoked.send(&ClientFrame::Subscribe { topic: "log/output".into(), from_offset: None }).await.unwrap();
    assert!(matches!(reply(&mut revoked).await, ServerFrame::Subscribed { .. }));
    let mut kept = logged_in(&node, &fixture, &other, 2).await;

    // The same party's SSE stream, on the node's REST side, which shares the
    // node's parties, sessions and tokens.
    let rest = DuplexRest::with_auth(Arc::clone(&node.auth), node.stream.clone(), fixture.cert.pin);
    let token = node
        .auth
        .tokens
        .issue(&common::enrolled(&node.auth, &plant.id), TokenClass::Browser, Instant::now())
        .expect("token")
        .token;
    let mut sse_client = rest.client(peer(3)).await;
    let mut events = sse_client.open_stream("/topics/telemetry/stream", &token).await.expect("open the stream");
    assert_eq!(events.status, 200);

    let start = Instant::now();
    fixture.write_parties(&[&plant.clone().disabled(), &other]);

    // The paused-clock timeout only turns a missed revocation into a clean
    // failure; the assertion below is on the paused clock too.
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match revoked.recv().await {
                Ok(ServerFrame::Message { .. }) => continue,
                other => return other,
            }
        }
    })
    .await
    .expect("the revoked connection must close");
    let elapsed = Instant::now() - start;
    assert!(matches!(closed, Err(NodeError::ConnectionClosed)), "expected the node to close, got {closed:?}");
    assert!(elapsed <= RELOAD_INTERVAL, "dropped after {elapsed:?}, more than the one-second check");

    // The SSE stream ends too, within the same check.
    let ended = tokio::time::timeout(Duration::from_secs(5), events.next_data())
        .await
        .expect("the revoked party's SSE stream must end");
    let elapsed = Instant::now() - start;
    assert!(matches!(ended, Ok(None)), "expected the stream to end, got {ended:?}");
    assert!(elapsed <= RELOAD_INTERVAL, "SSE stream ended after {elapsed:?}, more than the one-second check");
    let refused = sse_client.request(hyper::Method::GET, "/health", Some(&token), None).await.expect("reply");
    assert_eq!(refused.status, 401, "the revoked party's token is dropped with it");

    assert_eq!(node.auth.quotas.in_use(&plant.id), 0, "the revoked party's subscription slots are released");
    assert_eq!(node.stream.subscriber_count("log/output"), 0);
    assert_eq!(node.stream.subscriber_count("telemetry"), 0);

    kept.send(&ClientFrame::Ping).await.unwrap();
    assert!(matches!(reply(&mut kept).await, ServerFrame::Pong), "another party is untouched");
}

// ─────────────────────────────────────────────────────────────────────────────
// Reply discipline
// ─────────────────────────────────────────────────────────────────────────────

/// A party that may publish and subscribe to `log/output` and nothing else.
fn reply_party() -> TestParty {
    TestParty::new("north-plant").publish(&["log/output"]).subscribe(&["log/output"])
}

/// A client logged in over the node's real connection task.
async fn client_for(node: &DuplexNode, fixture: &Fixture, party: &TestParty, from: u8) -> StreamNodeClient {
    StreamNodeClient::connect_io(node.connect(peer(from)), &party.client_auth(fixture.cert.pin))
        .await
        .unwrap_or_else(|e| panic!("{} must log in: {e}", party.id))
}

/// Await `future` on the paused clock, so a request that never completes
/// fails cleanly instead of hanging.
async fn within<T>(what: &str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .unwrap_or_else(|_| panic!("{what} did not complete within 5 s on the paused clock"))
}

/// The five pipelined requests: a refused acknowledged publish, a `Subscribe`
/// that succeeds, a no-ack publish, a `Subscribe` that is refused, and an
/// acknowledged publish that succeeds.
fn pipelined_requests() -> Vec<ClientFrame> {
    vec![
        publish("telemetry", b"refused"),
        ClientFrame::Subscribe { topic: "log/output".into(), from_offset: None },
        ClientFrame::PublishNoAck { topic: "log/output".into(), payload: b"no ack".to_vec() },
        ClientFrame::Subscribe { topic: "telemetry".into(), from_offset: None },
        publish("log/output", b"acknowledged"),
    ]
}

/// A refused `Publish` returns its `Error` to its own caller, and the next
/// accepted publish returns its own offset.
#[tokio::test(start_paused = true)]
async fn refused_publish_returns_its_error_to_its_own_caller_and_the_next_publish_its_own_offset() {
    let fixture = Fixture::new();
    let plant = reply_party();
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let client = client_for(&node, &fixture, &plant, 1).await;

    let refused = within("the refused publish", client.publish("telemetry", bytes::Bytes::from_static(b"refused"))).await;
    match refused {
        Err(NodeError::RequestRefused { code, .. }) => assert_eq!(code, ERR_FORBIDDEN),
        other => panic!("the refused publish must get its own Error, got {other:?}"),
    }
    let accepted = within("the accepted publish", client.publish("log/output", bytes::Bytes::from_static(b"ok"))).await;
    assert_eq!(accepted.expect("the accepted publish"), 0, "the accepted publish gets its own offset");
}

/// A refused `Subscribe` returns an error to its caller and leaves no
/// subscriber channel.
#[tokio::test(start_paused = true)]
async fn refused_subscribe_returns_an_error_and_leaves_no_channel() {
    let fixture = Fixture::new();
    let plant = reply_party();
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let client = client_for(&node, &fixture, &plant, 1).await;

    match within("the refused subscribe", client.subscribe("telemetry", None)).await {
        Err(NodeError::RequestRefused { code, .. }) => assert_eq!(code, ERR_FORBIDDEN),
        Err(other) => panic!("expected the node's refusal, got {other}"),
        Ok(_) => panic!("a subscribe without a grant was accepted"),
    }
    assert!(!client.is_subscribed("telemetry"), "a refused subscribe leaves no channel");

    // Control: a granted subscribe keeps its channel.
    let _channel = within("the granted subscribe", client.subscribe("log/output", None)).await.expect("granted");
    assert!(client.is_subscribed("log/output"));
}

/// An accepted `Subscribe` returns after `Subscribed`, and no `Message` for it
/// arrives before that, replays included.
#[tokio::test(start_paused = true)]
async fn accepted_subscribe_returns_after_subscribed_and_no_message_arrives_before_it() {
    let fixture = Fixture::new();
    let plant = reply_party();
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    for payload in [b"m0", b"m1", b"m2"] {
        node.stream.producer("log/output").send_bytes(bytes::Bytes::from_static(payload));
    }

    // On the wire: `Subscribed` first, then the replayed Messages in order.
    let mut session = logged_in(&node, &fixture, &plant, 1).await;
    session.send(&ClientFrame::Subscribe { topic: "log/output".into(), from_offset: Some(0) }).await.unwrap();
    match reply(&mut session).await {
        ServerFrame::Subscribed { head } => assert_eq!(head, 3, "the topic's head when the subscription starts"),
        other => panic!("the first frame after Subscribe must be Subscribed, got {other:?}"),
    }
    for want in 0..3 {
        match reply(&mut session).await {
            ServerFrame::Message { offset, .. } => assert_eq!(offset, want),
            other => panic!("expected the replayed Message {want}, got {other:?}"),
        }
    }

    // Through the client: `subscribe` returns once `Subscribed` arrives, and
    // the replay follows on the channel.
    let client = client_for(&node, &fixture, &plant, 2).await;
    let mut channel = within("the subscribe", client.subscribe("log/output", Some(0))).await.expect("granted");
    for want in 0..3 {
        let (offset, _, _) = within("a replayed message", channel.recv()).await.expect("the channel is open");
        assert_eq!(offset, want);
    }
}

/// A reply whose kind does not match the front request closes the connection
/// as a protocol error. The server here answers a `Publish` with `Pong`.
#[tokio::test(start_paused = true)]
async fn reply_of_the_wrong_kind_closes_the_connection() {
    let fixture = Fixture::new();
    let plant = reply_party();
    let acceptor = fixture.cert.node_tls().tcp_acceptor();
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        let mut tls = acceptor.accept(server_io).await.expect("TLS");
        let challenge = ServerFrame::Challenge { node_id: "test-node".into(), nonce: [7; 32] };
        write_server_frame(&mut tls, &challenge).await.unwrap();
        let _hello = read_client_frame(&mut tls, 64 * 1024).await.expect("Hello");
        write_server_frame(&mut tls, &ServerFrame::Welcome { party_id: "north-plant".into() }).await.unwrap();
        let request = read_client_frame(&mut tls, 64 * 1024).await.expect("the Publish");
        assert!(matches!(request, ClientFrame::Publish { .. }), "got {request:?}");
        write_server_frame(&mut tls, &ServerFrame::Pong).await.unwrap();
        // What the client does next: it must close the connection.
        read_client_frame(&mut tls, 64 * 1024).await
    });

    let client = StreamNodeClient::connect_io(client_io, &plant.client_auth(fixture.cert.pin)).await.expect("logged in");
    let answered = within("the publish", client.publish("log/output", bytes::Bytes::from_static(b"x"))).await;
    assert!(matches!(answered, Err(NodeError::Protocol(_))), "a Pong for a Publish is a protocol error, got {answered:?}");
    let after = within("the close", server).await.expect("the fake server");
    assert!(matches!(after, Err(NodeError::ConnectionClosed)), "the client must close the connection, got {after:?}");
    let later = within("a later publish", client.publish("log/output", bytes::Bytes::from_static(b"y"))).await;
    assert!(matches!(later, Err(NodeError::ConnectionClosed)), "nothing goes out after the close, got {later:?}");
}

/// One connection, pipelined back to back (see `pipelined_requests`). On the
/// wire the replies are `Error`, `Subscribed`, nothing, `Error`, `Ack`, in that
/// order. Messages may come between replies, but none before `Subscribed`.
#[tokio::test(start_paused = true)]
async fn pipelined_requests_get_their_replies_in_order_on_the_wire() {
    let fixture = Fixture::new();
    let plant = reply_party();
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut session = logged_in(&node, &fixture, &plant, 1).await;

    for frame in pipelined_requests() {
        session.send(&frame).await.unwrap();
    }
    let mut replies = Vec::new();
    let mut messages = Vec::new();
    while replies.len() < 4 {
        match reply(&mut session).await {
            ServerFrame::Message { offset, .. } => {
                assert!(replies.len() >= 2, "a Message arrived before Subscribed: {replies:?}");
                messages.push(offset);
            }
            other => replies.push(other),
        }
    }
    assert!(matches!(replies[0], ServerFrame::Error { code, .. } if code == ERR_FORBIDDEN), "{replies:?}");
    assert!(matches!(replies[1], ServerFrame::Subscribed { head: 0 }), "{replies:?}");
    assert!(matches!(replies[2], ServerFrame::Error { code, .. } if code == ERR_FORBIDDEN), "{replies:?}");
    assert!(matches!(replies[3], ServerFrame::Ack { offset: 1 }), "{replies:?}");

    // The no-ack publish and the accepted publish reach the subscription, in order.
    while messages.len() < 2 {
        match reply(&mut session).await {
            ServerFrame::Message { offset, .. } => messages.push(offset),
            other => panic!("only Messages may follow the four replies, got {other:?}"),
        }
    }
    assert_eq!(messages, vec![0, 1]);
}

/// The same five requests through the client's own calls, started in that
/// order on one connection. Each waiter completes in request order with its
/// own reply: `Error`, `Subscribed`, `Error`, `Ack`. The no-ack publish has no
/// waiter.
#[tokio::test(start_paused = true)]
async fn pipelined_requests_through_the_client_complete_their_waiters_in_order() {
    let fixture = Fixture::new();
    let plant = reply_party();
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let client = client_for(&node, &fixture, &plant, 1).await;
    let completed: Arc<std::sync::Mutex<Vec<&'static str>>> = Arc::default();

    // Spawned in order on this current-thread runtime, so each task is first
    // polled, and writes its frame, in that order. A woken task runs in wake
    // order, so `completed` records the order the waiters completed in.
    let refused_publish = tokio::spawn({
        let (client, completed) = (client.clone(), Arc::clone(&completed));
        async move {
            let result = client.publish("telemetry", bytes::Bytes::from_static(b"refused")).await;
            completed.lock().unwrap().push("refused publish");
            result
        }
    });
    let subscribe = tokio::spawn({
        let (client, completed) = (client.clone(), Arc::clone(&completed));
        async move {
            let result = client.subscribe("log/output", None).await.map(|_channel| ());
            completed.lock().unwrap().push("subscribe");
            result
        }
    });
    let no_ack = tokio::spawn({
        let client = client.clone();
        async move { client.publish_no_ack("log/output", bytes::Bytes::from_static(b"no ack")).await }
    });
    let refused_subscribe = tokio::spawn({
        let (client, completed) = (client.clone(), Arc::clone(&completed));
        async move {
            let result = client.subscribe("telemetry", None).await.map(|_channel| ());
            completed.lock().unwrap().push("refused subscribe");
            result
        }
    });
    let accepted_publish = tokio::spawn({
        let (client, completed) = (client.clone(), Arc::clone(&completed));
        async move {
            let result = client.publish("log/output", bytes::Bytes::from_static(b"acknowledged")).await;
            completed.lock().unwrap().push("accepted publish");
            result
        }
    });

    let refused_publish = within("the refused publish", refused_publish).await.expect("task");
    let subscribe = within("the subscribe", subscribe).await.expect("task");
    let no_ack = within("the no-ack publish", no_ack).await.expect("task");
    let refused_subscribe = within("the refused subscribe", refused_subscribe).await.expect("task");
    let accepted_publish = within("the accepted publish", accepted_publish).await.expect("task");

    assert!(matches!(refused_publish, Err(NodeError::RequestRefused { code, .. }) if code == ERR_FORBIDDEN), "{refused_publish:?}");
    assert!(subscribe.is_ok(), "{subscribe:?}");
    assert!(no_ack.is_ok(), "{no_ack:?}");
    assert!(matches!(refused_subscribe, Err(NodeError::RequestRefused { code, .. }) if code == ERR_FORBIDDEN), "{refused_subscribe:?}");
    assert_eq!(accepted_publish.expect("the accepted publish"), 1, "its own offset: the no-ack publish took 0");
    assert_eq!(
        *completed.lock().unwrap(),
        vec!["refused publish", "subscribe", "refused subscribe", "accepted publish"],
        "the waiters complete in request order"
    );
    assert!(!client.is_subscribed("telemetry"), "the refused subscribe left no channel");
}

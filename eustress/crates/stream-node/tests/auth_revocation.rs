//! Revocation, grants, keys and limits, and the client's one writer task.
//!
//! A revoked party loses a connection even while its dispatch waits on a
//! write; every write has the 10-second deadline; an SSE stream ends at its
//! token's expiry and when its grant is removed; an unsubscribed forwarder
//! sends nothing stale, and its forwarder checks the stop flag under the
//! writer lock; a client registers a subscription's channel only when
//! `Subscribed` arrives; a repeated `Subscribe` is checked against the grant;
//! a reload that removes a grant ends the subscription; QUIC answers an
//! unvalidated address with a Retry; a cancelled client call leaves the reply
//! queue whole; one party holds at most 32 live tokens; a key change ends
//! what the old key held; IPv6 sources count by their /64; a revocation
//! during the login is seen; a bearer route takes no query parameters; REST
//! refusal lines are rate-limited per party; and the parties folder is
//! checked on every reload.
//!
//! Every test runs on the paused clock but the QUIC one, whose library needs
//! the real clock; that test asserts no durations.

mod common;

use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use bytes::Bytes;
use eustress_identity::keypair::UserKeypair;
use eustress_stream_node::auth::{
    self, ReloadOutcome, TokenClass, BROWSER_TOKEN_LIFETIME, ERR_FORBIDDEN, MAX_PRE_LOGIN_PER_IP,
    MAX_TOKENS_PER_PARTY, RELOAD_INTERVAL, WARN_WINDOW, WRITE_DEADLINE,
};
use eustress_stream_node::protocol::{read_client_frame, write_server_frame, ClientFrame, ServerFrame};
use eustress_stream_node::{NodeAuth, NodeError, StreamNodeClient};
use hyper::Method;
use tokio::io::DuplexStream;
use tokio::time::Instant;

use common::{peer, DuplexNode, DuplexRest, Fixture, RawSession, TestParty};

/// The polling step on the paused clock.
const STEP: Duration = Duration::from_millis(1);

/// What a measured instant may lie past the instant it is checked against:
/// a few polling steps on the paused clock, far below any interval tested.
const SLACK: Duration = Duration::from_millis(10);

/// A paused-clock bound that only turns an event that never comes into a
/// clean failure instead of a hang.
const GUARD: Duration = Duration::from_secs(60);

fn subscribe(topic: &str) -> ClientFrame {
    ClientFrame::Subscribe { topic: topic.to_string(), from_offset: None }
}

/// Log in over `io` as `party`.
async fn login_over(io: DuplexStream, fixture: &Fixture, party: &TestParty) -> RawSession<DuplexStream> {
    let mut session = RawSession::open(io, fixture.cert.pin).await.expect("TLS");
    match session.login(party).await.expect("reply") {
        ServerFrame::Welcome { .. } => session,
        other => panic!("{} must log in, got {other:?}", party.id),
    }
}

async fn logged_in(node: &DuplexNode, fixture: &Fixture, party: &TestParty, from: u8) -> RawSession<DuplexStream> {
    login_over(node.connect(peer(from)), fixture, party).await
}

/// The next frame from the node, within 5 seconds of the paused clock.
async fn reply(session: &mut RawSession<DuplexStream>) -> ServerFrame {
    tokio::time::timeout(Duration::from_secs(5), session.recv())
        .await
        .expect("a frame within 5 s on the paused clock")
        .expect("a frame")
}

/// Let every task that can run, run, without moving the clock.
async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

/// Publish enough to `topic` that its forwarding task fills the pipe to a
/// peer that does not read, and blocks in a write: 64 messages of 16 KB
/// against a 256 KB pipe.
fn fill_the_pipe(node: &DuplexNode, topic: &str) {
    let payload = Bytes::from(vec![0u8; 16 * 1024]);
    for _ in 0..64 {
        node.stream.producer(topic).send_bytes(payload.clone());
    }
}

/// Wait, on the paused clock, until `party` holds no quota slot and `topic`
/// has no subscriber; return when that was first seen.
async fn released(node: &DuplexNode, party: &str, topic: &str) -> Instant {
    tokio::time::timeout(GUARD, async {
        while node.auth.quotas.in_use(party) > 0 || node.stream.subscriber_count(topic) > 0 {
            tokio::time::sleep(STEP).await;
        }
        Instant::now()
    })
    .await
    .expect("the connection's subscriber and quota slots are released")
}

/// A token issued now for `party`, as the node holds it.
fn issue(auth: &NodeAuth, party: &TestParty, class: TokenClass) -> String {
    auth.tokens.issue(&common::enrolled(auth, &party.id), class, Instant::now()).expect("token").token
}

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

// ─────────────────────────────────────────────────────────────────────────────
// Revocation and the write deadline
// ─────────────────────────────────────────────────────────────────────────────

/// A party is revoked while its dispatch waits on a write to a peer that has
/// stopped reading. The revocation still lands within the one-second check:
/// the connection's subscriber and quota slots are released then, and the
/// connection ends within the check interval plus the write deadline.
/// Without the revocation race the dispatch would hold everything until its
/// own write deadline, 10 seconds.
#[tokio::test(start_paused = true)]
async fn a_party_revoked_while_its_dispatch_waits_on_a_stalled_write_is_closed_within_the_check_interval() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    node.auth.spawn_revocation_task(None);
    let (io, task) = node.connect_with_task(peer(1));
    let mut session = login_over(io, &fixture, &plant).await;
    session.send(&subscribe("log/output")).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Subscribed { .. }));

    // The peer stops reading. The forwarding task fills the pipe and blocks
    // in a write, holding the writer; the Ping's Pong then waits behind it,
    // so the dispatch waits on a write.
    fill_the_pipe(&node, "log/output");
    settle().await;
    session.send(&ClientFrame::Ping).await.unwrap();
    settle().await;

    let start = Instant::now();
    fixture.write_parties(&[&plant.clone().disabled()]);
    let at = released(&node, &plant.id, "log/output").await - start;
    assert!(at <= RELOAD_INTERVAL + SLACK, "the revoked connection's slots were released {at:?} after the revocation");

    tokio::time::timeout(RELOAD_INTERVAL + WRITE_DEADLINE, task)
        .await
        .expect("the connection ends within the check interval plus the write deadline")
        .expect("the connection task");
    let ended = Instant::now() - start;
    assert!(ended <= RELOAD_INTERVAL + WRITE_DEADLINE, "the connection ended {ended:?} after the revocation");
    drop(session);
}

/// A peer that stops reading is closed when a write to it misses the
/// 10-second deadline (here the forwarding task's write), and the
/// connection's slots are released then, measured on the paused clock.
#[tokio::test(start_paused = true)]
async fn a_stalled_peers_write_misses_the_10_second_deadline_and_the_connection_closes() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let (io, task) = node.connect_with_task(peer(1));
    let mut session = login_over(io, &fixture, &plant).await;
    session.send(&subscribe("log/output")).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Subscribed { .. }));

    let start = Instant::now();
    fill_the_pipe(&node, "log/output");
    let at = released(&node, &plant.id, "log/output").await - start;
    assert!(
        at >= WRITE_DEADLINE && at <= WRITE_DEADLINE + SLACK,
        "the stalled write closed the connection {at:?} after it began"
    );
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("the connection ends")
        .expect("the connection task");
    drop(session);
}

/// A REST reader that stops reading is closed when a write to it misses the
/// 10-second deadline, which frees its stream's subscriber and quota slots.
#[tokio::test(start_paused = true)]
async fn a_stalled_sse_reader_is_closed_at_the_write_deadline_and_frees_its_slots() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["telemetry"]);
    fixture.write_parties(&[&plant]);
    let rest = DuplexRest::start(&fixture);
    let token = issue(&rest.auth, &plant, TokenClass::Session);
    let mut client = rest.client(peer(1)).await;
    let events = client.open_stream("/topics/telemetry/stream", &token).await.expect("open the stream");
    assert_eq!(events.status, 200);
    assert_eq!(rest.auth.quotas.in_use(&plant.id), 1);

    // The reader stops reading; its stream's events (256 of about 22 KB each,
    // far more than the pipe and the HTTP and TLS buffers on both sides hold)
    // fill the pipe until the server's write waits on the reader.
    let start = Instant::now();
    let payload = Bytes::from(vec![0u8; 16 * 1024]);
    for _ in 0..256 {
        rest.stream.producer("telemetry").send_bytes(payload.clone());
    }
    let freed = tokio::time::timeout(GUARD, async {
        while rest.auth.quotas.in_use(&plant.id) > 0 || rest.stream.subscriber_count("telemetry") > 0 {
            tokio::time::sleep(STEP).await;
        }
        Instant::now()
    })
    .await
    .expect("the stalled reader's slots are freed");
    let at = freed - start;
    assert!(
        at >= WRITE_DEADLINE && at <= WRITE_DEADLINE + SLACK,
        "the stalled reader's slots were freed {at:?} after it stopped reading"
    );
    drop(events);
    drop(client);
}

// ─────────────────────────────────────────────────────────────────────────────
// SSE streams
// ─────────────────────────────────────────────────────────────────────────────

/// An SSE stream ends at its token's `expires_at`, on the paused clock. Just
/// before, it still delivers.
#[tokio::test(start_paused = true)]
async fn an_sse_stream_ends_at_its_tokens_expiry() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["telemetry"]);
    fixture.write_parties(&[&plant]);
    let rest = DuplexRest::start(&fixture);
    let issued_at = Instant::now();
    let token = issue(&rest.auth, &plant, TokenClass::Browser);
    let mut client = rest.client(peer(1)).await;
    let mut events = client.open_stream("/topics/telemetry/stream", &token).await.expect("open the stream");
    assert_eq!(events.status, 200);

    tokio::time::sleep_until(issued_at + BROWSER_TOKEN_LIFETIME - Duration::from_secs(1)).await;
    rest.stream.producer("telemetry").send_bytes(Bytes::from_static(b"still valid"));
    let data = events.next_data().await.expect("readable").expect("an event before the expiry");
    assert!(data.contains("payload_b64"), "{data}");

    let ended = tokio::time::timeout(GUARD, events.next_data())
        .await
        .expect("the stream must end at its token's expiry");
    assert!(matches!(ended, Ok(None)), "expected the stream to end, got {ended:?}");
    let at = Instant::now() - issued_at;
    assert!(
        at >= BROWSER_TOKEN_LIFETIME && at <= BROWSER_TOKEN_LIFETIME + SLACK,
        "the stream ended {at:?} after its token was issued"
    );
}

/// An SSE stream ends at the first reload that removes its party's grant for
/// the topic, within the one-second check on the paused clock. A stream
/// whose grant stays keeps delivering.
#[tokio::test(start_paused = true)]
async fn an_sse_stream_ends_at_the_first_reload_that_removes_its_grant() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["telemetry", "alerts"]);
    fixture.write_parties(&[&plant]);
    let rest = DuplexRest::start(&fixture);
    rest.auth.spawn_revocation_task(None);
    let token = issue(&rest.auth, &plant, TokenClass::Session);
    let mut first = rest.client(peer(1)).await;
    let mut narrowed = first.open_stream("/topics/telemetry/stream", &token).await.expect("open");
    assert_eq!(narrowed.status, 200);
    let mut second = rest.client(peer(2)).await;
    let mut kept = second.open_stream("/topics/alerts/stream", &token).await.expect("open");
    assert_eq!(kept.status, 200);

    let start = Instant::now();
    fixture.write_parties(&[&TestParty { subscribe: vec!["alerts".to_string()], ..plant.clone() }]);
    let ended = tokio::time::timeout(GUARD, narrowed.next_data())
        .await
        .expect("the stream must end when its grant is removed");
    assert!(matches!(ended, Ok(None)), "expected the stream to end, got {ended:?}");
    let at = Instant::now() - start;
    assert!(at <= RELOAD_INTERVAL + SLACK, "the stream ended {at:?} after its grant was removed");

    rest.stream.producer("alerts").send_bytes(Bytes::from_static(b"kept"));
    let data = tokio::time::timeout(Duration::from_secs(5), kept.next_data())
        .await
        .expect("the kept stream delivers")
        .expect("readable")
        .expect("an event");
    assert!(data.contains("payload_b64"), "{data}");
    assert_eq!(rest.auth.quotas.in_use(&plant.id), 1, "the ended stream's quota slot went back");
}

// ─────────────────────────────────────────────────────────────────────────────
// Subscriptions
// ─────────────────────────────────────────────────────────────────────────────

/// `Subscribe`, `Unsubscribe` and `Subscribe` again on one connection, while
/// the first subscription's forwarding task is blocked in a write with more
/// of its messages queued. Its messages may come before the second
/// `Subscribed`; none comes after it.
#[tokio::test(start_paused = true)]
async fn subscribe_unsubscribe_and_subscribe_again_deliver_no_stale_message() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut session = logged_in(&node, &fixture, &plant, 1).await;
    session.send(&subscribe("log/output")).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Subscribed { head: 0 }));

    fill_the_pipe(&node, "log/output");
    settle().await;
    session.send(&ClientFrame::Unsubscribe { topic: "log/output".into() }).await.unwrap();
    session.send(&subscribe("log/output")).await.unwrap();
    settle().await;

    // The peer reads again.
    let head = loop {
        match reply(&mut session).await {
            ServerFrame::Message { offset, .. } => assert!(offset < 64, "a Message from the first subscription: {offset}"),
            ServerFrame::Subscribed { head } => break head,
            other => panic!("expected Messages, then Subscribed, got {other:?}"),
        }
    };
    assert_eq!(head, 64, "the second subscription starts at the topic's head");
    node.stream.producer("log/output").send_bytes(Bytes::from_static(b"fresh"));
    match reply(&mut session).await {
        ServerFrame::Message { offset, .. } => {
            assert_eq!(offset, 64, "the first Message after the second Subscribed must be its own, not a stale one")
        }
        other => panic!("expected the fresh Message, got {other:?}"),
    }
    session.send(&ClientFrame::Ping).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Pong), "nothing stale follows");
}

/// An `Unsubscribe` that lands while the subscription's forwarding task is
/// part way through a frame leaves the stream decodable: the task finishes
/// that frame, whole, and writes nothing more; the next frame reads whole. The
/// frame (1 MB) is far larger than the pipe and the TLS buffer, so its write
/// is certainly part way when the `Unsubscribe` arrives. An aborted task
/// would leave half the frame on the wire.
#[tokio::test(start_paused = true)]
async fn an_unsubscribe_during_a_partly_written_frame_leaves_the_stream_decodable() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut session = logged_in(&node, &fixture, &plant, 1).await;
    session.send(&subscribe("log/output")).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Subscribed { head: 0 }));

    node.stream.producer("log/output").send_bytes(Bytes::from(vec![7u8; 1 << 20]));
    settle().await;
    session.send(&ClientFrame::Unsubscribe { topic: "log/output".into() }).await.unwrap();
    session.send(&ClientFrame::Ping).await.unwrap();
    settle().await;

    // The peer reads again: the frame in flight, whole, then the Pong.
    match reply(&mut session).await {
        ServerFrame::Message { offset, payload, .. } => {
            assert_eq!(offset, 0);
            assert_eq!(payload.len(), 1 << 20, "the frame in flight arrives whole");
        }
        other => panic!("expected the frame in flight, got {other:?}"),
    }
    assert!(matches!(reply(&mut session).await, ServerFrame::Pong), "the next frame reads whole");
}

/// The forwarder checks its stop flag after it has taken the writer lock,
/// never before. The old subscription's forwarder is held, by the test hook,
/// between receiving a message and taking the lock; an `Unsubscribe` and a
/// new `Subscribe` to the same topic run meanwhile, and `Subscribed` goes out.
/// Released, the old forwarder takes the lock, finds its flag set and drops
/// the message: no Message reaches the new subscriber before its first new
/// one. Needs `--features test-hooks`.
#[cfg(feature = "test-hooks")]
#[tokio::test(start_paused = true)]
async fn a_forwarder_paused_before_the_writer_lock_writes_nothing_after_a_new_subscribed() {
    use eustress_stream_node::handler::ForwarderGate;

    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let gate = Arc::new(ForwarderGate::default());
    assert!(node.ctx.forwarder_gate.set(Arc::clone(&gate)).is_ok(), "the gate is set before any connection");
    let client = StreamNodeClient::connect_io(node.connect(peer(1)), &plant.client_auth(fixture.cert.pin))
        .await
        .expect("logged in");
    let _old = tokio::time::timeout(Duration::from_secs(5), client.subscribe("log/output", None))
        .await
        .expect("the first subscribe completes")
        .expect("granted");

    // The old forwarder receives a message and waits at the pause point,
    // before the writer lock.
    gate.arm();
    let stale = node.stream.producer("log/output").send_bytes(Bytes::from_static(b"stale"));
    tokio::time::timeout(Duration::from_secs(5), gate.paused()).await.expect("the old forwarder pauses");

    // Unsubscribe, then a new Subscribe to the same topic, while it waits.
    client.unsubscribe("log/output").await.expect("the Unsubscribe is queued");
    let mut fresh = tokio::time::timeout(Duration::from_secs(5), client.subscribe("log/output", None))
        .await
        .expect("the second subscribe completes")
        .expect("granted");

    // Released, it takes the lock, sees its stop flag, and drops the message.
    gate.release();
    let own = node.stream.producer("log/output").send_bytes(Bytes::from_static(b"fresh"));
    let (offset, _, payload) = tokio::time::timeout(Duration::from_secs(5), fresh.recv())
        .await
        .expect("a message")
        .expect("the channel is open");
    assert_eq!(offset, own, "the new subscriber's first Message must be its own, not the stale one ({stale})");
    assert_eq!(payload, b"fresh");
    // A round trip later nothing else has come: the stale Message never did.
    tokio::time::timeout(Duration::from_secs(5), client.ping()).await.expect("a Pong").expect("ping");
    assert!(fresh.try_recv().is_err(), "no stale Message reached the new subscriber");
}

/// A repeated `Subscribe` to a topic the connection still holds, but whose
/// grant is gone, is refused, and the subscription it held ends. Only the
/// party set is swapped here, before any revocation pass, so the connection
/// still holds the subscription when the repeated `Subscribe` arrives.
#[tokio::test(start_paused = true)]
async fn a_repeated_subscribe_to_a_topic_no_longer_granted_is_refused() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut session = logged_in(&node, &fixture, &plant, 1).await;
    session.send(&subscribe("log/output")).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Subscribed { .. }));

    fixture.write_parties(&[&TestParty { subscribe: Vec::new(), ..plant.clone() }]);
    assert_eq!(node.auth.parties().reload(), ReloadOutcome::Replaced);
    session.send(&subscribe("log/output")).await.unwrap();
    match reply(&mut session).await {
        ServerFrame::Error { code, .. } => assert_eq!(code, ERR_FORBIDDEN),
        other => panic!("a repeated Subscribe to a topic no longer granted must be refused, got {other:?}"),
    }
    assert_eq!(node.stream.subscriber_count("log/output"), 0, "the subscription it still held ends with the refusal");
    assert_eq!(node.auth.quotas.in_use(&plant.id), 0);
}

/// A reload that removes a grant ends the connection's subscription to that
/// topic; the connection stays open, and a subscription still granted stays.
#[tokio::test(start_paused = true)]
async fn a_reload_that_removes_a_grant_ends_the_connections_subscription() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["log/output", "alerts"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let mut session = logged_in(&node, &fixture, &plant, 1).await;
    for topic in ["log/output", "alerts"] {
        session.send(&subscribe(topic)).await.unwrap();
        assert!(matches!(reply(&mut session).await, ServerFrame::Subscribed { .. }), "{topic}");
    }

    fixture.write_parties(&[&TestParty { subscribe: vec!["alerts".to_string()], ..plant.clone() }]);
    let report = node.auth.revocation_tick().await;
    assert_eq!(report.reload, ReloadOutcome::Replaced);

    // The connection applies the new set before it reads its next frame.
    session.send(&ClientFrame::Ping).await.unwrap();
    assert!(matches!(reply(&mut session).await, ServerFrame::Pong), "the connection stays open");
    assert_eq!(node.stream.subscriber_count("log/output"), 0, "the subscription to the topic no longer granted ended");
    assert_eq!(node.stream.subscriber_count("alerts"), 1, "the subscription still granted stays");
    assert_eq!(node.auth.quotas.in_use(&plant.id), 1);

    node.stream.producer("log/output").send_bytes(Bytes::from_static(b"gone"));
    node.stream.producer("alerts").send_bytes(Bytes::from_static(b"kept"));
    match reply(&mut session).await {
        ServerFrame::Message { topic, .. } => assert_eq!(topic, "alerts"),
        other => panic!("expected the kept topic's Message, got {other:?}"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// QUIC address validation
// ─────────────────────────────────────────────────────────────────────────────

/// A QUIC connection from an address the node has not validated receives a
/// Retry and takes no pre-login slot. A relay passes the client's packets to
/// the node and swallows every answer, so the client never sees the Retry and
/// its address is never validated: what a spoofed source looks like to the
/// node. A client that does see the Retry validates its address and logs in.
#[cfg(feature = "quic")]
#[tokio::test]
async fn a_quic_connection_from_an_unvalidated_address_gets_a_retry_and_takes_no_pre_login_slot() {
    use eustress_stream_node::quic::{QuicNode, QuicNodeClient};
    use tokio::net::UdpSocket;

    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = QuicNode::start(fixture.config()).await.expect("QUIC node starts");
    let node_addr = SocketAddr::from(([127, 0, 0, 1], node.listen_addr().port()));

    let front = UdpSocket::bind("127.0.0.1:0").await.expect("relay front");
    let back = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("relay back"));
    back.connect(node_addr).await.expect("relay to the node");
    let relay_addr = front.local_addr().expect("relay address");
    let (answers_tx, mut answers) = tokio::sync::mpsc::unbounded_channel::<u8>();
    let forward = {
        let back = Arc::clone(&back);
        tokio::spawn(async move {
            let mut buf = vec![0u8; 65_536];
            while let Ok((n, _)) = front.recv_from(&mut buf).await {
                let _ = back.send(&buf[..n]).await;
            }
        })
    };
    let swallow = tokio::spawn(async move {
        let mut buf = vec![0u8; 65_536];
        while let Ok(n) = back.recv(&mut buf).await {
            if n > 0 {
                let _ = answers_tx.send(buf[0]);
            }
        }
    });

    let attempt = tokio::spawn({
        let auth = plant.client_auth(fixture.cert.pin);
        async move { QuicNodeClient::connect_with_policy(relay_addr, &auth, &common::fixed_policy(0)).await.map(|_| ()) }
    });
    let first = tokio::time::timeout(Duration::from_secs(10), answers.recv())
        .await
        .expect("the node answers the client's Initial")
        .expect("an answer");
    // A QUIC v1 long header: the form bit, the fixed bit, then the packet
    // type, where 0b11 is Retry.
    assert_eq!(first & 0xF0, 0xF0, "the node must answer an unvalidated Initial with a Retry, got first byte {first:#04x}");
    assert_eq!(node.auth().pre_login.in_use(), 0, "an unvalidated address takes no pre-login slot");
    attempt.abort();
    forward.abort();
    swallow.abort();

    // Control: a client that receives the Retry proves its address, takes a
    // slot, and logs in.
    let client = QuicNodeClient::connect(node_addr, &plant.client_auth(fixture.cert.pin)).await;
    assert!(client.is_ok(), "a validated address logs in");
    node.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// The client's one writer task
// ─────────────────────────────────────────────────────────────────────────────

/// A stale `Message` that arrives before a new subscription's `Subscribed` is
/// not delivered to that subscription: the client registers the channel when
/// `Subscribed` arrives, not when it sends `Subscribe`. The fake node answers
/// the `Subscribe` with a Message for the topic first, as a frame of an
/// earlier subscription would arrive, then `Subscribed`, then the
/// subscription's own Message.
#[tokio::test(start_paused = true)]
async fn a_stale_message_before_subscribed_is_not_delivered_to_the_new_subscription() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    let acceptor = fixture.cert.node_tls().tcp_acceptor();
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        let mut tls = acceptor.accept(server_io).await.expect("TLS");
        let challenge = ServerFrame::Challenge { node_id: "test-node".into(), nonce: [7; 32] };
        write_server_frame(&mut tls, &challenge).await.unwrap();
        let _hello = read_client_frame(&mut tls, 64 * 1024).await.expect("Hello");
        write_server_frame(&mut tls, &ServerFrame::Welcome { party_id: "north-plant".into() }).await.unwrap();
        let request = read_client_frame(&mut tls, 64 * 1024).await.expect("the Subscribe");
        assert!(matches!(request, ClientFrame::Subscribe { .. }), "got {request:?}");
        let message = |offset: u64, payload: &[u8]| ServerFrame::Message {
            topic: "log/output".into(),
            offset,
            timestamp: 0,
            payload: payload.to_vec(),
        };
        write_server_frame(&mut tls, &message(7, b"stale")).await.unwrap();
        write_server_frame(&mut tls, &ServerFrame::Subscribed { head: 8 }).await.unwrap();
        write_server_frame(&mut tls, &message(8, b"fresh")).await.unwrap();
        // Hold the connection open until the client closes it.
        let _ = read_client_frame(&mut tls, 64 * 1024).await;
    });
    let client = StreamNodeClient::connect_io(client_io, &plant.client_auth(fixture.cert.pin)).await.expect("logged in");

    let mut channel = tokio::time::timeout(Duration::from_secs(5), client.subscribe("log/output", None))
        .await
        .expect("the subscribe completes")
        .expect("Subscribed");
    let (offset, _, payload) = tokio::time::timeout(Duration::from_secs(5), channel.recv())
        .await
        .expect("a message")
        .expect("the channel is open");
    assert_eq!(
        (offset, payload.as_slice()),
        (8, &b"fresh"[..]),
        "the first message must be the subscription's own, not the stale one before Subscribed"
    );
    drop(client);
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("the fake node finishes")
        .expect("the fake node");
}

/// A client request whose caller is dropped under backpressure never leaves a
/// queued request without its frame, and the next reply reaches its own
/// caller. The first request's frame (1 MB) is far larger than the pipe and
/// the TLS buffers, so its write blocks part way, and its caller gives up.
/// The node then reads again and answers each `Publish` with an `Ack` whose
/// offset names the request (the first payload byte).
#[tokio::test(start_paused = true)]
async fn a_request_whose_caller_is_dropped_under_backpressure_still_goes_out_whole_and_the_next_reply_reaches_its_own_caller() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    let acceptor = fixture.cert.node_tls().tcp_acceptor();
    let (client_io, server_io) = tokio::io::duplex(16 * 1024);
    let (resume_tx, resume_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let mut tls = acceptor.accept(server_io).await.expect("TLS");
        let challenge = ServerFrame::Challenge { node_id: "test-node".into(), nonce: [7; 32] };
        write_server_frame(&mut tls, &challenge).await.unwrap();
        let _hello = read_client_frame(&mut tls, 64 * 1024).await.expect("Hello");
        write_server_frame(&mut tls, &ServerFrame::Welcome { party_id: "north-plant".into() }).await.unwrap();
        // The node stops reading until the test says so.
        let _ = resume_rx.await;
        let mut seen = Vec::new();
        for _ in 0..2 {
            match read_client_frame(&mut tls, 16 * 1024 * 1024).await.expect("a whole frame") {
                ClientFrame::Publish { payload, .. } => {
                    let id = payload[0];
                    seen.push((id, payload.len()));
                    write_server_frame(&mut tls, &ServerFrame::Ack { offset: u64::from(id) }).await.unwrap();
                }
                other => panic!("expected a Publish, got {other:?}"),
            }
        }
        seen
    });
    let client = StreamNodeClient::connect_io(client_io, &plant.client_auth(fixture.cert.pin)).await.expect("logged in");

    let first = tokio::time::timeout(Duration::from_secs(1), client.publish("t", Bytes::from(vec![1u8; 1 << 20]))).await;
    assert!(first.is_err(), "the first caller is dropped while its frame is still being written");

    resume_tx.send(()).unwrap();
    let second = tokio::time::timeout(Duration::from_secs(5), client.publish("t", Bytes::from(vec![3u8; 16])))
        .await
        .expect("the next request completes")
        .expect("its reply");
    assert_eq!(second, 3, "the next reply reaches its own caller");
    let seen = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("the fake node finishes")
        .expect("the fake node");
    assert_eq!(seen, vec![(1, 1 << 20), (3, 16)], "the dropped caller's frame went out whole, before the next one");
}

// ─────────────────────────────────────────────────────────────────────────────
// Tokens and keys
// ─────────────────────────────────────────────────────────────────────────────

/// The 33rd live token for one party gets 429 with `Retry-After: 1`, for that
/// party only: another party still logs in.
#[tokio::test(start_paused = true)]
async fn the_33rd_live_token_for_one_party_gets_429_and_another_party_still_logs_in() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    let other = TestParty::new("south-plant");
    fixture.write_parties(&[&plant, &other]);
    let rest = DuplexRest::start(&fixture);
    let mut client = rest.client(peer(1)).await;
    for n in 0..MAX_TOKENS_PER_PARTY {
        client
            .login(&plant.id, &plant.keypair, TokenClass::Browser)
            .await
            .unwrap_or_else(|e| panic!("login {n} is within the cap: {e}"));
    }
    assert_eq!(rest.auth.tokens.live_for(&plant.id), MAX_TOKENS_PER_PARTY);

    // The 33rd, by hand, to read the status and the header.
    let challenge = client.challenge().await.expect("challenge");
    let message = auth::rest_transcript(
        &challenge.node_id,
        &challenge.nonce,
        &client.cert_sha256(),
        &plant.id,
        auth::PROTOCOL_VERSION,
        TokenClass::Browser,
    )
    .unwrap();
    let body = serde_json::json!({
        "party_id": plant.id,
        "nonce": STANDARD.encode(challenge.nonce),
        "token_class": "browser",
        "signature": plant.keypair.sign_bytes(&message),
    });
    let refused = client.request(Method::POST, "/v1/login", None, Some(&body)).await.expect("reply");
    assert_eq!(refused.status, 429, "the 33rd live token for one party");
    assert_eq!(refused.retry_after, Some(Duration::from_secs(1)), "429 carries Retry-After: 1");

    let token = client.login(&other.id, &other.keypair, TokenClass::Browser).await.expect("another party logs in");
    assert_eq!(token.class, TokenClass::Browser);
}

/// A key change under the same party id ends the sessions, tokens and streams
/// recorded under the old key's fingerprint, within the one-second check. The
/// new key logs in; another party is untouched.
#[tokio::test(start_paused = true)]
async fn a_key_change_under_the_same_party_id_ends_its_sessions_tokens_and_streams() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").subscribe(&["telemetry"]);
    let other = TestParty::new("south-plant").subscribe(&["telemetry"]);
    fixture.write_parties(&[&plant, &other]);
    let node = DuplexNode::start(&fixture);
    node.auth.spawn_revocation_task(None);
    let rest = DuplexRest::with_auth(Arc::clone(&node.auth), node.stream.clone(), fixture.cert.pin);

    let mut old_session = logged_in(&node, &fixture, &plant, 1).await;
    let old_token = issue(&node.auth, &plant, TokenClass::Browser);
    let mut sse_client = rest.client(peer(2)).await;
    let mut old_stream = sse_client.open_stream("/topics/telemetry/stream", &old_token).await.expect("open the stream");
    assert_eq!(old_stream.status, 200);
    let mut kept = logged_in(&node, &fixture, &other, 3).await;

    // The same id, a new key.
    let rekeyed = TestParty { keypair: Arc::new(UserKeypair::generate()), ..plant.clone() };
    let start = Instant::now();
    fixture.write_parties(&[&rekeyed, &other]);

    let closed = tokio::time::timeout(Duration::from_secs(5), old_session.recv())
        .await
        .expect("the old key's connection must close");
    assert!(matches!(closed, Err(NodeError::ConnectionClosed)), "expected the node to close, got {closed:?}");
    let at = Instant::now() - start;
    assert!(at <= RELOAD_INTERVAL + SLACK, "the old key's connection closed {at:?} after the key change");
    let ended = tokio::time::timeout(Duration::from_secs(5), old_stream.next_data())
        .await
        .expect("the old key's stream must end");
    assert!(matches!(ended, Ok(None)), "expected the stream to end, got {ended:?}");
    let refused = sse_client.request(Method::GET, "/health", Some(&old_token), None).await.expect("reply");
    assert_eq!(refused.status, 401, "the old key's token is dropped");

    let mut fresh = RawSession::open(node.connect(peer(4)), fixture.cert.pin).await.expect("TLS");
    assert!(matches!(fresh.login(&rekeyed).await.unwrap(), ServerFrame::Welcome { .. }), "the new key logs in");
    kept.send(&ClientFrame::Ping).await.unwrap();
    assert!(matches!(reply(&mut kept).await, ServerFrame::Pong), "another party is untouched");
}

// ─────────────────────────────────────────────────────────────────────────────
// Limits and the login window
// ─────────────────────────────────────────────────────────────────────────────

/// 17 connections from 17 addresses in one IPv6 /64 are capped at 16: the
/// /64 is one source. An address in another /64 is admitted.
#[tokio::test(start_paused = true)]
async fn seventeen_connections_from_seventeen_addresses_in_one_ipv6_64_are_capped_at_16() {
    let fixture = Fixture::new();
    fixture.write_parties(&[&TestParty::new("north-plant")]);
    let node = DuplexNode::start(&fixture);
    let in_one_64 =
        |n: u16| SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0x1, 0x2, 0xa, 0xb, 0xc, n)), 40_000 + n);

    let mut held = Vec::new();
    for n in 1..=(MAX_PRE_LOGIN_PER_IP as u16) {
        held.push(node.try_connect(in_one_64(n)).unwrap_or_else(|| panic!("address {n} is within the cap")));
    }
    assert!(node.try_connect(in_one_64(17)).is_none(), "the 17th address in the same /64 is refused");
    let other_64 = SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0x1, 0x3, 0, 0, 0, 1)), 40_100);
    assert!(node.try_connect(other_64).is_some(), "an address in another /64 is admitted");
    drop(held);
}

/// A revocation that lands between the login's check of the party and the
/// registration of its session is seen by the re-check that follows the
/// registration. Here the party set is swapped after the check (the party
/// `check_hello` admitted is kept), before any revocation pass could see the
/// session.
#[tokio::test]
async fn a_revocation_between_the_login_check_and_registration_is_seen_by_the_re_check() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let checked = common::enrolled(&node.auth, &plant.id);

    // Control: with nothing in between, the session registers.
    let session = node.auth.register_session(&checked).expect("an enabled party registers");
    assert_eq!(node.auth.sessions.live_for(&plant.id), 1);
    drop(session);

    fixture.write_parties(&[&plant.clone().disabled()]);
    assert_eq!(node.auth.parties().reload(), ReloadOutcome::Replaced);
    assert!(node.auth.register_session(&checked).is_none(), "the re-check after registration sees the revocation");
    assert_eq!(node.auth.sessions.live_for(&plant.id), 0, "and the session it registered is gone");
}

// ─────────────────────────────────────────────────────────────────────────────
// REST
// ─────────────────────────────────────────────────────────────────────────────

/// A bearer route refuses `?%74oken=` (an encoded `token`) and any other
/// query parameter, even beside a valid header: its allowlist is empty.
#[tokio::test(start_paused = true)]
async fn a_bearer_route_refuses_an_encoded_token_name_and_any_other_query_parameter() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let rest = DuplexRest::start(&fixture);
    let token = issue(&rest.auth, &plant, TokenClass::Session);
    let mut client = rest.client(peer(1)).await;
    for path in ["/health?%74oken=abc", "/health?foo=", "/health?foo"] {
        let refused = client.request(Method::GET, path, Some(&token), None).await.expect("reply");
        assert_eq!(refused.status, 400, "{path}: a bearer route takes no query parameters");
    }
    let control = client.request(Method::GET, "/health", Some(&token), None).await.expect("reply");
    assert_eq!(control.status, 200, "control: the same route without a query");
}

/// The warn lines for refused REST requests are rate-limited per party: one
/// line per party per 10 seconds, the next one reporting how many were held
/// back. Another party has its own window.
#[tokio::test(start_paused = true)]
async fn rest_refusal_warn_lines_are_rate_limited_per_party() {
    const LINE: &str = "REST request refused";
    let logs = Captured::default();
    let _logging = logs.install();
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    let other = TestParty::new("south-plant");
    fixture.write_parties(&[&plant, &other]);
    let rest = DuplexRest::start(&fixture);
    let token = issue(&rest.auth, &plant, TokenClass::Session);
    let other_token = issue(&rest.auth, &other, TokenClass::Session);
    let mut client = rest.client(peer(1)).await;

    for _ in 0..3 {
        let refused = client.request(Method::POST, "/topics/telemetry/publish", Some(&token), None).await.expect("reply");
        assert_eq!(refused.status, 403);
    }
    let refused = client.request(Method::POST, "/topics/telemetry/publish", Some(&other_token), None).await.expect("reply");
    assert_eq!(refused.status, 403);
    let lines = logs.lines_containing(LINE);
    assert_eq!(lines.iter().filter(|l| l.contains("north-plant")).count(), 1, "one line per party per window: {lines:?}");
    assert_eq!(lines.iter().filter(|l| l.contains("south-plant")).count(), 1, "another party has its own window: {lines:?}");

    tokio::time::advance(WARN_WINDOW).await;
    let refused = client.request(Method::POST, "/topics/telemetry/publish", Some(&token), None).await.expect("reply");
    assert_eq!(refused.status, 403);
    let lines: Vec<String> = logs.lines_containing(LINE).into_iter().filter(|l| l.contains("north-plant")).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[1].contains("suppressed_in_last_window=2"), "the next line reports the two held back: {}", lines[1]);
}

// ─────────────────────────────────────────────────────────────────────────────
// Files
// ─────────────────────────────────────────────────────────────────────────────

/// A reload whose parties folder has gained an access entry for another
/// account keeps the last good set, even though the file itself is still
/// owner-only. Control: an edit while the folder is protected is taken.
#[tokio::test(start_paused = true)]
async fn a_reload_whose_parties_folder_gained_an_extra_access_entry_keeps_the_last_good_set() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);

    let second = TestParty::new("second-plant");
    fixture.write_parties(&[&plant, &second]);
    assert_eq!(node.auth.revocation_tick().await.reload, ReloadOutcome::Replaced, "control: a protected folder");

    let intruder = TestParty::new("intruder").publish(&["scene_deltas"]).admin();
    fixture.write_parties(&[&plant, &second, &intruder]);
    common::grant_foreign_access(&fixture.private_dir);
    let report = node.auth.revocation_tick().await;
    assert!(matches!(report.reload, ReloadOutcome::KeptLastGood(_)), "{:?}", report.reload);
    assert!(node.auth.party_set().get("intruder").is_none(), "the new party must not be enrolled");
    assert!(node.auth.party_set().get("second-plant").is_some(), "the last good set stays in force");
}

//! Login: the TCP login sequence, its refusals, label separation, and the
//! limits before login.
//!
//! Connections run through the node's real connection task. Most tests feed
//! it over in-memory pipes (so a test chooses the source address and can run
//! on a paused clock); the first test and the accept-level limit test use
//! real loopback sockets through the real accept loop. The retry tests check
//! the native client's retry rule on TCP: what it retries, how long it waits,
//! and what it never retries.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use eustress_stream_node::auth::{self, LOGIN_DEADLINE, MAX_PRE_LOGIN_PER_IP, REASON_AUTH_FAILED, REASON_VERSION_UNSUPPORTED};
use eustress_stream_node::protocol::{write_server_frame, ClientFrame, ServerFrame};
use eustress_stream_node::transport;
use eustress_stream_node::{NodeError, StreamNode, StreamNodeClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;

use common::{peer, DuplexNode, Fixture, RawSession, TestParty};

fn decode(raw: &[u8]) -> ServerFrame {
    bincode::deserialize(raw).expect("a server frame")
}

/// An enrolled key logs in over a real socket and receives `Welcome`; the
/// session then works under its grants.
#[tokio::test(start_paused = true)]
async fn enrolled_key_logs_in_and_receives_welcome() {
    let _hold = common::hold_clock();
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = StreamNode::start(fixture.config()).await.expect("node starts");
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], node.listen_addr().port()));

    let client = StreamNodeClient::connect(addr, &plant.client_auth(fixture.cert.pin))
        .await
        .expect("an enrolled key logs in");
    let offset = client.publish("log/output", bytes::Bytes::from_static(b"hello")).await.expect("granted publish");
    assert_eq!(offset, 0);

    // The frame-level view of the same login: Welcome names the party.
    let tcp = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let mut raw = RawSession::open(tcp, fixture.cert.pin).await.expect("TLS and Challenge");
    assert_eq!(raw.node_id, "test-node");
    match raw.login(&plant).await.expect("reply") {
        ServerFrame::Welcome { party_id } => assert_eq!(party_id, "north-plant"),
        other => panic!("expected Welcome, got {other:?}"),
    }
    node.shutdown();
}

/// A wrong key, an unknown party and a disabled party each receive
/// `AuthFailed("auth_failed")`, byte for byte the same.
#[tokio::test(start_paused = true)]
async fn wrong_key_unknown_party_and_disabled_party_get_identical_auth_failed() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["log/output"]);
    let retired = TestParty::new("retired").disabled();
    fixture.write_parties(&[&plant, &retired]);
    let node = DuplexNode::start(&fixture);
    let impostor = TestParty::new("north-plant"); // same id, another key

    let mut replies = Vec::new();
    for (i, (party_id, keypair)) in [
        ("north-plant", &impostor.keypair),
        ("nobody", &impostor.keypair),
        ("retired", &retired.keypair),
    ]
    .into_iter()
    .enumerate()
    {
        let mut session = RawSession::open(node.connect(peer(10 + i as u8)), fixture.cert.pin).await.expect("TLS");
        let hello = session.hello(party_id, keypair, auth::PROTOCOL_VERSION);
        session.send(&hello).await.expect("send Hello");
        let raw = session.recv_raw().await.expect("a reply before the close");
        match decode(&raw) {
            ServerFrame::AuthFailed { reason } => assert_eq!(reason, REASON_AUTH_FAILED),
            other => panic!("{party_id}: expected AuthFailed, got {other:?}"),
        }
        assert!(matches!(session.recv().await, Err(NodeError::ConnectionClosed)), "{party_id}: the node closes after AuthFailed");
        replies.push(raw);
    }
    assert_eq!(replies[0], replies[1], "a wrong key and an unknown party must look the same");
    assert_eq!(replies[1], replies[2], "an unknown party and a disabled party must look the same");

    // Control: the enrolled key itself is welcome on the same node.
    let mut session = RawSession::open(node.connect(peer(20)), fixture.cert.pin).await.expect("TLS");
    assert!(matches!(session.login(&plant).await.expect("reply"), ServerFrame::Welcome { .. }));
}

/// A malformed `party_id` (empty, longer than 64 bytes, or with a space) is
/// refused with the same reply.
#[tokio::test(start_paused = true)]
async fn malformed_party_id_is_refused_with_the_same_reply() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);

    let reference = {
        let mut session = RawSession::open(node.connect(peer(1)), fixture.cert.pin).await.expect("TLS");
        let hello = session.hello("nobody", &plant.keypair, auth::PROTOCOL_VERSION);
        session.send(&hello).await.unwrap();
        session.recv_raw().await.expect("reply")
    };
    let long = "a".repeat(65);
    for (i, party_id) in ["", long.as_str(), "north plant"].into_iter().enumerate() {
        let mut session = RawSession::open(node.connect(peer(2 + i as u8)), fixture.cert.pin).await.expect("TLS");
        let hello = ClientFrame::Hello { protocol_version: auth::PROTOCOL_VERSION, party_id: party_id.to_string(), signature: vec![7; 64] };
        session.send(&hello).await.unwrap();
        let raw = session.recv_raw().await.expect("reply");
        assert_eq!(raw, reference, "malformed id {party_id:?} must get the same bytes as an unknown party");
        assert!(matches!(decode(&raw), ServerFrame::AuthFailed { ref reason } if reason == REASON_AUTH_FAILED));
    }
}

/// A `Hello` signed for connection A is rejected on connection B: each
/// connection has its own nonce and exporter.
#[tokio::test(start_paused = true)]
async fn hello_signed_for_connection_a_is_rejected_on_connection_b() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);

    let a = RawSession::open(node.connect(peer(1)), fixture.cert.pin).await.expect("TLS A");
    let hello_for_a = a.hello(&plant.id, &plant.keypair, auth::PROTOCOL_VERSION);
    let mut b = RawSession::open(node.connect(peer(2)), fixture.cert.pin).await.expect("TLS B");
    assert_ne!(a.nonce, b.nonce, "every connection gets a fresh nonce");

    b.send(&hello_for_a).await.unwrap();
    assert!(matches!(b.recv().await.expect("reply"), ServerFrame::AuthFailed { ref reason } if reason == REASON_AUTH_FAILED));

    // Control: the same party signing for B is welcome on B's twin.
    let mut c = RawSession::open(node.connect(peer(3)), fixture.cert.pin).await.expect("TLS C");
    assert!(matches!(c.login(&plant).await.expect("reply"), ServerFrame::Welcome { .. }));
}

/// A `Hello` with `protocol_version = 2` receives `AuthFailed("version_unsupported")`.
#[tokio::test(start_paused = true)]
async fn protocol_version_2_gets_version_unsupported() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);

    let mut session = RawSession::open(node.connect(peer(1)), fixture.cert.pin).await.expect("TLS");
    let hello = session.hello(&plant.id, &plant.keypair, 2);
    session.send(&hello).await.unwrap();
    match session.recv().await.expect("reply") {
        ServerFrame::AuthFailed { reason } => assert_eq!(reason, REASON_VERSION_UNSUPPORTED),
        other => panic!("expected AuthFailed, got {other:?}"),
    }
}

/// Two transcripts whose fields split differently produce different bytes.
#[test]
fn transcripts_whose_fields_split_differently_differ() {
    let nonce = [1u8; 32];
    let exporter = [2u8; 32];
    let ab_cd = auth::tcp_transcript("ab", &nonce, &exporter, "cd", 1).unwrap();
    let abc_d = auth::tcp_transcript("abc", &nonce, &exporter, "d", 1).unwrap();
    assert_ne!(ab_cd, abc_d);
}

/// A first frame that is not `Hello` closes the connection, with no reply.
#[tokio::test(start_paused = true)]
async fn first_frame_that_is_not_hello_closes_the_connection() {
    let fixture = Fixture::new();
    fixture.write_parties(&[&TestParty::new("north-plant")]);
    let node = DuplexNode::start(&fixture);

    let mut session = RawSession::open(node.connect(peer(1)), fixture.cert.pin).await.expect("TLS");
    session.send(&ClientFrame::Ping).await.unwrap();
    assert!(matches!(session.recv().await, Err(NodeError::ConnectionClosed)), "no Pong before login; the connection closes");
}

/// A pre-login frame over 64 KB closes the connection.
#[tokio::test(start_paused = true)]
async fn pre_login_frame_over_64_kb_closes_the_connection() {
    let fixture = Fixture::new();
    fixture.write_parties(&[&TestParty::new("north-plant")]);
    let node = DuplexNode::start(&fixture);

    let mut session = RawSession::open(node.connect(peer(1)), fixture.cert.pin).await.expect("TLS");
    let too_big = (auth::PRE_LOGIN_MAX_FRAME as u64) + 1;
    session.tls.write_all(&too_big.to_le_bytes()).await.unwrap();
    session.tls.write_all(&[0u8; 1024]).await.unwrap();
    session.tls.flush().await.unwrap();
    assert!(matches!(session.recv().await, Err(NodeError::ConnectionClosed)));
}

/// A TLS handshake that never completes closes the connection at 5 seconds,
/// measured on the paused clock, and frees its slot.
#[tokio::test(start_paused = true)]
async fn handshake_that_never_completes_closes_at_five_seconds_and_frees_its_slot() {
    let fixture = Fixture::new();
    fixture.write_parties(&[&TestParty::new("north-plant")]);
    let node = DuplexNode::start(&fixture);
    let source = peer(1);

    let start = Instant::now();
    let mut silent: Vec<_> = (0..MAX_PRE_LOGIN_PER_IP).map(|_| node.connect(source)).collect();
    assert!(node.try_connect(source).is_none(), "every slot for this source is held");

    for client in &mut silent {
        let mut buf = [0u8; 64];
        let read = client.read(&mut buf).await.expect("the pipe reports the close");
        assert_eq!(read, 0, "the node sends nothing and closes");
    }
    let elapsed = Instant::now() - start;
    assert!(
        elapsed >= LOGIN_DEADLINE && elapsed <= LOGIN_DEADLINE + Duration::from_millis(2),
        "closed at {elapsed:?} on the paused clock, expected 5 s"
    );
    assert_eq!(node.auth.pre_login.in_use_for(source.ip()), 0, "every slot is free again");
    assert!(node.try_connect(source).is_some(), "the source is admitted again");
}

/// The 17th pre-login connection from one source IP is refused while a
/// connection from another source IP is admitted and served. The addresses are
/// injected; on the paused clock nothing expires while the test runs.
#[tokio::test(start_paused = true)]
async fn seventeenth_pre_login_connection_from_one_ip_is_refused_while_another_ip_is_admitted() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let (a, b) = (peer(1), peer(2));

    let _held: Vec<_> = (0..MAX_PRE_LOGIN_PER_IP).map(|_| node.connect(a)).collect();
    assert!(node.try_connect(a).is_none(), "the 17th from one IP is refused at accept");
    let from_b = node.try_connect(b).expect("another IP is admitted");
    assert_eq!(node.auth.pre_login.in_use_for(a.ip()), MAX_PRE_LOGIN_PER_IP);

    let mut session = RawSession::open(from_b, fixture.cert.pin).await.expect("TLS from the other IP");
    assert!(matches!(session.login(&plant).await.expect("reply"), ServerFrame::Welcome { .. }));
}

/// The same limit at the real accept loop, from 127.0.0.1: 16 connections that
/// start TLS and stall hold every slot, the 17th is refused at accept, and
/// once the deadline frees the slots (advanced on the paused clock) the source
/// is served again.
#[tokio::test(start_paused = true)]
async fn accept_level_seventeenth_loopback_connection_is_refused() {
    let _hold = common::hold_clock();
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = StreamNode::start(fixture.config()).await.expect("node starts");
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], node.listen_addr().port()));

    let client_hello = {
        let config = std::sync::Arc::new(transport::pinned_client_config(fixture.cert.pin, &[]).unwrap());
        let mut connection = rustls::ClientConnection::new(config, transport::node_server_name().unwrap()).unwrap();
        let mut bytes = Vec::new();
        connection.write_tls(&mut bytes).unwrap();
        bytes
    };

    // Each of these gets the node's ServerHello back, so the node has admitted
    // it, and then stalls mid-handshake holding its slot.
    let mut stalled = Vec::new();
    for i in 0..MAX_PRE_LOGIN_PER_IP {
        let mut socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
        socket.write_all(&client_hello).await.unwrap();
        let mut buf = [0u8; 512];
        let n = socket.read(&mut buf).await.expect("ServerHello");
        assert!(n > 0, "connection {i} must be admitted and answered");
        stalled.push(socket);
    }

    let mut refused = tokio::net::TcpStream::connect(addr).await.expect("the kernel completes the TCP handshake");
    let _ = refused.write_all(&client_hello).await;
    let mut buf = [0u8; 512];
    match refused.read(&mut buf).await {
        Ok(0) | Err(_) => {}
        Ok(n) => panic!("the 17th connection from 127.0.0.1 was answered with {n} bytes; it must be refused at accept"),
    }

    tokio::time::advance(LOGIN_DEADLINE + Duration::from_millis(10)).await;
    for socket in &mut stalled {
        let mut buf = [0u8; 512];
        loop {
            match socket.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => continue,
            }
        }
    }
    assert_eq!(node.auth().pre_login.in_use(), 0);

    let client = StreamNodeClient::connect(addr, &plant.client_auth(fixture.cert.pin)).await;
    assert!(client.is_ok(), "slots freed by the deadline admit the source again");
    node.shutdown();
}

/// The relay test: a proxy with its own certificate, which the client is
/// configured to trust, forwards the login to a real node. The exporter
/// differs on the two legs, so the node refuses the signature.
#[tokio::test(start_paused = true)]
async fn relay_with_its_own_certificate_is_refused() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);

    let relay_cert = common::generate_cert();
    let relay_tls = relay_cert.node_tls();
    let (client_side, relay_front) = tokio::io::duplex(256 * 1024);
    let relay_back = node.connect(peer(1));
    let node_pin = fixture.cert.pin;
    tokio::spawn(async move {
        let Ok(mut front) = relay_tls.tcp_acceptor().accept(relay_front).await else { return };
        let Ok(mut back) = transport::tls_connect(relay_back, node_pin, &[]).await else { return };
        let _ = tokio::io::copy_bidirectional(&mut front, &mut back).await;
    });

    // The client trusts the relay: it is pinned to the relay's certificate.
    let result = StreamNodeClient::connect_io(client_side, &plant.client_auth(relay_cert.pin)).await;
    match result {
        Err(NodeError::AuthFailed(reason)) => assert_eq!(reason, REASON_AUTH_FAILED),
        Err(other) => panic!("expected the node to refuse the relayed login, got {other}"),
        Ok(_) => panic!("a relayed login succeeded: the exporter is not binding the signature to the connection"),
    }

    // Control: the same party connected directly is welcome.
    let direct = StreamNodeClient::connect_io(node.connect(peer(2)), &plant.client_auth(fixture.cert.pin)).await;
    assert!(direct.is_ok(), "the direct login must succeed");
}

/// A signature made over the TCP transcript is refused on REST: the labels
/// differ, so it does not verify.
#[tokio::test(start_paused = true)]
async fn tcp_signature_is_refused_on_rest() {
    let _hold = common::hold_clock();
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = StreamNode::start(fixture.config()).await.expect("node starts");
    let rest = node.start_rest_server().await.expect("REST starts");
    let addr = rest.local_addr().expect("bound").to_string();

    let mut client = eustress_stream_node::RestClient::connect(&addr, fixture.cert.pin).await.expect("TLS");
    let challenge = client.challenge().await.expect("challenge");
    let cert_hash = client.cert_sha256();
    // The TCP transcript, with every REST field in place except the label.
    let mut message = auth::tcp_transcript(&challenge.node_id, &challenge.nonce, &cert_hash, &plant.id, 1).unwrap();
    message.push(auth::TokenClass::Session.byte());
    let body = serde_json::json!({
        "party_id": plant.id,
        "nonce": STANDARD.encode(challenge.nonce),
        "token_class": "session",
        "signature": plant.keypair.sign_bytes(&message),
    });
    let reply = client.request(hyper::Method::POST, "/v1/login", None, Some(&body)).await.expect("reply");
    assert_eq!(reply.status, 401, "a TCP-labelled signature must not log in over REST");

    // Control: the REST transcript itself logs in.
    let token = client.login(&plant.id, &plant.keypair, auth::TokenClass::Session).await;
    assert!(token.is_ok(), "the REST transcript must log in");
    node.shutdown();
}

/// A signature made over the TCP transcript is refused on QUIC.
#[cfg(feature = "quic")]
#[tokio::test]
async fn tcp_signature_is_refused_on_quic() {
    use eustress_stream_node::protocol::{read_server_frame, write_client_frame};
    use eustress_stream_node::quic::{QuicNode, QuicNodeClient};

    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = QuicNode::start(fixture.config()).await.expect("QUIC node starts");
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], node.listen_addr().port()));

    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(transport::quic_client_config(fixture.cert.pin).unwrap());
    let connection = endpoint.connect(addr, transport::NODE_SERVER_NAME).unwrap().await.expect("QUIC handshake");
    let exporter = transport::quic_exporter(&connection).unwrap();
    let (mut send, mut recv) = connection.accept_bi().await.expect("the node opens the login stream");
    let ServerFrame::Challenge { node_id, nonce } = read_server_frame(&mut recv, 64 * 1024).await.unwrap() else {
        panic!("expected Challenge");
    };
    // Signed over the TCP transcript, with this QUIC connection's nonce and exporter.
    let message = auth::tcp_transcript(&node_id, &nonce, &exporter, &plant.id, 1).unwrap();
    let hello = ClientFrame::Hello { protocol_version: 1, party_id: plant.id.clone(), signature: common::sign(&plant.keypair, &message) };
    write_client_frame(&mut send, &hello).await.unwrap();
    match read_server_frame(&mut recv, 64 * 1024).await.expect("reply") {
        ServerFrame::AuthFailed { reason } => assert_eq!(reason, REASON_AUTH_FAILED),
        other => panic!("a TCP-labelled signature must not log in over QUIC, got {other:?}"),
    }

    // Control: the QUIC client signs the QUIC transcript and is welcome.
    let client = QuicNodeClient::connect(addr, &plant.client_auth(fixture.cert.pin)).await;
    assert!(client.is_ok(), "the QUIC transcript must log in");
    node.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// The retry rule on TCP
// ─────────────────────────────────────────────────────────────────────────────

/// Hold every pre-login slot for `source`: sixteen connections that never
/// start TLS. The node closes them at its 5-second deadline.
fn hold_every_slot(node: &DuplexNode, source: std::net::SocketAddr) -> Vec<tokio::io::DuplexStream> {
    let held: Vec<_> = (0..MAX_PRE_LOGIN_PER_IP).map(|_| node.connect(source)).collect();
    assert!(node.try_connect(source).is_none(), "every slot for this source is held");
    held
}

/// A TCP connection closed before `Challenge` is retried with the wait and
/// the jitter of the retry rule. The node refuses the first attempt at accept
/// (its 16 pre-login slots for this source are held), the slots come free at
/// one second, and the second attempt, one second plus 250 ms of jitter after
/// the first, logs in.
#[tokio::test(start_paused = true)]
async fn tcp_connection_closed_before_challenge_is_retried_after_the_wait_and_jitter() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let source = peer(1);
    let held = hold_every_slot(&node, source);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        drop(held);
    });

    let attempts = Mutex::new(Vec::new());
    let client = StreamNodeClient::connect_with(
        || {
            attempts.lock().unwrap().push(Instant::now());
            std::future::ready(Ok::<_, std::io::Error>(node.dial(source)))
        },
        &plant.client_auth(fixture.cert.pin),
        &common::fixed_policy(250),
    )
    .await;
    let attempts = attempts.into_inner().unwrap();
    assert_eq!(attempts.len(), 2, "one refusal at accept, then a login");
    let wait = attempts[1] - attempts[0];
    assert!(
        wait >= Duration::from_millis(1_250) && wait <= Duration::from_millis(1_252),
        "waited {wait:?} on the paused clock; the rule is one second plus the 250 ms jitter"
    );
    let client = client.expect("the second attempt logs in");
    client.publish("log/output", bytes::Bytes::from_static(b"after a retry")).await.expect("granted publish");
}

/// At most three attempts in all: a node that refuses every one at accept
/// fails the login after the third, with an error that names the refusal.
#[tokio::test(start_paused = true)]
async fn tcp_refused_on_all_three_attempts_fails_naming_the_refusal() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let source = peer(1);
    let _held = hold_every_slot(&node, source);

    let attempts = Mutex::new(Vec::new());
    let result = StreamNodeClient::connect_with(
        || {
            attempts.lock().unwrap().push(Instant::now());
            std::future::ready(Ok::<_, std::io::Error>(node.dial(source)))
        },
        &plant.client_auth(fixture.cert.pin),
        &common::fixed_policy(0),
    )
    .await;
    let attempts = attempts.into_inner().unwrap();
    assert_eq!(attempts.len(), 3, "three attempts in all, never a fourth");
    for pair in attempts.windows(2) {
        let wait = pair[1] - pair[0];
        assert!(wait >= Duration::from_secs(1) && wait <= Duration::from_millis(1_002), "waited {wait:?}");
    }
    match result {
        Err(error @ NodeError::RetriesExhausted { attempts: 3, .. }) => {
            assert!(error.to_string().contains("closed the connection"), "the error names the refusal: {error}");
        }
        Err(other) => panic!("expected the refusal after three attempts, got {other}"),
        Ok(_) => panic!("every attempt was refused at accept, yet the login succeeded"),
    }
}

/// A connection closed after `Challenge` has arrived is not retried: the
/// client makes exactly one attempt and returns an error. The server here
/// completes TLS, sends `Challenge`, and closes.
#[tokio::test(start_paused = true)]
async fn connection_closed_after_challenge_is_not_retried() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let acceptor = fixture.cert.node_tls().tcp_acceptor();

    let attempts = AtomicU32::new(0);
    let result = StreamNodeClient::connect_with(
        || {
            attempts.fetch_add(1, Ordering::SeqCst);
            let (client, server) = tokio::io::duplex(64 * 1024);
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(server).await else { return };
                let challenge = ServerFrame::Challenge { node_id: "test-node".to_string(), nonce: [7; 32] };
                let _ = write_server_frame(&mut tls, &challenge).await;
                let _ = tls.shutdown().await;
            });
            std::future::ready(Ok::<_, std::io::Error>(client))
        },
        &plant.client_auth(fixture.cert.pin),
        &common::fixed_policy(0),
    )
    .await;
    assert_eq!(attempts.load(Ordering::SeqCst), 1, "a close after Challenge must not be retried");
    match result {
        Err(NodeError::RetriesExhausted { .. }) => panic!("a close after Challenge was treated as a refusal and retried"),
        Err(_) => {}
        Ok(_) => panic!("the server closed after Challenge, yet the login succeeded"),
    }
}

/// `AuthFailed` is not retried: exactly one attempt, and the node's reason.
#[tokio::test(start_paused = true)]
async fn auth_failed_is_not_retried() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let impostor = TestParty::new("north-plant"); // the enrolled id, another key

    let attempts = AtomicU32::new(0);
    let result = StreamNodeClient::connect_with(
        || {
            attempts.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok::<_, std::io::Error>(node.dial(peer(1))))
        },
        &impostor.client_auth(fixture.cert.pin),
        &common::fixed_policy(0),
    )
    .await;
    assert_eq!(attempts.load(Ordering::SeqCst), 1, "AuthFailed must not be retried");
    match result {
        Err(NodeError::AuthFailed(reason)) => assert_eq!(reason, REASON_AUTH_FAILED),
        Err(other) => panic!("expected AuthFailed, got {other}"),
        Ok(_) => panic!("a wrong key logged in"),
    }
}

/// Under `test-hooks`, the dummy signature check is counted where it verifies:
/// a login for a party the node does not know, and a REST login with a nonce
/// the node never issued, each run exactly one verify against the timing key.
/// A timing key that does not decode, or an early return before the verify,
/// leaves a count at zero. Needs `--features test-hooks`.
#[cfg(feature = "test-hooks")]
#[tokio::test(start_paused = true)]
async fn an_unknown_party_and_a_bad_rest_nonce_each_make_exactly_one_dummy_verify() {
    use eustress_stream_node::auth::{burn_hook, TokenClass};

    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);

    // TCP: a login for a party the node does not know.
    burn_hook::take();
    let stranger = TestParty::new("nobody");
    let mut session = RawSession::open(node.connect(peer(30)), fixture.cert.pin).await.expect("TLS");
    match session.login(&stranger).await.expect("a reply before the close") {
        ServerFrame::AuthFailed { reason } => assert_eq!(reason, REASON_AUTH_FAILED),
        other => panic!("expected AuthFailed, got {other:?}"),
    }
    assert_eq!(burn_hook::take(), 1, "an unknown party's login runs one dummy verify");

    // REST: a login with a nonce the node never issued, signed by the enrolled key.
    let rest = common::DuplexRest::with_auth(std::sync::Arc::clone(&node.auth), node.stream.clone(), fixture.cert.pin);
    let mut client = rest.client(peer(31)).await;
    let node_id = client.challenge().await.expect("challenge").node_id;
    let never_issued = [0x5A; 32];
    let message = auth::rest_transcript(&node_id, &never_issued, &client.cert_sha256(), &plant.id, auth::PROTOCOL_VERSION, TokenClass::Session)
        .expect("transcript");
    let body = serde_json::json!({
        "party_id": plant.id,
        "nonce": STANDARD.encode(never_issued),
        "token_class": "session",
        "signature": plant.keypair.sign_bytes(&message),
    });
    burn_hook::take();
    let reply = client.request(hyper::Method::POST, "/v1/login", None, Some(&body)).await.expect("reply");
    assert_eq!(reply.status, 401, "a nonce the node never issued is refused");
    assert_eq!(burn_hook::take(), 1, "a bad REST nonce runs one dummy verify");
}

//! REST and SSE: bearer tokens, the certificate-bound login, token classes and
//! lifetimes, and SSE over `fetch` with the header.
//!
//! Most tests run the real REST server on a loopback socket and talk to it
//! with the native, pinned REST client. The token-lifetime tests run on a
//! paused clock with auto-advance held, so time moves only when the test
//! advances it. The nonce, cap and retry tests feed the same server over
//! in-memory pipes on a paused clock that advances by itself, so a client's
//! retry wait is measured exactly.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use eustress_stream_node::auth::{
    self, TokenClass, BROWSER_TOKEN_LIFETIME, CHALLENGE_LIFETIME, MAX_CHALLENGES_PER_IP, MAX_CHALLENGES_TOTAL,
    SESSION_TOKEN_LIFETIME,
};
use eustress_stream_node::client;
use eustress_stream_node::{NodeError, RestClient, RestServer, RetryPolicy, StreamNode};
use hyper::Method;
use tokio::time::Instant;

use common::{DuplexRest, Fixture, TestParty};

struct RestNode {
    node: StreamNode,
    _rest: RestServer,
    addr: String,
    fixture: Fixture,
}

impl RestNode {
    async fn start(parties: &[&TestParty]) -> RestNode {
        let fixture = Fixture::new();
        fixture.write_parties(parties);
        let node = StreamNode::start(fixture.config()).await.expect("node starts");
        let rest = node.start_rest_server().await.expect("REST starts");
        let addr = rest.local_addr().expect("bound").to_string();
        RestNode { node, _rest: rest, addr, fixture }
    }

    async fn client(&self) -> RestClient {
        RestClient::connect(&self.addr, self.fixture.cert.pin).await.expect("TLS to the REST port")
    }

    async fn token(&self, party: &TestParty, class: TokenClass) -> String {
        let mut client = self.client().await;
        client.login(&party.id, &party.keypair, class).await.expect("login").token
    }

    /// GET `path` on a fresh connection with `token` in the header.
    async fn get_status(&self, path: &str, token: Option<&str>) -> u16 {
        let mut client = self.client().await;
        client.request(Method::GET, path, token, None).await.expect("reply").status
    }
}

/// A request without a token gets 401.
#[tokio::test(start_paused = true)]
async fn request_without_token_gets_401() {
    let _hold = common::hold_clock();
    let plant = TestParty::new("north-plant").subscribe(&["telemetry"]);
    let node = RestNode::start(&[&plant]).await;

    for path in ["/health", "/topics", "/topics/telemetry/replay/0"] {
        assert_eq!(node.get_status(path, None).await, 401, "{path} without a token");
    }
    assert_eq!(node.get_status("/health", Some("bm90IGEgdG9rZW4=")).await, 401, "an unknown token");

    // Control: a token from a login opens the same routes.
    let token = node.token(&plant, TokenClass::Session).await;
    assert_eq!(node.get_status("/health", Some(&token)).await, 200);
    node.node.shutdown();
}

/// An expired token gets 401.
#[tokio::test(start_paused = true)]
async fn expired_token_gets_401() {
    let _hold = common::hold_clock();
    let plant = TestParty::new("north-plant");
    let node = RestNode::start(&[&plant]).await;

    let token = node.token(&plant, TokenClass::Session).await;
    assert_eq!(node.get_status("/health", Some(&token)).await, 200);
    tokio::time::advance(SESSION_TOKEN_LIFETIME + Duration::from_secs(1)).await;
    assert_eq!(node.get_status("/health", Some(&token)).await, 401);
    node.node.shutdown();
}

/// A `browser` token expires at 15 minutes and a `session` token at one hour,
/// measured on the paused clock.
#[tokio::test(start_paused = true)]
async fn browser_token_expires_at_15_minutes_and_session_token_at_one_hour() {
    let _hold = common::hold_clock();
    let plant = TestParty::new("north-plant");
    let node = RestNode::start(&[&plant]).await;

    let mut client = node.client().await;
    let browser = client.login(&plant.id, &plant.keypair, TokenClass::Browser).await.expect("browser login");
    let session = client.login(&plant.id, &plant.keypair, TokenClass::Session).await.expect("session login");
    assert_eq!(browser.expires_in, BROWSER_TOKEN_LIFETIME);
    assert_eq!(session.expires_in, SESSION_TOKEN_LIFETIME);
    assert_eq!(BROWSER_TOKEN_LIFETIME, Duration::from_secs(15 * 60));
    assert_eq!(SESSION_TOKEN_LIFETIME, Duration::from_secs(60 * 60));

    let second = Duration::from_secs(1);
    tokio::time::advance(BROWSER_TOKEN_LIFETIME - second).await;
    assert_eq!(node.get_status("/health", Some(&browser.token)).await, 200, "browser token just before 15 minutes");
    tokio::time::advance(second).await;
    assert_eq!(node.get_status("/health", Some(&browser.token)).await, 401, "browser token at 15 minutes");
    assert_eq!(node.get_status("/health", Some(&session.token)).await, 200, "session token at 15 minutes");

    tokio::time::advance(SESSION_TOKEN_LIFETIME - BROWSER_TOKEN_LIFETIME - second).await;
    assert_eq!(node.get_status("/health", Some(&session.token)).await, 200, "session token just before one hour");
    tokio::time::advance(second).await;
    assert_eq!(node.get_status("/health", Some(&session.token)).await, 401, "session token at one hour");
    node.node.shutdown();
}

/// A login whose signature was made over a different certificate hash (what a
/// relay presenting its own certificate would produce) is refused.
#[tokio::test(start_paused = true)]
async fn login_signed_over_another_certificate_hash_is_refused() {
    let _hold = common::hold_clock();
    let plant = TestParty::new("north-plant");
    let node = RestNode::start(&[&plant]).await;
    let mut client = node.client().await;

    let challenge = client.challenge().await.expect("challenge");
    let relay_hash = [0xEE; 32];
    assert_ne!(relay_hash, client.cert_sha256());
    let message = auth::rest_transcript(&challenge.node_id, &challenge.nonce, &relay_hash, &plant.id, 1, TokenClass::Session).unwrap();
    let body = serde_json::json!({
        "party_id": plant.id,
        "nonce": STANDARD.encode(challenge.nonce),
        "token_class": "session",
        "signature": plant.keypair.sign_bytes(&message),
    });
    let reply = client.request(Method::POST, "/v1/login", None, Some(&body)).await.expect("reply");
    assert_eq!(reply.status, 401, "a signature over another certificate's hash must be refused");

    // Control: the hash of the certificate this client saw logs in, with a
    // fresh challenge (the first nonce was used up by the refused attempt).
    let challenge = client.challenge().await.expect("challenge");
    let message = auth::rest_transcript(&challenge.node_id, &challenge.nonce, &client.cert_sha256(), &plant.id, 1, TokenClass::Session).unwrap();
    let body = serde_json::json!({
        "party_id": plant.id,
        "nonce": STANDARD.encode(challenge.nonce),
        "token_class": "session",
        "signature": plant.keypair.sign_bytes(&message),
    });
    let reply = client.request(Method::POST, "/v1/login", None, Some(&body)).await.expect("reply");
    assert_eq!(reply.status, 200, "the real certificate hash must log in");
    node.node.shutdown();
}

/// A login with no signature, or with an unknown `token_class`, is refused.
#[tokio::test(start_paused = true)]
async fn login_with_no_signature_or_an_unknown_token_class_is_refused() {
    let _hold = common::hold_clock();
    let plant = TestParty::new("north-plant");
    let node = RestNode::start(&[&plant]).await;
    let mut client = node.client().await;

    let challenge = client.challenge().await.expect("challenge");
    let nonce = STANDARD.encode(challenge.nonce);
    let unsigned = serde_json::json!({ "party_id": plant.id, "nonce": nonce, "token_class": "session" });
    let reply = client.request(Method::POST, "/v1/login", None, Some(&unsigned)).await.expect("reply");
    assert_eq!(reply.status, 401, "no signature");

    let message = auth::rest_transcript(&challenge.node_id, &challenge.nonce, &client.cert_sha256(), &plant.id, 1, TokenClass::Session).unwrap();
    let signature = plant.keypair.sign_bytes(&message);
    let unknown_class = serde_json::json!({ "party_id": plant.id, "nonce": nonce, "token_class": "admin", "signature": signature });
    let reply = client.request(Method::POST, "/v1/login", None, Some(&unknown_class)).await.expect("reply");
    assert_eq!(reply.status, 400, "an unknown token_class");

    // The class is signed: a session signature presented as a browser login fails.
    let swapped = serde_json::json!({ "party_id": plant.id, "nonce": nonce, "token_class": "browser", "signature": signature });
    let reply = client.request(Method::POST, "/v1/login", None, Some(&swapped)).await.expect("reply");
    assert_eq!(reply.status, 401, "the token class is part of the signed transcript");

    // Control: a fresh challenge, signed for its own class, logs in. (The
    // first nonce was used up by the swapped attempt.)
    let challenge = client.challenge().await.expect("challenge");
    let message = auth::rest_transcript(&challenge.node_id, &challenge.nonce, &client.cert_sha256(), &plant.id, 1, TokenClass::Session).unwrap();
    let good = serde_json::json!({
        "party_id": plant.id,
        "nonce": STANDARD.encode(challenge.nonce),
        "token_class": "session",
        "signature": plant.keypair.sign_bytes(&message),
    });
    let reply = client.request(Method::POST, "/v1/login", None, Some(&good)).await.expect("reply");
    assert_eq!(reply.status, 200);
    node.node.shutdown();
}

/// A token in the query string is refused.
#[tokio::test(start_paused = true)]
async fn token_in_the_query_string_is_refused() {
    let _hold = common::hold_clock();
    let plant = TestParty::new("north-plant").subscribe(&["telemetry"]);
    let node = RestNode::start(&[&plant]).await;
    let token = node.token(&plant, TokenClass::Session).await;
    let encoded: String = token.chars().map(|c| match c { '+' => "%2B".to_string(), '/' => "%2F".to_string(), '=' => "%3D".to_string(), c => c.to_string() }).collect();

    let in_query = format!("/health?token={encoded}");
    assert_ne!(node.get_status(&in_query, None).await, 200, "a token in the query string alone");
    assert_eq!(node.get_status(&in_query, Some(&token)).await, 400, "a query token is refused even beside a valid header");
    let access_token = format!("/topics/telemetry/replay/0?access_token={encoded}");
    assert_eq!(node.get_status(&access_token, Some(&token)).await, 400);
    // A bearer route takes no query parameters at all, so the replay offset
    // is in the path, and no replay route reads it from the query string.
    assert_eq!(node.get_status("/topics/telemetry/replay/0?from=0", Some(&token)).await, 400);
    assert_eq!(node.get_status("/topics/telemetry/replay?from=0", Some(&token)).await, 404, "no query replay route");

    // Control: the header alone works, and the offset in the path works.
    assert_eq!(node.get_status("/health", Some(&token)).await, 200);
    assert_eq!(node.get_status("/topics/telemetry/replay/0", Some(&token)).await, 200);
    node.node.shutdown();
}

/// An SSE stream opened the way a browser `fetch` does it, with the bearer
/// token in the `Authorization` header, delivers a published message.
#[tokio::test(start_paused = true)]
async fn sse_stream_opened_with_fetch_and_the_header_delivers_a_published_message() {
    let _hold = common::hold_clock();
    let plant = TestParty::new("north-plant").subscribe(&["telemetry"]);
    let node = RestNode::start(&[&plant]).await;
    let token = node.token(&plant, TokenClass::Browser).await;

    let mut client = node.client().await;
    let mut events = client.open_stream("/topics/telemetry/stream", &token).await.expect("open the stream");
    assert_eq!(events.status, 200);

    // The subscription exists before the response starts, so this message is
    // delivered to it.
    node.node.stream().producer("telemetry").send_bytes(bytes::Bytes::from_static(b"hello"));
    let data = events.next_data().await.expect("stream readable").expect("an event before the end");
    let event: serde_json::Value = serde_json::from_str(&data).expect("JSON event");
    assert_eq!(event["topic"], "telemetry");
    assert_eq!(event["payload_b64"], "aGVsbG8=");

    // A topic without a subscribe grant is refused.
    let mut other = node.client().await;
    let refused = other.open_stream("/topics/scene_deltas/stream", &token).await.expect("reply");
    assert_eq!(refused.status, 403);
    node.node.shutdown();
}

/// The 17th challenge from one source address gets 429 with `Retry-After: 1`.
#[tokio::test(start_paused = true)]
async fn the_17th_challenge_from_one_source_gets_429_with_retry_after_1() {
    let _hold = common::hold_clock();
    let plant = TestParty::new("north-plant");
    let node = RestNode::start(&[&plant]).await;
    let mut client = node.client().await;
    for _ in 0..MAX_CHALLENGES_PER_IP {
        client.challenge().await.expect("within the per-source limit");
    }
    let reply = client.request(Method::POST, "/v1/login/challenge", None, None).await.expect("reply");
    assert_eq!(reply.status, 429);
    assert_eq!(reply.retry_after, Some(Duration::from_secs(1)), "429 carries Retry-After: 1");
    node.node.shutdown();
}

/// A login with an unknown, expired or already-used nonce is refused with the
/// same reply as a bad signature, byte for byte.
#[tokio::test(start_paused = true)]
async fn login_with_an_unknown_expired_or_used_nonce_gets_the_bad_signature_reply() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let rest = DuplexRest::start(&fixture);
    let source = common::peer(1);

    let mut client = rest.client(source).await;
    let cert_hash = client.cert_sha256();
    let body = |nonce: &[u8; 32], node_id: &str, signer: &TestParty| {
        let message = auth::rest_transcript(node_id, nonce, &cert_hash, &plant.id, 1, TokenClass::Session).unwrap();
        serde_json::json!({
            "party_id": plant.id,
            "nonce": STANDARD.encode(nonce),
            "token_class": "session",
            "signature": signer.keypair.sign_bytes(&message),
        })
    };

    // The reference: a live nonce with a signature by another key.
    let impostor = TestParty::new("north-plant");
    let challenge = client.challenge().await.expect("challenge");
    let bad_signature = client
        .request(Method::POST, "/v1/login", None, Some(&body(&challenge.nonce, &challenge.node_id, &impostor)))
        .await
        .expect("reply");
    assert_eq!(bad_signature.status, 401);

    // Unknown: a nonce the node never issued, signed by the enrolled key.
    let unknown = client
        .request(Method::POST, "/v1/login", None, Some(&body(&[0x5A; 32], &challenge.node_id, &plant)))
        .await
        .expect("reply");

    // Used: a nonce that already logged in once.
    let challenge = client.challenge().await.expect("challenge");
    let first = client
        .request(Method::POST, "/v1/login", None, Some(&body(&challenge.nonce, &challenge.node_id, &plant)))
        .await
        .expect("reply");
    assert_eq!(first.status, 200, "control: a live nonce and the enrolled key log in");
    let reused = client
        .request(Method::POST, "/v1/login", None, Some(&body(&challenge.nonce, &challenge.node_id, &plant)))
        .await
        .expect("reply");

    // Expired: a nonce older than its 5 seconds, measured on the paused clock.
    let challenge = client.challenge().await.expect("challenge");
    tokio::time::advance(CHALLENGE_LIFETIME).await;
    let mut late = rest.client(source).await;
    let expired = late
        .request(Method::POST, "/v1/login", None, Some(&body(&challenge.nonce, &challenge.node_id, &plant)))
        .await
        .expect("reply");

    for (name, reply) in [("unknown", &unknown), ("used", &reused), ("expired", &expired)] {
        assert_eq!(reply.status, bad_signature.status, "{name} nonce");
        assert_eq!(reply.body, bad_signature.body, "{name} nonce: the reply must be the bad-signature reply");
    }
}

/// The 4097th outstanding challenge gets 503 with `Retry-After: 1`.
#[tokio::test(start_paused = true)]
async fn the_4097th_outstanding_challenge_gets_503_with_retry_after_1() {
    let fixture = Fixture::new();
    fixture.write_parties(&[&TestParty::new("north-plant")]);
    let rest = DuplexRest::start(&fixture);
    let now = Instant::now();
    for n in 0..(MAX_CHALLENGES_TOTAL / MAX_CHALLENGES_PER_IP) {
        let source = std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 9, (n / 256) as u8, (n % 256) as u8));
        for _ in 0..MAX_CHALLENGES_PER_IP {
            rest.auth.challenges.issue(source, now).expect("within both caps");
        }
    }
    assert_eq!(rest.auth.challenges.outstanding(), MAX_CHALLENGES_TOTAL);

    // From a source with none outstanding, so only the total cap applies.
    let mut client = rest.client(common::peer(1)).await;
    let reply = client.request(Method::POST, "/v1/login/challenge", None, None).await.expect("reply");
    assert_eq!(reply.status, 503);
    assert_eq!(reply.retry_after, Some(Duration::from_secs(1)), "503 carries Retry-After: 1");
}

/// A native client refused with 429 retries after the wait and succeeds.
/// Sixteen challenges from its address were issued 4.5 s earlier, so the
/// first attempt is refused; they expire at 5 s, and the second attempt,
/// `Retry-After: 1` plus 250 ms of jitter later, logs in.
#[tokio::test(start_paused = true)]
async fn a_client_refused_with_429_retries_after_the_wait_and_succeeds() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let rest = DuplexRest::start(&fixture);
    let source = common::peer(1);
    let issued_at = Instant::now();
    for _ in 0..MAX_CHALLENGES_PER_IP {
        rest.auth.challenges.issue(source.ip(), issued_at).expect("within the cap");
    }
    tokio::time::advance(Duration::from_millis(4_500)).await;

    let attempts = Mutex::new(Vec::new());
    let token = client::rest_login_with(
        || {
            attempts.lock().unwrap().push(Instant::now());
            std::future::ready(Ok::<_, std::io::Error>(rest.dial(source)))
        },
        rest.pin,
        &plant.id,
        &plant.keypair,
        TokenClass::Session,
        &common::fixed_policy(250),
    )
    .await
    .expect("the second attempt logs in");
    let attempts = attempts.into_inner().unwrap();
    assert_eq!(attempts.len(), 2, "one 429, then a login");
    let wait = attempts[1] - attempts[0];
    assert!(
        wait >= Duration::from_millis(1_250) && wait <= Duration::from_millis(1_252),
        "waited {wait:?} on the paused clock; the rule is Retry-After (1 s) plus the 250 ms jitter"
    );
    assert_eq!(token.class, TokenClass::Session);
}

/// A native client refused on all three attempts fails with an error that
/// names the refusal.
#[tokio::test(start_paused = true)]
async fn a_client_refused_on_all_three_attempts_fails_naming_the_refusal() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let rest = DuplexRest::start(&fixture);
    let source = common::peer(1);
    let now = Instant::now();
    for _ in 0..MAX_CHALLENGES_PER_IP {
        rest.auth.challenges.issue(source.ip(), now).expect("within the cap");
    }

    let attempts = Mutex::new(Vec::new());
    let result = client::rest_login_with(
        || {
            attempts.lock().unwrap().push(Instant::now());
            std::future::ready(Ok::<_, std::io::Error>(rest.dial(source)))
        },
        rest.pin,
        &plant.id,
        &plant.keypair,
        TokenClass::Session,
        &common::fixed_policy(0),
    )
    .await;
    assert_eq!(attempts.into_inner().unwrap().len(), 3, "three attempts in all, never a fourth");
    match result {
        Err(error @ NodeError::RetriesExhausted { attempts: 3, .. }) => {
            assert!(error.to_string().contains("HTTP 429"), "the error names the refusal: {error}");
        }
        other => panic!("expected the 429 refusal after three attempts, got {other:?}"),
    }
}

/// The jitter stays within 0 to 500 milliseconds, both included, whatever
/// the random source gives.
#[test]
fn the_jitter_stays_within_0_to_500_ms() {
    for value in [0, 1, 250, 499, 500, 501, 1_000, 123_456_789, u64::MAX - 1, u64::MAX] {
        let policy = RetryPolicy::with_random(Arc::new(common::FixedRandom(value)));
        let jitter = policy.jitter();
        assert!(jitter <= Duration::from_millis(500), "source value {value} gave {jitter:?}");
        let wait = policy.wait_before_retry(None);
        assert_eq!(wait, Duration::from_secs(1) + jitter, "no Retry-After means one second plus the jitter");
    }
    // The whole range is reachable: 0 and 500 both come out.
    assert_eq!(RetryPolicy::with_random(Arc::new(common::FixedRandom(0))).jitter(), Duration::ZERO);
    assert_eq!(RetryPolicy::with_random(Arc::new(common::FixedRandom(500))).jitter(), Duration::from_millis(500));
}

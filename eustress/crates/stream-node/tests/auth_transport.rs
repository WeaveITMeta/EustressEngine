//! Transport: the SPKI pin, the handshake signature, TLS 1.3 only, no QUIC
//! early data, the retry rule on QUIC, and a ratchet over the crate's source.
//!
//! QUIC tests need `--features quic` and run on the real clock: quinn's own
//! timers (acknowledgement delay, loss detection) must run. QUIC cannot be
//! configured for TLS 1.2 at all, so for QUIC the TLS 1.3 guard is the type
//! system and there is no TLS 1.2 QUIC test.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eustress_identity::private_file;
use eustress_stream_node::auth::{TokenClass, BROWSER_TOKEN_LIFETIME};
use eustress_stream_node::client::{self, TokenRequest};
use eustress_stream_node::transport;
use eustress_stream_node::{NodeError, RestClient, StreamNode, StreamNodeClient};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use common::{peer, DuplexNode, Fixture, TestCert, TestParty};

// ─────────────────────────────────────────────────────────────────────────────
// Test servers
// ─────────────────────────────────────────────────────────────────────────────

/// Always answers with one certificate and signing key.
#[derive(Debug)]
struct FixedKey(Arc<rustls::sign::CertifiedKey>);

impl rustls::server::ResolvesServerCert for FixedKey {
    fn resolve(&self, _hello: rustls::server::ClientHello<'_>) -> Option<Arc<rustls::sign::CertifiedKey>> {
        Some(Arc::clone(&self.0))
    }
}

/// A TLS 1.3 server configuration that presents `presented`'s certificate and
/// signs the handshake with `signer`'s private key. With two different
/// certificates this is the pinned certificate replayed by someone who does
/// not hold its key: the CertificateVerify signature does not match the
/// certificate.
///
/// No server-side check is bypassed: `CertifiedKey::new` does not compare the
/// key with the certificate (only `CertifiedKey::from_der` and
/// `with_single_cert` do), and the handshake signs with whatever key the
/// resolver returns.
// VERIFY: CertifiedKey::new(Vec<CertificateDer<'static>>, Arc<dyn SigningKey>)
// performs no key match (rustls-0.23.41/src/crypto/signer.rs), and
// CertifiedKey derives Debug (needed by FixedKey's derive).
fn presenting(presented: &TestCert, signer: &TestCert, alpn: &[&[u8]]) -> rustls::ServerConfig {
    let provider = transport::crypto_provider();
    let key = PrivateKeyDer::from_pem_slice(signer.key_pem.as_bytes()).expect("key PEM");
    let signing_key = provider.key_provider.load_private_key(key).expect("load the signing key");
    let certified = rustls::sign::CertifiedKey::new(vec![CertificateDer::from(presented.cert_der.clone())], signing_key);
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3")
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(FixedKey(Arc::new(certified))));
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    config
}

/// A WebPKI client that trusts `cert` as a root, restricted to `versions`.
/// It verifies TLS 1.2 handshakes normally, so a refusal comes from the node.
fn webpki_client(cert: &TestCert, versions: &[&'static rustls::SupportedProtocolVersion]) -> rustls::ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(cert.cert_der.clone())).expect("the test certificate as a root");
    rustls::ClientConfig::builder_with_provider(transport::crypto_provider())
        .with_protocol_versions(versions)
        .expect("protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

async fn rest_node(fixture: &Fixture) -> (StreamNode, eustress_stream_node::RestServer, String) {
    let node = StreamNode::start(fixture.config()).await.expect("node starts");
    let rest = node.start_rest_server().await.expect("REST starts");
    let addr = rest.local_addr().expect("bound").to_string();
    (node, rest, addr)
}

// ─────────────────────────────────────────────────────────────────────────────
// The pin
// ─────────────────────────────────────────────────────────────────────────────

/// A TCP client pinned to one certificate refuses a node that presents
/// another. Fails when the pin comparison is removed.
#[tokio::test(start_paused = true)]
async fn tcp_client_refuses_a_node_with_another_certificate() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);
    let other = common::generate_cert();

    assert!(transport::tls_connect(node.connect(peer(1)), other.pin, &[]).await.is_err(), "another certificate's pin");
    let login = StreamNodeClient::connect_io(node.connect(peer(2)), &plant.client_auth(other.pin)).await;
    assert!(matches!(login, Err(NodeError::Tls(_))), "the client must refuse before it signs anything");

    // Control: pinned to the node's own certificate.
    assert!(transport::tls_connect(node.connect(peer(3)), fixture.cert.pin, &[]).await.is_ok());
}

/// The REST client and `party token` refuse a node whose certificate SPKI does
/// not match the pin.
#[tokio::test(start_paused = true)]
async fn rest_client_and_party_token_refuse_a_node_whose_spki_does_not_match_the_pin() {
    let _hold = common::hold_clock();
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let (node, _rest, addr) = rest_node(&fixture).await;
    let other = common::generate_cert();

    assert!(matches!(RestClient::connect(&addr, other.pin).await, Err(NodeError::Tls(_))));

    let identity = fixture.private_dir.join("eustress-north-plant.toml");
    private_file::write_private(&identity, plant.identity_toml().as_bytes()).expect("identity file");
    let request = TokenRequest {
        party_id: plant.id.clone(),
        node: addr.clone(),
        pin: other.pin,
        identity: identity.clone(),
        class: TokenClass::Browser,
    };
    match client::request_token(&request).await {
        Err(NodeError::Tls(_)) => {}
        other => panic!("party token must refuse a node that does not match --pin, got {other:?}"),
    }

    // Control: the node's own pin gets a browser token by default scope.
    let issued = client::request_token(&TokenRequest { pin: fixture.cert.pin, ..request })
        .await
        .expect("the right pin logs in");
    assert_eq!(issued.class, TokenClass::Browser);
    assert_eq!(issued.expires_in, BROWSER_TOKEN_LIFETIME);
    node.shutdown();
}

/// A QUIC client pinned to one certificate refuses a node that presents
/// another.
#[cfg(feature = "quic")]
#[tokio::test]
async fn quic_client_refuses_a_node_with_another_certificate() {
    use eustress_stream_node::quic::{QuicNode, QuicNodeClient};

    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = QuicNode::start(fixture.config()).await.expect("QUIC node starts");
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], node.listen_addr().port()));
    let other = common::generate_cert();

    assert!(matches!(QuicNodeClient::connect(addr, &plant.client_auth(other.pin)).await, Err(NodeError::Tls(_))));
    assert!(QuicNodeClient::connect(addr, &plant.client_auth(fixture.cert.pin)).await.is_ok(), "control: the right pin");
    node.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// The handshake signature
// ─────────────────────────────────────────────────────────────────────────────

/// A server presents the pinned certificate but signs with another key. The
/// pin matches, so only the handshake signature check can refuse it. Fails
/// when `verify_tls13_signature` returns `Ok` without checking.
#[tokio::test(start_paused = true)]
async fn tcp_client_refuses_the_pinned_certificate_signed_with_another_key() {
    let pinned = common::generate_cert();
    let other = common::generate_cert();

    for (signer, should_connect) in [(&other, false), (&pinned, true)] {
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(presenting(&pinned, signer, &[])));
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        tokio::spawn(async move {
            let _ = acceptor.accept(server_io).await;
        });
        let result = transport::tls_connect(client_io, pinned.pin, &[]).await;
        assert_eq!(
            result.is_ok(),
            should_connect,
            "signing key matches the certificate: {should_connect}; the client must connect only when it does"
        );
    }
}

/// The same over QUIC.
#[cfg(feature = "quic")]
#[tokio::test]
async fn quic_client_refuses_the_pinned_certificate_signed_with_another_key() {
    let pinned = common::generate_cert();
    let other = common::generate_cert();

    for (signer, should_connect) in [(&other, false), (&pinned, true)] {
        let tls = presenting(&pinned, signer, &[transport::QUIC_ALPN]);
        let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls).expect("QUIC server configuration");
        let server = quinn::Endpoint::server(
            quinn::ServerConfig::with_crypto(Arc::new(crypto)),
            "127.0.0.1:0".parse().unwrap(),
        )
        .expect("server endpoint");
        let addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            while let Some(incoming) = server.accept().await {
                tokio::spawn(async move {
                    let _ = incoming.await;
                });
            }
        });

        let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        endpoint.set_default_client_config(transport::quic_client_config(pinned.pin).unwrap());
        let result = endpoint.connect(addr, transport::NODE_SERVER_NAME).unwrap().await;
        assert_eq!(result.is_ok(), should_connect, "signing key matches the certificate: {should_connect}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TLS 1.3 only
// ─────────────────────────────────────────────────────────────────────────────

/// A client restricted to TLS 1.2 cannot connect over TCP. The client here
/// verifies TLS 1.2 normally, so the refusal is the node's.
#[tokio::test(start_paused = true)]
async fn tls12_client_cannot_connect_over_tcp() {
    let fixture = Fixture::new();
    fixture.write_parties(&[&TestParty::new("north-plant")]);
    let node = DuplexNode::start(&fixture);

    let cases: [(&[&'static rustls::SupportedProtocolVersion], bool); 2] =
        [(&[&rustls::version::TLS12], false), (&[&rustls::version::TLS13], true)];
    for (i, (versions, should_connect)) in cases.into_iter().enumerate() {
        let connector = tokio_rustls::TlsConnector::from(Arc::new(webpki_client(&fixture.cert, versions)));
        let result = connector.connect(transport::node_server_name().unwrap(), node.connect(peer(1 + i as u8))).await;
        assert_eq!(result.is_ok(), should_connect, "versions {versions:?}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// QUIC early data
// ─────────────────────────────────────────────────────────────────────────────

/// Connect twice with a client that offers 0-RTT; returns whether the server
/// accepted early data on the second connection. `first_round_trip` runs one
/// exchange on the first connection so its session ticket has arrived.
#[cfg(feature = "quic")]
async fn second_connection_gets_zero_rtt<F, Fut>(
    addr: std::net::SocketAddr,
    pin: transport::SpkiPin,
    first_round_trip: F,
) -> bool
where
    F: FnOnce(quinn::Connection) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut tls = transport::pinned_client_config(pin, &[transport::QUIC_ALPN]).unwrap();
    // Test only: this client asks for 0-RTT so the test can see the node refuse it.
    tls.enable_early_data = true;
    let crypto = Arc::new(quinn::crypto::rustls::QuicClientConfig::try_from(tls).unwrap());
    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(quinn::ClientConfig::new(crypto));

    let first = endpoint.connect(addr, transport::NODE_SERVER_NAME).unwrap().await.expect("first handshake");
    first_round_trip(first.clone()).await;
    first.close(0u32.into(), b"done");

    // VERIFY: quinn 0.11 Connecting::into_0rtt() -> Result<(Connection,
    // ZeroRttAccepted), Connecting>; ZeroRttAccepted resolves to whether the
    // server accepted the early data (quinn-0.11.11/src/connection.rs).
    let connecting = endpoint.connect(addr, transport::NODE_SERVER_NAME).unwrap();
    match connecting.into_0rtt() {
        Ok((connection, accepted)) => {
            let accepted = accepted.await;
            connection.close(0u32.into(), b"done");
            accepted
        }
        Err(connecting) => {
            let _ = connecting.await;
            false
        }
    }
}

/// The QUIC server accepts no early data. The control server, the same
/// certificate with early data allowed, shows the client really can do 0-RTT,
/// so the node's refusal is its own.
#[cfg(feature = "quic")]
#[tokio::test]
async fn quic_server_accepts_no_early_data() {
    use eustress_stream_node::auth;
    use eustress_stream_node::protocol::{read_server_frame, write_client_frame, ClientFrame, ServerFrame};
    use eustress_stream_node::quic::QuicNode;

    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);

    // Control: early data allowed.
    let control_addr = {
        let provider = transport::crypto_provider();
        let key = PrivateKeyDer::from_pem_slice(fixture.cert.key_pem.as_bytes()).unwrap();
        let mut tls = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(fixture.cert.cert_der.clone())], key)
            .unwrap();
        tls.alpn_protocols = vec![transport::QUIC_ALPN.to_vec()];
        tls.max_early_data_size = u32::MAX;
        let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap();
        let server = quinn::Endpoint::server(quinn::ServerConfig::with_crypto(Arc::new(crypto)), "127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            while let Some(incoming) = server.accept().await {
                tokio::spawn(async move {
                    let Ok(connection) = incoming.await else { return };
                    if let Ok((mut send, _recv)) = connection.open_bi().await {
                        let _ = send.write_all(b"x").await;
                    }
                    connection.closed().await;
                });
            }
        });
        addr
    };
    let control = second_connection_gets_zero_rtt(control_addr, fixture.cert.pin, |connection| async move {
        let (_send, mut recv) = connection.accept_bi().await.expect("control stream");
        let mut byte = [0u8; 1];
        recv.read_exact(&mut byte).await.expect("control byte");
    })
    .await;
    assert!(control, "control: a server that allows early data accepts this client's 0-RTT");

    // The node: a full login on the first connection, then 0-RTT is offered.
    let node = QuicNode::start(fixture.config()).await.expect("QUIC node starts");
    let node_addr = std::net::SocketAddr::from(([127, 0, 0, 1], node.listen_addr().port()));
    let party = plant.clone();
    let accepted = second_connection_gets_zero_rtt(node_addr, fixture.cert.pin, |connection| async move {
        let exporter = transport::quic_exporter(&connection).unwrap();
        let (mut send, mut recv) = connection.accept_bi().await.expect("login stream");
        let ServerFrame::Challenge { node_id, nonce } = read_server_frame(&mut recv, 64 * 1024).await.unwrap() else {
            panic!("expected Challenge");
        };
        let message = auth::quic_transcript(&node_id, &nonce, &exporter, &party.id, 1).unwrap();
        let hello = ClientFrame::Hello { protocol_version: 1, party_id: party.id.clone(), signature: common::sign(&party.keypair, &message) };
        write_client_frame(&mut send, &hello).await.unwrap();
        assert!(matches!(read_server_frame(&mut recv, 64 * 1024).await.unwrap(), ServerFrame::Welcome { .. }));
    })
    .await;
    assert!(!accepted, "the node accepted 0-RTT early data");
    node.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// The retry rule on QUIC
// ─────────────────────────────────────────────────────────────────────────────

/// A QUIC connection the node refuses at accept is retried, at most three
/// attempts in all, and then fails with an error that names the refusal.
/// Sixteen connections that never answer `Challenge` hold every pre-login
/// slot for 127.0.0.1, so the node refuses the client's attempts with
/// CONNECTION_REFUSED. Once they close, the same client logs in.
///
/// quinn's own timers need the real clock, so this test waits out the retry
/// rule's two one-second pauses for real. It asserts no durations.
#[cfg(feature = "quic")]
#[tokio::test]
async fn quic_connection_refused_at_accept_is_retried_three_times_then_fails() {
    use eustress_stream_node::auth::MAX_PRE_LOGIN_PER_IP;
    use eustress_stream_node::quic::{QuicNode, QuicNodeClient};

    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant");
    fixture.write_parties(&[&plant]);
    let node = QuicNode::start(fixture.config()).await.expect("QUIC node starts");
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], node.listen_addr().port()));

    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(transport::quic_client_config(fixture.cert.pin).unwrap());
    let mut stalled = Vec::new();
    for i in 0..MAX_PRE_LOGIN_PER_IP {
        let connection = endpoint.connect(addr, transport::NODE_SERVER_NAME).unwrap().await;
        stalled.push(connection.unwrap_or_else(|e| panic!("stalled connection {i} must be admitted: {e}")));
    }

    let refused = QuicNodeClient::connect_with_policy(addr, &plant.client_auth(fixture.cert.pin), &common::fixed_policy(0)).await;
    match refused {
        Err(error @ NodeError::RetriesExhausted { attempts: 3, .. }) => {
            assert!(error.to_string().contains("refused the QUIC connection"), "the error names the refusal: {error}");
        }
        Err(other) => panic!("expected the refusal after three attempts, got {other}"),
        Ok(_) => panic!("every pre-login slot was held, yet the client logged in"),
    }

    // Control: once the stalled connections close, the slots come free.
    for connection in &stalled {
        connection.close(0u32.into(), b"done");
    }
    let client = QuicNodeClient::connect(addr, &plant.client_auth(fixture.cert.pin)).await;
    assert!(client.is_ok(), "the freed slots admit the client again");
    node.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// Ratchet
// ─────────────────────────────────────────────────────────────────────────────

fn rust_files(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    for entry in std::fs::read_dir(dir).expect("read src") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).expect("read source");
            out.push((path, text));
        }
    }
}

/// The code of `text` with `//` comments removed, line by line.
fn code_only(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| match line.find("//") {
            Some(at) => line[..at].to_string(),
            None => line.to_string(),
        })
        .collect()
}

/// The crate's source has exactly one `dangerous()` call, the sanctioned one
/// in `transport.rs` that installs `PinnedServerVerifier`; exactly one
/// verifier type; no accept-any verifier; no
/// `HandshakeSignatureValid::assertion`; a `verify_tls13_signature` that
/// delegates to rustls with the provider's signature algorithms and never
/// returns a bare `Ok`; and a `verify_tls12_signature` that returns an error.
/// It also never generates a certificate, never names TLS 1.2, and never
/// allows early data.
#[test]
fn source_has_one_sanctioned_dangerous_call_and_no_accept_any_verifier() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 10, "the ratchet must see the crate's source");

    let mut dangerous_calls = Vec::new();
    let mut server_verifier_impls = Vec::new();
    let mut verified_assertions = 0;
    for (path, text) in &files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let code = code_only(text);
        for (i, line) in code.iter().enumerate() {
            let at = format!("{name}:{}", i + 1);
            let lower = line.to_ascii_lowercase();
            if line.contains("dangerous") {
                assert!(line.trim() == ".dangerous()", "{at}: only the sanctioned `.dangerous()` call may appear: {line}");
                let next = code.get(i + 1).map(|l| l.trim()).unwrap_or("");
                assert_eq!(next, ".with_custom_certificate_verifier(verifier)", "{at}: `.dangerous()` may only install the pinning verifier");
                dangerous_calls.push(at.clone());
            }
            if line.contains("ServerCertVerifier for ") {
                server_verifier_impls.push(format!("{at}: {}", line.trim()));
            }
            for forbidden in [
                "ClientCertVerifier for ",
                "ResolvesServerCert for ",
                "HandshakeSignatureValid::assertion",
                "TLS12",
                "rcgen",
                "generate_simple_self_signed",
                "enable_early_data = true",
                "max_early_data_size = u32::MAX",
            ] {
                assert!(!line.contains(forbidden), "{at}: `{forbidden}` must not appear in the crate's source: {line}");
            }
            for forbidden in ["noverifier", "nocertificateverification", "acceptany", "acceptall", "insecure", "skipverif", "noserververification"] {
                assert!(!lower.contains(forbidden), "{at}: an accept-any verifier name: {line}");
            }
            if line.contains("ServerCertVerified::assertion()") {
                verified_assertions += 1;
            }
        }
    }
    assert_eq!(dangerous_calls.len(), 1, "exactly one `.dangerous()` call: {dangerous_calls:?}");
    assert!(dangerous_calls[0].starts_with("transport.rs:"), "{dangerous_calls:?}");
    assert_eq!(server_verifier_impls.len(), 1, "{server_verifier_impls:?}");
    assert!(server_verifier_impls[0].ends_with("impl ServerCertVerifier for PinnedServerVerifier {"), "{server_verifier_impls:?}");
    assert_eq!(verified_assertions, 1, "ServerCertVerified only after the pin matches");

    let transport_rs = &files.iter().find(|(p, _)| p.ends_with("transport.rs")).expect("transport.rs").1;
    let transport_code = code_only(transport_rs).join("\n");
    assert!(transport_code.contains("let verifier = Arc::new(PinnedServerVerifier::new(pin));"), "the installed verifier is the pinning verifier");
    assert!(
        transport_code.contains("PinnedServerVerifier { pin, algorithms: crypto_provider().signature_verification_algorithms }"),
        "the verifier checks signatures with the crypto provider's algorithms"
    );

    // verify_tls13_signature: one statement, the delegation. No bare Ok.
    let tls13 = transport_code
        .split("fn verify_tls13_signature(")
        .nth(1)
        .and_then(|rest| rest.split("fn supported_verify_schemes").next())
        .expect("verify_tls13_signature body");
    assert!(
        tls13.contains("rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)"),
        "verify_tls13_signature must delegate to the provider's signature check"
    );
    assert!(!tls13.contains("Ok("), "verify_tls13_signature must never return a bare Ok");

    // verify_tls12_signature: an error, always.
    let tls12 = transport_code
        .split("fn verify_tls12_signature(")
        .nth(1)
        .and_then(|rest| rest.split("fn verify_tls13_signature(").next())
        .expect("verify_tls12_signature body");
    assert!(tls12.contains("Err(rustls::Error::"), "verify_tls12_signature must return an error");
    assert!(!tls12.contains("Ok("), "verify_tls12_signature must never return Ok");

    assert!(transport_code.contains("config.max_early_data_size = 0;"), "no early data on any listener");
}

//! Shared fixtures for the authentication tests and the network benchmark:
//! a generated certificate, owner-only key and parties files, enrolled party
//! keypairs, node configurations, nodes served over in-memory pipes, a fixed
//! jitter source, and frame-level protocol helpers.
//!
//! Test code generates certificates. Production code never does.

#![allow(dead_code)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use eustress_identity::keypair::UserKeypair;
use eustress_identity::private_file;
use eustress_stream::EustressStream;
use eustress_stream_node::auth::{self, PartiesFile, PartyEntry};
use eustress_stream_node::handler::{handle_connection, ConnectionContext};
use eustress_stream_node::protocol::{read_raw_frame, read_server_frame, write_client_frame, ClientFrame, ServerFrame};
use eustress_stream_node::transport::{self, NodeTls, SpkiPin};
use eustress_stream_node::{
    ClientAuth, NodeAuth, NodeConfig, NodeError, RandomSource, RestClient, RestServer, RestState, RetryPolicy,
};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream};

pub const MAX_FRAME: usize = 16 * 1024 * 1024;

// ─────────────────────────────────────────────────────────────────────────────
// Certificates
// ─────────────────────────────────────────────────────────────────────────────

/// A generated self-signed certificate and its key, as PEM and DER.
#[derive(Clone)]
pub struct TestCert {
    pub cert_pem: String,
    pub key_pem: String,
    pub cert_der: Vec<u8>,
    pub pin: SpkiPin,
}

impl TestCert {
    /// The node TLS material built from this certificate, for test servers.
    pub fn node_tls(&self) -> NodeTls {
        NodeTls::from_pem(self.cert_pem.as_bytes(), self.key_pem.as_bytes()).expect("test certificate loads")
    }
}

/// A fresh ECDSA P-256 certificate whose names include the node server name,
/// so a WebPKI client can also verify it (used only by the TLS 1.2 test).
pub fn generate_cert() -> TestCert {
    let certified = rcgen::generate_simple_self_signed(vec![
        transport::NODE_SERVER_NAME.to_string(),
        "localhost".to_string(),
    ])
    .expect("generate a test certificate");
    let pin = SpkiPin::of_certificate(certified.cert.der()).expect("pin of the test certificate");
    TestCert {
        cert_pem: certified.cert.pem(),
        key_pem: certified.key_pair.serialize_pem(),
        cert_der: certified.cert.der().to_vec(),
        pin,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Parties
// ─────────────────────────────────────────────────────────────────────────────

/// A party with its own keypair and grants.
#[derive(Clone)]
pub struct TestParty {
    pub id: String,
    pub keypair: Arc<UserKeypair>,
    pub enabled: bool,
    pub admin: bool,
    pub max_subscriptions: usize,
    pub subscribe: Vec<String>,
    pub publish: Vec<String>,
}

impl TestParty {
    pub fn new(id: &str) -> Self {
        TestParty {
            id: id.to_string(),
            keypair: Arc::new(UserKeypair::generate()),
            enabled: true,
            admin: false,
            max_subscriptions: auth::DEFAULT_MAX_SUBSCRIPTIONS,
            subscribe: Vec::new(),
            publish: Vec::new(),
        }
    }

    pub fn subscribe(mut self, topics: &[&str]) -> Self {
        self.subscribe.extend(topics.iter().map(|t| t.to_string()));
        self
    }

    pub fn publish(mut self, topics: &[&str]) -> Self {
        self.publish.extend(topics.iter().map(|t| t.to_string()));
        self
    }

    pub fn admin(mut self) -> Self {
        self.admin = true;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn max_subscriptions(mut self, n: usize) -> Self {
        self.max_subscriptions = n;
        self
    }

    pub fn entry(&self) -> PartyEntry {
        PartyEntry {
            id: self.id.clone(),
            public_key: self.keypair.public_key_b64(),
            enabled: self.enabled,
            admin: self.admin,
            max_subscriptions: self.max_subscriptions,
            subscribe: self.subscribe.clone(),
            publish: self.publish.clone(),
        }
    }

    /// Client credentials for this party against a node with `pin`.
    pub fn client_auth(&self, pin: SpkiPin) -> ClientAuth {
        ClientAuth::new(self.id.clone(), Arc::clone(&self.keypair), pin)
    }

    /// An identity file for this party, in the engine's format (hex keys).
    pub fn identity_toml(&self) -> String {
        let public = STANDARD.decode(self.keypair.public_key_b64()).expect("public key");
        let private = STANDARD.decode(self.keypair.private_key_b64()).expect("private key");
        format!(
            "[identity]\nversion = \"1.0\"\nusername = \"{}\"\npublic_key = \"{}\"\nprivate_key = \"{}\"\n",
            self.id,
            hex(&public),
            hex(&private)
        )
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The raw Ed25519 signature of `keypair` over `message`.
pub fn sign(keypair: &UserKeypair, message: &[u8]) -> Vec<u8> {
    STANDARD.decode(keypair.sign_bytes(message)).expect("signature decodes")
}

// ─────────────────────────────────────────────────────────────────────────────
// Files
// ─────────────────────────────────────────────────────────────────────────────

/// A temporary node folder: the certificate, and an owner-only folder holding
/// the private key and `parties.toml`, both written owner-only.
pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub private_dir: PathBuf,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub parties_path: PathBuf,
    pub cert: TestCert,
}

impl Fixture {
    pub fn new() -> Fixture {
        let dir = tempfile::tempdir().expect("temp dir");
        let private_dir = dir.path().join("private");
        private_file::private_dir(&private_dir).expect("owner-only folder");
        let cert = generate_cert();
        let cert_path = dir.path().join("node.crt");
        std::fs::write(&cert_path, &cert.cert_pem).expect("write the certificate");
        let key_path = private_dir.join("node.key");
        private_file::write_private(&key_path, cert.key_pem.as_bytes()).expect("write the key owner-only");
        let parties_path = private_dir.join("parties.toml");
        private_file::write_private(&parties_path, b"").expect("write parties.toml owner-only");
        Fixture { dir, private_dir, cert_path, key_path, parties_path, cert }
    }

    /// Replace `parties.toml`, owner-only, through a temporary file and a rename.
    pub fn write_parties(&self, parties: &[&TestParty]) {
        let file = PartiesFile { parties: parties.iter().map(|p| p.entry()).collect() };
        let text = file.to_toml().expect("parties serialize");
        private_file::write_private(&self.parties_path, text.as_bytes()).expect("write parties.toml");
    }

    /// A loopback node on ephemeral ports with this fixture's files.
    pub fn config(&self) -> NodeConfig {
        NodeConfig::default()
            .with_tls(self.cert_path.clone(), self.key_path.clone())
            .with_parties(self.parties_path.clone())
            .with_node_id("test-node")
            .with_port(0)
            .with_rest_port(0)
    }
}

/// Give another account read access to `path`: BUILTIN\Users on Windows,
/// group and other on Unix. Asserts that the owner-only check now fails.
pub fn grant_foreign_access(path: &Path) {
    #[cfg(windows)]
    {
        let status = std::process::Command::new("icacls")
            .arg(path)
            .arg("/grant")
            .arg("*S-1-5-32-545:(R)")
            .stdout(std::process::Stdio::null())
            .status()
            .expect("run icacls");
        assert!(status.success(), "icacls could not add an access entry to {}", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = if path.is_dir() { 0o755 } else { 0o644 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }
    assert!(
        private_file::verify(path).is_err(),
        "the extra access entry must make the owner-only check fail for {}",
        path.display()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Nodes served over in-memory pipes
// ─────────────────────────────────────────────────────────────────────────────

/// A source address for an injected connection: 10.0.0.<last>.
pub fn peer(last: u8) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, last)), 40_000 + u16::from(last))
}

/// A node's real connection task, fed over in-memory pipes instead of
/// sockets, so a test can choose the source address and run on a paused
/// clock without real I/O.
pub struct DuplexNode {
    pub auth: Arc<NodeAuth>,
    pub ctx: Arc<ConnectionContext>,
    pub stream: EustressStream,
}

impl DuplexNode {
    /// Run the real startup checks on the fixture's files.
    pub fn start(fixture: &Fixture) -> DuplexNode {
        let config = fixture.config();
        let auth = NodeAuth::from_config(&config).expect("startup checks pass");
        let stream = EustressStream::new(config.stream_config.clone());
        let ctx = ConnectionContext::new(
            stream.clone(),
            Arc::clone(&auth),
            config.frame_max_bytes,
            config.connection_channel_capacity,
        );
        DuplexNode { auth, ctx, stream }
    }

    /// Accept a connection from `from` now, as the TCP accept loop does: take
    /// a pre-login slot or refuse. Returns the client's end of the pipe.
    pub fn try_connect(&self, from: SocketAddr) -> Option<DuplexStream> {
        self.try_connect_with_task(from).map(|(client, _task)| client)
    }

    /// The same, with the node's connection task, so a test can see when the
    /// node ends the connection.
    pub fn try_connect_with_task(&self, from: SocketAddr) -> Option<(DuplexStream, tokio::task::JoinHandle<()>)> {
        let permit = self.auth.pre_login.try_acquire(from.ip())?;
        let (client, server) = tokio::io::duplex(256 * 1024);
        let task =
            tokio::spawn(handle_connection(server, from, permit, Arc::clone(&self.ctx), tokio::time::Instant::now()));
        Some((client, task))
    }

    pub fn connect_with_task(&self, from: SocketAddr) -> (DuplexStream, tokio::task::JoinHandle<()>) {
        self.try_connect_with_task(from).expect("the connection is admitted")
    }

    pub fn connect(&self, from: SocketAddr) -> DuplexStream {
        self.try_connect(from).expect("the connection is admitted")
    }

    /// Dial the node from `from` the way a client sees it: an admitted
    /// connection, or, when the node refuses it at accept, a pipe whose far
    /// end is already closed. A client cannot tell a refusal at accept from a
    /// dropped connection.
    pub fn dial(&self, from: SocketAddr) -> DuplexStream {
        match self.try_connect(from) {
            Some(client) => client,
            None => {
                let (client, _closed_by_the_node) = tokio::io::duplex(64);
                client
            }
        }
    }
}

/// The party `id` as the node holds it now: what a login admits, and what
/// a token is issued for.
pub fn enrolled(auth: &NodeAuth, id: &str) -> Arc<auth::Party> {
    auth.party_set().active(id).cloned().expect("an enabled, enrolled party")
}

// ─────────────────────────────────────────────────────────────────────────────
// REST served over in-memory pipes
// ─────────────────────────────────────────────────────────────────────────────

/// The real REST server fed over in-memory pipes, so a test chooses the
/// source address and runs on a paused clock that advances by itself.
pub struct DuplexRest {
    pub auth: Arc<NodeAuth>,
    pub stream: EustressStream,
    pub rest: RestServer,
    pub pin: SpkiPin,
}

impl DuplexRest {
    /// Run the real startup checks on the fixture's files.
    pub fn start(fixture: &Fixture) -> DuplexRest {
        let config = fixture.config();
        let auth = NodeAuth::from_config(&config).expect("startup checks pass");
        let stream = EustressStream::new(config.stream_config.clone());
        Self::with_auth(auth, stream, fixture.cert.pin)
    }

    /// REST for a node whose authentication state already exists, such as a
    /// [`DuplexNode`]'s, so REST and TCP share parties, sessions and tokens.
    pub fn with_auth(auth: Arc<NodeAuth>, stream: EustressStream, pin: SpkiPin) -> DuplexRest {
        let rest = RestServer::unbound(RestState::new(stream.clone(), Arc::clone(&auth), 0, 0));
        DuplexRest { auth, stream, rest, pin }
    }

    /// A connection from `from`: the client's end of the pipe.
    pub fn dial(&self, from: SocketAddr) -> DuplexStream {
        let (client, server) = tokio::io::duplex(256 * 1024);
        assert!(self.rest.serve_io(server, from), "the REST handshake limit admits the connection");
        client
    }

    /// A pinned REST client connected from `from`.
    pub async fn client(&self, from: SocketAddr) -> RestClient {
        RestClient::handshake(self.dial(from), self.pin).await.expect("TLS to the REST server")
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Retry
// ─────────────────────────────────────────────────────────────────────────────

/// A jitter source that always gives the same number.
pub struct FixedRandom(pub u64);

impl RandomSource for FixedRandom {
    fn next_u64(&self) -> u64 {
        self.0
    }
}

/// The retry rule with a fixed jitter of `jitter_ms` milliseconds.
pub fn fixed_policy(jitter_ms: u64) -> RetryPolicy {
    RetryPolicy::with_random(Arc::new(FixedRandom(jitter_ms)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Frame-level TCP client
// ─────────────────────────────────────────────────────────────────────────────

/// TLS to a node and its `Challenge`, with no login yet: the test decides
/// what to send.
pub struct RawSession<IO> {
    pub tls: tokio_rustls::client::TlsStream<IO>,
    pub node_id: String,
    pub nonce: [u8; 32],
    pub exporter: [u8; 32],
}

impl<IO: AsyncRead + AsyncWrite + Unpin> RawSession<IO> {
    pub async fn open(io: IO, pin: SpkiPin) -> Result<Self, NodeError> {
        let mut tls = transport::tls_connect(io, pin, &[]).await?;
        let exporter = {
            let (_, connection) = tls.get_ref();
            transport::tcp_exporter(&**connection)?
        };
        match read_server_frame(&mut tls, auth::PRE_LOGIN_MAX_FRAME).await? {
            ServerFrame::Challenge { node_id, nonce } => Ok(RawSession { tls, node_id, nonce, exporter }),
            other => Err(NodeError::Protocol(format!("expected Challenge, got {other:?}"))),
        }
    }

    /// A `Hello` signed over this connection's TCP transcript. A malformed
    /// party id cannot form a transcript; it gets a zero signature, which the
    /// node never reaches.
    pub fn hello(&self, party_id: &str, keypair: &UserKeypair, protocol_version: u16) -> ClientFrame {
        let signature = match auth::tcp_transcript(&self.node_id, &self.nonce, &self.exporter, party_id, protocol_version) {
            Ok(message) => sign(keypair, &message),
            Err(_) => vec![0u8; 64],
        };
        ClientFrame::Hello { protocol_version, party_id: party_id.to_string(), signature }
    }

    pub async fn send(&mut self, frame: &ClientFrame) -> Result<(), NodeError> {
        write_client_frame(&mut self.tls, frame).await
    }

    pub async fn recv(&mut self) -> Result<ServerFrame, NodeError> {
        read_server_frame(&mut self.tls, MAX_FRAME).await
    }

    pub async fn recv_raw(&mut self) -> Result<Vec<u8>, NodeError> {
        read_raw_frame(&mut self.tls, MAX_FRAME).await.map(|b| b.to_vec())
    }

    /// Log in as `party`; returns the node's reply.
    pub async fn login(&mut self, party: &TestParty) -> Result<ServerFrame, NodeError> {
        let hello = self.hello(&party.id, &party.keypair, auth::PROTOCOL_VERSION);
        self.send(&hello).await?;
        self.recv().await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Clock
// ─────────────────────────────────────────────────────────────────────────────

/// While held, a paused current-thread clock does not auto-advance: a pending
/// blocking task inhibits it. Real socket I/O can then be awaited without the
/// runtime jumping to the next timer, and time moves only through
/// `tokio::time::advance`. Dropping the hold releases it.
pub struct ClockHold {
    release: Option<std::sync::mpsc::Sender<()>>,
}

pub fn hold_clock() -> ClockHold {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    // VERIFY: tokio (test-util, current_thread) inhibits auto-advance while a
    // spawn_blocking task is pending; see tokio-1.52.3/src/runtime/blocking/schedule.rs.
    tokio::task::spawn_blocking(move || {
        let _ = rx.recv();
    });
    ClockHold { release: Some(tx) }
}

impl Drop for ClockHold {
    fn drop(&mut self) {
        self.release.take();
    }
}

/// A port nothing listens on right now.
pub fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind an ephemeral port");
    listener.local_addr().expect("local addr").port()
}

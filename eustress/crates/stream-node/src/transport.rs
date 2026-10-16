//! TLS 1.3 for every listener and every client, with the node's certificate
//! pinned on the client side.
//!
//! - Servers (TCP, REST, QUIC) present one operator-supplied certificate and
//!   admit TLS 1.3 and nothing older. No listener accepts early data.
//! - Every client, loopback included, pins the SHA-256 of the node
//!   certificate's SubjectPublicKeyInfo (SPKI). The pin survives a renewal
//!   that keeps the key. Name checks are replaced by the pin, so a node is
//!   addressed by its pin, not by a name.
//! - [`PinnedServerVerifier`] compares the pin and then verifies the handshake
//!   signature for real, through the crypto provider. A pin check alone would
//!   accept anyone who replays the public certificate. There is no option to
//!   skip either check, and no other certificate verifier exists in this crate.
//! - Installing the verifier takes the crate's one `dangerous()` call, which
//!   Security approved as the one sanctioned call (2026-10-09). The ratchet
//!   test in `tests/auth_transport.rs` holds the crate to it.
//!
//! Production code never generates a certificate. Tests may.

use std::fmt;
use std::sync::{Arc, OnceLock};
#[cfg(feature = "quic")]
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::warn;

#[cfg(feature = "quic")]
use crate::auth::QUIC_LABEL;
use crate::auth::{BINDING_LEN, TCP_LABEL};
use crate::error::NodeError;

/// The TLS server name clients send. The pin, not the name, identifies the
/// node, so every client uses this one name.
pub const NODE_SERVER_NAME: &str = "eustress-node";
/// ALPN for the QUIC transport.
pub const QUIC_ALPN: &[u8] = b"eustress/1";
/// ALPN for the REST listener.
pub const REST_ALPN: &[u8] = b"http/1.1";

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// The crypto provider every configuration in this crate uses (ring). It is
/// passed explicitly, so nothing depends on a process-wide default provider.
pub fn crypto_provider() -> Arc<CryptoProvider> {
    static PROVIDER: OnceLock<Arc<CryptoProvider>> = OnceLock::new();
    Arc::clone(PROVIDER.get_or_init(|| Arc::new(rustls::crypto::ring::default_provider())))
}

// ─────────────────────────────────────────────────────────────────────────────
// The pin
// ─────────────────────────────────────────────────────────────────────────────

/// SHA-256 of a certificate's SubjectPublicKeyInfo.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpkiPin(pub [u8; 32]);

impl SpkiPin {
    /// The pin of a DER certificate.
    pub fn of_certificate(cert: &CertificateDer<'_>) -> Result<Self, NodeError> {
        spki_sha256(cert)
            .map(SpkiPin)
            .map_err(|e| NodeError::Tls(format!("cannot read the certificate's public key: {e}")))
    }

    /// Parse 64 hex digits. Colons between byte pairs are accepted.
    pub fn from_hex(text: &str) -> Result<Self, NodeError> {
        let digits: String = text.trim().chars().filter(|c| *c != ':').collect();
        decode_hex32(&digits).map(SpkiPin).ok_or_else(|| {
            NodeError::Tls(
                "a pin is 64 hex digits: the SHA-256 of the certificate's SubjectPublicKeyInfo".to_string(),
            )
        })
    }

    /// 64 lower-case hex digits.
    pub fn to_hex(&self) -> String {
        hex_lower(&self.0)
    }
}

impl fmt::Display for SpkiPin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for SpkiPin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SpkiPin({})", self.to_hex())
    }
}

fn spki_sha256(cert: &CertificateDer<'_>) -> Result<[u8; 32], rustls::Error> {
    let parsed = rustls::server::ParsedCertificate::try_from(cert)?;
    Ok(sha256(parsed.subject_public_key_info().as_ref()))
}

/// 32 bytes from exactly 64 hex digits.
pub(crate) fn decode_hex32(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in bytes.chunks_exact(2).enumerate() {
        let hi = char::from(pair[0]).to_digit(16)?;
        let lo = char::from(pair[1]).to_digit(16)?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Some(out)
}

/// Lower-case hex of `bytes`.
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// The pinning verifier
// ─────────────────────────────────────────────────────────────────────────────

/// Accepts exactly the node whose leaf certificate has the pinned SPKI hash,
/// and only when the TLS 1.3 handshake signature verifies against that
/// certificate's key.
///
/// Certificate validity dates are not checked, on purpose. With an SPKI pin,
/// the pinned key is the trust anchor: whoever holds that key can mint a
/// certificate with any dates, so a date check adds no security against a key
/// compromise, only an outage on the day a renewal is forgotten. A
/// compromised key is handled by the planned pin change (a renewal with a new
/// key), not by a date check. Do not add one.
#[derive(Debug)]
pub struct PinnedServerVerifier {
    pin: SpkiPin,
    algorithms: WebPkiSupportedAlgorithms,
}

impl PinnedServerVerifier {
    /// A verifier for `pin`, checking signatures with the crate's provider.
    pub fn new(pin: SpkiPin) -> Self {
        PinnedServerVerifier { pin, algorithms: crypto_provider().signature_verification_algorithms }
    }
}

// VERIFY: the four ServerCertVerifier method signatures below were copied from
// rustls-0.23.41/src/verify.rs (trait ServerCertVerifier). Confirm they still
// match the rustls version in Cargo.lock.
impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let seen = spki_sha256(end_entity)
            .map_err(|_| rustls::Error::InvalidCertificate(CertificateError::BadEncoding))?;
        // The pin comparison. The node presents one certificate, its leaf;
        // names, chains and dates are not consulted, because the pin is the
        // trust decision.
        if seen == self.pin.0 {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        // TLS 1.2 is never accepted. The client configuration admits TLS 1.3
        // only; this refusal is the second line.
        Err(rustls::Error::General("TLS 1.2 is never accepted by a stream node client".to_string()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        // The handshake signature, verified for real with the pinned
        // certificate's key through the provider's algorithms. Never a bare Ok.
        // VERIFY: rustls::crypto::verify_tls13_signature(msg: &[u8], cert:
        // &CertificateDer<'_>, dss: &DigitallySignedStruct, supported_schemes:
        // &WebPkiSupportedAlgorithms) -> Result<HandshakeSignatureValid, Error>,
        // copied from rustls-0.23.41/src/webpki/verify.rs.
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Client configuration
// ─────────────────────────────────────────────────────────────────────────────

/// A TLS 1.3 client configuration pinned to `pin`, with no early data.
pub fn pinned_client_config(pin: SpkiPin, alpn: &[&[u8]]) -> Result<rustls::ClientConfig, NodeError> {
    let verifier = Arc::new(PinnedServerVerifier::new(pin));
    let mut config = rustls::ClientConfig::builder_with_provider(crypto_provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| NodeError::Tls(format!("TLS 1.3 client configuration: {e}")))?
        // The one `dangerous()` call in this crate. rustls offers no other way
        // to install a certificate verifier; the verifier installed here is the
        // pinning verifier above, which checks the pin and the handshake
        // signature. The ratchet test in tests/auth_transport.rs holds this to
        // exactly one call, at this site.
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    config.enable_early_data = false;
    Ok(config)
}

/// The server name every client sends.
pub fn node_server_name() -> Result<ServerName<'static>, NodeError> {
    ServerName::try_from(NODE_SERVER_NAME).map_err(|_| NodeError::Tls("invalid node server name".to_string()))
}

/// Open TLS 1.3 to a node over `io`, pinned to `pin`.
///
/// A node that closes the connection during the handshake, which is how a
/// refusal at accept by a pre-login limit looks from here, gives
/// [`NodeError::Refused`], which the native clients retry under their retry
/// rule. Any TLS failure, a pin mismatch or a bad handshake signature among
/// them, gives [`NodeError::Tls`] and is never retried.
pub async fn tls_connect<IO>(
    io: IO,
    pin: SpkiPin,
    alpn: &[&[u8]],
) -> Result<tokio_rustls::client::TlsStream<IO>, NodeError>
where
    IO: AsyncRead + AsyncWrite + Unpin,
{
    let config = pinned_client_config(pin, alpn)?;
    tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(node_server_name()?, io)
        .await
        .map_err(|e| {
            // VERIFY: tokio-rustls 0.26.4 reports a peer that closes during the
            // handshake as io::ErrorKind::UnexpectedEof ("tls handshake eof",
            // src/common/mod.rs) and every rustls error as InvalidData.
            if closed_by_peer(&e) {
                NodeError::Refused {
                    reason: format!("the node closed the connection during the TLS handshake ({e})"),
                    retry_after: None,
                }
            } else {
                NodeError::Tls(format!("TLS handshake with the node failed: {e}"))
            }
        })
}

/// `true` for the I/O errors a connection closed by the other side gives.
pub(crate) fn closed_by_peer(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe
    )
}

/// 32 bytes of the TLS 1.3 exporter for a TCP login: label
/// `eustress-stream-login-v1`, empty context.
pub fn tcp_exporter<Data>(connection: &rustls::ConnectionCommon<Data>) -> Result<[u8; BINDING_LEN], NodeError> {
    let empty_context: &[u8] = &[];
    // VERIFY: ConnectionCommon::export_keying_material<T: AsMut<[u8]>>(&self,
    // output: T, label: &[u8], context: Option<&[u8]>) -> Result<T, Error>,
    // copied from rustls-0.23.41/src/conn.rs. `Some(&[])` is the empty
    // context; quinn passes `Some(context)` the same way for QUIC.
    connection
        .export_keying_material([0u8; BINDING_LEN], TCP_LABEL, Some(empty_context))
        .map_err(|e| NodeError::Tls(format!("TLS exporter: {e}")))
}

// ─────────────────────────────────────────────────────────────────────────────
// QUIC
// ─────────────────────────────────────────────────────────────────────────────

/// A QUIC client configuration pinned to `pin`: TLS 1.3, no early data.
#[cfg(feature = "quic")]
pub fn quic_client_config(pin: SpkiPin) -> Result<quinn::ClientConfig, NodeError> {
    let tls = pinned_client_config(pin, &[QUIC_ALPN])?;
    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .map_err(|e| NodeError::Tls(format!("QUIC client configuration: {e}")))?;
    let mut config = quinn::ClientConfig::new(Arc::new(crypto));
    let mut transport = quinn::TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(5)));
    // Datagrams are off: the node never reads them, so none is buffered.
    transport.datagram_receive_buffer_size(None);
    config.transport_config(Arc::new(transport));
    Ok(config)
}

/// 32 bytes of the QUIC connection's TLS 1.3 exporter: label
/// `eustress-stream-login-quic-v1`, empty context.
#[cfg(feature = "quic")]
pub fn quic_exporter(connection: &quinn::Connection) -> Result<[u8; BINDING_LEN], NodeError> {
    let mut out = [0u8; BINDING_LEN];
    // VERIFY: quinn 0.11 Connection::export_keying_material(&self, output:
    // &mut [u8], label: &[u8], context: &[u8]), copied from
    // quinn-0.11.11/src/connection.rs.
    connection
        .export_keying_material(&mut out, QUIC_LABEL, b"")
        .map_err(|_| NodeError::Tls("QUIC exporter failed".to_string()))?;
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// The node's certificate and server configurations
// ─────────────────────────────────────────────────────────────────────────────

/// The node's certificate and private key, loaded from operator-supplied PEM
/// files, with one server configuration per listener. All listeners share
/// the one certificate and key.
pub struct NodeTls {
    cert: CertificateDer<'static>,
    cert_sha256: [u8; 32],
    spki_pin: SpkiPin,
    tcp: Arc<rustls::ServerConfig>,
    rest: Arc<rustls::ServerConfig>,
    #[cfg(feature = "quic")]
    quic: Arc<rustls::ServerConfig>,
}

impl fmt::Debug for NodeTls {
    // The key is left out on purpose.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeTls").field("spki_pin", &self.spki_pin).finish_non_exhaustive()
    }
}

impl NodeTls {
    /// Build from PEM text. The first certificate is the leaf the node
    /// presents; any others are ignored with a warning. Startup reads the key
    /// through the handle its owner-only check opened
    /// (`NodeAuth::from_config`); nothing here reads a file.
    pub fn from_pem(cert_pem: &[u8], key_pem: &[u8]) -> Result<Self, NodeError> {
        let mut certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(cert_pem)
            .collect::<Result<_, _>>()
            .map_err(|e| NodeError::Tls(format!("the certificate file is not PEM: {e}")))?;
        if certs.is_empty() {
            return Err(NodeError::Tls("the certificate file holds no certificate".to_string()));
        }
        if certs.len() > 1 {
            warn!(
                "the certificate file holds {} certificates; the node presents only the first, the leaf",
                certs.len()
            );
        }
        let cert = certs.remove(0);
        let key = PrivateKeyDer::from_pem_slice(key_pem)
            .map_err(|e| NodeError::Tls(format!("the private key file is not a PEM private key: {e}")))?;

        let provider = crypto_provider();
        let tcp = server_config(&provider, &cert, &key, &[])?;
        let rest = server_config(&provider, &cert, &key, &[REST_ALPN])?;
        #[cfg(feature = "quic")]
        let quic = server_config(&provider, &cert, &key, &[QUIC_ALPN])?;

        let cert_sha256 = sha256(cert.as_ref());
        let spki_pin = SpkiPin::of_certificate(&cert)?;
        Ok(NodeTls {
            cert,
            cert_sha256,
            spki_pin,
            tcp: Arc::new(tcp),
            rest: Arc::new(rest),
            #[cfg(feature = "quic")]
            quic: Arc::new(quic),
        })
    }

    /// The DER certificate the node presents.
    pub fn certificate(&self) -> &CertificateDer<'static> {
        &self.cert
    }

    /// SHA-256 of the DER certificate: the REST channel binding.
    pub fn cert_sha256(&self) -> [u8; 32] {
        self.cert_sha256
    }

    /// The pin clients hold: SHA-256 of the certificate's SPKI.
    pub fn spki_pin(&self) -> SpkiPin {
        self.spki_pin
    }

    /// The acceptor for the TCP listener.
    pub fn tcp_acceptor(&self) -> tokio_rustls::TlsAcceptor {
        tokio_rustls::TlsAcceptor::from(Arc::clone(&self.tcp))
    }

    /// The acceptor for the REST listener (ALPN `http/1.1`).
    pub fn rest_acceptor(&self) -> tokio_rustls::TlsAcceptor {
        tokio_rustls::TlsAcceptor::from(Arc::clone(&self.rest))
    }

    /// The QUIC server configuration: TLS 1.3, no early data, no streams
    /// opened by the client (the node opens the one stream a session uses),
    /// and no datagrams.
    #[cfg(feature = "quic")]
    pub fn quic_server_config(&self) -> Result<quinn::ServerConfig, NodeError> {
        let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(Arc::clone(&self.quic))
            .map_err(|e| NodeError::Tls(format!("QUIC server configuration: {e}")))?;
        let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
        let idle: quinn::IdleTimeout = Duration::from_secs(30)
            .try_into()
            .map_err(|_| NodeError::Tls("QUIC idle timeout out of range".to_string()))?;
        let mut transport = quinn::TransportConfig::default();
        transport.keep_alive_interval(Some(Duration::from_secs(5)));
        transport.max_idle_timeout(Some(idle));
        transport.max_concurrent_bidi_streams(0u32.into());
        transport.max_concurrent_uni_streams(0u32.into());
        // Datagrams are off: the node never reads them, so none is buffered.
        transport.datagram_receive_buffer_size(None);
        config.transport_config(Arc::new(transport));
        Ok(config)
    }
}

/// One listener's server configuration: TLS 1.3 only, the one certificate,
/// no client certificates, no early data.
fn server_config(
    provider: &Arc<CryptoProvider>,
    cert: &CertificateDer<'static>,
    key: &PrivateKeyDer<'static>,
    alpn: &[&[u8]],
) -> Result<rustls::ServerConfig, NodeError> {
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::clone(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| NodeError::Tls(format!("TLS 1.3 server configuration: {e}")))?
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key.clone_key())
        .map_err(|e| NodeError::Tls(format!("the certificate and private key do not form a usable pair: {e}")))?;
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    // No early data on any listener. QUIC permits only 0 or u32::MAX here,
    // and 0 refuses 0-RTT.
    config.max_early_data_size = 0;
    config.send_half_rtt_data = false;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_hex_round_trip() {
        let pin = SpkiPin([0xab; 32]);
        assert_eq!(SpkiPin::from_hex(&pin.to_hex()).unwrap(), pin);
        let colons: String = pin.to_hex().as_bytes().chunks(2).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join(":");
        assert_eq!(SpkiPin::from_hex(&colons).unwrap(), pin);
        assert!(SpkiPin::from_hex("abc").is_err());
        assert!(SpkiPin::from_hex(&"zz".repeat(32)).is_err());
    }
}

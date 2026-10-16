//! The one HTTP client for SAM.gov and Grants.gov calls.
//!
//! The shared `eustress_data` transport is the wrong client for a request that
//! carries an API key in its query string, for three reasons:
//!
//! - **It follows redirects.** ureq 2 follows five by default. A request
//!   carrying a key has no business arriving anywhere it was not sent.
//! - **It may read a proxy from the environment.** ureq 2 does so whenever any
//!   crate in the build turns on its `proxy-from-env` feature, and a proxy sees
//!   the whole query string, key included.
//! - **ureq's errors carry the full URL.** Its `Display` for a transport error,
//!   and for a status error, prints the request URL first, query included. A
//!   DNS failure or a 403 would put the key in a log line, a status bar, or a
//!   toast.
//!
//! This client fixes all three in one place, [`build_agent`], and builds every
//! error message from the error KIND and the host only, never by formatting a
//! ureq error.
//!
//! TLS certificate and hostname verification are ureq's defaults (rustls with
//! the webpki roots). Production never passes a TLS configuration, and
//! `a_certificate_no_root_vouches_for_is_refused` proves verification is on by
//! having a self-signed loopback server rejected through the same builder.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use eustress_data::source::http::{HttpRequest, HttpResponse, HttpTransport};
use eustress_data::{DataError, Result};

/// How the capture client behaves. A plain value, so a test can read it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportPolicy {
    /// Redirects to follow. Zero: a 3xx comes back as an ordinary response.
    pub redirects: u32,
    /// Whether to read `HTTPS_PROXY` and its relatives from the environment.
    pub proxy_from_env: bool,
    /// Refuse any `http://` request, and any redirect to one.
    pub https_only: bool,
    /// Connect and read timeout.
    pub timeout: Duration,
}

/// The production policy. Every field is the safe choice, stated explicitly
/// rather than inherited from a default that a feature flag can change.
pub const CAPTURE_POLICY: TransportPolicy = TransportPolicy {
    redirects: 0,
    proxy_from_env: false,
    https_only: true,
    timeout: Duration::from_secs(30),
};

/// Largest response body read, in bytes. A SAM page of 1000 records is a few
/// megabytes; anything far past that is not a search result.
const MAX_BODY: u64 = 32 * 1024 * 1024;

/// Build the client: the ONE place the capture agent is configured, so the
/// production client and every test go through the same builder.
///
/// `tls` is `None` in production, which keeps ureq's verifying default. Only
/// the TLS test passes a configuration, to trust its own throwaway certificate;
/// the function is private to this module, so nothing outside it can.
fn build_agent(policy: TransportPolicy, tls: Option<Arc<rustls::ClientConfig>>) -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .timeout(policy.timeout)
        .redirects(policy.redirects)
        .try_proxy_from_env(policy.proxy_from_env)
        .https_only(policy.https_only);
    if let Some(config) = tls {
        builder = builder.tls_config(config);
    }
    builder.build()
}

/// The capture client.
pub struct CaptureTransport {
    policy: TransportPolicy,
    agent: ureq::Agent,
}

impl CaptureTransport {
    /// The production client.
    pub fn new() -> Self {
        Self::with_policy(CAPTURE_POLICY)
    }

    /// A client with a different policy. Tests use this to reach a plain-http
    /// loopback listener; production uses [`CaptureTransport::new`].
    pub fn with_policy(policy: TransportPolicy) -> Self {
        Self { policy, agent: build_agent(policy, None) }
    }

    /// The policy this client was built with.
    pub fn policy(&self) -> TransportPolicy {
        self.policy
    }
}

impl Default for CaptureTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpTransport for CaptureTransport {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        let mut request = self.agent.request(req.method.as_str(), &req.url);
        for (name, value) in &req.headers {
            request = request.set(name, value);
        }
        let sent = match &req.body {
            Some(body) => request.send_bytes(body),
            None => request.call(),
        };
        match sent {
            Ok(resp) => {
                let status = resp.status();
                Ok(HttpResponse { status, body: read_body(resp) })
            }
            // A 4xx or 5xx is a normal round trip, so it comes back as a
            // response. Taken apart by hand, never formatted: ureq's Display
            // for this case prints the request URL.
            Err(ureq::Error::Status(status, resp)) => {
                Ok(HttpResponse { status, body: read_body(resp) })
            }
            // The kind and the host, and nothing else. ureq's Display for a
            // transport error starts with the full URL, query string included.
            Err(ureq::Error::Transport(t)) => Err(DataError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("{} to {} failed: {}", req.method, host_of(&req.url), t.kind()),
            ))),
        }
    }
}

fn read_body(resp: ureq::Response) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = resp.into_reader().take(MAX_BODY).read_to_end(&mut buf);
    buf
}

/// The host of a URL, for an error message. Never userinfo, path, or query.
fn host_of(url: &str) -> &str {
    url.split_once("://")
        .and_then(|(_, rest)| rest.split(['/', '?', '#']).next())
        .map(|authority| authority.rsplit('@').next().unwrap_or(authority))
        .filter(|host| !host.is_empty())
        .unwrap_or("the source")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    /// Loopback tests speak plain http to 127.0.0.1, so they relax ONLY
    /// `https_only` and shorten the timeout. Redirects and the proxy setting
    /// stay at the production values.
    fn loopback_policy() -> TransportPolicy {
        TransportPolicy { https_only: false, timeout: Duration::from_secs(5), ..CAPTURE_POLICY }
    }

    /// Read an HTTP request head, stopping at the blank line.
    fn read_head(stream: &mut impl Read) {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && head.len() < 64 * 1024 {
            match stream.read(&mut byte) {
                Ok(1) => head.push(byte[0]),
                _ => break,
            }
        }
    }

    /// Accept one connection, read its request, answer with `response`.
    fn serve_once(listener: TcpListener, response: String) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                read_head(&mut stream);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        })
    }

    fn nothing_connected(listener: &TcpListener) -> bool {
        matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
    }

    #[test]
    fn the_production_policy_is_no_redirects_no_env_proxy_https_only() {
        let t = CaptureTransport::new();
        assert_eq!(t.policy(), CAPTURE_POLICY);
        assert_eq!(CAPTURE_POLICY.redirects, 0, "a key-bearing request must not be redirected");
        assert!(!CAPTURE_POLICY.proxy_from_env, "a proxy would see the key in the query");
        assert!(CAPTURE_POLICY.https_only, "the key must never travel in clear text");
    }

    #[test]
    fn a_redirect_is_returned_not_followed() {
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        let target_port = target.local_addr().unwrap().port();
        target.set_nonblocking(true).unwrap();
        let origin = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin_port = origin.local_addr().unwrap().port();
        let server = serve_once(
            origin,
            format!(
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{target_port}/collect\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        );

        let t = CaptureTransport::with_policy(loopback_policy());
        let resp = t
            .send(&HttpRequest::get(format!(
                "http://127.0.0.1:{origin_port}/search?api_key=SUPERSECRET123"
            )))
            .expect("a 302 comes back as a response");
        server.join().unwrap();
        assert_eq!(resp.status, 302);

        // Allow a follow-up connection, if one were coming, time to land.
        std::thread::sleep(Duration::from_millis(200));
        assert!(nothing_connected(&target), "the redirect target was contacted");
    }

    #[test]
    fn https_only_refuses_plain_http_before_connecting() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();

        let err = CaptureTransport::new()
            .send(&HttpRequest::get(format!("http://127.0.0.1:{port}/search?api_key=SUPERSECRET123")))
            .expect_err("plain http must be refused");
        let text = err.to_string();
        assert!(!text.contains("SUPERSECRET123"), "{text}");
        assert!(!text.contains("api_key") && !text.contains('?'), "the query leaked: {text}");
        assert!(nothing_connected(&listener), "it connected before refusing");
    }

    #[test]
    fn a_403_and_a_refused_connection_carry_neither_key_nor_query() {
        // A 403 is a response: the transport produces no error text at all.
        let origin = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = origin.local_addr().unwrap().port();
        let server = serve_once(
            origin,
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 9\r\nConnection: close\r\n\r\nforbidden"
                .to_string(),
        );
        let t = CaptureTransport::with_policy(loopback_policy());
        let resp = t
            .send(&HttpRequest::get(format!("http://127.0.0.1:{port}/s?api_key=SUPERSECRET123")))
            .expect("a 403 comes back as a response");
        server.join().unwrap();
        assert_eq!(resp.status, 403);

        // Refused: bind a port, release it, then connect to nothing there.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let err = t
            .send(&HttpRequest::get(format!("http://127.0.0.1:{closed}/s?api_key=SUPERSECRET123")))
            .expect_err("nothing is listening");
        let text = err.to_string();
        assert!(!text.contains("SUPERSECRET123"), "{text}");
        assert!(!text.contains("api_key") && !text.contains('?'), "the query leaked: {text}");
        assert!(text.contains("127.0.0.1"), "the host should be named: {text}");
    }

    #[test]
    fn a_certificate_no_root_vouches_for_is_refused() {
        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let issued = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()])
            .expect("a throwaway certificate");
        let cert = issued.cert.der().clone();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(issued.key_pair.serialize_der()));
        let server_config = Arc::new(
            rustls::ServerConfig::builder_with_provider(provider.clone())
                .with_safe_default_protocol_versions()
                .expect("protocol versions")
                .with_no_client_auth()
                .with_single_cert(vec![cert.clone()], key)
                .expect("server certificate"),
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // Two connections: the production client, which must fail, then one
        // that trusts this certificate, which must succeed. The second proves
        // the server works, so the first failed on the certificate and not on
        // the plumbing.
        std::thread::spawn(move || {
            for _ in 0..2 {
                let Ok((tcp, _)) = listener.accept() else { return };
                let _ = tcp.set_read_timeout(Some(Duration::from_secs(5)));
                let Ok(conn) = rustls::ServerConnection::new(server_config.clone()) else { return };
                let mut tls = rustls::StreamOwned::new(conn, tcp);
                read_head(&mut tls);
                let _ = tls.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
                let _ = tls.flush();
                tls.conn.send_close_notify();
                let _ = tls.flush();
            }
        });

        let url = format!("https://127.0.0.1:{port}/search?api_key=SUPERSECRET123");
        let policy = TransportPolicy { timeout: Duration::from_secs(5), ..CAPTURE_POLICY };

        // The production builder: the webpki roots do not vouch for this.
        let err = CaptureTransport::with_policy(policy)
            .send(&HttpRequest::get(url.clone()))
            .expect_err("a self-signed certificate must be refused");
        assert!(!err.to_string().contains("SUPERSECRET123"), "{err}");

        // The same builder, with this certificate added as a root: connects.
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).expect("root");
        let trusting = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("protocol versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        let t = CaptureTransport { policy, agent: build_agent(policy, Some(Arc::new(trusting))) };
        let resp = t.send(&HttpRequest::get(url)).expect("a trusted root must connect");
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn an_error_names_the_host_without_userinfo() {
        assert_eq!(host_of("https://api.sam.gov/opportunities/v2/search?api_key=x"), "api.sam.gov");
        assert_eq!(host_of("https://user:pass@api.sam.gov/"), "api.sam.gov");
        assert_eq!(host_of("not a url"), "the source");
    }
}

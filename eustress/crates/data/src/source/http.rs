//! The one HTTP seam every remote provider uses.
//!
//! Providers are testable without a network because they never construct a
//! client: they hold an [`HttpTransport`] and a test hands them a stub. That
//! only works if there is exactly ONE seam — two of them means a stub written
//! for one provider cannot drive another, and a security property proven on one
//! path says nothing about the other.
//!
//! This type is the superset of what the providers actually need:
//!
//! - **A typed method, including `POST`.** Graph databases and GraphQL send
//!   queries in a body; blob stores only read. One enum covers both.
//! - **An optional request body, as bytes.** Text bodies convert in; nothing is
//!   forced through UTF-8 that is not text.
//! - **A response body as bytes, with [`HttpResponse::text`] for the text
//!   providers.** A blob store returns parquet and images. A `String` response
//!   cannot hold those, and lossily decoding a blob to make it fit would corrupt
//!   the data silently.
//!
//! ## Header values are never formattable
//!
//! [`HttpRequest`]'s `Debug` prints header NAMES and never their values. An auth
//! header holds a live credential, and `Debug` output reaches logs, panic
//! messages, and error reports. Redacting only headers known to be secret means
//! a provider that invents a new auth header leaks it; redacting all of them
//! cannot regress that way.

use std::fmt;
use std::sync::Arc;

use crate::{DataError, Result};

/// HTTP verb. Only what the providers use, so an unsupported verb is a compile
/// error rather than a runtime surprise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    /// Read a resource.
    Get,
    /// Read a resource's metadata only — the cheap liveness probe.
    Head,
    /// Send a query or document in the body.
    Post,
}

impl HttpMethod {
    /// The wire token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
        }
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One outbound request.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    /// Absolute URL, including scheme and any query string.
    pub url: String,
    /// Header name/value pairs, in send order.
    pub headers: Vec<(String, String)>,
    /// Request body. Bytes, so a text body and a binary one share a path.
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self { method: HttpMethod::Get, url: url.into(), headers: Vec::new(), body: None }
    }

    pub fn head(url: impl Into<String>) -> Self {
        Self { method: HttpMethod::Head, url: url.into(), headers: Vec::new(), body: None }
    }

    /// A JSON POST — the shape every graph and GraphQL query uses.
    pub fn post_json(url: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Post,
            url: url.into(),
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Accept".to_string(), "application/json".to_string()),
            ],
            body: Some(body.into().into_bytes()),
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn with_headers(mut self, headers: impl IntoIterator<Item = (String, String)>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// The request body as text, when there is one.
    pub fn body_text(&self) -> Option<String> {
        self.body.as_ref().map(|b| String::from_utf8_lossy(b).into_owned())
    }

    /// Whether a header was set, by case-insensitive name. For assertions that
    /// a provider sent auth without exposing the value.
    pub fn has_header(&self, name: &str) -> bool {
        self.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name))
    }

    /// A header's value, by case-insensitive name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl fmt::Debug for HttpRequest {
    /// Header NAMES only. See the module docs: no header value is ever
    /// formattable, not merely the ones known today to be secret.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(k, _)| k.as_str()).collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &redact_query(&self.url))
            .field("headers", &names)
            .field("body_bytes", &self.body.as_ref().map(|b| b.len()))
            .finish()
    }
}

/// One inbound response.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    /// Body bytes. Empty for a `HEAD`.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// A text response, for the providers that parse JSON.
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self { status, body: body.into().into_bytes() }
    }

    /// A binary response, for the blob stores.
    pub fn bytes(status: u16, body: Vec<u8>) -> Self {
        Self { status, body }
    }

    /// A 2xx status.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The body decoded as UTF-8, lossily.
    ///
    /// Lossy on purpose: a provider that has already decided the payload is
    /// text should surface a mangled character rather than fail the whole read
    /// over one bad byte in a field it may not even use.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

/// Blocking HTTP transport.
///
/// `Send + Sync` so a source holding one stays `Send + Sync` and satisfies
/// [`super::DataSource`].
pub trait HttpTransport: Send + Sync {
    /// Issue `request` and return the response.
    ///
    /// Return `Err` only for TRANSPORT failures (DNS, connect, TLS, read). A
    /// 404 or a 500 is a successful round trip and belongs in the status, so a
    /// provider can report the server's own reason instead of a generic error.
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse>;
}

/// The transport used when no HTTP client is compiled in.
///
/// Every provider still constructs, validates, and can be driven by a stub — it
/// is only the live call that is unavailable, and it says so by name rather
/// than failing obscurely.
pub struct UnavailableTransport;

impl HttpTransport for UnavailableTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse> {
        Err(DataError::Schema(format!(
            "no HTTP transport is compiled in, so {} {} cannot be sent; \
             rebuild with the `http` feature, or inject one with `with_transport`",
            request.method,
            redact_query(&request.url)
        )))
    }
}

/// The default transport for a source built with `new`.
pub(super) fn default_transport() -> Arc<dyn HttpTransport> {
    #[cfg(feature = "http")]
    {
        Arc::new(UreqTransport::default())
    }
    #[cfg(not(feature = "http"))]
    {
        Arc::new(UnavailableTransport)
    }
}

/// Real blocking transport, on `ureq` — the same client the rest of the
/// workspace already links. Requires the `http` feature.
#[cfg(feature = "http")]
#[derive(Debug, Clone)]
pub struct UreqTransport {
    /// Connect + read timeout.
    pub timeout: std::time::Duration,
}

#[cfg(feature = "http")]
impl Default for UreqTransport {
    fn default() -> Self {
        Self { timeout: std::time::Duration::from_secs(30) }
    }
}

#[cfg(feature = "http")]
impl HttpTransport for UreqTransport {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        use std::io::Read;
        let agent = ureq::AgentBuilder::new().timeout(self.timeout).build();
        let mut r = agent.request(req.method.as_str(), &req.url);
        for (k, v) in &req.headers {
            r = r.set(k, v);
        }
        let sent = match &req.body {
            Some(b) => r.send_bytes(b),
            None => r.call(),
        };
        // Bodies are read as BYTES: a blob store returns parquet and images, and
        // decoding those as text to fit a String would corrupt them silently.
        let read_body = |resp: ureq::Response| -> Vec<u8> {
            let mut buf = Vec::new();
            let _ = resp.into_reader().take(64 * 1024 * 1024).read_to_end(&mut buf);
            buf
        };
        match sent {
            Ok(resp) => {
                let status = resp.status();
                Ok(HttpResponse { status, body: read_body(resp) })
            }
            // ureq reports a non-2xx status as an error; the provider wants it
            // as a normal response so every status flows through one code path.
            Err(ureq::Error::Status(status, resp)) => {
                Ok(HttpResponse { status, body: read_body(resp) })
            }
            Err(ureq::Error::Transport(t)) => Err(DataError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                // The URL is config, never a secret; the auth header is not in
                // the transport error.
                // The path is config; the query can hold a pre-signed credential.
                format!("{} {} failed: {t}", req.method, redact_query(&req.url)),
            ))),
        }
    }
}

/// A URL with its query string redacted.
///
/// Azure and S3 pre-signed URLs carry the credential IN THE QUERY (`?sig=…`,
/// `?X-Amz-Signature=…`), so printing a URL verbatim leaks exactly what
/// redacting headers was meant to prevent. The path is kept because it is the
/// useful part of a diagnostic.
pub fn redact_query(url: &str) -> String {
    match url.split_once('?') {
        Some((base, _)) => format!("{base}?<redacted>"),
        None => url.to_string(),
    }
}

/// A short, single-line excerpt of a response body, for error messages.
///
/// Bounded because a server's error page can be megabytes, and an error message
/// that dumps it is unreadable in a log and useless in a UI.
pub fn excerpt(body: &[u8]) -> String {
    const MAX: usize = 200;
    let text = String::from_utf8_lossy(body);
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > MAX {
        let cut: String = flat.chars().take(MAX).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

/// Whether an authority (`host` or `host:port`) is loopback.
///
/// Used to allow plain `http://` against a local stub in tests while requiring
/// TLS everywhere else, so a test fixture can never become the reason a
/// credential goes out in clear text to a real host.
pub fn is_loopback_authority(authority: &str) -> bool {
    let host = authority
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(authority)
        .trim_matches(|c| c == '[' || c == ']');
    host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host.starts_with("127.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_prints_a_header_value() {
        let req = HttpRequest::get("https://api.example.com/x")
            .with_header("Authorization", "Bearer super-secret-token")
            .with_header("X-Custom-Auth", "another-secret");
        let rendered = format!("{req:?}");

        assert!(rendered.contains("Authorization"), "names are useful and safe");
        assert!(rendered.contains("X-Custom-Auth"));
        // A provider inventing a new auth header must not be able to leak it.
        assert!(!rendered.contains("super-secret-token"));
        assert!(!rendered.contains("another-secret"));
    }

    #[test]
    fn debug_never_prints_a_credential_carried_in_the_url() {
        // A pre-signed blob URL holds the credential in the query string.
        let req = HttpRequest::get(
            "https://a.blob.core.windows.net/c/b.csv?sig=SUPERSECRET&se=2026-01-01",
        );
        let rendered = format!("{req:?}");
        assert!(!rendered.contains("SUPERSECRET"), "credential leaked: {rendered}");
        assert!(rendered.contains("<redacted>"));
        assert!(rendered.contains("b.csv"), "the path stays, it is the useful part");
    }

    #[test]
    fn redact_query_leaves_a_bare_url_alone() {
        assert_eq!(redact_query("https://x.test/a/b"), "https://x.test/a/b");
    }

    #[test]
    fn debug_reports_body_size_not_body_content() {
        let req = HttpRequest::post_json("https://x.test/q", r#"{"query":"secret business logic"}"#);
        let rendered = format!("{req:?}");
        assert!(rendered.contains("body_bytes"));
        assert!(!rendered.contains("secret business logic"));
    }

    #[test]
    fn a_text_body_round_trips() {
        let req = HttpRequest::post_json("https://x.test/q", "{\"a\":1}");
        assert_eq!(req.body_text().as_deref(), Some("{\"a\":1}"));
        assert_eq!(req.method, HttpMethod::Post);
        assert!(req.has_header("content-type"), "lookup is case-insensitive");
        assert_eq!(req.header("Accept"), Some("application/json"));
    }

    #[test]
    fn a_binary_response_is_not_forced_through_utf8() {
        // A parquet blob is not text; the seam must carry it intact.
        let blob = vec![0x50, 0x41, 0x52, 0x31, 0xff, 0xfe, 0x00, 0x01];
        let resp = HttpResponse::bytes(200, blob.clone());
        assert_eq!(resp.body, blob);
        assert!(resp.is_success());
    }

    #[test]
    fn text_decodes_lossily_rather_than_failing_the_read() {
        let resp = HttpResponse::bytes(200, vec![b'o', b'k', 0xff]);
        assert!(resp.text().starts_with("ok"));
    }

    #[test]
    fn status_classes_are_distinguished() {
        assert!(HttpResponse::new(204, "").is_success());
        assert!(!HttpResponse::new(404, "missing").is_success());
        assert!(!HttpResponse::new(500, "boom").is_success());
    }

    #[test]
    fn excerpt_is_bounded_and_single_line() {
        let long = "x ".repeat(1000);
        let e = excerpt(long.as_bytes());
        assert!(e.chars().count() <= 201, "bounded");
        assert!(!e.contains('\n'));

        assert_eq!(excerpt(b"line one\n  line two"), "line one line two");
    }

    #[test]
    fn loopback_is_recognised_in_its_usual_spellings() {
        for a in ["localhost", "localhost:8080", "127.0.0.1", "127.0.0.1:0", "127.4.5.6", "[::1]:80"]
        {
            assert!(is_loopback_authority(a), "{a} should be loopback");
        }
        for a in ["example.com", "10.0.0.1", "evil-localhost.com", "notlocalhost"] {
            assert!(!is_loopback_authority(a), "{a} must NOT be loopback");
        }
    }

    #[test]
    fn the_unavailable_transport_names_what_it_could_not_send() {
        let err = UnavailableTransport
            .send(&HttpRequest::get("https://api.example.com/rows"))
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("GET"));
        assert!(msg.contains("api.example.com"));
        // The same honest-failure contract every provider asserts: name the
        // missing feature AND the way out, so this is never mistaken for an
        // empty result.
        assert!(msg.contains("`http` feature"));
        assert!(msg.contains("with_transport"));
    }
}

//! Azure Blob Storage source — the Data menu's "AzureBlob" provider.
//!
//! Unlike [`super::postgres`] and [`super::s3`], this provider really reads.
//! Azure Blob's data plane is plain HTTPS: `GET` the blob URL, optionally with
//! a SAS token in the query string or a bearer token in `Authorization`, and
//! the object comes back as bytes. None of that needs an SDK, a Tokio runtime,
//! or a crypto library — so this module implements the whole request, and asks
//! the caller for the one thing a dependency-free leaf cannot own: a blocking
//! HTTP client.
//!
//! ## The transport seam
//!
//! [`HttpTransport`] is that seam. The Studio injects one built on `ureq`
//! (which the engine already links); a test injects one built on
//! [`std::net::TcpStream`] and points it at a [`std::net::TcpListener`] bound
//! to port 0. Either way `eustress-data` stays a leaf with no HTTP dependency
//! (`DATA_PLATFORM_PLAN.md` invariant **D2**), and the *whole* fetch path —
//! URL construction, auth headers, status handling, decoding — is exercised in
//! CI with no network and no credentials.
//!
//! The trait lives in this module because Azure Blob is where it was first
//! needed; [`super::oracle`] uses it too. Hoist it to a shared `source::http`
//! module the moment a third provider wants it.
//!
//! ## Secrets
//!
//! [`SourceConfig::secret_ref`] names the environment variable holding the SAS
//! token or the bearer token; it is read at call time, never stored. Because a
//! SAS token *is* a query string, [`HttpRequest`]'s [`fmt::Debug`] redacts every
//! URL query and every credential-bearing header — a request struct can be
//! logged without leaking the credential that makes it work.

// This provider parses JSON and shares the HTTP seam in `rest`, both of
// which live behind `import`. Without this gate the leaf fails to build
// with default features, which is the D2 purity contract in lib.rs.
#![cfg(feature = "import")]

use std::fmt;
use std::sync::Arc;

use super::{ConnectionStatus, DataSource, SourceConfig, SourceKind};
use crate::{DataError, Frame, Result};

/// The `x-ms-version` used when the config does not pin one.
pub const DEFAULT_API_VERSION: &str = "2021-08-06";

/// The public Blob endpoint suffix for the Azure commercial cloud.
const PUBLIC_SUFFIX: &str = "blob.core.windows.net";

/// The longest blob name Azure accepts, in characters.
const MAX_BLOB_CHARS: usize = 1024;

// ── The HTTP seam ────────────────────────────────────────────────────────────

/// The HTTP verbs this crate's providers issue. A source reads; there is
/// deliberately no way to express a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// Read a resource.
    Get,
    /// Read a resource's metadata only — the liveness probe.
    Head,
}

impl HttpMethod {
    /// The wire token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
        }
    }
}

/// One blocking HTTP request, fully resolved.
///
/// [`fmt::Debug`] is implemented by hand: it redacts the URL query string and
/// every credential-bearing header, so a request can be logged or surfaced in
/// an error without leaking a SAS or bearer token.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    /// Verb.
    pub method: HttpMethod,
    /// Absolute URL, including any query string.
    pub url: String,
    /// Header name/value pairs, in send order.
    pub headers: Vec<(String, String)>,
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| (k.as_str(), if is_secret_header(k) { "<redacted>" } else { v.as_str() }))
            .collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &redact_query(&self.url))
            .field("headers", &headers)
            .finish()
    }
}

/// Whether a header's value must never be printed.
fn is_secret_header(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "authorization"
        || n == "cookie"
        || n.contains("token")
        || n.contains("secret")
        || n.contains("-key")
        || n.contains("signature")
}

/// Strip a URL's query string — a SAS token lives there.
fn redact_query(url: &str) -> String {
    match url.split_once('?') {
        Some((base, _)) => format!("{base}?<redacted>"),
        None => url.to_string(),
    }
}

/// One HTTP response.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Status code.
    pub status: u16,
    /// Body bytes (empty for a `HEAD`).
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// A 2xx status.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Bodies can be large and can echo a signed URL; print shape, not content.
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// A blocking HTTP client, injected by the caller.
///
/// `eustress-data` deliberately links none: the engine already carries `ureq`,
/// and the leaf must stay dependency-free (invariant **D2**). Implementations
/// must be usable from any thread, because a live source polls on a worker.
pub trait HttpTransport: Send + Sync {
    /// Issue `request` and return the response. Return `Err` only for transport
    /// failures (DNS, connect, TLS, read); a 404 is a successful round trip.
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse>;
}

// ── Resolved target ──────────────────────────────────────────────────────────

/// How a blob's bytes are decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobFormat {
    /// Comma-separated values with a header row.
    Csv,
    /// A JSON document holding an array of records.
    Json,
    /// Newline-delimited JSON objects.
    Jsonl,
}

impl BlobFormat {
    /// Stable wire token, as written in the `format` option.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Jsonl => "jsonl",
        }
    }

    /// Parse the `format` option.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "csv" => Some(Self::Csv),
            "json" => Some(Self::Json),
            "jsonl" | "ndjson" => Some(Self::Jsonl),
            _ => None,
        }
    }

    /// Infer from a blob name's extension.
    pub fn from_name(name: &str) -> Option<Self> {
        let ext = name.rsplit('.').next()?;
        if ext == name {
            return None;
        }
        Self::parse(ext)
    }

    /// Every token the `format` option accepts, for error messages.
    const TOKENS: &'static str = "csv, json, jsonl";
}

/// How the request authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobAuth {
    /// A public container — no credential at all.
    Anonymous,
    /// A shared access signature, appended to the URL query.
    Sas,
    /// An Entra ID access token in `Authorization: Bearer …`.
    Bearer,
}

impl BlobAuth {
    /// Stable wire token, as written in the `auth` option.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anonymous => "anonymous",
            Self::Sas => "sas",
            Self::Bearer => "bearer",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "anonymous" | "none" | "public" => Some(Self::Anonymous),
            "sas" => Some(Self::Sas),
            "bearer" | "entra" | "aad" => Some(Self::Bearer),
            _ => None,
        }
    }

    /// Whether this mode needs [`SourceConfig::secret_ref`] to be set.
    fn needs_secret(self) -> bool {
        !matches!(self, Self::Anonymous)
    }
}

/// A fully resolved blob target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobTarget {
    /// Storage account name.
    pub account: String,
    /// Container name.
    pub container: String,
    /// Blob name (may contain `/`).
    pub blob: String,
    /// How the bytes are decoded.
    pub format: BlobFormat,
    /// How the request authenticates.
    pub auth: BlobAuth,
    /// `x-ms-version` sent with every request.
    pub api_version: String,
    /// Scheme+host of an Azurite (or other emulator) endpoint, if configured.
    pub emulator_base: Option<String>,
    /// For [`BlobFormat::Json`]: the dotted path to the array of records.
    /// Empty means the document itself is the array.
    pub records_path: String,
}

impl BlobTarget {
    /// The URL the blob lives at, without any SAS query.
    ///
    /// Path style against an emulator (Azurite puts the account in the path),
    /// host style against the public cloud. Pure, so the UI can show it and a
    /// test can assert on it.
    pub fn blob_url(&self) -> String {
        match &self.emulator_base {
            Some(base) => format!(
                "{}/{}/{}/{}",
                base.trim_end_matches('/'),
                self.account,
                self.container,
                self.blob
            ),
            None => format!(
                "https://{}.{PUBLIC_SUFFIX}/{}/{}",
                self.account, self.container, self.blob
            ),
        }
    }
}

impl fmt::Display for BlobTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.account, self.container, self.blob)
    }
}

// ── Source ───────────────────────────────────────────────────────────────────

/// The Azure Blob Storage provider.
///
/// Construction validates. A transport must be attached with
/// [`with_transport`](AzureBlobSource::with_transport) before the source can
/// reach anything — without one it says so plainly rather than failing oddly.
#[derive(Clone)]
pub struct AzureBlobSource {
    config: SourceConfig,
    target: BlobTarget,
    transport: Option<Arc<dyn HttpTransport>>,
}

impl fmt::Debug for AzureBlobSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AzureBlobSource")
            .field("target", &self.target)
            .field("transport", &if self.transport.is_some() { "attached" } else { "none" })
            .finish()
    }
}

impl AzureBlobSource {
    /// Validate `config` and resolve its target. No transport is attached yet.
    pub fn new(config: SourceConfig) -> Result<Self> {
        validate(&config)?;
        let target = resolve_target(&config)?;
        Ok(Self { config, target, transport: None })
    }

    /// Attach the blocking HTTP client this source will use.
    pub fn with_transport(mut self, transport: Arc<dyn HttpTransport>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// The config this source was built from.
    pub fn config(&self) -> &SourceConfig {
        &self.config
    }

    /// The resolved target.
    pub fn target(&self) -> &BlobTarget {
        &self.target
    }

    /// Build the request this source would issue, resolving the credential from
    /// the environment at call time.
    ///
    /// Public so the UI can show exactly what will be sent — its [`fmt::Debug`]
    /// is redacted, so doing so cannot leak the credential.
    pub fn request(&self, method: HttpMethod) -> Result<HttpRequest> {
        let mut url = self.target.blob_url();
        let mut headers =
            vec![("x-ms-version".to_string(), self.target.api_version.clone())];

        match self.target.auth {
            BlobAuth::Anonymous => {}
            BlobAuth::Sas => {
                let sas = self.secret()?;
                url = format!("{url}?{}", sas.trim_start_matches('?'));
            }
            BlobAuth::Bearer => {
                headers.push(("Authorization".to_string(), format!("Bearer {}", self.secret()?)));
            }
        }
        Ok(HttpRequest { method, url, headers })
    }

    /// The credential named by [`SourceConfig::secret_ref`], read now.
    fn secret(&self) -> Result<String> {
        let name = self.config.secret_ref.as_deref().unwrap_or("<unset>");
        self.config.resolve_secret().ok_or_else(|| {
            DataError::Schema(format!(
                "Azure Blob source is configured for {} auth, but the environment variable \
                 '{name}' named by secret_ref is not set",
                self.target.auth.as_str()
            ))
        })
    }

    fn transport(&self) -> Result<&dyn HttpTransport> {
        self.transport.as_deref().ok_or_else(|| {
            DataError::Io(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!(
                    "Azure Blob source for {} has no HTTP transport — eustress-data links no HTTP \
                     client (leaf purity, invariant D2), so the caller must attach one with \
                     `AzureBlobSource::with_transport`",
                    self.target
                ),
            ))
        })
    }
}

impl DataSource for AzureBlobSource {
    fn kind(&self) -> SourceKind {
        SourceKind::AzureBlob
    }

    fn test_connection(&self) -> Result<ConnectionStatus> {
        let transport = self.transport()?;
        let request = self.request(HttpMethod::Head)?;
        // A transport failure is an *answer* to "is it reachable?", not an error
        // in asking; only being unable to ask at all is `Err`.
        match transport.send(&request) {
            Err(e) => Ok(ConnectionStatus::failed(format!("{} unreachable: {e}", self.target))),
            Ok(r) if r.is_success() => {
                Ok(ConnectionStatus::ok(format!("HTTP {} from {}", r.status, self.target)))
            }
            Ok(r) => Ok(ConnectionStatus::failed(format!(
                "HTTP {} for {} ({})",
                r.status,
                self.target,
                status_hint(r.status)
            ))),
        }
    }

    fn fetch(&self) -> Result<Frame> {
        let transport = self.transport()?;
        let request = self.request(HttpMethod::Get)?;
        let response = transport.send(&request)?;
        if !response.is_success() {
            return Err(DataError::Io(std::io::Error::other(format!(
                "Azure Blob GET {} returned HTTP {} ({}): {}",
                self.target,
                response.status,
                status_hint(response.status),
                excerpt(&response.body)
            ))));
        }
        decode(self.target.format, &response.body, &self.target.records_path)
    }
}

/// Plain-language reading of the status codes the Blob API actually returns.
fn status_hint(status: u16) -> &'static str {
    match status {
        401 => "not authenticated — check the SAS or bearer token",
        403 => "authenticated but not authorized, or the SAS has expired",
        404 => "no such container or blob",
        409 => "conflict",
        429 => "throttled by the storage account",
        s if (500..600).contains(&s) => "storage service error",
        _ => "unexpected status",
    }
}

/// A short, single-line, printable slice of a response body — enough to
/// diagnose a service's error payload without pasting a megabyte into a log.
/// Shared with [`super::oracle`].
pub(super) fn excerpt(body: &[u8]) -> String {
    let cut = body.len().min(240);
    let text = String::from_utf8_lossy(&body[..cut]);
    let flat = text.replace(['\n', '\r', '\t'], " ");
    let trimmed = flat.trim().to_string();
    if body.len() > cut {
        format!("{trimmed}…")
    } else {
        trimmed
    }
}

// ── Decoding (shared with `super::oracle`) ───────────────────────────────────

fn decode(format: BlobFormat, body: &[u8], records_path: &str) -> Result<Frame> {
    match format {
        BlobFormat::Csv => frame_from_csv_bytes(body),
        BlobFormat::Jsonl => frame_from_jsonl_bytes(body),
        BlobFormat::Json => frame_from_json_records(body, records_path),
    }
}

/// The error a build without the `import` feature returns once the bytes are
/// already in hand — the request succeeded, only the decoder is absent.
#[cfg(not(feature = "import"))]
pub(super) fn decoder_unavailable(what: &str) -> DataError {
    DataError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!(
            "the response arrived, but decoding a {what} body requires the 'import' feature \
             (csv + serde_json), which is not compiled in"
        ),
    ))
}

#[cfg(feature = "import")]
fn frame_from_csv_bytes(body: &[u8]) -> Result<Frame> {
    crate::import::frame_from_csv(body)
}

#[cfg(not(feature = "import"))]
fn frame_from_csv_bytes(_body: &[u8]) -> Result<Frame> {
    Err(decoder_unavailable("CSV"))
}

#[cfg(feature = "import")]
fn frame_from_jsonl_bytes(body: &[u8]) -> Result<Frame> {
    crate::import::frame_from_jsonl(body)
}

#[cfg(not(feature = "import"))]
fn frame_from_jsonl_bytes(_body: &[u8]) -> Result<Frame> {
    Err(decoder_unavailable("JSONL"))
}

/// Decode a JSON document whose records live at `records_path` (a dotted path;
/// empty means the document itself is the array) into a [`Frame`].
///
/// Shared with [`super::oracle`], whose ORDS payloads wrap their rows in
/// `items`.
#[cfg(feature = "import")]
pub(super) fn frame_from_json_records(body: &[u8], records_path: &str) -> Result<Frame> {
    let root: serde_json::Value = serde_json::from_slice(body)
        .map_err(|e| DataError::Schema(format!("response is not JSON: {e}")))?;

    let mut node = &root;
    for segment in records_path.split('.').filter(|s| !s.is_empty()) {
        node = node.get(segment).ok_or_else(|| {
            DataError::Schema(format!(
                "response has no '{records_path}' field (looking for '{segment}')"
            ))
        })?;
    }

    let array = node.as_array().ok_or_else(|| {
        DataError::Schema(if records_path.is_empty() {
            "response is not a JSON array of records".to_string()
        } else {
            format!("response field '{records_path}' is not a JSON array of records")
        })
    })?;

    // Reuse the JSONL decoder so dtype/unit inference is identical everywhere.
    let mut jsonl = String::new();
    for (i, item) in array.iter().enumerate() {
        if !item.is_object() {
            return Err(DataError::Schema(format!("record {i} is not a JSON object")));
        }
        jsonl.push_str(&item.to_string());
        jsonl.push('\n');
    }
    crate::import::frame_from_jsonl(jsonl.as_bytes())
}

#[cfg(not(feature = "import"))]
pub(super) fn frame_from_json_records(_body: &[u8], _records_path: &str) -> Result<Frame> {
    Err(decoder_unavailable("JSON"))
}

// ── Validation (pure, offline) ───────────────────────────────────────────────

/// Validate an Azure Blob config completely, without touching the network.
pub fn validate(config: &SourceConfig) -> Result<()> {
    if config.kind != SourceKind::AzureBlob {
        return Err(DataError::Schema(format!(
            "Azure Blob validation was handed a {} config",
            config.kind.as_str()
        )));
    }
    super::validate_config(config)?;
    resolve_target(config).map(|_| ())
}

fn schema_err(msg: impl Into<String>) -> DataError {
    DataError::Schema(msg.into())
}

fn non_empty(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|s| !s.is_empty())
}

fn resolve_target(config: &SourceConfig) -> Result<BlobTarget> {
    let endpoint = config.endpoint.trim();
    let emulator_base = resolve_emulator_base(config)?;

    let (account, container, path_blob) = if let Some(rest) = endpoint.strip_prefix("azure://") {
        let account = non_empty(config.option("account"))
            .ok_or_else(|| {
                schema_err(
                    "Azure Blob endpoint azure://container/blob requires the 'account' option \
                     naming the storage account",
                )
            })?
            .to_string();
        let (container, blob) = split_first(rest);
        (account, container.to_string(), blob.map(str::to_string))
    } else if let Some(rest) = endpoint.strip_prefix("https://") {
        if emulator_base.is_some() {
            return Err(schema_err(
                "Azure Blob sets 'emulator_base' with a https:// endpoint — the emulator form \
                 uses azure://container/blob plus the 'account' option",
            ));
        }
        let (host, path) = split_first(rest);
        let account = host.split('.').next().unwrap_or("").to_string();
        if account.is_empty() || !host.contains('.') {
            return Err(schema_err(format!(
                "Azure Blob endpoint host '{host}' does not name a storage account (expected \
                 https://<account>.{PUBLIC_SUFFIX}/<container>/<blob>)"
            )));
        }
        if let Some(named) = non_empty(config.option("account")) {
            if named != account {
                return Err(schema_err(format!(
                    "Azure Blob endpoint names account '{account}' but the 'account' option says \
                     '{named}' — set exactly one"
                )));
            }
        }
        let (container, blob) = split_first(path.unwrap_or(""));
        (account, container.to_string(), blob.map(str::to_string))
    } else {
        return Err(schema_err(format!(
            "Azure Blob endpoint '{endpoint}' must be https://<account>.{PUBLIC_SUFFIX}/… or \
             azure://<container>/<blob>"
        )));
    };

    validate_account(&account)?;
    validate_container(&container)?;

    let blob = match (non_empty(path_blob.as_deref()), non_empty(config.option("key"))) {
        (Some(a), Some(b)) if a != b => {
            return Err(schema_err(format!(
                "Azure Blob endpoint names blob '{a}' but the 'key' option says '{b}' — set \
                 exactly one"
            )))
        }
        (Some(a), _) => a.to_string(),
        (None, Some(b)) => b.to_string(),
        (None, None) => {
            return Err(schema_err(format!(
                "Azure Blob source names no blob in container '{container}' — put it in the \
                 endpoint path, or set the 'key' option"
            )))
        }
    };
    validate_blob_name(&blob)?;

    let format = resolve_format(config, &blob)?;
    let auth = resolve_auth(config)?;
    let api_version = resolve_api_version(config)?;
    let records_path = non_empty(config.option("records_path")).unwrap_or("").to_string();
    if !records_path.is_empty() && format != BlobFormat::Json {
        return Err(schema_err(format!(
            "Azure Blob sets 'records_path' but the format is {} — a records path only applies \
             to a JSON document",
            format.as_str()
        )));
    }
    resolve_timeout_seconds(config)?;

    Ok(BlobTarget {
        account,
        container,
        blob,
        format,
        auth,
        api_version,
        emulator_base,
        records_path,
    })
}

/// Split `a/b/c` into (`a`, Some(`b/c`)).
fn split_first(s: &str) -> (&str, Option<&str>) {
    match s.split_once('/') {
        Some((head, tail)) => (head, Some(tail)),
        None => (s, None),
    }
}

fn validate_account(account: &str) -> Result<()> {
    let ok = (3..=24).contains(&account.len())
        && account.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !ok {
        return Err(schema_err(format!(
            "Azure storage account '{account}' must be 3 to 24 lowercase letters and digits"
        )));
    }
    Ok(())
}

fn validate_container(container: &str) -> Result<()> {
    let bad = |why: &str| schema_err(format!("Azure container '{container}' {why}"));
    if !(3..=63).contains(&container.len()) {
        return Err(bad("must be 3 to 63 characters long"));
    }
    if !container.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(bad("may contain only lowercase letters, digits and hyphens"));
    }
    let first = container.chars().next().unwrap_or('-');
    let last = container.chars().next_back().unwrap_or('-');
    if !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
        return Err(bad("must begin and end with a letter or a digit"));
    }
    if container.contains("--") {
        return Err(bad("must not contain two adjacent hyphens"));
    }
    Ok(())
}

fn validate_blob_name(blob: &str) -> Result<()> {
    let bad = |why: &str| schema_err(format!("Azure blob name '{blob}' {why}"));
    if blob.chars().count() > MAX_BLOB_CHARS {
        return Err(bad("is longer than 1024 characters"));
    }
    if blob.starts_with('/') {
        return Err(bad("must not start with '/'"));
    }
    if blob.ends_with('/') || blob.ends_with('.') {
        return Err(bad("must not end with '/' or '.'"));
    }
    if blob.chars().any(char::is_control) {
        return Err(bad("contains a control character"));
    }
    if blob.contains('?') || blob.contains('#') {
        return Err(bad("contains '?' or '#', which would be read as a URL query or fragment"));
    }
    if blob.chars().any(|c| c == ' ') {
        return Err(bad("contains a space — percent-encode it before storing the config"));
    }
    Ok(())
}

fn resolve_format(config: &SourceConfig, blob: &str) -> Result<BlobFormat> {
    match non_empty(config.option("format")) {
        Some(f) if f.eq_ignore_ascii_case("parquet") => Err(schema_err(
            "Azure Blob cannot decode Parquet in this crate — a Parquet reader needs a file it \
             can seek in, so download the blob and use `eustress_data::read_parquet` instead",
        )),
        Some(f) => BlobFormat::parse(f).ok_or_else(|| {
            schema_err(format!(
                "Azure Blob 'format' option '{f}' is not one of {}",
                BlobFormat::TOKENS
            ))
        }),
        None => BlobFormat::from_name(blob).ok_or_else(|| {
            schema_err(format!(
                "Azure Blob cannot infer a format from blob name '{blob}' — set the 'format' \
                 option to one of {}",
                BlobFormat::TOKENS
            ))
        }),
    }
}

fn resolve_auth(config: &SourceConfig) -> Result<BlobAuth> {
    let named = non_empty(config.option("auth"));
    let has_secret = config.secret_ref.is_some();

    let auth = match named {
        Some(a) => BlobAuth::parse(a).ok_or_else(|| {
            schema_err(format!(
                "Azure Blob 'auth' option '{a}' is not one of anonymous, sas, bearer"
            ))
        })?,
        None if has_secret => {
            return Err(schema_err(
                "Azure Blob source sets secret_ref but no 'auth' option — say whether the secret \
                 is a 'sas' token or a 'bearer' token, so the request cannot be built wrongly",
            ))
        }
        None => BlobAuth::Anonymous,
    };

    if auth.needs_secret() && !has_secret {
        return Err(schema_err(format!(
            "Azure Blob '{}' auth requires secret_ref to name the environment variable holding \
             the token",
            auth.as_str()
        )));
    }
    if !auth.needs_secret() && has_secret {
        return Err(schema_err(
            "Azure Blob 'anonymous' auth cannot use a secret_ref — set 'auth' to sas or bearer, \
             or drop the secret_ref",
        ));
    }
    Ok(auth)
}

fn resolve_api_version(config: &SourceConfig) -> Result<String> {
    let Some(v) = non_empty(config.option("api_version")) else {
        return Ok(DEFAULT_API_VERSION.to_string());
    };
    let shaped = v.len() == 10
        && v.as_bytes().iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        });
    if !shaped {
        return Err(schema_err(format!(
            "Azure Blob 'api_version' option '{v}' is not an x-ms-version date (YYYY-MM-DD)"
        )));
    }
    Ok(v.to_string())
}

fn resolve_emulator_base(config: &SourceConfig) -> Result<Option<String>> {
    let Some(base) = non_empty(config.option("emulator_base")) else {
        return Ok(None);
    };
    let rest = match base.strip_prefix("https://") {
        Some(r) => r,
        None => base.strip_prefix("http://").ok_or_else(|| {
            schema_err(format!(
                "Azure Blob 'emulator_base' option '{base}' must start with http:// or https://"
            ))
        })?,
    };
    if rest.contains('/') {
        return Err(schema_err(format!(
            "Azure Blob 'emulator_base' option '{base}' must be a bare scheme://host[:port] — the \
             account, container and blob come from the endpoint"
        )));
    }
    if rest.is_empty() {
        return Err(schema_err("Azure Blob 'emulator_base' option has no host"));
    }
    // Plaintext HTTP would put a SAS token on the wire in the clear. Azurite on
    // loopback is the one place that is acceptable.
    if base.starts_with("http://") && !is_loopback_authority(rest) {
        return Err(schema_err(format!(
            "Azure Blob 'emulator_base' option '{base}' is plaintext http:// to a non-loopback \
             host — a SAS token would travel in the clear"
        )));
    }
    Ok(Some(base.to_string()))
}

/// Whether `host[:port]` addresses this machine.
pub(super) fn is_loopback_authority(authority: &str) -> bool {
    let host = match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
}

fn resolve_timeout_seconds(config: &SourceConfig) -> Result<Option<u64>> {
    let Some(raw) = non_empty(config.option("timeout_seconds")) else {
        return Ok(None);
    };
    let n: u64 = raw.parse().map_err(|_| {
        schema_err(format!(
            "Azure Blob 'timeout_seconds' option '{raw}' is not a whole number"
        ))
    })?;
    if n == 0 {
        return Err(schema_err(
            "Azure Blob 'timeout_seconds' option must be greater than zero",
        ));
    }
    Ok(Some(n))
}

// ── Test-only HTTP rig, shared with `super::oracle` ──────────────────────────

#[cfg(test)]
pub(crate) mod testing {
    //! A real socket, no network: a one-shot-per-connection HTTP/1.1 server on
    //! `127.0.0.1:0` and a `std`-only transport that talks to it. Together they
    //! exercise the whole request path — URL, method, headers, status, body —
    //! with no dependency, no credentials and no internet.

    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{HttpRequest, HttpResponse, HttpTransport};
    use crate::{DataError, Result};

    /// A canned HTTP responder that records every request head it received.
    pub(crate) struct StubServer {
        addr: SocketAddr,
        requests: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    }

    impl StubServer {
        /// Serve `status` with `body` to every request until dropped.
        pub(crate) fn spawn(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
            let addr = listener.local_addr().expect("local addr");
            listener.set_nonblocking(true).expect("nonblocking");

            let requests = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));
            let (thread_requests, thread_stop) = (Arc::clone(&requests), Arc::clone(&stop));

            std::thread::spawn(move || {
                while !thread_stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ = serve_one(stream, status, content_type, &body, &thread_requests);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
            });

            Self { addr, requests, stop }
        }

        /// `http://127.0.0.1:<port>` — the base a config points at.
        pub(crate) fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }

        /// Every request head received so far, in order.
        pub(crate) fn requests(&self) -> Vec<String> {
            self.requests.lock().expect("stub server mutex").clone()
        }
    }

    impl Drop for StubServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    fn serve_one(
        mut stream: TcpStream,
        status: u16,
        content_type: &str,
        body: &[u8],
        log: &Arc<Mutex<Vec<String>>>,
    ) -> std::io::Result<()> {
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;

        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !ends_with(&head, b"\r\n\r\n") {
            if stream.read(&mut byte)? == 0 {
                break;
            }
            head.push(byte[0]);
        }
        let head_text = String::from_utf8_lossy(&head).to_string();
        let is_head_request = head_text.starts_with("HEAD ");
        log.lock().expect("stub server mutex").push(head_text);

        let response = format!(
            "HTTP/1.1 {status} STUB\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes())?;
        if !is_head_request {
            stream.write_all(body)?;
        }
        stream.flush()
    }

    fn ends_with(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.len() >= needle.len() && &haystack[haystack.len() - needle.len()..] == needle
    }

    /// A `std`-only blocking HTTP/1.1 client. Plaintext only — it exists so
    /// tests can drive a real socket, not to ship.
    pub(crate) struct TcpTransport;

    impl HttpTransport for TcpTransport {
        fn send(&self, request: &HttpRequest) -> Result<HttpResponse> {
            let rest = request.url.strip_prefix("http://").ok_or_else(|| {
                DataError::Io(std::io::Error::other(format!(
                    "the test transport speaks only http://, got {}",
                    super::redact_query(&request.url)
                )))
            })?;
            let (authority, path) = match rest.split_once('/') {
                Some((a, p)) => (a, format!("/{p}")),
                None => (rest, "/".to_string()),
            };

            let mut stream = TcpStream::connect(authority)?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut head = format!(
                "{} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n",
                request.method.as_str()
            );
            for (name, value) in &request.headers {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
            head.push_str("\r\n");
            stream.write_all(head.as_bytes())?;
            stream.flush()?;

            let mut raw = Vec::new();
            stream.read_to_end(&mut raw)?;

            let split = find(&raw, b"\r\n\r\n").ok_or_else(|| {
                DataError::Io(std::io::Error::other("response has no header terminator"))
            })?;
            let status_line = String::from_utf8_lossy(&raw[..split]);
            let status: u16 = status_line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| {
                    DataError::Io(std::io::Error::other("response has no status code"))
                })?;
            Ok(HttpResponse { status, body: raw[split + 4..].to_vec() })
        }
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    /// A transport that never reaches the wire — for the "is it reachable?"
    /// path.
    pub(crate) struct DeadTransport;

    impl HttpTransport for DeadTransport {
        fn send(&self, _request: &HttpRequest) -> Result<HttpResponse> {
            Err(DataError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "connection refused",
            )))
        }
    }

    /// A transport that records the request and replies from a script.
    pub(crate) struct RecordingTransport {
        pub(crate) response: HttpResponse,
        pub(crate) seen: Mutex<Vec<HttpRequest>>,
    }

    impl RecordingTransport {
        pub(crate) fn new(status: u16, body: &str) -> Self {
            Self {
                response: HttpResponse { status, body: body.as_bytes().to_vec() },
                seen: Mutex::new(Vec::new()),
            }
        }

        pub(crate) fn last(&self) -> HttpRequest {
            self.seen.lock().expect("recording mutex").last().cloned().expect("a request")
        }
    }

    impl HttpTransport for RecordingTransport {
        fn send(&self, request: &HttpRequest) -> Result<HttpResponse> {
            self.seen.lock().expect("recording mutex").push(request.clone());
            Ok(self.response.clone())
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::testing::{DeadTransport, RecordingTransport, StubServer, TcpTransport};
    use super::*;

    fn base() -> SourceConfig {
        SourceConfig::new(
            SourceKind::AzureBlob,
            "https://telemetry.blob.core.windows.net/runs/2026/08/readings.csv",
        )
    }

    fn err_of(config: &SourceConfig) -> String {
        validate(config).expect_err("expected a validation error").to_string()
    }

    /// An `azure://` config wired to a live stub server through Azurite-style
    /// path addressing.
    fn against(server: &StubServer, blob: &str, format: &str) -> SourceConfig {
        SourceConfig::new(SourceKind::AzureBlob, format!("azure://runs/{blob}"))
            .with_option("account", "devstoreaccount1")
            .with_option("emulator_base", server.base_url())
            .with_option("format", format)
    }

    // ── Happy paths ──────────────────────────────────────────────────────────

    #[test]
    fn a_public_url_resolves_account_container_blob_and_format() {
        let src = AzureBlobSource::new(base()).unwrap();
        let t = src.target();
        assert_eq!(t.account, "telemetry");
        assert_eq!(t.container, "runs");
        assert_eq!(t.blob, "2026/08/readings.csv");
        assert_eq!(t.format, BlobFormat::Csv);
        assert_eq!(t.auth, BlobAuth::Anonymous);
        assert_eq!(t.api_version, DEFAULT_API_VERSION);
        assert_eq!(
            t.blob_url(),
            "https://telemetry.blob.core.windows.net/runs/2026/08/readings.csv"
        );
        assert_eq!(src.kind(), SourceKind::AzureBlob);
    }

    #[test]
    fn the_short_form_needs_an_account_and_builds_the_same_url() {
        let c = SourceConfig::new(SourceKind::AzureBlob, "azure://runs/2026/08/readings.csv")
            .with_option("account", "telemetry");
        let t = AzureBlobSource::new(c).unwrap().target().clone();
        assert_eq!(
            t.blob_url(),
            "https://telemetry.blob.core.windows.net/runs/2026/08/readings.csv"
        );
    }

    #[test]
    fn an_emulator_base_switches_to_path_addressing() {
        let c = SourceConfig::new(SourceKind::AzureBlob, "azure://runs/readings.csv")
            .with_option("account", "devstoreaccount1")
            .with_option("emulator_base", "http://127.0.0.1:10000");
        let t = AzureBlobSource::new(c).unwrap().target().clone();
        assert_eq!(
            t.blob_url(),
            "http://127.0.0.1:10000/devstoreaccount1/runs/readings.csv"
        );
    }

    #[test]
    fn the_blob_may_come_from_the_key_option() {
        let c = SourceConfig::new(SourceKind::AzureBlob, "azure://runs")
            .with_option("account", "telemetry")
            .with_option("key", "nested/readings.jsonl");
        let t = AzureBlobSource::new(c).unwrap().target().clone();
        assert_eq!(t.blob, "nested/readings.jsonl");
        assert_eq!(t.format, BlobFormat::Jsonl);
    }

    #[test]
    fn every_format_token_round_trips() {
        for f in [BlobFormat::Csv, BlobFormat::Json, BlobFormat::Jsonl] {
            assert_eq!(BlobFormat::parse(f.as_str()), Some(f), "{f:?}");
        }
        assert_eq!(BlobFormat::parse("NDJSON"), Some(BlobFormat::Jsonl));
        assert_eq!(BlobFormat::parse("avro"), None);
    }

    #[test]
    fn every_auth_token_round_trips() {
        for a in [BlobAuth::Anonymous, BlobAuth::Sas, BlobAuth::Bearer] {
            assert_eq!(BlobAuth::parse(a.as_str()), Some(a), "{a:?}");
        }
        assert_eq!(BlobAuth::parse("nonsense"), None);
    }

    // ── Rejected shapes ──────────────────────────────────────────────────────

    #[test]
    fn a_non_azure_config_is_refused_by_name() {
        let c = SourceConfig::new(SourceKind::Rest, "https://api.example.com/x");
        assert!(err_of(&c).contains("REST"));
    }

    #[test]
    fn an_unrecognized_scheme_is_rejected() {
        let c = SourceConfig::new(SourceKind::AzureBlob, "abfss://runs@telemetry/x.csv");
        assert!(validate(&c).is_err());
    }

    #[test]
    fn the_short_form_without_an_account_is_rejected() {
        let c = SourceConfig::new(SourceKind::AzureBlob, "azure://runs/readings.csv");
        assert!(err_of(&c).contains("'account' option"));
    }

    #[test]
    fn an_account_named_twice_and_differently_is_rejected() {
        let c = base().with_option("account", "somethingelse");
        assert!(err_of(&c).contains("exactly one"));
    }

    #[test]
    fn a_url_whose_host_is_not_an_account_is_rejected() {
        let c = SourceConfig::new(SourceKind::AzureBlob, "https://localhost/runs/readings.csv");
        assert!(err_of(&c).contains("does not name a storage account"));
    }

    #[test]
    fn every_account_naming_rule_is_enforced() {
        for account in ["ab", &"a".repeat(25), "Telemetry", "tele-metry", "tele_metry"] {
            let c = SourceConfig::new(SourceKind::AzureBlob, "azure://runs/readings.csv")
                .with_option("account", account);
            assert!(validate(&c).is_err(), "account '{account}' accepted");
        }
    }

    #[test]
    fn every_container_naming_rule_is_enforced() {
        for container in ["ab", &"a".repeat(64), "Runs", "-runs", "runs-", "ru--ns", "run.s"] {
            let c = SourceConfig::new(
                SourceKind::AzureBlob,
                format!("azure://{container}/readings.csv"),
            )
            .with_option("account", "telemetry");
            assert!(validate(&c).is_err(), "container '{container}' accepted");
        }
    }

    #[test]
    fn a_malformed_blob_name_is_rejected() {
        for blob in ["with space.csv", "trailing/", "trailing.", "with?query.csv", "with#f.csv"] {
            let c = SourceConfig::new(SourceKind::AzureBlob, "azure://runs")
                .with_option("account", "telemetry")
                .with_option("key", blob)
                .with_option("format", "csv");
            assert!(validate(&c).is_err(), "blob '{blob}' accepted");
        }
    }

    #[test]
    fn naming_no_blob_at_all_is_rejected() {
        let c = SourceConfig::new(SourceKind::AzureBlob, "azure://runs")
            .with_option("account", "telemetry");
        assert!(err_of(&c).contains("names no blob"));
    }

    #[test]
    fn a_blob_named_twice_and_differently_is_rejected() {
        let c = base().with_option("key", "somewhere/else.csv");
        assert!(err_of(&c).contains("exactly one"));
    }

    #[test]
    fn parquet_is_rejected_with_the_reason_and_the_alternative() {
        let c = base().with_option("format", "parquet");
        let msg = err_of(&c);
        assert!(msg.contains("read_parquet"), "the alternative must be named: {msg}");
    }

    #[test]
    fn an_unknown_format_is_rejected_and_lists_the_known_ones() {
        let c = base().with_option("format", "avro");
        assert!(err_of(&c).contains("csv, json, jsonl"));
    }

    #[test]
    fn a_format_that_cannot_be_inferred_is_rejected() {
        let c = SourceConfig::new(SourceKind::AzureBlob, "azure://runs/latest")
            .with_option("account", "telemetry");
        assert!(err_of(&c).contains("cannot infer a format"));
    }

    #[test]
    fn a_secret_without_an_auth_mode_is_rejected() {
        let c = base().with_secret_ref("AZURE_SAS");
        assert!(err_of(&c).contains("'auth' option"));
    }

    #[test]
    fn sas_and_bearer_both_require_a_secret_ref() {
        for mode in ["sas", "bearer"] {
            let c = base().with_option("auth", mode);
            assert!(err_of(&c).contains("secret_ref"), "{mode}");
        }
    }

    #[test]
    fn anonymous_auth_with_a_secret_ref_is_rejected() {
        let c = base().with_option("auth", "anonymous").with_secret_ref("AZURE_SAS");
        assert!(err_of(&c).contains("anonymous"));
    }

    #[test]
    fn an_unknown_auth_mode_is_rejected() {
        let c = base().with_option("auth", "kerberos");
        assert!(err_of(&c).contains("anonymous, sas, bearer"));
    }

    #[test]
    fn a_malformed_api_version_is_rejected() {
        for v in ["2021-8-6", "latest", "2021/08/06", "20210806"] {
            let c = base().with_option("api_version", v);
            assert!(validate(&c).is_err(), "api_version '{v}' accepted");
        }
    }

    #[test]
    fn a_pinned_api_version_is_kept() {
        let c = base().with_option("api_version", "2023-11-03");
        assert_eq!(AzureBlobSource::new(c).unwrap().target().api_version, "2023-11-03");
    }

    #[test]
    fn a_malformed_emulator_base_is_rejected() {
        for v in ["127.0.0.1:10000", "http://127.0.0.1:10000/devstoreaccount1", "http://"] {
            let c = SourceConfig::new(SourceKind::AzureBlob, "azure://runs/readings.csv")
                .with_option("account", "devstoreaccount1")
                .with_option("emulator_base", v);
            assert!(validate(&c).is_err(), "emulator_base '{v}' accepted");
        }
    }

    #[test]
    fn plaintext_http_to_a_remote_emulator_is_rejected() {
        let c = SourceConfig::new(SourceKind::AzureBlob, "azure://runs/readings.csv")
            .with_option("account", "devstoreaccount1")
            .with_option("emulator_base", "http://storage.internal:10000");
        assert!(err_of(&c).contains("in the clear"));
    }

    #[test]
    fn an_emulator_base_alongside_a_https_endpoint_is_rejected() {
        let c = base().with_option("emulator_base", "http://127.0.0.1:10000");
        assert!(err_of(&c).contains("azure://container/blob"));
    }

    #[test]
    fn a_records_path_on_a_non_json_blob_is_rejected() {
        let c = base().with_option("records_path", "items");
        assert!(err_of(&c).contains("records path"));
    }

    #[test]
    fn a_bad_timeout_is_rejected() {
        for v in ["0", "-5", "soon"] {
            let c = base().with_option("timeout_seconds", v);
            assert!(validate(&c).is_err(), "timeout '{v}' accepted");
        }
    }

    #[test]
    fn the_shared_checks_still_apply() {
        let mut c = base();
        c.poll_seconds = Some(0);
        assert!(validate(&c).is_err());
    }

    // ── The request that would be sent ───────────────────────────────────────

    #[test]
    fn an_anonymous_request_carries_only_the_api_version() {
        let src = AzureBlobSource::new(base()).unwrap();
        let req = src.request(HttpMethod::Get).unwrap();
        assert_eq!(req.method, HttpMethod::Get);
        assert!(!req.url.contains('?'), "no credential in the query: {}", req.url);
        assert_eq!(req.headers, vec![("x-ms-version".to_string(), DEFAULT_API_VERSION.to_string())]);
    }

    #[test]
    fn a_sas_token_lands_in_the_query_and_a_bearer_token_in_the_header() {
        let sas_key = "EUSTRESS_TEST_AZURE_SAS";
        std::env::set_var(sas_key, "?sv=2021-08-06&sig=STUBSIG");
        let src = AzureBlobSource::new(
            base().with_option("auth", "sas").with_secret_ref(sas_key),
        )
        .unwrap();
        let req = src.request(HttpMethod::Get).unwrap();
        assert!(req.url.ends_with("?sv=2021-08-06&sig=STUBSIG"), "{}", req.url);
        std::env::remove_var(sas_key);

        let bearer_key = "EUSTRESS_TEST_AZURE_BEARER";
        std::env::set_var(bearer_key, "STUBTOKEN");
        let src = AzureBlobSource::new(
            base().with_option("auth", "bearer").with_secret_ref(bearer_key),
        )
        .unwrap();
        let req = src.request(HttpMethod::Get).unwrap();
        assert!(req
            .headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer STUBTOKEN"));
        std::env::remove_var(bearer_key);
    }

    #[test]
    fn an_unset_secret_is_reported_by_variable_name() {
        let key = "EUSTRESS_TEST_AZURE_MISSING";
        std::env::remove_var(key);
        let src =
            AzureBlobSource::new(base().with_option("auth", "sas").with_secret_ref(key)).unwrap();
        let msg = src.request(HttpMethod::Get).unwrap_err().to_string();
        assert!(msg.contains(key), "{msg}");
    }

    #[test]
    fn debug_never_prints_a_credential() {
        let req = HttpRequest {
            method: HttpMethod::Get,
            url: "https://a.blob.core.windows.net/c/b.csv?sig=SUPERSECRET".to_string(),
            headers: vec![
                ("x-ms-version".to_string(), DEFAULT_API_VERSION.to_string()),
                ("Authorization".to_string(), "Bearer SUPERSECRET".to_string()),
            ],
        };
        let rendered = format!("{req:?}");
        assert!(!rendered.contains("SUPERSECRET"), "credential leaked: {rendered}");
        assert!(rendered.contains("<redacted>"), "{rendered}");
        assert!(rendered.contains("x-ms-version"), "non-secret headers stay visible: {rendered}");
    }

    // ── The fetch path, against a real socket ────────────────────────────────

    #[test]
    fn with_no_transport_the_source_says_so_plainly() {
        let src = AzureBlobSource::new(base()).unwrap();
        let err = src.fetch().expect_err("no transport attached");
        let msg = err.to_string();
        assert!(msg.contains("Azure Blob"), "must name the provider: {msg}");
        assert!(msg.contains("with_transport"), "must name the remedy: {msg}");
        assert!(
            matches!(&err, DataError::Io(e) if e.kind() == std::io::ErrorKind::Unsupported),
            "a missing capability must not masquerade as a config error"
        );
        assert!(src.test_connection().is_err());
    }

    #[test]
    fn test_connection_reports_unreachable_rather_than_erroring() {
        let src = AzureBlobSource::new(base()).unwrap().with_transport(Arc::new(DeadTransport));
        let status = src.test_connection().expect("asking is possible; the answer is 'no'");
        assert!(!status.reachable);
        assert!(status.detail.contains("unreachable"), "{}", status.detail);
    }

    #[test]
    fn test_connection_issues_a_head_and_reads_the_status() {
        let server = StubServer::spawn(200, "text/csv", b"ignored".to_vec());
        let src = AzureBlobSource::new(against(&server, "readings.csv", "csv"))
            .unwrap()
            .with_transport(Arc::new(TcpTransport));

        let status = src.test_connection().unwrap();
        assert!(status.reachable, "{}", status.detail);

        let seen = server.requests();
        assert_eq!(seen.len(), 1);
        assert!(
            seen[0].starts_with("HEAD /devstoreaccount1/runs/readings.csv HTTP/1.1"),
            "a probe must not download the blob: {}",
            seen[0]
        );
        assert!(seen[0].contains("x-ms-version: 2021-08-06"), "{}", seen[0]);
    }

    #[test]
    fn a_404_is_a_reachable_answer_with_a_plain_language_hint() {
        let server = StubServer::spawn(404, "application/xml", b"<Error/>".to_vec());
        let src = AzureBlobSource::new(against(&server, "missing.csv", "csv"))
            .unwrap()
            .with_transport(Arc::new(TcpTransport));
        let status = src.test_connection().unwrap();
        assert!(!status.reachable);
        assert!(status.detail.contains("no such container or blob"), "{}", status.detail);
    }

    #[test]
    fn a_403_fetch_names_the_expired_sas_case() {
        let server = StubServer::spawn(403, "application/xml", b"<Error>nope</Error>".to_vec());
        let src = AzureBlobSource::new(against(&server, "readings.csv", "csv"))
            .unwrap()
            .with_transport(Arc::new(TcpTransport));
        let msg = src.fetch().unwrap_err().to_string();
        assert!(msg.contains("HTTP 403"), "{msg}");
        assert!(msg.contains("expired"), "{msg}");
        assert!(msg.contains("<Error>nope</Error>"), "the body helps diagnose: {msg}");
    }

    #[test]
    fn the_request_reaches_the_wire_with_its_headers_intact() {
        // Asserted independently of the `import` feature: this is about the
        // request, which is always built the same way.
        let server = StubServer::spawn(200, "text/csv", b"t,psi\n0,1\n".to_vec());
        let src = AzureBlobSource::new(against(&server, "readings.csv", "csv"))
            .unwrap()
            .with_transport(Arc::new(TcpTransport));
        let _ = src.fetch();
        let seen = server.requests();
        assert!(seen[0].starts_with("GET /devstoreaccount1/runs/readings.csv HTTP/1.1"), "{}", seen[0]);
        assert!(seen[0].contains("Host: 127.0.0.1:"), "{}", seen[0]);
    }

    #[test]
    fn a_recording_transport_sees_the_sas_query_on_the_real_url() {
        let key = "EUSTRESS_TEST_AZURE_SAS_WIRE";
        std::env::set_var(key, "sv=2021-08-06&sig=STUBSIG");
        let transport = Arc::new(RecordingTransport::new(200, "t,psi\n0,1\n"));
        let src = AzureBlobSource::new(
            base().with_option("auth", "sas").with_secret_ref(key),
        )
        .unwrap()
        .with_transport(transport.clone());
        let _ = src.fetch();
        assert!(transport.last().url.ends_with("?sv=2021-08-06&sig=STUBSIG"));
        std::env::remove_var(key);
    }

    #[cfg(not(feature = "import"))]
    #[test]
    fn without_the_import_feature_decoding_fails_honestly_after_a_good_response() {
        let server = StubServer::spawn(200, "text/csv", b"t,psi\n0,1\n".to_vec());
        let src = AzureBlobSource::new(against(&server, "readings.csv", "csv"))
            .unwrap()
            .with_transport(Arc::new(TcpTransport));
        let msg = src.fetch().unwrap_err().to_string();
        assert!(msg.contains("the response arrived"), "{msg}");
        assert!(msg.contains("'import' feature"), "{msg}");
    }

    #[cfg(feature = "import")]
    mod decoding {
        use super::*;
        use crate::ColumnDtype;

        #[test]
        fn a_csv_blob_becomes_a_typed_frame() {
            let body = b"t (s),psi\n0.0,14.7\n0.5,15.2\n".to_vec();
            let server = StubServer::spawn(200, "text/csv", body);
            let src = AzureBlobSource::new(against(&server, "readings.csv", "csv"))
                .unwrap()
                .with_transport(Arc::new(TcpTransport));

            let frame = src.fetch().expect("a 200 CSV must decode");
            assert_eq!(frame.n_rows(), 2);
            assert_eq!(frame.n_cols(), 2);
            let t = frame.specs().find(|s| s.name == "t").unwrap();
            assert_eq!(t.dtype, ColumnDtype::F64);
            assert_eq!(t.unit.as_deref(), Some("s"), "the header unit survives the fetch");
        }

        #[test]
        fn a_jsonl_blob_becomes_a_typed_frame() {
            let body = b"{\"t\":0,\"ok\":true}\n{\"t\":1,\"ok\":false}\n".to_vec();
            let server = StubServer::spawn(200, "application/x-ndjson", body);
            let src = AzureBlobSource::new(against(&server, "readings.jsonl", "jsonl"))
                .unwrap()
                .with_transport(Arc::new(TcpTransport));
            let frame = src.fetch().unwrap();
            assert_eq!(frame.n_rows(), 2);
            assert_eq!(frame.specs().find(|s| s.name == "ok").unwrap().dtype, ColumnDtype::Bool);
        }

        #[test]
        fn a_json_array_blob_becomes_a_typed_frame() {
            let body = br#"[{"t":0,"psi":14.7},{"t":1,"psi":15.2}]"#.to_vec();
            let server = StubServer::spawn(200, "application/json", body);
            let src = AzureBlobSource::new(against(&server, "readings.json", "json"))
                .unwrap()
                .with_transport(Arc::new(TcpTransport));
            let frame = src.fetch().unwrap();
            assert_eq!(frame.n_rows(), 2);
            assert_eq!(frame.specs().find(|s| s.name == "psi").unwrap().dtype, ColumnDtype::F64);
        }

        #[test]
        fn a_records_path_reaches_into_a_wrapped_document() {
            let body = br#"{"meta":{"n":2},"rows":[{"t":0},{"t":1}]}"#.to_vec();
            let server = StubServer::spawn(200, "application/json", body);
            let config = against(&server, "readings.json", "json").with_option("records_path", "rows");
            let src =
                AzureBlobSource::new(config).unwrap().with_transport(Arc::new(TcpTransport));
            assert_eq!(src.fetch().unwrap().n_rows(), 2);
        }

        #[test]
        fn a_missing_records_path_is_named_in_the_error() {
            let body = br#"{"rows":[{"t":0}]}"#.to_vec();
            let server = StubServer::spawn(200, "application/json", body);
            let config =
                against(&server, "readings.json", "json").with_option("records_path", "items");
            let src =
                AzureBlobSource::new(config).unwrap().with_transport(Arc::new(TcpTransport));
            let msg = src.fetch().unwrap_err().to_string();
            assert!(msg.contains("'items'"), "{msg}");
        }

        #[test]
        fn a_json_document_that_is_not_records_is_rejected() {
            let server = StubServer::spawn(200, "application/json", b"{\"t\":0}".to_vec());
            let src = AzureBlobSource::new(against(&server, "readings.json", "json"))
                .unwrap()
                .with_transport(Arc::new(TcpTransport));
            assert!(src.fetch().unwrap_err().to_string().contains("not a JSON array"));
        }

        #[test]
        fn a_body_that_is_not_json_at_all_is_rejected() {
            let server = StubServer::spawn(200, "application/json", b"<html/>".to_vec());
            let src = AzureBlobSource::new(against(&server, "readings.json", "json"))
                .unwrap()
                .with_transport(Arc::new(TcpTransport));
            assert!(src.fetch().unwrap_err().to_string().contains("not JSON"));
        }
    }
}

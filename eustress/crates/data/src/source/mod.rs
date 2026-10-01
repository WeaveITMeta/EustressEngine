//! Data sources — the seam between "where data comes from" and the columnar core.
//!
//! Every provider the Studio's Data menu offers (REST, GraphQL, PostgreSQL,
//! CSV/Excel, Firebase, Supabase, S3, Azure Blob, Oracle) implements
//! [`DataSource`]. That indirection is what makes the whole surface testable
//! without credentials or a network: a test can drive a real provider against a
//! local stub server, or substitute a fake implementation entirely.
//!
//! ## Design rules
//!
//! 1. **[`DataSource::validate`] never touches the network.** It is a pure
//!    function of the config, so every provider has meaningful unit tests from
//!    day one, in CI, with no infrastructure at all.
//! 2. **Secrets are never stored in config.** [`SourceConfig::secret_ref`] holds
//!    the *name* of an environment variable or credential-store key, never the
//!    value — so a Connector's `_instance.toml` can be committed to a repo
//!    without leaking anything, and tests never need real credentials.
//! 3. **Blocking, not async.** This crate is a pure leaf with no async runtime,
//!    and the engine consuming it is a synchronous ECS. Callers that need
//!    concurrency run `fetch` on a worker thread.
//! 4. **Everything normalizes to [`Frame`].** A provider's job is to produce the
//!    same columnar structure the rest of the platform already understands, so
//!    a REST payload and a CSV file are indistinguishable downstream.

use std::collections::BTreeMap;

use crate::{DataError, Frame, Result};

// Providers. Each implements [`DataSource`] for one entry in the Studio's Data
// menu. Modules that need a network client or a parser gate those parts
// internally, so this list stays unconditional and every provider's pure
// config validation is always compiled and always testable.
/// The single HTTP seam every remote provider shares.
pub mod http;

pub mod azure;
pub mod csv;
pub mod firebase;
pub mod graphql;
/// Neo4j + Amazon Neptune (Cypher over HTTP).
pub mod graphdb;
pub mod oracle;
pub mod postgres;
pub mod rest;
pub mod s3;
pub mod supabase;

/// Connector → Dataset materialization (the pure, engine-independent half).
pub mod materialize;

/// Reading a Connector: attributes to a fetched Frame, shared by every reader.
pub mod connector;

/// Which provider a config describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    Csv,
    Rest,
    GraphQl,
    Postgres,
    S3,
    Firebase,
    Supabase,
    AzureBlob,
    Oracle,
    /// Neo4j over its HTTP transaction API (Cypher).
    Neo4j,
    /// Amazon Neptune over its openCypher HTTP endpoint.
    Neptune,
}

impl SourceKind {
    /// Stable wire name — matches the string the Data menu sends and the
    /// `source_type` attribute persisted in a Connector's `_instance.toml`.
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceKind::Csv => "CSV",
            SourceKind::Rest => "REST",
            SourceKind::GraphQl => "GraphQL",
            SourceKind::Postgres => "PostgreSQL",
            SourceKind::S3 => "S3",
            SourceKind::Firebase => "Firebase",
            SourceKind::Supabase => "Supabase",
            SourceKind::AzureBlob => "AzureBlob",
            SourceKind::Oracle => "Oracle",
            SourceKind::Neo4j => "Neo4j",
            SourceKind::Neptune => "Neptune",
        }
    }

    /// Parse the wire name. Accepts a few friendly aliases the UI may send.
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim();
        let lowered = t.to_ascii_lowercase();
        Some(match lowered.as_str() {
            "csv" | "csv/excel" | "csv / excel file" | "excel" => SourceKind::Csv,
            "rest" | "http" | "http/rest" | "http / rest api" | "http_rest" => SourceKind::Rest,
            "graphql" => SourceKind::GraphQl,
            "postgresql" | "postgres" => SourceKind::Postgres,
            "s3" | "aws s3" | "aws_s3" => SourceKind::S3,
            "firebase" => SourceKind::Firebase,
            "supabase" => SourceKind::Supabase,
            "azureblob" | "azure blob" | "azure_blob" | "azure" => SourceKind::AzureBlob,
            "oracle" | "oracle cloud" => SourceKind::Oracle,
            "neo4j" => SourceKind::Neo4j,
            "neptune" | "aws neptune" | "amazon neptune" => SourceKind::Neptune,
            _ => return None,
        })
    }

    /// Every provider the Data menu offers, in menu order.
    pub const ALL: [SourceKind; 11] = [
        SourceKind::Rest,
        SourceKind::GraphQl,
        SourceKind::Postgres,
        SourceKind::Csv,
        SourceKind::Firebase,
        SourceKind::Supabase,
        SourceKind::S3,
        SourceKind::AzureBlob,
        SourceKind::Oracle,
        SourceKind::Neo4j,
        SourceKind::Neptune,
    ];

    /// Whether this provider reads from the local filesystem rather than a
    /// remote endpoint — the UI asks for a file path instead of a URL.
    pub fn is_local(&self) -> bool {
        matches!(self, SourceKind::Csv)
    }
}

/// Where a source's data lives, and how to reach it.
///
/// Deliberately provider-agnostic: the provider-specific knobs live in
/// [`options`](SourceConfig::options) so adding a provider never changes this
/// struct or the TOML schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceConfig {
    pub kind: SourceKind,
    /// URL, connection string, bucket URI, or local path depending on `kind`.
    pub endpoint: String,
    /// Provider-specific settings (e.g. `region`, `table`, `query`, `delimiter`).
    pub options: BTreeMap<String, String>,
    /// NAME of an env var / credential-store key holding the secret — never the
    /// secret itself. See the module docs.
    pub secret_ref: Option<String>,
    /// How often a live runtime should re-fetch. `None` = manual only.
    pub poll_seconds: Option<u64>,
    /// Inert until explicitly enabled, so creating a source can never start
    /// unattended network traffic.
    pub enabled: bool,
}

impl SourceConfig {
    pub fn new(kind: SourceKind, endpoint: impl Into<String>) -> Self {
        Self {
            kind,
            endpoint: endpoint.into(),
            options: BTreeMap::new(),
            secret_ref: None,
            poll_seconds: None,
            enabled: false,
        }
    }

    pub fn with_option(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.options.insert(k.into(), v.into());
        self
    }

    pub fn with_secret_ref(mut self, r: impl Into<String>) -> Self {
        self.secret_ref = Some(r.into());
        self
    }

    pub fn option(&self, k: &str) -> Option<&str> {
        self.options.get(k).map(String::as_str)
    }

    /// Resolve the secret from the environment at call time. Never cached, never
    /// persisted, never logged.
    pub fn resolve_secret(&self) -> Option<String> {
        self.secret_ref.as_ref().and_then(|k| std::env::var(k).ok())
    }
}

/// Result of a cheap liveness probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionStatus {
    pub reachable: bool,
    /// Human-readable detail for the UI — an error message, a server version,
    /// or a row count.
    pub detail: String,
}

impl ConnectionStatus {
    pub fn ok(detail: impl Into<String>) -> Self {
        Self { reachable: true, detail: detail.into() }
    }
    pub fn failed(detail: impl Into<String>) -> Self {
        Self { reachable: false, detail: detail.into() }
    }
}

/// A provider that can validate its configuration, probe liveness, and produce
/// a [`Frame`].
///
/// Object-safe: the Studio holds `Box<dyn DataSource>` in a registry.
/// Config validation is a separate free function per provider (see
/// [`validate_config`]) precisely so it can be called before any instance
/// exists — that is what makes the UI able to say "this config is wrong"
/// without attempting a connection.
pub trait DataSource: Send + Sync {
    fn kind(&self) -> SourceKind;

    /// Cheap liveness probe. Implementations MUST NOT fetch a full dataset.
    fn test_connection(&self) -> Result<ConnectionStatus>;

    /// Fetch and normalize into the columnar core.
    fn fetch(&self) -> Result<Frame>;
}

/// Build the provider a config names. Construction validates what each
/// provider needs; the network is touched only when the caller probes
/// ([`DataSource::test_connection`]) or fetches.
///
/// A provider with a secret comes back wrapped so that no text it returns (an
/// error, a Test result) carries the secret ([`scrub_secret`]), whichever path
/// produced the text. A secret shorter than [`MIN_SECRET_CHARS`] is refused
/// here, before anything is sent, because it could not be scrubbed.
pub fn open(config: SourceConfig) -> Result<Box<dyn DataSource>> {
    let Some(var) = config.secret_ref.clone() else {
        return provider(config);
    };
    if config.resolve_secret().is_some_and(|s| s.chars().count() < MIN_SECRET_CHARS) {
        return Err(DataError::Schema(format!(
            "the secret in `{var}` is shorter than {MIN_SECRET_CHARS} characters, so it is not sent: a value \
             that short cannot be kept out of error messages"
        )));
    }
    let inner = provider(config.clone())?;
    Ok(Box::new(Scrubbed { inner, config }))
}

/// The shortest secret Eustress sends. Scrubbing a shorter value out of text
/// would mangle ordinary words, so [`open`] refuses it instead.
pub const MIN_SECRET_CHARS: usize = 8;

/// `text` with `secret`, and the forms a server that echoes it is likely to
/// use (percent-encoded, base64, base64url), replaced by `<redacted>`. A
/// secret shorter than [`MIN_SECRET_CHARS`] leaves the text as it is.
pub fn scrub_secret(text: &str, secret: &str) -> String {
    if secret.chars().count() < MIN_SECRET_CHARS {
        return text.to_string();
    }
    const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let upper = percent_encoded(secret, true);
    let mut forms = vec![
        secret.to_string(),
        percent_encoded(secret, false),
        upper.replace("%20", "+"),
        upper,
        base64(secret.as_bytes(), STANDARD, true),
        base64(secret.as_bytes(), STANDARD, false),
        base64(secret.as_bytes(), URL_SAFE, true),
        base64(secret.as_bytes(), URL_SAFE, false),
    ];
    // Longest first, so a form that contains another is replaced whole.
    forms.sort_by_key(|form| std::cmp::Reverse(form.len()));
    forms.dedup();
    let mut out = text.to_string();
    for form in forms {
        out = out.replace(&form, "<redacted>");
    }
    out
}

/// RFC 3986 percent-encoding, unreserved characters kept.
fn percent_encoded(text: &str, upper: bool) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else if upper {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push_str(&format!("%{b:02x}"));
        }
    }
    out
}

fn base64(bytes: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let v = (u32::from(chunk[0]) << 16)
            | (u32::from(chunk.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        for i in 0..4 {
            if i <= n {
                out.push(alphabet[((v >> (18 - 6 * i)) & 63) as usize] as char);
            } else if pad {
                out.push('=');
            }
        }
    }
    out
}

/// A provider whose every outgoing text has its secret scrubbed. The secret is
/// read again from the environment when a text is scrubbed, never kept.
struct Scrubbed {
    inner: Box<dyn DataSource>,
    config: SourceConfig,
}

impl Scrubbed {
    fn scrub(&self, text: &str) -> String {
        match self.config.resolve_secret() {
            Some(secret) => scrub_secret(text, &secret),
            None => text.to_string(),
        }
    }

    fn scrub_error(&self, e: DataError) -> DataError {
        match e {
            DataError::Io(io) => {
                let text = io.to_string();
                let clean = self.scrub(&text);
                if clean == text {
                    DataError::Io(io)
                } else {
                    DataError::Io(std::io::Error::new(io.kind(), clean))
                }
            }
            DataError::Parquet(m) => DataError::Parquet(self.scrub(&m)),
            DataError::Arrow(m) => DataError::Arrow(self.scrub(&m)),
            DataError::Schema(m) => DataError::Schema(self.scrub(&m)),
        }
    }
}

impl DataSource for Scrubbed {
    fn kind(&self) -> SourceKind {
        self.inner.kind()
    }

    fn test_connection(&self) -> Result<ConnectionStatus> {
        match self.inner.test_connection() {
            Ok(status) => Ok(ConnectionStatus { detail: self.scrub(&status.detail), ..status }),
            Err(e) => Err(self.scrub_error(e)),
        }
    }

    fn fetch(&self) -> Result<Frame> {
        self.inner.fetch().map_err(|e| self.scrub_error(e))
    }
}

/// The provider a config names, as built (see [`open`]).
fn provider(config: SourceConfig) -> Result<Box<dyn DataSource>> {
    Ok(match config.kind {
        SourceKind::Csv => Box::new(csv::CsvSource::new(config)),
        SourceKind::Postgres => Box::new(postgres::PostgresSource::new(config)?),
        SourceKind::S3 => Box::new(s3::S3Source::new(config)?),
        // These providers parse JSON over the HTTP seam, so their modules
        // exist only with `import`. Each arm is gated with its module, and the
        // catch-all below stands in for them when the feature is off: a build
        // such as eustress-data-store's (`parquet` only) must still compile,
        // and a request for one of these providers must say what is missing.
        #[cfg(feature = "import")]
        SourceKind::Rest => Box::new(rest::RestSource::new(config)?),
        #[cfg(feature = "import")]
        SourceKind::GraphQl => Box::new(graphql::GraphQlSource::new(config)?),
        #[cfg(feature = "import")]
        SourceKind::Firebase => Box::new(firebase::FirebaseSource::new(config)?),
        #[cfg(feature = "import")]
        SourceKind::Supabase => Box::new(supabase::SupabaseSource::new(config)?),
        #[cfg(feature = "import")]
        SourceKind::AzureBlob => Box::new(azure::AzureBlobSource::new(config)?),
        #[cfg(feature = "import")]
        SourceKind::Oracle => Box::new(oracle::OracleSource::new(config)?),
        #[cfg(feature = "import")]
        SourceKind::Neo4j => Box::new(graphdb::Neo4jSource::new(config)?),
        #[cfg(feature = "import")]
        SourceKind::Neptune => Box::new(graphdb::NeptuneSource::new(config)?),
        #[cfg(not(feature = "import"))]
        other => {
            return Err(DataError::Schema(format!(
                "{} sources need the `import` feature of eustress-data, which this build does not enable",
                other.as_str()
            )))
        }
    })
}

/// Validate a config for its provider without touching the network.
///
/// Pure: same input always yields the same answer, in CI, offline. This is the
/// function every provider's unit tests exercise first.
pub fn validate_config(config: &SourceConfig) -> Result<()> {
    if config.endpoint.trim().is_empty() {
        return Err(DataError::Schema(format!(
            "{} source requires a non-empty {}",
            config.kind.as_str(),
            if config.kind.is_local() { "file path" } else { "endpoint" }
        )));
    }

    // Remote providers need a URL-ish endpoint; a local file must not be one.
    if config.kind.is_local() {
        if config.endpoint.starts_with("http://") || config.endpoint.starts_with("https://") {
            return Err(DataError::Schema(
                "CSV source expects a local file path, not a URL".into(),
            ));
        }
    } else if !looks_like_endpoint(&config.endpoint, config.kind) {
        return Err(DataError::Schema(format!(
            "{} endpoint '{}' is not a valid endpoint for this provider",
            config.kind.as_str(),
            config.endpoint
        )));
    }

    if let Some(p) = config.poll_seconds {
        if p == 0 {
            return Err(DataError::Schema(
                "poll_seconds must be greater than zero (omit it for manual-only)".into(),
            ));
        }
    }

    // A secret_ref must name a key, never contain the secret. Reject anything
    // that looks like a literal credential so a bad config fails loudly at
    // author time rather than silently persisting a secret to disk.
    if let Some(r) = &config.secret_ref {
        if r.trim().is_empty() {
            return Err(DataError::Schema("secret_ref must not be empty".into()));
        }
        if r.len() > 128 || r.contains(char::is_whitespace) {
            return Err(DataError::Schema(
                "secret_ref must be the NAME of an env var or credential key, not the secret value"
                    .into(),
            ));
        }
    }

    provider_requirements(config)
}

/// Endpoint shape check, per provider family.
fn looks_like_endpoint(endpoint: &str, kind: SourceKind) -> bool {
    match kind {
        SourceKind::Postgres => {
            endpoint.starts_with("postgres://") || endpoint.starts_with("postgresql://")
        }
        SourceKind::S3 => endpoint.starts_with("s3://") || endpoint.starts_with("https://"),
        SourceKind::AzureBlob => {
            endpoint.starts_with("https://") || endpoint.starts_with("azure://")
        }
        SourceKind::Csv => true,
        // Neo4j is commonly written with its native schemes. They are accepted
        // here so a familiar connection string is not rejected as malformed;
        // the provider explains at connect time that reads use the HTTP API.
        SourceKind::Neo4j => {
            endpoint.starts_with("http://")
                || endpoint.starts_with("https://")
                || endpoint.starts_with("bolt://")
                || endpoint.starts_with("neo4j://")
                || endpoint.starts_with("neo4j+s://")
        }
        // REST / GraphQL / Firebase / Supabase / Oracle / Neptune are HTTP(S).
        _ => endpoint.starts_with("http://") || endpoint.starts_with("https://"),
    }
}

/// Provider-specific required options.
fn provider_requirements(config: &SourceConfig) -> Result<()> {
    let missing = |field: &str| -> DataError {
        DataError::Schema(format!(
            "{} source requires the '{}' option",
            config.kind.as_str(),
            field
        ))
    };
    match config.kind {
        SourceKind::GraphQl => {
            if config.option("query").map(str::trim).unwrap_or("").is_empty() {
                return Err(missing("query"));
            }
        }
        SourceKind::Postgres => {
            let has_table = config.option("table").map(str::trim).is_some_and(|s| !s.is_empty());
            let has_query = config.option("query").map(str::trim).is_some_and(|s| !s.is_empty());
            if !has_table && !has_query {
                return Err(DataError::Schema(
                    "PostgreSQL source requires either a 'table' or a 'query' option".into(),
                ));
            }
        }
        SourceKind::S3 | SourceKind::AzureBlob => {
            if config.option("key").map(str::trim).unwrap_or("").is_empty()
                && !config.endpoint.contains('/')
            {
                return Err(missing("key"));
            }
        }
        // A graph database is queried, never listed: without a query there is
        // no meaningful default, and returning "the whole graph" from a supply
        // chain would be a denial of service against your own warehouse.
        SourceKind::Neo4j | SourceKind::Neptune => {
            if config.option("query").map(str::trim).unwrap_or("").is_empty() {
                return Err(missing("query"));
            }
        }
        SourceKind::Firebase | SourceKind::Supabase => {
            if config.option("table").map(str::trim).unwrap_or("").is_empty()
                && config.option("collection").map(str::trim).unwrap_or("").is_empty()
            {
                return Err(DataError::Schema(format!(
                    "{} source requires a 'table' or 'collection' option",
                    config.kind.as_str()
                )));
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(kind: SourceKind, endpoint: &str) -> SourceConfig {
        SourceConfig::new(kind, endpoint)
    }

    #[test]
    fn base64_matches_the_standard_vectors() {
        const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        for (raw, padded) in [("Man", "TWFu"), ("Ma", "TWE="), ("M", "TQ=="), ("foobar", "Zm9vYmFy"), ("fooba", "Zm9vYmE=")] {
            assert_eq!(base64(raw.as_bytes(), STANDARD, true), padded);
            assert_eq!(base64(raw.as_bytes(), STANDARD, false), padded.trim_end_matches('='));
        }
    }

    #[test]
    fn scrub_secret_removes_the_value_and_its_encodings() {
        let secret = "s3cr3t/Key+v@l?ue";
        let echoes = [
            secret.to_string(),
            "s3cr3t%2FKey%2Bv%40l%3Fue".to_string(),
            "s3cr3t%2fKey%2bv%40l%3fue".to_string(),
            "czNjcjN0L0tleSt2QGw/dWU=".to_string(),
            "czNjcjN0L0tleSt2QGw_dWU".to_string(),
        ];
        for echo in echoes {
            let text = format!("HTTP 401: invalid key '{echo}' for this project");
            let clean = scrub_secret(&text, secret);
            assert_eq!(clean, "HTTP 401: invalid key '<redacted>' for this project", "{echo}");
        }
        assert_eq!(scrub_secret("ordinary words", secret), "ordinary words");
        // A short value is never scrubbed (it would mangle text); open refuses it.
        assert_eq!(scrub_secret("the token is abc", "abc"), "the token is abc");
    }

    /// A provider that echoes its secret everywhere it can.
    struct Echo(String);

    impl DataSource for Echo {
        fn kind(&self) -> SourceKind {
            SourceKind::Rest
        }
        fn test_connection(&self) -> Result<ConnectionStatus> {
            Ok(ConnectionStatus::failed(format!("HTTP 401: key {} rejected", self.0)))
        }
        fn fetch(&self) -> Result<Frame> {
            Err(DataError::Io(std::io::Error::other(format!("GET failed: key={}", self.0))))
        }
    }

    #[test]
    fn no_text_leaving_a_provider_carries_its_secret() {
        let var = "EUSTRESS_TEST_SCRUB_SECRET";
        std::env::set_var(var, "Sup3rSecretValue");
        let config = cfg(SourceKind::Rest, "https://api.example.com/x").with_secret_ref(var);
        let source = Scrubbed { inner: Box::new(Echo("Sup3rSecretValue".into())), config };
        let status = source.test_connection().unwrap();
        assert_eq!(status.detail, "HTTP 401: key <redacted> rejected");
        assert!(!status.reachable);
        let e = source.fetch().unwrap_err().to_string();
        assert!(!e.contains("Sup3rSecretValue") && e.contains("key=<redacted>"), "{e}");
        std::env::remove_var(var);
    }

    #[test]
    fn a_secret_too_short_to_scrub_is_never_sent() {
        let var = "EUSTRESS_TEST_SHORT_SECRET";
        std::env::set_var(var, "abc1234");
        let config = cfg(SourceKind::Rest, "https://api.example.com/x").with_secret_ref(var);
        let e = open(config).err().expect("a 7-character secret is refused").to_string();
        assert!(e.contains("shorter than 8 characters") && e.contains(var), "{e}");
        std::env::remove_var(var);
    }

    #[test]
    fn every_kind_round_trips_its_wire_name() {
        for k in SourceKind::ALL {
            assert_eq!(SourceKind::parse(k.as_str()), Some(k), "{k:?}");
        }
    }

    #[test]
    fn menu_labels_parse_to_the_right_kind() {
        assert_eq!(SourceKind::parse("HTTP / REST API"), Some(SourceKind::Rest));
        assert_eq!(SourceKind::parse("CSV / Excel File"), Some(SourceKind::Csv));
        assert_eq!(SourceKind::parse("AWS S3"), Some(SourceKind::S3));
        assert_eq!(SourceKind::parse("Azure Blob"), Some(SourceKind::AzureBlob));
        assert_eq!(SourceKind::parse("Oracle Cloud"), Some(SourceKind::Oracle));
        assert_eq!(SourceKind::parse("not-a-provider"), None);
    }

    #[test]
    fn empty_endpoint_is_rejected_for_every_provider() {
        for k in SourceKind::ALL {
            assert!(validate_config(&cfg(k, "  ")).is_err(), "{k:?} accepted empty endpoint");
        }
    }

    #[test]
    fn csv_rejects_a_url_and_accepts_a_path() {
        assert!(validate_config(&cfg(SourceKind::Csv, "https://example.com/a.csv")).is_err());
        assert!(validate_config(&cfg(SourceKind::Csv, "data/readings.csv")).is_ok());
    }

    #[test]
    fn postgres_requires_a_postgres_scheme() {
        assert!(validate_config(&cfg(SourceKind::Postgres, "https://db.example.com")).is_err());
        let ok = cfg(SourceKind::Postgres, "postgres://host/db").with_option("table", "readings");
        assert!(validate_config(&ok).is_ok());
    }

    #[test]
    fn postgres_requires_table_or_query() {
        let bare = cfg(SourceKind::Postgres, "postgres://host/db");
        assert!(validate_config(&bare).is_err());
        let with_query = bare.clone().with_option("query", "select 1");
        assert!(validate_config(&with_query).is_ok());
    }

    #[test]
    fn graphql_requires_a_query() {
        let bare = cfg(SourceKind::GraphQl, "https://api.example.com/graphql");
        assert!(validate_config(&bare).is_err());
        assert!(validate_config(&bare.with_option("query", "{ me { id } }")).is_ok());
    }

    #[test]
    fn firebase_and_supabase_require_a_collection_or_table() {
        for k in [SourceKind::Firebase, SourceKind::Supabase] {
            let bare = cfg(k, "https://x.example.com");
            assert!(validate_config(&bare).is_err(), "{k:?}");
            assert!(validate_config(&bare.with_option("table", "readings")).is_ok(), "{k:?}");
        }
    }

    #[test]
    fn zero_poll_seconds_is_rejected() {
        let mut c = cfg(SourceKind::Rest, "https://api.example.com/x");
        c.poll_seconds = Some(0);
        assert!(validate_config(&c).is_err());
        c.poll_seconds = Some(30);
        assert!(validate_config(&c).is_ok());
    }

    #[test]
    fn secret_ref_must_be_a_key_name_not_a_secret_value() {
        let base = cfg(SourceKind::Rest, "https://api.example.com/x");
        assert!(validate_config(&base.clone().with_secret_ref("MY_API_TOKEN")).is_ok());
        // A pasted bearer token has whitespace / excessive length — reject it so
        // a secret never reaches disk.
        assert!(validate_config(&base.clone().with_secret_ref("Bearer abc.def")).is_err());
        assert!(validate_config(&base.with_secret_ref("")).is_err());
    }

    #[test]
    fn a_source_is_inert_until_explicitly_enabled() {
        let c = cfg(SourceKind::Rest, "https://api.example.com/x");
        assert!(!c.enabled, "a newly created source must never start enabled");
        assert!(c.poll_seconds.is_none(), "a new source must not poll by default");
    }

    #[test]
    fn secrets_are_never_stored_in_the_config_itself() {
        let c = cfg(SourceKind::Rest, "https://api.example.com/x").with_secret_ref("TOKEN_KEY");
        let rendered = format!("{c:?}");
        assert!(rendered.contains("TOKEN_KEY"));
        // resolve_secret reads the env at call time; with the var unset there is
        // nothing to leak.
        std::env::remove_var("TOKEN_KEY");
        assert_eq!(c.resolve_secret(), None);
    }
}

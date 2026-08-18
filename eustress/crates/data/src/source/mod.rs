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
pub mod azure;
pub mod csv;
pub mod firebase;
pub mod graphql;
pub mod oracle;
pub mod postgres;
pub mod rest;
pub mod s3;
pub mod supabase;

/// Connector → Dataset materialization (the pure, engine-independent half).
pub mod materialize;

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
            _ => return None,
        })
    }

    /// Every provider the Data menu offers, in menu order.
    pub const ALL: [SourceKind; 9] = [
        SourceKind::Rest,
        SourceKind::GraphQl,
        SourceKind::Postgres,
        SourceKind::Csv,
        SourceKind::Firebase,
        SourceKind::Supabase,
        SourceKind::S3,
        SourceKind::AzureBlob,
        SourceKind::Oracle,
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
        // REST / GraphQL / Firebase / Supabase / Oracle are all HTTP(S).
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

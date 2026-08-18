//! PostgreSQL source — the Data menu's "PostgreSQL" provider.
//!
//! ## Why this module links no client
//!
//! Every maintained PostgreSQL client for Rust drags in something this crate
//! must not have: `tokio-postgres` and `sqlx` pull a Tokio runtime, and the
//! `libpq` bindings need a C toolchain. `eustress-data` is a dependency-free
//! leaf (`DATA_PLATFORM_PLAN.md` invariant **D2**) and the engine consuming it
//! is a synchronous ECS, so neither is acceptable here. A `postgres` feature
//! that turns the wire protocol on can be added later without changing one line
//! of the surface below.
//!
//! What this module does instead is everything that *can* be done without a
//! client, and it does it completely: it parses the connection URI, rejects
//! every malformed or unsafe shape, resolves the exact statement that would be
//! run, and — only then — returns a [`DataError`] that names the missing
//! feature and shows the resolved target. The UI can therefore tell the user
//! the truth ("this build cannot reach PostgreSQL, but your config is correct
//! and this is the query it would run") rather than failing mysteriously.
//!
//! ## Secrets
//!
//! [`SourceConfig::secret_ref`] names the environment variable holding the
//! password; the password is read at call time and never stored. A password
//! embedded in the connection URI is a hard validation error — that is the one
//! shape which would otherwise persist a live credential to a Connector's
//! `_instance.toml`.

use std::fmt;

use super::{ConnectionStatus, DataSource, SourceConfig, SourceKind};
use crate::{DataError, Frame, Result};

/// The port a PostgreSQL server listens on when the URI omits one.
pub const DEFAULT_PORT: u16 = 5432;

/// The `sslmode` values libpq accepts, in increasing strictness.
pub const SSL_MODES: [&str; 6] =
    ["disable", "allow", "prefer", "require", "verify-ca", "verify-full"];

/// The `sslmode` assumed when neither the URI nor the options name one.
const DEFAULT_SSL_MODE: &str = "prefer";

/// Cargo feature that would compile a real wire-protocol client in.
const FEATURE: &str = "postgres";

// ── Resolved target ──────────────────────────────────────────────────────────

/// What the source will read: a whole relation, or an author-supplied statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostgresQuery {
    /// A relation name, optionally schema-qualified (`public.readings`).
    Table(String),
    /// A read-only SQL statement supplied by the author.
    Sql(String),
}

/// A fully resolved connection target — everything a client would need, parsed
/// out of the URI and options with no network access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresTarget {
    /// Host name or IP literal (IPv6 without its brackets).
    pub host: String,
    /// TCP port; [`DEFAULT_PORT`] when the URI omits one.
    pub port: u16,
    /// Database name (the URI path).
    pub database: String,
    /// Role name from the URI userinfo, if present.
    pub user: Option<String>,
    /// Effective `sslmode`.
    pub sslmode: String,
    /// What to read.
    pub query: PostgresQuery,
    /// Row cap applied to a `table` read. Never set alongside `query`.
    pub limit: Option<u64>,
}

impl PostgresTarget {
    /// The exact statement this source would execute.
    ///
    /// Pure and offline, so the UI can show the author the real SQL before
    /// anything is connected — and so a test can assert on it.
    pub fn resolved_sql(&self) -> String {
        match &self.query {
            PostgresQuery::Sql(q) => q.trim().trim_end_matches(';').trim().to_string(),
            PostgresQuery::Table(t) => match self.limit {
                Some(n) => format!("SELECT * FROM {t} LIMIT {n}"),
                None => format!("SELECT * FROM {t}"),
            },
        }
    }
}

impl fmt::Display for PostgresTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}/{}", self.host, self.port, self.database)
    }
}

// ── Source ───────────────────────────────────────────────────────────────────

/// The PostgreSQL provider.
///
/// Construction validates; a `PostgresSource` that exists is a config the
/// platform has fully understood.
#[derive(Debug, Clone)]
pub struct PostgresSource {
    config: SourceConfig,
    target: PostgresTarget,
}

impl PostgresSource {
    /// Validate `config` and resolve its target.
    pub fn new(config: SourceConfig) -> Result<Self> {
        validate(&config)?;
        let target = resolve_target(&config)?;
        Ok(Self { config, target })
    }

    /// The config this source was built from.
    pub fn config(&self) -> &SourceConfig {
        &self.config
    }

    /// The resolved connection target and statement.
    pub fn target(&self) -> &PostgresTarget {
        &self.target
    }

    /// The password, read from the environment variable named by
    /// [`SourceConfig::secret_ref`], at call time. Never cached or logged.
    pub fn password(&self) -> Option<String> {
        self.config.resolve_secret()
    }
}

impl DataSource for PostgresSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Postgres
    }

    fn test_connection(&self) -> Result<ConnectionStatus> {
        Err(unsupported(&self.target, "probe"))
    }

    fn fetch(&self) -> Result<Frame> {
        Err(unsupported(&self.target, "read"))
    }
}

/// The one place this module admits it cannot reach a server.
///
/// [`std::io::ErrorKind::Unsupported`] is deliberate: it lets a caller tell
/// "this build cannot do it" apart from [`DataError::Schema`], which always
/// means "your configuration is wrong and you can fix it". The message carries
/// the resolved target so the UI can prove the config itself is sound.
fn unsupported(target: &PostgresTarget, verb: &str) -> DataError {
    DataError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!(
            "PostgreSQL support requires the '{FEATURE}' feature, which is not compiled in — \
             eustress-data links no PostgreSQL client (every maintained one pulls a Tokio \
             runtime or a C toolchain). The config is valid: it would {verb} {target} with \
             `{}`.",
            target.resolved_sql()
        ),
    ))
}

// ── Validation (pure, offline) ───────────────────────────────────────────────

/// Validate a PostgreSQL config completely, without touching the network.
///
/// Runs the shared [`super::validate_config`] checks first, then everything
/// specific to this provider: URI shape, embedded-credential rejection, port
/// range, relation naming, statement read-onlyness, `sslmode` agreement, and
/// option coherence.
pub fn validate(config: &SourceConfig) -> Result<()> {
    if config.kind != SourceKind::Postgres {
        return Err(DataError::Schema(format!(
            "PostgreSQL validation was handed a {} config",
            config.kind.as_str()
        )));
    }
    super::validate_config(config)?;
    resolve_target(config).map(|_| ())
}

fn schema_err(msg: impl Into<String>) -> DataError {
    DataError::Schema(msg.into())
}

/// Parse the config into a [`PostgresTarget`], surfacing every malformed shape
/// as a [`DataError::Schema`]. This is where validation actually lives — a
/// config that resolves is a config that is correct.
fn resolve_target(config: &SourceConfig) -> Result<PostgresTarget> {
    let endpoint = config.endpoint.trim();
    let rest = endpoint
        .strip_prefix("postgresql://")
        .or_else(|| endpoint.strip_prefix("postgres://"))
        .ok_or_else(|| {
            schema_err(format!(
                "PostgreSQL endpoint '{endpoint}' must start with postgres:// or postgresql://"
            ))
        })?;

    // URI query string (`?sslmode=require&…`) is split off first.
    let (authority_path, uri_query) = match rest.split_once('?') {
        Some((a, q)) => (a, Some(q)),
        None => (rest, None),
    };

    let (authority, path) = match authority_path.split_once('/') {
        Some((a, p)) => (a, p),
        None => (authority_path, ""),
    };

    // Userinfo. A password here would be persisted verbatim into the Connector
    // TOML, which is exactly what `secret_ref` exists to prevent.
    let (user, hostport) = match authority.rsplit_once('@') {
        Some((userinfo, hp)) => {
            if userinfo.contains(':') {
                return Err(schema_err(
                    "PostgreSQL endpoint embeds a password — put the password in an environment \
                     variable and name it in secret_ref instead (postgres://user@host/db)",
                ));
            }
            if userinfo.trim().is_empty() {
                return Err(schema_err(
                    "PostgreSQL endpoint has an empty user before '@'".to_string(),
                ));
            }
            (Some(userinfo.to_string()), hp)
        }
        None => (None, authority),
    };

    let (host, port) = split_host_port(hostport)?;
    if host.is_empty() {
        return Err(schema_err(format!(
            "PostgreSQL endpoint '{endpoint}' has no host"
        )));
    }

    let database = path.trim();
    if database.is_empty() {
        return Err(schema_err(format!(
            "PostgreSQL endpoint '{endpoint}' names no database (expected postgres://host/database)"
        )));
    }
    if database.contains('/') {
        return Err(schema_err(format!(
            "PostgreSQL endpoint '{endpoint}' has extra path segments after the database name"
        )));
    }

    let sslmode = resolve_sslmode(config, uri_query)?;
    let query = resolve_query(config)?;
    let limit = resolve_limit(config, &query)?;

    Ok(PostgresTarget { host, port, database: database.to_string(), user, sslmode, query, limit })
}

/// Split `host:port`, tolerating a bracketed IPv6 literal.
fn split_host_port(hostport: &str) -> Result<(String, u16)> {
    let (host, port_str) = if let Some(after_bracket) = hostport.strip_prefix('[') {
        let (inside, tail) = after_bracket.split_once(']').ok_or_else(|| {
            schema_err(format!(
                "PostgreSQL endpoint host '[{after_bracket}' is missing its closing ']'"
            ))
        })?;
        match tail.strip_prefix(':') {
            Some(p) => (inside.to_string(), Some(p)),
            None if tail.is_empty() => (inside.to_string(), None),
            None => {
                return Err(schema_err(format!(
                    "PostgreSQL endpoint has trailing '{tail}' after the IPv6 host"
                )))
            }
        }
    } else {
        match hostport.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (hostport.to_string(), None),
        }
    };

    let port = match port_str {
        None => DEFAULT_PORT,
        Some(p) => {
            let n: u16 = p.parse().map_err(|_| {
                schema_err(format!("PostgreSQL port '{p}' is not a number in 1..=65535"))
            })?;
            if n == 0 {
                return Err(schema_err("PostgreSQL port must not be 0"));
            }
            n
        }
    };
    Ok((host, port))
}

/// Effective `sslmode`, rejecting a URI and an option that disagree — silently
/// preferring one would decide the security posture behind the author's back.
fn resolve_sslmode(config: &SourceConfig, uri_query: Option<&str>) -> Result<String> {
    let from_uri = uri_query.and_then(|q| {
        q.split('&')
            .filter_map(|kv| kv.split_once('='))
            .find(|(k, _)| k.eq_ignore_ascii_case("sslmode"))
            .map(|(_, v)| v.trim().to_ascii_lowercase())
    });
    let from_option = config.option("sslmode").map(|v| v.trim().to_ascii_lowercase());

    let mode = match (&from_uri, &from_option) {
        (Some(a), Some(b)) if a != b => {
            return Err(schema_err(format!(
                "PostgreSQL sslmode disagrees: the endpoint says '{a}' and the 'sslmode' option \
                 says '{b}' — set exactly one"
            )))
        }
        (Some(a), _) => a.clone(),
        (None, Some(b)) => b.clone(),
        (None, None) => DEFAULT_SSL_MODE.to_string(),
    };

    if !SSL_MODES.contains(&mode.as_str()) {
        return Err(schema_err(format!(
            "PostgreSQL sslmode '{mode}' is not one of {}",
            SSL_MODES.join(", ")
        )));
    }
    Ok(mode)
}

/// Resolve `table` XOR `query`.
fn resolve_query(config: &SourceConfig) -> Result<PostgresQuery> {
    let table = non_empty(config.option("table"));
    let sql = non_empty(config.option("query"));

    match (table, sql) {
        (Some(_), Some(_)) => Err(schema_err(
            "PostgreSQL source sets both 'table' and 'query' — set exactly one, so which \
             statement runs is never ambiguous",
        )),
        (Some(t), None) => {
            validate_relation(t)?;
            Ok(PostgresQuery::Table(t.to_string()))
        }
        (None, Some(q)) => {
            validate_read_only(q)?;
            Ok(PostgresQuery::Sql(q.to_string()))
        }
        // Unreachable via `validate`, which runs the shared check first; kept so
        // `resolve_target` is sound when called directly.
        (None, None) => Err(schema_err(
            "PostgreSQL source requires either a 'table' or a 'query' option",
        )),
    }
}

fn resolve_limit(config: &SourceConfig, query: &PostgresQuery) -> Result<Option<u64>> {
    let Some(raw) = non_empty(config.option("limit")) else {
        return Ok(None);
    };
    let n: u64 = raw
        .parse()
        .map_err(|_| schema_err(format!("PostgreSQL 'limit' option '{raw}' is not a whole number")))?;
    if n == 0 {
        return Err(schema_err("PostgreSQL 'limit' option must be greater than zero"));
    }
    if matches!(query, PostgresQuery::Sql(_)) {
        return Err(schema_err(
            "PostgreSQL 'limit' cannot be combined with 'query' — put the LIMIT in the statement \
             so the SQL that runs is the SQL the author wrote",
        ));
    }
    Ok(Some(n))
}

fn non_empty(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|s| !s.is_empty())
}

/// A relation name: `table` or `schema.table`, each an unquoted SQL identifier.
///
/// The relation is interpolated into `SELECT * FROM …`, so anything outside
/// this grammar is refused rather than escaped.
fn validate_relation(relation: &str) -> Result<()> {
    let parts: Vec<&str> = relation.split('.').collect();
    if parts.len() > 2 {
        return Err(schema_err(format!(
            "PostgreSQL 'table' option '{relation}' has too many parts (expected table or \
             schema.table)"
        )));
    }
    for part in parts {
        if !is_identifier(part) {
            return Err(schema_err(format!(
                "PostgreSQL 'table' option '{relation}' is not a plain identifier — expected \
                 letters, digits and underscores, optionally schema-qualified"
            )));
        }
    }
    Ok(())
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Reject statements that are obviously not a single read.
///
/// This is a config sanity check, not a security boundary: a source is a read
/// path, so a config asking it to `DELETE` is a mistake worth catching at
/// author time. Real authorization belongs to the database role.
fn validate_read_only(sql: &str) -> Result<()> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if trimmed.is_empty() {
        return Err(schema_err("PostgreSQL 'query' option is empty"));
    }
    if trimmed.contains(';') {
        return Err(schema_err(
            "PostgreSQL 'query' option contains more than one statement — a source runs exactly \
             one read",
        ));
    }
    let head = trimmed
        .split(|c: char| c.is_whitespace() || c == '(')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(head.as_str(), "select" | "with" | "table" | "values") {
        return Err(schema_err(format!(
            "PostgreSQL 'query' option starts with '{head}' — a source reads, so the statement \
             must begin with SELECT, WITH, TABLE or VALUES"
        )));
    }
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> SourceConfig {
        SourceConfig::new(SourceKind::Postgres, "postgres://reader@db.example.com:5433/metrics")
            .with_option("table", "public.readings")
    }

    fn err_of(config: &SourceConfig) -> String {
        validate(config).expect_err("expected a validation error").to_string()
    }

    // ── Happy paths ──────────────────────────────────────────────────────────

    #[test]
    fn a_full_uri_resolves_every_field() {
        let src = PostgresSource::new(base()).unwrap();
        let t = src.target();
        assert_eq!(t.host, "db.example.com");
        assert_eq!(t.port, 5433);
        assert_eq!(t.database, "metrics");
        assert_eq!(t.user.as_deref(), Some("reader"));
        assert_eq!(t.sslmode, DEFAULT_SSL_MODE);
        assert_eq!(t.query, PostgresQuery::Table("public.readings".into()));
        assert_eq!(t.resolved_sql(), "SELECT * FROM public.readings");
        assert_eq!(src.kind(), SourceKind::Postgres);
    }

    #[test]
    fn port_user_and_sslmode_all_have_defaults() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics")
            .with_option("table", "readings");
        let t = PostgresSource::new(c).unwrap().target().clone();
        assert_eq!(t.port, DEFAULT_PORT);
        assert_eq!(t.user, None);
        assert_eq!(t.sslmode, "prefer");
    }

    #[test]
    fn a_bracketed_ipv6_host_keeps_its_port() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://[::1]:6000/metrics")
            .with_option("table", "readings");
        let t = PostgresSource::new(c).unwrap().target().clone();
        assert_eq!(t.host, "::1");
        assert_eq!(t.port, 6000);
    }

    #[test]
    fn a_bracketed_ipv6_host_without_a_port_is_accepted() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://[::1]/metrics")
            .with_option("table", "readings");
        let t = PostgresSource::new(c).unwrap().target().clone();
        assert_eq!((t.host.as_str(), t.port), ("::1", DEFAULT_PORT));
    }

    #[test]
    fn the_postgresql_scheme_is_equivalent() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgresql://db/metrics")
            .with_option("table", "readings");
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn limit_is_applied_to_a_table_read() {
        let c = base().with_option("limit", "500");
        let t = PostgresSource::new(c).unwrap().target().clone();
        assert_eq!(t.limit, Some(500));
        assert_eq!(t.resolved_sql(), "SELECT * FROM public.readings LIMIT 500");
    }

    #[test]
    fn an_author_statement_is_passed_through_verbatim() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics")
            .with_option("query", "  SELECT t, psi FROM readings ORDER BY t;  ");
        let t = PostgresSource::new(c).unwrap().target().clone();
        assert_eq!(t.resolved_sql(), "SELECT t, psi FROM readings ORDER BY t");
    }

    #[test]
    fn every_sslmode_libpq_accepts_is_accepted() {
        for mode in SSL_MODES {
            let c = base().with_option("sslmode", mode);
            assert!(validate(&c).is_ok(), "{mode} rejected");
        }
    }

    #[test]
    fn sslmode_can_come_from_the_uri() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics?sslmode=require")
            .with_option("table", "readings");
        assert_eq!(PostgresSource::new(c).unwrap().target().sslmode, "require");
    }

    #[test]
    fn a_uri_and_option_that_agree_are_fine() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics?sslmode=require")
            .with_option("table", "readings")
            .with_option("sslmode", "REQUIRE");
        assert_eq!(PostgresSource::new(c).unwrap().target().sslmode, "require");
    }

    #[test]
    fn with_and_values_statements_are_reads() {
        for sql in ["WITH x AS (SELECT 1) SELECT * FROM x", "VALUES (1),(2)", "TABLE readings"] {
            let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics")
                .with_option("query", sql);
            assert!(validate(&c).is_ok(), "{sql} rejected");
        }
    }

    // ── Rejected shapes ──────────────────────────────────────────────────────

    #[test]
    fn a_non_postgres_config_is_refused_by_name() {
        let c = SourceConfig::new(SourceKind::Rest, "https://api.example.com/x");
        assert!(err_of(&c).contains("REST"));
    }

    #[test]
    fn a_non_postgres_scheme_is_rejected() {
        let c = SourceConfig::new(SourceKind::Postgres, "mysql://db/metrics")
            .with_option("table", "readings");
        assert!(validate(&c).is_err());
    }

    #[test]
    fn an_embedded_password_is_a_hard_error() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://user:hunter2@db/metrics")
            .with_option("table", "readings");
        let msg = err_of(&c);
        assert!(msg.contains("secret_ref"), "message must point at secret_ref: {msg}");
        assert!(!msg.contains("hunter2"), "the password must never be echoed: {msg}");
    }

    #[test]
    fn an_empty_user_before_at_is_rejected() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://@db/metrics")
            .with_option("table", "readings");
        assert!(validate(&c).is_err());
    }

    #[test]
    fn a_missing_database_is_rejected() {
        for endpoint in ["postgres://db", "postgres://db/", "postgres://db/  "] {
            let c = SourceConfig::new(SourceKind::Postgres, endpoint)
                .with_option("table", "readings");
            assert!(validate(&c).is_err(), "{endpoint} accepted");
        }
    }

    #[test]
    fn extra_path_segments_after_the_database_are_rejected() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics/extra")
            .with_option("table", "readings");
        assert!(validate(&c).is_err());
    }

    #[test]
    fn a_missing_host_is_rejected() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres:///metrics")
            .with_option("table", "readings");
        assert!(err_of(&c).contains("no host"));
    }

    #[test]
    fn a_bad_port_is_rejected() {
        for endpoint in
            ["postgres://db:0/metrics", "postgres://db:99999/metrics", "postgres://db:abc/metrics"]
        {
            let c = SourceConfig::new(SourceKind::Postgres, endpoint)
                .with_option("table", "readings");
            assert!(validate(&c).is_err(), "{endpoint} accepted");
        }
    }

    #[test]
    fn an_unclosed_ipv6_bracket_is_rejected() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://[::1:6000/metrics")
            .with_option("table", "readings");
        assert!(validate(&c).is_err());
    }

    #[test]
    fn setting_both_table_and_query_is_ambiguous_and_refused() {
        let c = base().with_option("query", "SELECT 1");
        assert!(err_of(&c).contains("exactly one"));
    }

    #[test]
    fn neither_table_nor_query_is_rejected() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics");
        assert!(validate(&c).is_err());
    }

    #[test]
    fn a_relation_that_is_not_an_identifier_is_rejected() {
        for table in
            ["readings; DROP TABLE users", "a.b.c", "\"quoted\"", "1readings", "read ings", ""]
        {
            let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics")
                .with_option("table", table);
            // An empty table falls through to the shared "table or query" check;
            // either way the config must not be accepted.
            assert!(validate(&c).is_err(), "table '{table}' accepted");
        }
    }

    #[test]
    fn a_mutating_statement_is_rejected() {
        for sql in [
            "DELETE FROM readings",
            "UPDATE readings SET t = 0",
            "INSERT INTO readings VALUES (1)",
            "DROP TABLE readings",
        ] {
            let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics")
                .with_option("query", sql);
            assert!(validate(&c).is_err(), "{sql} accepted");
        }
    }

    #[test]
    fn a_multi_statement_query_is_rejected() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics")
            .with_option("query", "SELECT 1; DROP TABLE users");
        assert!(err_of(&c).contains("more than one statement"));
    }

    #[test]
    fn an_unknown_sslmode_is_rejected() {
        let c = base().with_option("sslmode", "paranoid");
        assert!(validate(&c).is_err());
    }

    #[test]
    fn a_uri_and_option_sslmode_that_disagree_are_rejected() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics?sslmode=disable")
            .with_option("table", "readings")
            .with_option("sslmode", "require");
        assert!(err_of(&c).contains("disagrees"));
    }

    #[test]
    fn a_bad_limit_is_rejected() {
        for limit in ["0", "-1", "lots", "1.5"] {
            let c = base().with_option("limit", limit);
            assert!(validate(&c).is_err(), "limit '{limit}' accepted");
        }
    }

    #[test]
    fn limit_alongside_a_statement_is_rejected() {
        let c = SourceConfig::new(SourceKind::Postgres, "postgres://db/metrics")
            .with_option("query", "SELECT 1")
            .with_option("limit", "10");
        assert!(err_of(&c).contains("LIMIT"));
    }

    #[test]
    fn the_shared_checks_still_apply() {
        // poll_seconds and secret_ref are enforced by `super::validate_config`;
        // this provider must not bypass them.
        let mut c = base();
        c.poll_seconds = Some(0);
        assert!(validate(&c).is_err());

        let c = base().with_secret_ref("Bearer abc.def");
        assert!(validate(&c).is_err());
    }

    // ── The not-compiled-in surface ──────────────────────────────────────────

    #[test]
    fn test_connection_names_the_provider_and_the_missing_feature() {
        let src = PostgresSource::new(base()).unwrap();
        let err = src.test_connection().expect_err("no client is linked");
        let msg = err.to_string();
        assert!(msg.contains("PostgreSQL"), "must name the provider: {msg}");
        assert!(msg.contains("'postgres' feature"), "must name the feature: {msg}");
        assert!(msg.contains("not compiled in"), "must say it is absent: {msg}");
        assert!(
            matches!(&err, DataError::Io(e) if e.kind() == std::io::ErrorKind::Unsupported),
            "a missing capability must not masquerade as a config error"
        );
    }

    #[test]
    fn fetch_reports_the_statement_it_would_have_run() {
        let src = PostgresSource::new(base().with_option("limit", "7")).unwrap();
        let msg = src.fetch().expect_err("no client is linked").to_string();
        assert!(msg.contains("SELECT * FROM public.readings LIMIT 7"), "{msg}");
        assert!(msg.contains("db.example.com:5433/metrics"), "{msg}");
    }

    #[test]
    fn the_password_is_read_from_the_environment_at_call_time() {
        let key = "EUSTRESS_TEST_PG_PASSWORD_UNSET";
        std::env::remove_var(key);
        let src = PostgresSource::new(base().with_secret_ref(key)).unwrap();
        assert_eq!(src.password(), None, "an unset variable must yield nothing to leak");
        assert!(
            !format!("{:?}", src.config()).contains("hunter2"),
            "the config carries only the variable name"
        );
    }
}

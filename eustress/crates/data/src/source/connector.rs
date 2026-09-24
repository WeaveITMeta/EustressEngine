//! Reading a Connector: from its persisted `[attributes]` to a fetched
//! [`Frame`].
//!
//! Bound Parameters, scripts, the agent tools and the command line all read
//! Connectors through this one path, so they agree on every rule: how the
//! attributes become a [`SourceConfig`], where a relative file endpoint points,
//! when a one-off statement may replace the configured query, and when a
//! disabled Connector is refused.
//!
//! Reading the `_instance.toml` itself stays with the caller: this crate has
//! no TOML parser, and each caller already has one.

use std::path::Path;

use super::{open, validate_config, SourceConfig, SourceKind};
use crate::{DataError, Frame, Result};

/// How one read of a Connector should go.
#[derive(Clone, Copy, Debug)]
pub struct ConnectorRead<'a> {
    /// The Space the Connector belongs to. A relative file endpoint (a CSV in
    /// the Space) resolves against it.
    pub space_root: &'a Path,
    /// A statement that replaces the Connector's configured `query` for this
    /// read only: Cypher for Neo4j and Neptune, SQL for PostgreSQL, a query
    /// document for GraphQL.
    pub statement: Option<&'a str>,
    /// Refuse a Connector whose `enabled` attribute is false.
    ///
    /// Automated readers (bound Parameters, scripts, agents) set this, so
    /// nothing reaches an outside system unless a person switched the
    /// Connector on. A person's own explicit read, such as a command they
    /// typed, leaves it off.
    pub require_enabled: bool,
}

/// The providers whose read is a statement, and so accept a
/// [`ConnectorRead::statement`].
pub fn takes_statement(kind: SourceKind) -> bool {
    matches!(kind, SourceKind::Neo4j | SourceKind::Neptune | SourceKind::GraphQl | SourceKind::Postgres)
}

/// Build the [`SourceConfig`] a read will use, without touching the network.
pub fn connector_config<I, K, V>(name: &str, attributes: I, how: &ConnectorRead<'_>) -> Result<SourceConfig>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let mut config = super::materialize::config_from_attribute_pairs(attributes)
        .map_err(|e| DataError::Schema(format!("Connector '{name}': {e}")))?;
    if how.require_enabled && !config.enabled {
        return Err(DataError::Schema(format!(
            "Connector '{name}' is disabled; set `enabled` to true in its Properties before anything reads it"
        )));
    }
    if config.kind.is_local() && !config.endpoint.is_empty() && !config.endpoint.contains("://") {
        let path = Path::new(&config.endpoint);
        if path.is_relative() {
            config.endpoint = how.space_root.join(path).to_string_lossy().into_owned();
        }
    }
    if let Some(statement) = how.statement.map(str::trim).filter(|s| !s.is_empty()) {
        if !takes_statement(config.kind) {
            return Err(DataError::Schema(format!(
                "Connector '{name}' is {}, which reads a fixed resource; a statement applies to \
                 Neo4j, Neptune, GraphQL and PostgreSQL Connectors",
                config.kind.as_str()
            )));
        }
        // PostgreSQL takes a table OR a query; the statement is the query.
        config.options.remove("table");
        config.options.insert("query".to_string(), statement.to_string());
    }
    validate_config(&config).map_err(|e| DataError::Schema(format!("Connector '{name}': {e}")))?;
    Ok(config)
}

/// Read a Connector: build its config (see [`connector_config`]) and fetch.
/// Blocking; run it off the frame loop.
pub fn read_connector<I, K, V>(name: &str, attributes: I, how: &ConnectorRead<'_>) -> Result<Frame>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let config = connector_config(name, attributes, how)?;
    open(config)?.fetch().map_err(|e| DataError::Schema(format!("Connector '{name}': {e}")))
}

/// Whether `name` can name a Connector folder: one path segment, nothing that
/// climbs out of the Space.
pub fn is_connector_name(name: &str) -> bool {
    !name.trim().is_empty()
        && !name.contains(['/', '\\'])
        && name != "."
        && name != ".."
        && !name.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn how(require_enabled: bool) -> ConnectorRead<'static> {
        ConnectorRead { space_root: Path::new("/space"), statement: None, require_enabled }
    }

    #[test]
    fn a_disabled_connector_is_refused_only_when_the_reader_requires_it() {
        let attrs = [("source_type", "CSV"), ("endpoint", "data/readings.csv"), ("enabled", "false")];
        let e = connector_config("Bench", attrs, &how(true)).unwrap_err();
        assert!(e.to_string().contains("disabled"), "{e}");
        let c = connector_config("Bench", attrs, &how(false)).unwrap();
        assert!(c.endpoint.ends_with("readings.csv"));
        assert!(Path::new(&c.endpoint).starts_with("/space"), "relative CSV resolves in the Space: {}", c.endpoint);
    }

    #[test]
    fn a_statement_replaces_the_query_and_a_table() {
        let attrs = [
            ("source_type", "PostgreSQL"),
            ("endpoint", "postgres://db.example.com/ops"),
            ("table", "readings"),
            ("enabled", "true"),
        ];
        let read = ConnectorRead { statement: Some("SELECT t, psi FROM readings"), ..how(true) };
        let c = connector_config("Ops", attrs, &read).unwrap();
        assert_eq!(c.option("query"), Some("SELECT t, psi FROM readings"));
        assert_eq!(c.option("table"), None);
    }

    #[test]
    fn a_statement_on_a_fixed_resource_is_refused() {
        let attrs = [("source_type", "CSV"), ("endpoint", "a.csv"), ("enabled", "true")];
        let read = ConnectorRead { statement: Some("MATCH (n) RETURN n"), ..how(true) };
        let e = connector_config("Bench", attrs, &read).unwrap_err();
        assert!(e.to_string().contains("statement"), "{e}");
    }

    #[test]
    fn connector_names_cannot_leave_the_space() {
        assert!(is_connector_name("SupplierGraph"));
        for bad in ["", "..", "a/b", "a\\b", "../x", "."] {
            assert!(!is_connector_name(bad), "{bad:?}");
        }
    }
}

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
//!
//! A local file endpoint is a path inside the Connector's Space and nowhere
//! else ([`confine_to_space`]). Connectors are written by people and by agents
//! (an agent's `write_file` can create one), so an endpoint is a request, not
//! an address: an absolute path, a `..` step or a link out of the Space would
//! otherwise let anything that can write a file read any file the engine can,
//! and a bound Parameter's error reports a CSV's first line.

use std::path::{Component, Path, PathBuf};

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
    if config.kind.is_local() && !config.endpoint.trim().is_empty() {
        if config.endpoint.contains("://") {
            return Err(DataError::Schema(format!(
                "Connector '{name}': a {} Connector reads a file inside its Space, such as \
                 Datasets/readings.csv, not a URL",
                config.kind.as_str()
            )));
        }
        let path = confine_to_space(how.space_root, &config.endpoint)
            .map_err(|why| DataError::Schema(format!("Connector '{name}': {why}")))?;
        config.endpoint = path.to_string_lossy().into_owned();
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

/// Windows' reserved device names, which name a device in every folder.
const DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
    "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A local endpoint as a file inside the Space, or why not. The same rules as
/// the agent tools' sandbox (`eustress-tools` `sandbox::resolve`): the text
/// must be a relative path of ordinary names (no drive, root, UNC or verbatim
/// prefix, no `:` stream, no `..`, no `.git`, no name Windows would read as a
/// device or trim), and the place it reaches must stay under the Space once
/// every link and junction on disk is followed.
pub fn confine_to_space(space_root: &Path, endpoint: &str) -> std::result::Result<PathBuf, String> {
    let raw = endpoint.trim();
    let outside = || {
        format!("the file endpoint `{raw}` is outside the Space; use a path inside it, such as Datasets/readings.csv")
    };
    let cleaned = raw.replace('\\', "/");
    if cleaned.contains(':') {
        return Err(outside());
    }
    let mut rel = PathBuf::new();
    for part in Path::new(&cleaned).components() {
        match part {
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                let stem = name.split('.').next().unwrap_or("").trim_end();
                if name.trim_end_matches(['.', ' ']).eq_ignore_ascii_case(".git")
                    || name.ends_with('.')
                    || name.ends_with(' ')
                    || name.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
                    || DEVICE_NAMES.iter().any(|d| d.eq_ignore_ascii_case(stem))
                {
                    return Err(format!("the file endpoint `{raw}` names `{name}`, which is not an ordinary file name"));
                }
                rel.push(name.as_ref());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return Err(outside()),
        }
    }
    if rel.as_os_str().is_empty() {
        return Err(outside());
    }
    let joined = space_root.join(&rel);
    // Follow every link and junction in the part that exists. A Space that is
    // not on disk has nothing to read, and the text checks above already keep
    // the path under it.
    if let Ok(root) = space_root.canonicalize() {
        let mut existing = joined.clone();
        while !existing.exists() {
            match existing.parent() {
                Some(parent) if existing.file_name().is_some() => existing = parent.to_path_buf(),
                _ => break,
            }
        }
        let reached = existing.canonicalize().map_err(|_| outside())?;
        if !reached.starts_with(&root) {
            return Err(outside());
        }
    }
    Ok(joined)
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
    fn a_file_endpoint_stays_inside_the_space() {
        let space = std::env::temp_dir().join(format!("eustress_confine_{}", std::process::id()));
        std::fs::create_dir_all(space.join("Datasets")).unwrap();
        std::fs::write(space.join("Datasets").join("readings.csv"), "t,psi\n0,1\n").unwrap();
        let ok = confine_to_space(&space, "Datasets/readings.csv").unwrap();
        assert_eq!(ok, space.join("Datasets").join("readings.csv"));
        assert!(confine_to_space(&space, "Datasets\\readings.csv").is_ok(), "either separator");
        for bad in [
            "C:\\Users\\me\\AppData\\Local\\EustressEngine\\auth_token",
            "C:auth_token",
            "/etc/passwd",
            "\\\\server\\share\\x.csv",
            "\\\\?\\C:\\x.csv",
            "../other/readings.csv",
            "Datasets/../../readings.csv",
            "readings.csv:hidden",
            ".git/config",
            "CON",
            "readings.csv.",
            "",
        ] {
            assert!(confine_to_space(&space, bad).is_err(), "{bad:?} must be refused");
        }
        // Through the shared reader: an absolute endpoint never becomes a read.
        let attrs = [("source_type", "CSV"), ("endpoint", "C:\\Windows\\win.ini"), ("enabled", "true")];
        let read = ConnectorRead { space_root: &space, statement: None, require_enabled: true };
        let e = connector_config("Leak", attrs, &read).unwrap_err();
        assert!(e.to_string().contains("outside the Space"), "{e}");
        let url = [("source_type", "CSV"), ("endpoint", "file:///etc/passwd"), ("enabled", "true")];
        assert!(connector_config("Leak", url, &read).unwrap_err().to_string().contains("not a URL"));
        std::fs::remove_dir_all(&space).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_out_of_the_space_is_refused() {
        let base = std::env::temp_dir().join(format!("eustress_confine_junction_{}", std::process::id()));
        let (space, elsewhere) = (base.join("space"), base.join("elsewhere"));
        std::fs::create_dir_all(&space).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("secret.csv"), "token").unwrap();
        // A junction needs no elevation, so anything that can run a command can make one.
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(space.join("Datasets"))
            .arg(&elsewhere)
            .output()
            .unwrap();
        assert!(made.status.success(), "mklink /J failed: {made:?}");
        assert!(confine_to_space(&space, "Datasets/secret.csv").is_err());
        assert!(confine_to_space(&space, "Datasets/not_yet.csv").is_err());
        std::fs::remove_dir(space.join("Datasets")).unwrap();
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_space_is_refused() {
        let base = std::env::temp_dir().join(format!("eustress_confine_link_{}", std::process::id()));
        let (space, elsewhere) = (base.join("space"), base.join("elsewhere"));
        std::fs::create_dir_all(&space).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("secret.csv"), "token\n").unwrap();
        std::os::unix::fs::symlink(&elsewhere, space.join("Datasets")).unwrap();
        assert!(confine_to_space(&space, "Datasets/secret.csv").is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn connector_names_cannot_leave_the_space() {
        assert!(is_connector_name("SupplierGraph"));
        for bad in ["", "..", "a/b", "a\\b", "../x", "."] {
            assert!(!is_connector_name(bad), "{bad:?}");
        }
    }
}

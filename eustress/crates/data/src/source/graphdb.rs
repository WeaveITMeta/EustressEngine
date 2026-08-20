//! Graph database sources: Neo4j and Amazon Neptune.
//!
//! Both speak Cypher over HTTP, so neither needs a driver, a binary protocol,
//! or a cloud SDK. They differ only in the request envelope and the shape of the
//! result, which is the whole reason they share a module.
//!
//! ## Why a graph source is not just another REST source
//!
//! A REST source returns rows. A graph query returns rows *about relationships*,
//! and the useful next step is almost always to rebuild the graph:
//!
//! ```text
//! MATCH (s:Supplier)-[r:SUPPLIED_BY]->(k:SKU)
//! RETURN s.id AS supplier, k.id AS sku, r.lead_days AS lead_days
//! ```
//!
//! That result is an edge list. [`Neo4jSource::fetch_graph`] hands back a
//! [`Graph`] instead of a table, so "which SKUs lose their only supplier if this
//! vendor stops" is a traversal rather than a pile of self-joins.
//!
//! Both providers refuse a config with no query. A graph database is queried,
//! never listed: returning "the whole graph" of a real supply chain would be a
//! denial of service against your own warehouse.

#![cfg(feature = "import")]

use std::sync::Arc;

use serde_json::Value;

use super::rest::{HttpMethod, HttpRequest, HttpResponse, HttpTransport};
use super::{validate_config, ConnectionStatus, DataSource, SourceConfig, SourceKind};
use crate::graph::Graph;
use crate::{DataError, Frame, Result};

/// Which column of a graph query's result holds each part of an edge.
///
/// Named explicitly rather than guessed: a query aliases its columns however the
/// author likes, and inferring "the first column is the source" would silently
/// build a wrong graph from a correct answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeMapping {
    pub from: String,
    pub to: String,
    /// Column holding the relation type. When absent, `default_rel` is used for
    /// every edge, which suits a single-relation query.
    pub rel: Option<String>,
    pub default_rel: String,
}

impl EdgeMapping {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            rel: None,
            default_rel: "RELATED".to_string(),
        }
    }

    pub fn with_rel_column(mut self, col: impl Into<String>) -> Self {
        self.rel = Some(col.into());
        self
    }

    pub fn with_default_rel(mut self, rel: impl Into<String>) -> Self {
        self.default_rel = rel.into();
        self
    }
}

/// Shared behaviour of a Cypher-over-HTTP source.
trait CypherSource {
    fn transport(&self) -> &Arc<dyn HttpTransport>;
    /// The request that carries this source's query.
    fn request(&self) -> Result<HttpRequest>;
    /// Turn a successful response body into a [`Frame`].
    fn frame_from_body(&self, body: &str) -> Result<Frame>;
}

/// Auth header from the config's `secret_ref`, resolved at call time.
///
/// Neo4j takes HTTP Basic; Neptune is normally IAM-signed, and a bearer token
/// covers the proxied and IAM-disabled deployments this can actually reach.
fn auth_headers(config: &SourceConfig, scheme: &str) -> Vec<(String, String)> {
    match config.resolve_secret() {
        Some(secret) if !secret.is_empty() => {
            vec![("Authorization".to_string(), format!("{scheme} {secret}"))]
        }
        _ => Vec::new(),
    }
}

/// Reject a body that reports query errors, so a failure is never mistaken for
/// an empty result.
fn check_errors(root: &Value) -> Result<()> {
    if let Some(errs) = root.get("errors").and_then(|e| e.as_array()) {
        if !errs.is_empty() {
            let msg = errs
                .iter()
                .filter_map(|e| {
                    e.get("message")
                        .and_then(|m| m.as_str())
                        .or_else(|| e.as_str())
                })
                .collect::<Vec<_>>()
                .join("; ");
            return Err(DataError::Schema(format!("graph query failed: {msg}")));
        }
    }
    Ok(())
}

/// Reject a non-2xx status with its body, which is where these servers put the
/// reason.
fn check_status(resp: &HttpResponse) -> Result<()> {
    if !(200..300).contains(&resp.status) {
        let detail = resp.text().chars().take(300).collect::<String>();
        return Err(DataError::Schema(format!(
            "graph endpoint returned HTTP {}: {}",
            resp.status, detail
        )));
    }
    Ok(())
}

/// Run the query and hand back the tabular result.
fn run_frame<S: CypherSource>(src: &S) -> Result<Frame> {
    let req = src.request()?;
    let resp = src.transport().send(&req)?;
    check_status(&resp)?;
    src.frame_from_body(&resp.text())
}

/// Run the query and rebuild a [`Graph`] from the edge columns.
fn run_graph<S: CypherSource>(src: &S, mapping: &EdgeMapping) -> Result<Graph> {
    let frame = run_frame(src)?;
    Graph::from_edge_frame(
        &frame,
        &mapping.from,
        &mapping.to,
        mapping.rel.as_deref(),
        &mapping.default_rel,
    )
}

/// A cheap liveness probe: run the query and report row count or the failure.
fn probe<S: CypherSource>(src: &S) -> Result<ConnectionStatus> {
    match run_frame(src) {
        Ok(f) => Ok(ConnectionStatus::ok(format!(
            "{} row(s), {} column(s)",
            f.n_rows(),
            f.n_cols()
        ))),
        // A refusal is an ANSWER, not an error: the UI needs to show why rather
        // than surface a failed call.
        Err(e) => Ok(ConnectionStatus::failed(e.to_string())),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Neo4j
// ─────────────────────────────────────────────────────────────────────────────

/// Neo4j over the HTTP transaction endpoint.
pub struct Neo4jSource {
    config: SourceConfig,
    transport: Arc<dyn HttpTransport>,
}

impl Neo4jSource {
    /// Build on the live transport, when one is compiled in.
    ///
    /// Validates the config; still never touches the network here. Without the
    /// `http` feature this succeeds and the first `fetch` explains what is
    /// missing, so a misconfiguration and a missing client stay distinguishable.
    pub fn new(config: SourceConfig) -> Result<Self> {
        Self::with_transport(config, super::rest::default_transport())
    }

    /// Build on a caller-supplied transport. Validates first; never touches the
    /// network.
    pub fn with_transport(config: SourceConfig, transport: Arc<dyn HttpTransport>) -> Result<Self> {
        if config.kind != SourceKind::Neo4j {
            return Err(DataError::Schema(format!(
                "Neo4jSource cannot serve a {} config",
                config.kind.as_str()
            )));
        }
        validate_config(&config)?;
        Ok(Self { config, transport })
    }

    pub fn config(&self) -> &SourceConfig {
        &self.config
    }

    /// Run the query and rebuild the graph it describes.
    pub fn fetch_graph(&self, mapping: &EdgeMapping) -> Result<Graph> {
        run_graph(self, mapping)
    }

    /// The request `fetch` would send. Exposed so the UI can show exactly what
    /// goes out before a source is enabled.
    pub fn request(&self) -> Result<HttpRequest> {
        CypherSource::request(self)
    }

    /// `{endpoint}/db/{database}/tx/commit`, the transactional Cypher endpoint.
    fn url(&self) -> Result<String> {
        let base = self.config.endpoint.trim_end_matches('/');
        // Native schemes are accepted by validation because they are what people
        // paste, but reads go over HTTP and the difference must be stated rather
        // than silently rewritten to a guessed port.
        if base.starts_with("bolt://") || base.starts_with("neo4j://") || base.starts_with("neo4j+s://")
        {
            return Err(DataError::Schema(format!(
                "Neo4j reads use the HTTP API; '{base}' is a Bolt endpoint. \
                 Use the HTTP address instead, usually port 7474 (http://host:7474)."
            )));
        }
        let db = self.config.option("database").unwrap_or("neo4j");
        Ok(format!("{base}/db/{db}/tx/commit"))
    }
}

impl CypherSource for Neo4jSource {
    fn transport(&self) -> &Arc<dyn HttpTransport> {
        &self.transport
    }

    fn request(&self) -> Result<HttpRequest> {
        let query = self.config.option("query").unwrap_or_default();
        let body = serde_json::json!({
            "statements": [{ "statement": query }]
        });
        let mut headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        headers.extend(auth_headers(&self.config, "Basic"));
        Ok(HttpRequest {
            method: HttpMethod::Post,
            url: self.url()?,
            headers,
            body: Some(body.to_string().into_bytes()),
        })
    }

    /// Neo4j returns columns and positional rows; flatten to named objects so
    /// the shared JSON → Frame inference can take it from there.
    fn frame_from_body(&self, body: &str) -> Result<Frame> {
        let root: Value = serde_json::from_str(body)
            .map_err(|e| DataError::Schema(format!("Neo4j response was not JSON: {e}")))?;
        check_errors(&root)?;

        let result = root
            .get("results")
            .and_then(|r| r.as_array())
            .and_then(|a| a.first())
            .ok_or_else(|| DataError::Schema("Neo4j response had no results".into()))?;

        let columns: Vec<&str> = result
            .get("columns")
            .and_then(|c| c.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let mut objects = Vec::new();
        for entry in result.get("data").and_then(|d| d.as_array()).into_iter().flatten() {
            let row = entry.get("row").and_then(|r| r.as_array());
            let Some(row) = row else { continue };
            let mut obj = serde_json::Map::new();
            for (i, name) in columns.iter().enumerate() {
                obj.insert(
                    (*name).to_string(),
                    row.get(i).cloned().unwrap_or(Value::Null),
                );
            }
            objects.push(Value::Object(obj));
        }
        super::rest::frame_from_json_value(&Value::Array(objects), None)
    }
}

impl DataSource for Neo4jSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Neo4j
    }
    fn test_connection(&self) -> Result<ConnectionStatus> {
        probe(self)
    }
    fn fetch(&self) -> Result<Frame> {
        run_frame(self)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Neptune
// ─────────────────────────────────────────────────────────────────────────────

/// Amazon Neptune over its openCypher HTTP endpoint.
pub struct NeptuneSource {
    config: SourceConfig,
    transport: Arc<dyn HttpTransport>,
}

impl NeptuneSource {
    /// Build on the live transport, when one is compiled in. See
    /// [`Neo4jSource::new`].
    pub fn new(config: SourceConfig) -> Result<Self> {
        Self::with_transport(config, super::rest::default_transport())
    }

    pub fn with_transport(config: SourceConfig, transport: Arc<dyn HttpTransport>) -> Result<Self> {
        if config.kind != SourceKind::Neptune {
            return Err(DataError::Schema(format!(
                "NeptuneSource cannot serve a {} config",
                config.kind.as_str()
            )));
        }
        validate_config(&config)?;
        Ok(Self { config, transport })
    }

    pub fn config(&self) -> &SourceConfig {
        &self.config
    }

    pub fn fetch_graph(&self, mapping: &EdgeMapping) -> Result<Graph> {
        run_graph(self, mapping)
    }

    pub fn request(&self) -> Result<HttpRequest> {
        CypherSource::request(self)
    }
}

impl CypherSource for NeptuneSource {
    fn transport(&self) -> &Arc<dyn HttpTransport> {
        &self.transport
    }

    fn request(&self) -> Result<HttpRequest> {
        let query = self.config.option("query").unwrap_or_default();
        let base = self.config.endpoint.trim_end_matches('/');
        let mut headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        // Neptune is usually IAM-signed. Request signing is not implemented, so
        // this reaches proxied or IAM-disabled clusters; saying so beats a
        // mystery 403.
        headers.extend(auth_headers(&self.config, "Bearer"));
        Ok(HttpRequest {
            method: HttpMethod::Post,
            url: format!("{base}/openCypher"),
            headers,
            body: Some(serde_json::json!({ "query": query }).to_string().into_bytes()),
        })
    }

    /// Neptune already returns named objects under `results`.
    fn frame_from_body(&self, body: &str) -> Result<Frame> {
        let root: Value = serde_json::from_str(body)
            .map_err(|e| DataError::Schema(format!("Neptune response was not JSON: {e}")))?;
        check_errors(&root)?;
        super::rest::frame_from_json_body(body, Some("results")).or_else(|e| {
            // Some deployments return a bare array; fall back before failing.
            if root.is_array() {
                super::rest::frame_from_json_value(&root, None)
            } else {
                Err(e)
            }
        })
    }
}

impl DataSource for NeptuneSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Neptune
    }
    fn test_connection(&self) -> Result<ConnectionStatus> {
        probe(self)
    }
    fn fetch(&self) -> Result<Frame> {
        run_frame(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Direction;
    use std::sync::Mutex;

    /// Records what was sent and replays a canned response.
    struct Stub {
        response: HttpResponse,
        seen: Mutex<Vec<HttpRequest>>,
    }

    impl Stub {
        fn new(status: u16, body: &str) -> Arc<Self> {
            Arc::new(Self {
                response: HttpResponse::new(status, body),
                seen: Mutex::new(Vec::new()),
            })
        }
        fn last(&self) -> HttpRequest {
            self.seen.lock().unwrap().last().cloned().expect("a request was sent")
        }
    }

    impl HttpTransport for Stub {
        fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
            self.seen.lock().unwrap().push(req.clone());
            Ok(self.response.clone())
        }
    }

    const NEO4J_EDGES: &str = r#"{
      "results": [{
        "columns": ["supplier", "sku", "lead_days"],
        "data": [
          {"row": ["ACME", "SKU-1", 14]},
          {"row": ["ACME", "SKU-2", 21]},
          {"row": ["GLOBEX", "SKU-2", 30]}
        ]
      }],
      "errors": []
    }"#;

    const NEPTUNE_EDGES: &str = r#"{
      "results": [
        {"supplier": "ACME", "sku": "SKU-1", "lead_days": 14},
        {"supplier": "GLOBEX", "sku": "SKU-2", "lead_days": 30}
      ]
    }"#;

    fn neo4j_config() -> SourceConfig {
        SourceConfig::new(SourceKind::Neo4j, "http://graph.internal:7474")
            .with_option("query", "MATCH (s)-[r]->(k) RETURN s.id AS supplier")
    }

    fn neptune_config() -> SourceConfig {
        SourceConfig::new(SourceKind::Neptune, "https://cluster.neptune.amazonaws.com:8182")
            .with_option("query", "MATCH (s)-[r]->(k) RETURN s.id AS supplier")
    }

    #[test]
    fn a_graph_source_builds_on_the_live_transport() {
        // Construction must succeed whether or not a client is compiled in:
        // a bad config and a missing client are different problems and must
        // fail at different moments.
        let src = Neo4jSource::new(neo4j_config()).expect("valid config builds");
        assert_eq!(src.kind(), SourceKind::Neo4j);
        assert!(NeptuneSource::new(neptune_config()).is_ok());

        // And an invalid config is still refused up front, not at fetch time.
        let no_query = SourceConfig::new(SourceKind::Neo4j, "http://host:7474");
        assert!(Neo4jSource::new(no_query).is_err());
    }

    #[cfg(not(feature = "http"))]
    #[test]
    fn without_the_http_feature_a_live_fetch_says_exactly_that() {
        let err = Neo4jSource::new(neo4j_config()).unwrap().fetch().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("`http` feature"), "{msg}");
        assert!(msg.contains("with_transport"), "the error names the way out: {msg}");
    }

    #[test]
    fn a_graph_source_without_a_query_is_refused() {
        for kind in [SourceKind::Neo4j, SourceKind::Neptune] {
            let bare = SourceConfig::new(kind, "https://host:7474");
            let err = validate_config(&bare).unwrap_err();
            assert!(format!("{err}").contains("query"), "{kind:?}: {err}");
        }
    }

    #[test]
    fn a_provider_refuses_a_config_for_a_different_provider() {
        let stub = Stub::new(200, NEO4J_EDGES);
        let Err(err) = Neo4jSource::with_transport(neptune_config(), stub.clone()) else {
            panic!("a provider must refuse another provider's config");
        };
        assert!(format!("{err}").contains("Neptune"));
    }

    #[test]
    fn neo4j_posts_cypher_to_the_transaction_endpoint() {
        let stub = Stub::new(200, NEO4J_EDGES);
        let src = Neo4jSource::with_transport(neo4j_config(), stub.clone()).unwrap();
        src.fetch().unwrap();

        let req = stub.last();
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(req.url, "http://graph.internal:7474/db/neo4j/tx/commit");
        assert!(req.body_text().unwrap().contains("statements"));
    }

    #[test]
    fn neo4j_honours_a_named_database() {
        let stub = Stub::new(200, NEO4J_EDGES);
        let cfg = neo4j_config().with_option("database", "supplychain");
        let src = Neo4jSource::with_transport(cfg, stub.clone()).unwrap();
        src.fetch().unwrap();
        assert!(stub.last().url.contains("/db/supplychain/tx/commit"));
    }

    #[test]
    fn a_bolt_endpoint_is_explained_rather_than_silently_rewritten() {
        let stub = Stub::new(200, NEO4J_EDGES);
        let cfg = SourceConfig::new(SourceKind::Neo4j, "bolt://graph.internal:7687")
            .with_option("query", "MATCH (n) RETURN n");
        // Validation accepts it, because that is what people paste.
        let src = Neo4jSource::with_transport(cfg, stub).unwrap();
        let err = src.fetch().unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("Bolt"), "{msg}");
        assert!(msg.contains("7474"), "the message names the port to use instead");
    }

    #[test]
    fn neo4j_columns_and_rows_flatten_into_a_frame() {
        let stub = Stub::new(200, NEO4J_EDGES);
        let src = Neo4jSource::with_transport(neo4j_config(), stub).unwrap();
        let f = src.fetch().unwrap();
        assert_eq!(f.n_rows(), 3);
        assert!(f.column("supplier").is_some());
        assert!(f.column("lead_days").is_some());
    }

    #[test]
    fn neo4j_result_rebuilds_the_supply_graph() {
        let stub = Stub::new(200, NEO4J_EDGES);
        let src = Neo4jSource::with_transport(neo4j_config(), stub).unwrap();
        let g = src
            .fetch_graph(&EdgeMapping::new("supplier", "sku").with_default_rel("SUPPLIED_BY"))
            .unwrap();

        assert_eq!(g.node_count(), 4, "ACME, GLOBEX, SKU-1, SKU-2");
        assert_eq!(g.edge_count(), 3);
        // The question that motivated all of this.
        let sole = g.sole_sourced("SUPPLIED_BY");
        assert!(sole.iter().any(|id| *id == "SKU-1"));
        assert!(!sole.iter().any(|id| *id == "SKU-2"), "SKU-2 has two suppliers");
        // Query columns survive as edge properties.
        let e = g.incident("SKU-1", Direction::In, None)[0];
        assert!(e.prop("lead_days").is_some());
    }

    #[test]
    fn neptune_posts_opencypher_and_reads_named_rows() {
        let stub = Stub::new(200, NEPTUNE_EDGES);
        let src = NeptuneSource::with_transport(neptune_config(), stub.clone()).unwrap();
        let f = src.fetch().unwrap();

        assert!(stub.last().url.ends_with("/openCypher"));
        assert_eq!(f.n_rows(), 2);
        assert!(f.column("supplier").is_some());
    }

    #[test]
    fn neptune_result_rebuilds_the_graph_too() {
        let stub = Stub::new(200, NEPTUNE_EDGES);
        let src = NeptuneSource::with_transport(neptune_config(), stub).unwrap();
        let g = src
            .fetch_graph(&EdgeMapping::new("supplier", "sku").with_default_rel("SUPPLIED_BY"))
            .unwrap();
        assert_eq!(g.edge_count(), 2);
        assert_eq!(g.relation_types().into_iter().collect::<Vec<_>>(), vec!["SUPPLIED_BY"]);
    }

    #[test]
    fn a_query_error_is_reported_and_never_read_as_an_empty_result() {
        let body = r#"{"results":[],"errors":[{"code":"Neo.ClientError","message":"Invalid syntax"}]}"#;
        let stub = Stub::new(200, body);
        let src = Neo4jSource::with_transport(neo4j_config(), stub).unwrap();
        let err = src.fetch().unwrap_err();
        assert!(format!("{err}").contains("Invalid syntax"));
    }

    #[test]
    fn a_non_2xx_status_carries_its_reason() {
        let stub = Stub::new(403, "access denied for user");
        let src = Neo4jSource::with_transport(neo4j_config(), stub).unwrap();
        let err = src.fetch().unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("403"));
        assert!(msg.contains("access denied"));
    }

    #[test]
    fn test_connection_reports_a_failure_instead_of_erroring() {
        let stub = Stub::new(500, "boom");
        let src = Neo4jSource::with_transport(neo4j_config(), stub).unwrap();
        // The UI needs an answer to show, not a call that blew up.
        let status = src.test_connection().unwrap();
        assert!(!status.reachable);
        assert!(status.detail.contains("500"));
    }

    #[test]
    fn the_auth_header_is_sent_when_the_secret_resolves() {
        std::env::set_var("TEST_NEO4J_SECRET", "dXNlcjpwYXNz");
        let stub = Stub::new(200, NEO4J_EDGES);
        let cfg = neo4j_config().with_secret_ref("TEST_NEO4J_SECRET");
        let src = Neo4jSource::with_transport(cfg, stub.clone()).unwrap();
        src.fetch().unwrap();

        let req = stub.last();
        assert!(req
            .headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Basic dXNlcjpwYXNz"));
        // The credential must never reach a log through Debug.
        assert!(!format!("{req:?}").contains("dXNlcjpwYXNz"));
        std::env::remove_var("TEST_NEO4J_SECRET");
    }

    #[test]
    fn no_auth_header_is_sent_when_no_secret_is_configured() {
        let stub = Stub::new(200, NEO4J_EDGES);
        let src = Neo4jSource::with_transport(neo4j_config(), stub.clone()).unwrap();
        src.fetch().unwrap();
        assert!(!stub.last().headers.iter().any(|(k, _)| k == "Authorization"));
    }
}

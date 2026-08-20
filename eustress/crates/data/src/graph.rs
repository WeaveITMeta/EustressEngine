//! Property graph: typed nodes, typed edges, and traversal.
//!
//! The Data Platform's second first-class shape. A [`Frame`] answers questions
//! about rows; a [`Graph`] answers questions about **relationships**, and a
//! supply chain is relationships: which supplier feeds which SKU, through which
//! site, with what lead time, and what breaks if one of them stops.
//!
//! Those questions are miserable in a table. "Which SKUs depend on this supplier
//! within three hops" is a self-join per hop against a rows engine, and a
//! constant-time neighbour lookup here.
//!
//! ## The model
//!
//! A property graph, in the shape Neo4j and Neptune use, because that is what
//! the domain already speaks:
//!
//! - A [`Node`] has a stable business id (`"SKU-4417"`), one or more **labels**
//!   (`"SKU"`, `"Supplier"`), and arbitrary properties.
//! - An [`Edge`] is directed, carries a **relation type** (`"SUPPLIED_BY"`) and
//!   its own properties, and there may be many between the same pair.
//!
//! Relation type is a free string, not an enum: a supply chain needs
//! `SUPPLIED_BY` and `SHIPS_TO`, and a knowledge graph needs `IsA` and `Causes`.
//! Fixing the vocabulary would serve neither.
//!
//! ## Relationship to Frame
//!
//! A graph converts to and from tables losslessly, so it is a *view* over data
//! the platform already holds rather than a parallel universe:
//! [`Graph::from_edge_frame`] reads an edge list a warehouse can produce,
//! [`Graph::to_edge_frame`] and [`Graph::to_node_frame`] hand it back for
//! charting, export, and provenance.
//!
//! [`PropValue`] mirrors [`ColumnDtype`] exactly, so nothing is coerced or lost
//! crossing that boundary.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    frame_from_columns, ColumnData, ColumnDtype, ColumnSpec, DataError, Frame, Result,
};

/// A node's stable business key: a SKU code, a supplier id, a site name.
pub type NodeId = String;

/// A node or edge property. Mirrors [`ColumnDtype`] so graph/table conversion
/// never coerces.
#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    Str(String),
    F64(f64),
    I64(i64),
    Bool(bool),
}

impl PropValue {
    pub fn dtype(&self) -> ColumnDtype {
        match self {
            Self::Str(_) => ColumnDtype::Str,
            Self::F64(_) => ColumnDtype::F64,
            Self::I64(_) => ColumnDtype::I64,
            Self::Bool(_) => ColumnDtype::Bool,
        }
    }

    /// Numeric view, for weighted traversal. `None` for non-numeric.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::F64(v) => Some(*v),
            Self::I64(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }
}

impl From<&str> for PropValue {
    fn from(v: &str) -> Self {
        Self::Str(v.to_string())
    }
}
impl From<String> for PropValue {
    fn from(v: String) -> Self {
        Self::Str(v)
    }
}
impl From<f64> for PropValue {
    fn from(v: f64) -> Self {
        Self::F64(v)
    }
}
impl From<i64> for PropValue {
    fn from(v: i64) -> Self {
        Self::I64(v)
    }
}
impl From<bool> for PropValue {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

/// Properties, ordered so every render and export is deterministic.
pub type Props = BTreeMap<String, PropValue>;

/// A vertex.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: NodeId,
    /// Type tags. A node may carry several (`["Supplier", "Tier1"]`).
    pub labels: Vec<String>,
    pub props: Props,
}

impl Node {
    pub fn new(id: impl Into<NodeId>) -> Self {
        Self { id: id.into(), labels: Vec::new(), props: Props::new() }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        let l = label.into();
        if !self.labels.contains(&l) {
            self.labels.push(l);
        }
        self
    }

    pub fn with_prop(mut self, key: impl Into<String>, value: impl Into<PropValue>) -> Self {
        self.props.insert(key.into(), value.into());
        self
    }

    pub fn has_label(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l == label)
    }

    pub fn prop(&self, key: &str) -> Option<&PropValue> {
        self.props.get(key)
    }
}

/// A directed, typed relationship.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    /// `SUPPLIED_BY`, `SHIPS_TO`, `IsA`, …
    pub rel: String,
    pub props: Props,
}

impl Edge {
    pub fn new(from: impl Into<NodeId>, rel: impl Into<String>, to: impl Into<NodeId>) -> Self {
        Self { from: from.into(), rel: rel.into(), to: to.into(), props: Props::new() }
    }

    pub fn with_prop(mut self, key: impl Into<String>, value: impl Into<PropValue>) -> Self {
        self.props.insert(key.into(), value.into());
        self
    }

    pub fn prop(&self, key: &str) -> Option<&PropValue> {
        self.props.get(key)
    }
}

/// Which way to walk an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Follow edges leaving the node.
    Out,
    /// Follow edges arriving at the node.
    In,
    /// Ignore direction.
    Both,
}

/// A property graph with adjacency and label indexes.
///
/// Indexes are maintained on insert, so traversal never scans the edge list.
/// That is the whole reason to hold this shape instead of a table: a supply
/// chain with a hundred thousand SKUs answers "who supplies this" in constant
/// time rather than a full scan per hop.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    nodes: BTreeMap<NodeId, Node>,
    edges: Vec<Edge>,
    /// node -> indices into `edges` leaving it.
    out_adj: BTreeMap<NodeId, Vec<usize>>,
    /// node -> indices into `edges` arriving at it.
    in_adj: BTreeMap<NodeId, Vec<usize>>,
    /// label -> node ids.
    by_label: BTreeMap<String, BTreeSet<NodeId>>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    // ── Construction ──────────────────────────────────────────────────────

    /// Insert or replace a node, keeping the label index consistent.
    pub fn add_node(&mut self, node: Node) {
        if let Some(old) = self.nodes.get(&node.id) {
            for l in &old.labels {
                if let Some(set) = self.by_label.get_mut(l) {
                    set.remove(&node.id);
                }
            }
        }
        for l in &node.labels {
            self.by_label.entry(l.clone()).or_default().insert(node.id.clone());
        }
        self.nodes.insert(node.id.clone(), node);
    }

    /// Add an edge. Endpoints that do not exist yet are created bare, so an
    /// edge list can be loaded without a separate node pass — which is how
    /// warehouse exports usually arrive.
    pub fn add_edge(&mut self, edge: Edge) {
        for id in [&edge.from, &edge.to] {
            if !self.nodes.contains_key(id) {
                self.add_node(Node::new(id.clone()));
            }
        }
        let idx = self.edges.len();
        self.out_adj.entry(edge.from.clone()).or_default().push(idx);
        self.in_adj.entry(edge.to.clone()).or_default().push(idx);
        self.edges.push(edge);
    }

    // ── Access ────────────────────────────────────────────────────────────

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.iter()
    }

    /// Every node carrying a label, in id order.
    pub fn nodes_with_label(&self, label: &str) -> Vec<&Node> {
        self.by_label
            .get(label)
            .map(|ids| ids.iter().filter_map(|id| self.nodes.get(id)).collect())
            .unwrap_or_default()
    }

    /// Every distinct relation type present.
    pub fn relation_types(&self) -> BTreeSet<&str> {
        self.edges.iter().map(|e| e.rel.as_str()).collect()
    }

    /// Edges touching a node in a direction, optionally filtered by relation.
    pub fn incident(&self, id: &str, dir: Direction, rel: Option<&str>) -> Vec<&Edge> {
        let mut idxs: Vec<usize> = Vec::new();
        if matches!(dir, Direction::Out | Direction::Both) {
            idxs.extend(self.out_adj.get(id).into_iter().flatten().copied());
        }
        if matches!(dir, Direction::In | Direction::Both) {
            idxs.extend(self.in_adj.get(id).into_iter().flatten().copied());
        }
        idxs.sort_unstable();
        idxs.dedup();
        idxs.into_iter()
            .map(|i| &self.edges[i])
            .filter(|e| rel.is_none_or(|r| e.rel == r))
            .collect()
    }

    /// Adjacent node ids, deduplicated and ordered.
    pub fn neighbors(&self, id: &str, dir: Direction, rel: Option<&str>) -> Vec<&NodeId> {
        let mut out: Vec<&NodeId> = self
            .incident(id, dir, rel)
            .into_iter()
            .map(|e| if e.from == id { &e.to } else { &e.from })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    // ── Traversal ─────────────────────────────────────────────────────────

    /// Breadth-first reachable set within `max_depth` hops, excluding the start.
    ///
    /// Depth is bounded on purpose. An unbounded walk of a real supply chain
    /// reaches nearly everything and answers nothing.
    pub fn traverse(
        &self,
        start: &str,
        dir: Direction,
        rel: Option<&str>,
        max_depth: usize,
    ) -> Vec<Reached> {
        let mut seen: BTreeSet<NodeId> = BTreeSet::new();
        let mut out = Vec::new();
        let mut queue: VecDeque<(NodeId, usize)> = VecDeque::new();

        seen.insert(start.to_string());
        queue.push_back((start.to_string(), 0));

        while let Some((id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            for n in self.neighbors(&id, dir, rel) {
                if seen.insert(n.clone()) {
                    out.push(Reached { id: n.clone(), depth: depth + 1 });
                    queue.push_back((n.clone(), depth + 1));
                }
            }
        }
        out
    }

    /// Shortest hop path between two nodes, inclusive of both ends.
    ///
    /// Unweighted BFS: for supply questions the number of intermediaries is
    /// usually the thing that matters, and a cost-weighted variant would need a
    /// declared cost property to be meaningful.
    pub fn shortest_path(&self, from: &str, to: &str, rel: Option<&str>) -> Option<Vec<NodeId>> {
        if !self.nodes.contains_key(from) || !self.nodes.contains_key(to) {
            return None;
        }
        if from == to {
            return Some(vec![from.to_string()]);
        }
        let mut prev: BTreeMap<NodeId, NodeId> = BTreeMap::new();
        let mut seen: BTreeSet<NodeId> = BTreeSet::from([from.to_string()]);
        let mut queue = VecDeque::from([from.to_string()]);

        while let Some(cur) = queue.pop_front() {
            for n in self.neighbors(&cur, Direction::Out, rel) {
                if !seen.insert(n.clone()) {
                    continue;
                }
                prev.insert(n.clone(), cur.clone());
                if n == to {
                    let mut path = vec![to.to_string()];
                    let mut c = to.to_string();
                    while let Some(p) = prev.get(&c) {
                        path.push(p.clone());
                        c = p.clone();
                    }
                    path.reverse();
                    return Some(path);
                }
                queue.push_back(n.clone());
            }
        }
        None
    }

    /// Cypher-flavoured one-hop pattern: `(:from_label)-[:rel]->(:to_label)`.
    ///
    /// Not a Cypher parser. It covers the pattern that actually gets asked of a
    /// supply graph, without pretending to a query language this does not
    /// implement.
    pub fn match_pattern(
        &self,
        from_label: Option<&str>,
        rel: Option<&str>,
        to_label: Option<&str>,
    ) -> Vec<&Edge> {
        self.edges
            .iter()
            .filter(|e| rel.is_none_or(|r| e.rel == r))
            .filter(|e| {
                from_label.is_none_or(|l| self.node(&e.from).is_some_and(|n| n.has_label(l)))
            })
            .filter(|e| to_label.is_none_or(|l| self.node(&e.to).is_some_and(|n| n.has_label(l))))
            .collect()
    }

    /// Induced subgraph over the given nodes: edges are kept only when BOTH
    /// endpoints are in the set, so no dangling reference is produced.
    pub fn subgraph(&self, ids: &BTreeSet<NodeId>) -> Graph {
        let mut g = Graph::new();
        for id in ids {
            if let Some(n) = self.nodes.get(id) {
                g.add_node(n.clone());
            }
        }
        for e in &self.edges {
            if ids.contains(&e.from) && ids.contains(&e.to) {
                g.add_edge(e.clone());
            }
        }
        g
    }

    // ── Supply-chain questions ────────────────────────────────────────────

    /// Nodes with exactly one inbound edge of `rel`: the sole-source risks.
    ///
    /// The question a supply chain is actually asked. A SKU with one supplier
    /// is a single point of failure, and it is invisible in a table until
    /// somebody thinks to group by and count.
    pub fn sole_sourced(&self, rel: &str) -> Vec<&NodeId> {
        self.nodes
            .keys()
            .filter(|id| {
                let suppliers: BTreeSet<&NodeId> = self
                    .incident(id, Direction::In, Some(rel))
                    .into_iter()
                    .map(|e| &e.from)
                    .collect();
                suppliers.len() == 1
            })
            .collect()
    }

    /// Nodes with no inbound edge of `rel`: unsourced, which is usually either
    /// a raw input or a gap in the data.
    pub fn unsourced(&self, rel: &str) -> Vec<&NodeId> {
        self.nodes
            .keys()
            .filter(|id| self.incident(id, Direction::In, Some(rel)).is_empty())
            .collect()
    }

    /// Everything that would be affected if a node stopped: the set reachable
    /// by walking `rel` edges backwards.
    pub fn impact_of_losing(&self, id: &str, rel: &str) -> Vec<Reached> {
        self.traverse(id, Direction::In, Some(rel), usize::MAX)
    }

    /// Cycles in the `rel` sub-graph, each as the node ids on the loop.
    ///
    /// A circular dependency in a bill of materials is a data error or a
    /// genuine deadlock, and either way it must surface rather than send a
    /// traversal round forever.
    pub fn cycles(&self, rel: Option<&str>) -> Vec<Vec<NodeId>> {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            Open,
            Done,
        }
        let mut state: BTreeMap<NodeId, Mark> = BTreeMap::new();
        let mut stack: Vec<NodeId> = Vec::new();
        let mut found: Vec<Vec<NodeId>> = Vec::new();

        // Iterative DFS: a deep bill of materials would blow a recursive one.
        for root in self.nodes.keys() {
            if state.contains_key(root) {
                continue;
            }
            let mut work: Vec<(NodeId, bool)> = vec![(root.clone(), false)];
            while let Some((id, backtrack)) = work.pop() {
                if backtrack {
                    state.insert(id.clone(), Mark::Done);
                    stack.pop();
                    continue;
                }
                match state.get(&id) {
                    Some(Mark::Done) => continue,
                    Some(Mark::Open) => {
                        // Closed a loop: capture from the earlier visit onward.
                        if let Some(pos) = stack.iter().position(|s| *s == id) {
                            found.push(stack[pos..].to_vec());
                        }
                        continue;
                    }
                    None => {}
                }
                state.insert(id.clone(), Mark::Open);
                stack.push(id.clone());
                work.push((id.clone(), true));
                for e in self.incident(&id, Direction::Out, rel) {
                    if state.get(&e.to) != Some(&Mark::Done) {
                        work.push((e.to.clone(), false));
                    }
                }
            }
        }
        found
    }

    // ── Frame bridge ──────────────────────────────────────────────────────

    /// Build a graph from an edge-list table.
    ///
    /// This is the projection view: an edge list is exactly what an ERP or
    /// warehouse exports, so a graph is a lens over data the platform already
    /// holds rather than a second copy of it. Remaining columns become edge
    /// properties, so nothing in the table is discarded.
    pub fn from_edge_frame(
        frame: &Frame,
        from_col: &str,
        to_col: &str,
        rel_col: Option<&str>,
        default_rel: &str,
    ) -> Result<Graph> {
        let cols = frame.columns();
        let find = |name: &str| -> Result<usize> {
            cols.iter()
                .position(|(s, _)| s.name == name)
                .ok_or_else(|| DataError::Schema(format!("edge column '{name}' not found")))
        };
        let fi = find(from_col)?;
        let ti = find(to_col)?;
        let ri = rel_col.map(find).transpose()?;

        let mut g = Graph::new();
        for r in 0..frame.n_rows() {
            let (Some(from), Some(to)) = (cell_string(&cols[fi].1, r), cell_string(&cols[ti].1, r))
            else {
                // A null endpoint is not an edge. Skipping beats inventing one.
                continue;
            };
            let rel = ri
                .and_then(|i| cell_string(&cols[i].1, r))
                .unwrap_or_else(|| default_rel.to_string());

            let mut edge = Edge::new(from, rel, to);
            for (i, (spec, data)) in cols.iter().enumerate() {
                if i == fi || i == ti || Some(i) == ri {
                    continue;
                }
                if let Some(v) = cell_prop(data, r) {
                    edge.props.insert(spec.name.clone(), v);
                }
            }
            g.add_edge(edge);
        }
        Ok(g)
    }

    /// Edges as a table: `from`, `rel`, `to`, then every property that appears.
    ///
    /// Round-trips through [`Graph::from_edge_frame`], so a graph can be
    /// charted, exported, and given provenance like any other dataset.
    pub fn to_edge_frame(&self) -> Result<Frame> {
        let n = self.edges.len();
        let mut from = Vec::with_capacity(n);
        let mut rel = Vec::with_capacity(n);
        let mut to = Vec::with_capacity(n);
        for e in &self.edges {
            from.push(Some(e.from.clone()));
            rel.push(Some(e.rel.clone()));
            to.push(Some(e.to.clone()));
        }
        let mut columns = vec![
            (ColumnSpec::new("from", ColumnDtype::Str), ColumnData::Str(from)),
            (ColumnSpec::new("rel", ColumnDtype::Str), ColumnData::Str(rel)),
            (ColumnSpec::new("to", ColumnDtype::Str), ColumnData::Str(to)),
        ];
        columns.extend(prop_columns(self.edges.iter().map(|e| &e.props), n));
        frame_from_columns(columns)
    }

    /// Nodes as a table: `id`, `labels` (comma-joined), then properties.
    pub fn to_node_frame(&self) -> Result<Frame> {
        let n = self.nodes.len();
        let mut ids = Vec::with_capacity(n);
        let mut labels = Vec::with_capacity(n);
        for node in self.nodes.values() {
            ids.push(Some(node.id.clone()));
            labels.push(Some(node.labels.join(",")));
        }
        let mut columns = vec![
            (ColumnSpec::new("id", ColumnDtype::Str), ColumnData::Str(ids)),
            (ColumnSpec::new("labels", ColumnDtype::Str), ColumnData::Str(labels)),
        ];
        columns.extend(prop_columns(self.nodes.values().map(|n| &n.props), n));
        frame_from_columns(columns)
    }
}

/// A node found by traversal, with how many hops away it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reached {
    pub id: NodeId,
    pub depth: usize,
}

/// Build one column per property key seen across a set of property maps.
///
/// A property absent from a row becomes null rather than a default, because a
/// missing lead time and a lead time of zero are different facts.
fn prop_columns<'a>(
    maps: impl Iterator<Item = &'a Props> + Clone,
    n: usize,
) -> Vec<(ColumnSpec, ColumnData)> {
    let mut keys: BTreeMap<String, ColumnDtype> = BTreeMap::new();
    for m in maps.clone() {
        for (k, v) in m {
            // First sighting wins the type; a later mismatch falls back to text
            // rather than dropping the value.
            keys.entry(k.clone()).or_insert_with(|| v.dtype());
        }
    }

    let mut out = Vec::new();
    for (key, dtype) in keys {
        let consistent = maps
            .clone()
            .all(|m| m.get(&key).is_none_or(|v| v.dtype() == dtype));
        let effective = if consistent { dtype } else { ColumnDtype::Str };

        let data = match effective {
            ColumnDtype::F64 => ColumnData::F64(
                maps.clone().map(|m| m.get(&key).and_then(|v| v.as_f64())).collect(),
            ),
            ColumnDtype::I64 => ColumnData::I64(
                maps.clone()
                    .map(|m| match m.get(&key) {
                        Some(PropValue::I64(v)) => Some(*v),
                        _ => None,
                    })
                    .collect(),
            ),
            ColumnDtype::Bool => ColumnData::Bool(
                maps.clone()
                    .map(|m| match m.get(&key) {
                        Some(PropValue::Bool(v)) => Some(*v),
                        _ => None,
                    })
                    .collect(),
            ),
            ColumnDtype::Str => ColumnData::Str(
                maps.clone().map(|m| m.get(&key).map(prop_to_string)).collect(),
            ),
        };
        debug_assert_eq!(data.len(), n);
        out.push((ColumnSpec::new(key, effective), data));
    }
    out
}

fn prop_to_string(v: &PropValue) -> String {
    match v {
        PropValue::Str(s) => s.clone(),
        PropValue::F64(x) => x.to_string(),
        PropValue::I64(x) => x.to_string(),
        PropValue::Bool(x) => x.to_string(),
    }
}

fn cell_string(data: &ColumnData, r: usize) -> Option<String> {
    match data {
        ColumnData::Str(v) => v.get(r).and_then(|o| o.clone()),
        ColumnData::I64(v) => v.get(r).and_then(|o| *o).map(|x| x.to_string()),
        ColumnData::F64(v) => v.get(r).and_then(|o| *o).map(|x| x.to_string()),
        ColumnData::Bool(v) => v.get(r).and_then(|o| *o).map(|x| x.to_string()),
    }
}

fn cell_prop(data: &ColumnData, r: usize) -> Option<PropValue> {
    match data {
        ColumnData::Str(v) => v.get(r).and_then(|o| o.clone()).map(PropValue::Str),
        ColumnData::I64(v) => v.get(r).and_then(|o| *o).map(PropValue::I64),
        ColumnData::F64(v) => v.get(r).and_then(|o| *o).map(PropValue::F64),
        ColumnData::Bool(v) => v.get(r).and_then(|o| *o).map(PropValue::Bool),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small supply chain:
    ///   ACME  -SUPPLIED_BY->  SKU-1, SKU-2
    ///   GLOBEX -SUPPLIED_BY-> SKU-2
    ///   SKU-1 -CONTAINS->     PART-A
    fn chain() -> Graph {
        let mut g = Graph::new();
        g.add_node(Node::new("ACME").with_label("Supplier").with_prop("region", "US"));
        g.add_node(Node::new("GLOBEX").with_label("Supplier").with_prop("region", "EU"));
        g.add_node(Node::new("SKU-1").with_label("SKU").with_prop("price", 12.5));
        g.add_node(Node::new("SKU-2").with_label("SKU").with_prop("price", 4.0));
        g.add_node(Node::new("PART-A").with_label("Part"));

        g.add_edge(Edge::new("ACME", "SUPPLIED_BY", "SKU-1").with_prop("lead_days", 14i64));
        g.add_edge(Edge::new("ACME", "SUPPLIED_BY", "SKU-2").with_prop("lead_days", 21i64));
        g.add_edge(Edge::new("GLOBEX", "SUPPLIED_BY", "SKU-2").with_prop("lead_days", 30i64));
        g.add_edge(Edge::new("SKU-1", "CONTAINS", "PART-A"));
        g
    }

    #[test]
    fn nodes_and_edges_are_indexed_on_insert() {
        let g = chain();
        assert_eq!(g.node_count(), 5);
        assert_eq!(g.edge_count(), 4);
        assert_eq!(g.nodes_with_label("Supplier").len(), 2);
        assert_eq!(g.nodes_with_label("SKU").len(), 2);
        assert_eq!(
            g.relation_types().into_iter().collect::<Vec<_>>(),
            vec!["CONTAINS", "SUPPLIED_BY"]
        );
    }

    #[test]
    fn an_edge_creates_missing_endpoints() {
        // Warehouse exports arrive as edge lists with no separate node pass.
        let mut g = Graph::new();
        g.add_edge(Edge::new("A", "SUPPLIED_BY", "B"));
        assert_eq!(g.node_count(), 2);
        assert!(g.node("A").is_some());
    }

    #[test]
    fn re_adding_a_node_does_not_leave_a_stale_label() {
        let mut g = Graph::new();
        g.add_node(Node::new("X").with_label("Draft"));
        g.add_node(Node::new("X").with_label("Final"));
        assert!(g.nodes_with_label("Draft").is_empty(), "old label must be dropped");
        assert_eq!(g.nodes_with_label("Final").len(), 1);
    }

    #[test]
    fn direction_and_relation_filter_neighbours() {
        let g = chain();
        assert_eq!(g.neighbors("SKU-2", Direction::In, Some("SUPPLIED_BY")).len(), 2);
        assert!(g.neighbors("SKU-2", Direction::Out, Some("SUPPLIED_BY")).is_empty());
        assert_eq!(g.neighbors("ACME", Direction::Out, None).len(), 2);
        // A relation filter must exclude the other relation entirely.
        assert!(g.neighbors("SKU-1", Direction::Out, Some("SUPPLIED_BY")).is_empty());
        assert_eq!(g.neighbors("SKU-1", Direction::Out, Some("CONTAINS")).len(), 1);
    }

    #[test]
    fn traversal_is_depth_bounded_and_reports_hops() {
        let g = chain();
        let one = g.traverse("ACME", Direction::Out, None, 1);
        assert_eq!(one.len(), 2, "SKU-1 and SKU-2");
        assert!(one.iter().all(|r| r.depth == 1));

        let two = g.traverse("ACME", Direction::Out, None, 2);
        assert!(two.iter().any(|r| r.id == "PART-A" && r.depth == 2));
        // Bounding is real, not cosmetic.
        assert!(!one.iter().any(|r| r.id == "PART-A"));
    }

    #[test]
    fn shortest_path_walks_the_chain() {
        let g = chain();
        let p = g.shortest_path("ACME", "PART-A", None).unwrap();
        assert_eq!(p, vec!["ACME", "SKU-1", "PART-A"]);
        assert_eq!(g.shortest_path("ACME", "ACME", None).unwrap(), vec!["ACME"]);
        assert!(g.shortest_path("GLOBEX", "PART-A", None).is_none(), "no route exists");
        assert!(g.shortest_path("ACME", "NOPE", None).is_none(), "unknown node");
    }

    #[test]
    fn sole_sourced_finds_the_single_point_of_failure() {
        let g = chain();
        let sole = g.sole_sourced("SUPPLIED_BY");
        // SKU-1 has one supplier; SKU-2 has two. PART-A has none of this relation.
        assert!(sole.iter().any(|id| *id == "SKU-1"));
        assert!(!sole.iter().any(|id| *id == "SKU-2"));
    }

    #[test]
    fn unsourced_finds_the_gaps() {
        let g = chain();
        let un = g.unsourced("SUPPLIED_BY");
        assert!(un.iter().any(|id| *id == "PART-A"));
        assert!(un.iter().any(|id| *id == "ACME"), "a supplier is itself unsourced here");
        assert!(!un.iter().any(|id| *id == "SKU-1"));
    }

    #[test]
    fn impact_of_losing_a_supplier_walks_downstream() {
        let g = chain();
        let reached = g.impact_of_losing("PART-A", "CONTAINS");
        let hit: Vec<&str> = reached.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(hit, vec!["SKU-1"], "losing the part takes the SKU that contains it");
    }

    #[test]
    fn cycles_are_detected_and_absent_when_there_are_none() {
        assert!(chain().cycles(None).is_empty());

        let mut g = Graph::new();
        g.add_edge(Edge::new("A", "CONTAINS", "B"));
        g.add_edge(Edge::new("B", "CONTAINS", "C"));
        g.add_edge(Edge::new("C", "CONTAINS", "A"));
        let cycles = g.cycles(Some("CONTAINS"));
        assert!(!cycles.is_empty(), "a circular bill of materials must surface");
        assert_eq!(cycles[0].len(), 3);
    }

    #[test]
    fn a_deep_chain_does_not_blow_the_stack() {
        // Iterative DFS, so depth is bounded by heap not stack.
        let mut g = Graph::new();
        for i in 0..20_000 {
            g.add_edge(Edge::new(format!("n{i}"), "CONTAINS", format!("n{}", i + 1)));
        }
        assert!(g.cycles(Some("CONTAINS")).is_empty());
    }

    #[test]
    fn match_pattern_reads_like_a_cypher_hop() {
        let g = chain();
        // (:Supplier)-[:SUPPLIED_BY]->(:SKU)
        let hits = g.match_pattern(Some("Supplier"), Some("SUPPLIED_BY"), Some("SKU"));
        assert_eq!(hits.len(), 3);
        // A label that matches nothing yields nothing rather than everything.
        assert!(g.match_pattern(Some("Nope"), None, None).is_empty());
    }

    #[test]
    fn subgraph_never_leaves_a_dangling_edge() {
        let g = chain();
        let ids: BTreeSet<NodeId> = ["ACME".to_string(), "SKU-1".to_string()].into();
        let sub = g.subgraph(&ids);
        assert_eq!(sub.node_count(), 2);
        assert_eq!(sub.edge_count(), 1, "SKU-1 -> PART-A is dropped, PART-A is outside");
    }

    #[test]
    fn an_edge_list_table_becomes_a_graph() {
        let f = frame_from_columns(vec![
            (
                ColumnSpec::new("supplier", ColumnDtype::Str),
                ColumnData::Str(vec![Some("ACME".into()), Some("GLOBEX".into())]),
            ),
            (
                ColumnSpec::new("sku", ColumnDtype::Str),
                ColumnData::Str(vec![Some("SKU-1".into()), Some("SKU-2".into())]),
            ),
            (
                ColumnSpec::new("lead_days", ColumnDtype::I64),
                ColumnData::I64(vec![Some(14), Some(30)]),
            ),
        ])
        .unwrap();

        let g = Graph::from_edge_frame(&f, "supplier", "sku", None, "SUPPLIED_BY").unwrap();
        assert_eq!(g.node_count(), 4);
        assert_eq!(g.edge_count(), 2);
        // Remaining columns survive as edge properties.
        let e = g.incident("SKU-1", Direction::In, None)[0];
        assert_eq!(e.prop("lead_days"), Some(&PropValue::I64(14)));
        assert_eq!(e.rel, "SUPPLIED_BY");
    }

    #[test]
    fn a_null_endpoint_is_skipped_rather_than_invented() {
        let f = frame_from_columns(vec![
            (
                ColumnSpec::new("a", ColumnDtype::Str),
                ColumnData::Str(vec![Some("X".into()), None]),
            ),
            (
                ColumnSpec::new("b", ColumnDtype::Str),
                ColumnData::Str(vec![Some("Y".into()), Some("Z".into())]),
            ),
        ])
        .unwrap();
        let g = Graph::from_edge_frame(&f, "a", "b", None, "R").unwrap();
        assert_eq!(g.edge_count(), 1, "the null-endpoint row is not an edge");
    }

    #[test]
    fn a_missing_edge_column_is_a_named_error() {
        let f = frame_from_columns(vec![(
            ColumnSpec::new("a", ColumnDtype::Str),
            ColumnData::Str(vec![Some("X".into())]),
        )])
        .unwrap();
        let err = Graph::from_edge_frame(&f, "a", "nope", None, "R").unwrap_err();
        assert!(format!("{err}").contains("nope"));
    }

    #[test]
    fn a_graph_round_trips_through_its_edge_table() {
        let g = chain();
        let f = g.to_edge_frame().unwrap();
        assert_eq!(f.n_rows(), 4);

        let back = Graph::from_edge_frame(&f, "from", "to", Some("rel"), "R").unwrap();
        assert_eq!(back.edge_count(), g.edge_count());
        assert_eq!(back.node_count(), g.node_count());
        // Relations and properties survive the round-trip.
        assert_eq!(back.relation_types(), g.relation_types());
        let e = back.incident("SKU-1", Direction::In, Some("SUPPLIED_BY"))[0];
        assert_eq!(e.prop("lead_days"), Some(&PropValue::I64(14)));
    }

    #[test]
    fn a_property_missing_on_some_edges_is_null_not_zero() {
        // SKU-1 -CONTAINS-> PART-A has no lead_days; a default would be a lie.
        let f = chain().to_edge_frame().unwrap();
        match f.column("lead_days").unwrap() {
            ColumnData::I64(v) => {
                assert_eq!(v.iter().filter(|o| o.is_none()).count(), 1);
                assert_eq!(v.iter().filter(|o| o.is_some()).count(), 3);
            }
            other => panic!("expected i64 column, got {:?}", other.dtype()),
        }
    }

    #[test]
    fn nodes_export_with_labels_and_properties() {
        let f = chain().to_node_frame().unwrap();
        assert_eq!(f.n_rows(), 5);
        assert!(f.column("labels").is_some());
        assert!(f.column("region").is_some(), "node properties become columns");
        assert!(f.column("price").is_some());
    }
}

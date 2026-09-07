//! # Reference resolution
//!
//! Turns the `Reference` instances in a Space's `Website` service into typed
//! values, reading the live datamodel rather than the files on disk.
//!
//! ## The one rule
//!
//! **A failed reference fails the publish.** Nothing here emits `null` and
//! nothing carries a previous value forward. The failure this whole feature
//! exists to prevent is a website confidently displaying a stale number, and a
//! resolver that silently degrades reintroduces that failure in a new place
//! where nobody is looking for it.
//!
//! The corollary is that every error has to be actionable. An error names the
//! reference, names the source it could not resolve, and offers the nearest
//! candidates in the tree, because "specific_energy: no instance at
//! Workspace/V-Cell/V1/Core/Enclosur (did you mean Enclosure?)" is a fix and
//! "resolution error" is a ticket.
//!
//! ## Live values, and where "live" runs out
//!
//! Section 3.1 of WEBSITE_SERVICE.md requires resolution to read the live
//! datamodel, so a value the engine computed or reconciled is what gets baked.
//! [`WorldDatamodel`] does that: it reads ECS components first.
//!
//! Some authored sections have no ECS representation at all. `[properties]`
//! lands on `BasePart` under different names, unknown sections are dropped
//! entirely once `PendingExtraSections` is drained, and
//! `TomlMaterialProperties::to_component` discards non-numeric
//! `[material.custom]` entries. For those the stored TOML text, read DB first
//! through `active_db`, is not a lesser source: it is the only representation
//! that exists. The component path is tried first and the text path is the
//! fallback, so a value that IS live is always the one that gets baked.
//!
//! ## Table of Contents
//!
//! 1. [`Scalar`] - the typed result of one reference
//! 2. Errors: [`ErrorKind`], [`DataError`], [`ResolveError`], did-you-mean
//! 3. [`Reference`] and its TOML form
//! 4. Dependency order and cycle naming for `expr`
//! 5. [`Datamodel`] - the seam between resolution logic and the ECS
//! 6. [`resolve_all`] - the five kinds
//! 7. [`WorldDatamodel`] - the ECS-backed implementation
//! 8. Path globbing for `count`
//! 9. Tests

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use eustress_common::attributes::{AttributeValue, Attributes, Tags};
use eustress_common::classes::{BasePart, Instance};
use eustress_common::parameters::{
    InstanceParameters, ParameterValue, DEFAULT_PARAMETER_DOMAIN,
};

use crate::space::file_loader::SpaceFileRegistry;

// ============================================================================
// 1. Scalar
// ============================================================================

/// What one reference resolves to.
///
/// Typed rather than stringly so the manifest can emit `value` for arithmetic
/// and `display` for the DOM without a consumer ever parsing one back into the
/// other.
#[derive(Clone, Debug, PartialEq)]
pub enum Scalar {
    /// A finite number. Non-finite values never reach here: NaN and infinity
    /// have no JSON form other than `null`, and emitting `null` is exactly
    /// what the one rule forbids.
    Number(f64),
    /// A string, such as a `[material.custom]` role or a material name.
    Text(String),
    /// A boolean, such as `properties.anchored`.
    Bool(bool),
}

impl Scalar {
    /// The number behind this value, when there is one. `expr` operands and
    /// `measure` conversions need it; text and booleans have none.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Scalar::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Name of the underlying type, for type-mismatch messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Scalar::Number(_) => "number",
            Scalar::Text(_) => "text",
            Scalar::Bool(_) => "boolean",
        }
    }
}

// ============================================================================
// 2. Errors
// ============================================================================

/// Machine-readable discriminator so a caller can tell an authoring mistake
/// from a capability the engine does not have yet.
///
/// Every variant fails the publish. The distinction is for the operator, not
/// for control flow: there is no variant that means "skip this one".
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The `_instance.toml` did not parse, or is missing `[reference]`.
    Parse,
    /// `kind` is not one of the five.
    UnknownKind,
    /// A field this kind requires is missing.
    MissingField,
    /// Two references claim the same manifest key.
    DuplicateKey,
    /// No instance at the referenced path.
    NoInstance,
    /// The instance exists but has no such section.
    NoSection,
    /// The section exists but has no such field.
    NoField,
    /// The field exists but is not a value a manifest can carry.
    TypeMismatch,
    /// No experiment matches `run_label`.
    NoRun,
    /// The run exists but does not carry the requested value.
    NoRunValue,
    /// Telemetry needed for `at = "at_cycle:<n>"` is missing or too short.
    NoTelemetry,
    /// The `expr` graph contains a cycle.
    Cycle,
    /// An `expr` names something that is not a reference key.
    UnknownOperand,
    /// Arithmetic failed, or produced something with no representable unit.
    BadExpression,
    /// The `format` string is not one this engine can apply.
    BadFormat,
    /// The result is NaN or infinite, so it has no JSON number form.
    NotFinite,
    /// A `count` filter that is not understood.
    BadFilter,
    /// A `measure` source or axis that is not understood.
    BadMeasure,
    /// A unit string that is not one the engine converts.
    BadUnit,
    /// A shape the resolver recognises but does not yet implement. Still fails
    /// the publish, deliberately: a silent skip is the stale-number failure.
    NotImplemented,
}

/// A failure raised by the datamodel, before it knows which reference asked.
///
/// [`ResolveError::from_data`] attaches the reference key and source.
#[derive(Clone, Debug)]
pub struct DataError {
    /// What went wrong, phrased as a sentence fragment.
    pub reason: String,
    /// Nearest things in the tree, for did-you-mean.
    pub candidates: Vec<String>,
    /// Machine-readable discriminator.
    pub kind: ErrorKind,
}

impl DataError {
    /// A failure with no suggestions available.
    pub fn new(kind: ErrorKind, reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            candidates: Vec::new(),
            kind,
        }
    }

    /// A failure that can point at nearby names.
    pub fn with_candidates(
        kind: ErrorKind,
        reason: impl Into<String>,
        candidates: Vec<String>,
    ) -> Self {
        Self {
            reason: reason.into(),
            candidates,
            kind,
        }
    }
}

/// A reference that could not be resolved, with everything an author needs to
/// fix it without opening a debugger.
#[derive(Clone, Debug)]
pub struct ResolveError {
    /// The manifest key, or the file path when the failure happened before a
    /// key could be read.
    pub key: String,
    /// The `source` string that failed, empty when the failure is structural.
    pub source: String,
    /// What went wrong.
    pub reason: String,
    /// Nearest candidates in the tree, at most three.
    pub candidates: Vec<String>,
    /// Machine-readable discriminator.
    pub kind: ErrorKind,
}

impl ResolveError {
    /// A failure attributable to one reference.
    pub fn new(key: impl Into<String>, kind: ErrorKind, reason: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            source: String::new(),
            reason: reason.into(),
            candidates: Vec::new(),
            kind,
        }
    }

    /// Attach the source string that failed.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    /// Lift a datamodel failure into a reference failure.
    pub fn from_data(key: &str, source: &str, err: DataError) -> Self {
        Self {
            key: key.to_string(),
            source: source.to_string(),
            reason: err.reason,
            candidates: err.candidates,
            kind: err.kind,
        }
    }
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.key, self.reason)?;
        match self.candidates.len() {
            0 => Ok(()),
            1 => write!(f, " (did you mean {}?)", self.candidates[0]),
            n => {
                let head = self.candidates[..n - 1].join(", ");
                write!(f, " (did you mean {} or {}?)", head, self.candidates[n - 1])
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// Longest candidate list a message carries. Three names help; ten are noise.
const MAX_CANDIDATES: usize = 3;

/// Upper bound on how many names a fuzzy scan will consider.
///
/// A large Space registers six figures of paths and this runs on the failure
/// path, where the publish has already stopped. The cap keeps a bad path from
/// turning a clear error into a hang.
const MAX_FUZZY_SCAN: usize = 50_000;

/// Nearest names to `needle`, closest first.
///
/// Case-insensitive matching, original casing returned, because an author who
/// typed `enclosure` for `Enclosure` wants to be told the real name.
pub fn did_you_mean<I, S>(needle: &str, haystack: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let needle_lc = needle.to_ascii_lowercase();
    // Scale the tolerance with the name: two edits on a six-character leaf is
    // a different claim from two edits on a sixty-character path.
    let budget = (needle_lc.chars().count() / 3).max(2);

    let mut scored: Vec<(usize, String)> = Vec::new();
    for (seen, candidate) in haystack.into_iter().enumerate() {
        if seen >= MAX_FUZZY_SCAN {
            break;
        }
        let raw = candidate.as_ref();
        let lc = raw.to_ascii_lowercase();
        // Cheap reject before the quadratic part.
        if lc.chars().count().abs_diff(needle_lc.chars().count()) > budget {
            continue;
        }
        let d = levenshtein(&needle_lc, &lc, budget);
        if d <= budget {
            scored.push((d, raw.to_string()));
        }
    }

    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    scored.dedup_by(|a, b| a.1 == b.1);
    scored
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|(_, s)| s)
        .collect()
}

/// Edit distance with an early exit once every cell in a row exceeds `budget`.
///
/// The early exit is what makes scanning a whole Space affordable: most
/// candidates are rejected after one or two rows.
fn levenshtein(a: &str, b: &str, budget: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];

    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        let mut row_min = cur[0];
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
            row_min = row_min.min(cur[j + 1]);
        }
        if row_min > budget {
            return budget + 1;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ============================================================================
// 3. Reference
// ============================================================================

/// Where a value comes from. Five, because the numbers a site quotes come from
/// five different places, and a design that only handles scalars solves half
/// the problem.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ReferenceKind {
    /// A property on one instance.
    Instance,
    /// A published simulation value from a named run.
    Sim,
    /// Entities matching a path glob.
    Count,
    /// A geometric measure over a subtree.
    Measure,
    /// An expression over other reference keys.
    Expr,
}

impl ReferenceKind {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "instance" => Some(ReferenceKind::Instance),
            "sim" => Some(ReferenceKind::Sim),
            "count" => Some(ReferenceKind::Count),
            "measure" => Some(ReferenceKind::Measure),
            "expr" => Some(ReferenceKind::Expr),
            _ => None,
        }
    }

    /// The disk spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            ReferenceKind::Instance => "instance",
            ReferenceKind::Sim => "sim",
            ReferenceKind::Count => "count",
            ReferenceKind::Measure => "measure",
            ReferenceKind::Expr => "expr",
        }
    }

    /// The basis these kinds imply on their own.
    ///
    /// `count` and `measure` are self-describing: a count was counted and a
    /// measure was measured, and there is no other honest answer. The other
    /// three are not, so [`Reference::from_toml_str`] requires the author to
    /// declare one. A figure without its basis is not a figure, and guessing
    /// "derived" for something that was measured on a rig would be a guess a
    /// reader could not see.
    fn implied_basis(self) -> Option<&'static str> {
        match self {
            ReferenceKind::Count => Some("counted"),
            ReferenceKind::Measure => Some("measured"),
            _ => None,
        }
    }
}

/// One authored `Reference` instance.
#[derive(Clone, Debug)]
pub struct Reference {
    /// Manifest key. Defaults to the instance name; an explicit `key`
    /// overrides it.
    pub key: String,
    /// Which of the five kinds.
    pub kind: ReferenceKind,
    /// The kind-specific source string.
    pub source: String,
    /// Human label. Defaults to the key.
    pub label: String,
    /// Unit for display. Presentation metadata, never fed to arithmetic.
    pub unit: Option<String>,
    /// Format string applied to produce `display`.
    pub format: Option<String>,
    /// Where the number came from: measured, simulated, derived, counted.
    pub basis: String,
    /// `sim` only. Names the run, so a figure on a website is reproducible.
    pub run_label: Option<String>,
    /// `sim` only. `final` | `min` | `max` | `mean` | `at_cycle:<n>`.
    pub at: String,
    /// `sim` only, for `at_cycle:<n>`: the telemetry key holding the cycle
    /// index.
    pub cycle_key: Option<String>,
    /// `count` only. `class_name = X`, `class_name != X`, or `tag = X`.
    pub filter: Option<String>,
    /// `measure` only. `x` | `y` | `z` | `volume` | `surface`.
    pub axis: Option<String>,
    /// Space-relative path of the file this came from, for error messages.
    pub origin: String,
}

impl Reference {
    /// Parse one `_instance.toml`.
    ///
    /// `Ok(None)` means the file is not a `Reference` and the caller should
    /// move on: a `Website` folder may hold notes or a stray instance, and
    /// failing a publish over one would be the resolver inventing a rule.
    pub fn from_toml_str(origin: &str, text: &str) -> Result<Option<Reference>, ResolveError> {
        let parse_err = |reason: String| ResolveError::new(origin, ErrorKind::Parse, reason);

        let mut doc: toml::Value = text
            .parse()
            .map_err(|e: toml::de::Error| parse_err(format!("{origin} did not parse as TOML: {e}")))?;
        // Same normalisation the loader applies, so a PascalCase file left
        // over from the aborted migration reads the same as a fresh one.
        eustress_common::class_schema::normalise_keys(&mut doc);

        // The engine writes instances as [metadata] + [attributes]: see the
        // fallback writer in space_ops.rs and the saver in instance_loader.rs,
        // and every sibling class schema. [properties] and [reference] are
        // accepted as a fallback so a hand-authored file still loads, but
        // [metadata]/[attributes] is the shape the engine round-trips.
        let class_name = doc
            .get("metadata")
            .and_then(|m| m.get("class_name"))
            .or_else(|| doc.get("properties").and_then(|p| p.get("class_name")))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if class_name != "Reference" {
            return Ok(None);
        }

        // Name comes from the folder when the file does not carry one, which is
        // the normal case: the instance directory name IS the manifest key.
        let name = doc
            .get("metadata")
            .and_then(|m| m.get("name"))
            .or_else(|| doc.get("properties").and_then(|p| p.get("name")))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                // `origin` is the Space-relative path, e.g.
                // "Website/specific_energy/_instance.toml". The instance
                // FOLDER is the manifest key, which is what the spec means by
                // "key defaults to the instance name".
                origin
                    .rsplit('/')
                    .nth(1)
                    .unwrap_or_default()
                    .to_string()
            });

        // A file that declares itself a Reference but carries no fields is an
        // ERROR, never a skip. Skipping is how forty authored references
        // publish as zero with nothing on screen to say so.
        let section = doc
            .get("attributes")
            .and_then(|v| v.as_table())
            .or_else(|| doc.get("reference").and_then(|v| v.as_table()))
            .ok_or_else(|| {
                parse_err(format!(
                    "{origin} declares class_name = \"Reference\" but has no [attributes] section"
                ))
            })?;

        let text_of = |field: &str| -> Option<String> {
            section
                .get(field)
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };

        // An explicit key wins; otherwise the instance name is the key, which
        // is what makes the common case zero configuration.
        let key = text_of("key").unwrap_or(name);
        if key.is_empty() {
            return Err(parse_err(format!(
                "{origin} has no key: set [properties] name, or [reference] key"
            )));
        }

        let kind_text = text_of("kind").ok_or_else(|| {
            ResolveError::new(
                &key,
                ErrorKind::MissingField,
                "no kind: one of instance, sim, count, measure or expr",
            )
        })?;
        let kind = ReferenceKind::parse(&kind_text).ok_or_else(|| ResolveError {
            key: key.clone(),
            source: String::new(),
            reason: format!("unknown kind \"{kind_text}\""),
            candidates: did_you_mean(
                &kind_text,
                ["instance", "sim", "count", "measure", "expr"],
            ),
            kind: ErrorKind::UnknownKind,
        })?;

        let source = text_of("source").ok_or_else(|| {
            ResolveError::new(key.as_str(), ErrorKind::MissingField, "no source")
        })?;

        let basis = match text_of("basis") {
            Some(b) => b,
            None => match kind.implied_basis() {
                Some(b) => b.to_string(),
                None => {
                    return Err(ResolveError::new(
                        &key,
                        ErrorKind::MissingField,
                        format!(
                            "no basis: a {} reference must declare where its number came from \
                             (measured, simulated, derived or authored)",
                            kind.as_str()
                        ),
                    )
                    .with_source(source))
                }
            },
        };

        if kind == ReferenceKind::Sim && text_of("run_label").is_none() {
            return Err(ResolveError::new(
                &key,
                ErrorKind::MissingField,
                "no run_label: a sim reference must name the run it came from, or the \
                 number on the website cannot be reproduced",
            )
            .with_source(source));
        }

        Ok(Some(Reference {
            label: text_of("label").unwrap_or_else(|| key.clone()),
            key,
            kind,
            source,
            unit: text_of("unit"),
            format: text_of("format"),
            basis,
            run_label: text_of("run_label"),
            at: text_of("at").unwrap_or_else(|| "final".to_string()),
            cycle_key: text_of("cycle_key"),
            filter: text_of("filter"),
            axis: text_of("axis"),
            origin: origin.to_string(),
        }))
    }
}

// ============================================================================
// 4. Dependency order
// ============================================================================

/// True for the character set `eustress_cad::expr` tokenises as an identifier.
fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Every identifier in an expression, in order of appearance.
///
/// Deliberately the same rule the expression evaluator uses, so an operand
/// this function finds is one the evaluator will look for, and an operand it
/// misses is one the evaluator would reject anyway.
pub fn expression_operands(src: &str) -> Vec<String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if is_ident_start(chars[i]) {
            let start = i;
            while i < chars.len() && is_ident_char(chars[i]) {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
        } else {
            i += 1;
        }
    }
    out
}

/// Order references so every `expr` runs after everything it names.
///
/// Returns indices into `refs`. A cycle fails, and the message names the loop,
/// because "cyclic dependency" leaves an author reading twenty files.
pub fn dependency_order(refs: &[Reference]) -> Result<Vec<usize>, ResolveError> {
    let mut by_key: HashMap<&str, usize> = HashMap::new();
    for (i, r) in refs.iter().enumerate() {
        if let Some(prev) = by_key.insert(r.key.as_str(), i) {
            return Err(ResolveError::new(
                &r.key,
                ErrorKind::DuplicateKey,
                format!(
                    "two references claim this key: {} and {}",
                    refs[prev].origin, r.origin
                ),
            ));
        }
    }

    // Edges point from a dependency to the expression that consumes it.
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); refs.len()];
    let mut indegree = vec![0usize; refs.len()];
    for (i, r) in refs.iter().enumerate() {
        if r.kind != ReferenceKind::Expr {
            continue;
        }
        let mut seen = HashSet::new();
        for operand in expression_operands(&r.source) {
            let Some(&d) = by_key.get(operand.as_str()) else {
                // Unknown operands are caught here rather than deep inside the
                // evaluator, so the message can suggest a real key.
                let keys: Vec<&str> = refs.iter().map(|x| x.key.as_str()).collect();
                return Err(ResolveError {
                    key: r.key.clone(),
                    source: r.source.clone(),
                    reason: format!("\"{operand}\" is not a reference key in this namespace"),
                    candidates: did_you_mean(&operand, keys),
                    kind: ErrorKind::UnknownOperand,
                });
            };
            if d == i {
                return Err(ResolveError {
                    key: r.key.clone(),
                    source: r.source.clone(),
                    reason: format!("reference cycle: {} refers to itself", r.key),
                    candidates: Vec::new(),
                    kind: ErrorKind::Cycle,
                });
            }
            if seen.insert(d) {
                deps[d].push(i);
                indegree[i] += 1;
            }
        }
    }

    let mut queue: Vec<usize> = (0..refs.len()).filter(|i| indegree[*i] == 0).collect();
    queue.sort_unstable();
    let mut order = Vec::with_capacity(refs.len());
    let mut head = 0;
    while head < queue.len() {
        let n = queue[head];
        head += 1;
        order.push(n);
        for &next in &deps[n] {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                queue.push(next);
            }
        }
    }

    if order.len() != refs.len() {
        let remaining: Vec<usize> = (0..refs.len()).filter(|i| indegree[*i] > 0).collect();
        let loop_path = name_cycle(refs, &deps, &remaining);
        let culprit = loop_path
            .first()
            .and_then(|name| refs.iter().find(|r| &r.key == name))
            .map(|r| (r.key.clone(), r.source.clone()))
            .unwrap_or_default();
        return Err(ResolveError {
            key: culprit.0,
            source: culprit.1,
            reason: format!("reference cycle: {}", loop_path.join(" -> ")),
            candidates: Vec::new(),
            kind: ErrorKind::Cycle,
        });
    }

    Ok(order)
}

/// Walk the unresolved subgraph until a node repeats, and report that loop.
///
/// Naming the actual loop is the point. Reporting the whole strongly connected
/// component would be correct and useless; a path an author can read top to
/// bottom is what turns the error into an edit.
fn name_cycle(refs: &[Reference], deps: &[Vec<usize>], remaining: &[usize]) -> Vec<String> {
    let live: HashSet<usize> = remaining.iter().copied().collect();
    let Some(&start) = remaining.first() else {
        return Vec::new();
    };

    // Annotated: `refs[*i]` below needs the element type settled to pick a
    // slice `Index` impl, and `push` is too late to help it.
    let mut path: Vec<usize> = Vec::new();
    let mut position: HashMap<usize, usize> = HashMap::new();
    let mut node = start;
    loop {
        if let Some(&first_seen) = position.get(&node) {
            let mut names: Vec<String> = path[first_seen..]
                .iter()
                .map(|i| refs[*i].key.clone())
                .collect();
            // Close the loop so the last name repeats the first: a reader
            // should not have to infer the wrap-around.
            names.push(refs[node].key.clone());
            return names;
        }
        position.insert(node, path.len());
        path.push(node);

        match deps[node].iter().copied().find(|n| live.contains(n)) {
            Some(next) => node = next,
            // Every node here has indegree above zero, so a dead end means the
            // graph changed under us. Report what we walked rather than panic.
            None => return path.iter().map(|&i| refs[i].key.clone()).collect(),
        }
    }
}

// ============================================================================
// 5. The datamodel seam
// ============================================================================

/// Everything resolution needs from the running engine.
///
/// A trait rather than a direct `&World` dependency so the ordering,
/// formatting, cycle and did-you-mean logic can be tested without spinning up
/// an App, and so a headless bake can supply a different backing later.
pub trait Datamodel {
    /// One field on one instance. `field` is the part after `#`, dotted.
    fn instance_scalar(&self, path: &str, field: &str) -> Result<Scalar, DataError>;

    /// How many instances match a path glob, after the filter.
    fn count_matching(&self, pattern: &str, filter: Option<&str>) -> Result<f64, DataError>;

    /// World-space axis-aligned bounding box size, in metres, of the subtree
    /// rooted at `path`.
    fn bbox_size_m(&self, path: &str) -> Result<[f64; 3], DataError>;
}

// ============================================================================
// 6. resolve_all
// ============================================================================

/// Resolve every reference, in dependency order.
///
/// `universe_root` is where the run records live, so `sim` references can name
/// their run. All five kinds are attempted; there is no partial success and no
/// skipped reference.
pub fn resolve_all(
    refs: &[Reference],
    data: &dyn Datamodel,
    universe_root: &Path,
) -> Result<BTreeMap<String, Scalar>, ResolveError> {
    let order = dependency_order(refs)?;
    let mut resolved: BTreeMap<String, Scalar> = BTreeMap::new();

    for i in order {
        let r = &refs[i];
        let value = match r.kind {
            ReferenceKind::Instance => resolve_instance(r, data)?,
            ReferenceKind::Sim => resolve_sim(r, universe_root)?,
            ReferenceKind::Count => resolve_count(r, data)?,
            ReferenceKind::Measure => resolve_measure(r, data)?,
            ReferenceKind::Expr => resolve_expr(r, &resolved)?,
        };
        if let Scalar::Number(n) = value {
            if !n.is_finite() {
                return Err(ResolveError::new(
                    &r.key,
                    ErrorKind::NotFinite,
                    format!(
                        "resolved to {n}, which has no JSON number form; the manifest would \
                         have to carry null and a null is a stale number waiting to happen"
                    ),
                )
                .with_source(r.source.as_str()));
            }
        }
        resolved.insert(r.key.clone(), value);
    }

    Ok(resolved)
}

/// `<path>#<section>.<field>`, resolved against the live datamodel.
fn resolve_instance(r: &Reference, data: &dyn Datamodel) -> Result<Scalar, ResolveError> {
    let (path, field) = r.source.split_once('#').ok_or_else(|| {
        ResolveError::new(
            &r.key,
            ErrorKind::Parse,
            "an instance source is <path>#<section>.<field>, and this one has no #",
        )
        .with_source(r.source.as_str())
    })?;
    let (path, field) = (path.trim(), field.trim());
    if path.is_empty() || field.is_empty() {
        return Err(ResolveError::new(
            &r.key,
            ErrorKind::Parse,
            "an instance source needs a path before the # and a field after it",
        )
        .with_source(r.source.as_str()));
    }
    data.instance_scalar(path, field)
        .map_err(|e| ResolveError::from_data(&r.key, &r.source, e))
}

/// Measure prefixes that are named in the design and not built.
///
/// They exist here so an author writing one gets [`ErrorKind::NotImplemented`]
/// rather than a suggestion to check their spelling. Adding one of these later
/// changes no existing reference, because the prefix is part of the source.
const PLANNED_MEASURES: &[&str] = &["hull", "mesh", "convex"];

/// Entities under a path glob, after an optional filter.
fn resolve_count(r: &Reference, data: &dyn Datamodel) -> Result<Scalar, ResolveError> {
    data.count_matching(&r.source, r.filter.as_deref())
        .map(Scalar::Number)
        .map_err(|e| ResolveError::from_data(&r.key, &r.source, e))
}

/// A geometric measure over a subtree, converted into the declared unit.
///
/// Only `bbox:` is supported, and the axis is a property of that box: `x`,
/// `y`, `z`, its `volume`, or its `surface` area. Naming the prefix means a
/// later `hull:` or `mesh:` measure can be added without any existing
/// reference changing meaning.
fn resolve_measure(r: &Reference, data: &dyn Datamodel) -> Result<Scalar, ResolveError> {
    let Some(path) = r.source.strip_prefix("bbox:") else {
        // A prefix that is planned but unbuilt gets its own error, because
        // "not implemented" tells an author to stop and wait while
        // "unsupported" sends them hunting for a typo they did not make.
        let prefix = r.source.split_once(':').map(|(p, _)| p).unwrap_or_default();
        if PLANNED_MEASURES.contains(&prefix) {
            return Err(ResolveError::new(
                &r.key,
                ErrorKind::NotImplemented,
                format!(
                    "{prefix}: measures are not implemented. This engine computes bbox: only, \
                     so the publish stops here rather than baking a bounding box under a name \
                     that promises a hull"
                ),
            )
            .with_source(r.source.as_str()));
        }
        return Err(ResolveError {
            key: r.key.clone(),
            source: r.source.clone(),
            reason: format!(
                "unsupported measure \"{}\": the only measure this engine computes is \
                 bbox:<path>",
                r.source
            ),
            candidates: did_you_mean(prefix, ["bbox", "hull", "mesh", "convex"]),
            kind: ErrorKind::BadMeasure,
        });
    };
    let path = path.trim();

    let axis = r.axis.as_deref().unwrap_or("x");
    let size = data
        .bbox_size_m(path)
        .map_err(|e| ResolveError::from_data(&r.key, &r.source, e))?;

    // The engine is metre-native, so every measure starts in metres and the
    // declared unit is a presentation conversion applied once, here.
    let unit = match r.unit.as_deref() {
        None => eustress_common::units::Unit::Meter,
        Some(u) => eustress_common::units::Unit::from_any(u).ok_or_else(|| {
            ResolveError {
                key: r.key.clone(),
                source: r.source.clone(),
                reason: format!("unit \"{u}\" is not a length this engine converts"),
                candidates: did_you_mean(u, ["m", "cm", "mm", "ft", "in", "studs"]),
                kind: ErrorKind::BadUnit,
            }
        })?,
    };
    let per_metre = 1.0 / unit.to_meters();

    let value = match axis {
        "x" => size[0] * per_metre,
        "y" => size[1] * per_metre,
        "z" => size[2] * per_metre,
        // Volume and surface are the bounding box's, not the meshes'. That is
        // what `bbox:` claims and all it claims; a hull or mesh measure would
        // need its own prefix and its own geometry pass.
        "volume" => size[0] * size[1] * size[2] * per_metre.powi(3),
        "surface" => {
            2.0 * (size[0] * size[1] + size[1] * size[2] + size[2] * size[0])
                * per_metre.powi(2)
        }
        other => {
            return Err(ResolveError {
                key: r.key.clone(),
                source: r.source.clone(),
                reason: format!("unknown axis \"{other}\""),
                candidates: did_you_mean(other, ["x", "y", "z", "volume", "surface"]),
                kind: ErrorKind::BadMeasure,
            })
        }
    };

    Ok(Scalar::Number(value))
}

/// Arithmetic over other reference keys.
///
/// Operands go in as bare unitless numbers. The expression evaluator's unit
/// algebra has no term for `Wh/kg`, so carrying a reference's declared unit
/// through it would reject `energy_wh / pack_mass_kg` outright. `unit` is
/// presentation metadata and is treated as such: it rides along to the
/// manifest and never enters the arithmetic.
fn resolve_expr(r: &Reference, resolved: &BTreeMap<String, Scalar>) -> Result<Scalar, ResolveError> {
    let mut vars: HashMap<String, String> = HashMap::new();
    for operand in expression_operands(&r.source) {
        let Some(value) = resolved.get(&operand) else {
            // dependency_order guarantees the key exists and ran first, so
            // reaching here means the graph and the evaluator disagree.
            return Err(ResolveError::new(
                &r.key,
                ErrorKind::UnknownOperand,
                format!("\"{operand}\" was not resolved before this expression"),
            )
            .with_source(r.source.as_str()));
        };
        let Some(n) = value.as_number() else {
            return Err(ResolveError::new(
                &r.key,
                ErrorKind::TypeMismatch,
                format!(
                    "\"{operand}\" is {}, and arithmetic needs a number",
                    value.type_name()
                ),
            )
            .with_source(r.source.as_str()));
        };
        // Parenthesised so a negative operand cannot re-associate with a
        // neighbouring operator.
        vars.insert(operand, format!("({n})"));
    }

    let quantity = eustress_cad::expr::eval(&r.source, &vars).map_err(|e| {
        ResolveError::new(r.key.as_str(), ErrorKind::BadExpression, e).with_source(r.source.as_str())
    })?;
    Ok(Scalar::Number(quantity.to_si()))
}

// ============================================================================
// 6b. sim: reading a named run
// ============================================================================

/// A published simulation value, taken from a named run record.
///
/// The live `SimValuesResource` is deliberately not consulted. It is the
/// frame-by-frame ECS mirror and carries no run identity at all, so baking
/// from it would put a number on a website that nobody can reproduce, which is
/// the exact thing `run_label` exists to prevent.
fn resolve_sim(r: &Reference, universe_root: &Path) -> Result<Scalar, ResolveError> {
    let run_label = r.run_label.as_deref().unwrap_or_default();
    let exp_dir = universe_root.join(".eustress").join("experiments");
    let run = find_experiment(&exp_dir, run_label)
        .map_err(|e| ResolveError::from_data(&r.key, &r.source, e))?;

    let value = read_run_value(&run, r, universe_root)
        .map_err(|e| ResolveError::from_data(&r.key, &r.source, e))?;
    Ok(Scalar::Number(value))
}

/// The newest run record whose `name` is `run_label`.
///
/// `resolve_experiment_path` in the tools crate takes only `latest`,
/// `latest-1` or an exact filename, none of which is a label. Records carry
/// their label in `name` and their instant in `timestamp`, so the newest run
/// under a label is a scan of the directory, not a filename convention.
fn find_experiment(exp_dir: &Path, run_label: &str) -> Result<serde_json::Value, DataError> {
    let entries = std::fs::read_dir(exp_dir).map_err(|e| {
        DataError::new(
            ErrorKind::NoRun,
            format!(
                "no run records at {}: {e}. Run the experiment before publishing, so the \
                 number on the site is one somebody can reproduce",
                exp_dir.display()
            ),
        )
    })?;

    let mut labels_seen: Vec<String> = Vec::new();
    let mut best: Option<(String, serde_json::Value)> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e != "json").unwrap_or(true) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(doc) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let name = doc.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        if !labels_seen.iter().any(|l| l == name) {
            labels_seen.push(name.to_string());
        }
        if name != run_label {
            continue;
        }
        // `timestamp` is the run's own instant. Sorting on it rather than on
        // file mtime means a restored or copied record still orders correctly.
        let stamp = doc
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let newer = match &best {
            None => true,
            Some((prev, _)) => stamp > *prev,
        };
        if newer {
            best = Some((stamp, doc));
        }
    }

    match best {
        Some((_, doc)) => Ok(doc),
        None => Err(DataError::with_candidates(
            ErrorKind::NoRun,
            format!("no run labelled \"{run_label}\" in {}", exp_dir.display()),
            did_you_mean(run_label, labels_seen),
        )),
    }
}

/// Pull one value out of a run record according to `at`.
fn read_run_value(
    run: &serde_json::Value,
    r: &Reference,
    universe_root: &Path,
) -> Result<f64, DataError> {
    let source = r.source.as_str();

    let known_keys = |field: &str| -> Vec<String> {
        run.get(field)
            .and_then(|v| v.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default()
    };

    match r.at.as_str() {
        "final" => run
            .get("final_values")
            .and_then(|v| v.get(source))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| {
                DataError::with_candidates(
                    ErrorKind::NoRunValue,
                    format!(
                        "run \"{}\" has no final value for \"{source}\"",
                        r.run_label.as_deref().unwrap_or_default()
                    ),
                    did_you_mean(source, known_keys("final_values")),
                )
            }),

        "min" | "max" | "mean" => {
            let stat = r.at.as_str();
            run.get("stats")
                .and_then(|v| v.get(source))
                .and_then(|v| v.get(stat))
                .and_then(|v| v.as_f64())
                .ok_or_else(|| {
                    DataError::with_candidates(
                        ErrorKind::NoRunValue,
                        format!(
                            "run \"{}\" has no {stat} for \"{source}\"; the run's telemetry \
                             may not have sampled it",
                            r.run_label.as_deref().unwrap_or_default()
                        ),
                        did_you_mean(source, known_keys("stats")),
                    )
                })
        }

        at if at.starts_with("at_cycle:") => {
            let n: f64 = at["at_cycle:".len()..].trim().parse().map_err(|_| {
                DataError::new(
                    ErrorKind::Parse,
                    format!("at = \"{at}\" needs a number after at_cycle:"),
                )
            })?;
            read_at_cycle(run, r, universe_root, n)
        }

        other => Err(DataError::with_candidates(
            ErrorKind::Parse,
            format!("unknown at = \"{other}\""),
            did_you_mean(other, ["final", "min", "max", "mean", "at_cycle:<n>"]),
        )),
    }
}

/// The value at the first telemetry sample where the cycle counter reaches
/// `target`.
///
/// A run record holds a final value and per-key statistics, not a series, so
/// this is the one `at` form that has to go back to `telemetry.jsonl`. There
/// is no cycle column in that file: cycle count is just another published sim
/// value whose key depends on what the Space simulates. The author names it,
/// because guessing `battery.cycle_count` would silently read the wrong series
/// on every Space that is not a battery.
fn read_at_cycle(
    run: &serde_json::Value,
    r: &Reference,
    universe_root: &Path,
    target: f64,
) -> Result<f64, DataError> {
    let tele_path = universe_root.join(".eustress").join("telemetry.jsonl");
    let raw = std::fs::read_to_string(&tele_path).map_err(|e| {
        DataError::new(
            ErrorKind::NoTelemetry,
            format!("no telemetry at {}: {e}", tele_path.display()),
        )
    })?;

    let since = run.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");

    let Some(cycle_key) = r.cycle_key.as_deref() else {
        // List what the run actually published so the author picks a real key
        // rather than guessing at one.
        let mut keys: Vec<String> = run
            .get("stats")
            .and_then(|v| v.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        keys.sort();
        keys.truncate(12);
        return Err(DataError::new(
            ErrorKind::MissingField,
            format!(
                "at = \"at_cycle:{target}\" also needs cycle_key, naming the telemetry value \
                 that holds the cycle index. Values published by this run: {}",
                if keys.is_empty() {
                    "none recorded".to_string()
                } else {
                    keys.join(", ")
                }
            ),
        ));
    };

    let mut highest_cycle = f64::NEG_INFINITY;
    for line in raw.lines() {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        // Telemetry is append-only across runs, so entries before this run
        // started belong to a different run and must not answer for it.
        if !since.is_empty() {
            let t = entry.get("t").and_then(|v| v.as_str()).unwrap_or("");
            if t < since {
                continue;
            }
        }
        let Some(values) = entry.get("values").and_then(|v| v.as_object()) else {
            continue;
        };
        let Some(cycle) = values.get(cycle_key).and_then(|v| v.as_f64()) else {
            continue;
        };
        if cycle > highest_cycle {
            highest_cycle = cycle;
        }
        if cycle >= target {
            return values
                .get(&r.source)
                .and_then(|v| v.as_f64())
                .ok_or_else(|| {
                    DataError::with_candidates(
                        ErrorKind::NoRunValue,
                        format!(
                            "telemetry reached cycle {target} but that sample has no \
                             \"{}\"",
                            r.source
                        ),
                        did_you_mean(&r.source, values.keys().cloned().collect::<Vec<_>>()),
                    )
                });
        }
    }

    Err(DataError::new(
        ErrorKind::NoTelemetry,
        if highest_cycle.is_finite() {
            format!(
                "run \"{}\" reached cycle {highest_cycle} and never {target}",
                r.run_label.as_deref().unwrap_or_default()
            )
        } else {
            format!(
                "run \"{}\" published no \"{cycle_key}\" samples, so cycle {target} cannot \
                 be located",
                r.run_label.as_deref().unwrap_or_default()
            )
        },
    ))
}

// ============================================================================
// 7. WorldDatamodel
// ============================================================================

/// Sections that have a live ECS component, and therefore a live value.
///
/// Everything else falls through to the stored TOML text, which for those
/// sections is not a weaker source but the only one: `[properties]` maps onto
/// `BasePart` under different names, and unknown sections are dropped from the
/// ECS entirely once `PendingExtraSections` is drained.
const LIVE_SECTIONS: &[&str] = &[
    "instance",
    "transform",
    "material",
    "thermodynamic",
    "electrochemical",
    "attributes",
    "parameters",
];

/// Extensions a flat legacy instance file can carry.
const FLAT_INSTANCE_SUFFIXES: &[&str] = &[".part.toml", ".glb.toml", ".instance.toml"];

/// The ECS-backed [`Datamodel`].
///
/// Built once per bake. The path index is materialised up front because all
/// three trait methods need it and rebuilding it per reference would turn a
/// twenty-five value manifest into twenty-five full scans of the registry.
pub struct WorldDatamodel<'w> {
    world: &'w World,
    /// Space-relative logical path to the entity it names.
    by_path: HashMap<String, Entity>,
    /// Space-relative logical path to the instance file backing it, for the
    /// stored-text fallback.
    toml_of: HashMap<String, PathBuf>,
    /// Sorted logical paths, for globbing and for did-you-mean.
    paths: Vec<String>,
}

impl<'w> WorldDatamodel<'w> {
    /// Index the Space's instances.
    ///
    /// `space_root` must be the root the [`SpaceFileRegistry`] was populated
    /// against, which is what `SpaceRoot` holds.
    pub fn new(world: &'w World, space_root: &Path) -> Self {
        let mut by_path = HashMap::new();
        let mut toml_of: HashMap<String, PathBuf> = HashMap::new();

        if let Some(registry) = world.get_resource::<SpaceFileRegistry>() {
            for (entity, abs) in registry.entity_to_file.iter() {
                // Only entities that are instances. The registry also indexes
                // materials, scripts and meshes, and a `count` that included
                // those would report a number nobody could explain.
                if world.get::<Instance>(*entity).is_none() {
                    continue;
                }
                let Some(rel) = crate::space::space_source::rel_from_root(space_root, abs) else {
                    continue;
                };
                let logical = logical_path(&rel);
                by_path.insert(logical.clone(), *entity);
                // Prefer an actual `.toml` over the enclosing folder: the
                // folder itself has no text to read, and a folder-form
                // instance is registered under both.
                let already_have_file = toml_of
                    .get(&logical)
                    .map(|p| p.extension().map(|e| e == "toml").unwrap_or(false))
                    .unwrap_or(false);
                if !already_have_file {
                    let chosen = if rel.ends_with(".toml") {
                        abs.clone()
                    } else {
                        abs.join("_instance.toml")
                    };
                    toml_of.insert(logical, chosen);
                }
            }
        }

        let mut paths: Vec<String> = by_path.keys().cloned().collect();
        paths.sort();

        Self {
            world,
            by_path,
            toml_of,
            paths,
        }
    }

    /// How many instances this datamodel can see. Useful in a bake log line.
    pub fn indexed(&self) -> usize {
        self.paths.len()
    }

    fn entity_at(&self, path: &str) -> Result<Entity, DataError> {
        let wanted = logical_path(path);
        if let Some(e) = self.by_path.get(&wanted) {
            return Ok(*e);
        }

        // Nearest full paths first; if nothing is close, try leaf names inside
        // the same parent, which is the shape of a typo an author actually
        // makes.
        let mut candidates = did_you_mean(&wanted, self.paths.iter());
        if candidates.is_empty() {
            let parent = wanted.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            let leaf = wanted.rsplit('/').next().unwrap_or(wanted.as_str());
            let leaf_names: Vec<String> = self
                .paths
                .iter()
                .filter(|p| p.rsplit_once('/').map(|(pp, _)| pp).unwrap_or("") == parent)
                .map(|p| p.rsplit('/').next().unwrap_or(p.as_str()).to_string())
                .collect();
            candidates = did_you_mean(leaf, leaf_names)
                .into_iter()
                .map(|name| {
                    if parent.is_empty() {
                        name
                    } else {
                        format!("{parent}/{name}")
                    }
                })
                .collect();
        }

        Err(DataError::with_candidates(
            ErrorKind::NoInstance,
            format!("no instance at {wanted}"),
            candidates,
        ))
    }

    /// The stored TOML for an instance, DB first and disk second.
    fn instance_text(&self, path: &str) -> Option<String> {
        let logical = logical_path(path);
        let abs = self.toml_of.get(&logical)?;
        if let Some(text) = crate::space::active_db::get_instance_text(abs) {
            return Some(text);
        }
        if let Ok(text) = std::fs::read_to_string(abs) {
            return Some(text);
        }
        // A folder-form instance registered under the folder rather than the
        // marker still has a marker on disk.
        for suffix in FLAT_INSTANCE_SUFFIXES {
            let leaf = logical.rsplit('/').next().unwrap_or(&logical);
            let candidate = abs.with_file_name(format!("{leaf}{suffix}"));
            if let Some(text) = crate::space::active_db::get_instance_text(&candidate) {
                return Some(text);
            }
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                return Some(text);
            }
        }
        None
    }

    fn live_scalar(&self, entity: Entity, section: &str, rest: &str) -> Result<Scalar, DataError> {
        match section {
            "instance" => {
                let Some(inst) = self.world.get::<Instance>(entity) else {
                    return Err(DataError::new(ErrorKind::NoSection, "no instance data"));
                };
                match rest {
                    "name" => Ok(Scalar::Text(inst.name.clone())),
                    "class_name" => Ok(Scalar::Text(inst.class_name.as_str().to_string())),
                    "uuid" => Ok(Scalar::Text(inst.uuid.clone())),
                    "id" => Ok(Scalar::Number(inst.id as f64)),
                    "archivable" => Ok(Scalar::Bool(inst.archivable)),
                    "ai" => Ok(Scalar::Bool(inst.ai)),
                    other => Err(DataError::with_candidates(
                        ErrorKind::NoField,
                        format!("instance has no field \"{other}\""),
                        did_you_mean(
                            other,
                            ["name", "class_name", "uuid", "id", "archivable", "ai"],
                        ),
                    )),
                }
            }

            "transform" => {
                let Some(t) = self.world.get::<Transform>(entity) else {
                    return Err(DataError::new(ErrorKind::NoSection, "no transform"));
                };
                // Hand-built rather than reflected: the manifest needs plain
                // numbers, and Transform's serialized shape is a Bevy
                // implementation detail a website should not inherit.
                let doc = serde_json::json!({
                    "position": { "x": t.translation.x, "y": t.translation.y, "z": t.translation.z },
                    "rotation": { "x": t.rotation.x, "y": t.rotation.y, "z": t.rotation.z, "w": t.rotation.w },
                    "scale": { "x": t.scale.x, "y": t.scale.y, "z": t.scale.z },
                });
                json_scalar(&doc, rest, "transform")
            }

            "material" => {
                use eustress_common::realism::materials::prelude::MaterialProperties;
                let Some(m) = self.world.get::<MaterialProperties>(entity) else {
                    return Err(DataError::new(ErrorKind::NoSection, "no material"));
                };
                let doc = serde_json::to_value(m).map_err(|e| {
                    DataError::new(ErrorKind::TypeMismatch, format!("material unreadable: {e}"))
                })?;
                // The TOML section is `[material.custom]`; the component field
                // is `custom_properties`. The disk spelling is the one authors
                // write and the one the specification uses, so it is the one
                // this accepts.
                let rest = match rest.strip_prefix("custom.") {
                    Some(tail) => format!("custom_properties.{tail}"),
                    None => rest.to_string(),
                };
                json_scalar(&doc, &rest, "material")
            }

            "thermodynamic" => {
                use eustress_common::realism::particles::components::ThermodynamicState;
                let Some(t) = self.world.get::<ThermodynamicState>(entity) else {
                    return Err(DataError::new(ErrorKind::NoSection, "no thermodynamic state"));
                };
                let doc = serde_json::to_value(t).map_err(|e| {
                    DataError::new(
                        ErrorKind::TypeMismatch,
                        format!("thermodynamic state unreadable: {e}"),
                    )
                })?;
                json_scalar(&doc, rest, "thermodynamic")
            }

            "electrochemical" => {
                use eustress_common::realism::particles::components::ElectrochemicalState;
                let Some(e) = self.world.get::<ElectrochemicalState>(entity) else {
                    return Err(DataError::new(
                        ErrorKind::NoSection,
                        "no electrochemical state",
                    ));
                };
                let doc = serde_json::to_value(e).map_err(|e| {
                    DataError::new(
                        ErrorKind::TypeMismatch,
                        format!("electrochemical state unreadable: {e}"),
                    )
                })?;
                json_scalar(&doc, rest, "electrochemical")
            }

            "attributes" => {
                let Some(attrs) = self.world.get::<Attributes>(entity) else {
                    return Err(DataError::new(ErrorKind::NoSection, "no attributes"));
                };
                // An attribute name may itself contain a dot, so try the whole
                // remainder as a name before splitting off a component suffix.
                if let Some(v) = attrs.get(rest) {
                    return attribute_scalar(v, None, rest);
                }
                if let Some((name, comp)) = rest.rsplit_once('.') {
                    if let Some(v) = attrs.get(name) {
                        return attribute_scalar(v, Some(comp), name);
                    }
                }
                Err(DataError::with_candidates(
                    ErrorKind::NoField,
                    format!("no attribute \"{rest}\""),
                    did_you_mean(rest, attrs.values.keys().cloned().collect::<Vec<_>>()),
                ))
            }

            "parameters" => {
                let Some(params) = self.world.get::<InstanceParameters>(entity) else {
                    return Err(DataError::new(ErrorKind::NoSection, "no parameters"));
                };
                // A flat key lands in the default domain; a dotted one names
                // its domain explicitly, which is the same rule the loader
                // applies when it reads `[parameters]` off disk.
                let direct = params.get(DEFAULT_PARAMETER_DOMAIN, rest);
                let scoped = rest
                    .split_once('.')
                    .and_then(|(domain, key)| params.get(domain, key));
                match direct.or(scoped) {
                    Some(v) => parameter_scalar(v, rest),
                    None => Err(DataError::new(
                        ErrorKind::NoField,
                        format!("no parameter \"{rest}\""),
                    )),
                }
            }

            other => Err(DataError::new(
                ErrorKind::NoSection,
                format!("\"{other}\" has no live component"),
            )),
        }
    }
}

impl Datamodel for WorldDatamodel<'_> {
    fn instance_scalar(&self, path: &str, field: &str) -> Result<Scalar, DataError> {
        let entity = self.entity_at(path)?;

        let (section, rest) = field.split_once('.').unwrap_or((field, ""));
        if rest.is_empty() {
            return Err(DataError::new(
                ErrorKind::Parse,
                format!("\"{field}\" names a section but no field inside it"),
            ));
        }

        // Live first, per specification 3.1: a value the engine computed or
        // reconciled is what gets baked.
        let live = if LIVE_SECTIONS.contains(&section) {
            match self.live_scalar(entity, section, rest) {
                Ok(v) => return Ok(v),
                Err(e) => Some(e),
            }
        } else {
            None
        };

        // Then the stored text, which is the only representation for sections
        // the ECS does not model and for values the typed loader drops on the
        // way in, such as a string-valued `[material.custom]` entry.
        let Some(text) = self.instance_text(path) else {
            return Err(live.unwrap_or_else(|| {
                DataError::new(
                    ErrorKind::NoSection,
                    format!("no stored text for {path}, so \"{field}\" cannot be read"),
                )
            }));
        };

        let mut doc: toml::Value = match text.parse() {
            Ok(d) => d,
            Err(e) => {
                return Err(live.unwrap_or_else(|| {
                    DataError::new(ErrorKind::Parse, format!("{path} did not parse: {e}"))
                }))
            }
        };
        eustress_common::class_schema::normalise_keys(&mut doc);

        // The stored-text failure is the one reported: it saw the whole
        // document, so it knows more about what is actually there than a live
        // probe that only knows one component was absent.
        toml_scalar(&doc, field)
    }

    fn count_matching(&self, pattern: &str, filter: Option<&str>) -> Result<f64, DataError> {
        let filter = parse_filter(filter)?;
        let mut n = 0u64;
        for (path, entity) in self.by_path.iter() {
            if !glob_matches(pattern, path) {
                continue;
            }
            if filter.accepts(self.world, *entity) {
                n += 1;
            }
        }
        if n == 0 {
            return Err(DataError::with_candidates(
                ErrorKind::NoInstance,
                format!(
                    "\"{pattern}\" matched no instances. A count of zero is almost always a \
                     wrong path rather than an empty subtree, so it fails rather than \
                     publishing a confident 0"
                ),
                did_you_mean(pattern.trim_end_matches("/**"), self.paths.iter()),
            ));
        }
        Ok(n as f64)
    }

    fn bbox_size_m(&self, path: &str) -> Result<[f64; 3], DataError> {
        let root = self.entity_at(path)?;

        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        let mut found = false;

        // Same walk the selection box draws, so the datasheet and the viewport
        // agree by construction rather than by somebody checking.
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if let (Some(gt), Some(bp)) = (
                self.world.get::<GlobalTransform>(entity),
                self.world.get::<BasePart>(entity),
            ) {
                let m = gt.affine();
                let h = bp.size * 0.5;
                for sx in [-1.0f32, 1.0] {
                    for sy in [-1.0f32, 1.0] {
                        for sz in [-1.0f32, 1.0] {
                            let corner =
                                m.transform_point3(Vec3::new(sx * h.x, sy * h.y, sz * h.z));
                            min = min.min(corner);
                            max = max.max(corner);
                        }
                    }
                }
                found = true;
            }
            if let Some(children) = self.world.get::<Children>(entity) {
                stack.extend(children.iter());
            }
        }

        if !found {
            return Err(DataError::new(
                ErrorKind::BadMeasure,
                format!("{path} has no parts under it, so it has no bounding box"),
            ));
        }

        let size = max - min;
        Ok([size.x as f64, size.y as f64, size.z as f64])
    }
}

/// Strip the marker or the flat suffix so a path in a reference is the path an
/// author sees in the Explorer.
fn logical_path(rel: &str) -> String {
    let rel = rel.trim_matches('/');
    if let Some(stem) = rel.strip_suffix("/_instance.toml") {
        return stem.to_string();
    }
    if rel == "_instance.toml" {
        return String::new();
    }
    for suffix in FLAT_INSTANCE_SUFFIXES {
        if let Some(stem) = rel.strip_suffix(suffix) {
            return stem.to_string();
        }
    }
    rel.to_string()
}

/// Walk a dotted path through a JSON document down to a scalar.
fn json_scalar(doc: &serde_json::Value, field: &str, section: &str) -> Result<Scalar, DataError> {
    let mut cursor = doc;
    for (depth, seg) in field.split('.').enumerate() {
        let Some(next) = cursor.get(seg) else {
            let siblings: Vec<String> = cursor
                .as_object()
                .map(|o| o.keys().cloned().collect())
                .unwrap_or_default();
            let so_far: Vec<&str> = field.split('.').take(depth).collect();
            let where_ = if so_far.is_empty() {
                section.to_string()
            } else {
                format!("{section}.{}", so_far.join("."))
            };
            return Err(DataError::with_candidates(
                ErrorKind::NoField,
                format!("{where_} has no field \"{seg}\""),
                did_you_mean(seg, siblings),
            ));
        };
        cursor = next;
    }

    match cursor {
        serde_json::Value::Number(n) => n.as_f64().map(Scalar::Number).ok_or_else(|| {
            DataError::new(
                ErrorKind::NotFinite,
                format!("{section}.{field} is not a finite number"),
            )
        }),
        serde_json::Value::String(s) => Ok(Scalar::Text(s.clone())),
        serde_json::Value::Bool(b) => Ok(Scalar::Bool(*b)),
        other => Err(DataError::new(
            ErrorKind::TypeMismatch,
            format!(
                "{section}.{field} is {}, and a manifest value has to be a number, a string \
                 or a boolean",
                json_type_name(other)
            ),
        )),
    }
}

fn json_type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "a table",
    }
}

/// Walk a dotted path through a TOML document down to a scalar.
fn toml_scalar(doc: &toml::Value, field: &str) -> Result<Scalar, DataError> {
    let mut cursor = doc;
    for (depth, seg) in field.split('.').enumerate() {
        let Some(next) = cursor.get(seg) else {
            let siblings: Vec<String> = cursor
                .as_table()
                .map(|t| t.keys().cloned().collect())
                .unwrap_or_default();
            let so_far: Vec<&str> = field.split('.').take(depth).collect();
            let where_ = if so_far.is_empty() {
                "the instance".to_string()
            } else {
                so_far.join(".")
            };
            return Err(DataError::with_candidates(
                if depth == 0 {
                    ErrorKind::NoSection
                } else {
                    ErrorKind::NoField
                },
                format!("{where_} has no \"{seg}\""),
                did_you_mean(seg, siblings),
            ));
        };
        cursor = next;
    }

    match cursor {
        toml::Value::Integer(i) => Ok(Scalar::Number(*i as f64)),
        toml::Value::Float(f) => Ok(Scalar::Number(*f)),
        toml::Value::String(s) => Ok(Scalar::Text(s.clone())),
        toml::Value::Boolean(b) => Ok(Scalar::Bool(*b)),
        other => Err(DataError::new(
            ErrorKind::TypeMismatch,
            format!(
                "{field} is {}, and a manifest value has to be a number, a string or a boolean",
                other.type_str()
            ),
        )),
    }
}

/// One attribute, optionally indexed by a vector component.
fn attribute_scalar(
    value: &AttributeValue,
    component: Option<&str>,
    name: &str,
) -> Result<Scalar, DataError> {
    let vec_component = |x: f32, y: f32, z: Option<f32>| -> Result<Scalar, DataError> {
        match component {
            Some("x") => Ok(Scalar::Number(x as f64)),
            Some("y") => Ok(Scalar::Number(y as f64)),
            Some("z") => z.map(|v| Scalar::Number(v as f64)).ok_or_else(|| {
                DataError::new(ErrorKind::NoField, format!("{name} has no z component"))
            }),
            _ => Err(DataError::new(
                ErrorKind::TypeMismatch,
                format!("{name} is a vector; name a component, such as {name}.x"),
            )),
        }
    };

    match value {
        AttributeValue::Number(n) => Ok(Scalar::Number(*n)),
        AttributeValue::Int(i) => Ok(Scalar::Number(*i as f64)),
        AttributeValue::Bool(b) => Ok(Scalar::Bool(*b)),
        AttributeValue::String(s) => Ok(Scalar::Text(s.clone())),
        AttributeValue::Vector2(v) => vec_component(v.x, v.y, None),
        AttributeValue::Vector3(v) => vec_component(v.x, v.y, Some(v.z)),
        AttributeValue::NumberRange { min, max } => match component {
            Some("min") => Ok(Scalar::Number(*min)),
            Some("max") => Ok(Scalar::Number(*max)),
            _ => Err(DataError::new(
                ErrorKind::TypeMismatch,
                format!("{name} is a range; name {name}.min or {name}.max"),
            )),
        },
        other => Err(DataError::new(
            ErrorKind::TypeMismatch,
            format!(
                "attribute {name} is a {} and has no manifest value form",
                other.type_name()
            ),
        )),
    }
}

/// One parameter, narrowed to a manifest-carryable value.
fn parameter_scalar(value: &ParameterValue, name: &str) -> Result<Scalar, DataError> {
    match value {
        ParameterValue::Float(f) => Ok(Scalar::Number(*f)),
        ParameterValue::Int(i) => Ok(Scalar::Number(*i as f64)),
        ParameterValue::Bool(b) => Ok(Scalar::Bool(*b)),
        ParameterValue::String(s) => Ok(Scalar::Text(s.clone())),
        other => Err(DataError::new(
            ErrorKind::TypeMismatch,
            format!(
                "parameter {name} is {} and has no manifest value form",
                other.type_name()
            ),
        )),
    }
}

/// The `count` filter, kept deliberately small.
#[derive(Debug)]
enum CountFilter {
    All,
    ClassIs(String),
    ClassIsNot(String),
    Tagged(String),
}

impl CountFilter {
    fn accepts(&self, world: &World, entity: Entity) -> bool {
        match self {
            CountFilter::All => true,
            CountFilter::ClassIs(want) => world
                .get::<Instance>(entity)
                .map(|i| i.class_name.as_str() == want)
                .unwrap_or(false),
            CountFilter::ClassIsNot(want) => world
                .get::<Instance>(entity)
                .map(|i| i.class_name.as_str() != want)
                .unwrap_or(false),
            CountFilter::Tagged(tag) => world
                .get::<Tags>(entity)
                .map(|t| t.has(tag))
                .unwrap_or(false),
        }
    }
}

fn parse_filter(filter: Option<&str>) -> Result<CountFilter, DataError> {
    let Some(raw) = filter.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(CountFilter::All);
    };
    // `!=` first: splitting on `=` would otherwise leave a trailing `!`.
    if let Some((lhs, rhs)) = raw.split_once("!=") {
        if lhs.trim() == "class_name" {
            return Ok(CountFilter::ClassIsNot(rhs.trim().to_string()));
        }
    }
    if let Some((lhs, rhs)) = raw.split_once('=') {
        match lhs.trim() {
            "class_name" => return Ok(CountFilter::ClassIs(rhs.trim().to_string())),
            "tag" => return Ok(CountFilter::Tagged(rhs.trim().to_string())),
            other => {
                return Err(DataError::with_candidates(
                    ErrorKind::BadFilter,
                    format!("unknown filter field \"{other}\""),
                    did_you_mean(other, ["class_name", "tag"]),
                ))
            }
        }
    }
    Err(DataError::new(
        ErrorKind::BadFilter,
        format!(
            "filter \"{raw}\" is not understood; write class_name = Part, class_name != Folder \
             or tag = <name>"
        ),
    ))
}

// ============================================================================
// 8. Path globbing
// ============================================================================

/// Match a Space-relative path against a `count` pattern.
///
/// Three rules, and one of them differs from a shell on purpose:
///
/// - `**` matches **one or more** path segments. `A/**` therefore means
///   strictly under `A` and never `A` itself, which is what "count the parts
///   in the assembly" has to mean. A zero-or-more `**` would quietly include
///   the assembly folder and put a number one too high on a datasheet.
/// - `*` matches within a single segment and never crosses `/`. Without that,
///   `Workspace/V-Cell/V1/Assembly/*` and `Workspace/*` would count the same
///   things.
/// - `?` matches one character within a segment.
pub fn glob_matches(pattern: &str, path: &str) -> bool {
    let p: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let t: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    segments_match(&p, &t)
}

fn segments_match(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.first() {
        None => path.is_empty(),
        Some(&"**") => (1..=path.len()).any(|take| segments_match(&pattern[1..], &path[take..])),
        Some(seg) => {
            !path.is_empty()
                && segment_matches(seg, path[0])
                && segments_match(&pattern[1..], &path[1..])
        }
    }
}

fn segment_matches(pattern: &str, segment: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = segment.chars().collect();
    chars_match(&p, &s)
}

fn chars_match(pattern: &[char], text: &[char]) -> bool {
    match pattern.first() {
        None => text.is_empty(),
        Some('*') => (0..=text.len()).any(|skip| chars_match(&pattern[1..], &text[skip..])),
        Some('?') => !text.is_empty() && chars_match(&pattern[1..], &text[1..]),
        Some(c) => !text.is_empty() && text[0] == *c && chars_match(&pattern[1..], &text[1..]),
    }
}

// ============================================================================
// 9. Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(key: &str, kind: ReferenceKind, source: &str) -> Reference {
        Reference {
            key: key.to_string(),
            kind,
            source: source.to_string(),
            label: key.to_string(),
            unit: None,
            format: None,
            basis: "derived".to_string(),
            run_label: None,
            at: "final".to_string(),
            cycle_key: None,
            filter: None,
            axis: None,
            origin: format!("Website/{key}/_instance.toml"),
        }
    }

    /// A datamodel that answers from a fixed table, so ordering, cycles and
    /// formatting can be tested without an App.
    struct FakeData {
        values: HashMap<String, Scalar>,
    }

    impl Datamodel for FakeData {
        fn instance_scalar(&self, path: &str, field: &str) -> Result<Scalar, DataError> {
            self.values
                .get(&format!("{path}#{field}"))
                .cloned()
                .ok_or_else(|| {
                    DataError::with_candidates(
                        ErrorKind::NoInstance,
                        format!("no instance at {path}"),
                        did_you_mean(path, self.values.keys().map(|k| {
                            k.split('#').next().unwrap_or(k).to_string()
                        }).collect::<Vec<_>>()),
                    )
                })
        }

        fn count_matching(&self, _pattern: &str, _filter: Option<&str>) -> Result<f64, DataError> {
            Ok(3.0)
        }

        fn bbox_size_m(&self, _path: &str) -> Result<[f64; 3], DataError> {
            Ok([0.3, 0.1, 0.1])
        }
    }

    fn fake(pairs: &[(&str, Scalar)]) -> FakeData {
        FakeData {
            values: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    // ---- parsing ----------------------------------------------------------

    #[test]
    fn a_reference_parses_with_the_documented_shape() {
        let toml = r#"
[properties]
name = "specific_energy"
class_name = "Reference"

[reference]
kind   = "instance"
source = "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg"
label  = "Specific energy, pack level"
unit   = "Wh/kg"
format = "{:.0}"
basis  = "derived"
"#;
        let r = Reference::from_toml_str("Website/specific_energy/_instance.toml", toml)
            .unwrap()
            .expect("a Reference");
        assert_eq!(r.key, "specific_energy");
        assert_eq!(r.kind, ReferenceKind::Instance);
        assert_eq!(r.unit.as_deref(), Some("Wh/kg"));
        assert_eq!(r.format.as_deref(), Some("{:.0}"));
        assert_eq!(r.basis, "derived");
        assert_eq!(r.label, "Specific energy, pack level");
    }

    #[test]
    fn a_non_reference_instance_is_skipped_rather_than_failed() {
        let toml = r#"
[properties]
name = "Notes"
class_name = "Folder"
"#;
        assert!(Reference::from_toml_str("Website/Notes/_instance.toml", toml)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_sim_reference_without_a_run_label_fails() {
        let toml = r#"
[properties]
name = "cycles"
class_name = "Reference"

[reference]
kind = "sim"
source = "battery.capacity_retention"
basis = "simulated"
"#;
        let err = Reference::from_toml_str("Website/cycles/_instance.toml", toml).unwrap_err();
        assert_eq!(err.kind, ErrorKind::MissingField);
        assert!(err.to_string().contains("run_label"));
    }

    #[test]
    fn an_instance_reference_without_a_basis_fails_and_says_so() {
        let toml = r#"
[properties]
name = "mass"
class_name = "Reference"

[reference]
kind = "instance"
source = "Workspace/Pack#material.density"
"#;
        let err = Reference::from_toml_str("Website/mass/_instance.toml", toml).unwrap_err();
        assert_eq!(err.kind, ErrorKind::MissingField);
        assert!(err.to_string().contains("basis"));
    }

    #[test]
    fn count_and_measure_carry_their_own_basis() {
        for (kind, expected) in [("count", "counted"), ("measure", "measured")] {
            let toml = format!(
                r#"
[properties]
name = "x"
class_name = "Reference"

[reference]
kind = "{kind}"
source = "bbox:Workspace/A"
"#
            );
            let r = Reference::from_toml_str("Website/x/_instance.toml", &toml)
                .unwrap()
                .unwrap();
            assert_eq!(r.basis, expected);
        }
    }

    #[test]
    fn an_unknown_kind_suggests_a_real_one() {
        let toml = r#"
[properties]
name = "x"
class_name = "Reference"

[reference]
kind = "instanse"
source = "a#b.c"
basis = "derived"
"#;
        let err = Reference::from_toml_str("Website/x/_instance.toml", toml).unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnknownKind);
        assert!(err.to_string().contains("did you mean instance?"), "{err}");
    }

    // ---- did-you-mean -----------------------------------------------------

    #[test]
    fn a_missing_instance_names_the_nearest_path() {
        let data = fake(&[(
            "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg",
            Scalar::Number(953.0),
        )]);
        let r = reference(
            "specific_energy",
            ReferenceKind::Instance,
            "Workspace/V-Cell/V1/Core/Enclosur#material.custom.wh_per_kg",
        );
        let err = resolve_all(&[r], &data, Path::new(".")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.starts_with("specific_energy: no instance at"), "{msg}");
        assert!(msg.contains("did you mean"), "{msg}");
        assert!(msg.contains("Enclosure"), "{msg}");
    }

    #[test]
    fn did_you_mean_ranks_the_closest_first_and_caps_the_list() {
        let got = did_you_mean(
            "Enclosur",
            [
                "Enclosure",
                "Enclosures",
                "Enclosurexy",
                "Terminal",
                "Busbar",
            ],
        );
        // Distance 1 beats distance 2, and nothing unrelated gets in.
        assert_eq!(got.first().map(String::as_str), Some("Enclosure"));
        assert!(got.len() <= MAX_CANDIDATES);
        assert!(!got.iter().any(|c| c == "Terminal"));
        assert!(!got.iter().any(|c| c == "Busbar"));
    }

    #[test]
    fn did_you_mean_is_case_insensitive_but_returns_the_real_name() {
        let got = did_you_mean("enclosure", ["Enclosure"]);
        assert_eq!(got, vec!["Enclosure".to_string()]);
    }

    #[test]
    fn did_you_mean_stays_silent_when_nothing_is_close() {
        assert!(did_you_mean("Enclosure", ["Terminal", "Busbar"]).is_empty());
    }

    // ---- dependency order and cycles --------------------------------------

    #[test]
    fn expressions_run_after_everything_they_name() {
        let refs = vec![
            reference("specific_energy", ReferenceKind::Expr, "energy_wh / pack_mass_kg"),
            reference("energy_wh", ReferenceKind::Instance, "Workspace/Pack#a.b"),
            reference("pack_mass_kg", ReferenceKind::Instance, "Workspace/Pack#a.c"),
        ];
        let order = dependency_order(&refs).unwrap();
        let pos = |key: &str| {
            order
                .iter()
                .position(|&i| refs[i].key == key)
                .expect("in order")
        };
        assert!(pos("energy_wh") < pos("specific_energy"));
        assert!(pos("pack_mass_kg") < pos("specific_energy"));
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn a_chain_of_expressions_orders_transitively() {
        let refs = vec![
            reference("c", ReferenceKind::Expr, "b * 2"),
            reference("b", ReferenceKind::Expr, "a + 1"),
            reference("a", ReferenceKind::Instance, "Workspace/P#x.y"),
        ];
        let order = dependency_order(&refs).unwrap();
        let keys: Vec<&str> = order.iter().map(|&i| refs[i].key.as_str()).collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn a_cycle_fails_the_publish_and_names_the_loop() {
        let refs = vec![
            reference("a", ReferenceKind::Expr, "b + 1"),
            reference("b", ReferenceKind::Expr, "c + 1"),
            reference("c", ReferenceKind::Expr, "a + 1"),
        ];
        let err = dependency_order(&refs).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Cycle);
        let msg = err.to_string();
        assert!(msg.contains("reference cycle:"), "{msg}");
        // Every member named, and the loop closed so the wrap-around is shown.
        for key in ["a", "b", "c"] {
            assert!(msg.contains(key), "{msg}");
        }
        let arrow_count = msg.matches("->").count();
        assert_eq!(arrow_count, 3, "{msg}");
    }

    #[test]
    fn a_self_reference_is_a_cycle() {
        let refs = vec![reference("a", ReferenceKind::Expr, "a * 2")];
        let err = dependency_order(&refs).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Cycle);
        assert!(err.to_string().contains("refers to itself"));
    }

    #[test]
    fn an_unknown_operand_suggests_a_real_key() {
        let refs = vec![
            reference("ratio", ReferenceKind::Expr, "energy_wh / pack_mas_kg"),
            reference("energy_wh", ReferenceKind::Instance, "Workspace/P#a.b"),
            reference("pack_mass_kg", ReferenceKind::Instance, "Workspace/P#a.c"),
        ];
        let err = dependency_order(&refs).unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnknownOperand);
        assert!(err.to_string().contains("pack_mass_kg"), "{err}");
    }

    #[test]
    fn two_references_cannot_claim_one_key() {
        let mut a = reference("energy_wh", ReferenceKind::Instance, "Workspace/A#x.y");
        let b = reference("energy_wh", ReferenceKind::Instance, "Workspace/B#x.y");
        a.origin = "Website/energy_wh/_instance.toml".to_string();
        let err = dependency_order(&[a, b]).unwrap_err();
        assert_eq!(err.kind, ErrorKind::DuplicateKey);
    }

    // ---- resolution -------------------------------------------------------

    #[test]
    fn an_expression_computes_over_resolved_operands() {
        let data = fake(&[
            ("Workspace/Pack#a.energy", Scalar::Number(3779.0)),
            ("Workspace/Pack#a.mass", Scalar::Number(3.526)),
        ]);
        let refs = vec![
            reference("specific_energy", ReferenceKind::Expr, "energy_wh / pack_mass_kg"),
            reference("energy_wh", ReferenceKind::Instance, "Workspace/Pack#a.energy"),
            reference("pack_mass_kg", ReferenceKind::Instance, "Workspace/Pack#a.mass"),
        ];
        let out = resolve_all(&refs, &data, Path::new(".")).unwrap();
        let got = out["specific_energy"].as_number().unwrap();
        assert!((got - 3779.0 / 3.526).abs() < 1e-9, "{got}");
    }

    #[test]
    fn a_negative_operand_does_not_re_associate() {
        let data = fake(&[("Workspace/P#a.b", Scalar::Number(-5.0))]);
        let refs = vec![
            reference("delta", ReferenceKind::Instance, "Workspace/P#a.b"),
            reference("result", ReferenceKind::Expr, "10 - delta"),
        ];
        let out = resolve_all(&refs, &data, Path::new(".")).unwrap();
        assert_eq!(out["result"].as_number(), Some(15.0));
    }

    #[test]
    fn an_expression_over_text_fails_rather_than_coercing() {
        let data = fake(&[("Workspace/P#a.role", Scalar::Text("cathode".into()))]);
        let refs = vec![
            reference("role", ReferenceKind::Instance, "Workspace/P#a.role"),
            reference("doubled", ReferenceKind::Expr, "role * 2"),
        ];
        let err = resolve_all(&refs, &data, Path::new(".")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::TypeMismatch);
    }

    #[test]
    fn a_non_finite_value_fails_rather_than_becoming_null() {
        // A manifest carrying `null` is the stale-number failure wearing a
        // different hat, so NaN has to stop the publish rather than serialize.
        let data = fake(&[("Workspace/P#a.broken", Scalar::Number(f64::NAN))]);
        let refs = vec![reference("broken", ReferenceKind::Instance, "Workspace/P#a.broken")];
        let err = resolve_all(&refs, &data, Path::new(".")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFinite);
    }

    #[test]
    fn division_by_zero_fails_the_publish() {
        let data = fake(&[
            ("Workspace/P#a.e", Scalar::Number(1.0)),
            ("Workspace/P#a.m", Scalar::Number(0.0)),
        ]);
        let refs = vec![
            reference("e", ReferenceKind::Instance, "Workspace/P#a.e"),
            reference("m", ReferenceKind::Instance, "Workspace/P#a.m"),
            reference("ratio", ReferenceKind::Expr, "e / m"),
        ];
        let err = resolve_all(&refs, &data, Path::new(".")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::BadExpression);
    }

    #[test]
    fn a_measure_converts_out_of_metres() {
        let data = fake(&[]);
        let mut r = reference("length", ReferenceKind::Measure, "bbox:Workspace/Assembly");
        r.axis = Some("x".into());
        r.unit = Some("mm".into());
        let out = resolve_all(&[r], &data, Path::new(".")).unwrap();
        let got = out["length"].as_number().unwrap();
        assert!((got - 300.0).abs() < 1e-9, "{got}");
    }

    #[test]
    fn a_measure_volume_cubes_the_conversion() {
        let data = fake(&[]);
        let mut r = reference("vol", ReferenceKind::Measure, "bbox:Workspace/Assembly");
        r.axis = Some("volume".into());
        r.unit = Some("mm".into());
        let out = resolve_all(&[r], &data, Path::new(".")).unwrap();
        // 0.3 x 0.1 x 0.1 m is 300 x 100 x 100 mm.
        let got = out["vol"].as_number().unwrap();
        assert!((got - 3_000_000.0).abs() < 1e-6, "{got}");
    }

    #[test]
    fn a_planned_but_unbuilt_measure_fails_as_not_implemented() {
        let data = fake(&[]);
        let r = reference("v", ReferenceKind::Measure, "hull:Workspace/Assembly");
        let err = resolve_all(&[r], &data, Path::new(".")).unwrap_err();
        // Not silently skipped, and not blamed on the author's spelling.
        assert_eq!(err.kind, ErrorKind::NotImplemented);
        assert!(err.to_string().contains("not implemented"), "{err}");
    }

    #[test]
    fn a_misspelt_measure_prefix_fails_with_a_suggestion() {
        let data = fake(&[]);
        let r = reference("v", ReferenceKind::Measure, "bbx:Workspace/Assembly");
        let err = resolve_all(&[r], &data, Path::new(".")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::BadMeasure);
        assert!(err.to_string().contains("did you mean bbox?"), "{err}");
    }

    #[test]
    fn an_instance_source_without_a_hash_fails_with_the_shape() {
        let data = fake(&[]);
        let r = reference("x", ReferenceKind::Instance, "Workspace/Pack");
        let err = resolve_all(&[r], &data, Path::new(".")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err.to_string().contains("<path>#<section>.<field>"));
    }

    #[test]
    fn a_missing_run_directory_fails_with_a_path() {
        let mut r = reference("cycles", ReferenceKind::Sim, "battery.capacity_retention");
        r.run_label = Some("I_life_25C_5MPa_res0".into());
        let data = fake(&[]);
        let err = resolve_all(&[r], &data, Path::new("no/such/universe")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NoRun);
        assert!(err.to_string().contains("experiments"), "{err}");
    }

    // ---- globbing ---------------------------------------------------------

    #[test]
    fn double_star_means_strictly_under() {
        assert!(glob_matches(
            "Workspace/V-Cell/V1/Assembly/**",
            "Workspace/V-Cell/V1/Assembly/Cell01"
        ));
        assert!(glob_matches(
            "Workspace/V-Cell/V1/Assembly/**",
            "Workspace/V-Cell/V1/Assembly/Cell01/Anode"
        ));
        assert!(
            !glob_matches("Workspace/V-Cell/V1/Assembly/**", "Workspace/V-Cell/V1/Assembly"),
            "the subtree root is not one of its own descendants"
        );
    }

    #[test]
    fn single_star_never_crosses_a_separator() {
        assert!(glob_matches("Workspace/*", "Workspace/Pack"));
        assert!(!glob_matches("Workspace/*", "Workspace/Pack/Cell"));
        assert!(glob_matches("Workspace/Cell*", "Workspace/Cell01"));
        assert!(!glob_matches("Workspace/Cell*", "Workspace/Cell01/Anode"));
    }

    #[test]
    fn question_mark_matches_one_character() {
        assert!(glob_matches("Workspace/Cell0?", "Workspace/Cell01"));
        assert!(!glob_matches("Workspace/Cell0?", "Workspace/Cell012"));
    }

    #[test]
    fn a_literal_pattern_matches_only_itself() {
        assert!(glob_matches("Workspace/Pack", "Workspace/Pack"));
        assert!(!glob_matches("Workspace/Pack", "Workspace/Packs"));
    }

    // ---- path normalisation ----------------------------------------------

    #[test]
    fn logical_paths_drop_the_marker_and_the_flat_suffixes() {
        assert_eq!(logical_path("Workspace/Pack/_instance.toml"), "Workspace/Pack");
        assert_eq!(logical_path("Workspace/Pack.part.toml"), "Workspace/Pack");
        assert_eq!(logical_path("Workspace/Pack.glb.toml"), "Workspace/Pack");
        assert_eq!(logical_path("Workspace/Pack"), "Workspace/Pack");
    }

    // ---- filters ----------------------------------------------------------

    #[test]
    fn filters_parse_the_three_documented_forms() {
        assert!(matches!(parse_filter(None).unwrap(), CountFilter::All));
        assert!(matches!(
            parse_filter(Some("class_name = Part")).unwrap(),
            CountFilter::ClassIs(ref c) if c == "Part"
        ));
        assert!(matches!(
            parse_filter(Some("class_name != Folder")).unwrap(),
            CountFilter::ClassIsNot(ref c) if c == "Folder"
        ));
        assert!(matches!(
            parse_filter(Some("tag = structural")).unwrap(),
            CountFilter::Tagged(ref t) if t == "structural"
        ));
    }

    #[test]
    fn an_unknown_filter_field_fails_with_a_suggestion() {
        let err = parse_filter(Some("classname = Part")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::BadFilter);
        assert!(err.candidates.iter().any(|c| c == "class_name"));
    }
}

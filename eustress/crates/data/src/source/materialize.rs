//! Connector → Dataset materialization — the pure half of the ingest pipeline.
//!
//! A Connector is persisted as a `_instance.toml` whose `[attributes]` table
//! carries the connection config; a Dataset is a sibling directory holding a
//! CSV artifact plus one nested Series per column. Until this module existed
//! nothing read a Connector back, and nothing ever produced a Series — the
//! two halves of the Data Platform were written but never joined.
//!
//! This module is deliberately engine-independent: no bevy, no filesystem, no
//! network. It converts between the attribute map the engine already reads and
//! [`SourceConfig`], and it turns a fetched [`Frame`] into a [`DatasetPlan`] —
//! plain data the engine executes by writing directories and TOML.
//!
//! ## Design rules
//!
//! 1. **Parsing is liberal, errors are precise.** A hand-edited Connector with
//!    `source_type = "postgres"`, `enabled = "yes"`, and a quoted
//!    `poll_seconds` still parses; an unknown `source_type` names every
//!    provider it could have been.
//! 2. **The attribute map round-trips.** `config → attributes → config` is the
//!    identity, so the Properties panel can rewrite a Connector without ever
//!    silently dropping a provider-specific option.
//! 3. **Parsing is structural, not semantic.** [`config_from_attributes`]
//!    accepts a freshly created Connector (`endpoint = ""`), because the UI has
//!    to be able to load one in order to let you fill it in. Callers run
//!    [`validate_config`](super::validate_config) before fetching.
//! 4. **The artifact is what the reader already reads.** The CSV
//!    [`DatasetPlan`] emits uses the `name (unit)` header convention
//!    `import::frame_from_csv` parses, so the Properties panel's live stats
//!    light up on a materialized Dataset with no further wiring.

use std::collections::BTreeMap;

use super::{SourceConfig, SourceKind};
use crate::{ColumnData, ColumnDtype, DataError, Frame, Result};

// ── Connector attribute keys ─────────────────────────────────────────────────

/// `[attributes]` key holding [`SourceKind::as_str`].
pub const ATTR_SOURCE_TYPE: &str = "source_type";
/// `[attributes]` key holding [`SourceConfig::endpoint`].
pub const ATTR_ENDPOINT: &str = "endpoint";
/// `[attributes]` key holding [`SourceConfig::poll_seconds`].
pub const ATTR_POLL_SECONDS: &str = "poll_seconds";
/// `[attributes]` key holding [`SourceConfig::enabled`].
pub const ATTR_ENABLED: &str = "enabled";
/// `[attributes]` key holding [`SourceConfig::secret_ref`] — the NAME of an
/// env var, never the secret itself (see the [`source`](super) module docs).
pub const ATTR_SECRET_REF: &str = "secret_ref";

/// Attribute keys that map to named [`SourceConfig`] fields. Every OTHER key in
/// the table — including the Connector template's `format` — is a
/// provider-specific [`SourceConfig::options`] entry, which is what lets a new
/// provider add knobs without touching the TOML schema.
const NAMED_KEYS: [&str; 5] =
    [ATTR_SOURCE_TYPE, ATTR_ENDPOINT, ATTR_POLL_SECONDS, ATTR_ENABLED, ATTR_SECRET_REF];

/// Parse a Connector's persisted `[attributes]` into a [`SourceConfig`].
///
/// Structural only — it does not check that the config is *usable*, because a
/// freshly created Connector has an empty endpoint and the UI must still be
/// able to load it. Run [`validate_config`](super::validate_config) before
/// calling [`DataSource::fetch`](super::DataSource::fetch).
///
/// Liberal in what it accepts: values may arrive quoted (a raw
/// `toml::Value::to_string()`), `enabled` accepts `true/1/yes/on` and their
/// negatives, and `poll_seconds` treats `0` / an empty value / `manual` as
/// "manual only" rather than failing. Unknown keys become options verbatim.
pub fn config_from_attributes(attrs: &BTreeMap<String, String>) -> Result<SourceConfig> {
    let raw = attrs
        .get(ATTR_SOURCE_TYPE)
        .map(|s| unquote(s))
        .ok_or_else(|| {
            DataError::Schema(format!(
                "Connector is missing the required '{ATTR_SOURCE_TYPE}' attribute"
            ))
        })?;
    let kind = SourceKind::parse(raw).ok_or_else(|| {
        DataError::Schema(format!(
            "Connector has unknown {ATTR_SOURCE_TYPE} '{raw}'; expected one of: {}",
            SourceKind::ALL.map(|k| k.as_str()).join(", ")
        ))
    })?;

    let endpoint = attrs.get(ATTR_ENDPOINT).map(|s| unquote(s)).unwrap_or("").to_string();

    let poll_seconds = match attrs.get(ATTR_POLL_SECONDS).map(|s| unquote(s)) {
        None => None,
        Some(v) => parse_poll_seconds(v)?,
    };

    let enabled = match attrs.get(ATTR_ENABLED).map(|s| unquote(s)) {
        None => false,
        Some(v) => parse_bool(v)?,
    };

    // An empty secret_ref means "no secret", not "a secret named the empty
    // string" — otherwise a UI that writes every key would fail validation.
    let secret_ref = attrs
        .get(ATTR_SECRET_REF)
        .map(|s| unquote(s))
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);

    let options = attrs
        .iter()
        .filter(|(k, _)| !NAMED_KEYS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), unquote(v).to_string()))
        .collect();

    Ok(SourceConfig { kind, endpoint, options, secret_ref, poll_seconds, enabled })
}

/// [`config_from_attributes`] for any key/value pairs — the engine reads
/// `[attributes]` into a `HashMap<String, String>`.
pub fn config_from_attribute_pairs<I, K, V>(pairs: I) -> Result<SourceConfig>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let map: BTreeMap<String, String> =
        pairs.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
    config_from_attributes(&map)
}

/// Serialize a [`SourceConfig`] back to a Connector's `[attributes]` map.
///
/// The inverse of [`config_from_attributes`]: feeding the result back returns
/// an identical config. `poll_seconds` and `secret_ref` are omitted when unset
/// rather than written as a sentinel, so "manual only" and "no secret" survive
/// the round-trip as `None` instead of decaying to `Some(0)` / `Some("")`.
pub fn attributes_from_config(config: &SourceConfig) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    out.insert(ATTR_SOURCE_TYPE.to_string(), config.kind.as_str().to_string());
    out.insert(ATTR_ENDPOINT.to_string(), config.endpoint.clone());
    out.insert(ATTR_ENABLED.to_string(), config.enabled.to_string());
    if let Some(p) = config.poll_seconds {
        out.insert(ATTR_POLL_SECONDS.to_string(), p.to_string());
    }
    if let Some(r) = &config.secret_ref {
        out.insert(ATTR_SECRET_REF.to_string(), r.clone());
    }
    // Provider-specific knobs (including `format`) ride through verbatim. A
    // key that collides with a named field would corrupt the round-trip, so it
    // is dropped — `config_from_attributes` can never have produced one.
    for (k, v) in &config.options {
        if !NAMED_KEYS.contains(&k.as_str()) {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// Strip one layer of surrounding double quotes, so a raw
/// `toml::Value::to_string()` is as acceptable as an already-unwrapped string.
fn unquote(s: &str) -> &str {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        &t[1..t.len() - 1]
    } else {
        t
    }
}

fn parse_bool(v: &str) -> Result<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "" => Ok(false),
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        other => Err(DataError::Schema(format!(
            "Connector attribute '{ATTR_ENABLED}' must be a boolean, got '{other}'"
        ))),
    }
}

fn parse_poll_seconds(v: &str) -> Result<Option<u64>> {
    let t = v.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("manual") || t.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let n: u64 = t.parse().map_err(|_| {
        DataError::Schema(format!(
            "Connector attribute '{ATTR_POLL_SECONDS}' must be a whole number of seconds, got '{t}'"
        ))
    })?;
    // Zero is the UI's way of saying "do not poll"; treat it as manual-only
    // rather than rejecting a config a user can produce with a slider.
    Ok(if n == 0 { None } else { Some(n) })
}

// ── Dataset plan ─────────────────────────────────────────────────────────────

/// What a column is *for*, written to a Series' `role` attribute and read back
/// by the Properties panel's Schema section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesRole {
    /// The independent variable — time, sample number, the x axis.
    Index,
    /// A measured dependent variable.
    Value,
    /// A categorical / textual annotation.
    Label,
}

impl SeriesRole {
    /// Stable token written to the `role` attribute.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Index => "index",
            Self::Value => "value",
            Self::Label => "label",
        }
    }

    /// Parse [`SeriesRole::as_str`].
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "index" => Some(Self::Index),
            "value" => Some(Self::Value),
            "label" => Some(Self::Label),
            _ => None,
        }
    }
}

/// One Series to nest under the Dataset — exactly one per [`Frame`] column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesPlan {
    /// Instance name for the Series (the column name, made safe for a
    /// directory and de-duplicated within the plan).
    pub name: String,
    /// `column` attribute — the exact column header in the artifact, which is
    /// how the Properties panel finds this Series' data in the parent's CSV.
    pub column: String,
    /// `dtype` attribute, as [`ColumnDtype::as_token`].
    pub dtype: ColumnDtype,
    /// `unit` attribute — the authored symbol, e.g. `"psi"`.
    pub unit: Option<String>,
    /// `dimension` attribute — the column's own token when it carried one,
    /// otherwise derived from `unit` (see [`si_dimension_for_unit`]).
    pub dimension: Option<String>,
    /// `role` attribute.
    pub role: SeriesRole,
}

impl SeriesPlan {
    /// The `[attributes]` table for this Series' `_instance.toml`, keyed
    /// exactly as the Properties panel's Schema section reads it.
    pub fn attributes(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("column".to_string(), self.column.clone());
        m.insert("dtype".to_string(), self.dtype.as_token().to_string());
        if let Some(u) = &self.unit {
            m.insert("unit".to_string(), u.clone());
        }
        m.insert("role".to_string(), self.role.as_str().to_string());
        if let Some(d) = &self.dimension {
            m.insert("dimension".to_string(), d.clone());
        }
        m
    }
}

/// Serialization of the Dataset's data artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactFormat {
    /// CSV with a `name (unit)` header row — the format the engine's Dataset
    /// reader (`import::frame_from_csv`) already parses.
    Csv,
}

impl ArtifactFormat {
    /// File extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
        }
    }
}

/// The data file to write beside the Dataset's `_instance.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactPlan {
    /// File name relative to the Dataset directory, e.g. `"Readings.csv"`.
    pub file_name: String,
    /// Encoding of [`bytes`](ArtifactPlan::bytes).
    pub format: ArtifactFormat,
    /// The complete file contents.
    pub bytes: Vec<u8>,
}

/// Everything the engine needs to turn a fetched [`Frame`] into a Dataset with
/// nested Series — and nothing about *how* to write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetPlan {
    /// Instance name for the Dataset, made safe for a directory.
    pub dataset_name: String,
    /// The data file to write inside the Dataset directory.
    pub artifact: ArtifactPlan,
    /// One Series per column, in column order.
    pub series: Vec<SeriesPlan>,
    /// Row count, mirrored into the Dataset's `rows` attribute.
    pub n_rows: usize,
    /// Human-readable provenance for the Dataset's `source` attribute, e.g.
    /// `"REST https://api.example.com/readings"`.
    pub provenance: Option<String>,
}

impl DatasetPlan {
    /// Attach the provenance string the Properties panel shows as `source`.
    pub fn with_provenance(mut self, provenance: impl Into<String>) -> Self {
        self.provenance = Some(provenance.into());
        self
    }

    /// The `[attributes]` table for the Dataset's own `_instance.toml`.
    pub fn attributes(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("rows".to_string(), self.n_rows.to_string());
        m.insert("columns".to_string(), self.series.len().to_string());
        m.insert("format".to_string(), self.artifact.format.extension().to_string());
        if let Some(p) = &self.provenance {
            m.insert("source".to_string(), p.clone());
        }
        m
    }

    /// The Series playing the independent-variable role, if the frame had one.
    pub fn index_series(&self) -> Option<&SeriesPlan> {
        self.series.iter().find(|s| s.role == SeriesRole::Index)
    }
}

/// Plan the Dataset a fetched [`Frame`] should materialize into.
///
/// One Series per column, in column order, each carrying the column's exact
/// dtype and unit; the `dimension` is the column's own token when it has one
/// and is otherwise derived from the unit symbol. Roles come from
/// [`infer_role`]: at most one column is the [`Index`](SeriesRole::Index).
pub fn materialize_plan(frame: &Frame, name: &str) -> DatasetPlan {
    let dataset_name = sanitize_instance_name(name, "Dataset");
    let roles = infer_roles(frame);

    let mut used: Vec<String> = Vec::with_capacity(frame.n_cols());
    let mut series = Vec::with_capacity(frame.n_cols());
    for ((spec, _), role) in frame.columns().iter().zip(roles) {
        let base = sanitize_instance_name(&spec.name, "Column");
        let name = dedupe(&base, &mut used);
        let dimension = spec
            .dimension
            .clone()
            .or_else(|| spec.unit.as_deref().and_then(si_dimension_for_unit));
        series.push(SeriesPlan {
            name,
            column: spec.name.clone(),
            dtype: spec.dtype,
            unit: spec.unit.clone(),
            dimension,
            role,
        });
    }

    DatasetPlan {
        artifact: ArtifactPlan {
            file_name: format!("{dataset_name}.{}", ArtifactFormat::Csv.extension()),
            format: ArtifactFormat::Csv,
            bytes: frame_to_csv_bytes(frame),
        },
        dataset_name,
        series,
        n_rows: frame.n_rows(),
        provenance: None,
    }
}

/// [`materialize_plan`] with the provenance a Connector supplies, so the
/// Dataset records which source produced it.
pub fn plan_for_connector(frame: &Frame, name: &str, config: &SourceConfig) -> DatasetPlan {
    let provenance = if config.endpoint.trim().is_empty() {
        config.kind.as_str().to_string()
    } else {
        format!("{} {}", config.kind.as_str(), config.endpoint)
    };
    materialize_plan(frame, name).with_provenance(provenance)
}

// ── Role inference ───────────────────────────────────────────────────────────

/// Column names that name an independent variable outright.
const INDEX_NAMES: [&str; 14] = [
    "t", "x", "time", "timestamp", "ts", "date", "datetime", "epoch", "index", "idx", "seq",
    "sequence", "sample", "n",
];

/// The role one column plays, judged in isolation — no index promotion.
///
/// A string column is a [`Label`](SeriesRole::Label); everything else is a
/// [`Value`](SeriesRole::Value). Index detection needs the whole frame (only
/// one column can be the index), so it lives in [`infer_roles`].
pub fn infer_role(dtype: ColumnDtype) -> SeriesRole {
    match dtype {
        ColumnDtype::Str => SeriesRole::Label,
        _ => SeriesRole::Value,
    }
}

/// Assign a role to every column, promoting at most one to
/// [`Index`](SeriesRole::Index).
///
/// The index is the first numeric column whose name is a known independent
/// variable (`t`, `time`, `timestamp`, `sample`, …) or whose unit is a time
/// unit. Failing that — an anonymous export whose first column is just a ramp
/// — it is the first numeric column that is complete (no nulls) and strictly
/// increasing over at least two rows, which is what an independent axis
/// actually looks like. A constant or noisy column is never promoted.
pub fn infer_roles(frame: &Frame) -> Vec<SeriesRole> {
    let cols = frame.columns();
    let mut roles: Vec<SeriesRole> = cols.iter().map(|(s, _)| infer_role(s.dtype)).collect();

    let numeric =
        |d: ColumnDtype| matches!(d, ColumnDtype::F64 | ColumnDtype::I64);

    let named = cols.iter().position(|(s, _)| {
        numeric(s.dtype)
            && (INDEX_NAMES.contains(&s.name.trim().to_ascii_lowercase().as_str())
                || s.unit.as_deref().is_some_and(is_time_unit))
    });
    let chosen = named.or_else(|| {
        cols.iter().position(|(s, d)| numeric(s.dtype) && is_strictly_increasing(d))
    });
    if let Some(i) = chosen {
        roles[i] = SeriesRole::Index;
    }
    roles
}

fn is_time_unit(unit: &str) -> bool {
    matches!(
        unit.trim().to_ascii_lowercase().as_str(),
        "s" | "sec" | "second" | "seconds" | "ms" | "us" | "ns" | "min" | "minute" | "hr" | "h"
            | "hour"
    )
}

/// A complete, strictly increasing numeric column over at least two rows — the
/// shape of a sampled independent axis. Nulls disqualify it: a gap-riddled
/// column is a measurement, not an axis.
fn is_strictly_increasing(data: &ColumnData) -> bool {
    fn rising<T: PartialOrd + Copy>(v: &[Option<T>]) -> bool {
        if v.len() < 2 {
            return false;
        }
        let mut prev = match v[0] {
            Some(x) => x,
            None => return false,
        };
        for cell in &v[1..] {
            let Some(x) = *cell else { return false };
            if !(x > prev) {
                return false;
            }
            prev = x;
        }
        true
    }
    match data {
        ColumnData::F64(v) => rising(v),
        ColumnData::I64(v) => rising(v),
        ColumnData::Bool(_) | ColumnData::Str(_) => false,
    }
}

// ── Unit → SI dimension ──────────────────────────────────────────────────────

/// ASCII letters for each SI base slot, matching
/// `common::dimension::Dimension`'s `[L, M, T, I, K, N, J]` order.
const BASE_SYMS: [char; 7] = ['L', 'M', 'T', 'I', 'K', 'N', 'J'];

/// The canonical SI dimension token for a unit symbol, in the exact form
/// `common::dimension::Dimension::to_si_string` emits (`"si:L-1M1T-2"` for
/// pressure).
///
/// Mirrors `Dimension::from_unit_symbol`, which the leaf cannot call — it
/// carries no dependency on `common`. Returns `None` for an unrecognized
/// symbol, so an unknown unit yields a Series with a `unit` and no
/// `dimension` rather than a confidently wrong one. A symbol that is already a
/// `si:` token passes through unchanged; a known-dimensionless unit (`count`,
/// `deg`, `pH`) yields `"si:"`, which is how the canonical form spells
/// "dimensionless" and is distinct from "unknown".
pub fn si_dimension_for_unit(unit: &str) -> Option<String> {
    let s = unit.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    if s.starts_with("si:") {
        return Some(unit.trim().to_string());
    }
    #[rustfmt::skip]
    let exps: [i8; 7] = match s.as_str() {
        // dimensionless — pure numbers, angles (rad/deg), and pH
        "1" | "scalar" | "ratio" | "count" | "%" | "percent"
        | "rad" | "radian" | "radians" | "deg" | "degree" | "degrees" | "°"
        | "ph" => [0, 0, 0, 0, 0, 0, 0],
        // length
        "m" | "meter" | "meters" | "metre" | "mm" | "millimeter" | "cm" | "centimeter"
        | "km" | "kilometer" | "in" | "inch" | "inches" | "ft" | "foot" | "feet" | "yd"
        | "yard" | "stud" | "studs" => [1, 0, 0, 0, 0, 0, 0],
        // mass
        "kg" | "kilogram" | "g" | "gram" | "grams" | "mg" | "lb" | "pound" | "pounds"
            => [0, 1, 0, 0, 0, 0, 0],
        // time
        "s" | "sec" | "second" | "seconds" | "ms" | "us" | "ns" | "min" | "minute" | "hr"
        | "h" | "hour" => [0, 0, 1, 0, 0, 0, 0],
        // temperature
        "k" | "kelvin" | "°c" | "celsius" | "degc" | "°f" | "fahrenheit"
            => [0, 0, 0, 0, 1, 0, 0],
        // current
        "a" | "amp" | "ampere" | "amps" | "ma" => [0, 0, 0, 1, 0, 0, 0],
        // amount
        "mol" | "mole" | "moles" | "mmol" => [0, 0, 0, 0, 0, 1, 0],
        // luminous intensity
        "cd" | "candela" => [0, 0, 0, 0, 0, 0, 1],
        // derived
        "n" | "newton" | "newtons" | "kn" | "lbf" => [1, 1, -2, 0, 0, 0, 0],
        "pa" | "pascal" | "kpa" | "mpa" | "gpa" | "psi" | "bar" | "atm"
            => [-1, 1, -2, 0, 0, 0, 0],
        "j" | "joule" | "joules" | "kj" | "mj" | "wh" | "kwh" | "ev"
            => [2, 1, -2, 0, 0, 0, 0],
        "w" | "watt" | "watts" | "kw" | "mw" | "gw" => [2, 1, -3, 0, 0, 0, 0],
        "v" | "volt" | "volts" | "mv" | "kv" => [2, 1, -3, -1, 0, 0, 0],
        "hz" | "hertz" | "khz" | "mhz" | "ghz" | "rpm" => [0, 0, -1, 0, 0, 0, 0],
        "coulomb" | "coulombs" => [0, 0, 1, 1, 0, 0, 0],
        "m/s" | "mps" => [1, 0, -1, 0, 0, 0, 0],
        "m/s2" | "m/s^2" => [1, 0, -2, 0, 0, 0, 0],
        "m2" | "m^2" => [2, 0, 0, 0, 0, 0, 0],
        "m3" | "m^3" => [3, 0, 0, 0, 0, 0, 0],
        "mol/m3" | "mol/m^3" => [-3, 0, 0, 0, 0, 1, 0],
        _ => return None,
    };
    let mut out = String::from("si:");
    for (i, &e) in exps.iter().enumerate() {
        if e != 0 {
            out.push(BASE_SYMS[i]);
            out.push_str(&e.to_string());
        }
    }
    Some(out)
}

// ── CSV artifact ─────────────────────────────────────────────────────────────

/// Encode a [`Frame`] as the CSV the engine's Dataset reader parses: a
/// `name (unit)` header row, then one row per record with an empty field for a
/// null. Feeding the result to `import::frame_from_csv` recovers the same
/// column names, units, dtypes, and nulls.
pub fn frame_to_csv_bytes(frame: &Frame) -> Vec<u8> {
    let cols = frame.columns();
    let mut out = String::new();

    for (i, (spec, _)) in cols.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let header = match &spec.unit {
            Some(u) if !u.trim().is_empty() => format!("{} ({})", spec.name, u.trim()),
            _ => spec.name.clone(),
        };
        out.push_str(&csv_field(&header));
    }
    out.push('\n');

    for r in 0..frame.n_rows() {
        for (i, (_, data)) in cols.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            if let Some(cell) = csv_cell(data, r) {
                out.push_str(&csv_field(&cell));
            }
        }
        out.push('\n');
    }
    out.into_bytes()
}

/// The rendered value of one cell, or `None` for a null (an empty CSV field,
/// which is what the reader turns back into a null).
fn csv_cell(data: &ColumnData, row: usize) -> Option<String> {
    match data {
        ColumnData::F64(v) => v[row].map(|x| x.to_string()),
        ColumnData::I64(v) => v[row].map(|x| x.to_string()),
        ColumnData::Bool(v) => v[row].map(|x| x.to_string()),
        ColumnData::Str(v) => v[row].clone(),
    }
}

/// RFC 4180 quoting — only when the field actually needs it, so the common
/// case stays byte-identical to a hand-written CSV.
fn csv_field(s: &str) -> String {
    if s.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ── Naming ───────────────────────────────────────────────────────────────────

/// Make a column or Dataset name safe to use as a directory name on every
/// platform (Windows is the strict one), falling back to `fallback` when
/// nothing usable survives.
pub fn sanitize_instance_name(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').trim().to_string();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

/// Append ` 2`, ` 3`, … until the name is unused, then record it. Sanitizing
/// can collapse two distinct columns onto one name; the Dataset's children are
/// directories, so a collision would silently lose a Series.
fn dedupe(base: &str, used: &mut Vec<String>) -> String {
    let mut candidate = base.to_string();
    let mut n = 2;
    while used.iter().any(|u| u.eq_ignore_ascii_case(&candidate)) {
        candidate = format!("{base} {n}");
        n += 1;
    }
    used.push(candidate.clone());
    candidate
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{frame_from_columns, ColumnSpec};

    fn attrs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    // ── Attribute round-trip ────────────────────────────────────────────────

    #[test]
    fn config_to_attributes_to_config_is_the_identity() {
        let cases = [
            SourceConfig::new(SourceKind::Rest, "https://api.example.com/readings"),
            SourceConfig::new(SourceKind::Csv, "data/readings.csv")
                .with_option("format", "csv")
                .with_option("delimiter", ";"),
            {
                let mut c = SourceConfig::new(SourceKind::Postgres, "postgres://host/db")
                    .with_option("table", "readings")
                    .with_option("format", "json")
                    .with_secret_ref("PG_PASSWORD");
                c.poll_seconds = Some(30);
                c.enabled = true;
                c
            },
            {
                // Manual-only + no secret: the two "absent" cases that a
                // sentinel-based encoding would quietly turn into Some(0) /
                // Some("").
                let mut c = SourceConfig::new(SourceKind::GraphQl, "https://api.example.com/gql")
                    .with_option("query", "{ me { id } }");
                c.poll_seconds = None;
                c.enabled = false;
                c
            },
        ];
        for original in cases {
            let attrs = attributes_from_config(&original);
            let back = config_from_attributes(&attrs).expect("round-trip parse");
            assert_eq!(back, original, "round-trip changed the config");
            // Idempotent a second time round, so repeated UI saves are stable.
            assert_eq!(attributes_from_config(&back), attrs);
        }
    }

    #[test]
    fn the_shipped_connector_template_parses() {
        // Byte-for-byte the `[attributes]` the Data menu writes for a new
        // Connector (Connector/_instance.toml).
        let c = config_from_attributes(&attrs(&[
            ("source_type", "REST"),
            ("endpoint", ""),
            ("poll_seconds", "10"),
            ("format", "json"),
            ("enabled", "false"),
        ]))
        .expect("the shipped Connector template must parse");
        assert_eq!(c.kind, SourceKind::Rest);
        assert_eq!(c.endpoint, "");
        assert_eq!(c.poll_seconds, Some(10));
        assert!(!c.enabled);
        assert_eq!(c.option("format"), Some("json"), "format is a provider option");
        assert_eq!(c.secret_ref, None);
    }

    #[test]
    fn arbitrary_options_survive_the_round_trip() {
        let a = attrs(&[
            ("source_type", "S3"),
            ("endpoint", "s3://bucket"),
            ("enabled", "false"),
            ("region", "us-west-2"),
            ("key", "readings/2026.csv"),
            ("format", "parquet"),
        ]);
        let c = config_from_attributes(&a).unwrap();
        assert_eq!(c.option("region"), Some("us-west-2"));
        assert_eq!(attributes_from_config(&c), a, "an unknown key must ride through verbatim");
    }

    #[test]
    fn parsing_is_liberal_about_how_values_are_spelled() {
        let c = config_from_attributes(&attrs(&[
            ("source_type", "\"postgres\""), // raw toml::Value::to_string()
            ("endpoint", " postgres://host/db "),
            ("poll_seconds", "\"15\""),
            ("enabled", "YES"),
            ("secret_ref", "   "), // blank means "no secret", not an empty name
        ]))
        .unwrap();
        assert_eq!(c.kind, SourceKind::Postgres);
        assert_eq!(c.endpoint, "postgres://host/db");
        assert_eq!(c.poll_seconds, Some(15));
        assert!(c.enabled);
        assert_eq!(c.secret_ref, None);
    }

    #[test]
    fn zero_or_manual_poll_seconds_means_manual_only() {
        for v in ["0", "", "manual", "none"] {
            let c = config_from_attributes(&attrs(&[
                ("source_type", "REST"),
                ("endpoint", "https://x.example.com"),
                ("poll_seconds", v),
            ]))
            .unwrap_or_else(|e| panic!("poll_seconds '{v}' should parse: {e}"));
            assert_eq!(c.poll_seconds, None, "poll_seconds '{v}'");
        }
    }

    #[test]
    fn an_unknown_source_type_is_rejected_with_the_valid_names() {
        let err = config_from_attributes(&attrs(&[
            ("source_type", "MongoDB"),
            ("endpoint", "mongodb://host/db"),
        ]))
        .expect_err("an unknown source_type must not silently become a default provider");
        let msg = err.to_string();
        assert!(msg.contains("MongoDB"), "error should quote the bad value: {msg}");
        assert!(msg.contains("PostgreSQL"), "error should list the valid providers: {msg}");
    }

    #[test]
    fn a_missing_source_type_is_rejected() {
        let err = config_from_attributes(&attrs(&[("endpoint", "https://x.example.com")]))
            .expect_err("a Connector with no source_type is not a source");
        assert!(err.to_string().contains("source_type"), "{err}");
    }

    #[test]
    fn a_non_boolean_enabled_is_rejected() {
        let err = config_from_attributes(&attrs(&[
            ("source_type", "REST"),
            ("endpoint", "https://x.example.com"),
            ("enabled", "sometimes"),
        ]))
        .expect_err("'sometimes' is not a boolean");
        assert!(err.to_string().contains("enabled"), "{err}");
    }

    #[test]
    fn the_secret_itself_never_reaches_the_attribute_map() {
        std::env::set_var("MATERIALIZE_TEST_TOKEN", "s3cr3t-value");
        let c = SourceConfig::new(SourceKind::Rest, "https://api.example.com/x")
            .with_secret_ref("MATERIALIZE_TEST_TOKEN");
        let a = attributes_from_config(&c);
        assert_eq!(a.get("secret_ref").map(String::as_str), Some("MATERIALIZE_TEST_TOKEN"));
        assert!(
            !a.values().any(|v| v.contains("s3cr3t-value")),
            "the resolved secret must never be serialized"
        );
        std::env::remove_var("MATERIALIZE_TEST_TOKEN");
    }

    // ── Frame → DatasetPlan ─────────────────────────────────────────────────

    fn readings_frame() -> Frame {
        frame_from_columns(vec![
            (
                ColumnSpec::new("t", ColumnDtype::F64).with_unit("s"),
                ColumnData::F64(vec![Some(0.0), Some(0.5), Some(1.0)]),
            ),
            (
                ColumnSpec::new("pressure", ColumnDtype::F64).with_unit("psi"),
                ColumnData::F64(vec![Some(14.7), None, Some(15.2)]),
            ),
            (
                ColumnSpec::new("count", ColumnDtype::I64),
                ColumnData::I64(vec![Some(10), Some(20), Some(30)]),
            ),
            (
                ColumnSpec::new("ok", ColumnDtype::Bool),
                ColumnData::Bool(vec![Some(true), Some(false), None]),
            ),
            (
                ColumnSpec::new("label", ColumnDtype::Str),
                ColumnData::Str(vec![Some("a".into()), None, Some("c".into())]),
            ),
        ])
        .unwrap()
    }

    #[test]
    fn every_column_becomes_one_series_with_its_exact_dtype_and_unit() {
        let plan = materialize_plan(&readings_frame(), "Readings");
        assert_eq!(plan.dataset_name, "Readings");
        assert_eq!(plan.n_rows, 3);
        assert_eq!(plan.series.len(), 5, "exactly one Series per column");

        let names: Vec<&str> = plan.series.iter().map(|s| s.column.as_str()).collect();
        assert_eq!(names, ["t", "pressure", "count", "ok", "label"], "column order preserved");

        let dtypes: Vec<&str> = plan.series.iter().map(|s| s.dtype.as_token()).collect();
        assert_eq!(dtypes, ["f64", "f64", "i64", "bool", "str"]);

        let p = &plan.series[1];
        assert_eq!(p.unit.as_deref(), Some("psi"));
        assert_eq!(p.dimension.as_deref(), Some("si:L-1M1T-2"), "psi is a pressure");
        assert_eq!(plan.series[2].unit, None, "a unitless column must not invent one");
        assert_eq!(plan.series[2].dimension, None, "no unit → no dimension, not a wrong one");

        // The attribute map is exactly what the Properties panel reads.
        let a = plan.series[0].attributes();
        assert_eq!(a.get("column").map(String::as_str), Some("t"));
        assert_eq!(a.get("dtype").map(String::as_str), Some("f64"));
        assert_eq!(a.get("unit").map(String::as_str), Some("s"));
        assert_eq!(a.get("role").map(String::as_str), Some("index"));
        assert_eq!(a.get("dimension").map(String::as_str), Some("si:T1"));
    }

    #[test]
    fn an_authored_dimension_wins_over_the_derived_one() {
        let frame = frame_from_columns(vec![(
            ColumnSpec::new("stress", ColumnDtype::F64).with_unit("psi").with_dimension("Pa"),
            ColumnData::F64(vec![Some(1.0)]),
        )])
        .unwrap();
        let plan = materialize_plan(&frame, "Stress");
        assert_eq!(plan.series[0].dimension.as_deref(), Some("Pa"));
    }

    // ── Role heuristic ──────────────────────────────────────────────────────

    #[test]
    fn the_named_time_column_is_the_index_and_the_rest_are_values() {
        let plan = materialize_plan(&readings_frame(), "Readings");
        let roles: Vec<&str> = plan.series.iter().map(|s| s.role.as_str()).collect();
        assert_eq!(roles, ["index", "value", "value", "value", "label"]);
        assert_eq!(plan.index_series().map(|s| s.column.as_str()), Some("t"));
        assert_eq!(
            plan.series.iter().filter(|s| s.role == SeriesRole::Index).count(),
            1,
            "a Dataset has at most one independent variable"
        );
    }

    #[test]
    fn an_anonymous_strictly_increasing_column_is_the_index() {
        // No name hint and no unit: the ramp is the axis, the noise is not.
        let frame = frame_from_columns(vec![
            (
                ColumnSpec::new("alpha", ColumnDtype::F64),
                ColumnData::F64(vec![Some(3.0), Some(1.0), Some(2.0)]),
            ),
            (
                ColumnSpec::new("beta", ColumnDtype::I64),
                ColumnData::I64(vec![Some(0), Some(1), Some(2)]),
            ),
        ])
        .unwrap();
        let plan = materialize_plan(&frame, "Anon");
        assert_eq!(plan.index_series().map(|s| s.column.as_str()), Some("beta"));
        assert_eq!(plan.series[0].role, SeriesRole::Value);
    }

    #[test]
    fn a_constant_or_gappy_column_is_never_promoted_to_index() {
        let frame = frame_from_columns(vec![
            (
                ColumnSpec::new("flat", ColumnDtype::F64),
                ColumnData::F64(vec![Some(1.0), Some(1.0), Some(1.0)]),
            ),
            (
                // Rising but with a hole — a measurement, not an axis.
                ColumnSpec::new("gappy", ColumnDtype::I64),
                ColumnData::I64(vec![Some(0), None, Some(2)]),
            ),
        ])
        .unwrap();
        let plan = materialize_plan(&frame, "NoAxis");
        assert!(plan.index_series().is_none(), "nothing here is an independent axis");
        assert!(plan.series.iter().all(|s| s.role == SeriesRole::Value));
    }

    #[test]
    fn a_time_unit_promotes_an_oddly_named_column() {
        let frame = frame_from_columns(vec![
            (
                ColumnSpec::new("elapsed", ColumnDtype::F64).with_unit("ms"),
                // Deliberately NOT monotonic, so only the unit can explain it.
                ColumnData::F64(vec![Some(5.0), Some(1.0), Some(9.0)]),
            ),
            (
                ColumnSpec::new("volts", ColumnDtype::F64).with_unit("V"),
                ColumnData::F64(vec![Some(1.0), Some(2.0), Some(3.0)]),
            ),
        ])
        .unwrap();
        let plan = materialize_plan(&frame, "Scope");
        assert_eq!(plan.index_series().map(|s| s.column.as_str()), Some("elapsed"));
        assert_eq!(plan.series[1].dimension.as_deref(), Some("si:L2M1T-3I-1"), "volt");
    }

    #[test]
    fn a_string_column_is_a_label_even_when_it_is_first() {
        let frame = frame_from_columns(vec![
            (
                ColumnSpec::new("t", ColumnDtype::Str),
                ColumnData::Str(vec![Some("a".into()), Some("b".into())]),
            ),
            (
                ColumnSpec::new("value", ColumnDtype::F64),
                ColumnData::F64(vec![Some(1.0), Some(2.0)]),
            ),
        ])
        .unwrap();
        let plan = materialize_plan(&frame, "Mixed");
        assert_eq!(plan.series[0].role, SeriesRole::Label, "an index cannot be a string");
        assert_eq!(plan.series[1].role, SeriesRole::Index, "the ramp is the axis");
    }

    #[test]
    fn role_tokens_round_trip() {
        for r in [SeriesRole::Index, SeriesRole::Value, SeriesRole::Label] {
            assert_eq!(SeriesRole::parse(r.as_str()), Some(r));
        }
        assert_eq!(SeriesRole::parse("axis"), None);
    }

    // ── Artifact ────────────────────────────────────────────────────────────

    #[test]
    fn the_csv_artifact_uses_the_header_convention_the_reader_parses() {
        let plan = materialize_plan(&readings_frame(), "Readings");
        assert_eq!(plan.artifact.file_name, "Readings.csv");
        assert_eq!(plan.artifact.format, ArtifactFormat::Csv);
        let text = String::from_utf8(plan.artifact.bytes.clone()).unwrap();
        let mut lines = text.lines();
        assert_eq!(lines.next().unwrap(), "t (s),pressure (psi),count,ok,label");
        assert_eq!(lines.next().unwrap(), "0,14.7,10,true,a");
        assert_eq!(lines.next().unwrap(), "0.5,,20,false,", "a null is an empty field");
        assert_eq!(lines.next().unwrap(), "1,15.2,30,,c");
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn fields_needing_quotes_get_them() {
        let frame = frame_from_columns(vec![
            (
                ColumnSpec::new("a,b", ColumnDtype::I64),
                ColumnData::I64(vec![Some(1)]),
            ),
            (
                ColumnSpec::new("note", ColumnDtype::Str),
                ColumnData::Str(vec![Some("he said \"hi\", loudly".into())]),
            ),
        ])
        .unwrap();
        let text = String::from_utf8(frame_to_csv_bytes(&frame)).unwrap();
        assert_eq!(text, "\"a,b\",note\n1,\"he said \"\"hi\"\", loudly\"\n");
    }

    #[test]
    fn an_empty_frame_still_produces_a_header_only_artifact() {
        let frame = frame_from_columns(vec![(
            ColumnSpec::new("t", ColumnDtype::F64).with_unit("s"),
            ColumnData::F64(vec![]),
        )])
        .unwrap();
        let plan = materialize_plan(&frame, "Empty");
        assert_eq!(plan.n_rows, 0);
        assert_eq!(plan.series.len(), 1, "a 0-row frame still declares its schema");
        assert_eq!(String::from_utf8(plan.artifact.bytes).unwrap(), "t (s)\n");
    }

    #[cfg(feature = "import")]
    #[test]
    fn the_artifact_reads_back_as_the_same_frame() {
        // The load-bearing claim: what materialize writes is exactly what the
        // engine's Dataset reader (`import::frame_from_csv`) parses, units and
        // nulls included.
        let frame = readings_frame();
        let plan = materialize_plan(&frame, "Readings");
        let back = crate::import::frame_from_csv(plan.artifact.bytes.as_slice()).unwrap();
        assert_eq!(back, frame, "the CSV artifact did not survive a read-back");
    }

    // ── Naming + provenance ─────────────────────────────────────────────────

    #[test]
    fn names_that_cannot_be_directories_are_made_safe_and_unique() {
        let frame = frame_from_columns(vec![
            (ColumnSpec::new("a/b", ColumnDtype::I64), ColumnData::I64(vec![Some(1)])),
            (ColumnSpec::new("a:b", ColumnDtype::I64), ColumnData::I64(vec![Some(2)])),
        ])
        .unwrap();
        let plan = materialize_plan(&frame, "  bad/name?  ");
        assert_eq!(plan.dataset_name, "bad_name_");
        assert_eq!(plan.series[0].name, "a_b");
        assert_eq!(plan.series[1].name, "a_b 2", "a sanitize collision must not lose a Series");
        // The `column` attribute keeps the TRUE header, or the reader could
        // never find the data.
        assert_eq!(plan.series[1].column, "a:b");
    }

    #[test]
    fn a_connector_plan_records_its_provenance() {
        let cfg = SourceConfig::new(SourceKind::Rest, "https://api.example.com/readings");
        let plan = plan_for_connector(&readings_frame(), "Readings", &cfg);
        assert_eq!(plan.provenance.as_deref(), Some("REST https://api.example.com/readings"));
        let a = plan.attributes();
        assert_eq!(a.get("rows").map(String::as_str), Some("3"));
        assert_eq!(a.get("columns").map(String::as_str), Some("5"));
        assert_eq!(a.get("format").map(String::as_str), Some("csv"));
        assert_eq!(a.get("source").map(String::as_str), Some("REST https://api.example.com/readings"));
    }

    // ── Unit → dimension ────────────────────────────────────────────────────

    #[test]
    fn known_units_derive_the_canonical_si_token() {
        assert_eq!(si_dimension_for_unit("s").as_deref(), Some("si:T1"));
        assert_eq!(si_dimension_for_unit("m").as_deref(), Some("si:L1"));
        assert_eq!(si_dimension_for_unit("psi").as_deref(), Some("si:L-1M1T-2"));
        assert_eq!(si_dimension_for_unit("N").as_deref(), Some("si:L1M1T-2"));
        assert_eq!(si_dimension_for_unit("Hz").as_deref(), Some("si:T-1"));
        assert_eq!(si_dimension_for_unit("m/s").as_deref(), Some("si:L1T-1"));
        // Known-dimensionless is a real answer, spelled the canonical way.
        assert_eq!(si_dimension_for_unit("deg").as_deref(), Some("si:"));
        assert_eq!(si_dimension_for_unit("count").as_deref(), Some("si:"));
        // An already-canonical token passes through.
        assert_eq!(si_dimension_for_unit("si:L2M1T-2").as_deref(), Some("si:L2M1T-2"));
        // Unknown stays unknown rather than becoming a confident guess.
        assert_eq!(si_dimension_for_unit("widgets"), None);
        assert_eq!(si_dimension_for_unit("  "), None);
    }
}

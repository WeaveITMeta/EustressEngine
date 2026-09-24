//! A JSON front door to the mining runs.
//!
//! One request names a run, the data, and its settings; the reply is the run's
//! report. This is the surface for callers that speak JSON rather than Rust:
//! the `mine_data` tool an agent calls, the `eustress data` command, and
//! scripts. Every run reads the same way:
//!
//! ```json
//! { "run": "classify", "file": "Datasets/deliveries.csv", "target": "late",
//!   "features": ["lead_time_days", "distance_km", "unit_price"],
//!   "model": { "name": "decision_tree", "max_depth": 4 },
//!   "options": { "seed": 7, "group": "supplier" } }
//! ```
//!
//! The reply is `{ "run", "source", "report" }`, where `report` is the run's
//! report from [`super::workflow`], cautions included.
//!
//! Unknown fields are refused by name rather than ignored: a misspelled
//! setting that silently fell back to its default would produce a plausible,
//! wrong result. When `features` is left out, every numeric column except the
//! target and the group column is used, and the report lists which ones.
//! Non-finite numbers serialize as `null`; the one that occurs in practice is
//! the ANOVA F of a feature that separates the classes perfectly, which a
//! `perfect_separator` caution also reports.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::File;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::assoc::Apriori;
use super::classify::{
    ClassifierSpec, Criterion, DecisionTree, GaussianNaiveBayes, KNearestNeighbors, LogisticRegression,
};
use super::cluster::{Agglomerative, Dbscan, KMeans, Linkage};
use super::regress::{Lasso, LinearRegression, RegressorSpec};
use super::workflow::{self, Baskets, ClusterMethod, RunOptions, Series, Task};
use crate::import::{frame_from_csv, frame_from_jsonl};
use crate::numerics::stats;
use crate::{ColumnData, ColumnDtype, DataError, Frame, Result};

/// Every run the front door accepts.
pub const RUNS: &[&str] = &[
    "describe",
    "classify",
    "regress",
    "compare",
    "rank",
    "selection_check",
    "forward_select",
    "lasso",
    "cluster",
    "pca",
    "rules",
    "residuals",
];

/// Classifier names a request may give.
pub const CLASSIFIERS: &[&str] =
    &["logistic_regression", "decision_tree", "k_nearest_neighbors", "gaussian_naive_bayes"];
/// Regressor names a request may give.
pub const REGRESSORS: &[&str] = &["linear_regression", "ridge_regression", "lasso"];

fn bad(message: impl Into<String>) -> DataError {
    DataError::Schema(message.into())
}

// ─────────────────────────────────────────────────────────────────────────────
// Reading a request
// ─────────────────────────────────────────────────────────────────────────────

/// Typed access to one JSON object, with errors that name the field and say
/// where it was expected.
struct Req<'a> {
    ctx: String,
    obj: &'a Map<String, Value>,
}

impl<'a> Req<'a> {
    fn get(&self, key: &str) -> Option<&'a Value> {
        self.obj.get(key).filter(|v| !v.is_null())
    }

    fn string(&self, key: &str) -> Result<Option<&'a str>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(other) => Err(bad(format!("`{key}` in {} must be a string, got {other}", self.ctx))),
        }
    }

    fn need_string(&self, key: &str) -> Result<&'a str> {
        self.string(key)?.ok_or_else(|| bad(format!("{} needs `{key}`", self.ctx)))
    }

    fn number(&self, key: &str) -> Result<Option<f64>> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v
                .as_f64()
                .map(Some)
                .ok_or_else(|| bad(format!("`{key}` in {} must be a number, got {v}", self.ctx))),
        }
    }

    fn whole(&self, key: &str) -> Result<Option<u64>> {
        match self.get(key) {
            None => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| {
                bad(format!("`{key}` in {} must be a whole number of 0 or more, got {v}", self.ctx))
            }),
        }
    }

    fn count(&self, key: &str) -> Result<Option<usize>> {
        Ok(self.whole(key)?.map(|n| n as usize))
    }

    fn flag(&self, key: &str) -> Result<Option<bool>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(other) => Err(bad(format!("`{key}` in {} must be true or false, got {other}", self.ctx))),
        }
    }

    /// A list of names, as a JSON array or one comma-separated string.
    fn names(&self, key: &str) -> Result<Option<Vec<String>>> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| bad(format!("`{key}` in {} must list names as strings", self.ctx)))
                })
                .collect::<Result<Vec<_>>>()
                .map(Some),
            Some(Value::String(s)) => {
                Ok(Some(s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()))
            }
            Some(other) => Err(bad(format!("`{key}` in {} must be a list of names, got {other}", self.ctx))),
        }
    }
}

fn check_keys(obj: &Map<String, Value>, allowed: &[&str], ctx: &str) -> Result<()> {
    match obj.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(bad(format!("unknown field `{k}` in {ctx}; expected one of: {}", allowed.join(", ")))),
        None => Ok(()),
    }
}

fn at_least(v: usize, min: usize, key: &str) -> Result<usize> {
    if v >= min { Ok(v) } else { Err(bad(format!("`{key}` must be at least {min}, got {v}"))) }
}

fn non_negative(v: f64, key: &str) -> Result<f64> {
    if v.is_finite() && v >= 0.0 { Ok(v) } else { Err(bad(format!("`{key}` must be 0 or more, got {v}"))) }
}

fn positive(v: f64, key: &str) -> Result<f64> {
    if v.is_finite() && v > 0.0 { Ok(v) } else { Err(bad(format!("`{key}` must be above 0, got {v}"))) }
}

/// A model or method given either as `"name"` or as `{"name": ..., settings}`.
fn named(v: &Value, what: &str) -> Result<(String, Map<String, Value>)> {
    match v {
        Value::String(s) => Ok((s.clone(), Map::new())),
        Value::Object(o) => {
            let name = o
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| bad(format!("a {what} object needs a `name`")))?
                .to_string();
            let mut settings = o.clone();
            settings.remove("name");
            Ok((name, settings))
        }
        other => Err(bad(format!("a {what} is a name or an object with a `name`, got {other}"))),
    }
}

/// The classifier a request names, with its settings; a decision tree when
/// none is named.
pub fn classifier_from(v: Option<&Value>) -> Result<Box<dyn ClassifierSpec>> {
    let Some(v) = v else { return Ok(Box::new(DecisionTree::default())) };
    let (name, settings) = named(v, "model")?;
    let ctx = format!("model `{name}`");
    let r = Req { ctx: ctx.clone(), obj: &settings };
    let spec: Box<dyn ClassifierSpec> = match name.as_str() {
        "logistic_regression" => {
            check_keys(&settings, &["l2", "max_iter", "tol"], &ctx)?;
            let d = LogisticRegression::default();
            Box::new(LogisticRegression {
                l2: non_negative(r.number("l2")?.unwrap_or(d.l2), "l2")?,
                max_iter: at_least(r.count("max_iter")?.unwrap_or(d.max_iter), 1, "max_iter")?,
                tol: positive(r.number("tol")?.unwrap_or(d.tol), "tol")?,
            })
        }
        "decision_tree" => {
            check_keys(&settings, &["criterion", "max_depth", "min_samples_split", "min_samples_leaf"], &ctx)?;
            let d = DecisionTree::default();
            let criterion = match r.string("criterion")? {
                None | Some("gini") => Criterion::Gini,
                Some("entropy") => Criterion::Entropy,
                Some(other) => return Err(bad(format!("`criterion` must be `gini` or `entropy`, got `{other}`"))),
            };
            Box::new(DecisionTree {
                criterion,
                max_depth: r.count("max_depth")?,
                min_samples_split: at_least(
                    r.count("min_samples_split")?.unwrap_or(d.min_samples_split),
                    2,
                    "min_samples_split",
                )?,
                min_samples_leaf: at_least(
                    r.count("min_samples_leaf")?.unwrap_or(d.min_samples_leaf),
                    1,
                    "min_samples_leaf",
                )?,
            })
        }
        "k_nearest_neighbors" => {
            check_keys(&settings, &["k", "distance_weighted"], &ctx)?;
            let d = KNearestNeighbors::default();
            Box::new(KNearestNeighbors {
                k: at_least(r.count("k")?.unwrap_or(d.k), 1, "k")?,
                distance_weighted: r.flag("distance_weighted")?.unwrap_or(d.distance_weighted),
            })
        }
        "gaussian_naive_bayes" => {
            check_keys(&settings, &["var_smoothing"], &ctx)?;
            let d = GaussianNaiveBayes::default();
            Box::new(GaussianNaiveBayes {
                var_smoothing: non_negative(r.number("var_smoothing")?.unwrap_or(d.var_smoothing), "var_smoothing")?,
            })
        }
        other => {
            return Err(bad(format!("unknown classifier `{other}`; expected one of: {}", CLASSIFIERS.join(", "))))
        }
    };
    Ok(spec)
}

/// The regressor a request names, with its settings; least squares when none
/// is named.
pub fn regressor_from(v: Option<&Value>) -> Result<Box<dyn RegressorSpec>> {
    let Some(v) = v else { return Ok(Box::new(LinearRegression::default())) };
    let (name, settings) = named(v, "model")?;
    let ctx = format!("model `{name}`");
    let r = Req { ctx: ctx.clone(), obj: &settings };
    let spec: Box<dyn RegressorSpec> = match name.as_str() {
        "linear_regression" => {
            check_keys(&settings, &[], &ctx)?;
            Box::new(LinearRegression::default())
        }
        "ridge_regression" => {
            check_keys(&settings, &["ridge"], &ctx)?;
            Box::new(LinearRegression { ridge: positive(r.number("ridge")?.unwrap_or(1.0), "ridge")? })
        }
        "lasso" => {
            check_keys(&settings, &["alpha", "max_iter", "tol"], &ctx)?;
            let d = Lasso::default();
            Box::new(Lasso {
                alpha: non_negative(r.number("alpha")?.unwrap_or(d.alpha), "alpha")?,
                max_iter: at_least(r.count("max_iter")?.unwrap_or(d.max_iter), 1, "max_iter")?,
                tol: positive(r.number("tol")?.unwrap_or(d.tol), "tol")?,
            })
        }
        other => {
            return Err(bad(format!("unknown regressor `{other}`; expected one of: {}", REGRESSORS.join(", "))))
        }
    };
    Ok(spec)
}

fn cluster_method(v: &Value) -> Result<ClusterMethod> {
    let (name, settings) = named(v, "method")?;
    let ctx = format!("method `{name}`");
    let r = Req { ctx: ctx.clone(), obj: &settings };
    Ok(match name.as_str() {
        "k_means" => {
            check_keys(&settings, &["k", "n_init", "max_iter", "seed"], &ctx)?;
            let d = KMeans::default();
            ClusterMethod::KMeans(KMeans {
                k: at_least(r.count("k")?.ok_or_else(|| bad("k_means needs `k`, the number of clusters"))?, 1, "k")?,
                n_init: at_least(r.count("n_init")?.unwrap_or(d.n_init), 1, "n_init")?,
                max_iter: at_least(r.count("max_iter")?.unwrap_or(d.max_iter), 1, "max_iter")?,
                seed: r.whole("seed")?.unwrap_or(d.seed),
            })
        }
        "dbscan" => {
            check_keys(&settings, &["eps", "min_samples"], &ctx)?;
            ClusterMethod::Dbscan(Dbscan {
                eps: positive(r.number("eps")?.ok_or_else(|| bad("dbscan needs `eps`, the neighbourhood radius"))?, "eps")?,
                min_samples: at_least(r.count("min_samples")?.unwrap_or(5), 1, "min_samples")?,
            })
        }
        "agglomerative" => {
            check_keys(&settings, &["n_clusters", "linkage"], &ctx)?;
            let linkage = match r.string("linkage")? {
                None | Some("ward") => Linkage::Ward,
                Some("single") => Linkage::Single,
                Some("complete") => Linkage::Complete,
                Some("average") => Linkage::Average,
                Some(other) => {
                    return Err(bad(format!(
                        "`linkage` must be `ward`, `single`, `complete` or `average`, got `{other}`"
                    )))
                }
            };
            ClusterMethod::Agglomerative(Agglomerative {
                n_clusters: at_least(
                    r.count("n_clusters")?.ok_or_else(|| bad("agglomerative needs `n_clusters`"))?,
                    1,
                    "n_clusters",
                )?,
                linkage,
            })
        }
        other => {
            return Err(bad(format!("unknown clustering `{other}`; expected one of: k_means, dbscan, agglomerative")))
        }
    })
}

fn run_options(v: Option<&Value>) -> Result<RunOptions> {
    let mut o = RunOptions::default();
    let Some(v) = v else { return Ok(o) };
    let obj = v.as_object().ok_or_else(|| bad("`options` must be an object"))?;
    check_keys(obj, &["test_fraction", "seed", "cv_folds", "importance_repeats", "group"], "options")?;
    let r = Req { ctx: "options".to_string(), obj };
    if let Some(f) = r.number("test_fraction")? {
        o.test_fraction = f;
    }
    if let Some(s) = r.whole("seed")? {
        o.seed = s;
    }
    if let Some(k) = r.count("cv_folds")? {
        o.cv_folds = k;
    }
    if let Some(n) = r.count("importance_repeats")? {
        o.importance_repeats = n;
    }
    o.group = r.string("group")?.map(str::to_string);
    Ok(o)
}

/// Classification for text and true/false targets, regression for decimal
/// ones; an integer target could be either, so it must be said.
fn task_for(r: &Req<'_>, frame: &Frame, target: &str) -> Result<Task> {
    match r.string("task")? {
        Some("classification") => return Ok(Task::Classification),
        Some("regression") => return Ok(Task::Regression),
        Some(other) => return Err(bad(format!("`task` must be `classification` or `regression`, got `{other}`"))),
        None => {}
    }
    match frame.column(target).map(ColumnData::dtype) {
        Some(ColumnDtype::Str) | Some(ColumnDtype::Bool) => Ok(Task::Classification),
        Some(ColumnDtype::F64) => Ok(Task::Regression),
        Some(ColumnDtype::I64) => Err(bad(format!(
            "`{target}` holds whole numbers, which could be class codes or counts; say which with \
             `task`: `classification` or `regression`"
        ))),
        None => Err(bad(format!("no column `{target}`"))),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Loading data
// ─────────────────────────────────────────────────────────────────────────────

fn open(path: &Path) -> Result<File> {
    File::open(path).map_err(|e| bad(format!("cannot open `{}`: {e}", path.display())))
}

fn frame_from_rows(rows: &[Value]) -> Result<Frame> {
    let mut text = String::new();
    for (i, row) in rows.iter().enumerate() {
        if !row.is_object() {
            return Err(bad(format!("row {i} of the data is not an object of column values")));
        }
        text.push_str(&row.to_string());
        text.push('\n');
    }
    frame_from_jsonl(text.as_bytes())
}

#[cfg(feature = "parquet")]
fn read_parquet_file(path: &Path) -> Result<Frame> {
    crate::read_parquet(path)
}

#[cfg(not(feature = "parquet"))]
fn read_parquet_file(path: &Path) -> Result<Frame> {
    Err(bad(format!(
        "`{}` is Parquet, which this build reads only with the `parquet` feature of eustress-data",
        path.display()
    )))
}

/// Read a data file into a [`Frame`] by its extension: `.csv`, `.jsonl` or
/// `.ndjson`, `.json` (an array of row objects), or `.parquet`.
pub fn load_frame(path: &Path) -> Result<Frame> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "csv" => frame_from_csv(open(path)?),
        "jsonl" | "ndjson" => frame_from_jsonl(open(path)?),
        "json" => {
            let v: Value = serde_json::from_reader(open(path)?)
                .map_err(|e| bad(format!("`{}` is not valid JSON: {e}", path.display())))?;
            match v {
                Value::Array(rows) => frame_from_rows(&rows),
                _ => Err(bad(format!("`{}` must hold an array of row objects", path.display()))),
            }
        }
        "parquet" => read_parquet_file(path),
        _ => Err(bad(format!(
            "cannot read `{}`: use a .csv, .jsonl, .ndjson, .json or .parquet file",
            path.display()
        ))),
    }
}

/// The frame a request points at: a file (through `resolve`) or rows inline.
fn load_source(
    r: &Req<'_>,
    resolve: &dyn Fn(&str) -> Result<PathBuf>,
    file_key: &str,
    data_key: &str,
) -> Result<(Frame, String)> {
    match (r.string(file_key)?, r.get(data_key)) {
        (Some(_), Some(_)) => Err(bad(format!("give `{file_key}` or `{data_key}`, not both"))),
        (Some(f), None) => Ok((load_frame(&resolve(f)?)?, f.to_string())),
        (None, Some(Value::Array(rows))) => Ok((frame_from_rows(rows)?, "inline".to_string())),
        (None, Some(_)) => Err(bad(format!("`{data_key}` must be an array of row objects"))),
        (None, None) => Err(bad(format!(
            "{} needs `{file_key}` (a data file) or `{data_key}` (an array of row objects)",
            r.ctx
        ))),
    }
}

fn refs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

/// The named features, or every numeric column that is not excluded.
fn feature_names(r: &Req<'_>, frame: &Frame, exclude: &[&str]) -> Result<Vec<String>> {
    if let Some(f) = r.names("features")?.filter(|f| !f.is_empty()) {
        return Ok(f);
    }
    let found: Vec<String> = frame
        .specs()
        .filter(|s| matches!(s.dtype, ColumnDtype::F64 | ColumnDtype::I64) && !exclude.contains(&s.name.as_str()))
        .map(|s| s.name.clone())
        .collect();
    if found.is_empty() {
        return Err(bad("no numeric column is left to use as a feature; name them in `features`"));
    }
    Ok(found)
}

fn to_value<T: Serialize>(report: &T) -> Result<Value> {
    serde_json::to_value(report).map_err(|e| bad(format!("report serialization failed: {e}")))
}

/// Keep the first `limit` entries of a list field, recording how many there
/// were in `<field>_total`.
fn truncate(v: &mut Value, field: &str, limit: usize) {
    if let Some(obj) = v.as_object_mut() {
        let total = obj.get(field).and_then(Value::as_array).map(Vec::len);
        if let Some(total) = total.filter(|&t| t > limit) {
            if let Some(Value::Array(items)) = obj.get_mut(field) {
                items.truncate(limit);
            }
            obj.insert(format!("{field}_total"), json!(total));
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Running a request
// ─────────────────────────────────────────────────────────────────────────────

/// Run one request. `resolve` turns the request's file names into paths; a
/// sandboxed caller refuses anything outside its sandbox there.
pub fn run(request: &Value, resolve: &dyn Fn(&str) -> Result<PathBuf>) -> Result<Value> {
    let obj = request.as_object().ok_or_else(|| bad("a request is a JSON object with a `run` field"))?;
    let run = obj
        .get("run")
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("a request needs `run`, one of: {}", RUNS.join(", "))))?;
    let specific: &[&str] = match run {
        "describe" => &[],
        "classify" | "regress" => &["target", "features", "model", "options"],
        "compare" => &["task", "target", "features", "models", "options"],
        "rank" => &["task", "target", "features"],
        "selection_check" => &["target", "features", "model", "keep", "options"],
        "forward_select" => &["target", "features", "model", "max_features", "options"],
        "lasso" => &["target", "features", "options"],
        "cluster" => &["features", "method", "standardize", "assignments"],
        "pca" => &["features", "standardize"],
        "rules" => &["basket", "item", "flags", "min_support", "min_confidence", "max_len", "limit"],
        "residuals" => {
            &["measured", "simulated", "time", "value", "simulated_file", "simulated_data", "sim_time", "sim_value"]
        }
        other => return Err(bad(format!("unknown run `{other}`; expected one of: {}", RUNS.join(", ")))),
    };
    let ctx = format!("run `{run}`");
    let mut allowed = vec!["run", "file", "data"];
    allowed.extend_from_slice(specific);
    check_keys(obj, &allowed, &ctx)?;
    let r = Req { ctx, obj };
    let (frame, source) = load_source(&r, resolve, "file", "data")?;

    let report = match run {
        "describe" => describe(&frame),
        "classify" => {
            let target = r.need_string("target")?;
            let opts = run_options(r.get("options"))?;
            let features = feature_names(&r, &frame, &excluded(target, &opts))?;
            let spec = classifier_from(r.get("model"))?;
            to_value(&workflow::classify(&frame, &refs(&features), target, spec.as_ref(), &opts)?)?
        }
        "regress" => {
            let target = r.need_string("target")?;
            let opts = run_options(r.get("options"))?;
            let features = feature_names(&r, &frame, &excluded(target, &opts))?;
            let spec = regressor_from(r.get("model"))?;
            to_value(&workflow::regress(&frame, &refs(&features), target, spec.as_ref(), &opts)?)?
        }
        "compare" => {
            let target = r.need_string("target")?;
            let opts = run_options(r.get("options"))?;
            let features = feature_names(&r, &frame, &excluded(target, &opts))?;
            let listed = match r.get("models") {
                None => None,
                Some(Value::Array(models)) => Some(models.as_slice()),
                Some(other) => return Err(bad(format!("`models` must be a list, got {other}"))),
            };
            match task_for(&r, &frame, target)? {
                Task::Classification => {
                    let specs: Vec<Box<dyn ClassifierSpec>> = match listed {
                        Some(models) => models.iter().map(|m| classifier_from(Some(m))).collect::<Result<_>>()?,
                        None => CLASSIFIERS.iter().map(|&n| classifier_from(Some(&json!(n)))).collect::<Result<_>>()?,
                    };
                    let specs: Vec<&dyn ClassifierSpec> = specs.iter().map(|s| s.as_ref()).collect();
                    to_value(&workflow::compare_classifiers(&frame, &refs(&features), target, &specs, &opts)?)?
                }
                Task::Regression => {
                    let specs: Vec<Box<dyn RegressorSpec>> = match listed {
                        Some(models) => models.iter().map(|m| regressor_from(Some(m))).collect::<Result<_>>()?,
                        None => REGRESSORS.iter().map(|&n| regressor_from(Some(&json!(n)))).collect::<Result<_>>()?,
                    };
                    let specs: Vec<&dyn RegressorSpec> = specs.iter().map(|s| s.as_ref()).collect();
                    to_value(&workflow::compare_regressors(&frame, &refs(&features), target, &specs, &opts)?)?
                }
            }
        }
        "rank" => {
            let target = r.need_string("target")?;
            let features = feature_names(&r, &frame, &[target])?;
            let task = task_for(&r, &frame, target)?;
            to_value(&workflow::rank_features(&frame, &refs(&features), target, task)?)?
        }
        "selection_check" => {
            let target = r.need_string("target")?;
            let opts = run_options(r.get("options"))?;
            let features = feature_names(&r, &frame, &excluded(target, &opts))?;
            let keep = r.count("keep")?.unwrap_or_else(|| (features.len() / 2).clamp(1, 10));
            let spec = classifier_from(r.get("model"))?;
            to_value(&workflow::selection_check(&frame, &refs(&features), target, spec.as_ref(), keep, &opts)?)?
        }
        "forward_select" => {
            let target = r.need_string("target")?;
            let opts = run_options(r.get("options"))?;
            let features = feature_names(&r, &frame, &excluded(target, &opts))?;
            let max = r.count("max_features")?.unwrap_or_else(|| features.len().min(10));
            let spec = classifier_from(r.get("model"))?;
            to_value(&workflow::forward_select(&frame, &refs(&features), target, spec.as_ref(), max, &opts)?)?
        }
        "lasso" => {
            let target = r.need_string("target")?;
            let opts = run_options(r.get("options"))?;
            let features = feature_names(&r, &frame, &excluded(target, &opts))?;
            to_value(&workflow::lasso_cv(&frame, &refs(&features), target, &opts)?)?
        }
        "cluster" => {
            let method = cluster_method(
                r.get("method").ok_or_else(|| bad("run `cluster` needs `method`: k_means, dbscan or agglomerative"))?,
            )?;
            let features = feature_names(&r, &frame, &[])?;
            let standardize = r.flag("standardize")?.unwrap_or(true);
            let mut v = to_value(&workflow::cluster(&frame, &refs(&features), &method, standardize)?)?;
            if !r.flag("assignments")?.unwrap_or(false) {
                if let Some(o) = v.as_object_mut() {
                    o.remove("assignments");
                }
            }
            v
        }
        "pca" => {
            let features = feature_names(&r, &frame, &[])?;
            let standardize = r.flag("standardize")?.unwrap_or(true);
            to_value(&workflow::pca(&frame, &refs(&features), standardize)?)?
        }
        "rules" => {
            let d = Apriori::default();
            let apriori = Apriori {
                min_support: r.number("min_support")?.unwrap_or(d.min_support),
                min_confidence: r.number("min_confidence")?.unwrap_or(d.min_confidence),
                max_len: r.count("max_len")?.unwrap_or(d.max_len),
            };
            let flags = r.names("flags")?;
            let flag_refs: Vec<&str> = flags.as_deref().map(refs).unwrap_or_default();
            let baskets = match (r.string("basket")?, r.string("item")?, flags.is_some()) {
                (Some(basket), Some(item), false) => Baskets::Long { basket, item },
                (None, None, true) => Baskets::Flags(&flag_refs),
                _ => {
                    return Err(bad(
                        "run `rules` needs `basket` and `item` (one row per basket and item), or `flags` \
                         (one true/false column per item), not both",
                    ))
                }
            };
            let mut v = to_value(&workflow::associate(&frame, baskets, &apriori)?)?;
            let limit = r.count("limit")?.unwrap_or(50);
            truncate(&mut v, "itemsets", limit);
            truncate(&mut v, "rules", limit);
            v
        }
        "residuals" => match (r.string("measured")?, r.string("simulated")?) {
            (Some(measured), Some(simulated)) => {
                to_value(&workflow::compare_columns(&frame, measured, simulated)?)?
            }
            (None, None) => {
                let time = r.need_string("time")?;
                let value = r.need_string("value")?;
                let (sim, _) = load_source(&r, resolve, "simulated_file", "simulated_data")?;
                let sim_time = r.string("sim_time")?.unwrap_or(time);
                let sim_value = r.string("sim_value")?.unwrap_or(value);
                to_value(&workflow::compare_frames(
                    Series { frame: &frame, time, value },
                    Series { frame: &sim, time: sim_time, value: sim_value },
                )?)?
            }
            _ => {
                return Err(bad(
                    "run `residuals` needs `measured` and `simulated` (two columns of one file), or \
                     `time`, `value` and `simulated_file` (two series, each on its own clock)",
                ))
            }
        },
        _ => unreachable!("every run was matched above"),
    };
    Ok(json!({ "run": run, "source": source, "report": report }))
}

fn excluded<'a>(target: &'a str, opts: &'a RunOptions) -> Vec<&'a str> {
    let mut v = vec![target];
    if let Some(g) = &opts.group {
        v.push(g.as_str());
    }
    v
}

fn null_count(data: &ColumnData) -> usize {
    match data {
        ColumnData::F64(v) => v.iter().filter(|x| !x.is_some_and(f64::is_finite)).count(),
        ColumnData::I64(v) => v.iter().filter(|x| x.is_none()).count(),
        ColumnData::Bool(v) => v.iter().filter(|x| x.is_none()).count(),
        ColumnData::Str(v) => v.iter().filter(|x| x.is_none()).count(),
    }
}

/// Rows, and per column: type, unit, missing values, and either summary
/// statistics (numbers) or the most common values (text, true/false).
fn describe(frame: &Frame) -> Value {
    let columns: Vec<Value> = frame
        .columns()
        .iter()
        .map(|(spec, data)| {
            let mut c = json!({
                "name": spec.name,
                "dtype": spec.dtype.as_token(),
                "missing": null_count(data),
            });
            if let Some(unit) = &spec.unit {
                c["unit"] = json!(unit);
            }
            match data {
                ColumnData::F64(_) | ColumnData::I64(_) => {
                    if let Ok(s) = stats(data) {
                        c["mean"] = json!(s.mean);
                        c["std"] = json!(s.std_dev);
                        c["min"] = json!(s.min);
                        c["max"] = json!(s.max);
                    }
                }
                ColumnData::Str(v) => {
                    let mut counts: HashMap<&str, usize> = HashMap::new();
                    for s in v.iter().flatten() {
                        *counts.entry(s.as_str()).or_insert(0) += 1;
                    }
                    let mut top: Vec<(&str, usize)> = counts.iter().map(|(&k, &n)| (k, n)).collect();
                    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
                    c["distinct"] = json!(counts.len());
                    c["top"] = top
                        .iter()
                        .take(5)
                        .map(|(value, count)| json!({ "value": value, "count": count }))
                        .collect();
                }
                ColumnData::Bool(v) => {
                    c["true"] = json!(v.iter().filter(|x| **x == Some(true)).count());
                    c["false"] = json!(v.iter().filter(|x| **x == Some(false)).count());
                }
            }
            c
        })
        .collect();
    json!({ "rows": frame.n_rows(), "columns": columns })
}

// ─────────────────────────────────────────────────────────────────────────────
// Rendering a reply as text
// ─────────────────────────────────────────────────────────────────────────────

fn num(v: &Value) -> String {
    match v.as_f64() {
        Some(x) if x.abs() >= 1e5 || (x != 0.0 && x.abs() < 1e-3) => format!("{x:.3e}"),
        Some(x) => format!("{x:.3}"),
        None if v.is_null() => "n/a".to_string(),
        None => v.to_string(),
    }
}

fn cv(v: &Value) -> String {
    match v["scores"].as_array() {
        Some(scores) => format!("{} ± {} over {} folds", num(&v["mean"]), num(&v["std"]), scores.len()),
        None => "not run".to_string(),
    }
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn list(v: &Value) -> String {
    v.as_array()
        .map(|a| a.iter().map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string)).collect::<Vec<_>>().join(", "))
        .unwrap_or_default()
}

fn importance(out: &mut String, r: &Value, what: &str) {
    if let Some(imps) = r["importance"].as_array().filter(|a| !a.is_empty()) {
        let _ = writeln!(out, "importance ({what} lost when the feature is shuffled):");
        for i in imps.iter().take(10) {
            let _ = writeln!(out, "  {:<24} {} ± {}", text(&i["feature"]), num(&i["mean_drop"]), num(&i["std_drop"]));
        }
    }
}

fn rows_line(out: &mut String, source: &str, r: &Value) {
    let _ = write!(out, "{source}: {} rows used, {} dropped", r["rows_used"], r["rows_dropped"]);
    if r["train_rows"].is_number() {
        let _ = write!(out, "; {} train, {} test", r["train_rows"], r["test_rows"]);
    }
    out.push('\n');
}

/// The reply of [`run`] as readable text: the headline numbers of the report
/// and every caution. The JSON reply holds everything else.
pub fn render(reply: &Value) -> String {
    let run = text(&reply["run"]);
    let source = text(&reply["source"]);
    let r = &reply["report"];
    let mut out = String::new();
    match run {
        "describe" => {
            let _ = writeln!(out, "{source}: {} rows", r["rows"]);
            for c in r["columns"].as_array().into_iter().flatten() {
                let unit = c["unit"].as_str().map(|u| format!(" ({u})")).unwrap_or_default();
                let _ = write!(out, "  {:<24} {:<5} missing {:<6}", format!("{}{unit}", text(&c["name"])), text(&c["dtype"]), c["missing"]);
                if c["mean"].is_number() {
                    let _ = write!(out, " mean {}  std {}  min {}  max {}", num(&c["mean"]), num(&c["std"]), num(&c["min"]), num(&c["max"]));
                } else if c["distinct"].is_number() {
                    let top: Vec<String> = c["top"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|t| format!("{} ({})", text(&t["value"]), t["count"]))
                        .collect();
                    let _ = write!(out, " {} distinct: {}", c["distinct"], top.join(", "));
                } else if c["true"].is_number() {
                    let _ = write!(out, " true {}  false {}", c["true"], c["false"]);
                }
                out.push('\n');
            }
        }
        "classify" => {
            let _ = writeln!(out, "classify `{}` with {}", text(&r["target"]), text(&r["model"]));
            rows_line(&mut out, source, r);
            let _ = writeln!(out, "classes: {}; features: {}", list(&r["classes"]), list(&r["features"]));
            let _ = writeln!(out, "{:<22} {:>8} {:>9}", "test rows", "model", "baseline");
            let _ = writeln!(out, "{:<22} {:>8} {:>9}", "accuracy", num(&r["accuracy"]), num(&r["baseline_accuracy"]));
            let _ = writeln!(
                out,
                "{:<22} {:>8} {:>9}",
                "balanced accuracy",
                num(&r["balanced_accuracy"]),
                num(&r["baseline_balanced_accuracy"])
            );
            let _ = writeln!(out, "{:<22} {:>8}", "macro F1", num(&r["macro_f1"]));
            if r["roc_auc"].is_number() {
                let _ = writeln!(out, "{:<22} {:>8}", "ROC AUC", num(&r["roc_auc"]));
            }
            let _ = writeln!(out, "training accuracy {}; cross-validated {}", num(&r["train_accuracy"]), cv(&r["cv_accuracy"]));
            importance(&mut out, r, "test accuracy");
        }
        "regress" => {
            let _ = writeln!(out, "regress `{}` with {}", text(&r["target"]), text(&r["model"]));
            rows_line(&mut out, source, r);
            let _ = writeln!(out, "{:<22} {:>10} {:>10}", "test rows", "model", "baseline");
            for (label, key) in [("R²", "r2"), ("RMSE", "rmse"), ("MAE", "mae"), ("bias", "bias")] {
                let _ = writeln!(out, "{:<22} {:>10} {:>10}", label, num(&r["test"][key]), num(&r["baseline"][key]));
            }
            let _ = writeln!(out, "training R² {}; cross-validated R² {}", num(&r["train"]["r2"]), cv(&r["cv_r2"]));
            if let Some(coefs) = r["coefficients"].as_array().filter(|c| !c.is_empty()) {
                let terms: Vec<String> =
                    coefs.iter().map(|c| format!("{} · {}", num(&c["value"]), text(&c["feature"]))).collect();
                let _ = writeln!(out, "{} = {} + {}", text(&r["target"]), num(&r["intercept"]), terms.join(" + "));
            }
            importance(&mut out, r, "test R²");
        }
        "compare" => {
            let _ = writeln!(out, "compare models for `{}`", text(&r["target"]));
            rows_line(&mut out, source, r);
            let classification = r["classes"].is_array();
            if classification {
                let _ = writeln!(out, "baseline accuracy {}", num(&r["baseline_accuracy"]));
                let _ = writeln!(out, "  {:<58} {:>22} {:>9} {:>9}", "model (best first)", "cross-validated F1", "accuracy", "macro F1");
            } else {
                let _ = writeln!(out, "baseline R² {}", num(&r["baseline"]["r2"]));
                let _ = writeln!(out, "  {:<58} {:>22} {:>9} {:>9}", "model (best first)", "cross-validated R²", "R²", "RMSE");
            }
            for e in r["entries"].as_array().into_iter().flatten() {
                let (cvk, a, b) = if classification { ("cv_macro_f1", "accuracy", "macro_f1") } else { ("cv_r2", "r2", "rmse") };
                let cvs = if e[cvk].is_object() { format!("{} ± {}", num(&e[cvk]["mean"]), num(&e[cvk]["std"])) } else { "not run".into() };
                let _ = writeln!(out, "  {:<58} {:>22} {:>9} {:>9}", text(&e["model"]), cvs, num(&e[a]), num(&e[b]));
            }
        }
        "rank" => {
            let _ = writeln!(out, "rank features against `{}` ({})", text(&r["target"]), text(&r["task"]));
            rows_line(&mut out, source, r);
            let linear = if r["task"] == "classification" { "ANOVA F" } else { "|Pearson r|" };
            let _ = writeln!(out, "  {:<24} {:>12} {:>12} {:>12}", "feature", "mutual info", linear, "variance");
            for f in r["features"].as_array().into_iter().flatten() {
                let _ = writeln!(
                    out,
                    "  {:<24} {:>12} {:>12} {:>12}",
                    text(&f["feature"]),
                    num(&f["mutual_information"]),
                    num(&f["linear_score"]),
                    num(&f["variance"])
                );
            }
        }
        "selection_check" => {
            let _ = writeln!(
                out,
                "selection check: keep {} of {} features for `{}` with {}",
                r["keep"], r["features_available"], text(&r["target"]), text(&r["model"])
            );
            let _ = writeln!(out, "chosen on all rows (the mistake):   accuracy {}", cv(&r["naive"]));
            let _ = writeln!(out, "chosen inside each fold (honest):   accuracy {}", cv(&r["honest"]));
            let _ = writeln!(out, "optimism the mistake invents: {}", num(&r["optimism"]));
            let chosen: Vec<String> = r["stability"]
                .as_array()
                .into_iter()
                .flatten()
                .take(10)
                .map(|s| format!("{} {:.0}%", text(&s["feature"]), 100.0 * s["share"].as_f64().unwrap_or(0.0)))
                .collect();
            let _ = writeln!(out, "chosen most often across folds: {}", chosen.join(", "));
        }
        "forward_select" => {
            let _ = writeln!(out, "forward selection for `{}` with {}", text(&r["target"]), text(&r["model"]));
            for (i, s) in r["steps"].as_array().into_iter().flatten().enumerate() {
                let _ = writeln!(out, "  {:>2}. + {:<24} cross-validated accuracy {} ± {}", i + 1, text(&s["added"]), num(&s["cv_accuracy"]), num(&s["cv_std"]));
            }
            let _ = writeln!(out, "recommended (one-standard-error rule): {}", list(&r["recommended"]));
            let _ = writeln!(
                out,
                "test accuracy of the recommendation {} (baseline {})",
                num(&r["test_accuracy"]),
                num(&r["baseline_accuracy"])
            );
        }
        "lasso" => {
            let _ = writeln!(out, "LASSO path for `{}`", text(&r["target"]));
            let _ = writeln!(out, "penalty: lowest error at {}, chosen (one standard error) {}", num(&r["alpha_min"]), num(&r["alpha_1se"]));
            for s in r["selected"].as_array().into_iter().flatten() {
                let _ = writeln!(
                    out,
                    "  keeps {:<24} {} per unit ({} per standard deviation)",
                    text(&s["feature"]),
                    num(&s["coefficient"]),
                    num(&s["standardized"])
                );
            }
            let _ = writeln!(out, "drops: {}", list(&r["dropped"]));
            let _ = writeln!(out, "test R² {} (baseline {})", num(&r["test"]["r2"]), num(&r["baseline"]["r2"]));
        }
        "cluster" => {
            let _ = writeln!(out, "cluster with {}", text(&r["method"]));
            rows_line(&mut out, source, r);
            let _ = writeln!(out, "{} clusters, sizes {}; {} noise", r["n_clusters"], list(&r["sizes"]), r["noise"]);
            let _ = writeln!(
                out,
                "silhouette {} (higher is better), Davies-Bouldin {} (lower is better), Calinski-Harabasz {}",
                num(&r["silhouette"]),
                num(&r["davies_bouldin"]),
                num(&r["calinski_harabasz"])
            );
        }
        "pca" => {
            let _ = writeln!(out, "principal components of {}", list(&r["features"]));
            let _ = writeln!(out, "{} components keep 95% of the variance", r["components_for_95"]);
            for (i, c) in r["components"].as_array().into_iter().flatten().enumerate() {
                let lead: Vec<String> = c["loadings"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(3)
                    .map(|l| format!("{} {}", num(&l["weight"]), text(&l["feature"])))
                    .collect();
                let _ = writeln!(
                    out,
                    "  PC{}: {} of the variance (cumulative {}); {}",
                    i + 1,
                    num(&c["explained_ratio"]),
                    num(&c["cumulative_ratio"]),
                    lead.join(", ")
                );
            }
        }
        "rules" => {
            let _ = writeln!(out, "{} baskets", r["transactions"]);
            let rules = r["rules"].as_array().map_or(0, Vec::len);
            let total = r["rules_total"].as_u64().map_or(rules, |t| t as usize);
            let _ = writeln!(out, "{total} rules; strongest first:");
            for x in r["rules"].as_array().into_iter().flatten().take(20) {
                let _ = writeln!(
                    out,
                    "  {{{}}} → {{{}}}  support {}  confidence {}  lift {}",
                    list(&x["antecedent"]),
                    list(&x["consequent"]),
                    num(&x["support"]),
                    num(&x["confidence"]),
                    num(&x["lift"])
                );
            }
        }
        "residuals" => {
            let _ = writeln!(out, "simulation against measurement, {} points compared", r["n"]);
            for (label, key) in [("RMSE", "rmse"), ("MAE", "mae"), ("bias", "bias"), ("max |error|", "max_abs"), ("NRMSE", "nrmse"), ("R²", "r2")] {
                let _ = writeln!(out, "  {:<12} {}", label, num(&r[key]));
            }
        }
        _ => {
            let _ = writeln!(out, "{}", serde_json::to_string_pretty(reply).unwrap_or_default());
        }
    }
    if let Some(cautions) = r["cautions"].as_array().filter(|c| !c.is_empty()) {
        out.push_str("cautions:\n");
        for c in cautions {
            let _ = writeln!(out, "  [{}] {}", text(&c["code"]), text(&c["message"]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_files(name: &str) -> Result<PathBuf> {
        Err(bad(format!("no file access in this test (`{name}`)")))
    }

    fn blobs() -> Value {
        let rows: Vec<Value> = (0..40)
            .map(|i| {
                let base = if i < 20 { 0.0 } else { 10.0 };
                json!({
                    "x": base + (i % 5) as f64 * 0.3,
                    "y": base + (i % 4) as f64 * 0.2,
                    "kind": if i < 20 { "a" } else { "b" },
                })
            })
            .collect();
        Value::Array(rows)
    }

    #[test]
    fn a_classify_request_runs_on_inline_rows() {
        let reply = run(&json!({ "run": "classify", "data": blobs(), "target": "kind" }), &no_files).unwrap();
        assert_eq!(reply["source"], "inline");
        assert_eq!(reply["report"]["accuracy"], 1.0);
        // Every numeric column but the target became a feature.
        assert_eq!(reply["report"]["features"], json!(["x", "y"]));
        let text = render(&reply);
        assert!(text.contains("accuracy") && text.contains("[perfect_separator]"), "{text}");
    }

    #[test]
    fn a_misspelled_field_is_refused_by_name() {
        let e = run(&json!({ "run": "classify", "data": blobs(), "targt": "kind" }), &no_files).unwrap_err();
        assert!(e.to_string().contains("`targt`"), "{e}");
        let e = run(&json!({ "run": "classify", "data": blobs(), "target": "kind",
                             "model": { "name": "decision_tree", "depth": 3 } }), &no_files)
            .unwrap_err();
        assert!(e.to_string().contains("`depth`") && e.to_string().contains("max_depth"), "{e}");
        let e = run(&json!({ "run": "clasify", "data": blobs() }), &no_files).unwrap_err();
        assert!(e.to_string().contains("classify"), "{e}");
    }

    #[test]
    fn model_settings_reach_the_model() {
        let reply = run(
            &json!({ "run": "classify", "data": blobs(), "target": "kind",
                     "model": { "name": "k_nearest_neighbors", "k": 3 },
                     "options": { "seed": 4, "cv_folds": 3 } }),
            &no_files,
        )
        .unwrap();
        assert_eq!(reply["report"]["model"], "k_nearest_neighbors(k=3, distance_weighted=false)");
        assert_eq!(reply["report"]["options"]["seed"], 4);
        assert_eq!(reply["report"]["cv_accuracy"]["scores"].as_array().map(Vec::len), Some(3));
    }

    #[test]
    fn an_integer_target_needs_the_task_said() {
        let rows = json!([{ "a": 1.0, "t": 0 }, { "a": 2.0, "t": 1 }, { "a": 3.0, "t": 0 }, { "a": 4.0, "t": 1 }]);
        let e = run(&json!({ "run": "rank", "data": rows, "target": "t" }), &no_files).unwrap_err();
        assert!(e.to_string().contains("`task`"), "{e}");
        let reply = run(&json!({ "run": "rank", "data": rows, "target": "t", "task": "classification" }), &no_files);
        assert!(reply.is_ok(), "{reply:?}");
    }

    #[test]
    fn describe_summarizes_every_column() {
        let reply = run(&json!({ "run": "describe", "data": blobs() }), &no_files).unwrap();
        // Inline rows may arrive with their keys sorted, so look columns up
        // by name rather than position.
        let col = |name: &str| {
            reply["report"]["columns"].as_array().unwrap().iter().find(|c| c["name"] == name).cloned().unwrap()
        };
        assert_eq!(reply["report"]["rows"], 40);
        assert_eq!(col("kind")["distinct"], 2);
        assert_eq!(col("x")["dtype"], "f64");
        assert!((col("x")["min"].as_f64().unwrap()).abs() < 1e-12);
        assert!(render(&reply).contains("40 rows"));
    }

    #[test]
    fn rules_come_from_inline_order_lines() {
        let rows = json!([
            { "order": 1, "sku": "A" }, { "order": 1, "sku": "B" },
            { "order": 2, "sku": "A" }, { "order": 2, "sku": "B" },
            { "order": 3, "sku": "A" }
        ]);
        let reply = run(
            &json!({ "run": "rules", "data": rows, "basket": "order", "item": "sku", "min_support": 0.5 }),
            &no_files,
        )
        .unwrap();
        assert_eq!(reply["report"]["transactions"], 3);
        assert!(render(&reply).contains("{B} → {A}"), "{}", render(&reply));
    }

    #[test]
    fn a_file_is_read_through_the_resolver() {
        let dir = std::env::temp_dir().join(format!("eustress-mine-api-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("m.csv");
        std::fs::write(&csv, "measured,simulated\n1,1.5\n2,2.5\n3,3.5\n").unwrap();
        let resolve = |name: &str| -> Result<PathBuf> { Ok(dir.join(name)) };
        let reply =
            run(&json!({ "run": "residuals", "file": "m.csv", "measured": "measured", "simulated": "simulated" }), &resolve)
                .unwrap();
        assert_eq!(reply["report"]["n"], 3);
        assert!((reply["report"]["bias"].as_f64().unwrap() - 0.5).abs() < 1e-12);
        let e = run(&json!({ "run": "describe", "file": "m.xlsx" }), &resolve).unwrap_err();
        assert!(e.to_string().contains(".csv"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cluster_reply_leaves_out_per_row_assignments_unless_asked() {
        let method = json!({ "name": "k_means", "k": 2 });
        let short = run(&json!({ "run": "cluster", "data": blobs(), "method": method }), &no_files).unwrap();
        assert!(short["report"].get("assignments").is_none());
        assert_eq!(short["report"]["n_clusters"], 2);
        let full = run(&json!({ "run": "cluster", "data": blobs(), "method": method, "assignments": true }), &no_files)
            .unwrap();
        assert_eq!(full["report"]["assignments"].as_array().map(Vec::len), Some(40));
    }
}

//! `eustress data`: data mining from the command line.
//!
//! A thin wrapper over `eustress_data::mine::api`, the JSON front door the
//! `mine_data` agent tool also uses: the flags build one request, the request
//! runs, and the reply prints as a summary (or as JSON with `--json`). An agent
//! and a person at a terminal therefore run the same procedure and read the
//! same report.

use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use clap::builder::PossibleValuesParser;
use clap::Args;
use eustress_data::mine::api;
use serde_json::{json, Map, Value};

const EXAMPLES: &str = "\
Examples:
  eustress data describe deliveries.csv
  eustress data classify deliveries.csv --target late --group supplier
  eustress data classify deliveries.csv --target late --model decision_tree:max_depth=4
  eustress data compare deliveries.csv --target late --model decision_tree --model k_nearest_neighbors:k=7
  eustress data regress sensors.parquet --target pressure --model lasso:alpha=0.05
  eustress data rank deliveries.csv --target late
  eustress data selection-check genes.csv --target outcome --keep 10
  eustress data cluster sites.csv --method k_means:k=4
  eustress data rules order_lines.csv --basket order_id --item sku --min-support 0.02
  eustress data residuals run.csv --measured pressure --simulated sim_pressure
  eustress data residuals bench.csv --time t --value temp --simulated-file sim.csv

Every supervised run splits before fitting, cross-validates inside the training
rows, and scores against a trivial baseline. Read the cautions it prints before
trusting a score; --strict turns any caution into a failing exit code.";

/// Mine a data file: describe it, fit and compare models, rank and select
/// features, cluster, find association rules, or score a simulation against
/// measurements. Every run reports the pitfalls it detected.
#[derive(Args, Debug)]
#[command(after_long_help = EXAMPLES)]
pub struct DataArgs {
    /// The run. `selection-check` and `forward-select` may also be written
    /// with underscores.
    #[arg(value_parser = PossibleValuesParser::new([
        "describe", "classify", "regress", "compare", "rank", "selection-check", "selection_check",
        "forward-select", "forward_select", "lasso", "cluster", "pca", "rules", "residuals",
    ]))]
    run: String,

    /// Data file: .csv, .jsonl, .ndjson, .json (an array of rows) or .parquet.
    /// May be left out when --request names one.
    file: Option<PathBuf>,

    /// Column to predict.
    #[arg(long)]
    target: Option<String>,

    /// Columns to learn from, comma-separated. Default: every numeric column
    /// except the target and the group.
    #[arg(long, value_delimiter = ',')]
    features: Vec<String>,

    /// `classification` or `regression`, for compare and rank. Inferred from
    /// the target's type; required when the target holds whole numbers.
    #[arg(long)]
    task: Option<String>,

    /// Model, as `name` or `name:key=value,key=value`. Repeat it for compare.
    /// Classifiers: logistic_regression, decision_tree, k_nearest_neighbors,
    /// gaussian_naive_bayes. Regressors: linear_regression, ridge_regression,
    /// lasso.
    #[arg(long)]
    model: Vec<String>,

    /// Share of rows held out for the test score (default 0.25).
    #[arg(long)]
    test_fraction: Option<f64>,

    /// Seed for the split, the folds and the permutations (default 0).
    #[arg(long)]
    seed: Option<u64>,

    /// Cross-validation folds within the training rows (default 5; 0 skips).
    #[arg(long)]
    folds: Option<usize>,

    /// Shuffles per feature for permutation importance (default 5; 0 skips).
    #[arg(long)]
    repeats: Option<usize>,

    /// Entity column (a supplier, a machine): keeps each entity's rows on one
    /// side of every split.
    #[arg(long)]
    group: Option<String>,

    /// selection-check: how many features to keep.
    #[arg(long)]
    keep: Option<usize>,

    /// forward-select: the most features to add.
    #[arg(long)]
    max_features: Option<usize>,

    /// cluster: `k_means:k=4`, `dbscan:eps=0.5,min_samples=5` or
    /// `agglomerative:n_clusters=3,linkage=ward`.
    #[arg(long)]
    method: Option<String>,

    /// cluster and pca: use the features in their own units instead of
    /// standardizing them.
    #[arg(long)]
    raw: bool,

    /// cluster: include the cluster of every row in the JSON reply.
    #[arg(long)]
    assignments: bool,

    /// rules: the basket id column of an order-lines table.
    #[arg(long)]
    basket: Option<String>,

    /// rules: the item column of an order-lines table.
    #[arg(long)]
    item: Option<String>,

    /// rules: one true/false column per item, comma-separated, instead of
    /// --basket and --item.
    #[arg(long, value_delimiter = ',')]
    flags: Vec<String>,

    /// rules: minimum share of baskets holding an itemset (default 0.1).
    #[arg(long)]
    min_support: Option<f64>,

    /// rules: minimum confidence of a rule (default 0.5).
    #[arg(long)]
    min_confidence: Option<f64>,

    /// rules: largest itemset (default 4).
    #[arg(long)]
    max_len: Option<usize>,

    /// rules: how many itemsets and rules to return (default 50).
    #[arg(long)]
    limit: Option<usize>,

    /// residuals: the measured column, beside --simulated in the same file.
    #[arg(long)]
    measured: Option<String>,

    /// residuals: the simulated column.
    #[arg(long)]
    simulated: Option<String>,

    /// residuals on two clocks: the measured file's time column.
    #[arg(long)]
    time: Option<String>,

    /// residuals on two clocks: the measured file's value column.
    #[arg(long)]
    value: Option<String>,

    /// residuals on two clocks: the simulation's data file.
    #[arg(long)]
    simulated_file: Option<PathBuf>,

    /// residuals on two clocks: the simulation's time column (default: --time).
    #[arg(long)]
    sim_time: Option<String>,

    /// residuals on two clocks: the simulation's value column (default: --value).
    #[arg(long)]
    sim_value: Option<String>,

    /// Start from a JSON request file (`-` reads standard input); flags given
    /// here override its fields.
    #[arg(long)]
    request: Option<PathBuf>,

    /// Print the full JSON reply instead of a summary.
    #[arg(long)]
    json: bool,

    /// Exit with an error when the report carries any caution.
    #[arg(long)]
    strict: bool,
}

/// `name` or `name:key=value,key=value`. Values read as JSON where they parse
/// (numbers, true, false) and as text otherwise.
fn spec(text: &str) -> Result<Value> {
    let (name, settings) = text.split_once(':').unwrap_or((text, ""));
    let mut m = Map::new();
    m.insert("name".to_string(), json!(name.trim()));
    for pair in settings.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| anyhow!("`{pair}` in `{text}` is not key=value"))?;
        let value = value.trim();
        m.insert(key.trim().to_string(), serde_json::from_str(value).unwrap_or_else(|_| json!(value)));
    }
    Ok(Value::Object(m))
}

fn put<T: Into<Value>>(req: &mut Map<String, Value>, key: &str, value: Option<T>) {
    if let Some(v) = value {
        req.insert(key.to_string(), v.into());
    }
}

fn path_text(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

/// The JSON request the flags describe.
fn request(args: &DataArgs) -> Result<Value> {
    let mut req = match &args.request {
        None => Map::new(),
        Some(p) => {
            let text = if p.as_os_str() == "-" {
                std::io::read_to_string(std::io::stdin()).context("reading the request from standard input")?
            } else {
                std::fs::read_to_string(p).with_context(|| format!("reading the request {}", p.display()))?
            };
            match serde_json::from_str(&text).context("the request is not valid JSON")? {
                Value::Object(m) => m,
                _ => bail!("the request must be a JSON object"),
            }
        }
    };
    req.insert("run".to_string(), json!(args.run.replace('-', "_")));
    put(&mut req, "file", args.file.as_deref().map(path_text));
    put(&mut req, "target", args.target.clone());
    if !args.features.is_empty() {
        req.insert("features".to_string(), json!(args.features));
    }
    put(&mut req, "task", args.task.clone());
    if req["run"] == "compare" {
        if !args.model.is_empty() {
            let models = args.model.iter().map(|m| spec(m)).collect::<Result<Vec<_>>>()?;
            req.insert("models".to_string(), Value::Array(models));
        }
    } else {
        match args.model.as_slice() {
            [] => {}
            [one] => {
                req.insert("model".to_string(), spec(one)?);
            }
            _ => bail!("give one --model, or run `compare` to judge several"),
        }
    }

    let mut options = match req.remove("options") {
        None => Map::new(),
        Some(Value::Object(m)) => m,
        Some(_) => bail!("`options` in the request must be an object"),
    };
    put(&mut options, "test_fraction", args.test_fraction);
    put(&mut options, "seed", args.seed);
    put(&mut options, "cv_folds", args.folds);
    put(&mut options, "importance_repeats", args.repeats);
    put(&mut options, "group", args.group.clone());
    if !options.is_empty() {
        req.insert("options".to_string(), Value::Object(options));
    }

    put(&mut req, "keep", args.keep);
    put(&mut req, "max_features", args.max_features);
    if let Some(m) = &args.method {
        req.insert("method".to_string(), spec(m)?);
    }
    if args.raw {
        req.insert("standardize".to_string(), json!(false));
    }
    if args.assignments {
        req.insert("assignments".to_string(), json!(true));
    }
    put(&mut req, "basket", args.basket.clone());
    put(&mut req, "item", args.item.clone());
    if !args.flags.is_empty() {
        req.insert("flags".to_string(), json!(args.flags));
    }
    put(&mut req, "min_support", args.min_support);
    put(&mut req, "min_confidence", args.min_confidence);
    put(&mut req, "max_len", args.max_len);
    put(&mut req, "limit", args.limit);
    put(&mut req, "measured", args.measured.clone());
    put(&mut req, "simulated", args.simulated.clone());
    put(&mut req, "time", args.time.clone());
    put(&mut req, "value", args.value.clone());
    put(&mut req, "simulated_file", args.simulated_file.as_deref().map(path_text));
    put(&mut req, "sim_time", args.sim_time.clone());
    put(&mut req, "sim_value", args.sim_value.clone());
    Ok(Value::Object(req))
}

/// Run `eustress data`. Files are read relative to the current directory.
pub fn run(args: DataArgs) -> Result<()> {
    let req = request(&args)?;
    let reply = api::run(&req, &|name| Ok(PathBuf::from(name))).map_err(|e| anyhow!("{e}"))?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&reply)?);
    } else {
        print!("{}", api::render(&reply));
    }
    let cautions = reply["report"]["cautions"].as_array().map_or(0, Vec::len);
    if args.strict && cautions > 0 {
        bail!("{cautions} caution(s) raised, and --strict was given");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Probe {
        #[command(flatten)]
        args: DataArgs,
    }

    fn req(argv: &[&str]) -> Value {
        let probe = Probe::try_parse_from(std::iter::once("data").chain(argv.iter().copied())).unwrap();
        request(&probe.args).unwrap()
    }

    #[test]
    fn flags_become_the_json_request() {
        let r = req(&[
            "classify", "d.csv", "--target", "late", "--features", "a,b", "--seed", "7", "--group", "supplier",
            "--model", "decision_tree:max_depth=4,criterion=entropy",
        ]);
        assert_eq!(r["run"], "classify");
        assert_eq!(r["file"], "d.csv");
        assert_eq!(r["features"], json!(["a", "b"]));
        assert_eq!(r["options"], json!({ "seed": 7, "group": "supplier" }));
        assert_eq!(r["model"], json!({ "name": "decision_tree", "max_depth": 4, "criterion": "entropy" }));
    }

    #[test]
    fn compare_takes_every_model_and_dashed_runs_map_to_the_api() {
        let r = req(&["compare", "d.csv", "--target", "t", "--model", "decision_tree", "--model", "k_nearest_neighbors:k=7"]);
        assert_eq!(r["models"], json!([{ "name": "decision_tree" }, { "name": "k_nearest_neighbors", "k": 7 }]));
        assert_eq!(req(&["selection-check", "d.csv", "--target", "t"])["run"], "selection_check");
    }

    #[test]
    fn two_models_outside_compare_are_refused() {
        let probe = Probe::try_parse_from(["data", "classify", "d.csv", "--model", "a", "--model", "b"]).unwrap();
        assert!(request(&probe.args).is_err());
    }
}

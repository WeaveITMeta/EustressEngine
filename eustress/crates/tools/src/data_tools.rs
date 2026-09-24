//! Data mining over files in the Universe: the `mine_data` tool.
//!
//! A thin adapter over `eustress_data::mine::api`, the JSON front door the
//! `eustress data` command also uses, so an agent and a person at a terminal
//! run the same procedure and get the same report. The tool only reads: files
//! resolve inside the Universe sandbox, and the reply is the report itself
//! (a text summary for reading, the full report as structured data).

use std::path::PathBuf;

use eustress_data::mine::api;
use eustress_data::DataError;

use crate::modes::WorkshopMode;
use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};

/// The same sandbox rule as the file tools: relative to the Universe root,
/// never climbing out of it.
fn resolve_in_universe(ctx: &ToolContext, relative: &str) -> Option<PathBuf> {
    let cleaned = relative.replace('\\', "/");
    if cleaned.contains("..") {
        return None;
    }
    let resolved = ctx.universe_root.join(&cleaned);
    resolved.starts_with(&ctx.universe_root).then_some(resolved)
}

pub struct MineDataTool;

impl ToolHandler for MineDataTool {
    /// Read-only: reads a data file and computes; writes nothing.
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "mine_data",
            description: "Mine a data file in the Universe (CSV, JSONL, a JSON array of rows, or Parquet) \
                without writing code. Runs: describe (columns, types, missing values, summary \
                statistics; start here), classify, regress, compare (several models on identical \
                splits), rank (filter scores of every feature), selection_check (how much choosing \
                features on all rows inflates accuracy), forward_select, lasso (penalty chosen by \
                cross-validation), cluster, pca, rules (association rules from baskets), and \
                residuals (a simulation scored against measured data). Every supervised run splits \
                before fitting, cross-validates inside the training rows, scores against a trivial \
                baseline, and returns cautions with stable codes: perfect_separator, target_copy, \
                train_test_overlap, selection_bias, overfit, class_imbalance, no_better_than_baseline \
                and more. Read the cautions before trusting a score. When rows repeat per entity \
                (a supplier, a machine), set options.group to that column so no entity lands on both \
                sides of the split. The same request and seed always give the same report.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "run": {
                        "type": "string",
                        "enum": api::RUNS,
                        "description": "Which run."
                    },
                    "file": {
                        "type": "string",
                        "description": "Data file, relative to the Universe root: .csv, .jsonl, .ndjson, .json or .parquet."
                    },
                    "data": {
                        "type": "array",
                        "items": { "type": "object" },
                        "description": "Rows inline, one object per row, instead of `file`."
                    },
                    "target": {
                        "type": "string",
                        "description": "Column to predict (classify, regress, compare, rank, selection_check, forward_select, lasso)."
                    },
                    "features": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Columns to learn from. Default: every numeric column except the target and the group column."
                    },
                    "task": {
                        "type": "string",
                        "enum": ["classification", "regression"],
                        "description": "For compare and rank. Inferred from the target's type; required when the target holds whole numbers."
                    },
                    "model": {
                        "description": "A model name, or {\"name\": ..., settings}. Classifiers (default decision_tree): logistic_regression {l2, max_iter, tol}, decision_tree {criterion: gini|entropy, max_depth, min_samples_split, min_samples_leaf}, k_nearest_neighbors {k, distance_weighted}, gaussian_naive_bayes {var_smoothing}. Regressors (default linear_regression): linear_regression, ridge_regression {ridge}, lasso {alpha, max_iter, tol}."
                    },
                    "models": {
                        "type": "array",
                        "description": "For compare: the models to judge, each a name or {name, settings}. Default: every classifier or every regressor."
                    },
                    "options": {
                        "type": "object",
                        "description": "Evaluation settings: test_fraction (default 0.25), seed (0), cv_folds (5), importance_repeats (5), group (entity column name).",
                        "properties": {
                            "test_fraction": { "type": "number" },
                            "seed": { "type": "integer", "minimum": 0 },
                            "cv_folds": { "type": "integer", "minimum": 0 },
                            "importance_repeats": { "type": "integer", "minimum": 0 },
                            "group": { "type": "string" }
                        }
                    },
                    "keep": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "For selection_check: how many features to keep."
                    },
                    "max_features": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "For forward_select: the most features to add."
                    },
                    "method": {
                        "description": "For cluster: {\"name\": \"k_means\", \"k\", \"n_init\", \"max_iter\", \"seed\"}, {\"name\": \"dbscan\", \"eps\", \"min_samples\"} or {\"name\": \"agglomerative\", \"n_clusters\", \"linkage\": ward|single|complete|average}."
                    },
                    "standardize": {
                        "type": "boolean",
                        "description": "For cluster and pca: scale every feature to unit variance first (default true)."
                    },
                    "assignments": {
                        "type": "boolean",
                        "description": "For cluster: include the cluster of every row (default false)."
                    },
                    "basket": { "type": "string", "description": "For rules: the basket id column of an order-lines table." },
                    "item": { "type": "string", "description": "For rules: the item column of an order-lines table." },
                    "flags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "For rules: one true/false column per item, instead of basket and item."
                    },
                    "min_support": { "type": "number", "description": "For rules (default 0.1)." },
                    "min_confidence": { "type": "number", "description": "For rules (default 0.5)." },
                    "max_len": { "type": "integer", "description": "For rules: largest itemset (default 4)." },
                    "limit": { "type": "integer", "description": "For rules: how many itemsets and rules to return (default 50)." },
                    "measured": { "type": "string", "description": "For residuals: the measured column, beside `simulated` in the same file." },
                    "simulated": { "type": "string", "description": "For residuals: the simulated column." },
                    "time": { "type": "string", "description": "For residuals on two clocks: the measured file's time column." },
                    "value": { "type": "string", "description": "For residuals on two clocks: the measured file's value column." },
                    "simulated_file": { "type": "string", "description": "For residuals on two clocks: the simulation's data file." },
                    "simulated_data": { "type": "array", "items": { "type": "object" }, "description": "The simulation's rows inline, instead of simulated_file." },
                    "sim_time": { "type": "string", "description": "The simulation's time column (default: same name as `time`)." },
                    "sim_value": { "type": "string", "description": "The simulation's value column (default: same name as `value`)." }
                },
                "required": ["run"]
            }),
            modes: &[WorkshopMode::General],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let resolve = |name: &str| -> eustress_data::Result<PathBuf> {
            resolve_in_universe(ctx, name)
                .ok_or_else(|| DataError::Schema(format!("`{name}` is outside the Universe folder")))
        };
        let (success, content, structured_data) = match api::run(&input, &resolve) {
            Ok(reply) => (true, api::render(&reply), Some(reply)),
            Err(e) => (false, e.to_string(), None),
        };
        ToolResult {
            tool_name: "mine_data".to_string(),
            tool_use_id: String::new(),
            success,
            content,
            structured_data,
            stream_topic: None,
        }
    }
}

---
name: eustress-data-mining
description: >-
  Mines data in Eustress without writing code: describes a data file, pages through its
  rows, answers graph questions over an edge list (sole-sourced parts, what losing a
  supplier reaches, paths, cycles), fits and compares classifiers and regressors, ranks and
  selects features, clusters rows, runs PCA, finds association rules in baskets, and scores
  a simulation against measured data. Every run splits before fitting, cross-validates,
  compares against a baseline, and reports cautions such as target leaks, perfect
  separators, duplicate records across the split, selection bias and overfitting. Use when
  the user asks what predicts something, which features matter, how rows group, which items
  go together, which SKUs hang on one supplier, whether a simulation matches measurements,
  or wants scikit-learn style analysis inside Eustress. Covers the mine_data tool (Workshop
  and MCP), the `eustress data` command, and scripts (Luau `DataService`, Rune
  `eustress::data`), which also query the Space's Connectors with Cypher, SQL or GraphQL.
---

# Data mining in Eustress

Three doors lead to one procedure:

| Door | Who uses it | Example |
|---|---|---|
| `mine_data` tool | the Workshop agent, any MCP client | `mine_data {run: "classify", file: "Datasets/deliveries.csv", target: "late"}` |
| `eustress data` | a shell, a script, CI | `eustress data classify Datasets/deliveries.csv --target late` |
| Scripts | Luau and Rune in a Space | `DataService:Mine({ run = "classify", file = "Datasets/deliveries.csv", target = "late" })` |

All three build the same JSON request and return the same report, so a result found in one
reproduces in the others. The tool reads files relative to the Universe root, the command
relative to the current folder, and a script relative to its Space. Files can be `.csv`,
`.jsonl`, `.ndjson`, `.json` (an array of row objects) or `.parquet`. The standalone MCP
server reads every format but Parquet; Studio's Workshop, scripts and the command read
Parquet too. Small tables can go inline as `data: [{...}, {...}]` instead of `file`, and a
script can name one of the Space's Connectors as `connector` (see Scripts and Connectors).

## The order of work

1. **describe** the file. It lists every column with its type, unit, missing values and
   summary statistics, or its most common values for text. Pick the target and features
   from this, never from guesses about the column names.
2. **rank** the features against the target. It scores each one by mutual information
   (any dependence) and by ANOVA F or Pearson r (linear only). A feature high on mutual
   information and low on the linear score has a curved or threshold relationship.
3. **classify** (a class target) or **regress** (a number), or **compare** several models
   on identical splits.
4. **Read the cautions** before quoting any score (see below).
5. When features were chosen from a ranking, run **selection_check** to measure how much
   that choice inflates the score, then **forward_select** or **lasso** to choose them
   honestly.

Leave `features` out and every numeric column except the target and the group column is
used; the report lists which ones ran.

## Requests by question

| Question | Run | Tool request (the command takes the same fields as flags) |
|---|---|---|
| What is in this file? | describe | `{run: "describe", file: "orders.csv"}` |
| Show me the rows | rows | `{run: "rows", file: "orders.csv", columns: ["sku", "qty"], limit: 20, offset: 40}` |
| Which SKUs have one supplier? | graph | `{run: "graph", file: "supply.csv", from: "supplier", to: "sku", default_rel: "SUPPLIES", ask: "sole_sourced", relation: "SUPPLIES"}` |
| What predicts late delivery? | classify | `{run: "classify", file: "deliveries.csv", target: "late", options: {group: "supplier"}}` |
| Which model is best here? | compare | `{run: "compare", file: "deliveries.csv", target: "late"}` |
| What drives the price? | regress | `{run: "regress", file: "quotes.csv", target: "unit_price", model: "lasso"}` |
| Which features matter at all? | rank | `{run: "rank", file: "deliveries.csv", target: "late"}` |
| Did choosing features fool me? | selection_check | `{run: "selection_check", file: "genes.csv", target: "outcome", keep: 10}` |
| Which small set of features is enough? | forward_select, lasso | `{run: "lasso", file: "sensors.csv", target: "pressure"}` |
| How do the sites group? | cluster | `{run: "cluster", file: "sites.csv", method: {name: "k_means", k: 4}}` |
| How many dimensions does this data really have? | pca | `{run: "pca", file: "sensors.csv"}` |
| What sells together? | rules | `{run: "rules", file: "order_lines.csv", basket: "order_id", item: "sku", min_support: 0.02}` |
| Does the simulation match the bench test? | residuals | `{run: "residuals", file: "bench.csv", time: "t", value: "temp", simulated_file: "sim.csv"}` |

Models are a name or a name with settings: `"decision_tree"` or
`{name: "decision_tree", max_depth: 4}`. On the command line, `--model decision_tree:max_depth=4`;
repeat `--model` for `compare`.

- Classifiers: `logistic_regression` {l2, max_iter, tol}, `decision_tree` {criterion: gini or
  entropy, max_depth, min_samples_split, min_samples_leaf}, `k_nearest_neighbors` {k,
  distance_weighted}, `gaussian_naive_bayes` {var_smoothing}. The default is a decision tree.
- Regressors: `linear_regression`, `ridge_regression` {ridge}, `lasso` {alpha, max_iter, tol}.
  The default is least squares.
- Clustering: `k_means` {k, n_init, max_iter, seed}, `dbscan` {eps, min_samples},
  `agglomerative` {n_clusters, linkage: ward, single, complete or average}.

`compare` and `rank` infer classification or regression from the target's type. A target of
whole numbers could be class codes or counts, so those need `task`.

A misspelled field is refused with the list of accepted names. Read the error and fix the
field; it never falls back to a default.

## Reading a report

- **Baseline first.** Classification reports the accuracy of always predicting the most
  common class; regression reports predicting the training mean. A score means something
  only by how far it clears that line.
- **Test, training, cross-validated.** The test score comes from rows held out before
  fitting. Cross-validation runs inside the training rows. A training score far above the
  test score means the model memorised.
- **Balanced accuracy** beats accuracy whenever one class dominates.
- **Importance** is the test score lost when one feature is shuffled. A feature near zero
  contributes nothing the model uses.
- **Coefficients** (linear models) are in the target's units per unit of the feature.

## Cautions, and what to do

Every caution has a stable `code`. Act on it before quoting a result.

| Code | Meaning | Do |
|---|---|---|
| `perfect_separator` | one feature alone sorts every row into its class | check the feature is known before the outcome; an ID, a timestamp or a status set afterwards is a leak |
| `target_copy` | a feature tracks a numeric target at \|r\| >= 0.999 | the same quantity in other units, or computed from it: drop it |
| `dominant_feature` | one feature carries nearly all of the model | treat as a leak until shown otherwise |
| `train_test_overlap` | test rows repeat training rows exactly | duplicates or repeated entities: deduplicate, or set `options.group` |
| `no_better_than_baseline` | the model does not beat the trivial prediction | say so plainly; the features as given do not predict the target |
| `overfit` | training score far above test score | shallower tree, larger k, stronger penalty, or more rows |
| `unstable_cv` | fold scores spread widely | more rows or fewer features; do not quote the mean alone |
| `class_imbalance` | one class is 80% or more of the rows | read balanced accuracy and per-class recall |
| `rare_class` | a class has fewer training rows than folds | its per-class scores rest on a handful of rows |
| `selection_bias` | choosing features on all rows inflated accuracy | choose inside cross-validation: forward_select or lasso |
| `unstable_selection` | the chosen features change fold to fold | no subset is well supported; report the ranking instead |
| `collinear_features` | two features move in lockstep | their coefficients and importances split one effect; keep one |
| `constant_feature` | a feature never varies | drop it |
| `few_rows` | fewer than ten training rows per feature | expect scores to move with the seed |
| `rows_dropped` | over a tenth of rows had a missing value | ask whether missingness relates to the target |
| `mixed_scales` | unstandardized features differ a thousandfold in variance | standardize (the default) unless the units match on purpose |
| `single_cluster`, `mostly_noise` | clustering found no structure | for DBSCAN change `eps`; standardize first |
| `not_converged` | an iterative fit hit its limit | raise `max_iter` |
| `ranked_on_test`, `group_missing` | as described in the message | read the message |

On the command line `--strict` exits with an error when any caution is raised, which suits a
pipeline that must stop on a leak.

## Entities: the group option

When rows repeat per supplier, machine, patient or batch, a random split puts one entity on
both sides and the test score measures recognition rather than prediction. Set
`options: {group: "supplier"}` (`--group supplier`): the split and the folds then keep each
entity's rows together, and the score describes suppliers the model has never seen.

## Feature selection without fooling yourself

Choosing features by looking at all the rows, then scoring a model on those same rows,
invents accuracy: with many features and few rows some correlate with the target by chance,
and the choice finds exactly those. `selection_check` shows the size of the effect by
choosing twice, once on all rows (the mistake) and once inside each fold (honest). On pure
noise the mistake can report 0.73 accuracy where the honest estimate is 0.35.

- `forward_select` adds features greedily on the training rows, recommends the smallest set
  within one standard error of the best, and scores that set on held-out rows.
- `lasso` searches its penalty by cross-validation and keeps the sparsest model within one
  standard error of the best; features it drives to zero are listed as dropped.

## Graph questions over an edge list

`graph` reads the table as edges: `from` and `to` name the endpoint columns, and `rel` the
column holding each edge's relation type. Without `rel`, every edge takes `default_rel`
(RELATED unless set). The other columns stay on the edges as properties. `ask` picks the
question:

| ask | Answers | Fields |
|---|---|---|
| `summary` (the default) | the ten nodes with the most outgoing and the most incoming edges | `relation` narrows it |
| `neighbors` | the nodes one edge away | `node`; `direction`, `relation` |
| `traverse` | everything within `depth` edges (3 by default), with each node's hop count | `node`; `depth`, `direction`, `relation` |
| `path` | the shortest path between two nodes | `node`, `to_node`; `relation` |
| `sole_sourced` | nodes with exactly one inbound `relation` edge: single points of failure | `relation` |
| `unsourced` | nodes with no inbound `relation` edge: raw inputs, or gaps in the data | `relation` |
| `impact` | everything that losing `node` would reach | `node`, `relation`; `direction` |
| `cycles` | loops, such as a circular bill of materials | `relation` narrows it |

`direction` is `out` (the default), `in` or `both`. `impact` walks `relation` edges backwards
unless `direction` is given, which suits edges that point at what they depend on (an assembly
CONTAINS its part); for edges that point from a supplier to what it supplies, add
`direction: "out"`. Lists stop at `limit` (200) and report the full `total`.

## Scripts and Connectors

Luau and Rune reach the same front door from inside a Space, in Play and in one-shot runs such
as the command bar and `execute_luau`. Of the three doors, scripts alone read the Space's
Connectors (the `DataService/<Name>` instances that hold an endpoint, a format and the name
of a secret):

```lua
local DataService = game:GetService("DataService")
local report = DataService:Mine({ run = "rank", file = "Datasets/deliveries.csv", target = "late" })
print(DataService:Render(report))
local links = DataService:Query("SupplierGraph",
    "MATCH (s:Supplier)-[:SUPPLIES]->(k:SKU) RETURN s.id AS supplier, k.id AS sku")
for _, row in ipairs(links) do
    print(row.supplier, row.sku)
end
```

```rune
use eustress::data;

let report = data::mine(#{ run: "graph", connector: "SupplierGraph", from: "supplier", to: "sku",
    default_rel: "SUPPLIES", ask: "sole_sourced", relation: "SUPPLIES" })?;
log_info(data::render(report)?);
let low = data::query("Warehouse", "SELECT sku, qty FROM stock WHERE qty < 10")?;
```

| Luau | Rune | Returns |
|---|---|---|
| `DataService:Mine(request)` | `data::mine(request)` | the reply: `run`, `source` and `report` |
| `DataService:Describe(file)` | `data::describe(file)` | the describe reply |
| `DataService:Query(connector, statement)` | `data::query(connector, statement)` | the Connector's rows, one table (Rune: object) per row |
| `DataService:Render(reply)` | `data::render(reply)` | the reply as readable text |

- Any request can name `connector` in place of `file`, and add `statement` to replace the
  Connector's configured query: Cypher for Neo4j, openCypher for Neptune, GraphQL for a
  GraphQL endpoint, SQL for PostgreSQL. Other kinds read a fixed resource and refuse a
  statement. An empty statement reads the configured query.
- Only enabled Connectors are read; a disabled one returns an error naming the switch.
- A query's rows are kept for five seconds, so a script asking every frame reaches the source
  a few times a minute. One query returns at most 10,000 rows; page larger sources with
  `rows` and `offset`, or narrow the statement.
- Reads block the calling script: query in setup, not in a per-frame loop.
- A missing cell arrives in Luau as `nil`. Rune calls return a `Result`, so `?` passes an
  error up.
- File names stay inside the Space: absolute paths, drive letters and `..` are refused.

Parameters bound to a Connector in the Properties panel read it through the same rules; see
the Parameters section of the Data Platform plan.

## Simulation against measurement

`residuals` is the score for a digital twin: calibration, choosing between forked branches,
or judging a change. Two columns of one file compare row by row (`measured`, `simulated`).
Two series on their own clocks compare by interpolating the simulation onto each measured
time, never extrapolating past its span. `bias` says which way the simulation leans,
`rmse` how far it misses, and `nrmse` makes quantities in different units comparable.

## Reproducibility

Every random choice (split, folds, permutations, k-means starts) comes from
`options.seed`, and every report records the options it ran with. The same request and seed
give the same splits on any machine and the same report on the same build. Change the seed to
see how much a result moves.

## Limits

- Data sits in memory as dense rows; nulls are dropped per run, never filled in.
- Agglomerative clustering takes up to 5000 rows; sample larger data or use k-means.
- The silhouette index uses at most 5000 evenly spaced rows.
- k-nearest neighbours and DBSCAN compare every pair of rows, so they slow past about
  50,000 rows.
- Cluster replies leave out the per-row assignments unless `assignments: true`; rules
  replies keep the first `limit` (50) itemsets and rules and give the totals.

Rust code (engine systems, tests, tools) calls the same runs directly through
`eustress_data::mine::workflow`, or the parts in `mine::classify`, `regress`, `cluster`,
`reduce`, `select`, `assoc` and `eval`.

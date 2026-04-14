//! # Stage 13: Meta — Introspection, Pipeline Composition & Audit
//!
//! Provides DSL-level observability: query function metadata, chain multiple
//! functions into reusable pipelines, and trace execution for debugging.
//!
//! ## Table of Contents
//! 1. Result types      — FunctionMeta, PipelineResult, AuditEntry
//! 2. MetaBridge        — thread-local function registry + audit log
//! 3. Rune functions    — introspect / compose_pipeline / audit
//! 4. Module registration
//!
//! ## Functions
//!
//! | Function                       | Purpose                                                    |
//! |--------------------------------|------------------------------------------------------------|
//! | `introspect(fn_name)`          | Query metadata about a DSL function                        |
//! | `compose_pipeline(steps_csv)`  | Chain DSL functions into a named reusable pipeline         |
//! | `audit(pipeline_name)`         | Retrieve the audit log for a named pipeline                |
//!
//! ## Rune Usage
//!
//! ```rune
//! use eustress::functions::meta;
//!
//! pub fn inspect_and_trace() {
//!     let info = meta::introspect("proximity::nearest");
//!     eustress::log_info(&format!("{}: {}", info.name, info.description));
//!
//!     meta::compose_pipeline("thermal_analysis", "measurement::measure,refinement::validate,statistical::regress");
//!     let log = meta::audit("thermal_analysis");
//!     eustress::log_info(&format!("{} audit entries", log.entry_count));
//! }
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use tracing::{info, warn};

// ============================================================================
// 1. Result Types
// ============================================================================

/// Metadata for a registered DSL function.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct FunctionMeta {
    /// Fully qualified function name (e.g. "proximity::nearest")
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub name: String,
    /// Module this function belongs to
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub module: String,
    /// Short description of what the function does
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub description: String,
    /// Comma-separated parameter types
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub param_types: String,
    /// Return type label
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub return_type: String,
    /// Approximate cost category: "cheap", "moderate", "expensive"
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub cost: String,
    /// Comma-separated dependency modules
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub dependencies: String,
    /// Whether the function was found in the registry
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub found: bool,
}

impl FunctionMeta {
    fn unknown(fn_name: &str) -> Self {
        Self {
            name: fn_name.to_string(),
            module: String::new(),
            description: format!("'{}' not found in function registry", fn_name),
            param_types: String::new(),
            return_type: String::new(),
            cost: "unknown".to_string(),
            dependencies: String::new(),
            found: false,
        }
    }
}

/// Result from `compose_pipeline()`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct PipelineResult {
    /// Pipeline name
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub name: String,
    /// Number of steps in the pipeline
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub step_count: i64,
    /// Comma-separated list of step function names (in order)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub steps_csv: String,
    /// Whether the pipeline was successfully registered
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub registered: bool,
}

/// A single audit log entry.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct AuditEntry {
    /// Step index within the pipeline
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub step: i64,
    /// Function name executed at this step
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub function: String,
    /// Status: "ok", "skipped", "error"
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub status: String,
    /// Duration in microseconds (0 if not measured)
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub duration_us: i64,
    /// Optional message or error detail
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub message: String,
}

/// The full audit log for a pipeline from `audit()`.
#[derive(Debug)]
#[cfg_attr(feature = "rune-dsl", derive(rune::Any))]
pub struct AuditLog {
    /// Pipeline name
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub pipeline: String,
    /// Ordered `AuditEntry` values
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub entries: rune::runtime::Vec,
    /// Total entries
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub entry_count: i64,
    /// Number of "error" entries
    #[cfg_attr(feature = "rune-dsl", rune(get))]
    pub error_count: i64,
}

impl AuditLog {
    fn empty(pipeline: &str) -> Self {
        Self {
            pipeline: pipeline.to_string(),
            entries: rune::runtime::Vec::new(),
            entry_count: 0,
            error_count: 0,
        }
    }
}

// ============================================================================
// 2. MetaBridge — thread-local function registry + pipeline store + audit log
// ============================================================================

/// Registration record for a single DSL function.
#[derive(Debug, Clone)]
pub struct FunctionRecord {
    pub name: String,
    pub module: String,
    pub description: String,
    pub param_types: String,
    pub return_type: String,
    pub cost: String,
    pub dependencies: String,
}

/// Bridge holding the function registry, named pipelines, and audit traces.
pub struct MetaBridge {
    /// Registered function metadata, keyed by "module::function"
    pub registry: HashMap<String, FunctionRecord>,
    /// Named pipelines: pipeline_name → ordered step names
    pub pipelines: HashMap<String, Vec<String>>,
    /// Audit log per pipeline: pipeline_name → Vec<AuditEntry>
    pub audit_logs: HashMap<String, Vec<AuditEntry>>,
}

impl MetaBridge {
    /// Create a bridge pre-populated with built-in DSL function records.
    pub fn new() -> Self {
        let mut bridge = Self {
            registry: HashMap::new(),
            pipelines: HashMap::new(),
            audit_logs: HashMap::new(),
        };
        bridge.register_builtins();
        bridge
    }

    /// Register a custom function record.
    pub fn register(&mut self, record: FunctionRecord) {
        self.registry.insert(record.name.clone(), record);
    }

    /// Append an audit entry for a pipeline.
    pub fn log(&mut self, pipeline: &str, entry: AuditEntry) {
        self.audit_logs.entry(pipeline.to_string()).or_default().push(entry);
    }

    /// Populate the registry with all built-in DSL functions.
    fn register_builtins(&mut self) {
        let builtins: &[(&str, &str, &str, &str, &str, &str, &str)] = &[
            // (name, module, description, params, return, cost, deps)
            ("genesis::identity",    "genesis",    "Spawn a new entity with a name",            "name:str",              "u64",          "cheap",    ""),
            ("genesis::bind",        "genesis",    "Bind a property to an entity",              "bits:u64,key:str,val",  "()",           "cheap",    ""),
            ("genesis::locate",      "genesis",    "Set entity world-space position",           "bits:u64,x,y,z:f64",    "()",           "cheap",    ""),
            ("concurrence::fork_bool","concurrence","Fork execution based on bool condition",   "condition:bool",        "bool",         "cheap",    ""),
            ("concurrence::spawn_task","concurrence","Submit a background task by name",        "task_name:str",         "()",           "cheap",    ""),
            ("proximity::nearest",   "proximity",  "Find k nearest entities globally",          "bits:u64,k:i64",        "Vec<Neighbor>","moderate", "embedvec"),
            ("proximity::nearest_class","proximity","Find k nearest within ontology class",    "bits:u64,class:str,k:i64","Vec<Neighbor>","moderate","embedvec"),
            ("proximity::compose",   "proximity",  "Text-to-vector semantic search",            "text:str,k:i64",        "Vec<Neighbor>","moderate", "embedvec"),
            ("ontology::classify",   "ontology",   "Look up ontology path for a class name",   "class:str",             "OntologyPath", "cheap",    "embedvec"),
            ("ontology::relate",     "ontology",   "Define a deferred relationship",            "from,to,pred:str",      "()",           "cheap",    ""),
            ("ontology::engineer",   "ontology",   "Create a new ontology domain branch",      "domain:str",            "()",           "cheap",    ""),
            ("knowledge::model",     "knowledge",  "Build knowledge subgraph for an entity",   "bits:u64",              "SubGraph",     "moderate", "embedvec,ontology"),
            ("knowledge::weave",     "knowledge",  "Add typed edge between two concepts",      "from,to,rel:str",       "()",           "cheap",    ""),
            ("knowledge::traverse",  "knowledge",  "BFS walk the knowledge graph",             "start:str,depth:i64",   "SubGraph",     "moderate", ""),
            ("measurement::measure", "measurement","Read a physics property from entity",      "bits:u64,prop:str",     "f64",          "cheap",    ""),
            ("measurement::entropy", "measurement","Shannon entropy of entity physics state",  "bits:u64",              "f64",          "cheap",    ""),
            ("measurement::stats",   "measurement","Aggregate stats across N entities",        "csv:str,prop:str",      "MeasurementStats","cheap", ""),
            ("refinement::cleanse",  "refinement", "Remove NaN/Inf, clamp, dedupe series",     "csv:str",               "str",          "cheap",    ""),
            ("refinement::transform","refinement", "Apply named transform to data series",     "csv:str,name:str",      "str",          "cheap",    ""),
            ("refinement::validate", "refinement", "Check value against constraint rule",      "val:f64,rule:str",      "ValidationResult","cheap",""),
            ("language::embed_query","language",   "Convert text to embedding vector CSV",     "text:str",              "str",          "moderate", "embedvec"),
            ("language::tokenize",   "language",   "Split text into classified tokens",        "text:str",              "Vec<Token>",   "cheap",    ""),
            ("language::lex",        "language",   "Parse text against named grammar",         "text,grammar:str",      "LexResult",    "cheap",    ""),
            ("temporal::tick",       "temporal",   "Get current simulation tick",              "",                      "i64",          "cheap",    ""),
            ("temporal::diff",       "temporal",   "Compute property deltas over N ticks",     "bits:u64,ago:i64",      "DiffResult",   "cheap",    ""),
            ("temporal::evolve",     "temporal",   "Project property forward t ticks",         "bits:u64,prop:str,t:f64","f64",         "cheap",    ""),
            ("temporal::snapshot",   "temporal",   "Capture current world tick summary",       "",                      "WorldSnapshot","cheap",    ""),
            ("spatial_intelligence::graph",   "spatial_intelligence","Build navigation graph for region","region:str","NavGraph",     "moderate", ""),
            ("spatial_intelligence::link",    "spatial_intelligence","Add weighted edge to spatial graph","a,b:u64,w:f64","()",        "cheap",    ""),
            ("spatial_intelligence::spatial_query","spatial_intelligence","Find entities within radius","bits:u64,r:f64","Vec<SpatialNode>","moderate",""),
            ("spatial_intelligence::resolve", "spatial_intelligence","Get full spatial context for entity","bits:u64","SpatialContext","moderate",""),
            ("statistical::correlate","statistical","Pearson correlation between two series",  "a,b:str",               "f64",          "cheap",    ""),
            ("statistical::regress", "statistical","OLS linear regression",                   "x,y:str",               "RegressionModel","moderate",""),
            ("statistical::predict", "statistical","Predict y from regression model",          "model,x:f64",           "f64",          "cheap",    ""),
            ("planning::goal",       "planning",   "Define objective and check feasibility",   "description:str",       "GoalResult",   "moderate", ""),
            ("planning::plan",       "planning",   "Generate action sequence for goal",        "goal,constraints:str",  "ActionPlan",   "expensive",""),
            ("planning::act",        "planning",   "Execute one plan step",                    "step_index:i64",        "ActionResult", "cheap",    ""),
            ("meta::introspect",     "meta",       "Query metadata for a DSL function",        "fn_name:str",           "FunctionMeta", "cheap",    ""),
            ("meta::compose_pipeline","meta",      "Chain DSL functions into a pipeline",      "name,steps:str",        "PipelineResult","cheap",   ""),
            ("meta::audit",          "meta",       "Retrieve audit log for a pipeline",        "pipeline:str",          "AuditLog",     "cheap",    ""),
        ];

        for &(name, module, description, params, ret, cost, deps) in builtins {
            self.registry.insert(name.to_string(), FunctionRecord {
                name: name.to_string(),
                module: module.to_string(),
                description: description.to_string(),
                param_types: params.to_string(),
                return_type: ret.to_string(),
                cost: cost.to_string(),
                dependencies: deps.to_string(),
            });
        }
    }
}

impl Default for MetaBridge {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static META_BRIDGE: RefCell<Option<MetaBridge>> = RefCell::new(None);
}

/// Install the meta bridge before Rune execution.
pub fn set_meta_bridge(bridge: MetaBridge) {
    META_BRIDGE.with(|cell| {
        *cell.borrow_mut() = Some(bridge);
    });
}

/// Remove and return the bridge after Rune execution.
pub fn take_meta_bridge() -> Option<MetaBridge> {
    META_BRIDGE.with(|cell| cell.borrow_mut().take())
}

fn with_bridge<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&MetaBridge) -> R,
{
    META_BRIDGE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(bridge) => f(bridge),
            None => {
                warn!("[Meta] Bridge not available — returning fallback");
                fallback
            }
        }
    })
}

fn with_bridge_mut<F, R>(fallback: R, f: F) -> R
where
    F: FnOnce(&mut MetaBridge) -> R,
{
    META_BRIDGE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(bridge) => f(bridge),
            None => {
                warn!("[Meta] Bridge not available — returning fallback");
                fallback
            }
        }
    })
}

// ============================================================================
// 3. Rune Functions
// ============================================================================

/// Query metadata about a registered DSL function.
///
/// Returns name, module, description, parameter types, return type,
/// cost category, and dependency list for any built-in or registered function.
///
/// # Arguments
/// * `fn_name` — Fully qualified function name (e.g. `"proximity::nearest"`)
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn introspect(fn_name: &str) -> FunctionMeta {
    with_bridge(FunctionMeta::unknown(fn_name), |bridge| {
        match bridge.registry.get(fn_name) {
            Some(record) => {
                info!("[Meta] introspect('{}') → found", fn_name);
                FunctionMeta {
                    name: record.name.clone(),
                    module: record.module.clone(),
                    description: record.description.clone(),
                    param_types: record.param_types.clone(),
                    return_type: record.return_type.clone(),
                    cost: record.cost.clone(),
                    dependencies: record.dependencies.clone(),
                    found: true,
                }
            }
            None => {
                warn!("[Meta] introspect('{}') → not found", fn_name);
                FunctionMeta::unknown(fn_name)
            }
        }
    })
}

/// Chain multiple DSL functions into a named reusable pipeline.
///
/// The pipeline is stored in the bridge and can be audited later.
/// Steps are not executed here — this registers the composition for
/// later execution and audit tracking by the calling system.
///
/// # Arguments
/// * `pipeline_name` — Unique name for this pipeline (e.g. `"thermal_analysis"`)
/// * `steps_csv`     — Comma-separated function names in execution order
///                     (e.g. `"measurement::measure,refinement::validate"`)
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn compose_pipeline(pipeline_name: &str, steps_csv: &str) -> PipelineResult {
    with_bridge_mut(
        PipelineResult {
            name: pipeline_name.to_string(),
            step_count: 0,
            steps_csv: String::new(),
            registered: false,
        },
        |bridge| {
            let steps: Vec<String> = steps_csv
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let step_count = steps.len() as i64;
            let stored_csv = steps.join(",");

            bridge.pipelines.insert(pipeline_name.to_string(), steps);

            // Seed audit log for this pipeline (empty, ready for entries)
            bridge.audit_logs.entry(pipeline_name.to_string()).or_default();

            info!(
                "[Meta] compose_pipeline('{}') → {} steps: [{}]",
                pipeline_name, step_count, stored_csv
            );

            PipelineResult {
                name: pipeline_name.to_string(),
                step_count,
                steps_csv: stored_csv,
                registered: true,
            }
        },
    )
}

/// Retrieve the execution audit log for a named pipeline.
///
/// Returns all `AuditEntry` records logged to this pipeline since the
/// bridge was installed. The calling system writes entries by calling
/// `MetaBridge::log()` as each step executes.
///
/// # Arguments
/// * `pipeline_name` — Pipeline name to fetch the audit log for
#[cfg(feature = "rune-dsl")]
#[rune::function]
pub fn audit(pipeline_name: &str) -> AuditLog {
    with_bridge(AuditLog::empty(pipeline_name), |bridge| {
        let Some(entries_raw) = bridge.audit_logs.get(pipeline_name) else {
            warn!("[Meta] audit('{}') → no log found", pipeline_name);
            return AuditLog::empty(pipeline_name);
        };

        let mut entries = rune::runtime::Vec::new();
        let mut error_count: i64 = 0;

        for entry in entries_raw {
            if entry.status == "error" {
                error_count += 1;
            }
            if let Ok(v) = rune::to_value(entry.clone()) {
                let _ = entries.push(v);
            }
        }

        let entry_count = entries.len() as i64;

        info!(
            "[Meta] audit('{}') → {} entries, {} errors",
            pipeline_name, entry_count, error_count
        );

        AuditLog {
            pipeline: pipeline_name.to_string(),
            entries,
            entry_count,
            error_count,
        }
    })
}

// ============================================================================
// 4. Module Registration
// ============================================================================

/// Create the `meta` Rune module.
#[cfg(feature = "rune-dsl")]
pub fn create_meta_module() -> Result<rune::Module, rune::ContextError> {
    let mut module = rune::Module::with_crate_item("eustress", ["functions", "meta"])?;

    module.ty::<FunctionMeta>()?;
    module.ty::<PipelineResult>()?;
    module.ty::<AuditEntry>()?;
    module.ty::<AuditLog>()?;

    module.function_meta(introspect)?;
    module.function_meta(compose_pipeline)?;
    module.function_meta(audit)?;

    Ok(module)
}

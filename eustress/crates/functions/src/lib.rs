//! # Eustress Functions — Spatial Intelligence DSL
//!
//! A 12-stage function ontology exposed to Rune scripts for composable
//! spatial reasoning, entity management, knowledge representation, and
//! autonomous planning.
//!
//! ## Table of Contents
//!
//! 1. **Genesis** — Entity creation, property binding, spatial placement
//! 2. **Concurrence** — Parallel execution, fork/join, task spawning
//! 3. **Proximity** — KNN search, spatial queries, vector composition
//! 4. **Ontology** — Classification, relationships, hierarchy engineering
//! 5. **Knowledge** — Knowledge base modeling, weaving, traversal
//! 6. **Measurement** — Distribution measurement, entropy, bridge stats
//! 7. **Refinement** — Data cleansing, transformation, validation
//! 8. **Language** — Tokenization, parsing, lexical analysis
//! 9. **Temporal** — Timestamps, diffs, evolution, prediction
//! 10. **Spatial** — Graph construction, linking, spatial queries, resolution
//! 11. **Statistical** — Probability, confidence, Bayesian inference
//! 12. **Planning** — Plan/simulate/decide/execute/observe loop
//! 13. **Meta** — Introspection, profiling, optimization
//!
//! ## Architecture
//!
//! Each stage is a separate Rust module behind a feature flag. Rune functions
//! access engine state through thread-local bridges installed before script
//! execution and cleared after. This follows the established pattern from
//! `rune_ecs_module.rs` (SPATIAL_BRIDGE, ECS_BINDINGS).
//!
//! ## Crate Design
//!
//! - Feature flags per stage minimize compile times and binary size
//! - Thread-local bridges avoid passing Bevy system params into Rune
//! - All Rune functions are `#[rune::function]` with `module.function_meta()`
//! - Each stage module exposes `create_*_module() -> Result<Module, ContextError>`

// ============================================================================
// Stage Modules
// ============================================================================

#[cfg(feature = "genesis")]
pub mod genesis;

#[cfg(feature = "concurrence")]
pub mod concurrence;

#[cfg(feature = "proximity")]
pub mod proximity;

#[cfg(feature = "ontology")]
pub mod ontology;

// Future stages (uncomment as implemented):
// #[cfg(feature = "knowledge")]
// pub mod knowledge;
// #[cfg(feature = "measurement")]
// pub mod measurement;
// #[cfg(feature = "refinement")]
// pub mod refinement;
// #[cfg(feature = "language")]
// pub mod language;
// #[cfg(feature = "temporal")]
// pub mod temporal;
// #[cfg(feature = "spatial")]
// pub mod spatial;
// #[cfg(feature = "statistical")]
// pub mod statistical;
// #[cfg(feature = "planning")]
// pub mod planning;
// #[cfg(feature = "meta")]
// pub mod meta;

// ============================================================================
// Unified Module Registration
// ============================================================================

/// Register all enabled Eustress Functions modules with a Rune context.
///
/// Call this alongside `create_ecs_module()` in your Rune context builder:
///
/// ```rust,ignore
/// use eustress_functions::register_all_modules;
///
/// let mut context = rune::Context::with_default_modules()?;
/// for module in register_all_modules()? {
///     context.install(module)?;
/// }
/// ```
#[cfg(feature = "rune-dsl")]
pub fn register_all_modules() -> Result<Vec<rune::Module>, rune::ContextError> {
    let mut modules = Vec::new();

    #[cfg(feature = "genesis")]
    modules.push(genesis::create_genesis_module()?);

    #[cfg(feature = "concurrence")]
    modules.push(concurrence::create_concurrence_module()?);

    #[cfg(feature = "proximity")]
    modules.push(proximity::create_proximity_module()?);

    #[cfg(feature = "ontology")]
    modules.push(ontology::create_ontology_module()?);

    Ok(modules)
}

/// Prelude for convenient imports
pub mod prelude {
    #[cfg(feature = "genesis")]
    pub use crate::genesis::*;

    #[cfg(feature = "concurrence")]
    pub use crate::concurrence::*;

    #[cfg(feature = "proximity")]
    pub use crate::proximity::*;

    #[cfg(feature = "ontology")]
    pub use crate::ontology::*;

    #[cfg(feature = "rune-dsl")]
    pub use crate::register_all_modules;
}

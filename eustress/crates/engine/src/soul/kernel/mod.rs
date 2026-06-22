//! # L12 Kernel — Rune Rewrite Validator (Agent-RSI gate)
//!
//! This module is the **Layer 12 gate** named in
//! `docs/documents/forge-eustress-integration.md` §4/§5:
//!
//! > tick loop: "REWRITE Rune (iff Kernel-valid) — YOUR L12 validator gates this"
//! > §5: "The RSI rewrite step must call your Kernel validator before committing
//! >      a Rune rewrite."
//!
//! ## Scope — read this first
//!
//! `docs/development/KERNEL_LAW_SYSTEM.md` describes TWO nested RSI loops:
//!
//! 1. **Agent-level RSI** — an agent rewrites *its own Rune program* to improve
//!    world-model prediction. THIS module gates that loop.
//! 2. **Universe-level RSI** — Claude Opus rewrites the *Kernel Laws themselves*
//!    in Rust. That is the `KernelLaw` / `KernelLawRegistry` machinery in the
//!    spec doc and is NOT what this module is. (It lives, when built, in
//!    `crates/common/src/kernel/` — see [`laws::LawSource`].)
//!
//! The KERNEL VERDICT review explicitly flagged that the spec doc conflates the
//! two and that the *agent-rewrite* accept/reject contract was undesigned. This
//! module is that contract. It validates a **candidate Rune program** against the
//! active universe's allowed-capability vocabulary and returns a structured
//! [`RewriteVerdict`]. It does NOT validate a physics `KernelLaw` TOML.
//!
//! ## The non-bypassable invariant
//!
//! The ONLY supported way to admit a rewrite is through
//! [`validator::validate_rune_rewrite`] (or the legacy-shaped
//! [`validator::validate_rune_script`] adapter that wraps it). Both return a
//! `Result`-shaped verdict; there is no "skip validation" path. The existing RSI
//! loop in `build_pipeline.rs` already gates on `validate_rune_script`, so wiring
//! the real validator there makes "every rewrite validated before commit" a
//! structural property, not a prose instruction.
//!
//! ## Latency contract (deterministic, per-tick safe)
//!
//! Per the verdict correction, the per-tick commit gate runs ONLY the cheap,
//! deterministic tiers:
//!
//! - **Tier 1 — Syntactic:** Rune parse (`parse_all::<ast::File>`). Reject on
//!   parse failure.
//! - **Tier 2 — Grammar / capability:** walk the AST, resolve every call against
//!   the [`capability::CapabilityCatalog`], reject unknown or law-withheld
//!   capabilities + scope violations + missing/mis-arity entrypoints.
//!
//! Both tiers are pure, in-process, sub-millisecond, and have NO dependency on a
//! network LLM call or RSI-learned thresholds (honors constitutional CONST-004
//! determinism). The **effect tier** (run the candidate in a forked sandbox and
//! reject on a law-violating end-state) is explicitly OUT of the per-tick gate
//! and deferred to [`validator::evaluate_effect_tier`] (a TODO seam) so it can
//! run as async post-hoc audit.

pub mod capability;
pub mod laws;
pub mod verdict;
pub mod validator;

#[cfg(test)]
mod tests;

pub use capability::{Capability, CapabilityCatalog, CapabilityClass};
pub use laws::{LawSource, UniverseLaws, ScopeRule, EntrypointContract};
pub use verdict::{
    KernelVerdict, LawViolation, RewriteVerdict, ViolationKind, ViolationSeverity,
};
pub use validator::{validate_rune_rewrite, validate_rune_script};

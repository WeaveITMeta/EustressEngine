//! # Accept/Reject Contract — the Kernel verdict types
//!
//! These types are the formal "is this rewritten Rune **program** admissible
//! against the active universe laws?" contract that the KERNEL VERDICT review
//! found undesigned. They describe a verdict over a *program*, not over a
//! physics `KernelLaw` (which is the separate universe-RSI machinery).
//!
//! ## Load-bearing string shape
//!
//! `build_pipeline.rs` keys its deterministic auto-fix loop off the *joined
//! error string* (`error_tracker.get_deterministic_fix(&error_str)`). To keep
//! that loop working, [`RewriteVerdict::into_legacy_result`] flattens a
//! rejection into the exact `Result<(), Vec<String>>` shape the loop already
//! consumes, with one human-readable message per violation. Do not change that
//! flattening without re-mapping the auto-fix patterns in `error_tracker.rs`.

/// A single law/grammar violation found in a candidate program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LawViolation {
    /// Stable id of the rule that fired (e.g. `"UNKNOWN_CAPABILITY"`,
    /// `"WITHHELD_CAPABILITY:network"`, `"NO_ENTRYPOINT"`,
    /// `"ENTRYPOINT_ARITY"`, `"PATH_TRAVERSAL"`, `"SCOPE_VIOLATION"`).
    pub law_id: String,
    /// What went wrong, suitable for surfacing to the author / RSI loop.
    pub message: String,
    /// 1-based source line, or 0 if not localizable.
    pub line: u32,
    /// 1-based source column, or 0 if not localizable.
    pub column: u32,
    /// Whether this violation is fatal (rejects) or advisory (warns only).
    pub severity: ViolationSeverity,
    /// Classification for tooling / metrics.
    pub kind: ViolationKind,
}

impl LawViolation {
    /// Format a violation as `LAW_ID @ line:col — message` for the legacy
    /// `Vec<String>` channel.
    pub fn to_message_line(&self) -> String {
        if self.line == 0 {
            format!("{} — {}", self.law_id, self.message)
        } else {
            format!("{} @ {}:{} — {}", self.law_id, self.line, self.column, self.message)
        }
    }
}

/// Severity of a single violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationSeverity {
    /// Rejects the rewrite — the program never runs.
    Fatal,
    /// Logged/surfaced but does not by itself reject (e.g. an Advisory-class
    /// capability used under a soft universe).
    Advisory,
}

/// Taxonomy of *program* rejection reasons (distinct from the `LawConflict`
/// taxonomy in `KERNEL_LAW_SYSTEM.md`, which is about law-vs-law conflicts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationKind {
    /// Program failed to parse (Tier 1 syntactic).
    Syntax,
    /// Calls a symbol that is not in the universe vocabulary at all.
    UnknownCapability,
    /// Calls a known capability whose class the active universe withholds.
    WithheldCapability,
    /// Defines no recognized entrypoint, or the wrong set for the context.
    MissingEntrypoint,
    /// Entrypoint exists but has the wrong arity/signature.
    EntrypointSignature,
    /// Filesystem write/read with a path that escapes the allowed root.
    PathTraversal,
    /// A scope rule was violated (e.g. world-law mutation in an immutable
    /// universe, or a context-forbidden capability).
    Scope,
    /// Reserved for the deferred effect tier (sandbox-detected end-state
    /// violation). Never produced by the per-tick static gate.
    EffectViolation,
}

/// The verdict over a candidate Rune **program** rewrite. This is the type the
/// non-bypassable gate [`super::validator::validate_rune_rewrite`] returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteVerdict {
    /// `true` iff no fatal violations were found.
    pub accepted: bool,
    /// All violations found (fatal + advisory). Empty on a clean accept.
    pub violations: Vec<LawViolation>,
}

impl RewriteVerdict {
    /// A clean acceptance with no violations.
    pub fn accept() -> Self {
        Self { accepted: true, violations: Vec::new() }
    }

    /// An acceptance that still carries advisory violations (warnings).
    pub fn accept_with_warnings(violations: Vec<LawViolation>) -> Self {
        debug_assert!(violations.iter().all(|v| v.severity == ViolationSeverity::Advisory));
        Self { accepted: true, violations }
    }

    /// A rejection carrying the fatal (and any advisory) violations.
    pub fn reject(violations: Vec<LawViolation>) -> Self {
        Self { accepted: false, violations }
    }

    /// Build a verdict from a violation list, deciding acceptance by whether any
    /// fatal violation is present.
    pub fn from_violations(violations: Vec<LawViolation>) -> Self {
        let accepted = !violations.iter().any(|v| v.severity == ViolationSeverity::Fatal);
        Self { accepted, violations }
    }

    /// Only the fatal violations.
    pub fn fatal(&self) -> impl Iterator<Item = &LawViolation> {
        self.violations.iter().filter(|v| v.severity == ViolationSeverity::Fatal)
    }

    /// Flatten into the legacy `Result<(), Vec<String>>` shape that
    /// `build_pipeline.rs` / `validate_rune_script` consumers expect.
    ///
    /// On accept => `Ok(())` (advisory warnings are dropped from the legacy
    /// channel; they should be surfaced through the structured verdict instead).
    /// On reject => `Err(one message per fatal violation)`.
    pub fn into_legacy_result(self) -> Result<(), Vec<String>> {
        if self.accepted {
            Ok(())
        } else {
            Err(self
                .fatal()
                .map(LawViolation::to_message_line)
                .collect())
        }
    }
}

/// Alias kept for callers/tests that think in terms of a "kernel verdict" over a
/// program. Same type as [`RewriteVerdict`]; the distinct name documents intent
/// at call sites that aren't specifically about the RSI *rewrite* path (e.g. the
/// command-bar one-shot gate).
pub type KernelVerdict = RewriteVerdict;

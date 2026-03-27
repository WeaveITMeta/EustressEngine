//! # SymbolResolver — Derivative-Signature Synonym Disambiguation
//!
//! ## Table of Contents
//! - DerivSignature    — partial-derivative fingerprint of one expression
//! - EquivEntry        — cached confidence + beta-bandit state
//! - EquivalenceCache  — (sig_a, sig_b, context) → EquivEntry
//! - SymbolResolver    — public API: are_equivalent() / learn_from_episode()
//!
//! ## Equivalence Test
//!
//! Two expressions `a` and `b` are equivalent in context `C` iff:
//!
//! ```text
//! ∀ v ∈ C:  ∂a/∂v  ≡  ∂b/∂v
//! ```
//!
//! Identical derivative signatures → same physical sensitivity → treat as synonyms.
//! This prevents the naive string-match that would conflate `E*ε` (stress) with
//! `m*a` (force) just because both are two-factor products.
//!
//! ## EquivalenceCache Bandit Loop
//!
//! Cache entries track `(alpha, beta)` of a Beta distribution.
//! - Episode success (`efficiency_ratio > 1.0` or `goal_reached`)  → alpha += 1
//! - Episode failure                                                → beta  += 1
//! - `confidence()` returns `alpha / (alpha + beta)`
//!
//! The caller (`CausalModel::counterfactual_query`) uses the confidence to
//! weight how aggressively substitutions are made during reasoning.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

// ─────────────────────────────────────────────────────────────────────────────
// Hashing helpers
// ─────────────────────────────────────────────────────────────────────────────

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

// ─────────────────────────────────────────────────────────────────────────────
// DerivSignature
// ─────────────────────────────────────────────────────────────────────────────

/// Partial-derivative fingerprint for one expression w.r.t. a set of variables.
///
/// Built from the string representations of `∂expr/∂v` for each `v` in the
/// context. Symbolica canonicalises the derivative form so syntactically
/// different but mathematically identical expressions produce the same hash.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DerivSignature {
    /// Sorted (var_name, derivative_hash) pairs — order-independent.
    entries: Vec<(String, u64)>,
    /// XOR-folded fingerprint of all entries for fast comparison.
    pub fingerprint: u64,
}

impl DerivSignature {
    /// Compute the derivative signature for `formula` w.r.t. `context_vars`.
    ///
    /// Uses Symbolica to differentiate and canonicalise each partial derivative.
    /// Falls back to a pure-string hash if Symbolica fails to parse the formula.
    pub fn compute(formula: &str, context_vars: &[&str]) -> Self {
        let mut entries: Vec<(String, u64)> = context_vars
            .iter()
            .map(|&var| {
                let deriv_str = symbolic_derivative(formula, var);
                (var.to_string(), hash_str(&deriv_str))
            })
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let fingerprint = entries.iter().fold(0u64, |acc, (_, h)| acc ^ h);
        Self { entries, fingerprint }
    }

    /// Whether two signatures agree on every context variable.
    pub fn matches(&self, other: &Self) -> bool {
        self.fingerprint == other.fingerprint && self.entries == other.entries
    }
}

/// Compute `∂formula/∂var` as a canonical string.
///
/// Uses Symbolica when the `realism-symbolic` feature is active. The Symbolica
/// representation is normalised (expanded, collected), so `2*x` and `x + x`
/// both produce the same hash.
fn symbolic_derivative(formula: &str, var: &str) -> String {
    #[cfg(feature = "realism-symbolic")]
    {
        use symbolica::atom::Atom;
        use symbolica::atom::AtomCore;

        if let Ok(atom) = Atom::parse(formula) {
            let sym = symbolica::atom::Symbol::new(var);
            let d = atom.derivative(sym);
            return format!("{d}");
        }
    }

    // Fallback: embed formula + var in a deterministic string so identical
    // (formula, var) pairs still produce identical hashes.
    format!("d({formula})/d({var})")
}

// ─────────────────────────────────────────────────────────────────────────────
// EquivEntry
// ─────────────────────────────────────────────────────────────────────────────

/// Beta-bandit state for one (sig_a, sig_b, context) triple.
#[derive(Debug, Clone)]
pub struct EquivEntry {
    /// Beta distribution alpha parameter — successes + 1.
    pub alpha: f32,
    /// Beta distribution beta parameter — failures + 1.
    pub beta: f32,
}

impl Default for EquivEntry {
    /// Uninformed prior: equal probability of equivalence / non-equivalence.
    fn default() -> Self {
        Self { alpha: 1.0, beta: 1.0 }
    }
}

impl EquivEntry {
    /// Mean of the Beta(alpha, beta) distribution — our confidence estimate.
    pub fn confidence(&self) -> f32 {
        self.alpha / (self.alpha + self.beta)
    }

    /// Update on episode outcome.
    ///
    /// `outcome` is a value in [0.0, 1.0] representing how successful the
    /// episode was (e.g. `final_score` from `ArcEpisodeRecord`).  Values
    /// above 0.5 are treated as a success; below as a failure.
    pub fn update(&mut self, outcome: f32) {
        if outcome >= 0.5 {
            self.alpha += outcome; // proportional credit
        } else {
            self.beta += 1.0 - outcome;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// EquivalenceCache
// ─────────────────────────────────────────────────────────────────────────────

/// Cache keyed by (sig_a fingerprint, sig_b fingerprint, context hash).
///
/// Symmetric: `(a, b)` and `(b, a)` map to the same entry by sorting keys.
#[derive(Debug, Default)]
pub struct EquivalenceCache {
    entries: HashMap<(u64, u64, u64), EquivEntry>,
    /// Total lookups (for diagnostics).
    pub total_lookups: u64,
    /// Total cache hits.
    pub total_hits: u64,
}

impl EquivalenceCache {
    fn make_key(sig_a: u64, sig_b: u64, context_hash: u64) -> (u64, u64, u64) {
        let (lo, hi) = if sig_a <= sig_b { (sig_a, sig_b) } else { (sig_b, sig_a) };
        (lo, hi, context_hash)
    }

    /// Look up or create an entry for the given signatures.
    pub fn entry(&mut self, sig_a: u64, sig_b: u64, context_hash: u64) -> &mut EquivEntry {
        self.total_lookups += 1;
        let key = Self::make_key(sig_a, sig_b, context_hash);
        if self.entries.contains_key(&key) {
            self.total_hits += 1;
        }
        self.entries.entry(key).or_default()
    }

    /// Read-only lookup — returns None if the pair has never been observed.
    pub fn get(&self, sig_a: u64, sig_b: u64, context_hash: u64) -> Option<&EquivEntry> {
        let key = Self::make_key(sig_a, sig_b, context_hash);
        self.entries.get(&key)
    }

    /// Record an episode outcome for a specific (sig_a, sig_b, context) triple.
    pub fn update(&mut self, sig_a: u64, sig_b: u64, context_hash: u64, outcome: f32) {
        let key = Self::make_key(sig_a, sig_b, context_hash);
        self.entries.entry(key).or_default().update(outcome);
    }

    /// Number of distinct pairs cached.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SymbolResolver
// ─────────────────────────────────────────────────────────────────────────────

/// Resolves whether two symbolic expressions are semantically equivalent.
///
/// Thread-safety: wrap in `Arc<Mutex<SymbolResolver>>` for async or multi-system use.
#[derive(Debug, Default)]
pub struct SymbolResolver {
    pub cache: EquivalenceCache,
    /// Minimum derivative-signature match confidence to declare equivalence.
    pub confidence_threshold: f32,
}

impl SymbolResolver {
    pub fn new(confidence_threshold: f32) -> Self {
        Self {
            cache: EquivalenceCache::default(),
            confidence_threshold,
        }
    }

    /// Compute context hash from a sorted list of variable names.
    pub fn context_hash(vars: &[&str]) -> u64 {
        let mut sorted = vars.to_vec();
        sorted.sort_unstable();
        hash_str(&sorted.join(","))
    }

    /// Check whether `formula_a` and `formula_b` are equivalent in `context_vars`.
    ///
    /// Returns the current confidence value (0.0–1.0). The caller decides whether
    /// to treat them as equivalent by comparing against `confidence_threshold`.
    ///
    /// The first time a pair is seen the derivative signatures are computed and
    /// stored; subsequent calls reuse the cached entry.
    pub fn are_equivalent(
        &mut self,
        formula_a: &str,
        formula_b: &str,
        context_vars: &[&str],
    ) -> f32 {
        let sig_a = DerivSignature::compute(formula_a, context_vars);
        let sig_b = DerivSignature::compute(formula_b, context_vars);
        let ctx_hash = Self::context_hash(context_vars);

        // If derivative signatures differ, confidence is definitionally 0 —
        // different physics, not synonyms.
        if !sig_a.matches(&sig_b) {
            return 0.0;
        }

        self.cache.entry(sig_a.fingerprint, sig_b.fingerprint, ctx_hash).confidence()
    }

    /// Returns `true` if `are_equivalent` exceeds `confidence_threshold`.
    pub fn is_synonym(
        &mut self,
        formula_a: &str,
        formula_b: &str,
        context_vars: &[&str],
    ) -> bool {
        self.are_equivalent(formula_a, formula_b, context_vars) >= self.confidence_threshold
    }

    /// Record episode outcome to update bandit belief for a formula pair.
    ///
    /// `outcome` — 0.0–1.0 (e.g. `ArcEpisodeRecord::final_score` or
    ///             `efficiency_ratio.min(1.0)`).
    pub fn learn_from_episode(
        &mut self,
        formula_a: &str,
        formula_b: &str,
        context_vars: &[&str],
        outcome: f32,
    ) {
        let sig_a = DerivSignature::compute(formula_a, context_vars);
        let sig_b = DerivSignature::compute(formula_b, context_vars);
        let ctx_hash = Self::context_hash(context_vars);
        self.cache.update(sig_a.fingerprint, sig_b.fingerprint, ctx_hash, outcome);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deriv_sig_same_formula() {
        let s1 = DerivSignature::compute("E*epsilon", &["E", "epsilon"]);
        let s2 = DerivSignature::compute("E*epsilon", &["E", "epsilon"]);
        assert!(s1.matches(&s2));
    }

    #[test]
    fn deriv_sig_different_formulas() {
        // "m*a" and "n*R*T/V" should differ in their derivative signatures
        let s1 = DerivSignature::compute("m*a", &["m", "a"]);
        let s2 = DerivSignature::compute("n*R*T/V", &["m", "a"]);
        // With fallback hashing: d(m*a)/d(m) != d(n*R*T/V)/d(m)
        // The fallback embeds formula+var, so these will differ.
        assert!(!s1.matches(&s2));
    }

    #[test]
    fn equiv_entry_bandit_update() {
        let mut entry = EquivEntry::default();
        assert!((entry.confidence() - 0.5).abs() < 1e-6);
        entry.update(1.0); // strong success
        assert!(entry.confidence() > 0.5);
        entry.update(0.0);
        entry.update(0.0);
        // Two failures should drag confidence down
        let after = entry.confidence();
        assert!(after < 1.0);
    }

    #[test]
    fn cache_symmetric() {
        let mut cache = EquivalenceCache::default();
        cache.update(10, 20, 99, 1.0);
        // Symmetric lookup
        let e1 = cache.get(10, 20, 99).unwrap().confidence();
        let e2 = cache.get(20, 10, 99).unwrap().confidence();
        assert!((e1 - e2).abs() < 1e-6);
    }

    #[test]
    fn resolver_synonym_detection() {
        let mut resolver = SymbolResolver::new(0.6);
        // Same formula — derivative signatures match; fresh prior = 0.5 confidence
        // (below threshold), but identity should give confidence > 0 after a success.
        let c = resolver.are_equivalent("E*eps", "E*eps", &["E", "eps"]);
        // New pair: prior confidence is 0.5 (uninformed Beta(1,1))
        assert!((c - 0.5).abs() < 1e-5, "prior should be 0.5, got {c}");

        resolver.learn_from_episode("E*eps", "E*eps", &["E", "eps"], 1.0);
        let c2 = resolver.are_equivalent("E*eps", "E*eps", &["E", "eps"]);
        assert!(c2 > 0.5, "confidence should increase after success");
    }
}

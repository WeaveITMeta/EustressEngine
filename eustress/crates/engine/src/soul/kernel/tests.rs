//! # Kernel validator test suite — the CI gate for every RSI Rune rewrite
//!
//! Per the KERNEL VERDICT correction, the L12 gate's whole purpose is to keep
//! self-modifying agents safe, so the disqualifying gap in the original design
//! was the *absence of an adversarial-rewrite test suite*. This module is that
//! suite. It is table-driven: each case is `(name, rune source, expected)`.
//!
//! ACCEPT fixtures: minimal programs using only sanctioned capabilities with a
//! valid entrypoint.
//!
//! REJECT fixtures (one per law): unknown-function call, withheld-capability
//! call under a restricted universe, path-traversal write, world-law mutation in
//! an immutable-physics universe, missing entrypoint, wrong `on_update` arity.
//!
//! Each reject case also asserts the *specific* `law_id`/`kind` fired and that
//! a line:col is populated, so a future regression that "rejects for the wrong
//! reason" is caught.

use super::capability::CapabilityClass;
use super::laws::{ScopeRule, UniverseLaws};
use super::validator::{validate_rune_rewrite, validate_rune_script};
use super::verdict::ViolationKind;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A universe that withholds Network + Persistence and makes physics immutable —
/// the kind of profile "Matrix" would declare.
fn matrix_laws() -> UniverseLaws {
    UniverseLaws::restricted(
        "Matrix",
        &[CapabilityClass::Network, CapabilityClass::Persistence],
        vec![ScopeRule::ImmutablePhysics],
    )
}

/// Assert the verdict has a fatal violation of the given kind, with a populated
/// (non-zero) line for kinds that localize.
fn assert_rejected_for(verdict: &super::verdict::RewriteVerdict, kind: ViolationKind) {
    assert!(!verdict.accepted, "expected rejection, got accept");
    let hit = verdict
        .fatal()
        .find(|v| v.kind == kind)
        .unwrap_or_else(|| panic!("no fatal violation of kind {:?}; got {:?}", kind, verdict.violations));
    // MissingEntrypoint is the one kind that legitimately has no line (it's a
    // whole-program property). Everything else must localize.
    if kind != ViolationKind::MissingEntrypoint {
        assert!(hit.line > 0, "violation {:?} should populate a line:col", hit);
    }
}

// ---------------------------------------------------------------------------
// ACCEPT fixtures
// ---------------------------------------------------------------------------

#[test]
fn accept_minimal_lifecycle_program() {
    let laws = UniverseLaws::eustress_core_default();
    let src = r#"
        pub fn on_init() {
            log_info("hello");
        }
        pub fn on_update(dt) {
            let v = get_sim_value("voltage");
            set_sim_value("voltage", v);
        }
    "#;
    let verdict = validate_rune_rewrite(src, &laws);
    assert!(verdict.accepted, "expected accept, got {:?}", verdict.violations);
}

#[test]
fn accept_oneshot_main_under_oneshot_contract() {
    let mut laws = UniverseLaws::eustress_core_default();
    laws.entrypoints = super::laws::EntrypointContract::oneshot();
    let src = r#"
        pub fn main() {
            log_info("one shot");
        }
    "#;
    let verdict = validate_rune_rewrite(src, &laws);
    assert!(verdict.accepted, "expected accept, got {:?}", verdict.violations);
}

#[test]
fn accept_program_calling_its_own_helper() {
    // A call to a locally-defined helper must NOT be flagged as an unknown
    // capability.
    let laws = UniverseLaws::eustress_core_default();
    let src = r#"
        fn helper(x) { x + 1 }
        pub fn on_init() {
            let _ = helper(41);
        }
    "#;
    let verdict = validate_rune_rewrite(src, &laws);
    assert!(verdict.accepted, "expected accept, got {:?}", verdict.violations);
}

// ---------------------------------------------------------------------------
// REJECT fixtures — one per law
// ---------------------------------------------------------------------------

#[test]
fn reject_unknown_capability() {
    let laws = UniverseLaws::eustress_core_default();
    let src = r#"
        pub fn on_init() {
            summon_demon("baal");
        }
    "#;
    let verdict = validate_rune_rewrite(src, &laws);
    assert_rejected_for(&verdict, ViolationKind::UnknownCapability);
}

#[test]
fn reject_withheld_capability_network() {
    // http_get_async is a Network capability — Matrix withholds it.
    let laws = matrix_laws();
    let src = r#"
        pub fn on_init() {
            let _ = http_get_async("https://example.com");
        }
    "#;
    let verdict = validate_rune_rewrite(src, &laws);
    assert_rejected_for(&verdict, ViolationKind::WithheldCapability);
}

#[test]
fn reject_withheld_capability_persistence() {
    let laws = matrix_laws();
    let src = r#"
        pub fn on_init() {
            datastore_set("k", "v");
        }
    "#;
    let verdict = validate_rune_rewrite(src, &laws);
    assert_rejected_for(&verdict, ViolationKind::WithheldCapability);
}

#[test]
fn reject_world_law_mutation_in_immutable_universe() {
    // workspace_set_gravity is a WorldLaw capability; Matrix forbids it via the
    // ImmutablePhysics scope rule.
    let laws = matrix_laws();
    let src = r#"
        pub fn on_init() {
            workspace_set_gravity(0.0);
        }
    "#;
    let verdict = validate_rune_rewrite(src, &laws);
    assert_rejected_for(&verdict, ViolationKind::Scope);
}

#[test]
fn reject_path_traversal_write() {
    let laws = UniverseLaws::eustress_core_default(); // path-traversal guard on
    let src = r#"
        pub fn on_init() {
            write_space_file("../../etc/passwd", "x");
        }
    "#;
    let verdict = validate_rune_rewrite(src, &laws);
    assert_rejected_for(&verdict, ViolationKind::PathTraversal);
}

#[test]
fn reject_missing_entrypoint() {
    let laws = UniverseLaws::eustress_core_default();
    let src = r#"
        fn helper() { log_info("no entrypoint here"); }
    "#;
    let verdict = validate_rune_rewrite(src, &laws);
    assert_rejected_for(&verdict, ViolationKind::MissingEntrypoint);
}

#[test]
fn reject_wrong_on_update_arity() {
    // on_update must take exactly one arg (dt). Zero args => arity violation.
    let laws = UniverseLaws::eustress_core_default();
    let src = r#"
        pub fn on_update() {
            log_info("missing dt");
        }
    "#;
    let verdict = validate_rune_rewrite(src, &laws);
    assert_rejected_for(&verdict, ViolationKind::EntrypointSignature);
}

#[test]
fn reject_syntax_error() {
    let laws = UniverseLaws::eustress_core_default();
    let src = r#"
        pub fn on_init( {
            // missing close paren
        }
    "#;
    let verdict = validate_rune_rewrite(src, &laws);
    assert_rejected_for(&verdict, ViolationKind::Syntax);
}

// ---------------------------------------------------------------------------
// Non-bypassability + legacy-shape contract
// ---------------------------------------------------------------------------

#[test]
fn legacy_adapter_returns_ok_on_accept() {
    // The default universe is permissive, so a sanctioned program passes the
    // legacy `Result<(), Vec<String>>` adapter that build_pipeline.rs consumes.
    let src = r#"
        pub fn on_init() { log_info("ok"); }
    "#;
    assert!(validate_rune_script(src).is_ok());
}

#[test]
fn legacy_adapter_returns_err_messages_on_reject() {
    // A syntactically valid but law-violating program must produce one Err
    // message per fatal violation, in the shape the RSI auto-fix loop reads.
    let src = r#"
        pub fn on_init() { summon_demon("x"); }
    "#;
    let result = validate_rune_script(src);
    let errs = result.expect_err("expected rejection");
    assert!(!errs.is_empty(), "rejection must carry at least one message");
    assert!(
        errs.iter().any(|m| m.contains("UNKNOWN_CAPABILITY")),
        "messages should name the law id; got {:?}",
        errs
    );
}

#[test]
fn determinism_same_input_same_verdict() {
    // Honors CONST-004: identical (source, laws) => identical verdict.
    let laws = UniverseLaws::eustress_core_default();
    let src = r#"
        pub fn on_init() { http_get_async("x"); set_sim_value("a", 1.0); }
        pub fn on_update(dt) { log_info("tick"); }
    "#;
    let a = validate_rune_rewrite(src, &laws);
    let b = validate_rune_rewrite(src, &laws);
    assert_eq!(a, b, "verdict must be deterministic for identical inputs");
}

// ---------------------------------------------------------------------------
// Catalog / law-source sanity
// ---------------------------------------------------------------------------

#[test]
fn catalog_is_nonempty_and_covers_core_classes() {
    let cat = super::capability::CapabilityCatalog::eustress_core();
    assert!(cat.len() > 50, "core catalog should enumerate the full surface");
    let classes = cat.classes();
    assert!(classes.contains(&CapabilityClass::Network));
    assert!(classes.contains(&CapabilityClass::Persistence));
    assert!(classes.contains(&CapabilityClass::WorldLaw));
    assert!(classes.contains(&CapabilityClass::WriteSim));
}

#[test]
fn default_universe_grants_full_vocabulary() {
    let laws = UniverseLaws::eustress_core_default();
    for cap in laws.catalog.iter() {
        assert!(
            laws.grants(cap.class),
            "default universe should grant every catalogued class; missing {:?}",
            cap.class
        );
    }
}

// ---------------------------------------------------------------------------
// TODO: effect-tier seam (deferred) — placeholder so the seam stays visible.
// ---------------------------------------------------------------------------

#[test]
fn effect_tier_is_deferred_returns_none() {
    let laws = UniverseLaws::eustress_core_default();
    let src = r#"pub fn on_init() { log_info("x"); }"#;
    assert!(
        super::validator::evaluate_effect_tier(src, &laws).is_none(),
        "effect tier must be deferred (None) until the sandbox-backed audit is built"
    );
}

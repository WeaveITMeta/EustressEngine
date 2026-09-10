//! End-to-end tests for the CAD authoring rules and the shared-part
//! library, driven through the real tool handlers.
//!
//! These run against a temporary Space rather than through the MCP
//! server on purpose. `shared_registry::build_context` deliberately
//! follows the *running engine's* current Space so that a disk-writing
//! tool lands where the user is actually looking, which means no
//! environment variable can sandbox it while the Studio is open. A test
//! that went through that path would write into whichever Universe
//! happened to be open, which is how a probe run ends up creating parts
//! inside somebody's live assembly.
//!
//! Every assertion here is on a number or a file that can come out
//! wrong: exact volumes, byte-for-byte file equality, and per-feature
//! status. `evaluates` is deliberately never trusted on its own, because
//! `evaluate_tree` reports Ok for the tree as a whole while an
//! individual feature inside it failed and contributed nothing.

use eustress_tools::{ToolContext, ToolHandler, ToolResult};
use serde_json::{json, Value};

// ── Harness ─────────────────────────────────────────────────────────

struct Scratch {
    root: std::path::PathBuf,
    ctx: ToolContext,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("eustress-cad-{tag}-{nanos}"));
        let space = root.join("Spaces").join("TestSpace");
        std::fs::create_dir_all(space.join("Workspace")).unwrap();
        let ctx = ToolContext {
            space_root: space,
            universe_root: root.clone(),
            user_id: None,
            username: None,
            luau_executor: None,
            display_unit: None,
            cancelled: None,
            permissions: Default::default(),
        };
        Self { root, ctx }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Structured payload of a tool result, or a panic naming the summary.
fn data(r: &ToolResult) -> &Value {
    r.structured_data
        .as_ref()
        .unwrap_or_else(|| panic!("no structured data; summary was: {}", r.content))
}

fn call(tool: &dyn ToolHandler, args: Value, ctx: &ToolContext) -> ToolResult {
    tool.execute(args, ctx)
}

fn ok_call(tool: &dyn ToolHandler, args: Value, ctx: &ToolContext) -> ToolResult {
    let r = call(tool, args, ctx);
    assert!(r.success, "expected success, got: {}", r.content);
    r
}

/// Volume of the current body, from the tool surface rather than
/// recomputed here, so the test measures what a caller would see.
fn volume(ctx: &ToolContext, path: &str) -> f64 {
    let r = ok_call(
        &eustress_tools::cad_tools::CadDescribePartTool,
        json!({ "path": path, "include_mesh_stats": true }),
        ctx,
    );
    data(&r)["mesh"]["volume_m3"]
        .as_f64()
        .unwrap_or_else(|| panic!("no volume in {}", data(&r)))
}

/// Names of features that failed, which is the signal `evaluates` is not.
fn broken(r: &ToolResult) -> Vec<String> {
    data(r)["feature_status"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|s| s["ok"].as_bool() != Some(true))
                .map(|s| s["name"].as_str().unwrap_or("?").to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn plate(ctx: &ToolContext, name: &str) -> String {
    ok_call(
        &eustress_tools::cad_tools::CadCreatePartTool,
        json!({ "name": name, "template": "plate" }),
        ctx,
    );
    format!("Workspace/{name}")
}

// ── Offset ──────────────────────────────────────────────────────────

#[test]
fn offset_inward_then_subtract_hollows_the_part() {
    let s = Scratch::new("shell");
    let p = plate(&s.ctx, "Plate");

    // 100 x 60 x 10 mm.
    assert!(
        (volume(&s.ctx, &p) - 6.0e-5).abs() < 1.0e-9,
        "solid plate volume was {}",
        volume(&s.ctx, &p)
    );

    let r = ok_call(
        &eustress_tools::cad_tools::CadOffsetSketchTool,
        json!({ "path": p, "sketch": "Sketch1", "distance": "-5 mm", "name": "Inner" }),
        &s.ctx,
    );
    assert_eq!(data(&r)["created"]["entities"], 4);
    assert!(
        (data(&r)["created"]["distance_m"].as_f64().unwrap() + 0.005).abs() < 1.0e-12,
        "distance resolved to {}",
        data(&r)["created"]["distance_m"]
    );

    let r = ok_call(
        &eustress_tools::cad_tools::CadAddFeatureTool,
        json!({
            "path": p, "op": "extrude", "name": "Pocket", "sketch": "Inner",
            "depth": "10 mm", "end_condition": "through_all", "combine": "subtract"
        }),
        &s.ctx,
    );
    assert!(broken(&r).is_empty(), "features failed: {:?}", broken(&r));

    // A 5 mm wall around a 100x60 outline, 10 mm tall:
    // (0.100*0.060 - 0.090*0.050) * 0.010 = 1.5e-5
    let v = volume(&s.ctx, &p);
    assert!(
        (v - 1.5e-5).abs() < 1.0e-9,
        "hollow frame volume was {v}, expected 1.5e-5"
    );
}

#[test]
fn offset_refuses_what_it_cannot_represent() {
    let s = Scratch::new("refuse");
    let p = plate(&s.ctx, "Plate");
    let t = eustress_tools::cad_tools::CadOffsetSketchTool;

    // Past collapse: the narrowest span is 60 mm, so 40 mm inward eats it.
    let r = call(
        &t,
        json!({ "path": p, "sketch": "Sketch1", "distance": "-40 mm", "name": "Gone" }),
        &s.ctx,
    );
    assert!(!r.success, "collapsing offset was accepted: {}", r.content);

    // A bare number is the silent 1000x error the unit system exists for.
    let r = call(
        &t,
        json!({ "path": p, "sketch": "Sketch1", "distance": "-5", "name": "NoUnit" }),
        &s.ctx,
    );
    assert!(!r.success, "unitless distance was accepted: {}", r.content);

    let r = call(
        &t,
        json!({ "path": p, "sketch": "Nope", "distance": "-1 mm" }),
        &s.ctx,
    );
    assert!(!r.success, "unknown sketch was accepted: {}", r.content);
}

#[test]
fn offset_distance_is_a_rule_not_a_number() {
    let s = Scratch::new("rule");
    let p = plate(&s.ctx, "Plate");
    ok_call(
        &eustress_tools::cad_tools::CadSetVariableTool,
        json!({ "path": p, "name": "wall", "value": "5 mm" }),
        &s.ctx,
    );
    let t = eustress_tools::cad_tools::CadOffsetSketchTool;

    let r = ok_call(
        &t,
        json!({ "path": p, "sketch": "Sketch1", "distance": "-wall", "name": "A" }),
        &s.ctx,
    );
    assert!((data(&r)["created"]["distance_m"].as_f64().unwrap() + 0.005).abs() < 1.0e-12);

    let r = ok_call(
        &t,
        json!({ "path": p, "sketch": "Sketch1", "distance": "-(wall + 2 mm)", "name": "B" }),
        &s.ctx,
    );
    assert!(
        (data(&r)["created"]["distance_m"].as_f64().unwrap() + 0.007).abs() < 1.0e-12,
        "expression resolved to {}",
        data(&r)["created"]["distance_m"]
    );
}

// ── Count as a rule ─────────────────────────────────────────────────

/// Add a linear pattern of the plate and return (result, volume).
fn patterned(ctx: &ToolContext, name: &str, count: Value, vars: &[(&str, &str)]) -> (ToolResult, f64) {
    let p = plate(ctx, name);
    for (k, v) in vars {
        ok_call(
            &eustress_tools::cad_tools::CadSetVariableTool,
            json!({ "path": p, "name": k, "value": v }),
            ctx,
        );
    }
    let r = call(
        &eustress_tools::cad_tools::CadAddFeatureTool,
        json!({
            "path": p, "op": "pattern", "name": "Array", "pattern_kind": "linear",
            "features": ["Extrude1"], "count": count,
            "spacing": "120 mm", "direction": [1.0, 0.0, 0.0], "combine": "add"
        }),
        ctx,
    );
    let v = volume(ctx, &p);
    (r, v)
}

#[test]
fn count_accepts_a_literal_and_an_expression_identically() {
    let s = Scratch::new("count");

    let (r_lit, v_lit) = patterned(&s.ctx, "Lit", json!(4), &[]);
    assert!(broken(&r_lit).is_empty(), "literal count failed: {:?}", broken(&r_lit));

    let (r_exp, v_exp) = patterned(
        &s.ctx,
        "Expr",
        json!("rows * cols"),
        &[("rows", "2"), ("cols", "2")],
    );
    assert!(
        broken(&r_exp).is_empty(),
        "expression count failed: {:?}, summary: {}",
        broken(&r_exp),
        r_exp.content
    );

    let (_, v_one) = patterned(&s.ctx, "One", json!(1), &[]);
    // The metric has to be able to fail: if count were ignored, all three
    // would agree and this test would prove nothing.
    assert!(
        (v_lit - v_one).abs() > 1.0e-9,
        "4 instances measured the same as 1 ({v_lit} vs {v_one}); count is not driving anything"
    );
    assert!(
        (v_lit - v_exp).abs() < 1.0e-12,
        "expression count built a different body: {v_lit} vs {v_exp}"
    );
}

#[test]
fn a_count_carrying_a_unit_is_refused() {
    let s = Scratch::new("countunit");
    // A count is dimensionless. `evaluate_tree` returns Ok for the tree
    // even when one feature inside it failed, so the per-feature status
    // is the only signal that can catch this.
    let (r, _) = patterned(&s.ctx, "Bad", json!("3 mm"), &[]);
    assert!(
        !r.success || !broken(&r).is_empty(),
        "a count of '3 mm' was accepted: {}",
        r.content
    );
}

// ── The shared library and the sourced edge ─────────────────────────

#[test]
fn publish_place_and_the_source_edge() {
    let s = Scratch::new("library");
    let p = plate(&s.ctx, "Bracket");

    let r = ok_call(
        &eustress_tools::cad_tools::CadPublishPartTool,
        json!({ "path": p, "id": "bracket_m6" }),
        &s.ctx,
    );
    assert!(data(&r)["triangles"].as_u64().unwrap_or(0) > 0);
    let lib_path = data(&r)["library_path"].as_str().unwrap().to_string();

    // Publishing over an existing definition restates every placement of
    // it, so it needs to be asked for.
    let r = call(
        &eustress_tools::cad_tools::CadPublishPartTool,
        json!({ "path": p, "id": "bracket_m6" }),
        &s.ctx,
    );
    assert!(!r.success, "silently overwrote a definition: {}", r.content);

    for bad in ["../escape", "nested/id", "", ".."] {
        let r = call(
            &eustress_tools::cad_tools::CadPublishPartTool,
            json!({ "path": p, "id": bad }),
            &s.ctx,
        );
        assert!(!r.success, "library id '{bad}' was accepted");
    }

    let r = ok_call(
        &eustress_tools::cad_tools::CadListSourcesTool,
        json!({ "evaluate": true }),
        &s.ctx,
    );
    let defs = data(&r)["definitions"].as_array().unwrap();
    let row = defs
        .iter()
        .find(|d| d["id"] == "bracket_m6")
        .expect("published definition missing from the listing");
    assert_eq!(row["evaluates"], true);
    assert!(row["broken_features"].as_array().unwrap().is_empty());

    // A placement takes its body from the library, carries the edge as an
    // attribute, and is not a template copy.
    let r = ok_call(
        &eustress_tools::cad_tools::CadCreatePartTool,
        json!({ "name": "Placement", "source": "bracket_m6" }),
        &s.ctx,
    );
    assert_eq!(data(&r)["sourced_from"], "bracket_m6");
    let dir = std::path::PathBuf::from(data(&r)["path"].as_str().unwrap());
    let inst = std::fs::read_to_string(dir.join("_instance.toml")).unwrap();
    assert!(
        inst.contains("cad_source = \"bracket_m6\""),
        "placement did not record its source:\n{inst}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("features.toml")).unwrap(),
        std::fs::read_to_string(&lib_path).unwrap(),
        "placement body differs from the published definition"
    );

    // A stale template argument must neither defeat the source nor fail
    // the call: the source decides the geometry, so the template is not
    // consulted at all.
    let r = ok_call(
        &eustress_tools::cad_tools::CadCreatePartTool,
        json!({ "name": "Stale", "source": "bracket_m6", "template": "not_a_template" }),
        &s.ctx,
    );
    assert_eq!(data(&r)["sourced_from"], "bracket_m6");

    // An unpublished id is a refusal, not a silent fallback to a plate.
    let r = call(
        &eustress_tools::cad_tools::CadCreatePartTool,
        json!({ "name": "Ghost", "source": "no_such_part" }),
        &s.ctx,
    );
    assert!(!r.success, "placed a part that was never published: {}", r.content);
}

#[test]
fn a_definition_that_does_not_evaluate_is_not_published() {
    let s = Scratch::new("badpublish");
    let p = plate(&s.ctx, "Broken");
    // Extrude a sketch that does not exist: the feature is written, the
    // tree still reports Ok overall, and the body is empty. Publishing
    // that would hand the same fault to every future placement while
    // reporting it at each one as if it were local.
    call(
        &eustress_tools::cad_tools::CadAddFeatureTool,
        json!({
            "path": p, "op": "extrude", "name": "Nowhere",
            "sketch": "DoesNotExist", "depth": "5 mm", "combine": "add"
        }),
        &s.ctx,
    );
    let r = call(
        &eustress_tools::cad_tools::CadPublishPartTool,
        json!({ "path": p, "id": "broken_part" }),
        &s.ctx,
    );
    assert!(!r.success, "published a definition with a failing feature: {}", r.content);
    assert!(
        !s.root
            .join(".eustress/assets/cad/broken_part/features.toml")
            .exists(),
        "a refused publish still wrote to the library"
    );
}

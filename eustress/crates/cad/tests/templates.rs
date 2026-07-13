//! Template contract tests: every built-in template must evaluate to
//! a solid with every feature reporting ok, and every declared
//! variable must actually drive geometry — a Properties field that
//! silently does nothing is a lie to the user (and exactly the bug
//! class that shipped in the first draft of these templates).

use eustress_cad::{evaluate_tree, parse_tree, templates, EvalMesh};

/// (template, variable) pairs where perturbing the variable
/// legitimately leaves the mesh unchanged. Keep this list SHORT and
/// justified.
const EXCLUDED: &[(&str, &str)] = &[
    // Scaling an already-through hole deeper is still through — same
    // geometry by design (blind holes are covered by hole_dia).
    ("plate_hole", "hole_depth"),
];

fn mesh_signature(mesh: &EvalMesh) -> Vec<u32> {
    // Bit-exact position stream — enough to detect any geometric change.
    mesh.positions
        .iter()
        .flat_map(|p| p.iter().map(|c| c.to_bits()))
        .collect()
}

fn scale_length(value: &str, k: f64) -> String {
    let n: f64 = value
        .strip_suffix(" m")
        .unwrap_or_else(|| panic!("template variables use \"<n> m\" form, got '{value}'"))
        .parse()
        .expect("numeric quantity");
    format!("{} m", n * k)
}

#[test]
fn every_template_evaluates_clean() {
    for (name, toml) in templates::all() {
        let tree = parse_tree(toml).unwrap_or_else(|e| panic!("{name}: parse: {e}"));
        let out = evaluate_tree(&tree).unwrap_or_else(|e| panic!("{name}: eval: {e}"));
        for s in &out.entry_status {
            assert!(s.ok, "{name}: entry '{}' failed: {}", s.name, s.message);
        }
        assert!(out.body.is_some(), "{name}: no body");
        let mesh = out.mesh.as_ref().expect("mesh");
        assert!(!mesh.indices.is_empty(), "{name}: empty mesh");
        assert_eq!(mesh.positions.len(), mesh.normals.len(), "{name}: normals");
    }
}

#[test]
fn every_variable_drives_geometry() {
    for (name, toml) in templates::all() {
        let tree = parse_tree(toml).unwrap_or_else(|e| panic!("{name}: parse: {e}"));
        let base = evaluate_tree(&tree).unwrap_or_else(|e| panic!("{name}: eval: {e}"));
        let base_sig = mesh_signature(base.mesh.as_ref().expect("base mesh"));

        for (var, value) in &tree.variables {
            if EXCLUDED.contains(&(name, var.as_str())) {
                continue;
            }
            let mut tree2 = tree.clone();
            tree2
                .variables
                .insert(var.clone(), scale_length(value, 1.5));
            let out2 = evaluate_tree(&tree2)
                .unwrap_or_else(|e| panic!("{name}: eval with {var} x1.5: {e}"));
            for s in &out2.entry_status {
                assert!(
                    s.ok,
                    "{name} with {var} x1.5: entry '{}' failed: {}",
                    s.name, s.message
                );
            }
            let mesh2 = out2
                .mesh
                .as_ref()
                .unwrap_or_else(|| panic!("{name}: no mesh with {var} x1.5"));
            assert_ne!(
                mesh_signature(mesh2),
                base_sig,
                "{name}: variable '{var}' does not drive geometry — dead Properties field"
            );
        }
    }
}

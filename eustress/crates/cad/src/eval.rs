//! Deterministic feature-tree evaluator — walks a [`FeatureTree`] in
//! declaration order, applies each sketch + feature, produces a final
//! `truck` `Solid` + a tessellated mesh.
//!
//! ## Shipped evaluators (2026-04-22)
//!
//! | Feature   | Status    | Notes                                           |
//! |-----------|-----------|-------------------------------------------------|
//! | Extrude   | ✅ working | Rectangle / Circle / closed-polyline profiles   |
//! | Revolve   | ✅ working | `builder::rsweep` around arbitrary axis+angle   |
//! | Mirror    | ✅ working | Plane reflection via transform                  |
//! | Pattern   | ✅ working | Linear + Circular (Path/Sketch pending)         |
//! | Split     | ✅ working | Plane cut → boolean with slab                   |
//! | Hole      | ✅ working | Decomposes to circular extrude-cut              |
//! | Boolean   | ✅ working | truck-shapeops `and_solid` / `or_solid` / `not_solid` |
//! | Fillet    | 🟡 mesh    | Mesh crease soften interim (BRep fillet later)  |
//! | Chamfer   | 🟡 mesh    | Same mesh soften path as Fillet                 |
//! | Shell     | ✅ approx  | Open-top inner cut (enclosed cavities need offset surfaces) |
//! | Sweep     | ✅ working | Profile along path polyline (segment chain)     |
//! | Loft      | 🚧 pending | Profile interpolation + guide curves            |
//! | Solver    | ✅ working | Gauss-Newton on sketch constraints/dimensions   |
//!
//! ## Combine-mode semantics
//!
//! Every body-producing feature carries a `FeatureOp` telling the
//! evaluator how it combines with the running body:
//! - `NewBody` — discard running body, replace with this feature's.
//! - `Add` — union (running OR feature).
//! - `Subtract` — difference (running minus feature).
//! - `Intersect` — intersection (running AND feature).
//!
//! Booleans route through [`boolean_combine`] which calls
//! `truck-shapeops`. On failure (tolerance issues, non-manifold),
//! returns the new feature body alone so the user's work isn't
//! silently discarded.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use truck_base::cgmath64::*;
use truck_modeling::*;

use crate::{FeatureTree, FeatureEntry, Feature, Sketch, SketchEntity, CadError, CadResult};

/// Output of a successful tree evaluation.
pub struct EvalOutput {
    pub body: Option<Solid>,
    pub mesh: Option<EvalMesh>,
    pub entry_status: Vec<EntryStatus>,
}

/// Flat triangle arrays — the engine lifts these into a Bevy `Mesh`
/// + Avian trimesh collider; the glTF exporter writes them as a
/// primitive. Per-corner attributes are deduplicated: smooth-surface
/// corners share vertices, crease corners (same position, different
/// normal) are split.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct EvalMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals:   Vec<[f32; 3]>,
    pub uvs:       Vec<[f32; 2]>,
    pub indices:   Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct EntryStatus {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

/// Tolerance ladder for normalized boolean ops. Values are relative
/// to the *normalized* geometry, where the geometric mean of the two
/// operands' bounding-box diagonals is 1.0. The probe suite showed
/// shapeops 0.4's success landscape is jagged and proportion-
/// dependent (e.g. 0.005 succeeds where both 0.01 and 0.003 fail),
/// so the ladder is dense; failed rungs return fast, so the walk is
/// cheap relative to a successful op.
const BOOLEAN_TOLERANCE_LADDER: [f64; 6] = [0.005, 0.01, 0.002, 0.02, 0.05, 0.001];

/// Run a truck-shapeops binary op with **scale normalization**.
///
/// shapeops 0.4 has an absolute scale floor: identical geometry that
/// booleans fine at unit scale returns `None` at centimeter scale
/// (verified empirically in `tests/shapeops_probe.rs` — same
/// part/hole ratio, only the absolute size varied). Since the engine
/// is meter-native, real parts sit under that floor. Workaround:
/// uniformly scale both operands toward unit size, run the op, scale
/// the result back. The scale target is the *geometric mean* of the
/// two bounding-box diagonals — it balances a large base against a
/// small cut so both land near the unit-scale regime truck's own
/// examples run in (min-based normalization left a 10 mm-hole-in-a-
/// 40 mm-plate base at 4x while the cut sat at 1x, off the reliable
/// band). Hugely asymmetric construction bodies (Split's half-space
/// slab) pull the mean, so the scale is additionally clamped to keep
/// the smaller operand within sane bounds.
///
/// The result is returned at the caller's (meter) scale, but its
/// `IntersectionCurve` edges carry the composed transform — naively
/// evaluating their surfaces at meter scale diverges (and truck
/// panics on the internal unwrap). Every downstream surface-
/// evaluating op must therefore re-normalize first: booleans do (this
/// fn), and `tessellate_solid` does. Plain `builder::transformed`
/// (Mirror/Pattern) only composes matrices and is safe.
fn boolean_normalized<F>(a: &Solid, b: &Solid, op: F) -> Option<Solid>
where
    F: Fn(&Solid, &Solid, f64) -> Option<Solid>,
{
    let da = solid_bbox_diagonal(a);
    let db = solid_bbox_diagonal(b);
    let mean = (da * db).sqrt();
    let scale = if mean > 1.0e-12 {
        // Keep the smaller operand's normalized diagonal in [0.05, 20]
        // even when the operands are wildly different sizes.
        let s = 1.0 / mean;
        let d_min = da.min(db);
        s.clamp(0.05 / d_min.max(1.0e-12), 20.0 / d_min.max(1.0e-12))
    } else {
        1.0
    };
    let a = builder::transformed(a, Matrix4::from_scale(scale));
    let b = builder::transformed(b, Matrix4::from_scale(scale));
    for tol in BOOLEAN_TOLERANCE_LADDER {
        // truck-geometry `unwrap()`s Newton projections internally
        // (IntersectionCurve::subs), so a tolerance its numerics
        // can't handle PANICS rather than returning None. Catch and
        // treat as "this rung failed" — rayon propagates worker
        // panics to this thread, so catch_unwind sees them all.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            op(&a, &b, tol)
        }));
        if let Ok(Some(out)) = result {
            return Some(builder::transformed(&out, Matrix4::from_scale(1.0 / scale)));
        }
    }
    None
}

/// Bounding-box diagonal from topological vertices — corner points
/// only, but that's plenty for a scale estimate.
fn solid_bbox_diagonal(s: &Solid) -> f64 {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for shell in s.boundaries() {
        for v in shell.vertex_iter() {
            let p = v.point();
            let c = [p.x, p.y, p.z];
            for axis in 0..3 {
                min[axis] = min[axis].min(c[axis]);
                max[axis] = max[axis].max(c[axis]);
            }
        }
    }
    if min[0] > max[0] {
        return 1.0; // no vertices — fall back to unit scale
    }
    let dx = max[0] - min[0];
    let dy = max[1] - min[1];
    let dz = max[2] - min[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Axis-aligned bounds over the solid's topological vertices.
fn solid_bbox(s: &Solid) -> Option<(Point3, Point3)> {
    let mut min = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut max = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for shell in s.boundaries() {
        for v in shell.vertex_iter() {
            let p = v.point();
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            min.z = min.z.min(p.z);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
            max.z = max.z.max(p.z);
        }
    }
    (min.x <= max.x).then_some((min, max))
}

/// Sweep a profile sketch along a path sketch's line chain.
///
/// For each consecutive path segment, extrudes the profile along the
/// segment direction and unions the results. Profile is assumed to lie
/// in XY; each segment orients the profile so local Z aligns with the
/// segment direction (simple frame — no freeform Frenet twist).
fn sweep_profile_along_path(profile: &Sketch, path: &Sketch) -> CadResult<Solid> {
    let points = path_polyline_points(path)?;
    if points.len() < 2 {
        return Err(CadError::EvalFailed {
            feature: "Sweep".into(),
            reason: "path sketch needs ≥2 points (Line chain or Points)".into(),
        });
    }

    // Build a unit-depth profile solid along +Z, then reorient per segment.
    let unit_profile = extrude_sketch(profile, 1.0, false)?;
    let mut parts: Vec<Solid> = Vec::new();

    for w in points.windows(2) {
        let a = w[0];
        let b = w[1];
        let dir = b - a;
        let len = dir.magnitude();
        if len < 1e-9 {
            continue;
        }
        let axis = dir / len;
        // Rotation taking +Z to `axis`.
        let z = Vector3::unit_z();
        let rot = rotation_between(z, axis);
        let mat = Matrix4::from_translation(a.to_vec())
            * Matrix4::from(rot)
            * Matrix4::from_nonuniform_scale(1.0, 1.0, len);
        parts.push(builder::transformed(&unit_profile, mat));
    }

    if parts.is_empty() {
        return Err(CadError::EvalFailed {
            feature: "Sweep".into(),
            reason: "all path segments had zero length".into(),
        });
    }
    Ok(union_many(&parts).unwrap_or_else(|| parts[0].clone()))
}

fn path_polyline_points(path: &Sketch) -> CadResult<Vec<Point3>> {
    let mut pts: Vec<Point3> = Vec::new();
    for e in &path.entities {
        match e {
            SketchEntity::Line { p1, p2 } | SketchEntity::Construction { p1, p2 } => {
                let a = Point3::new(p1[0], p1[1], 0.0);
                let b = Point3::new(p2[0], p2[1], 0.0);
                if pts.last().map(|p| (*p - a).magnitude() > 1e-9).unwrap_or(true) {
                    pts.push(a);
                }
                pts.push(b);
            }
            SketchEntity::Point { p } => {
                pts.push(Point3::new(p[0], p[1], 0.0));
            }
            _ => {}
        }
    }
    if pts.len() < 2 {
        // Fall back: rectangle / circle path not supported for sweep path.
        return Err(CadError::EvalFailed {
            feature: "Sweep".into(),
            reason: "path must be Line/Point entities forming a polyline".into(),
        });
    }
    Ok(pts)
}

/// Rotation matrix mapping unit vector `from` → unit vector `to`.
fn rotation_between(from: Vector3, to: Vector3) -> Matrix3 {
    let f = from.normalize();
    let t = to.normalize();
    let cos = f.dot(t).clamp(-1.0, 1.0);
    if (cos - 1.0).abs() < 1e-9 {
        return Matrix3::one();
    }
    if (cos + 1.0).abs() < 1e-9 {
        // 180° — pick any perpendicular axis.
        let axis = if f.x.abs() < 0.9 {
            f.cross(Vector3::unit_x()).normalize()
        } else {
            f.cross(Vector3::unit_y()).normalize()
        };
        return Matrix3::from_axis_angle(axis, Rad(PI));
    }
    let axis = f.cross(t).normalize();
    let angle = cos.acos();
    Matrix3::from_axis_angle(axis, Rad(angle))
}

/// Z-extent over the solid's topological vertices — used by the Hole
/// arm to detect through cuts and span them. v0 holes always cut
/// along +z of the sketch plane, so z is the right axis until
/// sketch-plane transforms land.
fn solid_z_range(s: &Solid) -> (f64, f64) {
    s.boundaries()
        .iter()
        .flat_map(|shell| shell.vertex_iter())
        .map(|v| v.point().z)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
            (lo.min(z), hi.max(z))
        })
}

/// Walk the tree top-to-bottom, accumulating a running body.
///
/// Sketches with constraints / dimensions are solved via
/// [`crate::solver::solve_sketch`] before Extrude / Revolve / Sweep
/// consume them. Solve status is reported on the sketch entry.
pub fn evaluate_tree(tree: &FeatureTree) -> CadResult<EvalOutput> {
    let mut body: Option<Solid> = None;
    let mut feature_outputs: HashMap<String, Solid> = HashMap::new();
    let mut entry_status = Vec::with_capacity(tree.entries.len());
    // Max fillet/chamfer radius seen — drives mesh crease soften after tessellate.
    let mut mesh_round_radius: f64 = 0.0;

    // Own solved sketches so Extrude sees constrained geometry.
    let mut solved_sketches: HashMap<String, Sketch> = HashMap::new();

    for entry in &tree.entries {
        if entry.is_suppressed() {
            entry_status.push(EntryStatus {
                name: entry.name().to_string(),
                ok: true,
                message: "(suppressed)".to_string(),
            });
            continue;
        }
        match entry {
            FeatureEntry::Sketch { name, body: sk } => {
                let has_work = !sk.constraints.is_empty() || !sk.dimensions.is_empty();
                if has_work {
                    match crate::solver::solve_sketch(sk, &tree.variables) {
                        Ok(report) => {
                            let mut sk2 = sk.clone();
                            crate::solver::apply_solve(&mut sk2, &report);
                            let msg = format!(
                                "solved {:?} residual={:.2e} dof={} iters={}",
                                report.status,
                                report.residual_norm,
                                report.free_dof,
                                report.iterations
                            );
                            solved_sketches.insert(name.clone(), sk2);
                            entry_status.push(EntryStatus {
                                name: name.clone(),
                                ok: report.converged || report.residual_norm < 1e-3,
                                message: msg,
                            });
                        }
                        Err(e) => {
                            solved_sketches.insert(name.clone(), sk.clone());
                            entry_status.push(EntryStatus {
                                name: name.clone(),
                                ok: false,
                                message: format!("solver: {e}"),
                            });
                        }
                    }
                } else {
                    solved_sketches.insert(name.clone(), sk.clone());
                    entry_status.push(EntryStatus {
                        name: name.clone(),
                        ok: true,
                        message: "sketch loaded".to_string(),
                    });
                }
            }
            FeatureEntry::Feature { name, body: feature_body } => {
                // Build a map of references for evaluate_feature_into_body
                let sketch_refs: HashMap<String, &Sketch> = solved_sketches
                    .iter()
                    .map(|(k, v)| (k.clone(), v))
                    .collect();
                match evaluate_feature_into_body(
                    feature_body,
                    &sketch_refs,
                    body.as_ref(),
                    &feature_outputs,
                    &tree.variables,
                ) {
                    Ok(FeatureEvalResult::ReplacedBody { body: new_body, note, mesh_round }) => {
                        feature_outputs.insert(name.clone(), new_body.clone());
                        body = Some(new_body);
                        if let Some(r) = mesh_round {
                            mesh_round_radius = mesh_round_radius.max(r);
                        }
                        entry_status.push(EntryStatus {
                            name: name.clone(),
                            ok: true,
                            message: note.unwrap_or_else(|| "ok".to_string()),
                        });
                    }
                    Ok(FeatureEvalResult::NoBodyChange) => {
                        entry_status.push(EntryStatus {
                            name: name.clone(), ok: true,
                            message: "reference-only (no body change)".to_string(),
                        });
                    }
                    Err(e) => {
                        entry_status.push(EntryStatus {
                            name: name.clone(), ok: false,
                            message: e.to_string(),
                        });
                    }
                }
            }
            FeatureEntry::Suppressed { .. } => unreachable!(),
        }
    }

    let tolerance = tree
        .metadata
        .mesh_tolerance
        .unwrap_or(DEFAULT_MESH_TOLERANCE);
    let mut mesh = body.as_ref().map(|b| tessellate_solid(b, tolerance));
    // Mesh-edge fillet/chamfer interim: soften crease vertices when any
    // Fillet/Chamfer feature requested a radius. True BRep fillet waits
    // on truck-shapeops; this keeps the feature tree useful and visibly
    // rounds sharp edges in the viewport / GLB export.
    if mesh_round_radius > 1.0e-9 {
        if let Some(ref mut m) = mesh {
            soften_mesh_creases(m, mesh_round_radius as f32);
        }
    }
    Ok(EvalOutput { body, mesh, entry_status })
}

/// Per-entry result. `ReplacedBody` means the running body becomes
/// the contained `Solid`; `NoBodyChange` keeps the existing running
/// body untouched (used for ReferencePlane etc.).
enum FeatureEvalResult {
    ReplacedBody {
        body: Solid,
        note: Option<String>,
        /// When set, max crease-soften radius for post-tessellation.
        mesh_round: Option<f64>,
    },
    NoBodyChange,
}

impl FeatureEvalResult {
    fn body(body: Solid) -> Self {
        FeatureEvalResult::ReplacedBody {
            body,
            note: None,
            mesh_round: None,
        }
    }
}

fn evaluate_feature_into_body(
    feature: &Feature,
    sketches: &HashMap<String, &Sketch>,
    current: Option<&Solid>,
    prior_outputs: &HashMap<String, Solid>,
    vars: &HashMap<String, String>,
) -> CadResult<FeatureEvalResult> {
    use Feature::*;
    match feature {
        Extrude { sketch, depth, combine, both_sides, .. } => {
            let sk = sketches.get(sketch).copied()
                .ok_or_else(|| CadError::SketchNotFound(sketch.clone()))?;
            let depth_m = resolve_length_meters(depth, vars)?;
            let new_body = extrude_sketch(sk, depth_m, *both_sides)?;
            finish_combine(current, new_body, *combine)
        }
        Revolve { sketch, axis, angle, combine } => {
            let sk = sketches.get(sketch).copied()
                .ok_or_else(|| CadError::SketchNotFound(sketch.clone()))?;
            let angle_rad = resolve_angle_radians(angle, vars)?;
            let (origin, axis_dir) = resolve_axis(axis, sk)?;
            let new_body = revolve_sketch(sk, origin, axis_dir, angle_rad)?;
            finish_combine(current, new_body, *combine)
        }
        Hole {
            sketch_point, diameter, depth,
            counterbore_diameter, counterbore_depth,
            countersink_diameter, countersink_angle: _,
            tap_class: _,
        } => {
            let (sk_name, _point_ix) = parse_sketch_ref(sketch_point)?;
            let sk = sketches.get(&sk_name).copied()
                .ok_or_else(|| CadError::SketchNotFound(sk_name.clone()))?;
            // v0: treat the named sketch-point as a circle center in
            // the sketch's plane. For a 1-point sketch, take the
            // `Point` entity. If the user wants a non-origin hole,
            // they place the sketch on the target face + dimension the
            // point.
            let point = first_sketch_point(sk).unwrap_or([0.0, 0.0]);
            let radius = resolve_length_meters(diameter, vars)? * 0.5;
            let depth_m = resolve_length_meters(depth, vars)?;

            // Cut bodies must protrude past the faces they enter —
            // shapeops 0.4 booleans degenerate on coplanar/flush
            // faces (see `boolean_not`). Over-extend above the
            // sketch plane always, and out the bottom too when the
            // hole reaches the far face (a through hole).
            let overcut = (depth_m * 0.05).max(1.0e-4);
            let (part_z_min, part_z_max) = current.map(solid_z_range).unwrap_or((0.0, depth_m));
            let through = depth_m >= part_z_max - 1.0e-9;
            // Through cuts span the part's FULL z-range: a both_sides
            // base puts the sketch plane mid-part, so a cut measured
            // from the plane alone would leave the bottom half solid.
            let (cut_start, cut_len) = if through {
                (
                    part_z_min.min(0.0) - overcut,
                    (part_z_max - part_z_min.min(0.0)) + 2.0 * overcut,
                )
            } else {
                (-overcut, depth_m + overcut)
            };
            let body = extrude_circle(point, radius, cut_len, false)?;
            let mut cut_body = builder::translated(&body, Vector3::new(0.0, 0.0, cut_start));

            // Counterbore — wider shallow cylinder at the top.
            // Staggered overcut (2x) so its top face isn't coplanar
            // with the main cut's — that union would degenerate too.
            if let (Some(cb_d), Some(cb_depth)) = (counterbore_diameter, counterbore_depth) {
                let cb_r = resolve_length_meters(cb_d, vars)? * 0.5;
                let cb_depth_m = resolve_length_meters(cb_depth, vars)?;
                let cb_overcut = overcut * 2.0;
                let cb_body = extrude_circle(point, cb_r, cb_depth_m + cb_overcut, false)?;
                let cb_body = builder::translated(&cb_body, Vector3::new(0.0, 0.0, -cb_overcut));
                // Union onto the hole cylinder — the whole thing
                // subtracts from the current body below.
                cut_body = boolean_or(&cut_body, &cb_body).unwrap_or(cut_body);
            }
            // Countersink — conical widening. Approximated in v0 as a
            // larger cylinder at the top; true cone lands with a
            // Revolve-around-point path. Staggered overcut (3x).
            if let Some(csk_d) = countersink_diameter {
                let csk_r = resolve_length_meters(csk_d, vars)? * 0.5;
                let csk_overcut = overcut * 3.0;
                let csk_body = extrude_circle(point, csk_r, 0.005 + csk_overcut, false)?;
                let csk_body = builder::translated(&csk_body, Vector3::new(0.0, 0.0, -csk_overcut));
                cut_body = boolean_or(&cut_body, &csk_body).unwrap_or(cut_body);
            }

            // Always subtract — that's what a hole does.
            finish_combine(current, cut_body, crate::FeatureOp::Subtract)
        }
        Mirror { plane, features, combine } => {
            let reflected = mirror_bodies(plane, features, current, prior_outputs)?;
            finish_combine(current, reflected, *combine)
        }
        Pattern { kind, features, count, spacing, direction, axis, angle } => {
            let source = resolve_pattern_source(features, current, prior_outputs)?;
            // Pattern doesn't carry its own combine mode — instances
            // always union with the running body (same behaviour as a
            // sequence of identical Add features). When the user
            // wants Subtract/Intersect semantics they should mark
            // the *source* feature as such, not the Pattern.
            let combine = crate::FeatureOp::Add;
            let combined = match kind {
                crate::PatternKind::Linear => {
                    let dir = direction.unwrap_or([1.0, 0.0, 0.0]);
                    let step = match spacing {
                        Some(s) => resolve_length_meters(s, vars)?,
                        None => 1.0,
                    };
                    pattern_linear(&source, dir, step, *count)
                }
                crate::PatternKind::Circular => {
                    let axis_ref = axis.as_deref().unwrap_or("y");
                    let (origin, axis_dir) = resolve_world_axis(axis_ref);
                    let total = match angle {
                        Some(a) => resolve_angle_radians(a, vars)?,
                        None    => TAU,
                    };
                    pattern_circular(&source, origin, axis_dir, total, *count)
                }
                crate::PatternKind::Path => {
                    return Err(CadError::NotImplemented(
                        "Pattern::Path — awaits path-sketch resolver (lands with Sweep)".into()
                    ));
                }
                crate::PatternKind::Sketch => {
                    return Err(CadError::NotImplemented(
                        "Pattern::Sketch — sketch-point-driven patterns land after sketch solver".into()
                    ));
                }
            };
            finish_combine(current, combined, combine)
        }
        Boolean { target, boolean_op } => {
            let Some(target_body) = prior_outputs.get(target) else {
                return Err(CadError::EvalFailed {
                    feature: "Boolean".into(),
                    reason: format!("target feature '{}' not found", target),
                });
            };
            let Some(cur) = current else {
                return Err(CadError::EvalFailed {
                    feature: "Boolean".into(),
                    reason: "no running body to combine with".into(),
                });
            };
            let result = match boolean_op {
                crate::BooleanOp::Union      => boolean_or(cur, target_body),
                crate::BooleanOp::Difference => boolean_not(cur, target_body),
                crate::BooleanOp::Intersect  => boolean_and(cur, target_body),
            }.ok_or_else(|| CadError::Kernel(
                "boolean operation produced no result (non-manifold or disjoint)".into()
            ))?;
            Ok(FeatureEvalResult::body(result))
        }
        Split { plane } => {
            let Some(cur) = current else {
                return Err(CadError::EvalFailed {
                    feature: "Split".into(),
                    reason: "no running body to split".into(),
                });
            };
            let (origin, normal) = resolve_plane(plane)?;
            // Build a half-space slab large enough to fully contain
            // the body, then Difference with it → the "positive side"
            // body. The "negative side" gets reconstructed by a second
            // Split entry in follow-up work.
            let slab = build_halfspace_slab(origin, normal, 1000.0);
            let remaining = boolean_not(cur, &slab).ok_or_else(|| CadError::Kernel(
                "Split: boolean difference with half-space failed".into()
            ))?;
            Ok(FeatureEvalResult::body(remaining))
        }
        Fillet { edges, radius, .. } => {
            // truck-shapeops has no stable BRep fillet. Keep the solid
            // topology intact and flag a post-tessellation mesh crease
            // soften so edges round in the viewport / GLB. True BRep
            // fillet upgrades in place once truck lands the API.
            let r = resolve_length_meters(radius, vars)?;
            let Some(cur) = current else {
                return Err(CadError::EvalFailed {
                    feature: "Fillet".into(),
                    reason: "no body to fillet".into(),
                });
            };
            let n_edges = edges.len().max(1);
            Ok(FeatureEvalResult::ReplacedBody {
                body: cur.clone(),
                note: Some(format!(
                    "mesh-edge fillet r={r:.4}m ({n_edges} edge refs) — BRep pending"
                )),
                mesh_round: Some(r),
            })
        }
        Chamfer { edges, distance, .. } => {
            let d = resolve_length_meters(distance, vars)?;
            let Some(cur) = current else {
                return Err(CadError::EvalFailed {
                    feature: "Chamfer".into(),
                    reason: "no body to chamfer".into(),
                });
            };
            let n_edges = edges.len().max(1);
            // Mesh soften uses the same crease path as Fillet (visual
            // interim). True BRep chamfer lands with truck.
            Ok(FeatureEvalResult::ReplacedBody {
                body: cur.clone(),
                note: Some(format!(
                    "mesh-edge chamfer d={d:.4}m ({n_edges} edge refs) — BRep pending"
                )),
                mesh_round: Some(d),
            })
        }
        Shell { wall_thickness, open_faces: _ } => {
            let t = resolve_length_meters(wall_thickness, vars)?;
            let Some(cur) = current else {
                return Err(CadError::EvalFailed {
                    feature: "Shell".into(),
                    reason: "no body to shell".into(),
                });
            };
            // v0 shell is OPEN-TOP: the inner cut protrudes through the
            // +z face so shapeops has intersection curves to work with.
            // A fully-enclosed cavity (scaled inner body strictly inside)
            // produces NO intersection curves and shapeops 0.4 returns
            // None every time — the original scale-to-centroid shell
            // could never succeed. Per-face open_faces selection lands
            // with real offset surfaces.
            let Some((bmin, bmax)) = solid_bbox(cur) else {
                return Err(CadError::EvalFailed {
                    feature: "Shell".into(),
                    reason: "body has no vertices".into(),
                });
            };
            let dx = bmax.x - bmin.x;
            let dy = bmax.y - bmin.y;
            let dz = bmax.z - bmin.z;
            if t <= 0.0 || dx <= 2.0 * t + 1e-6 || dy <= 2.0 * t + 1e-6 || dz <= t + 1e-6 {
                return Err(CadError::EvalFailed {
                    feature: "Shell".into(),
                    reason: format!(
                        "wall {:.4}m too thick for body {:.4}×{:.4}×{:.4}m",
                        t, dx, dy, dz
                    ),
                });
            }
            let overcut = (dz * 0.05).max(1.0e-4);
            let sx = (dx - 2.0 * t) / dx;
            let sy = (dy - 2.0 * t) / dy;
            // Inner spans [bmin.z + t, bmax.z + overcut] — wall at the
            // bottom, protruding out the top.
            let sz = ((bmax.z + overcut) - (bmin.z + t)) / dz;
            let anchor = Vector3::new((bmin.x + bmax.x) * 0.5, (bmin.y + bmax.y) * 0.5, bmin.z);
            let m = Matrix4::from_translation(Vector3::new(0.0, 0.0, t))
                * Matrix4::from_translation(anchor)
                * Matrix4::from_nonuniform_scale(sx, sy, sz)
                * Matrix4::from_translation(-anchor);
            let inner = builder::transformed(cur, m);
            let shelled = boolean_not(cur, &inner).ok_or_else(|| CadError::Kernel(
                "Shell: boolean difference with inner body failed".into()
            ))?;
            Ok(FeatureEvalResult::ReplacedBody {
                body: shelled,
                note: Some(format!(
                    "open-top shell t={t:.4}m (per-face open_faces pending)"
                )),
                mesh_round: None,
            })
        }
        Sweep { profile, path, combine } => {
            let profile_sk = sketches.get(profile).copied()
                .ok_or_else(|| CadError::SketchNotFound(profile.clone()))?;
            let path_sk = sketches.get(path).copied()
                .ok_or_else(|| CadError::SketchNotFound(path.clone()))?;
            let new_body = sweep_profile_along_path(profile_sk, path_sk)?;
            finish_combine(current, new_body, *combine)
        }
        Loft { profiles, .. } => {
            Err(CadError::NotImplemented(format!(
                "Loft ({} profiles) — profile-interpolation routine pending; \
                 truck-modeling has `homotopy` for between-two surfaces but multi-profile \
                 lofting requires a guide-curve solver not yet in 0.6.",
                profiles.len()
            )))
        }
        ReferencePlane { .. } => Ok(FeatureEvalResult::NoBodyChange),
    }
}

// ============================================================================
// Extrude — supports Rectangle / Circle / closed-polyline profiles
// ============================================================================

fn extrude_sketch(sk: &Sketch, depth_m: f64, both_sides: bool) -> CadResult<Solid> {
    let face = build_planar_face(sk)?;
    let vec = Vector3::new(0.0, 0.0, if both_sides { depth_m } else { depth_m });
    // both_sides: translate the face down by half first, then sweep full depth.
    // Simpler: produce a solid centered on the sketch plane by building from the
    // negative half-face.
    let (face_use, vec_use) = if both_sides {
        let half = depth_m * 0.5;
        (
            builder::translated(&face, Vector3::new(0.0, 0.0, -half)),
            Vector3::new(0.0, 0.0, depth_m),
        )
    } else {
        (face, vec)
    };
    Ok(builder::tsweep(&face_use, vec_use))
}

fn extrude_circle(center: [f64; 2], radius: f64, depth_m: f64, both_sides: bool) -> CadResult<Solid> {
    // Build a circle wire by 3-point arc approximation (truck's
    // `circle_arc` needs a transit point; for a full circle we chain
    // two half-arcs).
    let cx = center[0];
    let cy = center[1];
    let v_r  = builder::vertex(Point3::new(cx + radius, cy, 0.0));
    let v_l  = builder::vertex(Point3::new(cx - radius, cy, 0.0));
    let top = Point3::new(cx, cy + radius, 0.0);
    let bot = Point3::new(cx, cy - radius, 0.0);
    let arc_upper = builder::circle_arc(&v_r, &v_l, top);
    let arc_lower = builder::circle_arc(&v_l, &v_r, bot);
    let wire: Wire = vec![arc_upper, arc_lower].into();
    let face = builder::try_attach_plane(&[wire]).map_err(|e| CadError::Kernel(e.to_string()))?;

    let (face_use, vec_use) = if both_sides {
        let half = depth_m * 0.5;
        (
            builder::translated(&face, Vector3::new(0.0, 0.0, -half)),
            Vector3::new(0.0, 0.0, depth_m),
        )
    } else {
        (face, Vector3::new(0.0, 0.0, depth_m))
    };
    Ok(builder::tsweep(&face_use, vec_use))
}

/// Build a planar Face from a sketch's entities. Supports three
/// profile shapes:
/// - exactly one `Rectangle` entity
/// - exactly one `Circle` entity
/// - a closed chain of `Line` entities forming a single loop
fn build_planar_face(sk: &Sketch) -> CadResult<Face> {
    // Rectangle profile.
    if let Some(SketchEntity::Rectangle { p1, p2 }) = sk.entities.iter()
        .find(|e| matches!(e, SketchEntity::Rectangle { .. }))
    {
        let (min_x, max_x) = (p1[0].min(p2[0]), p1[0].max(p2[0]));
        let (min_y, max_y) = (p1[1].min(p2[1]), p1[1].max(p2[1]));
        let v00 = builder::vertex(Point3::new(min_x, min_y, 0.0));
        let v10 = builder::vertex(Point3::new(max_x, min_y, 0.0));
        let v11 = builder::vertex(Point3::new(max_x, max_y, 0.0));
        let v01 = builder::vertex(Point3::new(min_x, max_y, 0.0));
        let wire: Wire = vec![
            builder::line(&v00, &v10),
            builder::line(&v10, &v11),
            builder::line(&v11, &v01),
            builder::line(&v01, &v00),
        ].into();
        return builder::try_attach_plane(&[wire]).map_err(|e| CadError::Kernel(e.to_string()));
    }

    // Circle profile.
    if let Some(SketchEntity::Circle { center, radius }) = sk.entities.iter()
        .find(|e| matches!(e, SketchEntity::Circle { .. }))
    {
        let v_r  = builder::vertex(Point3::new(center[0] + radius, center[1], 0.0));
        let v_l  = builder::vertex(Point3::new(center[0] - radius, center[1], 0.0));
        let top  = Point3::new(center[0], center[1] + radius, 0.0);
        let bot  = Point3::new(center[0], center[1] - radius, 0.0);
        let arc_upper = builder::circle_arc(&v_r, &v_l, top);
        let arc_lower = builder::circle_arc(&v_l, &v_r, bot);
        let wire: Wire = vec![arc_upper, arc_lower].into();
        return builder::try_attach_plane(&[wire]).map_err(|e| CadError::Kernel(e.to_string()));
    }

    // Closed polyline of `Line` entities.
    let lines: Vec<&SketchEntity> = sk.entities.iter()
        .filter(|e| matches!(e, SketchEntity::Line { .. }))
        .collect();
    if lines.len() >= 3 {
        // Chain by consecutive endpoint matching. Simple walker —
        // picks the first line, then finds the next line whose start
        // matches the previous line's end. Works for user-authored
        // polygons; doesn't handle self-intersecting or branching
        // chains (that's the sketch-solver's job anyway).
        let mut chain: Vec<Edge> = Vec::with_capacity(lines.len());
        let mut remaining: Vec<(usize, [f64; 2], [f64; 2])> = lines.iter().enumerate()
            .map(|(i, e)| match e {
                SketchEntity::Line { p1, p2 } => (i, *p1, *p2),
                _ => unreachable!(),
            }).collect();
        let (_, start, next_point) = remaining.remove(0);
        let v_start = builder::vertex(Point3::new(start[0], start[1], 0.0));
        let mut v_prev = v_start.clone();
        let mut next_target = next_point;
        let v_next = builder::vertex(Point3::new(next_point[0], next_point[1], 0.0));
        chain.push(builder::line(&v_prev, &v_next));
        v_prev = v_next;

        while !remaining.is_empty() {
            let pos = remaining.iter().position(|(_, p1, _)| {
                (p1[0] - next_target[0]).abs() < 1e-6 && (p1[1] - next_target[1]).abs() < 1e-6
            });
            let Some(ix) = pos else { break; };
            let (_, _, p2) = remaining.remove(ix);
            let lands_on_start =
                (p2[0] - start[0]).abs() < 1e-6 && (p2[1] - start[1]).abs() < 1e-6;
            if remaining.is_empty() && lands_on_start {
                // Closing edge must reuse the START vertex — truck
                // checks wire closure by vertex IDENTITY, not by
                // position, so a fresh vertex at the same coordinates
                // still yields "This wire is not closed".
                chain.push(builder::line(&v_prev, &v_start));
                next_target = start;
            } else {
                let v_new = builder::vertex(Point3::new(p2[0], p2[1], 0.0));
                chain.push(builder::line(&v_prev, &v_new));
                v_prev = v_new;
                next_target = p2;
            }
        }
        // Close the loop if we landed back at the starting point.
        let closes = (next_target[0] - start[0]).abs() < 1e-6
                  && (next_target[1] - start[1]).abs() < 1e-6;
        if !closes {
            return Err(CadError::EvalFailed {
                feature: "Extrude".into(),
                reason: "polyline profile isn't closed — chain doesn't return to start".into(),
            });
        }
        let wire: Wire = chain.into();
        return builder::try_attach_plane(&[wire]).map_err(|e| CadError::Kernel(e.to_string()));
    }

    Err(CadError::EvalFailed {
        feature: "Extrude".into(),
        reason: "sketch must contain exactly one Rectangle, one Circle, or a closed \
                 polyline of Line entities".into(),
    })
}

// ============================================================================
// Revolve
// ============================================================================

fn revolve_sketch(sk: &Sketch, origin: Point3, axis: Vector3, angle: f64) -> CadResult<Solid> {
    let face = build_planar_face(sk)?;
    Ok(builder::rsweep(&face, origin, axis, Rad(angle)))
}

// ============================================================================
// Mirror
// ============================================================================

fn mirror_bodies(
    plane: &str,
    features: &[String],
    current: Option<&Solid>,
    prior_outputs: &HashMap<String, Solid>,
) -> CadResult<Solid> {
    let (origin, normal) = resolve_plane(plane)?;
    let source = if features.is_empty() {
        current.ok_or_else(|| CadError::EvalFailed {
            feature: "Mirror".into(),
            reason: "no running body to mirror and no explicit feature list".into(),
        })?.clone()
    } else {
        // Union the listed features' output bodies, then mirror the union.
        let mut bodies: Vec<Solid> = Vec::new();
        for name in features {
            if let Some(b) = prior_outputs.get(name) {
                bodies.push(b.clone());
            }
        }
        if bodies.is_empty() {
            return Err(CadError::EvalFailed {
                feature: "Mirror".into(),
                reason: "no referenced features produced bodies".into(),
            });
        }
        union_many(&bodies).ok_or_else(|| CadError::Kernel(
            "Mirror: union of referenced features failed".into()
        ))?
    };

    // Reflection via an affine transform: `p' = p - 2 ((p - origin) · n) n`
    // truck's `builder::transformed` takes a 4x4 matrix. Compose a
    // reflection matrix.
    let mat = reflection_matrix(origin, normal);
    Ok(builder::transformed(&source, mat))
}

// ============================================================================
// Pattern
// ============================================================================

fn resolve_pattern_source(
    features: &[String],
    current: Option<&Solid>,
    prior_outputs: &HashMap<String, Solid>,
) -> CadResult<Solid> {
    if features.is_empty() {
        current.cloned().ok_or_else(|| CadError::EvalFailed {
            feature: "Pattern".into(),
            reason: "no running body and no features referenced".into(),
        })
    } else {
        let mut bodies: Vec<Solid> = Vec::new();
        for name in features {
            if let Some(b) = prior_outputs.get(name) {
                bodies.push(b.clone());
            }
        }
        union_many(&bodies).ok_or_else(|| CadError::EvalFailed {
            feature: "Pattern".into(),
            reason: "referenced features produced no bodies".into(),
        })
    }
}

fn pattern_linear(source: &Solid, dir: [f64; 3], step: f64, count: u32) -> Solid {
    let dir_vec = Vector3::new(dir[0], dir[1], dir[2]);
    let dir_norm = dir_vec.magnitude();
    let unit = if dir_norm > 1e-9 { dir_vec / dir_norm } else { Vector3::unit_x() };
    let mut copies = vec![source.clone()];
    for i in 1..count {
        let offset = unit * (step * i as f64);
        copies.push(builder::translated(source, offset));
    }
    union_many(&copies).unwrap_or_else(|| source.clone())
}

fn pattern_circular(
    source: &Solid,
    origin: Point3,
    axis: Vector3,
    total_angle: f64,
    count: u32,
) -> Solid {
    if count < 2 {
        return source.clone();
    }
    // Full-360 sweep wraps evenly; partial sweep distributes
    // endpoints exactly.
    let step = if (total_angle.abs() - TAU).abs() < 1e-6 {
        total_angle / count as f64
    } else {
        total_angle / (count - 1) as f64
    };
    let mut copies = vec![source.clone()];
    for i in 1..count {
        let theta = step * i as f64;
        copies.push(builder::rotated(source, origin, axis, Rad(theta)));
    }
    union_many(&copies).unwrap_or_else(|| source.clone())
}

// ============================================================================
// Split
// ============================================================================

fn build_halfspace_slab(origin: Point3, normal: Vector3, half_extent: f64) -> Solid {
    // Plane frame — pick any two orthonormal vectors perpendicular to `normal`.
    let n = normal.normalize();
    let up = if n.x.abs() < 0.9 { Vector3::unit_x() } else { Vector3::unit_y() };
    let tangent1 = up.cross(n).normalize();
    let tangent2 = n.cross(tangent1);

    let e = half_extent;
    // Build a square face on the plane centered on origin.
    let c00 = origin + (-tangent1 * e) + (-tangent2 * e);
    let c10 = origin + ( tangent1 * e) + (-tangent2 * e);
    let c11 = origin + ( tangent1 * e) + ( tangent2 * e);
    let c01 = origin + (-tangent1 * e) + ( tangent2 * e);

    let v00 = builder::vertex(c00);
    let v10 = builder::vertex(c10);
    let v11 = builder::vertex(c11);
    let v01 = builder::vertex(c01);
    let wire: Wire = vec![
        builder::line(&v00, &v10),
        builder::line(&v10, &v11),
        builder::line(&v11, &v01),
        builder::line(&v01, &v00),
    ].into();
    let face = builder::try_attach_plane(&[wire])
        .expect("half-space plane face construction should never fail");
    // Sweep along +normal for a large extent — that's the slab.
    builder::tsweep(&face, n * (half_extent * 2.0))
}

// ============================================================================
// Combine helpers + boolean ops
// ============================================================================

/// Apply `FeatureOp` to combine a feature's output body with the
/// running body.
fn finish_combine(
    current: Option<&Solid>,
    new_body: Solid,
    op: crate::FeatureOp,
) -> CadResult<FeatureEvalResult> {
    use crate::FeatureOp::*;
    let result = match op {
        NewBody => new_body,
        Add => match current {
            Some(cur) => boolean_or(cur, &new_body).unwrap_or_else(|| {
                // Fallback: if union fails, keep the new body as the
                // running result rather than silently dropping the
                // feature's work.
                new_body
            }),
            None => new_body,
        },
        Subtract => match current {
            Some(cur) => boolean_not(cur, &new_body).ok_or_else(|| CadError::Kernel(
                "Subtract: boolean difference failed".into()
            ))?,
            None => return Err(CadError::EvalFailed {
                feature: "Subtract".into(),
                reason: "no running body to subtract from".into(),
            }),
        },
        Intersect => match current {
            Some(cur) => boolean_and(cur, &new_body).ok_or_else(|| CadError::Kernel(
                "Intersect: boolean intersect failed".into()
            ))?,
            None => return Err(CadError::EvalFailed {
                feature: "Intersect".into(),
                reason: "no running body to intersect with".into(),
            }),
        },
    };
    Ok(FeatureEvalResult::body(result))
}

/// Union a slice of solids into one, via pairwise boolean-or.
/// Returns None if no bodies or if every union fails.
fn union_many(bodies: &[Solid]) -> Option<Solid> {
    let mut iter = bodies.iter();
    let mut acc = iter.next().cloned()?;
    for next in iter {
        acc = boolean_or(&acc, next).unwrap_or(acc);
    }
    Some(acc)
}

/// Boolean OR (union) via truck-shapeops. Thin layer so the
/// one-time swap to a newer truck version lands in a single spot.
/// `pub(crate)` so `parts_csg` can reuse the scale-normalized path.
pub(crate) fn boolean_or(a: &Solid, b: &Solid) -> Option<Solid> {
    boolean_normalized(a, b, |x, y, tol| truck_shapeops::or(x, y, tol))
}

pub(crate) fn boolean_and(a: &Solid, b: &Solid) -> Option<Solid> {
    boolean_normalized(a, b, |x, y, tol| truck_shapeops::and(x, y, tol))
}

/// Difference (A ∖ B) = A ∩ ¬B. truck-shapeops 0.4 exports only
/// `or` + `and`, but `truck_topology::Solid::not()` inverts face
/// orientation, turning the solid inside-out — this is exactly how
/// truck's own `punched-cube-shapeops` example computes difference.
///
/// Known limitation (shapeops 0.4): coplanar/flush faces between the
/// operands degenerate the intersection curve and the op returns
/// `None`. Cuts should protrude through the faces they enter (the
/// evaluator's Hole arm over-extends its cut body for this reason).
pub(crate) fn boolean_not(a: &Solid, b: &Solid) -> Option<Solid> {
    // Invert INSIDE the normalized op — i.e. after the rescale.
    // Scaling an already-inverted solid via builder::transformed
    // breaks shapeops (every tolerance panics in IntersectionCurve);
    // scale-then-invert matches truck's own example order and works.
    boolean_normalized(a, b, |x, y, tol| {
        let mut y_inverted = y.clone();
        y_inverted.not();
        truck_shapeops::and(x, &y_inverted, tol)
    })
}

// ============================================================================
// Reference resolution
// ============================================================================

fn resolve_plane(s: &str) -> CadResult<(Point3, Vector3)> {
    // v0: built-in planes by name. Feature-produced reference planes
    // resolve via a lookup into prior outputs — that path lands with
    // the full reference-tree plumbing.
    match s {
        "xy" => Ok((Point3::origin(), Vector3::unit_z())),
        "xz" => Ok((Point3::origin(), Vector3::unit_y())),
        "yz" => Ok((Point3::origin(), Vector3::unit_x())),
        other => Err(CadError::EvalFailed {
            feature: "plane lookup".into(),
            reason: format!("unsupported plane '{}' — v0 supports xy/xz/yz; \
                             feature-produced planes land with reference-tree plumbing", other),
        }),
    }
}

fn resolve_world_axis(s: &str) -> (Point3, Vector3) {
    match s {
        "x" => (Point3::origin(), Vector3::unit_x()),
        "z" => (Point3::origin(), Vector3::unit_z()),
        _   => (Point3::origin(), Vector3::unit_y()),
    }
}

fn resolve_axis(s: &str, _sk: &Sketch) -> CadResult<(Point3, Vector3)> {
    // v0: world axis names. Edge references like "Extrude1/edge-4"
    // land with the reference-tree plumbing alongside ReferencePlane.
    match s {
        "x" | "world/x" => Ok((Point3::origin(), Vector3::unit_x())),
        "y" | "world/y" => Ok((Point3::origin(), Vector3::unit_y())),
        "z" | "world/z" => Ok((Point3::origin(), Vector3::unit_z())),
        other => Err(CadError::EvalFailed {
            feature: "axis lookup".into(),
            reason: format!("unsupported axis '{}' — v0 supports x/y/z world axes; \
                             edge refs land with reference-tree plumbing", other),
        }),
    }
}

fn parse_sketch_ref(s: &str) -> CadResult<(String, String)> {
    // Format: "<sketch_name>/<entity_spec>" — e.g. "Sketch1/point-0".
    let (sk, ent) = s.split_once('/').ok_or_else(|| CadError::EvalFailed {
        feature: "sketch ref parse".into(),
        reason: format!("expected '<sketch>/<entity>', got '{}'", s),
    })?;
    Ok((sk.to_string(), ent.to_string()))
}

fn first_sketch_point(sk: &Sketch) -> Option<[f64; 2]> {
    sk.entities.iter().find_map(|e| match e {
        SketchEntity::Point { p } => Some(*p),
        _ => None,
    })
}

// ============================================================================
// Math helpers
// ============================================================================

fn reflection_matrix(origin: Point3, normal: Vector3) -> Matrix4 {
    let n = normal.normalize();
    // I - 2 n nᵀ in 3x3, then embed in homogeneous 4x4 with the
    // translation part set so the reflection is about the plane
    // through `origin`.
    let nx = n.x; let ny = n.y; let nz = n.z;
    let r00 = 1.0 - 2.0 * nx * nx;
    let r01 =       -2.0 * nx * ny;
    let r02 =       -2.0 * nx * nz;
    let r10 =       -2.0 * ny * nx;
    let r11 = 1.0 - 2.0 * ny * ny;
    let r12 =       -2.0 * ny * nz;
    let r20 =       -2.0 * nz * nx;
    let r21 =       -2.0 * nz * ny;
    let r22 = 1.0 - 2.0 * nz * nz;

    // Translation: t = origin - R * origin
    let ox = origin.x; let oy = origin.y; let oz = origin.z;
    let tx = ox - (r00 * ox + r01 * oy + r02 * oz);
    let ty = oy - (r10 * ox + r11 * oy + r12 * oz);
    let tz = oz - (r20 * ox + r21 * oy + r22 * oz);

    Matrix4::new(
        r00, r10, r20, 0.0,
        r01, r11, r21, 0.0,
        r02, r12, r22, 0.0,
        tx,  ty,  tz,  1.0,
    )
}

fn resolve_length_meters(s: &str, vars: &HashMap<String, String>) -> CadResult<f64> {
    let q = crate::feature_tree::resolve_quantity(s, vars)
        .ok_or_else(|| CadError::EvalFailed {
            feature: "length lookup".into(),
            reason: format!("could not resolve '{}' as a Quantity", s),
        })?;
    match q.unit {
        crate::Unit::Length(_) | crate::Unit::Scalar => Ok(q.to_si()),
        other => Err(CadError::UnitMismatch {
            expected: "length".into(),
            got: format!("{:?}", other),
        }),
    }
}

fn resolve_angle_radians(s: &str, vars: &HashMap<String, String>) -> CadResult<f64> {
    let q = crate::feature_tree::resolve_quantity(s, vars)
        .ok_or_else(|| CadError::EvalFailed {
            feature: "angle lookup".into(),
            reason: format!("could not resolve '{}' as a Quantity", s),
        })?;
    match q.unit {
        crate::Unit::Angle(_) => Ok(q.to_si()),
        crate::Unit::Scalar   => Ok(q.value * PI / 180.0), // bare numbers treated as degrees
        other => Err(CadError::UnitMismatch {
            expected: "angle".into(),
            got: format!("{:?}", other),
        }),
    }
}

// ============================================================================
// Tessellation — truck Solid → flat triangle arrays via truck-meshalgo
// ============================================================================

/// Default deviation tolerance for tessellation, in meters (the
/// engine is meter-native). 1 mm keeps curved surfaces crisp at
/// part scale without exploding triangle counts. Override per tree
/// via `metadata.mesh_tolerance`.
pub const DEFAULT_MESH_TOLERANCE: f64 = 0.001;

/// Tessellate a truck `Solid` into flat triangle arrays ready to lift
/// into a Bevy `Mesh` (engine side) or a glTF primitive (exporter).
/// The kernel crate stays Bevy-free — output is plain `f32` arrays.
///
/// `tolerance` is the maximum deviation between the true surface and
/// the triangle mesh, in model units (meters). Clamped to stay above
/// truck's internal epsilon, which `triangulation` panics below.
pub fn tessellate_solid(solid: &Solid, tolerance: f64) -> EvalMesh {
    use truck_meshalgo::filters::{NormalFilters, OptimizingFilter};
    use truck_meshalgo::tessellation::{MeshableShape, MeshedShape, RobustMeshableShape};

    // Normalize to unit scale before evaluating any surface — the
    // same absolute scale floor that breaks shapeops booleans on
    // meter-native parts (see `boolean_normalized`) makes truck's
    // Newton projections diverge (and panic) during triangulation
    // of boolean results. Tessellate at ~unit size, then emit
    // positions scaled back with plain f32 math — no truck
    // transform touches the output.
    let diagonal = solid_bbox_diagonal(solid);
    let scale = if diagonal > 1.0e-12 { 1.0 / diagonal } else { 1.0 };
    let solid = builder::transformed(solid, Matrix4::from_scale(scale));
    let tolerance = (tolerance * scale).max(1.0e-5);
    let inv_scale = (1.0 / scale) as f32;

    // Fast path requires boundary curves to ride exactly on their
    // surfaces. Boolean (shapeops) output can violate that within
    // tolerance — faces then come back `None` and would silently
    // drop, leaving holes. Detect and retry with the robust
    // (project-onto-surface) path before giving up on those faces.
    // Both paths run under catch_unwind: truck unwrap()s internal
    // Newton projections, and a kernel panic must never take down
    // the editor — degrade to an empty mesh instead.
    let Ok(mut poly) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut meshed = solid.triangulation(tolerance);
        if count_missing_faces(&meshed) > 0 {
            meshed = solid.robust_triangulation(tolerance);
        }
        meshed.to_polygon()
    })) else {
        return EvalMesh::default();
    };

    // Weld attributes duplicated along shared edges so the triangle
    // soup becomes a connected (closed, for the fast path) surface,
    // drop anything the weld orphaned, then fill missing normals
    // from face geometry. `false` = never overwrite the analytic
    // surface normals tessellation already produced.
    poly.put_together_same_attrs(truck_base::tolerance::TOLERANCE);
    poly.remove_degenerate_faces().remove_unused_attrs();
    poly.add_naive_normals(false);

    // Expand truck's per-corner (pos, uv, nor) index triples into a
    // single index space, deduplicating identical triples so shared
    // smooth-surface corners stay welded while crease corners (same
    // position, different normal) stay split.
    let attrs = poly.attributes();
    let mut remap: HashMap<(usize, Option<usize>, Option<usize>), u32> = HashMap::new();
    let mut out = EvalMesh::default();
    for tri in poly.faces().triangle_iter() {
        for v in tri {
            let key = (v.pos, v.uv, v.nor);
            let next = remap.len() as u32;
            let idx = *remap.entry(key).or_insert_with(|| {
                let p = attrs.positions[v.pos];
                out.positions.push([
                    p.x as f32 * inv_scale,
                    p.y as f32 * inv_scale,
                    p.z as f32 * inv_scale,
                ]);
                let n = match v.nor {
                    Some(i) => attrs.normals[i],
                    None => Vector3::new(0.0, 0.0, 0.0),
                };
                out.normals.push([n.x as f32, n.y as f32, n.z as f32]);
                let t = match v.uv {
                    Some(i) => attrs.uv_coords[i],
                    None => Vector2::new(0.0, 0.0),
                };
                out.uvs.push([t.x as f32, t.y as f32]);
                next
            });
            out.indices.push(idx);
        }
    }
    out
}

/// Soften sharp mesh creases as an interim Fillet/Chamfer.
///
/// For each geometric position that has multiple split-normals (a crease
/// after tessellation), pull the corner **inward** along the average
/// outward normal and blend the normals. Does not change triangle count —
/// pure attribute edit so colliders / GLB stay simple.
fn soften_mesh_creases(mesh: &mut EvalMesh, radius: f32) {
    if mesh.positions.is_empty() || radius <= 0.0 {
        return;
    }
    let n = mesh.positions.len();
    // Quantize positions for welding keys (1 µm bins).
    let key_of = |p: [f32; 3]| -> (i32, i32, i32) {
        (
            (p[0] * 1.0e6).round() as i32,
            (p[1] * 1.0e6).round() as i32,
            (p[2] * 1.0e6).round() as i32,
        )
    };
    let mut buckets: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
    for i in 0..n {
        buckets.entry(key_of(mesh.positions[i])).or_default().push(i);
    }

    // Estimate a safe max offset from mesh extent so tiny parts don't
    // invert and huge parts still get a visible round.
    let mut min_p = [f32::INFINITY; 3];
    let mut max_p = [f32::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for a in 0..3 {
            min_p[a] = min_p[a].min(p[a]);
            max_p[a] = max_p[a].max(p[a]);
        }
    }
    let extent = ((max_p[0] - min_p[0]).powi(2)
        + (max_p[1] - min_p[1]).powi(2)
        + (max_p[2] - min_p[2]).powi(2))
    .sqrt()
    .max(1.0e-6);
    let max_off = (radius.min(extent * 0.15)).max(0.0);

    let mut new_positions = mesh.positions.clone();
    let mut new_normals = mesh.normals.clone();
    if new_normals.len() != n {
        new_normals = vec![[0.0, 1.0, 0.0]; n];
    }

    for indices in buckets.values() {
        if indices.len() < 2 {
            continue;
        }
        // Average outward normal across crease splits.
        let mut avg = [0.0f32; 3];
        for &i in indices {
            let nn = new_normals[i];
            avg[0] += nn[0];
            avg[1] += nn[1];
            avg[2] += nn[2];
        }
        let len = (avg[0] * avg[0] + avg[1] * avg[1] + avg[2] * avg[2]).sqrt();
        if len < 1.0e-8 {
            continue;
        }
        avg[0] /= len;
        avg[1] /= len;
        avg[2] /= len;

        // Measure crease strength: min pairwise normal dot.
        let mut min_dot = 1.0f32;
        for (a, &ia) in indices.iter().enumerate() {
            for &ib in indices.iter().skip(a + 1) {
                let na = new_normals[ia];
                let nb = new_normals[ib];
                let d = na[0] * nb[0] + na[1] * nb[1] + na[2] * nb[2];
                min_dot = min_dot.min(d);
            }
        }
        // Only soften genuine creases (angle ≳ 25°).
        if min_dot > 0.9 {
            continue;
        }
        // Stronger creases get more of the radius.
        let strength = (1.0 - min_dot).clamp(0.0, 1.0);
        let off = max_off * strength * 0.55;
        if off <= 1.0e-9 {
            continue;
        }
        // Pull inward (opposite outward average) for convex corners.
        for &i in indices {
            new_positions[i][0] -= avg[0] * off;
            new_positions[i][1] -= avg[1] * off;
            new_positions[i][2] -= avg[2] * off;
            // Blend normal toward average for softer shading.
            let nn = new_normals[i];
            let mut blended = [
                nn[0] * 0.45 + avg[0] * 0.55,
                nn[1] * 0.45 + avg[1] * 0.55,
                nn[2] * 0.45 + avg[2] * 0.55,
            ];
            let bl = (blended[0] * blended[0]
                + blended[1] * blended[1]
                + blended[2] * blended[2])
                .sqrt()
                .max(1.0e-8);
            blended[0] /= bl;
            blended[1] /= bl;
            blended[2] /= bl;
            new_normals[i] = blended;
        }
    }
    mesh.positions = new_positions;
    mesh.normals = new_normals;
}

/// Count faces whose tessellation failed (`surface() == None`) in a
/// meshed shape — the trigger for the robust-triangulation retry.
fn count_missing_faces<P, C, S: Clone>(
    meshed: &truck_topology::Solid<P, C, Option<S>>,
) -> usize {
    meshed
        .boundaries()
        .iter()
        .flat_map(|shell| shell.face_iter())
        .filter(|face| face.surface().is_none())
        .count()
}

fn index_sketches(tree: &FeatureTree) -> HashMap<String, &Sketch> {
    let mut out = HashMap::new();
    for entry in &tree.entries {
        if let FeatureEntry::Sketch { name, body } = entry {
            out.insert(name.clone(), body);
        }
    }
    out
}

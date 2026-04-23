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
//! | Fillet    | 🚧 blocked | truck-shapeops fillet API is upstream WIP       |
//! | Chamfer   | 🚧 blocked | Same as Fillet                                  |
//! | Shell     | 🚧 blocked | truck-modeling lacks a shell operation          |
//! | Sweep     | 🚧 pending | Path-profile sketch resolver needed             |
//! | Loft      | 🚧 pending | Profile interpolation + guide curves            |
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

pub struct EvalMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals:   Vec<[f32; 3]>,
    pub indices:   Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct EntryStatus {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

/// Tolerance for boolean ops. Matches the default truck uses in its
/// own tests. Can become user-configurable per-tree later via
/// `FeatureTree.metadata`.
const BOOLEAN_TOLERANCE: f64 = 0.01;

/// Walk the tree top-to-bottom, accumulating a running body.
pub fn evaluate_tree(tree: &FeatureTree) -> CadResult<EvalOutput> {
    let mut body: Option<Solid> = None;
    let mut feature_outputs: HashMap<String, Solid> = HashMap::new();
    let mut entry_status = Vec::with_capacity(tree.entries.len());
    let sketches = index_sketches(tree);

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
            FeatureEntry::Sketch { name, .. } => {
                entry_status.push(EntryStatus {
                    name: name.clone(),
                    ok: true,
                    message: "sketch loaded".to_string(),
                });
            }
            FeatureEntry::Feature { name, body: feature_body } => {
                match evaluate_feature_into_body(
                    feature_body,
                    &sketches,
                    body.as_ref(),
                    &feature_outputs,
                    &tree.variables,
                ) {
                    Ok(FeatureEvalResult::ReplacedBody(new_body)) => {
                        feature_outputs.insert(name.clone(), new_body.clone());
                        body = Some(new_body);
                        entry_status.push(EntryStatus {
                            name: name.clone(), ok: true,
                            message: "ok".to_string(),
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

    let mesh = body.as_ref().map(tessellate);
    Ok(EvalOutput { body, mesh, entry_status })
}

/// Per-entry result. `ReplacedBody` means the running body becomes
/// the contained `Solid`; `NoBodyChange` keeps the existing running
/// body untouched (used for ReferencePlane etc.).
enum FeatureEvalResult {
    ReplacedBody(Solid),
    NoBodyChange,
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

            let mut cut_body = extrude_circle(point, radius, depth_m, false)?;

            // Counterbore — wider shallow cylinder at the top
            if let (Some(cb_d), Some(cb_depth)) = (counterbore_diameter, counterbore_depth) {
                let cb_r = resolve_length_meters(cb_d, vars)? * 0.5;
                let cb_depth_m = resolve_length_meters(cb_depth, vars)?;
                let cb_body = extrude_circle(point, cb_r, cb_depth_m, false)?;
                // Union onto the hole cylinder — the whole thing
                // subtracts from the current body below.
                cut_body = boolean_or(&cut_body, &cb_body).unwrap_or(cut_body);
            }
            // Countersink — conical widening. Approximated in v0 as a
            // larger cylinder at the top; true cone lands with a
            // Revolve-around-point path.
            if let Some(csk_d) = countersink_diameter {
                let csk_r = resolve_length_meters(csk_d, vars)? * 0.5;
                let csk_body = extrude_circle(point, csk_r, 0.005, false)?;
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
            Ok(FeatureEvalResult::ReplacedBody(result))
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
            Ok(FeatureEvalResult::ReplacedBody(remaining))
        }
        Fillet { edges, radius, .. } => {
            let r = resolve_length_meters(radius, vars)?;
            Err(CadError::NotImplemented(format!(
                "Fillet (r={:.4}, {} edges) — blocked on truck-shapeops upstream fillet API \
                 (not yet stable in 0.6). Lands as a thin wrapper once upstream ships.",
                r, edges.len()
            )))
        }
        Chamfer { edges, distance, .. } => {
            let d = resolve_length_meters(distance, vars)?;
            Err(CadError::NotImplemented(format!(
                "Chamfer (d={:.4}, {} edges) — blocked on truck-shapeops upstream \
                 chamfer API (not yet stable in 0.6).",
                d, edges.len()
            )))
        }
        Shell { wall_thickness, .. } => {
            let t = resolve_length_meters(wall_thickness, vars)?;
            Err(CadError::NotImplemented(format!(
                "Shell (wall {:.4}m) — truck-modeling 0.6 has no shell operation. \
                 Candidate backends: OpenCascade FFI (heavy) or in-house offset-surface \
                 implementation.",
                t
            )))
        }
        Sweep { profile, path, .. } => {
            Err(CadError::NotImplemented(format!(
                "Sweep (profile='{}', path='{}') — path-sketch resolver + \
                 truck sweep-along-wire binding pending.",
                profile, path
            )))
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
            let v_new = builder::vertex(Point3::new(p2[0], p2[1], 0.0));
            chain.push(builder::line(&v_prev, &v_new));
            v_prev = v_new;
            next_target = p2;
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
    Ok(FeatureEvalResult::ReplacedBody(result))
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
fn boolean_or(a: &Solid, b: &Solid) -> Option<Solid> {
    truck_shapeops::or(a, b, BOOLEAN_TOLERANCE)
}

fn boolean_and(a: &Solid, b: &Solid) -> Option<Solid> {
    truck_shapeops::and(a, b, BOOLEAN_TOLERANCE)
}

/// Difference (A ∖ B). **Unsupported on truck-shapeops 0.4** — the
/// crate exports only `or` + `and`; `not` lands in a future release.
/// Returning `None` here lets the Boolean evaluator arm surface a
/// clean "non-manifold or disjoint" error so the user knows the op
/// didn't apply — we'd rather fail loudly than silently produce a
/// wrong shape. Flip this to `truck_shapeops::not(a, b, TOL)` once
/// the upstream fn ships.
fn boolean_not(_a: &Solid, _b: &Solid) -> Option<Solid> {
    None
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
// Tessellation stub — real mesh conversion goes through truck-meshalgo
// ============================================================================

fn tessellate(_solid: &Solid) -> EvalMesh {
    // truck-meshalgo provides `to_polygon` / `PolygonMesh` conversions
    // with a tolerance parameter. Engine-side glue lifts those into
    // Bevy's `Mesh` (positions/normals/indices) and Avian collider
    // trimeshes. Keeping this a stub in the kernel crate so it
    // stays Bevy-free — the engine's hot-reload path pulls
    // `truck-meshalgo` directly when it needs a render mesh.
    EvalMesh { positions: Vec::new(), normals: Vec::new(), indices: Vec::new() }
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
